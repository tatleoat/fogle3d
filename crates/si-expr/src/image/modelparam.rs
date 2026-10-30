// SPDX-License-Identifier: MIT
//! A model node's parameters: `SI_modelGetParam` and `SI_modelSetParam` of `edparams.c` in full (finding 0013, section 20).
//!
//! Both are a switch on the fcurve type of the parameter. Of the stored object they reach the node itself (scaling, rotation,
//! translation, visibility and matte flags, the lattice interpolation, the dynamics record at +0x3f0 ...), its definition
//! (+0x48: a metaball's parameters), its fittings (+0x60, +0x64 splines/patches, +0x2d8, +0x2dc patch fittings), the
//! quick-stretch record, the records at +0x2f0, +0x2f4 and +0x304, and the curve the parameter hangs on.
//!
//! Left to the outside world are the routines that other crates own and that this one cannot depend on without a cycle
//! (`si-dynamics`' geometry update) or that are not wired yet (the fitting matrices, the metaball primitive, the node update,
//! the quick-stretch record, the attribute dictionary's access mode), see [`ExprCalls`]. The parameters that
//! `SI_edSumatraTo3x` selects (types 0x10b, 0x237 and 0x29e of the reader; the hidden/matte readers' other branches) are those of
//! a product the render does not run: the global is 0 on the full product's path and they are left out.
use super::calls::{DictOutside, ExprCalls};
use super::elem::{current_time, ed_element_static_value_get, fcurve, global_angles, model, model_root, model_set_hidden_param, DEG_TO_RAD, RAD_TO_DEG};
use super::eval::get_attr_val_in_float;
use super::params::{curve_static_set, drop_stale_static, fcopy, fscaled, get_attr_val_in_boolean, model_propagate_matte_param, start_value};
use super::rec::{self, cid};
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{AttGrpIndex, AttributeValue, ElemIdentity};
use si_x87::{Round, F80};

/// `SI_useHiddenForMatte` (0x0867967c, a short): a model's matte reads and writes its display state instead of the graf node's.
const SI_USE_HIDDEN_FOR_MATTE: u32 = 0x0867_967c;
/// `SI_dblIdentity` (0x086d3940): the 4 x 4 identity of doubles `VCCreatePrim` is given.
const SI_DBL_IDENTITY: u32 = 0x086d_3940;

/// Fields of the node the two routines reach beyond [`model`].
mod nf {
    pub const LATTICE_LIST: u32 = 0x50;
    pub const LATTICE_BRANCH_LIST: u32 = 0x58;
    pub const SPLINE_FIT: u32 = 0x60;
    pub const PATCH_FIT: u32 = 0x64;
    pub const PFIT_A: u32 = 0x2d8;
    pub const PFIT_B: u32 = 0x2dc;
    pub const REC_2F0: u32 = 0x2f0;
    pub const REC_2F4: u32 = 0x2f4;
    pub const REC_304: u32 = 0x304;
    pub const MATTE: u32 = 0x3f4;
    pub const FLAGS_3EC: u32 = 0x3ec;
    pub const U16_3F8: u32 = 0x3f8;
    pub const FLAGS_404: u32 = 0x404;
    pub const FLAGS_406: u32 = 0x406;
    pub const U16_420: u32 = 0x420;
}

/// Where a fitting parameter (types 0x6b to 0x8a) lives: the node's field holding the fitting, the offset in it and whether
/// the value is an angle (degrees outside, radians inside).
fn fitting_field(t: i32) -> (u32, u32, bool) {
    match t {
        0x6b..=0x71 | 0x72..=0x78 => {
            let (obj, k) = if t <= 0x71 { (nf::SPLINE_FIT, t - 0x6b) } else { (nf::PATCH_FIT, t - 0x72) };
            let off = [0xc, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24][k as usize];
            (obj, off, k == 3)
        }
        0x79..=0x81 => {
            let k = (t - 0x79) as usize;
            (nf::PFIT_A, [8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28][k], (3..=5).contains(&k))
        }
        _ => {
            let k = (t - 0x82) as usize;
            (nf::PFIT_B, [8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x28, 0x28, 0x28][k], (3..=5).contains(&k))
        }
    }
}

/// One parameter of a model node into a value record [soft 0x080f0c3c SI_modelGetParam]: 1 when answered, 0 when the
/// node lacks what the parameter lives in.
///
/// The record starts as the integer-typed zero ("type 1, value 0"); a curve's static value is dropped when the current time
/// lies further than 0.0001 from its time. Then by type: scaling and translation are the local fields, or with `coord` 1
/// the global ones; a rotation is the local angle, or with `coord` 1 the angle extracted from the global matrix starting
/// from the local ones, in degrees; the curve-only types (7, 8, 0x42, 0x43, 0x9a, 0xb7, 0xb8) the curve's value at the
/// current frame, or its static value; a fitting parameter (0x6b to 0x8a) a field of the fitting - the curve's static value
/// when it holds one, with no look at the fitting; 0xb3 to 0xb5, 0x15d to 0x162 fields of the two small records
/// (+0x2f4, +0x304), 0xb6 and the flag types (0x21a, 0x29b to 0x29d, 0x2a4) a 16-bit integer (record type 2), 0xb9 to 0xbc
/// the metaball's doubles, 0xdd to 0xe8 floats of the record at +0x2f0, 0xe9 to 0x100 the quick stretch's (`|a| * b`, and
/// `a / 0.005`), 0xaf and 0x111 the matte state (0 to 3), 0x173 and 0x174 the matte value, 0x26c, 0x277 to 0x279 flag bits
/// as 1 or 0. A node of skeleton kind 3 (an effector) reads its translation as the global one less its root's local
/// translation, unless it is its own root.
pub fn model_get_param(m: &mut Mem, o: &mut dyn Outside, t: i32, mdl: u32, fcv: u32, out: Ptr32<AttributeValue>, coord: i32) -> i16 {
    let out = out.addr();
    start_value(m, out);
    drop_stale_static(m, fcv);
    let mut q = 0u32;
    match t {
        7 | 8 | 0x42 | 0x43 | 0x9a | 0xb7 | 0xb8 if fcv == 0 => return 0,
        0xb6 if m.u32(mdl + nf::REC_304) == 0 => return 0,
        0x15d | 0x160..=0x162 if m.u32(mdl + nf::REC_304) == 0 && m.u32(mdl + nf::REC_2F4) == 0 => return 0,
        0xb3..=0xb5 | 0x15e | 0x15f if m.u32(mdl + nf::REC_2F4) == 0 => return 0,
        0xb9..=0xbc if m.i32(mdl + model::KIND) != 8 => return 0,
        0xdd..=0xe8 if m.u32(mdl + nf::REC_2F0) == 0 => return 0,
        0xe9..=0x100 => {
            let data = m.local(4);
            let idx = m.local(4);
            let r = o.quick_stretch_get_elem(m, mdl, idx, data);
            q = m.u32(data);
            m.release(8);
            if r == 0 || q == 0 {
                return 0;
            }
        }
        0x6b..=0x8a => {
            if fcv != 0 && m.i16(fcv + fcurve::STATIC) != 0 {
                fcopy(m, out + 4, fcv + fcurve::LAST_VALUE);
                return 1;
            }
            if m.u32(mdl + fitting_field(t).0) == 0 {
                return 0;
            }
        }
        _ => {}
    }
    let deg = |v: f32| (F80::from_f32(v) * F80::from_f64(RAD_TO_DEG)).to_f32();
    match t {
        1..=3 => {
            let k = (t - 1) as u32 * 4;
            fcopy(m, out + 4, mdl + if coord == 1 { model::GSCAL } else { model::LSCAL } + k);
        }
        4..=6 => {
            let k = (t - 4) as usize;
            let v = if coord == 1 {
                let l = m.local(12);
                for i in 0..3 {
                    let w = m.u32(mdl + model::LROT + 4 * i);
                    m.set_u32(l + 4 * i, w);
                }
                let a = global_angles(m, mdl, l);
                m.release(12);
                deg(a[k])
            } else {
                deg(m.f32(mdl + model::LROT + 4 * k as u32))
            };
            m.set_f32(out + 4, v);
        }
        0x8b..=0x8d => {
            let k = (t - 0x8b) as u32 * 4;
            fcopy(m, out + 4, mdl + if coord == 1 { model::GTRANS } else { model::LTRANS } + k);
        }
        7 | 8 | 0x42 | 0x43 | 0x9a | 0xb7 | 0xb8 => {
            super::params::curve_value_out(m, fcv, out);
        }
        0x6b..=0x8a => {
            let (f, off, angle) = fitting_field(t);
            let obj = m.u32(mdl + f);
            if angle {
                fscaled(m, out + 4, obj + off, RAD_TO_DEG);
            } else {
                fcopy(m, out + 4, obj + off);
            }
        }
        0xb3..=0xb5 => {
            let off = [0x14, 0xc, 0x10][(t - 0xb3) as usize];
            fcopy(m, out + 4, m.u32(mdl + nf::REC_2F4) + off);
        }
        0xb6 => {
            m.set_i32(out, 2);
            let w = m.u16(m.u32(mdl + nf::REC_304) + 0x38);
            m.set_u16(out + 4, w);
        }
        0xb9..=0xbc => {
            let off = [0xd4, 0xdc, 0xe4, 0xec][(t - 0xb9) as usize];
            let v = F80::from_f64(m.f64(m.u32(mdl + model::DEFINITION) + off)).to_f32();
            m.set_f32(out + 4, v);
        }
        0xdd..=0xe8 => fcopy(m, out + 4, m.u32(mdl + nf::REC_2F0) + 0x30 + 4 * (t - 0xdd) as u32),
        0xe9..=0xf4 => {
            let i = (t - 0xe9) as u32;
            let a = F80::from_f32(m.f32(q + 0xc + 0x18 * i)).abs();
            let v = (a * F80::from_f32(m.f32(q + 0x10 + 0x18 * i))).to_f32();
            m.set_f32(out + 4, v);
        }
        0xf5..=0x100 => {
            let i = (t - 0xf5) as u32;
            let v = (F80::from_f32(m.f32(q + 0xc + 0x18 * i)) / F80::from_f64(0.005)).to_f32();
            m.set_f32(out + 4, v);
        }
        0xaf | 0x111 => {
            if m.i16(SI_USE_HIDDEN_FOR_MATTE) != 0 {
                let ed = rec::cur_dict(m);
                let chap = rec::chap(m, cid::MODEL);
                let idx = si_dict::elemdict::find_elem_by_data(m, &mut DictOutside(o), ed, chap, Ptr32::new(mdl));
                if idx != -1 {
                    let e = si_dict::elemdict::get_elem(m, ed, chap, idx).addr();
                    if e != 0 {
                        let flags = m.u32(e + 0x20);
                        let v: f32 = if flags & 2 == 0 {
                            0.0
                        } else if flags & 0x80 != 0 {
                            2.0
                        } else if flags & 0x100 != 0 {
                            3.0
                        } else {
                            1.0
                        };
                        m.set_f32(out + 4, v);
                    }
                }
            } else {
                let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
                let gl = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(mdl), 2).addr();
                if gl != 0 {
                    m.set_f32(out + 4, if m.i32(gl + 0xc) == 2 { 1.0 } else { 0.0 });
                }
            }
        }
        0x15d => {
            let q304 = m.u32(mdl + nf::REC_304);
            let src = if q304 != 0 { q304 + 0x60 } else { m.u32(mdl + nf::REC_2F4) + 8 };
            fcopy(m, out + 4, src);
        }
        0x15e => fcopy(m, out + 4, m.u32(mdl + nf::REC_2F4)),
        0x15f => fcopy(m, out + 4, m.u32(mdl + nf::REC_2F4) + 4),
        0x160..=0x162 => {
            let k = (t - 0x160) as u32;
            let q304 = m.u32(mdl + nf::REC_304);
            let src = if q304 != 0 { q304 + 0x64 + 4 * k } else { m.u32(mdl + nf::REC_2F4) + [0x20, 0x24, 0x28][k as usize] };
            fcopy(m, out + 4, src);
        }
        0x173 | 0x174 => fcopy(m, out + 4, mdl + nf::MATTE),
        0x21a => {
            m.set_i32(out, 2);
            let w = m.u16(mdl + nf::U16_3F8);
            m.set_u16(out + 4, w);
        }
        0x26c | 0x277 | 0x278 | 0x279 => {
            let (a, bit) = match t {
                0x26c => (nf::FLAGS_404, 1),
                0x277 => (nf::FLAGS_404, 2),
                0x278 => (nf::FLAGS_406, 1),
                _ => (nf::FLAGS_406, 2),
            };
            m.set_f32(out + 4, if m.u8(mdl + a) & bit != 0 { 1.0 } else { 0.0 });
        }
        0x29b..=0x29d => {
            m.set_i32(out, 2);
            let w = m.u16(mdl + nf::FLAGS_3EC) & (1 << (t - 0x29b));
            m.set_u16(out + 4, w);
        }
        0x2a4 => {
            m.set_i32(out, 2);
            let w = m.u16(mdl + nf::U16_420);
            m.set_u16(out + 4, w);
        }
        // the three the Sumatra product answers (0x10b, 0x237, 0x29e) are left out: nothing is stored
        0x10b | 0x237 | 0x29e => {}
        _ => return 0,
    }
    // SI_edSumatraTo3x is 0 on the full product's path
    if m.i32(mdl + model::SKE_KIND) == 3 && matches!(t, 0x8b..=0x8d) {
        let k = (t - 0x8b) as u32 * 4;
        fcopy(m, out + 4, mdl + model::GTRANS + k);
        let g = m.f32(out + 4);
        let root = model_root(m, mdl);
        if root != 0 && root != mdl {
            let v = (F80::from_f32(g) - F80::from_f32(m.f32(root + model::LTRANS + k))).to_f32();
            m.set_f32(out + 4, v);
        }
    }
    1
}

/// One parameter of a model node from a value record [soft 0x080f3d1c SI_modelSetParam]: 1 when stored, 0 when the node
/// lacks what the parameter lives in or the type is not settable.
///
/// A null model answers 0. The translation of an effector (skeleton kind 3) or of a node of kind 5 that has an
/// inverse-kinematics record (+0x318) goes to that record's goal (+8, +0xc, +0x10, first filled from the global translation
/// unless +0x38 is set), plus the root's local translation with `coord` 0 when the node is not its own root, and +0x38 is
/// set. The fitting types (0x6b to 0x8a) first store the value as the curve's static value (value, set flag, current time)
/// and answer 0 without the fitting. Scaling and translation are stored locally, or with `coord` 1 set globally
/// (`SI_modelSetGlobalScaling`, `..Translation`); a rotation as degrees times 0.0174532925 into the local angle, or with
/// `coord` 1 into the angles extracted from the global matrix, which are then set globally; a joint of a chain (kind 5,
/// skeleton kind 1) then has its preferred angle reset on that axis. The matte types (0xaf, 0x111) set the node's display
/// state (`SI_modelSetHiddenParam`, branch for 0x111) and with the hidden-for-matte option the element's display bits.
/// After the store the dependents are updated: a fitting's matrix, the path translation (type 7), the metaball (0xb9 to
/// 0xbc, 0xb9 through a throw-away primitive that carries the radius over), the whole node (0x128, 0x129), the dynamics
/// geometry (0x15e). Types without a store answer 0.
pub fn model_set_param(m: &mut Mem, o: &mut dyn Outside, t: i32, mdl: u32, fcv: u32, value: Ptr32<AttributeValue>, coord: i32) -> i32 {
    let value = value.addr();
    // SI_frameToSec(SI_frameCurrent), stored to a float first
    let now = current_time(m).to_f32();
    if mdl == 0 {
        return 0;
    }
    let ik = m.u32(mdl + model::IK_GOAL);
    if (m.i32(mdl + model::SKE_KIND) == 3 || m.i32(mdl + model::KIND) == 5) && ik != 0 && matches!(t, 0x8b..=0x8d) {
        let root = model_root(m, mdl);
        if m.i32(ik + 0x38) == 0 {
            for i in 0..3 {
                let w = m.u32(mdl + model::GTRANS + 4 * i);
                m.set_u32(ik + 8 + 4 * i, w);
            }
        }
        let k = (t - 0x8b) as u32 * 4;
        let v = get_attr_val_in_float(m, Ptr32::new(value)).to_f32();
        m.set_f32(ik + 8 + k, v);
        if coord == 0 && root != 0 && root != mdl {
            let s = (F80::from_f32(m.f32(ik + 8 + k)) + F80::from_f32(m.f32(root + model::LTRANS + k))).to_f32();
            m.set_f32(ik + 8 + k, s);
        }
        m.set_i32(ik + 0x38, 1);
        return 1;
    }
    let attr = |m: &Mem| get_attr_val_in_float(m, Ptr32::new(value));
    // the generic store: the value as the curve's static value now
    let store = |m: &mut Mem| {
        if fcv != 0 {
            curve_static_set(m, fcv, value, now);
        }
    };
    let rad = |m: &Mem| (F80::from_f64(DEG_TO_RAD) * attr(m)).to_f32();
    match t {
        0x6b..=0x8a => {
            store(m);
            let (f, off, angle) = fitting_field(t);
            let obj = m.u32(mdl + f);
            if obj == 0 {
                return 0;
            }
            if angle {
                fscaled(m, obj + off, value + 4, DEG_TO_RAD);
            } else {
                fcopy(m, obj + off, value + 4);
            }
        }
        0xb3..=0xb5 | 0x15e if m.u32(mdl + nf::REC_2F4) == 0 => return 0,
        0xb6 if m.u32(mdl + nf::REC_304) == 0 => return 0,
        0xb9..=0xbc if m.i32(mdl + model::KIND) != 8 => return 0,
        0xdd..=0xe8 => return 0,
        1..=3 | 0x8b..=0x8d => {
            let (local, global, k) = if t <= 3 {
                (model::LSCAL, model::GSCAL, (t - 1) as u32)
            } else {
                (model::LTRANS, model::GTRANS, (t - 0x8b) as u32)
            };
            if coord != 1 {
                let v = attr(m).to_f32();
                m.set_f32(mdl + local + 4 * k, v);
            } else {
                let l = m.local(12);
                for i in 0..3u32 {
                    let v = if i == k { attr(m).to_f32() } else { m.f32(mdl + global + 4 * i) };
                    m.set_f32(l + 4 * i, v);
                }
                if t <= 3 {
                    o.model_set_global_scaling(m, mdl, l, 1, 0);
                } else {
                    o.model_set_global_translation(m, mdl, l, 1, 0);
                }
                m.release(12);
            }
        }
        4..=6 => {
            let k = (t - 4) as u32;
            if coord == 1 {
                let a = global_angles(m, mdl, mdl + model::LROT);
                for (i, x) in a.iter().enumerate() {
                    m.set_f32(mdl + model::LROT + 4 * i as u32, *x);
                }
                let v = rad(m);
                m.set_f32(mdl + model::LROT + 4 * k, v);
                o.model_set_global_rotation(m, mdl, mdl + model::LROT, 1, 0);
            } else {
                let v = rad(m);
                m.set_f32(mdl + model::LROT + 4 * k, v);
            }
            if m.i32(mdl + model::KIND) == 5 && m.i32(mdl + model::SKE_KIND) == 1 {
                let w = [m.u32(mdl + model::LROT), m.u32(mdl + model::LROT + 4), m.u32(mdl + model::LROT + 8)];
                o.inv_kin_reset_pref_angle(m, mdl, k as i32 + 1, w);
            }
        }
        7 => {
            if fcv == 0 {
                return 0;
            }
            store(m);
            let chap = m.u32(rec::cid::MODEL);
            o.set_path_translation(m, 0x70, chap, mdl, fcv);
        }
        8 | 0x9a | 0xb7 | 0xb8 => store(m),
        0x42 | 0x43 => {
            let list = m.u32(mdl + if t == 0x42 { nf::LATTICE_LIST } else { nf::LATTICE_BRANCH_LIST });
            let first = m.u32(list);
            if first != 0 && fcv != 0 {
                fcopy(m, first + 0x40, value + 4);
                store(m);
            }
        }
        0xaf | 0x111 => {
            let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
            let gl = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(mdl), 2).addr();
            if gl == 0 {
                return 1;
            }
            let ed = rec::cur_dict(m);
            let chap = rec::chap(m, cid::MODEL);
            let idx = si_dict::elemdict::find_elem_by_data(m, &mut DictOutside(o), ed, chap, Ptr32::new(mdl));
            if idx == -1 {
                return 1;
            }
            let e = si_dict::elemdict::get_elem(m, ed, chap, idx).addr();
            if e == 0 {
                return 1;
            }
            let hidden_opt = m.i16(SI_USE_HIDDEN_FOR_MATTE) != 0;
            let state = if hidden_opt {
                if attr(m).lt(F80::from_f64(0.5)) { 0 } else { 2 }
            } else if attr(m).compare(F80::from_f64(1.0)) == Some(core::cmp::Ordering::Equal) {
                2
            } else {
                0
            };
            model_set_hidden_param(m, o, gl, mdl, e, state, (t == 0x111) as i16);
            if hidden_opt {
                let bits = m.u32(e + 0x20);
                let n = if attr(m).lt(F80::from_f64(1.5)) {
                    bits & 0xffff_fe7f
                } else if attr(m).lt(F80::from_f64(2.5)) {
                    (bits | 0x80) & !0x100
                } else {
                    (bits & !0x80) | 0x100
                };
                m.set_u32(e + 0x20, n);
            }
        }
        0xb3..=0xb5 => {
            let off = [0x14, 0xc, 0x10][(t - 0xb3) as usize];
            fcopy(m, m.u32(mdl + nf::REC_2F4) + off, value + 4);
        }
        0xb6 => {
            let w = m.u16(value + 4);
            m.set_u16(m.u32(mdl + nf::REC_304) + 0x38, w);
        }
        0xb9..=0xbc => {
            // the attribute dictionary may declare the parameter read-only
            let idx = m.local(0x14);
            let mode = m.local(4);
            let got = o.attdict_get_grp_index_from_fcurve(m, SI_ATD.addr(), 0xb9, idx);
            if got != 0 {
                let ok = o.attdict_attribute_get_access_mode(m, SI_ATD.addr(), idx, mode);
                if ok != 0 && m.i16(mode) & 2 == 0 {
                    m.release(0x18);
                    return 0;
                }
            }
            m.release(0x18);
            let off = [0xd4, 0xdc, 0xe4, 0xec][(t - 0xb9) as usize];
            let v = F80::from_f32(m.f32(value + 4)).to_f64();
            m.set_f64(m.u32(mdl + model::DEFINITION) + off, v);
        }
        0x128 | 0x129 => {
            let d = m.u32(mdl + model::DEFINITION);
            if d != 0 {
                // `fistp` to a 32-bit integer (truncating), the low 16 bits stored
                let v = F80::from_f32(m.f32(value + 4)).to_i32(Round::Zero);
                m.set_i16(d + if t == 0x128 { 0x1c } else { 0x1e }, v as i16);
            }
        }
        0x15e => fcopy(m, m.u32(mdl + nf::REC_2F4), value + 4),
        0x173 => fcopy(m, mdl + nf::MATTE, value + 4),
        0x174 => model_propagate_matte_param(m, mdl, m.f32(value + 4)),
        0x21a => {
            let w = m.u16(value + 4);
            m.set_u16(mdl + nf::U16_3F8, w);
        }
        0x26c | 0x277 | 0x278 | 0x279 => {
            let (a, bit) = match t {
                0x26c => (nf::FLAGS_404, 1u8),
                0x277 => (nf::FLAGS_404, 2),
                0x278 => (nf::FLAGS_406, 1),
                _ => (nf::FLAGS_406, 2),
            };
            let b = m.u8(mdl + a);
            let set = attr(m).compare(F80::from_f64(1.0)) == Some(core::cmp::Ordering::Equal);
            m.set_u8(mdl + a, if set { b | bit } else { b & !bit });
        }
        0x2a4 => {
            let b = get_attr_val_in_boolean(m, value) == 1;
            m.set_u16(mdl + nf::U16_420, b as u16);
            store(m);
        }
        _ => return 0,
    }
    match t {
        0x82..=0x8a => o.pfitting_update_matrix(m, m.u32(mdl + nf::PFIT_B)),
        0x79..=0x81 => o.pfitting_update_matrix(m, m.u32(mdl + nf::PFIT_A)),
        0x72..=0x78 => o.fitting_update_matrix(m, m.u32(mdl + nf::PATCH_FIT)),
        0x6b..=0x71 => o.fitting_update_matrix(m, m.u32(mdl + nf::SPLINE_FIT)),
        0xb9 => {
            let def = m.u32(mdl + model::DEFINITION);
            let h = m.local(4);
            o.vc_create_prim(m, 0, SI_DBL_IDENTITY, h);
            let handle = m.i32(h);
            o.vc_set_prim_param(m, handle, 2, def + 0xd4);
            o.vc_get_prim_param(m, handle, 1, def + 0xcc);
            let handle = m.i32(h);
            o.vc_delete_prim(m, handle);
            m.release(4);
            let def = m.u32(mdl + model::DEFINITION);
            o.metaball_touch(m, def, 0);
        }
        0xba..=0xbc => {
            let def = m.u32(mdl + model::DEFINITION);
            o.metaball_touch(m, def, 0);
        }
        0x128 | 0x129 => o.model_update_all(m, mdl, 1, 1),
        0x15e => o.dyna_geometry_update(m, mdl),
        _ => {}
    }
    1
}

/// An attribute of the group `General_Model` read from a model's element [soft 0x080f6dd0 SI_modelAttrGetValue]: 1 when
/// `value` was filled (0 for the matte state of a model without a graf node), else 0.
///
/// The element (`SI_elemdictGetElem` in the current dictionary) must exist and be in use (+0x48 zero), the attribute
/// (`SI_attdictAttributeGetByIndex(SI_ATD, index)`) exist, `value` be given, the element have its object (+4: the model)
/// and the component asked for (`index` + 0x10) be one of the attribute's. Then `value` + 0 takes the attribute's value type and
/// the component's parameter id says what follows, with `value` + 4 read as the coordinate system (1: global) where the id
/// has two:
///
/// | id | value record |
/// |---|---|
/// | 1 to 3, 0x8b to 0x8d | type 9, +8 the scaling / translation, global or local (the words) |
/// | 4 to 6 | type 9, +8 the rotation in degrees (x87 product with 57.29577951308232); global: extracted from the global matrix starting from the local angles |
/// | 0xaf, 0x111 | type 2, +4 (16 bits) the matte state 0 to 3 as `SI_modelGetParam` has it (the option reads the element's display bits, else the graf node's display type 2 gives 1) |
/// | 0xb2, 0x110 | +4 the attribute's enum type, +8 the node's skeleton kind (+0x2ec) or kind (+0x30) |
/// | 0x173, 0x174 | type 1, +4 the matte value (+0x3f4) |
/// | 0x21a | type 2, +4 the 16 bits at +0x3f8 |
/// | 0x26c, 0x277, 0x278, 0x279 | type 2, +4 bit 0 / 1 of the 16 bits at +0x404 / +0x406 |
///
/// Any other id is read like a parameter, through `SI_edElementStaticValueGet`. The Sumatra branch of the matte state is left out
/// (`SI_edSumatraTo3x` is 0 on the full product's path).
pub fn model_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    // the local the generic read gets as its access record: the word 1, two more
    let access = m.local(12);
    m.set_u32(access, 1);
    let r = attr_get_body(m, o, ident, index, value, access);
    m.release(12);
    r
}

fn attr_get_body(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>, access: u32) -> i32 {
    let ed = rec::cur_dict(m);
    let (chap, id) = (m.i32(ident.addr()), m.i32(ident.addr().wrapping_add(4)));
    let e = si_dict::elemdict::get_elem(m, ed, chap, id).addr();
    if e == 0 || m.i16(e + 0x48) != 0 {
        return 0;
    }
    let a = si_dict::attdict::attribute_get_by_index(m, SI_ATD, index).addr();
    let v = value.addr();
    if a == 0 || v == 0 {
        return 0;
    }
    let mdl = m.u32(e + 4);
    let k = m.i32(index.addr().wrapping_add(0x10));
    if mdl == 0 || k < 0 || k >= m.i16(a + 0x56) as i32 {
        return 0;
    }
    m.set_i32(v, m.i32(a + 0x2c));
    let k = m.i32(index.addr().wrapping_add(0x10));
    let fid = m.i16(m.u32(a + 0x58).wrapping_add(2 * k as u32)) as i32;
    let global = |m: &Mem| m.i32(v + 4) == 1;
    let copy = |m: &mut Mem, g: u32, l: u32| {
        m.set_i32(v, 9);
        let src = if global(m) { g } else { l };
        let w = m.u32(mdl + src);
        m.set_u32(v + 8, w);
    };
    let angle = |m: &mut Mem, k: usize| {
        m.set_i32(v, 9);
        let x = if global(m) {
            let l = m.local(12);
            for i in 0..3 {
                let w = m.u32(mdl + model::LROT + 4 * i);
                m.set_u32(l + 4 * i, w);
            }
            let r = global_angles(m, mdl, l);
            m.release(12);
            r[k]
        } else {
            m.f32(mdl + model::LROT + 4 * k as u32)
        };
        let d = (F80::from_f32(x) * F80::from_f64(RAD_TO_DEG)).to_f32();
        m.set_f32(v + 8, d);
    };
    match fid {
        1 => copy(m, model::GSCAL, model::LSCAL),
        2 => copy(m, model::GSCAL + 4, model::LSCAL + 4),
        3 => copy(m, model::GSCAL + 8, model::LSCAL + 8),
        0x8b => copy(m, model::GTRANS, model::LTRANS),
        0x8c => copy(m, model::GTRANS + 4, model::LTRANS + 4),
        0x8d => copy(m, model::GTRANS + 8, model::LTRANS + 8),
        4 => angle(m, 0),
        5 => angle(m, 1),
        6 => angle(m, 2),
        0xaf | 0x111 => {
            m.set_i32(v, 2);
            let state: u16 = if m.i16(SI_USE_HIDDEN_FOR_MATTE) == 0 {
                let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
                let gl = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(mdl), 2).addr();
                if gl == 0 {
                    return 0;
                }
                (m.i32(gl + 0xc) == 2) as u16
            } else {
                let flags = m.u32(e + 0x20);
                if flags & 2 == 0 {
                    0
                } else if flags & 0x80 != 0 {
                    2
                } else if flags & 0x100 != 0 {
                    3
                } else {
                    1
                }
            };
            m.set_u16(v + 4, state);
        }
        0x173 | 0x174 => {
            m.set_i32(v, 1);
            let w = m.u32(mdl + nf::MATTE);
            m.set_u32(v + 4, w);
        }
        0x21a => {
            m.set_i32(v, 2);
            let w = m.u16(mdl + nf::U16_3F8);
            m.set_u16(v + 4, w);
        }
        0x26c | 0x277 | 0x278 | 0x279 => {
            m.set_i32(v, 2);
            let (off, mask) = match fid {
                0x26c => (nf::FLAGS_404, 1),
                0x277 => (nf::FLAGS_404, 2),
                0x278 => (nf::FLAGS_406, 1),
                _ => (nf::FLAGS_406, 2),
            };
            let w = m.u16(mdl + off) & mask;
            m.set_u16(v + 4, w);
        }
        0x110 | 0xb2 => {
            let en = m.i16(a + 0x30) as i32;
            m.set_i32(v + 4, en);
            let w = m.u32(mdl + if fid == 0x110 { model::KIND } else { model::SKE_KIND });
            m.set_u32(v + 8, w);
        }
        _ => {
            let r = ed_element_static_value_get(m, o, Ptr32::new(e), chap, fid, Ptr32::new(access), value, 0);
            return r as i16 as i32;
        }
    }
    1
}
