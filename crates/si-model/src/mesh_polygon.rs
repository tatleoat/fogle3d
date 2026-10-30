// SPDX-License-Identifier: MIT
//! The polygon tools' view of a mesh: the original's `meshPolygon.c` (finding 0015).
//!
//! A mesh keeps two lists beside its arrays: the **selected polygons** (+0x30; polygon flag
//! bit 0x1) and **their vertices** (+0x34), each vertex with the list of the selected
//! polygons that have a corner there. Every node of the first list carries a coordinate
//! system: that of the group of connected selected polygons it belongs to (average normal,
//! an edge of one polygon projected into its plane, the average centroid).
//! [`update_polygon_lists`] rebuilds both; it runs at the end of every mesh rebuild
//! ([`crate::mesh_b::update_nodes`]).
//!
//! The tools then move the selected polygons through a matrix built in that system
//! ([`transform_selected_polygons`]) and repair normals and discontinuities of what was
//! touched ([`update_tagged`]).
//!
//! Not ported from this source file: the extrusion (`meshExtrudePolygonSelected2`,
//! `SI_meshDuplicatePolygonSelected`, `SI_meshExtrudePolOnSpline`,
//! `meshPolyExtrudeTransform`, `meshTransformCurveToPoly`) and `SI_meshRefFrameCreate`
//! (a command over the selection on screen). The finding says what each does as far as
//! read.

use si_math::algebra::{
    matrix_copy, matrix_inverse, matrix_multiply, matrix_scaling, matrix_translation, matrix_xyz_rotation, vector_multiply_matrix,
    vector_normalize,
};
use si_math::types::{Matrix, Vector};
use si_x87::{x, F80};

use crate::mesh::{ListItem, ListNode, Mesh, MeshList, POLY_HIDDEN, POLY_QUEUED, POLY_SELECTED, POLY_VISITED};
use crate::mesh_b::{compute_hard_edge, computes_polygon_normal, computes_vertex_normal, MeshEnv};
use crate::node::{Models, NodeId};
use crate::obj_global::mesh_polygon_centroid;
use crate::transform::Globals;

const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// The double 0.0001 of this file (0x0865fa88, 0x0865fa90): the squared length below which
/// an average normal, or a projected vector, counts as none, and the size a dot product must
/// exceed for [`proj_vect`] to subtract anything.
const SMALL: f64 = 1e-4;

/// A new node at the front of a list [soft 0x08455cc8 SI_meshAddList]: 0x50 zeroed bytes
/// with the item at +0 and the old head at +0x4c. Returns the node's place in `list.nodes`
/// (the original returns its address). Class: integer.
pub fn add_list(list: &mut MeshList, item: ListItem) -> usize {
    list.nodes.push(ListNode { item, sub: MeshList::default(), sub2: MeshList::default(), matrix: [[0.0; 4]; 4] });
    list.nodes.len() - 1
}

/// Hangs `tail` behind the last node of `list` [soft 0x08455d18 SI_meshConcatList].
pub fn concat_list(list: &mut MeshList, tail: MeshList) {
    let mut nodes = tail.nodes;
    nodes.append(&mut list.nodes);
    list.nodes = nodes;
}

/// Frees a list with the two lists of every node [soft 0x08455d40 SI_meshFreeList] and
/// clears the caller's pointer. Always answers 1.
pub fn free_list(list: &mut MeshList) -> i32 {
    list.nodes.clear();
    1
}

/// The first node from the head whose item is `item` [soft 0x08455d9c SI_meshFindList], as
/// its place in `list.nodes`.
pub fn find_list(list: &MeshList, item: ListItem) -> Option<usize> {
    list.nodes.iter().rposition(|n| n.item == item)
}

/// The number of nodes [soft 0x08455dc0 SI_meshCntList].
pub fn cnt_list(list: &MeshList) -> i32 {
    list.nodes.len() as i32
}

/// The item of the first node whose record **begins with the word** `value`
/// [soft 0x08455de0 SI_meshFindVertex]. The original compares the first four bytes of what
/// each node points at with a `long`; `first_word` reads them for an item (the bits of a
/// vertex's x or a polygon's normal x in the lists of this file).
pub fn find_vertex(list: &MeshList, value: i32, first_word: impl Fn(ListItem) -> i32) -> Option<ListItem> {
    list.iter().find(|n| first_word(n.item) == value).map(|n| n.item)
}

/// Clears the two scratch marks (0x4 visited, 0x8 queued) of every polygon of a list
/// [soft 0x08455ff4 clearTagMark].
pub fn clear_tag_mark(mesh: &mut Mesh, list: &MeshList) {
    for n in list.iter() {
        if let ListItem::Polygon(p) = n.item {
            if let Some(poly) = mesh.polygons.get_mut(p as usize) {
                poly.flag &= !(POLY_VISITED | POLY_QUEUED);
            }
        }
    }
}

/// The polygons round the corners of polygon `p` that carry neither scratch mark (and, with
/// `selected_only`, are selected) [soft 0x08456018 getPolygonNeighbors]: every star entry
/// of every corner's vertex, in order; each polygon found is marked 0x8 and put at the
/// front of the list. The polygon itself is found too unless it is marked.
pub fn get_polygon_neighbors(mesh: &mut Mesh, p: i32, selected_only: bool) -> MeshList {
    let mut out = MeshList::default();
    let corners: Vec<i32> = mesh.polygons[p as usize].nodes.iter().map(|c| c.vertex).collect();
    for v in corners {
        if v == -1 {
            continue;
        }
        // (a corner outside the vertex array: the original reads a star from foreign memory)
        let Some(vertex) = mesh.vertices.get(v as usize) else { continue };
        for k in 0..vertex.star.len() {
            let s = mesh.vertices[v as usize].star[k];
            if s.vertex == -1 || s.polygon == -1 {
                continue;
            }
            let Some(q) = mesh.polygons.get_mut(s.polygon as usize) else { continue };
            if q.flag & (POLY_VISITED | POLY_QUEUED) == 0 && (!selected_only || q.flag & POLY_SELECTED != 0) {
                add_list(&mut out, ListItem::Polygon(s.polygon));
                q.flag |= POLY_QUEUED;
            }
        }
    }
    out
}

/// Everything connected to polygon `p` through shared vertices, depth first
/// [soft 0x084560e4 getConnectedPolygons]: `p` is marked 0x4 and, unless `selected_only`
/// and it is not selected, put at the front of `out`; then the same for each of its
/// neighbours ([`get_polygon_neighbors`]) in list order. A polygon already marked 0x4 is
/// left alone.
///
/// The original recurses once per polygon; this walk keeps its own stack and visits in the
/// same order.
pub fn get_connected_polygons(mesh: &mut Mesh, p: Option<i32>, selected_only: bool, out: &mut MeshList) {
    // each frame: a neighbour list and how many of its nodes (from the head) are done
    let mut stack: Vec<(Vec<i32>, usize)> = vec![(p.into_iter().collect(), 0)];
    while let Some((list, at)) = stack.last_mut() {
        if *at == list.len() {
            stack.pop();
            continue;
        }
        let q = list[*at];
        *at += 1;
        let Some(poly) = mesh.polygons.get_mut(q as usize) else { continue };
        if poly.flag & POLY_VISITED != 0 {
            continue;
        }
        poly.flag |= POLY_VISITED;
        if !selected_only || poly.flag & POLY_SELECTED != 0 {
            add_list(out, ListItem::Polygon(q));
        }
        let near = get_polygon_neighbors(mesh, q, selected_only);
        let near: Vec<i32> = near.iter().filter_map(|n| if let ListItem::Polygon(i) = n.item { Some(i) } else { None }).collect();
        if !near.is_empty() {
            stack.push((near, 0));
        }
    }
}

/// The two lists of a mesh's selection [soft 0x08455ef4 SI_meshGetSelPolInfo]: every
/// polygon with flag bit 0x1 goes to the front of `polygons`; each of its corners' vertices
/// gets a node in `vertices` the first time it is met, and the polygon goes to the front of
/// that node's own list. Nodes are **added** to what the two lists hold.
pub fn get_sel_pol_info(polygons: &mut MeshList, vertices: &mut MeshList, mesh: &Mesh) {
    // the original's table from vertex number to list node (SI_calloc, nb_vertices words)
    let mut node_of: Vec<Option<usize>> = vec![None; mesh.vertices.len()];
    for (p, poly) in mesh.polygons.iter().enumerate() {
        if poly.flag & POLY_SELECTED == 0 {
            continue;
        }
        add_list(polygons, ListItem::Polygon(p as i32));
        for c in &poly.nodes {
            if c.vertex == -1 {
                continue;
            }
            // A corner outside the vertex array makes the original read and write beyond
            // its table; here such a corner gets a node of its own each time.
            let known = node_of.get(c.vertex as usize).copied().flatten();
            let at = match known {
                Some(at) => at,
                None => {
                    let at = add_list(vertices, ListItem::Vertex(c.vertex));
                    if let Some(slot) = node_of.get_mut(c.vertex as usize) {
                        *slot = Some(at);
                    }
                    at
                }
            };
            add_list(&mut vertices.nodes[at].sub, ListItem::Polygon(p as i32));
        }
    }
}

/// Collects every selected polygon that hangs together with polygon `p` through shared
/// vertices [soft 0x08455e0c addPolsToList]. `vertices` is a mesh's list of selected
/// vertices (each node with its selected polygons); `done` receives every polygon visited
/// and `found` every polygon reached from another one: `p` itself goes to `done` only.
///
/// For each corner of `p` the vertex's node is looked up and each polygon of its list that
/// is not yet in `found` is put there and visited in turn; a polygon already in `done` is
/// not visited again. A corner whose vertex has no node (a polygon that is not selected)
/// makes the original read through a null pointer; here such a corner is passed over.
///
/// The original recurses once per polygon; this walk keeps its own stack and visits in the
/// same order.
pub fn add_pols_to_list(found: &mut MeshList, vertices: &MeshList, done: &mut MeshList, mesh: &Mesh, p: i32) {
    // each frame: a polygon and the (corner, place in the vertex's list) reached
    let mut stack: Vec<(i32, usize, usize)> = Vec::new();
    let enter = |q: i32, done: &mut MeshList, stack: &mut Vec<(i32, usize, usize)>| {
        if find_list(done, ListItem::Polygon(q)).is_none() {
            add_list(done, ListItem::Polygon(q));
            stack.push((q, 0, 0));
        }
    };
    enter(p, done, &mut stack);
    while let Some(&(q, corner_at, sub_at)) = stack.last() {
        let Some(poly) = mesh.polygons.get(q as usize) else {
            stack.pop();
            continue;
        };
        let Some(c) = poly.nodes.get(corner_at) else {
            stack.pop();
            continue;
        };
        let node = if c.vertex == -1 { None } else { find_list(vertices, ListItem::Vertex(c.vertex)) };
        let next = node.and_then(|at| vertices.nodes[at].sub.at(sub_at)).map(|n| n.item);
        let top = stack.last_mut().unwrap();
        match next {
            None => *top = (q, corner_at + 1, 0),
            Some(item) => {
                top.2 += 1;
                if find_list(found, item).is_none() {
                    add_list(found, item);
                    if let ListItem::Polygon(r) = item {
                        enter(r, done, &mut stack);
                    }
                }
            }
        }
    }
}

/// Takes from `d` its part along `n` [soft 0x084562c4 projVect]: with t = (d.y·n.y +
/// d.x·n.x) + d.z·n.z in the register, the result is `d` itself when |t| is not above
/// 0.0001; otherwise d − r32(n·t) per component, and the answer is false when the squared
/// length of that difference (from the unrounded components, (z² + (y² + x²))) is below
/// 0.0001. Class: model.
pub fn proj_vect(d: [f32; 3], n: [f32; 3]) -> ([f32; 3], bool) {
    let t = (x(d[1]) * n[1] + x(d[0]) * n[0]) + x(d[2]) * n[2];
    if !t.abs().gt(x(SMALL)) {
        return (d, true);
    }
    let along = [(x(n[0]) * t).to_f32(), (x(n[1]) * t).to_f32(), (t * n[2]).to_f32()];
    let r = [x(d[0]) - along[0], x(d[1]) - along[1], x(d[2]) - along[2]];
    let out = [r[0].to_f32(), r[1].to_f32(), r[2].to_f32()];
    let len2 = r[2] * r[2] + (r[1] * r[1] + r[0] * r[0]);
    (out, !x(SMALL).gt(len2))
}

fn pos(mesh: &Mesh, vertex: i32) -> [f32; 3] {
    // an index outside the array (a missing corner's −1 among them) reads foreign memory
    // in the original
    mesh.vertices.get(vertex as usize).map_or([0.0; 3], |v| v.pos)
}

fn corner(mesh: &Mesh, p: i32, k: usize) -> i32 {
    // the original reads the k-th corner record whatever the polygon's count
    mesh.polygons[p as usize].nodes.get(k).map_or(-1, |c| c.vertex)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [(x(a[0]) - b[0]).to_f32(), (x(a[1]) - b[1]).to_f32(), (x(a[2]) - b[2]).to_f32()]
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let mut t = Vector::from_array(v);
    vector_normalize(&mut t);
    t.to_array()
}

/// `a` × `n` as the two coordinate-system routines form it: each component one difference
/// of two products, rounded once.
fn cross(a: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    [
        (x(a[1]) * n[2] - x(a[2]) * n[1]).to_f32(),
        (x(a[2]) * n[0] - x(a[0]) * n[2]).to_f32(),
        (x(n[1]) * a[0] - x(n[0]) * a[1]).to_f32(),
    ]
}

fn frame(xa: [f32; 3], n: [f32; 3], z: [f32; 3], origin: [f32; 3]) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    m[0][..3].copy_from_slice(&xa);
    m[1][..3].copy_from_slice(&n);
    m[2][..3].copy_from_slice(&z);
    m[3][..3].copy_from_slice(&origin);
    m
}

/// The coordinate system of one polygon [soft 0x08455478 getPolyCoordSystem]: row 0 the
/// direction from its second corner to its first, normalised; row 1 its stored normal;
/// row 2 their cross product, normalised; row 3 its centroid. Class: model.
pub fn get_poly_coord_system(mesh: &Mesh, p: i32) -> Matrix {
    let xa = normalized(sub(pos(mesh, corner(mesh, p, 0)), pos(mesh, corner(mesh, p, 1))));
    let n = mesh.polygons[p as usize].normal;
    let z = normalized(cross(xa, n));
    let (c, _) = mesh_polygon_centroid(mesh, p);
    frame(xa, n, z, c)
}

/// The coordinate system of a group of polygons [soft 0x084555d0 getAverageCoordSystem].
/// Class: model.
///
/// - Row 1: the sum of the polygons' normals (float sums, in list order) times 1/count,
///   normalised (`SI_vectorNormalize`).
/// - Row 3: the sum of their centroids ([`mesh_polygon_centroid`]) times 1/count.
/// - Row 0: of the **last** polygon of the list, the vector from its second corner to its
///   first, with its part along row 1 taken out ([`proj_vect`]); if nothing is left, the
///   same from its third corner; if nothing again, a status message and the world axes.
///   Normalised.
/// - Row 2: row 0 × row 1, normalised.
///
/// When the average normal's squared length is below 0.0001 the three rows are the world
/// axes X, Y, Z. The 1/count factor stays in the register (it is not rounded to float).
///
/// For an empty list the original divides by zero and then reads the polygon through a
/// register nothing set; no caller passes one. Here the answer is then the world axes at
/// the origin.
pub fn get_average_coord_system(list: &MeshList, mesh: &Mesh, env: &mut MeshEnv) -> Matrix {
    let mut n = [0.0f32; 3];
    let mut c = [0.0f32; 3];
    let mut count = 0i32;
    let mut last = None;
    for node in list.iter() {
        let ListItem::Polygon(p) = node.item else { continue };
        let pn = mesh.polygons[p as usize].normal;
        n = [(x(n[0]) + pn[0]).to_f32(), (x(n[1]) + pn[1]).to_f32(), (x(n[2]) + pn[2]).to_f32()];
        let (pc, _) = mesh_polygon_centroid(mesh, p);
        c = [(x(c[0]) + pc[0]).to_f32(), (x(c[1]) + pc[1]).to_f32(), (x(c[2]) + pc[2]).to_f32()];
        count += 1;
        last = Some(p);
    }
    let Some(last) = last else {
        return matrix_copy(&IDENTITY);
    };
    let inv: F80 = x(1.0f32) / x(count);
    n = normalized([(inv * n[0]).to_f32(), (inv * n[1]).to_f32(), (inv * n[2]).to_f32()]);
    c = [(inv * c[0]).to_f32(), (inv * c[1]).to_f32(), (inv * c[2]).to_f32()];
    let axes: ([f32; 3], [f32; 3], [f32; 3]) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    if ((x(n[1]) * n[1] + x(n[0]) * n[0]) + x(n[2]) * n[2]).lt(x(SMALL)) {
        return frame(axes.0, axes.1, axes.2, c);
    }
    let first = pos(mesh, corner(mesh, last, 0));
    let (mut v, mut ok) = proj_vect(sub(first, pos(mesh, corner(mesh, last, 1))), n);
    if !ok {
        (v, ok) = proj_vect(sub(first, pos(mesh, corner(mesh, last, 2))), n);
    }
    if !ok {
        env.messages.push(si_rt::text(0x086d_57c0).to_string());
        v = axes.0;
        n = axes.1;
    }
    let xa = normalized(v);
    let z = normalized(cross(xa, n));
    frame(xa, n, z, c)
}

/// Gives every node of the list of selected polygons the coordinate system of its group
/// [soft 0x08456174 SI_meshGetSelPolCoordSys].
///
/// When the list holds as many nodes as the mesh has polygons, all of them are one group
/// ([`get_average_coord_system`] of the whole list). Otherwise the scratch marks of the
/// listed polygons are cleared and, for each listed polygon not yet visited, the selected
/// polygons connected to it ([`get_connected_polygons`]) form a group whose system is
/// written into each member's node. The marks 0x4 and 0x8 stay set afterwards.
pub fn get_sel_pol_coord_sys(list: &mut MeshList, mesh: &mut Mesh, env: &mut MeshEnv) {
    if mesh.nb_polygons() >= 1 && cnt_list(list) == mesh.nb_polygons() {
        let m = get_average_coord_system(list, mesh, env);
        for n in list.iter_mut() {
            n.matrix = matrix_copy(&m);
        }
        return;
    }
    clear_tag_mark(mesh, list);
    let heads: Vec<ListItem> = list.iter().map(|n| n.item).collect();
    for item in heads {
        let ListItem::Polygon(p) = item else { continue };
        if mesh.polygons.get(p as usize).is_none_or(|q| q.flag & POLY_VISITED != 0) {
            continue;
        }
        let mut group = MeshList::default();
        get_connected_polygons(mesh, Some(p), true, &mut group);
        if group.is_empty() {
            continue;
        }
        let m = get_average_coord_system(&group, mesh, env);
        for g in group.iter() {
            // (a group member that is not in the list would make the original write
            // through a null pointer; every selected polygon is listed)
            if let Some(at) = find_list(list, g.item) {
                list.nodes[at].matrix = matrix_copy(&m);
            }
        }
    }
}

/// Rebuilds the two selection lists of a mesh [soft 0x084584a8 SI_meshUpdatePolygonLists]:
/// both freed, [`get_sel_pol_info`], [`get_sel_pol_coord_sys`]. (The original brackets
/// this with the activity cursor.)
pub fn update_polygon_lists(mesh: &mut Mesh, env: &mut MeshEnv) {
    let (mut polygons, mut vertices) = (MeshList::default(), MeshList::default());
    get_sel_pol_info(&mut polygons, &mut vertices, mesh);
    get_sel_pol_coord_sys(&mut polygons, mesh, env);
    mesh.selected_polygons = polygons;
    mesh.selected_vertices = vertices;
}

/// The lists of `dst` rebuilt, keeping the coordinate systems of `src`'s
/// [soft 0x08458514 SI_meshCopyPolygonLists]. `src` is `None` when it is `dst` itself: then
/// `dst`'s old lists are taken off first and freed at the end.
///
/// The new nodes are **added** to `dst`'s lists ([`get_sel_pol_info`]); then the matrix of
/// each node of the old polygon list is copied onto the node at the same place of the new
/// one, head for head. Where the new list is shorter the original writes through a null
/// pointer; here the rest is dropped.
pub fn copy_polygon_lists(src: Option<&Mesh>, dst: &mut Mesh, env: &mut MeshEnv) {
    let old = match src {
        Some(s) => s.selected_polygons.clone(),
        None => {
            dst.selected_vertices = MeshList::default();
            core::mem::take(&mut dst.selected_polygons)
        }
    };
    let (mut polygons, mut vertices) = (core::mem::take(&mut dst.selected_polygons), core::mem::take(&mut dst.selected_vertices));
    get_sel_pol_info(&mut polygons, &mut vertices, dst);
    get_sel_pol_coord_sys(&mut polygons, dst, env);
    for (new, old) in polygons.iter_mut().zip(old.iter()) {
        new.matrix = matrix_copy(&old.matrix);
    }
    dst.selected_polygons = polygons;
    dst.selected_vertices = vertices;
}

/// For an extrusion that keeps the polygons apart, gives every node of a polygon list a
/// coordinate system of its own [soft 0x084570e8 computeTransformForExtrusion]: unless
/// `together`, each node's own list becomes that one polygon and its matrix
/// [`get_average_coord_system`] of it.
pub fn compute_transform_for_extrusion(list: &mut MeshList, mesh: &Mesh, together: bool, env: &mut MeshEnv) {
    if together {
        return;
    }
    for n in list.iter_mut() {
        free_list(&mut n.sub);
        add_list(&mut n.sub, n.item);
        n.matrix = get_average_coord_system(&n.sub, mesh, env);
    }
}

/// Recomputes what a move has marked [soft 0x084585bc SI_meshUpdateTagged].
///
/// 1. Every polygon with flag 0x400: its normal ([`computes_polygon_normal`]), the flag
///    cleared; each of its corners' vertices gets flag bit 15 and each corner's edge gets
///    0x8000.
/// 2. Every edge with 0x8000: [`compute_hard_edge`], the bit cleared.
/// 3. Every vertex with bit 15: [`computes_vertex_normal`], the bit cleared.
///
/// (Step 2 uses the display mark of `SI_meshOptimizeDisplay` as its work mark: on a mesh
/// whose display lines are built, where every edge has it, every edge is recomputed and
/// loses the mark.)
pub fn update_tagged(mesh: &mut Mesh) {
    for p in 0..mesh.polygons.len() {
        if mesh.polygons[p].flag & 0x400 == 0 {
            continue;
        }
        computes_polygon_normal(mesh, p as i32);
        mesh.polygons[p].flag &= !0x400;
        for k in 0..mesh.polygons[p].nodes.len() {
            let (v, e) = (mesh.polygons[p].nodes[k].vertex, mesh.polygons[p].nodes[k].edge);
            if v == -1 {
                continue;
            }
            // (indices outside the arrays write into foreign memory in the original)
            if let Some(vertex) = mesh.vertices.get_mut(v as usize) {
                vertex.flag = (vertex.flag as u16 | 0x8000) as i16;
            }
            if let Some(edge) = mesh.edges.get_mut(e.unsigned_abs() as usize) {
                edge.flag = (edge.flag as u16 | 0x8000) as i16;
            }
        }
    }
    for e in 1..=mesh.nb_edges() {
        if mesh.edges[e as usize].flag < 0 {
            compute_hard_edge(mesh, e);
            mesh.edges[e as usize].flag &= 0x7fff;
        }
    }
    for v in 0..mesh.nb_vertices() {
        if mesh.vertices[v as usize].flag < 0 {
            computes_vertex_normal(mesh, v);
            mesh.vertices[v as usize].flag &= 0x7fff;
        }
    }
}

/// The three kinds of move of [`transform_selected_polygons`] (the original's `long`).
pub const MOVE_TRANSLATE: i32 = 0;
pub const MOVE_ROTATE: i32 = 1;
pub const MOVE_SCALE: i32 = 2;

/// What the transformation routines of this file call outside it.
pub trait Hooks {
    /// `SI_matrixRotationReferTag(view, &angles, gmat, out)` (`model.c`, not ported): the
    /// rotation by three angles about the world's, the reference object's or the view's
    /// axes, for the rotation modes 1, 3 and 4.
    fn matrix_rotation_refer_tag(&mut self, _angles: Vector, _gmat: &Matrix) -> Matrix {
        IDENTITY
    }
}

impl Hooks for () {}

fn zero_row3(m: &mut Matrix) {
    m[3][0] = 0.0;
    m[3][1] = 0.0;
    m[3][2] = 0.0;
}

/// A translation of selected polygons as a matrix [soft 0x084552e0 getPolyTranslate]:
/// the vector is brought into the mesh's space as the translation mode (`SI_glbManip`
/// + 0x0c) says, and `out` becomes the translation by the result, which is returned.
///
/// With L the inverse of the model's global matrix, its row 3 cleared:
///
/// | Mode | The vector goes through |
/// |---|---|
/// | 0 | the polygon group's coordinate system (`system`, row 3 cleared) |
/// | 1, 4 | L |
/// | 2 | father's global matrix × L (row 3 cleared), or nothing for a root |
/// | 3 | the reference object's global matrix × L (row 3 cleared) |
///
/// `out` is the caller's matrix and is used as the work space: when the model's matrix has
/// no inverse, L is whatever `out` held (counted, [`crate::env::stale`]). In any other mode
/// the original returns three floats it never set; here zeros, counted likewise. Class:
/// model.
pub fn get_poly_translate(system: &Matrix, models: &Models, model: NodeId, v: Vector, g: &Globals, out: &mut Matrix) -> Vector {
    match matrix_inverse(&models.m(model).gmat) {
        Some(inv) => *out = inv,
        None => crate::env::stale::note(),
    }
    zero_row3(out);
    let through = |a: Option<NodeId>, out: &mut Matrix| match a {
        Some(a) => {
            let t = matrix_copy(&models.m(a).gmat);
            *out = matrix_multiply(&t, out);
            zero_row3(out);
            vector_multiply_matrix(&v, out)
        }
        None => {
            // the original reads the reference object through a null pointer
            crate::env::stale::note();
            v
        }
    };
    let t = match g.trans_mode {
        0 => {
            *out = matrix_copy(system);
            zero_row3(out);
            vector_multiply_matrix(&v, out)
        }
        1 | 4 => vector_multiply_matrix(&v, out),
        2 => match models.father(model) {
            None => v,
            f => through(f, out),
        },
        3 => through(g.ref_model, out),
        _ => {
            crate::env::stale::note();
            Vector::new(0.0, 0.0, 0.0)
        }
    };
    *out = matrix_translation(&t);
    t
}

/// A rotation of selected polygons as a matrix [soft 0x0845598c getPolyRotateMatrix], by
/// the rotation mode (`SI_glbManip` + 0x08): modes 1, 3 and 4 through
/// `SI_matrixRotationReferTag` with the model's global matrix (a hook); mode 0 about the
/// polygon group's own axes, system⁻¹ × R(angles) × system; any other mode R(angles)
/// alone. Class: libm.
///
/// When the system has no inverse the original multiplies with a matrix it never set
/// (zeros here, counted).
pub fn get_poly_rotate_matrix(system: &Matrix, models: &Models, model: NodeId, angles: Vector, g: &Globals, hooks: &mut dyn Hooks) -> Matrix {
    match g.rot_mode {
        1 | 3 | 4 => hooks.matrix_rotation_refer_tag(angles, &models.m(model).gmat),
        0 => about(system, matrix_xyz_rotation(&angles)),
        _ => matrix_xyz_rotation(&angles),
    }
}

/// system⁻¹ × m × system.
fn about(system: &Matrix, m: Matrix) -> Matrix {
    let s = matrix_copy(system);
    let inv = matrix_inverse(&s).unwrap_or_else(|| {
        crate::env::stale::note();
        [[0.0; 4]; 4]
    });
    matrix_multiply(&matrix_multiply(&inv, &m), &s)
}

/// A scaling of selected polygons as a matrix [soft 0x08455a30 getPolyScaleMatrix]:
/// system⁻¹ × S(factors) × system, the scaling along the polygon group's own axes about
/// its centroid. Class: model.
pub fn get_poly_scale_matrix(system: &Matrix, factors: Vector) -> Matrix {
    about(system, matrix_scaling(&factors))
}

/// The matrix of one move [soft 0x0845521c getPolyTransformMatrix]: [`get_poly_translate`]
/// for kind 0, [`get_poly_rotate_matrix`] for 1, [`get_poly_scale_matrix`] for 2. For any
/// other kind `out` is left as it is.
#[allow(clippy::too_many_arguments)]
pub fn get_poly_transform_matrix(
    system: &Matrix, models: &Models, model: NodeId, kind: i32, v: Vector, g: &Globals, hooks: &mut dyn Hooks, out: &mut Matrix,
) {
    match kind {
        MOVE_ROTATE => *out = get_poly_rotate_matrix(system, models, model, v, g, hooks),
        MOVE_TRANSLATE => {
            get_poly_translate(system, models, model, v, g, out);
        }
        MOVE_SCALE => *out = get_poly_scale_matrix(system, v),
        _ => {}
    }
}

/// Moves the selected polygons of a mesh [soft 0x08455054 SI_meshTransformSelectedPolygons].
///
/// For every node of the list of selected polygons whose polygon is not hidden (0x800):
/// the move's matrix M in that node's coordinate system ([`get_poly_transform_matrix`]);
/// every corner's vertex that is in the list of selected vertices and does not have flag
/// bit 15 yet goes through M as ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ, gets the flag bits 0x8020,
/// and every polygon of its star gets 0x400. Unless the move is a scaling, the node's
/// system becomes system × M: **the group's axes move with it**. At the end
/// [`update_tagged`] recomputes the normals of what was marked.
///
/// Bit 15 is what keeps a vertex shared by two selected polygons from moving twice. Bit
/// 0x20 of a vertex flag stays set afterwards. M is one local of the original for all
/// nodes: a kind other than 0, 1, 2 uses whatever it held (zeros at first, counted).
/// Class: model.
#[allow(clippy::too_many_arguments)]
pub fn transform_selected_polygons(
    mesh: &mut Mesh, models: &Models, model: NodeId, kind: i32, v: Vector, g: &Globals, hooks: &mut dyn Hooks,
) {
    let mut m: Matrix = [[0.0; 4]; 4];
    if !(0..=2).contains(&kind) && !mesh.selected_polygons.is_empty() {
        crate::env::stale::note();
    }
    for at in (0..mesh.selected_polygons.nodes.len()).rev() {
        let ListItem::Polygon(p) = mesh.selected_polygons.nodes[at].item else { continue };
        if mesh.polygons[p as usize].flag & POLY_HIDDEN != 0 {
            continue;
        }
        let system = mesh.selected_polygons.nodes[at].matrix;
        get_poly_transform_matrix(&system, models, model, kind, v, g, hooks, &mut m);
        for k in 0..mesh.polygons[p as usize].nodes.len() {
            let c = mesh.polygons[p as usize].nodes[k].vertex;
            if c == -1 || find_list(&mesh.selected_vertices, ListItem::Vertex(c)).is_none() {
                continue;
            }
            let Some(vertex) = mesh.vertices.get_mut(c as usize) else { continue };
            if vertex.flag < 0 {
                continue;
            }
            let [px, py, pz] = vertex.pos;
            let col = |j: usize| (((x(py) * m[1][j] + x(px) * m[0][j]) + x(pz) * m[2][j]) + m[3][j]).to_f32();
            vertex.pos = [col(0), col(1), col(2)];
            vertex.flag = (vertex.flag as u16 | 0x8020) as i16;
            for s in 0..vertex.star.len() {
                let q = mesh.vertices[c as usize].star[s].polygon;
                if q != -1 {
                    if let Some(poly) = mesh.polygons.get_mut(q as usize) {
                        poly.flag |= 0x400;
                    }
                }
            }
        }
        if kind != MOVE_SCALE {
            mesh.selected_polygons.nodes[at].matrix = matrix_multiply(&system, &m);
        }
    }
    update_tagged(mesh);
}

#[cfg(test)]
mod tests;
