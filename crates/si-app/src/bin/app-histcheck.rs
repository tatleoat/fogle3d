// SPDX-License-Identifier: MIT
//! `app-histcheck <dir>`: the undo/redo history (`si_app::history`, finding 0019, section 13)
//! against the vectors `re/tools/si-app/gen_history.py` writes: one routine of the original
//! per case, stepped in the emulation harness on made-up worlds. Same replay scheme as
//! `app-imagecheck`: the original's calls are replayed (name and arguments held against the
//! port's, the recorded answer handed back, the words a stand-in stored through an argument
//! stored the same way), and the result, the case's buffers, the heap blocks, the blocks
//! freed and the printed text are compared. Nothing here reproduces a function of the
//! original.
//!
//! `EMUCHECK_MUTATE=1` flips a bit of everything the port produced; `EMUCHECK_MUTATE=calls`
//! a bit of the first compared argument of every call.

use si_app::history as h;
use si_mem::vecfmt::{load_case_at, parse_words, FieldAt, Names, Word};
use si_mem::{Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;

/// One recorded call: `@name(args)=answer`.
#[derive(Clone, Debug)]
struct RecCall {
    name: String,
    args: Vec<Word>,
    ret: Word,
}

#[derive(Clone, Debug, Default)]
struct Case {
    name: String,
    inputs: Vec<(String, Option<Vec<Word>>)>,
    outputs: Vec<(String, Option<Vec<Word>>)>,
    calls: Vec<RecCall>,
    notes: Vec<String>,
}

impl Case {
    fn input(&self, name: &str) -> Option<&[Word]> {
        self.inputs.iter().find(|(n, _)| n == name).and_then(|(_, w)| w.as_deref())
    }
    fn output(&self, name: &str) -> Option<&Option<Vec<Word>>> {
        self.outputs.iter().find(|(n, _)| n == name).map(|(_, w)| w)
    }
    fn note(&self, prefix: &str) -> Option<&str> {
        self.notes.iter().find(|n| n.starts_with(prefix)).map(|s| s.as_str())
    }
}

/// Splits at the blanks that are not inside a quoted text.
fn tokens(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), None::<usize>, false);
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'"' && (i == 0 || b[i - 1] != b'\\') {
            quoted = !quoted;
        }
        if c == b' ' && !quoted {
            if let Some(s) = start.take() {
                out.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

fn parse(line: &str) -> Result<Option<Case>, String> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut c = Case::default();
    let mut part = 0;
    for (k, t) in tokens(line).into_iter().enumerate() {
        if k == 0 {
            c.name = t.to_string();
            continue;
        }
        match (part, t) {
            (_, "=>") => part = 1,
            (_, "!") => part = 2,
            (2, note) => c.notes.push(note.to_string()),
            (_, call) if call.starts_with('@') => {
                let open = call.find('(').ok_or("call without `(`")?;
                let close = call.rfind(")=").ok_or("call without `)=`")?;
                let args = &call[open + 1..close];
                c.calls.push(RecCall {
                    name: call[1..open].to_string(),
                    args: if args.is_empty() { Vec::new() } else { parse_words(args)?.unwrap_or_default() },
                    ret: parse_words(&call[close + 2..])?.and_then(|w| w.into_iter().next()).unwrap_or(Word::Value { bytes: 4, value: 0 }),
                });
            }
            (p, field) => {
                let (name, value) = field.split_once('=').ok_or_else(|| format!("no `=` in `{field}`"))?;
                let words = parse_words(value)?;
                if p == 0 {
                    c.inputs.push((name.to_string(), words));
                } else {
                    c.outputs.push((name.to_string(), words));
                }
            }
        }
    }
    Ok(Some(c))
}

/// The address of a global buffer `gXXXXXXXX[_label]`.
fn global_at(name: &str) -> Option<u32> {
    let hex = name.strip_prefix('g')?.get(..8)?;
    if name.len() > 9 && name.as_bytes()[9] != b'_' {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn is_buffer(name: &str) -> bool {
    !(name.starts_with("p_") || name.starts_with("v_") || name.starts_with("file:"))
}

fn place(m: &mut Mem, c: &Case) -> Result<Names, String> {
    let fields: Vec<FieldAt<'_>> = c.inputs.iter().filter(|(n, _)| is_buffer(n)).map(|(n, w)| (n.as_str(), global_at(n), w.as_deref())).collect();
    load_case_at(m, &fields)
}

fn word_value(m: &Mem, names: &Names, w: &Word) -> Option<u32> {
    match w {
        Word::Value { value, .. } => Some(*value as u32),
        Word::Unwritten { .. } => None,
        Word::Ref { name, offset } => names.addr(m, name).map(|a| a.wrapping_add(*offset)),
    }
}

fn arg(m: &Mem, names: &Names, c: &Case, name: &str) -> u32 {
    for prefix in ["p_", "v_"] {
        if let Some(w) = c.input(&format!("{prefix}{name}")).and_then(|w| w.first()) {
            return word_value(m, names, w).unwrap_or(0);
        }
    }
    0
}

const LOCAL: u32 = 0x57ac_57ac;

/// The outside world that replays the original's calls.
struct Replay<'c> {
    calls: &'c [RecCall],
    k: usize,
    names: Names,
    diff: Option<String>,
    printed: [Vec<u8>; 2],
    mutate_calls: bool,
}

impl Replay<'_> {
    fn differ(&mut self, text: String) {
        if self.diff.is_none() {
            self.diff = Some(text);
        }
    }
}

/// The arguments (by index) a stand-in stores its answers through: the `o` arguments of
/// `gen_history.py`'s table.
fn out_args(name: &str) -> &'static [usize] {
    match name {
        "SI_edElementAttValueGet" => &[2],
        "SI_elemdictFindElemFromGrafNode" => &[1, 2],
        "SI_subelUserDataCopyFromSetup" => &[2, 3],
        _ => &[],
    }
}

impl Outside for Replay<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let mut args = args.to_vec();
        let k = self.k;
        self.k += 1;
        let Some(rec) = self.calls.get(k) else {
            self.differ(format!("call {k}: the port calls {} after the original's last call", r.name));
            return Ret::NONE;
        };
        if self.mutate_calls {
            // a bit of the first argument the check compares
            for (i, w) in rec.args.iter().enumerate() {
                if i < args.len() && word_value(m, &self.names, w).is_some_and(|v| v != LOCAL) {
                    args[i] ^= 1;
                    break;
                }
            }
        }
        if rec.name != r.name {
            self.differ(format!("call {k}: original {}, port {}", rec.name, r.name));
        } else if rec.args.len() != args.len() {
            self.differ(format!("call {k} {}: {} arguments, port {}", r.name, rec.args.len(), args.len()));
        } else {
            for (i, (w, got)) in rec.args.iter().zip(&args).enumerate() {
                if let Some(want) = word_value(m, &self.names, w) {
                    let mask = if w.bytes() >= 4 { u32::MAX } else { (1u32 << (8 * w.bytes())) - 1 };
                    if want != LOCAL && want & mask != got & mask {
                        self.differ(format!("call {k} {} argument {i}: original {w}, port {got:#x}", r.name));
                    }
                }
            }
        }
        let ret = word_value(m, &self.names, &rec.ret);
        // what the stand-in stored through its `o` arguments follows as `NAME.out(...)`
        for &i in out_args(r.name) {
            let Some(next) = self.calls.get(self.k) else { break };
            if next.name != format!("{}.out", r.name) {
                break;
            }
            self.k += 1;
            let mut p = args[i];
            for w in &next.args {
                match w {
                    Word::Value { bytes: 2, value } => {
                        m.set_u16(p, *value as u16);
                        p += 2;
                    }
                    other => {
                        if let Some(v) = word_value(m, &self.names, other) {
                            m.set_u32(p, v);
                        }
                        p += 4;
                    }
                }
            }
        }
        if r.name == "SI_matrixCopy" || r.name == "SI_matrixInverse" {
            // the stand-in's effect: 64 bytes from the first argument to the second
            let bytes = m.bytes(args[0], 64).to_vec();
            m.set_bytes(args[1], &bytes);
        }
        Ret::eax(ret.unwrap_or(0))
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.printed[(stream == 2) as usize].extend_from_slice(text);
    }
}

enum Outcome {
    Returned(Option<u32>),
}

fn run(m: &mut Mem, o: &mut Replay, c: &Case, names: &Names) -> Result<Outcome, String> {
    let a = |m: &Mem, n: &str| arg(m, names, c, n);
    let done = Outcome::Returned(None);
    let ret = |v: u32| Outcome::Returned(Some(v));
    Ok(match c.name.as_str() {
        "SI_historyOpenCluster" => ret(h::history_open_cluster(m) as u32),
        "SI_historyCloseCluster" => ret(h::history_close_cluster(m) as u32),
        "SI_historySetActive" => {
            h::history_set_active(m, a(m, "on") as i16);
            done
        }
        "SI_getCurrentHistoryStruct" => ret(h::get_current_history_struct(m)),
        "SI_setCurrentHistoryStruct" => ret(h::set_current_history_struct(m, a(m, "h")) as u32),
        "SI_initHistoryStruct" => ret(h::init_history_struct(m, a(m, "h")) as u32),
        "SI_createHistoryStruct" => ret(h::create_history_struct(m)),
        "SI_destroyHistoryStruct" => ret(h::destroy_history_struct(m, a(m, "h")) as u32),
        "historyFixGrafnode" => {
            h::history_fix_grafnode(m, a(m, "old"), a(m, "new"));
            done
        }
        "verifyHierNSEstate" => {
            h::verify_hier_nse_state(m, o, a(m, "elem"));
            done
        }
        "historyDeleteLink" => ret(h::history_delete_link(m, o, a(m, "gn"), a(m, "link")) as u32),
        "historyCreateLink" => ret(h::history_create_link(m, o, a(m, "gn"), a(m, "link")) as u32),
        "modelHistXformSet" => {
            h::model_hist_xform_set(m, o, a(m, "model"), a(m, "rec"));
            done
        }
        "SI_historyDump" => {
            h::history_dump(m, o);
            done
        }
        "geoSubsDispose" => ret(h::geo_subs_dispose(m, o, a(m, "pp")) as u32),
        "historyNodeDispose" => {
            h::history_node_dispose(m, o, a(m, "pnode"), a(m, "all") as i16);
            done
        }
        "SI_historyDispose" => {
            h::history_dispose(m, o);
            done
        }
        "historyAddNode" => {
            h::history_add_node(m, o, a(m, "node"));
            done
        }
        "SI_historySetup" => {
            h::history_setup(m, o);
            done
        }
        "modelHistClusterSnapshot" => ret(h::model_hist_cluster_snapshot(m, o, a(m, "model"))),
        "modelHistLatSnapShot" => ret(h::model_hist_lat_snapshot(m, o, a(m, "model"), a(m, "tree") as i16)),
        "modelHistGeoSnapshot" => ret(h::model_hist_geo_snapshot(m, o, a(m, "model"), a(m, "target"))),
        "cameraHistorySnapshot" => ret(h::camera_history_snapshot(m, a(m, "cam"))),
        "cameraSwapHistory" => {
            h::camera_swap_history(m, a(m, "cam"), a(m, "rec"));
            done
        }
        "lightHistorySnapshot" => ret(h::light_history_snapshot(m, o, a(m, "light"))),
        "lightSwapHistory" => {
            h::light_swap_history(m, o, a(m, "light"), a(m, "rec"));
            done
        }
        "audioHistorySnapShot" => ret(h::audio_history_snapshot(m, a(m, "id") as i32, a(m, "a"), a(m, "b"), a(m, "c") as i32)),
        "actionsHistorySnapShot" => ret(h::actions_history_snapshot(m, o, a(m, "hier"), a(m, "a") as i32, a(m, "b") as i32, a(m, "motion") as i16)),
        "SI_grafnodeSaveHistoryDelete" => ret(h::grafnode_save_history_delete(m, o, a(m, "gn")) as u32),
        "grafnodeSaveHistoryAdd" => ret(h::grafnode_save_history_add(m, o, a(m, "gn")) as u32),
        "SI_cameraSaveHistory" => ret(h::camera_save_history(m, o, a(m, "gn")) as u32),
        "SI_lightSaveHistory" => ret(h::light_save_history(m, o, a(m, "gn")) as u32),
        "SI_audioSaveHistory" => ret(h::audio_save_history(m, o, a(m, "track"), a(m, "a"), a(m, "b"), a(m, "c") as i32) as u32),
        "SI_actionsSaveHistory" => ret(h::actions_save_history(m, o, a(m, "hier"), a(m, "a") as i32, a(m, "b") as i32) as u32),
        "SI_mstractSaveHistory" => ret(h::mstract_save_history(m, o, a(m, "hier"), a(m, "a") as i32, a(m, "b") as i32) as u32),
        "SI_modelSaveHistoryTransform" => ret(h::model_save_history_transform(m, o, a(m, "gn"), a(m, "tree") as i16, a(m, "mode") as i16, a(m, "target")) as u32),
        "modelHistXformSnapshot" => {
            h::model_hist_xform_snapshot(m, o, a(m, "model"), a(m, "rec"));
            done
        }
        "SI_modelSaveHistoryGeometry" => ret(h::model_save_history_geometry(m, o, a(m, "gn"), a(m, "target") as i32) as u32),
        "SI_fcurveSaveHistory" => ret(h::fcurve_save_history(m, o, a(m, "elem")) as u32),
        "SI_sprdSaveHistory" => ret(h::sprd_save_history(m, o, a(m, "ident"), a(m, "idx")) as u32),
        "sprdSwapHistory" => {
            h::sprd_swap_history(m, o, a(m, "rec"));
            done
        }
        "SI_historyUndoRedo" => ret(h::history_undo_redo(m, o, a(m, "redo") as i16) as u32),
        "SI_modelSaveHistoryParenting" => ret(h::model_save_history_parenting(m, o, a(m, "child"), a(m, "parent"), a(m, "unlink") as i16) as u32),
        other => return Err(format!("no port for `{other}`")),
    })
}

#[derive(Default)]
struct Count {
    cases: usize,
    same: usize,
    differ: usize,
    not_compared: usize,
}

enum Verdict {
    Same,
    Differ(String),
    NotCompared,
}

/// The result of a routine that returns an `int` or a `short`: the width of the original's
/// `eax` that the vector holds.
fn check(c: &Case, mutate: &str) -> Verdict {
    if c.note("UNINIT").is_some() {
        return Verdict::NotCompared;
    }
    let mut m = Mem::new();
    si_app::image::init_data(&mut m);
    h::init_data(&mut m);
    let names = match place(&mut m, c) {
        Ok(n) => n,
        Err(e) => return Verdict::Differ(format!("the case cannot be placed: {e}")),
    };
    let mut o = Replay { calls: &c.calls, k: 0, names: names.clone(), diff: None, printed: [Vec::new(), Vec::new()], mutate_calls: mutate == "calls" };
    let outcome = match run(&mut m, &mut o, c, &names) {
        Ok(x) => x,
        Err(e) => return Verdict::Differ(e),
    };
    if let Some(stop) = c.note("STOP:") {
        return if m.fault().is_some() { Verdict::Same } else { Verdict::Differ(format!("the original stopped ({stop}), the port did not")) };
    }
    let Outcome::Returned(mut v) = outcome;
    if mutate == "1" {
        match v {
            Some(x) => v = Some(x ^ 1),
            None => return Verdict::Differ("mutated: nothing returned".into()),
        }
        for (name, words) in &c.outputs {
            if words.as_ref().is_some_and(|w| !w.is_empty()) {
                if let Some(addr) = names.addr(&m, name).filter(|a| *a != 0) {
                    let b = m.u8(addr);
                    m.set_u8(addr, b ^ 1);
                }
            }
        }
        for p in o.printed.iter_mut() {
            if let Some(b) = p.first_mut() {
                *b ^= 1;
            }
        }
    }
    if let Some(d) = o.diff.take() {
        return Verdict::Differ(d);
    }
    if o.k < c.calls.len() {
        return Verdict::Differ(format!("the original made {} calls, the port {}", c.calls.len(), o.k));
    }
    for key in ["eax", "ptr"] {
        if let Some(Some(w)) = c.output(key) {
            let want = w.first().and_then(|w| word_value(&m, &names, w));
            let mask = match w.first() {
                Some(w) if w.bytes() < 4 => (1u32 << (8 * w.bytes())) - 1,
                _ => u32::MAX,
            };
            if want.is_some() && want.map(|x| x & mask) != v.map(|x| x & mask) {
                return Verdict::Differ(format!("{key}: original {want:x?}, port {v:x?}"));
            }
        }
    }
    let n_heap = c.outputs.iter().filter(|(n, _)| n.starts_with("heap") && n[4..].bytes().all(|b| b.is_ascii_digit())).count();
    if n_heap != m.heap_blocks().len() {
        return Verdict::Differ(format!("heap blocks: original {n_heap}, port {}", m.heap_blocks().len()));
    }
    for (name, words) in &c.outputs {
        match name.as_str() {
            "eax" | "ptr" | "stderr" | "stdout" | "freed" => continue,
            _ => {}
        }
        let Some(words) = words else { continue };
        let Some(addr) = names.addr(&m, name) else {
            return Verdict::Differ(format!("output `{name}`: no such buffer here"));
        };
        if let Err(e) = m.compare_words(addr, words, &names) {
            return Verdict::Differ(format!("{name}: {e}"));
        }
    }
    let freed: Vec<Option<u32>> = c.output("freed").and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| word_value(&m, &names, w)).collect()).unwrap_or_default();
    let port_freed: Vec<Option<u32>> = m.freed().iter().map(|p| Some(*p)).collect();
    if freed != port_freed {
        return Verdict::Differ(format!("freed: original {freed:x?}, port {port_freed:x?}"));
    }
    for (key, k) in [("stdout", 0), ("stderr", 1)] {
        let want: Vec<u8> = c.output(key).and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| if let Word::Value { value, .. } = w { *value as u8 } else { 0 }).collect()).unwrap_or_default();
        if want != o.printed[k] {
            return Verdict::Differ(format!("{key}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(&o.printed[k])));
        }
    }
    if m.fault().is_some() {
        return Verdict::Differ(format!("the port faulted: {:?}", m.fault()));
    }
    Verdict::Same
}

fn main() {
    // the original's messages come from the user's install (unit rta)
    si_rt::check_or_exit("app-histcheck");
    let dir = std::env::args().nth(1).expect("usage: app-histcheck <vector directory> [routine]");
    let only = std::env::args().nth(2);
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("vector directory").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vec")).collect();
    files.sort();
    let mut table: BTreeMap<String, Count> = BTreeMap::new();
    let mut shown = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).expect("vector file");
        for line in text.lines() {
            let c = match parse(line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}: {e}", f.display()),
            };
            if only.as_deref().is_some_and(|o| o != c.name) {
                continue;
            }
            let t = table.entry(c.name.clone()).or_default();
            t.cases += 1;
            match check(&c, &mutate) {
                Verdict::Same => t.same += 1,
                Verdict::NotCompared => t.not_compared += 1,
                Verdict::Differ(why) => {
                    t.differ += 1;
                    if shown < 40 && mutate.is_empty() {
                        shown += 1;
                        eprintln!("{}: {why}", c.name);
                    }
                }
            }
        }
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9}", "routine", "cases", "identical", "different", "not comp.");
    let mut total = Count::default();
    for (name, t) in &table {
        println!("{:<34} {:>7} {:>9} {:>9} {:>9}", name, t.cases, t.same, t.differ, t.not_compared);
        total.cases += t.cases;
        total.same += t.same;
        total.differ += t.differ;
        total.not_compared += t.not_compared;
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9}", "total", total.cases, total.same, total.differ, total.not_compared);
    if !mutate.is_empty() {
        println!("mutation {mutate}: {} of {} compared cases differ", total.differ, total.cases - total.not_compared);
    }
}
