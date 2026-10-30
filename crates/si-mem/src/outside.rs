// SPDX-License-Identifier: MIT
//! What a ported function takes besides the image: the rest of the program.
//!
//! Nothing here reproduces a function of the original. **This module is a proposal built
//! as far as it can be without changing a client** (finding 0024, section 11.5): no crate
//! uses it yet, and the lead decides whether one does.
//!
//! A ported function works on the image and makes the original's calls. Where a callee is
//! on the image too it is called directly. Where it is not (another package that is not
//! ported, the graphics layer, the C library's services that are not memory) the call
//! has to go somewhere, and today every crate has a trait of its own for that, so that a
//! function of one crate cannot hand its outside world to a function of another.
//!
//! The shape proposed is the one two crates arrived at separately (`si-dict`,
//! `si-rensetup`'s `native` modules): **one trait with one method, the original's own way
//! of calling.** A callee is named by its address in the binary ([`Routine`]); the
//! arguments are the 32-bit words the caller pushes, in C order; the answer is what the
//! callee leaves in the registers ([`Ret`]). That is the calling convention of
//! [`Registry`], so the three things a call can meet are one implementation each:
//!
//! - a ported function, registered by address: [`Wired`] runs it, with itself as that
//!   function's outside world, so calls go on from crate to crate;
//! - a routine nobody ported: whatever [`Wired`] was given as the rest, by default a
//!   [`Trace`] that notes the call and answers zeros;
//! - a check against the emulated original: a [`Trace`] whose answers are the ones the
//!   emulator recorded, and whose list of calls is compared with the emulator's.
//!
//! A ported function is then `fn f(m: &mut Mem, o: &mut dyn Outside, …the original's
//! arguments…)`, and the typed names the code calls are an extension of the one trait,
//! declared with [`outside_calls!`](crate::outside_calls): one line per callee with its
//! arguments, its name and its address.

use crate::{FnPtr, Mem, Ptr32, Registry, Ret};

/// A routine of the original as a call names it: its address in the Linux binary's
/// `.text` (for a routine of the C library: of the binary's own stub), and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

impl Routine {
    pub const fn new(name: &'static str, addr: u32) -> Routine {
        Routine { name, addr }
    }

    /// A routine known by its address only: a function pointer read from a record.
    pub const fn at(addr: u32) -> Routine {
        Routine { name: "", addr }
    }

    pub const fn ptr(self) -> FnPtr {
        FnPtr::new(self.addr)
    }
}

/// The rest of the program, as a ported function calls it.
pub trait Outside {
    /// Calls `routine` with the stack words `args` (C order: every argument a 32-bit
    /// word, a `float` its bits, a `double` two words, a structure by value its words)
    /// and answers what it leaves in the registers. The callee works on the same image:
    /// a pointer argument is an address in it, an output argument the address of a local
    /// the caller took from the image's stack ([`Mem::local`]).
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret;

    /// What the program prints: text already formatted, for stream 1 (`stdout`) or 2
    /// (`stderr`). Dropped by default. (Formatted output is variadic and has no words to
    /// compare; a check compares the text.)
    fn print(&mut self, _stream: u32, _text: &[u8]) {}
}

/// The context type of the proposal, for a [`Registry`] of ported functions: a handler
/// is `fn(&mut World, &mut Mem, &[u32]) -> Ret`.
pub type World = dyn Outside;

/// One recorded call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub routine: Routine,
    pub args: Vec<u32>,
}

/// How a [`Trace`] answers: it may read and write the image (to play the callee's side
/// effects) and returns the callee's result.
pub type Responder = Box<dyn FnMut(&mut Mem, Routine, &[u32]) -> Ret>;

/// An outside world that writes down every call and answers through a closure: for unit
/// tests, for routines nobody ported, and for a check that replays what the emulated
/// original's callees answered.
pub struct Trace {
    pub calls: Vec<Call>,
    pub printed: Vec<(u32, Vec<u8>)>,
    pub respond: Responder,
}

impl Default for Trace {
    fn default() -> Self {
        Trace::new()
    }
}

impl Trace {
    /// Records, and answers zeros to everything.
    pub fn new() -> Trace {
        Trace::with(|_, _, _| Ret::NONE)
    }

    pub fn with(respond: impl FnMut(&mut Mem, Routine, &[u32]) -> Ret + 'static) -> Trace {
        Trace { calls: Vec::new(), printed: Vec::new(), respond: Box::new(respond) }
    }

    /// The names of the recorded calls, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.calls.iter().map(|c| c.routine.name).collect()
    }
}

impl Outside for Trace {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        self.calls.push(Call { routine, args: args.to_vec() });
        (self.respond)(m, routine, args)
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.printed.push((stream, text.to_vec()));
    }
}

/// Ported functions in front of the rest: a call to a registered address runs the port,
/// with this whole value as the port's own outside world; any other call goes to `rest`.
/// As packages are ported their functions are registered and stop reaching `rest`.
pub struct Wired {
    pub registry: Registry<World>,
    pub rest: Box<World>,
}

impl Wired {
    /// Nothing registered; what is not registered is noted and answered with zeros.
    pub fn new() -> Wired {
        Wired { registry: Registry::new(), rest: Box::new(Trace::new()) }
    }

    pub fn over(rest: impl Outside + 'static) -> Wired {
        Wired { registry: Registry::new(), rest: Box::new(rest) }
    }
}

impl Default for Wired {
    fn default() -> Self {
        Wired::new()
    }
}

impl Outside for Wired {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        match self.registry.get(routine.ptr()) {
            Some(port) => port(self, m, args),
            None => self.rest.call(m, routine, args),
        }
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.rest.print(stream, text)
    }
}

/// A registry whose handlers have a context of their own, as an outside world: every
/// call is looked up by its address; one nobody registered answers zeros and is noted in
/// the registry's list of gaps. (The form `si-rensetup`'s `native` modules use.)
pub struct Registered<'a, C: ?Sized> {
    pub registry: &'a mut Registry<C>,
    pub ctx: &'a mut C,
}

impl<C: ?Sized> Outside for Registered<'_, C> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        self.registry.call(self.ctx, m, routine.ptr(), args).ret()
    }
}

/// An argument as the stack words the original pushes for it.
pub trait Words {
    fn push(&self, words: &mut Vec<u32>);
}

macro_rules! words {
    ($($t:ty),*) => { $( impl Words for $t {
        /// Widened to the stack word as C widens it (a signed type with its sign).
        fn push(&self, words: &mut Vec<u32>) { words.push(*self as i32 as u32) }
    } )* };
}
words!(i8, i16, i32, u8, u16, bool);

impl Words for u32 {
    fn push(&self, words: &mut Vec<u32>) {
        words.push(*self)
    }
}
impl Words for f32 {
    /// A `float` argument of a prototyped function: its four bytes.
    fn push(&self, words: &mut Vec<u32>) {
        words.push(self.to_bits())
    }
}
impl Words for f64 {
    /// A `double`: the low word first.
    fn push(&self, words: &mut Vec<u32>) {
        let bits = self.to_bits();
        words.extend([bits as u32, (bits >> 32) as u32]);
    }
}
impl<T: ?Sized> Words for Ptr32<T> {
    fn push(&self, words: &mut Vec<u32>) {
        words.push(self.0)
    }
}
impl<const N: usize> Words for [u32; N] {
    /// A structure passed by value: its words.
    fn push(&self, words: &mut Vec<u32>) {
        words.extend(self)
    }
}

/// A result as the caller reads it out of the registers.
pub trait FromRet {
    fn from_ret(r: Ret) -> Self;
}

macro_rules! from_ret {
    ($($t:ty),*) => { $( impl FromRet for $t {
        /// The low bytes of `eax` (a `short` result is its low word).
        fn from_ret(r: Ret) -> $t { r.eax as $t }
    } )* };
}
from_ret!(i8, i16, i32, u8, u16, u32);

impl FromRet for () {
    fn from_ret(_: Ret) {}
}
impl FromRet for bool {
    fn from_ret(r: Ret) -> bool {
        r.eax != 0
    }
}
impl<T: ?Sized> FromRet for Ptr32<T> {
    fn from_ret(r: Ret) -> Ptr32<T> {
        Ptr32::new(r.eax)
    }
}
impl FromRet for Ret {
    /// Everything: for a `float` or `double` result, whose 80-bit register the caller's
    /// crate reads with the arithmetic crate's type.
    fn from_ret(r: Ret) -> Ret {
        r
    }
}

/// Declares the callees of a source file as methods on every outside world: one line each
/// with the arguments the original passes, the result it answers, its name and its
/// address.
///
/// ```
/// use si_mem::{outside_calls, Mem, Outside, Ptr32, Ret, Trace};
///
/// outside_calls! {
///     /// What `elemdict.c` calls in other files (two of them).
///     pub trait DictCalls {
///         /// `dbloadsave.c`: whether `SI_LOAD_OPTIM` was in the environment.
///         fn si_get_load_optim() -> i16 = "SI_getLoadOptim", 0x080d94b0;
///         /// `edtools.c`: saves one model element's relations.
///         fn si_ed_mdl_rel_back_up(ed: Ptr32<u8>, elem: Ptr32<u8>, index: i32) = "SI_edMdlRelBackUp", 0x08105d7c;
///     }
/// }
///
/// // a ported function: the image, the outside world, the original's arguments
/// fn add_elem(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<u8>, elem: Ptr32<u8>) -> i16 {
///     o.si_ed_mdl_rel_back_up(m, ed, elem, -1);
///     o.si_get_load_optim(m)
/// }
///
/// let mut m = Mem::new();
/// let mut o = Trace::with(|_, routine, _| Ret::int((routine.name == "SI_getLoadOptim") as i32));
/// assert_eq!(add_elem(&mut m, &mut o, Ptr32::new(0x6100_0000), Ptr32::new(0x6100_0040)), 1);
/// assert_eq!(o.names(), ["SI_edMdlRelBackUp", "SI_getLoadOptim"]);
/// assert_eq!(o.calls[0].args, [0x6100_0000, 0x6100_0040, 0xffff_ffff]);
/// assert_eq!(<dyn Outside as DictCalls>::ROUTINES[1].addr, 0x08105d7c);
/// ```
#[macro_export]
macro_rules! outside_calls {
    (
        $(#[$tm:meta])*
        pub trait $T:ident {
            $( $(#[$fm:meta])* fn $f:ident ( $($a:ident : $t:ty),* $(,)? ) $(-> $r:ty)? = $name:literal, $addr:literal ; )*
        }
    ) => {
        $(#[$tm])*
        pub trait $T: $crate::Outside {
            /// Name and address of every routine declared here.
            const ROUTINES: &'static [$crate::Routine] = &[ $( $crate::Routine::new($name, $addr) ),* ];
            $(
                $(#[$fm])*
                #[allow(clippy::too_many_arguments, unused_mut)]
                fn $f(&mut self, m: &mut $crate::Mem, $($a: $t),*) $(-> $r)? {
                    let mut words = ::std::vec::Vec::new();
                    $( $crate::outside::Words::push(&$a, &mut words); )*
                    $crate::outside::FromRet::from_ret($crate::Outside::call(self, m, $crate::Routine::new($name, $addr), &words))
                }
            )*
        }
        impl<O: $crate::Outside + ?Sized> $T for O {}
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Called, BSS};

    crate::outside_calls! {
        /// Made-up callees of three packages.
        pub trait DemoCalls {
            fn model_update(model: Ptr32<u8>, flag: i16) = "SI_modelUpdate", 0x0847_0000;
            fn is_constrained(obj: u32, time: f32, weight: f64) -> bool = "SI_edIsItemConstrained", 0x080f_0000;
            fn length(obj: u32) -> Ret = "SI_modelLength", 0x0848_0000;
            fn count(dict: Ptr32<u8>, rect: [u32; 2]) -> i16 = "SI_elemdictCount", 0x0813_0000;
        }
    }

    /// A "ported" function of one package: the image, the outside world, the arguments.
    fn refresh(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<u8>) -> i16 {
        m.set_u8(model.addr(), 1);
        o.model_update(m, model, -2);
        if o.is_constrained(m, model.addr(), 0.5, -2.0) {
            return -1;
        }
        o.count(m, model.cast(), [3, 4]) + m.i16(BSS.0)
    }

    /// One of another package, which calls the first through the outside world.
    fn count(m: &mut Mem, o: &mut dyn Outside, dict: Ptr32<u8>) -> i16 {
        m[Ptr32::<i16>::new(BSS.0)] += 1;
        let r: Ret = o.length(m, dict.addr());
        r.eax as i16 + 40
    }

    #[test]
    fn calls_are_words_at_addresses() {
        let mut m = Mem::new();
        let mut o = Trace::with(|m, routine, args| match routine.name {
            // a callee writes through its pointer argument, as the original's would
            "SI_modelUpdate" => {
                m.set_u8(args[0] + 1, 2);
                Ret::NONE
            }
            "SI_elemdictCount" => Ret::eax(0xffff_0007),
            _ => Ret::NONE,
        });
        let model = Ptr32::new(m.place(8, 8));
        assert_eq!(refresh(&mut m, &mut o, model), 7); // a short result: the low word
        assert_eq!(m.bytes(model.addr(), 2), &[1, 2]);
        assert_eq!(o.names(), ["SI_modelUpdate", "SI_edIsItemConstrained", "SI_elemdictCount"]);
        assert_eq!(o.calls[0], Call { routine: Routine::new("SI_modelUpdate", 0x0847_0000), args: vec![model.addr(), 0xffff_fffe] });
        // a float is its bits, a double two words with the low one first
        assert_eq!(o.calls[1].args, [model.addr(), 0x3f00_0000, 0, 0xc000_0000]);
        assert_eq!(o.calls[2].args, [model.addr(), 3, 4]);
        assert_eq!(<Trace as DemoCalls>::ROUTINES.len(), 4);
        o.print(2, b"Data of Relid 246 needs to be freed\n");
        assert_eq!(o.printed.len(), 1);
    }

    #[test]
    fn ported_functions_are_wired_by_address() {
        let mut m = Mem::new();
        let rest = Trace::with(|_, routine, _| if routine.name == "SI_modelLength" { Ret::eax(2) } else { Ret::NONE });
        let mut w = Wired::over(rest);
        // nothing ported: every call reaches the rest, and counting answers 0
        let model = Ptr32::new(m.place(8, 8));
        assert_eq!(refresh(&mut m, &mut w, model), 0);
        // the second package is ported: its function is registered under the original's
        // address, and the first package's call now runs it, with the same outside world
        w.registry.register(0x0813_0000, "SI_elemdictCount", |o, m, a| Ret::int(count(m, o, Ptr32::new(a[0])) as i32));
        assert_eq!(refresh(&mut m, &mut w, model), 42 + 1);
        assert_eq!(m.i16(BSS.0), 1);
        // a call through a function pointer of a record goes the same way
        assert_eq!(w.call(&mut m, Routine::at(0x0813_0000), &[model.addr()]).eax, 42);
        assert!(m.fault().is_none());

        // a registry with a context of its own, as an outside world
        let mut reg: Registry<Vec<u32>> = Registry::new();
        reg.register(0x0847_0000, "SI_modelUpdate", |log, _, a| {
            log.push(a[1]);
            Ret::NONE
        });
        let mut log = Vec::new();
        let mut o = Registered { registry: &mut reg, ctx: &mut log };
        assert_eq!(refresh(&mut m, &mut o, model), 2);
        assert_eq!((log.as_slice(), reg.unported()), (&[0xffff_fffe][..], &[0x080f_0000, 0x0813_0000][..]));
        assert_eq!(reg.call(&mut log, &mut m, FnPtr::NULL, &[]), Called::Fault);
    }
}
