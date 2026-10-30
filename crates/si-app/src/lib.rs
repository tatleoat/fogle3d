// SPDX-License-Identifier: MIT
//! Softimage|3D start-up, command line, top-level state and main loop (finding 0019): the
//! frame every other part of the program hangs on.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `soft_main.c`, `sys_main.c`, `sys_memory.c`,
//! `sys_signal.c`, `globals.c`, `SI_error.c`, `in_progress.c`, `request.c`, `log.c`,
//! `emergencySave.c`, `userpref.c`, and the parts of `server.c` and of the user
//! interface's `event.c` that the main loop is made of.
//!
//! - [`App`] is the program: its globals ([`Globals`], each field with the original's
//!   symbol and address) and the world outside this crate.
//! - Every call into a part that is not ported goes through a trait of [`env`], one per
//!   subsystem; [`Recorder`] implements them all, writes each call down with its
//!   arguments and answers from a script. The binary `soft` runs the whole start-up
//!   sequence and main loop against it and prints the calls; `app-emucheck` compares the
//!   record, call for call, with the one the emulation harness makes of the original.
//! - The original's `exit(n)` is the error value [`Exit`]. No process is started, no
//!   signal handler installed and no real file touched by the recorder.
//! - [`image`] is the part already on `si-mem`'s shared image (finding 0019, section 12):
//!   the batch renderer's path from `si_runCreative` to the hand-over to Get Scene and the
//!   options after it, with the entry points [`image::start_up`] and [`image::finish`] for
//!   checks that load a scene. Methods of `App` whose routine is there run it through an
//!   adapter.
//! - Licence code is not ported: where the original's sequence has a licence check a
//!   comment says "licence check, not ported", and where a value depends on one the port
//!   uses the full product's value.

pub mod emergency;
pub mod env;
pub mod error;
mod error_table;
pub mod event;
pub mod globals;
pub mod history;
pub mod image;
pub mod log;
pub mod memory;
pub mod progress;
pub mod render_cmd;
pub mod request;
pub mod server;
pub mod signal;
pub mod state;
pub mod sysmain;
mod usage_text;
pub mod userpref;

pub use env::{Bytes, Exit, Handle, Recorder, Run, World};
pub use server::{Clients, ScriptedClients};
pub use state::{App, Event, EventRef, Globals, NetEntry};
pub use usage_text::{PRINT_USAGE, RENDER_USAGE};
