// SPDX-License-Identifier: MIT
//! Quick stretch on the image: the update of relation 239 (`QStretch`) that
//! `SI_modelReadjustDefinition` makes, `SI_animQuickStretchUpdate` of `qstretch.c`, with the
//! two small routines of other files it needs that no crate has on the image:
//! `SI_QuickStretchGetElem` (`qstretch_misc.c`) and `SI_modelKineStateAllocate`,
//! `SI_modelKineStateSetDefault` (`settings.c`). Finding 0032, "Quick stretch". With them
//! `SI_quickStretchActive` (`qstretch_misc.c`), the update's question whether quick stretch
//! deforms a model.
//!
//! The deformation itself, `SI_QuickStretchCalculation` (12,748 bytes of `qstretch.c`), is
//! [`super::qscalc`].

use si_mem::Mem;
use si_x87::{x, F80};

use crate::outside as out;
use crate::outside::Outside;

/// `SI_RelQSData`, the data of relation 239 (the relation entry's +0x18): twelve groups of
/// six floats from +0x0c (each a parameter and its rate, scaled below), then two 16-bit
/// words. Named offsets: the record is not laid out further than this file reads it.
pub mod qs {
    /// 16-bit: 0 the controls are not used; 1 the stretch is computed from the first father
    /// that has quick stretch too.
    pub const MODE: u32 = 0x120;
    /// 16-bit: handed to `SI_QuickStretchCalculation` as its last argument.
    pub const LAST_ARG: u32 = 0x122;
    /// The float of curve kind `0xf5 + k` (k < 12), multiplied by 0.005.
    pub const fn scaled(k: u32) -> u32 {
        0xc + 0x18 * k
    }
    /// The float of curve kind `0xe9 + k`, divided by |the float of `scaled(k)`|.
    pub const fn rate(k: u32) -> u32 {
        0x10 + 0x18 * k
    }
}

/// The kinematic record (`__SI_KineInfo`, `__SI_Model` + 0x2f0, 0x60 bytes; finding 0015's
/// `SI_modelKineStateCompute` reads the same layout): position, a 3 × 3 orientation,
/// linear velocity and acceleration, angular velocity and acceleration.
pub mod kine {
    pub const SIZE: u32 = 0x60;
    pub const ORIENT: u32 = 0x0c;
    pub const LIN_VEL: u32 = 0x30;
    pub const LIN_ACC: u32 = 0x3c;
    pub const ANG_VEL: u32 = 0x48;
    pub const ANG_ACC: u32 = 0x54;
}

/// `SI_matrix3ID` (0x086dc220), the 3 × 3 identity in `.data`.
pub const SI_MATRIX3_ID: u32 = 0x086d_c220;
/// `SI_identite` (0x086dc160), `SI_yAxis` (0x086dc1f8).
pub const SI_IDENTITE: u32 = 0x086d_c160;
pub const SI_Y_AXIS: u32 = 0x086d_c1f8;
/// `SI_cid` (0x08781660): its first word the ANIMATION chapter, +0x20 the MODELS chapter.
pub const SI_CID: u32 = 0x0878_1660;
/// `SI_RD` (0x087815f0), the relation dictionary.
pub const SI_RD: u32 = 0x0878_15f0;
/// `SI_playBackRun` (0x08695e92, 16 bits), `SI_frameStart` (0x08782050),
/// `SI_frameRate` (0x0878204c).
pub const SI_PLAY_BACK_RUN: u32 = 0x0869_5e92;
pub const SI_FRAME_START: u32 = 0x0878_2050;
pub const SI_FRAME_RATE: u32 = 0x0878_204c;
/// 0x08672b60, a double: 0.005; 0x08672b68, a double: 2.
pub const SCALE: f64 = 0.005;
pub const TWO: f64 = 2.0;

/// Relation 239 and the four kinematic attributes (`LinVel`, `LinAcc`, `AngVel`,
/// `AngAcc`) whose curves fill the record.
pub const REL_QSTRETCH: u32 = 0xef;
const ATTR_LIN_VEL: u32 = 0x5a;
const ATTR_LIN_ACC: u32 = 0x5b;
const ATTR_ANG_VEL: u32 = 0x5c;
const ATTR_ANG_ACC: u32 = 0x5d;

/// The current dictionary: `SI_elemDicts + 0x70 × SI_procData[0]`, as computed at each call.
fn cur_dict(m: &Mem) -> u32 {
    let p = m.u32(crate::globals::SI_PROC_DATA.addr());
    m.u32(0x0867_931c).wrapping_add(m.u32(p).wrapping_mul(0x70))
}

/// [soft 0x0849f37c SI_modelKineStateSetDefault]: nothing for null; else the position
/// zero, the orientation `SI_matrix3Copy(SI_matrix3ID)`, then the four vectors zero (each
/// written z, y, x).
pub fn kine_state_set_default(m: &mut Mem, k: u32) {
    if k == 0 {
        return;
    }
    for o in [8u32, 4, 0] {
        m.set_u32(k + o, 0);
    }
    let mut id = [[0.0f32; 3]; 3];
    for (i, row) in id.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(SI_MATRIX3_ID + 12 * i as u32 + 4 * j as u32);
        }
    }
    let c = si_math::matrix3tools::matrix3_copy(&id);
    for (i, row) in c.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_u32(k + kine::ORIENT + 12 * i as u32 + 4 * j as u32, v.to_bits());
        }
    }
    for base in [kine::LIN_VEL, kine::LIN_ACC, kine::ANG_VEL, kine::ANG_ACC] {
        for o in [8u32, 4, 0] {
            m.set_u32(k + base + o, 0);
        }
    }
}

/// [soft 0x0849f9f8 SI_modelKineStateAllocate]: `SI_malloc(0x60)` and, when that gave a
/// block, its defaults ([`kine_state_set_default`]).
pub fn kine_state_allocate(m: &mut Mem) -> u32 {
    let k = m.si_malloc(kine::SIZE);
    if k != 0 {
        kine_state_set_default(m, k);
    }
    k
}

/// [soft 0x085a2428 SI_QuickStretchGetElem]: the element of the model the node's element
/// is related to by quick stretch (239, forward list 2, a model); `*out` the data of that
/// relation entry (`SI_elemdictGetCurRelation` + 0x18, read without a test of the entry),
/// null when there is none; the related element's index goes to `*index`.
pub fn quick_stretch_get_elem(m: &mut Mem, o: &mut dyn Outside, model: u32, index: u32, data_out: u32) -> u32 {
    m.set_u32(data_out, 0);
    let mine = m.local(4);
    let chap_out = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[model, mine]).eax;
    let d = cur_dict(m);
    let chap = m.u32(SI_CID + 0x20);
    let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, e, 1, 2, REL_QSTRETCH, chap, chap_out, index]).eax;
    if r != 0 {
        let d = cur_dict(m);
        let rel = o.call(m, &out::SI_ELEMDICT_GET_CUR_RELATION, &[d, e, 1, 2]).eax;
        let v = m.u32(rel.wrapping_add(0x18));
        m.set_u32(data_out, v);
    }
    m.release(4);
    m.release(4);
    r
}

/// [soft 0x085a23c0 SI_quickStretchActive]: whether the model has quick stretch on, as the
/// update asks it before it takes a model for undeformed: the model's element
/// (`SI_edGetElemFromModel`, its index into a local); none, 0. Else the entry of relation
/// 239 that names that element itself (`SI_elemdictGetRelationFromIds(current dictionary,
/// element, 239, the MODELS chapter, index, -1)`, the dictionary computed as
/// `SI_procData[0] × 0x70 + SI_elemDicts`); none, 0; else the entry's 16-bit word at +0x14,
/// sign-extended.
pub fn quick_stretch_active(m: &mut Mem, o: &mut dyn Outside, model: u32) -> i32 {
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[model, index]).eax;
    let mut r = 0;
    if e != 0 {
        let (chap, i) = (m.u32(SI_CID + 0x20), m.u32(index));
        let d = cur_dict(m);
        let rel = o.call(m, &out::SI_ELEMDICT_GET_RELATION_FROM_IDS, &[d, e, REL_QSTRETCH, chap, i, u32::MAX]).eax;
        if rel != 0 {
            r = m.i16(rel.wrapping_add(0x14)) as i32;
        }
    }
    m.release(4);
    r
}

/// A curve's value as the update reads it: 1 for a curve without keys (`fld1`), else
/// `SI_fcurveValue(curve, time, 1)` at the register's width.
fn curve_value(m: &mut Mem, o: &mut dyn Outside, curve: u32, time: f32) -> F80 {
    if m.i32(curve + 0xc) < 1 {
        F80::ONE
    } else {
        o.call(m, &out::SI_FCURVE_VALUE, &[curve, time.to_bits(), 1]).st0
    }
}

/// The controls' vector: `SI_yAxis` through the 3 × 3 corner of `t` (a control's global
/// rotation transposed), each component ((t[i][0]·y₀ + t[i][1]·y₁) + t[i][2]·y₂) kept in
/// the register and multiplied by |s|·s (s the control's scaling in y, +0x104), then
/// rounded to a float.
fn control_vector(t: &si_math::types::Matrix, y: [f32; 3], s: f32) -> [f32; 3] {
    let k = x(s).abs() * s;
    let r = |i: usize| (x(t[i][0]) * y[0] + x(t[i][1]) * y[1]) + x(t[i][2]) * y[2];
    [(r(0) * k).to_f32(), (r(1) * k).to_f32(), (r(2) * k).to_f32()]
}

/// The original's frame: one block of locals laid out at the offsets the original gives
/// them below its frame pointer (`at(0x98)` is `ebp − 0x98`).
struct Frame(u32);

const FRAME: u32 = 0x140;

impl Frame {
    fn at(&self, below: u32) -> u32 {
        self.0 + FRAME - below
    }
}

/// [soft 0x085a144c SI_animQuickStretchUpdate]: the quick-stretch step of an update node
/// (relation 239) at `time`, as `SI_modelReadjustDefinition` calls it.
///
/// In the original's order:
///
/// 1. Two local matrices from `SI_identite` (`SI_matrixCopy`); nothing for a null node or
///    a node without a model; the model's quick-stretch element and data
///    ([`quick_stretch_get_elem`]), nothing without one. The data's word +0x122 is kept.
/// 2. Twice over the node's arguments from the second: an active argument of the
///    ANIMATION chapter with a curve gives the curve's value (1 without keys,
///    `SI_fcurveValue(curve, time, 1)` otherwise); by the curve's kind, 0xf5 … 0x100 store
///    it into [`qs::scaled`] and then that float × 0.005 (a double); in the second pass
///    0xe9 … 0xf4 store it into [`qs::rate`] and then that ÷ |the scaled float|. Each
///    pass asks every such curve for its value, whatever its kind.
/// 3. While `SI_playBackRun` is set: nothing when float(trunc(float(`SI_frameStart` + 2))) ÷
///    `SI_frameRate` > time.
/// 4. A model with definitions whose current definition is still the stored one gets a
///    copy of it (`SI_patchCopy`, `SI_faceCopy`, `SI_meshCopy`, `SI_splineCopy`,
///    `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy` by kind 1, 2, 4, 6, 9, 10) as its current one.
/// 5. Two vectors, zero; with the data's mode (+0x120) set, every active argument of the
///    MODELS chapter (a control) of algorithm 0x10 gives the first, of 0x11 the second
///    ([`control_vector`] of its transposed global rotation, `SI_matrixTranspose`).
/// 6. The model's kinematic record, allocated when it has none
///    ([`kine_state_allocate`]); nothing more without one. When `SI_reldictGetRelChapGrp`
///    knows `LinVel` and `LinAcc` (90, 91) and `SI_getFcvTriplet` finds all six of their
///    curves, their values go to the record's +0x30 … +0x44; likewise `AngVel` and
///    `AngAcc` (92, 93) to +0x48 … +0x5c.
/// 7. Up the fathers that have quick stretch too, from the model, while the data's mode is
///    not 1 (with mode 1 one step only): the last element found.
/// 8. `SI_QuickStretchCalculation` twice with (dictionary, element, the node's 16-bit
///    first word, other element, &buffer, the two matrices, the two vectors, the record,
///    null, the word +0x122): the other element is the one found in 7 and then the
///    element itself, the other way round when the mode is not 1. The buffer it may leave
///    is freed.
/// 9. A patch model's surfaces are updated (`SI_patchUpdateSurface`): its first definition
///    and, when different, its current one.
pub fn anim_quick_stretch_update(m: &mut Mem, o: &mut dyn Outside, node: u32, time: f32) {
    let f = Frame(m.local(FRAME));
    m.set_u16(f.at(0x132), 0);
    m.set_u32(f.at(0x120), 0);
    m.set_u32(f.at(0x124), 0);
    let id = read_matrix(m, SI_IDENTITE);
    write_matrix(m, f.at(0x98), &si_math::algebra::matrix_copy(&id));
    let id = read_matrix(m, SI_IDENTITE);
    write_matrix(m, f.at(0xd8), &si_math::algebra::matrix_copy(&id));
    update(m, o, node, time, &f);
    m.release(FRAME);
}

fn update(m: &mut Mem, o: &mut dyn Outside, node: u32, time: f32, f: &Frame) {
    if node == 0 {
        return;
    }
    let model = m.u32(node + 8);
    if model == 0 {
        return;
    }
    let (index, data) = (f.at(0xfc), f.at(0xf8));
    let elem = quick_stretch_get_elem(m, o, model, index, data);
    if elem == 0 {
        return;
    }
    let last_arg = m.u16(m.u32(data) + qs::LAST_ARG) as i16;
    // 2. the parameters' curves, twice
    for pass in 0..2 {
        let mut i = 1i32;
        while i < m.i16(node + 0x18) as i32 {
            let a = m.u32(node + 0x1c).wrapping_add(0x14u32.wrapping_mul(i as u32));
            let curve = m.u32(a + 8);
            if m.i16(a + 0xc) != 0 && m.i16(a) as i32 == m.i32(SI_CID) && curve != 0 {
                let v = curve_value(m, o, curve, time);
                let kind = m.u32(curve);
                let q = m.u32(data);
                if pass == 0 {
                    let k = kind.wrapping_sub(0xf5);
                    if k <= 0xb {
                        m.set_f32(q + qs::scaled(k), v.to_f32());
                        let s = (x(m.f32(q + qs::scaled(k))) * SCALE).to_f32();
                        m.set_f32(q + qs::scaled(k), s);
                    }
                } else {
                    let k = kind.wrapping_sub(0xe9);
                    if k <= 0xb {
                        m.set_f32(q + qs::rate(k), v.to_f32());
                        let r = (x(m.f32(q + qs::rate(k))) / x(m.f32(q + qs::scaled(k))).abs()).to_f32();
                        m.set_f32(q + qs::rate(k), r);
                    }
                }
            }
            i += 1;
        }
    }
    // 3. not before the second frame of a playback
    if m.i16(SI_PLAY_BACK_RUN) != 0 {
        let start = (F80::from_i32(m.i32(SI_FRAME_START)) + TWO).to_f32();
        let whole = x(start).round_int(si_x87::Round::Zero).to_f32();
        if (x(whole) / m.f32(SI_FRAME_RATE)).gt(x(time)) {
            return;
        }
    }
    // 4. a copy of the stored definition to deform
    if m.u16(model + 0x34) != 0 {
        let defs = m.u32(model + 0x38);
        let stored = m.u32(defs.wrapping_add(4 * m.u16(model + 0x36) as u32));
        if m.u32(model + 0x48) == stored {
            let copy = match m.i32(model + 0x30) {
                1 => Some(&out::SI_PATCH_COPY),
                2 => Some(&out::SI_FACE_COPY),
                4 => Some(&out::SI_MESH_COPY),
                6 => Some(&out::SI_SPLINE_COPY),
                9 => Some(&out::SI_NURBS_CRV_COPY),
                10 => Some(&out::SI_NURBS_SRF_COPY),
                _ => None,
            };
            if let Some(r) = copy {
                let c = o.call(m, r, &[stored]).eax;
                m.set_u32(model + 0x48, c);
            }
        }
    }
    // 5. the controls' vectors (first at ebp − 0xc, second at ebp − 0x18)
    let (va, vb) = (f.at(0xc), f.at(0x18));
    for a in [va, vb] {
        for k in 0..3 {
            m.set_u32(a + 4 * k, 0);
        }
    }
    if m.i16(m.u32(data) + qs::MODE) != 0 && m.i16(node + 0x18) > 1 {
        let mut i = 1i32;
        while i < m.i16(node + 0x18) as i32 {
            let a = m.u32(node + 0x1c).wrapping_add(0x14u32.wrapping_mul(i as u32));
            if m.i16(a + 0xc) != 0 && m.i16(a) as i32 == m.i32(SI_CID + 0x20) {
                let ctl = m.u32(a + 8);
                let t = si_math::algebra::matrix_transpose(&read_matrix(m, ctl + 0x178));
                write_matrix(m, f.at(0x58), &t);
                let t = read_matrix(m, f.at(0x58));
                let y = [m.f32(SI_Y_AXIS), m.f32(SI_Y_AXIS + 4), m.f32(SI_Y_AXIS + 8)];
                let target = match m.i32(ctl + 0x2ec) {
                    0x10 => Some(va),
                    0x11 => Some(vb),
                    _ => None,
                };
                if let Some(at) = target {
                    let v = control_vector(&t, y, m.f32(ctl + 0x104));
                    for (k, c) in v.iter().enumerate() {
                        m.set_f32(at + 4 * k as u32, *c);
                    }
                }
            }
            i += 1;
        }
    }
    // 6. the kinematic record
    if m.u32(model + 0x2f0) == 0 {
        let k = kine_state_allocate(m);
        m.set_u32(model + 0x2f0, k);
    }
    let k = m.u32(model + 0x2f0);
    if k == 0 {
        return;
    }
    for (first, second, base) in [(ATTR_LIN_VEL, ATTR_LIN_ACC, kine::LIN_VEL), (ATTR_ANG_VEL, ATTR_ANG_ACC, kine::ANG_VEL)] {
        let (g1, g2) = (f.at(0x100), f.at(0x104));
        let c1 = o.call(m, &out::SI_RELDICT_GET_REL_CHAP_GRP, &[SI_RD, first, g1]).eax as i32;
        let c2 = o.call(m, &out::SI_RELDICT_GET_REL_CHAP_GRP, &[SI_RD, second, g2]).eax as i32;
        if c1 < 0 || c2 < 0 {
            continue;
        }
        let (a0, a1, a2) = (f.at(0x110), f.at(0x10c), f.at(0x108));
        let (b0, b1, b2) = (f.at(0x11c), f.at(0x118), f.at(0x114));
        let gv1 = m.u32(g1);
        o.call(m, &out::SI_GET_FCV_TRIPLET, &[elem, a0, a1, a2, gv1, first]);
        let gv2 = m.u32(g2);
        o.call(m, &out::SI_GET_FCV_TRIPLET, &[elem, b0, b1, b2, gv2, second]);
        let curves = [a0, a1, a2, b0, b1, b2].map(|p| m.u32(p));
        if curves.contains(&0) {
            continue;
        }
        for (j, c) in curves.iter().enumerate() {
            let v = curve_value(m, o, *c, time);
            m.set_f32(k + base + 4 * j as u32, v.to_f32());
        }
    }
    // 7. up the fathers that have quick stretch too
    let mut other = elem;
    let mut upper = model;
    let flag = f.at(0x132);
    while m.u16(flag) == 0 {
        let father = m.u32(upper + 0x2bc);
        if father == 0 {
            break;
        }
        let r = quick_stretch_get_elem(m, o, father, index, f.at(0x120));
        if r == 0 || m.u32(f.at(0x120)) == 0 {
            break;
        }
        other = r;
        if m.i16(m.u32(data) + qs::MODE) == 1 {
            m.set_u16(flag, 1);
        }
        upper = m.u32(r + 4);
    }
    // 8. the deformation, twice
    let buf = f.at(0x124);
    let id16 = m.i16(node) as i32 as u32;
    let mode1 = m.i16(m.u32(data) + qs::MODE) == 1;
    let (first, second) = if mode1 { (other, elem) } else { (elem, other) };
    for x_elem in [first, second] {
        let d = cur_dict(m);
        super::qscalc::quick_stretch_calculation(m, o, d, elem, id16 as i32, x_elem, buf, f.at(0x98), f.at(0xd8), va, vb, k, 0, last_arg as i16);
    }
    let b = m.u32(buf);
    if b != 0 {
        m.free(b);
        m.set_u32(buf, 0);
    }
    m.set_u32(buf, 0);
    // 9. a patch's surfaces
    if m.i32(model + 0x30) == 1 {
        let defs = m.u32(model + 0x38);
        let first_def = m.u32(defs);
        o.call(m, &out::SI_PATCH_UPDATE_SURFACE, &[first_def]);
        let defs = m.u32(model + 0x38);
        let cur = m.u32(model + 0x48);
        if cur != m.u32(defs.wrapping_add(4 * m.u16(model + 0x36) as u32)) {
            o.call(m, &out::SI_PATCH_UPDATE_SURFACE, &[cur]);
        }
    }
}

fn read_matrix(m: &Mem, a: u32) -> si_math::types::Matrix {
    let mut r = [[0.0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = f32::from_bits(m.u32(a + 16 * i as u32 + 4 * j as u32));
        }
    }
    r
}

fn write_matrix(m: &mut Mem, a: u32, v: &si_math::types::Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_u32(a + 16 * i as u32 + 4 * j as u32, e.to_bits());
        }
    }
}
