// SPDX-License-Identifier: MIT
//! The rest of `skeleton.c`, on the shared memory image (finding 0024): drawing a joint,
//! a chain from a spline's points, the tube mesh around a chain and the automatic
//! envelope. Two functions of the first pass are here again in image form because these
//! call them ([`ske_joint_number`], [`ske_chain_total_length`]).
//!
//! Graphics (`graphics.c`, `draw.c`), geometry construction (`primitive.c`,
//! `extrusion.c`, `spline.c`, `patch.c`, `nrb_*.c`) and the node routines of `model.c`
//! are outside this package and reached through [`Outside`].

use si_dict::globals::SI_GLB_MANIP;
use si_math::algebra::points_angle_signed;
use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey, Vector};
use si_types::model::Model;
use si_x87::{fmov, x, F80};

use crate::outside as out;
use crate::outside::Outside;
use crate::rodata::{S_AUTOENV, SKE_JOINT_POINTS};

fn mv(v: Vector) -> si_math::types::Vector {
    si_math::types::Vector::new(v.x, v.y, v.z)
}

/// An effector: a null of algorithm 3 or 4.
fn is_effector(m: &Mem, node: Ptr32<Model>) -> bool {
    m[node].kind == 0 && (m[node].algo_type.wrapping_sub(3) as u32) <= 1
}

/// A chain root: a null of algorithm 1 or 2.
fn is_root(m: &Mem, node: Ptr32<Model>) -> bool {
    !node.is_null() && m[node].kind == 0 && (m[node].algo_type.wrapping_sub(1) as u32) <= 1
}

/// The number of joints of a chain [soft 0x081d2234 si_skeJointNumber]: the nodes from
/// the root's son down to, not including, the first effector; 0 for anything but a chain
/// root. No son is tested for null.
pub fn ske_joint_number(m: &Mem, root: Ptr32<Model>) -> i32 {
    let mut count = 0;
    if !is_root(m, root) {
        return count;
    }
    let mut node = m[root].son;
    while !is_effector(m, node) {
        if m.fault().is_some() {
            break;
        }
        node = m[node].son;
        count += 1;
    }
    count
}

/// The length of a chain [soft 0x081d2180 skeChainTotalLength]: over the joints from the
/// root's son down to the effector, the distance from each to its son, the running sum
/// rounded to float after every addition; under classical scaling (`SI_glbManip` word 0
/// is 1) each term is instead |the joint's +0x3a0| × the son's local x. 0 for anything
/// but a chain root. The answer is left in the register.
pub fn ske_chain_total_length(m: &Mem, root: Ptr32<Model>) -> F80 {
    let mut sum = 0.0f32;
    if !is_root(m, root) {
        return x(sum);
    }
    let mut node = m[root].son;
    while !is_effector(m, node) {
        if m.fault().is_some() {
            break;
        }
        let son = m[node].son;
        let term = if m.i32(SI_GLB_MANIP.addr()) == 1 {
            x(m[node].cscl.x).abs() * m[son].trans.x
        } else {
            let (a, b) = (m[node].gtrs, m[son].gtrs);
            let (dx, dy) = (x(a.x) - b.x, x(a.y) - b.y);
            ((dx * dx + dy * dy) + (x(a.z) - b.z) * (x(a.z) - b.z)).sqrt()
        };
        sum = (x(sum) + term).to_f32();
        node = son;
    }
    x(sum)
}

/// Draws a joint's bone [soft 0x081d236c SI_skeJointDraw]: three closed lines through the
/// six points of a static table (`p.243`), of which two coordinates are set first: the
/// first point's x is the bone's length (the son's local x) and the third point's x is
/// −0.25 for a positive global x scaling, else 0.25. The lines are points 0 1 2 3,
/// 0 4 2 5 and 1 5 3 4. With `center` not 0 the centre mark is drawn too, the colour
/// read before and set again after.
pub fn ske_joint_draw(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, center: i16) {
    let p = SKE_JOINT_POINTS.addr();
    let length = m[m[node].son].trans.x;
    m.set_f32(p, fmov(length));
    let side = if x(m[node].gscl.x).gt(x(0.0f32)) { -0.25f32 } else { 0.25f32 };
    m.set_f32(p + 24, side);
    for line in [[0u32, 1, 2, 3], [0, 4, 2, 5], [1, 5, 3, 4]] {
        o.call(m, &out::BGNCLOSEDLINE, &[]);
        for i in line {
            o.call(m, &out::V3F, &[p + 12 * i]);
        }
        o.call(m, &out::ENDCLOSEDLINE, &[]);
    }
    if center != 0 {
        let colour = o.call(m, &out::GETCOLOR, &[]).eax;
        o.call(m, &out::SI_DRAW_CENTER, &[]);
        o.call(m, &out::COLOR, &[colour as u16 as u32]);
    }
}

/// Makes a chain along a spline's keys [soft 0x081d24a0 skeSplineToChain] and answers its
/// first node. Every node comes from `SI_modelAllocate(0)`.
///
/// The first node takes the first key's position as its translation. One node follows
/// per key, each the son of the one before; all but the last are joints (kind 5); from
/// the second on the local x is the distance between its key and the one before. The
/// first joint's z rotation is the signed angle (`SI_pointsAngleSigned`) at the first key
/// between the second and the last key; each later joint's is π − the signed angle at its
/// key between its neighbours, with −π when that angle is not above 0. A spline without
/// keys is not provided for (the original then follows a link nobody set).
pub fn ske_spline_to_chain(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>) -> Ptr32<Model> {
    let first: Ptr32<Model> = out::si_model_allocate(o, m, 0);
    let p = m[m[spline].keys].pos;
    m[first].trans = p;
    let mut at = first;
    let mut i = 0i32;
    while i < m[spline].nb_keys as u16 as i32 {
        let node: Ptr32<Model> = out::si_model_allocate(o, m, 0);
        m[at].son = node;
        m[node].father = at;
        if i < m[spline].nb_keys as u16 as i32 - 1 {
            m[node].kind = 5;
        }
        if i > 0 {
            let keys: Ptr32<SplineKey> = m[spline].keys;
            let (a, b) = (m[keys.add(i - 1)].pos, m[keys.add(i)].pos);
            let (dx, dy, dz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
            m[node].trans.x = ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32();
        }
        at = m[at].son;
        i += 1;
    }
    let keys: Ptr32<SplineKey> = m[spline].keys;
    let count = m[spline].nb_keys as u16 as i32;
    let (a, b, c) = (m[keys.add(1)].pos, m[keys].pos, m[keys.add(count - 1)].pos);
    let mut joint = m[first].son;
    m[joint].vrot.z = points_angle_signed(&mv(a), &mv(b), &mv(c));
    joint = m[joint].son;
    let mut i = 1i32;
    while i < m[spline].nb_keys as u16 as i32 - 1 {
        let keys: Ptr32<SplineKey> = m[spline].keys;
        let (a, b, c) = (m[keys.add(i - 1)].pos, m[keys.add(i)].pos, m[keys.add(i + 1)].pos);
        let angle = x(points_angle_signed(&mv(a), &mv(b), &mv(c)));
        let half_turn = if x(0.0f32).lt(angle) { x(core::f64::consts::PI) } else { x(-core::f64::consts::PI) };
        m[joint].vrot.z = (half_turn - angle).to_f32();
        joint = m[joint].son;
        i += 1;
    }
    first
}

/// The tube mesh around a chain [soft 0x081d26bc skeTubing]: a circle (`SI_splineCircle(2,
/// 0.67, 12)`) extruded along a spline through the chain (`SI_meshExtrudeSplineOnSpline(
/// circle, 1, path, 6, 1)`). Answers the mesh; both splines are disposed of.
///
/// The path is a new spline of type 3, tension 0.5, step 10, with joints + 3 keys: the
/// global positions of the root's son, of each node down the chain and of the last one
/// again, the first and the last twice; a key's handles are its position.
pub fn ske_tubing(m: &mut Mem, o: &mut dyn Outside, root: Ptr32<Model>) -> u32 {
    let joints = ske_joint_number(m, root) as i16;
    let sp = m.sp();
    let path_at = m.local(4);
    let circle_at = m.local(4);
    let path: Ptr32<Spline> = out::si_spline_allocate(o, m);
    m.set_u32(path_at, path.addr());
    m[path].tension = 0.5;
    m[path].kind = 3;
    m[path].step = 10;
    m[path].nb_keys = joints.wrapping_add(3);
    let keys: Ptr32<SplineKey> = out::si_spline_key_allocate(o, m, m[path].nb_keys as u16 as i32);
    m[path].keys = keys;
    let mut node = m[root].son;
    let put = |m: &mut Mem, index: i32, node: Ptr32<Model>| {
        let p = m[node].gtrs;
        let key = m[path].keys.add(index);
        m[key].pos = p;
        m[key].next = p;
        m[key].prev = p;
    };
    put(m, 0, node);
    let mut i = 1i32;
    while i < m[path].nb_keys as u16 as i32 - 1 {
        put(m, i, node);
        if i < m[path].nb_keys as u16 as i32 - 2 {
            node = m[node].son;
        }
        i += 1;
    }
    put(m, i, node);
    out::si_spline_update_matrix(o, m, path);
    let circle = o.call(m, &out::SI_SPLINE_CIRCLE, &[2, 0.67f32.to_bits(), 12]).eax;
    m.set_u32(circle_at, circle);
    let mesh = o.call(m, &out::SI_MESH_EXTRUDE_SPLINE_ON_SPLINE, &[circle, 1, path.addr(), 6, 1]).eax;
    o.call(m, &out::SI_SPLINE_DISPOSE, &[path_at]);
    o.call(m, &out::SI_SPLINE_DISPOSE, &[circle_at]);
    m.set_sp(sp);
    mesh
}

/// Offset in `__SI_Model` of the node that shows a chain's tube (`unk_320` in `si-types`).
const TUBE_NODE: u32 = 0x320;

/// Rebuilds a chain's tube [soft 0x081d28ac skeTubingUpdate], for a root whose envelope
/// word (+0x31c) is 3 only. Without a tube node one is made (`SI_modelAllocate(0)`,
/// stored at +0x320) and gets the new mesh as definition 4; otherwise the mesh replaces
/// the node's definition 0. The node's geometry is marked changed.
pub fn ske_tubing_update(m: &mut Mem, o: &mut dyn Outside, root: Ptr32<Model>) {
    if m[root].envelop != 3 {
        return;
    }
    let slot = root.addr().wrapping_add(TUBE_NODE);
    if m.u32(slot) == 0 {
        let node: Ptr32<Model> = out::si_model_allocate(o, m, 0);
        m.set_u32(slot, node.addr());
        let mesh = ske_tubing(m, o, root);
        out::si_model_add_definition(o, m, Ptr32::<Model>::new(m.u32(slot)), 4, mesh);
    } else {
        let mesh = ske_tubing(m, o, root);
        o.call(m, &out::SI_MODEL_SUBSTITUTE_I_DEFINITION, &[m.u32(slot), 0, mesh]);
    }
    o.call(m, &out::SI_MODEL_TOUCH_GEOMETRY, &[m.u32(slot)]);
}

/// Makes the automatic envelope of a chain [soft 0x081d2934 si_skeAutoEnvelope]: a
/// profile extruded to the chain's length. Answers the new node (named `autoenv`), null
/// for anything but a chain root.
///
/// The length is [`ske_chain_total_length`] rounded to float; under classical scaling it
/// is divided by the root's +0x3a0 x when that is above 0.0001 in size, else by its
/// global x scaling when that is. The profile is `profile`'s current definition, a NURBS
/// curve when the node's kind is 9 and a spline otherwise; without a profile a circle of
/// radius 0.67 is made, as a NURBS curve when `nurbs` is set (its word at +0x34 set to 3)
/// and as a spline otherwise (step 3), and disposed of at the end. A spline is extruded
/// to a patch (`SI_splineExtrudeToPatch(profile, 2, length, steps × joints, 2, 3)`,
/// definition 1, y rotation +π/2), a NURBS curve to a NURBS surface
/// (`SI_nurbsExtrudeToNurbs(profile, 3, −length, …)`, definition 10, y rotation −π/2);
/// `cap_end` and `cap_start` set a word of the surface and ask for a cap. The node is
/// updated and its rotation frozen.
pub fn ske_auto_envelope(m: &mut Mem, o: &mut dyn Outside, root: Ptr32<Model>, profile: Ptr32<Model>, steps: i32, cap_end: i16, cap_start: i16, nurbs: i16) -> Ptr32<Model> {
    if !is_root(m, root) {
        return Ptr32::NULL;
    }
    let sp = m.sp();
    let spline_at = m.local(4);
    let curve_at = m.local(4);
    m.set_u32(spline_at, 0);
    m.set_u32(curve_at, 0);
    let mut made = false;
    let mut length = ske_chain_total_length(m, root).to_f32();
    if m.i32(SI_GLB_MANIP.addr()) == 1 {
        let by = if x(0.0001f64).lt(x(m[root].cscl.x).abs()) {
            Some(m[root].cscl.x)
        } else if x(m[root].gscl.x).abs().gt(x(0.0001f64)) {
            Some(m[root].gscl.x)
        } else {
            None
        };
        if let Some(by) = by {
            length = (x(length) / by).to_f32();
        }
    }
    let joints = ske_joint_number(m, root);
    if profile.is_null() {
        made = true;
        if nurbs != 0 {
            let curve = o.call(m, &out::SI_NURBS_CIRCLE, &[0.67f32.to_bits(), 4, 10]).eax;
            m.set_u32(curve_at, curve);
            m.set_i32(curve.wrapping_add(0x34), 3);
        } else {
            let spline = o.call(m, &out::SI_SPLINE_CIRCLE, &[2, 0.67f32.to_bits(), 10]).eax;
            m.set_u32(spline_at, spline);
            m.set_i16(spline.wrapping_add(0x20), 3);
        }
    } else {
        let definition = m.u32(m[profile].defs.addr().wrapping_add(m[profile].cur_def as u32 * 4));
        m.set_u32(if m[profile].kind == 9 { curve_at } else { spline_at }, definition);
    }
    let node: Ptr32<Model> = out::si_model_allocate(o, m, S_AUTOENV.0);
    let count = steps.wrapping_mul(joints) as u32;
    let turn = if m.u32(curve_at) != 0 {
        length = (-x(length)).to_f32();
        let surface = o.call(m, &out::SI_NURBS_EXTRUDE_TO_NURBS, &[m.u32(curve_at), 3, length.to_bits(), count, 2, 3]).eax;
        if cap_end != 0 {
            m.set_i16(surface.wrapping_add(0x8e), 1);
            o.call(m, &out::SI_NURBS_SRF_FORCE_CAPPING, &[surface, 1, 1]);
        }
        if cap_start != 0 {
            m.set_i16(surface.wrapping_add(0x8c), 1);
            o.call(m, &out::SI_NURBS_SRF_FORCE_CAPPING, &[surface, 0, 1]);
        }
        out::si_model_add_definition(o, m, node, 10, surface);
        f32::from_bits(0xbfc9_0fdb)
    } else {
        let patch = o.call(m, &out::SI_SPLINE_EXTRUDE_TO_PATCH, &[m.u32(spline_at), 2, length.to_bits(), count, 2, 3]).eax;
        if cap_end != 0 {
            m.set_i16(patch.wrapping_add(0x1ce), 1);
            o.call(m, &out::SI_PATCH_FORCE_CAPPING, &[patch, 3, 1]);
        }
        if cap_start != 0 {
            m.set_i16(patch.wrapping_add(0x1cc), 1);
            o.call(m, &out::SI_PATCH_FORCE_CAPPING, &[patch, 2, 1]);
        }
        out::si_model_add_definition(o, m, node, 1, patch);
        f32::from_bits(0x3fc9_0fdb)
    };
    m[node].vrot.y = turn;
    out::si_model_update_son_state(o, m, Ptr32::<Model>::NULL, node, 0);
    o.call(m, &out::SI_MODEL_FREEZE_ROTATION, &[node.addr()]);
    if made {
        if m.u32(curve_at) != 0 {
            o.call(m, &out::SI_NURBS_CRV_DISPOSE, &[curve_at]);
        } else {
            o.call(m, &out::SI_SPLINE_DISPOSE, &[spline_at]);
        }
    }
    m.set_sp(sp);
    node
}
