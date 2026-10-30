// SPDX-License-Identifier: MIT
//! `gen-emucheck <dir>`: genList.c on the image against the emulated original (finding 0033).
//!
//! Each line of a vector file (made by `re/tools/si-gen/gen_list.py` with the harness of
//! `re/tools/emu`) is one call of one original routine on made-up lists: the buffers it was
//! given, what it returned, every buffer it changed, every block it got from `malloc` with
//! its bytes, the blocks it freed in order, and the calls it made with their arguments: to
//! `fprintf`, and to the callbacks it was handed (stubs of the harness, recorded under the
//! argument's name: `destroy`, `cmp`, `copy`, `dump`).
//!
//! The check loads the case's buffers into the image at the addresses the harness gave them
//! (`si_mem::load_case_at`), runs the port on them and compares:
//!
//! - the return value;
//! - every buffer of the case, changed or not;
//! - the heap: as many blocks, each of the same size, each written byte the same, each
//!   byte the original did not write still holding the fill;
//! - the freed blocks, in order;
//! - the calls, in order, with their arguments (a callback is known by its address: the
//!   argument of the case that holds it names it);
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! Columns of the report: cases; identical; identical in the defined part (the case
//! carries an `UNINIT` note and agrees in every word that has a value); different; not
//! compared (the original stopped). `EMUCHECK_MUTATE=1` flips one bit of what the port
//! returned and of the first byte it changed before the comparison: every case that returns
//! or changes something must then differ. `EMUCHECK_MUTATE=calls` flips a bit of the first
//! argument of every call instead: every case in which the port calls out must differ.

mod table;

use si_mem::{Fill, Mem, Names, Region, Routine, Word as W};
use si_x87::vectors::{Case, Field, Word};
use std::collections::BTreeMap;
use std::io::BufRead;

/// The arguments that hand a routine a callback: a call to the address one of them holds is
/// recorded under its name.
const CALLBACKS: [&str; 4] = ["destroy", "cmp", "copy", "dump"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mutate {
    No,
    Memory,
    Calls,
}

fn mutate() -> Mutate {
    match std::env::var("EMUCHECK_MUTATE").ok().as_deref() {
        None | Some("") | Some("0") => Mutate::No,
        Some("calls") => Mutate::Calls,
        Some(_) => Mutate::Memory,
    }
}

fn words(f: &Field) -> Option<Vec<W>> {
    let convert = |w: &Word| match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    };
    f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
}

fn is_argument(name: &str) -> bool {
    name.starts_with("v_") || name.starts_with("p_")
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
}

impl Args<'_> {
    fn word(&self, name: &str) -> Option<u32> {
        let f = self.case.try_input(name)?;
        match f.words().first() {
            Some(Word::Value { value, .. }) => Some(*value as u32),
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                Some(base.wrapping_add(*offset))
            }
            _ => panic!("argument `{name}` has no value"),
        }
    }

    /// An argument as the 32-bit stack word (by value or a pointer).
    pub fn v(&self, name: &str) -> u32 {
        self.word(&format!("v_{name}"))
            .or_else(|| self.word(&format!("p_{name}")))
            .unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name))
    }

    pub fn i(&self, name: &str) -> i32 {
        self.v(name) as i32
    }

    /// The callback a routine at `addr` is, by the argument that holds it.
    fn callback(&self, addr: u32) -> Option<&'static str> {
        CALLBACKS.into_iter().find(|n| self.word(&format!("v_{n}")) == Some(addr))
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug)]
pub enum Ret {
    None,
    Eax(u32),
}

impl From<()> for Ret {
    fn from(_: ()) -> Ret {
        Ret::None
    }
}
impl From<u32> for Ret {
    fn from(v: u32) -> Ret {
        Ret::Eax(v)
    }
}

/// The outside world of a case: every call is held against the next one the original
/// made and answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    args: &'a Args<'a>,
    names: &'a Names,
    next: usize,
    flip: bool,
    pub wrong: Option<String>,
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

impl si_mem::Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> si_mem::Ret {
        let name = if routine.name.is_empty() {
            match self.args.callback(routine.addr) {
                Some(n) => n,
                None => {
                    self.wrong.get_or_insert_with(|| format!("call {}: the port calls {:08x}, which no argument of the case holds", self.next, routine.addr));
                    return si_mem::Ret::eax(0);
                }
            }
        } else {
            routine.name
        };
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("call {}: the port calls {name} where the original made no further call", self.next));
            return si_mem::Ret::eax(0);
        };
        let n = self.next;
        self.next += 1;
        if want.name != name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {name}, the original {}", want.name));
            return si_mem::Ret::eax(0);
        }
        if want.args.len() != args.len() {
            self.wrong.get_or_insert_with(|| format!("call {n} {name}: {} arguments in the port, {} in the original", args.len(), want.args.len()));
        }
        let mut args = args.to_vec();
        if self.flip {
            if let Some(a) = args.first_mut() {
                *a ^= 1;
            }
        }
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, {expect:08x?} in the original"));
            }
        }
        si_mem::Ret::eax(self.resolve(m, &want.result).unwrap_or(0))
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
/// they do not agree with each other, the port's result depends on memory nobody wrote, and
/// the case is not compared.
fn check(c: &Case, number: u32) -> Outcome {
    let first = attempt(c, number, false);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(c, number, true);
    match (&first, &second) {
        (Outcome::Same, Outcome::Same) => first,
        (Outcome::Different(a), Outcome::Different(b)) if a == b => first,
        (Outcome::Different(_), Outcome::Different(_)) if mutate() != Mutate::No => first,
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
    if c.notes.iter().any(|n| n.starts_with("GW:")) {
        return Outcome::Skip("the original wrote a global the case does not declare");
    }
    if c.has_note("UNINIT:calls") {
        return Outcome::Skip("the original's calls depend on memory nobody wrote");
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);

    // the buffers, in the arena in the case's order (genList.c has no globals)
    let loaded: Vec<(&Field, Option<Vec<W>>)> = c.inputs.iter().filter(|f| !is_argument(&f.name)).map(|f| (f, words(f))).collect();
    let fields: Vec<si_mem::FieldAt<'_>> = loaded.iter().map(|(f, w)| (f.name.as_str(), None, w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Outcome::Different(format!("loading the case: {e}")),
    };
    let buffers: Vec<(&Field, u32, &Vec<W>)> = loaded.iter().filter_map(|(f, w)| w.as_ref().map(|w| (*f, names.addr(&m, &f.name).unwrap_or(0), w))).collect();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names };
    let mut outside = Recorded { case: c, args: &args, names: &names, next: 0, flip: mutate() == Mutate::Calls, wrong: None };
    let routine = c.name.split('#').next().unwrap();
    let Some(ret) = table::run(routine, &mut m, &mut outside, &args) else {
        return Outcome::Skip("no port in the table");
    };
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if outside.next != c.calls.len() {
        return Outcome::Different(format!("the original made {} calls, the port {}", c.calls.len(), outside.next));
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    if mutate() == Mutate::Memory {
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        if let Some((a, bytes)) = m.changed_in(&before, Region::Arena).into_iter().next() {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().first() {
            if n > 0 {
                let b = m.u8(a);
                m.set_u8(a, b ^ 1);
            }
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ax", 0xffff), ("al", 0xff), ("ptr", u32::MAX)] {
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
            None => (*input).clone(),
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
        f.words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => names.addr(&m, name).map(|a| a.wrapping_add(*offset)),
                Word::Value { value, .. } => Some(*value as u32),
                Word::Unwritten { .. } => None,
            })
            .collect()
    });
    let got_freed: Vec<Option<u32>> = m.freed().iter().map(|&a| Some(a)).collect();
    if want_freed != got_freed {
        let at = want_freed.iter().zip(&got_freed).position(|(a, b)| a != b);
        return Outcome::Different(format!("freed: {} blocks in the original, {} in the port, first difference at {at:?}", want_freed.len(), got_freed.len()));
    }

    // nothing else was touched
    for region in [Region::Arena, Region::Data, Region::Bss] {
        for (a, bytes) in m.changed_in(&before, region) {
            let inside = (0..bytes.len() as u32).all(|k| buffers.iter().any(|(_, addr, w)| a + k >= *addr && a + k < *addr + si_mem::words_len(w)));
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
    defined_part: u32,
    different: u32,
    skipped: u32,
    reasons: BTreeMap<&'static str, u32>,
    examples: Vec<String>,
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: gen-emucheck <directory of vector files>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("the directory").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "vec")).collect();
    paths.sort();
    std::panic::set_hook(Box::new(|_| {}));
    println!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "defined", "different", "not-comp");
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
                Outcome::Same => row.identical += 1,
                Outcome::Skip(why) => {
                    row.skipped += 1;
                    *row.reasons.entry(why).or_default() += 1;
                }
                Outcome::Different(d) => {
                    row.different += 1;
                    if row.examples.len() < 2 {
                        let mut text = line.clone();
                        if text.len() > 700 {
                            text.truncate(700);
                            text.push('…');
                        }
                        row.examples.push(format!("line {}: {d}\n      {text}", number + 1));
                    }
                }
            }
        }
        println!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", stem, row.cases, row.identical, row.defined_part, row.different, row.skipped);
        for e in &row.examples {
            println!("    {e}");
        }
        total.cases += row.cases;
        total.identical += row.identical;
        total.defined_part += row.defined_part;
        total.different += row.different;
        total.skipped += row.skipped;
        for (k, v) in row.reasons {
            *all_reasons.entry(k).or_default() += v;
        }
    }
    println!(
        "{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}",
        format!("TOTAL ({} files)", paths.len()),
        total.cases,
        total.identical,
        total.defined_part,
        total.different,
        total.skipped
    );
    for (why, n) in &all_reasons {
        println!("  not compared, {why}: {n}");
    }
    std::process::exit(if total.different == 0 { 0 } else { 1 });
}
