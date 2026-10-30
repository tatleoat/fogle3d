// SPDX-License-Identifier: MIT
//! Small whole-mesh helpers: the original's `meshUtil.c` (finding 0015).

use si_x87::x;

use crate::hierarchy::{next_def_get, DefSlot};
use crate::mesh::{Mesh, PolyNode};
use crate::mesh_b::MeshEnv;
use crate::node::{Current, Definition, Models, NodeId};

/// Gives every corner its polygon's normal [soft 0x08458770 SI_removeFaceted]: integer
/// copies of the three words into every corner record, separators included. Class: copy.
pub fn remove_faceted(mesh: &mut Mesh) {
    for p in &mut mesh.polygons {
        for c in &mut p.nodes {
            c.normal = p.normal;
        }
    }
}

/// The half width of the band a corner must lie in for [`mesh_planar`]: the doubles
/// −2⁻⁵² and 2⁻⁵² (0x0865fad8, 0x0865fae0).
const PLANAR_EPS: f64 = 2.220446049250313e-16;

/// Whether every polygon is flat [soft 0x08458848 SI_meshPlanar]. Class: model.
///
/// Per polygon, with n its stored normal and p₀ the position of its first corner:
/// d₀ = −((n.x·p₀.x + n.y·p₀.y) + n.z·p₀.z), rounded to double; for every further corner
/// that is not negative (a separator), ((p.x·n.x + p.y·n.y) + p.z·n.z) + d₀ rounded to
/// float must lie within [−2⁻⁵², 2⁻⁵²]. In effect the distance must round to exactly zero:
/// only polygons whose corners agree to the last bit pass.
pub fn mesh_planar(mesh: &Mesh) -> bool {
    for p in &mesh.polygons {
        let n = p.normal;
        // (a first corner of −1 reads the 20 bytes before the vertex array in the original)
        let p0 = p.nodes.first().and_then(|c| mesh.vertices.get(c.vertex as usize)).map_or([0.0; 3], |v| v.pos);
        let d0 = (-((x(n[0]) * p0[0] + x(n[1]) * p0[1]) + x(n[2]) * p0[2])).to_f64();
        for c in p.nodes.iter().skip(1) {
            if c.vertex < 0 {
                continue;
            }
            let q = mesh.vertices.get(c.vertex as usize).map_or([0.0; 3], |v| v.pos);
            let d = (((x(q[0]) * n[0] + x(q[1]) * n[1]) + x(q[2]) * n[2]) + d0).to_f32();
            if x(d).lt(x(-PLANAR_EPS)) || x(d).gt(x(PLANAR_EPS)) {
                return false;
            }
        }
    }
    true
}

/// The average of the corner normals at vertex `v` [soft 0x08458960
/// SI_computeMeshVertexNormal]: over every star entry of the vertex that has a polygon,
/// the normals of that polygon's corners at `v` are summed in floats; the sums are
/// multiplied by 1 / (number of such star entries). Not normalised. A vertex without
/// polygons gives 0 × ∞. Class: model.
pub fn compute_mesh_vertex_normal(mesh: &Mesh, v: i32) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut count = 0u32;
    // (the original does not test `v` against the vertex count)
    let star = mesh.vertices.get(v as usize).map_or(&[][..], |vt| &vt.star[..]);
    for s in star {
        if s.polygon == -1 {
            continue;
        }
        count += 1;
        let Some(poly) = mesh.polygons.get(s.polygon as usize) else { continue };
        for c in poly.nodes.iter().filter(|c| c.vertex == v) {
            sum = [(x(sum[0]) + c.normal[0]).to_f32(), (x(sum[1]) + c.normal[1]).to_f32(), (x(sum[2]) + c.normal[2]).to_f32()];
        }
    }
    let inv = x(1.0f32) / si_x87::F80::from_i64(count as i64);
    [(inv * sum[0]).to_f32(), (inv * sum[1]).to_f32(), (inv * sum[2]).to_f32()]
}

/// Copies a corner's texture coordinates onto another [soft 0x08453d90 SI_nodesCopyUV]
/// (`meshEditFct.c`): the first pair always. The further pairs, when the source has any:
/// without both models, fourteen floats (seven pairs) whatever the arrays hold; with both
/// models and the same number of coordinate sets (`nb_uv`, +0x424), `nb_uv` − 1 pairs;
/// with different numbers, none. A missing destination array is allocated first.
///
/// `nb_uv` is `None` for the call without models, else the two models' numbers. Where the
/// original copies more floats than the source holds, the pairs that exist are copied and
/// the rest of the destination is zero.
pub fn nodes_copy_uv(nb_uv: Option<(i32, i32)>, dst: &mut PolyNode, src: &PolyNode) {
    dst.uv = src.uv;
    if src.extra_uv.is_empty() {
        return;
    }
    let pairs = match nb_uv {
        None => 7,
        Some((a, b)) if a == b => (a - 1).max(0) as usize,
        Some(_) => return,
    };
    if dst.extra_uv.len() < pairs {
        dst.extra_uv.resize(pairs, [0.0; 2]);
    }
    for (d, s) in dst.extra_uv.iter_mut().zip(&src.extra_uv).take(pairs) {
        *d = *s;
    }
}

/// Bits of the `what` argument of [`mesh_copy_attributes`].
pub const COPY_UV: i32 = 1;
pub const COPY_COLOR: i32 = 2;

/// Copies texture coordinates (bit 1) and vertex colours (bit 2) corner for corner from one
/// mesh to another of the same shape [soft 0x08458a54 meshCopyAttributes]. False, with
/// nothing copied, when neither bit is set or the two meshes differ in their polygon count
/// or in any polygon's corner count. `nb_uv` is the model's number of coordinate sets (both
/// arguments of `SI_nodesCopyUV` are the one model), `None` for a null model.
pub fn mesh_copy_attributes(nb_uv: Option<i32>, src: &Mesh, dst: &mut Mesh, what: i32) -> bool {
    if what & (COPY_UV | COPY_COLOR) == 0 || src.polygons.len() != dst.polygons.len() {
        return false;
    }
    if src.polygons.iter().zip(&dst.polygons).any(|(a, b)| a.nodes.len() as u16 != b.nodes.len() as u16) {
        return false;
    }
    for (a, b) in src.polygons.iter().zip(&mut dst.polygons) {
        for (s, d) in a.nodes.iter().zip(&mut b.nodes) {
            if what & COPY_UV != 0 {
                nodes_copy_uv(nb_uv.map(|n| (n, n)), d, s);
            }
            if what & COPY_COLOR != 0 {
                d.color = s.color;
            }
        }
    }
    true
}

/// One of a node's geometries as an object: a stored definition, the working definition
/// when it is a block of its own, or the original definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Object {
    Stored(usize),
    Own,
    Original,
}

fn object(models: &Models, node: NodeId, slot: DefSlot) -> Option<Object> {
    match slot {
        DefSlot::Stored(i) => Some(Object::Stored(i)),
        DefSlot::Original => Some(Object::Original),
        DefSlot::Current => match &models.data[node].current {
            Current::None => None,
            Current::Def(i) => Some(Object::Stored(*i)),
            Current::Own(_) => Some(Object::Own),
        },
    }
}

fn object_mut(models: &mut Models, node: NodeId, o: Object) -> Option<&mut Definition> {
    let d = &mut models.data[node];
    match o {
        Object::Stored(i) => d.defs.get_mut(i),
        Object::Own => match &mut d.current {
            Current::Own(b) => Some(b),
            _ => None,
        },
        Object::Original => d.org_def.as_deref_mut(),
    }
}

/// Brings one definition's texture coordinates and colours to every other definition of a
/// mesh node [soft 0x08458b60 SI_meshUpdateDefinitions]: the definitions are walked as
/// `SI_modelNextDefGet` walks them (stored ones, the working one if it is not the stored
/// one in use, the original one); the one that is `from` itself is passed over. True when
/// the walk came to its end; false for a node that is not a mesh, and as soon as one
/// definition has another shape ([`mesh_copy_attributes`]): the definitions before it have
/// been written by then.
pub fn mesh_update_definitions(models: &mut Models, node: NodeId, from: DefSlot, what: i32) -> bool {
    if models.m(node).kind != 4 {
        return false;
    }
    let nb_uv = Some(models.m(node).nb_uv);
    let Some(from) = object(models, node, from) else { return false };
    let Some(Definition::Mesh(src)) = object_mut(models, node, from).map(|d| d.clone()) else { return false };
    let mut cursor = 0i32;
    while let Some(slot) = next_def_get(models, node, &mut cursor) {
        let Some(o) = object(models, node, slot) else { break };
        if o == from {
            continue;
        }
        // (a definition of another kind under a mesh node is read as a mesh by the original)
        let Some(Definition::Mesh(dst)) = object_mut(models, node, o) else { return false };
        if !mesh_copy_attributes(nb_uv, &src, dst, what) {
            return false;
        }
    }
    true
}

/// What [`model_convert_to_mesh`] calls for the kinds whose conversion lives in other
/// source files.
pub trait ConvertHooks {
    /// `SI_patchConvertToMesh(model, patch)` (`patch.c`).
    fn patch_convert_to_mesh(&mut self, _models: &Models, _node: NodeId) -> Option<Mesh> {
        None
    }
    /// `SI_meta2Mesh(system)` (the metaball code).
    fn meta_to_mesh(&mut self, _models: &Models, _node: NodeId) -> Option<Mesh> {
        None
    }
    /// `SI_nurbsConvertToMesh(model, surface)` (the NURBS code).
    fn nurbs_convert_to_mesh(&mut self, _models: &Models, _node: NodeId) -> Option<Mesh> {
        None
    }
    /// `SI_faceConvertToMesh(face, 0)` (`face.c`; see [`crate::face`]).
    fn face_convert_to_mesh(&mut self, _models: &Models, _node: NodeId) -> Option<Mesh> {
        None
    }
}

/// A mesh made from a node's working definition [soft 0x084587d4 SI_modelConvertToMesh],
/// by the node's kind: a patch (1), a face (2), a metaball system (7) and a NURBS surface
/// (10) through their conversion routines, a mesh (4) through `SI_meshCopy`; nothing for
/// any other kind or for no node.
pub fn model_convert_to_mesh(models: &Models, node: Option<NodeId>, env: &mut MeshEnv, hooks: &mut dyn ConvertHooks) -> Option<Mesh> {
    let node = node?;
    match models.m(node).kind {
        1 => hooks.patch_convert_to_mesh(models, node),
        2 => hooks.face_convert_to_mesh(models, node),
        4 => match crate::hierarchy::definition(models, node, DefSlot::Current) {
            Some(Definition::Mesh(m)) => Some(crate::obj_global::mesh_copy_with(m, env)),
            // (the original copies whatever the pointer leads to)
            _ => None,
        },
        7 => hooks.meta_to_mesh(models, node),
        10 => hooks.nurbs_convert_to_mesh(models, node),
        _ => None,
    }
}

/// Sets every corner's colour to the default 0xffb2b2b2
/// [soft 0x08458bc8 SI_meshInitVertexColors].
pub fn mesh_init_vertex_colors(mesh: &mut Mesh) {
    for p in &mut mesh.polygons {
        for c in &mut p.nodes {
            c.color = 0xffb2_b2b2;
        }
    }
}
