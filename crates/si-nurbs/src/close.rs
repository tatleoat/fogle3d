// SPDX-License-Identifier: MIT
//! last1: `SI_nurbsSrfClose` (`nrb_point.c`) and `SI_nurbsConvertToFace` (`nrb_convert.c`) on the memory image
//! (finding 0025, section "last1").

use crate::image::{at8, crv_copy, crv_fill_distance_points, crv_inverse, free_clear, nurbs_crv_dispose, Halt, LEFT, SI_GBL_INDEX};
use crate::NurbsDir;
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_x87::{x, F80};

/// `SI_indexModifAllocate (0x08176860)`: ported in `si-modeler`, answered by the caller's world.
pub const SI_INDEX_MODIF_ALLOCATE: Routine = Routine::new("SI_indexModifAllocate", 0x0817_6860);
/// `SI_indexModifPropagate (0x08176988)`: ported in `si-modeler`, answered by the caller's world.
pub const SI_INDEX_MODIF_PROPAGATE: Routine = Routine::new("SI_indexModifPropagate", 0x0817_6988);
/// `SI_clusterModelUpdate (0x081f1a1c)`: ported in `si-anim`, answered by the caller's world.
pub const SI_CLUSTER_MODEL_UPDATE: Routine = Routine::new("SI_clusterModelUpdate", 0x081f_1a1c);
/// `SI_statusDisplayMessage (0x083cca94)`.
const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `mdlClose` (0x08822210): the model the editing commands work on.
const MDL_CLOSE: u32 = 0x0882_2210;
/// The text at 0x086da420 ("To open it linearly you must have at least %d points.").
const MSG_OPEN_LINEAR: &[u8] = b"To open it linearly you must have at least ";

fn knot_at(m: &Mem, k: u32, i: i32) -> F80 {
    x(m.f64(at8(k, i)))
}

/// The parameter is `t`: `net point / w` for x, y and z, each division in the register.
fn net_xyz(m: &Mem, p: u32) -> [F80; 3] {
    let w = x(m.f64(p + 0x18));
    [x(m.f64(p)) / w, x(m.f64(p + 8)) / x(m.f64(p + 0x18)), x(m.f64(p + 0x10)) / x(m.f64(p + 0x18))]
}

/// `SI_nurbsSrfKnotInsert` after `SI_nurbsKnotsCorrectMultAndValue` on the direction record at `dir`.
fn correct_mult(m: &mut Mem, dir: u32, t: &mut f64, left: i32, r: &mut i32) -> Result<(), Halt> {
    let rec: NurbsDir = m.read(Ptr32::new(dir));
    let kv = crate::fit2::knot_vec(m, &rec);
    crate::insert::knots_correct_mult_and_value(crate::Dir { rec: &rec, knots: &kv }, t, left, r).ok_or(Halt::Refused("SI_nurbsKnotsCorrectMultAndValue"))
}

/// After an insertion, the editing commands' index table (`SI_gblIndex`) is applied to the clusters and a fresh one is made.
fn propagate_index(m: &mut Mem, o: &mut dyn Outside) {
    if m.u32(SI_GBL_INDEX) != 0 {
        let mdl = m.u32(MDL_CLOSE);
        o.call(m, SI_INDEX_MODIF_PROPAGATE, &[mdl, SI_GBL_INDEX, 1]);
        let mdl = m.u32(MDL_CLOSE);
        o.call(m, SI_CLUSTER_MODEL_UPDATE, &[mdl]);
        let mdl = m.u32(MDL_CLOSE);
        let g = o.call(m, SI_INDEX_MODIF_ALLOCATE, &[mdl, 1]).eax;
        m.set_u32(SI_GBL_INDEX, g);
    }
}

/// The sum of `count` distances `dist[i]`, `i` counting up from `start` and wrapping to 0 at `n`, in the register from +0.
fn wrapped_sum(m: &Mem, dist: u32, mut i: i32, count: i32, n: i32) -> F80 {
    let mut sum = F80::ZERO;
    let mut left = count;
    while left > 0 {
        sum = sum + x(m.f64(at8(dist, i)));
        i = i.wrapping_add(1);
        if i == n {
            i = 0;
        }
        left -= 1;
    }
    sum
}

/// The distance of the first corner of the last row to the first row's, the weight of the new row: 1.0 for a surface whose
/// parametrisation word (+0x4c) is 0, else the Euclidean distance of the two corners (their coordinates divided by w in the
/// register; a second square root for 4), held to 0.01 from below (a NaN passes).
fn corner_distance(m: &Mem, p: u32, last: u32, mode: i32) -> F80 {
    if mode == 0 {
        return F80::ONE;
    }
    let [ax, ay, az] = net_xyz(m, p.wrapping_add(last));
    let [bx, by, bz] = net_xyz(m, p);
    let (dy, dx, dz) = (ay - by, ax - bx, az - bz);
    let mut d = ((dy * dy + dx * dx) + dz * dz).sqrt();
    if mode == 4 {
        d = d.sqrt();
    }
    if x(0.01).gt(d) { x(0.01) } else { d }
}

/// [soft 0x084ed69c SI_nurbsSrfClose]: `(srf, how)` closes or opens a surface in its second (V, at +0x24) direction; `how` 2 opens
/// a closed direction (+0x44 not 0) linearly or closes an open one by new rows, 1 turns a closed V direction into an open one
/// by inserting its end knots, any other value makes the parametrisation (+0x4c) of an open V direction from the net's chords
/// (2) or trims its end knots (`openNormal`) when the surface already has chord-length knots, and frees the knots otherwise.
/// At the end the display lines are disposed, the V closed flag is flipped and `SI_knotsSrfReparametrization` runs (not after
/// the message of the linear opening, which returns at once).
/// - 2, open (+0x44 = 0): `n - 2` rows of points are appended (n the V point count), row `i` the weighted mean of the last
///   row and the first row at `t = i / (n - 1)` (coordinates divided by w and blended in the register as `x*(1-t) + y*t`,
///   the weight `(w1 + w2) * 0.5`); a knot is appended, the distance between the last row's first point and the first row's
///   (1.0 for parametrisation 0; held to 0.01), and the knot count grows.
/// - 2, closed: when `count + 2 - order < order` the message "To open it linearly you must have at least %d points."
///   (level 2) is shown and the routine returns; else the last `n - 2` rows go (-1 into `SI_gblIndex`), the knots are
///   reallocated, the first `n - 2` set to the knot `n - 2` and the tail ones to the knot at `count - n + 1`, and
///   `checkValidOpenLinear` follows.
/// - 1 (closed only): the flags of all points are saved, the first and last distinct knots are found (`lo`, `hi`), knots at
///   `knot[lo]` (corrected for multiplicity) and `knot[hi]` (`n - 2` times) are inserted (V), with `SI_gblIndex` propagated
///   after each; the net is cut to the rows `lo ..` (or 0 .. when fewer than `n + 2` knots lie below `hi`, which also
///   frees the knots), the flags put back and the knots cut to match.
/// - otherwise, with +0x4c = 2 and V open: the knots from the chord lengths between the rows ([`crate::geom7::srf_average_points_distance`]
///   and the two curve helpers), the last interval from the mean distance of the corner points (held to 0.01).
///
/// Returns `Err` where one of the helpers refused (a read the original would make outside what the image holds).
pub fn srf_close(m: &mut Mem, o: &mut dyn Outside, s: u32, how: i32) -> Result<(), Halt> {
    let sp0 = m.sp();
    let r = close_inner(m, o, s, how);
    m.set_sp(sp0);
    r
}

fn close_inner(m: &mut Mem, o: &mut dyn Outside, s: u32, how: i32) -> Result<(), Halt> {
    let n28 = m.i32(s + 0x28);
    let tot = m.i32(s + 0x48);
    let n24 = m.i32(s + 0x24);
    let dir = s.wrapping_add(0x24);
    if how == 2 {
        if m.i16(s + 0x44) == 0 {
            let d = n24.wrapping_sub(2);
            let bytes = (n28.wrapping_add(d) as u32).wrapping_mul(m.u32(s + 4)).wrapping_mul(0x24);
            let p = m.si_realloc(m.u32(s + 0x54), bytes);
            m.set_u32(s + 0x54, p);
            let old_total = m.i32(s + 0x48);
            let step = (F80::ONE / F80::from_i32(d.wrapping_add(1))).to_f64();
            let mut t = step;
            let mut dst = (old_total as u32).wrapping_mul(0x24);
            let mut i: i32 = 0;
            if 0 < d {
                loop {
                    i += 1;
                    let tn = (x(t) + x(step)).to_f64();
                    let v = m.i32(s + 4);
                    if 0 < v {
                        let c = (F80::ONE - x(t)).to_f64();
                        let a_off = (m.i32(s + 0x48).wrapping_sub(v) as u32).wrapping_mul(0x24);
                        let mut b_off: u32 = 0;
                        let mut a_off = a_off;
                        let mut j: i32 = 0;
                        loop {
                            let base = m.u32(s + 0x54);
                            let (pa, pb) = (base.wrapping_add(a_off), base.wrapping_add(b_off));
                            let [t2, t3, t4] = net_xyz(m, pa);
                            let [t5, t6, t7] = net_xyz(m, pb);
                            let (cx, tx) = (x(c), x(t));
                            // x and y: both products in the register; z: the second product with the stored double of t7
                            let xs = (t2 * cx + t5 * tx).to_f64();
                            let ys = (t3 * cx + t6 * tx).to_f64();
                            let zs = (t4 * cx + tx * x(t7.to_f64())).to_f64();
                            m.set_u32(base.wrapping_add(dst).wrapping_add(0x20), 0);
                            let w = |m: &Mem| x(m.f64(pa + 0x18)) + x(m.f64(pb + 0x18));
                            let base = m.u32(s + 0x54);
                            let _ = base;
                            let half = x(0.5);
                            let (px, py, pz) = ((w(m) * x(xs)) * half, (w(m) * x(ys)) * half, (w(m) * x(zs)) * half);
                            let b2 = m.u32(s + 0x54).wrapping_add(dst);
                            m.set_f64(b2, px.to_f64());
                            m.set_f64(b2 + 8, py.to_f64());
                            m.set_f64(b2 + 0x10, pz.to_f64());
                            m.set_f64(b2 + 0x18, (w(m) * half).to_f64());
                            j += 1;
                            a_off = a_off.wrapping_add(0x24);
                            b_off = b_off.wrapping_add(0x24);
                            dst = dst.wrapping_add(0x24);
                            if !(j < m.i32(s + 4)) {
                                break;
                            }
                        }
                    }
                    t = tn;
                    if !(i < d) {
                        break;
                    }
                }
            }
            let base = m.u32(s + 0x54);
            let last = ((m.i32(s + 0x48).wrapping_sub(m.i32(s + 4))) as u32).wrapping_mul(0x24);
            let val = corner_distance(m, base, last, m.i32(s + 0x4c));
            let esi = m.i32(s + 0x2c);
            let n28n = m.i32(s + 0x28).wrapping_add(d);
            m.set_i32(s + 0x28, n28n);
            m.set_i32(s + 0x48, n28n.wrapping_mul(m.i32(s + 4)));
            let kp = m.si_realloc(m.u32(s + 0x30), (esi as u32).wrapping_mul(8).wrapping_add(8));
            m.set_u32(s + 0x30, kp);
            let nk = val + x(m.f64(at8(kp, esi.wrapping_sub(1))));
            m.set_f64(at8(kp, esi), nk.to_f64());
            m.set_i32(s + 0x2c, m.i32(s + 0x2c).wrapping_add(1));
            m.set_u32(s + 0x4c, 2);
        } else {
            if n28.wrapping_add(2).wrapping_sub(n24) < n24 {
                let buf = m.local(80);
                for (k, b) in MSG_OPEN_LINEAR.iter().enumerate() {
                    m.set_u8(buf + k as u32, *b);
                }
                let mut at = buf + MSG_OPEN_LINEAR.len() as u32;
                for b in si_mem::clib::decimal(n24.wrapping_add(2)).iter().chain(b" points.".iter()) {
                    m.set_u8(at, *b);
                    at += 1;
                }
                m.set_u8(at, 0);
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 2]);
                return Ok(());
            }
            let d = n24.wrapping_sub(2);
            if m.u32(SI_GBL_INDEX) != 0 {
                let mut b: i32 = 0;
                while b < d.wrapping_mul(m.i32(s + 4)) {
                    let g = m.u32(SI_GBL_INDEX);
                    let idx = m.i32(s + 0x48).wrapping_sub(1).wrapping_sub(b);
                    m.set_i32(g.wrapping_add((idx as u32).wrapping_mul(4)), -1);
                    b += 1;
                }
            }
            let n28n = m.i32(s + 0x28).wrapping_sub(d);
            m.set_i32(s + 0x28, n28n);
            let t2 = n28n.wrapping_mul(m.i32(s + 4));
            m.set_i32(s + 0x48, t2);
            let p = m.si_realloc(m.u32(s + 0x54), (t2 as u32).wrapping_mul(0x24));
            m.set_u32(s + 0x54, p);
            let esi = n24.wrapping_add(m.i32(s + 0x28)).wrapping_sub(2);
            m.set_i32(s + 0x2c, esi);
            let kp = m.si_realloc(m.u32(s + 0x30), (esi as u32).wrapping_mul(8));
            m.set_u32(s + 0x30, kp);
            let mut b: i32 = 0;
            while b < d {
                let v = knot_at(m, kp, n24.wrapping_sub(2));
                m.set_f64(at8(kp, b), v.to_f64());
                b += 1;
            }
            let edx = esi.wrapping_sub(n24);
            let mut b = edx.wrapping_add(2);
            while b < esi {
                let v = knot_at(m, kp, edx.wrapping_add(1));
                m.set_f64(at8(kp, b), v.to_f64());
                b += 1;
            }
            m.set_u32(s + 0x4c, 2);
            crate::geom7::check_valid_open_linear(m, dir);
        }
    } else if how == 1 {
        if m.i16(s + 0x44) != 0 {
            close_v(m, o, s, n24, tot)?;
        }
    } else if m.i32(s + 0x4c) == 2 {
        if m.i16(s + 0x44) == 0 {
            chord_knots(m, s, n24, n28)?;
        } else {
            crate::geom8::open_normal(m, dir, s + 0x54, m.i32(s + 4));
            m.set_i32(s + 0x48, m.i32(s + 4).wrapping_mul(m.i32(s + 0x28)));
        }
    } else {
        free_clear(m, s + 0x30);
    }
    crate::image::srf_dispose_lines(m, o, s);
    let f = m.u8(s + 0x44) ^ 1;
    m.set_u8(s + 0x44, f);
    crate::geom3::knots_srf_reparametrization(m, o, s);
    Ok(())
}

/// How 1: the closed V direction is cut open at its first and last distinct knots (see [`srf_close`]).
fn close_v(m: &mut Mem, o: &mut dyn Outside, s: u32, n24: i32, tot: i32) -> Result<(), Halt> {
    let dir = s.wrapping_add(0x24);
    let old = m.u32(s + 0x54);
    let flags = m.si_calloc(tot as u32, 4);
    let (l180, l188) = (n24.wrapping_sub(2), n24.wrapping_add(2));
    let mut j: i32 = 0;
    while j < tot {
        let v = m.u32(old.wrapping_add((j as u32).wrapping_mul(0x24)).wrapping_add(0x20));
        m.set_u32(flags.wrapping_add((j as u32).wrapping_mul(4)), v);
        j += 1;
    }
    let mut mult: i32 = l180;
    // the first scan: knots from 0 up, until n - 2 steps have been counted
    let mut esi: i32 = 0;
    let mut ebx: i32 = 0;
    if esi != l180 {
        let kn = m.u32(s + 0x30);
        let edx = m.i32(s + 0x2c).wrapping_sub(1);
        if ebx < edx {
            loop {
                if !knot_at(m, kn, ebx.wrapping_add(1)).eq(knot_at(m, kn, ebx)) {
                    esi += 1;
                }
                ebx += 1;
                if esi == l180 || !(ebx < edx) {
                    break;
                }
            }
        }
    }
    let mut lo = ebx;
    // the second scan: from the end down to the first step
    let mut esi: i32 = 0;
    let mut ebx = m.i32(s + 0x2c).wrapping_sub(1);
    if ebx > 0 {
        let kn = m.u32(s + 0x30);
        loop {
            if !knot_at(m, kn, ebx.wrapping_sub(1)).eq(knot_at(m, kn, ebx)) {
                esi += 1;
            }
            ebx -= 1;
            if esi == 1 || !(ebx > 0) {
                break;
            }
        }
    }
    let mut hi = ebx;
    if lo > ebx {
        hi = lo;
        lo = ebx;
    }
    let kn = m.u32(s + 0x30);
    let mut t_lo = m.f64(at8(kn, lo));
    let t_hi = m.f64(at8(kn, hi));
    // the span of the first parameter
    let open = (m.i16(s + 0x44) == 0) as i16;
    let rec: NurbsDir = m.read(Ptr32::new(dir));
    let kv = crate::fit2::knot_vec(m, &rec);
    let mut st = crate::Statics { left: m.i32(LEFT) };
    let span = crate::knots::interv(&mut st, crate::Dir { rec: &rec, knots: &kv }, t_lo, open);
    m.set_i32(LEFT, st.left);
    let span = span.ok_or(Halt::Refused("si_nurbsInterv"))?;
    if m.u32(SI_GBL_INDEX) != 0 {
        // SI_indexModifFree (0x08176a00) on the table, then a fresh one
        free_clear(m, SI_GBL_INDEX);
        let mdl = m.u32(MDL_CLOSE);
        let g = o.call(m, SI_INDEX_MODIF_ALLOCATE, &[mdl, 1]).eax;
        m.set_u32(SI_GBL_INDEX, g);
    }
    correct_mult(m, dir, &mut t_lo, span, &mut mult)?;
    crate::bridge::srf_knot_insert(m, o, s, t_lo, mult, 1);
    propagate_index(m, o);
    crate::bridge::srf_knot_insert(m, o, s, t_hi, l180, 1);
    propagate_index(m, o);
    // how many knots from `lo` on are not above the second parameter
    let kn = m.u32(s + 0x30);
    let thi = x(t_hi);
    let mut esi: i32 = 0;
    let mut ebx = lo;
    let k0 = knot_at(m, kn, ebx);
    if !(thi.lt(k0) || thi.unordered(k0)) {
        let edi = m.i32(s + 0x2c);
        if ebx < edi {
            loop {
                esi += 1;
                ebx += 1;
                let k = knot_at(m, kn, ebx);
                if thi.lt(k) || thi.unordered(k) || !(ebx < edi) {
                    break;
                }
            }
        }
    }
    let mut wrapped = false;
    if esi < l188 {
        esi = l188;
        lo = 0;
        wrapped = true;
    }
    let n28n = esi.wrapping_sub(n24).wrapping_add(2);
    m.set_i32(s + 0x28, n28n);
    let old = m.u32(s + 0x54);
    let v = m.i32(s + 4);
    m.set_i32(s + 0x48, n28n.wrapping_mul(v));
    let newp = nurbs_points_allocate(m, m.i32(s + 0x48));
    let base_idx = lo.wrapping_mul(m.i32(s + 4));
    let (mut src, mut dst) = ((base_idx as u32).wrapping_mul(0x24), 0u32);
    let mut j: i32 = 0;
    while j < m.i32(s + 0x48) {
        for k in 0..9u32 {
            let w = m.u32(old.wrapping_add(src).wrapping_add(4 * k));
            m.set_u32(newp.wrapping_add(dst).wrapping_add(4 * k), w);
        }
        let f = m.u32(flags.wrapping_add((j as u32).wrapping_mul(4)));
        m.set_u32(newp.wrapping_add(dst).wrapping_add(0x20), f);
        let g = m.u32(SI_GBL_INDEX);
        if g != 0 {
            m.set_i32(g.wrapping_add((base_idx.wrapping_add(j) as u32).wrapping_mul(4)), j);
        }
        src = src.wrapping_add(0x24);
        dst = dst.wrapping_add(0x24);
        j += 1;
    }
    free_clear(m, s + 0x54);
    m.set_u32(s + 0x54, newp);
    if flags != 0 {
        m.free(flags);
    }
    let total = n24.wrapping_add(m.i32(s + 0x28));
    let cnt = total.wrapping_sub(2);
    let oldk = m.u32(s + 0x30);
    if wrapped {
        free_clear(m, s + 0x30);
        m.set_u32(s + 0x30, 0);
        m.set_i32(s + 0x2c, 0);
        return Ok(());
    }
    let nk = m.si_calloc(cnt as u32, 8);
    let mut b: i32 = 0;
    while b < cnt {
        let v = knot_at(m, oldk, lo.wrapping_add(b));
        m.set_f64(at8(nk, b), v.to_f64());
        b += 1;
    }
    free_clear(m, s + 0x30);
    m.set_u32(s + 0x30, nk);
    m.set_i32(s + 0x2c, cnt);
    Ok(())
}

/// The last case of [`srf_close`] (parametrisation 2, V open): the knots of V made from the chord lengths between the rows.
fn chord_knots(m: &mut Mem, s: u32, n24: i32, n28: i32) -> Result<(), Halt> {
    let slot = m.local(8);
    let tot_at = slot + 4;
    let crv = crate::geom7::srf_average_points_distance(m, s, 0)?;
    m.set_u32(slot, crv);
    let dist = knots_allocate(m, n28);
    crv_fill_distance_points(m, crv, dist, tot_at, 1)?;
    // the mean distance of the rows' end points, accumulated as a double
    let mut acc: f64 = 0.0;
    let v0 = m.i32(s + 4);
    let mut j: i32 = 0;
    if 0 < v0 {
        let mut pa = m.u32(s + 0x54);
        loop {
            let base = m.u32(s + 0x54);
            let v = m.i32(s + 4);
            let pb = base.wrapping_add(((m.i32(s + 0x48).wrapping_sub(v.wrapping_sub(j))) as u32).wrapping_mul(0x24));
            let [ax, ay, az] = net_xyz(m, pa);
            let [bx, by, bz] = net_xyz(m, pb);
            let (dx, dy, dz) = (ax - bx, ay - by, az - bz);
            let mut d = (dz * dz + (dy * dy + dx * dx)).sqrt();
            if m.i32(s + 0x4c) == 4 {
                d = d.sqrt();
            }
            acc = (d + x(acc)).to_f64();
            pa = pa.wrapping_add(0x24);
            j += 1;
            if !(j < m.i32(s + 4)) {
                break;
            }
        }
    }
    let mean = x(acc) / F80::from_i32(m.i32(s + 0x28));
    let mean = if x(0.01).gt(mean) { x(0.01) } else { mean };
    m.set_f64(at8(dist, n28.wrapping_sub(1)), mean.to_f64());
    let nseg = n28.wrapping_sub(n24).wrapping_add(2);
    let segs = knots_allocate(m, nseg);
    crate::blend::crv_fill_segment_points(m, crv, dist, segs);
    let scale = (x(m.f64(crv + 0x18)) - x(m.f64(crv + 0x10))) / x(m.f64(at8(segs, nseg.wrapping_sub(1))));
    let oldk = m.u32(s + 0x30);
    let newk = m.si_calloc(n28.wrapping_add(1) as u32, 8);
    let mut cur = x(m.f64(crv + 0x10));
    let mut sidx = n28.wrapping_sub(1);
    let mut b = n24.wrapping_sub(3);
    if b >= 0 {
        loop {
            let sum = wrapped_sum(m, dist, sidx, n24.wrapping_sub(1), n28);
            cur = cur - sum * scale;
            m.set_f64(at8(newk, b), cur.to_f64());
            sidx = sidx.wrapping_sub(1);
            b -= 1;
            if b < 0 {
                break;
            }
        }
    }
    let mut b = n24.wrapping_sub(2);
    while b < m.i32(crv + 8).wrapping_sub(n24).wrapping_add(2) {
        let v = knot_at(m, oldk, b);
        m.set_f64(at8(newk, b), v.to_f64());
        b += 1;
    }
    let prev = knot_at(m, newk, b.wrapping_sub(1));
    let sum = wrapped_sum(m, dist, sidx, n24.wrapping_sub(1), n28);
    let v = prev + scale * sum;
    m.set_f64(at8(newk, b), v.to_f64());
    free_clear(m, s + 0x30);
    m.set_u32(s + 0x30, newk);
    m.set_i32(s + 0x2c, n28.wrapping_add(1));
    if segs != 0 {
        m.free(segs);
    }
    nurbs_crv_dispose(m, slot);
    Ok(())
}

/// [soft 0x084e1450 SI_nurbsConvertToFace]: `(curve, face)` makes the face's outline from a curve: when the face is not null its
/// +0x84 short is cleared, a copy of the curve (`SI_nurbsCrvCopy`) is stored at +0x80 and reversed (`SI_nurbsCrvInverse`), and
/// the short at +0x78 is set to 1.
pub fn nurbs_convert_to_face(m: &mut Mem, o: &mut dyn Outside, crv: u32, face: u32) {
    if face != 0 {
        m.set_u16(face.wrapping_add(0x84), 0);
        let c = crv_copy(m, crv);
        m.set_u32(face.wrapping_add(0x80), c);
        crv_inverse(m, o, c);
        m.set_u16(face.wrapping_add(0x78), 1);
    }
}
