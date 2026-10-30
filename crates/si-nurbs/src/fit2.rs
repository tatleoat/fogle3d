// SPDX-License-Identifier: MIT
//! Curve fitting, part two (`curveFit.c`, package 14, nurbs7): the basis-function matrix of a curve's
//! parameter and the routines that were still missing from `fit.rs`. Everything works on the memory image
//! and makes the original's allocations in the original's order.
//!
//! The matrices of doubles are the program's `SI_generalMatrixDbl` records (rows at +0, columns at +4, the
//! row table at +8); the four helpers below are the same arithmetic as `si-math`'s
//! `matrix_gen_dbl` (identity, product, element copy, disposal), written on the image because the
//! heap blocks matter here. They are not cited again: they belong to `si-math` and `si-hrc`.

use crate::fit::{cleanup_partition, x, SI_SHOW_ACTIVITY};
use crate::knots;
use crate::{Dir, NurbsDir, Statics};
use si_mem::{Mem, Ptr32};
use si_hrc::image::nurbs::nurbs_crv_allocate;
use si_x87::F80;

// ---- the matrices of doubles on the image ----

pub(crate) fn mat_alloc(m: &mut Mem, rows: i32, cols: i32) -> u32 {
    let p = m.si_malloc(0xc);
    if p == 0 {
        return 0;
    }
    m.set_i32(p, rows);
    m.set_i32(p + 4, cols);
    let tab = m.si_malloc((rows as u32) << 2);
    m.set_u32(p + 8, tab);
    if tab == 0 {
        return 0;
    }
    let mut i = 0;
    while i < rows {
        let row = m.si_malloc((cols as u32) << 3);
        m.set_u32(tab.wrapping_add(4 * i as u32), row);
        if row == 0 {
            return 0;
        }
        i += 1;
    }
    p
}

pub(crate) fn mat_identity(m: &mut Mem, n: i32) -> u32 {
    let p = mat_alloc(m, n, n);
    if p == 0 {
        return 0;
    }
    for i in 0..n {
        for j in 0..n {
            let at = mat_at(m, p, i, j);
            m.set_f64(at, if i == j { 1.0 } else { 0.0 });
        }
    }
    p
}

pub(crate) fn mat_at(m: &Mem, mat: u32, i: i32, j: i32) -> u32 {
    let row = m.u32(m.u32(mat.wrapping_add(8)).wrapping_add(4 * i as u32));
    row.wrapping_add(8 * j as u32)
}

/// `out = a * b` (the sum rounded to a double after every term, the product on the register).
pub(crate) fn mat_multiply(m: &mut Mem, a: u32, b: u32, out: u32) {
    let (ar, ac, bc) = (m.i32(a), m.i32(a.wrapping_add(4)), m.i32(b.wrapping_add(4)));
    for i in 0..ar {
        for j in 0..bc {
            let o = mat_at(m, out, i, j);
            m.set_f64(o, 0.0);
            for k in 0..ac {
                let v = (x(m.f64(mat_at(m, a, i, k))) * x(m.f64(mat_at(m, b, k, j))) + x(m.f64(o))).to_f64();
                m.set_f64(o, v);
            }
        }
    }
}

pub(crate) fn mat_copy_element(m: &mut Mem, a: u32, out: u32) {
    let (ar, ac) = (m.i32(a), m.i32(a.wrapping_add(4)));
    for i in 0..ar {
        for j in 0..ac {
            let v = x(m.f64(mat_at(m, a, i, j))).to_f64();
            let o = mat_at(m, out, i, j);
            m.set_f64(o, v);
        }
    }
}

/// Disposal of the matrix the pointer at `pp` names, then the pointer cleared.
pub(crate) fn mat_dispose(m: &mut Mem, pp: u32) {
    let g = m.u32(pp);
    if g == 0 {
        return;
    }
    let rows = m.i32(g);
    let mut i = 0;
    while i < m.i32(g) {
        let slot = m.u32(g.wrapping_add(8)).wrapping_add(4 * i as u32);
        let row = m.u32(slot);
        if row != 0 {
            m.free(row);
            m.set_u32(slot, 0);
        }
        i += 1;
    }
    let _ = rows;
    let tab = m.u32(g.wrapping_add(8));
    if tab != 0 {
        m.free(tab);
        m.set_u32(g.wrapping_add(8), 0);
    }
    m.free(g);
    m.set_u32(pp, 0);
}

pub(crate) fn knot_vec(m: &Mem, rec: &NurbsDir) -> Vec<f64> {
    let (a, n) = (rec.knots.addr(), rec.nb_knots);
    if a == 0 || n <= 0 || n > 1 << 24 || !m.is_mapped(a, (n as u32) * 8) {
        return Vec::new();
    }
    (0..n as u32).map(|i| m.f64(a + 8 * i)).collect()
}

/// [soft 0x084d93c4 SI_nurbsMatrixBasisFunction]: `(dir, t, idx, flag)`: the matrix, `order x order`, whose
/// column (the last for `flag` 0, the first otherwise) holds the values of the `order` basis functions of
/// the span of `t` (Cox-de Boor as a product of bidiagonal matrices). `*idx` receives the span's index
/// less `order - 2` (plus the point count when that is negative). Returns the matrix (the caller owns it),
/// 0 for an order below 2 (also when the span search would not end, which the port reports the same way).
/// The original allocates the knot distances `a` and `b` (`SI_calloc(order - 1, 8)` each) and four identity
/// matrices, frees three of them and both arrays before it returns.
pub fn nurbs_matrix_basis_function(m: &mut Mem, dir: u32, t: f64, idx: u32, flag: i16) -> u32 {
    let order = m.i32(dir);
    let count = m.i32(dir.wrapping_add(8));
    let kn = m.u32(dir.wrapping_add(0xc));
    let d = order.wrapping_sub(1);
    if order <= 1 {
        return 0;
    }
    let closed16 = m.u16(dir.wrapping_add(0x20));
    let esi;
    {
        let rec: NurbsDir = m.read(Ptr32::new(dir));
        let kv = knot_vec(m, &rec);
        let mut st = Statics { left: m.i32(crate::image::LEFT) };
        let r = knots::interv(&mut st, Dir { rec: &rec, knots: &kv }, t, (closed16 == 0) as i16);
        m.set_i32(crate::image::LEFT, st.left);
        match r {
            Some(v) => esi = v,
            None => return 0,
        }
    }
    let mut v = esi.wrapping_sub(order).wrapping_add(2);
    m.set_i32(idx, v);
    if v < 0 {
        v = v.wrapping_add(m.i32(dir.wrapping_add(4)));
        m.set_i32(idx, v);
    }
    let a = m.si_calloc(d as u32, 8);
    let b = m.si_calloc(d as u32, 8);
    macro_rules! k { ($i:expr) => { m.f64(kn.wrapping_add(($i as u32).wrapping_mul(8))) }; }
    let tt = x(t);
    let mut i = 0;
    while i < d {
        let val = if closed16 != 0 && i > esi {
            tt - k!(0) + k!(count.wrapping_sub(1)) - k!(esi.wrapping_add(count).wrapping_sub(1).wrapping_sub(i))
        } else {
            tt - k!(esi.wrapping_sub(i))
        };
        let at = a.wrapping_add(8 * i as u32);
        let r = val.to_f64();
        m.set_f64(at, r);
        i += 1;
    }
    let mut i = 0;
    while i < d {
        let lo = esi.wrapping_sub(count).wrapping_add(2).wrapping_add(i);
        let hi = esi.wrapping_add(1).wrapping_add(i);
        let val = if closed16 != 0 && hi >= count {
            (x(k!(count.wrapping_sub(1))) - tt) + k!(lo) - k!(0)
        } else {
            x(k!(hi)) - tt
        };
        let at = b.wrapping_add(8 * i as u32);
        m.set_f64(at, val.to_f64());
        i += 1;
    }
    let mut m1 = mat_identity(m, order);
    let mut m2 = mat_identity(m, order);
    let mut m3 = mat_identity(m, order);
    let mut m4 = mat_identity(m, order);
    let (p1, p2, p3, p4) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let _ = (&mut m1, &mut m2, &mut m3, &mut m4);
    m.set_u32(p1, m1);
    m.set_u32(p2, m2);
    m.set_u32(p3, m3);
    m.set_u32(p4, m4);
    let mut kk = 1i32;
    let mut i = 0;
    while i < d {
        let mut cnt = d - i;
        let (mut row, mut col) = if flag != 0 { (0, 0) } else { (kk - 1, kk) };
        let mut j = 0;
        let ntimes = d - i;
        while j < ntimes {
            let av = x(m.f64(a.wrapping_add(8 * (cnt - 1) as u32)));
            let bv = m.f64(b.wrapping_add(8 * j as u32));
            let q = x(bv) / (av + bv);
            let at = mat_at(m, m3, row, col);
            m.set_f64(at, q.to_f64());
            let r = x(1.0) - q;
            let at = mat_at(m, m3, row + 1, col);
            m.set_f64(at, r.to_f64());
            cnt -= 1;
            row += 1;
            col += 1;
            j += 1;
        }
        mat_multiply(m, m1, m3, m2);
        mat_copy_element(m, m2, m1);
        mat_copy_element(m, m4, m3);
        kk += 1;
        i += 1;
    }
    mat_dispose(m, p3);
    mat_dispose(m, p2);
    mat_dispose(m, p4);
    if a != 0 {
        m.free(a);
    }
    if b != 0 {
        m.free(b);
    }
    let res = m.u32(p1);
    m.release(16);
    res
}

/// [soft 0x084d9668 SI_nurbsCrvBasisFunctions]: `(mx, flag)`: a new array (`SI_calloc(rows, 8)`) of the
/// last column of the matrix `mx` (the first when `flag` is non-zero), one double per row.
pub fn nurbs_crv_basis_functions(m: &mut Mem, mx: u32, flag: i16) -> u32 {
    let n = m.i32(mx);
    let arr = m.si_calloc(n as u32, 8);
    let col = if flag != 0 { 0 } else { n.wrapping_sub(1) };
    let mut i = 0;
    while i < n {
        let v = x(m.f64(mat_at(m, mx, i, col))).to_f64();
        m.set_f64(arr.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    arr
}

// ---- the allocation of the fit's working data and the knot update ----

/// Frees the block the word at `slot` names (when not null) and clears the word.
fn free_clear(m: &mut Mem, slot: u32) {
    let v = m.u32(slot);
    if v != 0 {
        m.free(v);
        m.set_u32(slot, 0);
    }
}

/// [soft 0x084cf6ec allocateFixedData]: `(n, k, pts, mat_a, mat_b, pts2, p5, p6, p7)`: the working storage
/// of a fit with `n` points and order `k`. Through the pointers it receives: `*pts = SI_calloc(n, 0x24)`,
/// `*mat_a` an `n x k` and `*mat_b` an `n x (2k - 1)` matrix of doubles, then four more point arrays of `n`
/// points (`*pts2`, `*p5`, `*p6`, `*p7`). Returns 1; if any allocation fails what was made is given back (the
/// pointers are cleared, matrices disposed) in the reverse order and 0 is returned.
pub fn allocate_fixed_data(m: &mut Mem, n: i32, k: i32, pts: u32, mat_a: u32, mat_b: u32, pts2: u32, p5: u32, p6: u32, p7: u32) -> i32 {
    let a = m.si_calloc(n as u32, 0x24);
    m.set_u32(pts, a);
    if a == 0 {
        return 0;
    }
    let v = mat_alloc(m, n, k);
    m.set_u32(mat_a, v);
    if v == 0 {
        free_clear(m, pts);
        return 0;
    }
    let v = mat_alloc(m, n, k.wrapping_mul(2).wrapping_sub(1));
    m.set_u32(mat_b, v);
    if v == 0 {
        mat_dispose(m, mat_a);
        free_clear(m, pts);
        return 0;
    }
    let v = m.si_calloc(n as u32, 0x24);
    m.set_u32(pts2, v);
    if v == 0 {
        mat_dispose(m, mat_b);
        mat_dispose(m, mat_a);
        free_clear(m, pts);
        return 0;
    }
    let v = m.si_calloc(n as u32, 0x24);
    m.set_u32(p5, v);
    if v == 0 {
        free_clear(m, pts2);
        mat_dispose(m, mat_b);
        mat_dispose(m, mat_a);
        free_clear(m, pts);
        return 0;
    }
    let v = m.si_calloc(n as u32, 0x24);
    m.set_u32(p6, v);
    if v == 0 {
        free_clear(m, p5);
        free_clear(m, pts2);
        mat_dispose(m, mat_b);
        mat_dispose(m, mat_a);
        free_clear(m, pts);
        return 0;
    }
    let v = m.si_calloc(n as u32, 0x24);
    m.set_u32(p7, v);
    if v == 0 {
        free_clear(m, p6);
        free_clear(m, p5);
        free_clear(m, pts2);
        mat_dispose(m, mat_b);
        mat_dispose(m, mat_a);
        free_clear(m, pts);
        return 0;
    }
    1
}

use si_mem::{Outside, Routine};
use std::cell::Cell;

pub const SI_NURBS_CRV_KNOT_INSERT: Routine = Routine::new("SI_nurbsCrvKnotInsert", 0x084d_4130);

thread_local! {
    static UNDEFINED_READS: Cell<u32> = const { Cell::new(0) };
}

pub(crate) fn undefined_read() {
    UNDEFINED_READS.with(|c| c.set(c.get() + 1));
}

/// A checker aid, no part of the original: how many reads of memory the original leaves undefined (stack words it
/// never wrote, knots past the end of an array) the routines of this file made since the last call.
pub fn take_undefined_reads() -> u32 {
    UNDEFINED_READS.with(|c| c.replace(0))
}

/// [soft 0x084d0444 updateKnotVector]: `(dir, ptsp, k, done, flags, m, params, template)`: raises the point count
/// of the direction `dir` (a knot vector and its points) until it holds `m` more points than the interior of
/// the fit needs, by inserting knots, one per span at most. The working copy is a curve record on the stack
/// of which the direction (the first 0x24 bytes, with a fresh `SI_calloc` copy of the knots) and the points
/// (+0x2c, `SI_calloc(count, 0x24)`: the end points `template[0]` and `template[m + 1]` and the old points
/// `*ptsp` in between for an open curve, the old points for a closed one) are made; the rest of the record is
/// whatever the stack held. If the point count (less 2 when open) is already at least `m`, `*done = 0` and
/// nothing is done. Else for each of the `k + 1` knot spans (spans shorter than 1e-6 skipped) whose flag
/// (`flags[c]`, for `c < k`) is set, the parameter of `params[0..m]` nearest the span's middle among those
/// below the span's end plus 1e-6 and not below its start less 1e-6 (the search position only moves forward)
/// is inserted once with `SI_nurbsCrvKnotInsert(&work, t, 1, 1)`, through `o`; a zero answer ends the routine
/// with 0 (what was made stays). When the count reaches the target `*done` is cleared and the loop ends. The
/// points are copied back to `*ptsp`, the working points and the old knots are freed, and the direction is
/// replaced by the working copy. Returns 1.
/// Where the original reads the stack word of the chosen index before it set one, or the knots past the end,
/// the port reads zero / the image, and counts it (`take_undefined_reads`).
pub fn update_knot_vector(m: &mut Mem, o: &mut dyn Outside, dir: u32, ptsp: u32, k: i32, done: u32, flags: u32, nm: i32, params: u32, template: u32) -> i32 {
    let bound = m.i32(dir.wrapping_add(4)).wrapping_sub(if m.u16(dir.wrapping_add(0x20)) != 0 { 0 } else { 2 });
    if bound >= nm {
        m.set_i32(done, 0);
        return 1;
    }
    let sp = m.sp();
    let w = m.local(0x64);
    let r = update_knot_vector_body(m, o, w, dir, ptsp, k, done, flags, nm, params, template);
    m.set_sp(sp);
    r
}

fn update_knot_vector_body(m: &mut Mem, o: &mut dyn Outside, w: u32, dir: u32, ptsp: u32, k: i32, done: u32, flags: u32, nm: i32, params: u32, template: u32) -> i32 {
    let nk = m.i32(dir.wrapping_add(8));
    let knots = m.u32(dir.wrapping_add(0xc));
    let closed = m.u16(dir.wrapping_add(0x20)) != 0;
    m.copy(w, dir, 0x24);
    let wk = m.si_calloc(m.i32(dir.wrapping_add(8)) as u32, 8);
    m.set_u32(w.wrapping_add(0xc), wk);
    if wk == 0 {
        return 0;
    }
    let mut i = 0;
    while i < nk {
        let v = m.f64(knots.wrapping_add(8 * i as u32));
        m.set_f64(wk.wrapping_add(8 * i as u32), x(v).to_f64());
        i += 1;
    }
    let n0 = m.i32(dir.wrapping_add(4));
    let wp = m.si_calloc(n0 as u32, 0x24);
    m.set_u32(w.wrapping_add(0x2c), wp);
    if wp == 0 {
        free_clear(m, w.wrapping_add(0xc));
        return 0;
    }
    let src = m.u32(ptsp);
    if closed {
        let mut i = 0;
        while i < n0 {
            m.copy(wp.wrapping_add(0x24 * i as u32), src.wrapping_add(0x24 * i as u32), 0x24);
            i += 1;
        }
    } else {
        m.copy(wp, template, 0x24);
        let mut i = 1;
        while i < n0.wrapping_sub(1) {
            m.copy(wp.wrapping_add(0x24 * i as u32), src.wrapping_add(0x24 * (i - 1) as u32), 0x24);
            i += 1;
        }
        let last = wp.wrapping_add((n0 as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
        m.copy(last, template.wrapping_add(((nm as u32).wrapping_mul(8).wrapping_add(nm as u32)).wrapping_mul(4)).wrapping_add(0x24), 0x24);
    }
    let kn = |m: &Mem, i: i32| {
        if i >= nk || i < 0 {
            undefined_read();
        }
        m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)))
    };
    let (mut j, mut e, mut c) = (0i32, 0i32, 0i32);
    let mut sel: Option<i32> = None;
    let c1 = x(1e-6);
    if k >= 0 {
        loop {
            while e < nk.wrapping_sub(1) && (x(kn(m, e + 1)) - kn(m, e)).lt(c1) {
                e += 1;
            }
            if c < k && m.u32(flags.wrapping_add(4 * c as u32)) != 0 {
                let k0 = x(kn(m, e));
                let k1 = x(kn(m, e + 1));
                let low: F80 = k0 - 1e-6f64;
                while j < nm && low.gt(x(m.f64(params.wrapping_add(8 * j as u32)))) {
                    j += 1;
                }
                let mid = x(0.5) * (k0 + k1);
                let hi = c1 + k1;
                let mut best = x(3.4028234663852886e+38);
                while j < nm {
                    let p = x(m.f64(params.wrapping_add(8 * j as u32)));
                    if !p.lt(hi) {
                        break;
                    }
                    let d = (p - mid).abs();
                    if d.lt(best) {
                        best = d;
                        sel = Some(j);
                    }
                    j += 1;
                }
                let s = match sel {
                    Some(s) => s,
                    None => {
                        undefined_read();
                        0
                    }
                };
                let t = m.f64(params.wrapping_add(8 * s as u32));
                let [lo, hi2] = [t.to_bits() as u32, (t.to_bits() >> 32) as u32];
                let r = o.call(m, SI_NURBS_CRV_KNOT_INSERT, &[w, lo, hi2, 1, 1]).eax;
                if r as u16 == 0 {
                    return 0;
                }
                let mut cnt = m.i32(w.wrapping_add(4));
                let wclosed = m.u16(w.wrapping_add(0x20)) != 0;
                if !wclosed {
                    cnt = cnt.wrapping_sub(2);
                }
                if cnt >= nm {
                    m.set_i32(done, 0);
                    break;
                }
            }
            e += 1;
            c += 1;
            if !(c <= k) {
                break;
            }
        }
    }
    let wn = m.i32(w.wrapping_add(4));
    let wp2 = m.u32(w.wrapping_add(0x2c));
    let dst = m.u32(ptsp);
    if closed {
        let mut i = 0;
        while i < wn {
            m.copy(dst.wrapping_add(0x24 * i as u32), wp2.wrapping_add(0x24 * i as u32), 0x24);
            i += 1;
        }
    } else {
        let mut i = 1;
        while i < wn.wrapping_sub(1) {
            m.copy(dst.wrapping_add(0x24 * (i - 1) as u32), wp2.wrapping_add(0x24 * i as u32), 0x24);
            i += 1;
        }
    }
    free_clear(m, w.wrapping_add(0x2c));
    free_clear(m, dir.wrapping_add(0xc));
    m.copy(dir, w, 0x24);
    1
}

const CE_EPS: f64 = 1e-6; // 0x08661610

/// [soft 0x084d019c calculateError]: `(dir, a, b, n, count, params, nflag, out, tol)`: which knot spans of the
/// direction hold a sample at which the fit is off. `a` and `b` are two arrays of points (0x24 bytes each, x, y,
/// z used), `params` the `n` sample parameters in ascending order, `tol` the tolerance. `*count` receives the
/// number of spans longer than 1e-6 (an open curve: from knot `order - 2` to `count - order`, closed: all),
/// `*out` a new array (`SI_calloc(count, 4)`) of one flag per span and `*nflag` the number of flags set; returns
/// 1, or 0 when the array cannot be had (`*count` is then written, `*nflag` not). Walking the spans and the samples
/// together (the sample index never goes back; the byte offset into `a` and `b` is set to 0 once and keeps pace with it), a sample belongs to a span while it lies below the span's end
/// plus 1e-6; with the squared distance `|a - b|^2 > tol^2` (the squares and the sum on the register, `tol^2`
/// rounded to a double) the span is flagged when `lo < p < hi` (`lo`, `hi` the span's ends moved in by 1e-6), or
/// `p > lo` fails and the next sample is below `hi`, or `p > hi` and the previous sample is below `lo`;
/// a flagged span is not measured again. 
pub fn calculate_error(m: &mut Mem, dir: u32, a: u32, b: u32, n: i32, count: u32, params: u32, nflag: u32, out: u32, tol: f64) -> i32 {
    let tol2 = (x(tol) * x(tol)).to_f64();
    let order = m.i32(dir);
    let nkn = m.i32(dir.wrapping_add(8));
    let closed = m.i16(dir.wrapping_add(0x20)) as i32;
    let knots = m.u32(dir.wrapping_add(0xc));
    let kn = |m: &Mem, i: i32| m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)));
    m.set_i32(count, 0);
    let start = if closed == 0 { order.wrapping_sub(2) } else { 0 };
    let end = if closed == 0 { nkn.wrapping_add(1).wrapping_sub(order) } else { nkn.wrapping_sub(1) };
    let c = x(CE_EPS);
    let mut j = start;
    if j < end {
        loop {
            if (x(kn(m, j + 1)) - kn(m, j)).gt(c) {
                let v = m.i32(count).wrapping_add(1);
                m.set_i32(count, v);
            }
            j += 1;
            if j >= end {
                break;
            }
        }
    }
    let flags = m.si_calloc(m.i32(count) as u32, 4);
    if flags == 0 {
        return 0;
    }
    m.set_i32(nflag, 0);
    let nk1 = nkn.wrapping_sub(1);
    let mut j = start;
    let mut s = 0i32;
    let mut q = 0i32;
    let mut off = 0u32; // set once before the first span; it keeps counting across the spans
    while q < m.i32(count) {
        m.set_i32(flags.wrapping_add(4 * q as u32), 0);
        let k1 = x(kn(m, j + 1));
        let next_j = j + 1;
        let lo = c + kn(m, j); // 1e-6 + K0
        let hi = k1 - CE_EPS;
        if s < n {
            let p = x(m.f64(params.wrapping_add(8 * s as u32)));
            if p.lt(k1 + CE_EPS) {
                let mut p = p;
                loop {
                    if m.i32(flags.wrapping_add(4 * q as u32)) == 0 {
                        let dx = x(m.f64(a.wrapping_add(off))) - m.f64(b.wrapping_add(off));
                        let dy = x(m.f64(a.wrapping_add(off + 8))) - m.f64(b.wrapping_add(off + 8));
                        let dz = x(m.f64(a.wrapping_add(off + 0x10))) - m.f64(b.wrapping_add(off + 0x10));
                        let d2 = dz * dz + (dx * dx + dy * dy);
                        if d2.gt(x(tol2)) {
                            let mut hit = false;
                            if lo.lt(p) && p.lt(hi) {
                                hit = true;
                            } else {
                                if s + 1 < n && lo.gt(p) && hi.gt(x(m.f64(params.wrapping_add(8 * (s + 1) as u32)))) {
                                    hit = true;
                                }
                                if !hit && s > 0 && hi.lt(p) && lo.lt(x(m.f64(params.wrapping_add(8 * (s - 1) as u32)))) {
                                    hit = true;
                                }
                            }
                            if hit {
                                m.set_i32(flags.wrapping_add(4 * q as u32), 1);
                                let v = m.i32(nflag).wrapping_add(1);
                                m.set_i32(nflag, v);
                            }
                        }
                    }
                    if !p.lt(hi) {
                        break;
                    }
                    off = off.wrapping_add(0x24);
                    s += 1;
                    if s >= n {
                        break;
                    }
                    p = x(m.f64(params.wrapping_add(8 * s as u32)));
                    if !p.lt(k1 + CE_EPS) {
                        break;
                    }
                }
            }
        }
        // the next span of more than 1e-6
        j = next_j;
        while j < nk1 && (x(kn(m, j + 1)) - kn(m, j)).lt(c) {
            j += 1;
        }
        q += 1;
    }
    m.set_u32(out, flags);
    1
}

/// [soft 0x084d1800 nurbsCrvFromComponents]: `(dirs, n, pts, closed)`: one curve built from `n` consecutive pieces,
/// `dirs` the `n` direction records (0x24 each) and `pts[i]` the points of piece `i` (0x24 each). The curve
/// (`SI_nurbsCrvAllocate`) takes the first record's order; its knot count is the sum of the pieces' counts less
/// `(n - 1) * (order - 1)` (an open piece counts one more knot than its record: `nk + 1 - order` knots are taken), plus
/// `2 - order` when `closed` is set and the first piece is open; the knots are the pieces' knots one after the
/// other, followed (first piece open) by `order - 1` knots read on past the last piece's (one when `closed`);
/// the range is `knots[0]`, `knots[total - 1]` (closed) or `knots[order - 2]`, `knots[total - order + 1]`; the points are all
/// but the last of every piece, then (when `closed` is 0 or the first piece's closing flag is set) the last
/// piece's last point. Returns the curve, 0 when
/// an allocation fails (the curve, and the knots when they were had, are freed then).
pub fn nurbs_crv_from_components(m: &mut Mem, dirs: u32, n: i32, pts: u32, closed: i32) -> u32 {
    let order = m.i32(dirs);
    let crv = nurbs_crv_allocate(m).addr();
    if crv == 0 {
        return 0;
    }
    m.set_i16(crv.wrapping_add(0x20), closed as i16);
    m.set_i32(crv, order);
    let order1 = order.wrapping_sub(1);
    let mut total = 0i32;
    if n > 0 {
        let mut run = dirs.wrapping_add(8);
        for _ in 0..n {
            total = total.wrapping_add(m.i32(run));
            run = run.wrapping_add(0x24);
        }
    }
    total = total.wrapping_sub((n.wrapping_sub(1)).wrapping_mul(order1));
    if closed != 0 && m.i16(dirs.wrapping_add(0x20)) == 0 {
        total = total.wrapping_add(2).wrapping_sub(order);
    }
    m.set_i32(crv.wrapping_add(8), total);
    let knots = m.si_calloc(total as u32, 8);
    m.set_u32(crv.wrapping_add(0xc), knots);
    if knots == 0 {
        m.free(crv);
        return 0;
    }
    let npts = if closed == 0 { total.wrapping_add(2).wrapping_sub(order) } else { total.wrapping_sub(1) };
    m.set_i32(crv.wrapping_add(4), npts);
    let points = m.si_calloc(npts as u32, 0x24);
    m.set_u32(crv.wrapping_add(0x2c), points);
    if points == 0 {
        if m.u32(crv.wrapping_add(0xc)) != 0 {
            m.free(m.u32(crv.wrapping_add(0xc)));
            m.set_u32(crv.wrapping_add(0xc), 0);
        }
        m.free(crv);
        return 0;
    }
    // the knots
    let mut k = 0i32;
    let mut kp = 0u32; // [ebp-0x18]: the last piece's knots
    let mut j = 0i32; // [ebp-8]
    if n <= 0 {
        undefined_read();
    }
    let mut i = 0i32;
    if 0 < n {
        loop {
            let d = dirs.wrapping_add(36u32.wrapping_mul(i as u32));
            let mut cnt = m.i32(d.wrapping_add(8));
            if m.i16(d.wrapping_add(0x20)) == 0 {
                cnt = cnt.wrapping_add(1).wrapping_sub(order);
            }
            kp = m.u32(d.wrapping_add(0xc));
            j = 0;
            while j < cnt {
                let v = m.f64(kp.wrapping_add(8u32.wrapping_mul(j as u32)));
                m.set_f64(knots.wrapping_add(8u32.wrapping_mul(k as u32)), v);
                k = k.wrapping_add(1);
                j += 1;
            }
            i += 1;
            if i >= n {
                break;
            }
        }
    }
    if m.i16(dirs.wrapping_add(0x20)) == 0 {
        let mut c = 0i32;
        loop {
            let go = if closed == 0 { c < order1 } else { c <= 0 };
            if !go {
                break;
            }
            c += 1;
            let v = m.f64(kp.wrapping_add(8u32.wrapping_mul(j as u32)));
            m.set_f64(knots.wrapping_add(8u32.wrapping_mul(k as u32)), v);
            j += 1;
            k = k.wrapping_add(1);
        }
    }
    if closed != 0 {
        let v = m.f64(knots);
        m.set_f64(crv.wrapping_add(0x10), v);
        let v = m.f64(knots.wrapping_add(8u32.wrapping_mul(total as u32)).wrapping_sub(8));
        m.set_f64(crv.wrapping_add(0x18), v);
    } else {
        let v = m.f64(knots.wrapping_add(8u32.wrapping_mul(order as u32)).wrapping_sub(0x10));
        m.set_f64(crv.wrapping_add(0x10), v);
        total = total.wrapping_sub(order);
        let v = m.f64(knots.wrapping_add(8u32.wrapping_mul(total as u32)).wrapping_add(8));
        m.set_f64(crv.wrapping_add(0x18), v);
    }
    // the points
    let mut q = 0u32; // [ebp-0xc], the points written
    let mut off = 0u32; // [ebp-0x2c], the byte offset of the next one
    let mut src = 0u32; // [ebp-0x20]
    let mut jj = 0i32; // [ebp-8]
    if n <= 0 {
        undefined_read();
    }
    if 0 < n {
        let mut i = 0i32;
        loop {
            src = m.u32(pts.wrapping_add(4u32.wrapping_mul(i as u32)));
            let d = dirs.wrapping_add(36u32.wrapping_mul(i as u32));
            jj = 0;
            let mut so = 0u32;
            while jj < m.i32(d.wrapping_add(4)).wrapping_sub(1) {
                m.copy(points.wrapping_add(off), src.wrapping_add(so), 0x24);
                so = so.wrapping_add(0x24);
                off = off.wrapping_add(0x24);
                q = q.wrapping_add(1);
                jj += 1;
            }
            i += 1;
            if i >= n {
                break;
            }
        }
    }
    if closed == 0 || m.i16(dirs.wrapping_add(0x20)) != 0 {
        m.copy(
            points.wrapping_add(36u32.wrapping_mul(q)),
            src.wrapping_add(36u32.wrapping_mul(jj as u32)),
            0x24,
        );
    }
    crv
}

/// `tmp[0..count] = zero point` (the origin with weight 1.0).
fn zero_points(m: &mut Mem, tmp: u32, count: i32) {
    for i in 0..count.max(0) as u32 {
        crate::fit::put_zero_point(m, tmp.wrapping_add(0x24u32.wrapping_mul(i)));
    }
}

/// The rows `first .. first + cnt` of `mat` against the points `pts[first ..]`: `tmp[e] += row[e + shift] * pts[r]`
/// for `e < cols` (the same point for all the columns of a row).
fn rows_into(m: &mut Mem, mat: u32, pts: u32, tmp: u32, first: i32, cnt: i32, cols: i32, shift: u32) {
    let rows = m.u32(mat.wrapping_add(8));
    let mut r = first;
    let mut d = 0;
    while d < cnt {
        let row = m.u32(rows.wrapping_add(4u32.wrapping_mul(r as u32)));
        let pt = pts.wrapping_add(0x24u32.wrapping_mul(r as u32));
        d += 1;
        r = r.wrapping_add(1);
        let mut e = 0;
        while e < cols {
            let coef = x(m.f64(row.wrapping_add(8u32.wrapping_mul(e as u32 + shift))));
            crate::fit::axpy_point(m, tmp.wrapping_add(0x24u32.wrapping_mul(e as u32)), coef, pt);
            e += 1;
        }
    }
}

/// `out[at ..] += tmp[0 .. count]` coordinate by coordinate (x, y, z; each sum on the register, rounded at the store).
fn add_points(m: &mut Mem, out: u32, tmp: u32, count: i32, at: i32, wrap: Option<i32>) {
    let mut at = at;
    for i in 0..count.max(0) as u32 {
        let o = out.wrapping_add(0x24u32.wrapping_mul(at as u32));
        let t = tmp.wrapping_add(0x24u32.wrapping_mul(i));
        for c in 0..3u32 {
            let r = x(m.f64(o.wrapping_add(8 * c))) + m.f64(t.wrapping_add(8 * c));
            m.set_f64(o.wrapping_add(8 * c), r.to_f64());
        }
        at = at.wrapping_add(1);
        if wrap == Some(at) {
            at = 0;
        }
    }
}

/// [soft 0x084d1f74 multVecByEvalMatTrans]: `(seg, mat, closed, pts, n, out)`: the transpose of
/// `multVecByEvalMat (0x084d1cb8)`: the product of the transposed evaluation matrix with the `n` points `pts`
/// (0x24 bytes each), added into `out`. `out[0 .. n]` is first set to the origin with weight 1.0; every segment
/// then works in a block of `k` points (the matrix's order, at +4: `SI_calloc` for `k > 4`, else a stack block;
/// that one is not cleared between uses beyond the origin fill) in which row `r` of the segment adds `row[e] * pts[r]`
/// to block entry `e`, and the block is added (x, y, z) into `out` from a start index that moves one point per segment
/// (wrapping at `n`). An open curve: the first segment uses columns 1 ..= `k - 2` (`k - 1` when it is not also the last)
/// and its block replaces `out[0 ..]`, the last uses `k - 1` columns from column 0 and does not wrap; a closed curve
/// starts at `n - k + 2`. Returns 1, 0 when the block cannot be had (nothing else is done then).
pub fn mult_vec_by_eval_mat_trans(m: &mut Mem, seg: u32, mat: u32, closed: i32, pts: u32, n: i32, out: u32) -> i32 {
    let sp = m.sp();
    let zero = m.local(0x24);
    crate::fit::put_zero_point(m, zero);
    let k = m.i32(mat.wrapping_add(4));
    let stack_blk = m.local(0x90);
    let mut cur = seg;
    let tmp = if k > 4 {
        let t = m.si_calloc(k as u32, 0x24);
        if t == 0 {
            m.set_sp(sp);
            return 0;
        }
        t
    } else {
        stack_blk
    };
    let _ = zero;
    for i in 0..n.max(0) as u32 {
        crate::fit::put_zero_point(m, out.wrapping_add(0x24 * i));
    }
    let mut c4: i32;
    if closed == 0 {
        zero_points(m, tmp, k);
        let cnt = m.i32(seg.wrapping_add(0x14));
        let cols = if m.u32(seg.wrapping_add(4)) != 0 { k.wrapping_sub(1) } else { k.wrapping_sub(2) };
        rows_into(m, mat, pts, tmp, 0, cnt, cols, 1);
        for i in 0..cols.max(0) as u32 {
            m.copy(out.wrapping_add(0x24 * i), tmp.wrapping_add(0x24 * i), 0x24);
        }
        cur = m.u32(seg.wrapping_add(4));
        c4 = 0;
    } else {
        c4 = n.wrapping_sub(k).wrapping_add(2);
    }
    if cur != 0 && (closed != 0 || m.u32(cur.wrapping_add(4)) != 0) {
        loop {
            let next = c4.wrapping_add(1);
            zero_points(m, tmp, k);
            let cnt = m.i32(cur.wrapping_add(0x14));
            let mut first = m.i32(cur.wrapping_add(0x10));
            if closed == 0 {
                first = first.wrapping_sub(1);
            }
            rows_into(m, mat, pts, tmp, first, cnt, k, 0);
            add_points(m, out, tmp, k, c4, Some(n));
            c4 = next;
            if c4 == n {
                c4 = 0;
            }
            cur = m.u32(cur.wrapping_add(4));
            if cur == 0 {
                break;
            }
            if closed == 0 && m.u32(cur.wrapping_add(4)) == 0 {
                break;
            }
        }
    }
    if closed == 0 && cur != 0 {
        let dk = k.wrapping_sub(1);
        zero_points(m, tmp, k);
        let cnt = m.i32(cur.wrapping_add(0x14));
        let first = m.i32(cur.wrapping_add(0x10)).wrapping_sub(1);
        rows_into(m, mat, pts, tmp, first, cnt, dk, 0);
        add_points(m, out, tmp, dk, c4, None);
    }
    if k > 4 && tmp != 0 {
        m.free(tmp);
    }
    m.set_sp(sp);
    1
}

pub const PARTITION_LINEAR_DATA: Routine = Routine::new("partitionLinearData", 0x084c_e53c);
pub const FIT_NURBS_POINTS: Routine = Routine::new("fitNurbsPoints", 0x084d_081c);

/// SI_fitNurbsCurve (0x084ce350): `(crv, n, a, b, angle)`: fits curves to the `n` sample points of `crv`'s record
/// (its points at +0x2c): with two points the curve itself is the answer; else `partitionLinearData` (through
/// `o`, given `cos(angle * 0.017453314)`) cuts the data into pieces (the direction records, the piece count,
/// the points, the parameters, the starting indices and the per-piece values), `fitNurbsPoints` (through `o`, after
/// `SI_showActivity`) fits each piece, and `nurbsCrvFromComponents` joins the fitted pieces into the new curve
/// (its closing flag is the old curve's). Every piece's result is freed and the partition cleaned up (the points
/// array is left alone when it is the curve's own). 0 when the partition, the array of results or a fit fails.
/// Not checked on its own: the callees it needs are answered by a stand-in. Written from the listing, not cited.
pub fn fit_nurbs_curve(m: &mut Mem, o: &mut dyn Outside, crv: u32, n: i32, a2: u32, a3: u32, angle: f64) -> u32 {
    let c = (x(angle) * x(f64::from_bits(0x3f91_df48_1358_e711))).to_f64();
    let cs = c.cos();
    if n == 2 {
        return crv;
    }
    let sp = m.sp();
    let l1c = m.local(4);
    let l18 = m.local(4);
    let l14 = m.local(4);
    let l10 = m.local(4);
    let l0c = m.local(4);
    let l08 = m.local(4);
    let l04 = m.local(4);
    let bits = cs.to_bits();
    let r = o.call(m, PARTITION_LINEAR_DATA, &[crv, bits as u32, (bits >> 32) as u32, n as u32, l1c, l18, l14, l10, l0c, l08, l04]).eax;
    if r & 0xffff == 0 {
        m.set_sp(sp);
        return 0;
    }
    let (v1c, v18, v14, v10, v08, v04) = (m.u32(l1c), m.u32(l18), m.u32(l14), m.u32(l10), m.u32(l08), m.u32(l04));
    let cond14 = if m.u32(crv.wrapping_add(0x2c)) == v14 { 0 } else { v14 };
    let res = m.si_calloc(v18 as u32, 4);
    if res != 0 {
        let mut off = 0u32;
        let mut i = 0i32;
        while i < v18 as i32 {
            o.call(m, SI_SHOW_ACTIVITY, &[]);
            let first = m.u32(v08.wrapping_add(4 * i as u32));
            let r = o
                .call(
                    m,
                    FIT_NURBS_POINTS,
                    &[
                        off.wrapping_add(v1c),
                        a2,
                        a3,
                        m.u32(v04.wrapping_add(4 * i as u32)),
                        v14.wrapping_add(0x24u32.wrapping_mul(first)),
                        v10.wrapping_add(8u32.wrapping_mul(first)),
                    ],
                )
                .eax;
            m.set_u32(res.wrapping_add(4 * i as u32), r);
            if r == 0 {
                for j in 0..i as u32 {
                    let p = m.u32(res.wrapping_add(4 * j));
                    if p != 0 {
                        m.free(p);
                        m.set_u32(res.wrapping_add(4 * j), 0);
                    }
                }
                m.free(res);
                cleanup_partition(m, v1c, v18 as i32, cond14, v10, v08, v04);
                m.set_sp(sp);
                return 0;
            }
            off = off.wrapping_add(0x24);
            i += 1;
        }
        let closed = m.i16(crv.wrapping_add(0x20)) as i32;
        let out = nurbs_crv_from_components(m, v1c, v18 as i32, res, closed);
        for j in 0..v18 {
            let p = m.u32(res.wrapping_add(4 * j));
            if p != 0 {
                m.free(p);
                m.set_u32(res.wrapping_add(4 * j), 0);
            }
        }
        m.free(res);
        cleanup_partition(m, v1c, v18 as i32, cond14, v10, v08, v04);
        m.set_sp(sp);
        return out;
    }
    cleanup_partition(m, v1c, v18 as i32, cond14, v10, v08, v04);
    m.set_sp(sp);
    0
}
