// SPDX-License-Identifier: MIT
//! `rens_udf.c`: the scan of a render dictionary for the mental ray shaders a render uses.
//!
//! A shader is a record (`__SI_UDF`) that is the object of an element of one of the eleven
//! shader chapters; what uses it is related to it by 550 (list 3). The scan marks no shader
//! as used (`SI_udfResetActive`), then walks the lights, the models with their materials and
//! textures, and the scene element, and for every shader record it finds that is switched on
//! (`SI_udfIsActive` answers 1) calls `SI_udfSetAsSelected`. The three shader routines are
//! `udf_fct.c`'s and are asked of [`Outside`]. Every walk moves list cursors, as the
//! original's does.

use crate::outside::Outside;
use crate::world::{rel, Env};
use si_scene::dict::{Dict, ElemId};
use si_scene::elemdict as ed;
use si_types::dict::ELEM_VALID;

/// The words of `SI_RenControl` the scan reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanControl {
    /// +0x25c: when not 0 the atmosphere shaders are not scanned. Meaning not established.
    pub w25c: i16,
    /// +0x100: the atmosphere shaders are scanned when this is not 0 (and +0x25c is 0).
    pub atmosphere: i16,
    /// +0x586: 2D textures are rendered.
    pub textures_2d: i16,
    /// +0x588: 3D textures are rendered.
    pub textures_3d: i16,
    /// +0x58a: reflection is rendered (a material's reflection map counts).
    pub reflection: i16,
}

/// The material's reflection map as a relation: `T2DEnv`, which the resource file calls
/// unused.
const T2D_ENV: i32 = 402;

/// Marks the first shader an element has in a chapter, if it is switched on.
fn first_shader(d: &mut Dict, out: &mut dyn Outside, e: ElemId, chap: i32) {
    let shader = ed::get_first_related_elem(d, e, true, 3, rel::SHADER, chap).data.0;
    if shader != 0 && out.udf_is_active(shader) == 1 {
        out.udf_set_as_selected(shader);
    }
}

/// The whole scan [soft 0x082c7be0 SI_mentalShaderScan]: lights, models (`all`: every
/// model; otherwise the selected ones), the scene's camera and output shaders, and its
/// atmosphere shaders when the controls ask for them.
pub fn mental_shader_scan(d: &mut Dict, env: &Env, out: &mut dyn Outside, ctl: &ScanControl, all: bool) {
    out.udf_reset_active();
    mental_scan_all_lights(d, env, out);
    mental_scan_all_models(d, env, out, ctl, all);
    mental_scan_camera(d, env, out);
    mental_scan_output(d, env, out);
    if ctl.w25c == 0 && ctl.atmosphere != 0 {
        mental_scan_atmosphere(d, env, out);
    }
}

/// [soft 0x082c7c50 si_mentalScanAllModels]: every root model element in use whose node is
/// a null, patch, face, mesh, joint, metaball system, NURBS curve or NURBS surface (that is,
/// not a spline and not a single metaball), with its hierarchy.
pub fn mental_scan_all_models(d: &mut Dict, env: &Env, out: &mut dyn Outside, ctl: &ScanControl, all: bool) {
    let cm = env.p.cid.model;
    let mut i = 0;
    while (i as usize) < d.ed.lists[cm as usize].elems.len() {
        let rec = d.ed.lists[cm as usize].elems[i as usize].rec;
        if rec.status == ELEM_VALID && rec.hier_level == 0 && env.heap.model(rec.data.0).is_some_and(|m| matches!(m.kind, 0 | 1 | 2 | 4 | 5 | 7 | 9 | 10)) {
            mental_scan_model(d, env, out, ctl, ElemId::new(cm, i), all);
        }
        i += 1;
    }
}

/// [soft 0x082c7ce8 si_mentalScanAllLights]: the light shader and the light photon shader
/// of every root light element in use that has a record.
pub fn mental_scan_all_lights(d: &mut Dict, env: &Env, out: &mut dyn Outside) {
    let cl = env.p.cid.lig;
    let mut i = 0;
    while (i as usize) < d.ed.lists[cl as usize].elems.len() {
        let rec = d.ed.lists[cl as usize].elems[i as usize].rec;
        if rec.status == ELEM_VALID && rec.hier_level == 0 && !rec.data.is_null() {
            first_shader(d, out, ElemId::new(cl, i), env.p.cid.shd_lig);
            first_shader(d, out, ElemId::new(cl, i), env.p.cid.shd_lig_photon);
        }
        i += 1;
    }
}

/// [soft 0x082c7df0 si_mentalScanCamera]: the scene's camera shaders.
pub fn mental_scan_camera(d: &mut Dict, env: &Env, out: &mut dyn Outside) {
    mental_scan_multi(d, env, out, env.p.cid.shd_cam);
}

/// [soft 0x082c7e0c si_mentalScanOutput]: the scene's output shaders.
pub fn mental_scan_output(d: &mut Dict, env: &Env, out: &mut dyn Outside) {
    mental_scan_multi(d, env, out, env.p.cid.shd_out);
}

/// [soft 0x082c7e28 si_mentalScanAtmosphere]: the scene's atmosphere shaders, which are
/// elements of VOLUME_SHADERS.
pub fn mental_scan_atmosphere(d: &mut Dict, env: &Env, out: &mut dyn Outside) {
    mental_scan_multi(d, env, out, env.p.cid.shd_vol);
}

/// Every shader of one chapter that the scene element is related to
/// [soft 0x082c7e44 mentalScanMulti]. The walk ends at the first shader element without a
/// record. (Without a scene element the original walks from a null pointer.)
pub fn mental_scan_multi(d: &mut Dict, env: &Env, out: &mut dyn Outside, chap: i32) {
    let Some(scene) = ed::get_elem_from_scene(d, &env.p) else { return };
    let mut r = ed::get_first_related_elem(d, scene, true, 3, rel::SHADER, chap);
    while !r.data.is_null() {
        if out.udf_is_active(r.data.0) == 1 {
            out.udf_set_as_selected(r.data.0);
        }
        r = ed::get_next_related_elem(d, scene, true, 3, rel::SHADER, chap, Some(r.cursor));
    }
}

/// One model element and the elements under it [soft 0x082c7ed0 mentalScanModel]: when the
/// element is selected (selection state 1, 2 or 3) or `all`, its materials, and with the
/// switches on its global 2D textures and its global 3D texture.
pub fn mental_scan_model(d: &mut Dict, env: &Env, out: &mut dyn Outside, ctl: &ScanControl, e: ElemId, all: bool) {
    scan_model(d, env, out, ctl, e, all, 0);
}

fn scan_model(d: &mut Dict, env: &Env, out: &mut dyn Outside, ctl: &ScanControl, e: ElemId, all: bool, depth: usize) {
    if depth > d.ed.lists[e.chap as usize].elems.len() {
        return; // a cycle of parent relations: the original recurses until its stack is gone
    }
    let Some(rec) = d.slot(e).map(|s| s.rec) else { return };
    if matches!(rec.selection, 1..=3) || all {
        mental_scan_material(d, env, out, ctl, e);
        if ctl.textures_2d != 0 {
            mental_scan_texture2d(d, env, out, e, true);
        }
        if ctl.textures_3d != 0 {
            mental_scan_texture3d(d, env, out, e, true);
        }
    }
    let mut son = ed::find_son(d, &env.p, e);
    while let Some(s) = son {
        scan_model(d, env, out, ctl, s, all, depth + 1);
        son = ed::find_next_son(d, &env.p, e);
    }
}

/// The shaders of a model element's materials [soft 0x082c7f98 mentalScanMaterial]: for
/// each material, its first shader in MATERIAL_SHADERS, VOLUME_SHADERS, SHADOW_SHADERS and
/// MATERIAL_PHOTON_SHADERS; with 2D textures on, the two shaders of its reflection map
/// (relation 402) when reflection is on and the map has a record, and its local 2D
/// textures; with 3D textures on, its local 3D texture.
pub fn mental_scan_material(d: &mut Dict, env: &Env, out: &mut dyn Outside, ctl: &ScanControl, e: ElemId) {
    let c = env.p.cid;
    let mut m = ed::get_first_related_elem(d, e, true, 3, rel::MAT, c.mat);
    while let Some(me) = m.elem {
        for chap in [c.shd_mat, c.shd_vol, c.shd_sha, c.shd_mat_photon] {
            first_shader(d, out, me, chap);
        }
        let map = ed::get_first_related_elem(d, me, true, 3, T2D_ENV, c.t2d);
        if ctl.textures_2d != 0 {
            if let (true, false, Some(t)) = (ctl.reflection != 0, map.data.is_null(), map.elem) {
                first_shader(d, out, t, c.shd_t2d);
                first_shader(d, out, t, c.shd_env);
            }
            mental_scan_texture2d(d, env, out, me, false);
        }
        if ctl.textures_3d != 0 {
            mental_scan_texture3d(d, env, out, me, false);
        }
        m = ed::get_next_related_elem(d, e, true, 3, rel::MAT, c.mat, Some(m.cursor));
    }
}

/// The shaders of an element's 2D textures [soft 0x082c8234 mentalScanTexture2D]: the
/// global ones of a model (relation 400) or the local ones of a material (401); of each
/// texture its first shader in TEXTURE2D_SHADERS and in ENVIRON_SHADERS.
pub fn mental_scan_texture2d(d: &mut Dict, env: &Env, out: &mut dyn Outside, e: ElemId, global: bool) {
    let c = env.p.cid;
    let relid = if global { rel::T2D_GLB } else { rel::T2D_LOC };
    let mut t = ed::get_first_related_elem(d, e, true, 3, relid, c.t2d);
    while let Some(te) = t.elem {
        first_shader(d, out, te, c.shd_t2d);
        first_shader(d, out, te, c.shd_env);
        t = ed::get_next_related_elem(d, e, true, 3, relid, c.t2d, Some(t.cursor));
    }
}

/// The shader of an element's 3D texture [soft 0x082c837c mentalScanTexture3D]: the global
/// one of a model (500) or the local one of a material (501).
pub fn mental_scan_texture3d(d: &mut Dict, env: &Env, out: &mut dyn Outside, e: ElemId, global: bool) {
    let c = env.p.cid;
    let relid = if global { rel::T3D_GLB } else { rel::T3D_LOC };
    if let Some(te) = ed::get_first_related_elem(d, e, true, 3, relid, c.t3d).elem {
        first_shader(d, out, te, c.shd_t3d);
    }
}
