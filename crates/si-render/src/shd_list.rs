// SPDX-License-Identifier: MIT
//! The six Textlist routines of `shd_fct.c` (finding 0023, section 39): the dialog's list of the
//! shaders of a material or light, on the memory image. A Textlist is 8 bytes: +0 the number of
//! lines (short), +4 a block of 0x14-byte lines; in a line +4 is the text (a `strdup` copy) and
//! +0xc the shader record it stands for (a user-defined function, `__SI_UDF`: +0 the name, +0xc the
//! enable flag (short), +0x10 the kind (short, nonzero = internal)). The list always ends with
//! one extra line, `<< Empty >>`, which has no shader.
//!
//! Where the original would run off a buffer (a line text longer than the 2000-byte stack buffer of
//! the callers; a null block behind a nonnull list) the Rust writes what the C would and leaves
//! the rest to the image's faults.

use si_mem::{clib, Mem};
use si_rt::Table;

/// The strings of `shd_fct.c` (`0x086a2e20..0x086a2eb8`) of the initial
/// `.data`: the list lines' texts and formats and the four pointer variables to them, then the
/// dialog glue's one-byte empty strings and messages (`shd_ui.rs`), so that a bare image can run the routines.
const STRINGS_ADDR: u32 = 0x086a_2e20;
/// Read from the user's program at run time (si-rt), checked against our FNV-1a of the range.
static STRINGS: Table = Table::new("si-render shd_fct.c strings", STRINGS_ADDR, 0x98, 0x1a28_f9b7_4cf3_097a);
/// `strEMPTY`, `strFORMAT`, `strFORMATINT`, `strNONAME`: pointer variables.
const STR_EMPTY: u32 = 0x086a_2e34;
const STR_FORMAT: u32 = 0x086a_2e44;
const STR_FORMATINT: u32 = 0x086a_2e54;
const STR_NONAME: u32 = 0x086a_2e64;

/// Puts those strings into the image (the program's loader has them already).
pub fn store_strings(m: &mut Mem) {
    m.set_bytes(STRINGS_ADDR, STRINGS.get());
}

/// `strdup` of a line text, as every caller does after formatting (the original formats into a
/// 2000-byte stack buffer; a longer text overruns it there, here it does not).
fn line_copy(m: &mut Mem, udf: u32) -> u32 {
    let text = line_text(m, udf);
    let p = m.malloc(text.len() as u32 + 1);
    if p != 0 {
        m.put_cstr(p, &text);
    }
    p
}

/// The line text: the format string of the image with its `%c` and `%s` filled (the only two
/// conversions the two formats hold).
fn line_text(m: &Mem, udf: u32) -> Vec<u8> {
    let mark = if m.u16(udf.wrapping_add(0xc)) != 0 { b'X' } else { b' ' };
    let name = m.u32(udf);
    let name = if name != 0 && m.u8(name) != 0 { name } else { m.u32(STR_NONAME) };
    let internal = m.u16(udf.wrapping_add(0x10)) != 0;
    let fmt = m.cstr(m.u32(if internal { STR_FORMATINT } else { STR_FORMAT }));
    let mut out = Vec::new();
    let mut args: Vec<u8> = if internal { vec![] } else { vec![mark] };
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] == b'%' && i + 1 < fmt.len() {
            match fmt[i + 1] {
                b'c' => {
                    out.extend(args.drain(..));
                }
                b's' => out.extend(m.cstr(name)),
                c => out.extend([b'%', c]),
            }
            i += 2;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    out
}

/// [soft 0x082a5b90 shdlistLineSprintf]: writes the line of a shader to `out`: `[X]    name` (`[ ]`
/// when its enable flag at +0xc is 0) for a user effect, `       name` for an internal one (kind
/// at +0x10 nonzero); a null or empty name reads `<<no name>>`. Nothing for a null argument.
pub fn shdlist_line_sprintf(m: &mut Mem, udf: u32, out: u32) {
    if out == 0 || udf == 0 {
        return;
    }
    let text = line_text(m, udf);
    m.put_cstr(out, &text);
}

/// `n` as the sign-extended short the original reads.
fn short(m: &Mem, a: u32) -> i32 {
    i32::from(m.u16(a) as i16)
}

/// [soft 0x082a6044 SI_shdlistFill]: a list of the `count` shader records of the array `udfs`
/// (null entries skipped), closed by the `<< Empty >>` line; null for a negative count or no
/// memory. Reproduced quirk: after a skipped entry the line is written at the position of the
/// array index, not of the number of lines (the original's offset runs ahead of the count),
/// and the closing line's text goes to the array index too.
pub fn si_shdlist_fill(m: &mut Mem, udfs: u32, count: i32) -> u32 {
    if count < 0 {
        return 0;
    }
    let list = m.si_calloc(1, 8);
    if list == 0 {
        return 0;
    }
    m.set_u16(list, 0);
    m.set_u32(list.wrapping_add(4), 0);
    let mut i: i32 = 0;
    if count > 0 && udfs != 0 {
        let mut off: u32 = 0;
        loop {
            let udf = m.u32(udfs.wrapping_add((i as u32).wrapping_shl(2)));
            if udf != 0 {
                let n = short(m, list);
                let block = m.si_realloc(m.u32(list.wrapping_add(4)), (n.wrapping_mul(5).wrapping_add(5) as u32).wrapping_shl(2));
                m.set_u32(list.wrapping_add(4), block);
                if block == 0 {
                    return 0;
                }
                clib::memset(m, block.wrapping_add((n as u32).wrapping_mul(0x14)), 0, 0x14);
                let copy = line_copy(m, udf);
                let block = m.u32(list.wrapping_add(4));
                m.set_u32(block.wrapping_add(off).wrapping_add(4), copy);
                m.set_u32(block.wrapping_add(off).wrapping_add(0xc), udf);
                m.set_u16(list, (n + 1) as u16);
            }
            off = off.wrapping_add(0x14);
            i += 1;
            if i >= count {
                break;
            }
        }
    }
    let n = short(m, list);
    let block = m.si_realloc(m.u32(list.wrapping_add(4)), (n.wrapping_mul(5).wrapping_add(5) as u32).wrapping_shl(2));
    m.set_u32(list.wrapping_add(4), block);
    if block == 0 {
        return 0;
    }
    clib::memset(m, block.wrapping_add((n as u32).wrapping_mul(0x14)), 0, 0x14);
    let copy = clib::strdup(m, m.u32(STR_EMPTY));
    let block = m.u32(list.wrapping_add(4));
    m.set_u32(block.wrapping_add((i as u32).wrapping_mul(0x14)).wrapping_add(4), copy);
    m.set_u16(list, (n + 1) as u16);
    list
}

/// [soft 0x082a61b0 SI_shdlistAdd]: puts the shader `udf` in front of the closing line. False for
/// a null list or shader or a list without lines; when the block cannot grow its pointer is
/// lost (set to 0) and the answer is false.
pub fn si_shdlist_add(m: &mut Mem, list: u32, udf: u32) -> bool {
    if list == 0 || udf == 0 || short(m, list) < 1 {
        return false;
    }
    let n = short(m, list);
    let block = m.si_realloc(m.u32(list.wrapping_add(4)), (n.wrapping_mul(5).wrapping_add(5) as u32).wrapping_shl(2));
    m.set_u32(list.wrapping_add(4), block);
    if block != 0 {
        let at = ((short(m, list) - 1) as u32).wrapping_mul(0x14);
        clib::memmove(m, block.wrapping_add((short(m, list) as u32).wrapping_mul(0x14)), at.wrapping_add(block), 0x14);
        clib::memset(m, at.wrapping_add(m.u32(list.wrapping_add(4))), 0, 0x14);
        let b = m.u32(list.wrapping_add(4));
        m.set_u32(b.wrapping_add(at).wrapping_add(0xc), udf);
        let copy = line_copy(m, udf);
        let b = m.u32(list.wrapping_add(4));
        m.set_u32(b.wrapping_add(at).wrapping_add(4), copy);
        let n = short(m, list);
        m.set_u16(list, (n + 1) as u16);
    }
    m.u32(list.wrapping_add(4)) != 0
}

/// [soft 0x082a6278 SI_shdlistDelete]: removes line `index` (not the last, the closing line, since
/// `index <= lines - 1` is allowed, it can be), freeing its text and closing the gap. True when
/// the block is still there afterwards; false for a negative or too large index or a null list.
pub fn si_shdlist_delete(m: &mut Mem, list: u32, index: i32) -> bool {
    if index < 0 || list == 0 || index > short(m, list) - 1 {
        return false;
    }
    let off = (index as u32).wrapping_mul(0x14);
    let block = m.u32(list.wrapping_add(4));
    let text = m.u32(block.wrapping_add(4).wrapping_add(off));
    if text != 0 {
        m.free(text);
        let block = m.u32(list.wrapping_add(4));
        m.set_u32(block.wrapping_add(4).wrapping_add(off), 0);
    }
    let tail = short(m, list) - index - 1;
    if tail > 0 {
        let block = m.u32(list.wrapping_add(4));
        clib::memmove(m, block.wrapping_add(off), block.wrapping_add(0x14).wrapping_add(off), (tail as u32).wrapping_mul(0x14));
    }
    let n = short(m, list) - 1;
    m.set_u16(list, n as u16);
    let block = m.si_realloc(m.u32(list.wrapping_add(4)), (n as u32).wrapping_mul(0x14));
    m.set_u32(list.wrapping_add(4), block);
    block != 0
}

/// [soft 0x082a632c SI_shdlistMoveup]: swaps line `index` with the one before it; only for
/// `0 < index < lines - 1` and more than two lines (the closing line never moves).
pub fn si_shdlist_moveup(m: &mut Mem, list: u32, index: i32) {
    if list != 0 && index > 0 && index < short(m, list) - 1 && short(m, list) > 2 {
        let at = m.u32(list.wrapping_add(4)).wrapping_add((index as u32).wrapping_mul(0x14));
        let mut tmp = [0u32; 5];
        for (k, t) in tmp.iter_mut().enumerate() {
            *t = m.u32(at.wrapping_add(4 * k as u32));
        }
        for k in 0..5u32 {
            let v = m.u32(at.wrapping_sub(0x14).wrapping_add(4 * k));
            m.set_u32(at.wrapping_add(4 * k), v);
        }
        for (k, t) in tmp.iter().enumerate() {
            m.set_u32(at.wrapping_sub(0x14).wrapping_add(4 * k as u32), *t);
        }
    }
}

/// [soft 0x082a6390 SI_shdlistModify]: line `index` gets the text of shader `udf` and points to
/// it; nothing for a negative index or a null shader. The index is not checked against the count.
pub fn si_shdlist_modify(m: &mut Mem, list: u32, index: i32, udf: u32) {
    if index < 0 || udf == 0 {
        return;
    }
    let off = (index as u32).wrapping_mul(0x14);
    let block = m.u32(list.wrapping_add(4));
    let text = m.u32(block.wrapping_add(4).wrapping_add(off));
    if text != 0 {
        m.free(text);
        let block = m.u32(list.wrapping_add(4));
        m.set_u32(block.wrapping_add(4).wrapping_add(off), 0);
    }
    let copy = line_copy(m, udf);
    let block = m.u32(list.wrapping_add(4));
    m.set_u32(block.wrapping_add(4).wrapping_add(off), copy);
    m.set_u32(block.wrapping_add(0xc).wrapping_add(off), udf);
}
