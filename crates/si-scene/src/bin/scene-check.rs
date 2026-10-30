// SPDX-License-Identifier: MIT
//! Corpus check for scene loading and saving (finding 0018): loads every scene of every
//! database under a directory the way the program's "Get Scene" does, and writes each one
//! back the way "Save Scene" writes its description.
//!
//!   scene-check <dir> [load|save|all|saveall|image] [--list] [--strict] [--rsrc <Relations.rsrc>] [--only <text>]
//!
//! `--strict` turns off the two stand-ins for packages that are not ported (`load::Assume`).
//!
//! `load`: for each scene, every element the description names is looked for and read in
//! the version the program would pick; counts of scenes loaded completely, of elements by
//! chapter and outcome, and every failure with its cause.
//!
//! `save`: load, write the description, compare with the file byte for byte. The file a
//! save is compared with (here and in `saveall`) carries the skins' self-relations 207 the
//! original's load adds and its save writes (`with_relation_set`).
//!
//! `saveall`: load, then the whole of "Save Scene" into a file system in memory, every file
//! written compared with the file it was loaded from (`scene_check_parts/saveall.rs`).
//!
//! `image`: load, and for each scene one line about the memory image the load built: the
//! heap at its peak, what is live in it, what the allocator did, how much of the image was
//! touched, and every access outside memory; then the largest.
//!
//! One scene is in memory at a time. Run under a memory cap:
//! `(ulimit -v 2000000; timeout 600 scene-check <dir>)`.

use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_scene::calls::{Channel, Recorder};
use si_scene::load::{self, Outcome, Scene};
use si_scene::save::{self, Build};
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;

#[path = "scene_check_parts/saveall.rs"]
mod saveall;
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

struct Args {
    dir: PathBuf,
    mode: String,
    list: bool,
    rsrc: Option<PathBuf>,
    only: Option<String>,
    strict: bool,
}

fn args() -> Args {
    let mut a = Args { dir: PathBuf::new(), mode: "all".into(), list: false, rsrc: None, only: None, strict: false };
    let mut it = std::env::args().skip(1);
    let mut pos = 0;
    while let Some(x) = it.next() {
        match x.as_str() {
            "--list" => a.list = true,
            "--strict" => a.strict = true,
            "--rsrc" => a.rsrc = it.next().map(PathBuf::from),
            "--only" => a.only = it.next(),
            _ => {
                if pos == 0 {
                    a.dir = PathBuf::from(&x);
                } else {
                    a.mode = x;
                }
                pos += 1;
            }
        }
    }
    if pos == 0 {
        eprintln!("usage: scene-check <dir> [load|save|all|saveall|image] [--list] [--strict] [--rsrc <Relations.rsrc>] [--only <text>]");
        std::process::exit(2);
    }
    a
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The description the original's own save writes after its load, as far as the load alone
/// changes it: the file, plus a line `N N 207 ;` for each skin it has none for. A load's
/// `SI_envelopRelationSet` (0x084bd5a8) relates each skin (an element of the models' chapter
/// in use whose model's word +0x31c is 5, 6 or 8) to itself by a relation 207 (`FlexEnv`)
/// where it has none, and the save writes it: the original's plain save of QuickStart's
/// Actor-Walk (`re/out/oracle/tools/evalScene/saved/QuickStart_databases_QuickStart/ev`)
/// has `0 0 207 ;` and `1 1 207 ;`, which the file lacks. The skins are taken from the
/// models' words in the image the load built, not from the dictionary's relations, so a
/// relation the port makes or leaves out wrongly still shows. Each line goes after the
/// element's last line of the MODELS/MODELS block, where that save has them. Returns the
/// file to compare with, the lines added, and the skins that could not be placed (no such
/// block, or a name not in the file's list).
fn with_relation_set(sc: &Scene, file: &[u8]) -> (Vec<u8>, usize, usize) {
    use si_scene::world;
    let m = &sc.mem;
    let ed = world::current_dict(m);
    let models = world::cid(m).model;
    let words = |l: &[u8]| -> Vec<Vec<u8>> { l.split(|c| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).map(|w| w.to_vec()).collect() };
    let lines: Vec<&[u8]> = file.split(|&c| c == b'\n').collect();
    // the names of the models' chapter, in the file's numbering
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut in_models = false;
    for l in &lines {
        let w = words(l);
        match w.first().map(|x| x.as_slice()) {
            Some(b"EndOfELEMENTS") => break,
            Some(b"CHAPTER") => in_models = w.get(1).is_some_and(|c| c == b"MODELS"),
            Some(b"EndOfCHAPTER") => in_models = false,
            Some(n) if in_models && w.last().is_some_and(|x| x == b";") => names.push(n.to_vec()),
            _ => {}
        }
    }
    // the MODELS/MODELS block of the relations: its lines, and the self-relations 207 it has
    let mut section = false;
    let mut block: Option<(usize, usize)> = None;
    for (i, l) in lines.iter().enumerate() {
        let w = words(l);
        if w.first().is_some_and(|x| x == b"RELATIONS") {
            section = true;
        } else if w.first().is_some_and(|x| x == b"EndOfRELATIONS") {
            break;
        } else if section && w == [b"CHAPTER".to_vec(), b"MODELS".to_vec(), b"CHAPTER".to_vec(), b"MODELS".to_vec()] {
            let end = (i + 1..lines.len()).find(|&j| words(lines[j]).first().is_some_and(|x| x == b"EndOfCHAPTER")).unwrap_or(lines.len());
            block = Some((i + 1, end));
            break;
        }
    }
    let num = |l: &[u8], k: usize| -> Option<i64> { words(l).get(k).and_then(|x| std::str::from_utf8(x).ok()).and_then(|x| x.parse().ok()) };
    let mut add: Vec<i64> = Vec::new();
    let mut unplaced = 0;
    for index in 0..world::nb_elems(m, ed, models) {
        let id = si_scene::dict::ElemId { chap: models, index };
        let Some(e) = world::slot(m, ed, id) else { continue };
        let data = m[e].data.addr();
        if m[e].status != 0 || data == 0 || !matches!(m.i32(data + 0x31c), 5 | 6 | 8) {
            continue;
        }
        let n = world::full_name(m, ed, id).and_then(|f| names.iter().position(|x| *x == f));
        match (n, block) {
            (Some(n), Some((a, b))) => {
                let n = n as i64;
                if !lines[a..b].iter().any(|l| num(l, 0) == Some(n) && num(l, 1) == Some(n) && num(l, 2) == Some(207)) {
                    add.push(n);
                }
            }
            _ => unplaced += 1,
        }
    }
    let Some((a, b)) = block else { return (file.to_vec(), 0, unplaced) };
    if add.is_empty() {
        return (file.to_vec(), 0, unplaced);
    }
    add.sort();
    let added = add.len();
    let mut out: Vec<Vec<u8>> = lines[..a].iter().map(|l| l.to_vec()).collect();
    let mut pending = add.into_iter().peekable();
    for l in &lines[a..b] {
        let first = num(l, 0).unwrap_or(i64::MAX);
        while let Some(&n) = pending.peek() {
            if n >= first {
                break;
            }
            out.push(format!("       {n} {n} 207 ; ").into_bytes());
            pending.next();
        }
        out.push(l.to_vec());
    }
    for n in pending {
        out.push(format!("       {n} {n} 207 ; ").into_bytes());
    }
    out.extend(lines[b..].iter().map(|l| l.to_vec()));
    (out.join(&b'\n'), added, unplaced)
}

#[derive(Default)]
struct LoadTotals {
    scenes: usize,
    complete: usize,
    not_read: usize,
    by_chapter: BTreeMap<(String, &'static str), usize>,
    failures: Vec<String>,
    incomplete: Vec<String>,
    refused: BTreeMap<i32, usize>,
    messages: BTreeMap<String, usize>,
    calls: BTreeMap<&'static str, usize>,
    relations: usize,
    update_nodes: usize,
    cycles: usize,
    fallback: usize,
    in_sub_dir: usize,
    case_only: usize,
    renamed: Vec<String>,
}

fn outcome_word(o: &Outcome) -> &'static str {
    match o {
        Outcome::Read { complete: true, .. } => "read to its end",
        Outcome::Read { complete: false, .. } => "read, not to its end",
        Outcome::Located { .. } => "found, reader not ported",
        Outcome::Node => "node of a root's file",
        Outcome::NodeMissing => "node missing in the root's file",
        Outcome::PartOfRoot => "shares its root's object",
        Outcome::Missing { .. } => "file missing",
        Outcome::Invalid { .. } => "file refused",
        Outcome::NoModel => "no model to read onto",
        Outcome::NotRead => "not read from disk",
    }
}

#[derive(Default)]
struct SaveTotals {
    scenes: usize,
    plain: BTreeMap<&'static str, usize>,
    replayed: BTreeMap<&'static str, usize>,
    needs: BTreeMap<(&'static str, &'static str), usize>,
    replayed_no_mprflg: usize,
    nt_scenes: Vec<String>,
    lines_only: usize,
    differ: Vec<String>,
    elements_same: usize,
    elements_nt: usize,
    elements_old: usize,
    elements_differ: Vec<String>,
}

fn build_word(b: Build) -> &'static str {
    match b {
        Build::Linux => "Linux build (bytes compared, two-digit exponents)",
        Build::Nt => "NT build (case ignored, three-digit exponents)",
        Build::NtNamesLinuxNumbers => "NT name order, Linux number format",
        Build::LinuxNamesNtNumbers => "Linux name order, NT number format",
        Build::Old => "plain strcmp name order (versions 2.61 to 2.65)",
    }
}

fn main() {
    let a = args();
    let mut files = Vec::new();
    walk(&a.dir, &mut files);
    let rsrc_path = a.rsrc.clone().or_else(|| files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned());
    let Some(rsrc_path) = rsrc_path else {
        eprintln!("no Relations.rsrc under {} (give one with --rsrc)", a.dir.display());
        std::process::exit(2);
    };
    let rsrc = std::fs::read(&rsrc_path).expect("Relations.rsrc");
    println!("relations resource file: {}", rsrc_path.display());
    let scenes: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "dsc") && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "SCENES"))
        .filter(|p| a.only.as_ref().is_none_or(|o| p.to_string_lossy().contains(o.as_str())))
        .collect();
    let mut databases: Vec<&Path> = scenes.iter().map(|p| p.parent().unwrap().parent().unwrap()).collect();
    databases.dedup();
    println!("{} scene descriptions in {} databases", scenes.len(), databases.len());
    let fs = RealFs;
    let mut lt = LoadTotals::default();
    let mut st = SaveTotals::default();
    let mut sa = saveall::Totals::default();
    let with_load = a.mode == "load" || a.mode == "all" || a.mode == "saveall";
    let mut image_peak = (0u32, String::new(), 0usize);
    let mut image_faults = 0u32;
    let mut pictures_put_back = 0usize;
    let mut contents = ContentsTotals::default();
    // scenes and lines `with_relation_set` added, skins it could not place, and the scenes
    let mut relation_set = (0usize, 0usize, 0usize, Vec::<String>::new());
    for path in scenes {
        let data = std::fs::read(path).unwrap_or_default();
        if !data.starts_with(b"SDSC") {
            continue;
        }
        let db = path.parent().unwrap().parent().unwrap();
        let name = path.file_stem().unwrap().as_bytes().to_vec();
        let mut sc = match Scene::start(&rsrc) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Relations.rsrc: {e}");
                std::process::exit(1);
            }
        };
        if !a.strict {
            sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
        }
        let mut info = DbInfo::new();
        info.name = db.file_name().map(|n| n.as_bytes().to_vec());
        info.path = Some(db.as_os_str().as_bytes().to_vec());
        let mut log = Log::default();
        si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
        let mut un = Recorder::default();
        let ok = load::load_scene(&mut sc, &fs, &mut info, &name, &mut un);
        // the writer is checked against the files as their machine had the pictures' paths
        // (the load's picture search replaces them where it finds the picture: finding 0018,
        // section 19)
        pictures_put_back += si_scene::t2dpath::file_paths_in_stores(&mut sc);
        let label = path.strip_prefix(&a.dir).unwrap_or(path).display().to_string();
        // what the saves are compared with: the file, with the skins' relations 207 the
        // original's load adds (`with_relation_set`)
        let reference = if ok && a.mode != "image" {
            let (r, added, unplaced) = with_relation_set(&sc, &data);
            if added != 0 {
                relation_set.0 += 1;
                relation_set.1 += added;
                relation_set.3.push(format!("{label}: {added}"));
            }
            relation_set.2 += unplaced;
            r
        } else {
            data.clone()
        };
        if a.mode == "image" {
            // the image the load built: how much heap it needed, what is live in it, and
            // whether any access of the load went outside memory
            let h = sc.mem.heap_stats();
            let verify = sc.mem.heap_verify().err().unwrap_or_default();
            let elements: i32 = (0..sc.mem[si_scene::world::current_dict(&sc.mem)].nb_lists).map(|c| si_scene::world::nb_elems(&sc.mem, si_scene::world::current_dict(&sc.mem), c)).sum();
            println!(
                "{label}\tloaded {ok}\telements {elements}\theap peak {}\tused {}\tlive {} blocks {} bytes\tfree {}\tmallocs {}\tfrees {}\treallocs in place {} moved {} ({} bytes)\timage touched {}\tfaults {}{}{}",
                h.peak,
                h.used,
                h.live_blocks,
                h.live_bytes,
                h.free_bytes,
                h.mallocs,
                h.frees,
                h.reallocs_in_place,
                h.reallocs_moved,
                h.bytes_moved,
                sc.mem.committed_bytes(),
                sc.mem.fault_count(),
                sc.mem.fault().map_or(String::new(), |f| format!(" first {f:?}")),
                if verify.is_empty() { String::new() } else { format!("\tHEAP: {verify}") },
            );
            // what the readers on the image put into it, summed over the corpus
            let c = &sc.contents;
            for (k, v) in &c.records {
                *contents.records.entry(k.to_string()).or_default() += v;
            }
            for (k, v) in &c.models_unread {
                *contents.unread.entry(k.to_string()).or_default() += v;
            }
            for (k, v) in &c.hooks {
                *contents.hooks.entry(k.to_string()).or_default() += v;
            }
            for (k, v) in &c.ported {
                *contents.ported.entry(k.to_string()).or_default() += v;
            }
            for (what, p) in &c.disagreed {
                contents.disagreed.push(format!("{what} {}", String::from_utf8_lossy(p)));
            }
            for (what, p) in &c.no_return {
                contents.disagreed.push(format!("no return ({what}) {}", String::from_utf8_lossy(p)));
            }
            contents.unpaired += c.models_unpaired;
            contents.meshes += c.meshes_rebuilt;
            contents.not_filed += c.polygons_not_filed;
            contents.status += c.status.len();
            contents.graph[0] += c.graph.paired;
            contents.graph[1] += c.graph.image_only;
            contents.graph[2] += c.graph.standin_only;
            for (k, v) in &c.not_in_image {
                *contents.foreign.entry(k.to_string()).or_default() += v;
            }
            for (k, v) in &c.scnrel_run {
                *contents.scnrel_run.entry(*k).or_default() += v;
            }
            for ((k, why), v) in &c.scnrel_refused {
                *contents.scnrel_refused.entry(format!("{k} ({why})")).or_default() += v;
            }
            if h.peak > image_peak.0 {
                image_peak = (h.peak, label.clone(), sc.mem.committed_bytes());
            }
            image_faults += sc.mem.fault_count();
            continue;
        }
        let mut loaded_completely = false;
        if with_load {
            lt.scenes += 1;
            let mut complete = ok;
            if !ok {
                lt.failures.push(format!("{label}: the description could not be read"));
            }
            for r in &sc.report {
                let chap = text(&sc.dict.ed.lists[r.chap as usize].name);
                *lt.by_chapter.entry((chap.clone(), outcome_word(&r.outcome))).or_default() += 1;
                match &r.outcome {
                    Outcome::Read { complete: true, fallback, sub_dir, .. } => {
                        lt.fallback += *fallback as usize;
                        lt.in_sub_dir += *sub_dir as usize;
                    }
                    Outcome::Node | Outcome::PartOfRoot | Outcome::NotRead | Outcome::Located { .. } => {}
                    other => {
                        complete = false;
                        let detail = match other {
                            Outcome::Missing { tried } => {
                                // a file of that name in another letter case: the scene was
                                // saved on a file system that does not tell them apart
                                let p = Path::new(std::ffi::OsStr::from_bytes(tried));
                                let stem = text(&r.name).to_lowercase();
                                let other_case = p.parent().and_then(|d| std::fs::read_dir(d).ok()).is_some_and(|rd| {
                                    rd.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().starts_with(&format!("{stem}.")))
                                });
                                if other_case {
                                    lt.case_only += 1;
                                }
                                format!("{}last tried {}", if other_case { "(a file of that name exists in another letter case) " } else { "" }, text(tried))
                            }
                            Outcome::Invalid { path } | Outcome::Read { path, .. } => text(path),
                            _ => String::new(),
                        };
                        lt.failures.push(format!("{label}: {chap} [{}]: {} {detail}", text(&r.name), outcome_word(other)));
                    }
                }
            }
            for (relid, _, _) in &sc.refused_relations {
                *lt.refused.entry(*relid).or_default() += 1;
            }
            for m in &sc.messages {
                let ch = match m.channel {
                    Channel::Status(l) => format!("status {l}"),
                    Channel::Stdout => "stdout".into(),
                    Channel::Stderr => "stderr".into(),
                };
                // group by the fixed part of the message
                let t = text(&m.text);
                let key: String = t.split(['[', ':']).next().unwrap_or("").trim().to_string();
                *lt.messages.entry(format!("{ch}: {key}")).or_default() += 1;
                if matches!(m.channel, Channel::Stderr) {
                    complete = false;
                }
            }
            for c in &un.calls {
                *lt.calls.entry(c.name).or_default() += 1;
            }
            if sc.renamed_duplicates != 0 {
                lt.renamed.push(format!("{label}: {}", sc.renamed_duplicates));
            }
            lt.relations += sc.dict.ed.lists.iter().flat_map(|l| l.elems.iter()).flat_map(|e| e.forward.iter()).map(|l| l.rels.len()).sum::<usize>();
            lt.update_nodes += sc.upd_list.nodes.len();
            if sc.dict.ed.rec.upd_list.dirty != 0 {
                lt.cycles += 1;
            }
            loaded_completely = complete;
            if complete {
                lt.complete += 1;
            } else {
                lt.incomplete.push(label.clone());
            }
            lt.not_read += (!ok) as usize;
        }
        if ok && a.mode == "dump" {
            use std::io::Write;
            let out = save::write_replayed(&mut sc, &data, Build::Linux);
            std::io::stdout().write_all(&out).unwrap();
        }
        if ok && (a.mode == "save" || a.mode == "all") {
            st.scenes += 1;
            let r = save::check_against(&mut sc, &reference);
            match r.elements_same {
                Some(si_scene::elemsort::NameOrder::Linux) => st.elements_same += 1,
                Some(si_scene::elemsort::NameOrder::Plain) => {
                    st.elements_same += 1;
                    st.elements_old += 1;
                }
                Some(si_scene::elemsort::NameOrder::Nt) => {
                    st.elements_same += 1;
                    st.elements_nt += 1;
                }
                None => st.elements_differ.push(format!("{label} (version {})", sc.dsc_version)),
            }
            if let Some(b) = r.same {
                *st.plain.entry(build_word(b)).or_default() += 1;
            }
            match r.same_replayed {
                Some(b) => {
                    *st.replayed.entry(build_word(b)).or_default() += 1;
                    st.replayed_no_mprflg += r.no_mprflg as usize;
                    // what the file needs: which name orders and which number formats fit
                    let has = |x: Build| r.fits.contains(&x);
                    let names = if has(Build::Linux) || has(Build::LinuxNamesNtNumbers) {
                        if has(Build::Nt) || has(Build::NtNamesLinuxNumbers) { "either name order" } else { "Linux name order only" }
                    } else if has(Build::Nt) || has(Build::NtNamesLinuxNumbers) {
                        "NT name order only"
                    } else {
                        "plain strcmp only"
                    };
                    let numbers = if has(Build::Linux) || has(Build::NtNamesLinuxNumbers) || has(Build::Old) {
                        if has(Build::Nt) || has(Build::LinuxNamesNtNumbers) { "either exponent width" } else { "two-digit exponents only" }
                    } else {
                        "three-digit exponents only"
                    };
                    *st.needs.entry((names, numbers)).or_default() += 1;
                    if names == "NT name order only" || numbers == "three-digit exponents only" {
                        st.nt_scenes.push(format!("{label} (version {}): {names}, {numbers}", sc.dsc_version));
                    }
                }
                None => {
                    st.lines_only += r.same_lines as usize;
                    st.differ.push(format!("{label} (version {}): {}{}", sc.dsc_version, r.why, if r.same_lines { "; same lines in another order" } else { "" }));
                }
            }
        }
        if a.mode == "saveall" && loaded_completely {
            saveall::content_check(&mut sc, &mut info, &label, &data, &reference, &mut sa);
            drop(sc);
            // loaded again for the save as the dialogue does it
            let mut sc = Scene::start(&rsrc).expect("Relations.rsrc");
            if !a.strict {
                sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
            }
            let mut info = DbInfo::new();
            info.name = db.file_name().map(|n| n.as_bytes().to_vec());
            info.path = Some(db.as_os_str().as_bytes().to_vec());
            let mut log = Log::default();
            si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
            let mut un = Recorder::default();
            if load::load_scene(&mut sc, &fs, &mut info, &name, &mut un) {
                si_scene::t2dpath::file_paths_in_stores(&mut sc);
                saveall::name_check(&mut sc, &mut info, &label, &mut sa);
            }
        }
        // the scene is dropped here: one scene in memory at a time
    }
    if a.mode == "image" {
        println!("largest heap: {} bytes at its peak, {} bytes of the image touched: {}", image_peak.0, image_peak.2, image_peak.1);
        println!("faults of the image over all loads: {image_faults}");
        println!("pictures the load's search found in a database (their paths put back in the stores for the writer's checks): {pictures_put_back}");
        println!("records the readers on the image made: {:?}", contents.records);
        println!("model files not read on the image, by the first reader missing: {:?}; hierarchies that did not pair with the value reader's: {}", contents.unread, contents.unpaired);
        println!("meshes rebuilt by the adapter {}, of them with a polygon not filed {}; status lines of the readers {}", contents.meshes, contents.not_filed, contents.status);
        println!("graph list in the image: {} elements tied to a node paired with the stand-in's, {} with a node in the image only, {} with a stand-in node only", contents.graph[0], contents.graph[1], contents.graph[2]);
        println!("objects of replaced elements made in the image (adapter): {:?}", contents.foreign);
        println!("files the two readers disagreed on: {}", contents.disagreed.len());
        for d in contents.disagreed.iter().take(20) {
            println!("  {d}");
        }
        println!("SI_scnRelationsFromElemDict on the image, calls by relation: {:?}", contents.scnrel_run);
        println!("SI_scnRelationsFromElemDict left recorded, by relation and reason: {:?}", contents.scnrel_refused);
        println!("calls answered by a port or an adapter: {:?}", contents.ported);
        println!("calls answered 0 (not on the image): {:?}", contents.hooks);
        return;
    }
    if with_load {
        println!("\n== loading ==");
        println!("{} scenes; {} loaded completely; {} not completely ({} descriptions not read)", lt.scenes, lt.complete, lt.scenes - lt.complete, lt.not_read);
        println!("elements by chapter and outcome:");
        let mut per_chapter: BTreeMap<&str, usize> = BTreeMap::new();
        for ((c, _), n) in &lt.by_chapter {
            *per_chapter.entry(c.as_str()).or_default() += n;
        }
        for ((c, o), n) in &lt.by_chapter {
            println!("  {c:20} {n:6}  {o}");
        }
        println!("  total per chapter: {}", per_chapter.iter().map(|(c, n)| format!("{c} {n}")).collect::<Vec<_>>().join(", "));
        println!("  total: {}", per_chapter.values().sum::<usize>());
        println!("files found under the unversioned name: {}; in another sub-directory than named: {}", lt.fallback, lt.in_sub_dir);
        println!("relations recorded (forward entries): {}; refused by the dictionary: {:?}", lt.relations, lt.refused);
        println!("update list nodes after the topological sort: {}; scenes where the sort found a cycle: {}", lt.update_nodes, lt.cycles);
        println!("scenes in which the duplicate name check renamed elements: {}", lt.renamed.len());
        if a.list {
            for r in &lt.renamed {
                println!("  {r}");
            }
        }
        println!("messages of the original:");
        for (m, n) in &lt.messages {
            println!("  {n:6}  {m}");
        }
        println!("calls into packages not ported, by name ({} names):", lt.calls.len());
        for (m, n) in &lt.calls {
            println!("  {n:6}  {m}");
        }
        println!("{} failures ({} of them files that exist under a name in another letter case){}", lt.failures.len(), lt.case_only, if a.list { ":" } else { " (--list shows them)" });
        if a.list {
            for f in &lt.failures {
                println!("  {f}");
            }
            println!("scenes not loaded completely:");
            for f in &lt.incomplete {
                println!("  {f}");
            }
        }
    }
    if a.mode == "save" || a.mode == "all" || a.mode == "saveall" {
        println!(
            "\nskins' self-relations 207 a load's SI_envelopRelationSet adds where the file has none, and the original's save writes: {} lines in {} scenes, put into the file the saves are compared with; skins not placed: {}",
            relation_set.1, relation_set.0, relation_set.2
        );
        if a.list {
            for s in &relation_set.3 {
                println!("  {s}");
            }
        }
    }
    if a.mode == "saveall" {
        saveall::print(&sa, a.list);
    }
    if a.mode == "save" || a.mode == "all" {
        println!("\n== saving ==");
        println!("{} scenes loaded and written back; stand-ins: {}", st.scenes, if a.strict { "none" } else { "custom effects installed, model relations put back from the description" });
        println!("ELEMENTS section (the order and numbering produced by the sort) identical to the file: {} ({} of them only with the NT build's name order, {} only with plain strcmp)", st.elements_same, st.elements_nt, st.elements_old);
        println!("whole file identical, elements gone through in index order (the order of the file, after a load):");
        for (b, n) in &st.plain {
            println!("  {n:4}  {b}");
        }
        println!("  {:4}  in all", st.plain.values().sum::<usize>());
        println!("whole file identical, elements gone through in the order the file's own lines show:");
        for (b, n) in &st.replayed {
            println!("  {n:4}  {b}");
        }
        println!("  {:4}  in all ({} of them files without any MPRFLG item, compared without)", st.replayed.values().sum::<usize>(), st.replayed_no_mprflg);
        println!("what the identical files need:");
        for ((names, numbers), n) in &st.needs {
            println!("  {n:4}  {names}, {numbers}");
        }
        println!("different: {} ({} hold the same lines in another order)", st.differ.len(), st.lines_only);
        if a.list {
            println!("files that differ:");
            for d in &st.differ {
                println!("  {d}");
            }
            println!("files whose ELEMENTS section differs:");
            for d in &st.elements_differ {
                println!("  {d}");
            }
            println!("files identical only with the NT build's name order or number format:");
            for d in &st.nt_scenes {
                println!("  {d}");
            }
        }
    }
}

/// What the readers on the image did over the corpus (`image` mode).
#[derive(Default)]
struct ContentsTotals {
    records: std::collections::BTreeMap<String, usize>,
    unread: std::collections::BTreeMap<String, usize>,
    hooks: std::collections::BTreeMap<String, usize>,
    ported: std::collections::BTreeMap<String, usize>,
    disagreed: Vec<String>,
    unpaired: usize,
    meshes: usize,
    not_filed: usize,
    status: usize,
    graph: [usize; 3],
    foreign: std::collections::BTreeMap<String, usize>,
    scnrel_run: std::collections::BTreeMap<i32, usize>,
    scnrel_refused: std::collections::BTreeMap<String, usize>,
}
