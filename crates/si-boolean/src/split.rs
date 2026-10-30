// SPDX-License-Identifier: MIT
//! `si_splitFace` of `boolean.c` (finding 0037): cuts one face by a tree node's plane.

use crate::boolean::*;
use si_mem::{Mem, Outside};
use si_x87::x;

/// Stack frame words (the original passes the addresses of these locals to its callees).
const FLAG: u32 = 0; // local_4d: getInterListNode's "rewind" byte
const PFIRST: u32 = 4; // local_58: first inter node ever (addInterNode's pfirst)
const PHEAD: u32 = 8; // local_54: head of the sorted inter list (addInterNode's phead)
const VEC: u32 = 0x10; // local_1c: the direction vector (24 bytes)
const PLIST: u32 = 0x28; // local_60: findOnEdge's list
const POUT: u32 = 0x2c; // local_5c
const PSTAT: u32 = 0x30; // local_64
const MN: u32 = 0x38; // local_34 (24 bytes)
const MX: u32 = 0x50; // local_4c (24 bytes)
const FRAME: u32 = 0x68;

fn r(m: &Mem, a: u32) -> u32 {
    m.u32(a)
}
fn w(m: &mut Mem, a: u32, v: u32) {
    m.set_u32(a, v)
}

/// `edge->pt[1 - end]` of an edge node.
fn far_pt(m: &Mem, node: u32) -> u32 {
    let edge = r(m, node + n::EDGE);
    r(m, edge + e::PT + (1 - m.u8(node + n::END) as u32).wrapping_mul(4))
}

/// Sets a point to "on the plane": distance 0, side 1.
fn on_plane(m: &mut Mem, pt: u32) {
    w(m, pt + p::DIST, 0);
    w(m, pt + p::DIST + 4, 0);
    m.set_u8(pt + p::SIDE, 1);
}

/// Pushes a contour made from the edge list `list` onto the list of contours whose newest is `*last`.
fn push_contour(m: &mut Mem, o: &mut dyn Outside, list: u32, last: &mut u32) {
    let c = call(m, o, SI_CREATE_CONTOUR, &[list]);
    if *last != 0 {
        w(m, c + 8, *last);
        w(m, *last + 4, c);
    }
    w(m, c + 4, 0);
    *last = c;
}

/// Puts `face` after the head of the face list `*pl` (or makes it the head).
fn list_after_head(m: &mut Mem, pl: u32, face: u32) {
    let head = r(m, pl);
    if head == 0 {
        w(m, pl, face);
        return;
    }
    let nx = r(m, head + f::NEXT);
    w(m, face + f::NEXT, nx);
    w(m, face + f::PREV, head);
    w(m, head + f::NEXT, face);
    let nx = r(m, face + f::NEXT);
    if nx != 0 {
        w(m, nx + f::PREV, face);
    }
}

/// The face box (min/max at `+0x20`/`+0x38`) contains the contour box `mn`/`mx`, within 1e-6 (80-bit differences).
fn box_contains(m: &Mem, face: u32, mn: u32, mx: u32) -> bool {
    let eps = x(1e-6); // the double at 0x08657c30 (read-only data)
    for ax in 0..3u32 {
        let fl = d(m, face + 0x20 + ax * 8);
        let lo = d(m, mn + ax * 8);
        if !(fl.lt(lo) || (lo - fl).abs().lt(eps)) {
            return false;
        }
    }
    for ax in 0..3u32 {
        let fh = d(m, face + 0x38 + ax * 8);
        let hi = d(m, mx + ax * 8);
        if !(hi.lt(fh) || (hi - fh).abs().lt(eps)) {
            return false;
        }
    }
    true
}

/// The end of a walk that would run off a list in the original (a null pointer read faults there).
const GUARD: u32 = 1 << 20;

/// `si_splitFace(tree, face, pback, pfront)` [soft 0x08313b74 si_splitFace]: cuts `face` by the plane of the tree
/// node. Each contour is walked twice: first every edge crossing the plane is split at the plane (`splitEdge`; a
/// point that lies within the tolerance of an end is marked on the plane, side 1), then the edges are taken apart
/// and sorted into runs by side (back 2, on 1, front 3), the points where the contour meets the plane going into
/// the inter list (`addInterNode`, sorted along the plane by the key); a contour that never crosses the plane
/// becomes a contour of its own. The inter list is then paired up (the 0x24/0x30 triples of its nodes) and every
/// closed loop becomes a new face (`si_createBoolFace`) put into `*pback` (status 3) or `*pfront` (anything else);
/// leftover contours are attached to a new face whose box contains them as an inner contour (or merged into it
/// where they share a point), or become faces of their own (`buildFace`). Without any contour the face is left
/// whole: `verifyEdge` is run on the edges of the inter list when `classifying` is set; at the end `resetFace(face)`.
pub fn si_split_face(m: &mut Mem, o: &mut dyn Outside, tree: u32, face: u32, pback: u32, pfront: u32) {
    let fr = m.local(FRAME);
    si_split_face_in(m, o, tree, face, pback, pfront, fr);
    m.release(FRAME);
}

fn si_split_face_in(m: &mut Mem, o: &mut dyn Outside, tree: u32, face: u32, pback: u32, pfront: u32, fr: u32) {
    let flag = fr + FLAG;
    let pfirst = fr + PFIRST;
    let phead = fr + PHEAD;
    let vec = fr + VEC;
    for k in (0..FRAME).step_by(4) {
        w(m, fr + k, 0);
    }
    let mut local_74: i32 = 1;
    w(m, pback, 0);
    w(m, pfront, 0);
    w(m, phead, 0);
    w(m, pfirst, 0);
    let mut local_6c: u32 = 0; // newest of the loose contours
    m.set_u8(flag, 1);
    let mut cont = r(m, face + f::CONTOURS);
    #[allow(unused_assignments)]
    let mut u5: u32 = 0;
    while cont != 0 {
        let mut n84: u32 = 0;
        let mut n88 = get_inter_list_node(m, o, flag);
        m.set_i32(n88 + 0x30, local_74);
        let mut n8c = get_inter_list_node(m, o, flag);
        m.set_i32(n8c + 0x24, local_74);
        local_74 += 1;
        let mut node = r(m, cont);
        let mut p7c = node_pt(m, node);
        let mut s80 = get_pt_pos(m, tree, p7c) as u32;
        // pass 1: split the edges that cross the plane
        loop {
            let cur = node;
            let p3 = far_pt(m, cur);
            u5 = get_pt_pos(m, tree, p3) as u32;
            if (u5 == 2 && s80 == 3) || (u5 == 3 && s80 == 2) {
                let dist = m.f64(p3 + p::DIST);
                let p6 = split_edge(m, o, tree, dist, p7c, p3);
                if p6 == p7c {
                    on_plane(m, p7c);
                    u5 = 1;
                } else if p6 == p3 {
                    on_plane(m, p3);
                    u5 = 1;
                }
            }
            let nx = r(m, cur + n::NEXT);
            let head = r(m, cont);
            s80 = u5;
            p7c = p3;
            node = nx;
            if nx == head {
                break;
            }
        }
        let mut n78 = r(m, cont);
        p7c = node_pt(m, n78);
        s80 = m.u8(p7c + p::SIDE) as i8 as i32 as u32;
        // pass 2: take the edge nodes apart and sort them
        loop {
            let nx = r(m, n78 + n::NEXT);
            let mut n98 = 0u32;
            if n78 != nx {
                let pv = r(m, n78 + n::PREV);
                w(m, pv + n::NEXT, nx);
                let nn = r(m, n78 + n::NEXT);
                w(m, nn + n::PREV, pv);
                n98 = nx;
            }
            w(m, n78 + n::PREV, n78);
            w(m, n78 + n::NEXT, n78);
            let p3 = far_pt(m, n78);
            u5 = m.u8(p3 + p::SIDE) as i8 as i32 as u32;
            let mut n9c = n78;
            let end78 = m.u8(n78 + n::END) as u32;
            if u5 == 2 {
                if s80 == 3 {
                    let dist = m.f64(p3 + p::DIST);
                    let p6 = split_edge(m, o, tree, dist, p7c, p3);
                    let nn = call(m, o, SI_CREATE_EDGE_NODE, &[]);
                    let e78 = r(m, n78 + n::EDGE);
                    let ne = add_pt_to_edge(m, o, e78, p6, face);
                    w(m, nn + n::EDGE, ne);
                    m.set_u8(nn + n::END, m.u8(n78 + n::END));
                    n9c = nn;
                    if r(m, ne + e::PT) != p3 && r(m, ne + e::PT + 4) != p3 {
                        n9c = n78;
                        n78 = nn;
                    }
                    w(m, n9c + n::NEXT, n9c);
                    w(m, n9c + n::PREV, n9c);
                    w(m, n78 + n::NEXT, n78);
                    w(m, n78 + n::PREV, n78);
                    add_edge_to_inter(m, n88, n8c, n78);
                    if r(m, n88) == p6 {
                        push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
                        w(m, n88 + 0x38, 0);
                        w(m, n88 + 0x34, 2);
                        w(m, n8c + 0x2c, 0);
                        w(m, n8c + 0x28, 2);
                        if n84 == n88 {
                            n84 = 0;
                        }
                    } else {
                        if n84 == 0 {
                            n84 = n88;
                            w(m, n88 + 0x34, 3);
                            w(m, n8c + 0x28, 3);
                        } else {
                            add_inter_node(m, pfirst, phead, n88, vec);
                        }
                        n88 = n8c;
                        let n19 = get_inter_list_node(m, o, flag);
                        w(m, n8c, p6);
                        let ee = r(m, n9c + n::EDGE);
                        m.copy(n8c + 4, ee + 0x18 + m.u8(n9c + n::END) as u32 * 0x48, 0x18);
                        w(m, n8c + 0x34, 2);
                        m.set_i32(n8c + 0x30, local_74);
                        w(m, n19 + 0x28, 2);
                        m.set_i32(n19 + 0x24, local_74);
                        local_74 += 1;
                        n8c = n19;
                    }
                } else if s80 == 1 {
                    w(m, n88 + 0x34, 2);
                    w(m, n8c + 0x28, 2);
                    let ee = r(m, n78 + n::EDGE);
                    m.copy(n88 + 4, ee + 0x18 + end78 * 0x48, 0x18);
                }
                add_edge_to_inter(m, n88, n8c, n9c);
            } else if u5 < 3 {
                if u5 == 1 {
                    add_edge_to_inter(m, n88, n8c, n78);
                    if r(m, n88) == p3 {
                        push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
                        w(m, n88 + 0x38, 0);
                        w(m, n88 + 0x34, 1);
                        w(m, n8c + 0x2c, 0);
                        w(m, n8c + 0x28, 1);
                        if n84 == n88 {
                            n84 = 0;
                        }
                    } else {
                        if n84 == 0 {
                            n84 = n88;
                            w(m, n88 + 0x34, s80);
                            w(m, n8c + 0x28, s80);
                        } else {
                            add_inter_node(m, pfirst, phead, n88, vec);
                        }
                        n88 = n8c;
                        let n19 = get_inter_list_node(m, o, flag);
                        w(m, n8c, p3);
                        let ee = r(m, n78 + n::EDGE);
                        m.copy(n8c + 4, ee + end78 * 0x18 + 0x30, 0x18);
                        w(m, n8c + 0x34, 1);
                        m.set_i32(n8c + 0x30, local_74);
                        w(m, n19 + 0x28, 1);
                        m.set_i32(n19 + 0x24, local_74);
                        local_74 += 1;
                        n8c = n19;
                    }
                }
            } else if u5 == 3 {
                if s80 == 2 {
                    let dist = m.f64(p3 + p::DIST);
                    let p6 = split_edge(m, o, tree, dist, p7c, p3);
                    n9c = call(m, o, SI_CREATE_EDGE_NODE, &[]);
                    let e78 = r(m, n78 + n::EDGE);
                    let ne = add_pt_to_edge(m, o, e78, p6, face);
                    w(m, n9c + n::EDGE, ne);
                    m.set_u8(n9c + n::END, m.u8(n78 + n::END));
                    let mut p7 = n78;
                    if r(m, ne + e::PT) != p3 && r(m, ne + e::PT + 4) != p3 {
                        p7 = n9c;
                        n9c = n78;
                    }
                    w(m, p7 + n::NEXT, p7);
                    w(m, p7 + n::PREV, p7);
                    w(m, n9c + n::NEXT, n9c);
                    w(m, n9c + n::PREV, n9c);
                    add_edge_to_inter(m, n88, n8c, p7);
                    if r(m, n88) == p6 {
                        push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
                        w(m, n88 + 0x38, 0);
                        w(m, n88 + 0x34, 3);
                        w(m, n8c + 0x2c, 0);
                        w(m, n8c + 0x28, 3);
                        if n84 == n88 {
                            n84 = 0;
                        }
                    } else {
                        if n84 == 0 {
                            n84 = n88;
                            w(m, n88 + 0x34, 2);
                            w(m, n8c + 0x28, 2);
                        } else {
                            add_inter_node(m, pfirst, phead, n88, vec);
                        }
                        n88 = n8c;
                        let n19 = get_inter_list_node(m, o, flag);
                        w(m, n8c, p6);
                        let ee = r(m, n9c + n::EDGE);
                        m.copy(n8c + 4, ee + 0x18 + m.u8(n9c + n::END) as u32 * 0x48, 0x18);
                        w(m, n8c + 0x34, 3);
                        m.set_i32(n8c + 0x30, local_74);
                        w(m, n19 + 0x28, 3);
                        m.set_i32(n19 + 0x24, local_74);
                        local_74 += 1;
                        n8c = n19;
                    }
                } else if s80 == 1 {
                    w(m, n88 + 0x34, 3);
                    w(m, n8c + 0x28, 3);
                    let ee = r(m, n78 + n::EDGE);
                    m.copy(n88 + 4, ee + 0x18 + end78 * 0x48, 0x18);
                }
                add_edge_to_inter(m, n88, n8c, n9c);
            }
            n78 = n98;
            s80 = u5;
            p7c = p3;
            if n98 == 0 {
                break;
            }
        }
        // close the contour's run
        let join = |m: &mut Mem, by_empty: bool| {
            let take = if by_empty { r(m, n88 + 0x38) == 0 } else { u5 == 1 };
            if take {
                for k in [0x30, 0x34, 0x38] {
                    let v = r(m, n84 + k);
                    w(m, n88 + k, v);
                }
            } else {
                let t8c = r(m, n8c + 0x2c);
                let l84 = r(m, n84 + 0x38);
                w(m, t8c + 0x10, l84);
                let l88 = r(m, n88 + 0x38);
                let l84p = r(m, l84 + 0xc);
                w(m, l88 + 0xc, l84p);
                w(m, l84p + 0x10, l88);
                w(m, l84 + 0xc, t8c);
                let v = r(m, n84 + 0x30);
                w(m, n88 + 0x30, v);
            }
        };
        if n84 == 0 {
            push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
        } else if r(m, pfirst) == 0 {
            join(m, true);
            push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
        } else {
            join(m, false);
            let first = r(m, pfirst);
            if r(m, first) == r(m, n88) {
                for k in [0x24, 0x28, 0x2c] {
                    let v = r(m, n88 + k);
                    w(m, first + k, v);
                }
                push_contour(m, o, r(m, n88 + 0x38), &mut local_6c);
            } else {
                add_inter_node(m, pfirst, phead, n88, vec);
            }
        }
        cont = r(m, cont + c::NEXT);
    }
    sort_and_build(m, o, tree, face, pback, pfront, fr, local_74, local_6c);
}

/// The second half of `si_splitFace`: sorts the inter list into pairs, builds the faces of the closed loops and
/// places the loose contours.
fn sort_and_build(m: &mut Mem, o: &mut dyn Outside, tree: u32, face: u32, pback: u32, pfront: u32, fr: u32, _next_id: i32, mut local_6c: u32) {
    let flag = fr + FLAG;
    let phead = fr + PHEAD;
    let plist = fr + PLIST;
    let pout = fr + POUT;
    let pstat = fr + PSTAT;
    let (mn, mx) = (fr + MN, fr + MX);
    w(m, plist, 0);
    let mut local_70 = 0u32;
    let mut idx: i32 = 0;
    let mut p8 = local_70;
    let mut p20 = r(m, phead);
    let mut guard = 0u32;
    while p20 != 0 {
        guard += 1;
        if guard > GUARD {
            return;
        }
        let pt = r(m, p20);
        for off in [0x38u32, 0x2c] {
            let start = r(m, p20 + off);
            let mut it = start;
            if start != 0 {
                let mut g = 0;
                loop {
                    let ed = r(m, it + n::EDGE);
                    if r(m, ed + e::PT) == pt || r(m, ed + e::PT + 4) == pt {
                        break;
                    }
                    it = r(m, it + n::NEXT);
                    g += 1;
                    if it == start || g > GUARD {
                        break;
                    }
                }
                w(m, p20 + off, it);
            }
        }
        local_70 = p8;
        if m.i32(p20 + 0x34) == 1 {
            let n19 = r(m, p20 + 0x3c);
            let mut i14 = 0i32;
            let head = r(m, phead);
            let mut a8 = head;
            let mut i15 = m.i32(head + 0x24);
            while i15 != m.i32(p20 + 0x30) {
                a8 = r(m, a8 + 0x40);
                i14 += 1;
                if a8 == 0 {
                    return; // the original reads through a null pointer here
                }
                i15 = m.i32(a8 + 0x24);
            }
            let p9 = r(m, p20 + 0x40);
            if a8 != n19 && a8 != p9 {
                let mut p9 = p9;
                if n19 != 0 {
                    w(m, n19 + 0x40, p9);
                    p9 = r(m, phead);
                }
                w(m, phead, p9);
                let nx = r(m, p20 + 0x40);
                if nx != 0 {
                    let pv = r(m, p20 + 0x3c);
                    w(m, nx + 0x3c, pv);
                }
                if idx < i14 {
                    w(m, p20 + 0x40, a8);
                    let pv = r(m, a8 + 0x3c);
                    w(m, p20 + 0x3c, pv);
                    w(m, a8 + 0x3c, p20);
                    if pv != 0 {
                        w(m, pv + 0x40, p20);
                    }
                } else {
                    w(m, p20 + 0x3c, a8);
                    let nx = r(m, a8 + 0x40);
                    w(m, p20 + 0x40, nx);
                    w(m, a8 + 0x40, p20);
                    if nx != 0 {
                        w(m, nx + 0x3c, p20);
                    }
                }
            }
            let p9 = get_inter_list_node(m, o, flag);
            local_70 = get_inter_list_node(m, o, flag);
            for k in [0x30u32, 0x34, 0x38] {
                let v = r(m, p20 + k);
                w(m, p9 + k, v);
            }
            for (dst, src) in [(0x24u32, 0x24u32), (0x28, 0x28), (0x2c, 0x2c)] {
                let v = r(m, a8 + src);
                w(m, local_70 + dst, v);
            }
            w(m, p20 + 0x38, 0);
            m.set_i32(p20 + 0x34, -1);
            m.set_i32(p20 + 0x30, -1);
            w(m, a8 + 0x2c, 0);
            m.set_i32(a8 + 0x28, -1);
            m.set_i32(a8 + 0x24, -1);
            let mut p19 = p20;
            // `<=` through fcomp / and ah,0x45 / jne: also taken when unordered
            if !(m.f64(a8 + 0x1c) > m.f64(p20 + 0x1c)) {
                let l = r(m, p9 + 0x38);
                m.set_u8(l + 4, 1u8.wrapping_sub(m.u8(l + 4)));
                p19 = a8;
                a8 = p20;
            }
            w(m, p9, r(m, p19));
            m.copy(p9 + 0x1c, p19 + 0x1c, 8);
            m.copy(p9 + 4, p19 + 4, 0x18);
            w(m, local_70, r(m, a8));
            m.copy(local_70 + 0x1c, a8 + 0x1c, 8);
            m.copy(local_70 + 4, a8 + 4, 0x18);
            w(m, p9 + 0x40, local_70);
            w(m, local_70 + 0x3c, p9);
            let mut p19 = p9;
            if p8 != 0 {
                w(m, p8 + 0x40, p9);
                p19 = r(m, plist);
            }
            w(m, plist, p19);
            w(m, p9 + 0x3c, p8);
        }
        if m.i32(p20 + 0x34) != -1 && m.i32(p20 + 0x34) == m.i32(p20 + 0x28) {
            let t2c = r(m, p20 + 0x2c);
            let t38 = r(m, p20 + 0x38);
            if r(m, t2c + 0x10) == t38 {
                let nx = r(m, t2c + 0x10);
                let c = call(m, o, SI_CREATE_CONTOUR, &[nx]);
                if local_6c != 0 {
                    w(m, c + 8, local_6c);
                    w(m, local_6c + 4, c);
                }
                w(m, c + 4, 0);
                local_6c = c;
            } else {
                let head = r(m, phead);
                let mut p19 = head;
                let mut i15 = m.i32(head + 0x24);
                while i15 != m.i32(p20 + 0x30) {
                    p19 = r(m, p19 + 0x40);
                    if p19 == 0 {
                        return;
                    }
                    i15 = m.i32(p19 + 0x24);
                }
                let v = r(m, p20 + 0x24);
                w(m, p19 + 0x24, v);
                let a_prev = r(m, t38 + 0xc);
                let b_next = r(m, t2c + 0x10);
                w(m, a_prev + 0x10, b_next);
                w(m, b_next + 0xc, a_prev);
                w(m, t38 + 0xc, t2c);
                w(m, t2c + 0x10, t38);
            }
            let p8b = r(m, p20 + 0x3c);
            w(m, p20 + 0x38, 0);
            m.set_i32(p20 + 0x34, -1);
            m.set_i32(p20 + 0x30, -1);
            w(m, p20 + 0x2c, 0);
            m.set_i32(p20 + 0x28, -1);
            m.set_i32(p20 + 0x24, -1);
            if p8b == 0 || r(m, p8b) != r(m, p20) {
                let p8c = r(m, p20 + 0x40);
                if p8c != 0 && r(m, p20) == r(m, p8c) {
                    let pv = r(m, p20 + 0x3c);
                    w(m, p8c + 0x3c, pv);
                    if pv == 0 {
                        let nx = r(m, p20 + 0x40);
                        w(m, phead, nx);
                    } else {
                        let nx = r(m, p20 + 0x40);
                        w(m, pv + 0x40, nx);
                    }
                }
            } else {
                let nx = r(m, p20 + 0x40);
                w(m, p8b + 0x40, nx);
                if nx != 0 {
                    let pv = r(m, p20 + 0x3c);
                    w(m, nx + 0x3c, pv);
                }
            }
        }
        idx += 1;
        p8 = local_70;
        p20 = r(m, p20 + 0x40);
    }
    // build a face from each closed loop
    let mut outer = 0u32;
    while r(m, phead) != 0 {
        outer += 1;
        if outer > GUARD {
            return;
        }
        let pc = call(m, o, SI_CREATE_BOOL_FACE, &[face]);
        let mut b4 = r(m, phead);
        let bvar4: bool;
        let i11: i32;
        let bc: i32;
        loop {
            if b4 == 0 {
                return; // null read in the original
            }
            if r(m, b4 + 0x38) != 0 {
                bvar4 = true;
                i11 = m.i32(b4 + 0x30);
                bc = m.i32(b4 + 0x34);
                let c = call(m, o, SI_CREATE_CONTOUR, &[r(m, b4 + 0x38)]);
                w(m, pc + f::CONTOURS, c);
                m.set_i32(b4 + 0x30, i11.wrapping_neg());
                break;
            }
            if r(m, b4 + 0x2c) != 0 {
                bvar4 = false;
                i11 = m.i32(b4 + 0x24);
                bc = m.i32(b4 + 0x28);
                let c = call(m, o, SI_CREATE_CONTOUR, &[r(m, b4 + 0x2c)]);
                w(m, pc + f::CONTOURS, c);
                m.set_i32(b4 + 0x24, i11.wrapping_neg());
                break;
            }
            b4 = r(m, b4 + 0x40);
        }
        let mut b8 = r(m, phead);
        if b8 != 0 {
            let mut i15 = m.i32(b8 + 0x30);
            let mut skip = false;
            if i11 != i15 {
                let mut i14 = m.i32(b8 + 0x24);
                while i11 != i14 {
                    b8 = r(m, b8 + 0x40);
                    if b8 == 0 {
                        skip = true;
                        break;
                    }
                    i15 = m.i32(b8 + 0x30);
                    if i11 == i15 {
                        break;
                    }
                    i14 = m.i32(b8 + 0x24);
                }
            }
            if !skip && b8 != 0 {
                if i11 == i15 {
                    m.set_i32(b8 + 0x30, i11.wrapping_neg());
                } else {
                    m.set_i32(b8 + 0x24, i11.wrapping_neg());
                }
            }
        }
        w(m, pout, b4);
        let mut p20 = b4;
        'walk: while p20 != 0 && p20 != b8 {
            let mut p19 = r(m, p20 + 0x40);
            if p19 != 0 {
                let mut cnt = 0;
                loop {
                    let p9 = r(m, pout);
                    cnt += 1;
                    let nx = r(m, p20 + 0x40);
                    let ed = find_on_edge(m, o, p9, nx, plist, face, pout);
                    if r(m, pout) == p9 || cnt == 200 {
                        let nx = r(m, p20 + 0x40);
                        w(m, pout, nx);
                    }
                    let nn = call(m, o, SI_CREATE_EDGE_NODE, &[]);
                    w(m, nn + n::EDGE, r(m, ed + n::EDGE));
                    if bvar4 {
                        m.set_u8(nn + n::END, 1u8.wrapping_sub(m.u8(ed + n::END)));
                    } else {
                        m.set_u8(nn + n::END, m.u8(ed + n::END));
                    }
                    let con = r(m, pc + f::CONTOURS);
                    let first = r(m, con);
                    let e_nn = r(m, nn + n::EDGE);
                    let far = r(m, e_nn + e::PT + (1 - m.u8(nn + n::END) as u32) * 4);
                    let fe = r(m, first + n::EDGE);
                    let fpt = r(m, fe + e::PT + m.u8(first + n::END) as u32 * 4);
                    let u10;
                    if far == fpt {
                        let pv = r(m, first + n::PREV);
                        w(m, nn + n::PREV, pv);
                        u10 = r(m, r(m, pc + f::CONTOURS));
                    } else {
                        w(m, nn + n::PREV, first);
                        u10 = r(m, first + n::NEXT);
                    }
                    w(m, nn + n::NEXT, u10);
                    let pv = r(m, nn + n::PREV);
                    w(m, pv + n::NEXT, nn);
                    let nx2 = r(m, nn + n::NEXT);
                    w(m, nx2 + n::PREV, nn);
                    let con = r(m, pc + f::CONTOURS);
                    w(m, con, nn);
                    let lastp = r(m, pout);
                    let nxn = r(m, p20 + 0x40);
                    if r(m, lastp) == r(m, nxn) {
                        break;
                    }
                    if cnt > GUARD {
                        return;
                    }
                }
            }
            loop {
                if p19 == 0 {
                    break 'walk;
                }
                p20 = p19;
                let b34 = m.i32(p19 + 0x34);
                let b28;
                if p19 == b4 || p19 == b8 {
                    break;
                }
                b28 = m.i32(p19 + 0x28);
                let p1 = b34 != bc || m.i32(p19 + 0x30) < 1;
                let p2 = b28 != bc || m.i32(p19 + 0x24) < 1;
                if p1 && p2 {
                    break;
                }
                let id;
                let a;
                if b28 == bc {
                    id = m.i32(p19 + 0x24);
                    a = r(m, p19 + 0x2c);
                    m.set_i32(p19 + 0x24, id.wrapping_neg());
                } else {
                    id = m.i32(p19 + 0x30);
                    a = r(m, p19 + 0x38);
                    m.set_i32(p19 + 0x30, id.wrapping_neg());
                }
                let to_b96;
                if id == m.i32(p19 + 0x30) {
                    to_b96 = true;
                } else {
                    let mut i14 = m.i32(p19 + 0x24);
                    let mut found = false;
                    let mut g = 0;
                    while id != i14 {
                        p19 = r(m, p19 + 0x40);
                        if p19 == 0 {
                            return;
                        }
                        if id == m.i32(p19 + 0x30) {
                            found = true;
                            break;
                        }
                        i14 = m.i32(p19 + 0x24);
                        g += 1;
                        if g > GUARD {
                            return;
                        }
                    }
                    to_b96 = found || id == m.i32(p19 + 0x30);
                }
                let b;
                if to_b96 {
                    b = r(m, p19 + 0x38);
                    let v = m.i32(p19 + 0x30);
                    m.set_i32(p19 + 0x30, v.wrapping_neg());
                } else {
                    b = r(m, p19 + 0x2c);
                    let v = m.i32(p19 + 0x24);
                    m.set_i32(p19 + 0x24, v.wrapping_neg());
                }
                let con = r(m, pc + f::CONTOURS);
                let first = r(m, con);
                if bvar4 {
                    w(m, a + n::NEXT, first);
                    let fp = r(m, first + n::PREV);
                    w(m, b + n::PREV, fp);
                    let an = r(m, a + n::NEXT);
                    w(m, an + n::PREV, a);
                    let bp = r(m, b + n::PREV);
                    w(m, bp + n::NEXT, b);
                } else {
                    w(m, a + n::PREV, first);
                    let fnx = r(m, first + n::NEXT);
                    w(m, b + n::NEXT, fnx);
                    let ap = r(m, a + n::PREV);
                    w(m, ap + n::NEXT, a);
                    let bn = r(m, b + n::NEXT);
                    w(m, bn + n::PREV, b);
                }
                let con = r(m, pc + f::CONTOURS);
                w(m, con, b);
                w(m, pout, p19);
            }
        }
        // LAB_08314c5c
        if bc == 3 {
            list_after_head(m, pback, pc);
        } else {
            list_after_head(m, pfront, pc);
        }
        let con = r(m, pc + f::CONTOURS);
        let first = r(m, con);
        let mut it = first;
        let mut g = 0;
        loop {
            let ed = r(m, it + n::EDGE);
            w(m, ed + e::FACE + m.u8(it + n::END) as u32 * 4, pc);
            it = r(m, it + n::NEXT);
            g += 1;
            if it == r(m, r(m, pc + f::CONTOURS)) || g > GUARD {
                break;
            }
        }
        if it != 0 {
            let first = r(m, r(m, pc + f::CONTOURS));
            compute_face_size(m, first, pc + f::MIN, pc + f::MAX, true);
        }
        if b4 == 0 || b8 == 0 {
            w(m, phead, 0);
        } else {
            let stop = r(m, b8 + 0x40);
            let mut cur = b4;
            let mut g = 0;
            loop {
                let nx = r(m, cur + 0x40);
                if m.i32(cur + 0x30) < 0 {
                    w(m, cur + 0x38, 0);
                    m.set_i32(cur + 0x30, -1);
                }
                if m.i32(cur + 0x24) < 0 {
                    w(m, cur + 0x2c, 0);
                    m.set_i32(cur + 0x24, -1);
                }
                if r(m, cur + 0x38) == 0 && r(m, cur + 0x2c) == 0 {
                    let pv = r(m, cur + 0x3c);
                    if pv == 0 {
                        let nx0 = r(m, cur + 0x40);
                        w(m, phead, nx0);
                    } else {
                        let nx0 = r(m, cur + 0x40);
                        w(m, pv + 0x40, nx0);
                    }
                    let nx0 = r(m, cur + 0x40);
                    if nx0 != 0 {
                        let pv = r(m, cur + 0x3c);
                        w(m, nx0 + 0x3c, pv);
                    }
                    w(m, cur + 0x40, 0);
                    w(m, cur + 0x3c, 0);
                }
                cur = nx;
                g += 1;
                if cur == stop || g > GUARD {
                    break;
                }
            }
        }
        loop {
            let h = r(m, phead);
            if h == 0 {
                break;
            }
            if m.i32(h + 0x34) != -1 || m.i32(h + 0x28) != -1 {
                break;
            }
            let nh = r(m, h + 0x40);
            w(m, phead, nh);
            if nh == 0 {
                break;
            }
            let pv = r(m, nh + 0x3c);
            w(m, pv + 0x40, 0);
            w(m, nh + 0x3c, 0);
        }
    }
    // the loose contours
    loop {
        let p8 = local_6c;
        if p8 == 0 {
            if m.u8(CLASSIFYING.addr()) != 0 {
                let mut p20 = r(m, plist);
                let mut g = 0;
                while p20 != 0 {
                    let ed = if r(m, p20 + 0x38) == 0 {
                        r(m, r(m, p20 + 0x2c) + n::EDGE)
                    } else {
                        r(m, r(m, p20 + 0x38) + n::EDGE)
                    };
                    verify_edge(m, o, tree, ed);
                    let nx = r(m, p20 + 0x40);
                    p20 = r(m, nx + 0x40);
                    g += 1;
                    if g > GUARD {
                        break;
                    }
                }
            }
            reset_face(m, face);
            return;
        }
        let mut attached = false;
        local_6c = r(m, p8 + c::NEXT);
        w(m, p8 + 4, 0);
        w(m, p8 + 8, 0);
        if r(m, p8) != 0 {
            let first = r(m, p8);
            compute_face_size(m, first, mn, mx, true);
        }
        let mut pass = 0;
        'passes: loop {
            let mut pf = if pass == 0 { r(m, pback) } else { r(m, pfront) };
            pass += 1;
            while pf != 0 {
                if box_contains(m, pf, mn, mx) {
                    let p22 = r(m, pf + f::CONTOURS);
                    let mut i15 = r(m, p22);
                    let mut spliced = false;
                    let mut g = 0;
                    loop {
                        let first = r(m, p8);
                        let mut i14 = first;
                        let mut hit = false;
                        loop {
                            if node_pt(m, i15) == node_pt(m, i14) {
                                hit = true;
                                break;
                            }
                            i14 = r(m, i14 + n::NEXT);
                            if i14 == first {
                                break;
                            }
                        }
                        if hit {
                            let i16 = r(m, i15 + n::PREV);
                            w(m, i16 + n::NEXT, i14);
                            let p14 = r(m, i14 + n::PREV);
                            w(m, p14 + n::NEXT, i15);
                            w(m, i15 + n::PREV, p14);
                            w(m, i14 + n::PREV, i16);
                            spliced = true;
                            break;
                        }
                        i15 = r(m, i15 + n::NEXT);
                        g += 1;
                        if i15 == r(m, p22) || g > GUARD {
                            break;
                        }
                    }
                    if !spliced {
                        let nx = r(m, p22 + 8);
                        w(m, p8 + 8, nx);
                        let c = r(m, pf + f::CONTOURS);
                        w(m, p8 + 4, c);
                        w(m, c + 8, p8);
                        let nx = r(m, p8 + 8);
                        if nx != 0 {
                            w(m, nx + 4, p8);
                        }
                    }
                    let mut it = r(m, p8);
                    let stamp = next_visit(m);
                    let mut g = 0;
                    loop {
                        w(m, it, stamp);
                        let ed = r(m, it + n::EDGE);
                        w(m, ed + e::FACE + m.u8(it + n::END) as u32 * 4, pf);
                        it = r(m, it + n::NEXT);
                        g += 1;
                        if r(m, it) == stamp || g > GUARD {
                            break;
                        }
                    }
                    attached = true;
                    break 'passes;
                }
                pf = r(m, pf + f::NEXT);
            }
            if pass > 1 {
                break;
            }
        }
        if attached {
            continue;
        }
        let first = r(m, p8);
        let nf = build_face(m, o, face, first, pstat);
        let st = m.i32(pstat);
        let pl = if st == 3 { pback } else { pfront };
        list_after_head(m, pl, nf);
    }
}
