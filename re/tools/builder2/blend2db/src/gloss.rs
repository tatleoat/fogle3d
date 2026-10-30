// SPDX-License-Identifier: MIT
//! Principled BSDF -> the Softimage material's specular colour and exponent, reflectivity and the diffuse/ambient
//! colour of metals (unit gloss1). Our own code: nothing here reproduces a function of the original.
//!
//! What the program's shading does with a material (finding 0012, section 6, `SI_shade`; Phong, shading model 4):
//!   local  = ambience x ambient + sum over lights (diffuse x light colour x N.L) + sum (specular x light colour x f(N.H))
//!   colour = reflectivity x (mirror ray) + (1 - reflectivity) x local,   clamped to 0..1 at the end,
//! all in display values, with f the material's decay table: (N.H)^exponent sampled at 200 cosines and interpolated
//! linearly, the last segment rising to 1 (so a highlight is never narrower than N.H >= 0.995, about 4 degrees half
//! width). The mirror ray is not tinted, the reflectivity is one number, there is no Fresnel term and no glossy blur:
//! these are the limits the mapping works in. Approximations:
//! - Highlight size: Cycles shows the image of the light through the GGX lobe (alpha = roughness^2). The lobe is taken
//!   as a Blinn exponent n_r = N_SCALE x (2 / alpha^2 - 2), the light's disc (angular radius t seen from the object) as
//!   an exponent C_SIZE / t^2, the two combined like variances, at most N_MAX. The light is the one that lights the
//!   material's objects most (export.py key_spreads, by Cycles' fall-off formulas as fit.rs). Point and spot lights of
//!   radius 0 and suns of angle 0 add nothing. One exponent per material: every light draws a highlight of that size.
//! - Highlight brightness: the decay table's lobe is normalised to the energy of a normalised Blinn-Phong lobe, times
//!   the Fresnel reflectance at normal incidence (dielectrics: from the IOR and Specular IOR Level, times Specular Tint;
//!   metals: the display base colour), times K_SPEC (calibrated: display values add a highlight more strongly than
//!   linear ones).
//! - Metal tint: the mirror ray cannot be tinted, so the reflectivity is the smallest display channel of the base colour
//!   (times a mirror weight w, 1 at roughness MIRROR_LO and below, 0 at MIRROR_HI and above: a rough metal is nearer to
//!   a coloured diffuse surface) and the colour above it is put into the local part: diffuse and ambient colour
//!   w x (t - min) x C_TINT + (1 - w) x t x C_ROUGH; the specular colour is divided by (1 - reflectivity) so the
//!   highlight survives the mirror mix (the program does not clamp it before).
//! - Dielectric gloss: a reflectivity of R_DIEL x w, standing for the Fresnel reflection the program does not have (the
//!   same at every angle, so too strong face on and too weak at grazing angles).
//! - Coat: a second, white highlight of the coat's roughness, added to the base's in energy; the exponent is the
//!   energy-weighted mean of the two in log space. The coat adds no reflectivity (it made the calibration worse).
//! - Clear glass (`glass`): the program shows no highlight and no mirror ray on a surface it sees through whole
//!   (transparency 1), so the glass keeps an opacity GLASS_O x transmission; that share is black with the glass's
//!   highlight, and GLASS_R of the colour would be the mirror ray. GLASS_R is 0: a mirror ray on glass makes every hit
//!   inside it branch in two (the still life rendered 2.3 times slower for 0.004 of the glass ball's rmse). The glass's
//!   shadow lets 0.8 of the light through (it let all through).
use serde_json::Value;

/// Calibration (re/tools/gloss1, calibration scenes against Cycles; the numbers in the unit's report). The add-on's
/// material_nodes.GLOSS and gloss_values mirror this file operation for operation (Softimage nodes from Principled).
#[derive(Clone, Copy, Debug)]
pub struct Tune {
    pub k_spec: f64,
    pub c_size: f64,
    pub r_diel: f64,
    pub mirror_lo: f64,
    pub mirror_hi: f64,
    pub c_tint: f64,
    pub c_rough: f64,
    pub n_max: f64,
    /// glass: its opacity (0: glass as before, pure refraction) and the share of the mirror ray
    pub glass_o: f64,
    pub glass_r: f64,
    /// the roughness exponent's scale (n_r = n_scale x (2 / alpha^2 - 2))
    pub n_scale: f64,
}

pub const TUNE: Tune = Tune { k_spec: 0.6, c_size: 24.0, r_diel: 0.12, mirror_lo: 0.1, mirror_hi: 0.8, c_tint: 0.45, c_rough: 0.5, n_max: 1000.0,
    glass_o: 0.1, glass_r: 0.0, n_scale: 0.35 };

/// The calibration in use: TUNE. A build with the `gloss-tune` feature (re/tools/gloss1/sweep.sh, development only) reads
/// overrides from GLOSS1_TUNE="k_spec=1.2,c_size=10".
#[cfg(not(feature = "gloss-tune"))]
pub fn tune() -> Tune {
    TUNE
}

#[cfg(feature = "gloss-tune")]
pub fn tune() -> Tune {
    let mut t = TUNE;
    if let Ok(s) = std::env::var("GLOSS1_TUNE") {
        for kv in s.split(',') {
            let mut it = kv.splitn(2, '=');
            let (Some(k), Some(v)) = (it.next(), it.next().and_then(|v| v.trim().parse::<f64>().ok())) else { continue };
            match k.trim() {
                "k_spec" => t.k_spec = v,
                "c_size" => t.c_size = v,
                "r_diel" => t.r_diel = v,
                "mirror_lo" => t.mirror_lo = v,
                "mirror_hi" => t.mirror_hi = v,
                "c_tint" => t.c_tint = v,
                "c_rough" => t.c_rough = v,
                "n_max" => t.n_max = v,
                "glass_o" => t.glass_o = v,
                "glass_r" => t.glass_r = v,
                "n_scale" => t.n_scale = v,
                _ => {}
            }
        }
    }
    t
}

fn f(v: &Value, d: f64) -> f64 {
    v.as_f64().unwrap_or(d)
}

/// The decay table's lobe integrated over the hemisphere of half vectors (2 pi times the mean over the cosine):
/// 200 values (i / 200)^n, linear between them, the last segment rising by 1.
fn lobe_energy(n: f64) -> f64 {
    let v = |i: usize| (i as f64 / 200.0).powf(n);
    let mut s = 0.0;
    for i in 0..200 {
        let step = if i == 199 { 1.0 } else { v(i + 1) - v(i) };
        s += v(i) + step / 2.0;
    }
    2.0 * std::f64::consts::PI * s / 200.0
}

/// A normalised Blinn-Phong lobe's peak (n + 1) / 8 per unit Fresnel, for the table's lobe of exponent n.
fn peak(n: f64) -> f64 {
    (std::f64::consts::PI / 4.0) / lobe_energy(n)
}

fn exponent_of(rough: f64, spread: f64, t: &Tune) -> f64 {
    let r = rough.clamp(0.02, 1.0);
    let a = r * r;
    let n_r = (t.n_scale * (2.0 / (a * a) - 2.0)).max(1.0);
    let inv = 1.0 / n_r + if spread > 0.0 { spread * spread / t.c_size } else { 0.0 };
    (1.0 / inv).clamp(1.0, t.n_max)
}

pub fn srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// What the mapping writes into the material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gloss {
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub exponent: f32,
    pub reflectivity: f32,
}

fn f3(v: &Value, d: [f64; 3]) -> [f64; 3] {
    match v.as_array() {
        Some(a) => [0, 1, 2].map(|i| a.get(i).and_then(Value::as_f64).unwrap_or(d[i])),
        None => d,
    }
}

/// An opaque Principled material (export.py's "diffuse", "roughness", "metallic", "specular_level" (else the old
/// "specular"), "specular_tint", "ior", "coat", "coat_roughness", "coat_ior"), lit mostly by a light of angular radius
/// "key_spread" (export.key_spreads; 0 when absent).
pub fn map(m: &Value, t: &Tune) -> Gloss {
    let spread = f(&m["key_spread"], 0.0).max(0.0);
    let base = f3(&m["diffuse"], [0.8; 3]);
    let tb = base.map(srgb);
    let rough = f(&m["roughness"], 0.5).clamp(0.0, 1.0);
    let metal = f(&m["metallic"], 0.0).clamp(0.0, 1.0);
    let level = m["specular_level"].as_f64().or(m["specular"].as_f64()).unwrap_or(0.5).clamp(0.0, 1.0);
    let tint = f3(&m["specular_tint"], [1.0; 3]).map(|v| v.clamp(0.0, 1.0));
    let ior = f(&m["ior"], 1.5).max(1.0);
    let q = (ior - 1.0) / (ior + 1.0);
    let f0 = (q * q * 2.0 * level).clamp(0.0, 1.0);
    let n = exponent_of(rough, spread, t);
    let pk = peak(n) * t.k_spec;
    // the mirror weight: 1 for a polished surface, 0 for a rough one
    let w = ((t.mirror_hi - rough) / (t.mirror_hi - t.mirror_lo).max(1e-6)).clamp(0.0, 1.0);
    // dielectric: diffuse as it was, white (tinted) highlight, optional constant gloss
    let r_d = (t.r_diel * w).clamp(0.0, 0.99);
    let spec_d = tint.map(|k| f0 * k * pk);
    // metal: the smallest display channel is the mirror, the colour above it goes to the local part
    let tmin = tb.iter().cloned().fold(1.0, f64::min);
    let r_m = (w * tmin).clamp(0.0, 0.99);
    let loc_m = [0, 1, 2].map(|k| w * (tb[k] - tmin) * t.c_tint + (1.0 - w) * tb[k] * t.c_rough);
    let spec_m = tb.map(|k| k * pk);
    // coat: a white highlight of its own roughness on top
    let coat = f(&m["coat"], 0.0).clamp(0.0, 1.0);
    let (mut n_out, mut spec_c) = (n, 0.0);
    if coat > 0.0 {
        let cior = f(&m["coat_ior"], 1.5).max(1.0);
        let cq = (cior - 1.0) / (cior + 1.0);
        let cf0 = cq * cq;
        let nc = exponent_of(f(&m["coat_roughness"], 0.03), spread, t);
        let eb = metal * (tb[0] + tb[1] + tb[2]) / 3.0 + (1.0 - metal) * f0;
        let ec = coat * cf0;
        n_out = ((eb * n.ln() + ec * nc.ln()) / (eb + ec).max(1e-12)).exp();
        spec_c = coat * cf0 * peak(nc) * t.k_spec;
    }
    let r = metal * r_m + (1.0 - metal) * r_d;
    let keep = (1.0 - r).max(1e-3);
    let diffuse = [0, 1, 2].map(|k| ((metal * loc_m[k] + (1.0 - metal) * (1.0 - r_d) * tb[k]) / keep) as f32);
    let specular = [0, 1, 2].map(|k| ((metal * spec_m[k] + (1.0 - metal) * (1.0 - r_d) * spec_d[k] + spec_c) / keep) as f32);
    Gloss { diffuse, specular, exponent: n_out as f32, reflectivity: r as f32 }
}

/// A clear Principled glass (transmission, alpha 1): the program shows no highlight and no mirror ray on a surface it
/// sees through whole, so the glass keeps an opacity `glass_o` x transmission that carries them: (diffuse, specular,
/// exponent, reflectivity, transparency). None with `glass_o` 0.
pub fn glass(m: &Value, t: &Tune) -> Option<(Gloss, f32)> {
    if t.glass_o <= 0.0 {
        return None;
    }
    let spread = f(&m["key_spread"], 0.0).max(0.0);
    let trans = f(&m["transmission"], 0.0).clamp(0.0, 1.0);
    let o = (t.glass_o * trans + (1.0 - trans)).clamp(1e-3, 1.0);
    let level = m["specular_level"].as_f64().or(m["specular"].as_f64()).unwrap_or(0.5).clamp(0.0, 1.0);
    let tint = f3(&m["specular_tint"], [1.0; 3]).map(|v| v.clamp(0.0, 1.0));
    let ior = f(&m["ior"], 1.5).max(1.0);
    let q = (ior - 1.0) / (ior + 1.0);
    let f0 = (q * q * 2.0 * level).clamp(0.0, 1.0);
    let n = exponent_of(f(&m["roughness"], 0.0).clamp(0.0, 1.0), spread, t);
    let r = (t.glass_r / o).clamp(0.0, 0.95);
    let keep = o * (1.0 - r);
    let specular = tint.map(|k| (f0 * k * peak(n) * t.k_spec / keep) as f32);
    Some((Gloss { diffuse: [0.0; 3], specular, exponent: n as f32, reflectivity: r as f32 }, (1.0 - o) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lobe_is_normalised_where_the_table_resolves_it() {
        // (n + 1) / 8 for a smooth lobe; capped by the table's last segment for sharp ones
        for (n, tol) in [(1.0, 0.02), (5.0, 0.02), (20.0, 0.06), (50.0, 0.12)] {
            assert!((peak(n) / ((n + 1.0) / 8.0) - 1.0).abs() < tol, "{n} {}", peak(n));
        }
        assert!(peak(1000.0) < 60.0 && peak(1000.0) > 40.0);
    }

    #[test]
    fn dielectric_keeps_its_colour_and_metal_carries_its_tint() {
        let t = TUNE;
        // a rough dielectric keeps its colour; a smooth one adds the gloss reflectivity and a white highlight
        let d = map(&json!({"diffuse": [0.1, 0.3, 0.85, 1.0], "roughness": 0.9, "metallic": 0.0, "specular_level": 0.5, "ior": 1.5, "key_spread": 0.25}), &t);
        assert_eq!(d.diffuse, [srgb(0.1) as f32, srgb(0.3) as f32, srgb(0.85) as f32]);
        assert_eq!(d.reflectivity, 0.0);
        let d = map(&json!({"diffuse": [0.1, 0.3, 0.85, 1.0], "roughness": 0.05, "metallic": 0.0, "specular_level": 0.5, "ior": 1.5, "key_spread": 0.25}), &t);
        assert!((d.reflectivity - t.r_diel as f32).abs() < 1e-6 && (d.diffuse[2] - srgb(0.85) as f32).abs() < 1e-6);
        assert!(d.specular[0] == d.specular[2] && d.specular[0] > 0.3);
        let g = map(&json!({"diffuse": [1.0, 0.766, 0.336, 1.0], "roughness": 0.05, "metallic": 1.0, "ior": 1.5, "key_spread": 0.25}), &t);
        assert!(g.reflectivity > 0.5 && g.diffuse[0] > g.diffuse[1] && g.diffuse[1] > g.diffuse[2] && g.diffuse[2] == 0.0);
        assert!(g.specular[0] > g.specular[2]);
        // a rough metal: no mirror, the base colour as a diffuse colour
        let r = map(&json!({"diffuse": [1.0, 0.766, 0.336, 1.0], "roughness": 0.8, "metallic": 1.0, "key_spread": 0.25}), &t);
        assert_eq!(r.reflectivity, 0.0);
        assert!((r.diffuse[1] - srgb(0.766) as f32 * t.c_rough as f32).abs() < 1e-6);
    }

    #[test]
    fn clear_glass_keeps_a_share_for_highlight_and_mirror() {
        let t = TUNE;
        let (g, tr) = glass(&json!({"diffuse": [1.0, 1.0, 1.0, 1.0], "roughness": 0.0, "transmission": 1.0, "ior": 1.45, "key_spread": 0.25}), &t).unwrap();
        assert!((tr - (1.0 - t.glass_o) as f32).abs() < 1e-6);
        assert!((g.reflectivity as f64 * t.glass_o - t.glass_r).abs() < 1e-6);
        assert!(g.diffuse == [0.0; 3] && g.specular[0] > 0.0 && g.specular[0] == g.specular[2]);
    }
}
