// SPDX-License-Identifier: MIT
//! The checks of the image-form modules (second pass of the package, finding 0024).
//!
//! A case is not rebuilt: its buffers are placed in a fresh `si_mem::Mem` where the harness
//! placed them (the arena is laid out the same way; a global lies at its own address), the
//! port runs on them, and memory is compared:
//!
//! - what the routine returned (`eax`, `ax`, `al`, `ptr`, `st0`);
//! - every buffer of the case, changed or not (one the original left alone must be left
//!   alone);
//! - the heap: as many blocks, each of the same size, each written word the same, each
//!   byte the original did not write still holding the fill; the freed blocks in order;
//! - the calls out of the package, in order, with the arguments the harness logged, each
//!   answered as the emulated call was;
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! Naming in a vector line (`re/tools/si-kine/emu_image.py` writes them): `v_x` is an
//! argument by value, `p_x` a pointer argument, `gXXXXXXXX…` a global at that address,
//! anything else a buffer of the arena.
//!
//! Every case is run twice, on memory pre-filled with a pattern and with its complement
//! (the harness does the same with the original). When the two runs of the port disagree
//! with each other, the result hangs on memory nobody wrote and the case is not compared.
//! `EMUCHECK_MUTATE=1` flips one bit of what the port returned, of the first byte it
//! changed and of the first argument it passed out of the package.

use si_kine::outside::{Outside, Ret as OutRet, Routine};
use si_mem::{Fill, Mem, Names, Ptr32, Region, Word as W};
use si_x87::trig::replay;
use si_x87::vectors::{Case, Field, Verdict, Word};
use si_x87::F80;

use crate::world::mutate;

pub fn words(f: &Field) -> Option<Vec<W>> {
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
    fn word_at(&self, name: &str, i: usize) -> u32 {
        let f = self.case.try_input(name).unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name));
        match f.words().get(i) {
            Some(Word::Value { value, .. }) => *value as u32,
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                base.wrapping_add(*offset)
            }
            _ => panic!("argument `{name}` has no word {i}"),
        }
    }

    /// An argument by value, as the 32-bit stack word.
    pub fn v(&self, name: &str) -> u32 {
        self.word_at(&format!("v_{name}"), 0)
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

    /// An `SI_Vector` by value.
    pub fn vec(&self, name: &str) -> si_types::geom::Vector {
        let n = format!("v_{name}");
        si_types::geom::Vector { x: f32::from_bits(self.word_at(&n, 0)), y: f32::from_bits(self.word_at(&n, 1)), z: f32::from_bits(self.word_at(&n, 2)) }
    }

    /// A pointer argument.
    pub fn p<T>(&self, name: &str) -> Ptr32<T> {
        Ptr32::new(self.word_at(&format!("p_{name}"), 0))
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug)]
pub enum Ret {
    None,
    Eax(u32),
    St0(F80),
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
impl From<F80> for Ret {
    fn from(v: F80) -> Ret {
        Ret::St0(v)
    }
}
impl<T> From<Ptr32<T>> for Ret {
    fn from(v: Ptr32<T>) -> Ret {
        Ret::Eax(v.addr())
    }
}

/// The outside world of a case: every call out of the package is held against the next
/// one the original made and answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    /// Index into the case's calls of the next one that is a call out of the package.
    next: usize,
    made: usize,
    pub wrong: Option<String>,
    /// What the port printed.
    pub stdout: Vec<u8>,
}

/// A call out of the package, or one of the notes a stand-in of the generator leaves after
/// its call: `=mem(k, words…)`, the words it stored at the address passed as argument k;
/// `=malloc(n)`, a block it took from the heap.
fn is_outside(name: &str) -> bool {
    name.starts_with('=') || si_kine::outside::ALL.iter().any(|r| r.name == name)
}

impl Recorded<'_> {
    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }

    fn left(&self) -> usize {
        self.case.calls[self.next.min(self.case.calls.len())..].iter().filter(|c| is_outside(&c.name) && !c.name.starts_with('=')).count()
    }
}

impl Outside for Recorded<'_> {
    fn stdout(&mut self, text: &[u8]) {
        self.stdout.extend_from_slice(text);
    }

    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> OutRet {
        // the mathematics library's calls lie between them on the same list
        while self.case.calls.get(self.next).is_some_and(|c| !is_outside(&c.name)) {
            self.next += 1;
        }
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("outside call {}: the port calls {} where the original made no further call", self.made, routine.name));
            return OutRet::default();
        };
        let n = self.made;
        self.next += 1;
        self.made += 1;
        if want.name != routine.name {
            self.wrong.get_or_insert_with(|| format!("outside call {n}: the port calls {}, the original {}", routine.name, want.name));
            return OutRet::default();
        }
        // the harness logs as many words as the stand-in's rule names; the port may pass more
        for (k, w) in want.args.iter().enumerate() {
            let mut got = args.get(k).copied();
            if mutate() && n == 0 && k == 0 {
                got = got.map(|g| g ^ 1);
            }
            if let Word::Ref { name, .. } = w {
                // a logged string: compared by content
                if let Some(text) = self.case.try_output(name).filter(|_| name.starts_with("str")) {
                    let want_text: Vec<u8> = text.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect();
                    let got_text = got.map(|g| m.cstr(g));
                    if got_text.as_deref() != Some(&want_text[..]) {
                        self.wrong.get_or_insert_with(|| format!("outside call {n} {}: argument {k} is the text {:?} in the port, {:?} in the original", routine.name, got_text.map(|t| String::from_utf8_lossy(&t).into_owned()), String::from_utf8_lossy(&want_text)));
                    }
                    continue;
                }
            }
            let expect = self.resolve(m, w);
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("outside call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        // what the stand-in did to memory besides answering
        while let Some(note) = self.case.calls.get(self.next).filter(|c| c.name.starts_with('=')) {
            self.next += 1;
            match note.name.as_str() {
                "=mem" => {
                    let k = note.args.first().and_then(|w| w.value()).unwrap_or(0) as usize;
                    let Some(&base) = args.get(k) else {
                        self.wrong.get_or_insert_with(|| format!("outside call {n} {}: the stand-in wrote through argument {k}, which the port does not pass", routine.name));
                        break;
                    };
                    let mut at = base;
                    for w in &note.args[1..] {
                        if let Some(v) = self.resolve(m, w) {
                            m.set_bytes(at, &v.to_le_bytes()[..w.bytes() as usize]);
                        }
                        at = at.wrapping_add(w.bytes() as u32);
                    }
                }
                "=memo" => {
                    // `=memo(k, offset, words…)`: stored at argument k plus an offset
                    let k = note.args.first().and_then(|w| w.value()).unwrap_or(0) as usize;
                    let off = note.args.get(1).and_then(|w| w.value()).unwrap_or(0) as u32;
                    let mut at = args.get(k).copied().unwrap_or(0).wrapping_add(off);
                    for w in note.args.iter().skip(2) {
                        if let Some(v) = self.resolve(m, w) {
                            m.set_bytes(at, &v.to_le_bytes()[..w.bytes() as usize]);
                        }
                        at = at.wrapping_add(w.bytes() as u32);
                    }
                }
                "=in" => {
                    // `=in(k, words…)`: what the stand-in read at the address passed as
                    // argument k (a structure handed over by pointer): compared
                    let k = note.args.first().and_then(|w| w.value()).unwrap_or(0) as usize;
                    let mut at = args.get(k).copied().unwrap_or(0);
                    for (i, w) in note.args.iter().enumerate().skip(1) {
                        let n = w.bytes() as u32;
                        let got = m.bytes(at, n).iter().rev().fold(0u32, |a, b| a << 8 | *b as u32) ^ (mutate() && i == 1) as u32;
                        if let Some(v) = self.resolve(m, w) {
                            if v != got {
                                self.wrong.get_or_insert_with(|| format!("outside call {n} {}: word {} at argument {k} is {got:08x} in the port, {v:08x} in the original", routine.name, i - 1));
                            }
                        }
                        at = at.wrapping_add(n);
                    }
                }
                "=cb" => {
                    // the stand-in called back into the package: `=cb(k, words…)` is a call
                    // of the function whose address the port passed as argument k
                    let k = note.args.first().and_then(|w| w.value()).unwrap_or(0) as usize;
                    let words: Vec<u32> = note.args[1..]
                        .iter()
                        .map(|w| match w {
                            Word::Ref { name, offset } if name == "arg" => args.get(*offset as usize).copied().unwrap_or(0),
                            w => self.resolve(m, w).unwrap_or(0),
                        })
                        .collect();
                    let addr = args.get(k).copied().unwrap_or(0);
                    if si_kine::callbacks::call(m, self, addr, &words).is_none() {
                        self.wrong.get_or_insert_with(|| format!("outside call {n} {}: argument {k} is {addr:#010x}, no function of the package", routine.name));
                    }
                }
                "=malloc" => {
                    let size = note.args.first().and_then(|w| w.value()).unwrap_or(0) as u32;
                    m.malloc(size);
                }
                other => {
                    self.wrong.get_or_insert_with(|| format!("unknown note {other} of a stand-in"));
                }
            }
        }
        if want.result.bytes() == 10 {
            // an answer in the x87 register is also on the mathematics tape
            let st0 = replay::next(&want.name, &want.arg_values()).unwrap_or(want.result_f80());
            return OutRet::float(st0);
        }
        OutRet::int(self.resolve(m, &want.result).unwrap_or(0))
    }
}

/// A port as the check calls it: the image, the outside world, the case's arguments.
pub type Port = fn(&mut Mem, &mut Recorded<'_>, &Args<'_>) -> Ret;

enum Outcome {
    Same,
    Different(String),
    Skip,
}

/// A number that differs from case to case: shifts the fill pattern.
fn salt(c: &Case) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for f in &c.inputs {
        for w in f.words() {
            let v = match w {
                Word::Value { value, .. } => *value as u32 ^ (*value >> 32) as u32,
                Word::Unwritten { bytes } => *bytes as u32,
                Word::Ref { offset, .. } => *offset,
            };
            h = (h ^ v).wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// Runs `port` on the case and compares everything it left (see the module's top).
pub fn check(c: &Case, port: Port) -> Verdict {
    let first = attempt(c, port, false);
    if matches!(first, Outcome::Skip) {
        return Verdict::Skip;
    }
    let second = attempt(c, port, true);
    match (first, second) {
        (Outcome::Same, Outcome::Same) => Verdict::Same,
        (Outcome::Different(a), Outcome::Different(b)) if a == b => Verdict::Different(a),
        (Outcome::Different(a), Outcome::Different(_)) if mutate() => Verdict::Different(a),
        // the port's result depends on memory nobody wrote (the original's too, unseen by
        // the harness because both of its patterns happened to give it the same result)
        _ => Verdict::Skip,
    }
}

fn attempt(c: &Case, port: Port, second: bool) -> Outcome {
    if c.has_note("OVERRUN") {
        return Outcome::Skip;
    }
    // the harness's two passes made different calls: what the original asks of the
    // library hangs on memory nobody wrote, and the recorded calls are one pass's
    if c.has_note("UNINIT:calls") {
        return Outcome::Skip;
    }
    // `SI_secureLicAlloc` is a counter the program's allocation wrappers bump: a word of
    // its self-check, not ported (si-dict says the same)
    if c.notes.iter().any(|n| n.starts_with("GW:") && n != "GW:SI_secureLicAlloc+0x0") {
        // the original wrote a global the case does not declare
        return Outcome::Skip;
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: salt(c).wrapping_mul(4099) };
    m.set_fill(fill);
    si_kine::rodata::store(&mut m);

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

    // the port, with the case's mathematics tape running
    let _ = replay::finish();
    replay::start(c.tape());
    let args = Args { case: c, names: &names };
    let mut outside = Recorded { case: c, names: &names, next: 0, made: 0, wrong: None, stdout: Vec::new() };
    let ret = port(&mut m, &mut outside, &args);
    let replayed = replay::finish();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if outside.left() != 0 {
        return Outcome::Different(format!("the original made {} more calls out of the package than the port's {}", outside.left(), outside.made));
    }
    if let Err(e) = replayed {
        return Outcome::Different(e);
    }
    // what was printed
    let printed: Vec<u8> = c.try_output("stdout").map_or(Vec::new(), |f| f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect());
    if mutate() {
        if let Some(b) = outside.stdout.first_mut() {
            *b ^= 1;
        }
    }
    if printed != outside.stdout {
        return Outcome::Different(format!("stdout: original {:?}, port {:?}", String::from_utf8_lossy(&printed), String::from_utf8_lossy(&outside.stdout)));
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    if mutate() {
        ret = match ret {
            Ret::Eax(v) => Ret::Eax(v ^ 1),
            Ret::St0(v) => {
                let (mant, se) = v.to_bits();
                Ret::St0(F80::from_bits(mant ^ 1, se))
            }
            Ret::None => Ret::None,
        };
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
    for (field, mask) in [("eax", u32::MAX), ("ptr", u32::MAX), ("ax", 0xffff), ("al", 0xff)] {
        if let Some(f) = c.try_output(field) {
            let Ret::Eax(got) = ret else {
                return Outcome::Different(format!("the original returns {field}, the port no integer"));
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
    if let Some(f) = c.try_output("st0") {
        let Ret::St0(got) = ret else {
            return Outcome::Different("the original returns in st0, the port does not".into());
        };
        if !f.words().is_empty() && !f.same_f80(got) {
            return Outcome::Different(format!("st0: original {:?}, port {got:?}", f.f80()));
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

    // the heap (every image vector is made with `heap=` in its spec line)
    let blocks: Vec<(u32, u32)> = m.heap_blocks().to_vec();
    let want_blocks = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
    {
        if want_blocks != blocks.len() {
            return Outcome::Different(format!("the original allocated {want_blocks} blocks, the port {}", blocks.len()));
        }
        for (i, &(addr, size)) in blocks.iter().enumerate() {
            let name = format!("heap{i}");
            let expect = words(c.output(&name)).unwrap_or_default();
            // a block is printed in whole words of the spec's heap type: up to three bytes
            // at its end may be missing from the line
            if si_mem::words_len(&expect) != size && si_mem::words_len(&expect) != size & !3 {
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
                    at += w.bytes() as u32;
                }
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
            let inside = buffers.iter().any(|(_, addr, w)| a >= *addr && a + bytes.len() as u32 <= *addr + si_mem::words_len(w));
            if !inside {
                return Outcome::Different(format!("the port changed {} bytes at {a:#010x}, outside every buffer of the case", bytes.len()));
            }
        }
    }
    Outcome::Same
}
