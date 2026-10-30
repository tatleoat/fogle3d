// SPDX-License-Identifier: MIT
//! A 3D texture made ready on the memory image (finding 0012, section 20): `SI_updTxt3dTurb`
//! of `t3d_fct.c` with the routines of `tt.c` and `tt_noise.c` it calls, each the second
//! form of a routine ported on this crate's records ([`crate::t3d_ren`], [`crate::tt`],
//! [`crate::tt_noise`]), here on the records and globals of the image, with the original's
//! allocation calls.
//!
//! The model and texture readers call `SI_updTxt3dTurb` for every 3D texture they make
//! (through `texture3DTransfer`); a loader that has it as a hook leaves the texture's
//! turbulence object (+0x8c) null, and the renderer then reads through it. Whoever embeds
//! the readers can answer the address 0x082b8d20 with [`upd_txt3d_turb`].

use crate::tt::TtLib;
use si_mem::Mem;
use si_x87::{fmov, x, Round, F80};

/// The globals of `tt_noise.c` and `drand48.c` (finding 0012, section 14).
pub mod noise {
    /// `table`: 65,536 floats.
    pub const TABLE: u32 = 0x087b_afc0;
    /// `first_call` (16 bits; 1 in the file).
    pub const FIRST_CALL: u32 = 0x086b_9a80;
    /// `seed`: three 16-bit words (0x330e, 0xabcd, 0x1234 in the file).
    pub const SEED: u32 = 0x086b_a000;
}

/// Offsets in `SI_Txt3d` (0x40a4 bytes) and `__SI_Texture3D` (0x158 bytes).
pub mod off {
    pub const TXT_SIZE: u32 = 0x40a4;
    pub const TXT_KIND: u32 = 0x00;
    pub const TXT_HAS_TFORM: u32 = 0x04;
    pub const TXT_TFORM: u32 = 0x08;
    pub const TXT_SCALE: u32 = 0x48;
    pub const TXT_TAPER: u32 = 0x4c;
    pub const TXT_STRENGTH: u32 = 0x50;
    pub const TXT_TERMS: u32 = 0x54;
    pub const TXT_TERM: u32 = 0x58;
    pub const TXT_RAMP: u32 = 0x98;
    pub const TXT_STOPS: u32 = 0x4098;
    pub const TEX_KIND: u32 = 0x0c;
    pub const TEX_COLOURS: u32 = 0x30;
    pub const TEX_STOPS: u32 = 0x80;
    pub const TEX_TURBULENCE: u32 = 0x8c;
    pub const TEX_SCALING: u32 = 0x90;
    pub const TEX_ROTATION: u32 = 0x9c;
    pub const TEX_TRANSLATION: u32 = 0xa8;
    pub const TEX_MATRIX: u32 = 0xb4;
}

/// 0x08657cd0, 0x08657ce0 (double): 2π.
const TWO_PI: f64 = core::f64::consts::TAU;
/// 0x08657cc8 (float), 0x08657cd8 and 0x08657d08 (double): 0.5.
const HALF: f64 = 0.5;
/// 0x08657ce8 (float), 0x08657cf8 (double): wood's first amplitude; 0x08657cec (float),
/// 0x08657cf0 (double): its second frequency.
const WOOD_AMP_F: f32 = 0.8;
const WOOD_AMP: f64 = 0.8;
const WOOD_FREQ: f32 = 8.0;
/// 0x08657d10 (float): the ramp's last index.
const RAMP_LAST: f32 = 1023.0;
/// 0x08656320 (double), 0x08656328 (double): a factor beyond these drives its channel.
const SOME: f64 = 1e-4;

/// Fills the noise table on the first call [soft 0x08317270 SI_ttnInitNoise]:
/// `SI_ttnInitNoise()` on the image: while `first_call` is not 0, `SI_srand48(1)` and
/// 65,536 values of `SI_drand48()`, each stored as a float, then `first_call` = 0.
/// Class: model (exact).
pub fn init_noise(m: &mut Mem) {
    if m.i16(noise::FIRST_CALL) == 0 {
        return;
    }
    // SI_srand48(1): word 0 = 0x330e, then the 32-bit seed over words 1 and 2
    let mut seed = crate::drand48::Seed([0, 0, 0]);
    crate::drand48::srand48(&mut seed, 1);
    m.set_u16(noise::SEED, seed.0[0]);
    m.set_i32(noise::SEED + 2, 1);
    let mut words = [m.u16(noise::SEED), m.u16(noise::SEED + 2), m.u16(noise::SEED + 4)];
    let mut table = Vec::with_capacity(0x40000);
    for _ in 0..0x10000 {
        let v = crate::drand48::erand48(&mut words);
        table.extend_from_slice(&v.to_f32().to_le_bytes());
    }
    for (k, w) in words.iter().enumerate() {
        m.set_u16(noise::SEED + 2 * k as u32, *w);
    }
    m.set_bytes(noise::TABLE, &table);
    m.set_u16(noise::FIRST_CALL, 0);
}

/// The terms of marble and cloud into the record at `t`: amplitude 0.5, frequency 1; each
/// further term twice the frequency and amplitude float(pow(2 × frequency, −power)) × 0.5;
/// then every amplitude divided by their sum (a float), read back from the record. More
/// than eight terms are written on into the ramp, as the original writes them.
fn octaves(m: &mut Mem, t: u32, n: i32, power: f32, keep_frequency_wide: bool, lib: &mut dyn TtLib) {
    let exponent = (-x(power)).to_f64();
    let mut amp = 0.5f32;
    let mut freq = F80::ONE;
    let mut total = 0.0f32;
    for i in 0..n.max(0) as u32 {
        let at = t.wrapping_add(off::TXT_TERM).wrapping_add(8 * i);
        m.set_f32(at, amp);
        m.set_f32(at.wrapping_add(4), freq.to_f32());
        total = (x(total) + amp).to_f32();
        let twice = freq + freq;
        freq = if keep_frequency_wide { twice } else { x(twice.to_f32()) };
        let p = lib.pow(twice.to_f64(), exponent).to_f32();
        amp = (x(p) * HALF).to_f32();
    }
    for i in 0..n.max(0) as u32 {
        let at = t.wrapping_add(off::TXT_TERM).wrapping_add(8 * i);
        let v = (x(m.f32(at)) / total).to_f32();
        m.set_f32(at, v);
    }
}

/// A marble on the image [soft 0x08318890 SI_ttMakeMarble]: `SI_ttMakeMarble(spacing,
/// taper, strength, terms, power)`: `SI_malloc(0x40a4)` (null: null), kind 0, +0x04 = 0,
/// scale, taper, strength, the term count and the terms ([`crate::tt::make_marble`] has
/// the arithmetic), `SI_ttnInitNoise()`. The rest of the block is what the allocator left.
/// Class: libm.
pub fn make_marble(m: &mut Mem, spacing: f32, taper: f32, strength: f32, terms: i32, power: f32, lib: &mut dyn TtLib) -> u32 {
    let t = m.si_malloc(off::TXT_SIZE);
    if t == 0 {
        return 0;
    }
    m.set_i32(t + off::TXT_KIND, 0);
    m.set_i16(t + off::TXT_HAS_TFORM, 0);
    m.set_f32(t + off::TXT_SCALE, (x(TWO_PI) * spacing).to_f32());
    let tan = lib.tanf(taper).to_f32();
    m.set_f32(t + off::TXT_TAPER, tan);
    m.set_f32(t + off::TXT_STRENGTH, strength);
    m.set_i32(t + off::TXT_TERMS, terms);
    octaves(m, t, terms, power, false, lib);
    init_noise(m);
    t
}

/// A wood on the image [soft 0x08318994 SI_ttMakeWood]: `SI_ttMakeWood(spacing, taper,
/// strength, power)`: as [`make_marble`], with two terms (frequencies 1 and 8, amplitudes
/// 0.8 and q = float(float(pow(8, −power)) × 0.8), both divided by (0.8 + q)).
/// Class: libm.
pub fn make_wood(m: &mut Mem, spacing: f32, taper: f32, strength: f32, power: f32, lib: &mut dyn TtLib) -> u32 {
    let t = m.si_malloc(off::TXT_SIZE);
    if t == 0 {
        return 0;
    }
    m.set_i32(t + off::TXT_KIND, 1);
    m.set_i16(t + off::TXT_HAS_TFORM, 0);
    m.set_f32(t + off::TXT_SCALE, (x(TWO_PI) * spacing).to_f32());
    let tan = lib.tanf(taper).to_f32();
    m.set_f32(t + off::TXT_TAPER, tan);
    m.set_f32(t + off::TXT_STRENGTH, strength);
    m.set_f32(t + off::TXT_TERM, WOOD_AMP_F);
    m.set_f32(t + off::TXT_TERM + 12, WOOD_FREQ);
    m.set_f32(t + off::TXT_TERM + 4, 1.0);
    m.set_i32(t + off::TXT_TERMS, 2);
    let p = lib.pow(WOOD_FREQ as f64, (-x(power)).to_f64()).to_f32();
    let q = x(p) * WOOD_AMP;
    let sum = x(m.f32(t + off::TXT_TERM)) + x(q.to_f32());
    let first = (x(m.f32(t + off::TXT_TERM)) / sum).to_f32();
    m.set_f32(t + off::TXT_TERM, first);
    m.set_f32(t + off::TXT_TERM + 8, (x(q.to_f32()) / sum).to_f32());
    init_noise(m);
    t
}

/// A cloud on the image [soft 0x08318a60 SI_ttMakeCloud]: `SI_ttMakeCloud(terms, power)`:
/// kind 2, +0x04 = 0, the term count and the terms with the frequency kept in the register;
/// scale, taper and strength are not written. Class: libm.
pub fn make_cloud(m: &mut Mem, terms: i32, power: f32, lib: &mut dyn TtLib) -> u32 {
    let t = m.si_malloc(off::TXT_SIZE);
    if t == 0 {
        return 0;
    }
    m.set_i32(t + off::TXT_KIND, 2);
    m.set_i16(t + off::TXT_HAS_TFORM, 0);
    m.set_i32(t + off::TXT_TERMS, terms);
    octaves(m, t, terms, power, true, lib);
    init_noise(m);
    t
}

/// The ramp on the image [soft 0x08318c8c SI_ttSetColorMap]: `SI_ttSetColorMap(texture,
/// colour 0 … colour 4, f1, f2, f3)`: the three stops trunc(f × 1023) stored at +0x4098 …
/// first; then for each of the four stretches between the positions 0, the stops and 1023
/// with n > 0 entries: the first colour copied as it is (four words), the running colour
/// (each component a float, the step (difference / n) kept in the register) at the next
/// n − 1, the second colour copied at the far end; a stretch of n < 1 moves the writing
/// position back by −n entries. A position outside the ramp is written where it falls (into
/// the terms before it, or behind the block, which the image records as a fault); a null
/// texture is written through. Class: model.
pub fn set_color_map(m: &mut Mem, t: u32, colours: &[[u32; 4]; 5], f: [f32; 3]) {
    let stop = |v: f32| (x(v) * RAMP_LAST).to_i32(Round::Zero);
    let stops = [stop(f[0]), stop(f[1]), stop(f[2])];
    for (k, s) in stops.iter().enumerate() {
        m.set_i32(t.wrapping_add(off::TXT_STOPS + 4 * k as u32), *s);
    }
    let at = [0, stops[0], stops[1], stops[2], 0x3ff];
    let mut pos = t.wrapping_add(off::TXT_RAMP);
    let put = |m: &mut Mem, pos: u32, c: &[u32; 4]| {
        for (k, w) in c.iter().enumerate() {
            m.set_u32(pos.wrapping_add(4 * k as u32), *w);
        }
    };
    for seg in 0..4 {
        let n = at[seg + 1].wrapping_sub(at[seg]);
        if n < 1 {
            pos = pos.wrapping_add((n as u32).wrapping_mul(16));
            continue;
        }
        let (from, to) = (colours[seg], colours[seg + 1]);
        let step = [0, 1, 2, 3].map(|c| (x(f32::from_bits(to[c])) - f32::from_bits(from[c])) / x(n));
        let mut cur = from;
        let mut k = at[seg];
        while k < at[seg + 1] {
            put(m, pos, &cur);
            for c in 0..4 {
                cur[c] = (x(f32::from_bits(cur[c])) + step[c]).to_f32().to_bits();
            }
            pos = pos.wrapping_add(16);
            k += 1;
        }
        put(m, pos, &to);
    }
}

/// The texture's matrix on the image [soft 0x08318b58 SI_ttSetTform]:
/// `SI_ttSetTform(texture, matrix)`: the first three columns of the four rows of the
/// sixteen floats at `matrix` loaded and stored at +0x08 … (row r column c at +0x08 + 16r
/// + 4c), and +0x04 = 0 when those twelve are exactly the identity's, else 1. Class: copy.
pub fn set_tform(m: &mut Mem, t: u32, matrix: u32) {
    let mut identity = true;
    for r in 0..4u32 {
        for c in 0..3u32 {
            let v = m.f32(matrix.wrapping_add(16 * r + 4 * c));
            m.set_f32(t.wrapping_add(off::TXT_TFORM + 16 * r + 4 * c), fmov(v));
            let want = if r == c { 1.0f32 } else { 0.0 };
            identity &= x(v).eq(x(want));
        }
    }
    m.set_i16(t.wrapping_add(off::TXT_HAS_TFORM), i16::from(!identity));
}

/// Makes a 3D texture of the image ready [soft 0x082b8d20 SI_updTxt3dTurb]:
/// `SI_updTxt3dTurb(texture)`.
///
/// 1. A turbulence object at +0x8c is given back (`free`) and the pointer cleared.
/// 2. By the kind (+0x0c): 1 `SI_ttMakeWood(+0x10, +0x14, +0x18, +0x20)`, 0
///    `SI_ttMakeMarble(+0x10, +0x14, +0x18, +0x1c, +0x20)`, 2 `SI_ttMakeCloud(+0x1c,
///    +0x20)`; the result at +0x8c. Any other kind leaves it null.
/// 3. `SI_ttSetColorMap(+0x8c, the five colours at +0x30, the three stops at +0x80)`.
/// 4. `SI_matrixXyzRotation(+0x9c, +0xb4)` (si-math's port on values; its sixteen floats
///    stored); the first three floats of rows 0, 1, 2 times the scaling +0x90, +0x94,
///    +0x98 (each product rounded to float); the translation (+0xa8, three words) copied
///    into row 3 (+0xe4).
/// 5. `SI_ttSetTform(+0x8c, +0xb4)`.
/// 6. The effect words: +0xf4, +0xf6, +0xf8: 1 when the factor at +0x108, +0x10c, +0x110 is
///    above 0.0001; +0xfc, +0xfa, +0x102: 1 when the factor at +0x118, +0x114, +0x120 is
///    below −0.0001 or above 0.0001.
///
/// Class: libm.
pub fn upd_txt3d_turb(m: &mut Mem, tex: u32, lib: &mut dyn TtLib) {
    let old = m.u32(tex.wrapping_add(off::TEX_TURBULENCE));
    if old != 0 {
        m.free(old);
        m.set_u32(tex.wrapping_add(off::TEX_TURBULENCE), 0);
    }
    let f = |m: &Mem, o: u32| m.f32(tex.wrapping_add(o));
    match m.i32(tex.wrapping_add(off::TEX_KIND)) {
        1 => {
            let t = make_wood(m, f(m, 0x10), f(m, 0x14), f(m, 0x18), f(m, 0x20), lib);
            m.set_u32(tex.wrapping_add(off::TEX_TURBULENCE), t);
        }
        0 => {
            let t = make_marble(m, f(m, 0x10), f(m, 0x14), f(m, 0x18), m.i32(tex.wrapping_add(0x1c)), f(m, 0x20), lib);
            m.set_u32(tex.wrapping_add(off::TEX_TURBULENCE), t);
        }
        2 => {
            let t = make_cloud(m, m.i32(tex.wrapping_add(0x1c)), f(m, 0x20), lib);
            m.set_u32(tex.wrapping_add(off::TEX_TURBULENCE), t);
        }
        _ => {}
    }
    let txt = m.u32(tex.wrapping_add(off::TEX_TURBULENCE));
    let colours: [[u32; 4]; 5] = core::array::from_fn(|i| core::array::from_fn(|k| m.u32(tex.wrapping_add(off::TEX_COLOURS + 16 * i as u32 + 4 * k as u32))));
    let stops = [f(m, off::TEX_STOPS), f(m, off::TEX_STOPS + 4), f(m, off::TEX_STOPS + 8)];
    set_color_map(m, txt, &colours, stops);
    let rot = si_math::types::Vector::new(f(m, off::TEX_ROTATION), f(m, off::TEX_ROTATION + 4), f(m, off::TEX_ROTATION + 8));
    let r = si_math::algebra::matrix_xyz_rotation(&rot);
    let mat = tex.wrapping_add(off::TEX_MATRIX);
    for (i, row) in r.iter().enumerate() {
        for (k, v) in row.iter().enumerate() {
            m.set_f32(mat + 16 * i as u32 + 4 * k as u32, *v);
        }
    }
    for i in 0..3u32 {
        let s = f(m, off::TEX_SCALING + 4 * i);
        for k in 0..3u32 {
            let at = mat + 16 * i + 4 * k;
            let v = (x(m.f32(at)) * s).to_f32();
            m.set_f32(at, v);
        }
    }
    for k in 0..3u32 {
        let w = m.u32(tex.wrapping_add(off::TEX_TRANSLATION + 4 * k));
        m.set_u32(mat + 0x30 + 4 * k, w);
    }
    set_tform(m, txt, mat);
    let above = |v: f32| i16::from(x(SOME).lt(x(v)));
    let beyond = |v: f32| i16::from(x(v).lt(x(-SOME)) || x(v).gt(x(SOME)));
    let fx = |m: &Mem, o: u32| m.f32(tex.wrapping_add(o));
    for (word, factor, both) in [(0xf4, 0x108, false), (0xf6, 0x10c, false), (0xf8, 0x110, false), (0xfc, 0x118, true), (0xfa, 0x114, true), (0x102, 0x120, true)] {
        let v = fx(m, factor);
        let on = if both { beyond(v) } else { above(v) };
        m.set_i16(tex.wrapping_add(word), on);
    }
}
