// SPDX-License-Identifier: MIT
//! The trees of `hidden.c`: for every triangle a tree (balanced by the AVL rule, records of
//! five words) of the triangles it meets, and for every edge a tree of the triangles that
//! cut it. The original keeps its working pointers in `static` locals; they are written at
//! their addresses in the original's order.

use crate::hidden::lists::insert_edge;
use crate::outside::{self, Outside};
use si_mem::Mem;

const CHUNK_HID: u32 = 0x086b_afc0;
const LIST_TRI: u32 = 0x086b_afc4;
const LIST_EDG2: u32 = 0x087f_b800;
const EDGE_MEM: u32 = 0x087f_b804;
const EDGE_CUR: u32 = 0x087f_b808;
const EDGE_N: u32 = 0x087f_b80c;

// statics of intritreeStoreTriangle
const I_P: u32 = 0x087f_b6e8;
const I_Q: u32 = 0x087f_b6ec;
const I_R: u32 = 0x087f_b6f0;
const I_S: u32 = 0x087f_b6f4;
const I_T: u32 = 0x087f_b6f8;
const I_A: u32 = 0x087f_b6fc;
// statics of inedgtreeStoreTriangle
const E_P: u32 = 0x087f_b700;
const E_Q: u32 = 0x087f_b704;
const E_R: u32 = 0x087f_b708;
const E_S: u32 = 0x087f_b70c;
const E_T: u32 = 0x087f_b710;
const E_A: u32 = 0x087f_b714;
// statics of xferList
const X_I: u32 = 0x087f_b718;
const X_TRI: u32 = 0x087f_b71c;
const X_LST: u32 = 0x087f_b720;

/// A tree record: `[0]` left, `[4]` right, `[8]` parent (edge trees only), `[0xc]` the key
/// in the triangle trees' records at `[8]` (see each routine), `[0x10]` balance.
fn w(m: &Mem, p: u32, off: u32) -> u32 {
    m.u32(p.wrapping_add(off))
}
fn put(m: &mut Mem, p: u32, off: u32, v: u32) {
    m.set_u32(p.wrapping_add(off), v);
}

/// Stores, in the tree of triangle `min(a, b)`, the larger number `max(a, b)` with the
/// value `c` (records of five words from `SI_chunkMalloc` on `chunkHid`: left, right, key,
/// value, balance); a key already in the tree gives an edge (`insertEdge` on the list
/// `list_edg2`, kind 2) and answers 1, else 0 [soft 0x08340674 intritreeStoreTriangle].
/// The tree is kept balanced by single and double rotations and the root pointer of the
/// triangle (at 0x24 of its 0x38-byte record) follows the rotation.
pub fn intritree_store_triangle(m: &mut Mem, o: &mut dyn Outside, a: i32, b: i32, c: i32) -> i32 {
    let (lo, key) = if b < a { (b, a) } else { (a, b) };
    let slot = m.u32(LIST_TRI).wrapping_add(0x24).wrapping_add((lo as u32).wrapping_mul(0x38));
    let mut t = m.u32(slot);
    m.set_u32(I_T, t);
    let alloc = |m: &mut Mem, o: &mut dyn Outside| {
        let h = m.u32(CHUNK_HID);
        o.call(m, &outside::SI_CHUNK_MALLOC, &[h, 0x14])
    };
    if t == 0 {
        t = alloc(m, o);
        m.set_u32(I_T, t);
        let n = alloc(m, o);
        put(m, t, 4, n);
        let tri = m.u32(LIST_TRI);
        put(m, t, 0, 0);
        put(m, t, 0x10, 0);
        put(m, t, 8, 0);
        put(m, t, 0xc, 0);
        m.set_u32(tri.wrapping_add(0x24).wrapping_add((lo as u32).wrapping_mul(0x38)), t);
        let p = w(m, t, 4);
        m.set_u32(I_P, p);
        put(m, p, 8, key as u32);
        put(m, p, 0xc, c as u32);
        put(m, p, 4, 0);
        put(m, p, 0, 0);
        put(m, p, 0x10, 0);
        return 0;
    }
    let mut cur = w(m, t, 4);
    m.set_u32(I_P, cur);
    m.set_u32(I_S, cur);
    // descend to the place of the key, remembering the last node with a balance (s) and its parent (t)
    let q;
    loop {
        let p = cur;
        m.set_u32(I_P, p);
        let pk = w(m, p, 8) as i32;
        let next;
        if key < pk {
            let nx = w(m, p, 0);
            if nx == 0 {
                let n = alloc(m, o);
                let p = m.u32(I_P);
                m.set_u32(I_Q, n);
                put(m, p, 0, n);
                q = n;
                break;
            }
            next = nx;
        } else {
            if key <= pk {
                // the key is there: its value and this one make an edge
                let la = m.u32(LIST_EDG2);
                insert_edge(m, o, la, w(m, p, 0xc) as i32, c, lo, key, 0, 2);
                return 1;
            }
            let nx = w(m, p, 4);
            if nx == 0 {
                let n = alloc(m, o);
                let p = m.u32(I_P);
                m.set_u32(I_Q, n);
                put(m, p, 4, n);
                q = n;
                break;
            }
            next = nx;
        }
        m.set_u32(I_Q, next);
        if w(m, next, 0x10) != 0 {
            m.set_u32(I_T, p);
            m.set_u32(I_S, next);
        }
        cur = next;
    }
    put(m, q, 8, key as u32);
    put(m, q, 0xc, c as u32);
    put(m, q, 4, 0);
    put(m, q, 0, 0);
    put(m, q, 0x10, 0);
    let s = m.u32(I_S);
    let mut pp = if key < w(m, s, 8) as i32 { w(m, s, 0) } else { w(m, s, 4) };
    m.set_u32(I_P, pp);
    m.set_u32(I_R, pp);
    let r = pp;
    while pp != q {
        let k = w(m, pp, 8) as i32;
        if key < k {
            put(m, pp, 0x10, (-1i32) as u32);
            pp = w(m, pp, 0);
            m.set_u32(I_P, pp);
        } else if k < key {
            put(m, pp, 0x10, 1);
            pp = w(m, pp, 4);
            m.set_u32(I_P, pp);
        }
    }
    let a = if key < w(m, s, 8) as i32 { -1i32 } else { 1 };
    m.set_i32(I_A, a);
    let sb = w(m, s, 0x10) as i32;
    if sb == 0 {
        put(m, s, 0x10, a as u32);
        return 0;
    }
    if sb == a.wrapping_neg() {
        put(m, s, 0x10, 0);
        return 0;
    }
    let rb = w(m, r, 0x10) as i32;
    let p;
    if rb == a {
        p = r;
        m.set_u32(I_P, p);
        if a == 1 {
            put(m, s, 4, w(m, r, 0));
            put(m, r, 0, s);
        } else {
            put(m, s, 0, w(m, r, 4));
            put(m, r, 4, s);
        }
        put(m, r, 0x10, 0);
        put(m, s, 0x10, 0);
    } else if rb == a.wrapping_neg() {
        if a == 1 {
            p = w(m, r, 0);
            m.set_u32(I_P, p);
            put(m, r, 0, w(m, p, 4));
            put(m, p, 4, r);
            put(m, s, 4, w(m, p, 0));
            put(m, p, 0, s);
        } else {
            p = w(m, r, 4);
            m.set_u32(I_P, p);
            put(m, r, 4, w(m, p, 0));
            put(m, p, 0, r);
            put(m, s, 0, w(m, p, 4));
            put(m, p, 4, s);
        }
        let pb = w(m, p, 0x10) as i32;
        if pb == a {
            put(m, s, 0x10, a.wrapping_neg() as u32);
            put(m, r, 0x10, 0);
        } else if pb == 0 {
            put(m, s, 0x10, 0);
            put(m, r, 0x10, 0);
        } else {
            put(m, s, 0x10, 0);
            put(m, r, 0x10, a as u32);
        }
        put(m, p, 0x10, 0);
    } else {
        p = m.u32(I_P);
    }
    // the rotated subtree takes the place of s under its parent t
    let t = m.u32(I_T);
    if s == w(m, t, 4) {
        put(m, t, 4, p);
    } else {
        put(m, t, 0, p);
    }
    0
}

/// Takes the next edge record (six words) from the pool of blocks of 1000 records, making
/// the first block or the next one when needed [the pool of soft 0x08343a18 insertEdge].
fn edge_rec(m: &mut Mem) -> u32 {
    if m.u32(EDGE_MEM) == 0 {
        let blk = m.si_malloc(0x5dc4);
        m.set_u32(EDGE_MEM, blk);
        m.set_u32(EDGE_CUR, blk);
        m.set_u32(blk, 0);
        m.set_i32(EDGE_N, 0);
    } else if m.i32(EDGE_N) == 1000 {
        let cur = m.u32(EDGE_CUR);
        if m.u32(cur) == 0 {
            let blk = m.si_malloc(0x5dc4);
            m.set_u32(cur, blk);
            let nx = m.u32(m.u32(EDGE_CUR));
            m.set_u32(nx, 0);
        }
        let nx = m.u32(m.u32(EDGE_CUR));
        m.set_u32(EDGE_CUR, nx);
        m.set_i32(EDGE_N, 0);
    }
    let rec = m.u32(EDGE_CUR).wrapping_add((m.i32(EDGE_N) as u32).wrapping_mul(24)).wrapping_add(4);
    m.set_i32(EDGE_N, m.i32(EDGE_N).wrapping_add(1));
    rec
}

/// Sets `n`'s parent to `parent` and its side flag (`[0x14]`: 0 left, 1 right, 2 the
/// header's root, 3 the root itself) in the parent's child when `n` is not null.
fn adopt(m: &mut Mem, n: u32, parent: u32, child_off: u32, side: u32) {
    if n != 0 {
        put(m, n, 8, parent);
        let c = w(m, parent, child_off);
        put(m, c, 0x14, side);
    }
}

/// Adds triangle number `tri` to the tree of triangles of an edge: `*head` is a header
/// record whose right child is the root; records have six words (left, right, parent,
/// key, balance, side) from the pool `edge_mem`; a number already there changes nothing
/// [soft 0x083409e4 inedgtreeStoreTriangle].
pub fn inedgtree_store_triangle(m: &mut Mem, head: u32, tri: i32) {
    let t0 = m.u32(head);
    m.set_u32(E_T, t0);
    if t0 == 0 {
        let rec = edge_rec(m);
        m.set_u32(head, rec);
        let t = m.u32(head);
        m.set_u32(E_T, t);
        let r2 = edge_rec(m);
        put(m, t, 4, r2);
        put(m, t, 8, 0);
        put(m, t, 0, 0);
        put(m, t, 0x14, 2);
        put(m, t, 0x10, 0);
        put(m, t, 0xc, 0);
        let p = w(m, t, 4);
        m.set_u32(E_P, p);
        put(m, p, 0xc, tri as u32);
        put(m, p, 0x10, 0);
        put(m, p, 0x14, 3);
        put(m, p, 8, 0);
        put(m, p, 4, 0);
        put(m, p, 0, 0);
        return;
    }
    let mut cur = w(m, t0, 4);
    m.set_u32(E_P, cur);
    m.set_u32(E_S, cur);
    let q;
    loop {
        let p = cur;
        m.set_u32(E_P, p);
        let pk = w(m, p, 0xc) as i32;
        let next;
        if tri < pk {
            let nx = w(m, p, 0);
            if nx == 0 {
                let rec = edge_rec(m);
                m.set_u32(E_Q, rec);
                let p = m.u32(E_P);
                put(m, p, 0, rec);
                put(m, rec, 8, p);
                put(m, rec, 0x14, 0);
                q = rec;
                break;
            }
            next = nx;
        } else {
            if tri <= pk {
                return;
            }
            let nx = w(m, p, 4);
            if nx == 0 {
                let rec = edge_rec(m);
                m.set_u32(E_Q, rec);
                put(m, p, 4, rec);
                put(m, rec, 8, p);
                put(m, rec, 0x14, 1);
                q = rec;
                break;
            }
            next = nx;
        }
        m.set_u32(E_Q, next);
        if w(m, next, 0x10) != 0 {
            m.set_u32(E_T, p);
            m.set_u32(E_S, next);
        }
        cur = next;
    }
    put(m, q, 0xc, tri as u32);
    put(m, q, 4, 0);
    put(m, q, 0, 0);
    put(m, q, 0x10, 0);
    let s = m.u32(E_S);
    let mut pp = if tri < w(m, s, 0xc) as i32 { w(m, s, 0) } else { w(m, s, 4) };
    m.set_u32(E_P, pp);
    m.set_u32(E_R, pp);
    let r = pp;
    while pp != q {
        let k = w(m, pp, 0xc) as i32;
        if tri < k {
            put(m, pp, 0x10, (-1i32) as u32);
            pp = w(m, pp, 0);
            m.set_u32(E_P, pp);
        } else if k < tri {
            put(m, pp, 0x10, 1);
            pp = w(m, pp, 4);
            m.set_u32(E_P, pp);
        }
    }
    let a = if tri < w(m, s, 0xc) as i32 { -1i32 } else { 1 };
    m.set_i32(E_A, a);
    let sb = w(m, s, 0x10) as i32;
    if sb == 0 {
        put(m, s, 0x10, a as u32);
        return;
    }
    if sb == a.wrapping_neg() {
        put(m, s, 0x10, 0);
        return;
    }
    let rb = w(m, r, 0x10) as i32;
    let p;
    if rb == a {
        p = r;
        m.set_u32(E_P, p);
        if a == 1 {
            let x = w(m, r, 0);
            put(m, s, 4, x);
            adopt(m, x, s, 4, 1);
            put(m, r, 0, s);
            adopt(m, s, r, 0, 0);
        } else {
            let x = w(m, r, 4);
            put(m, s, 0, x);
            adopt(m, x, s, 0, 0);
            put(m, r, 4, s);
            adopt(m, s, r, 4, 1);
        }
        put(m, r, 0x10, 0);
        put(m, s, 0x10, 0);
    } else if rb == a.wrapping_neg() {
        if a == 1 {
            p = w(m, r, 0);
            m.set_u32(E_P, p);
            let x = w(m, p, 4);
            put(m, r, 0, x);
            adopt(m, x, r, 0, 0);
            put(m, p, 4, r);
            adopt(m, r, p, 4, 1);
            let y = w(m, p, 0);
            put(m, s, 4, y);
            adopt(m, y, s, 4, 1);
            put(m, p, 0, s);
            adopt(m, s, p, 0, 0);
        } else {
            p = w(m, r, 4);
            m.set_u32(E_P, p);
            let x = w(m, p, 0);
            put(m, r, 4, x);
            adopt(m, x, r, 4, 1);
            put(m, p, 0, r);
            adopt(m, r, p, 0, 0);
            let y = w(m, p, 4);
            put(m, s, 0, y);
            adopt(m, y, s, 0, 0);
            put(m, p, 4, s);
            adopt(m, s, p, 4, 1);
        }
        let pb = w(m, p, 0x10) as i32;
        if pb == a {
            put(m, s, 0x10, a.wrapping_neg() as u32);
            put(m, r, 0x10, 0);
        } else if pb == 0 {
            put(m, s, 0x10, 0);
            put(m, r, 0x10, 0);
        } else {
            put(m, s, 0x10, 0);
            put(m, r, 0x10, a as u32);
        }
        put(m, p, 0x10, 0);
    } else {
        p = m.u32(E_P);
    }
    let t = m.u32(E_T);
    if s == w(m, t, 4) {
        put(m, t, 4, p);
        adopt(m, p, t, 4, 1);
    } else {
        put(m, t, 0, p);
        adopt(m, p, t, 0, 0);
    }
    let root = w(m, m.u32(head), 4);
    put(m, root, 8, 0);
    put(m, root, 0x14, 3);
}

/// Stores every triangle number of a list of triangles (blocks of eight numbers after a
/// link: `[0]` the first block, `[4]` the last, `[0xc]` the count in the last), except
/// `skip1` and `skip2`, in the edge tree `*head` [soft 0x0834108c xferList].
pub fn xfer_list(m: &mut Mem, list: u32, head: u32, skip1: i32, skip2: i32) {
    if w(m, list, 4) == 0 {
        return;
    }
    let first = w(m, list, 0);
    m.set_u32(X_LST, first);
    let mut blk = first;
    if blk != w(m, list, 4) {
        loop {
            m.set_i32(X_I, 0);
            loop {
                let i = m.i32(X_I);
                let n = m.i32(m.u32(X_LST).wrapping_add(4).wrapping_add((i as u32).wrapping_shl(2)));
                m.set_i32(X_TRI, n);
                if n != skip1 && n != skip2 {
                    inedgtree_store_triangle(m, head, n);
                }
                let i = m.i32(X_I).wrapping_add(1);
                m.set_i32(X_I, i);
                if i > 7 {
                    break;
                }
            }
            blk = m.u32(m.u32(X_LST));
            m.set_u32(X_LST, blk);
            if blk == w(m, list, 4) {
                break;
            }
        }
    }
    m.set_i32(X_I, 0);
    if w(m, list, 0xc) as i32 > 0 {
        loop {
            let i = m.i32(X_I);
            let n = m.i32(m.u32(X_LST).wrapping_add(4).wrapping_add((i as u32).wrapping_shl(2)));
            m.set_i32(X_TRI, n);
            if n != skip1 && n != skip2 {
                inedgtree_store_triangle(m, head, n);
            }
            let i = m.i32(X_I).wrapping_add(1);
            m.set_i32(X_I, i);
            if i >= w(m, list, 0xc) as i32 {
                break;
            }
        }
    }
}
