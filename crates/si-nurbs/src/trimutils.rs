// SPDX-License-Identifier: MIT
//! The small list routines of `trimUtils.c` (package 15, finding 0025, section 14) that the
//! trim-line code calls: the line's deletion, the insertion of a node, the reversal and the box.
//! Same record layout as [`crate::trimline`].

use si_mem::Mem;
use si_x87::F80;

const L_COUNT: u32 = 0;
const L_SIGN: u32 = 4;
const L_HEAD: u32 = 8;
const L_CUR: u32 = 0xc;
const N_FLAG: u32 = 0x10;
const N_NEXT: u32 = 0x14;
const C_1E6: f64 = 1e-6; // 0x08661b08

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// [soft 0x084f769c si_delTrimLine]: frees every node of the line (following the links at +0x14
/// from the first node) and then the line itself. Nothing for a null line. The line's fields are
/// not cleared.
pub fn si_del_trim_line(m: &mut Mem, line: u32) {
    if line == 0 {
        return;
    }
    let mut n = m.u32(line.wrapping_add(L_HEAD));
    while n != 0 {
        let next = m.u32(n.wrapping_add(N_NEXT));
        m.free(n);
        n = next;
    }
    m.free(line);
}

/// [soft 0x084f7818 si_trimDataAllocate]: a node from `SI_malloc(0x18)`; when it is not null its
/// mark (+0x10) and link (+0x14) are cleared. The point (u, v) is left as `SI_malloc` fills it.
pub fn si_trim_data_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(0x18);
    if p != 0 {
        m.set_u32(p.wrapping_add(N_FLAG), 0);
        m.set_u32(p.wrapping_add(N_NEXT), 0);
    }
    p
}

/// [soft 0x084f7910 si_trmDataIns]: `(line, u, v)` makes a node ([`si_trim_data_allocate`]) with
/// the point and puts it into the line after the cursor node (as the first node when the line is
/// empty), makes it the cursor and counts it.
pub fn si_trm_data_ins(m: &mut Mem, line: u32, u: f64, v: f64) {
    let node = si_trim_data_allocate(m);
    m.set_f64(node, u);
    m.set_f64(node.wrapping_add(8), v);
    if m.u32(line.wrapping_add(L_HEAD)) == 0 {
        m.set_u32(line.wrapping_add(L_HEAD), node);
        m.set_u32(node.wrapping_add(N_NEXT), 0);
    } else {
        let cur = m.u32(line.wrapping_add(L_CUR));
        let nx = m.u32(cur.wrapping_add(N_NEXT));
        m.set_u32(node.wrapping_add(N_NEXT), nx);
        m.set_u32(cur.wrapping_add(N_NEXT), node);
    }
    m.set_u32(line.wrapping_add(L_CUR), node);
    let c = m.i32(line.wrapping_add(L_COUNT));
    m.set_i32(line.wrapping_add(L_COUNT), c.wrapping_add(1));
}

/// [soft 0x084f79b4 revTrimFg]: swaps, in the node's mark (+0x10), bit 8 with bit 9 (when one of
/// them is set; bit 8 first) and then bit 16 with bit 17 (the same way).
pub fn rev_trim_fg(m: &mut Mem, node: u32) {
    let at = node.wrapping_add(N_FLAG);
    let f = m.u32(at);
    if f & 0x100 != 0 {
        m.set_u32(at, (f & !0x100) | 0x200);
    } else if f & 0x200 != 0 {
        m.set_u32(at, (f & !0x200) | 0x100);
    }
    let f = m.u32(at);
    if f & 0x2_0000 != 0 {
        m.set_u32(at, (f & !0x2_0000) | 0x1_0000);
    } else if f & 0x1_0000 != 0 {
        m.set_u32(at, (f & !0x1_0000) | 0x2_0000);
    }
}

/// [soft 0x084f7a0c SI_trmCvrRve]: reverses the line: the sign is negated; with two nodes or more
/// the links are turned round (the first node ends the list) and every node goes through
/// [`rev_trim_fg`]; the first node becomes the old last. The cursor is left.
pub fn si_trm_cvr_rve(m: &mut Mem, line: u32) {
    let s = m.u32(line.wrapping_add(L_HEAD));
    let sg = m.i32(line.wrapping_add(L_SIGN));
    m.set_i32(line.wrapping_add(L_SIGN), sg.wrapping_neg());
    if s == 0 {
        return;
    }
    let mut b = m.u32(s.wrapping_add(N_NEXT));
    if b == 0 {
        return;
    }
    let mut c = m.u32(b.wrapping_add(N_NEXT));
    rev_trim_fg(m, s);
    rev_trim_fg(m, b);
    m.set_u32(s.wrapping_add(N_NEXT), 0);
    m.set_u32(b.wrapping_add(N_NEXT), s);
    while c != 0 {
        rev_trim_fg(m, c);
        let a = b;
        b = c;
        c = m.u32(b.wrapping_add(N_NEXT));
        m.set_u32(b.wrapping_add(N_NEXT), a);
    }
    m.set_u32(line.wrapping_add(L_HEAD), b);
}

/// [soft 0x084f7af0 si_trmCrvBBox]: the box of the line (minU, maxV, maxU, minV at +0x10, +0x18,
/// +0x20, +0x28) from the nodes after the first (the first is the last again for a closed
/// line); and, when no node carries a mark, the sign (+4) from the turning direction at the node
/// before the leftmost one (within 1e-6 of the least u the lowest v wins): the sign of the cross
/// product of the two edges from the next node, 1 unless it is negative.
pub fn si_trm_crv_bbox(m: &mut Mem, line: u32) {
    let eps = x(C_1E6);
    let mut edx = m.u32(line.wrapping_add(L_HEAD));
    let mut edi = m.u32(edx.wrapping_add(N_NEXT));
    let mut all_free = true;
    let mut ebx = 0u32;
    let (u0, v0) = (m.f64(edi), m.f64(edi.wrapping_add(8)));
    m.set_f64(line.wrapping_add(0x10), u0);
    m.set_f64(line.wrapping_add(0x20), u0);
    m.set_f64(line.wrapping_add(0x28), v0);
    m.set_f64(line.wrapping_add(0x18), v0);
    while edi != 0 {
        if m.u32(edi.wrapping_add(N_FLAG)) != 0 {
            all_free = false;
        }
        let (u, v) = (m.f64(edi), m.f64(edi.wrapping_add(8)));
        let min_u = x(m.f64(line.wrapping_add(0x10)));
        if x(u).lt(min_u + eps) {
            let set = if !(min_u - eps).lt(x(u)) || ebx == 0 {
                true
            } else {
                x(v).lt(x(m.f64(ebx.wrapping_add(8))))
            };
            if set {
                ebx = edx;
            }
            m.set_f64(line.wrapping_add(0x10), u);
        }
        if x(u).gt(x(m.f64(line.wrapping_add(0x20)))) {
            m.set_f64(line.wrapping_add(0x20), u);
        }
        if x(v).lt(x(m.f64(line.wrapping_add(0x28)))) {
            m.set_f64(line.wrapping_add(0x28), v);
        }
        if x(v).gt(x(m.f64(line.wrapping_add(0x18)))) {
            m.set_f64(line.wrapping_add(0x18), v);
        }
        edx = edi;
        edi = m.u32(edi.wrapping_add(N_NEXT));
    }
    if all_free {
        let edi = m.u32(ebx.wrapping_add(N_NEXT));
        let (ub, vb) = (x(m.f64(ebx)), x(m.f64(ebx.wrapping_add(8))));
        let (ue, ve) = (x(m.f64(edi)), x(m.f64(edi.wrapping_add(8))));
        let mut a = m.u32(edi.wrapping_add(N_NEXT));
        if a == 0 {
            a = m.u32(m.u32(line.wrapping_add(L_HEAD)).wrapping_add(N_NEXT));
        }
        let (au, av) = (x(m.f64(a)), x(m.f64(a.wrapping_add(8))));
        let x1 = (ub - ue) * (av - ve);
        let x2 = (au - ue) * (vb - ve);
        let d = x1 - x2;
        m.set_i32(line.wrapping_add(L_SIGN), if x(0.0).gt(d) { -1 } else { 1 });
    }
}

/// [soft 0x084f8594 si_getEdgeSteps]: `(t, n, steps)`: the answer in st0 is the greatest gap
/// `t[i + 1] - t[i]` (from 0; compared on the register) for `i < n`, divided by the integer
/// `steps`.
pub fn si_get_edge_steps(m: &Mem, t: u32, n: i32, steps: i32) -> F80 {
    let mut best = x(0.0);
    let mut i: i32 = 0;
    while i < n {
        let at = t.wrapping_add((i as u32).wrapping_mul(8));
        let d = x(m.f64(at.wrapping_add(8))) - x(m.f64(at));
        // `fcom st(1)`: the gap replaces the best only when it is greater (ordered)
        if d.gt(best) {
            best = d;
        }
        i += 1;
    }
    best / F80::from_i32(steps)
}

/// [soft 0x084f8444 si_uniqueD]: `(srf, u1, v1, u2, v2)`, a short: whether the points (u1, v1) and
/// (u2, v2) are different places of the surface's parameter plane (1), the same place (0), or
/// the same place across a seam (a closed direction: 3 when they lie on the two v ends, 2 when
/// on the two u ends). With `|u1 - u2| < 1e-5` (`d` = the difference, compared on the register):
/// 0 when `|v1 - v2| < 1e-5` too; else, for a surface closed in v (short at +0x44), 3 when the
/// two points are within 1e-5 of the ends `vS`/`vE` (+0x34, +0x3c) in either order; else 1.
/// With `|u1 - u2|` not below it: when `|v1 - v2| < 1e-5` and the surface is closed in u (short
/// at +0x20), 2 when the points are within 1e-5 of the ends `uS`/`uE` (+0x10, +0x18) in either
/// order; else 1.
pub fn si_unique_d(m: &Mem, srf: u32, u1: f64, v1: f64, u2: f64, v2: f64) -> i16 {
    let eps = x(1e-5);
    let near = |a: f64, at: u32| (x(a) - x(m.f64(srf.wrapping_add(at)))).abs().lt(eps);
    let (u1x, v1x, u2x, v2x) = (x(u1), x(v1), x(u2), x(v2));
    let mut r: i16 = 1;
    // `fcom`: eps against |du|; the first branch runs when eps > |du| (ordered)
    if eps.gt((u1x - u2x).abs()) {
        if (v1x - v2x).abs().lt(eps) {
            return 0;
        }
        if m.u16(srf.wrapping_add(0x44)) != 0 {
            if near(v2, 0x34) && near(v1, 0x3c) {
                r = 3;
            }
            if near(v1, 0x34) && near(v2, 0x3c) {
                r = 3;
            }
        }
        r
    } else {
        if (v1x - v2x).abs().lt(eps) && m.u16(srf.wrapping_add(0x20)) != 0 {
            if near(u2, 0x10) && near(u1, 0x18) {
                r = 2;
            }
            if near(u1, 0x10) && near(u2, 0x18) {
                r = 2;
            }
        }
        r
    }
}

/// The product's wiring of the four small trim routines the trim-line code and `own.rs` call through
/// [`si_mem::Outside`]: `si_delTrimLine (0x084f769c)`, `si_trmDataIns (0x084f7910)`,
/// `SI_trmCvrRve (0x084f7a0c)` and `si_trmCrvBBox (0x084f7af0)` run as the ports above (arguments as the stack
/// words of the original: the two doubles of `si_trmDataIns` are two words each); every other call goes on
/// to the host behind it. The four return nothing (the original leaves the registers as they were).
pub struct TrimUtilsOutside<'a> {
    pub inner: &'a mut dyn si_mem::Outside,
}

/// Runs one of the four routines when `addr` is its address; `None` for any other.
pub fn call_trim_util(m: &mut Mem, addr: u32, args: &[u32]) -> Option<si_mem::Ret> {
    let w = |i: usize| args.get(i).copied().unwrap_or(0);
    let d = |i: usize| f64::from_bits(w(i) as u64 | (w(i + 1) as u64) << 32);
    match addr {
        0x084f_769c => si_del_trim_line(m, w(0)),
        0x084f_7910 => si_trm_data_ins(m, w(0), d(1), d(3)),
        0x084f_7a0c => si_trm_cvr_rve(m, w(0)),
        0x084f_7af0 => si_trm_crv_bbox(m, w(0)),
        _ => return None,
    }
    Some(si_mem::Ret::NONE)
}

impl si_mem::Outside for TrimUtilsOutside<'_> {
    fn call(&mut self, m: &mut Mem, routine: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        match call_trim_util(m, routine.addr, args) {
            Some(r) => r,
            None => self.inner.call(m, routine, args),
        }
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.inner.print(stream, text)
    }
}
