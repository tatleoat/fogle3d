// SPDX-License-Identifier: MIT
//! Checks of the material and texture set-up on the vectors of
//! `re/tools/si-render/emu/gen_mat.py` (finding 0012, section 13): `SI_materials`,
//! `setTxtFlag`, `updateDefaultMaterial`, `decayTable`, the picture routines and
//! `shadowMap`.
use crate::prep::mutate;
use crate::tex::{kept, map_of, material_of, put_map, put_material_setup, put_tex3d, same_image, tex3d_of, Image};
use crate::verdict;
use si_render::jitter::{decay_table, Jitter};
use si_render::materials::{
    clean_up_texture, get_maps_list, global_2d_map_crop, mark_model_texture_images, mark_texture_images, materials, process_textures, read_texture_file, reset_texture_dirty_flags, set_txt_flag,
    static_initialization, textures, update_default_material, DictElem, GlbMap, MaterialsEnv, ModelElem, Preview, SetupError, TexPixels, TextureDict, msg_no_file,
};
use crate::tex::glb_of;
use si_render::shade::{DecayTable, ShadeEnv, ShadeModel, ShadeState};
use si_render::trace::shadow_map;
use si_render::types::{Ray, Vector};
use si_x87::vectors::{Case, Check, Verdict, Word};
use si_x87::F80;

/// What the generator's stand-in of `SI_global2dMapInsert` stores at a map's +0x24.
const FAKE_GLB: u32 = 0x0bad0000;
/// A pointer of the case that is neither null nor one of its records.
const WILD: usize = 0xdead_beef;
/// No routine sets bit 15 of a material's flag word.
const UNSET_FLAGS: u16 = 0x8000;

/// The names of the records of a case, in the order the state holds them.
#[derive(Default)]
pub struct Names {
    pub materials: Vec<String>,
    pub maps: Vec<String>,
    pub tex3ds: Vec<String>,
    pub models: Vec<String>,
}

impl Names {
    pub fn of(c: &Case) -> Names {
        let mut n = Names::default();
        for f in &c.inputs {
            let digits = f.name.trim_start_matches(|ch: char| ch.is_ascii_alphabetic());
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            match &f.name[..f.name.len() - digits.len()] {
                "mat" => n.materials.push(f.name.clone()),
                "map" => n.maps.push(f.name.clone()),
                "t" => n.tex3ds.push(f.name.clone()),
                "mdl" => n.models.push(f.name.clone()),
                _ => {}
            }
        }
        if c.try_input("maps").is_some_and(|f| !f.is_null()) {
            n.maps.insert(0, "maps".into());
        }
        if c.try_input("mat").is_some_and(|f| !f.is_null()) {
            n.materials.push("mat".into());
        }
        if c.try_input("g_def").is_some() {
            n.materials.push("g_def".into());
        }
        n
    }
    fn index(list: &[String], p: Option<(&str, u32)>) -> Option<usize> {
        p.and_then(|(name, _)| list.iter().position(|n| n == name))
    }
}

/// The environment of the checks: the two picture routines as the generator's stand-ins
/// behave, `si_initJitter` recorded, `pow` from the tape without the `errno` guess.
#[derive(Default)]
pub struct MatEnv {
    pub has_path: Vec<bool>,
    pub inserts: Vec<(usize, i16)>,
    pub removes: Vec<usize>,
    pub jitters: Vec<(i32, usize, usize)>,
}

impl MaterialsEnv for MatEnv {
    fn drand48(&mut self) -> f64 {
        0.0
    }
    fn pow(&self, b: f64, e: f64) -> (F80, bool) {
        (si_x87::trig::pow(b, e), false)
    }
    fn map_insert(&mut self, st: &mut ShadeState, map: usize, frame: i16) -> bool {
        self.inserts.push((map, frame));
        if self.has_path[map] {
            st.maps[map].glb = Some(0);
        }
        self.has_path[map]
    }
    fn map_remove(&mut self, st: &mut ShadeState, map: usize) {
        self.removes.push(map);
        st.maps[map].glb = None;
    }
    fn init_jitter(&mut self, count: i32, _: &mut Jitter, material: usize, which: usize) {
        self.jitters.push((count, material, which));
    }
}

/// The state of a case's records and globals.
pub fn load(c: &Case, n: &Names) -> (ShadeState, MatEnv) {
    let mut st = ShadeState::default();
    let mut env = MatEnv::default();
    let l = crate::outer::Loaded::of(c);
    for name in &n.maps {
        let im = Image::of(c.input(name));
        let mut m = map_of(&l, name);
        m.next = Names::index(&n.maps, im.ptr(0x16c));
        m.glb = (im.ptr(0x24).is_some() || im.u32(0x24) != 0).then_some(0);
        env.has_path.push(im.ptr(0x14).is_some());
        st.maps.push(m);
    }
    for name in &n.tex3ds {
        st.tex3ds.push(tex3d_of(&l, name));
    }
    for name in &n.materials {
        let im = Image::of(c.input(name));
        let mut m = material_of(&l, name);
        m.maps = Names::index(&n.maps, im.ptr(0x6ec));
        m.tex3d = Names::index(&n.tex3ds, im.ptr(0x6fc)).map_or(0, |i| i as u32 + 1);
        // the flag word is 32 bits in the record and 16 here: a value the set-up cannot
        // produce marks a word it did not write
        m.flags = UNSET_FLAGS;
        m.reflect_map = match im.ptr(0x6f4) {
            Some(p) => Names::index(&n.maps, Some(p)),
            None if im.u32(0x6f4) != 0 => Some(WILD),
            None => None,
        };
        st.materials.push(m);
    }
    st.default_material = n.materials.iter().position(|m| m == "g_def");
    for (k, name) in n.models.iter().enumerate() {
        let im = Image::of(c.input(name));
        let count = im.i16(0x206).max(0) as usize;
        let array = c.input(&format!("marr{k}")).words();
        st.models.push(ShadeModel {
            flags: im.i16(0x214) as u16,
            maps: Names::index(&n.maps, im.ptr(0x21c)),
            tex3d: Names::index(&n.tex3ds, im.ptr(0x224)).map_or(0, |i| i as u32 + 1),
            materials: (0..count)
                .map(|i| match array.get(i) {
                    Some(Word::Ref { name, .. }) => n.materials.iter().position(|m| m == name),
                    _ => None,
                })
                .collect(),
            unused: 0,
        });
    }
    let g = &mut st.glb;
    let a = c.input("g_amb").f32s();
    g.ambience = [a[0], a[1], a[2]];
    let sw = c.input("g_sw").u16s();
    g.textures = sw[0] as i16;
    g.textures_2d = sw[1] as i16;
    g.textures_3d = sw[2] as i16;
    g.reflection = sw[3] as i16;
    g.refraction = sw[4] as i16;
    g.shadows = sw[5] as i16;
    g.w_dec = c.input("g_dec").i16();
    g.raytrace = c.input("g_rt").u16s()[0] & 1 != 0;
    g.frame = c.input("g_frame").i16();
    (st, env)
}

fn chapter(c: &Case, name: &str, list: &[String]) -> Vec<DictElem> {
    let im = Image::of(c.input(name));
    (0..im.bytes.len() / 0x84).map(|i| DictElem { data: Names::index(list, im.ptr(0x84 * i + 4)), unused: im.i16(0x84 * i + 0x48), dirty: im.bytes[0x84 * i + 0x5b] }).collect()
}

/// Every record of the case as the port left it against the original's.
pub fn same_records(c: &Case, n: &Names, st: &ShadeState) -> Result<(), String> {
    for (i, name) in n.materials.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        let kept_flags = port.u32(0x6e0);
        put_material_setup(&st.materials[i], &mut port);
        if st.materials[i].flags == UNSET_FLAGS {
            port.put_u32(0x6e0, kept_flags);
        }
        match st.materials[i].reflect_map {
            Some(WILD) => {}
            Some(m) => port.set_ptr(0x6f4, Some((n.maps[m].as_str(), 0))),
            None => port.set_ptr(0x6f4, None),
        }
        same_image(name, c.after(name), &port, &kept)?;
    }
    for (i, name) in n.maps.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        put_map(&st.maps[i], &mut port);
        // the picture record: the stand-in's mark, or null
        port.set_ptr(0x24, None);
        if st.maps[i].glb.is_some() {
            port.put_u32(0x24, FAKE_GLB);
        }
        same_image(name, c.after(name), &port, &kept)?;
    }
    for (i, name) in n.tex3ds.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        put_tex3d(&st.tex3ds[i], &mut port);
        same_image(name, c.after(name), &port, &kept)?;
    }
    for (i, name) in n.models.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        port.put_i16(0x214, st.models[i].flags as i16);
        same_image(name, c.after(name), &port, &kept)?;
    }
    let rt = c.input("g_rt").u16s()[0] | u16::from(st.glb.raytrace);
    if c.after("g_rt").u16s()[0] != rt ^ u16::from(mutate()) {
        return Err(format!("ray tracing word: original {:#x}, port {rt:#x}", c.after("g_rt").u16s()[0]));
    }
    Ok(())
}

/// The recorded calls of the three stand-ins against what the port asked its environment.
fn same_calls(c: &Case, n: &Names, env: &MatEnv) -> Result<(), String> {
    let target = |w: &Word, list: &[String]| match w {
        Word::Ref { name, offset } => list.iter().position(|x| x == name).map(|i| (i, *offset)),
        _ => None,
    };
    let inserts: Vec<(usize, i16)> = c.calls.iter().filter(|k| k.name == "SI_global2dMapInsert").map(|k| (target(&k.args[0], &n.maps).map_or(WILD, |t| t.0), k.args[1].value().unwrap() as i16)).collect();
    if inserts != env.inserts {
        return Err(format!("SI_global2dMapInsert: original {inserts:?}, port {:?}", env.inserts));
    }
    let removes: Vec<usize> = c.calls.iter().filter(|k| k.name == "SI_global2dMapRemove").map(|k| target(&k.args[0], &n.maps).map_or(WILD, |t| t.0)).collect();
    if removes != env.removes {
        return Err(format!("SI_global2dMapRemove: original {removes:?}, port {:?}", env.removes));
    }
    let jitters: Vec<(i32, usize, usize)> = c
        .calls
        .iter()
        .filter(|k| k.name == "si_initJitter")
        .map(|k| {
            let (m, off) = target(&k.args[1], &n.materials).unwrap_or((WILD, 0));
            (k.args[0].value().unwrap() as i32, m, usize::from(off == 0x6bc))
        })
        .collect();
    if jitters != env.jitters {
        return Err(format!("si_initJitter: original {jitters:?}, port {:?}", env.jitters));
    }
    Ok(())
}

/// The outcome of a set-up routine against the original's: its fatal text, or the records.
fn settle(c: &Case, n: &Names, st: &ShadeState, env: &MatEnv, got: Result<(), SetupError>) -> Verdict {
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        // the stand-in lets the original run on after its fatal error; the port has stopped
        // (and so has not made the library calls the original went on to make)
        si_x87::trig::replay::start(Vec::new());
        let text = String::from_utf8_lossy(&c.output("str0").to_bytes()).into_owned();
        return Verdict::from(text == msg_no_file() && matches!(&got, Err(SetupError::Fatal(t)) if t.starts_with(msg_no_file())), || format!("fatal: original {text:?}, port {got:?}"));
    }
    verdict((|| {
        got.map_err(|e| format!("the port stops with {e:?}, the original does not"))?;
        same_calls(c, n, env)?;
        same_records(c, n, st)
    })())
}

fn check_materials(c: &Case) -> Verdict {
    let n = Names::of(c);
    let (mut st, mut env) = load(c, &n);
    let mats = chapter(c, "e_mat", &n.materials);
    let models = chapter(c, "e_mdl", &n.models);
    let got = materials(&mut st, &mats, &models, &mut env);
    settle(c, &n, &st, &env, got)
}

fn check_set_txt_flag(c: &Case) -> Verdict {
    let n = Names::of(c);
    let (mut st, mut env) = load(c, &n);
    let head = n.maps.iter().position(|m| m == "maps");
    let material = n.materials.iter().position(|m| m == "mat");
    let got = set_txt_flag(&mut st, head, material, &mut env);
    let flags = *got.as_ref().unwrap_or(&0);
    let v = settle(c, &n, &st, &env, got.map(|_| ()));
    if !matches!(v, Verdict::Same) || c.calls.iter().any(|k| k.name == "si_fatal") {
        return v;
    }
    let want = c.after("flags").u16s()[0];
    Verdict::from(want == flags ^ u16::from(mutate()), || format!("flags: original {want:#x}, port {flags:#x}"))
}

fn check_default_material(c: &Case) -> Verdict {
    let n = Names::of(c);
    let (mut st, mut env) = load(c, &n);
    update_default_material(&mut st, &mut env);
    settle(c, &n, &st, &env, Ok(()))
}

fn check_decay_table(c: &Case) -> Verdict {
    let w = c.input("t").f32s();
    let mut t = DecayTable { exponent: w[0], pairs: (0..200).map(|i| [w[1 + 2 * i], w[2 + 2 * i]]).collect(), after: [0.0; 2] };
    decay_table(&mut t, &|b, e| (si_x87::trig::pow(b, e), false));
    let mut port = vec![t.exponent];
    port.extend(t.pairs.iter().flatten());
    let port: Vec<f32> = if mutate() { port.iter().map(|v| f32::from_bits(v.to_bits() ^ 1)).collect() } else { port };
    Verdict::from(c.after("t").same_f32(&port), || "the table differs".to_string())
}

// ---------------------------------------------------------------- pictures and regions

/// The state of a case of the texture pass: three picture records, four maps, two
/// material heads, two models, and the three chapters.
fn load_textures(c: &Case) -> (ShadeState, TextureDict, Vec<String>, Vec<String>) {
    let mut st = ShadeState::default();
    let glbs: Vec<String> = (0..3).map(|k| format!("glb{k}")).collect();
    let maps: Vec<String> = (0..4).map(|k| format!("map{k}")).collect();
    let l = crate::outer::Loaded::of(c);
    for name in &glbs {
        st.pictures.push(glb_of(&l, c, name));
    }
    for name in &maps {
        let im = Image::of(c.input(name));
        let mut m = map_of(&l, name);
        m.next = Names::index(&maps, im.ptr(0x16c));
        m.glb = Names::index(&glbs, im.ptr(0x24));
        st.maps.push(m);
    }
    for name in ["mh0", "mh1"] {
        let im = Image::of(c.input(name));
        st.materials.push(si_render::shade::ShadeMaterial { maps: Names::index(&maps, im.ptr(0x6ec)), ..Default::default() });
    }
    let array = c.input("marr0").words();
    for name in ["mdl0", "mdl1"] {
        let im = Image::of(c.input(name));
        let has_array = im.ptr(0x20c).is_some();
        st.models.push(ShadeModel {
            maps: Names::index(&maps, im.ptr(0x21c)),
            materials: (0..im.i16(0x206).max(0) as usize)
                .map(|i| match array.get(i) {
                    Some(Word::Ref { name, .. }) if has_array => Some(usize::from(name == "mh1")),
                    _ => None,
                })
                .collect(),
            ..ShadeModel::default()
        });
    }
    let elems = |name: &str, list: &[String]| chapter(c, name, list);
    let models = {
        let im = Image::of(c.input("e_mdl"));
        let names = ["mdl0".to_string(), "mdl1".to_string()];
        (0..2).map(|i| ModelElem { elem: DictElem { data: Names::index(&names, im.ptr(0x84 * i + 4)), unused: im.i16(0x84 * i + 0x48), dirty: 0 }, w24: im.u32(0x84 * i + 0x24) }).collect()
    };
    let dict = TextureDict { pictures: elems("e_pic", &glbs), maps: elems("e_map", &maps), models };
    (st, dict, glbs, maps)
}

/// Records what `SI_processTextures` asks to be read.
#[derive(Default)]
struct ReadEnv(Vec<(usize, i16)>);
impl MaterialsEnv for ReadEnv {
    fn drand48(&mut self) -> f64 {
        0.0
    }
    fn read_texture(&mut self, st: &mut ShadeState, picture: usize, count: i16) -> Result<(), SetupError> {
        self.0.push((picture, count));
        st.pictures[picture].pixels = Some(TexPixels::default());
        Ok(())
    }
}

fn same_texture_state(c: &Case, st: &ShadeState, dict: &TextureDict, glbs: &[String], maps: &[String]) -> Result<(), String> {
    for (i, name) in glbs.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        if port.ptr(0x1c).is_none() {
            port.put_u32(0x1c, if st.pictures[i].pixels.is_some() { FAKE_GLB } else { 0 });
        } else if st.pictures[i].pixels.is_none() {
            port.set_ptr(0x1c, None);
        }
        same_image(name, c.after(name), &port, &kept)?;
    }
    for (i, name) in maps.iter().enumerate() {
        let mut port = Image::of(c.input(name));
        put_map(&st.maps[i], &mut port);
        same_image(name, c.after(name), &port, &kept)?;
    }
    for (name, list) in [("e_pic", &dict.pictures), ("e_map", &dict.maps)] {
        let mut port = Image::of(c.input(name));
        for (i, e) in list.iter().enumerate() {
            port.bytes[0x84 * i + 0x5b] = e.dirty;
        }
        same_image(name, c.after(name), &port, &kept)?;
    }
    same_image("e_mdl", c.after("e_mdl"), &Image::of(c.input("e_mdl")), &kept)
}

fn check_textures(c: &Case, which: &str) -> Verdict {
    let (mut st, mut dict, glbs, maps) = load_textures(c);
    let preview = Preview { selection_call: c.input("g_prev").i16(), optimize: c.input("g_opt").i16() };
    let mut env = ReadEnv::default();
    let no_dict = c.try_input("dict").is_some_and(|f| f.is_null());
    let got = match which {
        "SI_textures" => textures(&mut st, &mut dict, c.input("load").i16(), preview, &mut env),
        "SI_processTextures" => process_textures(&mut st, &mut dict, c.input("load").i16(), preview, &mut env),
        "resetTextureDirtyFlags" if !no_dict => {
            reset_texture_dirty_flags(&mut dict);
            Ok(())
        }
        "markModelTextureImages" if !no_dict => mark_model_texture_images(&st, &mut dict),
        "markTextureImages" => {
            let first = match &c.input("first").words()[0] {
                Word::Ref { name, .. } => maps.iter().position(|m| m == name),
                _ => None,
            };
            mark_texture_images(&st, &mut dict, first);
            Ok(())
        }
        "si_cleanUpTexture" => {
            clean_up_texture(&mut st, &dict.pictures);
            Ok(())
        }
        _ => Ok(()),
    };
    verdict((|| {
        got.map_err(|e| format!("the port stops with {e:?}"))?;
        let asked: Vec<(usize, i16)> = c
            .calls
            .iter()
            .filter(|k| k.name == "SI_readTextureFile")
            .map(|k| (match &k.args[0] { Word::Ref { name, .. } => glbs.iter().position(|g| g == name).unwrap_or(WILD), _ => WILD }, k.args[1].value().unwrap() as i16))
            .collect();
        if asked != env.0 {
            return Err(format!("SI_readTextureFile: original {asked:?}, port {:?}", env.0));
        }
        same_texture_state(c, &st, &dict, &glbs, &maps)
    })())
}

/// The bytes of a pixel block against the original's, where the original wrote.
fn same_block(c: &Case, got: Option<&TexPixels>) -> Result<(), String> {
    let Some(want) = c.try_output("block") else {
        return if got.is_none() || c.try_output("glb").is_none() { Ok(()) } else { Ok(()) };
    };
    let got = got.ok_or("the port has no pixels")?;
    let w = want.written();
    if w.len() != got.block.len() {
        return Err(format!("block: original {} bytes, port {}", w.len(), got.block.len()));
    }
    for (i, (o, p)) in w.iter().zip(&got.block).enumerate() {
        // a picture one pixel wide: the second column behind each row is read from the byte
        // after the one-byte line buffers (not the picture's); it has no value to compare
        if got.origin == 5 && (i / 5) % 4 == 3 {
            continue;
        }
        let p = *p ^ u8::from(mutate());
        match o {
            Some(v) if *v as u8 != p => return Err(format!("block byte {i} (pixel {}, row {}): original {:02x}, port {p:02x}", i / 5, 0, *v as u8)),
            _ => {}
        }
    }
    Ok(())
}

/// The picture files of a case.
struct FileEnv<'a> {
    c: &'a Case,
    status: Vec<String>,
}
impl MaterialsEnv for FileEnv<'_> {
    fn drand48(&mut self) -> f64 {
        0.0
    }
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        let name = String::from_utf8_lossy(name);
        let path = if default {
            "/res/noIcon.pic".to_string()
        } else if name.starts_with('/') {
            format!("{name}.pic")
        } else {
            format!("/tex/{name}.pic")
        };
        self.c.try_input(&format!("file:{path}")).map(|f| f.to_bytes())
    }
    fn status(&mut self, text: &str) {
        self.status.push(text.to_string());
    }
}

fn picture_of(c: &Case) -> GlbMap {
    let im = Image::of(c.input("glb"));
    let name = c.try_input("name").map(|f| f.to_bytes().into_iter().take_while(|b| *b != 0).collect()).unwrap_or_default();
    let pixels = im.ptr(0x1c).map(|(buffer, offset)| TexPixels { block: c.input(buffer).to_bytes(), origin: offset as usize / 5 });
    GlbMap { name, crop: im.i16(4), crop_x0: im.i16(6), crop_x1: im.i16(8), crop_y0: im.i16(0xa), crop_y1: im.i16(0xc), width: im.i16(0x16), height: im.i16(0x18), pixels, ..GlbMap::default() }
}

fn same_picture(c: &Case, st: &ShadeState, moved: bool) -> Result<(), String> {
    let g = &st.pictures[0];
    let mut port = Image::of(c.input("glb"));
    port.put_i16(0x16, g.width);
    port.put_i16(0x18, g.height);
    // the pixels: a new block of the heap, or the buffer the case gave
    same_image("picture record", c.after("glb"), &port, &|at, o, p| if at == 0x1c { Some(if moved { o.is_some_and(|x| x.0.starts_with("heap")) } else { o == p }) } else { kept(at, o, p) })?;
    let stat = c.after("g_stat").i32s()[0];
    let want = c.input("g_stat").i32s()[0].wrapping_add(st.texture_pixels);
    if stat != want ^ i32::from(mutate()) {
        return Err(format!("pixel counter: original {stat}, port {want}"));
    }
    if moved {
        same_block(c, g.pixels.as_ref())?;
    }
    Ok(())
}

fn check_read_texture(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    st.pictures.push(picture_of(c));
    let mut env = FileEnv { c, status: Vec::new() };
    let got = read_texture_file(&mut st, 0, c.input("count").i16(), &mut env);
    verdict((|| {
        // the open routine shows messages of its own before the one of this routine
        let texts: Vec<String> = c.outputs.iter().filter(|f| f.name.starts_with("str")).map(|f| String::from_utf8_lossy(&f.to_bytes()).into_owned()).collect();
        let replaced = texts.iter().filter(|t| t.starts_with("Cannot find ")).cloned().collect::<Vec<_>>();
        if replaced != env.status {
            return Err(format!("status: original {texts:?}, port {:?}", env.status));
        }
        if c.calls.iter().any(|k| k.name == "si_fatal") {
            return if matches!(got, Err(SetupError::Fatal(_))) { Ok(()) } else { Err(format!("fatal in the original, port {got:?}")) };
        }
        got.map_err(|e| format!("the port stops with {e:?}"))?;
        if c.output("eax").u32() != 1 {
            return Err("the original does not return 1".into());
        }
        same_picture(c, &st, true)
    })())
}

fn check_crop(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    st.pictures.push(picture_of(c));
    let before = (st.pictures[0].width, st.pictures[0].height);
    global_2d_map_crop(&mut st, 0, c.input("count").i16());
    let moved = c.try_output("block").is_some() && Image::of(c.after("glb")).ptr(0x1c).is_some_and(|p| p.0.starts_with("heap"));
    let port_moved = (st.pictures[0].width, st.pictures[0].height) != before || st.pictures[0].pixels.as_ref().is_some_and(|p| p.origin != before.0 as usize + 4) || moved;
    verdict(if moved != port_moved && !moved { Err("the port crops, the original does not".to_string()) } else { same_picture(c, &st, moved) })
}

fn check_static_init(c: &Case) -> Verdict {
    let defeat = c.name.contains('#');
    let before = c.input("g_opt").i16();
    let run = c.try_input("a").map_or(true, |a| a.i32() != 0 && c.input("b").i32() == 0xffff);
    let want = if run { static_initialization(defeat) } else { before };
    let got = c.after("g_opt").i16();
    Verdict::from(got == want ^ i16::from(mutate()), || format!("SI_PreviewOptimize: original {got}, port {want}"))
}

// ---------------------------------------------------------------------- the texture stage

/// The four maps, two pictures and three vertices' pairs of a textured scene, added to a
/// state that has its three vertices.
fn add_textures(st: &mut ShadeState, c: &Case) -> Vec<String> {
    let maps: Vec<String> = (0..4).map(|k| format!("map{k}")).collect();
    let glbs = ["glb0".to_string(), "glb1".to_string()];
    let l = crate::outer::Loaded::of(c);
    for g in &glbs {
        st.pictures.push(glb_of(&l, c, g));
    }
    for name in &maps {
        let im = Image::of(c.input(name));
        let mut m = map_of(&l, name);
        m.next = Names::index(&maps, im.ptr(0x16c));
        m.glb = Names::index(&glbs, im.ptr(0x24));
        st.maps.push(m);
    }
    for k in 0..3 {
        let w = c.input(&format!("uv{k}")).f32s();
        st.verts[k].uv = st.uvs.len() as u32;
        st.uvs.extend(w.chunks(2).map(|p| [p[0], p[1]]));
    }
    maps
}

/// The three texture routines as ported; nothing else may be called.
struct Ports;
impl ShadeEnv for Ports {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        panic!("si_search called")
    }
    fn s_trace(&mut self, _: &mut ShadeState, _: &mut Ray, _: usize, _: f32, _: usize) -> [f32; 4] {
        panic!("si_sTrace called")
    }
    fn trace(&mut self, _: &ShadeState, _: &mut Ray, _: usize) -> bool {
        panic!("si_trace called")
    }
}

/// `SI_shade` with 2D maps: the scene of `gen_render.py` with the texture switch on.
fn shade_textured(c: &Case, five: bool) -> Verdict {
    crate::shade_verdict_with(c, five, &mut Ports, &|st, c| {
        let maps = add_textures(st, c);
        let head = |w: &Word| match w {
            Word::Ref { name, .. } => maps.iter().position(|m| m == name),
            _ => None,
        };
        st.materials[0].maps = head(&c.input("mat").words()[444]);
        st.models[0].maps = head(&c.input("model").words()[0x21c / 4]);
    })
}

/// The 2D routines as ported, `SI_renderTxt3d` recorded with the texture it was given.
#[derive(Default)]
struct Recorded3d(Vec<(u32, [u32; 3], bool)>);
impl ShadeEnv for Recorded3d {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        panic!("si_search called")
    }
    fn render_txt3d(&mut self, st: &mut ShadeState, material: usize, p: Vector, normal: Option<&mut Vector>) {
        self.0.push((st.materials[material].tex3d, [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()], normal.is_some()));
    }
    fn s_trace(&mut self, _: &mut ShadeState, _: &mut Ray, _: usize, _: f32, _: usize) -> [f32; 4] {
        panic!("si_sTrace called")
    }
    fn trace(&mut self, _: &ShadeState, _: &mut Ray, _: usize) -> bool {
        panic!("si_trace called")
    }
}

fn check_shadow_map(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    let (ray_im, tn_im, mat_im, mdl_im) = (Image::of(c.input("ray")), Image::of(c.input("tn")), Image::of(c.input("mat")), Image::of(c.input("mdl")));
    st.verts = vec![si_math::geometry::RVertex::default(); 3];
    let maps = add_textures(&mut st, c);
    let t3 = ["t0".to_string(), "t1".to_string()];
    let l = crate::outer::Loaded::of(c);
    st.tex3ds = t3.iter().map(|n| tex3d_of(&l, n)).collect();
    st.tns.push(si_math::geometry::Tn { v: [Some(0), Some(1), Some(2)], inv_n: tn_im.f32(0x24), material: 0, w58: tn_im.i16(0x58) as u16 as _, ..Default::default() });
    let mut m = material_of(&l, "mat");
    m.maps = Names::index(&maps, mat_im.ptr(0x6ec));
    m.tex3d = Names::index(&t3, mat_im.ptr(0x6fc)).map_or(0, |i| i as u32 + 1);
    st.materials.push(m);
    st.models.push(ShadeModel { maps: Names::index(&maps, mdl_im.ptr(0x21c)), tex3d: Names::index(&t3, mdl_im.ptr(0x224)).map_or(0, |i| i as u32 + 1), ..ShadeModel::default() });
    let hit = ray_im.f32s::<3>(0x28);
    let mut ray = Ray { hit: Vector::new(hit[0], hit[1], hit[2]), tn: Some(0), ..Ray::default() };
    let p = c.input("p").f32s();
    let mut env = Recorded3d::default();
    shadow_map(&mut st, &mut env, &mut ray, 0, Vector::new(p[0], p[1], p[2]));
    verdict((|| {
        let mut port = ray_im.clone();
        port.put_f32s(0x28, &[ray.hit.x, ray.hit.y, ray.hit.z]);
        same_image("ray", c.after("ray"), &port, &kept)?;
        let mut port = mat_im.clone();
        crate::tex::put_material(&st.materials[0], &mut port);
        same_image("material", c.after("mat"), &port, &kept)?;
        let asked: Vec<(u32, [u32; 3], bool)> = c
            .calls
            .iter()
            .filter(|k| k.name == "SI_renderTxt3d")
            .map(|k| {
                let tex = match &k.args[0] { Word::Ref { name, .. } => t3.iter().position(|t| t == name).map_or(0, |i| i as u32 + 1), _ => 0 };
                let w = |i: usize| k.args[i].value().unwrap() as u32;
                (tex, [w(1), w(2), w(3)], w(4) != 0)
            })
            .collect();
        if asked != env.0 {
            return Err(format!("SI_renderTxt3d: original {asked:?}, port {:?}", env.0));
        }
        Ok(())
    })())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_materials" => check_materials,
        "setTxtFlag" => check_set_txt_flag,
        "updateDefaultMaterial" => check_default_material,
        "decayTable" => check_decay_table,
        "SI_textures" => |c: &Case| check_textures(c, "SI_textures"),
        "SI_processTextures" => |c: &Case| check_textures(c, "SI_processTextures"),
        "resetTextureDirtyFlags" => |c: &Case| check_textures(c, "resetTextureDirtyFlags"),
        "markModelTextureImages" => |c: &Case| check_textures(c, "markModelTextureImages"),
        "markTextureImages" => |c: &Case| check_textures(c, "markTextureImages"),
        "si_cleanUpTexture" => |c: &Case| check_textures(c, "si_cleanUpTexture"),
        "SI_getMapsList" => |c: &Case| {
            // the head of the picture list is the word the routine returns
            let word = (0x1c + c.input("pd").u32s()[1] as usize * 0x24) / 4;
            let st = ShadeState { picture_head: Some(c.input("graf").u32s()[word] as usize), ..ShadeState::default() };
            let got = get_maps_list(&st).unwrap() as u32 ^ u32::from(mutate());
            Verdict::from(c.output("eax").u32() == got, || format!("head: original {:#x}, port {got:#x}", c.output("eax").u32()))
        },
        "0825b7ac" | "0825b7ac#defeat" | "0825b7ac#empty" | "0825b7e4" => check_static_init,
        "SI_readTextureFile" => check_read_texture,
        "global2dMapCrop" => check_crop,
        "shadowMap" => check_shadow_map,
        "SI_shade#tex" => |c: &Case| shade_textured(c, false),
        "SI_shade#tex!five" => |c: &Case| shade_textured(c, true),
        _ => return None,
    })
}
