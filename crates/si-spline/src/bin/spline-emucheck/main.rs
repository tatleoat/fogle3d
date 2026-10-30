// SPDX-License-Identifier: MIT
//! Compares `si-spline` with the original, routine by routine, on the vectors that
//! `re/tools/si-spline/emu_gen.py` made by stepping the original's instructions in a CPU
//! emulator (finding 0011).
//!
//!   (ulimit -v 2000000; target/si-spline/release/spline-emucheck re/out/si-spline/vec)
//!
//! With `EMUCHECK_MUTATE=1` every comparison flips a bit of the port's value first: every
//! compared case must then be reported as different.

mod fitk;
mod imagek;
mod imgchecks;
mod pat;
mod patchk;
mod util;

use std::path::Path;

use si_math::types::Matrix;
use si_spline::{basis, convert, edit, eval, length, tools, Vector};
use si_x87::vectors::{check_dir_with, print_report, Case, Check, Verdict};
use util::*;

fn flat(m: &Matrix) -> Vec<u32> {
    m.iter().flatten().map(|f| f.to_bits()).collect()
}

fn v3(c: &Case, name: &str) -> Vector {
    let f = c.input(name).f32s();
    Vector { x: f[0], y: f[1], z: f[2] }
}

fn mat(c: &Case, name: &str) -> Matrix {
    let f = c.input(name).f32s();
    core::array::from_fn(|i| core::array::from_fn(|j| f[4 * i + j]))
}

fn lookup(name: &str) -> Option<Check> {
    // the routines of the third pass exist on the image only; with SPLINE_IMAGE=1 the
    // earlier passes' vectors go through the image forms of the routines that have one
    if let Some(check) = imgchecks::lookup(name) {
        return Some(check);
    }
    if std::env::var_os("SPLINE_IMAGE").is_some() {
        return imgchecks::lookup_old(name);
    }
    Some(match name {
        // ---------------------------------------------------------------- basis
        "SI_matrixBezier" => |c| same_out(c, "m", &flat(&basis::matrix_bezier())),
        "si_matrixBspline" => |c| same_out(c, "m", &flat(&basis::matrix_bspline())),
        // the linear routine clears all sixteen floats and then sets three
        "si_matrixLinear" => |c| same_out(c, "m", &flat(&basis::matrix_linear())),
        "SI_matrixCardinal" | "si_matrixCardinal" => |c| same_out(c, "m", &flat(&basis::matrix_cardinal(c.input("t").f32()))),
        "SI_splineBasisMatrix" => |c| {
            // the port's "untouched" is checked against the original's unwritten words
            let kind = c.input("kind").i32();
            let mut m = [[f32::from_bits(0xdead_beef); 4]; 4];
            basis::spline_basis_matrix(kind, c.input("t").f32(), &mut m);
            let f = c.output("m");
            let untouched = f.written().iter().all(|w| w.is_none());
            if !(0..=3).contains(&kind) {
                return Verdict::from(untouched && m[0][0].to_bits() == 0xdead_beef && !mutate(), || "an unknown type wrote the matrix".to_string());
            }
            same("m", f, &flat(&m))
        },
        "SI_matrixPrecision" => |c| same_out(c, "m", &flat(&basis::matrix_precision(c.input("n").i32()))),
        "SI_splineNbSegmentStep" => |c| {
            let (seg, step) = eval::nb_segment_step(c.input("kind").i32(), c.input("nb").i32(), c.input("closed").i16());
            same_out(c, "seg", &[seg as u32]).and(|| same_out(c, "step", &[step as u32]))
        },
        "SI_splineUpdateMatrix" => |c| {
            let mut s = curve(c, "");
            basis::update_matrix(&mut s);
            same_curve(c, "", "o", &s)
        },
        "SI_splineNbSegment" => |c| same_eax(c, eval::nb_segment(&curve(c, "")) as u32),
        // ---------------------------------------------------------------- evaluation
        "splineFillGeometry" => |c| {
            let s = curve(c, "");
            let mut u = c.input("u").f32();
            match eval::fill_geometry(&s, &mut u) {
                None => same_ax(c, 0).and(|| same_out(c, "u", &[c.input("u").u32()])),
                Some(g) => same_ax(c, 1).and(|| same_out(c, "u", &[u.to_bits()])).and(|| same_out(c, "g", &flat(&g))),
            }
        },
        "SI_splineValue" => |c| {
            let (p, r) = eval::value(&curve(c, ""), c.input("u").f32());
            same_eax(c, r as u32).and(|| same_out(c, "out", &vw(p)))
        },
        "SI_splineTangent" => |c| match eval::tangent(&curve(c, ""), c.input("u").f32()) {
            Some(p) => same_eax(c, 1).and(|| same_out(c, "out", &vw(p))),
            None => same_eax(c, 0).and(|| Verdict::from(c.output("out").written().iter().all(|w| w.is_none()), || "the original wrote the tangent".into())),
        },
        "SI_splineAcceleration" => |c| match eval::acceleration(&curve(c, ""), c.input("u").f32()) {
            Some(p) => same_eax(c, 1).and(|| same_out(c, "out", &vw(p))),
            None => same_eax(c, 0).and(|| Verdict::from(c.output("out").written().iter().all(|w| w.is_none()), || "the original wrote the result".into())),
        },
        "splineSetEnds" => |c| same_out(c, "out", &vw(edit::set_ends(&v3(c, "a"), &v3(c, "b"), &v3(c, "c")))),
        "SI_splineSetDummy" => |c| {
            let mut s = curve(c, "");
            edit::set_dummy(&mut s);
            same_curve(c, "", "o", &s)
        },
        "SI_splineTouch" => |c| {
            let mut s = curve(c, "");
            let mut h = Rec::new();
            edit::touch(&mut s, &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        // ---------------------------------------------------------------- length
        "SI_splineUpdateLength" => |c| {
            let mut s = curve(c, "");
            let mut h = Rec::new();
            length::update_length(&mut s, c.input("samples").i16(), c.input("touch").i16(), &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        "SI_splineLength" => |c| {
            let mut s = curve(c, "");
            let mut h = Rec::new();
            let l = length::length(&mut s, &mut h);
            same_st0(c, si_x87::x(l)).and(|| same_curve(c, "", "o", &s))
        },
        "SI_splineDistanceToFactor" => |c| {
            let mut s = curve(c, "");
            let mut h = Rec::new();
            let r = length::distance_to_factor(&mut s, c.input("d").f32(), &mut h);
            same_st0(c, r).and(|| same_curve(c, "", "o", &s))
        },
        "SI_splineFactorToDistance" => |c| {
            let mut s = curve(c, "");
            let mut h = Rec::new();
            let r = length::factor_to_distance(&mut s, c.input("u").f32(), &mut h);
            same_st0(c, r).and(|| same_curve(c, "", "o", &s))
        },
        // ---------------------------------------------------------------- editing
        "SI_splineChangePoint" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            edit::change_point(&mut s, c.input("i").i32(), &v3(c, "p"), &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        "SI_splineAddPoint" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let at = edit::add_point(&mut s, c.input("i").i32(), &v3(c, "p"), &mut h);
            let Some((_, off)) = ptr_out(c) else { return Verdict::Different("no pointer returned".into()) };
            Verdict::from(off as usize == at * 0x30 && !mutate(), || format!("returned key: original +{off:#x}, port {at}"))
                .and(|| same_curve(c, "", "o", &s))
                .and(|| same_calls(c))
                .and(|| same_index(c, &h))
        },
        "SI_splineDeleteSelection" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let r = edit::delete_selection(&mut s, &mut h);
            let al = c.output("al").values()[0] as u8;
            Verdict::from(al == r as u8 && !mutate(), || format!("result: original {al}, port {r}"))
                .and(|| same_curve(c, "", "o", &s))
                .and(|| same_calls(c))
                .and(|| same_index(c, &h))
        },
        "SI_splineDeletePoint" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let r = edit::delete_point(&mut s, c.input("i").i32(), &mut h);
            same_eax(c, r as u32).and(|| same_curve(c, "", "o", &s)).and(|| same_calls(c)).and(|| same_index(c, &h))
        },
        "SI_allDummy" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            edit::all_dummy(&mut s, &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        "SI_splineInverse" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            edit::inverse(&mut s, &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c)).and(|| same_index(c, &h))
        },
        "SI_splineMultiplyDefinition" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            edit::multiply_definition(&mut s, &mat(c, "m"), &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        "SI_splineTransform" => |c| {
            let mut s = curve(c, "");
            edit::transform(&mut s, &mat(c, "m"));
            same_curve(c, "", "o", &s)
        },
        "SI_splineTransfer" => |c| {
            let mut to = curve(c, "2");
            edit::transfer(&curve(c, ""), &mut to);
            same("spl2", c.output("spl2"), &rec_words(&to.rec)).and(|| same_keys(c, "o2.keys", &to))
        },
        "SI_splineTransferPoints" => |c| {
            let mut to = curve(c, "2");
            edit::transfer_points(&curve(c, ""), &mut to);
            same("spl2", c.output("spl2"), &rec_words(&to.rec)).and(|| same_keys(c, "o2.keys", &to))
        },
        "SI_splineCopy" => |c| {
            let mut counter = 0;
            let n = edit::copy(&curve(c, ""), &mut counter);
            let name = c.output("n.name").to_bytes();
            Verdict::from(Some(&name) == n.name.as_ref() && !mutate(), || format!("name: original {name:?}, port {:?}", n.name))
                .and(|| same_new_rec(c, "n.rec", &n.rec))
                .and(|| same_keys(c, "n.keys", &n))
        },
        "SI_splineDispose" => |c| {
            // nothing is left to compare but the order of the frees and the cleared pointer
            let Some(spl) = c.try_input("spl") else {
                return Verdict::from(c.try_output("freed").is_none() && !mutate(), || "a null pointer freed something".into());
            };
            let w = nums(spl);
            let mut want = Vec::new();
            for (i, name) in [(8, "keys"), (33, "len"), (0, "nm")] {
                if w[i] != Some(0) {
                    want.push(name.to_string());
                }
            }
            want.push("spl".to_string());
            let got: Vec<String> = c.output("freed").words().iter().map(|w| match w {
                si_x87::vectors::Word::Ref { name, .. } => name.clone(),
                _ => "?".to_string(),
            }).collect();
            let mut s = Some(curve(c, ""));
            edit::dispose(&mut s);
            Verdict::from(want == got && s.is_none() && !mutate(), || format!("freed: original {got:?}, expected {want:?}"))
                .and(|| same_out(c, "pp", &[0]))
        },
        "SI_splineKeyAllocate" => |c| {
            let keys = edit::key_allocate(c.input("n").i32());
            let null = ptr_out(c).is_none();
            if null != (keys.is_empty() ^ mutate()) {
                return Verdict::Different(format!("block: original null {null}, port {} records", keys.len()));
            }
            match c.try_output("heap0") {
                None => Verdict::from(keys.is_empty() && !mutate(), || "the original allocated nothing".into()),
                Some(f) => {
                    let w: Vec<u32> = keys.iter().flat_map(|k| {
                        let kw = key_words(k);
                        let mut v = kw[..10].to_vec();
                        v.push(kw[10] | kw[11] << 16);
                        v.push(kw[12] | kw[13] << 16);
                        v
                    }).collect();
                    same("heap0", f, &w)
                }
            }
        },
        "SI_bezierGetPoint" => |c| {
            let p = c.input("p").i32();
            if p < -2 {
                return Verdict::Skip; // a key before the block: other memory in the original
            }
            match edit::bezier_get_point(&curve(c, ""), p) {
                Some(v) => same_out(c, "out", &vw(v)),
                None => Verdict::from(c.output("out").written().iter().all(|w| w.is_none()) && !mutate(), || "the original wrote the vector".into()),
            }
        },
        "si_splineGetType" => |c| same_eax(c, edit::get_type(&c.input("name").to_bytes()) as u32),
        // ---------------------------------------------------------------- conversions
        "splineConvertToLinearRegular" => |c| {
            let mut n = 0;
            same_new(c, &convert::convert_to_linear_regular(&curve(c, ""), c.input("steps").i32(), &mut n))
        },
        "splineConvertToLinearFlatness" => |c| {
            let mut n = 0;
            let mut h = host(c);
            same_new(c, &convert::convert_to_linear_flatness(&curve(c, ""), c.input("tol").f32(), &mut n, &mut h))
        },
        "SI_splineConvertToLinear" => |c| {
            let mut n = 0;
            let mut h = host(c);
            let r = convert::convert_to_linear(&curve(c, ""), c.input("regular").i16(), c.input("steps").i32(), c.input("tol").f32(), &mut n, &mut h);
            same_new(c, &r)
        },
        "SI_splineConvertToBezier" => |c| {
            let mut n = 0;
            same_new(c, &convert::convert_to_bezier(&curve(c, ""), &mut n))
        },
        "SI_bezierExpand" => |c| {
            let mut n = 0;
            same_new(c, &convert::bezier_expand(&curve(c, ""), &mut n))
        },
        "SI_bezierShrink" => |c| {
            let mut n = 0;
            same_new(c, &convert::bezier_shrink(&curve(c, ""), &mut n))
        },
        "SI_splineCleanup" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            convert::cleanup(&mut s, &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c)).and(|| same_index(c, &h))
        },
        "splineSubdivision" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let grows = s.rec.kind == 1 && s.nb() % 16 == 0;
            let r = convert::subdivision(&mut s, c.input("seg").i32(), c.input("t").f32(), &mut h);
            if grows && r {
                // the original writes two handles into the block it has just reallocated
                // away (see the port's comment); the emulator's allocator always moves
                return Verdict::Skip;
            }
            same_eax(c, r as u32).and(|| same_curve(c, "", "o", &s)).and(|| same_calls(c))
        },
        // ---------------------------------------------------------------- tools
        "SI_solveCubic" => |c| {
            let f = c.input("c").f32s();
            let mut coef = [f[0], f[1], f[2], f[3]];
            match tools::solve_cubic(&mut coef, c.input("t").f32()) {
                None => Verdict::Skip, // the original returns what a register happened to hold
                Some((r, n)) => {
                    // only the first n roots are written
                    let want = nums(c.output("r"));
                    let written = want.iter().filter(|w| w.is_some()).count();
                    same_eax(c, n as u32)
                        .and(|| same_out(c, "c", &f32w(&coef)))
                        .and(|| Verdict::from(written == n as usize, || format!("roots written: original {written}, port {n}")))
                        .and(|| same_out(c, "r", &f32w(&r)))
                }
            }
        },
        "SI_splineRandomize" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let mut g = si_spline::patch_edit::Drand48(0x1234_ABCD_330E);
            tools::randomize(&mut s, &v3(c, "amp"), c.input("count").i16(), c.input("tagged").i16(), &mut g, &mut h);
            same_curve(c, "", "o", &s).and(|| same_calls(c))
        },
        "SI_bezierMovePoint" => |c| {
            let mut s = curve(c, "");
            let mut h = host(c);
            let r = tools::bezier_move_point(&mut s, c.input("point").i32(), &v3(c, "p"), &mut h);
            same_eax(c, r as u32).and(|| same_curve(c, "", "o", &s)).and(|| same_calls(c))
        },
        "SI_bezierMerge" => |c| {
            let mut n = 0;
            let mut h = host(c);
            let (a, b) = (curve(c, ""), curve(c, "2"));
            if a.nb() == 0 && c.input("mode").i16() == 2 {
                return Verdict::Skip; // the original reads the record before the first spline's keys
            }
            let got = tools::bezier_merge(&a, &b, c.input("mode").i16(), &mut n, &mut h);
            let off = match got.as_ref().and_then(|g| g.1) {
                Some(v) => same_out(c, "off", &vw(v)),
                None => Verdict::from(c.output("off").written().iter().all(|w| w.is_none()) && !mutate(), || "the original wrote the offset".into()),
            };
            same_new(c, &got.map(|g| g.0)).and(|| off).and(|| same_index(c, &h)).and(|| same_index2(c, &h))
        },
        "SI_splineMerge" => |c| {
            let mut n = 0;
            let mut h = host(c);
            let (a, b) = (curve(c, ""), curve(c, "2"));
            if (a.rec.kind == 2 || a.rec.kind == 3) && a.nb() < 3 {
                return Verdict::Skip; // the original writes before the block it has just allocated
            }
            if !(0..=3).contains(&a.rec.kind) {
                // the new spline's matrix is made from an uninitialised local, which here
                // holds what the allocator's calls left on the stack: the same in both runs
                return Verdict::Skip;
            }
            let got = tools::merge(&a, &b, c.input("blend").f32(), &mut n, &mut h);
            same_new(c, &got).and(|| same_index2(c, &h))
        },
        _ => return patchk::lookup(name).or_else(|| fitk::lookup(name)),
    })
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: spline-emucheck <directory of .vec files>");
        std::process::exit(2);
    });
    let rows = check_dir_with(Path::new(&dir), &lookup, true).unwrap_or_else(|e| {
        eprintln!("spline-emucheck: {e}");
        std::process::exit(2);
    });
    std::process::exit(if print_report(&rows) { 0 } else { 1 });
}
