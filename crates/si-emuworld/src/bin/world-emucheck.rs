// SPDX-License-Identifier: MIT
//! `world-emucheck`: ports run on a scene's image, held against the original routines the
//! emulation harness ran on the same image (finding 0011, "A real scene as the world").
//!
//!   world-emucheck <vector dir> <dump dir>
//!
//! The vector directory has one sub-directory per scene, written by
//! `re/tools/emu/scene_proof.py`; each vector file starts with a line naming the scene's
//! image (`# scene LABEL IDX region=HASH …`). The scene is built again from its database
//! (`si_emuworld::build`, the stage its index names) and its image held against those
//! hashes; then for every case the image is put back, the case's buffers placed where the
//! harness put them, the port called with the case's arguments, and compared: the answer,
//! the buffers, and every byte of the image the routine changed.
//!
//! `EMUCHECK_MUTATE=1` flips one bit of every answer and of the first byte each port
//! changes before the comparison: every compared case must then differ.
//!
//! Run under the memory cap: `(ulimit -v 2000000; timeout 900 world-emucheck …)`.

use si_emuworld::build::{build, Stage};
use si_emuworld::case::{parse, scene_header, Case};
use si_emuworld::check::{first_difference, harness_bytes, place, port_bytes, word, Base};
use si_mem::vecfmt::{Names, Word};
use si_mem::{CStr32, Mem, OnFault, Ptr32};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a port did with one case.
struct Ran {
    eax: Option<u32>,
}

/// The ports, by the original's name. `a` are the argument words in C order.
fn port(name: &str, m: &mut Mem, a: &[u32]) -> Option<Ran> {
    use si_dict::elemdict as d;
    use si_model::model_image::{bounds, tree};
    use si_view::graflist as g;
    let p = |k: usize| a.get(k).copied().unwrap_or(0);
    let i = |k: usize| p(k) as i32;
    let h = |k: usize| p(k) as u16 as i16;
    let eax = |v: u32| Some(Ran { eax: Some(v) });
    match name {
        // ---- si-dict: dictionary queries
        "SI_elemdictGetElem" => eax(d::get_elem(m, Ptr32::new(p(0)), i(1), i(2)).addr()),
        "SI_elemdictGetChapName" => eax(d::get_chap_name(m, Ptr32::new(p(0)), i(1)).addr()),
        "SI_elemdictFindListByName" => eax(d::find_list_by_name(m, Ptr32::new(p(0)), CStr32::new(p(1))) as u32),
        "SI_elemdictFindElemByName" => eax(d::find_elem_by_name(m, Ptr32::new(p(0)), i(1), CStr32::new(p(2)), CStr32::new(p(3))) as u32),
        "SI_elemlistCountValidElems" => eax(d::elemlist_count_valid_elems(m, Ptr32::new(p(0))) as u32),
        // ---- si-view: walks of the graph list
        "SI_findGlData" => eax(g::find_gl_data(m, Ptr32::new(p(0)), Ptr32::new(p(1)), i(2)).addr()),
        "SI_stateGlData" => eax(g::state_gl_data(m, Ptr32::new(p(0)), Ptr32::new(p(1))) as u32),
        "si_graflistFindParent" => eax(g::graflist_find_parent(m, Ptr32::new(p(0))).addr()),
        "SI_graflistFindRoot" => eax(g::graflist_find_root(m, Ptr32::new(p(0))).addr()),
        "SI_graflistCountSelHierarchies" => eax(g::graflist_count_sel_hierarchies(m, Ptr32::new(p(0)), h(1)) as u32),
        "SI_grafnodeGetByName" => eax(g::grafnode_get_by_name(m, Ptr32::new(p(0)), CStr32::new(p(1)), i(2)).addr()),
        // ---- si-model: the model tree and its geometry
        "SI_modelRoot" => eax(tree::root(m, Ptr32::new(p(0))).addr()),
        "SI_modelNextNode" => eax(tree::next_node(m, Ptr32::new(p(0))).addr()),
        "SI_modelNextSon" => eax(tree::next_son(m, Ptr32::new(p(0)), Ptr32::new(p(1))).addr()),
        "SI_modelComputeBounding" => {
            bounds::compute_bounding(m, Ptr32::new(p(0)));
            Some(Ran { eax: None })
        }
        _ => None,
    }
}

#[derive(Default, Clone)]
struct Row {
    cases: usize,
    identical: usize,
    different: usize,
    /// the original stopped (a fault) and so did the port
    both_stop: usize,
    /// the original stopped and the port did not, or the other way round
    one_stop: usize,
    /// not compared: the original read memory nobody defined
    undefined: usize,
    first: Option<String>,
}

fn value_of(w: Option<&[Word]>) -> Option<u32> {
    match w?.first()? {
        Word::Value { value, .. } => Some(*value as u32),
        _ => None,
    }
}

fn check_case(m: &mut Mem, base: &Base, c: &Case, mutate: bool) -> Result<Option<Result<(), String>>, String> {
    base.restore(m);
    let names: Names = place(m, c)?;
    let args: Vec<u32> = c.args().iter().map(|(_, w)| word(m, &names, w)).collect();
    let routine = c.routine().to_string();
    m.on_fault(OnFault::Stop);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port(&routine, m, &args)));
    m.on_fault(OnFault::Record);
    let port_stopped = ran.is_err() || m.fault().is_some();
    let ran = match ran {
        Ok(Some(r)) => Some(r),
        Ok(None) => return Ok(None),
        Err(_) => None,
    };
    let original_stopped = c.stopped();
    if original_stopped || port_stopped {
        return Ok(Some(if original_stopped && port_stopped && c.has_note("STOP:fault") {
            Err("both-stop".into())
        } else {
            Err(format!("one-stop: original {}, port {}", c.notes.join(" "), if port_stopped { format!("{:?}", m.fault()) } else { "returned".into() }))
        }));
    }
    let ran = ran.unwrap();
    // the answer
    if let Some(want) = value_of(c.output("eax")) {
        let mut got = ran.eax.unwrap_or(0);
        if mutate {
            got ^= 1;
        }
        if got != want {
            return Ok(Some(Err(format!("eax: original {want:08x}, port {got:08x}"))));
        }
    }
    // the image
    let mut changes = base.changes(m);
    if mutate {
        if let Some((_, b)) = changes.first_mut() {
            b[0] ^= 1;
        }
    }
    if let Some(d) = first_difference(&harness_bytes(c), &port_bytes(&changes)) {
        return Ok(Some(Err(d)));
    }
    // the case's own buffers the original changed
    for (name, words) in &c.outputs {
        if let (Some(words), Some(addr)) = (words, names.addr(m, name)) {
            if c.buffers().iter().any(|(n, _)| n == name) {
                m.compare_words(addr, words, &names).map_err(|e| format!("{name}: {e}"))?;
            }
        }
    }
    Ok(Some(Ok(())))
}

fn main() {
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap_or(3));
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: world-emucheck <vector dir> <dump dir>");
        return 2;
    }
    let (vecs, dumps) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let mutate = std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "1");
    let mut scenes: Vec<PathBuf> = std::fs::read_dir(&vecs).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    scenes.sort();
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let (mut built, mut refused) = (0usize, Vec::<String>::new());
    for dir in &scenes {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vec")).collect()).unwrap_or_default();
        files.sort();
        let Some(first) = files.first() else { continue };
        let text = std::fs::read_to_string(first).unwrap_or_default();
        let Some((label, idx, hashes)) = text.lines().next().and_then(scene_header) else {
            refused.push(format!("{}: no scene line", first.display()));
            continue;
        };
        // the scene's database and name from its index
        let index = std::fs::read_to_string(dumps.join(&idx)).unwrap_or_default();
        let field = |k: &str| index.lines().find_map(|l| l.strip_prefix(&format!("{k}\t")).map(str::to_string));
        let (Some(db), Some(name), Some(rsrc), Some(stage)) = (field("db"), field("name"), field("rsrc"), field("stage").as_deref().and_then(Stage::parse)) else {
            refused.push(format!("{label}: index {idx} incomplete"));
            continue;
        };
        let Ok(rsrc) = std::fs::read(&rsrc) else {
            refused.push(format!("{label}: cannot read {rsrc}"));
            continue;
        };
        let w = match build(Path::new(&db), name.as_bytes(), &rsrc, stage) {
            Ok(w) => w,
            Err(e) => {
                refused.push(format!("{label}: {e}"));
                continue;
            }
        };
        let mut m = w.sc.mem;
        let now = si_emuworld::dump::hashes(&m);
        let same = hashes.iter().all(|(k, v)| now.iter().any(|(n, h)| n == k && h == v));
        if !same {
            refused.push(format!("{label}: the image built again is not the one the vectors were made on"));
            continue;
        }
        built += 1;
        let base = Base::new(&m);
        for f in &files {
            for line in std::fs::read_to_string(f).unwrap_or_default().lines() {
                let c = match parse(line) {
                    Ok(Some(c)) => c,
                    Ok(None) => continue,
                    Err(e) => {
                        refused.push(format!("{}: {e}", f.display()));
                        continue;
                    }
                };
                let row = rows.entry(c.name.clone()).or_default();
                row.cases += 1;
                if c.has_note("UNINIT") {
                    row.undefined += 1;
                    continue;
                }
                match check_case(&mut m, &base, &c, mutate) {
                    Ok(None) => row.undefined += 1,
                    Ok(Some(Ok(()))) => row.identical += 1,
                    Ok(Some(Err(e))) if e == "both-stop" => row.both_stop += 1,
                    Ok(Some(Err(e))) => {
                        if e.starts_with("one-stop") {
                            row.one_stop += 1;
                        } else {
                            row.different += 1;
                        }
                        if row.first.is_none() {
                            row.first = Some(format!("{label}: {e}: {}", &line[..line.len().min(200)]));
                        }
                    }
                    Err(e) => {
                        row.different += 1;
                        if row.first.is_none() {
                            row.first = Some(format!("{label}: {e}"));
                        }
                    }
                }
            }
        }
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9}", "function", "cases", "identical", "different", "both-stop", "one-stop", "undefined");
    let mut total = Row::default();
    for (name, r) in &rows {
        println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9}", name, r.cases, r.identical, r.different, r.both_stop, r.one_stop, r.undefined);
        total.cases += r.cases;
        total.identical += r.identical;
        total.different += r.different;
        total.both_stop += r.both_stop;
        total.one_stop += r.one_stop;
        total.undefined += r.undefined;
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9}", "TOTAL", total.cases, total.identical, total.different, total.both_stop, total.one_stop, total.undefined);
    println!("scenes built again and held against their images: {built}");
    for (name, r) in &rows {
        if let Some(f) = &r.first {
            println!("first difference of {name}: {f}");
        }
    }
    for r in &refused {
        println!("refused: {r}");
    }
    (total.different + total.one_stop != 0 || !refused.is_empty()) as i32
}
