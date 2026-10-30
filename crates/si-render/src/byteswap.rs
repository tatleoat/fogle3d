// SPDX-License-Identifier: MIT
//! Byte order: the six routines of the original's `byteswap.c` (0x083f9320) that the
//! picture writer calls, on the memory image. The program's files are high byte first; on
//! this machine every header goes through these before it is written.
//!
//! Other crates have the same routines on values (`si-file`, `si-picture`, `si-hrc`); here
//! they work where the original works: on the bytes at an address of the image, leaving the
//! last byte moved in the file's own `tmp` (0x08817f38), as the original does.
//!
//! Class of every function here: copy.

use crate::host::FileHost;
use si_mem::Mem;

/// `tmp` (0x08817f38, one byte of `.bss`): the byte the swap routines move through.
pub const TMP: u32 = 0x0881_7f38;

/// Exchanges two bytes of the image through [`TMP`].
fn exchange(m: &mut Mem, a: u32, b: u32) {
    let t = m.u8(a);
    m.set_u8(TMP, t);
    let v = m.u8(b);
    m.set_u8(a, v);
    let t = m.u8(TMP);
    m.set_u8(b, t);
}

/// [soft 0x083f9330 SI_swap2byte]: `SI_swap2byte(p)`: the two bytes at `p` exchanged.
pub fn swap2byte(m: &mut Mem, p: u32) {
    exchange(m, p, p.wrapping_add(1));
}

/// [soft 0x083f9350 SI_swap4byte]: `SI_swap4byte(p)`: bytes 0 and 3, then 1 and 2.
pub fn swap4byte(m: &mut Mem, p: u32) {
    exchange(m, p, p.wrapping_add(3));
    exchange(m, p.wrapping_add(1), p.wrapping_add(2));
}

/// [soft 0x083f9388 SI_swap8byte]: `SI_swap8byte(p)`: bytes 0 and 7, 1 and 6, 2 and 5, 3
/// and 4.
pub fn swap8byte(m: &mut Mem, p: u32) {
    for k in 0..4 {
        exchange(m, p.wrapping_add(k), p.wrapping_add(7 - k));
    }
}

/// [soft 0x083f9878 SI_fix_SI_FileChunk]: `SI_fix_SI_FileChunk(header)`: the magic number
/// and the version word of a file header, each a 32-bit word.
pub fn fix_file_chunk(m: &mut Mem, p: u32) {
    swap4byte(m, p);
    swap4byte(m, p.wrapping_add(4));
}

/// [soft 0x083f98a0 SI_fix_SI_PictChunk]: `SI_fix_SI_PictChunk(chunk)`: width and height
/// (16 bits each), the pixel ratio (32), the fields word and the word after it (16 each).
pub fn fix_pict_chunk(m: &mut Mem, p: u32) {
    swap2byte(m, p);
    swap2byte(m, p.wrapping_add(2));
    swap4byte(m, p.wrapping_add(4));
    swap2byte(m, p.wrapping_add(8));
    swap2byte(m, p.wrapping_add(10));
}

/// One pass of [`byteswap_fwrite`] over the items.
fn swap_items(m: &mut Mem, p: u32, size: u32, count: u32) {
    let mut i = 0u32;
    while i < count {
        match size {
            4 => swap4byte(m, p.wrapping_add(i.wrapping_mul(4))),
            2 => swap2byte(m, p.wrapping_add(i.wrapping_mul(2))),
            8 => swap8byte(m, p.wrapping_add(i.wrapping_mul(8))),
            _ => {}
        }
        i += 1;
    }
}

/// [soft 0x083f9c40 SI_byteswap_fwrite]: `SI_byteswap_fwrite(p, size, count, file)`:
/// `fwrite` with every item's bytes reversed. The `count` items of 2, 4 or 8 bytes are
/// swapped where they lie, written, and swapped back; items of any other size are written
/// as they are. Answers what `fwrite` answers: the number of whole items written.
pub fn byteswap_fwrite(m: &mut Mem, files: &mut dyn FileHost, p: u32, size: u32, count: u32, file: u32) -> u32 {
    swap_items(m, p, size, count);
    let n = size.wrapping_mul(count);
    let written = if n == 0 { 0 } else { files.fwrite(file, m.bytes(p, n)) };
    swap_items(m, p, size, count);
    if size == 0 {
        0
    } else {
        written / size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemFiles;

    #[test]
    fn items_are_written_high_byte_first_and_left_as_they_were() {
        let mut m = Mem::new();
        let p = m.calloc(1, 16);
        m.set_bytes(p, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut h = MemFiles::new();
        let f = h.open_named(b"f", b"", b"w");
        assert_eq!(byteswap_fwrite(&mut m, &mut h, p, 4, 2, f), 2);
        assert_eq!(byteswap_fwrite(&mut m, &mut h, p, 2, 1, f), 1);
        assert_eq!(byteswap_fwrite(&mut m, &mut h, p, 3, 1, f), 1);
        assert_eq!(byteswap_fwrite(&mut m, &mut h, p, 8, 1, f), 1);
        assert_eq!(h.files[&b"f".to_vec()], [4, 3, 2, 1, 8, 7, 6, 5, 2, 1, 1, 2, 3, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(m.bytes(p, 8), [1, 2, 3, 4, 5, 6, 7, 8]);
        // the last byte moved stays in `tmp`
        assert_eq!(m.u8(TMP), 5);
        fix_pict_chunk(&mut m, p);
        assert_eq!(m.bytes(p, 12), [2, 1, 4, 3, 8, 7, 6, 5, 0, 0, 0, 0]);
    }
}
