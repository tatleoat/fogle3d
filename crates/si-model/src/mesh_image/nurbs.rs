// SPDX-License-Identifier: MIT
//! A NURBS surface as a mesh on the image (tenth pass): `SI_nurbsConvertToMesh` and
//! `nurbsToMeshUpdatePolygonUV` (`nrb_convert.c`). The surface's triangulation is
//! `si-nurbs`' image form of `SI_nurbs2Triangle`, which leaves
//! the render vertices (`SI_RVertex`, 0x4c bytes: the texture pair at +0, the position at
//! +8) in the node and the grid's counts in `uNoPoints` and `vNoPoints`; a trimmed surface's
//! triangulation (`TriangulateTrimmedNurbsSrf`) is not ported there, and the conversion
//! stops with it.

use si_mem::Mem;
use si_nurbs::convert::{NEED_TRIANGLES_WITH_INDEX, TRIANGLES_WITH_INDEX, U_NO_POINTS, V_NO_POINTS};
use si_nurbs::image::Halt;
use si_x87::fmov;

use super::mattxt::{mdl_get_mat_and_txt2d, Callbacks, MAT_AND_TXT2D_DESTROY};
use super::off as mo;
use super::patch::{CLEANUP_TOLERANCE, UV_METHOD};
use crate::model_image::Outside;

/// The node's render arrays, which the triangulation fills and the conversion gives up.
mod nd {
    pub const N_VERTS: u32 = 0x228;
    pub const VERTS: u32 = 0x22c;
    pub const N_TRIS: u32 = 0x230;
    pub const TRIS: u32 = 0x234;
    pub const MATERIALS: u32 = 0x20c;
    pub const ELEM_ID: u32 = 0x354;
}

/// Offsets in a NURBS surface (`SI_NurbsSrf`) the conversion reads.
mod srf {
    /// The closed flag of U (16 bits): the grid's last row repeats its first.
    pub const U_CLOSED: u32 = 0x20;
    /// The closed flag of V (16 bits).
    pub const V_CLOSED: u32 = 0x44;
    /// The number of trimming curves.
    pub const NB_TRIMS: u32 = 0x60;
}

/// The size of a render vertex.
const RVERTEX: u32 = 0x4c;

/// The node's render arrays freed and their counts cleared, as at both ends of the
/// conversion.
fn give_up_arrays(m: &mut Mem, node: u32) {
    for off in [nd::VERTS, nd::TRIS] {
        let a = m.u32(node + off);
        if a != 0 {
            m.free(a);
            m.set_u32(node + off, 0);
        }
    }
    m.set_u32(node + nd::N_VERTS, 0);
    m.set_u32(node + nd::N_TRIS, 0);
}

/// [soft 0x084e2a74 nurbsToMeshUpdatePolygonUV]: `nurbsToMeshUpdatePolygonUV(poly, verts,
/// index…)`, one index of a render vertex per corner of the polygon (the 16-bit count at
/// +0x10): corner k gets `1 − u` (formed in the register, rounded once) and `v` of the
/// vertex's texture pair (+0, +4) at +0x14 and +0x18.
pub fn nurbs_to_mesh_update_polygon_uv(m: &mut Mem, poly: u32, verts: u32, index: &[i32]) {
    let mut k = 0usize;
    while k < m.u16(poly + mo::POLYGON_NB_NODES) as usize {
        let v = verts.wrapping_add((index.get(k).copied().unwrap_or(0) as u32).wrapping_mul(RVERTEX));
        let corner = m.u32(poly + mo::POLYGON_NODES).wrapping_add(mo::NODE * k as u32);
        let u = (si_x87::x(1.0f32) - si_x87::x(m.f32(v))).to_f32();
        m.set_f32(corner + mo::NODE_UV, u);
        let w = fmov(m.f32(v + 4));
        m.set_f32(corner + mo::NODE_UV + 4, w);
        k += 1;
    }
}

/// The calls of the trimmed branch of `SI_nurbs2Triangle` go to this module's outside world (unit route3); only `eax` comes back.
struct MemSide<'a>(&'a mut dyn Outside);
impl si_mem::Outside for MemSide<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        si_mem::Ret::eax(self.0.call(m, &crate::model_image::Callee { name: r.name, addr: r.addr }, args))
    }
}

/// The surface `surface` of the node `node` as a mesh [soft 0x084df320 SI_nurbsConvertToMesh];
/// null when no mesh could be allocated or it would have more polygons than `SI_nbPolMax`;
/// `Err` where `si-nurbs`' triangulation stops (a trimmed surface). In order:
///
/// 1. The node's render arrays given up (+0x22c and +0x234 freed, the counts at +0x228 and
///    +0x230 cleared). `SI_mdlGetMatAndTxt2D(node, 4)`, only tested (texture coordinates or
///    not) and given up at once. A surface with trims (+0x60 above 0) sets
///    `needTrianglesWithIndex`.
/// 2. `SI_nurbs2Triangle(null, surface, node's materials, node's element number, the node's
///    four render words, 0)`; `needTrianglesWithIndex` cleared.
/// 3. `SI_meshAllocate`. The polygon count: the triangles' (+0x230) of a trimmed surface,
///    else `(vNoPoints − 1) × (uNoPoints − 1)`. Above `SI_nbPolMax` the mesh is disposed of
///    and the answer is null **at once** (the node's render arrays stay).
/// 4. A closed direction takes one off its global count (`uNoPoints` for U, `vNoPoints` for
///    V). The vertex count: the render vertices' (+0x228) of a trimmed surface, else the
///    product of the two counts; `SI_vertexAllocate`, `SI_polygonAllocate`.
/// 5. Trimmed: each vertex's position copied from its render vertex (+8, through the unit);
///    each triangle of `trianglesWithIndex` (three indices) a polygon of three corners, with
///    texture coordinates by [`nurbs_to_mesh_update_polygon_uv`]; `trianglesWithIndex`
///    freed. Not trimmed: the vertices row by row from the render vertices, a closed V's
///    repeated last vertex of each row skipped (and a closed U's last row); polygon (i, j)
///    for i below the original U count less 1 and j below the original V count less 1 has
///    the corners `i V + j`, `i₁ V + j`, `i₁ V + j₁`, `i V + j₁` (V the reduced count; `i₁`,
///    `j₁` the next row and column modulo the reduced counts, `idiv`) and texture
///    coordinates from the render vertices `i Vo + j`, `(i + 1) Vo + j`,
///    `(i + 1) Vo + j + 1`, `i Vo + j + 1` (Vo the original count).
/// 6. `SI_meshUpdate`, `SI_meshInitVertexColors`, `SI_newMeshCleanup(mesh, 0.001 (0x08661780),
///    0.0, 1, 0, 0)`; the node's render arrays given up as in step 1; the answer is the mesh.
pub fn nurbs_convert_to_mesh(m: &mut Mem, o: &mut dyn Outside, node: u32, surface: u32) -> Result<u32, Halt> {
    // the frame: the mesh (its address goes to SI_meshDispose), the list of textures (to
    // SI_genListDestroy)
    let frame = m.local(8);
    let r = convert(m, o, node, surface, frame, frame + 4);
    m.release(8);
    r
}

fn rem(m: &Mem, a: i32, b: i32) -> Option<i32> {
    if b == 0 || (a == i32::MIN && b == -1) {
        m.record_fault(0, 0, si_mem::Access::Read);
        return None;
    }
    Some(a.wrapping_rem(b))
}

fn convert(m: &mut Mem, o: &mut dyn Outside, node: u32, surface: u32, pmesh: u32, plist: u32) -> Result<u32, Halt> {
    give_up_arrays(m, node);
    let list = mdl_get_mat_and_txt2d(m, o, node, UV_METHOD);
    m.set_u32(plist, list);
    let textured = list != 0;
    if textured {
        si_gen::list::destroy(m, &mut Callbacks, plist, MAT_AND_TXT2D_DESTROY);
    }
    let trimmed = m.i32(surface + srf::NB_TRIMS) > 0;
    if trimmed {
        m.set_i16(NEED_TRIANGLES_WITH_INDEX, 1);
    }
    let (materials, elem) = (m.u32(node + nd::MATERIALS), m.i32(node + nd::ELEM_ID));
    si_nurbs::convert2::nurbs2triangle_full(m, &mut MemSide(&mut *o), 0, surface, materials, elem, node + nd::N_VERTS, node + nd::VERTS, node + nd::N_TRIS, node + nd::TRIS, 0)?;
    m.set_i16(NEED_TRIANGLES_WITH_INDEX, 0);
    let verts = m.u32(node + nd::VERTS);
    let mesh = super::mesh_allocate(m);
    m.set_u32(pmesh, mesh);
    if mesh == 0 {
        give_up_arrays(m, node);
        return Ok(0);
    }
    let np = if trimmed {
        m.i32(node + nd::N_TRIS)
    } else {
        let c = m.i32(V_NO_POINTS).wrapping_sub(1);
        c.wrapping_mul(m.i32(U_NO_POINTS)).wrapping_sub(c)
    };
    m.set_i32(mesh + mo::NB_POLYGONS, np);
    if m.i32(mesh + mo::NB_POLYGONS) > m[super::g::SI_NB_POL_MAX] {
        super::obj::mesh_dispose(m, pmesh);
        return Ok(0);
    }
    let (u0, v0) = (m.i32(U_NO_POINTS), m.i32(V_NO_POINTS));
    if m.i16(surface + srf::U_CLOSED) != 0 {
        m.set_i32(U_NO_POINTS, u0.wrapping_sub(1));
    }
    if m.i16(surface + srf::V_CLOSED) != 0 {
        m.set_i32(V_NO_POINTS, v0.wrapping_sub(1));
    }
    let nv = if trimmed { m.i32(node + nd::N_VERTS) } else { m.i32(U_NO_POINTS).wrapping_mul(m.i32(V_NO_POINTS)) };
    m.set_i32(mesh + mo::NB_VERTICES, nv);
    let vertices = si_hrc::image::geom::vertex_allocate(m, m.i32(mesh + mo::NB_VERTICES));
    m.set_u32(mesh + mo::VERTICES, vertices);
    let polygons = si_hrc::image::geom::polygon_allocate(m, m.i32(mesh + mo::NB_POLYGONS));
    m.set_u32(mesh + mo::POLYGONS, polygons);
    if trimmed {
        let mut k = 0i32;
        while k < m.i32(mesh + mo::NB_VERTICES) {
            let from = verts.wrapping_add((k as u32).wrapping_mul(RVERTEX)) + 8;
            put_vertex(m, pmesh, (k as u32).wrapping_mul(mo::VERTEX), from);
            k += 1;
        }
        let mut p = 0i32;
        while p < m.i32(mesh + mo::NB_POLYGONS) {
            let t = m.u32(TRIANGLES_WITH_INDEX).wrapping_add((p as u32).wrapping_mul(12));
            let corners = [m.i32(t), m.i32(t + 4), m.i32(t + 8)];
            let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(mo::POLYGON));
            super::rebuild::polygon_initialize(m, poly, &corners);
            if textured {
                let t = m.u32(TRIANGLES_WITH_INDEX).wrapping_add((p as u32).wrapping_mul(12));
                let index = [m.i32(t), m.i32(t + 4), m.i32(t + 8)];
                let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(mo::POLYGON));
                nurbs_to_mesh_update_polygon_uv(m, poly, verts, &index);
            }
            p += 1;
        }
        let t = m.u32(TRIANGLES_WITH_INDEX);
        if t != 0 {
            m.free(t);
            m.set_u32(TRIANGLES_WITH_INDEX, 0);
        }
    } else {
        // the vertices: row by row, a closed V's repeated vertex skipped at each row's end
        let (rows, cols) = (m.i32(U_NO_POINTS), m.i32(V_NO_POINTS));
        let mut from = verts.wrapping_add(8);
        let mut to = 0u32;
        for _ in 0..rows.max(0) {
            for _ in 0..cols.max(0) {
                put_vertex(m, pmesh, to, from);
                to = to.wrapping_add(mo::VERTEX);
                from = from.wrapping_add(RVERTEX);
            }
            if m.i16(surface + srf::V_CLOSED) != 0 {
                from = from.wrapping_add(RVERTEX);
            }
        }
        // the polygons: over the original counts less one
        let mut at = 0u32;
        for i in 0..u0.wrapping_sub(1).max(0) {
            for j in 0..v0.wrapping_sub(1).max(0) {
                let vg = m.i32(V_NO_POINTS);
                let Some(j1) = rem(m, j + 1, vg) else { return Ok(0) };
                let Some(i1) = rem(m, i + 1, m.i32(U_NO_POINTS)) else { return Ok(0) };
                let (iv, i1v) = (i.wrapping_mul(vg), i1.wrapping_mul(vg));
                let corners = [j.wrapping_add(iv), j.wrapping_add(i1v), j1.wrapping_add(i1v), j1.wrapping_add(iv)];
                let poly = m.u32(mesh + mo::POLYGONS).wrapping_add(at);
                super::rebuild::polygon_initialize(m, poly, &corners);
                if textured {
                    let (a, b) = (i.wrapping_mul(v0), (i + 1).wrapping_mul(v0));
                    let index = [a.wrapping_add(j), b.wrapping_add(j), b.wrapping_add(j + 1), a.wrapping_add(j + 1)];
                    let poly = m.u32(mesh + mo::POLYGONS).wrapping_add(at);
                    nurbs_to_mesh_update_polygon_uv(m, poly, verts, &index);
                }
                at = at.wrapping_add(mo::POLYGON);
            }
        }
    }
    super::rebuild::update(m, o, mesh);
    super::obj::mesh_init_vertex_colors(m, mesh);
    super::cleanup::new_mesh_cleanup(m, o, mesh, CLEANUP_TOLERANCE, 0.0, 1, 0, 0);
    give_up_arrays(m, node);
    Ok(m.u32(pmesh))
}

/// A vertex's position (`to`, its offset in the mesh's array, read again at each word) from
/// the three floats at `from`, each moved through the unit.
fn put_vertex(m: &mut Mem, pmesh: u32, to: u32, from: u32) {
    for off in [0, 4, 8] {
        let f = fmov(m.f32(from + off));
        let at = m.u32(m.u32(pmesh) + mo::VERTICES).wrapping_add(to + off);
        m.set_f32(at, f);
    }
}
