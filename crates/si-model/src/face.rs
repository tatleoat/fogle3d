// SPDX-License-Identifier: MIT
//! The face object: the original's `face.c` (finding 0015).
//!
//! A face (`SI_Face`, 0x88 bytes) is a planar region bounded by curves: an **outline** and
//! any number of **holes**, each a spline (the outline embedded in the record at +4, the
//! holes an array of 0x74-byte splines at +0x7c); or, when the word at +0x84 is 0, an
//! array of NURBS curves at +0x80 of which the first is the outline. The count at +0x78 is
//! the number of holes of a spline face and the number of curves of a NURBS face.
//!
//! The face has no surface of its own. To be drawn, rendered or edited as polygons it is
//! **turned into one polygon with holes** ([`face_convert_to_mesh`]): every curve is
//! sampled into straight pieces, the curves are given consistent directions
//! ([`face_ordinate_points`]: the holes wind against the outline), and the mesh gets all
//! the points as vertices and one polygon whose corner list is the outline, −1, hole, −1,
//! hole… The triangulator ([`crate::triangulate`]) does the rest.
//!
//! Records: the face is `si_hrc::Face` (what a model file stores), as the crate holds
//! every geometry that is not a mesh. What `face.c` calls in the spline and NURBS code
//! (sampling, reversing, point editing, drawing) is behind [`Hooks`], each method naming
//! the routine it stands for; the contour is passed as a number: −1 the outline, k hole k
//! (for a NURBS face: curve k).

use si_hrc::{Face, NurbsCurve, Spline};
use si_math::types::Matrix;
use si_x87::{x, F80};

use crate::mesh::{Mesh, PolyNode, Polygon, Vertex};
use crate::mesh_b::MeshEnv;

/// The length a contour's summed normal must exceed to be normalised, and what the
/// product of a hole's normal with the outline's must exceed for the hole to count as
/// wound the same way: the double 0.0001 (0x0865f758 in `SI_faceOrdinatePoints`,
/// 0x0865f760 in `SI_isFaceVisible`).
const SMALL: f64 = 1e-4;
/// What `SI_faceRandomize` subtracts from each random number: the double 0.5 (0x0865f750).
const HALF: f64 = 0.5;

/// What `face.c` calls in the spline and NURBS source files. Defaults do the plain thing
/// where there is one (a copy, a reversal of the key order) and nothing otherwise.
#[allow(unused_variables)]
pub trait Hooks {
    /// `SI_splineTouch(spline)`.
    fn spline_touch(&mut self, which: i32, s: &mut Spline) {}
    /// `SI_splineDraw(spline)`.
    fn spline_draw(&mut self, which: i32, s: &Spline) {}
    /// `SI_splineDrawModif(spline)`.
    fn spline_draw_modif(&mut self, which: i32, s: &Spline) {}
    /// `SI_nurbsCrvDraw(curve)`.
    fn nurbs_crv_draw(&mut self, which: i32, c: &NurbsCurve) {}
    /// `SI_splineInverse(spline)`: the direction of travel turned round.
    fn spline_inverse(&mut self, which: i32, s: &mut Spline) {}
    /// `SI_nurbsCrvInverse(curve)`; false stops `SI_faceInverse`.
    fn nurbs_crv_inverse(&mut self, which: i32, c: &mut NurbsCurve) -> bool {
        true
    }
    /// `SI_splineDeletePoint(spline, key)`.
    fn spline_delete_point(&mut self, which: i32, s: &mut Spline, key: i32) -> i16 {
        0
    }
    /// `SI_nurbsCrvDeletePoint(curve, point)`.
    fn nurbs_crv_delete_point(&mut self, which: i32, c: &mut NurbsCurve, point: i32) {}
    /// `SI_splineAddPoint(spline, key, position)`.
    fn spline_add_point(&mut self, which: i32, s: &mut Spline, key: i32, pos: [f32; 3]) {}
    /// `SI_splineDeleteSelection(spline)`.
    fn spline_delete_selection(&mut self, which: i32, s: &mut Spline) -> i16 {
        0
    }
    /// `SI_splineConvertToLinear(spline, 1, step, 0.0)`: the spline sampled into a spline
    /// of straight pieces.
    fn spline_convert_to_linear(&mut self, which: i32, s: &Spline, step: i16) -> Spline {
        s.clone()
    }
    /// `SI_splineCleanup(spline)`.
    fn spline_cleanup(&mut self, which: i32, s: &mut Spline) {}
    /// `SI_nurbsCrvConvertToLinear(curve)`.
    fn nurbs_crv_convert_to_linear(&mut self, which: i32, c: &NurbsCurve) -> NurbsCurve {
        c.clone()
    }
    /// `SI_splineTransform(spline, matrix)`.
    fn spline_transform(&mut self, which: i32, s: &mut Spline, m: &Matrix) {}
    /// `SI_nurbsCrvApplyMatrix(curve, matrix, 0)`.
    fn nurbs_crv_apply_matrix(&mut self, which: i32, c: &mut NurbsCurve, m: &Matrix) {}
    /// `SI_nurbsCrvTransferPointsKnots(from, to)`.
    fn nurbs_crv_transfer_points_knots(&mut self, which: i32, from: &NurbsCurve, to: &mut NurbsCurve) {
        to.points = from.points.clone();
        to.knots = from.knots.clone();
    }
    /// `SI_knotsCrvReparametrization(curve)`.
    fn knots_crv_reparametrization(&mut self, which: i32, c: &mut NurbsCurve) {}
    /// `drand48()` of the C library.
    fn drand48(&mut self) -> f64 {
        0.5
    }
    /// The NURBS branch of `SI_faceAddPoint` as a whole (`SI_SampleKnotVector`,
    /// `SI_nurbsCrvIndexHit`, `SI_nurbsCrvAddPoint`): not ported.
    fn nurbs_face_add_point(&mut self, face: &mut Face, index: &mut i32, pos: [f32; 4]) {}
}

impl Hooks for () {}

fn is_spline(face: &Face) -> bool {
    face.is_spline != 0
}

/// The contour of a spline face by number: −1 the outline, k hole k.
fn contour(face: &Face, which: i32) -> Option<&Spline> {
    if which == -1 {
        Some(&face.exterior)
    } else {
        face.holes.get(which as usize)
    }
}

fn contour_mut(face: &mut Face, which: i32) -> Option<&mut Spline> {
    if which == -1 {
        Some(&mut face.exterior)
    } else {
        face.holes.get_mut(which as usize)
    }
}

/// The numbers of a spline face's contours as the original's loops visit them: −1, then
/// 0 up to the count; nothing at all when the count is negative.
fn contours(face: &Face) -> impl Iterator<Item = i32> {
    let n = face.count as i32;
    (-1..n.max(0)).filter(move |_| n >= 0)
}

fn keys(s: &Spline) -> usize {
    (s.nkeys as usize).min(s.keys.len())
}

/// Frees a face and clears the caller's pointer [soft 0x08434828 SI_faceDispose]: the
/// name; for a NURBS face each curve's points and knots (the curve array itself is **not**
/// freed); for a spline face the outline's keys, each hole's keys and the hole array;
/// then the record.
pub fn face_dispose(face: &mut Option<Box<Face>>) {
    *face = None;
}

/// A copy of a face [soft 0x08434990 SI_faceCopy]: a new face with the name duplicated;
/// the curves of a NURBS face through `SI_nurbsCrvInit`, the outline and holes of a spline
/// face through `SI_splineTransfer`. (The owned form has no name; both copies are clones
/// here.)
pub fn face_copy(face: &Face) -> Face {
    let mut out = Face::default();
    if !is_spline(face) {
        out.nurbs = face.nurbs.iter().take(face.count.max(0) as usize).cloned().collect();
        out.count = face.count;
        out.is_spline = 0;
    } else {
        out.exterior = face.exterior.clone();
        out.count = face.count;
        out.holes = face.holes.iter().take(face.count.max(0) as usize).cloned().collect();
        out.is_spline = 1;
    }
    out
}

/// Marks every spline of a spline face as changed [soft 0x08434ac8 SI_faceTouch]: the
/// outline, then the holes. Nothing for a NURBS face.
pub fn face_touch(face: &mut Face, hooks: &mut dyn Hooks) {
    if is_spline(face) {
        hooks.spline_touch(-1, &mut face.exterior);
        for k in 0..face.count.max(0) as usize {
            if let Some(s) = face.holes.get_mut(k) {
                hooks.spline_touch(k as i32, s);
            }
        }
    }
}

/// Draws a face [soft 0x08434b20 SI_faceDraw]: every curve of a NURBS face, or the
/// outline and then every hole of a spline face.
pub fn face_draw(face: &Face, hooks: &mut dyn Hooks) {
    if !is_spline(face) {
        for (k, c) in face.nurbs.iter().enumerate().take(face.count.max(0) as usize) {
            hooks.nurbs_crv_draw(k as i32, c);
        }
    } else {
        hooks.spline_draw(-1, &face.exterior);
        for (k, s) in face.holes.iter().enumerate().take(face.count.max(0) as usize) {
            hooks.spline_draw(k as i32, s);
        }
    }
}

/// Draws a spline face as being modified [soft 0x08434ba4 SI_faceDrawModif]; nothing for
/// a NURBS face.
pub fn face_draw_modif(face: &Face, hooks: &mut dyn Hooks) {
    if is_spline(face) {
        hooks.spline_draw_modif(-1, &face.exterior);
        for (k, s) in face.holes.iter().enumerate().take(face.count.max(0) as usize) {
            hooks.spline_draw_modif(k as i32, s);
        }
    }
}

/// Turns round every curve of a face [soft 0x08434bf8 SI_faceInverse]: the outline and
/// every hole of a spline face; the curves of a NURBS face one after the other until one
/// cannot be turned.
pub fn face_inverse(face: &mut Face, hooks: &mut dyn Hooks) {
    if !is_spline(face) {
        for k in 0..face.count.max(0) as usize {
            let Some(c) = face.nurbs.get_mut(k) else { break };
            if !hooks.nurbs_crv_inverse(k as i32, c) {
                break;
            }
        }
    } else {
        hooks.spline_inverse(-1, &mut face.exterior);
        for k in 0..face.count.max(0) as usize {
            if let Some(s) = face.holes.get_mut(k) {
                hooks.spline_inverse(k as i32, s);
            }
        }
    }
}

/// The number of points of contour `which` as the index routines count them: a NURBS
/// curve's control points; a spline's keys, times three with `tangents` (each key then
/// counts as previous tangent, position, next tangent).
fn size(face: &Face, which: i32, tangents: bool) -> i32 {
    if !is_spline(face) {
        // (a face without curves makes the original read through a null pointer)
        face.nurbs.get(which as usize).map_or(0, |c| c.npoints)
    } else {
        contour(face, which).map_or(0, |s| s.nkeys as i32 * if tangents { 3 } else { 1 })
    }
}

/// From a point's number over the whole face to (contour, number within it)
/// [soft 0x08434c80 SI_faceGlobalToLocalIndex]. The contours are numbered from −1 (the
/// outline of a spline face) or from 0 (the first curve of a NURBS face).
///
/// The walk passes over a contour while the index is not below its size. Without
/// `tangents` it stops at the last contour, so an index beyond the end stays there with a
/// number past that contour's size; **with `tangents` it may run one contour further**,
/// answering a contour that does not exist (the size it then goes on comparing with is
/// still the last contour's).
pub fn face_global_to_local_index(face: &Face, global: i32, tangents: bool) -> (i32, i32) {
    let count = face.count as i32;
    let first = if is_spline(face) { -1 } else { 0 };
    let bound = if tangents { count } else { count - 1 };
    let (mut which, mut local) = (first, global);
    let mut n = size(face, first, tangents);
    while n <= local && which < bound {
        local -= n;
        which += 1;
        if which < count {
            n = size(face, which, tangents);
        }
    }
    (which, local)
}

/// From (contour, number within it) to a point's number over the whole face
/// [soft 0x08434e68 SI_faceLocalToGlobalIndex]: the sizes of the contours before `which`,
/// plus `local`. The original counts the contours in a 16-bit variable.
pub fn face_local_to_global_index(face: &Face, which: i32, local: i32, tangents: bool) -> i32 {
    let mut total = 0i32;
    let mut k: i16 = if is_spline(face) { -1 } else { 0 };
    while (k as i32) < which {
        // (for a NURBS face the flag makes no difference)
        total = total.wrapping_add(size(face, k as i32, tangents));
        k = k.wrapping_add(1);
    }
    total.wrapping_add(local)
}

/// Moves the points of a face at random [soft 0x08434f6c SI_faceRandomize]: every point,
/// or with `tagged_only` those whose flag has bit 0, `times` times over is displaced by
/// (drand48() − 0.5) × `amount` per axis, three random numbers per round in the order x,
/// y, z. Class: model.
///
/// A key of a spline moves in floats: r32(position + (random − 0.5) × amount), the
/// difference and the product in the register; its tangent points do not move. Each
/// spline is touched afterwards (`SI_splineTouch`). A NURBS control point moves in
/// doubles, r53((random − 0.5) × amount + coordinate) (the weight is left alone); a curve
/// whose parameterisation (+0x24) is 3 or 4 is then reparameterised
/// (`SI_knotsCrvReparametrization`).
pub fn face_randomize(face: &mut Face, amount: [f32; 3], times: i16, tagged_only: bool, hooks: &mut dyn Hooks) {
    if !is_spline(face) {
        for k in 0..face.count.max(0) as usize {
            let Some(c) = face.nurbs.get_mut(k) else { break };
            for p in c.points.iter_mut().take(c.npoints.max(0) as usize) {
                if tagged_only && p.flag & 1 == 0 {
                    continue;
                }
                for _ in 0..times.max(0) {
                    for a in 0..3 {
                        let r = hooks.drand48();
                        p.xyzw[a] = ((x(r) - x(HALF)) * amount[a] + p.xyzw[a]).to_f64();
                    }
                }
            }
            if (c.uniformity.wrapping_sub(3) as u32) < 2 {
                hooks.knots_crv_reparametrization(k as i32, c);
            }
        }
        return;
    }
    for which in contours(face).collect::<Vec<_>>() {
        let Some(s) = contour_mut(face, which) else { break };
        let n = keys(s);
        for key in s.keys.iter_mut().take(n) {
            if tagged_only && key.flag & 1 == 0 {
                continue;
            }
            for _ in 0..times.max(0) {
                for a in 0..3 {
                    let r = hooks.drand48();
                    key.pos[a] = (x(key.pos[a]) + (x(r) - x(HALF)) * amount[a]).to_f32();
                }
            }
        }
        hooks.spline_touch(which, s);
    }
}

/// Removes one point of a face by its number counted with tangents
/// [soft 0x084353a4 SI_faceDeletePoint]. A spline must keep three keys and a NURBS curve
/// as many points as its order: otherwise the status line says so and nothing is removed.
/// The answer is `SI_splineDeletePoint`'s for a spline; in the other cases the original
/// returns whatever its register held (`None`).
pub fn face_delete_point(face: &mut Face, index: i16, env: &mut MeshEnv, hooks: &mut dyn Hooks) -> Option<i16> {
    const REFUSED: &str = "You can't delete another point: delete the hole/exterior instead.";
    let (which, local) = face_global_to_local_index(face, index as i32, true);
    if !is_spline(face) {
        // (a contour beyond the last makes the original work on foreign memory)
        let c = face.nurbs.get_mut(which as usize)?;
        if c.order < c.npoints {
            hooks.nurbs_crv_delete_point(which, c, local);
        } else {
            env.messages.push(REFUSED.to_string());
        }
        return None;
    }
    let s = contour_mut(face, which)?;
    if s.nkeys > 3 {
        return Some(hooks.spline_delete_point(which, s, local / 3));
    }
    env.messages.push(REFUSED.to_string());
    None
}

/// Inserts a point into a face: the spline branch of `SI_faceAddPoint` (0x0843548c). The
/// index (counted in keys) is brought to its contour and `SI_splineAddPoint` called with
/// the first three coordinates. For a NURBS face the original samples the knot vectors to
/// find the curve and the place (`SI_SampleKnotVector`, `SI_nurbsCrvIndexHit`), inserts
/// with `SI_nurbsCrvAddPoint`, rewrites the index, and on failure shows a message and
/// sleeps a second; that branch is **not ported**: it is one hook here, and the routine
/// is not counted as reproduced.
pub fn face_add_point(face: &mut Face, index: &mut i32, pos: [f32; 4], hooks: &mut dyn Hooks) {
    if !is_spline(face) {
        hooks.nurbs_face_add_point(face, index, pos);
        return;
    }
    let (which, local) = face_global_to_local_index(face, *index, false);
    if let Some(s) = contour_mut(face, which) {
        hooks.spline_add_point(which, s, local, [pos[0], pos[1], pos[2]]);
    }
}

/// Removes the selected keys of every spline of a spline face
/// [soft 0x08435628 SI_faceDeleteSelection]: `SI_splineDeleteSelection` on the outline and
/// each hole; the answer is 1 or-ed with every result (so never 0). 1 for a NURBS face.
pub fn face_delete_selection(face: &mut Face, hooks: &mut dyn Hooks) -> i16 {
    let mut r = 1i16;
    if is_spline(face) {
        for which in contours(face).collect::<Vec<_>>() {
            if let Some(s) = contour_mut(face, which) {
                r |= hooks.spline_delete_selection(which, s);
            }
        }
    }
    r
}

/// The number of triangles a face makes [soft 0x08435690 SI_faceNbTriangles]: the number
/// of points its contours are sampled into, minus 2, plus 2 per hole. A linear spline
/// counts its keys, any other spline keys × step; a NURBS curve points × step (and a NURBS
/// face of n curves has n − 1 holes).
pub fn face_nb_triangles(face: &Face) -> i32 {
    let count = face.count as i32;
    let mut total = 0i32;
    if !is_spline(face) {
        for c in face.nurbs.iter().take(count.max(0) as usize) {
            total = total.wrapping_add(c.npoints.wrapping_mul(c.step));
        }
        return total - 4 + count * 2;
    }
    for which in contours(face) {
        if let Some(s) = contour(face, which) {
            let n = s.nkeys as u32;
            total = total.wrapping_add(if s.kind == 0 { n } else { n * s.step as u16 as u32 } as i32);
        }
    }
    total - 2 + count * 2
}

/// A face of straight pieces from a face [soft 0x08435750 SI_faceConvertToLinear].
///
/// Spline face: the outline and every hole that has keys through
/// `SI_splineConvertToLinear(spline, 1, step, 0.0)`; a spline that was not linear already
/// is cleaned up (`SI_splineCleanup`) when `cleanup` is set. **A face whose outline has no
/// key gives nothing** (and the new face is leaked). A hole without keys stays an empty
/// spline. NURBS face: every curve through `SI_nurbsCrvConvertToLinear`. Either way the
/// result's curves are then given their directions ([`face_ordinate_points`] without the
/// "face up" rule).
pub fn face_convert_to_linear(face: &Face, step: i16, cleanup: bool, hooks: &mut dyn Hooks) -> Option<Face> {
    let mut out = Face::default();
    let count = face.count.max(0) as usize;
    if !is_spline(face) {
        for (k, c) in face.nurbs.iter().enumerate().take(count) {
            out.nurbs.push(hooks.nurbs_crv_convert_to_linear(k as i32, c));
        }
        out.count = face.count;
        out.is_spline = 0;
        face_ordinate_points(&mut out, false, hooks);
        return Some(out);
    }
    out.holes = vec![Spline::default(); count];
    for which in contours(face) {
        let Some(s) = contour(face, which) else { break };
        if s.nkeys == 0 {
            if which == -1 {
                return None;
            }
            continue;
        }
        let mut lin = hooks.spline_convert_to_linear(which, s, step);
        if s.kind != 0 && cleanup {
            hooks.spline_cleanup(which, &mut lin);
        }
        *contour_mut(&mut out, which).unwrap() = lin;
    }
    out.count = face.count;
    face_ordinate_points(&mut out, false, hooks);
    Some(out)
}

/// A face as one polygon with holes [soft 0x08435960 SI_faceConvertToMesh].
///
/// The face is made linear ([`face_convert_to_linear`] with step −1; nothing comes back
/// when that fails, and the new mesh is leaked). The mesh gets one vertex per point of
/// every contour, in contour order (a NURBS point as r32(coordinate × (1/w)), the
/// reciprocal kept in the register), and **one polygon** whose corners are the vertices in order with a −1
/// record between two contours. Then `SI_meshUpdate` and `SI_meshInitVertexColors`.
///
/// The corner count is 16 bits: (points + holes) for a spline face, (points + curves − 1)
/// for a NURBS face.
///
/// **The mesh is allocated first** (`SI_meshAllocate`, 0x08458c20, is the routine's first
/// call, before the face is made linear), and that allocator asks the C library for the
/// cosine of its angle constant (the double at 0x0865faf8, 60 degrees to nine digits) and
/// stores it as a float at +0x10. The call is made here, at that place and through
/// `si_x87::trig`, so that it is a library call of the port as it is one of the
/// original, also when the conversion then fails.
pub fn face_convert_to_mesh(face: &Face, cleanup: bool, env: &mut MeshEnv, hooks: &mut dyn Hooks) -> Option<Mesh> {
    let cos_angle = si_x87::trig::cos(f64::from_bits(0x3ff0_c152_37db_38a0)).to_f32();
    let lin = face_convert_to_linear(face, -1, cleanup, hooks)?;
    let mut mesh = Mesh::default();
    mesh.cos_angle = cos_angle;
    let mut corners: Vec<i32> = Vec::new();
    let vertex = |pos: [f32; 3]| Vertex { pos, star: Vec::new(), flag: 0 };
    if !is_spline(face) {
        for (k, c) in lin.nurbs.iter().enumerate().take(lin.count.max(0) as usize) {
            if k != 0 {
                corners.push(-1);
            }
            for p in c.points.iter().take(c.npoints.max(0) as usize) {
                let inv = x(1.0f32) / x(p.xyzw[3]);
                let co = |a: usize| (x(p.xyzw[a]) * inv).to_f32();
                corners.push(mesh.vertices.len() as i32);
                mesh.vertices.push(vertex([co(0), co(1), co(2)]));
            }
        }
    } else {
        for which in contours(&lin) {
            if which != -1 {
                corners.push(-1);
            }
            let Some(s) = contour(&lin, which) else { continue };
            for key in s.keys.iter().take(keys(s)) {
                corners.push(mesh.vertices.len() as i32);
                mesh.vertices.push(vertex(key.pos));
            }
        }
    }
    // the count is a 16-bit word
    corners.truncate(corners.len() as u16 as usize);
    let nodes = corners.iter().map(|&vertex| PolyNode { vertex, ..PolyNode::default() }).collect();
    mesh.polygons.push(Polygon { normal: [0.0; 3], nodes, material: 0, flag: 0 });
    crate::mesh_b::update(&mut mesh, env);
    crate::mesh_util::mesh_init_vertex_colors(&mut mesh);
    Some(mesh)
}

/// The normal of one closed run of points by Newell's sums, as the two routines below
/// compute it. `linear` gives one point per step; a Bézier spline (kind 1) gives three per
/// key (previous tangent, position, next tangent), whose three terms are summed in the
/// register and rounded to float once per key. The result is divided by its length
/// (r32(√((y² + x²) + z²))) when that exceeds 0.0001.
struct Newell {
    sum: [f32; 3],
}

impl Newell {
    fn term(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
        [
            (x(a[1]) - b[1]) * (x(a[2]) + b[2]),
            (x(a[2]) - b[2]) * (x(b[0]) + a[0]),
            (x(a[0]) - b[0]) * (x(b[1]) + a[1]),
        ]
    }

    /// One step from `a` to `b`, rounded at once.
    fn step(&mut self, a: [f32; 3], b: [f32; 3]) {
        let t = Self::term(a, b);
        for k in 0..3 {
            self.sum[k] = (t[k] + self.sum[k]).to_f32();
        }
    }

    /// Three steps a → p0 → p1 → p2, rounded once at the end.
    fn steps3(&mut self, a: [f32; 3], p: [[f32; 3]; 3]) {
        let (t0, t1, t2) = (Self::term(a, p[0]), Self::term(p[0], p[1]), Self::term(p[1], p[2]));
        for k in 0..3 {
            self.sum[k] = (t2[k] + ((t0[k] + self.sum[k]) + t1[k])).to_f32();
        }
    }

    fn normal(mut self) -> [f32; 3] {
        let [sx, sy, sz] = self.sum;
        let len = ((x(sy) * sy + x(sx) * sx) + x(sz) * sz).sqrt().to_f32();
        if x(SMALL).lt(x(len).abs()) {
            self.sum = [(x(sx) / len).to_f32(), (x(sy) / len).to_f32(), (x(sz) / len).to_f32()];
        }
        self.sum
    }
}

/// The normal of a spline's key polygon; `None` for a spline without keys.
pub(crate) fn spline_normal(s: &Spline) -> Option<[f32; 3]> {
    let n = keys(s);
    if s.nkeys == 0 || n == 0 {
        return None;
    }
    let mut w = Newell { sum: [0.0; 3] };
    if s.kind == 1 {
        let mut a = s.keys[n - 1].next;
        for key in &s.keys[..n] {
            w.steps3(a, [key.prev, key.pos, key.next]);
            a = key.next;
        }
    } else {
        let mut a = s.keys[n - 1].pos;
        for key in &s.keys[..n] {
            w.step(a, key.pos);
            a = key.pos;
        }
    }
    Some(w.normal())
}

/// The normal of a NURBS curve's control polygon, each point as r32(coordinate × (1/w))
/// with the reciprocal in the register; `None` for a curve without points.
pub(crate) fn curve_normal(c: &NurbsCurve) -> Option<[f32; 3]> {
    let n = (c.npoints.max(0) as usize).min(c.points.len());
    if c.npoints <= 0 || n == 0 {
        return None;
    }
    let point = |i: usize| {
        let p = &c.points[i].xyzw;
        let inv = x(1.0f32) / x(p[3]);
        [(x(p[0]) * inv).to_f32(), (x(p[1]) * inv).to_f32(), (inv * p[2]).to_f32()]
    };
    let mut w = Newell { sum: [0.0; 3] };
    let mut a = point(n - 1);
    for i in 0..n {
        let b = point(i);
        w.step(a, b);
        a = b;
    }
    Some(w.normal())
}

pub(crate) fn dot_above(reference: [f32; 3], n: [f32; 3]) -> bool {
    ((x(reference[0]) * n[0] + x(reference[1]) * n[1]) + x(reference[2]) * n[2]).gt(x(SMALL))
}

/// Gives the curves of a face consistent directions
/// [soft 0x08435d3c SI_faceOrdinatePoints]. Class: model.
///
/// The normal of every contour is taken from its points by Newell's sums (see
/// [`spline_normal`], [`curve_normal`]). The outline's normal is the reference; with
/// `face_up`, an outline whose normal has a negative z is turned round and the reference
/// negated. **Every hole whose normal has a product with the reference above 0.0001 is
/// turned round**: holes wind against the outline. A contour without points is passed
/// over.
///
/// When the outline has no points the original compares the holes with a reference it
/// never set (zeros here, counted).
pub fn face_ordinate_points(face: &mut Face, face_up: bool, hooks: &mut dyn Hooks) {
    let mut reference: Option<[f32; 3]> = None;
    let rule = |first: bool, n: [f32; 3], reference: &mut Option<[f32; 3]>| -> bool {
        if first {
            *reference = Some(n);
            if face_up && x(n[2]).lt(x(0.0f32)) {
                *reference = Some([(-x(n[0])).to_f32(), (-x(n[1])).to_f32(), (-x(n[2])).to_f32()]);
                return true;
            }
            return false;
        }
        let r = reference.unwrap_or_else(|| {
            crate::env::stale::note();
            [0.0; 3]
        });
        dot_above(r, n)
    };
    if !is_spline(face) {
        for k in 0..face.count.max(0) as usize {
            let Some(c) = face.nurbs.get_mut(k) else { break };
            if let Some(n) = curve_normal(c) {
                if rule(k == 0, n, &mut reference) {
                    hooks.nurbs_crv_inverse(k as i32, c);
                }
            }
        }
        return;
    }
    for which in contours(face).collect::<Vec<_>>() {
        let Some(s) = contour_mut(face, which) else { break };
        if let Some(n) = spline_normal(s) {
            if rule(which == -1, n, &mut reference) {
                hooks.spline_inverse(which, s);
            }
        }
    }
}

/// Whether the curves of a spline face already have the directions
/// [`face_ordinate_points`] would give them [soft 0x08436364 SI_isFaceVisible]: false when
/// (with `face_up`) the outline's normal has a negative z, or a hole's normal has a
/// product with the outline's above 0.0001. Always true for a NURBS face. Class: model.
pub fn is_face_visible(face: &Face, face_up: bool) -> bool {
    if !is_spline(face) {
        return true;
    }
    let mut reference: Option<[f32; 3]> = None;
    for which in contours(face) {
        let Some(s) = contour(face, which) else { break };
        let Some(n) = spline_normal(s) else { continue };
        if which == -1 {
            reference = Some(n);
            if face_up && x(n[2]).lt(x(0.0f32)) {
                return false;
            }
        } else {
            let r = reference.unwrap_or_else(|| {
                crate::env::stale::note();
                [0.0; 3]
            });
            if dot_above(r, n) {
                return false;
            }
        }
    }
    true
}

/// Copies the points of one face onto another of the same build
/// [soft 0x0843672c SI_faceTransferPoints]: for a spline face the three vectors of every
/// key (previous tangent, position, next tangent; the count is the source's), contour by
/// contour; for a NURBS face `SI_nurbsCrvTransferPointsKnots` per curve.
///
/// Where the destination has fewer keys or contours the original writes past its arrays;
/// here the surplus is dropped.
pub fn face_transfer_points(src: &Face, dst: &mut Face, hooks: &mut dyn Hooks) {
    if !is_spline(src) {
        for k in 0..src.count.max(0) as usize {
            let (Some(a), Some(b)) = (src.nurbs.get(k), dst.nurbs.get_mut(k)) else { break };
            hooks.nurbs_crv_transfer_points_knots(k as i32, a, b);
        }
        return;
    }
    for which in contours(src) {
        let (Some(a), Some(b)) = (contour(src, which), contour_mut(dst, which)) else { break };
        for (ka, kb) in a.keys.iter().take(keys(a)).zip(b.keys.iter_mut()) {
            kb.prev = ka.prev;
            kb.pos = ka.pos;
            kb.next = ka.next;
        }
    }
}

/// The text `SI_faceDump` writes [soft 0x08436838 SI_faceDump]: a heading, the title, the
/// name, and for a NURBS face `NURBS FACE` before each curve's own dump
/// (`SI_nurbsCrvDump`, passed in as `curve_dump`). A spline face shows nothing more.
pub fn face_dump(face: &Face, title: &str, name: Option<&str>, mut curve_dump: impl FnMut(usize, &NurbsCurve) -> String) -> String {
    let mut out = format!("DUMP FACE\n---------\n{title}\nname : \"{}\"\n", name.unwrap_or("(null)"));
    if !is_spline(face) {
        for (k, c) in face.nurbs.iter().enumerate().take(face.count.max(0) as usize) {
            out.push_str("NURBS FACE\n");
            out.push_str(&curve_dump(k, c));
        }
    }
    out
}

/// From a segment's number to the number of the point it starts at, counted with tangents
/// [soft 0x084368fc SI_faceSegmentToPoint]: for a spline face three per key (the walk
/// over the contours is that of [`face_global_to_local_index`] with tangents, in keys);
/// for a NURBS face the points passed plus the rest plus 1.
pub fn face_segment_to_point(face: &Face, segment: i32) -> i32 {
    let count = face.count as i32;
    let mut passed = 0i32;
    let mut rest = segment;
    if !is_spline(face) {
        let mut which = 0;
        let mut n = size(face, 0, false);
        while n <= rest && which < count {
            passed += n;
            rest -= n;
            which += 1;
            if which < count {
                n = size(face, which, false);
            }
        }
        return passed + rest + 1;
    }
    let mut which = -1;
    let mut n = face.exterior.nkeys as i32;
    while n <= rest && which < count {
        passed += n * 3;
        rest -= n;
        which += 1;
        if which < count {
            n = size(face, which, false);
        }
    }
    passed + rest * 3
}

/// Removes one hole [soft 0x084369c0 SI_faceDeleteHole]. `index` counts the contours from
/// 0 with the outline first, and 0 does nothing: the outline cannot be removed. A spline
/// face loses hole `index` − 1; a NURBS face loses curve `index`, unless it has fewer than
/// two curves. The count goes down by one.
pub fn face_delete_hole(face: &mut Face, index: i32) {
    if index == 0 {
        return;
    }
    if !is_spline(face) {
        if face.count < 2 {
            return;
        }
        // (an index beyond the curves copies them all and still lowers the count)
        if (index as usize) < face.nurbs.len() && index > 0 {
            face.nurbs.remove(index as usize);
        }
    } else if index >= 1 && (index as usize) <= face.holes.len() {
        face.holes.remove(index as usize - 1);
    }
    face.count -= 1;
}

/// Sends every curve of a face through a matrix [soft 0x08436c88 SI_faceTransform]:
/// `SI_splineTransform` for the outline and holes, `SI_nurbsCrvApplyMatrix(curve, m, 0)`
/// for NURBS curves.
pub fn face_transform(face: &mut Face, m: &Matrix, hooks: &mut dyn Hooks) {
    if !is_spline(face) {
        for k in 0..face.count.max(0) as usize {
            let Some(c) = face.nurbs.get_mut(k) else { break };
            hooks.nurbs_crv_apply_matrix(k as i32, c, m);
        }
        return;
    }
    for which in contours(face).collect::<Vec<_>>() {
        if let Some(s) = contour_mut(face, which) {
            hooks.spline_transform(which, s, m);
        }
    }
}

/// The points of a face as (contour, number) pairs [soft 0x08436d28 SI_faceGetIndexes]:
/// all of them, or with `tagged_only` those whose flag has bit 0 (a key's flag, a control
/// point's). The contour is −1 for the outline of a spline face. The original fills an
/// array of the caller's and answers the number of pairs; with its `count_only` argument
/// it only counts.
///
/// This is the call that lets the routine allocate the array (a null pointer handed in):
/// it counts first and asks `SI_malloc` for eight bytes a pair. **For no pairs that is a
/// request for no bytes, which `SI_malloc` refuses, and the routine answers −1**: `None`
/// here.
pub fn face_get_indexes(face: &Face, tagged_only: bool) -> Option<Vec<(i32, i32)>> {
    let out = face_list_indexes(face, tagged_only);
    (!out.is_empty()).then_some(out)
}

/// The listing of [`face_get_indexes`] into an array the caller has.
pub fn face_list_indexes(face: &Face, tagged_only: bool) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    if !is_spline(face) {
        for (k, c) in face.nurbs.iter().enumerate().take(face.count.max(0) as usize) {
            for (i, p) in c.points.iter().enumerate().take(c.npoints.max(0) as usize) {
                if !tagged_only || p.flag & 1 != 0 {
                    out.push((k as i32, i as i32));
                }
            }
        }
        return out;
    }
    for which in contours(face) {
        let Some(s) = contour(face, which) else { break };
        for (i, key) in s.keys.iter().enumerate().take(keys(s)) {
            if !tagged_only || key.flag & 1 != 0 {
                out.push((which, i as i32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
