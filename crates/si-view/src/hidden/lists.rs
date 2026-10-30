// SPDX-License-Identifier: MIT
//! The chunked lists of `hidden.c`: vectors, original vectors and edges are appended to
//! lists that grow by one chunk of 0x2000 (0x4000 for edges) entries at a time.

use crate::outside::{self, Outside};
use si_mem::Mem;

const CHUNK_HID: u32 = 0x086b_afc0;
const ERROR: u32 = 0x086b_afe0;
const FP: u32 = 0x086b_afe4;
const NB_VCT: u32 = 0x087f_b810;
const EDGE_MEM: u32 = 0x087f_b804;
const EDGE_CUR: u32 = 0x087f_b808;
const EDGE_N: u32 = 0x087f_b80c;
const NB_EDG: u32 = 0x087f_b818;
const NB_EDG1: u32 = 0x087f_b81c;
const NB_EDG2: u32 = 0x087f_b820;
const TOT_EDG: u32 = 0x087f_b828;
const MSG_INSERT_EDGE: u32 = 0x086b_b26a;

/// Makes room in a list: `[0]` the array of chunks, `[4]` the chunk in use, `[8]` the entry
/// in it, `[0xc]` how many chunks there are. When the chunk in use is full a pointer is
/// added to the array (a new array of 4 bytes the first time, else the old one
/// reallocated to the new count) and a chunk of `bytes` is allocated for it, by
/// `SI_chunkMalloc (0x0839e3f4)` on `chunkHid` or, with `chunk` false, by `SI_malloc`.
fn grow(m: &mut Mem, o: &mut dyn Outside, list: u32, full: i32, bytes: u32, chunk: bool) {
    if m.i32(list + 8) == full {
        let n = m.i32(list + 0xc);
        let a;
        if n == 0 {
            m.set_i32(list + 8, 1);
            m.set_i32(list + 4, 0);
            m.set_i32(list + 0xc, 1);
            a = m.si_malloc(4);
        } else {
            m.set_i32(list + 8, 0);
            let c = m.i32(list + 4).wrapping_add(1);
            m.set_i32(list + 4, c);
            m.set_i32(list + 0xc, n.wrapping_add(1));
            a = m.si_realloc(m.u32(list), (n as u32).wrapping_mul(4).wrapping_add(4));
        }
        m.set_u32(list, a);
        let blk = if chunk {
            let h = m.u32(CHUNK_HID);
            o.call(m, &outside::SI_CHUNK_MALLOC, &[h, bytes])
        } else {
            m.si_malloc(bytes)
        };
        let at = m.u32(list).wrapping_add((m.i32(list + 4) as u32).wrapping_mul(4));
        m.set_u32(at, blk);
    }
}

/// Where the entry in use is: chunk base plus entry times `size`.
fn slot(m: &Mem, list: u32, size: u32) -> u32 {
    let base = m.u32(m.u32(list).wrapping_add((m.i32(list + 4) as u32).wrapping_mul(4)));
    base.wrapping_add((m.i32(list + 8) as u32).wrapping_mul(size))
}

/// Appends a vector of three longs to a list of vectors (12 bytes an entry, 0x2000 to a
/// chunk) and counts it in `nb_vct`; answers the count [soft 0x08343874 insertVector].
pub fn insert_vector(m: &mut Mem, o: &mut dyn Outside, list: u32, x: i32, y: i32, z: i32) -> i32 {
    grow(m, o, list, 0x2000, 0x18000, true);
    let s = slot(m, list, 12);
    m.set_i32(s, x);
    let s = slot(m, list, 12);
    m.set_i32(s + 4, y);
    let s = slot(m, list, 12);
    m.set_i32(s + 8, z);
    m.set_i32(list + 8, m.i32(list + 8).wrapping_add(1));
    let n = m.i32(NB_VCT).wrapping_add(1);
    m.set_i32(NB_VCT, n);
    n
}

/// The same for three floats, with chunks from `SI_malloc` [soft 0x08343950 insertOrgVector].
pub fn insert_org_vector(m: &mut Mem, o: &mut dyn Outside, list: u32, x: f32, y: f32, z: f32) {
    grow(m, o, list, 0x2000, 0x18000, false);
    let s = slot(m, list, 12);
    m.set_f32(s, x);
    let s = slot(m, list, 12);
    m.set_f32(s + 4, y);
    let s = slot(m, list, 12);
    m.set_f32(s + 8, z);
    m.set_i32(list + 8, m.i32(list + 8).wrapping_add(1));
}

/// Appends an edge (0x18 bytes: a pointer to its record, four longs, a kind) to a list of
/// edges (0x4000 to a chunk). With `info` not null a record of six words is taken from the
/// pool `edge_mem` (blocks of 1000 records and a link, 0x5dc4 bytes) and the edge points at
/// it. The kind counts the edge: 0 and 1 in `nb_edg`, 2 and 3 in `nb_edg2`, -1 in `nb_edg1`;
/// any other kind prints a message to `fp` and sets `Error` to 1. `tot_edg` counts all
/// [soft 0x08343a18 insertEdge].
#[allow(clippy::too_many_arguments)]
pub fn insert_edge(m: &mut Mem, o: &mut dyn Outside, list: u32, a: i32, b: i32, c: i32, d: i32, info: u32, kind: i32) {
    grow(m, o, list, 0x4000, 0x60000, true);
    let s = slot(m, list, 0x18);
    m.set_i32(s + 4, a);
    let s = slot(m, list, 0x18);
    m.set_i32(s + 8, b);
    let s = slot(m, list, 0x18);
    m.set_i32(s + 0xc, c);
    let s = slot(m, list, 0x18);
    m.set_i32(s + 0x10, d);
    let s = slot(m, list, 0x18);
    m.set_i32(s + 0x14, kind);
    if info == 0 {
        let s = slot(m, list, 0x18);
        m.set_u32(s, 0);
    } else {
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
                let nx = m.u32(EDGE_CUR);
                let nx = m.u32(nx);
                m.set_u32(nx, 0);
            }
            let nx = m.u32(m.u32(EDGE_CUR));
            m.set_u32(EDGE_CUR, nx);
            m.set_i32(EDGE_N, 0);
        }
        let rec = m.u32(EDGE_CUR).wrapping_add((m.i32(EDGE_N) as u32).wrapping_mul(24)).wrapping_add(4);
        m.set_i32(EDGE_N, m.i32(EDGE_N).wrapping_add(1));
        m.set_u32(0x087f_b724, rec);
        m.set_u32(rec + 4, info);
        m.set_u32(rec + 8, 0);
        m.set_u32(rec, 0);
        m.set_u32(rec + 0x14, 1);
        m.set_u32(rec + 0x10, 0);
        m.set_u32(rec + 0xc, 0);
        let s = slot(m, list, 0x18);
        m.set_u32(s, rec);
    }
    m.set_i32(list + 8, m.i32(list + 8).wrapping_add(1));
    match kind {
        0 | 1 => m.set_i32(NB_EDG, m.i32(NB_EDG).wrapping_add(1)),
        2 | 3 => m.set_i32(NB_EDG2, m.i32(NB_EDG2).wrapping_add(1)),
        -1 => m.set_i32(NB_EDG1, m.i32(NB_EDG1).wrapping_add(1)),
        _ => {
            let fp = m.u32(FP);
            o.call(m, &outside::FPRINTF, &[fp, MSG_INSERT_EDGE]);
            m.set_i32(ERROR, 1);
        }
    }
    m.set_i32(TOT_EDG, m.i32(TOT_EDG).wrapping_add(1));
}

const LIST_TRI: u32 = 0x086b_afc4;
const LIST_VCT: u32 = 0x086b_afc8;
const LIST_EDG: u32 = 0x087f_b7f8;
const LIST_EDG1: u32 = 0x087f_b7fc;
const LIST_EDG2: u32 = 0x087f_b800;

/// Frees a chain of pool blocks, the deepest link first [soft 0x08344fa0 edgeFreeMem].
pub fn edge_free_mem(m: &mut Mem, p: u32) {
    if p != 0 {
        let next = m.u32(p);
        if next != 0 {
            edge_free_mem(m, next);
        }
        m.free(p);
    }
}

/// Gives back everything the hidden-line pass allocated: the triangle list, the array of
/// chunk pointers of each of the four lists (and clears it), the edge-record pool, then the
/// chunk buffer (`SI_chunkFreeAll (0x0857cb00)` on its value, `SI_chunkDestroy (0x0857ca5c)`
/// on its address) [soft 0x08340504 allFree].
pub fn all_free(m: &mut Mem, o: &mut dyn Outside) {
    let t = m.u32(LIST_TRI);
    if t != 0 {
        m.free(t);
        m.set_u32(LIST_TRI, 0);
    }
    for g in [LIST_VCT, LIST_EDG, LIST_EDG1, LIST_EDG2] {
        let l = m.u32(g);
        let a = m.u32(l);
        if a != 0 {
            m.free(a);
            m.set_u32(l, 0);
        }
    }
    let e = m.u32(EDGE_MEM);
    if e != 0 {
        edge_free_mem(m, e);
    }
    let h = m.u32(CHUNK_HID);
    o.call(m, &outside::SI_CHUNK_FREE_ALL, &[h]);
    o.call(m, &outside::SI_CHUNK_DESTROY, &[CHUNK_HID]);
}

/// Adds a triangle number to a list of triangles: blocks of eight numbers (after a link),
/// 0x24 bytes from `SI_chunkMalloc`; `[0]` the first block, `[4]` the one in use, `[0xc]`
/// how many it holds [soft 0x083405e8 inlistStoreTriangle].
pub fn inlist_store_triangle(m: &mut Mem, o: &mut dyn Outside, list: u32, tri: i32) {
    if m.u32(list + 4) == 0 {
        let h = m.u32(CHUNK_HID);
        let b = o.call(m, &outside::SI_CHUNK_MALLOC, &[h, 0x24]);
        m.set_u32(list, b);
        m.set_u32(list + 4, b);
        m.set_u32(b, 0);
        let cur = m.u32(list + 4);
        m.set_i32(cur + 4, tri);
        m.set_i32(list + 0xc, 1);
    } else {
        let at = (m.i32(list + 0xc) as u32).wrapping_mul(4).wrapping_add(4).wrapping_add(m.u32(list + 4));
        m.set_i32(at, tri);
        let n = m.i32(list + 0xc).wrapping_add(1);
        m.set_i32(list + 0xc, n);
        if n == 8 {
            m.set_i32(list + 0xc, 0);
            let h = m.u32(CHUNK_HID);
            let b = o.call(m, &outside::SI_CHUNK_MALLOC, &[h, 0x24]);
            let cur = m.u32(list + 4);
            m.set_u32(cur, b);
            let nx = m.u32(m.u32(list + 4));
            m.set_u32(list + 4, nx);
            m.set_u32(nx, 0);
        }
    }
}
