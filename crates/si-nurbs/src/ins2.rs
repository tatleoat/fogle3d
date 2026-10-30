// SPDX-License-Identifier: MIT
//! nurbs15 (finding 0025 section 33): the earlier knot-sequence changer and the knot insertion on the memory image.
//! [`crate::chgknots::old_srf_change_knots_seq`] and [`crate::split::srf_insert_knot`] refuse where the earlier span search
//! (`si_OLDnurbsInterv`) walks off the knot array; here the search reads the image the way the original reads the heap.

use crate::bridge::srf_swap_uv;
use crate::chgknots::srf_subdivc;
use crate::image::Halt;
use crate::sample::knot_position;
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::Mem;
use si_x87::F80;

const PT: u32 = 0x24;

thread_local! {
    static OUTSIDE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How many times the span search read a knot outside the array since the last call (the original then reads the word before
/// or after the block, which is a heap header or uninitialised memory; a check cannot hold the port to it).
pub fn take_outside_reads() -> u32 {
    OUTSIDE.with(|c| c.replace(0))
}

/// [soft 0x084d81a0 si_OLDnurbsInterv] on a direction record at `dir` (order +0, count +8, knots +0xc): the number of knots not
/// above `t`; 0 for `t` outside the knots; else the knots are scanned down from the last to index `count - order - 1` for the
/// first one strictly below `t` (or not comparable) and that index plus one is answered; with none, the vector is scanned up from
/// the start while `t >= knot` and that index is answered. The upward scan can leave the array when `t` equals a repeated last
/// knot: the words are read from the image (the neighbouring heap, as in the original); only a read outside mapped memory
/// is refused (the original would fault there).
pub fn old_interv_img(m: &Mem, dir: u32, t: f64) -> Result<i32, Halt> {
    let (order, n, ks) = (m.i32(dir), m.i32(dir + 8), m.u32(dir + 0xc));
    let at = |i: i32| -> Result<F80, Halt> {
        if i < 0 || i >= n {
            OUTSIDE.with(|c| c.set(c.get() + 1));
        }
        let a = ks.wrapping_add((i as u32).wrapping_mul(8));
        if !m.is_mapped(a, 8) {
            return Err(Halt::Refused("si_OLDnurbsInterv"));
        }
        Ok(F80::from_f64(m.f64(a)))
    };
    let t = F80::from_f64(t);
    if t.lt(at(0)?) || t.gt(at(n.wrapping_sub(1))?) {
        return Ok(0);
    }
    let stop = n.wrapping_sub(order).wrapping_sub(1);
    let mut i = n.wrapping_sub(1);
    if i >= stop {
        loop {
            if !t.le(at(i)?) {
                return Ok(i.wrapping_add(1));
            }
            i = i.wrapping_sub(1);
            if i < stop {
                break;
            }
        }
    }
    let mut i = 0;
    while t.ge(at(i)?) {
        i += 1;
    }
    Ok(i)
}

/// [soft 0x084d822c si_OLDnurbsSrfChangeKnotsSeq]: `(s, n, knots)`: the surface re-expressed on the `n` knots of its first
/// direction with the earlier span search ([`old_interv_img`]); x, y and z of every old point are multiplied by the weight in place
/// (each product rounded on the store), the new points built one per new knot by [`srf_subdivc`], the records swapped, and the
/// new points' x, y, z divided by their weights. Always 1.
pub fn old_srf_change_knots_seq_img(m: &mut Mem, s: u32, n: i32, knots: u32) -> Result<i32, Halt> {
    let sp0 = m.sp();
    let rec = m.local(0x68);
    let order = m.i32(s);
    let np = n.wrapping_sub(order);
    m.set_i32(rec, order);
    m.set_i32(rec + 4, np);
    m.set_i32(rec + 8, n);
    m.set_u32(rec + 0xc, knots);
    for o in [0x10u32, 0x14, 0x18, 0x1c] {
        let v = m.u32(s + o);
        m.set_u32(rec + o, v);
    }
    let v = m.u16(s + 0x20);
    m.set_u16(rec + 0x20, v);
    for w in 0..9u32 {
        let v = m.u32(s + 0x24 + 4 * w);
        m.set_u32(rec + 0x24 + 4 * w, v);
    }
    let total = np.wrapping_mul(m.i32(rec + 0x28));
    let newpts = nurbs_points_allocate(m, total);
    m.set_i32(rec + 0x48, total);
    m.set_u32(rec + 0x54, newpts);
    for p in 0..m.i32(s + 0x48) {
        let q = m.u32(s + 0x54).wrapping_add((p as u32).wrapping_mul(PT));
        let wt = F80::from_f64(m.f64(q + 0x18));
        for o in [0u32, 8, 0x10] {
            let v = (F80::from_f64(m.f64(q + o)) * wt).to_f64();
            m.set_f64(q + o, v);
        }
    }
    let mut i = 0;
    while i < np {
        let t = m.f64(knots.wrapping_add(8 * i as u32));
        let left = old_interv_img(m, s, t)?;
        i += 1;
        srf_subdivc(m, s, rec, left, i)?;
    }
    let old = m.u32(s + 0xc);
    if old != 0 {
        m.free(old);
        m.set_u32(s + 0xc, 0);
    }
    for w in 0..9u32 {
        let v = m.u32(rec + 4 * w);
        m.set_u32(s + 4 * w, v);
    }
    let ka = knots_allocate(m, n);
    m.set_u32(s + 0xc, ka);
    let mut i = 0;
    while i < m.i32(rec + 8) {
        let v = m.f64(knots.wrapping_add(8 * i as u32));
        let kn = m.u32(s + 0xc);
        m.set_f64(kn.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let op = m.u32(s + 0x54);
    if op != 0 {
        m.free(op);
        m.set_u32(s + 0x54, 0);
    }
    m.set_i32(s + 0x48, total);
    m.set_u32(s + 0x54, newpts);
    for p in 0..total {
        let q = newpts.wrapping_add((p as u32).wrapping_mul(PT));
        let wt = F80::from_f64(m.f64(q + 0x18));
        for o in [0u32, 8, 0x10] {
            let v = (F80::from_f64(m.f64(q + o)) / wt).to_f64();
            m.set_f64(q + o, v);
        }
    }
    for w in 0..9u32 {
        let v = m.u32(s + 0x24 + 4 * w);
        m.set_u32(rec + 4 * w, v);
    }
    for w in 0..9u32 {
        let v = m.u32(s + 4 * w);
        m.set_u32(s + 0x24 + 4 * w, v);
    }
    for w in 0..9u32 {
        let v = m.u32(rec + 4 * w);
        m.set_u32(s + 4 * w, v);
    }
    m.set_u32(s + 0x4c, 2);
    m.set_sp(sp0);
    Ok(1)
}

fn read_f64s(m: &Mem, a: u32, n: i32) -> Option<Vec<f64>> {
    if a == 0 || !(0..=1 << 24).contains(&n) || !m.is_mapped(a, (n as u32).wrapping_mul(8)) {
        return None;
    }
    Some((0..n as u32).map(|i| m.f64(a + 8 * i)).collect())
}

/// [soft 0x084d84a4 SI_nurbsSrfInsertKnot]: `(srf, t, dir)`: `dir == 1` exchanges the directions first, the first direction's knots
/// are copied with `t` inserted in order ([soft 0x084d80f8 getKnotPosition]) into a block of one more entry, the surface is
/// re-expressed by [`old_srf_change_knots_seq_img`], the block freed and with `dir == 0` the directions exchanged afterwards (any other
/// `dir` never). Answers the insertion index.
pub fn srf_insert_knot_img(m: &mut Mem, s: u32, t: f64, dir: i32) -> Result<i32, Halt> {
    if dir == 1 {
        srf_swap_uv(m, s);
    }
    let n = m.i32(s + 8);
    let nk = knots_allocate(m, n.wrapping_add(1));
    let old = read_f64s(m, m.u32(s + 0xc), n).ok_or(Halt::Refused("getKnotPosition"))?;
    let mut out = read_f64s(m, nk, n.wrapping_add(1)).ok_or(Halt::Refused("getKnotPosition"))?;
    let pos = knot_position(&mut out, &old, t, n).ok_or(Halt::Refused("getKnotPosition"))?;
    for (i, v) in out.iter().enumerate() {
        m.set_f64(nk + 8 * i as u32, *v);
    }
    old_srf_change_knots_seq_img(m, s, n.wrapping_add(1), nk)?;
    if nk != 0 {
        m.free(nk);
    }
    if dir == 0 {
        srf_swap_uv(m, s);
    }
    Ok(pos)
}
