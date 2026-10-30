// SPDX-License-Identifier: MIT
//! Compares `si-nurbs` with the original, routine by routine, on the vectors that
//! `re/tools/si-nurbs/emu_gen.py` made by stepping the original's instructions in a CPU
//! emulator (finding 0011).
//!
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck re/out/si-nurbs/vec)
//!
//! With `EMUCHECK_MUTATE=1` every comparison flips a bit of the port's value first: every
//! compared case must then be reported as different.

mod image13;
mod recs;
mod util;

use std::path::Path;

use si_nurbs::own::NoHost;
use si_nurbs::{eval, insert, knots, misc, sample, Dir};
use si_x87::vectors::{check_dir_with, print_report, Check, Verdict};
use util::*;

fn lookup(name: &str) -> Option<Check> {
    if let Some(c) = recs::lookup(name) {
        return Some(c);
    }
    Some(match name {
        // ---------------------------------------------------------------- knots
        "si_binom" => |c| match eval::binom(c.input("n").i32(), c.input("k").i32()) {
            Some(v) => same_eax(c, v as u32),
            None => refused(c, "division by zero"),
        },
        "si_nurbsInterv" => |c| {
            let (dir, k) = dir_of(c);
            let mut st = statics(c);
            match knots::interv(&mut st, Dir { rec: &dir, knots: &k }, c.input("t").f64(), c.input("open").i16()) {
                Some(v) => same_eax(c, v as u32).and(|| same_left(c, &st)),
                None => refused(c, "span search"),
            }
        },
        "si_OLDnurbsInterv" => |c| {
            let (dir, k) = dir_of(c);
            match knots::old_interv(Dir { rec: &dir, knots: &k }, c.input("t").f64()) {
                Some(v) => same_eax(c, v as u32),
                None => refused(c, "old span search"),
            }
        },
        "si_knotsInitSequence" => |c| {
            let (dir, _) = dir_of(c);
            let f = c.output("knots");
            let mut k = vec![f64::from_bits(0xdead_beef_dead_beef); f.len()];
            match knots::init_sequence(&dir, &mut k, c.input("kind").i32()) {
                Some(()) => {
                    // a knot the port left alone must be one the original did not write
                    let w = f.written();
                    let untouched = k.iter().zip(&w).all(|(g, o)| (g.to_bits() == 0xdead_beef_dead_beef) == o.is_none());
                    Verdict::from(untouched, || "not the same knots written".to_string()).and(|| same("knots", f, &f64w(&k)))
                }
                None => refused(c, "knot sequence"),
            }
        },
        "si_nurbsDirInverse" => |c| {
            let (mut dir, mut k) = dir_of(c);
            match knots::dir_inverse(&mut dir, &mut k) {
                Some(r) => same_eax(c, r as u32).and(|| same_out(c, "dir", &dir_words(&dir))).and(|| same_out(c, "knots", &f64w(&k))),
                None => refused(c, "direction reversal"),
            }
        },
        // ---------------------------------------------------------------- evaluation
        "SI_nurbsCrvValue" => |c| {
            let crv = curve(c, "");
            let mut st = statics(c);
            match eval::crv_value(&mut st, crv.view(), c.input("t").f64(), c.input("nd").i32()) {
                Some(p) => same_out(c, "ret", &f64w(&p)).and(|| same_left(c, &st)),
                None => refused(c, "curve value"),
            }
        },
        "SI_nurbsCrvValue4D" => |c| {
            let crv = curve(c, "");
            let mut st = statics(c);
            match eval::crv_value4d(&mut st, crv.view(), c.input("t").f64()) {
                Some(p) => same_out(c, "ret", &pt_words(&p)).and(|| same_left(c, &st)),
                None => refused(c, "curve value"),
            }
        },
        "SI_nurbsSrfValue" => |c| {
            let s = surface(c, "");
            let mut st = statics(c);
            match eval::srf_value(&mut st, s.view(), c.input("u").f64(), c.input("v").f64()) {
                Some(p) => same_out(c, "ret", &f64w(&p)).and(|| same_left(c, &st)),
                None => refused(c, "surface value"),
            }
        },
        "SI_nurbsSrfValue4D" => |c| {
            let s = surface(c, "");
            let mut st = statics(c);
            match eval::srf_value4d(&mut st, s.view(), c.input("u").f64(), c.input("v").f64()) {
                Some(p) => same_out(c, "ret", &pt_words(&p)).and(|| same_left(c, &st)),
                None => refused(c, "surface value"),
            }
        },
        "SI_nurbsSrfExtractCurveSeg" => |c| {
            let mut s = surface(c, "");
            let mut st = statics(c);
            let r = eval::srf_extract_curve_seg(&mut st, &mut NoHost, &mut s, c.input("u").f64(), c.input("v").f64(), c.input("swap").i32(), c.input("nd").i32(), None);
            match r {
                Some(Some(crv)) => same_curve(c, "n", &crv).and(|| same_surface(c, "o", &s)).and(|| same_left(c, &st)),
                _ => refused(c, "segment extraction"),
            }
        },
        "SI_nurbsSrfExtractCurveU" | "SI_nurbsSrfExtractCurveV" => |c| {
            let s = surface(c, "");
            let mut st = statics(c);
            let (t, nd) = (c.input("t").f64(), c.input("nd").i32());
            let r = if c.name.starts_with("SI_nurbsSrfExtractCurveU") { eval::srf_extract_curve_u(&mut st, s.view(), t, nd) } else { eval::srf_extract_curve_v(&mut st, s.view(), t, nd) };
            match r {
                Some(crv) => same_curve(c, "n", &crv).and(|| same_left(c, &st)),
                None => refused(c, "curve extraction"),
            }
        },
        "SI_nurbsSrfExtractCurve" => |c| {
            let s = surface(c, "");
            let mut st = statics(c);
            match eval::srf_extract_curve(&mut st, s.view(), c.input("t").f64(), c.input("dir").i32(), c.input("nd").i32()) {
                Some(crv) => same_curve(c, "n", &crv).and(|| same_left(c, &st)),
                None => refused(c, "curve extraction"),
            }
        },
        "SI_nurbsSrfValueAndNormal" => |c| {
            let s = surface(c, "");
            let mut st = statics(c);
            let a = (c.input("crva").len() > 1).then(|| curve(c, "a"));
            let b = (c.input("crvb").len() > 1).then(|| curve(c, "b"));
            let mut tries = [0u32; 2];
            let r = eval::srf_value_and_normal_traced(&mut st, s.view(), c.input("u").f64(), c.input("v").f64(), a.as_ref().map(|x| x.view()), b.as_ref().map(|x| x.view()), &mut tries);
            if std::env::var_os("EMUCHECK_TRACE").is_some() {
                eprintln!("normal: given {}{} tries {} {}", a.is_some() as u8, b.is_some() as u8, tries[0], tries[1]);
            }
            match r {
                Some((p, n)) => same_out(c, "ret", &f64w(&p)).and(|| same_out(c, "nrm", &f64w(&n))).and(|| same_left(c, &st)),
                None => refused(c, "value and normal"),
            }
        },
        // ---------------------------------------------------------------- sampling
        "SI_SampleKnotVector" => |c| {
            let (dir, k) = dir_of(c);
            match sample::sample_knot_vector(Some(Dir { rec: &dir, knots: &k }), true, c.input("per").i32()) {
                Some(Some(a)) => same_out(c, "cnt", &[a.len() as u64]).and(|| same_out(c, "arr", &f64w(&a))),
                _ => refused(c, "knot vector sampling"),
            }
        },
        "SI_samplesPerKnot" => |c| {
            let (dir, k) = dir_of(c);
            match sample::samples_per_knot(Dir { rec: &dir, knots: &k }, c.input("per").i32()) {
                Some((a, count, total)) => same_eax(c, total as u32)
                    .and(|| same_out(c, "cnt", &[count as u32 as u64]))
                    .and(|| same_out(c, "arr", &f64w(&a.unwrap_or_default()))),
                None => refused(c, "samples per knot"),
            }
        },
        "getKnotPosition" => |c| {
            let k = c.input("knots").f64s();
            let f = c.output("out");
            let mut out = vec![f64::from_bits(0xdead_beef_dead_beef); f.len()];
            match sample::knot_position(&mut out, &k, c.input("t").f64(), c.input("n").i32()) {
                Some(i) => same_eax(c, i as u32).and(|| same("out", f, &f64w(&out))),
                None => refused(c, "knot position"),
            }
        },
        "SI_nurbsCrvSamples" => |c| {
            let crv = curve(c, "");
            let mut st = statics(c);
            match sample::crv_samples(&mut st, crv.view(), c.input("mode").i32(), c.input("step").f64()) {
                None => Verdict::from(c.output("cnt").written()[0].is_none() && !mutate(), || "the original wrote a count".to_string()),
                Some(Some((n, pts))) => {
                    let flat: Vec<f64> = pts.iter().flatten().copied().collect();
                    same_out(c, "cnt", &[n as u32 as u64]).and(|| same_out(c, "arr", &f64w(&flat))).and(|| same_left(c, &st))
                }
                Some(None) => refused(c, "curve samples"),
            }
        },
        // ---------------------------------------------------------------- knot insertion
        "SI_nurbsKnotsCorrectMultAndValue" => |c| {
            let (dir, k) = dir_of(c);
            let (mut t, mut r) = (c.input("t").f64(), c.input("r").i32());
            match insert::knots_correct_mult_and_value(Dir { rec: &dir, knots: &k }, &mut t, c.input("left").i32(), &mut r) {
                Some(()) => same_out(c, "t", &[t.to_bits()]).and(|| same_out(c, "r", &[r as u32 as u64])),
                None => refused(c, "multiplicity correction"),
            }
        },
        "SI_nurbsCrvKnotInsert" => |c| {
            let mut crv = curve(c, "");
            let mut st = statics(c);
            let mut host = recs::Rec::new(c);
            match insert::crv_knot_insert(&mut st, &mut host, &mut crv, c.input("t").f64(), c.input("r").i32(), c.input("correct").i16()) {
                Some(r) => same_eax(c, r as u32).and(|| same_curve(c, "o", &crv)).and(|| same_left(c, &st)).and(|| recs::same_index(c, &host)),
                None => refused(c, "knot insertion"),
            }
        },
        "SI_nurbsSrfKnotInsertU" | "SI_nurbsSrfKnotInsertV" | "SI_nurbsSrfKnotInsert" => |c| {
            let mut s = surface(c, "");
            let mut st = statics(c);
            let mut host = recs::Rec::new(c);
            let (t, r) = (c.input("t").f64(), c.input("r").i32());
            let got = if c.name.starts_with("SI_nurbsSrfKnotInsertU") {
                insert::srf_knot_insert_u(&mut st, &mut host, &mut s, t, r).map(|v| v as u16 as u32)
            } else if c.name.starts_with("SI_nurbsSrfKnotInsertV") {
                insert::srf_knot_insert_v(&mut st, &mut host, &mut s, t, r).map(|v| v as u16 as u32)
            } else {
                insert::srf_knot_insert(&mut st, &mut host, &mut s, t, r, c.input("dir").i32()).map(|v| v as u32)
            };
            match got {
                // the two direction routines return 16 bits: the upper half of eax is not theirs
                Some(r) => {
                    let eax = c.output("eax").u32();
                    let want = if c.name.starts_with("SI_nurbsSrfKnotInsert") && c.try_input("dir").is_none() { eax & 0xffff } else { eax };
                    let r = if mutate() { r ^ 1 } else { r };
                    Verdict::from(want == r, || format!("result: original {want:x}, port {r:x}"))
                        .and(|| same_surface(c, "o", &s))
                        .and(|| same_left(c, &st))
                        .and(|| recs::same_index(c, &host))
                        .and(|| recs::same_calls(c))
                }
                None => refused(c, "surface knot insertion"),
            }
        },
        // ---------------------------------------------------------------- nrb_misc.c
        "SI_nrbPtsMultiplyMatrix" => |c| {
            let n = c.input("n").i32();
            if n == 0 {
                return Verdict::from(c.outputs.is_empty() && !mutate(), || "the original wrote something".to_string());
            }
            let src = pts_from(&nums(c.input("src")));
            let mut dst = pts_from(&nums(c.input("dst")));
            match misc::pts_multiply_matrix(&src, &mat(c), &mut dst, n) {
                Some(()) => same_out(c, "dst", &pts_words(&dst)),
                None => refused(c, "points through a matrix"),
            }
        },
        "SI_nurbsCrvApplyMatrix" => |c| {
            let mut crv = curve(c, "");
            let mut p = crv.points.take().unwrap_or_default();
            match misc::crv_apply_matrix(&mut crv.rec, &mut p, &mat(c), c.input("mask").i32()) {
                Some(()) => same_out(c, "crv", &crv_rec_words(&crv.rec)).and(|| same_out(c, "pts", &pts_words(&p))),
                None => refused(c, "curve through a matrix"),
            }
        },
        "SI_nurbsCrvMovePoint" => |c| {
            let mut crv = curve(c, "");
            let mut p = crv.points.take().unwrap_or_default();
            match misc::crv_move_point(&mut crv.rec, &mut p, c.input("i").i32(), c.input("x").f64(), c.input("y").f64(), c.input("z").f64()) {
                Some(r) => same_eax(c, r as u32).and(|| same_out(c, "crv", &crv_rec_words(&crv.rec))).and(|| same_out(c, "pts", &pts_words(&p))),
                None => refused(c, "move point"),
            }
        },
        "SI_nurbsCrvTransferPoints" | "SI_nurbsCrvTransferPointsKnots" => |c| {
            let a = curve(c, "");
            let mut b = curve(c, "2");
            let (mut k, mut p) = (b.knots.take().unwrap_or_default(), b.points.take().unwrap_or_default());
            let av = a.view();
            let r = if c.name.starts_with("SI_nurbsCrvTransferPointsKnots") {
                misc::crv_transfer_points_knots(av.rec, av.knots, av.points, &mut b.rec, &mut k, &mut p)
            } else {
                misc::crv_transfer_points(av.rec, av.points, &mut b.rec, &mut p)
            };
            match r {
                Some(()) => same_out(c, "crv2", &crv_rec_words(&b.rec)).and(|| same_out(c, "knots2", &f64w(&k))).and(|| same_out(c, "pts2", &pts_words(&p))),
                None => refused(c, "transfer"),
            }
        },
        "SI_nurbsTransferDirKnots" => |c| {
            let src = dir_from(&nums(c.input("src")));
            let mut dst = dir_from(&nums(c.input("dst")));
            let (ks, mut kd) = (c.input("ks").f64s(), c.input("kd").f64s());
            match misc::transfer_dir_knots(&src, &ks, &mut dst, &mut kd) {
                Some(()) => same_out(c, "dst", &dir_words(&dst)).and(|| same_out(c, "kd", &f64w(&kd))),
                None => refused(c, "transfer"),
            }
        },
        "SI_nurbsSrfTransferPoints" | "SI_nurbsSrfTransferPointsKnots" => |c| {
            let a = surface(c, "");
            let mut b = surface(c, "2");
            let mut host = recs::Rec::new(c);
            let r = if c.name.starts_with("SI_nurbsSrfTransferPointsKnots") {
                misc::srf_transfer_points_knots(&mut host, &a, &mut b)
            } else {
                let av = a.view();
                misc::srf_transfer_points(av.rec, av.points, &mut b.rec, b.points.as_deref_mut().unwrap_or(&mut []))
            };
            match r {
                Some(()) => same_out(c, "srf2", &srf_rec_words(&b.rec))
                    .and(|| same_out(c, "ku2", &f64w(b.knots_u.as_deref().unwrap_or(&[]))))
                    .and(|| same_out(c, "kv2", &f64w(b.knots_v.as_deref().unwrap_or(&[]))))
                    .and(|| same_out(c, "pts2", &pts_words(b.points.as_deref().unwrap_or(&[]))))
                    .and(|| recs::same_calls(c)),
                None => refused(c, "transfer"),
            }
        },
        "SI_nurbsCrvGetCtrlVertex" | "SI_nurbsSrfGetCtrlVertex" => |c| {
            let i = c.input("i").i32();
            let r = if c.name.starts_with("SI_nurbsCrv") {
                let crv = curve(c, "");
                misc::crv_get_ctrl_vertex(&crv.rec, crv.view().points, i)
            } else {
                let s = surface(c, "");
                misc::srf_get_ctrl_vertex(&s.rec, s.view().points, i)
            };
            match r {
                Some(Ok(v)) => same_eax(c, 0).and(|| same_out(c, "out", &[v.x.0.to_bits(), v.y.0.to_bits(), v.z.0.to_bits(), v.w.0.to_bits(), v.flag as u32 as u64])),
                Some(Err(e)) => same_eax(c, e as u32).and(|| Verdict::from(c.output("out").written().iter().all(|w| w.is_none()), || "the original wrote the vertex".to_string())),
                None => refused(c, "control vertex"),
            }
        },
        _ => return None,
    })
}

fn mat(c: &si_x87::vectors::Case) -> [[f32; 4]; 4] {
    let f = c.input("m").f32s();
    core::array::from_fn(|i| core::array::from_fn(|j| f[4 * i + j]))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--image <directory>`: the vectors of the second form on the image and of the
    // conversion to triangles (gen_convert.py; module `image13`)
    let (dir, on_image) = match args.as_slice() {
        [flag, d] if flag == "--image" => (d.clone(), true),
        [d] => (d.clone(), false),
        _ => {
            eprintln!("usage: nurbs-emucheck [--image] <directory of vector files>");
            std::process::exit(2);
        }
    };
    if on_image {
        let child = std::thread::Builder::new().stack_size(256 << 20).spawn(move || {
            let rows = check_dir_with(Path::new(&dir), &image13::lookup, false).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2);
            });
            print_report(&rows)
        });
        let ok = child.unwrap().join().unwrap_or(false);
        if std::env::var_os("EMUCHECK_TRACE").is_some() {
            let n = |i: usize| si_nurbs::convert::SEARCHES[i].load(std::sync::atomic::Ordering::Relaxed);
            println!("searches for a vanished derivative in the port's conversions: across {}, along {}; tries that extracted anew {}", n(0), n(1), n(2));
        }
        std::process::exit(if ok { 0 } else { 1 });
    }
    let rows = check_dir_with(Path::new(&dir), &lookup, true).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let ok = print_report(&rows);
    println!(
        "of the skipped cases, {} are inputs marked hostile that the port refuses; the others did not return in the original \
         (STOP) or are undefined in part and agree in every defined word (UNINIT)",
        REFUSED.load(std::sync::atomic::Ordering::Relaxed)
    );
    std::process::exit(if ok { 0 } else { 1 });
}
