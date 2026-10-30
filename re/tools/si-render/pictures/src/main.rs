// SPDX-License-Identifier: MIT
//! The port's pictures of the sample scenes (finding 0012, section 20).
//!
//! `render-pictures <installed dir> <output dir>` loads every scene of the sample databases
//! the way `rensetup-check` does (si-scene's Get Scene into the memory image, si-rensetup's
//! render command), renders the first frame of each scene's range with si-render's
//! `SI_image`, and leaves the frame as a picture file: `<output dir>/<scene>/<name>.<frame>.pic`,
//! `<name>` being the scene's output name as `buildName` makes it (the scene's picture path is
//! the directory `/<scene>` below the output directory), the output name without its
//! directory: what the original's batch renderer writes with `-D <database> -s <first>
//! <first> 1` into the database's `RENDER_PICTURES`, here in a directory per scene. A second
//! scene of the same name (in another database) gets `<scene>@<database>`.
//!
//! Writes `<output dir>/index.tsv`: database, scene, directory, the command's answer,
//! `SI_image`'s answer, triangles filed, lines written, the files written, and what ended a
//! scene early. Nothing here reproduces a function of the original.

use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_mem::Mem;
use si_rensetup::native::globals as g;
use si_rensetup::native::outside as out;
use si_rensetup::native::rec::ctl;
use si_rensetup::native::world::{Watch, World};
use si_rensetup::native::{bridge, matter, rnd_dial};
use si_scene::load::{self, Scene};
use std::collections::BTreeSet;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

/// The user at the sub-region box accepts it as the scene's set-up has it (as in
/// `rensetup-check`); the render window's routine (`SI_openRenderWindow (0x082c9c14)`, not
/// ported) is answered with the one part of it the picture depends on: with the crop
/// switch on (the 32-bit word at +0x224 of the controls it is handed), the sub-region box
/// from fractions of the picture to pixels, `SI_updateSubregionValues` (si-render's port).
/// Every other hook answers 0.
struct Accept {
    windows: usize,
    boxes_to_pixels: usize,
}
struct Quiet;
impl si_render::img::Outside for Quiet {
    fn call(&mut self, _: &mut Mem, _: u32, _: &[u32]) -> u32 {
        0
    }
}
impl Watch for Accept {
    fn hook(&mut self, m: &mut Mem, f: si_mem::FnPtr, a: &[u32]) -> Option<u32> {
        if f == out::SI_OPEN_RENDER_WINDOW {
            self.windows += 1;
            let ren = a.first().copied().unwrap_or(0);
            if ren != 0 && m.i32(ren + 0x224) != 0 {
                si_render::matter_image::update_subregion_values(m, &mut Quiet, ren);
                self.boxes_to_pixels += 1;
            }
            return None;
        }
        (f == out::SI_DO_SUBREGION).then_some(1)
    }
}

/// `SI_matterInit (0x08291bec)`'s first two steps, which no crate makes at start-up yet:
/// `si_lightInitialize(&SI_defaultLight, 1)` (si-elemio's port) and the light's position
/// (+0x34, +0x38, +0x3c) the float at 0x086551e8, 100.0. `initData` copies this record into
/// a scene that has no light (finding 0012, section 21).
fn default_light(m: &mut Mem) {
    struct NoCalls;
    impl si_elemio::image::Outside for NoCalls {
        fn call(&mut self, _: &mut Mem, _: &'static si_elemio::image::Routine, _: &[u32]) -> u32 {
            0
        }
        fn file_left(&mut self, _: &mut Mem, _: u32) -> u32 {
            0
        }
    }
    let l = si_render::adapter::SI_DEFAULT_LIGHT;
    si_elemio::image::lights::light_initialize(m, &mut NoCalls, l, 1);
    for o in [0x34, 0x38, 0x3c] {
        m.set_f32(l + o, 100.0);
    }
}

/// The screen the check assumes: 1280 × 1024.
const SCREEN: (i16, i16) = (1279, 1023);

struct Row {
    command: i32,
    image: u32,
    triangles: i32,
    lines: usize,
    files: Vec<String>,
    end: String,
    /// The model nodes in use by kind (+0x30): count, and how many have a geometry (+0x48).
    kinds: String,
    /// What the renderer's set-up said (each distinct note once, with its count).
    notes: String,
}

fn render(sc: &mut Scene, label: &str, root: &Path, no_icon: &Option<Vec<u8>>) -> Result<Row, String> {
    let mut m = std::mem::take(&mut sc.mem);
    let mut world = World::new(Accept { windows: 0, boxes_to_pixels: 0 });
    world.pictures.default = no_icon.clone();
    // texture pictures from the file system, by the names the load's search left, as in
    // rensetup-check (PICTURES_NO_PICTURE_FILES=1: the default only)
    world.pictures.files = std::env::var_os("PICTURES_NO_PICTURE_FILES").is_none();
    world.pictures.texture_path = match m.u32(si_scene::t2dpath::SI_TEXTURE_PATH) {
        0 => Vec::new(),
        p => m.cstr(p),
    };
    let r = (|| -> Result<Row, String> {
        let m = &mut m;
        let placed = bridge::place_scene(m, &mut world, sc)?;
        m[g::SI_XMAXSCREEN] = SCREEN.0;
        m[g::SI_YMAXSCREEN] = SCREEN.1;
        if std::env::var_os("PICTURES_NO_DEFAULT_LIGHT").is_none() {
            default_light(m);
        }
        matter::ren_control_init(m, g::SI_DEFAULT_CONTROL.0);
        matter::matter_fade_init(m);
        matter::fog_init_default(m);
        if let Some(setup) = &sc.setup {
            bridge::controls_from_setup(m, &setup.render);
        }
        // the output name without its directory, as `soft -R <scene> -D <database>` takes
        // it (finding 0019, 3.2): a name that is a path of the authors' machines would
        // otherwise not open, here as in the original
        let at = g::SI_DEFAULT_CONTROL.0 + 0x17c;
        let p = m.u32(at);
        if p != 0 {
            if let Some(k) = m.cstr(p).iter().rposition(|&c| c == b'/') {
                m.set_u32(at, p + k as u32 + 1);
            }
        }
        // the picture path: the scene's directory below the output directory
        let path = format!("/{label}");
        let local = m.local(path.len() as u32 + 1);
        m.put_cstr(local, path.as_bytes());
        let copy = m.strdup(local);
        m.release(path.len() as u32 + 1);
        m[g::SI_PICTURE_PATH] = copy;
        let first = m.i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_FIRST);
        m.set_i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_LAST, first);
        si_render::adapter::write_pictures_under(Some(root.to_path_buf()));
        let command = rnd_dial::sequence_render(&mut world, m);
        si_render::adapter::write_pictures_under(None);
        let image = world.images.first();
        let mut end = String::new();
        if let Some(f) = m.fault() {
            end = format!("fault {:#x} ({:?})", f.addr, f.access);
        }
        if let Some(i) = image {
            if let Some(n) = i.notes.iter().find(|n| n.starts_with("fatal") || n.starts_with("fault")) {
                end = if end.is_empty() { n.clone() } else { format!("{end}; {n}") };
            }
        }
        let files = image.map(|i| i.pictures.iter().map(|p| String::from_utf8_lossy(&p.0).into_owned()).collect()).unwrap_or_default();
        // the model nodes in use by kind, as rensetup-check's RENSETUP_LIST counts them
        let mut kinds: std::collections::BTreeMap<i32, (usize, usize)> = Default::default();
        {
            use si_types::dict::ELEM_VALID;
            use si_types::model::Model;
            use si_types::Ptr32;
            let models = m[m[placed.dict].lists.add(m[g::SI_CID].model)];
            for i in 0..models.nb_elems {
                let e = models.elems.add(i);
                let node: Ptr32<Model> = m[e].data.cast();
                if m[e].status == ELEM_VALID && !node.is_null() && si_scene::world::obj_of(m[e].data) == 0 {
                    let k = kinds.entry(m.i32(node.0 + 0x30)).or_default();
                    k.0 += 1;
                    k.1 += (m.u32(node.0 + 0x48) != 0) as usize;
                }
            }
        }
        let kinds = kinds.iter().map(|(k, (n, g))| format!("{k}:{n}/{g}")).collect::<Vec<_>>().join(" ");
        let mut seen: Vec<(String, usize)> = Vec::new();
        for n in image.map(|i| i.notes.clone()).unwrap_or_default() {
            let n = n.replace(['\t', '\n'], " ");
            match seen.iter_mut().find(|s| s.0 == n) {
                Some(s) => s.1 += 1,
                None => seen.push((n, 1)),
            }
        }
        let notes = seen.iter().map(|(n, c)| if *c > 1 { format!("{n} (x{c})") } else { n.clone() }).collect::<Vec<_>>().join(" | ");
        Ok(Row {
            command,
            image: image.map_or(0, |i| i.status),
            triangles: image.map_or(0, |i| i.triangles),
            lines: image.map_or(0, |i| i.pictures.iter().map(|p| p.1).sum()),
            files,
            end,
            kinds,
            notes,
        })
    })();
    si_render::adapter::write_pictures_under(None);
    sc.mem = m;
    r
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: render-pictures <installed dir> <output dir>");
        std::process::exit(2);
    }
    let (dir, root) = (Path::new(&args[0]), PathBuf::from(&args[1]));
    // the original's data and messages come from the install at run time (crates/si-rt); checked
    // before the panic hook below silences the scenes' panics
    si_rt::add_root(dir);
    si_rt::check_or_exit("render-pictures");
    std::fs::create_dir_all(&root).expect("the output directory");
    let mut files = Vec::new();
    walk(dir, &mut files);
    let rsrc_path = files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned().expect("Relations.rsrc");
    let rsrc = std::fs::read(&rsrc_path).expect("Relations.rsrc");
    let no_icon = std::fs::read(rsrc_path.with_file_name("noIcon.pic")).ok();
    // the resource directory, absolute, as `SI_resdir` holds it; `SI_texturePath` is the
    // working directory (`standAloneRender`: `strdup(SI_cwd)`), as rensetup-check sets both
    let resdir = rsrc_path.parent().and_then(|d| d.canonicalize().ok()).map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    let cwd = std::env::current_dir().map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    let scenes: Vec<&PathBuf> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "dsc") && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "SCENES")).collect();
    let only = std::env::var("PICTURES_ONLY").ok();
    let mut index = std::fs::File::create(root.join("index.tsv")).expect("index.tsv");
    writeln!(index, "database\tscene\tdirectory\tcommand\tSI_image\ttriangles\tlines\tfiles\tended\tkinds\tnotes\trelations refused").unwrap();
    std::panic::set_hook(Box::new(|_| {}));
    let mut used = BTreeSet::new();
    let (mut n, mut done, mut written) = (0, 0, 0);
    for path in scenes {
        if only.as_deref().is_some_and(|o| !path.to_string_lossy().contains(o)) {
            continue;
        }
        let data = std::fs::read(path).unwrap_or_default();
        if !data.starts_with(b"SDSC") {
            continue;
        }
        let db = path.parent().unwrap().parent().unwrap();
        let db_label = db.strip_prefix(dir).unwrap_or(db).display().to_string();
        let name = path.file_stem().unwrap().as_bytes().to_vec();
        let scene = String::from_utf8_lossy(&name).into_owned();
        let mut label = scene.clone();
        if !used.insert(label.clone()) {
            label = format!("{scene}@{}", db.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            used.insert(label.clone());
        }
        let _ = std::fs::create_dir_all(root.join(&label));
        n += 1;
        let mut sc = Scene::start(&rsrc).expect("Relations.rsrc");
        // the renderer's part of the binary's data segment (si_render::data: SI_rightHand,
        // mailboxCurrent, cellVolumn, ...), which the process holds from its start, before
        // anything is loaded, and which neither the loader nor the render command's world
        // writes into the image (finding 0012, section 21). PICTURES_NO_DATA=1 and
        // PICTURES_NO_DEFAULT_LIGHT=1 leave this or SI_matterInit's light out, to measure what
        // each changes.
        if std::env::var_os("PICTURES_NO_DATA").is_none() {
            si_render::data::init(&mut sc.mem);
        }
        // the two directories the picture search of the load and SI_readTextureFile read
        for (at, text) in [(si_scene::t2dpath::SI_RESDIR, &resdir[..]), (si_scene::t2dpath::SI_TEXTURE_PATH, &cwd[..])] {
            let p = sc.mem.malloc(text.len() as u32 + 1);
            sc.mem.put_cstr(p, text);
            sc.mem.set_u32(at, p);
        }
        sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
        let mut info = DbInfo::new();
        info.name = db.file_name().map(|n| n.as_bytes().to_vec());
        info.path = Some(db.as_os_str().as_bytes().to_vec());
        let mut log = Log::default();
        si_db::database_list_chapters(&RealFs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
        let mut un = si_scene::calls::Recorder::default();
        let ok = load::load_scene(&mut sc, &RealFs, &mut info, &name, &mut un);
        let complete = ok && sc.report.iter().all(|r| !matches!(r.outcome, load::Outcome::Missing { .. } | load::Outcome::Invalid { .. } | load::Outcome::NodeMissing | load::Outcome::NoModel));
        // a scene the load did not complete is rendered anyway, as the original carries on past
        // the missing pieces (finding 0018, section 7.1; finding 0023, section 29); the index's
        // notes then begin with what it missed. PICTURES_SKIP_INCOMPLETE=1 skips it, as before
        let incomplete = std::env::var_os("PICTURES_SKIP_INCOMPLETE").is_none().then(|| {
            sc.report
                .iter()
                .filter_map(|r| {
                    let what = match &r.outcome {
                        load::Outcome::Missing { tried } => format!("file missing (last tried {})", String::from_utf8_lossy(tried)),
                        load::Outcome::Invalid { path } => format!("file refused ({})", String::from_utf8_lossy(path)),
                        load::Outcome::NodeMissing => "node missing in the root's file".into(),
                        load::Outcome::NoModel => "no model to read it onto".into(),
                        _ => return None,
                    };
                    Some(format!("{} [{}]: {what}", si_db::chap_info_from_chap_id(r.chap).map_or_else(|| r.chap.to_string(), |c| c.directory.to_string()), String::from_utf8_lossy(&r.name)))
                })
                .collect::<Vec<_>>()
                .join("; ")
        });
        let row = if !complete && incomplete.is_none() {
            Err("not loaded completely".to_string())
        } else {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(&mut sc, &label, &root, &no_icon))) {
                Ok(r) => r,
                Err(e) => Err(format!("panic: {}", e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
            }
        };
        match row {
            Ok(r) => {
                done += r.end.is_empty() as usize;
                // what is on disk in the scene's directory (an output name that is a path of
                // its own writes elsewhere, or fails)
                let mut on_disk: Vec<String> = std::fs::read_dir(root.join(&label)).map(|rd| rd.flatten().map(|e| format!("{label}/{}", e.file_name().to_string_lossy())).collect()).unwrap_or_default();
                on_disk.sort();
                if on_disk.is_empty() && !r.files.is_empty() {
                    on_disk.push(format!("(asked: {})", r.files.join(" ")));
                } else {
                    written += on_disk.len();
                }
                // the loader's SI_scnRelationsFromElemDict calls it could not make on the image
                // (relation id: why), from si-scene's own bookkeeping
                let refused = sc.contents.scnrel_refused.iter().map(|((id, why), n)| format!("{id}x{n}: {why}")).collect::<Vec<_>>().join(" | ");
                let notes = match (&incomplete, complete) {
                    (Some(m), false) => format!("not loaded completely, rendered anyway ({m}); {}", r.notes),
                    _ => r.notes.to_string(),
                };
                writeln!(index, "{db_label}\t{scene}\t{label}\t{:#x}\t{:#x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", r.command, r.image, r.triangles, r.lines, on_disk.join(" "), r.end, r.kinds, notes, refused).unwrap();
            }
            Err(e) => {
                writeln!(index, "{db_label}\t{scene}\t{label}\t\t\t\t\t\t{e}").unwrap();
            }
        }
    }
    println!("scenes {n}; rendered to their end {done}; picture files written {written}; index: {}", root.join("index.tsv").display());
}
