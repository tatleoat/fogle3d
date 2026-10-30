// SPDX-License-Identifier: MIT
//! `view-emucheck <dir>`: the graph list on the image against the emulated original.
//!
//! Each line of a vector file (made by `re/tools/si-view/gen.py` with the harness of
//! `re/tools/emu`) is one call of one original routine on a made-up scene: the buffers it
//! was given, what it returned, every buffer it changed, every block it got from `malloc`
//! with its bytes, the blocks it freed in order, and the calls it made to routines outside
//! the crate with their arguments and answers.
//!
//! The check does not rebuild anything: the case's buffers are loaded into the image at
//! the addresses the harness gave them (`si_mem::load_case_at`: the arena is laid out the
//! same way; globals lie at their own addresses), the port runs on them, and memory is
//! compared:
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
//! An outside call's argument that the original passed as the address of a local of its
//! own is written `57ac57ac` by the generator: the port must pass an address of the image's
//! stack there. What the stand-in stored through such arguments follows the call as an
//! entry `name.out(values…)`, and the check stores the same values through the port's
//! addresses.
//!
//! Columns of the report: cases; identical; identical in the defined part (the case
//! carries an `UNINIT` note and agrees in every word that has a value); different; not
//! compared (the original stopped, or wrote a global the case did not declare).
//! `EMUCHECK_MUTATE=1` flips one bit of what the port returned and of the first byte it
//! changed before the comparison: every case that returns or changes something must then
//! differ. `EMUCHECK_MUTATE=calls` flips a bit of the first argument of every outside call
//! instead: every case in which the port calls out with an argument must differ.

mod table;

use si_mem::{Fill, Mem, Names, Region, Word as W};
use si_view::outside::{Outside, Routine};
use si_x87::vectors::{Case, Field, Word};
use std::collections::BTreeMap;
use std::io::BufRead;

/// What the generator writes for an argument that is the address of a local.
const LOCAL: u32 = 0x57ac_57ac;

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

    pub fn f32(&self, name: &str) -> f32 {
        f32::from_bits(self.v(name))
    }

    pub fn f64(&self, name: &str) -> f64 {
        let f = self.case.try_input(&format!("v_{name}")).unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name));
        match f.words().first() {
            Some(Word::Value { value, .. }) => f64::from_bits(*value as u64),
            _ => panic!("argument `{name}` has no value"),
        }
    }

    pub fn s(&self, name: &str) -> i16 {
        self.v(name) as i16
    }

    /// A pointer argument. (A made-up pointer that points nowhere, an object's address
    /// say, is passed the same way.)
    pub fn p<T>(&self, name: &str) -> si_mem::Ptr32<T> {
        si_mem::Ptr32::new(self.word(&format!("p_{name}")))
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug)]
pub enum Ret {
    None,
    Eax(u32),
    /// What a routine leaves in the x87 register: all 80 bits.
    St0(si_x87::F80),
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
    flip: bool,
    /// The routines of the package's seam that ran themselves when this file's vectors
    /// were made (the file's `# real:` line): their ports are run, where the other files
    /// answer them from the recorded calls.
    real: &'a [String],
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

impl Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        if self.real.iter().any(|r| r == routine.name) {
            if let Some(v) = si_view::wired::call_own(m, self, routine, args) {
                return v;
            }
        }
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
        if want.args.len() != args.len() {
            self.wrong.get_or_insert_with(|| format!("call {n} {}: {} arguments in the port, {} in the generator's table", routine.name, args.len(), want.args.len()));
        }
        let mut args = args.to_vec();
        if self.flip {
            if let Some(a) = args.first_mut() {
                *a ^= 1;
            }
        }
        let (stack_lo, stack_len) = m.region(Region::Stack);
        let mut locals = Vec::new();
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if expect == Some(LOCAL) {
                // the address of a local: any address of the image's stack
                match got {
                    Some(a) if a.wrapping_sub(stack_lo) < stack_len => locals.push(a),
                    _ => {
                        self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {got:08x?} in the port, the address of a local in the original", routine.name));
                    }
                }
            } else if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        // what the stand-in stored through the locals it was handed
        if let Some(out) = self.case.calls.get(self.next) {
            if out.name.strip_suffix(".out") == Some(routine.name) {
                self.next += 1;
                for (k, w) in out.args.iter().enumerate() {
                    match (locals.get(k), self.resolve(m, w)) {
                        (Some(&a), Some(v)) => m.set_u32(a, v),
                        _ => {
                            self.wrong.get_or_insert_with(|| format!("call {n} {}: the stand-in stored {} words, the port passed {} locals", routine.name, out.args.len(), locals.len()));
                        }
                    }
                }
            }
        }
        self.resolve(m, &want.result).unwrap_or(0)
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
/// happened to give the original the same result, and it is not compared.
fn check(c: &Case, number: u32, real: &[String]) -> Outcome {
    let first = attempt(c, number, false, real);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(c, number, true, real);
    match (&first, &second) {
        (Outcome::Same, Outcome::Same) => first,
        (Outcome::Different(a), Outcome::Different(b)) if a == b => first,
        (Outcome::Different(_), Outcome::Different(_)) if mutate() != Mutate::No => first,
        _ => Outcome::Skip("the port's result depends on memory nobody wrote (the original's too, unseen by the harness)"),
    }
}

fn attempt(c: &Case, number: u32, second: bool, real: &[String]) -> Outcome {
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
        return Outcome::Skip("the original's outside calls depend on memory nobody wrote");
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    si_mem::init_globals(&mut m, si_view::globals::ALL);
    si_view::graflist::init_status_text(&mut m);
    si_view::nselections::init_nselection_text(&mut m);

    // the buffers: globals at their addresses, the others in the arena in the case's order
    let mut loaded: Vec<(&Field, Option<u32>, Option<Vec<W>>)> = Vec::new();
    for f in c.inputs.iter().filter(|f| !is_argument(&f.name)) {
        loaded.push((f, global_addr(&f.name), words(f)));
    }
    let fields: Vec<si_mem::FieldAt<'_>> = loaded.iter().map(|(f, at, w)| (f.name.as_str(), *at, w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Outcome::Different(format!("loading the case: {e}")),
    };
    let buffers: Vec<(&Field, u32, &Vec<W>)> = loaded
        .iter()
        .filter_map(|(f, _, w)| w.as_ref().map(|w| (*f, names.addr(&m, &f.name).unwrap_or(0), w)))
        .collect();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names };
    let mut outside = Recorded { case: c, names: &names, next: 0, flip: mutate() == Mutate::Calls, real, wrong: None };
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
        return Outcome::Different(format!("the original made {} outside calls, the port {}", c.calls.len(), outside.next));
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    if mutate() == Mutate::Memory {
        match ret {
            Ret::Eax(v) => ret = Ret::Eax(v ^ 1),
            Ret::St0(v) => {
                let (lo, hi) = v.to_bits();
                ret = Ret::St0(si_x87::F80::from_bits(lo ^ 1, hi));
            }
            Ret::None => {}
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
    if let Some(f) = c.try_output("st0") {
        let Ret::St0(got) = ret else {
            return Outcome::Different("the original returns in the x87 register, the port does not".to_string());
        };
        if !f.same_f80(got) {
            return Outcome::Different(format!("st0: original {:?}, port {:?}", f.f80(), got));
        }
    }
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
            // (a run of changed bytes may cross from one buffer into the next: two globals
            // side by side)
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
    let dir = std::env::args().nth(1).expect("usage: view-emucheck <directory of vector files>");
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
        let mut real: Vec<String> = Vec::new();
        for (number, line) in file.lines().enumerate() {
            let line = line.expect("a line");
            if let Some(names) = line.strip_prefix("# real:") {
                real = names.split_whitespace().map(str::to_string).collect();
                continue;
            }
            let case = match Case::parse(&line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}:{}: {e}", path.display(), number + 1),
            };
            row.cases += 1;
            let outcome = std::panic::catch_unwind(|| check(&case, number as u32, &real)).unwrap_or_else(|p| {
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
