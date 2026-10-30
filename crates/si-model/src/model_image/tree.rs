// SPDX-License-Identifier: MIT
//! The hierarchy on the image: the walks over a tree of nodes (two of them from
//! `model_b.c`, which the routines here cannot do without) and the cutting of a node from
//! its father.

use si_mem::{Mem, Ptr32};
use si_types::geom::{Patch, Spline};
use si_types::model::Model;

use super::{callee, defs, elem_dict, g, models_chapter, readjust, setup, state, Callee, Outside, CUT_MESSAGES};

/// The root above a node [soft 0x0847838c SI_modelRoot] (`model_b.c`).
pub fn root(m: &Mem, node: Ptr32<Model>) -> Ptr32<Model> {
    let mut at = node;
    while !m[at].father.is_null() {
        at = m[at].father;
    }
    at
}

/// The node after this one in the order "sons before brothers", over the whole forest the
/// node's root begins (the root's own brothers included) [soft 0x08478340 SI_modelNextNode]
/// (`model_b.c`); null after the last.
pub fn next_node(m: &Mem, node: Ptr32<Model>) -> Ptr32<Model> {
    let next = m[node].son;
    if !next.is_null() {
        return next;
    }
    let next = m[node].brother;
    if !next.is_null() {
        return next;
    }
    let mut at = node;
    loop {
        let father = m[at].father;
        if father.is_null() {
            if m[at].brother.is_null() {
                return Ptr32::NULL;
            }
            break;
        }
        at = father;
        if !m[at].brother.is_null() {
            break;
        }
    }
    m[at].brother
}

/// The node after `at` in the subtree of `top`, in the same order; the first son of `top`
/// for a null `at`; null after the last [soft 0x0846d70c SI_modelNextSon]. The climb
/// follows the father links until it meets `top`, without a test for null.
pub fn next_son(m: &Mem, top: Ptr32<Model>, at: Ptr32<Model>) -> Ptr32<Model> {
    if at.is_null() {
        return m[top].son;
    }
    let next = m[at].son;
    if !next.is_null() {
        return next;
    }
    let next = m[at].brother;
    if !next.is_null() {
        return next;
    }
    let mut at = at;
    if at != top {
        loop {
            at = m[at].father;
            if !m[at].brother.is_null() {
                break;
            }
            if at == top {
                break;
            }
        }
        if at != top {
            return m[at].brother;
        }
    }
    Ptr32::NULL
}

/// Takes a node out of its father's list of sons [soft 0x0846d790 SI_modelCutFather]:
/// every metaball system on the way up, the node itself included (kind 7), is touched
/// (`SI_metaballTouch(NULL, its definition)`); the envelope is cut (`SI_envelopCut`); then
/// the node is unlinked: its father's first son, or the brother before it, takes its
/// brother; its father and brother links are cleared. A node without a father keeps its
/// brother link.
pub fn cut_father(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    let mut at = node;
    while !at.is_null() {
        if m[at].kind == 7 {
            let system = m[at].definition.addr();
            o.call(m, &callee::SI_METABALL_TOUCH, &[0, system]);
        }
        at = m[at].father;
    }
    o.call(m, &callee::SI_ENVELOP_CUT, &[node.addr()]);
    let father = m[node].father;
    if !father.is_null() {
        let mut before = m[father].son;
        if before == node {
            let next = m[node].brother;
            m[father].son = next;
        } else {
            while m[before].brother != node {
                before = m[before].brother;
            }
            let next = m[node].brother;
            m[before].brother = next;
        }
        m[node].father = Ptr32::NULL;
        m[node].brother = Ptr32::NULL;
    }
}

/// Cuts a displayed node from its father [soft 0x0846d838 SI_modelCut]; 1 when done, 0
/// with a message at `message` when refused. `graf` is a `__SI_GrafNode` (named offsets:
/// the model at +4, a selection mode at +8, a 16-bit flag word at +0x16).
///
/// Refused: a joint or an effector ("A part of a chain cannot be cut"); when the physical
/// properties say no (`SI_dynaPhyPropsValidateCopyDelete(node, mode, 0)`, the mode from
/// the flag word: without bit 0x1000 the word at +8, else 1 for bit 0x2000, 3 for 0x4000,
/// 2 for 0x8000, 0); when the father is a null whose element has a relation 0xfa or 0xfb.
///
/// Otherwise: the physical properties get a new root, the schematic view is told, every
/// node of the whole tree gets bit 0x20 of its update flags, the metaball systems above
/// are touched, the cut goes into the history (not with flag bit 0x1000 of the displayed
/// node, nor during an export), the node keeps its place if `SI_hrchCpst` says so
/// ([`state::compensate_cutting_matrix`]), it is unlinked ([`cut_father`]); a node whose
/// geometry lives in another node's space has its dependency recomputed and its own and
/// its descendants' definitions readjusted; it is updated as a root; and the display list
/// (or, during an export, the scene) is told.
pub fn cut(m: &mut Mem, o: &mut dyn Outside, graf: u32, message: u32) -> u32 {
    let scratch = m.local(4);
    let node: Ptr32<Model> = Ptr32::new(m.u32(graf + 4));
    let refuse = |m: &mut Mem, which: usize| {
        // sprintf(message, text): the texts hold no conversion
        si_mem::clib::strcpy(m, message, CUT_MESSAGES[which]);
        m.release(4);
        0
    };
    if m[node].kind == 5 || (m[node].kind == 0 && (m[node].algo_type.wrapping_sub(3) as u32) < 2) {
        return refuse(m, 0);
    }
    let flags = m.u16(graf + 0x16);
    let mode = if flags & 0x1000 == 0 {
        m.u32(graf + 8)
    } else if flags & 0x2000 != 0 {
        1
    } else if flags & 0x4000 != 0 {
        3
    } else if (flags as i16) < 0 {
        2
    } else {
        0
    };
    if o.call(m, &callee::SI_DYNA_PHY_PROPS_VALIDATE_COPY_DELETE, &[node.addr(), mode, 0]) as i16 == 0 {
        return refuse(m, 1);
    }
    let father = m[node].father;
    if !father.is_null() && m[father].kind == 0 {
        let element = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[father.addr(), scratch]);
        if element != 0 {
            let (dict, chapter) = (elem_dict(m), models_chapter(m));
            let a = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 2, 0xfa, chapter, scratch, scratch]);
            let (dict, chapter) = (elem_dict(m), models_chapter(m));
            let b = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 2, 0xfb, chapter, scratch, scratch]);
            if a != 0 || b != 0 {
                return refuse(m, 2);
            }
        }
    }
    o.call(m, &callee::SI_DYNA_PHY_PROPS_SET_NEW_ROOT, &[node.addr()]);
    o.call(m, &callee::SI_SCHEMATIC_CHECK_DELETE_EXPAND, &[graf, 1]);
    let mut at = root(m, node);
    loop {
        m.or_u8(at.addr() + 0x0a, 0x20);
        at = next_node(m, at);
        if at.is_null() {
            break;
        }
    }
    let mut above = m[node].father;
    while !above.is_null() {
        if m[above].kind == 7 {
            let system = m[above].definition.addr();
            o.call(m, &callee::SI_METABALL_TOUCH, &[0, system]);
        }
        above = m[above].father;
    }
    let father = m[node].father;
    let list = m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24));
    let shown = o.call(m, &callee::SI_FIND_GL_DATA, &[list, father.addr(), 0]);
    if m.u8(graf + 0x17) & 0x10 == 0 && m.i16(g::SI_ED_SUMATRA_TO_3X.addr()) == 0 {
        o.call(m, &callee::SI_MODEL_SAVE_HISTORY_PARENTING, &[graf, shown, 1]);
    }
    if m[g::SI_HRCH_CPST] != 0 {
        state::compensate_cutting_matrix(m, node, father);
    }
    cut_father(m, o, node);
    if m[node].latdep != node {
        o.call(m, &callee::SI_MODEL_UPDATE_DEPENDENCY, &[node.addr(), 0]);
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
        let mut at = Ptr32::NULL;
        loop {
            at = next_son(m, node, at);
            if at.is_null() {
                break;
            }
            o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[at.addr()]);
        }
    }
    state::update_son_state(m, o, Ptr32::NULL, node, 1);
    if m.i16(g::SI_ED_SUMATRA_TO_3X.addr()) == 0 {
        let list = m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24));
        o.call(m, &callee::SI_GRAFLIST_CUT_MODEL, &[list, graf, father.addr(), 0]);
    } else {
        o.call(m, &callee::SI_ED_CUT_MODEL, &[father.addr(), graf, 0]);
    }
    m.release(4);
    1
}

/// Hangs a displayed node under another [soft 0x0817b500 SI_modelCreateParent]: `father`
/// and `child` are `__SI_GrafNode`s (named offset: the model at +4); `mode` is a 16-bit
/// argument handed on to the display list. Either null: nothing.
///
/// The child keeps its place if `SI_hrchCpst` says so
/// ([`state::compensate_parenting_matrix`]), is appended as the father's last son and gets
/// the father's global values (`SI_modelUpdateSonState(father, child, 1)`), and the display
/// list links the two (`SI_graflistLinkModel`, not during an export). A father whose
/// geometry lives in another node's space, or that has a lattice, spline fitting, patch
/// fitting or wave for its branch, has the dependency of its root recomputed and the
/// child's and the child's descendants' definitions readjusted. Every node of the child's
/// tree gets bit 0x20 of its update flags (+0x0a and +0x3fa), and the metaball systems on
/// the way up from the father, the father included (kind 7), are touched
/// (`SI_metaballTouch(NULL, its definition)`).
pub fn create_parent(m: &mut Mem, o: &mut dyn Outside, father_graf: u32, child_graf: u32, mode: i16) {
    if father_graf == 0 || child_graf == 0 {
        return;
    }
    let child: Ptr32<Model> = Ptr32::new(m.u32(child_graf + 4));
    let father: Ptr32<Model> = Ptr32::new(m.u32(father_graf + 4));
    if m[g::SI_HRCH_CPST] != 0 {
        state::compensate_parenting_matrix(m, o, child, father);
    }
    let mut last = m[father].son;
    if last.is_null() {
        m[father].son = child;
    } else {
        while !m[last].brother.is_null() {
            last = m[last].brother;
        }
        m[last].brother = child;
    }
    m[child].father = father;
    state::update_son_state(m, o, father, child, 1);
    if m.i16(g::SI_ED_SUMATRA_TO_3X.addr()) == 0 {
        let list = m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24));
        o.call(m, &callee::SI_GRAFLIST_LINK_MODEL, &[list, father_graf, child_graf, mode as i32 as u32, 0]);
    }
    if m[father].latdep != father
        || super::readjust::tree_lattice(m, father) != 0
        || super::readjust::tree_spline_fitting(m, father) != 0
        || super::readjust::branch_patch_fitting(m, father) != 0
        || super::readjust::branch_wave(m, father) != 0
    {
        let top = root(m, father);
        o.call(m, &callee::SI_MODEL_UPDATE_DEPENDENCY, &[top.addr(), 0]);
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[child.addr()]);
        let mut at = Ptr32::NULL;
        loop {
            at = next_son(m, child, at);
            if at.is_null() {
                break;
            }
            o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[at.addr()]);
        }
    }
    let mut at = root(m, child);
    loop {
        m.or_u8(at.addr() + 0x0a, 0x20);
        m.or_u8(at.addr() + 0x3fa, 0x20);
        at = next_node(m, at);
        if at.is_null() {
            break;
        }
    }
    let mut above = Ptr32::<Model>::new(m.u32(father_graf + 4));
    while !above.is_null() {
        if m[above].kind == 7 {
            let system = m[above].definition.addr();
            o.call(m, &callee::SI_METABALL_TOUCH, &[0, system]);
        }
        above = m[above].father;
    }
}

/// Routines of `si-view` reached through this module's [`Outside`]: the same name and address.
struct ViewOutside<'a>(&'a mut dyn Outside);

impl si_view::Outside for ViewOutside<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        // the walk over a model's sons is this module's own: [soft 0x08478340 SI_modelNextNode]
        if routine.addr == 0x0847_8340 {
            return next_node(m, Ptr32::new(args[0])).addr();
        }
        self.0.call(m, &Callee { name: routine.name, addr: routine.addr }, args)
    }
}

/// The same for the routines `si-spline` reaches.
struct SplineOutside<'a>(&'a mut dyn Outside);

impl si_spline::spline_image::Outside for SplineOutside<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_spline::spline_image::Callee, args: &[u32]) -> u32 {
        self.0.call(m, &Callee { name: callee.name, addr: callee.addr }, args)
    }
}

/// The list of displayed nodes of the current scene: `SI_grafLists` + 0x24 × the second word
/// of the process's data (the original loads both globals again at every use).
fn current_graf_list(m: &Mem) -> Ptr32<si_types::scene::GrafList> {
    Ptr32::new(m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24)))
}

/// Makes the nodes of a patch surface from its spline [soft 0x081568a8 si_createPatchHierarchyFromSpline]: `model_at` is the
/// address of a `__SI_Model *` that is read and then written (the node on which the whole
/// hangs). `caps` asks for the cap nodes: a "bottom" and a "top" node, each with a face
/// made of the patch's first and last spline, as sons of the node around a node that holds
/// the patch itself (the nodes of a revolution, `extrude` 0 "revol", or of an extrusion,
/// "extru"); without `caps` the patch goes into a single node. `update` replaces the
/// definitions of nodes that exist: with `caps` the node's son, that son's brother and
/// that one's brother (the bottom cap, the patch node, the top cap), without `caps` the
/// node itself, or its son's brother when it has no geometry kind (0); without `update`
/// the nodes are made new (`SI_modelAllocate`, through `o`: it lies in `si-rensetup`). With
/// `caps` and `update` and a node without a son, the nodes are made new and the node's
/// definitions are deleted, the three nodes hung under it through the display list
/// (`SI_findGlData`, `SI_graflistAddData`, `SI_modelCreateParent`). With `caps` the answer
/// stored at `model_at` is the node itself, otherwise the node that holds the patch.
///
/// Where the original reads a stored definition array that is null it reads address 0; the
/// image faults there.
pub fn create_patch_hierarchy_from_spline(m: &mut Mem, o: &mut dyn Outside, patch: Ptr32<Patch>, model_at: u32, update: i16, caps: i16, extrude: i16) {
    let top_node: Ptr32<Model> = Ptr32::new(m.u32(model_at));
    let mut bottom: Ptr32<Model> = Ptr32::NULL;
    let mut top: Ptr32<Model> = Ptr32::NULL;
    let mut replace = false;
    if caps != 0 {
        replace = m[top_node].son.is_null() && update != 0;
        let first = si_spline::patch_image::patch_extract_spline(m, patch, 1, 0.0);
        si_spline::convert_image::spline_inverse(m, &mut SplineOutside(o), first);
        let face = si_hrc::image::spline::face_allocate(m);
        let outline: Ptr32<Spline> = Ptr32::new(face.addr() + 4);
        si_spline::spline_image::spline_init(m, outline);
        si_spline::intersect_image::spline_transfer(m, first, outline);
        if update == 0 || replace {
            bottom = Ptr32::new(o.call(m, &callee::SI_MODEL_ALLOCATE, &[0x0868_9ea0]));
        } else {
            bottom = m[top_node].son;
            top = m[m[bottom].brother].brother;
        }
        if update != 0 && !replace {
            defs::substitute_idefinition(m, o, bottom, 0, face.addr());
        } else {
            setup::add_definition(m, bottom, 2, face.addr());
        }
        let (segments, _step) = si_spline::eval::nb_segment_step(m[patch].u_type, m[patch].nb_u as i32, m[patch].u_closed);
        // (`fild` of the count, stored as a float)
        let second = si_spline::patch_image::patch_extract_spline(m, patch, 1, segments as f32);
        let face = si_hrc::image::spline::face_allocate(m);
        let outline: Ptr32<Spline> = Ptr32::new(face.addr() + 4);
        si_spline::spline_image::spline_init(m, outline);
        si_spline::intersect_image::spline_transfer(m, second, outline);
        if update == 0 || replace {
            top = Ptr32::new(o.call(m, &callee::SI_MODEL_ALLOCATE, &[0x0868_9ea7]));
        }
        if update != 0 && !replace {
            defs::substitute_idefinition(m, o, top, 0, face.addr());
        } else {
            setup::add_definition(m, top, 2, face.addr());
        }
        if update == 0 {
            m[top_node].son = bottom;
            m[bottom].father = top_node;
            m[top].father = top_node;
        }
    }
    // the node that holds the patch
    let holder: Ptr32<Model>;
    if update == 0 || replace {
        let name = if extrude == 0 { 0x0868_9eb1 } else { 0x0868_9eab };
        holder = Ptr32::new(o.call(m, &callee::SI_MODEL_ALLOCATE, &[name]));
    } else if caps != 0 {
        holder = m[bottom].brother;
    } else if m[top_node].kind != 0 {
        holder = top_node;
    } else {
        holder = m[m[top_node].son].brother;
    }
    if update != 0 && !replace {
        let first_stored = m.u32(m[holder].defs.addr());
        if m[holder].definition.addr() != first_stored || m[holder].bbox_valid != 0 {
            si_spline::patch_image::patch_update_surface(m, patch);
        }
        defs::substitute_idefinition(m, o, holder, 0, patch.addr());
    } else {
        setup::add_definition(m, holder, 1, patch.addr());
    }
    if replace {
        readjust::delete_definition(m, o, top_node, 0);
        let list = current_graf_list(m);
        let shown = si_view::graflist::find_gl_data(m, list, Ptr32::new(top_node.addr()), 2);
        for node in [bottom, holder, top] {
            let list = current_graf_list(m);
            let added = si_view::graflist::graflist_add_data(m, &mut ViewOutside(o), list, Ptr32::new(node.addr()), 2, 0);
            create_parent(m, o, shown.addr(), added.addr(), 0);
            m[node].brother = Ptr32::NULL;
        }
    }
    let mut result = holder;
    if caps != 0 {
        result = top_node;
        if update == 0 {
            m[holder].father = top_node;
            m[bottom].brother = holder;
            m[holder].brother = top;
            defs::change_name(m, top_node, 0x0868_9eb7);
        }
    }
    m.set_u32(model_at, result.addr());
}

/// Makes the nodes of a revolution of a spline [soft 0x08195b00 createPatchFromRevolutionFromSpline]:
/// the spline is turned into a patch by `SI_splineRevolutionToPatch`
/// ([`si_spline::revolution_image::spline_revolution_to_patch`], with `compensate` 1) from the
/// revolution record at `rev` (+0 the caps flag, a short; +4 the angle, a float; +8 the step
/// count, its low 16 bits; +0xc the axis; +0x18 the patch type; +0x1c the definition word),
/// `model` is the axis model. When a patch results, its nodes are made by
/// [`create_patch_hierarchy_from_spline`] (extrude 0) at `model_at` with the caps flag, which
/// is 0 when the spline's `closed` word (+8) is 0, and `update`. Answers whether a patch was made.
pub fn create_patch_from_revolution_from_spline(m: &mut Mem, o: &mut dyn Outside, model_at: u32, spline: Ptr32<Spline>, model: u32, rev: u32, update: i16) -> bool {
    let mut caps = m.u16(rev) as i16;
    if m[spline].closed == 0 {
        caps = 0;
    }
    let kind = m.u32(rev + 0x18) as i32;
    let steps = m.u32(rev + 8) as i16;
    let angle = f32::from_bits(m.u32(rev + 4));
    let axis = m.u32(rev + 0xc) as i32;
    let def = m.u32(rev + 0x1c) as i32;
    let patch = si_spline::revolution_image::spline_revolution_to_patch(m, &mut SplineOutside(o), spline, kind, steps, angle, axis, model, 1, def);
    if patch.is_null() {
        return false;
    }
    create_patch_hierarchy_from_spline(m, o, patch, model_at, update, caps, 0);
    true
}

/// The nodes of a revolution of a model [soft 0x08195b7c] (createModelFromRevolution, PARTIAL
/// port). `src` is the model revolved (its kind at +0x30, its definition at +0x48, its
/// matrix at +0x1c4), `model` the axis model, `model_at` the address of a `__SI_Model *`
/// (read as the node to update when not null, and written), `rev` the revolution record
/// (see [`create_patch_from_revolution_from_spline`]; +0 caps, +4 angle, +8 steps, +0xc
/// axis, +0x10 a short, +0x14 the patch flag, +0x18 the patch type, +0x1c the definition word).
///
/// Branch map of the original, by the kind of `src`:
/// - 6 (spline): copy the spline, multiply its points by `src`'s matrix; make or take the
///   node ("revol"); with +0x14 zero `meshRevolveSpline` gives a mesh (kind 4), otherwise
///   [`create_patch_from_revolution_from_spline`] makes a patch (IS_PATCH 1) and the
///   node, or all its sons but the caps, get kind 1; dispose the spline copy.
///   PORTED, both sub-branches (the mesh one through the `meshRevolveSpline` hook).
/// - 2 (face): `SI_faceCopy`, then `meshRevolveFace` (+0x14 zero), or `SI_revolveNurbs` per
///   outline / `faceRevolutionToPatchs` with `si_createPatchHierarchyFromFace`.
///   NOT PORTED.
/// - 9 (NURBS curve): `SI_nurbsCrvCopy`, `SI_nurbsCrvApplyMatrix`, then `meshRevolveFace`
///   on a sampled face or `SI_revolveNurbs` with `si_createNurbsHierarchyFromNurbsCrv`.
///   NOT PORTED.
/// - any other kind: answer 1 with nothing made.
///
/// Answers `Some(1)` (the original's short answer is 1 on every path of the spline branch,
/// failures included) and `None` for a kind whose branch is not ported (nothing is done).
pub fn create_model_from_revolution(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Model>, model: u32, model_at: u32, rev: u32) -> Option<i32> {
    let kind = m.u32(src.addr() + 0x30) as i32;
    if kind == 2 || kind == 9 {
        return None;
    }
    if kind != 6 {
        return Some(1);
    }
    let caps = m.u16(rev) as i16;
    let angle_bits = m.u32(rev + 4);
    let steps = m.u32(rev + 8);
    let axis = m.u32(rev + 0xc);
    let short_10 = m.u16(rev + 0x10) as i16;
    let update = m.u32(model_at) != 0;
    let spline_at = m.local(4);
    let node_at = m.local(4);
    let copy = si_spline::intersect_image::spline_copy(m, Ptr32::new(m.u32(src.addr() + 0x48)));
    m.set_u32(spline_at, copy.addr());
    si_spline::convert_image::spline_multiply_definition(m, &mut SplineOutside(o), copy, src.addr() + 0x1c4);
    let existing = m.u32(model_at);
    m.set_u32(node_at, existing);
    if !update {
        // the name is the string "revol" at 0x0868d660
        let n = o.call(m, &callee::SI_MODEL_ALLOCATE, &[0x0868_d660]);
        m.set_u32(node_at, n);
    }
    let mut done = false;
    if m.u16(rev + 0x14) == 0 {
        let mesh = o.call(
            m,
            &callee::MESH_REVOLVE_SPLINE,
            &[m.u32(spline_at), short_10 as i32 as u32, steps, angle_bits, axis, model, caps as i32 as u32],
        );
        if mesh == 0 {
            if !update {
                o.call(m, &callee::SI_MODEL_DISPOSE_NODE, &[node_at]);
            }
            si_hrc::image::spline::spline_dispose(m, spline_at);
            m.release(4);
            m.release(4);
            return Some(1);
        }
        let node = Ptr32::new(m.u32(node_at));
        if !update {
            setup::add_definition(m, node, 4, mesh);
        } else {
            defs::substitute_idefinition(m, o, node, 0, mesh);
        }
        m.set_u32(node.addr() + 0x30, 4);
        done = true;
    }
    if !done {
        if !create_patch_from_revolution_from_spline(m, o, node_at, Ptr32::new(m.u32(spline_at)), model, rev, update as i16) {
            if !update {
                o.call(m, &callee::SI_MODEL_DISPOSE_NODE, &[node_at]);
            }
            si_hrc::image::spline::spline_dispose(m, spline_at);
            m.release(4);
            m.release(4);
            return Some(1);
        }
        let node = m.u32(node_at);
        let son = m.u32(node + 0x2c4);
        if son == 0 || m.u32(node + 0x30) != 0 {
            m.set_u32(node + 0x30, 1);
        } else {
            let mut at = son;
            while at != 0 {
                if m.u32(at + 0x30) != 2 {
                    m.set_u32(at + 0x30, 1);
                }
                at = m.u32(at + 0x2c0);
            }
        }
    }
    si_hrc::image::spline::spline_dispose(m, spline_at);
    let node = m.u32(node_at);
    m.set_u32(model_at, node);
    m.release(4);
    m.release(4);
    Some(1)
}

/// `SI_statusDisplayMessage (0x083cca94)`, `SI_mdlStatusMessages (0x0818129c)`,
/// `SI_edRelaxRelFromDelModel (0x080ff5f0)`, `sleep (0x080c4468`, its entry in the import
/// table) and `createModelFromRevolution (0x08195b7c)`: reached through [`Outside`] by
/// [`revolution_update`] (the last is ported only in part, [`create_model_from_revolution`]).
pub(super) const STATUS_DISPLAY_MESSAGE: Callee = Callee { name: "SI_statusDisplayMessage", addr: 0x083c_ca94 };
const MDL_STATUS_MESSAGES: Callee = Callee { name: "SI_mdlStatusMessages", addr: 0x0818_129c };
pub(super) const ED_RELAX_REL_FROM_DEL_MODEL: Callee = Callee { name: "SI_edRelaxRelFromDelModel", addr: 0x080f_f5f0 };
pub(super) const SLEEP: Callee = Callee { name: "sleep", addr: 0x080c_4468 };
const CREATE_MODEL_FROM_REVOLUTION: Callee = Callee { name: "createModelFromRevolution", addr: 0x0819_5b7c };

/// The format `SI_revolutionUpdate` hands to `sprintf` (0x0868d700, one `%d` for `SI_nbPolMax`
/// at 0x086d3920); read from the user's install at run time.
fn too_many_polygons(limit: i32) -> String {
    let fmt = si_rt::text(0x0868_d700);
    let at = fmt.find("%d").expect("the format holds one %d");
    format!("{}{}{}", &fmt[..at], limit, &fmt[at + 2..])
}

/// Brings a revolution's result up to date [soft 0x08196384 SI_revolutionUpdate] (the
/// update routine of a revolution relation; the float argument is not used). `node` is the
/// update node: +8 the model to update (the result of the revolution), +0x1c the relation
/// record (+8 the model revolved, +0xc a short that is 0 when the relation is gone, +0x30 the
/// axis model, +0x34 a short that is 0 when there is none), +0x24 the revolution record
/// (see [`create_patch_from_revolution_from_spline`]; its word at +0xc is the axis kind,
/// 3 and 4 name an axis model). Does nothing when any of the first four is missing.
///
/// - The model revolved a NURBS curve (kind 9) with fewer points than its minimum: a status
///   message, `sleep(1)`, `SI_mdlStatusMessages`, and the relation is relaxed from the model
///   (`SI_edRelaxRelFromDelModel(model, 0xfb)`); nothing else.
/// - With an axis kind 3 or 4 the axis model must exist, and when it is a spline (kind 6)
///   with fewer than 2 points (16-bit word at +0x10 of its definition) or a NURBS curve
///   (kind 9) with fewer than 2 (word at +4), a message says the relation is deleted and both
///   models are relaxed (spline: the revolved model first; curve: the axis first).
/// - A model revolved that has a definition then goes through `createModelFromRevolution`
///   (a hook: its answer is a short); 0 gives the "not performed" message (with
///   `SI_nbPolMax`) and `sleep(1)`; otherwise the result and each of its sons (through the
///   `+0x2c4` / `+0x2c0` chain) has `SI_modelTouchGeometry` called, its displayed node in the
///   scene's list found (`SI_findGlData`, kind 2) and `SI_updateGnFunc` run on it.
///
/// The original passes `createModelFromRevolution` the address of its local holding the
/// result node; as that cell is never null here the callee leaves it unchanged, and it is
/// read back unchanged.
pub fn revolution_update(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    let rel = m.u32(node + 0x1c);
    let mut axis = 0u32;
    if m.u16(rel + 0xc) == 0 {
        return;
    }
    let cell = m.local(4);
    let buffer = m.local(256);
    m.set_u32(cell, m.u32(node + 8));
    revolution_update_body(m, o, node, rel, &mut axis, cell, buffer);
    m.release(256);
    m.release(4);
}

fn revolution_update_body(m: &mut Mem, o: &mut dyn Outside, node: u32, rel: u32, axis: &mut u32, cell: u32, buffer: u32) {
    if m.u32(cell) == 0 {
        return;
    }
    let src = m.u32(rel + 8);
    if src == 0 {
        return;
    }
    let rev = m.u32(node + 0x24);
    if rev == 0 {
        return;
    }
    let relax = |m: &mut Mem, o: &mut dyn Outside, model: u32| {
        o.call(m, &ED_RELAX_REL_FROM_DEL_MODEL, &[model, 0xfb]);
    };
    if m.i32(src + 0x30) == 9 && {
        let def = m.u32(src + 0x48);
        m.i32(def + 4) < m.i32(def)
    } {
        o.call(m, &STATUS_DISPLAY_MESSAGE, &[0x0868_d680, 2]);
        o.call(m, &SLEEP, &[1]);
        o.call(m, &MDL_STATUS_MESSAGES, &[]);
        relax(m, o, src);
        return;
    }
    if m.u32(rev + 0xc).wrapping_sub(3) < 2 {
        if m.u16(rel + 0x34) == 0 {
            return;
        }
        *axis = m.u32(rel + 0x30);
        if *axis == 0 {
            return;
        }
        let kind = m.i32(*axis + 0x30);
        if kind == 6 {
            let def = m.u32(*axis + 0x48);
            if m.u16(def + 0x10) < 2 {
                o.call(m, &STATUS_DISPLAY_MESSAGE, &[0x0868_d6c0, 2]);
                relax(m, o, src);
                relax(m, o, *axis);
                return;
            }
        } else if kind == 9 {
            let def = m.u32(*axis + 0x48);
            if m.i32(def + 4) < 2 {
                o.call(m, &STATUS_DISPLAY_MESSAGE, &[0x0868_d6e0, 2]);
                relax(m, o, *axis);
                relax(m, o, src);
                return;
            }
        }
    }
    if m.u32(src + 0x48) == 0 {
        return;
    }
    let made = o.call(m, &CREATE_MODEL_FROM_REVOLUTION, &[src, *axis, cell, rev]) as u16;
    if made == 0 {
        // sprintf(buffer, text, SI_nbPolMax)
        let text = too_many_polygons(m.i32(0x086d_3920));
        for (i, b) in text.bytes().chain(std::iter::once(0)).enumerate() {
            m.set_u8(buffer + i as u32, b);
        }
        o.call(m, &STATUS_DISPLAY_MESSAGE, &[buffer, 3]);
        o.call(m, &SLEEP, &[1]);
        return;
    }
    let result = m.u32(cell);
    revolution_refresh(m, o, result);
    let mut at = m.u32(result + 0x2c4);
    while at != 0 {
        revolution_refresh(m, o, at);
        at = m.u32(at + 0x2c0);
    }
}

/// `SI_modelTouchGeometry(model)`, then the displayed node of the model in the scene's list
/// (`SI_findGlData(list, model, 2)`) goes through `SI_updateGnFunc`. (A model with no
/// displayed node is handed a null node, which the image faults on, as the original does.)
pub(super) fn revolution_refresh(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    o.call(m, &callee::SI_MODEL_TOUCH_GEOMETRY, &[model]);
    let list = current_graf_list(m);
    let shown = si_view::graflist::find_gl_data(m, list, Ptr32::new(model), 2);
    si_view::func::update_gn_func(m, &mut ViewOutside(o), shown);
}
