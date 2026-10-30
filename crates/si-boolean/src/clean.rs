// SPDX-License-Identifier: MIT
//! The clean-up drivers of `boolean.c` (finding 0037): `si_cleanUpEdges`, later the others.

use crate::boolean::*;
use si_mem::{Mem, Outside};
use si_x87::x;

fn r(m: &Mem, a: u32) -> u32 {
    m.u32(a)
}
fn w(m: &mut Mem, a: u32, v: u32) {
    m.set_u32(a, v)
}
/// The face slot `edge->face[end]` address of an edge node.
fn face_slot(m: &Mem, node: u32) -> u32 {
    r(m, node + n::EDGE) + e::FACE + m.u8(node + n::END) as u32 * 4
}
/// The face slot of the other side.
fn other_slot(m: &Mem, node: u32) -> u32 {
    r(m, node + n::EDGE) + e::FACE + (1 - m.u8(node + n::END) as u32) * 4
}

/// `si_cleanUpEdges(plist)` [soft 0x08312038 si_cleanUpEdges]: over the face list `*plist`: merges similar
/// ring edges, then per contour removes edges that continue a straight line (or whose end points coincide
/// within 1e-3), joining the two faces' edge nodes, deletes faces left with fewer than three edges, and
/// resets the visit stamps of each contour it finished with.
pub fn si_clean_up_edges(m: &mut Mem, plist: u32) {
    const GUARD: u32 = 1 << 22;
    // pass 1: merge similar edges of every edge-node ring
    let mut face = r(m, plist);
    while face != 0 {
        let mut con = r(m, face + f::CONTOURS);
        while con != 0 {
            let first = r(m, con + c::FIRST);
            if first != 0 {
                let mut node = first;
                let mut g = 0;
                loop {
                    let mut edge = r(m, node + n::EDGE);
                    if r(m, edge + e::NEXT) != edge {
                        let mut g2 = 0;
                        loop {
                            merge_similar_edge(m, edge, 0);
                            merge_similar_edge(m, edge, 1);
                            let nx = r(m, edge + e::NEXT);
                            if edge == nx {
                                break;
                            }
                            edge = nx;
                            g2 += 1;
                            if edge == r(m, node + n::EDGE) || g2 > GUARD {
                                break;
                            }
                        }
                    }
                    node = r(m, node + n::NEXT);
                    g += 1;
                    if node == r(m, con + c::FIRST) || g > GUARD {
                        break;
                    }
                }
            }
            con = r(m, con + c::NEXT);
        }
        face = r(m, face + f::NEXT);
    }
    // scratch for the address-taken locals local_c / local_10 / local_8
    let fr = m.local(16);
    let r_c = fr;
    let mut pface = r(m, plist);
    'faces: loop {
        // next face with contours
        let (cur, mut con) = loop {
            if pface == 0 {
                m.release(16);
                return;
            }
            let cur = pface;
            pface = r(m, cur + f::NEXT);
            let con = r(m, cur + f::CONTOURS);
            if con != 0 {
                break (cur, con);
            }
        };
        let mut done = false; // bVar10
        loop {
            let mut node = r(m, con + c::FIRST);
            if node != 0 {
                let mut count = get_nb_edges(m, node);
                // LAB_08312382 loop
                loop {
                    let mut next = r(m, node + n::NEXT);
                    let edge = r(m, node + n::EDGE);
                    let end = m.u8(node + n::END) as u32;
                    let here = r(m, edge + e::FACE + end * 4);
                    let fa = r(m, edge + e::FACE + (1 - end) * 4);
                    let nedge = r(m, next + n::EDGE);
                    let nend = m.u8(next + n::END) as u32;
                    let fb = r(m, nedge + e::FACE + (1 - nend) * 4);
                    let far = r(m, edge + e::PT + (1 - end) * 4);
                    let cond_a = count > 3
                        && (m.u8(far + p::FLAG) != 0 || m.u8(REMOVE_POINTS.addr()) != 0)
                        && here == r(m, nedge + e::FACE + nend * 4)
                        && fa == fb
                        && (colinear(m, node, next) & 0xff) != 0
                        && (here != 0 || fa != 0);
                    let mut stop = false; // goto LAB_0831260a
                    if cond_a {
                        let mut skip = false;
                        let mut nb = 0i32;
                        if fa != 0 {
                            let nd = find_edge_node(m, fa, nedge, r_c);
                            if nd == 0 {
                                w(m, node, 0xffff_ffff);
                            } else if r(m, r(m, nd + n::NEXT) + n::EDGE) == edge {
                                let cc = r(m, r_c);
                                nb = get_nb_edges(m, r(m, cc));
                                if nb > 3 {
                                    if nd == r(m, nd + n::NEXT) {
                                        w(m, node, 0xffff_ffff);
                                    }
                                    let nn = r(m, nd + n::NEXT);
                                    w(m, cc, nn);
                                    let pv = r(m, nd + n::PREV);
                                    w(m, pv + n::NEXT, nn);
                                    w(m, nn + n::PREV, pv);
                                }
                            } else {
                                w(m, node, 0xffff_ffff);
                            }
                            if fa != 0 && nb < 4 {
                                w(m, node, 0xffff_ffff);
                                skip = true;
                            }
                        }
                        if !skip {
                            let v = r(m, nedge + e::PT + (1 - nend) * 4);
                            w(m, edge + e::PT + (1 - end) * 4, v);
                            next = node;
                            let nn = r(m, r(m, node + n::NEXT) + n::NEXT);
                            w(m, nn + n::PREV, node);
                            w(m, node + n::NEXT, nn);
                            count -= 1;
                        }
                        // LAB_08312369
                        node = next;
                        if r(m, node) == 0xffff_ffff || done {
                            stop = true;
                        }
                    } else if count > 2 {
                        let near = near_point_edge(m, edge, f64::from_bits(0x3f50624d_d2f1a9fc));
                        if near == 0 {
                            w(m, node, 0xffff_ffff);
                        } else {
                            count -= 1;
                            let cur_nb = m[CUR_NB_EDGES];
                            let mut lim = 100i32;
                            let keep = r(m, edge + e::PT);
                            let gone = r(m, edge + e::PT + 4);
                            let mut blk = m[HEAD_EDGE_TABLE];
                            while blk != 0 {
                                if r(m, blk + 0x3074) == 0 {
                                    lim = cur_nb % 100;
                                }
                                for k in 0..lim.max(0) as u32 {
                                    let base = blk + 8 + k * 0x7c;
                                    for j in 0..2u32 {
                                        if r(m, base + j * 4) == gone {
                                            w(m, base + j * 4, keep);
                                        }
                                    }
                                }
                                blk = r(m, blk + 0x3074);
                            }
                            let start = r(m, node + n::EDGE);
                            let mut ed = start;
                            let mut g = 0;
                            loop {
                                for s in 0..2u32 {
                                    let slot = ed + e::FACE + s * 4;
                                    let pf = r(m, slot);
                                    if pf != 0 {
                                        delete_edge(m, pf, ed);
                                        let pcon = r(m, pf + f::CONTOURS);
                                        if pcon != 0 {
                                            let nb = get_nb_edges(m, r(m, pcon));
                                            if (nb as u32).wrapping_sub(1) < 2 {
                                                let i17 = r(m, pcon);
                                                let i13 = r(m, i17 + n::NEXT);
                                                if i17 == i13 {
                                                    let sl = face_slot(m, i17);
                                                    w(m, sl, 0);
                                                } else {
                                                    let e13 = r(m, i13 + n::EDGE);
                                                    let f13 = r(m, other_slot(m, i13));
                                                    let fnd = find_edge_node(m, f13, e13, r_c + 8);
                                                    if fnd != 0 && e13 == r(m, fnd + n::EDGE) {
                                                        let v = r(m, other_slot(m, i13));
                                                        let sl = face_slot(m, i17);
                                                        w(m, sl, v);
                                                        w(m, e13 + e::FACE, 0);
                                                        w(m, e13 + e::FACE + 4, 0);
                                                        let ee = r(m, i17 + n::EDGE);
                                                        w(m, fnd + n::EDGE, ee);
                                                        let en = m.u8(i17 + n::END);
                                                        m.set_u8(fnd + n::END, en);
                                                    }
                                                }
                                                remove_face_from_list(m, plist, pf);
                                                reset_face(m, pf);
                                                if pf == cur {
                                                    done = true;
                                                }
                                            }
                                        }
                                        w(m, slot, 0);
                                    }
                                }
                                ed = r(m, ed + e::NEXT);
                                g += 1;
                                if ed == r(m, node + n::EDGE) || g > GUARD {
                                    break;
                                }
                            }
                        }
                        // LAB_08312369
                        node = next;
                        if r(m, node) == 0xffff_ffff || done {
                            stop = true;
                        }
                    } else {
                        if count == 1 {
                            let sl = face_slot(m, node);
                            w(m, sl, 0);
                        } else if count == 2 {
                            let sl = face_slot(m, node);
                            w(m, sl, fb);
                            let nd = find_edge_node(m, fb, nedge, r_c + 4);
                            if nd != 0 {
                                let ee = r(m, node + n::EDGE);
                                w(m, nd + n::EDGE, ee);
                                let en = m.u8(node + n::END);
                                m.set_u8(nd + n::END, en);
                            }
                        }
                        if r(m, con + c::NEXT - 4) == 0 {
                            remove_face_from_list(m, plist, cur);
                        } else {
                            let pv = r(m, con + 4);
                            let nx = r(m, con + 8);
                            w(m, pv + 8, nx);
                            if nx != 0 {
                                w(m, nx + 4, pv);
                            }
                        }
                        done = true;
                        stop = true;
                    }
                    if stop {
                        break;
                    }
                }
                // LAB_0831260a
                w(m, con, node);
                let mut g = 0;
                loop {
                    let nx = r(m, node + n::NEXT);
                    w(m, node, 0);
                    node = nx;
                    g += 1;
                    if r(m, node) == 0 || g > GUARD {
                        break;
                    }
                }
            }
            con = r(m, con + 8);
            if con == 0 || done {
                continue 'faces;
            }
        }
    }
}

/// `a < b` or the difference within `eps` (80-bit), both doubles at addresses.
fn le_close(m: &Mem, a: u32, b: u32, eps: f64) -> bool {
    let (fa, fb) = (d(m, a), d(m, b));
    fa.lt(fb) || (fb - fa).abs().lt(x(eps))
}

/// `si_cleanUpFaces(plist)` [soft 0x0831265c si_cleanUpFaces]: over the face list `*plist`: walks each contour of
/// edge nodes (a face with a negative id loses its contours) and takes out edges that fold back on themselves (an
/// edge twice in a row, or with the same edge as the previous node), cuts a contour in two where it passes the
/// same edge twice, and merges two faces that lie on both sides of an edge when their planes agree within 5e-7 (or
/// their ids are equal): the other face's contour is joined into this one's and that face is dropped from the
/// list. Contours left empty are unlinked; a face without contours leaves the list; the first contour is
/// swapped with a later one whose box encloses it (within 1e-6).
pub fn si_clean_up_faces(m: &mut Mem, o: &mut dyn Outside, plist: u32) {
    const GUARD: u32 = 1 << 22;
    const MIN1: u32 = 0;
    const MAX1: u32 = 24;
    const MIN2: u32 = 48;
    const MAX2: u32 = 72;
    const L74: u32 = 96;
    const L6C: u32 = 104;
    const L78: u32 = 112;
    let fr = m.local(0x78);
    let mut next_face = r(m, plist); // local_80 in the decompile
    let mut outer = 0u32;
    loop {
        let face = next_face;
        if face == 0 {
            break;
        }
        outer += 1;
        if outer > GUARD {
            break;
        }
        next_face = r(m, face + f::NEXT);
        let mut con = r(m, face + f::CONTOURS);
        if m.i32(face + f::ID) < 0 {
            w(m, face + f::CONTOURS, 0);
            con = 0;
        }
        while con != 0 {
            let mut node = r(m, con + c::FIRST);
            if node != 0 {
                w(m, con, 0);
                let mut g = 0u32;
                // `nxt` is local_88 of the decompile
                let mut nxt;
                loop {
                    let after = r(m, node + n::NEXT);
                    let ed = r(m, node + n::EDGE);
                    w(m, node, 0xffff_ffff);
                    nxt = after;
                    if m.u8(ed + 6) != 0 || m.u8(REMOVE_EDGES.addr()) != 0 {
                        if ed == r(m, after + n::EDGE) {
                            if node == after {
                                nxt = 0;
                            } else {
                                nxt = r(m, after + n::NEXT);
                                if node == nxt {
                                    nxt = 0;
                                } else {
                                    let pv = r(m, node + n::PREV);
                                    w(m, pv + n::NEXT, nxt);
                                    w(m, nxt + n::PREV, pv);
                                }
                            }
                        } else {
                            let mut p8 = r(m, node + n::PREV);
                            if ed == r(m, p8 + n::EDGE) {
                                if node == p8 {
                                    nxt = 0;
                                } else {
                                    p8 = r(m, p8 + n::PREV);
                                    if node == p8 {
                                        nxt = 0;
                                    } else {
                                        w(m, p8 + n::NEXT, after);
                                        w(m, after + n::PREV, p8);
                                    }
                                }
                            } else {
                                let fa = r(m, ed + e::FACE);
                                let fb = r(m, ed + e::FACE + 4);
                                if fa != 0 && fb != 0 && m.i32(fa + f::ID) >= 0 && m.i32(fb + f::ID) >= 0 {
                                    let nedge = r(m, node + n::EDGE);
                                    let a = fa;
                                    let b = r(m, nedge + e::FACE + 4);
                                    w(m, fr + L6C, a);
                                    w(m, fr + L6C + 4, b);
                                    if a == b && m.i32(a + f::ID) > 0 {
                                        let mut p8 = after;
                                        let mut g2 = 0u32;
                                        loop {
                                            if r(m, p8 + n::EDGE) == r(m, node + n::EDGE) {
                                                break;
                                            }
                                            p8 = r(m, p8 + n::NEXT);
                                            g2 += 1;
                                            if r(m, p8) == 0xffff_ffff || g2 > GUARD {
                                                // the loop condition `*p8 != -1` ends the walk
                                                if r(m, p8) != 0xffff_ffff {
                                                    break;
                                                }
                                                break;
                                            }
                                        }
                                        if p8 != node {
                                            nxt = r(m, p8 + n::NEXT);
                                            if nxt == node {
                                                let ppv = r(m, p8 + n::PREV);
                                                let nn = r(m, node + n::NEXT);
                                                w(m, ppv + n::NEXT, nn);
                                                w(m, nn + n::PREV, ppv);
                                                nxt = after;
                                            } else {
                                                let pe3 = r(m, node + n::NEXT);
                                                let ppv = r(m, p8 + n::PREV);
                                                w(m, ppv + n::NEXT, pe3);
                                                w(m, pe3 + n::PREV, ppv);
                                                let npv = r(m, node + n::PREV);
                                                w(m, npv + n::NEXT, nxt);
                                                w(m, nxt + n::PREV, npv);
                                                let nc = call(m, o, SI_CREATE_CONTOUR, &[pe3]);
                                                // local_84 is never null in this loop
                                                let cn = r(m, con + c::NEXT);
                                                w(m, nc + 8, cn);
                                                w(m, nc + 4, con);
                                                w(m, con + 8, nc);
                                                let nn = r(m, nc + 8);
                                                if nn != 0 {
                                                    w(m, nn + 4, nc);
                                                }
                                            }
                                        }
                                    } else {
                                        let close = |m: &Mem, off: u32| -> bool {
                                            (d(m, a + off) - d(m, b + off)).abs().lt(x(5e-7))
                                        };
                                        if m.i32(a + f::ID) == m.i32(b + f::ID)
                                            || (close(m, 0) && close(m, 8) && close(m, 0x10) && close(m, 0x18) && m.i32(a + f::COUNT) == m.i32(b + f::COUNT))
                                        {
                                            let idx = (face == a) as u32;
                                            let other = r(m, fr + L6C + idx * 4);
                                            w(m, fr + L74 + (1 - idx) * 4, node);
                                            let found = find_edge_node(m, other, r(m, node + n::EDGE), fr + L78);
                                            w(m, fr + L74 + idx * 4, found);
                                            if found != 0 {
                                                nxt = r(m, found + n::NEXT);
                                                let cc = r(m, fr + L78);
                                                let mut q = r(m, cc);
                                                let stamp = next_visit(m);
                                                let mut g3 = 0u32;
                                                loop {
                                                    w(m, q, stamp);
                                                    let qe = r(m, q + n::EDGE);
                                                    w(m, qe + e::FACE + m.u8(q + n::END) as u32 * 4, face);
                                                    q = r(m, q + n::NEXT);
                                                    g3 += 1;
                                                    if r(m, q) == stamp || g3 > GUARD {
                                                        break;
                                                    }
                                                }
                                                let l0 = r(m, fr + L74);
                                                let l1 = r(m, fr + L74 + 4);
                                                let v = r(m, l1 + n::NEXT);
                                                let pv = r(m, l0 + n::PREV);
                                                w(m, pv + n::NEXT, v);
                                                w(m, r(m, l1 + n::NEXT) + n::PREV, pv);
                                                let pv1 = r(m, l1 + n::PREV);
                                                w(m, r(m, l0 + n::NEXT) + n::PREV, pv1);
                                                let nx0 = r(m, l0 + n::NEXT);
                                                w(m, r(m, l1 + n::PREV) + n::NEXT, nx0);
                                                // unlink the contour from the other face
                                                let cp = r(m, cc + 4);
                                                let cn = r(m, cc + 8);
                                                if cp == 0 {
                                                    w(m, other + f::CONTOURS, cn);
                                                } else {
                                                    w(m, cp + 8, cn);
                                                }
                                                if r(m, cc + 8) != 0 {
                                                    let cn2 = r(m, cc + 8);
                                                    let cp2 = r(m, cc + 4);
                                                    w(m, cn2 + 4, cp2);
                                                }
                                            }
                                            let ne = r(m, node + n::EDGE);
                                            w(m, ne + e::FACE, 0);
                                            w(m, ne + e::FACE + 4, 0);
                                            if other == next_face {
                                                next_face = r(m, next_face + f::NEXT);
                                            }
                                            remove_face_from_list(m, plist, other);
                                            let oc = r(m, other + f::CONTOURS);
                                            if oc != 0 {
                                                let mut p7 = con;
                                                let mut g4 = 0u32;
                                                while r(m, p7 + 8) != 0 {
                                                    if p7 == r(m, p7 + 8) {
                                                        w(m, p7 + 8, 0);
                                                        break;
                                                    }
                                                    p7 = r(m, p7 + 8);
                                                    g4 += 1;
                                                    if g4 > GUARD {
                                                        break;
                                                    }
                                                }
                                                w(m, p7 + 8, oc);
                                                w(m, oc + 4, p7);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    g += 1;
                    if nxt == 0 || g > GUARD {
                        break;
                    }
                    node = nxt;
                    if r(m, nxt) == 0xffff_ffff {
                        break;
                    }
                }
                w(m, con, nxt);
            }
            if con == r(m, con + 8) {
                w(m, con + 8, 0);
            }
            con = r(m, con + 8);
        }
        // second walk: close the rings, drop empty contours
        let mut cc = r(m, face + f::CONTOURS);
        while cc != 0 {
            let cn = r(m, cc + 8);
            let first = r(m, cc);
            if first == 0 {
                let cp = r(m, cc + 4);
                if cp == 0 {
                    w(m, face + f::CONTOURS, cn);
                } else {
                    w(m, cp + 8, cn);
                }
                if cn != 0 {
                    w(m, cn + 4, cp);
                }
                w(m, cc + 4, 0);
                w(m, cc + 8, 0);
            } else {
                let mut q = first;
                let mut g = 0u32;
                loop {
                    w(m, q, 0);
                    q = r(m, q + n::NEXT);
                    if r(m, q) == 0 {
                        if q != r(m, cc) {
                            w(m, q + n::NEXT, r(m, cc));
                        }
                        break;
                    }
                    g += 1;
                    if q == r(m, cc) || g > GUARD {
                        break;
                    }
                }
            }
            let same = cc == cn;
            cc = cn;
            if same {
                cc = 0;
            }
        }
        let c0 = r(m, face + f::CONTOURS);
        if c0 == 0 {
            remove_face_from_list(m, plist, face);
        } else if r(m, c0 + 8) != 0 && r(m, c0) != 0 {
            compute_face_size(m, r(m, c0), fr + MIN1, fr + MAX1, true);
            let mut cc = r(m, face + f::CONTOURS);
            let mut g = 0u32;
            loop {
                cc = r(m, cc + 8);
                if cc == 0 {
                    break;
                }
                if r(m, cc) != 0 {
                    compute_face_size(m, r(m, cc), fr + MIN2, fr + MAX2, true);
                    let e6 = 1e-6;
                    let ok = le_close(m, fr + MIN2, fr + MIN1, e6)
                        && le_close(m, fr + MIN2 + 8, fr + MIN1 + 8, e6)
                        && le_close(m, fr + MIN2 + 16, fr + MIN1 + 16, e6)
                        && le_close(m, fr + MAX1, fr + MAX2, e6)
                        && le_close(m, fr + MAX1 + 8, fr + MAX2 + 8, e6)
                        && le_close(m, fr + MAX1 + 16, fr + MAX2 + 16, e6);
                    if ok {
                        let t = r(m, cc);
                        let c0 = r(m, face + f::CONTOURS);
                        w(m, cc, r(m, c0));
                        w(m, c0, t);
                        m.copy(fr + MIN1, fr + MIN2, 24);
                        m.copy(fr + MAX1, fr + MAX2, 24);
                    }
                }
                if cc == r(m, cc + 8) {
                    w(m, cc + 8, 0);
                }
                g += 1;
                if g > GUARD {
                    break;
                }
            }
            next_face = r(m, face + f::NEXT);
        }
    }
    m.release(0x78);
}
