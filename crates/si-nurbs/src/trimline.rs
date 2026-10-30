// SPDX-License-Identifier: MIT
//! The trim-line code of `trimLineOp.c` (package 15, "NURBS trimming: curves on surfaces";
//! finding 0025, section 14), on the memory image.
//!
//! A trim line (`SI_trimLine`) is a record with the count of nodes at +0, the sign (turning
//! direction, negated by a reversal) at +4, the first node at +8, a cursor node at +0xc and
//! four doubles (the box) at +0x10. A node (`SI_trimData`, 0x18 bytes) holds a point of the
//! surface's parameter plane (+0 u, +8 v), a mark (+0x10, an int: 0 free, other values pair
//! the crossing nodes of two lines) and the link (+0x14). The globals `uEps` (0x08822218) to
//! `vEgInc` (0x08822250) are the range of the surface being trimmed.
//!
//! Every float expression is evaluated on the 80-bit register ([`F80`]); a store to memory
//! rounds to `double`. Routines not yet ported (`si_delTrimLine`, `si_trmDataIns`,
//! `SI_trmCvrRve`, `si_trmCrvBBox`, ...) go through [`Outside`] and are answered 0 until
//! their own files are ported.

use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

// --- the record fields -----------------------------------------------------------------
const N_U: u32 = 0;
const N_V: u32 = 8;
const N_FLAG: u32 = 0x10;
const N_NEXT: u32 = 0x14;
const L_COUNT: u32 = 0;
const L_SIGN: u32 = 4;
const L_HEAD: u32 = 8;
const L_CUR: u32 = 0xc;

// --- the globals -----------------------------------------------------------------------
pub const U_EPS: u32 = 0x0882_2218;
pub const V_EPS: u32 = 0x0882_2220;
pub const U_S: u32 = 0x0882_2228;
pub const U_E: u32 = 0x0882_2230;
pub const V_S: u32 = 0x0882_2238;
pub const V_E: u32 = 0x0882_2240;

// --- the constants in the data section ---------------------------------------------------
const C_1E4: f64 = 0.0001; // 0x08661a28
const C_1E5: f64 = 1e-5; // 0x08661a30 .. 0x08661a50 (copies)
const C_M1E5: f64 = -1e-5; // 0x08661a38, 0x08661a40
const C_1E6: f64 = 1e-6; // 0x08661a70, 0x08661a78

// --- the routines outside this file ------------------------------------------------------
pub const SI_DEL_TRIM_LINE: Routine = Routine::new("si_delTrimLine", 0x084f_769c);

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

fn g(m: &Mem, a: u32) -> F80 {
    x(m.f64(a))
}

/// `a > b`, ordered: the x87 compare with the condition bits (C3, C2, C0) all clear.
fn gt(a: F80, b: F80) -> bool {
    a.gt(b)
}

/// `a < b`, ordered: only C0 set.
fn lt(a: F80, b: F80) -> bool {
    a.lt(b)
}

/// [soft 0x084f0460 linerInterp]: `linerInterp(p1, p2, p3, p4, p5)`, the answer left in st0:
/// `base + num / den` with `num` and `den` chosen by whether `p1 < p3` and whether
/// `p2 > p4`, every step on the register.
pub fn liner_interp(p1: f64, p2: f64, p3: f64, p4: f64, p5: f64) -> F80 {
    let (p1, p2, p3, p4, p5) = (x(p1), x(p2), x(p3), x(p4), x(p5));
    let (num, base, den);
    if lt(p1, p3) {
        if !gt(p2, p4) {
            num = (p4 - p2) * p1;
            base = p2;
        } else {
            num = (p2 - p4) * (p5 - p3);
            base = p4;
        }
        den = (p5 - p3) + p1;
    } else {
        if !gt(p2, p4) {
            num = (p4 - p2) * (p5 - p1);
            base = p2;
        } else {
            num = (p2 - p4) * p3;
            base = p4;
        }
        den = (p5 - p1) + p3;
    }
    num / den + base
}

/// [soft 0x084f04d8 clearTLnFlag]: sets the mark (+0x10) of every node of the line to 0.
pub fn clear_tln_flag(m: &mut Mem, line: u32) {
    let mut n = m.u32(line.wrapping_add(L_HEAD));
    while n != 0 {
        m.set_i32(n.wrapping_add(N_FLAG), 0);
        n = m.u32(n.wrapping_add(N_NEXT));
    }
}

/// [soft 0x084f04f8 bboxesIntsect]: whether two boxes, the second widened by 0.0001, overlap:
/// 0 when `a3 - eps > a5`, `a1 + eps < a7`, `a0 - eps > a6` or `a2 + eps < a4`, else 1.
#[allow(clippy::too_many_arguments)]
pub fn bboxes_intsect(a0: f64, a1: f64, a2: f64, a3: f64, a4: f64, a5: f64, a6: f64, a7: f64) -> i32 {
    let eps = x(C_1E4);
    let (a0, a1, a2, a3, a4, a5, a6, a7) = (x(a0), x(a1), x(a2), x(a3), x(a4), x(a5), x(a6), x(a7));
    if gt(a3 - eps, a5) || lt(a1 + eps, a7) || gt(a0 - eps, a6) || lt(a2 + eps, a4) {
        0
    } else {
        1
    }
}

/// [soft 0x084f0ac8 atSameSide]: `atSameSide(a, b, c, p, q)` with two points (two doubles each)
/// at `p` and `q`: with `dp = a*p.x + b*p.y + c` and `dq` likewise, 1 when both are above
/// -1e-5 or both below 1e-5, else 0.
pub fn at_same_side(m: &Mem, a: f64, b: f64, c: f64, p: u32, q: u32) -> i32 {
    let (a, b, c) = (x(a), x(b), x(c));
    let dp = (a * g(m, p) + b * g(m, p.wrapping_add(8))) + c;
    let dq = (a * g(m, q) + b * g(m, q.wrapping_add(8))) + c;
    let (lo, hi) = (x(C_M1E5), x(C_1E5));
    if (gt(dp, lo) && gt(dq, lo)) || (gt(hi, dp) && gt(hi, dq)) {
        1
    } else {
        0
    }
}

/// [soft 0x084f0b58 nextNode]: the node after `node`, the second node of the line when
/// `node` is the last (the last repeats the first).
pub fn next_node(m: &Mem, first: u32, node: u32) -> u32 {
    let n = m.u32(node.wrapping_add(N_NEXT));
    if n != 0 {
        n
    } else {
        m.u32(first.wrapping_add(N_NEXT))
    }
}

/// [soft 0x084f0b70 newPt]: 0 when both `|a - c|` and `|b - d|` are below 1e-5, else 1.
pub fn new_pt(a: f64, b: f64, c: f64, d: f64) -> i32 {
    let eps = x(C_1E5);
    if gt(eps, (x(a) - x(c)).abs()) && lt((x(b) - x(d)).abs(), eps) {
        0
    } else {
        1
    }
}

/// [soft 0x084f2cc0 atEdge]: where the point (u, v) lies against the range edges: 0 inside,
/// 1 near u-start, 2 near u-end, 3 near v-start, 4 near v-end; in a corner the nearer
/// edge, by the distances scaled by the other direction's tolerance.
pub fn at_edge(m: &Mem, u: f64, v: f64) -> i32 {
    let (u, v) = (x(u), x(v));
    let (ue, ve) = (g(m, U_EPS), g(m, V_EPS));
    let d1 = u - g(m, U_S);
    let dv_s = v - g(m, V_S);
    let dv_e = g(m, V_E) - v;
    if !gt(ue, d1) {
        let d2 = g(m, U_E) - u;
        if !gt(ue, d2) {
            // u is not near an edge
            if gt(ve, dv_s) {
                return 3;
            }
            return if gt(ve, dv_e) { 4 } else { 0 };
        }
        // near the end of u
        if gt(ve, dv_s) {
            return if gt(d2 * ve, dv_s * ue) { 3 } else { 2 };
        }
        if !gt(ve, dv_e) {
            return 2;
        }
        return if gt(d2 * ve, dv_e * ue) { 4 } else { 2 };
    }
    // near the start of u
    if gt(ve, dv_s) {
        return if gt(d1 * ve, dv_s * ue) { 3 } else { 1 };
    }
    if !gt(ve, dv_e) {
        return 1;
    }
    if gt(d1 * ve, dv_e * ue) {
        4
    } else {
        1
    }
}

/// [soft 0x084f26b0 forceAtEdge]: the nearest of the four edges of the range to (u, v): 1 u-start,
/// 2 u-end, 3 v-start, 4 v-end.
pub fn force_at_edge(m: &Mem, u: f64, v: f64) -> i32 {
    let (u, v) = (x(u), x(v));
    let de = g(m, U_E) - u;
    let mut best = u - g(m, U_S);
    let mut r = 1;
    if !lt(best, de) {
        r = 2;
        best = de;
    }
    let d = v - g(m, V_S);
    if lt(d, best) {
        r = 3;
        best = d;
    }
    let d = g(m, V_E) - v;
    if lt(d, best) {
        r = 4;
    }
    r
}

/// [soft 0x084f4440 edgVal]: `edgVal(a, b, edge)`, the answer in st0: for edge 1 `b`; for 2
/// `((vE + uE) + vE) - b`; for 3 `(uE + ((vE + uE) + vE)) - a`; for 4 `a + vE`; else 0.
pub fn edg_val(m: &Mem, a: f64, b: f64, edge: i32) -> F80 {
    let (a, b) = (x(a), x(b));
    let (ue, ve) = (g(m, U_E), g(m, V_E));
    match edge {
        1 => b,
        2 => ((ve + ue) + ve) - b,
        3 => (ue + ((ve + ue) + ve)) - a,
        4 => a + ve,
        _ => x(0.0),
    }
}

/// [soft 0x084f1918 brdIntsct]: 1 when `a` is within 1e-6 of uS, `b` of uE, `c` of vS or `d` of vE.
pub fn brd_intsct(m: &Mem, a: f64, b: f64, c: f64, d: f64) -> i32 {
    let eps = x(C_1E6);
    let near = |v: f64, e: u32| gt(eps, (x(v) - g(m, e)).abs());
    if near(a, U_S) || near(b, U_E) || near(c, V_S) || near(d, V_E) {
        1
    } else {
        0
    }
}

/// [soft 0x084f159c getIntPt]: looks for the node marked `k`, first from the cursor (+0xc) to the
/// end, then from the first node; on a find the cursor is set there and 1 is returned, else 0.
pub fn get_int_pt(m: &mut Mem, line: u32, k: i32) -> i32 {
    let mut n = m.u32(line.wrapping_add(L_CUR));
    while n != 0 {
        if m.i32(n.wrapping_add(N_FLAG)) == k {
            m.set_u32(line.wrapping_add(L_CUR), n);
            return 1;
        }
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    let mut n = m.u32(line.wrapping_add(L_HEAD));
    while n != 0 {
        if m.i32(n.wrapping_add(N_FLAG)) == k {
            m.set_u32(line.wrapping_add(L_CUR), n);
            return 1;
        }
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    0
}

/// [soft 0x084f15e8 nxtIntsect]: the next crossing of two lines: for each node of `l1` marked
/// above 0, the node of `l2` with the same mark; when the node after either of them is
/// marked 0 or less the cursors are set to the pair and the answer is 1 if the cross product
/// of the two edges is positive, else -1; when both following nodes are marked, the two
/// marks are cleared. 0 when none is left.
pub fn nxt_intsect(m: &mut Mem, l1: u32, l2: u32) -> i32 {
    let mut n1 = m.u32(l1.wrapping_add(L_HEAD));
    while n1 != 0 {
        if 0 < m.i32(n1.wrapping_add(N_FLAG)) {
            let mut n2 = m.u32(l2.wrapping_add(L_HEAD));
            while n2 != 0 {
                if m.i32(n2.wrapping_add(N_FLAG)) == m.i32(n1.wrapping_add(N_FLAG)) {
                    let a = next_node(m, m.u32(l1.wrapping_add(L_HEAD)), n1);
                    let b = next_node(m, m.u32(l2.wrapping_add(L_HEAD)), n2);
                    if m.i32(a.wrapping_add(N_FLAG)) < 1 || m.i32(b.wrapping_add(N_FLAG)) < 1 {
                        m.set_u32(l1.wrapping_add(L_CUR), n1);
                        m.set_u32(l2.wrapping_add(L_CUR), n2);
                        let p = (g(m, b + N_V) - g(m, n2 + N_V)) * (g(m, a + N_U) - g(m, n1 + N_U));
                        let q = (g(m, a + N_V) - g(m, n1 + N_V)) * (g(m, b + N_U) - g(m, n2 + N_U));
                        return if lt(x(0.0), p - q) { 1 } else { -1 };
                    }
                    m.set_i32(n1.wrapping_add(N_FLAG), 0);
                    m.set_i32(n2.wrapping_add(N_FLAG), 0);
                    break;
                }
                n2 = m.u32(n2.wrapping_add(N_NEXT));
            }
        }
        n1 = m.u32(n1.wrapping_add(N_NEXT));
    }
    0
}

/// [soft 0x084f6240 resTrmCrv]: reverses the line: the sign negated, the list turned round (the
/// old first node becomes the last and the cursor), the first node the old last. The line
/// must have two nodes at least (the original reads the second without testing).
pub fn res_trm_crv(m: &mut Mem, line: u32) {
    let mut a = m.u32(line.wrapping_add(L_HEAD));
    let s = m.i32(line.wrapping_add(L_SIGN));
    m.set_i32(line.wrapping_add(L_SIGN), s.wrapping_neg());
    let mut b = m.u32(a.wrapping_add(N_NEXT));
    let mut c = m.u32(b.wrapping_add(N_NEXT));
    m.set_u32(a.wrapping_add(N_NEXT), 0);
    m.set_u32(b.wrapping_add(N_NEXT), a);
    m.set_u32(line.wrapping_add(L_CUR), a);
    while c != 0 {
        a = b;
        b = c;
        c = m.u32(b.wrapping_add(N_NEXT));
        m.set_u32(b.wrapping_add(N_NEXT), a);
    }
    m.set_u32(line.wrapping_add(L_HEAD), b);
}

/// [soft 0x084f7158 delTrimLine]: `delTrimLine(s)`: `s` holds at +0xc a list owner whose +8 is the
/// first node; frees every node in order, then the owner, and sets `s`+0xc to 0 (nothing is
/// done when it is 0).
pub fn del_trim_line(m: &mut Mem, s: u32) {
    let owner = m.u32(s.wrapping_add(0xc));
    if owner == 0 {
        return;
    }
    let mut n = m.u32(owner.wrapping_add(8));
    while n != 0 {
        let next = m.u32(n.wrapping_add(N_NEXT));
        m.free(n);
        n = next;
    }
    let owner = m.u32(s.wrapping_add(0xc));
    if owner != 0 {
        m.free(owner);
        m.set_u32(s.wrapping_add(0xc), 0);
    }
}

/// [soft 0x084f187c reorderTrimStat]: `reorderTrimStat(st)`: the status record has the count at
/// +0x24 and the table of line pointers at +0x114. A line with fewer than 3 nodes is deleted
/// (`si_delTrimLine`, through `o`) and its slot cleared; then the empty slots are closed up
/// and the count lowered.
pub fn reorder_trim_stat(m: &mut Mem, o: &mut dyn Outside, st: u32) {
    let slot = |m: &Mem, i: i32| m.u32(st.wrapping_add(0x114)).wrapping_add((i as u32).wrapping_mul(4));
    let mut i: i32 = 0;
    if 0 < m.i32(st.wrapping_add(0x24)) {
        loop {
            let line = m.u32(slot(m, i));
            if line != 0 && m.i32(line.wrapping_add(L_COUNT)) < 3 {
                o.call(m, SI_DEL_TRIM_LINE, &[line]);
                let s = slot(m, i);
                m.set_u32(s, 0);
            }
            i += 1;
            if i >= m.i32(st.wrapping_add(0x24)) {
                break;
            }
        }
    }
    let mut n = m.i32(st.wrapping_add(0x24));
    let mut i: i32 = 0;
    if 0 < n {
        loop {
            if m.u32(slot(m, i)) == 0 {
                m.set_i32(st.wrapping_add(0x24), n - 1);
                let mut j = i;
                if i < n - 1 {
                    loop {
                        let v = m.u32(slot(m, j + 1));
                        let s = slot(m, j);
                        m.set_u32(s, v);
                        j += 1;
                        if j >= m.i32(st.wrapping_add(0x24)) {
                            break;
                        }
                    }
                }
            } else {
                i += 1;
            }
            n = m.i32(st.wrapping_add(0x24));
            if i >= n {
                break;
            }
        }
    }
}

/// [soft 0x084f128c fillTheCoefs]: `fillTheCoefs(line, dv, du, c, a5, a6, a7, a8)`: for each edge
/// from node `a` (the first node, whose mark is cleared first) to its successor `b`:
/// `dv[i] = b.v - a.v` and `du[i] = a.u - b.u` (stored; the test uses the unrounded `du`);
/// an edge with `|dv| < 1e-6` (of the stored value) and `|du| < 1e-6` is removed (node `b`
/// unlinked and freed, the count lowered, `a` and `i` as they were); else
/// `c[i] = a.v*b.u - a.u*b.v`, the mark of `b` is cleared, and the u of the edge's ends goes
/// to `a5`/`a7` and the v to `a8`/`a6` by the sign of `du` and `dv`.
#[allow(clippy::too_many_arguments)]
pub fn fill_the_coefs(m: &mut Mem, line: u32, dv: u32, du: u32, c: u32, a5: u32, a6: u32, a7: u32, a8: u32) {
    let eps = x(C_1E6);
    let mut a = m.u32(line.wrapping_add(L_HEAD));
    let mut i: u32 = 0;
    m.set_i32(a.wrapping_add(N_FLAG), 0);
    let at = |base: u32, i: u32| base.wrapping_add(i.wrapping_mul(8));
    loop {
        let b = m.u32(a.wrapping_add(N_NEXT));
        if b == 0 {
            return;
        }
        let v80 = g(m, b + N_V) - g(m, a + N_V);
        m.set_f64(at(dv, i), v80.to_f64());
        let u80 = g(m, a + N_U) - g(m, b + N_U);
        m.set_f64(at(du, i), u80.to_f64());
        let dv64 = g(m, at(dv, i));
        if gt(eps, dv64.abs()) && lt(u80.abs(), eps) {
            let nn = m.u32(b.wrapping_add(N_NEXT));
            m.set_u32(a.wrapping_add(N_NEXT), nn);
            let cnt = m.i32(line.wrapping_add(L_COUNT));
            m.set_i32(line.wrapping_add(L_COUNT), cnt.wrapping_sub(1));
            m.free(b);
            continue;
        }
        let cc = g(m, a + N_V) * g(m, b + N_U) - g(m, a + N_U) * g(m, b + N_V);
        m.set_f64(at(c, i), cc.to_f64());
        m.set_i32(b.wrapping_add(N_FLAG), 0);
        let du64 = g(m, at(du, i));
        let (au, bu) = (m.f64(a + N_U), m.f64(b + N_U));
        if gt(x(0.0), du64) {
            m.set_f64(at(a5, i), au);
            m.set_f64(at(a7, i), bu);
        } else {
            m.set_f64(at(a5, i), bu);
            m.set_f64(at(a7, i), au);
        }
        let dv64 = g(m, at(dv, i));
        let (av, bv) = (m.f64(a + N_V), m.f64(b + N_V));
        if gt(x(0.0), dv64) {
            m.set_f64(at(a8, i), bv);
            m.set_f64(at(a6, i), av);
        } else {
            m.set_f64(at(a8, i), av);
            m.set_f64(at(a6, i), bv);
        }
        a = b;
        i = i.wrapping_add(1);
    }
}

// --- more routines outside this file -----------------------------------------------------
pub const SI_TRM_DATA_INS: Routine = Routine::new("si_trmDataIns", 0x084f_7910);
pub const SI_TRM_CVR_RVE: Routine = Routine::new("SI_trmCvrRve", 0x084f_7a0c);
pub const SI_TRM_CRV_BBOX: Routine = Routine::new("si_trmCrvBBox", 0x084f_7af0);
const EPS_U: u32 = 0x0882_2268;
const EPS_V: u32 = 0x0882_2270;
const C_1E5B: f64 = 1e-5; // 0x08661a98

/// `si_trmDataIns(line, u, v)` through the outside: the two doubles are two words each.
fn data_ins(m: &mut Mem, o: &mut dyn Outside, line: u32, u: f64, v: f64) {
    let (ub, vb) = (u.to_bits(), v.to_bits());
    o.call(m, SI_TRM_DATA_INS, &[line, ub as u32, (ub >> 32) as u32, vb as u32, (vb >> 32) as u32]);
}

/// [soft 0x084f139c resetTLnFlag]: `resetTLnFlag(l1, l2)`: pairs the crossings of two lines.
/// A negative mark is made positive in both lines; `l1` is searched for its last node marked 0
/// (the stop node, taken as null when none exists: the original then reads an unset local); the
/// first node of `l2` is given to its last node when marked; when the signs differ `l2` is
/// reversed (`SI_trmCvrRve`) for the run and back after. Then, round `l1` from the stop node,
/// a marked node with a marked successor and a marked one after it whose mark and the two
/// next are found in `l2` in the same order get 5000 added (the middle one in each line), the
/// mark of the first node of a line taking that of its last node; at the end every mark above
/// 5000 is cleared.
pub fn reset_tln_flag(m: &mut Mem, o: &mut dyn Outside, l1: u32, l2: u32) {
    let mut flag = false;
    let mut stop = 0u32;
    let mut n = m.u32(l1.wrapping_add(L_HEAD));
    while n != 0 {
        let k = m.i32(n.wrapping_add(N_FLAG));
        if k < 0 {
            m.set_i32(n.wrapping_add(N_FLAG), k.wrapping_neg());
        } else if k == 0 {
            stop = n;
        }
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    let mut last = 0u32;
    let mut n = m.u32(l2.wrapping_add(L_HEAD));
    while n != 0 {
        let k = m.i32(n.wrapping_add(N_FLAG));
        last = n;
        if k < 0 {
            m.set_i32(n.wrapping_add(N_FLAG), k.wrapping_neg());
        }
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    let h2 = m.u32(l2.wrapping_add(L_HEAD));
    let k = m.i32(h2.wrapping_add(N_FLAG));
    if k != 0 {
        m.set_i32(last.wrapping_add(N_FLAG), k);
    }
    if m.i32(l2.wrapping_add(L_SIGN)).wrapping_mul(m.i32(l1.wrapping_add(L_SIGN))) < 0 {
        flag = true;
        o.call(m, SI_TRM_CVR_RVE, &[l2]);
    }
    let h1 = |m: &Mem| m.u32(l1.wrapping_add(L_HEAD));
    let h2 = |m: &Mem| m.u32(l2.wrapping_add(L_HEAD));
    let mut b = next_node(m, h1(m), stop);
    while b != stop {
        if m.i32(b.wrapping_add(N_FLAG)) == 0 {
            b = next_node(m, h1(m), b);
            continue;
        }
        let s = next_node(m, h1(m), b);
        if m.i32(s.wrapping_add(N_FLAG)) != 0 {
            let d = next_node(m, h1(m), s);
            if m.i32(d.wrapping_add(N_FLAG)) == 0 {
                b = d;
                continue;
            }
            let mut f = m.u32(m.u32(l2.wrapping_add(L_HEAD)).wrapping_add(N_NEXT));
            while f != 0 {
                if m.i32(f.wrapping_add(N_FLAG)) == m.i32(b.wrapping_add(N_FLAG)) {
                    let t = next_node(m, h2(m), f);
                    if m.i32(t.wrapping_add(N_FLAG)) != m.i32(s.wrapping_add(N_FLAG)) {
                        break;
                    }
                    let t2 = next_node(m, h2(m), t);
                    if m.i32(t2.wrapping_add(N_FLAG)) == m.i32(d.wrapping_add(N_FLAG)) {
                        let ks = m.i32(s.wrapping_add(N_FLAG)).wrapping_add(5000);
                        m.set_i32(s.wrapping_add(N_FLAG), ks);
                        let kt = m.i32(t.wrapping_add(N_FLAG)).wrapping_add(5000);
                        m.set_i32(t.wrapping_add(N_FLAG), kt);
                        if m.u32(s.wrapping_add(N_NEXT)) == 0 {
                            let a = h1(m);
                            m.set_i32(a.wrapping_add(N_FLAG), ks);
                        }
                        if m.u32(t.wrapping_add(N_NEXT)) == 0 {
                            let a = h2(m);
                            m.set_i32(a.wrapping_add(N_FLAG), kt);
                        }
                    }
                    break;
                }
                f = m.u32(f.wrapping_add(N_NEXT));
            }
        }
        b = s;
    }
    for l in [l1, l2] {
        let mut n = m.u32(l.wrapping_add(L_HEAD));
        while n != 0 {
            if m.i32(n.wrapping_add(N_FLAG)) > 5000 {
                m.set_i32(n.wrapping_add(N_FLAG), 0);
            }
            n = m.u32(n.wrapping_add(N_NEXT));
        }
    }
    if flag {
        o.call(m, SI_TRM_CVR_RVE, &[l2]);
    }
}

/// [soft 0x084f16dc closeLine]: `closeLine(out, a, b, i, j)`: walks the line chosen by the sign of
/// `i * j` (`b` when positive, else `a`) from its cursor, copying each node's point to `out`
/// (`si_trmDataIns`) until a node carrying the cursor's mark is met. A node with a positive
/// mark is looked up in the other line (`getIntPt`; a miss clears that mark and stops without
/// the closing copy); on a find both marks are cleared and, by the sign of the cross product of
/// the two next edges and of `j`, the walk goes on along the other line from its node after the
/// find (the lines swapped) or on along the same line. The closing node is copied last and the
/// marks of both start nodes are cleared.
pub fn close_line(m: &mut Mem, o: &mut dyn Outside, out: u32, a: u32, b: u32, i: i32, j: i32) {
    let c4 = m.u32(a.wrapping_add(L_CUR));
    let c8 = m.u32(b.wrapping_add(L_CUR));
    let k = m.i32(c4.wrapping_add(N_FLAG));
    let (mut vc, mut ed) = if j.wrapping_mul(i) > 0 { (b, a) } else { (a, b) };
    let mut n = next_node(m, m.u32(vc.wrapping_add(L_HEAD)), m.u32(vc.wrapping_add(L_CUR)));
    while m.i32(n.wrapping_add(N_FLAG)) != k {
        let (u, v) = (m.f64(n.wrapping_add(N_U)), m.f64(n.wrapping_add(N_V)));
        data_ins(m, o, out, u, v);
        let mark = m.i32(n.wrapping_add(N_FLAG));
        if mark <= 0 {
            n = next_node(m, m.u32(vc.wrapping_add(L_HEAD)), n);
            continue;
        }
        if get_int_pt(m, ed, mark) as i16 == 0 {
            m.set_i32(n.wrapping_add(N_FLAG), 0);
            return;
        }
        m.set_i32(n.wrapping_add(N_FLAG), 0);
        let cur = m.u32(ed.wrapping_add(L_CUR));
        m.set_i32(cur.wrapping_add(N_FLAG), 0);
        let s = next_node(m, m.u32(vc.wrapping_add(L_HEAD)), n);
        let cur = m.u32(ed.wrapping_add(L_CUR));
        let d = next_node(m, m.u32(ed.wrapping_add(L_HEAD)), cur);
        let p = (g(m, s + N_U) - g(m, n + N_U)) * (g(m, d + N_V) - g(m, cur + N_V));
        let q = (g(m, d + N_U) - g(m, cur + N_U)) * (g(m, s + N_V) - g(m, n + N_V));
        let pos = lt(x(0.0), p - q);
        let swap = if pos { j > 0 } else { j < 0 };
        if swap {
            std::mem::swap(&mut vc, &mut ed);
            n = d;
        } else {
            n = s;
        }
    }
    let (u, v) = (m.f64(n.wrapping_add(N_U)), m.f64(n.wrapping_add(N_V)));
    data_ins(m, o, out, u, v);
    m.set_i32(n.wrapping_add(N_FLAG), 0);
    m.set_i32(c4.wrapping_add(N_FLAG), 0);
    m.set_i32(c8.wrapping_add(N_FLAG), 0);
}

/// [soft 0x084f1998 travelingIntCrvs]: `travelingIntCrvs(st, l1, l2)`: when either line's box
/// touches the range border (`brdIntsct` on +0x10, +0x20, +0x28, +0x18) the status record's
/// object (+0x78) gets bit 0 of the byte at +0x5b and the short at +0x5c cleared; the marks
/// are paired (`resetTLnFlag`); then, for each crossing left (`nxtIntsect`), a new line (0x30
/// bytes from `SI_calloc`) is put in the status table (+0x114, count +0x24) with its first point
/// the first line's cursor point, closed by `closeLine` (the direction -1 when `l1` has a
/// positive sign and `l2` a negative one, else 1) and its box made by `si_trmCrvBBox`.
pub fn traveling_int_crvs(m: &mut Mem, o: &mut dyn Outside, st: u32, l1: u32, l2: u32) {
    let mut dir = 1;
    let hit = |m: &Mem, l: u32| {
        brd_intsct(m, m.f64(l + 0x10), m.f64(l + 0x20), m.f64(l + 0x28), m.f64(l + 0x18)) as i16 != 0
    };
    if hit(m, l1) || hit(m, l2) {
        let obj = m.u32(st.wrapping_add(0x78));
        let b = m.u8(obj.wrapping_add(0x5b));
        m.set_u8(obj.wrapping_add(0x5b), b | 1);
        m.set_u16(obj.wrapping_add(0x5c), 0);
    }
    reset_tln_flag(m, o, l1, l2);
    if 0 < m.i32(l1.wrapping_add(L_SIGN)) && m.i32(l2.wrapping_add(L_SIGN)) < 0 {
        dir = -1;
    }
    loop {
        let r = nxt_intsect(m, l1, l2);
        if r == 0 {
            break;
        }
        let nl = m.si_calloc(1, 0x30);
        let tab = m.u32(st.wrapping_add(0x114));
        let cnt = m.i32(st.wrapping_add(0x24));
        m.set_u32(tab.wrapping_add((cnt as u32).wrapping_mul(4)), nl);
        m.set_i32(st.wrapping_add(0x24), cnt.wrapping_add(1));
        m.set_i32(nl.wrapping_add(4), 0);
        m.set_i32(nl, 0);
        m.set_i32(nl.wrapping_add(8), 0);
        let cur = m.u32(l1.wrapping_add(L_CUR));
        let (u, v) = (m.f64(cur), m.f64(cur.wrapping_add(8)));
        data_ins(m, o, nl, u, v);
        close_line(m, o, nl, l1, l2, r, dir);
        o.call(m, SI_TRM_CRV_BBOX, &[nl]);
    }
}

/// [soft 0x084f56a0 checkAtBorder]: `checkAtBorder(node, edge)` with `*edge` the border the point
/// lies on (1 uS, 2 uE, 3 vS, 4 vE): the coordinate along the border is snapped to it when
/// within `epsU`/`epsV` (the node's byte at +0x12 or-ed with 2), then the other coordinate
/// is snapped to either end in turn, and `*edge` set to the new border (3, 1, 4, 2) when it was.
pub fn check_at_border(m: &mut Mem, node: u32, edge: &mut i32) {
    let near = |m: &Mem, d: F80, e: u32| lt(d, g(m, e));
    let snap = |m: &mut Mem, at: u32, to: u32| {
        let v = m.f64(to);
        m.set_f64(at, v);
        let b = m.u8(node.wrapping_add(0x12));
        m.set_u8(node.wrapping_add(0x12), b | 2);
    };
    let (u, v) = (node.wrapping_add(N_U), node.wrapping_add(N_V));
    match *edge {
        1 | 2 => {
            if *edge == 1 {
                if near(m, g(m, u) - g(m, U_S), EPS_U) {
                    snap(m, u, U_S);
                }
            } else if near(m, g(m, U_E) - g(m, u), EPS_U) {
                snap(m, u, U_E);
            }
            if near(m, g(m, v) - g(m, V_S), EPS_V) {
                snap(m, v, V_S);
                *edge = 3;
                return;
            }
            if near(m, g(m, V_E) - g(m, v), EPS_V) {
                snap(m, v, V_E);
                *edge = 4;
            }
        }
        3 | 4 => {
            if *edge == 3 {
                if near(m, g(m, v) - g(m, V_S), EPS_V) {
                    snap(m, v, V_S);
                }
            } else if near(m, g(m, V_E) - g(m, v), EPS_V) {
                snap(m, v, V_E);
            }
            if near(m, g(m, u) - g(m, U_S), EPS_U) {
                snap(m, u, U_S);
                *edge = 1;
                return;
            }
            if near(m, g(m, U_E) - g(m, u), EPS_U) {
                snap(m, u, U_E);
                *edge = 2;
            }
        }
        _ => {}
    }
}

/// [soft 0x084f5874 perturbSngPtOnBorder]: `perturbSngPtOnBorder(node, edge)`: `checkAtBorder`, then
/// the point pulled 1e-5 in from the border it lies on (`uS + 1e-5`, `uE - 1e-5`, `vS + 1e-5`,
/// `vE - 1e-5`); the sum is rounded on the store.
pub fn perturb_sng_pt_on_border(m: &mut Mem, node: u32, edge: i32) {
    let mut e = edge;
    check_at_border(m, node, &mut e);
    let d = x(C_1E5B);
    match e {
        1 => m.set_f64(node + N_U, (g(m, U_S) + d).to_f64()),
        2 => m.set_f64(node + N_U, (g(m, U_E) - d).to_f64()),
        3 => m.set_f64(node + N_V, (g(m, V_S) + d).to_f64()),
        4 => m.set_f64(node + N_V, (g(m, V_E) - d).to_f64()),
        _ => {}
    }
}

const C_1E6B: f64 = 1e-6; // 0x08661a90

/// [soft 0x084f44b8 edgeDir]: `edgeDir(p1, p2, p3, e1, e2, e3)` answers -1 or 1: whether the
/// point `p2` comes before `p3` going round the range's border (the edges `e1..e3` as
/// `atEdge` numbers them). When `p1 == p3`, `p2` and `p3` on edges 1+2 / 3+4 are compared on
/// the sum of their v (u) coordinates against `vE + vS` (`uE + uS`); the other edge pairs
/// decide by the edge numbers or by one coordinate. Else the three points are unrolled along
/// the border (`edgVal`) and compared, `C = edgVal(p3)` against `edgVal(p1)` (stored as a
/// double) with the tolerance 1e-6, the value of `p2` kept on the register.
pub fn edge_dir(m: &Mem, p1: u32, p2: u32, p3: u32, e1: i32, e2: i32, e3: i32) -> i32 {
    let f = |a: u32| g(m, a);
    if p1 == p3 {
        let sum = e2.wrapping_add(e3);
        let sum_cmp = |ax: u32, hi: u32, lo: u32| -> (F80, F80) { (f(p2 + ax) + f(p3 + ax), f(hi) + f(lo)) };
        // "ge": -1 when vE+vS >= sum (ordered); "lt": -1 when vE+vS < sum
        let ge = |s: (F80, F80)| if !s.1.lt(s.0) && !s.1.unordered(s.0) { -1 } else { 1 };
        let lt_ = |s: (F80, F80)| if s.1.lt(s.0) { -1 } else { 1 };
        if sum == 3 {
            let s = sum_cmp(8, V_E, V_S);
            return if e2 == 1 { lt_(s) } else { ge(s) };
        }
        if sum == 7 {
            let s = sum_cmp(0, U_E, U_S);
            return if e2 == 3 { ge(s) } else { lt_(s) };
        }
        if e2 == e3 {
            let (b, c) = (|ax: u32| f(p2 + ax), |ax: u32| f(p3 + ax));
            return match e3 {
                1 => if gt(c(8), b(8)) { -1 } else { 1 },
                2 => if lt(c(8), b(8)) { -1 } else { 1 },
                3 => if lt(c(0), b(0)) { -1 } else { 1 },
                4 => if gt(c(0), b(0)) { -1 } else { 1 },
                _ => 1,
            };
        }
        if (sum.wrapping_sub(4) as u32) <= 2 {
            return match sum {
                4 => if e1 == 1 { -1 } else { 1 },
                5 => if (e1.wrapping_sub(3) as u32) < 2 { -1 } else { 1 },
                _ => if e1 == 2 { -1 } else { 1 },
            };
        }
    }
    let a64 = edg_val(m, m.f64(p1), m.f64(p1 + 8), e1).to_f64();
    let a64 = x(a64);
    let b = edg_val(m, m.f64(p2), m.f64(p2 + 8), e2);
    let c = edg_val(m, m.f64(p3), m.f64(p3 + 8), e3);
    let ce = c + x(C_1E6B);
    // `ce` against the stored `a`: not (ce <= a or unordered) ...
    let ce_le_a = ce.le(a64) || ce.unordered(a64);
    if !ce_le_a {
        // ... then -1 when b > c (ordered)
        if b.gt(c) {
            return -1;
        }
    } else if !(b.le(c) || b.unordered(c)) {
        // b > c and ordered: falls to the last compare
    } else {
        return 1;
    }
    if b.lt(a64) {
        -1
    } else {
        1
    }
}

// --- the status record's line table, and routines outside this file --------------------------
pub const ADD_CW_BOUNDARY_EDGE: Routine = Routine::new("addCWBoundaryEdge", 0x084f_2de0);
pub const ADD_CCW_BOUNDARY_EDGE: Routine = Routine::new("addCCWBoundaryEdge", 0x084f_38c0);
pub const U_CLOSED: u32 = 0x0882_2278;
pub const V_CLOSED: u32 = 0x0882_227a;
const ST_COUNT: u32 = 0x24;
const ST_TABLE: u32 = 0x114;

fn tab(m: &Mem, st: u32, i: i32) -> u32 {
    m.u32(m.u32(st.wrapping_add(ST_TABLE)).wrapping_add((i as u32).wrapping_mul(4)))
}

fn set_tab(m: &mut Mem, st: u32, i: i32, v: u32) {
    let t = m.u32(st.wrapping_add(ST_TABLE));
    m.set_u32(t.wrapping_add((i as u32).wrapping_mul(4)), v);
}

/// [soft 0x084f52e4 moveTrimLine]: `moveTrimLine(st, idx, n3, n4, cnt, mode, p7, p8)`: cuts the
/// line `idx` of the status record's table after the node `n3` (its tail from the node after
/// `n3` to `n4` becomes a new line of `cnt` nodes, put at the end of the table, the count of
/// the old line lowered by `cnt`, the sign copied). The ends of the border walk are the
/// points of `n3`'s successor and `n4` (copied to two locals), the coordinate named by
/// `mode` (1: v of `n4`'s point to `p7` and v of the other to `p8`; 2: u likewise) replaced;
/// `addCWBoundaryEdge` (sign 1) or `addCCWBoundaryEdge` is called with the new line, the
/// edges `forceAtEdge` found for `n4` and for `n3`'s successor and the two locals; the
/// head point is added to the table's last line (`si_trmDataIns`), the new line's last node
/// gets the head's mark, the box is made, and a positive-signed line is reversed.
#[allow(clippy::too_many_arguments)]
pub fn move_trim_line(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32, n3: u32, n4: u32, cnt: i32, mode: i32, p7: f64, p8: f64) {
    let n3n = m.u32(n3.wrapping_add(N_NEXT));
    let ea = force_at_edge(m, m.f64(n3n), m.f64(n3n + 8));
    let eb = force_at_edge(m, m.f64(n4), m.f64(n4 + 8));
    let old = tab(m, st, idx);
    let c = m.i32(old).wrapping_sub(cnt);
    m.set_i32(old, c);
    let nl = m.si_calloc(1, 0x30);
    let n = m.i32(st.wrapping_add(ST_COUNT));
    set_tab(m, st, n, nl);
    let sign = m.i32(tab(m, st, idx).wrapping_add(L_SIGN));
    m.set_i32(nl.wrapping_add(L_SIGN), sign);
    m.set_i32(st.wrapping_add(ST_COUNT), n.wrapping_add(1));
    let sp = m.sp();
    let l_b = m.local(16); // the point of n4
    let l_a = m.local(16); // the point of n3's successor
    let n3n = m.u32(n3.wrapping_add(N_NEXT));
    m.set_f64(l_a, m.f64(n3n));
    m.set_f64(l_a + 8, m.f64(n3n + 8));
    m.set_f64(l_b, m.f64(n4));
    m.set_f64(l_b + 8, m.f64(n4 + 8));
    m.set_i32(nl, cnt);
    m.set_u32(nl.wrapping_add(L_HEAD), n3n);
    let n4n = m.u32(n4.wrapping_add(N_NEXT));
    m.set_u32(n3.wrapping_add(N_NEXT), n4n);
    m.set_u32(n4.wrapping_add(N_NEXT), 0);
    m.set_u32(nl.wrapping_add(L_CUR), n4);
    if mode == 1 {
        m.set_f64(l_b + 8, p7);
        m.set_f64(l_a + 8, p8);
    } else if mode == 2 {
        m.set_f64(l_b, p7);
        m.set_f64(l_a, p8);
    }
    if m.i32(nl.wrapping_add(L_SIGN)) == 1 {
        add_cw_boundary_edge(m, o, nl, eb, ea, l_b, l_a);
    } else {
        add_ccw_boundary_edge(m, o, nl, eb, ea, l_b, l_a);
    }
    let h = m.u32(nl.wrapping_add(L_HEAD));
    let last = tab(m, st, m.i32(st.wrapping_add(ST_COUNT)).wrapping_sub(1));
    data_ins(m, o, last, m.f64(h), m.f64(h + 8));
    let cur = m.u32(nl.wrapping_add(L_CUR));
    let h = m.u32(nl.wrapping_add(L_HEAD));
    let mk = m.i32(h.wrapping_add(N_FLAG));
    m.set_i32(cur.wrapping_add(N_FLAG), mk);
    o.call(m, SI_TRM_CRV_BBOX, &[nl]);
    if 0 < m.i32(nl.wrapping_add(L_SIGN)) {
        o.call(m, SI_TRM_CVR_RVE, &[nl]);
    }
    m.set_sp(sp);
}

/// `a <= b` as the compiled branch tests it: also true when unordered.
fn le_or_unord(a: F80, b: F80) -> bool {
    a.le(b) || a.unordered(b)
}

const C_1E5C: f64 = 1e-5; // 0x08661aa0

/// [soft 0x084f5fb4 addPtAtEdge]: `addPtAtEdge(l1, l2, p, q)`: on a surface closed in u or in v, the
/// short way between the points `p` (of `l1`) and `q` (of `l2`) may go over the seam. With the
/// distances taken on the register (`d3` the direct one, `d4` the one over the seam; over the
/// seam when `d4 < d3`) the two lines get a point 1e-5 inside each end of the seam
/// (`si_trmDataIns`; the line's cursor is set to the node while it is inserted when the node
/// has a successor). A surface closed both ways takes the u seam unless the v way is the
/// shorter (`d4 < d3` of its test). Reproduced as it is: the second insertion sets the cursor
/// of `l1`, not of `l2`, and restores that of `l2`.
pub fn add_pt_at_edge(m: &mut Mem, o: &mut dyn Outside, l1: u32, l2: u32, p: u32, q: u32) {
    let ucl = m.i16(U_CLOSED);
    let vcl = m.i16(V_CLOSED);
    if ucl == 0 && vcl == 0 {
        return;
    }
    let mut pu = m.f64(p);
    let mut qu = m.f64(q);
    let mut pv = m.f64(p + 8);
    let mut qv = m.f64(q + 8);
    let (ue, us, ve, vs) = (g(m, U_E), g(m, U_S), g(m, V_E), g(m, V_S));
    let eps = x(C_1E5C);
    let mut flag = false;
    let mut ucl_eff = ucl;
    if ucl != 0 && vcl != 0 {
        let d3 = if x(qu).lt(x(pu)) { (ue - x(pu)) + x(qu) } else { (ue - x(qu)) + x(pu) } - us;
        let d4 = if le_or_unord(x(pv), x(qv)) { ((ue - x(qu)) + x(pu)) - us } else { ((ve - x(pv)) + x(qv)) - vs };
        if d4.lt(d3) {
            ucl_eff = 0;
        }
    }
    if ucl_eff != 0 {
        if x(qu).lt(x(pu)) {
            let d3 = x(pu) - x(qu);
            let d4 = ((ue - x(pu)) + x(qu)) - us;
            if !d4.lt(d3) {
                return;
            }
            qu = (us + eps).to_f64();
            pu = (ue - eps).to_f64();
        } else {
            let d3 = x(qu) - x(pu);
            let d4 = ((ue - x(qu)) + x(pu)) - us;
            if !d4.lt(d3) {
                return;
            }
            qu = (ue - eps).to_f64();
            pu = (eps + us).to_f64();
        }
    } else if le_or_unord(x(pv), x(qv)) {
        let d3 = x(qu) - x(pu);
        let d4 = ((ue - x(qu)) + x(pu)) - us;
        if d4.lt(d3) {
            pv = (vs + eps).to_f64();
            qv = (ve - eps).to_f64();
            flag = true;
        }
    } else {
        let d3 = x(pv) - x(qv);
        let d4 = ((ve - x(pv)) + x(qv)) - vs;
        if !d4.lt(d3) {
            return;
        }
        pv = (ve - eps).to_f64();
        qv = (eps + vs).to_f64();
        flag = true;
    }
    if ucl_eff == 0 && !flag {
        return;
    }
    if m.u32(p.wrapping_add(N_NEXT)) == 0 {
        data_ins(m, o, l1, pu, pv);
    } else {
        let save = m.u32(l1.wrapping_add(L_CUR));
        m.set_u32(l1.wrapping_add(L_CUR), p);
        data_ins(m, o, l1, pu, pv);
        m.set_u32(l1.wrapping_add(L_CUR), save);
    }
    if m.u32(q.wrapping_add(N_NEXT)) == 0 {
        data_ins(m, o, l2, qu, qv);
    } else {
        let save = m.u32(l2.wrapping_add(L_CUR));
        m.set_u32(l1.wrapping_add(L_CUR), q);
        data_ins(m, o, l2, qu, qv);
        m.set_u32(l2.wrapping_add(L_CUR), save);
    }
}

/// [soft 0x084f6280 joinTwoLnAtEnd]: joins the status record's two lines (table slots 0 and 1)
/// when fewer than two nodes of the second carry bit 0 of the byte at +0x11: the last such
/// node (if one) gets mark 0; both ends are joined (`addPtAtEdge` on the heads, then on
/// the tails), the second line is reversed (`resTrmCrv`) and appended to the first, the counts
/// added, the box made (`si_trmCrvBBox`), slot 1 cleared and the line count set to 1.
pub fn join_two_ln_at_end(m: &mut Mem, o: &mut dyn Outside, st: u32) {
    let l1 = tab(m, st, 0);
    let l2 = tab(m, st, 1);
    let mut cnt = 0;
    let mut last = m.u32(l1.wrapping_add(L_HEAD));
    let mut n = m.u32(l2.wrapping_add(L_HEAD));
    while n != 0 {
        if m.u8(n.wrapping_add(0x11)) & 1 != 0 {
            last = n;
            cnt += 1;
        }
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    if cnt < 2 {
        if cnt == 1 {
            m.set_i32(last.wrapping_add(N_FLAG), 0);
        }
        let (a, b) = (m.u32(l1.wrapping_add(L_HEAD)), m.u32(l2.wrapping_add(L_HEAD)));
        add_pt_at_edge(m, o, l1, l2, a, b);
        let (a, b) = (m.u32(l1.wrapping_add(L_CUR)), m.u32(l2.wrapping_add(L_CUR)));
        add_pt_at_edge(m, o, l1, l2, a, b);
        res_trm_crv(m, l2);
        let c1 = m.u32(l1.wrapping_add(L_CUR));
        let h2 = m.u32(l2.wrapping_add(L_HEAD));
        m.set_u32(c1.wrapping_add(N_NEXT), h2);
        let c2 = m.u32(l2.wrapping_add(L_CUR));
        m.set_u32(l1.wrapping_add(L_CUR), c2);
        let t = m.i32(l1).wrapping_add(m.i32(l2));
        m.set_i32(l1, t);
        o.call(m, SI_TRM_CRV_BBOX, &[l1]);
        let t = m.u32(st.wrapping_add(ST_TABLE));
        m.set_u32(t.wrapping_add(4), 0);
        m.set_i32(st.wrapping_add(ST_COUNT), 1);
    }
}

/// [soft 0x084f632c midJoinTwoLines]: `midJoinTwoLines(st, a, b)`: joins the table's two lines
/// in the middle: the nodes after `a` and `b` get mark 0; when the second line's tail point
/// lies within 1e-6 (both coordinates, ordered) of its head point the head is dropped (the
/// line starts at its successor, `b` moved on if it was the head, count lowered, node freed);
/// `addPtAtEdge` is called on (a, b) and on the two successors; the second line is reversed
/// and its tail linked to its head, the successor of `b`'s successor swapped with that of
/// `a`, the counts added, the box made, slot 1 cleared and the line count set to 1.
pub fn mid_join_two_lines(m: &mut Mem, o: &mut dyn Outside, st: u32, a: u32, b: u32) {
    let l1 = tab(m, st, 0);
    let l2 = tab(m, st, 1);
    let an = m.u32(a.wrapping_add(N_NEXT));
    let bn = m.u32(b.wrapping_add(N_NEXT));
    m.set_i32(an.wrapping_add(N_FLAG), 0);
    m.set_i32(bn.wrapping_add(N_FLAG), 0);
    let mut b = b;
    let head = m.u32(l2.wrapping_add(L_HEAD));
    let tail = m.u32(l2.wrapping_add(L_CUR));
    let eps = x(0.000001);
    let d1 = (g(m, tail) - g(m, head)).abs();
    if eps.gt(d1) {
        let d2 = (g(m, tail + 8) - g(m, head + 8)).abs();
        if d2.lt(eps) {
            let hn = m.u32(head.wrapping_add(N_NEXT));
            m.set_u32(l2.wrapping_add(L_HEAD), hn);
            if b == head {
                b = m.u32(b.wrapping_add(N_NEXT));
            }
            let c = m.i32(l2).wrapping_sub(1);
            m.set_i32(l2, c);
            if head != 0 {
                m.free(head);
            }
        }
    }
    add_pt_at_edge(m, o, l1, l2, a, b);
    add_pt_at_edge(m, o, l1, l2, an, bn);
    let bn2 = m.u32(b.wrapping_add(N_NEXT));
    res_trm_crv(m, l2);
    let c2 = m.u32(l2.wrapping_add(L_CUR));
    let h2 = m.u32(l2.wrapping_add(L_HEAD));
    m.set_u32(c2.wrapping_add(N_NEXT), h2);
    let t = m.u32(bn2.wrapping_add(N_NEXT));
    let an_next = m.u32(a.wrapping_add(N_NEXT));
    m.set_u32(bn2.wrapping_add(N_NEXT), an_next);
    m.set_u32(a.wrapping_add(N_NEXT), t);
    let s = m.i32(l1).wrapping_add(m.i32(l2));
    m.set_i32(l1, s);
    o.call(m, SI_TRM_CRV_BBOX, &[l1]);
    let t = m.u32(st.wrapping_add(ST_TABLE));
    m.set_u32(t.wrapping_add(4), 0);
    m.set_i32(st.wrapping_add(ST_COUNT), 1);
}

/// [soft 0x084f6720 trimPeak]: `trimPeak(st)`: two lines (table slots 0, 1) whose counts differ by
/// at most 2 are joined. The longer goes first in the table; its nodes are walked for the one
/// with bit 0 of the byte at +0x11 (only exactly one is accepted), remembering the node
/// before it and its index; unless both neighbours' `atEdge` are non-zero and on the same
/// pair of edges (u edges with `uClosed`, v edges with `vClosed`) nothing is done. A found
/// node at index 0 is cleared and the lines joined at their ends (`joinTwoLnAtEnd`); else
/// the matching nodes of both lines are walked to by the count difference (+2, 0 or other)
/// and `midJoinTwoLines` is called.
pub fn trim_peak(m: &mut Mem, o: &mut dyn Outside, st: u32) {
    let mut ok = false;
    let mut longer = tab(m, st, 0);
    let b = tab(m, st, 1);
    let (cb, ca) = (m.i32(b), m.i32(longer));
    if cb.wrapping_add(2) < ca || ca.wrapping_add(2) < cb {
        return;
    }
    let mut shorter = b;
    if ca < cb {
        set_tab(m, st, 0, b);
        set_tab(m, st, 1, longer);
        shorter = tab(m, st, 1);
        longer = b;
    }
    let mut idx: i32 = 0;
    let mut count = 0;
    let mut node = m.u32(longer.wrapping_add(L_HEAD));
    let mut prev = m.u32(longer.wrapping_add(L_CUR));
    let (mut at_idx, mut found, mut before) = (0i32, 0u32, 0u32);
    while node != 0 {
        if m.u8(node.wrapping_add(0x11)) & 1 != 0 {
            count += 1;
            at_idx = idx;
            before = prev;
            found = node;
        }
        idx += 1;
        prev = node;
        node = m.u32(node.wrapping_add(N_NEXT));
    }
    if count != 1 {
        return;
    }
    let e1 = at_edge(m, m.f64(found), m.f64(found + 8));
    let e2 = at_edge(m, m.f64(before), m.f64(before + 8));
    if e1 != 0 && e2 != 0 {
        if e1 < 3 {
            if e2 < 3 && m.i16(U_CLOSED) != 0 {
                ok = true;
            }
        } else if 2 < e2 && m.i16(V_CLOSED) != 0 {
            ok = true;
        }
        if !ok {
            return;
        }
    }
    if at_idx == 0 {
        m.set_i32(found.wrapping_add(N_FLAG), 0);
        join_two_ln_at_end(m, o, st);
    } else {
        let mut s9 = m.u32(shorter.wrapping_add(L_HEAD));
        let mut s10 = m.u32(longer.wrapping_add(L_HEAD));
        let nx = |m: &Mem, p: u32| m.u32(p.wrapping_add(N_NEXT));
        if m.i32(longer) == m.i32(shorter).wrapping_add(2) {
            if at_idx != 1 {
                let mut k = at_idx - 2;
                while 0 < k {
                    s10 = nx(m, s10);
                    s9 = nx(m, s9);
                    k -= 1;
                }
                s10 = nx(m, s10);
            }
        } else if m.i32(longer) == m.i32(shorter) {
            let mut k = at_idx - 1;
            while 0 < k {
                s10 = nx(m, s10);
                s9 = nx(m, s9);
                k -= 1;
            }
        } else {
            let mut k = at_idx - 2;
            while 0 < k {
                s10 = nx(m, s10);
                s9 = nx(m, s9);
                k -= 1;
            }
            s10 = nx(m, s10);
            let n9 = nx(m, s9);
            if m.i32(n9.wrapping_add(N_FLAG)) == 0 {
                s9 = n9;
            }
        }
        mid_join_two_lines(m, o, st, s10, s9);
    }
}

// --- the edge samples -------------------------------------------------------------------
pub const U_EG_INC: u32 = 0x0882_2248;
pub const V_EG_INC: u32 = 0x0882_2250;
pub const VH_EG_INC: u32 = 0x0882_2260;
pub const U_SMP: u32 = 0x086d_b8e4;
pub const V_SMP: u32 = 0x086d_b8e8;
const C_5E5: f64 = 5e-5; // 0x08661a80, 0x08661a88

thread_local! {
    static SMP_OUTSIDE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Checker aid, not part of the original: counts reads of an edge-sample table that the routine itself
/// allocated (`si_detectEdgeCross` callocs them), or that a check placed in a buffer, at an index outside the
/// block. The original there reads a malloc chunk header, a neighbouring block or fill (undefined memory); the
/// port reads whatever its image holds, so a comparison of such a case says nothing.
fn note_smp_index(m: &Mem, base: u32, i: i32) {
    let at = base.wrapping_add((i as u32).wrapping_mul(8));
    let block = m
        .heap_blocks()
        .iter()
        .find(|b| b.0 == base)
        .or_else(|| m.arena_blocks().iter().find(|b| base.wrapping_sub(b.0) < b.1.max(1)));
    if let Some(&(s, n)) = block {
        if at < s || (at as u64) + 8 > s as u64 + n as u64 {
            SMP_OUTSIDE.with(|c| c.set(c.get() + 1));
        }
    }
}

/// Returns and clears the count of reads outside a routine-allocated edge-sample table (see `note_smp_index`).
pub fn take_smp_outside_reads() -> u32 {
    SMP_OUTSIDE.with(|c| c.replace(0))
}

/// The body of `addUEgSmp` and `addVEgSmp`, which are the same routine on the two axes.
/// `fixed` is the coordinate that stays, `a` and `b` the ends of the one that runs
/// (`axis` 0: u runs and the other is v; 1: v runs). Points go in through `si_trmDataIns`
/// with the running coordinate first for axis 0, second for axis 1.
fn add_eg_smp(m: &mut Mem, o: &mut dyn Outside, line: u32, fixed: f64, a: f64, b: f64, axis: u32) {
    let ins = |m: &mut Mem, o: &mut dyn Outside, run: f64| {
        if axis == 0 {
            data_ins(m, o, line, run, fixed)
        } else {
            data_ins(m, o, line, fixed, run)
        }
    };
    let eps5 = x(C_5E5);
    let (inc, smp, closed_other, eps_g) =
        if axis == 0 { (U_EG_INC, U_SMP, V_CLOSED, EPS_U) } else { (V_EG_INC, V_SMP, U_CLOSED, EPS_V) };
    let head = m.u32(line.wrapping_add(L_HEAD));
    let mut do_ins = true;
    if head != 0 {
        let cur = m.u32(line.wrapping_add(L_CUR));
        let (c_run, c_fix) = if axis == 0 { (g(m, cur), g(m, cur + 8)) } else { (g(m, cur + 8), g(m, cur)) };
        let d1 = (c_run - x(a)).abs();
        if !eps5.lt(d1) {
            let d2 = (c_fix - x(fixed)).abs();
            // skip when |d2| <= eps or unordered
            if d2.le(eps5) || d2.unordered(eps5) {
                do_ins = false;
            }
        }
    }
    if do_ins {
        ins(m, o, a);
    }
    let a8 = x(a);
    if m.i16(closed_other) == 0 {
        let vh = g(m, VH_EG_INC);
        let inc_v = g(m, inc);
        if a8.lt(x(b)) {
            let e = x(b) - vh;
            let mut c = a8 + inc_v;
            if c.lt(e) {
                loop {
                    ins(m, o, c.to_f64());
                    c = c + inc_v;
                    if !e.gt(c) {
                        break;
                    }
                }
            }
        } else {
            let e = x(b) + vh;
            let mut c = a8 - inc_v;
            if c.gt(e) {
                loop {
                    ins(m, o, c.to_f64());
                    c = c - inc_v;
                    if !e.lt(c) {
                        break;
                    }
                }
            }
        }
    } else {
        let inc_v = g(m, inc);
        // the quotients are truncated (the control word is changed to round toward zero)
        let mut i0 = (a8 / inc_v).to_i32(si_x87::Round::Zero);
        let mut i1 = (x(b) / inc_v).to_i32(si_x87::Round::Zero);
        let sm = |m: &Mem, i: i32| {
            let base = m.u32(smp);
            note_smp_index(m, base, i);
            g(m, base.wrapping_add((i as u32).wrapping_mul(8)))
        };
        let smf = |m: &Mem, i: i32| {
            let base = m.u32(smp);
            note_smp_index(m, base, i);
            m.f64(base.wrapping_add((i as u32).wrapping_mul(8)))
        };
        if i0 < i1 {
            i0 += 1;
            // fcomp: a against the sample; no step when a < sample or unordered
            if !(a8.lt(sm(m, i0)) || a8.unordered(sm(m, i0))) {
                i0 += 1;
            }
            let t = sm(m, i1) + g(m, eps_g);
            if !(t.lt(x(b)) || t.unordered(x(b))) {
                i1 -= 1;
            }
            let mut k = i0;
            while k <= i1 {
                let v = smf(m, k);
                ins(m, o, v);
                k += 1;
            }
        } else if i0 > i1 {
            i1 += 1;
            let s = sm(m, i1);
            if !(x(b).lt(s) || x(b).unordered(s)) {
                i1 += 1;
            }
            let t = sm(m, i0) + g(m, eps_g);
            if !(t.lt(a8) || t.unordered(a8)) {
                i0 -= 1;
            }
            let mut k = i0;
            while k >= i1 {
                let v = smf(m, k);
                ins(m, o, v);
                k -= 1;
            }
        }
    }
    let cur = m.u32(line.wrapping_add(L_CUR));
    let mut node = m.u32(cur.wrapping_add(N_NEXT));
    if node == 0 {
        node = m.u32(line.wrapping_add(L_HEAD));
    }
    let (n_run, n_fix) = if axis == 0 { (g(m, node), g(m, node + 8)) } else { (g(m, node + 8), g(m, node)) };
    let d1 = (n_run - x(b)).abs();
    let mut put = eps5.lt(d1);
    if !put {
        let d2 = (n_fix - x(fixed)).abs();
        put = !(d2.le(eps5) || d2.unordered(eps5));
    }
    if put {
        ins(m, o, b);
    }
}

/// [soft 0x084f2730 addUEgSmp]: `addUEgSmp(line, v, u0, u1)`: adds to the line the points along the
/// row `v` from `u0` to `u1`: a first point at `u0` unless the cursor's point is within 5e-5
/// of it in both coordinates; then, on a surface not closed in v, points `uEgInc` apart
/// stepping from `u0` towards `u1` and stopping `vhEgInc` short of it (the sums kept on
/// the register); else the samples of `uSmp` between them (indices by the truncated quotient
/// with `uEgInc`, the end ones dropped when within `epsU`); then a last point at `u1`
/// unless the point after the cursor is within 5e-5 of it.
pub fn add_u_eg_smp(m: &mut Mem, o: &mut dyn Outside, line: u32, v: f64, u0: f64, u1: f64) {
    add_eg_smp(m, o, line, v, u0, u1, 0)
}

/// [soft 0x084f2a00 addVEgSmp]: `addVEgSmp(line, u, v0, v1)`: `addUEgSmp` on the other axis (steps
/// `vEgInc`, samples `vSmp`, tolerance `epsV`, the seam test on `uClosed`).
pub fn add_v_eg_smp(m: &mut Mem, o: &mut dyn Outside, line: u32, u: f64, v0: f64, v1: f64) {
    add_eg_smp(m, o, line, u, v0, v1, 1)
}

/// [soft 0x084f6438 trimHalfPeak]: `trimHalfPeak(line)`: when exactly one node of the line has bit 0
/// of the byte at +0x11 and neither it nor its predecessor lies on the border, its mark is
/// cleared and, if exactly one pair of neighbours (not marked) lay on the two u edges
/// (`atEdge` sum 3, with `uClosed`) or on the two v edges (sum 7, `vClosed`), two points are
/// added across the seam at the mean of the marked pair's other coordinate and 1e-5 inside
/// each end (the cursor set to the predecessor meanwhile when it is not the cursor).
pub fn trim_half_peak(m: &mut Mem, o: &mut dyn Outside, line: u32) {
    let (mut cnt, mut cu, mut cv) = (0, 0, 0);
    let mut prev = m.u32(line.wrapping_add(L_CUR));
    let (mut node_m, mut prev_m) = (0u32, 0u32);
    let mut node = m.u32(line.wrapping_add(L_HEAD));
    while node != 0 {
        if m.u8(node.wrapping_add(0x11)) & 1 == 0 {
            if m.i16(U_CLOSED) != 0 {
                let a = at_edge(m, m.f64(node), m.f64(node + 8));
                let b = at_edge(m, m.f64(prev), m.f64(prev + 8));
                if b.wrapping_add(a) == 3 {
                    cu += 1;
                }
            }
            if m.i16(V_CLOSED) != 0 {
                let a = at_edge(m, m.f64(node), m.f64(node + 8));
                let b = at_edge(m, m.f64(prev), m.f64(prev + 8));
                if b.wrapping_add(a) == 7 {
                    cv += 1;
                }
            }
        } else {
            node_m = node;
            prev_m = prev;
            cnt += 1;
        }
        prev = node;
        node = m.u32(node.wrapping_add(N_NEXT));
    }
    if cnt != 1 {
        return;
    }
    if at_edge(m, m.f64(node_m), m.f64(node_m + 8)) != 0 && at_edge(m, m.f64(prev_m), m.f64(prev_m + 8)) != 0 {
        return;
    }
    m.set_i32(node_m.wrapping_add(N_FLAG), 0);
    let half = x(0.5);
    let eps = x(C_1E5);
    let (mid, stored, reg, along_u);
    if cu == 1 {
        mid = ((g(m, node_m + 8) + g(m, prev_m + 8)) * half).to_f64();
        if le_or_unord(g(m, prev_m), g(m, node_m)) {
            stored = (g(m, U_E) - eps).to_f64();
            reg = (eps + g(m, U_S)).to_f64();
        } else {
            stored = (g(m, U_S) + eps).to_f64();
            reg = (g(m, U_E) - eps).to_f64();
        }
        along_u = true;
    } else {
        if cv != 1 {
            return;
        }
        mid = ((g(m, node_m) + g(m, prev_m)) * half).to_f64();
        if le_or_unord(g(m, prev_m + 8), g(m, node_m + 8)) {
            stored = (g(m, V_E) - eps).to_f64();
            reg = (eps + g(m, V_S)).to_f64();
        } else {
            stored = (g(m, V_S) + eps).to_f64();
            reg = (g(m, V_E) - eps).to_f64();
        }
        along_u = false;
    }
    let ins = |m: &mut Mem, o: &mut dyn Outside, run: f64| {
        if along_u {
            data_ins(m, o, line, run, mid)
        } else {
            data_ins(m, o, line, mid, run)
        }
    };
    let cur = m.u32(line.wrapping_add(L_CUR));
    if prev_m != cur {
        m.set_u32(line.wrapping_add(L_CUR), prev_m);
        ins(m, o, reg);
        ins(m, o, stored);
        m.set_u32(line.wrapping_add(L_CUR), cur);
    } else {
        ins(m, o, reg);
        ins(m, o, stored);
    }
}

/// Calls `addCWBoundaryEdge` (sign 1) or `addCCWBoundaryEdge` with the border points `(u1, v1)`
/// and `(u2, v2)` as two stack records. Used by the routines that close a line along the border.
fn add_boundary(m: &mut Mem, o: &mut dyn Outside, line: u32, e1: i32, e2: i32, a: (f64, f64), b: (f64, f64)) {
    let la = m.local(16);
    let lb = m.local(16);
    m.set_f64(la, a.0);
    m.set_f64(la + 8, a.1);
    m.set_f64(lb, b.0);
    m.set_f64(lb + 8, b.1);
    if m.i32(line.wrapping_add(L_SIGN)) == 1 {
        add_cw_boundary_edge(m, o, line, e1, e2, la, lb);
    } else {
        add_ccw_boundary_edge(m, o, line, e1, e2, la, lb);
    }
}

/// The seam adjustment shared by `trmCrvRevAEdg` and `trmCrvAEdg`: when the two border edges
/// sum to 3 on a surface closed in u, both v values become `linerInterp(.., uE)`; when they
/// sum to 7 on one closed in v, both u values become `linerInterp(.., vE)` (the result
/// stored to both doubles from the register).
fn seam_interp(m: &Mem, esum: i32, a: &mut (f64, f64), b: &mut (f64, f64)) {
    if esum == 3 && m.i16(U_CLOSED) != 0 {
        let r = liner_interp(a.0, a.1, b.0, b.1, m.f64(U_E)).to_f64();
        a.1 = r;
        b.1 = r;
    } else if esum == 7 && m.i16(V_CLOSED) != 0 {
        let r = liner_interp(a.1, a.0, b.1, b.0, m.f64(V_E)).to_f64();
        a.0 = r;
        b.0 = r;
    }
}

/// [soft 0x084f5474 trmCrvRevAEdg]: `trmCrvRevAEdg(st, idx)`: walks line `idx` of the table from
/// the second node with a count; a node with bit 0x100 of its mark resets the count and
/// remembers the node before it; a node with 0x200 and a remembered node cuts the line there
/// (`moveTrimLine(st, idx, remembered, node, count + 1, 0, 0, 0)`; the walk goes on after the node
/// that was next, whose bit 0 of the byte at +0x11 resets the count), without one it is
/// kept as the closing node. Of the closing node and the successor of the remembered one
/// (the `forceAtEdge` edges and the points, the seam adjusted by `linerInterp`) the border
/// walk is added with `addCWBoundaryEdge` or `addCCWBoundaryEdge` by the sign; then the box
/// is made and a positive-signed line is reversed.
pub fn trm_crv_rev_a_edg(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32) {
    let mut cnt: i32 = 0;
    let line = tab(m, st, idx);
    let (mut rem, mut found) = (0u32, 0u32);
    let mut prev = m.u32(line.wrapping_add(L_HEAD));
    let mut cur = m.u32(prev.wrapping_add(N_NEXT));
    while cur != 0 {
        let flag = m.u32(cur.wrapping_add(N_FLAG));
        if flag & 0x100 != 0 {
            cnt = 0;
            rem = prev;
        } else if flag & 0x200 != 0 {
            if rem == 0 {
                found = cur;
            } else {
                let nxt = m.u32(cur.wrapping_add(N_NEXT));
                move_trim_line(m, o, st, idx, rem, cur, cnt.wrapping_add(1), 0, 0.0, 0.0);
                cur = nxt;
                if cur == 0 {
                    break;
                }
                if m.u8(cur.wrapping_add(0x11)) & 1 != 0 {
                    cnt = 0;
                }
            }
        }
        cnt = cnt.wrapping_add(1);
        prev = cur;
        cur = m.u32(cur.wrapping_add(N_NEXT));
    }
    if found == 0 {
        return;
    }
    m.set_u32(line.wrapping_add(L_CUR), found);
    let e1 = force_at_edge(m, m.f64(found), m.f64(found + 8));
    let rn = m.u32(rem.wrapping_add(N_NEXT));
    let e2 = force_at_edge(m, m.f64(rn), m.f64(rn + 8));
    let mut a = (m.f64(found), m.f64(found + 8));
    let mut b = (m.f64(rn), m.f64(rn + 8));
    seam_interp(m, e1.wrapping_add(e2), &mut a, &mut b);
    let sp = m.sp();
    add_boundary(m, o, line, e1, e2, a, b);
    m.set_sp(sp);
    let l = tab(m, st, idx);
    o.call(m, SI_TRM_CRV_BBOX, &[l]);
    let l = tab(m, st, idx);
    if 0 < m.i32(l.wrapping_add(L_SIGN)) {
        o.call(m, SI_TRM_CVR_RVE, &[l]);
    }
}

/// [soft 0x084f58f8 trmCrvAEdg]: `trmCrvAEdg(st, idx)`: puts the trim line `idx` along the
/// border: returns 1 when done, 0 when the line's sign is to be negated (`st`+2 set), and 2..6
/// when it does not apply (2 no node of the line is marked on the border, 3 and 4 more than
/// two crossings of a closed seam, 5 crossings of both kinds, 6 a seam node on no edge).
/// First `atEdge` of every node goes to an array (the first value again at the end); then, round
/// the nodes after the head, a node with bit 0x100 of its mark counts as the u or v seam (by
/// its edge, 1-2 or 3-4; none is the answer 6); an other node whose edge is the same as its
/// predecessor's has `checkAtBorder` (when the byte at +0x12 has no bit 2), one whose
/// predecessor's edge sums to 3 (u closed) or 7 (v closed) with its own is marked 0x100
/// (counted), and one that is alone on an edge is moved off it (`perturbSngPtOnBorder`); a node
/// with bit 0 of the byte at +0x11 sets bit 1 of its predecessor's byte there, records its edge
/// and the node, and clears the sign. A second round drops the nodes with bit 1 of the byte at
/// +0x12 that follow a run of such (a second array, the edges kept), marking the first of a run
/// 0x200 and the last 0x100 as it goes; then the edge arrays are chosen, the counts checked
/// (answers 2 to 5) and the line walked: each node with bit 0 of the byte at +0x11 gets the
/// sign (`edgeDir`), and the border from the previous node to it is added as in
/// `trmCrvRevAEdg`; at the end the box and, when positive, the reversal.
pub fn trm_crv_a_edg(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32) -> u32 {
    let (mut c_edge, mut c_seam) = (0i32, 0i32); // -0x40, -0x3c
    let (mut cu3, mut cv7, mut clo, mut chi) = (0i32, 0i32, 0i32, 0i32); // -0x4c -0x50 -0x54 -0x58
    let mut last = 0u32; // -0x5c
    let line = tab(m, st, idx);
    let count = m.i32(line);
    let mut a = m.si_calloc(count.wrapping_add(1) as u32, 4);
    let b = m.si_calloc(count.wrapping_add(1) as u32, 4);
    let ai = |m: &Mem, base: u32, i: i32| m.i32(base.wrapping_add((i as u32).wrapping_mul(4)));
    let set_ai = |m: &mut Mem, base: u32, i: i32, v: i32| m.set_i32(base.wrapping_add((i as u32).wrapping_mul(4)), v);
    let mut i = 0i32;
    let mut n = m.u32(line.wrapping_add(L_HEAD));
    while n != 0 {
        let e = at_edge(m, m.f64(n), m.f64(n + 8));
        set_ai(m, a, i, e);
        i = i.wrapping_add(1);
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    let a0 = ai(m, a, 0);
    set_ai(m, a, i, a0);
    let mut idx2 = 1i32;
    let mut prev = m.u32(line.wrapping_add(L_HEAD));
    let mut cur = m.u32(prev.wrapping_add(N_NEXT));
    while cur != 0 {
        let flag = m.u32(cur.wrapping_add(N_FLAG));
        if flag & 0x100 != 0 {
            let e = ai(m, a, idx2);
            if e == 0 {
                if a != 0 {
                    m.free(a);
                }
                return 6;
            }
            if e > 2 {
                chi += 1;
            } else {
                clo += 1;
            }
        } else {
            let e = ai(m, a, idx2);
            let p = ai(m, a, idx2 - 1);
            if e <= 0 || p <= 0 {
                if p == 0 && ai(m, a, idx2 + 1) == 0 && e != 0 {
                    perturb_sng_pt_on_border(m, cur, e);
                }
            } else if e == p {
                if m.u8(prev.wrapping_add(0x12)) & 2 == 0 {
                    let mut ev = e;
                    check_at_border(m, prev, &mut ev);
                    set_ai(m, a, idx2, ev);
                }
                if m.u8(cur.wrapping_add(0x12)) & 2 == 0 {
                    let mut ev = ai(m, a, idx2);
                    check_at_border(m, cur, &mut ev);
                    set_ai(m, a, idx2, ev);
                }
            } else if p + e == 3 {
                if m.i16(U_CLOSED) == 0 {
                    clo += 1;
                } else {
                    m.set_u32(cur.wrapping_add(N_FLAG), flag | 0x100);
                    cu3 += 1;
                }
            } else if p + e == 7 {
                if m.i16(V_CLOSED) == 0 {
                    chi += 1;
                } else {
                    m.set_u32(cur.wrapping_add(N_FLAG), flag | 0x100);
                    cv7 += 1;
                }
            }
        }
        if m.u8(cur.wrapping_add(0x11)) & 1 != 0 {
            if m.u32(cur.wrapping_add(N_NEXT)) == 0 {
                let h = m.u32(line.wrapping_add(L_HEAD));
                let v = m.u8(h.wrapping_add(0x11)) | 1;
                m.set_u8(h.wrapping_add(0x11), v);
            }
            let v = m.u8(prev.wrapping_add(0x11)) | 2;
            m.set_u8(prev.wrapping_add(0x11), v);
            c_edge = ai(m, a, idx2);
            last = cur;
            m.set_i32(line.wrapping_add(L_SIGN), 0);
        }
        prev = cur;
        cur = m.u32(cur.wrapping_add(N_NEXT));
        idx2 += 1;
    }
    // the second round
    let mut edi = 0u32;
    let mut ebx = m.u32(line.wrapping_add(L_HEAD));
    let mut j = 0i32;
    let mut k = 0i32;
    'second: while ebx != 0 {
        if m.u8(ebx.wrapping_add(0x12)) & 2 == 0 {
            let v = ai(m, a, k);
            set_ai(m, b, j, v);
            j += 1;
            edi = 0;
        } else if edi == 0 {
            m.set_i32(ebx.wrapping_add(N_FLAG), 0x200);
            let v = ai(m, a, k);
            set_ai(m, b, j, v);
            j += 1;
            edi = ebx;
        } else {
            let nx = m.u32(ebx.wrapping_add(N_NEXT));
            if nx == 0 {
                let head = m.u32(line.wrapping_add(L_HEAD));
                if m.u8(head.wrapping_add(0x11)) & 2 == 0 {
                    break 'second;
                }
                let h1 = m.u32(head.wrapping_add(N_NEXT));
                if m.i32(h1.wrapping_add(N_FLAG)) == 0 {
                    m.set_i32(ebx.wrapping_add(N_FLAG), 0x100);
                    c_edge = ai(m, a, k);
                    last = ebx;
                    m.set_i32(line.wrapping_add(L_SIGN), 0);
                    let v = ai(m, a, k);
                    set_ai(m, b, j, v);
                    let h = m.u32(line.wrapping_add(L_HEAD));
                    let f = m.i32(last.wrapping_add(N_FLAG));
                    m.set_i32(h.wrapping_add(N_FLAG), f);
                } else {
                    m.set_f64(head, m.f64(edi));
                    let h = m.u32(line.wrapping_add(L_HEAD));
                    m.set_f64(h + 8, m.f64(edi + 8));
                    let h = m.u32(line.wrapping_add(L_HEAD));
                    let f = m.i32(edi.wrapping_add(N_FLAG));
                    m.set_i32(h.wrapping_add(N_FLAG), f);
                    m.set_u32(edi.wrapping_add(N_NEXT), 0);
                    if ebx != 0 {
                        m.free(ebx);
                    }
                    let c = m.i32(line).wrapping_sub(1);
                    m.set_i32(line, c);
                    set_ai(m, b, 0, c_seam);
                }
                break 'second;
            }
            if m.u8(nx.wrapping_add(0x12)) & 2 == 0 {
                m.set_i32(ebx.wrapping_add(N_FLAG), 0x100);
                c_edge = ai(m, a, k);
                last = ebx;
                m.set_i32(line.wrapping_add(L_SIGN), 0);
                let v = ai(m, a, k);
                set_ai(m, b, j, v);
                j += 1;
            } else {
                m.set_u32(edi.wrapping_add(N_NEXT), nx);
                if ebx != 0 {
                    m.free(ebx);
                }
                let c = m.i32(line).wrapping_sub(1);
                m.set_i32(line, c);
                c_seam = ai(m, a, k);
                ebx = edi;
            }
        }
        ebx = m.u32(ebx.wrapping_add(N_NEXT));
        k += 1;
    }
    let cur_arr;
    if c_seam != 0 {
        if a != 0 {
            m.free(a);
        }
        a = b;
        cur_arr = b;
    } else {
        if b != 0 {
            m.free(b);
        }
        cur_arr = a;
    }
    let _ = cur_arr;
    if c_edge == 0 {
        if a != 0 {
            m.free(a);
        }
        return 2;
    }
    if (cu3 > 1 && chi != 0) || (cv7 > 1 && clo != 0) {
        if a != 0 {
            m.free(a);
        }
        return 5;
    }
    if cu3 > 2 && m.i16(U_CLOSED) != 0 {
        if a != 0 {
            m.free(a);
        }
        return 3;
    }
    if cv7 > 2 && m.i16(V_CLOSED) != 0 {
        if a != 0 {
            m.free(a);
        }
        return 4;
    }
    let mut k = 1i32;
    let head = m.u32(line.wrapping_add(L_HEAD));
    m.set_u32(line.wrapping_add(L_CUR), head);
    let mut n = m.u32(head.wrapping_add(N_NEXT));
    while n != 0 {
        if m.u8(n.wrapping_add(0x11)) & 1 != 0 {
            let obj = m.u32(st.wrapping_add(0x78));
            let v = m.u8(obj.wrapping_add(0x5b)) | 1;
            m.set_u8(obj.wrapping_add(0x5b), v);
            let (e1, e2) = (ai(m, a, k - 1), ai(m, a, k));
            if m.i32(line.wrapping_add(L_SIGN)) == 0 {
                let cur = m.u32(line.wrapping_add(L_CUR));
                let r = edge_dir(m, last, cur, n, c_edge, e1, e2);
                m.set_i32(line.wrapping_add(L_SIGN), r);
                if m.i16(st.wrapping_add(2)) != 0 {
                    m.set_i32(line.wrapping_add(L_SIGN), r.wrapping_neg());
                    if a != 0 {
                        m.free(a);
                    }
                    return 0;
                }
            }
            let cur = m.u32(line.wrapping_add(L_CUR));
            let v = m.u8(cur.wrapping_add(0x12)) | 1;
            m.set_u8(cur.wrapping_add(0x12), v);
            let v = m.u8(n.wrapping_add(0x12)) | 2;
            m.set_u8(n.wrapping_add(0x12), v);
            let mut pa = (m.f64(cur), m.f64(cur + 8));
            let mut pb = (m.f64(n), m.f64(n + 8));
            seam_interp(m, e1.wrapping_add(e2), &mut pa, &mut pb);
            let sp = m.sp();
            add_boundary(m, o, line, e1, e2, pa, pb);
            m.set_sp(sp);
            if m.u32(n.wrapping_add(N_NEXT)) == 0 {
                let h = m.u32(line.wrapping_add(L_HEAD));
                m.set_f64(h, m.f64(n));
                let h = m.u32(line.wrapping_add(L_HEAD));
                m.set_f64(h + 8, m.f64(n + 8));
                let h = m.u32(line.wrapping_add(L_HEAD));
                let f = m.i32(n.wrapping_add(N_FLAG));
                m.set_i32(h.wrapping_add(N_FLAG), f);
            }
        }
        m.set_u32(line.wrapping_add(L_CUR), n);
        n = m.u32(n.wrapping_add(N_NEXT));
        k += 1;
    }
    o.call(m, SI_TRM_CRV_BBOX, &[line]);
    if 0 < m.i32(line.wrapping_add(L_SIGN)) {
        o.call(m, SI_TRM_CVR_RVE, &[line]);
    }
    if a != 0 {
        m.free(a);
    }
    1
}

const U_EPS_G: u32 = U_EPS;
const V_EPS_G: u32 = V_EPS;

/// The body of `handleUCsdMulIntsct` and `handleVCsdMulIntsct`. `axis` 0 is the u routine
/// (scans v, compares `uE - uEps` with u, cuts with mode 1, interpolates towards `uE`), 1 the
/// v routine (the other way round). `o_a` is the offset of the coordinate that stays,
/// `o_b` of the one that is interpolated.
fn handle_csd_mul_intsct(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32, axis: u32) {
    let (o_a, o_b): (u32, u32) = if axis == 0 { (0, 8) } else { (8, 0) };
    let (lim_e, lim_s, eps_g, mode) = if axis == 0 { (V_E, V_S, U_EPS_G, 1) } else { (U_E, U_S, V_EPS_G, 2) };
    let (e_cmp, e_int) = if axis == 0 { (U_E, U_E) } else { (V_E, V_E) };
    let line = tab(m, st, idx);
    let mut vmin = g(m, lim_e);
    let mut vmax = g(m, lim_s);
    let (mut min_node, mut max_node) = (0u32, 0u32);
    let mut tail = 0u32;
    let mut n = m.u32(m.u32(line.wrapping_add(L_HEAD)).wrapping_add(N_NEXT));
    while n != 0 {
        if m.u8(n.wrapping_add(0x11)) & 2 != 0 {
            let v = g(m, n + o_b);
            if v.lt(vmin) {
                vmin = v;
                min_node = n;
            }
            if v.gt(vmax) {
                vmax = v;
                max_node = n;
            }
        }
        tail = n;
        n = m.u32(n.wrapping_add(N_NEXT));
    }
    let obj = m.u32(st.wrapping_add(0x78));
    let b = m.u8(obj.wrapping_add(0x5b)) | 1;
    m.set_u8(obj.wrapping_add(0x5b), b);
    let uepsd = g(m, eps_g);
    let lim = g(m, e_cmp) - uepsd;
    // the next marked node after the highest, going round
    let mut e = m.u32(max_node.wrapping_add(N_NEXT));
    while m.u8(e.wrapping_add(0x11)) & 2 == 0 {
        e = m.u32(e.wrapping_add(N_NEXT));
        if e == 0 {
            e = m.u32(line.wrapping_add(L_HEAD));
        }
    }
    let mut x_node = max_node;
    let (c38, c3c): (i32, i32);
    let taken;
    if e == min_node {
        x_node = e;
        taken = le_ord(lim, g(m, e + o_a));
        if axis == 0 {
            if taken { m.set_i32(line.wrapping_add(L_SIGN), 1); c38 = 1; c3c = 2; } else { m.set_i32(line.wrapping_add(L_SIGN), -1); c38 = 2; c3c = 1; }
        } else if taken {
            m.set_i32(line.wrapping_add(L_SIGN), -1); c38 = 3; c3c = 4;
        } else {
            m.set_i32(line.wrapping_add(L_SIGN), 1); c38 = 4; c3c = 3;
        }
    } else {
        taken = le_ord(lim, g(m, e + o_a));
        if axis == 0 {
            if taken { m.set_i32(line.wrapping_add(L_SIGN), 1); c38 = 2; c3c = 1; } else { m.set_i32(line.wrapping_add(L_SIGN), -1); c38 = 1; c3c = 2; }
        } else if taken {
            m.set_i32(line.wrapping_add(L_SIGN), -1); c38 = 4; c3c = 3;
        } else {
            m.set_i32(line.wrapping_add(L_SIGN), 1); c38 = 3; c3c = 4;
        }
    }
    // turn the loop round so that it starts after x_node
    let h0 = m.u32(line.wrapping_add(L_HEAD));
    let xn = m.u32(x_node.wrapping_add(N_NEXT));
    m.set_u32(line.wrapping_add(L_HEAD), xn);
    let h0n = m.u32(h0.wrapping_add(N_NEXT));
    m.set_u32(tail.wrapping_add(N_NEXT), h0n);
    // (the tail may be `x_node`: its successor is read again, after the store above)
    let xn2 = m.u32(x_node.wrapping_add(N_NEXT));
    m.set_f64(h0, m.f64(xn2));
    m.set_f64(h0 + 8, m.f64(xn2 + 8));
    m.set_i32(h0.wrapping_add(N_FLAG), m.i32(xn2.wrapping_add(N_FLAG)));
    m.set_u32(h0.wrapping_add(N_NEXT), 0);
    m.set_u32(x_node.wrapping_add(N_NEXT), h0);
    let rec = |a: f64, r: f64| if axis == 0 { (a, r) } else { (r, a) };
    // one cut: the two interpolations, moveTrimLine, the border walk between the two points
    let cut = |m: &mut Mem, o: &mut dyn Outside, c40: u32, esi: u32, cnt: i32, ca: i32, cb: i32| -> u32 {
        let ev = m.f64(e_int);
        let r1 = liner_interp(m.f64(c40 + o_a), m.f64(c40 + o_b), m.f64(m.u32(c40 + N_NEXT) + o_a), m.f64(m.u32(c40 + N_NEXT) + o_b), ev);
        let r2 = liner_interp(m.f64(esi + o_a), m.f64(esi + o_b), m.f64(m.u32(esi + N_NEXT) + o_a), m.f64(m.u32(esi + N_NEXT) + o_b), ev);
        move_trim_line(m, o, st, idx, c40, esi, cnt.wrapping_add(1), mode, r2.to_f64(), r1.to_f64());
        let edi = m.u32(c40.wrapping_add(N_NEXT));
        m.set_u32(line.wrapping_add(L_CUR), c40);
        let nn = m.u32(edi.wrapping_add(N_NEXT));
        let ra = rec(m.f64(c40 + o_a), r1.to_f64());
        let rb = rec(m.f64(nn + o_a), r2.to_f64());
        let sp = m.sp();
        add_boundary(m, o, line, ca, cb, ra, rb);
        m.set_sp(sp);
        edi
    };
    let (mut cx, mut c40, mut cnt) = (false, 0u32, 0i32);
    let mut esi = 0u32;
    if m.i16(st.wrapping_add(2)) != 0 {
        let h = m.u32(line.wrapping_add(L_HEAD));
        m.set_u32(h0.wrapping_add(N_NEXT), h);
        m.set_u32(line.wrapping_add(L_HEAD), h0);
        m.set_f64(h0, m.f64(x_node));
        m.set_f64(h0 + 8, m.f64(x_node + 8));
        m.set_i32(h0.wrapping_add(N_FLAG), m.i32(x_node.wrapping_add(N_FLAG)));
        m.set_u32(x_node.wrapping_add(N_NEXT), 0);
        let mut edi = h0;
        cx = true;
        let mut edx = m.u32(edi.wrapping_add(N_NEXT));
        let s = m.i32(line.wrapping_add(L_SIGN));
        m.set_i32(line.wrapping_add(L_SIGN), s.wrapping_neg());
        while edx != 0 {
            let fl = m.u32(edx.wrapping_add(N_FLAG));
            if fl & 0x100 != 0 {
                cnt = 0;
                c40 = edi;
            } else if fl & 0x200 != 0 {
                esi = edx;
                if m.u32(esi.wrapping_add(N_NEXT)) == 0 {
                    break;
                }
                if !cx {
                    cx = true;
                } else {
                    let e2 = cut(m, o, c40, esi, cnt, c3c, c3c);
                    edx = m.u32(e2.wrapping_add(N_NEXT));
                    cx = false;
                }
            }
            cnt = cnt.wrapping_add(1);
            edi = edx;
            edx = m.u32(edx.wrapping_add(N_NEXT));
        }
        let _ = edi;
    } else {
        let mut edi = m.u32(line.wrapping_add(L_HEAD));
        let mut edx = m.u32(edi.wrapping_add(N_NEXT));
        'la: while edx != 0 {
            let fl = m.u32(edx.wrapping_add(N_FLAG));
            if fl & 0x100 != 0 {
                cnt = 0;
                c40 = edi;
            } else if fl & 0x200 != 0 {
                esi = edx;
                let n1 = m.u32(esi.wrapping_add(N_NEXT));
                if m.u32(n1.wrapping_add(N_NEXT)) == 0 {
                    break 'la;
                }
                if !cx {
                    cx = true;
                } else {
                    let e2 = cut(m, o, c40, esi, cnt, c38, c38);
                    edx = m.u32(e2.wrapping_add(N_NEXT));
                    cx = false;
                }
            }
            cnt = cnt.wrapping_add(1);
            edi = edx;
            edx = m.u32(edx.wrapping_add(N_NEXT));
        }
        // the two closing walks
        let ev = m.f64(e_int);
        m.set_u32(line.wrapping_add(L_CUR), c40);
        let cn = m.u32(c40.wrapping_add(N_NEXT));
        let r = liner_interp(m.f64(c40 + o_a), m.f64(c40 + o_b), m.f64(cn + o_a), m.f64(cn + o_b), ev).to_f64();
        let ra = rec(m.f64(c40 + o_a), r);
        let rb = rec(m.f64(cn + o_a), r);
        let sp = m.sp();
        add_boundary(m, o, line, c38, c3c, ra, rb);
        m.set_sp(sp);
        let ev = m.f64(e_int);
        m.set_u32(line.wrapping_add(L_CUR), esi);
        let sn = m.u32(esi.wrapping_add(N_NEXT));
        let r = liner_interp(m.f64(esi + o_a), m.f64(esi + o_b), m.f64(sn + o_a), m.f64(sn + o_b), ev).to_f64();
        let ra = rec(m.f64(esi + o_a), r);
        let rb = rec(m.f64(sn + o_a), r);
        let sp = m.sp();
        add_boundary(m, o, line, c3c, c38, ra, rb);
        m.set_sp(sp);
    }
    m.set_f64(line.wrapping_add(0x10), m.f64(U_S));
    m.set_f64(line.wrapping_add(0x20), m.f64(U_E));
    m.set_f64(line.wrapping_add(0x28), m.f64(V_S));
    m.set_f64(line.wrapping_add(0x18), m.f64(V_E));
    if 0 < m.i32(line.wrapping_add(L_SIGN)) {
        o.call(m, SI_TRM_CVR_RVE, &[line]);
    }
}

/// `a <= b` ordered (false when unordered), as `fcomp; and ah,0x45; dec ah; cmp ah,0x40; jb`.
fn le_ord(a: F80, b: F80) -> bool {
    a.lt(b) || a.eq(b)
}

/// [soft 0x084f46e0 handleUCsdMulIntsct]: `handleUCsdMulIntsct(st, idx)`: handles a trim line whose
/// crossings with the seam of the closed range are several (shared body `handle_csd_mul_intsct`,
/// axis 0: the seam edges 1 and 2).
pub fn handle_u_csd_mul_intsct(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32) {
    handle_csd_mul_intsct(m, o, st, idx, 0)
}

/// [soft 0x084f4ce0 handleVCsdMulIntsct]: `handleVCsdMulIntsct(st, idx)`: the same on the other axis
/// (seam edges 3 and 4).
pub fn handle_v_csd_mul_intsct(m: &mut Mem, o: &mut dyn Outside, st: u32, idx: i32) {
    handle_csd_mul_intsct(m, o, st, idx, 1)
}

/// Which way round the border a boundary walk goes, and the order of the edges it meets.
#[derive(Clone, Copy, PartialEq)]
enum Way {
    Cw,
    Ccw,
}

/// The border walk shared by `addCWBoundaryEdge` and `addCCWBoundaryEdge`, whose edge
/// numbers are 1 (u = uS), 2 (u = uE), 3 (v = vS), 4 (v = vE). Clockwise the edges come in
/// the order 1, 4, 2, 3; each full edge is walked with `addUEgSmp`/`addVEgSmp` from one
/// `EgInc` past its start corner (the sums on the register, rounded when passed); the end
/// is a walk along the last edge from `EgInc` past its start corner to `q` when `q` is
/// beyond that, else only `q` (`si_trmDataIns`). On a walk that starts and ends on the same
/// edge with `q` ahead of `p` the walk is direct.
fn border_walk(m: &mut Mem, o: &mut dyn Outside, way: Way, line: u32, e1: i32, e2: i32, p: u32, q: u32) {
    // the edges in walking order and the data of each: (fixed on u?, fixed value, from, to)
    let order: [i32; 4] = if way == Way::Cw { [1, 4, 2, 3] } else { [1, 3, 2, 4] };
    if !(1..=4).contains(&e1) || !(1..=4).contains(&e2) {
        return;
    }
    let (us, ue, vs, ve) = (g(m, U_S), g(m, U_E), g(m, V_S), g(m, V_E));
    let (ui, vi) = (g(m, U_EG_INC), g(m, V_EG_INC));
    let (pu, pv, qu, qv) = (m.f64(p), m.f64(p + 8), m.f64(q), m.f64(q + 8));
    // legs along u-edges (1, 2) run in v with `V`; along v-edges (3, 4) in u with `U`
    // The leg from the point `from` to the end of the edge, and the full edge legs.
    let cw = way == Way::Cw;
    let start = |m: &mut Mem, o: &mut dyn Outside, e: i32| match (e, cw) {
        (1, true) => add_v_eg_smp(m, o, line, us.to_f64(), pv, ve.to_f64()),
        (2, true) => add_v_eg_smp(m, o, line, ue.to_f64(), pv, vs.to_f64()),
        (3, true) => add_u_eg_smp(m, o, line, vs.to_f64(), pu, us.to_f64()),
        (4, true) => add_u_eg_smp(m, o, line, ve.to_f64(), pu, ue.to_f64()),
        (1, false) => add_v_eg_smp(m, o, line, us.to_f64(), pv, vs.to_f64()),
        (2, false) => add_v_eg_smp(m, o, line, ue.to_f64(), pv, ve.to_f64()),
        (3, false) => add_u_eg_smp(m, o, line, vs.to_f64(), pu, ue.to_f64()),
        _ => add_u_eg_smp(m, o, line, ve.to_f64(), pu, us.to_f64()),
    };
    let (w_cw, w_ccw) = (way == Way::Cw, way == Way::Ccw);
    let _ = (w_cw, w_ccw);
    let full = |m: &mut Mem, o: &mut dyn Outside, e: i32| {
        if way == Way::Cw {
            match e {
                4 => add_u_eg_smp(m, o, line, ve.to_f64(), (us + ui).to_f64(), ue.to_f64()),
                2 => add_v_eg_smp(m, o, line, ue.to_f64(), (ve - vi).to_f64(), vs.to_f64()),
                3 => add_u_eg_smp(m, o, line, vs.to_f64(), (ue - ui).to_f64(), us.to_f64()),
                _ => add_v_eg_smp(m, o, line, us.to_f64(), (vs + vi).to_f64(), ve.to_f64()),
            }
        } else {
            match e {
                3 => add_u_eg_smp(m, o, line, vs.to_f64(), (us + ui).to_f64(), ue.to_f64()),
                2 => add_v_eg_smp(m, o, line, ue.to_f64(), (vs + vi).to_f64(), ve.to_f64()),
                4 => add_u_eg_smp(m, o, line, ve.to_f64(), (ue - ui).to_f64(), us.to_f64()),
                _ => add_v_eg_smp(m, o, line, us.to_f64(), (ve - vi).to_f64(), vs.to_f64()),
            }
        }
    };
    // the end on edge e2
    let finish = |m: &mut Mem, o: &mut dyn Outside, e: i32| {
        let (qvx, qux) = (x(qv), x(qu));
        if way == Way::Cw {
            match e {
                1 => {
                    let t = vs + vi;
                    if qvx.gt(t) { add_v_eg_smp(m, o, line, us.to_f64(), t.to_f64(), qv) } else { data_ins(m, o, line, us.to_f64(), qv) }
                }
                2 => {
                    let t = ve - vi;
                    if qvx.lt(t) { add_v_eg_smp(m, o, line, ue.to_f64(), t.to_f64(), qv) } else { data_ins(m, o, line, ue.to_f64(), qv) }
                }
                3 => {
                    let t = ue - ui;
                    if qux.lt(t) { add_u_eg_smp(m, o, line, vs.to_f64(), t.to_f64(), qu) } else { data_ins(m, o, line, qu, vs.to_f64()) }
                }
                _ => {
                    let t = us + ui;
                    if qux.gt(t) { add_u_eg_smp(m, o, line, ve.to_f64(), t.to_f64(), qu) } else { data_ins(m, o, line, qu, ve.to_f64()) }
                }
            }
        } else {
            match e {
                1 => {
                    let t = ve - vi;
                    if qvx.lt(t) { add_v_eg_smp(m, o, line, us.to_f64(), t.to_f64(), qv) } else { data_ins(m, o, line, us.to_f64(), qv) }
                }
                2 => {
                    let t = vs + vi;
                    if qvx.gt(t) { add_v_eg_smp(m, o, line, ue.to_f64(), t.to_f64(), qv) } else { data_ins(m, o, line, ue.to_f64(), qv) }
                }
                3 => {
                    let t = us + ui;
                    // the walk from edge 1 tests the other way round
                    let walk = if e1 == 1 { qux.lt(t) } else { qux.gt(t) };
                    if walk { add_u_eg_smp(m, o, line, vs.to_f64(), t.to_f64(), qu) } else { data_ins(m, o, line, qu, vs.to_f64()) }
                }
                _ => {
                    let t = ue - ui;
                    let walk = if e1 == 1 { qux.gt(t) } else { qux.lt(t) };
                    if walk { add_u_eg_smp(m, o, line, ve.to_f64(), t.to_f64(), qu) } else { data_ins(m, o, line, qu, ve.to_f64()) }
                }
            }
        }
    };
    let i1 = order.iter().position(|&e| e == e1).unwrap();
    if e1 == e2 {
        // direct when q is ahead of p on the edge
        let direct = match (way, e1) {
            (Way::Cw, 1) | (Way::Ccw, 2) => x(qv).gt(x(pv)),
            (Way::Cw, 2) | (Way::Ccw, 1) => x(qv).lt(x(pv)),
            (Way::Cw, 3) | (Way::Ccw, 4) => x(qu).lt(x(pu)),
            _ => x(qu).gt(x(pu)),
        };
        if direct {
            match e1 {
                1 => add_v_eg_smp(m, o, line, us.to_f64(), pv, qv),
                2 => add_v_eg_smp(m, o, line, ue.to_f64(), pv, qv),
                3 => add_u_eg_smp(m, o, line, vs.to_f64(), pu, qu),
                _ => add_u_eg_smp(m, o, line, ve.to_f64(), pu, qu),
            }
            return;
        }
        start(m, o, e1);
        for k in 1..4 {
            full(m, o, order[(i1 + k) % 4]);
        }
        finish(m, o, e2);
        return;
    }
    start(m, o, e1);
    let mut k = 1;
    while order[(i1 + k) % 4] != e2 {
        full(m, o, order[(i1 + k) % 4]);
        k += 1;
    }
    finish(m, o, e2);
}

/// [soft 0x084f2de0 addCWBoundaryEdge]: `addCWBoundaryEdge(line, e1, e2, p, q)`: adds to the line
/// the points of the walk round the border of the parameter range, clockwise (edge order 1, 4,
/// 2, 3 with 1 = u start, 2 = u end, 3 = v start, 4 = v end), from the point `p` on edge `e1` to
/// the point `q` on edge `e2` (see `border_walk`).
pub fn add_cw_boundary_edge(m: &mut Mem, o: &mut dyn Outside, line: u32, e1: i32, e2: i32, p: u32, q: u32) {
    border_walk(m, o, Way::Cw, line, e1, e2, p, q)
}

/// [soft 0x084f38c0 addCCWBoundaryEdge]: `addCCWBoundaryEdge(line, e1, e2, p, q)`: the same walk as `add_cw_boundary_edge` the other way round
/// the border (edge order 1, 3, 2, 4).
pub fn add_ccw_boundary_edge(m: &mut Mem, o: &mut dyn Outside, line: u32, e1: i32, e2: i32, p: u32, q: u32) {
    border_walk(m, o, Way::Ccw, line, e1, e2, p, q)
}

// --- the line-segment intersection (second half of the file) -------------------------------

/// `|v| < 1e-5`, ordered (`fabs`, `fcomp`, then `and ah,0x45; cmp ah,1`).
fn small(v: F80) -> bool {
    lt(v.abs(), x(C_1E5))
}

/// [soft 0x084f055c si_lnSegsIntsect]: `si_lnSegsIntsect(p1 .. p14, out_u, out_v)`: the segment
/// (p1, p2)-(p3, p4) against the segment (p5, p6)-(p7, p8); (p9, p10, p11) is the line
/// `p9 x + p10 y + p11` of the first segment and (p12, p13, p14) that of the second. Answers 0 (no
/// contact), 1 (a proper crossing, the point written to both outs), 2 .. 6 (an end point touches or
/// the segments overlap: the end point written), 7 and 8 (a segment end lies on the other line,
/// nothing written). The line values are kept on the register; the four products of the crossing
/// are stored to the argument slots (rounded to `double`) and the quotients use the register `e`.
pub fn ln_segs_intsect(m: &mut Mem, p: [f64; 14], o1: u32, o2: u32) -> i32 {
    let (r, out) = ln_segs_core(p);
    if let Some((u, v)) = out {
        m.set_f64(o1, u);
        m.set_f64(o2, v);
    }
    r
}

/// The arithmetic of [`ln_segs_intsect`]: the answer and the point it writes (none for 0, 7, 8).
pub fn ln_segs_core(p: [f64; 14]) -> (i32, Option<(f64, f64)>) {
    let q: Vec<F80> = p.iter().map(|&v| x(v)).collect();
    let (p1, p2, p3, p4, p5, p6, p7, p8) = (q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]);
    let (p9, p10, p11, p12, p13, p14) = (q[8], q[9], q[10], q[11], q[12], q[13]);
    let (hi, lo) = (x(C_1E5), x(C_M1E5));
    let put = |u: F80, v: F80| Some((u.to_f64(), v.to_f64()));
    let a = (p9 * p5 + p10 * p6) + p11;
    let b = (p10 * p8 + p9 * p7) + p11;
    if gt(a, hi) {
        if gt(b, hi) {
            return (0, None);
        }
    } else if lt(a, lo) && lt(b, lo) {
        return (0, None);
    }
    let c = (p12 * p1 + p13 * p2) + p14;
    let d = (p13 * p4 + p12 * p3) + p14;
    if gt(c, hi) {
        if gt(d, hi) {
            return (0, None);
        }
    } else if lt(c, lo) && lt(d, lo) {
        return (0, None);
    }
    let e = p9 * p13 - p10 * p12;
    if !small(e) {
        if small(p7 - p3) && small(p8 - p4) {
            return (2, put(p3, p4));
        }
        if small(b) {
            let r = if !small(p7 - p1) || !small(p8 - p2) { 3 } else { 6 };
            return (r, put(p7, p8));
        }
        if small(d) {
            let r = if !small(p5 - p3) || !small(p6 - p4) { 4 } else { 5 };
            return (r, put(p3, p4));
        }
        if small(a) {
            return (7, None);
        }
        if small(c) {
            return (8, None);
        }
        let r = |v: F80| x(v.to_f64());
        let n10 = r(p10 * p14);
        let n13 = r(p13 * p11);
        let n12 = r(p12 * p11);
        let n9 = r(p9 * p14);
        return (1, Some((((n10 - n13) / e).to_f64(), ((n12 - n9) / e).to_f64())));
    }
    // parallel (or nearly so)
    if small(p7 - p3) && small(p8 - p4) {
        return (2, put(p3, p4));
    }
    if small(p5 - p3) && small(p6 - p4) {
        return (5, put(p3, p4));
    }
    if small(p7 - p1) && small(p8 - p2) {
        return (6, put(p7, p8));
    }
    let ux = if lt(p3 + hi, p1) { gt(p7, p3) && lt(p7, p1) } else { gt(p3 - hi, p1) && lt(p7, p3) && gt(p7, p1) };
    let uy = !ux
        && if lt(p4 + hi, p2) {
            gt(p8, p4) && lt(p8, p2)
        } else {
            gt(p4 - hi, p2) && lt(p8, p4) && gt(p8, p2)
        };
    if ux || uy {
        return (3, put(p7, p8));
    }
    let vx = if lt(p7 + hi, p5) { lt(p7, p3) && lt(p3, p5) } else { gt(p7 - hi, p5) && gt(p7, p3) && gt(p3, p5) };
    if !vx {
        let vy = if lt(p8 + hi, p6) { lt(p8, p4) && lt(p4, p6) } else { gt(p8 - hi, p6) && gt(p8, p4) && gt(p4, p6) };
        if !vy {
            return (0, None);
        }
    }
    (4, put(p3, p4))
}

/// `array[i]` of a table of doubles whose address is `base`.
fn at8(m: &Mem, base: u32, i: i32) -> f64 {
    m.f64(base.wrapping_add((i as u32).wrapping_mul(8)))
}

/// The squared distance `(a - x)^2 + (b - y)^2` on the register, from the point `(a, b)` to node `n`.
fn dist2(m: &Mem, a: F80, b: F80, n: u32) -> F80 {
    let du = a - g(m, n.wrapping_add(N_U));
    let dv = b - g(m, n.wrapping_add(N_V));
    du * du + dv * dv
}


/// [soft 0x084f0bb4 tCrvIntscted]: `tCrvIntscted(l1, l2, a3 .. a16, flagp, flag)`: finds where the
/// segments of trim line `l1` cross those of `l2`. `a3 .. a5` and `a10 .. a12` are arrays of the
/// line coefficients of each line's segments, `a6 .. a9` and `a13 .. a16` arrays of their boxes
/// (in the order `bboxesIntsect` takes them). The nodes whose mark is above 0 are skipped. For
/// two segments whose boxes meet `si_lnSegsIntsect` answers; when `flag` is 0 a crossing is entered in
/// both lines (the cursors first put on the segment starts, then on the following node nearest
/// to the point): 1 a new node in each (`newPt` against the cursor), both cursors marked with the
/// running count; 2 and 5 mark two touching nodes with the negated count, 6 the segment start and a
/// node; 3 and 4 enter a node that lies on the range border (or is not on the same side as the next
/// node) into the other line, marking it with the negated count, and 3 or 4 on the border sets
/// `*flagp` to 1. With a `flag` only the count moves (2 counts every touch, else at least 1). The
/// answer is the count. The last mark of each line is copied to the first node of the other.
#[allow(clippy::too_many_arguments)]
pub fn t_crv_intscted(m: &mut Mem, o: &mut dyn Outside, l1: u32, l2: u32, a: [u32; 14], flagp: u32, flag: i16) -> i32 {
    let box_of = |m: &Mem, l: u32| [m.f64(l + 0x10), m.f64(l + 0x18), m.f64(l + 0x20), m.f64(l + 0x28)];
    let bb = |b1: [f64; 4], b2: [f64; 4]| bboxes_intsect(b1[0], b1[1], b1[2], b1[3], b2[0], b2[1], b2[2], b2[3]) as i16 != 0;
    let mark = |m: &Mem, n: u32| m.i32(n.wrapping_add(N_FLAG));
    let set_mark = |m: &mut Mem, n: u32, v: i32| m.set_i32(n.wrapping_add(N_FLAG), v);
    let nxt = |m: &Mem, n: u32| m.u32(n.wrapping_add(N_NEXT));
    let pt = |m: &Mem, n: u32| (m.f64(n.wrapping_add(N_U)), m.f64(n.wrapping_add(N_V)));
    let mut count: i32 = 0;
    // the two stack doubles `lnSegsIntsect` writes: they keep the last value written (taken as 0
    // before the first; the original then reads what the stack held)
    let (mut qu, mut qv) = (0.0f64, 0.0f64);
    if !bb(box_of(m, l1), box_of(m, l2)) {
        return 0;
    }
    let mut p3 = m.u32(l1.wrapping_add(L_HEAD));
    let mut p11 = nxt(m, p3);
    let mut i1: i32 = 0;
    while p11 != 0 && mark(m, p11) > 0 {
        p11 = nxt(m, p11);
    }
    while p11 != 0 {
        let seg1_box = [at8(m, a[3], i1), at8(m, a[4], i1), at8(m, a[5], i1), at8(m, a[6], i1)];
        if bb(seg1_box, box_of(m, l2)) {
            let mut p10 = m.u32(l2.wrapping_add(L_HEAD));
            let mut p6 = nxt(m, p10);
            let mut i2: i32 = 0;
            while p6 != 0 {
                while p6 != 0 && mark(m, p6) > 0 {
                    p6 = nxt(m, p6);
                }
                let p9 = p6;
                if p9 == 0 {
                    break;
                }
                let seg2_box = [at8(m, a[10], i2), at8(m, a[11], i2), at8(m, a[12], i2), at8(m, a[13], i2)];
                let next_i2 = i2.wrapping_add(1);
                let mut r = 0;
                if bb(seg1_box, seg2_box) {
                    let (s3, s11, s10, s9) = (pt(m, p3), pt(m, p11), pt(m, p10), pt(m, p9));
                    let args = [
                        s3.0, s3.1, s11.0, s11.1, s10.0, s10.1, s9.0, s9.1,
                        at8(m, a[0], i1), at8(m, a[1], i1), at8(m, a[2], i1),
                        at8(m, a[7], i2), at8(m, a[8], i2), at8(m, a[9], i2),
                    ];
                    let (rr, out) = ln_segs_core(args);
                    r = rr;
                    if let Some((u, v)) = out {
                        qu = u;
                        qv = v;
                    }
                }
                if r != 0 && flag != 0 {
                    // only the count moves
                    if r == 2 {
                        count = count.wrapping_add(1);
                    } else if count < 1 {
                        count = 1;
                    }
                    p10 = p9;
                    p6 = nxt(m, p9);
                    i2 = next_i2;
                    continue;
                }
                if r != 0 {
                    // flag 0: the cursors, then the case
                    m.set_u32(l1.wrapping_add(L_CUR), p3);
                    m.set_u32(l2.wrapping_add(L_CUR), p10);
                    for (line, start, slot) in [(l1, p3, L_CUR), (l2, p10, L_CUR)] {
                        let mut n = nxt(m, start);
                        if mark(m, n) > 0 {
                            let (sx, sy) = pt(m, start);
                            let d1 = {
                                let (iu, iv) = (x(qu), x(qv));
                                let du = iu - x(sx);
                                let dv = iv - x(sy);
                                du * du + dv * dv
                            };
                            while mark(m, n) > 0 {
                                let d2 = dist2(m, g(m, n.wrapping_add(N_U)), g(m, n.wrapping_add(N_V)), start);
                                if lt(d2, d1) {
                                    m.set_u32(line.wrapping_add(slot), n);
                                }
                                n = nxt(m, n);
                            }
                        }
                    }
                }
                if r != 0 {
                    match r {
                        1 => {
                            let cur = m.u32(l1.wrapping_add(L_CUR));
                            let (cu, cv) = pt(m, cur);
                            if new_pt(qu, qv, cu, cv) as i16 != 0 {
                                data_ins(m, o, l1, qu, qv);
                            }
                            count = count.wrapping_add(1);
                            let cur = m.u32(l1.wrapping_add(L_CUR));
                            set_mark(m, cur, count);
                            let cur2 = m.u32(l2.wrapping_add(L_CUR));
                            let (cu, cv) = pt(m, cur2);
                            if new_pt(qu, qv, cu, cv) as i16 != 0 {
                                data_ins(m, o, l2, qu, qv);
                            }
                            let cur2 = m.u32(l2.wrapping_add(L_CUR));
                            set_mark(m, cur2, count);
                        }
                        2 => {
                            if mark(m, p11) == 0 {
                                count = count.wrapping_add(1);
                                set_mark(m, p11, count.wrapping_neg());
                                set_mark(m, p9, count.wrapping_neg());
                            }
                        }
                        3 => {
                            let (u, v) = pt(m, p9);
                            if at_edge(m, u, v) != 0 {
                                count = count.wrapping_add(1);
                                set_mark(m, p9, count.wrapping_neg());
                                data_ins(m, o, l1, u, v);
                                let cur = m.u32(l1.wrapping_add(L_CUR));
                                set_mark(m, cur, count);
                                m.set_u16(flagp, 1);
                            } else {
                                let nn = next_node(m, m.u32(l2.wrapping_add(L_HEAD)), p9);
                                if at_same_side(m, at8(m, a[0], i1), at8(m, a[1], i1), at8(m, a[2], i1), p10, nn) == 0 {
                                    count = count.wrapping_add(1);
                                    set_mark(m, p9, count.wrapping_neg());
                                    data_ins(m, o, l1, u, v);
                                    let cur = m.u32(l1.wrapping_add(L_CUR));
                                    set_mark(m, cur, count);
                                }
                            }
                        }
                        4 => {
                            let (u, v) = pt(m, p11);
                            if at_edge(m, u, v) != 0 {
                                count = count.wrapping_add(1);
                                set_mark(m, p11, count.wrapping_neg());
                                data_ins(m, o, l2, u, v);
                                let cur = m.u32(l2.wrapping_add(L_CUR));
                                set_mark(m, cur, count);
                                m.set_u16(flagp, 1);
                            } else {
                                let nn = next_node(m, m.u32(l1.wrapping_add(L_HEAD)), p11);
                                if at_same_side(m, at8(m, a[7], i2), at8(m, a[8], i2), at8(m, a[9], i2), p3, nn) == 0 {
                                    count = count.wrapping_add(1);
                                    set_mark(m, p11, count.wrapping_neg());
                                    data_ins(m, o, l2, u, v);
                                    let cur = m.u32(l2.wrapping_add(L_CUR));
                                    set_mark(m, cur, count);
                                }
                            }
                        }
                        5 => {
                            if mark(m, p11) == 0 {
                                count = count.wrapping_add(1);
                                set_mark(m, p11, count.wrapping_neg());
                                set_mark(m, p10, count.wrapping_neg());
                            }
                        }
                        6 => {
                            if mark(m, p9) == 0 {
                                count = count.wrapping_add(1);
                                set_mark(m, p3, count.wrapping_neg());
                                set_mark(m, p9, count.wrapping_neg());
                            }
                        }
                        _ => {}
                    }
                }
                p10 = p9;
                p6 = nxt(m, p9);
                i2 = next_i2;
            }
            if mark(m, p10) != 0 {
                let first2 = m.u32(l2.wrapping_add(L_HEAD));
                set_mark(m, first2, mark(m, p10));
            }
        }
        p3 = p11;
        p11 = nxt(m, p11);
        i1 = i1.wrapping_add(1);
        while p11 != 0 && mark(m, p11) > 0 {
            p11 = nxt(m, p11);
        }
    }
    if mark(m, p3) != 0 {
        let first1 = m.u32(l1.wrapping_add(L_HEAD));
        set_mark(m, first1, mark(m, p3));
    }
    count
}

// --- the conversions between a trim line and a trim curve, and the automatic border -------------

use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};

/// [soft 0x084f6cf8 si_convertTrimLineToTrimCrv]: the composite curve `comp` (its line at +0xc)
/// gets one linear curve of the line's points: +0x10 and +0x14 cleared, the boundary short (+0)
/// 0 for a line with a positive sign and 1 otherwise, one curve (+4) from `SI_calloc(1, 4)` (+8);
/// the curve (`SI_nurbsCrvAllocate`) has closing 1, order 2, `count - 1` points (`SI_nurbsPointsAllocate`)
/// that take the nodes' u and v (the word at +0x20 cleared), the short at +0x20 set to 1, +0x24
/// cleared, `count` knots (`si_knotsAllocate`) holding 0, 1, 2, ... and the range 0 to `count - 1`
/// (+0x10 and +0x18). The last node is not entered (the line closes on its first).
pub fn convert_trim_line_to_trim_crv(m: &mut Mem, comp: u32) {
    let line = m.u32(comp.wrapping_add(0xc));
    m.set_u32(comp.wrapping_add(0x10), 0);
    m.set_u32(comp.wrapping_add(0x14), 0);
    m.set_u16(comp, if m.i32(line.wrapping_add(L_SIGN)) > 0 { 0 } else { 1 });
    m.set_i32(comp.wrapping_add(4), 1);
    let arr = m.si_calloc(1, 4);
    m.set_u32(comp.wrapping_add(8), arr);
    let crv = nurbs_crv_allocate(m).addr();
    m.set_u32(arr, crv);
    m.set_i32(crv.wrapping_add(0x34), 1);
    m.set_i32(crv, 2);
    m.set_i32(crv.wrapping_add(4), m.i32(line).wrapping_sub(1));
    let pts = nurbs_points_allocate(m, m.i32(crv.wrapping_add(4)));
    m.set_u32(crv.wrapping_add(0x2c), pts);
    let mut node = m.u32(line.wrapping_add(L_HEAD));
    let mut i: i32 = 0;
    while i < m.i32(crv.wrapping_add(4)) {
        let at = m.u32(crv.wrapping_add(0x2c)).wrapping_add((i as u32).wrapping_mul(0x24));
        m.set_i32(at.wrapping_add(0x20), 0);
        let (u, v) = (m.f64(node.wrapping_add(N_U)), m.f64(node.wrapping_add(N_V)));
        m.set_f64(at, u);
        m.set_f64(at.wrapping_add(8), v);
        node = m.u32(node.wrapping_add(N_NEXT));
        i += 1;
    }
    m.set_u16(crv.wrapping_add(0x20), 1);
    m.set_i32(crv.wrapping_add(0x24), 0);
    m.set_i32(crv.wrapping_add(8), m.i32(crv.wrapping_add(4)).wrapping_add(1));
    let kn = knots_allocate(m, m.i32(crv.wrapping_add(8)));
    m.set_u32(crv.wrapping_add(0xc), kn);
    let mut i: i32 = 0;
    while i < m.i32(crv.wrapping_add(8)) {
        let k = m.u32(crv.wrapping_add(0xc));
        m.set_f64(k.wrapping_add((i as u32).wrapping_mul(8)), i as f64);
        i += 1;
    }
    m.set_u32(crv.wrapping_add(0x10), 0);
    m.set_u32(crv.wrapping_add(0x14), 0);
    let last = m.i32(crv.wrapping_add(8)).wrapping_sub(1);
    m.set_f64(crv.wrapping_add(0x18), last as f64);
}

/// [soft 0x084f6e0c si_handleAutoBorder]: `si_handleAutoBorder(srf, flag, linep)` on a surface (the
/// count of trim composites at +0x60, their table at +0x64, flags at +0x58 / +0x5b, the border short
/// at +0x5c; the box doubles +0x10, +0x18, +0x34, +0x3c). Always 1 except when a surface already has
/// trim composites and none of the old ones holds the new line `*linep`'s box (returns 0). With no
/// composites a flag (or bit 24 of +0x58) sets that bit and clears +0x5c. With composites, the
/// call is for the border composite when `flag` and the short (+0x5c) are not both zero; when the
/// global `trimIndex` (0x086da5a0) is -1 the composites of the sign asked for (negative for flag 0,
/// positive otherwise) are compared with the new line's box: one that contains it answers 1, one
/// contained by it with a flag sets bit 0 of +0x5b and clears +0x5c (answers 1), without a flag
/// builds a new border composite. The new border composite is a line of 0x30 bytes (sign -1, the
/// box copied from the surface) walked round the four edges by `addUEgSmp`/`addVEgSmp`, closed
/// with `si_trmDataIns` at the range start, converted by [`convert_trim_line_to_trim_crv`], the count
/// raised, bit 0 of +0x5b cleared and +0x5c set to 1.
pub fn handle_auto_border(m: &mut Mem, o: &mut dyn Outside, srf: u32, flag: i16, linep: u32) -> i32 {
    let n = m.i32(srf.wrapping_add(0x60));
    let make = if n == 0 {
        let f = m.u32(srf.wrapping_add(0x58));
        if flag != 0 || f & 0x100_0000 != 0 {
            m.set_u32(srf.wrapping_add(0x58), f | 0x100_0000);
            m.set_u16(srf.wrapping_add(0x5c), 0);
            return 1;
        }
        true
    } else {
        let sh = m.u16(srf.wrapping_add(0x5c));
        let go = if flag != 0 { sh != 0 } else { sh == 0 };
        if !go {
            return 1;
        }
        // the original's tests: flag != 0 && sh != 0 => 6e80; flag != 0 && sh == 0 => return 1;
        // flag == 0 && sh == 0 => 6e80; flag == 0 && sh != 0 => return 1
        let new = m.u32(linep);
        if m.i32(0x086d_a5a0) != -1 {
            return 1;
        }
        let le = |a: F80, b: F80| a.le(b);
        let mut found = false;
        let mut make = false;
        let mut i: i32 = 0;
        let cnt = m.i32(srf.wrapping_add(0x60));
        while i < cnt {
            let comp = m.u32(m.u32(srf.wrapping_add(0x64)).wrapping_add((i as u32).wrapping_mul(4)));
            let l = m.u32(comp.wrapping_add(0xc));
            let sign = m.i32(l.wrapping_add(4));
            let take = if flag == 0 { sign < 0 } else { sign > 0 };
            if take {
                let (a10, a18, a20, a28) = (g(m, new + 0x10), g(m, new + 0x18), g(m, new + 0x20), g(m, new + 0x28));
                let (l10, l18, l20, l28) = (g(m, l + 0x10), g(m, l + 0x18), g(m, l + 0x20), g(m, l + 0x28));
                if le(l10, a10) && le(a20, l20) && le(l28, a28) && le(a18, l18) {
                    found = true;
                    break;
                }
                if le(a10, l10) && le(l20, a20) && le(a28, l28) && le(l18, a18) {
                    if flag != 0 {
                        let b = m.u8(srf.wrapping_add(0x5b));
                        m.set_u8(srf.wrapping_add(0x5b), b | 1);
                        m.set_u16(srf.wrapping_add(0x5c), 0);
                        return 1;
                    }
                    make = true;
                    break;
                }
            }
            i += 1;
        }
        if !make {
            return if found { 1 } else { 0 };
        }
        true
    };
    if !make {
        return 1;
    }
    let tab = m.u32(srf.wrapping_add(0x64));
    let n = m.i32(srf.wrapping_add(0x60));
    let tab = if tab == 0 { m.si_calloc(1, 4) } else { m.si_realloc(tab, (n.wrapping_add(1) as u32).wrapping_mul(4)) };
    m.set_u32(srf.wrapping_add(0x64), tab);
    let comp = m.si_malloc(0x18);
    m.set_u32(tab.wrapping_add((n as u32).wrapping_mul(4)), comp);
    let line = m.si_calloc(1, 0x30);
    m.set_u32(comp.wrapping_add(0xc), line);
    m.set_i32(line.wrapping_add(4), -1);
    m.set_i32(line, 0);
    for (to, from) in [(0x10u32, 0x10u32), (0x20, 0x18), (0x28, 0x34), (0x18, 0x3c)] {
        let v = m.f64(srf.wrapping_add(from));
        m.set_f64(line.wrapping_add(to), v);
    }
    m.set_u32(line.wrapping_add(8), 0);
    let (us, ue, vs, ve) = (m.f64(U_S), m.f64(U_E), m.f64(V_S), m.f64(V_E));
    let (uinc, vinc) = (m.f64(0x0882_2248), m.f64(0x0882_2250));
    add_u_eg_smp(m, o, line, vs, us, ue);
    add_v_eg_smp(m, o, line, ue, (x(vs) + x(vinc)).to_f64(), ve);
    add_u_eg_smp(m, o, line, ve, (x(ue) - x(uinc)).to_f64(), us);
    add_v_eg_smp(m, o, line, us, (x(ve) - x(vinc)).to_f64(), (x(vs) + x(vinc)).to_f64());
    data_ins(m, o, line, us, vs);
    let comp = m.u32(m.u32(srf.wrapping_add(0x64)).wrapping_add((m.i32(srf.wrapping_add(0x60)) as u32).wrapping_mul(4)));
    convert_trim_line_to_trim_crv(m, comp);
    m.set_i32(srf.wrapping_add(0x60), m.i32(srf.wrapping_add(0x60)).wrapping_add(1));
    let b = m.u8(srf.wrapping_add(0x5b));
    m.set_u8(srf.wrapping_add(0x5b), b & 0xfe);
    m.set_u16(srf.wrapping_add(0x5c), 1);
    1
}

pub const SI_NURBS_CURVE_INTER_BLD_LIN_FROM_COMP: Routine = Routine::new("si_nurbsCurveInterBldLinFromComp", 0x0850_85e8);

/// [soft 0x084f71c0 SI_trimCrvToLineUpdate]: rebuilds the trim line of the composite `comp` from
/// its curves. The curve comes from `si_nurbsCurveInterBldLinFromComp` (0x085085e8, outside; asked with
/// the curve table at +8 and the count at +4); when there is one, an old line (+0xc) is deleted
/// ([`del_trim_line`]) and, when the count is not 0, a new line (0x30 bytes, `SI_calloc`) with the box
/// set to its empty extremes (+-3.4028234663852886e38, the minima high) gets every point of
/// the curve (`si_trmDataIns`), the first point once more when it is not the cursor point
/// ([`new_pt`]), its box (`si_trmCrvBBox`) and the sign 1 for a boundary short of 0 and -1
/// otherwise; the curve is disposed.
pub fn trim_crv_to_line_update(m: &mut Mem, o: &mut dyn Outside, comp: u32) {
    if comp == 0 {
        return;
    }
    let slot = m.local(4);
    let crv = o.call(m, SI_NURBS_CURVE_INTER_BLD_LIN_FROM_COMP, &[m.u32(comp.wrapping_add(8)), m.u32(comp.wrapping_add(4)), slot]).eax;
    m.set_u32(slot, crv);
    if crv == 0 {
        return;
    }
    if m.u32(comp.wrapping_add(0xc)) != 0 {
        del_trim_line(m, comp);
    }
    if m.i32(comp.wrapping_add(4)) != 0 {
        let line = m.si_calloc(1, 0x30);
        m.set_u32(comp.wrapping_add(0xc), line);
        let big = f32::MAX as f64;
        m.set_f64(line.wrapping_add(0x28), big);
        m.set_f64(line.wrapping_add(0x10), big);
        m.set_f64(line.wrapping_add(0x18), -big);
        m.set_f64(line.wrapping_add(0x20), -big);
        m.set_i32(line.wrapping_add(4), 0);
        m.set_i32(line, 0);
        m.set_i32(line.wrapping_add(8), 0);
        let mut at = m.u32(crv.wrapping_add(0x2c));
        let mut i: i32 = 0;
        while i < m.i32(crv.wrapping_add(4)) {
            let (u, v) = (m.f64(at), m.f64(at.wrapping_add(8)));
            data_ins(m, o, line, u, v);
            i += 1;
            at = at.wrapping_add(0x24);
        }
        let (h, c) = (m.u32(line.wrapping_add(L_HEAD)), m.u32(line.wrapping_add(L_CUR)));
        let r = new_pt(m.f64(h), m.f64(h.wrapping_add(8)), m.f64(c), m.f64(c.wrapping_add(8)));
        if r as i16 != 0 {
            let h = m.u32(line.wrapping_add(L_HEAD));
            let (u, v) = (m.f64(h), m.f64(h.wrapping_add(8)));
            data_ins(m, o, line, u, v);
        }
        o.call(m, SI_TRM_CRV_BBOX, &[line]);
        m.set_i32(line.wrapping_add(4), if m.u16(comp) == 0 { 1 } else { -1 });
        nurbs_crv_dispose(m, slot);
    }
}

// --- the merge of intersecting trim curves --------------------------------------------------

const TRIM_INDEX: u32 = 0x086d_a5a0;

/// [soft 0x084f1af0 mergerIntesectedTCrv]: `mergerIntesectedTCrv(st)` on the status record (count
/// +0x24, table of lines +0x114, the surface at +0x78 with the composite count at +0x60 and their
/// table at +0x64). Answers 0, 1 (two lines cross at a point that is a segment end of one of them)
/// or 2 (a line crosses another twice, or a composite does).
///
/// Seven tables of per-line coefficient arrays are built (`fillTheCoefs`, arrays of `SI_calloc(count, 8)`;
/// a line without a second node gets none). Every pair of lines of the table is tried with
/// `tCrvIntscted` (flag 0): a pair that crosses is joined by `travelingIntCrvs` and both lines are
/// deleted (`si_delTrimLine`, through `o`), but a second hit for the first line stops with 2, and a
/// hit at a segment end with 1. When that gave 0 and the surface has composites, each composite's
/// line is tried against the lines `n ..` the joins added (flag 1: only the count, a hit gives 2)
/// and against the old ones (flag 1: a hit at a segment end gives 1, a second hit for the line
/// gives 2); with still 0, each composite is tried against the old lines for real (flag 0) and a
/// hit joins them, the composite is disposed and rebuilt from the joined line
/// (`si_convertTrimLineToTrimCrv`), the global `trimIndex` set to its index. Then the arrays are
/// freed, `reorderTrimStat` closes up the table and, if a crossing met the range border, bit 0 of
/// the surface's byte at +0x5b is set and its short at +0x5c cleared.
///
/// After a join the loops go on with the first line's pointer as it was (freed): the original
/// does, and so does this.
pub fn merger_intersected_t_crv(m: &mut Mem, o: &mut dyn Outside, st: u32) -> i32 {
    let mut r: i32 = 0;
    let flagp = m.local(2);
    m.set_u16(flagp, 0);
    let obj = m.u32(st.wrapping_add(0x78));
    let n = m.i32(st.wrapping_add(ST_COUNT));
    let w = |i: i32| (i as u32).wrapping_mul(4);
    let calloc = |m: &mut Mem, c: i32, sz: u32| m.si_calloc(c as u32, sz);
    // the seven tables of per-line arrays, and the marks
    let mut pt = [0u32; 7];
    for p in pt.iter_mut() {
        *p = calloc(m, n, 4);
    }
    let marks = calloc(m, n, 4);
    let slot = |m: &Mem, t: u32, i: i32| m.u32(t.wrapping_add(w(i)));
    let line_at = |m: &Mem, i: i32| m.u32(m.u32(st.wrapping_add(ST_TABLE)).wrapping_add(w(i)));
    let mut i: i32 = 0;
    if 0 < n {
        loop {
            let line = line_at(m, i);
            let head = m.u32(line.wrapping_add(L_HEAD));
            if head == 0 || m.u32(head.wrapping_add(N_NEXT)) == 0 {
                for k in 0..6 {
                    m.set_u32(pt[k].wrapping_add(w(i)), 0);
                }
            } else {
                let cnt = m.i32(line);
                for k in 0..7 {
                    let a = calloc(m, cnt, 8);
                    m.set_u32(pt[k].wrapping_add(w(i)), a);
                }
                let a: Vec<u32> = (0..7).map(|k| slot(m, pt[k], i)).collect();
                fill_the_coefs(m, line, a[0], a[1], a[2], a[3], a[4], a[5], a[6]);
            }
            i += 1;
            if i >= n {
                break;
            }
        }
    }
    let mut i: i32 = 0;
    if 0 < n {
        loop {
            let line = line_at(m, i);
            clear_tln_flag(m, line);
            i += 1;
            if i >= n {
                break;
            }
        }
    }
    let arrs = |m: &Mem, i: i32| -> [u32; 7] { std::array::from_fn(|k| slot(m, pt[k], i)) };
    let cat = |a: [u32; 7], b: [u32; 7]| -> [u32; 14] { std::array::from_fn(|k| if k < 7 { a[k] } else { b[k - 7] }) };
    // every pair of lines
    let mut i: i32 = 0;
    if 0 < n - 1 {
        loop {
            let l1 = line_at(m, i);
            if l1 != 0 {
                m.set_u32(marks.wrapping_add(w(i)), 0);
                let mut j = i + 1;
                while j < n {
                    let l2 = line_at(m, j);
                    if l2 != 0 {
                        let a = cat(arrs(m, i), arrs(m, j));
                        let k = t_crv_intscted(m, o, l1, l2, a, flagp, 0);
                        if k > 0 {
                            if k >= m.i32(l1).wrapping_sub(1) || k >= m.i32(l2).wrapping_sub(1) {
                                i = n;
                                r = 1;
                                break;
                            }
                            let mk = m.u32(marks.wrapping_add(w(i)));
                            m.set_u32(marks.wrapping_add(w(i)), mk.wrapping_add(1));
                            if mk != 0 {
                                i = n;
                                r = 2;
                                break;
                            }
                            traveling_int_crvs(m, o, st, l1, l2);
                            o.call(m, SI_DEL_TRIM_LINE, &[l1]);
                            let t = m.u32(st.wrapping_add(ST_TABLE));
                            m.set_u32(t.wrapping_add(w(i)), 0);
                            o.call(m, SI_DEL_TRIM_LINE, &[l2]);
                            let t = m.u32(st.wrapping_add(ST_TABLE));
                            m.set_u32(t.wrapping_add(w(j)), 0);
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
            if i >= n - 1 {
                break;
            }
        }
    }
    // the composites of the surface
    let ccount = m.i32(obj.wrapping_add(0x60));
    if ccount > 0 && r == 0 {
        m.set_i32(TRIM_INDEX, -1);
        let comp_line = |m: &Mem, i: i32| m.u32(m.u32(m.u32(obj.wrapping_add(0x64)).wrapping_add(w(i))).wrapping_add(0xc));
        let mut m1: i32 = 0;
        let mut i: i32 = 0;
        loop {
            let l = comp_line(m, i);
            clear_tln_flag(m, l);
            let c = m.i32(l);
            if m1 < c {
                m1 = c;
            }
            i += 1;
            if i >= m.i32(obj.wrapping_add(0x60)) {
                break;
            }
        }
        let q: [u32; 7] = std::array::from_fn(|_| calloc(m, m1, 8));
        let mut m2: i32 = 0;
        let mut i = n;
        while i < m.i32(st.wrapping_add(ST_COUNT)) {
            let l = line_at(m, i);
            clear_tln_flag(m, l);
            let c = m.i32(l);
            if m2 < c {
                m2 = c;
            }
            i += 1;
        }
        let rr: [u32; 7] = std::array::from_fn(|_| calloc(m, m2, 8));
        let mut i: i32 = 0;
        while i < m.i32(obj.wrapping_add(0x60)) {
            let cl = comp_line(m, i);
            fill_the_coefs(m, cl, q[0], q[1], q[2], q[3], q[4], q[5], q[6]);
            let mut b = n;
            while b < m.i32(st.wrapping_add(ST_COUNT)) {
                let lb = line_at(m, b);
                fill_the_coefs(m, lb, rr[0], rr[1], rr[2], rr[3], rr[4], rr[5], rr[6]);
                let k = t_crv_intscted(m, o, cl, lb, cat(q, rr), flagp, 1);
                if k != 0 {
                    i = m.i32(obj.wrapping_add(0x60));
                    r = 2;
                    break;
                }
                b += 1;
            }
            let mut b: i32 = 0;
            while b < n {
                let l2 = line_at(m, b);
                if l2 != 0 {
                    let k = t_crv_intscted(m, o, cl, l2, cat(q, arrs(m, b)), flagp, 1);
                    if k >= m.i32(cl).wrapping_sub(1) || k >= m.i32(l2).wrapping_sub(1) {
                        i = n;
                        r = 1;
                        break;
                    }
                    if k > 0 {
                        if m.u32(marks.wrapping_add(w(b))) != 0 {
                            i = m.i32(obj.wrapping_add(0x60));
                            r = 2;
                            break;
                        }
                        m.set_u32(marks.wrapping_add(w(b)), 1);
                    }
                }
                b += 1;
            }
            i += 1;
        }
        if r == 0 {
            let mut i: i32 = 0;
            if 0 < m.i32(obj.wrapping_add(0x60)) {
                loop {
                    let cl = comp_line(m, i);
                    fill_the_coefs(m, cl, q[0], q[1], q[2], q[3], q[4], q[5], q[6]);
                    let next_i = i + 1;
                    let mut b: i32 = 0;
                    while b < n {
                        let l2 = line_at(m, b);
                        if l2 != 0 {
                            let k = t_crv_intscted(m, o, cl, l2, cat(q, arrs(m, b)), flagp, 0);
                            if k > 0 {
                                traveling_int_crvs(m, o, st, cl, l2);
                                let tp = m.u32(obj.wrapping_add(0x64)).wrapping_add(w(i));
                                crate::image::comp_dispose(m, tp);
                                let c = m.i32(st.wrapping_add(ST_COUNT)).wrapping_sub(1);
                                m.set_i32(st.wrapping_add(ST_COUNT), c);
                                m.set_i32(TRIM_INDEX, i);
                                let nc = m.si_malloc(0x18);
                                let tp = m.u32(obj.wrapping_add(0x64)).wrapping_add(w(i));
                                m.set_u32(tp, nc);
                                let last = line_at(m, c);
                                m.set_u32(nc.wrapping_add(0xc), last);
                                convert_trim_line_to_trim_crv(m, nc);
                                let t = m.u32(st.wrapping_add(ST_TABLE));
                                m.set_u32(t.wrapping_add(w(c)), 0);
                                o.call(m, SI_DEL_TRIM_LINE, &[l2]);
                                let t = m.u32(st.wrapping_add(ST_TABLE));
                                m.set_u32(t.wrapping_add(w(b)), 0);
                            }
                        }
                        b += 1;
                    }
                    i = next_i;
                    if i >= m.i32(obj.wrapping_add(0x60)) {
                        break;
                    }
                }
            }
        }
        for p in q.iter().chain(rr.iter()) {
            if *p != 0 {
                m.free(*p);
            }
        }
    }
    let mut i: i32 = 0;
    while i < n {
        for k in 0..7 {
            let a = slot(m, pt[k], i);
            if a != 0 {
                m.free(a);
                m.set_u32(pt[k].wrapping_add(w(i)), 0);
            }
        }
        i += 1;
    }
    for p in pt.iter().chain(std::iter::once(&marks)) {
        if *p != 0 {
            m.free(*p);
        }
    }
    reorder_trim_stat(m, o, st);
    if m.u16(flagp) != 0 && r == 0 {
        m.set_u16(obj.wrapping_add(0x5c), 0);
        let b = m.u8(obj.wrapping_add(0x5b));
        m.set_u8(obj.wrapping_add(0x5b), b | 1);
    }
    r
}

// --- the entry of the trim: detecting where the lines meet the border ------------------------

use crate::trimutils::{si_get_edge_steps, si_trm_cvr_rve, si_trm_crv_bbox, si_trm_data_ins, si_unique_d};

const U_HEG_INC: u32 = 0x0882_2258;

/// [soft 0x084f68e8 si_detectEdgeCross]: `si_detectEdgeCross(st)`, `st` the status record (the surface at
/// +0x78, count +0x24, lines +0x114, a short at +2). Sets the range globals from the surface
/// (`uS`, `uE`, `vS`, `vE`, `uClosed`, `vClosed`, `uEgInc`, `vEgInc` from `si_getEdgeSteps`, their halves, the
/// tolerances `uEps` = (uE - uS) * 0.05, `vEps` likewise, `epsU` = `uEps` * 1e-4, `epsV`), builds the edge
/// sample tables `uSmp` (when the surface is closed in v) and `vSmp` (closed in u) from the quotient
/// `(range + eps) / EgInc` truncated, clears bit 0 of the byte at +0x5b of a surface without
/// composites, `reorderTrimStat`, then `trimHalfPeak` (one line) or `trimPeak` (two). Every line
/// is closed (`si_uniqueD` of its end and its start: a copy of the start is appended when they
/// are different, else the end is made the start) and boxed (`si_trmCrvBBox`). Then each line
/// is classified by `trmCrvAEdg`: 0 `trmCrvRevAEdg`, 2 reversed by the turning direction
/// (`SI_trmCvrRve`: positive sign when the short at +2 is set, else negative), 3 and 4 the
/// multiple-crossing handlers, 5 and 6 end the routine with 3 and 4. When all pass the answer is that of
/// `mergerIntesectedTCrv`.
pub fn si_detect_edge_cross(m: &mut Mem, o: &mut dyn Outside, st: u32) -> i32 {
    use si_x87::Round;
    let obj = m.u32(st.wrapping_add(0x78));
    let f = |m: &Mem, at: u32| m.f64(obj.wrapping_add(at));
    m.set_f64(U_S, f(m, 0x10));
    m.set_f64(V_S, f(m, 0x34));
    m.set_f64(U_E, f(m, 0x18));
    m.set_f64(V_E, f(m, 0x3c));
    m.set_u16(U_CLOSED, m.u16(obj.wrapping_add(0x20)));
    m.set_u16(V_CLOSED, m.u16(obj.wrapping_add(0x44)));
    let ui = si_get_edge_steps(m, m.u32(obj.wrapping_add(0xc)), m.i32(obj.wrapping_add(8)), m.i32(obj.wrapping_add(0x70)));
    m.set_f64(U_EG_INC, ui.to_f64());
    let vi = si_get_edge_steps(m, m.u32(obj.wrapping_add(0x30)), m.i32(obj.wrapping_add(0x2c)), m.i32(obj.wrapping_add(0x74)));
    m.set_f64(V_EG_INC, vi.to_f64());
    let (c_half, c_5, c_4) = (x(0.5), x(0.05), x(0.0001));
    let ui64 = g(m, U_EG_INC);
    let uhalf = ui64 * c_half;
    let vhalf = vi * c_half;
    m.set_f64(VH_EG_INC, vhalf.to_f64());
    let du = g(m, U_E) - g(m, U_S);
    m.set_f64(U_HEG_INC, uhalf.to_f64());
    let dv = g(m, V_E) - g(m, V_S);
    let a = du * c_5;
    let b = c_5 * dv;
    m.set_f64(U_EPS, a.to_f64());
    let e = a * c_4;
    let d = du + e;
    let n1 = d / ui64;
    m.set_f64(V_EPS, b.to_f64());
    let fv = c_4 * b;
    m.set_f64(EPS_U, e.to_f64());
    m.set_f64(EPS_V, fv.to_f64());
    let edi = n1.to_i32(Round::Zero);
    let ebx = edi.wrapping_add(1);
    if m.u16(V_CLOSED) != 0 {
        let t = m.si_calloc(ebx as u32, 8);
        m.set_u32(U_SMP, t);
        let (us, ue) = (m.f64(U_S), m.f64(U_E));
        m.set_f64(t, us);
        m.set_f64(t.wrapping_add((ebx as u32).wrapping_mul(8)).wrapping_sub(8), ue);
        let mut acc = g(m, U_EG_INC);
        let mut i: i32 = 1;
        while i < edi {
            let t = m.u32(U_SMP);
            m.set_f64(t.wrapping_add((i as u32).wrapping_mul(8)), acc.to_f64());
            acc = acc + g(m, U_EG_INC);
            i += 1;
        }
    }
    let q = (g(m, V_E) - g(m, V_S) + g(m, EPS_V)) / g(m, V_EG_INC);
    let edi = q.to_i32(Round::Zero);
    let ebx = edi.wrapping_add(1);
    if m.u16(U_CLOSED) != 0 {
        let t = m.si_calloc(ebx as u32, 8);
        m.set_u32(V_SMP, t);
        let (vs, ve) = (m.f64(V_S), m.f64(V_E));
        m.set_f64(t, vs);
        m.set_f64(t.wrapping_add((ebx as u32).wrapping_mul(8)).wrapping_sub(8), ve);
        let mut acc = g(m, V_EG_INC);
        let mut i: i32 = 1;
        while i < edi {
            let t = m.u32(V_SMP);
            m.set_f64(t.wrapping_add((i as u32).wrapping_mul(8)), acc.to_f64());
            acc = acc + g(m, V_EG_INC);
            i += 1;
        }
    }
    if m.i32(obj.wrapping_add(0x60)) == 0 {
        let b = m.u8(obj.wrapping_add(0x5b));
        m.set_u8(obj.wrapping_add(0x5b), b & 0xfe);
    }
    reorder_trim_stat(m, o, st);
    match m.i32(st.wrapping_add(ST_COUNT)) {
        1 => {
            let l = tab(m, st, 0);
            trim_half_peak(m, o, l);
        }
        2 => trim_peak(m, o, st),
        _ => {}
    }
    let mut i: i32 = 0;
    while i < m.i32(st.wrapping_add(ST_COUNT)) {
        let line = tab(m, st, i);
        let (head, cur) = (m.u32(line.wrapping_add(L_HEAD)), m.u32(line.wrapping_add(L_CUR)));
        let r = si_unique_d(m, obj, m.f64(cur), m.f64(cur.wrapping_add(8)), m.f64(head), m.f64(head.wrapping_add(8)));
        if r == 1 {
            let (hu, hv) = (m.f64(head), m.f64(head.wrapping_add(8)));
            si_trm_data_ins(m, line, hu, hv);
            let (head, cur) = (m.u32(line.wrapping_add(L_HEAD)), m.u32(line.wrapping_add(L_CUR)));
            let fl = m.u32(head.wrapping_add(N_FLAG));
            m.set_u32(cur.wrapping_add(N_FLAG), fl);
        } else {
            let fl = m.u32(head.wrapping_add(N_FLAG));
            m.set_u32(cur.wrapping_add(N_FLAG), fl);
            let (hu, hv) = (m.f64(head), m.f64(head.wrapping_add(8)));
            m.set_f64(cur, hu);
            m.set_f64(cur.wrapping_add(8), hv);
        }
        si_trm_crv_bbox(m, line);
        i += 1;
    }
    let n = m.i32(st.wrapping_add(ST_COUNT));
    let mut i: i32 = 0;
    while i < n {
        match trm_crv_a_edg(m, o, st, i) {
            0 => trm_crv_rev_a_edg(m, o, st, i),
            2 => {
                let line = tab(m, st, i);
                let sign = m.i32(line.wrapping_add(L_SIGN));
                let rev = if m.u16(st.wrapping_add(2)) != 0 { sign > 0 } else { sign < 0 };
                if rev {
                    si_trm_cvr_rve(m, line);
                }
            }
            3 => handle_u_csd_mul_intsct(m, o, st, i),
            4 => handle_v_csd_mul_intsct(m, o, st, i),
            5 => return 3,
            6 => return 4,
            _ => {}
        }
        i += 1;
    }
    merger_intersected_t_crv(m, o, st)
}
