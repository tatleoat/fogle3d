// SPDX-License-Identifier: MIT
//! Unit tests on made-up curves and surfaces. They guard the port against drifting from the
//! reading; the comparison with the original is `nurbs-emucheck`.

use crate::own::{self, Curve, NoHost, Surface};
use crate::{eval, insert, knots, sample, NurbsCrv, NurbsPoint, NurbsSrf, Statics};
use si_types::{Double, Zero};

fn pt(x: f64, y: f64, z: f64, w: f64) -> NurbsPoint {
    let mut p = NurbsPoint::zero();
    p.x = Double(x * w);
    p.y = Double(y * w);
    p.z = Double(z * w);
    p.w = Double(w);
    p
}

/// An open curve in the program's layout: `n + order − 2` knots.
fn curve(order: i32, kn: &[f64], pts: Vec<NurbsPoint>, closed: i16) -> Curve {
    let mut rec = NurbsCrv::allocate();
    rec.dir.order = order;
    rec.dir.nb_points = pts.len() as i32;
    rec.dir.nb_knots = kn.len() as i32;
    rec.dir.closed = closed;
    let (a, b) = if closed != 0 { (0, kn.len() - 1) } else { (order as usize - 2, kn.len() + 1 - order as usize) };
    rec.dir.range_start = Double(kn[a]);
    rec.dir.range_end = Double(kn[b]);
    Curve::from_parts(rec, Some(kn.to_vec()), Some(pts))
}

fn bezier_cubic() -> Curve {
    curve(4, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![pt(0.0, 0.0, 0.0, 1.0), pt(1.0, 2.0, 0.0, 1.0), pt(3.0, 2.0, 0.0, 1.0), pt(4.0, 0.0, 0.0, 1.0)], 0)
}

#[test]
fn binomials() {
    assert_eq!(eval::binom(5, 2), Some(10));
    assert_eq!(eval::binom(6, 3), Some(20));
    assert_eq!(eval::binom(4, 0), Some(1));
    assert_eq!(eval::binom(4, 4), Some(1));
    assert_eq!(eval::binom(7, 6), Some(7));
}

#[test]
fn span_of_a_parameter() {
    let c = curve(4, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0], (0..6).map(|i| pt(i as f64, 0.0, 0.0, 1.0)).collect(), 0);
    let d = c.view().dir();
    let mut st = Statics::default();
    // on a knot: the span that starts there
    assert_eq!(knots::interv(&mut st, d, 1.0, 1), Some(3));
    assert_eq!(knots::interv(&mut st, d, 0.0, 1), Some(2));
    assert_eq!(knots::interv(&mut st, d, 1.999, 1), Some(3));
    // at the end of the range, and within 1e-5 of it: the last span
    assert_eq!(knots::interv(&mut st, d, 3.0, 1), Some(4));
    assert_eq!(knots::interv(&mut st, d, 3.0 - 5e-6, 1), Some(4));
    // outside: clamped
    assert_eq!(knots::interv(&mut st, d, -7.0, 1), Some(2));
    assert_eq!(knots::interv(&mut st, d, 7.0, 1), Some(4));
    assert_eq!(st.left, 4);
}

#[test]
fn a_range_that_disagrees_with_the_knots_never_returns() {
    let mut c = bezier_cubic();
    c.rec.dir.range_end = Double(2.0);
    assert_eq!(knots::interv(&mut Statics::default(), c.view().dir(), 1.5, 1), None);
}

#[test]
fn bezier_values_and_derivatives() {
    let c = bezier_cubic();
    let mut st = Statics::default();
    assert_eq!(eval::crv_value(&mut st, c.view(), 0.0, 0), Some([0.0, 0.0, 0.0]));
    assert_eq!(eval::crv_value(&mut st, c.view(), 1.0, 0), Some([4.0, 0.0, 0.0]));
    assert_eq!(eval::crv_value(&mut st, c.view(), 0.5, 0), Some([2.0, 1.5, 0.0]));
    // first derivative at the start: 3 (P1 − P0)
    assert_eq!(eval::crv_value(&mut st, c.view(), 0.0, 1), Some([3.0, 6.0, 0.0]));
    // second: 6 (P2 − 2 P1 + P0)
    assert_eq!(eval::crv_value(&mut st, c.view(), 0.0, 2), Some([6.0, -12.0, 0.0]));
    // a derivative order above the degree repeats the last one instead of giving zero
    assert_eq!(eval::crv_value(&mut st, c.view(), 0.25, 4), eval::crv_value(&mut st, c.view(), 0.25, 3));
}

#[test]
fn a_factor_near_a_knot_copies_the_point() {
    let c = bezier_cubic();
    let mut st = Statics::default();
    // within a millionth of the span from its start, the value is the start's exactly
    assert_eq!(eval::crv_value(&mut st, c.view(), 5e-7, 0), Some([0.0, 0.0, 0.0]));
    assert_ne!(eval::crv_value(&mut st, c.view(), 5e-6, 0), Some([0.0, 0.0, 0.0]));
}

#[test]
fn a_rational_quarter_circle() {
    let h = 0.5f64.sqrt();
    let c = curve(3, &[0.0, 0.0, 1.0, 1.0], vec![pt(1.0, 0.0, 0.0, 1.0), pt(1.0, 1.0, 0.0, h), pt(0.0, 1.0, 0.0, 1.0)], 0);
    let mut st = Statics::default();
    for i in 0..=8 {
        let p = eval::crv_value(&mut st, c.view(), i as f64 / 8.0, 0).unwrap();
        assert!((p[0] * p[0] + p[1] * p[1] - 1.0).abs() < 1e-15, "{p:?}");
    }
    // a zero weight is returned undivided
    let mut z = c.clone();
    z.points.as_mut().unwrap()[0] = pt(0.0, 0.0, 0.0, 1.0);
    z.points.as_mut().unwrap()[0].x = Double(5.0);
    z.points.as_mut().unwrap()[0].w = Double(0.0);
    assert_eq!(eval::crv_value(&mut st, z.view(), 0.0, 0), Some([5.0, 0.0, 0.0]));
}

#[test]
fn a_closed_curve_meets_itself() {
    let pts: Vec<NurbsPoint> = (0..5).map(|i| pt((i as f64).cos(), (i as f64 * 2.0).sin(), i as f64 * 0.25, 1.0)).collect();
    let c = curve(4, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], pts, 1);
    let mut st = Statics::default();
    let a = eval::crv_value(&mut st, c.view(), 0.0, 0).unwrap();
    let b = eval::crv_value(&mut st, c.view(), 5.0, 0).unwrap();
    for q in 0..3 {
        assert!((a[q] - b[q]).abs() < 1e-14);
    }
}

#[test]
fn beyond_the_range_the_factors_are_clamped() {
    let mut st = Statics::default();
    // a clamped curve: every factor is below zero beyond the end, every point is replaced by
    // its right neighbour, and the result is the end point exactly
    let c = bezier_cubic();
    assert_eq!(eval::crv_value(&mut st, c.view(), 7.0, 0), Some([4.0, 0.0, 0.0]));
    assert_eq!(eval::crv_value(&mut st, c.view(), -7.0, 0), Some([0.0, 0.0, 0.0]));
    // an unclamped curve: far beyond its end it gives the last control point of the last
    // span, which is not a point of the curve
    let u = curve(4, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], (0..6).map(|i| pt(i as f64, (i * i) as f64, 0.0, 1.0)).collect(), 0);
    let end = eval::crv_value(&mut st, u.view(), 5.0, 0).unwrap();
    assert_eq!(eval::crv_value(&mut st, u.view(), 9.0, 0), Some([5.0, 25.0, 0.0]));
    assert_ne!(end, [5.0, 25.0, 0.0]);
    // just beyond, some factors are clamped and some are not: neither the end nor that point
    let near = eval::crv_value(&mut st, u.view(), 5.5, 0).unwrap();
    assert!(near != end && near != [5.0, 25.0, 0.0]);
}

fn plane(nu: usize, nv: usize) -> Surface {
    let mut rec = NurbsSrf::allocate();
    let knots = |n: usize| -> Vec<f64> { (0..n + 2).map(|i| i as f64).collect() };
    for (d, n) in [(&mut rec.u, nu), (&mut rec.v, nv)] {
        d.order = 4;
        d.nb_points = n as i32;
        d.nb_knots = n as i32 + 2;
        d.range_start = Double(2.0);
        d.range_end = Double(n as f64 - 1.0);
    }
    rec.nb_points = (nu * nv) as i32;
    let mut pts = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            pts.push(pt(i as f64, 2.0 * j as f64, 0.5 * i as f64, 1.0));
        }
    }
    Surface::from_parts(rec, Some(knots(nu)), Some(knots(nv)), Some(pts))
}

#[test]
fn a_plane_and_its_normal() {
    let s = plane(6, 5);
    let mut st = Statics::default();
    let p = eval::srf_value(&mut st, s.view(), 3.25, 2.5).unwrap();
    // a uniform cubic reproduces a linear grid: the point of parameter (u, v) is (u − 1, 2 (v − 1), …)
    assert!((p[0] - 2.25).abs() < 1e-14 && (p[1] - 3.0).abs() < 1e-14 && (p[2] - 1.125).abs() < 1e-14, "{p:?}");
    let (q, n) = eval::srf_value_and_normal(&mut st, s.view(), 3.25, 2.5, None, None).unwrap();
    assert_eq!(p, q);
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    assert!((len - 1.0).abs() < 1e-6);
    // across × along for the plane z = x / 2: (1, 0, 0.5) × (0, 2, 0) is along (−1, 0, 2)
    assert!(n[1].abs() < 1e-12 && n[0] < 0.0 && n[2] > 0.0, "{n:?}");
    // the whole curve in V at u passes through the same point
    let c = eval::srf_extract_curve_u(&mut st, s.view(), 3.25, 0).unwrap();
    let r = eval::crv_value(&mut st, c.view(), 2.5, 0).unwrap();
    for k in 0..3 {
        assert!((r[k] - p[k]).abs() < 1e-13);
    }
    assert_eq!(c.rec.dir.nb_points, 5);
    assert_eq!(c.rec.step, s.rec.v_step);
}

#[test]
fn a_collapsed_surface_has_no_normal() {
    let mut s = plane(5, 5);
    for p in s.points.as_mut().unwrap() {
        *p = pt(1.0, 2.0, 3.0, 1.0);
    }
    let mut tries = [0; 2];
    let (p, n) = eval::srf_value_and_normal_traced(&mut Statics::default(), s.view(), 2.5, 2.5, None, None, &mut tries).unwrap();
    assert_eq!(p, [1.0, 2.0, 3.0]);
    assert_eq!(n, [0.0, 0.0, 0.0]);
    assert_eq!(tries, [100, 100]);
}

#[test]
fn exchanging_u_and_v_twice() {
    let s = plane(6, 4);
    let mut w = s.clone();
    assert_eq!(own::srf_swap_uv(&mut NoHost, &mut w), Some(1));
    assert_eq!(w.rec.u.nb_points, 4);
    // point (i, j) of the exchanged surface is point (j, i) of the original
    assert_eq!(w.points.as_ref().unwrap()[2 * 4 + 1], s.points.as_ref().unwrap()[6 + 2]);
    assert_eq!(own::srf_swap_uv(&mut NoHost, &mut w), Some(1));
    assert_eq!(w, s);
    let mut empty = Surface::allocate();
    assert_eq!(own::srf_swap_uv(&mut NoHost, &mut empty), Some(0));
}

#[test]
fn inserting_a_knot_keeps_the_curve() {
    let c = curve(4, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0], (0..6).map(|i| pt(i as f64, (i * i) as f64 * 0.5, (i % 2) as f64, 1.0 + 0.25 * i as f64)).collect(), 0);
    let mut st = Statics::default();
    let mut d = c.clone();
    assert_eq!(insert::crv_knot_insert(&mut st, &mut NoHost, &mut d, 1.5, 2, 1), Some(1));
    assert_eq!(d.rec.dir.nb_points, 8);
    assert_eq!(d.rec.dir.nb_knots, 10);
    assert_eq!(d.rec.param, 2);
    assert_eq!(d.knots.as_ref().unwrap()[3..7], [1.0, 1.5, 1.5, 2.0]);
    for i in 0..=30 {
        let t = i as f64 / 10.0;
        let (a, b) = (eval::crv_value(&mut st, c.view(), t, 0).unwrap(), eval::crv_value(&mut st, d.view(), t, 0).unwrap());
        for q in 0..3 {
            assert!((a[q] - b[q]).abs() < 1e-13, "t {t}: {a:?} {b:?}");
        }
    }
    // on a knot of full multiplicity nothing is inserted
    let mut e = d.clone();
    assert_eq!(insert::crv_knot_insert(&mut st, &mut NoHost, &mut e, 1.5, 2, 1), Some(1));
    assert_eq!(e.rec.dir.nb_points, 9);
    assert_eq!(insert::crv_knot_insert(&mut st, &mut NoHost, &mut e, 1.5, 2, 1), Some(0));
    assert_eq!(e.rec.dir.nb_points, 9);
}

#[test]
fn sampling_a_knot_vector() {
    let c = curve(4, &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0, 4.0, 4.0], (0..7).map(|i| pt(i as f64, 0.0, 0.0, 1.0)).collect(), 0);
    let d = c.view().dir();
    // spans 1, 1, 0, 2: the mean of the three real ones is 4/3; 3 samples to a mean span
    let a = sample::sample_knot_vector(Some(d), true, 3).unwrap().unwrap();
    assert_eq!(a.first(), Some(&0.0));
    assert_eq!(a.last(), Some(&4.0));
    assert_eq!(a.len(), 10);
    let (per, count, total) = sample::samples_per_knot(d, 3).unwrap();
    assert_eq!(count, 4);
    assert_eq!(per.unwrap(), vec![3.0, 3.0, 0.0, 3.0]);
    assert_eq!(total, 9);
    let mut out = [0.0; 5];
    assert_eq!(sample::knot_position(&mut out, &[0.0, 1.0, 2.0, 4.0], 2.0, 4), Some(2));
    assert_eq!(out, [0.0, 1.0, 2.0, 2.0, 4.0]);
    assert_eq!(sample::knot_position(&mut out, &[0.0, 1.0, 2.0, 4.0], 9.0, 4), Some(4));
}

#[test]
fn reversing_a_direction() {
    let mut c = curve(4, &[0.0, 0.0, 0.0, 1.0, 3.0, 4.0, 4.0, 4.0], (0..6).map(|i| pt(i as f64, 0.0, 0.0, 1.0)).collect(), 0);
    let (mut k, mut p) = (c.knots.take().unwrap(), c.points.take().unwrap());
    assert_eq!(crate::edit::crv_inverse(&mut NoHost, &mut c.rec, &mut k, &mut p), Some(1));
    assert_eq!(k, vec![0.0, 0.0, 0.0, 1.0, 3.0, 4.0, 4.0, 4.0].iter().rev().map(|v| 4.0 - v).collect::<Vec<f64>>());
    let (x, rs, re) = (p[0].x.0, c.rec.dir.range_start.0, c.rec.dir.range_end.0);
    assert_eq!((x, rs, re), (5.0, 0.0, 4.0));
}

#[test]
fn copies_and_disposal() {
    let c = bezier_cubic();
    let d = own::crv_copy(&c).unwrap();
    assert_eq!(d.points, c.points);
    assert_eq!(d.knots, c.knots);
    assert_eq!(own::crv_dispose(&mut Some(Box::new(d))), vec!["knots", "pts", "crv"]);
    let comp = own::comp_from_curve(&c).unwrap();
    assert_eq!(comp.rec.nb_curves, 1);
    assert_eq!(own::comp_dispose(&mut Some(Box::new(comp))), vec!["c0.knots", "c0.pts", "c0.crv", "curves", "comp"]);
}

#[test]
fn a_flat_square_becomes_two_triangles_on_the_image() {
    // order 2 both ways, 2 × 2 points, display steps 1: the samples are the knots alone
    use si_mem::{Mem, Ptr32};
    let mut m = Mem::new();
    let k = m.place(16, 8);
    m.set_f64(k, 0.0);
    m.set_f64(k + 8, 1.0);
    let corners = [(0.0, 0.0), (2.0, 0.0), (0.0, 3.0), (2.0, 3.0)];
    let pts = m.place(4 * 0x24, 8);
    for (i, (x, y)) in corners.iter().enumerate() {
        m.write(Ptr32::new(pts + 0x24 * i as u32), pt(*x, *y, 0.0, 1.0));
    }
    let mut s = NurbsSrf::allocate();
    for d in [&mut s.u, &mut s.v] {
        d.order = 2;
        d.nb_points = 2;
        d.nb_knots = 2;
        d.knots = si_types::Ptr32::new(k);
        d.range_start = Double(0.0);
        d.range_end = Double(1.0);
    }
    s.nb_points = 4;
    s.points = si_types::Ptr32::new(pts);
    s.u_step = 1;
    s.v_step = 1;
    let srf = m.place(0xdc, 8);
    m.write(Ptr32::new(srf), s);
    let out = m.place(16, 4);
    crate::convert::nurbs2triangle(&mut m, srf, srf, 0, 7, out, out + 4, out + 8, out + 12, -1).unwrap();
    assert_eq!((m.i32(out), m.i32(out + 8)), (4, 2));
    let (verts, tris) = (m.u32(out + 4), m.u32(out + 12));
    // vertex (i, j) is point (u_i, v_j); V is the major direction of the points, U of the vertices
    let pos = |v: u32| (m.f32(verts + 0x4c * v + 8), m.f32(verts + 0x4c * v + 12), m.f32(verts + 0x4c * v + 16));
    assert_eq!([pos(0), pos(1), pos(2), pos(3)], [(0.0, 0.0, 0.0), (0.0, 3.0, 0.0), (2.0, 0.0, 0.0), (2.0, 3.0, 0.0)]);
    // across (∂/∂u) × along (∂/∂v): x × y
    assert_eq!((m.f32(verts + 0x14), m.f32(verts + 0x18), m.f32(verts + 0x1c)), (0.0, 0.0, 1.0));
    // texture pairs: 1 − u, v
    assert_eq!((m.f32(verts), m.f32(verts + 4), m.f32(verts + 0x4c * 3), m.f32(verts + 0x4c * 3 + 4)), (1.0, 0.0, 0.0, 1.0));
    let corner = |t: u32, c: u32| (m.u32(tris + 0x60 * t + 4 * c) - verts) / 0x4c;
    assert_eq!([corner(0, 0), corner(0, 1), corner(0, 2), corner(1, 0), corner(1, 1), corner(1, 2)], [0, 2, 3, 0, 3, 1]);
    assert_eq!((m.u32(tris + 0x5c), m.u32(tris + 0x60 + 0x5c), m.i16(tris + 0x58)), (tris + 0x60, 0, 7));
    // the sample arrays and every curve the conversion extracted went back to the heap
    assert_eq!(m.live_blocks().len(), 2);
}

#[test]
fn a_point_moved_on_the_image_is_the_point_moved_on_values() {
    // the second of three points, weight 0.3: the image form writes what the first form
    // writes, marks the point and the curve, and leaves the other points alone
    use si_mem::{Mem, Ptr32};
    let mut m = Mem::new();
    let pts = m.place(3 * 0x24, 8);
    let ps = [pt(1.0, 2.0, 3.0, 1.0), pt(-1.5, 0.25, 7.0, 0.3), pt(4.0, 4.0, 4.0, 2.0)];
    for (i, p) in ps.iter().enumerate() {
        m.write(Ptr32::new(pts + 0x24 * i as u32), *p);
    }
    let mut c = NurbsCrv::allocate();
    c.dir.order = 2;
    c.dir.nb_points = 3;
    c.points = si_types::Ptr32::new(pts);
    let crv = m.place(core::mem::size_of::<NurbsCrv>() as u32, 8);
    m.write(Ptr32::new(crv), c);
    let (x, y, z) = (0.1, -9.75, 1e300);
    assert_eq!(crate::image::crv_move_point(&mut m, crv, 1, x, y, z), 1);
    let mut vals = ps.to_vec();
    assert_eq!(crate::misc::crv_move_point(&mut c, &mut vals, 1, x, y, z), Some(1));
    for (i, want) in vals.iter().enumerate() {
        let got: NurbsPoint = m.read(Ptr32::new(pts + 0x24 * i as u32));
        assert_eq!((got.x.0.to_bits(), got.y.0.to_bits(), got.z.0.to_bits(), got.w.0.to_bits(), got.flag), (want.x.0.to_bits(), want.y.0.to_bits(), want.z.0.to_bits(), want.w.0.to_bits(), want.flag));
    }
    assert_eq!(m.u8(pts + 0x24 + 0x23) & 0x60, 0x60);
    let got: NurbsCrv = m.read(Ptr32::new(crv));
    assert_eq!(got.flags_38, c.flags_38);
    assert_eq!(got.flags_38 & crate::misc::MOVED, crate::misc::MOVED);
}

#[test]
fn length_and_the_parameter_at_a_distance_on_values_and_on_the_image() {
    // tables of three entries: lengths 0, 1, 3 at parameters 0, 0.5, 1; the range 0 … 1
    use crate::geometry::{self, Refusal};
    use crate::image::{self, Halt};
    use si_mem::{Mem, Ptr32};
    use si_x87::F80;
    let mut c = bezier_cubic();
    c.length_param = Some(vec![0.0, 0.5, 1.0]);
    c.length_dist = Some(vec![0.0, 1.0, 3.0]);
    c.rec.length_data[0] = 3;
    c.sync();
    let mut m = Mem::new();
    let lp = m.place(24, 8);
    let ld = m.place(24, 8);
    for i in 0..3 {
        m.set_f64(lp + 8 * i, c.length_param.as_ref().unwrap()[i as usize]);
        m.set_f64(ld + 8 * i, c.length_dist.as_ref().unwrap()[i as usize]);
    }
    let crv = m.place(core::mem::size_of::<NurbsCrv>() as u32, 8);
    let put = |m: &mut Mem, c: &Curve| {
        let mut r = c.rec;
        r.length_data[1] = lp;
        r.length_data[2] = ld;
        m.write(Ptr32::new(crv), r);
    };
    let x = |v: f64| F80::from_f64(v).to_bits();
    let mut st = Statics::default();
    let mut o = si_mem::Trace::new();
    fn b<E>(r: Result<F80, E>) -> Result<(u64, u16), E> {
        r.map(F80::to_bits)
    }
    for closed in [0i16, 1] {
        c.rec.dir.closed = closed;
        put(&mut m, &c);
        assert_eq!(b(geometry::curve_length(&mut st, Some(&mut c))), Ok(x(3.0)));
        assert_eq!(b(image::nurbs_curve_length(&mut m, &mut o, crv)), Ok(x(3.0)));
        // within 0.0001 of 0 the start, of the length the end; inside, the line between entries
        // 1 and 2; beyond the ends the range's ends when open, one length round when closed
        let wrapped = if closed != 0 { x(0.75) } else { x(1.0) };
        let below = if closed != 0 { x(0.75) } else { x(0.0) };
        for (d, want) in [(0.0, x(0.0)), (-0.0001, x(0.0)), (2.99995, x(1.0)), (2.0, x(0.75)), (0.5, x(0.25)), (5.0, wrapped), (-1.0, below)] {
            assert_eq!(b(geometry::distance_to_factor(&mut st, &mut c, d)), Ok(want), "closed {closed} distance {d}");
            assert_eq!(b(image::nurbs_distance_to_factor(&mut m, &mut o, crv, d)), Ok(want), "closed {closed} distance {d}");
        }
    }
    // a NaN distance finds no entry above it; a closed curve of length 0 loops for ever
    assert_eq!(b(geometry::distance_to_factor(&mut st, &mut c, f64::NAN)), Err(Refusal::NoSpan));
    assert_eq!(b(image::nurbs_distance_to_factor(&mut m, &mut o, crv, f64::NAN)), Err(Halt::Refused("SI_nurbsDistanceToFactor")));
    c.length_dist = Some(vec![0.0, 0.0, 0.0]);
    assert_eq!(b(geometry::distance_to_factor(&mut st, &mut c, 1.0)), Err(Refusal::Endless));
    // no curve: a length of 0
    assert_eq!(b(geometry::curve_length(&mut st, None)), Ok(F80::ZERO.to_bits()));
    assert_eq!(b(image::nurbs_curve_length(&mut m, &mut o, 0)), Ok(F80::ZERO.to_bits()));
    assert!(o.calls.is_empty());
}

#[test]
fn length_tables_built_on_values_and_on_the_image() {
    // a straight line of length 5 (order 2, two points) and the cubic: the tables built by
    // SI_nurbsCurveLength (100 samples a unit, the touch), the same on values and on the image
    use crate::geometry;
    use crate::image;
    use si_mem::{Mem, Ptr32};
    for (mut c, keyframed) in [(curve(2, &[0.0, 1.0], vec![pt(0.0, 0.0, 0.0, 1.0), pt(3.0, 4.0, 0.0, 1.0)], 0), 0i16), (bezier_cubic(), 1)] {
        c.rec.keyframed = keyframed;
        let mut m = Mem::new();
        let knots = c.knots.clone().unwrap();
        let points = c.points.clone().unwrap();
        let ka = m.place(8 * knots.len() as u32, 8);
        for (i, k) in knots.iter().enumerate() {
            m.set_f64(ka + 8 * i as u32, *k);
        }
        let pa = m.place(0x24 * points.len() as u32, 8);
        for (i, p) in points.iter().enumerate() {
            m.write(Ptr32::new(pa + 0x24 * i as u32), *p);
        }
        // the scene: list 0, empty
        let pd = m.place(8, 4);
        let gl = m.place(0x24, 4);
        m.set_u32(image::SI_PROC_DATA, pd);
        m.set_u32(image::SI_GRAF_LISTS, gl);
        let mut r = c.rec;
        r.dir.knots = Ptr32::new(ka);
        r.points = Ptr32::new(pa);
        let crv = m.place(core::mem::size_of::<NurbsCrv>() as u32, 8);
        m.write(Ptr32::new(crv), r);
        let mut st = Statics::default();
        let mut o = si_mem::Trace::with(|_, r, _| si_mem::Ret { eax: if r.name == "SI_findNURBSPathFcurveOwner" { 0x5150 } else { 0 }, ..si_mem::Ret::NONE });
        let want = geometry::curve_length(&mut st, Some(&mut c)).unwrap();
        let got = image::nurbs_curve_length(&mut m, &mut o, crv).unwrap();
        assert_eq!(got.to_bits(), want.to_bits());
        let n = c.nb_length();
        assert!(n > 2);
        assert_eq!(m.i32(crv + 0x50), n);
        let (lp, ld) = (m.u32(crv + 0x54), m.u32(crv + 0x58));
        for i in 0..n as u32 {
            assert_eq!(m.f64(lp + 8 * i).to_bits(), c.length_param.as_ref().unwrap()[i as usize].to_bits());
            assert_eq!(m.f64(ld + 8 * i).to_bits(), c.length_dist.as_ref().unwrap()[i as usize].to_bits());
        }
        // the touch: bits 0x60 of +0x3b, 0x10 of +0x38; a keyframed curve's four calls
        assert_eq!(m.u32(crv + 0x38) & 0x6000_0010, 0x6000_0010);
        assert_eq!(c.rec.flags_38 & 0x6000_0010, 0x6000_0010);
        if keyframed == 1 {
            assert_eq!(o.names(), ["SI_findNURBSPathFcurveOwner", "SI_updateFcurveNURBSPathKeys", "SI_checkNrbCrvToFcurve", "SI_refreshFcurve"]);
            assert_eq!(o.calls[1].args, [0x5150, crv]);
        } else {
            assert!(o.calls.is_empty());
            assert!((want.to_f64() - 5.0).abs() < 1e-12, "{want:?}");
        }
        // asked again: the tables are there, nothing is built
        assert_eq!(image::nurbs_curve_length(&mut m, &mut o, crv).unwrap().to_bits(), want.to_bits());
        assert_eq!(m.u32(crv + 0x58), ld);
    }
}

/// A surface of the value form put on an image: the record with its knots and points placed
/// beside it (the record's three pointers made theirs). Returns the record's address.
fn surface_on_image(m: &mut si_mem::Mem, s: &Surface) -> u32 {
    use si_mem::Ptr32;
    let mut rec = s.rec;
    let ku = s.knots_u.as_deref().unwrap();
    let kv = s.knots_v.as_deref().unwrap();
    let ps = s.points.as_deref().unwrap();
    let a = m.place(8 * ku.len() as u32, 8);
    let b = m.place(8 * kv.len() as u32, 8);
    let p = m.place(0x24 * ps.len() as u32, 8);
    for (i, k) in ku.iter().enumerate() {
        m.set_f64(a + 8 * i as u32, *k);
    }
    for (i, k) in kv.iter().enumerate() {
        m.set_f64(b + 8 * i as u32, *k);
    }
    for (i, q) in ps.iter().enumerate() {
        m.write(Ptr32::new(p + 0x24 * i as u32), *q);
    }
    rec.u.knots = si_types::Ptr32::new(a);
    rec.v.knots = si_types::Ptr32::new(b);
    rec.points = si_types::Ptr32::new(p);
    let at = m.place(core::mem::size_of::<NurbsSrf>() as u32, 8);
    m.write(Ptr32::new(at), rec);
    at
}

#[test]
fn a_surface_copied_moved_handed_back_and_disposed_on_the_image() {
    // a 3 × 4 plane: the copy holds the same knots and points in blocks of its own; a point
    // of the copy moved (weight 0.5) is x, y, z times the weight, the point and the surface
    // marked; the source's points handed back over the copy's undo the move; the copy's
    // disposal gives back its knots, points and record, in that order, and clears the word
    use crate::image::{self, SrfHeld};
    use si_mem::{Mem, Ret, Trace};
    let s = plane(3, 4);
    let mut m = Mem::new();
    let src = surface_on_image(&mut m, &s);
    let mut o = Trace::with(|_, _, _| Ret::NONE);
    let c = image::srf_copy(&mut m, &mut o, src);
    assert_ne!(c, 0);
    let (a, b) = (SrfHeld::at(&m, src), SrfHeld::at(&m, c));
    assert_eq!((a.knots_u.clone(), a.knots_v.clone(), a.points.clone()), (b.knots_u.clone(), b.knots_v.clone(), b.points.clone()));
    assert_eq!((b.rec.u.order, b.rec.v.nb_points, b.rec.nb_points, b.rec.nb_trims), (4, 4, 12, 0));
    assert_ne!(b.rec.points.addr(), a.rec.points.addr());
    // the copy's display lines disposed of (nothing there), with the tree's destruction
    assert_eq!(o.names(), vec!["SI_genTreeDestroy"]);

    assert_eq!(image::srf_move_point(&mut m, &mut o, c, 5, 1.5, -2.0, 1e300, 0.5), 1);
    let p = SrfHeld::at(&m, c).points[5];
    assert_eq!((p.x.0, p.y.0, p.z.0, p.w.0), (0.75, -1.0, 5e299, 0.5));
    assert_eq!(p.flag as u32 & crate::misc::MOVED, crate::misc::MOVED);
    assert_eq!(m.u8(c + 0x5b) & 0x60, 0x60);
    // untrimmed: the display lines' marking, then the tree (no capping word is set)
    assert_eq!(&o.names()[1..], ["markDirtyCcrvs", "SI_genTreeDestroy"]);
    assert_eq!(o.calls[1].args, vec![c, 5]);

    image::srf_transfer_points_knots(&mut m, &mut o, src, c);
    let b = SrfHeld::at(&m, c);
    for (q, r) in a.points.iter().zip(&b.points) {
        assert_eq!((q.x.0, q.y.0, q.z.0, q.w.0, q.flag), (r.x.0, r.y.0, r.z.0, r.w.0, r.flag));
    }

    let pp = m.place(4, 4);
    m.set_u32(pp, c);
    let before = m.freed().len();
    image::srf_dispose(&mut m, &mut o, pp);
    assert_eq!(m.freed()[before..], [b.rec.u.knots.addr(), b.rec.v.knots.addr(), b.rec.points.addr(), c]);
    assert_eq!(m.u32(pp), 0);
}

#[test]
fn a_curve_copied_on_the_image_is_the_curve_copied_on_values() {
    use crate::image::{self, CrvHeld};
    use si_mem::{Mem, Ptr32};
    let c = bezier_cubic();
    let mut m = Mem::new();
    let k = m.place(8 * 6, 8);
    let p = m.place(0x24 * 4, 8);
    for (i, x) in c.knots.as_ref().unwrap().iter().enumerate() {
        m.set_f64(k + 8 * i as u32, *x);
    }
    for (i, q) in c.points.as_ref().unwrap().iter().enumerate() {
        m.write(Ptr32::new(p + 0x24 * i as u32), *q);
    }
    let mut rec = c.rec;
    rec.dir.knots = si_types::Ptr32::new(k);
    rec.points = si_types::Ptr32::new(p);
    rec.step = 7;
    let at = m.place(core::mem::size_of::<NurbsCrv>() as u32, 8);
    m.write(Ptr32::new(at), rec);
    let n = image::crv_copy(&mut m, at);
    let got = CrvHeld::at(&m, n);
    let mut c2 = c.clone();
    c2.rec.step = 7;
    let want = own::crv_copy(&c2).unwrap();
    assert_eq!(got.knots, want.knots.unwrap());
    assert_eq!(got.points, want.points.unwrap());
    assert_eq!((got.rec.dir.order, got.rec.dir.nb_points, got.rec.step, got.rec.unk_48), (4, 4, 7, 0));
    assert_ne!(got.rec.points.addr(), p);
}

use si_mem::{Mem, Ptr32};

/// A curve of the given order through `ps` (x, y, z, w) on the image, its record placed;
/// `knots` its knot vector (none: the pointer null), `param` its parametrisation.
fn curve_on_image(m: &mut Mem, order: i32, ps: &[(f64, f64, f64, f64)], knots: Option<&[f64]>, closed: i16, param: i32) -> u32 {
    let pts = m.place(ps.len() as u32 * 0x24, 8);
    for (i, &(x, y, z, w)) in ps.iter().enumerate() {
        m.write(Ptr32::new(pts + 0x24 * i as u32), pt(x, y, z, w));
    }
    let mut c = NurbsCrv::allocate();
    c.dir.order = order;
    c.dir.nb_points = ps.len() as i32;
    c.dir.closed = closed;
    c.param = param;
    if let Some(k) = knots {
        let a = m.place(8 * k.len() as u32, 8);
        for (i, v) in k.iter().enumerate() {
            m.set_f64(a + 8 * i as u32, *v);
        }
        c.dir.nb_knots = k.len() as i32;
        c.dir.knots = si_types::Ptr32::new(a);
    }
    c.points = si_types::Ptr32::new(pts);
    let crv = m.place(core::mem::size_of::<NurbsCrv>() as u32, 8);
    m.write(Ptr32::new(crv), c);
    crv
}

fn knots_of(m: &Mem, crv: u32) -> Vec<f64> {
    let (n, k) = (m.i32(crv + 8), m.u32(crv + 0xc));
    (0..n as u32).map(|i| m.f64(k + 8 * i)).collect()
}

#[test]
fn uniform_knots_are_made_on_the_image_for_a_curve_without_knots() {
    let mut m = Mem::new();
    let ps = [(0.0, 0.0, 0.0, 1.0), (1.0, 0.0, 0.0, 1.0), (2.0, 1.0, 0.0, 1.0), (3.0, 0.0, 0.0, 1.0), (4.0, 0.0, 0.0, 1.0)];
    let open = curve_on_image(&mut m, 4, &ps, None, 0, 0);
    crate::image::knots_crv_reparametrization(&mut m, open).unwrap();
    assert_eq!(knots_of(&m, open), [0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0]);
    assert_eq!((m.f64(open + 0x10), m.f64(open + 0x18)), (0.0, 2.0));
    let closed = curve_on_image(&mut m, 2, &ps[..3], None, 1, 0);
    crate::image::knots_crv_reparametrization(&mut m, closed).unwrap();
    assert_eq!(knots_of(&m, closed), [0.0, 1.0, 2.0, 3.0]);
    assert_eq!((m.f64(closed + 0x10), m.f64(closed + 0x18)), (0.0, 3.0));
    // fewer points than the order: nothing
    let short = curve_on_image(&mut m, 4, &ps[..3], None, 0, 0);
    crate::image::knots_crv_reparametrization(&mut m, short).unwrap();
    assert_eq!(m.u32(short + 0xc), 0);
}

#[test]
fn chord_knots_follow_the_distances_and_the_shortest_chord_is_kept() {
    let mut m = Mem::new();
    // chords 3, 0 (kept at 0.01), 5 (the last point of weight 2, divided by it)
    let ps = [(0.0, 0.0, 0.0, 1.0), (3.0, 0.0, 0.0, 1.0), (3.0, 0.0, 0.0, 1.0), (6.0, 4.0, 0.0, 2.0)];
    let c = curve_on_image(&mut m, 2, &ps, Some(&[9.0; 4]), 0, 1);
    crate::image::knots_crv_reparametrization(&mut m, c).unwrap();
    assert_eq!(knots_of(&m, c), [0.0, 3.0, 3.01, 3.01 + 5.0]);
    // the old knots are given back
    assert_eq!(m.freed().len(), 2);
    // parametrisation 2 keeps knots it has, moved to start at 0, the close ones merged
    let k = [1.0, 1.0000005, 2.0, 3.0];
    let c2 = curve_on_image(&mut m, 2, &ps, Some(&k), 0, 2);
    crate::image::knots_crv_reparametrization(&mut m, c2).unwrap();
    assert_eq!(knots_of(&m, c2), [0.0, 0.0, 1.0, 2.0]);
}

#[test]
fn a_curve_made_linear_on_the_image_goes_through_its_samples() {
    let mut m = Mem::new();
    let ps = [(0.0, 0.0, 0.0, 1.0), (1.0, 2.0, 0.0, 1.0), (2.0, 2.0, 0.0, 1.0), (3.0, 0.0, 0.0, 1.0)];
    let c = curve_on_image(&mut m, 4, &ps, Some(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]), 0, 0);
    m.write(Ptr32::<i32>::new(c + 0x34), 4);
    m.set_f64(c + 0x18, 1.0);
    let l = crate::image::crv_convert_to_linear(&mut m, c).unwrap();
    assert_ne!(l, 0);
    assert_eq!((m.i32(l), m.i32(l + 0x34)), (2, 1));
    let n = m.i32(l + 4);
    assert!(n >= 2);
    let p0: NurbsPoint = m.read(Ptr32::new(m.u32(l + 0x2c)));
    let pn: NurbsPoint = m.read(Ptr32::new(m.u32(l + 0x2c) + 0x24 * (n as u32 - 1)));
    assert_eq!((p0.x.0, p0.y.0, p0.w.0), (0.0, 0.0, 1.0));
    assert_eq!((pn.x.0, pn.y.0), (3.0, 0.0));
    assert_eq!(knots_of(&m, l), (0..n).map(|i| i as f64).collect::<Vec<_>>());
    // null, without points or knots: null; of order 2: a copy
    assert_eq!(crate::image::crv_convert_to_linear(&mut m, 0).unwrap(), 0);
    let bare = curve_on_image(&mut m, 4, &ps, None, 0, 0);
    assert_eq!(crate::image::crv_convert_to_linear(&mut m, bare).unwrap(), 0);
    let lin = curve_on_image(&mut m, 2, &ps, Some(&[0.0, 1.0, 2.0, 3.0]), 0, 0);
    let copy = crate::image::crv_convert_to_linear(&mut m, lin).unwrap();
    assert_eq!(knots_of(&m, copy), [0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn a_curve_reversed_on_the_image_is_the_curve_reversed_on_values() {
    let mut m = Mem::new();
    let knots = [0.0, 0.0, 0.0, 1.0, 3.0, 4.5, 4.5, 4.5];
    let ps: Vec<(f64, f64, f64, f64)> = (0..6).map(|i| (i as f64, (i * i) as f64, 0.5, 1.0 + i as f64)).collect();
    let c = curve_on_image(&mut m, 4, &ps, Some(&knots), 0, 0);
    // the index table, written where it is set
    let table = m.place(4 * 6, 4);
    m.set_u32(crate::image::SI_GBL_INDEX, table);
    let mut o = si_mem::Trace::with(|_, _, _| si_mem::Ret::NONE);
    assert_eq!(crate::image::crv_inverse(&mut m, &mut o, c), 1);
    let mut v = curve(4, &knots, ps.iter().map(|&(x, y, z, w)| pt(x, y, z, w)).collect(), 0);
    let (mut k, mut p) = (v.knots.take().unwrap(), v.points.take().unwrap());
    crate::edit::crv_inverse(&mut NoHost, &mut v.rec, &mut k, &mut p).unwrap();
    assert_eq!(knots_of(&m, c), k);
    let got: Vec<NurbsPoint> = (0..6).map(|i| m.read(Ptr32::new(m.u32(c + 0x2c) + 0x24 * i))).collect();
    assert_eq!(got, p);
    assert_eq!((m.f64(c + 0x10), m.f64(c + 0x18)), (v.rec.dir.range_start.0, v.rec.dir.range_end.0));
    // the pairs (0, 5), (1, 4), (2, 3); the scratch intervals given back; no call out
    assert_eq!((0..6).map(|i| m.i32(table + 4 * i)).collect::<Vec<_>>(), [5, 4, 3, 2, 1, 0]);
    assert_eq!(m.freed().len(), 1);
    assert!(o.calls.is_empty());
    // a keyframed curve has its path keys turned round: owner, 0, the curve
    m.set_u32(crate::image::SI_GBL_INDEX, 0);
    m.set_i16(c + 0x4c, 1);
    let mut o = si_mem::Trace::with(|_, r, _| si_mem::Ret { eax: if r.name == "SI_findNURBSPathFcurveOwner" { 0x5150 } else { 0 }, ..si_mem::Ret::NONE });
    crate::image::crv_inverse(&mut m, &mut o, c);
    assert_eq!(o.calls.iter().map(|k| (k.routine.name, k.args.clone())).collect::<Vec<_>>(), [("SI_findNURBSPathFcurveOwner", vec![c]), ("SI_invertFcurvePathKeys", vec![0x5150, 0, c])]);
    // twice reversed: the points as they were
    let back: Vec<NurbsPoint> = (0..6).map(|i| m.read(Ptr32::new(m.u32(c + 0x2c) + 0x24 * i))).collect();
    assert_eq!(back, ps.iter().map(|&(x, y, z, w)| pt(x, y, z, w)).collect::<Vec<_>>());
}
