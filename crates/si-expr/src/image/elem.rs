// SPDX-License-Identifier: MIT
//! An expression's way into the parameters of elements: the parts of `edanim.c` and
//! `edparams.c` the evaluator reaches (finding 0013, section 12).
//!
//! The evaluator reads a parameter with `SI_edElementStaticValueGet` and writes one with
//! `SI_edElementStaticValueSet`; both find the parameter's curve first
//! (`SI_edElementStaticFcvGet`) and hand the chapter's object to the chapter's parameter
//! routine (`SI_modelGetParam`, `SI_camGetParam` …). The old value of an assignment's
//! target is read through the attribute dictionary (`SI_edElementAttValueGet`).
//!
//! Of the chapter routines only `SI_modelGetParam` and `SI_modelSetParam` are here, and
//! only for scaling, rotation and explicit translation (fcurve types 1 to 6 and 139 to
//! 141), the parameters every stored expression of the sample corpus drives or reads.
//! Every other type is handed to the routine itself through the outside world.
//!
//! Also two small routines of `edparams.c` and `attrib.c` the render's update reaches: a
//! model's visibility (`SI_modelSetHiddenParam`, the update's relations 10 and 12) and the
//! reading routine of the attribute group `General_Element` (`SI_generalAttrGetValue`,
//! called by its address from [`ed_element_att_value_get`] and the dictionary's
//! conditions).

use super::calls::{DictOutside, ExprCalls};
use super::params;
use super::rec::{self, cid};
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{AttGrpIndex, AttributeValue, Elem, ElemIdentity};
use si_x87::F80;

/// Fcurve type of a custom effect's parameter.
const FCV_CUSTOM: i32 = 0x236;

/// The fcurve record's fields the parameter routines touch (finding 0009): the time and
/// value of a static value set while the curve is not evaluated, and its flag.
pub(super) mod fcurve {
    pub const KIND: u32 = 0x00;
    pub const LAST_TIME: u32 = 0x10;
    pub const LAST_VALUE: u32 = 0x14;
    pub const ACTIVE: u32 = 0x20;
    pub const CACHE: u32 = 0x50;
    pub const STATIC: u32 = 0x60;
}

/// `__SI_Model` fields read or written here (finding 0015).
pub(super) mod model {
    pub const KIND: u32 = 0x30;
    pub const DEFINITION: u32 = 0x48;
    pub const LSCAL: u32 = 0x100;
    pub const LROT: u32 = 0x10c;
    pub const LTRANS: u32 = 0x158;
    pub const GSCAL: u32 = 0x16c;
    pub const GROT: u32 = 0x178;
    pub const GTRANS: u32 = 0x1b8;
    pub const FATHER: u32 = 0x2bc;
    pub const SKE_KIND: u32 = 0x2ec;
    pub const IK_GOAL: u32 = 0x318;
    pub const VISIBLE: u32 = 0x2b8;
}

/// `__SI_Elem` fields (si-types `dict::Elem`) and those of its name record
/// (`SI_DbListNode`) and graf node (`__SI_GrafNode`) read here.
mod el {
    pub const NODE: u32 = 0x00;
    pub const DATA: u32 = 0x04;
    pub const AUX_DATA: u32 = 0x08;
    pub const WIRE_COLOR: u32 = 0x1c;
    pub const WIRE_TYPE: u32 = 0x20;
    pub const SELECTED: u32 = 0x42;
    pub const STATUS: u32 = 0x48;
    pub const HIER_LEVEL: u32 = 0x54;
    pub const NODE_PREFIX: u32 = 0x10;
    pub const NODE_NAME: u32 = 0x14;
    pub const NODE_VERSION: u32 = 0x18;
    pub const NODE_SUBST: u32 = 0x1c;
    pub const GN_SELECTION: u32 = 0x08;
    pub const GN_WIRE_TYPE: u32 = 0x0c;
    pub const GN_WIRE_COLOR: u32 = 0x12;
    pub const GN_FLAGS: u32 = 0x16;
}

/// `SI_Attribute` fields (si-types `dict::Attribute`): the value type, the enum type, the
/// number of components and their parameter ids.
mod attr {
    pub const VALUE_TYPE: u32 = 0x2c;
    pub const ENUM_TYPE: u32 = 0x30;
    pub const NB_VALUES: u32 = 0x56;
    pub const FCV_IDS: u32 = 0x58;
}

/// The short at 0x08781580 (in `SI_glbManip`) that `SI_matrixExtractXyzRotation` reads:
/// keep angles within π of the previous ones.
const KEEP_TURNS: u32 = 0x0878_1580;

/// 0.0001 (double, 0x08646320): how far the current time may lie from a curve's static
/// value before the value is dropped.
pub(super) const STATIC_EPS: f64 = 0.0001;
/// 57.29577951308232 (double, 0x08646328): radians to degrees.
pub(super) const RAD_TO_DEG: f64 = 57.29577951308232;
/// 0.0174532925 (double, 0x08647390): degrees to radians.
pub(super) const DEG_TO_RAD: f64 = 0.0174532925;

fn local_i32(m: &mut Mem) -> Ptr32<i32> {
    Ptr32::new(m.local(4))
}

/// The curve a parameter of an element is driven by
/// [soft 0x080efe18 SI_edElementStaticFcvGet].
///
/// `*relid` and `*group` receive the relation and the relation list the curve hangs on
/// (`SI_attdictGetInfoFromFcurve`; for a named custom parameter 20010's group of custom
/// curves), `*found` 1 unless a sub-curve that was asked for is missing. Shape and cluster
/// parameters look on the element's shape (relation 0xd2, 0xd7 or 200 to the lattice,
/// shape chapter) or on the cluster of the access information's id; a shape weight (0x9a)
/// and a cluster weight (0xb8) take the `access0`-th curve of their type.
pub fn ed_element_static_fcv_get(
    m: &mut Mem,
    o: &mut dyn Outside,
    mut e: Ptr32<Elem>,
    fcv_type: i32,
    access: Ptr32<i32>,
    relid: Ptr32<i32>,
    group: Ptr32<i32>,
    custom: u32,
    found: Ptr32<i16>,
) -> u32 {
    let sp = m.sp();
    let l14 = local_i32(m);
    let l10 = local_i32(m);
    let lc = local_i32(m);
    let l8 = local_i32(m);
    m.set_u32(l8.addr(), 0);
    m.write(found, 1);
    let ed = rec::cur_dict(m);
    if fcv_type == FCV_CUSTOM && custom != 0 {
        let obj = m.u32(e.addr() + 4);
        o.cus_fetch_custom_value(m, Ptr32::new(obj), Ptr32::new(custom), l8.cast());
        let g = si_dict::attdict::get_group_custom_fcurve();
        m.write(l10, g);
        m.write(relid, 0x4e2a);
    } else {
        si_dict::attdict::get_info_from_fcurve(m, SI_ATD, fcv_type, relid, l10, lc, lc);
    }
    let g = m.read(l10);
    m.write(group, g);
    if fcv_type != FCV_CUSTOM || custom == 0 {
        let related = |m: &mut Mem, e: Ptr32<Elem>, rel: i32, chap: i32| {
            si_dict::elemrel::find_related_elem(m, ed, e, 1, 2, rel, chap, lc, lc)
        };
        if fcv_type == 0x43 {
            e = related(m, e, 0xd7, rec::chap(m, cid::LAT));
        } else if fcv_type < 0x44 {
            if fcv_type == 8 {
                e = related(m, e, 200, rec::chap(m, cid::SHP));
            } else if fcv_type == 0x42 {
                e = related(m, e, 0xd2, rec::chap(m, cid::LAT));
            }
        } else if fcv_type == 0x9a {
            e = related(m, e, 200, rec::chap(m, cid::SHP));
        } else if fcv_type == 0xb7 || fcv_type == 0xb8 {
            let cls = rec::chap(m, cid::CLS);
            let mut c = si_dict::elemrel::find_related_elem(m, ed, e, 1, 2, 700, cls, lc, l14);
            loop {
                if c.is_null() {
                    m.write(found, 0);
                    m.set_sp(sp);
                    return 0;
                }
                if m.read(l14) == m.i32(access.addr() + 4) {
                    e = c;
                    break;
                }
                c = si_dict::elemrel::find_next_related_elem(m, ed, e, 1, 2, 700, cls, lc, l14);
            }
        }
        let (gr, rid) = (m.read(group), m.read(relid));
        let mut f = si_motion::ed::find_fcv(m, e.addr(), gr, rid);
        while f != 0 && m.i32(f + fcurve::KIND) != fcv_type {
            f = si_motion::ed::find_next_fcv(m, e.addr(), gr, rid);
        }
        m.set_u32(l8.addr(), f);
        if fcv_type != 0x9a && fcv_type != 0xb8 {
            m.set_sp(sp);
            return f;
        }
        let want = m.read(access);
        if want == -1 {
            m.set_sp(sp);
            return f;
        }
        let mut k = 0;
        if want > 0 {
            if f == 0 {
                m.write(found, 0);
                m.set_sp(sp);
                return 0;
            }
            loop {
                f = si_motion::ed::find_next_fcv(m, e.addr(), gr, rid);
                if f != 0 && m.i32(f + fcurve::KIND) == fcv_type {
                    k += 1;
                }
                if !(k < want && f != 0) {
                    break;
                }
            }
            m.set_u32(l8.addr(), f);
        }
    }
    let f = m.u32(l8.addr());
    if f == 0 {
        m.write(found, 0);
    }
    m.set_sp(sp);
    f
}

/// The current value of a parameter of an element, into the value record `out`
/// [soft 0x080f04e0 SI_edElementStaticValueGet]: the curve is looked up (not for a custom
/// parameter) and, if the look-up did not fail, the chapter's routine reads the object.
/// The answer is the routine's (0 when nothing was read).
pub fn ed_element_static_value_get(
    m: &mut Mem,
    o: &mut dyn Outside,
    e: Ptr32<Elem>,
    chap: i32,
    fcv_type: i32,
    access: Ptr32<i32>,
    out: Ptr32<AttributeValue>,
    custom: u32,
) -> i32 {
    let sp = m.sp();
    let l14 = local_i32(m);
    let l10 = local_i32(m);
    let lc = Ptr32::<i32>::new(m.local(8));
    let l6 = Ptr32::<i16>::new(m.local(4));
    m.set_u32(l14.addr(), 0);
    let mut r: i16 = 0;
    let go = if fcv_type == FCV_CUSTOM {
        true
    } else {
        let f = ed_element_static_fcv_get(m, o, e, fcv_type, access, l10, lc, 0, l6);
        m.set_u32(l14.addr(), f);
        m.read(l6) != 0
    };
    if !go {
        m.set_sp(sp);
        return 0;
    }
    let obj = m.u32(e.addr() + 4);
    let f = m.u32(l14.addr());
    if chap == rec::chap(m, cid::MODEL) {
        if fcv_type == FCV_CUSTOM {
            let v = o.cus_fetch_custom_value(m, Ptr32::new(obj), Ptr32::new(custom), l14.cast());
            if v != 0 {
                let f = m.u32(l14.addr());
                r = o.custom_get_param(m, v, f, out.addr());
            }
        } else {
            let coord = m.read(access);
            r = model_get_param(m, o, fcv_type, obj, f, out, coord);
        }
    } else if chap == rec::chap(m, cid::CAM) {
        r = params::cam_get_param(m, fcv_type, obj, f, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::WAV) {
        r = params::wave_get_param(m, fcv_type, obj, f, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::MAT) {
        r = params::material_get_param(m, fcv_type, obj, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::T2D) {
        r = params::text2d_get_param(m, o, fcv_type, obj, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::T3D) {
        r = params::text3d_get_param(m, fcv_type, obj, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::LIG) {
        r = params::light_get_param(m, o, fcv_type, obj, f, out.addr()) as i16;
    } else if chap == rec::chap(m, cid::DSC) && m[rec::SI_ED_SUMATRA_TO_3X] != 0 {
        // the export to the successor product: not taken by the full product's 3.x path
        r = params::scene_get_param(m, o, fcv_type, 0x0878_1800, out.addr()) as i16;
    }
    m.set_sp(sp);
    r as i32
}

/// Writes a parameter of an element from the value record `value`
/// [soft 0x080f06e4 SI_edElementStaticValueSet]: 0 when the parameter's curve look-up
/// failed, else 1 whatever the chapter's routine did. The sixth argument is not read.
pub fn ed_element_static_value_set(
    m: &mut Mem,
    o: &mut dyn Outside,
    e: Ptr32<Elem>,
    chap: i32,
    fcv_type: i32,
    access: Ptr32<i32>,
    value: Ptr32<AttributeValue>,
    _unused: i32,
    custom: u32,
) -> i32 {
    let sp = m.sp();
    let l14 = local_i32(m);
    let l10 = local_i32(m);
    let lc = Ptr32::<i32>::new(m.local(8));
    let l6 = Ptr32::<i16>::new(m.local(4));
    if fcv_type != FCV_CUSTOM {
        let f = ed_element_static_fcv_get(m, o, e, fcv_type, access, l10, lc, 0, l6);
        m.set_u32(l14.addr(), f);
        if m.read(l6) == 0 {
            m.set_sp(sp);
            return 0;
        }
    }
    let obj = m.u32(e.addr() + 4);
    let f = m.u32(l14.addr());
    if chap == rec::chap(m, cid::MODEL) {
        if fcv_type == FCV_CUSTOM {
            let v = o.cus_fetch_custom_value(m, Ptr32::new(obj), Ptr32::new(custom), l14.cast());
            if v != 0 {
                let f = m.u32(l14.addr());
                o.custom_set_param(m, v, f, value.addr());
            }
        } else {
            let coord = m.read(access);
            model_set_param(m, o, fcv_type, obj, f, value, coord);
        }
    } else if chap == rec::chap(m, cid::CAM) {
        params::cam_set_param(m, o, fcv_type, obj, f, value.addr());
    } else if chap == rec::chap(m, cid::WAV) {
        params::wave_set_param(m, o, fcv_type, obj, f, value.addr());
    } else if chap == rec::chap(m, cid::MAT) {
        material_set_param(m, o, fcv_type, obj, value.addr());
    } else if chap == rec::chap(m, cid::T2D) {
        params::text2d_set_param(m, o, fcv_type, obj, value.addr());
    } else if chap == rec::chap(m, cid::T3D) {
        params::text3d_set_param(m, o, fcv_type, obj, value.addr());
    } else if chap == rec::chap(m, cid::LIG) {
        params::light_set_param(m, o, fcv_type, obj, f, value.addr());
    }
    m.set_sp(sp);
    1
}

/// Writes one parameter of a material from the value record `value` [soft 0x080f5964 SI_materialSetParam]. The fcurve type
/// picks the field: the colour and shading floats of the material record (types 0x2d to 0x37, 0x39 and 0x3b, the float at
/// `value + 4` copied), the 16-bit words (0x187 to +0x758, 0x188 to +0x1e, 0x2a5 and 0x2a6 to +0xdc8 and +0xdca; for 0x188 the
/// value 7 becomes 1 and sets both of the last two to 2) and the blur width and decay (0x101, 0x102: the value, or 0 when below
/// 0). Any other type leaves the record alone. When the type was one of those the material is marked dirty
/// (`SI_materialSetDirty`) with 0x40; the answer is whether it was.
pub fn material_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, mat: u32, value: u32) -> bool {
    if !material_set_param_fields(m, fcv_type, mat, value) {
        return false;
    }
    material_set_dirty(m, o, mat, 0x40);
    true
}

/// The part of [`material_set_param`] before the dirty mark: the fields written, true when the type was a known one.
pub fn material_set_param_fields(m: &mut Mem, fcv_type: i32, mat: u32, value: u32) -> bool {
    let float_at = match fcv_type {
        0x2d => 0x34,
        0x2e => 0x38,
        0x2f => 0x3c,
        0x30 => 0x44,
        0x31 => 0x48,
        0x32 => 0x4c,
        0x33 => 0x54,
        0x34 => 0x58,
        0x35 => 0x5c,
        0x36 => 0x64,
        0x37 => 0x6a8,
        0x39 => 0x6b8,
        0x3b => 0x6dc,
        _ => 0,
    };
    if float_at != 0 {
        let v = m.f32(value + 4);
        m.set_f32(mat + float_at, v);
    } else {
        match fcv_type {
            0x187 => {
                let v = m.u16(value + 4);
                m.set_u16(mat + 0x758, v);
            }
            0x188 => {
                let v = m.u16(value + 8);
                m.set_u16(mat + 0x1e, v);
                if v as i16 == 7 {
                    m.set_u16(mat + 0x1e, 1);
                    m.set_u16(mat + 0xdc8, 2);
                    m.set_u16(mat + 0xdca, 2);
                }
            }
            0x2a5 => {
                let v = m.u16(value + 8);
                m.set_u16(mat + 0xdc8, v);
            }
            0x2a6 => {
                let v = m.u16(value + 8);
                m.set_u16(mat + 0xdca, v);
            }
            // SI_validMaterialBlurWidth (0x0828ca8c), SI_validMaterialBlurDecay (0x0828ca60): a negative value becomes 0
            0x101 | 0x102 => {
                let v = m.f32(value + 4);
                m.set_f32(mat + if fcv_type == 0x101 { 0x75c } else { 0x760 }, if v < 0.0 { 0.0 } else { v });
            }
            _ => return false,
        }
    }
    true
}

/// Marks a material changed SI_materialSetDirty (0x08281f9c): its flag word (+0xdac) becomes 1, and every model related to
/// it by relation 300 (the models that use it) gets `flags` ORed into its update flags (+0xa) and is marked for the display
/// (`si_mdlTxtSetDirtyHW`).
pub fn material_set_dirty(m: &mut Mem, o: &mut dyn Outside, mat: u32, flags: u16) {
    m.set_u16(mat + 0xdac, 1);
    let ed = rec::cur_dict(m);
    let (idx, c, i) = (local_i32(m), local_i32(m), local_i32(m));
    let chap = rec::chap(m, cid::MAT);
    let e = si_dict::elemmisc::get_elem_from_data(m, &mut DictOutside(o), ed, chap, Ptr32::new(mat), idx);
    if e.is_null() {
        return;
    }
    let models = rec::chap(m, cid::MODEL);
    let mut r = si_dict::elemrel::find_related_elem(m, ed, e, 0, 0, 0x12c, models, c, i);
    while !r.is_null() {
        let mdl = m.u32(r.addr() + 4);
        let f = m.u16(mdl + 0xa);
        m.set_u16(mdl + 0xa, f | flags);
        mdl_txt_set_dirty_hw(m, mdl, 1);
        r = si_dict::elemrel::find_next_related_elem(m, ed, e, 0, 0, 0x12c, models, c, i);
    }
}

/// [soft 0x082b2488 si_mdlTxtSetDirtyHW]: the model's byte at +0x3fa gets its top bit; with `flag` non-zero the same goes to
/// the first son and then each brother, as long as the one it has reached has its word at +0x206 zero.
pub fn mdl_txt_set_dirty_hw(m: &mut Mem, mdl: u32, flag: i16) {
    let b = m.u8(mdl + 0x3fa);
    m.set_u8(mdl + 0x3fa, b | 0x80);
    if flag == 0 {
        return;
    }
    let mut son = m.u32(mdl + 0x2c4);
    while son != 0 && m.u16(son + 0x206) == 0 {
        mdl_txt_set_dirty_hw(m, son, flag);
        son = m.u32(son + 0x2c0);
    }
}

/// A parameter's value through the attribute dictionary
/// [soft 0x080f00f0 SI_edElementAttValueGet]: when the attribute is valid for the element
/// (`SI_attdictValidateAttr`), its group exists and so does the element, the group's
/// reading routine (+0x18) answers, else 0. The routine is called by its address.
pub fn ed_element_att_value_get(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    let ok = si_dict::attdict::validate_attr(m, &mut DictOutside(o), ident, index);
    if ok == 0 {
        return 0;
    }
    let g = si_dict::attdict::attrib_group_get_by_index(m, SI_ATD, index);
    if g.is_null() {
        return 0;
    }
    let ed = rec::cur_dict(m);
    let (chap, id) = (m.i32(ident.addr()), m.i32(ident.addr() + 4));
    if si_dict::elemdict::get_elem(m, ed, chap, id).is_null() {
        return 0;
    }
    let get = m.u32(g.addr() + 0x18);
    let mut r: i16 = 0;
    if get != 0 {
        r = o.call(m, si_mem::Routine::at(get), &[ident.addr(), index.addr(), value.addr()]).eax as i16;
    }
    r as i32
}

/// `SI_frameToSec(SI_frameCurrent)` (0x08201554): the frame over the rate, in the
/// register.
pub(super) fn current_time(m: &Mem) -> F80 {
    F80::from_i32(m[rec::SI_FRAME_CURRENT]) / F80::from_f32(m[rec::SI_FRAME_RATE])
}

/// `SI_getAttrValInFloat` on the image.
pub(super) fn attr_float(m: &Mem, v: Ptr32<AttributeValue>) -> F80 {
    super::eval::get_attr_val_in_float(m, v)
}

/// `SI_modelRoot(model)` (0x0847838c): the top of the node's hierarchy.
pub(crate) fn model_root(m: &Mem, mut p: u32) -> u32 {
    while m.u32(p + model::FATHER) != 0 {
        p = m.u32(p + model::FATHER);
    }
    p
}

/// `SI_matrixExtractXyzRotation(model + 0x178, angles)` with the angles starting from the
/// local ones, as the two routines below call it (si-math has the routine).
pub(super) fn global_angles(m: &Mem, mdl: u32, previous: u32) -> [f32; 3] {
    let mut mat = [[0f32; 4]; 4];
    for (r, row) in mat.iter_mut().enumerate() {
        for (c, x) in row.iter_mut().enumerate() {
            *x = m.f32(mdl + model::GROT + 16 * r as u32 + 4 * c as u32);
        }
    }
    let prev = si_math::types::Vector::new(m.f32(previous), m.f32(previous + 4), m.f32(previous + 8));
    let keep = m.i16(KEEP_TURNS) != 0;
    let v = si_math::algebra::matrix_extract_xyz_rotation(&mat, &prev, keep);
    [v.x, v.y, v.z]
}

/// `SI_fcurveFrameValue(curve, frame)` (0x08200258) on the curve at `a`, through
/// `si-motion`'s bridge to `si-fcurve` (the curve's evaluation state stored back, as
/// `si_motion::fcv::value` does).
pub(super) fn fcurve_frame_value(m: &mut Mem, a: u32, frame: i32) -> F80 {
    let mut e = si_motion::fcv::env(m);
    let mut f = si_motion::fcv::load(m, a);
    let r = f.frame_value(frame, &mut e);
    m.set_f32(a + fcurve::LAST_TIME, f.last_time);
    m.set_f32(a + fcurve::LAST_VALUE, f.last_value);
    m.set_i32(a + fcurve::CACHE, f.cache);
    r.map_or(F80::ZERO, si_motion::fcv::f80)
}

pub use super::modelparam::{model_get_param, model_set_param};

/// A model's visibility, and with `branch` its sons' [soft 0x080f3c44 SI_modelSetHiddenParam].
///
/// For `state` 2 the graf node's display type (+0xc) becomes 2, the model's `visible`
/// (+0x2b8) 1 and bit 1 of the element's display type (+0x20) is set; for any other state
/// 0, 0 and the bit cleared. A metaball (kind 8) is touched,
/// `SI_metaballTouch(definition, NULL)`. With `branch` non-zero the same goes down to every
/// son of the element (`SI_edFindSon`, `SI_edFindNextSon` of the element), each with its
/// model's node in the current graf list (`SI_findGlData(list, model, 2)`, `si-view`'s).
/// Nothing tests the graf node: a null one is written through.
pub fn model_set_hidden_param(m: &mut Mem, o: &mut dyn Outside, gn: u32, mdl: u32, elem: u32, state: i32, branch: i16) {
    let on = state == 2;
    m.set_i32(gn.wrapping_add(el::GN_WIRE_TYPE), if on { 2 } else { 0 });
    m.set_i16(mdl.wrapping_add(model::VISIBLE), on as i16);
    let bits = m.u8(elem.wrapping_add(el::WIRE_TYPE));
    m.set_u8(elem.wrapping_add(el::WIRE_TYPE), if on { bits | 2 } else { bits & 0xfd });
    if m.i32(mdl.wrapping_add(model::KIND)) == 8 {
        let def = m.u32(mdl.wrapping_add(model::DEFINITION));
        o.metaball_touch(m, def, 0);
    }
    if branch == 0 {
        return;
    }
    let mut son = o.ed_find_son(m, elem);
    while son != 0 {
        let son_mdl = m.u32(son.wrapping_add(el::DATA));
        let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
        let son_gn = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(son_mdl), 2);
        model_set_hidden_param(m, o, son_gn.addr(), son_mdl, son, state, branch);
        son = o.ed_find_next_son(m, elem);
    }
}

/// An attribute of the group `General_Element` read from an element
/// [soft 0x080f6460 SI_generalAttrGetValue]: 1 when `value` was filled, else 0.
///
/// The element (`SI_elemdictGetElem` in the current dictionary) must exist and be in use
/// (+0x48 zero), the attribute (`SI_attdictAttributeGetByIndex(SI_ATD, index)`) exist,
/// `value` be given and the component asked for (`index` + 0x10) be one of the
/// attribute's. Then `value` + 0 takes the attribute's value type and the component's
/// parameter id says what follows:
///
/// | id | `value` + 4 | `value` + 8 |
/// |---|---|---|
/// | 0x104 | the enum type | the element's chapter (`ident` + 0) |
/// | 0x105, 0x106 | the name record's prefix, name (string pointers) | |
/// | 0x107, 0x108 | the name record's version, substitution through `strtol(s, NULL, 10)`, 0 for none | |
/// | 0x109 | the enum type | the graf node's selection, or the element's (+0x42) without one |
/// | 0x10a | the element's level in its hierarchy (+0x54) | |
/// | 0x10b | `SI_mapWireToIndexColour(colour, 1)`, the colour the graf node's or the element's | |
/// | 0x10c | the enum type | the graf node's display type, or the element's (+0x20) |
/// | 0x237 | (16 bits) 1 when bit 0 of the graf node's flags is clear; 0 is answered without a graf node | |
///
/// Any other id answers 0, `value` + 0 written.
pub fn general_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    let ed = rec::cur_dict(m);
    let (chap, id) = (m.i32(ident.addr()), m.i32(ident.addr().wrapping_add(4)));
    let e = si_dict::elemdict::get_elem(m, ed, chap, id).addr();
    if e == 0 || m.i16(e + el::STATUS) != 0 {
        return 0;
    }
    let a = si_dict::attdict::attribute_get_by_index(m, SI_ATD, index).addr();
    let v = value.addr();
    if a == 0 || v == 0 {
        return 0;
    }
    let k = m.i32(index.addr().wrapping_add(0x10));
    if k < 0 || k >= m.i16(a + attr::NB_VALUES) as i32 {
        return 0;
    }
    m.set_i32(v, m.i32(a + attr::VALUE_TYPE));
    // the component is read again after the store, as the original does
    let k = m.i32(index.addr().wrapping_add(0x10));
    let ids = m.u32(a + attr::FCV_IDS);
    let enum_type = |m: &Mem| m.i16(a + attr::ENUM_TYPE) as i32;
    let node_word = |m: &Mem, off: u32| m.u32(m.u32(e + el::NODE).wrapping_add(off));
    match m.u16(ids.wrapping_add(2 * k as u32)) {
        0x104 => {
            m.set_i32(v + 4, enum_type(m));
            m.set_i32(v + 8, m.i32(ident.addr()));
        }
        0x105 => m.set_u32(v + 4, node_word(m, el::NODE_PREFIX)),
        0x106 => m.set_u32(v + 4, node_word(m, el::NODE_NAME)),
        id @ (0x107 | 0x108) => {
            let s = node_word(m, if id == 0x107 { el::NODE_VERSION } else { el::NODE_SUBST });
            let n = if s == 0 { 0 } else { si_mem::clib::strtol10(m, s) };
            m.set_i32(v + 4, n);
        }
        0x109 => {
            m.set_i32(v + 4, enum_type(m));
            let gn = m.u32(e + el::AUX_DATA);
            let sel = if gn != 0 { m.i32(gn + el::GN_SELECTION) } else { m.i16(e + el::SELECTED) as i32 };
            m.set_i32(v + 8, sel);
        }
        0x10a => m.set_i32(v + 4, m.i32(e + el::HIER_LEVEL)),
        0x10b => {
            let gn = m.u32(e + el::AUX_DATA);
            let colour = if gn != 0 { m.i16(gn + el::GN_WIRE_COLOR) } else { m.i16(e + el::WIRE_COLOR) };
            let index = o.map_wire_to_index_colour(m, colour, 1);
            m.set_i32(v + 4, index as i32);
        }
        0x10c => {
            m.set_i32(v + 4, enum_type(m));
            let gn = m.u32(e + el::AUX_DATA);
            let wire = if gn != 0 { m.i32(gn + el::GN_WIRE_TYPE) } else { m.i32(e + el::WIRE_TYPE) };
            m.set_i32(v + 8, wire);
        }
        0x237 => {
            let gn = m.u32(e + el::AUX_DATA);
            if gn == 0 {
                return 0;
            }
            m.set_u16(v + 4, ((m.u8(gn + el::GN_FLAGS) ^ 1) & 1) as u16);
        }
        _ => return 0,
    }
    1
}

#[cfg(test)]
mod material_tests {
    use super::*;
    use si_mem::Trace;

    /// The colour types copy the float at value+4 into their field, blur widths below 0 become 0, an unknown type writes
    /// nothing and answers false; the dirty son walk marks son and brother but stops at a brother with +0x206 set.
    #[test]
    fn material_params_write_their_fields() {
        let mut m = Mem::new();
        let mat = m.si_malloc(0xe00);
        let val = m.si_malloc(0x20);
        let mut o = Trace::default();
        m.set_f32(val + 4, 1.25);
        assert!(material_set_param(&mut m, &mut o, 0x2e, mat, val));
        assert_eq!(m.f32(mat + 0x38), 1.25);
        assert_eq!(m.u16(mat + 0xdac), 1);
        m.set_f32(val + 4, -3.0);
        assert!(material_set_param(&mut m, &mut o, 0x101, mat, val));
        assert_eq!(m.f32(mat + 0x75c), 0.0);
        m.set_u16(val + 8, 7);
        assert!(material_set_param(&mut m, &mut o, 0x188, mat, val));
        assert_eq!((m.u16(mat + 0x1e), m.u16(mat + 0xdc8), m.u16(mat + 0xdca)), (1, 2, 2));
        m.set_u16(mat + 0xdac, 0);
        assert!(!material_set_param(&mut m, &mut o, 0x999, mat, val));
        assert_eq!(m.u16(mat + 0xdac), 0);

        let root = m.si_malloc(0x430);
        let s1 = m.si_malloc(0x430);
        let s2 = m.si_malloc(0x430);
        let s3 = m.si_malloc(0x430);
        m.set_u32(root + 0x2c4, s1);
        m.set_u32(s1 + 0x2c0, s2);
        m.set_u32(s2 + 0x2c0, s3);
        m.set_u16(s3 + 0x206, 1);
        mdl_txt_set_dirty_hw(&mut m, root, 1);
        assert_eq!((m.u8(root + 0x3fa), m.u8(s1 + 0x3fa), m.u8(s2 + 0x3fa), m.u8(s3 + 0x3fa)), (0x80, 0x80, 0x80, 0));
    }
}
