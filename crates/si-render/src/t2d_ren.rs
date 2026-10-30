// SPDX-License-Identifier: MIT
//! A 2D texture sample applied to a material: the original's `t2d_ren.c` (finding 0012,
//! section 13).

use crate::materials::BYTE_UNIT;
use crate::shade::ShadeState;
use si_x87::{x, F80};

/// 0x08655f08 (double): a sample's greatest value.
const FULL: f64 = 255.0;
/// 0x08655f10, 0x08655f18 (double): the two ends of the "black and white" key.
const KEY_LOW: f64 = 0.001;
const KEY_HIGH: f64 = 254.999;

/// Blends one sample of a 2D map into a material's working block
/// [soft 0x082b3670 SI_renderTxt2d]: `SI_renderTxt2d(material, map, sample)`; the sample is
/// five floats by value: red, green, blue, alpha, intensity, each 0 … 255.
///
/// - The blend B: the map's blend factor (+0x10c); with mask (+0x10a) 1 times the sample's
///   alpha, with mask 2 times its intensity: ((value × 0.003922) × blend), rounded to
///   float. R = 1 − B, rounded to float.
/// - Ambient (+0x112): each component of the working ambient (+0x710) becomes ((factor ×
///   sample) × B) + (R × component), the factor being the map's working ambient factor
///   (+0x148). Diffuse (+0x114; factor +0x14c; working diffuse +0x720) the same, **except
///   with mask 4: (factor × sample) × component**, no blend. Specular (+0x116; +0x150;
///   +0x730) as ambient.
/// - With h the sample's alpha when the map's channel word (+0x110) is 1, else its
///   intensity, and N = 255 − h rounded to float:
///   reflectivity (+0x118; factor f at +0x154; +0x740) = ((f × h) × B) + (R × old) for f at
///   or above 0, (((−f) × N) × B) + (R × old) below;
///   opacity (+0x11a; factor f at +0x158; +0x744) = (B × (1 − h × f)) + (R × old) for f at
///   or above 0, (B × ((f × N) + 1)) + (R × old) below.
/// - With the key (+0x18e) 1: the opacity becomes 0 when red, green and blue are all at or
///   below 0.001, or all at or above 254.999.
///
/// Every stored value is rounded once, from the register. Class: model.
pub fn render_txt2d(st: &mut ShadeState, material: usize, map: usize, sample: [f32; 5]) {
    let m = &st.maps[map];
    let w = &mut st.materials[material].work;
    let b: f32 = match m.mask {
        2 => ((x(sample[4]) * BYTE_UNIT) * m.blend).to_f32(),
        1 => ((x(sample[3]) * BYTE_UNIT) * m.blend).to_f32(),
        _ => m.blend,
    };
    let r = (F80::ONE - b).to_f32();
    let mix = |factor: f32, s: f32, old: f32| (((x(factor) * s) * b) + (x(r) * old)).to_f32();
    if m.fx_ambient != 0 {
        for c in 0..3 {
            w.ambient[c] = mix(m.work[0], sample[c], w.ambient[c]);
        }
    }
    if m.fx_diffuse != 0 {
        for c in 0..3 {
            w.diffuse[c] = if m.mask == 4 { ((x(m.work[1]) * sample[c]) * w.diffuse[c]).to_f32() } else { mix(m.work[1], sample[c], w.diffuse[c]) };
        }
    }
    if m.fx_specular != 0 {
        for c in 0..3 {
            w.specular[c] = mix(m.work[2], sample[c], w.specular[c]);
        }
    }
    let h = if m.bump_alpha == 1 { sample[3] } else { sample[4] };
    let n = (x(FULL) - h).to_f32();
    if m.fx_reflect != 0 {
        let f = m.work[3];
        let v = if x(f).lt(x(0.0f32)) || x(f).unordered(x(0.0f32)) { (-x(f)) * n } else { x(f) * h };
        w.reflectivity = ((v * b) + (x(r) * w.reflectivity)).to_f32();
    }
    if m.fx_transp != 0 {
        let f = m.work[4];
        let v = if x(f).lt(x(0.0f32)) || x(f).unordered(x(0.0f32)) { (x(f) * n) + F80::ONE } else { F80::ONE - (x(h) * f) };
        w.opacity = ((x(b) * v) + (x(r) * w.opacity)).to_f32();
    }
    if m.black_white == 1 {
        let rgb = [sample[0], sample[1], sample[2]];
        // 0.001 ≥ each (an unordered comparison goes on to the second test, which it fails)
        let low = rgb.iter().all(|&c| x(KEY_LOW).ge(x(c)));
        let high = rgb.iter().all(|&c| x(KEY_HIGH).le(x(c)));
        if low || high {
            w.opacity = 0.0;
        }
    }
}
