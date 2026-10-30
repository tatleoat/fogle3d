// SPDX-License-Identifier: MIT
//! The parameter readers and writers of `edparams.c` for the chapters other than a model's: waves, cameras, materials,
//! 2-D and 3-D textures, lights and the render control (the scene). A model's own parameters stay in [`super::elem`]. `SI_edElementStaticValueGet` and `…Set` ([`super::elem`]) hand a chapter's object, the
//! parameter's curve and a value record (`SI_AttributeValue`: the value type at +0, the value at +4, a second word at +8) to
//! one of them.
//!
//! Every routine here reads and writes the original's records on the image. Where the original copies a float through the
//! x87 unit (`fld` then `fstp`) the copy is made through the register ([`fcopy`]), which turns a signalling NaN into a quiet
//! one; where it moves the word (`mov`) the bits are kept.
//!
//! `SI_edSumatraTo3x` (the export to the successor product, 0 on the full product's path) is not taken: the branches the
//! original has for it being non-zero (the scene's current element at 0x08680e60, the globals `SI_gmrCamera`,
//! `SI_gSoftMaterial`, `SI_gSoftColor3D`, `SI_gSoftLight`) are left out, and a comment says so at each place.

use super::calls::ExprCalls;
use super::elem::{current_time, fcurve, fcurve_frame_value, DEG_TO_RAD, RAD_TO_DEG, STATIC_EPS};
use super::rec::{self, cid};
use si_mem::{Mem, Outside, Ptr32};
use si_x87::{Round, F80};

/// A float copied through the x87 register (`fld`, `fstp`): a signalling NaN comes out quiet.
pub(super) fn fq(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// `fld dword [src]; fstp dword [dst]`.
pub(super) fn fcopy(m: &mut Mem, dst: u32, src: u32) {
    let v = m.f32(src);
    m.set_f32(dst, fq(v));
}

/// `fld dword [src]; fmul qword [k]; fstp dword [dst]`.
pub(super) fn fscaled(m: &mut Mem, dst: u32, src: u32, k: f64) {
    let v = (F80::from_f32(m.f32(src)) * F80::from_f64(k)).to_f32();
    m.set_f32(dst, v);
}

/// The first words of a value record: the answer starts as "the value 0, type 1".
pub(super) fn start_value(m: &mut Mem, out: u32) {
    m.set_u32(out + 4, 0);
    m.set_u32(out, 1);
}

/// A curve's static value is dropped (+0x60 cleared) when the current time lies further than 0.0001 from the time it was
/// set at (+0x10): the head of `SI_waveGetParam`, `SI_camGetParam`, `SI_modelGetParam` and others.
pub(super) fn drop_stale_static(m: &mut Mem, fcv: u32) {
    if fcv != 0 && m.i16(fcv + fcurve::STATIC) != 0 {
        let d = (current_time(m) - F80::from_f32(m.f32(fcv + fcurve::LAST_TIME))).abs();
        if F80::from_f64(STATIC_EPS).lt(d) {
            m.set_i16(fcv + fcurve::STATIC, 0);
        }
    }
}

/// A parameter that is read from its curve: none without a curve (false); the curve's value at the current frame when the
/// curve is evaluated (+0x20) and holds no static value, else the curve's last value (+0x14).
pub(super) fn curve_value_out(m: &mut Mem, fcv: u32, out: u32) -> bool {
    if fcv == 0 {
        return false;
    }
    if m.i16(fcv + fcurve::STATIC) == 0 && m.i16(fcv + fcurve::ACTIVE) != 0 {
        let frame = m[rec::SI_FRAME_CURRENT];
        let v = fcurve_frame_value(m, fcv, frame).to_f32();
        m.set_f32(out + 4, v);
    } else {
        fcopy(m, out + 4, fcv + fcurve::LAST_VALUE);
    }
    true
}

/// The value of a curve stored as its static value at the current time (`fld` of the record's value, the time held on the
/// register since the head of the function, `fstp` of both): +0x14 the value, +0x60 set, +0x10 the time.
pub(super) fn curve_static_set(m: &mut Mem, fcv: u32, value: u32, time: f32) {
    fcopy(m, fcv + fcurve::LAST_VALUE, value + 4);
    m.set_i16(fcv + fcurve::STATIC, 1);
    m.set_f32(fcv + fcurve::LAST_TIME, time);
}

/// `|v| < 0.0001` replaced by ±0.0001 (single precision constants), the sign taken from `v`: the clamp the camera's lens
/// parameters get. A NaN is kept. `v` is the register value (a signalling NaN of the record is quiet already).
fn clamp_small(v: F80) -> f32 {
    let small = v.abs().compare(F80::from_f64(STATIC_EPS)) == Some(core::cmp::Ordering::Less);
    if small {
        if v.compare(F80::ZERO) == Some(core::cmp::Ordering::Less) {
            -9.999999747378752e-05_f32
        } else {
            9.999999747378752e-05_f32
        }
    } else {
        v.to_f32()
    }
}

/// A value record's number as a truth value [soft 0x080f0be8 SI_getAttrValInBoolean]: for value type 1 the float at +4, for
/// type 9 the float at +8, true unless it equals zero (a NaN is true); for types 2 and 4 the signed 16-bit word at +4 itself;
/// 0 for any other type.
pub fn get_attr_val_in_boolean(m: &Mem, v: u32) -> i32 {
    match m.i32(v) {
        2 | 4 => m.i16(v + 4) as i32,
        t @ (1 | 9) => {
            let x = F80::from_f32(m.f32(v + if t == 1 { 4 } else { 8 }));
            (x.compare(F80::ZERO) != Some(core::cmp::Ordering::Equal)) as i32
        }
        _ => 0,
    }
}

/// A model's matte value set on it and on its whole branch [soft 0x080f3c00 SI_modelPropagateMatteParam]: the float at +0x3f4
/// of the model and, going down the sons (first son +0x2c4, next brother +0x2c0), of each of them.
pub fn model_propagate_matte_param(m: &mut Mem, mdl: u32, v: f32) {
    m.set_f32(mdl + 0x3f4, fq(v));
    let mut son = m.u32(mdl + 0x2c4);
    while son != 0 {
        model_propagate_matte_param(m, son, v);
        son = m.u32(son + 0x2c0);
    }
}

/// A wave's icon model [soft 0x084c6f00 SI_waveGetIconModel] is the word at +0x74 (`si-dynamics` has it; this crate does not
/// depend on that one).
fn wave_icon(m: &Mem, wave: u32) -> u32 {
    m.u32(wave + 0x74)
}

/// One parameter of a wave into a value record [soft 0x080f1930 SI_waveGetParam]: the record starts as the float 0; a stale static
/// value of the curve is dropped; the icon model's scaling (0xa3 to 0xa5: +0x100 +0x104 +0x108), rotation (0xa6 to 0xa8: +0x10c
/// +0x110 +0x114, times 57.29577951308232 in the register) and translation (0xa9 to 0xab: +0x158 +0x15c +0x160); the curve's value
/// for 0xac to 0xae (0 without a curve). The two parameters of the export to the successor product (0x10b, 0x237) leave the record
/// as it is. Any other type answers 0.
pub fn wave_get_param(m: &mut Mem, fcv_type: i32, wave: u32, fcv: u32, out: u32) -> u32 {
    start_value(m, out);
    drop_stale_static(m, fcv);
    let icon = wave_icon(m, wave);
    match fcv_type {
        0xa3..=0xa5 | 0xa9..=0xab => {
            let off = match fcv_type {
                0xa3 => 0x100,
                0xa4 => 0x104,
                0xa5 => 0x108,
                0xa9 => 0x158,
                0xaa => 0x15c,
                _ => 0x160,
            };
            fcopy(m, out + 4, icon + off);
        }
        0xa6..=0xa8 => fscaled(m, out + 4, icon + 0x10c + 4 * (fcv_type - 0xa6) as u32, RAD_TO_DEG),
        0xac..=0xae => {
            if !curve_value_out(m, fcv, out) {
                return 0;
            }
        }
        // SI_edSumatraTo3x is 0: nothing more is read
        0x10b | 0x237 => {}
        _ => return 0,
    }
    1
}

/// One parameter of a wave from a value record [soft 0x080f4b70 SI_waveSetParam]: the same fields as [`wave_get_param`] (the
/// rotation in degrees times 0.0174532925 in the register); for 0xac to 0xae the record's value becomes the curve's static value
/// at the current time (0 without a curve), and for 0xac the wave is placed on its path (`SI_setPathTranslation(0x264, SI_cid.WAVES,
/// wave, curve)`). Any other type answers 0.
pub fn wave_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, wave: u32, fcv: u32, value: u32) -> u32 {
    // SI_frameToSec(SI_frameCurrent), stored to a float
    let time = current_time(m).to_f32();
    let icon = wave_icon(m, wave);
    match fcv_type {
        0xa3..=0xa5 | 0xa9..=0xab => {
            let off = match fcv_type {
                0xa3 => 0x100,
                0xa4 => 0x104,
                0xa5 => 0x108,
                0xa9 => 0x158,
                0xaa => 0x15c,
                _ => 0x160,
            };
            fcopy(m, icon + off, value + 4);
        }
        0xa6..=0xa8 => fscaled(m, icon + 0x10c + 4 * (fcv_type - 0xa6) as u32, value + 4, DEG_TO_RAD),
        0xac..=0xae => {
            if fcv == 0 {
                return 0;
            }
            curve_static_set(m, fcv, value, time);
            if fcv_type == 0xac {
                let wav = rec::chap(m, cid::WAV) as u32;
                o.set_path_translation(m, 0x264, wav, wave, fcv);
            }
        }
        _ => return 0,
    }
    1
}

/// The camera's float fields a type copies as they are: (type, offset). 0x172 is the pixel ratio the reader copies from +0x64
/// (the writer does more, see [`cam_set_param`]).
const CAM_FLOATS: [(i32, u32); 18] = [
    (0x8e, 0x10),
    (0x8f, 0x14),
    (0x90, 0x18),
    (0x91, 0x1c),
    (0x92, 0x20),
    (0x93, 0x24),
    (0xd, 0x34),
    (0xe, 0x38),
    (0xf, 0x70),
    (0x10, 0x94),
    (0x11, 0x74),
    (0x12, 0x7c),
    (0x13, 0x84),
    (0x14, 0x8c),
    (0x15, 0x98),
    (0x172, 0x64),
    (0x179, 0x11c),
    (0x178, 0x118),
];

/// The float fields of the camera record that the read copies as they are besides [`CAM_FLOATS`]: the interest-to-lens values.
const CAM_LENS: [(i32, u32); 3] = [(0x175, 0x10c), (0x176, 0x110), (0x177, 0x114)];

/// One parameter of a camera into a value record [soft 0x080f1b64 SI_camGetParam]. The record starts as the float 0 and a stale
/// static value of the curve is dropped. Types 9 and 10 (the camera's translation curves) read the curve; 0xb (rotation +0x30)
/// and 0xc (field of view +0x40) are floats times 57.29577951308232 in the register; the other floats are copied from the fields
/// of [`CAM_FLOATS`] and [`CAM_LENS`]; 0x233 and 0x17a are 16-bit values of type 2 (+0x3c, +0x108); 0xb1 (whether the camera is
/// shown) is type 2, 1 when the camera's graf node (`SI_findGlData`, kind 4, in the current list) has display type 2, left 0
/// without a node; 0x234 (the projection mode, +0x6c) fills the second word, the first stays 1. The export-to-the-successor types
/// (0x10b, 0x237) leave the record as it is. Any other type answers 0.
pub fn cam_get_param(m: &mut Mem, fcv_type: i32, cam: u32, fcv: u32, out: u32) -> u32 {
    start_value(m, out);
    drop_stale_static(m, fcv);
    match fcv_type {
        9 | 0xa => {
            if !curve_value_out(m, fcv, out) {
                return 0;
            }
        }
        0xb => fscaled(m, out + 4, cam + 0x30, RAD_TO_DEG),
        0xc => fscaled(m, out + 4, cam + 0x40, RAD_TO_DEG),
        0x233 => {
            m.set_u32(out, 2);
            let v = m.u16(cam + 0x3c);
            m.set_u16(out + 4, v);
        }
        0x17a => {
            m.set_u32(out, 2);
            let v = m.u16(cam + 0x108);
            m.set_u16(out + 4, v);
        }
        0xb1 => {
            m.set_u32(out, 2);
            // SI_edSumatraTo3x is 0: the camera's graf node in the current list
            let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
            let gn = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(cam), 4).addr();
            if gn != 0 {
                let v = (m.i32(gn + 0xc) == 2) as u16;
                m.set_u16(out + 4, v);
            }
        }
        0x234 => {
            // SI_edSumatraTo3x is 0
            let v = m.u32(cam + 0x6c);
            m.set_u32(out + 8, v);
        }
        0x10b | 0x237 => {}
        t => {
            let off = CAM_FLOATS.iter().chain(CAM_LENS.iter()).find(|(k, _)| *k == t).map(|(_, o)| *o);
            match off {
                Some(off) => fcopy(m, out + 4, cam + off),
                None => return 0,
            }
        }
    }
    1
}

/// The lens values a camera's set routine clamps away from zero and then recomputes the field of view from, with the field
/// they are stored in.
fn cam_set_lens(m: &mut Mem, cam: u32, off: u32, v: f32) {
    // fld, fst to the field, compare |v| on the register, replaced by ±0.0001
    m.set_f32(cam + off, fq(v));
    let r = F80::from_f32(v);
    if r.abs().compare(F80::from_f64(STATIC_EPS)) == Some(core::cmp::Ordering::Less) {
        m.set_f32(cam + off, clamp_small(r));
    }
}

/// One parameter of a camera from a value record [soft 0x080f4c90 SI_camSetParam]: the float fields of [`CAM_FLOATS`]; 0xb and 0xc
/// in degrees times 0.0174532925 (0xb into +0x30; 0xc into +0x40, the product stored to a float first, a magnitude below 0.0001
/// replaced by ±0.0001, then the focal length +0x10c made from it as half the aspect +0x114 over the tangent of half the angle
/// (`tan`, the C library's)); 0x175 (focal, +0x10c) and 0x177 (aspect, +0x114) are clamped the same way and then the angle
/// +0x40 is made again as twice the arc tangent of half the aspect over the focal length; 0x176 (+0x110) is clamped and nothing
/// else; 0x172 (the pixel ratio) sets the format at +0x54 to 0xd, +0x58 and the global 0x08679574 to the value and calls
/// `SI_changeCameraAspect(cam, 0xd)`; 0x233 and 0x17a are 16-bit words; 0x234 stores the second word at +0x6c; 0xb1 sets the
/// camera's graf node's display type (+0xc) to 2 when the value is 1, else 0 (nothing without a node); 9 and 10 store the value as
/// the curve's static value and place the camera on its path (`SI_setPathTranslation` with the relation of the curve's type).
/// Any other type answers 0.
pub fn cam_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, cam: u32, fcv: u32, value: u32) -> u32 {
    let time = current_time(m).to_f32();
    let v = m.f32(value + 4);
    match fcv_type {
        9 | 0xa => {
            if fcv == 0 {
                return 0;
            }
            curve_static_set(m, fcv, value, time);
            let (rel, g, r) = (m.local(4), m.local(4), m.local(4));
            si_dict::attdict::get_info_from_fcurve(m, si_dict::globals::SI_ATD, fcv_type, Ptr32::new(rel), Ptr32::new(g), Ptr32::new(r), Ptr32::new(r));
            let rel_id = m.u32(rel);
            let chap = rec::chap(m, cid::CAM) as u32;
            o.set_path_translation(m, rel_id, chap, cam, fcv);
            m.release(12);
        }
        0xb => fscaled(m, cam + 0x30, value + 4, DEG_TO_RAD),
        0xc => {
            let a = (F80::from_f32(v) * F80::from_f64(DEG_TO_RAD)).to_f32();
            cam_set_lens(m, cam, 0x40, a);
            let half = F80::from_f64(0.5);
            let arg = (half * F80::from_f32(m.f32(cam + 0x40))).to_f64();
            let t = si_x87::trig::tan(arg);
            let f = ((half * F80::from_f32(m.f32(cam + 0x114))) / t).to_f32();
            m.set_f32(cam + 0x10c, f);
        }
        0x175 | 0x177 => {
            cam_set_lens(m, cam, if fcv_type == 0x175 { 0x10c } else { 0x114 }, v);
            cam_recompute_angle(m, cam);
        }
        0x176 => cam_set_lens(m, cam, 0x110, v),
        0x233 => {
            let w = m.u16(value + 4);
            m.set_u16(cam + 0x3c, w);
        }
        0x17a => {
            let w = m.u16(value + 4);
            m.set_u16(cam + 0x108, w);
        }
        0x234 => {
            let w = m.u32(value + 8);
            m.set_u32(cam + 0x6c, w);
        }
        0x172 => {
            m.set_u32(cam + 0x54, 0xd);
            let q = fq(v);
            m.set_f32(cam + 0x58, q);
            m.set_f32(0x0867_9574, q);
            o.change_camera_aspect(m, cam, 0xd);
        }
        0xb1 => {
            // SI_edSumatraTo3x is 0 and the node is the camera's in the current list
            let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
            let gn = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(cam), 4).addr();
            if gn != 0 {
                let x = super::elem::attr_float(m, Ptr32::new(value));
                let one = x.compare(F80::from_i32(1)) == Some(core::cmp::Ordering::Equal);
                m.set_i32(gn + 0xc, if one { 2 } else { 0 });
            }
        }
        t => match CAM_FLOATS.iter().find(|(k, _)| *k == t).map(|(_, o)| *o) {
            Some(off) => fcopy(m, cam + off, value + 4),
            None => return 0,
        },
    }
    1
}

/// The camera's angle from its aspect and focal length: twice the arc tangent of half the aspect (+0x114) over the focal length
/// (+0x10c), as `fpatan(aspect × 0.5 / focal, 1)` doubled on the register, stored to +0x40.
fn cam_recompute_angle(m: &mut Mem, cam: u32) {
    let r = F80::from_f32(m.f32(cam + 0x114)) * F80::from_f64(0.5) / F80::from_f32(m.f32(cam + 0x10c));
    let a = si_x87::trig::fpatan(r, F80::from_i32(1));
    m.set_f32(cam + 0x40, (a + a).to_f32());
}

/// One parameter of a material into a value record [soft 0x080f2b7c SI_materialGetParam]. The record starts as the float 0.
/// The colour and shading floats (types 0x2d to 0x37 and 0x39: +0x34 +0x38 +0x3c +0x44 +0x48 +0x4c +0x54 +0x58 +0x5c +0x64 +0x6a8
/// +0x6b8) are copied through the register; then, on the full product's path: 0x38, 0x3a and 0x3b are the floats at +0x6ac,
/// +0x6bc, +0x6dc; 0x101 and 0x102 (blur width +0x75c, decay +0x760) answer 0 when the 16-bit flag at +0x758 is zero; 0x186 is the
/// float at +0x24; 0x187 is the 16-bit flag +0x758 (type 2); 0x188, 0x2a5 and 0x2a6 fill the second word with the signed 16-bit
/// words at +0x1e, +0xdc8, +0xdca (the first stays 1). Any other type answers 0.
pub fn material_get_param(m: &mut Mem, fcv_type: i32, mat: u32, out: u32) -> u32 {
    start_value(m, out);
    let off = match fcv_type {
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
        // SI_edSumatraTo3x is 0 from here on
        0x38 => 0x6ac,
        0x3a => 0x6bc,
        0x3b => 0x6dc,
        0x186 => 0x24,
        0x101 | 0x102 => {
            if m.i16(mat + 0x758) == 0 {
                return 0;
            }
            if fcv_type == 0x101 { 0x75c } else { 0x760 }
        }
        0x187 => {
            m.set_u32(out, 2);
            let v = m.u16(mat + 0x758);
            m.set_u16(out + 4, v);
            return 1;
        }
        0x188 | 0x2a5 | 0x2a6 => {
            let w = m.i16(mat + match fcv_type { 0x188 => 0x1e, 0x2a5 => 0xdc8, _ => 0xdca }) as i32;
            m.set_i32(out + 8, w);
            return 1;
        }
        _ => return 0,
    };
    fcopy(m, out + 4, mat + off);
    1
}

/// A 3-D texture's float fields, (type, offset), copied through the register by the reader and the writer alike (0x49 to 0x4b
/// are validated on writing).
const T3D_FLOATS: [(i32, u32); 38] = [
    (0x44, 0x18),
    (0x45, 0x10),
    (0x46, 0x20),
    (0x49, 0x80),
    (0x4a, 0x84),
    (0x4b, 0x88),
    (0x4c, 0x30),
    (0x4d, 0x34),
    (0x4e, 0x38),
    (0x4f, 0x40),
    (0x50, 0x44),
    (0x51, 0x48),
    (0x52, 0x50),
    (0x53, 0x54),
    (0x54, 0x58),
    (0x55, 0x60),
    (0x56, 0x64),
    (0x57, 0x68),
    (0x58, 0x70),
    (0x59, 0x74),
    (0x5a, 0x78),
    (0x5b, 0x90),
    (0x5c, 0x94),
    (0x5d, 0x98),
    (0x61, 0xa8),
    (0x62, 0xac),
    (0x63, 0xb0),
    (0x64, 0x108),
    (0x65, 0x10c),
    (0x66, 0x110),
    (0x67, 0x114),
    (0x68, 0x118),
    (0x9b, 0x28),
    (0x9c, 0x3c),
    (0x9d, 0x4c),
    (0x9e, 0x5c),
    (0x9f, 0x6c),
    (0xa0, 0x7c),
];

/// One parameter of a 3-D texture into a value record [soft 0x080f37c4 SI_text3dGetParam]. The record starts as the float 0.
/// The floats of [`T3D_FLOATS`] are copied; 0x5e to 0x60 (+0x9c +0xa0 +0xa4) are times 57.29577951308232; 0x195 is the integer at
/// +0x1c (type 4, the word moved as it is). On the full product's path also: 0x48 (+0x14, times 57.29577951308232), 0x6a (+0x120),
/// 0x196 (second word: the signed 16-bit word at +0x24) and 0x27b (second word: the word at +0xc). Any other type answers 0.
pub fn text3d_get_param(m: &mut Mem, fcv_type: i32, t: u32, out: u32) -> u32 {
    start_value(m, out);
    if let Some((_, off)) = T3D_FLOATS.iter().find(|(k, _)| *k == fcv_type) {
        fcopy(m, out + 4, t + off);
        return 1;
    }
    match fcv_type {
        0x5e..=0x60 => fscaled(m, out + 4, t + 0x9c + 4 * (fcv_type - 0x5e) as u32, RAD_TO_DEG),
        0x195 => {
            m.set_u32(out, 4);
            let w = m.u32(t + 0x1c);
            m.set_u32(out + 4, w);
        }
        // SI_edSumatraTo3x is 0 from here on
        0x48 => fscaled(m, out + 4, t + 0x14, RAD_TO_DEG),
        0x6a => fcopy(m, out + 4, t + 0x120),
        0x196 => {
            let w = m.i16(t + 0x24) as i32;
            m.set_i32(out + 8, w);
        }
        0x27b => {
            let w = m.u32(t + 0xc);
            m.set_u32(out + 8, w);
        }
        _ => return 0,
    }
    1
}

/// One parameter of a 3-D texture from a value record [soft 0x080f6124 SI_text3dSetParam]: the fields of [`text3d_get_param`]
/// (0x48, 0x5e to 0x60 in degrees times 0.0174532925 in the register; 0x6a +0x120; 0x196 the 16-bit word at +0x24 from the second
/// word), except that 0x49 to 0x4b go through `SI_validTxt3dFct1`, `…Fct2`, `…Fct3` (the object and the float) and 0x195 through
/// `SI_validTxt3dNbIter` (the object and the integer), whose answer, non-zero for a rejected value, is the result; an unknown type
/// is a rejection too. When the value was taken the texture is marked changed (`SI_texture3DSetDirty`) and its turbulence updated
/// (`SI_updTxt3dTurb`). The answer is whether the value was taken.
pub fn text3d_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, t: u32, value: u32) -> bool {
    let mut bad = 0;
    if let Some((_, off)) = T3D_FLOATS.iter().find(|(k, _)| *k == fcv_type && !(0x49..=0x4b).contains(k)) {
        fcopy(m, t + off, value + 4);
    } else {
        match fcv_type {
            0x48 => fscaled(m, t + 0x14, value + 4, DEG_TO_RAD),
            0x5e..=0x60 => fscaled(m, t + 0x9c + 4 * (fcv_type - 0x5e) as u32, value + 4, DEG_TO_RAD),
            0x6a => fcopy(m, t + 0x120, value + 4),
            0x49 => bad = o.valid_txt3d_fct1(m, t, fq(m.f32(value + 4))),
            0x4a => bad = o.valid_txt3d_fct2(m, t, fq(m.f32(value + 4))),
            0x4b => bad = o.valid_txt3d_fct3(m, t, fq(m.f32(value + 4))),
            0x195 => bad = o.valid_txt3d_nb_iter(m, t, m.i32(value + 4)),
            0x196 => {
                let w = m.u16(value + 8);
                m.set_u16(t + 0x24, w);
            }
            _ => bad = 4,
        }
    }
    if bad == 0 {
        o.texture3d_set_dirty(m, t);
        o.upd_txt3d_turb(m, t);
    }
    bad == 0
}

/// The 2-D texture's floats copied as they are by `SI_text2dGetParam`: by parameter type, the field's offset.
const T2D_FLOATS: [(i32, u32); 18] = [
    (0x21, 0x58),
    (0x22, 0x5c),
    (0x25, 0x128),
    (0x26, 0x12c),
    (0x27, 0x130),
    (0x29, 0x138),
    (0xa2, 0x134),
    (0xd1, 0xac),
    (0xd2, 0xb0),
    (0xd3, 0xb4),
    (0xd4, 0xa0),
    (0xd5, 0xa4),
    (0xd6, 0xa8),
    // the tail, reached with SI_edSumatraTo3x = 0
    (0x23, 0x68),
    (0x24, 0x6c),
    (0x28, 0x144),
    (0x2b, 0x140),
    (0xa1, 0x10c),
];

/// The picture-size parameters of a 2-D texture, 0x18c to 0x18f (left, right, top, bottom of the crop): with the crop on (the
/// word at +0x2e is 1) the stored border kept inside the picture, else the whole picture (left and top 0, right and bottom the
/// size less one). The picture's size comes from `SI_pictureGetDimensions` on the file name at +0x20; when that fails the answer
/// is 0 (the record's type word may already be 4). 0x18e and 0x18f first make the file name for the current frame
/// (`SI_setFileName`; 0x18e only when the crop is on).
fn t2d_crop(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, t: u32, out: u32) -> bool {
    let crop = m.i16(t + 0x2e) == 1;
    // (the border field, its meaning): 0x18c and 0x18d keep the field, 0x18e and 0x18f count it from the far end
    let (field, from_far) = match fcv_type {
        0x18c => (0x30, false),
        0x18d => (0x32, false),
        0x18e => (0x36, true),
        _ => (0x34, true),
    };
    let no_crop_zero = matches!(fcv_type, 0x18c | 0x18e);
    let type_first = fcv_type != 0x18d && fcv_type != 0x18f;
    if type_first {
        m.set_u32(out, 4);
    }
    if no_crop_zero && !crop {
        m.set_u32(out + 4, 0);
        return true;
    }
    if fcv_type == 0x18e || fcv_type == 0x18f {
        let frame = m[rec::SI_FRAME_CURRENT] as i32;
        o.set_file_name(m, t, frame);
    }
    let local = m.local(4);
    let name = m.u32(t + 0x20);
    // the width comes through the second argument, the height (0x18e, 0x18f) through the third
    let bad = if from_far { o.picture_get_dimensions(m, name, 0, local) } else { o.picture_get_dimensions(m, name, local, 0) } != 0;
    let w = m.i16(local) as i32;
    m.release(4);
    if bad {
        return false;
    }
    m.set_u32(out, 4);
    let edge = m.i16(t + field) as i32;
    let v = if from_far { w - (edge + 1) } else { edge };
    let r = if crop && v < w { v } else { w - 1 };
    m.set_i32(out + 4, r);
    true
}

/// One parameter of a 2-D texture into a value record [soft 0x080f2e30 SI_text2dGetParam]. The record starts as the float 0.
/// The floats of [`T2D_FLOATS`] are copied; 0xce to 0xd0 (+0x94 +0x98 +0x9c) are times 57.29577951308232, and so is 0x2c
/// (+0x168); 0x190 and 0x191 are the signed 16-bit words at +0xf8 and +0xfa (type 4); 0x192 to 0x194 the words at +0xfc +0xfe +0x100,
/// 0x18a (+0x2c), 0x216 (+0x1a2), 0x217 (+0x1a0) and 0x220 (+0x1c8) words of type 2; 0x218 is the word at +0x14 (type 5); 0x189 and
/// 0x197 only give the second word (the signed 16-bit words at +0x2a and +0x28); 0x18c to 0x18f are the crop ([`t2d_crop`]). Any
/// other type answers 0.
///
/// `SI_edSumatraTo3x` = 0 on the full product's path: the branches for it being non-zero (which answer from the globals at
/// 0x08784e60 to 0x08784ea8) are left out.
pub fn text2d_get_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, t: u32, out: u32) -> u32 {
    start_value(m, out);
    if let Some((_, off)) = T2D_FLOATS.iter().find(|(k, _)| *k == fcv_type) {
        fcopy(m, out + 4, t + off);
        return 1;
    }
    let word = |m: &mut Mem, off: u32| {
        m.set_u32(out, 2);
        let v = m.u16(t + off);
        m.set_u16(out + 4, v);
    };
    match fcv_type {
        0xce..=0xd0 => fscaled(m, out + 4, t + 0x94 + 4 * (fcv_type - 0xce) as u32, RAD_TO_DEG),
        0x2c => fscaled(m, out + 4, t + 0x168, RAD_TO_DEG),
        0x190 | 0x191 => {
            m.set_u32(out, 4);
            let v = m.i16(t + 0xf8 + 2 * (fcv_type - 0x190) as u32) as i32;
            m.set_i32(out + 4, v);
        }
        0x192..=0x194 => word(m, 0xfc + 2 * (fcv_type - 0x192) as u32),
        0x18a => word(m, 0x2c),
        0x216 => word(m, 0x1a2),
        0x217 => word(m, 0x1a0),
        0x220 => word(m, 0x1c8),
        0x218 => {
            m.set_u32(out, 5);
            let w = m.u32(t + 0x14);
            m.set_u32(out + 4, w);
        }
        0x189 => {
            let v = m.i16(t + 0x2a) as i32;
            m.set_i32(out + 8, v);
        }
        0x197 => {
            let v = m.i16(t + 0x28) as i32;
            m.set_i32(out + 8, v);
        }
        0x18c..=0x18f => return t2d_crop(m, o, fcv_type, t, out) as u32,
        _ => return 0,
    }
    1
}

/// One parameter of a 2-D texture from a value record [soft 0x080f5be8 SI_text2dSetParam]. The floats of [`T2D_FLOATS`] (the first
/// thirteen and 0x23, 0x24, 0x28, 0xa1) are stored; 0x2c and 0xce to 0xd0 in degrees times 0.0174532925; 0xd1 to 0xd3 and 0xd4 to
/// 0xd6 go through `SI_validTxt2dWindowScal` and `…WindowTrans` (the texture, the field's address, the float); 0x189 through
/// `SI_validTxt2dMethod`, 0x190 and 0x191 `…URepeat` and `…VRepeat`, 0x18c and 0x18d store the word, make the file name for the
/// current frame (`SI_setFileName`) and call `…CropU`, 0x18e and 0x18f call `…CropV` (the value, 1 or 0); 0x216, 0x217 and 0x220
/// store `SI_getAttrValInBoolean` of the record and set the word at +0x194 to 7. Whatever is taken marks the texture changed
/// (`SI_texture2DSetDirty`) except the validators' (they do it themselves); 0x21 and 0x22 also `SI_txt2dSetScale`, and 0x25 to
/// 0x27, 0x29, 0x2b and 0xa2 `SI_updTxt2dEffects`. The answer is whether the value was taken; an unknown type is not.
pub fn text2d_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, t: u32, value: u32) -> bool {
    let mut bad = 0;
    let mut dirty = true;
    let v = value + 4;
    match fcv_type {
        0x21 | 0x22 => {
            fcopy(m, t + if fcv_type == 0x21 { 0x58 } else { 0x5c }, v);
            o.txt2d_set_scale(m, t);
        }
        0x23 => fcopy(m, t + 0x68, v),
        0x24 => fcopy(m, t + 0x6c, v),
        0x25 => {
            fcopy(m, t + 0x128, v);
            o.texture2d_set_dirty(m, t);
            o.upd_txt2d_effects(m, t);
            dirty = false;
        }
        0x26 | 0x27 | 0x29 | 0x2b | 0xa2 => {
            let off = match fcv_type {
                0x26 => 0x12c,
                0x27 => 0x130,
                0x29 => 0x138,
                0x2b => 0x140,
                _ => 0x134,
            };
            fcopy(m, t + off, v);
            o.upd_txt2d_effects(m, t);
        }
        0xa1 => fcopy(m, t + 0x10c, v),
        0x28 => fcopy(m, t + 0x144, v),
        0x2c => fscaled(m, t + 0x168, v, DEG_TO_RAD),
        0xce..=0xd0 => fscaled(m, t + 0x94 + 4 * (fcv_type - 0xce) as u32, v, DEG_TO_RAD),
        0xd1..=0xd3 => {
            dirty = false;
            let f = fq(m.f32(v));
            bad = o.valid_txt2d_window_scal(m, t, t + 0xac + 4 * (fcv_type - 0xd1) as u32, f);
        }
        0xd4..=0xd6 => {
            dirty = false;
            let f = fq(m.f32(v));
            bad = o.valid_txt2d_window_trans(m, t, t + 0xa0 + 4 * (fcv_type - 0xd4) as u32, f);
        }
        0x189 => {
            dirty = false;
            let n = m.i16(value + 8) as i32;
            bad = o.valid_txt2d_method(m, t, n);
        }
        0x18a => {
            let w = m.u16(v);
            m.set_u16(t + 0x2c, w);
        }
        0x18c | 0x18d => {
            dirty = false;
            let frame = m[rec::SI_FRAME_CURRENT] as i32;
            o.set_file_name(m, t, frame);
            let w = m.u16(v);
            m.set_u16(t + if fcv_type == 0x18c { 0x30 } else { 0x32 }, w);
            bad = o.valid_txt2d_crop_u(m, t);
        }
        0x18e | 0x18f => {
            dirty = false;
            let frame = m[rec::SI_FRAME_CURRENT] as i32;
            o.set_file_name(m, t, frame);
            let n = m.i16(v) as i32;
            bad = o.valid_txt2d_crop_v(m, t, n, (fcv_type == 0x18e) as i32);
        }
        0x190 => {
            dirty = false;
            let n = m.i16(v) as i32;
            bad = o.valid_txt2d_u_repeat(m, t, n);
        }
        0x191 => {
            dirty = false;
            let n = m.i16(v) as i32;
            bad = o.valid_txt2d_v_repeat(m, t, n);
        }
        0x192..=0x194 => {
            let w = m.u16(v);
            m.set_u16(t + 0xfc + 2 * (fcv_type - 0x192) as u32, w);
        }
        0x197 => {
            let w = m.u16(value + 8);
            m.set_u16(t + 0x28, w);
        }
        0x216 | 0x217 | 0x220 => {
            let b = get_attr_val_in_boolean(m, value) as u16;
            let off = match fcv_type {
                0x216 => 0x1a2,
                0x217 => 0x1a0,
                _ => 0x1c8,
            };
            m.set_u16(t + off, b);
            m.set_u16(t + 0x194, 7);
        }
        _ => {
            dirty = false;
            bad = 4;
        }
    }
    if bad == 0 && dirty {
        o.texture2d_set_dirty(m, t);
    }
    bad == 0
}

/// One parameter of a light into a value record [soft 0x080f20a8 SI_lightGetParam]. The record starts as the float 0; the
/// curve's static value is dropped when stale ([`drop_stale_static`]). `light` is the light's record (its kind: the word at
/// +0x1e, 1 and 2 the point and spot kinds, 4 the sun's: the answers of the kind-specific parameters are 0 for another kind),
/// `fcv` its parameter curve (or 0). Answers 1 when the parameter is known and 0 otherwise; a few answer 1 without writing
/// (0x10b, 0x237, 0x185, and 0x223 to 0x226 on a light that is not of kind 2).
///
/// `SI_edSumatraTo3x` = 0 on the full product's path: the branches for it being non-zero (the scene's current element at
/// 0x08680e60 and the globals `SI_gSoftLight` and the constants at 0x08784f20 to 0x08784f4c) are left out.
pub fn light_get_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, light: u32, fcv: u32, out: u32) -> u32 {
    start_value(m, out);
    drop_stale_static(m, fcv);
    // (the original reads the light's kind only in the branches that need it: a null light with another type is fine)
    let kind_of = |m: &Mem| m.u16(light + 0x1e);
    let small_of = |m: &Mem| kind_of(m).wrapping_sub(1) <= 1;
    let word = |m: &mut Mem, off: u32| {
        m.set_u32(out, 2);
        let v = m.u16(light + off);
        m.set_u16(out + 4, v);
    };
    let int = |m: &mut Mem, off: u32| {
        m.set_u32(out, 4);
        let v = m.u32(light + off);
        m.set_u32(out + 4, v);
    };
    match fcv_type {
        0x10b | 0x237 => return 1,
        0xb0 => {
            m.set_u32(out, 2);
            let gt = o.light_get_graf_type(m, light);
            let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
            let gn = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(light), gt).addr();
            if gn != 0 {
                let v = (m.i32(gn + 0xc) == 2) as u16;
                m.set_u16(out + 4, v);
            }
        }
        0x94 | 0x95 | 0x96 => fcopy(m, out + 4, light + 0x34 + 4 * (fcv_type - 0x94) as u32),
        0x16 | 0x17 | 0x18 => fcopy(m, out + 4, light + 0x24 + 4 * (fcv_type - 0x16) as u32),
        0x1c => fcopy(m, out + 4, light + 0x84),
        0x1d => fcopy(m, out + 4, light + 0x88),
        0x1e => fcopy(m, out + 4, light + 0x90),
        0x17d => {
            if !small_of(m) {
                return 0;
            }
            word(m, 0x182);
        }
        0xd7..=0xdc | 0x17f..=0x181 => {
            if !small_of(m) || m.i16(light + 0x182) != 1 {
                return 0;
            }
            match fcv_type {
                0xd7 => fscaled(m, out + 4, light + 0x1d0, RAD_TO_DEG),
                0xd8 => fscaled(m, out + 4, light + 0x1d4, RAD_TO_DEG),
                0xd9 => fscaled(m, out + 4, light + 0x1d8, RAD_TO_DEG),
                0xda => fcopy(m, out + 4, light + 0x1c4),
                0xdb => fcopy(m, out + 4, light + 0x1c8),
                0xdc => fcopy(m, out + 4, light + 0x1cc),
                0x17f => int(m, 0x188),
                0x180 => int(m, 0x18c),
                _ => {
                    if kind_of(m) != 2 {
                        return 0;
                    }
                    word(m, 0x1c0);
                }
            }
        }
        0x182 => word(m, 0x15c),
        0x183 => word(m, 0x160),
        0x1a | 0x97 | 0x98 | 0x99 => {
            if kind_of(m) != 2 {
                return 0;
            }
            match fcv_type {
                0x97 => fcopy(m, out + 4, light + 0x58),
                0x98 => fcopy(m, out + 4, light + 0x5c),
                0x99 => fcopy(m, out + 4, light + 0x60),
                _ => fscaled(m, out + 4, light + 0x74, RAD_TO_DEG),
            }
        }
        0xbd | 0x17b | 0x17c | 0x184 | 0x185 => {
            if fcv_type < 0x184 {
                // the sun's date record: a stored integer as a float, then on to the block of 0x184
                if kind_of(m) != 4 {
                    return 0;
                }
                let rec = m.u32(light + 0x16c);
                if rec == 0 {
                    return 0;
                }
                let n = match fcv_type {
                    0x17b => m.i32(rec + 8),
                    0xbd => m.i32(rec + 0x10),
                    _ => m.i32(rec + 4),
                };
                m.set_f32(out + 4, F80::from_i32(n).to_f32());
            }
            let rec = m.u32(light + 0x178);
            if rec == 0 {
                return 1;
            }
            if fcv_type == 0x184 {
                m.set_u32(out, 5);
                let w = m.u32(rec);
                m.set_u32(out + 4, w);
            } else if fcv_type != 0x185 {
                return 0;
            }
        }
        0x1f | 0x20 => {
            if !curve_value_out(m, fcv, out) {
                return 0;
            }
        }
        0x238 => word(m, 0x180),
        0x223..=0x226 => {
            if kind_of(m) == 2 {
                match fcv_type {
                    0x223 => fcopy(m, out + 4, light + 0x128),
                    0x224 => int(m, 0x118),
                    0x225 => int(m, 0x11c),
                    _ => int(m, 0x124),
                }
            }
        }
        0x270 => fcopy(m, out + 4, light + 0x230),
        0x271 => int(m, 0x234),
        0x272 => int(m, 0x238),
        0x273 => word(m, 0x23c),
        0x274 => word(m, 0x23e),
        // the types that are not in the first switch: the tail for SI_edSumatraTo3x = 0
        0x222 => {
            let v = m.i16(light + 0x8c) as i32;
            m.set_i32(out + 8, v);
        }
        0x19 => {
            if kind_of(m) != 2 {
                return 0;
            }
            fscaled(m, out + 4, light + 0x70, RAD_TO_DEG);
        }
        0x17e => {
            if !small_of(m) || m.i16(light + 0x182) != 1 {
                return 0;
            }
            let v = m.u32(light + 0x184);
            m.set_u32(out + 8, v);
        }
        0x26d => fcopy(m, out + 4, light + 0x224),
        0x26e => fcopy(m, out + 4, light + 0x228),
        0x26f => fcopy(m, out + 4, light + 0x22c),
        _ => return 0,
    }
    1
}

/// A light's value is checked by `check`, a validator that answers 0 for a valid one.
fn light_ok(bad: i32) -> u32 {
    (bad == 0) as u32
}

/// One parameter of a light from a value record [soft 0x080f51b8 SI_lightSetParam]. The floats are stored as the registers hold
/// them (0x16 to 0x18, 0x1c, 0x1d, 0x1e, 0x94 to 0x96, 0x97 to 0x99 and 0x223 as they are; 0x19, 0x1a, 0xd7 to 0xd9 times
/// 0.0174532925; 0xda to 0xdc as they are); 0x26d to 0x270 only when not below 0 (a NaN is refused too); the integers 0x225, 0x271
/// (not below 0), 0x272 (the same), the 16-bit words 0x17d, 0x181, 0x238 and the flag words 0x273 and 0x274 (the value, taken as a
/// float, equals 1); the area light's and the sun's parameters are checked after they were stored by `SI_lightValidate…`; 0x222,
/// 0x224 and 0x226 are checked by `SI_validLight…` first. 0x1f and 0x20 store the value as the curve's static value at the current
/// time and place the light on its path (`SI_setPathTranslation`, a curve of the light chapter). 0xb0 sets the light's graf node's
/// display type (+0xc) to 2 when the value is 1, else 0 (and, for a node not of kind 7, the word at +0x176). 0xbd stores the value
/// cut toward zero in the sun's date record (+0x10) and checks it. The answer is 1 when the value was taken, else 0.
pub fn light_set_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, light: u32, fcv: u32, value: u32) -> u32 {
    let time = current_time(m).to_f32();
    let v = value + 4;
    // (read only where a branch needs it, as the original does)
    let kind_of = |m: &Mem| m.u16(light + 0x1e);
    let small_of = |m: &Mem| kind_of(m).wrapping_sub(1) <= 1;
    let direct = |m: &mut Mem, off: u32| fcopy(m, light + off, v);
    let wide = |m: &mut Mem, off: u32| {
        let w = m.u32(v);
        m.set_u32(light + off, w);
    };
    match fcv_type {
        0x16 | 0x17 | 0x18 => direct(m, 0x24 + 4 * (fcv_type - 0x16) as u32),
        0x1c => direct(m, 0x84),
        0x1d => direct(m, 0x88),
        0x1e => direct(m, 0x90),
        0x94 | 0x95 | 0x96 => direct(m, 0x34 + 4 * (fcv_type - 0x94) as u32),
        0x19 | 0x1a | 0x97 | 0x98 | 0x99 => {
            if kind_of(m) != 2 {
                return 0;
            }
            match fcv_type {
                0x97 => direct(m, 0x58),
                0x98 => direct(m, 0x5c),
                0x99 => direct(m, 0x60),
                0x19 => fscaled(m, light + 0x70, v, DEG_TO_RAD),
                _ => fscaled(m, light + 0x74, v, DEG_TO_RAD),
            }
        }
        0x1f | 0x20 => {
            if fcv == 0 {
                return 0;
            }
            curve_static_set(m, fcv, value, time);
            let (rel, g, r) = (m.local(4), m.local(4), m.local(4));
            si_dict::attdict::get_info_from_fcurve(m, si_dict::globals::SI_ATD, fcv_type, Ptr32::new(rel), Ptr32::new(g), Ptr32::new(r), Ptr32::new(r));
            let rel_id = m.u32(rel);
            let chap = rec::chap(m, cid::LIG) as u32;
            o.set_path_translation(m, rel_id, chap, light, fcv);
            m.release(12);
        }
        0xb0 => {
            let gt = o.light_get_graf_type(m, light);
            let list = Ptr32::new(si_motion::glob::cur_graf_list(m));
            let gn = si_view::graflist::list::find_gl_data(m, list, Ptr32::new(light), gt).addr();
            if gn != 0 {
                let on = super::eval::get_attr_val_in_float(m, Ptr32::new(value)).eq(F80::from_i32(1));
                m.set_u32(gn + 0xc, if on { 2 } else { 0 });
                if m.u32(gn) != 7 {
                    m.set_u16(light + 0x176, on as u16);
                }
            }
        }
        0xbd => {
            if kind_of(m) != 4 {
                return 0;
            }
            let p = m.u32(light + 0x16c);
            if p == 0 {
                return 0;
            }
            let n = F80::from_f32(m.f32(v)).to_i32(Round::Zero);
            m.set_i32(p + 0x10, n);
            return light_ok(o.light_sun_validate_date_time(m, light));
        }
        0x17d => {
            if !small_of(m) {
                return 0;
            }
            let w = m.u16(v);
            m.set_u16(light + 0x182, w);
            return light_ok(o.light_validate_area_light(m, light));
        }
        0x17e..=0x181 => {
            if !small_of(m) || m.i16(light + 0x182) != 1 {
                return 0;
            }
            match fcv_type {
                0x17e => {
                    let w = m.u32(value + 8);
                    m.set_u32(light + 0x184, w);
                    return light_ok(o.light_validate_area_type(m, light));
                }
                0x17f => wide(m, 0x188),
                0x180 => wide(m, 0x18c),
                _ => {
                    let w = m.u16(v);
                    m.set_u16(light + 0x1c0, w);
                }
            }
        }
        0xd7..=0xd9 => {
            if !small_of(m) || m.i16(light + 0x182) != 1 {
                return 0;
            }
            fscaled(m, light + 0x1d0 + 4 * (fcv_type - 0xd7) as u32, v, DEG_TO_RAD);
            return light_ok(o.light_validate_area_rotation(m, light));
        }
        0xda..=0xdc => {
            if !small_of(m) || m.i16(light + 0x182) != 1 {
                return 0;
            }
            direct(m, 0x1c4 + 4 * (fcv_type - 0xda) as u32);
            return light_ok(o.light_validate_area_scaling(m, light));
        }
        0x182 => {
            let w = m.i16(v) as i32;
            m.set_i32(light + 0x15c, w);
            return light_ok(o.light_validate_selective(m, light));
        }
        0x183 => {
            let w = m.i16(v) as i32;
            m.set_i32(light + 0x160, w);
        }
        0x238 => {
            let w = m.u16(v);
            m.set_u16(light + 0x180, w);
        }
        0x222 => {
            let n = m.i16(value + 8) as i32;
            return light_ok(o.valid_light_shadow_type(m, light, n));
        }
        0x223 => direct(m, 0x128),
        0x224 => {
            let n = m.i32(v);
            return light_ok(o.valid_light_map_res(m, light, n));
        }
        0x225 => wide(m, 0x11c),
        0x226 => {
            let n = m.i32(v);
            return light_ok(o.valid_light_filter_step(m, light, n));
        }
        0x26d..=0x270 => {
            let f = F80::from_f32(m.f32(v));
            // fcom against 0: C0 (below) or C2 (unordered) refuses
            if matches!(f.compare(F80::from_i32(0)), None | Some(core::cmp::Ordering::Less)) {
                return 0;
            }
            let off = match fcv_type {
                0x26d => 0x224,
                0x26e => 0x228,
                0x26f => 0x22c,
                _ => 0x230,
            };
            m.set_f32(light + off, f.to_f32());
        }
        0x271 | 0x272 => {
            let n = m.i32(v);
            if n < 0 {
                return 0;
            }
            m.set_i32(light + if fcv_type == 0x271 { 0x234 } else { 0x238 }, n);
        }
        0x273 | 0x274 => {
            let on = super::eval::get_attr_val_in_float(m, Ptr32::new(value)).eq(F80::from_i32(1)) as u16;
            m.set_u16(light + if fcv_type == 0x273 { 0x23c } else { 0x23e }, on);
        }
        _ => return 0,
    }
    1
}

/// How [`scene_get_param`] reads a field of the render control record.
#[derive(Clone, Copy)]
enum Sk {
    /// A float copied through the register; the record keeps its type (float).
    F,
    /// The same, and the type is set to float after it.
    FT,
    /// A 16-bit word, type 2.
    W,
    /// A signed 16-bit word, type 4.
    I16,
    /// A 32-bit word, type 4.
    I32,
    /// A 32-bit word, type 5.
    T5,
    /// A 32-bit word moved into the second slot (+8), the type kept.
    M8,
    /// A signed 16-bit word into the second slot, the type kept.
    S8,
}

const SCENE_PARAMS: [(u16, Sk, u16); 124] = [
    (0x3c, Sk::F, 0x104), (0x3d, Sk::F, 0x10c), (0x3e, Sk::F, 0x118), (0x3f, Sk::F, 0x11c),
    (0x40, Sk::F, 0x120), (0x41, Sk::F, 0x130), (0xc6, Sk::F, 0x26c), (0xc7, Sk::F, 0x270),
    (0xc8, Sk::F, 0x284), (0xc9, Sk::F, 0x288), (0xca, Sk::F, 0x28c), (0xcb, Sk::F, 0x260),
    (0xcc, Sk::F, 0x264), (0xcd, Sk::F, 0x268), (0x1a3, Sk::W, 0x254), (0x1a7, Sk::I16, 0x1fa),
    (0x1a9, Sk::W, 0x586), (0x1aa, Sk::W, 0x588), (0x1ab, Sk::W, 0x20c), (0x1ac, Sk::W, 0x250),
    (0x1ae, Sk::W, 0x58a), (0x1af, Sk::W, 0x58c), (0x1b0, Sk::W, 0x240), (0x1b1, Sk::I16, 0x1f6),
    (0x1b2, Sk::W, 0x244), (0x1b4, Sk::I16, 0x1f8), (0x1b6, Sk::W, 0x20e), (0x1b7, Sk::I16, 0x15c),
    (0x1b8, Sk::I16, 0x1dc), (0x1ba, Sk::W, 0x210), (0x1bb, Sk::W, 0x42e), (0x1bd, Sk::W, 0x498),
    (0x1c3, Sk::W, 0x3dc), (0x1c4, Sk::F, 0x4a8), (0x1c5, Sk::W, 0x42c), (0x1ca, Sk::F, 0x444),
    (0x1cc, Sk::F, 0x440), (0x1cf, Sk::F, 0x424), (0x1d0, Sk::F, 0x4a4), (0x1d1, Sk::I32, 0x2b8),
    (0x1d2, Sk::W, 0x49a), (0x1d4, Sk::I32, 0x2bc), (0x1d5, Sk::I32, 0x3e8), (0x1d6, Sk::I32, 0x3e4),
    (0x1d9, Sk::W, 0x3e0), (0x1da, Sk::W, 0x3d8), (0x1db, Sk::W, 0x3da), (0x1dc, Sk::I32, 0x410),
    (0x1dd, Sk::I32, 0x408), (0x1de, Sk::I32, 0x40c), (0x1e7, Sk::I32, 0x3fc), (0x1e8, Sk::W, 0x2ca),
    (0x1e9, Sk::F, 0x2ac), (0x1ee, Sk::M8, 0x48c), (0x1f0, Sk::W, 0x2c8), (0x1f1, Sk::I32, 0x3f4),
    (0x1f2, Sk::I32, 0x3f8), (0x1f3, Sk::W, 0x3e2), (0x1f4, Sk::W, 0x3de), (0x1f5, Sk::F, 0x258),
    (0x1f6, Sk::W, 0x13c), (0x1f7, Sk::T5, 0x184), (0x1f8, Sk::T5, 0x180), (0x1f9, Sk::W, 0x228),
    (0x1fa, Sk::W, 0x224), (0x1fb, Sk::F, 0x154), (0x1fc, Sk::F, 0x214), (0x1fd, Sk::W, 0x22a),
    (0x1fe, Sk::W, 0x226), (0x1ff, Sk::T5, 0x17c), (0x200, Sk::I16, 0x1e4), (0x201, Sk::I16, 0x158),
    (0x209, Sk::I16, 0x1f4), (0x20b, Sk::I16, 0x1da), (0x20c, Sk::I16, 0x59a), (0x20d, Sk::I16, 0x59c),
    (0x20e, Sk::W, 0x24c), (0x20f, Sk::W, 0x58e), (0x210, Sk::F, 0x21c), (0x211, Sk::T5, 0x490),
    (0x213, Sk::W, 0x25c), (0x228, Sk::F, 0x16c), (0x22d, Sk::F, 0x1fc), (0x22e, Sk::F, 0x200),
    (0x22f, Sk::F, 0x204), (0x230, Sk::F, 0x430), (0x231, Sk::F, 0x434), (0x232, Sk::F, 0x438),
    (0x239, Sk::W, 0x598), (0x247, Sk::S8, 0x49c), (0x248, Sk::S8, 0x49e), (0x249, Sk::I32, 0x4a0),
    (0x24b, Sk::F, 0x4ac), (0x24c, Sk::F, 0x4b0), (0x24d, Sk::F, 0x4b4), (0x24e, Sk::F, 0x4b8),
    (0x24f, Sk::W, 0x59e), (0x250, Sk::W, 0x5a4), (0x251, Sk::W, 0x5aa), (0x252, Sk::W, 0x5a6),
    (0x253, Sk::W, 0x5a8), (0x254, Sk::W, 0x5a0), (0x255, Sk::W, 0x5a2), (0x256, Sk::FT, 0x4bc),
    (0x257, Sk::W, 0x4c0), (0x258, Sk::W, 0x4c2), (0x259, Sk::W, 0x4c8), (0x25a, Sk::W, 0x4ca),
    (0x25b, Sk::W, 0x4cc), (0x25c, Sk::W, 0x4ce), (0x25d, Sk::W, 0x4d0), (0x25e, Sk::W, 0x4d2),
    (0x25f, Sk::T5, 0x4d4), (0x261, Sk::I32, 0x4dc), (0x262, Sk::I32, 0x4e0), (0x263, Sk::I32, 0x4e4),
    (0x264, Sk::I32, 0x4e8), (0x265, Sk::I32, 0x4ec), (0x267, Sk::FT, 0x4f4), (0x268, Sk::FT, 0x4f8),
    (0x269, Sk::FT, 0x4fc), (0x26a, Sk::FT, 0x500), (0x275, Sk::W, 0x4c4), (0x276, Sk::W, 0x4c6),
];

/// One parameter of the render control record (`SI_RenControl`, the scene's rendering options) into a value record
/// [soft 0x080fac08 SI_sceneGetParam]. The record starts as the float 0. [`SCENE_PARAMS`] lists the fields copied straight; the rest are
/// the frame globals (0x163 current, 0x164 start, 0x165 end, 0x167 step, 0x221 pause: integers; 0x166 the rate: a float), 0x1a4
/// (the second word is -1 when the word at +0x158 is 1, else the signed word at +0x15a), 0x1b3 (the second word: the signed word
/// at +0x13e), 0x1bc, 0x1c1 and 0x1c2 (the field of the mental-ray set when `SI_getmi2Output` answers non-zero), 0x1c8 and 0x1cb (the
/// second word: the word at +0x2cc and +0x448), 0x202 to 0x205 (the floats at +0x238, +0x22c, +0x230, +0x234), 0x215 and 0x260
/// (type 3, the second word: a signed word), 0x229 (a signed word at +0x17a, type 4). Any other type answers 0.
///
/// `SI_edSumatraTo3x` = 0 on the full product's path: the branches for it being non-zero (which turn some of these into
/// the successor product's codes) are left out.
pub fn scene_get_param(m: &mut Mem, o: &mut dyn Outside, fcv_type: i32, ctrl: u32, out: u32) -> u32 {
    m.set_u32(out, 1);
    let frame_global = |m: &mut Mem, addr: u32| {
        m.set_u32(out, 4);
        let v = m.u32(addr);
        m.set_u32(out + 4, v);
    };
    if let Some(&(_, k, off)) = SCENE_PARAMS.iter().find(|(c, _, _)| *c as i32 == fcv_type) {
        let a = ctrl + off as u32;
        match k {
            Sk::F => fcopy(m, out + 4, a),
            Sk::FT => {
                fcopy(m, out + 4, a);
                m.set_u32(out, 1);
            }
            Sk::W => {
                let v = m.u16(a);
                m.set_u16(out + 4, v);
                m.set_u32(out, 2);
            }
            Sk::I16 => {
                let v = m.i16(a) as i32;
                m.set_i32(out + 4, v);
                m.set_u32(out, 4);
            }
            Sk::I32 => {
                let v = m.u32(a);
                m.set_u32(out + 4, v);
                m.set_u32(out, 4);
            }
            Sk::T5 => {
                let v = m.u32(a);
                m.set_u32(out + 4, v);
                m.set_u32(out, 5);
            }
            Sk::M8 => {
                let v = m.u32(a);
                m.set_u32(out + 8, v);
            }
            Sk::S8 => {
                let v = m.i16(a) as i32;
                m.set_i32(out + 8, v);
            }
        }
        return 1;
    }
    match fcv_type {
        0x163 => frame_global(m, 0x0878_2040),
        0x164 => frame_global(m, 0x0878_2050),
        0x165 => frame_global(m, 0x0878_2044),
        0x167 => frame_global(m, 0x0878_2054),
        0x221 => frame_global(m, 0x0878_2048),
        0x166 => fcopy(m, out + 4, 0x0878_204c),
        0x1a4 => {
            let v = if m.i32(ctrl + 0x158) == 1 { -1 } else { m.i16(ctrl + 0x15a) as i32 };
            m.set_i32(out + 8, v);
        }
        0x1b3 => {
            let v = m.i16(ctrl + 0x13e) as i32;
            m.set_i32(out + 8, v);
        }
        0x1bc => {
            let mi = o.get_mi2_output(m) != 0;
            let v = m.u32(ctrl + if mi { 0x404 } else { 0x400 });
            m.set_u32(out + 8, v);
        }
        0x1c1 | 0x1c2 => {
            let mi = o.get_mi2_output(m) != 0;
            let off = match (fcv_type, mi) {
                (0x1c1, false) => 0x414,
                (0x1c1, true) => 0x41c,
                (_, false) => 0x418,
                _ => 0x420,
            };
            let v = m.u32(ctrl + off);
            m.set_u32(out + 4, v);
            m.set_u32(out, 4);
        }
        0x1c8 => {
            let v = m.u32(ctrl + 0x2cc);
            m.set_u32(out + 8, v);
        }
        0x1cb => {
            let v = m.u32(ctrl + 0x448);
            m.set_u32(out + 8, v);
        }
        0x202 => fcopy(m, out + 4, ctrl + 0x238),
        0x203 => fcopy(m, out + 4, ctrl + 0x22c),
        0x204 => fcopy(m, out + 4, ctrl + 0x230),
        0x205 => fcopy(m, out + 4, ctrl + 0x234),
        0x212 => {
            let v = m.u32(ctrl + 0x494);
            m.set_u32(out + 4, v);
            m.set_u32(out, 5);
        }
        0x214 => {
            let v = m.u16(ctrl + 0x100);
            m.set_u16(out + 4, v);
            m.set_u32(out, 2);
        }
        0x215 | 0x260 => {
            m.set_u32(out, 3);
            let v = m.i16(ctrl + if fcv_type == 0x215 { 0xfa } else { 0x4d8 }) as i32;
            m.set_i32(out + 8, v);
        }
        0x227 | 0x22a | 0x22b | 0x22c => {
            m.set_u32(out, 2);
            let off = match fcv_type {
                0x227 => 0xfe,
                0x22a => 0x1e6,
                0x22b => 0x1e8,
                _ => 0x1ea,
            };
            let v = m.u16(ctrl + off);
            m.set_u16(out + 4, v);
        }
        0x229 => {
            m.set_u32(out, 4);
            let v = m.i16(ctrl + 0x17a) as i32;
            m.set_i32(out + 4, v);
        }
        0x266 => {
            let v = m.u32(ctrl + 0x4f0);
            m.set_u32(out + 4, v);
            m.set_u32(out, 4);
        }
        0x26b => {
            fcopy(m, out + 4, ctrl + 0x504);
            m.set_u32(out, 1);
        }
        _ => return 0,
    }
    1
}
