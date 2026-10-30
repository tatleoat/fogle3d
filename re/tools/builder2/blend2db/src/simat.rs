// SPDX-License-Identifier: MIT
//! Softimage node materials (unit nodes1): the add-on's `si` record of a material (export.py, from a Softimage Material
//! node and its Softimage Texture / 3D Texture nodes) as the program's records, field for field. Nothing is converted here:
//! the node holds Softimage values (colours as stored in a material file). A material without `si` never comes here
//! (`build::material_for` keeps the Principled formulas for it), and a texture without `si` keeps the old defaults.
//! Field meanings: docs/design/softimage-nodes.md and si_hrc's model.rs.
use serde_json::Value;
use si_hrc::{Material, Texture2D, Texture3D};

fn f(v: &Value, k: &str) -> Option<f32> {
    v.get(k).and_then(Value::as_f64).map(|x| x as f32)
}

fn i(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_bool().map(i64::from)))
}

fn c3(v: &Value, k: &str) -> Option<[f32; 3]> {
    let a = v.get(k)?.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([0, 1, 2].map(|n| a[n].as_f64().unwrap_or(0.0) as f32))
}

/// The material record of a Softimage Material node. Shading models outside 1..5 (6 and 7 fall through to stale values in
/// the original's shader) keep the default, Phong.
pub fn material(name: &str, s: &Value) -> Material {
    let mut m = Material::default();
    m.name = name.as_bytes().to_vec();
    if let Some(x) = i(s, "shading").filter(|x| (1..=5).contains(x)) {
        m.shading = x as i16;
    }
    if let Some(c) = c3(s, "ambient") {
        m.ambient = c;
    }
    if let Some(c) = c3(s, "diffuse") {
        m.diffuse = c;
    }
    if let Some(c) = c3(s, "specular") {
        m.specular = c;
    }
    let set = |dst: &mut f32, k: &str| {
        if let Some(x) = f(s, k) {
            *dst = x;
        }
    };
    set(&mut m.exponent, "exponent");
    set(&mut m.reflectivity, "reflectivity");
    set(&mut m.transparency, "transparency");
    set(&mut m.refraction, "refraction");
    set(&mut m.blur_width, "blur_width");
    set(&mut m.blur_decay, "blur_decay");
    if let Some(x) = i(s, "blur") {
        m.blur = x.clamp(0, 1) as i16;
    }
    m
}

/// The fields of a Softimage Texture node (or the overrides of a linked standard input) onto a 2D texture record.
pub fn apply_texture(t: &mut Texture2D, p: &Value) {
    if !p.is_object() {
        return;
    }
    let short = |k: &str, lo: i64, hi: i64| i(p, k).filter(|x| (lo..=hi).contains(x)).map(|x| x as i16);
    if let Some(x) = short("method", 1, 8) {
        t.method = x;
    }
    if let Some(x) = short("pixel_interp", 0, 1) {
        t.pixel_interp = x;
    }
    if let Some(x) = short("urepeat", 1, 32767) {
        t.urepeat = x;
    }
    if let Some(x) = short("vrepeat", 1, 32767) {
        t.vrepeat = x;
    }
    if let Some(x) = short("swap_uv", 0, 1) {
        t.swap_uv = x;
    }
    if let Some(x) = short("alt_u", 0, 1) {
        t.alt_u = x;
    }
    if let Some(x) = short("alt_v", 0, 1) {
        t.alt_v = x;
    }
    if let Some(x) = short("mask", 1, 4) {
        t.mask = x;
    }
    if let Some(x) = short("channel", 1, 2) {
        t.channel = x;
    }
    if let Some(x) = short("black_white", 0, 1) {
        t.black_white = x;
    }
    // the picture's cropping (unit fidelity2): the switch (+0x2e) and the box x0, x1, y0, y1 (+0x30 to +0x36, the
    // record's first four crop shorts; the fifth is the neighbouring field and keeps its default)
    if let Some(x) = short("crop_flag", 0, 1) {
        t.crop_flag = x;
    }
    if let Some(c) = p["crop"].as_array().filter(|c| c.len() == 4) {
        for (k, v) in c.iter().enumerate() {
            if let Some(x) = v.as_i64().filter(|x| (-32768..=32767).contains(x)) {
                t.crop[k] = x as i16;
            }
        }
    }
    for (k, dst) in [
        ("uscale", &mut t.uscale),
        ("vscale", &mut t.vscale),
        ("uoffset", &mut t.uoffset),
        ("voffset", &mut t.voffset),
        ("blend", &mut t.blend),
        ("ambient", &mut t.ambient),
        ("diffuse", &mut t.diffuse),
        ("specular", &mut t.specular),
        ("reflectivity", &mut t.reflectivity),
        ("transparency", &mut t.transparency),
        ("roughness", &mut t.roughness),
        ("reflection_map", &mut t.reflection_map),
        ("env_rotation", &mut t.env_rotation),
    ] {
        if let Some(x) = f(p, k) {
            *dst = x;
        }
    }
}

/// The 3D texture record of a Softimage 3D Texture node (marble 0, wood 1, cloud 2).
pub fn texture3d(name: &str, p: &Value) -> Texture3D {
    let mut t = Texture3D::default();
    t.name = name.as_bytes().to_vec();
    if let Some(x) = i(p, "kind").filter(|x| (0..=2).contains(x)) {
        t.kind = x as i32;
    }
    if let Some(x) = i(p, "iterations").filter(|x| (1..=32).contains(x)) {
        t.iterations = x as i32;
    }
    if let Some(x) = i(p, "mask").filter(|x| (1..=4).contains(x)) {
        t.mask = x as i16;
    }
    if let Some(x) = i(p, "channel").filter(|x| (1..=2).contains(x)) {
        t.channel = x as i16;
    }
    for (k, dst) in [
        ("spacing", &mut t.spacing),
        ("taper", &mut t.taper),
        ("strength", &mut t.strength),
        ("power", &mut t.power),
        ("blending", &mut t.blending),
        ("ambient", &mut t.ambient),
        ("diffuse", &mut t.diffuse),
        ("specular", &mut t.specular),
        ("reflection", &mut t.reflection),
        ("transparency", &mut t.transparency),
        ("roughness", &mut t.roughness),
    ] {
        if let Some(x) = f(p, k) {
            *dst = x;
        }
    }
    for (k, dst) in [("scaling", &mut t.scaling), ("rotation", &mut t.rotation), ("translation", &mut t.translation)] {
        if let Some(c) = c3(p, k) {
            *dst = c;
        }
    }
    if let Some(cs) = p.get("colours").and_then(Value::as_array) {
        for (n, c) in cs.iter().take(5).enumerate() {
            if let Some(a) = c.as_array() {
                for k in 0..4 {
                    t.colours[n][k] = a.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                }
            }
        }
    }
    if let Some(fs) = p.get("factors").and_then(Value::as_array) {
        for (n, x) in fs.iter().take(3).enumerate() {
            t.factors[n] = x.as_f64().unwrap_or(t.factors[n] as f64) as f32;
        }
    }
    t
}

/// A 3D texture of an object whose vertices are written in world space (build.rs bakes the object's matrix into them,
/// the model's own transformation is the identity), put back in the object's own space (unit sky2).  The original
/// takes a point to the node's space (the inverse of its global matrix) and then through the texture's own matrix
/// (`updateTxt3DTransf`, 0x0826f5ec, as si-render reproduces it: the rotation's rows times the scaling, the translation
/// in row 3; row vectors): p_tex = p_obj K + tr.  With p_obj = A p + b (column vectors: `a`, `b`, Softimage world to
/// the object's space) that is p (Aᵀ K) + (b K + tr), written back as scaling, rotation and translation.
/// Returns None when nothing changes (the object's matrix is the identity), Some(true) when the result is exact up to
/// float rounding, Some(false) when Aᵀ K has no exact scaling-and-rotation form (a non-uniform scaling under a
/// rotation, or a zero scaling): then the nearest one, rows made orthogonal in order.
pub fn texture3d_object_space(t: &mut Texture3D, a: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<bool> {
    let ident = (0..3).all(|i| (0..3).all(|j| a[i][j] == if i == j { 1.0 } else { 0.0 })) && b.iter().all(|&x| x == 0.0);
    if ident {
        return None;
    }
    let s: [f64; 3] = core::array::from_fn(|i| t.scaling[i] as f64);
    let r = xyz_rotation([t.rotation[0] as f64, t.rotation[1] as f64, t.rotation[2] as f64]);
    let k: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|c| s[i] * r[i][c]));
    // the new translation: b K + tr
    let tr: [f64; 3] = core::array::from_fn(|c| t.translation[c] as f64 + (0..3).map(|i| b[i] * k[i][c]).sum::<f64>());
    let diagonal = (0..3).all(|i| (0..3).all(|j| i == j || a[i][j] == 0.0));
    let exact = if diagonal {
        // row i of Aᵀ K is a_ii times row i of K: the scaling changes, the rotation stays as it is
        for i in 0..3 {
            t.scaling[i] = (a[i][i] * s[i]) as f32;
        }
        true
    } else {
        let m: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|c| (0..3).map(|j| a[j][i] * k[j][c]).sum::<f64>()));
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let sc: [f64; 3] = core::array::from_fn(|i| len(m[i]));
        let mut rows: [[f64; 3]; 3] = core::array::from_fn(|i| if sc[i] > 0.0 { core::array::from_fn(|c| m[i][c] / sc[i]) } else { [0.0; 3] });
        let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let mut exact = sc.iter().all(|&x| x > 0.0) && dot(rows[0], rows[1]).abs() < 1e-6 && dot(rows[0], rows[2]).abs() < 1e-6 && dot(rows[1], rows[2]).abs() < 1e-6;
        if !exact {
            // the nearest rotation: the rows made orthogonal in order (a zero row from the other two)
            let cross = |u: [f64; 3], v: [f64; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let unit = |v: [f64; 3]| { let l = len(v); if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0; 3] } };
            let fix = |rows: &mut [[f64; 3]; 3]| {
                if len(rows[0]) == 0.0 {
                    rows[0] = unit(cross(rows[1], rows[2]));
                }
                if len(rows[0]) == 0.0 {
                    rows[0] = [1.0, 0.0, 0.0];
                }
                let r0 = rows[0];
                let d = dot(rows[1], r0);
                rows[1] = unit([rows[1][0] - d * r0[0], rows[1][1] - d * r0[1], rows[1][2] - d * r0[2]]);
                if len(rows[1]) == 0.0 {
                    let any = if r0[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                    rows[1] = unit(cross(cross(r0, any), r0));
                }
                rows[2] = unit(cross(r0, rows[1]));
            };
            fix(&mut rows);
            exact = false;
        }
        // a mirror: the third row and its scaling negated, so the rows are a rotation
        let det = dot(rows[0], [rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1], rows[1][2] * rows[2][0] - rows[1][0] * rows[2][2], rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]]);
        let mut sc = sc;
        if det < 0.0 {
            rows[2] = [-rows[2][0], -rows[2][1], -rows[2][2]];
            sc[2] = -sc[2];
        }
        // the scaling of each row: its length along the new row (the length itself when exact)
        for i in 0..3 {
            t.scaling[i] = if exact { sc[i] } else { dot(m[i], rows[i]) } as f32;
        }
        let e = xyz_angles(&rows);
        for i in 0..3 {
            t.rotation[i] = e[i] as f32;
        }
        exact
    };
    for c in 0..3 {
        t.translation[c] = tr[c] as f32;
    }
    Some(exact)
}

/// The rotation matrix of si-math's `matrix_xyz_rotation` (radians), in f64.
fn xyz_rotation(rot: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = rot[0].sin_cos();
    let (sy, cy) = rot[1].sin_cos();
    let (sz, cz) = rot[2].sin_cos();
    [
        [cy * cz, cy * sz, -sy],
        [sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy],
        [sx * sz + cx * sy * cz, sz * cx * sy - cz * sx, cx * cy],
    ]
}

/// The angles `xyz_rotation` takes for the rotation `r` (one of the two solutions; at cos y = 0 the z angle 0).
fn xyz_angles(r: &[[f64; 3]; 3]) -> [f64; 3] {
    let ry = (-r[0][2]).clamp(-1.0, 1.0).asin();
    if ry.cos() > 1e-9 {
        [r[1][2].atan2(r[2][2]), ry, r[0][1].atan2(r[0][0])]
    } else {
        [(-r[2][1]).atan2(r[1][1]), ry, 0.0]
    }
}

#[cfg(test)]
mod object_space_tests {
    use super::*;

    fn apply(t: &Texture3D, p: [f64; 3]) -> [f64; 3] {
        let r = xyz_rotation([t.rotation[0] as f64, t.rotation[1] as f64, t.rotation[2] as f64]);
        core::array::from_fn(|c| t.translation[c] as f64 + (0..3).map(|i| p[i] * t.scaling[i] as f64 * r[i][c]).sum::<f64>())
    }

    fn tex(s: [f32; 3], r: [f32; 3], tr: [f32; 3]) -> Texture3D {
        let mut t = Texture3D::default();
        t.scaling = s;
        t.rotation = r;
        t.translation = tr;
        t
    }

    fn check(a: [[f64; 3]; 3], b: [f64; 3], t0: Texture3D, want_exact: bool) {
        let mut t = t0.clone();
        let ex = texture3d_object_space(&mut t, &a, &b);
        assert_eq!(ex, Some(want_exact));
        if !want_exact {
            return;
        }
        for p in [[1.0, 2.0, 3.0], [-4.0, 0.5, 7.0], [0.0, 0.0, 0.0], [10.0, -3.0, 2.0]] {
            let obj: [f64; 3] = core::array::from_fn(|i| (0..3).map(|j| a[i][j] * p[j]).sum::<f64>() + b[i]);
            let want = apply(&t0, obj);
            let got = apply(&t, p);
            for c in 0..3 {
                assert!((want[c] - got[c]).abs() < 1e-4 * (1.0 + want[c].abs()), "{want:?} {got:?}");
            }
        }
    }

    #[test]
    fn identity_changes_nothing() {
        let mut t = tex([0.5, 3.0, 1.0], [0.1, 0.2, 0.3], [1.0, 2.0, 3.0]);
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(texture3d_object_space(&mut t, &id, &[0.0; 3]), None);
        assert_eq!((t.scaling, t.rotation, t.translation), ([0.5, 3.0, 1.0], [0.1, 0.2, 0.3], [1.0, 2.0, 3.0]));
    }

    #[test]
    fn diagonal_keeps_rotation() {
        // Sand_Gust's sphere: scaling 204.0102, 43.58796, 204.0102 and a translation
        let a = [[1.0 / 204.0102, 0.0, 0.0], [0.0, 1.0 / 43.58796, 0.0], [0.0, 0.0, 1.0 / 204.0102]];
        let b = [5.26 / 204.0102, 6.444 / 43.58796, 289.276 / 204.0102];
        let t0 = tex([0.5, 3.0, 1.0], [0.0, 0.034906585, 0.0], [0.0, 7.0, 0.5]);
        check(a, b, t0.clone(), true);
        let mut t = t0.clone();
        texture3d_object_space(&mut t, &a, &b);
        assert_eq!(t.rotation, t0.rotation);
    }

    #[test]
    fn rotation_uniform_scale_exact() {
        let r = xyz_rotation([0.3, -0.7, 1.1]);
        let a: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| 0.25 * r[i][j]));
        check(a, [1.0, -2.0, 0.5], tex([2.0, 2.0, 2.0], [0.0, 0.0, 0.0], [0.0; 3]), true);
        check(a, [1.0, -2.0, 0.5], tex([1.0, 1.0, 1.0], [0.2, 0.4, -0.1], [0.3, 0.0, 1.0]), true);
    }

    #[test]
    fn mirror_and_skew() {
        let a = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        check(a, [0.0; 3], tex([1.0, 2.0, 3.0], [0.0; 3], [0.0; 3]), true);
        let m = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        check(m, [0.0; 3], tex([1.0, 2.0, 3.0], [0.1, 0.0, 0.0], [0.0; 3]), true);
        // a non-uniform texture scaling under a rotated object: only approximate
        let r = xyz_rotation([0.0, 0.0, 0.5]);
        check(r, [0.0; 3], tex([1.0, 5.0, 1.0], [0.0; 3], [0.0; 3]), false);
    }
}
