// SPDX-License-Identifier: MIT
//! `order.c` on the shared memory image (finding 0024): the second form of [`crate::order`],
//! written beside it as the measure of what converting a module from Rust-owned structures
//! to the image takes. **[`crate::order`] stays the one in use**; this module is checked
//! against the same vector files (`model-emucheck`, `ORDER_IMAGE=1`).
//!
//! Here the routines take what the original takes and leave what it leaves: the mesh is an
//! `SI_Mesh` at an address, the file's tables are the heap blocks the original allocates
//! (same sizes, same order, grown by the same steps), its ten file-scope variables are at
//! their addresses in `.data`, and the answer is a new `SI_Mesh` in the heap. What that
//! adds over [`crate::order`]: the result's address and the order in which blocks are
//! freed are the original's and are compared with the emulated original's; a table that
//! moved under a pointer the original keeps using is the same stale block here; and the
//! tables can be looked at by any other crate, as memory.
//!
//! The code follows the original statement for statement, with its loops in the shape the
//! compiler left them (a test before a do-while), because the places where a pointer is
//! read again after a call, and the places where it is not, are behaviour.
//!
//! Class: integer. Positions, texture coordinates and colours are copied as 32-bit words.

// the loop tests are the original's (`do { … } while (a < b)` is `if !(a < b) { break }`)
#![allow(clippy::nonminimal_bool)]

use si_mem::{Mem, Ptr32};
use si_types::geom::{Mesh, PolyNode, Polygon, Vertex};

si_mem::record! {
    /// An entry of `polTable` (allocated as `nbPol + 1` records of 0x24 bytes; entry 0 is
    /// not used).
    pub struct Pol = "polTable[]", size 0x24 {
        0x00 material: i32, "the polygon's material number";
        0x04 nb: i32, "number of corners";
        0x08 verts: Ptr32<i32>, "per corner: the vertex; -1 separates contours";
        0x0c edges: Ptr32<i32>, "per corner: the edge leaving it, signed";
        0x10 u: Ptr32<u32>, "per corner: the word at +0x14 of the mesh's corner (u)";
        0x14 v: Ptr32<u32>, "per corner: the word at +0x18 (v)";
        0x18 color: Ptr32<u32>, "per corner: the word at +0x1c (colour)";
        0x1c extra: Ptr32<Ptr32<[u32; 14]>>, "per corner: a copy of the 56 bytes the corner's +0x20 points to, or null";
        0x20 done: u8, "set once the polygon was visited";
        0x21 pad_21: [u8; 3], "padding";
    }
}

si_mem::record! {
    /// An entry of `edgTable` (grown by 100 records; entry 0 is a scratch list).
    pub struct Edg = "edgTable[]", size 0x10 {
        0x00 v0: i32, "first vertex; -1 in a record not used yet";
        0x04 v1: i32, "second vertex";
        0x08 nb: i32, "number of polygons along the edge";
        0x0c pols: Ptr32<i32>, "the polygons, signed; grown by 4";
    }
}

si_mem::record! {
    /// An entry of `vrtTable` (grown by 200 records).
    pub struct Vrt = "vrtTable[]", size 0x18 {
        0x00 pos: [u32; 3], "the position, as words";
        0x0c flag: i16, "the vertex's flag";
        0x0e pad_0e: [u8; 2], "not written";
        0x10 nb: i32, "number of polygon pairs";
        0x14 pairs: Ptr32<[i32; 2]>, "pairs of polygons that follow each other round the vertex; grown by 4";
    }
}

si_mem::globals! {
    EDG_TABLE: Ptr32<Edg> = 0x086d6554, "edgTable", init 0;
    NB_EDG: i32 = 0x086d6558, "nbEdg", init 0;
    NB_POL: i32 = 0x086d655c, "nbPol", init 0;
    NB_POL_STACK: i32 = 0x086d6560, "nbPolStack", init 0;
    NB_TO_DO: i32 = 0x086d6564, "nbToDo", init 0;
    NB_VRT: i32 = 0x086d6568, "nbVrt", init 0;
    POL_STACK: Ptr32<i32> = 0x086d656c, "polStack", init 0;
    POL_TABLE: Ptr32<Pol> = 0x086d6570, "polTable", init 0;
    TO_DO_LIST: Ptr32<[i32; 4]> = 0x086d6574, "toDoList", init 0;
    VRT_TABLE: Ptr32<Vrt> = 0x086d6578, "vrtTable", init 0;
}

/// `abs` as the compiler wrote it: negate when negative (the smallest number stays).
fn abs(v: i32) -> i32 {
    if v < 0 {
        v.wrapping_neg()
    } else {
        v
    }
}

fn p<T>(addr: u32) -> Ptr32<T> {
    Ptr32::new(addr)
}

/// A new vertex, its number answered [soft 0x084789c0 createVertex]. The table grows by
/// 200 records when the count is a multiple of 200; the new records get no pairs.
fn create_vertex(m: &mut Mem, pos: [u32; 3], flag: i16) -> i32 {
    let nb = m[NB_VRT];
    if nb == (nb / 200).wrapping_mul(200) {
        let old = m[VRT_TABLE];
        let new: Ptr32<Vrt> = p(if old.is_null() { m.malloc(0x12c0) } else { m.realloc(old.addr(), (nb.wrapping_mul(3).wrapping_add(600)).wrapping_mul(8) as u32) });
        m[VRT_TABLE] = new;
        for i in nb..nb.wrapping_add(200) {
            m[new.add(i)].nb = 0;
            m[new.add(i)].pairs = Ptr32::NULL;
        }
    }
    let (t, nb) = (m[VRT_TABLE], m[NB_VRT]);
    let e = t.add(nb);
    m[e].pos = pos;
    m[e].flag = flag;
    m[e].nb = 0;
    m[e].pairs = Ptr32::NULL;
    m[NB_VRT] = nb.wrapping_add(1);
    nb
}

/// A new edge between two vertices, its number answered [soft 0x08478aa4 createEdge]. The
/// table grows by 100 records; numbering starts at 1.
fn create_edge(m: &mut Mem, a: i32, b: i32) -> i32 {
    let nb = m[NB_EDG];
    if nb == (nb / 100).wrapping_mul(100) {
        let old = m[EDG_TABLE];
        let new: Ptr32<Edg> = p(if old.is_null() { m.malloc(0x640) } else { m.realloc(old.addr(), nb.wrapping_add(100).wrapping_mul(0x10) as u32) });
        m[EDG_TABLE] = new;
        for i in nb..nb.wrapping_add(100) {
            m[new.add(i)] = Edg { v0: -1, v1: -1, nb: 0, pols: Ptr32::NULL };
        }
    }
    let t = m[EDG_TABLE];
    if m[NB_EDG] == 0 {
        m[NB_EDG] = 1;
    }
    let nb = m[NB_EDG];
    m[t.add(nb)] = Edg { v0: a, v1: b, nb: 0, pols: Ptr32::NULL };
    m[NB_EDG] = nb.wrapping_add(1);
    nb
}

/// [soft 0x08478b8c addPolygonToEdge]: `pol` as it is for a positive `e`, negated otherwise
/// (also for `e` = 0, the scratch record).
fn add_polygon_to_edge(m: &mut Mem, e: i32, pol: i32) {
    let sign = if 0 < e { 1 } else { -1i32 };
    let ed = m[EDG_TABLE].add(abs(e));
    if m[ed].nb as u32 & 3 == 0 {
        let old = m[ed].pols;
        let new = if old.is_null() { m.malloc(0x10) } else { m.realloc(old.addr(), (m[ed].nb as u32).wrapping_mul(4).wrapping_add(0x10)) };
        m[ed].pols = p(new);
    }
    let (pols, nb) = (m[ed].pols, m[ed].nb);
    m[pols.add(nb)] = sign.wrapping_mul(pol);
    m[ed].nb = nb.wrapping_add(1);
}

/// The edge between two vertices [soft 0x08479154 findEdges]: its number when it runs
/// from `a` to `b`, negated when it runs from `b` to `a`, 0 when there is none.
fn find_edges(m: &Mem, a: i32, b: i32) -> i32 {
    let mut i = 1;
    while i < m[NB_EDG] {
        let e = m[m[EDG_TABLE].add(i)];
        if e.v0 == a && e.v1 == b {
            return i;
        }
        if e.v0 == b && e.v1 == a {
            return i.wrapping_neg();
        }
        i += 1;
    }
    0
}

/// Builds the edges of every polygon [soft 0x084791b4 createEdgTable].
fn create_edg_table(m: &mut Mem) {
    let mut pol = 1;
    if 0 < m[NB_POL] {
        loop {
            let mut start = 0;
            let mut c = 0;
            let next = pol + 1;
            if 0 < m[m[POL_TABLE].add(pol)].nb {
                loop {
                    let verts = m[m[POL_TABLE].add(pol)].verts;
                    let v = m[verts.add(c)];
                    if v == -1 {
                        start = c + 1;
                    } else {
                        // the next corner of the contour: the contour's first one after the
                        // polygon's last corner (the one after it is then not read) or
                        // before a separator
                        let w = if c == m[m[POL_TABLE].add(pol)].nb.wrapping_sub(1) || m[verts.add(c + 1)] == -1 { m[verts.add(start)] } else { m[verts.add(c + 1)] };
                        let mut e = find_edges(m, v, w);
                        if e == 0 {
                            e = create_edge(m, v, w);
                        }
                        add_polygon_to_edge(m, e, pol);
                        let edges = m[m[POL_TABLE].add(pol)].edges;
                        m[edges.add(c)] = e;
                    }
                    c += 1;
                    if !(c < m[m[POL_TABLE].add(pol)].nb) {
                        break;
                    }
                }
            }
            pol = next;
            if !(next <= m[NB_POL]) {
                break;
            }
        }
    }
}

/// Turns polygon `pol` round [soft 0x084785e0 inversePolygon]: each contour's corners and
/// edges reversed (with the original's index for the mirror, which is right for the first
/// contour only), then every edge's sign changed in the corner and in the edge's list.
fn inverse_polygon(m: &mut Mem, pol: i32) {
    let pl = m[POL_TABLE].add(pol);
    let mut start: i32 = 0;
    let mut nb = m[pl].nb as u32;
    let mut i: u32 = 0;
    if nb < 0x8000_0000 {
        loop {
            if i == nb || m[m[pl].verts.add(i as i32)] == -1 {
                let mut k = start;
                while k < (i as i32).wrapping_sub(start) / 2 + start {
                    let j = (i as i32).wrapping_sub(k);
                    let verts = m[pl].verts;
                    let t = m[verts.add(j - 1)];
                    m[verts.add(j - 1)] = m[verts.add(k)];
                    let verts = m[pl].verts;
                    m[verts.add(k)] = t;
                    let edges = m[pl].edges;
                    let t = m[edges.add(j - 1)];
                    m[edges.add(j - 1)] = m[edges.add(k)];
                    let edges = m[pl].edges;
                    m[edges.add(k)] = t;
                    k += 1;
                }
                if i != m[pl].nb as u32 && m[m[pl].verts.add(i as i32)] == -1 {
                    start = i.wrapping_add(1) as i32;
                }
            }
            i = i.wrapping_add(1);
            nb = m[pl].nb as u32;
            if !(i as i32 <= nb as i32) {
                break;
            }
        }
    }
    let mut c = 0;
    if 0 < m[pl].nb {
        loop {
            let next = c + 1;
            if m[m[pl].verts.add(c)] != -1 {
                let ep = m[pl].edges.add(c);
                m[ep] = m[ep].wrapping_neg();
                let ed = m[EDG_TABLE].add(abs(m[m[pl].edges.add(c)]));
                let mut k = 0;
                while k < m[ed].nb {
                    let pols = m[ed].pols;
                    if abs(m[pols.add(k)]) == pol {
                        m[pols.add(k)] = m[pols.add(k)].wrapping_neg();
                        break;
                    }
                    k += 1;
                }
            }
            c = next;
            if !(next < m[pl].nb) {
                break;
            }
        }
    }
}

/// [soft 0x084787cc addToDoList]: one more entry (edge, old vertex, new vertex, polygon).
/// The list grows by 10 entries.
fn add_to_do_list(m: &mut Mem, a: i32, b: i32, c: i32, d: i32) {
    let nb = m[NB_TO_DO];
    if nb == (nb / 10).wrapping_mul(10) {
        let old = m[TO_DO_LIST];
        let new: Ptr32<[i32; 4]> = p(if old.is_null() { m.malloc(0xa0) } else { m.realloc(old.addr(), nb.wrapping_add(10).wrapping_mul(0x10) as u32) });
        m[TO_DO_LIST] = new;
        // from the entry after the one about to be written
        for i in nb.wrapping_add(1)..nb.wrapping_add(10) {
            m[new.add(i)] = [-1; 4];
        }
    }
    let (t, nb) = (m[TO_DO_LIST], m[NB_TO_DO]);
    m[t.add(nb)] = [a, b, c, d];
    m[NB_TO_DO] = nb.wrapping_add(1);
}

/// [soft 0x08478720 fixEdge]: the edge's end at `old` becomes `new`, and so does that
/// corner of every polygon along it but `skip`. Nothing when the edge has no end at `old`.
fn fix_edge(m: &mut Mem, e: i32, old: i32, new: i32, skip: i32) {
    let ed = m[EDG_TABLE].add(abs(e));
    if m[ed].v0 == old {
        m[ed].v0 = new;
    } else {
        if m[ed].v1 != old {
            return;
        }
        m[ed].v1 = new;
    }
    let mut k = 0;
    if 0 < m[ed].nb {
        loop {
            let q = abs(m[m[ed].pols.add(k)]);
            let next = k + 1;
            if q != abs(skip) {
                let pl = m[POL_TABLE].add(abs(m[m[ed].pols.add(k)]));
                let mut c = 0;
                if 0 < m[pl].nb {
                    let verts = m[pl].verts;
                    if m[verts] == old {
                        m[verts] = new;
                    } else {
                        loop {
                            c += 1;
                            if m[pl].nb <= c {
                                break;
                            }
                            if m[verts.add(c)] == old {
                                m[verts.add(c)] = new;
                                break;
                            }
                        }
                    }
                }
            }
            k = next;
            if !(next < m[ed].nb) {
                break;
            }
        }
    }
}

/// [soft 0x084788a4 fixPolygon]: in polygon `pol` the corner at `v3` becomes `n5` with `e`
/// as its edge and the corner at `v4` becomes `n6`; the edges met on the way are noted for
/// [`fix_edge`]. Ends after two notes.
fn fix_polygon(m: &mut Mem, pol: i32, e: i32, v3: i32, v4: i32, n5: i32, n6: i32) {
    let pl = m[POL_TABLE].add(pol);
    let mut noted = 0;
    let mut before = -1;
    let mut found = false;
    let mut c = 0;
    if 0 < m[pl].nb {
        loop {
            let verts = m[pl].verts;
            let mut v = m[verts.add(c)];
            let mut edge = before;
            let mut to = n5;
            let note;
            if v == -1 {
                v = v3;
                if found {
                    note = true;
                } else {
                    before = -1;
                    note = false;
                }
            } else if v == v3 {
                m[verts.add(c)] = n5;
                let edges = m[pl].edges;
                m[edges.add(c)] = e;
                if before == -1 {
                    found = true;
                    note = false;
                } else {
                    note = true;
                }
            } else if v == v4 {
                m[verts.add(c)] = n6;
                edge = m[m[pl].edges.add(c)];
                to = n6;
                note = true;
            } else {
                if found && c == m[pl].nb.wrapping_sub(1) {
                    let x = m[m[pl].edges.add(c)];
                    add_to_do_list(m, x, v3, n5, pol);
                } else {
                    before = m[m[pl].edges.add(c)];
                }
                note = false;
            }
            if note {
                add_to_do_list(m, edge, v, to, pol);
                noted += 1;
                if noted == 2 {
                    return;
                }
            }
            c += 1;
            if !(c < m[pl].nb) {
                break;
            }
        }
    }
}

/// The polygon across edge `e` from `pol`, or 0 [soft 0x08478c00 associatePolygon]. An
/// edge with more than two polygons (or with two of which none can be the partner) is
/// split on the way: the other polygons get an edge and two vertices of their own.
///
/// `ed` is the original's pointer into the edge table. `create_edge` may move the table;
/// the original goes on reading through the old pointer until it computes it again, and so
/// does this (the old block stays readable in the image, as it does in the emulator).
fn associate_polygon(m: &mut Mem, e: i32, pol: i32) -> i32 {
    let mut pol = pol;
    let mut ed = m[EDG_TABLE].add(abs(e));
    let n = m[ed].nb;
    let mut partner = 0;
    if !(n < 2) {
        if e < 1 {
            pol = pol.wrapping_neg();
        }
        let mut i = 0;
        let mut cur = partner;
        if 0 < n {
            let pols = m[ed].pols;
            cur = m[pols];
            loop {
                if cur != pol {
                    if (0 < cur) != (0 < e) {
                        break;
                    }
                    let q = abs(m[pols.add(i)]);
                    if m[m[POL_TABLE].add(q)].done == 0 {
                        partner = m[pols.add(i)];
                    }
                }
                i += 1;
                cur = partner;
                if n <= i {
                    break;
                }
                cur = m[pols.add(i)];
            }
        }
        partner = cur;
        if 2 < n || partner == 0 {
            let v = m[m[VRT_TABLE].add(m[ed].v0)];
            let a = create_vertex(m, v.pos, v.flag);
            let v = m[m[VRT_TABLE].add(m[ed].v1)];
            let b = create_vertex(m, v.pos, v.flag);
            let ne = create_edge(m, a, b);
            m[NB_TO_DO] = 0;
            m[TO_DO_LIST] = Ptr32::NULL;
            let mut i = 0;
            if 0 < m[ed].nb {
                loop {
                    let q = m[m[ed].pols.add(i)];
                    if q != pol && q != partner {
                        let se = if q < 1 { ne.wrapping_neg() } else { ne };
                        add_polygon_to_edge(m, ne, q);
                        ed = m[EDG_TABLE].add(abs(e));
                        let (v3, v4, n5, n6) = if se < 1 { (m[ed].v1, m[ed].v0, b, a) } else { (m[ed].v0, m[ed].v1, a, b) };
                        let r = abs(m[m[ed].pols.add(i)]);
                        fix_polygon(m, r, se, v3, v4, n5, n6);
                    }
                    i += 1;
                    if !(i < m[ed].nb) {
                        break;
                    }
                }
            }
            let mut todo = m[TO_DO_LIST];
            let mut t = 0;
            if 0 < m[NB_TO_DO] {
                loop {
                    ed = m[EDG_TABLE].add(abs(m[todo][0]));
                    t += 1;
                    let mut k = 0;
                    if 0 < m[ed].nb {
                        loop {
                            if abs(m[m[ed].pols.add(k)]) != abs(partner) {
                                k += 1;
                                if m[ed].nb <= k {
                                    break;
                                }
                                continue;
                            }
                            // the partner lies along this edge too: it gets a new edge, and
                            // the others are put back on the old one through the scratch list
                            let (v0, v1) = (m[ed].v0, m[ed].v1);
                            let e9 = create_edge(m, v0, v1);
                            let e10 = m[EDG_TABLE].add(abs(m[todo][0]));
                            let q = m[m[e10].pols.add(k)];
                            add_polygon_to_edge(m, e9, q);
                            let pl = m[POL_TABLE].add(abs(m[m[e10].pols.add(k)]));
                            let mut c = 0;
                            if 0 < m[pl].nb {
                                loop {
                                    if m[m[pl].verts.add(c)] != -1 && abs(m[m[pl].edges.add(c)]) == abs(m[todo][0]) {
                                        let s = if m[m[e10].pols.add(k)] < 1 { e9.wrapping_neg() } else { e9 };
                                        let edges = m[pl].edges;
                                        m[edges.add(c)] = s;
                                    }
                                    c += 1;
                                    if !(c < m[pl].nb) {
                                        break;
                                    }
                                }
                            }
                            let mut j = 0;
                            if 0 < m[e10].nb {
                                loop {
                                    if abs(m[m[e10].pols.add(j)]) != abs(partner) {
                                        let q = m[m[e10].pols.add(j)];
                                        add_polygon_to_edge(m, 0, q);
                                    }
                                    j += 1;
                                    if !(j < m[e10].nb) {
                                        break;
                                    }
                                }
                            }
                            let old = m[EDG_TABLE].add(abs(m[todo][0]));
                            let block = m[old].pols.addr();
                            m.free(block);
                            let mut table = m[EDG_TABLE];
                            m[old].nb = 0;
                            m[old].pols = Ptr32::NULL;
                            let mut j = 0;
                            if 0 < m[table].nb {
                                loop {
                                    let x = abs(m[todo][0]);
                                    let q = m[m[table].pols.add(j)];
                                    add_polygon_to_edge(m, x, q);
                                    j += 1;
                                    table = m[EDG_TABLE];
                                    if !(j < m[table].nb) {
                                        break;
                                    }
                                }
                            }
                            let block = m[m[EDG_TABLE]].pols.addr();
                            m.free(block);
                            let table = m[EDG_TABLE];
                            m[table].nb = 0;
                            m[table].pols = Ptr32::NULL;
                            break;
                        }
                    }
                    todo = todo.add(1);
                    if !(t < m[NB_TO_DO]) {
                        break;
                    }
                }
            }
            let mut todo = m[TO_DO_LIST];
            let mut t = 0;
            if 0 < m[NB_TO_DO] {
                loop {
                    let [a0, a1, a2, a3] = m[todo];
                    fix_edge(m, a0, a1, a2, a3);
                    t += 1;
                    todo = todo.add(1);
                    if !(t < m[NB_TO_DO]) {
                        break;
                    }
                }
            }
            let block = m[TO_DO_LIST].addr();
            m.free(block);
            m[TO_DO_LIST] = Ptr32::NULL;
            m[NB_TO_DO] = 0;
            let own = m[EDG_TABLE].add(abs(e));
            let block = m[own].pols.addr();
            m.free(block);
            m[own].nb = 0;
            m[own].pols = Ptr32::NULL;
            add_polygon_to_edge(m, abs(e), pol);
            if partner != 0 {
                add_polygon_to_edge(m, abs(e), partner);
            }
        }
    }
    partner
}

/// [soft 0x084790ac processPolygon]: marks `pol` visited and goes on, depth first, to the
/// partner across each of its edges, turning the partner round first when it runs along
/// the shared edge the same way.
fn process_polygon(m: &mut Mem, pol: i32) {
    let pl = m[POL_TABLE].add(pol);
    m[pl].done = 1;
    let mut c = 0;
    if 0 < m[pl].nb {
        loop {
            if m[m[pl].verts.add(c)] != -1 {
                let edge = m[m[pl].edges.add(c)];
                let r = associate_polygon(m, edge, pol);
                if r != 0 && m[m[POL_TABLE].add(abs(r))].done == 0 {
                    if (0 < r) != (m[m[pl].edges.add(c)] < 1) {
                        inverse_polygon(m, abs(r));
                    }
                    process_polygon(m, abs(r));
                }
            }
            c += 1;
            if !(c < m[pl].nb) {
                break;
            }
        }
    }
}

/// Puts a polygon on the stack [soft 0x084792d4 addPolygonToStack]; it grows by 10.
fn add_polygon_to_stack(m: &mut Mem, pol: i32) {
    let nb = m[NB_POL_STACK];
    if nb == (nb / 10).wrapping_mul(10) {
        let old = m[POL_STACK];
        let new: Ptr32<i32> = p(if old.is_null() { m.malloc(0x28) } else { m.realloc(old.addr(), (nb.wrapping_mul(4).wrapping_add(0x28)) as u32) });
        m[POL_STACK] = new;
        for i in nb..nb.wrapping_add(10) {
            m[new.add(i)] = -1;
        }
    }
    let (s, nb) = (m[POL_STACK], m[NB_POL_STACK]);
    m[s.add(nb)] = pol;
    m[NB_POL_STACK] = nb.wrapping_add(1);
}

/// Empties the stack [soft 0x0847936c clearPolygonStack].
fn clear_polygon_stack(m: &mut Mem) {
    if !m[POL_STACK].is_null() {
        let block = m[POL_STACK].addr();
        m.free(block);
        m[POL_STACK] = Ptr32::NULL;
    }
    m[NB_POL_STACK] = 0;
}

/// Whether the stack is empty [soft 0x084793e8 polygonStackEmpty].
fn polygon_stack_empty(m: &Mem) -> bool {
    m[NB_POL_STACK] == 0
}

/// [soft 0x0847939c processPolygonStack]: in every polygon of the stack the first corner
/// at vertex `old` becomes `new`.
fn process_polygon_stack(m: &mut Mem, old: i32, new: i32) {
    let mut i = 0;
    loop {
        if m[NB_POL_STACK] <= i {
            return;
        }
        let at = i;
        i += 1;
        let mut c = 0;
        loop {
            let pl = m[POL_TABLE].add(m[m[POL_STACK].add(at)]);
            if !(c < m[pl].nb) {
                break;
            }
            let verts = m[pl].verts;
            if m[verts.add(c)] == old {
                m[verts.add(c)] = new;
                break;
            }
            c += 1;
        }
    }
}

/// [soft 0x084793fc addVrtPair]: at vertex `v`, polygon `a` is followed by polygon `b`
/// (0 stands for a border). The list grows by 4 pairs.
fn add_vrt_pair(m: &mut Mem, v: i32, a: i32, b: i32) {
    let vr = m[VRT_TABLE].add(v);
    if m[vr].nb as u32 & 3 == 0 {
        let old = m[vr].pairs;
        let new = if old.is_null() { m.malloc(0x20) } else { m.realloc(old.addr(), (m[vr].nb as u32).wrapping_mul(8).wrapping_add(0x20)) };
        m[vr].pairs = p(new);
        let mut i = m[vr].nb;
        if i < i.wrapping_add(4) {
            loop {
                let pairs = m[vr].pairs;
                m[pairs.add(i)] = [-1, -1];
                i += 1;
                if !(i < m[vr].nb.wrapping_add(4)) {
                    break;
                }
            }
        }
    }
    let (pairs, nb) = (m[vr].pairs, m[vr].nb);
    m[pairs.add(nb)] = [a, b];
    m[vr].nb = nb.wrapping_add(1);
}

/// A new, empty mesh [soft 0x08458c20 SI_meshAllocate] in a block of 0x54 bytes from
/// `SI_malloc`: nothing is cleared, so the bytes at +0x22, +0x3a and +0x4e (two each) hold
/// whatever the heap held. The cosine of the constant at 0x0865faf8 is asked of the C
/// library (`si_x87::trig`, replayed from a vector's record when there is one) and stored
/// as a float.
pub(crate) fn mesh_allocate(m: &mut Mem) -> Ptr32<Mesh> {
    let mesh: Ptr32<Mesh> = p(m.si_malloc(0x54));
    if !mesh.is_null() {
        m[mesh].angle = f32::from_bits(0x4270_0000);
        m[mesh].nb_vertices = 0;
        m[mesh].vertices = Ptr32::NULL;
        m[mesh].nb_polygons = 0;
        m[mesh].polygons = Ptr32::NULL;
        m[mesh].nb_edges = 0;
        m[mesh].edges = Ptr32::NULL;
        m[mesh].cos_angle = si_x87::trig::cos(f64::from_bits(0x3ff0_c152_37db_38a0)).to_f32();
        m[mesh].mr_edge = f32::from_bits(0x40a0_0000);
        m[mesh].mr_ucurv = f32::from_bits(0x3dcc_cccd);
        m[mesh].mr_vcurv = f32::from_bits(0x4234_0000);
        m[mesh].flags = 0x804;
        m[mesh].nb_display = 0;
        m[mesh].display = Ptr32::NULL;
        m[mesh].unk_2c = 0;
        m[mesh].polygon_list0 = Ptr32::NULL;
        m[mesh].polygon_list1 = Ptr32::NULL;
        m[mesh].mr_first = 0;
        m[mesh].mr_approx = 2;
        m[mesh].mr_view = 0;
        m[mesh].mr_min = 1;
        m[mesh].mr_max = 5;
        m[mesh].mr_rec = 0;
    }
    mesh
}

/// The count the two allocators below ask for: `n` rounded up to a multiple of 32.
fn round32(n: i32) -> u32 {
    (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 0x1f).wrapping_sub(0x1f))
}

/// `n` vertices [soft 0x08459d48 SI_vertexAllocate]: room for a multiple of 32, cleared,
/// and the first `n` set to zero field by field.
fn vertex_allocate(m: &mut Mem, n: i32) -> Ptr32<Vertex> {
    let first: Ptr32<Vertex> = p(m.si_calloc(round32(n), 0x14));
    let mut v = first;
    let mut left = n;
    while left != 0 {
        m.set_fill_bytes(v.addr(), 0, 0x14);
        v = v.add(1);
        left = left.wrapping_sub(1);
    }
    first
}

/// `n` polygons [soft 0x08459e20 SI_polygonAllocate], allocated like the vertices. The two
/// bytes at +0x16 of each are not written (they are zero from the allocation).
fn polygon_allocate(m: &mut Mem, n: i32) -> Ptr32<Polygon> {
    let first: Ptr32<Polygon> = p(m.si_calloc(round32(n), 0x18));
    let mut q = first;
    let mut left = n;
    while left != 0 {
        m.set_fill_bytes(q.addr(), 0, 0x16);
        q = q.add(1);
        left = left.wrapping_sub(1);
    }
    first
}

/// A copy of a mesh with its polygons turned one consistent way and its vertices doubled
/// where the surface is not a manifold [soft 0x08479494 SI_orderMesh]: see
/// [`crate::order::order_mesh`] for what the six steps do. This form answers the address
/// of the new mesh, a block of the heap; the tables it worked on are freed (all but each
/// polygon's three arrays of texture coordinates and colours, which the original loses)
/// and the file's ten variables are back at zero.
pub fn order_mesh(m: &mut Mem, mesh: Ptr32<Mesh>) -> Ptr32<Mesh> {
    m[NB_VRT] = 0;
    m[VRT_TABLE] = Ptr32::NULL;
    m[NB_EDG] = 0;
    m[EDG_TABLE] = Ptr32::NULL;
    m[POL_TABLE] = Ptr32::NULL;
    m[NB_TO_DO] = 0;
    m[TO_DO_LIST] = Ptr32::NULL;
    m[NB_POL_STACK] = 0;
    m[POL_STACK] = Ptr32::NULL;
    m[NB_POL] = m[mesh].nb_polygons;
    let count = m[NB_POL].wrapping_add(1) as u32;
    m[POL_TABLE] = p(m.si_calloc(count, 0x24));

    // 1. the polygons and the vertices into the tables
    let mut k = 0;
    let mut src = m[mesh].polygons;
    let mut at = m[POL_TABLE];
    if 0 < m[NB_POL] {
        loop {
            let e = at.add(1);
            m[e].done = 0;
            let n = m[src].nb_nodes;
            m[e].nb = n as i32;
            m[e].verts = p(m.malloc((n as u32) << 2));
            for which in 0..4 {
                let size = (m[e].nb as u32) << 2;
                let block = m.malloc(size);
                match which {
                    0 => m[e].edges = p(block),
                    1 => m[e].u = p(block),
                    2 => m[e].v = p(block),
                    _ => m[e].color = p(block),
                }
            }
            let size = m[e].nb as u32;
            m[e].extra = p(m.si_calloc(size, 4));
            m[e].material = m[src].material as i32;
            let mut c = 0;
            k += 1;
            if m[src].nb_nodes != 0 {
                loop {
                    let node: Ptr32<PolyNode> = m[src].nodes.add(c);
                    let (verts, u, v, color) = (m[e].verts, m[e].u, m[e].v, m[e].color);
                    m[verts.add(c)] = m[node].vertex;
                    m[u.add(c)] = m.u32(node.addr() + 0x14);
                    m[v.add(c)] = m.u32(node.addr() + 0x18);
                    m[color.add(c)] = m.u32(node.addr() + 0x1c);
                    if !m[node].extra_uv.is_null() {
                        let block: Ptr32<[u32; 14]> = p(m.si_calloc(0xe, 4));
                        let extra = m[e].extra;
                        m[extra.add(c)] = block;
                        let from: Ptr32<[u32; 14]> = m[m[src].nodes.add(c)].extra_uv.cast();
                        let to = m[m[e].extra.add(c)];
                        for w in 0..14 {
                            m[to.elem(w)] = m[from.elem(w)];
                        }
                    }
                    c += 1;
                    if !(c < m[src].nb_nodes as i32) {
                        break;
                    }
                }
            }
            src = src.add(1);
            at = at.add(1);
            if !(k < m[NB_POL]) {
                break;
            }
        }
    }
    let mut k = 0;
    if 0 < m[mesh].nb_vertices {
        loop {
            let v = m[mesh].vertices.add(k);
            let pos = [m.u32(v.addr()), m.u32(v.addr() + 4), m.u32(v.addr() + 8)];
            let flag = m[v].flag;
            create_vertex(m, pos, flag);
            k += 1;
            if !(k < m[mesh].nb_vertices) {
                break;
            }
        }
    }

    // 2. edges; every piece turned the way of its first polygon
    create_edg_table(m);
    let mut k = 1;
    if 0 < m[NB_POL] {
        loop {
            if m[m[POL_TABLE].add(k)].done == 0 {
                process_polygon(m, k);
            }
            k += 1;
            if !(k <= m[NB_POL]) {
                break;
            }
        }
    }

    // 3. the edges again, from the corners as they now are
    free_edges(m);
    m[NB_EDG] = 0;
    m[EDG_TABLE] = Ptr32::NULL;
    create_edg_table(m);

    // 4. round each vertex: which polygon follows which
    let mut k = 1;
    if 1 < m[NB_EDG] {
        loop {
            let e = m[EDG_TABLE].add(k);
            if m[e].nb == 2 {
                let pols = m[e].pols;
                let (a0, a1) = (m[pols], m[pols.add(1)]);
                if a0 < 1 || -1 < a1 {
                    if !(a1 < 1 || -1 < a0) {
                        let v0 = m[e].v0;
                        add_vrt_pair(m, v0, a1, a0.wrapping_neg());
                        let x = m[m[e].pols.add(1)];
                        let y = m[m[e].pols];
                        let v1 = m[e].v1;
                        add_vrt_pair(m, v1, y.wrapping_neg(), x);
                    }
                } else {
                    let v0 = m[e].v0;
                    add_vrt_pair(m, v0, a0, a1.wrapping_neg());
                    let x = m[m[e].pols];
                    let y = m[m[e].pols.add(1)];
                    let v1 = m[e].v1;
                    add_vrt_pair(m, v1, y.wrapping_neg(), x);
                }
            } else if m[e].nb == 1 {
                let a = m[m[e].pols];
                let v0 = m[e].v0;
                if a < 1 {
                    add_vrt_pair(m, v0, 0, a.wrapping_neg());
                    let y = m[m[e].pols];
                    let v1 = m[e].v1;
                    add_vrt_pair(m, v1, y.wrapping_neg(), 0);
                } else {
                    add_vrt_pair(m, v0, a, 0);
                    let x = m[m[e].pols];
                    let v1 = m[e].v1;
                    add_vrt_pair(m, v1, 0, x);
                }
            }
            k += 1;
            if !(k < m[NB_EDG]) {
                break;
            }
        }
    }

    // 5. one vertex per chain of polygons round it
    m[NB_POL_STACK] = 0;
    m[POL_STACK] = Ptr32::NULL;
    let mut v = 0;
    if !(m[NB_VRT] < 1) {
        loop {
            let mut first = true;
            let next = v + 1;
            let table = loop {
                let mut table = m[VRT_TABLE];
                let (mut from, mut to) = (0, 0);
                let mut i = 0;
                if 0 < m[table.add(v)].nb {
                    loop {
                        let pairs = m[m[VRT_TABLE].add(v)].pairs;
                        let a = m[pairs.add(i)][0];
                        if a != 0 {
                            to = m[pairs.add(i)][1];
                            m[pairs.add(i)][0] = 0;
                            let pairs = m[table.add(v)].pairs;
                            m[pairs.add(i)][1] = 0;
                            from = a;
                            if !first && to != 0 {
                                add_polygon_to_stack(m, to);
                            }
                            break;
                        }
                        i += 1;
                        if !(i < m[m[VRT_TABLE].add(v)].nb) {
                            break;
                        }
                    }
                }
                table = m[VRT_TABLE];
                if from == 0 || to == 0 {
                    break table;
                }
                let mut i = 0;
                if 0 < m[table.add(v)].nb {
                    loop {
                        let pairs = m[table.add(v)].pairs;
                        if m[pairs.add(i)][0] == to {
                            if !first {
                                let x = m[pairs.add(i)][1];
                                if x != 0 {
                                    add_polygon_to_stack(m, x);
                                }
                            }
                            table = m[VRT_TABLE];
                            let pairs = m[table.add(v)].pairs;
                            to = m[pairs.add(i)][1];
                            if from == to {
                                if !first {
                                    let vr = m[table.add(v)];
                                    let nv = create_vertex(m, vr.pos, vr.flag);
                                    process_polygon_stack(m, v, nv);
                                    clear_polygon_stack(m);
                                }
                                table = m[VRT_TABLE];
                                let pairs = m[table.add(v)].pairs;
                                m[pairs.add(i)][0] = 0;
                                m[pairs.add(i)][1] = 0;
                                break;
                            }
                            m[pairs.add(i)][0] = 0;
                            let pairs = m[table.add(v)].pairs;
                            m[pairs.add(i)][1] = 0;
                            if to == 0 {
                                break;
                            }
                            i = -1;
                        }
                        i += 1;
                        if !(i < m[table.add(v)].nb) {
                            break;
                        }
                    }
                }
                first = false;
                if !polygon_stack_empty(m) {
                    let vr = m[m[VRT_TABLE].add(v)];
                    let nv = create_vertex(m, vr.pos, vr.flag);
                    add_polygon_to_stack(m, from);
                    process_polygon_stack(m, v, nv);
                    clear_polygon_stack(m);
                }
            };
            let now = m[VRT_TABLE].add(v);
            m[now].nb = 0;
            let pairs = m[table.add(v)].pairs;
            if !pairs.is_null() {
                m.free(pairs.addr());
                let now = m[VRT_TABLE].add(v);
                m[now].pairs = Ptr32::NULL;
            }
            clear_polygon_stack(m);
            v = next;
            if m[NB_VRT] <= next {
                break;
            }
        }
    }

    // 6. the new mesh
    let out = mesh_allocate(m);
    let nv = m[NB_VRT];
    m[out].nb_vertices = nv;
    m[out].vertices = vertex_allocate(m, nv);
    let table = m[VRT_TABLE];
    for i in 0..m[NB_VRT].max(0) {
        let dst = m[out].vertices.add(i);
        let src = m[table.add(i)];
        for w in 0..3 {
            m.set_u32(dst.addr() + 4 * w, src.pos[w as usize]);
        }
        let dst = m[out].vertices.add(i);
        m[dst].flag = src.flag;
    }
    let np = m[NB_POL];
    m[out].nb_polygons = np;
    let mut dst = polygon_allocate(m, np);
    let mut at = m[POL_TABLE];
    m[out].polygons = dst;
    let mut k = 0;
    if 0 < m[NB_POL] {
        loop {
            let e = at.add(1);
            m[dst].nb_nodes = m[e].nb as u16;
            let n = m[dst].nb_nodes as u32;
            let room = n.wrapping_sub((n.wrapping_sub(1) & 3).wrapping_sub(3));
            m[dst].nodes = p(m.si_calloc(room, 0x24));
            m[dst].material = m[e].material as i16;
            let mut c = 0;
            k += 1;
            if m[dst].nb_nodes != 0 {
                loop {
                    let node = m[dst].nodes.add(c);
                    m[node].vertex = m[m[e].verts.add(c)];
                    let w = m[m[e].u.add(c)];
                    m.set_u32(m[dst].nodes.add(c).addr() + 0x14, w);
                    let w = m[m[e].v.add(c)];
                    m.set_u32(m[dst].nodes.add(c).addr() + 0x18, w);
                    let w = m[m[e].color.add(c)];
                    m.set_u32(m[dst].nodes.add(c).addr() + 0x1c, w);
                    if !m[m[e].extra.add(c)].is_null() {
                        let block = m.si_calloc(0xe, 4);
                        let node = m[dst].nodes.add(c);
                        m[node].extra_uv = p(block);
                        let from = m[m[e].extra.add(c)];
                        let to: Ptr32<[u32; 14]> = m[m[dst].nodes.add(c)].extra_uv.cast();
                        for w in 0..14 {
                            m[to.elem(w)] = m[from.elem(w)];
                        }
                    }
                    c += 1;
                    if !(c < m[dst].nb_nodes as i32) {
                        break;
                    }
                }
            }
            dst = dst.add(1);
            at = at.add(1);
            if !(k < m[NB_POL]) {
                break;
            }
        }
    }

    // the tables go
    let block = m[VRT_TABLE].addr();
    m.free(block);
    m[VRT_TABLE] = Ptr32::NULL;
    m[NB_VRT] = 0;
    free_edges(m);
    m[EDG_TABLE] = Ptr32::NULL;
    m[NB_EDG] = 0;
    let mut k = 1;
    if 0 < m[NB_POL] {
        loop {
            let e = m[POL_TABLE].add(k);
            if !m[e].verts.is_null() {
                let block = m[e].verts.addr();
                m.free(block);
                m[e].verts = Ptr32::NULL;
            }
            if !m[e].edges.is_null() {
                let block = m[e].edges.addr();
                m.free(block);
                m[e].edges = Ptr32::NULL;
            }
            let next = k + 1;
            if !m[e].extra.is_null() {
                let mut i = 0;
                if 0 < m[e].nb {
                    loop {
                        let slot = m[e].extra.add(i);
                        if !m[slot].is_null() {
                            let block = m[slot].addr();
                            m.free(block);
                            let slot = m[e].extra.add(i);
                            m[slot] = Ptr32::NULL;
                        }
                        i += 1;
                        if !(i < m[e].nb) {
                            break;
                        }
                    }
                }
                if !m[e].extra.is_null() {
                    let block = m[e].extra.addr();
                    m.free(block);
                    m[e].extra = Ptr32::NULL;
                }
                m[e].extra = Ptr32::NULL;
            }
            k = next;
            if !(next <= m[NB_POL]) {
                break;
            }
        }
    }
    let block = m[POL_TABLE].addr();
    m.free(block);
    m[POL_TABLE] = Ptr32::NULL;
    m[NB_POL] = 0;
    if !m[TO_DO_LIST].is_null() {
        let block = m[TO_DO_LIST].addr();
        m.free(block);
        m[TO_DO_LIST] = Ptr32::NULL;
    }
    m[NB_TO_DO] = 0;
    if !m[POL_STACK].is_null() {
        let block = m[POL_STACK].addr();
        m.free(block);
        m[POL_STACK] = Ptr32::NULL;
    }
    m[NB_POL_STACK] = 0;
    out
}

/// Every edge's list of polygons freed, then the table (part of
/// [soft 0x08479494 SI_orderMesh], which does this twice).
fn free_edges(m: &mut Mem) {
    let mut k = 1;
    if 1 < m[NB_EDG] {
        loop {
            let e = m[EDG_TABLE].add(k);
            if !m[e].pols.is_null() {
                let block = m[e].pols.addr();
                m.free(block);
                let e = m[EDG_TABLE].add(k);
                m[e].pols = Ptr32::NULL;
            }
            k += 1;
            if !(k < m[NB_EDG]) {
                break;
            }
        }
    }
    let block = m[EDG_TABLE].addr();
    m.free(block);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mesh in the arena: vertices at the given positions, polygons by vertex numbers.
    fn mesh(m: &mut Mem, positions: &[[f32; 3]], polygons: &[&[i32]]) -> Ptr32<Mesh> {
        // cleared buffers: a test sets only the fields the routine reads
        fn zeroed(m: &mut Mem, n: u32) -> u32 {
            let a = m.place(n, 8);
            m.set_fill_bytes(a, 0, n);
            a
        }
        let verts: Ptr32<Vertex> = p(zeroed(m, 0x14 * positions.len() as u32));
        for (i, pos) in positions.iter().enumerate() {
            m[verts.add(i as i32)].pos = si_types::geom::Vector { x: pos[0], y: pos[1], z: pos[2] };
        }
        let polys: Ptr32<Polygon> = p(zeroed(m, 0x18 * polygons.len() as u32));
        for (i, corners) in polygons.iter().enumerate() {
            let nodes: Ptr32<PolyNode> = p(zeroed(m, 0x24 * corners.len() as u32));
            for (c, v) in corners.iter().enumerate() {
                m[nodes.add(c as i32)].vertex = *v;
            }
            m[polys.add(i as i32)].nodes = nodes;
            m[polys.add(i as i32)].nb_nodes = corners.len() as u16;
        }
        let mesh: Ptr32<Mesh> = p(zeroed(m, 0x54));
        m[mesh].vertices = verts;
        m[mesh].polygons = polys;
        m[mesh].nb_vertices = positions.len() as i32;
        m[mesh].nb_polygons = polygons.len() as i32;
        mesh
    }

    fn corners(m: &Mem, mesh: Ptr32<Mesh>) -> Vec<Vec<i32>> {
        (0..m[mesh].nb_polygons)
            .map(|i| {
                let q = m[mesh].polygons.add(i);
                (0..m[q].nb_nodes as i32).map(|c| m[m[q].nodes.add(c)].vertex).collect()
            })
            .collect()
    }

    const SQUARE: [[f32; 3]; 4] = [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];

    #[test]
    fn the_same_answers_as_the_form_on_rust_structures() {
        // the three cases of `crate::order`'s tests
        let mut m = Mem::new();
        let a = mesh(&mut m, &SQUARE, &[&[0, 1, 2], &[0, 2, 3]]);
        let out = order_mesh(&mut m, a);
        assert_eq!(corners(&m, out), [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(m[out].nb_vertices, 4);
        let b = mesh(&mut m, &SQUARE, &[&[0, 1, 2], &[0, 3, 2]]);
        let out = order_mesh(&mut m, b);
        assert_eq!(corners(&m, out), [[0, 1, 2], [2, 3, 0]]);
        let c = mesh(&mut m, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]], &[&[0, 1, 2], &[1, 0, 3], &[1, 0, 4]]);
        let out = order_mesh(&mut m, c);
        assert_eq!(m[out].nb_vertices, 7);
        let v5 = m[m[out].vertices.add(5)].pos;
        let v6 = m[m[out].vertices.add(6)].pos;
        assert_eq!(([v5.x, v5.y, v5.z], [v6.x, v6.y, v6.z]), ([0.0; 3], [1.0, 0.0, 0.0]));
        assert!(m.fault().is_none());
    }

    #[test]
    fn what_the_original_leaves_in_memory() {
        let mut m = Mem::new();
        m.set_fill(si_mem::Fill::Pattern { second: false, salt: 7 });
        let a = mesh(&mut m, &SQUARE, &[&[0, 1, 2], &[0, 2, 3]]);
        let before = m.heap_blocks().len();
        let out = order_mesh(&mut m, a);
        assert!(m.fault().is_none());
        // the new mesh is a block of 0x54 bytes whose three gaps nobody wrote
        assert_eq!(m.block_size(out.addr()), Some(0x54));
        assert_eq!(m.u8(out.addr() + 0x22), m.fill().byte(out.addr() + 0x22));
        assert_eq!((m[out].flags, m[out].angle, m[out].cos_angle), (0x804, 60.0, 0.5));
        // the file's variables are back at zero
        for g in ALL {
            assert_eq!(m.u32(g.addr), 0, "{}", g.name);
        }
        // live afterwards: the mesh, its two arrays, the corners of each polygon, and the
        // three arrays per polygon the original never frees
        let live: Vec<u32> = m.heap_blocks()[before..].iter().filter(|b| !m.is_freed(b.0)).map(|b| b.1).collect();
        assert_eq!(live.iter().filter(|&&s| s == 12).count(), 6);
        assert_eq!(live.len(), 6 + 5);
        // the first polygon's tables came first: polTable, then its six arrays
        assert_eq!(m.heap_blocks()[before].1, 3 * 0x24);
    }
}
