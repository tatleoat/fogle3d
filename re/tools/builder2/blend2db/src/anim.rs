// SPDX-License-Identifier: MIT
//! Animation files from sampled frames (unit builder2; our own code, only the file format is the original's: finding 0005's
//! animation files, written through si-elemio's `write_ani_file`).
//!
//! Blender is sampled at every frame (export.sample_range); each moving quantity becomes one function curve with one key
//! per frame (linear segments, time = frame / rate in seconds). Curve types are the attribute ids of `Relations.rsrc`:
//! model scaling 1 to 3, rotation 4 to 6 (degrees), explicit translation 139 to 141; camera position 142 to 144, interest
//! 145 to 147; light colour 22 to 24, position 148 to 150, interest 151 to 153; material ambient 45 to 47, diffuse 48 to 50.
use si_elemio::anim::{write_ani_file, HierAnim, ItemData, ItemHeaders};
use si_elemio::fcurve::{Fcurve, Key};
use si_elemio::Writer;


/// One curve: type and the value at each sampled frame.
pub struct Curve {
    pub ty: i32,
    pub vals: Vec<f64>,
}

fn varies(v: &[f64]) -> bool {
    v.iter().any(|x| (x - v[0]).abs() > 1e-6 * (1.0 + v[0].abs()))
}

/// Keeps a group of curves (a vector's three components) only if one of them moves.
pub fn group(tys: [i32; 3], vals: [Vec<f64>; 3]) -> Vec<Curve> {
    if vals.iter().any(|v| v.len() > 1 && varies(v)) {
        tys.iter().zip(vals).map(|(&ty, vals)| Curve { ty, vals }).collect()
    } else {
        Vec::new()
    }
}

/// The animation file of one element (`chapter` names the list the element is in, `name` the element).
pub fn ani_file(chapter: &str, name: &str, curves: &[Curve], frames: &[i32], fps: f64) -> Vec<u8> {
    let mut ha = HierAnim::new();
    ha.chapter = Some(chapter.as_bytes().to_vec());
    let e = ha.add_elem(name.as_bytes(), -1) as i32;
    for c in curves {
        let keys: Vec<Key> = frames
            .iter()
            .zip(&c.vals)
            .map(|(&f, &v)| {
                let mut k = Key::new();
                k.time = (f as f32) / (fps as f32);
                k.value = v as f32;
                k.interpolation = 1;
                k
            })
            .collect();
        let mut fc = Fcurve::blank(1);
        fc.curve_type = c.ty;
        fc.count = keys.len() as i32;
        fc.keys = keys;
        ha.add_item(e, 0, -1, 0, 1, ItemData::Curve(fc));
    }
    let mut w = Writer::new();
    write_ani_file(&mut w, Some(chapter.as_bytes()), &ha, ItemHeaders::Stored, false);
    w.out
}

/// Scale, Euler rotation (degrees, R = Rz Ry Rx of column vectors, i.e. X first) and translation of an affine matrix in
/// Softimage coordinates. Shear is dropped.
pub fn srt(a: &[[f64; 4]; 4]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let col = |j: usize| [a[0][j], a[1][j], a[2][j]];
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let s = [len(col(0)), len(col(1)), len(col(2))];
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][j] / if s[j] > 1e-12 { s[j] } else { 1.0 };
        }
    }
    let b = (-r[2][0]).clamp(-1.0, 1.0).asin();
    let (x, z);
    if b.cos().abs() > 1e-6 {
        x = r[2][1].atan2(r[2][2]);
        z = r[1][0].atan2(r[0][0]);
    } else {
        // gimbal lock: all of it into x
        z = 0.0;
        x = (-r[1][2]).atan2(r[1][1]);
    }
    (s, [x.to_degrees(), b.to_degrees(), z.to_degrees()], [a[0][3], a[1][3], a[2][3]])
}

/// Unwraps angles so that consecutive samples differ by less than half a turn.
pub fn unwrap(v: &mut [f64]) {
    for i in 1..v.len() {
        while v[i] - v[i - 1] > 180.0 {
            v[i] -= 360.0;
        }
        while v[i] - v[i - 1] < -180.0 {
            v[i] += 360.0;
        }
    }
}

/// Inverse of an affine 4x4 matrix (last row 0 0 0 1).
pub fn inverse(m: &[[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let mut inv = [[0.0; 4]; 4];
    for i in 0..3 {
        for j in 0..3 {
            let (r1, r2, c1, c2) = ((j + 1) % 3, (j + 2) % 3, (i + 1) % 3, (i + 2) % 3);
            inv[i][j] = (a[r1][c1] * a[r2][c2] - a[r1][c2] * a[r2][c1]) / det;
        }
    }
    for i in 0..3 {
        inv[i][3] = -(inv[i][0] * m[0][3] + inv[i][1] * m[1][3] + inv[i][2] * m[2][3]);
    }
    inv[3][3] = 1.0;
    Some(inv)
}

pub fn mul(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut o = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            o[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

/// A change of basis from Blender's axes to Softimage's: C T C^-1, with C: (x, y, z) -> (x, z, -y).
pub fn to_si(t: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let c = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let ci = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    mul(&mul(&c, t), &ci)
}
