// SPDX-License-Identifier: MIT
//! Save key for cameras, lights and the two deformations of a model (`motionKey.c`).
//!
//! The camera and light commands have the shape of the model's (`savekey.rs`): one routine
//! that puts one key on one curve of the object, with the path key and the refusals folded
//! into it ([`savekey_single_cam`], [`savekey_lit`], [`savekey_triple_lit`]), and commands
//! above it that pick the values ([`savekey_camera`], [`savekey_light`],
//! [`savekey_light_matter`]). The fitting commands go through the model's own routine.

use crate::ed;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::{grafnode, model, pathslot};
use crate::path;
use crate::savekey::{self, SaveKeyHost, RAD2DEG};
use crate::update::params::o::{camera as cam, light as lit};
use crate::update::shape::fit;
use si_x87::x;

/// The texts of these routines, at their addresses in the binary's data.
pub mod msg {
    use si_mem::Addr;
    /// "Camera already has a path position fcurve." / "… path interest …"
    pub const CAM_HAS_PATH_POSITION: Addr = 0x0869_71a0;
    pub const CAM_HAS_PATH_INTEREST: Addr = 0x0869_71e0;
    /// "Camera already has explicit position." / "… explicit interest …"
    pub const CAM_HAS_EXPLICIT_POSITION: Addr = 0x0869_7220;
    pub const CAM_HAS_EXPLICIT_INTEREST: Addr = 0x0869_7260;
    /// The three texts of `si_savekeyTripleLit`.
    pub const LIT3_HAS_PATH_POSITION: Addr = 0x0869_72a0;
    pub const LIT3_HAS_PATH_INTEREST: Addr = 0x0869_72e0;
    pub const LIT3_WRONG_TYPE: Addr = 0x0869_7320;
    /// The five texts of `SI_savekeyLit`.
    pub const LIT_HAS_PATH_INTEREST: Addr = 0x0869_7360;
    pub const LIT_HAS_PATH_POSITION: Addr = 0x0869_73a0;
    pub const LIT_HAS_EXPLICIT_POSITION: Addr = 0x0869_73e0;
    pub const LIT_HAS_EXPLICIT_INTEREST: Addr = 0x0869_7420;
    pub const LIT_WRONG_TYPE: Addr = 0x0869_7460;
}

/// Light types (the 16-bit word at light + 0x1e) as these routines tell them apart: a spot
/// has an interest and a cone, a sun has neither a position of its own nor a fall-off, an
/// infinite light no fall-off.
pub mod kind {
    pub const SPOT: i16 = 2;
    pub const INFINITE: i16 = 3;
    pub const SUN: i16 = 4;
}

crate::host_trait! {
    /// What the camera and light save-key commands call outside this package.
    pub trait SaveKey2Host: SaveKeyHost {
        /// `SI_displaySunPositionDialog(light, 1)`: the sun's dialogue, before a sun is keyed.
        fn display_sun_position_dialog(light: Addr, flag: i32);
    }
}

/// The element of a camera and its index (`SI_edGetElemFromCamera(camera, &index)`).
fn camera_elem<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, camera: Addr) -> (Addr, i32) {
    let idx = m.local(4);
    let e = h.ed_get_elem_from_camera(m, camera, idx);
    let index = m.i32(idx);
    m.release(4);
    (e, index)
}

fn light_elem<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, light: Addr) -> (Addr, i32) {
    let idx = m.local(4);
    let e = h.ed_get_elem_from_light(m, light, idx);
    let index = m.i32(idx);
    m.release(4);
    (e, index)
}

/// The present position `from` of a camera or light becomes a point of the path in `slot`,
/// with a key on the percentage curve `curve`: the position through the inverse of the path
/// model's global matrix ([`savekey::savekey_map_translation`]), then `SI_pathAddKeyPct` for
/// a spline path (model kind 6), `SI_nurbsPathAddKeyPct` for any other.
fn path_key<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr, slot: u32, from: u32, curve: Addr, keyed: i16) {
    let v = m.local(12);
    for k in 0..3 {
        let w = m.u32(obj + from + 4 * k);
        m.set_u32(v + 4 * k, w);
    }
    let p = m.ptr(obj + slot);
    savekey::savekey_map_translation(m, p, v);
    let p = m.ptr(obj + slot);
    let geom = m.ptr(p + model::DEFINITION);
    if m.i32(p + model::KIND) == 6 {
        path::path_add_key_pct(m, h, geom, curve, v, keyed);
    } else {
        path::nurbs_path_add_key_pct(m, h, geom, curve, v, keyed);
    }
    m.release(12);
}

/// One key on one curve of a camera [soft 0x0821b6c4 SI_savekeySingleCam].
///
/// `group` and `relid` name the relation, `kind` the curve type; with `check` the camera's
/// curves of the relation are searched for one of that type (without it the first curve of
/// the relation is taken); a missing curve is made (`SI_fcurveAllocate(kind, camera)`) and
/// filed in the CAMERAS chapter. Before that, by type (every look-up below is made in list
/// `group`, as the caller passed it):
///
/// | Type | |
/// |---|---|
/// | 9 position on a path | refused with "Camera already has explicit position." when the element has curves of relation 1113; refused when the path (+0xa0) cannot take a point ([`savekey::savekey_valid_translation`]) |
/// | 10 interest on a path | the same with relation 1115 and the path at +0xa4 |
/// | 142–144 position | refused with "… already has a path position …" when the element has a curve of relation 1112 or the camera a position path |
/// | 145–147 interest | the same with relation 1114 and the interest path |
///
/// Then the key: for the types 9 and 10 a path is made if there is none ([`path::path_new`]
/// type 3, the slot set, [`path::path_element_set`] with graf kind 4 or 5), and the position
/// (+0x10) or interest (+0x1c) becomes a point of it; for any other type a key with `value`
/// at the current frame.
#[allow(clippy::too_many_arguments)]
pub fn savekey_single_cam<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, camera: Addr, group: i32, relid: i32, kind: i32, check: i16,
                                                   value: f32, keyed: i16) {
    let (e, _) = camera_elem(m, h, camera);
    if e == 0 {
        return;
    }
    match kind {
        9 | 10 => {
            let (explicit, text, slot) = if kind == 9 {
                (1113, msg::CAM_HAS_EXPLICIT_POSITION, pathslot::CAMERA_POSITION)
            } else {
                (1115, msg::CAM_HAS_EXPLICIT_INTEREST, pathslot::CAMERA_INTEREST)
            };
            if ed::find_fcv(m, e, group, explicit) != 0 {
                KeyHost::status_display_message(h, m, text, 3);
                return;
            }
            let p = m.ptr(camera + slot);
            if !savekey::savekey_valid_translation(m, h, p) {
                return;
            }
        }
        0x8e..=0x90 => {
            if ed::find_fcv(m, e, group, 1112) != 0 || m.ptr(camera + pathslot::CAMERA_POSITION) != 0 {
                KeyHost::status_display_message(h, m, msg::CAM_HAS_PATH_POSITION, 3);
                return;
            }
        }
        0x91..=0x93 => {
            if ed::find_fcv(m, e, group, 1114) != 0 || m.ptr(camera + pathslot::CAMERA_INTEREST) != 0 {
                KeyHost::status_display_message(h, m, msg::CAM_HAS_PATH_INTEREST, 3);
                return;
            }
        }
        _ => {}
    }
    let (e, index) = camera_elem(m, h, camera);
    let mut c = ed::find_fcv(m, e, group, relid);
    if check != 0 {
        while c != 0 && !savekey::fcv_check_type_range(m, c, kind, kind) {
            c = ed::find_next_fcv(m, e, group, relid);
        }
    }
    if c == 0 {
        c = key::fcurve_allocate(m, kind, camera);
        let chap = glob::chap(m, cid::CAMERAS);
        h.ed_fcv_add(m, c, e, chap, index);
    }
    match kind {
        9 | 10 => {
            let (slot, graf, from) =
                if kind == 9 { (pathslot::CAMERA_POSITION, 4, cam::POSITION) } else { (pathslot::CAMERA_INTEREST, 5, cam::INTEREST) };
            if m.ptr(camera + slot) == 0 {
                let p = path::path_new(m, h, 3);
                m.set_ptr(camera + slot, p);
                path::path_element_set(m, h, camera, graf, p);
            }
            path_key(m, h, camera, slot, from, c, keyed);
        }
        _ => key::add_key_now(m, h, c, value, c),
    }
}

/// Save key, a camera [soft 0x0821b9e8 SI_savekeyCamera]. Nothing for a camera without an
/// element. In this order:
///
/// - position: when the element has curves of relation 1113 in list 1, keys of types 142,
///   143, 144 ← +0x10, +0x14, +0x18 (each looked up by type); otherwise a path key (relation
///   1112, type 9);
/// - interest: the same with relation 1115, types 145–147 ← +0x1c …, or a path key (1114,
///   type 10);
/// - roll: list 1, relation 1130, type 11 ← +0x30 × 57.29577951308232 (the double at
///   0x0864d8d8), rounded to `float`;
/// - field of view: list 0, 1131, type 12 ← 57.29577951308232 × +0x40;
/// - 1132 … 1140, types 13 … 21 ← +0x34, +0x38, +0x70, +0x94, +0x74, +0x7c, +0x84, +0x8c,
///   +0x98;
/// - visibility: 1100, type 177 ← 1 when the graf node's display type (+0xc) is 2, else 0.
///
/// The projection plane (relations 1141 to 1144), which the update applies, gets no key.
pub fn savekey_camera<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, camera: Addr, gn: Addr) {
    let (e, _) = camera_elem(m, h, camera);
    if e == 0 {
        return;
    }
    for (explicit, first, from, path_rel, path_kind) in [(1113, 0x8e, cam::POSITION, 1112, 9), (1115, 0x91, cam::INTEREST, 1114, 10)] {
        if ed::find_fcv(m, e, 1, explicit) == 0 {
            savekey_single_cam(m, h, camera, 1, path_rel, path_kind, 0, 0.0, 0);
        } else {
            for k in 0..3 {
                let v = m.f32(camera + from + 4 * k as u32);
                savekey_single_cam(m, h, camera, 1, explicit, first + k, 1, v, 0);
            }
        }
    }
    let roll = (x(m.f32(camera + cam::ROLL)) * RAD2DEG).to_f32();
    savekey_single_cam(m, h, camera, 1, 1130, 11, 0, roll, 0);
    let fov = (x(RAD2DEG) * m.f32(camera + cam::FOV)).to_f32();
    savekey_single_cam(m, h, camera, 0, 1131, 12, 0, fov, 0);
    for (k, at) in [cam::NEAR, cam::FAR, cam::FOCAL_LENGTH, cam::F_STOP, cam::FOCAL_DISTANCE, cam::NEAR_FOCUS, cam::FAR_FOCUS, cam::OUT_OF_FOCUS,
                    cam::MAXIMUM_COC].into_iter()
                                     .enumerate()
    {
        let v = m.f32(camera + at);
        savekey_single_cam(m, h, camera, 0, 1132 + k as i32, 13 + k as i32, 0, v, 0);
    }
    let shown = if m.i32(gn + grafnode::DISPLAY) == 2 { 1.0 } else { 0.0 };
    savekey_single_cam(m, h, camera, 0, 1100, 0xb1, 0, shown, 0);
}

/// Three keys on the three curves of a light's colour, position or interest
/// [soft 0x0821bd34 si_savekeyTripleLit].
///
/// Nothing for a light without an element, and nothing for a sun (type 4) unless the
/// relation is the colour's (2130). For the position (`kind` 148): refused with "Light
/// already has a path position …" when the element has a curve of relation 2112 **in list
/// `group`** or the light a position path; not for a sun. For the interest (151): the same
/// with relation 2114 and the interest path; only for a spot. A light of the wrong type gets
/// "Incorrect light type for this property". Otherwise the three curves of `relid` are
/// looked up or made ([`key::fcurve_check_add3`]) and keyed, each with its own interpolation.
pub fn savekey_triple_lit<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, light: Addr, group: i32, relid: i32, kind: i32, v: [f32; 3]) {
    let (e, index) = light_elem(m, h, light);
    let ty = m.i16(light + lit::KIND);
    if e == 0 || (ty == kind::SUN && relid != 2130) {
        return;
    }
    let mut ok = true;
    if kind == 0x94 {
        ok = ty != kind::SUN;
        if ed::find_fcv(m, e, group, 2112) != 0 || m.ptr(light + pathslot::LIGHT_POSITION) != 0 {
            KeyHost::status_display_message(h, m, msg::LIT3_HAS_PATH_POSITION, 3);
            return;
        }
    } else if kind == 0x97 {
        ok = ty == kind::SPOT;
        if ed::find_fcv(m, e, group, 2114) != 0 || m.ptr(light + pathslot::LIGHT_INTEREST) != 0 {
            KeyHost::status_display_message(h, m, msg::LIT3_HAS_PATH_INTEREST, 3);
            return;
        }
    }
    if !ok {
        KeyHost::status_display_message(h, m, msg::LIT3_WRONG_TYPE, 3);
        return;
    }
    let c = key::fcurve_check_add3(m, h, e, index, relid);
    for k in 0..3 {
        key::add_key_now(m, h, c[k], v[k], c[k]);
    }
}

/// One key on one curve of a light [soft 0x0821bef4 SI_savekeyLit].
///
/// The curve is found **by its type** (`SI_fcurveCheckAdd(elem, index, kind)`: the relation
/// comes from the attribute dictionary); the relation id the caller passes is not looked at.
/// Before that, by type (look-ups in list `group`):
///
/// | Type | Light type wanted | Refusal |
/// |---|---|---|
/// | 31 position on a path | not a sun | "Light already has explicit position." when the element has curves of relation 2113; the path (+0x144) must be able to take a point |
/// | 32 interest on a path | a spot | the same with 2115 and +0x148 |
/// | 25, 26 cone, spread | a spot | |
/// | 28, 29 fall-off start, end | type 1 or 2 | |
/// | 148–150 position | not a sun | "… already has a path position …" when the element has a curve of relation 2112 or the light a position path |
/// | 151–153 interest | a spot | the same with 2114 and the interest path |
///
/// The refusals come first; then a light of the wrong type gets "Incorrect light type for
/// this property". For the types 31 and 32 a path is made if there is none (`SI_pathNew(3)`,
/// the slot set, `SI_pathElementSet` with graf kind 6 or 7) and the position (+0x34) or
/// interest (+0x58) becomes a point of it; any other type gets a key with `value`.
pub fn savekey_lit<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, light: Addr, group: i32, _relid: i32, kind: i32, value: f32, keyed: i16) {
    let (e, _) = light_elem(m, h, light);
    if e == 0 {
        return;
    }
    let ty = m.i16(light + lit::KIND);
    let mut ok = true;
    match kind {
        0x1f | 0x20 => {
            let (explicit, text, slot) = if kind == 0x1f {
                ok = ty != kind::SUN;
                (2113, msg::LIT_HAS_EXPLICIT_POSITION, pathslot::LIGHT_POSITION)
            } else {
                ok = ty == kind::SPOT;
                (2115, msg::LIT_HAS_EXPLICIT_INTEREST, pathslot::LIGHT_INTEREST)
            };
            if ed::find_fcv(m, e, group, explicit) != 0 {
                KeyHost::status_display_message(h, m, text, 3);
                return;
            }
            let p = m.ptr(light + slot);
            if !savekey::savekey_valid_translation(m, h, p) {
                return;
            }
        }
        0x19 | 0x1a => ok = ty == kind::SPOT,
        0x1c | 0x1d => ok = (ty.wrapping_sub(1) as u16) < 2,
        0x94..=0x96 => {
            ok = ty != kind::SUN;
            if ed::find_fcv(m, e, group, 2112) != 0 || m.ptr(light + pathslot::LIGHT_POSITION) != 0 {
                KeyHost::status_display_message(h, m, msg::LIT_HAS_PATH_POSITION, 3);
                return;
            }
        }
        0x97..=0x99 => {
            ok = ty == kind::SPOT;
            if ed::find_fcv(m, e, group, 2114) != 0 || m.ptr(light + pathslot::LIGHT_INTEREST) != 0 {
                KeyHost::status_display_message(h, m, msg::LIT_HAS_PATH_INTEREST, 3);
                return;
            }
        }
        _ => {}
    }
    if !ok {
        KeyHost::status_display_message(h, m, msg::LIT_WRONG_TYPE, 3);
        return;
    }
    let (e, index) = light_elem(m, h, light);
    if e == 0 {
        return;
    }
    let c = key::fcurve_check_add(m, h, e, index, kind);
    match kind {
        0x1f => {
            if m.ptr(light + pathslot::LIGHT_POSITION) == 0 {
                let p = path::path_new(m, h, 3);
                path::light_set_position_path(m, light, p);
                path::path_element_set(m, h, light, 6, p);
            }
            path_key(m, h, light, pathslot::LIGHT_POSITION, lit::POSITION, c, keyed);
        }
        0x20 => {
            if m.ptr(light + pathslot::LIGHT_INTEREST) == 0 {
                let p = path::path_new(m, h, 3);
                path::light_set_interest_path(m, light, p);
                path::path_element_set(m, h, light, 7, p);
            }
            let (e, _) = light_elem(m, h, light);
            if e == 0 {
                return;
            }
            path_key(m, h, light, pathslot::LIGHT_INTEREST, lit::INTEREST, c, keyed);
        }
        _ => key::add_key_now(m, h, c, value, c),
    }
}

/// Save key, the parameters of a light [soft 0x0821c3f8 SI_savekeyLightMatter]:
///
/// - a spot: cone 2131 (type 25) ← +0x70 × 57.29577951308232 and spread 2132 (26) ←
///   57.29577951308232 × +0x74, in degrees (the double at 0x0864d8e0);
/// - colour 2130 (types 22–24) ← +0x24 …;
/// - unless infinite or a sun: fall-off start 2134 (28) ← +0x84 and end 2135 (29) ← +0x88;
/// - shadow factor 2136 (30) ← +0x90;
/// - a sun: list 1, 2120 (type 189) ← the sun record's date and time (+0x16c → +0x10, an
///   integer) as a `float`.
///
/// The area and photon parameters, which the update applies, get no key.
pub fn savekey_light_matter<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, light: Addr) {
    if m.i16(light + lit::KIND) == kind::SPOT {
        let cone = (x(m.f32(light + lit::CONE_ANGLE)) * RAD2DEG).to_f32();
        savekey_lit(m, h, light, 0, 2131, 0x19, cone, 0);
        let spread = (x(RAD2DEG) * m.f32(light + lit::SPREAD_ANGLE)).to_f32();
        savekey_lit(m, h, light, 0, 2132, 0x1a, spread, 0);
    }
    let colour = [m.f32(light + lit::COLOUR), m.f32(light + lit::COLOUR + 4), m.f32(light + lit::COLOUR + 8)];
    savekey_triple_lit(m, h, light, 0, 2130, 0x16, colour);
    if (m.i16(light + lit::KIND).wrapping_sub(kind::INFINITE) as u16) > 1 {
        let v = m.f32(light + lit::FALL_OFF_START);
        savekey_lit(m, h, light, 0, 2134, 0x1c, v, 0);
        let v = m.f32(light + lit::FALL_OFF_END);
        savekey_lit(m, h, light, 0, 2135, 0x1d, v, 0);
    }
    let v = m.f32(light + lit::SHADOW_FACTOR);
    savekey_lit(m, h, light, 0, 2136, 0x1e, v, 0);
    if m.i16(light + lit::KIND) == kind::SUN {
        let sun = m.ptr(light + lit::SUN);
        let v = m.i32(sun + lit::SUN_DATE_TIME) as f32;
        savekey_lit(m, h, light, 1, 2120, 0xbd, v, 0);
    }
}

/// Save key, a light [soft 0x0821c2b0 si_savekeyLight]. Nothing for a light without an
/// element. A sun first gets its dialogue (`SI_displaySunPositionDialog(light, 1)`). Then
/// the parameters ([`savekey_light_matter`]); the visibility (list 0, type 176 ← 1 when the
/// graf node's display type is 2, else 0); and, unless the light is a sun, the position:
/// keys of types 148–150 ← +0x34 … when the element has curves of relation 2113 in list 1,
/// otherwise a path key (type 31); for a spot the interest likewise (2115, types 151–153 ←
/// +0x58 …, or a path key of type 32).
///
/// The explicit keys are asked for with list **0** (`si_savekeyTripleLit(light, 0, …)`), so
/// that routine's own look-up of a path curve, made in the list it is given, finds nothing.
pub fn savekey_light<H: SaveKey2Host + ?Sized>(m: &mut Mem, h: &mut H, light: Addr, gn: Addr) {
    let (e, _) = light_elem(m, h, light);
    if e == 0 {
        return;
    }
    if m.i16(light + lit::KIND) == kind::SUN {
        h.display_sun_position_dialog(m, light, 1);
    }
    savekey_light_matter(m, h, light);
    let shown = if m.i32(gn + grafnode::DISPLAY) == 2 { 1.0 } else { 0.0 };
    savekey_lit(m, h, light, 0, 2100, 0xb0, shown, 0);
    if m.i16(light + lit::KIND) == kind::SUN {
        return;
    }
    if ed::find_fcv(m, e, 1, 2113) == 0 {
        savekey_lit(m, h, light, 1, 2112, 0x1f, 0.0, 0);
    } else {
        let v = [m.f32(light + lit::POSITION), m.f32(light + lit::POSITION + 4), m.f32(light + lit::POSITION + 8)];
        savekey_triple_lit(m, h, light, 0, 2113, 0x94, v);
    }
    if m.i16(light + lit::KIND) == kind::SPOT {
        if ed::find_fcv(m, e, 1, 2115) == 0 {
            savekey_lit(m, h, light, 1, 2114, 0x20, 0.0, 0);
        } else {
            let v = [m.f32(light + lit::INTEREST), m.f32(light + lit::INTEREST + 4), m.f32(light + lit::INTEREST + 8)];
            savekey_triple_lit(m, h, light, 0, 2115, 0x97, v);
        }
    }
}

/// Save key, a deformation by spline [soft 0x0821c554 si_savekeyFitAll]: nothing without a
/// fitting record or with an inactive one (first word zero); otherwise seven keys through
/// [`savekey::savekey_model`] in list 2, each curve looked up by type: for the node
/// (`branch` zero) relation 220, types 107 … 113; for the branch 221, types 114 … 120:
/// scaling ← +0x0c, +0x10, +0x14; roll ← +0x18 × 57.29577951308232 (the double at
/// 0x0864d8e8); translation ← +0x1c, +0x20, +0x24.
pub fn savekey_fit_all<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, rec: Addr, branch: i16) {
    if rec == 0 || m.i16(rec) == 0 {
        return;
    }
    let (relid, first) = if branch == 0 { (220, 0x6b) } else { (221, 0x72) };
    for k in 0..7u32 {
        let at = rec + fit::SPL_SCALING + 4 * k;
        let v = if k == 3 { (x(m.f32(at)) * RAD2DEG).to_f32() } else { m.f32(at) };
        savekey::savekey_model(m, h, mdl, 2, relid, first + k as i32, 1, v, 0, rec);
    }
}

/// Save key, a deformation by patch [soft 0x0821c764 si_savekeyPFitAll]: as
/// [`savekey_fit_all`], nine keys: for the node relation 230, types 121 … 129; for the branch
/// 231, types 130 … 138: scaling ← +0x08 …; rotation ← 57.29577951308232 × +0x14, +0x18,
/// +0x1c (the double at 0x0864d8f0); translation ← +0x20 ….
pub fn savekey_pfit_all<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, rec: Addr, branch: i16) {
    if rec == 0 || m.i16(rec) == 0 {
        return;
    }
    let (relid, first) = if branch == 0 { (230, 0x79) } else { (231, 0x82) };
    for k in 0..9u32 {
        let at = rec + fit::PTCH_SCALING + 4 * k;
        let v = if (3..6).contains(&k) { (x(RAD2DEG) * m.f32(at)).to_f32() } else { m.f32(at) };
        savekey::savekey_model(m, h, mdl, 2, relid, first + k as i32, 1, v, 0, rec);
    }
}
