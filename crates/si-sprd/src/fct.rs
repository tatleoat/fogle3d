// SPDX-License-Identifier: MIT
//! `sprd_fct.c`: the setup records of a spreadsheet (finding 0043). Every function takes
//! the image, whoever answers for the outside, and the addresses the original takes.
//!
//! Not reproduced: the counter `SI_secureLicAlloc` (0x08679660) that the original adds one
//! to before each allocation call (the allocators clear it again); the Rust does not
//! count.

use crate::glob::*;
use crate::hook::*;
use crate::{g, list, TEXTS};
use si_mem::clib;
use si_mem::{Mem, Outside};

const ALL: u32 = 0x086f_6cec;
const MAIN_QUERY_1: u32 = 0x086f_6cff;
const MAIN_QUERY_2: u32 = 0x086f_6d0a;
const EQUALS: u32 = 0x086f_6d15;
const QUERY: u32 = 0x086f_6d17;
const CHPTYP: u32 = 0x086f_6d1d;
const ALL_WORD: &[u8] = b"ALL";

fn call(m: &mut Mem, o: &mut dyn Outside, r: si_mem::Routine, a: &[u32]) -> u32 {
    o.call(m, r, a).eax
}

fn free_field(m: &mut Mem, at: u32) {
    let p = m.u32(at);
    if p != 0 {
        m.free(p);
        m.set_u32(at, 0);
    }
}

/// A query initialised from its text [soft 0x085a347c si_sprdQueryInitialize]: the text is
/// copied, parsed as `= text` in query mode, and the relation data made for the frame range.
pub fn query_initialize(m: &mut Mem, o: &mut dyn Outside, q: u32, text: u32) {
    let local = m.local(4);
    m.set_u32(local, 0);
    m.set_u32(q, 0);
    m.set_u32(q + 4, 0);
    let s = m.strdup(text);
    m.set_u32(q + 8, s);
    m.set_i16(q + 0x0c, 0);
    let s = call(m, o, CHECK_ALLOC_STR, &[0, EQUALS, 1]);
    let t = m.u32(q + 8);
    let s = call(m, o, CHECK_ALLOC_STR, &[s, t, 0]);
    m.set_i16(SI_QUERY_MODE, 1);
    call(m, o, GEN_CUS_EXPR_PARSING_TREE, &[s, 0, 0, local, 1, 0]);
    m.set_i16(SI_QUERY_MODE, 0);
    if s != 0 {
        m.free(s);
    }
    let (start, end) = (m.i32(SI_FRAME_START), m.i32(SI_FRAME_END));
    let expr = m.u32(local);
    m.set_u32(q, expr);
    let rel = call(m, o, EXPR_INIT_REL_DATA, &[expr, 1, end.wrapping_sub(start).wrapping_add(1) as u32]);
    m.set_u32(q + 4, rel);
    call(m, o, GET_CLAUSE_FROM_QUERY, &[q]);
    m.release(4);
}

/// A group setup in its initial state [soft 0x085a3310 sprdInitGroupSetup]: the query is
/// `this.chptyp == ALL`.
pub fn init_group_setup(m: &mut Mem, o: &mut dyn Outside, grp: u32) {
    m.set_u32(grp, 0);
    m.set_i32(grp + 4, -1);
    m.set_i16(grp + 8, 1);
    query_initialize(m, o, grp + g::QUERY, ALL);
    for off in [0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c] {
        m.set_u32(grp + off, 0);
    }
    m.set_i16(grp + 0x40, 0);
    m.set_u32(grp + 0x44, 0);
    m.set_i32(grp + 0x48, 1);
    m.set_i16(grp + 0x4c, 0);
}

/// [soft 0x085a338c SI_sprdInitElem]: an element's name words and sub list cleared.
pub fn init_elem(m: &mut Mem, e: u32) {
    m.set_u32(e, 0);
    m.set_u32(e + 4, 0);
    m.set_u32(e + 0x10, 0);
    m.set_u32(e + 0x14, 0);
}

/// [soft 0x085a33b4 sprdInitElemList]: count and array cleared.
pub fn init_elem_list(m: &mut Mem, l: u32) {
    m.set_u32(l, 0);
    m.set_u32(l + 4, 0);
}

/// A setup in its initial state [soft 0x085a33cc SI_sprdInitSetup].
pub fn init_setup(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    init_group_setup(m, o, s + 8);
    init_elem_list(m, s + 0x58);
    let p = m.strdup(MAIN_QUERY_1);
    m.set_u32(s, p);
    let p = m.strdup(MAIN_QUERY_2);
    m.set_u32(s + 8, p);
    m.set_i16(s + 4, 0);
    m.set_i16(s + 0x6c, 0);
    m.set_i16(s + 0x6e, 0);
    m.set_i16(s + 0x74, 0);
    m.set_i16(s + 0x6a, 1);
    m.set_i16(s + 0x70, 1);
    m.set_i16(s + 0x72, 2);
}

/// A new setup, initialised [soft 0x085a3444 SI_sprdSetupAllocate]: `SI_calloc(1, 0x78)`.
pub fn setup_allocate(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let p = m.si_calloc(1, 0x78);
    if p != 0 {
        init_setup(m, o, p);
    }
    p
}

/// A clause and its text freed, the pointer cleared [soft 0x085a4af4 si_sprdFreeClause].
pub fn free_clause(m: &mut Mem, pp: u32) {
    let c = m.u32(pp);
    if c != 0 {
        free_field(m, c + 0x18);
        if m.u32(pp) != 0 {
            let c = m.u32(pp);
            m.free(c);
            m.set_u32(pp, 0);
        }
    }
}

/// A group's query made again from its text, and its sub groups' [soft 0x085a3548 si_sprdQueryRegenerate].
pub fn query_regenerate(m: &mut Mem, o: &mut dyn Outside, grp: u32) {
    // (the original tests the address of the query inside the group against null: never true)
    let e = m.u32(grp + 0x0c);
    call(m, o, EXPRESS_DATA_DISPOSE, &[e]);
    m.set_u32(grp + 0x0c, 0);
    let r = m.u32(grp + 0x10);
    call(m, o, EXPR_REL_DATA_DISPOSE, &[r]);
    m.set_u32(grp + 0x10, 0);
    let mut i = 0i32;
    while i < m.i32(grp + 0x1c) {
        let a = m.u32(grp + 0x20).wrapping_add((i as u32).wrapping_mul(4));
        free_clause(m, a);
        i += 1;
    }
    let t = m.u32(grp + 0x14);
    query_initialize(m, o, grp + 0x0c, t);
    let mut i = 0i32;
    while i < m.i16(grp + g::NSUB) as i32 {
        let sub = m.u32(grp + g::SUB).wrapping_add((i as u32).wrapping_mul(g::SIZE));
        query_regenerate(m, o, sub);
        i += 1;
    }
}

/// A query copied by making it again from the text and copying the clauses' 16-bit flag
/// [soft 0x085a35ec sprdQueryDuplicate]; answers 1.
pub fn query_duplicate(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) -> u32 {
    let t = m.u32(src + 8);
    query_initialize(m, o, dst, t);
    let mut i = 0u32;
    while (i as i32) < m.i32(dst + 0x10) {
        let d = m.u32(m.u32(dst + 0x14).wrapping_add(i * 4));
        let s = m.u32(m.u32(src + 0x14).wrapping_add(i * 4));
        let v = m.i16(s.wrapping_add(0x20));
        m.set_i16(d.wrapping_add(0x20), v);
        i += 1;
    }
    1
}

/// The shared body of the sort and the field list copies: a count, an array of 0x14-byte
/// items and an array of 16-bit directions; 1 on success, 0 when an allocation failed.
fn items_duplicate(m: &mut Mem, src: u32, dst: u32) -> u32 {
    let n = m.i32(src);
    m.set_i32(dst, n);
    m.set_u32(dst + 4, 0);
    m.set_u32(dst + 8, 0);
    if n < 1 {
        return 1;
    }
    let a = m.si_malloc((n as u32).wrapping_mul(0x14));
    m.set_u32(dst + 4, a);
    let d = m.si_malloc((n as u32).wrapping_mul(2));
    m.set_u32(dst + 8, d);
    if m.u32(dst + 4) == 0 {
        if d != 0 {
            m.free(d);
            m.set_u32(dst + 8, 0);
        }
    } else {
        if d != 0 {
            let mut k = 0u32;
            while (k as i32) < m.i32(src) {
                let s = m.u32(src + 4).wrapping_add(k * 0x14);
                let t = m.u32(dst + 4).wrapping_add(k * 0x14);
                clib::memcpy(m, t, s, 0x14);
                let v = m.i16(m.u32(src + 8).wrapping_add(k * 2));
                m.set_i16(m.u32(dst + 8).wrapping_add(k * 2), v);
                k += 1;
            }
            return 1;
        }
        let a = m.u32(dst + 4);
        m.free(a);
        m.set_u32(dst + 4, 0);
    }
    0
}

/// [soft 0x085a3638 sprdSortListDuplicate]
pub fn sort_list_duplicate(m: &mut Mem, src: u32, dst: u32) -> u32 {
    items_duplicate(m, src, dst)
}

/// [soft 0x085a373c sprdFieldListDuplicate]
pub fn field_list_duplicate(m: &mut Mem, src: u32, dst: u32) -> u32 {
    items_duplicate(m, src, dst)
}

/// [soft 0x085a4b3c si_sprdQueryDispose]: the expression, its data, the text and the clauses.
pub fn query_dispose(m: &mut Mem, o: &mut dyn Outside, grp: u32) {
    let e = m.u32(grp + 0x0c);
    call(m, o, EXPRESS_DATA_DISPOSE, &[e]);
    m.set_u32(grp + 0x0c, 0);
    let r = m.u32(grp + 0x10);
    call(m, o, EXPR_REL_DATA_DISPOSE, &[r]);
    m.set_u32(grp + 0x10, 0);
    free_field(m, grp + 0x14);
    let mut i = 0i32;
    while i < m.i32(grp + 0x1c) {
        let a = m.u32(grp + 0x20).wrapping_add((i as u32).wrapping_mul(4));
        free_clause(m, a);
        i += 1;
    }
}

/// [soft 0x085a49ec si_sprdFieldListDispose]
pub fn field_list_dispose(m: &mut Mem, grp: u32) {
    free_field(m, grp + 0x38);
    free_field(m, grp + 0x3c);
    m.set_u32(grp + 0x34, 0);
    m.set_u32(grp + 0x38, 0);
    m.set_u32(grp + 0x3c, 0);
}

/// [soft 0x085a4994 si_sprdSortListDispose]
pub fn sort_list_dispose(m: &mut Mem, grp: u32) {
    free_field(m, grp + 0x2c);
    free_field(m, grp + 0x30);
    m.set_u32(grp + 0x28, 0);
    m.set_u32(grp + 0x2c, 0);
    m.set_u32(grp + 0x30, 0);
}

/// A group and its sub groups taken apart (the array of sub groups itself is not freed)
/// [soft 0x085a4bb8 si_sprdGroupSetupDispose].
pub fn group_setup_dispose(m: &mut Mem, o: &mut dyn Outside, grp: u32) {
    free_field(m, grp);
    sort_list_dispose(m, grp);
    field_list_dispose(m, grp);
    query_dispose(m, o, grp);
    let mut i = 0i32;
    while i < m.i16(grp + g::NSUB) as i32 {
        let sub = m.u32(grp + g::SUB).wrapping_add((i as u32).wrapping_mul(g::SIZE));
        group_setup_dispose(m, o, sub);
        i += 1;
    }
    m.set_i16(grp + g::NSUB, 0);
}

/// A group copied with its sub groups; 1 on success [soft 0x085a3840 sprdSetupGroupDuplicate].
pub fn setup_group_duplicate(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) -> u32 {
    clib::memcpy(m, dst, src, 0x50);
    let n = m.u32(src);
    let name = m.strdup(n);
    m.set_u32(dst, name);
    if query_duplicate(m, o, src + 0x0c, dst + 0x0c) as i16 == 0 {
        free_field(m, dst);
        return 0;
    }
    if sort_list_duplicate(m, src + 0x28, dst + 0x28) as i16 == 0 {
        free_field(m, dst);
        query_dispose(m, o, dst);
        return 0;
    }
    if field_list_duplicate(m, src + 0x34, dst + 0x34) as i16 != 0 {
        if m.i16(src + g::NSUB) < 1 {
            m.set_u32(dst + g::SUB, 0);
        } else {
            let n = m.i16(src + g::NSUB) as i32 as u32;
            let a = m.si_malloc(n.wrapping_mul(0x50));
            m.set_u32(dst + g::SUB, a);
            if a == 0 {
                return 0;
            }
            let mut i = 0i32;
            while i < m.i16(src + g::NSUB) as i32 {
                let s = m.u32(src + g::SUB).wrapping_add((i as u32).wrapping_mul(0x50));
                let d = m.u32(dst + g::SUB).wrapping_add((i as u32).wrapping_mul(0x50));
                if setup_group_duplicate(m, o, s, d) as i16 == 0 {
                    let mut k = i - 1;
                    while k >= 0 {
                        let d = m.u32(dst + g::SUB).wrapping_add((k as u32).wrapping_mul(0x50));
                        group_setup_dispose(m, o, d);
                        k -= 1;
                    }
                    if m.u32(dst + g::SUB) == 0 {
                        return 0;
                    }
                    free_field(m, dst + g::SUB);
                    return 0;
                }
                i += 1;
            }
        }
        return 1;
    }
    free_field(m, dst);
    query_dispose(m, o, dst);
    free_field(m, dst + 0x2c);
    free_field(m, dst + 0x30);
    0
}

/// A setup copied; a null setup gives a new one [soft 0x085a3a6c si_sprdSetupDuplicate].
/// The copy is made with `SI_calloc(0x78, 0x78)` (the original's count; a block of 14,400 bytes).
pub fn setup_duplicate(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    if src == 0 {
        return setup_allocate(m, o);
    }
    let p = m.si_calloc(0x78, 0x78);
    if p != 0 {
        clib::memcpy(m, p, src, 0x78);
        let mut n = m.u32(src);
        if n == 0 {
            n = QUERY;
        }
        let name = m.strdup(n);
        m.set_u32(p, name);
        m.set_u32(p + 0x58, 0);
        m.set_u32(p + 0x5c, 0);
        m.set_i16(p + 0x60, 0);
        if setup_group_duplicate(m, o, src + 8, p + 8) as i16 != 0 {
            return p;
        }
        if m.u32(p) != 0 {
            let n = m.u32(p);
            m.free(n);
            m.set_u32(p, 0);
        }
        m.free(p);
    }
    0
}

/// A new clause for the type column, all words set [soft 0x085a3b3c SI_sprdCreateDefaultClause]:
/// `SI_calloc(1, 0x24)`; answers the clause.
pub fn create_default_clause(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x24);
    m.set_u32(p, CHPTYP);
    m.set_u32(p + 4, 0xe);
    m.set_u32(p + 8, 3);
    m.set_u32(p + 0xc, 0xffff_ffff);
    m.set_u32(p + 0x10, 0xffff_ffff);
    m.set_u32(p + 0x14, 0x14);
    m.set_u32(p + 0x18, 0);
    m.set_i16(p + 0x1c, 0);
    m.set_i16(p + 0x1e, 0);
    p
}

/// The C library's `%f` of a double.
fn fmt_f(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{:.6}", v)
}

/// The text of an attribute value into `out` [soft 0x085a3b94 getAttrValueStr]. The value
/// is a type word (1 float, 2 boolean, 3 enum pair, 4 and 11 integer, 5 string pointer,
/// 6 double, 7 character, 8 short, 9 float at +8); anything else prints `UNKOWN`.
pub fn get_attr_value_str(m: &mut Mem, o: &mut dyn Outside, v: u32, out: u32) {
    m.set_u8(out, 0);
    let text: Vec<u8> = match m.u32(v).wrapping_sub(1) {
        0 => fmt_f(m.f32(v + 4) as f64).into_bytes(),
        1 => {
            if m.i16(v + 4) != 0 { b"TRUE".to_vec() } else { b"FALSE".to_vec() }
        }
        2 => {
            let (a, b) = (m.u32(v + 4), m.u32(v + 8));
            if a == 0xffff_ffff && b == 0xffff_ffff {
                ALL_WORD.to_vec()
            } else {
                let s = call(m, o, ENMDICT_ENUM_CODE_GET_BY_VALUE, &[SI_EVD, a, b]);
                m.cstr(s)
            }
        }
        3 | 10 => format!("{}", m.i32(v + 4)).into_bytes(),
        4 => {
            let p = m.u32(v + 4);
            // glibc prints a null string as "(null)"
            let s = if p == 0 { b"(null)".to_vec() } else { m.cstr(p) };
            let mut t = vec![b'"'];
            t.extend(s);
            t.push(b'"');
            t
        }
        5 => fmt_f(m.f64(v + 4)).into_bytes(),
        6 => vec![b'"', m.i8(v + 4) as i32 as u8, b'"'],
        7 => format!("{}", m.i16(v + 4)).into_bytes(),
        8 => fmt_f(m.f32(v + 8) as f64).into_bytes(),
        _ => b"UNKOWN".to_vec(),
    };
    m.put_cstr(out, &text);
}

/// The text of clause `idx` of `n` into the clause's own text pointer (+0x18)
/// [soft 0x085a3cb8 SI_sprdGenClauseStr], through the expression module's string builder.
pub fn gen_clause_str(m: &mut Mem, o: &mut dyn Outside, n: i32, clauses: u32, idx: i32) {
    let local = m.local(100);
    let clause = m.u32(clauses.wrapping_add((idx as u32).wrapping_mul(4)));
    let mut sum = 0i32;
    let mut i = 0i32;
    while i < idx {
        let c = m.u32(clauses.wrapping_add((i as u32).wrapping_mul(4)));
        sum = sum.wrapping_add(m.i16(c + 0x1c) as i32).wrapping_sub(m.i16(c + 0x1e) as i32);
        i += 1;
    }
    let mut s = 0u32;
    if sum > 0 {
        while sum != 0 {
            s = call(m, o, CHECK_ALLOC_STR, &[s, 0x086f_6d53, 0]);
            sum -= 1;
        }
    }
    let mut i = 0i32;
    while i < m.i16(clause + 0x1c) as i32 {
        s = call(m, o, CHECK_ALLOC_STR, &[s, 0x086f_6d56, 0]);
        i += 1;
    }
    s = call(m, o, CHECK_ALLOC_STR, &[s, 0x086f_6d59, 0]);
    let name = m.u32(clause);
    s = call(m, o, CHECK_ALLOC_STR, &[s, name, 1]);
    let op = m.u32(clause + 4);
    call(m, o, GET_EXPR_OPER_STR, &[op, local]);
    s = call(m, o, CHECK_ALLOC_STR, &[s, local, 1]);
    get_attr_value_str(m, o, clause + 8, local);
    s = call(m, o, CHECK_ALLOC_STR, &[s, local, 1]);
    let mut i = 0i32;
    while i < m.i16(clause + 0x1e) as i32 {
        s = call(m, o, CHECK_ALLOC_STR, &[s, 0x086f_6d5f, 0]);
        i += 1;
    }
    if idx < n.wrapping_sub(1) {
        m.set_u8(local, 0);
        let op = m.u32(clause + 0x14);
        call(m, o, GET_EXPR_OPER_STR, &[op, local]);
        s = call(m, o, CHECK_ALLOC_STR, &[s, local, 0]);
    }
    free_field(m, clause + 0x18);
    m.set_u32(clause + 0x18, s);
    m.release(100);
}

/// A sub group appended to a group's array and initialised; answers it
/// [soft 0x085a3e3c si_sprdAddSprdSetupGroup].
pub fn add_sprd_setup_group(m: &mut Mem, o: &mut dyn Outside, grp: u32, name: u32) -> u32 {
    let n = m.i16(grp + g::NSUB) as i32;
    let p = if m.u32(grp + g::SUB) == 0 {
        m.si_malloc(0x50)
    } else {
        let old = m.u32(grp + g::SUB);
        m.si_realloc(old, ((n.wrapping_mul(5).wrapping_add(5)) as u32).wrapping_mul(0x10))
    };
    m.set_u32(grp + g::SUB, p);
    let q = m.u32(grp + g::SUB).wrapping_add((n as u32).wrapping_mul(0x50));
    init_group_setup(m, o, q);
    if name != 0 {
        let s = m.strdup(name);
        m.set_u32(q, s);
    }
    let c = m.i16(grp + g::NSUB).wrapping_add(1);
    m.set_i16(grp + g::NSUB, c);
    q
}

/// A sub list appended to an element's array (count at +0x10, array at +0x14, 0xc bytes
/// a list) [soft 0x085a3ebc si_sprdAddSubElemList].
pub fn add_sub_elem_list(m: &mut Mem, e: u32) {
    let n = m.i32(e + 0x10);
    let p = if m.u32(e + 0x14) == 0 {
        m.si_malloc(0xc)
    } else {
        let old = m.u32(e + 0x14);
        m.si_realloc(old, (n.wrapping_mul(3).wrapping_add(3) as u32).wrapping_mul(4))
    };
    m.set_u32(e + 0x14, p);
    let q = m.u32(e + 0x14).wrapping_add((n as u32).wrapping_mul(0xc));
    m.set_u32(q, 0);
    m.set_u32(q + 4, 0);
    m.set_i16(q + 8, 1);
    let c = m.i32(e + 0x10).wrapping_add(1);
    m.set_i32(e + 0x10, c);
}

/// Insert an item in a list of `lst` (count +0, items +4, directions +8, relative to the
/// list's base `base` in the group); shared by the sort and the field list.
fn add_item(m: &mut Mem, base: u32, idx: i32, item: u32, dir: i16) {
    let (cnt_a, items_a, dirs_a) = (base, base + 4, base + 8);
    let mut cnt = m.i32(cnt_a);
    let (p, d);
    if m.u32(items_a) == 0 {
        p = m.si_malloc(0x14);
        m.set_u32(items_a, p);
        d = m.si_malloc(2);
    } else {
        let old = m.u32(items_a);
        p = m.si_realloc(old, (cnt.wrapping_add(1) as u32).wrapping_mul(0x14));
        m.set_u32(items_a, p);
        let oldd = m.u32(dirs_a);
        d = m.si_realloc(oldd, (cnt.wrapping_add(1) as u32).wrapping_mul(2));
    }
    m.set_u32(dirs_a, d);
    let mut at = cnt;
    // the position counts only below the last item (the original appends for idx == count - 1)
    if idx > -1 && idx < cnt.wrapping_sub(1) {
        at = idx;
        if idx < cnt {
            let mut off = (cnt as u32).wrapping_mul(0x14);
            while idx < cnt {
                let items = m.u32(items_a);
                clib::memcpy(m, items.wrapping_add(off), items.wrapping_add(off).wrapping_sub(0x14), 0x14);
                off = off.wrapping_sub(0x14);
                let dirs = m.u32(dirs_a);
                let v = m.i16(dirs.wrapping_add((cnt as u32).wrapping_mul(2)).wrapping_sub(2));
                m.set_i16(dirs.wrapping_add((cnt as u32).wrapping_mul(2)), v);
                cnt -= 1;
            }
        }
    }
    let items = m.u32(items_a);
    clib::memcpy(m, items.wrapping_add((at as u32).wrapping_mul(0x14)), item, 0x14);
    let dirs = m.u32(dirs_a);
    m.set_i16(dirs.wrapping_add((at as u32).wrapping_mul(2)), dir);
    let c = m.i32(cnt_a).wrapping_add(1);
    m.set_i32(cnt_a, c);
}

/// An item inserted in the sort list [soft 0x085a3f28 si_sprdAddSortItem].
pub fn add_sort_item(m: &mut Mem, grp: u32, idx: i32, item: u32, dir: i16) {
    add_item(m, grp + 0x28, idx, item, dir)
}

/// An item inserted in the field list [soft 0x085a432c si_sprdAddFieldItem]; `dir` is stored as 16 bits.
pub fn add_field_item(m: &mut Mem, grp: u32, idx: i32, item: u32, dir: i32) {
    add_item(m, grp + 0x34, idx, item, dir as i16)
}

/// Remove item `idx`; 0 when there is none, 1 when removed. Shared by the two lists.
fn rem_item(m: &mut Mem, base: u32, idx: i32) -> u32 {
    let (cnt_a, items_a, dirs_a) = (base, base + 4, base + 8);
    let cnt = m.i32(cnt_a);
    if m.u32(items_a) == 0 || idx < 0 || cnt <= idx {
        return 0;
    }
    if cnt == 1 {
        let p = m.u32(items_a);
        m.free(p);
        m.set_u32(items_a, 0);
        free_field(m, dirs_a);
        m.set_u32(items_a, 0);
        m.set_u32(dirs_a, 0);
    }
    let last = cnt - 1;
    let d;
    if idx == last {
        let old = m.u32(items_a);
        let p = m.si_realloc(old, (idx as u32).wrapping_mul(0x14));
        m.set_u32(items_a, p);
        let oldd = m.u32(dirs_a);
        d = m.si_realloc(oldd, (idx as u32).wrapping_mul(2));
    } else {
        let a = m.si_malloc((last as u32).wrapping_mul(0x14));
        d = m.si_malloc((last as u32).wrapping_mul(2));
        let (n20, n2) = ((idx as u32).wrapping_mul(0x14), (idx as u32).wrapping_mul(2));
        let (items, dirs) = (m.u32(items_a), m.u32(dirs_a));
        clib::memcpy(m, a, items, n20);
        clib::memcpy(m, d, dirs, n2);
        let k = (cnt - idx - 1) as u32;
        let (items, dirs) = (m.u32(items_a), m.u32(dirs_a));
        clib::memcpy(m, a.wrapping_add(n20), items.wrapping_add(n20).wrapping_add(0x14), k.wrapping_mul(0x14));
        clib::memcpy(m, d.wrapping_add(n2), dirs.wrapping_add(n2).wrapping_add(2), k.wrapping_mul(2));
        free_field(m, items_a);
        free_field(m, dirs_a);
        m.set_u32(items_a, a);
    }
    m.set_u32(dirs_a, d);
    let c = m.i32(cnt_a).wrapping_sub(1);
    m.set_i32(cnt_a, c);
    1
}

/// [soft 0x085a4044 si_sprdRemSortItem]
pub fn rem_sort_item(m: &mut Mem, grp: u32, idx: i32) -> u32 {
    rem_item(m, grp + 0x28, idx)
}

/// [soft 0x085a4440 si_sprdRemFieldItem]
pub fn rem_field_item(m: &mut Mem, grp: u32, idx: i32) -> u32 {
    rem_item(m, grp + 0x34, idx)
}

/// Swap item `idx` with its neighbour (the one before for a non-zero flag, otherwise the next).
fn move_item(m: &mut Mem, base: u32, idx: i32, back: i16) -> u32 {
    let other = if back != 0 { idx.wrapping_sub(1) } else { idx.wrapping_add(1) };
    let cnt = m.i32(base);
    if idx < 0 || cnt <= idx || other < 0 || cnt <= other {
        return 0;
    }
    let tmp = m.local(20);
    let items = m.u32(base + 4);
    clib::memcpy(m, tmp, items.wrapping_add((other as u32).wrapping_mul(0x14)), 0x14);
    let dirs = m.u32(base + 8);
    let keep = m.i16(dirs.wrapping_add((other as u32).wrapping_mul(2)));
    let items = m.u32(base + 4);
    clib::memcpy(m, items.wrapping_add((other as u32).wrapping_mul(0x14)), items.wrapping_add((idx as u32).wrapping_mul(0x14)), 0x14);
    let items = m.u32(base + 4);
    clib::memcpy(m, items.wrapping_add((idx as u32).wrapping_mul(0x14)), tmp, 0x14);
    let dirs = m.u32(base + 8);
    let v = m.i16(dirs.wrapping_add((idx as u32).wrapping_mul(2)));
    m.set_i16(dirs.wrapping_add((other as u32).wrapping_mul(2)), v);
    let dirs = m.u32(base + 8);
    m.set_i16(dirs.wrapping_add((idx as u32).wrapping_mul(2)), keep);
    m.release(20);
    1
}

/// [soft 0x085a481c si_sprdMoveFieldItem]
pub fn move_field_item(m: &mut Mem, grp: u32, idx: i32, back: i16) -> u32 {
    move_item(m, grp + 0x34, idx, back)
}

/// [soft 0x085a48d4 si_sprdMoveSortItem]
pub fn move_sort_item(m: &mut Mem, grp: u32, idx: i32, back: i16) -> u32 {
    move_item(m, grp + 0x28, idx, back)
}

/// The order of two elements of the list being sorted, by the sort list in
/// `glbSprdSortList` [soft 0x085a4220 sprdSortCompareElems]. Both must lie in
/// `[glbSprdSortStart, glbSprdSortEnd]`, else 0.
pub fn sort_compare_elems(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    let local = m.local(4);
    m.set_i32(local, 0);
    let (list, start, end) = (m.u32(SORT_LIST), m.u32(SORT_START), m.u32(SORT_END));
    let r;
    if a == 0 || b == 0 || list == 0 || start == 0 || end == 0 || a < start || b < start || a > end || b > end {
        r = 0;
    } else {
        let mut i = 0u32;
        let mut ret = None;
        loop {
            let list = m.u32(SORT_LIST);
            if (i as i32) >= m.i32(list) {
                break;
            }
            let item = m.u32(list + 4).wrapping_add(i * 0x14);
            call(m, o, ED_ELEMENT_ATT_COMP_VALUE, &[a, b, item, local]);
            let v = m.i32(local);
            if v != 0 {
                let list = m.u32(SORT_LIST);
                let dir = m.i16(m.u32(list + 8).wrapping_add(i * 2));
                ret = Some(if dir != 0 { v } else { v.wrapping_neg() });
                break;
            }
            i += 1;
        }
        r = ret.unwrap_or_else(|| m.i32(local));
    }
    m.release(4);
    r
}

/// An element list sorted in place by a group's sort list [soft 0x085a42c4 si_sprdSortSprdList]:
/// the elements are 0x18 bytes; the list's 16-bit flag (+8) is cleared afterwards.
pub fn sort_sprd_list(m: &mut Mem, o: &mut dyn Outside, grp: u32, l: u32) {
    if l != 0 && m.i16(l + 8) != 0 && m.i32(grp + 0x28) != 0 {
        let n = m.i32(l);
        if n > 1 {
            let start = m.u32(l + 4);
            m.set_u32(SORT_START, start);
            m.set_u32(SORT_LIST, grp + 0x28);
            m.set_u32(SORT_END, start.wrapping_add((n as u32).wrapping_mul(0x18)).wrapping_sub(0x18));
            clib::qsort(m, start, n as u32, 0x18, &mut |m, a, b| sort_compare_elems(m, o, a, b));
            m.set_i16(l + 8, 0);
            m.set_u32(SORT_LIST, 0);
        }
    }
}

/// An element appended to an element list and set from an identity (4 words); answers the
/// element [soft 0x085a4610 si_sprdAddElemListItem].
pub fn add_elem_list_item(m: &mut Mem, l: u32, ident: u32) -> u32 {
    let n = m.i32(l);
    let p = if m.u32(l + 4) == 0 {
        m.si_malloc(0x18)
    } else {
        let old = m.u32(l + 4);
        m.si_realloc(old, (n.wrapping_mul(3).wrapping_add(3) as u32).wrapping_mul(8))
    };
    m.set_u32(l + 4, p);
    let e = m.u32(l + 4).wrapping_add((n as u32).wrapping_mul(0x18));
    init_elem(m, e);
    for k in 0..4 {
        let w = m.u32(ident + k * 4);
        m.set_u32(e + k * 4, w);
    }
    let c = m.i32(l).wrapping_add(1);
    m.set_i32(l, c);
    e
}

/// An element list emptied, each element's sub lists first [soft 0x085a4a44 si_sprdElemListDispose].
pub fn elem_list_dispose(m: &mut Mem, l: u32) {
    let mut i = 0i32;
    while i < m.i32(l) {
        let e = m.u32(l + 4).wrapping_add((i as u32).wrapping_mul(0x18));
        elem_dispose(m, e);
        i += 1;
    }
    free_field(m, l + 4);
    m.set_u32(l, 0);
}

/// An element's sub lists emptied and freed [soft 0x085a4a98 si_sprdElemDispose].
pub fn elem_dispose(m: &mut Mem, e: u32) {
    let mut i = 0i32;
    while i < m.i32(e + 0x10) {
        let l = m.u32(e + 0x14).wrapping_add((i as u32).wrapping_mul(0xc));
        elem_list_dispose(m, l);
        i += 1;
    }
    free_field(m, e + 0x14);
    m.set_u32(e + 0x10, 0);
}

/// The first 0x18 bytes at `a` and `b` equal (stopping at the first difference, as `repz cmpsb`).
fn same24(m: &Mem, a: u32, b: u32) -> bool {
    (0..0x18u32).all(|k| m.u8(a.wrapping_add(k)) == m.u8(b.wrapping_add(k)))
}

/// The element whose first 0x18 bytes equal those at `ident` removed from an element list
/// [soft 0x085a4694 si_sprdRemElemListItem]. A list of one element loses it whatever it is;
/// the match compares 0x18 bytes although an identity is 0x10.
pub fn rem_elem_list_item(m: &mut Mem, l: u32, ident: u32) {
    let n = m.i32(l);
    if n == 1 {
        let e = m.u32(l + 4);
        elem_dispose(m, e);
        m.set_u32(l, 0);
        free_field(m, l + 4);
        return;
    }
    let base = m.u32(l + 4);
    let mut found = -1i32;
    let mut i = 0i32;
    while i < n {
        if same24(m, base.wrapping_add((i as u32).wrapping_mul(0x18)), ident) {
            found = i;
            break;
        }
        i += 1;
    }
    if found == -1 {
        return;
    }
    let off = (found as u32).wrapping_mul(0x18);
    let e = m.u32(l + 4).wrapping_add(off);
    elem_dispose(m, e);
    if found == n - 1 {
        let old = m.u32(l + 4);
        let p = m.si_realloc(old, off);
        m.set_u32(l + 4, p);
    } else {
        let a = m.si_malloc(((n - 1) as u32).wrapping_mul(0x18));
        let old = m.u32(l + 4);
        clib::memcpy(m, a, old, off);
        let old = m.u32(l + 4);
        clib::memcpy(m, a.wrapping_add(off), old.wrapping_add(off).wrapping_add(0x18), ((n - found - 1) as u32).wrapping_mul(0x18));
        free_field(m, l + 4);
        m.set_u32(l + 4, a);
    }
    let c = m.i32(l).wrapping_sub(1);
    m.set_i32(l, c);
}

/// Which sub group of `parent` (searching its sub groups' too) is the group at `target`:
/// its index among its siblings, with the sibling's parent stored at `*out`; -1 when none
/// [soft 0x085a4c38 sprdFindGroupSetup]. The index is the byte offset divided by 0x50 with
/// the compiler's reciprocal multiplication (`0xcccccccd`, `.rodata` 0x08672c24, shifted by 38).
pub fn find_group_setup(m: &mut Mem, parent: u32, target: u32, out: u32) -> u32 {
    let p = m.u32(parent + g::SUB);
    if p != 0 {
        let n = m.i16(parent + g::NSUB) as i32;
        let hi = p.wrapping_add((n as u32).wrapping_mul(0x50)).wrapping_sub(0x50);
        if target >= p && target <= hi {
            m.set_u32(out, parent);
            let d = target.wrapping_sub(m.u32(parent + g::SUB)) as u64;
            return ((d * 0xcccc_cccd) >> 38) as u32;
        }
        let mut i = 0i32;
        while i < n {
            let sub = m.u32(parent + g::SUB).wrapping_add((i as u32).wrapping_mul(0x50));
            let r = find_group_setup(m, sub, target, out);
            if r != 0xffff_ffff {
                return r;
            }
            i += 1;
        }
    }
    0xffff_ffff
}

/// A sub group of a setup disposed of and taken out of its parent's array
/// [soft 0x085a4ccc si_sprdSubGroupSetupDispose]; 0 when it is not found.
pub fn sub_group_setup_dispose(m: &mut Mem, o: &mut dyn Outside, setup: u32, grp: u32) -> u32 {
    let local = m.local(4);
    m.set_u32(local, 0);
    let mut idx = find_group_setup(m, setup + 8, grp, local) as i32;
    let parent = m.u32(local);
    let r;
    if idx == -1 || parent == 0 {
        r = 0;
    } else {
        group_setup_dispose(m, o, grp);
        let n = m.i16(parent + g::NSUB);
        if n < 2 {
            m.set_i16(parent + g::NSUB, n.wrapping_sub(1));
            free_field(m, parent + g::SUB);
        } else {
            if (idx as i32) < n as i32 - 1 {
                let mut off = (idx as u32).wrapping_mul(0x50);
                loop {
                    let a = m.u32(parent + g::SUB);
                    clib::memcpy(m, a.wrapping_add(off), a.wrapping_add(off).wrapping_add(0x50), 0x50);
                    off = off.wrapping_add(0x50);
                    idx += 1;
                    if !(idx < m.i16(parent + g::NSUB) as i32 - 1) {
                        break;
                    }
                }
            }
            let c = m.i16(parent + g::NSUB).wrapping_sub(1);
            m.set_i16(parent + g::NSUB, c);
            let old = m.u32(parent + g::SUB);
            let p = m.si_realloc(old, (c as i32 as u32).wrapping_mul(0x50));
            m.set_u32(parent + g::SUB, p);
        }
        r = 1;
    }
    m.release(4);
    r
}

/// A setup taken apart and freed [soft 0x085a4dc4 si_sprdSetupDispose].
pub fn setup_dispose(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    free_field(m, s);
    group_setup_dispose(m, o, s + 8);
    elem_list_dispose(m, s + 0x58);
    if s != 0 {
        m.free(s);
    }
}

/// The first group (searching sub groups) whose query text is not valid, as a pointer into
/// that text; 0 when all are valid. Sets each group's 16-bit "invalid" flag (+0x24)
/// [soft 0x085a4e38 checkIfSubQueryAreOk].
pub fn check_if_sub_query_are_ok(m: &mut Mem, o: &mut dyn Outside, grp: u32) -> u32 {
    let text = m.u32(grp + 0x14);
    let len = clib::strlen(m, text);
    let s = m.si_calloc(len.wrapping_add(3), 1);
    let mut line = b"= ".to_vec();
    line.extend(m.cstr(text));
    m.put_cstr(s, &line);
    m.set_i16(grp + 0x24, 0);
    let bad = call(m, o, VALID_CUSTOM_EXPRESS_R, &[s, 0, 0, 0, 1, 1]);
    let r;
    if bad == 0 {
        let mut child = m.u32(grp + g::SUB);
        let mut res = 0;
        let mut i = 0i32;
        while i < m.i16(grp + g::NSUB) as i32 {
            res = check_if_sub_query_are_ok(m, o, child);
            if res != 0 {
                break;
            }
            child = child.wrapping_add(0x50);
            i += 1;
        }
        r = res;
    } else {
        m.set_i16(grp + 0x24, 1);
        let text = m.u32(grp + 0x14);
        r = clib::strstr(m, text, bad);
    }
    if s != 0 {
        m.free(s);
    }
    r
}
const VALID_CUSTOM_EXPRESS_R: si_mem::Routine = VALID_CUSTOM_EXPRESS;

/// Whether every query of a setup is valid, in query mode: the 16-bit flag at +0x74 is 1
/// when one is not [soft 0x085a4f50 si_checkIfQueryIsOk].
pub fn check_if_query_is_ok(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    m.set_i16(s + 0x74, 0);
    m.set_i16(SI_QUERY_MODE, 1);
    let r = check_if_sub_query_are_ok(m, o, s + 8);
    m.set_i16(SI_QUERY_MODE, 0);
    m.set_i16(s + 0x74, (r != 0) as i16);
}

fn fprintf(m: &mut Mem, o: &mut dyn Outside, fmt: u32, args: &[u32]) {
    let mut a = vec![m.u32(STDERR), fmt];
    a.extend_from_slice(args);
    call(m, o, FPRINTF, &a);
}

fn dump_items(m: &mut Mem, o: &mut dyn Outside, grp: u32, base: u32, elem: u32, oper: u32, fmt: u32) {
    let mut i = 0i32;
    while i < m.i32(grp + base) {
        let items = m.u32(grp + base + 4);
        let item = items.wrapping_add((i as u32).wrapping_mul(0x14));
        let rec = call(m, o, ATTDICT_ATTRIBUTE_GET_BY_INDEX, &[SI_ATD, item]);
        let kind = if m.u32(rec) == 0 { elem } else { oper };
        let name = m.u32(rec + 0x20);
        let fcv = m.u32(m.u32(rec + 0x5c).wrapping_add(m.u32(item + 0x10).wrapping_mul(4)));
        fprintf(m, o, fmt, &[i as u32, kind, name, fcv]);
        i += 1;
    }
}

/// A group's fields and sort lists printed on `stderr`, then its sub groups'
/// [soft 0x085a4fa0 sprdGroupSetupDump].
pub fn group_setup_dump(m: &mut Mem, o: &mut dyn Outside, grp: u32) {
    let name = m.u32(grp);
    fprintf(m, o, 0x086f_6d67, &[name]);
    fprintf(m, o, 0x086f_6d85, &[]);
    fprintf(m, o, 0x086f_6da0, &[]);
    dump_items(m, o, grp, 0x34, 0x086f_6dae, 0x086f_6db3, 0x086f_6dc0);
    fprintf(m, o, 0x086f_6de8, &[]);
    dump_items(m, o, grp, 0x28, 0x086f_6df4, 0x086f_6df9, 0x086f_6e00);
    let mut i = 0i32;
    while i < m.i16(grp + g::NSUB) as i32 {
        let sub = m.u32(grp + g::SUB).wrapping_add((i as u32).wrapping_mul(0x50));
        group_setup_dump(m, o, sub);
        i += 1;
    }
}

/// A setup printed on `stderr` [soft 0x085a5108 SI_sprdSetupDump].
pub fn setup_dump(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    fprintf(m, o, 0x086f_6e28, &[]);
    fprintf(m, o, 0x086f_6e40, &[]);
    group_setup_dump(m, o, s + 8);
    fprintf(m, o, 0x086f_6e80, &[]);
}

#[allow(dead_code)]
fn _texts_used() -> usize {
    TEXTS.len() + list::ITEM as usize
}
