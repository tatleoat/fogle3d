// SPDX-License-Identifier: MIT
//! `preview.c` on the memory image: the colour lookup of the procedural previews and the
//! four previews that need no routine of another package that answers with a float (finding 0018,
//! section "Render set-up: the display side").
//!
//! `previewTxt (0x083182dc)` is ported on the real noise of `si-render` (render2 round): it
//! calls `SI_ttnGetNoise (0x083172bc)` itself and `sinf`. The window routines
//! (`SI_ttpInitWindow`, `SI_ttpDisplayTxt3d`) drive the display and stay hooks.
//!
//! **Compared with the emulated original**: `lookupColor`, `previewNull`, `previewCloud`,
//! `previewMarble` and `previewWood` were run against the original stepped through in the emulation harness
//! (`re/tools/si-rensetup/emu_gen_pass6.py`, `rensetup-emucheck`).

use super::globals::*;
use si_mem::Mem;
use si_render::tt_noise::{get_noise, Noise};
use si_x87::trig;
use si_x87::x87::{Round, F80};

/// Picks a colour from a table of 1024 entries of four floats (16 bytes each) by a number
/// from 0 to 1 and writes it as four bytes [soft 0x08318750 lookupColor].
///
/// With u = t × 1023 (formed in the register) and i = u cut towards zero to 32 bits: the
/// entry's weight is 1 − (u − i), held as a float; the next entry is i + 1, but 1023 stays
/// at 1023. Each of the four bytes is the 32-bit cut of (weight × entry i + (u − i) × next
/// entry) × 255, the low 8 bits kept, all but the weight in the register. A number outside
/// 0 to 1 reads outside the table, as in the original.
pub fn lookup_color(m: &mut Mem, table: u32, t: f32, out: u32) {
    let u = F80::from_f32(t) * F80::from_f32(1023.0);
    let i = u.to_i32(Round::Zero);
    let frac = u - F80::from_i32(i);
    let w = F80::from_f32((F80::from_f64(1.0) - frac).to_f32());
    let next = if i == 0x3ff { 0x3ff } else { i.wrapping_add(1) };
    let (a, b) = (table.wrapping_add((i as u32) << 4), table.wrapping_add((next as u32) << 4));
    let k255 = F80::from_f32(255.0);
    for c in 0..4u32 {
        let ea = F80::from_f32(m.f32(a.wrapping_add(4 * c)));
        let eb = F80::from_f32(m.f32(b.wrapping_add(4 * c)));
        let v = ((w * ea) + (frac * eb)) * k255;
        m.set_u8(out + c, v.to_i32(Round::Zero) as u8);
    }
}

/// The preview of a texture of no kind: four zero bytes [soft 0x083180dc previewNull]
/// (`out` is the fifth argument; the first four are not read).
pub fn preview_null(m: &mut Mem, out: u32) {
    for c in 0..4 {
        m.set_u8(out + c, 0);
    }
}

/// The cloud preview [soft 0x08318238 previewCloud]: `txt` is the texture record (+0x54 the
/// count n, +0x58 pairs of floats of which the first of each is a weight, +0x98 the colour
/// table), `y` and `x` select the stored noise of the pixel (`NOISE_TABLE` + y × 0xc80 + x ×
/// 32, floats). With p the sum, from 0, of weight(i) × noise(i) for i < n (signed, in the
/// register) and v = ((3 − p) − p) × (p × p) held as a float, the colour position is 1 when
/// v is above 1, 0 when 0 is above v, else v.
pub fn preview_cloud(m: &mut Mem, txt: u32, _pt: u32, x: i32, y: i32, out: u32) {
    let p = noise_sum(m, txt, x, y);
    let v = ((F80::from_f64(3.0) - p) - p) * (p * p);
    let vf = F80::from_f32(v.to_f32());
    let pos = if vf.gt(F80::from_f64(1.0)) {
        1.0f32
    } else if F80::ZERO.gt(vf) {
        0.0f32
    } else {
        vf.to_f32()
    };
    lookup_color(m, txt + 0x98, pos, out);
}

/// The marble preview [soft 0x083180f8 previewMarble]: p is formed as in the cloud preview;
/// with the point `pt` (three floats) and the record's +0x48, +0x4c, +0x50 (floats) the
/// angle is (((pt[1] × t4c) + pt[0]) + p × t50) × t48 as a float, and the colour position
/// is 0.5 − sinf(angle) × 0.5, held as a float.
pub fn preview_marble(m: &mut Mem, txt: u32, pt: u32, x: i32, y: i32, out: u32) {
    let p = noise_sum(m, txt, x, y);
    let f = |m: &Mem, a: u32| F80::from_f32(m.f32(a));
    let a = ((f(m, pt + 4) * f(m, txt + 0x4c)) + f(m, pt)) + p * f(m, txt + 0x50);
    let angle = (a * f(m, txt + 0x48)).to_f32();
    colour_position(m, txt, angle, out);
}

/// The wood preview [soft 0x08318194 previewWood]: the angle is
/// (((pt[0] × t4c) + sqrt(pt[2]² + pt[1]²)) + ((t60 × noise[3] + t58 × noise[0]) × t50)) × t48
/// as a float, where noise is the pixel's row of stored noise; the colour position as in
/// the marble preview.
pub fn preview_wood(m: &mut Mem, txt: u32, pt: u32, x: i32, y: i32, out: u32) {
    let base = noise_base(m, x, y);
    let f = |m: &Mem, a: u32| F80::from_f32(m.f32(a));
    let r = (f(m, pt + 8) * f(m, pt + 8) + f(m, pt + 4) * f(m, pt + 4)).sqrt();
    let n = (f(m, txt + 0x60) * f(m, base + 0xc) + f(m, txt + 0x58) * f(m, base)) * f(m, txt + 0x50);
    let a = ((f(m, pt) * f(m, txt + 0x4c)) + r) + n;
    let angle = (a * f(m, txt + 0x48)).to_f32();
    colour_position(m, txt, angle, out);
}

fn noise_base(m: &Mem, x: i32, y: i32) -> u32 {
    m.u32(NOISE_TABLE.0).wrapping_add((y as u32).wrapping_mul(0xc80)).wrapping_add((x as u32) << 5)
}

/// Sum, from 0, of the record's weights (floats at +0x58, eight bytes apart) times the
/// pixel's stored noise, for as many as the record's count (signed, +0x54).
fn noise_sum(m: &Mem, txt: u32, x: i32, y: i32) -> F80 {
    let n = m.i32(txt + 0x54);
    let base = noise_base(m, x, y);
    let mut p = F80::ZERO;
    let mut i = 0;
    while i < n {
        p = p + F80::from_f32(m.f32(txt + 0x58 + 8 * i as u32)) * F80::from_f32(m.f32(base.wrapping_add(4 * i as u32)));
        i += 1;
    }
    p
}

/// 0.5 − sinf(angle) × 0.5 held as a float, looked up in the record's colour table.
fn colour_position(m: &mut Mem, txt: u32, angle: f32, out: u32) {
    let half = F80::from_f64(0.5);
    let pos = (half - trig::sinf(angle) * half).to_f32();
    lookup_color(m, txt + 0x98, pos, out);
}

/// The preview of a 3D texture at one pixel, from the noise of its point [soft 0x083182dc previewTxt].
///
/// `noise` is the table `SI_ttnGetNoise (0x083172bc)` reads, which the caller built from the
/// image (the real callee runs, not an answer). The texture's kind (word at +0): 0 is a
/// turbulence, 1 a two-sample noise, 2 a cloud; any other prints "Unexpected default ..." on
/// the standard error (not reproduced: it prints no state) and leaves everything alone.
/// Every kind writes the pixel's stored noise (`NOISE_TABLE` + y x 0xc80 + x x 32) as floats.
///
/// Kind 0: for each of the n octaves (count at +0x54, frequency at +0x5c + 8i, weight at
/// +0x58 + 8i) the noise at the point scaled by the frequency (each product rounded to
/// float) is folded, 1 - u - u when u is not above 0.5, else 2u - 1, rounded to float and
/// stored as the octave's value; the sum, in the register, is of value x weight. The colour
/// position is 0.5 - sinf(((pt[1] x t4c + pt[0]) + t50 x sum) x t48) x 0.5. When fewer than
/// 8 octaves ran, the rest (up to 8) are noise at the point scaled by the last frequency
/// doubled, again and again, folded and stored (nothing else is done with them).
/// Kind 1: the noise at the point (rounded to float, stored at +0) and at the point times 8
/// (stored at +0xc as float; its register value, unrounded, is used) give the wood angle of
/// `preview_wood` with those two. Kind 2: as kind 0 without the fold, the sum taken of the
/// unrounded noise x weight; the colour position is the sum's s x s x ((3 - s) - s), rounded
/// to float and held to 0 to 1 (a NaN passes), and the rest of the 8 stored as noise (rounded).
pub fn preview_txt(m: &mut Mem, noise: &Noise, txt: u32, pt: u32, x: i32, y: i32, out: u32) {
    let f = |m: &Mem, a: u32| F80::from_f32(m.f32(a));
    let base = noise_base(m, x, y);
    let p = [m.f32(pt), m.f32(pt + 4), m.f32(pt + 8)];
    let at = |s: F80| get_noise(noise, (s * F80::from_f32(p[0])).to_f32(), (s * F80::from_f32(p[1])).to_f32(), (s * F80::from_f32(p[2])).to_f32());
    let half = F80::from_f64(0.5);
    let fold = |u: F80| if !u.gt(half) { (F80::ONE - u) - u } else { (u + u) - F80::ONE };
    let kind = m.i32(txt);
    match kind {
        0 | 2 => {
            let n = m.i32(txt + 0x54);
            let mut sum = F80::ZERO;
            let mut last = F80::ZERO;
            let mut i = 0i32;
            while i < n {
                last = f(m, txt + 0x5c + 8 * i as u32);
                let u = at(last);
                if kind == 0 {
                    let v = fold(u).to_f32();
                    m.set_f32(base.wrapping_add(4 * i as u32), v);
                    sum = sum + F80::from_f32(v) * f(m, txt + 0x58 + 8 * i as u32);
                } else {
                    m.set_f32(base.wrapping_add(4 * i as u32), u.to_f32());
                    sum = sum + u * f(m, txt + 0x58 + 8 * i as u32);
                }
                i += 1;
            }
            if kind == 0 {
                let a = ((f(m, pt + 4) * f(m, txt + 0x4c)) + f(m, pt)) + f(m, txt + 0x50) * sum;
                colour_position(m, txt, (a * f(m, txt + 0x48)).to_f32(), out);
            } else {
                let v = (sum * sum * ((F80::from_f64(3.0) - sum) - sum)).to_f32();
                let vf = F80::from_f32(v);
                let pos = if vf.gt(F80::ONE) {
                    1.0f32
                } else if F80::ZERO.gt(vf) {
                    0.0f32
                } else {
                    v
                };
                lookup_color(m, txt + 0x98, pos, out);
            }
            let mut j = i.max(0);
            while j <= 7 {
                last = last + last;
                let u = at(last);
                let v = if kind == 0 { fold(u).to_f32() } else { u.to_f32() };
                m.set_f32(base.wrapping_add(4 * j as u32), v);
                j += 1;
            }
        }
        1 => {
            let a = get_noise(noise, p[0], p[1], p[2]).to_f32();
            let b = at(F80::from_f64(8.0));
            m.set_f32(base, a);
            m.set_f32(base.wrapping_add(0xc), b.to_f32());
            let sum = b * f(m, txt + 0x60) + F80::from_f32(a) * f(m, txt + 0x58);
            let r = (F80::from_f32(p[2]) * F80::from_f32(p[2]) + F80::from_f32(p[1]) * F80::from_f32(p[1])).sqrt();
            let a = ((F80::from_f32(p[0]) * f(m, txt + 0x4c)) + r) + sum * f(m, txt + 0x50);
            colour_position(m, txt, (a * f(m, txt + 0x48)).to_f32(), out);
        }
        _ => {}
    }
}

/// Puts a point through the texture's matrix [soft 0x08318058 SI_ttpTformPt]: when the
/// record's 16-bit word at +4 is non-zero each coordinate k of `out` is the row k of the
/// 4 x 3 block of floats at +0x8 (its three entries a coordinate apart 0x10) times the point, summed in the order
/// x, y, z and then the translation at +0x38 (x), +0x3c (y), +0x40 (z); otherwise the point
/// is copied (as floats through the register).
pub fn ttp_tform_pt(m: &mut Mem, txt: u32, pt: u32, out: u32) {
    let f = |m: &Mem, a: u32| F80::from_f32(m.f32(a));
    if m.i16(txt + 4) != 0 {
        for k in 0..3u32 {
            let a = txt + 8 + 4 * k;
            let v = ((f(m, a) * f(m, pt) + f(m, a + 0x10) * f(m, pt + 4)) + f(m, a + 0x20) * f(m, pt + 8)) + f(m, txt + 0x38 + 4 * k);
            m.set_f32(out + 4 * k, v.to_f32());
        }
    } else {
        for k in 0..3 {
            let v = f(m, pt + 4 * k).to_f32();
            m.set_f32(out + 4 * k, v);
        }
    }
}

/// Chooses the function that previews a pixel [soft 0x08317fb4 SI_ttpInitPixelSphTxt3d]:
/// the word `SI_ttpPreviewSphFct` becomes `previewNull` when there is no noise table,
/// else `previewTxt` when `texture_image` (the short argument) is non-zero, else by the
/// record's kind (word at +0): 0 `previewMarble`, 1 `previewWood`, 2 `previewCloud`. Any
/// other kind leaves the word alone and the original prints "Unexpected default ..." on the
/// standard error (not reproduced: it prints no state).
pub fn ttp_init_pixel_sph_txt3d(m: &mut Mem, txt: u32, texture_image: i16) {
    let f = if m.u32(NOISE_TABLE.0) == 0 {
        PREVIEW_NULL
    } else if texture_image != 0 {
        PREVIEW_TXT
    } else {
        match m.i32(txt) {
            0 => PREVIEW_MARBLE,
            1 => PREVIEW_WOOD,
            2 => PREVIEW_CLOUD,
            _ => return,
        }
    };
    m.set_u32(SI_TTP_PREVIEW_SPH_FCT.0, f);
}

pub const PREVIEW_NULL: u32 = 0x083180dc;
pub const PREVIEW_MARBLE: u32 = 0x083180f8;
pub const PREVIEW_WOOD: u32 = 0x08318194;
pub const PREVIEW_CLOUD: u32 = 0x08318238;
pub const PREVIEW_TXT: u32 = 0x083182dc;
