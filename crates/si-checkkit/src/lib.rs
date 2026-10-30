// SPDX-License-Identifier: MIT
//! `si-checkkit`: the driver every `*-emucheck` binary used to carry a copy of.
//!
//! Nothing here reproduces a function of the original. A check binary registers its ports
//! and calls [`run`]:
//!
//! ```ignore
//! fn main() {
//!     si_checkkit::run(&[
//!         ("SI_genListGetHead", |m, _o, a| l::get_head(m, a.v("list")).into()),
//!         ("SI_genListDestroy", |m, o, a| l::destroy(m, o, a.v("plist"), a.v("destroy")).into()),
//!     ]);
//! }
//! ```
//!
//! Also: out-local arguments (`57ac57ac`) and `NAME.out(..)` stores are handled by every run, [`Kit::normalise_stack_args`],
//! [`Kit::reset_globals`] and the replay mode [`ReplayKit`] (finding 0011, "si-checkkit, 2026-10-28 (T7)").
//!
//! Use [`Kit`] when the check needs globals ([`Kit::setup`]), stand-ins ([`Kit::stand_in`]),
//! a routing of a callee to a crate's own Rust ([`Kit::route`]) or a stand-in's effect on the
//! image ([`Kit::effect`]).
//!
//! The comparison, per case (a case is run twice, on fresh memory holding a pattern and then
//! its complement; both runs must agree with the original):
//!
//! - the return value (`eax`, `ax`, `al`, `ptr`);
//! - every buffer of the case, changed or not;
//! - the heap: as many blocks, each of the same size, each written byte the same, each byte
//!   the original did not write still holding the fill;
//! - the freed blocks, in order;
//! - the calls, in order, with their arguments;
//! - what the port printed (`stdout`, `stderr`);
//! - that the port changed no byte outside the case's buffers and the heap.
//!
//! Naming in a vector line: `v_x` an argument by value, `p_x` a pointer argument, `gXXXXXXXX…`
//! a global at that address, anything else a buffer of the arena. The columns of the report
//! are those of the older drivers.

mod args;
mod check;
mod outside;
mod replay;
mod run;

pub use args::{Args, Ret};
pub use check::{check_case, Config, Mutate, Outcome};
pub use outside::{Recorded, StackArgs, LOCAL};
pub use replay::{word_value, RecCall, ReplayCase, ReplayCount, ReplayEffect, ReplayKit, ReplayOutcome, ReplayOutside, ReplayPort, ReplayVerdict};
pub use run::{check_caught, run, Kit, Tally};

/// A port: the image, whoever answers for the routines outside the port, and the case's
/// arguments; what the routine returned.
pub type Port = fn(&mut si_mem::Mem, &mut dyn si_mem::Outside, &Args) -> Ret;

/// A stand-in for a callee by name: the image and the argument words, the answer.
pub type StandIn = fn(&mut si_mem::Mem, &[u32]) -> si_mem::Ret;

/// A crate's own Rust for a callee: `None` leaves the call to the recorded answer.
pub type Route = fn(&mut si_mem::Mem, si_mem::Routine, &[u32]) -> Option<si_mem::Ret>;

/// What a stand-in did to the image, held against what the original's stand-in did: the
/// routine, its argument words, the answer the original got; `Err` is a difference.
pub type Effect = fn(&mut si_mem::Mem, si_mem::Routine, &[u32], u32) -> Result<(), String>;

#[cfg(test)]
mod tests;
