// SPDX-License-Identifier: MIT
//! Routines of `hidden.c` that put an edge into one of the edge lists and that find where
//! an edge meets a triangle.

use crate::hidden::lists::insert_edge;
use crate::hidden::x87stack::X;
use crate::outside::{self, Outside};
use si_mem::Mem;
use si_x87::F80;

const LIST_TRI: u32 = 0x086b_afc4;
const LIST_VCT: u32 = 0x086b_afc8;
const LIST_EDG: u32 = 0x087f_b7f8;
const LIST_EDG1: u32 = 0x087f_b7fc;
const NZ0: u32 = 0x087f_b500;
const NZ1: u32 = 0x087f_b508;
const X_MIN: u32 = 0x087f_b840;
const X_MAX: u32 = 0x087f_b844;
const Y_MIN: u32 = 0x087f_b848;
const Y_MAX: u32 = 0x087f_b84c;
const ROW: u32 = 0x087f_b858;
const COL: u32 = 0x087f_b85c;
const ERROR: u32 = 0x086b_afe0;
const FP: u32 = 0x086b_afe4;

/// Widens the bounding box of the view by a vector of the vector list (index `i`: chunk
/// `i >> 13`, entry `i & 0x1fff`); `row` and `col` keep the last index split.
fn widen(m: &mut Mem, i: i32) {
    let row = i >> 13;
    let col = i & 0x1fff;
    m.set_i32(ROW, row);
    m.set_i32(COL, col);
    let ent = |m: &Mem| {
        let chunk = m.u32(m.u32(m.u32(LIST_VCT)).wrapping_add((m.i32(ROW) as u32).wrapping_mul(4)));
        chunk.wrapping_add((m.i32(COL) as u32).wrapping_mul(12))
    };
    let x = m.i32(ent(m));
    if x < m.i32(X_MIN) {
        m.set_i32(X_MIN, x);
    } else if x > m.i32(X_MAX) {
        m.set_i32(X_MAX, x);
    }
    let y = m.i32(ent(m).wrapping_add(4));
    if y < m.i32(Y_MIN) {
        m.set_i32(Y_MIN, y);
    } else if y > m.i32(Y_MAX) {
        m.set_i32(Y_MAX, y);
    }
}

/// Puts the edge between vectors `v1` and `v2` into the list that fits the triangles on its
/// two sides (`t1`, `t2`: triangle numbers; one above `base` is looked up in `table` and
/// taken as negative, "no triangle", when the table holds a negative number there;
/// `flags`: 2 and 0x18 send an edge that has no triangle to the visible list, 4 to the
/// list of edges to draw; every other value to the draw list) [soft 0x0833f19c lookInsertEdge].
/// The triangles' z components of the normal (`nz0`, `nz1`, doubles at 0x1c of a record of
/// 0x38 bytes of `list_tri`; -1.0 when the triangle is -1) decide which side faces the eye:
/// a normal component of no more than -1e-5 counts as facing.
#[allow(clippy::too_many_arguments)]
pub fn look_insert_edge(m: &mut Mem, o: &mut dyn Outside, v1: i32, v2: i32, t1: i32, t2: i32, table: u32, base: i32, flags: i32) {
    let (mut t1, mut t2) = (t1, t2);
    if t1 > 0 {
        let k = t1.wrapping_sub(base);
        let e = m.i32(table.wrapping_add((k as u32).wrapping_mul(4)));
        t1 = t1.wrapping_sub(e);
        if e < 0 {
            t1 = -1;
        }
    }
    if t2 > 0 {
        let k = t2.wrapping_sub(base);
        let e = m.i32(table.wrapping_add((k as u32).wrapping_mul(4)));
        t2 = t2.wrapping_sub(e);
        if e < 0 {
            t2 = -1;
        }
    }
    let both_const;
    if t1 > 0 && t2 == -1 {
        t2 = 0;
        both_const = t1 == -1;
    } else if t1 == -1 {
        if t2 > 0 {
            t1 = t2;
            t2 = 0;
        }
        both_const = t1 == -1;
    } else {
        both_const = false;
    }
    let tri = m.u32(LIST_TRI);
    let rec = |t: i32| tri.wrapping_add(0x1c).wrapping_add((t as u32).wrapping_mul(0x38));
    if both_const {
        let c = -1.0f64; // the double at 0x08658b10
        m.set_f64(NZ0, c);
        m.set_f64(NZ1, c);
    } else {
        let a = m.f64(rec(t1));
        m.set_f64(NZ0, a);
        let b = m.f64(rec(t2));
        m.set_f64(NZ1, b);
    }
    let list_edg = m.u32(LIST_EDG);
    let list_edg1 = m.u32(LIST_EDG1);
    if t1 == 0 && t2 == 0 {
        widen(m, v1);
        widen(m, v2);
        if flags & 2 == 0 && flags & 4 != 0 || flags & 2 == 0 && flags & 0x18 == 0 {
            insert_edge(m, o, list_edg1, v1, v2, 0, 0, 0, -1);
        } else {
            insert_edge(m, o, list_edg, v1, v2, 0, 0, 0, 0);
        }
        return;
    }
    // "facing": -1e-5 <= nz (ordered), the compare of the original
    let lim = F80::from_f64(-1e-5);
    let c1 = lim.le(F80::from_f64(m.f64(NZ0)));
    let c2 = lim.le(F80::from_f64(m.f64(NZ1)));
    if c1 != c2 {
        // one side faces the eye: the edge belongs to the one that does
        let t = if c1 { t1 } else { t2 };
        insert_edge(m, o, list_edg, v1, v2, t, 0, 0, 0);
        return;
    }
    if !c1 {
        return;
    }
    if flags & 2 == 0 && (flags & 4 != 0 || flags & 0x18 == 0) {
        insert_edge(m, o, list_edg1, v1, v2, t1, t2, 0, -1);
    } else {
        insert_edge(m, o, list_edg, v1, v2, t1, t2, 0, 0);
    }
}

/// Where the line through a point of one edge meets the plane of a triangle (the parameters
/// of the three directions, three doubles at `o7`, `o8` and `o9`)
/// [soft 0x0833f884 edgeIntersectTriangle]. Twelve differences of coordinates are kept
/// as doubles in the routine's statics (0x087fb618 to 0x087fb670); the determinant, three
/// numerators and the reciprocal likewise. The compiler kept some products in registers and
/// spilled others to doubles, so the work is written instruction by instruction on the
/// register stack ([`X`]) with the spills as `f64` locals. When the determinant is no more
/// than 1e-5 in size three messages go to `fp` and `Error` is 3.
#[allow(clippy::too_many_arguments)]
pub fn edge_intersect_triangle(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, o7: u32, o8: u32, o9: u32) {
    let df = |m: &Mem, a: u32, b: u32| F80::from_i32(m.i32(a).wrapping_sub(m.i32(b))).to_f64();
    let a0 = df(m, p1 + 4, p1);
    let b0 = df(m, p2 + 4, p2);
    let c0 = df(m, p3 + 4, p3);
    let a1 = df(m, p4, p4 + 4);
    let b1 = df(m, p5, p5 + 4);
    let c1 = df(m, p6, p6 + 4);
    let a2 = df(m, p4, p4 + 8);
    let b2 = df(m, p5, p5 + 8);
    let c2 = df(m, p6, p6 + 8);
    let xx = df(m, p4, p1);
    let yy = df(m, p5, p2);
    let zz = df(m, p6, p3);
    for (k, v) in [a0, a1, a2, b0, b1, b2, c0, c1, c2, xx, yy, zz].into_iter().enumerate() {
        m.set_f64(0x087f_b618 + 8 * k as u32, v);
    }
    // stack slots [ebp-0x10 ..]: a0 b0 c0 c1 b2 c2 x z; a1, b1, a2, y stay in registers
    let mut l = [0f64; 16];
    l[2] = a0;
    l[3] = b0;
    l[4] = c0;
    l[5] = c1;
    l[6] = b2;
    l[7] = c2;
    l[8] = xx;
    l[9] = zz;
    let mut x = X::default();
    x.push(a1);
    x.push(b1);
    x.push(a2);
    x.push(yy);
    {
        x.push(l[2]);
        x.fmul_st(3);
        x.ld_st(4);
        x.xch(1);
        l[10] = x.st0().to_f64();
        x.fmul_m(l[7]);
        x.xch(1);
        x.fmul_m(l[6]);
        x.xch(1);
        l[14] = x.st0().to_f64();
        x.pop();
        x.ld_st(0);
        x.fmul_m(l[4]);
        x.fadd_m(l[14]);
        l[14] = x.st0().to_f64();
        x.pop();
        x.ld_st(2);
        x.fmul_m(l[3]);
        x.ld_st(3);
        x.xch(1);
        l[11] = x.st0().to_f64();
        x.fmul_m(l[5]);
        x.xch(1);
        x.fmul_st(5);
        x.xch(1);
        x.fadd_m(l[14]);
        l[14] = x.st0().to_f64();
        x.pop();
        x.ld_st(0);
        x.fmul_m(l[4]);
        x.fsubr_m(l[14]);
        l[14] = x.st0().to_f64();
        x.pop();
        x.ld_st(5);
        x.fmul_m(l[3]);
        l[12] = x.st0().to_f64();
        x.fmul_m(l[7]);
        x.fsubr_m(l[14]);
        l[14] = x.st0().to_f64();
        x.pop();
        x.push(l[2]);
        x.fmul_m(l[6]);
        l[13] = x.st0().to_f64();
        x.fmul_m(l[5]);
        x.fsubr_m(l[14]);
        l[14] = x.st0().to_f64();
        m.set_f64(0x87fb610, x.st0().to_f64());
        let _ = &l;
    }
    let lim = F80::from_f64(1e-5);
    let dn = x.st0().abs();
    x.pop();
    if dn.le(lim) || dn.unordered(lim) {
        let fp = m.u32(FP);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b040]);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b075]);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b0a0]);
        m.set_i32(ERROR, 3);
        return;
    }
    {
        x.push(l[8]);
        x.fmulp(5);
        x.xch(3);
        x.fmul_st(2);
        x.xch(1);
        x.fmul_m(l[9]);
        x.ld_st(4);
        x.xch(4);
        x.fmul_m(l[9]);
        x.ld_st(2);
        x.xch(5);
        x.fmul_m(l[7]);
        x.xch(7);
        x.fmul_st(4);
        x.xch(5);
        x.fmul_m(l[5]);
        x.xch(7);
        x.faddp(2);
        x.xch(1);
        x.faddp(6);
        x.ld_st(3);
        x.push(l[8]);
        x.fmul_m(l[6]);
        x.xch(1);
        x.fmul_m(l[7]);
        x.xch(7);
        x.fsubrp(2);
        x.xch(1);
        x.fsubrp(6);
        x.ld_st(0);
        x.fmul_m(l[5]);
        x.fsubp(6);
        x.xch(5);
        m.set_f64(0x87fb5f8, x.st0().to_f64());
        x.xch(2);
        x.fmul_m(l[2]);
        x.ld_st(0);
        x.xch(6);
        x.fmul_m(l[4]);
        x.xch(6);
        x.fmul_m(l[7]);
        x.push(l[11]);
        x.fmul_m(l[9]);
        x.xch(1);
        x.faddp(7);
        x.push(l[8]);
        x.fmul_m(l[3]);
        x.xch(7);
        x.faddp(1);
        x.ld_st(6);
        x.xch(3);
        x.fmul_m(l[4]);
        x.xch(3);
        x.fmul_m(l[7]);
        x.xch(1);
        x.fsubrp(3);
        x.fsubp(2);
        x.push(l[13]);
        x.fmul_m(l[9]);
        x.fsubp(2);
        x.xch(1);
        m.set_f64(0x87fb600, x.st0().to_f64());
        x.pop();
        x.push(l[10]);
        x.fmul_m(l[9]);
        x.xch(3);
        x.fmul_m(l[4]);
        x.xch(5);
        x.fmul_m(l[5]);
        x.xch(4);
        x.fmul_m(l[4]);
        x.xch(5);
        x.faddp(3);
        x.push(l[12]);
        x.fmul_m(l[9]);
        x.xch(3);
        l[10] = x.st0().to_f64();
        x.pop();
        x.xch(3);
        x.fadd_m(l[10]);
        l[10] = x.st0().to_f64();
        x.pop();
        x.xch(3);
        x.fsubr_m(l[10]);
        x.xch(1);
        l[12] = x.st0().to_f64();
        x.pop();
        x.xch(1);
        x.fmul_m(l[5]);
        x.xch(1);
        x.fsub_m(l[12]);
        x.fsubrp(1);
        x.push(1.0);
        x.xch(1);
        l[10] = x.st0().to_f64();
        m.set_f64(0x87fb608, x.st0().to_f64());
        x.pop();
        x.fdiv_m(l[14]);
        x.fmul_to(1);
        m.set_f64(0x87fb610, x.st0().to_f64());
        x.pop();
        m.set_f64(o7, x.st0().to_f64());
        x.pop();
        x.push(m.f64(0x87fb600));
        x.fmul_m(m.f64(0x87fb610));
        m.set_f64(o8, x.st0().to_f64());
        x.pop();
        x.push(m.f64(0x87fb608));
        x.fmul_m(m.f64(0x87fb610));
        m.set_f64(o9, x.st0().to_f64());
        x.pop();    }
}
