// SPDX-License-Identifier: MIT
//! Function pointers: from an address of the original's code to the Rust that stands for it.
//!
//! Nothing here reproduces a function of the original.
//!
//! The original stores code addresses in records and tables (a gadget's handler, a menu
//! entry's action, the comparison function handed to `qsort`, the drawing routine chosen at
//! start-up) and calls through them. In the image such a field holds the same number, a
//! [`FnPtr`]: the address of the function in the Linux binary's `.text`. That keeps the
//! tables comparable with the original's byte for byte, and lets a record point to a
//! function nobody has ported yet. To call through one, the port asks a [`Registry`].
//!
//! The registry is deliberately small: a map from address to a handler with one uniform
//! shape, the original's own calling convention (cdecl: every argument a 32-bit word on
//! the stack, a `double` two words, a structure by value its words; the result in `eax`,
//! `edx:eax` or the x87 register). The type `C` is whatever the handlers need besides the
//! image (the crate's outside world; one day the program). The finding (0024) lists what
//! wiring the whole program needs beyond this.

use crate::{Access, FnPtr, Mem, TEXT};
use std::collections::BTreeMap;

/// What a called function leaves behind for its caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ret {
    /// `eax`: an integer or pointer result (the low half of a 64-bit one).
    pub eax: u32,
    /// `edx`: the high half of a `long long` result.
    pub edx: u32,
    /// The x87 register a `float` or `double` function leaves: the ten bytes of the 80-bit
    /// value in memory order, unrounded. `None` for a function that returns no such value.
    pub st0: Option<[u8; 10]>,
}

impl Ret {
    pub const NONE: Ret = Ret { eax: 0, edx: 0, st0: None };

    pub fn eax(v: u32) -> Ret {
        Ret { eax: v, ..Ret::NONE }
    }
    pub fn int(v: i32) -> Ret {
        Ret::eax(v as u32)
    }
    pub fn long_long(v: u64) -> Ret {
        Ret { eax: v as u32, edx: (v >> 32) as u32, st0: None }
    }
    pub fn st0(register: [u8; 10]) -> Ret {
        Ret { st0: Some(register), ..Ret::NONE }
    }
}

/// The Rust that stands for one function of the original: the context, the image, and the
/// argument words in C order. The context may be a trait object (`dyn` [`crate::Outside`]).
pub type Handler<C> = fn(&mut C, &mut Mem, &[u32]) -> Ret;

/// How a call through a pointer came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Called {
    /// A registered function ran.
    Ran(Ret),
    /// The address is a function of the original that nobody registered: a gap in the
    /// port, not a fault of the original. The call is noted ([`Registry::unported`]).
    Unported,
    /// The address is null or not in the code: the original would have been killed. A
    /// fault is recorded in the image ([`Access::Call`]).
    Fault,
}

impl Called {
    /// The result, with all zeros where no function ran (the defined answer for an address
    /// nobody registered).
    pub fn ret(self) -> Ret {
        match self {
            Called::Ran(r) => r,
            _ => Ret::NONE,
        }
    }
}

/// From the original's code addresses to handlers.
pub struct Registry<C: ?Sized> {
    map: BTreeMap<u32, (&'static str, Handler<C>)>,
    unported: Vec<u32>,
}

impl<C: ?Sized> Default for Registry<C> {
    fn default() -> Self {
        Registry { map: BTreeMap::new(), unported: Vec::new() }
    }
}

impl<C: ?Sized> Registry<C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `handler` stands for the function `name` at `addr`. The address must lie in the
    /// binary's `.text`, and be registered once: both are mistakes of the crate, not of
    /// data, so they panic.
    pub fn register(&mut self, addr: u32, name: &'static str, handler: Handler<C>) {
        assert!(is_code(addr), "{name}: {addr:#010x} is not in .text");
        if let Some((old, _)) = self.map.insert(addr, (name, handler)) {
            panic!("{addr:#010x} registered twice: {old} and {name}");
        }
    }

    /// The handler for an address, if one is registered. A handler is a plain function
    /// pointer: copy it out, then call it with the context (which may own this registry).
    pub fn get(&self, f: FnPtr) -> Option<Handler<C>> {
        self.map.get(&f.0).map(|e| e.1)
    }

    /// The original's name of a registered address.
    pub fn name(&self, f: FnPtr) -> Option<&'static str> {
        self.map.get(&f.0).map(|e| e.0)
    }

    /// The address a name was registered under: for code that stores a function pointer
    /// in a record.
    pub fn addr_of(&self, name: &str) -> Option<FnPtr> {
        self.map.iter().find(|(_, e)| e.0 == name).map(|(a, _)| FnPtr::new(*a))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// What to do for a call through `f`: the handler, or what went wrong, recorded. Use
    /// this when the context owns the registry; otherwise [`Registry::call`].
    pub fn resolve(&mut self, m: &Mem, f: FnPtr) -> Result<Handler<C>, Called> {
        if let Some(h) = self.get(f) {
            return Ok(h);
        }
        if is_code(f.0) {
            self.unported.push(f.0);
            Err(Called::Unported)
        } else {
            m.record_fault(f.0, 0, Access::Call);
            Err(Called::Fault)
        }
    }

    /// Calls through `f` with a context that does not own this registry.
    pub fn call(&mut self, ctx: &mut C, m: &mut Mem, f: FnPtr, args: &[u32]) -> Called {
        match self.resolve(m, f) {
            Ok(h) => Called::Ran(h(ctx, m, args)),
            Err(c) => c,
        }
    }

    /// The code addresses that were called and are not registered, in order.
    pub fn unported(&self) -> &[u32] {
        &self.unported
    }
}

/// True for an address inside the Linux binary's `.text`.
pub fn is_code(addr: u32) -> bool {
    addr.wrapping_sub(TEXT.0) < TEXT.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ptr32, BSS};

    struct World {
        log: Vec<String>,
    }

    // stands for a handler of the original that takes (record *, short)
    fn handler_a(w: &mut World, m: &mut Mem, args: &[u32]) -> Ret {
        w.log.push(format!("a({:#x}, {})", args[0], args[1] as i16));
        m.set_i16(args[0], args[1] as i16);
        Ret::int(-1)
    }

    #[test]
    fn a_table_of_handlers() {
        let mut m = Mem::new();
        let mut reg: Registry<World> = Registry::new();
        reg.register(0x083d_2000, "inTitleCB", handler_a);
        let mut w = World { log: Vec::new() };
        // the original's table holds code addresses; so does the image
        let table: Ptr32<[FnPtr; 3]> = Ptr32::new(BSS.0 + 0x100);
        m[table] = [reg.addr_of("inTitleCB").unwrap(), FnPtr::new(0x083d_3000), FnPtr::NULL];
        assert_eq!(m.u32(BSS.0 + 0x100), 0x083d_2000);

        let f = m[table][0];
        assert_eq!(reg.call(&mut w, &mut m, f, &[BSS.0, 0xffff_fffe]), Called::Ran(Ret::int(-1)));
        assert_eq!((m.i16(BSS.0), w.log.as_slice()), (-2, &["a(0x8780c80, -2)".to_string()][..]));
        assert_eq!(reg.name(f), Some("inTitleCB"));

        // a function of the original nobody ported: noted, answers zeros, no fault
        let f = m[table][1];
        let r = reg.call(&mut w, &mut m, f, &[]);
        assert_eq!((r, r.ret()), (Called::Unported, Ret::NONE));
        assert_eq!(reg.unported(), &[0x083d_3000]);
        assert!(m.fault().is_none());

        // a null function pointer: the original dies
        let f = m[table][2];
        assert_eq!(reg.call(&mut w, &mut m, f, &[]), Called::Fault);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((0, Access::Call)));
    }

    #[test]
    fn results() {
        assert_eq!(Ret::long_long(0x1_0000_0002), Ret { eax: 2, edx: 1, st0: None });
        assert_eq!(Ret::st0([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]).st0.unwrap()[9], 0x3f);
        assert!(is_code(0x080c_51b0) && !is_code(0x0864_4c60) && !is_code(0));
    }
}
