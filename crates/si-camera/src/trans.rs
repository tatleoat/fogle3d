// SPDX-License-Identifier: MIT
//! Moving a camera or its interest by a vector: `SI_addCamTrans`, `SI_addCitTrans`
//! (finding 0032, "Moves"). The viewer's translation (`doTransSGI`) and the schematic's
//! drag call them.

use crate::mat::{cam_mat, read_matrix};
use si_elemio::image::off::camera as cam;
use si_math::algebra as alg;
use si_math::types::Vector;
use si_mem::{Mem, Outside};
use si_x87::x;

/// `SI_glbManip` + 0x0c (0x0878156c): the translation preference (the setup file's
/// `PREF_TRANSLATION`); 0 moves in the camera's own axes.
pub const SI_GLB_MANIP_TRANSLATION: u32 = 0x0878_156c;

/// The body both routines share: the vector copied (as words) into a local; when the
/// translation preference is 0, turned into the camera's axes: `SI_camMat` into a local
/// matrix, its translation (row 3, three floats) cleared, `SI_vectorMultiplyMatrix` in
/// place; then each of the three floats at `field` gets the vector's float added (in the
/// register, stored rounded).
fn add_trans(m: &mut Mem, o: &mut dyn Outside, camera: u32, v: u32, field: u32) {
    let mut d = Vector::new(f32::from_bits(m.u32(v)), f32::from_bits(m.u32(v + 4)), f32::from_bits(m.u32(v + 8)));
    if m.i32(SI_GLB_MANIP_TRANSLATION) == 0 {
        let mat = m.local(64);
        cam_mat(m, o, camera, mat);
        for k in 0..3 {
            m.set_u32(mat + 0x30 + 4 * k, 0);
        }
        d = alg::vector_multiply_matrix(&d, &read_matrix(m, mat));
        m.release(64);
    }
    for (k, c) in [d.x, d.y, d.z].into_iter().enumerate() {
        let a = camera + field + 4 * k as u32;
        let sum = (x(c) + m.f32(a)).to_f32();
        m.set_u32(a, sum.to_bits());
    }
}

/// [soft 0x080d676c SI_addCamTrans]: moves the camera's position (+0x10) by `v` (see
/// [`add_trans`]).
pub fn add_cam_trans(m: &mut Mem, o: &mut dyn Outside, camera: u32, v: u32) {
    add_trans(m, o, camera, v, cam::POSITION);
}

/// [soft 0x080d67e8 SI_addCitTrans]: moves the camera's interest (+0x1c) by `v`.
pub fn add_cit_trans(m: &mut Mem, o: &mut dyn Outside, camera: u32, v: u32) {
    add_trans(m, o, camera, v, cam::INTEREST);
}
