// SPDX-License-Identifier: MIT
//! `byteswap.c`: the reads that turn the files' big-endian fields into the machine's order.
//!
//! The routines swap in place, in the image, through the file's one static byte `tmp`
//! (which keeps the last byte moved), and look at `fread`'s count only to know how many
//! items to swap: an item `fread` did not complete is left as the bytes that did arrive.

use super::globals::TMP;
use super::outside::{fread, Outside};
use si_mem::Mem;

/// `tmp = p[a]; p[a] = p[b]; p[b] = tmp`
fn swap(m: &mut Mem, p: u32, a: u32, b: u32) {
    let t = m.u8(p + a);
    m.set_u8(TMP.addr(), t);
    let v = m.u8(p + b);
    m.set_u8(p + a, v);
    let t = m.u8(TMP.addr());
    m.set_u8(p + b, t);
}

/// [soft 0x083f9330 SI_swap2byte]
pub fn swap2byte(m: &mut Mem, p: u32) {
    swap(m, p, 0, 1);
}

/// [soft 0x083f9350 SI_swap4byte]
pub fn swap4byte(m: &mut Mem, p: u32) {
    swap(m, p, 0, 3);
    swap(m, p, 1, 2);
}

/// [soft 0x083f9388 SI_swap8byte]
pub fn swap8byte(m: &mut Mem, p: u32) {
    swap(m, p, 0, 7);
    swap(m, p, 1, 6);
    swap(m, p, 2, 5);
    swap(m, p, 3, 4);
}

/// [soft 0x083f9320 validate_float]
/// Does nothing (in this build).
pub fn validate_float(_m: &mut Mem, _p: u32) {}

/// [soft 0x083f9ba0 SI_byteswap_fread]
/// `fread`, then every item read is swapped when items are 2, 4 or 8 bytes. Items of
/// another size are left as read.
pub fn byteswap_fread(m: &mut Mem, o: &mut dyn Outside, buffer: u32, size: u32, count: u32, stream: u32) -> u32 {
    let n = fread(o, m, buffer, size, count, stream);
    if size != 1 {
        // three pointers step through the buffer, one for each item size
        let (mut p2, mut p4, mut p8) = (buffer, buffer, buffer);
        let mut i = 0u32;
        while i < n {
            match size {
                4 => swap4byte(m, p4),
                2 => swap2byte(m, p2),
                8 => swap8byte(m, p8),
                _ => {}
            }
            p8 = p8.wrapping_add(8);
            p4 = p4.wrapping_add(4);
            p2 = p2.wrapping_add(2);
            i += 1;
        }
    }
    n
}

/// [soft 0x083f9a7c SI_float_fread]
/// `fread`, then every item read is swapped as four bytes, whatever the item size given.
pub fn float_fread(m: &mut Mem, o: &mut dyn Outside, buffer: u32, size: u32, count: u32, stream: u32) -> u32 {
    let n = fread(o, m, buffer, size, count, stream);
    let mut p = buffer;
    let mut i = 0u32;
    while i < n {
        swap4byte(m, p);
        validate_float(m, p);
        p = p.wrapping_add(4);
        i += 1;
    }
    n
}

/// [soft 0x083f9878 SI_fix_SI_FileChunk]
/// A file header's two leading words (the magic number and the version) swapped.
pub fn fix_file_chunk(m: &mut Mem, chunk: u32) {
    swap4byte(m, chunk);
    swap4byte(m, chunk.wrapping_add(4));
}

/// [soft 0x083faa40 SI_FileChunk_fread]
/// `fread`, then every header read (items of 0x58 bytes, whatever the item size given) has
/// its two leading words swapped.
pub fn file_chunk_fread(m: &mut Mem, o: &mut dyn Outside, buffer: u32, size: u32, count: u32, stream: u32) -> u32 {
    let n = fread(o, m, buffer, size, count, stream);
    let mut p = buffer;
    let mut i = 0u32;
    while i < n {
        fix_file_chunk(m, p);
        validate_float(m, p.wrapping_add(4));
        p = p.wrapping_add(0x58);
        i += 1;
    }
    n
}
