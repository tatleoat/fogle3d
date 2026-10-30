// SPDX-License-Identifier: MIT
//! Calls into parts of the program that are not ported yet.
//!
//! Loading and saving a scene run through the dictionary code and then hand over to the
//! rest of the program: the scene graph, models, materials, custom effects, the schematic
//! view, dialogs. Each such call is made through [`Unported`], with the original's name and
//! arguments, in the original's order, so that the sequence is on record and a later
//! package can take the calls over one by one.

use crate::dict::ElemId;

/// One argument of a recorded call.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i64),
    /// A chapter index.
    Chap(i32),
    Elem(ElemId),
    /// A pointer argument: the handle of an object, zero for null.
    Ptr(u32),
    Str(Vec<u8>),
}

/// A call the original makes into a package that is not ported.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The original function's name.
    pub name: &'static str,
    pub args: Vec<Arg>,
}

/// The rest of the program, as far as scene loading and saving call into it.
pub trait Unported {
    fn call(&mut self, name: &'static str, args: &[Arg]);

    /// `SI_sceneRefresh (0x0820f470)`, which `SI_dbLoadElementsFromDict` calls near its end:
    /// the scene brought to the current frame (`SI_sceneUpdate`: `refreshObj` is 1 in the
    /// binary's data, so the update runs, in the batch renderer as well). The update is
    /// `si-motion`'s and its callees are other crates': a caller that has them runs it on
    /// the image and answers true; by default the call is recorded (finding 0018, section 19).
    fn scene_refresh(&mut self, _m: &mut si_mem::Mem) -> bool {
        false
    }

    /// `SI_dynamicChainsUpdate (0x081cc1cc)`, which `SI_dbLoadElementsFromDict` calls after the
    /// elements are in: the chains of the dynamics linked and their geometry brought up to date.
    /// A caller that has the port (`si-dynamics`) runs it on the image and answers true; by
    /// default the call is recorded.
    fn dynamic_chains_update(&mut self, _m: &mut si_mem::Mem) -> bool {
        false
    }

    /// A routine of the original that has no port reachable from `si-scene` (its crate depends on
    /// this one) and that the loader's readers call by address: a host above answers it on the image
    /// (`Some(result)`), else the call stays a hook answered 0. Not a call of the original.
    fn answer(&mut self, _m: &mut si_mem::Mem, _addr: u32, _args: &[u32]) -> Option<u32> {
        None
    }

    /// Told before (`after` false) and after (`after` true) the loader's dispatch
    /// (`image::Prog`) hands the routine at `addr` to its port: for a check that compares a
    /// port with the original on the very state the load reached (finding 0029, third
    /// pass). Not a call of the original; nothing by default.
    fn port_call(&mut self, _m: &mut si_mem::Mem, _addr: u32, _args: &[u32], _after: bool) {}
}

/// Records every call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorder {
    pub calls: Vec<Call>,
}

impl Unported for Recorder {
    fn call(&mut self, name: &'static str, args: &[Arg]) {
        self.calls.push(Call { name, args: args.to_vec() });
    }
}

impl Recorder {
    /// The names of the calls, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.calls.iter().map(|c| c.name).collect()
    }
}

/// Whoever answers the program's dialogues, and the questions the save puts to packages
/// that are not ported. A call names the original routine and what it shows; the answer is
/// the routine's result (for a dialogue, the number of the button, from 0).
pub trait Ask {
    fn ask(&mut self, name: &'static str, args: &[Arg]) -> i32;
}

/// Answers every question with one number, after any answers queued for it, and keeps what
/// was asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answers {
    pub queue: std::collections::VecDeque<i32>,
    pub default: i32,
    pub asked: Vec<Call>,
}

impl Ask for Answers {
    fn ask(&mut self, name: &'static str, args: &[Arg]) -> i32 {
        self.asked.push(Call { name, args: args.to_vec() });
        self.queue.pop_front().unwrap_or(self.default)
    }
}

/// Where a message of the original goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `SI_statusDisplayMessage(text, level)`: the status line, with its level (2 a remark,
    /// 3 an error).
    Status(i32),
    Stdout,
    Stderr,
}

/// A message the original prints.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub channel: Channel,
    pub text: Vec<u8>,
}
