// SPDX-License-Identifier: MIT
//! Turning models into render vertices and triangles: the original's `softvertex.c`
//! (finding 0012, section 3b): the mesh converter `SI_meshTriangles`, the box of a node shown
//! as its bounding box, the metaball surface, the face, and the two accessors of a render
//! vertex's texture coordinates. Patches and NURBS surfaces come in through
//! [`crate::softtriangle::Tessellators`].
//!
//! What a converter leaves in a node is a [`NodeRender`]: the arrays the original hangs on
//! the model record at +0x228 … +0x234 and +0x42c, with every pointer an index into the
//! array it points into.

use crate::types::{RVertex, Tn, Vector};
use si_model::mesh::{Edge, Mesh, PolyNode, Polygon};
use si_model::obj_global::mesh_nb_triangles;
use si_model::triangulate::{polygon_points, triangulate_contour};
use si_x87::{fmov, x, F80};

/// Reads pair `n` of a vertex's texture coordinates [soft 0x0826d510 SI_rvtxGetUV]:
/// `SI_rvtxGetUV(vertex, n, &u, &v)`. Pair 0 (and any n ≤ 0) is the two floats at the start
/// of the vertex record; pair n > 0 is entry n − 1 of the array the vertex points to at
/// +0x48. `extra` is that array for this vertex. Class: copy.
pub fn rvtx_get_uv(v: &RVertex, n: i32, extra: &[[f32; 2]]) -> (f32, f32) {
    if n > 0 {
        let p = extra[(n - 1) as usize];
        (p[0], p[1])
    } else {
        (f32::from_bits(v.w00[0]), f32::from_bits(v.w00[1]))
    }
}

/// Writes pair `n` of a vertex's texture coordinates [soft 0x0826d548 SI_rvtxSetUV]; the
/// same addressing as [`rvtx_get_uv`]. Class: copy.
pub fn rvtx_set_uv(v: &mut RVertex, n: i32, extra: &mut [[f32; 2]], u: f32, w: f32) {
    if n > 0 {
        extra[(n - 1) as usize] = [u, w];
    } else {
        v.w00 = [u.to_bits(), w.to_bits()];
    }
}

/// A null pointer at a vertex's +0x40 or +0x48 (the fields hold an index here, and 0 is one).
pub const NO_UV: u32 = u32::MAX;
/// A null material pointer in a triangle's +0x4c.
pub const NO_MATERIAL: u32 = u32::MAX;

/// What a converter hangs on a model node (`__SI_Model` +0x228 … +0x234, +0x330, +0x42c).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeRender {
    /// +0x22c, count at +0x228: the render vertices. `RVertex::uv` (+0x40) is [`NO_UV`] or
    /// the index of the vertex's first pair in `tcoords`; `RVertex::p48` (+0x48) is
    /// [`NO_UV`] or the index of its first pair in `uv2`.
    pub verts: Vec<RVertex>,
    /// +0x234: the triangle records as allocated; corners index `verts`, `Tn::next` this
    /// array, `Tn::material` is the caller's material identifier or [`NO_MATERIAL`].
    pub tns: Vec<Tn>,
    /// +0x230: how many of them are triangles (the first `tn_count` records).
    pub tn_count: i32,
    /// +0x42c: the pairs of the node's further texture coordinate sets, `uv2_stride` per
    /// vertex; `None` is a null pointer.
    pub uv2: Option<Vec<[f32; 2]>>,
    pub uv2_stride: i32,
    /// +0x330, count at +0x334: the pairs the shader interpolates (`SI_textureVerts`).
    pub tcoords: Vec<[f32; 2]>,
    /// Not a member of the original: triangles the port took back because the original
    /// gives them a corner 0x4c bytes before the vertex array ([`Wild::VertexBeforeArray`]
    /// when [`MeshTrianglesArgs::strict`] is set).
    pub wild_corners: u32,
}

/// The texture coordinate sets of a mesh node: `__SI_Model` +0x424 and +0x428.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UvDomains {
    /// +0x424: the number of sets; 1 is the pair every corner carries and nothing more.
    pub count: i32,
    /// +0x428: one 8-byte record per set after the first: a pointer to the identifiers of
    /// the textures that use the set, and their number. Here the list; its length is the
    /// number.
    pub sets: Vec<Vec<i32>>,
}

/// Whether set `d` of a mesh node is in use [soft 0x08453e50 SI_meshGetUVDomainActive]
/// (`meshEditFct.c`): `SI_meshGetUVDomainActive(model, d)`. False for a null model or a
/// negative `d`; true for set 0; for a later set below the count, whether its record lists
/// at least one texture; false otherwise. (A record the array does not have is a read
/// past it in the original; false here.) Class: integer.
pub fn mesh_get_uv_domain_active(model: Option<&UvDomains>, d: i16) -> bool {
    let Some(m) = model else { return false };
    if d < 0 {
        return false;
    }
    if d == 0 {
        return true;
    }
    (d as i32) < m.count && m.sets.get(d as usize - 1).is_some_and(|s| !s.is_empty())
}

/// The set a texture uses [soft 0x08453668 SI_meshGetTxtUVDomain] (`meshEditFct.c`):
/// `SI_meshGetTxtUVDomain(model, texture, &set)`: the first set after set 0 whose record
/// lists the texture identifier (the texture's word at +0x190), numbered from 1, else 0.
/// The original returns 0 without writing for a null model. Class: integer.
pub fn mesh_get_txt_uv_domain(model: Option<&UvDomains>, texture: i32) -> Option<i32> {
    let m = model?;
    for k in 0..(m.count - 1).max(0) as usize {
        // a record the array does not have is a read past it in the original
        if m.sets.get(k).is_some_and(|s| s.contains(&texture)) {
            return Some(k as i32 + 1);
        }
    }
    Some(0)
}

/// Pair `d` of a corner's texture coordinates [soft 0x084534d8 SI_nodeGetUV]
/// (`meshEditFct.c`): `SI_nodeGetUV(corner, d, &u, &v)`: the pair in the corner record
/// (+0x14) for d < 1 or a corner without further pairs (+0x20 null), else entry d − 1 of
/// that array. (An entry the array does not have is a read past it in the original; zeros
/// here.) Class: copy.
pub fn node_get_uv(corner: &PolyNode, d: i32) -> (f32, f32) {
    if d < 1 || corner.extra_uv.is_empty() {
        (corner.uv[0], corner.uv[1])
    } else {
        let p = corner.extra_uv.get(d as usize - 1).copied().unwrap_or([0.0; 2]);
        (p[0], p[1])
    }
}

/// Where the original leaves its arrays: the port answers with this instead of following
/// it (finding 0012, section 3b lists each).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wild {
    /// The walk round a vertex's star runs off the star's end: a group that starts with an
    /// entry whose vertex is −1, or such an entry last.
    StarOverrun,
    /// An index outside its array: a star's edge, an edge's polygon, a corner's vertex.
    Index(&'static str),
    /// More triangles than `SI_meshNbTriangles` counted: the original writes past the
    /// array it allocated (a mesh with a polygon of fewer than two corners).
    TriangleOverrun,
    /// A triangle corner that would point before the vertex array (a star entry that got
    /// no render vertex).
    VertexBeforeArray,
    /// The routine reads the model record and was given none.
    NullModel,
    /// A material is looked up in a null array.
    NullMaterials,
    /// A count that is negative: the original asks the allocator for that many records.
    Count,
    /// An array of no records is asked of `SI_malloc`, which answers null for no bytes:
    /// the original calls `si_fatal` (`SYSALLOC failed, not enough system memory`).
    FatalAlloc,
}

/// One of a node's materials as `SI_meshTriangles` sees it: what goes into a triangle's
/// +0x4c, and the head of the material's own list of 2D texture maps (+0x6ec).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatRef {
    pub id: u32,
    pub maps: Option<usize>,
}

/// The arguments of [`mesh_triangles`] in the original's order.
pub struct MeshTrianglesArgs<'a> {
    /// 1: the model. Only its texture coordinate sets are read (+0x424, +0x428).
    pub model: Option<&'a UvDomains>,
    /// 2: the mesh whose positions go to a vertex's +0x20.
    pub positions: &'a Mesh,
    /// 4, 5: the node's materials.
    pub n_materials: i32,
    pub materials: Option<&'a [MatRef]>,
    /// 6: the node's element number, stored in 16 bits at a triangle's +0x58.
    pub elem_id: i32,
    /// 9: whether the pairs of the further texture coordinate sets are wanted.
    pub want_uv2: bool,
    /// 12: the head of the model's own list of 2D texture maps.
    pub model_maps: Option<usize>,
    /// 13: non-zero makes every edge a discontinuity.
    pub all_hard: i16,
    /// 14, 15: whether the per-polygon triangle table and the per-corner vertex table are
    /// wanted.
    pub want_poly_tris: bool,
    pub want_node_verts: bool,
    /// Not an argument of the original: refuse a mesh for which it makes a triangle corner
    /// that points before the vertex array, instead of taking that triangle back and
    /// counting it in [`NodeRender::wild_corners`].
    pub strict: bool,
}

/// What [`mesh_triangles`] returns besides the node's arrays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshTriangles {
    pub node: NodeRender,
    /// `*param_14` (`SI_PolyTriangles`, 8 bytes a polygon): the count the triangulator
    /// gave and one entry per triangle: the record it became, `None` for a triangle that
    /// was dropped (a word the original leaves zero). `None` for the whole when the
    /// polygon is hidden or was refused (a record left zero).
    pub poly_tris: Option<Vec<Option<Vec<Option<usize>>>>>,
    /// `*param_15` (`SI_NodeRVertex`, 8 bytes a corner): per polygon, per corner, the
    /// render vertices made from that corner.
    pub node_verts: Option<Vec<Vec<Vec<usize>>>>,
}

/// The double 0.01 (0x086543e8): two corner normals further apart are a discontinuity.
const NORMAL_EPS: f64 = 0.01;
/// The double 0.001 (0x086543f0): the same for a texture coordinate.
const UV_EPS: f64 = 0.001;
/// The mark `SI_meshTriangles` puts on an edge across which vertices are not shared.
const MARK: i16 = 0x8000u16 as i16;

fn edge_at<'a>(mesh: &'a Mesh, n: i32) -> Result<&'a Edge, Wild> {
    usize::try_from(n).ok().and_then(|k| mesh.edges.get(k)).ok_or(Wild::Index("edge"))
}

fn polygon_at<'a>(mesh: &'a Mesh, p: i32) -> Result<&'a Polygon, Wild> {
    usize::try_from(p).ok().and_then(|k| mesh.polygons.get(k)).ok_or(Wild::Index("polygon"))
}

/// The walk round the star of vertex `v` that both passes of `SI_meshTriangles` make.
/// `f(j, new)` is called for every star entry the walk visits, in its order; `new` says
/// that the entry starts a render vertex.
///
/// The star is cut into groups as `SI_meshComputesVertexNormal` cuts it: a group ends
/// before entry i (0 < i < n, and i past the group's first entry) when the edge of entry i
/// has no polygon on its far side (the second side for an edge this vertex starts, the
/// first otherwise; edge number 0 counts as "otherwise") or that polygon is not entry
/// i − 1's; it ends before an entry whose vertex is −1 (which is then passed over) and at
/// the end. A group ended in the first or the last way is walked from its first entry
/// whose edge is marked (from its first entry when none is), round to where it started; a
/// group ended by a −1 entry from its first entry. An entry with a polygon starts a render
/// vertex when it is the entry the walk started from or its edge is marked.
fn walk_star(mesh: &Mesh, v: usize, f: &mut dyn FnMut(usize, bool) -> Result<(), Wild>) -> Result<(), Wild> {
    let star = &mesh.vertices[v].star;
    let n = star.len() as u16 as i32;
    if n == 0 {
        return Ok(());
    }
    let marked = |e: i32| -> Result<bool, Wild> { Ok(edge_at(mesh, e.wrapping_abs())?.flag < 0) };
    let (mut cut, mut start, mut i) = (false, 0i32, 0i32);
    loop {
        if start < i && i < n {
            let e = star[i as usize].edge;
            let beyond = if e < 1 { edge_at(mesh, e.wrapping_neg())?.p0 } else { edge_at(mesh, e)?.p1 };
            if beyond == -1 || beyond != star[i as usize - 1].polygon {
                cut = true;
            }
        }
        if cut || i == n || star[i as usize].vertex == -1 {
            let last = i - 1;
            let mut first = start;
            if (cut || i == n) && start <= last {
                for k in start..=last {
                    if marked(star[k as usize].edge)? {
                        first = k;
                        break;
                    }
                }
            }
            let mut j = first;
            loop {
                // with an empty group (start past last) the original walks on through
                // the memory behind the star
                let ent = usize::try_from(j).ok().and_then(|k| star.get(k)).ok_or(Wild::StarOverrun)?;
                let new = ent.polygon != -1 && (j == first || marked(ent.edge)?);
                f(j as usize, new)?;
                j = if j == last { start } else { j + 1 };
                if j == first {
                    break;
                }
            }
            start = if cut { i } else { i + 1 };
            cut = false;
        }
        i += 1;
        if i > n {
            return Ok(());
        }
    }
}

/// A polygon mesh as render vertices and triangles [soft 0x0826a130 SI_meshTriangles]:
/// `SI_meshTriangles(model, positions, mesh, n_materials, materials, elem_id, &n_verts,
/// &verts, &uv2, &n_tns, &tns, model_maps, all_hard, &poly_tris, &node_verts)`.
///
/// When `SI_meshNbTriangles(mesh)` is 0 the routine returns at once (`None`): it has set
/// the triangle count to 0 and the triangle array to null and cleared the two tables'
/// pointers; **the vertex count, the vertex array and the further-pairs pointer are not
/// written**, and the edges keep whatever mark they came with. Otherwise, in this order:
///
/// 1. **Edges are marked** (bit 15 of the flag word, cleared first): a border (0x20), a
///    discontinuity found from the angle (0x10) or set by the user (0x08); every edge of a
///    faceted mesh (mesh flag 0x2) or when `all_hard` is set; with more than one material,
///    an edge between two polygons of different materials (a number outside the node's
///    materials counts as 0) of which one has a list of 2D maps that is not the model's.
/// 2. **Corner data decide the rest**: an unmarked edge between two polygons, not a
///    border, is marked when at either of its ends the two polygons' corners there (the
///    first corner of each polygon at that vertex) differ in colour, or their normals are
///    more than 0.01 apart (√((Δx² + Δy²) + Δz²) in the register), or, in any texture
///    coordinate set in use, u or v differs by more than 0.001. The first end that marks
///    the edge ends its test.
/// 3. **Render vertices are counted and made** by the same walk round every vertex's star
///    ([`walk_star`]): one per run of polygons between two marked edges. A vertex gets:
///    the position (+0x08) from `mesh` and (+0x20) from `positions`; from the polygon's
///    first corner at this vertex the normal (+0x14), the colour (+0x44) and the texture
///    coordinates: without further sets, (1 − u, v) at +0x00; with them, every set's pair
///    through [`rvtx_set_uv`], u as 1 − u; on a faceted mesh the polygon's normal
///    instead of the corner's. Every star entry visited is given the number of the vertex
///    current at that moment.
/// 4. **Triangles**: `SI_meshNbTriangles` records, each linked to the next, with the
///    element number and the node's first material. Every polygon that is not hidden
///    (flag 0x800): its distinct vertices in order of first appearance, through
///    `SI_matrixZAlign` of its normal, to `SI_triangulateContour`; a polygon the
///    triangulator refuses gives nothing. Each triangle's corner is the render vertex of
///    the **last** star entry of that mesh vertex that names the polygon; its material is
///    the polygon's when that number is one of the node's. A triangle with a corner
///    still null is taken back (its record zeroed in the corners and the material, and
///    used for the next triangle).
/// 5. The last triangle's link is cleared, the marks are taken off the edges.
///
/// Where the original leaves its arrays the port returns a [`Wild`]. Class: model (the
/// three comparisons of step 2 and the turn into the plane).
pub fn mesh_triangles(a: &MeshTrianglesArgs, mesh: &mut Mesh) -> Result<Option<MeshTriangles>, Wild> {
    let mut out = MeshTriangles::default();
    let alloc = mesh_nb_triangles(mesh);
    if alloc == 0 {
        return Ok(None);
    }
    let ne = mesh.nb_edges();
    // 1
    for n in 1..=ne as usize {
        let f = mesh.edges[n].flag;
        mesh.edges[n].flag = if f & 0x38 != 0 { (f & 0x7fff) | MARK } else { f & 0x7fff };
    }
    if mesh.flags & 2 != 0 || a.all_hard != 0 {
        for n in 1..=ne as usize {
            mesh.edges[n].flag |= MARK;
        }
    }
    if a.n_materials > 1 {
        for n in 1..=ne as usize {
            let e = mesh.edges[n];
            if e.flag < 0 || e.p0 == -1 || e.p1 == -1 {
                continue;
            }
            let held = |m: i16| if m < 0 || m as i32 >= a.n_materials { 0usize } else { m as usize };
            let (m0, m1) = (held(polygon_at(mesh, e.p0)?.material), held(polygon_at(mesh, e.p1)?.material));
            if m0 != m1 {
                let mats = a.materials.ok_or(Wild::NullMaterials)?;
                for m in [m0, m1] {
                    let maps = mats.get(m).ok_or(Wild::Index("material"))?.maps;
                    if maps.is_some() && maps != a.model_maps {
                        mesh.edges[n].flag |= MARK;
                    }
                }
            }
        }
    }
    // 2
    for n in 1..=ne as usize {
        let e = mesh.edges[n];
        if e.flag < 0 || e.flag & 0x20 != 0 || e.p0 == -1 || e.p1 == -1 {
            continue;
        }
        let (pa, pb) = (polygon_at(mesh, e.p0)?, polygon_at(mesh, e.p1)?);
        let mut mark = false;
        for end in [e.v0, e.v1] {
            let (Some(ia), Some(ib)) = (pa.nodes.iter().position(|c| c.vertex == end), pb.nodes.iter().position(|c| c.vertex == end)) else {
                continue;
            };
            let (ca, cb) = (&pa.nodes[ia], &pb.nodes[ib]);
            mark = ca.color != cb.color || {
                let d = [x(ca.normal[0]) - cb.normal[0], x(ca.normal[1]) - cb.normal[1], x(ca.normal[2]) - cb.normal[2]];
                ((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]).sqrt().gt(x(NORMAL_EPS))
            };
            if !mark {
                let model = a.model.ok_or(Wild::NullModel)?;
                for d in 0..model.count {
                    if mesh_get_uv_domain_active(a.model, d as i16) {
                        let (ua, va) = node_get_uv(ca, d);
                        let (ub, vb) = node_get_uv(cb, d);
                        if (x(ub) - ua).abs().gt(x(UV_EPS)) || (x(vb) - va).abs().gt(x(UV_EPS)) {
                            mark = true;
                            break;
                        }
                    }
                }
            }
            if mark {
                break;
            }
        }
        if mark {
            mesh.edges[n].flag |= MARK;
        }
    }
    // 3: the count
    let nv = mesh.vertices.len();
    let mut count = 0usize;
    for v in 0..nv {
        walk_star(mesh, v, &mut |_, new| {
            count += new as usize;
            Ok(())
        })?;
    }
    let mut verts = vec![RVertex { uv: NO_UV, p48: NO_UV, ..RVertex::default() }; count];
    // the number of further sets: one less than the model's sets, 1 without a model
    let e4: i32 = match a.model {
        Some(m) => m.count.wrapping_sub(1),
        None => 1,
    };
    // SI_calloc answers null for no records (and the C library for a size it cannot give:
    // a negative number of sets)
    let pairs = (count as u32).wrapping_mul(e4 as u32);
    let mut uv2: Option<Vec<[f32; 2]>> = if a.want_uv2 && e4 != 0 && pairs != 0 && pairs < 0x2000_0000 { Some(vec![[0.0; 2]; pairs as usize]) } else { None };
    // 3: the vertices
    let mut map: Vec<Vec<i32>> = mesh.vertices.iter().map(|v| vec![-1; v.star.len()]).collect();
    let mut made: Vec<(i32, usize, usize)> = Vec::new();
    let mut b4: i32 = -1;
    for v in 0..nv {
        let mesh_ref: &Mesh = mesh;
        let (verts, uv2, map, made, b4) = (&mut verts, &mut uv2, &mut map, &mut made, &mut b4);
        walk_star(mesh_ref, v, &mut |j, new| {
            if new {
                *b4 += 1;
                let at = *b4 as usize;
                let rv = &mut verts[at];
                rv.raw = Vector::from_array(mesh_ref.vertices[v].pos);
                rv.world = Vector::from_array(a.positions.vertices.get(v).ok_or(Wild::Index("positions"))?.pos);
                let pi = mesh_ref.vertices[v].star[j].polygon;
                let poly = polygon_at(mesh_ref, pi)?;
                if let Some(ci) = poly.nodes.iter().position(|c| c.vertex == v as i32) {
                    let c = &poly.nodes[ci];
                    rv.raw_normal = Vector::from_array(c.normal);
                    rv.w44 = c.color;
                    match uv2 {
                        Some(pool) if e4 != 0 => {
                            let first = (at as u32).wrapping_mul(e4 as u32) as usize;
                            rv.p48 = first as u32;
                            for d in 0..=e4 {
                                let (u, w) = node_get_uv(c, d);
                                let u = (F80::ONE - u).to_f32();
                                rvtx_set_uv(rv, d, &mut pool[first..first + e4 as usize], u, fmov(w));
                            }
                        }
                        _ => rv.w00 = [(F80::ONE - c.uv[0]).to_f32().to_bits(), fmov(c.uv[1]).to_bits()],
                    }
                    if a.want_node_verts {
                        made.push((pi, ci, at));
                    }
                }
                if mesh_ref.flags & 2 != 0 {
                    rv.raw_normal = Vector::from_array(poly.normal);
                }
            }
            map[v][j] = *b4;
            Ok(())
        })?;
    }
    if a.want_node_verts {
        let mut table: Vec<Vec<Vec<usize>>> = mesh.polygons.iter().map(|p| vec![Vec::new(); p.nodes.len()]).collect();
        for (pi, ci, at) in made {
            table[pi as usize][ci].push(at);
        }
        out.node_verts = Some(table);
    }
    // 4
    let alloc = usize::try_from(alloc).map_err(|_| Wild::Count)?;
    let first_material = a.materials.map_or(Ok(NO_MATERIAL), |m| m.first().map(|r| r.id).ok_or(Wild::Index("material")))?;
    let mut tns: Vec<Tn> = (0..alloc)
        .map(|i| Tn { v: [None; 3], next: if i + 1 == alloc { None } else { Some(i + 1) }, w58: a.elem_id as i16, material: first_material, ..Tn::default() })
        .collect();
    let mut poly_tris = a.want_poly_tris.then(|| vec![None; mesh.polygons.len()]);
    let mut used = 0usize;
    let mut wild_corners = 0u32;
    for p in 0..mesh.polygons.len() {
        if mesh.polygons[p].flag & 0x800 != 0 {
            continue;
        }
        for c in &mesh.polygons[p].nodes {
            if c.vertex != -1 && usize::try_from(c.vertex).map_or(true, |k| k >= nv) {
                return Err(Wild::Index("corner"));
            }
        }
        let mut pp = polygon_points(mesh, p);
        let (ok, tris) = triangulate_contour(&mut pp.points, &mut pp.indices);
        if !ok {
            continue;
        }
        let mut row = vec![None; tris.len()];
        let material = mesh.polygons[p].material;
        for (t, tri) in tris.iter().enumerate() {
            // a triangle beyond the count is a write past the array in the original
            let cur = tns.get_mut(used).ok_or(Wild::TriangleOverrun)?;
            let mut wild = false;
            for k in 0..3 {
                let vtx = *usize::try_from(tri[k]).ok().and_then(|i| pp.vertices.get(i)).ok_or(Wild::Index("triangle"))? as usize;
                for (s, ent) in mesh.vertices[vtx].star.iter().enumerate() {
                    if ent.polygon == p as i32 {
                        match usize::try_from(map[vtx][s]) {
                            Ok(i) => cur.v[k] = Some(i),
                            // the original stores the address 0x4c bytes before the array
                            Err(_) if a.strict => return Err(Wild::VertexBeforeArray),
                            Err(_) => wild = true,
                        }
                    }
                }
            }
            if material >= 0 && (material as i32) < a.n_materials {
                cur.material = a.materials.ok_or(Wild::NullMaterials)?.get(material as usize).ok_or(Wild::Index("material"))?.id;
            }
            if wild {
                // not the original's: there the triangle is kept with that corner
                wild_corners += 1;
            }
            if wild || cur.v.iter().any(|c| c.is_none()) {
                cur.v = [None; 3];
                cur.material = NO_MATERIAL;
            } else {
                row[t] = Some(used);
                used += 1;
            }
        }
        if let Some(table) = &mut poly_tris {
            table[p] = Some(row);
        }
    }
    if used != 0 {
        tns[used - 1].next = None;
    }
    for n in 1..=ne as usize {
        mesh.edges[n].flag &= 0x7fff;
    }
    out.poly_tris = poly_tris;
    out.node = NodeRender { verts, tns, tn_count: used as i32, uv2, uv2_stride: e4, tcoords: Vec::new(), wild_corners };
    Ok(Some(out))
}

/// The box of a node shown as its bounding box [soft 0x08269320 SI_bounding2triangle]:
/// `SI_bounding2triangle(model)`. Nothing for a node of kind −1, 0 or 5. Otherwise 24
/// vertices, four a side, in the order of the sides y max, y min, z max, z min, x min,
/// x max, each with that side's outward unit normal as raw normal, the position also
/// copied to +0x20, and a null pointer at +0x40; and 12 triangles, two a side: corners
/// (0, 1, 2) and (0, 2, 3) of the side's four, linked in order, with the element number
/// and the node's first material (null without a material array).
///
/// The vertex array comes from `SI_malloc`: the members not named above are whatever the
/// block held (the Rust has zeros), except that `SI_malloc` writes the byte 0x6f to a
/// block's first byte and 0x6b to its last: the low byte of the first vertex's +0x00 and
/// the high byte of the last vertex's +0x48 (the latter is [`NO_UV`] here). An allocation
/// that fails is fatal (`SYSALLOC failed, not enough system memory`). Class: copy.
pub fn bounding2triangle(kind: i32, bbox_min: Vector, bbox_max: Vector, elem_id: i32, first_material: Option<u32>) -> Option<NodeRender> {
    if kind == -1 || kind == 0 || kind == 5 {
        return None;
    }
    let (x0, y0, z0) = (bbox_min.x, bbox_min.y, bbox_min.z);
    let (x1, y1, z1) = (bbox_max.x, bbox_max.y, bbox_max.z);
    let sides: [([[f32; 3]; 4], [f32; 3]); 6] = [
        ([[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]], [0.0, 1.0, 0.0]),
        ([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], [0.0, -1.0, 0.0]),
        ([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], [0.0, 0.0, 1.0]),
        ([[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]], [0.0, 0.0, -1.0]),
        ([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]], [-1.0, 0.0, 0.0]),
        ([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], [1.0, 0.0, 0.0]),
    ];
    let mut verts = Vec::with_capacity(24);
    for (corners, normal) in sides {
        for c in corners {
            let p = Vector::from_array(c);
            verts.push(RVertex { raw: p, world: p, raw_normal: Vector::from_array(normal), uv: NO_UV, p48: NO_UV, ..RVertex::default() });
        }
    }
    verts[0].w00[0] = 0x6f;
    let mut tns = Vec::with_capacity(12);
    for s in 0..6 {
        for (b, c) in [(1, 2), (2, 3)] {
            let i = tns.len();
            tns.push(Tn {
                v: [Some(4 * s), Some(4 * s + b), Some(4 * s + c)],
                next: if i < 11 { Some(i + 1) } else { None },
                w58: elem_id as i16,
                material: first_material.unwrap_or(NO_MATERIAL),
                ..Tn::default()
            });
        }
    }
    Some(NodeRender { verts, tns, tn_count: 12, ..NodeRender::default() })
}

/// The surface `SI_metaSysGetSurface` leaves in a metaball system: `SI_MetaSystem` +0x4c
/// (vertex count), +0x50 (72 bytes a vertex: nine doubles, of which the first three are the
/// position and the next three the normal), +0x54 (triangle count) and +0x58 (32 bytes a
/// triangle; the word at +4 points to its three vertex numbers).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaSurface {
    pub verts: Vec<[f64; 9]>,
    /// `None` is a null triangle array (+0x58).
    pub tris: Option<Vec<[i32; 3]>>,
}

/// A metaball system as render vertices and triangles [soft 0x0826bd40 SI_metaTriangles]:
/// `SI_metaTriangles(system, materials, elem_id, &n_verts, &verts, &n_tns, &tns, flag)`
/// after `SI_metaSysGetSurface(system, 1, flag)` (not ported: `surface` is what it left).
/// Nothing is written without a triangle array. Each vertex: position and normal rounded
/// to float, the position also at +0x20, +0x40 null and +0x44 the word 0xffb2b2b2 (the
/// vertex colour a new mesh corner has: opaque grey); each triangle: the surface triangle's vertices **in reverse order**,
/// linked in order, with the element number and the first material. Both arrays come from
/// `SI_malloc` (members not named are whatever the block held; zeros here, but for the
/// byte 0x6f the allocator writes to the vertex block's first byte), and a surface with a
/// triangle array but no vertex or no triangle is fatal ([`Wild::FatalAlloc`]).
/// Class: single.
pub fn meta_triangles(surface: &MetaSurface, first_material: Option<u32>, elem_id: i32) -> Result<Option<NodeRender>, Wild> {
    let Some(tris) = &surface.tris else { return Ok(None) };
    if surface.verts.is_empty() || tris.is_empty() {
        return Err(Wild::FatalAlloc);
    }
    let verts: Vec<RVertex> = surface
        .verts
        .iter()
        .map(|d| {
            let p = Vector::new(x(d[0]).to_f32(), x(d[1]).to_f32(), x(d[2]).to_f32());
            RVertex { raw: p, world: p, raw_normal: Vector::new(x(d[3]).to_f32(), x(d[4]).to_f32(), x(d[5]).to_f32()), uv: NO_UV, w44: 0xffb2_b2b2, p48: NO_UV, ..RVertex::default() }
        })
        .collect();
    let mut verts = verts;
    verts[0].w00[0] = 0x6f;
    let n = tris.len();
    let mut tns = Vec::with_capacity(n);
    for (i, t) in tris.iter().enumerate() {
        let corner = |k: i32| usize::try_from(k).ok().filter(|&k| k < verts.len()).ok_or(Wild::Index("metaball vertex"));
        tns.push(Tn {
            v: [Some(corner(t[2])?), Some(corner(t[1])?), Some(corner(t[0])?)],
            next: if i + 1 < n { Some(i + 1) } else { None },
            w58: elem_id as i16,
            material: first_material.unwrap_or(NO_MATERIAL),
            ..Tn::default()
        });
    }
    Ok(Some(NodeRender { verts, tn_count: n as i32, tns, ..NodeRender::default() }))
}

/// A face as render vertices and triangles [soft 0x08269894 SI_face2triangle]:
/// `SI_face2triangle(positions_face, face, materials, elem_id, &n_verts, &verts, &n_tns,
/// &tns)`. The results are cleared; `SI_faceConvertToMesh(face, 0)` makes the face one
/// polygon with holes (`to_mesh(false)`; nothing more when it gives none); when the two
/// faces are not the same record the other one is converted too (`to_mesh(true)`) and
/// lends its positions; then [`mesh_triangles`] with no model, a material count of 1 with
/// a material array and 0 without, no further texture coordinates, no map list, no
/// tables. (The meshes are disposed of.)
///
/// With no model `SI_meshTriangles` would read a null record at the first edge that has a
/// polygon on both sides and is not marked; a face's one polygon has only borders.
pub fn face2triangle(same_face: bool, to_mesh: &mut dyn FnMut(bool) -> Option<Mesh>, materials: Option<&[MatRef]>, elem_id: i32) -> Result<NodeRender, Wild> {
    let Some(mut mesh) = to_mesh(false) else { return Ok(NodeRender::default()) };
    let other = if same_face { None } else { to_mesh(true) };
    // a second face that converts to nothing is a null mesh read by the original
    let positions = match (&other, same_face) {
        (Some(m), _) => m.clone(),
        (None, true) => mesh.clone(),
        (None, false) => return Err(Wild::Index("positions")),
    };
    let args = MeshTrianglesArgs {
        model: None,
        positions: &positions,
        n_materials: materials.is_some() as i32,
        materials,
        elem_id,
        want_uv2: false,
        model_maps: None,
        all_hard: 0,
        want_poly_tris: false,
        want_node_verts: false,
        strict: true,
    };
    Ok(mesh_triangles(&args, &mut mesh)?.map(|m| m.node).unwrap_or_default())
}

/// A patch as render vertices and triangles [soft 0x08269988 si_patch2triangle]:
/// `si_patch2triangle(shape, patch, 0, materials, element, &vertex count, &vertices,
/// &triangle count, &triangles, flag)`. `shape` is the patch that lends the positions when
/// it is another record than `patch` (`None`: the same record).
///
/// 1. `SI_splineNbSegmentStep` gives the segments in u and in v (types +4, +8; point
///    counts +0xc, +0xe; closed +0x10, +0x12). With `flag` above 0 the steps of both
///    patches (+0x1c, +0x1e) become `flag` and the patch's subdivision is marked stale
///    (+0x1c0 = 0); they are put back at the end.
/// 2. u step × u segments × v step × v segments = 0: no vertices, no triangles (and the
///    steps a `flag` changed are **not** put back: the routine has left).
/// 3. Rows in u: step × segments + 1, or step × segments + segments for a type below 2
///    (linear, Bézier: every segment has its own first and last row); columns in v
///    likewise. The vertex array is rows × columns records of `SI_malloc`.
/// 4. `SI_patchComputesSubdivision(patch)` unless the mark at +0x1c0 is set;
///    `SI_patchSurface(shape)` when the shape is another record.
/// 5. With A = r32(1 / u segments), B = r32(1 / v segments), du = r32(A / u step), dv =
///    r32(B / v step): for each u segment i, each step j (0 … step, the last left out
///    unless the type is below 2 or the segment is the last), each v segment k and each
///    step l likewise, one vertex: +0x00 = u, +0x04 = v, where **u starts from A × i and v
///    from B × k in the register and each grows by du or dv there**, rounded only where it
///    is stored; +0x08 and +0x20 the subdivision's point ((v step + 1) × ((u step + 1) × (i
///    × v segments + k) + j) + l), +0x14 its normal; +0x40 null; +0x44 = 0xffb2b2b2. With
///    another shape, +0x20 is the shape's surface point ((j + u step × i) mod its rows,
///    (v step × k) mod its columns): **l is not in the column**, so the points of one
///    cell's row all get the row's first position.
/// 6. Triangles: 2 × u step × v step × u segments × v segments records of `SI_malloc`,
///    linked in order, each with the element's number (+0x58) and **the first material**
///    (+0x4c; null without a material array). Two a cell: (p, right, right-below) and (p,
///    right-below, below), p the cell's vertex, "right" the next column (back to the row's
///    first after the last column), "below" the next row (back to the first row after the
///    last): a patch closed in a direction joins up there, and an open one's last cells do
///    not exist. Nothing else of a triangle record is set.
///
/// `Err` where the original divides by zero (a shape without rows or columns) or reads
/// outside the subdivision's arrays. Class: single (the two parameters).
pub fn patch2triangle(mut shape: Option<&mut si_spline::patch::Surface>, patch: &mut si_spline::patch::Surface, materials: Option<&[MatRef]>, elem_id: i32, flag: i16) -> Result<NodeRender, Wild> {
    let r = patch.rec;
    let (nu_seg, _) = si_spline::eval::nb_segment_step(r.u_type, r.nb_u as i32, r.u_closed);
    let (nv_seg, _) = si_spline::eval::nb_segment_step(r.v_type, r.nb_v as i32, r.v_closed);
    let kept = (r.u_step, r.v_step);
    // (a positive flag: read from the listing; the emulator's cases come through
    // SI_collect, which passes -1, and do not reach this branch or its counterpart below)
    if flag > 0 {
        (patch.rec.u_step, patch.rec.v_step) = (flag as u16, flag as u16);
        if let Some(s) = shape.as_deref_mut() {
            (s.rec.u_step, s.rec.v_step) = (flag as u16, flag as u16);
        }
        patch.rec.unk_1c0 = 0;
    }
    let (us, vs) = (patch.rec.u_step as i32, patch.rec.v_step as i32);
    if (us as u32).wrapping_mul(nu_seg as u32).wrapping_mul(vs as u32).wrapping_mul(nv_seg as u32) == 0 {
        return Ok(NodeRender::default());
    }
    let (open_u, open_v) = ((r.u_type as u32) < 2, (r.v_type as u32) < 2);
    let rows = if open_u { us.wrapping_mul(nu_seg).wrapping_add(nu_seg) } else { us.wrapping_mul(nu_seg).wrapping_add(1) };
    let columns = if open_v { vs.wrapping_mul(nv_seg).wrapping_add(nv_seg) } else { vs.wrapping_mul(nv_seg).wrapping_add(1) };
    let total = rows.wrapping_mul(columns);
    if total <= 0 || nu_seg < 0 || nv_seg < 0 {
        return Err(Wild::Count);
    }
    if patch.rec.unk_1c0 == 0 {
        si_spline::patch_grid::computes_subdivision(patch);
    }
    let grid = shape.as_deref().and_then(si_spline::patch_grid::patch_surface);
    let a = (F80::ONE / x(nu_seg)).to_f32();
    let b = (F80::ONE / x(nv_seg)).to_f32();
    let du = (x(a) / x(us)).to_f32();
    let dv = (x(b) / x(vs)).to_f32();
    let v3 = |p: &si_spline::Vector| Vector::new(p.x, p.y, p.z);
    let mut verts = Vec::with_capacity(total as usize);
    for i in 0..nu_seg {
        let last_j = if open_u || i == nu_seg - 1 { us } else { us - 1 };
        let mut u = x(a) * x(i);
        for j in 0..=last_j {
            let next_u = u + du;
            for k in 0..nv_seg {
                let last_l = if open_v || k == nv_seg - 1 { vs } else { vs - 1 };
                let mut v = x(b) * x(k);
                for l in 0..=last_l {
                    let at = (vs + 1).wrapping_mul((us + 1).wrapping_mul(i.wrapping_mul(nv_seg).wrapping_add(k)).wrapping_add(j)).wrapping_add(l);
                    let point = usize::try_from(at).ok().and_then(|n| patch.subdiv0.get(n)).ok_or(Wild::Index("subdivision"))?;
                    let normal = patch.subdiv1.get(at as usize).ok_or(Wild::Index("subdivision"))?;
                    let mut world = v3(point);
                    if let Some(g) = &grid {
                        let (nu, nv) = (g.nu as i32, g.nv as i32);
                        if nu == 0 || nv == 0 {
                            return Err(Wild::Index("a shape without points"));
                        }
                        // (the remainders are C's: of a non-negative number here)
                        let n = (vs.wrapping_mul(k) % nv).wrapping_add((j.wrapping_add(us.wrapping_mul(i)) % nu).wrapping_mul(nv));
                        world = v3(usize::try_from(n).ok().and_then(|n| g.points.get(n)).ok_or(Wild::Index("shape"))?);
                    }
                    verts.push(RVertex { w00: [u.to_f32().to_bits(), v.to_f32().to_bits()], raw: v3(point), raw_normal: v3(normal), world, uv: NO_UV, w44: 0xffb2_b2b2, p48: NO_UV, ..RVertex::default() });
                    v = v + dv;
                }
            }
            u = next_u;
        }
    }
    if verts.len() != total as usize {
        // (the loops fill fewer or more records than were allocated)
        return Err(Wild::Count);
    }
    let count = (us as u32).wrapping_mul(vs as u32).wrapping_mul(2).wrapping_mul(nu_seg as u32).wrapping_mul(nv_seg as u32) as i32;
    if count <= 0 {
        return Err(Wild::Count);
    }
    let material = materials.and_then(|m| m.first()).map_or(NO_MATERIAL, |m| m.id);
    let mut tns: Vec<Tn> = (0..count as usize).map(|t| Tn { material, w58: elem_id as i16, next: (t + 1 < count as usize).then_some(t + 1), ..Tn::default() }).collect();
    let (per_u, per_v) = (if open_u { us + 1 } else { us }, if open_v { vs + 1 } else { vs });
    let mut t = 0usize;
    for i in 0..nu_seg {
        for j in 0..us {
            let row = i.wrapping_mul(per_u).wrapping_add(j);
            for k in 0..nv_seg {
                let mut column = per_v.wrapping_mul(k);
                let first = row.wrapping_mul(columns).wrapping_add(column);
                for l in 0..vs {
                    column = column.wrapping_add(1);
                    let p = first.wrapping_add(l);
                    let (mut right, mut below, mut across) = (p.wrapping_add(1), p.wrapping_add(columns), p.wrapping_add(columns).wrapping_add(1));
                    if column == columns {
                        right = right.wrapping_sub(column);
                        across = across.wrapping_sub(column);
                    }
                    if row.wrapping_add(1) == rows {
                        across = across.wrapping_sub(total);
                        below = below.wrapping_sub(total);
                    }
                    let corner = |n: i32| usize::try_from(n).ok().filter(|n| *n < verts.len()).ok_or(Wild::Index("corner"));
                    if t + 1 >= tns.len() {
                        return Err(Wild::TriangleOverrun);
                    }
                    tns[t].v = [Some(corner(p)?), Some(corner(right)?), Some(corner(across)?)];
                    tns[t + 1].v = [Some(corner(p)?), Some(corner(across)?), Some(corner(below)?)];
                    t += 2;
                }
            }
        }
    }
    if flag > 0 {
        (patch.rec.u_step, patch.rec.v_step) = kept;
        if let Some(s) = shape.as_deref_mut() {
            (s.rec.u_step, s.rec.v_step) = kept;
        }
        patch.rec.unk_1c0 = 0;
    }
    Ok(NodeRender { verts, tns, tn_count: count, ..NodeRender::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    use si_model::mesh_b::{update_nodes, MeshEnv};

    /// A flat patch of straight segments, 2 by 2 points, one step each way.
    fn flat_patch(u_step: u16) -> si_spline::patch::Surface {
        let mut s = si_spline::patch::Surface::new();
        (s.rec.u_type, s.rec.v_type, s.rec.nb_u, s.rec.nb_v) = (0, 0, 2, 2);
        (s.rec.u_step, s.rec.v_step) = (u_step, 1);
        s.points = vec![[0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0]];
        s.rec.points = si_types::Ptr32::new(1);
        s
    }

    #[test]
    fn a_patch_of_one_cell_is_four_vertices_and_two_triangles() {
        let mut s = flat_patch(1);
        let r = patch2triangle(None, &mut s, Some(&[MatRef { id: 3, maps: None }]), 5, -1).unwrap();
        assert_eq!((r.verts.len(), r.tn_count, r.tns.len()), (4, 2, 2));
        // the parameters at +0x00 and +0x04: the row's u, the column's v
        let uv: Vec<[f32; 2]> = r.verts.iter().map(|v| [f32::from_bits(v.w00[0]), f32::from_bits(v.w00[1])]).collect();
        assert_eq!(uv, [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]]);
        assert!(r.verts.iter().all(|v| v.w44 == 0xffb2_b2b2 && v.uv == NO_UV && v.raw == v.world));
        // (p, right, across), (p, across, below); linked; the first material, the element
        assert_eq!(r.tns[0].v, [Some(0), Some(1), Some(3)]);
        assert_eq!(r.tns[1].v, [Some(0), Some(3), Some(2)]);
        assert_eq!((r.tns[0].next, r.tns[1].next), (Some(1), None));
        assert!(r.tns.iter().all(|t| t.material == 3 && t.w58 == 5));
        // the subdivision was made on the way
        assert_ne!(s.rec.unk_1c0, 0);
    }

    #[test]
    fn a_patch_with_a_step_of_zero_gives_nothing() {
        let mut s = flat_patch(0);
        let r = patch2triangle(None, &mut s, None, 0, -1).unwrap();
        assert_eq!((r.verts.len(), r.tn_count), (0, 0));
    }

    #[test]
    fn a_positive_flag_replaces_the_steps_and_puts_them_back() {
        let mut s = flat_patch(1);
        let r = patch2triangle(None, &mut s, None, 0, 2).unwrap();
        // two steps each way: three rows of three vertices, eight triangles
        assert_eq!((r.verts.len(), r.tn_count), (9, 8));
        assert_eq!((s.rec.u_step, s.rec.v_step, s.rec.unk_1c0), (1, 1, 0));
    }

    /// Two triangles on the unit square, sharing the edge 0-2.
    fn square() -> Mesh {
        let mut m = Mesh::with_vertices(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        m.push_polygon(&[0, 1, 2]);
        m.push_polygon(&[0, 2, 3]);
        update_nodes(&mut m, true, &mut MeshEnv::default());
        m
    }

    fn args<'a>(positions: &'a Mesh, model: &'a UvDomains) -> MeshTrianglesArgs<'a> {
        MeshTrianglesArgs { model: Some(model), positions, n_materials: 0, materials: None, elem_id: 7, want_uv2: true, model_maps: None, all_hard: 0, want_poly_tris: true, want_node_verts: false, strict: true }
    }

    #[test]
    fn a_smooth_edge_shares_its_vertices() {
        let mut m = square();
        let (p, model) = (m.clone(), UvDomains { count: 1, sets: Vec::new() });
        let got = mesh_triangles(&args(&p, &model), &mut m).unwrap().unwrap();
        // four mesh vertices, coplanar polygons, the same corner data: four render vertices
        assert_eq!((got.node.verts.len(), got.node.tn_count, got.node.tns.len()), (4, 2, 2));
        assert_eq!(got.node.tns[0].next, Some(1));
        assert_eq!(got.node.tns[1].next, None);
        assert_eq!(got.node.tns[0].w58, 7);
        assert_eq!(got.node.tns[0].material, NO_MATERIAL);
        // the two triangles use the same render vertices at the shared edge
        let shared: Vec<_> = got.node.tns[0].v.iter().filter(|c| got.node.tns[1].v.contains(c)).collect();
        assert_eq!(shared.len(), 2);
        // u is stored as 1 - u, and no further pairs with one set
        assert_eq!(got.node.verts[0].w00, [1.0f32.to_bits(), 0.0f32.to_bits()]);
        assert!(got.node.uv2.is_none());
        assert_eq!(got.poly_tris, Some(vec![Some(vec![Some(0)]), Some(vec![Some(1)])]));
        // the marks are taken off again
        assert!(m.edges.iter().all(|e| e.flag >= 0));
    }

    #[test]
    fn a_discontinuity_splits_them() {
        for what in 0..4 {
            let mut m = square();
            let mut model = UvDomains { count: 1, sets: Vec::new() };
            let mut a_hard = 0;
            match what {
                // a user discontinuity on the shared edge
                0 => {
                    let e = m.edges.iter().position(|e| e.p0 != -1 && e.p1 != -1 && e.v0 + e.v1 == 2).unwrap();
                    m.edges[e].flag |= 0x08;
                }
                // another colour in one polygon's corner at vertex 0
                1 => m.polygons[1].nodes[0].color = 0xff00_0000,
                // a texture coordinate more than 0.001 apart at vertex 2
                2 => m.polygons[1].nodes[1].uv = [0.5, 0.0],
                _ => a_hard = 1,
            }
            model.count = 1;
            let p = m.clone();
            let mut a = args(&p, &model);
            a.all_hard = a_hard;
            let got = mesh_triangles(&a, &mut m).unwrap().unwrap();
            assert_eq!(got.node.verts.len(), 6, "case {what}");
            assert!(got.node.tns[0].v.iter().all(|c| !got.node.tns[1].v.contains(c)), "case {what}");
        }
    }

    #[test]
    fn nothing_to_make_is_not_an_empty_node() {
        let mut m = square();
        for p in &mut m.polygons {
            p.flag |= 0x800;
        }
        let (p, model) = (m.clone(), UvDomains { count: 1, sets: Vec::new() });
        assert_eq!(mesh_triangles(&args(&p, &model), &mut m), Ok(None));
    }

    #[test]
    fn further_sets_go_to_their_own_array() {
        let mut m = square();
        for p in &mut m.polygons {
            for c in &mut p.nodes {
                c.uv = [0.25, 0.5];
                c.extra_uv = vec![[0.75, 0.125]];
            }
        }
        let (p, model) = (m.clone(), UvDomains { count: 2, sets: vec![vec![9]] });
        let got = mesh_triangles(&args(&p, &model), &mut m).unwrap().unwrap();
        assert_eq!(got.node.verts.len(), 4);
        assert_eq!(got.node.uv2_stride, 1);
        assert_eq!(got.node.uv2, Some(vec![[0.25, 0.125]; 4]));
        assert_eq!(got.node.verts[3].p48, 3);
        assert_eq!(rvtx_get_uv(&got.node.verts[0], 0, &[]), (0.75, 0.5));
        assert!(mesh_get_uv_domain_active(Some(&model), 1));
        assert!(!mesh_get_uv_domain_active(Some(&model), 2));
        assert_eq!(mesh_get_txt_uv_domain(Some(&model), 9), Some(1));
        assert_eq!(mesh_get_txt_uv_domain(Some(&model), 8), Some(0));
    }

    #[test]
    fn a_box_is_six_sides_of_two_triangles() {
        let r = bounding2triangle(4, Vector::new(-1.0, -2.0, -3.0), Vector::new(1.0, 2.0, 3.0), 5, Some(2)).unwrap();
        assert_eq!((r.verts.len(), r.tns.len(), r.tn_count), (24, 12, 12));
        assert_eq!(r.verts[4].raw, Vector::new(-1.0, -2.0, -3.0));
        assert_eq!(r.verts[4].raw_normal, Vector::new(0.0, -1.0, 0.0));
        assert_eq!(r.verts[23].world, Vector::new(1.0, -2.0, 3.0));
        assert_eq!(r.tns[3].v, [Some(4), Some(6), Some(7)]);
        assert_eq!((r.tns[11].next, r.tns[11].material, r.tns[11].w58), (None, 2, 5));
        assert!(bounding2triangle(5, Vector::default(), Vector::default(), 0, None).is_none());
    }

    #[test]
    fn a_metaball_surface_is_reversed() {
        let mut v = [[0.0f64; 9]; 3];
        v[1][0] = 1.0;
        v[2][1] = 1.0;
        let s = MetaSurface { verts: v.to_vec(), tris: Some(vec![[0, 1, 2]]) };
        let r = meta_triangles(&s, None, 3).unwrap().unwrap();
        assert_eq!(r.tns[0].v, [Some(2), Some(1), Some(0)]);
        assert_eq!(r.verts[1].w44, 0xffb2_b2b2);
        assert_eq!(meta_triangles(&MetaSurface { verts: v.to_vec(), tris: None }, None, 3), Ok(None));
        assert_eq!(meta_triangles(&MetaSurface { verts: v.to_vec(), tris: Some(Vec::new()) }, None, 3), Err(Wild::FatalAlloc));
    }

    #[test]
    fn pair_zero_is_in_the_vertex() {
        let mut v = RVertex::default();
        let mut extra = [[0.0f32; 2]; 2];
        rvtx_set_uv(&mut v, 0, &mut extra, 0.25, 0.5);
        rvtx_set_uv(&mut v, 2, &mut extra, 1.0, 2.0);
        assert_eq!(rvtx_get_uv(&v, 0, &extra), (0.25, 0.5));
        assert_eq!(rvtx_get_uv(&v, -3, &extra), (0.25, 0.5));
        assert_eq!(rvtx_get_uv(&v, 2, &extra), (1.0, 2.0));
        assert_eq!(extra, [[0.0, 0.0], [1.0, 2.0]]);
    }
}
