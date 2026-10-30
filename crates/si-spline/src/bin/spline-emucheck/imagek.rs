// SPDX-License-Identifier: MIT
//! Checks of the image forms (finding 0024; `si_spline::spline_image`, `fit_image` and the
//! modules of the third pass beside them).
//!
//! A case is not rebuilt in structures of the check's own. Its buffers go into `si-mem`'s
//! image as they are (`load_case_at`: arena buffers as the harness lays them out, a buffer
//! named `g<address>` at that address, the three globals of the earlier passes' vectors by
//! their names), the port runs on that memory, and then
//!
//! 1. every call the original made out of the package must have been made by the port, in
//!    order and with the same arguments (a pointer compared as "this buffer at this
//!    offset"); the port gets the recorded answer;
//! 2. every buffer the vector lists as an output is held against memory word for word
//!    wherever the original wrote, and **where the original did not write the port must
//!    not have written either** (the word still holds the fill); so is every heap block,
//!    by its number in the order of allocation, and the number of blocks (`nheap`);
//! 3. the result (`eax`, `ax`, a pointer, the x87 register) and the blocks freed, in
//!    order, must be the original's;
//! 4. the port must not have faulted (a case in which the original did is not compared).
//!
//! Each case is run twice, on zeroed and on patterned fresh memory: a word the original
//! stored that the port left alone is found by the second run when its value is zero, by
//! the first otherwise. `EMUCHECK_MUTATE=1` flips one bit of every run of bytes the port
//! changed in the image, of its result and of the first argument of every call it makes,
//! before the comparison; `EMUCHECK_MUTATE=calls` flips only the first argument of every call
//! (a routine that makes no recorded call then still agrees).

use std::panic::{catch_unwind, AssertUnwindSafe};

use si_mem::{FieldAt, Fill, Mem, Names, OnFault, Word as W};
use si_spline::spline_image::{self, Callee, Outside};
use si_x87::vectors::{Case, Field, Verdict, Word};
use si_x87::F80;

use crate::util::{mutate, mutate_calls};

/// What a port returned.
#[allow(dead_code)]
pub enum Ret {
    None,
    Int(u32),
    St0(F80),
}

/// The address a field lies at when the vector fixes it.
fn fixed(name: &str) -> Option<u32> {
    match name {
        "gl" => Some(0x0867_9498),
        "gi" => Some(0x0878_896c),
        "gi2" => Some(0x0878_8970),
        _ => name.strip_prefix('g').filter(|h| h.len() == 8).and_then(|h| u32::from_str_radix(h, 16).ok()),
    }
}

fn convert_word(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn convert(f: &Field) -> Option<Vec<W>> {
    f.words.as_ref().map(|ws| ws.iter().map(convert_word).collect())
}

/// The arguments of a case: where its buffers lie, and its values.
pub struct Args<'a> {
    at: Vec<(String, u32)>,
    pub c: &'a Case,
}

#[allow(dead_code)]
impl Args<'_> {
    /// A pointer argument: the address of the buffer of that name; 0 for a null one.
    pub fn p(&self, name: &str) -> u32 {
        match self.at.iter().find(|b| b.0 == name) {
            Some(b) => b.1,
            None => panic!("the case has no buffer `{name}`"),
        }
    }

    /// An integer argument by value, as the word pushed (a narrow one sign-extended).
    pub fn w(&self, name: &str) -> u32 {
        match &self.c.input(name).words()[0] {
            Word::Value { bytes, value } => match bytes {
                1 => *value as u8 as i8 as i32 as u32,
                2 => *value as u16 as i16 as i32 as u32,
                _ => *value as u32,
            },
            Word::Ref { name, offset } => self.p(name) + offset,
            Word::Unwritten { .. } => 0,
        }
    }

    pub fn i(&self, name: &str) -> i32 {
        self.w(name) as i32
    }

    pub fn f32(&self, name: &str) -> f32 {
        self.c.input(name).f32()
    }

    pub fn f64(&self, name: &str) -> f64 {
        self.c.input(name).f64()
    }

    /// An `SI_DblPnt` passed by value.
    pub fn pnt(&self, name: &str) -> [f64; 3] {
        let f = self.c.input(name).f64s();
        [f[0], f[1], f[2]]
    }
}

/// The C library's mathematics, which `si_x87::trig` replays by itself.
const LIBM: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "log", "log10", "pow", "cbrt", "sinf", "cosf", "tanf", "asinf", "acosf", "atanf", "expf", "powf", "fpatan", "sqrt"];

/// The C library's own routines a `record` rule may have logged; the port makes them on
/// the image.
const LIBC: &[&str] = &["drand48"];

/// The outside world of a case: the recorded calls, in order.
pub struct Replay<'a> {
    c: &'a Case,
    names: &'a Names,
    at: usize,
    pub wrong: Option<String>,
}

impl<'a> Replay<'a> {
    fn skip(&mut self) {
        while self.at < self.c.calls.len() && (LIBM.contains(&self.c.calls[self.at].name.as_str()) || LIBC.contains(&self.c.calls[self.at].name.as_str())) {
            self.at += 1;
        }
    }

    fn fail(&mut self, text: String) {
        if self.wrong.is_none() {
            self.wrong = Some(text);
        }
    }

    fn finished(&mut self) -> bool {
        self.skip();
        self.at == self.c.calls.len()
    }
}

fn in_stack(a: u32) -> bool {
    (0x7fd0_0000..0x7ff0_0000).contains(&a)
}

impl Replay<'_> {
    /// The next recorded call, held against the port's: the same callee and arguments.
    fn step(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> Option<Word> {
        self.skip();
        let Some(rec) = self.c.calls.get(self.at) else {
            self.fail(format!("call {}: the port calls {}{args:08x?}, the original nothing more", self.at, callee.name));
            return None;
        };
        self.at += 1;
        if rec.name != callee.name {
            self.fail(format!("call {}: the port calls {}, the original {}", self.at - 1, callee.name, rec.name));
            return None;
        }
        // the first pass's vectors recorded two of the three words SI_invertFcurvePathKeys
        // is called with (the third, an `SI_NurbsDir *`, is 0 for a spline)
        let args = if callee.name == "SI_invertFcurvePathKeys" && rec.args.len() == 2 && args.len() == 3 { &args[..2] } else { args };
        if rec.args.len() != args.len() {
            self.fail(format!("{}: {} arguments against the port's {}", callee.name, rec.args.len(), args.len()));
            return None;
        }
        for (k, (want, got)) in rec.args.iter().zip(args).enumerate() {
            let got = *got ^ (mutate_calls() && k == 0) as u32;
            let same = match want {
                Word::Value { value, .. } => *value as u32 == got || (in_stack(*value as u32) && in_stack(got)),
                Word::Unwritten { .. } => true,
                Word::Ref { name, offset } => match self.names.addr(m, name) {
                    Some(a) => a.wrapping_add(*offset) == got,
                    None => match self.c.try_output(name) {
                        Some(text) => text.to_bytes() == m.cstr(got),
                        None => false,
                    },
                },
            };
            if !same {
                let where_ = self.names.name_of(m, got).map_or(format!("{got:08x}"), |(n, o)| format!("&{n}+{o:#x}"));
                self.fail(format!("call {} {}: argument {k} is {want:?}, the port's {where_}", self.at - 1, callee.name));
            }
        }
        Some(rec.result.clone())
    }
}

impl Outside for Replay<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        let Some(result) = self.step(m, callee, args) else {
            return 0;
        };
        // the stand-in of SI_waveGetIconModel answers the first word of the record it is
        // given (a pointer to a buffer of the case, which lies elsewhere in the emulator)
        if callee.name == "SI_waveGetIconModel" {
            return m.u32(args[0]);
        }
        // and that of SI_indexModifAllocate(model, kind) the word at model + 4 * kind
        if callee.name == "SI_indexModifAllocate" {
            return m.u32(args[0].wrapping_add(args[1].wrapping_mul(4)));
        }
        match &result {
            Word::Value { value, .. } => *value as u32,
            Word::Ref { name, offset } => self.names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
            Word::Unwritten { .. } => 0,
        }
    }

    fn call_float(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> F80 {
        let recorded = self.step(m, callee, args);
        // a call that answered in the register is on the library's tape as well: taken
        // from there too, so that the tape ends where the port's calls end
        let words: Vec<u128> = args.iter().map(|a| *a as u128).collect();
        let taped = si_x87::trig::replay::next(callee.name, &words);
        match recorded {
            Some(Word::Value { bytes: 10, value }) => taped.unwrap_or(F80::from_bits(value as u64, (value >> 64) as u16)),
            Some(other) => {
                self.fail(format!("{}: the original's answer {other:?} is not a number of the x87 register", callee.name));
                F80::ZERO
            }
            None => F80::ZERO,
        }
    }
}

/// A buffer's words against memory: where the original wrote, the same value; where it
/// did not (and the case has no undefined words), the fill.
fn same_words(m: &Mem, names: &Names, name: &str, addr: u32, f: &Field, strict: bool) -> Verdict {
    let words = convert(f).unwrap_or_default();
    if let Err(e) = m.compare_words(addr, &words, names) {
        return Verdict::Different(format!("{name}: {e}"));
    }
    if strict {
        let fill = m.fill();
        let mut at = addr;
        for (i, w) in words.iter().enumerate() {
            let n = w.bytes();
            if matches!(w, W::Unwritten { .. }) {
                let held = m.try_bytes(at, n).map(|b| b.to_vec());
                let fresh: Vec<u8> = (0..n).map(|k| fill.byte(at + k)).collect();
                if held.as_deref() != Some(&fresh[..]) {
                    return Verdict::Different(format!("{name}: word {i} (+{:#x}) was not written by the original; the port wrote {held:02x?}", at - addr));
                }
            }
            at = at.wrapping_add(n);
        }
    }
    Verdict::Same
}

/// Output fields a generator's `post` hook made, which a check compares itself.
pub type Extra = fn(&Mem, &Names, &Case, &Field) -> Option<Verdict>;

fn compare(m: &Mem, names: &Names, c: &Case, ret: &Ret, extra: Option<Extra>) -> Verdict {
    // an output the harness marks `UNINIT:name` depends on memory nobody initialised: its
    // bytes are whatever the two emulator runs happened to agree on, and it is left out;
    // every other output of the case is defined and compared as usual
    let undefined: Vec<&str> = c.notes.iter().filter_map(|n| n.strip_prefix("UNINIT:")).collect();
    let strict = true;
    for f in &c.outputs {
        let name = f.name.as_str();
        if undefined.contains(&name) {
            continue;
        }
        let int = |bits: u32| -> Verdict {
            let Ret::Int(v) = ret else {
                return Verdict::Different(format!("{name}: the port returns nothing"));
            };
            let got = (*v ^ mutate() as u32) & (u32::MAX >> (32 - bits));
            match &f.words()[0] {
                Word::Value { value, .. } => Verdict::from(*value as u32 == got, || format!("{name}: {value:x} against the port's {got:x}")),
                Word::Ref { name: b, offset } => {
                    let want = names.addr(m, b).map(|a| a + offset);
                    Verdict::from(want == Some(got), || format!("{name}: &{b}+{offset:#x} against the port's {got:08x}"))
                }
                Word::Unwritten { .. } => Verdict::Same,
            }
        };
        let v = match name {
            "eax" | "ptr" => int(32),
            "ax" => int(16),
            "al" => int(8),
            "st0" => match ret {
                Ret::St0(v) => {
                    let v = if mutate() { F80::from_bits(v.to_bits().0 ^ 1, v.to_bits().1) } else { *v };
                    Verdict::from(f.same_f80(v), || format!("st0: {:?} against the port's {v:?}", f.words()))
                }
                _ => Verdict::from(f.words().is_empty(), || "st0: the port returns nothing".to_string()),
            },
            "nheap" => {
                let want = f.u32() as usize;
                Verdict::from(want == m.heap_blocks().len(), || format!("{want} heap blocks against the port's {}", m.heap_blocks().len()))
            }
            "freed" => {
                let got: Vec<u32> = m.freed().to_vec();
                let want: Vec<Option<u32>> = f
                    .words()
                    .iter()
                    .map(|w| match w {
                        Word::Ref { name, offset } => names.addr(m, name).map(|a| a + offset),
                        Word::Value { value, .. } => Some(*value as u32),
                        Word::Unwritten { .. } => None,
                    })
                    .collect();
                Verdict::from(want.len() == got.len() && want.iter().zip(&got).all(|(a, b)| *a == Some(*b)), || format!("freed: {:?} against the port's {got:08x?}", f.words()))
            }
            _ if name.starts_with("str") => Verdict::Same,
            _ => match names.addr(m, name) {
                Some(0) | None if name.starts_with("heap") => Verdict::Different(format!("the port has no block {name}")),
                Some(0) => Verdict::Same,
                Some(a) => same_words(m, names, name, a, f, strict),
                None => match extra.and_then(|e| e(m, names, c, f)) {
                    Some(v) => v,
                    None => Verdict::Different(format!("the check does not know the output `{name}`")),
                },
            },
        };
        if v != Verdict::Same {
            return v;
        }
    }
    if !c.outputs.iter().any(|f| f.name == "freed") && !m.freed().is_empty() {
        return Verdict::Different(format!("the port frees {} blocks, the original none", m.freed().len()));
    }
    Verdict::Same
}

/// Runs a case through an image form and compares. `bufs` names the case's buffers
/// (pointer arguments, side buffers, globals) in the order of the spec line; one that is
/// only an output is placed unfilled.
pub fn check(c: &Case, bufs: &[&str], extra: Option<Extra>, port: &dyn Fn(&mut Mem, &mut Replay, &Args) -> Ret) -> Verdict {
    if c.has_note("UNINIT:calls") {
        // the calls the original made depend on memory nobody initialised
        return Verdict::Skip;
    }
    match pass(c, bufs, extra, port, false) {
        Verdict::Same if !mutate() => {
            let tape = c.tape();
            if !tape.is_empty() {
                if let Err(e) = si_x87::trig::replay::finish() {
                    return Verdict::Different(e);
                }
                si_x87::trig::replay::start(tape);
            }
            pass(c, bufs, extra, port, true)
        }
        other => other,
    }
}

fn pass(c: &Case, bufs: &[&str], extra: Option<Extra>, port: &dyn Fn(&mut Mem, &mut Replay, &Args) -> Ret, pattern: bool) -> Verdict {
    let mut m = Mem::new();
    if pattern {
        m.set_fill(Fill::Pattern { second: false, salt: 0x51 });
    }
    spline_image::init(&mut m);
    let mut owned: Vec<(&str, Option<Vec<W>>)> = Vec::new();
    for name in bufs {
        if let Some(f) = c.try_input(name) {
            owned.push((name, convert(f)));
        } else if let Some(f) = c.try_output(name) {
            // only written: placed, and left holding the fill
            owned.push((name, Some(f.words().iter().map(|w| W::Unwritten { bytes: w.bytes() }).collect())));
        }
    }
    let fields: Vec<FieldAt<'_>> = owned.iter().map(|(n, w)| (*n, fixed(n), w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Verdict::Different(e),
    };
    let before = m.snapshot();
    let args = Args { at: names.iter().map(|(n, a, _)| (n.to_string(), a)).collect(), c };
    let mut o = Replay { c, names: &names, at: 0, wrong: None };
    m.on_fault(OnFault::Stop);
    let ret = catch_unwind(AssertUnwindSafe(|| port(&mut m, &mut o, &args)));
    m.on_fault(OnFault::Record);
    let ret = match ret {
        Ok(r) => r,
        Err(p) => {
            return match p.downcast_ref::<si_mem::Stop>() {
                // an adapter's limit (memory outside the arrays a value form holds): not comparable
                Some(stop) if stop.0.addr == si_spline::patch_image::ADAPTER_LIMIT => Verdict::Skip,
                // a loop the original never leaves (its case has no result either)
                Some(stop) if stop.0.addr == si_spline::merge_image::ENDLESS => Verdict::Skip,
                Some(stop) => Verdict::Different(format!("the port faulted: {:?}", stop.0)),
                None => std::panic::resume_unwind(p),
            };
        }
    };
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = o.wrong.take() {
        return Verdict::Different(w);
    }
    if !o.finished() {
        return Verdict::Different(format!("the original made {} calls, the port {}", c.calls.len(), o.at));
    }
    if mutate() {
        let changed = m.changed(&before);
        if changed.is_empty() && matches!(ret, Ret::None) {
            return Verdict::Skip;
        }
        for (addr, bytes) in changed {
            m.set_u8(addr, bytes[0] ^ 1);
        }
    }
    compare(&m, &names, c, &ret, extra)
}
