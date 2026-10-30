// SPDX-License-Identifier: MIT
//! Keys: `motionKey.c`, the "save key" commands (finding 0017, "Keys").
//!
//! What saving a key is, for every kind of element: for each parameter of the group, the
//! parameter's curve is looked up on the element and **created if missing**
//! ([`fcurve_check_add1`], [`fcurve_check_add3`], [`fcurve_check_add`]); then a key is made
//! at the **current frame** with the parameter's present value, slope 0, automatic slope
//! type and the **curve's own interpolation** (`SI_fkeyFrameCreate(SI_frameCurrent, value,
//! 0.0, 0, curve->interpolation)`), and inserted (`SI_fcurveAddKey`: a key within the key
//! tolerance of an existing one replaces it, and the slopes of the key and its neighbours
//! are recomputed; finding 0009). Angles are stored in the structures in radians and in the
//! curves in degrees: the value is multiplied by the double 57.29577951308232.
//!
//! Ported here: the curve look-up-or-create family, key removal at a time, and the
//! commands for materials, 2D and 3D textures, depth fading, layer fog, physical properties
//! and the dynamics root constraint. The commands for models are in `savekey.rs`, those for
//! cameras, lights and fittings in `savekey2.rs`.

use crate::ed::{self, EdHost};
use crate::fcv;
use crate::glob::{self, cid};
use si_mem::{Addr, Mem};
use crate::off::{dict, elem, fcurve as fo, list, model};
use crate::reset::ResetHost;
use si_fcurve::{Fcurve, Key};
use si_x87::F80;

/// The constant at 0x0864d8a0 and its copies in this file's read-only data: radians to
/// degrees.
pub const RAD2DEG: f64 = 57.29577951308232;

crate::host_trait! {
    /// What `motionKey.c` calls outside this package.
    pub trait KeyHost: EdHost {
        /// `SI_fcvGetRelInfo(curve type, &relation, &a, &b)`: the relation id a curve type
        /// belongs to (the attribute dictionary; two further outputs are not used here).
        fn fcv_get_rel_info(kind: i32, relid: Addr, a: Addr, b: Addr);
        /// `SI_reldictGetRelChapGrp(reldict, relation, &list)` (reldict.c): the chapter of
        /// the relation's source and, through the pointer, its relation list; negative when
        /// the relation is unknown.
        fn reldict_get_rel_chap_grp(reldict: Addr, relid: i32, group: Addr) -> i32;
        /// `SI_relGetFcvType(relation)`: the curve type of the relation's first component.
        fn rel_get_fcv_type(relid: i32) -> i32;
        /// `SI_edFcvAdd(curve, elem, chapter, index)` (edanim.c): files the curve as an
        /// element of the ANIMATION chapter and relates the element to it.
        fn ed_fcv_add(curve: Addr, elem: Addr, chap: i32, index: i32) -> i32;
        /// `SI_statusDisplayMessage(text, kind)` (status.c).
        fn status_display_message(text: Addr, kind: i32);
        /// `SI_edGetElemFromMaterial(material, &index)` (edtools.c).
        fn ed_get_elem_from_material(mat: Addr, index: Addr) -> Addr;
        /// `SI_edGetElemFromTexture2D(texture, &index)` (edtools.c).
        fn ed_get_elem_from_texture2d(tex: Addr, index: Addr) -> Addr;
        /// `SI_edGetElemFromTexture3D(texture, &index)` (edtools.c).
        fn ed_get_elem_from_texture3d(tex: Addr, index: Addr) -> Addr;
        /// `SI_edGetElemFromScene(&index)` (edtools.c).
        fn ed_get_elem_from_scene(index: Addr) -> Addr;
        /// `SI_elemdictSetDeleteElem(dict, chapter, elem, index, a, 1, 1)` (elemdict.c).
        fn elemdict_set_delete_elem(dict: Addr, chap: i32, elem: Addr, index: i32, a: i32, b: i32, c: i32);
        /// `SI_dopeFlagsSet(1, 0, 1, 1)` (the dope sheet must be redrawn).
        fn dope_flags_set(a: i32, b: i32, c: i32, d: i32);
        /// `SI_edCheckAnimFileDispose(elem, chapter)` (edanim.c).
        fn ed_check_anim_file_dispose(elem: Addr, chap: i32) -> i16;
        /// `SI_elemdictSortAnimElements(list)` (elemdict.c).
        fn elemdict_sort_anim_elements(list: Addr);
    }
}

/// `SI_fcurveAllocate(type, owner)` (0x0820a810, ported in `si-fcurve`) in the image: a
/// 100-byte record with the type's interpolation (constant for the on/off parameters,
/// linear for the sun's time, slope-0 for shape and cluster weights, the user's default
/// otherwise) and no keys.
pub fn fcurve_allocate(m: &mut Mem, kind: i32, owner: Addr) -> Addr {
    let a = m.si_malloc(fo::SIZE);
    let f = Fcurve::allocate(kind, owner, &fcv::env(m));
    fcv::store(m, a, &f);
    m.set_ptr(a + fo::AUX, 0);
    a
}

/// A key at the current frame: `SI_fkeyFrameCreate(SI_frameCurrent, value, 0.0, 0, interp)`
/// and `SI_fcurveAddKey(curve, key)` (both ported in `si-fcurve`; [`fcv::add_key`] has what
/// the insertion does around the curve: a new array for a first key, the question about a
/// locked key, the curve code's own calls). `interp_of` is the curve whose interpolation word
/// (+0x1e) the key takes: the curve itself, except where the original reads another curve's
/// (see [`depth_fading_save_key`]).
///
/// A null curve (the relation is unknown to the relation dictionary) makes the original read
/// the interpolation at address 0x1e; here that is a recorded fault.
pub fn add_key_now<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, value: f32, interp_of: Addr) {
    let env = fcv::env(m);
    let interp = m.i16(interp_of + fo::INTERP);
    let key = Key::frame_create(env.frame_current, value, 0.0, 0, interp, &env);
    fcv::add_key(m, h, curve, key);
}

/// The curve of type `kind` on an element, created if the element has none
/// [soft 0x082194b0 SI_fcurveCheckAdd]. The relation the type belongs to comes from
/// `SI_fcvGetRelInfo`, its list from the relation dictionary; the element's curves of that
/// relation are searched for one of that type. A new curve is owned by the element's object
/// and filed with `SI_edFcvAdd(curve, elem, chapter, index)`. Null when the relation is
/// unknown.
pub fn fcurve_check_add<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, kind: i32) -> Addr {
    let out = m.local(16);
    h.fcv_get_rel_info(m, kind, out, out + 4, out + 8);
    let relid = m.i32(out);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out + 12);
    let group = m.i32(out + 12);
    m.release(16);
    if chap < 0 {
        return 0;
    }
    let mut c = ed::find_fcv(m, e, group, relid);
    while c != 0 {
        if fcv::kind(m, c) == kind {
            return c;
        }
        c = ed::find_next_fcv(m, e, group, relid);
    }
    let c = fcurve_allocate(m, kind, m.ptr(e + elem::DATA));
    h.ed_fcv_add(m, c, e, chap, index);
    c
}

/// The (first) curve of relation `relid` on an element, created if missing
/// [soft 0x08219560 SI_fcurveCheckAdd1]; a new curve has the type `SI_relGetFcvType(relid)`.
pub fn fcurve_check_add1<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32) -> Addr {
    let out = m.local(4);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out);
    let group = m.i32(out);
    m.release(4);
    if chap < 0 {
        return 0;
    }
    let mut c = ed::find_fcv(m, e, group, relid);
    if c == 0 {
        let kind = h.rel_get_fcv_type(m, relid);
        c = fcurve_allocate(m, kind, m.ptr(e + elem::DATA));
        h.ed_fcv_add(m, c, e, chap, index);
    }
    c
}

/// The three curves of a three-component relation, each created if missing
/// [soft 0x082195e8 SI_fcurveCheckAdd3]: types T, T + 1, T + 2 with T =
/// `SI_relGetFcvType(relid)`. All null when the relation is unknown.
pub fn fcurve_check_add3<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32) -> [Addr; 3] {
    let out = m.local(16);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out);
    let group = m.i32(out);
    if chap < 0 {
        m.release(16);
        return [0; 3];
    }
    let t = h.rel_get_fcv_type(m, relid);
    crate::utils::get_fcv_triplet(m, h, e, out + 4, out + 8, out + 12, group, relid);
    let mut c = [m.ptr(out + 4), m.ptr(out + 8), m.ptr(out + 12)];
    m.release(16);
    for (k, slot) in c.iter_mut().enumerate() {
        if *slot == 0 {
            *slot = fcurve_allocate(m, t + k as i32, m.ptr(e + elem::DATA));
            h.ed_fcv_add(m, *slot, e, chap, index);
        }
    }
    c
}

/// Removes the key at a time from a curve, and the curve with its last key
/// [soft 0x08219700 fcurveRemoveKeyFromTime]: the keys strictly between `time − 0.01` and
/// `time + 0.01` seconds go (`SI_fcurveRemoveKeyBetweenTimes(curve, 0, t − 0.01, t + 0.01,
/// 0)`; each bound is one operation with the double 0.01, rounded to `float`); a curve left
/// with no key has its ANIMATION element deleted (`SI_elemdictSetDeleteElem(dict,
/// ANIMATION, elem, index, arg, 1, 1)`).
pub fn fcurve_remove_key_from_time<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, time: f32, arg: i32) {
    if curve == 0 {
        return;
    }
    if m.i32(curve + fo::NB_KEYS) > 0 {
        // the upper bound is computed first (0x0821972f), then the lower: each the time and
        // the double 0.01 (0x0864d898) in the register, rounded to `float` by its store
        let hi = (F80::from_f32(time) + 0.01f64).to_f32();
        let lo = (F80::from_f32(time) - 0.01f64).to_f32();
        fcv::Ops::on(m, h, curve, |f, env| f.remove_key_between_times(0, lo, hi, 0, env));
    }
    if m.i32(curve + fo::NB_KEYS) == 0 {
        let d = glob::cur_dict(m);
        let ani = glob::chap(m, cid::ANIMATION);
        let i = h.elemdict_find_elem_by_data(m, d, ani, curve);
        let d = glob::cur_dict(m);
        let ani = glob::chap(m, cid::ANIMATION);
        let e = ed::get_elem(m, d, ani, i);
        let d = glob::cur_dict(m);
        let ani = glob::chap(m, cid::ANIMATION);
        h.elemdict_set_delete_elem(m, d, ani, e, i, arg, 1, 1);
    }
}

/// Removes the keys at a time from the three curves of a relation
/// [soft 0x082197e4 SI_fcurveCheckRemoveKey3]. Nothing at all when the element has none of
/// the three. Otherwise each curve there is loses its key ([`fcurve_remove_key_from_time`]),
/// the dope sheet is flagged (`SI_dopeFlagsSet(1, 0, 1, 1)`), and unless the element's
/// animation file went with the curves (`SI_edCheckAnimFileDispose`: a 16-bit answer), the
/// ANIMATION chapter is sorted again.
pub fn fcurve_check_remove_key3<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, arg: i32, relid: i32, time: f32) {
    if e == 0 {
        return;
    }
    let out = m.local(16);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out);
    if chap < 0 {
        m.release(16);
        return;
    }
    let group = m.i32(out);
    m.set_bytes(out + 4, &[0; 12]);
    crate::utils::get_fcv_triplet(m, h, e, out + 4, out + 8, out + 12, group, relid);
    let c = [m.ptr(out + 4), m.ptr(out + 8), m.ptr(out + 12)];
    m.release(16);
    if c == [0; 3] {
        return;
    }
    for curve in c {
        if curve != 0 {
            fcurve_remove_key_from_time(m, h, curve, time, arg);
        }
    }
    h.dope_flags_set(m, 1, 0, 1, 1);
    if h.ed_check_anim_file_dispose(m, e, chap) == 0 {
        let ani = glob::chap(m, cid::ANIMATION);
        let l = m.ptr(glob::cur_dict(m) + dict::LISTS).wrapping_add((ani as u32).wrapping_mul(list::SIZE));
        h.elemdict_sort_anim_elements(m, l);
    }
}

/// One key on a one-curve parameter of a material [soft 0x0821a850 savekeyMatSingleFcurve];
/// the same body serves 2D textures [soft 0x0821ab1c savekeyMapFcurve] and 3D textures
/// [soft 0x0821af50 savekeyTX3DFcurve].
pub fn savekey_single_fcurve<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, value: f32) {
    let c = fcurve_check_add1(m, h, e, index, relid);
    add_key_now(m, h, c, value, c);
}

/// One key on each of the three curves of a material colour
/// [soft 0x0821a8a0 savekeyMatTripleFcurve].
pub fn savekey_mat_triple_fcurve<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, v: [f32; 3]) {
    let c = fcurve_check_add3(m, h, e, index, relid);
    for k in 0..3 {
        add_key_now(m, h, c[k], v[k], c[k]);
    }
}

/// The message addresses of this file's status-line texts.
pub mod msg {
    use si_mem::Addr;
    /// "SavekeyMaterial … model does not have a material" (0x08696fe0).
    pub const NO_MATERIAL: Addr = 0x0869_6fe0;
    /// The two messages of the local texture command (no material; no texture).
    pub const LTEX_NO_MATERIAL: Addr = 0x0869_7060;
    pub const LTEX_NO_TEXTURE: Addr = 0x0869_7020;
    /// "SavekeyGTexture … model does not have a texture" (0x086970a0).
    pub const GTEX_NO_TEXTURE: Addr = 0x0869_70a0;
    /// The two messages of the local 3D texture command (no material; no 3D texture).
    pub const LTEX3D_NO_MATERIAL: Addr = 0x0869_7120;
    pub const LTEX3D_NO_TEXTURE: Addr = 0x0869_70e0;
    /// "SavekeyGTexture3D … model does not have a 3D texture" (0x08697160).
    pub const GTEX3D_NO_TEXTURE: Addr = 0x0869_7160;
}

/// Fields of a model as the material and texture commands use them (finding 0017, section 10).
pub mod mdl {
    /// 16 bits, signed: index of the current material in the array at [`MATERIALS`].
    pub const CUR_MATERIAL: u32 = 0x208;
    /// Array of material pointers.
    pub const MATERIALS: u32 = 0x20c;
    /// The global 2D texture, and the model's list of 2D textures (which must not be empty).
    pub const GTEX2D: u32 = 0x218;
    pub const TEX2D_LIST: u32 = 0x21c;
    /// The global 3D texture.
    pub const GTEX3D: u32 = 0x224;
}

/// Fields of `__SI_Material` that get keys (the same fields the update writes: section 7).
pub mod mat {
    pub const AMBIENT: u32 = 0x34;
    pub const DIFFUSE: u32 = 0x44;
    pub const SPECULAR: u32 = 0x54;
    pub const SPEC_DECAY: u32 = 0x64;
    pub const REFLECT: u32 = 0x6a8;
    pub const TRANSP: u32 = 0x6b8;
    pub const REFR_INDEX: u32 = 0x6dc;
    /// The material's first local 2D texture; its local 3D texture.
    pub const TEX2D: u32 = 0x6f0;
    pub const TEX3D: u32 = 0x6fc;
    pub const BLUR_WIDTH: u32 = 0x75c;
    pub const BLUR_DECAY: u32 = 0x760;
}

/// Fields of `__SI_LclMap` (a 2D texture) that get keys.
pub mod tex2d {
    pub const U_SCALE: u32 = 0x58;
    pub const V_SCALE: u32 = 0x5c;
    pub const U_OFFSET: u32 = 0x68;
    pub const V_OFFSET: u32 = 0x6c;
    /// The projection support: rotation (radians), translation, scaling, three floats each.
    pub const S_ROTATION: u32 = 0x94;
    pub const S_TRANSLATION: u32 = 0xa0;
    pub const S_SCALING: u32 = 0xac;
    pub const BLEND: u32 = 0x10c;
    pub const AMBIENT: u32 = 0x128;
    pub const DIFFUSE: u32 = 0x12c;
    pub const SPECULAR: u32 = 0x130;
    pub const REFLECT: u32 = 0x134;
    pub const TRANSP: u32 = 0x138;
    pub const ROUGHNESS: u32 = 0x140;
    pub const REFL_MAP: u32 = 0x144;
    /// Rotation of the picture, radians.
    pub const ROTATION: u32 = 0x168;
}

/// Fields of `__SI_Texture3D` that get keys.
pub mod tex3d {
    pub const SPACING: u32 = 0x10;
    /// Taper angle, radians.
    pub const TAPER: u32 = 0x14;
    pub const STRENGTH: u32 = 0x18;
    pub const POWER: u32 = 0x20;
    pub const BLEND: u32 = 0x28;
    /// Five colours, four floats each (red, green, blue, alpha), 0x10 apart.
    pub const COLOR0: u32 = 0x30;
    pub const FACTOR1: u32 = 0x80;
    pub const FACTOR2: u32 = 0x84;
    pub const FACTOR3: u32 = 0x88;
    pub const SCALING: u32 = 0x90;
    /// Rotation, radians.
    pub const ROTATION: u32 = 0x9c;
    pub const TRANSLATION: u32 = 0xa8;
    pub const AMBIENT: u32 = 0x108;
    pub const DIFFUSE: u32 = 0x10c;
    pub const SPECULAR: u32 = 0x110;
    pub const REFLECT: u32 = 0x114;
    pub const TRANSP: u32 = 0x118;
    pub const ROUGHNESS: u32 = 0x120;
}

/// Fields of the render control record (`SI_defaultControl`) that get keys.
pub mod ctl {
    pub const FAD_START: u32 = 0x104;
    pub const FAD_STOP: u32 = 0x10c;
    pub const FAD_COLOR: u32 = 0x118;
    pub const FAD_TRANSP: u32 = 0x130;
    pub const FOG_DENSITY: u32 = 0x260;
    pub const FOG_BASE: u32 = 0x264;
    pub const FOG_THICKNESS: u32 = 0x268;
    pub const FOG_START: u32 = 0x26c;
    pub const FOG_STOP: u32 = 0x270;
    pub const FOG_COLOR: u32 = 0x284;
}

/// Fields of the two dynamics records of a model that get keys.
pub mod dyna {
    /// `SI_DynaMatterInfo` (model + 0x2f4): static roughness, kinetic roughness, elasticity.
    pub const STAT_ROUGH: u32 = 0x0c;
    pub const KINE_ROUGH: u32 = 0x10;
    pub const ELASTICITY: u32 = 0x14;
    /// `SI_DynaChnVars` (model + 0x304): the root constraint, 16 bits.
    pub const ROOT_CONS: u32 = 0x38;
}

/// The current material of a model: entry `(short)model[+0x208]` of the array at +0x20c.
fn current_material(m: &Mem, model: Addr) -> Addr {
    let i = m.i16(model + mdl::CUR_MATERIAL) as i32 as u32;
    m.ptr(m.ptr(model + mdl::MATERIALS).wrapping_add(i.wrapping_mul(4)))
}

fn vec3(m: &Mem, a: Addr) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

/// Save key, material of a model [soft 0x0821a968 SI_savekeyMaterial].
///
/// The model's update flags get bit 0x40. The material is the model's current one (array
/// +0x20c, index +0x208); without one the status line says so. Keys, in this order:
///
/// | Relation | Value from the material |
/// |---|---|
/// | 301 ambient, 302 diffuse, 303 specular | +0x34, +0x44, +0x54 (three floats each) |
/// | 304 specular decay | +0x64 |
/// | 305 reflectivity | +0x6a8 |
/// | 307 transparency | +0x6b8 |
/// | 310 refraction index | +0x6dc |
/// | 311 blur width, 312 blur decay | +0x75c, +0x760 |
///
/// The two jitter parameters (306, 308), which the update applies, get **no key** here.
pub fn savekey_material<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    m.or_u8(mdl + model::UPDATE_FLAGS, 0x40);
    m.or_u8(mdl + model::UPDATE_FLAGS2, 0x40);
    let mat = current_material(m, mdl);
    if mat == 0 {
        KeyHost::status_display_message(h, m, msg::NO_MATERIAL, 3);
        return;
    }
    let idx = m.local(4);
    let e = h.ed_get_elem_from_material(m, mat, idx);
    let index = m.i32(idx);
    m.release(4);
    if e == 0 {
        return;
    }
    for (relid, at) in [(301, mat::AMBIENT), (302, mat::DIFFUSE), (303, mat::SPECULAR)] {
        let v = vec3(m, mat + at);
        savekey_mat_triple_fcurve(m, h, e, index, relid, v);
    }
    for (relid, at) in [(304, mat::SPEC_DECAY), (305, mat::REFLECT), (307, mat::TRANSP), (310, mat::REFR_INDEX), (311, mat::BLUR_WIDTH),
                        (312, mat::BLUR_DECAY)]
    {
        let v = m.f32(mat + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
}

/// One key on each of the three curves of a 2D texture's support
/// [soft 0x0821ab6c savekeyMapTripleFcurve]; the rotation (relation 424) is converted from
/// radians to degrees first (x, then z, then y: each the double 57.29577951308232 at
/// 0x0864d8b8 times the float, rounded to `float`).
pub fn savekey_map_triple_fcurve<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, v: [f32; 3]) {
    let mut v = v;
    if relid == 424 {
        let k = F80::from_f64(RAD2DEG);
        v = [(k * v[0]).to_f32(), (k * v[1]).to_f32(), (k * v[2]).to_f32()];
    }
    let c = fcurve_check_add3(m, h, e, index, relid);
    for k in 0..3 {
        add_key_now(m, h, c[k], v[k], c[k]);
    }
}

/// Save key, all parameters of a 2D texture [soft 0x0821ad18 skTexture]. Keys in this
/// order (relation ← field of the texture):
///
/// 411 ← +0x58, 412 ← +0x5c, 413 ← +0x68, 414 ← +0x6c, 416 ← +0x128, 417 ← +0x12c,
/// 418 ← +0x130, 422 ← +0x144, 419 ← +0x138, 421 ← +0x140, 423 ← +0x168 × 57.29577951308232,
/// 415 ← +0x10c, 420 ← +0x134; then the support: 424 ← +0x94 (radians → degrees),
/// 425 ← +0xac, 426 ← +0xa0.
pub fn sk_texture<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, tex: Addr) {
    let idx = m.local(4);
    let e = h.ed_get_elem_from_texture2d(m, tex, idx);
    let index = m.i32(idx);
    m.release(4);
    if e == 0 {
        return;
    }
    for (relid, at) in [(411, tex2d::U_SCALE), (412, tex2d::V_SCALE), (413, tex2d::U_OFFSET), (414, tex2d::V_OFFSET), (416, tex2d::AMBIENT),
                        (417, tex2d::DIFFUSE), (418, tex2d::SPECULAR), (422, tex2d::REFL_MAP), (419, tex2d::TRANSP), (421, tex2d::ROUGHNESS)]
    {
        let v = m.f32(tex + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
    let rot = (F80::from_f32(m.f32(tex + tex2d::ROTATION)) * RAD2DEG).to_f32();
    savekey_single_fcurve(m, h, e, index, 423, rot);
    for (relid, at) in [(415, tex2d::BLEND), (420, tex2d::REFLECT)] {
        let v = m.f32(tex + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
    for (relid, at) in [(424, tex2d::S_ROTATION), (425, tex2d::S_SCALING), (426, tex2d::S_TRANSLATION)] {
        let v = vec3(m, tex + at);
        savekey_map_triple_fcurve(m, h, e, index, relid, v);
    }
}

/// Save key, the local texture of a model's current material
/// [soft 0x0821ac5c SI_savekeyLTexture]: the 2D texture at material + 0x6f0; the model's
/// update flags get bit 0x80.
pub fn savekey_l_texture<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    if mdl == 0 {
        return;
    }
    let mat = current_material(m, mdl);
    if mat == 0 {
        KeyHost::status_display_message(h, m, msg::LTEX_NO_MATERIAL, 3);
        return;
    }
    let tex = m.ptr(mat + mat::TEX2D);
    if tex == 0 {
        KeyHost::status_display_message(h, m, msg::LTEX_NO_TEXTURE, 3);
        return;
    }
    sk_texture(m, h, tex);
    m.or_u8(mdl + model::UPDATE_FLAGS, 0x80);
    m.or_u8(mdl + model::UPDATE_FLAGS2, 0x80);
}

/// Save key, the global texture of a model [soft 0x0821acc8 SI_savekeyGTexture]: the 2D
/// texture at model + 0x218, provided the model's texture list (+0x21c) is not empty.
pub fn savekey_g_texture<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    if mdl == 0 {
        return;
    }
    if m.ptr(mdl + mdl::TEX2D_LIST) == 0 || m.ptr(mdl + mdl::GTEX2D) == 0 {
        KeyHost::status_display_message(h, m, msg::GTEX_NO_TEXTURE, 3);
        return;
    }
    let tex = m.ptr(mdl + mdl::GTEX2D);
    sk_texture(m, h, tex);
    m.or_u8(mdl + model::UPDATE_FLAGS, 0x80);
    m.or_u8(mdl + model::UPDATE_FLAGS2, 0x80);
}

/// Four keys of a 3D texture's colour, alpha included
/// [soft 0x0821afa0 savekeyTX3DQuadFcurve].
///
/// The element's first four curves of the relation (list 0) are taken **in the order they are
/// filed**, whatever their types (the second is looked for only if there is a first, and so
/// on); a missing one is made with the type `kind`, `kind + 1`, `kind + 2` or, for the
/// fourth, `alpha_kind` (the alpha curves have type numbers of their own, 156 to 160), owned
/// by the texture and filed in the TEXTURES3D chapter. Then red, green, blue and alpha are
/// keyed on the first, second, third and fourth: so curves filed in another order than red,
/// green, blue, alpha get one another's values.
#[allow(clippy::too_many_arguments)]
pub fn savekey_tx3d_quad_fcurve<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, tex: Addr, index: i32, relid: i32, kind: i32,
                                                       alpha_kind: i32, v: [f32; 4]) {
    let mut c = [0 as Addr; 4];
    c[0] = ed::find_fcv(m, e, 0, relid);
    for k in 1..4 {
        if c[k - 1] != 0 {
            c[k] = ed::find_next_fcv(m, e, 0, relid);
        }
    }
    let kinds = [kind, kind.wrapping_add(1), kind.wrapping_add(2), alpha_kind];
    for k in 0..4 {
        if c[k] == 0 {
            c[k] = fcurve_allocate(m, kinds[k], tex);
            let chap = glob::chap(m, cid::TEXTURES3D);
            h.ed_fcv_add(m, c[k], e, chap, index);
        }
    }
    for k in 0..4 {
        add_key_now(m, h, c[k], v[k], c[k]);
    }
}

/// One key on each of the three curves of a 3D texture's scaling, rotation or translation
/// [soft 0x0821b1a8 savekeyTX3DTripleFcurve]; the rotation (relation 525) is converted from
/// radians to degrees (after the curves are looked up; the double at 0x0864d8c8).
pub fn savekey_tx3d_triple_fcurve<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, v: [f32; 3]) {
    let c = fcurve_check_add3(m, h, e, index, relid);
    let mut v = v;
    if relid == 525 {
        let k = F80::from_f64(RAD2DEG);
        v = [(k * v[0]).to_f32(), (k * v[1]).to_f32(), (k * v[2]).to_f32()];
    }
    for k in 0..3 {
        add_key_now(m, h, c[k], v[k], c[k]);
    }
}

/// Save key, all parameters of a 3D texture [soft 0x0821b34c skTexture3D]. Nothing for a
/// null texture or one without an element. Keys in this order (relation ← field):
///
/// 513 ← +0x18, 511 ← +0x10, 514 ← +0x20, 512 ← +0x14 × 57.29577951308232 (the taper, in
/// degrees), 521 ← +0x80, 522 ← +0x84, 523 ← +0x88; the five colours 516 … 520 ← +0x30,
/// +0x40, +0x50, +0x60, +0x70 (four floats each; curve types 76, 79, 82, 85, 88 and the
/// alpha types 156 … 160); 524 ← +0x90, 525 ← +0x9c (radians → degrees), 526 ← +0xa8;
/// 527 ← +0x108, 528 ← +0x10c, 529 ← +0x110, 531 ← +0x114, 530 ← +0x118, 515 ← +0x28,
/// 532 ← +0x120.
pub fn sk_texture3d<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, tex: Addr) {
    if tex == 0 {
        return;
    }
    let idx = m.local(4);
    let e = h.ed_get_elem_from_texture3d(m, tex, idx);
    let index = m.i32(idx);
    m.release(4);
    if e == 0 {
        return;
    }
    for (relid, at) in [(513, tex3d::STRENGTH), (511, tex3d::SPACING), (514, tex3d::POWER)] {
        let v = m.f32(tex + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
    let taper = (F80::from_f32(m.f32(tex + tex3d::TAPER)) * RAD2DEG).to_f32();
    savekey_single_fcurve(m, h, e, index, 512, taper);
    for (relid, at) in [(521, tex3d::FACTOR1), (522, tex3d::FACTOR2), (523, tex3d::FACTOR3)] {
        let v = m.f32(tex + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
    for k in 0..5u32 {
        let at = tex + tex3d::COLOR0 + 0x10 * k;
        let v = [m.f32(at), m.f32(at + 4), m.f32(at + 8), m.f32(at + 12)];
        savekey_tx3d_quad_fcurve(m, h, e, tex, index, 516 + k as i32, 76 + 3 * k as i32, 156 + k as i32, v);
    }
    for (relid, at) in [(524, tex3d::SCALING), (525, tex3d::ROTATION), (526, tex3d::TRANSLATION)] {
        let v = vec3(m, tex + at);
        savekey_tx3d_triple_fcurve(m, h, e, index, relid, v);
    }
    for (relid, at) in [(527, tex3d::AMBIENT), (528, tex3d::DIFFUSE), (529, tex3d::SPECULAR), (531, tex3d::REFLECT), (530, tex3d::TRANSP),
                        (515, tex3d::BLEND), (532, tex3d::ROUGHNESS)]
    {
        let v = m.f32(tex + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
}

/// Save key, the local 3D texture of a model's current material
/// [soft 0x0821b29c SI_savekeyLTexture3D]: the 3D texture at material + 0x6fc; the model's
/// update flags get bit 0x80.
pub fn savekey_l_texture3d<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    if mdl == 0 {
        return;
    }
    let mat = current_material(m, mdl);
    if mat == 0 {
        KeyHost::status_display_message(h, m, msg::LTEX3D_NO_MATERIAL, 3);
        return;
    }
    let tex = m.ptr(mat + mat::TEX3D);
    if tex == 0 {
        KeyHost::status_display_message(h, m, msg::LTEX3D_NO_TEXTURE, 3);
        return;
    }
    sk_texture3d(m, h, tex);
    m.or_u8(mdl + model::UPDATE_FLAGS, 0x80);
    m.or_u8(mdl + model::UPDATE_FLAGS2, 0x80);
}

/// Save key, the global 3D texture of a model [soft 0x0821b308 SI_savekeyGTexture3D]: the
/// 3D texture at model + 0x224.
pub fn savekey_g_texture3d<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    if mdl == 0 {
        return;
    }
    let tex = m.ptr(mdl + mdl::GTEX3D);
    if tex == 0 {
        KeyHost::status_display_message(h, m, msg::GTEX3D_NO_TEXTURE, 3);
        return;
    }
    sk_texture3d(m, h, tex);
    m.or_u8(mdl + model::UPDATE_FLAGS, 0x80);
    m.or_u8(mdl + model::UPDATE_FLAGS2, 0x80);
}

/// The scene's element and its index (`SI_edGetElemFromScene(&index)`).
fn scene_elem<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H) -> (Addr, i32) {
    let idx = m.local(4);
    let e = h.ed_get_elem_from_scene(m, idx);
    let index = m.i32(idx);
    m.release(4);
    (e, index)
}

/// The three keys of a colour of the render control record. All three take the
/// interpolation of the **first** colour curve (the original reads `curve1->interpolation`
/// three times).
fn control_colour_keys<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, at: u32) {
    let c = fcurve_check_add3(m, h, e, index, relid);
    for k in 0..3 {
        let v = m.f32(glob::SI_DEFAULT_CONTROL + at + 4 * k as u32);
        add_key_now(m, h, c[k], v, c[0]);
    }
}

fn control_key<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, index: i32, relid: i32, at: u32) {
    let v = m.f32(glob::SI_DEFAULT_CONTROL + at);
    savekey_single_fcurve(m, h, e, index, relid, v);
}

/// Save key, depth fading of the scene [soft 0x0821a464 SI_depthFadingSaveKey]: from the
/// render control record `SI_defaultControl`, relation 3000 ← +0x104 (start), 3001 ← +0x10c
/// (stop), 3003 ← +0x118, +0x11c, +0x120 (colour), 3002 ← +0x130 (transparency).
pub fn depth_fading_save_key<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H) {
    let (e, index) = scene_elem(m, h);
    if e == 0 {
        return;
    }
    control_key(m, h, e, index, 3000, ctl::FAD_START);
    control_key(m, h, e, index, 3001, ctl::FAD_STOP);
    control_colour_keys(m, h, e, index, 3003, ctl::FAD_COLOR);
    control_key(m, h, e, index, 3002, ctl::FAD_TRANSP);
}

/// Save key, layer fog of the scene [soft 0x0821a618 SI_layerFogSaveKey]: from
/// `SI_defaultControl`, relation 3030 ← +0x26c (start), 3031 ← +0x270 (stop), 3033 ← +0x284,
/// +0x288, +0x28c (colour, with the first colour curve's interpolation as in
/// [`depth_fading_save_key`]), 3032 ← +0x260 (density), 3034 ← +0x264 (base), 3035 ← +0x268
/// (thickness).
pub fn layer_fog_save_key<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H) {
    let (e, index) = scene_elem(m, h);
    if e == 0 {
        return;
    }
    control_key(m, h, e, index, 3030, ctl::FOG_START);
    control_key(m, h, e, index, 3031, ctl::FOG_STOP);
    control_colour_keys(m, h, e, index, 3033, ctl::FOG_COLOR);
    control_key(m, h, e, index, 3032, ctl::FOG_DENSITY);
    control_key(m, h, e, index, 3034, ctl::FOG_BASE);
    control_key(m, h, e, index, 3035, ctl::FOG_THICKNESS);
}

/// Save key, physical properties of a model [soft 0x0821cac4 SI_savekeyPhysicalProps]: from
/// the record at model + 0x2f4, relation 50 ← +0x14 (elasticity), 51 ← +0x0c (static
/// roughness), 52 ← +0x10 (kinetic roughness). Nothing without the record.
pub fn savekey_physical_props<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    let rec = m.ptr(mdl + model::DYNA_MATTER);
    let (e, index) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 || rec == 0 {
        return;
    }
    for (relid, at) in [(50, dyna::ELASTICITY), (51, dyna::STAT_ROUGH), (52, dyna::KINE_ROUGH)] {
        let v = m.f32(rec + at);
        savekey_single_fcurve(m, h, e, index, relid, v);
    }
}

/// Save key, the dynamics root constraint of a model
/// [soft 0x0821ca48 SI_savekeyDynamicSetup]: relation 53 ← the 16-bit word at +0x38 of the
/// record at model + 0x304, as a float.
pub fn savekey_dynamic_setup<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    let rec = m.ptr(mdl + model::DYNA_CHN_VARS);
    let (e, index) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 || rec == 0 {
        return;
    }
    let v = m.i16(rec + dyna::ROOT_CONS) as f32;
    savekey_single_fcurve(m, h, e, index, 53, v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ed::build as edb;
    use crate::host::{Arg, Ret, Trace};
    use crate::off::key as ko;

    /// A host that knows three relations and files new curves on the element.
    fn host(elem_of: Addr) -> Trace {
        Trace::with(move |m, name, args| match name {
            "reldict_get_rel_chap_grp" => {
                let (Arg::I(rel), Arg::I(out)) = (args[1], args[2]) else { panic!() };
                m.set_i32(out, 0);
                Ret::int(if (rel as i32) < 9000 { 7 } else { -1 })
            }
            "rel_get_fcv_type" => {
                let Arg::I(rel) = args[0] else { panic!() };
                Ret::int(match rel {
                    0x12d => 45,
                    0x12e => 48,
                    0x12f => 51,
                    r => 1000 + 4 * r as i32,
                })
            }
            "ed_get_elem_from_material" | "ed_get_elem_from_texture2d" => {
                let Arg::I(out) = args[1] else { panic!() };
                m.set_i32(out, 0);
                Ret::ptr(elem_of)
            }
            _ => Ret::default(),
        })
    }

    #[test]
    fn material_keys_in_order_with_new_curves() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        m.set_i32(glob::SI_FRAME_CURRENT, 15);
        let mats = glob::chap(&m, cid::MATERIALS);
        let mat = m.si_calloc(1, 0xdcc);
        let (e, _) = edb::add_elem(&mut m, d, mats, mat);
        let mdl = m.si_calloc(1, 0x430);
        let arr = m.si_calloc(1, 4);
        m.set_ptr(arr, mat);
        m.set_ptr(mdl + 0x20c, arr);
        m.set_f32(mat + 0x44, 0.25);
        m.set_f32(mat + 0x4c, 0.75);
        m.set_f32(mat + 0x6b8, 0.5);
        let mut t = host(e);
        savekey_material(&mut m, &mut t, mdl);
        assert_eq!(m.u8(mdl + 0xa), 0x40);
        let added: Vec<(u32, i32)> = t.calls
                                      .iter()
                                      .filter(|c| c.name == "ed_fcv_add")
                                      .map(|c| {
                                          let Arg::I(curve) = c.args[0] else { panic!() };
                                          (curve, fcv::kind(&m, curve))
                                      })
                                      .collect();
        let kinds: Vec<i32> = added.iter().map(|a| a.1).collect();
        assert_eq!(kinds, [45, 46, 47, 48, 49, 50, 51, 52, 53, 1000 + 4 * 0x130, 1000 + 4 * 0x131, 1000 + 4 * 0x133, 1000 + 4 * 0x136, 1000 + 4 * 0x137,
                           1000 + 4 * 0x138]);
        // the diffuse red key: frame 15 at 30 frames a second, value 0.25, slope 0, Hermite
        let red = added[3].0;
        assert_eq!(fcv::nb_keys(&m, red), 1);
        let k = m.ptr(red + fo::KEYS);
        assert_eq!((m.f32(k + ko::TIME), m.f32(k + ko::VALUE)), (0.5, 0.25));
        assert_eq!((m.f32(k + ko::RIGHT_SLOPE), m.i16(k + ko::SLOPE_TYPE), m.i16(k + ko::INTERP), m.u16(k + ko::FLAGS)), (0.0, 0, 2, 0));
        assert_eq!(m.ptr(red + fo::OWNER), mat);
        assert!(m.fault().is_none());
        // no material: a message, nothing else
        m.set_ptr(arr, 0);
        let mut t = host(e);
        savekey_material(&mut m, &mut t, mdl);
        assert_eq!(t.names(), ["status_display_message"]);
    }

    #[test]
    fn a_second_key_goes_into_the_existing_curve() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        let mats = glob::chap(&m, cid::MATERIALS);
        let mat = m.si_calloc(1, 0xdcc);
        let (_, mi) = edb::add_elem(&mut m, d, mats, mat);
        let c = fcv::make(&mut m, 54, 2, &[(0.0, 1.0)]);
        edb::attach_curve(&mut m, d, mats, mi, 0, 0x130, 5, c);
        let e = ed::get_elem(&m, d, mats, mi);
        m.set_i32(glob::SI_FRAME_CURRENT, 30);
        let mut t = host(e);
        savekey_single_fcurve(&mut m, &mut t, e, mi, 0x130, 4.0);
        assert!(!t.names().contains(&"ed_fcv_add"));
        let f = fcv::load(&m, c);
        assert_eq!(f.k().iter().map(|k| (k.time, k.value)).collect::<Vec<_>>(), [(0.0, 1.0), (1.0, 4.0)]);
        // the same frame again: the key is replaced, not added
        savekey_single_fcurve(&mut m, &mut t, e, mi, 0x130, 5.0);
        assert_eq!(fcv::load(&m, c).k().len(), 2);
        assert_eq!(fcv::load(&m, c).k()[1].value, 5.0);
        // removal at that time, then at the first key's: the curve's element goes with its last key
        fcurve_remove_key_from_time(&mut m, &mut t, c, 1.0, 9);
        assert_eq!(fcv::nb_keys(&m, c), 1);
        t.calls.clear();
        fcurve_remove_key_from_time(&mut m, &mut t, c, 0.0, 9);
        assert_eq!(fcv::nb_keys(&m, c), 0);
        // the curve code marks the fast-playback cache stale when it removes the key
        assert_eq!(t.names(), ["fast_set_dirty", "elemdict_find_elem_by_data", "elemdict_set_delete_elem"]);
        assert_eq!(t.calls[2].args[4..], [Arg::I(9), Arg::I(1), Arg::I(1)]);
    }

    #[test]
    fn texture_rotation_is_saved_in_degrees() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        let t2d = glob::chap(&m, cid::TEXTURES2D);
        let tex = m.si_calloc(1, 0x1d8);
        let (e, _) = edb::add_elem(&mut m, d, t2d, tex);
        m.set_f32(tex + 0x168, core::f32::consts::FRAC_PI_2);
        m.set_f32(tex + 0x94, core::f32::consts::PI);
        m.set_f32(tex + 0xac, 2.0);
        let mut t = host(e);
        sk_texture(&mut m, &mut t, tex);
        let value_of = |m: &Mem, t: &Trace, kind: i32| {
            let c = t.calls
                     .iter()
                     .filter(|c| c.name == "ed_fcv_add")
                     .map(|c| if let Arg::I(a) = c.args[0] { a } else { 0 })
                     .find(|&a| fcv::kind(m, a) == kind)
                     .unwrap();
            m.f32(m.ptr(c + fo::KEYS) + ko::VALUE)
        };
        assert_eq!(value_of(&m, &t, 1000 + 4 * 0x1a7), 90.0);
        assert_eq!(value_of(&m, &t, 1000 + 4 * 0x1a8), 180.0);
        assert_eq!(value_of(&m, &t, 1000 + 4 * 0x1a9), 2.0);
        // 13 single parameters and three triples
        assert_eq!(t.calls.iter().filter(|c| c.name == "ed_fcv_add").count(), 13 + 9);
        assert!(m.fault().is_none());
    }
}
