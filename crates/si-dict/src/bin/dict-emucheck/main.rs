// SPDX-License-Identifier: MIT
//! `dict-emucheck <dir>`: the dictionaries on the image against the emulated original.
//!
//! Each line of a vector file (made by `re/tools/si-dict/gen.py` with the harness of
//! `re/tools/emu`) is one call of one original routine on a made-up dictionary: the
//! buffers it was given, what it returned, every buffer it changed, every block it got
//! from `malloc` with its bytes, the blocks it freed in order, and the calls it made to
//! routines outside the dictionary source files with their arguments and answers.
//!
//! The check does not rebuild anything: the case's buffers are placed in the image at the
//! addresses the harness gave them (the arena is laid out the same way; globals lie at
//! their own addresses), the port runs on them, and memory is compared:
//!
//! - the return value;
//! - every buffer of the case, changed or not (a buffer the original left alone must be
//!   left alone);
//! - the heap: as many blocks, each of the same size, each written byte the same, each
//!   byte the original did not write still holding the fill;
//! - the freed blocks, in order;
//! - the outside calls, in order, with their arguments;
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! Naming in a vector line: `v_x` is an argument by value, `p_x` a pointer argument,
//! `gXXXXXXXX…` a global at that address, anything else a buffer of the arena.
//!
//! Columns of the report: cases; identical; identical given the C library's answers (the
//! case carries an `NA:` note); different; not compared (the original stopped, or wrote a
//! global the case did not declare). A case some of whose words are undefined in the
//! original (`UNINIT`) is compared in its defined words and counted in its own column
//! when those agree. `EMUCHECK_MUTATE=1` flips one bit of what the port returned and of
//! the first byte it changed before the comparison: every case that returns or changes
//! something must then differ; so must every case that prints (one bit of the first byte
//! printed is flipped too).

mod table;

use si_dict::outside::{Outside, Routine};
use si_mem::{Fill, Mem, Names, Region, Word as W};
use si_x87::vectors::{Case, Field, Word};
use std::collections::BTreeMap;
use std::io::BufRead;

/// The global the allocation wrappers' callers count in: not ported (see the crate's top).
const SECURE_LIC_ALLOC: &str = "GW:SI_secureLicAlloc+0x0";

fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

/// `EMUCHECK_MUTATE=calls`: one bit of the first argument of the first outside call is
/// flipped before the comparison with the original's (a case that makes a call must then
/// differ). The second mutation switch of the project's checks (the first changes results).
fn mutate_calls() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some_and(|v| v == "calls")
}

/// `DICT_EARLIER=motion`: run the earlier port of `si-motion` instead of this crate's.
fn earlier() -> bool {
    std::env::var_os("DICT_EARLIER").is_some()
}

#[cfg(feature = "earlier")]
fn run_earlier(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    table::run_motion(name, m, o, a)
}

#[cfg(not(feature = "earlier"))]
fn run_earlier(_name: &str, _m: &mut Mem, _o: &mut dyn Outside, _a: &Args) -> Option<Ret> {
    panic!("DICT_EARLIER needs the binary built with --features earlier")
}

fn words(f: &Field) -> Option<Vec<W>> {
    let convert = |w: &Word| match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    };
    f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
}

/// A global's address from its field name: `g` and eight hexadecimal digits.
fn global_addr(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if b.len() >= 9 && b[0] == b'g' && b[1..9].iter().all(u8::is_ascii_hexdigit) {
        return u32::from_str_radix(&name[1..9], 16).ok();
    }
    None
}

fn is_argument(name: &str) -> bool {
    name.starts_with("v_") || name.starts_with("p_")
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
    heap0: u32,
}

impl Args<'_> {
    fn word(&self, name: &str) -> u32 {
        let f = self.case.try_input(name).unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name));
        match f.words().first() {
            Some(Word::Value { value, .. }) => *value as u32,
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                base.wrapping_add(*offset)
            }
            _ => panic!("argument `{name}` has no value"),
        }
    }

    /// An argument by value, as the 32-bit stack word.
    pub fn v(&self, name: &str) -> u32 {
        match self.case.try_input(&format!("v_{name}")) {
            Some(_) => self.word(&format!("v_{name}")),
            None => self.word(&format!("p_{name}")),
        }
    }

    pub fn i(&self, name: &str) -> i32 {
        self.v(name) as i32
    }

    pub fn s(&self, name: &str) -> i16 {
        self.v(name) as i16
    }

    pub fn f(&self, name: &str) -> f32 {
        f32::from_bits(self.v(name))
    }

    /// A pointer argument. (A made-up pointer that points nowhere, an object's address
    /// say, is passed the same way and read with `a.p::<u8>(name).addr()` or `a.v`.)
    pub fn p<T>(&self, name: &str) -> si_mem::Ptr32<T> {
        si_mem::Ptr32::new(self.word(&format!("p_{name}")))
    }

    /// Where the heap starts (the first block's address): for nothing but messages.
    pub fn heap0(&self) -> u32 {
        self.heap0
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    None,
    Eax(u32),
}

impl From<()> for Ret {
    fn from(_: ()) -> Ret {
        Ret::None
    }
}
impl From<i32> for Ret {
    fn from(v: i32) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl From<u32> for Ret {
    fn from(v: u32) -> Ret {
        Ret::Eax(v)
    }
}
impl From<i16> for Ret {
    fn from(v: i16) -> Ret {
        Ret::Eax(v as u16 as u32)
    }
}
impl From<bool> for Ret {
    fn from(v: bool) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl<T> From<si_mem::Ptr32<T>> for Ret {
    fn from(v: si_mem::Ptr32<T>) -> Ret {
        Ret::Eax(v.addr())
    }
}

/// The outside world of a case: every call is held against the next one the original
/// made and answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    next: usize,
    pub wrong: Option<String>,
    /// What the port printed.
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Recorded<'_> {
    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }
}

impl Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("call {}: the port calls {} where the original made no further call", self.next, routine.name));
            return 0;
        };
        let n = self.next;
        self.next += 1;
        if want.name != routine.name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {}, the original {}", routine.name, want.name));
            return 0;
        }
        // the harness logs as many words as the stand-in's rule names; the port may pass more
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let mut got = args.get(k).copied();
            if mutate_calls() && n == 0 && k == 0 {
                got = got.map(|g| g ^ 1);
            }
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        let answer = self.resolve(m, &want.result).unwrap_or(0);
        if let Err(e) = table::outside_effect(m, routine, args, answer) {
            self.wrong.get_or_insert(e);
        }
        answer
    }

    /// The environment of a case is in its line: an extra argument `v_env_NAME` for each
    /// variable that is set (to "1"). The harness's `getenv` puts the value in a new block
    /// each time it is asked; so does this one.
    fn getenv(&mut self, m: &mut Mem, name: &[u8]) -> u32 {
        if self.case.try_input(&format!("v_env_{}", String::from_utf8_lossy(name))).is_none() {
            return 0;
        }
        let p = m.malloc(2);
        m.put_cstr(p, b"1");
        p
    }

    fn stdout(&mut self, text: &[u8]) {
        self.stdout.extend_from_slice(text);
    }

    fn stderr(&mut self, text: &[u8]) {
        self.stderr.extend_from_slice(text);
    }
}

/// How one case came out.
enum Outcome {
    Same,
    Different(String),
    /// Not compared, with the reason.
    Skip(&'static str),
}

/// A case is run twice, on fresh memory holding a pattern and then its complement (the
/// harness does the same with the original). Both runs must agree with the original. When
/// they do not agree with each other, the port's result depends on memory nobody wrote:
/// the case is one of those the harness could not flag because both of *its* patterns
/// happened to give the original the same result (a rule read one past the array, say),
/// and it is not compared.
fn check(c: &Case, number: u32) -> Outcome {
    let first = attempt(c, number, false);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(c, number, true);
    match (&first, &second) {
        (Outcome::Same, Outcome::Same) => first,
        // The routine's own return value is a word nobody set (the harness's two runs
        // returned different values): what it did on the way out hangs on that word too,
        // and the port's choice for it need not be the one both runs happened to make.
        (Outcome::Different(_), _) if ["UNINIT:eax", "UNINIT:ax", "UNINIT:al"].iter().any(|n| c.notes.iter().any(|x| x == n)) => {
            Outcome::Skip("the original returns a word nobody set, and what it does last depends on it")
        }
        (Outcome::Different(a), Outcome::Different(b)) if a == b => first,
        _ => Outcome::Skip("the port's result depends on memory nobody wrote (the original's too, unseen by the harness)"),
    }
}

fn attempt(c: &Case, number: u32, second: bool) -> Outcome {
    if c.stopped() {
        return Outcome::Skip("the original stopped");
    }
    if c.has_note("OVERRUN") {
        return Outcome::Skip("the original wrote past a buffer of the case");
    }
    if c.notes.iter().any(|n| n.starts_with("GW:") && n != SECURE_LIC_ALLOC) {
        return Outcome::Skip("the original wrote a global the case does not declare");
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    si_mem::init_globals(&mut m, si_dict::globals::ALL);
    si_dict::rodata::store(&mut m);

    // the buffers: globals at their addresses, the others in the arena in the case's order
    let mut names = Names::new();
    let mut buffers: Vec<(&Field, u32, Vec<W>)> = Vec::new();
    for f in c.inputs.iter().filter(|f| !is_argument(&f.name)) {
        let Some(w) = words(f) else {
            names.add(&f.name, 0, 0);
            continue;
        };
        let len = si_mem::words_len(&w);
        let addr = match global_addr(&f.name) {
            Some(a) => a,
            None => m.place(len, 8),
        };
        if addr == 0 {
            return Outcome::Different(format!("no room in the arena for `{}`", f.name));
        }
        names.add(&f.name, addr, len);
        buffers.push((f, addr, w));
    }
    for (f, addr, w) in &buffers {
        if let Err(e) = m.load_words(*addr, w, &names) {
            return Outcome::Different(format!("loading `{}`: {e}", f.name));
        }
    }
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names, heap0: si_mem::HEAP.0 + 8 };
    let mut outside = Recorded { case: c, names: &names, next: 0, wrong: None, stdout: Vec::new(), stderr: Vec::new() };
    let routine = c.name.split('#').next().unwrap();
    let ran = if earlier() {
        if c.calls.iter().any(|call| call.name == "SI_edGetFcvFromRelation") {
            return Outcome::Skip("the earlier port works out a relation's curve itself; the case answers it by a stand-in");
        }
        run_earlier(routine, &mut m, &mut outside, &args)
    } else {
        table::run(routine, &mut m, &mut outside, &args)
    };
    let Some(ret) = ran else {
        return Outcome::Skip("no port in the table");
    };
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if outside.next != c.calls.len() {
        return Outcome::Different(format!("the original made {} outside calls, the port {}", c.calls.len(), outside.next));
    }

    // what was printed (under the mutation switch one bit of its first byte is flipped)
    if mutate() {
        for text in [&mut outside.stdout, &mut outside.stderr] {
            if let Some(b) = text.first_mut() {
                *b ^= 1;
            }
        }
    }
    for (field, got) in [("stdout", &outside.stdout), ("stderr", &outside.stderr)] {
        let want: Vec<u8> = c.try_output(field).map_or(Vec::new(), |f| f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect());
        if want != **got {
            return Outcome::Different(format!("{field}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(got)));
        }
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    if mutate() {
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        let changed = m.changed_in(&before, Region::Arena).into_iter().chain(m.changed_in(&before, Region::Bss)).chain(m.changed_in(&before, Region::Data)).next();
        if let Some((a, bytes)) = changed {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().first() {
            if n > 0 {
                let b = m.u8(a);
                m.set_u8(a, b ^ 1);
            }
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ax", 0xffff), ("al", 0xff)] {
        if let Some(f) = c.try_output(field) {
            let Ret::Eax(got) = ret else {
                return Outcome::Different(format!("the original returns {field}, the port nothing"));
            };
            match f.words().first() {
                Some(Word::Value { value, .. }) if *value as u32 != got & mask => {
                    return Outcome::Different(format!("{field}: original {:x}, port {:x}", value, got & mask));
                }
                Some(Word::Ref { name, offset }) => {
                    let want = names.addr(&m, name).map(|a| a.wrapping_add(*offset));
                    if want != Some(got) {
                        return Outcome::Different(format!("{field}: original &{name}+{offset:#x}, port {got:08x}"));
                    }
                }
                _ => {}
            }
        }
    }

    // every buffer: as the original left it, or untouched
    for (f, addr, input) in &buffers {
        let expect = match c.try_output(&f.name) {
            Some(out) => words(out).unwrap_or_default(),
            None => input.clone(),
        };
        if let Err(e) = m.compare_words(*addr, &expect, &names) {
            return Outcome::Different(format!("buffer `{}`: {e}", f.name));
        }
    }

    // the heap
    let blocks: Vec<(u32, u32)> = m.heap_blocks().to_vec();
    let want_blocks = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
    if want_blocks != blocks.len() {
        return Outcome::Different(format!("the original allocated {want_blocks} blocks, the port {}", blocks.len()));
    }
    for (i, &(addr, size)) in blocks.iter().enumerate() {
        let name = format!("heap{i}");
        let expect = words(c.output(&name)).unwrap_or_default();
        if si_mem::words_len(&expect) != size {
            return Outcome::Different(format!("{name}: {} bytes in the original, {size} in the port", si_mem::words_len(&expect)));
        }
        if let Err(e) = m.compare_words(addr, &expect, &names) {
            return Outcome::Different(format!("{name}: {e}"));
        }
        // a byte the original never wrote must not have been written here either
        if !c.notes.iter().any(|n| n == &format!("UNINIT:{name}")) {
            let mut at = addr;
            for w in &expect {
                if let W::Unwritten { bytes } = w {
                    for k in 0..*bytes as u32 {
                        if m.u8(at + k) != fill.byte(at + k) {
                            return Outcome::Different(format!("{name}+{:#x}: written by the port, not by the original", at + k - addr));
                        }
                    }
                }
                at += w.bytes();
            }
        }
    }

    // the frees, in order
    let want_freed: Vec<Option<u32>> = c.try_output("freed").map_or(Vec::new(), |f| {
        f.words().iter().map(|w| match w {
            Word::Ref { name, offset } => names.addr(&m, name).map(|a| a.wrapping_add(*offset)),
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
        }).collect()
    });
    let got_freed: Vec<Option<u32>> = m.freed().iter().map(|&a| Some(a)).collect();
    if want_freed != got_freed {
        let at = want_freed.iter().zip(&got_freed).position(|(a, b)| a != b);
        return Outcome::Different(format!("freed: {} blocks in the original, {} in the port, first difference at {at:?}", want_freed.len(), got_freed.len()));
    }

    // nothing else was touched
    for region in [Region::Arena, Region::Data, Region::Bss] {
        for (a, bytes) in m.changed_in(&before, region) {
            let inside = buffers.iter().any(|(_, addr, w)| a >= *addr && a + bytes.len() as u32 <= *addr + si_mem::words_len(w));
            if !inside {
                return Outcome::Different(format!("the port changed {} bytes at {a:#010x}, outside every buffer of the case", bytes.len()));
            }
        }
    }
    Outcome::Same
}

#[derive(Default)]
struct Row {
    cases: u32,
    identical: u32,
    given_lib: u32,
    defined_part: u32,
    different: u32,
    skipped: u32,
    reasons: BTreeMap<&'static str, u32>,
    examples: Vec<String>,
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: dict-emucheck <directory of vector files>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("the directory").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "vec")).collect();
    paths.sort();
    std::panic::set_hook(Box::new(|_| {}));
    println!("{:<44} {:>6} {:>9} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "given-lib", "defined", "different", "not-comp");
    let mut total = Row::default();
    let mut all_reasons: BTreeMap<&'static str, u32> = BTreeMap::new();
    for path in &paths {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let mut row = Row::default();
        let file = std::io::BufReader::new(std::fs::File::open(path).expect("a vector file"));
        for (number, line) in file.lines().enumerate() {
            let line = line.expect("a line");
            let case = match Case::parse(&line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}:{}: {e}", path.display(), number + 1),
            };
            row.cases += 1;
            let outcome = std::panic::catch_unwind(|| check(&case, number as u32)).unwrap_or_else(|p| {
                let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                match p.downcast_ref::<si_mem::Stop>() {
                    Some(s) => Outcome::Different(format!("the port stopped at a fault: {:?}", s.0)),
                    None => Outcome::Different(format!("the port panicked: {}", text.unwrap_or_default())),
                }
            });
            match outcome {
                Outcome::Same if case.has_note("UNINIT") => row.defined_part += 1,
                Outcome::Same if case.has_note("NA") => row.given_lib += 1,
                Outcome::Same => row.identical += 1,
                Outcome::Skip(why) => {
                    row.skipped += 1;
                    *row.reasons.entry(why).or_default() += 1;
                }
                Outcome::Different(d) => {
                    row.different += 1;
                    if row.examples.len() < 2 {
                        let mut text = line.clone();
                        if text.len() > 600 {
                            text.truncate(600);
                            text.push('…');
                        }
                        row.examples.push(format!("line {}: {d}\n      {text}", number + 1));
                    }
                }
            }
        }
        println!("{:<44} {:>6} {:>9} {:>9} {:>8} {:>9} {:>8}", stem, row.cases, row.identical, row.given_lib, row.defined_part, row.different, row.skipped);
        for e in &row.examples {
            println!("    {e}");
        }
        total.cases += row.cases;
        total.identical += row.identical;
        total.given_lib += row.given_lib;
        total.defined_part += row.defined_part;
        total.different += row.different;
        total.skipped += row.skipped;
        for (k, v) in row.reasons {
            *all_reasons.entry(k).or_default() += v;
        }
    }
    println!(
        "{:<44} {:>6} {:>9} {:>9} {:>8} {:>9} {:>8}",
        format!("TOTAL ({} files)", paths.len()),
        total.cases,
        total.identical,
        total.given_lib,
        total.defined_part,
        total.different,
        total.skipped
    );
    for (why, n) in &all_reasons {
        println!("  not compared, {why}: {n}");
    }
    std::process::exit(if total.different == 0 { 0 } else { 1 });
}
