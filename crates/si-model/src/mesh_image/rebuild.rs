// SPDX-License-Identifier: MIT
//! `mesh_b.c` on the image: a mesh's connectivity and normals rebuilt (finding 0015, "The
//! rebuild"). The value form is [`crate::mesh_b`]; the reading and the emulation of the
//! first pass apply to both, and the floating point of the three normal passes is the value
//! form's, computed here on the image's floats (each routine says so).
//!
//! What only this form has: the star arrays, the edge table and the display lines are the
//! original's heap blocks, grown by its calls (`SI_calloc(4, 12)` for a new star, `SI_realloc`
//! by four entries, a new block of the rounded count for the rewrite of a star; the edge
//! table one pointer and one block of 1024 records at a time), so a later block's address
//! and every byte the original leaves in a block beyond a count are the original's.

use si_mem::Mem;

use super::{callee, edge_record, f3, g, off as mo, polygon_at, put_f3, vertex_at};
use crate::model_image::Outside;

/// Bits of an edge's flag.
const EDGE_USER_HARD: u16 = 0x08;
const EDGE_AUTO_HARD: u16 = 0x10;
const EDGE_BORDER: u16 = 0x20;
const EDGE_DISPLAYED: u16 = 0x8000;

/// [soft 0x08458e10 SI_setCurrentModel]: the node whose mesh is being rebuilt, for the
/// collision messages.
pub fn set_current_model(m: &mut Mem, node: u32) {
    m[g::CURR_MDL] = node;
}

/// [soft 0x08458e20 findModelFromMesh]: the first node of the current graph list
/// (`SI_grafLists` + 0x24 × the second word of `SI_procData`, its head at +4, the nodes
/// linked through +0x1c) that shows a model (kind 2 or 3, the model at +4) of kind 4 with a
/// name, one of whose definitions (`SI_modelNextDefGet`) is this mesh; else null.
pub fn find_model_from_mesh(m: &mut Mem, mesh: u32) -> u32 {
    use crate::model_image::g as mg;
    let lists = m[mg::SI_GRAF_LISTS];
    let index = m.i32(m[mg::SI_PROC_DATA].addr() + 4);
    let mut node = m.u32(lists.wrapping_add(4).wrapping_add((index as u32).wrapping_mul(0x24)));
    while node != 0 {
        let model = m.u32(node + 4);
        if m.u32(node).wrapping_sub(2) < 2 && model != 0 && m.i32(model + 0x30) == 4 && m.u32(model + 4) != 0 {
            let cursor = m.local(4);
            m.set_u32(cursor, 0);
            loop {
                let d = crate::model_image::touch::next_def_get(m, si_mem::Ptr32::new(model), cursor);
                if d == 0 {
                    break;
                }
                if d == mesh {
                    m.release(4);
                    return model;
                }
            }
            m.release(4);
        }
        node = m.u32(node + 0x1c);
    }
    0
}

/// [soft 0x0845a694 SI_polygonInitialize]: the polygon at `poly` gets `corners.len()`
/// corners (the count stored as 16 bits) in a new array (`SI_polygonNodeAllocate`, rounded
/// up to four corners), each with its vertex.
pub fn polygon_initialize(m: &mut Mem, poly: u32, corners: &[i32]) {
    let n = corners.len() as i32;
    m.set_i16(poly + mo::POLYGON_NB_NODES, n as i16);
    let nodes = si_hrc::image::geom::polygon_node_allocate(m, n);
    m.set_u32(poly + mo::POLYGON_NODES, nodes);
    for (k, &v) in corners.iter().enumerate() {
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        m.set_i32(nodes + mo::NODE * k as u32, v);
    }
}

/// [soft 0x08459bf8 meshInsertEdge]: a new edge (`v0`, `v1`, `p0`, `p1`, 0, 0) after the
/// last; answers the count before the call (one less than the new edge's number). The
/// table is made on the first edge (`SI_calloc(1, 4)` and a zeroed block of 1024 records,
/// so record 0 exists and is zero), and grows by one pointer (`SI_realloc`) and one block
/// whenever the count's low ten bits are all ones. With `SI_gblIndex3` installed and its
/// entry 0 not zero, that entry counts down.
pub fn insert_edge(m: &mut Mem, mesh: u32, v0: i32, v1: i32, p0: i32, p1: i32) -> i32 {
    let nb = m.u32(mesh + mo::NB_EDGES);
    if m.u32(mesh + mo::EDGES) == 0 {
        let table = m.si_calloc(1, 4);
        m.set_u32(mesh + mo::EDGES, table);
        let block = m.si_calloc(0x400, mo::EDGE);
        m.set_u32(m.u32(mesh + mo::EDGES), block);
    } else if !nb & 0x3ff == 0 {
        let table = m.si_realloc(m.u32(mesh + mo::EDGES), ((nb.wrapping_add(1) as i32 >> 10) as u32).wrapping_mul(4).wrapping_add(4));
        m.set_u32(mesh + mo::EDGES, table);
        let block = m.si_calloc(0x400, mo::EDGE);
        let slot = m.u32(mesh + mo::EDGES).wrapping_add(((m.i32(mesh + mo::NB_EDGES).wrapping_add(1) >> 10) as u32).wrapping_mul(4));
        m.set_u32(slot, block);
    }
    let r = edge_record(m, mesh, m.i32(mesh + mo::NB_EDGES).wrapping_add(1));
    m.set_i32(r, v0);
    m.set_i32(r + 4, v1);
    m.set_i32(r + 8, p0);
    m.set_i32(r + 12, p1);
    m.set_u16(r + 0x10, 0);
    m.set_u16(r + mo::EDGE_FLAG, 0);
    let index3 = m[g::SI_GBL_INDEX3];
    if index3 != 0 && m.i32(index3) != 0 {
        m.set_i32(index3, m.i32(index3).wrapping_sub(1));
    }
    let old = m.i32(mesh + mo::NB_EDGES);
    m.set_i32(mesh + mo::NB_EDGES, old.wrapping_add(1));
    old
}

/// One star entry: (neighbour, signed edge, polygon).
fn put_entry(m: &mut Mem, at: u32, vertex: i32, edge: i32, polygon: i32) {
    m.set_i32(at, vertex);
    m.set_i32(at + 4, edge);
    m.set_i32(at + 8, polygon);
}

/// The collision message on the status line, formatted into the original's 120-byte stack
/// buffer, and `SI_statusDisplayMessage(buffer, 2)`.
///
/// Where the Rust differs: the named forms of the original (`"%s" : collision of polygons
/// (…, p%d, p%d, v%d, v%d)`) are given four values after the name for five conversions, so
/// the last number is a word of the stack; here it is the fourth value the unnamed form
/// prints (as in [`crate::mesh_b`]). A text longer than the buffer (a long model name)
/// overruns it in the original; here it is cut to the buffer.
fn collision(m: &mut Mem, o: &mut dyn Outside, mesh: u32, which: &str, named_possible: bool, a: i32, b: i32, c: i32, d: i32) {
    let mut name = None;
    if named_possible {
        let mut node = m[g::CURR_MDL];
        if node == 0 {
            node = find_model_from_mesh(m, mesh);
        }
        if node != 0 && m.u32(node + 4) != 0 {
            name = Some(m.cstr(m.u32(node + 4)));
        }
    }
    let text = match name {
        Some(n) => format!("\"{}\" : collision of polygons ({which}, p{a}, p{b}, v{c}, v{d})", String::from_utf8_lossy(&n)),
        None => format!("collision of polygons ({which}, p{a}, p{b}, e{c}, e{d})"),
    };
    let buf = m.local(120);
    let mut bytes = text.into_bytes();
    bytes.truncate(119);
    m.put_cstr(buf, &bytes);
    o.call(m, &callee::SI_STATUS_DISPLAY_MESSAGE, &[buf, 2]);
    m.release(120);
}

/// [soft 0x08458ea4 meshInsertPolygonAtVertex]: the corner of polygon `p` at vertex `v`
/// (`prev` and `next` the vertices of the corners before and after it) filed in `v`'s star,
/// and the two edges at the corner made or completed; answers the signed edge from `v` to
/// `next`, or 0 when refused (an index outside the vertex array, silently; a collision,
/// with a message). The cases are those of [`crate::mesh_b::insert_polygon_at_vertex`];
/// here with the original's blocks:
///
/// - an empty star becomes `SI_calloc(4, 12)` with one entry (`next` = `prev`) or two;
/// - an insertion of one entry, when the star's count is a multiple of four, first
///   `SI_realloc`s the star to the next multiple of four plus four entries; an append of
///   two, when fewer than two entries are free, to the count less ((count + 1) & 3) plus
///   five; the entries above the place move up from the top;
/// - the rewrite of a star (both neighbours known, the run not in order) is a new zeroed
///   block of the count rounded up to four (`SI_calloc`) filled run by run, the old one
///   freed.
///
/// Where the Rust differs: when `next` and `prev` are the same vertex the rewrite's runs
/// overlap and the original writes up to twice the count into its new block, past its end
/// into the heap; here an entry beyond the block is not written to the image (the one entry
/// the call reads back from there is kept aside, as [`crate::mesh_b`] does).
pub fn insert_polygon_at_vertex(m: &mut Mem, o: &mut dyn Outside, mesh: u32, p: i32, prev: i32, v: i32, next: i32) -> i32 {
    let nv = m.i32(mesh + mo::NB_VERTICES);
    if prev < 0 || nv <= prev || v < 0 || nv <= v || next < 0 || nv <= next {
        return 0;
    }
    let vr = vertex_at(m, mesh, v);
    let star = |m: &Mem| m.u32(vr + mo::VERTEX_STAR);
    let count = |m: &Mem| m.u16(vr + mo::VERTEX_NB_STAR) as i32;
    let entry = |m: &Mem, k: i32| star(m).wrapping_add((k as u32).wrapping_mul(12));
    let mut i_next: i32 = -1;
    let mut i_prev: i32 = -1;
    let cur: i32;
    // the entry the rewrite writes beyond its block: (index, entry)
    let mut beyond: Option<(i32, [i32; 3])> = None;
    if count(m) == 0 {
        if next == prev {
            let s = m.si_calloc(4, 12);
            m.set_u32(vr + mo::VERTEX_STAR, s);
            m.set_u16(vr + mo::VERTEX_NB_STAR, 1);
        } else {
            let s = m.si_calloc(4, 12);
            m.set_u32(vr + mo::VERTEX_STAR, s);
            put_entry(m, s + 12, prev, 0, -1);
            m.set_u16(vr + mo::VERTEX_NB_STAR, 2);
        }
        let s = star(m);
        put_entry(m, s, next, 0, p);
        cur = 0;
    } else {
        let s = star(m);
        let mut u = 0;
        while u < count(m) {
            let w = m.i32(s.wrapping_add((u as u32).wrapping_mul(12)));
            if w == next {
                i_next = u;
                if i_prev != -1 {
                    break;
                }
            }
            if w == prev {
                i_prev = u;
                if i_next != -1 {
                    break;
                }
            }
            u += 1;
        }
        if i_next != -1 && m.i32(s.wrapping_add((i_next as u32) * 12 + 8)) != -1 {
            let other = m.i32(entry(m, i_next) + 8);
            collision(m, o, mesh, "vn", true, p, other, v, next);
            return 0;
        }
        if i_prev != -1 {
            let e = m.i32(s.wrapping_add((i_prev as u32) * 12 + 4)).wrapping_abs();
            let r = edge_record(m, mesh, e);
            if m.i32(r) == v {
                let p1 = m.i32(r + 12);
                if p1 != -1 && p1 != p {
                    collision(m, o, mesh, "vp1", true, p, p1, prev, v);
                    return 0;
                }
            } else {
                let p0 = m.i32(r + 8);
                if p0 != -1 && p0 != p {
                    // the original tests a register that is always zero here: never named
                    collision(m, o, mesh, "vp2", false, p, p0, prev, v);
                    return 0;
                }
            }
        }
        if i_next == -1 {
            if i_prev == -1 {
                let n = count(m) as u32;
                if n.wrapping_neg() & 3 < 2 {
                    let s2 = m.si_realloc(star(m), n.wrapping_sub(((n + 1) & 3).wrapping_sub(5)).wrapping_mul(12));
                    m.set_u32(vr + mo::VERTEX_STAR, s2);
                }
                let n = count(m);
                put_entry(m, entry(m, n), next, 0, p);
                put_entry(m, entry(m, n + 1), prev, 0, -1);
                m.set_u16(vr + mo::VERTEX_NB_STAR, (n + 2) as u16);
                cur = n;
            } else {
                grow_by_one(m, vr);
                let mut u = count(m);
                while i_prev < u {
                    u -= 1;
                    let from = entry(m, u);
                    let words = [m.i32(from), m.i32(from + 4), m.i32(from + 8)];
                    put_entry(m, from + 12, words[0], words[1], words[2]);
                }
                put_entry(m, entry(m, i_prev), next, 0, p);
                cur = i_prev;
                m.set_u16(vr + mo::VERTEX_NB_STAR, (count(m) + 1) as u16);
                i_prev += 1;
            }
        } else if i_prev == -1 {
            grow_by_one(m, vr);
            let mut u = count(m);
            while i_next + 1 < u {
                u -= 1;
                let from = entry(m, u);
                let words = [m.i32(from), m.i32(from + 4), m.i32(from + 8)];
                put_entry(m, from + 12, words[0], words[1], words[2]);
            }
            let at = entry(m, i_next);
            m.set_i32(at + 12, prev);
            m.set_i32(at + 16, 0);
            m.set_i32(at + 20, -1);
            m.set_i32(at + 8, p);
            cur = i_next;
            m.set_u16(vr + mo::VERTEX_NB_STAR, (count(m) + 1) as u16);
        } else {
            // back from the entry before `next`'s to the start of its run
            let mut j = i_next - 1;
            while j > -1 {
                let at = entry(m, j);
                let e = m.i32(at + 4).wrapping_abs();
                if m.i32(at) == prev || m.i32(edge_record(m, mesh, e) + 12) == -1 {
                    break;
                }
                j -= 1;
            }
            if j == -1 {
                j = 0;
            }
            m.set_i32(entry(m, i_next) + 8, p);
            let mut at_cur = i_next;
            if j != i_prev {
                let n = count(m) as u32;
                let new = m.si_calloc(n.wrapping_sub((n.wrapping_sub(1) & 3).wrapping_sub(3)), 12);
                let room = m.block_size(new).unwrap_or(0) / 12;
                let n = n as i32;
                let step = |u: i32| if u != n - 1 { u + 1 } else { 0 };
                let mut k = 0i32;
                let mut copy = |m: &mut Mem, k: &mut i32, u: i32| {
                    let from = m.u32(vr + mo::VERTEX_STAR).wrapping_add((u as u32).wrapping_mul(12));
                    let words = [m.i32(from), m.i32(from + 4), m.i32(from + 8)];
                    if (*k as u32) < room {
                        put_entry(m, new + 12 * *k as u32, words[0], words[1], words[2]);
                    } else if beyond.is_none() {
                        beyond = Some((*k, words));
                    }
                    *k += 1;
                };
                if j != i_next {
                    let mut u = j;
                    loop {
                        copy(m, &mut k, u);
                        u = step(u);
                        if u == i_next {
                            break;
                        }
                    }
                }
                at_cur = k;
                copy(m, &mut k, i_next);
                if i_prev != j {
                    let mut u = i_prev;
                    loop {
                        copy(m, &mut k, u);
                        u = step(u);
                        if u == j {
                            break;
                        }
                    }
                }
                let mut u = step(i_next);
                if u != i_prev {
                    loop {
                        copy(m, &mut k, u);
                        u = step(u);
                        if u == i_prev {
                            break;
                        }
                    }
                }
                let old = star(m);
                if old != 0 {
                    m.free(old);
                    m.set_u32(vr + mo::VERTEX_STAR, 0);
                }
                m.set_u32(vr + mo::VERTEX_STAR, new);
                i_prev = at_cur + 1;
            }
            cur = at_cur;
        }
    }
    let at_prev = if i_prev == -1 { if cur != count(m) - 1 { cur + 1 } else { 0 } } else { i_prev };
    // an entry read back from beyond the rewrite's block
    let read_edge = |m: &Mem, k: i32| match beyond {
        Some((b, w)) if b == k => w[1],
        _ => m.i32(entry(m, k) + 4),
    };
    let write_edge = |m: &mut Mem, beyond: &mut Option<(i32, [i32; 3])>, k: i32, e: i32| match beyond {
        Some((b, w)) if *b == k => w[1] = e,
        _ => {
            let at = entry(m, k);
            m.set_i32(at + 4, e)
        }
    };

    // the edge prev → v
    let e = read_edge(m, at_prev);
    if e == 0 {
        let pr = vertex_at(m, mesh, prev);
        let pn = m.u16(pr + mo::VERTEX_NB_STAR) as i32;
        let mut u = 0;
        if pn != 0 {
            let ps = m.u32(pr + mo::VERTEX_STAR);
            while m.i32(ps.wrapping_add((u as u32) * 12)) != v {
                u += 1;
                if pn <= u {
                    break;
                }
            }
        }
        let e = if u == pn || v == prev {
            !insert_edge(m, mesh, prev, v, p, -1)
        } else {
            let e2 = m.i32(m.u32(pr + mo::VERTEX_STAR).wrapping_add((u as u32) * 12 + 4));
            let e = e2.wrapping_neg();
            if e < 1 {
                m.set_i32(edge_record(m, mesh, e2) + 8, p);
            } else {
                m.set_i32(edge_record(m, mesh, e) + 12, p);
            }
            e
        };
        write_edge(m, &mut beyond, at_prev, e);
    } else if e < 1 {
        m.set_i32(edge_record(m, mesh, e.wrapping_neg()) + 8, p);
    } else {
        m.set_i32(edge_record(m, mesh, e) + 12, p);
    }

    // the edge v → next
    let e = m.i32(entry(m, cur) + 4);
    if e == 0 {
        let nr = vertex_at(m, mesh, next);
        let nn = m.u16(nr + mo::VERTEX_NB_STAR) as i32;
        let mut u = 0;
        if nn != 0 {
            let ns = m.u32(nr + mo::VERTEX_STAR);
            loop {
                if m.i32(ns.wrapping_add((u as u32) * 12)) == v {
                    break;
                }
                u += 1;
                if nn <= u {
                    break;
                }
            }
        }
        let e = if u == nn || v == next {
            insert_edge(m, mesh, v, next, p, -1).wrapping_add(1)
        } else {
            let e2 = m.i32(m.u32(nr + mo::VERTEX_STAR).wrapping_add((u as u32) * 12 + 4));
            let e = e2.wrapping_neg();
            if e < 1 {
                m.set_i32(edge_record(m, mesh, e2) + 12, p);
            } else {
                m.set_i32(edge_record(m, mesh, e) + 8, p);
            }
            e
        };
        let at = entry(m, cur);
        m.set_i32(at + 4, e);
        e
    } else {
        if e < 1 {
            m.set_i32(edge_record(m, mesh, e.wrapping_neg()) + 12, p);
        } else {
            m.set_i32(edge_record(m, mesh, e) + 8, p);
        }
        e
    }
}

/// The star of the vertex at `vr` made room for one more entry: when its count is a
/// multiple of four, `SI_realloc` to the next multiple of four plus four.
fn grow_by_one(m: &mut Mem, vr: u32) {
    let n = m.u16(vr + mo::VERTEX_NB_STAR) as u32;
    if n.wrapping_neg() & 3 == 0 {
        let s = m.si_realloc(m.u32(vr + mo::VERTEX_STAR), n.wrapping_sub((n & 3) | 0xffff_fffc).wrapping_mul(12));
        m.set_u32(vr + mo::VERTEX_STAR, s);
    }
}

/// [soft 0x0845a528 si_meshInsertIPolygon]: every corner of polygon `p` filed, contour by
/// contour (corners between −1 separators): [`insert_polygon_at_vertex`] for the contour's
/// last corner first, then for the others in order, each answer stored in the corner as its
/// edge. 0 as soon as one is refused (the corners filed before stay filed); 0 for a polygon
/// number outside the array. While `SI_gblIndex3` is installed, a corner (not the first of
/// the polygon) whose old edge has −1 in the table gets its new edge's number there.
///
/// For a contour of one corner the original takes the corner before it as `prev`: the
/// separator of the contour before (−1, refused by the range test), or, for the first
/// contour, the word 0x24 bytes in front of the node block, whatever the heap holds there
/// (the end of the block allocated before it). The port reads the same word of the image.
/// Where the Rust differs: when that word is not mapped in the image (no block before),
/// −1 (refused); the original reads its heap there.
pub fn insert_ipolygon(m: &mut Mem, o: &mut dyn Outside, mesh: u32, p: i32) -> i32 {
    if p < 0 || m.i32(mesh + mo::NB_POLYGONS) <= p {
        return 0;
    }
    let poly = polygon_at(m, mesh, p);
    let nodes = |m: &Mem| m.u32(poly + mo::POLYGON_NODES);
    let node = |m: &Mem, k: i32| nodes(m).wrapping_add((k as u32).wrapping_mul(mo::NODE));
    let mut k = 0i32;
    let mut i = 0i32;
    loop {
        let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        if count < i {
            break;
        }
        if i == count || m.i32(node(m, i)) == -1 {
            let last = i - 1;
            // a contour of one corner at the start of the array: the original takes the corner "before" it,
            // the word 0x24 bytes in front of the node block (finding 0034, "geomupd8")
            let mut prev = if last >= 1 { m.i32(node(m, last - 1)) } else { word_before_nodes(m, node(m, -1)) };
            if k <= last {
                let mut v = m.i32(node(m, last));
                let mut at = last;
                loop {
                    let next = m.i32(node(m, k));
                    let old = m.i32(node(m, at) + 4).wrapping_abs();
                    let e = insert_polygon_at_vertex(m, o, mesh, p, prev, v, next);
                    let a = node(m, at) + 4;
                    m.set_i32(a, e);
                    if e == 0 {
                        return 0;
                    }
                    let index3 = m[g::SI_GBL_INDEX3];
                    if index3 != 0 && old != 0 && m.i32(index3.wrapping_add((old as u32).wrapping_mul(4))) == -1 && k != 0 {
                        let new = m.i32(node(m, at) + 4).wrapping_abs();
                        m.set_i32(index3.wrapping_add((old as u32).wrapping_mul(4)), new);
                    }
                    prev = v;
                    v = next;
                    at = k;
                    k += 1;
                    if last < k {
                        break;
                    }
                }
            }
            k = i + 1;
        }
        i += 1;
    }
    1
}

/// The word at `at`, in front of a polygon's node block; −1 where the image has nothing
/// readable there.
fn word_before_nodes(m: &Mem, at: u32) -> i32 {
    if m.is_mapped(at, 4) { m.i32(at) } else { -1 }
}

/// A position by vertex number; zeros outside the array (as [`crate::mesh_b`]: the original
/// reads whatever lies there, which only happens where the terms cancel).
fn position(m: &Mem, mesh: u32, v: i32) -> [f32; 3] {
    if v < 0 || m.i32(mesh + mo::NB_VERTICES) <= v {
        return [0.0; 3];
    }
    f3(m, vertex_at(m, mesh, v))
}

/// [soft 0x084599f4 SI_meshComputesPolygonNormal]: the unit normal of polygon `n`, or of
/// every polygon for −1: Newell's sums over each contour as doubles in memory, the length
/// in the register, zero at or below 1e-6. The arithmetic is
/// [`crate::mesh_b::computes_polygon_normal`]'s, here on the image's floats.
pub fn computes_polygon_normal(m: &mut Mem, mesh: u32, n: i32) {
    use si_x87::x;
    let (first, last) = if n == -1 { (0, m.i32(mesh + mo::NB_POLYGONS) - 1) } else { (n, n) };
    let mut p = first;
    while p <= last {
        let poly = polygon_at(m, mesh, p);
        p += 1;
        let (mut sx, mut sy, mut sz) = (0.0f64, 0.0f64, 0.0f64);
        let nb = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        let vert = |m: &Mem, k: i32| m.i32(nodes.wrapping_add((k as u32).wrapping_mul(mo::NODE)));
        let mut start = 0i32;
        for i in 0..=nb {
            if i == nb || vert(m, i) == -1 {
                let end = if i == 0 { 0 } else { i - 1 };
                if start <= end && end < nb {
                    let mut a = position(m, mesh, vert(m, end));
                    for k in start..=end {
                        let b = position(m, mesh, vert(m, k));
                        let tx = (x(a[1]) - b[1]) * (x(a[2]) + b[2]);
                        let tz = (x(a[1]) + b[1]) * (x(a[0]) - b[0]);
                        let ty = (x(a[0]) + b[0]) * (x(a[2]) - b[2]);
                        sx = (tx + sx).to_f64();
                        sz = (tz + sz).to_f64();
                        sy = (ty + sy).to_f64();
                        a = b;
                    }
                }
                start = i + 1;
            }
        }
        let mut normal = [0.0f32; 3];
        let len = ((x(sx) * sx + x(sy) * sy) + x(sz) * sz).sqrt();
        if len.gt(crate::mesh_b::polygon_normal_min()) {
            normal = [(x(sx) / len).to_f32(), (x(sy) / len).to_f32(), (x(sz) / len).to_f32()];
        }
        put_f3(m, poly, normal);
    }
}

/// [soft 0x0845a6e4 SI_meshComputeHardEdge]: edge `n`, or every edge for −1, marked border
/// (0x20: a side without polygon) and, unless border or set by the user (0x08), as a
/// discontinuity (0x10) when the mesh has the automatic discontinuity (flag 0x4 or 0x8)
/// and `SI_vectorCosAngle` of its polygons' normals is not above the mesh's cosine (`si-math`).
/// Nothing for `n` below −1, 0 or above the count.
pub fn compute_hard_edge(m: &mut Mem, mesh: u32, n: i32) {
    use si_math::{algebra::vector_cos_angle, types::Vector};
    let count = m.i32(mesh + mo::NB_EDGES);
    if n < -1 || n == 0 || count < n {
        return;
    }
    let (first, last) = if n == -1 { (1, count) } else { (n, n) };
    for e in first..=last {
        let r = edge_record(m, mesh, e);
        let mut flag = m.u16(r + mo::EDGE_FLAG) & !EDGE_BORDER;
        m.set_u16(r + mo::EDGE_FLAG, flag);
        let (p0, p1) = (m.i32(r + 8), m.i32(r + 12));
        if p0 == -1 || p1 == -1 {
            flag |= EDGE_BORDER;
            m.set_u16(r + mo::EDGE_FLAG, flag);
        }
        if flag & (EDGE_BORDER | EDGE_USER_HARD) == 0 {
            m.set_u16(r + mo::EDGE_FLAG, flag & !EDGE_AUTO_HARD);
            if m.u8(mesh + mo::FLAGS) & 0xc != 0 {
                let a = Vector::from_array(f3(m, polygon_at(m, mesh, p0)));
                let b = Vector::from_array(f3(m, polygon_at(m, mesh, p1)));
                if vector_cos_angle(&a, &b) <= m.f32(mesh + mo::ANGLE_COS) {
                    m.or_u8(r + mo::EDGE_FLAG, EDGE_AUTO_HARD as u8);
                }
            }
        }
    }
}

/// [soft 0x08459e8c SI_meshComputesVertexNormal]: the corner normals at vertex `n`; at every
/// vertex for a negative `n`, for −2 only where the vertex flag has bit 15. The rules (the
/// mesh flags, faceted, closed smooth fans, runs between hard edges, polygons with 0x400
/// kept) and the arithmetic are [`crate::mesh_b::computes_vertex_normal`]'s, here on the
/// image (the corner written is the first corner of the polygon at that vertex).
///
/// Where the Rust differs: as the value form, in the smooth and flag-2 cases a polygon with no
/// corner at the vertex (an inconsistent star) gets nothing written. The hard-edge case
/// keeps the original's single corner index: a polygon with no corner there is written at
/// the index left by the last search (0 before the first), even into a separator.
pub fn computes_vertex_normal(m: &mut Mem, mesh: u32, n: i32) {
    use crate::mesh::Star;
    let flags = m.u16(mesh + mo::FLAGS);
    if flags & 0x800 != 0 {
        m.set_u16(mesh + mo::FLAGS, flags | 8);
    } else if flags & 0xa == 0 {
        return;
    }
    let flags = m.u16(mesh + mo::FLAGS);
    let nv = m.i32(mesh + mo::NB_VERTICES);
    let (first, last) = if n < 0 { (0, nv - 1) } else { (n, n) };
    let pnormal = |m: &Mem, p: i32| f3(m, polygon_at(m, mesh, p));
    let pflag = |m: &Mem, p: i32| m.i16(polygon_at(m, mesh, p) + mo::POLYGON_FLAG);
    let eflag = |m: &Mem, e: i32| m.u16(edge_record(m, mesh, e.wrapping_abs()) + mo::EDGE_FLAG);
    let set_corner = |m: &mut Mem, p: i32, v: i32, normal: [f32; 3]| {
        let poly = polygon_at(m, mesh, p);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        for k in 0..m.u16(poly + mo::POLYGON_NB_NODES) as u32 {
            let node = nodes + mo::NODE * k;
            if m.i32(node) == v {
                put_f3(m, node + mo::NODE_NORMAL, normal);
                return;
            }
        }
    };
    let mut corner = 0u32;
    for v in first..=last {
        if nv <= v {
            break; // the original does not test `n` against the count
        }
        let vr = vertex_at(m, mesh, v);
        if n == -2 && m.i16(vr + mo::VERTEX_FLAG) >= 0 {
            continue;
        }
        let count = m.u16(vr + mo::VERTEX_NB_STAR) as i32;
        if count == 0 {
            continue;
        }
        let sa = m.u32(vr + mo::VERTEX_STAR);
        let star: Vec<Star> = (0..count as u32).map(|k| Star { vertex: m.i32(sa + 12 * k), edge: m.i32(sa + 12 * k + 4), polygon: m.i32(sa + 12 * k + 8) }).collect();
        if flags & 2 != 0 {
            for s in &star {
                if s.polygon != -1 {
                    let normal = pnormal(m, s.polygon);
                    set_corner(m, s.polygon, v, normal);
                }
            }
            continue;
        }
        let s = |k: i32| star[k as usize];
        let mut split = false;
        let mut start = 0i32;
        let mut i = 0i32;
        while i <= count {
            if i > start && i < count {
                let e = s(i).edge;
                let r = edge_record(m, mesh, e.wrapping_abs());
                let across = if e > 0 { m.i32(r + 12) } else { m.i32(r + 8) };
                if across == -1 || across != s(i - 1).polygon {
                    split = true;
                }
            }
            if !split && i < count && s(i).vertex != -1 {
                i += 1;
                continue;
            }
            let end = i - 1;
            let mut sum = [0.0f32; 3];
            let mut smooth = true;
            for k in start..=end {
                if eflag(m, s(k).edge) & (EDGE_AUTO_HARD | EDGE_USER_HARD) != 0 || s(k).polygon == -1 {
                    smooth = false;
                    break;
                }
                let pn = pnormal(m, s(k).polygon);
                sum = [pn[0] + sum[0], pn[1] + sum[1], pn[2] + sum[2]];
            }
            if smooth {
                normalize_sum(&mut sum);
                for k in start..=end {
                    let p = s(k).polygon;
                    if p != -1 && pflag(m, p) & 0x400 == 0 {
                        set_corner(m, p, v, sum);
                    }
                }
            } else {
                let hard = EDGE_AUTO_HARD | EDGE_USER_HARD | EDGE_BORDER;
                for k in start..=end {
                    let p = s(k).polygon;
                    if p == -1 || pflag(m, p) & 0x400 != 0 {
                        continue;
                    }
                    let stop = if k == end { start } else { k + 1 };
                    let mut at = k;
                    while at != stop {
                        if eflag(m, s(at).edge) & hard != 0 {
                            break;
                        }
                        at = if at == start { end } else { at - 1 };
                    }
                    let from = at;
                    let mut acc = match s(at).polygon {
                        -1 => [0.0f32; 3],
                        q => pnormal(m, q),
                    };
                    loop {
                        at = if at == end { start } else { at + 1 };
                        if at == from || eflag(m, s(at).edge) & hard != 0 {
                            break;
                        }
                        let q = s(at).polygon;
                        if q != -1 {
                            let pn = pnormal(m, q);
                            acc = [acc[0] + pn[0], acc[1] + pn[1], acc[2] + pn[2]];
                        }
                    }
                    normalize_sum(&mut acc);
                    // the original's corner index (`local_48`) is one variable for the whole call: a
                    // polygon whose corners do not name `v` (a star the edge routines left behind) is
                    // written at the index the last search left (0 before the first, as unset stack)
                    let poly = polygon_at(m, mesh, p);
                    let nodes = m.u32(poly + mo::POLYGON_NODES);
                    if m.u16(poly + mo::POLYGON_NB_NODES) != 0 {
                        if let Some(j) = (0..m.u16(poly + mo::POLYGON_NB_NODES) as u32).find(|&j| m.i32(nodes + mo::NODE * j) == v) {
                            corner = j;
                        }
                    }
                    put_f3(m, nodes + mo::NODE * corner + mo::NODE_NORMAL, acc);
                }
            }
            if i < count {
                start = if s(i).vertex == -1 { i + 1 } else { i };
            }
            split = false;
            i += 1;
        }
    }
}

/// The corner-normal pass's normalisation (as [`crate::mesh_b`]): the length rounded to
/// float, each component divided by it when its size is above 0.0001.
fn normalize_sum(v: &mut [f32; 3]) {
    use si_x87::x;
    let len = ((x(v[1]) * v[1] + x(v[0]) * v[0]) + x(v[2]) * v[2]).sqrt().to_f32();
    if x(len).abs().gt(x(1e-4f64)) {
        *v = [v[0] / len, v[1] / len, v[2] / len];
    }
}

/// [soft 0x0845a7c8 SI_meshOptimizeDisplay]: the wire-frame polylines. The old array is
/// freed; every edge's bit 0x8000 cleared and set again where its polygons are all hidden
/// (0x800); `SI_calloc(3 × the others, 4)`; then, vertex by vertex (`SI_showActivity` every
/// 64th) and star entry by star entry, each unmarked edge starts a line followed from vertex
/// to vertex through the first unmarked edge of each, −1 after it. The count ends one less
/// than the integers written.
pub fn optimize_display(m: &mut Mem, o: &mut dyn Outside, mesh: u32) {
    let old = m.u32(mesh + 0x28);
    if old != 0 {
        m.free(old);
        m.set_u32(mesh + 0x28, 0);
    }
    m.set_u32(mesh + 0x28, 0);
    m.set_i32(mesh + 0x24, 0);
    let hidden = |m: &Mem, p: i32| m.u8(polygon_at(m, mesh, p) + 0x15) & 8 != 0;
    let mut counted = 0i32;
    let mut e = 1;
    while e <= m.i32(mesh + mo::NB_EDGES) {
        let r = edge_record(m, mesh, e);
        let p0 = m.i32(r + 8);
        let flag = m.u16(r + mo::EDGE_FLAG) & 0x7fff;
        m.set_u16(r + mo::EDGE_FLAG, flag);
        let p1 = m.i32(r + 12);
        let mark = if p0 == -1 {
            p1 != -1 && hidden(m, p1)
        } else if p1 == -1 {
            hidden(m, p0)
        } else {
            hidden(m, p0) && hidden(m, p1)
        };
        if mark {
            m.set_u16(r + mo::EDGE_FLAG, flag | EDGE_DISPLAYED);
        }
        if m.i16(r + mo::EDGE_FLAG) >= 0 {
            counted += 1;
        }
        e += 1;
    }
    let mut out = m.si_calloc((counted as u32).wrapping_mul(3), 4);
    m.set_u32(mesh + 0x28, out);
    let bump = |m: &mut Mem| {
        let n = m.i32(mesh + 0x24);
        m.set_i32(mesh + 0x24, n.wrapping_add(1));
    };
    // the first star entry of vertex `w` whose edge is not marked yet: marked, and its
    // neighbour answered
    let take = |m: &mut Mem, w: i32| -> i32 {
        let wr = vertex_at(m, mesh, w);
        let mut u = 0i32;
        while u < m.u16(wr + mo::VERTEX_NB_STAR) as i32 {
            let at = m.u32(wr + mo::VERTEX_STAR).wrapping_add((u as u32) * 12);
            let e = m.i32(at + 4);
            if e != 0 {
                let r = edge_record(m, mesh, e.wrapping_abs());
                let f = m.u16(r + mo::EDGE_FLAG);
                if (f as i16) >= 0 {
                    m.set_u16(r + mo::EDGE_FLAG, f | EDGE_DISPLAYED);
                    return m.i32(at);
                }
            }
            u += 1;
        }
        -1
    };
    let mut v = 0i32;
    while v < m.i32(mesh + mo::NB_VERTICES) {
        if v & 0x3f == 0 {
            o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
        }
        let vr = vertex_at(m, mesh, v);
        let mut k = 0i32;
        while k < m.u16(vr + mo::VERTEX_NB_STAR) as i32 {
            let at = m.u32(vr + mo::VERTEX_STAR).wrapping_add((k as u32) * 12);
            k += 1;
            let e = m.i32(at + 4);
            if e == 0 {
                continue;
            }
            let r = edge_record(m, mesh, e.wrapping_abs());
            let f = m.u16(r + mo::EDGE_FLAG);
            if (f as i16) < 0 {
                continue;
            }
            m.set_u16(r + mo::EDGE_FLAG, f | EDGE_DISPLAYED);
            m.set_i32(out, v);
            bump(m);
            let mut next = m.i32(at);
            loop {
                let slot = out + 4;
                if next == -1 {
                    m.set_i32(slot, -1);
                    out += 8;
                    bump(m);
                    break;
                }
                m.set_i32(slot, next);
                bump(m);
                out = slot;
                next = take(m, next);
            }
        }
        v += 1;
    }
    let n = m.i32(mesh + 0x24);
    m.set_i32(mesh + 0x24, n.wrapping_sub(1));
}

/// [soft 0x08458cf8 SI_meshUpdateNodes]: every polygon filed ([`insert_ipolygon`]; a
/// refusal remembered and the others still filed), then with `SI_showActivity` between
/// them the polygon normals, the border and discontinuity edges, the corner normals when
/// `with_normals`, the display lines, and `SI_meshUpdatePolygonLists`. Answers 0 when a
/// polygon was refused.
pub fn update_nodes(m: &mut Mem, o: &mut dyn Outside, mesh: u32, with_normals: i16) -> i16 {
    let mut ok = 1;
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        if insert_ipolygon(m, o, mesh, p) as i16 == 0 {
            ok = 0;
        }
        p += 1;
    }
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    computes_polygon_normal(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    compute_hard_edge(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    if with_normals != 0 {
        computes_vertex_normal(m, mesh, -1);
    }
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    optimize_display(m, o, mesh);
    super::lists::update_polygon_lists(m, o, mesh);
    ok
}

/// [soft 0x08458d94 SI_meshUpdate]: the same with the corner normals always and no answer.
pub fn update(m: &mut Mem, o: &mut dyn Outside, mesh: u32) {
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        insert_ipolygon(m, o, mesh, p);
        p += 1;
    }
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    computes_polygon_normal(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    compute_hard_edge(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    computes_vertex_normal(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    optimize_display(m, o, mesh);
    super::lists::update_polygon_lists(m, o, mesh);
}
