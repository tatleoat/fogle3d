// SPDX-License-Identifier: MIT
//! `dynamicio.c`: what the dynamics of a model writes to a scene file.

use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};

/// [soft 0x08499460 odynamicWriteBinaryChunk]: a flag word (one when the model has a vector at `+0x2f8`), then the three
/// floats of that vector when it has one. The flag is written as a short from a local, as the original does.
pub fn odynamic_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32) {
    let v = m.u32(model + 0x2f8);
    let flag = m.local(4);
    m.set_u16(flag, (v != 0) as u16);
    o.si_byteswap_fwrite(m, flag, 2, 1, file);
    let v = m.u32(model + 0x2f8);
    if v != 0 {
        o.si_vector_fwrite(m, v, 0xc, 1, file);
    }
    m.release(4);
}
