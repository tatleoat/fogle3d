// SPDX-License-Identifier: MIT
//! Two routines of `t2d_fct.c` on the memory image (finding 0012, section 20): the default
//! mapping method and the transformation of a texture coordinate pair by a texture's
//! scaling and offsets. The file's other ports are [`crate::t2d_fct`], on values.

use crate::data::text;
use crate::img::{f, Outside};
use si_mem::Mem;
use si_x87::{x, F80};

/// `defaultMapMethod` (0x086a4d40, 1 in the file) and the method word of `SI_defaultMap`
/// (0x0869ebc0 + 0x2a).
pub const DEFAULT_MAP_METHOD: u32 = 0x086a_4d40;
pub const DEFAULT_MAP_METHOD_WORD: u32 = 0x0869_ebea;
/// 0x08655ee8 (float): what stands for a scaling of 0.
const TINY: f32 = 1e-5;
/// 0x08655ef0 (double), 0x08655ef8 (float).
const HALF: f64 = 0.5;
const TWO: f32 = 2.0;

/// [soft 0x082b24f0 SI_t2dSetDefaultMethod]: `SI_t2dSetDefaultMethod(method)`: the 16-bit
/// method is stored in `defaultMapMethod` and in `SI_defaultMap` +0x2a. Class: copy.
pub fn set_default_method(m: &mut Mem, method: i16) {
    m.set_i16(DEFAULT_MAP_METHOD, method);
    m.set_i16(DEFAULT_MAP_METHOD_WORD, method);
}

/// A texture coordinate pair through a texture's scaling and offsets
/// [soft 0x082b33fc SI_txtApplyTransform]: `SI_txtApplyTransform(texture, &pair)`.
///
/// su = the u scaling (+0x58), sv = the v scaling (+0x5c), each replaced by the float
/// 0.00001 when it is 0 (a NaN is kept). u = float((u − u offset (+0x68)) / su); v =
/// float(((v offset (+0x6c) + (−v)) + sv) / sv); all in the register. For methods 5 and 6
/// (+0x2a), unless the environment has `SI_OLD_UV_CONVERSION` (`getenv`), and when su is
/// above 0.5: u = float((1 / ((1 − su) + 2)) × u). With the exchange word (+0xfc) set: v
/// becomes float(1 − u) and u float(1 − v), both of the values before. Class: model.
pub fn txt_apply_transform(m: &mut Mem, env: &mut dyn Outside, map: u32, pair: u32) {
    let du = m.f32(map + 0x58);
    let du = if du == 0.0 { x(TINY) } else { x(du) };
    let dv = m.f32(map + 0x5c);
    let dv = if dv == 0.0 { x(TINY) } else { x(dv) };
    let (a, b) = (x(m.f32(map + 0x68)), x(m.f32(map + 0x6c)));
    let u = ((x(m.f32(pair)) - a) / du).to_f32();
    m.set_f32(pair, u);
    let v = (((b + (-x(m.f32(pair + 4)))) + dv) / dv).to_f32();
    m.set_f32(pair + 4, v);
    if m.u16(map + 0x2a).wrapping_sub(5) <= 1 && env.call(m, f::GETENV, &[text::OLD_UV_CONVERSION]) == 0 && x(m.f32(map + 0x58)).gt(x(HALF)) {
        let u = ((F80::ONE / ((F80::ONE - du) + TWO)) * m.f32(pair)).to_f32();
        m.set_f32(pair, u);
    }
    if m.i16(map + 0xfc) != 0 {
        let (u, v) = (m.f32(pair), m.f32(pair + 4));
        m.set_f32(pair + 4, (F80::ONE - x(u)).to_f32());
        m.set_f32(pair, (F80::ONE - x(v)).to_f32());
    }
}
