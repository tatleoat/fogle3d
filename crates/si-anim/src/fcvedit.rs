// SPDX-License-Identifier: MIT
//! `fcurveEdit.c` (finding 0030, section 18): the data routines of the function-curve
//! editor that change or test the keys of one curve (the slope kinds, the slope vector, the
//! lock test, the dirty mark). The drawing, the mouse loops and the dialogs of the file are
//! not ported (`SI_scannerDraw`, `SI_fcurveDraw`, `SI_fcurveFunc`), and `fcurvePrintAllFrames`
//! (0x08204fac) only prints a debugging table on standard output.
//!
//! A curve (`fcurve`): +0 kind, +8 the keys, +0xc their count, +0x62 a 16-bit "marked
//! dirty" word. A key is 0x24 bytes: +0 time, +4 value, +8 right slope, +0xc the slope-kind
//! word (bit 0 set: slopes are not computed; bit 1: the two sides differ), +0x14 a tag,
//! +0x18 left slope, +0x1c flags (low 4 bits: 1 is "locked"). Key numbers are 1-based.

use crate::outside::{Outside, Routine};
use si_mem::Mem;
use si_x87::{x, Round, F80};

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this module names.
        pub const ROUTINES: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_fcnManageSlope (0x08202194)`: recompute the slopes of a key (curve, 0-based key).
    SI_FCN_MANAGE_SLOPE = "SI_fcnManageSlope", 0x08202194;
    /// `SI_fcvSwapWithAux (0x0820ac4c)`.
    SI_FCV_SWAP_WITH_AUX = "SI_fcvSwapWithAux", 0x0820ac4c;
    /// `si_actionsKeepTiming (0x081e42c4)`.
    SI_ACTIONS_KEEP_TIMING = "si_actionsKeepTiming", 0x081e42c4;
    /// `si_fcurveScale (0x08200ef4)`.
    SI_FCURVE_SCALE = "si_fcurveScale", 0x08200ef4;
    /// `si_copyFkeyToBuffer (0x0820b3b0)`.
    SI_COPY_FKEY_TO_BUFFER = "si_copyFkeyToBuffer", 0x0820b3b0;
    /// `si_pasteFkeyToBuffer (0x0820b4f0)`.
    SI_PASTE_FKEY_TO_BUFFER = "si_pasteFkeyToBuffer", 0x0820b4f0;
    /// `SI_screenToWorld (0x08334878)`.
    SI_SCREEN_TO_WORLD = "SI_screenToWorld", 0x08334878;
    /// `SI_hitProj (0x083346ec)`.
    SI_HIT_PROJ = "SI_hitProj", 0x083346ec;
    /// `SI_grafnodeInitProjection (0x08335c18)`.
    SI_GRAFNODE_INIT_PROJECTION = "SI_grafnodeInitProjection", 0x08335c18;
}

/// Frames per second (`SI_frameRate`).
const FRAME_RATE: u32 = 0x0878204c;
/// `SI_fcurveDirty`, `SI_cusFcurveDirty`, `SI_cusValNumCallback`.
const FCURVE_DIRTY: u32 = 0x086e49dc;
const CUS_FCURVE_DIRTY: u32 = 0x086e49e0;
const CUS_VAL_NUM_CALLBACK: u32 = 0x086e49e8;

fn key(m: &Mem, fcv: u32, idx: i16) -> u32 {
    m.u32(fcv.wrapping_add(8)).wrapping_add((idx as i32).wrapping_mul(0x24).wrapping_sub(0x24) as u32)
}

/// Mark the curve changed once: when the custom-value callback is set and the curve is not
/// marked yet, mark it and count it (custom-parameter curves, kind 0x236, apart).
fn mark_dirty(m: &mut Mem, fcv: u32) {
    if m.i32(CUS_VAL_NUM_CALLBACK) > 0 && m.u16(fcv.wrapping_add(0x62)) == 0 {
        m.set_u16(fcv.wrapping_add(0x62), 1);
        let at = if m.u32(fcv) == 0x236 { CUS_FCURVE_DIRTY } else { FCURVE_DIRTY };
        m.set_i32(at, m.i32(at).wrapping_add(1));
    }
}

/// [soft 0x082040b4 fcurveUnifySlope]
///
/// Make key `idx` use one slope on both sides: the slope-kind word becomes (word & 0xfffd) | 1.
pub fn fcurve_unify_slope(m: &mut Mem, o: &mut dyn Outside, fcv: u32, idx: i16) {
    let k = key(m, fcv, idx);
    let w = m.u16(k.wrapping_add(0xc));
    m.set_u16(k.wrapping_add(0xc), (w & 0xfffd) | 1);
    o.call(m, &SI_FCN_MANAGE_SLOPE, &[fcv, (idx as i32).wrapping_sub(1) as u32]);
}

/// [soft 0x082040ec fcurveBreakSlope]
///
/// Give key `idx` two slopes: bits 0 and 1 of the slope-kind byte are set.
pub fn fcurve_break_slope(m: &mut Mem, o: &mut dyn Outside, fcv: u32, idx: i16) {
    let k = key(m, fcv, idx);
    m.set_u8(k.wrapping_add(0xc), m.u8(k.wrapping_add(0xc)) | 3);
    o.call(m, &SI_FCN_MANAGE_SLOPE, &[fcv, (idx as i32).wrapping_sub(1) as u32]);
}

/// [soft 0x08204118 fcurveSystemSlope]
///
/// Let the program compute key `idx`'s slopes: the slope-kind word becomes 0.
pub fn fcurve_system_slope(m: &mut Mem, o: &mut dyn Outside, fcv: u32, idx: i16) {
    let k = key(m, fcv, idx);
    m.set_u16(k.wrapping_add(0xc), 0);
    o.call(m, &SI_FCN_MANAGE_SLOPE, &[fcv, (idx as i32).wrapping_sub(1) as u32]);
}

/// [soft 0x08204148 SI_fcurveSetSlopeVector]
///
/// Set the slope of key `idx` from a point `vec` (frame, value): (value difference) over
/// (frame difference), the key's time taken in frames; when the frames are equal the key's
/// frame is first moved 0.001 on (rounded to a float). The slope goes through `SI_secAsFrame`.
/// Without the two-sided bit it is stored on both sides; with it on the right side when `flag`
/// is 0, else on the left. The "slopes not computed" bit is then set.
pub fn fcurve_set_slope_vector(m: &mut Mem, o: &mut dyn Outside, fcv: u32, idx: i16, vec: u32, flag: i16) {
    let k = key(m, fcv, idx);
    let mut a = o.call_f(m, &crate::outside::SI_SEC_AS_FRAME, &[m.u32(k)]);
    let v = x(m.f32(k.wrapping_add(4)));
    let vx = x(m.f32(vec));
    if vx.eq(a) {
        a = x((a + x(0.001f64)).to_f32());
    }
    let slope = (x(m.f32(vec.wrapping_add(4))) - v) / (vx - a);
    let r = o.call_f(m, &crate::outside::SI_SEC_AS_FRAME, &[slope.to_f32().to_bits()]);
    let f = r.to_f32();
    if m.u8(k.wrapping_add(0xc)) & 2 == 0 {
        m.set_f32(k.wrapping_add(8), f);
        m.set_f32(k.wrapping_add(0x18), f);
    } else if flag == 0 {
        m.set_f32(k.wrapping_add(8), f);
    } else {
        m.set_f32(k.wrapping_add(0x18), f);
    }
    m.set_u8(k.wrapping_add(0xc), m.u8(k.wrapping_add(0xc)) | 1);
}

/// [soft 0x082072cc fcvSetKeySlopes2Zero]
///
/// Flatten key `idx` (0 to the count): slope-kind word 1, both slopes 0; the curve is
/// refreshed and marked. Null curve, no keys and a key out of range do nothing.
pub fn fcv_set_key_slopes_2_zero(m: &mut Mem, o: &mut dyn Outside, fcv: u32, idx: i32) {
    if fcv == 0 || m.u32(fcv.wrapping_add(8)) == 0 || idx < 0 || idx > m.i32(fcv.wrapping_add(0xc)) {
        return;
    }
    let k = m.u32(fcv.wrapping_add(8)).wrapping_add((idx as u32).wrapping_mul(0x24));
    m.set_u16(k.wrapping_sub(0x18), 1);
    m.set_u32(k.wrapping_sub(0x1c), 0);
    m.set_u32(k.wrapping_sub(0xc), 0);
    o.call(m, &crate::outside::SI_REFRESH_FCURVE, &[1]);
    mark_dirty(m, fcv);
}

/// [soft 0x08208200 si_checkKeyLock]
///
/// Is there a locked key (flags & 0xf == 1) other than `except` on the frame (rounded away
/// from zero by 0.5 and truncated) of `t`? Times are scaled by the frame rate in the 80-bit
/// register, the time passed in going through a division and a multiplication by it.
pub fn si_check_key_lock(m: &mut Mem, fcv: u32, except: u32, t: f32) -> u32 {
    let rate = x(m.f32(FRAME_RATE));
    let q = x(t) / rate;
    let p = q * rate;
    let half = |v: F80, pos: bool| if pos { v + x(0.5f64) } else { v + x(-0.5f64) };
    let target = half(p, F80::from(0i32).lt(q)).to_i32(Round::Zero);
    let n = m.i32(fcv.wrapping_add(0xc));
    for i in 0..n.max(0) as u32 {
        let k = m.u32(fcv.wrapping_add(8)).wrapping_add(i.wrapping_mul(0x24));
        if k == except {
            continue;
        }
        let tm = m.f32(k);
        let v = x(tm) * rate;
        if half(v, F80::from(0i32).lt(x(tm))).to_i32(Round::Zero) == target && m.u16(k.wrapping_add(0x1c)) & 0xf == 1 {
            return 1;
        }
    }
    0
}
