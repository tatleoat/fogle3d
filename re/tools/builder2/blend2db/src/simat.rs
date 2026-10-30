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
    // a projection's rotation (+0x94, radians), translation (+0xa0) and scaling (+0xac) (unit imp3): only for the
    // projected methods, the only ones that read them, so a UV texture's record stays as it was
    if matches!(t.method, 1 | 2 | 3 | 5 | 6) {
        for (k, dst) in [("rotation", &mut t.rotation), ("translation", &mut t.translation), ("scaling", &mut t.scaling)] {
            if let Some(c) = c3(p, k) {
                *dst = c;
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

/// A projected 2D texture (methods 1, 2, 3 the planes, 5 the cylinder, 6 the sphere) of an object whose vertices are
/// written in world space, put back in the object's own space (unit imp3). The original projects a render vertex's
/// position in the node's own space (+0x20): P = rᵀ p_obj, r the texture's rotation (`SI_txtProjectionInfo` and
/// `SI_modelComputeRotBounding`, crates/si-render polygon.rs and softtriangle.rs), and takes the coordinates relative
/// to the box of all the node's P (scaled about its centre by the texture's scaling). With p_obj = A p + b that is
/// P = (rᵀ A) p + rᵀ b. When the rows of N = rᵀ A are orthogonal, a rotation r' with r'ᵀ = D N (D the positive
/// diagonal that makes the rows unit) gives P' = D (P − rᵀ b): every coordinate scaled by a positive number and moved,
/// which the box takes out, so a plane's coordinates are the original's. For a plane the axis it does not read
/// (z for XY, y for XZ, x for YZ) may be negated, which makes a mirrored object's r' a rotation too. The cylinder needs
/// D's x and z equal and the sphere all three (their u is an angle about the box's centre), and both a zero
/// translation (the original moves their centre by the translation times the box's half extents, which D changes).
/// The bump of a plane follows fixed axes of the space the renderer shades in (shade.rs), the same in both, so it is
/// the original's too. Returns None when nothing changes (not a projection, or A the identity with nothing that reads
/// b), Some(true) when the rotation was replaced (exact up to float rounding), Some(false) when there is no exact form:
/// the texture is left as it was (projected in world space).
pub fn texture2d_object_space(t: &mut Texture2D, a: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<bool> {
    let method = t.method;
    if !matches!(method, 1 | 2 | 3 | 5 | 6) {
        return None;
    }
    let round = matches!(method, 5 | 6);
    let moved = t.translation.iter().any(|&x| x != 0.0);
    let ident = (0..3).all(|i| (0..3).all(|j| a[i][j] == if i == j { 1.0 } else { 0.0 }));
    if ident && !(round && moved && b.iter().any(|&x| x != 0.0)) {
        return None;
    }
    if round && moved {
        return Some(false);
    }
    let r = xyz_rotation([t.rotation[0] as f64, t.rotation[1] as f64, t.rotation[2] as f64]);
    // N = rᵀ A: row i is Σ_k r[k][i] A[k][.]
    let n: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| (0..3).map(|k| r[k][i] * a[k][j]).sum::<f64>()));
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let l: [f64; 3] = core::array::from_fn(|i| len(n[i]));
    if !l.iter().all(|&x| x > 0.0 && x.is_finite()) {
        return Some(false);
    }
    let mut u: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|c| n[i][c] / l[i]));
    // looser than the importer's 1e-6 (importer.py `_plane_in_object_space`, on the dump's matrix), so that what the
    // importer kept as a projection is not refused here over Blender's single-precision matrices
    const TOL: f64 = 1e-4;
    if dot(u[0], u[1]).abs() > TOL || dot(u[0], u[2]).abs() > TOL || dot(u[1], u[2]).abs() > TOL {
        return Some(false);
    }
    let same = |p: f64, q: f64| (p - q).abs() <= TOL * p.abs().max(q.abs());
    if (method == 5 && !same(l[0], l[2])) || (method == 6 && !(same(l[0], l[1]) && same(l[0], l[2]))) {
        return Some(false);
    }
    let det = dot(u[0], [u[1][1] * u[2][2] - u[1][2] * u[2][1], u[1][2] * u[2][0] - u[1][0] * u[2][2], u[1][0] * u[2][1] - u[1][1] * u[2][0]]);
    if det < 0.0 {
        // a mirror: negate the axis the plane does not read
        let free = match method {
            1 => 2,
            2 => 1,
            3 => 0,
            _ => return Some(false),
        };
        u[free] = [-u[free][0], -u[free][1], -u[free][2]];
    }
    // r' = Uᵀ, and its angles for `xyz_rotation`
    let rr: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| u[j][i]));
    let e = xyz_angles(&rr);
    for i in 0..3 {
        t.rotation[i] = e[i] as f32;
    }
    Some(true)
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

#[cfg(test)]
mod projection_tests {
    use super::*;

    /// The planar (and cylinder/sphere angle) coordinates of points as the renderer forms them: P = rᵀ p, the box of all P
    /// scaled about its centre, then by method (t2d_fct.rs, polygon.rs).
    fn coords(t: &Texture2D, pts: &[[f64; 3]]) -> Vec<[f64; 2]> {
        let r = xyz_rotation([t.rotation[0] as f64, t.rotation[1] as f64, t.rotation[2] as f64]);
        let ps: Vec<[f64; 3]> = pts.iter().map(|p| core::array::from_fn(|c| (0..3).map(|i| p[i] * r[i][c]).sum::<f64>())).collect();
        let lo: [f64; 3] = core::array::from_fn(|c| ps.iter().map(|p| p[c]).fold(f64::MAX, f64::min));
        let hi: [f64; 3] = core::array::from_fn(|c| ps.iter().map(|p| p[c]).fold(f64::MIN, f64::max));
        let ctr: [f64; 3] = core::array::from_fn(|c| 0.5 * (lo[c] + hi[c]));
        let lo: [f64; 3] = core::array::from_fn(|c| ctr[c] - 0.5 * (hi[c] - lo[c]) * t.scaling[c] as f64);
        let size: [f64; 3] = core::array::from_fn(|c| 2.0 * (ctr[c] - lo[c]));
        let along = |p: &[f64; 3], c: usize| (p[c] - lo[c]) / size[c];
        ps.iter()
            .map(|p| match t.method {
                1 => [along(p, 0), 1.0 - along(p, 1)],
                2 => [along(p, 0), 1.0 - along(p, 2)],
                3 => [along(p, 2), 1.0 - along(p, 1)],
                _ => {
                    let d = [p[0] - ctr[0], p[1] - ctr[1], p[2] - ctr[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let u = (d[2].atan2(-d[0]) + std::f64::consts::PI) / std::f64::consts::PI * 0.5;
                    if t.method == 5 { [u, 1.0 - along(p, 1)] } else { [u, (d[1] / l).acos() / std::f64::consts::PI] }
                }
            })
            .collect()
    }

    fn tex(method: i16, rot: [f32; 3], sc: [f32; 3]) -> Texture2D {
        let mut t = Texture2D::default();
        t.method = method;
        t.rotation = rot;
        t.scaling = sc;
        t
    }

    /// The object's points, its world points p = A⁻¹ (p_obj − b) for world-to-object (A, b), and the check that the
    /// folded texture on the world points gives the original's coordinates.
    fn check(a: [[f64; 3]; 3], b: [f64; 3], t0: Texture2D, want: Option<bool>) {
        let obj: Vec<[f64; 3]> = vec![[1.0, 2.0, 3.0], [-4.0, 0.5, 7.0], [0.0, 0.0, 0.0], [10.0, -3.0, 2.0], [2.5, 6.0, -1.0], [-3.0, -2.0, -5.0]];
        // invert A by Cramer's rule
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        let inv: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| {
            let (r0, r1) = ((j + 1) % 3, (j + 2) % 3);
            let (c0, c1) = ((i + 1) % 3, (i + 2) % 3);
            (a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]) / det
        }));
        let world: Vec<[f64; 3]> = obj.iter().map(|p| core::array::from_fn(|i| (0..3).map(|j| inv[i][j] * (p[j] - b[j])).sum::<f64>())).collect();
        let mut t = t0.clone();
        let got = texture2d_object_space(&mut t, &a, &b);
        assert_eq!(got, want);
        if got == Some(false) {
            assert_eq!((t.rotation, t.scaling, t.translation), (t0.rotation, t0.scaling, t0.translation));
            return;
        }
        let (want_uv, got_uv) = (coords(&t0, &obj), coords(&t, &world));
        for (w, g) in want_uv.iter().zip(&got_uv) {
            for c in 0..2 {
                assert!((w[c] - g[c]).abs() < 1e-5, "{want_uv:?} {got_uv:?}");
            }
        }
    }

    const ID: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn identity_and_uv_change_nothing() {
        let mut t = tex(2, [0.1, 0.2, 0.3], [1.0, 2.0, 1.0]);
        assert_eq!(texture2d_object_space(&mut t, &ID, &[1.0, 2.0, 3.0]), None);
        let mut uv = tex(4, [0.0; 3], [1.0; 3]);
        assert_eq!(texture2d_object_space(&mut uv, &[[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], &[0.0; 3]), None);
    }

    #[test]
    fn planes_under_scaling_and_rotation() {
        // Sand_Gust's terrain: a scaling 110.68, 85.64, 133.15 and a translation; the third texture turned half round
        let a = [[1.0 / 110.6836, 0.0, 0.0], [0.0, 1.0 / 85.64204, 0.0], [0.0, 0.0, 1.0 / 133.1539]];
        let b = [0.0, 14.58177 / 85.64204, 196.6824 / 133.1539];
        for m in [1, 2, 3] {
            check(a, b, tex(m, [0.0, 3.1415927, 0.0], [1.0; 3]), Some(true));
            check(a, b, tex(m, [0.0; 3], [0.5, 2.0, 1.5]), Some(true));
        }
        // a rotated, unevenly scaled object (scaling first, in its own space) with an unrotated texture
        let r = xyz_rotation([0.3, -0.7, 1.1]);
        let s = [2.0, 0.5, 3.0];
        let a2: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| r[j][i] / s[i]));
        for m in [1, 2, 3] {
            check(a2, [1.0, -2.0, 0.5], tex(m, [0.0; 3], [1.0; 3]), Some(true));
        }
        // the same with a rotated texture: no exact form
        check(a2, [0.0; 3], tex(2, [0.0, 0.0, 0.4], [1.0; 3]), Some(false));
        // a uniform scaling under a rotation, the texture rotated too
        let a3: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| 0.25 * r[j][i]));
        check(a3, [0.5, 0.0, 1.0], tex(3, [0.2, 0.4, -0.1], [1.0; 3]), Some(true));
    }

    #[test]
    fn mirrored_object() {
        let a = [[-0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]];
        for m in [1, 2, 3] {
            check(a, [0.0; 3], tex(m, [0.0; 3], [1.0; 3]), Some(true));
        }
        check(a, [0.0; 3], tex(5, [0.0; 3], [1.0; 3]), Some(false));
    }

    #[test]
    fn cylinder_and_sphere() {
        let even = [[0.5, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 0.5]];
        check(even, [1.0, 0.0, 2.0], tex(5, [0.0; 3], [1.0; 3]), Some(true));
        check(even, [1.0, 0.0, 2.0], tex(6, [0.0; 3], [1.0; 3]), Some(false));
        let uniform = [[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]];
        check(uniform, [1.0, 0.0, 2.0], tex(6, [0.0; 3], [1.0; 3]), Some(true));
        let mut moved = tex(6, [0.0; 3], [1.0; 3]);
        moved.translation = [0.1, 0.0, 0.0];
        check(uniform, [1.0, 0.0, 2.0], moved, Some(false));
    }
}
