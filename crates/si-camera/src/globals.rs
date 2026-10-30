// SPDX-License-Identifier: MIT
//! The globals `sys_camera.c` reads, at the binary's addresses.
//!
//! Nothing here reproduces a function of the original. The scalars are declared with
//! `si_mem::globals!` (initial values as the binary's `.data` has them); the identity matrix
//! and the axes are vectors of floats in `.data` that [`init`] stores.

use si_mem::{Mem, Ptr32};

si_mem::globals! {
    /// The running process's record; its first word is the number of the current element
    /// dictionary.
    SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
    /// The element dictionaries, 0x70 bytes each.
    SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts", init 0;
    /// The number of the MODELS chapter (`SI_cid` + 0x20; no symbol of its own).
    SI_CID_MODEL: i32 = 0x08781680, "";
    /// The current frame.
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    /// Frames per second.
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
}

/// `SI_identite`: the identity matrix every initialisation copies (`.data`).
pub const SI_IDENTITE: Ptr32<[[f32; 4]; 4]> = Ptr32::new(0x086d_c160);
/// `SI_xAxis`, `SI_yAxis`, `SI_zAxis`: the three unit vectors (`.data`).
pub const SI_X_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c1ec);
pub const SI_Y_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c1f8);
pub const SI_Z_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c204);

/// The initial values of these globals as the binary's `.data` holds them: for a fresh
/// image (the scalars include `SI_procData` and `SI_elemDicts`, which a loaded scene has
/// set otherwise; on such an image use [`init_vectors`]).
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    init_vectors(m);
}

/// The identity matrix and the three axes, as the binary's `.data` holds them.
pub fn init_vectors(m: &mut Mem) {
    m.write(SI_IDENTITE, si_math::types::IDENTITY);
    m.write(SI_X_AXIS, [1.0, 0.0, 0.0]);
    m.write(SI_Y_AXIS, [0.0, 1.0, 0.0]);
    m.write(SI_Z_AXIS, [0.0, 0.0, 1.0]);
}

/// The current element dictionary as the original computes it at each call:
/// `SI_elemDicts + 0x70 × SI_procData[0]`.
pub fn cur_dict(m: &Mem) -> u32 {
    let p = m[SI_PROC_DATA];
    m[SI_ELEM_DICTS].wrapping_add(m.u32(p).wrapping_mul(0x70))
}
