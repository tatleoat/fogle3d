// SPDX-License-Identifier: MIT
//! Unit tests of the update on made-up memory. Expected values are worked by hand from the
//! formulas in the doc comments (finding 0017).

use super::build::{curve_node, node};
use super::params::o::{camera, control, light, material, tex2d, tex3d};
use super::*;
use crate::mem::MemExt;
use crate::ed::build as edb;
use crate::host::{Arg, Ret, Trace};
use crate::off::model as mo;

const DEG: f64 = 0.0174532925;

fn rad(deg: f32) -> f32 {
    (F80::from_f64(DEG) * deg).to_f32()
}

/// A scene, and an object of `size` zero bytes registered as element 0 of chapter `which`.
fn scene_with(which: u32, size: u32) -> (Mem, Addr, i32, Addr) {
    let mut m = Mem::new();
    let d = edb::scene(&mut m);
    let obj = m.si_calloc(1, size);
    let chap = glob::chap(&m, which);
    let (e, _) = edb::add_elem(&mut m, d, chap, obj);
    (m, obj, chap, e)
}

fn lin(m: &mut Mem, kind: i32, keys: &[(f32, f32)]) -> Addr {
    fcv::make(m, kind, 1, keys)
}

#[test]
fn scaling_rotation_translation_write_the_local_transformation() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let mut t = Trace::new();
    let sx = lin(&mut m, 1, &[(0.0, 1.0), (1.0, 3.0)]);
    let sz = lin(&mut m, 3, &[(0.0, 2.0), (1.0, 2.0)]);
    let empty = lin(&mut m, 2, &[]);
    let n = curve_node(&mut m, models, 1, 0, mdl, 100, &[sx, empty, sz]);
    transfo::anim_scaling_update(&mut m, &mut t, n, 0.5);
    assert_eq!([m.f32(mdl + 0x100), m.f32(mdl + 0x104), m.f32(mdl + 0x108)], [2.0, 1.0, 2.0]);
    assert_eq!((m.u8(mdl + 0xa), m.u8(mdl + 0x3fa)), (2, 2));

    let ry = lin(&mut m, 5, &[(0.0, 90.0), (1.0, 90.0)]);
    let n = curve_node(&mut m, models, 1, 0, mdl, 101, &[ry]);
    transfo::anim_rotation_update(&mut m, &mut t, n, 0.5);
    assert_eq!(m.f32(mdl + 0x110), rad(90.0));
    assert_eq!(m.f32(mdl + 0x168), rad(90.0));
    assert_eq!(m.f32(mdl + 0x10c), 0.0);
    assert_eq!(m.u8(mdl + 0xa), 6);

    let tx = lin(&mut m, 139, &[(0.0, 5.0), (1.0, 7.0)]);
    m.set_f32(mdl + 0x15c, 11.0);
    let n = curve_node(&mut m, models, 1, 0, mdl, 113, &[tx]);
    transfo::anim_explicit_pos_update(&mut m, &mut t, n, 0.5);
    assert_eq!([m.f32(mdl + 0x158), m.f32(mdl + 0x15c), m.f32(mdl + 0x160)], [6.0, 11.0, 0.0]);
    assert_eq!(m.u8(mdl + 0xa), 14);
    assert_eq!(t.names(), ["ed_is_item_constrained"]);
    assert_eq!(t.calls[0].args[3], Arg::I(0x6f));
    assert!(m.fault().is_none());
}

#[test]
fn translation_yields_to_a_constraint_and_to_a_path() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let tx = lin(&mut m, 139, &[(1.0, 5.0), (2.0, 7.0)]);
    let n = curve_node(&mut m, models, 1, 0, mdl, 113, &[tx]);
    // constrained: nothing written, no flag
    let mut t = Trace::with(|_, name, _| if name == "ed_is_item_constrained" { Ret::int(1) } else { Ret::default() });
    transfo::anim_explicit_pos_update(&mut m, &mut t, n, 1.5);
    assert_eq!((m.f32(mdl + 0x158), m.u8(mdl + 0xa)), (0.0, 0));
    // on a path, before the curve's first key: the path keeps the node, nobody is asked
    m.set_ptr(mdl + mo::PATH, 0x1234);
    let mut t = Trace::new();
    transfo::anim_explicit_pos_update(&mut m, &mut t, n, 0.5);
    assert!(t.calls.is_empty());
    assert_eq!(m.f32(mdl + 0x158), 0.0);
    // on a path, within the keys: the curve wins
    transfo::anim_explicit_pos_update(&mut m, &mut t, n, 2.0);
    assert_eq!(m.f32(mdl + 0x158), 7.0);
}

#[test]
fn joint_rotation_resets_the_preferred_angle() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    m.set_i32(mdl + mo::KIND, 5);
    m.set_i32(mdl + mo::ALGO_TYPE, 1);
    let rz = lin(&mut m, 6, &[(0.0, 10.0)]);
    let n = curve_node(&mut m, models, 1, 0, mdl, 101, &[rz]);
    let mut t = Trace::new();
    transfo::anim_rotation_update(&mut m, &mut t, n, 0.0);
    assert_eq!(t.names(), ["inv_kin_reset_pref_angle"]);
    assert_eq!(t.calls[0].args, [Arg::I(mdl), Arg::I(3), Arg::f(0.0), Arg::f(0.0), Arg::f(rad(10.0))]);
}

#[test]
fn model_parameters() {
    let (mut m, mdl, models, e) = scene_with(cid::MODELS, 0x430);
    let dm = m.si_calloc(1, 0x20);
    m.set_ptr(mdl + mo::DYNA_MATTER, dm);
    let chn = m.si_calloc(1, 0x60);
    m.set_ptr(mdl + mo::DYNA_CHN_VARS, chn);
    let mut t = Trace::new();
    let c = lin(&mut m, 371, &[(0.0, 0.25)]);
    let n = curve_node(&mut m, models, 0, 0, mdl, 15, &[c]);
    params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.f32(mdl + 0x3f4), 0.25);
    for (rel, kind, off) in [(50, 179, 0x14), (51, 180, 0x0c), (52, 181, 0x10)] {
        let c = lin(&mut m, kind, &[(0.0, rel as f32)]);
        let n = curve_node(&mut m, models, 0, 0, mdl, rel, &[c]);
        params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
        assert_eq!(m.f32(dm + off), rel as f32);
    }
    let c = lin(&mut m, 182, &[(0.0, 1.9)]);
    let n = curve_node(&mut m, models, 0, 0, mdl, 53, &[c]);
    params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.i16(chn + 0x38), 1); // truncated
    for (rel, at, mask) in [(4000, 0x404, 1u8), (4001, 0x404, 2), (4002, 0x406, 1), (4003, 0x406, 2)] {
        let c = lin(&mut m, 620, &[(0.0, 1.0), (1.0, 0.0)]);
        let n = curve_node(&mut m, models, 0, 0, mdl, rel, &[c]);
        params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
        assert_eq!(m.u8(mdl + at) & mask, mask);
        params::anim_model_parameters_update(&mut m, &mut t, n, 1.0);
        assert_eq!(m.u8(mdl + at) & mask, 0);
    }
    assert!(t.calls.is_empty());
    // visibility: the node (10) and the branch (12)
    let c = fcv::make(&mut m, 175, 0, &[(0.0, 1.0), (1.0, 0.0)]);
    let n = curve_node(&mut m, models, 0, 0, mdl, 10, &[c]);
    params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
    params::anim_model_parameters_update(&mut m, &mut t, n, 1.0);
    let nb = curve_node(&mut m, models, 0, 0, mdl, 12, &[c]);
    params::anim_model_parameters_update(&mut m, &mut t, nb, 0.0);
    let hidden: Vec<_> = t.calls.iter().filter(|c| c.name == "model_set_hidden_param").map(|c| c.args.clone()).collect();
    assert_eq!(hidden[0], [Arg::I(0), Arg::I(mdl), Arg::I(e), Arg::I(2), Arg::I(0)]);
    assert_eq!(hidden[1][3..], [Arg::I(0), Arg::I(0)]);
    assert_eq!(hidden[2][3..], [Arg::I(2), Arg::I(1)]);
    assert_eq!(t.calls[0].name, "find_gl_data");
    assert_eq!(t.calls[0].args[2], Arg::I(2));
    assert!(m.fault().is_none());
}

#[test]
fn hidden_for_matte_sets_the_alpha_bits() {
    let (mut m, mdl, models, e) = scene_with(cid::MODELS, 0x430);
    m.set_i16(glob::SI_USE_HIDDEN_FOR_MATTE, 1);
    let mut t = Trace::new();
    for (v, state, bits) in [(0.25f32, 0u32, 0u32), (1.0, 2, 0), (2.0, 2, 0x80), (3.0, 2, 0x100)] {
        t.calls.clear();
        m.set_u32(e + 0x20, 0x182);
        let c = fcv::make(&mut m, 175, 0, &[(0.0, v)]);
        let n = curve_node(&mut m, models, 0, 0, mdl, 10, &[c]);
        params::anim_model_parameters_update(&mut m, &mut t, n, 0.0);
        let set = t.calls.iter().find(|c| c.name == "model_set_hidden_param").unwrap();
        assert_eq!(set.args[3..], [Arg::I(state), Arg::I(1)], "value {v}");
        assert_eq!(m.u32(e + 0x20), 2 | bits, "value {v}");
        assert_eq!(t.names().last(), Some(&"elemdict_find_son"));
    }
}

#[test]
fn camera_parameters() {
    let (mut m, cam, cameras, _) = scene_with(cid::CAMERAS, 0x13c);
    let plain = [(1132, 13, camera::NEAR), (1133, 14, camera::FAR), (1134, 15, camera::FOCAL_LENGTH), (1135, 16, camera::F_STOP),
                 (1136, 17, camera::FOCAL_DISTANCE), (1137, 18, camera::NEAR_FOCUS), (1138, 19, camera::FAR_FOCUS),
                 (1139, 20, camera::OUT_OF_FOCUS), (1140, 21, camera::MAXIMUM_COC)];
    for (rel, kind, off) in plain {
        let c = lin(&mut m, kind, &[(0.0, rel as f32 + 0.5)]);
        let n = curve_node(&mut m, cameras, 0, 0, cam, rel, &[c]);
        params::anim_camera_parameters_update(&mut m, n, 0.0);
        assert_eq!(m.f32(cam + off), rel as f32 + 0.5, "relation {rel}");
    }
    let c = lin(&mut m, 11, &[(0.0, 30.0)]);
    let n = curve_node(&mut m, cameras, 1, 0, cam, 1130, &[c]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert_eq!(m.f32(cam + camera::ROLL), rad(30.0));
    // field of view 90 degrees with a plane 2 high: the plane is 1 away
    m.set_f32(cam + camera::PP_HEIGHT, 2.0);
    let c = lin(&mut m, 12, &[(0.0, 90.0)]);
    let n = curve_node(&mut m, cameras, 0, 0, cam, 1131, &[c]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert_eq!(m.f32(cam + camera::FOV), rad(90.0));
    assert!((m.f32(cam + camera::PP_DISTANCE) - 1.0).abs() < 1e-6);
    // a field of view of zero is pushed to 0.0001 radians
    let c = lin(&mut m, 12, &[(0.0, 0.0)]);
    let n = curve_node(&mut m, cameras, 0, 0, cam, 1131, &[c]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert_eq!(m.f32(cam + camera::FOV).to_bits(), 0x38d1_b717);
    // plane distance 1, height 2: 90 degrees again
    let c = lin(&mut m, 373, &[(0.0, 1.0)]);
    let n = curve_node(&mut m, cameras, 0, 0, cam, 1141, &[c]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert!((m.f32(cam + camera::FOV) - core::f32::consts::FRAC_PI_2).abs() < 1e-6);
    // size and offset by curve type; a negative near-zero width keeps its sign
    let w = lin(&mut m, 374, &[(0.0, -0.00001)]);
    let hh = lin(&mut m, 375, &[(0.0, 4.0)]);
    let ox = lin(&mut m, 376, &[(0.0, 0.5)]);
    let oy = lin(&mut m, 377, &[(0.0, -0.5)]);
    let n = curve_node(&mut m, cameras, 0, 0, cam, 1142, &[w, hh, ox, oy]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert_eq!(m.f32(cam + camera::PP_WIDTH).to_bits(), 0xb8d1_b717);
    assert_eq!(m.f32(cam + camera::PP_HEIGHT), 4.0);
    assert_eq!((m.f32(cam + camera::PP_OFFSET_X), m.f32(cam + camera::PP_OFFSET_Y)), (0.5, -0.5));
    let want = (trig::fpatan(F80::from_f32(4.0) * 0.5f64 / 1.0f32, F80::ONE) * 2.0f32).to_f32();
    assert_eq!(m.f32(cam + camera::FOV), want);
    let c = fcv::make(&mut m, 378, 0, &[(0.0, 1.0)]);
    let n = curve_node(&mut m, cameras, 0, 0, cam, 1144, &[c]);
    params::anim_camera_parameters_update(&mut m, n, 0.0);
    assert_eq!(m.i16(cam + camera::PP_ACTIVE), 1);
    // position and interest
    let x = lin(&mut m, 142, &[(0.0, 1.0)]);
    let z = lin(&mut m, 144, &[(0.0, 3.0)]);
    m.set_f32(cam + camera::POSITION + 4, 9.0);
    let n = curve_node(&mut m, cameras, 1, 0, cam, 1113, &[x, z]);
    params::anim_camera_explicit_pos_update(&mut m, n, 0.0);
    assert_eq!(m.vector(cam + camera::POSITION).to_array(), [1.0, 9.0, 3.0]);
    let y = lin(&mut m, 146, &[(0.0, -2.0)]);
    let n = curve_node(&mut m, cameras, 1, 0, cam, 1115, &[y]);
    params::anim_camera_explicit_pos_update(&mut m, n, 0.0);
    assert_eq!(m.vector(cam + camera::INTEREST).to_array(), [0.0, -2.0, 0.0]);
    assert!(m.fault().is_none());
}
use si_x87::trig;

#[test]
fn light_parameters() {
    let (mut m, lig, lights, _) = scene_with(cid::LIGHTS, 0x240);
    let mut t = Trace::new();
    let (r, g, b) = (lin(&mut m, 22, &[(0.0, 0.1)]), lin(&mut m, 23, &[(0.0, 0.2)]), lin(&mut m, 24, &[(0.0, 0.3)]));
    let n = curve_node(&mut m, lights, 0, 0, lig, 2130, &[b, r, g]);
    params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!([m.f32(lig + 0x24), m.f32(lig + 0x28), m.f32(lig + 0x2c)], [0.1, 0.2, 0.3]);
    for (rel, kind, off, deg) in [(2131, 25, light::CONE_ANGLE, true), (2132, 26, light::SPREAD_ANGLE, true),
                                  (2134, 28, light::FALL_OFF_START, false), (2135, 29, light::FALL_OFF_END, false),
                                  (2136, 30, light::SHADOW_FACTOR, false)]
    {
        let c = lin(&mut m, kind, &[(0.0, 40.0)]);
        let n = curve_node(&mut m, lights, 0, 0, lig, rel, &[c]);
        params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
        assert_eq!(m.f32(lig + off), if deg { rad(40.0) } else { 40.0 }, "relation {rel}");
    }
    // energy and photons: nothing below zero
    let (er, eg) = (lin(&mut m, 621, &[(0.0, -5.0)]), lin(&mut m, 622, &[(0.0, 5.0)]));
    m.set_f32(lig + light::ENERGY, 1.0);
    let n = curve_node(&mut m, lights, 0, 0, lig, 2203, &[er, eg]);
    params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!((m.f32(lig + light::ENERGY), m.f32(lig + light::ENERGY + 4)), (0.0, 5.0));
    let c = fcv::make(&mut m, 625, 0, &[(0.0, 1234.0)]);
    let n = curve_node(&mut m, lights, 0, 0, lig, 2205, &[c]);
    params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.i32(lig + light::CAUSTIC_PHOTON), 1234);
    let c = fcv::make(&mut m, 627, 0, &[(0.0, 1.0)]);
    let n = curve_node(&mut m, lights, 0, 0, lig, 2207, &[c]);
    params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.u16(lig + light::CAUSTIC_ENABLE), 1);
    assert!(t.calls.is_empty());
    // the x scale of a disk area light is its radius, and the disk is rebuilt
    m.set_i16(lig + light::AREA_LIGHT, 1);
    m.set_i32(lig + light::AREA_TYPE, 2);
    let c = lin(&mut m, 218, &[(0.0, 2.5)]);
    let n = curve_node(&mut m, lights, 0, 0, lig, 2202, &[c]);
    params::anim_light_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!((m.f32(lig + light::AREA_SCALE), m.f32(lig + light::AREA_DISK_RADIUS)), (2.5, 2.5));
    assert_eq!(t.names(), ["set_area_disk"]);
    // position
    let y = lin(&mut m, 149, &[(0.0, 8.0)]);
    let n = curve_node(&mut m, lights, 1, 0, lig, 2113, &[y]);
    params::anim_light_explicit_pos_update(&mut m, n, 0.0);
    assert_eq!(m.vector(lig + light::POSITION).to_array(), [0.0, 8.0, 0.0]);
    assert!(m.fault().is_none());
}

#[test]
fn sun_time_is_truncated_and_moves_the_light() {
    let (mut m, lig, lights, _) = scene_with(cid::LIGHTS, 0x240);
    let sun = m.si_calloc(1, 0x20);
    m.set_ptr(lig + light::SUN, sun);
    m.set_i32(sun + 0x10, 500);
    let c = lin(&mut m, 189, &[(0.0, 1000.9)]);
    let n = curve_node(&mut m, lights, 1, 0, lig, 2120, &[c]);
    let mut t = Trace::with(|m, name, args| {
        if name == "light_sun_get_position" {
            let Arg::I(out) = args[1] else { panic!() };
            m.set_vector(out, si_math::types::Vector::new(1.0, 2.0, 3.0));
        }
        Ret::default()
    });
    params::anim_light_sun_time_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.i32(sun + 0x10), 1000);
    assert_eq!(m.vector(lig + light::POSITION).to_array(), [1.0, 2.0, 3.0]);
}

#[test]
fn constraint_averages_the_active_objects() {
    let (mut m, cam, cameras, _) = scene_with(cid::CAMERAS, 0x13c);
    let models = glob::chap(&m, cid::MODELS);
    let (a, b, c) = (m.si_calloc(1, 0x430), m.si_calloc(1, 0x430), m.si_calloc(1, 0x430));
    m.set_vector(a + mo::GTRS, si_math::types::Vector::new(1.0, 0.0, 0.0));
    m.set_vector(b + mo::GTRS, si_math::types::Vector::new(0.0, 2.0, 0.0));
    m.set_vector(c + mo::GTRS, si_math::types::Vector::new(100.0, 100.0, 100.0));
    let off = fcv::make(&mut m, 196, 0, &[(0.0, 0.0)]);
    let args = [(models, 0, a, 1, 0), (models, 1, b, 1, 0), (models, 2, c, 1, off)];
    let n = node(&mut m, cameras, 1, 0, cam, 1116, &args);
    for i in 0..3 {
        let at = arg(&m, n, i);
        m.set_i16(at + updelem::GROUP, 1);
    }
    // the node's element is a camera interest (kind 5), the arguments are models (kind 2)
    let mut first = true;
    let mut t = Trace::with(move |_, name, _| {
        if name == "db_chapter2graf_type" {
            let k = if first { 5 } else { 2 };
            first = false;
            return Ret::int(k);
        }
        Ret::default()
    });
    params::anim_camera_constraint_update(&mut m, &mut t, n, 0.0);
    let half = (F80::ONE / F80::from_i32(2)) * 1.0f32;
    assert_eq!(m.vector(cam + camera::INTEREST).to_array(), [half.to_f32(), 1.0, 0.0]);
    assert_eq!(m.vector(cam + camera::POSITION).to_array(), [0.0, 0.0, 0.0]);
}

#[test]
fn material_and_texture_parameters() {
    let (mut m, mat, mats, _) = scene_with(cid::MATERIALS, 0xdcc);
    let (r, b) = (lin(&mut m, 48, &[(0.0, 0.5)]), lin(&mut m, 50, &[(0.0, 0.75)]));
    m.set_f32(mat + material::DIFFUSE + 4, 0.25);
    let n = curve_node(&mut m, mats, 0, 0, mat, 302, &[r, b]);
    params::anim_mat_parameters_update(&mut m, n, 0.0);
    assert_eq!([m.f32(mat + 0x44), m.f32(mat + 0x48), m.f32(mat + 0x4c)], [0.5, 0.25, 0.75]);
    for (rel, kind, off, clamp) in [(304, 54, material::SPEC_DECAY, true), (305, 55, material::REFLECT, true),
                                    (306, 56, material::REFLECT_JITTER, true), (307, 57, material::TRANSP, true),
                                    (308, 58, material::TRANSP_JITTER, true), (310, 59, material::REFRACT_INDEX, true),
                                    (311, 257, material::BLUR_WIDTH, false), (312, 258, material::BLUR_DECAY, false)]
    {
        let c = lin(&mut m, kind, &[(0.0, -1.5), (1.0, 1.5)]);
        let n = curve_node(&mut m, mats, 0, 0, mat, rel, &[c]);
        params::anim_mat_parameters_update(&mut m, n, 1.0);
        assert_eq!(m.f32(mat + off), 1.5, "relation {rel}");
        params::anim_mat_parameters_update(&mut m, n, 0.0);
        assert_eq!(m.f32(mat + off), if clamp { 0.0 } else { -1.5 }, "relation {rel}");
    }
    assert!(m.fault().is_none());

    let (mut m, tex, t2d, _) = scene_with(cid::TEXTURES2D, 0x1d8);
    let mut t = Trace::new();
    let c = lin(&mut m, 44, &[(0.0, 45.0)]);
    let n = curve_node(&mut m, t2d, 0, 0, tex, 423, &[c]);
    params::anim_tex_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.f32(tex + tex2d::ROTATION), rad(45.0));
    assert_eq!(m.u8(tex + tex2d::DIRTY), 2);
    assert_eq!(t.names(), ["texture2d_set_dirty", "upd_txt2d_effects"]);
    // the support's rotation: the component without a curve is converted again
    m.set_f32(tex + tex2d::S_ROTATION + 4, 1.0);
    let c = lin(&mut m, 206, &[(0.0, 180.0)]);
    let n = curve_node(&mut m, t2d, 0, 0, tex, 424, &[c]);
    params::anim_tex_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.f32(tex + tex2d::S_ROTATION), rad(180.0));
    assert_eq!(m.f32(tex + tex2d::S_ROTATION + 4), rad(1.0));
    // scaling: a curve without keys is 1
    let c = lin(&mut m, 210, &[]);
    let n = curve_node(&mut m, t2d, 0, 0, tex, 425, &[c]);
    params::anim_tex_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.vector(tex + tex2d::S_SCALING).to_array(), [0.0, 1.0, 0.0]);

    let (mut m, tex, t3d, _) = scene_with(cid::TEXTURES3D, 0x158);
    let mut t = Trace::new();
    let c = lin(&mut m, 73, &[(0.0, -0.5), (1.0, 0.5), (2.0, 1.5)]);
    let n = curve_node(&mut m, t3d, 0, 0, tex, 521, &[c]);
    for (time, want) in [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0)] {
        params::anim_tex3d_parameters_update(&mut m, &mut t, n, time);
        assert_eq!(m.f32(tex + tex3d::FACTORS), want);
    }
    let (g, a) = (lin(&mut m, 80, &[(0.0, 0.5)]), lin(&mut m, 157, &[(0.0, -1.0)]));
    m.set_f32(tex + tex3d::COLORS + 16 + 12, 0.7);
    let n = curve_node(&mut m, t3d, 0, 0, tex, 517, &[g, a]);
    params::anim_tex3d_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!([m.f32(tex + 0x40), m.f32(tex + 0x44), m.f32(tex + 0x48), m.f32(tex + 0x4c)], [0.0, 0.5, 0.0, 0.0]);
    let c = lin(&mut m, 104, &[(0.0, -3.0)]);
    let n = curve_node(&mut m, t3d, 0, 0, tex, 530, &[c]);
    params::anim_tex3d_parameters_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.f32(tex + tex3d::TRANSP), -1.0);
    assert!(t.names().iter().all(|n| *n == "upd_txt3d_turb"));
    assert!(m.fault().is_none());
}

#[test]
fn fade_and_fog_go_to_the_render_control() {
    let mut m = Mem::new();
    edb::scene(&mut m);
    let scenes = glob::chap(&m, cid::SCENES);
    let (start, red) = (lin(&mut m, 60, &[(0.0, 10.0)]), lin(&mut m, 62, &[(0.0, 0.5)]));
    let n = curve_node(&mut m, scenes, 0, 0, 0x1234, 3000, &[start, red]);
    let mut t = Trace::new();
    upd_node_parameter_update(&mut m, &mut t, n, 0.0);
    assert_eq!(data(&m, n), glob::SI_DEFAULT_CONTROL);
    assert_eq!(m.f32(glob::SI_DEFAULT_CONTROL + control::FADE_START), 10.0);
    assert_eq!(m.f32(glob::SI_DEFAULT_CONTROL + control::FADE_RED), 0.5);
    assert_eq!(m.f32(params::SI_REN + control::FADE_START), 0.0);
    // asked for by a renderer: the renderer's own record follows
    m.set_u16(glob::FROM_RENDERING, 1);
    let dens = lin(&mut m, 203, &[(0.0, 0.25)]);
    let n = curve_node(&mut m, scenes, 0, 0, 0, 3032, &[dens]);
    upd_node_parameter_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.f32(glob::SI_DEFAULT_CONTROL + control::FOG_DENSITY), 0.25);
    assert_eq!(m.f32(params::SI_REN + control::FOG_DENSITY), 0.25);
    assert_eq!(data(&m, n), glob::SI_DEFAULT_CONTROL);
    assert!(m.fault().is_none());
}

#[test]
fn parameters_the_dispatcher_does_not_apply() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let mut t = Trace::new();
    let c = fcv::make(&mut m, 676, 0, &[(0.0, 1.0)]);
    let n = curve_node(&mut m, models, 0, 0, mdl, 4100, &[c]);
    let before = m.bytes(mdl, 0x430).to_vec();
    upd_node_parameter_update(&mut m, &mut t, n, 0.0);
    assert_eq!(m.bytes(mdl, 0x430), &before[..]);
    assert_eq!(m.f32(c + 0x10), -100000.0); // the curve was not even evaluated
}

#[test]
fn activation_curve_moves_the_time_back() {
    let mut m = Mem::new();
    edb::scene(&mut m);
    // on until 1 s, off from 1 s to 3 s (two keys of value 0), on again from 3 s
    let act = fcv::make(&mut m, 196, 0, &[(0.0, 1.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0)]);
    assert_eq!(activation_fcv_check_time(&m, act, 2.5), (1.0, false));
    assert_eq!(activation_fcv_check_time(&m, act, 0.5), (0.0, true));
    assert_eq!(activation_fcv_check_time(&m, act, -1.0), (0.0, true));
    assert_eq!(activation_fcv_check_time(&m, act, 9.0), (3.0, true));
    let (_, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let _ = (mdl, models);
}

#[test]
fn gate_and_node_dispatch() {
    let (mut m, mdl, models, e) = scene_with(cid::MODELS, 0x430);
    let gn = m.si_calloc(1, 0x24);
    m.set_ptr(e + elem::AUX_DATA, gn);
    m.set_i32(gn + 0xc, 2); // visible
    let sx = lin(&mut m, 1, &[(0.0, 1.0), (4.0, 5.0)]);
    let act = fcv::make(&mut m, 196, 0, &[(0.0, 1.0), (1.0, 0.0), (3.0, 1.0)]);
    let ani = glob::chap(&m, cid::ANIMATION);
    let n = node(&mut m, models, 1, 0, mdl, 100, &[(ani, 0, sx, 1, act)]);
    let mut t = Trace::new();
    // active: evaluated at the time asked
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), Some(0.5));
    // switched off at 1 s: the scaling freezes at its value of 1 s
    assert_eq!(check_active_time(&mut m, &mut t, n, 2.0), Some(1.0));
    let post = m.si_calloc(1, 4);
    upd_node_update(&mut m, &mut t, n, 2.0, post);
    assert_eq!(m.f32(mdl + 0x100), 2.0);
    // an inactive argument: all arguments inactive, the node is skipped
    let a0 = arg(&m, n, 0);
    m.set_i16(a0 + updelem::ACTIVE, 0);
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), None);
    m.set_i16(a0 + updelem::ACTIVE, 1);
    // an inactive node
    m.set_i16(n + updnode::ACTIVE, 0);
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), None);
    m.set_i16(n + updnode::ACTIVE, 1);
    // hidden and used by nobody: skipped; a visibility node passes all the same
    m.set_i32(gn + 0xc, 0);
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), None);
    assert_eq!(t.names().last(), Some(&"elemdict_is_elem_used"));
    let vis = fcv::make(&mut m, 175, 0, &[(0.0, 1.0)]);
    let nv = curve_node(&mut m, models, 0, 0, mdl, 10, &[vis]);
    assert_eq!(check_active_time(&mut m, &mut t, nv, 0.5), Some(0.5));
    // selected-only update: an unselected element is skipped
    m.set_i32(gn + 0xc, 2);
    m.set_i32(glob::SI_UPDATE_OPTION, 1);
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), None);
    m.set_i32(gn + 8, 1);
    assert_eq!(check_active_time(&mut m, &mut t, n, 0.5), Some(0.5));
    assert!(m.fault().is_none());
}

#[test]
fn update_list_runs_nodes_in_order_then_the_trailers() {
    let (mut m, mdl, models, e) = scene_with(cid::MODELS, 0x430);
    let gn = m.si_calloc(1, 0x24);
    m.set_ptr(e + elem::AUX_DATA, gn);
    m.set_i32(gn + 0xc, 2);
    let ani = glob::chap(&m, cid::ANIMATION);
    let sx = lin(&mut m, 1, &[(0.0, 2.0)]);
    let d = glob::cur_dict(&m);
    let list = d + dict::UPD_LIST;
    let nodes = m.si_calloc(4, updnode::SIZE);
    build::fill(&mut m, nodes, models, 1, 0, mdl, 100, &[(ani, 0, sx, 1, 0)]);
    build::fill(&mut m, nodes + updnode::SIZE, models, 1, 0, mdl, -100, &[]);
    build::fill(&mut m, nodes + 2 * updnode::SIZE, models, 2, 0, mdl, -200, &[]);
    m.set_i16(nodes + 3 * updnode::SIZE, -1);
    m.set_i32(list + updlist::NB, 3);
    m.set_ptr(list + updlist::NODES, nodes);
    m.set_i32(mdl + mo::KIND, 4);
    m.set_ptr(mdl + mo::DEFINITION, 0x7777);
    let mut t = Trace::new();
    upd_list_update(&mut m, &mut t, list, 0.0);
    assert_eq!(m.f32(mdl + 0x100), 2.0);
    assert_eq!(t.names(),
               ["cus_pre_scene_transfer", "model_update_son_state", "mesh_computes_polygon_normal", "cus_scene_update",
                "env_skel_reset_flag"]);
    assert_eq!(t.calls[1].args, [Arg::I(0), Arg::I(mdl), Arg::I(1)]);
    assert_eq!(upd_list_get_current(&m), list);
    assert_eq!(m.i16(glob::SI_GLB_ANIM + anim::IN_UPDATE), 0);
    // the whole-scene entry: slider first, then the sort, then the list
    t.calls.clear();
    m.set_i32(glob::SI_FRAME_CURRENT, 0);
    scene_update(&mut m, &mut t, 0.5);
    // the scene's list is still dirty from creation: it is sorted (the model has no
    // relation, so it comes out empty), then run
    assert_eq!(t.names(),
               ["fslider_draw", "db_check_extra_un_told_relation", "db_check_extra_un_told_relation", "cus_resort_upd_list",
                "getbutton", "cus_pre_scene_transfer", "cus_scene_update", "env_skel_reset_flag"]);
    assert_eq!(t.calls[0].args, [Arg::I(15)]);
    // refresh off: only the slider
    t.calls.clear();
    set_refresh_obj(&mut m, 0);
    scene_update(&mut m, &mut t, -0.5);
    assert_eq!(t.names(), ["fslider_draw"]);
    assert_eq!(t.calls[0].args, [Arg::I(-15i32 as u32)]);
    assert!(m.fault().is_none());
}

#[test]
fn fast_hierarchy_update_skips_sons_with_their_own_step() {
    let (mut m, root, models, _) = scene_with(cid::MODELS, 0x430);
    let (a, b) = (m.si_calloc(1, 0x430), m.si_calloc(1, 0x430));
    m.set_ptr(root + mo::SON, a);
    m.set_ptr(a + mo::BROTHER, b);
    m.set_ptr(a + mo::FATHER, root);
    m.set_ptr(b + mo::FATHER, root);
    let nodes = m.si_calloc(3, updnode::SIZE);
    build::fill(&mut m, nodes, models, 1, 0, root, -100, &[]);
    build::fill(&mut m, nodes + updnode::SIZE, models, 1, 1, b, -100, &[]);
    m.set_i16(nodes + 2 * updnode::SIZE, -1);
    m.set_u8(glob::SI_VIEWER_UPD_OPT, 4);
    let mut t = Trace::new();
    transfo::anim_transfo_update(&mut m, &mut t, nodes, 0.0);
    let got: Vec<_> = t.calls.iter().map(|c| (c.args[1], c.args[2])).collect();
    assert_eq!(got, [(Arg::I(root), Arg::I(0)), (Arg::I(a), Arg::I(0))]);
}

#[test]
fn path_places_models_cameras_and_lights() {
    let (mut m, mdl, models, e) = scene_with(cid::MODELS, 0x430);
    let d = glob::cur_dict(&m);
    let path = m.si_calloc(1, 0x430);
    edb::add_elem(&mut m, d, models, path);
    m.set_i32(path + mo::KIND, 6);
    m.set_ptr(path + mo::DEFINITION, 0x5555);
    let mut g = si_math::types::IDENTITY;
    g[3] = [10.0, 20.0, 30.0, 1.0];
    m.set_matrix(path + mo::GMAT, &g);
    let perc = lin(&mut m, 7, &[(0.0, 0.0), (1.0, 100.0)]);
    let ani = glob::chap(&m, cid::ANIMATION);
    let n = node(&mut m, models, 1, 0, mdl, 112, &[(models, 1, path, 1, 0), (ani, 0, perc, 1, 0)]);
    let respond = |m: &mut Mem, name: &'static str, args: &[Arg]| match name {
        "spline_length" => Ret::float(F80::from_f32(8.0)),
        "spline_distance_to_factor" => {
            let Arg::F(d) = args[1] else { panic!() };
            Ret::float(F80::from_f32(f32::from_bits(d) / 8.0))
        }
        "spline_value" => {
            let (Arg::F(f), Arg::I(out)) = (args[1], args[2]) else { panic!() };
            m.set_vector(out, si_math::types::Vector::new(f32::from_bits(f), 2.0, 3.0));
            Ret::default()
        }
        _ => Ret::default(),
    };
    let mut t = Trace::with(respond);
    transfo::anim_all_type_path_update(&mut m, &mut t, n, 0.5);
    // half way: distance 8 × (0.01 × 50), factor a half; no father: world coordinates
    let dist = (F80::from_f32(8.0) * (F80::from_f64(0.01) * 50.0f32)).to_f32();
    assert_eq!(m.vector(mdl + mo::TRANS).to_array(), [dist / 8.0 + 10.0, 22.0, 33.0]);
    assert_eq!(t.names(), ["spline_length", "spline_distance_to_factor", "spline_value"]);
    assert_eq!(m.u8(mdl + 0xa), 0); // no update flag from the path
    let _ = e;

    // a camera on the same path: world position
    let cam = m.si_calloc(1, 0x13c);
    let cameras = glob::chap(&m, cid::CAMERAS);
    let n = node(&mut m, cameras, 1, 0, cam, 1114, &[(models, 1, path, 1, 0), (ani, 0, perc, 1, 0)]);
    let mut t = Trace::with(respond);
    transfo::anim_all_type_path_update(&mut m, &mut t, n, 1.0);
    assert_eq!(m.vector(cam + camera::INTEREST).to_array(), [11.0, 22.0, 33.0]);
    assert_eq!(m.vector(cam + camera::POSITION).to_array(), [0.0, 0.0, 0.0]);
    assert!(m.fault().is_none());
}

#[test]
fn path_point_in_the_fathers_space() {
    let mut m = Mem::new();
    edb::scene(&mut m);
    let (mdl, path, father, pt) = (m.si_calloc(1, 0x430), m.si_calloc(1, 0x430), m.si_calloc(1, 0x430), m.si_calloc(1, 12));
    m.set_ptr(mdl + mo::FATHER, father);
    m.set_ptr(path + mo::FATHER, father);
    // same father: the path model's own scaling, rotation and translation
    m.set_matrix(path + mo::MROT, &si_math::types::IDENTITY);
    m.set_vector(path + mo::SCAL, si_math::types::Vector::new(2.0, 3.0, 4.0));
    m.set_vector(path + mo::TRANS, si_math::types::Vector::new(1.0, 1.0, 1.0));
    m.set_vector(pt, si_math::types::Vector::new(1.0, 1.0, 1.0));
    transfo::model_compute_path_translation(&mut m, mdl, path, pt);
    assert_eq!(m.vector(pt).to_array(), [3.0, 4.0, 5.0]);
    // different fathers: world point, then the inverse of the father's matrix
    let other = m.si_calloc(1, 0x430);
    m.set_ptr(path + mo::FATHER, other);
    let mut g = si_math::types::IDENTITY;
    g[3] = [5.0, 0.0, 0.0, 1.0];
    m.set_matrix(path + mo::GMAT, &g);
    let mut f = si_math::types::IDENTITY;
    f[3] = [1.0, 2.0, 3.0, 1.0];
    m.set_matrix(father + mo::GMAT, &f);
    m.set_vector(pt, si_math::types::Vector::new(0.0, 0.0, 0.0));
    transfo::model_compute_path_translation(&mut m, mdl, path, pt);
    assert_eq!(m.vector(pt).to_array(), [4.0, -2.0, -3.0]);
}

#[test]
fn geometry_steps() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let post = m.si_calloc(1, 4);
    let mut t = Trace::new();
    // a node wave leaves 2, the model's geometry step finishes it
    let wave = m.si_calloc(1, 16);
    let waves = glob::chap(&m, cid::WAVES);
    let n = node(&mut m, models, 2, 0, mdl, 600, &[(waves, 0, wave, 1, 0)]);
    geom::upd_node_geometry_update(&mut m, &mut t, n, 0.0, post);
    assert_eq!(m.i32(post), 2);
    let n = node(&mut m, models, 2, 0, mdl, -200, &[]);
    geom::upd_node_geometry_update(&mut m, &mut t, n, 0.0, post);
    assert_eq!(m.i32(post), 0);
    assert_eq!(t.names(), ["post_anim_touch"]);
    assert_eq!(t.calls[0].args, [Arg::I(mdl), Arg::I(0)]);
    // lattice: position 1.25 between keys 2 and 3 of three keys (clamped to the last)
    t.calls.clear();
    let lat = m.si_calloc(1, 0x44);
    let lats = glob::chap(&m, cid::LATTICES);
    let d = glob::cur_dict(&m);
    let (_, li) = edb::add_elem(&mut m, d, lats, lat);
    let interp = lin(&mut m, 66, &[(0.0, 1.25)]);
    edb::attach_curve(&mut m, d, lats, li, 2, 211, 101, interp);
    let list = m.si_calloc(4, 4);
    for k in 0..4 {
        m.set_ptr(list + 4 * k, 0x9000 + k);
    }
    m.set_i16(mdl + 0x4c, 4);
    m.set_ptr(mdl + 0x50, list);
    let ni = curve_node(&mut m, lats, 2, li, lat, 211, &[interp]);
    geom::upd_node_geometry_update(&mut m, &mut t, ni, 0.0, post);
    assert_eq!(m.f32(lat + 0x40), 1.25);
    let n = node(&mut m, models, 2, 0, mdl, 210, &[(lats, li, lat, 1, 0)]);
    let mut t = Trace::with(move |m, name, _| {
        if name == "ed_get_elem_from_lattice" {
            let d = glob::cur_dict(m);
            return Ret::ptr(ed::get_elem(m, d, glob::chap(m, cid::LATTICES), 0));
        }
        Ret::default()
    });
    geom::upd_node_geometry_update(&mut m, &mut t, n, 0.0, post);
    assert_eq!(m.i32(post), 2);
    let li_call = t.calls.iter().find(|c| c.name == "lattice_interpolate").unwrap();
    assert_eq!(li_call.args, [Arg::I(0x9000), Arg::f(0.25), Arg::I(0x9002), Arg::I(0x9003)]);
    assert!(m.fault().is_none());
}

#[test]
fn finding_nodes_and_the_geometry_refresh() {
    let (mut m, mdl, models, _) = scene_with(cid::MODELS, 0x430);
    let d = glob::cur_dict(&m);
    let list = d + dict::TOPO_LIST;
    let nodes = m.si_calloc(3, updnode::SIZE);
    build::fill(&mut m, nodes, models, 1, 0, mdl, 100, &[]);
    build::fill(&mut m, nodes + updnode::SIZE, models, 1, 0, mdl, -100, &[]);
    build::fill(&mut m, nodes + 2 * updnode::SIZE, models, 1, 0, mdl, 100, &[]);
    m.set_i32(list + updlist::NB, 3);
    m.set_ptr(list + updlist::NODES, nodes);
    let cur = m.si_calloc(1, 4);
    assert_eq!(ed_find_update_node(&mut m, list, mdl, 100, cur), nodes);
    assert_eq!(ed_find_update_node(&mut m, list, mdl, 100, cur), nodes + 2 * updnode::SIZE);
    assert_eq!(ed_find_update_node(&mut m, list, mdl, 100, cur), 0);
    assert_eq!(ed_find_update_node(&mut m, list, mdl, -100, 0), nodes + updnode::SIZE);
    // geometry refresh: constraints present -> update at the current frame, then refresh
    let gl = m.si_calloc(1, 0x24);
    m.set_ptr(glob::SI_GRAF_LISTS, gl);
    m.set_i32(glob::SI_FRAME_CURRENT, 60);
    let mut t = Trace::with(|_, name, _| if name == "get_constraint_update_list" { Ret::int(1) } else { Ret::default() });
    assert!(update_geometry_relation(&mut m, &mut t));
    assert_eq!(t.names(), ["get_constraint_update_list", "scene_constr_update", "refresh_system"]);
    assert_eq!(t.calls[1].args, [Arg::I(5), Arg::f(2.0), Arg::I(glob::SI_ORIGIN)]);
    let mut t = Trace::new();
    assert!(!update_geometry_relation(&mut m, &mut t));
}
