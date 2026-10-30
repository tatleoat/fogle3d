// SPDX-License-Identifier: MIT
//! The checks of the curve-fitting helpers (`fitCurve.c`): points are three doubles.

use si_spline::fit::{self, DblPnt};
use si_x87::vectors::{Case, Check, Field, Verdict};
use si_x87::F80;

use crate::util::{mutate, same_out, same_st0};

fn pts(c: &Case, name: &str) -> Vec<DblPnt> {
    c.input(name).f64s().chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect()
}

fn pt(c: &Case, name: &str) -> DblPnt {
    let f = c.input(name).f64s();
    [f[0], f[1], f[2]]
}

fn four(c: &Case, name: &str) -> [DblPnt; 4] {
    let p = pts(c, name);
    [p[0], p[1], p[2], p[3]]
}

/// Doubles against a field of 64-bit words; an unwritten word is not compared.
fn same64(name: &str, f: &Field, got: &[f64]) -> Verdict {
    let want = f.written();
    if want.len() != got.len() {
        return Verdict::Different(format!("{name}: the original has {} doubles, the port {}", want.len(), got.len()));
    }
    let mut flip = mutate();
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let Some(w) = w else { continue };
        let mut g = g.to_bits();
        if flip {
            g ^= 1;
            flip = false;
        }
        if *w as u64 != g {
            return Verdict::Different(format!("{name}[{i}]: original {:016x}, port {g:016x}", *w as u64));
        }
    }
    Verdict::Same
}

fn out64(c: &Case, name: &str, got: &[f64]) -> Verdict {
    match c.try_output(name) {
        Some(f) => same64(name, f, got),
        None => Verdict::Different(format!("the case has no output `{name}`")),
    }
}

fn point_or_skip(c: &Case, got: Option<DblPnt>) -> Verdict {
    match got {
        Some(p) => out64(c, "ret", &p),
        None => Verdict::Skip,
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "v2AddII" => |c| out64(c, "ret", &fit::v2_add(pt(c, "a"), pt(c, "b"))),
        "v2ScaleII" => |c| out64(c, "ret", &fit::v2_scale(pt(c, "a"), c.input("s").f64())),
        "v2Sub" => |c| out64(c, "ret", &fit::v2_sub(pt(c, "a"), pt(c, "b"))),
        "dvectorCosAngle" => |c| same_st0(c, fit::dvector_cos_angle(&pt(c, "a"), &pt(c, "b"))),
        "dpointsCosAngle" => |c| same_st0(c, fit::dpoints_cos_angle(&pt(c, "a"), &pt(c, "b"), &pt(c, "c"))),
        "bezMult0" => |c| same_st0(c, fit::bez_mult0(c.input("t").f64())),
        "bezMult1" => |c| same_st0(c, fit::bez_mult1(c.input("t").f64())),
        "bezMult2" => |c| same_st0(c, fit::bez_mult2(c.input("t").f64())),
        "bezMult3" => |c| same_st0(c, fit::bez_mult3(c.input("t").f64())),
        "bezierValue" => |c| point_or_skip(c, fit::bezier_value(c.input("degree").i32(), &pts(c, "v"), c.input("t").f64())),
        "computeLeftTangent" => |c| point_or_skip(c, fit::compute_left_tangent(&pts(c, "d"), c.input("end").i32())),
        "computeRightTangent" => |c| point_or_skip(c, fit::compute_right_tangent(&pts(c, "d"), c.input("end").i32())),
        "computeCenterTangent" => |c| point_or_skip(c, fit::compute_center_tangent(&pts(c, "d"), c.input("end").i32())),
        "chordLengthParameterize" => |c| match fit::chord_length_parameterize(&pts(c, "d"), c.input("first").i32(), c.input("last").i32()) {
            Some(u) => out64(c, "heap0", &u),
            None => Verdict::Skip,
        },
        "computeMaxError" => |c| {
            match fit::compute_max_error(&pts(c, "d"), c.input("first").i32(), c.input("last").i32(), &four(c, "bez"), &c.input("u").f64s()) {
                Some((err, split)) => same_st0(c, F80::from_f64(err)).and(|| same_out(c, "split", &[split as u32])),
                None => Verdict::Skip,
            }
        },
        "newtonRaphsonRootFind" => |c| same_st0(c, fit::newton_raphson_root_find(&four(c, "q"), pt(c, "p"), c.input("u").f64())),
        "reparameterize" => |c| {
            match fit::reparameterize(&pts(c, "d"), c.input("first").i32(), c.input("last").i32(), &c.input("u").f64s(), &four(c, "bez")) {
                Some(u) => out64(c, "heap0", &u),
                None => Verdict::Skip,
            }
        },
        "generateBezier" => |c| {
            let got = fit::generate_bezier(&pts(c, "d"), c.input("first").i32(), c.input("last").i32(), &c.input("u").f64s(), pt(c, "t1"), pt(c, "t2"));
            match got {
                Some(b) => out64(c, "heap0", &b.iter().flatten().copied().collect::<Vec<f64>>()),
                None => Verdict::Skip,
            }
        },
        _ => return None,
    })
}
