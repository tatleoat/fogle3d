// SPDX-License-Identifier: MIT
//! The small point-vector routines of `curveFit.c` (package 14, finding 0025, section 14.9): arrays
//! of control points of 0x24 bytes each (x, y, z at +0, +8, +0x10 as doubles and the weight 1.0 at
//! +0x18). Sums run on the 80-bit register and are stored as doubles, as in the original.

use si_mem::Mem;
use si_x87::F80;

const STRIDE: u32 = 0x24;

pub(crate) fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

fn set_one(m: &mut Mem, at: u32) {
    m.set_u32(at, 0);
    m.set_u32(at.wrapping_add(4), 0x3ff0_0000);
}

/// [soft 0x084d007c SI_NurbsPointVecSubtractXYZ]: `(n, a, b, out)`: for each of `n` points
/// (`n <= 0`: none) `out = a - b` in x, y and z (each difference rounded to a double), and the
/// weight 1.0 written at +0x18.
pub fn point_vec_subtract_xyz(m: &mut Mem, n: i32, a: u32, b: u32, out: u32) {
    let mut o = 0u32;
    let mut k = n;
    while k > 0 {
        for c in 0..3u32 {
            let d = x(m.f64(a.wrapping_add(o + 8 * c))) - x(m.f64(b.wrapping_add(o + 8 * c)));
            if c == 2 {
                set_one(m, out.wrapping_add(o + 0x18));
            }
            m.set_f64(out.wrapping_add(o + 8 * c), d.to_f64());
        }
        o = o.wrapping_add(STRIDE);
        k -= 1;
    }
}

/// [soft 0x084d0144 SI_NurbsPointVecInnerProduct]: `(n, a, b, px, py, pz)`: the sums over the `n`
/// points of `a.x * b.x`, `a.y * b.y` and `a.z * b.z`, accumulated on the register (the product
/// of two doubles and the running sum are not rounded between steps), each stored as a double.
pub fn point_vec_inner_product(m: &mut Mem, n: i32, a: u32, b: u32, px: u32, py: u32, pz: u32) {
    let mut s = [F80::from_f64(0.0); 3];
    let mut o = 0u32;
    let mut k = n;
    while k > 0 {
        for c in 0..3u32 {
            let p = x(m.f64(a.wrapping_add(o + 8 * c))) * x(m.f64(b.wrapping_add(o + 8 * c)));
            s[c as usize] = s[c as usize] + p;
        }
        o = o.wrapping_add(STRIDE);
        k -= 1;
    }
    m.set_f64(px, s[0].to_f64());
    m.set_f64(py, s[1].to_f64());
    m.set_f64(pz, s[2].to_f64());
}

/// [soft 0x084d00d4 SI_NurbsPointVecGaxpy]: `(n, sx, sy, sz, a, b, out)` (the three scales are doubles):
/// `out = s * a + b` per coordinate (the product and the sum on the register, one rounding at the
/// store), the weight 1.0 written at +0x18.
pub fn point_vec_gaxpy(m: &mut Mem, n: i32, s: [f64; 3], a: u32, b: u32, out: u32) {
    let mut o = 0u32;
    let mut k = n;
    while k > 0 {
        for c in 0..3u32 {
            let r = x(s[c as usize]) * x(m.f64(a.wrapping_add(o + 8 * c))) + x(m.f64(b.wrapping_add(o + 8 * c)));
            if c == 2 {
                set_one(m, out.wrapping_add(o + 0x18));
            }
            m.set_f64(out.wrapping_add(o + 8 * c), r.to_f64());
        }
        o = o.wrapping_add(STRIDE);
        k -= 1;
    }
}

/// [soft 0x084cf5c8 classifySample]: `(p1, p2, p3, tol)` (three points by value, only x, y, z
/// matter, and a double): 1 when `tol > (p1 - p2) . (p3 - p1)`, else 0 (also for a NaN). The
/// differences and the sum of products stay on the 80-bit register.
pub fn classify_sample(p1: [f64; 3], p2: [f64; 3], p3: [f64; 3], tol: f64) -> i32 {
    let t = |i: usize| x(p1[i]) - x(p2[i]);
    let d = |i: usize| x(p3[i]) - x(p1[i]);
    let dot = t(2) * d(2) + (t(1) * d(1) + t(0) * d(0));
    x(tol).gt(dot) as i32
}

/// [soft 0x084d29bc freeSegmentList]: `(head)`: frees every cell of the list whose first cell the
/// word `*head` points at; a cell's link is the word at +4 (read before the cell is freed). The word
/// `*head` is left as it was.
pub fn free_segment_list(m: &mut Mem, head: u32) {
    let mut p = m.u32(head);
    while p != 0 {
        let next = m.u32(p.wrapping_add(4));
        m.free(p);
        p = next;
    }
}

/// [soft 0x084cf64c cleanupPartition]: `(arr, n, a, b, c, d)`: frees `a`, `b`, `c` and `d` when they
/// are not null, then the pointer at +0xc of each of the `n` records of 0x24 bytes of `arr` (the
/// pointer is cleared), then `arr`.
pub fn cleanup_partition(m: &mut Mem, arr: u32, n: i32, a: u32, b: u32, c: u32, d: u32) {
    for p in [a, b, c, d] {
        if p != 0 {
            m.free(p);
        }
    }
    let mut k = n;
    let mut o = 0u32;
    while k > 0 {
        let at = arr.wrapping_add(o).wrapping_add(0xc);
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            m.set_u32(at, 0);
        }
        o = o.wrapping_add(STRIDE);
        k -= 1;
    }
    if arr != 0 {
        m.free(arr);
    }
}

/// [soft 0x084d29f4 freeEvalTemps]: `(n, tab, extra)`: frees and clears each non-null pointer of the
/// first `n` words of `tab`, then `tab` and `extra` (each when not null).
pub fn free_eval_temps(m: &mut Mem, n: i32, tab: u32, extra: u32) {
    let mut i = 0i32;
    while i < n {
        let at = tab.wrapping_add((i as u32).wrapping_mul(4));
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            m.set_u32(at, 0);
        }
        i += 1;
    }
    if tab != 0 {
        m.free(tab);
    }
    if extra != 0 {
        m.free(extra);
    }
}

/// [soft 0x084cf9a0 adjustOpenData]: `(n, out, src, a, b)`: moves the fixed end points of an open fit out of
/// the data. `out[0..n]` get the points `src[1..=n]` (0x24 bytes each); then the first point `src[0]`
/// times the first entry of every row of `b`'s table (`a`'s count of them) is taken off `out[0..]`, and the
/// point `src[n + 1]` times the entry `b.count - 1` of the rows starting at the last cell of the chain of
/// `a` (links at +4) is taken off `out[first row..]`. Each product and difference stays on the register,
/// one rounding at the store. `a`: count at +0x14, link +4, first row +0x10; `b`: count +4, row table +8.
pub fn adjust_open_data(m: &mut Mem, n: i32, out: u32, src: u32, a: u32, b: u32) {
    let last_col = m.i32(b.wrapping_add(4)).wrapping_sub(1);
    let mut k = n;
    let mut o = 0u32;
    let mut s = STRIDE;
    while k > 0 {
        m.copy(out.wrapping_add(o), src.wrapping_add(s), STRIDE);
        o = o.wrapping_add(STRIDE);
        s = s.wrapping_add(STRIDE);
        k -= 1;
    }
    let sub_row = |m: &mut Mem, at: u32, v: F80, p: [F80; 3]| {
        for c in 0..3u32 {
            let q = at.wrapping_add(8 * c);
            let r = x(m.f64(q)) - v * p[c as usize];
            m.set_f64(q, r.to_f64());
        }
    };
    let p0 = [x(m.f64(src)), x(m.f64(src.wrapping_add(8))), x(m.f64(src.wrapping_add(0x10)))];
    let cnt = m.i32(a.wrapping_add(0x14));
    let tab = m.u32(b.wrapping_add(8));
    let mut j = 0i32;
    let mut at = out;
    while j < cnt {
        let row = m.u32(tab.wrapping_add((j as u32).wrapping_mul(4)));
        let v = x(m.f64(row));
        sub_row(m, at, v, p0);
        at = at.wrapping_add(STRIDE);
        j += 1;
    }
    let mut last = a;
    while m.u32(last.wrapping_add(4)) != 0 {
        last = m.u32(last.wrapping_add(4));
    }
    let at1 = src.wrapping_add((n.wrapping_add(1) as u32).wrapping_mul(STRIDE));
    let p1 = [x(m.f64(at1)), x(m.f64(at1.wrapping_add(8))), x(m.f64(at1.wrapping_add(0x10)))];
    let cnt = m.i32(last.wrapping_add(0x14));
    let mut r = m.i32(last.wrapping_add(0x10)).wrapping_sub(1);
    let mut at = out.wrapping_add((r as u32).wrapping_mul(STRIDE));
    let mut k = cnt;
    while k > 0 {
        let row = m.u32(tab.wrapping_add((r as u32).wrapping_mul(4)));
        let v = x(m.f64(row.wrapping_add((last_col as u32).wrapping_mul(8))));
        sub_row(m, at, v, p1);
        r = r.wrapping_add(1);
        at = at.wrapping_add(STRIDE);
        k -= 1;
    }
}

/// [soft 0x084cfae0 nurbsPowerBasis]: `(dir, i, out)`: the coefficients, in powers of the parameter, of the
/// order 3 or 4 basis functions of the span `i` of a periodic knot vector (`dir`: order at +0, knot count at
/// +8, knots at +0xc; `out`: one row of `order` doubles per basis function, in `out[0..order]`). The
/// `2 * order - 2` knots round the span are taken with the periodic wrap (below the first: first minus last
/// plus knot `idx + count - 1`; at or above the count: last minus first plus knot `idx - (count - 1)`). The
/// coefficient formulas run on the register: values the original keeps on the stack (the reciprocals `t1`..
/// `t8` below) are not rounded where it does not store them, and each store is one rounding. Returns 1;
/// 0 for an order other than 3, 4 or 5 or when the scratch cannot be had (order 5 is accepted and nothing
/// is written). Order 6 and above allocate the scratch with `SI_calloc` and free it.
/// Where the original would free the address of its own stack buffer (an order below 3), the port frees nothing.
pub fn nurbs_power_basis(m: &mut Mem, dir: u32, i: i32, out: u32) -> i32 {
    let k = m.i32(dir);
    let n = m.i32(dir.wrapping_add(8));
    let cnt = k.wrapping_mul(2).wrapping_sub(2);
    let nm1 = n.wrapping_sub(1);
    let kn = m.u32(dir.wrapping_add(0xc));
    let mut heap = 0u32;
    if k > 5 {
        heap = m.si_calloc(cnt as u32, 8);
        if heap == 0 {
            return 0;
        }
    }
    let kv = |m: &Mem, idx: i32| m.f64(kn.wrapping_add((idx as u32).wrapping_mul(8)));
    let mut buf: Vec<f64> = Vec::new();
    let mut idx = i.wrapping_sub(k).wrapping_add(2);
    let mut e = 0i32;
    while e < cnt {
        let v = if idx < 0 {
            ((x(kv(m, 0)) - x(kv(m, nm1))) + x(kv(m, idx.wrapping_add(nm1)))).to_f64()
        } else if idx >= n {
            ((x(kv(m, nm1)) - x(kv(m, 0))) + x(kv(m, idx.wrapping_sub(nm1)))).to_f64()
        } else {
            kv(m, idx)
        };
        if heap != 0 {
            m.set_f64(heap.wrapping_add((e as u32).wrapping_mul(8)), v);
        } else {
            buf.push(v);
        }
        idx = idx.wrapping_add(1);
        e += 1;
    }
    let g = |m: &Mem, j: usize| if heap != 0 { m.f64(heap + 8 * j as u32) } else { buf[j] };
    let one = x(1.0);
    let row = |m: &Mem, r: u32| m.u32(out.wrapping_add(4 * r));
    let ok = match k {
        3 => {
            let (a, b, c, d) = (g(m, 0), g(m, 1), g(m, 2), g(m, 3));
            let t1 = one / ((x(c) - x(a)) * (x(c) - x(b)));
            let t2 = one / ((x(c) - x(b)) * (x(d) - x(b)));
            let l90 = t1.to_f64();
            let l98 = t2.to_f64();
            let r0 = row(m, 0);
            m.set_f64(r0, t1.to_f64());
            m.set_f64(r0 + 8, ((x(c) * x(-2.0)) * x(l90)).to_f64());
            m.set_f64(r0 + 0x10, ((x(c) * x(c)) * x(l90)).to_f64());
            let r1 = row(m, 1);
            m.set_f64(r1, (-(x(l90) + x(l98))).to_f64());
            m.set_f64(r1 + 8, (((x(d) + x(b)) * x(l98)) + ((x(a) + x(c)) * x(l90))).to_f64());
            let l90 = (((x(l90) * x(a)) * x(c)).to_f64()) as f64;
            m.set_f64(r1 + 0x10, (-(((x(l98) * x(d)) * x(b)) + x(l90))).to_f64());
            let r2 = row(m, 2);
            m.set_f64(r2, l98);
            m.set_f64(r2 + 8, ((x(-2.0) * x(b)) * x(l98)).to_f64());
            m.set_f64(r2 + 0x10, ((x(b) * x(b)) * x(l98)).to_f64());
            true
        }
        4 => {
            let (a, b, c, d, e, f) = (g(m, 0), g(m, 1), g(m, 2), g(m, 3), g(m, 4), g(m, 5));
            let t2 = x(e) - x(b);
            let l90 = (one / (((x(d) - x(a)) * (x(d) - x(b))) * (x(d) - x(c)))).to_f64();
            let l98 = (one / (((x(d) - x(b)) * t2) * (x(d) - x(c)))).to_f64();
            let r0 = row(m, 0);
            m.set_f64(r0, (-x(l90)).to_f64());
            m.set_f64(r0 + 8, ((x(d) * x(3.0)) * x(l90)).to_f64());
            m.set_f64(r0 + 0x10, (((x(d) * x(-3.0)) * x(d)) * x(l90)).to_f64());
            m.set_f64(r0 + 0x18, (((x(d) * x(d)) * x(d)) * x(l90)).to_f64());
            let t4 = one / ((x(d) - x(c)) * ((x(e) - x(c)) * t2));
            let la0 = (x(d) + x(d)).to_f64();
            let t3 = x(e) + x(b);
            let mut la8 = t3.to_f64();
            let r1 = row(m, 1);
            m.set_f64(r1, ((x(l90) + x(l98)) + t4).to_f64());
            let s44 = ((x(a) + x(la0)) * x(l90)) + ((t3 + x(d)) * x(l98)) + (((x(e) + x(e)) + x(c)) * t4);
            m.set_f64(r1 + 8, (-s44).to_f64());
            let t5 = x(e) * x(b);
            let t6 = x(c) + x(c);
            let term1 = ((x(b) * x(d)) + ((x(e) * x(d)) + t5)) * x(l98);
            let term2 = (((x(a) + x(a)) + x(d)) * x(l90)) * x(d);
            let term3 = ((t6 + x(e)) * t4) * x(e);
            m.set_f64(r1 + 0x10, ((term1 + term2) + term3).to_f64());
            let l90b = (((x(l90) * x(a)) * x(d)) * x(d)).to_f64();
            let t7 = x(l98) * x(b);
            let l90c = (((t7 * x(d)) * x(e)) + x(l90b)).to_f64();
            m.set_f64(r1 + 0x18, (-((((t4 * x(c)) * x(e)) * x(e)) + x(l90c))).to_f64());
            let t8 = one / (((x(f) - x(c)) * (x(e) - x(c))) * (x(d) - x(c)));
            let r2 = row(m, 2);
            m.set_f64(r2, (-((x(l98) + t4) + t8)).to_f64());
            let l98b = ((((x(b) + x(b)) + x(d)) * x(l98))).to_f64();
            la8 = (x(la8) + x(c)).to_f64();
            let l98c = ((t4 * x(la8)) + x(l98b)).to_f64();
            m.set_f64(r2 + 8, (((t6 + x(f)) * t8) + x(l98c)).to_f64());
            let s54 = ((((t5 + (x(e) * x(c))) + (x(b) * x(c))) * t4) + (t7 * (x(b) + x(la0)))) + ((t8 * x(c)) * ((x(f) + x(f)) + x(c)));
            m.set_f64(r2 + 0x10, (-s54).to_f64());
            let s55 = ((((t4 * x(e)) * x(b)) * x(c)) + ((t7 * x(b)) * x(d))) + (((t8 * x(f)) * x(c)) * x(c));
            m.set_f64(r2 + 0x18, s55.to_f64());
            let r3 = row(m, 3);
            m.set_f64(r3, t8.to_f64());
            m.set_f64(r3 + 8, ((x(c) * x(-3.0)) * t8).to_f64());
            m.set_f64(r3 + 0x10, (((x(c) * x(3.0)) * x(c)) * t8).to_f64());
            m.set_f64(r3 + 0x18, (t8 * ((-x(c) * x(c)) * x(c))).to_f64());
            true
        }
        5 => true,
        _ => false,
    };
    if heap != 0 {
        m.free(heap);
    }
    ok as i32
}

/// [soft 0x084d244c computeSegmentNormalEquations]: `(seg, k, flag, mat)`: for the `seg.count` (at +0x14) rows of
/// the basis matrix `mat` (count, columns, row table at +8) starting at row `seg.first` (at +0x10, one less when
/// `flag` is 0), adds up the products of the first `k` entries of each row into the `k (k + 1) / 2` sums of the
/// upper triangle (each sum rounded to a double after every row, the product on the register), then writes them as the
/// symmetric `k x k` block of rows of four doubles from `seg + 0x18` (row stride 0x20). Only orders 3 and 4
/// fill the block; any other order leaves it (the original still allocates and zeroes the sums above
/// order 4). Returns 1, or 0 when the scratch of an order above 4 cannot be allocated.
pub fn compute_segment_normal_equations(m: &mut Mem, seg: u32, k: i32, flag: i32, mat: u32) -> i32 {
    let t = k.wrapping_add(1).wrapping_mul(k);
    let nsum = (t.wrapping_add(((t as u32) >> 31) as i32)) >> 1;
    let mut heap = 0u32;
    if k > 4 {
        heap = m.si_calloc(nsum as u32, 8);
        if heap == 0 {
            return 0;
        }
    }
    // order 4 and below: a local array of ten doubles, the first `nsum` zeroed
    let mut s = vec![0.0f64; 10.max(nsum.max(0) as usize)];
    let mut row = m.i32(seg.wrapping_add(0x10));
    if flag == 0 {
        row = row.wrapping_sub(1);
    }
    let mut cnt = m.i32(seg.wrapping_add(0x14));
    let tab = m.u32(mat.wrapping_add(8));
    let (order, pairs): (usize, &[(usize, usize)]) = match k {
        3 => (3, &[(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)]),
        4 => (4, &[(0, 0), (0, 1), (0, 2), (0, 3), (1, 1), (1, 2), (1, 3), (2, 2), (2, 3), (3, 3)]),
        _ => (0, &[]),
    };
    if order != 0 {
        while cnt > 0 {
            let p = m.u32(tab.wrapping_add((row as u32).wrapping_mul(4)));
            for (n, &(i, j)) in pairs.iter().enumerate() {
                let a = x(m.f64(p.wrapping_add(8 * i as u32)));
                let b = x(m.f64(p.wrapping_add(8 * j as u32)));
                s[n] = (a * b + x(s[n])).to_f64();
            }
            row = row.wrapping_add(1);
            cnt -= 1;
        }
        for r in 0..order {
            for c in 0..order {
                let n = pairs.iter().position(|&(i, j)| (i, j) == (r.min(c), r.max(c))).unwrap();
                m.set_f64(seg.wrapping_add(0x18 + 0x20 * r as u32 + 8 * c as u32), s[n]);
            }
        }
    }
    if heap != 0 {
        m.free(heap);
    }
    1
}

/// [soft 0x084d26c4 plusEqualSegmentNormalEqns]: `(row, col, k, flag, seg, mat)`: adds the `k x k` block of the
/// segment `seg` (rows of four doubles from +0x18, stride 0x20; its link at +4) into the normal-equation
/// matrix `mat` (rows at +0, columns at +4, row table at +8) with its corner at (`row`, `col`). Rows outside
/// `0..rows` are skipped. With `flag` 0 the columns run on without wrapping while the matrix has fewer than
/// `2 * k` rows (stopping at the last column; a negative column is skipped), else they wrap to 0 (over
/// `k` columns, `k - 1` for the last segment of a chain); with `flag` non-zero a negative start column has
/// `cols` added and the columns always wrap. Each sum is a double added on the register and rounded at the store.
pub fn plus_equal_segment_normal_eqns(m: &mut Mem, mut row: i32, col0: i32, k: i32, flag: i32, seg: u32, mat: u32) {
    let cols = m.i32(mat.wrapping_add(4));
    let rows = m.i32(mat);
    let tab = m.u32(mat.wrapping_add(8));
    let add = |m: &mut Mem, p: u32, col: i32, r: i32, c: i32| {
        let at = p.wrapping_add((col as u32).wrapping_mul(8));
        let src = seg.wrapping_add((c as u32).wrapping_mul(8)).wrapping_add((r as u32).wrapping_mul(0x20)).wrapping_add(0x18);
        let v = x(m.f64(at)) + x(m.f64(src));
        m.set_f64(at, v.to_f64());
    };
    let mut r = 0i32;
    if flag == 0 {
        if rows < k.wrapping_mul(2) {
            while r < k {
                if -1 < row && row < rows {
                    let p = m.u32(tab.wrapping_add((row as u32).wrapping_mul(4)));
                    let mut col = col0;
                    let mut c = 0;
                    while c < k {
                        if -1 < col {
                            add(m, p, col, r, c);
                        }
                        col = col.wrapping_add(1);
                        if col == cols {
                            break;
                        }
                        c += 1;
                    }
                }
                r += 1;
                row = row.wrapping_add(1);
            }
        } else {
            let mut lim = k;
            if m.i32(seg.wrapping_add(4)) == 0 {
                lim = k.wrapping_sub(1);
            }
            while r < k {
                if -1 < row && row < rows {
                    let p = m.u32(tab.wrapping_add((row as u32).wrapping_mul(4)));
                    let mut col = col0;
                    let mut c = 0;
                    while c < lim {
                        if -1 < col {
                            add(m, p, col, r, c);
                        }
                        col = col.wrapping_add(1);
                        if col == cols {
                            col = 0;
                        }
                        c += 1;
                    }
                }
                r += 1;
                row = row.wrapping_add(1);
            }
        }
    } else {
        while r < k {
            if -1 < row && row < rows {
                let p = m.u32(tab.wrapping_add((row as u32).wrapping_mul(4)));
                let mut col = col0;
                if col0 < 0 {
                    col = col0.wrapping_add(cols);
                }
                let mut c = 0;
                while c < k {
                    add(m, p, col, r, c);
                    col = col.wrapping_add(1);
                    if col == cols {
                        col = 0;
                    }
                    c += 1;
                }
            }
            r += 1;
            row = row.wrapping_add(1);
        }
    }
}

/// [soft 0x084d2844 computeNormalEquations]: `(seg, flag, k, n, mat)`: sets `mat` to `n` rows of `min(2 k - 1, n)`
/// columns, zeroes them, then adds the block of every segment of the chain `seg` (links at +4; a segment with
/// no rows, count at +0x14 not above 0, is skipped but still advances the position) by
/// [`plus_equal_segment_normal_eqns`]. With `flag` non-zero the last segment goes first at (1 - k, 1 - k), the
/// walk starts at (2 - k, 2 - k) and the first `k - 2` segments are added once more at its end
/// (the matrix is periodic); with `flag` 0 it starts at (-1, -1). The column wraps to 0 at the column count.
pub fn compute_normal_equations(m: &mut Mem, seg: u32, flag: i32, k: i32, n: i32, mat: u32) {
    let mut cols = k.wrapping_mul(2).wrapping_sub(1);
    if n < cols {
        cols = n;
    }
    m.set_i32(mat, n);
    m.set_i32(mat.wrapping_add(4), cols);
    let tab = m.u32(mat.wrapping_add(8));
    let mut r = 0;
    while r < n {
        let p = m.u32(tab.wrapping_add((r as u32).wrapping_mul(4)));
        let mut c = 0;
        while c < cols {
            m.set_u32(p.wrapping_add((c as u32).wrapping_mul(8)), 0);
            m.set_u32(p.wrapping_add((c as u32).wrapping_mul(8)).wrapping_add(4), 0);
            c += 1;
        }
        r += 1;
    }
    let (mut col, mut row);
    if flag == 0 {
        col = -1;
        row = -1;
    } else {
        row = (-k).wrapping_add(1);
        let mut last = seg;
        while m.u32(last.wrapping_add(4)) != 0 {
            last = m.u32(last.wrapping_add(4));
        }
        if m.i32(last.wrapping_add(0x14)) > 0 {
            plus_equal_segment_normal_eqns(m, row, row, k, flag, last, mat);
        }
        col = (-k).wrapping_add(2);
        row = col;
    }
    let mut s = seg;
    while s != 0 {
        if m.i32(s.wrapping_add(0x14)) > 0 {
            plus_equal_segment_normal_eqns(m, row, col, k, flag, s, mat);
        }
        col = col.wrapping_add(1);
        if col == cols {
            col = 0;
        }
        row = row.wrapping_add(1);
        s = m.u32(s.wrapping_add(4));
    }
    if flag != 0 {
        let mut left = k.wrapping_sub(2);
        let mut s = seg;
        while left > 0 {
            if m.i32(s.wrapping_add(0x14)) > 0 {
                plus_equal_segment_normal_eqns(m, row, col, k, flag, s, mat);
            }
            row = row.wrapping_add(1);
            col = col.wrapping_add(1);
            if col == cols {
                col = 0;
            }
            s = m.u32(s.wrapping_add(4));
            left -= 1;
        }
    }
}

/// [soft 0x084d1b18 multVecByNrmlEqns]: `(mat, k, pts, out)`: `out = mat * pts` for the square (periodic) normal-equation
/// matrix `mat` (rows at +0, row table at +8) of band `2 k - 1` and the points `pts` (0x24 bytes each: x, y, z used);
/// each output point is first set to (0, 0, 0) with weight 1.0 (words 0 at +0x20), then the row's entries are
/// added in: `d * p + out` for x and y, `out + d * p` for z, product and sum on the register, one rounding at
/// each store. A band narrower than the matrix has the row `i` start at column `k + i` and at point `n - k + 1 + i`
/// (both wrapping), else the row is multiplied with the points `0..n`.
pub fn mult_vec_by_nrml_eqns(m: &mut Mem, mat: u32, k: i32, pts: u32, out: u32) {
    let w = k.wrapping_mul(2).wrapping_sub(1);
    let n = m.i32(mat);
    let tab = m.u32(mat.wrapping_add(8));
    let init = |m: &mut Mem, at: u32| {
        for j in 0..9u32 {
            m.set_u32(at.wrapping_add(4 * j), 0);
        }
        m.set_u32(at.wrapping_add(0x1c), 0x3ff0_0000);
    };
    let acc = |m: &mut Mem, at: u32, d: F80, p: u32| {
        let ox = x(m.f64(at));
        let oy = x(m.f64(at.wrapping_add(8)));
        let oz = x(m.f64(at.wrapping_add(0x10)));
        m.set_f64(at, (d * x(m.f64(p)) + ox).to_f64());
        m.set_f64(at.wrapping_add(8), (d * x(m.f64(p.wrapping_add(8))) + oy).to_f64());
        m.set_f64(at.wrapping_add(0x10), (oz + d * x(m.f64(p.wrapping_add(0x10)))).to_f64());
    };
    if w < n {
        let mut col0 = k;
        let mut idx0 = n.wrapping_sub(k).wrapping_add(1);
        let mut i = 0;
        while i < n {
            let row = m.u32(tab.wrapping_add((i as u32).wrapping_mul(4)));
            let at = out.wrapping_add((i as u32).wrapping_mul(0x24));
            init(m, at);
            i += 1;
            let (mut col, mut idx) = (col0, idx0);
            let mut left = w;
            while left > 0 {
                let d = x(m.f64(row.wrapping_add((col as u32).wrapping_mul(8))));
                acc(m, at, d, pts.wrapping_add((idx as u32).wrapping_mul(0x24)));
                col = col.wrapping_add(1);
                if col == w {
                    col = 0;
                }
                idx = idx.wrapping_add(1);
                if idx == n {
                    idx = 0;
                }
                left -= 1;
            }
            idx0 = idx0.wrapping_add(1);
            if idx0 == n {
                idx0 = 0;
            }
            col0 = col0.wrapping_add(1);
            if col0 == w {
                col0 = 0;
            }
        }
    } else {
        let mut i = 0;
        while i < n {
            let row = m.u32(tab.wrapping_add((i as u32).wrapping_mul(4)));
            let at = out.wrapping_add((i as u32).wrapping_mul(0x24));
            init(m, at);
            i += 1;
            let mut j = 0;
            while j < n {
                let d = x(m.f64(row.wrapping_add((j as u32).wrapping_mul(8))));
                acc(m, at, d, pts.wrapping_add((j as u32).wrapping_mul(0x24)));
                j += 1;
            }
        }
    }
}

// ---- the evaluation matrix (second half of curveFit.c; nurbs6, 2026-10-28) ----

use si_mem::{Outside, Routine};

pub const SI_NURBS_MATRIX_BASIS_FUNCTION: Routine = Routine::new("SI_nurbsMatrixBasisFunction", 0x084d_93c4);
pub const SI_NURBS_CRV_BASIS_FUNCTIONS: Routine = Routine::new("SI_nurbsCrvBasisFunctions", 0x084d_9668);
pub const SI_GENERAL_MATRIX_DISPOSE_DBL: Routine = Routine::new("SI_generalMatrixDisposeDbl", 0x0851_bf50);
pub const SI_SHOW_ACTIVITY: Routine = Routine::new("SI_showActivity", 0x083d_1a60);


/// [soft 0x084d2a60 evaluateSegment]: the order 5 and above path calls the ports of
/// [soft 0x084d93c4 SI_nurbsMatrixBasisFunction] and [soft 0x084d9668 SI_nurbsCrvBasisFunctions]
/// (`fit2.rs`) directly; `o` is not used any more.
///
/// `(dir, seg, i, mat, params, tab, out)`: `seg` is a segment cell (first parameter index at +0x10, count at
/// +0x14); for each of its `count` parameters `t = params[first + j]` the polynomials `tab[r]` (rows of `order`
/// coefficients, from `nurbsPowerBasis(dir, i, tab)`) are evaluated by Horner's rule on the register (one
/// rounding at the store into `out[r]`) and copied into row `first + j` (`first + j - 1` of an open curve) of the
/// matrix `mat` (row pointers at `mat[2]`). Returns 1 (0 when the order 5 path's matrix cannot be had).
pub fn evaluate_segment(m: &mut Mem, _o: &mut dyn Outside, dir: u32, seg: u32, i: i32, mat: u32, params: u32, tab: u32, out: u32) -> i32 {
    let order = m.i32(dir);
    let cnt = m.i32(seg.wrapping_add(0x14));
    if order <= 4 {
        nurbs_power_basis(m, dir, i, tab);
    }
    let first = m.i32(seg.wrapping_add(0x10));
    let mut ebx = first;
    let mut edi = first;
    if m.u16(dir.wrapping_add(0x20)) == 0 {
        edi = edi.wrapping_sub(1);
    }
    let mut out = out;
    let mut j = 0;
    while j < cnt {
        let t = x(m.f64(params.wrapping_add((ebx as u32).wrapping_mul(8))));
        if order <= 4 {
            let mut r = 0;
            while r < order {
                let row = m.u32(tab.wrapping_add(4 * r as u32));
                let mut acc = x(m.f64(row));
                let mut c = 1;
                while c < order {
                    acc = acc * t + x(m.f64(row.wrapping_add(8 * c as u32)));
                    c += 1;
                }
                m.set_f64(out.wrapping_add(8 * r as u32), acc.to_f64());
                r += 1;
            }
        } else {
            // [soft 0x084d93c4 SI_nurbsMatrixBasisFunction] and [soft 0x084d9668 SI_nurbsCrvBasisFunctions], ported in `fit2`
            let tv = t.to_f64();
            let local = m.local(4);
            let mx = crate::fit2::nurbs_matrix_basis_function(m, dir, tv, local, 1);
            if mx == 0 {
                return 0;
            }
            out = crate::fit2::nurbs_crv_basis_functions(m, mx, 1);
            let cell = m.local(4);
            m.set_u32(cell, mx);
            crate::fit2::mat_dispose(m, cell);
        }
        let rows = m.u32(mat.wrapping_add(8));
        let dst = m.u32(rows.wrapping_add((edi as u32).wrapping_mul(4)));
        let mut r = 0;
        while r < order {
            let v = m.f64(out.wrapping_add(8 * r as u32));
            m.set_f64(dst.wrapping_add(8 * r as u32), v);
            r += 1;
        }
        ebx = ebx.wrapping_add(1);
        edi = edi.wrapping_add(1);
        j += 1;
    }
    1
}

const EVAL_EPS: f64 = 1e-6; // 0x08661650: spans shorter than this are not segments

/// [soft 0x084d2bbc computeNurbsEvaluationMatrix]: `(dir, mat, list_out, n, params)`: builds the list of
/// segments (cells of 0x98 bytes: previous at +0, next at +4, knot at +8, first parameter row at +0x10, count
/// at +0x14; `*list_out` receives the first) of the knot spans longer than 1e-6 (a curve open below: from knot
/// `order - 2` to `count - order`, closed: from 0 to `count - 2`), assigns each span the parameters in
/// `params[0..n]` (or `n - 1` for an open curve) that lie below its end knot plus 1e-6, evaluates the basis
/// functions at them into `mat` (`evaluateSegment`) and fills the segment's normal equations
/// (`computeSegmentNormalEquations`). Returns 1; 0 when an allocation or either callee fails (an order of 5
/// or more frees its scratch first). The scratch of an order below 5 lives on the stack, as in the original.
/// `SI_showActivity (0x083d1a60)` is called once per segment through `o`.
pub fn compute_nurbs_evaluation_matrix(m: &mut Mem, o: &mut dyn Outside, dir: u32, mat: u32, list_out: u32, n: i32, params: u32) -> i32 {
    let closed = m.i16(dir.wrapping_add(0x20)) as i32;
    let order = m.i32(dir);
    let knots = m.u32(dir.wrapping_add(0xc));
    let (out, tab);
    if order > 4 {
        out = m.si_calloc(order as u32, 8);
        if out == 0 {
            return 0;
        }
        tab = m.si_calloc(order as u32, 4);
        if tab == 0 {
            m.free(out);
            return 0;
        }
        let mut b = 0;
        while b < order {
            let r = m.si_calloc(order as u32, 8);
            m.set_u32(tab.wrapping_add(4 * b as u32), r);
            if r == 0 {
                free_eval_temps(m, order, tab, out);
                return 0;
            }
            b += 1;
        }
    } else {
        let mut rows = [0u32; 4];
        let o0 = m.local(0x20);
        for r in rows.iter_mut() {
            *r = m.local(0x20);
        }
        let t = m.local(16);
        for (k, r) in rows.iter().enumerate() {
            m.set_u32(t.wrapping_add(4 * k as u32), *r);
        }
        out = o0;
        tab = t;
    }
    let kd = |m: &Mem, i: i32| m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)));
    let eps = x(EVAL_EPS);
    let mut start = 0i32;
    if closed == 0 {
        start = order.wrapping_sub(2);
    }
    let end = if closed == 0 { m.i32(dir.wrapping_add(8)).wrapping_add(1).wrapping_sub(order) } else { m.i32(dir.wrapping_add(8)).wrapping_sub(1) };
    let mut nseg = 0i32;
    let mut b = start;
    while b < end {
        if (x(kd(m, b + 1)) - x(kd(m, b))).gt(eps) {
            nseg += 1;
        }
        b += 1;
    }
    let fail = |m: &mut Mem| {
        if order > 4 {
            free_eval_temps(m, order, tab, out);
        }
        0
    };
    let first = m.si_calloc(1, 0x98);
    if first == 0 && order > 4 {
        return fail(m);
    }
    m.set_u32(list_out, first);
    m.set_u32(first.wrapping_add(4), 0);
    m.set_u32(first, 0);
    let mut cell = first;
    let mut k = 1;
    while k < nseg {
        let nc = m.si_calloc(1, 0x98);
        if nc == 0 && order > 4 {
            return fail(m);
        }
        m.set_u32(cell.wrapping_add(4), nc);
        m.set_u32(nc, cell);
        m.set_u32(nc.wrapping_add(4), 0);
        cell = m.u32(cell.wrapping_add(4));
        k += 1;
    }
    let mut ebx = start;
    let mut row = (closed == 0) as i32;
    let limit = if closed == 0 { n.wrapping_sub(1) } else { n };
    let mut esi = m.u32(list_out);
    let mut done = 0;
    while done < nseg {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        m.set_i32(esi.wrapping_add(0x10), row);
        m.set_f64(esi.wrapping_add(8), kd(m, ebx));
        let k1 = x(kd(m, ebx + 1));
        while row < limit {
            // K1 + 1e-6 on the register against the parameter
            let s = k1 + eps;
            let p = x(m.f64(params.wrapping_add((row as u32).wrapping_mul(8))));
            if !s.gt(p) {
                break;
            }
            row += 1;
        }
        let c = row.wrapping_sub(m.i32(esi.wrapping_add(0x10)));
        m.set_i32(esi.wrapping_add(0x14), c);
        if evaluate_segment(m, o, dir, esi, ebx, mat, params, tab, out) as u16 == 0 {
            return fail(m);
        }
        if compute_segment_normal_equations(m, esi, order, closed, mat) as u16 == 0 {
            return fail(m);
        }
        let next = m.u32(esi.wrapping_add(4));
        ebx = ebx.wrapping_add(1);
        if ebx < end {
            let d = x(kd(m, ebx + 1)) - x(kd(m, ebx));
            if eps.gt(d) {
                loop {
                    ebx = ebx.wrapping_add(1);
                    if ebx >= end {
                        break;
                    }
                    let d = x(kd(m, ebx + 1)) - x(kd(m, ebx));
                    if !d.lt(eps) {
                        break;
                    }
                }
            }
        }
        done += 1;
        esi = next;
    }
    if order > 4 {
        free_eval_temps(m, order, tab, out);
    }
    1
}

/// The origin point written into each output row: zeros, the weight 1.0 at +0x18 (a 9-word record).
pub(crate) fn put_zero_point(m: &mut Mem, at: u32) {
    for w in 0..9u32 {
        m.set_u32(at.wrapping_add(4 * w), 0);
    }
    set_one(m, at.wrapping_add(0x18));
}

/// `out += coef * p` on x, y and z (each product and sum on the register, one rounding at each store).
pub(crate) fn axpy_point(m: &mut Mem, out: u32, coef: F80, p: u32) {
    for c in 0..3u32 {
        let r = coef * x(m.f64(p.wrapping_add(8 * c))) + x(m.f64(out.wrapping_add(8 * c)));
        m.set_f64(out.wrapping_add(8 * c), r.to_f64());
    }
}

/// [soft 0x084d1cb8 multVecByEvalMat]: `(seg, mat, closed, n, pts, out)`: the product of the evaluation matrix
/// `mat` (row pointers at +8, the order `k` at +4) with the `n` control points `pts` (0x24 bytes each), into
/// `out` (one point of 0x24 bytes per row, the origin with weight 1.0 first, then the accumulated products
/// `row[s] * pts[..]`, each coordinate stored as a double). `seg` is the segment list of
/// `computeNurbsEvaluationMatrix` (row count at +0x14, next at +4). An open curve: its first segment skips
/// column 0 and uses `k - 1` columns (`k - 2` after column 0 when it is also the last), the middle segments run
/// `k` columns over the points from a start index that moves one point per segment (wrapping at `n`), the last
/// uses `k - 1` columns; a closed curve starts at point `n - k + 2` and wraps.
pub fn mult_vec_by_eval_mat(m: &mut Mem, seg: u32, mat: u32, closed: i32, n: i32, pts: u32, out: u32) {
    let k = m.i32(mat.wrapping_add(4));
    let rows = m.u32(mat.wrapping_add(8));
    let mut start = if closed != 0 { n.wrapping_sub(k).wrapping_add(2) } else { 0 };
    let mut r: u32 = 0;
    let mut cur = seg;
    let pt = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(STRIDE));
    if closed == 0 {
        let cnt = m.i32(seg.wrapping_add(0x14));
        let mut lim = k;
        if m.u32(seg.wrapping_add(4)) == 0 {
            lim = lim.wrapping_sub(1);
        }
        let mut b = 0;
        while b < cnt {
            let o = out.wrapping_add(r.wrapping_mul(STRIDE));
            put_zero_point(m, o);
            let row = m.u32(rows.wrapping_add(4 * r));
            r = r.wrapping_add(1);
            let mut s = 1;
            while s < lim {
                let coef = x(m.f64(row.wrapping_add(8 * s as u32)));
                axpy_point(m, o, coef, pt(s - 1));
                s += 1;
            }
            b += 1;
        }
        cur = m.u32(seg.wrapping_add(4));
    }
    while cur != 0 && (closed != 0 || m.u32(cur.wrapping_add(4)) != 0) {
        let cnt = m.i32(cur.wrapping_add(0x14));
        let mut b = 0;
        while b < cnt {
            let o = out.wrapping_add(r.wrapping_mul(STRIDE));
            put_zero_point(m, o);
            let row = m.u32(rows.wrapping_add(4 * r));
            r = r.wrapping_add(1);
            let mut at = start;
            let mut s = 0;
            while s < k {
                let coef = x(m.f64(row.wrapping_add(8 * s as u32)));
                axpy_point(m, o, coef, pt(at));
                at += 1;
                if at == n {
                    at = 0;
                }
                s += 1;
            }
            b += 1;
        }
        start += 1;
        if start == n {
            start = 0;
        }
        cur = m.u32(cur.wrapping_add(4));
    }
    if closed == 0 && cur != 0 {
        let cnt = m.i32(cur.wrapping_add(0x14));
        let mut b = 0;
        while b < cnt {
            let o = out.wrapping_add(r.wrapping_mul(STRIDE));
            put_zero_point(m, o);
            let row = m.u32(rows.wrapping_add(4 * r));
            r = r.wrapping_add(1);
            let mut s = 0;
            while s < k - 1 {
                let coef = x(m.f64(row.wrapping_add(8 * s as u32)));
                axpy_point(m, o, coef, pt(start + s));
                s += 1;
            }
            b += 1;
        }
    }
}
