// SPDX-License-Identifier: MIT
//! Checks of `face.rs` (vectors of `emu_face.py`). A case lays a face out in emulator
//! memory: the record, three hole splines and their keys, three NURBS curves and their
//! points; the spline and NURBS routines `face.c` calls are recorded stand-ins.

use std::collections::VecDeque;

use si_hrc::{Face, NurbsCurve, NurbsPoint, Spline, SplineKey};
use si_math::types::Matrix;
use si_model::face::{self as fc, Hooks};
use si_model::mesh_b::MeshEnv;
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

use crate::support::{same, same_field, same_mesh};
use crate::world::mutate;

const SPL: usize = 33;

fn val(w: &Word) -> u32 {
    w.value().unwrap_or(0) as u32
}

fn keys(c: &Case, name: &str) -> Vec<SplineKey> {
    let f = c.input(name).words();
    f.chunks(14)
        .map(|k| {
            let v = |i: usize| f32::from_bits(val(&k[i]));
            SplineKey { prev: [v(0), v(1), v(2)], pos: [v(3), v(4), v(5)], next: [v(6), v(7), v(8)], constraint: val(&k[10]) as i16, segment: val(&k[11]) as i16, flag: val(&k[12]) as i16 }
        })
        .collect()
}

fn spline(c: &Case, w: &[Word]) -> Spline {
    let keys = match &w[8] {
        Word::Ref { name, .. } => keys(c, name),
        _ => Vec::new(),
    };
    Spline { name: Vec::new(), phantom: val(&w[2]) as i16, close: val(&w[3]) as i16, kind: val(&w[5]) as i32, nkeys: val(&w[6]) as u16, keys, tension: f32::from_bits(val(&w[10])), step: val(&w[11]) as i16, old_points: false }
}

fn curve(c: &Case, w: &[Word]) -> NurbsCurve {
    let points = match &w[10] {
        Word::Ref { name, .. } => c.input(name).words().chunks(5).map(|p| NurbsPoint { xyzw: [0, 1, 2, 3].map(|i| f64::from_bits(p[i].value().unwrap_or(0) as u64)), flag: val(&p[4]) as i32 }).collect(),
        _ => Vec::new(),
    };
    NurbsCurve { order: val(&w[0]) as i32, npoints: val(&w[1]) as i32, points, uniformity: val(&w[8]) as i32, closing: val(&w[11]) as i32, step: val(&w[12]) as i32, ..NurbsCurve::default() }
}

/// The face of a case, from the record `name` and what it points to. All three holes and
/// curves of the buffers are kept whatever the count says, as the original can read them.
fn face_named(c: &Case, name: &str) -> Face {
    let w = c.input(name).words();
    let holes = match &w[1 + SPL + 2] {
        Word::Ref { name, .. } => c.input(name).words().chunks(SPL).map(|s| spline(c, s)).collect(),
        _ => Vec::new(),
    };
    let nurbs = match &w[1 + SPL + 3] {
        Word::Ref { name, .. } => c.input(name).words().chunks(24).map(|s| curve(c, s)).collect(),
        _ => Vec::new(),
    };
    Face { exterior: spline(c, &w[1..1 + SPL]), count: val(&w[1 + SPL]) as i16, holes, nurbs, is_spline: val(&w[1 + SPL + 4]) as i16 }
}

fn face(c: &Case) -> Face {
    face_named(c, "face")
}

/// The stand-ins of the generator: every call logged as (routine, contour, further words);
/// a NURBS curve's number is 100 + k.
struct Log {
    calls: Vec<(&'static str, Vec<u32>)>,
    answers: VecDeque<u32>,
    rand: u64,
    nurbs: bool,
}

impl Log {
    fn new(c: &Case, nurbs: bool) -> Log {
        Log { calls: Vec::new(), answers: c.input("answers").u32s().into(), rand: 0x1234_abcd_330e, nurbs }
    }

    fn log(&mut self, name: &'static str, which: i32, extra: &[u32]) {
        let w = if self.nurbs && which >= 0 { which + 100 } else { which };
        let mut v = vec![w as u32];
        v.extend_from_slice(extra);
        self.calls.push((name, v));
    }

    fn answer(&mut self) -> u32 {
        self.answers.pop_front().unwrap_or(0)
    }

    /// The same calls with the same contours in the same order as the original made.
    fn same(&self, c: &Case) -> Verdict {
        const NAMES: [&str; 17] = [
            "SI_splineTouch", "SI_splineDraw", "SI_splineDrawModif", "SI_nurbsCrvDraw", "SI_splineInverse", "SI_nurbsCrvInverse", "SI_splineDeletePoint",
            "SI_nurbsCrvDeletePoint", "SI_splineAddPoint", "SI_splineDeleteSelection", "SI_splineCleanup", "SI_splineTransform", "SI_nurbsCrvApplyMatrix",
            "SI_knotsCrvReparametrization", "SI_nurbsCrvTransferPointsKnots", "SI_splineConvertToLinear", "SI_nurbsCrvConvertToLinear",
        ];
        let want: Vec<(String, Vec<u32>)> = c.calls.iter().filter(|k| NAMES.contains(&k.name.as_str())).map(|k| (k.name.clone(), k.arg_values().iter().map(|&v| v as u32).collect())).collect();
        let got: Vec<(String, Vec<u32>)> = self.calls.iter().map(|(n, a)| (n.to_string(), a.iter().map(|v| v ^ mutate() as u32).collect())).collect();
        Verdict::from(want == got, || format!("the original calls {want:?}, the port {got:?}"))
    }
}

impl Hooks for Log {
    fn spline_touch(&mut self, which: i32, _: &mut Spline) {
        self.log("SI_splineTouch", which, &[]);
    }
    fn spline_draw(&mut self, which: i32, _: &Spline) {
        self.log("SI_splineDraw", which, &[]);
    }
    fn spline_draw_modif(&mut self, which: i32, _: &Spline) {
        self.log("SI_splineDrawModif", which, &[]);
    }
    fn nurbs_crv_draw(&mut self, which: i32, _: &NurbsCurve) {
        self.log("SI_nurbsCrvDraw", which, &[]);
    }
    fn spline_inverse(&mut self, which: i32, _: &mut Spline) {
        self.log("SI_splineInverse", which, &[]);
    }
    fn nurbs_crv_inverse(&mut self, which: i32, _: &mut NurbsCurve) -> bool {
        self.log("SI_nurbsCrvInverse", which, &[]);
        self.answer() as i16 != 0
    }
    fn spline_delete_point(&mut self, which: i32, _: &mut Spline, key: i32) -> i16 {
        self.log("SI_splineDeletePoint", which, &[key as u32]);
        self.answer() as i16
    }
    fn nurbs_crv_delete_point(&mut self, which: i32, _: &mut NurbsCurve, point: i32) {
        self.log("SI_nurbsCrvDeletePoint", which, &[point as u32]);
    }
    fn spline_add_point(&mut self, which: i32, _: &mut Spline, key: i32, _: [f32; 3]) {
        self.log("SI_splineAddPoint", which, &[key as u32]);
    }
    fn spline_delete_selection(&mut self, which: i32, _: &mut Spline) -> i16 {
        self.log("SI_splineDeleteSelection", which, &[]);
        self.answer() as i16
    }
    fn spline_convert_to_linear(&mut self, which: i32, s: &Spline, step: i16) -> Spline {
        self.log("SI_splineConvertToLinear", which, &[1, step as u16 as u32]);
        s.clone()
    }
    /// (The original cleans the new spline, which is no contour of a face: 999.)
    fn spline_cleanup(&mut self, _: i32, _: &mut Spline) {
        self.calls.push(("SI_splineCleanup", vec![999]));
    }
    fn nurbs_crv_convert_to_linear(&mut self, which: i32, c: &NurbsCurve) -> NurbsCurve {
        self.log("SI_nurbsCrvConvertToLinear", which, &[]);
        c.clone()
    }
    fn spline_transform(&mut self, which: i32, _: &mut Spline, _: &Matrix) {
        self.log("SI_splineTransform", which, &[]);
    }
    fn nurbs_crv_apply_matrix(&mut self, which: i32, _: &mut NurbsCurve, _: &Matrix) {
        self.log("SI_nurbsCrvApplyMatrix", which, &[]);
    }
    fn nurbs_crv_transfer_points_knots(&mut self, which: i32, _: &NurbsCurve, _: &mut NurbsCurve) {
        self.log("SI_nurbsCrvTransferPointsKnots", which, &[]);
    }
    fn knots_crv_reparametrization(&mut self, which: i32, _: &mut NurbsCurve) {
        self.log("SI_knotsCrvReparametrization", which, &[]);
    }
    /// The C library's `drand48` from its start state.
    fn drand48(&mut self) -> f64 {
        self.rand = (self.rand.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb)) & 0xffff_ffff_ffff;
        self.rand as f64 / (1u64 << 48) as f64
    }
}

fn nurbs(c: &Case) -> bool {
    val(&c.input("face").words()[1 + SPL + 4]) as i16 == 0
}

fn messages(c: &Case, env: &MeshEnv) -> Verdict {
    Verdict::from(env.messages.len() == crate::calls(c, "SI_statusDisplayMessage"), || format!("{} messages", env.messages.len()))
}

fn global_to_local(c: &Case) -> Verdict {
    let (which, local) = fc::face_global_to_local_index(&face(c), c.input("g").i32(), c.input("t").i16() != 0);
    same(c, "c", &[which as u32]).and(|| same(c, "l", &[local as u32]))
}

fn local_to_global(c: &Case) -> Verdict {
    same(c, "o", &[fc::face_local_to_global_index(&face(c), c.input("c").i32(), c.input("l").i32(), c.input("t").i16() != 0) as u32])
}

fn segment_to_point(c: &Case) -> Verdict {
    same(c, "eax", &[fc::face_segment_to_point(&face(c), c.input("seg").i32()) as u32])
}

fn nb_triangles(c: &Case) -> Verdict {
    same(c, "eax", &[fc::face_nb_triangles(&face(c)) as u32])
}

fn get_indexes(c: &Case) -> Verdict {
    let pairs = fc::face_get_indexes(&face(c), c.input("tagged").i16() != 0);
    let flat: Vec<u32> = pairs.iter().flatten().flat_map(|&(a, b)| [a as u32, b as u32]).collect();
    same(c, "eax", &[pairs.as_ref().map_or(-1i32 as u32, |p| p.len() as u32)]).and(|| same(c, "r.pairs", &flat))
}

fn visible(c: &Case) -> Verdict {
    si_model::env::stale::reset();
    let v = fc::is_face_visible(&face(c), c.input("up").i16() != 0);
    if si_model::env::stale::count() > 0 {
        return Verdict::Skip;
    }
    same(c, "ax", &[v as u32])
}

/// A routine whose effect is the calls it makes.
fn by_calls(c: &Case, f: impl FnOnce(&mut Face, &mut Log, &mut MeshEnv) -> Verdict) -> Verdict {
    si_model::env::stale::reset();
    let mut face = face(c);
    let mut log = Log::new(c, nurbs(c));
    let mut env = MeshEnv::default();
    let v = f(&mut face, &mut log, &mut env);
    if si_model::env::stale::count() > 0 {
        return Verdict::Skip;
    }
    v.and(|| log.same(c)).and(|| messages(c, &env))
}

fn ordinate(c: &Case) -> Verdict {
    by_calls(c, |f, log, _| {
        fc::face_ordinate_points(f, c.input("up").i16() != 0, log);
        Verdict::Same
    })
}

fn matrix(f: &Field) -> Matrix {
    let w = f.f32s();
    let mut m = [[0.0f32; 4]; 4];
    for i in 0..16 {
        m[i / 4][i % 4] = w[i];
    }
    m
}

/// The keys of the case's four key buffers after the call against the port's splines.
fn same_keys(c: &Case, f: &Face, names: [&str; 4]) -> Verdict {
    for (i, name) in names.iter().enumerate() {
        let s = if i == 0 { &f.exterior } else { &f.holes[i - 1] };
        let mut got = crate::values(c.input(name));
        for (k, key) in s.keys.iter().enumerate() {
            for (j, v) in key.prev.iter().chain(&key.pos).chain(&key.next).enumerate() {
                got[14 * k + j] = v.to_bits();
            }
        }
        let v = same_field(c.after(name), name, &got);
        if v != Verdict::Same {
            return v;
        }
    }
    Verdict::Same
}

fn randomize(c: &Case) -> Verdict {
    by_calls(c, |f, log, _| {
        let a = c.input("amount").f32s();
        fc::face_randomize(f, [a[0], a[1], a[2]], c.input("times").i16(), c.input("tagged").i16() != 0, log);
        if f.is_spline == 0 {
            // the control points, as doubles
            for k in 0..3 {
                let name = format!("p{k}");
                let want = c.after(&name).written();
                for (i, p) in f.nurbs[k].points.iter().enumerate() {
                    for a in 0..3 {
                        if want[5 * i + a].is_some_and(|w| w as u64 != p.xyzw[a].to_bits() ^ mutate() as u64) {
                            return Verdict::Different(format!("{name}: point {i} axis {a}"));
                        }
                    }
                }
            }
            return Verdict::Same;
        }
        same_keys(c, f, ["k0", "k1", "k2", "k3"])
    })
}

fn transfer_points(c: &Case) -> Verdict {
    let src = face(c);
    let mut dst = face_named(c, "face2");
    let mut log = Log::new(c, false);
    fc::face_transfer_points(&src, &mut dst, &mut log);
    same_keys(c, &dst, ["kk0", "kk1", "kk2", "kk3"]).and(|| log.same(c))
}

/// A face as the generator dumps one: count, the spline word, per contour its key count,
/// kind and nine floats per key.
fn face_words(f: &Face) -> Vec<u32> {
    let mut w = vec![f.count as i32 as u32, f.is_spline as u16 as u32];
    let n = f.count.max(0) as usize;
    for s in std::iter::once(&f.exterior).chain(f.holes.iter().take(n)) {
        w.extend([s.nkeys as u32, s.kind as u32]);
        for key in s.keys.iter().take(s.nkeys as usize) {
            w.extend(key.prev.iter().chain(&key.pos).chain(&key.next).map(|v| v.to_bits()));
        }
    }
    w
}

fn delete_hole(c: &Case) -> Verdict {
    let mut f = face(c);
    // an index beyond the contours makes the original copy more splines or curves than
    // its new array holds
    if c.input("i").i32() > f.count as i32 {
        return Verdict::Skip;
    }
    fc::face_delete_hole(&mut f, c.input("i").i32());
    if f.is_spline == 0 {
        let mut w = vec![f.count as i32 as u32];
        for k in f.nurbs.iter().take(f.count.max(0) as usize) {
            w.extend([k.order as u32, k.npoints as u32, k.step as u32]);
        }
        return same(c, "r.curves", &w);
    }
    same(c, "r.face", &face_words(&f))
}

fn copy(c: &Case) -> Verdict {
    same(c, "r.face", &face_words(&fc::face_copy(&face(c))))
}

fn convert_to_linear(c: &Case) -> Verdict {
    by_calls(c, |f, log, _| match fc::face_convert_to_linear(f, c.input("step").i16(), c.input("clean").i16() != 0, log) {
        Some(lin) => same(c, "r.face", &face_words(&lin)),
        None => same(c, "r.face", &[]),
    })
}

fn convert_to_mesh(c: &Case) -> Verdict {
    by_calls(c, |f, log, env| {
        // (the allocator's cosine is asked by the port itself, as its first call)
        // A contour of one point at the head of the corner list: the rebuild reads the
        // word before the corner array for its "previous corner" (finding 0015, "Stars
        // and edges"); the port takes -1 there.
        let first = if f.is_spline != 0 { f.exterior.nkeys as i32 } else { f.nurbs.first().map_or(0, |k| k.npoints) };
        if first == 1 {
            return Verdict::Skip;
        }
        match fc::face_convert_to_mesh(f, c.input("clean").i16() != 0, env, log) {
            Some(m) => match same_mesh(c, &m, "m.", 6) {
                Verdict::Different(d) => Verdict::Different(format!("{d}; the port's corners {:?}, messages {:?}", m.polygons[0].nodes.iter().map(|n| n.vertex).collect::<Vec<_>>(), env.messages)),
                v => v,
            },
            None => Verdict::from(c.output("m.hdr").is_null(), || "the port makes no mesh".to_string()),
        }
    })
}

fn dump(c: &Case) -> Verdict {
    let title = String::from_utf8_lossy(&c.input("title").to_bytes()).into_owned();
    let text = fc::face_dump(&face(c), &title, Some("fc"), |_, _| String::new());
    let want = c.output("f").to_bytes();
    let got: Vec<u8> = if mutate() { text.to_uppercase().into_bytes() } else { text.into_bytes() };
    Verdict::from(want == got, || format!("the original writes {:?}", String::from_utf8_lossy(&want)))
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_faceGlobalToLocalIndex" => global_to_local,
        "SI_faceLocalToGlobalIndex" => local_to_global,
        "SI_faceSegmentToPoint" => segment_to_point,
        "SI_faceNbTriangles" => nb_triangles,
        "SI_faceGetIndexes" => get_indexes,
        "SI_isFaceVisible" => visible,
        "SI_faceOrdinatePoints" => ordinate,
        "SI_faceTouch" => |c| by_calls(c, |f, log, _| {
            fc::face_touch(f, log);
            Verdict::Same
        }),
        "SI_faceDraw" => |c| by_calls(c, |f, log, _| {
            fc::face_draw(f, log);
            Verdict::Same
        }),
        "SI_faceDrawModif" => |c| by_calls(c, |f, log, _| {
            fc::face_draw_modif(f, log);
            Verdict::Same
        }),
        "SI_faceInverse" => |c| by_calls(c, |f, log, _| {
            fc::face_inverse(f, log);
            Verdict::Same
        }),
        "SI_faceDeleteSelection" => |c| by_calls(c, |f, log, _| same(c, "ax", &[fc::face_delete_selection(f, log) as u16 as u32])),
        "SI_faceTransform" => |c| by_calls(c, |f, log, _| {
            fc::face_transform(f, &matrix(c.input("m")), log);
            Verdict::Same
        }),
        "SI_faceDeletePoint" => |c| by_calls(c, |f, log, env| {
            // an index past the last contour makes the original work on what lies behind
            // the hole array
            if fc::face_global_to_local_index(f, c.input("i").i16() as i32, true).0 >= f.count as i32 {
                return Verdict::Skip;
            }
            fc::face_delete_point(f, c.input("i").i16(), env, log);
            Verdict::Same
        }),
        "SI_faceAddPoint" => |c| by_calls(c, |f, log, _| {
            let p = c.input("pos").f32s();
            let mut i = c.input("i").i32();
            fc::face_add_point(f, &mut i, [p[0], p[1], p[2], p[3]], log);
            same_field(c.after("i"), "i", &[i as u32])
        }),
        "SI_faceRandomize" => randomize,
        "SI_faceTransferPoints" => transfer_points,
        "SI_faceDeleteHole" => delete_hole,
        "SI_faceCopy" => copy,
        "SI_faceConvertToLinear" => convert_to_linear,
        "SI_faceConvertToMesh" => convert_to_mesh,
        "SI_faceDump" => dump,
        _ => return None,
    })
}
