// SPDX-License-Identifier: MIT
//! `hrc-emucheck <dir>`: the readers on the image against the emulated original.
//!
//! Each line of a vector file (made by `re/tools/si-hrc/gen.py` with the harness of
//! `re/tools/emu`) is one call of one original routine on a made-up file: the bytes of the
//! file, the buffers and globals it was given, what it returned, every buffer it changed,
//! every block it got from `malloc` with its bytes, the blocks it freed in order, where the
//! stream stood afterwards, and the calls it made to routines of other source files with
//! their arguments and answers.
//!
//! The check does not rebuild anything: the case's buffers are placed in the image at the
//! addresses the harness gave them (the arena is laid out the same way; globals lie at
//! their own addresses), the file is served by the same in-memory streams under the same
//! handle, the port runs, and memory is compared:
//!
//! - the return value;
//! - every buffer of the case, changed or not;
//! - the heap: as many blocks, each of the same size, each written byte the same, each
//!   byte the original did not write still holding the fill;
//! - the freed blocks, in order;
//! - the stream: its position, and whether its end was met or it was closed;
//! - the outside calls, in order, with their arguments;
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! Naming in a vector line: `v_x` is an argument by value, `p_x` a pointer argument, `file`
//! the stream argument's bytes, `file:PATH` a named file, `gXXXXXXXX…` a global at that
//! address, anything else a buffer of the arena.
//!
//! Every case is run twice, on memory holding a pattern and then its complement, as the
//! harness runs the original; both runs must agree with the original. Columns of the
//! report: cases; identical; identical in the words that have a value (the case carries an
//! `UNINIT` note: part of what the original left depends on memory nobody wrote);
//! different; not compared (the original stopped, or its undefined part steers what it
//! does). `EMUCHECK_MUTATE=1` flips one bit of what the port returned, of the first byte it
//! changed and of the stream's position before the comparison: every case that returns
//! something, changes something or has a stream must then differ.

mod table;

use si_hrc::image::{Files, Outside, Routine};
use si_mem::{Fill, Mem, Names, Region, Word as W};
use si_x87::vectors::{Case, Field, Word};
use std::collections::BTreeMap;
use std::io::BufRead;

/// The counter the original bumps before some allocations: not ported (see the module's top).
const SECURE_LIC_ALLOC: &str = "GW:SI_secureLicAlloc+0x0";

fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
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

fn is_buffer(name: &str) -> bool {
    !(name.starts_with("v_") || name.starts_with("p_") || name == "file" || name.starts_with("file:"))
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
    file: u32,
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
        self.word(&format!("v_{name}"))
    }

    pub fn s(&self, name: &str) -> i16 {
        self.v(name) as i16
    }

    pub fn f(&self, name: &str) -> f32 {
        f32::from_bits(self.v(name))
    }

    /// A pointer argument, as an address.
    pub fn p(&self, name: &str) -> u32 {
        self.word(&format!("p_{name}"))
    }

    /// A pointer argument, typed.
    pub fn t<T>(&self, name: &str) -> si_mem::Ptr32<T> {
        si_mem::Ptr32::new(self.p(name))
    }

    /// The stream argument.
    pub fn file(&self) -> u32 {
        self.file
    }

    /// The bytes of the stream argument's file.
    pub fn file_bytes(&self) -> Vec<u8> {
        self.case.try_input("file").map_or(Vec::new(), |f| f.to_bytes())
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    None,
    Eax(u32),
    /// The case is not compared, for the reason given.
    Skip(&'static str),
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

/// The outside world of a case: the streams over the case's files, and every call to a
/// routine of another source file held against the next one the original made and
/// answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    pub files: Files,
    next: usize,
    pub wrong: Option<String>,
    /// A running digest of the routines the port asked for, in order.
    pub asked: u64,
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
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        self.files.fopen(m, path, mode)
    }
    fn fclose(&mut self, _m: &mut Mem, file: u32) -> i32 {
        self.files.fclose(file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        let at = self.files.stream(file).map_or(0, |s| s.pos);
        let got = self.files.fread(m, ptr, size, n, file);
        if std::env::var_os("EMUCHECK_TRACE").is_some() {
            eprintln!("fread {ptr:08x} {size}x{n} at {at:#x}: {got} {:02x?}", m.bytes(ptr, (size * got).min(8)));
        }
        got
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.files.fgetc(m, file)
    }
    fn rewind(&mut self, _m: &mut Mem, file: u32) {
        self.files.rewind(file)
    }
    fn bytes_left(&mut self, _m: &mut Mem, file: u32) -> u64 {
        self.files.bytes_left(file)
    }

    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.asked = self.asked.wrapping_mul(0x100_0000_01b3).wrapping_add(routine.addr as u64);
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
            let got = args.get(k).copied();
            // the address of a local of the caller: the stand-in logged a mark instead
            if expect == Some(STACK_MARK) {
                continue;
            }
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        let answer = self.resolve(m, &want.result).unwrap_or(0);
        table::outside_effect(m, routine, args);
        answer
    }

    /// A vector of the first pass (it has no `v_pass` word) was recorded with the readers
    /// of `binary.c` that were not on the image then as stand-ins: the node reader's calls
    /// to them are held against the recorded ones, as its calls to other source files are.
    ///
    /// The byte-order fixers of custom effects (`byteswap.c`, 0x083f966c to 0x083f9810, ported
    /// in `si-app`) were stand-ins in every vector recorded so far, of both passes.
    fn stands_in(&mut self, routine: &'static Routine) -> bool {
        matches!(routine.addr, 0x083f966c | 0x083f96e4 | 0x083f9730 | 0x083f9810) || self.case.try_input("v_pass").is_none()
    }

    /// The C library's `cos`: the harness answered with the host's and wrote the call into
    /// the vector; the port must ask for the same argument bits and gets the same answer.
    fn cos(&mut self, _m: &mut Mem, x: f64) -> f64 {
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| "the port calls cos where the original made no further call".to_string());
            return 0.0;
        };
        self.next += 1;
        if want.name != "cos" {
            self.wrong.get_or_insert_with(|| format!("the port calls cos, the original {}", want.name));
            return 0.0;
        }
        let arg = want.args.first().and_then(Word::value).unwrap_or(0) as u64;
        if arg != x.to_bits() {
            self.wrong.get_or_insert_with(|| format!("cos: argument {:016x} in the port, {arg:016x} in the original", x.to_bits()));
        }
        want.result_f80().to_f64()
    }
}

/// What a stand-in of the harness logs in place of the address of a caller's local.
const STACK_MARK: u32 = 0x57ac_57ac;

/// Bytes of a record that the original takes from stack it never wrote.
///
/// The 2D and 3D texture readers fill a working copy on their stack and copy it into the
/// new record; three fields of the 2D copy and one of the 3D copy are set neither by the
/// initialiser nor (unless their token is in the file) by the reader, and are copied all
/// the same. Called alone by the harness, the reader finds fill there and the harness
/// marks the words undefined. Called from a file or material reader, it finds what
/// earlier callees left in that part of the stack (saved registers, return addresses,
/// bytes of the header): a value the same in both of the harness's runs, which no port
/// without the original's frames can have. Such bytes are not compared, and a case that
/// has any is counted in the report's `residue` line. In a 2D texture (0x1d8 bytes): the
/// reflectivity factor +0x134 and the flag derived from it at +0x118, +0x1b8, +0x1ca; in
/// a 3D texture (0x158 bytes): the reflection factor +0x114.
fn residue(size: u32) -> &'static [(u32, u32)] {
    match size {
        0x1d8 => &[(0x118, 2), (0x134, 4), (0x1b8, 4), (0x1ca, 2)],
        0x158 => &[(0x114, 4)],
        _ => &[],
    }
}

thread_local! {
    /// Cases in which a residue byte differed (see [`residue`]).
    static RESIDUE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How one case came out.
enum Outcome {
    Same,
    Different(String),
    /// Not compared, with the reason.
    Skip(&'static str),
}

fn check(c: &Case, number: u32) -> Outcome {
    let salt = number.wrapping_mul(4099);
    let mut left = [0u64; 3];
    let first = attempt(c, number, Fill::Pattern { second: false, salt }, &mut left[0]);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(c, number, Fill::Pattern { second: true, salt }, &mut left[1]);
    match (first, second) {
        (Outcome::Same, Outcome::Same) => Outcome::Same,
        (Outcome::Skip(why), _) | (_, Outcome::Skip(why)) => Outcome::Skip(why),
        // The port agrees with the original on one fill pattern and not on the other: what
        // it does hangs on memory nobody wrote. For a faithful port that is so exactly
        // when the original's does too, in a case the harness could not flag because both
        // of its own patterns happened to steer the original the same way. Counted apart,
        // and each routine that shows it is argued from the listing in finding 0004.
        (Outcome::Different(_), Outcome::Same) | (Outcome::Same, Outcome::Different(_)) => {
            Outcome::Skip("the port's two runs disagree: uninitialised memory steers the routine, unseen by the harness")
        }
        // Both runs differ from the original. If they also differ from each other where
        // the original left a defined value (or in where they stop), or if a third run on
        // zeroed memory comes out otherwise than they did, the port took a value from
        // stack that nothing in this call had written. The original, at that spot, finds
        // what an earlier callee of the same call left in its dead frame (a truncated read
        // into a scratch word, a pointer nobody cleared): a value that is the same in both
        // of the harness's runs and that no port without the original's frames can have.
        // Counted apart. A port that is wrong the same way on all three fills is different.
        (Outcome::Different(a), Outcome::Different(_)) => {
            const STACK: &str = "the port's runs leave different memory: a word taken from stack this call never wrote (in the original: what an earlier callee left there)";
            if left[0] != left[1] {
                return Outcome::Skip(STACK);
            }
            match attempt(c, number, Fill::Zero, &mut left[2]) {
                Outcome::Different(_) if left[2] == left[0] => Outcome::Different(a),
                _ => Outcome::Skip(STACK),
            }
        }
    }
}

/// A digest of what the port left **where the original left a defined value**: the
/// result, the stream's position, the frees, the sizes of the heap blocks, and the bytes
/// of every heap block and buffer at the positions whose word has a value in the vector
/// (the residue bytes of [`residue`] excepted). Two runs of a port on the two fill
/// patterns have the same digest unless a value the original defines hangs, in the port,
/// on memory nobody wrote.
fn digest(m: &Mem, c: &Case, ret: Ret, buffers: &[(&Field, u32, Vec<W>)], pos: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Ret::Eax(v) = ret {
        v.hash(&mut h);
    }
    pos.hash(&mut h);
    m.freed().hash(&mut h);
    let defined = |h: &mut std::collections::hash_map::DefaultHasher, addr: u32, size: u32, expect: &[W], skip: &[(u32, u32)]| {
        let mut at = addr;
        for w in expect {
            let n = w.bytes();
            let off = at - addr;
            if !matches!(w, W::Unwritten { .. }) && off + n <= size && !skip.iter().any(|&(o, k)| off < o + k && o < off + n) {
                m.bytes(at, n as u32).hash(h);
            }
            at += n;
        }
    };
    for (i, &(addr, size)) in m.heap_blocks().iter().enumerate() {
        size.hash(&mut h);
        if let Some(expect) = c.try_output(&format!("heap{i}")).and_then(words) {
            defined(&mut h, addr, size, &expect, residue(size));
        }
    }
    for (f, addr, input) in buffers {
        let expect = c.try_output(&f.name).and_then(words).unwrap_or_else(|| input.clone());
        defined(&mut h, *addr, si_mem::words_len(input), &expect, &[]);
    }
    h.finish()
}

fn attempt(c: &Case, _number: u32, fill: Fill, left: &mut u64) -> Outcome {
    if c.stopped() && std::env::var_os("EMUCHECK_STOPPED").is_none() {
        return Outcome::Skip("the original stopped");
    }
    if c.notes.iter().any(|n| n.starts_with("GW:") && n != SECURE_LIC_ALLOC) {
        return Outcome::Different(format!("the original wrote a global the case does not declare: {:?}", c.notes));
    }
    // the harness's layout with the heap the case was generated with (`v_heapsize`; see
    // `re/tools/si-hrc/hrclib.py`), so that a request the emulated original was refused is
    // refused here
    let heap = c.try_input("v_heapsize").and_then(|f| f.words().first().and_then(Word::value)).map_or(si_mem::HEAP.1, |v| v as u32);
    let mut m = Mem::with_map(si_mem::Map { heap: (si_mem::HEAP.0, heap), ..si_mem::Map::default() });
    m.set_fill(fill);
    si_hrc::image::init(&mut m);

    // the buffers: globals at their addresses, the others in the arena in the case's order
    let mut names = Names::new();
    let mut buffers: Vec<(&Field, u32, Vec<W>)> = Vec::new();
    for f in c.inputs.iter().filter(|f| is_buffer(&f.name)) {
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

    // the files: the stream argument first (the harness opens it before the call), then
    // the case's file system
    let mut files = Files::new();
    let mut file = 0;
    if let Some(f) = c.try_input("file") {
        file = files.open_bytes(b"file", f.to_bytes());
    }
    for f in c.inputs.iter().filter(|f| f.name.starts_with("file:")) {
        files.add(f.name[5..].as_bytes(), f.to_bytes());
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names, file };
    let mut outside = Recorded { case: c, names: &names, files, next: 0, wrong: None, asked: 0 };
    let routine = c.name.split('#').next().unwrap();
    // The port runs with the image stopping at the first fault, as the original is killed
    // at its first wild access: a loop whose count came from memory nobody wrote would
    // otherwise run on over zeros.
    m.on_fault(si_mem::OnFault::Stop);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| table::run(routine, &mut m, &mut outside, &args)));
    m.on_fault(si_mem::OnFault::Record);
    let ret = match ran {
        Ok(None) => return Outcome::Skip("no port in the table"),
        Ok(Some(Ret::Skip(why))) => return Outcome::Skip(why),
        Ok(Some(ret)) => ret,
        Err(p) => {
            return match p.downcast_ref::<si_mem::Stop>() {
                Some(s) => {
                    // where it stopped is what this run "left": a fault at an address
                    // taken from the fill differs between the two runs
                    *left = ((s.0.addr as u64) << 32) | s.0.size as u64 | 1 << 63;
                    Outcome::Different(format!("the port stopped at a fault: {:?}", s.0))
                }
                None => {
                    let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                    Outcome::Different(format!("the port panicked: {}", text.unwrap_or_default()))
                }
            };
        }
    };
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    *left = digest(&m, c, ret, &buffers, if file != 0 { outside.files.stream(file).map_or(0, |s| s.pos) } else { 0 }) ^ outside.asked;
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if outside.next != c.calls.len() {
        return Outcome::Different(format!("the original made {} outside calls, the port {}", c.calls.len(), outside.next));
    }

    // the mutation switch: one bit of the result, of the first byte the port changed and
    // of the stream's position
    let mut ret = ret;
    let mut pos_flip = 0;
    if mutate() {
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        pos_flip = 1;
        let changed = m.changed_in(&before, Region::Arena).into_iter().chain(m.changed_in(&before, Region::Bss)).chain(m.changed_in(&before, Region::Data)).next();
        if let Some((a, bytes)) = changed {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().iter().find(|b| b.1 > 0) {
            let _ = n;
            let b = m.u8(a);
            m.set_u8(a, b ^ 1);
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ptr", u32::MAX), ("ax", 0xffff), ("al", 0xff)] {
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

    // the stream
    if file != 0 {
        let s = outside.files.stream(file).expect("the stream argument");
        if let Some(f) = c.try_output("file.pos") {
            let want = f.words().first().and_then(Word::value).unwrap_or(0) as u32;
            if want != s.pos as u32 ^ pos_flip {
                return Outcome::Different(format!("the stream stands at {want:#x} in the original, at {:#x} in the port", s.pos));
            }
        }
        if let Some(f) = c.try_output("file.state") {
            let want = f.words().first().and_then(Word::value).unwrap_or(0) as u8;
            if want != s.state() {
                return Outcome::Different(format!("the stream's state is {want} in the original, {} in the port", s.state()));
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
            let want = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
            let sizes: Vec<u32> = (0..want).map(|i| c.try_output(&format!("heap{i}")).and_then(words).map_or(0, |w| si_mem::words_len(&w))).collect();
            let got: Vec<u32> = m.heap_blocks().iter().map(|b| b.1).collect();
            let at = sizes.iter().zip(&got).position(|(a, b)| a != b).unwrap_or(sizes.len().min(got.len()));
            let lo = at.saturating_sub(3);
            return Outcome::Different(format!("buffer `{}`: {e} (blocks: original {}, port {}; sizes part at {at}: original {:?}, port {:?})", f.name, sizes.len(), got.len(), &sizes[lo..(at + 3).min(sizes.len())], &got[lo..(at + 3).min(got.len())]));
        }
    }

    // the heap
    let blocks: Vec<(u32, u32)> = m.heap_blocks().to_vec();
    let want_blocks = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
    if want_blocks != blocks.len() {
        // where the two lists of block sizes part
        let want: Vec<u32> = (0..want_blocks).map(|i| c.try_output(&format!("heap{i}")).and_then(words).map_or(0, |w| si_mem::words_len(&w))).collect();
        let got: Vec<u32> = blocks.iter().map(|b| b.1).collect();
        let at = want.iter().zip(&got).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
        let lo = at.saturating_sub(3);
        return Outcome::Different(format!("the original allocated {want_blocks} blocks, the port {}; sizes part at block {at}: original {:?}, port {:?}", blocks.len(), &want[lo..(at + 3).min(want.len())], &got[lo..(at + 3).min(got.len())]));
    }
    for (i, &(addr, size)) in blocks.iter().enumerate() {
        let name = format!("heap{i}");
        let expect = words(c.output(&name)).unwrap_or_default();
        if si_mem::words_len(&expect) != size {
            return Outcome::Different(format!("{name}: {} bytes in the original, {size} in the port", si_mem::words_len(&expect)));
        }
        // the bytes that hold stack residue in the original are taken out of the comparison
        // where they differ (heap blocks are dumped as bytes: a word is a byte)
        let mut expect = expect;
        let mut had_residue = false;
        for &(off, n) in residue(size) {
            for k in off..off + n {
                if let Some(W::Value { bytes: 1, value }) = expect.get(k as usize) {
                    if *value as u8 != m.u8(addr + k) {
                        expect[k as usize] = W::Value { bytes: 1, value: m.u8(addr + k) as u128 };
                        had_residue = true;
                    }
                }
            }
        }
        if had_residue && matches!(fill, Fill::Pattern { second: false, .. }) {
            RESIDUE.with(|r| r.set(r.get() + 1));
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
            // a run of changed bytes may cross from one buffer into its neighbour; where
            // the original wrote past a buffer of the case (the harness notes it), the gap
            // behind that buffer is the port's to write too, and is not compared
            for k in 0..bytes.len() as u32 {
                let at = a + k;
                let inside = buffers.iter().any(|(f, addr, w)| {
                    let gap = if c.notes.iter().any(|n| n == &format!("OVERRUN:{}", f.name)) { 0x400 } else { 0 };
                    at >= *addr && at < *addr + si_mem::words_len(w) + gap
                });
                if !inside {
                    return Outcome::Different(format!("the port changed the byte at {at:#010x}, outside every buffer of the case"));
                }
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
    let dir = std::env::args().nth(1).expect("usage: hrc-emucheck <directory of vector files> [name-substring]");
    let only = std::env::args().nth(2);
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("the directory").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "vec")).collect();
    paths.sort();
    std::panic::set_hook(Box::new(|_| {}));
    println!("{:<40} {:>6} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "defined", "different", "not-comp");
    let mut total = Row::default();
    let mut all_reasons: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut files = 0;
    for path in &paths {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if only.as_ref().is_some_and(|o| !stem.contains(o.as_str())) {
            continue;
        }
        files += 1;
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
                    if std::env::var_os("EMUCHECK_VERBOSE").is_some() {
                        println!("    line {}: not compared, {why}", number + 1);
                    }
                    row.skipped += 1;
                    *row.reasons.entry(why).or_default() += 1;
                }
                Outcome::Different(d) => {
                    row.different += 1;
                    if row.examples.len() < 2 {
                        let mut text = line.clone();
                        if text.len() > 700 {
                            let mut cut = 700;
                            while !text.is_char_boundary(cut) {
                                cut -= 1;
                            }
                            text.truncate(cut);
                            text.push('…');
                        }
                        row.examples.push(format!("line {}: {d}\n      {text}", number + 1));
                    }
                }
            }
        }
        println!("{:<40} {:>6} {:>9} {:>8} {:>9} {:>8}", stem, row.cases, row.identical, row.defined_part, row.different, row.skipped);
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
    println!("{:<40} {:>6} {:>9} {:>8} {:>9} {:>8}", format!("TOTAL ({files} files)"), total.cases, total.identical, total.defined_part, total.different, total.skipped);
    for (why, n) in &all_reasons {
        println!("  not compared, {why}: {n}");
    }
    println!("  residue: in {} of the compared cases a texture field the original copies from stack it never wrote held what earlier callees left there; those bytes are not compared", RESIDUE.with(|r| r.get()));
    std::process::exit(if total.different == 0 { 0 } else { 1 });
}
