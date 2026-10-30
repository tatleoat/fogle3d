// SPDX-License-Identifier: MIT
//! The update at a time: `motionUpdate.c` (finding 0017).
//!
//! An update walks an **update list** ([`crate::topo`] builds it): an array of nodes, each
//! "bring this aspect of this element up to date", already in dependency order. A node
//! names an element (chapter, index, object), one of the element's relation lists (which is
//! also the kind of update: parameters, transformation, geometry …), a relation id, and the
//! arguments of that relation: for an animated parameter, the function curves.
//!
//! - this file: the drivers (`SI_sceneUpdate` … `SI_updListUpdate`), the per-node gate
//!   (`SI_checkActiveTime`) and the three dispatchers by relation id;
//! - [`params`]: parameters of models, cameras, lights, materials, textures, the scene;
//! - [`transfo`]: scaling, rotation, translation, constraints, paths;
//! - [`geom`]: shapes, clusters, lattices, deformations, envelopes.

use crate::ed::{self, EdHost};
use crate::fcv;
use crate::glob::{self, anim, cid};
use crate::mem::MemExt;
use si_mem::{Addr, Mem};
use crate::off::{dict, elem, list, model, updelem, updlist, updnode};
use si_x87::{Round, F80};

pub mod constr;
pub mod geom;
pub mod params;
pub mod shape;
pub mod texfile;
pub mod transfo;

/// Relation ids the dispatchers and the gate single out. Names are those of
/// `Relations.rsrc` (finding 0006, section 7); the numbers are the ones the code compares.
pub mod rel {
    pub const TRANSFO_UPDATE: i32 = -100;
    pub const WAV_TRANSFO_UPD: i32 = -101;
    pub const GEOM_UPDATE: i32 = -200;
    pub const T2D_SCRIPT_UPDATE: i32 = -400;
    pub const NVISIBILITY: i32 = 10;
    pub const ALGO_TYPE: i32 = 11;
    pub const BVISIBILITY: i32 = 12;
    pub const NMATTE: i32 = 15;
    pub const BMATTE: i32 = 16;
    pub const ELASTICITY: i32 = 50;
    pub const STAT_ROUGH: i32 = 51;
    pub const KINE_ROUGH: i32 = 52;
    pub const ROOT_CONS: i32 = 53;
    pub const SCALING: i32 = 100;
    pub const ROTATION: i32 = 101;
    pub const CNS_POS_LIM: i32 = 102;
    pub const CNS_ROT_LIM: i32 = 103;
    pub const CNS_POS: i32 = 111;
    pub const PATH: i32 = 112;
    pub const EXPL_TRANS: i32 = 113;
    pub const CNS_ORI: i32 = 114;
    pub const CNS_SCL: i32 = 115;
    pub const CNS_DIR: i32 = 116;
    pub const CNS_PLN: i32 = 117;
    pub const CNS_PIV_PLN: i32 = 118;
    pub const CNS_3PNTS: i32 = 119;
    pub const INV_KIN: i32 = 120;
    pub const CNS_TANG: i32 = 124;
    pub const CNS_NORM: i32 = 125;
    pub const CNS_OBJ_PNT: i32 = 126;
    pub const CNS_2PNTS: i32 = 127;
    pub const CNS_UP_VCT: i32 = 128;
    pub const CNS_PREF_AXIS: i32 = 129;
    pub const SYMMETRY: i32 = 150;
    pub const TRAJECTORY: i32 = 160;
    pub const SHAPE: i32 = 200;
    pub const RIGD_ENV: i32 = 206;
    pub const FLEX_ENV: i32 = 207;
    pub const NLATTICE: i32 = 210;
    pub const NLAT_INTERP: i32 = 211;
    pub const BLATTICE: i32 = 215;
    pub const BLAT_INTERP: i32 = 216;
    pub const QS_CONTROL: i32 = 238;
    pub const QSTRETCH: i32 = 239;
    pub const BOOL: i32 = 245;
    pub const CNS_FPNT_OBJ: i32 = 246;
    pub const CLUSTER_CTR: i32 = 247;
    pub const EXTRUSION: i32 = 250;
    pub const REVOLUTION: i32 = 251;
    pub const SKINNING: i32 = 252;
    pub const FOUR_SIDED: i32 = 253;
    pub const ZIP: i32 = 255;
    pub const EXTRACT: i32 = 256;
    pub const TRIM: i32 = 257;
    pub const INVERSE: i32 = 260;
    pub const NWAVE: i32 = 600;
    pub const BWAVE: i32 = 601;
    pub const WAV_SCALE: i32 = 610;
    pub const WAV_ROTATION: i32 = 611;
    pub const WAV_PATH: i32 = 612;
    pub const WAV_ETRN: i32 = 613;
    pub const WAV_DECAY: i32 = 614;
    pub const WAV_SPREAD: i32 = 615;
    pub const CLUSTER: i32 = 700;
    pub const DYNAMICS: i32 = 800;
    pub const DYN_NAIL: i32 = 806;
    pub const CAM_VISIB: i32 = 1100;
    pub const CAM_CONSTR: i32 = 1111;
    pub const CAM_PATH: i32 = 1112;
    pub const CAM_ETRN: i32 = 1113;
    pub const CAM_INT_PATH: i32 = 1114;
    pub const CAM_INT_ETRN: i32 = 1115;
    pub const CAM_INT_CONS: i32 = 1116;
    pub const CAM_UP_VCT: i32 = 1117;
    pub const CAM_ROLL: i32 = 1130;
    pub const CAM_PP_ACTIVE: i32 = 1144;
    pub const LIG_VISIB: i32 = 2100;
    pub const LIG_CONSTR: i32 = 2111;
    pub const LIG_PATH: i32 = 2112;
    pub const LIG_ETRN: i32 = 2113;
    pub const LIG_INT_PATH: i32 = 2114;
    pub const LIG_INT_ETRN: i32 = 2115;
    pub const LIG_INT_CONS: i32 = 2116;
    pub const LIG_SUN_TIME: i32 = 2120;
    pub const CUSTOM: i32 = 20000;
    pub const EXP: i32 = 20010;
}

crate::host_trait! {
    /// What `motionUpdate.c` calls outside this package, each with the arguments the
    /// original passes, in its order.
    pub trait UpdateHost: EdHost {
        // ---- drivers
        /// `SI_fsliderDraw(frame)` (fslider.c): the frame slider shows the frame.
        fn fslider_draw(frame: i32);
        /// `SI_statusDisplayMessage(text, kind)` (status.c). The text is passed by address.
        fn status_display_message(text: Addr, kind: i32);
        /// `getbutton(code)` (the IRIS GL layer): whether a key is down.
        fn getbutton(code: i32) -> i32;
        /// `SI_updlistDump(dict, reldict, list, names)` (updlist.c): a debugging listing.
        fn updlist_dump(dict: Addr, reldict: Addr, list: Addr, names: i16);
        /// `SI_cusPreSceneTransfer()` (custom_mn.c).
        fn cus_pre_scene_transfer();
        /// `SI_cusSceneUpdate(time)` (custom_mn.c).
        fn cus_scene_update(time: f32);
        /// `SI_sceneKeySetColor(time, 1, 0)` (motionUtils.c).
        fn scene_key_set_color(time: f32, a: i16, b: i16);
        /// `SI_envSkelResetFlag(dict)` (envtools.c).
        fn env_skel_reset_flag(dict: Addr);
        /// `SI_dynachnGeometryUpdate(model)` (settings.c).
        fn dynachn_geometry_update(model: Addr);
        /// `SI_modelKineStateCompute(model, dt)` (settings.c): velocities and accelerations.
        fn model_kine_state_compute(model: Addr, dt: f32);
        /// `SI_cusUpdateEffect(node, time)` (custom_dso.c): a custom effect's update.
        fn cus_update_effect(node: Addr, time: f32);
        /// `SI_animExpressionEvaluate(node, time)` (exp_fct.c): an expression drives a
        /// parameter of the node's element.
        fn anim_expression_evaluate(node: Addr, time: f32);
        /// `SI_dopeIsElemActive(chapter, element)` (dopeShMng.c).
        fn dope_is_elem_active(chap: i32, elemid: i32) -> i16;
        /// `SI_findGlData(graf list, object, kind)` (graflist.c): the object's graf node.
        fn find_gl_data(graf_list: Addr, data: Addr, kind: i32) -> Addr;
        /// `SI_elemdictIsElemUsed(elem, flag)` (elemdict.c): whether another element depends
        /// on this one.
        fn elemdict_is_elem_used(elem: Addr, flag: i16) -> i16;
        /// `SI_reldictNbRelForDeactCount(reldict, relation, arguments)` (reldict.c).
        fn reldict_nb_rel_for_deact_count(reldict: Addr, relid: i32, nb_args: i32) -> i32;
        /// `SI_getConstraintUpdateList(2, 0)` (constraint.c).
        fn get_constraint_update_list(a: i32, b: i32) -> i16;
        /// `SI_modelIsMaster(model)` (instance.c).
        fn model_is_master(model: Addr) -> i16;
        /// `SI_sceneConstrUpdate(5, time, origin)` (constraint.c).
        fn scene_constr_update(mode: i32, time: f32, origin: Addr);
        /// `SI_refreshSystem(port, list, code)` (draw.c).
        fn refresh_system(port: Addr, graf_list: Addr, code: i32);

        // ---- parameters
        /// `SI_modelSetHiddenParam(graf node, model, elem, state, branch)` (edparams.c).
        fn model_set_hidden_param(gn: Addr, model: Addr, elem: Addr, state: i32, branch: i32);
        /// `SI_modelPropagateMatteParam(model, value)` (edparams.c).
        fn model_propagate_matte_param(model: Addr, value: f32);
        /// `SI_skeChainRoot(model)` (skeleton.c): the root of the chain a joint is in.
        fn ske_chain_root(model: Addr) -> Addr;
        /// `SI_dynaAlgoTypeSet(model, on)` (settings.c).
        fn dyna_algo_type_set(model: Addr, on: i16);
        /// `SI_matrixExtractXyzRotation`/`SI_matrixXyzRotation` are `si-math`; the area
        /// light's shape is rebuilt by `SI_setAreaRect(light)` (graflist.c) …
        fn set_area_rect(light: Addr);
        /// … or `SI_setAreaDisk(light)` (graflist.c).
        fn set_area_disk(light: Addr);
        /// `SI_lightSunGetPosition(light, &position)` (sun_fct.c): writes three floats.
        fn light_sun_get_position(light: Addr, out: Addr);
        /// `SI_texture2DSetDirty(texture)` (t2d_dial.c).
        fn texture2d_set_dirty(tex: Addr);
        /// `SI_updTxt2dEffects(texture)` (t2d_fct.c).
        fn upd_txt2d_effects(tex: Addr);
        /// `SI_updTxt3dTurb(texture)` (t3d_fct.c).
        fn upd_txt3d_turb(tex: Addr);
        /// `fopen(path, mode)` (C library): a stream, or null.
        fn fopen(path: Addr, mode: Addr) -> Addr;
        /// `fgets(buffer, size, stream)` (C library): the buffer, or null at the end.
        fn fgets(buf: Addr, size: i32, stream: Addr) -> Addr;
        /// `fclose(stream)` (C library).
        fn fclose(stream: Addr) -> i32;
        /// `SI_stripPicExtension(text)` (t2d_dial.c): takes blanks and the picture
        /// extension off in place; a pointer into the text, or null.
        fn strip_pic_extension(text: Addr) -> Addr;

        // ---- transformations
        /// `SI_waveGetIconModel(wave)` (wave_sys.c): the model that stands for a wave.
        fn wave_get_icon_model(wave: Addr) -> Addr;
        /// `SI_invKinResetPrefAngle(model, axis, x, y, z)` (invkin.c).
        fn inv_kin_reset_pref_angle(model: Addr, axis: i32, x: f32, y: f32, z: f32);
        /// `SI_invKinSetGoal(model, x, y, z)` (invkin.c).
        fn inv_kin_set_goal(model: Addr, x: f32, y: f32, z: f32);
        /// `SI_modelSolveInvKin(model, x, y, z, time)` (invkin.c).
        fn model_solve_inv_kin(model: Addr, x: f32, y: f32, z: f32, time: f32);
        /// `SI_edIsItemConstrained(dict, object, chapter, relation, flag, time)` (edtools.c).
        fn ed_is_item_constrained(dict: Addr, obj: Addr, chap: i32, relid: i32, flag: i32, time: f32) -> i16;
        /// `SI_modelUpdateSonState(father, model, with sons)` (model.c): recomputes the
        /// node's matrices from its local transformation and its father's.
        fn model_update_son_state(father: Addr, model: Addr, sons: i32);
        /// `SI_dynamicSimulationCompute(model, time, dt)` (dynadial.c).
        fn dynamic_simulation_compute(model: Addr, time: f32, dt: f32);
        /// `SI_camMat(camera, matrix)` (sys_camera.c): the matrix goes to a local.
        fn cam_mat(camera: Addr, matrix: Addr);
        /// `SI_edGetCamFromCamInt(element)` (edtools.c): the camera of an interest.
        fn ed_get_cam_from_cam_int(elemid: i32) -> i32;
        /// `SI_edGetLigFromLigInt(element)` (edtools.c).
        fn ed_get_lig_from_lig_int(elemid: i32) -> i32;
        /// `SI_dbChapter2GrafType(chapter, element)` (edtools.c): the graf node kind of an
        /// element (2, 3 model; 4 camera, 5 its interest; 6 light, 7 its interest …).
        fn db_chapter2graf_type(chap: i32, elemid: i32) -> i32;
    }
}

/// Everything `motionUpdate.c` needs from outside: the host traits together.
pub trait Host: constr::ConstrHost + shape::ShapeHost + crate::topo::TopoHost {}
impl<T: constr::ConstrHost + shape::ShapeHost + crate::topo::TopoHost + ?Sized> Host for T {}

// ---- node accessors

/// Number of arguments of a node (16 bits at +0x18).
pub fn nb_args(m: &Mem, node: Addr) -> i32 {
    m.i16(node + updnode::NB_ARGS) as i32
}

/// Argument `i` of a node.
pub fn arg(m: &Mem, node: Addr, i: i32) -> Addr {
    m.ptr(node + updnode::ARGS).wrapping_add((i as u32).wrapping_mul(updelem::SIZE))
}

/// The curve of argument `i` when the argument is active and has an object, else null:
/// the test nearly every routine of this file opens its loop with.
pub fn arg_curve(m: &Mem, node: Addr, i: i32) -> Addr {
    let a = arg(m, node, i);
    if m.i16(a + updelem::ACTIVE) != 0 {
        m.ptr(a + updelem::DATA)
    } else {
        0
    }
}

pub fn relid(m: &Mem, node: Addr) -> i32 {
    m.i32(node + updnode::RELID)
}

pub fn data(m: &Mem, node: Addr) -> Addr {
    m.ptr(node + updnode::DATA)
}

/// The node's element in the current dictionary (`SI_elemdictGetElem` of its chapter and
/// index), null if deleted.
pub fn node_elem(m: &Mem, node: Addr) -> Addr {
    let d = glob::cur_dict(m);
    ed::get_elem(m, d, m.i16(node + updnode::CHAPID) as i32, m.i32(node + updnode::ELEMID))
}

/// The inlined `SI_secToFrame` (finding 0009): `(int)(time × rate + h)`, h = 0.5 for a
/// time above zero and −0.5 otherwise, truncated.
pub fn sec_to_frame(m: &Mem, time: f32) -> i32 {
    let h = if F80::from_f32(time).gt(F80::ZERO) { 0.5f64 } else { -0.5f64 };
    // the half is loaded as a float from the double constant; ±0.5 is exact either way
    (F80::from_f32(time) * glob::frame_rate(m) + (h as f32)).to_i32(Round::Zero)
}

/// `1.0 / SI_frameRate` as the original passes it: a `float` argument.
pub fn frame_duration(m: &Mem) -> f32 {
    (F80::ONE / glob::frame_rate(m)).to_f32()
}

// ---- the statics

/// [soft 0x08221760 si_setRefreshObj]: whether `SI_sceneUpdate` updates the scene at all.
pub fn set_refresh_obj(m: &mut Mem, v: i32) {
    m.set_i32(glob::REFRESH_OBJ, v);
}

/// [soft 0x08221770 si_getRefreshObj]
pub fn get_refresh_obj(m: &Mem) -> i32 {
    m.i32(glob::REFRESH_OBJ)
}

/// [soft 0x0822b074 si_updListSetCurrent]
pub fn upd_list_set_current(m: &mut Mem, list: Addr) {
    m.set_ptr(glob::CUR_UPDATE_LIST, list);
}

/// [soft 0x0822b084 SI_updListGetCurrent]
pub fn upd_list_get_current(m: &Mem) -> Addr {
    m.ptr(glob::CUR_UPDATE_LIST)
}

/// True when the node's element is on screen and selected as a node (1) or as a branch (3)
/// [soft 0x0822177c animElemSelected]. The element array is indexed directly (no range or
/// deletion test); an element without a graf node is not selected.
pub fn anim_elem_selected(m: &Mem, dict: Addr, node: Addr) -> bool {
    let l = m.ptr(dict + dict::LISTS).wrapping_add((m.i16(node + updnode::CHAPID) as u32).wrapping_mul(list::SIZE));
    let e = m.ptr(l + list::ELEMS).wrapping_add((m.i32(node + updnode::ELEMID) as u32).wrapping_mul(elem::SIZE));
    let gn = m.ptr(e + elem::AUX_DATA);
    if gn == 0 {
        return false;
    }
    let sel = m.i32(gn + 8);
    sel == 1 || sel == 3
}

// ---- drivers

/// The message of [`scene_update_topolist`], at its address in the binary's data.
pub const MSG_CYCLE: Addr = 0x0869_76c0;

/// Sorts the whole scene into the dictionary's update list
/// [soft 0x082220c0 SI_sceneUpdateTopolist]. A cycle is reported on the status line
/// ("A cycle has been detected in scene dependencies …", kind 3) and the list is used as
/// far as it got. In the interactive program, holding the keys 6 and 5 of the IRIS GL
/// device table dumps the list.
pub fn scene_update_topolist<H: Host + ?Sized>(m: &mut Mem, h: &mut H) {
    let d = glob::cur_dict(m);
    crate::topo::rel_topo_sort_init(m, d, true);
    let d = glob::cur_dict(m);
    if !crate::topo::elemdict_topo_sort(m, h, glob::SI_RD, d, d + dict::UPD_LIST, false, false) {
        h.status_display_message(m, MSG_CYCLE, 3);
    }
    if m.i32(glob::SI_STANDALONE_RENDERER) == 0 && h.getbutton(m, 6) != 0 && h.getbutton(m, 5) != 0 {
        let d = glob::cur_dict(m);
        h.updlist_dump(m, d, glob::SI_RD, d + dict::UPD_LIST, 1);
    }
}

/// Updates the whole scene to `time` [soft 0x082221a0 SI_sceneUpdateItems]: sort, then run
/// the scene's update list. `camera` is non-null when a renderer asks (the static
/// `fromRendering` records it for the depth-fading and fog updates); with `copy_camera`
/// the current camera (0x13c bytes at `SI_procData[2]`) is copied to it afterwards. The
/// second argument is not used.
pub fn scene_update_items<H: Host + ?Sized>(m: &mut Mem, h: &mut H, time: f32, _unused: i16, camera: Addr,
                                                  copy_camera: i16) {
    m.set_u16(glob::FROM_RENDERING, (camera != 0) as u16);
    scene_update_topolist(m, h);
    let optim = glob::SI_GLB_ANIM + anim::ENVELOP_OPTIM;
    if m.i16(optim) == 0 {
        m.set_i16(glob::SI_GLB_ANIM + anim::IN_UPDATE, 1);
    }
    let list = glob::cur_dict(m) + dict::UPD_LIST;
    upd_list_update(m, h, list, time);
    if m.i16(optim) == 0 {
        m.set_i16(glob::SI_GLB_ANIM + anim::IN_UPDATE, 0);
    }
    if camera != 0 && copy_camera != 0 {
        let src = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
        for w in 0..0x4f {
            let v = m.u32(src + 4 * w);
            m.set_u32(camera + 4 * w, v);
        }
    }
}

/// Updates one element [soft 0x08222238 SI_itemUpdate]: the dictionary's second update
/// list is marked dirty, rebuilt for the element (all its relation lists: everything the
/// element is made from, then the element's own steps) and run. What depends on the
/// element is not updated. Always answers 1.
pub fn item_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, chap: i32, elemid: i32, time: f32) -> i16 {
    let d = glob::cur_dict(m);
    m.set_i32(d + dict::TOPO_LIST + updlist::DIRTY, 1);
    crate::topo::rel_topo_sort_init(m, d, true);
    let d = glob::cur_dict(m);
    crate::topo::elemdict_topo_sort_element(m, h, glob::SI_RD, d, chap, elemid, -1, false);
    let list = glob::cur_dict(m) + dict::TOPO_LIST;
    upd_list_update(m, h, list, time);
    1
}

/// [soft 0x082222dc SI_modelUpdate]: [`item_update`] for a model node. The graf node
/// argument is not used.
pub fn model_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, time: f32, _gn: Addr) -> i32 {
    let (_, id) = ed::get_elem_from_model(m, h, mdl);
    let models = glob::chap(m, cid::MODELS);
    item_update(m, h, models, id, time) as i32
}

/// After the nodes [soft 0x0822230c SI_sceneUpdateDynamics]: every live model whose chain
/// record (+0x304) has its word at +0x5e set gets its dynamic chain's geometry updated;
/// then, with `kine` set, every live model that has a kinematic state record (+0x2f0) gets
/// it recomputed for a step of one frame.
pub fn scene_update_dynamics<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, kine: i16) {
    let models = glob::chap(m, cid::MODELS);
    let l = m.ptr(glob::cur_dict(m) + dict::LISTS).wrapping_add((models as u32).wrapping_mul(list::SIZE));
    let mut i = 0;
    while i < m.i32(l + list::NB_ELEMS) {
        let e = m.ptr(l + list::ELEMS) + i as u32 * elem::SIZE;
        if m.i16(e + elem::STATUS) == 0 {
            let mdl = m.ptr(e + elem::DATA);
            if mdl != 0 && m.ptr(mdl + model::DYNA_CHN_VARS) != 0 && m.i16(m.ptr(mdl + model::DYNA_CHN_VARS) + 0x5e) != 0 {
                h.dynachn_geometry_update(m, mdl);
            }
        }
        i += 1;
    }
    if kine != 0 {
        let mut i = 0;
        while i < m.i32(l + list::NB_ELEMS) {
            let e = m.ptr(l + list::ELEMS) + i as u32 * elem::SIZE;
            if m.i16(e + elem::STATUS) == 0 {
                let mdl = m.ptr(e + elem::DATA);
                if mdl != 0 && m.ptr(mdl + model::KINE_INFO) != 0 {
                    let dt = frame_duration(m);
                    h.model_kine_state_compute(m, mdl, dt);
                }
            }
            i += 1;
        }
    }
}

/// [soft 0x082223ec SI_updnKineStateUpdate]: the kinematic state of a model node's object,
/// unless the interaction mode (`SI_glbAnim` + 0x10) is 5.
pub fn updn_kine_state_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr) {
    if m.i16(node + updnode::CHAPID) as i32 == glob::chap(m, cid::MODELS)
       && m.i32(glob::SI_GLB_ANIM + anim::MODE) != 5
       && m.ptr(data(m, node) + model::KINE_INFO) != 0
    {
        let dt = frame_duration(m);
        let mdl = data(m, node);
        h.model_kine_state_compute(m, mdl, dt);
    }
}

/// [soft 0x08222438 SI_sceneFrameUpdate]: [`scene_update`] at the time of a frame, the
/// frame number converted to `float` and divided by the rate.
pub fn scene_frame_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, frame: i32) {
    let t = (F80::from_f32(frame as f32) / glob::frame_rate(m)).to_f32();
    scene_update(m, h, t);
}

/// The update at a time, as the interactive program asks for it
/// [soft 0x082224b0 SI_sceneUpdate]: unless this is a batch render the frame slider is
/// drawn at the frame nearest to the time; then, if scene refresh is on (`refreshObj`), the
/// whole scene is updated.
pub fn scene_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, time: f32) {
    if m.i32(glob::SI_STANDALONE_RENDERER) == 0 {
        let frame = sec_to_frame(m, time);
        h.fslider_draw(m, frame);
    }
    if m.i32(glob::REFRESH_OBJ) != 0 {
        let flag = (m.i32(glob::SI_UPDATE_OPTION) != 1) as i16;
        scene_update_items(m, h, time, flag, 0, 0);
    }
}

/// Runs an update list at a time [soft 0x0822b090 SI_updListUpdate].
///
/// 1. With the envelope optimisation on (`SI_glbAnim` + 0x38) the "in update" word is set.
/// 2. The list becomes the current one.
/// 3. Unless custom effects are off (`SI_glbAnim` + 0x2c), `SI_cusPreSceneTransfer`.
/// 4. Every node in list order ([`upd_node_update`]). One word of state travels along the
///    nodes (the "post info": what the last deformation left to be finished, see [`geom`]).
/// 5. [`scene_update_dynamics`] with the dynamics switch (`SI_glbAnim` + 0x08).
/// 6. Unless custom effects are off, `SI_cusSceneUpdate(time)`.
/// 7. With `SI_glbAnim` + 0x02 set, the keys are coloured (`SI_sceneKeySetColor(time, 1, 0)`).
/// 8. With the envelope optimisation on, the skeleton flags are reset and "in update" cleared.
pub fn upd_list_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, list: Addr, time: f32) {
    let optim = glob::SI_GLB_ANIM + anim::ENVELOP_OPTIM;
    let post = m.local(4);
    m.set_i32(post, 0);
    if m.i16(optim) != 0 {
        m.set_i16(glob::SI_GLB_ANIM + anim::IN_UPDATE, 1);
    }
    upd_list_set_current(m, list);
    if m.i32(glob::SI_GLB_ANIM + anim::NO_CUSTOM) == 0 {
        h.cus_pre_scene_transfer(m);
    }
    if list != 0 {
        let mut i = 0;
        while i < m.i32(list + updlist::NB) {
            let node = m.ptr(list + updlist::NODES) + i as u32 * updnode::SIZE;
            upd_node_update(m, h, node, time, post);
            i += 1;
        }
    }
    let dynamics = m.i16(glob::SI_GLB_ANIM + anim::DYNAMICS);
    scene_update_dynamics(m, h, dynamics);
    if m.i32(glob::SI_GLB_ANIM + anim::NO_CUSTOM) == 0 {
        h.cus_scene_update(m, time);
    }
    if m.i16(glob::SI_GLB_ANIM + anim::KEY_COLOR) != 0 {
        h.scene_key_set_color(m, time, 1, 0);
    }
    if m.i16(optim) != 0 {
        let d = glob::cur_dict(m);
        h.env_skel_reset_flag(m, d);
        m.set_i16(glob::SI_GLB_ANIM + anim::IN_UPDATE, 0);
    }
    m.release(4);
}

/// One node [soft 0x0822afb8 si_updNodeUpdate]: if the gate lets it through
/// ([`check_active_time`], which may also move the time back), the update by the node's
/// relation list: 0 parameters, 1 transformation, 2 geometry, 3 rendering (nothing), 5
/// only expressions. List 4 (custom) is not updated here.
pub fn upd_node_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, post: Addr) {
    let Some(t) = check_active_time(m, h, node, time) else { return };
    match m.i16(node + updnode::GROUP) {
        0 => upd_node_parameter_update(m, h, node, t),
        1 => upd_node_transform_update(m, h, node, t),
        2 => geom::upd_node_geometry_update(m, h, node, t, post),
        3 => upd_node_render_update(node, t),
        5 => {
            if relid(m, node) == rel::EXP {
                h.anim_expression_evaluate(m, node, t);
            }
        }
        _ => {}
    }
}

/// [soft 0x0822aa5c updNodeRenderUpdate]: nothing.
pub fn upd_node_render_update(_node: Addr, _time: f32) {}

/// When a relation's activation curve says "off" at `time`, the time the relation was
/// switched off, and whether it counts as on [soft 0x0822aa64 SI_activationFcvCheckTime].
/// Returns (time to evaluate at, value above 0.0001).
///
/// - `time` not after the first key: the first key's time and value.
/// - `time` at or after the last key: the last key's.
/// - otherwise the key before the first key later than `time`; and if that key's value is
///   below 1, the search walks back over the keys that hold the same value while it stays
///   below 1 (to the start of the "off" stretch).
///
/// The fourth argument of the original is not used. The key array is not bounded by the
/// count in the forward search (it relies on the last key being later than `time`, which
/// the second test has established); with a NaN time the first two tests fail and the
/// search stops on key 1 at once.
pub fn activation_fcv_check_time(m: &Mem, curve: Addr, time: f32) -> (f32, bool) {
    let keys = m.ptr(curve + 8);
    let t = F80::from_f32(time);
    let kt = |i: i32| m.f32(keys.wrapping_add((i as u32).wrapping_mul(0x24)));
    let kv = |i: i32| m.f32(keys.wrapping_add((i as u32).wrapping_mul(0x24)) + 4);
    let (out, v);
    // key 0 at or after the time (neither "less" nor unordered)
    if !(F80::from_f32(kt(0)).lt(t) || F80::from_f32(kt(0)).unordered(t)) {
        out = kt(0);
        v = kv(0);
    } else {
        let n = m.i32(curve + 0xc);
        let last = F80::from_f32(kt(n - 1));
        if last.lt(t) || last.eq(t) {
            out = kt(n - 1);
            v = kv(n - 1);
        } else {
            // the first key later than the time
            let mut i = 1;
            while !(t.lt(F80::from_f32(kt(i))) || t.unordered(F80::from_f32(kt(i)))) {
                i += 1;
                if !m.in_block(keys.wrapping_add((i as u32).wrapping_mul(0x24)), 4) {
                    // the original reads on past the array; here the search stops
                    break;
                }
            }
            if F80::ONE.gt(F80::from_f32(kv(i - 1))) && i > 1 {
                loop {
                    if !F80::from_f32(kv(i - 1)).eq(F80::from_f32(kv(i - 2))) {
                        break;
                    }
                    i -= 1;
                    if !F80::ONE.gt(F80::from_f32(kv(i - 1))) || i <= 1 {
                        break;
                    }
                }
            }
            out = kt(i - 1);
            v = kv(i - 1);
        }
    }
    (si_x87::fmov(out), F80::from_f32(v).gt(F80::from_f64(0.0001)))
}

/// The gate in front of every node [soft 0x0822ab70 SI_checkActiveTime]: `None` when the
/// node is not to be updated, else the time to update it at.
///
/// 1. With the dope sheet's word at 0x0878faa8 set, an element the dope sheet calls
///    inactive is skipped.
/// 2. The element's graf node is looked up (the element's, else by a search for the
///    object). With `SI_updateOption` 1 (selected only) a node whose graf node is neither
///    selected nor flagged (bits 0xe0 of its byte +0x17) is skipped; with 2 a hidden one
///    (graf node +0xc zero) is skipped unless the node is a visibility parameter (models
///    10 and 12, lights 2100, cameras 1100) or an expression on a visibility curve (175,
///    273, 176, 177). For cameras and spot lights the node after the element's is taken,
///    except for the interest's relations.
/// 3. A hidden model, camera or light (other than through its visibility parameters or an
///    expression) is skipped when nothing else uses the element; for models only plain
///    ones are tested (not instances' masters, not nulls with a chain algorithm, not
///    joints, not envelopes of kinds 4, 9, 10, 11).
/// 4. An inactive node (+0x20 zero) is skipped.
/// 5. For the relations that have activation curves and are not handled by their own
///    routines (not the constraints 102, 103, 111, 114–118, 126, 1111, 1116, 2111, 2116 and
///    not expressions): each argument whose activation curve is "off" at `time` moves the
///    time back to when it went off ([`activation_fcv_check_time`]; the earliest wins);
///    the node is skipped when all its arguments are inactive, or at least as many as the
///    relation dictionary allows to be (`SI_reldictNbRelForDeactCount`).
pub fn check_active_time<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) -> Option<f32> {
    let chap = m.i16(node + updnode::CHAPID) as i32;
    let models = glob::chap(m, cid::MODELS);
    let cameras = glob::chap(m, cid::CAMERAS);
    let lights = glob::chap(m, cid::LIGHTS);
    if m.i16(glob::DOPE_ACTIVE_CHECK) != 0 && h.dope_is_elem_active(m, chap, m.i32(node + updnode::ELEMID)) == 0 {
        return None;
    }
    let e = node_elem(m, node);
    let mut gn = if e != 0 { m.ptr(e + elem::AUX_DATA) } else { 0 };
    if gn == 0 {
        let gl = glob::cur_graf_list(m);
        let obj = data(m, node);
        gn = h.find_gl_data(m, gl, obj, 0);
    }
    let opt = m.i32(glob::SI_UPDATE_OPTION);
    let mut gated = true;
    if opt != 0 {
        let id = relid(m, node);
        let obj = data(m, node);
        if (chap == cameras && id != 0x45a && id != 0x45b && id != 0x45c)
           || (chap == lights && obj != 0 && m.i16(obj + 0x1e) == 2 && id != 0x842 && id != 0x843 && id != 0x844)
        {
            gn = m.ptr(gn.wrapping_add(0x1c));
        }
        if gn == 0 {
            gated = false;
        } else if (opt == 1 && m.i32(gn + 8) == 0 && m.u8(gn + 0x17) & 0xe0 == 0) || (opt == 2 && m.i32(gn + 0xc) == 0) {
            if opt != 2 {
                return None;
            }
            if id == rel::EXP {
                let rd = m.ptr(node + updnode::REL_DATA);
                if rd == 0 {
                    return None;
                }
                let param = m.i32(rd);
                let ok = if chap == models {
                    param == 0xaf || param == 0x111
                } else if chap == lights {
                    param == 0xb0
                } else {
                    chap == cameras && param == 0xb1
                };
                if !ok {
                    return None;
                }
            } else {
                let ok = if chap == models {
                    id == 0xc || id == 10
                } else if chap == lights {
                    id == 0x834
                } else {
                    chap == cameras && id == 0x44c
                };
                if !ok {
                    return None;
                }
            }
        }
    }
    if gated && gn != 0 && m.i32(gn + 0xc) == 0 && (chap == lights || chap == cameras || chap == models) {
        let id = relid(m, node);
        if id != rel::EXP && id != 10 && id != 0xc && id != 0x44c && id != 0x834 {
            let mut plain = false;
            let mut flag = false;
            if chap == models {
                let mdl = data(m, node);
                let kind = m.i32(mdl + model::KIND);
                let algo = m.i32(mdl + model::ALGO_TYPE);
                if m.i16(mdl + model::MASTER_FLAG) == 0
                   && (kind != 0 || (algo.wrapping_sub(1) as u32) > 1)
                   && kind != 5
                   && (kind != 0 || (algo.wrapping_sub(3) as u32) > 1)
                   && (mdl == 0 || m.ptr(mdl + model::ENV_SKEL_JNT) == 0 || {
                       let env = m.i32(mdl + model::ENVELOP);
                       env != 9 && env != 4 && env != 10 && env != 0xb
                   })
                {
                    plain = true;
                }
                flag = id == 0xcf;
            }
            if plain && h.elemdict_is_elem_used(m, e, flag as i16) == 0 {
                return None;
            }
        }
    }
    // the time goes out through the unit: a signalling NaN comes out quiet
    let out = si_x87::fmov(time);
    if m.i16(node + updnode::ACTIVE) == 0 {
        return None;
    }
    let id = relid(m, node);
    if id != 0x7e {
        if matches!(id, 0x6f | 0x66 | 0x67 | 0x72..=0x76 | 0x83f | 0x457 | 0x45c | 0x844 | 0x4e2a) {
            return Some(out);
        }
        let mut earliest = out;
        let mut inactive = 0;
        let mut i = 0;
        while i < nb_args(m, node) {
            let a = arg(m, node, i);
            if m.i16(a + updelem::ACTIVE) == 0 {
                inactive += 1;
            }
            let act = m.ptr(a + updelem::ACT_FCV);
            if act != 0 && fcv::integer_value(m, act, time, 0, 1) < 1 {
                let (t, _) = activation_fcv_check_time(m, act, time);
                if F80::from_f32(t).lt(F80::from_f32(earliest)) {
                    earliest = t;
                }
            }
            i += 1;
        }
        if inactive > 0 {
            if inactive == nb_args(m, node) {
                return None;
            }
            let n = nb_args(m, node);
            if h.reldict_nb_rel_for_deact_count(m, glob::SI_RD, id, n) <= inactive {
                return None;
            }
        }
        return Some(earliest);
    }
    Some(out)
}

/// Updates of relation list 0, by relation id [soft 0x08229e38 updNodeParameterUpdate]:
///
/// | Relation ids | Routine |
/// |---|---|
/// | −400 | a 2D texture's script or picture sequence: if it is not up to date for the frame, bits 0 and 2 of the texture's byte +0x194 are set |
/// | 10, 12, 15, 16, 50–53, 4000–4003 | [`params::anim_model_parameters_update`] |
/// | 11 | [`params::anim_algo_type_update`] |
/// | 301–312 | [`params::anim_mat_parameters_update`] |
/// | 411–426 | [`params::anim_tex_parameters_update`] |
/// | 511–532 | [`params::anim_tex3d_parameters_update`] |
/// | 614, 615 | [`params::anim_wave_parameters_update`] |
/// | 1100, 1131–1144 | [`params::anim_camera_parameters_update`] |
/// | 2100, 2130–2136, 2201–2208 | [`params::anim_light_parameters_update`] |
/// | 3000–3003 | depth fading, 3030–3035 fog: the node's object is replaced by the scene's render control `SI_defaultControl` first |
/// | 20000 | the custom effect; 20010 the expression |
///
/// Everything else is ignored, among them 4100 (`WFDisplay`) and the ids between the
/// ranges: a curve on such a parameter exists and is saved, but the update does not
/// apply it.
pub fn upd_node_parameter_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let id = relid(m, node);
    match id {
        -400 => {
            let tex = data(m, node);
            if tex == 0 {
                return;
            }
            if m.i16(tex + 0x28) == 2 {
                let frame = sec_to_frame(m, time);
                if texfile::is_script_up_to_date(m, h, tex, frame) as i16 == 0 {
                    m.or_u8(tex + 0x194, 5);
                    return;
                }
            }
            if m.i16(tex + 0x28) != 1 {
                return;
            }
            let frame = sec_to_frame(m, time);
            if texfile::is_sequence_up_to_date(m, h, tex, frame) as i16 != 0 {
                return;
            }
            m.or_u8(tex + 0x194, 5);
        }
        10 | 12 | 15 | 16 | 0x32..=0x35 | 4000..=4003 => params::anim_model_parameters_update(m, h, node, time),
        11 => params::anim_algo_type_update(m, h, node, time),
        0x12d..=0x138 => params::anim_mat_parameters_update(m, node, time),
        0x19b..=0x1aa => params::anim_tex_parameters_update(m, h, node, time),
        0x1ff..=0x214 => params::anim_tex3d_parameters_update(m, h, node, time),
        0x266 | 0x267 => params::anim_wave_parameters_update(m, node, time),
        0x44c | 0x46b..=0x478 => params::anim_camera_parameters_update(m, node, time),
        0x834 | 0x852..=0x858 | 0x899..=0x8a0 => params::anim_light_parameters_update(m, h, node, time),
        3000..=0xbbb => {
            m.set_ptr(node + updnode::DATA, glob::SI_DEFAULT_CONTROL);
            params::anim_fad_parameters_update(m, node, time);
        }
        0xbd6..=0xbdb => {
            m.set_ptr(node + updnode::DATA, glob::SI_DEFAULT_CONTROL);
            params::anim_fog_parameters_update(m, node, time);
        }
        20000 => h.cus_update_effect(m, node, time),
        0x4e2a => h.anim_expression_evaluate(m, node, time),
        _ => {}
    }
}

/// Updates of relation list 1, by relation id [soft 0x0822a1a8 updNodeTransformUpdate]:
///
/// | Relation ids | Routine |
/// |---|---|
/// | −100, −101 | [`transfo::anim_transfo_update`]: the node's matrices from its local transformation |
/// | 100, 610 | [`transfo::anim_scaling_update`] |
/// | 101, 611 | [`transfo::anim_rotation_update`] |
/// | 113, 613 | [`transfo::anim_explicit_pos_update`] |
/// | 112, 612, 1112, 1114, 2112, 2114 | the path update (`animAllTypePathUpdate`) |
/// | 102, 103, 111, 114, 115, 116, 124, 125, 128, 129, 238, 806 | [`constr::anim_model_constraint_update`] with index −1 |
/// | 117 | [`constr::anim_model_plane_constr_update`]; 118 the same, but only while interacting (`SI_glbAnim` + 0x10 or + 0x14 set) with the element selected |
/// | 119 | [`constr::anim_cns_3pts_update`]; 127 [`constr::anim_cns_2pts_update`]; 126 [`constr::anim_model_obj_pnt_cns_update`] |
/// | 120 | inverse kinematics, then the algorithm type |
/// | 150 | [`constr::anim_model_sym_constr_update`]; 160 [`constr::anim_trajectory_update`]; 206 rigid envelope; 800 dynamics |
/// | 1111, 1116 | [`params::anim_camera_constraint_update`]; 1113, 1115 [`params::anim_camera_explicit_pos_update`]; 1117 the camera's matrix is recomputed (`SI_camMat`); 1130 roll |
/// | 2111, 2116 | [`params::anim_light_constraint_update`]; 2113, 2115 [`params::anim_light_explicit_pos_update`]; 2120 sun time |
/// | 20000 | the custom effect; 20010 the expression |
pub fn upd_node_transform_update<H: Host + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let id = relid(m, node);
    match id {
        0xce => geom::anim_envelop_update(m, h, node),
        -101 | -100 => transfo::anim_transfo_update(m, h, node, time),
        100 | 0x262 => transfo::anim_scaling_update(m, h, node, time),
        0x65 | 0x263 => transfo::anim_rotation_update(m, h, node, time),
        0x71 | 0x265 => transfo::anim_explicit_pos_update(m, h, node, time),
        0x70 | 0x264 | 0x458 | 0x45a | 0x840 | 0x842 => transfo::anim_all_type_path_update(m, h, node, time),
        0x66 | 0x67 | 0x6f | 0x72..=0x74 | 0x7c | 0x7d | 0x80 | 0x81 | 0xee | 0x326 => {
            constr::anim_model_constraint_update(m, h, node, time, -1)
        }
        0x75 => constr::anim_model_plane_constr_update(m, h, node, time, -1, 0),
        0x76 => {
            if m.i32(glob::SI_GLB_ANIM + anim::MODE) == 0 && m.i32(glob::SI_GLB_ANIM + anim::MODE2) == 0 {
                return;
            }
            let d = glob::cur_dict(m);
            if !anim_elem_selected(m, d, node) {
                return;
            }
            constr::anim_model_plane_constr_update(m, h, node, time, -1, 0);
        }
        0x77 => constr::anim_cns_3pts_update(m, h, node, time),
        0x78 => {
            params::anim_inv_kin_update(m, h, node, time);
            params::anim_algo_type_update(m, h, node, time);
        }
        0x7e => constr::anim_model_obj_pnt_cns_update(m, h, node, time),
        0x7f => constr::anim_cns_2pts_update(m, h, node, time),
        0x96 => constr::anim_model_sym_constr_update(m, h, node, time, -1),
        0xa0 => constr::anim_trajectory_update(m, h, node, time),
        800 => params::anim_dynamics_update(m, h, node, time),
        0x457 | 0x45c => params::anim_camera_constraint_update(m, h, node, time),
        0x459 | 0x45b => params::anim_camera_explicit_pos_update(m, node, time),
        0x45d => {
            let cam = data(m, node);
            if cam == 0 {
                return;
            }
            let mat = m.local(64);
            h.cam_mat(m, cam, mat);
            m.release(64);
        }
        0x46a => params::anim_camera_parameters_update(m, node, time),
        0x83f | 0x844 => params::anim_light_constraint_update(m, h, node, time),
        0x841 | 0x843 => params::anim_light_explicit_pos_update(m, node, time),
        0x848 => params::anim_light_sun_time_update(m, h, node, time),
        20000 => h.cus_update_effect(m, node, time),
        0x4e2a => h.anim_expression_evaluate(m, node, time),
        _ => {}
    }
}

/// After an interactive change of geometry [soft 0x0822b18c SI_updateGeometryRelation]:
/// the constraints that depend on geometry are collected (`SI_getConstraintUpdateList(2,
/// 0)`) and, if there are any, updated at the current frame's time (`SI_sceneConstrUpdate(5,
/// frame / rate, origin)`); the views are refreshed (`SI_refreshSystem(0, 0, 0x2f)`) when
/// there were such constraints or when a selected model node (graf kinds 2, 3) of the
/// current graf list is the master of instances. Returns whether a refresh was asked for.
pub fn update_geometry_relation<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H) -> bool {
    let constraints = h.get_constraint_update_list(m, 2, 0);
    let mut master = false;
    let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
    while gn != 0 {
        if m.i32(gn + 8) != 0 && (m.i32(gn).wrapping_sub(2) as u32) < 2 {
            let mdl = m.ptr(gn + 4);
            if h.model_is_master(m, mdl) != 0 {
                master = true;
            }
        }
        gn = m.ptr(gn + 0x1c);
    }
    if constraints != 0 {
        let t = (F80::from_f32(m.i32(glob::SI_FRAME_CURRENT) as f32) / glob::frame_rate(m)).to_f32();
        h.scene_constr_update(m, 5, t, glob::SI_ORIGIN);
    }
    if constraints != 0 || master {
        h.refresh_system(m, 0, 0, 0x2f);
        true
    } else {
        false
    }
}

/// The next node of an update list for an object and a relation
/// [soft 0x0822b2a4 SI_edFindUpdateNode]: the search starts at `*cursor` (0 without a
/// cursor) and leaves the cursor behind the node it stopped on. Null when there is none.
pub fn ed_find_update_node(m: &mut Mem, list: Addr, obj: Addr, relation: i32, cursor: Addr) -> Addr {
    if list == 0 {
        return 0;
    }
    let mut i = if cursor != 0 { m.i32(cursor) } else { 0 };
    while i < m.i32(list + updlist::NB) {
        let p = m.ptr(list + updlist::NODES).wrapping_add((i as u32).wrapping_mul(updnode::SIZE));
        if cursor != 0 {
            m.set_i32(cursor, i + 1);
        }
        if m.ptr(p + updnode::DATA) == obj && m.i32(p + updnode::RELID) == relation {
            return p;
        }
        i += 1;
    }
    0
}

/// Test support: update nodes in the image.
pub mod build {
    use super::*;

    /// One argument of a node: (chapter, element index, object, active, activation curve).
    pub type Arg = (i32, i32, Addr, i16, Addr);

    /// Allocates one update node as `si_updlistAddNode` leaves it, followed by a record
    /// whose chapter is −1 (the end mark of the fast hierarchy update); returns its address.
    pub fn node(m: &mut Mem, chap: i32, group: i16, elemid: i32, data: Addr, relid: i32, args: &[Arg]) -> Addr {
        let n = m.si_calloc(2, updnode::SIZE);
        fill(m, n, chap, group, elemid, data, relid, args);
        m.set_i16(n + updnode::SIZE, -1);
        n
    }

    /// Fills the node record at `n`.
    pub fn fill(m: &mut Mem, n: Addr, chap: i32, group: i16, elemid: i32, data: Addr, relid: i32, args: &[Arg]) {
        m.set_i16(n + updnode::CHAPID, chap as i16);
        m.set_i16(n + updnode::GROUP, group);
        m.set_i32(n + updnode::ELEMID, elemid);
        m.set_ptr(n + updnode::DATA, data);
        m.set_i32(n + updnode::RELID, relid);
        m.set_i16(n + updnode::NB_ARGS, args.len() as i16);
        m.set_i16(n + updnode::ACTIVE, 1);
        let a = m.si_calloc(args.len().max(1) as u32, updelem::SIZE);
        m.set_ptr(n + updnode::ARGS, a);
        for (i, &(c, e, d, act, fcv)) in args.iter().enumerate() {
            let at = a + i as u32 * updelem::SIZE;
            m.set_i16(at + updelem::CHAPID, c as i16);
            m.set_i32(at + updelem::ELEMID, e);
            m.set_ptr(at + updelem::DATA, d);
            m.set_i16(at + updelem::ACTIVE, act);
            m.set_ptr(at + updelem::ACT_FCV, fcv);
        }
    }

    /// A node whose arguments are curves of the ANIMATION chapter, all active.
    pub fn curve_node(m: &mut Mem, chap: i32, group: i16, elemid: i32, data: Addr, relid: i32, curves: &[Addr]) -> Addr {
        let ani = glob::chap(m, cid::ANIMATION);
        let args: Vec<Arg> = curves.iter().map(|&c| (ani, 0, c, 1, 0)).collect();
        node(m, chap, group, elemid, data, relid, &args)
    }
}

#[cfg(test)]
mod tests;
