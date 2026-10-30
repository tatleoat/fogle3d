// SPDX-License-Identifier: MIT
//! The rest of the small routines of `trimUtils.c` (package 15 and 16, finding 0044 section trim1): the trim-status
//! record and its global copy, the clipping stack, the line table of a status, the borders of a surface and the
//! link-up of separate pieces of a projected curve. The status record (0x128 bytes) has the layout of the global
//! at 0x088222a0 (`trimStatus`): the record is copied field by field to and from it. Same trim-line records as
//! [`crate::trimutils`]. The printing and dialog routines of the file (`si_printTrimLine`, `si_nurbsPrintTrimLine`,
//! `si_printTrimStatus`, `SI_nurbsSrfHighlightTrimCurve`) are not ported: they print or draw.

use si_mem::Mem;
use si_x87::F80;

/// `trimStatus` and the fields after it: the global copy of the current status.
const STATUS: u32 = 0x0882_22a0;
/// 1e-6 (0x08661b38), the tolerance of [`pts_on_border`].
const BORDER_EPS: f64 = 1e-6;
/// 1e-4 (0x08661b40), the tolerance of [`across_border`].
const ACROSS_EPS: f64 = 1e-4;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// A 32-bit float word through the x87 stack (`fld dword` then `fstp dword`). The emulator the checks run on leaves
/// every word, a signalling NaN included, as it was; a real x87 would set the quiet bit of a signalling NaN (not
/// compared: no status field holds one).
fn through_x87(w: u32) -> u32 {
    w
}

/// [soft 0x084f7340 si_writeStack]: `(stack, ptr, d1, d2, d3, d4, s1, s2, s3)`: the pointer at +0, the doubles at
/// +4, +0xc, +0x14, +0x1c and the shorts at +0x24, +0x26, +0x28.
pub fn si_write_stack(m: &mut Mem, stack: u32, ptr: u32, d: [f64; 4], s: [i16; 3]) {
    for (k, v) in d.iter().enumerate() {
        m.set_f64(stack + 4 + 8 * k as u32, *v);
    }
    m.set_u32(stack, ptr);
    for (k, v) in s.iter().enumerate() {
        m.set_i16(stack + 0x24 + 2 * k as u32, *v);
    }
}

/// [soft 0x084f7384 si_readStack]: `(stack, o1..o4, p1..p3)`: the reverse of [`si_write_stack`], into the doubles
/// at `o1..o4` and the shorts at `p1..p3`.
pub fn si_read_stack(m: &mut Mem, stack: u32, o: [u32; 4], p: [u32; 3]) {
    for (k, at) in o.iter().enumerate() {
        let v = m.f64(stack + 4 + 8 * k as u32);
        m.set_f64(*at, v);
    }
    for (k, at) in p.iter().enumerate() {
        let v = m.i16(stack + 0x24 + 2 * k as u32);
        m.set_i16(*at, v);
    }
}

/// [soft 0x084f73d4 si_popStack]: frees the pointer at +0 of the stack when it is set; clears it.
pub fn si_pop_stack(m: &mut Mem, stack: u32) {
    let p = m.u32(stack);
    if p != 0 {
        m.free(p);
        m.set_u32(stack, 0);
    }
    m.set_u32(stack, 0);
}

/// [soft 0x084f7594 SI_initTrimStatus]: the global status gets its defaults: +0 = 1 (short), +2 = 0 (short), +4, +8,
/// +0xc = 0, +0x10, +0x14, +0x18 = 1.0f, +0x20 = 10, +0x24 = 1, +0x78, +0x7c, +0x118, +0x11c = 0, +0x120 = 2 and
/// the 20 words from +0x28 to +0x74 cleared.
pub fn si_init_trim_status(m: &mut Mem) {
    let g = STATUS;
    m.set_i16(g, 1);
    m.set_u32(g + 0xc, 0);
    m.set_u32(g + 8, 0);
    m.set_u32(g + 4, 0);
    m.set_i16(g + 2, 0);
    m.set_u32(g + 0x11c, 0);
    m.set_u32(g + 0x20, 10);
    m.set_u32(g + 0x24, 1);
    m.set_u32(g + 0x120, 2);
    m.set_u32(g + 0x78, 0);
    m.set_u32(g + 0x7c, 0);
    m.set_u32(g + 0x118, 0);
    for off in [0x18u32, 0x14, 0x10] {
        m.set_u32(g + off, 0x3f80_0000);
    }
    for k in 0..20u32 {
        m.set_u32(g + 0x74 - 4 * k, 0);
    }
}

/// The fields a status record and the global share (the offsets of [`si_update_trim_status`] and
/// [`si_trim_stat_allocate`]); the first six words go through the x87 stack.
fn copy_status(m: &mut Mem, from: u32, to: u32) {
    let w = m.u16(from);
    m.set_u16(to, w);
    let w = m.u16(from + 2);
    m.set_u16(to + 2, w);
    let w = m.u32(from + 0x11c);
    m.set_u32(to + 0x11c, w);
    for off in [4u32, 8, 0xc, 0x10, 0x14, 0x18] {
        let w = through_x87(m.u32(from + off));
        m.set_u32(to + off, w);
    }
    for off in [0x20u32, 0x24, 0x78, 0xc0, 0x120] {
        let w = m.u32(from + off);
        m.set_u32(to + off, w);
    }
}

/// [soft 0x084f7620 si_updateTrimStatus]: copies the fields of a status record into the global status.
pub fn si_update_trim_status(m: &mut Mem, rec: u32) {
    copy_status(m, rec, STATUS);
}

/// [soft 0x084f7840 si_trimStatAllocate]: a zeroed status record (`SI_calloc(1, 0x128)`) with the global status's
/// fields copied in, the line table (+0x114), +0x118 and +0x7c cleared, and the 20 words at +0x28 and at +0xc4
/// cleared.
pub fn si_trim_stat_allocate(m: &mut Mem) -> u32 {
    let r = m.si_calloc(1, 0x128);
    copy_status(m, STATUS, r);
    m.set_u32(r + 0x7c, 0);
    m.set_u32(r + 0x118, 0);
    m.set_u32(r + 0x114, 0);
    for k in 0..20u32 {
        m.set_u32(r + 0x28 + 4 * k, 0);
        m.set_u32(r + 0xc4 + 4 * k, 0);
    }
    r
}

/// [soft 0x084f76e4 si_trimStatDispose]: frees the arrays at +0x118 and +0x124; when there is a line table (+0x114)
/// deletes the first `count` (+0x24) lines of it ([`crate::trimutils::si_del_trim_line`]) and frees the table;
/// then frees the record (when not null).
pub fn si_trim_stat_dispose(m: &mut Mem, st: u32) {
    for off in [0x118u32, 0x124] {
        let p = m.u32(st + off);
        if p != 0 {
            m.free(p);
            m.set_u32(st + off, 0);
        }
    }
    if m.u32(st + 0x114) != 0 {
        let mut i = 0i32;
        if 0 < m.i32(st + 0x24) {
            loop {
                let t = m.u32(st + 0x114);
                let line = m.u32(t.wrapping_add((i as u32).wrapping_mul(4)));
                crate::trimutils::si_del_trim_line(m, line);
                i += 1;
                if i >= m.i32(st + 0x24) {
                    break;
                }
            }
        }
        let t = m.u32(st + 0x114);
        if t != 0 {
            m.free(t);
            m.set_u32(st + 0x114, 0);
        }
    }
    if st != 0 {
        m.free(st);
    }
}

/// [soft 0x084f7794 si_trimLineAllocate]: the line table of a status: 50 pointers (`SI_calloc(0x32, 4)`, at +0x114),
/// and for each of the first `count` (+0x24) entries a zeroed 0x30-byte line (`SI_calloc(1, 0x30)`) whose first
/// node (+8), count (+0) and +4 are cleared.
pub fn si_trim_line_allocate(m: &mut Mem, st: u32) {
    let t = m.si_calloc(0x32, 4);
    m.set_u32(st + 0x114, t);
    let mut i = 0i32;
    while i < m.i32(st + 0x24) {
        let line = m.si_calloc(1, 0x30);
        let t = m.u32(st + 0x114);
        let at = t.wrapping_add((i as u32).wrapping_mul(4));
        m.set_u32(at, line);
        m.set_u32(line + 4, 0);
        m.set_u32(line, 0);
        m.set_u32(line + 8, 0);
        i += 1;
    }
}

/// [soft 0x084f795c si_trmDataRem]: `(st, line_index, node)`: removes (and frees) the node after `node` in the line
/// `st`'s table holds at `line_index`; for a null `node` the first node of the line. The line's count (+0) goes
/// down by one. (A missing node is read through, as the original does.)
pub fn si_trm_data_rem(m: &mut Mem, st: u32, line_index: i32, node: u32) {
    let table = m.u32(st + 0x114);
    let line = m.u32(table.wrapping_add((line_index as u32).wrapping_mul(4)));
    let victim;
    if node == 0 {
        victim = m.u32(line + 8);
        let next = m.u32(victim.wrapping_add(0x14));
        m.set_u32(line + 8, next);
    } else {
        victim = m.u32(node + 0x14);
        let next = m.u32(victim.wrapping_add(0x14));
        m.set_u32(node + 0x14, next);
    }
    if victim != 0 {
        m.free(victim);
    }
    let table = m.u32(st + 0x114);
    let line = m.u32(table.wrapping_add((line_index as u32).wrapping_mul(4)));
    let c = m.i32(line).wrapping_sub(1);
    m.set_i32(line, c);
}

/// [soft 0x084f7a7c si_countNumLines]: counts the lines of the table with a positive count (+0); frees and clears
/// the table entries from that number up to the old `count` (+0x24) and sets the count (+0x24) to the number
/// found. (The lines are not packed: only the tail is freed.)
pub fn si_count_num_lines(m: &mut Mem, st: u32) {
    let mut n = 0i32;
    let mut i = 0i32;
    while i < m.i32(st + 0x24) {
        let t = m.u32(st + 0x114);
        let line = m.u32(t.wrapping_add((i as u32).wrapping_mul(4)));
        if 0 < m.i32(line) {
            n += 1;
        }
        i += 1;
    }
    let mut i = n;
    while i < m.i32(st + 0x24) {
        let t = m.u32(st + 0x114);
        let at = t.wrapping_add((i as u32).wrapping_mul(4));
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            let t = m.u32(st + 0x114);
            m.set_u32(t.wrapping_add((i as u32).wrapping_mul(4)), 0);
        }
        i += 1;
    }
    m.set_i32(st + 0x24, n);
}

/// [soft 0x084f8c5c getCrvIndex]: `(n, a, t)`: the index of the first of the `n` longs at `a` that is greater than
/// `t` (index 0 first, tested as `t < a[0]`), -1 when none is (or `n <= 0`).
pub fn get_crv_index(m: &Mem, n: i32, a: u32, t: i32) -> i32 {
    if n <= 0 {
        return -1;
    }
    if t < m.i32(a) {
        return 0;
    }
    let mut i = 0i32;
    loop {
        i += 1;
        if n <= i {
            return -1;
        }
        if m.i32(a.wrapping_add((i as u32).wrapping_mul(4))) > t {
            return i;
        }
    }
}

/// [soft 0x084f8c00 updateStepAndOrder]: `(comp, crv)`: with both non-null, `crv`'s +0x34 and +0 become the largest
/// +0x34 and the largest +0 of the `comp`'s pieces (the pointer array at +8, `count` at +4).
pub fn update_step_and_order(m: &mut Mem, comp: u32, crv: u32) {
    if comp == 0 || crv == 0 {
        return;
    }
    let arr = m.u32(comp + 8);
    let first = m.u32(arr);
    let mut step = m.i32(first + 0x34);
    let mut order = m.i32(first);
    let mut i = 1u32;
    while (i as i32) < m.i32(comp + 4) {
        let arr = m.u32(comp + 8);
        let p = m.u32(arr.wrapping_add(4 * i));
        let s = m.i32(p + 0x34);
        if step < s {
            step = s;
        }
        let o = m.i32(p);
        if order < o {
            order = o;
        }
        i += 1;
    }
    m.set_i32(crv + 0x34, step);
    m.set_i32(crv, order);
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    (x(a) - x(b)).abs().lt(x(eps))
}

/// [soft 0x084f8810 ptsOnBorder]: `(a, b, c, d, e, f)`: 1 when |a - c| < 1e-6 or |d - a| < 1e-6 or |b - e| < 1e-6 or
/// |f - b| < 1e-6 (80-bit differences), else 0.
pub fn pts_on_border(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> i32 {
    (near(a, c, BORDER_EPS) || near(d, a, BORDER_EPS) || near(b, e, BORDER_EPS) || near(f, b, BORDER_EPS)) as i32
}

/// [soft 0x084f888c acrossBorder]: `(srf, p, q)`: 1 when the two points (x at +0, y at +8, each 0x24 bytes) lie across
/// a closed direction of the surface: with +0x20 set, x of the two within 1e-4 of the ends (+0x10, +0x18) in either
/// order and their y within 1e-4 of each other; or, with +0x44 set, y of the two within 1e-4 of the ends (+0x34, +0x3c)
/// in either order and their x within 1e-4 of each other. 0 otherwise.
pub fn across_border(m: &Mem, srf: u32, p: u32, q: u32) -> i16 {
    let e = ACROSS_EPS;
    if m.i16(srf + 0x20) != 0 {
        let (u0, u1) = (m.f64(srf + 0x10), m.f64(srf + 0x18));
        let (px, qx) = (m.f64(p), m.f64(q));
        let ends = (near(px, u0, e) && near(qx, u1, e)) || (near(px, u1, e) && near(qx, u0, e));
        if ends && near(m.f64(p + 8), m.f64(q + 8), e) {
            return 1;
        }
    }
    if m.i16(srf + 0x44) != 0 {
        let (v0, v1) = (m.f64(srf + 0x34), m.f64(srf + 0x3c));
        let (py, qy) = (m.f64(p + 8), m.f64(q + 8));
        let ends = (near(py, v0, e) && near(qy, v1, e)) || (near(py, v1, e) && near(qy, v0, e));
        if ends && near(m.f64(p), m.f64(q), e) {
            return 1;
        }
    }
    0
}

/// [soft 0x084f89b8 linkSepCrv]: `(srf, pts, idx, n, k, out_start, out_count, out_closed)`: with `idx` the array of
/// `n` break indices into `pts` (0x24-byte points), from the piece at `k` walks the following pairs, pulling in
/// every pair whose first point lies across the border from the end of the run ([`across_border`]): the pair's
/// start index is bumped by one, the counts are added and the pair is swapped next to the run. `*out_start` gets the
/// last run index, `*out_count` the point count and `*out_closed` 1 when the run's end lies across from the first piece
/// (the count and the index then go down by one), else 0.
pub fn link_sep_crv(m: &mut Mem, srf: u32, pts: u32, idx: u32, n: i32, k: i32, out_start: u32, out_count: u32, out_closed: u32) {
    let at = |i: i32| idx.wrapping_add((i as u32).wrapping_mul(4));
    let pt = |m: &Mem, i: i32| pts.wrapping_add((m.i32(at(i)) as u32).wrapping_mul(0x24));
    let mut r = k + 1;
    m.set_i32(out_start, r);
    let mut j = k + 2;
    let c = m.i32(at(r)).wrapping_sub(m.i32(at(k))).wrapping_add(1);
    m.set_i32(out_count, c);
    if j < n {
        let mut p5 = at(r);
        loop {
            let (a, b) = (pt(m, r), pt(m, j));
            // (the original passes pts + idx[r] * 0x24 with idx[r] read at the time of the call)
            let _ = p5;
            if across_border(m, srf, a, b) == 0 {
                j += 2;
            } else {
                let v = m.i32(at(j));
                m.set_i32(at(j), v.wrapping_add(1));
                let c = m.i32(at(j + 1)).wrapping_sub(v.wrapping_add(1)).wrapping_add(m.i32(out_count)).wrapping_add(1);
                m.set_i32(out_count, c);
                if j != r + 1 {
                    let t = m.i32(p5 + 4);
                    let w = m.i32(at(j));
                    m.set_i32(p5 + 4, w);
                    m.set_i32(at(j), t);
                    let t = m.i32(p5 + 8);
                    let w = m.i32(at(j + 1));
                    m.set_i32(p5 + 8, w);
                    m.set_i32(at(j + 1), t);
                }
                p5 += 8;
                j = r + 3;
                r += 2;
            }
            if j >= n {
                break;
            }
        }
    }
    m.set_i32(out_start, r);
    let (a, b) = (pt(m, r), pt(m, k));
    if across_border(m, srf, a, b) == 0 {
        m.set_i16(out_closed, 0);
    } else {
        let c = m.i32(out_count).wrapping_sub(1);
        m.set_i32(out_count, c);
        let v = m.i32(at(r)).wrapping_sub(1);
        m.set_i32(at(r), v);
        m.set_i16(out_closed, 1);
    }
}
