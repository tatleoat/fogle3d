// SPDX-License-Identifier: MIT
//! Parameter updates of `motionUpdate.c`: one function curve, one field.
//!
//! Every routine takes an update node and a time. The node's arguments are the curves of
//! one parameter; a routine either looks at the first argument only (parameters with one
//! value) or goes through all of them and tells them apart by the curve's own type id
//! (parameters with several components). The value is `SI_fcurveValue(curve, time, 1)`:
//! evaluated with the curve's activation and range applied (finding 0009), kept in the
//! register, and rounded to `float` by the store into the field.
//!
//! Angles are stored in the curves in degrees and in the structures in radians: the
//! conversion is a multiplication by the double 0.0174532925 (the nine-digit `DEG2RAD` of
//! the SDK's `SI_macros.h`, not π/180), done in the register before the store.

use super::{arg, arg_curve, data, nb_args, node_elem, relid, UpdateHost};
use crate::mem::MemExt;
use crate::ed;
use crate::fcv;
use crate::glob::{self, anim, cid};
use si_mem::{Addr, Mem};
use crate::off::{model, updelem};
use si_math::algebra;
use si_x87::{trig, Round, F80};

/// The constant at 0x0864dca0 and its eleven copies in this file's read-only data.
pub const DEG2RAD: f64 = 0.0174532925;

/// Fields of the objects these routines write. **new**: every offset here that finding
/// 0010 does not list was read from the routine named.
pub mod o {
    /// `SI_DynaMatterInfo` (model + 0x2f4): static roughness, kinetic roughness, elasticity
    /// (`animModelParametersUpdate`; the attribute ids 180, 181, 179 of `Relations.rsrc`).
    pub const DYNA_STAT_ROUGH: u32 = 0x0c;
    pub const DYNA_KINE_ROUGH: u32 = 0x10;
    pub const DYNA_ELASTICITY: u32 = 0x14;
    /// `SI_DynaChnVars` (model + 0x304): 16-bit root-constraint switch (attribute 182).
    pub const CHN_ROOT_CONS: u32 = 0x38;
    /// `__SI_Elem` + 0x20 is the wire type (finding 0010). With `SI_useHiddenForMatte` the
    /// visibility curve also sets two bits above it: 0x80 for a value in [1.5, 2.5), 0x100
    /// from 2.5 on (`modelSetAlphaBits`).
    pub const ELEM_ALPHA_BITS: u32 = 0x20;
    /// `__SI_GrafNode` + 0x0c: display type of the node on screen (0 hidden, 2 visible).
    pub const GN_DISPLAY: u32 = 0x0c;
    /// `SI_InvKin` (model + 0x318): the goal (three floats) and a state word: 1 "goal
    /// set, solve", 2 "solved" (`animInvKinUpdate`).
    pub const IK_GOAL: u32 = 0x08;
    pub const IK_STATE: u32 = 0x38;

    /// `SI_Camera`, 0x13c bytes. Names of the fields: the keywords of the camera file
    /// (finding 0005) at the same offsets.
    pub mod camera {
        pub const POSITION: u32 = 0x10;
        pub const INTEREST: u32 = 0x1c;
        pub const ROLL: u32 = 0x30;
        pub const NEAR: u32 = 0x34;
        pub const FAR: u32 = 0x38;
        pub const FOV: u32 = 0x40;
        pub const FOCAL_LENGTH: u32 = 0x70;
        pub const FOCAL_DISTANCE: u32 = 0x74;
        pub const NEAR_FOCUS: u32 = 0x7c;
        pub const FAR_FOCUS: u32 = 0x84;
        pub const OUT_OF_FOCUS: u32 = 0x8c;
        pub const F_STOP: u32 = 0x94;
        pub const MAXIMUM_COC: u32 = 0x98;
        /// Projection plane: 16-bit active flag, distance, width, height, offset x, y.
        pub const PP_ACTIVE: u32 = 0x108;
        pub const PP_DISTANCE: u32 = 0x10c;
        pub const PP_WIDTH: u32 = 0x110;
        pub const PP_HEIGHT: u32 = 0x114;
        pub const PP_OFFSET_X: u32 = 0x118;
        pub const PP_OFFSET_Y: u32 = 0x11c;
    }

    /// `light`, 0x240 bytes (keywords of the light file, finding 0005).
    pub mod light {
        pub const KIND: u32 = 0x1e;
        pub const COLOUR: u32 = 0x24;
        pub const POSITION: u32 = 0x34;
        pub const INTEREST: u32 = 0x58;
        pub const CONE_ANGLE: u32 = 0x70;
        pub const SPREAD_ANGLE: u32 = 0x74;
        pub const FALL_OFF_START: u32 = 0x84;
        pub const FALL_OFF_END: u32 = 0x88;
        pub const SHADOW_FACTOR: u32 = 0x90;
        /// Pointer to the sun record; its word at +0x10 is the date and time (`time_t`).
        pub const SUN: u32 = 0x16c;
        pub const SUN_DATE_TIME: u32 = 0x10;
        /// **new**: 16-bit "visible" flag written by the visibility curve.
        pub const VISIBLE: u32 = 0x176;
        pub const AREA_LIGHT: u32 = 0x182;
        pub const AREA_TYPE: u32 = 0x184;
        /// Area geometry: +0x190 is the sphere's radius (type 1), +0x19c the disk's (type 2).
        pub const AREA_GEOMETRY: u32 = 0x190;
        pub const AREA_DISK_RADIUS: u32 = 0x19c;
        pub const AREA_SCALE: u32 = 0x1c4;
        pub const AREA_ROTATION: u32 = 0x1d0;
        pub const AREA_ROTATION_MAT: u32 = 0x1e0;
        pub const ENERGY: u32 = 0x224;
        pub const EXPONENT: u32 = 0x230;
        pub const CAUSTIC_PHOTON: u32 = 0x234;
        pub const GLOBILLUM_PHOTON: u32 = 0x238;
        pub const CAUSTIC_ENABLE: u32 = 0x23c;
        pub const GLOBILLUM_ENABLE: u32 = 0x23e;
    }

    /// `__SI_Material`, 0xdcc bytes. **new**: +0x6ac, +0x6bc, +0x75c, +0x760.
    pub mod material {
        pub const AMBIENT: u32 = 0x34;
        pub const DIFFUSE: u32 = 0x44;
        pub const SPECULAR: u32 = 0x54;
        pub const SPEC_DECAY: u32 = 0x64;
        pub const REFLECT: u32 = 0x6a8;
        pub const REFLECT_JITTER: u32 = 0x6ac;
        pub const TRANSP: u32 = 0x6b8;
        pub const TRANSP_JITTER: u32 = 0x6bc;
        pub const REFRACT_INDEX: u32 = 0x6dc;
        pub const BLUR_WIDTH: u32 = 0x75c;
        pub const BLUR_DECAY: u32 = 0x760;
    }

    /// `__SI_LclMap` (a 2D texture), 0x1d8 bytes. **new**: all of them.
    pub mod tex2d {
        /// 16 bits: 1 a picture sequence, 2 a script (`updNodeParameterUpdate`).
        pub const SOURCE_KIND: u32 = 0x28;
        pub const U_SCALE: u32 = 0x58;
        pub const V_SCALE: u32 = 0x5c;
        pub const U_OFFSET: u32 = 0x68;
        pub const V_OFFSET: u32 = 0x6c;
        /// The texture's own (projection support) rotation, radians; scaling; translation.
        pub const S_ROTATION: u32 = 0x94;
        pub const S_TRANSLATION: u32 = 0xa0;
        pub const S_SCALING: u32 = 0xac;
        pub const BLEND: u32 = 0x10c;
        pub const AMBIENT: u32 = 0x128;
        pub const DIFFUSE: u32 = 0x12c;
        pub const SPECULAR: u32 = 0x130;
        pub const REFLECT: u32 = 0x134;
        pub const TRANSP: u32 = 0x138;
        pub const ROUGHNESS: u32 = 0x140;
        pub const REFLECT_MAP: u32 = 0x144;
        /// Rotation of the picture in its plane, radians.
        pub const ROTATION: u32 = 0x168;
        /// A flag byte: bit 1 "parameters changed", bits 0 and 2 "picture out of date".
        pub const DIRTY: u32 = 0x194;
    }

    /// `__SI_Texture3D`, 0x158 bytes (finding 0010; **new**: +0x14 taper, +0x120).
    pub mod tex3d {
        pub const SPACING: u32 = 0x10;
        /// Radians.
        pub const TAPER: u32 = 0x14;
        pub const STRENGTH: u32 = 0x18;
        pub const POWER: u32 = 0x20;
        pub const BLEND: u32 = 0x28;
        /// Five colours of four floats: red, green, blue, alpha.
        pub const COLORS: u32 = 0x30;
        pub const FACTORS: u32 = 0x80;
        pub const SCALE: u32 = 0x90;
        /// Radians.
        pub const ROTATION: u32 = 0x9c;
        pub const TRANSL: u32 = 0xa8;
        pub const AMBIENT: u32 = 0x108;
        pub const DIFFUSE: u32 = 0x10c;
        pub const SPECULAR: u32 = 0x110;
        pub const REFLECTION: u32 = 0x114;
        pub const TRANSP: u32 = 0x118;
        pub const ROUGHNESS: u32 = 0x120;
    }

    /// The render control record (`SI_defaultControl`, `SI_ren`). **new**: all of them.
    pub mod control {
        pub const FADE_START: u32 = 0x104;
        pub const FADE_STOP: u32 = 0x10c;
        pub const FADE_RED: u32 = 0x118;
        pub const FADE_GREEN: u32 = 0x11c;
        pub const FADE_BLUE: u32 = 0x120;
        pub const FADE_TRANSP: u32 = 0x130;
        /// Curve types 203 `fodens`, 204 `fobase`, 205 `fothick` (names of `Relations.rsrc`).
        pub const FOG_DENSITY: u32 = 0x260;
        pub const FOG_BASE: u32 = 0x264;
        pub const FOG_THICKNESS: u32 = 0x268;
        pub const FOG_START: u32 = 0x26c;
        pub const FOG_STOP: u32 = 0x270;
        pub const FOG_RED: u32 = 0x284;
        pub const FOG_GREEN: u32 = 0x288;
        pub const FOG_BLUE: u32 = 0x28c;
    }
}

/// `SI_ren` (0x08790be0): the render control record the renderer works on; the fade and fog
/// updates repeat themselves on it when the update was asked for by a renderer.
pub const SI_REN: Addr = 0x0879_0be0;
/// The 16-bit `keep_turns` switch of `SI_matrixExtractXyzRotation` (finding 0008), inside
/// `SI_glbManip`.
pub const KEEP_TURNS: Addr = 0x0878_1580;

fn val(m: &mut Mem, curve: Addr, time: f32) -> F80 {
    fcv::value(m, curve, time, 1)
}

fn first_curve(m: &Mem, node: Addr) -> Addr {
    m.ptr(arg(m, node, 0) + updelem::DATA)
}

/// Parameters of a model [soft 0x082259c8 animModelParametersUpdate]. First argument only.
///
/// | Relation | Curve | Effect |
/// |---|---|---|
/// | 10 `NVisibility`, 12 `BVisibility` | 175, 273 | `SI_modelSetHiddenParam(graf node, model, element, state, branch)`: state 2 (visible) when the value equals 1, else 0; `branch` is 1 for relation 12. With `SI_useHiddenForMatte`: state 0 below 0.5 and 2 otherwise, `branch` 1, and the element's alpha bits follow the value ([`model_set_alpha_bits`]) |
/// | 15 `NMatte` | 371 | model + 0x3f4 |
/// | 16 `BMatte` | 372 | `SI_modelPropagateMatteParam(model, value)` |
/// | 50 `Elasticity`, 51 `StatRough`, 52 `KineRough` | 179, 180, 181 | +0x14, +0x0c, +0x10 of the record at model + 0x2f4 (not tested for null) |
/// | 53 `RootCons` | 182 | 16 bits at +0x38 of the record at model + 0x304: the value truncated |
/// | 4000 `CsTrans`, 4001 `CsReceiv` | 620, 631 | bits 0 and 1 of model + 0x404: set when the value equals 1 |
/// | 4002 `GITrans`, 4003 `GIReceiv` | 632, 633 | bits 0 and 1 of model + 0x406 |
pub fn anim_model_parameters_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let e = node_elem(m, node);
    let curve = first_curve(m, node);
    if curve == 0 {
        return;
    }
    let bit = |m: &mut Mem, at: u32, mask: u8, v: F80| {
        if v.eq(F80::ONE) {
            m.or_u8(mdl + at, mask);
        } else {
            m.and_u8(mdl + at, !mask);
        }
    };
    match relid(m, node) {
        0x33 => {
            let v = val(m, curve, time).to_f32();
            m.set_f32(m.ptr(mdl + model::DYNA_MATTER) + o::DYNA_STAT_ROUGH, v);
        }
        0xf => {
            let v = val(m, curve, time).to_f32();
            m.set_f32(mdl + model::MATTE, v);
        }
        id @ (10 | 0xc) => {
            let v = val(m, curve, time).to_f32();
            let gl = glob::cur_graf_list(m);
            let gn = h.find_gl_data(m, gl, mdl, 2);
            let hidden_for_matte = m.i16(glob::SI_USE_HIDDEN_FOR_MATTE) != 0;
            let state = if hidden_for_matte {
                if F80::from_f32(v).lt(F80::from_f64(0.5)) {
                    0
                } else {
                    2
                }
            } else if F80::from_f32(v).eq(F80::ONE) {
                2
            } else {
                0
            };
            let branch = (id == 0xc || m.i16(glob::SI_USE_HIDDEN_FOR_MATTE) != 0) as i32;
            h.model_set_hidden_param(m, gn, mdl, e, state, branch);
            if m.i16(glob::SI_USE_HIDDEN_FOR_MATTE) != 0 {
                model_set_alpha_bits(m, h, e, v);
            }
        }
        0x10 => {
            let v = val(m, curve, time).to_f32();
            h.model_propagate_matte_param(m, mdl, v);
        }
        0x32 => {
            let v = val(m, curve, time).to_f32();
            m.set_f32(m.ptr(mdl + model::DYNA_MATTER) + o::DYNA_ELASTICITY, v);
        }
        4000 => {
            let v = val(m, curve, time);
            bit(m, model::CAUSTIC, 1, v);
        }
        0x34 => {
            let v = val(m, curve, time).to_f32();
            m.set_f32(m.ptr(mdl + model::DYNA_MATTER) + o::DYNA_KINE_ROUGH, v);
        }
        0x35 => {
            let v = val(m, curve, time).to_i32(Round::Zero);
            m.set_i16(m.ptr(mdl + model::DYNA_CHN_VARS) + o::CHN_ROOT_CONS, v as i16);
        }
        0xfa2 => {
            let v = val(m, curve, time);
            bit(m, model::GLOBAL_ILLUM, 1, v);
        }
        0xfa1 => {
            let v = val(m, curve, time);
            bit(m, model::CAUSTIC, 2, v);
        }
        0xfa3 => {
            let v = val(m, curve, time);
            bit(m, model::GLOBAL_ILLUM, 2, v);
        }
        _ => {}
    }
}

/// The visibility value as an opacity class on an element and all its descendants
/// [soft 0x08225940 modelSetAlphaBits]: bits 0x80 and 0x100 of the word at element + 0x20
/// are cleared for a value below 1.5, 0x80 alone is set below 2.5, 0x100 alone from 2.5 on.
/// The sons are found through the element dictionary (`SI_edFindSon`, `SI_edFindNextSon`).
pub fn model_set_alpha_bits<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, value: f32) {
    let v = F80::from_f32(value);
    let w = m.u32(e + o::ELEM_ALPHA_BITS);
    let w = if v.lt(F80::from_f64(1.5)) {
        w & 0xffff_fe7f
    } else if v.lt(F80::from_f64(2.5)) {
        w & 0xffff_feff | 0x80
    } else {
        w & 0xffff_ff7f | 0x100
    };
    m.set_u32(e + o::ELEM_ALPHA_BITS, w);
    let mut son = ed::find_son(m, h, e);
    while son != 0 {
        model_set_alpha_bits(m, h, son, value);
        son = ed::find_next_son(m, h, e);
    }
}

/// The algorithm-type curve (relation 11, curve 178) switches dynamics simulation on and
/// off [soft 0x0822565c animAlgoTypeUpdate].
///
/// The curve is looked up on the element of the **chain root** when the node's model is a
/// joint (kind 5) or a null with algorithm 3 or 4, else on the model's own element. It is
/// read as an integer flag (`SI_fcurveIntegerValue(curve, time, 1, 1)`); "on" is the
/// value 1.
///
/// 1. A model that is its own root and has algorithm 2 or 12: `SI_dynaAlgoTypeSet(model, on)`.
/// 2. With the dynamics switch (`SI_glbAnim` + 8) off and a root whose algorithm is neither
///    2 nor 12, for a chain null (kind 0, algorithm 3 or 4) or a model with a chain record
///    (+0x304) that is not a null: if the curve is on now **and** was on one frame earlier
///    (at `time − 1/rate`), `SI_dynaAlgoTypeSet(root, 1)` and nothing more.
/// 3. Otherwise: a chain null whose root has neither 2 nor 12 sets the root to the curve's
///    state; a non-null model with a chain record and an algorithm of neither 2 nor 12 sets
///    itself.
pub fn anim_algo_type_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let kind = |m: &Mem, p: Addr| m.i32(p + model::KIND);
    let algo = |m: &Mem, p: Addr| m.i32(p + model::ALGO_TYPE);
    let chain_null = |m: &Mem, p: Addr| kind(m, p) == 0 && (algo(m, p).wrapping_sub(3) as u32) < 2;
    let mut root = mdl;
    if kind(m, mdl) == 5 || chain_null(m, mdl) {
        root = h.ske_chain_root(m, mdl);
    }
    let (e, _) = ed::get_elem_from_model(m, h, root);
    let curve = ed::find_fcv(m, e, 0, 0xb);
    if curve == 0 {
        return;
    }
    let on = |m: &mut Mem, t: f32| F80::from_i32(fcv::integer_value(m, curve, t, 1, 1)).eq(F80::ONE);
    if mdl == root && (algo(m, mdl) == 2 || algo(m, mdl) == 0xc) {
        let v = on(m, time);
        h.dyna_algo_type_set(m, mdl, v as i16);
    }
    let mut done = false;
    if m.i16(glob::SI_GLB_ANIM + anim::DYNAMICS) == 0
       && algo(m, root) != 2
       && algo(m, root) != 0xc
       && (chain_null(m, mdl) || (m.ptr(mdl + model::DYNA_CHN_VARS) != 0 && kind(m, mdl) != 0))
    {
        let now = on(m, time);
        let before_t = (F80::from_f32(time) - F80::ONE / glob::frame_rate(m)).to_f32();
        let before = on(m, before_t);
        if before && now {
            h.dyna_algo_type_set(m, root, 1);
            done = true;
        }
    }
    if !done {
        if chain_null(m, mdl) && algo(m, root) != 2 && algo(m, root) != 0xc {
            let v = on(m, time);
            h.dyna_algo_type_set(m, root, v as i16);
        }
        if m.ptr(mdl + model::DYNA_CHN_VARS) != 0 && kind(m, mdl) != 0 && algo(m, mdl) != 2 && algo(m, mdl) != 0xc {
            let v = on(m, time);
            h.dyna_algo_type_set(m, mdl, v as i16);
        }
    }
}

/// Relation 120 [soft 0x082255f4 animInvKinUpdate]: a model whose inverse-kinematics record
/// (+0x318) is in state 1 ("goal set") has its chain solved for the goal stored in the
/// record, and the state becomes 2.
pub fn anim_inv_kin_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let ik = m.ptr(mdl + model::INV_KIN);
    if ik != 0 && m.i32(ik + o::IK_STATE) == 1 {
        let (x, y, z) = (m.f32(ik + o::IK_GOAL), m.f32(ik + o::IK_GOAL + 4), m.f32(ik + o::IK_GOAL + 8));
        h.model_solve_inv_kin(m, mdl, x, y, z, time);
        let ik = m.ptr(mdl + model::INV_KIN);
        m.set_i32(ik + o::IK_STATE, 2);
    }
}

/// Relation 800 [soft 0x082258ac animDynamicsUpdate]: with the dynamics switch on, and for
/// a node whose first argument is the node's own model: the simulation of the model's
/// chain root (for a chain null) or of the model itself (any other non-null) is advanced by
/// one frame, provided that model has algorithm 2 or 12 and a chain record.
pub fn anim_dynamics_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    if m.i16(glob::SI_GLB_ANIM + anim::DYNAMICS) == 0 {
        return;
    }
    let mdl = data(m, node);
    let first = first_curve(m, node);
    if mdl == 0 || first == 0 || mdl != first {
        return;
    }
    let kind = m.i32(mdl + model::KIND);
    let target = if kind == 0 && (m.i32(mdl + model::ALGO_TYPE).wrapping_sub(3) as u32) < 2 {
        h.ske_chain_root(m, mdl)
    } else if kind != 0 {
        mdl
    } else {
        0
    };
    if target != 0 {
        let a = m.i32(target + model::ALGO_TYPE);
        if (a == 2 || a == 0xc) && m.ptr(target + model::DYNA_CHN_VARS) != 0 {
            let dt = super::frame_duration(m);
            h.dynamic_simulation_compute(m, target, time, dt);
        }
    }
}

/// Relations 614 and 615, a wave's decay and spread
/// [soft 0x08228754 animWaveParametersUpdate]: the first curve is evaluated and the result
/// is dropped. (The evaluation leaves the value in the curve's own "last value" field,
/// which is where the wave code would have to pick it up; not followed.)
pub fn anim_wave_parameters_update(m: &mut Mem, node: Addr, time: f32) {
    if data(m, node) != 0 {
        let c = first_curve(m, node);
        if c != 0 {
            val(m, c, time);
        }
    }
}

/// Reads three floats, lets the node's curves replace components by curve type, writes the
/// three back: the common shape of the colour and vector parameters. `pick` maps a curve
/// type to (component, evaluate as "1.0 when the curve has no key").
fn triple(m: &mut Mem, node: Addr, time: f32, at: Option<Addr>, pick: impl Fn(i32) -> Option<(usize, bool)>) -> [f32; 3] {
    // without a known relation id the original's three locals are uninitialised stack
    let mut v = match at {
        Some(a) => [m.f32(a), m.f32(a + 4), m.f32(a + 8)],
        None => [0.0; 3],
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            if let Some((k, unit_if_empty)) = pick(fcv::kind(m, c)) {
                v[k] = if unit_if_empty && fcv::nb_keys(m, c) < 1 { 1.0 } else { val(m, c, time).to_f32() };
            }
        }
        i += 1;
    }
    v
}

/// The three colours of a material [soft 0x08228788 animMatColorUpdate]: relation 301
/// ambient (+0x34), 302 diffuse (+0x44), 303 specular (+0x54); curves 45/48/51 red,
/// 46/49/52 green, 47/50/53 blue. Any of the nine curve types is accepted under any of the
/// three relations. Every curve of the node is evaluated here (unlike the other triples,
/// which evaluate only the curves whose type they know).
pub fn anim_mat_color_update(m: &mut Mem, node: Addr, time: f32) {
    let mat = data(m, node);
    if mat == 0 {
        return;
    }
    let at = match relid(m, node) {
        0x12d => Some(mat + o::material::AMBIENT),
        0x12e => Some(mat + o::material::DIFFUSE),
        0x12f => Some(mat + o::material::SPECULAR),
        _ => None,
    };
    let mut v = match at {
        Some(a) => [m.f32(a), m.f32(a + 4), m.f32(a + 8)],
        None => [0.0; 3],
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let x = val(m, c, time);
            match fcv::kind(m, c) {
                0x2d | 0x30 | 0x33 => v[0] = x.to_f32(),
                0x2e | 0x31 | 0x34 => v[1] = x.to_f32(),
                0x2f | 0x32 | 0x35 => v[2] = x.to_f32(),
                _ => {}
            }
        }
        i += 1;
    }
    if let Some(a) = at {
        m.set_f32(a, v[0]);
        m.set_f32(a + 4, v[1]);
        m.set_f32(a + 8, v[2]);
    }
}

/// A value with a lower bound: the curve is evaluated; if the result is below `floor` the
/// answer is `low`; otherwise the curve is evaluated a **second time** and that result is
/// the answer (the original's source names the call twice; the two results are equal). A
/// NaN is not below anything and is kept.
fn zero_if_negative(m: &mut Mem, curve: Addr, time: f32, floor: F80, low: F80) -> F80 {
    if val(m, curve, time).lt(floor) {
        low
    } else {
        val(m, curve, time)
    }
}

/// [`zero_if_negative`] with the bound 0.
fn non_negative(m: &mut Mem, curve: Addr, time: f32) -> F80 {
    zero_if_negative(m, curve, time, F80::ZERO, F80::ZERO)
}

/// Parameters of a material [soft 0x082288d4 animMatParametersUpdate]. First argument only
/// (the colours: all arguments, [`anim_mat_color_update`]).
///
/// | Relation | Curve | Field | |
/// |---|---|---|---|
/// | 301, 302, 303 | 45–53 | +0x34, +0x44, +0x54 | colours |
/// | 304 `MatSpecDecay` | 54 | +0x64 | not below 0 |
/// | 305 `MatReflect` | 55 | +0x6a8 | not below 0 |
/// | 306 `MatRefl_Jit` | 56 | +0x6ac | not below 0 |
/// | 307 `MatTransp` | 57 | +0x6b8 | not below 0 |
/// | 308 `MatTrsp_Jit` | 58 | +0x6bc | not below 0 |
/// | 310 `MatRefrIndex` | 59 | +0x6dc | not below 0 |
/// | 311 `MatBlurWidth` | 257 | +0x75c | |
/// | 312 `MatBlurDecay` | 258 | +0x760 | |
///
/// 309 has no case. The source curves 313 and 314 (`DiffuseSrc`, `TransparencySrc`) are
/// outside the dispatcher's range.
pub fn anim_mat_parameters_update(m: &mut Mem, node: Addr, time: f32) {
    let mat = data(m, node);
    if mat == 0 {
        return;
    }
    let c = first_curve(m, node);
    if c == 0 {
        return;
    }
    let clamped = |m: &mut Mem, at: u32| {
        let v = non_negative(m, c, time).to_f32();
        m.set_f32(mat + at, v);
    };
    match relid(m, node) {
        0x12d..=0x12f => anim_mat_color_update(m, node, time),
        0x130 => clamped(m, o::material::SPEC_DECAY),
        0x131 => clamped(m, o::material::REFLECT),
        0x132 => clamped(m, o::material::REFLECT_JITTER),
        0x133 => clamped(m, o::material::TRANSP),
        0x134 => clamped(m, o::material::TRANSP_JITTER),
        0x136 => clamped(m, o::material::REFRACT_INDEX),
        0x137 => {
            let v = val(m, c, time).to_f32();
            m.set_f32(mat + o::material::BLUR_WIDTH, v);
        }
        0x138 => {
            let v = val(m, c, time).to_f32();
            m.set_f32(mat + o::material::BLUR_DECAY, v);
        }
        _ => {}
    }
}

/// The projection support of a 2D texture [soft 0x08229398 animTex2dVecUpdate]: relation
/// 424 rotation (+0x94, curves 206–208, degrees → radians), 425 scaling (+0xac, curves
/// 209–211, a curve without keys counting as 1), 426 translation (+0xa0, curves 212–214).
/// As with the material colours, any of the nine curve types is accepted under any of the
/// three relations.
///
/// The rotation is converted after the components are collected: all three stored values
/// are multiplied by the constant, **including a component that has no curve**, whose
/// stored radians are thus multiplied by 0.0174532925 again at every update.
pub fn anim_tex2d_vec_update(m: &mut Mem, node: Addr, time: f32) {
    let t = data(m, node);
    if t == 0 {
        return;
    }
    let id = relid(m, node);
    let at = match id {
        0x1a8 => Some(t + o::tex2d::S_ROTATION),
        0x1a9 => Some(t + o::tex2d::S_SCALING),
        0x1aa => Some(t + o::tex2d::S_TRANSLATION),
        _ => None,
    };
    let v = triple(m, node, time, at, |k| match k {
        0xce | 0xd4 => Some((0, false)),
        0xcf | 0xd5 => Some((1, false)),
        0xd0 | 0xd6 => Some((2, false)),
        0xd1 => Some((0, true)),
        0xd2 => Some((1, true)),
        0xd3 => Some((2, true)),
        _ => None,
    });
    let Some(a) = at else { return };
    if id == 0x1a8 {
        // each product is formed with the double constant in the register
        let k = F80::from_f64(DEG2RAD);
        m.set_f32(a, (k * v[0]).to_f32());
        m.set_f32(a + 4, (k * v[1]).to_f32());
        m.set_f32(a + 8, (k * v[2]).to_f32());
    } else {
        m.set_f32(a, v[0]);
        m.set_f32(a + 4, v[1]);
        m.set_f32(a + 8, v[2]);
    }
}

/// Parameters of a 2D texture [soft 0x082295d8 animTexParametersUpdate]. First argument
/// only (the support: all arguments).
///
/// | Relation | Curve | Field | |
/// |---|---|---|---|
/// | 411 `MapUScale`, 412 `MapVScale` | 33, 34 | +0x58, +0x5c | |
/// | 413 `MapUOffset`, 414 `MapVOffset` | 35, 36 | +0x68, +0x6c | |
/// | 415 `MapBlend` | 161 | +0x10c | |
/// | 416 `MapAmbient`, 417 `MapDiffuse`, 418 `MapSpec` | 37, 38, 39 | +0x128, +0x12c, +0x130 | 0 when below 0 |
/// | 419 `MapTransp` | 41 | +0x138 | |
/// | 420 `MapReflect` | 162 | +0x134 | |
/// | 421 `MapRoughness` | 43 | +0x140 | |
/// | 422 `MapRefMap` | 40 | +0x144 | 0 when below 0 |
/// | 423 `MapRotation` | 44 | +0x168 | degrees → radians |
/// | 424, 425, 426 | 206–214 | +0x94, +0xac, +0xa0 | [`anim_tex2d_vec_update`] |
///
/// Afterwards, whether or not there was a curve: `SI_texture2DSetDirty`, bit 1 of the byte
/// at +0x194, `SI_updTxt2dEffects`.
pub fn anim_tex_parameters_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let t = data(m, node);
    if t == 0 {
        return;
    }
    let c = first_curve(m, node);
    if c != 0 {
        let plain = |m: &mut Mem, at: u32| {
            let v = val(m, c, time).to_f32();
            m.set_f32(t + at, v);
        };
        let floor0 = |m: &mut Mem, at: u32| {
            let v = zero_if_negative(m, c, time, F80::ZERO, F80::ZERO).to_f32();
            m.set_f32(t + at, v);
        };
        match relid(m, node) {
            0x19b => plain(m, o::tex2d::U_SCALE),
            0x19c => plain(m, o::tex2d::V_SCALE),
            0x19d => plain(m, o::tex2d::U_OFFSET),
            0x19e => plain(m, o::tex2d::V_OFFSET),
            0x19f => plain(m, o::tex2d::BLEND),
            0x1a0 => floor0(m, o::tex2d::AMBIENT),
            0x1a1 => floor0(m, o::tex2d::DIFFUSE),
            0x1a2 => floor0(m, o::tex2d::SPECULAR),
            0x1a3 => plain(m, o::tex2d::TRANSP),
            0x1a4 => plain(m, o::tex2d::REFLECT),
            0x1a5 => plain(m, o::tex2d::ROUGHNESS),
            0x1a6 => floor0(m, o::tex2d::REFLECT_MAP),
            0x1a7 => {
                let v = (F80::from_f64(DEG2RAD) * val(m, c, time)).to_f32();
                m.set_f32(t + o::tex2d::ROTATION, v);
            }
            0x1a8..=0x1aa => anim_tex2d_vec_update(m, node, time),
            _ => {}
        }
    }
    h.texture2d_set_dirty(m, t);
    m.or_u8(t + o::tex2d::DIRTY, 2);
    h.upd_txt2d_effects(m, t);
}

/// Scale, rotation and translation of a 3D texture [soft 0x08228ab4 animTex3dVecUpdate]:
/// relation 524 scale (+0x90, curves 91–93, a curve without keys counting as 1), 525
/// rotation (+0x9c, curves 94–96, degrees → radians), 526 translation (+0xa8, curves
/// 97–99). The rotation has the same re-conversion of curveless components as
/// [`anim_tex2d_vec_update`].
pub fn anim_tex3d_vec_update(m: &mut Mem, node: Addr, time: f32) {
    let t = data(m, node);
    if t == 0 {
        return;
    }
    let id = relid(m, node);
    let at = match id {
        0x20c => Some(t + o::tex3d::SCALE),
        0x20d => Some(t + o::tex3d::ROTATION),
        0x20e => Some(t + o::tex3d::TRANSL),
        _ => None,
    };
    let v = triple(m, node, time, at, |k| match k {
        0x5b => Some((0, true)),
        0x5c => Some((1, true)),
        0x5d => Some((2, true)),
        0x5e | 0x61 => Some((0, false)),
        0x5f | 0x62 => Some((1, false)),
        0x60 | 0x63 => Some((2, false)),
        _ => None,
    });
    let Some(a) = at else { return };
    if id == 0x20d {
        let k = F80::from_f64(DEG2RAD);
        m.set_f32(a, (k * v[0]).to_f32());
        m.set_f32(a + 4, (k * v[1]).to_f32());
        m.set_f32(a + 8, (k * v[2]).to_f32());
    } else {
        m.set_f32(a, v[0]);
        m.set_f32(a + 4, v[1]);
        m.set_f32(a + 8, v[2]);
    }
}

/// The five colours of a 3D texture [soft 0x08228cf8 animTex3dColorUpdate]: relations 516
/// to 520 are colours 0 to 4, four floats each from +0x30; curves 76–90 are red, green,
/// blue of the five colours and 156–160 their alphas. No component may be negative. A curve
/// type of any of the five colours is accepted under any of the five relations.
pub fn anim_tex3d_color_update(m: &mut Mem, node: Addr, time: f32) {
    let t = data(m, node);
    if t == 0 {
        return;
    }
    let id = relid(m, node);
    let at = if (0x204..=0x208).contains(&id) { Some(t + o::tex3d::COLORS + (id - 0x204) as u32 * 16) } else { None };
    let mut v = match at {
        Some(a) => [m.f32(a), m.f32(a + 4), m.f32(a + 8), m.f32(a + 12)],
        None => [0.0; 4],
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let k = match fcv::kind(m, c) {
                0x4c | 0x4f | 0x52 | 0x55 | 0x58 => Some(0),
                0x4d | 0x50 | 0x53 | 0x56 | 0x59 => Some(1),
                0x4e | 0x51 | 0x54 | 0x57 | 0x5a => Some(2),
                0x9c..=0xa0 => Some(3),
                _ => None,
            };
            if let Some(k) = k {
                v[k] = non_negative(m, c, time).to_f32();
            }
        }
        i += 1;
    }
    if let Some(a) = at {
        for (k, x) in v.iter().enumerate() {
            m.set_f32(a + 4 * k as u32, *x);
        }
    }
}

/// Parameters of a 3D texture [soft 0x08228fa4 animTex3DParametersUpdate]. First argument
/// only (colours and vectors: all arguments).
///
/// | Relation | Curve | Field | |
/// |---|---|---|---|
/// | 511 `T3DSpacing` | 69 | +0x10 | |
/// | 512 `T3DTaper` | 72 | +0x14 | degrees → radians |
/// | 513 `T3DStrength` | 68 | +0x18 | |
/// | 514 `T3DPower` | 70 | +0x20 | |
/// | 515 `T3DBlend` | 155 | +0x28 | |
/// | 516–520 | 76–90, 156–160 | +0x30 … | [`anim_tex3d_color_update`] |
/// | 521, 522, 523 `T3DFact1`–`3` | 73, 74, 75 | +0x80, +0x84, +0x88 | limited to [0, 1] |
/// | 524, 525, 526 | 91–99 | +0x90, +0x9c, +0xa8 | [`anim_tex3d_vec_update`] |
/// | 527 `T3DAmbient`, 528 `T3DDiffuse`, 529 `T3DSpecular` | 100, 101, 102 | +0x108, +0x10c, +0x110 | 0 when below 0 |
/// | 530 `T3DTransp` | 104 | +0x118 | −1 when below −1 |
/// | 531 `T3DReflection` | 103 | +0x114 | −1 when below −1 |
/// | 532 `T3DRoughness` | 106 | +0x120 | |
///
/// Afterwards, whether or not there was a curve: `SI_updTxt3dTurb`.
pub fn anim_tex3d_parameters_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let t = data(m, node);
    if t == 0 {
        return;
    }
    let c = first_curve(m, node);
    if c != 0 {
        let plain = |m: &mut Mem, at: u32| {
            let v = val(m, c, time).to_f32();
            m.set_f32(t + at, v);
        };
        let floor0 = |m: &mut Mem, at: u32| {
            let v = zero_if_negative(m, c, time, F80::ZERO, F80::ZERO).to_f32();
            m.set_f32(t + at, v);
        };
        let floor_m1 = |m: &mut Mem, at: u32| {
            // floor: the double −1.0 (0x0864e320); replacement: the float −1.0 (0x0864e328)
            let v = zero_if_negative(m, c, time, F80::from_f64(-1.0), F80::from_f32(-1.0)).to_f32();
            m.set_f32(t + at, v);
        };
        // stored first, then replaced by 0 when the unrounded value is at or below 0, else
        // by 1 when the stored value is at least 1 (a NaN stays)
        let unit = |m: &mut Mem, at: u32| {
            let v = val(m, c, time);
            m.set_f32(t + at, v.to_f32());
            if v.lt(F80::ZERO) || v.eq(F80::ZERO) {
                m.set_f32(t + at, 0.0);
            } else if F80::from_f32(m.f32(t + at)).ge(F80::ONE) {
                m.set_f32(t + at, 1.0);
            }
        };
        match relid(m, node) {
            0x1ff => plain(m, o::tex3d::SPACING),
            0x200 => {
                let v = (F80::from_f64(DEG2RAD) * val(m, c, time)).to_f32();
                m.set_f32(t + o::tex3d::TAPER, v);
            }
            0x201 => plain(m, o::tex3d::STRENGTH),
            0x202 => plain(m, o::tex3d::POWER),
            0x203 => plain(m, o::tex3d::BLEND),
            0x204..=0x208 => anim_tex3d_color_update(m, node, time),
            0x209 => unit(m, o::tex3d::FACTORS),
            0x20a => unit(m, o::tex3d::FACTORS + 4),
            0x20b => unit(m, o::tex3d::FACTORS + 8),
            0x20c..=0x20e => anim_tex3d_vec_update(m, node, time),
            0x20f => floor0(m, o::tex3d::AMBIENT),
            0x210 => floor0(m, o::tex3d::DIFFUSE),
            0x211 => floor0(m, o::tex3d::SPECULAR),
            0x212 => floor_m1(m, o::tex3d::TRANSP),
            0x213 => floor_m1(m, o::tex3d::REFLECTION),
            0x214 => plain(m, o::tex3d::ROUGHNESS),
            _ => {}
        }
    }
    h.upd_txt3d_turb(m, t);
}

fn by_type(m: &mut Mem, node: Addr, time: f32, obj: Addr, map: &[(i32, u32)]) {
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let v = val(m, c, time);
            let k = fcv::kind(m, c);
            if let Some(&(_, at)) = map.iter().find(|&&(t, _)| t == k) {
                m.set_f32(obj + at, v.to_f32());
            }
        }
        i += 1;
    }
}

/// Depth fading of the scene (relations 3000–3003) [soft 0x082298b0 animFadParametersUpdate]:
/// by curve type, into the render control record: 60 start (+0x104), 61 stop (+0x10c), 62,
/// 63, 64 red, green, blue (+0x118, +0x11c, +0x120), 65 transparency (+0x130). Every curve
/// of the node is evaluated.
///
/// When the update was asked for by a renderer (`fromRendering`) and the record is
/// `SI_defaultControl`, the routine runs a second time on the renderer's own record
/// `SI_ren` (the curves are evaluated again).
pub fn anim_fad_parameters_update(m: &mut Mem, node: Addr, time: f32) {
    use o::control as c;
    let ctl = data(m, node);
    if ctl == 0 {
        return;
    }
    by_type(m, node, time, ctl, &[(0x3c, c::FADE_START), (0x3d, c::FADE_STOP), (0x3e, c::FADE_RED), (0x3f, c::FADE_GREEN),
                                   (0x40, c::FADE_BLUE), (0x41, c::FADE_TRANSP)]);
    if m.u16(glob::FROM_RENDERING) != 0 && data(m, node) == glob::SI_DEFAULT_CONTROL {
        m.set_ptr(node + crate::off::updnode::DATA, SI_REN);
        anim_fad_parameters_update(m, node, time);
        m.set_ptr(node + crate::off::updnode::DATA, glob::SI_DEFAULT_CONTROL);
    }
}

/// Layered fog of the scene (relations 3030–3035) [soft 0x08229998 animFogParametersUpdate]:
/// 198 start (+0x26c), 199 stop (+0x270), 200, 201, 202 red, green, blue (+0x284, +0x288,
/// +0x28c), 203 density (+0x260), 204 base (+0x264), 205 thickness (+0x268); the names of
/// the last three are those `Relations.rsrc` gives the curve types (`fodens`, `fobase`,
/// `fothick`). Repeated on `SI_ren` like the depth fading.
pub fn anim_fog_parameters_update(m: &mut Mem, node: Addr, time: f32) {
    use o::control as c;
    let ctl = data(m, node);
    if ctl == 0 {
        return;
    }
    by_type(m, node, time, ctl, &[(0xc6, c::FOG_START), (199, c::FOG_STOP), (200, c::FOG_RED), (0xc9, c::FOG_GREEN),
                                   (0xca, c::FOG_BLUE), (0xcb, c::FOG_DENSITY), (0xcc, c::FOG_BASE), (0xcd, c::FOG_THICKNESS)]);
    if m.u16(glob::FROM_RENDERING) != 0 && data(m, node) == glob::SI_DEFAULT_CONTROL {
        m.set_ptr(node + crate::off::updnode::DATA, SI_REN);
        anim_fog_parameters_update(m, node, time);
        m.set_ptr(node + crate::off::updnode::DATA, glob::SI_DEFAULT_CONTROL);
    }
}

// ---- cameras

/// Keeps a stored value away from zero: when the magnitude of `unrounded` is below 0.0001,
/// the field becomes ±0.0001 (as a float: 9.9999997e-05) with the sign of the stored value,
/// zero counting as positive.
fn away_from_zero(m: &mut Mem, at: Addr, unrounded: F80) {
    if unrounded.abs().lt(F80::from_f64(0.0001)) {
        let tiny = f32::from_bits(0x38d1_b717);
        let v = if F80::from_f32(m.f32(at)).lt(F80::ZERO) { -tiny } else { tiny };
        m.set_f32(at, v);
    }
}

/// field of view = 2 · atan((height · 0.5) / distance), from the projection plane.
fn fov_from_plane(m: &mut Mem, cam: Addr) {
    use o::camera as c;
    let q = F80::from_f32(m.f32(cam + c::PP_HEIGHT)) * 0.5f64 / m.f32(cam + c::PP_DISTANCE);
    let a = trig::fpatan(q, F80::ONE);
    m.set_f32(cam + c::FOV, (a + a).to_f32());
}

/// Parameters of a camera [soft 0x082278a0 animCameraParametersUpdate]. First argument
/// only, except the projection plane's size and offset.
///
/// | Relation | Curve | Field | |
/// |---|---|---|---|
/// | 1100 `CamVisib` | 177 | | every graf node of kind 4 or 5 of the current graf list (**any** camera's, not only this one's): display type 2 and the element's wire type 2 when the value equals 1; else display type 0 and bit 1 of the element's wire type cleared |
/// | 1130 `CamRoll` | 11 | +0x30 | degrees → radians |
/// | 1131 `CamFov` | 12 | +0x40 | degrees → radians, kept away from zero; then the projection plane's distance (+0x10c) = (0.5 · height) / tan(0.5 · fov) |
/// | 1132 `CamNear`, 1133 `CamFar` | 13, 14 | +0x34, +0x38 | |
/// | 1134 `CamFocLen` | 15 | +0x70 | |
/// | 1135 `CamFStop` | 16 | +0x94 | |
/// | 1136 `CamFocDst` | 17 | +0x74 | |
/// | 1137 `CamNearFoc`, 1138 `CamFarFoc` | 18, 19 | +0x7c, +0x84 | |
/// | 1139 `CamFout` | 20 | +0x8c | |
/// | 1140 `CamCoc` | 21 | +0x98 | |
/// | 1141 `CamPPDist` | 373 | +0x10c | kept away from zero; then fov = 2 · atan(0.5 · height / distance) |
/// | 1142 `CamPPSize` | 374, 375 | +0x110, +0x114 | each kept away from zero; a height curve also recomputes the fov as above |
/// | 1143 `CamPPOffset` | 376, 377 | +0x118, +0x11c | |
/// | 1144 `CamPPActive` | 378 | +0x108, 16 bits | the value truncated |
///
/// Under 1142 and 1143 all four curve types are accepted.
pub fn anim_camera_parameters_update(m: &mut Mem, node: Addr, time: f32) {
    use o::camera as c;
    let e = node_elem(m, node);
    let cam = data(m, node);
    if cam == 0 {
        return;
    }
    let id = relid(m, node);
    if (id.wrapping_sub(0x476) as u32) < 2 {
        let mut i = 0;
        while i < nb_args(m, node) {
            let cv = arg_curve(m, node, i);
            if cv != 0 {
                let v = val(m, cv, time);
                match fcv::kind(m, cv) {
                    0x177 => {
                        m.set_f32(cam + c::PP_HEIGHT, v.to_f32());
                        away_from_zero(m, cam + c::PP_HEIGHT, v);
                        fov_from_plane(m, cam);
                    }
                    0x176 => {
                        m.set_f32(cam + c::PP_WIDTH, v.to_f32());
                        away_from_zero(m, cam + c::PP_WIDTH, v);
                    }
                    0x178 => m.set_f32(cam + c::PP_OFFSET_X, v.to_f32()),
                    0x179 => m.set_f32(cam + c::PP_OFFSET_Y, v.to_f32()),
                    _ => {}
                }
            }
            i += 1;
        }
        return;
    }
    let cv = first_curve(m, node);
    if cv == 0 {
        return;
    }
    let v = val(m, cv, time);
    let plain = |m: &mut Mem, at: u32| m.set_f32(cam + at, v.to_f32());
    match id {
        0x44c => {
            let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
            while gn != 0 {
                if (m.i32(gn).wrapping_sub(4) as u32) < 2 {
                    if v.eq(F80::ONE) {
                        m.set_i32(gn + o::GN_DISPLAY, 2);
                        m.set_i32(e + 0x20, 2);
                    } else {
                        m.set_i32(gn + o::GN_DISPLAY, 0);
                        m.and_u8(e + 0x20, 0xfd);
                    }
                }
                gn = m.ptr(gn + 0x1c);
            }
        }
        0x46a => m.set_f32(cam + c::ROLL, (F80::from_f64(DEG2RAD) * v).to_f32()),
        0x46b => {
            // rounded to float before it is stored and tested
            let fov = (F80::from_f64(DEG2RAD) * v).to_f32();
            m.set_f32(cam + c::FOV, fov);
            away_from_zero(m, cam + c::FOV, F80::from_f32(fov));
            let half = F80::from_f64(0.5);
            let t = trig::tan((half * m.f32(cam + c::FOV)).to_f64());
            let d = (half * m.f32(cam + c::PP_HEIGHT)) / t;
            m.set_f32(cam + c::PP_DISTANCE, d.to_f32());
        }
        0x46c => plain(m, c::NEAR),
        0x46d => plain(m, c::FAR),
        0x46e => plain(m, c::FOCAL_LENGTH),
        0x46f => plain(m, c::F_STOP),
        0x470 => plain(m, c::FOCAL_DISTANCE),
        0x471 => plain(m, c::NEAR_FOCUS),
        0x472 => plain(m, c::FAR_FOCUS),
        0x473 => plain(m, c::OUT_OF_FOCUS),
        0x474 => plain(m, c::MAXIMUM_COC),
        0x475 => {
            m.set_f32(cam + c::PP_DISTANCE, v.to_f32());
            away_from_zero(m, cam + c::PP_DISTANCE, v);
            fov_from_plane(m, cam);
        }
        0x478 => m.set_i16(cam + c::PP_ACTIVE, v.to_i32(Round::Zero) as i16),
        _ => {}
    }
}

/// Position (relation 1113, +0x10) and interest (1115, +0x1c) of a camera
/// [soft 0x08227790 animCameraExplicitPosUpdate]: curves 142/145 x, 143/146 y, 144/147 z;
/// the position's and the interest's curve types are accepted under either relation.
pub fn anim_camera_explicit_pos_update(m: &mut Mem, node: Addr, time: f32) {
    let cam = data(m, node);
    if cam == 0 {
        return;
    }
    let at = match relid(m, node) {
        0x459 => Some(cam + o::camera::POSITION),
        0x45b => Some(cam + o::camera::INTEREST),
        _ => None,
    };
    let mut v = match at {
        Some(a) => [m.f32(a), m.f32(a + 4), m.f32(a + 8)],
        None => [0.0; 3],
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            // evaluated whatever its type
            let x = val(m, c, time);
            match fcv::kind(m, c) {
                0x8e | 0x91 => v[0] = x.to_f32(),
                0x8f | 0x92 => v[1] = x.to_f32(),
                0x90 | 0x93 => v[2] = x.to_f32(),
                _ => {}
            }
        }
        i += 1;
    }
    if let Some(a) = at {
        m.set_f32(a, v[0]);
        m.set_f32(a + 4, v[1]);
        m.set_f32(a + 8, v[2]);
    }
}

/// The average of the positions of a node's active constraining objects: the body shared by
/// the camera and light position constraints. An argument counts when it has an object, is
/// active, sits in relation list 1 of its element, and its activation curve (if any) reads
/// above 0 at `time` (`SI_fcurveIntegerValue(curve, time, 0, 1)`: the raw time, as a flag).
/// Each position comes from `SI_constrLocation(object, graf kind, &p)`. Returns the count
/// and the three sums, accumulated in `float`s.
fn constraint_sum<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) -> (i32, [f32; 3]) {
    let mut sum = [0.0f32; 3];
    let mut n = 0;
    let p = m.local(12);
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        let obj = m.ptr(a + updelem::DATA);
        if obj != 0 && m.i16(a + updelem::ACTIVE) != 0 && m.i16(a + updelem::GROUP) == 1 {
            let act = m.ptr(a + updelem::ACT_FCV);
            if act == 0 || fcv::integer_value(m, act, time, 0, 1) > 0 {
                let obj = m.ptr(a + updelem::DATA);
                if obj != 0 {
                    let kind = h.db_chapter2graf_type(m, m.i16(a + updelem::CHAPID) as i32, m.i32(a + updelem::ELEMID));
                    super::transfo::constr_location(m, obj, kind, p);
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += m.f32(p + 4 * k as u32);
                    }
                    n += 1;
                }
            }
        }
        i += 1;
    }
    m.release(12);
    (n, sum)
}

fn store_average(m: &mut Mem, at: Addr, n: i32, sum: [f32; 3]) {
    // the reciprocal stays in the register, unrounded, for the three products
    let r = F80::ONE / F80::from_i32(n);
    m.set_f32(at, (r * sum[0]).to_f32());
    m.set_f32(at + 4, (r * sum[1]).to_f32());
    m.set_f32(at + 8, (r * sum[2]).to_f32());
}

/// Position constraint of a camera (relation 1111) or of its interest (1116)
/// [soft 0x08227610 animCameraConstraintUpdate]: the constrained point is set to the
/// average position of the active constraining objects. Which point is decided by the graf
/// kind of the node's element (for 1111 the kind of the camera the interest belongs to,
/// `SI_edGetCamFromCamInt`): 4 the position, 5 the interest.
pub fn anim_camera_constraint_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let cam = data(m, node);
    if cam == 0 {
        return;
    }
    let mut id = m.i32(node + crate::off::updnode::ELEMID);
    if relid(m, node) == 0x457 {
        id = h.ed_get_cam_from_cam_int(m, id);
    }
    let kind = h.db_chapter2graf_type(m, m.i16(node) as i32, id);
    let (n, sum) = constraint_sum(m, h, node, time);
    if n > 0 {
        match kind {
            4 => store_average(m, cam + o::camera::POSITION, n, sum),
            5 => store_average(m, cam + o::camera::INTEREST, n, sum),
            _ => {}
        }
    }
}

// ---- lights

/// Parameters of a light [soft 0x08228004 animLightParametersUpdate]. Every active argument
/// is treated (the loop is outside the switch).
///
/// | Relation | Curve | Field | |
/// |---|---|---|---|
/// | 2100 `LigVisib` | 176 | | the light's graf nodes (kinds 6, 7, 8, 0x13, 0x14 whose object is the light): display type 2, light + 0x176 = 1 and the element's wire type 2 when the value equals 1; else 0, 0 and bit 1 of the wire type cleared |
/// | 2130 `LigColor` | 22, 23, 24 | +0x24, +0x28, +0x2c | |
/// | 2131 `LigCone`, 2132 `LigSpread` | 25, 26 | +0x70, +0x74 | degrees → radians |
/// | 2134 `LigStart`, 2135 `LigEnd` | 28, 29 | +0x84, +0x88 | |
/// | 2136 `LigShadow` | 30 | +0x90 | |
/// | 2201 `LigARotn` | 215, 216, 217 | +0x1d0 … | the three angles are first re-read from the area's rotation matrix (+0x1e0), the curve's component is replaced by the curve's value **as it is** (no conversion from degrees), the matrix is rebuilt, and the area's shape is rebuilt (rectangle or disk) |
/// | 2202 `LigAScale` | 218, 219, 220 | +0x1c4, +0x1c8, +0x1cc | the x scale is also the radius of a sphere (+0x190) or disk (+0x19c) area; shape rebuilt |
/// | 2203 `Energy` | 621, 622, 623 | +0x224, +0x228, +0x22c | 0 when below 0 or not a number |
/// | 2204 `Exponent` | 624 | +0x230 | 0 when below 0 or not a number |
/// | 2205 `CsPhNbr`, 2206 `GIPhNbr` | 625, 626 | +0x234, +0x238 (32-bit integers) | truncated; 0 when below 0 or not a number |
/// | 2207 `Caustic`, 2208 `GlbIlm` | 627, 628 | +0x23c, +0x23e (16 bits) | 1 when the value equals 1 |
///
/// 2133 (0x855) has no case.
pub fn anim_light_parameters_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    use o::light as l;
    let e = node_elem(m, node);
    let lig = data(m, node);
    if lig == 0 {
        return;
    }
    // "0 ≤ value": a negative value and a NaN give 0
    let positive = |v: F80| F80::ZERO.lt(v) || F80::ZERO.eq(v);
    let rebuild = |m: &mut Mem, h: &mut H| {
        if m.i16(lig + l::AREA_LIGHT) != 0 {
            match m.i32(lig + l::AREA_TYPE) {
                0 => h.set_area_rect(m, lig),
                2 => h.set_area_disk(m, lig),
                _ => {}
            }
        }
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            match relid(m, node) {
                0x834 => {
                    let v = val(m, c, time);
                    let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
                    while gn != 0 {
                        let k = m.i32(gn);
                        if matches!(k, 8 | 6 | 7 | 0x14 | 0x13) && m.ptr(gn + 4) == lig {
                            if v.eq(F80::ONE) {
                                m.set_i32(gn + o::GN_DISPLAY, 2);
                                m.set_i16(lig + l::VISIBLE, 1);
                                m.set_i32(e + 0x20, 2);
                            } else {
                                m.set_i32(gn + o::GN_DISPLAY, 0);
                                m.set_i16(lig + l::VISIBLE, 0);
                                m.and_u8(e + 0x20, 0xfd);
                            }
                        }
                        gn = m.ptr(gn + 0x1c);
                    }
                }
                0x852 => {
                    let at = match fcv::kind(m, c) {
                        0x16 => Some(l::COLOUR),
                        0x17 => Some(l::COLOUR + 4),
                        0x18 => Some(l::COLOUR + 8),
                        _ => None,
                    };
                    if let Some(at) = at {
                        let v = val(m, c, time).to_f32();
                        m.set_f32(lig + at, v);
                    }
                }
                0x853 => {
                    let v = (F80::from_f64(DEG2RAD) * val(m, c, time)).to_f32();
                    m.set_f32(lig + l::CONE_ANGLE, v);
                }
                0x854 => {
                    let v = (F80::from_f64(DEG2RAD) * val(m, c, time)).to_f32();
                    m.set_f32(lig + l::SPREAD_ANGLE, v);
                }
                0x856 => {
                    let v = val(m, c, time).to_f32();
                    m.set_f32(lig + l::FALL_OFF_START, v);
                }
                0x857 => {
                    let v = val(m, c, time).to_f32();
                    m.set_f32(lig + l::FALL_OFF_END, v);
                }
                0x858 => {
                    let v = val(m, c, time).to_f32();
                    m.set_f32(lig + l::SHADOW_FACTOR, v);
                }
                0x899 => {
                    m.set_f32(lig + l::AREA_ROTATION, 0.0);
                    m.set_f32(lig + l::AREA_ROTATION + 4, 0.0);
                    m.set_f32(lig + l::AREA_ROTATION + 8, -1.0);
                    let mat = m.matrix(lig + l::AREA_ROTATION_MAT);
                    let prev = m.vector(lig + l::AREA_ROTATION);
                    let keep = m.i16(KEEP_TURNS) != 0;
                    let rot = algebra::matrix_extract_xyz_rotation(&mat, &prev, keep);
                    m.set_vector(lig + l::AREA_ROTATION, rot);
                    let at = match fcv::kind(m, c) {
                        0xd7 => Some(0),
                        0xd8 => Some(4),
                        0xd9 => Some(8),
                        _ => None,
                    };
                    if let Some(at) = at {
                        let v = val(m, c, time).to_f32();
                        m.set_f32(lig + l::AREA_ROTATION + at, v);
                    }
                    let rot = m.vector(lig + l::AREA_ROTATION);
                    let mat = algebra::matrix_xyz_rotation(&rot);
                    m.set_matrix(lig + l::AREA_ROTATION_MAT, &mat);
                    rebuild(m, h);
                }
                0x89a => {
                    match fcv::kind(m, c) {
                        0xda => {
                            let v = val(m, c, time);
                            m.set_f32(lig + l::AREA_SCALE, v.to_f32());
                            if m.i16(lig + l::AREA_LIGHT) == 0 {
                                // an area light that is off: nothing more for this curve,
                                // the shape is not rebuilt either
                                i += 1;
                                continue;
                            }
                            match m.i32(lig + l::AREA_TYPE) {
                                1 => m.set_f32(lig + l::AREA_GEOMETRY, v.to_f32()),
                                2 => m.set_f32(lig + l::AREA_DISK_RADIUS, v.to_f32()),
                                _ => {}
                            }
                        }
                        0xdb => {
                            let v = val(m, c, time).to_f32();
                            m.set_f32(lig + l::AREA_SCALE + 4, v);
                        }
                        0xdc => {
                            let v = val(m, c, time).to_f32();
                            m.set_f32(lig + l::AREA_SCALE + 8, v);
                        }
                        _ => {}
                    }
                    rebuild(m, h);
                }
                0x89b => {
                    let v = val(m, c, time);
                    let at = match fcv::kind(m, c) {
                        0x26d => Some(l::ENERGY),
                        0x26e => Some(l::ENERGY + 4),
                        0x26f => Some(l::ENERGY + 8),
                        _ => None,
                    };
                    if let Some(at) = at {
                        m.set_f32(lig + at, if positive(v) { v.to_f32() } else { 0.0 });
                    }
                }
                0x89c => {
                    let v = val(m, c, time);
                    m.set_f32(lig + l::EXPONENT, if positive(v) { v.to_f32() } else { 0.0 });
                }
                0x89d => {
                    let v = val(m, c, time);
                    // the conversion goes through a 64-bit integer whose low half is kept
                    m.set_i32(lig + l::CAUSTIC_PHOTON, if positive(v) { v.to_i64(Round::Zero) as i32 } else { 0 });
                }
                0x89e => {
                    let v = val(m, c, time);
                    m.set_i32(lig + l::GLOBILLUM_PHOTON, if positive(v) { v.to_i64(Round::Zero) as i32 } else { 0 });
                }
                0x89f => {
                    let v = val(m, c, time);
                    m.set_u16(lig + l::CAUSTIC_ENABLE, v.eq(F80::ONE) as u16);
                }
                0x8a0 => {
                    let v = val(m, c, time);
                    m.set_u16(lig + l::GLOBILLUM_ENABLE, v.eq(F80::ONE) as u16);
                }
                _ => {}
            }
        }
        i += 1;
    }
}

/// Position (relation 2113, +0x34) and interest (2115, +0x58) of a light
/// [soft 0x08227e10 animLightExplicitPosUpdate]: curves 148/151 x, 149/152 y, 150/153 z.
/// Only curves of these six types are evaluated.
pub fn anim_light_explicit_pos_update(m: &mut Mem, node: Addr, time: f32) {
    let lig = data(m, node);
    if lig == 0 {
        return;
    }
    let at = match relid(m, node) {
        0x841 => Some(lig + o::light::POSITION),
        0x843 => Some(lig + o::light::INTEREST),
        _ => None,
    };
    let v = triple(m, node, time, at, |k| match k {
        0x94 | 0x97 => Some((0, false)),
        0x95 | 0x98 => Some((1, false)),
        0x96 | 0x99 => Some((2, false)),
        _ => None,
    });
    if let Some(a) = at {
        m.set_f32(a, v[0]);
        m.set_f32(a + 4, v[1]);
        m.set_f32(a + 8, v[2]);
    }
}

/// The sun's time (relation 2120, curve 189) [soft 0x08227f30 animLightSunTimeUpdate]: the
/// sun record's date and time (a `time_t` at +0x10 of the record at light + 0x16c) is
/// replaced by the curve's value, truncated; without a curve of type 189 the integer goes
/// through the unit unchanged. Then the light's position is taken from
/// `SI_lightSunGetPosition`.
pub fn anim_light_sun_time_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    use o::light as l;
    let lig = data(m, node);
    if lig == 0 || m.ptr(lig + l::SUN) == 0 {
        return;
    }
    let mut t = F80::from_i32(m.i32(m.ptr(lig + l::SUN) + l::SUN_DATE_TIME));
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 && fcv::kind(m, c) == 0xbd {
            t = val(m, c, time);
        }
        i += 1;
    }
    m.set_i32(m.ptr(lig + l::SUN) + l::SUN_DATE_TIME, t.to_i32(Round::Zero));
    let p = m.local(12);
    h.light_sun_get_position(m, lig, p);
    for k in 0..3 {
        let w = si_x87::fmov(m.f32(p + 4 * k));
        m.set_f32(lig + l::POSITION + 4 * k, w);
    }
    m.release(12);
}

/// Position constraint of a light (relation 2111) or of its interest (2116)
/// [soft 0x08227c80 animLightConstraintUpdate]: as for cameras; graf kind 7 is the interest,
/// 6, 8 and 0x13 the light itself.
pub fn anim_light_constraint_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let lig = data(m, node);
    if lig == 0 {
        return;
    }
    let mut id = m.i32(node + crate::off::updnode::ELEMID);
    if relid(m, node) == 0x83f {
        id = h.ed_get_lig_from_lig_int(m, id);
    }
    let kind = h.db_chapter2graf_type(m, m.i16(node) as i32, id);
    let (n, sum) = constraint_sum(m, h, node, time);
    if n > 0 {
        match kind {
            7 => store_average(m, lig + o::light::INTEREST, n, sum),
            6 | 8 | 0x13 => store_average(m, lig + o::light::POSITION, n, sum),
            _ => {}
        }
    }
}

/// Chapter test used by tests and the table: the chapter index of MODELS.
pub fn models_chapter(m: &Mem) -> i32 {
    glob::chap(m, cid::MODELS)
}
