// SPDX-License-Identifier: MIT
//! The rebuild of a mesh's connectivity and normals: the original's `mesh_b.c` (finding 0015).
//!
//! A mesh comes out of a file, a primitive or an editing tool as positions and polygons.
//! [`update_nodes`] then does what the program does: it files every polygon corner in the
//! star of its vertex and creates the edges ([`insert_ipolygon`], [`insert_polygon_at_vertex`],
//! [`insert_edge`]), computes the polygon normals ([`computes_polygon_normal`]), marks border
//! and discontinuity edges ([`compute_hard_edge`]), computes the corner normals
//! ([`computes_vertex_normal`]) and builds the wire-frame polylines ([`optimize_display`]).
//!
//! Exactness classes as in `si-math` (finding 0008): **integer** (no floating point),
//! **copy**, **single** (each stored value is one operation on floats), **model** (several
//! operations in an x87 register before a store). Nothing here calls the C library's
//! trigonometry.
//!
//! The allocators of this source file (`SI_meshAllocate`, `SI_vertexAllocate`,
//! `SI_polygonAllocate`, `SI_polygonNodeAllocate`) are ported in `si-types`; here their
//! arrays are `Vec`s (see [`crate::mesh`]).

use si_math::algebra::vector_cos_angle;
use si_math::types::Vector;
use si_x87::{x, F80};

use crate::mesh::{Edge, Mesh, PolyNode, Polygon, Star, EDGE_AUTO_HARD, EDGE_BORDER, EDGE_DISPLAYED, EDGE_USER_HARD};

/// The length below which a polygon's summed normal is left at zero: the double 1e-6
/// (0x0865fb00), compared with the unrounded length.
const POLYGON_NORMAL_MIN: f64 = 1e-6;
/// The length a summed corner normal must exceed to be normalised: the double 0.0001
/// (0x0865fb08), compared with the length rounded to float.
const VERTEX_NORMAL_MIN: f64 = 1e-4;

/// How floating-point expressions are evaluated.
///
/// [`Eval::X87`] is the Linux build, the reference of this port: operands are widened to
/// the 80-bit register format and a value is rounded only where the code stores it. Every
/// cited function uses it.
///
/// The other two exist for the corpus check (`model-check`) and reproduce nothing that was
/// read from a binary. They evaluate the **same expressions with the same variable types**
/// as two other builds of the same source would, to tell a difference of arithmetic from a
/// difference of algorithm when a file written by such a build is compared:
///
/// - [`Eval::Single`]: every operation in the precision of its C operands (a float times a
///   float is rounded to float, anything involving a double is computed in double), as a
///   compiler for a processor without extended registers does it (the IRIX build's MIPS; a
///   guess about that build, which the corpus supports: see finding 0015);
/// - [`Eval::Double`]: every operation rounded to double, as an x87 unit under a 53-bit
///   precision control does it (how Windows starts a process; a guess about the NT build).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Eval {
    X87,
    Single,
    Double,
}

/// The three globals the rebuild reads besides the mesh.
#[derive(Clone, Debug, Default)]
pub struct MeshEnv {
    /// The name of the model being worked on, for the collision messages: the original's
    /// `currMdl` (0x086d59c0) when set, else what [`find_model_from_mesh`] finds. `None`
    /// when there is no such model or its name pointer is null.
    pub model_name: Option<Vec<u8>>,
    /// `SI_gblIndex3` (0x08788974): a table the vertex and edge removal tools install while
    /// they rebuild part of a mesh; null (`None`) at any other time. Entry 0 counts down as
    /// edges are created; entry `n` receives the new number of old edge `n`.
    pub index3: Option<Vec<i32>>,
    /// What the original shows on the status line (`SI_statusDisplayMessage(text, 2)`).
    pub messages: Vec<String>,
}

impl MeshEnv {
    /// [soft 0x08458e10 SI_setCurrentModel]: the original stores the model pointer; the only
    /// thing ever read through it is the name.
    pub fn set_current_model(&mut self, name: Option<&[u8]>) {
        self.model_name = name.map(|n| n.to_vec());
    }
}

/// One graf node as [`find_model_from_mesh`] sees it: its kind and, for kinds 2 and 3, the
/// model it shows with that model's definitions.
pub struct GrafModel<'a> {
    pub kind: i32,
    /// `None` for a null data pointer.
    pub model: Option<GrafModelData<'a>>,
}

pub struct GrafModelData<'a> {
    /// The record's `kind` (+0x30).
    pub model_kind: i32,
    /// `None` for a null name pointer.
    pub name: Option<&'a [u8]>,
    /// The meshes `SI_modelNextDefGet` yields for the model, in its order.
    pub meshes: &'a [&'a Mesh],
}

/// The model that owns a mesh [soft 0x08458e20 findModelFromMesh]: the first node of the
/// current graf list (`SI_grafLists[SI_procData[1]]`, followed through each node's `next`)
/// that is a model node (kind 2 or 3) of kind mesh (4) with a name, one of whose definitions
/// is this very mesh. Returns its position in `list`. Class: integer.
///
/// The original compares pointers; here a mesh is "the same" when it is the same object.
pub fn find_model_from_mesh(list: &[GrafModel], mesh: &Mesh) -> Option<usize> {
    list.iter().position(|g| {
        (g.kind == 2 || g.kind == 3)
            && g.model.as_ref().is_some_and(|m| {
                m.model_kind == 4 && m.name.is_some() && m.meshes.iter().any(|d| core::ptr::eq(*d, mesh))
            })
    })
}

/// Sets a polygon's corners [soft 0x0845a694 SI_polygonInitialize]: the count, a fresh
/// corner array (`SI_polygonNodeAllocate`) and one vertex index per corner. Class: integer.
///
/// The original is variadic and stores the count as 16 bits; its array is rounded up to a
/// multiple of four corners, of which only `corners.len()` belong to the polygon.
pub fn polygon_initialize(poly: &mut Polygon, corners: &[i32]) {
    poly.nodes = corners.iter().map(|&vertex| PolyNode { vertex, ..PolyNode::default() }).collect();
}

/// Appends an edge and returns the edge count before the call, one less than the new
/// edge's number [soft 0x08459bf8 meshInsertEdge]. Class: integer.
///
/// The original allocates the first block of 1024 zeroed records on the first call (so
/// record 0 exists and is zero) and a further block whenever the count's low ten bits are
/// all ones. When `SI_gblIndex3` is installed and its entry 0 is not zero, that entry is
/// decremented.
pub fn insert_edge(mesh: &mut Mesh, v0: i32, v1: i32, p0: i32, p1: i32, env: &mut MeshEnv) -> i32 {
    if mesh.edges.is_empty() {
        mesh.edges.push(Edge::default());
    }
    mesh.edges.push(Edge { v0, v1, p0, p1, x10: 0, flag: 0 });
    if let Some(t) = env.index3.as_mut() {
        if let Some(first) = t.first_mut() {
            if *first != 0 {
                *first -= 1;
            }
        }
    }
    mesh.nb_edges() - 1
}

fn collision(env: &mut MeshEnv, named: bool, which: &str, a: i32, b: i32, c: i32, d: i32) {
    // The original formats into a 120-byte stack buffer with sprintf: a model name longer
    // than about 60 bytes overruns it there. Here the text is simply as long as it is.
    let text = match (&env.model_name, named) {
        (Some(name), true) => {
            format!("\"{}\" : collision of polygons ({which}, p{a}, p{b}, v{c}, v{d})", String::from_utf8_lossy(name))
        }
        _ => format!("collision of polygons ({which}, p{a}, p{b}, e{c}, e{d})"),
    };
    env.messages.push(text);
}

/// Files one corner of polygon `p` in the star of its vertex `v` and creates or completes
/// the two edges that meet there [soft 0x08458ea4 meshInsertPolygonAtVertex]. `prev` and
/// `next` are the vertices of the corners before and after `v` in the contour. Returns the
/// signed number of the edge from `v` to `next`, or 0 on failure. Class: integer.
///
/// The star of `v` is kept ordered: the entry of a polygon's `next` vertex is followed by
/// the entry of its `prev` vertex. Four cases, by which of the two vertices already have an
/// entry:
///
/// - neither: `(next, 0, p)` and `(prev, 0, −1)` are appended;
/// - only `prev`: `(next, 0, p)` is inserted before it;
/// - only `next`: its polygon becomes `p` and `(prev, 0, −1)` is inserted after it;
/// - both: the polygon of `next`'s entry becomes `p`, and unless `prev`'s entry already
///   follows the run that ends at `next`'s, the star is rewritten as that run, then
///   `next`'s entry, then the entries from `prev`'s on, then the rest.
///
/// Refused with a status message ("collision of polygons"), leaving the mesh untouched:
/// `next`'s entry already has a polygon (two polygons run from `v` to `next`), or the edge
/// to `prev` already has another polygon on the side `p` would take. Refused silently: an
/// index outside the vertex array.
///
/// The edge to each neighbour is then looked up in the neighbour's star; if the neighbour
/// does not know `v` yet the edge is created with `p` on its first side, otherwise `p` is
/// written on the free side of the existing edge.
pub fn insert_polygon_at_vertex(mesh: &mut Mesh, p: i32, prev: i32, v: i32, next: i32, env: &mut MeshEnv) -> i32 {
    let nv = mesh.nb_vertices();
    if prev < 0 || prev >= nv || v < 0 || v >= nv || next < 0 || next >= nv {
        return 0;
    }
    let vi = v as usize;
    // An entry the original reads and writes just past the star's count (see the rewrite
    // below): it is used for this call and then lost.
    let mut beyond: Option<Star> = None;
    // index of the entry for `next` (the original's two locals start at −1)
    let mut i_next: i32 = -1;
    let mut i_prev: i32 = -1;
    let cur: usize;
    if mesh.vertices[vi].star.is_empty() {
        let star = &mut mesh.vertices[vi].star;
        star.push(Star { vertex: next, edge: 0, polygon: p });
        if next != prev {
            star.push(Star { vertex: prev, edge: 0, polygon: -1 });
        }
        cur = 0;
    } else {
        // The scan stops as soon as both are known; until then a later entry with the same
        // neighbour replaces an earlier one.
        for (k, s) in mesh.vertices[vi].star.iter().enumerate() {
            if s.vertex == next {
                i_next = k as i32;
                if i_prev != -1 {
                    break;
                }
            }
            if s.vertex == prev {
                i_prev = k as i32;
                if i_next != -1 {
                    break;
                }
            }
        }
        if i_next != -1 && mesh.vertices[vi].star[i_next as usize].polygon != -1 {
            let other = mesh.vertices[vi].star[i_next as usize].polygon;
            collision(env, true, "vn", p, other, v, next);
            return 0;
        }
        if i_prev != -1 {
            let e = mesh.vertices[vi].star[i_prev as usize].edge.unsigned_abs() as usize;
            let edge = mesh.edges[e];
            if edge.v0 == v {
                if edge.p1 != -1 && edge.p1 != p {
                    collision(env, true, "vp1", p, edge.p1, prev, v);
                    return 0;
                }
            } else if edge.p0 != -1 && edge.p0 != p {
                // The original tests a register that is always zero here and so never
                // prints the model's name with this message.
                collision(env, false, "vp2", p, edge.p0, prev, v);
                return 0;
            }
        }
        let star = &mut mesh.vertices[vi].star;
        if i_next == -1 {
            if i_prev == -1 {
                cur = star.len();
                star.push(Star { vertex: next, edge: 0, polygon: p });
                star.push(Star { vertex: prev, edge: 0, polygon: -1 });
            } else {
                star.insert(i_prev as usize, Star { vertex: next, edge: 0, polygon: p });
                cur = i_prev as usize;
                i_prev += 1;
            }
        } else if i_prev == -1 {
            star[i_next as usize].polygon = p;
            star.insert(i_next as usize + 1, Star { vertex: prev, edge: 0, polygon: -1 });
            cur = i_next as usize;
        } else {
            // Back from the entry before `next`'s to the start of its run: an entry for
            // `prev`, or one whose edge has no polygon on its second side.
            let mut j = i_next - 1;
            while j >= 0 {
                let s = star[j as usize];
                if s.vertex == prev || mesh.edges[s.edge.unsigned_abs() as usize].p1 == -1 {
                    break;
                }
                j -= 1;
            }
            if j == -1 {
                j = 0;
            }
            star[i_next as usize].polygon = p;
            if j != i_prev {
                let n = star.len() as i32;
                let step = |u: i32| if u == n - 1 { 0 } else { u + 1 };
                let mut out = Vec::with_capacity(star.len());
                let mut u = j;
                while u != i_next {
                    out.push(star[u as usize]);
                    u = step(u);
                }
                let at = out.len();
                out.push(star[i_next as usize]);
                // With `next` different from `prev` the three runs partition the old array.
                let mut u = i_prev;
                while u != j {
                    out.push(star[u as usize]);
                    u = step(u);
                }
                let mut u = step(i_next);
                while u != i_prev {
                    out.push(star[u as usize]);
                    u = step(u);
                }
                // The count does not change: what the copies wrote beyond it is lost (when
                // `next` and `prev` are the same vertex the second and third run repeat
                // entries and the original writes up to twice the count into a block sized
                // for the count rounded up to four).
                if at + 1 >= star.len() {
                    beyond = out.get(at + 1).copied();
                }
                out.resize(star.len(), Star::default());
                *star = out;
                cur = at;
                i_prev = at as i32 + 1;
            } else {
                cur = i_next as usize;
            }
        }
    }
    // the entry of `prev`: found or inserted above, else the one after the current entry
    let star_len = mesh.vertices[vi].star.len();
    let at_prev = if i_prev == -1 { if cur == star_len - 1 { 0 } else { cur + 1 } } else { i_prev as usize };

    // the edge prev → v
    let mut e = match mesh.vertices[vi].star.get(at_prev) {
        Some(s) => s.edge,
        None => beyond.map_or(0, |s| s.edge),
    };
    if e == 0 {
        let known = mesh.vertices[prev as usize].star.iter().position(|s| s.vertex == v);
        match known {
            Some(u) if v != prev => {
                let e2 = mesh.vertices[prev as usize].star[u].edge;
                e = e2.wrapping_neg();
                if e < 1 {
                    mesh.edges[e2 as usize].p0 = p;
                } else {
                    mesh.edges[e as usize].p1 = p;
                }
            }
            _ => e = !insert_edge(mesh, prev, v, p, -1, env),
        }
        if let Some(s) = mesh.vertices[vi].star.get_mut(at_prev) {
            s.edge = e;
        }
    } else if e < 1 {
        mesh.edges[e.unsigned_abs() as usize].p0 = p;
    } else {
        mesh.edges[e as usize].p1 = p;
    }

    // the edge v → next
    let mut e = mesh.vertices[vi].star[cur].edge;
    if e == 0 {
        let known = mesh.vertices[next as usize].star.iter().position(|s| s.vertex == v);
        match known {
            Some(u) if v != next => {
                let e2 = mesh.vertices[next as usize].star[u].edge;
                e = e2.wrapping_neg();
                if e < 1 {
                    mesh.edges[e2 as usize].p1 = p;
                } else {
                    mesh.edges[e as usize].p0 = p;
                }
            }
            _ => e = insert_edge(mesh, v, next, p, -1, env) + 1,
        }
        mesh.vertices[vi].star[cur].edge = e;
    } else if e < 1 {
        mesh.edges[e.unsigned_abs() as usize].p1 = p;
    } else {
        mesh.edges[e as usize].p0 = p;
    }
    e
}

/// Files every corner of polygon `p` [soft 0x0845a528 si_meshInsertIPolygon]: for each
/// contour (the corners between −1 separators) it calls [`insert_polygon_at_vertex`] for the
/// last corner first, then for the others in order, and stores each call's result in the
/// corner as its edge. False as soon as one call fails; the corners filed before stay
/// filed. Class: integer.
///
/// While `SI_gblIndex3` is installed, a corner (other than a contour's last, and not at
/// index 0) whose old edge number has −1 in the table gets its new edge number written
/// there.
pub fn insert_ipolygon(mesh: &mut Mesh, p: i32, env: &mut MeshEnv) -> bool {
    if p < 0 || p >= mesh.nb_polygons() {
        return false;
    }
    let pi = p as usize;
    let nb = mesh.polygons[pi].nodes.len();
    let mut k = 0usize;
    for i in 0..=nb {
        if i == nb || mesh.polygons[pi].nodes[i].vertex == -1 {
            if i > 0 && k <= i - 1 {
                let last = i - 1;
                // The corner before the contour's last. For a contour of one corner the
                // original reads the record before it: the separator (−1, which fails the
                // range test) or, at the start of the array, the four bytes before the
                // allocation. That read cannot be reproduced; it is taken as −1 here.
                let mut prev = if last >= 1 { mesh.polygons[pi].nodes[last - 1].vertex } else { -1 };
                let mut v = mesh.polygons[pi].nodes[last].vertex;
                let mut at = last;
                while k <= last {
                    let next = mesh.polygons[pi].nodes[k].vertex;
                    let old = mesh.polygons[pi].nodes[at].edge.unsigned_abs();
                    let e = insert_polygon_at_vertex(mesh, p, prev, v, next, env);
                    mesh.polygons[pi].nodes[at].edge = e;
                    if e == 0 {
                        return false;
                    }
                    if let Some(t) = env.index3.as_mut() {
                        if old != 0 && k != 0 && t.get(old as usize) == Some(&-1) {
                            t[old as usize] = e.unsigned_abs() as i32;
                        }
                    }
                    prev = v;
                    v = next;
                    at = k;
                    k += 1;
                }
            }
            k = i + 1;
        }
    }
    true
}

fn position(mesh: &Mesh, vertex: i32) -> [f32; 3] {
    // An index outside the array (a separator's −1 among them) makes the original read
    // whatever memory lies there. Such a read only happens for a polygon that starts with a
    // separator or has no corners, where both operands of every difference are that same
    // memory and the terms cancel; zeros give the same result.
    mesh.vertices.get(vertex as usize).map_or([0.0; 3], |v| v.pos)
}

/// The unit normal of polygon `n`, or of every polygon when `n` is −1
/// [soft 0x084599f4 SI_meshComputesPolygonNormal]. Class: model.
///
/// Newell's sums over each contour (holes included, each closed on itself), with `a` the
/// previous corner and `b` the current one, starting with the contour's last corner as `a`:
///
/// ```text
///   x += (a.y − b.y) × (a.z + b.z)
///   y += (a.x + b.x) × (a.z − b.z)
///   z += (a.y + b.y) × (a.x − b.x)
/// ```
///
/// The three sums are **doubles in memory**: each term is formed at 64 bits from the float
/// coordinates, added to the sum and the sum rounded to double. The length
/// √((x² + y²) + z²) stays in the register; if it is above 1e-6 the normal is each sum
/// divided by it, rounded to float, otherwise the normal is left at zero.
pub fn computes_polygon_normal(mesh: &mut Mesh, n: i32) {
    computes_polygon_normal_with(mesh, n, Eval::X87);
}

/// [`computes_polygon_normal`] under another evaluation (see [`Eval`]).
pub fn computes_polygon_normal_with(mesh: &mut Mesh, n: i32, eval: Eval) {
    let (first, last) = if n == -1 { (0, mesh.nb_polygons() - 1) } else { (n, n) };
    let mut p = first;
    while p <= last {
        // (the original does not test `n` against the polygon count)
        let pi = p as usize;
        p += 1;
        let (mut sx, mut sy, mut sz) = (0.0f64, 0.0f64, 0.0f64);
        {
            let nodes = &mesh.polygons[pi].nodes;
            let nb = nodes.len();
            let mut start = 0usize;
            for i in 0..=nb {
                if i == nb || nodes[i].vertex == -1 {
                    let end = if i == 0 { 0 } else { i - 1 };
                    if start <= end && end < nb {
                        let mut a = position(mesh, nodes[end].vertex);
                        for node in &nodes[start..=end] {
                            let b = position(mesh, node.vertex);
                            match eval {
                                Eval::X87 => {
                                    let tx = (x(a[1]) - b[1]) * (x(a[2]) + b[2]);
                                    let tz = (x(a[1]) + b[1]) * (x(a[0]) - b[0]);
                                    let ty = (x(a[0]) + b[0]) * (x(a[2]) - b[2]);
                                    sx = (tx + sx).to_f64();
                                    sz = (tz + sz).to_f64();
                                    sy = (ty + sy).to_f64();
                                }
                                // (float terms do not reproduce the files of the other builds: the
                                // coordinates must be doubles in the source, like the sums)
                                Eval::Single | Eval::Double => {
                                    let d = |v: f32| v as f64;
                                    sx += (d(a[1]) - d(b[1])) * (d(a[2]) + d(b[2]));
                                    sz += (d(a[1]) + d(b[1])) * (d(a[0]) - d(b[0]));
                                    sy += (d(a[0]) + d(b[0])) * (d(a[2]) - d(b[2]));
                                }
                            }
                            a = b;
                        }
                    }
                    start = i + 1;
                }
            }
        }
        let normal = &mut mesh.polygons[pi].normal;
        *normal = [0.0; 3];
        match eval {
            Eval::X87 => {
                let len = ((x(sx) * sx + x(sy) * sy) + x(sz) * sz).sqrt();
                if len.gt(x(POLYGON_NORMAL_MIN)) {
                    *normal = [(x(sx) / len).to_f32(), (x(sy) / len).to_f32(), (x(sz) / len).to_f32()];
                }
            }
            Eval::Single | Eval::Double => {
                let len = ((sx * sx + sy * sy) + sz * sz).sqrt();
                if len > POLYGON_NORMAL_MIN {
                    *normal = [(sx / len) as f32, (sy / len) as f32, (sz / len) as f32];
                }
            }
        }
    }
}

/// Marks edge `n`, or every edge when `n` is −1, as border and as discontinuity
/// [soft 0x0845a6e4 SI_meshComputeHardEdge]. Class: model (through `SI_vectorCosAngle`).
///
/// Bit 0x20 is set exactly when the edge lacks a polygon on one side. An edge with neither
/// 0x20 nor 0x08 then has 0x10 cleared and, if the mesh has the automatic discontinuity
/// (flag bits 0x4 or 0x8), set again when the cosine of the angle between its two polygons'
/// normals is not above the mesh's `cos_angle`. A border edge and an edge with 0x08 keep
/// whatever 0x10 they had. Nothing happens for `n` below −1, 0, or above the edge count.
pub fn compute_hard_edge(mesh: &mut Mesh, n: i32) {
    compute_hard_edge_with(mesh, n, Eval::X87);
}

/// `SI_vectorCosAngle` under another evaluation (see [`Eval`]); `si-math` has the Linux
/// build's.
fn cos_angle_with(a: [f32; 3], b: [f32; 3], eval: Eval) -> f32 {
    match eval {
        Eval::X87 => vector_cos_angle(&Vector::from_array(a), &Vector::from_array(b)),
        Eval::Single => {
            let la = (((a[1] * a[1] + a[0] * a[0]) + a[2] * a[2]) as f64).sqrt();
            let lb = (((b[1] * b[1] + b[0] * b[0]) + b[2] * b[2]) as f64).sqrt();
            let l = la * lb;
            if l.abs() < 1e-4 {
                return 1.0;
            }
            ((a[2] * b[2] + (b[1] * a[1] + a[0] * b[0])) as f64 / l) as f32
        }
        Eval::Double => {
            let d = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
            let (a, b) = (d(a), d(b));
            let la = ((a[1] * a[1] + a[0] * a[0]) + a[2] * a[2]).sqrt();
            let lb = ((b[1] * b[1] + b[0] * b[0]) + b[2] * b[2]).sqrt();
            let l = la * lb;
            if l.abs() < 1e-4 {
                return 1.0;
            }
            ((a[2] * b[2] + (b[1] * a[1] + a[0] * b[0])) / l) as f32
        }
    }
}

/// [`compute_hard_edge`] under another evaluation (see [`Eval`]).
pub fn compute_hard_edge_with(mesh: &mut Mesh, n: i32, eval: Eval) {
    let count = mesh.nb_edges();
    if n < -1 || n == 0 || n > count {
        return;
    }
    let (first, last) = if n == -1 { (1, count) } else { (n, n) };
    let auto = mesh.flags & 0xc != 0;
    for e in first..=last {
        let edge = &mut mesh.edges[e as usize];
        edge.flag &= !EDGE_BORDER;
        if edge.p0 == -1 || edge.p1 == -1 {
            edge.flag |= EDGE_BORDER;
        }
        if edge.flag & (EDGE_BORDER | EDGE_USER_HARD) == 0 {
            edge.flag &= !EDGE_AUTO_HARD;
            if auto {
                let (p0, p1) = (edge.p0 as usize, edge.p1 as usize);
                // the returned float against the float cos_angle: below or equal
                if cos_angle_with(mesh.polygons[p0].normal, mesh.polygons[p1].normal, eval) <= mesh.cos_angle {
                    mesh.edges[e as usize].flag |= EDGE_AUTO_HARD;
                }
            }
        }
    }
}

/// A summed normal brought to length 1 as the corner-normal pass does it: the length
/// √((y² + x²) + z²) is rounded to float; if its size is above 0.0001 each component is
/// divided by it (one operation on floats each). Otherwise the sum is left as it is.
fn normalize_sum(v: &mut [f32; 3], eval: Eval) {
    let len = match eval {
        Eval::X87 => ((x(v[1]) * v[1] + x(v[0]) * v[0]) + x(v[2]) * v[2]).sqrt().to_f32(),
        Eval::Single => (((v[1] * v[1] + v[0] * v[0]) + v[2] * v[2]) as f64).sqrt() as f32,
        Eval::Double => {
            let d = [v[0] as f64, v[1] as f64, v[2] as f64];
            ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt() as f32
        }
    };
    if x(len).abs().gt(x(VERTEX_NORMAL_MIN)) {
        *v = [v[0] / len, v[1] / len, v[2] / len];
    }
}

fn edge_flag(mesh: &Mesh, e: i32) -> i16 {
    mesh.edges[e.unsigned_abs() as usize].flag
}

/// Writes `normal` into the first corner of `polygon` that is at vertex `v`.
fn set_corner_normal(mesh: &mut Mesh, polygon: i32, v: i32, normal: [f32; 3]) {
    if let Some(node) = mesh.polygons[polygon as usize].nodes.iter_mut().find(|n| n.vertex == v) {
        node.normal = normal;
    }
}

/// The corner normals at vertex `n`; at every vertex when `n` is negative, and when it is
/// −2 only at the vertices whose flag has bit 15 set
/// [soft 0x08459e8c SI_meshComputesVertexNormal]. Class: single (float sums, one operation
/// per store) with a model-class length.
///
/// First the mesh flags: 0x800 adds 0x8 (so a new mesh gets the automatic discontinuity on
/// its first pass, and the bit stays); without 0x800 nothing at all is computed unless 0x2
/// or 0x8 is set.
///
/// **Faceted** (flag 0x2): every corner at the vertex gets its polygon's normal.
///
/// **Otherwise** the star is cut into groups: a group ends before an entry whose edge does
/// not lead back to the previous entry's polygon, before an entry with vertex −1, and at
/// the end. Within a group:
///
/// - if no edge of the group has 0x10 or 0x08 and every entry has a polygon (a closed,
///   smooth fan), all polygon normals are summed in star order and every corner gets the
///   normalised sum;
/// - otherwise each polygon is treated on its own: from its entry the group is walked
///   backwards (cyclically) until an edge with 0x10, 0x08 or 0x20, or until the walk has
///   come round; from there the polygon normals are summed forwards until the next such
///   edge. The corner gets that normalised sum.
///
/// A polygon whose flag has 0x400 keeps its corner normals. A corner is the first corner of
/// the polygon at that vertex; if a polygon has no corner there (an inconsistent star), the
/// smooth case writes nothing and the other case writes to the corner index left over from
/// the polygon before (uninitialised stack for the first): here it writes nothing.
pub fn computes_vertex_normal(mesh: &mut Mesh, n: i32) {
    computes_vertex_normal_with(mesh, n, Eval::X87);
}

/// [`computes_vertex_normal`] under another evaluation (see [`Eval`]).
pub fn computes_vertex_normal_with(mesh: &mut Mesh, n: i32, eval: Eval) {
    if mesh.flags & 0x800 != 0 {
        mesh.flags |= 8;
    } else if mesh.flags & 0xa == 0 {
        return;
    }
    let (first, last) = if n < 0 { (0, mesh.nb_vertices() - 1) } else { (n, n) };
    for v in first..=last {
        let vi = v as usize;
        if vi >= mesh.vertices.len() {
            break; // the original does not test `n` against the vertex count
        }
        if n == -2 && mesh.vertices[vi].flag >= 0 {
            continue;
        }
        let count = mesh.vertices[vi].star.len() as i32;
        if count == 0 {
            continue;
        }
        if mesh.flags & 2 != 0 {
            for k in 0..count as usize {
                let p = mesh.vertices[vi].star[k].polygon;
                if p != -1 {
                    let normal = mesh.polygons[p as usize].normal;
                    set_corner_normal(mesh, p, v, normal);
                }
            }
            continue;
        }
        let star = mesh.vertices[vi].star.clone();
        let s = |k: i32| star[k as usize];
        let mut split = false;
        let mut start = 0i32;
        let mut i = 0i32;
        while i <= count {
            if i > start && i < count {
                let e = s(i).edge;
                let across = if e > 0 { mesh.edges[e as usize].p1 } else { mesh.edges[e.unsigned_abs() as usize].p0 };
                if across == -1 || across != s(i - 1).polygon {
                    split = true;
                }
            }
            if !split && i < count && s(i).vertex != -1 {
                i += 1;
                continue;
            }
            let end = i - 1;
            // a closed smooth fan?
            let mut sum = [0.0f32; 3];
            let mut smooth = true;
            for k in start..=end {
                if edge_flag(mesh, s(k).edge) & (EDGE_AUTO_HARD | EDGE_USER_HARD) != 0 || s(k).polygon == -1 {
                    smooth = false;
                    break;
                }
                let pn = mesh.polygons[s(k).polygon as usize].normal;
                sum = [pn[0] + sum[0], pn[1] + sum[1], pn[2] + sum[2]];
            }
            if smooth {
                normalize_sum(&mut sum, eval);
                for k in start..=end {
                    let p = s(k).polygon;
                    if p != -1 && mesh.polygons[p as usize].flag & 0x400 == 0 {
                        set_corner_normal(mesh, p, v, sum);
                    }
                }
            } else {
                let hard = EDGE_AUTO_HARD | EDGE_USER_HARD | EDGE_BORDER;
                for k in start..=end {
                    let p = s(k).polygon;
                    if p == -1 || mesh.polygons[p as usize].flag & 0x400 != 0 {
                        continue;
                    }
                    let stop = if k == end { start } else { k + 1 };
                    let mut at = k;
                    while at != stop {
                        if edge_flag(mesh, s(at).edge) & hard != 0 {
                            break;
                        }
                        at = if at == start { end } else { at - 1 };
                    }
                    let from = at;
                    let mut acc = match s(at).polygon {
                        -1 => [0.0f32; 3],
                        q => mesh.polygons[q as usize].normal,
                    };
                    loop {
                        at = if at == end { start } else { at + 1 };
                        if at == from || edge_flag(mesh, s(at).edge) & hard != 0 {
                            break;
                        }
                        let q = s(at).polygon;
                        if q != -1 {
                            let pn = mesh.polygons[q as usize].normal;
                            acc = [acc[0] + pn[0], acc[1] + pn[1], acc[2] + pn[2]];
                        }
                    }
                    normalize_sum(&mut acc, eval);
                    set_corner_normal(mesh, p, v, acc);
                }
            }
            if i < count {
                start = if s(i).vertex == -1 { i + 1 } else { i };
            }
            split = false;
            i += 1;
        }
    }
}

/// Builds the wire-frame polylines [soft 0x0845a7c8 SI_meshOptimizeDisplay]. Class: integer.
///
/// Every edge first gets bit 0x8000 of its flag cleared, then set again if all the polygons
/// it has are hidden (polygon flag 0x800); the others are counted, and three integers per
/// counted edge are allocated. Then, vertex by vertex and star entry by star entry, each
/// edge not yet marked starts a line: the vertex, the neighbour, and from each vertex
/// reached the neighbour of the first star entry whose edge is not yet marked, until a
/// vertex has none left (or the neighbour is −1); −1 ends the line. Each edge is marked as
/// it is taken. `nb_display` ends as the number of integers written minus one. After the call every
/// edge that a star refers to has 0x8000 set.
pub fn optimize_display(mesh: &mut Mesh) {
    mesh.display.clear();
    mesh.nb_display = 0;
    let hidden = |mesh: &Mesh, p: i32| mesh.polygons[p as usize].flag & 0x800 != 0;
    for e in 1..=mesh.nb_edges() as usize {
        let Edge { p0, p1, .. } = mesh.edges[e];
        let mut flag = mesh.edges[e].flag as u16 & !EDGE_DISPLAYED;
        let all_hidden = match (p0 != -1, p1 != -1) {
            (false, false) => false,
            (true, false) => hidden(mesh, p0),
            (false, true) => hidden(mesh, p1),
            (true, true) => hidden(mesh, p0) && hidden(mesh, p1),
        };
        if all_hidden {
            flag |= EDGE_DISPLAYED;
        }
        mesh.edges[e].flag = flag as i16;
    }
    for v in 0..mesh.vertices.len() {
        let mut k = 0;
        while k < mesh.vertices[v].star.len() {
            let Star { vertex, edge, .. } = mesh.vertices[v].star[k];
            k += 1;
            if edge == 0 {
                continue;
            }
            let e = edge.unsigned_abs() as usize;
            if mesh.edges[e].flag < 0 {
                continue;
            }
            mesh.edges[e].flag = (mesh.edges[e].flag as u16 | EDGE_DISPLAYED) as i16;
            mesh.display.push(v as i32);
            let mut at = vertex;
            while at != -1 {
                mesh.display.push(at);
                let w = at as usize;
                at = -1;
                for u in 0..mesh.vertices[w].star.len() {
                    let s = mesh.vertices[w].star[u];
                    if s.edge != 0 {
                        let e = s.edge.unsigned_abs() as usize;
                        if mesh.edges[e].flag >= 0 {
                            mesh.edges[e].flag = (mesh.edges[e].flag as u16 | EDGE_DISPLAYED) as i16;
                            at = s.vertex;
                            break;
                        }
                    }
                }
            }
            mesh.display.push(-1);
        }
    }
    mesh.nb_display = mesh.display.len() as i32 - 1;
}

/// The rebuild after reading [soft 0x08458cf8 SI_meshUpdateNodes]: every polygon filed
/// ([`insert_ipolygon`]; a failure is remembered and the others still filed), polygon
/// normals, border and discontinuity edges, corner normals if `vertex_normals`, display
/// lines. False if a polygon could not be filed.
///
/// It ends with `SI_meshUpdatePolygonLists` ([`crate::mesh_polygon::update_polygon_lists`]),
/// which rebuilds the two lists of selected polygons at +0x30 and +0x34. The calls to the
/// activity cursor (`SI_showActivity`) have no counterpart.
pub fn update_nodes(mesh: &mut Mesh, vertex_normals: bool, env: &mut MeshEnv) -> bool {
    let mut ok = true;
    for p in 0..mesh.nb_polygons() {
        if !insert_ipolygon(mesh, p, env) {
            ok = false;
        }
    }
    computes_polygon_normal(mesh, -1);
    compute_hard_edge(mesh, -1);
    if vertex_normals {
        computes_vertex_normal(mesh, -1);
    }
    optimize_display(mesh);
    crate::mesh_polygon::update_polygon_lists(mesh, env);
    ok
}

/// The same with corner normals always and no result [soft 0x08458d94 SI_meshUpdate].
pub fn update(mesh: &mut Mesh, env: &mut MeshEnv) {
    update_nodes(mesh, true, env);
}

/// What the unrounded length of [`computes_polygon_normal`] is compared with, for callers
/// that repeat its test.
pub fn polygon_normal_min() -> F80 {
    x(POLYGON_NORMAL_MIN)
}

#[cfg(test)]
mod tests;
