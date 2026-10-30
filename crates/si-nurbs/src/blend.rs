// SPDX-License-Identifier: MIT
//! Knot sequences and the small routines of `blend.c` (package 14) and of `nrb_b.c` /
//! `nrb_geometry.c` that were still missing, on the memory image (finding 0025, section 15).
//!
//! A knot vector is an array of doubles; a direction record (`SI_NurbsDir`, 0x24 bytes) keeps
//! its order at +0, point count at +4, knot count at +8 and knot pointer at +0xc. A knot-list
//! node (`__SI_MeshListNode`, 0x50 bytes) keeps a pointer to a knot value at +0 and the next
//! node at +0x4c; the knot-list routines share the generic list calls `SI_meshAddList`,
//! `SI_meshFreeList` and `SI_meshCntList` of the `blend.c` copy (the program has a second copy of
//! the three in the mesh package; this one is `blend.c`'s).

use crate::fit::x;
use crate::image::Halt;
use si_hrc::image::nurbs::knots_allocate;
use si_mem::{Mem, Outside};
use si_x87::F80;

/// `1e-6`, the double at 0x086615a8 and at 0x08661580: the tolerance for "the same knot".
pub const KNOT_EPS: f64 = 1e-6;

/// [soft 0x084cabf8 reverseKnotSeq]: `(knots, n)`: a new knot array (`si_knotsAllocate(n)`) holding the
/// first `n` doubles of `knots` in reverse order. Returns it.
pub fn reverse_knot_seq(m: &mut Mem, knots: u32, n: i32) -> u32 {
    let out = knots_allocate(m, n);
    for i in 0..n.max(0) as u32 {
        let v = m.f64(knots.wrapping_add(8u32.wrapping_mul(n as u32 - 1 - i)));
        m.set_f64(out.wrapping_add(8 * i), v);
    }
    out
}

/// [soft 0x084cb0b8 extendEndKnotFromDeltas]: `(knots, n, deltas, count)`, `count` the long at `*countp`:
/// `knots[i] = knots[i - 1] + deltas[i - (n - count)]` for `i` from `n - count` below `n`.
pub fn extend_end_knot_from_deltas(m: &mut Mem, knots: u32, n: i32, deltas: u32, countp: u32) {
    let count = m.i32(countp);
    let mut i = n.wrapping_sub(count);
    while i < n {
        let shift = n.wrapping_sub(count);
        let d = i.wrapping_sub(shift);
        let v = x(m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8))) + x(m.f64(deltas.wrapping_add((d as u32).wrapping_mul(8))));
        m.set_f64(knots.wrapping_add((i as u32).wrapping_mul(8)), v.to_f64());
        i = i.wrapping_add(1);
    }
}

/// [soft 0x084cb0f4 extendBeginKnotFromDeltas]: `(knots, n, deltas, countp)`: the span `knots[c + 1] - knots[c]`
/// (`c` the long at `*countp`) is held on the register while `knots[0]` is set to 0 and `knots[i]` becomes
/// `knots[i - 1] + deltas[i - 1]` for `i` from 1 up to `c`; then, for `i` up to `n - 2`, `knots[i] = (knots[i] +
/// knots[i - 1]) - knots[i + 1]` taken with the register's pair `(knots[i] + knots[i-1])` kept from the
/// previous step's value (see the code); the last entry gets the held value added.
pub fn extend_begin_knot_from_deltas(m: &mut Mem, knots: u32, n: i32, deltas: u32, countp: u32) {
    let at = |i: i32| knots.wrapping_add((i as u32).wrapping_mul(8));
    let c = m.i32(countp);
    // st0: the span between knots c and c + 1
    let mut held = x(m.f64(at(c.wrapping_add(1)))) - x(m.f64(at(c)));
    m.set_f64(at(0), 0.0);
    let mut i = 1i32;
    let mut lim = m.i32(countp);
    while i < lim.wrapping_add(1) {
        let v = x(m.f64(at(i - 1))) + x(m.f64(deltas.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8)));
        m.set_f64(at(i), v.to_f64());
        lim = m.i32(countp);
        i += 1;
    }
    let end = n.wrapping_sub(1);
    // The remaining loop works with the held value in the register stack: it is loaded, the previous knot is
    // added to a copy, the next knot is subtracted from the sum (reversed), and the value is stored.
    while i < end {
        let cur = x(m.f64(at(i)));
        let prev = x(m.f64(at(i - 1)));
        // faddp st(2),st: st2 = held + prev
        held = held + prev;
        let next = x(m.f64(at(i + 1)));
        // fsubrp st(1),st: st1 = next - cur
        let r = next - cur;
        // fxch st(1); fstp [knots + i]: the stored one is `held`
        m.set_f64(at(i), held.to_f64());
        held = r;
        i += 1;
    }
    let v = x(m.f64(at(i - 1))) + held;
    m.set_f64(at(i), v.to_f64());
}

/// [soft 0x084cb4e0 buildDeltaVector]: `(n, a, b)`: a new knot array of `n - 1` doubles (`si_knotsAllocate`) holding
/// `a` (and `b`): `n` = 2: `[a]`; 3: `[a, b]`; even: `a` at `n / 2 - 1` and `b` at `n / 2`; odd above 3: `a` at
/// `n / 2` (the rest as the allocator left it); 1: nothing. Returns the array.
pub fn build_delta_vector(m: &mut Mem, n: i32, a: f64, b: f64) -> u32 {
    let out = knots_allocate(m, n.wrapping_sub(1));
    let at = |i: i32| out.wrapping_add((i as u32).wrapping_mul(8));
    if n == 2 {
        m.set_f64(at(0), a);
    } else if n == 3 {
        m.set_f64(at(0), a);
        m.set_f64(at(1), b);
    } else if n & 1 == 0 {
        let h = n / 2;
        m.set_f64(at(h - 1), a);
        m.set_f64(at(h), b);
    } else if n != 1 {
        let h = n / 2;
        m.set_f64(at(h), a);
    }
    out
}

/// [soft 0x084cc8dc checkMult]: `(knots, i, n)`: 1 plus the number of steps from `i` while `i < n - 1` and the
/// next knot differs by less than 1e-6. The answer is a `short` sign-extended.
pub fn check_mult(m: &Mem, knots: u32, i: i32, n: i32) -> i32 {
    let mut count: i16 = 1;
    let eps = x(KNOT_EPS);
    let mut i = i;
    let last = n.wrapping_sub(1);
    while i < last {
        let a = knots.wrapping_add((i as u32).wrapping_mul(8));
        let d = (x(m.f64(a.wrapping_add(8))) - x(m.f64(a))).abs();
        if d.lt(eps) {
            count = count.wrapping_add(1);
            i += 1;
        } else {
            break;
        }
    }
    count as i32
}

/// [soft 0x084ce274 SI_meshAddList]: `(listp, kp)`: pushes a new node (`SI_calloc(1, 0x50)`) holding `kp` onto
/// the list at `*listp`. 0 when the block cannot be had, else 1.
pub fn mesh_add_list(m: &mut Mem, listp: u32, kp: u32) -> i32 {
    let node = m.si_calloc(1, 0x50);
    if node == 0 {
        return 0;
    }
    m.set_u32(node.wrapping_add(0x4c), 0);
    m.set_u32(node, kp);
    m.set_u32(node.wrapping_add(4), 0);
    m.set_u32(node.wrapping_add(8), 0);
    let head = m.u32(listp);
    if head != 0 {
        m.set_u32(node.wrapping_add(0x4c), head);
    }
    m.set_u32(listp, node);
    1
}

/// [soft 0x084ce2c8 SI_meshFreeList]: `(listp)`: frees every node of the list at `*listp` (the lists at +4
/// and +8 of each node first, in that order), then clears `*listp`. Returns 1.
pub fn mesh_free_list(m: &mut Mem, listp: u32) -> i32 {
    let mut cur = m.u32(listp);
    while cur != 0 {
        let node = cur;
        cur = m.u32(node.wrapping_add(0x4c));
        mesh_free_list(m, node.wrapping_add(4));
        mesh_free_list(m, node.wrapping_add(8));
        m.free(node);
    }
    m.set_u32(listp, 0);
    1
}

/// [soft 0x084ce328 SI_meshCntList]: `(listp)`: the number of nodes of the list at `*listp`.
pub fn mesh_cnt_list(m: &Mem, listp: u32) -> i32 {
    let mut n = 0;
    let mut p = m.u32(listp);
    while p != 0 {
        p = m.u32(p.wrapping_add(0x4c));
        n += 1;
    }
    n
}

/// [soft 0x084cc020 insertIntoKnotList]: `(listp, kp, count, start)`: keeps in the sorted list at `*listp` a run of
/// nodes whose value is `*kp`, `count` of them. The search goes on from the node after `start` (from the head when
/// `start` is 0) while the node's value is below `*kp` by more than 1e-6. At a node within 1e-6 of `*kp` the run of
/// such neighbours (each within 1e-6 of the one before) is counted, `edi` long; the number still to be made is
/// `max(count, edi) - edi`. The cursor ends on the second node from the end of the run (the first node when the
/// run is one long) and the nodes made are linked between it and the node after the run: the run's last node
/// leaves the list. Returns the cursor after the last node made (`start` when none was made).
pub fn insert_into_knot_list(m: &mut Mem, listp: u32, kp: u32, count: i32, start: u32) -> u32 {
    let eps = x(KNOT_EPS);
    let mut count = count;
    let mut prev = start;
    let mut cur = if start == 0 { m.u32(listp) } else { m.u32(start.wrapping_add(0x4c)) };
    if cur != 0 {
        let v = x(m.f64(kp));
        let mut kpc = m.u32(cur);
        let mut ended = false;
        loop {
            // while v > *kp(cur) and |v - *kp(cur)| > eps
            let k = x(m.f64(kpc));
            if !v.gt(k) {
                break;
            }
            if !(v - k).abs().gt(eps) {
                break;
            }
            prev = cur;
            cur = m.u32(prev.wrapping_add(0x4c));
            if cur == 0 {
                ended = true;
                break;
            }
            kpc = m.u32(cur);
        }
        if !ended && cur != 0 {
            let k = x(m.f64(kpc));
            if (k - v).abs().lt(eps) {
                let mut run = 1i32;
                while m.u32(cur.wrapping_add(0x4c)) != 0 {
                    let nx = m.u32(cur.wrapping_add(0x4c));
                    let a = x(m.f64(m.u32(nx)));
                    let b = x(m.f64(m.u32(cur)));
                    if !(a - b).abs().lt(eps) {
                        break;
                    }
                    run += 1;
                    prev = cur;
                    cur = nx;
                }
                cur = m.u32(cur.wrapping_add(0x4c));
                if count < run {
                    count = run;
                }
                count -= run;
            }
        }
    }
    while count > 0 {
        let node = m.si_calloc(1, 0x50);
        m.set_u32(node.wrapping_add(0x4c), 0);
        m.set_u32(node, kp);
        if prev == 0 {
            m.set_u32(node.wrapping_add(0x4c), cur);
            m.set_u32(listp, node);
        } else if cur == 0 {
            m.set_u32(prev.wrapping_add(0x4c), node);
        } else {
            m.set_u32(prev.wrapping_add(0x4c), node);
            m.set_u32(node.wrapping_add(0x4c), cur);
        }
        prev = node;
        count -= 1;
    }
    prev
}

/// [soft 0x084caa20 mergeKnotSequence]: `(a, b, out, order)` on three direction records: the knots of `out` are
/// those of `a` up to `a.knots - order + 1` entries, then the join (for order 2 and 3 the last one or two knots of
/// `a`, followed by `a`'s end plus the first span or two of `b`; for a larger order the knots at the end of `a`
/// repeated as the order needs: `order / 2` or `order / 2 - 1` copies on each side of the joint), then the rest
/// of `out` from `b`'s spans: `out[i] = (b[k] - b[k - 1]) + out[i - 1]`, `k` starting at `order`.
pub fn merge_knot_sequence(m: &mut Mem, a: u32, b: u32, out: u32, order: i32) {
    let ak = |m: &Mem| m.u32(a.wrapping_add(0xc));
    let bk = m.u32(b.wrapping_add(0xc));
    let ok = m.u32(out.wrapping_add(0xc));
    let at = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(8));
    let an = m.i32(a.wrapping_add(8));
    let mut i = 0i32;
    while i < (an + 1) - order {
        let v = m.f64(at(ak(m), i));
        m.set_f64(at(ok, i), v);
        i += 1;
    }
    let akn = ak(m);
    if order == 3 {
        let v = m.f64(at(akn, an - 2));
        m.set_f64(at(ok, i), v);
        let last = m.f64(at(akn, an - 1));
        m.set_f64(at(ok, i + 1), last);
        let v = ((x(m.f64(at(bk, 2))) - x(m.f64(at(bk, 1)))) + x(last)).to_f64();
        i += 2;
        m.set_f64(at(ok, i), v);
        i += 1;
    } else if order == 2 {
        let last = m.f64(at(akn, an - 1));
        m.set_f64(at(ok, i), last);
        let v = ((x(m.f64(at(bk, 1))) - x(m.f64(bk))) + x(last)).to_f64();
        i += 1;
        m.set_f64(at(ok, i), v);
        i += 1;
    } else {
        if order & 1 == 0 {
            i = (an + 1) - order;
            let mut n = order / 2 - 1;
            while n > 0 {
                let v = m.f64(at(akn, i - 1));
                m.set_f64(at(ok, i), v);
                i += 1;
                n -= 1;
            }
            let v = m.f64(at(akn, i));
            m.set_f64(at(ok, i), v);
            let v = m.f64(at(akn, i + 1));
            m.set_f64(at(ok, i + 1), v);
            i += 2;
            n = order / 2 - 1;
            while n > 0 {
                let v = m.f64(at(ok, i - 1));
                m.set_f64(at(ok, i), v);
                i += 1;
                n -= 1;
            }
        } else {
            i = (an + 1) - order;
            let mut n = order / 2;
            while n > 0 {
                let v = m.f64(at(akn, i - 1));
                m.set_f64(at(ok, i), v);
                i += 1;
                n -= 1;
            }
            let v = m.f64(at(akn, i));
            m.set_f64(at(ok, i), v);
            i += 1;
            n = order / 2;
            while n > 0 {
                let v = m.f64(at(ok, i - 1));
                m.set_f64(at(ok, i), v);
                i += 1;
                n -= 1;
            }
        }
    }
    let mut order = order;
    while i < m.i32(out.wrapping_add(8)) {
        let v = (x(m.f64(at(bk, order))) - x(m.f64(at(bk, order - 1)))) + x(m.f64(at(ok, i - 1)));
        m.set_f64(at(ok, i), v.to_f64());
        order += 1;
        i += 1;
    }
}

/// [soft 0x084ddf54 si_nurbsCrvFillSegmentPoints]: `(dir, knots, out)`: `out[0] = 0` and, for `i` from 1 below
/// `points - order + 2`, `out[i] = out[i - 1] +` the sum of `knots[i - 1 .. i - 1 + order - 1]` (summed on the
/// register from 0, added to `out[i - 1]` last, rounded once at the store).
pub fn crv_fill_segment_points(m: &mut Mem, dir: u32, knots: u32, out: u32) {
    let order = m.i32(dir);
    let npts = m.i32(dir.wrapping_add(4));
    let count = npts.wrapping_sub(order).wrapping_add(2);
    m.set_u32(out, 0);
    m.set_u32(out.wrapping_add(4), 0);
    let mut i = 1i32;
    while i < count {
        let prev = m.f64(out.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8));
        let mut sum = F80::ZERO;
        let mut k = order.wrapping_sub(1);
        let mut p = knots.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8);
        // the loop is entered only for a positive count
        if k > 0 {
            while k != 0 {
                sum = sum + x(m.f64(p));
                p = p.wrapping_add(8);
                k -= 1;
            }
        }
        let v = sum + x(prev);
        m.set_f64(out.wrapping_add((i as u32).wrapping_mul(8)), v.to_f64());
        i += 1;
    }
}

/// [soft 0x08503378 SI_nurbsFactorToDistance]: `(crv, factor)`: the length along the curve at parameter
/// `factor`, as loaded into the unit. The curve's cumulated lengths (+0x58) are built first when they are not
/// there (`SI_nurbsCurveUpdateLength(crv, 100, 1)`). With `n` entries (+0x50) and the parameters at +0x54: the
/// last cumulated length not above 0 answers 0; `factor` not above the first parameter answers 0; `factor`
/// not below the last parameter answers the last cumulated length; else the first parameter above `factor` is
/// found (the search starts at index 1; none found leaves it 0), `t = (factor - p[k-1]) / (p[k] - p[k-1])` is
/// **rounded to a float**, and the answer is `l[k-1] + t * (l[k] - l[k-1])` on the register.
pub fn nurbs_factor_to_distance(m: &mut Mem, o: &mut dyn Outside, crv: u32, factor: f64) -> Result<F80, Halt> {
    if m.u32(crv.wrapping_add(0x58)) == 0 {
        crate::image::nurbs_curve_update_length(m, o, crv, 100, 1)?;
    }
    let n = m.i32(crv.wrapping_add(0x50));
    let lens = m.u32(crv.wrapping_add(0x58));
    let params = m.u32(crv.wrapping_add(0x54));
    let l_at = |m: &Mem, i: i32| m.f64(lens.wrapping_add((i as u32).wrapping_mul(8)));
    let p_at = |m: &Mem, i: i32| m.f64(params.wrapping_add((i as u32).wrapping_mul(8)));
    let f = x(factor);
    let last = x(l_at(m, n.wrapping_sub(1)));
    if !last.gt(F80::ZERO) && !last.unordered(F80::ZERO) {
        return Ok(F80::ZERO);
    }
    let p0 = x(p_at(m, 0));
    if !p0.lt(f) && !p0.unordered(f) {
        return Ok(F80::ZERO);
    }
    if !f.lt(x(p_at(m, n.wrapping_sub(1)))) && !f.unordered(x(p_at(m, n.wrapping_sub(1)))) {
        return Ok(last);
    }
    let mut k = 0i32;
    if 0 < n {
        // factor below the first parameter (cannot be, after the test above) keeps k at 0
        if !f.lt(p0) {
            let mut j = 0i32;
            loop {
                j += 1;
                if j >= n {
                    break;
                }
                if f.lt(x(p_at(m, j))) {
                    k = j;
                    break;
                }
            }
        }
    }
    let lo = x(p_at(m, k.wrapping_sub(1)));
    let diff = f - lo;
    let span = x(p_at(m, k)) - lo;
    let t = F80::from_f32((diff / span).to_f32());
    let l0 = x(l_at(m, k.wrapping_sub(1)));
    let l1 = x(l_at(m, k));
    let d = l1 - l0;
    Ok(t * d + l0)
}

// ---- making curves and surfaces share one knot vector

use si_mem::Routine;

pub const SI_NURBS_CRV_KNOT_INSERT: Routine = Routine::new("SI_nurbsCrvKnotInsert", 0x084d_4130);
pub const SI_NURBS_SRF_KNOT_INSERT: Routine = Routine::new("SI_nurbsSrfKnotInsert", 0x084d_96bc);
pub const SI_NURBS_SRF_SWAP_UV: Routine = Routine::new("SI_nurbsSrfSwapUV", 0x084d_c5f4);

/// The knots of a direction record read as the original does: `knots[i]` as loaded.
fn knot(m: &Mem, base: u32, i: i32) -> f64 {
    m.f64(base.wrapping_add((i as u32).wrapping_mul(8)))
}

/// The common walk of `insertMissingKnots` and `insertMissingKnotsSrf`: `target` is a direction record
/// (`order, points, knot count, knots` at +0 ..), `rec` the record the insertion call takes, `dirarg` its last
/// argument. `i` walks the wanted knots, `j` the record's own (the record's knots are read again at the top of
/// every step: the insertion changes them). For each wanted knot: the record's knots are walked forward while they
/// are below it by more than 1e-6; if the one reached is within 1e-6, the missing multiplicity (the larger of the
/// two multiplicities, `checkMult`, less the record's) is inserted at the last of the wanted run; else the whole
/// run's multiplicity is inserted.
/// True when the knot `j` of the array at `own` lies in the unused part of the heap (above the last block handed
/// out): memory nobody has written, which the original reads as whatever it holds.
fn beyond_heap_top(m: &Mem, own: u32, j: i32) -> bool {
    let at = own.wrapping_add((j as u32).wrapping_mul(8));
    let top = m.map().heap.0.wrapping_add(m.heap_stats().used);
    own >= m.map().heap.0 && at >= top && at < m.map().heap.0.wrapping_add(m.map().heap.1)
}

fn insert_missing(m: &mut Mem, o: &mut dyn Outside, wanted: u32, n: i32, dir: u32, rec: u32, routine: Routine, dirarg: u32) {
    let eps = x(KNOT_EPS);
    let mut j = 0i32;
    let mut i = 0i32;
    if n <= 0 {
        return;
    }
    let call = |m: &mut Mem, o: &mut dyn Outside, t: f64, r: i32| {
        let b = t.to_bits();
        o.call(m, routine, &[rec, b as u32, (b >> 32) as u32, r as u32, dirarg]);
    };
    loop {
        let own = m.u32(dir.wrapping_add(0xc));
        let v = x(knot(m, wanted, i));
        loop {
            if beyond_heap_top(m, own, j) {
                // The walk has left the record's knots (a wanted knot above all of them: with dir 0 of
                // `SI_nurbsSrfMakeCompatible` the parameters come from the other direction) and the heap's used
                // part: the original reads the words of unwritten memory, which are not below `v` for any
                // fill that ends the walk at all (finding 0025 section 23). Stop there.
                break;
            }
            let k = x(knot(m, own, j));
            if k.lt(v) && (v - k).abs().gt(eps) {
                j = j.wrapping_add(1);
            } else {
                break;
            }
        }
        let d = (v - x(knot(m, own, j))).abs();
        if !beyond_heap_top(m, own, j) && d.lt(eps) {
            let a = check_mult(m, wanted, i, n);
            i = i.wrapping_sub(1).wrapping_add(a);
            let own2 = m.u32(dir.wrapping_add(0xc));
            let nk = m.i32(dir.wrapping_add(8));
            let bm = check_mult(m, own2, j, nk);
            let target = if bm < a { a } else { bm };
            if target - bm > 0 {
                call(m, o, knot(m, wanted, i), target - bm);
            }
        } else {
            let a = check_mult(m, wanted, i, n);
            i = i.wrapping_sub(1).wrapping_add(a);
            call(m, o, knot(m, wanted, i), a);
        }
        i = i.wrapping_add(1);
        if i >= n {
            break;
        }
    }
}

/// [soft 0x084cc664 insertMissingKnots]: `(knots, n, crv)`: gives the curve every knot of the sorted array `knots`
/// (to 1e-6) with at least its multiplicity there, by `SI_nurbsCrvKnotInsert(crv, t, count, 1)` through `o`
/// (see [`insert_missing`]).
pub fn insert_missing_knots(m: &mut Mem, o: &mut dyn Outside, knots: u32, n: i32, crv: u32) {
    insert_missing(m, o, knots, n, crv, crv, SI_NURBS_CRV_KNOT_INSERT, 1);
}

/// [soft 0x084cc790 insertMissingKnotsSrf]: `(knots, n, srf, dir)`: the same on the surface's direction `dir`
/// (the record at +0x24 for `dir` not 0, else the one at +0), by `SI_nurbsSrfKnotInsert(srf, t, count, dir)`.
pub fn insert_missing_knots_srf(m: &mut Mem, o: &mut dyn Outside, knots: u32, n: i32, srf: u32, dir: i32) {
    let d = if dir == 0 { srf } else { srf.wrapping_add(0x24) };
    insert_missing(m, o, knots, n, d, srf, SI_NURBS_SRF_KNOT_INSERT, dir as u32);
}

/// [soft 0x084cc158 nurbsCrvMakeCompatibleKnots]: `(a, na, b, nb, countp)`: both knot arrays are scaled in place to
/// the common length `mid = (a[na-1] + b[nb-1]) * 0.5` (held unrounded in the register: `a[i] = mid * (a[i] /
/// last)`, `last` the array's own last knot as it was), a list of the scaled knots of `a` (`SI_meshAddList` from
/// the last knot to the first, so the list runs in order, holding the knots' addresses) takes the runs of `b`
/// (within 1e-6 of each other) by [`insert_into_knot_list`], each continuing from the cursor the last run left.
/// The result is a new array (`si_knotsAllocate`) of the list's knots in order, its length stored at `*countp`;
/// the list is freed.
pub fn nurbs_crv_make_compatible_knots(m: &mut Mem, a: u32, na: i32, b: u32, nb: i32, countp: u32) -> u32 {
    let sp = m.sp();
    let headp = m.local(4);
    let curp = m.local(4);
    m.set_u32(headp, 0);
    m.set_u32(curp, 0);
    let last_a = knot(m, a, na.wrapping_sub(1));
    let mid = (x(last_a) + x(knot(m, b, nb.wrapping_sub(1)))) * x(0.5);
    let mut i = na.wrapping_sub(1);
    while i >= 0 {
        let at = a.wrapping_add((i as u32).wrapping_mul(8));
        let v = mid * (x(m.f64(at)) / x(last_a));
        m.set_f64(at, v.to_f64());
        mesh_add_list(m, headp, at);
        i -= 1;
    }
    let last_b = x(knot(m, b, nb.wrapping_sub(1)));
    let mut i = nb.wrapping_sub(1);
    while i >= 0 {
        let at = b.wrapping_add((i as u32).wrapping_mul(8));
        let v = mid * (x(m.f64(at)) / last_b);
        m.set_f64(at, v.to_f64());
        i -= 1;
    }
    let eps = x(KNOT_EPS);
    let mut i = 0i32;
    if 0 < nb {
        loop {
            let mut run = 1i32;
            let mut next;
            loop {
                next = i.wrapping_add(1);
                if next < nb && (x(knot(m, b, next)) - x(knot(m, b, i))).abs().lt(eps) {
                    run += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            let cur = m.u32(curp);
            let r = insert_into_knot_list(m, headp, b.wrapping_add((i as u32).wrapping_mul(8)), run, cur);
            m.set_u32(curp, r);
            i = next;
            if i >= nb {
                break;
            }
        }
    }
    let cnt = mesh_cnt_list(m, headp);
    let out = knots_allocate(m, cnt);
    let mut k = 0u32;
    let mut node = m.u32(headp);
    while node != 0 {
        let v = m.f64(m.u32(node));
        m.set_f64(out.wrapping_add(8 * k), v);
        node = m.u32(node.wrapping_add(0x4c));
        k += 1;
    }
    m.set_i32(countp, k as i32);
    mesh_free_list(m, headp);
    m.set_sp(sp);
    out
}

/// The copy of a direction's knots every caller of [`nurbs_crv_make_compatible_knots`] makes: `si_knotsAllocate`
/// and a copy.
fn copy_knots(m: &mut Mem, knots: u32, n: i32) -> u32 {
    let c = knots_allocate(m, n);
    for i in 0..n.max(0) {
        let v = knot(m, knots, i);
        m.set_f64(c.wrapping_add((i as u32).wrapping_mul(8)), v);
    }
    c
}

/// The common part of the two `MakeCompatible` routines: `dir_off` is the offset of the direction record in the
/// record (0 for a curve, 0x24 for the surface's second direction). Returns the merged knots and their count, or
/// `None` for fewer than two records.
fn merged_knots(m: &mut Mem, recs: u32, n: i32, dir_off: u32) -> Option<(u32, i32)> {
    if n <= 1 {
        return None;
    }
    let sp = m.sp();
    let cntp = m.local(4);
    let rec = |m: &Mem, i: i32| m.u32(recs.wrapping_add((i as u32).wrapping_mul(4))).wrapping_add(dir_off);
    let d0 = rec(m, 0);
    let mut buf = copy_knots(m, m.u32(d0.wrapping_add(0xc)), m.i32(d0.wrapping_add(8)));
    let mut cnt = m.i32(d0.wrapping_add(8));
    let mut merged = 0u32;
    let mut i = 1;
    while i < n {
        let di = rec(m, i);
        let nk = m.i32(di.wrapping_add(8));
        let other = copy_knots(m, m.u32(di.wrapping_add(0xc)), nk);
        merged = nurbs_crv_make_compatible_knots(m, buf, cnt, other, nk, cntp);
        m.free(buf);
        m.free(other);
        cnt = m.i32(cntp);
        buf = merged;
        i += 1;
    }
    m.set_sp(sp);
    Some((merged, cnt))
}

/// Scales one direction's knots and range to the common length `len` (`len * (v / last)`, the last knot as it is
/// when read: it changes at the last step only).
fn rescale_dir(m: &mut Mem, d: u32, len: f64) {
    let last = |m: &Mem| m.f64(m.u32(d.wrapping_add(0xc)).wrapping_add((m.i32(d.wrapping_add(8)) as u32).wrapping_mul(8)).wrapping_sub(8));
    for off in [0x10u32, 0x18] {
        let v = x(len) * (x(m.f64(d.wrapping_add(off))) / x(last(m)));
        m.set_f64(d.wrapping_add(off), v.to_f64());
    }
    let mut j = 0i32;
    while j < m.i32(d.wrapping_add(8)) {
        let at = m.u32(d.wrapping_add(0xc)).wrapping_add((j as u32).wrapping_mul(8));
        let v = x(len) * (x(m.f64(at)) / x(last(m)));
        m.set_f64(at, v.to_f64());
        j += 1;
    }
}

/// [soft 0x084cc29c SI_nurbsCrvMakeCompatible]: `(crvs, n)`: for `n` of at least 2, merges every curve's knots
/// (scaled to a common length, [`nurbs_crv_make_compatible_knots`]) and gives each curve with `order <= points`
/// the merged knots by [`insert_missing_knots`] after scaling its own knots and range to that length. The merged
/// array is not freed (the original keeps it).
pub fn nurbs_crv_make_compatible(m: &mut Mem, o: &mut dyn Outside, crvs: u32, n: i32) {
    let Some((merged, cnt)) = merged_knots(m, crvs, n, 0) else { return };
    let len = knot(m, merged, cnt.wrapping_sub(1));
    for i in 0..n {
        let c = m.u32(crvs.wrapping_add(4 * i as u32));
        if m.i32(c) <= m.i32(c.wrapping_add(4)) {
            rescale_dir(m, c, len);
            insert_missing_knots(m, o, merged, cnt, c);
        }
    }
}

/// [soft 0x084cc458 SI_nurbsSrfMakeCompatible]: `(srfs, n, dir)`: the same on surfaces' second direction (the
/// record at +0x24; tested `order <= points` at +0x24 / +0x28), after `SI_nurbsSrfSwapUV` of every surface when
/// `dir` is 0 and again at the end, with [`insert_missing_knots_srf`]`(…, dir)`.
pub fn nurbs_srf_make_compatible(m: &mut Mem, o: &mut dyn Outside, srfs: u32, n: i32, dir: i32) {
    if n <= 1 {
        return;
    }
    let each = |m: &mut Mem, o: &mut dyn Outside| {
        for i in 0..n {
            let s = m.u32(srfs.wrapping_add(4 * i as u32));
            o.call(m, SI_NURBS_SRF_SWAP_UV, &[s]);
        }
    };
    if dir == 0 {
        each(m, o);
    }
    if let Some((merged, cnt)) = merged_knots(m, srfs, n, 0x24) {
        let len = knot(m, merged, cnt.wrapping_sub(1));
        for i in 0..n {
            let s = m.u32(srfs.wrapping_add(4 * i as u32));
            let d = s.wrapping_add(0x24);
            if m.i32(d) <= m.i32(d.wrapping_add(4)) {
                rescale_dir(m, d, len);
                insert_missing_knots_srf(m, o, merged, cnt, s, dir);
            }
        }
    }
    if dir == 0 {
        each(m, o);
    }
}
