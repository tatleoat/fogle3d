// SPDX-License-Identifier: MIT
//! `renu_util.c`: the camera a render uses, the greatest circle of confusion, picture file
//! names, the test for something to render, and the "transmit" flags that tell the mental
//! ray translator which elements must be sent again when a scene is rendered incrementally.

use crate::edlocal::find_elem_by_data;
use crate::instance::instance_elem;
use crate::outside::Outside;
use crate::world::{algo, cam, data_of, get_elem, mat, rel, t2d, Env};
use si_scene::dict::{Dict, ElemId};
use si_scene::elemdict as ed;
use si_scene::elemrel;
use si_types::dict::ELEM_VALID;
use si_types::model::{MBALL, MMETA};
use si_x87::x87::F80;

/// The camera a render looks through [soft 0x082cb590 SI_getRenderCamera].
///
/// In the stand-alone renderer it is the scene's camera, whose view code (+4) is set to
/// "perspective". Otherwise it is the camera of the highlighted window; with no window
/// highlighted, the camera of the default perspective window if its view code says
/// perspective, else the scene's camera, with (when `warn`) the status line `Using
/// perspective camera of default view B` and a pause of two seconds.
pub fn get_render_camera(env: &mut Env, out: &mut dyn Outside, warn: bool) -> u32 {
    if env.g.standalone_renderer {
        let c = env.g.scene_camera;
        if let Some(im) = env.heap.image_mut(c) {
            im.set_u16(cam::VIEW, cam::VIEW_PERSPECTIVE as u16);
        }
        return c;
    }
    match out.window_hilite_camera() {
        Some(c) => c,
        None => {
            let c = env.g.pers_camera;
            if env.heap.image(c).map_or(0, |im| im.i16(cam::VIEW)) == cam::VIEW_PERSPECTIVE {
                return c;
            }
            if warn {
                out.status_display_message(b"Using perspective camera of default view B", 2);
                out.usleep(2_000_000);
            }
            env.g.scene_camera
        }
    }
}

/// Reports and limits the camera's greatest circle of confusion
/// [soft 0x082cb620 SI_clampMaxcoc]. The camera is the scene's when `scene_camera`, else
/// the render camera. Its value (+0x98) as it stands is the answer. Then, only in depth of
/// field mode 2: a value below zero becomes 0 (`Max COC negative. Clamping at 0`); a value
/// above the limit becomes the limit (`Max COC exceeds limit. Clamping at %f`). The limit is
/// the picture width times 0.2, compared in the register before it is rounded to a float.
/// Each message is followed by a pause of two seconds unless `scene_camera`.
pub fn clamp_maxcoc(env: &mut Env, out: &mut dyn Outside, scene_camera: bool) -> Option<f32> {
    let c = if scene_camera { env.g.scene_camera } else { get_render_camera(env, out, false) };
    let width = env.g.picture_width;
    let im = env.heap.image_mut(c)?;
    let v = im.f32(cam::MAX_COC);
    // fld, fstp: the answer goes through the unit
    let answer = F80::from_f32(v).to_f32();
    if im.i32(cam::DOF_MODE) != 2 {
        return Some(answer);
    }
    let x = F80::from_f32(v);
    if x.lt(F80::from_f32(0.0)) {
        im.set_u32(cam::MAX_COC, 0);
        out.status_display_message(b"Max COC negative. Clamping at 0", 2);
        if !scene_camera {
            out.usleep(2_000_000);
        }
        return Some(answer);
    }
    let limit = F80::from_i32(width as i32) * F80::from_f64(0.2);
    if limit.lt(x) {
        let limit = limit.to_f32();
        im.set_f32(cam::MAX_COC, limit);
        out.status_display_message(format!("Max COC exceeds limit. Clamping at {:.6}", limit as f64).as_bytes(), 2);
        if !scene_camera {
            out.usleep(2_000_000);
        }
    }
    Some(answer)
}

/// Puts a value back as the greatest circle of confusion
/// [soft 0x082cb734 SI_assignMaxcoc], in depth of field mode 2 only.
pub fn assign_maxcoc(env: &mut Env, out: &mut dyn Outside, scene_camera: bool, value: f32) {
    let c = if scene_camera { env.g.scene_camera } else { get_render_camera(env, out, false) };
    if let Some(im) = env.heap.image_mut(c) {
        if im.i32(cam::DOF_MODE) == 2 {
            im.set_u32(cam::MAX_COC, value.to_bits());
        }
    }
}

/// True for a null string and for one of nothing but blanks, line feeds, backspaces and
/// tabs [soft 0x082cb774 SI_emptyString].
pub fn empty_string(s: Option<&[u8]>) -> bool {
    s.is_none_or(|s| s.iter().take_while(|c| **c != 0).all(|c| matches!(c, b' ' | b'\n' | 8 | b'\t')))
}

/// The name of a picture file [soft 0x082cb7a8 SI_buildPicFileName]:
/// `<name>.<frame>`, then `.<field>` when `with_field`, then `.pic` when `with_suffix`. A
/// name that does not start with a slash is put under `SI_picturePath`, with a slash between
/// unless the path ends in one. (For an empty picture path the original looks at the byte in
/// front of the string; the port takes it not to be a slash.) The original answers null
/// with the status line `Memory error` when an allocation fails; that cannot happen here.
pub fn build_pic_file_name(env: &Env, name: &[u8], frame: i32, with_field: bool, field: i32, with_suffix: bool) -> Vec<u8> {
    let mut s = Vec::new();
    if name.first() != Some(&b'/') {
        s.extend_from_slice(&env.g.picture_path);
        if env.g.picture_path.last() != Some(&b'/') {
            s.push(b'/');
        }
    }
    s.extend_from_slice(name);
    s.extend_from_slice(format!(".{frame}").as_bytes());
    if with_field {
        s.extend_from_slice(format!(".{field}").as_bytes());
    }
    if with_suffix {
        s.extend_from_slice(b".pic");
    }
    s
}

/// Whether a dictionary holds anything to render [soft 0x082cbaac SI_checkAllModels]: the
/// hierarchies of the root model elements in use are gone through until one answers yes.
/// `mode` 1 looks at every visible model, any other value at the selected ones only.
pub fn check_all_models(d: &mut Dict, env: &Env, out: &mut dyn Outside, mode: i16) -> i32 {
    let cm = env.p.cid.model;
    let mut found = false;
    let mut i = 0;
    while (i as usize) < d.ed.lists[cm as usize].elems.len() && !found {
        let rec = d.ed.lists[cm as usize].elems[i as usize].rec;
        if rec.status == ELEM_VALID && rec.hier_level == 0 {
            found = check_model_tree(d, env, out, ElemId::new(cm, i), mode, 0);
        }
        i += 1;
    }
    found as i32
}

/// One element and the elements under it [soft 0x082cbb1c checkModelTree]. An instance
/// that is still an instance is an error printed on standard output; the icon of a custom
/// effect is passed over; a patch, face, mesh, metaball system or NURBS surface counts when
/// its element is not hidden (+0x20), it is asked for (`mode` 1, or selection state 1, 2 or
/// 3) and it has triangles. An element without an object ends its branch.
fn check_model_tree(d: &mut Dict, env: &Env, out: &mut dyn Outside, e: ElemId, mode: i16, depth: usize) -> bool {
    if depth > d.ed.lists[e.chap as usize].elems.len() {
        return false;
    }
    let rec = d.slot(e).unwrap().rec;
    let Some(m) = env.heap.model(rec.data.0) else { return false };
    if rec.status != ELEM_VALID {
        return false;
    }
    if m.algo_type == algo::INSTANCE {
        let mut line = format!(
            "{},{}: Error: non-expanded instance found <",
            "/home/data/si3d/OfficialBuilds/v4_0_2_0/Creative/final.v4_0_2_0-Linux24-flexlm/renutil/renu_util.c", 0x1c1
        )
        .into_bytes();
        line.extend_from_slice(env.heap.str(m.name.0).unwrap_or(b"(null)"));
        line.extend_from_slice(b">\n");
        out.print(&line);
    } else if m.algo_type != algo::CUSTOM_ICON
        && matches!(m.kind, 1 | 2 | 4 | 7 | 10)
        && rec.wire_type != 0
        && (mode == 1 || matches!(rec.selection, 1..=3))
        && check_model(env, out, rec.data.0)
    {
        return true;
    }
    let mut son = ed::find_son(d, &env.p, e);
    while let Some(s) = son {
        if check_model_tree(d, env, out, s, mode, depth + 1) {
            return true;
        }
        son = ed::find_next_son(d, &env.p, e);
    }
    false
}

/// [soft 0x082cbc10 checkModel]: the node has at least one triangle.
fn check_model(env: &Env, out: &mut dyn Outside, model: u32) -> bool {
    out.model_nb_triangles(&env.heap, model) > 0
}

/// Whether an open window other than the fourth shows a perspective view
/// [soft 0x082cbc3c SI_perspectiveNotBView].
pub fn perspective_not_b_view(env: &Env, out: &mut dyn Outside) -> bool {
    out.template_windows()
        .iter()
        .enumerate()
        .any(|(i, (open, camera))| *open != 0 && i != 3 && env.heap.image(*camera).is_some_and(|im| im.i16(cam::VIEW) == cam::VIEW_PERSPECTIVE))
}

/// [soft 0x082cbc8c isADKitEffect]: the node is the icon of an installed custom effect
/// (algorithm 13) whose pointer at +0x104 is null or leads to a zero word.
pub fn is_adkit_effect(env: &Env, out: &mut dyn Outside, model: u32) -> bool {
    if env.heap.model(model).is_none_or(|m| m.algo_type != algo::CUSTOM_ICON) {
        return false;
    }
    matches!(out.cus_get_effect_by_icon(model), Some(None | Some(0)))
}

/// The transmit flags of an element: the low byte of its work flag (+0x58). Bit 1: the
/// element's transformation or instance must be sent; bit 2: all of it; bit 4: it is part of
/// a shade tree that must be sent. (The values of the `INCR_SEND_…` words of
/// `Relations.rsrc`: `INST` 1, `ALL` 3, `SHDTRE` 4, `WITH_SHDTRE` 7.)
fn or_flags(d: &mut Dict, e: ElemId, flags: i32) {
    if let Some(s) = d.slot_mut(e) {
        s.rec.work_flag |= flags & 0xff;
    }
}

/// Clears the transmit flags of every element in use of a dictionary
/// [soft 0x082cbccc InitSceneTransmitFlags].
pub fn init_scene_transmit_flags(d: &mut Dict) {
    for l in d.ed.lists.iter_mut().take(d.ed.rec.nb_lists.max(0) as usize) {
        for e in &mut l.elems {
            if e.rec.status == ELEM_VALID {
                e.rec.work_flag &= !0xff;
            }
        }
    }
}

/// Marks an element as part of a shade tree, and everything it is tied to by a relation
/// from 300 to 551 [soft 0x082cbd20 PropagateShadeTreeFlag]: first what points at it
/// (backward side, list 0), then what it points at (forward side, list 3). An element
/// already marked stops the walk, so a ring of relations ends.
pub fn propagate_shade_tree_flag(d: &mut Dict, e: ElemId) {
    let Some(s) = d.slot_mut(e) else { return };
    if s.rec.work_flag & 4 != 0 {
        return;
    }
    s.rec.work_flag |= 4;
    for forward in [false, true] {
        let group = if forward { 3 } else { 0 };
        let mut r = ed::find_range_related_elem(d, e, forward, group, rel::MAT, rel::SHADER_ARRAY, -1);
        while let Some((other, _)) = r {
            propagate_shade_tree_flag(d, other);
            r = ed::find_range_next_related_elem(d, e, forward, group, rel::MAT, rel::SHADER_ARRAY, -1);
        }
    }
}

fn both(flags: i32) -> bool {
    flags & 1 != 0 && flags & 2 != 0
}

/// Whether a 2D texture changes from frame to frame: its record says so (+0x19c), or the
/// first forward list of its element holds a relation 15005 (it is animated).
fn texture_changes(cur: &Dict, env: &Env, tex: u32, tex_elem: Option<ElemId>) -> bool {
    env.heap.image(tex).is_some_and(|im| im.i16(t2d::ANIMATED) != 0)
        || tex_elem.and_then(|t| cur.slot(t)).and_then(|s| s.forward.first()).is_some_and(|l| l.rels.iter().any(|r| r.relid == rel::ANIM_T2D))
}

/// The transmit flags of one element of a render dictionary and of the elements under it
/// [soft 0x082cbdf0 SetElemTransmitFlags]. `cur` is the program's current dictionary, the
/// scene's, which the original takes from its globals; `flags` is what the element inherits.
/// The answer says whether the element or one under it has a relation 2100 in the scene (a light whose
/// visibility flag is animated).
///
/// The element's counterpart in the scene is the scene element of the same index, or else
/// the one whose number the element's object carries at +0x354. From the counterpart:
/// - a metaball system takes bits 1 and 2 if a metaball under it has a forward relation
///   whose increment mode has one of them;
/// - the increment mode of every forward relation of the counterpart is added;
/// - a model that is the icon of an installed effect gives the increment mode of relation
///   20000 to every hierarchy the effect works on (list 4);
/// - a model takes bits 1 and 2 if one of its materials has static blur on, or (with 2D
///   textures switched on) one of its local or global 2D textures changes from frame to
///   frame.
///
/// With bit 4 the element's shade tree is marked. The elements under the element inherit
/// the flags without bit 4.
///
/// Where the original reads the number at +0x354 of an object that is not a model, it reads
/// whatever the record has there; the port does so for a material and finds nothing for the
/// shorter records. Where the original would never leave the loop over an effect's
/// hierarchies (a hierarchy whose element the render dictionary lacks) the port leaves it.
pub fn set_elem_transmit_flags(ren: &mut Dict, cur: &mut Dict, env: &mut Env, out: &mut dyn Outside, e: ElemId, flags: i32, chap: i32) -> bool {
    transmit(ren, cur, env, out, e, flags, chap, 0)
}

#[allow(clippy::too_many_arguments)]
fn transmit(ren: &mut Dict, cur: &mut Dict, env: &mut Env, out: &mut dyn Outside, e: ElemId, flags: i32, chap: i32, depth: usize) -> bool {
    let c = env.p.cid;
    if depth > ren.ed.lists.iter().map(|l| l.elems.len()).sum::<usize>() {
        return false;
    }
    let mut flags = flags;
    let mut found = false;
    let id = ed::get_elem_id(ren, chap, e);
    let counterpart = get_elem(cur, chap, id).or_else(|| {
        let data = data_of(ren, e);
        let carried = match env.heap.model(data) {
            Some(m) => Some(m.elem_id),
            None => env.heap.image(data).filter(|im| im.bytes.len() >= 0x358).map(|im| im.i32(0x354)),
        };
        get_elem(cur, chap, carried?)
    });
    if let Some(se) = counterpart {
        let se_model = data_of(cur, se);
        if chap == c.model && env.heap.model(se_model).is_some_and(|m| m.kind == MMETA) {
            let mut son = ed::find_son(cur, &env.p, se);
            while let (false, Some(s)) = (both(flags), son) {
                if env.heap.model(data_of(cur, s)).is_some_and(|m| m.kind == MBALL) {
                    let ids: Vec<i32> = cur.slot(s).unwrap().forward.iter().flat_map(|l| l.rels.iter().map(|r| r.relid)).collect();
                    for relid in ids {
                        if env.p.rd.get_rel_incmode_flag(relid) & 3 != 0 {
                            flags |= 3;
                        }
                    }
                }
                son = ed::find_next_son(cur, &env.p, se);
            }
        }
        let ids: Vec<i32> = cur.slot(se).unwrap().forward.iter().flat_map(|l| l.rels.iter().map(|r| r.relid)).collect();
        for relid in ids {
            if relid == rel::LIG_VISIB {
                found = true;
            }
            flags |= env.p.rd.get_rel_incmode_flag(relid);
        }
        if chap == c.model {
            if is_adkit_effect(env, out, se_model) {
                let inc = env.p.rd.get_rel_incmode_flag(rel::CUSTOM);
                let mut r = elemrel::find_related_elem(cur, se, true, 4, rel::CUSTOM, c.model);
                while let Some(target) = r {
                    let Some(p) = get_elem(ren, chap, target.index) else { break };
                    set_hierarchy_transmit_flags(ren, env, p, inc, 0);
                    r = elemrel::find_next_related_elem(cur, se, true, 4, rel::CUSTOM, c.model);
                }
            }
            let mut m = ed::get_first_related_elem(cur, se, true, 3, rel::MAT, c.mat);
            while let (Some(me), false) = (m.elem, both(flags)) {
                if env.heap.image(m.data.0).is_some_and(|im| im.i16(mat::STATIC_BLUR) != 0) {
                    flags |= 3;
                }
                if env.g.textures_2d_on != 0 {
                    let mut t = ed::get_first_related_elem(cur, me, true, 3, rel::T2D_LOC, c.t2d);
                    while !t.data.is_null() && !both(flags) {
                        if texture_changes(cur, env, t.data.0, t.elem) {
                            flags |= 3;
                        }
                        t = ed::get_next_related_elem(cur, me, true, 3, rel::T2D_LOC, c.t2d, Some(t.cursor));
                    }
                }
                m = ed::get_next_related_elem(cur, se, true, 3, rel::MAT, c.mat, Some(m.cursor));
            }
            if env.g.textures_2d_on != 0 {
                let mut t = ed::get_first_related_elem(cur, se, true, 3, rel::T2D_GLB, c.t2d);
                while !t.data.is_null() && !both(flags) {
                    if texture_changes(cur, env, t.data.0, t.elem) {
                        flags |= 3;
                    }
                    t = ed::get_next_related_elem(cur, se, true, 3, rel::T2D_GLB, c.t2d, Some(t.cursor));
                }
            }
        }
    }
    if flags & 4 != 0 {
        propagate_shade_tree_flag(ren, e);
    }
    or_flags(ren, e, flags);
    let mut son = ed::find_son(ren, &env.p, e);
    while let Some(s) = son {
        found |= transmit(ren, cur, env, out, s, flags & !4, chap, depth + 1);
        son = ed::find_next_son(ren, &env.p, e);
    }
    found
}

/// Adds flags to an element and to every element under it
/// [soft 0x082cc3dc setHierarchyTransmitFlags], leaving the cursor of the element's list of
/// sons where it was.
pub fn set_hierarchy_transmit_flags(ren: &mut Dict, env: &Env, e: ElemId, flags: i32, depth: usize) {
    if depth > ren.ed.lists[e.chap as usize].elems.len() {
        return;
    }
    let saved = ren.slot(e).unwrap().get_cur_relation_index(false, 1);
    or_flags(ren, e, flags);
    let mut son = ed::find_son(ren, &env.p, e);
    while let Some(s) = son {
        set_hierarchy_transmit_flags(ren, env, s, flags, depth + 1);
        son = ed::find_next_son(ren, &env.p, e);
    }
    ren.slot_mut(e).unwrap().set_cur_relation_index(false, 1, saved);
}

/// Works out the transmit flags of a whole render dictionary
/// [soft 0x082cc448 SI_SetSceneTransmitFlags]. `cur` is the program's current dictionary.
///
/// 1. If the current dictionary's update list is stale and the global at 0x08781596 is set,
///    it is sorted again (`SI_elemdictTopoSort`).
/// 2. All flags are cleared; `SI_dbCheckExtraUnToldRelation(current, 1, 1, 0)`.
/// 3. The roots in use of LIGHTS, starting from no flags.
/// 4. The roots in use of MATERIALS, starting from bit 4 if a light has a relation 2100.
/// 5. The roots in use of every other chapter, in order, starting from no flags.
/// 6. Every root model in use that belongs to an instance takes bit 2 from the render
///    element of the instance's master.
/// 7. `SI_dbCheckExtraUnToldRelation(current, 0, 1, 0)`.
pub fn set_scene_transmit_flags(ren: &mut Dict, cur: &mut Dict, env: &mut Env, out: &mut dyn Outside) {
    let c = env.p.cid;
    if cur.ed.rec.upd_list.dirty != 0 && env.p.backup_enabled {
        out.topo_sort(cur);
    }
    init_scene_transmit_flags(ren);
    out.db_check_extra_untold_relation(cur, 1, 1, 0);
    let roots = |ren: &Dict, chap: i32, i: usize| {
        let rec = ren.ed.lists[chap as usize].elems[i].rec;
        rec.status == ELEM_VALID && rec.hier_level == 0
    };
    let mut any = false;
    let mut i = 0;
    while i < ren.ed.lists[c.lig as usize].elems.len() {
        if roots(ren, c.lig, i) {
            any |= transmit(ren, cur, env, out, ElemId::new(c.lig, i as i32), 0, c.lig, 0);
        }
        i += 1;
    }
    let mut i = 0;
    while i < ren.ed.lists[c.mat as usize].elems.len() {
        if roots(ren, c.mat, i) {
            transmit(ren, cur, env, out, ElemId::new(c.mat, i as i32), (any as i32) << 2, c.mat, 0);
        }
        i += 1;
    }
    for chap in 0..ren.ed.rec.nb_lists {
        if chap == c.lig || chap == c.mat {
            continue;
        }
        let mut i = 0;
        while i < ren.ed.lists[chap as usize].elems.len() {
            if roots(ren, chap, i) {
                transmit(ren, cur, env, out, ElemId::new(chap, i as i32), 0, chap, 0);
            }
            i += 1;
        }
    }
    let mut i = 0;
    while i < ren.ed.lists[c.model as usize].elems.len() {
        let index = i as i32;
        i += 1;
        let rec = ren.ed.lists[c.model as usize].elems[index as usize].rec;
        if rec.status != ELEM_VALID || rec.data.is_null() {
            continue;
        }
        let Some(inst) = instance_elem(index, cur, ren, env) else { continue };
        if rec.hier_level != 0 {
            continue;
        }
        let Some(master) = elemrel::find_related_elem(cur, inst, true, 4, rel::INST, c.model) else { continue };
        let at = find_elem_by_data(ren, c.model, data_of(cur, master));
        if let Some(p) = get_elem(ren, c.model, at) {
            let bit = ren.slot(p).unwrap().rec.work_flag & 2;
            ren.ed.lists[c.model as usize].elems[index as usize].rec.work_flag |= bit;
        }
    }
    out.db_check_extra_untold_relation(cur, 0, 1, 0);
}
