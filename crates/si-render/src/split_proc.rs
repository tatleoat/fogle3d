// SPDX-License-Identifier: MIT
//! `split_proc.c`: splitting a list of metaballs into the parts that are linked to each other (finding 0012, section 28).
//! The work is done on bit sets of the entries of the list, kept in a ring of nodes (a pointer to a set of
//! `mbListSize` bytes and the next node); the part of the ring from `vcS_start` up to the tail holds the sets found so far.

use crate::volume::*;
use si_mem::Mem;

pub const FIRST_INIT_33: u32 = 0x086e_eb40;
pub const VCS_START: u32 = 0x0882_9d00;
pub const VCS_TMP_LIST1: u32 = 0x0882_9d04;
pub const VCS_TMP_LIST2: u32 = 0x0882_9d08;
/// `vcS_mblTemp`: an index list (count, size, pointer) the group numbers are gathered in.
pub const VCS_MBL_TEMP: u32 = 0x0882_9d0c;
pub const VCS_MB_LIST_SIZE: u32 = 0x0882_9cfc;
pub const VCS_SPLIT_DATA: u32 = 0x0882_9d18;
pub const VCS_REAL_SPLIT_DATA: u32 = 0x0882_9d1c;
pub const VCS_LOCAL_EDGE_LIST: u32 = 0x0882_9d20;

/// Frees every node of the ring (and its bit set), starting at `vcS_start`.
fn free_ring(m: &mut Mem) {
    let start = m.u32(VCS_START);
    let mut n = 1;
    let mut p = m.u32(start + 4);
    while p != start {
        n += 1;
        p = m.u32(p + 4);
    }
    while n > 0 {
        let set = m.u32(p);
        m.free(set);
        let next = m.u32(p + 4);
        m.free(p);
        n -= 1;
        p = next;
    }
}

/// [soft 0x0857a7d0 VC_initSplitData]: `VC_initSplitData(metaballs, edges)`: the scratch data for lists of up to `metaballs` entries
/// and `edges` links; the second and later calls resize. 0, or 0x10 when an allocation fails.
pub fn init_split_data(m: &mut Mem, n: i32, edges: i32) -> i32 {
    if m.i16(FIRST_INIT_33) == 0 {
        for a in [VCS_LOCAL_EDGE_LIST, VCS_MBL_TEMP + 8, VCS_TMP_LIST1, VCS_TMP_LIST2] {
            if a == VCS_MBL_TEMP + 8 {
                let p = m.u32(a);
                m.free(p);
                m.set_u32(VCS_MBL_TEMP + 4, 0);
            } else {
                let p = m.u32(a);
                m.free(p);
            }
        }
        free_ring(m);
        for i in 0..100u32 {
            for g in [VCS_SPLIT_DATA, VCS_REAL_SPLIT_DATA] {
                let base = m.u32(g) + 12 * i;
                m.set_i32(base + 4, n);
                let old = m.u32(base + 8);
                let p = m.realloc(old, (n as u32) << 2);
                m.set_u32(m.u32(g) + 12 * i + 8, p);
                if p == 0 {
                    return 0x10;
                }
            }
        }
    } else {
        for g in [VCS_SPLIT_DATA, VCS_REAL_SPLIT_DATA] {
            let p = m.malloc(0x4b0);
            m.set_u32(g, p);
            if p == 0 {
                return 0x10;
            }
        }
        for i in 0..100u32 {
            for g in [VCS_SPLIT_DATA, VCS_REAL_SPLIT_DATA] {
                let base = m.u32(g) + 12 * i;
                m.set_i32(base + 4, n);
                let p = m.malloc((n as u32) << 2);
                m.set_u32(base + 8, p);
                if p == 0 {
                    return 0x10;
                }
            }
        }
        m.set_i16(FIRST_INIT_33, 0);
    }
    let p = m.malloc((edges as u32) << 3);
    m.set_u32(VCS_LOCAL_EDGE_LIST, p);
    if p == 0 {
        return 0x10;
    }
    let p = m.malloc((n as u32) << 2);
    m.set_u32(VCS_MBL_TEMP + 8, p);
    if p == 0 {
        return 0x10;
    }
    let adj = if n < 0 { n.wrapping_add(7) } else { n };
    let size = (adj >> 3).wrapping_add(1);
    m.set_i32(VCS_MB_LIST_SIZE, size);
    for a in [VCS_TMP_LIST1, VCS_TMP_LIST2] {
        let p = m.malloc(size as u32);
        m.set_u32(a, p);
        if p == 0 {
            return 0x10;
        }
    }
    let start = m.malloc(8);
    m.set_u32(VCS_START, start);
    if start == 0 {
        return 0x10;
    }
    let set = m.malloc(size as u32);
    m.set_u32(start, set);
    if set == 0 {
        return 0x10;
    }
    let mut p = start;
    let mut i = 0;
    loop {
        let q = m.malloc(8);
        m.set_u32(p + 4, q);
        if q == 0 {
            return 0x10;
        }
        let set = m.malloc(size as u32);
        m.set_u32(q, set);
        if set == 0 {
            return 0x10;
        }
        i += 1;
        p = q;
        if i > 3 {
            m.set_u32(q + 4, start);
            return 0;
        }
    }
}

fn bit(m: &Mem, set: u32, i: i32) -> bool {
    let adj = if i < 0 { i.wrapping_add(7) } else { i };
    let byte = m.u8(set.wrapping_add((adj >> 3) as u32)) as i8 as i32;
    byte & (1 << (i.wrapping_sub((adj >> 3).wrapping_mul(8)) & 31)) != 0
}

fn set_bit(m: &mut Mem, set: u32, i: i32, on: bool) {
    let adj = if i < 0 { i.wrapping_add(7) } else { i };
    let at = set.wrapping_add((adj >> 3) as u32);
    let mask = (1u32 << (i.wrapping_sub((adj >> 3).wrapping_mul(8)) & 31)) as u8;
    let v = m.u8(at);
    m.set_u8(at, if on { v | mask } else { v & !mask });
}

/// Whether the bytes `b` are all contained in the bytes of the set `s` (`s & b == b` for every byte).
fn covers(m: &Mem, s: u32, b: u32, nb: i32) -> bool {
    let mut all = true;
    for i in 0..nb.max(0) as u32 {
        if m.u8(s + i) & m.u8(b + i) != m.u8(b + i) {
            all = false;
        }
    }
    all
}

fn copy_bytes(m: &mut Mem, dst: u32, src: u32, nb: i32) {
    for i in 0..nb.max(0) as u32 {
        let v = m.u8(src + i);
        m.set_u8(dst + i, v);
    }
}

/// Makes room for a node after `tail` when the ring is full: four new nodes. False when an allocation fails.
fn grow_ring(m: &mut Mem, tail: u32) -> bool {
    let mut p = tail;
    for _ in 0..4 {
        let q = m.malloc(8);
        m.set_u32(p + 4, q);
        if q == 0 {
            return false;
        }
        let set = m.malloc(m.u32(VCS_MB_LIST_SIZE));
        m.set_u32(q, set);
        if set == 0 {
            return false;
        }
        p = q;
    }
    let s = m.u32(VCS_START);
    m.set_u32(p + 4, s);
    true
}

/// [soft 0x0857aab0 VC_realSplitMBList]: `VC_realSplitMBList(list, &count, &parts)`: splits the index list (groups) into the
/// sets that the metaball links (`vcG_edgeList`) join; the parts are in `vcS_realSplitMBListData`. 0; 0x10 on an allocation
/// failure; 0x12 for 100 parts or more. No link inside the list gives a count of 0 and no parts.
pub fn real_split_mb_list(m: &mut Mem, list: u32, p_count: u32, p_parts: u32) -> i32 {
    let mut local_edges = 0i32;
    let (mut la, mut lb) = (0i32, 0i32);
    let mut e = 0i32;
    while e < m.i32(VCG_NUM_EDGES) {
        let (mut fa, mut fb) = (false, false);
        let n = m.i32(list);
        let ptr = m.u32(list + 8);
        let ed = m.u32(VCG_EDGE_LIST) + 8 * e as u32;
        let mut i = 0;
        while i < n {
            let v = m.i32(ptr + 4 * i as u32);
            if v == m.i32(ed) {
                fa = true;
                la = i;
            }
            if v == m.i32(ed + 4) {
                fb = true;
                lb = i;
            }
            i += 1;
        }
        if fa && fb {
            let le = m.u32(VCS_LOCAL_EDGE_LIST) + 8 * local_edges as u32;
            m.set_i32(le, la);
            m.set_i32(le + 4, lb);
            local_edges += 1;
        }
        e += 1;
    }
    if local_edges == 0 {
        m.set_i32(p_count, 0);
        return 0;
    }
    let cnt = m.i32(list);
    let nb = (if cnt < 0 { cnt.wrapping_add(7) } else { cnt } >> 3).wrapping_add(1);
    let first = m.u32(m.u32(VCS_START));
    for i in 0..nb.max(0) as u32 {
        m.set_u8(first + i, 0);
    }
    let mut i = 0;
    while i < m.i32(list) {
        let set = m.u32(m.u32(VCS_START));
        set_bit(m, set, i, true);
        i += 1;
    }
    let mut tail = m.u32(m.u32(VCS_START) + 4);
    let mut e = 0;
    while e < local_edges {
        let snapshot = tail;
        let mut start = m.u32(VCS_START);
        if start != snapshot {
            loop {
                let cur = m.u32(start);
                let next = m.u32(start + 4);
                m.set_u32(VCS_START, next);
                let le = m.u32(VCS_LOCAL_EDGE_LIST) + 8 * e as u32;
                let (a, b) = (m.i32(le), m.i32(le + 4));
                if !(bit(m, cur, a) && bit(m, cur, b)) {
                    if m.u32(tail + 4) == next && !grow_ring(m, tail) {
                        return 0x10;
                    }
                    copy_bytes(m, m.u32(tail), cur, nb);
                    tail = m.u32(tail + 4);
                } else {
                    let (t1, t2) = (m.u32(VCS_TMP_LIST1), m.u32(VCS_TMP_LIST2));
                    for i in 0..nb.max(0) as u32 {
                        let v = m.u8(cur + i);
                        m.set_u8(t2 + i, v);
                        m.set_u8(t1 + i, v);
                    }
                    set_bit(m, t1, a, false);
                    set_bit(m, t2, b, false);
                    let (mut add1, mut add2) = (true, true);
                    let mut s = m.u32(VCS_START);
                    while s != tail {
                        if covers(m, m.u32(s), t1, nb) {
                            add1 = false;
                            break;
                        }
                        s = m.u32(s + 4);
                    }
                    let mut s = m.u32(VCS_START);
                    while s != tail {
                        if covers(m, m.u32(s), t2, nb) {
                            add2 = false;
                            break;
                        }
                        s = m.u32(s + 4);
                    }
                    if add1 {
                        if m.u32(tail + 4) == m.u32(VCS_START) && !grow_ring(m, tail) {
                            return 0x10;
                        }
                        copy_bytes(m, m.u32(tail), t1, nb);
                        tail = m.u32(tail + 4);
                    }
                    if add2 {
                        if m.u32(tail + 4) == m.u32(VCS_START) && !grow_ring(m, tail) {
                            return 0x10;
                        }
                        copy_bytes(m, m.u32(tail), t2, nb);
                        tail = m.u32(tail + 4);
                    }
                }
                start = m.u32(VCS_START);
                if start == snapshot {
                    break;
                }
            }
        }
        e += 1;
    }
    let mut parts = 0;
    let mut s = m.u32(VCS_START);
    while s != tail {
        parts += 1;
        s = m.u32(s + 4);
    }
    if parts > 99 {
        return 0x12;
    }
    m.set_i32(p_count, parts);
    let data = m.u32(VCS_REAL_SPLIT_DATA);
    m.set_u32(p_parts, data);
    let mut s = m.u32(VCS_START);
    let mut k = 0u32;
    while s != tail {
        let set = m.u32(s);
        let n = m.i32(list);
        let mut c = 0;
        for i in 0..n {
            if bit(m, set, i) {
                c += 1;
            }
        }
        let d = m.u32(p_parts);
        m.set_i32(d + 12 * k, c);
        let mut c = 0u32;
        for i in 0..m.i32(list) {
            if bit(m, set, i) {
                let d = m.u32(p_parts);
                let dst = m.u32(d + 12 * k + 8);
                let v = m.i32(m.u32(list + 8) + 4 * i as u32);
                m.set_i32(dst + 4 * c, v);
                c += 1;
            }
        }
        k += 1;
        s = m.u32(s + 4);
    }
    0
}

/// [soft 0x0857b0dc VC_splitMBList]: `VC_splitMBList(list, &count, &parts)`: splits a list of metaballs into the parts whose groups
/// are linked; `parts` (`vcS_splitMBListData`, `count` entries) holds the metaballs of each part. 0, or the error of
/// `VC_realSplitMBList`.
pub fn split_mb_list(m: &mut Mem, list: u32, p_count: u32, p_parts: u32) -> i32 {
    m.set_i32(VCS_MBL_TEMP, 0);
    let group_of = |m: &Mem, i: i32| {
        let mb = m.i32(m.u32(list + 8) + 4 * i as u32);
        m.i32(m.u32(VCG_MB).wrapping_add((mb as u32).wrapping_mul(0x1e4)) + 0x1dc)
    };
    let mut i = 0;
    while i < m.i32(list) {
        let g = group_of(m, i);
        let n = m.i32(VCS_MBL_TEMP);
        let tmp = m.u32(VCS_MBL_TEMP + 8);
        let mut found = false;
        if n >= 1 && m.i32(tmp) != g {
            let mut j = 0;
            loop {
                j += 1;
                if n <= j {
                    break;
                }
                if m.i32(tmp + 4 * j as u32) == g {
                    found = true;
                    break;
                }
            }
        } else if n >= 1 {
            found = true;
        }
        if !found {
            let n = m.i32(VCS_MBL_TEMP);
            m.set_i32(tmp + 4 * n as u32, g);
            m.set_i32(VCS_MBL_TEMP, n + 1);
        }
        i += 1;
    }
    let loc = m.local(8);
    let r = real_split_mb_list(m, VCS_MBL_TEMP, loc, loc + 4);
    let real = m.u32(loc + 4);
    let count = m.i32(loc);
    let sd = m.u32(VCS_SPLIT_DATA);
    m.set_u32(p_parts, sd);
    m.set_i32(p_count, count);
    if r != 0 {
        return r;
    }
    let mut k = 0;
    while k < count {
        let off = 12 * k as u32;
        let dst = m.u32(p_parts);
        m.set_i32(dst + off, 0);
        let mut j = 0;
        while j < m.i32(list) {
            let g = group_of(m, j);
            let rc = m.i32(real + off);
            let mut add = false;
            if rc >= 1 {
                let rp = m.u32(real + off + 8);
                if m.i32(rp) == g {
                    add = true;
                } else {
                    let mut t = 0;
                    loop {
                        t += 1;
                        if rc <= t {
                            break;
                        }
                        if m.i32(rp + 4 * t as u32) == g {
                            add = true;
                            break;
                        }
                    }
                }
            }
            if add {
                let dst = m.u32(p_parts);
                let c = m.i32(dst + off);
                let p = m.u32(dst + off + 8);
                let v = m.i32(m.u32(list + 8) + 4 * j as u32);
                m.set_i32(p + 4 * c as u32, v);
                m.set_i32(dst + off, c + 1);
            }
            j += 1;
        }
        k += 1;
    }
    0
}

/// [soft 0x0857b29c VCCleanSplit]: frees everything `VC_initSplitData` allocated.
pub fn vc_clean_split(m: &mut Mem) {
    for i in 0..100u32 {
        let r = m.u32(m.u32(VCS_REAL_SPLIT_DATA) + 12 * i + 8);
        m.free(r);
        let s = m.u32(m.u32(VCS_SPLIT_DATA) + 12 * i + 8);
        m.free(s);
    }
    for a in [VCS_REAL_SPLIT_DATA, VCS_SPLIT_DATA, VCS_LOCAL_EDGE_LIST, VCS_MBL_TEMP + 8, VCS_TMP_LIST1, VCS_TMP_LIST2] {
        let p = m.u32(a);
        m.free(p);
    }
    free_ring(m);
}
