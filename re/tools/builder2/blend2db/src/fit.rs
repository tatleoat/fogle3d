// SPDX-License-Identifier: MIT
//! Light and ambient calibration (unit builder2, our own code; nothing here reproduces a function of the original).
//!
//! Cycles (Blender 5.2, measured by re/tools/builder2/tests, mode "cycles") lights a white diffuse surface, normal incidence, with
//!   point and spot of P watts at distance d:   L = P / (4 pi^2 d^2)
//!   area of P watts (cosine lobe, small):      L = P cos(phi) / (pi^2 d^2)
//!   sun of strength S:                         L = S / pi
//!   world colour W (uniform):                  L = a W              (a the albedo)
//! all linear, times cos(theta) of the incidence, times the albedo / 1 for a white plane, per colour channel.
//! The Softimage renderer shades in display values: colour = ambient * material ambient + diffuse * light colour * cos(theta) * f(d)
//! with f a linear fall-off (1 up to `start`, then to 0 at `end`), spot factor linear in the cosine between the inner and the outer cone.
//! The picture is read as sRGB, so the target is the sRGB encoding of Cycles' linear result. We fit, per light, a light colour and a
//! fall-off end (start 0) by least squares over the scene's own triangles that the camera sees and the light reaches:
//!     sRGB(a E cos m) / sRGB(a)  ~  cos m_si (alpha + beta d),  beta = -alpha / end
//! (display values multiply almost like powers, sRGB(a b) ~ sRGB(a) sRGB(b), so dividing the albedo's encoding out is a fair model).
//! The ambience likewise: sRGB(a W) ~ sRGB(a) A.
pub struct Sample {
    pub p: [f64; 3],
    pub n: [f64; 3],
    pub w: f64,
    pub a: [f64; 3],
}

pub fn srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Point,
    Spot { cos_outer: f64, cos_inner: f64 },
    Sun,
    /// An area light: a Softimage spot whose cone is 0 and spread pi (factor = cos(angle from the axis)).
    Area,
}

pub struct Fit {
    pub colour: [f64; 3],
    pub start: f64,
    pub end: f64,
    pub samples_used: usize,
}

/// Fit one light. `energy` is Blender's (W; sun: strength), `col` its linear colour, `pos` and `fwd` (unit) in Softimage's world.
pub fn fit_light(kind: Kind, col: [f64; 3], energy: f64, pos: [f64; 3], fwd: [f64; 3], samples: &[Sample], cam: [f64; 3], world: [f64; 3], amb: [f64; 3]) -> Fit {
    let pi = std::f64::consts::PI;
    let k = match kind {
        Kind::Area => energy / (pi * pi),
        Kind::Sun => energy / pi,
        _ => energy / (4.0 * pi * pi),
    };
    // rows: (weight, distance, m_si, m_cycles * cos, cos * m_si, albedo) per usable sample
    struct Row {
        w: f64,
        d: f64,
        msi: f64,
        e: f64, // Cycles' linear irradiance factor without colour: k (/d^2) cos m_cy
        a: [f64; 3],
    }
    let mut rows: Vec<Row> = Vec::new();
    for s in samples {
        let to_cam = sub(cam, s.p);
        if dot(s.n, to_cam) <= 0.0 {
            continue;
        }
        let (l, d) = match kind {
            Kind::Sun => ([-fwd[0], -fwd[1], -fwd[2]], 1.0),
            _ => {
                let v = sub(pos, s.p);
                let d = dot(v, v).sqrt();
                if d < 1e-6 {
                    continue;
                }
                ([v[0] / d, v[1] / d, v[2] / d], d)
            }
        };
        let cos = dot(s.n, l);
        if cos <= 0.01 {
            continue;
        }
        let c = dot(fwd, [-l[0], -l[1], -l[2]]); // angle from the light's axis (not for the sun)
        let (msi, mcy) = match kind {
            Kind::Point | Kind::Sun => (1.0, 1.0),
            Kind::Area => {
                if c <= 0.0 {
                    continue;
                }
                (c, c)
            }
            Kind::Spot { cos_outer, cos_inner } => {
                if c <= cos_outer {
                    continue;
                }
                let t = if cos_inner > cos_outer { (c - cos_outer) / (cos_inner - cos_outer) } else { 1.0 };
                (t.clamp(0.0, 1.0), smooth(t))
            }
        };
        let e = match kind {
            Kind::Sun => k * cos * mcy,
            _ => k / (d * d) * cos * mcy,
        };
        rows.push(Row { w: s.w, d, msi: cos * msi, e, a: s.a });
    }
    let n = rows.len();
    if n == 0 {
        // nothing the light reaches that the camera sees: a plain guess (white plane at 5 units)
        let e = if kind == Kind::Sun { k } else { k / 25.0 };
        return Fit { colour: [srgb(col[0] * e), srgb(col[1] * e), srgb(col[2] * e)], start: 10000.0, end: 10000.0, samples_used: 0 };
    }
    let target = |r: &Row, ch: usize| -> f64 {
        let a = r.a[ch].clamp(0.0, 1.0);
        // what the light adds to the ambience (Cycles: albedo (W + E); ours: diffuse (ambience + light))
        (srgb(a * (world[ch] + col[ch] * r.e)) / srgb(a).max(0.05) - amb[ch]).max(0.0)
    };
    let refch = if col[0] >= col[1] && col[0] >= col[2] { 0 } else if col[1] >= col[2] { 1 } else { 2 };
    let dmax = rows.iter().map(|r| r.d).fold(0.0, f64::max);
    let (mut end, mut start) = (10000.0, 10000.0);
    if kind != Kind::Sun {
        // 2x2 normal equations in (alpha, beta)
        let (mut s11, mut s12, mut s22, mut b1, mut b2) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for r in &rows {
            let t = target(r, refch);
            let m2 = r.w * r.msi * r.msi;
            s11 += m2;
            s12 += m2 * r.d;
            s22 += m2 * r.d * r.d;
            b1 += r.w * r.msi * t;
            b2 += r.w * r.msi * r.d * t;
        }
        let det = s11 * s22 - s12 * s12;
        if det.abs() > 1e-12 {
            let alpha = (b1 * s22 - b2 * s12) / det;
            let beta = (s11 * b2 - s12 * b1) / det;
            if beta < -1e-9 && alpha > 0.0 {
                end = (-alpha / beta).max(1.3 * dmax);
                start = 0.0;
            }
        }
    }
    let f = |r: &Row| -> f64 { r.msi * if start < end { 1.0 - r.d / end } else { 1.0 } };
    let mut colour = [0.0; 3];
    for ch in 0..3 {
        let (mut num, mut den) = (0.0, 0.0);
        for r in &rows {
            num += r.w * f(r) * target(r, ch);
            den += r.w * f(r) * f(r);
        }
        colour[ch] = if den > 0.0 { (num / den).max(0.0) } else { 0.0 };
    }
    Fit { colour, start, end, samples_used: n }
}

/// The ambience: per channel A with sRGB(a W) ~ sRGB(a) A over the camera-facing triangles.
pub fn fit_ambient(world: [f64; 3], samples: &[Sample], cam: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for ch in 0..3 {
        let (mut num, mut den) = (0.0, 0.0);
        for s in samples {
            if dot(s.n, sub(cam, s.p)) <= 0.0 {
                continue;
            }
            let a = s.a[ch].clamp(0.0, 1.0);
            let d = srgb(a);
            num += s.w * d * srgb(a * world[ch]);
            den += s.w * d * d;
        }
        out[ch] = if den > 0.0 { num / den } else { srgb(world[ch]) };
    }
    out
}
