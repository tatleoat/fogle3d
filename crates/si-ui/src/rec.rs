// SPDX-License-Identifier: MIT
//! The call log of the recording implementations of [`crate::sys`].
//!
//! Every call `event.c` and `window.c` make outside themselves (IRIS GL, the C library's
//! system services, dialogs, menus, controls, the status line, …) goes through a trait. The
//! recording implementation of each trait appends one line to a shared [`Log`] and answers
//! with values configured beforehand, so a check can compare the exact sequence of outside
//! calls with their arguments against the original routine run in the emulation harness
//! (`re/tools/emu`, driven by `re/tools/si-ui/emu_ui.py`) on the same memory and the same
//! answers.
//!
//! Nothing here reproduces a function of the original; it is the measuring instrument.

use crate::mem::{Mem, P};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::rc::Rc;

/// One argument of a recorded call.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// An `int`, `long`, enum or pointer argument: the 32-bit stack word.
    W(u32),
    /// A `short` argument: the low 16 bits of the stack word (the callee reads no more).
    H(u16),
    /// A `char` argument: the low 8 bits.
    B(u8),
    /// A `float`, by bits.
    F(f32),
    /// A `double`, by bits.
    D(f64),
    /// A string the callee reads (without its terminating zero).
    S(Vec<u8>),
    /// Words the callee reads through a pointer to something that is not in the image (an
    /// array on the caller's stack).
    A(Vec<u32>),
    /// A pointer through which the callee only writes.
    Out,
}

/// `"text"` with everything outside printable ASCII, the quote and the backslash as `\xNN`.
pub fn quote(v: &[u8]) -> String {
    let mut s = String::from("\"");
    for &b in v {
        if (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\' {
            s.push(b as char);
        } else {
            write!(s, "\\x{:02x}", b).unwrap();
        }
    }
    s.push('"');
    s
}

/// One line of the transcript: `  name(0x1, 0xffff, f:3f800000, "text", w:[1,2], out)`.
pub fn call_line(name: &str, args: &[Val]) -> String {
    let mut s = format!("  {name}(");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        match a {
            Val::W(v) => write!(s, "{:#x}", v).unwrap(),
            Val::H(v) => write!(s, "{:#x}", v).unwrap(),
            Val::B(v) => write!(s, "{:#x}", v).unwrap(),
            Val::F(v) => write!(s, "f:{:08x}", v.to_bits()).unwrap(),
            Val::D(v) => write!(s, "d:{:016x}", v.to_bits()).unwrap(),
            Val::S(v) => s.push_str(&quote(v)),
            Val::A(v) => {
                s.push_str("w:[");
                for (j, w) in v.iter().enumerate() {
                    if j > 0 {
                        s.push(',');
                    }
                    write!(s, "{:x}", w).unwrap();
                }
                s.push(']');
            }
            Val::Out => s.push_str("out"),
        }
    }
    s.push(')');
    s
}

/// The configured answer to one call: the return value first, then what the callee writes
/// through its output pointers in argument order; and bytes the callee stores in the image
/// (a menu that fills in the event it was given, say).
#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub vals: Vec<u64>,
    pub pokes: Vec<(u32, Vec<u8>)>,
}

impl Answer {
    /// The `i`-th value, 0 when the answer has none.
    pub fn val(&self, i: usize) -> u64 {
        self.vals.get(i).copied().unwrap_or(0)
    }
}

/// The shared log: the calls made so far and the answers still to be given.
#[derive(Default)]
pub struct Log {
    pub lines: Vec<String>,
    /// For looking into a case: ranges of the image (address, length) whose bytes are
    /// appended to every recorded call (`UI_WATCH` in `ui-emucheck`, `--watch` in
    /// `emu_ui.py`). Empty in a check.
    pub watch: Vec<(u32, u32)>,
    queued: HashMap<String, VecDeque<Answer>>,
    fallback: HashMap<String, Answer>,
}

impl Log {
    /// Queue an answer for the next unanswered call of `name`.
    pub fn answer(&mut self, name: &str, a: Answer) {
        self.queued.entry(name.to_string()).or_default().push_back(a);
    }
    /// The answer for every call of `name` once its queue is empty.
    pub fn always(&mut self, name: &str, a: Answer) {
        self.fallback.insert(name.to_string(), a);
    }
    /// Forget the queued answers (a check does this between two cases).
    pub fn forget_queued(&mut self) {
        self.queued.clear();
    }
    /// Record a call, take its answer and store the answer's bytes in the image.
    pub fn call(&mut self, m: &mut Mem, name: &str, args: &[Val]) -> Answer {
        // a loop that waits on an answer nobody configured would never end against a recorder
        assert!(self.lines.len() < 20_000, "log-overflow");
        let mut line = call_line(name, args);
        for (addr, n) in &self.watch {
            write!(line, " [{:08x}:", addr).unwrap();
            for b in m.try_bytes(*addr, *n).unwrap_or(&[]) {
                write!(line, "{:02x}", b).unwrap();
            }
            line.push(']');
        }
        self.lines.push(line);
        let a = match self.queued.get_mut(name).and_then(|q| q.pop_front()) {
            Some(a) => a,
            None => self.fallback.get(name).cloned().unwrap_or_default(),
        };
        for (addr, bytes) in &a.pokes {
            m.set_bytes(*addr, bytes);
        }
        a
    }
}

pub type SharedLog = Rc<RefCell<Log>>;

pub fn new_log() -> SharedLog {
    Rc::new(RefCell::new(Log::default()))
}

/// An argument of an outside call: how it is logged, and (for an output) how it is filled
/// from the answer.
pub trait Arg {
    fn val(&self) -> Val;
    /// Take this argument's value from the answer; `next` is the index of the next unused
    /// value. Only outputs do anything.
    fn fill(&mut self, _a: &Answer, _next: &mut usize) {}
}

macro_rules! arg_word {
    ($($t:ty),*) => { $( impl Arg for $t { fn val(&self) -> Val { Val::W(*self as u32) } } )* };
}
arg_word!(i32, u32);
impl Arg for i16 {
    fn val(&self) -> Val {
        Val::H(*self as u16)
    }
}
impl Arg for u16 {
    fn val(&self) -> Val {
        Val::H(*self)
    }
}
impl Arg for u8 {
    fn val(&self) -> Val {
        Val::B(*self)
    }
}
impl Arg for f32 {
    fn val(&self) -> Val {
        Val::F(*self)
    }
}
impl Arg for f64 {
    fn val(&self) -> Val {
        Val::D(*self)
    }
}
impl<T> Arg for P<T> {
    fn val(&self) -> Val {
        Val::W(self.0)
    }
}
impl Arg for &[u8] {
    fn val(&self) -> Val {
        Val::S(self.to_vec())
    }
}
impl Arg for &[i32; 4] {
    fn val(&self) -> Val {
        Val::A(self.iter().map(|v| *v as u32).collect())
    }
}
impl Arg for &[u32; 4] {
    fn val(&self) -> Val {
        Val::A(self.to_vec())
    }
}
impl Arg for [u32; 10] {
    fn val(&self) -> Val {
        Val::A(self.to_vec())
    }
}

/// A pointer to a local variable of the caller that the callee reads and writes.
pub struct InOut<'a, T>(pub &'a mut T);

impl<T> Arg for InOut<'_, P<T>> {
    fn val(&self) -> Val {
        Val::W(self.0 .0)
    }
    fn fill(&mut self, a: &Answer, next: &mut usize) {
        *self.0 = P::at(a.val(*next) as u32);
        *next += 1;
    }
}
impl Arg for &mut i16 {
    fn val(&self) -> Val {
        Val::Out
    }
    fn fill(&mut self, a: &Answer, next: &mut usize) {
        **self = a.val(*next) as i16;
        *next += 1;
    }
}
impl Arg for &mut i32 {
    fn val(&self) -> Val {
        Val::Out
    }
    fn fill(&mut self, a: &Answer, next: &mut usize) {
        **self = a.val(*next) as i32;
        *next += 1;
    }
}
impl Arg for &mut u32 {
    fn val(&self) -> Val {
        Val::Out
    }
    fn fill(&mut self, a: &Answer, next: &mut usize) {
        **self = a.val(*next) as u32;
        *next += 1;
    }
}
impl<T> Arg for &mut P<T> {
    fn val(&self) -> Val {
        Val::Out
    }
    fn fill(&mut self, a: &Answer, next: &mut usize) {
        **self = P::at(a.val(*next) as u32);
        *next += 1;
    }
}

/// A result taken from the first value of an answer.
pub trait Ret {
    fn ret(a: &Answer) -> Self;
}
impl Ret for () {
    fn ret(_: &Answer) {}
}
impl Ret for i32 {
    fn ret(a: &Answer) -> i32 {
        a.val(0) as i32
    }
}
impl Ret for u32 {
    fn ret(a: &Answer) -> u32 {
        a.val(0) as u32
    }
}
impl Ret for i16 {
    fn ret(a: &Answer) -> i16 {
        a.val(0) as i16
    }
}
impl Ret for f64 {
    fn ret(a: &Answer) -> f64 {
        f64::from_bits(a.val(0))
    }
}
impl<T> Ret for P<T> {
    fn ret(a: &Answer) -> P<T> {
        P::at(a.val(0) as u32)
    }
}
