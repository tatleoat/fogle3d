// SPDX-License-Identifier: MIT
//! The readers of the binary element files **on the shared memory image** (`si-mem`,
//! finding 0024): the second form of this crate's readers, beside the value form in the
//! modules above, which stays as it is.
//!
//! A function here takes the image and the arguments the original takes (a `FILE *` is the
//! address of a stream, a record an address in the image), reads and writes the original's
//! globals at their addresses, and makes the original's allocation calls in its order. What
//! it builds is the original's record, byte for byte: the camera record `SI_image` is
//! handed, the light record the renderer's `si_lights` reads.
//!
//! Each function was written from the original's listing and is checked against the
//! original routine stepped through in the emulation harness (`elemio-emucheck`,
//! `re/tools/si-elemio/check.sh`): a function is cited `[soft 0xADDRESS name]` only when no
//! case of that check differs.
//!
//! Records are addressed by named offsets ([`off`]): `si-types` has no layout for the
//! light, the camera, the lattice or the curve records, and none is complete enough here to
//! tile a `record!` (the fields no reader touches are not established).
//!
//! # Where Rust must differ
//!
//! The original's readers look at no result of a read. Four things follow that a port
//! cannot do, and each ends a call here with a [`NoReturn`] instead:
//!
//! - **A loop that never ends.** A reader's token loop keeps its token in a static
//!   variable; when the file ends before the zero token the read fails, the variable keeps
//!   the last token and its handler runs again, for ever (finding 0005). The port follows
//!   the original up to the token read that fails with a token left that is not the loop's
//!   end, and stops there: everything it allocated and stored up to that read is in the
//!   image, as the original has it at that moment.
//! - **A local of the original's stack overrun** by what the file holds (a string of 256
//!   characters or more, a field of a negative size with more than 1 KB of file left).
//! - **A count from the file larger than the file** (and than [`COUNT_LIMIT`]): the
//!   original asks the C library for that much memory and what follows depends on the
//!   machine.
//! - **An access outside memory** (a token whose handler stores through a light that was
//!   never allocated): the original dies there. The image records the fault and the port
//!   stops at the next token.

use si_mem::Mem;

pub mod anim;
pub mod bsceneio;
pub mod byteswap;
pub mod camfct;
pub mod elemdict;
pub mod fcurve;
pub mod globals;
pub mod headers;
pub mod host;
pub mod lights;
pub mod off;
pub mod outside;
pub mod shader;
pub mod utilio;

pub use outside::{Outside, Routine};

#[cfg(test)]
mod tests;

/// A count read from a file may exceed the bytes the file has left by no more than this
/// before the port refuses to allocate for it (`AGENTS.md`, the memory rule). Below it the
/// port allocates as the original does, so that a file cut inside a small block leaves what
/// the original leaves.
pub const COUNT_LIMIT: u32 = 0x1_0000;

/// Why a routine of the original would not have come back from a call (see the module's
/// text). The text names the routine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoReturn {
    /// A token loop that the end of the file leaves running for ever.
    Endless(&'static str),
    /// A local of the original's stack overrun by the file's content.
    Stack(&'static str),
    /// A count from the file beyond the file and beyond [`COUNT_LIMIT`].
    Count(&'static str),
    /// An access outside memory, recorded by the image as a fault.
    Fault(&'static str),
}

/// What a call came to: the original's result, or the reason it has none.
pub type Run<T> = Result<T, NoReturn>;

/// The number of faults the image has recorded: a reader notes it when it starts and stops
/// at its next token once it has grown.
pub(crate) fn faults(m: &Mem) -> u32 {
    m.fault_count()
}

thread_local! {
    static SIGNALLING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How many signalling NaNs the readers have moved through the x87 unit on this thread.
/// The emulation check asks: the emulator does not quiet a signalling NaN where a real
/// unit does (finding 0011), so a case that moved one is not compared.
pub fn signalling_nans_moved() -> u32 {
    SIGNALLING.with(|c| c.get())
}

/// `fld m32; fstp m32` on a float held as its bits: how the original copies a float (a
/// signalling NaN comes out quiet, as on a real unit).
pub(crate) fn fmov(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 && bits & 0x0040_0000 == 0 {
        SIGNALLING.with(|c| c.set(c.get().wrapping_add(1)));
    }
    si_x87::fmov(f32::from_bits(bits)).to_bits()
}
