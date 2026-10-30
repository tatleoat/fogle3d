// SPDX-License-Identifier: MIT
//! `mat_validation.c`, the parameter validators of materials, textures and lights that the editor's
//! "set parameter" routines run after storing a value (finding 0012, section 33): 22 of its 41
//! routines (the other 19, `SI_validRenControl*`, belong to the render-control set-up).
//!
//! Each takes the record the parameter sits in and the new value as stack words and answers 0, or
//! 4 when the change is refused. The callees outside the file are hooks written `name (0xADDRESS)`.

use si_mem::Mem;
use si_x87::{x, F80};

si_mem::outside_calls! {
    /// What `mat_validation.c` calls in other source files and nobody has ported here: a hook answers 0.
    pub trait MatValidationCalls {
        /// `SI_texture2DSetDirty (0x082a8fc0)`, `t2d_dial.c`: (texture) marks the map's models and materials changed.
        fn si_texture2d_set_dirty(texture: u32) = "SI_texture2DSetDirty", 0x082a8fc0;
        /// `SI_updTxt2dEffects (0x082b289c)`, `t2d_fct.c` (ported on values in `crate::t2d_fct`): (texture).
        fn si_upd_txt2d_effects(texture: u32) = "SI_updTxt2dEffects", 0x082b289c;
        /// `SI_txt2dSetWrap (0x082ad9f8)`, `t2d_dial.c`: (flag, texture).
        fn si_txt2d_set_wrap(flag: i32, texture: u32) = "SI_txt2dSetWrap", 0x082ad9f8;
        /// `SI_pictureGetDimensions (0x085e30c4)`, `util.c` (ported in `si-picture`): (name, &width, &height), two
        /// 16-bit stores; 0 on success, 4 when the picture cannot be opened.
        fn si_picture_get_dimensions(name: u32, width: u32, height: u32) -> i32 = "SI_pictureGetDimensions", 0x085e30c4;
        /// `SI_edGetElemFromLight (0x080fd0bc)`, `edtools.c` (ported in `si-scene`): (light, &element).
        fn si_ed_get_elem_from_light(light: u32, elem: u32) = "SI_edGetElemFromLight", 0x080fd0bc;
        /// `SI_elemdictRemoveRelationId (0x08133998)`, `elemdict.c` (ported in `si-dict`): (dictionary, relation
        /// dictionary, element, relation id).
        fn si_elemdict_remove_relation_id(dict: u32, rd: u32, elem: u32, id: i32) = "SI_elemdictRemoveRelationId", 0x08133998;
        /// `SI_matrixXyzRotation (0x0851882c)`, `algebra.c` (ported in `si-math`): (matrix, angles).
        fn si_matrix_xyz_rotation(matrix: u32, angles: u32) = "SI_matrixXyzRotation", 0x0851882c;
        /// `SI_setAreaRect (0x08330450)`, `graflist.c`: (light) the corners of a rectangular area light.
        fn si_set_area_rect(light: u32) = "SI_setAreaRect", 0x08330450;
        /// `SI_setAreaDisk (0x083305a0)`, `graflist.c`: (light) the points of a disk-shaped area light.
        fn si_set_area_disk(light: u32) = "SI_setAreaDisk", 0x083305a0;
        /// `SI_lightSunGetPosition (0x082a6c40)`, `sun_fct.c` (ported in `crate::sun_position`): (light, &position).
        fn si_light_sun_get_position(light: u32, pos: u32) = "SI_lightSunGetPosition", 0x082a6c40;
    }
}

/// `SI_procData` (0x08679314): the pointer to the word that says which dictionary set is current.
pub const SI_PROC_DATA: u32 = 0x0867_9314;
/// `SI_elemDicts` (0x0867931c): the array of element dictionaries, 0x70 bytes each.
pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
/// `SI_RD` (0x087815f0): the relation dictionary.
pub const SI_RD: u32 = 0x0878_15f0;

/// A float through the x87 register and back: what a load and store of a single does to a signalling NaN.
fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// [soft 0x0828ca60 SI_validMaterialBlurDecay]: `(material, value)`: the blur decay (+0x760) is the value, or 0 when
/// the value is below 0 (a NaN is kept). Answers 0.
pub fn valid_material_blur_decay(m: &mut Mem, mat: u32, value: f32) -> i32 {
    let v = if x(value).lt(F80::ZERO) { 0.0 } else { value };
    m.set_f32(mat + 0x760, through_x87(v));
    0
}

/// [soft 0x0828ca8c SI_validMaterialBlurWidth]: as the decay, for the blur width (+0x75c).
pub fn valid_material_blur_width(m: &mut Mem, mat: u32, value: f32) -> i32 {
    let v = if x(value).lt(F80::ZERO) { 0.0 } else { value };
    m.set_f32(mat + 0x75c, through_x87(v));
    0
}

/// Whether a mapping method (16 bits) is 7 or 8, the reflection maps.
fn reflection_method(method: u16) -> bool {
    method.wrapping_sub(7) <= 1
}

/// [soft 0x0828cab8 SI_validTxt2dMethod]: `(texture, method)`. With the lock word (+0x198) set the method may change
/// between the reflection methods (7, 8) or between the others, not across: 4 then. Else the method (+0x2a, 16 bits)
/// is stored, the map marked changed, and for a reflection method the blend (+0x10c) and the factors (+0x128, +0x12c,
/// +0x130, +0x138, +0x134, +0x140) are cleared; the update word (+0x194) becomes 7, the map's effects are recomputed
/// and its wrap set (0). Answers 0.
pub fn valid_txt2d_method(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, method: i32) -> i32 {
    let new = method as u16;
    if m.u32(t + 0x198) != 0 && reflection_method(m.u16(t + 0x2a)) != reflection_method(new) {
        return 4;
    }
    m.set_u16(t + 0x2a, new);
    o.si_texture2d_set_dirty(m, t);
    if reflection_method(m.u16(t + 0x2a)) {
        for at in [0x10c, 0x128, 0x12c, 0x130, 0x138, 0x134, 0x140] {
            m.set_u32(t + at, 0);
        }
    }
    m.set_u16(t + 0x194, 7);
    o.si_upd_txt2d_effects(m, t);
    o.si_txt2d_set_wrap(m, 0, t);
    0
}

/// [soft 0x0828cb8c SI_validTxt2dURepeat]: `(texture, count)`: the u repeat (+0xf8, 16 bits) is the count, or 1 when
/// the low 16 bits are not above 0 (signed); the map is marked changed. Answers 0.
pub fn valid_txt2d_u_repeat(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, count: i32) -> i32 {
    let c = count as i16;
    m.set_i16(t + 0xf8, if c > 0 { c } else { 1 });
    o.si_texture2d_set_dirty(m, t);
    0
}

/// [soft 0x0828cbbc SI_validTxt2dVRepeat]: as the u repeat, for the v repeat (+0xfa).
pub fn valid_txt2d_v_repeat(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, count: i32) -> i32 {
    let c = count as i16;
    m.set_i16(t + 0xfa, if c > 0 { c } else { 1 });
    o.si_texture2d_set_dirty(m, t);
    0
}

/// Pulls a pair of 16-bit crop limits (lo, hi) into `0 ..= max` and in order, and keeps them apart: equal limits (not
/// both 0) move the high one up, or the low one down when it already is at `max`. `lo_at`/`hi_at` are the record's
/// offsets of the pair. The common body of the two crop validators, in the order their code does it.
fn crop_pair(m: &mut Mem, t: u32, lo_at: u32, hi_at: u32, max: u16) {
    for at in [lo_at, hi_at] {
        let v = m.i16(t + at);
        if v < 0 {
            m.set_i16(t + at, 0);
        } else if v > max as i16 {
            m.set_u16(t + at, max);
        }
    }
    let (a, b) = (m.i16(t + lo_at), m.i16(t + hi_at));
    if a > b {
        m.set_i16(t + lo_at, b);
        m.set_i16(t + hi_at, a);
    }
    // `cmp DWORD [lo], 0`: both words at once
    if m.u32(t + lo_at) != 0 {
        let lo = m.u16(t + lo_at);
        if lo == m.u16(t + hi_at) {
            if lo == max {
                m.set_u16(t + lo_at, lo.wrapping_sub(1));
            } else {
                m.set_u16(t + hi_at, lo.wrapping_add(1));
            }
        }
    }
}

/// The picture's size minus one, as 16-bit words: `(width - 1, height - 1)`, or the callee's refusal.
fn picture_limits(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32) -> Result<(u16, u16), i32> {
    let loc = m.local(4);
    let pic = m.u32(t + 0x20);
    let r = o.si_picture_get_dimensions(m, pic, loc, loc + 2);
    if r != 0 {
        return Err(r);
    }
    Ok((m.u16(loc).wrapping_sub(1), m.u16(loc + 2).wrapping_sub(1)))
}

/// Whether the crop window (+0x30, +0x32, +0x34, +0x36) is smaller than the picture, as the word +0x2e.
fn crop_flag(m: &mut Mem, t: u32, wm1: u16, hm1: u16) {
    let on = m.u16(t + 0x30) != 0 || m.u16(t + 0x32) != wm1 || m.u16(t + 0x34) != 0 || m.u16(t + 0x36) != hm1;
    m.set_u16(t + 0x2e, on as u16);
}

/// [soft 0x0828cbec SI_validTxt2dCropU]: `(texture)`: the crop window's u limits (+0x30, +0x32) are brought into the
/// picture's width (`SI_pictureGetDimensions` of the map's picture, +0x20), put in order and kept apart; the crop
/// flag (+0x2e) says whether the window is smaller than the picture; the map is marked changed, +0x194 becomes 7.
/// Answers 0, or the dimensions routine's answer when it fails (nothing changed then).
pub fn valid_txt2d_crop_u(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32) -> i32 {
    let sp = m.sp();
    let r = match picture_limits(m, o, t) {
        Err(r) => r,
        Ok((wm1, hm1)) => {
            crop_pair(m, t, 0x30, 0x32, wm1);
            crop_flag(m, t, wm1, hm1);
            o.si_texture2d_set_dirty(m, t);
            m.set_u16(t + 0x194, 7);
            0
        }
    };
    m.set_sp(sp);
    r
}

/// [soft 0x0828ccdc SI_validTxt2dCropV]: `(texture, value, high)`: the v limit counted from the bottom: the high limit
/// (+0x36) when `high` (low 16 bits) is not 0, else the low (+0x34), becomes height - 1 - value; then as the u crop
/// for +0x34 and +0x36 against height - 1. Answers 0, or the dimensions routine's answer when it fails.
pub fn valid_txt2d_crop_v(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, value: i32, high: i32) -> i32 {
    let sp = m.sp();
    let r = match picture_limits(m, o, t) {
        Err(r) => r,
        Ok((wm1, hm1)) => {
            let v = hm1.wrapping_sub(value as u16);
            m.set_u16(t + if high as u16 != 0 { 0x36 } else { 0x34 }, v);
            crop_pair(m, t, 0x34, 0x36, hm1);
            crop_flag(m, t, wm1, hm1);
            o.si_texture2d_set_dirty(m, t);
            m.set_u16(t + 0x194, 7);
            0
        }
    };
    m.set_sp(sp);
    r
}

/// [soft 0x0828cdf4 SI_validTxt2dWindowScal]: `(texture, &scale, value)`: the scale is the value, or 1.0 when below
/// 1.0; the map is marked changed. Answers 0.
pub fn valid_txt2d_window_scal(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, scale: u32, value: f32) -> i32 {
    let v = if x(value).lt(F80::ONE) { 1.0 } else { value };
    m.set_f32(scale, through_x87(v));
    o.si_texture2d_set_dirty(m, t);
    0
}

/// [soft 0x0828ce28 SI_validTxt2dWindowTrans]: `(texture, &offset, value)`: the offset is the value held between -1.0
/// and 1.0 (a NaN is kept); the map is marked changed. Answers 0.
pub fn valid_txt2d_window_trans(m: &mut Mem, o: &mut dyn si_mem::Outside, t: u32, offset: u32, value: f32) -> i32 {
    m.set_f32(offset, through_x87(value));
    if x(value).lt(x(-1.0)) {
        m.set_f32(offset, -1.0);
    } else if F80::ONE.lt(x(value)) {
        m.set_u32(offset, 0x3f80_0000);
    }
    o.si_texture2d_set_dirty(m, t);
    0
}

/// [soft 0x0828ce7c SI_validTxt3dNbIter]: `(texture, count)`: the iteration count (+0x1c) is the count held in 1 to 8.
/// Answers 0.
pub fn valid_txt3d_nb_iter(m: &mut Mem, t: u32, count: i32) -> i32 {
    m.set_i32(t + 0x1c, if count <= 0 { 1 } else if count > 8 { 8 } else { count });
    0
}

/// [soft 0x0828cea8 SI_validTxt3dFct1]: `(texture, value)`: the lower bound (+0x80) is the value, 0 when below 0, and
/// not above the middle one (+0x84). Answers 0.
pub fn valid_txt3d_fct1(m: &mut Mem, t: u32, value: f32) -> i32 {
    m.set_f32(t + 0x80, through_x87(value));
    if x(value).lt(F80::ZERO) {
        m.set_u32(t + 0x80, 0);
    } else if x(m.f32(t + 0x84)).lt(x(value)) {
        let mid = m.f32(t + 0x84);
        m.set_f32(t + 0x80, mid);
    }
    0
}

/// [soft 0x0828cefc SI_validTxt3dFct2]: `(texture, value)`: the middle bound (+0x84) is the value, pulled up to the
/// lower (+0x80) when below it, or down to the upper (+0x88) when above it. Answers 0.
pub fn valid_txt3d_fct2(m: &mut Mem, t: u32, value: f32) -> i32 {
    m.set_f32(t + 0x84, through_x87(value));
    let (lo, hi) = (m.f32(t + 0x80), m.f32(t + 0x88));
    if x(lo).gt(x(value)) {
        m.set_f32(t + 0x84, lo);
    } else if x(hi).lt(x(value)) {
        m.set_f32(t + 0x84, hi);
    }
    0
}

/// [soft 0x0828cf44 SI_validTxt3dFct3]: `(texture, value)`: the upper bound (+0x88) is the value, pulled up to the
/// middle one (+0x84) when below it, or down to 1.0 when above 1.0. Answers 0.
pub fn valid_txt3d_fct3(m: &mut Mem, t: u32, value: f32) -> i32 {
    m.set_f32(t + 0x88, through_x87(value));
    let mid = m.f32(t + 0x84);
    if x(mid).gt(x(value)) {
        m.set_f32(t + 0x88, mid);
    } else if F80::ONE.lt(x(value)) {
        m.set_u32(t + 0x88, 0x3f80_0000);
    }
    0
}

/// [soft 0x0828cf90 SI_validLightShadowType]: `(light, type)`: the shadow type (+0x8c, 16 bits) is stored, except that
/// 3 and 4 are refused (4) for a light that is not of kind 2. Answers 0 or 4.
pub fn valid_light_shadow_type(m: &mut Mem, light: u32, ty: i32) -> i32 {
    if m.i16(light + 0x1e) != 2 && (ty as u16).wrapping_sub(3) <= 1 {
        return 4;
    }
    m.set_u16(light + 0x8c, ty as u16);
    0
}

/// [soft 0x0828cfc0 SI_validLightMapRes]: `(light, value)`: the shadow map resolution (+0x118) is the value, or 1 when
/// not above 0. Answers 0.
pub fn valid_light_map_res(m: &mut Mem, light: u32, value: i32) -> i32 {
    m.set_i32(light + 0x118, if value > 0 { value } else { 1 });
    0
}

/// [soft 0x0828cfe0 SI_validLightFilterStep]: `(light, value)`: the filter step (+0x124) likewise. Answers 0.
pub fn valid_light_filter_step(m: &mut Mem, light: u32, value: i32) -> i32 {
    m.set_i32(light + 0x124, if value > 0 { value } else { 1 });
    0
}

/// [soft 0x0828d000 SI_lightValidateSelective]: `(light)`: 4 for no light. A light whose selective word (+0x15c) is 0
/// loses its selection: the relation 0x898 of its element (`SI_edGetElemFromLight`) is removed from the current
/// element dictionary, the list (+0x168) freed, the list and its count (+0x164) cleared and +0x160 set to 1. Answers 0.
pub fn light_validate_selective(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) -> i32 {
    if light == 0 {
        return 4;
    }
    if m.u32(light + 0x15c) == 0 {
        let sp = m.sp();
        let elem = m.local(4);
        o.si_ed_get_elem_from_light(m, light, elem);
        let proc = m.u32(SI_PROC_DATA);
        let dict = m.u32(proc).wrapping_mul(7).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS));
        let e = m.u32(elem);
        o.si_elemdict_remove_relation_id(m, dict, SI_RD, e, 0x898);
        let list = m.u32(light + 0x168);
        if list != 0 {
            m.free(list);
            m.set_u32(light + 0x168, 0);
        }
        m.set_u32(light + 0x168, 0);
        m.set_u32(light + 0x164, 0);
        m.set_u32(light + 0x160, 1);
        m.set_sp(sp);
    }
    0
}

/// The light is an area light: a spot (1) or infinite (2) light with the area word (+0x182) set.
fn is_area_light(m: &Mem, light: u32) -> bool {
    light != 0 && m.u16(light + 0x1e).wrapping_sub(1) <= 1 && m.u16(light + 0x182) != 0
}

/// [soft 0x0828d0ec SI_lightValidateAreaType]: `(light)`: 4 unless the light is an area light. By the shape (+0x184):
/// 0 nothing; 1 the size (+0x190) is the light's (+0x1c4) float; 2 the same into +0x19c; more is refused (4). Answers 0.
pub fn light_validate_area_type(m: &mut Mem, light: u32) -> i32 {
    if !is_area_light(m, light) {
        return 4;
    }
    match m.i32(light + 0x184) {
        0 => {}
        1 => m.set_f32(light + 0x190, through_x87(m.f32(light + 0x1c4))),
        2 => m.set_f32(light + 0x19c, through_x87(m.f32(light + 0x1c4))),
        _ => return 4,
    }
    0
}

/// [soft 0x0828d0a0 SI_lightValidateAreaLight]: `(light)`: 4 unless the light is a spot (1) or infinite (2) light. With
/// the area word equal to 1 and the shape 3, the shape becomes 0 and the area type is validated (its answer is not
/// used). Answers 0.
pub fn light_validate_area_light(m: &mut Mem, light: u32) -> i32 {
    if light == 0 || m.u16(light + 0x1e).wrapping_sub(1) > 1 {
        return 4;
    }
    if m.i16(light + 0x182) == 1 && m.i32(light + 0x184) == 3 {
        m.set_i32(light + 0x184, 0);
        light_validate_area_type(m, light);
    }
    0
}

/// [soft 0x0828d158 SI_lightValidateAreaRotation]: `(light)`: 4 unless the light is an area light. The rotation
/// angles (+0x1e0) are taken from the matrix (+0x1d0); then by the shape (+0x184): 0 the rectangle's corners
/// (`SI_setAreaRect`), 1 nothing, 2 the disk's points (`SI_setAreaDisk`), else 4.
pub fn light_validate_area_rotation(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) -> i32 {
    if !is_area_light(m, light) {
        return 4;
    }
    o.si_matrix_xyz_rotation(m, light + 0x1d0, light + 0x1e0);
    match m.i32(light + 0x184) {
        0 => o.si_set_area_rect(m, light),
        1 => {}
        2 => o.si_set_area_disk(m, light),
        _ => return 4,
    }
    0
}

/// [soft 0x0828d1e0 SI_lightValidateAreaScaling]: `(light)`: 4 unless the light is an area light. By the shape (+0x184):
/// 0 the rectangle's corners; 1 the size (+0x190) is the light's float (+0x1c4); 2 the same into +0x19c and the disk's
/// points; else 4.
pub fn light_validate_area_scaling(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) -> i32 {
    if !is_area_light(m, light) {
        return 4;
    }
    match m.i32(light + 0x184) {
        0 => o.si_set_area_rect(m, light),
        1 => m.set_f32(light + 0x190, through_x87(m.f32(light + 0x1c4))),
        2 => {
            m.set_f32(light + 0x19c, through_x87(m.f32(light + 0x1c4)));
            o.si_set_area_disk(m, light);
        }
        _ => return 4,
    }
    0
}

/// [soft 0x0828d268 SI_lightSunValidateDateTime]: `(light)`: 4 unless the light is a sun (kind 4) with its sun record
/// (+0x16c); else the sun's position (+0x34) is computed from its date and time (`SI_lightSunGetPosition`). Answers 0.
pub fn light_sun_validate_date_time(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) -> i32 {
    if light == 0 || m.u16(light + 0x1e) != 4 || m.u32(light + 0x16c) == 0 {
        return 4;
    }
    o.si_light_sun_get_position(m, light, light + 0x34);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::{Ret, Trace};

    const T: u32 = 0x6100_0000;

    fn quiet() -> Trace {
        Trace::with(|_, _, _| Ret::int(0))
    }

    #[test]
    fn blur_negative_is_zero() {
        let mut m = Mem::new();
        valid_material_blur_width(&mut m, T, -2.5);
        valid_material_blur_decay(&mut m, T, 3.5);
        assert_eq!((m.f32(T + 0x75c), m.f32(T + 0x760)), (0.0, 3.5));
    }

    #[test]
    fn method_lock_and_reflection_clear() {
        let mut m = Mem::new();
        let mut o = quiet();
        m.set_u32(T + 0x198, 1);
        m.set_u16(T + 0x2a, 7);
        assert_eq!(valid_txt2d_method(&mut m, &mut o, T, 2), 4);
        m.set_u32(T + 0x128, 5);
        assert_eq!(valid_txt2d_method(&mut m, &mut o, T, 8), 0);
        assert_eq!((m.u16(T + 0x2a), m.u32(T + 0x128), m.u16(T + 0x194)), (8, 0, 7));
        let names: Vec<_> = o.calls.iter().map(|c| c.routine.name).collect();
        assert_eq!(names, ["SI_texture2DSetDirty", "SI_updTxt2dEffects", "SI_txt2dSetWrap"]);
    }

    #[test]
    fn crop_limits_ordered_and_apart() {
        let mut m = Mem::new();
        let mut o = Trace::with(|m, r, a| {
            if r.name == "SI_pictureGetDimensions" {
                m.set_u16(a[1], 100);
                m.set_u16(a[2], 50);
            }
            Ret::int(0)
        });
        m.set_i16(T + 0x30, 90);
        m.set_i16(T + 0x32, -4);
        assert_eq!(valid_txt2d_crop_u(&mut m, &mut o, T), 0);
        assert_eq!((m.i16(T + 0x30), m.i16(T + 0x32)), (0, 90));
        m.set_i16(T + 0x30, 99);
        m.set_i16(T + 0x32, 200);
        valid_txt2d_crop_u(&mut m, &mut o, T);
        assert_eq!((m.i16(T + 0x30), m.i16(T + 0x32)), (98, 99));
    }

    #[test]
    fn txt3d_bounds() {
        let mut m = Mem::new();
        m.set_f32(T + 0x84, 0.5);
        valid_txt3d_fct1(&mut m, T, 0.75);
        assert_eq!(m.f32(T + 0x80), 0.5);
        valid_txt3d_fct3(&mut m, T, 4.0);
        assert_eq!(m.f32(T + 0x88), 1.0);
    }

    #[test]
    fn shadow_type_refusal() {
        let mut m = Mem::new();
        m.set_u16(T + 0x1e, 1);
        assert_eq!(valid_light_shadow_type(&mut m, T, 3), 4);
        m.set_u16(T + 0x1e, 2);
        assert_eq!(valid_light_shadow_type(&mut m, T, 3), 0);
        assert_eq!(m.u16(T + 0x8c), 3);
    }
}
