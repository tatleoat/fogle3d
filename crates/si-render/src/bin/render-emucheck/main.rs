// SPDX-License-Identifier: MIT
//! `render-emucheck <vectors directory>`: compares this crate with the original, routine by
//! routine, on the test vectors of the emulation harness (finding 0011, `re/tools/emu`).
//!
//! The vectors are what each routine of the Linux `soft` binary left behind when its
//! instructions were stepped through in a CPU emulator on made-up inputs
//! (`re/tools/si-render/emu/render.spec` and `gen_render.py` say which). They are derived
//! from the binary and are not in the repository. For every vector file the report gives the
//! number of cases, how many the port reproduces bit for bit, and how many differ.
use si_math::geometry::{RVertex, Tn};
use si_math::types::Vector;
use si_render::camera;
use si_render::render::{intersect, intersect1, intersect2, Antialias, Slice};
use si_render::renutil::{output, LineSink, OutputLine, OutputState};
use si_render::shade::{layer_fog, shade, MatWork, ShadeEnv, ShadeLight, ShadeModel, ShadeState};
use si_render::trace::{push, search, trace, Bsp, Link, Mailbox, Tree};
use si_render::types::{Ray, Rgbazo};
use si_x87::vectors::{check_dir, print_report, Case, Check, Field, Verdict, Word};
use si_x87::F80;
use std::path::Path;

mod chunk;
mod light;
mod mat;
mod mesh;
mod out;
mod outer;
mod pass9;
mod pass10;
mod pass11;
mod app2;
mod prep;
mod scene;
mod tex;

/// Word `i` of a field as 32 bits; a pointer or an unwritten word reads as 0.
fn bits(f: &Field, i: usize) -> u32 {
    f.words()[i].value().unwrap_or(0) as u32
}

fn fl(f: &Field, i: usize) -> f32 {
    f32::from_bits(bits(f, i))
}

fn vec3(f: &Field, i: usize) -> Vector {
    Vector::new(fl(f, i), fl(f, i + 1), fl(f, i + 2))
}

fn st0(c: &Case, got: F80) -> Verdict {
    Verdict::from(c.output("st0").same_f80(got), || format!("port {got:?}"))
}

fn ray_of(f: &Field) -> Ray {
    Ray { origin: vec3(f, 0), dir: vec3(f, 3), v18: vec3(f, 6), t: fl(f, 9), hit: vec3(f, 10), ..Ray::default() }
}

/// The triangle record and its three corners as the generator laid them out.
fn triangle(c: &Case) -> (Vec<Tn>, Vec<RVertex>) {
    let t = c.input("tn");
    let verts = ["v0", "v1", "v2"].iter().map(|n| RVertex { world: vec3(c.input(n), 8), ..RVertex::default() }).collect();
    let tn = Tn {
        v: [Some(0), Some(1), Some(2)],
        n: vec3(t, 3),
        d: fl(t, 6),
        inv_d: fl(t, 7),
        axis: bits(t, 8) as i16,
        inv_n: fl(t, 10),
        edge: [vec3(t, 11), vec3(t, 14), vec3(t, 17)],
        ..Tn::default()
    };
    (vec![tn], verts)
}

/// Compares the ray as the original left it with the port's. The ray is an in-out buffer, so
/// the vector holds every word after the call: the distance (word 9) and the weights (10 to
/// 12) are compared bit for bit whether they were written or not, and the triangle pointer
/// (word 13) must be a pointer exactly when the port recorded a hit.
fn same_ray(c: &Case, after: &Ray) -> Result<(), String> {
    let w = c.output("ray").words();
    for (i, new, name) in [(9, after.t, "t"), (10, after.hit.x, "hit.x"), (11, after.hit.y, "hit.y"), (12, after.hit.z, "hit.z")] {
        if w[i].value().map(|v| v as u32) != Some(new.to_bits()) {
            return Err(format!("{name}: original {:?}, port {:08x}", w[i], new.to_bits()));
        }
    }
    let wrote_tn = matches!(w[13], Word::Ref { .. });
    if wrote_tn != after.tn.is_some() {
        return Err(format!("triangle pointer: original wrote {wrote_tn}, port {:?}", after.tn));
    }
    Ok(())
}

fn verdict(r: Result<(), String>) -> Verdict {
    match r {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}

fn check_intersect(c: &Case, excluded: bool) -> Verdict {
    let (tns, verts) = triangle(c);
    let mut ray = ray_of(c.input("ray"));
    let hit = intersect(&mut ray, &tns, &verts, 0, if excluded { Some(0) } else { None }, c.input("tmin").f32());
    // the original returns 0 for a hit and 2 for a miss
    let eax = c.output("eax").u32();
    if eax != if hit { 0 } else { 2 } {
        return Verdict::Different(format!("eax {eax}, port hit {hit}"));
    }
    verdict(same_ray(c, &ray))
}

fn slice_of(c: &Case) -> Slice {
    Slice { key: fl(c.input("slice"), 0), tn: 0, ..Slice::default() }
}

/// `SI_shade` is given scenes in which it calls nothing outside `shade.c` but
/// `si_cameraToLight`: textures, ray tracing, reflection and refraction are off.
struct NoCalls;
impl ShadeEnv for NoCalls {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        panic!("si_search called")
    }
    fn render_txt3d(&mut self, _: &mut ShadeState, _: usize, _: Vector, _: Option<&mut Vector>) {
        panic!("SI_renderTxt3d called")
    }
    fn blookup(&mut self, _: &ShadeState, _: f32, _: f32, _: usize, _: bool) -> [[f32; 5]; 3] {
        panic!("si_BlookUp called")
    }
    fn render_txt2d(&mut self, _: &mut ShadeState, _: usize, _: usize, _: [f32; 5]) {
        panic!("SI_renderTxt2d called")
    }
    fn s_trace(&mut self, _: &mut ShadeState, _: &mut Ray, _: usize, _: f32, _: usize) -> [f32; 4] {
        panic!("si_sTrace called")
    }
    fn trace(&mut self, _: &ShadeState, _: &mut Ray, _: usize) -> bool {
        panic!("si_trace called")
    }
}

fn matrix(f: &Field, at: usize) -> si_math::types::Matrix {
    core::array::from_fn(|i| core::array::from_fn(|j| fl(f, at + 4 * i + j)))
}

/// The light `name` of a case, read from the case loaded into an image
/// (`si_render::inimage::light_at`); the table of a selective light has the length of the
/// case's buffer `sel`. The link is whether the record has one (the cases chain two lights).
fn light_of(c: &Case, name: &str) -> ShadeLight {
    let l = outer::Loaded::of(c);
    light_in(&l, c, name)
}

fn light_in(l: &outer::Loaded, c: &Case, name: &str) -> ShadeLight {
    let a = l.at(name);
    let mut r = si_render::inimage::light_at(&l.m, a, c.try_input("sel").map_or(0, |f| f.len() as u32));
    r.next = (l.m.u32(a + 0x140) != 0).then_some(1);
    r
}

/// One call of `SI_shade` on a whole made-up scene (see `gen_render.py`). Compared: the six
/// floats returned (the first five only when `five` is set: without depth of field, or for
/// a secondary ray, the sixth is a stack slot the original never writes), the hit weights and
/// the travelled distance left in the ray, and the material's working block as left behind.
fn shade_verdict(c: &Case, five: bool) -> Verdict {
    shade_verdict_with(c, five, &mut NoCalls, &|_, _| {})
}

/// [`shade_verdict`] with the environment given and the state completed by `prepare` before
/// the call (the texture cases add maps, pictures and coordinate pairs).
pub fn shade_verdict_with<E: ShadeEnv>(c: &Case, five: bool, env: &mut E, prepare: &dyn Fn(&mut ShadeState, &Case)) -> Verdict {
    let mut st = ShadeState::default();
    let (t, m, r) = (c.input("tn"), c.input("mat"), c.input("ray"));
    st.tns.push(Tn {
        v: [Some(0), Some(1), Some(2)],
        n: vec3(t, 3),
        d: fl(t, 6),
        inv_d: fl(t, 7),
        axis: bits(t, 8) as i16,
        inv_n: fl(t, 10),
        edge: [vec3(t, 11), vec3(t, 14), vec3(t, 17)],
        material: 0,
        flags: bits(t, 21) as u16,
        flags2: bits(t, 22) as u16,
        w58: 0,
        ..Tn::default()
    });
    for n in ["v0", "v1", "v2"] {
        let f = c.input(n);
        st.verts.push(RVertex { world: vec3(f, 8), normal: vec3(f, 13), w44: bits(f, 17), ..RVertex::default() });
    }
    let work: Vec<f32> = (449..469).map(|i| fl(m, i)).collect();
    let block = |w: &[f32]| MatWork {
        base_ambient: [w[0], w[1], w[2], w[3]],
        ambient: [w[4], w[5], w[6], w[7]],
        diffuse: [w[8], w[9], w[10], w[11]],
        specular: [w[12], w[13], w[14], w[15]],
        reflectivity: w[16],
        opacity: w[17],
        refraction: w[18],
        existence: w[19],
    };
    // the material, the lights: read from the case loaded into an image (eighth pass)
    let loaded = outer::Loaded::of(c);
    let _ = block(&work);
    st.materials.push(si_render::inimage::material_at(&loaded.m, loaded.at("mat")));
    st.models.push(ShadeModel { flags: bits(c.input("model"), 133) as u16, ..ShadeModel::default() });
    st.lights = vec![light_in(&loaded, c, "l0"), light_in(&loaded, c, "l1")];
    st.light_head = if matches!(c.input("g_lights").words()[0], Word::Ref { .. }) { Some(0) } else { None };
    let g = &mut st.glb;
    g.fade = bits(c.input("g_fade"), 0) as i16;
    let fp = c.input("g_fadep").f32s();
    (g.fade_start, g.fade_end, g.fade_scale) = (fp[0], fp[2], fp[3]);
    let bgw = c.input("g_bg").f32s();
    g.background = Rgbazo { r: bgw[0], g: bgw[1], b: bgw[2], a: bgw[3], z: bgw[4], o: bgw[5] };
    g.fog = bits(c.input("g_fog"), 0) as i16;
    let fog = c.input("fog").f32s();
    (g.fog_density, g.fog_base, g.fog_thickness, g.fog_start, g.fog_end) = (fog[0], fog[1], fog[2], fog[3], fog[4]);
    let fc = c.input("fogc").f32s();
    g.fog_color = [fc[0], fc[1], fc[2]];
    g.dof = c.input("g_dof").i32();
    let dz = c.input("g_dofz").f32s();
    (g.dof_z0, g.dof_z1, g.dof_z2) = (dz[0], dz[2], dz[4]);
    g.dof_scale = c.input("g_dofs").i32();
    let sw = c.input("g_sw");
    g.textures = bits(sw, 0) as i16;
    g.textures_2d = bits(sw, 1) as i16;
    g.textures_3d = bits(sw, 2) as i16;
    g.reflection = bits(sw, 3) as i16;
    g.refraction = bits(sw, 4) as i16;
    g.shadows = bits(sw, 5) as i16;
    g.max_depth = bits(c.input("g_depth"), 0) as i16;
    g.no_spec_alpha = bits(c.input("g_nsa"), 0) as i16;
    st.camera.world_to_raw_mat = matrix(c.input("mat4"), 0);
    let mut ray = Ray { tn: Some(0), w38: bits(r, 14), w3c: bits(r, 15) as i16, w3e: bits(r, 16) as i16, w40: bits(r, 17), ..ray_of(r) };
    let b = c.input("bg").f32s();
    let bg = Rgbazo { r: b[0], g: b[1], b: b[2], a: b[3], z: b[4], o: b[5] };
    prepare(&mut st, c);
    let out = shade(&mut st, env, &mut ray, bg, c.input("primary").i16());
    let got = [out.r, out.g, out.b, out.a, out.z, out.o];
    let want = c.output("ret").written();
    for k in 0..if five { 5 } else { 6 } {
        if let Some(w) = want[k] {
            if w as u32 != got[k].to_bits() {
                return Verdict::Different(format!("result[{k}]: original {:08x}, port {:08x} (model {})", w as u32, got[k].to_bits(), st.materials[0].model));
            }
        }
    }
    let after = c.output("ray");
    for (i, v) in [(10, ray.hit.x.to_bits()), (11, ray.hit.y.to_bits()), (12, ray.hit.z.to_bits()), (17, ray.w40)] {
        if bits(after, i) != v {
            return Verdict::Different(format!("ray word {i}: original {:08x}, port {v:08x}", bits(after, i)));
        }
    }
    let mat_after = c.output("mat");
    let w = st.materials[0].work;
    let port: Vec<f32> = [&w.base_ambient[..], &w.ambient[..], &w.diffuse[..], &w.specular[..], &[w.reflectivity, w.opacity, w.refraction, w.existence][..]].concat();
    for (k, v) in port.iter().enumerate() {
        if bits(mat_after, 449 + k) != v.to_bits() {
            return Verdict::Different(format!("material word +0x{:x}: original {:08x}, port {:08x}", 0x700 + 4 * k, bits(mat_after, 449 + k), v.to_bits()));
        }
    }
    Verdict::Same
}

/// The cases `check_dir` leaves out because some output word is uninitialised memory in the
/// original (the sixth result float of `SI_shade` and of `colSubdivid`, and what they pass
/// on): compared here on every other word. Such a word is printed as unwritten in the
/// vector, and the checks skip unwritten words.
fn uninit_pass(dir: &Path, file: &str, check: Check) {
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{file}.vec"))) else { return };
    let (mut cases, mut same, mut diff) = (0, 0, 0);
    for line in text.lines() {
        let Ok(Some(c)) = Case::parse(line) else { continue };
        // (a case that called the C library's mathematics is compared given its answers:
        // the checks of the scenes of meshes replay them themselves)
        let replays = file.starts_with("SI_update#") || file == "si_lights" || file.starts_with("SI_collect#face") || file.starts_with("SI_collect#patch");
        if c.notes.iter().any(|n| n.starts_with("STOP") || (n.starts_with("NA") && !replays)) || !c.notes.iter().any(|n| n.starts_with("UNINIT:")) {
            continue;
        }
        cases += 1;
        match std::panic::catch_unwind(|| check(&c)) {
            Ok(Verdict::Same) => same += 1,
            Ok(Verdict::Different(e)) => {
                diff += 1;
                if diff <= 3 {
                    println!("    {e}");
                }
            }
            // the check does not compare the case (it says why where it decides so)
            Ok(Verdict::Skip) => cases -= 1,
            _ => diff += 1,
        }
    }
    println!("{:<46} {:>6} {:>9} {:>9}", format!("{file} (uninitialised words left out)"), cases, same, diff);
}

const NODES: [&str; 6] = ["root", "c1", "c2", "out", "lo", "hi"];

/// The index a pointer word stands for: `&name` among `names`, or a numbered buffer
/// `prefix` + digit.
fn named(w: &Word, names: &[&str]) -> Option<usize> {
    match w {
        Word::Ref { name, .. } => names.iter().position(|n| n == name),
        _ => None,
    }
}

fn numbered(w: &Word, prefix: &str) -> Option<usize> {
    match w {
        Word::Ref { name, .. } => name.strip_prefix(prefix)?.parse().ok(),
        _ => None,
    }
}

/// The made-up tree of `gen_render.py`: a root, two cells, the outside node, two wall nodes;
/// six links, three triangles of three corners each.
fn trace_scene(c: &Case) -> (Bsp, Vec<Tn>, Vec<RVertex>) {
    let node = |name: &str| {
        let f = c.input(name);
        let w = f.words();
        let (kind, level, mailbox) = (bits(f, 0) as i16, bits(f, 1) as i16, bits(f, 2) as i32);
        if name.starts_with('c') || name == "out" {
            Tree { kind, level, mailbox, list: numbered(&w[3], "l"), walls: core::array::from_fn(|i| named(&w[4 + i], &NODES)), ..Tree::default() }
        } else {
            Tree { kind, level, mailbox, split: fl(f, 3), low: named(&w[4], &NODES), high: named(&w[5], &NODES), ..Tree::default() }
        }
    };
    let links = (0..6)
        .map(|k| {
            let f = c.input(&format!("l{k}"));
            Link { tn: numbered(&f.words()[6], "tn").unwrap_or(0), next: numbered(&f.words()[7], "l"), ..Link::default() }
        })
        .collect();
    let mut tns = Vec::new();
    let mut verts = Vec::new();
    for k in 0..3 {
        let t = c.input(&format!("tn{k}"));
        tns.push(Tn {
            v: [Some(3 * k), Some(3 * k + 1), Some(3 * k + 2)],
            n: vec3(t, 3),
            d: fl(t, 6),
            inv_d: fl(t, 7),
            axis: bits(t, 8) as i16,
            inv_n: fl(t, 10),
            edge: [vec3(t, 11), vec3(t, 14), vec3(t, 17)],
            flags: bits(t, 21) as u16,
            ..Tn::default()
        });
        for j in 0..3 {
            verts.push(RVertex { world: vec3(c.input(&format!("t{k}v{j}")), 8), ..RVertex::default() });
        }
    }
    (Bsp { nodes: NODES.iter().map(|n| node(n)).collect(), links, root: 0, outside: Some(3) }, tns, verts)
}

fn cell_ray(c: &Case) -> Ray {
    let f = c.input("ray");
    Ray { w38: named(&f.words()[14], &NODES).expect("the ray names its cell") as u32, w3c: bits(f, 15) as i16, w3e: bits(f, 16) as i16, w40: bits(f, 17), ..ray_of(f) }
}

fn check_push(c: &Case) -> Verdict {
    let (bsp, _, _) = trace_scene(c);
    let mut ray = cell_ray(c);
    let (node, exit) = push(&bsp.nodes, &mut ray);
    let want = named(&c.output("ptr").words()[0], &NODES);
    if want != Some(node) {
        return Verdict::Different(format!("node: original {want:?}, port {node}"));
    }
    if !c.output("exit").same_f32(&[exit.x, exit.y, exit.z]) {
        return Verdict::Different(format!("exit point: port {exit:?}"));
    }
    Verdict::from(bits(c.output("ray"), 9) == ray.t.to_bits(), || format!("ray distance: port {:?}", ray.t))
}

fn check_trace(c: &Case) -> Verdict {
    let (mut bsp, tns, verts) = trace_scene(c);
    let mut ray = cell_ray(c);
    let mut mailbox = Mailbox { current: c.input("g_mb").i32() };
    let hit = trace(&mut bsp, &mut mailbox, &mut ray, &tns, &verts, usize::MAX);
    if c.output("eax").u32() != hit as u32 {
        return Verdict::Different(format!("eax {}, port {hit}", c.output("eax").u32()));
    }
    let after = c.output("ray");
    let port = [ray.v18.x, ray.v18.y, ray.v18.z, ray.t, ray.hit.x, ray.hit.y, ray.hit.z];
    for (k, v) in port.iter().enumerate() {
        if bits(after, 6 + k) != v.to_bits() {
            return Verdict::Different(format!("ray word {}: original {:08x}, port {:08x}", 6 + k, bits(after, 6 + k), v.to_bits()));
        }
    }
    let w = after.words();
    if numbered(&w[13], "tn") != ray.tn {
        return Verdict::Different(format!("triangle: original {:?}, port {:?}", w[13], ray.tn));
    }
    if named(&w[14], &NODES) != Some(ray.w38 as usize) {
        return Verdict::Different(format!("cell: original {:?}, port {}", w[14], ray.w38));
    }
    Verdict::from(c.output("g_mb").i32() == mailbox.current, || format!("ray number: port {}", mailbox.current))
}

/// `SI_Output` is run with the file, the depth file, the display, the dither, the
/// incrustation and the progress text switched off: it must call nothing.
struct NoSink;
impl LineSink for NoSink {
    fn dither_scan(&mut self, _: &[Rgbazo], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [f32]) {
        panic!("SI_fsDitherScan called")
    }
    fn incrustation(&mut self, _: &mut OutputLine, _: i32) {
        panic!("si_incrustration called")
    }
    fn rle_write(&mut self, _: i32, _: i32, _: &OutputLine) {
        panic!("SI_rleWrite called")
    }
    fn depth_write(&mut self, _: &[f32]) {
        panic!("depth file written")
    }
    fn fb_write(&mut self, _: i16) {
        panic!("SI_fbWrite called")
    }
    fn status(&mut self, _: &str) {
        panic!("progress text")
    }
}

fn samples_of(f: &Field) -> Vec<Rgbazo> {
    f.f32s().chunks(6).map(|p| Rgbazo { r: p[0], g: p[1], b: p[2], a: p[3], z: p[4], o: p[5] }).collect()
}

fn out_state(c: &Case) -> OutputState {
    let gx = c.input("g_x").i32s();
    let blank = OutputLine { red: vec![0; 32], green: vec![0; 32], blue: vec![0; 32], alpha: vec![0; 32] };
    OutputState { x0: gx[0], x1: gx[1], red: c.input("r").u8s(), green: c.input("g").u8s(), blue: c.input("b").u8s(), alpha: c.input("a").u8s(), depth: c.input("z").f32s(), line: blank, y: c.input("g_y").i16(), width: c.input("g_w").i16(), ..OutputState::default() }
}

/// What `SI_Output` left: the four byte lines and the depth line, the first `copied` bytes
/// of the four outgoing lines (the rest must be unwritten), and the line counter.
fn same_output(c: &Case, st: &OutputState, copied: usize) -> Result<(), String> {
    for (name, got) in [("r", &st.red), ("g", &st.green), ("b", &st.blue), ("a", &st.alpha)] {
        if &c.output(name).u8s() != got {
            return Err(format!("byte line {name}: original {:?}, port {got:?}", c.output(name).u8s()));
        }
    }
    if !c.output("z").same_f32(&st.depth) {
        return Err("depth line".into());
    }
    for (name, got) in [("or", &st.line.red), ("og", &st.line.green), ("ob", &st.line.blue), ("oa", &st.line.alpha)] {
        for (k, w) in c.output(name).written().iter().enumerate() {
            let want = w.map(|v| v as u8);
            let port = if k < copied { Some(got[k]) } else { None };
            if want != port {
                return Err(format!("outgoing line {name}[{k}]: original {want:?}, port {port:?}"));
            }
        }
    }
    if c.output("g_y").i16() != st.y {
        return Err(format!("line counter: original {}, port {}", c.output("g_y").i16(), st.y));
    }
    Ok(())
}

fn check_output(c: &Case) -> Verdict {
    let mut st = out_state(c);
    let width = c.input("width").i32();
    output(&mut st, &mut NoSink, &samples_of(c.input("samples")), c.input("xoff").i32(), c.input("yoff").i32(), width);
    verdict(same_output(c, &st, width as usize))
}

fn check_antialias(c: &Case) -> Verdict {
    let f = c.input("g_filter").f32s();
    let blank = samples_of(c.input("g_blank"))[0];
    let mut aa = Antialias { size: c.input("g_size").i32(), filter: core::array::from_fn(|i| core::array::from_fn(|j| f[11 * i + j])), top: samples_of(c.input("top")), bot: samples_of(c.input("bot")), fc3: c.input("g_fc3").i32(), fc4: c.input("g_fc4").i32(), blank };
    let line = samples_of(c.input("line"));
    let xs = c.input("g_xs").i32s();
    let mut st = out_state(c);
    let (sx, sy, sres) = (c.input("g_sx").i32(), c.input("g_sy").i32(), c.input("g_sres").i32());
    let mut rows = 0;
    aa.antialias(&line, xs[0], xs[1], st.x0, &mut |row| {
        rows += 1;
        output(&mut st, &mut NoSink, row, sx, sy, sres)
    });
    for (name, got) in [("top", &aa.top), ("bot", &aa.bot)] {
        let flat: Vec<f32> = got.iter().flat_map(|p| [p.r, p.g, p.b, p.a, p.z, p.o]).collect();
        if !c.output(name).same_f32(&flat) {
            return Verdict::Different(format!("pixel row {name} (level {})", (aa.size + 1) / 2));
        }
    }
    if (c.output("g_fc3").i32(), c.output("g_fc4").i32()) != (aa.fc3, aa.fc4) {
        return Verdict::Different(format!("row counters: port {} {}", aa.fc3, aa.fc4));
    }
    verdict(same_output(c, &st, if rows > 0 { sres as usize } else { 0 }))
}

fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_Output" => check_output,
        "antialias" => check_antialias,
        "push" => check_push,
        "si_trace" => check_trace,
        "SI_shade" | "SI_shade#spotedge" => |c: &Case| shade_verdict(c, false),
        "findHyperfocal" => |c: &Case| st0(c, camera::find_hyperfocal(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findNear" => |c: &Case| st0(c, camera::find_near(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findFar" => |c: &Case| st0(c, camera::find_far(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findOutoFocus" => |c: &Case| st0(c, camera::find_outo_focus(c.input("a").f32(), c.input("b").f32(), c.input("c").f32(), c.input("d").f32())),
        "findCoc" | "findCoc#lens" => |c: &Case| st0(c, F80::from_f32(camera::find_coc(c.input("a").f32(), c.input("b").f32(), c.input("c").f32()))),
        "SI_Intersect" => |c: &Case| check_intersect(c, false),
        "SI_Intersect#excluded" => |c: &Case| check_intersect(c, true),
        "SI_Intersect1" => |c: &Case| {
            let (tns, verts) = triangle(c);
            let mut ray = ray_of(c.input("ray"));
            let (r, _) = intersect1(&mut ray, &slice_of(c), &tns, &verts, c.input("tmin").f32());
            if c.output("eax").u32() != r as u32 {
                return Verdict::Different(format!("eax {}, port {r}", c.output("eax").u32()));
            }
            if !matches!(c.output("out").words()[0], Word::Ref { .. }) {
                return Verdict::Different("the original did not write the triangle through its fourth argument".into());
            }
            verdict(same_ray(c, &ray))
        },
        "SI_Intersect2" => |c: &Case| {
            let (tns, verts) = triangle(c);
            let mut ray = ray_of(c.input("ray"));
            let r = intersect2(&mut ray, &slice_of(c), &tns, &verts);
            if c.output("eax").u32() != r as u32 {
                return Verdict::Different(format!("eax {}, port {r}", c.output("eax").u32()));
            }
            // this routine leaves the weights as they were, which the comparison covers
            verdict(same_ray(c, &ray))
        },
        "si_search" => |c: &Case| {
            let names = ["root", "n1", "n2", "n3", "n4", "n5", "n6"];
            let index = |w: &Word| match w {
                Word::Ref { name, .. } => names.iter().position(|n| n == name),
                _ => None,
            };
            let nodes: Vec<Tree> = names
                .iter()
                .map(|n| {
                    let f = c.input(n);
                    Tree { kind: bits(f, 0) as i16, split: fl(f, 3), low: index(&f.words()[4]), high: index(&f.words()[5]), ..Tree::default() }
                })
                .collect();
            let p = c.input("p").f32s();
            let got = search(&nodes, 0, Vector::new(p[0], p[1], p[2]));
            let out = c.output("ptr");
            let want = if out.is_null() { None } else { index(&out.words()[0]) };
            Verdict::from(got == want, || format!("original {want:?}, port {got:?}"))
        },
        "makeFilter" => |c: &Case| {
            // a pattern no weight can have marks what the port did not write
            let mark = f32::from_bits(0x7fa5_a5a5);
            let mut rows = vec![vec![mark; 11]; 11];
            camera::make_filter(&mut rows, c.input("radius").f32(), c.input("size").i32());
            for (i, row) in rows.iter().enumerate() {
                let want = c.output(&format!("r{i}")).written();
                for (j, v) in row.iter().enumerate() {
                    let got = if v.to_bits() == mark.to_bits() { None } else { Some(v.to_bits() as u128) };
                    if got != want[j] {
                        return Verdict::Different(format!("[{i}][{j}]: original {:?}, port {got:?}", want[j]));
                    }
                }
            }
            Verdict::Same
        },
        "layerFog" => |c: &Case| {
            let mut st = ShadeState::default();
            let (fog, fc, m) = (c.input("fog").f32s(), c.input("fogc").f32s(), c.input("mat").f32s());
            st.glb.fog_density = fog[0];
            st.glb.fog_base = fog[1];
            st.glb.fog_thickness = fog[2];
            st.glb.fog_start = fog[3];
            st.glb.fog_end = fog[4];
            st.glb.fog_color = [fc[0], fc[1], fc[2]];
            st.camera.world_to_raw_mat = core::array::from_fn(|i| core::array::from_fn(|j| m[4 * i + j]));
            let ray = ray_of(c.input("ray"));
            let p = c.input("p").f32s();
            let before = c.input("col").f32s();
            let mut col = [before[0], before[1], before[2]];
            layer_fog(&st, &ray, Vector::new(p[0], p[1], p[2]), &mut col);
            let want = c.output("col").written();
            for k in 0..3 {
                let expect = want[k].map(|v| v as u32).unwrap_or(before[k].to_bits());
                if expect != col[k].to_bits() {
                    return Verdict::Different(format!("colour[{k}]: original {expect:08x}, port {:08x}", col[k].to_bits()));
                }
            }
            Verdict::Same
        },
        _ => return chunk::lookup(name).or_else(|| scene::lookup(name)).or_else(|| out::lookup(name)).or_else(|| prep::lookup(name)).or_else(|| mesh::lookup(name)).or_else(|| light::lookup(name)).or_else(|| tex::lookup(name)).or_else(|| mat::lookup(name)).or_else(|| outer::lookup(name)).or_else(|| pass9::lookup(name)).or_else(|| pass10::lookup(name)).or_else(|| pass11::lookup(name)).or_else(|| app2::lookup(name)),
    })
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: render-emucheck <directory of .vec files>");
        std::process::exit(2);
    };
    match check_dir(Path::new(&dir), &lookup) {
        Ok(rows) => {
            let ok = print_report(&rows);
            println!("cases in which the original returns a word it never set, compared on the rest:");
            uninit_pass(Path::new(&dir), "SI_shade", |c: &Case| shade_verdict(c, true));
            for file in ["SI_shade#rt", "SI_shade#shadow", "SI_Sample", "SI_Sample#rt", "SI_Sample#shadow", "si_sTrace", "colSubdivid", "colSubdivid#frame", "dofSubdivid"] {
                uninit_pass(Path::new(&dir), file, scene::lookup(file).unwrap());
            }
            uninit_pass(Path::new(&dir), "si_rleInit", out::lookup("si_rleInit").unwrap());
            uninit_pass(Path::new(&dir), "SI_shade#tex", mat::lookup("SI_shade#tex!five").unwrap());
            scene::frame_pass(Path::new(&dir));
            let mut stops = (0, 0);
            for shape in ["fan", "holes", "odd", "outline", "sheet", "soup", "souph"] {
                let file = format!("SI_collect#{shape}");
                uninit_pass(Path::new(&dir), &file, mesh::lookup("SI_collect").unwrap());
                let s = mesh::stopped_pass(Path::new(&dir), &file);
                stops = (stops.0 + s.0, stops.1 + s.1);
            }
            uninit_pass(Path::new(&dir), "SI_metaTriangles", mesh::lookup("SI_metaTriangles").unwrap());
            uninit_pass(Path::new(&dir), "si_lights", light::lookup("si_lights").unwrap());
            for file in ["SI_update#mesh", "SI_update#meshframe", "SI_update#lit", "SI_update#dof"] {
                uninit_pass(Path::new(&dir), file, mesh::lookup(file).unwrap());
            }
            uninit_pass(Path::new(&dir), "SI_update#mat", pass9::lookup("SI_update#mat").unwrap());
            uninit_pass(Path::new(&dir), "SI_update#face", pass9::lookup("SI_update#face").unwrap());
            uninit_pass(Path::new(&dir), "SI_update#patch", pass9::lookup("SI_update#patch").unwrap());
            for file in ["si_rleInit#keep", "si_rleInit#frame"] {
                uninit_pass(Path::new(&dir), file, pass10::lookup(file).unwrap());
            }
            println!("SI_collect: {} cases stop in the original on a read through a null pointer; the port refuses {} of them", stops.0, stops.1);
            mesh::refused_report();
            pass9::report();
            pass10::report();
            if !ok {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("render-emucheck: {e}");
            std::process::exit(2);
        }
    }
}
