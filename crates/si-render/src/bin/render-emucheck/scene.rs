// SPDX-License-Identifier: MIT
//! Checks of the sampling routines on the whole scenes of
//! `re/tools/si-render/emu/gen_frame.py`: `SI_Sample`, `SI_shade` with reflection and
//! refraction, `colSubdivid`, `dofSubdivid`, `blur`.
//!
//! The scene is rebuilt from the words of the vector: four triangles and their slices, two
//! materials, a model, two lights, the two-cell tree, the chunk buffer and the globals. The
//! port then runs with everything real: `SI_shade`, `si_trace`, `si_search` and the chunk
//! allocator are this crate's.
use crate::chunk::{abs_in, buffer_in, same_heap, same_record_in, EmuHeap};
use crate::{bits, fl, matrix, named, numbered, samples_of, vec3, verdict, NoSink};
use si_math::geometry::{RVertex, Tn};
use si_math::types::Vector;
use si_render::mem_chunk::{ChunkBuffer, ChunkHeap};
use si_render::render::{col_subdivid, dof_subdivid, sample, DofBlur, SampleEnv, Sampler, Slice, Supersample};
use si_render::renutil::{OutputLine, OutputState};
use si_render::shade::{shade, MatWork, ShadeEnv, ShadeModel, ShadeState};
use si_render::trace::{s_trace, search, trace, Bsp, Link, Mailbox, Tree};
use si_render::types::{Ray, Rgbazo};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

/// The buffers whose addresses a scene vector lists in `at`.
const AT: [&str; 13] = ["chunk", "arena", "arena2", "tn0", "tn1", "tn2", "tn3", "root", "c1", "c2", "out", "lo", "hi"];
const NODES: [&str; 6] = ["root", "c1", "c2", "out", "lo", "hi"];
const NT: usize = 4;

/// What `SI_shade` calls outside `shade.c`, all real: the tree, the ray counter and the
/// chunk memory the secondary rays come from.
pub struct Inner {
    pub bsp: Bsp,
    pub mailbox: Mailbox,
    pub chunk: ChunkBuffer,
    pub heap: EmuHeap,
}

thread_local! {
    /// Whether the scenes being checked have textures (ninth pass): the texture routines
    /// are then the ports, as in the frame; else a call of one is a fault of the check.
    static TEXTURED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Calls of the texture routines from the frames of textured scenes: `si_BlookUp`,
/// `SI_renderTxt2d`, `SI_renderTxt3d`, `shadowMap`.
pub static TEX_CALLS: [std::sync::atomic::AtomicUsize; 4] = [const { std::sync::atomic::AtomicUsize::new(0) }; 4];

fn count(i: usize) {
    TEX_CALLS[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn set_textured(on: bool) {
    TEXTURED.with(|t| t.set(on));
}

fn textured() -> bool {
    TEXTURED.with(|t| t.get())
}

/// The texture stage alone: what `shadowMap` calls.
struct TexturesOnly;
impl ShadeEnv for TexturesOnly {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        unreachable!("shadowMap does not search the tree")
    }
    fn s_trace(&mut self, _: &mut ShadeState, _: &mut Ray, _: usize, _: f32, _: usize) -> [f32; 4] {
        unreachable!("shadowMap traces no ray")
    }
    fn trace(&mut self, _: &ShadeState, _: &mut Ray, _: usize) -> bool {
        unreachable!("shadowMap traces no ray")
    }
}

impl ShadeEnv for Inner {
    fn search(&mut self, _: &ShadeState, p: Vector) -> u32 {
        if textured() {
            // (without a cell the original stores what si_search returns: null)
            return search(&self.bsp.nodes, self.bsp.root, p).map_or(0, |n| n as u32);
        }
        search(&self.bsp.nodes, self.bsp.root, p).expect("the made-up tree has a cell everywhere") as u32
    }
    fn render_txt3d(&mut self, st: &mut ShadeState, material: usize, p: Vector, normal: Option<&mut Vector>) {
        assert!(textured(), "SI_renderTxt3d called");
        count(2);
        si_render::t3d_ren::render_txt3d(st, material, p, normal, &mut si_render::tt::TrigLib);
    }
    fn blookup(&mut self, st: &ShadeState, u: f32, v: f32, map: usize, bump: bool) -> [[f32; 5]; 3] {
        assert!(textured(), "si_BlookUp called");
        count(0);
        si_render::renutil::blookup(st, u, v, map, st.maps[map].w2c, bump)
    }
    fn render_txt2d(&mut self, st: &mut ShadeState, material: usize, map: usize, sample: [f32; 5]) {
        assert!(textured(), "SI_renderTxt2d called");
        count(1);
        si_render::t2d_ren::render_txt2d(st, material, map, sample);
    }
    fn s_trace(&mut self, st: &mut ShadeState, ray: &mut Ray, tn: usize, distance: f32, light: usize) -> [f32; 4] {
        s_trace(&mut self.bsp, &mut self.mailbox, st, ray, tn, distance, light, &mut |st, ray, mat, p| {
            assert!(textured(), "shadowMap called");
            count(3);
            si_render::trace::shadow_map(st, &mut TexturesOnly, ray, mat, p)
        })
    }
    fn trace(&mut self, st: &ShadeState, ray: &mut Ray, tn: usize) -> bool {
        trace(&mut self.bsp, &mut self.mailbox, ray, &st.tns, &st.verts, tn)
    }
    fn ray_alloc(&mut self) {
        self.chunk.malloc(&mut self.heap, 0x48);
    }
}

pub struct World {
    pub st: ShadeState,
    pub inner: Inner,
}

impl SampleEnv for World {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        (&self.st.tns, &self.st.verts)
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        shade(&mut self.st, &mut self.inner, ray, under, primary)
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        &mut self.inner.chunk
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.inner.chunk.malloc(&mut self.inner.heap, size)
    }
}

pub(crate) fn work_of(m: &Field) -> MatWork {
    let w: Vec<f32> = (449..469).map(|i| fl(m, i)).collect();
    MatWork { base_ambient: [w[0], w[1], w[2], w[3]], ambient: [w[4], w[5], w[6], w[7]], diffuse: [w[8], w[9], w[10], w[11]], specular: [w[12], w[13], w[14], w[15]], reflectivity: w[16], opacity: w[17], refraction: w[18], existence: w[19] }
}

/// Samples from a field in which a word may be one "nobody wrote" (a sixth value the call
/// before left unset): such a word is the port's `unset`.
fn samples_or(f: &Field, unset: f32) -> Vec<Rgbazo> {
    let w: Vec<f32> = f.words().iter().map(|w| w.value().map_or(unset, |v| f32::from_bits(v as u32))).collect();
    w.chunks(6).map(sample_of).collect()
}

fn sample_of(w: &[f32]) -> Rgbazo {
    Rgbazo { r: w[0], g: w[1], b: w[2], a: w[3], z: w[4], o: w[5] }
}

/// The scene of one vector; `unset` is what the port returns for a word the original never
/// set.
pub fn world(c: &Case, unset: f32) -> World {
    let mut st = ShadeState::default();
    st.glb.unset = unset;
    for k in 0..NT {
        let t = c.input(&format!("tn{k}"));
        st.tns.push(Tn {
            v: [Some(3 * k), Some(3 * k + 1), Some(3 * k + 2)],
            n: vec3(t, 3),
            d: fl(t, 6),
            inv_d: fl(t, 7),
            axis: bits(t, 8) as i16,
            inv_n: fl(t, 10),
            edge: [vec3(t, 11), vec3(t, 14), vec3(t, 17)],
            material: numbered(&t.words()[20], "mat").expect("a triangle names its material") as _,
            flags: bits(t, 21) as u16,
            flags2: bits(t, 22) as u16,
            w58: bits(t, 24) as _,
            ..Tn::default()
        });
        for j in 0..3 {
            let f = c.input(&format!("t{k}v{j}"));
            st.verts.push(RVertex { world: vec3(f, 8), normal: vec3(f, 13), w44: bits(f, 17), ..RVertex::default() });
        }
    }
    let loaded = crate::outer::Loaded::of(c);
    for name in ["mat0", "mat1"] {
        st.materials.push(si_render::inimage::material_at(&loaded.m, loaded.at(name)));
    }
    st.models.push(ShadeModel { flags: bits(c.input("model"), 133) as u16, ..ShadeModel::default() });
    st.lights = vec![crate::light_in(&loaded, c, "l0"), crate::light_in(&loaded, c, "l1")];
    st.light_head = if matches!(c.input("g_lights").words()[0], Word::Ref { .. }) { Some(0) } else { None };
    let g = &mut st.glb;
    g.fade = bits(c.input("g_fade"), 0) as i16;
    let fp = c.input("g_fadep").f32s();
    (g.fade_start, g.fade_end, g.fade_scale) = (fp[0], fp[2], fp[3]);
    g.background = sample_of(&c.input("g_bg").f32s());
    g.fog = bits(c.input("g_fog"), 0) as i16;
    let fog = c.input("fog").f32s();
    (g.fog_density, g.fog_base, g.fog_thickness, g.fog_start, g.fog_end) = (fog[0], fog[1], fog[2], fog[3], fog[4]);
    let fc = c.input("fogc").f32s();
    g.fog_color = [fc[0], fc[1], fc[2]];
    g.dof = c.input("g_dof").i32();
    let dz = c.input("g_dofz").f32s();
    (g.dof_z0, g.dof_z1, g.dof_z2) = (dz[0], dz[2], dz[4]);
    g.dof_scale = c.input("g_dofs").i32();
    g.raytrace = bits(c.input("g_rt"), 0) & 1 != 0;
    let sw = c.input("g_sw");
    (g.textures, g.textures_2d, g.textures_3d) = (bits(sw, 0) as i16, bits(sw, 1) as i16, bits(sw, 2) as i16);
    (g.reflection, g.refraction, g.shadows) = (bits(sw, 3) as i16, bits(sw, 4) as i16, bits(sw, 5) as i16);
    g.max_depth = bits(c.input("g_depth"), 0) as i16;
    g.no_spec_alpha = bits(c.input("g_nsa"), 0) as i16;
    st.camera.world_to_raw_mat = matrix(c.input("mat4"), 0);
    // the tree
    let node = |name: &str| {
        let f = c.input(name);
        let w = f.words();
        let (kind, level, mailbox) = (bits(f, 0) as i16, bits(f, 1) as i16, bits(f, 2) as i32);
        if name.starts_with('c') || name == "out" {
            Tree { kind, level, mailbox, list: numbered(&w[3], "k"), walls: core::array::from_fn(|i| named(&w[4 + i], &NODES)), ..Tree::default() }
        } else {
            Tree { kind, level, mailbox, split: fl(f, 3), low: named(&w[4], &NODES), high: named(&w[5], &NODES), ..Tree::default() }
        }
    };
    let links = (0..2 * NT)
        .map(|k| {
            let f = c.input(&format!("k{k}"));
            Link { tn: numbered(&f.words()[6], "tn").unwrap_or(0), next: numbered(&f.words()[7], "k"), ..Link::default() }
        })
        .collect();
    let bsp = Bsp { nodes: NODES.iter().map(|n| node(n)).collect(), links, root: 0, outside: Some(3) };
    World { st, inner: Inner { bsp, mailbox: Mailbox { current: c.input("g_mb").i32() }, chunk: buffer_in(c, &AT, c.input("chunk")), heap: EmuHeap::new() } }
}

fn slices_of(c: &Case) -> Vec<Slice> {
    (0..NT)
        .map(|k| {
            let f = c.input(&format!("sl{k}"));
            let w = f.words();
            Slice { key: fl(f, 0), x0: fl(f, 1), dx0: fl(f, 2), x1: fl(f, 3), dx1: fl(f, 4), y_end: bits(f, 5) as i32, tn: numbered(&w[6], "tn").expect("a slice names its triangle") as u32, next: numbered(&w[7], "sl").into(), link: numbered(&w[8], "sl").into(), by_depth: numbered(&w[9], "sl").into() }
        })
        .collect()
}

/// A ray from the words of a typed ray record (19 words: the two shorts apart).
fn ray_at(f: &Field, at: usize) -> Ray {
    let w = f.words();
    Ray { origin: vec3(f, at), dir: vec3(f, at + 3), v18: vec3(f, at + 6), t: fl(f, at + 9), hit: vec3(f, at + 10), tn: numbered(&w[at + 13], "tn"), w38: named(&w[at + 14], &NODES).unwrap_or(0) as u32, w3c: bits(f, at + 15) as i16, w3e: bits(f, at + 16) as i16, w40: bits(f, at + 17), w44: bits(f, at + 18) }
}

fn same_ray_at(f: &Field, at: usize, r: &Ray, what: &str) -> Result<(), String> {
    let port = [r.origin.x, r.origin.y, r.origin.z, r.dir.x, r.dir.y, r.dir.z, r.v18.x, r.v18.y, r.v18.z, r.t, r.hit.x, r.hit.y, r.hit.z];
    for (k, v) in port.iter().enumerate() {
        if bits(f, at + k) != v.to_bits() {
            return Err(format!("{what} word {k}: original {:08x}, port {:08x}", bits(f, at + k), v.to_bits()));
        }
    }
    let w = f.words();
    if numbered(&w[at + 13], "tn") != r.tn {
        return Err(format!("{what} triangle: original {:?}, port {:?}", w[at + 13], r.tn));
    }
    if bits(f, at + 16) as i16 != r.w3e || bits(f, at + 17) != r.w40 {
        return Err(format!("{what} depth or distance"));
    }
    Ok(())
}

/// What the routine left of the scene: the pixel's head, the slices' keys and links, the
/// materials' working blocks, the ray counter, the chunk buffer and the blocks it took.
fn same_scene(c: &Case, w: &World, slices: &[Slice], head: Option<usize>) -> Result<(), String> {
    if let Some(px) = c.try_output("px") {
        if numbered(&px.words()[8], "sl") != head {
            return Err(format!("pixel list head: original {:?}, port {head:?}", px.words()[8]));
        }
    }
    for (k, s) in slices.iter().enumerate() {
        let f = c.after(&format!("sl{k}"));
        let fw = f.words();
        if bits(f, 0) != s.key.to_bits() || numbered(&fw[8], "sl") != s.link.get() || numbered(&fw[9], "sl") != s.by_depth.get() {
            return Err(format!("slice {k}: original key {:08x} link {:?} depth {:?}, port {:08x} {:?} {:?}", bits(f, 0), fw[8], fw[9], s.key.to_bits(), s.link, s.by_depth));
        }
    }
    for (k, name) in ["mat0", "mat1"].iter().enumerate() {
        if work_of(c.after(name)) != w.st.materials[k].work {
            // (bit patterns: the working block holds ordinary numbers here)
            return Err(format!("working block of material {k}"));
        }
    }
    if c.after("g_mb").i32() != w.inner.mailbox.current {
        return Err(format!("ray counter: original {}, port {}", c.after("g_mb").i32(), w.inner.mailbox.current));
    }
    same_record_in(c, &AT, "chunk", &w.inner.chunk)?;
    same_heap(c, &w.inner.heap)
}

/// The two values the port is run with for words the original never set. A result word
/// that differs between the two runs depends on such a word and has no value to compare
/// (the original's is whatever its stack held, which may even be the same in both of the
/// harness's passes: an address an earlier call left there).
const UNSET: [f32; 2] = [0.0, 4321.5];

fn six(p: Rgbazo) -> [u32; 6] {
    [p.r, p.g, p.b, p.a, p.z, p.o].map(f32::to_bits)
}

fn same_result(c: &Case, name: &str, got: [Rgbazo; 2]) -> Result<(), String> {
    let (a, b) = (six(got[0]), six(got[1]));
    for (k, w) in c.output(name).written().iter().enumerate() {
        // a word the original left as it found it, or took from memory it never set, has
        // no value to compare
        if let Some(w) = w {
            if a[k] == b[k] && *w as u32 != a[k] {
                return Err(format!("{name}[{k}]: original {:08x}, port {:08x}", *w as u32, a[k]));
            }
        }
    }
    Ok(())
}

fn check_sample(c: &Case) -> Verdict {
    let run = |unset: f32| {
        let mut w = world(c, unset);
        let mut slices = slices_of(c);
        let mut head = numbered(&c.input("px").words()[8], "sl");
        let rf = c.input("rays");
        let mut rays: Vec<Ray> = (0..6).map(|i| ray_at(rf, 19 * i)).collect();
        let bg = w.st.glb.background;
        let got = sample(&mut w, &mut slices, &mut head, vec3(c.input("dir"), 0), c.input("relink").i16() != 0, &mut rays, bg);
        (got, w, slices, head, rays)
    };
    let (got, w, slices, head, rays) = run(UNSET[0]);
    let other = run(UNSET[1]).0;
    verdict((|| {
        same_result(c, "ret", [got, other])?;
        let after = c.after("rays");
        for (i, r) in rays.iter().enumerate() {
            same_ray_at(after, 19 * i, r, &format!("ray {i}"))?;
        }
        same_scene(c, &w, &slices, head)
    })())
}

fn check_shade_rt(c: &Case) -> Verdict {
    let run = |unset: f32| {
        let mut w = world(c, unset);
        let mut ray = ray_at(c.input("ray"), 0);
        let got = shade(&mut w.st, &mut w.inner, &mut ray, sample_of(&c.input("bg").f32s()), c.input("primary").i16());
        (got, w, ray)
    };
    let (got, w, ray) = run(UNSET[0]);
    let other = run(UNSET[1]).0;
    verdict((|| {
        same_result(c, "ret", [got, other])?;
        let after = c.after("ray");
        for (i, v) in [(10, ray.hit.x.to_bits()), (11, ray.hit.y.to_bits()), (12, ray.hit.z.to_bits()), (17, ray.w40)] {
            if bits(after, i) != v {
                return Err(format!("ray word {i}: original {:08x}, port {v:08x}", bits(after, i)));
            }
        }
        if named(&after.words()[14], &NODES) != Some(ray.w38 as usize) {
            return Err(format!("the ray's cell: original {:?}, port {}", after.words()[14], ray.w38));
        }
        same_scene(c, &w, &slices_of(c), None)
    })())
}

/// `si_sTrace` on a scene: a shadow ray along the stack of triangles. Compared: the three
/// factors (the fourth float returned is never written), the ray (reciprocals, distance,
/// weights, triangle, cell), every triangle's flag word (the "met" marks), the materials'
/// working blocks and the ray counter.
fn check_s_trace(c: &Case) -> Verdict {
    let run = |unset: f32| {
        let mut w = world(c, unset);
        let mut ray = ray_at(c.input("ray"), 0);
        let from = numbered(&c.input("from").words()[0], "tn").expect("the ray starts from a triangle");
        let light = numbered(&c.input("light").words()[0], "l").expect("a light");
        let got = s_trace(&mut w.inner.bsp, &mut w.inner.mailbox, &mut w.st, &mut ray, from, c.input("dist").f32(), light, &mut |_, _, _, _| panic!("shadowMap called"));
        (got, w, ray)
    };
    let (got, w, ray) = run(UNSET[0]);
    let other = run(UNSET[1]).0;
    verdict((|| {
        for (k, word) in c.output("ret").written().iter().enumerate() {
            if let Some(v) = word {
                if got[k].to_bits() == other[k].to_bits() && *v as u32 != got[k].to_bits() {
                    return Err(format!("factor {k}: original {:08x}, port {:08x}", *v as u32, got[k].to_bits()));
                }
            }
        }
        let after = c.after("ray");
        let port = [ray.v18.x, ray.v18.y, ray.v18.z, ray.t, ray.hit.x, ray.hit.y, ray.hit.z];
        for (k, v) in port.iter().enumerate() {
            if bits(after, 6 + k) != v.to_bits() {
                return Err(format!("ray word {}: original {:08x}, port {:08x}", 6 + k, bits(after, 6 + k), v.to_bits()));
            }
        }
        if numbered(&after.words()[13], "tn") != ray.tn {
            return Err(format!("the ray's triangle: original {:?}, port {:?}", after.words()[13], ray.tn));
        }
        if named(&after.words()[14], &NODES) != Some(ray.w38 as usize) {
            return Err(format!("the ray's cell: original {:?}, port {}", after.words()[14], ray.w38));
        }
        for k in 0..NT {
            let flags = bits(c.after(&format!("tn{k}")), 21) as u16;
            if flags != w.st.tns[k].flags {
                return Err(format!("flag word of triangle {k}: original {flags:04x}, port {:04x}", w.st.tns[k].flags));
            }
        }
        for (k, name) in ["mat0", "mat1"].iter().enumerate() {
            if work_of(c.after(name)) != w.st.materials[k].work {
                return Err(format!("working block of material {k}"));
            }
        }
        if c.after("g_mb").i32() != w.inner.mailbox.current {
            return Err(format!("ray counter: original {}, port {}", c.after("g_mb").i32(), w.inner.mailbox.current));
        }
        Ok(())
    })())
}

/// The 100 file-scope rays from 32-bit words (18 a ray); a triangle is an address.
fn winner_of(c: &Case, f: &Field) -> Vec<Ray> {
    let at = c.output("at").u32s();
    (0..100)
        .map(|i| {
            let b = 18 * i;
            let tn = (0..NT).find(|k| at[3 + k] == bits(f, b + 13) && bits(f, b + 13) != 0);
            let cell = (0..6).find(|k| at[7 + k] == bits(f, b + 14)).unwrap_or(0) as u32;
            Ray { origin: vec3(f, b), dir: vec3(f, b + 3), v18: vec3(f, b + 6), t: fl(f, b + 9), hit: vec3(f, b + 10), tn, w38: cell, w3c: bits(f, b + 15) as i16, w3e: (bits(f, b + 15) >> 16) as i16, w40: bits(f, b + 16), w44: bits(f, b + 17) }
        })
        .collect()
}

fn check_subdivid(c: &Case, dof: bool) -> Verdict {
    // the cache's rows in the order of the pointer table `asBuffer`
    let order: Vec<usize> = c.input("asb").words().iter().map(|w| numbered(w, "row").expect("a cache row")).collect();
    let run = |unset: f32| {
        let mut w = world(c, unset);
        let mut slices = slices_of(c);
        let mut head = numbered(&c.input("px").words()[8], "sl");
        let d = c.input("g_dir").f32s();
        let mut ss = Supersample {
            sub: c.input("g_sub").i16(),
            y: c.input("g_y").i16(),
            rows: order.iter().map(|r| samples_or(c.input(&format!("row{r}")), unset)).collect(),
            dir: [d[0], d[1], d[2], d[3], d[4]],
            resort: c.input("g_resort").i16(),
            winner: winner_of(c, c.input("winner")),
            contrast: c.input("g_con").f32(),
            dof_limit: c.input("g_lim").f32(),
            background: w.st.glb.background,
            unset,
        };
        let (x0, x1, y0, y1) = (c.input("x0").f32(), c.input("x1").f32(), c.input("y0").f32(), c.input("y1").f32());
        let got = if dof { dof_subdivid(&mut ss, &mut w, &mut slices, &mut head, x0, x1, y0, y1) } else { col_subdivid(&mut ss, &mut w, &mut slices, &mut head, x0, x1, y0, y1) };
        (got, w, slices, head, ss)
    };
    let (got, w, slices, head, ss) = run(UNSET[0]);
    let (other, _, _, _, ss2) = run(UNSET[1]);
    verdict((|| {
        same_result(c, "ret", [got, other])?;
        for (r, buffer) in order.iter().enumerate() {
            let name = format!("row{buffer}");
            let Some(f) = c.try_output(&name) else { continue };
            for (i, p) in ss.rows[r].iter().enumerate() {
                let (port, port2) = (six(*p), six(ss2.rows[r][i]));
                for k in 0..6 {
                    if let Some(v) = f.words()[6 * i + k].value() {
                        if port[k] == port2[k] && v as u32 != port[k] {
                            return Err(format!("cache row {r} entry {i} word {k}: original {:08x}, port {:08x}", v as u32, port[k]));
                        }
                    }
                }
            }
        }
        let after = winner_of(c, c.after("winner"));
        for (i, (a, b)) in after.iter().zip(&ss.winner).enumerate() {
            if (a.dir, a.t.to_bits(), a.hit, a.tn, a.w3e) != (b.dir, b.t.to_bits(), b.hit, b.tn, b.w3e) {
                return Err(format!("layer ray {i}: original {a:?}, port {b:?}"));
            }
        }
        same_scene(c, &w, &slices, head)
    })())
}

fn check_blur(c: &Case) -> Verdict {
    let order: Vec<usize> = c.input("brows").words().iter().filter_map(|w| numbered(w, "brow")).collect();
    let lines = c.input("g_lines").i32();
    let gx = c.input("g_x").i32s();
    let xs = c.input("g_xs").i32s();
    let blank = OutputLine { red: vec![0; 32], green: vec![0; 32], blue: vec![0; 32], alpha: vec![0; 32] };
    let mut s = Sampler {
        level: c.input("g_level").i16(),
        region: [xs[0], xs[1], 0, 0],
        blank: samples_of(c.input("g_blank"))[0],
        line: samples_of(c.input("line")),
        sr_x_offset: c.input("g_sx").i32(),
        sr_y_offset: c.input("g_sy").i32(),
        sr_x_res: c.input("g_sres").i32() as i16,
        out: OutputState { x0: gx[0], x1: gx[1], red: c.input("r").u8s(), green: c.input("g").u8s(), blue: c.input("b").u8s(), alpha: c.input("a").u8s(), depth: c.input("z").f32s(), line: blank, y: c.input("g_y").i16(), width: c.input("g_w").i16(), ..OutputState::default() },
        dof: DofBlur {
            lines,
            half: c.input("g_half").i32(),
            radius: c.input("g_rad").i32(),
            z2: c.input("g_z2").f32(),
            filters: (0..4).map(|r| (0..7).map(|j| c.input(&format!("f{r}_{j}")).f32s()).collect()).collect(),
            rows: order.iter().map(|&r| samples_of(c.input(&format!("brow{r}")))).collect(),
            n_call: c.input("g_ncall").i32(),
        },
        ..Sampler::default()
    };
    let before = s.out.y;
    s.blur(&mut NoSink);
    verdict((|| {
        // the row pointers turned by one
        let mut turned = order.clone();
        turned.rotate_left(1);
        let after: Vec<usize> = c.after("brows").words().iter().filter_map(|w| numbered(w, "brow")).collect();
        if after != turned {
            return Err(format!("row order: original {after:?}, port {turned:?}"));
        }
        for (k, &r) in turned.iter().enumerate() {
            let flat: Vec<f32> = s.dof.rows[k].iter().flat_map(|p| [p.r, p.g, p.b, p.a, p.z, p.o]).collect();
            if !c.after(&format!("brow{r}")).same_f32(&flat) {
                return Err(format!("row {k} (buffer {r})"));
            }
        }
        let flat: Vec<f32> = s.line.iter().flat_map(|p| [p.r, p.g, p.b, p.a, p.z, p.o]).collect();
        let want = c.after("line").f32s();
        if let Some(i) = (0..flat.len()).find(|&i| flat[i].to_bits() != want[i].to_bits()) {
            return Err(format!("line entry {} word {}: original {:08x}, port {:08x} (lines {lines})", i / 6, i % 6, want[i].to_bits(), flat[i].to_bits()));
        }
        if c.after("g_ncall").i32() != s.dof.n_call {
            return Err("line count".into());
        }
        crate::same_output(c, &s.out, if s.out.y != before { s.sr_x_res as usize } else { 0 })
    })())
}

// ---------------------------------------------------------------------------------- frames

/// The frame loop's surroundings for the frame check: the scene, and a sink that keeps the
/// lines of samples `SI_Output` is handed (through the dither routine's arguments, the one
/// place where a destination sees the samples themselves).
struct FrameWorld {
    world: World,
    lines: Vec<Vec<Rgbazo>>,
    width: usize,
}

impl SampleEnv for FrameWorld {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        self.world.geometry()
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        self.world.shade(ray, under, primary)
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        self.world.chunk()
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.world.chunk_malloc(size)
    }
}

impl si_render::renutil::LineSink for FrameWorld {
    fn dither_scan(&mut self, samples: &[Rgbazo], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [f32]) {
        self.lines.push(samples[..self.width].to_vec());
    }
    fn incrustation(&mut self, _: &mut OutputLine, _: i32) {}
    fn rle_write(&mut self, _: i32, _: i32, _: &OutputLine) {}
    fn depth_write(&mut self, _: &[f32]) {}
    fn fb_write(&mut self, _: i16) {}
    fn status(&mut self, _: &str) {}
}

impl si_render::render::FrameEnv for FrameWorld {
    fn display_status(&mut self, _: &str) {}
    fn check_render_abort(&mut self) {}
    fn check_for_exit(&mut self) {}
    fn depth_open(&mut self, _: &[u8]) {}
    fn depth_close(&mut self) {}
    fn tag_write_scanline(&mut self, _: &si_render::rnd_tag::TagImage) {}
}

/// The port's `SI_supersample` on the frame the first case of a frame describes: the lines
/// of samples, top line first.
fn render_frame(c: &Case, unset: f32) -> Vec<Vec<Rgbazo>> {
    let id = c.input("f_id").i32s();
    let (w, h) = ((id[3] & 0xffff) as usize, (id[3] >> 16) as usize);
    let world = world(c, unset);
    let d = c.input("g_dir").f32s();
    let slices: Vec<Slice> = (0..NT)
        .map(|k| {
            let f = c.input(&format!("f_sl{k}"));
            let fw = f.words();
            Slice { key: fl(f, 0), x0: fl(f, 1), dx0: fl(f, 2), x1: fl(f, 3), dx1: fl(f, 4), y_end: bits(f, 5) as i32, tn: numbered(&fw[6], "tn").expect("a slice names its triangle") as u32, next: numbered(&fw[7], "sl").into(), link: None.into(), by_depth: None.into() }
        })
        .collect();
    let bytes = || vec![0u8; w + 10];
    let mut s = Sampler {
        level: 1,
        region: [0, w as i32 - 1, 0, h as i32 - 1],
        region_y_px: [0, h as i32 - 1],
        picture_y: [0.0, (h - 1) as f32],
        window_x: [0.0, (w - 1) as f32],
        ss: Supersample { sub: c.input("g_sub").i16(), dir: [d[0], d[1], d[2], d[3], d[4]], contrast: c.input("g_con").f32(), dof_limit: c.input("g_lim").f32(), background: world.st.glb.background, unset, ..Supersample::default() },
        scan_lines: c.input("f_scan").words().iter().map(|w| numbered(w, "sl")).collect(),
        columns: vec![None; w + 2],
        slices,
        line: vec![Rgbazo::default(); w + 2],
        out: OutputState { x0: 0, x1: w as i32 - 1, red: bytes(), green: bytes(), blue: bytes(), alpha: bytes(), depth: vec![0.0; w + 10], line: OutputLine { red: bytes(), green: bytes(), blue: bytes(), alpha: bytes() }, width: w as i16, dither: true, ..OutputState::default() },
        ..Sampler::default()
    };
    let mut env = FrameWorld { world, lines: Vec::new(), width: w };
    s.supersample_frame(&mut env);
    env.lines
}

/// The port's `SI_supersample` on a scene the port prepared itself: the lines of samples of
/// the rendered region, top line first. Chunk memory is a fresh buffer of two blocks.
pub(crate) fn render_prepared(st: ShadeState, bsp: Bsp, ray_number: i32, mut s: Sampler, width: usize) -> Vec<Vec<Rgbazo>> {
    let mut heap = EmuHeap::new();
    let chunk = si_render::mem_chunk::chunk_allocate(&mut heap, 2, 0x60000).expect("a chunk buffer");
    let bytes = || vec![0u8; width + 10];
    s.out = OutputState { x0: 0, x1: width as i32 - 1, red: bytes(), green: bytes(), blue: bytes(), alpha: bytes(), depth: vec![0.0; width + 10], line: OutputLine { red: bytes(), green: bytes(), blue: bytes(), alpha: bytes() }, width: width as i16, dither: true, ..OutputState::default() };
    let world = World { st, inner: Inner { bsp, mailbox: Mailbox { current: ray_number }, chunk, heap } };
    let mut env = FrameWorld { world, lines: Vec::new(), width };
    s.supersample_frame(&mut env);
    env.lines
}

/// The frame check: `colSubdivid#frame.vec` holds frames rendered pixel by pixel through
/// the original's `colSubdivid` (see `gen_frame.py`); the port's `SI_supersample` renders
/// each frame whole and every pixel the original rendered is compared (red, green, blue,
/// alpha and depth; the sixth value is a stack word in the original).
pub fn frame_pass(dir: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(dir.join("colSubdivid#frame.vec")) else { return };
    let cases: Vec<Case> = text.lines().filter_map(|l| Case::parse(l).ok().flatten()).collect();
    let (mut frames, mut pixels, mut same, mut diff, mut shown) = (0, 0, 0, 0, 0);
    let mut at = 0;
    while at < cases.len() {
        let id = cases[at].input("f_id").i32s()[0];
        let n = cases[at..].iter().take_while(|c| c.input("f_id").i32s()[0] == id).count();
        frames += 1;
        let first = &cases[at];
        let rendered = std::panic::catch_unwind(|| (render_frame(first, UNSET[0]), render_frame(first, UNSET[1])));
        let h = (first.input("f_id").i32s()[3] >> 16) as usize;
        for c in &cases[at..at + n] {
            pixels += 1;
            if c.stopped() {
                continue;
            }
            let f = c.input("f_id").i32s();
            let (px, py) = (f[1] as usize, f[2] as usize);
            let ok = match &rendered {
                Ok((a, b)) if a.len() == h => same_result(c, "ret", [a[h - 1 - py][px], b[h - 1 - py][px]]),
                Ok((a, _)) => Err(format!("the port wrote {} lines", a.len())),
                Err(_) => Err("the port panicked".into()),
            };
            match ok {
                Ok(()) => same += 1,
                Err(e) => {
                    diff += 1;
                    if shown < 5 {
                        shown += 1;
                        println!("    frame {id} pixel ({px}, {py}): {e}");
                    }
                }
            }
        }
        at += n;
    }
    println!("frames rendered pixel by pixel through the original's colSubdivid, against the port's SI_supersample:");
    println!("{:<46} {:>6} {:>9} {:>9}   ({frames} frames)", "pixels with something to shade", pixels, same, diff);
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_Sample" | "SI_Sample#rt" | "SI_Sample#shadow" => check_sample,
        "SI_shade#rt" | "SI_shade#shadow" => check_shade_rt,
        "si_sTrace" => check_s_trace,
        "colSubdivid" | "colSubdivid#frame" => |c: &Case| check_subdivid(c, false),
        "dofSubdivid" => |c: &Case| check_subdivid(c, true),
        "blur__Fv" => check_blur,
        _ => return None,
    })
}

/// Silences the unused warning for helpers only some checks use.
#[allow(dead_code)]
fn _unused(c: &Case, w: &Word) -> u32 {
    abs_in(c, &AT, w)
}
#[allow(dead_code)]
fn _heap(h: &mut EmuHeap) -> u32 {
    h.malloc(0)
}
