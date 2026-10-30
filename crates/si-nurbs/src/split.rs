// SPDX-License-Identifier: MIT
//! `nrb_draw.c` and `eval.c` routines around knot insertion and counting (finding 0025 section 27): `SI_nurbsSrfInsertKnot`
//! (the earlier insertion through the OLD knot-sequence changer) and `SI_nurbsSrfNoTriangles` (the triangle count of a surface).

use crate::bridge::srf_swap_uv;
use crate::image::{srf_copy, srf_dispose, SrfHeld, crv_copy, keep, statics, sample_knot_vector, CrvHeld, Halt};
use crate::knots::interv;
use crate::bridge::crv_knot_insert;
use si_hrc::image::nurbs::{nurbs_srf_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};
use si_x87::F80;
use crate::trimmed::no_trimmed_triangles;
use si_hrc::image::nurbs::knots_allocate;
use si_mem::{Mem, Outside};

/// [soft 0x084d84a4 SI_nurbsSrfInsertKnot]: `(srf, t, dir)`: with `dir == 1` the surface's directions are exchanged first
/// ([soft 0x084dc5f4 SI_nurbsSrfSwapUV]); the knot vector of the first direction (count at +8, knots at +0xc) is copied with `t`
/// inserted in order into a new block of one more entry ([soft 0x084d80f8 getKnotPosition], whose answer is the result), the surface
/// is re-expressed on it by si_OLDnurbsSrfChangeKnotsSeq (0x084d822c), the block is freed and with `dir == 0` the
/// directions are exchanged afterwards (any other `dir` exchanges never). Answers the insertion index.
/// Since unit route3 this is [`crate::ins2::srf_insert_knot_img`].
pub fn srf_insert_knot(m: &mut Mem, s: u32, t: f64, dir: i32) -> Result<i32, Halt> {
    // routed (unit route3) to the image form (the span search reads the neighbouring heap as the original does)
    crate::ins2::srf_insert_knot_img(m, s, t, dir)
}

/// [soft 0x084e550c SI_nurbsSrfNoTriangles]: `(srf)`: a trimmed surface (`srf + 0x60` trim curves, more than 0) answers
/// [soft 0x08501700 SI_noTrimmedTriangles]; otherwise both directions are sampled ([soft 0x084d727c SI_SampleKnotVector] with the
/// `per` words at +0x70 and +0x74), the two arrays freed (the second first) and the answer is `(nu - 1) * 2 * (nv - 1)` with `nu`
/// and `nv` the sample counts.
pub fn srf_no_triangles(m: &mut Mem, s: u32) -> Result<u32, Halt> {
    if m.i32(s + 0x60) > 0 {
        return no_trimmed_triangles(m, s);
    }
    let sp0 = m.sp();
    let loc = m.local(16); // array u, count u, array v, count v ([ebp-8], [ebp-4], [ebp-0x10], [ebp-0xc])
    let (au, cu, av, cv) = (loc, loc + 4, loc + 8, loc + 12);
    let (pu, pv) = (m.i32(s + 0x70), m.i32(s + 0x74));
    sample_knot_vector(m, s, pu, au, cu)?;
    sample_knot_vector(m, s + 0x24, pv, av, cv)?;
    for a in [av, au] {
        let p = m.u32(a);
        if p != 0 {
            m.free(p);
            m.set_u32(a, 0);
        }
    }
    let r = m.i32(cu).wrapping_sub(1).wrapping_mul(m.i32(cv).wrapping_sub(1).wrapping_add(m.i32(cv).wrapping_sub(1)));
    m.set_sp(sp0);
    Ok(r as u32)
}

fn copy36(m: &mut Mem, dst: u32, src: u32) {
    for w in 0..9u32 {
        let v = m.u32(src.wrapping_add(4 * w));
        m.set_u32(dst.wrapping_add(4 * w), v);
    }
}

fn pt(base: u32, idx: i32) -> u32 {
    base.wrapping_add(0x24u32.wrapping_mul(idx as u32))
}

fn kf(m: &Mem, base: u32, i: i32) -> F80 {
    F80::from_f64(m.f64(base.wrapping_add((i as u32).wrapping_mul(8))))
}

fn set_kf(m: &mut Mem, base: u32, i: i32, v: F80) {
    m.set_f64(base.wrapping_add((i as u32).wrapping_mul(8)), v.to_f64());
}

/// The fields every new piece of [`crv_split`] gets (order, closed flag 0, parameterisation 2, the +0x28 flag and the step).
fn split_piece(m: &mut Mem, o: u32, cp: u32) {
    let c = m.u32(o);
    m.set_i32(c, m.i32(cp));
    m.set_i16(c + 0x20, 0);
    m.set_i32(c + 0x24, 2);
    if m.i16(cp + 0x28) != 0 {
        m.set_i16(c + 0x28, 1);
    }
    m.set_i32(c + 0x34, m.i32(cp + 0x34));
}

/// A new knot vector of a piece (count, `si_knotsAllocate`), its range (`k[order - 2]`, `k[count - order + 1]`).
fn piece_range(m: &mut Mem, c: u32) {
    let (o, nk, k) = (m.i32(c), m.i32(c + 8), m.u32(c + 0xc));
    let (lo, hi) = (m.f64(k.wrapping_add((o.wrapping_sub(2) as u32).wrapping_mul(8))), m.f64(k.wrapping_add((nk.wrapping_sub(o).wrapping_add(1) as u32).wrapping_mul(8))));
    m.set_f64(c + 0x10, lo);
    m.set_f64(c + 0x18, hi);
}

/// [soft 0x084d8e08 SI_nurbsCrvSplit]: `(crv, t, &first, &second)`. A closed curve whose end is within 1e-6 of `t` is split at its start.
/// A copy of the curve gets the knot `t` inserted `order - 1` times ([soft 0x084d4130 SI_nurbsCrvKnotInsert]) and the span of `t`
/// is found ([soft 0x084d76b4 si_nurbsInterv], open search for an open curve). An open curve is cut into two open curves sharing
/// the point at the cut (the first with `span - order + 3` points and the knots from the start, the second with the rest and the
/// knots from `span - order + 2`); a closed one becomes one open curve (`*second` = 0) that starts at the cut: its knot vector
/// begins with `order - 1` zeros and grows by the old knot differences from the span on, wrapping around, and its points are the
/// old ones rotated by the span with the first one repeated at the end. The copy is disposed.
pub fn crv_split(m: &mut Mem, c: u32, t: f64, out1: u32, out2: u32) -> Result<(), Halt> {
    if c == 0 {
        return Err(Halt::Refused("SI_nurbsCrvSplit"));
    }
    let mut t = t;
    if m.i16(c + 0x20) != 0 && (F80::from_f64(t) - F80::from_f64(m.f64(c + 0x18))).abs().lt(F80::from_f64(crate::EPS)) {
        t = m.f64(c + 0x10);
    }
    let sp0 = m.sp();
    let cell = m.local(4);
    let cp = crv_copy(m, c);
    m.set_u32(cell, cp);
    if cp == 0 {
        return Err(Halt::Refused("SI_nurbsCrvCopy"));
    }
    let o = m.i32(cp);
    crv_knot_insert(m, cp, t, o.wrapping_sub(1), 1);
    let span = {
        let mut stt = statics(m);
        let h = CrvHeld::at(m, cp);
        let v = h.view();
        let r = interv(&mut stt, v.dir(), t, (m.i16(cp + 0x20) == 0) as i16);
        keep(m, stt);
        r.ok_or(Halt::Refused("si_nurbsInterv"))?
    };
    let (cnp, cnk, ck, cpts) = (m.i32(cp + 4), m.i32(cp + 8), m.u32(cp + 0xc), m.u32(cp + 0x2c));
    if !(0..=1 << 20).contains(&cnp) || !(0..=1 << 20).contains(&cnk) || o < 2 || o > 64 || span < o - 2 || span - o + 3 > cnp {
        return Err(Halt::Refused("SI_nurbsCrvSplit"));
    }
    let a = nurbs_crv_allocate(m).addr();
    m.set_u32(out1, a);
    split_piece(m, out1, cp);
    if m.i16(cp + 0x20) == 0 {
        let c1 = m.u32(out1);
        let np1 = span - o + 3;
        m.set_i32(c1 + 4, np1);
        let nk1 = o + np1 - 2;
        m.set_i32(c1 + 8, nk1);
        let k1 = knots_allocate(m, nk1);
        m.set_u32(c1 + 0xc, k1);
        for i in 0..nk1 {
            let v = m.f64(ck.wrapping_add((i as u32).wrapping_mul(8)));
            m.set_f64(k1.wrapping_add((i as u32).wrapping_mul(8)), v);
        }
        piece_range(m, c1);
        let p1 = nurbs_points_allocate(m, np1);
        m.set_u32(c1 + 0x2c, p1);
        for i in 0..np1 as u32 {
            copy36(m, p1 + 0x24 * i, cpts + 0x24 * i);
        }
        let b = nurbs_crv_allocate(m).addr();
        m.set_u32(out2, b);
        split_piece(m, out2, cp);
        let c2 = m.u32(out2);
        let np2 = cnp - (np1 - 1);
        m.set_i32(c2 + 4, np2);
        let nk2 = o + np2 - 2;
        m.set_i32(c2 + 8, nk2);
        let k2 = knots_allocate(m, nk2);
        m.set_u32(c2 + 0xc, k2);
        for i in 0..nk2 {
            let v = m.f64(ck.wrapping_add(((i + span - o + 2) as u32).wrapping_mul(8)));
            m.set_f64(k2.wrapping_add((i as u32).wrapping_mul(8)), v);
        }
        piece_range(m, c2);
        let p2 = nurbs_points_allocate(m, np2);
        m.set_u32(c2 + 0x2c, p2);
        for i in 0..np2.max(0) as u32 {
            copy36(m, p2 + 0x24 * i, cpts.wrapping_add(0x24u32.wrapping_mul((span - o + 2) as u32 + i)));
        }
    } else {
        m.set_u32(out2, 0);
        let c1 = m.u32(out1);
        let sp2 = span - o + 2;
        let np1 = cnp + 1;
        m.set_i32(c1 + 4, np1);
        let nk1 = o + np1 - 2;
        m.set_i32(c1 + 8, nk1);
        let k1 = knots_allocate(m, nk1);
        m.set_u32(c1 + 0xc, k1);
        for i in 0..o - 1 {
            m.set_f64(k1.wrapping_add((i as u32).wrapping_mul(8)), 0.0);
        }
        for i in 0..cnk - span - 1 {
            let v = (kf(m, k1, i + o - 2) + kf(m, ck, span + 1 + i)) - kf(m, ck, span + i);
            set_kf(m, k1, i + o - 1, v);
        }
        for i in 0..sp2 {
            let v = (kf(m, k1, i + o + cnk - span - 3) + kf(m, ck, i + 1)) - kf(m, ck, i);
            set_kf(m, k1, i + o + cnk - span - 2, v);
        }
        for j in 0..o - 2 {
            let v = kf(m, k1, j + cnk - 1);
            set_kf(m, k1, j + cnk, v);
        }
        piece_range(m, c1);
        let p1 = nurbs_points_allocate(m, np1);
        m.set_u32(c1 + 0x2c, p1);
        for i in 0..cnp - sp2 {
            copy36(m, p1 + 0x24 * i as u32, cpts.wrapping_add(0x24u32.wrapping_mul((sp2 + i) as u32)));
        }
        for j in 0..=sp2 {
            copy36(m, p1.wrapping_add(0x24u32.wrapping_mul((cnp - sp2 + j) as u32)), cpts + 0x24 * j as u32);
        }
    }
    nurbs_crv_dispose(m, cell);
    m.set_sp(sp0);
    Ok(())
}

/// The second direction's part of a new piece of [`srf_split`]: its counts, knots, range, flag word, parameterisation and the words at +0x70..+0x7c.
fn srf_v_part(m: &mut Mem, a: u32, cp: u32) {
    for o in [0x24u32, 0x28, 0x2c] {
        m.set_i32(a + o, m.i32(cp + o));
    }
    let nk = m.i32(a + 0x2c);
    let k = knots_allocate(m, nk);
    m.set_u32(a + 0x30, k);
    for i in 0..m.i32(a + 0x2c) {
        let v = m.f64(m.u32(cp + 0x30).wrapping_add((i as u32).wrapping_mul(8)));
        m.set_f64(k.wrapping_add((i as u32).wrapping_mul(8)), v);
    }
    for o in [0x34u32, 0x3c] {
        m.set_f64(a + o, m.f64(cp + o));
    }
    m.set_u16(a + 0x44, m.u16(cp + 0x44));
    m.set_i32(a + 0x4c, 2);
    for o in [0x70u32, 0x74, 0x78, 0x7c] {
        m.set_i32(a + o, m.i32(cp + o));
    }
    if m.i16(cp + 0x50) != 0 {
        m.set_i16(a + 0x50, 1);
    }
}

/// [soft 0x084d8530 SI_nurbsSrfSplit]: `(srf, t, dir, &first, &second)`. A closed direction (the first, or with `dir != 0` the
/// second) whose end is within 1e-6 of `t` is split at its start. A copy of the surface (its directions exchanged first with `dir == 1`)
/// gets `t` inserted `order - 1` times in the first direction ([soft 0x084d96bc SI_nurbsSrfKnotInsert]) and the span is found
/// ([soft 0x084d76b4 si_nurbsInterv], open search for an open direction). The first direction is cut like [`crv_split`] cuts a
/// curve (two surfaces sharing the row at the cut, or for a closed direction one surface and `*second = 0` whose knots and
/// rows are rotated), the second direction is carried over to each piece with its knots and range. Both pieces are exchanged
/// back with `dir == 1`; the copy is disposed.
pub fn srf_split(m: &mut Mem, o: &mut dyn Outside, s: u32, t: f64, dir: i32, out1: u32, out2: u32) -> Result<(), Halt> {
    let dsel = if dir != 0 { s + 0x24 } else { s };
    let mut t = t;
    if m.i16(dsel + 0x20) != 0 && (F80::from_f64(t) - F80::from_f64(m.f64(dsel + 0x18))).abs().lt(F80::from_f64(crate::EPS)) {
        t = m.f64(dsel + 0x10);
    }
    let sp0 = m.sp();
    let cell = m.local(4);
    let cp = srf_copy(m, o, s);
    m.set_u32(cell, cp);
    if cp == 0 {
        return Err(Halt::Refused("SI_nurbsSrfCopy"));
    }
    if dir == 1 {
        srf_swap_uv(m, cp);
    }
    let ord = m.i32(cp);
    crate::bridge::srf_knot_insert(m, o, cp, t, ord.wrapping_sub(1), 0);
    let span = {
        let mut stt = statics(m);
        let h = SrfHeld::at(m, cp);
        let v = h.view();
        let r = interv(&mut stt, v.u(), t, (m.i16(cp + 0x20) == 0) as i16);
        keep(m, stt);
        r.ok_or(Halt::Refused("si_nurbsInterv"))?
    };
    let (cnp, cnk, ck, cpts, npv) = (m.i32(cp + 4), m.i32(cp + 8), m.u32(cp + 0xc), m.u32(cp + 0x54), m.i32(cp + 0x28));
    if !(0..=1 << 16).contains(&cnp) || !(0..=1 << 16).contains(&cnk) || !(0..=1 << 16).contains(&npv) || ord < 2 || ord > 64 || span < ord - 2 || span - ord + 3 > cnp || (cnp as i64) * (npv as i64) > 1 << 22 {
        return Err(Halt::Refused("SI_nurbsSrfSplit"));
    }
    let a = nurbs_srf_allocate(m).addr();
    m.set_u32(out1, a);
    m.set_i32(a, ord);
    m.set_i16(a + 0x20, 0);
    srf_v_part(m, a, cp);
    if m.i16(cp + 0x20) == 0 {
        let np1 = span - ord + 3;
        m.set_i32(a + 4, np1);
        let nk1 = ord + np1 - 2;
        m.set_i32(a + 8, nk1);
        let k1 = knots_allocate(m, nk1);
        m.set_u32(a + 0xc, k1);
        for i in 0..nk1 {
            let v = m.f64(ck.wrapping_add((i as u32).wrapping_mul(8)));
            m.set_f64(k1.wrapping_add((i as u32).wrapping_mul(8)), v);
        }
        piece_range(m, a);
        let tot = np1.wrapping_mul(m.i32(a + 0x28));
        m.set_i32(a + 0x48, tot);
        let p1 = nurbs_points_allocate(m, tot);
        m.set_u32(a + 0x54, p1);
        for j in 0..npv {
            for i in 0..np1 {
                copy36(m, pt(p1, j * np1 + i), pt(cpts, j * cnp + i));
            }
        }
        let b = nurbs_srf_allocate(m).addr();
        m.set_u32(out2, b);
        m.set_i32(b, ord);
        let np2 = cnp - (np1 - 1);
        m.set_i32(b + 4, np2);
        let nk2 = ord + np2 - 2;
        m.set_i32(b + 8, nk2);
        let k2 = knots_allocate(m, nk2);
        m.set_u32(b + 0xc, k2);
        for i in 0..nk2 {
            let v = m.f64(ck.wrapping_add(((span + i - ord + 2) as u32).wrapping_mul(8)));
            m.set_f64(k2.wrapping_add((i as u32).wrapping_mul(8)), v);
        }
        piece_range(m, b);
        m.set_i16(b + 0x20, 0);
        srf_v_part(m, b, cp);
        let tot2 = np2.wrapping_mul(m.i32(b + 0x28));
        m.set_i32(b + 0x48, tot2);
        let p2 = nurbs_points_allocate(m, tot2);
        m.set_u32(b + 0x54, p2);
        for j in 0..npv {
            for i in 0..np2 {
                copy36(m, pt(p2, j * np2 + i), pt(cpts, j * cnp + (span - ord + 2) + i));
            }
        }
    } else {
        m.set_u32(out2, 0);
        m.set_i32(a + 0x4c, 2);
        let sp2 = span - ord + 2;
        let np1 = cnp + 1;
        m.set_i32(a + 4, np1);
        let nk1 = ord + cnk - 2;
        m.set_i32(a + 8, nk1);
        let k1 = knots_allocate(m, nk1);
        m.set_u32(a + 0xc, k1);
        for i in 0..ord - 1 {
            m.set_f64(k1.wrapping_add((i as u32).wrapping_mul(8)), 0.0);
        }
        for i in 0..cnk - span - 1 {
            let v = (kf(m, ck, span + i + 1) - kf(m, ck, span + i)) + kf(m, k1, i + ord - 2);
            set_kf(m, k1, i + ord - 1, v);
        }
        for i in 0..sp2 {
            let v = (kf(m, ck, i + 1) - kf(m, ck, i)) + kf(m, k1, i + ord + cnk - span - 3);
            set_kf(m, k1, i + ord + cnk - span - 2, v);
        }
        for j in 0..ord - 2 {
            let v = kf(m, k1, j + cnk - 1);
            set_kf(m, k1, j + cnk, v);
        }
        piece_range(m, a);
        let tot = np1.wrapping_mul(m.i32(a + 0x28));
        m.set_i32(a + 0x48, tot);
        let p1 = nurbs_points_allocate(m, tot);
        m.set_u32(a + 0x54, p1);
        // the stray loop: row `j0` (the leftover counter of the knot loops) of the old points copied into the same row of the new
        // ones before the real copies; where `j0` is not below the row count it writes past the block, as the original does
        let j0 = (ord - 2).max(0);
        for i in 0..np1 {
            let (d, sr) = (pt(p1, j0 * np1 + i), pt(cpts, j0 * cnp + i));
            if !m.is_mapped(d, 0x24) || !m.is_mapped(sr, 0x24) {
                return Err(Halt::Refused("SI_nurbsSrfSplit"));
            }
            copy36(m, d, sr);
        }
        for j in 0..npv {
            for i in 0..cnp - sp2 {
                copy36(m, pt(p1, j * np1 + i), pt(cpts, j * cnp + sp2 + i));
            }
        }
        for j in 0..npv {
            for i in 0..=sp2 {
                copy36(m, pt(p1, j * np1 + (cnp - sp2 + i)), pt(cpts, j * cnp + i));
            }
        }
    }
    if dir == 1 {
        let a = m.u32(out1);
        srf_swap_uv(m, a);
    }
    let b = m.u32(out2);
    if b != 0 && dir == 1 {
        srf_swap_uv(m, b);
    }
    srf_dispose(m, o, cell);
    m.set_sp(sp0);
    Ok(())
}
