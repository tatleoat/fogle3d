// SPDX-License-Identifier: MIT
//! Checks of the ninth pass (finding 0012, section 18), on the vectors of
//! `re/tools/si-render/emu/gen_pass9.py`.
//!
//! `SI_update#mat`: the original's `SI_update` stepped whole on scenes whose surfaces come
//! from material and texture records, with `SI_materials`, `SI_getMapsList`, `SI_textures`
//! and `updateTxt3DTransf` running, and the frame from it. The case is loaded into an image;
//! the port's scene is read from the image (`si_render::adapter::build_scene`), the ported
//! `SI_update` runs on it, what it made of the materials, textures and pictures is stored
//! back into the image (`si_render::adapter::write_back`), and the image's records are held
//! against the original's, word for word. Then the prepared geometry and the frame, as the
//! checks of the scenes of meshes compare them.
use crate::mesh::{same_scene_of, skip_or};
use crate::outer::Loaded;
use crate::prep::{mutate, same_words};
use si_mem::{Mem, Names};
use si_model::mesh::Mesh;
use si_render::adapter::{build_scene, write_back, Placed};
use si_render::frame::{Job, PictureFiles};
use si_render::geometry::{Geometry, GeometryEnv};
use si_render::img::{dict, f, g, Outside};
use si_render::jitter::Drand48;
use si_render::shade::ShadeState;
use si_render::softtriangle::{NodeDef, Scene, Tessellators};
use si_render::softvertex::{MatRef, MetaSurface, NodeRender};
use si_render::update::{update, Prepared, UpdateEnv};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The three kinds of scene: all meshes; faces among them; patches among them.
const FACE: usize = 1;
const PATCH: usize = 2;
const NAMES: [&str; 3] = ["SI_update#mat", "SI_update#face", "SI_update#patch"];

/// The state the harness's `drand48` starts every case from (the C library's own start).
const DRAND_START: u64 = 0x1234_abcd_330e;
/// The globals of a case that the scene is read through, with their addresses (the
/// generator's spec line gives them; a vector's case only names them).
const GLOBALS: [(&str, u32, u32); 14] = [
    ("procdata", 0x0867_9314, 4),
    ("elemdicts", 0x0867_931c, 4),
    ("cid_pic", 0x0878_1670, 4),
    ("cid_mat", 0x0878_167c, 4),
    ("cidm", 0x0878_1680, 4),
    ("cid_map", 0x0878_16c8, 4),
    ("cid_t3d", 0x0878_16cc, 4),
    ("g_lights", 0x0879_07e4, 4),
    ("g_rootm", 0x0879_07e8, 4),
    ("g_graf", 0x0867_9498, 4),
    ("g_stats", 0x0879_11e0, 68),
    ("g_prev", 0x0869_bc8a, 2),
    ("g_opt", 0x0879_1640, 2),
    ("g_rt", 0x0879_0c7c, 2),
];

/// The picture files of a case, as the harness's file system holds them.
pub struct CaseFiles<'a>(pub &'a Case);
impl PictureFiles for CaseFiles<'_> {
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        let name = String::from_utf8_lossy(name);
        let path = if default {
            "/res/noIcon.pic".to_string()
        } else if name.starts_with('/') {
            format!("{name}.pic")
        } else {
            format!("/tex/{name}.pic")
        };
        self.0.try_input(&format!("file:{path}")).map(|f| f.to_bytes())
    }
}

/// The converters of the kinds a case answers with recorders: asked for, nothing made.
#[derive(Default)]
pub struct Recorders(pub Vec<&'static str>);
impl Tessellators for Recorders {
    fn patch2triangle(&mut self, _: usize, _: Option<&[MatRef]>, _: i32, _: i16) -> NodeRender {
        self.0.push("si_patch2triangle");
        NodeRender::default()
    }
    fn nurbs2triangle(&mut self, _: usize, _: Option<&[MatRef]>, _: i32, _: i16) -> NodeRender {
        self.0.push("SI_nurbs2Triangle");
        NodeRender::default()
    }
    fn face_is_its_shape(&mut self, _: usize) -> bool {
        self.0.push("SI_face2triangle");
        true
    }
    fn face_to_mesh(&mut self, _: usize, _: bool) -> Option<Mesh> {
        None
    }
    fn meta_surface(&mut self, _: usize) -> MetaSurface {
        self.0.push("SI_metaTriangles");
        MetaSurface::default()
    }
}

/// `SI_update`'s surroundings in these cases: the materials and textures are the job's (the
/// ports), `SI_collect` the port with the two projection routines; `si_initFade`,
/// `si_initDof` and `si_lights` are recorded in the original and do nothing here.
struct MatScene<'a> {
    job: Job<'a>,
    tess: Recorders,
    box_fill: u32,
}

impl GeometryEnv for MatScene<'_> {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.job.scene.collect(st, g, node, &mut self.tess);
        if self.job.scene.nodes[node].box_only != 0 && self.box_fill != 0 {
            let m = &g.models[node];
            for v in &mut st.verts[m.verts.unwrap_or(0)..m.verts.unwrap_or(0) + m.vert_count.max(0) as usize] {
                v.w44 = self.box_fill;
                v.w00 = [self.box_fill; 2];
            }
        }
    }
}

impl UpdateEnv for MatScene<'_> {
    fn status_message(&mut self, text: &str) {
        self.job.notes.push(text.to_string());
    }
    fn materials(&mut self, p: &mut Prepared) {
        UpdateEnv::materials(&mut self.job, p);
    }
    fn maps_list(&mut self, p: &mut Prepared) {
        UpdateEnv::maps_list(&mut self.job, p);
    }
    fn textures(&mut self, p: &mut Prepared) {
        UpdateEnv::textures(&mut self.job, p);
    }
    fn update_txt3d_transf(&mut self, p: &mut Prepared) {
        UpdateEnv::update_txt3d_transf(&mut self.job, p);
    }
}

/// The generator's stand-ins for the dictionary's two picture routines, on the image.
struct Dictionary;
impl Outside for Dictionary {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        let chapter = |m: &Mem| m.u32(args[0] + dict::DICT_LISTS) + dict::LIST_SIZE * m.u32(g::CID_PICTURES);
        match addr {
            f::SI_ELEMDICT_ADD_PICTURE => {
                let ch = chapter(m);
                let count = m.u32(ch + dict::LIST_COUNT);
                if count < 24 {
                    let e = m.u32(ch + dict::LIST_ELEMS) + dict::ELEM_SIZE * count;
                    m.set_u32(e + dict::ELEM_DATA, args[2]);
                    m.set_u32(ch + dict::LIST_COUNT, count + 1);
                }
                count
            }
            f::SI_ELEMDICT_PICTURE_ELEM_REMOVE => {
                let ch = chapter(m);
                for i in 0..m.u32(ch + dict::LIST_COUNT).min(24) {
                    let e = m.u32(ch + dict::LIST_ELEMS) + dict::ELEM_SIZE * i;
                    if m.u32(e + dict::ELEM_DATA) == args[1] && m.i16(e + dict::ELEM_UNUSED) == 0 {
                        m.set_i16(e + dict::ELEM_UNUSED, 1);
                        return 1;
                    }
                }
                0
            }
            _ => 0,
        }
    }
}

/// Copies the globals a case names to their addresses (those it has).
fn place_globals(m: &mut Mem, names: &Names) {
    for (name, addr, len) in GLOBALS {
        if let Some(at) = names.addr(m, name) {
            let bytes = m.bytes(at, len).to_vec();
            m.set_bytes(addr, &bytes);
        }
    }
}

/// One run of the port on a case.
struct Ran {
    p: Prepared,
    nmap: usize,
    scene: Scene,
    placed: Placed,
    m: Mem,
    names: Names,
    notes: Vec<String>,
    got: Result<(), &'static str>,
}

fn run(c: &Case, unset: f32, fill: Option<u32>, variant: usize) -> Ran {
    si_x87::trig::replay::start(c.tape());
    let Loaded { mut m, names } = Loaded::of(c);
    place_globals(&mut m, &names);
    let (mut p, nmap) = crate::prep::scene_case(c);
    crate::prep::shading_of(c, &mut p, unset);
    let a = c.input("g_amb").f32s();
    p.st.glb.ambience = [a[0], a[1], a[2]];
    p.st.glb.w_dec = c.input("g_dec").i16();
    p.st.tns.clear();
    p.st.verts.clear();
    p.st.uvs.clear();
    let mut files = CaseFiles(c);
    let (mut job, mut placed) = build_scene(&mut p, &mut files, &m);
    // (a vector answers the converters it does not test with recorders: what a node of
    // kind 1 or 2 points to is then no patch and no face)
    for node in &mut job.scene.nodes {
        if variant != FACE && matches!(node.def, NodeDef::FaceOf { .. }) {
            node.def = NodeDef::Face;
        }
        if variant != PATCH && matches!(node.def, NodeDef::PatchOf { .. }) {
            node.def = NodeDef::Patch;
        }
    }
    job.rng = Drand48(DRAND_START);
    // (the harness's `pow` leaves `errno` alone)
    job.pow_sets_errno = false;
    let mut env = MatScene { job, tess: Recorders::default(), box_fill: fill.unwrap_or(if unset == 0.0 { 0 } else { 0x5a3c_a5c3 }) };
    let got = update(&mut p, &mut env, c.input("time").f32());
    write_back(&mut m, &mut Dictionary, &p, &env.job, &mut placed);
    let MatScene { job, .. } = env;
    let Job { scene, notes, .. } = job;
    Ran { p, nmap, scene, placed, m, names, notes, got }
}

fn text_at(m: &Mem, a: u32) -> Vec<u8> {
    if a == 0 {
        Vec::new()
    } else {
        m.cstr(a)
    }
}

/// A record of the image against the original's words. A pointer into a buffer of the case
/// must be that buffer's address here; a pointer into memory the original allocated (a
/// picture record, a name, a pixel block) must not be null here: what it points to is
/// compared by content ([`same_pictures`]). Offsets in `skip` are left out.
fn same_record(what: &str, m: &Mem, names: &Names, at: u32, original: &Field, skip: &[u32]) -> Result<usize, String> {
    let mut off = 0u32;
    let mut compared = 0;
    for w in original.words() {
        let n = w.bytes() as u32;
        if !skip.contains(&off) {
            let bytes = m.try_bytes(at + off, n).ok_or_else(|| format!("{what} +{off:#x}: not mapped"))?;
            let mut v = [0u8; 16];
            v[..n as usize].copy_from_slice(bytes);
            let got = u128::from_le_bytes(v);
            match w {
                Word::Value { value, .. } => {
                    if got != *value {
                        return Err(format!("{what} +{off:#x}: original {value:0w$x}, port {got:0w$x}", w = 2 * n as usize));
                    }
                    compared += 1;
                }
                Word::Unwritten { .. } => {}
                Word::Ref { name, offset } => match names.addr(m, name) {
                    Some(a) if !name.starts_with("heap") => {
                        if got != a.wrapping_add(*offset) as u128 {
                            return Err(format!("{what} +{off:#x}: original &{name}+{offset:#x}, port {got:08x}"));
                        }
                        compared += 1;
                    }
                    _ => {
                        if got == 0 {
                            return Err(format!("{what} +{off:#x}: original &{name}+{offset:#x}, port null"));
                        }
                    }
                },
            }
        }
        off += n;
    }
    Ok(compared)
}

/// Every record the set-up writes, as the port's write-back left it in the image.
fn same_records(c: &Case, r: &Ran) -> Result<usize, String> {
    let (m, names) = (&r.m, &r.names);
    let mut words = 0;
    // a material's two jitter records hold a pointer into chunk memory and the row to use
    // next: the tables are not stored in the image (they are compared by content)
    let jitter = [0x6b0, 0x6b4, 0x6c0, 0x6c4];
    for name in ["mat0", "mat1", "G0869dde0"] {
        words += same_record(name, m, names, names.addr(m, name).unwrap(), c.after(name), &jitter)?;
    }
    for k in 0..5 {
        let name = format!("map{k}");
        words += same_record(&name, m, names, names.addr(m, &name).unwrap(), c.after(&name), &[])?;
    }
    for name in ["t0", "t1", "x0", "x1", "lists", "e_pic", "e_mat", "e_map", "graf"] {
        words += same_record(name, m, names, names.addr(m, name).unwrap(), c.after(name), &[])?;
    }
    // of a node the flag word SI_materials writes (the render arrays are the port's own)
    for name in ["n0", "n1", "n2"] {
        let want = crate::bits(c.after(name), 133) as u16;
        let got = m.u16(names.addr(m, name).unwrap() + 0x214);
        if want != got {
            return Err(format!("{name} +0x214: original {want:#06x}, port {got:#06x}"));
        }
        words += 1;
    }
    let (want, got) = (c.after("g_rt").u16s()[0], m.u16(0x0879_0c7c));
    if want != got {
        return Err(format!("ray tracing word: original {want:#x}, port {got:#x}"));
    }
    // what SI_update keeps of SI_getMapsList: the list's head (compared with the list)
    if matches!(c.output("g_maps").words()[0], Word::Ref { .. }) != (m.u32(g::SI_DATABASE + 0x1a0) != 0) {
        return Err("the picture list's head as SI_update keeps it".into());
    }
    Ok(words + 1)
}

/// The picture list from its head: every record, its name and its pixel block; and for each
/// 2D texture which picture it has and the name `SI_setFileName` gave it.
fn same_pictures(c: &Case, r: &Ran) -> Result<[usize; 2], String> {
    let m = &r.m;
    let mut list = Vec::new();
    let mut a = m.u32(m.u32(g::SI_GRAF_LISTS) + 0x1c);
    while a != 0 && list.len() < 16 {
        list.push(a);
        a = m.u32(a + 0x24);
    }
    same_words("pictures in the list", c.output("pics"), &[list.len() as u32])?;
    let mut bytes = 0;
    for (i, &a) in list.iter().enumerate() {
        let p = m.u32(a + 0x1c);
        let (w, h) = (m.i16(a + 0x16), m.i16(a + 0x18));
        let mut hdr: Vec<u32> = (0..5).map(|k| m.u16(a + 4 + 2 * k) as u32).collect();
        hdr.extend([m.u16(a + 0x14) as u32, w as u16 as u32, h as u16 as u32, (p != 0) as u32]);
        same_words(&format!("picture {i}"), c.output(&format!("pic{i}.hdr")), &hdr)?;
        let name: Vec<u32> = text_at(m, m.u32(a)).iter().map(|b| *b as u32).collect();
        same_words(&format!("picture {i}'s name"), c.output(&format!("pic{i}.name")), &name)?;
        if let Some(block) = c.try_output(&format!("pic{i}.block")) {
            let start = p.wrapping_sub((w as u32 + 4) * 5);
            let n = block.len() as u32;
            let got = m.try_bytes(start, n).ok_or_else(|| format!("picture {i}: the pixel block is not {n} bytes"))?;
            for (k, w) in block.words().iter().enumerate() {
                if let Word::Value { value, .. } = w {
                    if *value as u8 != got[k] ^ mutate() as u8 {
                        return Err(format!("picture {i}, byte {k} of its block: original {:02x}, port {:02x}", *value as u8, got[k]));
                    }
                    bytes += 1;
                }
            }
        }
    }
    let mut info = Vec::new();
    for k in 0..5 {
        let at = r.names.addr(m, &format!("map{k}")).unwrap();
        let glb = m.u32(at + 0x24);
        let text = text_at(m, m.u32(at + 0x20));
        info.extend([list.iter().position(|a| *a == glb).map_or(u32::MAX, |i| i as u32), text.len() as u32]);
        info.extend((0..16).map(|i| text.get(i).copied().unwrap_or(0) as u32));
    }
    same_words("the textures' pictures and file names", c.output("mapinfo"), &info)?;
    Ok([list.len(), bytes])
}

/// The jitter tables `si_initJitter` made for the two materials and the default material.
fn same_jitter(c: &Case, r: &Ran) -> Result<usize, String> {
    let mut port = Vec::new();
    let mut tables = 0;
    for name in ["mat0", "mat1", "G0869dde0"] {
        let at = r.names.addr(&r.m, name).unwrap();
        let mat = r.placed.materials.iter().position(|a| *a == at).map(|i| &r.p.st.materials[i]);
        for which in 0..2 {
            let j = mat.map(|m| if which == 0 { &m.jitter_6ac } else { &m.jitter_6bc });
            match j {
                Some(j) if !j.table.is_empty() => {
                    port.extend([1, j.next as u16 as u32]);
                    port.extend(j.table.iter().flatten().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]));
                    tables += 1;
                }
                _ => port.extend([0, 0]),
            }
        }
    }
    same_words("jitter tables", c.output("jit"), &port)?;
    Ok(tables)
}

/// Whether a node shown as its box (and rendered) carries a texture of method 4, of its own
/// or of one of its materials.
fn box_with_uv_texture(r: &Ran) -> bool {
    let st = &r.p.st;
    let uv = |mut cur: Option<usize>| {
        while let Some(k) = cur {
            if st.maps[k].method == 4 {
                return true;
            }
            cur = st.maps[k].next;
        }
        false
    };
    r.scene.nodes.iter().zip(&r.p.g.models).any(|(n, m)| n.box_only != 0 && n.render.tn_count != 0 && (uv(n.maps) || m.materials.iter().any(|i| uv(st.materials[*i].maps))))
}

/// Frames left out of the comparison: a node shown as its box with a UV texture; a read
/// behind a texture's block in the original.
pub static LEFT_OUT: [[AtomicUsize; 2]; 3] = [const { [const { AtomicUsize::new(0) }; 2] }; 3];

/// Scenes with material and texture records that agree: cases, record words compared,
/// pictures in the lists, bytes of pixel blocks, jitter tables, triangles filed, frames,
/// pixels, frames with something to shade, cases with a fatal error in the original.
pub static MAT: [[AtomicUsize; 10]; 3] = [const { [const { AtomicUsize::new(0) }; 10] }; 3];
/// In the scenes with faces that agree: face nodes that gave triangles, their triangles,
/// faces that gave none.
pub static FACES: [[AtomicUsize; 3]; 3] = [const { [const { AtomicUsize::new(0) }; 3] }; 3];

/// What the frames that agree asked of the texture routines: `si_BlookUp`,
/// `SI_renderTxt2d`, `SI_renderTxt3d`, `shadowMap`.
pub static TEXTURE: [[AtomicUsize; 4]; 3] = [const { [const { AtomicUsize::new(0) }; 4] }; 3];

pub fn report() {
    for (v, name) in NAMES.into_iter().enumerate() {
        let t: Vec<usize> = TEXTURE[v].iter().map(|c| c.load(Ordering::Relaxed)).collect();
        println!("{name}, in the frames that agree: {} calls of si_BlookUp, {} of SI_renderTxt2d, {} of SI_renderTxt3d, {} of shadowMap", t[0], t[1], t[2], t[3]);
        println!(
            "{name}, frames left out: {} with a UV texture on a node shown as its box (its coordinates are memory the original never sets), {} in which the original reads behind a texture's block",
            LEFT_OUT[v][0].load(Ordering::Relaxed),
            LEFT_OUT[v][1].load(Ordering::Relaxed)
        );
        let v: Vec<usize> = MAT[v].iter().map(|c| c.load(Ordering::Relaxed)).collect();
        println!(
            "{name}: {} scenes agree ({} record words, {} pictures, {} bytes of pixel blocks, {} jitter tables, {} triangles filed); {} frames agree ({} pixels, {} frames with something to shade); {} cases left out for a fatal error in the original",
            v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9]
        );
    }
    for (v, what) in [(FACE, "face"), (PATCH, "patch")] {
        let f: Vec<usize> = FACES[v].iter().map(|c| c.load(Ordering::Relaxed)).collect();
        println!("{}, in the scenes that agree: {} {what} nodes gave {} triangles, {} gave none", NAMES[v], f[0], f[1], f[2]);
    }
}

fn check_update_mat(c: &Case) -> Verdict {
    check_scene(c, 0)
}

fn check_update_face(c: &Case) -> Verdict {
    check_scene(c, FACE)
}

fn check_update_patch(c: &Case) -> Verdict {
    check_scene(c, PATCH)
}

fn check_scene(c: &Case, variant: usize) -> Verdict {
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        // (the recorder lets the original run on where the render would have ended)
        MAT[variant][9].fetch_add(1, Ordering::Relaxed);
        return Verdict::Skip;
    }
    crate::scene::set_textured(true);
    let v = std::panic::catch_unwind(|| check_inner(c, variant)).unwrap_or_else(|p| {
        let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
        Verdict::Different(format!("the port panicked: {}", text.unwrap_or_default()))
    });
    crate::scene::set_textured(false);
    // (EMUCHECK_LIST: every case that differs, by its time argument)
    if let (Verdict::Different(e), true) = (&v, std::env::var_os("EMUCHECK_LIST").is_some()) {
        eprintln!("DIFF time={:08x} {e}", c.input("time").f32().to_bits());
    }
    v
}

fn check_inner(c: &Case, variant: usize) -> Verdict {
    // the other preparations first: the port's own is the one the replay ends with
    let others: Vec<Prepared> = if c.output("frame").is_empty() { Vec::new() } else { [None, Some(0xffff_ffff), Some(0x8040_2010), Some(0x0000_00ff)].into_iter().map(|fill| run(c, 4321.5, fill, variant).p).collect() };
    let mut r = run(c, 0.0, None, variant);
    // what the library was asked during SI_update: the port must have made every call the
    // original made up to the frame, and none of the frame's
    let marker = c.calls.iter().position(|k| k.name == "frame_begins").unwrap_or(c.calls.len());
    let before = c.calls[..marker].iter().filter(|k| k.result.bytes() == 10).count();
    let total = c.tape().len();
    let asked = si_x87::trig::replay::finish();
    let in_step = match &asked {
        Ok(()) => before == total,
        Err(e) => e.starts_with(&format!("{} of {total} recorded calls were not made", total - before)),
    };
    // (a node the port refuses, because the original leaves its arrays there, has made
    // none of its calls: such a case is not compared at all, as before)
    let refused = !r.scene.refused.is_empty() || r.scene.nodes.iter().any(|n| n.render.wild_corners != 0);
    if r.got.is_ok() && !in_step && !refused {
        return Verdict::Different(format!("the library calls of SI_update: {}", asked.err().unwrap_or_else(|| "the port made the frame's calls too".into())));
    }
    if let Err(e) = r.got {
        return Verdict::Different(format!("the port stops with {e:?}"));
    }
    if let Some(n) = r.notes.iter().find(|n| n.starts_with("fatal")) {
        return Verdict::Different(format!("the port: {n}"));
    }
    if let Some(fault) = r.m.fault() {
        return Verdict::Different(format!("the image faulted: {fault:?}"));
    }
    if mutate() {
        let at = r.names.addr(&r.m, "mat0").unwrap() + 0x700;
        let b = r.m.u8(at);
        r.m.set_u8(at, b ^ 1);
    }
    crate::prep::set_extra_chunk(c.output("jbytes").u32() as i32);
    let geometry = same_scene_of(c, &r.p, r.nmap, &r.scene);
    crate::prep::set_extra_chunk(0);
    // what the library is asked after SI_update is the frame's
    let frame_tape: Vec<_> = c.tape().into_iter().skip(before).collect();
    let pixels = c.output("frame").len() / 7;
    let filed = r.p.g.bins.accepted.max(0) as usize;
    let box_uv = pixels != 0 && box_with_uv_texture(&r);
    let res = geometry.and_then(|_| {
        let words = same_records(c, &r)?;
        for n in &r.scene.nodes {
            if matches!(n.def, NodeDef::FaceOf { .. } | NodeDef::PatchOf { .. }) && n.box_only == 0 {
                let made = n.render.tn_count.max(0) as usize;
                FACES[variant][if made != 0 { 0 } else { 2 }].fetch_add(1, Ordering::Relaxed);
                FACES[variant][1].fetch_add(made, Ordering::Relaxed);
            }
        }
        let pictures = same_pictures(c, &r)?;
        let tables = same_jitter(c, &r)?;
        for (slot, n) in MAT[variant].iter().zip([1, words, pictures[0], pictures[1], tables, filed]) {
            slot.fetch_add(n, Ordering::Relaxed);
        }
        // (the texture routines' calls are counted for the port's own rendering, the last)
        let wild = si_render::renutil::WILD_TEXELS.load(Ordering::Relaxed);
        let mark = std::cell::Cell::new([0usize; 4]);
        let calls = || -> [usize; 4] { core::array::from_fn(|i| crate::scene::TEX_CALLS[i].load(Ordering::Relaxed)) };
        if box_uv {
            // the texture coordinates of a node shown as its box are memory the original
            // never sets (SI_malloc's block): a UV texture on it has no defined colour, and
            // the two fill patterns of the harness need not show it
            LEFT_OUT[variant][0].fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        let framed = crate::prep::same_frame_with(c, r.p, others, true, &|| {
            mark.set(calls());
            si_x87::trig::replay::start(frame_tape.clone())
        });
        if si_render::renutil::WILD_TEXELS.load(Ordering::Relaxed) != wild {
            // the original read behind a texture's block (si_BlookUp, alternation with a
            // negative picture coordinate): what it got is not a value of the case
            LEFT_OUT[variant][1].fetch_add(1, Ordering::Relaxed);
            si_x87::trig::replay::start(Vec::new());
            return Ok(());
        }
        framed?;
        if pixels != 0 {
            let (now, then) = (calls(), mark.get());
            for i in 0..4 {
                TEXTURE[variant][i].fetch_add(now[i] - then[i], Ordering::Relaxed);
            }
        }
        MAT[variant][6].fetch_add(1, Ordering::Relaxed);
        MAT[variant][7].fetch_add(pixels, Ordering::Relaxed);
        MAT[variant][8].fetch_add((pixels != 0) as usize, Ordering::Relaxed);
        Ok(())
    });
    skip_or(res)
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_update#mat" => check_update_mat,
        "SI_update#face" => check_update_face,
        "SI_update#patch" => check_update_patch,
        _ => return None,
    })
}
