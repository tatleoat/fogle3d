// SPDX-License-Identifier: MIT
//! A patch as a mesh of quadrilaterals on the image (ninth pass): `SI_patchConvertToMesh`
//! (`patch.c`). The patch's grid of points and the texture coordinates of a polygon are
//! `si-spline`'s image forms (`SI_patchSurface`, `patchToMeshSetPolygonUV`); the counts of
//! segments and the coordinates' arithmetic its value forms, the same the value form of this
//! routine (`si_spline::patch_grid::convert_to_mesh`) uses.

use si_mem::{Access, Mem, Ptr32};
use si_spline::eval::nb_segment_step;
use si_spline::patch_eval_image::{patch_surface, patch_to_mesh_set_polygon_uv};
use si_spline::patch_grid::{quad_uv, uv_steps};

use super::mattxt::{mdl_get_mat_and_txt2d, Callbacks, MAT_AND_TXT2D_DESTROY};
use super::off as mo;
use crate::model_image::Outside;

/// Offsets in a patch (`__SI_Patch`, as `si-types` has it) that the conversion reads.
mod p {
    pub const U_TYPE: u32 = 0x04;
    pub const V_TYPE: u32 = 0x08;
    pub const NB_U: u32 = 0x0c;
    pub const NB_V: u32 = 0x0e;
    pub const U_CLOSED: u32 = 0x10;
    pub const V_CLOSED: u32 = 0x12;
    pub const U_STEP: u32 = 0x1c;
    pub const V_STEP: u32 = 0x1e;
}

/// The tolerance the conversion cleans its mesh with: the float 0.001 (0x3a83126f, at
/// 0x086602f8).
pub const CLEANUP_TOLERANCE: f32 = f32::from_bits(0x3a83_126f);

/// The mapping method of the 2D textures that give the mesh texture coordinates.
pub const UV_METHOD: i16 = 4;

/// `a % b` as `idiv` leaves it; a divisor of 0 is the processor's divide error, which ends
/// the original: a recorded fault here, and `None`.
fn rem(m: &Mem, a: i32, b: i32) -> Option<i32> {
    if b == 0 || (a == i32::MIN && b == -1) {
        m.record_fault(0, 0, Access::Read);
        return None;
    }
    Some(a.wrapping_rem(b))
}

/// The patch `patch` of the node `model` as a mesh [soft 0x0847b69c SI_patchConvertToMesh];
/// null when no mesh could be allocated or the mesh would have no polygon or more than
/// `SI_nbPolMax`. In order:
///
/// 1. `SI_meshAllocate`; the segments in u and in v (`SI_splineNbSegmentStep` of the type,
///    the point count (16 bits, unsigned) and the closed flag (signed) of each direction);
///    `SI_patchSurface` (`nu × nv` points in a block of its own).
/// 2. One vertex per point (`SI_vertexAllocate(nu × nv)`, the count stored first), the three
///    words of each position copied; the points' block freed (`free`) when there is one.
/// 3. `SI_mdlGetMatAndTxt2D(model, 4)`: the mesh gets texture coordinates when the node or
///    one of its materials has a 2D texture of method 4. The answer is only tested and
///    given up (`SI_genListDestroy` with `SI_matAndTxt2DDestroy`).
/// 4. `qu = u_step × segments_u`, `qv = v_step × segments_v` (the steps 16 bits, unsigned),
///    the polygon count `qu × qv` stored; at or below 0 or above `SI_nbPolMax` the mesh is
///    disposed of (`SI_meshDispose`) and the answer is null.
/// 5. `SI_polygonAllocate(qu × qv)`; polygon `i × qv + j` (i < qu outer, j < qv inner) gets
///    four corners (`SI_polygonInitialize`): `i nv + j`, `i nv + (j + 1) mod nv`,
///    `((i + 1) mod nu) nv + (j + 1) mod nv`, `((i + 1) mod nu) nv + j` (`idiv`: a count of
///    0 is a divide error, which ends the original); with texture coordinates, the four
///    pairs of `si_spline::patch_grid::quad_uv` in a local, through
///    `patchToMeshSetPolygonUV`.
/// 6. `SI_meshUpdate`, `SI_meshInitVertexColors`, `SI_newMeshCleanup(mesh, 0.001, 0.0, 1, 0,
///    1)`; the answer is the mesh.
pub fn patch_convert_to_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, patch: u32) -> u32 {
    let mesh = super::mesh_allocate(m);
    if mesh == 0 {
        return 0;
    }
    // the frame: the mesh (its address goes to SI_meshDispose), the list of textures (to
    // SI_genListDestroy), the points (SI_patchSurface's answer), nu and nv, the eight
    // texture coordinates
    let frame = m.local(0x30);
    let (pmesh, plist, ppoints, pnu, pnv, uv) = (frame, frame + 4, frame + 8, frame + 0xc, frame + 0xe, frame + 0x10);
    let answer = convert(m, o, model, patch, mesh, [pmesh, plist, ppoints, pnu, pnv, uv]);
    m.release(0x30);
    answer
}

fn convert(m: &mut Mem, o: &mut dyn Outside, model: u32, patch: u32, mesh: u32, [pmesh, plist, ppoints, pnu, pnv, uv]: [u32; 6]) -> u32 {
    m.set_u32(pmesh, mesh);
    let (nsu, _) = nb_segment_step(m.i32(patch + p::U_TYPE), m.u16(patch + p::NB_U) as i32, m.i16(patch + p::U_CLOSED));
    let (nsv, _) = nb_segment_step(m.i32(patch + p::V_TYPE), m.u16(patch + p::NB_V) as i32, m.i16(patch + p::V_CLOSED));
    patch_surface(m, Ptr32::new(patch), ppoints, pnu, pnv);
    if m.fault().is_some() {
        return 0;
    }
    let nbv = (m.i16(pnu) as i32).wrapping_mul(m.i16(pnv) as i32);
    m.set_i32(mesh + mo::NB_VERTICES, nbv);
    let vertices = si_hrc::image::geom::vertex_allocate(m, nbv);
    m.set_u32(mesh + mo::VERTICES, vertices);
    let mut k = 0i32;
    while k < m.i32(mesh + mo::NB_VERTICES) {
        let from = m.u32(ppoints).wrapping_add((k as u32).wrapping_mul(12));
        let to = m.u32(mesh + mo::VERTICES).wrapping_add((k as u32).wrapping_mul(mo::VERTEX));
        for off in [0, 4, 8] {
            let w = m.u32(from + off);
            m.set_u32(to + off, w);
        }
        k += 1;
    }
    let points = m.u32(ppoints);
    if points != 0 {
        m.free(points);
        m.set_u32(ppoints, 0);
    }
    let list = mdl_get_mat_and_txt2d(m, o, model, UV_METHOD);
    m.set_u32(plist, list);
    let textured = list != 0;
    if textured {
        si_gen::list::destroy(m, &mut Callbacks, plist, MAT_AND_TXT2D_DESTROY);
    }
    let qu = (m.u16(patch + p::U_STEP) as i32).wrapping_mul(nsu);
    let qv = (m.u16(patch + p::V_STEP) as i32).wrapping_mul(nsv);
    let count = qu.wrapping_mul(qv);
    m.set_i32(mesh + mo::NB_POLYGONS, count);
    if count <= 0 || count > m[super::g::SI_NB_POL_MAX] {
        super::obj::mesh_dispose(m, pmesh);
        m.set_u32(pmesh, 0);
        return 0;
    }
    let polygons = si_hrc::image::geom::polygon_allocate(m, count);
    m.set_u32(mesh + mo::POLYGONS, polygons);
    let (fu, fv) = uv_steps(qu, qv);
    let mut at = 0u32;
    for i in 0..qu {
        for j in 0..qv {
            let (nu, nv) = (m.i16(pnu) as i32, m.i16(pnv) as i32);
            let Some(i1) = rem(m, i + 1, nu) else { return 0 };
            let Some(j1) = rem(m, j + 1, nv) else { return 0 };
            let corners = [i.wrapping_mul(nv).wrapping_add(j), i.wrapping_mul(nv).wrapping_add(j1), i1.wrapping_mul(nv).wrapping_add(j1), i1.wrapping_mul(nv).wrapping_add(j)];
            let poly = m.u32(mesh + mo::POLYGONS).wrapping_add(at);
            super::rebuild::polygon_initialize(m, poly, &corners);
            if textured {
                for (c, [u, v]) in quad_uv(i, j, fu, fv).into_iter().enumerate() {
                    m.set_f32(uv + 8 * c as u32, u);
                    m.set_f32(uv + 8 * c as u32 + 4, v);
                }
                let poly = m.u32(mesh + mo::POLYGONS).wrapping_add(at);
                patch_to_mesh_set_polygon_uv(m, poly, uv);
            }
            at = at.wrapping_add(mo::POLYGON);
        }
    }
    super::rebuild::update(m, o, mesh);
    super::obj::mesh_init_vertex_colors(m, mesh);
    super::cleanup::new_mesh_cleanup(m, o, mesh, CLEANUP_TOLERANCE, 0.0, 1, 0, 1);
    m.u32(pmesh)
}
