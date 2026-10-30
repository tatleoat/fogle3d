// SPDX-License-Identifier: MIT
//! Global variables: typed pointers at the addresses the binary gives them.
//!
//! Nothing here reproduces a function of the original.
//!
//! A global of the original is a place in `.data` (it has an initial value) or `.bss` (it
//! starts as zero). In the port it is a constant [`crate::Ptr32`] at that address, so
//! `m[SI_FRAME_CURRENT]` is the variable, in the one image, for every crate. The binary's
//! data is not embedded: the crate that owns a global declares its initial value here in
//! its own source, and a script of that crate compares every address with the binary's
//! symbol table and every initial value with the bytes of `.data`
//! (`re/tools/si-mem/check_globals.py` reads every `globals!` block of the workspace).
//!
//! Two crates may declare the same global (the owner of the source file that defines it,
//! and a user that cannot depend on the owner). Both declarations are held against the
//! binary, so they cannot disagree, and writing an initial value twice writes the same
//! bytes.

use crate::Mem;

/// One declared global: Rust name, address, size in bytes, the original's symbol (empty
/// when the binary has none), and the initial value of a `.data` scalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Global {
    pub name: &'static str,
    pub addr: u32,
    pub size: u32,
    pub symbol: &'static str,
    pub init: Option<i64>,
}

/// Writes the initial values of a table of globals (the scalars; a crate stores its tables
/// and strings itself).
pub fn init_globals(m: &mut Mem, table: &[Global]) {
    for g in table {
        if let Some(v) = g.init {
            m.set_bytes(g.addr, &v.to_le_bytes()[..(g.size as usize).min(8)]);
        }
    }
}

/// Declares globals: `NAME: type = address, "symbol"[, init value];` each. Every name
/// becomes a `pub const NAME: Ptr32<type>`, and `ALL` is the table of them.
///
/// ```
/// mod g {
///     si_mem::globals! {
///         /// The module shown: 0 Model, 1 Motion, 2 Actor, 3 Matter, 4 Tools.
///         SI_ACTIVE: i16 = 0x08679480, "SI_active", init 0;
///         /// Clock ticks within which a second press is a double click.
///         SI_DOUBLE_CLICK_DELAY: i32 = 0x08679310, "SI_doubleClickDelay", init 30;
///         SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
///     }
/// }
/// let mut m = si_mem::Mem::new();
/// si_mem::init_globals(&mut m, g::ALL);
/// assert_eq!(m[g::SI_DOUBLE_CLICK_DELAY], 30);
/// m[g::SI_FRAME_CURRENT] += 1;
/// assert_eq!(m.i32(0x08782040), 1);
/// ```
#[macro_export]
macro_rules! globals {
    ( $( $(#[$meta:meta])* $name:ident : $ty:ty = $addr:literal , $sym:literal $(, init $init:expr)? ; )* ) => {
        $( $(#[$meta])* pub const $name: $crate::Ptr32<$ty> = $crate::Ptr32::new($addr); )*
        /// Every variable declared in this block.
        pub const ALL: &[$crate::Global] = &[
            $( $crate::Global { name: stringify!($name), addr: $addr, size: ::core::mem::size_of::<$ty>() as u32,
                                symbol: $sym, init: $crate::globals!(@init $($init)?) } ),*
        ];
    };
    (@init) => { None };
    (@init $init:expr) => { Some($init as i64) };
}

#[cfg(test)]
mod tests {
    use crate::{Mem, Ptr32};

    crate::globals! {
        SWIFTMODE: i16 = 0x086be9a8, "swiftmode", init -1;
        SI_SBOXFI: crate::FnPtr = 0x086ee2a4, "SI_sboxfi", init 0x0861236c;
        Q_E: [[u8; 0x18]; 10] = 0x08815920, "qE";
    }

    #[test]
    fn globals_are_typed_places_with_initial_values() {
        let mut m = Mem::new();
        crate::init_globals(&mut m, ALL);
        assert_eq!(m[SWIFTMODE], -1);
        assert_eq!(m.u16(0x086be9a8), 0xffff);
        assert_eq!(m.u16(0x086be9aa), 0); // two bytes written, not eight
        assert_eq!(m[SI_SBOXFI], Ptr32::new(0x0861_236c));
        assert_eq!(ALL[2], crate::Global { name: "Q_E", addr: 0x08815920, size: 0xf0, symbol: "qE", init: None });
        m[Q_E][9][0x17] = 5;
        assert_eq!(m.u8(0x08815920 + 0xef), 5);
        assert!(m.fault().is_none());
    }
}
