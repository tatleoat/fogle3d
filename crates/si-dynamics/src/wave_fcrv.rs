// SPDX-License-Identifier: MIT
//! `wave_fcrv.c` of Softimage|3D 4.0 (0x084c5290 … 0x084c5c6f): the function curves of a wave and the keys saved on them
//! (finding 0045). The animation-editor and curve routines are reached through [`ParticleCalls`]. The wave record is
//! described in `wave_sys`; here +0x10 and +0x14 are the first and last frame and +0x28 a flag that sets the
//! extrapolation of a new curve (the short at +0x1a of a curve) to 2.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::x;

const SI_FRAME_CURRENT: u32 = 0x0878_2040;
const STDERR: u32 = 0x0878_0c88;
/// The word the animation-editor routines are given with a new curve (`DAT_087816d0`).
const ED_WORD: u32 = 0x0878_16d0;
/// 57.29577951308232, the double at 0x086613a0 (and 0x086613d0): degrees per radian.
const DEG: u32 = 0x0866_13a0;
const DEG2: u32 = 0x0866_13d0;

/// `fprintf(stderr, text)` for a text with no conversion.
fn warn(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    let s = m.cstr(text);
    let stream = m.u32(STDERR);
    o.print(stream, &s);
}

/// The kind (first or second argument of the editor's search) and the channel for a curve id of a wave, `None` for an
/// id the switch does not know.
fn kind_channel(id: i32) -> Option<(i32, i32)> {
    match id {
        0xa3..=0xa5 => Some((1, 0x262)),
        0xa6..=0xa8 => Some((1, 0x263)),
        0xa9..=0xab => Some((1, 0x265)),
        0xac => Some((1, 0x264)),
        0xad => Some((0, 0x266)),
        0xae => Some((0, 0x267)),
        _ => None,
    }
}

/// The curve of a wave with that id [soft 0x084c5290 SI_getWaveFcurvePointer]: 0 for a null wave (a message on stderr), a
/// wave with no element, an unknown id or no such curve; else the search of the editor is stepped until the curve's id
/// (its first word) is the one asked for.
pub fn si_get_wave_fcurve_pointer(m: &mut Mem, o: &mut dyn Outside, wave: u32, id: i32) -> u32 {
    if wave == 0 {
        warn(m, o, 0x086d_7ee0);
        return 0;
    }
    let out = m.local(8);
    let elem = o.si_ed_get_elem_from_wave(m, wave, out + 4);
    let r = (|| {
        if elem == 0 {
            return 0;
        }
        let Some((kind, ch)) = kind_channel(id) else { return 0 };
        let mut fcv = o.si_ed_find_fcv(m, elem, kind, ch);
        if fcv == 0 {
            return 0;
        }
        while m.i32(fcv) != id {
            fcv = o.si_ed_find_next_fcv(m, elem, kind, ch);
            if fcv == 0 {
                return 0;
            }
        }
        fcv
    })();
    m.release(8);
    r
}

/// Like [`si_get_wave_fcurve_pointer`] [soft 0x084c5378 SI_getWaveFcurvePointerPointer] but the result is the address of
/// a local that holds the curve (0 if it was not found) and so is the address of a stack slot that is gone when the
/// function returns; the address is returned as the original does. 0 for a null wave (message on stderr), no element
/// or an unknown id.
pub fn si_get_wave_fcurve_pointer_pointer(m: &mut Mem, o: &mut dyn Outside, wave: u32, id: i32) -> u32 {
    if wave == 0 {
        warn(m, o, 0x086d_7f20);
        return 0;
    }
    let loc = m.local(12);
    let elem = o.si_ed_get_elem_from_wave(m, wave, loc + 8);
    let mut r = 0;
    if elem != 0 {
        if let Some((kind, ch)) = kind_channel(id) {
            let mut fcv = o.si_ed_find_fcv(m, elem, kind, ch);
            while fcv != 0 && m.i32(fcv) != id {
                fcv = o.si_ed_find_next_fcv(m, elem, kind, ch);
            }
            m.set_u32(loc + 4, fcv);
            r = loc + 4;
        }
    }
    m.release(12);
    r
}

/// A key at the current frame with a value on a curve: `SI_fkeyFrameCreate(SI_frameCurrent, v, 0, 0, mode)` then
/// `SI_fcurveAddKey`, the mode being the curve's short at +0x1e.
fn add_key_now(m: &mut Mem, o: &mut dyn Outside, fcv: u32, v: f32) {
    let mode = m.u16(fcv + 0x1e) as i16 as i32;
    let frame = m.i32(SI_FRAME_CURRENT);
    let key = o.si_fkey_frame_create(m, frame, v, 0.0, 0, mode);
    o.si_fcurve_add_key(m, fcv, key);
}

/// Saves one key of the current state of a wave's icon model on the curve with the id given [soft 0x084c546c
/// SI_saveKeySingleWaveFcv]: translation 0xa3..0xa5 (the model's third vector read as scale/rotation/translation),
/// 0xa6..0xa8 the rotation (x, y, z) times 57.29577951308232 (as 80 bits, rounded to f32), 0xa9..0xab explicit position (refused with a
/// message at level 3 when the wave already has a path curve, 0x264).
pub fn si_save_key_single_wave_fcv(m: &mut Mem, o: &mut dyn Outside, wave: u32, id: i32) {
    if wave == 0 {
        return;
    }
    let model = m.u32(wave + 0x74);
    let loc = m.local(0x40);
    // locals: +0 ..: slot for the curve, +4 element index; vectors at +8 (scale), +0x14 (rotation), +0x20 (translation)
    let (idx, sc, rot, tr, slot) = (loc + 4, loc + 8, loc + 0x14, loc + 0x20, loc);
    let elem = o.si_ed_get_elem_from_wave(m, wave, idx);
    if elem == 0 {
        m.release(0x40);
        return;
    }
    o.si_model_state(m, model, sc, rot, tr);
    let v: f32;
    match id {
        0xa3 => v = m.f32(sc),
        0xa4 => v = m.f32(sc + 4),
        0xa5 => v = m.f32(sc + 8),
        0xa6 | 0xa7 | 0xa8 => {
            let r = m.f32(rot + 4 * (id as u32 - 0xa6));
            v = (x(r) * x(m.f64(DEG))).to_f32();
        }
        0xa9..=0xab => {
            if o.si_ed_find_fcv(m, elem, 1, 0x264) != 0 {
                o.si_status_display_message(m, 0x086d_7f60, 3);
                m.release(0x40);
                return;
            }
            v = match id {
                0xa9 => m.f32(tr),
                0xaa => m.f32(tr + 4),
                _ => m.f32(tr + 8),
            };
        }
        // a value outside the switch leaves the original's local unset
        _ => v = 0.0,
    }
    let ei = m.i32(idx);
    o.si_fcurve_check_add(m, elem, ei, id, slot);
    let fcv = m.u32(slot);
    add_key_now(m, o, fcv, v);
    m.release(0x40);
}

/// Saves a key on each of the three scale curves (0x262) of a wave [soft 0x084c55a0 SI_saveKeyWaveScale]: the model's scale
/// vector at the current frame. A null wave prints a message on stderr.
pub fn si_save_key_wave_scale(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    if wave == 0 {
        warn(m, o, 0x086d_7fa0);
        return;
    }
    let model = m.u32(wave + 0x74);
    let loc = m.local(0x30);
    let (idx, c, sc, rot, tr) = (loc, loc + 4, loc + 0x10, loc + 0x1c, loc + 0x28);
    let elem = o.si_ed_get_elem_from_wave(m, wave, idx);
    if elem != 0 {
        let ei = m.i32(idx);
        o.si_fcurve_check_add3(m, elem, ei, 0x262, c, c + 4, c + 8);
        o.si_model_state(m, model, sc, rot, tr);
        for k in 0..3u32 {
            let fcv = m.u32(c + 4 * k);
            let v = m.f32(sc + 4 * k);
            add_key_now(m, o, fcv, v);
        }
    }
    m.release(0x30);
}

/// Saves a key on each of the three rotation curves (0x263) of a wave [soft 0x084c56b4 SI_saveKeyWaveRotate]: the model's
/// rotation times 57.29577951308232 (80 bits, rounded to f32), in degrees. A null wave prints a message on stderr.
pub fn si_save_key_wave_rotate(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    if wave == 0 {
        warn(m, o, 0x086d_7fe0);
        return;
    }
    let model = m.u32(wave + 0x74);
    let loc = m.local(0x30);
    let (idx, c, sc, rot, tr) = (loc, loc + 4, loc + 0x10, loc + 0x1c, loc + 0x28);
    let elem = o.si_ed_get_elem_from_wave(m, wave, idx);
    if elem != 0 {
        let ei = m.i32(idx);
        o.si_fcurve_check_add3(m, elem, ei, 0x263, c, c + 4, c + 8);
        o.si_model_state(m, model, sc, rot, tr);
        for k in 0..3u32 {
            let fcv = m.u32(c + 4 * k);
            let v = (x(m.f64(DEG2)) * x(m.f32(rot + 4 * k))).to_f32();
            add_key_now(m, o, fcv, v);
        }
    }
    m.release(0x30);
}

/// Saves a point on a wave's path [soft 0x084c57f4 SI_saveKeyWaveTranslate]: nothing for a null wave or no element; a
/// wave with explicit position curves (0x265) gets the message "Selected wave already has explicit ..." (level 3). Else a
/// wave model with no path gets `SI_pathNew(kind)`, set on the model and as path element 0xb of the wave; a path with
/// more than one key (the unsigned short at +0x34 of the path) shows a dialogue ("Cannot add a point to the path.") and
/// stops. Then a curve 0x264 is found or made and the model's translation is added to the path's spline (path
/// +0x48) by `SI_pathAddKeyPct(spline, curve, translation, short)`.
pub fn si_save_key_wave_translate(m: &mut Mem, o: &mut dyn Outside, wave: u32, kind: u32, short: i16) {
    if wave == 0 {
        return;
    }
    let model = m.u32(wave + 0x74);
    let loc = m.local(0x30);
    let (slot, idx, v0, v1, v2) = (loc, loc + 4, loc + 8, loc + 0x14, loc + 0x20);
    let elem = o.si_ed_get_elem_from_wave(m, wave, idx);
    if elem != 0 {
        let f = o.si_ed_find_fcv(m, elem, 1, 0x265);
        m.set_u32(slot, f);
        if f == 0 {
            if m.u32(model + 0x2c8) == 0 {
                let p = o.si_path_new(m, kind);
                o.si_model_set_path(m, model, p);
                let p = m.u32(model + 0x2c8);
                o.si_path_element_set(m, wave, 0xb, p);
            } else if 1 < m.u16(m.u32(model + 0x2c8) + 0x34) {
                o.si_dialog_request(m, 0x086d_80a0, 0x086d_8080, 0x086d_8060, 0x086d_804c);
                m.release(0x30);
                return;
            }
            let ei = m.i32(idx);
            o.si_fcurve_check_add1(m, elem, ei, 0x264, slot);
            let spline = m.u32(m.u32(model + 0x2c8) + 0x48);
            o.si_model_state(m, model, v0, v1, v2);
            let f = m.u32(slot);
            o.si_path_add_key_pct(m, spline, f, v2, short as i32);
        } else {
            o.si_status_display_message(m, 0x086d_8020, 3);
        }
    }
    m.release(0x30);
}

/// Saves keys on the three explicit-position curves (0x265) of a wave [soft 0x084c590c SI_saveKeyWaveETranslate]: nothing
/// for a null wave or no element; with a path curve (0x264) the message "The selected wave already has path ..." (level
/// 3); else the model's translation at the current frame.
pub fn si_save_key_wave_etranslate(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    if wave == 0 {
        return;
    }
    let model = m.u32(wave + 0x74);
    let loc = m.local(0x30);
    let (idx, c, tr) = (loc, loc + 4, loc + 0x28);
    let elem = o.si_ed_get_elem_from_wave(m, wave, idx);
    if elem != 0 {
        if o.si_ed_find_fcv(m, elem, 1, 0x264) == 0 {
            let ei = m.i32(idx);
            o.si_fcurve_check_add3(m, elem, ei, 0x265, c, c + 4, c + 8);
            let sc = m.local(24);
            o.si_model_state(m, model, sc, sc + 12, tr);
            for k in 0..3u32 {
                let fcv = m.u32(c + 4 * k);
                let v = m.f32(tr + 4 * k);
                add_key_now(m, o, fcv, v);
            }
            m.release(24);
        } else {
            o.si_status_display_message(m, 0x086d_80e0, 3);
        }
    }
    m.release(0x30);
}

/// Saves the scale, rotation and translation keys of a wave [soft 0x084c5a3c saveKeyWaveAll]: the three routines above,
/// the last with kind 3 and short 0.
pub fn save_key_wave_all(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    si_save_key_wave_scale(m, o, wave);
    si_save_key_wave_rotate(m, o, wave);
    si_save_key_wave_translate(m, o, wave, 3, 0);
}

/// Makes the spread curve (0xae) of a wave [soft 0x084c5a70 SI_waveInitSpread]: nothing for a null wave. An existing curve
/// 0x267 is disposed; a new one is made and added, with a key of 1.0 at each of the wave's frames (+0x10, +0x14); a
/// non-zero flag at +0x28 sets the curve's short at +0x1a to 2.
pub fn si_wave_init_spread(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    if wave == 0 {
        return;
    }
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_wave(m, wave, loc);
    if o.si_ed_find_fcv(m, elem, 0, 0x267) != 0 {
        let w = m.u32(ED_WORD);
        o.si_ed_fcv_dispose(m, wave, w, 0, 0x267, 0x267);
    }
    let fcv = o.si_fcurve_allocate(m, 0xae, wave);
    let (w, ei) = (m.u32(ED_WORD), m.i32(loc));
    o.si_ed_fcv_add(m, fcv, elem, w, ei);
    for off in [0x10u32, 0x14] {
        let frame = m.i32(wave + off);
        let mode = m.u16(fcv + 0x1e) as i16 as i32;
        let key = o.si_fkey_frame_create(m, frame, 1.0, 0.0, 0, mode);
        o.si_fcurve_add_key(m, fcv, key);
    }
    if m.u32(wave + 0x28) != 0 {
        m.set_u16(fcv + 0x1a, 2);
    }
    m.release(4);
}

/// Makes the decay curve (0xad) of a wave [soft 0x084c5b50 SI_waveInitDecay]: nothing for a null wave. An existing curve
/// 0x266 is disposed; a new one is made and added, its short at +0x1e is set to 2, and keys are added at the first
/// frame (0.0, flag 1, mode 2), at the first frame plus a tenth of the span (truncated integer division; 1.0) and at the
/// last frame (0.0); a non-zero flag at +0x28 sets the short at +0x1a to 2.
pub fn si_wave_init_decay(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    if wave == 0 {
        return;
    }
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_wave(m, wave, loc);
    if o.si_ed_find_fcv(m, elem, 0, 0x266) != 0 {
        let w = m.u32(ED_WORD);
        o.si_ed_fcv_dispose(m, wave, w, 0, 0x266, 0x266);
    }
    let fcv = o.si_fcurve_allocate(m, 0xad, wave);
    let (w, ei) = (m.u32(ED_WORD), m.i32(loc));
    o.si_ed_fcv_add(m, fcv, elem, w, ei);
    m.set_u16(fcv + 0x1e, 2);
    let (f0, f1) = (m.i32(wave + 0x10), m.i32(wave + 0x14));
    let key = o.si_fkey_frame_create(m, f0, 0.0, 0.0, 1, 2);
    o.si_fcurve_add_key(m, fcv, key);
    let mode = m.u16(fcv + 0x1e) as i16 as i32;
    let key = o.si_fkey_frame_create(m, f0.wrapping_add(f1.wrapping_sub(f0) / 10), 1.0, 0.0, 1, mode);
    o.si_fcurve_add_key(m, fcv, key);
    let mode = m.u16(fcv + 0x1e) as i16 as i32;
    let key = o.si_fkey_frame_create(m, f1, 0.0, 0.0, 1, mode);
    o.si_fcurve_add_key(m, fcv, key);
    if m.u32(wave + 0x28) != 0 {
        m.set_u16(fcv + 0x1a, 2);
    }
    m.release(4);
}
