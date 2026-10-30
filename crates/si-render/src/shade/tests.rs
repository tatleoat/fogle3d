// SPDX-License-Identifier: MIT
//! Tests of `shade.rs` on made-up data. The expected bits of the precision tests are worked
//! out by hand in the comments; they pin the Rust to the reading of the instruction
//! sequence, not the reading to the binary.

use super::*;
use crate::types::{RVertex, Ray, Rgbazo, Tn, Vector};

/// Stand-ins for what `SI_shade` calls outside its source file.
#[derive(Default)]
struct Env {
    /// what `si_trace` answers: `None` is a miss, otherwise the triangle and distance hit
    hit: Option<(usize, f32)>,
    traced: Vec<Ray>,
    shadow: [f32; 4],
    shadow_rays: Vec<(Ray, f32, MatWork)>,
    samples: [[f32; 5]; 3],
    looked_up: Vec<(f32, f32, usize, bool)>,
    applied: Vec<(usize, usize)>,
}

impl ShadeEnv for Env {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        7
    }
    fn render_txt3d(&mut self, st: &mut ShadeState, material: usize, _: Vector, normal: Option<&mut Vector>) {
        st.materials[material].work.diffuse[0] = 0.125;
        if let Some(n) = normal {
            n.x += 0.0;
        }
    }
    fn blookup(&mut self, _: &ShadeState, u: f32, v: f32, map: usize, bump: bool) -> [[f32; 5]; 3] {
        self.looked_up.push((u, v, map, bump));
        self.samples
    }
    fn render_txt2d(&mut self, st: &mut ShadeState, material: usize, map: usize, sample: [f32; 5]) {
        self.applied.push((material, map));
        st.materials[material].work.diffuse[..3].copy_from_slice(&sample[..3]);
    }
    fn s_trace(&mut self, st: &mut ShadeState, ray: &mut Ray, tn: usize, distance: f32, _: usize) -> [f32; 4] {
        self.shadow_rays.push((*ray, distance, st.materials[st.tns[tn].material as usize].work));
        self.shadow
    }
    fn trace(&mut self, _: &ShadeState, ray: &mut Ray, _: usize) -> bool {
        self.traced.push(*ray);
        match self.hit {
            Some((tn, t)) => {
                ray.tn = Some(tn);
                ray.t = t;
                ray.hit = Vector::new(0.5, 0.25, 0.25);
                true
            }
            None => false,
        }
    }
}

/// One triangle in the plane z = −5 facing the eye, one material, one model, no lights.
fn scene() -> ShadeState {
    let mut st = ShadeState::default();
    let n = Vector::new(0.0, 0.0, 1.0);
    for (i, w) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)].iter().enumerate() {
        st.verts.push(RVertex { world: Vector::new(w.0, w.1, -5.0), raw: Vector::new(w.0, w.1, 0.0), normal: n, uv: 2 * i as u32, ..RVertex::default() });
    }
    st.uvs = vec![[0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 1.0]];
    st.tns.push(Tn { v: [Some(0), Some(1), Some(2)], n, d: -5.0, inv_d: -0.2, axis: 3, inv_n: 1.0, material: 0, w58: 0, ..Tn::default() });
    let mut m = ShadeMaterial { model: 3, vcol_rgb: 1, vcol_alpha: 1, ..ShadeMaterial::default() };
    m.work.diffuse = [1.0, 1.0, 1.0, 1.0];
    m.work.opacity = 1.0;
    m.work.existence = 1.0;
    st.materials.push(m);
    st.models.push(ShadeModel::default());
    st.glb.background = Rgbazo { r: 0.1, g: 0.2, b: 0.3, a: 0.4, z: 0.0, o: 0.0 };
    st.glb.max_depth = 3;
    st
}

fn ray() -> Ray {
    Ray { origin: Vector::new(0.25, 0.25, 0.0), dir: Vector::new(0.0, 0.0, -1.0), t: 5.0, hit: Vector::new(0.5, 0.25, 0.25), tn: Some(0), ..Ray::default() }
}

fn infinite(dir: Vector) -> ShadeLight {
    ShadeLight { kind: 3, color: [1.0, 1.0, 1.0, 1.0], dir, on: 1, ..ShadeLight::default() }
}

const BG: Rgbazo = Rgbazo { r: 0.0, g: 0.0, b: 0.0, a: 0.0, z: 0.0, o: 0.0 };

#[test]
fn record_sizes() {
    assert_eq!(core::mem::size_of::<MatWork>(), 0x50);
    assert_eq!(core::mem::size_of::<Rgbazo>(), 24);
    assert_eq!(crate::data::POISSON_TABLE.len, 40 * 8);
}

#[test]
fn clamp_leaves_nan() {
    assert_eq!(clamp01(-0.5), 0.0);
    assert_eq!(clamp01(1.5), 1.0);
    assert_eq!(clamp01(0.25), 0.25);
    assert!(clamp01(f32::NAN).is_nan());
    // −0 is not below 0: it stays −0
    assert_eq!(clamp01(-0.0).to_bits(), 0x8000_0000);
}

#[test]
fn decay_table_lookup() {
    let mut t = DecayTable::default();
    t.pairs[100] = [0.25, 0.5];
    t.after = [0.75, 9.0];
    // c = 0.5025: 100.5 → entry 100, fraction 0.5 → 0.25 + 0.5 × 0.5. (0.5025 is not exact
    // in a float; the product with 200 is formed in the register, so compare loosely.)
    let v = t.lookup(x(0.5025f32)).to_f32();
    assert!((v - 0.5).abs() < 1e-5, "{v}");
    // c = 0.5 exactly: fraction 0
    assert_eq!(t.lookup(x(0.5f32)).to_f32(), 0.25);
    // a cosine of exactly 1 reads the pair after the table, with fraction 0
    assert_eq!(t.lookup(x(1.0f32)).to_f32(), 0.75);
}

#[test]
#[should_panic]
fn decay_table_beyond_the_record() {
    DecayTable::default().lookup(x(1.01f32));
}

#[test]
fn constant_model_is_the_diffuse_colour() {
    let mut st = scene();
    st.materials[0].model = 1;
    st.materials[0].work.diffuse = [0.25, 0.5, 2.0, 1.0];
    let out = shade(&mut st, &mut Env::default(), &mut ray(), Rgbazo { a: 0.5, ..BG }, 1);
    // alpha = (1 + 0.5) − 0.5 × 1 = 1; blue is clamped
    assert_eq!(out, Rgbazo { r: 0.25, g: 0.5, b: 1.0, a: 1.0, z: -5.0, o: 0.0 });
}

#[test]
fn weights_are_scaled_and_stored_back() {
    let mut st = scene();
    st.tns[0].inv_n = 2.0;
    let mut r = ray();
    shade(&mut st, &mut Env::default(), &mut r, BG, 0);
    assert_eq!(r.hit, Vector::new(1.0, 0.5, 0.5));
}

#[test]
fn lambert_keeps_the_cosine_in_the_register_and_blinn_does_not() {
    // normal (1, 2^-24, 0), light direction (1, 1/2, 0): normal · light = 1 + 2^-25, which no
    // float holds. diffuse 0.5, ambient 2^-25:
    //   register: 0.5 × (1 + 2^-25) + 2^-25 = 0.5 + 0.75 × 2^-24 → rounds up: 0x3f000001
    //   Blinn stores the first two products' sum to a float first (1.0):
    //             0.5 × 1 + 2^-25 = a tie → rounds to even: 0x3f000000
    for (model, bits) in [(3, 0x3f00_0001u32), (4, 0x3f00_0001), (5, 0x3f00_0000)] {
        let mut st = scene();
        st.verts[0].normal = Vector::new(1.0, f32::from_bits(0x3380_0000), 0.0);
        st.materials[0].model = model;
        st.materials[0].work.diffuse = [0.5, 0.5, 0.5, 1.0];
        st.materials[0].work.ambient = [f32::from_bits(0x3300_0000); 4];
        st.lights.push(infinite(Vector::new(1.0, 0.5, 0.0)));
        st.light_head = Some(0);
        let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
        assert_eq!(out.r.to_bits(), bits, "model {model}");
        assert_eq!(out.b.to_bits(), bits, "model {model}");
    }
}

#[test]
fn spot_edge_differs_between_lambert_and_phong() {
    // the light sits on the normal through the hit point and points straight at it: the
    // cosine is exactly 1, and so is the cone's outer cosine
    for (model, lit) in [(3, false), (4, true), (5, true)] {
        let mut st = scene();
        st.materials[0].model = model;
        st.materials[0].work.diffuse = [0.5, 0.5, 0.5, 1.0];
        st.lights.push(ShadeLight {
            kind: 2,
            color: [1.0; 4],
            pos: Vector::new(0.25, 0.25, -3.0),
            spot_dir: Vector::new(0.0, 0.0, -1.0),
            spread_cos: 1.0,
            cone_cos: 0.5,
            cone_div: 1.0,
            on: 1,
            ..ShadeLight::default()
        });
        st.light_head = Some(0);
        let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
        assert_eq!(out.r, if lit { 0.5 } else { 0.0 }, "model {model}");
    }
}

#[test]
fn light_list_tests() {
    let mut st = scene();
    st.materials[0].work.diffuse = [0.25, 0.25, 0.25, 1.0];
    let up = Vector::new(0.0, 0.0, 1.0);
    // 0: off; 1: selective, table does not match; 2: selective, matches; 3: a direction
    // of all zeros; 4: behind the surface
    st.lights.push(ShadeLight { on: 0, next: Some(1), ..infinite(up) });
    st.lights.push(ShadeLight { selective: 1, select_id: 5, select_table: vec![4], next: Some(2), ..infinite(up) });
    st.lights.push(ShadeLight { selective: 1, select_id: 5, select_table: vec![5], next: Some(3), ..infinite(up) });
    st.lights.push(ShadeLight { next: Some(4), ..infinite(Vector::new(0.00005, -0.00005, 0.0)) });
    st.lights.push(infinite(Vector::new(0.0, 0.0, -1.0)));
    st.light_head = Some(0);
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    assert_eq!(out.r, 0.25);
}

#[test]
fn falloff_and_point_light_distance() {
    let mut st = scene();
    st.materials[0].work.diffuse = [1.0; 4];
    // 2 units above the hit point; falls off between 1 and 5: factor 1 − (2 − 1) / 4
    st.lights.push(ShadeLight { kind: 1, color: [0.5; 4], pos: Vector::new(0.25, 0.25, -3.0), falloff: true, falloff_start: 1.0, falloff_end: 5.0, on: 1, ..ShadeLight::default() });
    st.light_head = Some(0);
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    assert_eq!(out.g, 0.375);
    // beyond the end: nothing
    st.lights[0].falloff_end = 1.5;
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).g, 0.0);
}

#[test]
fn ray_traced_shadow_sees_the_material_as_on_entry() {
    let mut st = scene();
    st.glb.shadows = 1;
    st.glb.textures = 1;
    st.materials[0].flags = flag::TEXTURED;
    st.materials[0].diffuse = [0.5, 0.5, 0.5, 1.0];
    st.materials[0].work.diffuse = [0.75; 4];
    st.lights.push(ShadeLight { shadow: 2, ..infinite(Vector::new(0.0, 0.0, 1.0)) });
    st.light_head = Some(0);
    let mut env = Env { shadow: [0.5, 0.25, 1.0, 0.0], ..Env::default() };
    let mut r = ray();
    r.w38 = 42;
    let out = shade(&mut st, &mut env, &mut r, BG, 0);
    // the working block was reset from the material (0.5), the light is cut per channel
    assert_eq!((out.r, out.g, out.b), (0.25, 0.125, 0.5));
    let (sray, dist, seen) = env.shadow_rays[0];
    assert_eq!(sray.origin, Vector::new(0.25, 0.25, -5.0));
    assert_eq!(sray.dir, Vector::new(0.0, 0.0, 1.0));
    assert_eq!(sray.w38, 42);
    assert_eq!(dist, 100000.0);
    assert_eq!(seen.diffuse, [0.75; 4]);
    // and the block is put back at the end
    assert_eq!(st.materials[0].work.diffuse, [0.75; 4]);
}

#[test]
fn depth_map_shadows() {
    let mut st = scene();
    st.glb.shadows = 1;
    // both matrices the identity with a translation: the hit point (0.25, 0.25, −5) lands
    // in cell (2, 2) at depth −5
    let mut m = si_math::types::IDENTITY;
    m[3][0] = 2.0;
    m[3][1] = 2.0;
    let mut lt = ShadeLight { shadow: 3, umbra: 0.25, depth_mat: si_math::types::IDENTITY, xy_mat: m, zmap: vec![vec![-4.0; 5]; 5], map_size: 5, ..infinite(Vector::new(0.0, 0.0, 1.0)) };
    lt.soft_size = 1;
    lt.soft_step = 1;
    lt.soft_scale = 1.0;
    st.lights.push(lt);
    st.light_head = Some(0);
    // deeper than the map's −4: in shadow, the light times the umbra
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).r, 0.25);
    // the soft kind: window x 1..4 and y 1..4 (the far limit is not looked at: three by three
    // cells); two of the nine cells are nearer than the point: 2/9 lit
    st.lights[0].shadow = 4;
    st.lights[0].zmap[1][1] = -6.0;
    st.lights[0].zmap[3][2] = -6.0;
    let f = ((x(2i32) * 1.0f32) / x(9i32) * (F80::ONE - 0.25f32) + 0.25f32).to_f32();
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).r, f);
    // nearer than the map: lit
    st.lights[0].zmap = vec![vec![-6.0; 5]; 5];
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).r, 1.0);
}

#[test]
fn phong_highlight_and_alpha() {
    let mut st = scene();
    st.materials[0].model = 4;
    st.materials[0].work.diffuse = [0.0; 4];
    st.materials[0].work.specular = [0.5, 0.25, 0.75, 0.0];
    st.materials[0].work.opacity = 0.5;
    st.materials[0].flags = 0x40;
    // light straight above: the half vector is the normal, the cosine exactly 1: the entry
    // after the table
    st.materials[0].spec_table.after = [0.5, 0.0];
    st.lights.push(infinite(Vector::new(0.0, 0.0, 1.0)));
    st.light_head = Some(0);
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    // colour 0.5 × spec, then blended over a black background by the opacity 0.5
    assert_eq!((out.r, out.g, out.b), (0.125, 0.0625, 0.1875));
    // alpha = 0.5 + ((0.5 + 0.25 + 0.75) / 3 × 0.5) × 0.5 = 0.625, then + (1 − a) × 0
    assert_eq!(out.a, 0.625);
    st.glb.no_spec_alpha = 1;
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).a, 0.5);
}

#[test]
fn blinn_highlight() {
    let mut st = scene();
    st.materials[0].model = 5;
    st.materials[0].work.diffuse = [0.0; 4];
    st.materials[0].work.specular = [0.5, 0.5, 0.5, 0.0];
    st.materials[0].spec_table.after = [0.5, 0.0];
    st.lights.push(infinite(Vector::new(0.0, 0.0, 1.0)));
    st.light_head = Some(0);
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    // n·h = n·v = v·h = n·l = 1: both geometry terms are 2, held to 1; 0.5 × 0.5 × 1 / 1
    assert_eq!(out.r, 0.25);
    // alpha = 1 + ((0.75 / 3) × 1) × 0.5, held to 1
    assert_eq!(out.a, 1.0);
}

#[test]
fn model_two_uses_the_first_corner_and_leaves_early() {
    let mut st = scene();
    st.materials[0].model = 2;
    st.materials[0].flags = flag::REFLECTS;
    st.glb.reflection = 1;
    st.glb.fade = 1;
    st.glb.fade_start = 0.0;
    st.glb.fade_end = 1.0;
    st.materials[0].work.diffuse = [0.5; 4];
    // the hit triangle faces away from the ray here, which model 2 does not care about
    st.verts[0].normal = Vector::new(0.0, 0.0, -1.0);
    st.lights.push(ShadeLight { kind: 0, dir: Vector::new(0.0, 0.0, -1.0), ..infinite(Vector::default()) });
    st.light_head = Some(0);
    let mut env = Env::default();
    let out = shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert_eq!(out.r, 0.5);
    assert!(env.traced.is_empty(), "no reflection ray in model 2");
}

#[test]
fn transparency_and_refraction() {
    let mut st = scene();
    st.materials[0].model = 1;
    st.materials[0].flags = 0x40;
    st.materials[0].work.diffuse = [1.0, 0.0, 0.0, 1.0];
    st.materials[0].work.opacity = 0.25;
    st.glb.refraction = 1;
    let bg = Rgbazo { r: 0.0, g: 1.0, b: 0.0, a: 0.5, z: 0.0, o: 0.0 };
    // a primary ray without flag 0x200: no refracted ray; blended over the background
    let mut env = Env::default();
    let out = shade(&mut st, &mut env, &mut ray(), bg, 1);
    assert!(env.traced.is_empty());
    assert_eq!((out.r, out.g), (0.25, 0.75));
    // alpha: (0.25 + 0.5) − 0.5 × 0.25 = 0.625, then 0.625 + 0.375 × 0.5
    assert_eq!(out.a, 0.8125);
    // a secondary ray: the refracted ray misses, the background becomes the global one
    let out = shade(&mut st, &mut env, &mut ray(), bg, 0);
    assert_eq!(env.traced.len(), 1);
    assert_eq!(out.g, (x(0.75f32) * 0.2f32).to_f32());
    // existence scales the opacity; below 0.0001 the background is the answer, clamped
    st.materials[0].work.existence = 0.0;
    let out = shade(&mut st, &mut env, &mut ray(), Rgbazo { r: 2.0, a: -1.0, z: 9.0, o: 8.0, ..bg }, 1);
    assert_eq!(out, Rgbazo { r: 1.0, g: 1.0, b: 0.0, a: 0.0, z: 9.0, o: 8.0 });
}

#[test]
fn refract_straight_through_and_total_reflection() {
    let mut st = scene();
    st.materials[0].work.refraction = 1.0;
    let n = Vector::new(0.0, 0.0, 1.0);
    let p = Vector::new(0.25, 0.25, -5.0);
    let mut env = Env::default();
    // entering (normal against the ray), index 1: the direction is unchanged, depth too
    let mut r = ray();
    r.dir = Vector::new(0.6, 0.0, -0.8);
    r.w3e = 1;
    assert_eq!(refract(&mut st, &mut env, &r, 0, p, n), st.glb.background);
    let t = env.traced[0];
    assert_eq!(t.w3e, 1);
    assert_eq!(t.origin, p);
    assert!((t.dir.x - 0.6).abs() < 1e-6 && (t.dir.z + 0.8).abs() < 1e-6, "{:?}", t.dir);
    // leaving (normal along the ray): depth + 1 and the ratio is 1 / index
    let mut env = Env::default();
    refract(&mut st, &mut env, &r, 0, p, Vector::new(0.0, 0.0, -1.0));
    assert_eq!(env.traced[0].w3e, 2);
    // leaving a dense material at a shallow angle (q = (0.75, 0, −1), k = 1.5625 / 4 −
    // 0.5625 < 0): total reflection → `reflect` with the negated normal, whose mirror ray
    // keeps the depth the ray came with + 1
    st.materials[0].work.refraction = 2.0;
    let mut env = Env::default();
    assert_eq!(refract(&mut st, &mut env, &r, 0, p, Vector::new(0.0, 0.0, -1.0)), st.glb.background);
    assert_eq!(env.traced.len(), 1);
    assert_eq!(env.traced[0].w3e, 2);
    assert!((env.traced[0].dir.z - 0.8).abs() < 1e-6, "{:?}", env.traced[0].dir);
    // too deep
    r.w3e = 4;
    assert_eq!(refract(&mut st, &mut Env::default(), &r, 0, p, n), st.glb.background);
}

#[test]
fn reflect_mirror_direction() {
    let mut st = scene();
    let n = Vector::new(0.0, 0.0, 1.0);
    let p = Vector::new(0.25, 0.25, -5.0);
    let mut r = ray();
    r.dir = Vector::new(0.6, 0.0, -0.8);
    r.w3e = 2;
    r.w3c = 9;
    r.w40 = 3.5f32.to_bits();
    let mut env = Env::default();
    assert_eq!(reflect(&mut st, &mut env, &r, 0, p, n, BG), st.glb.background);
    let t = env.traced[0];
    assert_eq!((t.w3e, t.w3c, f32::from_bits(t.w40)), (3, 9, 3.5));
    assert!((t.dir.x - 0.6).abs() < 1e-6 && (t.dir.z - 0.8).abs() < 1e-6, "{:?}", t.dir);
    // a ray coming from behind the normal: no mirror ray
    let mut env = Env::default();
    assert_eq!(reflect(&mut st, &mut env, &r, 0, p, Vector::new(0.0, 0.0, -1.0), BG), st.glb.background);
    assert!(env.traced.is_empty());
}

#[test]
fn reflect_environment_map() {
    let mut st = scene();
    st.camera.world_to_raw_mat = si_math::types::IDENTITY;
    st.maps.push(ShadeMap { method: 4, env_scale: 0.5, ..ShadeMap::default() });
    st.materials[0].reflect_map = Some(0);
    let mut env = Env { samples: [[0.5, 1.0, 0.25, 0.0, 0.0]; 3], ..Env::default() };
    let mut r = ray();
    r.dir = Vector::new(0.0, 0.0, -1.0);
    let out = reflect(&mut st, &mut env, &r, 0, Vector::default(), Vector::new(0.0, 0.0, 1.0), BG);
    // a map that is not of method 7: no ray is traced
    assert!(env.traced.is_empty());
    assert_eq!((out.r, out.g, out.b), (0.25, 0.5, 0.125));
    // mirror direction (0, 0, 1): azimuth atan2(1, 0) = π/2 → u = 0.75; y = 0 → v = 0.5
    let (u, v, map, bump) = env.looked_up[0];
    assert!((u - 0.75).abs() < 1e-6 && (v - 0.5).abs() < 1e-6, "{u} {v}");
    assert_eq!((map, bump), (0, false));
    // method 7 traces first; on a miss the map is still looked up
    st.maps[0].method = 7;
    let mut env = Env::default();
    reflect(&mut st, &mut env, &r, 0, Vector::default(), Vector::new(0.0, 0.0, 1.0), BG);
    assert_eq!((env.traced.len(), env.looked_up.len()), (1, 1));
}

#[test]
fn reflection_blend_and_fade() {
    let mut st = scene();
    st.materials[0].model = 1;
    st.materials[0].flags = 0x10;
    st.materials[0].work.diffuse = [1.0, 0.0, 0.5, 1.0];
    st.materials[0].work.reflectivity = 0.25;
    st.glb.reflection = 1;
    // the mirror ray misses: the global background (0.1, 0.2, 0.3)
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    assert_eq!(out.r, (x(0.25f32) * 0.1f32 + x(0.75f32) * 1.0f32).to_f32());
    assert_eq!(out.g, (x(0.75f32) * 0.0f32 + x(0.25f32) * 0.2f32).to_f32());
    // fade: the ray's length 5 between 4 and 8 with 1/8 per unit: 1/8 of the fade colour
    st.materials[0].flags = 0;
    st.glb.fade = 1;
    st.glb.fade_start = 4.0;
    st.glb.fade_end = 8.0;
    st.glb.fade_scale = 0.125;
    let mut r = ray();
    let out = shade(&mut st, &mut Env::default(), &mut r, BG, 1);
    assert_eq!(f32::from_bits(r.w40), 5.0);
    assert_eq!(out.r, (x(0.125f32) * 0.1f32 + x(0.875f32) * 1.0f32).to_f32());
    // a secondary ray adds its length to the distance already travelled; beyond the end the
    // amount is the fourth background value
    r.w40 = 4.0f32.to_bits();
    let out = shade(&mut st, &mut Env::default(), &mut r, BG, 0);
    assert_eq!(f32::from_bits(r.w40), 9.0);
    assert_eq!(out.b, (x(0.6f32) * 0.5f32 + x(0.4f32) * 0.3f32).to_f32());
}

#[test]
fn depth_of_field_value() {
    let mut st = scene();
    st.materials[0].model = 1;
    st.glb.dof = 1;
    st.glb.dof_z0 = -2.0;
    st.glb.dof_z1 = -4.0;
    st.glb.dof_z2 = -8.0;
    st.glb.dof_scale = 3;
    // z = −5 lies between z1 and z2: z2 × (z − z1) / (z2 − z1) = −8 × (−1 / −4) = −2
    let none = Rgbazo { o: -100000.0, ..BG };
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), none, 1).o, -2.0);
    // a background value larger in size wins, with its sign
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), Rgbazo { o: 3.0, ..BG }, 1).o, 3.0);
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), Rgbazo { o: 1.0, ..BG }, 1).o, -2.0);
    // nearer than z0: (z − z0) × scale, held to −z2
    let mut r = ray();
    r.t = 1.0;
    assert_eq!(shade(&mut st, &mut Env::default(), &mut r, none, 1).o, 3.0);
    r.t = 0.0;
    assert_eq!(shade(&mut st, &mut Env::default(), &mut r, none, 1).o, 6.0);
    st.glb.dof_scale = 30;
    assert_eq!(shade(&mut st, &mut Env::default(), &mut r, none, 1).o, 8.0);
    // secondary rays do not compute it
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), none, 0).o, 0.0);
}

#[test]
fn smooth_normal_and_facing() {
    let mut st = scene();
    st.materials[0].flags = flag::SMOOTH;
    st.materials[0].work.diffuse = [1.0; 4];
    st.verts[1].normal = Vector::new(1.0, 0.0, 0.0);
    st.verts[2].normal = Vector::new(1.0, 0.0, 0.0);
    // weights 0.5, 0.25, 0.25: (0.5, 0, 0.5) normalised; a light along x sees 1/√2
    st.lights.push(infinite(Vector::new(1.0, 0.0, 0.0)));
    st.light_head = Some(0);
    let out = shade(&mut st, &mut Env::default(), &mut ray(), BG, 1);
    assert_eq!(out.r, core::f32::consts::FRAC_1_SQRT_2);
    // seen from behind (the plane normal along the ray) the normal is negated: dark
    st.tns[0].n = Vector::new(0.0, 0.0, -1.0);
    assert_eq!(shade(&mut st, &mut Env::default(), &mut ray(), BG, 1).r, 0.0);
}

#[test]
fn textures_reset_then_apply_in_list_order() {
    let mut st = scene();
    st.glb.textures = 1;
    st.glb.textures_2d = 1;
    st.materials[0].model = 1;
    st.materials[0].flags = flag::TEXTURED;
    st.materials[0].diffuse = [0.5; 4];
    st.materials[0].transparency = 0.25;
    st.materials[0].work.base_ambient = [0.125; 4];
    // map 0 on the material (skipped), map 1 on the material, map 2 on the model
    st.maps.push(ShadeMap { method: 4, skip: 1, next: Some(1), ..ShadeMap::default() });
    st.maps.push(ShadeMap { method: 4, umax: 1.0, vmax: 1.0, ..ShadeMap::default() });
    st.maps.push(ShadeMap { method: 4, umax: 0.2, vmax: 1.0, ..ShadeMap::default() });
    st.materials[0].maps = Some(0);
    st.models[0].maps = Some(2);
    // pair 1 of each corner: (0,0), (1,0), (0,1); pair 2 would be past the list: give the
    // corners three pairs
    st.uvs = vec![[0.0; 2], [0.0, 0.0], [0.0, 0.0], [0.0; 2], [1.0, 0.0], [1.0, 0.0], [0.0; 2], [0.0, 1.0], [0.0, 1.0]];
    for (i, v) in st.verts.iter_mut().enumerate() {
        v.uv = 3 * i as u32;
    }
    let mut env = Env { samples: [[0.25, 0.5, 0.75, 0.0, 0.0]; 3], ..Env::default() };
    let out = shade(&mut st, &mut env, &mut ray(), BG, 1);
    // u = 0.25, v = 0.25 for both maps; map 2's region ends at u = 0.2: not applied
    assert_eq!(env.looked_up, vec![(0.25, 0.25, 1, false)]);
    assert_eq!(env.applied, vec![(0, 1)]);
    assert_eq!((out.r, out.g, out.b), (0.25, 0.5, 0.75));
    // the working block is as it was before the call
    assert_eq!(st.materials[0].work.diffuse, [1.0; 4]);
    assert_eq!(st.materials[0].work.ambient, [0.0; 4]);
}

#[test]
fn map_wrap_and_repeat() {
    let mut st = scene();
    st.glb.textures = 1;
    st.glb.textures_2d = 1;
    st.materials[0].model = 1;
    st.materials[0].flags = flag::TEXTURED;
    // u runs 0 → 5 across the triangle: at the hit u = 1.25, v = 0.25
    st.uvs = vec![[0.0, 0.0], [0.0; 2], [5.0, 0.0], [0.0; 2], [0.0, 1.0], [0.0; 2]];
    st.materials[0].maps = Some(0);
    // repeat: the fraction by floor, no region test
    st.maps.push(ShadeMap { method: 4, repeat: 1, ..ShadeMap::default() });
    let mut env = Env::default();
    shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert_eq!(env.looked_up[0].0, 0.25);
    // wrap in u: the fraction by truncation, accepted through the second limits, and moved up
    // by one when below the second upper limit
    st.maps[0] = ShadeMap { method: 4, wrap_u: 1, umin: 0.5, umax: 0.75, u2min: 0.9, u2max: 0.3, vmax: 1.0, ..ShadeMap::default() };
    let mut env = Env::default();
    shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert_eq!(env.looked_up[0].0, 1.25);
    // a band: accepted only between the second limits
    st.maps[0] = ShadeMap { method: 4, band_u: 1, umin: 5.0, umax: 6.0, u2min: 1.0, u2max: 1.5, vmax: 1.0, ..ShadeMap::default() };
    let mut env = Env::default();
    shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert_eq!(env.looked_up[0].0, 1.25);
    st.maps[0].u2max = 1.2;
    let mut env = Env::default();
    shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert!(env.looked_up.is_empty());
}

#[test]
fn bump_mapping_planar_and_uv() {
    let mut st = scene();
    st.glb.textures = 1;
    st.glb.textures_2d = 1;
    st.materials[0].model = 3;
    st.materials[0].flags = flag::TEXTURED;
    st.materials[0].diffuse = [1.0; 4];
    st.materials[0].maps = Some(0);
    st.maps.push(ShadeMap { method: 1, bump: 1, bump_amount: 0.25, umax: 1.0, vmax: 1.0, ..ShadeMap::default() });
    // heights (fifth value): centre 10, towards u 12.9 (difference truncates to 2), towards
    // v 10: a = 0.5, b = 0, w = 0.5: normal (0.5, 0, 0.5) normalised
    let mut env = Env { samples: [[1.0, 1.0, 1.0, 0.0, 10.0], [0.0, 0.0, 0.0, 0.0, 12.9], [0.0, 0.0, 0.0, 0.0, 10.0]], ..Env::default() };
    st.lights.push(infinite(Vector::new(1.0, 0.0, 0.0)));
    st.light_head = Some(0);
    let out = shade(&mut st, &mut env, &mut ray(), BG, 1);
    assert_eq!(env.looked_up[0].3, true);
    assert_eq!(out.r, core::f32::consts::FRAC_1_SQRT_2);
    // the fourth value (alpha) is the height when the map says so: no difference there
    st.maps[0].bump_alpha = 1;
    assert_eq!(shade(&mut st, &mut env, &mut ray(), BG, 1).r, 0.0);
    // method 4: the edge of constant u is corner 0 → corner 2 (raw +y), so the v direction
    // is +y and the u direction normal × that = (0,0,1) × (0,1,0) = (−1, 0, 0)
    st.maps[0].bump_alpha = 0;
    st.maps[0].method = 4;
    st.lights[0].dir = Vector::new(-1.0, 0.0, 0.0);
    assert_eq!(shade(&mut st, &mut env, &mut ray(), BG, 1).r, core::f32::consts::FRAC_1_SQRT_2);
}

#[test]
fn vertex_colours() {
    let mut st = scene();
    st.materials[0].model = 1;
    st.materials[0].vcol_rgb = 2;
    st.materials[0].vcol_alpha = 3;
    st.materials[0].flags = 0x40;
    st.materials[0].work.opacity = 0.5;
    st.verts[0].w44 = 0xff_00_00_ff; // bytes: red 255, green 0, blue 0, alpha 255
    st.verts[1].w44 = 0xff_00_ff_00;
    st.verts[2].w44 = 0xff_ff_00_00;
    let mut r = ray();
    let p = Vector::new(0.25, 0.25, -5.0);
    set_material_from_vertex_color(&mut st, &r, p);
    // weights by area: corner 0 0.5, corner 1 0.25, corner 2 0.25
    assert_eq!(st.materials[0].work.diffuse, [0.5, 0.25, 0.25, 1.0]);
    assert_eq!(st.materials[0].work.opacity, 0.5);
    // through `SI_shade` the block is restored afterwards
    let out = shade(&mut st, &mut Env::default(), &mut r, BG, 1);
    // mode 2 replaces the red by 0.5; the opacity stays 0.5 × 1; over black: 0.25
    assert_eq!(out.r, 0.25);
    assert_eq!(st.materials[0].work.diffuse, [0.5, 0.25, 0.25, 1.0]);
    // mode 3 adds to the present colour
    st.materials[0].vcol_rgb = 3;
    set_material_from_vertex_color(&mut st, &r, p);
    assert_eq!(st.materials[0].work.diffuse[0], 1.0);
}

#[test]
fn layer_fog_same_side() {
    si_rt::skip_without_install!();
    let mut st = scene();
    st.camera.world_to_raw_mat = si_math::types::IDENTITY;
    st.glb.fog_base = 0.0;
    st.glb.fog_thickness = 60.0;
    st.glb.fog_density = 1.0;
    st.glb.fog_start = 0.0;
    st.glb.fog_end = 100.0;
    st.glb.fog_color = [1.0, 1.0, 1.0];
    // a horizontal ray of length 5 at height 3: both ends on the same side of the middle
    // (30), both 27 below it: index 27, one entry: mean 1 − 0.9965; amount = 2 × mean × 5 / 10
    let r = Ray { dir: Vector::new(1.0, 0.0, 0.0), t: 5.0, ..Ray::default() };
    let mut c = [0.0f32, 0.5, 1.0];
    layer_fog(&st, &r, Vector::new(5.0, 3.0, 0.0), &mut c);
    let mean = (F80::ONE - POISSON[27]) / x(1i32) * 1.0f64;
    let amount = ((mean + mean) * x(5.0f32)) / 10.0f64;
    let rest = F80::ONE - amount;
    assert_eq!(c[0], (amount * 1.0f32 + rest * 0.0f32).to_f32());
    assert_eq!(c[1], (amount * 1.0f32 + rest * 0.5f32).to_f32());
    assert_eq!(c[2], 1.0);
    // entirely above the layer: untouched
    let mut c = [0.25f32; 3];
    layer_fog(&st, &r, Vector::new(5.0, 61.0, 0.0), &mut c);
    assert_eq!(c, [0.25; 3]);
    // entirely below
    layer_fog(&st, &r, Vector::new(5.0, -1.0, 0.0), &mut c);
    assert_eq!(c, [0.25; 3]);
}

#[test]
fn layer_fog_across_the_middle() {
    si_rt::skip_without_install!();
    let mut st = scene();
    st.camera.world_to_raw_mat = si_math::types::IDENTITY;
    st.glb.fog_thickness = 60.0;
    st.glb.fog_density = 0.5;
    st.glb.fog_end = 100.0;
    st.glb.fog_color = [1.0; 3];
    // a vertical ray coming down from height 32 to the hit at height 28 (t = 4): the start is
    // 2 above the middle (index 2), the hit 2 below (index 2)
    let r = Ray { dir: Vector::new(0.0, -1.0, 0.0), t: 4.0, ..Ray::default() };
    let mut c = [0.0f32; 3];
    layer_fog(&st, &r, Vector::new(0.0, 28.0, 0.0), &mut c);
    let mut far = x(0.0f32);
    for k in 0..=2 {
        far = far + (F80::ONE - POISSON[k]);
    }
    let mut near = x(0.0f32);
    for k in 0..=2 {
        near = near + POISSON[k];
    }
    let mean = ((far + near) / x(6i32)) * 0.5f64;
    let amount = ((mean + mean) * x(4.0f32)) / 10.0f64;
    assert_eq!(c[0], (amount * 1.0f32 + (F80::ONE - amount) * 0.0f32).to_f32());
}
