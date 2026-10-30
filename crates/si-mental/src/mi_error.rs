// SPDX-License-Identifier: MIT
//! `mi_error.c`: the translator's three message routines (package 63, finding 0042).
//!
//! The original writes to the C library's `stderr` with `fprintf` and `vfprintf`; here the text is
//! formatted on the image ([`crate::cprintf`]) and handed to [`Outside::print`] on stream 2. The
//! variable-argument list is the argument words after the format, exactly as the caller pushed them
//! (a `double` as two words, low word first).

use crate::cprintf;
use crate::mi_mr_main::ext;
use si_mem::{Mem, Outside};

/// The program name (a pointer to a string), the model name (a pointer, or null) and the debug flag.
const PROGNAME: u32 = 0x086e_f9ac;
const MODEL_NAME: u32 = 0x0882_9f48;
const DEBUG_FLAG: u32 = 0x0882_9f0c;

fn put(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], words: &[u32]) {
    let t = cprintf::format_bytes(m, fmt, words);
    o.print(2, &t);
}

/// [soft 0x08598ba0 sfatal]: "<prog>: ERROR[ in model <name>]: <message>\nExiting.\n", then the
/// thread exit (`SI_mrthreadExit (0x084302cc)`, called with 0).
pub fn sfatal(m: &mut Mem, o: &mut dyn Outside, fmt: u32, va: &[u32]) {
    let prog = m.u32(PROGNAME);
    put(m, o, b"%s: ERROR", &[prog]);
    let model = m.u32(MODEL_NAME);
    if model != 0 {
        put(m, o, b" in model %s", &[model]);
    }
    put(m, o, b": ", &[]);
    let t = cprintf::format(m, fmt, va);
    o.print(2, &t);
    put(m, o, b"\nExiting.\n", &[]);
    o.call(m, ext::SI_MRTHREAD_EXIT, &[0]);
}

/// [soft 0x08598c44 swarning]: "<prog>: WARNING[, model <name>]: <message>\n".
pub fn swarning(m: &mut Mem, o: &mut dyn Outside, fmt: u32, va: &[u32]) {
    let prog = m.u32(PROGNAME);
    put(m, o, b"%s: WARNING", &[prog]);
    let model = m.u32(MODEL_NAME);
    if model != 0 {
        put(m, o, b", model %s", &[model]);
    }
    put(m, o, b": ", &[]);
    let t = cprintf::format(m, fmt, va);
    o.print(2, &t);
    put(m, o, b"\n", &[]);
}

/// [soft 0x08598cdc sdebug]: with the debug flag set, "<prog>: [model <name>: ]<message>", no newline.
pub fn sdebug(m: &mut Mem, o: &mut dyn Outside, fmt: u32, va: &[u32]) {
    if m.u32(DEBUG_FLAG) == 0 {
        return;
    }
    let prog = m.u32(PROGNAME);
    put(m, o, b"%s: ", &[prog]);
    let model = m.u32(MODEL_NAME);
    if model != 0 {
        put(m, o, b"model %s: ", &[model]);
    }
    let t = cprintf::format(m, fmt, va);
    o.print(2, &t);
}
