// SPDX-License-Identifier: MIT
//! The pseudo-inverse the solver's steps take of the root's Jacobian, on the image.
//!
//! `si-math` has the routine on values ([`si_math::matrix_gen_dbl`], measured there); the
//! solver's matrices are heap records a root's `SI_InvKin` points to, and the routine
//! allocates four work vectors with `SI_malloc` and frees them with `free`, so the image
//! form is written here, from that one: the same operations in the same order.

use si_mem::{Mem, Ptr32};
use si_x87::{fmov64 as fmovd, x, F80};

use super::{mat_at, GenMat};

/// The double 1e-10 (0x08662558) of `vtnInvDbl`, the double 0.001 (0x08662560) of the
/// column test.
const VTN_INV_EPS: f64 = 1e-10;
const GREVILLE_EPS: f64 = 0.001;

/// [soft 0x0851c670 vtnAnormDbl]: the sum of the magnitudes, unrounded (in the register
/// from +0.0, each addition `|v[i]| + sum`).
fn vtn_anorm_dbl(v: &[f64]) -> F80 {
    let mut sum = F80::ZERO;
    for &e in v {
        sum = x(e).abs() + sum;
    }
    sum
}

/// [soft 0x0851c694 vtnInvDbl]: `out = v / (v · v)`, or zeros when `v · v` is not above
/// 1e-10; the sum in the register, the reciprocal unrounded, each product rounded once.
fn vtn_inv_dbl(v: &[f64], out: &mut [f64]) {
    let mut s = F80::ZERO;
    for &e in v {
        s = x(e) * x(e) + s;
    }
    if x(VTN_INV_EPS).lt(s) {
        let r = F80::ONE / s;
        for (o, &e) in out.iter_mut().zip(v) {
            *o = (x(e) * r).to_f64();
        }
    } else {
        for o in out.iter_mut() {
            *o = 0.0;
        }
    }
}

/// The Moore–Penrose pseudo-inverse of `a` (m × n, the counts read from its header) into
/// `out` by Greville's column recursion [soft 0x0851c710 SI_generalMatrixGrevilleInverseDbl].
/// Always 1.
///
/// Four work vectors are taken with `SI_malloc` (m, m, m and n doubles, in that order: the
/// column, the new row, the column's part outside the span of the earlier ones, and the
/// column through the pseudo-inverse so far) and given back with `free` in the same order;
/// none is tested for null. The arithmetic is `si-math`'s
/// `general_matrix_greville_inverse_dbl` (finding 0008): the inner sums of `d` and `c` are
/// stored to double in every iteration, the dependent-column branch keeps its sums in the
/// register. What the work vectors hold when they are freed is what the original leaves in
/// them (they are written back before the `free`).
pub fn greville_inverse_dbl(m: &mut Mem, a: Ptr32<GenMat>, out: Ptr32<GenMat>) -> i32 {
    let rows = m[a].rows;
    let cols = m[a].cols;
    let mu = rows.max(0) as usize;
    let nu = cols.max(0) as usize;
    let b_col = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_kv = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_c = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_d = m.si_malloc((cols as u32) << 3);
    // what the blocks held (SI_malloc's markers and the fill): the parts of them the
    // routine never writes keep it
    let load = |m: &Mem, b: u32, n: usize| (0..n).map(|i| m.f64(b + 8 * i as u32)).collect::<Vec<f64>>();
    let mut col = load(m, b_col, mu);
    let mut kv = load(m, b_kv, mu);
    let mut c = load(m, b_c, mu);
    let mut d = load(m, b_d, nu);
    let get = |m: &Mem, mat: Ptr32<GenMat>, i: usize, j: usize| m.f64(mat_at(m, mat, i as u32, j as u32));
    let set = |m: &mut Mem, mat: Ptr32<GenMat>, i: usize, j: usize, v: f64| {
        let at = mat_at(m, mat, i as u32, j as u32);
        m.set_f64(at, v);
    };

    for i in 0..mu {
        col[i] = fmovd(get(m, a, i, 0));
    }
    vtn_inv_dbl(&col, &mut kv);
    for i in 0..mu {
        set(m, out, 0, i, fmovd(kv[i]));
    }
    for k in 1..nu {
        for i in 0..mu {
            col[i] = fmovd(get(m, a, i, k));
        }
        for j in 0..k {
            d[j] = 0.0;
            for i in 0..mu {
                d[j] = (x(get(m, out, j, i)) * col[i] + d[j]).to_f64();
            }
        }
        for i in 0..mu {
            c[i] = fmovd(col[i]);
            for j in 0..k {
                c[i] = (c[i] - x(get(m, a, i, j)) * d[j]).to_f64();
            }
        }
        let limit = vtn_anorm_dbl(&col) * GREVILLE_EPS;
        let rest = vtn_anorm_dbl(&c);
        if limit.gt(rest) {
            let mut s = F80::ONE;
            for &e in &d[..k] {
                s = x(e) * x(e) + s;
            }
            let r = F80::ONE / s;
            for i in 0..mu {
                let mut t = F80::ZERO;
                for j in 0..k {
                    t = x(get(m, out, j, i)) * d[j] + t;
                }
                kv[i] = (t * r).to_f64();
            }
        } else {
            vtn_inv_dbl(&c, &mut kv);
        }
        for i in 0..mu {
            for j in 0..k {
                let v = (get(m, out, j, i) - x(d[j]) * kv[i]).to_f64();
                set(m, out, j, i, v);
            }
            set(m, out, k, i, fmovd(kv[i]));
        }
    }
    let store = |m: &mut Mem, b: u32, v: &[f64]| {
        for (i, e) in v.iter().enumerate() {
            m.set_f64(b + 8 * i as u32, *e);
        }
    };
    store(m, b_col, &col);
    store(m, b_kv, &kv);
    store(m, b_c, &c);
    store(m, b_d, &d);
    for b in [b_col, b_kv, b_c, b_d] {
        if b != 0 {
            m.free(b);
        }
    }
    1
}
