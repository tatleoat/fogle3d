// SPDX-License-Identifier: MIT
//! `hooks.c`: the metaball volume's public entry points: creating, changing, grouping and linking metaballs
//! (finding 0012, section 28). Records and lists live in the image at the addresses of `volume.rs`.

use crate::range::get_bounding_volume;
use crate::vceval::init_eval_contrib;
use crate::volume::*;
use si_math::vector::vc_mat4x4_invert_upper_left;
use si_math::types::MatrixD;
use si_mem::clib::memset;
use si_mem::Mem;
use si_x87::F80;

/// `vcG_voxList`, `vcG_vtxList` and `vcG_triList`: the lists `VCCleanHooks` frees.
pub const VCG_VOX_LIST: u32 = 0x0882_99d0;
pub const VCG_VTX_LIST: u32 = 0x0882_99d8;
pub const VCG_TRI_LIST: u32 = 0x0882_99e0;

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f64(m.f64(a))
}

fn grp(m: &Mem, g: i32) -> u32 {
    m.u32(VCG_GROUP_LIST).wrapping_add((g as u32).wrapping_mul(0x18))
}

fn read_mat(m: &Mem, a: u32) -> MatrixD {
    let mut r = [[0f64; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f64(a + 32 * i as u32 + 8 * j as u32);
        }
    }
    r
}

fn write_mat(m: &mut Mem, a: u32, v: &MatrixD) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f64(a + 32 * i as u32 + 8 * j as u32, *e);
        }
    }
}

/// The part of `VC_setMBParam` (matrix) and `VC_createMB` that runs on a stored matrix: the inverse of its upper
/// left, minus its translation, and the bounding volume.
fn finish_matrix(m: &mut Mem, idx: i32) {
    let b = mb_at(m, idx);
    memset(m, b + mb::INV, 0, 0x80);
    let src = read_mat(m, b + mb::MAT);
    let mut inv = read_mat(m, b + mb::INV);
    vc_mat4x4_invert_upper_left(&src, &mut inv);
    write_mat(m, b + mb::INV, &inv);
    let b = mb_at(m, idx);
    for k in 0..3u32 {
        let t = f(m, b + 0x12c + 8 * k);
        m.set_f64(b + mb::NEG_T + 8 * k, (-t).to_f64());
    }
    get_bounding_volume(m, idx);
}

/// [soft 0x08577e60 VC_initLocalLists]: `VC_initLocalLists(n)`: the three scratch lists of `n` indices each. 0 or 0x10.
pub fn init_local_lists(m: &mut Mem, n: i32) -> i32 {
    if m.i16(HOOKS_FIRST_192) == 0 {
        for g in [LISTALL_MBS_LOCLIST, LISTALL_GROUPS_LOCLIST, LIST_MBLINKS_LOCLIST] {
            let p = m.u32(g);
            m.free(p);
        }
    }
    m.set_i16(HOOKS_FIRST_192, 0);
    for g in [LISTALL_MBS_LOCLIST, LISTALL_GROUPS_LOCLIST, LIST_MBLINKS_LOCLIST] {
        let p = m.malloc((n as u32).wrapping_mul(4));
        m.set_u32(g, p);
        if p == 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x08577efc VC_initOrigLists]: the first allocation of the metaball, group and edge tables.
pub fn init_orig_lists(m: &mut Mem) -> i32 {
    if m.i16(HOOKS_FIRST_196) == 0 {
        let mut i = 0;
        while i < m.i32(VCG_NUM_MBS) {
            let b = mb_at(m, i);
            let p = m.u32(b + mb::COLOR);
            m.free(p);
            m.set_u32(b + mb::COLOR, 0);
            i += 1;
        }
        let p = m.u32(VCG_MB);
        m.free(p);
        m.set_u32(VCG_MB, 0);
        let mut i = 0;
        while i < m.i32(VCG_MAX_MBS) {
            let g = grp(m, i);
            if m.u32(g + 8) != 0 && m.i32(g) != 0 {
                let p = m.u32(g + 8);
                m.free(p);
            }
            m.set_u32(g + 8, 0);
            m.set_i32(g, 0);
            i += 1;
        }
        for a in [VCG_GROUP_LIST, VCG_EDGE_LIST] {
            let p = m.u32(a);
            m.free(p);
            m.set_u32(a, 0);
        }
    }
    m.set_i16(HOOKS_FIRST_196, 0);
    let p = m.malloc(0xb58);
    m.set_u32(VCG_MB, p);
    if p == 0 {
        return 0x10;
    }
    let p = m.malloc(0x90);
    m.set_u32(VCG_GROUP_LIST, p);
    if p == 0 {
        return 0x10;
    }
    for i in 0..6 {
        let g = grp(m, i);
        m.set_i32(g, 0);
        m.set_u32(g + 8, 0);
    }
    let p = m.malloc(0x20);
    m.set_u32(VCG_EDGE_LIST, p);
    if p == 0 {
        return 0x10;
    }
    let r = crate::split_proc::init_split_data(m, 6, 4);
    if r != 0 {
        return r;
    }
    let r = init_eval_contrib(m, 6);
    if r != 0 {
        return r;
    }
    let r = init_local_lists(m, 6);
    if r != 0 {
        return r;
    }
    for a in [VCG_NUM_MBS, VCG_MAX_MBS, VCG_NUM_EDGES, VCG_MAX_EDGES, VCG_NUM_GROUPS] {
        m.set_i32(a, 0);
    }
    0
}

/// [soft 0x085780d8 VC_setMBParam]: `VC_setMBParam(index, which, value)`: sets the matrix (0), threshold (1), strength (2),
/// exponents (3), roughness (4) or colour (5) of a metaball. 1: no such metaball; 2: threshold of 1 or more; 8: `which` 6.
pub fn set_mb_param(m: &mut Mem, idx: i32, which: i32, p: u32) -> i32 {
    if idx >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, idx) + mb::FREE) != 0 {
        return 1;
    }
    let b = mb_at(m, idx);
    match which as u32 {
        0 => {
            for i in 0..4u32 {
                for j in 0..4u32 {
                    let v = m.f64(p + 32 * i + 8 * j);
                    m.set_f64(b + mb::MAT_IN + 32 * i + 8 * j, v);
                    m.set_f64(b + mb::MAT + 32 * i + 8 * j, v);
                }
            }
            finish_matrix(m, idx);
        }
        1 => {
            let v = f(m, p);
            if !v.lt(F80::ONE) && !v.is_nan() {
                return 2;
            }
            m.set_f64(b + mb::THRESH, v.to_f64());
            let w = wyvill(v.to_f64());
            m.set_f64(mb_at(m, idx) + mb::KSCALE, (F80::ONE / w).to_f64());
        }
        2 => {
            let v = f(m, p);
            m.set_f64(b + mb::KSCALE, v.to_f64());
            if !v.gt(F80::ONE) && !v.is_nan() {
                m.set_u32(b + mb::THRESH, 0);
                m.set_u32(b + mb::THRESH + 4, 0);
                return 0;
            }
            let one = F80::ONE;
            let half = F80::from_f64(0.5);
            let (mut pp, mut q, mut r) = (F80::ZERO, one - one / v, one);
            let mut mid = F80::ZERO;
            for _ in 0..15 {
                let w = wyvill(q.to_f64());
                let rr = one / w;
                if rr.lt(v) {
                    mid = (q + r) * half;
                    pp = q;
                    q = mid;
                } else {
                    mid = (pp + q) * half;
                    r = q;
                    q = mid;
                }
            }
            m.set_f64(mb_at(m, idx) + mb::THRESH, mid.to_f64());
        }
        3 => {
            for k in 0..3u32 {
                let v = m.f64(p + 8 * k);
                m.set_f64(b + mb::EXPON + 8 * k, v);
            }
            let two = F80::from_f64(2.0);
            let d0 = f(m, b + mb::EXPON) - two;
            let d1 = f(m, b + mb::EXPON + 8) - two;
            let d2 = f(m, b + mb::EXPON + 16) - two;
            let s = d2 * d2 + (d1 * d1 + d0 * d0);
            m.set_i16(b + mb::FLAG30, s.gt(F80::from_f64(1e-06)) as i16);
        }
        4 => {
            let v = m.f64(p);
            m.set_f64(b + mb::ROUGH, v);
        }
        5 => {
            for k in 0..3u32 {
                let c = m.u32(b + mb::COLOR);
                let v = m.f64(p + 8 * k);
                m.set_f64(c + 8 * k, v);
            }
        }
        6 => return 8,
        _ => {}
    }
    0
}

/// [soft 0x0857839c VC_getMBParam]: `VC_getMBParam(index, which, out)`: the inverse of `VC_setMBParam`, and (6) the bounding
/// box (low corner, then high corner). 1: no such metaball.
pub fn get_mb_param(m: &mut Mem, idx: i32, which: i32, out: u32) -> i32 {
    if idx >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, idx) + mb::FREE) != 0 {
        return 1;
    }
    let b = mb_at(m, idx);
    match which as u32 {
        0 => {
            for i in 0..4u32 {
                for j in 0..4u32 {
                    let v = m.f64(b + mb::MAT_IN + 32 * i + 8 * j);
                    m.set_f64(out + 32 * i + 8 * j, v);
                }
            }
        }
        1 => {
            let v = m.f64(b + mb::THRESH);
            m.set_f64(out, v);
        }
        2 => {
            let v = m.f64(b + mb::KSCALE);
            m.set_f64(out, v);
        }
        3 => {
            for k in 0..3u32 {
                let v = m.f64(b + mb::EXPON + 8 * k);
                m.set_f64(out + 8 * k, v);
            }
        }
        4 => {
            let v = m.f64(b + mb::ROUGH);
            m.set_f64(out, v);
        }
        5 => {
            for k in 0..3u32 {
                let c = m.u32(mb_at(m, idx) + mb::COLOR);
                let v = m.f64(c + 8 * k);
                m.set_f64(out + 8 * k, v);
            }
        }
        6 => {
            for k in 0..3u32 {
                let t = f(m, b + 0xac + 8 * k);
                let e = f(m, b + mb::EXTENT + 8 * k);
                m.set_f64(out + 8 * k, (t - e).to_f64());
                m.set_f64(out + 24 + 8 * k, (t + e).to_f64());
            }
        }
        _ => {}
    }
    0
}

/// Removes every edge that has group `g` at either end (the shared loop of the destroying functions).
fn remove_edges_of(m: &mut Mem, g: i32) {
    let mut i = 0;
    while i < m.i32(VCG_NUM_EDGES) {
        let e = m.u32(VCG_EDGE_LIST) + 8 * i as u32;
        if m.i32(e) == g || m.i32(e + 4) == g {
            let n = m.i32(VCG_NUM_EDGES);
            let mut j = i;
            while j < n - 1 {
                let s = m.u32(VCG_EDGE_LIST) + 8 * j as u32;
                let (a, b) = (m.i32(s + 8), m.i32(s + 12));
                m.set_i32(s, a);
                m.set_i32(s + 4, b);
                j += 1;
            }
            m.set_i32(VCG_NUM_EDGES, n - 1);
        }
        i += 1;
    }
}

/// The first group with no members, or `maxMBs` when there is none.
fn first_empty_group(m: &Mem) -> i32 {
    let mut i = 0;
    while m.i32(grp(m, i)) > 0 {
        i += 1;
    }
    i
}

/// Makes a group of one metaball in the first empty slot; false when the `malloc` failed.
fn make_singleton(m: &mut Mem, mbi: i32) -> bool {
    let g = first_empty_group(m);
    let b = mb_at(m, mbi);
    m.set_i32(b + mb::GROUP, g);
    let ga = grp(m, g);
    m.set_i32(ga, 1);
    m.set_i32(ga + 4, 2);
    let p = m.malloc(8);
    m.set_u32(ga + 8, p);
    if p == 0 {
        return false;
    }
    m.set_i32(p, mbi);
    true
}

/// [soft 0x08578508 VC_createMB]: `VC_createMB(matrix, &index)`: a new metaball with default parameters in a group of its own.
pub fn create_mb(m: &mut Mem, mat: u32, out: u32) -> i32 {
    let slot;
    if m.i32(VCG_NUM_MBS) == m.i32(VCG_MAX_MBS) {
        slot = m.i32(VCG_NUM_MBS);
        let n = slot + 1;
        m.set_i32(VCG_MAX_MBS, n);
        m.set_i32(VCG_NUM_MBS, n);
        if n > 4 && n % 3 == 0 {
            let p = m.u32(VCG_MB);
            let np = m.realloc(p, ((n + 3) as u32).wrapping_mul(0x1e4));
            m.set_u32(VCG_MB, np);
            if np == 0 {
                return 0x10;
            }
            let p = m.u32(VCG_GROUP_LIST);
            let np = m.realloc(p, ((n + 3) as u32).wrapping_mul(0x18));
            m.set_u32(VCG_GROUP_LIST, np);
            if np == 0 {
                return 0x10;
            }
            for k in 0..3 {
                let g = grp(m, n + k);
                m.set_i32(g, 0);
            }
            let r = crate::split_proc::init_split_data(m, n + 4, m.i32(VCG_NUM_EDGES) + 1);
            if r != 0 {
                return r;
            }
            let r = init_eval_contrib(m, n + 4);
            if r != 0 {
                return r;
            }
            let r = init_local_lists(m, n + 4);
            if r != 0 {
                return r;
            }
        }
    } else {
        let mut s = 0;
        while m.i16(mb_at(m, s) + mb::FREE) == 0 {
            s += 1;
        }
        slot = s;
        let n = m.i32(VCG_NUM_MBS);
        m.set_i32(VCG_NUM_MBS, n + 1);
    }
    let b = mb_at(m, slot);
    for i in 0..4u32 {
        for j in 0..4u32 {
            let v = m.f64(mat + 32 * i + 8 * j);
            m.set_f64(b + mb::MAT_IN + 32 * i + 8 * j, v);
            m.set_f64(b + mb::MAT + 32 * i + 8 * j, v);
        }
    }
    finish_matrix(m, slot);
    let b = mb_at(m, slot);
    m.set_f64(b + mb::ROUGH, 1.0);
    let c = m.malloc(0x48);
    m.set_u32(b + mb::COLOR, c);
    if c == 0 {
        return 0x10;
    }
    memset(m, c, 0, 0x48);
    for k in 0..3u32 {
        m.set_f64(c + 8 * k, 1.0);
    }
    m.set_f64(b + mb::THRESH, 0.5);
    m.set_f64(b + mb::KSCALE, 2.0);
    for k in 0..3u32 {
        m.set_f64(b + mb::EXPON + 8 * k, 2.0);
    }
    m.set_i16(b + mb::FLAG30, 0);
    m.set_i16(b + mb::FREE, 0);
    if !make_singleton(m, slot) {
        return 0x10;
    }
    m.set_i32(out, slot);
    0
}

/// [soft 0x085788d0 VC_destroyMB]: `VC_destroyMB(index)`: frees the slot; a group left empty goes with its links. 1: no such metaball.
pub fn destroy_mb(m: &mut Mem, idx: i32) -> i32 {
    if idx >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, idx) + mb::FREE) != 0 {
        return 1;
    }
    let b = mb_at(m, idx);
    m.set_i16(b + mb::FREE, 1);
    let n = m.i32(VCG_NUM_MBS);
    m.set_i32(VCG_NUM_MBS, n - 1);
    let c = m.u32(b + mb::COLOR);
    m.free(c);
    m.set_u32(b + mb::COLOR, 0);
    let gi = m.i32(b + mb::GROUP);
    let g = grp(m, gi);
    if m.i32(g) == 1 {
        let p = m.u32(g + 8);
        m.free(p);
        m.set_u32(g + 8, 0);
        m.set_i32(g, 0);
        m.set_i32(g + 4, 0);
        remove_edges_of(m, gi);
    } else {
        let cnt = m.i32(g);
        let arr = m.u32(g + 8);
        let mut i = 0;
        while m.i32(arr + 4 * i as u32) != idx {
            i += 1;
        }
        while i < cnt - 1 {
            let v = m.i32(arr + 4 * i as u32 + 4);
            m.set_i32(arr + 4 * i as u32, v);
            i += 1;
        }
        m.set_i32(g, cnt - 1);
    }
    0
}

/// [soft 0x08578a48 VC_listAllMBs]: `VC_listAllMBs(&list, &count)`: the indices of the metaballs in use.
pub fn list_all_mbs(m: &mut Mem, list: u32, count: u32) -> i32 {
    let l = m.u32(LISTALL_MBS_LOCLIST);
    m.set_u32(list, l);
    let mut k = 0;
    let mut i = 0;
    while i < m.i32(VCG_MAX_MBS) {
        if m.i16(mb_at(m, i) + mb::FREE) == 0 {
            let l = m.u32(LISTALL_MBS_LOCLIST);
            m.set_i32(l + 4 * k as u32, i);
            k += 1;
        }
        i += 1;
    }
    let n = m.i32(VCG_NUM_MBS);
    m.set_i32(count, n);
    0
}

/// [soft 0x08578aa0 VC_numMBs]: `VC_numMBs(&n)`.
pub fn num_mbs(m: &mut Mem, p: u32) -> i32 {
    let n = m.i32(VCG_NUM_MBS);
    m.set_i32(p, n);
    0
}

/// [soft 0x08578ab4 VC_addMBtoGroup]: `VC_addMBtoGroup(mb, group)`: moves a lone metaball into another group. 1: bad metaball;
/// 3: bad group; 4: the metaball is not alone or already in that group.
pub fn add_mb_to_group(m: &mut Mem, mbi: i32, group: i32) -> i32 {
    if mbi >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, mbi) + mb::FREE) != 0 {
        return 1;
    }
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    let b = mb_at(m, mbi);
    let old = m.i32(b + mb::GROUP);
    if m.i32(grp(m, old)) >= 2 || group == old {
        return 4;
    }
    let og = grp(m, old);
    let p = m.u32(og + 8);
    m.free(p);
    m.set_u32(og + 8, 0);
    m.set_i32(og, 0);
    m.set_i32(og + 4, 0);
    remove_edges_of(m, old);
    let g = grp(m, group);
    let cnt = m.i32(g);
    let arr = m.u32(g + 8);
    m.set_i32(arr + 4 * cnt as u32, mbi);
    let b = mb_at(m, mbi);
    m.set_i32(b + mb::GROUP, group);
    m.set_i32(g, cnt + 1);
    if cnt % 2 != 0 {
        m.set_i32(g + 4, cnt + 3);
        let np = m.realloc(arr, ((cnt + 3) as u32).wrapping_mul(4));
        m.set_u32(g + 8, np);
        if np == 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x08578c50 VC_removeMBfromGroup]: `VC_removeMBfromGroup(mb, group)`: the metaball leaves its group for a new one of its own.
pub fn remove_mb_from_group(m: &mut Mem, mbi: i32, group: i32) -> i32 {
    if mbi >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, mbi) + mb::FREE) != 0 {
        return 1;
    }
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    if m.i32(mb_at(m, mbi) + mb::GROUP) != group {
        return 5;
    }
    let g = grp(m, group);
    let cnt = m.i32(g);
    if cnt == 1 {
        return 6;
    }
    let arr = m.u32(g + 8);
    let mut i = 0;
    while m.i32(arr + 4 * i as u32) != mbi {
        i += 1;
    }
    while i < cnt - 1 {
        let v = m.i32(arr + 4 * i as u32 + 4);
        m.set_i32(arr + 4 * i as u32, v);
        i += 1;
    }
    m.set_i32(g, cnt - 1);
    if !make_singleton(m, mbi) {
        return 0x10;
    }
    0
}

/// [soft 0x08578db8 VC_dissolveMBGroup]: `VC_dissolveMBGroup(group)`: every member gets a group of its own; the links go.
pub fn dissolve_mb_group(m: &mut Mem, group: i32) -> i32 {
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    let g = grp(m, group);
    let arr = m.u32(g + 8);
    let cnt = m.i32(g);
    m.set_i32(g, 0);
    remove_edges_of(m, group);
    let mut i = 0;
    while i < cnt {
        let mbi = m.i32(arr + 4 * i as u32);
        if !make_singleton(m, mbi) {
            return 0x10;
        }
        i += 1;
    }
    m.free(arr);
    0
}

/// [soft 0x08578f08 VC_destroyMBGroup]: `VC_destroyMBGroup(group)`: frees every member and the group's links.
pub fn destroy_mb_group(m: &mut Mem, group: i32) -> i32 {
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    let g = grp(m, group);
    let mut i = 0;
    while i < m.i32(g) {
        let mbi = m.i32(m.u32(g + 8) + 4 * i as u32);
        let b = mb_at(m, mbi);
        m.set_i16(b + mb::FREE, 1);
        let n = m.i32(VCG_NUM_MBS);
        m.set_i32(VCG_NUM_MBS, n - 1);
        let c = m.u32(b + mb::COLOR);
        m.free(c);
        i += 1;
    }
    m.set_i32(g, 0);
    m.set_i32(g + 4, 0);
    let p = m.u32(g + 8);
    m.free(p);
    m.set_u32(g + 8, 0);
    remove_edges_of(m, group);
    0
}

/// [soft 0x08579038 VC_listMBGroup]: `VC_listMBGroup(group, &list, &count)`.
pub fn list_mb_group(m: &mut Mem, group: i32, list: u32, count: u32) -> i32 {
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    let g = grp(m, group);
    let p = m.u32(g + 8);
    m.set_u32(list, p);
    let c = m.i32(g);
    m.set_i32(count, c);
    0
}

/// [soft 0x08579080 VC_memberMBGroup]: `VC_memberMBGroup(mb, group, &is_member)`.
pub fn member_mb_group(m: &mut Mem, mbi: i32, group: i32, out: u32) -> i32 {
    if mbi >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, mbi) + mb::FREE) != 0 {
        return 1;
    }
    if group >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, group)) == 0 {
        return 3;
    }
    let v = (m.i32(mb_at(m, mbi) + mb::GROUP) == group) as i16;
    m.set_i16(out, v);
    0
}

/// [soft 0x085790f0 VC_getMBGroupID]: `VC_getMBGroupID(mb, &group)`.
pub fn get_mb_group_id(m: &mut Mem, mbi: i32, out: u32) -> i32 {
    if mbi >= m.i32(VCG_MAX_MBS) || m.i16(mb_at(m, mbi) + mb::FREE) != 0 {
        return 1;
    }
    let v = m.i32(mb_at(m, mbi) + mb::GROUP);
    m.set_i32(out, v);
    0
}

/// [soft 0x08579134 VC_listAllMBGroups]: `VC_listAllMBGroups(&list, &count)`: the groups that have members.
pub fn list_all_mb_groups(m: &mut Mem, list: u32, count: u32) -> i32 {
    let l = m.u32(LISTALL_GROUPS_LOCLIST);
    m.set_u32(list, l);
    let mut k = 0;
    let mut i = 0;
    while i < m.i32(VCG_MAX_MBS) {
        if m.i32(grp(m, i)) > 0 {
            let l = m.u32(LISTALL_GROUPS_LOCLIST);
            m.set_i32(l + 4 * k as u32, i);
            k += 1;
        }
        i += 1;
    }
    m.set_i32(count, k);
    0
}

/// [soft 0x08579190 VC_numMBGroups]: `VC_numMBGroups(&n)`.
pub fn num_mb_groups(m: &mut Mem, out: u32) -> i32 {
    let mut k = 0;
    let mut i = 0;
    while i < m.i32(VCG_MAX_MBS) {
        if m.i32(grp(m, i)) > 0 {
            k += 1;
        }
        i += 1;
    }
    m.set_i32(out, k);
    0
}

fn bad_group(m: &Mem, g: i32) -> bool {
    g >= m.i32(VCG_MAX_MBS) || m.i32(grp(m, g)) == 0
}

/// The index of the edge joining `a` and `b` (either way round), or `numEdges` when there is none.
fn find_edge(m: &Mem, a: i32, b: i32) -> i32 {
    let mut i = 0;
    while i < m.i32(VCG_NUM_EDGES) {
        let e = m.u32(VCG_EDGE_LIST) + 8 * i as u32;
        let (x, y) = (m.i32(e), m.i32(e + 4));
        if (x == a && y == b) || (x == b && y == a) {
            break;
        }
        i += 1;
    }
    i
}

/// [soft 0x085791bc VC_createMBLink]: `VC_createMBLink(a, b)`: 3: bad group; 9: the same group; 10: already linked.
pub fn create_mb_link(m: &mut Mem, a: i32, b: i32) -> i32 {
    if bad_group(m, a) || bad_group(m, b) {
        return 3;
    }
    if a == b {
        return 9;
    }
    if find_edge(m, a, b) < m.i32(VCG_NUM_EDGES) {
        return 10;
    }
    let n = m.i32(VCG_NUM_EDGES);
    let e = m.u32(VCG_EDGE_LIST) + 8 * n as u32;
    m.set_i32(e, a);
    m.set_i32(e + 4, b);
    if n == m.i32(VCG_MAX_EDGES) {
        m.set_i32(VCG_MAX_EDGES, n + 1);
    }
    m.set_i32(VCG_NUM_EDGES, n + 1);
    let me = m.i32(VCG_MAX_EDGES);
    if me == (me / 3) * 3 {
        let p = m.u32(VCG_EDGE_LIST);
        let np = m.realloc(p, (me as u32).wrapping_mul(8).wrapping_add(0x18));
        m.set_u32(VCG_EDGE_LIST, np);
        if np == 0 {
            return 0x10;
        }
        let mm = m.i32(VCG_MAX_MBS);
        if crate::split_proc::init_split_data(m, mm + 1, me + 4) != 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x08579304 VC_destroyMBLink]: `VC_destroyMBLink(a, b)`: 3: bad group; 7: no such link.
pub fn destroy_mb_link(m: &mut Mem, a: i32, b: i32) -> i32 {
    if bad_group(m, a) || bad_group(m, b) {
        return 3;
    }
    let mut i = find_edge(m, a, b);
    let n = m.i32(VCG_NUM_EDGES);
    if i >= n {
        return 7;
    }
    while i < n - 1 {
        let s = m.u32(VCG_EDGE_LIST) + 8 * i as u32;
        let (x, y) = (m.i32(s + 8), m.i32(s + 12));
        m.set_i32(s, x);
        m.set_i32(s + 4, y);
        i += 1;
    }
    m.set_i32(VCG_NUM_EDGES, n - 1);
    0
}

/// [soft 0x085793c4 VC_listAllMBLinks]: `VC_listAllMBLinks(&list, &count)`.
pub fn list_all_mb_links(m: &mut Mem, list: u32, count: u32) -> i32 {
    let l = m.u32(VCG_EDGE_LIST);
    m.set_u32(list, l);
    let n = m.i32(VCG_NUM_EDGES);
    m.set_i32(count, n);
    0
}

/// [soft 0x085793e4 VC_listMBLinks]: `VC_listMBLinks(group, &list, &count)`: the groups linked to `group`.
pub fn list_mb_links(m: &mut Mem, group: i32, list: u32, count: u32) -> i32 {
    if bad_group(m, group) {
        return 3;
    }
    let mut k = 0;
    let mut i = 0;
    while i < m.i32(VCG_NUM_EDGES) {
        let e = m.u32(VCG_EDGE_LIST) + 8 * i as u32;
        if m.i32(e) == group || m.i32(e + 4) == group {
            k += 1;
        }
        i += 1;
    }
    if k == 0 {
        m.set_u32(list, 0);
        m.set_i32(count, 0);
    } else {
        let l = m.u32(LIST_MBLINKS_LOCLIST);
        m.set_u32(list, l);
        let mut j = 0;
        let mut i = 0;
        while i < m.i32(VCG_NUM_EDGES) {
            let e = m.u32(VCG_EDGE_LIST) + 8 * i as u32;
            let (x, y) = (m.i32(e), m.i32(e + 4));
            if x == group || y == group {
                let l = m.u32(LIST_MBLINKS_LOCLIST);
                m.set_i32(l + 4 * j as u32, if x == group { y } else { x });
                j += 1;
            }
            i += 1;
        }
        m.set_i32(count, k);
    }
    0
}

/// [soft 0x085794a4 VC_numMBLinks]: `VC_numMBLinks(&n)`.
pub fn num_mb_links(m: &mut Mem, out: u32) -> i32 {
    let n = m.i32(VCG_NUM_EDGES);
    m.set_i32(out, n);
    0
}

/// [soft 0x085794b8 VCCleanHooks]: frees the three scratch lists and the three mesh lists.
pub fn vc_clean_hooks(m: &mut Mem) {
    for a in [LIST_MBLINKS_LOCLIST, LISTALL_GROUPS_LOCLIST, LISTALL_MBS_LOCLIST, VCG_VTX_LIST, VCG_TRI_LIST, VCG_VOX_LIST] {
        let p = m.u32(a);
        m.free(p);
    }
}
