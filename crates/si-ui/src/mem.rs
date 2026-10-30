// SPDX-License-Identifier: MIT
//! The program's writable memory as a 32-bit image, and typed pointers into it.
//!
//! Nothing here reproduces a function of the original; it is the ground the ported functions
//! stand on. The original's user-interface code is written against addresses: a global holds
//! a pointer to the active event, an event holds a pointer to the group it is about, the
//! screen template holds a pointer to the array of view windows, a view window points to its
//! camera. The port keeps those addresses as they are, so that
//!
//! - a record is a typed `#[repr(C)]` value at an address ([`P<T>`], read and written with
//!   `mem[p]`), with the original's layout ([`crate::records`]);
//! - records that belong to parts of the program not ported yet (a dialog, a menu, a camera)
//!   can be pointed to, and the few fields of them these files touch can be read, without
//!   inventing an owner for them;
//! - a check can put the same bytes at the same addresses in the emulated original and here,
//!   and compare every byte either side changed.
//!
//! **The image is `si-mem`'s** (finding 0024), the one every crate shares; this module was
//! its first form and is now the names this crate uses for it, and this crate's map. The
//! image has the original's `.data` and `.bss` at their addresses (zero until
//! [`crate::globals::init`] writes the initial values this crate knows) and, where this
//! crate's check has them, an arena for records other parts would own and a heap for what
//! the ported functions allocate themselves.

use si_mem::{Map, OnFault};
pub use si_mem::{Addr, Mem, Pod, Snapshot, BSS, DATA};

/// A pointer of the 32-bit original: an address, typed by what is there. Zero is null.
/// (`si_mem::Ptr32`, under the name these files were written with.)
pub use si_mem::Ptr32 as P;

/// Records owned by parts of the program that are not ported (in the original: their heap
/// blocks). The address is the emulation harness's buffer area, so that a check can use the
/// same addresses on both sides.
pub const ARENA: (u32, u32) = (0x6100_0000, 0x0010_0000);
/// What the ported functions allocate (`SI_calloc`), handed out upwards from the start: the
/// place `re/tools/si-ui/emu_ui.py` gives the original's blocks, without block headers.
pub const HEAP: (u32, u32) = (0x6110_0000, 0x0010_0000);

/// A fresh image, all zero, laid out as this crate's check lays out the emulator's memory.
///
/// A fault stops the routine (`OnFault::Stop`: the access unwinds with a `si_mem::Stop`
/// payload and no panic message): the original would be killed by the system there, and
/// the check compares what a faulting case did up to that point with the emulated
/// original's, which stopped at it.
pub fn image() -> Mem {
    let mut m = Mem::with_map(Map { arena: ARENA, heap: HEAP, heap_header: 0, arena_gap: 0, ..Map::default() });
    m.on_fault(OnFault::Stop);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_access_and_changes() {
        let mut m = image();
        let p: P<i16> = P::at(BSS.0 + 0x10);
        let before = m.snapshot();
        m[p] = -2;
        m[p.add(1)] = 7;
        assert_eq!(m[p], -2);
        assert_eq!(m.u32(BSS.0 + 0x10), 0x0007_fffe);
        assert_eq!(m.changed(&before), vec![(BSS.0 + 0x10, vec![0xfe, 0xff, 0x07])]);
        let a = m.malloc(0x18);
        assert_eq!(a, HEAP.0);
        assert_eq!(m.malloc(1), HEAP.0 + 0x18);
        m.put_cstr(ARENA.0, b"Edit ");
        assert_eq!(m.cstr(ARENA.0), b"Edit ");
    }

    #[test]
    fn a_null_pointer_faults() {
        let m = image();
        let p: P<i32> = P::NULL;
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m[p]));
        let stop = stopped.unwrap_err().downcast::<si_mem::Stop>().expect("a fault stops the routine");
        assert_eq!((stop.0.addr, stop.0.size), (0, 4));
        assert_eq!(m.fault(), Some(stop.0));
    }
}
