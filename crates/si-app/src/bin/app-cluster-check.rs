// SPDX-License-Identifier: MIT
//! `app-cluster-check <dir>`: `si_app::image::clusterread` (finding 0019, section 13d)
//! against the vectors `re/tools/si-app/gen_cluster.py` writes: `clusterKeyAsciiRead` and
//! `clusterAnimNodeAsciiRead`, one routine of the original per case, stepped in the
//! emulation harness with every callee a recorded stand-in.
//!
//! Per case: a fresh image with the case's buffers; the port runs against an outside world
//! that replays the case's `script` (what `fscanf` reads, one entry per item) and `ans`
//! (which of the case's buffers the allocators and `SI_klusterGetByName` answer with), and
//! checks every call it makes against the original's recorded one (name, arguments, answer;
//! strings by CRC-32). Compared: the result, every buffer, the calls (count and order).
//! Nothing here reproduces a function of the original.
//!
//! `EMUCHECK_MUTATE=1` flips a bit of everything the port produced; `=calls` flips an
//! argument of the first call; every compared case must then differ.

use si_app::image::clusterread as cr;
use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::vectors::{self, Case, Field, Word};
use std::collections::{BTreeMap, HashMap};

const WORDS: [&[u8]; 16] = [
    b"name", b"numCVs", b"numKeys", b"indexes", b"Cluster_key", b"End", b"Zzz", b"", b"name", b"numCVs", b"numKeys", b"indexes", b"Cluster_key", b"name",
    b"Cluster_key", b"End",
];
const KEYALLOC: [&str; 3] = ["blk0", "blk1", "blk2"];

fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn quiet(v: u32) -> u32 {
    if v & 0x7f80_0000 == 0x7f80_0000 && v & 0x7f_ffff != 0 {
        v | 0x40_0000
    } else {
        v
    }
}

struct World<'c> {
    c: &'c Case,
    names: HashMap<String, u32>,
    script: Vec<u32>,
    ans: Vec<u32>,
    used: usize,
    kl: usize,
    ka: usize,
    next_call: usize,
    diff: Option<String>,
    flip: u32,
}

impl World<'_> {
    fn differ(&mut self, t: String) {
        if self.diff.is_none() {
            self.diff = Some(t);
        }
    }

    fn at(&self, n: &str) -> u32 {
        self.names.get(n).copied().unwrap_or(0)
    }

    /// Holds the port's call against the original's record: the name, then each argument
    /// (a number, or a pointer into one of the case's buffers), then the answer.
    fn record(&mut self, name: &str, args: &[Arg], answer: Arg) {
        let k = self.next_call;
        self.next_call += 1;
        let Some(rec) = self.c.calls.get(k) else {
            self.differ(format!("call {k}: {name} after the original's last call"));
            return;
        };
        if rec.name != name {
            self.differ(format!("call {k}: original {}, port {name}", rec.name));
            return;
        }
        let first = (k == 0) as u32 * self.flip;
        let mut all: Vec<(&Arg, &Word)> = args.iter().zip(&rec.args).collect();
        all.push((&answer, &rec.result));
        if args.len() != rec.args.len() {
            self.differ(format!("call {k} {name}: {} arguments, original {}", args.len(), rec.args.len()));
            return;
        }
        for (i, (got, want)) in all.into_iter().enumerate() {
            let ok = match (got, want) {
                (Arg::N(v) | Arg::Ptr(v), Word::Value { value, .. }) => (*v ^ first) as u128 == *value & 0xffff_ffff,
                (Arg::Ptr(p), Word::Ref { name, offset }) => *p == self.at(name).wrapping_add(*offset),
                _ => false,
            };
            if !ok {
                self.differ(format!("call {k} {name} word {i}: port {got:?}, original {want:?}"));
            }
        }
    }

    fn pick(&self, n: &str) -> u32 {
        self.at(n)
    }
}

#[derive(Debug)]
enum Arg {
    N(u32),
    Ptr(u32),
}

impl Outside for World<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
        let r_name = r.name;
        match r_name {
            "fscanf" => {
                let fmt = a[1];
                let n = match fmt {
                    0x086c_bd35 => 4,
                    0x086c_bd2c | 0x086c_bd41 | 0x086c_bd4a | 0x086c_bd53 => 3,
                    _ => 1,
                };
                for d in &a[2..2 + n] {
                    let k = self.used;
                    self.used += 1;
                    match self.script.get(k).copied() {
                        None => match fmt {
                            0x086c_bd5c | 0x086c_bd64 => m.put_cstr(*d, b"End"),
                            _ => m.set_u32(*d, 0),
                        },
                        Some(v) => match fmt {
                            0x086c_bd5c | 0x086c_bd64 => m.put_cstr(*d, WORDS[(v & 15) as usize]),
                            0x086c_bd6e | 0x086c_bd79 | 0x086c_bd84 => m.set_u32(*d, (((v >> 4) & 15) as i32 - 4) as u32),
                            _ => m.set_u32(*d, quiet(v)),
                        },
                    }
                }
                self.record("fscanf", &[Arg::N(a[0]), Arg::N(fmt)], Arg::N(n as u32));
                Ret::eax(n as u32)
            }
            "SI_clusterAllocate" => {
                let r = if self.ans[0] != 0 { self.pick("clist") } else { 0 };
                self.record(r_name, &[], if r != 0 { Arg::Ptr(r) } else { Arg::N(0) });
                Ret::eax(r)
            }
            "SI_clusterDataAllocate" => {
                let r = if self.ans[1] != 0 { self.pick("dat") } else { 0 };
                self.record(r_name, &[], if r != 0 { Arg::Ptr(r) } else { Arg::N(0) });
                Ret::eax(r)
            }
            "SI_klusterGetByName" => {
                let k = self.kl;
                self.kl += 1;
                let p = self.ans.get(2 + k).copied().filter(|_| 2 + k < 6).unwrap_or(0);
                let r = if p != 0 { self.pick(["k0", "k1"][(p - 1) as usize]) } else { 0 };
                let crc = crc32(&m.cstr(a[1]));
                self.record(r_name, &[Arg::Ptr(a[0]), Arg::N(crc)], if r != 0 { Arg::Ptr(r) } else { Arg::N(0) });
                Ret::eax(r)
            }
            "SI_clusterKeyAllocate" => {
                let k = self.ka;
                self.ka += 1;
                let p = self.ans.get(6 + k).copied().filter(|_| 6 + k < 10).unwrap_or(0);
                let r = if p != 0 { self.pick(KEYALLOC[(p - 1) as usize]) } else { 0 };
                self.record(r_name, &[Arg::N(a[0]), Arg::N(a[1])], if r != 0 { Arg::Ptr(r) } else { Arg::N(0) });
                Ret::eax(r)
            }
            "SI_clusterDataDispose" => {
                self.record(r_name, &[Arg::Ptr(a[0])], Arg::N(0));
                Ret::NONE
            }
            "SI_statusDisplayMessage" => {
                let crc = crc32(&m.cstr(a[0]));
                self.record(r_name, &[Arg::N(crc), Arg::N(a[1])], Arg::N(0));
                Ret::NONE
            }
            other => {
                self.differ(format!("the port calls {other}, which the check does not know"));
                Ret::NONE
            }
        }
    }
}

fn is_buffer(n: &str) -> bool {
    !(matches!(n, "stream" | "count" | "kind") || n == "eax")
}

fn words_len(f: &Field) -> u32 {
    f.words().iter().map(|w| w.bytes() as u32).sum()
}

fn store(m: &mut Mem, names: &HashMap<String, u32>, mut at: u32, f: &Field) {
    for w in f.words() {
        match w {
            Word::Value { bytes, value } => {
                for i in 0..*bytes as u32 {
                    m.set_u8(at + i, (*value >> (8 * i)) as u8);
                }
            }
            Word::Ref { name, offset } => m.set_u32(at, names.get(name).copied().unwrap_or(0).wrapping_add(*offset)),
            Word::Unwritten { .. } => {}
        }
        at += w.bytes() as u32;
    }
}

fn compare(m: &Mem, names: &HashMap<String, u32>, mut at: u32, f: &Field, what: &str) -> Result<(), String> {
    let base = at;
    for w in f.words() {
        match w {
            Word::Value { bytes, value } => {
                let got = (0..*bytes as u32).fold(0u128, |a, i| a | ((m.u8(at + i) as u128) << (8 * i)));
                if got != *value {
                    return Err(format!("{what} at +{:#x}: original {value:#x}, port {got:#x}", at - base));
                }
            }
            Word::Ref { name, offset } => {
                let want = names.get(name).copied().unwrap_or(0).wrapping_add(*offset);
                if m.u32(at) != want {
                    return Err(format!("{what} at +{:#x}: pointer to {name}+{offset}, port {:#x}", at - base, m.u32(at)));
                }
            }
            Word::Unwritten { .. } => {}
        }
        at += w.bytes() as u32;
    }
    Ok(())
}

enum Verdict {
    Same,
    Differ(String),
    Skipped,
}

fn check(c: &Case, mutate: &str) -> Verdict {
    if !c.authoritative() || c.has_note("UNINIT") || c.has_note("OVERRUN") || c.stopped() {
        return Verdict::Skipped;
    }
    let mut m = Mem::new();
    let mut names: HashMap<String, u32> = HashMap::new();
    let mut sized: BTreeMap<String, u32> = BTreeMap::new();
    for f in c.inputs.iter().chain(&c.outputs) {
        if is_buffer(&f.name) && !f.is_null() && f.name != "eax" {
            let e = sized.entry(f.name.clone()).or_default();
            *e = (*e).max(words_len(f));
        }
    }
    for (n, len) in &sized {
        let at = m.place((*len).max(4), 8);
        names.insert(n.clone(), at);
    }
    for f in &c.inputs {
        if let Some(at) = names.get(&f.name).copied() {
            store(&mut m, &names, at, f);
        }
    }
    let u = |n: &str| c.try_input(n).map(|f| f.u32()).unwrap_or(0);
    let words = |n: &str| c.try_input(n).map(|f| f.values().iter().map(|v| *v as u32).collect::<Vec<u32>>()).unwrap_or_default();
    let mut o = World {
        c,
        names: names.clone(),
        script: words("script"),
        ans: words("ans"),
        used: 0,
        kl: 0,
        ka: 0,
        next_call: 0,
        diff: None,
        flip: (mutate == "calls") as u32,
    };
    if o.ans.len() < 10 {
        o.ans.resize(10, 0);
    }
    let p = |n: &str| names.get(n).copied().unwrap_or(0);
    let mut eax = match c.name.as_str() {
        "clusterKeyAsciiRead" => cr::cluster_key_ascii_read(&mut m, &mut o, u("stream"), p("data"), u("count") as i32, u("kind") as i32) as u32,
        "clusterAnimNodeAsciiRead" => cr::cluster_anim_node_ascii_read(&mut m, &mut o, u("stream"), p("model")) as u32,
        other => return Verdict::Differ(format!("no port for `{other}`")),
    };
    if let Some(d) = o.diff.take() {
        return Verdict::Differ(d);
    }
    if o.next_call != c.calls.len() {
        return Verdict::Differ(format!("calls: original {}, port {}", c.calls.len(), o.next_call));
    }
    if mutate == "1" {
        eax ^= 1;
        for (n, at) in &names {
            if c.try_output(n).is_some_and(|f| !f.words().is_empty()) {
                let b = m.u8(*at);
                m.set_u8(*at, b ^ 1);
            }
        }
    }
    if let Some(Some(w)) = c.try_output("eax").map(|f| f.words().first().and_then(|w| w.value())) {
        if w as u32 != eax {
            return Verdict::Differ(format!("eax: original {w:#x}, port {eax:#x}"));
        }
    }
    for f in &c.outputs {
        let n = f.name.as_str();
        if n == "eax" || f.is_null() {
            continue;
        }
        match names.get(n).copied() {
            Some(at) => {
                if let Err(e) = compare(&m, &names, at, f, n) {
                    return Verdict::Differ(e);
                }
            }
            None => return Verdict::Differ(format!("output `{n}`: no such buffer")),
        }
    }
    if m.fault().is_some() {
        return Verdict::Differ(format!("the port faulted: {:?}", m.fault()));
    }
    Verdict::Same
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: app-cluster-check <vector directory>");
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let all = vectors::read_dir(std::path::Path::new(&dir)).expect("vector directory");
    println!("{:<30} {:>7} {:>9} {:>9} {:>9}", "routine", "cases", "identical", "different", "not comp.");
    let (mut tc, mut ts, mut td, mut tn) = (0, 0, 0, 0);
    let mut shown = 0;
    for (name, cases) in all {
        let (mut same, mut diff, mut skip) = (0, 0, 0);
        for c in &cases {
            match check(c, &mutate) {
                Verdict::Same => same += 1,
                Verdict::Skipped => skip += 1,
                Verdict::Differ(why) => {
                    diff += 1;
                    if shown < 30 && mutate.is_empty() {
                        shown += 1;
                        eprintln!("{name}: {why}");
                    }
                }
            }
        }
        println!("{:<30} {:>7} {:>9} {:>9} {:>9}", name, cases.len(), same, diff, skip);
        tc += cases.len();
        ts += same;
        td += diff;
        tn += skip;
    }
    println!("{:<30} {:>7} {:>9} {:>9} {:>9}", "total", tc, ts, td, tn);
    if !mutate.is_empty() {
        println!("mutation {mutate}: {td} of {} compared cases differ", tc - tn);
    }
}
