// SPDX-License-Identifier: MIT
//! `blend2db build <scene.json> <out dir> <template db dir> <template scene> <rsrc dir>`: a database the
//! renderer's loader accepts, from the JSON of `export.scene_to_dict`.
//!
//! Conventions (what does not match the original's, and why, is in the add-on's README of this unit):
//! - Blender is right-handed Z-up; Softimage 3D is Y-up. (x, y, z) -> (x, z, -y), a rotation: the winding and the
//!   handedness stay. Units are 1:1 (one Blender unit is one Softimage unit).
//! - World matrices are baked into the vertices; every object is a root model with an identity transform.
//! - Colours: Blender's linear values are written sRGB-encoded (Softimage shades in display values).
//! - Lights (unit builder2): colour, fall-off end and spot cone/spread are fitted per light against Cycles' own formulas (fit.rs);
//!   shadow_type 2 (ray traced) when the light casts shadows; area lights are cosine-lobe spots. Ambience is fitted likewise.
//! - The set-up file (render options) is the template's, patched for the range, resolution, aspect, output name,
//!   ambience and ray depth, and BACK_CULLING (ACTIVE when every material drawn has Backface Culling, unit finish1).
//! - A light the Softimage importer made, unchanged since, keeps the scene's own colour, fall-off, cone, spread and umbra
//!   (the "si" values export.py hands over; unit finish1) instead of the fit.
use crate::construct::{flat_material, mesh_node, Corner, MeshData};
use crate::anim;
use crate::fit::{self, Sample};
use serde_json::Value;
use si_elemio::{camera, light, Writer};
use std::fmt::Write as _;
use std::path::PathBuf;

type M4 = [[f64; 4]; 4];

fn mat(v: &Value) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (i, r) in v.as_array().into_iter().flatten().enumerate().take(4) {
        for (j, c) in r.as_array().into_iter().flatten().enumerate().take(4) {
            m[i][j] = c.as_f64().unwrap_or(0.0);
        }
    }
    m
}

fn f3(v: &Value) -> [f64; 3] {
    let a = v.as_array().cloned().unwrap_or_default();
    let g = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    [g(0), g(1), g(2)]
}

/// Blender world -> Softimage world.
fn conv(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[2], -p[1]]
}

/// Softimage world -> the object's own Softimage space, for the Blender object matrix `m` (the inverse of `xform` then
/// `conv`): (A, b) with p_object = A p + b; None when the matrix has no inverse.
fn world_to_object(m: &M4) -> Option<([[f64; 3]; 3], [f64; 3])> {
    let a = |i: usize, j: usize| m[i][j];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    // the inverse of the 3x3 part (adjugate over the determinant); exact for the identity
    let inv = [
        [(a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) / det, (a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2)) / det, (a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1)) / det],
        [(a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2)) / det, (a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0)) / det, (a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2)) / det],
        [(a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)) / det, (a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)) / det, (a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)) / det],
    ];
    let unconv = |q: [f64; 3]| [q[0], -q[2], q[1]];
    let lin = |q: [f64; 3]| conv([0, 1, 2].map(|i| inv[i][0] * q[0] + inv[i][1] * q[1] + inv[i][2] * q[2]));
    let cols: [[f64; 3]; 3] = core::array::from_fn(|j| lin(unconv(core::array::from_fn(|k| if k == j { 1.0 } else { 0.0 }))));
    let am: [[f64; 3]; 3] = core::array::from_fn(|i| core::array::from_fn(|j| cols[j][i]));
    let t = conv([m[0][3], m[1][3], m[2][3]]);
    let b = [0, 1, 2].map(|i| -(am[i][0] * t[0] + am[i][1] * t[1] + am[i][2] * t[2]));
    Some((am, b))
}

fn xform(m: &M4, p: [f64; 3]) -> [f64; 3] {
    let r = |i: usize| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3];
    [r(0), r(1), r(2)]
}

/// Direction through the 3x3 part (normals: the inverse transpose, here by the cofactor matrix).
fn xnormal(m: &M4, n: [f64; 3]) -> [f64; 3] {
    let a = |i: usize, j: usize| m[i][j];
    let c = [
        [a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1), a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2), a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)],
        [a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2), a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0), a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)],
        [a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1), a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2), a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)],
    ];
    let r = [
        c[0][0] * n[0] + c[0][1] * n[1] + c[0][2] * n[2],
        c[1][0] * n[0] + c[1][1] * n[1] + c[1][2] * n[2],
        c[2][0] * n[0] + c[2][1] * n[1] + c[2][2] * n[2],
    ];
    norm(r)
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-20 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn add(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn f32x3(v: [f64; 3]) -> [f32; 3] {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

fn srgb(c: f64) -> f32 {
    let c = c.clamp(0.0, 1.0);
    (if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }) as f32
}

fn col(v: &Value) -> [f32; 3] {
    let c = f3(v);
    [srgb(c[0]), srgb(c[1]), srgb(c[2])]
}

/// A name the element syntax accepts (`<prefix>-<name>.<version>-<substitution>`): letters, digits, underscore.
fn clean(s: &str, taken: &mut Vec<String>) -> String {
    let mut o: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).take(24).collect();
    if o.is_empty() {
        o = "obj".into();
    }
    let base = o.clone();
    let mut k = 1;
    while taken.contains(&o) {
        k += 1;
        o = format!("{base}{k}");
    }
    taken.push(o.clone());
    o
}

fn pic_name(raw: &str) -> String {
    std::path::Path::new(raw).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into())
}

/// The add-on's raw dump of a Blender image (`SIRAW`, width, height, planes, bytes interleaved, top row first) as a picture file.
fn write_pic(raw: &str, to: &std::path::Path) -> Result<(), String> {
    let d = std::fs::read(raw).map_err(|e| format!("{raw}: {e}"))?;
    if d.len() < 17 || &d[..5] != b"SIRAW" {
        return Err(format!("{raw}: not a raw image"));
    }
    let u = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize;
    let (w, h, np) = (u(5), u(9), u(13));
    if d.len() < 17 + w * h * np || w == 0 || h == 0 || w > 32767 || h > 32767 || !(np == 3 || np == 4) {
        return Err(format!("{raw}: bad raw image"));
    }
    let px = &d[17..];
    let mut p = si_picture::Picture { width: w as i16, height: h as i16, ratio: 1.0, ..Default::default() };
    for c in 0..np {
        p.planes[c] = Some((0..w * h).map(|i| px[i * np + c]).collect());
    }
    std::fs::write(to, p.write(Some(b"blend2db"), false)).map_err(|e| format!("{to:?}: {e}"))
}

fn material_for(name: &str, m: Option<&Value>) -> si_hrc::Material {
    let Some(m) = m else {
        return flat_material(name, [0.5; 3], [0.7; 3], [0.0; 3], 50.0, 0.0, 0.0);
    };
    // a Softimage Material node (unit nodes1): its record as the node holds it, no Principled formulas
    if let Some(s) = m.get("si").filter(|s| s.is_object()) {
        return crate::simat::material(name, s);
    }
    let d = m["diffuse"].as_array().cloned().unwrap_or_default();
    let dv = Value::Array(d.iter().take(3).cloned().collect());
    let diffuse = col(&dv);
    let rough = m["roughness"].as_f64().unwrap_or(0.5).clamp(0.05, 1.0);
    let spec_i = m["specular"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);
    let metal = m["metallic"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
    let alpha = m["alpha"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0);
    let exponent = (2.0 / (rough * rough) - 2.0).clamp(1.0, 500.0) as f32;
    // dielectric highlight (white, weak) blending to the base colour as metallic rises
    let s = (spec_i * (1.0 - rough) * 0.5).clamp(0.0, 1.0) as f32;
    let spec = [0, 1, 2].map(|i| s * (1.0 - metal as f32) + diffuse[i] * metal as f32);
    // Principled transmission (glass) is the original's transparency: the light that passes, bent by the refraction index
    let trans = m["transmission"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
    let transp = 1.0 - alpha * (1.0 - trans);
    let mut mt = flat_material(name, diffuse, diffuse, spec, exponent, transp as f32, metal as f32);
    // an emissive Principled surface lights itself: constant shading in the emitted colour (the original has no emission
    // input; a constant-shaded model is the nearest self-lit surface)
    if let Some(e) = m["emission"].as_array() {
        let ec = [0, 1, 2].map(|i| e.get(i).and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 1.0) as f32);
        if ec.iter().cloned().fold(0.0, f32::max) > 0.0 {
            mt.shading = 1;
            mt.diffuse = ec;
            mt.ambient = ec;
        }
    }
    if let Some(ior) = m["ior"].as_f64() {
        if transp > 0.0 {
            mt.refraction = ior as f32;
        }
    }
    mt
}

fn light_file(kind: i32, name: &str, colour: [f32; 3], pos: [f64; 3], interest: [f64; 3], cone: f32, spread: f32, fall: (f32, f32), shadow: bool, soft: Option<i32>, umbra: Option<f32>, sel: Option<i32>, own: Option<&Value>) -> Vec<u8> {
    let mut l = light::Light::new(kind, 0);
    // SELECTIVE: on, and the value a model's entry must hold to be lit: 1 lights the models the scene's 2200
    // relations name (inclusive), 0 all the others (exclusive)
    if let Some(v) = sel {
        l.selective = [1, v];
    }
    l.name = [0; 30];
    l.name[..name.len().min(29)].copy_from_slice(&name.as_bytes()[..name.len().min(29)]);
    l.colour = colour;
    l.position = f32x3(pos);
    l.interest = f32x3(interest);
    l.cone_angle = cone;
    l.spread_angle = spread;
    l.fall_off_start = fall.0;
    l.fall_off_end = fall.1;
    l.shadow_type = if !shadow { 1 } else if soft.is_some() { 4 } else { 2 };
    if let Some(f) = soft {
        l.shadow_filter_size = f;
    }
    if let Some(u) = umbra {
        l.shadow_factor = u;
    }
    // an imported light left as it was (export.py hands its Softimage values over, "si"): the scene's own shadow kind
    // (SHADOW_TYPE: 2 ray traced, 3 and 4 depth maps, others none) and depth-map values, not the ray-traced shadow a
    // Blender light's switch gives (unit extra2: Torch_Fire's spot1 is a soft depth map, 4)
    if let Some(s) = own {
        if let Some(k) = s["shadow"].as_i64().filter(|k| (-32768..=32767).contains(k)) {
            l.shadow_type = k as i16;
        }
        if let Some(v) = s["shadow_map"].as_array().filter(|v| v.len() == 5) {
            let n = |i: usize| v[i].as_f64();
            if let (Some(res), Some(size), Some(step), Some(start), Some(rate)) = (n(0), n(1), n(2), n(3), n(4)) {
                (l.shadow_map_res, l.shadow_filter_size, l.shadow_filter_step) = (res as i32, size as i32, step as i32);
                (l.shadow_fade_start, l.shadow_fade_rate) = (start as f32, rate as f32);
            }
        }
    }
    let mut w = Writer::new();
    light::write_file(&mut w, &l);
    w.out
}

fn patch_setup(template: &str, first: i32, last: i32, w: i64, h: i64, ambience: [f32; 3], out_name: &str, ray_depth: i32, back_culling: bool, extra: &[(String, String)]) -> String {
    let mut o = String::new();
    for line in template.lines() {
        let t = line.trim_start();
        let ind = &line[..line.len() - t.len()];
        let rep = if t.starts_with("RENDERING_FRAME ") {
            Some(format!("RENDERING_FRAME {first} {last} 1"))
        } else if t.starts_with("OUTPUT_FILE ") {
            Some(format!("OUTPUT_FILE '/BLEND/RENDER_PICTURES/{out_name}'"))
        } else if t.starts_with("RESOLUTION ") {
            Some(format!("RESOLUTION {w} {h}"))
        } else if t.starts_with("ASPECT ") {
            Some(format!("ASPECT {}", w as f64 / h as f64))
        } else if t.starts_with("AMBIENCE ") {
            Some(format!("AMBIENCE {} {} {}", ambience[0], ambience[1], ambience[2]))
        } else if t.starts_with("MAX_RAY_DEPTH ") {
            Some(format!("MAX_RAY_DEPTH {ray_depth}"))
        } else if t.starts_with("BACK_CULLING ") {
            Some(if back_culling { "BACK_CULLING ACTIVE" } else { "BACK_CULLING INACTIVE" }.to_string())
        } else if t.starts_with("ANTIALIAS ") {
            // the scene's own words when the add-on hands them (an imported scene's set-up, unit fidelity2)
            Some(match extra.iter().find(|(k, _)| k == "ANTIALIAS") {
                Some((_, v)) => format!("ANTIALIAS {v}"),
                None => "ANTIALIAS ADAPTIVE 2 2 0.05".to_string(),
            })
        } else {
            // set-up words the add-on asks for (unit blender3: fields, motion blur, shutter), first word of the line
            extra.iter().find(|(k, _)| t.split_whitespace().next() == Some(k.as_str())).map(|(k, v)| format!("{k} {v}"))
        };
        o.push_str(ind);
        o.push_str(rep.as_deref().unwrap_or(t));
        o.push('\n');
    }
    // the original's file ends without a newline after EOF
    while o.ends_with('\n') {
        o.pop();
    }
    o
}

pub fn run(json: &str, out: &str, tdb: &str, tscene: &str, rsrc: &str) -> i32 {
    match build(json, out, tdb, tscene, rsrc) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("blend2db: {e}");
            1
        }
    }
}

fn build(json: &str, out: &str, tdb: &str, tscene: &str, rsrc: &str) -> Result<String, String> {
    let sc: Value = serde_json::from_slice(&std::fs::read(json).map_err(|e| format!("{json}: {e}"))?).map_err(|e| format!("json: {e}"))?;
    let (out, tdb) = (PathBuf::from(out), PathBuf::from(tdb));
    crate::roundtrip::copy_rsrc(&PathBuf::from(rsrc), &out).map_err(|e| format!("rsrc: {e}"))?;
    let db = out.join("db");
    let prefix = "blend";
    let res = sc["resolution"].as_array().cloned().unwrap_or_default();
    let pct = res.get(2).and_then(Value::as_f64).unwrap_or(100.0) / 100.0;
    let w = ((res.first().and_then(Value::as_f64).unwrap_or(320.0)) * pct).round().max(8.0) as i64;
    let h = ((res.get(1).and_then(Value::as_f64).unwrap_or(240.0)) * pct).round().max(8.0) as i64;
    let first = sc["frame_start"].as_i64().unwrap_or(1) as i32;
    // still frame: the frame asked for, else the first
    let frame = sc["frame"].as_i64().map(|f| f as i32).unwrap_or(first);
    // a still, or the sampled range (the add-on sends "anim" with one entry per frame; RENDER_FRAME then picks the frame)
    let a0 = sc["anim"]["first"].as_i64().unwrap_or(0) as i32;
    let anim = sc["anim"]["frames"].as_array().filter(|f| f.len() > 1 && a0 + f.len() as i32 - 1 == sc["anim"]["last"].as_i64().unwrap_or(-1) as i32);
    let (first, last) = match anim {
        Some(f) => (a0, a0 + f.len() as i32 - 1),
        None => (frame, frame),
    };
    let frame_nums: Vec<i32> = (first..=last).collect();
    let mut anim_files: Vec<(String, Vec<u8>)> = Vec::new(); // ANIMATION elements and their files
    let mut anim_rel: Vec<(&str, usize, usize, i32)> = Vec::new(); // (chapter, element index, animation index, relation id)
    let fps = sc["fps"].as_f64().unwrap_or(30.0);
    let mut report = String::new();

    let dir = |c: &str| -> Result<PathBuf, String> {
        let d = db.join(c);
        std::fs::create_dir_all(&d).map_err(|e| format!("{d:?}: {e}"))?;
        Ok(d)
    };

    // ---- models
    let mut names = Vec::new();
    let mut models: Vec<String> = Vec::new(); // element names, "blend-<n>.1-0"
    let mut mat_elems: Vec<String> = Vec::new();
    let mut model_mats: Vec<(usize, usize)> = Vec::new(); // (model index, material element index)
    let mut ntri = 0usize;
    let mut mat_src: Vec<Option<String>> = Vec::new(); // the Blender material of each Softimage material
    let mut tex_elems: Vec<String> = Vec::new(); // TEXTURES2D elements
    let mut tex_pics: Vec<String> = Vec::new(); // PICTURES entries (names without extension)
    let mut model_tex: Vec<(usize, usize)> = Vec::new();
    let mut mat_tex: Vec<(usize, usize)> = Vec::new(); // (material element, TEXTURES2D element): local textures, relation 401
    let mut t3d_elems: Vec<String> = Vec::new(); // TEXTURES3D elements
    let mut mat_t3d: Vec<(usize, usize)> = Vec::new(); // relation 501
    let mut samples: Vec<Sample> = Vec::new();
    let mut shape_elems: Vec<String> = Vec::new(); // SHAPES elements (unit anim1: deforming objects)
    let mut model_shape: Vec<(usize, usize)> = Vec::new(); // (model, shape element): relation 200
    let (mut cull_on, mut cull_off) = (0usize, Vec::<String>::new()); // materials drawn with and without Backface Culling
    let mut skipped_notes: Vec<String> = sc["skipped"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let mut model_src: Vec<String> = Vec::new(); // the Blender object each model comes from (light linking)
    for ob in sc["objects"].as_array().into_iter().flatten() {
        let oname = clean(ob["name"].as_str().unwrap_or("obj"), &mut names);
        let m = mat(&ob["matrix_world"]);
        let verts: Vec<[f32; 3]> = ob["verts"].as_array().into_iter().flatten().map(|v| f32x3(conv(xform(&m, f3(v))))).collect();
        let tris: Vec<Vec<usize>> = match ob["tris"].as_array() {
            Some(t) => t.iter().map(|x| x.as_array().map(|a| a.iter().map(|i| i.as_u64().unwrap_or(0) as usize).collect()).unwrap_or_default()).collect(),
            None => {
                // no triangulation given: fans of the polygons (right for convex polygons)
                let mut t = Vec::new();
                for p in ob["polys"].as_array().into_iter().flatten() {
                    let p: Vec<usize> = p.as_array().map(|a| a.iter().map(|i| i.as_u64().unwrap_or(0) as usize).collect()).unwrap_or_default();
                    for k in 1..p.len().saturating_sub(1) {
                        t.push(vec![p[0], p[k], p[k + 1]]);
                    }
                }
                t
            }
        };
        let tri_poly: Vec<usize> = ob["tri_poly"].as_array().map(|a| a.iter().map(|i| i.as_u64().unwrap_or(0) as usize).collect()).unwrap_or_default();
        let polys_mat: Vec<i64> = ob["poly_material"].as_array().map(|a| a.iter().map(|i| i.as_i64().unwrap_or(0)).collect()).unwrap_or_default();
        let tri_normals = ob["tri_normals"].as_array();
        let tri_uv = ob["tri_uv"].as_array();
        if verts.is_empty() || tris.is_empty() {
            skipped_notes.push(format!("{oname} (no triangles)"));
            continue;
        }
        // face normals for corners without a given normal
        let mut corners: Vec<Vec<Corner>> = Vec::new();
        let mut pmat: Vec<i16> = Vec::new();
        let mut tri_of: Vec<usize> = Vec::new(); // the triangle each kept polygon came from
        for (ti, t) in tris.iter().enumerate() {
            if t.len() != 3 || t.iter().any(|&i| i >= verts.len()) {
                continue;
            }
            let p: Vec<[f64; 3]> = t.iter().map(|&i| [verts[i][0] as f64, verts[i][1] as f64, verts[i][2] as f64]).collect();
            let fnrm = norm(cross(add(p[1], p[0], -1.0), add(p[2], p[0], -1.0)));
            let mut cs = Vec::new();
            for k in 0..3 {
                let n = tri_normals
                    .and_then(|a| a.get(ti))
                    .and_then(|a| a.get(k))
                    .map(|v| conv(xnormal(&m, f3(v))))
                    .unwrap_or(fnrm);
                let uv = tri_uv
                    .and_then(|a| a.get(ti))
                    .and_then(|a| a.get(k))
                    .and_then(Value::as_array)
                    .map(|a| [a.first().and_then(Value::as_f64).unwrap_or(0.0) as f32, a.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32])
                    .unwrap_or([0.0; 2]);
                cs.push(Corner { vertex: t[k], normal: f32x3(norm(n)), uv });
            }
            corners.push(cs);
            tri_of.push(ti);
            let pi = tri_poly.get(ti).copied().unwrap_or(ti);
            pmat.push(polys_mat.get(pi).copied().unwrap_or(0).max(0) as i16);
        }
        for (k, cs) in corners.iter().enumerate() {
            let p: Vec<[f64; 3]> = cs.iter().map(|c| [verts[c.vertex][0] as f64, verts[c.vertex][1] as f64, verts[c.vertex][2] as f64]).collect();
            let nn = cross(add(p[1], p[0], -1.0), add(p[2], p[0], -1.0));
            let len = (dot(nn, nn)).sqrt();
            if len < 1e-12 {
                continue;
            }
            let slot = pmat.get(k).copied().unwrap_or(0).max(0) as usize;
            let a = ob["materials"].as_array().and_then(|v| v.get(slot)).and_then(Value::as_str).and_then(|n| sc["materials"].get(n)).map(|m| match m["textures"].as_array().and_then(|t| t.iter().find(|t| t["raw"].is_string())) {
                Some(t) => f3(&t["mean"]),
                None => f3(&m["diffuse"]),
            }).unwrap_or([0.5; 3]);
            samples.push(Sample { p: [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, (p[0][2] + p[1][2] + p[2][2]) / 3.0], n: [nn[0] / len, nn[1] / len, nn[2] / len], w: len * 0.5, a });
        }
        // one Softimage material per (object, slot actually used)
        let slots: Vec<Option<&str>> = ob["materials"].as_array().into_iter().flatten().map(|v| v.as_str()).collect();
        let mut used: Vec<i16> = pmat.clone();
        used.sort();
        used.dedup();
        let mut mats = Vec::new();
        let mut remap = std::collections::HashMap::new();
        let mi = models.len();
        for &u in &used {
            let bm = slots.get(u as usize).copied().flatten().and_then(|n| sc["materials"].get(n));
            let mname = format!("mat{}", mat_elems.len() + 1);
            mats.push(material_for(&mname, bm));
            std::fs::write(dir("MATERIALS")?.join(format!("{prefix}-{mname}.1-0.mtr")), si_hrc::write_material(mats.last().unwrap(), si_file::VERSION_OUTPUT)).map_err(|e| e.to_string())?;
            remap.insert(u, mats.len() as i16 - 1);
            model_mats.push((mi, mat_elems.len()));
            mat_elems.push(format!("{prefix}-{mname}.1-0"));
            mat_src.push(slots.get(u as usize).copied().flatten().map(String::from));
            // Blender's Backface Culling is per material, the program's BACK_CULLING one switch for the picture (unit finish1)
            if bm.and_then(|m| m["backface_culling"].as_bool()) == Some(true) {
                cull_on += 1;
            } else {
                cull_off.push(slots.get(u as usize).copied().flatten().unwrap_or("(no material)").to_string());
            }
            // the Softimage Material's local textures (relation 401) and 3D texture (501), one set per material element
            if let Some(s) = bm.and_then(|m| m.get("si")).filter(|s| s.is_object()) {
                let me = mat_elems.len() - 1;
                for t in s["textures"].as_array().into_iter().flatten() {
                    let Some(raw) = t["raw"].as_str() else { continue };
                    let pic = match tex_pics.iter().position(|n| n == &pic_name(raw)) {
                        Some(i) => i,
                        None => {
                            write_pic(raw, &dir("PICTURES")?.join(format!("{}.pic", pic_name(raw))))?;
                            tex_pics.push(pic_name(raw));
                            tex_pics.len() - 1
                        }
                    };
                    let tn = tex_elems.len() + 1;
                    let mut t2 = si_hrc::Texture2D::default();
                    t2.name = tex_pics[pic].as_bytes().to_vec();
                    t2.filename = format!("t2d{tn}").into_bytes();
                    crate::simat::apply_texture(&mut t2, &t["si"]);
                    let el = format!("{prefix}-t2d{tn}.1-0");
                    std::fs::write(dir("TEXTURES2D")?.join(format!("{el}.txt")), si_hrc::write_texture(&t2, si_file::VERSION_OUTPUT)).map_err(|e| e.to_string())?;
                    mat_tex.push((me, tex_elems.len()));
                    tex_elems.push(el);
                }
                if let Some(p) = s.get("texture3d").filter(|v| v.is_object()) {
                    let el = format!("{prefix}-t3d{}.1-0", t3d_elems.len() + 1);
                    let mut t3 = crate::simat::texture3d(&format!("t3d{}", t3d_elems.len() + 1), p);
                    // the 3D texture follows the object (the original's is in the node's own space): the vertices here
                    // are in world space, so the object's matrix goes into the texture's (unit sky2)
                    // an object whose matrix has no exact form in the texture's (a non-uniform scaling under a
                    // rotation) keeps the texture in world space, as before
                    if let Some((a, b)) = world_to_object(&m) {
                        let mut t_obj = t3.clone();
                        match crate::simat::texture3d_object_space(&mut t_obj, &a, &b) {
                            Some(true) => {
                                t3 = t_obj;
                                report += &format!("texture3d t3d{} of {oname}: in the object's space\n", t3d_elems.len() + 1);
                            }
                            Some(false) => report += &format!("texture3d t3d{} of {oname}: kept in world space (no exact form: a non-uniform scaling under a rotation)\n", t3d_elems.len() + 1),
                            None => {}
                        }
                    }
                    std::fs::write(dir("TEXTURES3D")?.join(format!("{el}.t3D")), si_hrc::write_texture3d(&t3, si_file::VERSION_OUTPUT)).map_err(|e| e.to_string())?;
                    mat_t3d.push((me, t3d_elems.len()));
                    t3d_elems.push(el);
                }
            }
        }
        if mats.is_empty() {
            let mname = format!("mat{}", mat_elems.len() + 1);
            mats.push(material_for(&mname, None));
            std::fs::write(dir("MATERIALS")?.join(format!("{prefix}-{mname}.1-0.mtr")), si_hrc::write_material(mats.last().unwrap(), si_file::VERSION_OUTPUT)).map_err(|e| e.to_string())?;
            model_mats.push((mi, mat_elems.len()));
            mat_elems.push(format!("{prefix}-{mname}.1-0"));
            mat_src.push(None);
            remap.insert(0, 0);
        }
        for p in pmat.iter_mut() {
            *p = *remap.get(p).unwrap_or(&0);
        }
        // the first image texture of the used slots: one 2D texture on the model, by the corner UVs (method 4); then
        // that material's later Softimage Texture nodes of global scope with the UV method, which read the same
        // corner UVs (the importer makes a later projected texture UV only when its coordinates are the first's;
        // unit extra2: Torch_Fire's flame grid, flamme and its textured shadow fire_shade2)
        let texs: Vec<Value> = used
            .iter()
            .find_map(|&u| {
                let bm = slots.get(u as usize).copied().flatten().and_then(|n| sc["materials"].get(n))?;
                let all: Vec<&Value> = bm["textures"].as_array()?.iter().filter(|t| t["raw"].is_string()).collect();
                let first = (*all.first()?).clone();
                Some(std::iter::once(first).chain(all[1..].iter().filter(|t| t["si"]["method"].as_i64() == Some(4)).map(|t| (*t).clone())).collect())
            })
            .unwrap_or_default();
        for t in texs {
            let raw = t["raw"].as_str().unwrap_or("");
            let pic = match tex_pics.iter().position(|n| n == &pic_name(raw)) {
                Some(i) => i,
                None => {
                    write_pic(raw, &dir("PICTURES")?.join(format!("{}.pic", pic_name(raw))))?;
                    tex_pics.push(pic_name(raw));
                    tex_pics.len() - 1
                }
            };
            let tn = tex_elems.len() + 1;
            let mut t2 = si_hrc::Texture2D::default();
            t2.name = tex_pics[pic].as_bytes().to_vec();
            t2.filename = format!("t2d{tn}").into_bytes();
            t2.method = 4;
            t2.ambient = 1.0;
            t2.diffuse = 1.0;
            t2.specular = 0.0;
            t2.mask = 1;
            t2.channel = 2;
            // a Softimage Texture node of global scope, or the linked standard inputs the add-on carries (unit nodes1)
            if let Some(p) = t.get("si") {
                crate::simat::apply_texture(&mut t2, p);
            }
            let el = format!("{prefix}-t2d{tn}.1-0");
            std::fs::write(dir("TEXTURES2D")?.join(format!("{el}.txt")), si_hrc::write_texture(&t2, si_file::VERSION_OUTPUT)).map_err(|e| e.to_string())?;
            model_tex.push((mi, tex_elems.len()));
            tex_elems.push(el);
        }
        ntri += corners.len();
        let md = MeshData { name: oname.clone(), verts, polys: corners, poly_material: pmat, materials: mats };
        #[allow(unused_mut)]
        let mut node = mesh_node(&md);
        let el = format!("{prefix}-{oname}.1-0");
        // a deforming object (unit anim1): its sampled frames as shapes, world space, so no transformation curve
        // (the add-on's map "shapes": object name -> side file; parts of a split object share their object's)
        let shaped = sc["shapes"][ob["anim_name"].as_str().or(ob["name"].as_str()).unwrap_or("")]["path"].as_str();
        if let Some(sp) = shaped {
            let s = crate::shapes::read(sp)?;
            let tri_index: Option<Vec<usize>> = ob["tri_index"].as_array().map(|a| a.iter().map(|i| i.as_u64().unwrap_or(u64::MAX) as usize).collect());
            let side_tri: Vec<usize> = tri_of.iter().map(|&t| tri_index.as_ref().map_or(t, |m| m.get(t).copied().unwrap_or(usize::MAX))).collect();
            let edges = crate::shapes::edges_of(&md.polys);
            let own_n: Vec<Vec<[f32; 3]>> = md.polys.iter().map(|p| p.iter().map(|c| c.normal).collect()).collect();
            let frames = crate::shapes::definitions(&s, &md.polys, &side_tri).map_err(|e| format!("{oname}: {e}"))?;
            // the same edges hard at every time, so the renderer's vertex count does not change (shapes::hard_edges)
            let all_n: Vec<Vec<Vec<[f32; 3]>>> = std::iter::once(own_n.clone()).chain(frames.iter().map(|f| f.1.clone())).collect();
            let hard = crate::shapes::hard_edges(&md.polys, &all_n, &edges);
            if let Some(si_hrc::Definition::Mesh(m)) = node.definitions.first_mut() {
                for (e, h) in m.edges.iter_mut().zip(&hard) {
                    if *h {
                        e.flag |= 0x08;
                    }
                }
            }
            let own = crate::shapes::mesh_def(&md.verts, &md.polys, &own_n, &md.poly_material, &edges, &hard);
            let defs: Vec<_> = frames.iter().map(|(v, n)| crate::shapes::mesh_def(v, &md.polys, n, &md.poly_material, &edges, &hard)).collect();
            report += &format!("shape {oname}: {} of {} edges hard at every time\n", hard.iter().filter(|&&h| h).count(), hard.len());
            let se = format!("{prefix}-{oname}Shp.1-0");
            std::fs::write(dir("SHAPES")?.join(format!("{se}.shp")), crate::shapes::shape_file(own, defs)).map_err(|e| e.to_string())?;
            model_shape.push((models.len(), shape_elems.len()));
            anim_rel.push(("SHAPES", shape_elems.len(), anim_files.len(), 15010));
            anim_files.push((se.clone(), crate::shapes::shape_anim(&se, &s.frames, fps)));
            shape_elems.push(se);
            // the blur walk asks SI_modelMove, which looks for a curve among the model's own (transformation and
            // geometry) curves only, not the shape element's: a constant scaling curve (1, what the node has) makes it
            // ask, and the vertices' movement then decides as for any model
            if s.frames.len() > 1 {
                anim_rel.push(("MODELS", models.len(), anim_files.len(), 15000));
                anim_files.push((el.clone(), anim::ani_file("MODELS", &el, &[anim::Curve { ty: 1, vals: vec![1.0; s.frames.len()] }], &s.frames, fps)));
            }
            report += &format!("shape {oname}: {} frames {}..{}\n", s.frames.len(), s.frames.first().unwrap_or(&0), s.frames.last().unwrap_or(&0));
        }
        std::fs::write(dir("MODELS")?.join(format!("{el}.hrc")), si_hrc::write_model(&node)).map_err(|e| e.to_string())?;
        // motion: the vertices are baked at the still's frame, so the node's transformation at frame f is M_f M_still^-1
        if let (Some(fr), Some(inv), None) = (anim, anim::inverse(&m), shaped) {
            let mut mats: Vec<Option<M4>> = fr.iter().map(|f| f["objects"].get(ob["anim_name"].as_str().or(ob["name"].as_str()).unwrap_or("")).filter(|v| !v.is_null()).map(mat)).collect();
            // an object missing from some frames (a particle before its birth or after its death, an instance that comes
            // and goes): those frames hold the nearest sampled matrix, so the frames it is there move as sampled (unit anim1)
            if mats.iter().any(Option::is_some) && mats.iter().any(Option::is_none) {
                let have: Vec<usize> = (0..mats.len()).filter(|&i| mats[i].is_some()).collect();
                let filled: Vec<Option<M4>> = (0..mats.len()).map(|i| mats[i].or_else(|| have.iter().min_by_key(|&&j| (j as i64 - i as i64).abs()).and_then(|&j| mats[j]))).collect();
                mats = filled;
            }
            if mats.iter().all(Option::is_some) {
                let (mut sx, mut sy, mut sz, mut rx, mut ry, mut rz, mut tx, mut ty, mut tz) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
                for mf in mats.iter().flatten() {
                    let t = anim::to_si(&anim::mul(mf, &inv));
                    let (s, r, tr) = anim::srt(&t);
                    sx.push(s[0]); sy.push(s[1]); sz.push(s[2]);
                    rx.push(r[0]); ry.push(r[1]); rz.push(r[2]);
                    tx.push(tr[0]); ty.push(tr[1]); tz.push(tr[2]);
                }
                for v in [&mut rx, &mut ry, &mut rz] {
                    anim::unwrap(v);
                }
                let mut curves = anim::group([1, 2, 3], [sx, sy, sz]);
                curves.extend(anim::group([4, 5, 6], [rx, ry, rz]));
                curves.extend(anim::group([139, 140, 141], [tx, ty, tz]));
                if !curves.is_empty() {
                    anim_rel.push(("MODELS", models.len(), anim_files.len(), 15000));
                    anim_files.push((el.clone(), anim::ani_file("MODELS", &el, &curves, &frame_nums, fps)));
                }
            }
        }
        models.push(el);
        model_src.push(ob["source"].as_str().or(ob["name"].as_str()).unwrap_or("").to_string());
    }
    report += &format!("objects {} triangles {} materials {}\n", models.len(), ntri, mat_elems.len());
    if models.is_empty() {
        return Err("the scene has no mesh object to render".into());
    }

    // ---- material colours per frame (ambient and diffuse are written equal)
    if let Some(fr) = anim {
        for (mi, src) in mat_src.iter().enumerate() {
            let Some(src) = src else { continue };
            let vals: Vec<Option<[f32; 3]>> = fr.iter().map(|f| f["materials"].get(src.as_str()).filter(|v| v.is_array()).map(|v| col(&Value::Array(v.as_array().unwrap().iter().take(3).cloned().collect())))).collect();
            if vals.iter().all(Option::is_some) {
                let mut c = [vec![], vec![], vec![]];
                for v in vals.iter().flatten() {
                    for k in 0..3 {
                        c[k].push(v[k] as f64);
                    }
                }
                let mut curves = anim::group([48, 49, 50], c.clone());
                curves.extend(anim::group([45, 46, 47], c));
                if !curves.is_empty() {
                    let el = format!("{}Mat.1-0", mat_elems[mi].trim_end_matches(".1-0"));
                    anim_rel.push(("MATERIALS", mi, anim_files.len(), 15003));
                    anim_files.push((el, anim::ani_file("MATERIALS", &mat_elems[mi], &curves, &frame_nums, fps)));
                }
            }
        }
    }

    // ---- camera
    let mut cam = camera::Camera::perspective_default();
    let (cam_pos, cam_int);
    {
        let c = &sc["camera"];
        if c.is_null() {
            return Err("the scene has no camera".into());
        }
        let m = mat(&c["matrix_world"]);
        let pos = xform(&m, [0.0, 0.0, 0.0]);
        let fwd = norm([-m[0][2], -m[1][2], -m[2][2]]);
        let up = norm([m[0][1], m[1][1], m[2][1]]);
        // an orthographic camera (the renderer has none: every camera of the corpus is perspective) is a perspective camera
        // moved back by `back` along the view axis, its angle chosen so that the picture spans `ortho_scale` at the old place
        let ortho = c["type"].as_str() == Some("ORTHO");
        let back = if ortho { 2000.0 } else { 0.0 };
        let (pos_s, fwd_s, up_s) = (conv(pos), conv(fwd), conv(up));
        let pos_s = add(pos_s, fwd_s, -back);
        let dist = 10.0;
        let int_s = add(pos_s, fwd_s, dist);
        // roll: the angle from the level up (world Y) to the camera's up, about the view axis
        let level = norm(add([0.0, 1.0, 0.0], fwd_s, -fwd_s[1]));
        // (the renderer turns the view by minus the roll, SI_camera: a positive roll turns the camera's up from the level
        // up against the view axis; unit extra1, held against the program's own camera of CONSTRAINT-Camera_Object_Banking)
        let roll = dot(cross(up_s, level), fwd_s).atan2(dot(level, up_s));
        let aspect = w as f64 / h as f64;
        let lens = c["lens"].as_f64().unwrap_or(50.0);
        let sensor = c["sensor"].as_f64().unwrap_or(36.0);
        // Blender's sensor fit "auto": the sensor width spans the larger picture side
        let fov_h = if ortho { 2.0 * (c["ortho_scale"].as_f64().unwrap_or(6.0) / (2.0 * back)).atan() } else { 2.0 * (sensor / (2.0 * lens)).atan() };
        let fov_v = if aspect >= 1.0 { 2.0 * ((fov_h / 2.0).tan() / aspect).atan() } else { fov_h };
        cam.position = f32x3(pos_s);
        cam.interest = f32x3(int_s);
        cam.yaw_pitch_roll = [0.0, 0.0, roll as f32];
        cam.field_of_view = fov_v as f32;
        cam.format_aspect = aspect as f32;
        let clip = c["clip"].as_array().cloned().unwrap_or_default();
        cam.near = (clip.first().and_then(Value::as_f64).unwrap_or(0.1) + back) as f32;
        cam.far = (clip.get(1).and_then(Value::as_f64).unwrap_or(1000.0) + back) as f32;
        let ph = 2.0 * 50.0 * (fov_v / 2.0).tan();
        cam.proj_plane_size = [(ph * aspect) as f32, ph as f32];
        cam.proj_plane_distance = 50.0;
        // depth of field: the program's custom mode (2) with the zone and blur of Blender's thin lens: the near and far limits are
        // where the blur circle is half a pixel; the blur is the circle at four times the focus distance, in pixels
        if let (Some(focus), Some(fstop)) = (c["dof"]["focus"].as_f64(), c["dof"]["fstop"].as_f64()) {
            if !ortho && focus > 0.0 && fstop > 0.0 {
                let f = lens / 1000.0;
                let aperture = f / fstop;
                let sensor_m = sensor / 1000.0;
                let px_m = sensor_m / (if aspect >= 1.0 { w as f64 } else { h as f64 });
                let k = 0.5 * px_m * (focus - f) / (aperture * f);
                let coc_px = |d: f64| aperture * f / (focus - f) * (d - focus).abs() / d / px_m;
                let oof = 4.0 * focus;
                cam.depth_of_field = 2;
                cam.near_focus = (focus / (1.0 + k)) as f32;
                cam.far_focus = if k < 1.0 { (focus / (1.0 - k)).min(30000.0) as f32 } else { 30000.0 };
                cam.out_of_focus = oof as f32;
                cam.maximum_coc = coc_px(oof).clamp(1.0, 40.0) as f32;
                cam.focal_distance = focus as f32;
                skipped_notes.push(format!("depth of field: custom zone {:.2} to {:.2}, blur {:.1} pixels at {:.1}", cam.near_focus, cam.far_focus, cam.maximum_coc, oof));
            }
        } else if let (Some(mode), Some(v)) = (c["si_dof"]["mode"].as_i64(), c["si_dof"]["values"].as_array()) {
            // an imported scene's own depth of field (the importer keeps it on the camera): its mode and the seven values
            // si_initDof reads, focal length, focus distance, near, far, out of focus, aperture number, circle
            let v: Vec<f32> = v.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
            if mode != 0 && v.len() == 7 {
                cam.depth_of_field = mode as i32;
                (cam.focal_length, cam.focal_distance, cam.near_focus, cam.far_focus) = (v[0], v[1], v[2], v[3]);
                (cam.out_of_focus, cam.f_stop, cam.maximum_coc) = (v[4], v[5], v[6]);
            }
        }
        cam_pos = pos_s;
        cam_int = int_s;
        if c["type"].as_str() == Some("ORTHO") {
            skipped_notes.push("camera is orthographic: drawn as a perspective camera 2000 units back with a narrow angle (the renderer has no orthographic projection)".into());
        }
    }
    let _ = (cam_pos, cam_int);
    if let Some(fr) = anim {
        // the camera's position and interest (ten units ahead), per frame
        let ms: Vec<Option<M4>> = fr.iter().map(|f| Some(f["camera"]["matrix_world"].clone()).filter(|v| !v.is_null()).map(|v| mat(&v))).collect();
        if ms.iter().all(Option::is_some) {
            let (mut p, mut i) = ([vec![], vec![], vec![]], [vec![], vec![], vec![]]);
            for m in ms.iter().flatten() {
                let ps = conv(xform(m, [0.0; 3]));
                let is = add(ps, conv(norm([-m[0][2], -m[1][2], -m[2][2]])), 10.0);
                for k in 0..3 {
                    p[k].push(ps[k]);
                    i[k].push(is[k]);
                }
            }
            let mut curves = anim::group([142, 143, 144], p);
            curves.extend(anim::group([145, 146, 147], i));
            // lens animation: the field of view (curve type 12, degrees, vertical like the camera file's) per frame; the picture's
            // aspect and the sensor are the still's
            let lens_f: Vec<f64> = fr.iter().map(|f| f["camera"]["lens"].as_f64().unwrap_or(0.0)).collect();
            if sc["camera"]["type"].as_str() != Some("ORTHO") && lens_f.iter().all(|l| *l > 0.0) && lens_f.iter().any(|l| (l - lens_f[0]).abs() > 1e-6) {
                let sensor = sc["camera"]["sensor"].as_f64().unwrap_or(36.0);
                let aspect = w as f64 / h as f64;
                let fov: Vec<f64> = lens_f
                    .iter()
                    .map(|l| {
                        let fov_h = 2.0 * (sensor / (2.0 * l)).atan();
                        let fov_v = if aspect >= 1.0 { 2.0 * ((fov_h / 2.0).tan() / aspect).atan() } else { fov_h };
                        fov_v.to_degrees()
                    })
                    .collect();
                curves.push(anim::Curve { ty: 12, vals: fov });
            }
            if !curves.is_empty() {
                let el = format!("{prefix}-cam_int1Cam.1-0");
                anim_rel.push(("CAMERAS", 0, anim_files.len(), 15007));
                anim_files.push((el, anim::ani_file("CAMERAS", &format!("{prefix}-cam_int1.1-0"), &curves, &frame_nums, fps)));
            }
        }
    }
    let mut wr = Writer::new();
    camera::write_file(&mut wr, &cam);
    std::fs::write(dir("CAMERAS")?.join(format!("{prefix}-cam_int1.1-0.cam")), &wr.out).map_err(|e| e.to_string())?;

    // ---- lights: (root file element, optional non-root element, relation)
    let mut light_roots: Vec<String> = Vec::new();
    let mut light_other: Vec<String> = Vec::new(); // spot's non-root element, listed after
    let mut light_rel: Vec<(usize, usize)> = Vec::new();
    let mut light_models: Vec<(usize, usize)> = Vec::new(); // (light root element, model): relation 2200
    let mut nl = 0;
    let wcol = if sc["world"].is_null() { [0.0; 3] } else { f3(&sc["world"]) };
    let amb_fit = fit::fit_ambient(wcol, &samples, cam_pos);
    for l in sc["lights"].as_array().into_iter().flatten() {
        nl += 1;
        let m = mat(&l["matrix_world"]);
        let pos = conv(xform(&m, [0.0, 0.0, 0.0]));
        let fwd = conv(norm([-m[0][2], -m[1][2], -m[2][2]]));
        let shadow = l["shadow"].as_bool().unwrap_or(true);
        let energy = l["energy"].as_f64().unwrap_or(1000.0);
        let lcol = f3(&l["color"]);
        let kind_s = l["kind"].as_str().unwrap_or("POINT");
        // the spot's Softimage cone and spread from Blender's size and blend: Cycles' mask rises from cos(size/2) to
        // cos(size/2) + blend (1 - cos(size/2)); Softimage's factor is linear in the cosine between the outer cosine
        // cos((cone + spread) / 2) and the inner cosine cos(cone / 2): outer = size, inner = acos of the upper end.
        let size = l["spot_size"].as_f64().unwrap_or(0.785).clamp(0.01, 3.1);
        let blend = l["spot_blend"].as_f64().unwrap_or(0.15).clamp(0.0, 1.0);
        let cos_o = (size / 2.0).cos();
        let cos_i = cos_o + blend * (1.0 - cos_o);
        let cone = 2.0 * cos_i.clamp(-1.0, 1.0).acos();
        let spread = (size - cone).max(0.0);
        let kind = match kind_s {
            "SUN" => fit::Kind::Sun,
            "SPOT" => fit::Kind::Spot { cos_outer: cos_o, cos_inner: cos_i },
            "AREA" => fit::Kind::Area,
            _ => fit::Kind::Point,
        };
        let fit = fit::fit_light(kind, lcol, energy, pos, fwd, &samples, cam_pos, wcol, amb_fit);
        // a light that File > Import > Softimage 3D scene made and nobody changed since: export.py hands over the
        // scene's own values ("si", importer.softimage_light), which are written as they are instead of the colour
        // and fall-off fitted to Cycles' look of the Blender light (that fit makes imported scenes brighter; units
        // import2, finish1). A light without "si" (every Blender scene's) is fitted as before.
        let si = l.get("si").filter(|v| v.is_object());
        let si_f = |k: &str, d: f64| si.and_then(|s| s[k].as_f64()).unwrap_or(d);
        let colour = match si {
            Some(s) => f32x3(f3(&s["color"])),
            None => [fit.colour[0] as f32, fit.colour[1] as f32, fit.colour[2] as f32],
        };
        let (fs, fe) = match si {
            Some(_) => (si_f("falloff_start", 10000.0) as f32, si_f("falloff_end", 10000.0) as f32),
            None => (fit.start as f32, fit.end as f32),
        };
        let (cone, spread) = if si.is_some() && kind_s == "SPOT" { (si_f("cone", cone), si_f("spread", spread)) } else { (cone, spread) };
        // the scene's umbra (SHADOW_FACTOR, +0x90); None keeps the record's default 0.75
        let umbra = si.and_then(|s| s["umbra"].as_f64()).map(|u| u as f32);
        if si.is_some() {
            let kind_of = si.and_then(|s| s["shadow"].as_i64()).map_or(String::new(), |k| format!(", shadow kind {k}"));
            report += &format!("light {} {kind_s}: the Softimage scene's own values (imported, unchanged), colour {:.3} {:.3} {:.3} fall-off {} {}{kind_of}\n", l["name"].as_str().unwrap_or("?"), colour[0], colour[1], colour[2], fs, fe);
        } else {
            report += &format!("light {} {kind_s}: {} triangles fitted, colour {:.3} {:.3} {:.3} fall-off {} {}{}\n", l["name"].as_str().unwrap_or("?"), fit.samples_used, colour[0], colour[1], colour[2], fit.start, fit.end, if shadow { ", ray-traced shadows" } else { "" });
        }
        let root_at = light_roots.len();
        // Blender light linking (export.py: "lit", the objects the light lights, or "unlit", the ones it does not):
        // a selective light, its root element related to the models listed (2200; the original's spots relate the
        // interest's root file too, Torch_Fire). Shadows follow the same selection in the renderer.
        let sel = match (l["lit"].as_array(), l["unlit"].as_array()) {
            (Some(names), _) => Some((1, names)),
            (None, Some(names)) => Some((0, names)),
            _ => None,
        };
        let sel = sel.map(|(v, names)| {
            let want: std::collections::BTreeSet<&str> = names.iter().filter_map(|n| n.as_str()).collect();
            let hit: Vec<usize> = (0..models.len()).filter(|&i| want.contains(model_src[i].as_str())).collect();
            report += &format!("light {}: selective, {} {} model(s)\n", l["name"].as_str().unwrap_or("?"), if v == 1 { "lights only" } else { "lights all but" }, hit.len());
            light_models.extend(hit.into_iter().map(|i| (root_at, i)));
            v
        });
        match kind_s {
            "SUN" => {
                let el = format!("{prefix}-inf{nl}.1-0");
                // an infinite light has no place: its position field is the direction towards the light, from the origin; the interest is not read (finding 0012, si_lights step 1)
                let dir_to = [-fwd[0] * 100.0, -fwd[1] * 100.0, -fwd[2] * 100.0];
                let b = light_file(3, &format!("inf{nl}"), colour, dir_to, add(dir_to, fwd, 10.0), 0.5236, 0.0873, (fs, fe), shadow, None, umbra, sel, si);
                std::fs::write(dir("LIGHTS")?.join(format!("{el}.lig")), b).map_err(|e| e.to_string())?;
                light_roots.push(el);
            }
            "SPOT" | "AREA" => {
                // a spot is two elements: the light (inside the interest's file) and its interest (root)
                let (cone, spread) = if kind_s == "AREA" { (0.0, std::f64::consts::PI) } else { (cone, spread) };
                let el = format!("{prefix}-spot{nl}_int.1-0");
                // soft depth-map shadows (shadow type 4) were tried for area lights: a faint blur, rmse no better than ray traced
                // shadows (unit blender3, re/out/lead/blender/matrix.md), so area lights keep type 2
                let soft = None;
                let b = light_file(2, &format!("spot{nl}"), colour, pos, add(pos, fwd, 10.0), cone as f32, spread as f32, (fs, fe), shadow, soft, umbra, sel, si);
                std::fs::write(dir("LIGHTS")?.join(format!("{el}.lig")), b).map_err(|e| e.to_string())?;
                light_roots.push(el);
                light_other.push(format!("{prefix}-spot{nl}.1-0"));
                light_rel.push((light_roots.len() - 1, light_other.len() - 1));
                if kind_s == "AREA" {
                    skipped_notes.push(format!("area light {}: rendered as a cosine-lobe spot light (no area shape, no soft shadows)", l["name"].as_str().unwrap_or("?")));
                }
            }
            _ => {
                let el = format!("{prefix}-light{nl}.1-0");
                let b = light_file(1, &format!("light{nl}"), colour, pos, [0.0; 3], 0.5236, 0.0873, (fs, fe), shadow, None, umbra, sel, si);
                std::fs::write(dir("LIGHTS")?.join(format!("{el}.lig")), b).map_err(|e| e.to_string())?;
                light_roots.push(el);
            }
        }
        // motion: position (a sun's is its direction), a spot's interest, and the colour scaled by the energy and colour change
        if let Some(fr) = anim {
            let ls: Vec<Option<&Value>> = fr.iter().map(|f| f["lights"].get(l["name"].as_str().unwrap_or("")).filter(|v| !v.is_null())).collect();
            if ls.iter().all(Option::is_some) && root_at < light_roots.len() {
                let (mut p, mut i, mut c, mut fe_v) = ([vec![], vec![], vec![]], [vec![], vec![], vec![]], [vec![], vec![], vec![]], vec![]);
                // fit_light is a function of (colour, energy, position, direction) here (samples, camera, world and ambience are
                // the still's): a frame whose light has the same bits as the last one fitted takes that fit again. A 250-frame
                // range with a still light refitted over 16,000 triangles each frame took 0.12 s of blend2db's 0.22 s (unit speed2).
                let key = |col: [f64; 3], e: f64, ps: [f64; 3], fw: [f64; 3]| -> [u64; 10] {
                    let v = [col[0], col[1], col[2], e, ps[0], ps[1], ps[2], fw[0], fw[1], fw[2]];
                    v.map(f64::to_bits)
                };
                let mut last = (key(lcol, energy, pos, fwd), fit.colour, fit.end);
                for lf in ls.iter().flatten() {
                    let m = mat(&lf["matrix_world"]);
                    let ps = conv(xform(&m, [0.0; 3]));
                    let fw = conv(norm([-m[0][2], -m[1][2], -m[2][2]]));
                    let (pp, ii) = if kind_s == "SUN" { ([-fw[0] * 100.0, -fw[1] * 100.0, -fw[2] * 100.0], [0.0; 3]) } else { (ps, add(ps, fw, 10.0)) };
                    let e = lf["energy"].as_f64().unwrap_or(energy);
                    // colour and fall-off end are fitted again for the frame's light (on the still's geometry)
                    let lc = f3(&lf["color"]);
                    let k_now = key(lc, e, ps, fw);
                    // the frame's Softimage values when the importer's light is as imported at that frame
                    let si_now = lf.get("si").filter(|v| v.is_object());
                    if si_now.is_none() && k_now != last.0 {
                        let ft = fit::fit_light(kind, lc, e, ps, fw, &samples, cam_pos, wcol, amb_fit);
                        last = (k_now, ft.colour, ft.end);
                    }
                    let (cc, ee) = match si_now {
                        Some(s) => (f3(&s["color"]), s["falloff_end"].as_f64().unwrap_or(10000.0)),
                        None => (last.1, last.2),
                    };
                    for k in 0..3 {
                        p[k].push(pp[k]);
                        i[k].push(ii[k]);
                        c[k].push(cc[k]);
                    }
                    fe_v.push(ee);
                }
                let mut curves = anim::group([22, 23, 24], c);
                curves.extend(anim::group([148, 149, 150], p));
                if kind_s != "SUN" && kind_s != "POINT" {
                    curves.extend(anim::group([151, 152, 153], i));
                }
                if fe_v.iter().any(|x| (x - fe_v[0]).abs() > 1e-6 * (1.0 + fe_v[0].abs())) {
                    curves.push(anim::Curve { ty: 29, vals: fe_v });
                }
                if !curves.is_empty() {
                    let root = &light_roots[root_at];
                    anim_rel.push(("LIGHTS", root_at, anim_files.len(), 15008));
                    anim_files.push((format!("{}Lig", root.trim_end_matches(".1-0")) + ".1-0", anim::ani_file("LIGHTS", root, &curves, &frame_nums, fps)));
                }
            }
        }
    }

    if light_roots.is_empty() {
        // a scene with no light gets the program's default light (a white point light at 100 100 100): a black one replaces it
        let el = format!("{prefix}-light1.1-0");
        let b = light_file(1, "light1", [0.0; 3], [0.0, 100.0, 0.0], [0.0; 3], 0.5236, 0.0873, (10000.0, 10000.0), false, None, None, None, None);
        std::fs::write(dir("LIGHTS")?.join(format!("{el}.lig")), b).map_err(|e| e.to_string())?;
        light_roots.push(el);
        skipped_notes.push("no light in the scene: a black light stands in for the program's default light".into());
    }
    report += &format!("lights {}\n", light_roots.len());

    // ---- scene description (text in the corpus's form, read back by the dsc reader as a check)
    let mut t = String::from("SDSC3.704\nELEMENTS \n");
    if !anim_files.is_empty() {
        let _ = writeln!(t, "    CHAPTER ANIMATION NBELEM {}     ", anim_files.len());
        for (e, _) in &anim_files {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
        for (e, bytes) in &anim_files {
            std::fs::write(dir("ANIMATION")?.join(format!("{e}.ani")), bytes).map_err(|e| e.to_string())?;
        }
    }
    t += "    CHAPTER CAMERAS NBELEM 2     \n";
    t += &format!("       {prefix}-cam_int1.1-0 ROOT ; \n       {prefix}-Camera1.1-0 ; \n    EndOfCHAPTER \n\n");
    // lights: roots first (indices 0..), then the non-root spot elements
    if !light_roots.is_empty() {
        let _ = writeln!(t, "    CHAPTER LIGHTS NBELEM {}     ", light_roots.len() + light_other.len());
        for e in &light_roots {
            let _ = writeln!(t, "       {e} ROOT ; ");
        }
        for e in &light_other {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    let _ = writeln!(t, "    CHAPTER MATERIALS NBELEM {}     ", mat_elems.len());
    for e in &mat_elems {
        let _ = writeln!(t, "       {e} ; ");
    }
    t += "    EndOfCHAPTER \n\n";
    let _ = writeln!(t, "    CHAPTER MODELS NBELEM {}     ", models.len());
    for e in &models {
        let _ = writeln!(t, "       {e} ROOT ; ");
    }
    t += "    EndOfCHAPTER \n\n";
    if !tex_pics.is_empty() {
        let _ = writeln!(t, "    CHAPTER PICTURES NBELEM {}     ", tex_pics.len());
        for e in &tex_pics {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    t += &format!("    CHAPTER SETUP_SOFT NBELEM 1     \n       {prefix}.1-0 ; \n    EndOfCHAPTER \n\n");
    if !shape_elems.is_empty() {
        let _ = writeln!(t, "    CHAPTER SHAPES NBELEM {}     ", shape_elems.len());
        for e in &shape_elems {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !tex_elems.is_empty() {
        let _ = writeln!(t, "    CHAPTER TEXTURES2D NBELEM {}     ", tex_elems.len());
        for e in &tex_elems {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !t3d_elems.is_empty() {
        let _ = writeln!(t, "    CHAPTER TEXTURES3D NBELEM {}     ", t3d_elems.len());
        for e in &t3d_elems {
            let _ = writeln!(t, "       {e} ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    t += &format!("EndOfELEMENTS \n\nRELATIONS \n");
    t += "    CHAPTER MODELS CHAPTER MATERIALS \n";
    for (mi, ei) in &model_mats {
        let _ = writeln!(t, "       {mi} {ei} 300 ; ");
    }
    t += "    EndOfCHAPTER \n\n";
    if !model_tex.is_empty() {
        t += "    CHAPTER MODELS CHAPTER TEXTURES2D \n";
        for (mi, ei) in &model_tex {
            let _ = writeln!(t, "       {mi} {ei} 400 ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !mat_tex.is_empty() {
        t += "    CHAPTER MATERIALS CHAPTER TEXTURES2D \n";
        for (mi, ei) in &mat_tex {
            let _ = writeln!(t, "       {mi} {ei} 401 ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !mat_t3d.is_empty() {
        t += "    CHAPTER MATERIALS CHAPTER TEXTURES3D \n";
        for (mi, ei) in &mat_t3d {
            let _ = writeln!(t, "       {mi} {ei} 501 ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !model_shape.is_empty() {
        t += "    CHAPTER MODELS CHAPTER SHAPES \n";
        for (mi, si) in &model_shape {
            let _ = writeln!(t, "       {mi} {si} 200 ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    for ch in ["MODELS", "CAMERAS", "LIGHTS", "MATERIALS", "SHAPES"] {
        if anim_rel.iter().any(|r| r.0 == ch) {
            let _ = writeln!(t, "    CHAPTER {ch} CHAPTER ANIMATION ");
            for (_, from, to, id) in anim_rel.iter().filter(|r| r.0 == ch) {
                let _ = writeln!(t, "       {from} {to} {id} ; ");
            }
            t += "    EndOfCHAPTER \n\n";
        }
    }
    t += "    CHAPTER CAMERAS CHAPTER CAMERAS \n       1 0 1110 ; \n    EndOfCHAPTER \n\n";
    if !light_rel.is_empty() {
        t += "    CHAPTER LIGHTS CHAPTER LIGHTS \n";
        for (root, other) in &light_rel {
            // the spot (non-root, listed after the roots) points at its interest (the root)
            let _ = writeln!(t, "       {} {} 2110 ; ", light_roots.len() + other, root);
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !light_models.is_empty() {
        t += "    CHAPTER LIGHTS CHAPTER MODELS \n";
        for (li, mi) in &light_models {
            let _ = writeln!(t, "       {li} {mi} 2200 ; ");
        }
        t += "    EndOfCHAPTER \n\n";
    }
    t += "EndOfRELATIONS \n\nENVIRONMENT \n";
    t += "    CHAPTER CAMERAS \n       1 SCHEM 0 -2 0 MPRFLG 0 ; \n       0 SCHEM 0 0 0 MPRFLG 0 ; \n    EndOfCHAPTER \n\n";
    if !light_roots.is_empty() {
        t += "    CHAPTER LIGHTS \n";
        for i in 0..light_roots.len() + light_other.len() {
            let _ = writeln!(t, "       {i} SCHEM {} 0 0 MPRFLG 0 ; ", 12.5 + 2.5 * i as f32);
        }
        t += "    EndOfCHAPTER \n\n";
    }
    t += "    CHAPTER MODELS \n";
    for i in 0..models.len() {
        let _ = writeln!(t, "       {i} SCHEM {} 0 0 SRT 1 1 1 0 0 0 0 0 0 MPRFLG 0 ; ", 2.5 * (i + 1) as f32);
    }
    t += "    EndOfCHAPTER \n\n    CHAPTER MATERIALS \n";
    for i in 0..mat_elems.len() {
        let _ = writeln!(t, "       {i} SCHEM {} -4 0 MPRFLG 0 ; ", 2.5 * (i + 1) as f32);
    }
    t += "    EndOfCHAPTER \n\n";
    if !tex_elems.is_empty() {
        t += "    CHAPTER TEXTURES2D \n";
        for i in 0..tex_elems.len() {
            let _ = writeln!(t, "       {i} SCHEM {} -6 0 MPRFLG 0 ; ", 2.5 * (i + 1) as f32);
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !t3d_elems.is_empty() {
        t += "    CHAPTER TEXTURES3D \n";
        for i in 0..t3d_elems.len() {
            let _ = writeln!(t, "       {i} SCHEM {} -8 0 MPRFLG 0 ; ", 2.5 * (i + 1) as f32);
        }
        t += "    EndOfCHAPTER \n\n";
    }
    if !anim_files.is_empty() {
        t += "    CHAPTER ANIMATION \n";
        for i in 0..anim_files.len() {
            let _ = writeln!(t, "       {i} SCHEM {} 0 0 ; ", 2.5 * i as f32);
        }
        t += "    EndOfCHAPTER \n\n";
    }
    let _ = write!(t, "    PLAYCTRL \n       FRAME {first} {last} 1 {first} \n       PAUSE 0 \n       RATE {} \n    EndOfPLAYCTRL \nEndOfENVIRONMENT \n", fps.round() as i64);
    let (d, info) = si_ascii::dsc::read(t.as_bytes()).map_err(|e| format!("own scene description does not read: {e:?}"))?;
    if !info.sections_closed || !info.all_tokens_consumed {
        return Err("own scene description not read to its end".into());
    }
    let nrel: usize = d.relations.iter().map(|b| b.relations.len()).sum();
    report += &format!("description: {} relations\n", nrel);
    std::fs::write(dir("SCENES")?.join(format!("{prefix}.1-0.dsc")), t.as_bytes()).map_err(|e| e.to_string())?;

    // ---- set-up from the template
    let ts = std::fs::read(tdb.join("SETUP_SOFT").join(format!("{tscene}.sts"))).map_err(|e| format!("template set-up: {e}"))?;
    let amb = {
        let wc = f3(&sc["world"]);
        // a Softimage World node's ambience replaces the fitted one in the set-up (the lights stay as fitted with the
        // fitted one, so the ambience as set adds to or takes from the picture)
        let a = match sc["render"]["ambience"].as_array() {
            Some(v) if v.len() >= 3 => [0, 1, 2].map(|k| v[k].as_f64().unwrap_or(0.0) as f32 as f64),
            _ => amb_fit,
        };
        report +=&format!("ambience {:.3} {:.3} {:.3} (world {:.3} {:.3} {:.3})\n", a[0], a[1], a[2], wc[0], wc[1], wc[2]);
        [a[0] as f32, a[1] as f32, a[2] as f32]
    };
    // ray depth: a transparent (refracting) material needs the ray in, through and out of the body: 4; mirrors: 2
    let refracts = sc["materials"].as_object().into_iter().flat_map(|m| m.values()).any(|m| m["transmission"].as_f64().unwrap_or(0.0) > 0.0 || m["alpha"].as_f64().unwrap_or(1.0) < 1.0 || m["si"]["transparency"].as_f64().unwrap_or(0.0) > 0.0
        // a picture's alpha as the opacity (unit nodes1: Image Alpha -> Alpha, or a Softimage Texture driving it)
        || m["textures"].as_array().into_iter().chain(m["si"]["textures"].as_array()).flatten().any(|t| t["si"]["transparency"].as_f64().unwrap_or(0.0) != 0.0));
    let ray_depth = sc["render"]["ray_depth"].as_i64().map(|d| d as i32).unwrap_or(if refracts { 4 } else { 2 });
    let mut extra: Vec<(String, String)> = Vec::new();
    for (k, v) in sc["render"]["setup"].as_object().into_iter().flatten() {
        extra.push((k.clone(), v.as_str().map(String::from).unwrap_or_else(|| v.to_string())));
    }
    // back-face culling when every material drawn culls (Material > Settings > Backface Culling; an import of a scene
    // whose set-up culls sets it on all its materials); a scene that mixes them is drawn without culling, as before
    let back_culling = cull_on > 0 && cull_off.is_empty();
    if back_culling {
        report += "back culling: ACTIVE (every material has Backface Culling)\n";
    } else if cull_on > 0 {
        cull_off.sort();
        cull_off.dedup();
        skipped_notes.push(format!("back-face culling: {cull_on} drawn material slots cull, {} materials not ({}); the program culls the whole picture or nothing: off", cull_off.len(), cull_off.join(", ")));
    }
    let patched = patch_setup(&String::from_utf8_lossy(&ts), first, last, w, h, amb, "BLENDER", ray_depth, back_culling, &extra);
    si_ascii::setup::read(patched.as_bytes()).map_err(|e| format!("patched set-up does not read: {e:?}"))?;
    std::fs::write(dir("SETUP_SOFT")?.join(format!("{prefix}.1-0.sts")), patched.as_bytes()).map_err(|e| e.to_string())?;
    report += &format!("picture {w}x{h} frame {frame} -> BLENDER.{frame}.pic\n");
    for s in &skipped_notes {
        report += &format!("not carried: {s}\n");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn world_to_object_inverts_the_bake() {
        // a Blender object matrix (rotation, non-uniform scaling, translation) and points in the object's space
        let (c, s) = (0.6f64.cos(), 0.6f64.sin());
        let m: super::M4 = [[2.0 * c, -3.0 * s, 0.0, 1.5], [2.0 * s, 3.0 * c, 0.0, -2.0], [0.0, 0.0, 0.5, 4.0], [0.0, 0.0, 0.0, 1.0]];
        let (a, b) = super::world_to_object(&m).unwrap();
        for p in [[1.0, 2.0, 3.0], [-0.5, 0.0, 7.0]] {
            let world = super::conv(super::xform(&m, p));
            let obj: [f64; 3] = core::array::from_fn(|i| (0..3).map(|j| a[i][j] * world[j]).sum::<f64>() + b[i]);
            let want = super::conv(p);
            for k in 0..3 {
                assert!((obj[k] - want[k]).abs() < 1e-9, "{obj:?} {want:?}");
            }
        }
        let id: super::M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let (a, b) = super::world_to_object(&id).unwrap();
        assert_eq!(a, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(b.iter().all(|&x| x == 0.0));
    }
}
