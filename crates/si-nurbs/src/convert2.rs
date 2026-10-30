// SPDX-License-Identifier: MIT
//! nurbs15 (finding 0025 section 33): `SI_nurbs2Triangle` with its trimmed branch, on the memory image.
//! [`crate::convert::nurbs2triangle`] stops with [`Halt::Unported`] on a trimmed surface; this one goes on to
//! `TriangulateTrimmedNurbsSrf` ([`crate::trimmed::triangulate_trimmed_nurbs_srf`], the port of
//! [soft 0x084fd14c TriangulateTrimmedNurbsSrf]: 100 vector cases, 61 compared, 0 different; finding 0025 section 33).

use crate::convert::triangulate_non_trimmed;
use crate::image::Halt;
use crate::trimmed::triangulate_trimmed_nurbs_srf;
use si_mem::{Mem, Outside};

/// `needTrianglesWithIndex` (a 16-bit flag) and `trianglesWithIndex` (the index array pointer), the program's two globals the trimmed
/// branch hands to `TriangulateTrimmedNurbsSrf`.
pub const NEED_TRIANGLES_WITH_INDEX: u32 = 0x086d_9fc0;
pub const TRIANGLES_WITH_INDEX: u32 = 0x086d_9fc4;

/// [soft 0x084e16f8 SI_nurbs2Triangle]: `(shape, srf, materials, elem_id, n_verts, verts, n_tris, tris, flag)` as
/// [`crate::convert::nurbs2triangle`], with the trimmed branch (`srf + 0x60` nonzero) running `TriangulateTrimmedNurbsSrf` with
/// `shape` as the base geometry, the global `needTrianglesWithIndex` and the address of `trianglesWithIndex`; the two counts it
/// leaves are copied to `*n_verts` and `*n_tris` and the display steps put back after, as for an untrimmed surface.
#[allow(clippy::too_many_arguments)]
pub fn nurbs2triangle_full(
    m: &mut Mem, o: &mut dyn Outside, shape: u32, srf: u32, materials: u32, elem_id: i32, n_verts: u32, verts: u32, n_tris: u32, tris: u32, flag: i16,
) -> Result<(), Halt> {
    if srf == 0 || m.i32(srf + 4) <= 0 || m.u32(srf + 0x0c) == 0 || m.i32(srf + 0x28) <= 0 || m.u32(srf + 0x30) == 0 {
        return Ok(());
    }
    let (kept_u, kept_v) = (m.u32(srf + 0x70), m.u32(srf + 0x74));
    let other = shape != 0 && shape != srf;
    if flag > 0 {
        let f = flag as i32;
        m.set_i32(srf + 0x74, f);
        m.set_i32(srf + 0x70, f);
        if other {
            m.set_i32(shape + 0x74, f);
            m.set_i32(shape + 0x70, f);
        }
    }
    let put_back = |m: &mut Mem| {
        if flag > 0 {
            m.set_u32(srf + 0x70, kept_u);
            m.set_u32(srf + 0x74, kept_v);
            if other {
                m.set_u32(shape + 0x70, kept_u);
                m.set_u32(shape + 0x74, kept_v);
            }
        }
    };
    let counts = m.local(8);
    let r = if m.u32(srf + 0x60) != 0 {
        let need = m.i16(NEED_TRIANGLES_WITH_INDEX);
        triangulate_trimmed_nurbs_srf(
            m, o, shape, srf, materials, elem_id, counts, counts + 4, verts, tris, need, TRIANGLES_WITH_INDEX, &mut |_, _| {},
        )
    } else {
        triangulate_non_trimmed(m, shape, srf, materials, elem_id, counts, counts + 4, verts, tris)
    };
    if r.is_ok() {
        let (a, b) = (m.u32(counts), m.u32(counts + 4));
        m.set_u32(n_verts, a);
        m.set_u32(n_tris, b);
        put_back(m);
    }
    m.release(8);
    r
}
