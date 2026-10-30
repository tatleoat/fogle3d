// SPDX-License-Identifier: MIT
//! si-import-dump (units import1/import2): a Softimage|3D scene loaded with the engine and written out as JSON for the
//! Blender importer (`blender-addon/softimage3d/importer.py`). The format is described in `docs/design/softimage-import.md`.
//!
//! Tooling only: nothing of the original is reproduced here (no citation). The scene goes through the same steps as the
//! port's renderer before its first frame (the loader and the start-up of `rensetup-check`, `place_scene`, the scene's
//! render set-up), and every frame is brought to its time by the engine's own update (`SI_sceneUpdateItems` as the
//! renderer's `SI_update` calls it, through the render command's world). What is written is read back from the memory
//! image: the model tree with each node's global matrix, the geometry the renderer makes of it (its own converters and
//! texture coordinates: `SI_meshTriangles`, `SI_collect`, `SI_textureVerts` ports in si-render), materials, 2D textures,
//! the camera, the lights and the scene's settings, all in Softimage's own coordinates (the importer converts).
//!
//! Usage: si-import-dump [--install DIR] [--frames all|first|N] [--deform] SCENE.dsc [OUT.json]
//!   --install DIR   the Softimage|3D 4.0 install (else SI_INSTALL, else the development copy: si-install's rules)
//!   --frames        all frames of the scene's range (default), the first only, or at most N frames from the first
//!   --deform        also the vertex positions of deforming meshes per frame (shape animation, envelopes, lattices)
//! The JSON goes to OUT.json, or to standard output.
use serde_json::{json, Map, Value};
use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_mem::{FnPtr, Mem};
use si_render::softtriangle::{NodeDef, NodeSource, TextureNode};
use si_render::softvertex::{MatRef, MeshTrianglesArgs, NO_UV};
use si_rensetup::native::outside::Outside;
use si_rensetup::native::world::{NoWatch, World};
use si_scene::load::{self, Scene};
use si_x87::x;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const FORMAT: &str = "si3d-import-dump";
const VERSION: i32 = 1;

// addresses of the image this tool reads (names as the crates that own them give them)
const SI_DEFAULT_CONTROL: u32 = 0x0878_1800;
const SI_FRAME_CURRENT: u32 = 0x0878_2040;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const SI_DATABASE: u32 = 0x0879_0640;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_RIGHT_HAND: u32 = 0x0867_93ec;
const DICT_SIZE: u32 = 0x70;
const SI_SCENE_UPDATE_ITEMS: u32 = 0x0822_21a0;

/// A float as JSON, by its shortest `f32` text (so that Python's float reads back the same `f32`); NaN and
/// infinities are `null`.
fn f(v: f32) -> Value {
    if v.is_finite() {
        Value::from(v.to_string().parse::<f64>().unwrap_or(0.0))
    } else {
        Value::Null
    }
}
fn fv(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&a| f(a)).collect())
}
fn vec3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}
fn mat16(m: &Mem, a: u32) -> [f32; 16] {
    core::array::from_fn(|i| m.f32(a + 4 * i as u32))
}
fn text(m: &Mem, p: u32) -> String {
    if p == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(&m.cstr(p)).into_owned()
    }
}
fn bytes_text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// A shader reference (`__SI_UDF`, si-hrc image/udf.rs) as the dump carries it: the name the scene gives the shader
/// element (its element name without the scene prefix and version) and the active flag; null where there is none.
/// The shader's parameters are not in the reference at this point (its value lists are empty until the mental ray
/// export fills them): the dump reads them from the element's `.shd` file (see `shader_files`).
fn udf_ref(m: &Mem, u: u32) -> Value {
    if u == 0 || !m.is_mapped(u, 0x3c) {
        return Value::Null;
    }
    json!({"name": text(m, m.u32(u)), "active": m.i16(u + 0x0c) != 0})
}

/// A list of shader references: `count` words at `count_at`, the array's address at `array_at`.
fn udf_list(m: &Mem, count_at: u32, array_at: u32) -> Vec<Value> {
    let n = m.i32(count_at).clamp(0, 256) as u32;
    let arr = m.u32(array_at);
    if arr == 0 {
        return Vec::new();
    }
    (0..n).map(|i| udf_ref(m, m.u32(arr + 4 * i))).filter(|v| !v.is_null()).collect()
}

/// One shader element's file (`.shd`, text): its type tag, the library shader it instances (`_FILETYPE` DATABASE and
/// NAME) and its parameters (`_DATA [n t1 .. tn]`, then one line per parameter: its name and either one value per key
/// time or the parameter's value tokens). Numbers become numbers; other tokens (strings, `!n` references) stay text.
fn parse_shd(text: &str) -> Value {
    let mut lines = text.lines();
    let tag = lines.next().unwrap_or("").trim();
    let kind: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric() && !c.is_ascii_digit()).collect();
    let mut library = Value::Null;
    let mut database = Value::Null;
    let mut keys: Vec<Value> = Vec::new();
    let mut params = Map::new();
    let mut section = "";
    for line in lines {
        let l = line.trim_end();
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if t == "_END" {
            section = "";
            continue;
        }
        if t == "_FILETYPE" {
            section = "filetype";
            continue;
        }
        if let Some(rest) = t.strip_prefix("_DATA") {
            section = "data";
            let toks: Vec<&str> = rest.split_whitespace().collect();
            if let Some(n) = toks.first().and_then(|n| n.parse::<usize>().ok()) {
                keys = toks[1..].iter().take(n).filter_map(|k| k.parse::<f64>().ok()).map(Value::from).collect();
            }
            continue;
        }
        let (name, rest) = match t.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (t, ""),
        };
        match section {
            "filetype" => {
                let unq = rest.trim_matches('"').to_string();
                match name {
                    "NAME" => library = Value::from(unq),
                    "DATABASE" => database = Value::from(unq),
                    _ => {}
                }
            }
            "data" => {
                let vals: Vec<Value> = rest
                    .split_whitespace()
                    .map(|v| v.parse::<f64>().map_or_else(|_| Value::from(v.to_string()), Value::from))
                    .collect();
                params.insert(name.to_string(), Value::Array(vals));
            }
            _ => {}
        }
    }
    json!({"type": kind, "library": library, "library_database": database, "keys": keys, "params": params})
}

/// The shader elements a scene file (`.dsc`) lists: every chapter whose name ends in `_SHADERS`, each element read
/// from `<database>/<chapter>/<element>.shd`. The scene's references name an element by what follows the scene's
/// prefix (`<prefix>-<name>.<version>`): `udf` is that name.
fn shader_files(scene: &Path, database: &Path) -> Vec<Value> {
    let Ok(raw) = std::fs::read(scene) else { return Vec::new() };
    let text = String::from_utf8_lossy(&raw);
    let mut out = Vec::new();
    let mut chapter: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t == "RELATIONS" || t == "EndOfELEMENTS" {
            break;
        }
        if let Some(rest) = t.strip_prefix("CHAPTER ") {
            let c = rest.split_whitespace().next().unwrap_or("").to_string();
            chapter = c.ends_with("_SHADERS").then_some(c);
            continue;
        }
        if t.starts_with("EndOfCHAPTER") {
            chapter = None;
            continue;
        }
        let Some(ch) = &chapter else { continue };
        let Some(elem) = t.split(|c: char| c.is_whitespace() || c == ';').find(|w| !w.is_empty()) else { continue };
        let file = database.join(ch).join(format!("{elem}.shd"));
        // the element's name without the version (`.<a>-<b>`) and without the scene's prefix (to its first '-')
        let base = match elem.rsplit_once('.') {
            Some((b, v)) if v.split('-').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())) => b,
            _ => elem,
        };
        let udf = base.split_once('-').map_or(base, |(_, n)| n);
        let mut v = match std::fs::read(&file) {
            Ok(b) => parse_shd(&String::from_utf8_lossy(&b)),
            Err(_) => json!({"type": Value::Null, "library": Value::Null, "keys": [], "params": {}}),
        };
        let o = v.as_object_mut().expect("object");
        o.insert("id".into(), Value::from(out.len()));
        o.insert("chapter".into(), Value::from(ch.clone()));
        o.insert("element".into(), Value::from(elem.to_string()));
        o.insert("udf".into(), Value::from(udf.to_string()));
        o.insert("file".into(), Value::from(file.to_string_lossy().into_owned()));
        o.insert("found".into(), Value::from(file.is_file()));
        out.push(v);
    }
    out
}

/// Gives a shader reference (`{"name", "active"}`) the id of the element of `chapter` it names.
fn link_shader(r: &mut Value, chapter: &str, shaders: &[Value]) {
    let Some(o) = r.as_object_mut() else { return };
    let name = o.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let id = shaders
        .iter()
        .find(|s| s["chapter"] == chapter && s["udf"] == name.as_str())
        .or_else(|| shaders.iter().find(|s| s["chapter"] == chapter && s["element"].as_str().is_some_and(|e| e.contains(&format!("-{name}")))))
        .map(|s| s["id"].clone());
    o.insert("shader".into(), id.unwrap_or(Value::Null));
}

/// The screen the start-up assumes (`SI_xmaxscreen`, `SI_ymaxscreen`).
const SCREEN: (i16, i16) = (1279, 1023);

/// The program's start-up as far as it is on the image, before a scene is loaded: a copy of `start_up` in
/// si-rensetup's `rensetup-check` (read there; that file is another package's): the two directories, the loaded
/// globals, the screen, the values of `SI_glbManip` and `SI_glbAnim` and `SI_matterInit`.
fn start_up(m: &mut Mem, resdir: &[u8], texture_path: &[u8]) {
    use si_rensetup::native::{globals as g, matter};
    for (at, t) in [(si_scene::t2dpath::SI_RESDIR, resdir), (si_scene::t2dpath::SI_TEXTURE_PATH, texture_path)] {
        let p = m.malloc(t.len() as u32 + 1);
        m.put_cstr(p, t);
        m.set_u32(at, p);
    }
    g::init_loaded(m);
    m[g::SI_XMAXSCREEN] = SCREEN.0;
    m[g::SI_YMAXSCREEN] = SCREEN.1;
    let (gm, ga) = (0x0878_1560u32, 0x0878_1480u32);
    for (o, v) in [(0x00u32, 0u32), (0x04, 0), (0x08, 2), (0x0c, 4), (0x10, 0), (0x14, 4), (0x18, 0)] {
        m.set_u32(gm + o, v);
    }
    for (o, v) in [(0x1cu32, 0i16), (0x30, 0), (0x1e, 0), (0x20, 1), (0x22, 0), (0x24, 1), (0x26, 1), (0x28, 0), (0x2a, 0), (0x2c, 0), (0x2e, 0), (0x32, 0), (0x34, 0), (0x36, 1), (0x38, 0)] {
        m.set_i16(gm + o, v);
    }
    m.set_f32(ga + 0x18, 0.15);
    m.set_f32(ga + 0x28, 0.01);
    for (o, v) in [(0x00u32, 0i16), (0x02, 0), (0x04, 1), (0x08, 0), (0x0a, 0), (0x0c, 0), (0x14, 0), (0x2c, 0), (0x1c, 1), (0x1e, 0), (0x20, 1), (0x22, 1), (0x24, 0), (0x26, 1), (0x30, 2), (0x32, 1), (0x3c, 0), (0x3a, 0), (0x2e, 0), (0x38, 1), (0x3e, 0)] {
        m.set_i16(ga + o, v);
    }
    m.set_u32(ga + 0x10, 0);
    m.set_u32(ga + 0x34, 0);
    matter::matter_init(m, &|p: &[u8]| std::fs::read(std::ffi::OsStr::from_bytes(p)).ok());
}

struct Args {
    install: Option<String>,
    frames: Frames,
    deform: bool,
    scene: PathBuf,
    out: Option<PathBuf>,
}
#[derive(Clone, Copy)]
enum Frames {
    All,
    First,
    AtMost(usize),
}

fn args() -> Result<Args, String> {
    let mut a = Args { install: None, frames: Frames::All, deform: false, scene: PathBuf::new(), out: None };
    let mut it = std::env::args().skip(1);
    let mut rest = Vec::new();
    while let Some(s) = it.next() {
        match s.as_str() {
            "--install" => a.install = Some(it.next().ok_or("--install needs a directory")?),
            "--frames" => {
                let v = it.next().ok_or("--frames needs all, first or a number")?;
                a.frames = match v.as_str() {
                    "all" => Frames::All,
                    "first" => Frames::First,
                    n => Frames::AtMost(n.parse().map_err(|_| format!("--frames {n}: not all, first or a number"))?),
                };
            }
            "--deform" => a.deform = true,
            "-h" | "--help" => return Err(String::new()),
            _ => rest.push(s),
        }
    }
    match rest.len() {
        1 | 2 => {
            a.scene = PathBuf::from(&rest[0]);
            a.out = rest.get(1).map(PathBuf::from);
            Ok(a)
        }
        _ => Err("one scene file (.dsc) and at most one output file".into()),
    }
}

/// The loaded scene: the loader's state and its notes.
struct Loaded {
    sc: Scene,
    notes: Vec<String>,
    database: PathBuf,
    name: String,
}

fn load_scene(install: &si_install::Install, path: &Path) -> Result<Loaded, String> {
    let rsrc = std::fs::read(&install.rsrc).map_err(|e| format!("{}: {e}", install.rsrc.display()))?;
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !data.starts_with(b"SDSC") {
        return Err(format!("{}: not a Softimage scene file (no SDSC header)", path.display()));
    }
    let db = path.parent().and_then(|p| p.parent()).ok_or("the scene file is not in <database>/SCENES")?.to_path_buf();
    let name = path.file_stem().ok_or("no scene name")?.as_bytes().to_vec();
    let resdir = install.rsrc_dir.canonicalize().map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    // `SI_texturePath`: the batch renderer's working directory (strdup(SI_cwd)); the tool's own
    let cwd = std::env::current_dir().map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    let mut sc = Scene::start(&rsrc).map_err(|e| format!("Relations.rsrc refused: {e:?}"))?;
    start_up(&mut sc.mem, &resdir, &cwd);
    sc.program.backup_enabled = sc.mem.i16(0x0878_1560 + 0x36) != 0;
    // the stand-alone renderer's start-up: SI_standaloneRenderer, then SI_metaballInit
    sc.standalone_renderer = true;
    sc.mem.set_i16(0x0867_9400, 1);
    si_rensetup::native::world::start_up_metaball_init(&mut sc.mem);
    si_rensetup::native::motion::start_process();
    sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
    let mut info = DbInfo::new();
    info.name = db.file_name().map(|n| n.as_bytes().to_vec());
    info.path = Some(db.as_os_str().as_bytes().to_vec());
    let mut log = Log::default();
    let fs = RealFs;
    si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
    let mut lc = si_rensetup::native::motion::LoadCalls { refresh: true, ..Default::default() };
    let ok = load::load_scene(&mut sc, &fs, &mut info, &name, &mut lc);
    let mut notes = Vec::new();
    if !ok {
        notes.push("the load answered false (carried on, as the renderer does)".to_string());
    }
    for r in &sc.report {
        let what = match &r.outcome {
            load::Outcome::Missing { tried } => format!("file missing (last tried {})", String::from_utf8_lossy(&tried[..])),
            load::Outcome::Invalid { path } => format!("file refused ({})", String::from_utf8_lossy(&path[..])),
            load::Outcome::NodeMissing => "node missing in the root's file".into(),
            load::Outcome::NoModel => "no model to read it onto".into(),
            _ => continue,
        };
        let chap = si_db::chap_info_from_chap_id(r.chap).map_or_else(|| r.chap.to_string(), |c| c.directory.to_string());
        notes.push(format!("load: {chap} [{}]: {what}", String::from_utf8_lossy(&r.name)));
    }
    Ok(Loaded { sc, notes, database: db, name: String::from_utf8_lossy(&name).into_owned() })
}

/// Ids given to the image's records as they are met.
#[derive(Default)]
struct Table {
    ids: BTreeMap<u32, usize>,
    out: Vec<Value>,
}
impl Table {
    fn get(&self, a: u32) -> Option<usize> {
        self.ids.get(&a).copied()
    }
}

struct Dump {
    materials: Table,
    textures: Table,
    textures3d: Table,
    not_carried: BTreeMap<String, usize>,
    notes: Vec<String>,
}

impl Dump {
    fn note(&mut self, what: &str) {
        *self.not_carried.entry(what.to_string()).or_default() += 1;
    }

    /// The 2D texture record (`__SI_LclMap`) as its id; the picture file looked for as `SI_readTextureFile` names it.
    fn texture(&mut self, m: &Mem, a: u32, texture_path: &[u8], frame: i32) -> usize {
        if let Some(i) = self.textures.get(a) {
            return i;
        }
        let r = si_render::inimage::map_at(m, a);
        let path = bytes_text(&r.path);
        // SI_setFileName: one picture, or a numbered sequence (the path to its last '.', then '.' and the frame)
        let name: Option<Vec<u8>> = match r.anim {
            0 => Some(r.path.clone()),
            1 => {
                let stem = r.path.iter().rposition(|b| *b == b'.').map_or(&r.path[..], |d| &r.path[..d]);
                Some([stem, format!(".{frame}").as_bytes()].concat())
            }
            _ => None,
        };
        let file = name.map(|n| {
            let mut p = Vec::new();
            if n.first() != Some(&b'/') {
                p.extend_from_slice(texture_path);
                p.push(b'/');
            }
            p.extend_from_slice(&n);
            p.extend_from_slice(b".pic");
            String::from_utf8_lossy(&p).into_owned()
        });
        let found = file.as_deref().is_some_and(|p| Path::new(p).is_file());
        let id = self.textures.out.len();
        self.textures.ids.insert(a, id);
        self.textures.out.push(json!({
            "id": id,
            "picture": path,
            "file": file,
            "found": found,
            "sequence": r.anim,
            "method": r.method,
            "pixel_interp": r.w2c,
            "uscale": f(r.uscale), "vscale": f(r.vscale),
            "uoffset": f(r.uoff), "voffset": f(r.voff),
            "urepeat": r.urepeat, "vrepeat": r.vrepeat,
            "repeat": r.repeat,
            "swap_uv": r.swap_uv, "alt_u": r.alt_u, "alt_v": r.alt_v,
            "band_u": r.band_u, "band_v": r.band_v,
            "crop": [r.crop, r.crop_x0, r.crop_x1, r.crop_y0, r.crop_y1],
            "mask": r.mask, "blend": f(r.blend),
            "channel": r.bump_alpha,
            "ambient": r.fx_ambient, "diffuse": r.fx_diffuse, "specular": r.fx_specular,
            "reflectivity": r.fx_reflect, "transparency": r.fx_transp,
            "roughness": r.bump, "bump_amount": f(r.bump_amount),
            "factors": fv(&r.factors),
            "black_white": r.black_white,
            "projection": {"rotation": fv(&r.rotation), "scaling": fv(&r.scaling), "translation": fv(&r.translation)},
        }));
        let sh = udf_ref(m, m.u32(a + 0x198));
        if !sh.is_null() {
            self.textures.out[id]["shader"] = sh;
        }
        id
    }

    /// The 3D texture record (`__SI_Texture3D`) as its id: the fields the renderer's 3D texture code reads, as they
    /// stand at the first frame (an animated 3D texture's later values are not carried).
    fn texture3d(&mut self, m: &Mem, a: u32) -> usize {
        if let Some(i) = self.textures3d.get(a) {
            return i;
        }
        let id = self.textures3d.out.len();
        self.textures3d.ids.insert(a, id);
        let t = si_render::inimage::tex3d_at(m, a);
        let name_ptr = m.u32(a);
        let sh = udf_ref(m, m.u32(a + 0x14c));
        if !sh.is_null() {
            self.note("3D texture with a shader (.so) of its own (the reference and the shader's parameters are carried; the procedural texture too)");
        }
        self.textures3d.out.push(json!({
            "id": id,
            "name": if name_ptr == 0 { Value::Null } else { Value::from(text(m, name_ptr)) },
            "kind": t.kind,
            "spacing": f(t.spacing), "taper": f(t.taper), "strength": f(t.strength),
            "iterations": t.iterations, "power": f(t.power),
            "mask": t.mask, "blending": f(t.blend), "channel": t.channel,
            "colours": Value::Array(t.colours.iter().map(|c| fv(c)).collect()),
            "factors": fv(&t.stops),
            "scaling": fv(&t.scaling), "rotation": fv(&t.rotation), "translation": fv(&t.translation),
            "ambient": f(t.factors[0]), "diffuse": f(t.factors[1]), "specular": f(t.factors[2]),
            "reflection": f(t.factors[3]), "transparency": f(t.factors[4]), "roughness": f(t.factors[6]),
        }));
        if !sh.is_null() {
            self.textures3d.out[id]["shader"] = sh;
        }
        id
    }

    /// The material record (`__SI_Material`) as its id.
    fn material(&mut self, m: &Mem, a: u32, texture_path: &[u8], frame: i32) -> usize {
        if let Some(i) = self.materials.get(a) {
            return i;
        }
        let id = self.materials.out.len();
        self.materials.ids.insert(a, id);
        self.materials.out.push(Value::Null);
        let r = si_render::inimage::material_at(m, a);
        let mut maps = Vec::new();
        let mut p = m.u32(a + 0x6ec);
        let mut guard = 0;
        while p != 0 && guard < 1024 {
            maps.push(self.texture(m, p, texture_path, frame));
            p = m.u32(p + 0x16c);
            guard += 1;
        }
        let refl = m.u32(a + 0x6f4);
        let reflection_map = (refl != 0).then(|| self.texture(m, refl, texture_path, frame));
        let t3d = m.u32(a + 0x6fc);
        let texture3d = (t3d != 0).then(|| self.texture3d(m, t3d));
        self.materials.out[id] = json!({
            "id": id,
            "name": bytes_text(m.bytes(a, 0x1c)),
            "shading": r.model,
            "ambient": fv(&r.ambient[..3]),
            "diffuse": fv(&r.diffuse[..3]),
            "specular": fv(&r.specular[..3]),
            "exponent": f(r.spec_table.exponent),
            "reflectivity": f(r.reflectivity),
            "transparency": f(r.transparency),
            "refraction": f(r.refraction),
            "static_blur": r.static_blur,
            "blur_width": f(r.blur_width),
            "textures": maps,
            "reflection_map": reflection_map,
            "texture3d": texture3d,
        });
        // the four shader references (+0xdb0 material, +0xdb4 volume, +0xdb8 shadow, +0xdbc photon), when any is set
        let refs: Vec<(&str, Value)> = [("material", 0xdb0u32), ("volume", 0xdb4), ("shadow", 0xdb8), ("photon", 0xdbc)]
            .iter()
            .map(|&(k, off)| (k, udf_ref(m, m.u32(a + off))))
            .collect();
        if refs.iter().any(|(_, v)| !v.is_null()) {
            self.materials.out[id]["shaders"] = Value::Object(refs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        }
        id
    }
}

/// A list of 2D texture records linked through +0x16c into the state's maps; answers the first.
fn chain_maps(m: &Mem, st: &mut si_render::shade::ShadeState, head: u32) -> (Option<usize>, Vec<u32>) {
    let mut first = None;
    let mut prev: Option<usize> = None;
    let mut addrs = Vec::new();
    let mut p = head;
    while p != 0 && addrs.len() < 1024 {
        let i = st.maps.len();
        st.maps.push(si_render::inimage::map_at(m, p));
        addrs.push(p);
        match prev {
            Some(k) => st.maps[k].next = Some(i),
            None => first = Some(i),
        }
        prev = Some(i);
        p = m.u32(p + 0x16c);
    }
    (first, addrs)
}

/// One node's geometry as the renderer makes it at the current frame.
struct Geo {
    json: Value,
    triangles: usize,
}

/// The texture ids of the slots a triangle of material `mat` has: the material's list, then the node's own.
fn slots(mat_maps: &[Vec<usize>], own: &[usize], mat: u32) -> Vec<usize> {
    let mut v = mat_maps.get(mat as usize).cloned().unwrap_or_default();
    v.extend_from_slice(own);
    v
}

fn geometry(m: &mut Mem, d: &mut Dump, a: u32, kind: i32, mat_ids: &[usize], texture_path: &[u8], frame: i32) -> Result<Option<Geo>, String> {
    use si_render::shade::ShadeState;
    let mut src: NodeSource = si_render::inimage::node_source_at(m, a);
    if src.algo_type == 13 || src.garbage_matte == 1 {
        return Ok(None);
    }
    // the converters the render command runs on the image before its first frame
    if kind == 10 && src.def == NodeDef::Nurbs {
        src.def = si_render::adapter::nurbs_on_image(m, a)?;
    }
    let face = m.u32(a + 0x48);
    if kind == 2 && face != 0 && m.i16(face + 0x84) == 0 && src.def == NodeDef::Face {
        let mut side = si_render::adapter::FaceSide::default();
        src.def = si_render::adapter::nurbs_face_on_image(m, a, &mut side)?;
    }
    let mut st = ShadeState::default();
    let mut mat_maps: Vec<Vec<usize>> = Vec::new();
    let nmat = m.i16(a + 0x206).max(0) as u32;
    let arr = m.u32(a + 0x20c);
    for i in 0..nmat {
        let ma = if arr != 0 { m.u32(arr + 4 * i) } else { 0 };
        if ma == 0 {
            break;
        }
        let mut r = si_render::inimage::material_at(m, ma);
        let (first, addrs) = chain_maps(m, &mut st, m.u32(ma + 0x6ec));
        r.maps = first;
        st.materials.push(r);
        mat_maps.push(addrs.iter().map(|&p| d.texture(m, p, texture_path, frame)).collect());
    }
    let (own_first, own_addrs) = chain_maps(m, &mut st, m.u32(a + 0x21c));
    src.maps = own_first;
    let own: Vec<usize> = own_addrs.iter().map(|&p| d.texture(m, p, texture_path, frame)).collect();
    let n_maps = m.i16(a + 0x216);
    let have_materials = !src.no_material_array && !st.materials.is_empty();
    let matrefs: Vec<MatRef> = st.materials.iter().enumerate().map(|(i, r)| MatRef { id: i as u32, maps: r.maps }).collect();
    let reversed = m.u16(a + 0x08) & 8 != 0;
    let mut tess = si_render::bound::Ported;
    let elem_id = src.elem_id;
    let uvd = src.uv.clone();
    match kind {
        4 => {
            let NodeDef::Mesh { mesh, positions } = &mut src.def else { return Ok(None) };
            let mut mesh = mesh.clone();
            let positions = positions.clone().unwrap_or_else(|| mesh.clone());
            let args = MeshTrianglesArgs {
                model: Some(&uvd),
                positions: &positions,
                n_materials: src.n_materials as i32,
                materials: (!src.no_material_array).then_some(&matrefs[..]),
                elem_id,
                want_uv2: true,
                model_maps: src.maps,
                all_hard: 0,
                want_poly_tris: true,
                want_node_verts: true,
                strict: false,
            };
            let made = si_render::softvertex::mesh_triangles(&args, &mut mesh).map_err(|w| format!("SI_meshTriangles: {w:?}"))?;
            let n_polys = mesh.polygons.len();
            // the render vertex each (polygon, corner) became, and back
            let (mut render, poly_tris, node_verts) = match made {
                Some(t) => (t.node, t.poly_tris.unwrap_or_default(), t.node_verts.unwrap_or_default()),
                None => (Default::default(), vec![None; n_polys], vec![Vec::new(); n_polys]),
            };
            let mut origin: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
            for (pi, corners) in node_verts.iter().enumerate() {
                for (ci, vs) in corners.iter().enumerate() {
                    for &v in vs {
                        origin.insert(v, (pi, ci));
                    }
                }
            }
            let before: Vec<[Option<usize>; 3]> = render.tns.iter().map(|t| t.v).collect();
            let mut pairs_made = 0;
            if render.tn_count != 0 && have_materials {
                render.tcoords.clear();
                let tn = TextureNode { node: 0, kind, own_maps: n_maps, maps: src.maps, uv: &uvd, elem_id };
                pairs_made = si_render::softtriangle::texture_verts(&mut st, &mut render, &tn, &mut tess).map_err(|w| format!("SI_textureVerts: {w:?}"))?;
            }
            // the mesh as the importer builds it: its own vertices and polygons (contours split by -1), per corner the
            // normal and, for every texture of the polygon's material and of the node, the coordinates the renderer
            // interpolates there (taken from the polygon's triangle that has the corner's vertex)
            // the positions the renderer draws (SI_meshTriangles: a render vertex's +0x08 from the current definition,
            // +0x48; the definition that lends positions only gives +0x20, which the textures are projected from)
            let verts: Vec<Value> = mesh.vertices.iter().map(|v| fv(&v.pos)).collect();
            let mut polys = Vec::with_capacity(n_polys);
            let mut pmat = Vec::with_capacity(n_polys);
            let mut normals = Vec::with_capacity(n_polys);
            let mut hidden = Vec::new();
            let mut uv: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
            let mut uv_polys: Vec<Vec<(usize, Vec<[f32; 2]>)>> = Vec::with_capacity(n_polys);
            for (pi, p) in mesh.polygons.iter().enumerate() {
                polys.push(Value::Array(p.nodes.iter().map(|c| Value::from(c.vertex)).collect()));
                let local = if p.material >= 0 && (p.material as usize) < mat_ids.len() { p.material as i64 } else { 0 };
                pmat.push(Value::from(local));
                normals.push(Value::Array(p.nodes.iter().map(|c| fv(&c.normal)).collect()));
                if p.flag & 0x800 != 0 {
                    hidden.push(Value::from(pi));
                }
                let tex = if have_materials { slots(&mat_maps, &own, local as u32) } else { Vec::new() };
                let mut per_tex: Vec<(usize, Vec<[f32; 2]>)> = tex.iter().map(|&t| (t, vec![[0.0f32; 2]; p.nodes.len()])).collect();
                if pairs_made > 0 && !tex.is_empty() {
                    let tris: Vec<usize> = poly_tris.get(pi).cloned().flatten().unwrap_or_default().into_iter().flatten().collect();
                    for (ci, c) in p.nodes.iter().enumerate() {
                        if c.vertex < 0 {
                            continue;
                        }
                        // the triangle corner of this polygon at this vertex; else the vertex the corner made
                        let mut rv = None;
                        'find: for &t in &tris {
                            for k in 0..3 {
                                if let (Some(r0), Some(r1)) = (before.get(t).and_then(|c| c[k]), render.tns.get(t).and_then(|c| c.v[k])) {
                                    if let Some(&(opi, oci)) = origin.get(&r0) {
                                        if mesh.polygons[opi].nodes[oci].vertex == c.vertex {
                                            rv = Some(r1);
                                            break 'find;
                                        }
                                    }
                                }
                            }
                        }
                        let rv = rv.or_else(|| node_verts.get(pi).and_then(|cs| cs.get(ci)).and_then(|vs| vs.first().copied()));
                        if let Some(rv) = rv {
                            let base = render.verts[rv].uv;
                            if base != NO_UV {
                                for (s, entry) in per_tex.iter_mut().enumerate() {
                                    if let Some(pair) = render.tcoords.get(base as usize + s) {
                                        entry.1[ci] = *pair;
                                    }
                                }
                            }
                        }
                    }
                }
                uv_polys.push(per_tex);
            }
            let used: Vec<usize> = {
                let mut u: Vec<usize> = uv_polys.iter().flat_map(|p| p.iter().map(|e| e.0)).collect();
                u.sort();
                u.dedup();
                u
            };
            for &t in &used {
                let mut layer = Vec::with_capacity(n_polys);
                for (pi, per) in uv_polys.iter().enumerate() {
                    match per.iter().find(|e| e.0 == t) {
                        Some((_, pairs)) => layer.push(Value::Array(pairs.iter().map(|p| fv(p)).collect())),
                        None => layer.push(Value::Array(vec![Value::Array(vec![f(0.0), f(0.0)]); mesh.polygons[pi].nodes.len()])),
                    }
                }
                uv.insert(t, layer);
            }
            let tris = render.tn_count.max(0) as usize;
            Ok(Some(Geo {
                json: json!({
                    "type": "mesh",
                    "vertices": verts,
                    "polygons": polys,
                    "polygon_material": pmat,
                    "corner_normals": normals,
                    "hidden_polygons": hidden,
                    "faceted": mesh.flags & 2 != 0,
                    "discontinuity_angle": f(mesh.angle),
                    "uv": uv.into_iter().map(|(k, v)| (k.to_string(), Value::Array(v))).collect::<Map<String, Value>>(),
                    "render_triangles": tris,
                    "own_textures": own.clone(),
                    "reversed": reversed,
                }),
                triangles: tris,
            }))
        }
        1 | 2 | 10 => {
            let ren = si_render::geometry::RenModel { kind, reversed, materials: (0..st.materials.len()).collect(), n_maps, ..Default::default() };
            let right_hand = m.i16(SI_RIGHT_HAND) != 0;
            let mut tess = si_render::bound::Ported;
            if have_materials {
                si_render::softtriangle::collect(&mut st, &ren, &mut src, 0, right_hand, &mut tess).map_err(|w| format!("SI_collect: {w:?}"))?;
            } else {
                src.render.verts.clear();
                src.render.tns.clear();
                src.render.tn_count = 0;
                if src.visible != 0 {
                    si_render::softtriangle::make_vertices(&ren, &mut src, 0, None, -1, &mut tess).map_err(|w| format!("SI_makeVertices: {w:?}"))?;
                }
            }
            let r = &src.render;
            let n = r.tn_count.max(0) as usize;
            let mut tris = Vec::with_capacity(n);
            let mut tmat = Vec::with_capacity(n);
            let mut uv: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
            let mut tri_tex: Vec<Vec<usize>> = Vec::with_capacity(n);
            for t in r.tns.iter().take(n) {
                let c: Vec<i64> = t.v.iter().map(|v| v.map_or(-1, |i| i as i64)).collect();
                tris.push(json!(c));
                let local = if (t.material as usize) < mat_ids.len() { t.material as i64 } else { 0 };
                tmat.push(Value::from(local));
                tri_tex.push(if have_materials { slots(&mat_maps, &own, local as u32) } else { Vec::new() });
            }
            let mut used: Vec<usize> = tri_tex.iter().flatten().copied().collect();
            used.sort();
            used.dedup();
            for &tex in &used {
                let layer = r
                    .tns
                    .iter()
                    .take(n)
                    .zip(&tri_tex)
                    .map(|(t, tt)| {
                        let s = tt.iter().position(|&k| k == tex);
                        Value::Array(
                            t.v.iter()
                                .map(|v| {
                                    let pair = match (s, v) {
                                        (Some(s), Some(i)) if r.verts[*i].uv != NO_UV => r.tcoords.get(r.verts[*i].uv as usize + s).copied().unwrap_or([0.0; 2]),
                                        _ => [0.0; 2],
                                    };
                                    fv(&pair)
                                })
                                .collect(),
                        )
                    })
                    .collect();
                uv.insert(tex, layer);
            }
            let source = match kind {
                1 => "patch",
                2 => "face",
                _ => "nurbs surface",
            };
            Ok(Some(Geo {
                json: json!({
                    "type": "triangles",
                    "source": source,
                    "tessellation": "the renderer's own (SI_makeVertices at the scene's subdivision settings)",
                    "vertices": r.verts.iter().map(|v| fv(&[v.raw.x, v.raw.y, v.raw.z])).collect::<Vec<_>>(),
                    "normals": r.verts.iter().map(|v| fv(&[v.raw_normal.x, v.raw_normal.y, v.raw_normal.z])).collect::<Vec<_>>(),
                    "triangles": tris,
                    "triangle_material": tmat,
                    "uv": uv.into_iter().map(|(k, v)| (k.to_string(), Value::Array(v))).collect::<Map<String, Value>>(),
                    "own_textures": own.clone(),
                    "reversed": reversed,
                    "right_hand": right_hand,
                }),
                triangles: n,
            }))
        }
        6 => {
            let def = m.u32(a + 0x48);
            if def == 0 {
                return Ok(None);
            }
            let s = si_render::inimage::spline_at(m, def);
            Ok(Some(Geo {
                json: json!({
                    "type": "spline",
                    "spline_kind": s.kind,
                    "closed": s.close != 0,
                    "keys": s.keys.iter().map(|k| json!({"pos": fv(&k.pos), "prev": fv(&k.prev), "next": fv(&k.next)})).collect::<Vec<_>>(),
                    "tension": f(s.tension),
                    "step": s.step,
                }),
                triangles: 0,
            }))
        }
        _ => Ok(None),
    }
}

/// The camera as the renderer's update left it: the record `SI_update` read (`SI_database`, copied there by
/// `SI_sceneUpdateItems`) and the matrices `SI_camera` made of it for the picture.
fn camera_json(c: &si_render::camera::Camera, rc: &si_math::matrix::RenderCamera, v: &si_render::update::View) -> Value {
    let flat = |a: &[[f32; 4]; 4]| fv(&a.iter().flatten().copied().collect::<Vec<_>>());
    json!({
        "kind": c.kind,
        "position": fv(&[c.position.x, c.position.y, c.position.z]),
        "interest": fv(&[c.interest.x, c.interest.y, c.interest.z]),
        "roll": f(c.roll),
        "near": f(c.near),
        "far": f(c.far),
        "fov": f(c.fov),
        "dof": c.dof,
        "dof_values": fv(&[c.dof_focal, c.dof_focus, c.dof_near, c.dof_far, c.dof_beyond, c.dof_aperture, c.dof_circle]),
        "window": c.window,
        "window_values": fv(&c.window_v),
        "view_matrix": flat(&rc.raw_to_world_mat),
        "camera_matrix": flat(&rc.world_to_raw_mat),
        "perspective_matrix": flat(&rc.prsp_mat),
        "picture": fv(&v.picture),
        "screen_window": fv(&v.window),
    })
}

/// A light record (`light`, 0x240 bytes) as the update left it. `tree` is the dump's node list (model id = index).
fn light_now(m: &Mem, a: u32, default: bool, tree: &[u32]) -> Value {
    let name: Vec<u8> = (0..30).map(|i| m.u8(a + i)).collect();
    // a selective light (+0x15c) lights a model only when the table at +0x168 (one entry per MODELS element, the
    // node's element number at +0x354) holds the light's value (+0x160); shadows follow the same test (si-render
    // shade::light_applies, lights.rs). The ids of the models it lights, or null when it lights all.
    let selective = m.i32(a + 0x15c) != 0;
    let table = m.u32(a + 0x168);
    let lit: Value = if selective && table != 0 {
        let want = m.u32(a + 0x160);
        let ids: Vec<Value> = tree.iter().enumerate().filter(|(_, &n)| {
            let elem = m.i32(n + 0x354);
            elem >= 0 && m.is_mapped(table.wrapping_add(4 * elem as u32), 4) && m.u32(table.wrapping_add(4 * elem as u32)) == want
        }).map(|(i, _)| Value::from(i)).collect();
        Value::Array(ids)
    } else {
        Value::Null
    };
    let shader = udf_ref(m, m.u32(a + 0x178));
    let mut v = json!({
        "name": if default { "default light".to_string() } else { bytes_text(&name) },
        "default": default,
        "type": m.i16(a + 0x1e),
        "color": fv(&[m.f32(a + 0x24), m.f32(a + 0x28), m.f32(a + 0x2c)]),
        "position": fv(&vec3(m, a + 0x34)),
        "interest": fv(&vec3(m, a + 0x58)),
        "cone": f(m.f32(a + 0x70)),
        "spread": f(m.f32(a + 0x74)),
        "falloff": m.u32(a + 0x13c) & 1 != 0,
        "falloff_start": f(m.f32(a + 0x84)),
        "falloff_end": f(m.f32(a + 0x88)),
        "shadow": m.i16(a + 0x8c),
        "umbra": f(m.f32(a + 0x90)),
        // the depth-map shadow's values (shadow kinds 3 and 4; the light file's SHADOW_MAP_RES, SHADOW_FILTER_SIZE,
        // SHADOW_FILTER_STEP, SHADOW_FADE_START, SHADOW_FADE_RATE, si-elemio light.rs) as memory holds them; for kinds 3 and 4
        // the renderer sets +0x12c to 4 x +0x128 itself when it sets the lights up (crates/si-render lights.rs)
        "shadow_map": [m.i32(a + 0x118), m.i32(a + 0x11c), m.i32(a + 0x124), f(m.f32(a + 0x128)), f(m.f32(a + 0x12c))],
        "on": m.i16(a + 0x176) != 0,
        "selective": selective,
        "lit_models": lit,
    });
    if !shader.is_null() {
        v["shader"] = shader;
    }
    v
}

/// The scene's ambience and fog as the update left them in `SI_database`.
fn environment_now(m: &Mem) -> Value {
    json!({
        "ambience": fv(&vec3(m, 0x0879_0ddc)),
        "fog": {
            "on": m.i16(0x0879_0e3c),
            "density": f(m.f32(0x0879_0e40)),
            "base": f(m.f32(0x0879_0e44)),
            "thickness": f(m.f32(0x0879_0e48)),
            "start": f(m.f32(0x0879_0e4c)),
            "end": f(m.f32(0x0879_0e50)),
            "color": fv(&vec3(m, 0x0879_0e64)),
        },
        // the depth fade the shading reads (si-render's adapter::shading_from_image, camera::init_fade): the switch,
        // start and end distances, the colour surfaces fade to (also the background's), the transparency left at the end
        "fade": {
            "on": m.i16(0x0879_0ce0),
            "start": f(m.f32(0x0879_0ce4)),
            "end": f(m.f32(0x0879_0cec)),
            "color": fv(&vec3(m, 0x0879_0cf8)),
            "transparency": f(m.f32(0x0879_0d10)),
        },
    })
}

/// The renderer's tree from its root chain (0x087907e8): each node, then its sons, then its brothers; then the nodes
/// a `latdep` names that are not in the tree. The order `build_scene` numbers them in, depth first.
fn nodes(m: &Mem, root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![root];
    while let Some(a) = stack.pop() {
        if a == 0 || !seen.insert(a) || out.len() > 1_000_000 {
            continue;
        }
        out.push(a);
        // brother after the sons
        stack.push(m.u32(a + 0x2c0));
        stack.push(m.u32(a + 0x2c4));
    }
    let mut k = 0;
    while k < out.len() {
        let l = m.u32(out[k] + 0x44);
        if l != 0 && seen.insert(l) {
            out.push(l);
        }
        k += 1;
    }
    out
}

/// The light chain the render command handed over (0x087907e4, through +0x140).
fn light_chain(m: &Mem) -> Vec<u32> {
    let mut v = Vec::new();
    let mut l = m.u32(0x0879_07e4);
    while l != 0 && !v.contains(&l) && v.len() < 4096 {
        v.push(l);
        l = m.u32(l + 0x140);
    }
    v
}

/// What the dump needs besides the image, handed to the point where the render command calls `SI_image`.
struct Job {
    frames: Frames,
    deform: bool,
    texture_path: Vec<u8>,
    lights_read: Vec<u32>,
    notes: Vec<String>,
}

/// The render command's world with `SI_image` taken over: where the command calls the renderer, the dump is made
/// instead (the scene then is as the renderer receives it: the roots chained with the copies of the instances'
/// masters, the lights chained, the camera and the controls in `SI_ren`). Everything else goes to the world. The
/// stand-alone renderer opens no sub-region box.
struct Grab<'w> {
    world: &'w mut World<NoWatch>,
    job: Option<Job>,
    out: Option<Result<Value, String>>,
}

impl Outside for Grab<'_> {
    fn call(&mut self, m: &mut Mem, f: FnPtr, args: &[u32]) -> u32 {
        use si_rensetup::native::outside as out;
        if f == out::SI_IMAGE {
            if let Some(job) = self.job.take() {
                let a: [u32; 5] = core::array::from_fn(|i| args.get(i).copied().unwrap_or(0));
                self.out = Some(inside(m, self.world, a, job));
            }
            return 0;
        }
        if f == out::SI_DO_SUBREGION {
            return 1;
        }
        self.world.call(m, f, args)
    }
}

/// The renderer's start as `SI_image` makes it (the camera record copied to `SI_database`, the other four arguments
/// stored, `SI_initialize`: the material propagation, the texture counts, the default light), then `SI_update` for
/// every frame asked; at the first frame the tree, geometry, materials and textures are read, at every frame the
/// matrices, the camera and the lights.
fn inside(m: &mut Mem, world: &mut World<NoWatch>, a: [u32; 5], job: Job) -> Result<Value, String> {
    use si_render::img::{f as rf, g as rg, Outside as RenderOutside};
    let Job { frames: want, deform, texture_path, lights_read, notes } = job;
    let mut files = std::mem::take(&mut world.pictures);
    let mut rest = si_rensetup::native::world::RenderSide(&mut *world);
    let mut r = si_render::adapter::Render::new(&mut rest, &mut files);
    r.keep_lines = false;
    let [camera, word_a, word_b, lights, roots] = a;
    m.copy(SI_DATABASE, camera, 0x13c);
    m.set_u32(rg::DB_WORD_A, word_a);
    m.set_u32(rg::DB_WORD_B, word_b);
    m.set_u32(rg::DB_LIGHTS, lights);
    m.set_u32(rg::DB_ROOT, roots);
    let dictionary = m.u32(SI_ELEM_DICTS).wrapping_add((m.i32(m.u32(SI_PROC_DATA)) as u32).wrapping_mul(DICT_SIZE));
    RenderOutside::call(&mut r, m, rf::SI_INITIALIZE, &[dictionary]);
    if let Some(fault) = m.fault() {
        return Err(format!("SI_initialize faulted: {fault:?}"));
    }
    let ctl = SI_DEFAULT_CONTROL;
    let (first, last, step) = (m.i16(ctl + 0x1f4) as i32, m.i16(ctl + 0x1f6) as i32, m.i16(ctl + 0x1f8) as i32);
    let (width, height, pixel_ratio) = (m.i16(ctl + 0x59a), m.i16(ctl + 0x59c), m.f32(ctl + 0x154));
    // the set-up's crop (render a sub-rectangle of the picture) as the renderer's controls hold it after the start
    let (rs, _) = si_render::adapter::controls_from_image(m);
    let crop = if rs.crop != 0 {
        json!({"left": f(rs.crop_left), "right": f(rs.crop_right), "top": f(rs.crop_top), "bottom": f(rs.crop_bottom),
               "width": rs.crop_width, "height": rs.crop_height})
    } else {
        Value::Null
    };
    let rate = m.f32(SI_FRAME_RATE);
    let step_n = step.max(1) as usize;
    let all: Vec<i32> = (first..=last.max(first)).step_by(step_n).collect();
    let frames: Vec<i32> = match want {
        Frames::First => vec![first],
        Frames::All => all,
        Frames::AtMost(n) => all.into_iter().take(n.max(1)).collect(),
    };
    let mut d = Dump { materials: Table::default(), textures: Table::default(), textures3d: Table::default(), not_carried: BTreeMap::new(), notes };
    let sequence = m.i16(rg::SEQUENCE);
    if sequence != 2 {
        d.notes.push(format!("the render controls are not in sequence mode ({sequence}): the renderer does not bring the scene to each frame's time; the dump does (SI_sceneUpdateItems)"));
    }
    let mut tree: Vec<u32> = Vec::new();
    let mut id_of: BTreeMap<u32, usize>;
    let mut models: Vec<Value> = Vec::new();
    let mut total_tris = 0usize;
    let mut deforming: Vec<usize> = Vec::new();
    let mut chain: Vec<u32> = Vec::new();
    let mut per_frame: Vec<(i32, f32, Vec<[f32; 16]>, Value, Vec<Value>, Vec<Option<Vec<[f32; 3]>>>)> = Vec::new();
    let mut env0 = Value::Null;
    let mut scene_shaders = Value::Null;
    let mut render_notes: Vec<String> = Vec::new();
    for (k, &fr) in frames.iter().enumerate() {
        m.set_i16(rg::FRAME_NOW, fr as i16);
        // SI_image: time = (field × 0.5 + frame) / SI_frameRate, rounded to float
        let time = ((x(0i32) * 0.5f64 + x(fr)) / x(rate)).to_f32();
        if sequence != 2 {
            let saved = m.i32(SI_FRAME_CURRENT);
            m.set_i32(SI_FRAME_CURRENT, fr);
            RenderOutside::call(&mut r, m, SI_SCENE_UPDATE_ITEMS, &[time.to_bits(), 1, SI_DATABASE, 1]);
            m.set_i32(SI_FRAME_CURRENT, saved);
        }
        RenderOutside::call(&mut r, m, rf::SI_UPDATE, &[time.to_bits()]);
        for n in r.notes.drain(..) {
            if !render_notes.contains(&n) {
                render_notes.push(n);
            }
        }
        if let Some(fault) = m.fault() {
            d.notes.push(format!("frame {fr}: the update faulted: {fault:?}; the frames stop here"));
            if k == 0 {
                return Err(format!("the first frame's update faulted: {fault:?}"));
            }
            break;
        }
        if k == 0 {
            // ---- the first frame: the tree, geometry, materials, textures
            tree = nodes(m, m.u32(rg::DB_ROOT));
            id_of = tree.iter().enumerate().map(|(i, &a)| (a, i)).collect();
            chain = light_chain(m);
            env0 = environment_now(m);
            // the scene's own shader references: the atmosphere (volume) shader and the output and lens lists
            scene_shaders = json!({
                "atmosphere": udf_ref(m, m.u32(0x0878_1c4c)),
                "output": udf_list(m, 0x0878_1be4, 0x0878_1bec),
                "lens": udf_list(m, 0x0878_1be8, 0x0878_1bf0),
            });
            use si_render::img::dict as dk;
            let models_list = m.u32(dictionary + dk::DICT_LISTS).wrapping_add(m.u32(rg::CID_MODELS).wrapping_mul(dk::LIST_SIZE));
            let n_elems = m.i32(models_list + dk::LIST_COUNT).max(0) as u32;
            let mut geo_by_def: BTreeMap<(u32, Vec<usize>), usize> = BTreeMap::new();
            for (i, &a) in tree.clone().iter().enumerate() {
                let kind = m.i32(a + 0x30);
                let algo = m.i32(a + 0x2ec);
                let nmat = m.i16(a + 0x206).max(0) as u32;
                let arr = m.u32(a + 0x20c);
                let mut mat_ids = Vec::new();
                for j in 0..nmat {
                    let ma = if arr != 0 { m.u32(arr + 4 * j) } else { 0 };
                    if ma == 0 {
                        break;
                    }
                    mat_ids.push(d.material(m, ma, &texture_path, frames[0]));
                }
                let name_ptr = m.u32(a + 4);
                let elem = m.i32(a + 0x354);
                let registered = if elem >= 0 && (elem as u32) < n_elems { m.u32(m.u32(models_list + dk::LIST_ELEMS).wrapping_add((elem as u32).wrapping_mul(dk::ELEM_SIZE)) + dk::ELEM_DATA) } else { 0 };
                let def = m.u32(a + 0x48);
                let shared = (def != 0).then(|| geo_by_def.get(&(def, mat_ids.clone())).copied()).flatten();
                let geo = if shared.is_some() {
                    None
                } else {
                    match geometry(m, &mut d, a, kind, &mat_ids, &texture_path, frames[0]) {
                        Ok(g) => g,
                        Err(e) => {
                            d.notes.push(format!("node {i} ({}): geometry not made: {e}", text(m, name_ptr)));
                            None
                        }
                    }
                };
                if geo.is_some() && def != 0 {
                    geo_by_def.insert((def, mat_ids.clone()), i);
                }
                match kind {
                    7 | 8 => d.note("metaballs (the renderer's metaball surface is not built by the dump: carried as an empty)"),
                    9 => d.note("NURBS curves (not rendered: carried as an empty)"),
                    5 => d.note("skeleton joints (carried as empties with their animation baked per frame; no armature)"),
                    6 => d.note("splines (not rendered: carried as curves)"),
                    _ => {}
                }
                if m.u32(a + 0x2f0) != 0 && kind != 5 {
                    d.note("kinematics on a node (IK, constraints, expressions: baked into the per-frame matrices)");
                }
                if kind == 4 && m.i16(a + 0x34) > 1 {
                    deforming.push(i);
                }
                // the node's own 3D texture (+0x224): the renderer applies it where the triangle's material has none
                let own3d = m.u32(a + 0x224);
                let texture3d = (own3d != 0).then(|| d.texture3d(m, own3d));
                let tris = geo.as_ref().map_or(0, |g| g.triangles);
                total_tris += tris;
                let latdep = m.u32(a + 0x44);
                let flip = {
                    let l = if latdep != 0 { latdep } else { a };
                    let sc = if m.i32(0x0878_1560) == 1 { vec3(m, l + 0x3a0) } else { vec3(m, l + 0x16c) };
                    si_math::geometry::transform_flips_normals(si_math::types::Vector::new(sc[0], sc[1], sc[2]), m.i16(SI_RIGHT_HAND) != 0, m.u16(a + 0x08) & 8 != 0)
                };
                models.push(json!({
                    "id": i,
                    "name": if name_ptr == 0 { Value::Null } else { Value::from(text(m, name_ptr)) },
                    "kind": kind,
                    "dummy": kind == -1,
                    "father": id_of.get(&m.u32(a + 0x2bc)).copied(),
                    "visible": m.i16(a + 0x2b8),
                    "algorithm": algo,
                    "registered": (registered != 0 && registered != a).then(|| id_of.get(&registered).copied()).flatten(),
                    "scaling": fv(&vec3(m, a + 0x100)),
                    "rotation": fv(&vec3(m, a + 0x10c)),
                    "translation": fv(&vec3(m, a + 0x158)),
                    "global_matrix": fv(&mat16(m, a + 0x1c4)),
                    "latdep": (latdep != 0 && latdep != a).then(|| id_of.get(&latdep).copied()).flatten(),
                    "normals_flipped": flip,
                    "materials": mat_ids,
                    "texture3d": texture3d,
                    "geometry": geo.map(|g| g.json),
                    "geometry_of": shared,
                }));
            }
        }
        let mats: Vec<[f32; 16]> = tree.iter().map(|&a| mat16(m, a + 0x1c4)).collect();
        let verts: Vec<Option<Vec<[f32; 3]>>> = if deform {
            tree.iter()
                .map(|&a| {
                    if m.i32(a + 0x30) != 4 || m.u32(a + 0x48) == 0 {
                        return None;
                    }
                    match si_render::inimage::node_source_at(m, a).def {
                        NodeDef::Mesh { mesh, .. } => Some(mesh.vertices.iter().map(|v| v.pos).collect()),
                        _ => None,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let cam = camera_json(&r.p.camera, &r.p.st.camera, &r.p.view);
        let lts: Vec<Value> = light_chain(m).iter().map(|&l| light_now(m, l, !lights_read.contains(&l), &tree)).collect();
        per_frame.push((fr, time, mats, cam, lts, verts));
    }
    drop(r);
    world.pictures = files;
    let animated: Vec<usize> = (0..tree.len()).filter(|&i| per_frame.iter().any(|f| f.2[i] != per_frame[0].2[i])).collect();
    let deformed: Vec<usize> = if deform { (0..tree.len()).filter(|&i| per_frame.iter().any(|f| f.5.get(i) != per_frame[0].5.get(i))).collect() } else { Vec::new() };
    let frames_json: Vec<Value> = per_frame
        .iter()
        .map(|(fr, t, mats, cam, lts, verts)| {
            let mut o = Map::new();
            o.insert("frame".into(), Value::from(*fr));
            o.insert("time".into(), f(*t));
            o.insert("matrices".into(), Value::Object(animated.iter().map(|&i| (i.to_string(), fv(&mats[i]))).collect()));
            o.insert("camera".into(), cam.clone());
            o.insert("lights".into(), Value::Array(lts.clone()));
            if !deformed.is_empty() {
                o.insert("vertices".into(), Value::Object(deformed.iter().filter_map(|&i| verts.get(i).cloned().flatten().map(|v| (i.to_string(), Value::Array(v.iter().map(|p| fv(p)).collect())))).collect()));
            }
            Value::Object(o)
        })
        .collect();
    if !deform && !deforming.is_empty() {
        d.notes.push(format!("{} mesh node(s) have more than one definition (shape animation): rerun with --deform for their vertices per frame", deforming.len()));
    }
    if animated.iter().any(|&i| models[i]["kind"] == 5) {
        d.note("skeleton animation (carried as per-frame matrices of the joints and of the enveloped meshes; deformation with --deform)");
    }
    let picture_missing = d.textures.out.iter().filter(|t| t["found"] == false).count();
    if picture_missing > 0 {
        d.notes.push(format!("{picture_missing} texture picture file(s) not found where the renderer looks"));
    }
    for n in render_notes {
        d.notes.push(format!("renderer: {n}"));
    }
    let not_carried: Vec<Value> = d.not_carried.iter().map(|(k, n)| json!({"what": k, "count": n})).collect();
    let camera0 = per_frame.first().map(|f| f.3.clone()).unwrap_or(Value::Null);
    let lights0 = per_frame.first().map(|f| Value::Array(f.4.clone())).unwrap_or(Value::Null);
    Ok(json!({
        "settings": {
            "frame_first": first, "frame_last": last, "frame_step": step, "frame_rate": f(rate),
            "width": width, "height": height, "pixel_ratio": f(pixel_ratio),
            "right_hand": m.i16(SI_RIGHT_HAND) != 0,
            // the set-up's BACK_CULLING (renderer controls +0x20c, as si-rensetup's bridge stores it)
            "back_culling": m.i16(SI_DEFAULT_CONTROL + 0x20c) != 0,
            // the set-up's MAX_RAY_DEPTH (+0x1e4) and GLOBAL_SWITCHES (+0x584 .. +0x58e), as the renderer runs them
            // (a scene without a set-up file keeps the controls' start values)
            "ray_depth": m.i16(SI_DEFAULT_CONTROL + 0x1e4),
            // ANTIALIAS: +0x15a 0 FIXED / 1 ADAPTIVE (other: no line), +0x158 the filter, +0x15c the level,
            // +0x16c the threshold
            "antialias": match m.i16(SI_DEFAULT_CONTROL + 0x15a) {
                k @ (0 | 1) => json!({"mode": if k == 0 { "FIXED" } else { "ADAPTIVE" },
                    "filter": m.i16(SI_DEFAULT_CONTROL + 0x158), "level": m.i16(SI_DEFAULT_CONTROL + 0x15c),
                    "threshold": f(m.f32(SI_DEFAULT_CONTROL + 0x16c))}),
                _ => Value::Null,
            },
            "switches": {
                "material": m.i16(SI_DEFAULT_CONTROL + 0x584) != 0, "textures_2d": m.i16(SI_DEFAULT_CONTROL + 0x586) != 0,
                "textures_3d": m.i16(SI_DEFAULT_CONTROL + 0x588) != 0, "reflection": m.i16(SI_DEFAULT_CONTROL + 0x58a) != 0,
                "refraction": m.i16(SI_DEFAULT_CONTROL + 0x58c) != 0, "shadows": m.i16(SI_DEFAULT_CONTROL + 0x58e) != 0,
            },
            "crop": crop,
            "environment": env0,
        },
        "models": models,
        "materials": d.materials.out,
        "textures": d.textures.out,
        "textures3d": d.textures3d.out,
        "scene_shaders": scene_shaders,
        "camera": camera0,
        "lights": lights0,
        "animated_models": animated,
        "deformed_models": deformed,
        "frames": frames_json,
        "counts": {"models": tree.len(), "render_triangles": total_tris, "lights": chain.len(), "materials": d.materials.out.len(), "textures": d.textures.out.len(), "textures3d": d.textures3d.out.len(), "frames": per_frame.len()},
        "not_carried": not_carried,
        "notes": d.notes,
    }))
}

fn run(a: Args) -> Result<Value, String> {
    let install = si_install::locate(a.install.as_deref(), si_install::Need::Render).map_err(|e| e.to_string())?;
    // the original's tables and messages, read from the user's program file (root, SI_INSTALL, dev copy; si-rt)
    si_rt::add_root(&install.root);
    si_rt::check_or_exit("si-import-dump");
    let scene_path = a.scene.canonicalize().map_err(|e| format!("{}: {e}", a.scene.display()))?;
    let Loaded { mut sc, notes, database, name } = load_scene(&install, &scene_path)?;
    let mut m = std::mem::take(&mut sc.mem);
    let mut world = World::new(NoWatch);
    world.pictures.files = true;
    // the picture a texture that cannot be read is replaced by (`noIcon.pic` of the resource directory)
    world.pictures.default = std::fs::read(&install.no_icon).ok();
    world.pictures.texture_path = match m.u32(si_scene::t2dpath::SI_TEXTURE_PATH) {
        0 => Vec::new(),
        p => m.cstr(p),
    };
    let texture_path = world.pictures.texture_path.clone();
    let placed = si_rensetup::native::bridge::place_scene(&mut m, &mut world, &sc)?;
    if let Some(setup) = &sc.setup {
        si_rensetup::native::bridge::controls_from_setup(&mut m, &setup.render);
    }
    if let Some(fault) = m.fault() {
        return Err(format!("the scene's set-up faulted: {fault:?}"));
    }
    let job = Job { frames: a.frames, deform: a.deform, texture_path, lights_read: placed.lights.clone(), notes };
    let mut grab = Grab { world: &mut world, job: Some(job), out: None };
    let status = si_rensetup::native::rnd_dial::sequence_render(&mut grab, &mut m);
    let mut v = match grab.out.take() {
        Some(r) => r?,
        None => return Err(format!("the render command did not reach the renderer (status {status:#x})")),
    };
    let shaders = shader_files(&scene_path, &database);
    // the set-up's mental ray shaders (MR_ATMOS_SHADER, MR_RENDER_SHADER, MR_LENS_SHADER), where the image holds none
    if let (Some(setup), Some(ss)) = (&sc.setup, v.get_mut("scene_shaders").and_then(Value::as_object_mut)) {
        let mr = &setup.render.mental_ray;
        let one = |name: &Option<Vec<u8>>, active: i16| -> Value {
            json!({"name": name.as_deref().map(bytes_text), "active": active != 0, "from": "setup"})
        };
        if ss.get("atmosphere").is_none_or(Value::is_null) {
            if let Some(u) = &mr.atmos_shader {
                ss.insert("atmosphere".into(), one(&u.name, u.active));
            }
        }
        for (k, list) in [("output", &mr.output_shaders), ("lens", &mr.lens_shaders)] {
            if ss.get(k).and_then(Value::as_array).is_none_or(|a| a.is_empty()) {
                ss.insert(k.into(), Value::Array(list.iter().flatten().map(|u| one(&u.name, u.active)).collect()));
            }
        }
    }
    if !shaders.is_empty() {
        for mat in v["materials"].as_array_mut().into_iter().flatten() {
            if let Some(sh) = mat.get_mut("shaders").and_then(Value::as_object_mut) {
                for (k, r) in sh.iter_mut() {
                    let ch = match k.as_str() {
                        "material" => "MATERIAL_SHADERS",
                        "volume" => "VOLUME_SHADERS",
                        "shadow" => "SHADOW_SHADERS",
                        _ => "PHOTON_SHADERS",
                    };
                    link_shader(r, ch, &shaders);
                }
            }
        }
        for t in v["textures"].as_array_mut().into_iter().flatten() {
            if let Some(r) = t.get_mut("shader") {
                link_shader(r, "TEXTURE2D_SHADERS", &shaders);
            }
        }
        for t in v["textures3d"].as_array_mut().into_iter().flatten() {
            if let Some(r) = t.get_mut("shader") {
                link_shader(r, "TEXTURE3D_SHADERS", &shaders);
            }
        }
        for l in v["lights"].as_array_mut().into_iter().flatten() {
            if let Some(r) = l.get_mut("shader") {
                link_shader(r, "LIGHT_SHADERS", &shaders);
            }
        }
        for fr in v["frames"].as_array_mut().into_iter().flatten() {
            for l in fr["lights"].as_array_mut().into_iter().flatten() {
                if let Some(r) = l.get_mut("shader") {
                    link_shader(r, "LIGHT_SHADERS", &shaders);
                }
            }
        }
    }
    if let Some(ss) = v.get_mut("scene_shaders").and_then(Value::as_object_mut) {
        if let Some(r) = ss.get_mut("atmosphere") {
            if !r.is_null() {
                link_shader(r, "VOLUME_SHADERS", &shaders);
            }
        }
        for (k, ch) in [("output", "OUTPUT_SHADERS"), ("lens", "CAMERA_SHADERS")] {
            for r in ss.get_mut(k).and_then(Value::as_array_mut).into_iter().flatten() {
                link_shader(r, ch, &shaders);
            }
        }
    }
    let o = v.as_object_mut().ok_or("dump")?;
    o.insert("shaders".into(), Value::Array(shaders));
    o.insert("format".into(), Value::from(FORMAT));
    o.insert("version".into(), Value::from(VERSION));
    o.insert("scene".into(), json!({"path": scene_path.to_string_lossy(), "database": database.to_string_lossy(), "name": name}));
    // the picture the renderer reads for a texture whose own picture cannot be read (SI_readTextureFile:
    // "replaced by default texture"), so that the importer can carry it the same way
    o.insert(
        "default_picture".into(),
        if install.no_icon.is_file() { Value::from(install.no_icon.to_string_lossy().into_owned()) } else { Value::Null },
    );
    o.insert(
        "axes".into(),
        Value::from("Softimage: right-handed, Y up, camera looks along -Z; row vectors (p' = p M, translation in the last row of each 4x4, written row by row). Blender = (x, -z, y) of Softimage, units 1:1."),
    );
    Ok(v)
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("si-import-dump: {e}");
            }
            eprintln!("usage: si-import-dump [--install DIR] [--frames all|first|N] [--deform] SCENE.dsc [OUT.json]");
            std::process::exit(2);
        }
    };
    let out = a.out.clone();
    // the engine's panics end the dump with a message, not a backtrace
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(a)));
    let v = match r {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            eprintln!("si-import-dump: {e}");
            std::process::exit(1);
        }
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            eprintln!("si-import-dump: the engine panicked: {msg}");
            std::process::exit(1);
        }
    };
    let text = serde_json::to_string(&v).expect("json");
    match out {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, text) {
                eprintln!("si-import-dump: {}: {e}", p.display());
                std::process::exit(1);
            }
        }
        None => println!("{text}"),
    }
}
