// SPDX-License-Identifier: MIT
//! The call log of the recording back ends.
//!
//! Every call the layer makes to something outside the program (OpenGL, GLU, GLX, Xlib, the C
//! library's system services) goes through a trait in [`crate::sys`]. The recording
//! implementation of each trait appends one [`Call`] to a shared [`Log`] and answers with a
//! value configured beforehand, so a test or a check can compare the exact sequence of
//! outside calls, with arguments, against the original routine run in the emulation harness
//! (`re/tools/emu/`) on the same inputs and the same answers.
//!
//! Nothing here reproduces a function of the original; it is the measuring instrument.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::rc::Rc;

/// One argument of a recorded call, or one value of a configured answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// Any integer, enum, XID, atom or mask argument (the original's 32-bit stack word,
    /// zero-extended; a C `short`/`char` argument is the promoted 32-bit value).
    U(u32),
    /// A `float` argument or result, by bits.
    F(f32),
    /// A `double` argument or result, by bits.
    D(f64),
    /// A pointer the original got from outside (a `Display *`, a GLX context, an `XImage *`):
    /// the opaque value the back end handed out.
    P(u32),
    /// A NUL-terminated string argument (without the NUL), or a string result.
    S(Vec<u8>),
    /// Memory the callee reads through a pointer argument, or writes back through one.
    B(Vec<u8>),
    /// Words (4 bytes each) the callee reads through a pointer argument, or writes back.
    W(Vec<u32>),
    /// A pointer through which the callee only writes (the content before the call is not
    /// part of the comparison), or a null pointer where the original passes one for an
    /// optional output.
    Out,
}

impl Val {
    pub fn u(&self) -> u32 {
        match self {
            Val::U(v) | Val::P(v) => *v,
            Val::F(v) => v.to_bits(),
            _ => 0,
        }
    }
    pub fn i(&self) -> i32 {
        self.u() as i32
    }
    pub fn f(&self) -> f32 {
        match self {
            Val::F(v) => *v,
            Val::D(v) => *v as f32,
            Val::U(v) => f32::from_bits(*v),
            _ => 0.0,
        }
    }
    pub fn d(&self) -> f64 {
        match self {
            Val::D(v) => *v,
            Val::F(v) => *v as f64,
            _ => 0.0,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Val::S(v) | Val::B(v) => v,
            _ => &[],
        }
    }
    pub fn words(&self) -> &[u32] {
        match self {
            Val::W(v) => v,
            _ => &[],
        }
    }
}

/// One call that left the layer: the imported symbol's name and its arguments in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: &'static str,
    pub args: Vec<Val>,
}

impl Call {
    /// One line of text, the form the check scripts compare:
    /// `name(0x1, f:3f800000, d:3ff0000000000000, "text", b:0a0b, w:[1,2], out)`
    /// (a pointer value prints like any other word).
    pub fn line(&self) -> String {
        let mut s = String::new();
        s.push_str(self.name);
        s.push('(');
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            match a {
                Val::U(v) => write!(s, "{:#x}", v).unwrap(),
                Val::F(v) => write!(s, "f:{:08x}", v.to_bits()).unwrap(),
                Val::D(v) => write!(s, "d:{:016x}", v.to_bits()).unwrap(),
                Val::P(v) => write!(s, "{:#x}", v).unwrap(),
                Val::S(v) => {
                    s.push('"');
                    for &b in v {
                        if (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\' {
                            s.push(b as char);
                        } else {
                            write!(s, "\\x{:02x}", b).unwrap();
                        }
                    }
                    s.push('"');
                }
                Val::B(v) => {
                    s.push_str("b:");
                    for b in v {
                        write!(s, "{:02x}", b).unwrap();
                    }
                }
                Val::W(v) => {
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
}

/// The configured answer to one call: the return value first, then whatever the callee
/// writes through its output pointers, in argument order.
pub type Answer = Vec<Val>;

/// The shared log: the calls made so far and the answers still to be given.
#[derive(Default)]
pub struct Log {
    pub calls: Vec<Call>,
    queued: HashMap<String, VecDeque<Answer>>,
    fallback: HashMap<String, Answer>,
    /// Next value for a call that hands out a new handle and has no configured answer.
    next_handle: u32,
}

impl Log {
    /// Queue an answer for the next unanswered call of `name`.
    ///
    /// For the OpenGL queries (`glGetIntegerv`, `glGetFloatv`, `glGetDoublev`) the name may
    /// carry the query: `"glGetIntegerv:0xba2"` answers only a viewport query, and is
    /// looked for before the plain name.
    pub fn answer(&mut self, name: &str, a: Answer) {
        self.queued.entry(name.to_string()).or_default().push_back(a);
    }
    /// The answer for every call of `name` once its queue is empty.
    pub fn answer_always(&mut self, name: &str, a: Answer) {
        self.fallback.insert(name.to_string(), a);
    }
    /// Record a call and take its answer (`None`: nothing configured; the caller of this
    /// method then uses its own default, 0 for integers).
    pub fn call(&mut self, name: &'static str, args: Vec<Val>) -> Option<Answer> {
        // a loop of the layer that waits on an answer nobody configured (`qread` on an empty
        // queue, `mmode` popping until an underflow) would never end against a recorder
        assert!(self.calls.len() < 200_000, "log-overflow");
        self.calls.push(Call { name, args });
        self.take_answer(name)
    }
    /// Record a call whose answer may be configured under `key` (the name with the query,
    /// `"glGetIntegerv:0xba2"`) or, failing that, under the plain name.
    pub fn call_keyed(&mut self, name: &'static str, key: &str, args: Vec<Val>) -> Option<Answer> {
        assert!(self.calls.len() < 200_000, "log-overflow");
        self.calls.push(Call { name, args });
        match self.take_answer(key) {
            Some(a) => Some(a),
            None => self.take_answer(name),
        }
    }
    /// Take the next answer of `name` without recording a call (for the accessors that
    /// stand for memory the original reads, not for a call).
    pub fn take_answer(&mut self, name: &str) -> Option<Answer> {
        if let Some(q) = self.queued.get_mut(name) {
            if let Some(a) = q.pop_front() {
                return Some(a);
            }
        }
        self.fallback.get(name).cloned()
    }
    /// A fresh non-zero value for a pointer or XID result nobody configured.
    pub fn fresh(&mut self) -> u32 {
        self.next_handle += 0x10;
        0x7000_0000 + self.next_handle
    }
    pub fn lines(&self) -> Vec<String> {
        self.calls.iter().map(Call::line).collect()
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.calls.iter().map(|c| c.name).collect()
    }
    pub fn clear(&mut self) {
        self.calls.clear();
    }
}

/// The log as the recording back ends share it (one log for all libraries, so the order of
/// calls across libraries is kept).
pub type SharedLog = Rc<RefCell<Log>>;

pub fn new_log() -> SharedLog {
    Rc::new(RefCell::new(Log::default()))
}

/// Answer helpers for the recording implementations.
pub(crate) fn ret_u(a: &Option<Answer>, default: u32) -> u32 {
    a.as_ref().and_then(|v| v.first()).map(Val::u).unwrap_or(default)
}
pub(crate) fn out_u(a: &Option<Answer>, index: usize, default: u32) -> u32 {
    a.as_ref().and_then(|v| v.get(index)).map(Val::u).unwrap_or(default)
}

/// A scalar argument as a logged value.
pub trait IntoVal {
    fn into_val(&self) -> Val;
}
impl IntoVal for u32 {
    fn into_val(&self) -> Val {
        Val::U(*self)
    }
}
impl IntoVal for i32 {
    fn into_val(&self) -> Val {
        Val::U(*self as u32)
    }
}
impl IntoVal for u16 {
    fn into_val(&self) -> Val {
        Val::U(*self as u32)
    }
}
impl IntoVal for u8 {
    fn into_val(&self) -> Val {
        Val::U(*self as u32)
    }
}
impl IntoVal for f32 {
    fn into_val(&self) -> Val {
        Val::F(*self)
    }
}
impl IntoVal for f64 {
    fn into_val(&self) -> Val {
        Val::D(*self)
    }
}

/// A scalar result taken from a configured answer (its first value); 0 without one.
pub trait FromAns {
    fn from_ans(a: &Option<Answer>) -> Self;
}
impl FromAns for () {
    fn from_ans(_: &Option<Answer>) {}
}
impl FromAns for u32 {
    fn from_ans(a: &Option<Answer>) -> u32 {
        ret_u(a, 0)
    }
}
impl FromAns for i32 {
    fn from_ans(a: &Option<Answer>) -> i32 {
        ret_u(a, 0) as i32
    }
}
impl FromAns for u8 {
    fn from_ans(a: &Option<Answer>) -> u8 {
        ret_u(a, 0) as u8
    }
}

pub(crate) fn f32_words(v: &[f32]) -> Val {
    Val::W(v.iter().map(|f| f.to_bits()).collect())
}
pub(crate) fn f64_words(v: &[f64]) -> Val {
    let mut w = Vec::with_capacity(v.len() * 2);
    for d in v {
        let b = d.to_bits();
        w.push(b as u32);
        w.push((b >> 32) as u32);
    }
    Val::W(w)
}
pub(crate) fn i32_words(v: &[i32]) -> Val {
    Val::W(v.iter().map(|i| *i as u32).collect())
}
/// The `index`-th value of an answer as words (empty without one).
pub(crate) fn out_words(a: &Option<Answer>, index: usize) -> Vec<u32> {
    a.as_ref().and_then(|v| v.get(index)).map(|v| v.words().to_vec()).unwrap_or_default()
}
pub(crate) fn out_bytes(a: &Option<Answer>, index: usize) -> Vec<u8> {
    a.as_ref().and_then(|v| v.get(index)).map(|v| v.bytes().to_vec()).unwrap_or_default()
}
