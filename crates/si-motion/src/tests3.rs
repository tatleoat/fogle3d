// SPDX-License-Identifier: MIT
//! Unit tests of the third pass on made-up memory: constraints, trajectory, shapes, texture
//! pictures and the model save-key commands. Expected values are worked by hand from the
//! doc comments (finding 0017, sections 8a and 9).

use crate::mem::MemExt;
use crate::ed::build as scene;
use crate::fcv;
use crate::glob::{self, cid};
use crate::host::{Arg, Ret, Trace};
use si_mem::{Addr, Mem};
use crate::off::{fcurve as fo, key as ko, model, updnode};
use crate::savekey;
use crate::update::build::node;
use crate::update::{constr, shape, texfile};

fn world() -> (Mem, Addr) {
    let mut m = Mem::new();
    let d = scene::scene(&mut m);
    (m, d)
}

fn model_at(m: &mut Mem, d: Addr, pos: [f32; 3]) -> (Addr, i32) {
    let mdl = m.si_calloc(1, 0x430);
    for k in 0..3 {
        m.set_f32(mdl + model::GTRS + 4 * k, pos[k as usize]);
    }
    m.set_ptr(mdl + model::LATDEP, mdl);
    let models = glob::chap(m, cid::MODELS);
    let (_, i) = scene::add_elem(m, d, models, mdl);
    m.set_i32(mdl + model::ELEM_ID, i);
    (mdl, i)
}

/// The three floats a recorded call was handed behind a pointer argument.
fn seen(vectors: &std::rc::Rc<std::cell::RefCell<Vec<[f32; 3]>>>) -> Vec<[f32; 3]> {
    vectors.borrow().clone()
}

/// A host that notes the vector behind argument 1 of the model setters and calls every
/// object a model (graf kind 2).
fn setters() -> (Trace, std::rc::Rc<std::cell::RefCell<Vec<[f32; 3]>>>) {
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let l = log.clone();
    let t = Trace::with(move |m, name, args| {
        if name.starts_with("model_set_") {
            if let Arg::I(p) = args[1] {
                l.borrow_mut().push([m.f32(p), m.f32(p + 4), m.f32(p + 8)]);
            }
        }
        if name == "db_chapter2graf_type" {
            return Ret::int(2);
        }
        Ret::default()
    });
    (t, log)
}

#[test]
fn position_constraint_averages_the_objects() {
    let (mut m, d) = world();
    let models = glob::chap(&m, cid::MODELS);
    let (mdl, mi) = model_at(&mut m, d, [0.0; 3]);
    let (a, ai) = model_at(&mut m, d, [1.0, 2.0, 3.0]);
    let (b, bi) = model_at(&mut m, d, [3.0, 6.0, -1.0]);
    let (c, ci) = model_at(&mut m, d, [100.0, 100.0, 100.0]);
    // the third argument is switched off: it does not count
    let n = node(&mut m, models, 1, mi, mdl, 0x6f, &[(models, ai, a, 1, 0), (models, bi, b, 1, 0), (models, ci, c, 0, 0)]);
    for i in 0..3 {
        let args = m.ptr(n + updnode::ARGS);
        m.set_i16(args + i * 0x14 + 2, 1);
    }
    let (mut t, log) = setters();
    constr::anim_model_constraint_update(&mut m, &mut t, n, 0.0, -1);
    assert_eq!(t.names(), ["db_chapter2graf_type", "db_chapter2graf_type", "model_set_global_translation"]);
    assert_eq!(seen(&log), [[2.0, 4.0, 1.0]]);

    // an effector (a null with algorithm 3) gets the average as its inverse-kinematics goal
    m.set_i32(mdl + model::ALGO_TYPE, 3);
    let (mut t, _) = setters();
    constr::anim_model_constraint_update(&mut m, &mut t, n, 0.0, -1);
    assert_eq!(t.calls.last().unwrap().name, "inv_kin_set_goal");
    assert_eq!(t.calls.last().unwrap().args[1..], [Arg::f(2.0), Arg::f(4.0), Arg::f(1.0)]);

    // a dynamics node is not constrained at all
    m.set_i32(mdl + model::ALGO_TYPE, 2);
    let (mut t, _) = setters();
    constr::anim_model_constraint_update(&mut m, &mut t, n, 0.0, -1);
    assert!(t.calls.is_empty());
}

#[test]
fn two_and_three_points() {
    let (mut m, d) = world();
    let models = glob::chap(&m, cid::MODELS);
    let (mdl, mi) = model_at(&mut m, d, [0.0; 3]);
    let (a, ai) = model_at(&mut m, d, [1.0, 0.0, 0.0]);
    let (b, bi) = model_at(&mut m, d, [3.0, 4.0, 0.0]);
    let (c, ci) = model_at(&mut m, d, [2.0, 2.0, 6.0]);
    let args = [(models, ai, a, 1, 0), (models, bi, b, 1, 0), (models, ci, c, 1, 0)];
    let n = node(&mut m, models, 1, mi, mdl, 0x7f, &args);
    let (mut t, log) = setters();
    constr::anim_cns_2pts_update(&mut m, &mut t, n, 0.0);
    assert_eq!(seen(&log), [[2.0, 2.0, 0.0]]);
    // the x axis along b − a
    assert_eq!(t.calls[1].name, "tan_constr_map_direction");
    assert_eq!(t.calls[1].args[1..], [Arg::f(2.0), Arg::f(4.0), Arg::f(0.0)]);

    // three points: 0.333333 × the sum, which is not the sum divided by three
    let (mut t, log) = setters();
    constr::anim_cns_3pts_update(&mut m, &mut t, n, 0.0);
    let third = 0.333333f64;
    assert_eq!(seen(&log), [[(third * 6.0) as f32, (third * 6.0) as f32, (third * 6.0) as f32]]);
    assert_ne!(seen(&log)[0][0], 2.0);
    assert_eq!(t.calls[1].name, "constr3pts_map_direction");
}

#[test]
fn trajectory_takes_the_point_of_a_whole_frame() {
    let (mut m, d) = world();
    let models = glob::chap(&m, cid::MODELS);
    let (mdl, mi) = model_at(&mut m, d, [0.0; 3]);
    let (path, pi) = model_at(&mut m, d, [0.0; 3]);
    m.set_i32(path + model::KIND, 6);
    m.set_matrix(path + model::GMAT, &si_math::types::IDENTITY);
    // a spline of three points, (10 k, k, 0)
    let spl = m.si_calloc(1, 0x74);
    let pts = m.si_calloc(3, 0x30);
    m.set_u16(spl + 0x10, 3);
    m.set_ptr(spl + 0x14, pts);
    for k in 0..3u32 {
        m.set_f32(pts + 0x30 * k + 0xc, 10.0 * k as f32);
        m.set_f32(pts + 0x30 * k + 0x10, k as f32);
    }
    m.set_ptr(path + model::DEFINITION, spl);
    m.set_i32(glob::SI_FRAME_START, 10);
    m.set_f32(glob::SI_FRAME_RATE, 32.0);
    let n = node(&mut m, models, 1, mi, mdl, 0xa0, &[(models, pi, path, 1, 0)]);
    // frame 11 at 32 frames a second: point 1; no father, so the point is the translation
    let mut t = Trace::new();
    constr::anim_trajectory_update(&mut m, &mut t, n, 11.0 / 32.0);
    assert!(t.calls.is_empty());
    assert_eq!(m.vector(mdl + model::TRANS).to_array(), [10.0, 1.0, 0.0]);
    assert_eq!(m.u8(mdl + model::UPDATE_FLAGS) & 8, 8);
    // far past the end: the last point; between frames: the spline is evaluated
    constr::anim_trajectory_update(&mut m, &mut t, n, 3.0);
    assert_eq!(m.vector(mdl + model::TRANS).to_array(), [20.0, 2.0, 0.0]);
    constr::anim_trajectory_update(&mut m, &mut t, n, 11.5 / 32.0);
    assert_eq!(t.names(), ["spline_value"]);
    assert_eq!(t.calls[0].args[1], Arg::f(1.5));
}

#[test]
fn shape_keys_and_fraction() {
    let (mut m, d) = world();
    let models = glob::chap(&m, cid::MODELS);
    let shapes = glob::chap(&m, cid::SHAPES);
    let (mdl, mi) = model_at(&mut m, d, [0.0; 3]);
    m.set_i32(mdl + model::KIND, 4);
    // a working mesh and four keys
    let defs = m.si_calloc(5, 4);
    for k in 0..5u32 {
        m.set_ptr(defs + 4 * k, 0x9000 + 0x100 * k);
    }
    m.set_u16(mdl + model::NB_DEFS, 5);
    m.set_ptr(mdl + model::DEFS, defs);
    m.set_ptr(mdl + model::DEFINITION, 0x9200);
    let (se, si) = scene::add_elem(&mut m, d, shapes, mdl);
    let curve = fcv::make(&mut m, 8, 1, &[(0.0, 0.0), (1.0, 4.0)]);
    scene::attach_curve(&mut m, d, shapes, si, 0, 0xc9, 0, curve);
    let n = node(&mut m, models, 2, mi, mdl, 200, &[]);
    let host = move || Trace::with(move |_, name, _| if name == "ed_get_elem_from_shape" { Ret::ptr(se) } else { Ret::default() });
    // linear, value 1.25: keys 2 and 3, fraction 0.25, into the working mesh
    let mut t = host();
    shape::anim_shape_update(&mut m, &mut t, n, 0.3125);
    assert_eq!(m.ptr(mdl + model::DEFINITION), 0x9000, "the working pointer is put back on entry 0");
    let call = t.calls.iter().find(|c| c.name == "mesh_interpolate").unwrap();
    assert_eq!(call.args, [Arg::I(0x9200), Arg::I(0x9300), Arg::f(0.25), Arg::I(0x9000)]);
    assert_eq!(t.calls.last().unwrap().name, "model_touch_definition");
    // cardinal, past the last key: both middle keys are the last one
    m.set_i16(mdl + model::SHAPE_INTERP, 1);
    let mut t = host();
    shape::anim_shape_update(&mut m, &mut t, n, 5.0);
    let call = t.calls.iter().find(|c| c.name == "mesh_cardinal_interpolate").unwrap();
    assert_eq!(call.args, [Arg::I(0x9300), Arg::I(0x9400), Arg::I(0x9400), Arg::I(0x9400), Arg::f(0.0), Arg::I(0x9000)]);
}

fn text(m: &mut Mem, s: &str) -> Addr {
    let a = m.si_calloc(1, s.len() as u32 + 1);
    m.set_bytes(a, s.as_bytes());
    a
}

#[test]
fn sequence_and_script_names() {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    let tex = m.si_calloc(1, 0x1d8);
    let name = text(&mut m, "wall.12.pic");
    m.set_ptr(tex + texfile::tex::FILE, name);
    // the stand-in for SI_stripPicExtension answers its argument and leaves the text alone
    let mut t = Trace::with(|_, name, args| match (name, args.first()) {
        ("strip_pic_extension", Some(Arg::I(p))) => Ret::ptr(*p),
        _ => Ret::default(),
    });
    assert_eq!(texfile::is_sequence_up_to_date(&mut m, &mut t, tex, 7), 0);
    assert_eq!(m.cstr(m.ptr(tex + texfile::tex::FILE)), b"wall.12.7");
    assert_eq!(texfile::is_sequence_up_to_date(&mut m, &mut t, tex, 0), 1);

    // a script that is switched off: the place-holder picture, new the first time only
    let res = text(&mut m, "/res");
    m.set_ptr(texfile::SI_RESDIR, res);
    assert_eq!(texfile::is_script_up_to_date(&mut m, &mut t, tex, 3), 0);
    assert_eq!(m.cstr(m.ptr(tex + texfile::tex::PICTURE)), b"/res/noIcon");
    assert_eq!(texfile::is_script_up_to_date(&mut m, &mut t, tex, 3), 1);
}

#[test]
fn save_key_on_a_model() {
    let (mut m, d) = world();
    let models = glob::chap(&m, cid::MODELS);
    let (mdl, mi) = model_at(&mut m, d, [0.0; 3]);
    m.set_i32(glob::SI_FRAME_CURRENT, 15);
    // a y-scaling curve exists; the x-scaling curve does not
    let sy = fcv::make(&mut m, 2, 1, &[(0.0, 1.0)]);
    scene::attach_curve(&mut m, d, models, mi, 1, 100, 3, sy);
    let mut t = Trace::new();
    savekey::savekey_model(&mut m, &mut t, mdl, 1, 100, 2, 1, 2.5, 0, 0);
    assert_eq!(fcv::nb_keys(&m, sy), 2);
    let k = m.ptr(sy + fo::KEYS) + ko::SIZE;
    assert_eq!((m.f32(k + ko::TIME), m.f32(k + ko::VALUE)), (0.5, 2.5));
    assert_eq!(m.u8(mdl + model::UPDATE_FLAGS), 2);
    assert!(!t.names().contains(&"ed_fcv_add"));
    // type 1 with the check: the y curve is not taken, a new curve is made and filed
    let mut t = Trace::new();
    savekey::savekey_model(&mut m, &mut t, mdl, 1, 100, 1, 1, 4.0, 0, 0);
    let add = t.calls.iter().find(|c| c.name == "ed_fcv_add").unwrap();
    let Arg::I(new) = add.args[0] else { panic!() };
    assert_eq!((fcv::kind(&m, new), fcv::nb_keys(&m, new)), (1, 1));
    assert_eq!(fcv::nb_keys(&m, sy), 2);
    // a rotation key of a node under a direction constraint: the x angle kept at +0x164, in
    // degrees, whatever value is passed in
    m.set_f32(mdl + model::VROT_XY_COPY, std::f32::consts::FRAC_PI_2);
    let mut t = Trace::with(|_, name, _| if name == "ed_is_item_constrained" { Ret::int(1) } else { Ret::default() });
    savekey::savekey_model(&mut m, &mut t, mdl, 1, 0x65, 4, 1, 123.0, 0, 0);
    let add = t.calls.iter().find(|c| c.name == "ed_fcv_add").unwrap();
    let Arg::I(rx) = add.args[0] else { panic!() };
    assert_eq!(m.f32(m.ptr(rx + fo::KEYS) + ko::VALUE), 90.0);
}
