// SPDX-License-Identifier: MIT
//! Functions of this package that the rest of the program calls through a pointer: each
//! is known there by its code address in the Linux binary, which is what the ported code
//! hands on (finding 0024, "function pointers as the original's code addresses"). An
//! implementation of [`Outside`] that stands for a routine taking such a pointer calls
//! [`call`] with the address and the stack words.

use si_mem::{FnPtr, Mem, Ptr32};

use crate::outside::Outside;

/// `checkLow` (0x081f88a0): the callback `checklowestGeometryVertexRec` gives
/// `SI_modelTraversePnts`.
pub const CHECK_LOW: FnPtr = FnPtr::new(0x081f_88a0);

/// Runs the function of this package at `addr` with the stack words `args`; `None` when
/// there is none at that address. The answer is what the original leaves in `eax`.
pub fn call(m: &mut Mem, _o: &mut dyn Outside, addr: u32, args: &[u32]) -> Option<u32> {
    let arg = |i: usize| args.get(i).copied().unwrap_or(0);
    if addr == CHECK_LOW.addr() {
        crate::pivot::check_low(m, Ptr32::new(arg(0)), arg(1) as i32, Ptr32::new(arg(2)));
        return Some(0);
    }
    None
}
