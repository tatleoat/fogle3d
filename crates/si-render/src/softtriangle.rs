// SPDX-License-Identifier: MIT
//! From a model node to its render vertices and triangles: the original's `softtriangle.c`
//! (finding 0012, section 3b). `SI_collect` is what the geometry stage calls for every node
//! it renders; it has `SI_makeVertices` pick the converter for the node's kind
//! ([`crate::softvertex`]) and `SI_textureVerts` give the vertices the texture coordinate
//! pairs the shader interpolates.
//!
//! The node's arrays are a [`NodeRender`]; [`Scene`] keeps one per node beside the source
//! geometry and hands copies to the pools of [`ShadeState`] the rest of the renderer
//! works on.

use crate::geometry::{Geometry, RenModel};
use crate::shade::{ShadeMap, ShadeState};
use crate::softvertex::{
    bounding2triangle, face2triangle, mesh_get_txt_uv_domain, mesh_triangles, meta_triangles, rvtx_get_uv, MatRef, MeshTrianglesArgs, MetaSurface, NodeRender,
    UvDomains, Wild, NO_MATERIAL, NO_UV,
};
use crate::types::{RVertex, Tn, Vector};
use si_model::mesh::Mesh;
use si_x87::{x, F80};

/// The geometry `SI_makeVertices` reads from a node, by the node's kind (+0x30).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum NodeDef {
    /// A kind that is not rendered, or a node without geometry.
    #[default]
    None,
    /// Kind 4. `mesh` is the node's current geometry (+0x48). `positions` is the mesh
    /// whose vertex positions go to the render vertices' +0x20 (which `SI_textureVerts`
    /// projects textures from): the record at +0x3c when there is one, else the node's
    /// first definition (+0x38) when it has one definition and its second otherwise;
    /// `None` stands for "the same record as `mesh`".
    Mesh { mesh: Mesh, positions: Option<Mesh> },
    /// Kind 2 with its face records (ninth pass): `face` is the node's current face
    /// (+0x48), `shape` the face that lends positions when it is another record (the
    /// first definition of the array at +0x38 for a node with one definition, the second
    /// otherwise). Converted by [`Tessellators::face_mesh`].
    FaceOf { face: Box<si_hrc::Face>, shape: Option<Box<si_hrc::Face>> },
    /// Kind 1 with its patch records (ninth pass): `patch` is the node's current patch
    /// (+0x48), `shape` the patch that lends positions when it is another record.
    /// Converted by [`crate::softvertex::patch2triangle`].
    PatchOf { patch: Box<si_spline::patch::Surface>, shape: Option<Box<si_spline::patch::Surface>> },
    /// Kind 10 converted on the memory image (thirteenth pass): what
    /// `SI_nurbs2Triangle` (0x084e16f8; `si_nurbs::convert`, on the image) made of the
    /// node's surface: the render vertices as it wrote them, and each triangle's three
    /// corners as indices into them. The triangle records' other words (the link, the
    /// element number, the first material) are written by [`make_vertices`] as the
    /// original's loop writes them.
    NurbsOf { verts: Vec<RVertex>, corners: Vec<[Option<usize>; 3]> },
    /// Kind 2 with a face of NURBS curves converted on the memory image: what
    /// `SI_faceConvertToMesh(face, 0)` (0x08435960; `si_model::mesh_image::face`, on the
    /// image) made of the node's current face (`None`: no mesh) and, when the face that
    /// lends positions is another record, of that one (`shape`; `Some(None)`: no mesh). Made
    /// into triangles by [`face2triangle`] as the original's `SI_face2triangle` does.
    FaceMeshOf { mesh: Option<Box<Mesh>>, shape: Option<Option<Box<Mesh>>> },
    /// Kinds 1, 2, 7 and 10: made by [`Tessellators`].
    Patch,
    Face,
    Meta,
    Nurbs,
}

/// The members of `__SI_Model` that `SI_collect`, `SI_makeVertices` and `SI_textureVerts`
/// read and [`RenModel`] does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeSource {
    /// +0x0c: non-zero: the node is rendered as its bounding box.
    pub box_only: i16,
    /// +0x10, +0x1c: the bounding box.
    pub bbox_min: Vector,
    pub bbox_max: Vector,
    /// +0x34: the number of definitions (1: `SI_makeVertices` takes the first, else the
    /// second, as the shape that lends positions).
    pub nb_defs: i16,
    /// +0x38, +0x3c, +0x48.
    pub def: NodeDef,
    /// +0x206: the number of materials (the array is [`RenModel::materials`]).
    pub n_materials: i16,
    /// +0x20c is a null pointer.
    pub no_material_array: bool,
    /// +0x21c: the node's own list of 2D texture maps.
    pub maps: Option<usize>,
    /// +0x2b8: 0: hidden, no geometry is made.
    pub visible: i16,
    /// +0x2ec: 13: `SI_collect` leaves the node as it is.
    pub algo_type: i32,
    /// +0x354: the node's element number.
    pub elem_id: i32,
    /// +0x3f8: 1: the node's arrays are freed and nothing is made.
    pub garbage_matte: i16,
    /// +0x424, +0x428: the texture coordinate sets.
    pub uv: UvDomains,
    /// What `SI_collect` left: +0x228 … +0x234, +0x330, +0x334, +0x42c.
    pub render: NodeRender,
}

/// The converters of other source files that `SI_makeVertices` calls, and the texture
/// routines `SI_textureVerts` calls. Each method names the original it stands for.
pub trait Tessellators {
    /// `si_patch2triangle(shape, patch, 0, materials, elem_id, …, flag)` (0x08269988)
    /// (`softvertex.c`, not ported: it needs `SI_patchComputesSubdivision` and
    /// `SI_patchSurface`).
    fn patch2triangle(&mut self, _node: usize, _materials: Option<&[MatRef]>, _elem_id: i32, _flag: i16) -> NodeRender {
        NodeRender::default()
    }
    /// `SI_nurbs2Triangle(shape, surface, materials, elem_id, …, flag)` (0x084e16f8)
    /// (`nrb_convert.c`).
    fn nurbs2triangle(&mut self, _node: usize, _materials: Option<&[MatRef]>, _elem_id: i32, _flag: i16) -> NodeRender {
        NodeRender::default()
    }
    /// `SI_faceConvertToMesh(face, 0)` (0x08435960; `face.c`; ported in `si-model`)
    /// for the node's current face (`shape` false) or the face that lends positions
    /// (`shape` true, asked only when it is another record), and whether the two are one
    /// record.
    fn face_is_its_shape(&mut self, _node: usize) -> bool {
        true
    }
    fn face_to_mesh(&mut self, _node: usize, _shape: bool) -> Option<Mesh> {
        None
    }
    /// `SI_faceConvertToMesh(face, 0)` for a face the node carries
    /// ([`NodeDef::FaceOf`]). The default is `si-model`'s port bound to `si-spline`'s
    /// routines ([`crate::facemesh::face_to_mesh`]).
    fn face_mesh(&mut self, face: &si_hrc::Face) -> Option<Mesh> {
        crate::facemesh::face_to_mesh(face)
    }
    /// `SI_metaSysGetSurface(system, 1, 1)` (the metaball code): the surface it leaves.
    fn meta_surface(&mut self, _node: usize) -> MetaSurface {
        MetaSurface::default()
    }
    /// `SI_txtProjectionInfo(model, map, &bound, &size, &centre)`: the box a texture is
    /// projected from (x min, x max, y min, y max, z min, z max), its size and its centre.
    /// The three results are locals of `SI_textureVerts` that keep their values from one
    /// call to the next. It also stores the map's matrix at +0xb8, hence the map itself.
    /// `verts` are the node's render vertices (+0x22c) and `elem_id` its number (+0x354).
    /// The default is the port, [`crate::polygon::txt_projection_info`] with
    /// [`model_compute_rot_bounding`] on the vertices (the full product's
    /// `SI_viewerUpdOpt` has bit 0x20 clear while rendering: the radii are made).
    #[allow(clippy::too_many_arguments)]
    fn txt_projection_info(&mut self, _node: usize, _map: usize, record: &mut ShadeMap, verts: &[RVertex], elem_id: i32, bound: &mut [f32; 6], size: &mut Vector, centre: &mut Vector) {
        let mut bounding = |rot: &Vector| model_compute_rot_bounding(&mut verts.iter().map(|v| v.world), rot, true);
        crate::polygon::txt_projection_info(record, elem_id, &mut bounding, 0.0, bound, size, centre);
    }
    /// `SI_mapTcoordCompute(map, position, &pair, &bound, &size, &centre)`: the texture
    /// coordinates of a point; returns the seam bits: 1 for a u in the first quarter, 2 for
    /// the last. The default is the port, [`crate::t2d_fct::map_tcoord_compute`].
    #[allow(clippy::too_many_arguments)]
    fn map_tcoord_compute(&mut self, _map: usize, record: &ShadeMap, p: Vector, pair: &mut [f32; 2], bound: &[f32; 6], size: &Vector, centre: &Vector) -> u16 {
        crate::t2d_fct::map_tcoord_compute(record, p, pair, bound, size, centre)
    }
}

/// 0x08654678, 0x0865467c (float): the box of `SI_modelComputeRotBounding` starts from
/// these.
const ROT_FAR: f32 = 1e10;

/// The box of a node's points in a rotated frame, and the two radii
/// [soft 0x0827a48c SI_modelComputeRotBounding]: `SI_modelComputeRotBounding(model,
/// rotation, &least, &greatest, &radius5, &radius6)`.
///
/// The node's render vertices are made first when it has none (`SI_makeVertices(model,
/// −1)`). With R = `SI_matrixXyzRotation(rotation)` every point p (a render vertex's +0x20;
/// or, for a mesh node without render vertices, the vertices of the mesh that lends
/// positions) is taken to p × R (each coordinate a sum of three products and the
/// translation in the register, rounded to float); the box of those starts from ±1e10; x's
/// least value is compared before the rounding, everything else after. The radii: the
/// greatest x² + z² (cylinder) and the greatest x² + z² + y² (sphere) over the points, kept
/// in the register, their square roots rounded to float; they are **not written** when bit
/// 0x20 of `SI_viewerUpdOpt` (0x08679678) is set (`with_radii` false).
///
/// `points` is the positions in the order the original walks them; the caller makes the
/// vertices. Class: libm (the rotation).
pub fn model_compute_rot_bounding(points: &mut dyn Iterator<Item = Vector>, rot: &Vector, with_radii: bool) -> crate::polygon::RotBounding {
    let r = si_math::algebra::matrix_xyz_rotation(rot);
    let (mut lo, mut hi) = ([ROT_FAR; 3], [-ROT_FAR; 3]);
    let (mut cylinder, mut sphere) = (x(0.0f32), x(0.0f32));
    for p in points {
        let wide = ((x(p.z) * r[2][0]) + ((x(p.y) * r[1][0]) + (x(p.x) * r[0][0]))) + r[3][0];
        let px = wide.to_f32();
        let pz = (((x(p.z) * r[2][2]) + ((x(p.y) * r[1][2]) + (x(p.x) * r[0][2]))) + r[3][2]).to_f32();
        let py = ((((x(p.y) * r[1][1]) + (x(p.x) * r[0][1])) + (x(p.z) * r[2][1])) + r[3][1]).to_f32();
        if wide.lt(x(lo[0])) {
            lo[0] = px;
        }
        if x(px).gt(x(hi[0])) {
            hi[0] = px;
        }
        if x(py).lt(x(lo[1])) {
            lo[1] = py;
        }
        if x(py).gt(x(hi[1])) {
            hi[1] = py;
        }
        if x(pz).lt(x(lo[2])) {
            lo[2] = pz;
        }
        if x(pz).gt(x(hi[2])) {
            hi[2] = pz;
        }
        let flat = (x(px) * px) + (x(pz) * pz);
        if flat.gt(cylinder) {
            cylinder = flat;
        }
        let round = flat + (x(py) * py);
        if round.gt(sphere) {
            sphere = round;
        }
    }
    crate::polygon::RotBounding {
        least: Vector::new(lo[0], lo[1], lo[2]),
        greatest: Vector::new(hi[0], hi[1], hi[2]),
        radii: with_radii.then(|| (cylinder.sqrt().to_f32(), sphere.sqrt().to_f32())),
    }
}

/// The middle of a box [soft 0x0827a890 SI_modelCtrBbox]: `SI_modelCtrBbox(model, least,
/// greatest)` returns ((greatest + least) × 0.5) per component, the sum in the register,
/// the double 0.5 (0x08654680), rounded once. The model is not read. Class: model.
pub fn model_ctr_bbox(least: Vector, greatest: Vector) -> Vector {
    let half = x(0.5f64);
    Vector::new(((x(greatest.x) + least.x) * half).to_f32(), (half * (x(greatest.y) + least.y)).to_f32(), ((x(greatest.z) + least.z) * half).to_f32())
}

/// The half extents of a projection's box by mapping method
/// [soft 0x0827a8e4 SI_modelGeoBbox]: `SI_modelGeoBbox(model, method, least, greatest,
/// radius5, radius6)`. Half the box's extent per component ((greatest − least) × 0.5, the
/// double at 0x08654688, rounded once); for method 5 (cylindrical) x and z are `radius5`
/// instead; for method 6 (spherical) all three are `radius6`. The model is not read.
/// Class: model.
pub fn model_geo_bbox(method: i16, least: Vector, greatest: Vector, radius5: f32, radius6: f32) -> Vector {
    let half = x(0.5f64);
    let h = Vector::new((half * (x(greatest.x) - least.x)).to_f32(), ((x(greatest.y) - least.y) * half).to_f32(), (half * (x(greatest.z) - least.z)).to_f32());
    match method {
        5 => Vector::new(radius5, h.y, radius5),
        6 => Vector::new(radius6, radius6, radius6),
        _ => h,
    }
}

/// How many texture coordinate pairs a node's vertices need
/// [soft 0x08279b58 si_tcoordNeeded]: `si_tcoordNeeded(model)`. Every vertex's +0x40 is
/// cleared; for every triangle whose material's map count (+0x6e8) plus the node's own
/// (+0x216) is not 0, each corner not yet marked is marked and adds that sum; the marks are
/// cleared again. (A triangle with a null material or corner is read through the null
/// pointer; a [`Wild`] here.) Class: integer.
pub fn tcoord_needed(r: &mut NodeRender, own_maps: i16, material_maps: &dyn Fn(u32) -> Option<i16>) -> Result<i32, Wild> {
    for v in &mut r.verts {
        v.uv = NO_UV;
    }
    let mut total = 0i32;
    for t in 0..r.tn_count.max(0) as usize {
        let pairs = material_maps(r.tns[t].material).ok_or(Wild::NullMaterials)? as i32 + own_maps as i32;
        if pairs != 0 {
            for k in 0..3 {
                let v = &mut r.verts[r.tns[t].v[k].ok_or(Wild::Index("corner"))?];
                if v.uv == NO_UV {
                    v.uv = 1;
                    total = total.wrapping_add(pairs);
                }
            }
        }
    }
    for v in &mut r.verts {
        v.uv = NO_UV;
    }
    Ok(total)
}

/// Room for `more` render vertices [soft 0x0827a2f4 allocExtraRenVertices]:
/// `allocExtraRenVertices(model, more)`. The array (+0x22c) is made, or moved to a block,
/// of c + 64 − ((c − 1) & 64) records with c = count + `more` (that is c + 64, or c when
/// bit 6 of c − 1 is set: not a rounding to a multiple of 64); when the block moves, the
/// corners of all the node's triangles are moved with it. Returns whether the allocation
/// succeeded. Here the array is a `Vec` and the corners are indices: the routine answers
/// the size the original asks for. Class: integer.
pub fn alloc_extra_ren_vertices(r: &mut NodeRender, more: i32) -> u32 {
    let c = (r.verts.len() as u32).wrapping_add(more as u32);
    let records = c.wrapping_sub((c.wrapping_sub(1) & 0x40).wrapping_sub(0x40));
    r.verts.reserve((records as usize).saturating_sub(r.verts.len()).min(1 << 20));
    records
}

/// Room for `more` texture coordinate pairs behind the first `used`
/// [soft 0x0827a39c allocExtraTCoord]: `allocExtraTCoord(model, used, more)`. The array
/// (+0x330) is made, or moved to a block, of c + 64 − ((c − 1) & 64) pairs with c = `used`
/// + `more`; when the block moves, the pointer at +0x40 of every corner of every triangle
/// that points into the first `used` pairs of the old block is moved with it. Here the
/// array is a `Vec` grown to hold `used + more` pairs and the pointers are indices; the
/// routine answers the size the original asks for. Class: integer.
pub fn alloc_extra_tcoord(r: &mut NodeRender, used: i32, more: i32) -> u32 {
    let c = (used as u32).wrapping_add(more as u32);
    let pairs = c.wrapping_sub((c.wrapping_sub(1) & 0x40).wrapping_sub(0x40));
    if (c as usize) > r.tcoords.len() && c < (1 << 28) {
        r.tcoords.resize(c as usize, [0.0; 2]);
    }
    pairs
}

/// The doubles 0.25 and 0.75 (0x08654660, 0x08654668): a u at or below the first, or at or
/// above the second, lies beside the seam of a cylindrical or spherical projection. The
/// double 0.5 (0x08654670): a u at or below it is moved up by 1 on a triangle across the
/// seam.
const SEAM_LOW: f64 = 0.25;
const SEAM_HIGH: f64 = 0.75;
const SEAM_HALF: f64 = 0.5;

/// What `SI_textureVerts` reads of the node besides its arrays.
pub struct TextureNode<'a> {
    /// The node's number, handed to the texture routines.
    pub node: usize,
    /// +0x30
    pub kind: i32,
    /// +0x216: the node's own map count; +0x21c: its own map list.
    pub own_maps: i16,
    pub maps: Option<usize>,
    /// +0x424, +0x428
    pub uv: &'a UvDomains,
    /// +0x354: the node's number, which a map's kept projection box is tagged with.
    pub elem_id: i32,
}

/// The texture coordinate pairs of a node's vertices [soft 0x08279c1c SI_textureVerts]:
/// `SI_textureVerts(model)`. Returns the number of pairs made, which the original adds to
/// `SI_stats` +0x18 and stores at the node's +0x334.
///
/// With [`tcoord_needed`] 0 nothing is made (+0x334 = 0; the counter is left alone).
/// Otherwise, for every triangle whose pair count n (the 16-bit sum of the node's map
/// count and its material's) is not 0:
///
/// 1. Each corner without pairs yet gets the next n pairs of the node's array
///    ([`alloc_extra_tcoord`]) and is "fresh".
/// 2. For every map of the material's list, then of the node's own list, pair by pair:
///    `SI_txtProjectionInfo` gives the projection box; its centre is moved by the map's
///    translation (+0xa0) times the vector at +0x7c, each product through the rows of the
///    map's matrix at +0xb8 (formed in the register, each coordinate of the centre rounded
///    once). Then per corner: a fresh corner gets, for method 4 (the mesh's own
///    coordinates), (1 − u, 1 − v) of the set the map uses (`SI_meshGetTxtUVDomain` for a
///    mesh node, set 0 otherwise), and for any other method `SI_mapTcoordCompute` of its
///    position (+0x20), whose result is OR-ed into the map's seam bits; a corner that had
///    its pairs already contributes, for methods 5 and 6 only, bit 1 when its u is ≤ 0.25
///    and bit 2 when it is ≥ 0.75. A map whose seam bits end as 3 has the triangle across
///    the seam.
/// 3. A triangle across the seam of any map gets three vertices of its own
///    ([`alloc_extra_ren_vertices`]): copies of its corners with pairs of their own, in
///    which every such map's u that is ≤ 0.5 is raised by 1.
///
/// (So a vertex shared by several triangles keeps the pairs of the first triangle that
/// met it: the second triangle's materials do not compute again.) Class: model.
pub fn texture_verts(st: &mut ShadeState, r: &mut NodeRender, node: &TextureNode, env: &mut dyn Tessellators) -> Result<i32, Wild> {
    let needed = {
        let mats = &st.materials;
        tcoord_needed(r, node.own_maps, &|m| mats.get(m as usize).map(|m| m.n_maps))?
    };
    if needed == 0 {
        return Ok(0);
    }
    let mut total = 0i32;
    let mut bound = [0.0f32; 6];
    let (mut size, mut centre) = (Vector::default(), Vector::default());
    let mut seam_of = vec![0i16; 1024];
    for t in 0..r.tn_count.max(0) as usize {
        let material = st.materials.get(r.tns[t].material as usize).ok_or(Wild::NullMaterials)?;
        let (material_maps, material_list) = (material.n_maps, material.maps);
        let pairs = node.own_maps.wrapping_add(material_maps);
        if pairs == 0 {
            continue;
        }
        let corners = [0, 1, 2].map(|k| r.tns[t].v[k]);
        let [Some(c0), Some(c1), Some(c2)] = corners else { return Err(Wild::Index("corner")) };
        let corners = [c0, c1, c2];
        let mut across = false;
        let mut fresh = [false; 3];
        for k in 0..3 {
            let v = corners[k];
            fresh[k] = r.verts[v].uv == NO_UV;
            if fresh[k] {
                alloc_extra_tcoord(r, total, pairs as i32);
                r.verts[v].uv = total as u32;
                total = total.wrapping_add(pairs as i32);
            }
        }
        let mut slot: i16 = 0;
        for list in [material_list, node.maps] {
            let mut cur = list;
            while let Some(mi) = cur {
                let mut seam: u16 = 0;
                let this = slot as usize;
                // the original's table is 1024 shorts on the stack: a 1025th map writes past it
                *seam_of.get_mut(this).ok_or(Wild::Index("map"))? = 0;
                env.txt_projection_info(node.node, mi, &mut st.maps[mi], &r.verts, node.elem_id, &mut bound, &mut size, &mut centre);
                let map: &ShadeMap = &st.maps[mi];
                let t1 = x(map.v7c[0]) * map.translation[0];
                let t2 = x(map.v7c[1]) * map.translation[1];
                let t3 = x(map.v7c[2]) * map.translation[2];
                let m = &map.tmat;
                let t4 = (x(m[0][0]) * t1 + x(m[0][1]) * t2) + x(m[0][2]) * t3;
                let t5 = (x(m[1][0]) * t1 + x(m[1][1]) * t2) + x(m[1][2]) * t3;
                let t6 = t3 * m[2][2] + (t2 * m[2][1] + t1 * m[2][0]);
                centre.x = (t4 + centre.x).to_f32();
                slot = slot.wrapping_add(1);
                centre.y = (t5 + centre.y).to_f32();
                centre.z = (t6 + centre.z).to_f32();
                for k in 0..3 {
                    let v = corners[k];
                    let at = r.verts[v].uv as usize + this;
                    if at >= r.tcoords.len() {
                        // more maps in the two lists than the pair count says: a write past
                        // the vertex's pairs in the original
                        return Err(Wild::Index("pair"));
                    }
                    if !fresh[k] {
                        if map.method == 5 || map.method == 6 {
                            let u = x(r.tcoords[at][0]);
                            if u.le(x(SEAM_LOW)) {
                                seam |= 1;
                            }
                            if u.ge(x(SEAM_HIGH)) {
                                seam |= 2;
                            }
                        }
                    } else if map.method == 4 {
                        let set = if node.kind == 4 { mesh_get_txt_uv_domain(Some(node.uv), map.id).unwrap_or(0) } else { 0 };
                        let (u, w) = match r.verts[v].p48 {
                            // a set after the first read through a null pointer at +0x48
                            NO_UV if set > 0 => return Err(Wild::Index("further pairs")),
                            NO_UV => rvtx_get_uv(&r.verts[v], 0, &[]),
                            first => rvtx_get_uv(&r.verts[v], set, r.uv2.as_deref().and_then(|p| p.get(first as usize..)).ok_or(Wild::Index("further pairs"))?),
                        };
                        r.tcoords[at] = [(F80::ONE - u).to_f32(), (F80::ONE - w).to_f32()];
                    } else {
                        let p = r.verts[v].world;
                        let mut pair = r.tcoords[at];
                        seam |= env.map_tcoord_compute(mi, map, p, &mut pair, &bound, &size, &centre);
                        r.tcoords[at] = pair;
                    }
                }
                if seam == 3 {
                    across = true;
                    seam_of[this] = 1;
                }
                cur = map.next;
            }
        }
        if across {
            alloc_extra_ren_vertices(r, 3);
            let n = pairs as i32;
            for k in 0..3 {
                let new = r.verts.len();
                alloc_extra_tcoord(r, total, n);
                let old = r.tns[t].v[k].ok_or(Wild::Index("corner"))?;
                r.tns[t].v[k] = Some(new);
                let mut copy: RVertex = r.verts[old];
                let from = copy.uv as usize;
                copy.uv = total as u32;
                r.verts.push(copy);
                let to = total as usize;
                total = total.wrapping_add(n);
                for s in 0..n.max(0) as usize {
                    r.tcoords[to + s] = r.tcoords[from + s];
                    if seam_of[s] != 0 && x(SEAM_HALF).ge(x(r.tcoords[to + s][0])) {
                        r.tcoords[to + s][0] = (x(r.tcoords[to + s][0]) + F80::ONE).to_f32();
                    }
                }
            }
        }
    }
    r.tcoords.truncate(total.max(0) as usize);
    Ok(total)
}

/// The converter for a node's kind [soft 0x0827a964 SI_makeVertices]:
/// `SI_makeVertices(model, flag)`. A node shown as its box (+0x0c not 0) gets
/// `SI_bounding2triangle`. Otherwise by kind (+0x30): 1 `si_patch2triangle`, 2
/// `SI_face2triangle`, 4 `SI_meshTriangles` (with the node's material count, materials,
/// element number and own map list, the further texture coordinates asked for, no table),
/// 7 `SI_metaTriangles`, 10 `SI_nurbs2Triangle`; any other kind is left alone. For kinds
/// 1, 2, 4 and 10 the shape that lends positions is the node's first definition when it
/// has exactly one (+0x34) and its second otherwise; a mesh node takes the record at +0x3c
/// first when that is not null.
///
/// What a converter does not write stays as it was: `SI_meshTriangles` on a mesh without a
/// triangle to make leaves the vertex count and array alone.
pub fn make_vertices(m: &RenModel, src: &mut NodeSource, node: usize, materials: Option<&[MatRef]>, flag: i16, env: &mut dyn Tessellators) -> Result<(), Wild> {
    let first = materials.and_then(|m| m.first()).map(|r| r.id);
    if src.box_only != 0 {
        if let Some(r) = bounding2triangle(m.kind, src.bbox_min, src.bbox_max, src.elem_id, first) {
            src.render = r;
        }
        return Ok(());
    }
    match m.kind {
        1 => {
            if let NodeDef::PatchOf { patch, shape } = &mut src.def {
                src.render = crate::softvertex::patch2triangle(shape.as_deref_mut(), patch, materials, src.elem_id, flag)?;
            } else {
                src.render = env.patch2triangle(node, materials, src.elem_id, flag);
            }
        }
        2 => {
            if let NodeDef::FaceMeshOf { mesh, shape } = &src.def {
                let (mesh, shape) = (mesh.clone(), shape.clone());
                src.render = face2triangle(shape.is_none(), &mut |other| if other { shape.clone().flatten().map(|b| *b) } else { mesh.clone().map(|b| *b) }, materials, src.elem_id)?;
            } else if let NodeDef::FaceOf { face, shape } = &src.def {
                let (face, shape) = (face.clone(), shape.clone());
                src.render = face2triangle(shape.is_none(), &mut |other| env.face_mesh(if other { shape.as_deref().unwrap_or(&face) } else { &face }), materials, src.elem_id)?;
            } else {
                let same = env.face_is_its_shape(node);
                src.render = face2triangle(same, &mut |shape| env.face_to_mesh(node, shape), materials, src.elem_id)?;
            }
        }
        4 => {
            let NodeDef::Mesh { mesh, positions } = &mut src.def else { return Err(Wild::Index("mesh")) };
            let own;
            let positions: &Mesh = match positions {
                Some(p) => p,
                None => {
                    own = mesh.clone();
                    &own
                }
            };
            let args = MeshTrianglesArgs {
                model: Some(&src.uv),
                positions,
                n_materials: src.n_materials as i32,
                materials,
                elem_id: src.elem_id,
                want_uv2: true,
                model_maps: src.maps,
                all_hard: 0,
                want_poly_tris: false,
                want_node_verts: false,
                strict: false,
            };
            match mesh_triangles(&args, mesh)? {
                Some(made) => src.render = made.node,
                None => {
                    src.render.tns.clear();
                    src.render.tn_count = 0;
                }
            }
        }
        7 => {
            let surface = env.meta_surface(node);
            if let Some(r) = meta_triangles(&surface, first, src.elem_id)? {
                src.render = r;
            }
        }
        10 => {
            if let NodeDef::NurbsOf { verts, corners } = &src.def {
                // `TriangulateNonTrimmedNurbsSrf`'s loop over the triangle records: the
                // next record, the element number, the first material (`*materials`, a
                // null array giving 0)
                let material = materials.map_or(Ok(NO_MATERIAL), |m| m.first().map(|r| r.id).ok_or(Wild::Index("material")))?;
                let n = corners.len();
                let tns = corners.iter().enumerate().map(|(t, c)| Tn { v: *c, next: (t + 1 < n).then_some(t + 1), w58: src.elem_id as i16, material, ..Tn::default() }).collect();
                src.render = NodeRender { verts: verts.clone(), tns, tn_count: n as i32, ..NodeRender::default() };
            } else {
                src.render = env.nurbs2triangle(node, materials, src.elem_id, flag);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Which record lends a node's render vertices their positions at +0x20, as
/// `SI_makeVertices` picks it (part of [soft 0x0827a964 SI_makeVertices]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The record at +0x3c (a mesh node that has one).
    Org,
    /// The node's first definition (+0x38), when it has exactly one.
    First,
    /// Its second definition, in every other case (no definition at all included: the
    /// original reads the second pointer whatever the count).
    Second,
}

/// See [`Shape`]: `has_org` is "+0x3c is not null" (looked at for a mesh node only),
/// `nb_defs` the word at +0x34.
pub fn shape_of(kind: i32, has_org: bool, nb_defs: i16) -> Shape {
    if kind == 4 && has_org {
        Shape::Org
    } else if nb_defs == 1 {
        Shape::First
    } else {
        Shape::Second
    }
}

/// What `SI_collect` adds to the counters of `SI_stats`: +0x14 render vertices, +0x1c
/// triangles, +0x18 texture coordinate pairs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollectStats {
    pub vertices: i32,
    pub triangles: i32,
    pub pairs: i32,
}

/// A node's render vertices and triangles [soft 0x082799c0 SI_collect]:
/// `SI_collect(model)`, in this order:
///
/// 1. Nothing at all for a node whose word at +0x2ec is 13.
/// 2. The node's vertex array, its further-pairs array and its triangle array are freed
///    and both counts cleared. A node whose word at +0x3f8 is 1 ends here.
/// 3. Unless the node is hidden (+0x2b8 = 0): `SI_makeVertices(model, −1)`; then, for a
///    reversed node (bit 3 of +0x08), corners 0 and 2 of every triangle change places.
/// 4. If there is a triangle: the vertex and triangle counts are added to `SI_stats`
///    +0x14 and +0x1c, the node's pair array (+0x330) is freed and `SI_textureVerts` makes
///    it anew.
/// 5. In a left-handed set-up (`SI_rightHand` = 0): every vertex's raw normal is negated
///    (the vertices `SI_textureVerts` added among them) and corners 0 and 2 change places
///    (again, for a reversed node).
///
/// `right_hand` is `SI_rightHand` (0x086793ec) ≠ 0. Class: copy (the negation goes
/// through the register).
pub fn collect(st: &mut ShadeState, m: &RenModel, src: &mut NodeSource, node: usize, right_hand: bool, env: &mut dyn Tessellators) -> Result<CollectStats, Wild> {
    let mut stats = CollectStats::default();
    if src.algo_type == 13 {
        return Ok(stats);
    }
    src.render.verts.clear();
    src.render.uv2 = None;
    src.render.tns.clear();
    src.render.tn_count = 0;
    if src.garbage_matte == 1 {
        return Ok(stats);
    }
    if src.visible != 0 {
        let mats: Option<Vec<MatRef>> = (!src.no_material_array).then(|| m.materials.iter().map(|&i| MatRef { id: i as u32, maps: st.materials[i].maps }).collect());
        make_vertices(m, src, node, mats.as_deref(), -1, env)?;
        if m.reversed {
            swap_corners(&mut src.render);
        }
    }
    if src.render.tn_count != 0 {
        stats.vertices = src.render.verts.len() as i32;
        stats.triangles = src.render.tn_count;
        src.render.tcoords.clear();
        let tn = TextureNode { node, kind: m.kind, own_maps: m.n_maps, maps: src.maps, uv: &src.uv, elem_id: src.elem_id };
        let mut render = std::mem::take(&mut src.render);
        let pairs = texture_verts(st, &mut render, &tn, env);
        src.render = render;
        stats.pairs = pairs?;
    }
    if !right_hand {
        for v in &mut src.render.verts {
            v.raw_normal = Vector::new((-x(v.raw_normal.x)).to_f32(), (-x(v.raw_normal.y)).to_f32(), (-x(v.raw_normal.z)).to_f32());
        }
        swap_corners(&mut src.render);
    }
    Ok(stats)
}

/// Corners 0 and 2 of every triangle of a node change places (twice in `SI_collect`).
fn swap_corners(r: &mut NodeRender) {
    for t in r.tns.iter_mut().take(r.tn_count.max(0) as usize) {
        t.v.swap(0, 2);
    }
}

/// The source geometry of a scene's nodes, one entry per node of [`Geometry::models`], and
/// the glue between `SI_collect` and the pools the rest of the renderer works on.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub nodes: Vec<NodeSource>,
    /// What the converters refused, by node (the original leaves its arrays there).
    pub refused: Vec<(usize, Wild)>,
    /// What the calls of [`Scene::collect`] added to the three counters of `SI_stats`
    /// (+0x14 vertices, +0x18 pairs, +0x1c triangles): arrays that are not chunk memory.
    pub counted: [i32; 3],
}

impl Scene {
    /// [`collect`] for node `node`, its arrays appended to the pools and the node's counts
    /// and array starts set; the counters of `SI_stats` are brought up to date. (Not a
    /// routine of the original: there the arrays hang on the node. The pools keep the
    /// arrays of earlier calls, as `freeModel` here leaves them.)
    pub fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize, env: &mut dyn Tessellators) {
        let src = &mut self.nodes[node];
        let stats = match collect(st, &g.models[node], src, node, g.right_hand, env) {
            Ok(s) => s,
            Err(w) => {
                self.refused.push((node, w));
                src.render = NodeRender::default();
                CollectStats::default()
            }
        };
        g.clipper.made_vertices = g.clipper.made_vertices.wrapping_add(stats.vertices);
        g.made_tns = g.made_tns.wrapping_add(stats.triangles);
        g.clipper.made_pairs = g.clipper.made_pairs.wrapping_add(stats.pairs);
        self.counted = [self.counted[0].wrapping_add(stats.vertices), self.counted[1].wrapping_add(stats.pairs), self.counted[2].wrapping_add(stats.triangles)];
        if src.algo_type == 13 {
            return;
        }
        let r = &src.render;
        let (v0, t0, u0) = (st.verts.len(), st.tns.len(), st.uvs.len());
        st.uvs.extend(r.tcoords.iter().copied());
        st.verts.extend(r.verts.iter().map(|v| RVertex { uv: if v.uv == NO_UV { NO_UV } else { v.uv + u0 as u32 }, ..*v }));
        st.tns.extend(r.tns.iter().map(|t| {
            let mut t = t.clone();
            t.v = t.v.map(|c| c.map(|i| i + v0));
            t.next = t.next.map(|i| i + t0);
            t
        }));
        let m = &mut g.models[node];
        m.vert_count = r.verts.len() as i32;
        m.verts = (!r.verts.is_empty()).then_some(v0);
        m.tn_count = r.tn_count;
        m.tns = (!r.tns.is_empty()).then_some(t0);
        m.tcoords = (!r.tcoords.is_empty()).then_some(u0);
        m.uv2 = r.uv2.as_ref().map(|_| 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shade::ShadeMaterial;
    use si_model::mesh_b::{update_nodes, MeshEnv};

    /// u from x, with the seam bits of a cylinder: what `SI_mapTcoordCompute` answers.
    struct Cylinder;
    impl Tessellators for Cylinder {
        fn map_tcoord_compute(&mut self, _: usize, _: &ShadeMap, p: Vector, pair: &mut [f32; 2], _: &[f32; 6], _: &Vector, _: &Vector) -> u16 {
            *pair = [p.x, p.y];
            (p.x <= 0.25) as u16 | ((p.x >= 0.75) as u16) << 1
        }
    }

    fn node(positions: &[[f32; 3]]) -> (ShadeState, RenModel, NodeSource) {
        let mut mesh = Mesh::with_vertices(positions);
        mesh.push_polygon(&[0, 1, 2]);
        update_nodes(&mut mesh, true, &mut MeshEnv::default());
        let mut st = ShadeState::default();
        st.maps.push(ShadeMap { method: 5, ..ShadeMap::default() });
        st.materials.push(ShadeMaterial { n_maps: 1, maps: Some(0), ..ShadeMaterial::default() });
        let ren = RenModel { kind: 4, materials: vec![0], ..RenModel::default() };
        let src = NodeSource { def: NodeDef::Mesh { mesh, positions: None }, nb_defs: 1, n_materials: 1, visible: 1, uv: UvDomains { count: 1, sets: Vec::new() }, ..NodeSource::default() };
        (st, ren, src)
    }

    #[test]
    fn a_triangle_across_the_seam_gets_vertices_of_its_own() {
        let (mut st, ren, mut src) = node(&[[0.1, 0.0, 0.0], [0.9, 0.0, 0.0], [0.5, 1.0, 0.0]]);
        let stats = collect(&mut st, &ren, &mut src, 0, true, &mut Cylinder).unwrap();
        // three vertices from the mesh, three copies; six pairs
        assert_eq!(stats, CollectStats { vertices: 3, triangles: 1, pairs: 6 });
        let r = &src.render;
        assert_eq!(r.verts.len(), 6);
        assert_eq!(r.tns[0].v, [Some(3), Some(4), Some(5)]);
        // in the copies u at or below 0.5 is raised by 1; the first three keep theirs
        let u: Vec<f32> = r.verts.iter().map(|v| r.tcoords[v.uv as usize][0]).collect();
        assert_eq!(u, [0.1, 0.9, 0.5, 1.1, 0.9, 1.5]);
    }

    #[test]
    fn away_from_the_seam_nothing_is_added() {
        let (mut st, ren, mut src) = node(&[[0.3, 0.0, 0.0], [0.9, 0.0, 0.0], [0.5, 1.0, 0.0]]);
        let stats = collect(&mut st, &ren, &mut src, 0, true, &mut Cylinder).unwrap();
        assert_eq!(stats, CollectStats { vertices: 3, triangles: 1, pairs: 3 });
        assert_eq!(src.render.tns[0].v, [Some(0), Some(1), Some(2)]);
    }

    #[test]
    fn the_switches_of_a_node() {
        let (mut st, ren, mut src) = node(&[[0.3, 0.0, 0.0], [0.9, 0.0, 0.0], [0.5, 1.0, 0.0]]);
        // left-handed: normals negated, corners exchanged
        collect(&mut st, &ren, &mut src, 0, false, &mut Cylinder).unwrap();
        assert_eq!(src.render.tns[0].v, [Some(2), Some(1), Some(0)]);
        assert_eq!(src.render.verts[0].raw_normal.z, -1.0);
        // a reversed node in a left-handed set-up: exchanged twice
        let reversed = RenModel { reversed: true, ..ren.clone() };
        collect(&mut st, &reversed, &mut src, 0, false, &mut Cylinder).unwrap();
        assert_eq!(src.render.tns[0].v, [Some(0), Some(1), Some(2)]);
        // hidden: the arrays are freed and nothing is made
        src.visible = 0;
        assert_eq!(collect(&mut st, &ren, &mut src, 0, true, &mut Cylinder), Ok(CollectStats::default()));
        assert!(src.render.verts.is_empty() && src.render.tn_count == 0);
        // the pairs of the last frame that had triangles stay
        assert_eq!(src.render.tcoords.len(), 3);
        // shown as its box
        src.visible = 1;
        src.box_only = 1;
        src.bbox_max = Vector::new(1.0, 1.0, 1.0);
        let stats = collect(&mut st, &ren, &mut src, 0, true, &mut Cylinder).unwrap();
        assert_eq!((stats.vertices, stats.triangles), (24, 12));
    }

    #[test]
    fn which_record_lends_positions() {
        assert_eq!(shape_of(4, true, 1), Shape::Org);
        assert_eq!(shape_of(1, true, 1), Shape::First);
        assert_eq!(shape_of(4, false, 2), Shape::Second);
        assert_eq!(shape_of(4, false, 0), Shape::Second);
    }

    #[test]
    fn growth_is_not_a_rounding_to_64() {
        let mut r = NodeRender::default();
        assert_eq!(alloc_extra_tcoord(&mut r, 0, 1), 65);
        assert_eq!(alloc_extra_tcoord(&mut r, 63, 1), 128);
        // from 65 to 128 pairs the block is exactly as large as asked
        assert_eq!(alloc_extra_tcoord(&mut r, 64, 1), 65);
        assert_eq!(alloc_extra_tcoord(&mut r, 127, 1), 128);
        assert_eq!(alloc_extra_tcoord(&mut r, 128, 1), 193);
        assert_eq!(alloc_extra_ren_vertices(&mut r, 3), 67);
    }

    #[test]
    fn the_box_of_a_projection() {
        let (a, b) = (Vector::new(0.0, 1.0, 2.0), Vector::new(4.0, 2.0, 8.0));
        assert_eq!(model_ctr_bbox(a, b), Vector::new(2.0, 1.5, 5.0));
        assert_eq!(model_geo_bbox(1, a, b, 9.0, 7.0), Vector::new(2.0, 0.5, 3.0));
        assert_eq!(model_geo_bbox(5, a, b, 9.0, 7.0), Vector::new(9.0, 0.5, 9.0));
        assert_eq!(model_geo_bbox(6, a, b, 9.0, 7.0), Vector::new(7.0, 7.0, 7.0));
    }
}
