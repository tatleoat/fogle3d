// SPDX-License-Identifier: MIT
//! The shader database of Softimage|3D 4.0 on the shared memory image (finding 0041).
//!
//! Nine source files of the Linux `soft` (0x0854cab0 .. 0x08551a00, 92 functions): the
//! `_SHADER_PREVIEWER` text of a shader's definition is scanned (`shdb_lex`) and parsed
//! (`shdb_yacc`) into a tree of 0x1c-byte nodes (`shdb_support`), which `shdb_init` turns
//! into a preview scene and `shdb_translator` writes as mental ray text down a pipe.
//! Every function takes the image, whoever answers for the rest of the program
//! ([`si_mem::Outside`]) and the original's arguments.

pub mod cfmt;
pub mod custom_gen;
pub mod custom_dso;
pub mod custom_dso2;
pub mod custom_del;
pub mod custom_dyn;
pub mod custom_fcv;
pub mod custom_init;
pub mod custom_io;
pub mod custom_udf;
pub mod custom_udf2;
pub mod custom_val;
pub mod custom_enc;
pub mod custom_util;
pub mod custom_load;
pub mod custom_misc;
pub mod custom_mn;
pub mod custom_rel;
pub mod custom_util2;
pub mod custom_parse;
pub mod custom_scene;
pub mod custom_shader;
pub mod shdb_data;
pub mod shdb_display;
pub mod shdb_global;
pub mod shdb_hooks;
pub mod shdb_init;
pub mod shdb_main;
pub mod shdb_preview;
pub mod shdb_lex;
pub mod shdb_support;
pub mod shdb_translator;
pub mod shdb_yacc;
pub mod strlist;

use si_mem::{Mem, Outside, Routine};

/// The C library's `fprintf`, through the binary's own stub.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
/// `fflush`.
pub const FFLUSH: Routine = Routine::new("fflush", 0x080c_3be8);
/// `exit`.
pub const EXIT: Routine = Routine::new("exit", 0x080c_4b58);
/// `stderr`, `stdin` and `stdout` (the pointer variables).
pub const STDERR: u32 = 0x0878_0c88;
pub const STDIN: u32 = 0x0878_0e90;
pub const STDOUT: u32 = 0x0878_0c84;

/// A float copied through the x87 stack (`fld`, `fstp`): a signalling NaN comes out quiet.
pub fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// Puts the data of these sources (strings, flags, parser tables) into the image.
pub fn store_data(m: &mut Mem) {
    shdb_data::store(m);
}

/// `fprintf(fp, fmt, ...)` the way the image holds it: text for a standard stream is formatted
/// here and printed (the harness keeps the standard streams in memory and compares their text);
/// any other `FILE *` goes to the C library's `fprintf` as the original calls it.
pub fn fprintf(m: &mut Mem, o: &mut dyn Outside, fp: u32, fmt: u32, args: &[u32]) {
    let stream = if fp == m.u32(STDOUT) {
        1
    } else if fp == m.u32(STDERR) {
        2
    } else {
        let mut a = vec![fp, fmt];
        a.extend_from_slice(args);
        o.call(m, FPRINTF, &a);
        return;
    };
    let text = cfmt::format(m, fmt, args);
    o.print(stream, &text);
}

/// `fwrite(buf, size, 1, fp)` for a standard stream is printed, otherwise called.
pub fn fwrite_one(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, fp: u32) {
    let stream = if fp == m.u32(STDOUT) {
        1
    } else if fp == m.u32(STDERR) {
        2
    } else {
        o.call(m, FWRITE, &[buf, size, 1, fp]);
        return;
    };
    let text: Vec<u8> = (0..size).map(|i| m.u8(buf.wrapping_add(i))).collect();
    o.print(stream, &text);
}

/// `fflush(fp)`: nothing to do for a standard stream (the harness keeps their text in memory).
pub fn fflush(m: &mut Mem, o: &mut dyn Outside, fp: u32) {
    if fp != m.u32(STDOUT) && fp != m.u32(STDERR) {
        o.call(m, FFLUSH, &[fp]);
    }
}

/// `fwrite`.
pub const FWRITE: Routine = Routine::new("fwrite", 0x080c_4f78);
