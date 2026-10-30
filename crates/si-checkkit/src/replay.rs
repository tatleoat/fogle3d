// SPDX-License-Identifier: MIT
//! The replay mode: the checker `app-histcheck` and `app-imagecheck` each carried (about 300
//! lines of parser, replaying outside world, `.out` stores and report), as one reusable piece.
//!
//! It differs from [`crate::Kit`] on purpose: one run per case (no complement pass), the
//! routines are dispatched by one `port` function (a `match` on the case's name, with the
//! image's own `.data` values placed by `init`), the case's outputs decide what is compared
//! (a buffer without an output is not compared), a case may end by `exit` (a result of
//! [`ReplayOutcome::Exited`]), and the report has one row per routine over all files. The
//! vector text is the same; its parser here keeps quoted texts in a field in one token.
//!
//! The outside world replays the original's calls in order: name and argument count held
//! against the port's, every argument the vector knows held (`57ac57ac`, a local of the
//! original's, skipped), the recorded answer handed back. What a callee stored through the
//! arguments that were locals follows in the vector as `NAME.out(words)` entries, one per
//! local, in argument order; `out_args` names the argument indices when the table does not
//! mark them `57ac57ac`. A routine-specific side effect (a block `getcwd` filled, a `time`
//! store) is a [`ReplayEffect`].
//!
//! Nothing here reproduces a function of the original.

use si_mem::vecfmt::{load_case_at, parse_words, FieldAt, Names, Word};
use si_mem::{Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;
use std::path::Path;

/// What the generator writes for an argument that is the address of a local.
const LOCAL: u32 = 0x57ac_57ac;

/// One recorded call: `@name(args)=answer`.
#[derive(Clone, Debug)]
pub struct RecCall {
    pub name: String,
    pub args: Vec<Word>,
    pub ret: Word,
}

/// A case of a vector file (replay form).
#[derive(Clone, Debug, Default)]
pub struct ReplayCase {
    pub name: String,
    pub inputs: Vec<(String, Option<Vec<Word>>)>,
    pub outputs: Vec<(String, Option<Vec<Word>>)>,
    pub calls: Vec<RecCall>,
    pub notes: Vec<String>,
}

impl ReplayCase {
    pub fn input(&self, name: &str) -> Option<&[Word]> {
        self.inputs.iter().find(|(n, _)| n == name).and_then(|(_, w)| w.as_deref())
    }

    pub fn output(&self, name: &str) -> Option<&Option<Vec<Word>>> {
        self.outputs.iter().find(|(n, _)| n == name).map(|(_, w)| w)
    }

    /// The first note that starts with `prefix`.
    pub fn note(&self, prefix: &str) -> Option<&str> {
        self.notes.iter().find(|n| n.starts_with(prefix)).map(|s| s.as_str())
    }

    /// An argument as its 32-bit word: `p_name` or `v_name` (0 when the case has neither).
    pub fn arg(&self, m: &Mem, names: &Names, name: &str) -> u32 {
        for prefix in ["p_", "v_"] {
            if let Some(w) = self.input(&format!("{prefix}{name}")).and_then(|w| w.first()) {
                return word_value(m, names, w).unwrap_or(0);
            }
        }
        0
    }

    /// The bytes of an input field up to its first zero (a text the port reads, or a recorded
    /// answer the effect hands out).
    pub fn text(&self, name: &str) -> Vec<u8> {
        self.input(name).map(|w| w.iter().filter_map(|w| if let Word::Value { value, .. } = w { Some(*value as u8) } else { None }).take_while(|&b| b != 0).collect()).unwrap_or_default()
    }

    /// Parses a vector line (`Ok(None)` for a blank or comment line).
    pub fn parse(line: &str) -> Result<Option<ReplayCase>, String> {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            return Ok(None);
        }
        let mut c = ReplayCase::default();
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
                    // an `.out` entry has no answer: `@name.out(words)`
                    let (args, ret) = match call.rfind(")=") {
                        Some(close) => (&call[open + 1..close], parse_words(&call[close + 2..])?.and_then(|w| w.into_iter().next())),
                        None => (&call[open + 1..call.rfind(')').ok_or("call without `)`")?], None),
                    };
                    c.calls.push(RecCall {
                        name: call[1..open].to_string(),
                        args: if args.is_empty() { Vec::new() } else { parse_words(args)?.unwrap_or_default() },
                        ret: ret.unwrap_or(Word::Value { bytes: 4, value: 0 }),
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

/// The value of a word here (`None`: the original did not write it).
pub fn word_value(m: &Mem, names: &Names, w: &Word) -> Option<u32> {
    match w {
        Word::Value { value, .. } => Some(*value as u32),
        Word::Unwritten { .. } => None,
        Word::Ref { name, offset } => names.addr(m, name).map(|a| a.wrapping_add(*offset)),
    }
}

/// How the routine ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayOutcome {
    /// It returned, with `eax` if it has a result.
    Returned(Option<u32>),
    /// The program ended with this status.
    Exited(i32),
}

/// A routine-specific effect of a recorded callee: the image, the case, the callee's name,
/// the port's arguments, the recorded answer and a place to report a difference. `Some(answer)`
/// replaces the recorded answer.
pub type ReplayEffect = fn(&mut Mem, &ReplayCase, &str, &[u32], Option<u32>, &mut Option<String>) -> Option<u32>;

/// The outside world that replays the original's calls.
pub struct ReplayOutside<'c> {
    pub case: &'c ReplayCase,
    pub k: usize,
    pub names: Names,
    pub diff: Option<String>,
    pub printed: [Vec<u8>; 2],
    mutate_calls: bool,
    out_args: fn(&str) -> &'static [usize],
    effect: Option<ReplayEffect>,
}

impl ReplayOutside<'_> {
    fn differ(&mut self, text: String) {
        if self.diff.is_none() {
            self.diff = Some(text);
        }
    }
}

impl Outside for ReplayOutside<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let mut args = args.to_vec();
        let k = self.k;
        self.k += 1;
        let Some(rec) = self.case.calls.get(k) else {
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
        let mut ret = word_value(m, &self.names, &rec.ret);
        // what the callee stored through the arguments that were locals follows as `NAME.out(...)`
        let listed = (self.out_args)(r.name);
        let indices: Vec<usize> = if listed.is_empty() { rec.args.iter().enumerate().filter(|(_, w)| word_value(m, &self.names, w) == Some(LOCAL)).map(|(i, _)| i).collect() } else { listed.to_vec() };
        for i in indices {
            let Some(next) = self.case.calls.get(self.k) else { break };
            if next.name.strip_suffix(".out") != Some(r.name) {
                break;
            }
            self.k += 1;
            let Some(&base) = args.get(i) else { break };
            let mut p = base;
            for w in &next.args {
                match w {
                    Word::Value { bytes: 2, value } => {
                        m.set_u16(p, *value as u16);
                        p += 2;
                    }
                    Word::Value { bytes: 1, value } => {
                        m.set_u8(p, *value as u8);
                        p += 1;
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
        if let Some(effect) = self.effect {
            if let Some(v) = effect(m, self.case, r.name, &args, ret, &mut self.diff) {
                ret = Some(v);
            }
        }
        Ret::eax(ret.unwrap_or(0))
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.printed[(stream == 2) as usize].extend_from_slice(text);
    }
}

/// How one case came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayVerdict {
    Same,
    Differ(String),
    /// The original stopped (not by `exit`) and the port faulted too.
    StoppedBoth,
    NotCompared,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct ReplayCount {
    pub cases: usize,
    pub same: usize,
    pub differ: usize,
    pub stopped_both: usize,
    pub not_compared: usize,
    pub undeclared: usize,
}

/// The port of a replay check: runs the routine the case names.
pub type ReplayPort = fn(&mut Mem, &mut ReplayOutside, &ReplayCase, &Names) -> Result<ReplayOutcome, String>;

/// A replay check: `init` puts the image's own values in a fresh image (and may use the case),
/// `port` dispatches on the case's name.
pub struct ReplayKit {
    init: fn(&mut Mem, &ReplayCase),
    port: ReplayPort,
    out_args: fn(&str) -> &'static [usize],
    effect: Option<ReplayEffect>,
    fault_exit: Option<i32>,
}

impl ReplayKit {
    pub fn new(init: fn(&mut Mem, &ReplayCase), port: ReplayPort) -> ReplayKit {
        ReplayKit { init, port, out_args: |_| &[], effect: None, fault_exit: None }
    }

    /// The argument indices a callee stores its answers through, when the vector does not
    /// mark them `57ac57ac`.
    pub fn out_args(mut self, f: fn(&str) -> &'static [usize]) -> ReplayKit {
        self.out_args = f;
        self
    }

    /// A routine-specific effect of recorded callees.
    pub fn effect(mut self, f: ReplayEffect) -> ReplayKit {
        self.effect = Some(f);
        self
    }

    /// The exit status with which the port reports that it faulted: a case the original
    /// stopped on is then `StoppedBoth` when the port ends so.
    pub fn fault_exit(mut self, status: i32) -> ReplayKit {
        self.fault_exit = Some(status);
        self
    }

    /// Reads `<directory> [routine]` and `EMUCHECK_MUTATE`; prints the report; exits.
    pub fn run(self) -> ! {
        let dir = std::env::args().nth(1).unwrap_or_else(|| {
            eprintln!("usage: <check> <vector directory> [routine]\n  EMUCHECK_MUTATE=1 flips a bit of everything the port produced; =calls of the first compared argument of every call");
            std::process::exit(2)
        });
        let only = std::env::args().nth(2);
        let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
        let (text, ok) = self.report(Path::new(&dir), only.as_deref(), &mutate, true);
        print!("{text}");
        std::process::exit(if ok || !mutate.is_empty() { 0 } else { 1 })
    }

    /// The report for the `.vec` files in `dir`; `true` when no case differs.
    pub fn report(&self, dir: &Path, only: Option<&str>, mutate: &str, show: bool) -> (String, bool) {
        let mut files: Vec<_> = std::fs::read_dir(dir).expect("vector directory").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vec")).collect();
        files.sort();
        let mut table: BTreeMap<String, ReplayCount> = BTreeMap::new();
        let mut shown = 0;
        for f in files {
            let text = std::fs::read_to_string(&f).expect("vector file");
            for line in text.lines() {
                let c = match ReplayCase::parse(line) {
                    Ok(Some(c)) => c,
                    Ok(None) => continue,
                    Err(e) => panic!("{}: {e}", f.display()),
                };
                if only.is_some_and(|o| o != c.name) {
                    continue;
                }
                let t = table.entry(c.name.clone()).or_default();
                t.cases += 1;
                if c.note("GW:").is_some() {
                    t.undeclared += 1;
                }
                match self.check(&c, mutate) {
                    ReplayVerdict::Same => t.same += 1,
                    ReplayVerdict::StoppedBoth => t.stopped_both += 1,
                    ReplayVerdict::NotCompared => t.not_compared += 1,
                    ReplayVerdict::Differ(why) => {
                        t.differ += 1;
                        if show && shown < 40 && mutate.is_empty() {
                            shown += 1;
                            eprintln!("{}: {why}", c.name);
                        }
                    }
                }
            }
        }
        let mut out = format!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}\n", "routine", "cases", "identical", "different", "stopped", "not comp.");
        let mut total = ReplayCount::default();
        for (name, t) in &table {
            let extra = if t.undeclared > 0 { format!("   {} with undeclared global writes", t.undeclared) } else { String::new() };
            out += &format!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}{extra}\n", name, t.cases, t.same, t.differ, t.stopped_both, t.not_compared);
            total.cases += t.cases;
            total.same += t.same;
            total.differ += t.differ;
            total.stopped_both += t.stopped_both;
            total.not_compared += t.not_compared;
        }
        out += &format!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}\n", "total", total.cases, total.same, total.differ, total.stopped_both, total.not_compared);
        if !mutate.is_empty() {
            out += &format!("mutation {mutate}: {} of {} compared cases differ\n", total.differ, total.cases - total.not_compared - total.stopped_both);
        }
        (out, total.differ == 0)
    }

    /// Settles one case. `mutate` is `""`, `"1"` or `"calls"`.
    pub fn check(&self, c: &ReplayCase, mutate: &str) -> ReplayVerdict {
        use ReplayVerdict::*;
        if c.note("UNINIT").is_some() {
            return NotCompared;
        }
        let mut m = Mem::new();
        (self.init)(&mut m, c);
        let fields: Vec<FieldAt<'_>> = c.inputs.iter().filter(|(n, _)| is_buffer(n)).map(|(n, w)| (n.as_str(), global_at(n), w.as_deref())).collect();
        let names = match load_case_at(&mut m, &fields) {
            Ok(n) => n,
            Err(e) => return Differ(format!("the case cannot be placed: {e}")),
        };
        let mut o = ReplayOutside { case: c, k: 0, names: names.clone(), diff: None, printed: [Vec::new(), Vec::new()], mutate_calls: mutate == "calls", out_args: self.out_args, effect: self.effect };
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.port)(&mut m, &mut o, c, &names)));
        let mut outcome = match ran {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => return Differ(e),
            Err(p) => {
                // a fault the image turned into a stop, or a panic of the port
                if p.downcast_ref::<si_mem::Stop>().is_some() {
                    ReplayOutcome::Exited(self.fault_exit.unwrap_or(-1))
                } else {
                    let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    return Differ(format!("the port panicked: {text}"));
                }
            }
        };
        // where the original stopped other than through exit, the port must have faulted
        if let Some(stop) = c.note("STOP:") {
            if !stop.starts_with("STOP:exit:") {
                let exited_fault = matches!((outcome, self.fault_exit), (ReplayOutcome::Exited(x), Some(f)) if x == f);
                return if m.fault().is_some() || exited_fault { StoppedBoth } else { Differ(format!("the original stopped ({stop}), the port did not")) };
            }
        }
        if mutate == "1" {
            // a bit of everything the port produced
            outcome = match outcome {
                ReplayOutcome::Returned(Some(v)) => ReplayOutcome::Returned(Some(v ^ 1)),
                // a routine that returned with nothing else to compare: the way it ended
                ReplayOutcome::Returned(None) => ReplayOutcome::Exited(1),
                ReplayOutcome::Exited(x) => ReplayOutcome::Exited(x ^ 1),
            };
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
            return Differ(d);
        }
        if o.k < c.calls.len() {
            return Differ(format!("the original made {} calls, the port {}", c.calls.len(), o.k));
        }
        // the result, or how the program ended
        match (c.note("STOP:exit:"), &outcome) {
            (Some(stop), ReplayOutcome::Exited(x)) => {
                if stop != format!("STOP:exit:{x}") {
                    return Differ(format!("original {stop}, port exit {x}"));
                }
            }
            (Some(stop), ReplayOutcome::Returned(_)) => return Differ(format!("original {stop}, the port returned")),
            (None, ReplayOutcome::Exited(x)) => return Differ(format!("the original returned, the port exits with {x}")),
            (None, ReplayOutcome::Returned(v)) => {
                for key in ["eax", "ptr"] {
                    if let Some(Some(w)) = c.output(key) {
                        let want = w.first().and_then(|w| word_value(&m, &names, w));
                        let mask = match w.first() {
                            Some(w) if w.bytes() < 4 => (1u32 << (8 * w.bytes())) - 1,
                            _ => u32::MAX,
                        };
                        if want.is_some() && want.map(|x| x & mask) != v.map(|x| x & mask) {
                            return Differ(format!("{key}: original {want:x?}, port {v:x?}"));
                        }
                    }
                }
            }
        }
        // memory: the case's buffers and the heap
        let n_heap = c.outputs.iter().filter(|(n, _)| n.starts_with("heap") && n[4..].bytes().all(|b| b.is_ascii_digit())).count();
        if n_heap != m.heap_blocks().len() {
            return Differ(format!("heap blocks: original {n_heap}, port {}", m.heap_blocks().len()));
        }
        for (name, words) in &c.outputs {
            if matches!(name.as_str(), "eax" | "ptr" | "stderr" | "stdout" | "freed") {
                continue;
            }
            let Some(words) = words else { continue };
            let Some(addr) = names.addr(&m, name) else {
                return Differ(format!("output `{name}`: no such buffer here"));
            };
            if let Err(e) = m.compare_words(addr, words, &names) {
                return Differ(format!("{name}: {e}"));
            }
        }
        let freed: Vec<Option<u32>> = c.output("freed").and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| word_value(&m, &names, w)).collect()).unwrap_or_default();
        let port_freed: Vec<Option<u32>> = m.freed().iter().map(|p| Some(*p)).collect();
        if freed != port_freed {
            return Differ(format!("freed: original {freed:x?}, port {port_freed:x?}"));
        }
        for (key, k) in [("stdout", 0), ("stderr", 1)] {
            let want: Vec<u8> = c.output(key).and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| if let Word::Value { value, .. } = w { *value as u8 } else { 0 }).collect()).unwrap_or_default();
            if want != o.printed[k] {
                return Differ(format!("{key}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(&o.printed[k])));
            }
        }
        if m.fault().is_some() {
            return Differ(format!("the port faulted: {:?}", m.fault()));
        }
        Same
    }
}
