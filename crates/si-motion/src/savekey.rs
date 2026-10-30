// SPDX-License-Identifier: MIT
//! Save key for models (`motionKey.c`): one key at the current frame on the curve of a
//! parameter of a model node, for the node or for its whole branch.
//!
//! Everything goes through [`savekey_model`]: it finds the element's curve of a relation and
//! type (or makes it), and adds a key with the value given, or, for the translation of a
//! node that follows a path (curve type 7), a **point on the path** with a key on the
//! percentage curve (finding 0017, section 9a). The commands above it pick the values.

use crate::mem::MemExt;
use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::model;
use crate::path::{self, PathHost};
use crate::utils::UtilsHost;
use si_math::algebra;
use si_x87::{fmov, x};

/// The double 57.29577951308232 at 0x0864d8a0 and its copies 0x0864d8a8, 0x0864d8b0:
/// radians to degrees.
pub const RAD2DEG: f64 = 57.29577951308232;

/// The texts of these routines, at their addresses in the binary's data.
pub mod msg {
    use si_mem::Addr;
    /// "Selected item's path is animated.", "", "Cannot add a point to the path.", "Ok".
    pub const PATH_ANIMATED: [Addr; 4] = [0x0869_6e20, 0x0869_6e00, 0x0869_6de0, 0x0869_6dc0];
    /// "Cannot save translation of a path in branch mode."
    pub const BRANCH_PATH: Addr = 0x0869_6e60;
    /// "Selected model cannot have a translation path." (two copies)
    pub const NO_PATH_A: Addr = 0x0869_6ea0;
    pub const NO_PATH_B: Addr = 0x0869_6f20;
    /// "Selected model already has explicit translation."
    pub const HAS_EXPLICIT: Addr = 0x0869_6ee0;
    /// "Selected model already has a path translation fcurve."
    pub const HAS_PATH: Addr = 0x0869_6f60;
    /// "Invalid selection for shape animation"
    pub const SHAPE_KIND: Addr = 0x0869_6fa0;
}

crate::host_trait! {
    /// What the model save-key commands call outside this package.
    pub trait SaveKeyHost: PathHost, UtilsHost {
        /// `SI_shapeAddKey(model, 1, 0)` (shape.c): the present geometry becomes a shape key.
        fn shape_add_key(mdl: Addr, a: i32, b: i32);
        /// `SI_modelCreateParent(father's graf node, son's graf node, 0)` (mod_global.c).
        fn model_create_parent(father: Addr, son: Addr, flag: i32);
        /// `SI_savekeyCompensateInfo(model, curve)` (classic.c): under the classical
        /// transformation method.
        fn savekey_compensate_info(mdl: Addr, curve: Addr);
        /// `SI_modelNextNode(model)` (model_b.c): the next node in hierarchy order.
        fn model_next_node(mdl: Addr) -> Addr;
        /// `SI_modelIsFather(ancestor, model)` (model.c).
        fn model_is_father(ancestor: Addr, mdl: Addr) -> i16;
        /// `SI_splineIsPath(model)`: whether something follows the spline as a path.
        fn spline_is_path(mdl: Addr) -> i16;
    }
}

fn raise(m: &mut Mem, mdl: Addr, bit: u8) {
    m.or_u8(mdl + model::UPDATE_FLAGS, bit);
    m.or_u8(mdl + model::UPDATE_FLAGS2, bit);
}

/// Whether a curve's type lies in a range, both ends included
/// [soft 0x0810f6e4 SI_fcvCheckTypeRange] (`dbvsfcv.c`).
pub fn fcv_check_type_range(m: &Mem, curve: Addr, lo: i32, hi: i32) -> bool {
    let k = fcv::kind(m, curve);
    lo <= k && k <= hi
}

/// Whether a point may be added to a path model [soft 0x08219904 savekeyValidTranslation]:
/// yes without a path; no, after the dialogue "Selected item's path is animated. / Cannot
/// add a point to the path.", for a path with shapes (more than one definition) or with an
/// envelope of kind 5 or 6.
pub fn savekey_valid_translation<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, path: Addr) -> bool {
    if path == 0 {
        return true;
    }
    let env = m.i32(path + model::ENVELOP);
    if m.u16(path + model::NB_DEFS) <= 1 && env != 5 && env != 6 {
        return true;
    }
    h.dialog_request(m, msg::PATH_ANIMATED[0], msg::PATH_ANIMATED[1], msg::PATH_ANIMATED[2], msg::PATH_ANIMATED[3]);
    false
}

/// A world position in a path model's own space [soft 0x08219950 savekeyMapTranslation]:
/// the point through the inverse of the path's global matrix, each component ((y·m₁ⱼ +
/// x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ in the register. A matrix without inverse leaves the original with
/// an uninitialised one; here zeros.
pub fn savekey_map_translation(m: &mut Mem, path: Addr, v: Addr) {
    let inv = algebra::matrix_inverse(&m.matrix(path + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
    let p = [m.f32(v), m.f32(v + 4), m.f32(v + 8)];
    for j in 0..3 {
        let r = ((x(p[1]) * inv[1][j] + x(p[0]) * inv[0][j]) + x(p[2]) * inv[2][j]) + inv[3][j];
        m.set_f32(v + 4 * j as u32, r.to_f32());
    }
}

/// One key on one curve of a model [soft 0x08219fe0 SI_savekeyModel].
///
/// `group` and `relid` name the relation, `kind` the curve type; with `check` the element's
/// curves of the relation are searched for one of that type (without it the first curve of
/// the relation is taken); a missing curve is made (`SI_fcurveAllocate(kind, model)`) and
/// filed (`SI_edFcvAdd`). What happens before depends on the type:
///
/// | Type | |
/// |---|---|
/// | 1–3 scaling | update flag 2 |
/// | 4–6 rotation | not for a dynamics root (algorithm 2); update flag 4; for a node under a direction constraint (`SI_edIsItemConstrained(…, 116, 0, 0.0)`) the value is replaced: type 4 by the float at +0x164 in degrees, 5 and 6 by 0 |
/// | 7 path translation | not for an effector of kind 4 ("… cannot have a translation path."), not when the element has translation curves ("… already has explicit translation."), not when the path cannot take a point ([`savekey_valid_translation`]); a node without a path gets one ([`path::path_new`] type 3, [`path::path_element_set`]) which, for a node with a father that is no joint or effector, becomes a son of that father (`SI_modelCreateParent`); update flag 8 |
/// | 8 shape | only patches, faces, meshes, splines and NURBS; update flag 0x10; `SI_shapeAddKey(model, 1, 0)`, and nothing else |
/// | 107–120, 121–138 fittings | only with a fitting record (`rec`) whose first word is set |
/// | 139–141 translation | not for algorithm 4; not when the node has a path or a path curve ("… already has a path translation fcurve."); update flag 8 |
///
/// Then the key: for type 7 the node's global translation is brought into the path's space
/// ([`savekey_map_translation`]) and added to the path (`SI_pathAddKeyPct` for a spline,
/// `SI_nurbsPathAddKeyPct` otherwise, with `keyed`); for every other type
/// `SI_fkeyFrameCreate(SI_frameCurrent, value, 0.0, 0, the curve's interpolation)` and
/// `SI_fcurveAddKey`. Under the classical transformation method (`SI_glbManip` = 1) the
/// types 1, 4, 139 and 7 end with `SI_savekeyCompensateInfo(model, curve)`.
#[allow(clippy::too_many_arguments)]
pub fn savekey_model<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, group: i32, relid: i32, kind: i32, check: i16,
                                             value: f32, keyed: i16, rec: Addr) {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 {
        return;
    }
    let mut value = value;
    let models = glob::chap(m, cid::MODELS);
    match kind {
        8 => {
            if !matches!(m.i32(mdl + model::KIND), 4 | 1 | 2 | 6 | 9 | 10) {
                KeyHost::status_display_message(h, m, msg::SHAPE_KIND, 3);
                return;
            }
            raise(m, mdl, 0x10);
            h.shape_add_key(m, mdl, 1, 0);
            return;
        }
        1..=3 => raise(m, mdl, 2),
        4..=6 => {
            if m.i32(mdl + model::ALGO_TYPE) == 2 {
                return;
            }
            raise(m, mdl, 4);
            let d = glob::cur_dict(m);
            if h.ed_is_item_constrained(m, d, mdl, models, 0x74, 0, 0.0) != 0 {
                value = if kind == 4 { (x(m.f32(mdl + model::VROT_XY_COPY)) * RAD2DEG).to_f32() } else { 0.0 };
            }
        }
        7 => {
            if m.i32(mdl + model::ALGO_TYPE) == 4 {
                KeyHost::status_display_message(h, m, msg::NO_PATH_A, 3);
                return;
            }
            if ed::find_fcv(m, e, 1, 0x71) != 0 {
                KeyHost::status_display_message(h, m, msg::HAS_EXPLICIT, 3);
                return;
            }
            let p = m.ptr(mdl + model::PATH);
            if !savekey_valid_translation(m, h, p) {
                return;
            }
            if m.ptr(mdl + model::PATH) == 0 {
                let p = path::path_new(m, h, 3);
                m.set_ptr(mdl + model::PATH, p);
                path::path_element_set(m, h, mdl, 2, p);
                let k = m.i32(mdl + model::KIND);
                if m.ptr(mdl + model::FATHER) != 0
                   && !(k == 0 && (m.i32(mdl + model::ALGO_TYPE).wrapping_sub(3) as u32) <= 1)
                   && k != 5
                {
                    let gl = glob::cur_graf_list(m);
                    let p = m.ptr(mdl + model::PATH);
                    let son = h.find_gl_data(m, gl, p, 2);
                    let gl = glob::cur_graf_list(m);
                    let f = m.ptr(mdl + model::FATHER);
                    let father = h.find_gl_data(m, gl, f, 0);
                    h.model_create_parent(m, father, son, 0);
                }
            }
            raise(m, mdl, 8);
        }
        0x6b..=0x8a => {
            if rec == 0 || m.i16(rec) == 0 {
                return;
            }
        }
        0x8b..=0x8d => {
            if m.i32(mdl + model::ALGO_TYPE) == 4 {
                KeyHost::status_display_message(h, m, msg::NO_PATH_B, 3);
                return;
            }
            let c = ed::find_fcv(m, e, 1, 0x70);
            if m.ptr(mdl + model::PATH) != 0 || c != 0 {
                KeyHost::status_display_message(h, m, msg::HAS_PATH, 3);
                return;
            }
            raise(m, mdl, 8);
        }
        _ => {}
    }
    let (e, index) = ed::get_elem_from_model(m, h, mdl);
    let mut c = ed::find_fcv(m, e, group, relid);
    if check != 0 {
        while c != 0 && !fcv_check_type_range(m, c, kind, kind) {
            c = ed::find_next_fcv(m, e, group, relid);
        }
    }
    if c == 0 {
        c = key::fcurve_allocate(m, kind, mdl);
        h.ed_fcv_add(m, c, e, models, index);
    }
    if kind == 7 {
        let v = m.local(12);
        for k in 0..3 {
            let w = m.u32(mdl + model::GTRS + 4 * k);
            m.set_u32(v + 4 * k, w);
        }
        let p = m.ptr(mdl + model::PATH);
        savekey_map_translation(m, p, v);
        let p = m.ptr(mdl + model::PATH);
        let geom = m.ptr(p + model::DEFINITION);
        if m.i32(p + model::KIND) == 6 {
            path::path_add_key_pct(m, h, geom, c, v, keyed);
        } else {
            path::nurbs_path_add_key_pct(m, h, geom, c, v, keyed);
        }
        m.release(12);
    } else {
        key::add_key_now(m, h, c, value, c);
    }
    if m.i32(glob::SI_GLB_MANIP) == 1 && matches!(kind, 1 | 4 | 0x8b | 7) {
        h.savekey_compensate_info(m, mdl, c);
    }
}

/// The nodes of a branch below `root`, in hierarchy order: `SI_modelNextNode` from the root
/// on while `SI_modelIsFather(root, node)` holds. Only walked when the root has a son.
fn each_descendant<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, root: Addr, mut f: impl FnMut(&mut Mem, &mut H, Addr)) {
    if m.ptr(root + model::SON) == 0 {
        return;
    }
    let mut p = root;
    loop {
        p = h.model_next_node(m, p);
        if p == 0 || h.model_is_father(m, root, p) == 0 {
            return;
        }
        f(m, h, p);
    }
}

fn scale_keys<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    for k in 0..3 {
        let v = fmov(m.f32(mdl + model::SCAL + 4 * k));
        savekey_model(m, h, mdl, 1, 100, 1 + k as i32, 1, v, 0, 0);
    }
}

fn rotation_keys<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    for k in 0..3 {
        let v = (x(RAD2DEG) * m.f32(mdl + model::VROT + 4 * k)).to_f32();
        savekey_model(m, h, mdl, 1, 0x65, 4 + k as i32, 1, v, 0, 0);
    }
}

/// Save key, scaling of a branch [soft 0x08219a08 si_savekeyBranchScale]: the three local
/// scaling factors (relation 100, types 1–3) of the node and of every descendant.
pub fn savekey_branch_scale<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    scale_keys(m, h, mdl);
    each_descendant(m, h, mdl, |m, h, p| scale_keys(m, h, p));
}

/// Save key, rotation of a branch [soft 0x08219b1c si_savekeyBranchRotate]: the three local
/// angles in degrees (57.29577951308232 × angle, rounded to `float`; relation 101, types
/// 4–6) of the node and of every descendant.
pub fn savekey_branch_rotate<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    rotation_keys(m, h, mdl);
    each_descendant(m, h, mdl, |m, h, p| rotation_keys(m, h, p));
}

/// Save key, translation of a branch [soft 0x08219c7c si_savekeyBranchTranslate]: a path
/// key (relation 112, type 7) for the node and every descendant, except descendants that
/// are splines something follows as a path (`SI_splineIsPath`); for a root with sons the
/// branch is then brought up to date (`SI_modelUpdateSonState(father, root, 1)`) and, if a
/// path was skipped, the status line says "Cannot save translation of a path in branch
/// mode.".
pub fn savekey_branch_translate<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    savekey_model(m, h, mdl, 1, 0x70, 7, 0, 0.0, 0, 0);
    if m.ptr(mdl + model::SON) == 0 {
        return;
    }
    let mut skipped = false;
    each_descendant(m, h, mdl, |m, h, p| {
        if m.i32(p + model::KIND) == 6 && h.spline_is_path(m, p) != 0 {
            skipped = true;
        } else {
            savekey_model(m, h, p, 1, 0x70, 7, 0, 0.0, 0, 0);
        }
    });
    let father = m.ptr(mdl + model::FATHER);
    h.model_update_son_state(m, father, mdl, 1);
    if skipped {
        KeyHost::status_display_message(h, m, msg::BRANCH_PATH, 2);
    }
}

/// Save key, everything of a branch [soft 0x082199d8 si_savekeyBranchAll]: scaling, rotation,
/// translation.
pub fn savekey_branch_all<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    savekey_branch_scale(m, h, mdl);
    savekey_branch_rotate(m, h, mdl);
    savekey_branch_translate(m, h, mdl);
}

/// Save key, visibility of a branch [soft 0x08219d40 si_savekeyBranchHidden]: relation 10
/// (list 0), type 175, the value 1 when the graf node's display type (+0xc) is 2 and 0
/// otherwise: for the node with the graf node given, for each descendant with its own
/// (`SI_findGlData(graf list, node, 3)`; a descendant without one gets no key).
pub fn savekey_branch_hidden<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, gn: Addr) {
    let shown = |m: &Mem, gn: Addr| if m.i32(gn + 0xc) == 2 { 1.0f32 } else { 0.0 };
    let v = shown(m, gn);
    savekey_model(m, h, mdl, 0, 10, 0xaf, 0, v, 0, 0);
    each_descendant(m, h, mdl, |m, h, p| {
        let gl = glob::cur_graf_list(m);
        let g = h.find_gl_data(m, gl, p, 3);
        if g != 0 {
            let v = shown(m, g);
            savekey_model(m, h, p, 0, 10, 0xaf, 0, v, 0, 0);
        }
    });
}

/// Save key, the whole transformation of a node [soft 0x08219e04 SI_savekeyObjectAll]:
/// scaling, rotation in degrees, and the translation: when the element has translation
/// curves (relation 113 in list 1), keys of types 139–141 with the local translation (a
/// joint or an effector: the global one), otherwise a path key.
pub fn savekey_object_all<H: SaveKeyHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    scale_keys(m, h, mdl);
    for k in 0..3u32 {
        let a = m.f32(mdl + model::VROT + 4 * k);
        let v = if k == 0 { x(a) * RAD2DEG } else { x(RAD2DEG) * a }.to_f32();
        savekey_model(m, h, mdl, 1, 0x65, 4 + k as i32, 1, v, 0, 0);
    }
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if ed::find_fcv(m, e, 1, 0x71) == 0 {
        savekey_model(m, h, mdl, 1, 0x70, 7, 0, 0.0, 0, 0);
    } else {
        let from = if m.i32(mdl + model::KIND) == 5 || m.i32(mdl + model::ALGO_TYPE) == 3 { model::GTRS } else { model::TRANS };
        let v = [m.f32(mdl + from), m.f32(mdl + from + 4), m.f32(mdl + from + 8)];
        for k in 0..3 {
            savekey_model(m, h, mdl, 1, 0x71, 0x8b + k as i32, 1, fmov(v[k]), 0, 0);
        }
    }
}
