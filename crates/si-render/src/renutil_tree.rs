// SPDX-License-Identifier: MIT
//! The tree statistics, the tree dump and the incrustation scan line of `renutil.c` on the memory
//! image (finding 0023, section 37). They read the program's globals (the statistics tables, the
//! tree's root, the bounding box, the 7x13 character bitmap table) and write through `fprintf` and
//! `printf` of the C library (recorded calls: a `double` is two words, low word first).
//! Class: model (the x87 arithmetic of the leaf volumes, the truncating conversions).

use si_mem::{Mem, Outside, Routine};
use si_x87::{x, Round};

const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
const FFLUSH: Routine = Routine::new("fflush", 0x080c_3be8);
const PRINTF: Routine = Routine::new("printf", 0x080c_48b8);

/// `contents`: how many leaves hold `n` triangles (5000 entries); `level`: leaves by depth (1000).
const CONTENTS: u32 = 0x0879_32e0;
const LEVEL: u32 = 0x0879_8100;
/// `emptyVolumn` (a float).
const EMPTY_VOLUME: u32 = 0x0869_c1e0;
/// The root of the tree, the stream the statistics go to and the bounding box (six floats).
const ROOT: u32 = 0x0879_0928;
const STREAM: u32 = 0x0879_0dd0;
const BOX: u32 = 0x0879_08f4;
/// The text, mode and bitmap table of the incrustration, and the second mode.
const TEXT: u32 = 0x0879_cc04;
const MODE: u32 = 0x0879_cc02;
const FONT: u32 = 0x0879_cc08;
const MODE2: u32 = 0x0879_d908;

mod text {
    pub const TITLE: u32 = 0x0869_c30b;
    pub const TRIANGLES: u32 = 0x0869_c31c;
    pub const NEWLINE: u32 = 0x0869_c337;
    pub const EMPTY: u32 = 0x0869_c340;
    pub const NEWLINE2: u32 = 0x0869_c361;
    pub const DEPTH: u32 = 0x0869_c363;
    pub const NEWLINE3: u32 = 0x0869_c378;
    pub const SPACE: u32 = 0x0869_c37a;
    pub const LEVEL: u32 = 0x0869_c37c;
    pub const LEAF: u32 = 0x0869_c380;
    pub const SPACE2: u32 = 0x0869_c39d;
    pub const LEVEL2: u32 = 0x0869_c39f;
    pub const ZSPLIT: u32 = 0x0869_c3aa;
    pub const YSPLIT: u32 = 0x0869_c3a3;
    pub const XSPLIT: u32 = 0x0869_c3b1;
    pub const PLANE: u32 = 0x0869_c3b8;
}

fn dbl(bits: u64) -> [u32; 2] {
    [bits as u32, (bits >> 32) as u32]
}

/// The volume of a leaf's box, `(dx * dy) * dz` in the register: the box corners are the vertices at
/// +0x0c..+0x14 (low) and +0x18..+0x20 (high), each a pointer to a vertex whose x is at +8.
fn leaf_volume(m: &Mem, node: u32) -> si_x87::F80 {
    let c = |off: u32| m.f32(m.u32(node + off).wrapping_add(8));
    let d1 = x(c(0x18)) - x(c(0x0c));
    let d2 = x(c(0x1c)) - x(c(0x10));
    let d3 = x(c(0x20)) - x(c(0x14));
    d1 * d2 * d3
}

/// The length of the list behind a leaf's pointer at +8 (links at +0x1c).
fn chain(m: &Mem, node: u32) -> i32 {
    let mut n = 0;
    let mut p = m.u32(node + 8);
    while p != 0 {
        p = m.u32(p + 0x1c);
        n += 1;
    }
    n
}

/// The tree's statistics, one node [soft 0x08260d70 branchStatus]: a leaf (tag 0) counts into
/// `contents[length of its list]` and `level[its depth]` and, when its list is empty, adds its box's
/// volume to `emptyVolumn` (a float); a node of tag 1..3 visits its two children.
pub fn branch_status(m: &mut Mem, node: u32) {
    let tag = m.u16(node) as i16;
    if tag == 0 {
        let n = chain(m, node);
        let a = CONTENTS.wrapping_add((n as u32).wrapping_mul(4));
        let v = m.u32(a).wrapping_add(1);
        m.set_u32(a, v);
        let depth = m.i16(node + 2) as i32;
        let a = LEVEL.wrapping_add((depth as u32).wrapping_mul(4));
        let v = m.u32(a).wrapping_add(1);
        m.set_u32(a, v);
        if n == 0 {
            let sum = leaf_volume(m, node) + x(m.f32(EMPTY_VOLUME));
            m.set_f32(EMPTY_VOLUME, sum.to_f32());
        }
    } else if (1..=3).contains(&tag) {
        let (a, b) = (m.u32(node + 0xc), m.u32(node + 0x10));
        branch_status(m, a);
        branch_status(m, b);
    }
}

/// The statistics of the whole tree, printed [soft 0x08260c10 si_treestatus]: clears the first 500
/// entries of `contents` and 100 of `level`, visits the tree from its root, prints the counts and the
/// share of the box that is empty.
pub fn tree_status(m: &mut Mem, o: &mut dyn Outside) {
    for i in 0..500u32 {
        m.set_u32(CONTENTS + 4 * i, 0);
    }
    for i in 0..100u32 {
        m.set_u32(LEVEL + 4 * i, 0);
    }
    let root = m.u32(ROOT);
    branch_status(m, root);
    let stream = m.u32(STREAM);
    o.call(m, FPRINTF, &[stream, text::TITLE]);
    for i in 0..=0x1387u32 {
        let c = m.i32(CONTENTS + 4 * i);
        if c > 0 {
            o.call(m, FPRINTF, &[stream, text::TRIANGLES, i, c as u32]);
        }
    }
    o.call(m, FPRINTF, &[stream, text::NEWLINE]);
    let b = |k: u32| x(m.f32(BOX + k));
    let (a, bb) = (b(4), b(12));
    let ax = a - b(0);
    let by = bb - b(8);
    let cz = x(m.f32(BOX + 0x14)) - x(m.f32(BOX + 0x10));
    let ratio = x(m.f32(EMPTY_VOLUME)) / (ax * by * cz);
    m.set_f32(EMPTY_VOLUME, ratio.to_f32());
    let d = dbl(ratio.to_f64().to_bits());
    o.call(m, FPRINTF, &[stream, text::EMPTY, d[0], d[1]]);
    o.call(m, FPRINTF, &[stream, text::NEWLINE2]);
    for i in 0..=999u32 {
        let c = m.i32(LEVEL + 4 * i);
        if c > 0 {
            o.call(m, FPRINTF, &[stream, text::DEPTH, i, c as u32]);
        }
    }
    o.call(m, FPRINTF, &[stream, text::NEWLINE3]);
    o.call(m, FFLUSH, &[stream]);
}

/// The tree printed to standard output, indented by depth [soft 0x08260e0c writeTree]: a leaf
/// (tag 0) as `<depth> leaf <triangles> triangles volumn <v>`; any other tag (a split) prints its
/// first child, its own line (`<depth> XSPLIT|YSPLIT|ZSPLIT plane <v>`: tag 1 X, 2 Y, else Z) and its
/// second child.
pub fn write_tree(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    if m.u16(node) == 0 {
        let n = chain(m, node);
        let depth = m.i16(node + 2) as i32;
        let mut i = 0;
        while i < depth {
            o.call(m, PRINTF, &[text::SPACE]);
            i += 1;
        }
        o.call(m, PRINTF, &[text::LEVEL, m.i16(node + 2) as i32 as u32]);
        let d = dbl(leaf_volume(m, node).to_f64().to_bits());
        o.call(m, PRINTF, &[text::LEAF, n as u32, d[0], d[1]]);
    } else {
        let first = m.u32(node + 0xc);
        write_tree(m, o, first);
        let mut i = 0;
        if m.i16(node + 2) > 0 {
            loop {
                o.call(m, PRINTF, &[text::SPACE2]);
                i += 1;
                if i >= m.i16(node + 2) as i32 {
                    break;
                }
            }
        }
        o.call(m, PRINTF, &[text::LEVEL2, m.i16(node + 2) as i32 as u32]);
        let d = dbl(f64::from(m.f32(node + 8)).to_bits());
        let name = match m.u16(node) {
            1 => text::XSPLIT,
            2 => text::YSPLIT,
            _ => text::ZSPLIT,
        };
        o.call(m, PRINTF, &[text::PLANE, name, d[0], d[1]]);
        let second = m.u32(node + 0x10);
        write_tree(m, o, second);
    }
}

/// The scan line of the incrustration's text at one row of its 7x13 characters [soft 0x082613ac
/// createScanLine]: `(row, width)` returns a block of `width + 1` bytes (the text's characters are
/// drawn as 0xff for a set bit and 0 for a clear one, from a column that depends on the mode: 1 and 3
/// from the right edge, 0 and 2 from the left; 100 elsewhere). The original leaves the start column
/// unset for other modes (not reproduced; the generator makes none).
pub fn create_scan_line(m: &mut Mem, row: i32, width: i32) -> u32 {
    let text = m.u32(TEXT);
    let mut len: i32 = 0;
    while m.u8(text.wrapping_add(len as u32)) != 0 {
        len += 1;
    }
    let mode = m.u16(MODE) as i16;
    // The original leaves the column unset for a mode other than 0..3 (a stale stack word);
    // Rust takes 0.
    let mut pos: i32 = 0;
    if mode == 1 || mode == 3 {
        pos = width.wrapping_sub(len.wrapping_mul(7)).wrapping_sub(1);
        if pos < 0 {
            pos = 0;
        }
    }
    let buf = m.si_calloc(width.wrapping_add(1) as u32, 1);
    for i in 0..width {
        m.set_u8(buf.wrapping_add(i as u32), 0x64);
    }
    let last = width.wrapping_sub(7);
    let mut c = 0;
    while c < len {
        let ch = i32::from(m.u8(m.u32(TEXT).wrapping_add(c as u32)) as i8);
        let at = ch.wrapping_mul(13).wrapping_sub(1).wrapping_add(row);
        let mut bits = u32::from(m.u16(FONT.wrapping_add((at as u32).wrapping_mul(2))));
        let mut p = buf.wrapping_add(pos as u32);
        for _ in 0..7 {
            m.set_u8(p, if (bits as u16) as i16 >= 0 { 0 } else { 0xff });
            bits = bits.wrapping_add(bits);
            p = p.wrapping_add(1);
        }
        pos = pos.wrapping_add(7);
        if pos > last {
            break;
        }
        c += 1;
    }
    buf
}

/// The incrustation of one output line [soft 0x08261240 si_incrustration]: `(width)` clears or sets
/// the three colour lines (their pointers are the globals at 0x08790b54, 58, 5c) from the text's scan
/// line at the row the mode and the picture's position give. Row 1..=13 only; the original leaves the
/// row unset for modes other than 0..3 (not reproduced).
pub fn incrustration(m: &mut Mem, width: i32) {
    let base = i32::from(m.i16(0x0879_0b3c));
    let (top, left);
    if m.u32(0x0879_0e04) != 0 {
        top = (x(m.f32(0x0879_078c)) - x(m.f32(0x0879_0e14))).to_i32(Round::Zero);
        left = x(m.f32(0x0879_0e18)).to_i32(Round::Zero);
    } else {
        top = x(m.f32(0x0879_078c)).to_i32(Round::Zero);
        left = 0;
    }
    let mode = m.u16(MODE) as i16;
    // Reproduced for 0..=3 only (the original's row is unset otherwise).
    let row = if mode < 0 {
        0
    } else if mode <= 1 {
        13i32.wrapping_sub(top.wrapping_sub(base))
    } else if mode <= 3 {
        base.wrapping_sub(left)
    } else {
        0
    };
    if (row.wrapping_sub(1) as u32) > 12 {
        return;
    }
    let (r, g, b) = (m.u32(0x0879_0b54), m.u32(0x0879_0b58), m.u32(0x0879_0b5c));
    let scan = create_scan_line(m, row, width);
    let mut i = 0i32;
    while i <= width {
        let d = m.u8(scan.wrapping_add(i as u32));
        let mode2 = m.u16(MODE2);
        let (zero, full) = if d == 0 {
            (mode2 == 2, mode2 == 3)
        } else if d == 0xff {
            (mode2 == 1 || mode2 == 3, mode2 == 0 || mode2 == 2)
        } else {
            (false, false)
        };
        let v = if zero {
            Some(0)
        } else if full {
            Some(0xff)
        } else {
            None
        };
        if let Some(v) = v {
            for p in [b, g, r] {
                m.set_u8(p.wrapping_add(i as u32), v);
            }
        }
        i += 1;
    }
    if scan != 0 {
        m.free(scan);
    }
}
