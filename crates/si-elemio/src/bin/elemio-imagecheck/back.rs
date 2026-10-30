// SPDX-License-Identifier: MIT
//! // adapter: not in place
//!
//! The records the image form's readers build, read back into the value form's types so
//! that the two forms can be compared field for field. Nothing here reproduces a function
//! of the original; the offsets are those of `si_elemio::image::off` and of finding 0005.

use si_elemio::anim::{ActionElem, HierAnim, HierElem, Item, ItemData, PathAnimActs};
use si_elemio::camera::Camera;
use si_elemio::fcurve::{Fcurve, Key, StaticVal};
use si_elemio::image::globals::SI_CNSTR_LST;
use si_elemio::image::off;
use si_elemio::io::{Constraint, Udf, UserNode};
use si_elemio::lattice::Lattice;
use si_elemio::light::{Light, Sun};
use si_mem::Mem;

fn f3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

fn text(m: &Mem, a: u32) -> Vec<u8> {
    if a == 0 {
        Vec::new()
    } else {
        m.cstr(a)
    }
}

fn opt_text(m: &Mem, a: u32) -> Option<Vec<u8>> {
    (a != 0).then(|| m.cstr(a))
}

/// The entry of `si_cnstrLst` for an owner and a kind (the list is newest first: the last
/// one found is the first one read, which is what the value form keeps... the value form
/// keeps the latest; so the first found).
fn constraint(m: &Mem, owner: u32, kind: u32) -> Option<Constraint> {
    let mut p = m[SI_CNSTR_LST];
    while p != 0 {
        if m.u32(p + off::cnstr::OWNER) == owner && m.u32(p + off::cnstr::KIND) == kind {
            return Some(Constraint { graf_type: kind as i32, index: m.i32(p + off::cnstr::NUMBER), name: text(m, m.u32(p + off::cnstr::NAME)) });
        }
        p = m.u32(p + off::cnstr::NEXT);
    }
    None
}

fn user_data(m: &Mem, first: u32) -> Vec<UserNode> {
    let mut nodes = Vec::new();
    let mut p = first;
    while p != 0 {
        let size = m.u32(p + off::user_node::DATA_SIZE);
        nodes.push(UserNode { name: text(m, m.u32(p + off::user_node::NAME)), kind: m.i16(p + off::user_node::WORD), data: m.bytes(m.u32(p + off::user_node::DATA), size).to_vec() });
        p = m.u32(p + off::user_node::NEXT);
    }
    nodes
}

fn udf(m: &Mem, p: u32) -> Option<Udf> {
    (p != 0).then(|| Udf { name: text(m, m.u32(p + off::udf::NAME)), w0c: m.i16(p + off::udf::WORD_0C), w10: m.i16(p + off::udf::WORD_10), d14: m.i32(p + off::udf::WORD_14) })
}

pub fn camera_at(m: &Mem, c: u32) -> Camera {
    use off::camera as o;
    Camera {
        name: text(m, m.u32(c + o::NAME)),
        w_0a: m.i16(c + 0x0a),
        mode: m.i16(c + o::MODE),
        position: f3(m, c + o::POSITION),
        interest: f3(m, c + o::INTEREST),
        yaw_pitch_roll: f3(m, c + o::YAW),
        near: m.f32(c + o::NEAR),
        far: m.f32(c + o::FAR),
        w_3c: m.i16(c + 0x3c),
        field_of_view: m.f32(c + o::FIELD_OF_VIEW),
        w_44: m.i16(c + 0x44),
        v_48: f3(m, c + 0x48),
        format: m.i32(c + 0x54),
        format_aspect: m.f32(c + 0x58),
        f_5c: m.f32(c + 0x5c),
        f_60: m.f32(c + 0x60),
        f_64: m.f32(c + o::ASPECT),
        f_68: m.f32(c + 0x68),
        depth_of_field: m.i32(c + o::DEPTH_OF_FIELD),
        focal_length: m.f32(c + o::FOCAL_LENGTH),
        focal_distance: m.f32(c + o::FOCAL_DISTANCE),
        near_focus: m.f32(c + o::NEAR_FOCUS),
        far_focus: m.f32(c + o::FAR_FOCUS),
        out_of_focus: m.f32(c + o::OUT_OF_FOCUS),
        f_stop: m.f32(c + o::F_STOP),
        maximum_coc: m.f32(c + o::MAXIMUM_COC),
        enlarge: m.i16(c + o::ENLARGE),
        position_constraint: constraint(m, c, 4),
        interest_constraint: constraint(m, c, 5),
        w_a8: m.i16(c + o::WORD_A8),
        d_ac: m.i32(c + o::WORD_AC),
        d_b0: m.i32(c + o::WORD_B0),
        w_b4: m.i16(c + 0xb4),
        proj_plane_active: m.i16(c + o::PLANE_ACTIVE),
        proj_plane_distance: m.f32(c + o::PLANE_DISTANCE),
        proj_plane_size: [m.f32(c + o::PLANE_WIDTH), m.f32(c + o::PLANE_HEIGHT)],
        proj_plane_offset: [m.f32(c + o::PLANE_OFFSET_X), m.f32(c + o::PLANE_OFFSET_Y)],
        user_data: user_data(m, m.u32(c + o::USER_DATA)),
    }
}

pub fn light_at(m: &Mem, l: u32) -> Light {
    use off::light as o;
    let i = |a: u32| m.i32(l + a);
    let w = |a: u32| m.i16(l + a);
    let f = |a: u32| m.f32(l + a);
    let mut name = [0u8; 30];
    name.copy_from_slice(m.bytes(l + o::NAME, 30));
    let sun_p = m.u32(l + o::SUN);
    let sun = (sun_p != 0).then(|| Sun {
        name: text(m, m.u32(sun_p + off::sun::NAME)),
        latitude: m.i32(sun_p + off::sun::LATITUDE),
        longitude: m.i32(sun_p + off::sun::LONGITUDE),
        daylight_saving: m.i16(sun_p + off::sun::DAYST),
        date_time: m.i32(sun_p + off::sun::DATE_TIME),
    });
    let mut area_geometry = [0f32; 12];
    for (k, v) in area_geometry.iter_mut().enumerate() {
        *v = f(o::AREA_GEOMETRY + 4 * k as u32);
    }
    let mut area_rotation_mat = [0f32; 16];
    for (k, v) in area_rotation_mat.iter_mut().enumerate() {
        *v = f(o::AREA_ROTATION_MAT + 4 * k as u32);
    }
    Light {
        name,
        kind: w(o::TYPE),
        colour: f3(m, l + o::COLOUR),
        d_30: i(0x30),
        position: f3(m, l + o::POSITION),
        d_40: [i(0x40), i(0x44), i(0x48), i(0x4c), i(0x50), i(0x54)],
        interest: f3(m, l + o::INTEREST),
        d_64: [i(0x64), i(0x68), i(0x6c)],
        cone_angle: f(o::CONE_ANGLE),
        spread_angle: f(o::SPREAD_ANGLE),
        d_78: i(0x78),
        d_80: i(0x80),
        fall_off_start: f(o::FALL_OFF_START),
        fall_off_end: f(o::FALL_OFF_END),
        shadow_type: w(o::SHADOW_TYPE),
        shadow_factor: f(o::SHADOW_FACTOR),
        shadow_map_res: i(o::SHADOW_MAP_RES),
        shadow_filter_size: i(o::SHADOW_FILTER_SIZE),
        shadow_filter_step: i(o::SHADOW_FILTER_STEP),
        shadow_fade_start: f(o::SHADOW_FADE_START),
        shadow_fade_rate: f(o::SHADOW_FADE_RATE),
        d_130: i(0x130),
        d_134: i(0x134),
        w_138: w(0x138),
        w_13c: w(0x13c),
        d_140: i(o::NEXT),
        position_constraint: constraint(m, l, 6),
        interest_constraint: constraint(m, l, 7),
        w_14c: w(0x14c),
        d_150: i(0x150),
        d_154: i(0x154),
        w_158: w(0x158),
        selective: [i(o::SELECTIVE), i(o::INCLUDE)],
        d_164: i(0x164),
        d_168: i(0x168),
        sun,
        d_170: i(0x170),
        w_174: w(0x174),
        w_176: w(0x176),
        shader: udf(m, m.u32(l + o::SHADER)),
        shader2: udf(m, m.u32(l + o::SHADER2)),
        diffuse: w(o::DIFFUSE),
        area_light: w(o::AREA_LIGHT),
        area_type: i(o::AREA_TYPE),
        area_u_sample: i(o::AREA_U_SAMPLE),
        area_v_sample: i(o::AREA_V_SAMPLE),
        area_geometry,
        area_local_coords: w(o::AREA_LOCAL_COORDS),
        area_scale: f3(m, l + o::AREA_SCALE),
        area_rotation: f3(m, l + o::AREA_ROTATION),
        area_selected: w(o::AREA_SELECTED),
        area_rotation_mat,
        user_data: user_data(m, m.u32(l + o::USER_DATA)),
        energy: f3(m, l + o::ENERGY),
        exponent: f(o::EXPONENT),
        caustic_photon: i(o::CAUSTIC_PHOTON),
        globillum_photon: i(o::GLOBILLUM_PHOTON),
        caustic_enable: w(o::CAUSTIC_ENABLE),
        globillum_enable: w(o::GLOBILLUM_ENABLE),
    }
}

pub fn lattice_at(m: &Mem, l: u32) -> Lattice {
    use off::lattice as o;
    let nb_keys = [m.i16(l + o::COUNT_X), m.i16(l + o::COUNT_Y), m.i16(l + o::COUNT_Z)];
    let n = (nb_keys[0] as i32 * nb_keys[1] as i32 * nb_keys[2] as i32).max(0) as u32;
    let (keys, flags) = (m.u32(l + o::KEYS), m.u32(l + o::FLAGS));
    Lattice {
        active: m.i16(l + o::ACTIVE),
        nb_keys,
        interpolation: [m.i32(l + o::INTERPOLATION), m.i32(l + o::INTERPOLATION + 4), m.i32(l + o::INTERPOLATION + 8)],
        keys: (0..n).map(|k| f3(m, keys + 12 * k)).collect(),
        key_flags: (0..n).map(|k| m.i16(flags + 2 * k)).collect(),
        minimum: f3(m, l + o::MINIMUM),
        maximum: f3(m, l + o::MAXIMUM),
        scaling: f3(m, l + o::SCALING),
    }
}

fn key_at(m: &Mem, k: u32) -> Key {
    use off::fkey as o;
    Key {
        time: m.f32(k + o::TIME),
        value: m.f32(k + o::VALUE),
        right_slope: m.f32(k + o::RIGHT_SLOPE),
        flag: m.i16(k + o::FLAG),
        interpolation: m.i16(k + o::INTERPOLATION),
        w_14: m.i16(k + o::WORD_14),
        left_slope: m.f32(k + o::LEFT_SLOPE),
        key_type: m.u16(k + o::KEY_TYPE),
        spline_index: m.i16(k + o::SPLINE_INDEX),
        w_20: m.i16(k + o::WORD_20),
    }
}

pub fn fcurve_at(m: &Mem, c: u32) -> Fcurve {
    use off::fcurve as o;
    let count = m.i32(c + o::NB_KEYS);
    let keys = m.u32(c + o::KEYS);
    Fcurve {
        curve_type: m.i32(c + o::TYPE),
        count,
        keys: (0..count.max(0) as u32).map(|k| key_at(m, keys + k * off::fkey::SIZE)).collect(),
        pre_extrapolation: m.i16(c + o::PRE_EXTRAPOLATION),
        post_extrapolation: m.i16(c + o::POST_EXTRAPOLATION),
        flags: m.i16(c + o::FLAGS),
        interpolation: m.i16(c + o::INTERPOLATION),
    }
}

fn actions_at(m: &Mem, list: u32) -> Vec<ActionElem> {
    use off::actions_elem as o;
    let n = m.u32(list + off::actions::NB_ELEMS);
    let elems = m.u32(list + off::actions::ELEMS);
    (0..n)
        .map(|k| {
            let e = elems + k * o::SIZE;
            let p = m.u32(e + o::PATH);
            let path = (p != 0).then(|| {
                let c = m.u32(p + off::path_acts::CURVE);
                PathAnimActs {
                    d_00: m.i32(p + off::path_acts::WORD_00),
                    d_04: m.i32(p + off::path_acts::WORD_04),
                    fcurve: (c != 0).then(|| fcurve_at(m, c)),
                    s_0c: opt_text(m, m.u32(p + off::path_acts::TEXT_0C)),
                    s_10: opt_text(m, m.u32(p + off::path_acts::TEXT_10)),
                    s_14: opt_text(m, m.u32(p + off::path_acts::TEXT_14)),
                    s_18: opt_text(m, m.u32(p + off::path_acts::TEXT_18)),
                }
            });
            ActionElem {
                name: text(m, m.u32(e + o::NAME)),
                source: opt_text(m, m.u32(e + o::NAME2)),
                start: m.f32(e + o::START),
                end: m.f32(e + o::END),
                clip_start: m.f32(e + o::CLIP_START),
                clip_end: m.f32(e + o::CLIP_END),
                flags: m.i32(e + o::FLAGS),
                d_1c: m.i32(e + o::WORD_1C),
                d_20: m.i32(e + o::WORD_20),
                cycles: m.i32(e + o::CYCLES),
                path,
                interp: m.i16(e + o::INTERPOLATION),
            }
        })
        .collect()
}

/// An `SI_HierAnim` with everything it points at. The chapter is the name the reader
/// asked the dictionary for (the record holds the chapter's index).
pub fn anim_at(m: &Mem, a: u32, chapter: Option<Vec<u8>>) -> HierAnim {
    use off::hier_elem as o;
    let n = m.u32(a + off::hier_anim::NB_ELEMS);
    let elems = m.u32(a + off::hier_anim::ELEMS);
    let elems = (0..n)
        .map(|k| {
            let e = elems + k * o::SIZE;
            let count = m.u32(e + o::NB_ITEMS);
            let (types, rels, occs, data, ones, flags) = (m.u32(e + o::TYPES), m.u32(e + o::REL_IDS), m.u32(e + o::OCC_IDS), m.u32(e + o::DATA), m.u32(e + o::ONES), m.u32(e + o::FLAGS));
            let items = (0..count)
                .map(|j| {
                    let kind = m.i16(types + 2 * j);
                    let d = m.u32(data + 4 * j);
                    Item {
                        kind,
                        act_rel_id: m.i32(rels + 4 * j),
                        act_occ_id: m.i16(occs + 2 * j),
                        w_1c: m.i16(ones + 2 * j),
                        w_20: m.i16(flags + 2 * j),
                        data: match (kind, d) {
                            (_, 0) => ItemData::None,
                            (1, d) => ItemData::Static(StaticVal { curve_type: m.i32(d + off::static_val::TYPE), value: m.f32(d + off::static_val::VALUE) }),
                            (_, d) => ItemData::Curve(fcurve_at(m, d)),
                        },
                    }
                })
                .collect();
            HierElem { name: text(m, m.u32(e + o::NAME)), father: m.i32(e + o::FATHER), items }
        })
        .collect();
    HierAnim { chapter, elems, actions: actions_at(m, a + off::hier_anim::ACTIONS), sources: actions_at(m, a + off::hier_anim::ACTIONS2) }
}
