// SPDX-License-Identifier: MIT
//! The pivot constraint (`constraint.c`), on the shared memory image (finding 0024): an
//! object rolls over a plane on its lowest points. The constraint's state hangs on its
//! update node (`SI_UpdNode` + 0x28): the point the object stands on now, the point it is
//! tipping onto, and the plane each belongs to.
//!
//! The geometry's own routines (`model.c`) and the animation curves are outside this
//! package and reached through [`Outside`]; `SI_modelTraversePnts` calls back into
//! [`check_low`] by its code address ([`crate::callbacks`]).
//!
//! Rounding points are from the instruction listing (finding 0008): r32(…) is a store to
//! float, everything inside is in the x87 register in the order written.

use si_dict::records::UpdNode;
use si_math::algebra::vector_angle;
use si_math::types::Matrix;
use si_mem::{record, Mem, Ptr32};
use si_types::dict::UpdList;
use si_types::geom::Vector;
use si_types::model::Model;
use si_x87::{fmov, x, F80};

use crate::callbacks::CHECK_LOW;
use crate::globals::{SI_FRAME_CURRENT, SI_FRAME_RATE, SI_FRAME_STEP, SI_GLB_ANIM, SI_GRAF_LISTS, SI_PROC_DATA};
use crate::outside::{
    si_fcurve_integer_value, si_find_gl_data, si_model_compute_bounding, si_model_set_translation, si_model_get_pnt, si_model_set_global_translation, si_model_traverse_pnts, si_model_update_son_state, Outside,
};

record! {
    /// What the search for an object's lowest point over a plane carries from point to
    /// point (`CheckLowData`, a local of `checklowestGeometryVertex`).
    pub struct CheckLowData = "CheckLowData", size 0x34 {
        0x00 node: Ptr32<Model>, "the node whose points are being walked";
        0x04 plane: Ptr32<Model>, "the plane object";
        0x08 found: i16, "set to 1 by the first point that qualifies";
        0x0a pad_0a: [u8; 2], "not set";
        0x0c min: Vector, "the plane's bounding box minimum times its scaling";
        0x18 max: Vector, "the plane's bounding box maximum times its scaling";
        0x24 lowest: Ptr32<f32>, "the smallest height over the plane so far";
        0x28 point: Ptr32<Vector>, "the lowest point, in its own node's frame";
        0x2c model: Ptr32<Ptr32<Model>>, "the node the lowest point belongs to";
        0x30 index: Ptr32<i32>, "the lowest point's index in that node";
    }
}

record! {
    /// The state of a pivot constraint, 0x38 bytes from `SI_malloc`, at `SI_UpdNode` +
    /// 0x28. Positions are in the frame of the plane named beside them.
    pub struct PivotData = "(pivot constraint state)", size 0x38 {
        0x00 from: Ptr32<Model>, "the node whose point the object stands on; null before the first transition";
        0x04 from_index: i32, "that point's index; -1 at first";
        0x08 from_pos: Vector, "where that point stands on the plane";
        0x14 from_angle: f32, "degrees the object has to turn about that point (written by si_animPivotTime)";
        0x18 to: Ptr32<Model>, "the node whose point is the lowest now";
        0x1c to_index: i32, "that point's index; -1 at first";
        0x20 to_pos: Vector, "where that point is to stand; not set at first";
        0x2c to_angle: f32, "the turn about the next point, degrees";
        0x30 from_plane: Ptr32<Model>, "the plane `from_pos` is on";
        0x34 to_plane: Ptr32<Model>, "the plane `to_pos` is on; not set at first";
    }
}

/// `p − frame.gtrs`, unrounded: the three differences the routines of this file take
/// through a node's `grot` in several ways.
fn from_origin(m: &Mem, p: Vector, frame: Ptr32<Model>) -> (F80, F80, F80) {
    let o = m[frame].gtrs;
    (x(p.x) - o.x, x(p.y) - o.y, x(p.z) - o.z)
}

/// One point of an object's geometry, as `SI_modelTraversePnts` hands it over
/// [soft 0x081f88a0 checkLow].
///
/// The point is taken into the plane's frame: with d = point − plane.gtrs (unrounded) and
/// R the plane's `grot`, x = (R00 dx + R01 dy) + R02 dz, y = (R10 dx + R11 dy) + R12 dz,
/// z = dz R22 + (dy R21 + dx R20), all unrounded. It qualifies when y is not above the
/// lowest so far and x and z lie within the plane's box (bounds included; a NaN anywhere
/// disqualifies). Then the flag is set, the lowest height becomes r32(y), and the point's
/// place in its own node's frame (the same three sums with that node's position and
/// `grot`, each rounded to float), the node and the index are stored.
pub fn check_low(m: &mut Mem, point: Ptr32<Vector>, index: i32, data: Ptr32<CheckLowData>) {
    let p = m[point];
    let node = m[data].node;
    let (ax, ay, az) = from_origin(m, p, node);
    let r = m[node].grot;
    let own = Vector {
        x: ((x(r[0][0]) * ax + x(r[0][1]) * ay) + x(r[0][2]) * az).to_f32(),
        y: ((x(r[1][0]) * ax + x(r[1][1]) * ay) + x(r[1][2]) * az).to_f32(),
        z: (az * r[2][2] + (ay * r[2][1] + ax * r[2][0])).to_f32(),
    };
    let plane = m[data].plane;
    let (bx, by, bz) = from_origin(m, p, plane);
    let r = m[plane].grot;
    let px = (x(r[0][0]) * bx + x(r[0][1]) * by) + x(r[0][2]) * bz;
    let py = (x(r[1][0]) * bx + x(r[1][1]) * by) + x(r[1][2]) * bz;
    let pz = bz * r[2][2] + (by * r[2][1] + bx * r[2][0]);
    let lowest = m[m[data].lowest];
    if !py.le(x(lowest)) {
        return;
    }
    let (min, max) = (m[data].min, m[data].max);
    if !px.ge(x(min.x)) || !px.le(x(max.x)) || !pz.ge(x(min.z)) || !pz.le(x(max.z)) {
        return;
    }
    m[data].found = 1;
    let lowest = m[data].lowest;
    m[lowest] = py.to_f32();
    let out = m[data].point;
    m[out].x = fmov(own.x);
    let out = m[data].point;
    m[out].y = fmov(own.y);
    let out = m[data].point;
    m[out].z = fmov(own.z);
    let (model, node) = (m[data].model, m[data].node);
    m[model] = node;
    let at = m[data].index;
    m[at] = index;
}

/// Walks a node's points, then its sons' and its brothers'
/// [soft 0x081f8844 checklowestGeometryVertexRec]. Answers the flag of `data`. The top
/// node's own brothers are walked like any other's.
pub fn checklowest_geometry_vertex_rec(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, data: Ptr32<CheckLowData>) -> i32 {
    m[data].node = node;
    si_model_traverse_pnts(o, m, node, CHECK_LOW, data.addr(), 0);
    let son = m[node].son;
    if !son.is_null() {
        checklowest_geometry_vertex_rec(m, o, son, data);
    }
    let brother = m[node].brother;
    if !brother.is_null() {
        checklowest_geometry_vertex_rec(m, o, brother, data);
    }
    m[data].found as i32
}

/// The lowest point of a branch over a plane object
/// [soft 0x081f87b8 checklowestGeometryVertex]. Answers 1 when a point of `node`, its
/// descendants or its brothers lies over the plane's box at or below `*lowest`; then
/// `*lowest`, `*point`, `*index` and `*model` describe it (see [`check_low`]).
///
/// The box is the plane's bounding box times its scaling, each product rounded to float.
pub fn checklowest_geometry_vertex(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, plane: Ptr32<Model>, lowest: Ptr32<f32>, point: Ptr32<Vector>, index: Ptr32<i32>, model: Ptr32<Ptr32<Model>>) -> i32 {
    let data: Ptr32<CheckLowData> = Ptr32::new(m.local(CheckLowData::SIZE));
    m[data].node = node;
    m[data].plane = plane;
    m[data].found = 0;
    m[data].lowest = lowest;
    m[data].point = point;
    m[data].index = index;
    m[data].model = model;
    let (lo, hi, s) = (m[plane].bbox_min, m[plane].bbox_max, m[plane].scal);
    m[data].min = Vector { x: (x(lo.x) * s.x).to_f32(), y: (x(lo.y) * s.y).to_f32(), z: (x(lo.z) * s.z).to_f32() };
    m[data].max = Vector { x: (x(s.x) * hi.x).to_f32(), y: (x(s.y) * hi.y).to_f32(), z: (x(s.z) * hi.z).to_f32() };
    checklowest_geometry_vertex_rec(m, o, node, data);
    let found = m[data].found as i32;
    m.release(CheckLowData::SIZE);
    found
}

/// A whole number of frames as seconds: the count goes through the register (the
/// original's rounding to an integer leaves an integer as it is), is rounded to float and
/// divided by the frame rate. Unrounded. (Inlined in the routines below.)
fn frames_to_seconds(m: &Mem, frames: i32) -> F80 {
    x(F80::from_i32(frames).to_f32()) / m[SI_FRAME_RATE]
}

/// Starts a pivot constraint [soft 0x081f81a8 si_animPivotConstrInit]: gives its update
/// node a state record (0x38 bytes from `SI_malloc` when it has none) and finds the point
/// the object stands on.
///
/// The state is cleared (both nodes null, both indices −1, the first position, both
/// angles and the first plane zero; the second position and the second plane are left as
/// they are). Then for every argument of the node that has an object, is active and
/// belongs to group 1 (and, when it has an activation curve, whose curve's integer value
/// at the current frame's time is at least 1): the object is a plane; its bounding box is
/// recomputed and the constrained object's lowest point over it searched from a height
/// of 1e10. When one is found it becomes the next point: its place goes through its
/// node's `grot` (fourth row and the node's position added), into the plane's frame, and
/// is stored with y replaced by the top of the plane's box (box maximum y × scaling y);
/// both plane fields take the plane. A later plane overwrites an earlier one's result.
pub fn anim_pivot_constr_init(m: &mut Mem, o: &mut dyn Outside, upd: Ptr32<UpdNode>) {
    let sp = m.sp();
    let found: Ptr32<Ptr32<Model>> = Ptr32::new(m.local(4));
    m[found] = Ptr32::NULL;
    let node: Ptr32<Model> = m[upd].data.cast();
    let time = frames_to_seconds(m, m[SI_FRAME_CURRENT]).to_f32();
    if m[upd].extra.is_null() {
        m[upd].extra = Ptr32::new(m.si_malloc(PivotData::SIZE));
    }
    let state: Ptr32<PivotData> = m[upd].extra.cast();
    m[state].from_angle = 0.0;
    m[state].from_pos = Vector { x: 0.0, y: 0.0, z: 0.0 };
    m[state].to_angle = 0.0;
    m[state].to = Ptr32::NULL;
    m[state].to_index = -1;
    m[state].from = Ptr32::NULL;
    m[state].from_index = -1;
    m[state].from_plane = Ptr32::NULL;
    let low: Ptr32<f32> = Ptr32::new(m.local(4));
    let point: Ptr32<Vector> = Ptr32::new(m.local(12));
    let index: Ptr32<i32> = Ptr32::new(m.local(4));
    let mut next = 0i32;
    while next < m[upd].nb_args as i32 {
        let i = next;
        next += 1;
        let arg = m[upd].args.add(i);
        if m[arg].data.is_null() || m[arg].active == 0 || m[arg].group != 1 {
            continue;
        }
        let curve = m[arg].active_data;
        if !curve.is_null() && si_fcurve_integer_value(o, m, curve, time, 0, 1) <= 0 {
            continue;
        }
        let plane: Ptr32<Model> = m[m[upd].args.add(i)].data.cast();
        si_model_compute_bounding(o, m, plane);
        m[point] = Vector { x: 0.0, y: 0.0, z: 0.0 };
        m[low] = 1e10f32;
        if checklowest_geometry_vertex(m, o, node, plane, low, point, index, found) as i16 == 0 {
            continue;
        }
        let at = m[found];
        let (p, r, g) = (m[point], m[at].grot, m[at].gtrs);
        let wx = (((x(p.y) * r[1][0] + x(p.x) * r[0][0]) + x(p.z) * r[2][0]) + r[3][0]) + g.x;
        let wy = (((x(p.x) * r[0][1] + x(p.y) * r[1][1]) + x(p.z) * r[2][1]) + r[3][1]) + g.y;
        let wz = ((x(p.z) * r[2][2] + (x(p.y) * r[1][2] + x(p.x) * r[0][2])) + r[3][2]) + g.z;
        let (q, c) = (m[plane].grot, m[plane].gtrs);
        let (dx, dy, dz) = (wx - c.x, wy - c.y, wz - c.z);
        let on = Vector {
            x: ((x(q[0][0]) * dx + x(q[0][1]) * dy) + x(q[0][2]) * dz).to_f32(),
            y: (x(m[plane].bbox_max.y) * m[plane].scal.y).to_f32(),
            z: (dz * q[2][2] + (dy * q[2][1] + dx * q[2][0])).to_f32(),
        };
        m[state].to = at;
        m[state].to_index = m[index];
        m[state].to_pos = on;
        m[state].from_plane = plane;
        m[state].to_plane = plane;
    }
    m.set_sp(sp);
}

/// Starts every pivot constraint of an update list
/// [soft 0x081f84d0 SI_sceneConstraintInit]: the nodes whose relation is 118.
pub fn scene_constraint_init(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<UpdList>) {
    let mut i = 0i32;
    while i < m[list].nb {
        let node: Ptr32<UpdNode> = m[list].nodes.cast::<UpdNode>().add(i);
        if m[node].relid == 0x76 {
            anim_pivot_constr_init(m, o, node);
        }
        i += 1;
    }
}

/// Whether a pivot constraint is between two points
/// [soft 0x081f8514 si_animPivotCheckTransition]: it has a state, a point it stands on,
/// and the lowest point is another one (another node or another index).
pub fn anim_pivot_check_transition(m: &Mem, upd: Ptr32<UpdNode>) -> i32 {
    let state: Ptr32<PivotData> = m[upd].extra.cast();
    if state.is_null() || m[state].from.is_null() || (m[state].to == m[state].from && m[state].to_index == m[state].from_index) {
        0
    } else {
        1
    }
}

/// The time of a pivot constraint's next step [soft 0x081f8548 si_animPivotTime]: `time`
/// plus one frame step in seconds, or a fraction of it while the object is tipping from
/// one point onto the next. The answer is left in the register unrounded.
///
/// While tipping (the state has a point to stand on and the lowest point is another one)
/// the lowest point is fetched (`SI_modelGetPnt`) and taken into the frame of the first
/// argument's object; the angle between the up axis (0, 1, 0) and the line from the
/// standing point to it gives A = |r32(90 − angle × 57.29577951308232)|, stored as the
/// turn still to make. With B the absolute value of the turn about the next point (stored
/// back so), the fraction is A / (A + B), or 0.5 when A + B is 0. The step is the frame
/// step rounded to float over the frame rate.
pub fn anim_pivot_time(m: &mut Mem, o: &mut dyn Outside, upd: Ptr32<UpdNode>, time: f32) -> F80 {
    let state: Ptr32<PivotData> = m[upd].extra.cast();
    let plane: Ptr32<Model> = m[m[upd].args].data.cast();
    let mut share = x(0.0f32);
    let mut tipping = 0i32;
    let from = m[state].from;
    if !from.is_null() && (m[state].to != from || m[state].to_index != m[state].from_index) {
        let at: Ptr32<Vector> = Ptr32::new(m.local(12));
        si_model_get_pnt(o, m, m[state].to, m[state].to_index, at.addr());
        let (dx, dy, dz) = from_origin(m, m[at], plane);
        m.release(12);
        let r = m[plane].grot;
        let lx = (x(r[0][0]) * dx + x(r[0][1]) * dy) + x(r[0][2]) * dz;
        let ly = (x(r[1][0]) * dx + x(r[1][1]) * dy) + x(r[1][2]) * dz;
        let lz = dz * r[2][2] + (dy * r[2][1] + dx * r[2][0]);
        let stand = m[state].from_pos;
        let up = si_math::types::Vector::new(0.0, 1.0, 0.0);
        let lean = si_math::types::Vector::new((lx - stand.x).to_f32(), (ly - stand.y).to_f32(), (lz - stand.z).to_f32());
        let angle = vector_angle(&up, &lean);
        let turn = (x(90.0f64) - x(angle) * 57.29577951308232f64).to_f32();
        m[state].from_angle = x(turn).abs().to_f32();
        let (a, b) = (x(m[state].from_angle), x(m[state].to_angle).abs());
        let sum = a + b;
        m[state].to_angle = b.to_f32();
        share = if share.eq(sum) { x(0.5f32) } else { a / sum + share };
        tipping = 1;
    }
    if tipping > 0 {
        let part = share / F80::from_i32(tipping);
        part * frames_to_seconds(m, m[SI_FRAME_STEP]) + time
    } else {
        frames_to_seconds(m, m[SI_FRAME_STEP]) + time
    }
}

type V3 = (F80, F80, F80);

fn lift(v: Vector) -> V3 {
    (x(v.x), x(v.y), x(v.z))
}

/// An offset taken into a node's frame: `grot` times the column, unrounded.
fn into_frame(r: &Matrix, d: V3) -> V3 {
    ((x(r[0][0]) * d.0 + x(r[0][1]) * d.1) + x(r[0][2]) * d.2, (x(r[1][0]) * d.0 + x(r[1][1]) * d.1) + x(r[1][2]) * d.2, d.2 * r[2][2] + (d.1 * r[2][1] + d.0 * r[2][0]))
}

/// A vector of a node's frame taken out of it: the row times `grot`, fourth row added,
/// unrounded. The compiler summed the first coordinate's first two products in either
/// order; `y_first` says which.
fn out_of_frame(r: &Matrix, v: V3, y_first: bool) -> V3 {
    let first = if y_first { v.1 * r[1][0] + v.0 * r[0][0] } else { v.0 * r[0][0] + v.1 * r[1][0] };
    ((first + v.2 * r[2][0]) + r[3][0], ((v.0 * r[0][1] + v.1 * r[1][1]) + v.2 * r[2][1]) + r[3][1], (v.2 * r[2][2] + (v.1 * r[1][2] + v.0 * r[0][2])) + r[3][2])
}

/// The point `index` of `node`, in the frame of the state's second plane, unrounded.
fn point_on_plane(m: &mut Mem, o: &mut dyn Outside, state: Ptr32<PivotData>, node: Ptr32<Model>, index: i32) -> V3 {
    let at: Ptr32<Vector> = Ptr32::new(m.local(12));
    si_model_get_pnt(o, m, node, index, at.addr());
    let plane = m[state].to_plane;
    let d = from_origin(m, m[at], plane);
    m.release(12);
    into_frame(&m[plane].grot, d)
}

/// The top of a plane's box: bounding-box maximum y times scaling y, a float.
fn plane_top(m: &Mem, plane: Ptr32<Model>) -> f32 {
    (x(m[plane].bbox_max.y) * m[plane].scal.y).to_f32()
}

/// Where the next standing point goes: `base` moved by `d`. While both points are on one
/// plane the move is laid flat: its x and z are stretched by |d| / |d without y| (both
/// lengths rounded to float; left as they are when the flat length is not above 0.0001)
/// and its y becomes 0, so the point travels as far on the plane as it travelled through
/// the air. On a new plane the move is added as it is and y is the plane's top.
fn advance(m: &Mem, state: Ptr32<PivotData>, base: Vector, d: V3) -> Vector {
    let plane = m[state].to_plane;
    if plane == m[state].from_plane {
        let (mut sx, mut sy, mut sz) = (d.0.to_f32(), 0.0f32, d.2.to_f32());
        let xx = d.0 * d.0;
        let full = ((d.1 * d.1 + xx) + d.2 * d.2).sqrt().to_f32();
        let flat = (d.2 * d.2 + (xx + x(0.0f32))).sqrt().to_f32();
        if x(flat).abs().gt(x(0.0001f64)) {
            let k = x(full) / flat;
            sx = (d.0 * k).to_f32();
            sz = (k * d.2).to_f32();
            sy = (k * x(0.0f32)).to_f32();
        }
        Vector { x: (x(base.x) + sx).to_f32(), y: (x(base.y) + sy).to_f32(), z: (x(base.z) + sz).to_f32() }
    } else {
        Vector { x: (d.0 + base.x).to_f32(), y: plane_top(m, plane), z: (x(base.z) + d.2.to_f32()).to_f32() }
    }
}

/// Offset in the record at `__SI_Model` + 0x2f4 (matter properties; its layout is not
/// established) of the float this routine reads: by finding 0015's attribute table the
/// elasticity.
const MATTER_ELASTICITY: u32 = 0x0c;

/// Index of the 16-bit word of `SI_glbAnim` (+0x14) that keeps a pivot constraint from
/// changing planes and makes it step from the standing point instead of the stored place.
const GLB_ANIM_14: usize = 10;

/// One step of a pivot constraint: moves `node` so that it stands on its lowest point over
/// `plane` [soft 0x081f8a54 si_animModelPivotPlaneUpdate]. Answers 1 when the node was
/// moved; 0 when the state has no next point or no point of the node lies over the plane.
/// The third argument is not used.
///
/// 1. A factor e: 1 when neither the plane nor the next point's node has matter
///    properties; otherwise the float at +0xc of the plane's, or (only when the plane has
///    none) of the node's. (The halving for "both" is never reached: the node's record is
///    looked at only when the plane has none.)
/// 2. When e is above 0.9999 the node is first put where its next point stands at the
///    stored place (`SI_modelSetGlobalTranslation`, then `SI_modelUpdateSonState` from its
///    father).
/// 3. When `plane` is not the state's second plane (and the word at `SI_glbAnim` + 0x14
///    is 0) both stored places are carried from that plane's frame into `plane`'s, and the
///    planes shift: first becomes the old second, second becomes `plane`.
/// 4. The node's lowest point over the second plane is searched from 1e10 and taken into
///    the plane's frame (a float each).
/// 5. With the word at `SI_glbAnim` + 0x14 set and both points known, the next place is
///    the first place moved by the offset between the two points ([`advance`]) and the
///    first point becomes the next one. Otherwise, when the lowest point is a new one, the
///    first place becomes the next place, the next place advances by the offset from the
///    next point to the lowest, the points shift (first ← next ← lowest, with the angle)
///    and the next angle is 90 − 57.29577951308232 × the angle between up and the line
///    from the next place to the first point.
/// 6. When e is below 1 the next place is pulled towards the lowest point: x and z by the
///    factor e, y to the plane's top.
/// 7. The node is moved by the offset from the lowest point to the next place, taken out
///    of the plane's frame.
pub fn anim_model_pivot_plane_update(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, plane: Ptr32<Model>, _unused: i32, upd: Ptr32<UpdNode>) -> i32 {
    let state: Ptr32<PivotData> = m[upd].extra.cast();
    let to = m[state].to;
    if to.is_null() {
        return 0;
    }
    let sp = m.sp();
    let found: Ptr32<Ptr32<Model>> = Ptr32::new(m.local(4));
    m[found] = Ptr32::NULL;
    let low: Ptr32<f32> = Ptr32::new(m.local(4));
    let point: Ptr32<Vector> = Ptr32::new(m.local(12));
    let index: Ptr32<i32> = Ptr32::new(m.local(4));
    let target: Ptr32<Vector> = Ptr32::new(m.local(12));

    let mut e = 1.0f32;
    let plane_matter = m[plane].dyna_matter;
    let mut node_has = false;
    if plane_matter.is_null() {
        node_has = !m[to].dyna_matter.is_null();
    }
    if !plane_matter.is_null() || node_has {
        e = 0.0;
        if !plane_matter.is_null() {
            e = (x(e) + m.f32(plane_matter.addr().wrapping_add(MATTER_ELASTICITY))).to_f32();
        }
        if node_has {
            let matter = m[to].dyna_matter;
            e = (x(e) + m.f32(matter.addr().wrapping_add(MATTER_ELASTICITY))).to_f32();
        }
        if !plane_matter.is_null() && node_has {
            e = (x(0.5f64) * e).to_f32();
        }
    }
    if x(e).gt(x(0.9999f64)) {
        let l = point_on_plane(m, o, state, m[state].to, m[state].to_index);
        let want = m[state].to_pos;
        let back = (x(want.x) - l.0, x(want.y) - l.1, x(want.z) - l.2);
        let w = out_of_frame(&m[m[state].to_plane].grot, back, false);
        let g = m[node].gtrs;
        m[target] = Vector { x: (w.0 + g.x).to_f32(), y: (w.1 + g.y).to_f32(), z: (w.2 + g.z).to_f32() };
        si_model_set_global_translation(o, m, node, target.addr(), 1, 0);
        si_model_update_son_state(o, m, m[node].father, node, 1);
    }

    let old = m[state].to_plane;
    if plane != old && m[SI_GLB_ANIM][GLB_ANIM_14] == 0 {
        let carry = |m: &Mem, pos: Vector| {
            let w = out_of_frame(&m[old].grot, lift(pos), true);
            let (c, p) = (m[old].gtrs, m[plane].gtrs);
            let d = (w.0 + c.x - p.x, w.1 + c.y - p.y, w.2 + c.z - p.z);
            let l = into_frame(&m[plane].grot, d);
            Vector { x: l.0.to_f32(), y: l.1.to_f32(), z: l.2.to_f32() }
        };
        let from_pos = carry(m, m[state].from_pos);
        m[state].from_pos = from_pos;
        let to_pos = carry(m, m[state].to_pos);
        m[state].from_plane = old;
        m[state].to_plane = plane;
        m[state].to_pos = to_pos;
    }

    m[point] = Vector { x: 0.0, y: 0.0, z: 0.0 };
    m[low] = 1e10f32;
    if checklowest_geometry_vertex(m, o, node, m[state].to_plane, low, point, index, found) as i16 == 0 {
        m.set_sp(sp);
        return 0;
    }
    let at = m[found];
    let w = out_of_frame(&m[at].grot, lift(m[point]), true);
    let (g, c) = (m[at].gtrs, m[m[state].to_plane].gtrs);
    let d = (w.0 + g.x - c.x, w.1 + g.y - c.y, w.2 + g.z - c.z);
    let l = into_frame(&m[m[state].to_plane].grot, d);
    let lowest = Vector { x: l.0.to_f32(), y: l.1.to_f32(), z: l.2.to_f32() };

    if m[SI_GLB_ANIM][GLB_ANIM_14] != 0 && !m[state].from.is_null() && !m[state].to.is_null() {
        let a = point_on_plane(m, o, state, m[state].from, m[state].from_index);
        let a = Vector { x: a.0.to_f32(), y: a.1.to_f32(), z: a.2.to_f32() };
        let b = point_on_plane(m, o, state, m[state].to, m[state].to_index);
        let d = (b.0 - a.x, b.1 - a.y, b.2 - a.z);
        let next = advance(m, state, m[state].from_pos, d);
        m[state].to_pos = next;
        m[state].from = m[state].to;
        m[state].from_index = m[state].to_index;
        m[state].from_plane = m[state].to_plane;
    } else if m[state].to != m[found] || m[state].to_index != m[index] {
        m[state].from_pos = m[state].to_pos;
        let b = point_on_plane(m, o, state, m[state].to, m[state].to_index);
        let d = (x(lowest.x) - b.0, x(lowest.y) - b.1, x(lowest.z) - b.2);
        let next = advance(m, state, m[state].to_pos, d);
        m[state].to_pos = next;
        m[state].from = m[state].to;
        m[state].from_angle = fmov(m[state].to_angle);
        m[state].from_index = m[state].to_index;
        m[state].to = m[found];
        m[state].to_index = m[index];
        let c = point_on_plane(m, o, state, m[state].from, m[state].from_index);
        let next = m[state].to_pos;
        let up = si_math::types::Vector::new(0.0, 1.0, 0.0);
        let lean = si_math::types::Vector::new((c.0 - next.x).to_f32(), (c.1 - next.y).to_f32(), (c.2 - next.z).to_f32());
        let angle = vector_angle(&up, &lean);
        m[state].to_angle = (x(90.0f64) - x(angle) * 57.29577951308232f64).to_f32();
    }

    if x(1.0f32).gt(x(e)) {
        let next = m[state].to_pos;
        let pulled = (x(next.x) - lowest.x) * e;
        let z = ((x(next.z) - lowest.z) * e).to_f32();
        let top = plane_top(m, m[state].to_plane);
        m[state].to_pos.x = (pulled + lowest.x).to_f32();
        m[state].to_pos.y = (x(0.0f32) + top).to_f32();
        m[state].to_pos.z = (x(lowest.z) + z).to_f32();
    }

    let next = m[state].to_pos;
    let d = (x(next.x) - lowest.x, x(next.y) - lowest.y, x(next.z) - lowest.z);
    let w = out_of_frame(&m[m[state].to_plane].grot, d, false);
    let g = m[node].gtrs;
    m[target] = Vector { x: (w.0 + g.x).to_f32(), y: (w.1 + g.y).to_f32(), z: (w.2 + g.z).to_f32() };
    si_model_set_global_translation(o, m, node, target.addr(), 1, 0);
    m.set_sp(sp);
    1
}

/// The current graf list: `SI_grafLists` + 0x24 × the second word of `SI_procData`.
fn graf_list(m: &Mem) -> u32 {
    let slot = m.i32(m[SI_PROC_DATA].addr().wrapping_add(4));
    m[SI_GRAF_LISTS].wrapping_add((slot as u32).wrapping_mul(0x24))
}

/// Offset in a graf node (what `SI_findGlData` answers; its layout is not established
/// here) of the selection state: 0 not selected, 1 as a node, 2 as part of a branch.
const GRAF_SELECTION: u32 = 8;

/// Keeps an object above a plane [soft 0x081f979c si_animModelBoundPlaneUpdate]: when
/// its lowest point is below `height` over the plane's top, moves it up by the
/// difference. Answers 1 when it moved something. The fifth argument is not used.
///
/// The search starts at r32(height + top) with top = the plane's box maximum y times its
/// global scaling y (+0x170), so it finds a point only at or below that. What is moved
/// is the node itself, in mode 1, unless `select` is not −1: then the lowest point's own
/// node is looked up in the current graf list and the walk climbs to its father while
/// the list's first word is set and there is a father, or while the node is selected as
/// part of a branch; with the list's word set the last ancestor found selected is taken,
/// otherwise the node the walk ended on. A node selected as a node is moved in mode 0
/// unless it is a null of algorithm 3 or 4 (an effector); such a node, and one not
/// selected, fall back to the object itself in mode 1.
///
/// The moved node's position goes into the plane's frame; its y is lowered by the
/// lowest point's height less top (in the register); the result comes back out of the
/// frame with the plane's position added and, when the moved node has a father, goes into
/// the father's frame. `SI_modelSetTranslation` takes it.
pub fn anim_model_bound_plane_update(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, plane: Ptr32<Model>, height: f32, select: i32, _unused: i16) -> i32 {
    let sp = m.sp();
    let point: Ptr32<Vector> = Ptr32::new(m.local(12));
    m[point] = Vector { x: 0.0, y: 0.0, z: 0.0 };
    let mut mode = 1i32;
    let top_product = x(m[plane].bbox_max.y) * m[plane].gscl.y;
    let low: Ptr32<f32> = Ptr32::new(m.local(4));
    m[low] = (x(height) + top_product).to_f32();
    let top = top_product.to_f32();
    let index: Ptr32<i32> = Ptr32::new(m.local(4));
    let found: Ptr32<Ptr32<Model>> = Ptr32::new(m.local(4));
    let out: Ptr32<Vector> = Ptr32::new(m.local(12));
    if checklowest_geometry_vertex(m, o, node, plane, low, point, index, found) as i16 == 0 {
        m.set_sp(sp);
        return 0;
    }
    let mut target = node;
    if select != -1 {
        let mut graf = si_find_gl_data(o, m, graf_list(m), m[found].addr(), 2);
        let mut best = m[found];
        let mut walk = best;
        let mut best_graf = graf;
        let listed = loop {
            let listed = m.u16(graf_list(m));
            if (listed != 0 && !m[walk].father.is_null()) || m.i32(graf.wrapping_add(GRAF_SELECTION)) == 2 {
                walk = m[walk].father;
                graf = si_find_gl_data(o, m, graf_list(m), walk.addr(), 2);
                if m.u16(graf_list(m)) != 0 && m.i32(graf.wrapping_add(GRAF_SELECTION)) != 0 {
                    best = walk;
                    best_graf = graf;
                }
                continue;
            }
            break listed;
        };
        if listed != 0 {
            graf = best_graf;
        } else {
            best = walk;
        }
        target = best;
        let selection = m.i32(graf.wrapping_add(GRAF_SELECTION));
        let fall_back = if selection == 1 {
            mode = 0;
            m[best].kind == 0 && (m[best].algo_type.wrapping_sub(3) as u32) <= 1
        } else {
            selection == 0
        };
        if fall_back {
            target = node;
            mode = 1;
        }
    }
    let (c, r) = (m[plane].gtrs, m[plane].grot);
    let g = m[target].gtrs;
    let (dx, dy, dz) = (x(g.x) - c.x, x(g.y) - c.y, x(g.z) - c.z);
    let lx = (x(r[0][0]) * dx + x(r[0][1]) * dy) + x(r[0][2]) * dz;
    let lz = dz * r[2][2] + (dy * r[2][1] + dx * r[2][0]);
    let ly = ((x(r[1][1]) * dy + x(r[1][0]) * dx) + x(r[1][2]) * dz) - (x(m[low]) - top);
    let father = m[target].father;
    let wx = ((lz * r[2][0] + (lx * r[0][0] + ly * r[1][0])) + r[3][0]) + c.x;
    let wz = ((lz * r[2][2] + (ly * r[1][2] + lx * r[0][2])) + r[3][2]) + c.z;
    if father.is_null() {
        let wy = ((((ly * r[1][1] + lx * r[0][1]) + lz * r[2][1]) + r[3][1])) + c.y;
        m[out] = Vector { x: wx.to_f32(), y: wy.to_f32(), z: wz.to_f32() };
    } else {
        let wy = ((x(r[2][1]) * lz + (lx * r[0][1] + ly * r[1][1])) + r[3][1]) + c.y;
        let f = m[m[target].father].gtrs;
        let l = into_frame(&m[m[target].father].grot, (wx - f.x, wy - f.y, wz - f.z));
        m[out] = Vector { x: l.0.to_f32(), y: l.1.to_f32(), z: l.2.to_f32() };
    }
    si_model_set_translation(o, m, target, out.addr(), mode, 0);
    m.set_sp(sp);
    1
}
