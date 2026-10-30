// SPDX-License-Identifier: MIT
//! The image forms' checks: which buffers a routine's cases have and how the port is
//! called. `lookup` is for the routines that exist only on the image (the third pass);
//! `lookup_old` runs the earlier passes' vector files through the image forms of routines
//! that exist in both (`SPLINE_IMAGE=1`).

use si_mem::{Mem, Names, Ptr32};
use si_spline::{contour_image as ci, convert_image as cv, fit_image as fi, intersect_image as ii, merge_image as mi, revolution_image as ri, patch_eval_image as pe, patch_image as pi, spline_image as si, tools_image as ti, zip_image as zi};
use si_x87::vectors::{Case, Check, Field, Verdict};

use crate::imagek::{check, Ret};

/// The buffers of a made-up spline and of the scene's empty object list.
const SPL: &[&str] = &["spl", "keys", "len", "gl", "glist", "gi", "idx"];
/// The spline a fit builds, and the fit's statics.
const CBEZ: &[&str] = &["bez", "d", "g0881b620", "cbez", "g0881b208", "keys"];
/// A source spline, the scene's empty object list and the fit's statics, as `SI_fitCurve`
/// has them.
const FIT: &[&str] = &["spl", "keys", "len", "gl", "glist", "g0881b620", "g0881b208", "g086d6960"];

/// `o.keys` and `o.len` of the earlier passes' vectors: the keys in use and the length
/// table, read through the record's pointers; `o.name`: the new spline's name.
fn spline_post(m: &Mem, names: &Names, c: &Case, f: &Field) -> Option<Verdict> {
    let words: Vec<si_mem::Word> = f
        .words()
        .iter()
        .map(|w| match w {
            si_x87::vectors::Word::Value { bytes, value } => si_mem::Word::Value { bytes: *bytes, value: *value },
            si_x87::vectors::Word::Unwritten { bytes } => si_mem::Word::Unwritten { bytes: *bytes },
            si_x87::vectors::Word::Ref { name, offset } => si_mem::Word::Ref { name: name.clone(), offset: *offset },
        })
        .collect();
    // whose arrays: `o` the spline of the case, `o2` its second one, `n` the returned one
    let (who, what) = f.name.split_once('.')?;
    let returned = || match &c.output("ptr").words()[0] {
        si_x87::vectors::Word::Ref { name, offset } => names.addr(m, name).map(|a| a + offset),
        _ => None,
    };
    let spl = match (who, what) {
        ("o", "name") | ("n", _) => match returned() {
            Some(a) => a,
            None => return Some(Verdict::Same),
        },
        ("o", _) => names.addr(m, "spl")?,
        ("o2", _) => names.addr(m, "spl2")?,
        _ => return None,
    };
    let at = match what {
        "keys" => m.u32(spl + 0x14),
        "len" => m.u32(spl + 0x6c),
        "name" if who == "n" => {
            let want = f.to_bytes();
            let got = m.cstr(m.u32(spl));
            return Some(Verdict::from(want == got, || format!("name: original {want:?}, port {got:?}")));
        }
        "name" => m.u32(spl),
        "rec" => return Some(crate::util::same_new_rec(c, &f.name, &m.read(Ptr32::<si_types::geom::Spline>::new(spl)))),
        _ => return None,
    };
    if words.is_empty() {
        return Some(Verdict::Same);
    }
    // a length table the routine left where it was is dumped by the key count after the
    // call, which may reach past the case's buffer: that far is not the case's memory
    let mut words = words;
    let table = match who {
        "o" => "len",
        "o2" => "len2",
        _ => "",
    };
    if what == "len" && names.addr(m, table) == Some(at) {
        words.truncate(c.try_input(table).map_or(0, |f| f.words().len()));
    }
    Some(match m.compare_words(at, &words, names) {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(format!("{}: {e}", f.name)),
    })
}

/// The buffers of a made-up patch.
const PAT: &[&str] = &["pat", "pts", "tags", "s0", "m0", "s1", "m1", "d0", "d1", "table"];

/// A patch type outside 0..=3 in either direction: the routines then work on locals
/// nobody set, which hold other values here than in the emulator.
fn odd_type(c: &Case, buffer: &str) -> bool {
    let w = c.input(buffer).words();
    let kind = |i: usize| w[i].value().map_or(0, |v| v as u32);
    kind(1) > 3 || kind(2) > 3
}

/// A source patch and the two parts of a cut, each with its arrays.
const PAT3: &[&str] = &[
    "pat", "pata", "patb", "fa", "fb", "ta", "tb", "pts", "tags", "s0", "m0", "s1", "m1", "d0", "d1", "ptsa", "tagsa", "s0a", "m0a", "s1a", "m1a", "d0a", "d1a", "ptsb", "tagsb", "s0b", "m0b", "s1b",
    "m1b", "d0b", "d1b",
];

/// The scene of an `SI_splineIsPath` case: the model, the record `SI_procData` leads to,
/// the object lists, up to four graf nodes with their objects and deformation records.
const SCENE: &[&str] = &[
    "mdl", "other", "g08782108", "prec", "gl", "glist", "n0", "n1", "n2", "n3", "o0", "w0", "o1", "w1", "o2", "w2", "o3", "w3", "fa0", "fb0", "fa1", "fb1", "fa2", "fb2", "fa3", "fb3", "g0867931c", "g08781680",
];

/// A source spline and the two parts of a cut.
const SPL3: &[&str] = &["spl", "spla", "splb", "fa", "fb", "keys", "len", "keysa", "lena", "keysb", "lenb", "gi", "idx", "gi2", "idx2"];

/// Two models with their patches and the four index tables of a merge.
const MERGE: &[&str] = &[
    "models", "mdla", "mdlb", "pat", "patb", "pts", "tags", "s0", "m0", "s1", "m1", "d0", "d1", "ptsb", "tagsb", "s0b", "m0b", "s1b", "m1b", "d0b", "d1b", "ta0", "ta1", "tb0", "tb1",
];

/// A spline of a sample model file as si-hrc's image readers leave it (fifth pass), the
/// empty object list and the allocator's counter.
const SAMPLE: &[&str] = &["spl", "keys", "nm", "gl", "glist", "g086d6960"];
/// A patch of a sample model file, with the evaluators' outputs.
const SAMPLE_PAT: &[&str] = &["pat", "pts", "tags", "out", "nu", "nv", "p", "tu", "tv", "n"];

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_splineConvertToLinear#sample" | "SI_splineConvertToLinear#flat" => |c| {
            check(c, SAMPLE, None, &|m, o, a| Ret::Int(cv::spline_convert_to_linear(m, o, Ptr32::new(a.p("spl")), a.i("regular") as i16, a.i("steps"), a.f32("tol")).addr()))
        },
        "SI_splineCleanup#sample" => |c| {
            check(c, SAMPLE, None, &|m, o, a| {
                cv::spline_cleanup(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_splineInverse#sample" => |c| {
            check(c, SAMPLE, None, &|m, o, a| {
                cv::spline_inverse(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_patchSurface#sample" => |c| {
            check(c, SAMPLE_PAT, None, &|m, _, a| {
                pe::patch_surface(m, Ptr32::new(a.p("pat")), a.p("out"), a.p("nu"), a.p("nv"));
                Ret::None
            })
        },
        "SI_patchUpdateMatrix#sample" => |c| {
            check(c, SAMPLE_PAT, None, &|m, _, a| {
                pe::patch_update_matrix(m, Ptr32::new(a.p("pat")));
                Ret::None
            })
        },
        "SI_patchValue#sample" => |c| {
            check(c, SAMPLE_PAT, None, &|m, _, a| {
                pe::patch_value(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("out"));
                Ret::None
            })
        },
        "SI_patchValueAndTangent#sample" => |c| check(c, SAMPLE_PAT, None, &|m, _, a| Ret::Int(pe::patch_value_and_tangent(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("p"), a.p("tu"), a.p("tv")) as u32)),
        "SI_patchValueAndNormal#sample" => |c| check(c, SAMPLE_PAT, None, &|m, _, a| Ret::Int(pe::patch_value_and_normal(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("p"), a.p("n")) as u32)),
        "simplifyContour" => |c| {
            check(c, &["pts", "out", "fl", "nout", "g086d3710", "g086d3718"], None, &|m, _, a| {
                ci::simplify_contour(m, a.p("pts"), a.i("n"), a.p("out"), a.p("fl"), a.p("nout"));
                Ret::None
            })
        },
        "patchToMeshSetPolygonUV" => |c| {
            check(c, &["poly", "uv", "nodes"], None, &|m, _, a| {
                pe::patch_to_mesh_set_polygon_uv(m, a.p("poly"), a.p("uv"));
                Ret::None
            })
        },
        "splineFrenetCoord" | "SI_splineDistanceToValueFrenet" => |c| {
            if c.input("spl").words()[5].value().is_none_or(|k| k as u32 > 3) {
                return Verdict::Skip;
            }
            check(c, &["spl", "pt", "frames", "out", "keys", "len", "fr", "gl", "glist"], None, &|m, o, a| {
                use si_spline::frenet_image as fr;
                fr::init_axes(m);
                let s = Ptr32::new(a.p("spl"));
                if c.name == "splineFrenetCoord" {
                    Ret::Int(fr::spline_frenet_coord(m, o, s, a.f32("u"), a.p("frames"), a.p("out")) as u32)
                } else {
                    fr::spline_distance_to_value_frenet(m, o, s, a.f32("dist"), a.p("pt"), a.p("frames"), a.p("out"));
                    Ret::None
                }
            })
        },
        "si_splineUpdateFrenet" => |c| {
            // a type outside 0..=3: the basis is a local nobody set
            if c.input("spl").words()[5].value().is_none_or(|k| k as u32 > 3) {
                return Verdict::Skip;
            }
            check(c, &["spl", "out", "keys", "len", "gl", "glist"], None, &|m, o, a| {
                si_spline::frenet_image::init(m);
                Ret::Int(si_spline::frenet_image::spline_update_frenet(m, o, Ptr32::new(a.p("spl")), a.p("out")) as u32)
            })
        },
        "SI_intersectionPerform" => |c| {
            // an axis outside 0..=2: the number of roots is a local nobody set
            if c.input("axis").i16() as u16 > 2 {
                return Verdict::Skip;
            }
            check(c, &["spl", "keys", "len", "gl", "glist", "g086d6960"], None, &|m, o, a| Ret::Int(ii::intersection_perform(m, o, Ptr32::new(a.p("spl")), a.i("axis") as i16, a.f32("a"), a.f32("b"), a.f32("c"), a.i("mode") as i16)))
        },
        "SI_mergeTwoPatchs" => |c| {
            check(c, MERGE, None, &|m, o, a| {
                let w = |n: &str| a.i(n) as i16;
                Ret::Int(mi::merge_two_patchs(m, o, a.p("models"), w("adir"), w("ia"), w("bdir"), w("ib"), w("del"), w("ov"), a.f32("blend")).addr())
            })
        },
        "SI_findDiscontinuity" => |c| {
            check(c, &["ct", "fl", "cpts", "g086d3708"], None, &|m, _, a| {
                ci::find_discontinuity(m, a.p("ct"), a.p("fl"));
                Ret::None
            })
        },
        "dialogTestFit" => |c| {
            check(c, &["g086d3700", "g086d3708", "g086d3710", "g086d3718"], None, &|m, o, _| {
                ci::dialog_test_fit(m, o);
                Ret::None
            })
        },
        "setFitParam" => |c| {
            check(c, &["g086d3700", "g086d3708", "g086d3710", "g086d3718"], None, &|m, _, a| {
                ci::set_fit_param(m, a.i("look"), a.f64("a"), a.f64("b"), a.f64("c"));
                Ret::None
            })
        },
        "newPoint" => |c| {
            check(c, &["ret"], None, &|m, _, a| {
                let v = |n: &str| {
                    let f = c.input(n).f32s();
                    [f[0], f[1], f[2]]
                };
                ci::new_point(m, a.p("ret"), v("a"), v("b"), a.f64("d"));
                Ret::None
            })
        },
        "generateCurve" => |c| {
            check(c, &["d", "flags", "g0881b620", "cbez", "g0881b208", "keys", "g086d3700"], None, &|m, _, a| {
                ci::generate_curve(m, a.p("d"), a.p("flags"), a.i("n"));
                Ret::None
            })
        },
        "SI_splineIsPath" => |c| check(c, SCENE, None, &|m, o, a| Ret::Int(ti::spline_is_path(m, o, a.p("mdl")) as u32)),
        "SI_splineIsInExtRel" => |c| check(c, SCENE, None, &|m, o, a| Ret::Int(ti::spline_is_in_ext_rel(m, o, a.p("mdl")) as u32)),
        "SI_splineCut" => |c| {
            // the routine compares the picked point with keys `index − 1` and `index` before
            // it looks at anything else: an index that is not a key in use with one before
            // it reads records outside the keys (other memory, another answer here)
            let (index, nb) = (c.input("index").i32(), c.input("spl").words()[6].value().unwrap_or(0) as i32);
            if index <= 0 || index >= nb {
                return Verdict::Skip;
            }
            check(c, SPL3, None, &|m, _, a| {
                let p = |n: &str| Ptr32::new(a.p(n));
                let f = c.input("pt").f32s();
                ti::spline_cut(m, p("spl"), p("spla"), p("splb"), a.p("fa"), a.p("fb"), [f[0], f[1], f[2], f[3]], a.i("index"));
                Ret::None
            })
        },
        "SI_bezierSetMaximumConstraint" => |c| {
            check(c, SPL, None, &|m, _, a| {
                ti::bezier_set_maximum_constraint(m, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "splineRefinement" => |c| check(c, SPL, None, &|m, o, a| Ret::Int(ti::spline_refinement(m, o, Ptr32::new(a.p("spl")), a.f32("u")) as u32)),
        "SI_patchCut" => |c| {
            if odd_type(c, "pat") {
                return Verdict::Skip;
            }
            // a cut in U near the end of a B-spline or cardinal direction gives part `a`
            // more rows than it has points for: its array is grown and the new points are
            // memory nobody initialised, from which the display (and a capping) is then
            // computed. Whatever agrees between the emulator's two runs there (a NaN from
            // two different garbage values) is not a defined result.
            let rows = |f: &Field| f.words()[3].value().unwrap_or(0);
            if c.try_output("pata").is_some_and(|after| rows(after) > rows(c.input("pata"))) {
                return Verdict::Skip;
            }
            check(c, PAT3, None, &|m, _, a| {
                let p = |n: &str| Ptr32::new(a.p(n));
                Ret::Int(pi::patch_cut(m, p("pat"), p("pata"), p("patb"), a.p("fa"), a.p("fb"), a.i("index"), a.p("ta"), a.p("tb")) as u32)
            })
        },
        "SI_patchAddRow" => |c| {
            if odd_type(c, "pat") {
                return Verdict::Skip;
            }
            check(c, PAT, None, &|m, _, a| {
                pi::patch_add_row(m, Ptr32::new(a.p("pat")), a.i("index"), a.i("flag") as i16, a.p("table"));
                Ret::None
            })
        },
        "drawBezierCurve" => |c| {
            check(c, CBEZ, None, &|m, _, a| {
                fi::draw_bezier_curve(m, a.p("bez"));
                Ret::None
            })
        },
        "fitCubic" => |c| {
            check(c, CBEZ, None, &|m, _, a| {
                fi::fit_cubic(m, a.p("d"), a.i("first"), a.i("last"), a.pnt("t1"), a.pnt("t2"), a.f64("error"));
                Ret::None
            })
        },
        "SI_fitBezCurve" => |c| {
            check(c, CBEZ, None, &|m, _, a| {
                fi::fit_bez_curve(m, a.p("d"), a.i("n"), a.f64("error"));
                Ret::None
            })
        },
        "SI_fitCurve" => |c| check(c, FIT, Some(spline_post), &|m, o, a| Ret::Int(fi::fit_curve(m, o, Ptr32::new(a.p("spl")), a.f64("angle"), a.f64("tol")).addr())),
        _ => return None,
    })
}

/// `o.<array>` and `n.<array>` of the earlier passes' patch vectors: an array read through
/// the pointers of the record (`pat`, or the patch the routine returned); `n.rec`: the
/// returned record without its padding words, a pointer compared for being null or not.
fn patch_post(m: &Mem, names: &Names, c: &Case, f: &Field) -> Option<Verdict> {
    let (who, what) = f.name.split_once('.')?;
    let rec = match who {
        "o" => names.addr(m, "pat")?,
        "n" => match &c.output("ptr").words()[0] {
            si_x87::vectors::Word::Ref { name, offset } => names.addr(m, name)? + offset,
            _ => return Some(Verdict::Same),
        },
        _ => return None,
    };
    if what == "rec" {
        let got: Vec<u32> = crate::pat::pat_words(&m.read(Ptr32::<si_types::geom::Patch>::new(rec))).into_iter().enumerate().filter(|(i, _)| !crate::pat::PADS.contains(i)).map(|(_, w)| w).collect();
        let want = crate::util::nums(f);
        if want.len() != got.len() {
            return Some(Verdict::Different(format!("n.rec: {} words against the port's {}", want.len(), got.len())));
        }
        for (i, (w, g)) in want.iter().zip(&got).enumerate() {
            if w.is_some_and(|w| w != *g) {
                return Some(Verdict::Different(format!("n.rec[{i}]: original {:08x}, port {g:08x}", w.unwrap_or(0))));
            }
        }
        return Some(Verdict::Same);
    }
    let off = match what {
        "pts" => 0x1a4,
        "tags" => 0x1a8,
        "s0" => 0x1b0,
        "m0" => 0x1d8,
        "s1" => 0x1b8,
        "m1" => 0x1dc,
        "d0" => 0x1c4,
        "d1" => 0x1c8,
        _ => return None,
    };
    let words: Vec<si_mem::Word> = f
        .words()
        .iter()
        .map(|w| match w {
            si_x87::vectors::Word::Value { bytes, value } => si_mem::Word::Value { bytes: *bytes, value: *value },
            si_x87::vectors::Word::Unwritten { bytes } => si_mem::Word::Unwritten { bytes: *bytes },
            si_x87::vectors::Word::Ref { name, offset } => si_mem::Word::Ref { name: name.clone(), offset: *offset },
        })
        .collect();
    if words.is_empty() {
        return Some(Verdict::Same);
    }
    Some(match m.compare_words(m.u32(rec + off), &words, names) {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(format!("{}: {e}", f.name)),
    })
}

/// A made-up patch of the earlier passes' vectors, with the index table and the pointer
/// `SI_patchDispose` takes.
const PAT_OLD: &[&str] = &["pp", "pat", "t", "pts", "tags", "s0", "m0", "s1", "m1", "d0", "d1"];

/// A made-up patch of the earlier passes' evaluation vectors, with its outputs.
const PAT_EVAL: &[&str] = &["pat", "pts", "tags", "s0", "m0", "s1", "m1", "d0", "d1", "out", "nu", "nv", "p", "tu", "tv", "n"];

/// A U type outside 0..=3.
fn odd_u(c: &Case) -> bool {
    c.input("pat").words()[1].value().is_some_and(|k| k as u32 > 3)
}

/// `grid` of the earlier passes' `SI_patchSurface` vectors: the block `*out` points to.
fn grid_post(m: &Mem, names: &Names, _c: &Case, f: &Field) -> Option<Verdict> {
    if f.name != "grid" {
        return None;
    }
    let words: Vec<si_mem::Word> = f.words().iter().map(|w| match w {
        si_x87::vectors::Word::Value { bytes, value } => si_mem::Word::Value { bytes: *bytes, value: *value },
        si_x87::vectors::Word::Unwritten { bytes } => si_mem::Word::Unwritten { bytes: *bytes },
        si_x87::vectors::Word::Ref { name, offset } => si_mem::Word::Ref { name: name.clone(), offset: *offset },
    }).collect();
    if words.is_empty() {
        return Some(Verdict::Same);
    }
    let at = m.u32(names.addr(m, "out")?);
    Some(match m.compare_words(at, &words, names) {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(format!("grid: {e}")),
    })
}

/// `n.*` of the revolution's vectors is the patch it returned, `o.*` the profile spline.
fn revolution_post(m: &Mem, names: &Names, c: &Case, f: &Field) -> Option<Verdict> {
    if f.name.starts_with("n.") {
        patch_post(m, names, c, f)
    } else {
        spline_post(m, names, c, f)
    }
}

/// `n.*` of the extraction's vectors is the spline it returned.
fn extract_post(m: &Mem, names: &Names, c: &Case, f: &Field) -> Option<Verdict> {
    spline_post(m, names, c, f)
}

pub fn lookup_old(name: &str) -> Option<Check> {
    Some(match name {
        "SI_splineCopy" => |c| check(c, SPL, Some(spline_post), &|m, _, a| Ret::Int(ii::spline_copy(m, Ptr32::new(a.p("spl"))).addr())),
        "SI_bezierExpand" => |c| check(c, &["spl", "keys", "len", "gl", "glist", "g086d6960"], Some(spline_post), &|m, _, a| Ret::Int(cv::spline_bezier_expand(m, Ptr32::new(a.p("spl"))).addr())),
        "SI_splineMultiplyDefinition" => |c| {
            check(c, &["spl", "keys", "len", "gl", "glist", "gi", "idx", "m"], Some(spline_post), &|m, o, a| {
                cv::spline_multiply_definition(m, o, Ptr32::new(a.p("spl")), a.p("m"));
                Ret::None
            })
        },
        "SI_splineRevolutionToPatch" => |c| {
            check(
                c,
                &["spl", "keys", "len", "gl", "glist", "gi", "idx", "g086d6960", "mdl", "splm", "keysm", "lenm", "crv", "knots", "pts", "lp", "ld", "b5c"],
                Some(revolution_post),
                &|m, o, a| {
                    let model = if c.try_input("mdl").is_some() { a.p("mdl") } else { 0 };
                    Ret::Int(ri::spline_revolution_to_patch(m, o, Ptr32::new(a.p("spl")), a.i("kind"), a.i("steps") as i16, a.f32("angle"), a.i("axis"), model, a.i("comp") as i16, a.i("def")).addr())
                },
            )
        },
        "SI_patchExtractSpline" => |c| {
            check(c, &["pat", "pts", "tags", "g086d6960"], Some(extract_post), &|m, _, a| {
                Ret::Int(pi::patch_extract_spline(m, Ptr32::new(a.p("pat")), a.i("along") as i16, a.f32("t")).addr())
            })
        },
        "initMaxValue" => |c| {
            check(c, &["pat", "pts", "tags", "pat2", "pts2", "tags2", "g087889fc", "g087889f8", "g08788a02", "g08788a00", "g087889e0", "g087889e4", "g087889d8", "g087889dc"], None, &|m, _, a| {
                zi::init_max_value(m, Ptr32::new(a.p("pat")), Ptr32::new(a.p("pat2")));
                Ret::None
            })
        },
        "SI_splineTransfer" => |c| {
            check(c, &["spl", "spl2", "keys", "len", "keys2", "len2"], Some(spline_post), &|m, _, a| {
                ii::spline_transfer(m, Ptr32::new(a.p("spl")), Ptr32::new(a.p("spl2")));
                Ret::None
            })
        },
        "SI_splineDispose" => |c| {
            check(c, &["pp", "spl", "keys", "len", "nm"], Some(spline_post), &|m, _, a| {
                ii::spline_dispose(m, a.p("pp"));
                Ret::None
            })
        },
        "splineSubdivision" => |c| check(c, SPL, Some(spline_post), &|m, o, a| Ret::Int(ii::spline_subdivision(m, o, Ptr32::new(a.p("spl")), a.i("seg"), a.f32("t")) as u32)),
        "SI_patchCopy" => |c| check(c, PAT_OLD, Some(patch_post), &|m, _, a| Ret::Int(pi::patch_copy(m, Ptr32::new(a.p("pat"))).addr())),
        "SI_patchUpdateMatrix" => |c| {
            // a U type outside 0..=3: the basis is a local nobody set
            if odd_u(c) {
                return Verdict::Skip;
            }
            check(c, PAT_EVAL, Some(patch_post), &|m, _, a| {
                pe::patch_update_matrix(m, Ptr32::new(a.p("pat")));
                Ret::None
            })
        },
        "SI_patchSurface" => |c| {
            check(c, PAT_EVAL, Some(grid_post), &|m, _, a| {
                pe::patch_surface(m, Ptr32::new(a.p("pat")), a.p("out"), a.p("nu"), a.p("nv"));
                Ret::None
            })
        },
        "SI_patchValue" => |c| {
            check(c, PAT_EVAL, Some(patch_post), &|m, _, a| {
                pe::patch_value(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("out"));
                Ret::None
            })
        },
        "SI_patchValueAndTangent" => |c| {
            check(c, PAT_EVAL, Some(patch_post), &|m, _, a| Ret::Int(pe::patch_value_and_tangent(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("p"), a.p("tu"), a.p("tv")) as u32))
        },
        "SI_patchValueAndNormal" => |c| {
            check(c, PAT_EVAL, Some(patch_post), &|m, _, a| Ret::Int(pe::patch_value_and_normal(m, Ptr32::new(a.p("pat")), a.f32("u"), a.f32("v"), a.p("p"), a.p("n")) as u32))
        },
        "SI_patchDispose" => |c| {
            check(c, PAT_OLD, Some(patch_post), &|m, _, a| {
                pi::patch_dispose(m, a.p("pp"));
                Ret::None
            })
        },
        "SI_patchDeleteRow" => |c| check(c, PAT_OLD, Some(patch_post), &|m, _, a| Ret::Int(pi::patch_delete_row(m, Ptr32::new(a.p("pat")), a.i("index") as i16, a.i("along") as i16, a.p("t")) as u32)),
        "SI_splineSetDummy" => |c| {
            check(c, SPL, Some(spline_post), &|m, _, a| {
                si::spline_set_dummy(m, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_splineTouch" => |c| {
            check(c, SPL, Some(spline_post), &|m, o, a| {
                si::spline_touch(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_splineDeletePoint" => |c| check(c, SPL, Some(spline_post), &|m, o, a| Ret::Int(si::spline_delete_point(m, o, Ptr32::new(a.p("spl")), a.i("i")) as u32)),
        "SI_splineAddPoint" => |c| check(c, &["spl", "keys", "len", "gl", "glist", "gi", "idx", "p"], Some(spline_post), &|m, o, a| Ret::Int(si::spline_add_point(m, o, Ptr32::new(a.p("spl")), a.i("i"), a.p("p")).addr())),
        "SI_splineUpdateMatrix" => |c| {
            check(c, SPL, Some(spline_post), &|m, _, a| {
                si::spline_update_matrix(m, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_splineDeleteSelection" => |c| check(c, SPL, Some(spline_post), &|m, o, a| Ret::Int(cv::spline_delete_selection(m, o, Ptr32::new(a.p("spl"))) as u32)),
        "SI_splineInverse" => |c| {
            check(c, SPL, Some(spline_post), &|m, o, a| {
                cv::spline_inverse(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_splineCleanup" => |c| {
            check(c, SPL, Some(spline_post), &|m, o, a| {
                cv::spline_cleanup(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "SI_allDummy" => |c| {
            check(c, SPL, Some(spline_post), &|m, o, a| {
                cv::all_dummy(m, o, Ptr32::new(a.p("spl")));
                Ret::None
            })
        },
        "splineConvertToLinearRegular" => |c| check(c, SPL, Some(spline_post), &|m, _, a| Ret::Int(cv::spline_convert_to_linear_regular(m, Ptr32::new(a.p("spl")), a.i("steps")).addr())),
        "splineConvertToLinearFlatness" => |c| check(c, SPL, Some(spline_post), &|m, o, a| Ret::Int(cv::spline_convert_to_linear_flatness(m, o, Ptr32::new(a.p("spl")), a.f32("tol")).addr())),
        "SI_splineConvertToLinear" => |c| {
            check(c, SPL, Some(spline_post), &|m, o, a| Ret::Int(cv::spline_convert_to_linear(m, o, Ptr32::new(a.p("spl")), a.i("regular") as i16, a.i("steps"), a.f32("tol")).addr()))
        },
        "SI_splineKeyAllocate" => |c| check(c, &[], None, &|m, _, a| Ret::Int(si::spline_key_allocate(m, a.i("n")).addr())),
        "bezierValue" => |c| {
            check(c, &["ret", "v"], None, &|m, _, a| {
                fi::bezier_value(m, a.p("ret"), a.i("degree"), a.p("v"), a.f64("t"));
                Ret::None
            })
        },
        "computeLeftTangent" | "computeRightTangent" | "computeCenterTangent" => |c| {
            check(c, &["ret", "d"], None, &|m, _, a| {
                let p = match c.name.as_str() {
                    "computeLeftTangent" => fi::compute_left_tangent(m, a.p("d"), a.i("end")),
                    "computeRightTangent" => fi::compute_right_tangent(m, a.p("d"), a.i("end")),
                    _ => fi::compute_center_tangent(m, a.p("d"), a.i("end")),
                };
                fi::put(m, a.p("ret"), p);
                Ret::None
            })
        },
        "chordLengthParameterize" => |c| check(c, &["d"], None, &|m, _, a| Ret::Int(fi::chord_length_parameterize(m, a.p("d"), a.i("first"), a.i("last")))),
        "computeMaxError" => |c| check(c, &["d", "bez", "u", "split"], None, &|m, _, a| Ret::St0(fi::compute_max_error(m, a.p("d"), a.i("first"), a.i("last"), a.p("bez"), a.p("u"), a.p("split")))),
        "newtonRaphsonRootFind" => |c| check(c, &["q"], None, &|m, _, a| Ret::St0(fi::newton_raphson_root_find(m, a.p("q"), a.pnt("p"), a.f64("u")))),
        "reparameterize" => |c| check(c, &["d", "u", "bez"], None, &|m, _, a| Ret::Int(fi::reparameterize(m, a.p("d"), a.i("first"), a.i("last"), a.p("u"), a.p("bez")))),
        "generateBezier" => |c| check(c, &["d", "u"], None, &|m, _, a| Ret::Int(fi::generate_bezier(m, a.p("d"), a.i("first"), a.i("last"), a.p("u"), a.pnt("t1"), a.pnt("t2")))),
        _ => return None,
    })
}
