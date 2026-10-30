// SPDX-License-Identifier: MIT
use crate::{Args, Effect, Route, StandIn};
use si_mem::{Mem, Names, Region, Routine};
use si_x87::vectors::{Case, Word};
use std::collections::BTreeMap;

/// What the generator writes for an argument that is the address of a local of the original's.
pub const LOCAL: u32 = 0x57ac_57ac;

/// How a recorded argument that is a stack address is held against the port's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StackArgs {
    /// The address must be the original's (`57ac57ac` is the exception: any stack address).
    #[default]
    Exact,
    /// A recorded stack address matches any stack address of the port within `frame` bytes
    /// below the stack pointer at the port's entry; the port needs no padding of its frame.
    Normalised { frame: u32 },
}

/// What a crate adds to the generic outside world.
#[derive(Default, Clone)]
pub struct Hooks {
    /// Stand-ins by the callee's name: answered without looking at the recorded calls.
    pub stand_ins: Vec<(&'static str, StandIn)>,
    /// The crate's own Rust for a callee.
    pub route: Option<Route>,
    /// What a stand-in did to the image.
    pub effect: Option<Effect>,
}

/// The outside world of a case: every call is held against the next one the original made
/// and answered as that one was. Calls the C library's mathematics answered in st0 are
/// stepped over (`si_x87::trig::replay` plays them back to the port itself).
///
/// A callee that is not in the case's calls is an error, unless `lenient`: then it is
/// answered 0 and counted in `unported` (which callees a case reached).
pub struct Recorded<'a> {
    pub(crate) case: &'a Case,
    pub(crate) args: &'a Args<'a>,
    pub(crate) names: &'a Names,
    pub(crate) hooks: &'a Hooks,
    pub(crate) next: usize,
    pub(crate) flip: bool,
    pub(crate) lenient: bool,
    pub wrong: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub unported: BTreeMap<String, u32>,
    pub(crate) stack_args: StackArgs,
    pub(crate) entry_sp: u32,
}

impl<'a> Recorded<'a> {
    pub(crate) fn new(case: &'a Case, args: &'a Args<'a>, names: &'a Names, hooks: &'a Hooks, flip: bool, lenient: bool) -> Self {
        Recorded { case, args, names, hooks, next: 0, flip, lenient, wrong: None, stdout: Vec::new(), stderr: Vec::new(), unported: BTreeMap::new(), stack_args: StackArgs::Exact, entry_sp: 0 }
    }

    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }

    /// Steps over the recorded calls of the C library's mathematics.
    pub(crate) fn skip_libm(&mut self) {
        while self.case.calls.get(self.next).is_some_and(|c| c.result.bytes() == 10) {
            self.next += 1;
        }
    }

    pub(crate) fn calls_made(&self) -> usize {
        self.next
    }

    /// Whether `got` is an address the port may stand for the original's local `expect`.
    fn local_matches(&self, m: &Mem, expect: u32, got: u32) -> bool {
        let in_stack = |a: u32| m.region_of(a) == Some(Region::Stack);
        if expect == LOCAL {
            return in_stack(got);
        }
        match self.stack_args {
            StackArgs::Normalised { frame } => in_stack(expect) && in_stack(got) && got <= self.entry_sp && got >= self.entry_sp.saturating_sub(frame),
            StackArgs::Exact => false,
        }
    }

    /// Whether the recorded word is the original's local under the current mode.
    fn is_local(&self, m: &Mem, expect: u32) -> bool {
        expect == LOCAL || (matches!(self.stack_args, StackArgs::Normalised { .. }) && m.region_of(expect) == Some(Region::Stack))
    }

    /// Steps over the `NAME.out(...)` entries that follow the call `name` just answered.
    fn skip_outs(&mut self, name: &str) {
        while self.case.calls.get(self.next).is_some_and(|c| c.name.strip_suffix(".out") == Some(name)) {
            self.next += 1;
        }
    }

    fn unexpected(&mut self, name: &str, why: impl FnOnce() -> String) -> si_mem::Ret {
        if self.lenient {
            *self.unported.entry(name.to_string()).or_default() += 1;
        } else {
            self.wrong.get_or_insert_with(why);
        }
        si_mem::Ret::eax(0)
    }
}

impl si_mem::Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> si_mem::Ret {
        self.skip_libm();
        let name: String = if routine.name.is_empty() {
            match self.args.callback(routine.addr) {
                Some(n) => n,
                None => {
                    let (n, a) = (self.next, routine.addr);
                    return self.unexpected("(no name)", || format!("call {n}: the port calls {a:08x}, which no argument of the case holds"));
                }
            }
        } else {
            routine.name.to_string()
        };
        // the crate's own Rust, or a stand-in: the recorded call of the same name, if next, is used up
        let own = self.hooks.route.and_then(|r| r(m, routine, args)).or_else(|| self.hooks.stand_ins.iter().find(|s| s.0 == name).map(|s| (s.1)(m, args)));
        if let Some(ret) = own {
            if self.case.calls.get(self.next).is_some_and(|c| c.name == name) {
                self.next += 1;
                self.skip_outs(&name);
            }
            return ret;
        }
        let Some(want) = self.case.calls.get(self.next) else {
            let n = self.next;
            return self.unexpected(&name, || format!("call {n}: the port calls {name} where the original made no further call"));
        };
        if want.name != name && self.lenient {
            return self.unexpected(&name, String::new);
        }
        let n = self.next;
        self.next += 1;
        if want.name != name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {name}, the original {}", want.name));
            return si_mem::Ret::eax(0);
        }
        // the harness logs as many words as the stand-in's rule names; the port may pass more
        let mut args = args.to_vec();
        if self.flip {
            if let Some(a) = args.first_mut() {
                *a ^= 1;
            }
        }
        let mut locals: Vec<u32> = Vec::new();
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if let Some(e) = expect.filter(|&e| self.is_local(m, e)) {
                // the address of a local of the original's: any suitable stack address here
                match got {
                    Some(a) if self.local_matches(m, e, a) => locals.push(a),
                    _ => {
                        self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, a stack address ({e:08x}) in the original"));
                    }
                }
            } else if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, {expect:08x?} in the original"));
            }
        }
        // what the original's callee stored through the locals it was handed follows as
        // `NAME.out(words)`, one entry per local: the same words go to the port's locals
        let mut local = 0;
        while let Some(out) = self.case.calls.get(self.next) {
            if out.name.strip_suffix(".out") != Some(name.as_str()) {
                break;
            }
            self.next += 1;
            let Some(&base) = locals.get(local) else {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: the original stored through more locals than the port passed ({})", locals.len()));
                break;
            };
            local += 1;
            let mut at = base;
            for w in &out.args {
                match (w.bytes(), self.resolve(m, w)) {
                    (2, Some(v)) => m.set_u16(at, v as u16),
                    (1, Some(v)) => m.set_u8(at, v as u8),
                    (_, Some(v)) => m.set_u32(at, v),
                    _ => {}
                }
                at += w.bytes() as u32;
            }
        }
        let answer = self.resolve(m, &want.result).unwrap_or(0);
        if let Some(effect) = self.hooks.effect {
            if let Err(e) = effect(m, routine, &args, answer) {
                self.wrong.get_or_insert(e);
            }
        }
        si_mem::Ret::eax(answer)
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        if stream == 2 { &mut self.stderr } else { &mut self.stdout }.extend_from_slice(text);
    }
}
