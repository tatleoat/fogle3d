// SPDX-License-Identifier: MIT
//! The file helpers of the writers, on the image: the swapping writes of `byteswap.c`,
//! the string writer of `filetoken.c` and the header writers of `headers.c`.
//!
//! Every swapping write is the original's: the items are swapped **in place** in the
//! record (through the global `tmp`, as the swaps of the readers), the record is handed to
//! `fwrite`, and the items are swapped back. What the record holds afterwards is what it
//! held before; `tmp` is left with the last byte that passed through it.
//!
//! The version tests of the writers compare `SI_versionOutput` (a float, never written by
//! the program: 4.0) with a double constant in the x87 unit. The listing has four forms,
//! one helper each below; with the version at 4.0 every test of this build passes, and
//! the forms differ only for a version that would be NaN.

use super::globals::{SI_VERSION_OUTPUT, S_NO_COMMENT_HEADER, S_NO_COMMENT_NEWHEADER};
use super::io::{fix_file_chunk, swap2byte, swap4byte, swap8byte};
use super::outside::Outside;
use si_mem::Mem;

/// `SI_versionOutput > c` (`fcompl c`; `and $0x45,%ah`; `jne`): false for NaN.
pub fn out_above(m: &Mem, c: f64) -> bool {
    (m[SI_VERSION_OUTPUT] as f64) > c
}

/// `SI_versionOutput >= c` (`fcompl c`; `and $0x5,%ah`; `jne`, or `fldl c`; `fcomp`
/// against the version; `and $0x45`; `dec`; `cmp $0x40`; `jae`): false for NaN.
pub fn out_from(m: &Mem, c: f64) -> bool {
    (m[SI_VERSION_OUTPUT] as f64) >= c
}

/// `!(SI_versionOutput <= c)` (`and $0x45,%ah`; `dec %ah`; `cmp $0x40,%ah`; `jb`): true
/// for NaN.
pub fn out_above_or_nan(m: &Mem, c: f64) -> bool {
    !((m[SI_VERSION_OUTPUT] as f64) <= c)
}

/// `SI_versionOutput < c` (`and $0x45,%ah`; `cmp $0x1,%ah`): false for NaN.
pub fn out_below(m: &Mem, c: f64) -> bool {
    (m[SI_VERSION_OUTPUT] as f64) < c
}

/// One pass of the item swap of [`byteswap_fwrite`]: items of 2, 4 or 8 bytes are swapped,
/// any other size is left alone.
fn swap_items(m: &mut Mem, ptr: u32, size: u32, n: u32) {
    let mut i = 0u32;
    while i < n {
        let p = ptr.wrapping_add(size.wrapping_mul(i));
        match size {
            2 => swap2byte(m, p),
            4 => swap4byte(m, p),
            8 => swap8byte(m, p),
            _ => {}
        }
        i = i.wrapping_add(1);
    }
}

/// [soft 0x083f9c40 SI_byteswap_fwrite]: each of the `n` items of `size` bytes at `ptr`
/// swapped in place when the size is 2, 4 or 8, the whole written with `fwrite`, and the
/// items swapped back. Answers what `fwrite` answered.
pub fn byteswap_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    swap_items(m, ptr, size, n);
    let written = o.fwrite(m, ptr, size, n, file);
    swap_items(m, ptr, size, n);
    written
}

/// The three floats of each of `n` 12-byte items swapped in place.
fn swap_triples(m: &mut Mem, ptr: u32, n: u32, stride: u32, words: u32) {
    let mut i = 0u32;
    while i < n {
        let p = ptr.wrapping_add(stride.wrapping_mul(i));
        for k in 0..words {
            swap4byte(m, p.wrapping_add(4 * k));
        }
        i = i.wrapping_add(1);
    }
}

/// [soft 0x083f9df8 SI_Vector_fwrite]: vectors of three floats, `n` of them 12 bytes
/// apart, swapped, written as `n` items of `size` bytes, swapped back.
pub fn vector_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    swap_triples(m, ptr, n, 12, 3);
    let written = o.fwrite(m, ptr, size, n, file);
    swap_triples(m, ptr, n, 12, 3);
    written
}

/// [soft 0x083fa3d4 SI_RGB_fwrite]: the same for colours of three floats.
pub fn rgb_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    swap_triples(m, ptr, n, 12, 3);
    let written = o.fwrite(m, ptr, size, n, file);
    swap_triples(m, ptr, n, 12, 3);
    written
}

/// [soft 0x083fa24c SI_RGBA_fwrite]: colours of four floats, 16 bytes apart.
pub fn rgba_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    swap_triples(m, ptr, n, 16, 4);
    let written = o.fwrite(m, ptr, size, n, file);
    swap_triples(m, ptr, n, 16, 4);
    written
}

/// SI_3doubles_fwrite (0x083f9f5c); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the three doubles at `ptr` copied to 24 bytes of
/// the routine's stack, swapped there, and `n` items of `size` bytes written from that
/// copy (the record is not touched). Answers nothing (the original's `eax` is `fwrite`'s;
/// no caller looks).
///
/// With `size × n` above 24 the original writes on from its stack (its frame and the
/// caller's); the port's copy lies on the image's stack with the same 24 bytes, and what
/// follows there is the port's own.
pub fn three_doubles_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) {
    const FRAME: u32 = 0x18;
    let local = m.local(FRAME);
    for k in 0..3u32 {
        let v = m.u64(ptr.wrapping_add(8 * k));
        m.set_u64(local + 8 * k, v);
        swap8byte(m, local + 8 * k);
    }
    o.fwrite(m, local, size, n, file);
    m.release(FRAME);
}

/// SI_Matrix_fwrite (0x083fa524); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the sixteen floats of a 4 × 4 matrix swapped,
/// written as `n` items of `size` bytes, swapped back (always the sixteen, whatever `n`).
pub fn matrix_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    swap_triples(m, ptr, 1, 64, 16);
    let written = o.fwrite(m, ptr, size, n, file);
    swap_triples(m, ptr, 1, 64, 16);
    written
}

/// [soft 0x083faaa0 SI_FileChunk_fwrite]: headers of 0x58 bytes, of each the magic and the
/// version swapped (`SI_fix_SI_FileChunk`), written, swapped back.
pub fn file_chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let mut i = 0u32;
    while i < n {
        fix_file_chunk(m, ptr.wrapping_add(0x58u32.wrapping_mul(i)));
        i = i.wrapping_add(1);
    }
    let written = o.fwrite(m, ptr, size, n, file);
    let mut i = 0u32;
    while i < n {
        fix_file_chunk(m, ptr.wrapping_add(0x58u32.wrapping_mul(i)));
        i = i.wrapping_add(1);
    }
    written
}

/// [soft 0x08404104 SI_stringBinaryWrite]: the bytes of the string and its zero, one
/// `fputc` each; a null string is a lone zero byte. Answers 1, or 0 for a null string.
pub fn string_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, s: u32) -> i32 {
    if s == 0 {
        o.fputc(m, 0, file);
        return 0;
    }
    let mut p = s;
    loop {
        // `fputc((int)*p)`: the byte sign-extended
        let c = m.u8(p) as i8 as i32;
        o.fputc(m, c, file);
        if c == 0 {
            break;
        }
        p = p.wrapping_add(1);
    }
    1
}

/// `strncpy(dst, src, 80)`: the string and then zeros up to 80 bytes; a string of 80
/// bytes or more is cut without a terminator.
fn strncpy80(m: &mut Mem, dst: u32, src: u32) {
    let mut ended = false;
    for k in 0..0x50u32 {
        let b = if ended { 0 } else { m.u8(src.wrapping_add(k)) };
        if b == 0 {
            ended = true;
        }
        m.set_u8(dst + k, b);
    }
}

/// A header record on the routine's stack: magic, version, comment (`strncpy`, 80 bytes),
/// written with [`file_chunk_fwrite`].
fn header_write(m: &mut Mem, o: &mut dyn Outside, file: u32, magic: u32, version: f32, comment: u32) {
    const FRAME: u32 = 0x58;
    let local = m.local(FRAME);
    m.set_u32(local, magic);
    m.set_f32(local + 4, version);
    strncpy80(m, local + 8, comment);
    file_chunk_fwrite(m, o, local, 0x58, 1, file);
    m.release(FRAME);
}

/// [soft 0x083fb83c SI_headerBinaryWrite]: a header with the general magic 0x5380f634,
/// `SI_versionOutput` and the comment (`"No comment..."` for a null one).
pub fn header_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, comment: u32) {
    let comment = if comment == 0 { S_NO_COMMENT_HEADER } else { comment };
    let version = m[SI_VERSION_OUTPUT];
    header_write(m, o, file, 0x5380_f634, version, comment);
}

/// [soft 0x083fb88c si_newheaderBinaryWrite]: a header with the given magic, version and
/// comment (`"No comment..."` for a null one).
pub fn newheader_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, magic: u32, version: f32, comment: u32) {
    let comment = if comment == 0 { S_NO_COMMENT_NEWHEADER } else { comment };
    header_write(m, o, file, magic, version, comment);
}

/// [soft 0x083fb8dc SI_dbfileWriteBinHeader]: [`newheader_binary_write`] without a
/// comment.
pub fn dbfile_write_bin_header(m: &mut Mem, o: &mut dyn Outside, file: u32, version: f32, magic: u32) {
    newheader_binary_write(m, o, file, magic, version, 0);
}
