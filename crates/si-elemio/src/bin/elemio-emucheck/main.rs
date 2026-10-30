// SPDX-License-Identifier: MIT
//! `elemio-emucheck <dir>`: the element file readers on the image against the emulated
//! original (after `view-emucheck` of `si-view`, whose method this is).
//!
//! Each line of a vector file (made by `re/tools/si-elemio/gen.py` with the harness of
//! `re/tools/emu`) is one call of one original routine: the file's bytes, the buffers and
//! globals it was given, what it returned, every buffer and global it changed, every block
//! it got from `malloc` with its bytes, the blocks it freed in order, the calls it made to
//! routines outside the crate, what it printed, and where it left the stream.
//!
//! The check rebuilds nothing: the case's buffers are loaded into the image at the
//! addresses the harness gave them (`si_mem::load_case_at`), the stream is opened on the
//! same bytes at the address the harness gave it, the port runs, and memory is compared:
//!
//! - the return value;
//! - every buffer and global of the case, changed or not;
//! - the heap: as many blocks, each of the same size, each written byte the same, each
//!   byte the original did not write still holding the fill;
//! - the freed blocks, in order;
//! - the outside calls, in order, with their arguments;
//! - the standard output; each stream's position and state;
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! **A case that ends in `STOP:endless`** is one the generator stopped at the token read
//! after which the original's loop can never end (the read failed and left a token that
//! is not the loop's end). The port must stop at the same read
//! (`NoReturn::Endless`), and everything above but the return value is compared as the
//! original has it at that moment.
//!
//! Naming in a vector line: `v_x` an argument by value, `p_x` a pointer argument, `f_x` a
//! stream argument (its value is the file's bytes), `file:PATH` a file `fopen` finds,
//! `gXXXXXXXX…` a global at that address, anything else a buffer of the arena.
//!
//! Columns of the report: cases; identical; identical in the defined part (the case has an
//! `UNINIT` note and agrees in every word that has a value); different; not compared.
//! `EMUCHECK_MUTATE=1` flips one bit of what the port returned and of the first byte it
//! changed (or of its first heap block) before the comparison; `EMUCHECK_MUTATE=calls`
//! flips a bit of the first argument of every recorded outside call instead.

mod table;

use si_elemio::image::host::Files;
use si_elemio::image::outside::{Outside, Routine, TIME};
use si_elemio::image::NoReturn;
use si_mem::{Fill, Mem, Names, Region, Word as W};
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
    name.starts_with("v_") || name.starts_with("p_") || name.starts_with("f_") || name.starts_with("file:")
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
    streams: &'a BTreeMap<String, u32>,
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

    /// A pointer argument (a made-up pointer that points nowhere is passed the same way).
    pub fn p(&self, name: &str) -> u32 {
        self.word(&format!("p_{name}"))
    }

    /// A stream argument: the address the stream was opened at.
    pub fn f(&self, name: &str) -> u32 {
        *self.streams.get(&format!("f_{name}")).unwrap_or_else(|| panic!("case of {} has no stream `{name}`", self.case.name))
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug)]
pub enum Ret {
    None,
    Eax(u32),
    /// The port stopped where the original does not come back.
    Stopped(NoReturn),
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
        Ret::Eax(v as i32 as u32)
    }
}
impl<T: Into<Ret>> From<Result<T, NoReturn>> for Ret {
    fn from(v: Result<T, NoReturn>) -> Ret {
        match v {
            Ok(v) => v.into(),
            Err(e) => Ret::Stopped(e),
        }
    }
}

/// The outside world of a case: the streams are served from the case's files; every other
/// call is held against the next one the original made and answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    next: usize,
    flip: bool,
    pub files: Files,
    pub printed: Vec<u8>,
    pub printed_err: Vec<u8>,
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
        if let Some(v) = self.files.call(m, routine, args) {
            return v;
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
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if expect == Some(LOCAL) {
                if !matches!(got, Some(a) if a.wrapping_sub(stack_lo) < stack_len) {
                    self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {got:08x?} in the port, the address of a local in the original", routine.name));
                }
            } else if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        let answer = self.resolve(m, &want.result).unwrap_or(0);
        if routine.addr == TIME.addr && args.first().is_some_and(|a| *a != 0) {
            // the C library's `time` stores its answer through its argument
            m.set_u32(args[0] ^ self.flip as u32, answer);
        }
        answer
    }

    fn tan(&mut self, _m: &mut Mem, x: f64) -> f64 {
        // the C library's answer is replayed from the case (the harness asked this
        // machine's library, which is not the library of 2002: finding 0011); what is held
        // against the original is that the port asks the same question
        let n = self.next;
        self.next += 1;
        match self.case.calls.get(n) {
            Some(call) if call.name == "tan" => {
                if call.arg_values().first().map(|v| *v as u64) != Some(x.to_bits()) {
                    self.wrong.get_or_insert_with(|| format!("call {n} tan: argument {:016x} in the port, {:x?} in the original", x.to_bits(), call.arg_values()));
                }
                call.result_f80().to_f64()
            }
            other => {
                self.wrong.get_or_insert_with(|| format!("call {n}: the port calls tan, the original {:?}", other.map(|c| c.name.clone())));
                0.0
            }
        }
    }

    fn print(&mut self, _m: &mut Mem, text: &[u8]) {
        self.printed.extend_from_slice(text);
    }

    fn print_err(&mut self, _m: &mut Mem, text: &[u8]) {
        self.printed_err.extend_from_slice(text);
    }

    fn file_left(&mut self, _m: &mut Mem, file: u32) -> u32 {
        self.files.left(file)
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
        (_, Outcome::Skip(_)) => second,
        _ => Outcome::Skip("the port's result depends on memory nobody wrote (the original's too, unseen by the harness)"),
    }
}

fn attempt(c: &Case, number: u32, second: bool) -> Outcome {
    // the one stop that is compared: the generator's own, at the read after which the
    // original's loop cannot end
    let endless = c.notes.iter().any(|n| n.starts_with("STOP:endless"));
    if c.stopped() && !endless {
        return Outcome::Skip("the original stopped");
    }
    if c.has_note("OVERRUN") {
        return Outcome::Skip("the original wrote past a buffer of the case");
    }
    // `SI_secureLicAlloc` is a counter of the licence scheme that two readers increment;
    // the ports leave it alone (it is not ported), so a write to it is not a difference
    if c.notes.iter().any(|n| n.starts_with("GW:") && !n.starts_with("GW:SI_secureLicAlloc")) {
        return Outcome::Skip("the original wrote a global the case does not declare");
    }
    if c.has_note("UNINIT:calls") {
        return Outcome::Skip("the original's outside calls depend on memory nobody wrote");
    }
    // the heap is as long as the generator's (`elemlib.HEAP_SIZE`), so that an allocation
    // fails here where it failed there
    let mut m = Mem::with_map(si_mem::Map { heap: (si_mem::HEAP.0, 0x10_0000), ..si_mem::Map::default() });
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    si_elemio::image::globals::init(&mut m);

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

    // the streams, in the order the harness opened them, and the files `fopen` finds
    let mut files = Files::new();
    let mut streams: BTreeMap<String, u32> = BTreeMap::new();
    for f in &c.inputs {
        if f.name.starts_with("f_") {
            streams.insert(f.name.clone(), files.open(f.to_bytes()));
        } else if let Some(path) = f.name.strip_prefix("file:") {
            files.add_named(path.as_bytes(), f.to_bytes());
        }
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names, streams: &streams };
    let mut outside = Recorded { case: c, names: &names, next: 0, flip: mutate() == Mutate::Calls, files, printed: Vec::new(), printed_err: Vec::new(), wrong: None };
    let routine = c.name.split('#').next().unwrap();
    let signalling = si_elemio::image::signalling_nans_moved();
    let Some(ret) = table::run(routine, &mut m, &mut outside, &args) else {
        return Outcome::Skip("no port in the table");
    };
    if si_elemio::image::signalling_nans_moved() != signalling {
        return Outcome::Skip("a signalling NaN went through the x87 unit, which the emulator does not quiet as a real unit does");
    }
    match ret {
        Ret::Stopped(NoReturn::Endless(who)) if endless => {
            if !c.notes.iter().any(|n| n == &format!("STOP:endless:{who}")) {
                return Outcome::Different(format!("the port stops at the loop of {who}, the original at another: {:?}", c.notes));
            }
        }
        Ret::Stopped(NoReturn::Endless(who)) => return Outcome::Different(format!("the port stops at an endless loop of {who}, the original returned")),
        Ret::Stopped(NoReturn::Stack(_)) => return Outcome::Skip("the port stops where the original overruns a local of its stack"),
        Ret::Stopped(NoReturn::Count(_)) => return Outcome::Skip("the port does not allocate for a count beyond the file"),
        Ret::Stopped(NoReturn::Fault(who)) => return Outcome::Different(format!("the port stopped at a fault in {who}: {:?}", m.fault())),
        _ if endless => return Outcome::Different("the original's loop cannot end, the port returned".to_string()),
        _ => {}
    }
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
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        let changed = m.changed_in(&before, Region::Arena).into_iter().chain(m.changed_in(&before, Region::Bss)).chain(m.changed_in(&before, Region::Data)).next();
        if let Some((a, bytes)) = changed {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, _)) = m.heap_blocks().iter().find(|b| b.1 > 0) {
            let b = m.u8(a);
            m.set_u8(a, b ^ 1);
        }
    }

    // the return value
    if !endless {
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

    // what was printed
    let want_printed = c.try_output("stdout").map_or(Vec::new(), |f| f.to_bytes());
    if want_printed != outside.printed {
        return Outcome::Different(format!("stdout: {} bytes in the original, {} in the port", want_printed.len(), outside.printed.len()));
    }

    let want_printed = c.try_output("stderr").map_or(Vec::new(), |f| f.to_bytes());
    if want_printed != outside.printed_err {
        return Outcome::Different(format!("stderr: {} bytes in the original, {} in the port", want_printed.len(), outside.printed_err.len()));
    }

    // the streams: position and state (end met 1, closed 4)
    for (name, &addr) in &streams {
        let s = outside.files.stream(addr).unwrap();
        if let Some(f) = c.try_output(&format!("{name}.pos")) {
            if f.u32() != s.pos as u32 {
                return Outcome::Different(format!("{name}: the original left the stream at {}, the port at {}", f.u32(), s.pos));
            }
        }
        if let Some(f) = c.try_output(&format!("{name}.state")) {
            let state = s.eof as u32 | (s.closed as u32) << 2;
            if f.values().first().map(|v| *v as u32) != Some(state) {
                return Outcome::Different(format!("{name}: stream state {:?} in the original, {state} in the port", f.values().first()));
            }
        }
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
    endless: u32,
    reasons: BTreeMap<&'static str, u32>,
    examples: Vec<String>,
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: elemio-emucheck <directory of vector files>");
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
            let endless = case.notes.iter().any(|n| n.starts_with("STOP:endless"));
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
                        if text.len() > 900 {
                            text.truncate(900);
                            text.push('…');
                        }
                        row.examples.push(format!("line {}: {d}\n      {text}", number + 1));
                    }
                }
            }
            if endless && matches!(row.examples.len(), _) {
                row.endless += 1;
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
        total.endless += row.endless;
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
    println!("  of all cases, stopped by the generator at a loop that cannot end and compared up to there: {}", total.endless);
    for (why, n) in &all_reasons {
        println!("  not compared, {why}: {n}");
    }
    std::process::exit(if total.different == 0 { 0 } else { 1 });
}
