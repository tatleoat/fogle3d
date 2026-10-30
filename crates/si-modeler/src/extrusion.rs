// SPDX-License-Identifier: MIT
//! `extrusion.c` (first slice): the nodes an extrusion makes from a spline, a face, a NURBS
//! curve or a NURBS surface, the creation of the extruded geometry from them, and the
//! extrusion tool itself. Finding 0034.
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported (the
//! rest of `extrusion.c`, `nrb_convert.c`, the dialog and status layers) is called through
//! [`si_mem::Outside`], written `name (0xADDRESS)`.

use si_hrc::image::spline::face_allocate;
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::model_image::{defs, readjust, setup, state, tree, Callee as ModelCallee, Outside as ModelOutside};
use si_nurbs::image::Halt;
use si_math::types::Vector;
use si_types::geom::{Face, Patch, Spline};
use si_x87::F80;
use si_types::model::Model;

// ------------------------------------------------------------------------------ the rest of the program

/// `SI_nurbsConvertToFace (0x084e1450)`, `nrb_convert.c`.
pub const SI_NURBS_CONVERT_TO_FACE: Routine = Routine::new("SI_nurbsConvertToFace", 0x084e_1450);

/// `meshBevelExtrudeNurbsFace (0x08169c80)`, `extrusion.c` (another slice of the file).
pub const MESH_BEVEL_EXTRUDE_NURBS_FACE: Routine = Routine::new("meshBevelExtrudeNurbsFace", 0x0816_9c80);
/// `meshBevelExtrudeFace (0x081673fc)`.
pub const MESH_BEVEL_EXTRUDE_FACE: Routine = Routine::new("meshBevelExtrudeFace", 0x0816_73fc);
/// `meshBevelExtrudeSpline (0x08167360)`.
pub const MESH_BEVEL_EXTRUDE_SPLINE: Routine = Routine::new("meshBevelExtrudeSpline", 0x0816_7360);

/// The graph-list globals: `SI_procData` and `SI_grafLists`.
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The routines of `si-model` call out through their own trait; this hands them to ours.
pub(crate) struct ModelSide<'a>(pub(crate) &'a mut dyn Outside);
impl ModelOutside for ModelSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &ModelCallee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

/// The same for `si-view`.
struct ViewSide<'a>(&'a mut dyn Outside);
impl si_view::Outside for ViewSide<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(routine.name, routine.addr), args).eax
    }
}

/// The same for `si-spline`.
pub(crate) struct SplineSide<'a>(pub(crate) &'a mut dyn Outside);
impl si_spline::spline_image::Outside for SplineSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_spline::spline_image::Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

/// The display list of the current scene: `SI_grafLists + 0x24 * SI_procData[1]` (the
/// original loads both globals again at every use).
fn graf_list(m: &Mem) -> Ptr32<si_types::scene::GrafList> {
    Ptr32::new(m.u32(SI_GRAF_LISTS).wrapping_add((m.i32(m.u32(SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24)))
}

/// `SI_modelAllocate(name)` (0x08477cf4).
fn alloc_node(m: &mut Mem, name: u32) -> Ptr32<Model> {
    si_hrc::image::node::model_allocate(m, name)
}

/// `SI_modelAddDefinition(node, kind, definition)` (0x08467038).
fn add_def(m: &mut Mem, node: Ptr32<Model>, kind: i32, definition: u32) {
    setup::add_definition(m, node, kind, definition);
}

/// `SI_modelSubstituteIDefinition(node, number, definition)` (0x08467158).
fn substitute(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, number: i16, definition: u32) {
    defs::substitute_idefinition(m, &mut ModelSide(o), node, number, definition);
}

/// The step the three hierarchy builders share for a node that is made new in the display
/// list: `SI_graflistAddData(list, node, 2, 0)`, `SI_findGlData(list, father, 2)` and
/// `SI_modelCreateParent(found, added, 0)`.
fn add_to_list(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, father: Ptr32<Model>) {
    let list = graf_list(m);
    let added = si_view::graflist::graflist_add_data(m, &mut ViewSide(o), list, Ptr32::new(node.addr()), 2, 0);
    let list = graf_list(m);
    let shown = si_view::graflist::find_gl_data(m, list, Ptr32::new(father.addr()), 2);
    tree::create_parent(m, &mut ModelSide(o), shown.addr(), added.addr(), 0);
}

/// Whether a node's working definition is not its first stored one, or its box is not valid:
/// the test before `SI_patchUpdateSurface` in the builders.
fn needs_update(m: &Mem, node: Ptr32<Model>) -> bool {
    let first = m.u32(m[node].defs.addr());
    m[node].definition.addr() != first || m[node].bbox_valid != 0
}


/// The node names the builders hand to `SI_modelAllocate` and `SI_modelChangeName`: strings of the binary's data
/// at 0x08689ebc .. 0x08689f5d, which the image does not hold unless this puts them there.
const NAMES: &[(u32, &[u8])] = &[
    (0x08689ebc, b"bottom"),
    (0x08689ec3, b"top"),
    (0x08689ec7, b"extru"),
    (0x08689ecd, b"revol"),
    (0x08689ed3, b"null"),
    (0x08689ed8, b"bottom"),
    (0x08689edf, b"bottom"),
    (0x08689ee6, b"top"),
    (0x08689eea, b"top"),
    (0x08689eee, b"extru"),
    (0x08689ef4, b"revol"),
    (0x08689efa, b"extru"),
    (0x08689f00, b"revol"),
    (0x08689f06, b"extru"),
    (0x08689f0c, b"revol"),
    (0x08689f12, b"extru"),
    (0x08689f18, b"revol"),
    (0x08689f1e, b"null"),
    (0x08689f23, b"bottom"),
    (0x08689f2a, b"bottom"),
    (0x08689f31, b"top"),
    (0x08689f35, b"top"),
    (0x08689f39, b"extru"),
    (0x08689f3f, b"revol"),
    (0x08689f45, b"extru"),
    (0x08689f4b, b"revol"),
    (0x08689f51, b"extru"),
    (0x08689f57, b"revol"),
];

/// Puts [`NAMES`] into the image (the program's loader does it in the original).
pub fn store_names(m: &mut Mem) {
    for (at, text) in NAMES {
        m.put_cstr(*at, text);
    }
}

// ------------------------------------------------------------------------------ the nodes of an extrusion

/// Makes the nodes of a NURBS surface extruded from a curve (0x08156c84)
/// si_createNurbsHierarchyFromNurbsCrv, not checked yet: the same shape as
/// [`si_model::model_image::tree::create_patch_hierarchy_from_spline`] for a surface. `srf`
/// is the surface; `model_at` the address of a `__SI_Model *` that is read and written;
/// `update` replaces the definitions of nodes that exist; `caps` asks for a "bottom" and a
/// "top" node, each with a face made (`SI_nurbsConvertToFace (0x084e1450)`, a hook) from the
/// surface's first and last isoparametric curve (U at +0x10 and +0x18, or, with `extrude`,
/// V at +0x34 and +0x3c, as doubles); `extrude` names the nodes "extru" instead of "revol".
/// With `caps` the answer stored at `model_at` is the node itself, otherwise the node that
/// holds the surface. A null stored definition array reads address 0, and the image faults.
pub fn create_nurbs_hierarchy_from_nurbs_crv(m: &mut Mem, o: &mut dyn Outside, srf: u32, model_at: u32, update: i16, caps: i16, extrude: i16) -> Result<(), Halt> {
    let top_node: Ptr32<Model> = Ptr32::new(m.u32(model_at));
    let mut bottom: Ptr32<Model> = Ptr32::NULL;
    let mut top: Ptr32<Model> = Ptr32::NULL;
    let mut replace = false;
    if caps != 0 {
        if m[top_node].son.is_null() {
            replace = update != 0;
        }
        // the first cap: the curve at the start of U (reversed), or at the start of V
        let curve = if extrude != 0 {
            let t = m.f64(srf + 0x34);
            si_nurbs::image::srf_extract_curve(m, srf, t, 1, 0)?
        } else {
            let t = m.f64(srf + 0x10);
            let c = si_nurbs::image::srf_extract_curve(m, srf, t, 0, 0)?;
            si_nurbs::image::crv_inverse(m, o, c);
            c
        };
        let face = face_allocate(m);
        o.call(m, SI_NURBS_CONVERT_TO_FACE, &[curve, face.addr()]);
        if update == 0 || replace {
            bottom = alloc_node(m, 0x0868_9ebc);
        } else {
            bottom = m[top_node].son;
            top = m[m[bottom].brother].brother;
        }
        if update != 0 && !replace {
            substitute(m, o, bottom, 0, face.addr());
        } else {
            add_def(m, bottom, 2, face.addr());
        }
        // the second cap
        let curve = if extrude != 0 {
            let t = m.f64(srf + 0x3c);
            let c = si_nurbs::image::srf_extract_curve(m, srf, t, 1, 0)?;
            si_nurbs::image::crv_inverse(m, o, c);
            c
        } else {
            let t = m.f64(srf + 0x18);
            si_nurbs::image::srf_extract_curve(m, srf, t, 0, 0)?
        };
        let face = face_allocate(m);
        o.call(m, SI_NURBS_CONVERT_TO_FACE, &[curve, face.addr()]);
        if update == 0 || replace {
            top = alloc_node(m, 0x0868_9ec3);
        }
        if update != 0 && !replace {
            substitute(m, o, top, 0, face.addr());
        } else {
            add_def(m, top, 2, face.addr());
        }
        if update == 0 {
            m[top_node].son = bottom;
            m[bottom].father = top_node;
            m[top].father = top_node;
        }
    }
    // the node that holds the surface
    let holder: Ptr32<Model>;
    if update == 0 || replace {
        holder = alloc_node(m, if extrude != 0 { 0x0868_9ec7 } else { 0x0868_9ecd });
    } else if caps != 0 {
        holder = m[bottom].brother;
    } else if m[top_node].kind != 0 {
        holder = top_node;
    } else {
        holder = m[m[top_node].son].brother;
    }
    if update != 0 && !replace {
        if needs_update(m, holder) {
            let copy = si_nurbs::image::srf_copy(m, o, srf);
            substitute(m, o, holder, 0, copy);
        } else {
            substitute(m, o, holder, 0, srf);
        }
    } else {
        add_def(m, holder, 10, srf);
    }
    if replace {
        readjust::delete_definition(m, &mut ModelSide(o), top_node, 0);
        let list = graf_list(m);
        let shown = si_view::graflist::find_gl_data(m, list, Ptr32::new(top_node.addr()), 2);
        for node in [bottom, holder, top] {
            let list = graf_list(m);
            let added = si_view::graflist::graflist_add_data(m, &mut ViewSide(o), list, Ptr32::new(node.addr()), 2, 0);
            tree::create_parent(m, &mut ModelSide(o), shown.addr(), added.addr(), 0);
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
            defs::change_name(m, top_node, 0x0868_9ed3);
        }
    }
    m.set_u32(model_at, result.addr());
    Ok(())
}

/// A face made of one outline and the holes of `source`, each a spline extracted from the
/// patch at `patches[i]`: the cap faces of [`create_patch_hierarchy_from_face`]. `top`
/// extracts at the patch's last segment instead of 0, and the outline is reversed only
/// for the first cap. (The two caps are written out in the original; the order of the
/// calls is kept here.)
fn cap_face(m: &mut Mem, o: &mut dyn Outside, patches: u32, nb_holes: i16, top: bool) -> Ptr32<Face> {
    let patch0: Ptr32<Patch> = Ptr32::new(m.u32(patches));
    let at = |m: &mut Mem, patch: Ptr32<Patch>| -> f32 {
        if !top {
            return 0.0;
        }
        let (segments, _step) = si_spline::eval::nb_segment_step(m[patch].u_type, m[patch].nb_u as i32, m[patch].u_closed);
        segments as f32 // (`fild` of the count, stored as a float)
    };
    let t = at(m, patch0);
    let outline = si_spline::patch_image::patch_extract_spline(m, patch0, 1, t);
    if !top {
        si_spline::convert_image::spline_inverse(m, &mut SplineSide(o), outline);
    }
    let face = face_allocate(m);
    let target: Ptr32<Spline> = Ptr32::new(face.addr() + 4);
    si_spline::spline_image::spline_init(m, target);
    si_spline::intersect_image::spline_transfer(m, outline, target);
    m[face].nb_holes = nb_holes;
    let holes = m.si_malloc((nb_holes as i32).wrapping_mul(0x74) as u32);
    m[face].holes = Ptr32::new(holes);
    let mut i: i16 = 1;
    while i <= m[face].nb_holes {
        let patch: Ptr32<Patch> = Ptr32::new(m.u32(patches + 4 * i as i32 as u32));
        let t = at(m, patch);
        let sp = si_spline::patch_image::patch_extract_spline(m, patch, 1, t);
        if !top {
            si_spline::convert_image::spline_inverse(m, &mut SplineSide(o), sp);
        }
        let dst = m[face].holes.addr().wrapping_add((i as i32 * 0x74 - 0x74) as u32);
        si_spline::spline_image::spline_init(m, Ptr32::new(dst));
        si_spline::intersect_image::spline_transfer(m, sp, Ptr32::new(dst));
        i = i.wrapping_add(1);
    }
    face
}

/// Makes the nodes of a patch extruded from a face (0x08157088)
/// si_createPatchHierarchyFromFace, not checked yet: `patches` is an array of patches (the first for the
/// face's outline, then one for each hole, `face`'s `nb_holes` of them), `model_at` the
/// address of the `__SI_Model *` that is read and written. With `caps` a "bottom" and a
/// "top" node hold faces made of the first and last splines of the patches ([`cap_face`]),
/// with `update` the existing sons are used and new nodes are made (and hung under the
/// node through the display list) where they run out. Without `caps` and without holes the
/// answer is the son of the node, its father cleared.
pub fn create_patch_hierarchy_from_face(m: &mut Mem, o: &mut dyn Outside, patches: u32, model_at: u32, face: Ptr32<Face>, update: i16, caps: i16, extrude: i16) {
    let top_node: Ptr32<Model> = Ptr32::new(m.u32(model_at));
    let mut top_cap: Ptr32<Model> = Ptr32::NULL;
    let mut bottom: Ptr32<Model> = Ptr32::NULL;
    let mut replace = false;
    let nb = m[face].nb_holes;
    let patch_at = |m: &Mem, i: i16| -> Ptr32<Patch> { Ptr32::new(m.u32(patches.wrapping_add(4u32.wrapping_mul(i as i32 as u32)))) };
    if caps != 0 {
        let first = cap_face(m, o, patches, nb, false);
        if update == 0 {
            bottom = alloc_node(m, if extrude != 0 { 0x0868_9ed8 } else { 0x0868_9edf });
        } else {
            bottom = m[top_node].son;
            let mut next = m[bottom].brother;
            while !next.is_null() {
                top_cap = next;
                next = m[next].brother;
            }
        }
        if update == 0 {
            add_def(m, bottom, 2, first.addr());
        } else {
            substitute(m, o, bottom, 0, first.addr());
        }
        let second = cap_face(m, o, patches, nb, true);
        if update == 0 {
            top_cap = alloc_node(m, if extrude != 0 { 0x0868_9ee6 } else { 0x0868_9eea });
            add_def(m, top_cap, 2, second.addr());
        } else {
            substitute(m, o, top_cap, 0, second.addr());
        }
        if update == 0 {
            m[top_node].son = bottom;
            m[bottom].father = top_node;
            m[top_cap].father = top_node;
        }
    }
    // the node that holds the first patch
    let mut holder: Ptr32<Model>;
    if update == 0 {
        holder = alloc_node(m, if extrude != 0 { 0x0868_9eee } else { 0x0868_9ef4 });
    } else {
        if caps != 0 {
            holder = m[bottom].brother;
        } else if nb != 0 || m[top_node].kind == 0 {
            holder = m[top_node].son;
        } else {
            holder = top_node;
        }
        if holder.is_null() {
            holder = alloc_node(m, if extrude != 0 { 0x0868_9efa } else { 0x0868_9f00 });
            replace = true;
        }
    }
    let patch0 = patch_at(m, 0);
    if update == 0 || replace {
        add_def(m, holder, 1, patch0.addr());
    } else {
        if needs_update(m, holder) {
            si_spline::patch_image::patch_update_surface(m, patch0);
        }
        substitute(m, o, holder, 0, patch0.addr());
    }
    if replace {
        state::copy_transformation(m, &mut ModelSide(o), top_node, holder);
        readjust::delete_definition(m, &mut ModelSide(o), top_node, 0);
        add_to_list(m, o, holder, top_node);
    }
    if update == 0 || replace {
        m[holder].father = top_node;
        if caps == 0 {
            m[top_node].son = holder;
        } else {
            m[bottom].brother = holder;
        }
    }
    replace = false;
    let mut previous = holder;
    let mut i: i16 = 1;
    while i <= m[face].nb_holes {
        let mut node: Ptr32<Model>;
        if update == 0 {
            node = alloc_node(m, if extrude != 0 { 0x0868_9f06 } else { 0x0868_9f0c });
        } else {
            node = m[previous].brother;
        }
        if node.is_null() || node == top_cap {
            node = alloc_node(m, if extrude != 0 { 0x0868_9f12 } else { 0x0868_9f18 });
            replace = true;
        }
        let patch = patch_at(m, i);
        if update == 0 || replace {
            add_def(m, node, 1, patch.addr());
        } else {
            if needs_update(m, node) {
                si_spline::patch_image::patch_update_surface(m, patch);
            }
            substitute(m, o, node, 0, patch.addr());
        }
        if replace {
            state::copy_transformation(m, &mut ModelSide(o), top_node, node);
            add_to_list(m, o, node, top_node);
            if !top_cap.is_null() {
                m[top_cap].brother = Ptr32::NULL;
            }
        }
        if update == 0 || replace {
            m[node].father = top_node;
            m[previous].brother = node;
        }
        previous = node;
        i = i.wrapping_add(1);
    }
    if caps != 0 && (update == 0 || replace) {
        m[previous].brother = top_cap;
    }
    let mut result = top_node;
    if update == 0 {
        if caps == 0 && m[face].nb_holes == 0 {
            result = m[top_node].son;
            m[result].father = Ptr32::NULL;
        } else {
            defs::change_name(m, top_node, 0x0868_9f1e);
        }
    }
    m.set_u32(model_at, result.addr());
}

/// A face made of NURBS curves, for one cap of [`create_nurbs_srf_hierarchy_from_nurbs_face`]:
/// `nb_holes` curves (the face's `nb_holes` field is the number of curves), the curve `i`
/// extracted from `srfs[i]` at the parameter stored at `srf + at` (`dir` 0: U, 1: V) and
/// reversed when `inverse`.
fn nurbs_cap_face(m: &mut Mem, o: &mut dyn Outside, srfs: u32, nb: i16, at: u32, dir: i32, inverse: bool) -> Result<Ptr32<Face>, Halt> {
    let face = face_allocate(m);
    m[face].is_spline = 0;
    m[face].nb_holes = nb;
    let curves = m.si_calloc(nb as i32 as u32, 100);
    m[face].curves = Ptr32::new(curves);
    let mut i: i16 = 0;
    while i < m[face].nb_holes {
        let srf = m.u32(srfs.wrapping_add(4u32.wrapping_mul(i as i32 as u32)));
        let t = m.f64(srf + at);
        let crv = si_nurbs::image::srf_extract_curve(m, srf, t, dir, 0)?;
        if inverse {
            si_nurbs::image::crv_inverse(m, o, crv);
        }
        let dst = m[face].curves.addr().wrapping_add((i as i32 * 100) as u32);
        si_hrc::image::nurbs::nurbs_crv_init(m, dst, crv);
        i = i.wrapping_add(1);
    }
    Ok(face)
}

/// Makes the nodes of a NURBS surface extruded from a face of NURBS curves (0x0815775c)
/// si_createNurbsSrfHierarchyFromNurbsFace, not checked yet: as [`create_patch_hierarchy_from_face`], with
/// `srfs` an array of surfaces (one for each curve of `face`) and the caps faces of NURBS
/// curves ([`nurbs_cap_face`]) taken from the surfaces' U edges (`extrude` 0, at +0x10, then
/// +0x18 reversed) or V edges (at +0x34 reversed, then +0x3c). A node that gets no son of
/// its own is named "null" when it has no geometry kind.
pub fn create_nurbs_srf_hierarchy_from_nurbs_face(m: &mut Mem, o: &mut dyn Outside, srfs: u32, model_at: u32, face: Ptr32<Face>, update: i16, caps: i16, extrude: i16) -> Result<(), Halt> {
    let top_node: Ptr32<Model> = Ptr32::new(m.u32(model_at));
    let mut top_cap: Ptr32<Model> = Ptr32::NULL;
    let mut bottom: Ptr32<Model> = Ptr32::NULL;
    let nb = m[face].nb_holes;
    if caps != 0 {
        let first = if extrude == 0 { nurbs_cap_face(m, o, srfs, nb, 0x10, 0, false)? } else { nurbs_cap_face(m, o, srfs, nb, 0x34, 1, true)? };
        if update == 0 {
            bottom = alloc_node(m, if extrude != 0 { 0x0868_9f23 } else { 0x0868_9f2a });
        } else {
            bottom = m[top_node].son;
            let mut next = m[bottom].brother;
            while !next.is_null() {
                top_cap = next;
                next = m[next].brother;
            }
        }
        if update == 0 {
            add_def(m, bottom, 2, first.addr());
        } else {
            substitute(m, o, bottom, 0, first.addr());
        }
        let second = if extrude == 0 { nurbs_cap_face(m, o, srfs, nb, 0x18, 0, true)? } else { nurbs_cap_face(m, o, srfs, nb, 0x3c, 1, false)? };
        if update == 0 {
            top_cap = alloc_node(m, if extrude != 0 { 0x0868_9f31 } else { 0x0868_9f35 });
            add_def(m, top_cap, 2, second.addr());
        } else {
            substitute(m, o, top_cap, 0, second.addr());
        }
        if update == 0 {
            m[top_node].son = bottom;
            m[bottom].father = top_node;
            m[top_cap].father = top_node;
        }
    }
    let mut node: Ptr32<Model> = Ptr32::NULL;
    let mut last: Ptr32<Model> = Ptr32::NULL;
    let mut replace = false;
    let mut replace_after = false;
    let mut i: i16 = 0;
    while 0 < m[face].nb_holes {
        let previous = node;
        if update == 0 {
            node = alloc_node(m, if extrude != 0 { 0x0868_9f39 } else { 0x0868_9f3f });
        } else if i != 0 {
            node = m[node].brother;
        } else {
            node = if caps == 0 {
                if 1 < m[face].nb_holes { m[top_node].son } else { top_node }
            } else {
                m[bottom].brother
            };
            if node.is_null() {
                node = alloc_node(m, if extrude != 0 { 0x0868_9f45 } else { 0x0868_9f4b });
                replace = true;
            }
        }
        if node.is_null() || node == top_cap {
            node = alloc_node(m, if extrude != 0 { 0x0868_9f51 } else { 0x0868_9f57 });
            replace = true;
        }
        let srf = m.u32(srfs.wrapping_add(4u32.wrapping_mul(i as i32 as u32)));
        if update == 0 || replace {
            add_def(m, node, 10, srf);
        } else {
            substitute(m, o, node, 0, srf);
        }
        if replace {
            state::copy_transformation(m, &mut ModelSide(o), top_node, node);
            if i == 0 {
                readjust::delete_definition(m, &mut ModelSide(o), top_node, 0);
            }
            add_to_list(m, o, node, top_node);
            if !top_cap.is_null() {
                m[top_cap].brother = Ptr32::NULL;
            }
        }
        if update == 0 || replace {
            if i == 0 {
                m[node].father = top_node;
                if caps == 0 {
                    m[top_node].son = node;
                } else {
                    m[bottom].brother = node;
                }
                replace = false;
            } else {
                m[node].father = top_node;
                m[previous].brother = node;
            }
        }
        last = node;
        i = i.wrapping_add(1);
        replace_after = replace;
        if !(i < m[face].nb_holes) {
            break;
        }
    }
    if caps != 0 && (update == 0 || replace_after) {
        m[last].brother = top_cap;
    }
    let mut result = top_node;
    if update == 0 && caps == 0 && m[face].nb_holes < 2 {
        result = m[top_node].son;
        m[result].father = Ptr32::NULL;
    }
    if !result.is_null() && m[result].kind == 0 {
        defs::change_name(m, result, 0x0868_9f5d);
    }
    m.set_u32(model_at, result.addr());
    Ok(())
}

// ------------------------------------------------------------------------------ the extrusion data

/// The fields of an extrusion record (`__SI_ExtrusionData`, 0x38 bytes, relation 250,
/// (0x08156830 SI_extrusionDataAllocate); `si-ascii::reldata` names the keywords) as
/// the creators read them: the values are read once at the start, a float through the
/// x87 unit (a signalling NaN comes out quiet).
#[derive(Clone, Copy)]
struct Extrusion {
    close: i16,
    depth: f32,
    steps: i32,
    subdivision: i32,
    axis: i32,
    bevel: u16,
    nb_bevel: i16,
    radius: f32,
    top: i16,
    side: i16,
    bottom: i16,
}

/// Disposes a face the way the creators do, through a stack slot holding its pointer.
pub(crate) fn dispose_face_at(m: &mut Mem, face: u32) {
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, face);
    si_model::mesh_image::face::face_dispose(m, slot);
    m.set_sp(sp);
}

fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

impl Extrusion {
    fn read(m: &Mem, at: u32) -> Extrusion {
        Extrusion {
            close: m.i16(at),
            depth: through_x87(m.f32(at + 4)),
            steps: m.i32(at + 8),
            subdivision: m.i32(at + 0xc),
            axis: m.i32(at + 0x10),
            bevel: m.u16(at + 0x1c),
            nb_bevel: m.i16(at + 0x1e),
            radius: through_x87(m.f32(at + 0x20)),
            top: m.i16(at + 0x24),
            side: m.i16(at + 0x26),
            bottom: m.i16(at + 0x28),
        }
    }

    /// The short the creators hand to the mesh routines as the subdivision (the low 16 bits).
    fn subdivision16(&self) -> i16 {
        self.subdivision as i16
    }
}

/// The step of the extrusion along its axis (0: x, 1: y, 2: z): the depth divided by the
/// step count in the x87 unit, rounded to float when stored. For any other axis the original
/// leaves the vector unwritten (stack contents); here it is zero.
fn axis_step(e: &Extrusion) -> Vector {
    let step = (F80::from_f32(e.depth) / F80::from_i32(e.steps)).to_f32();
    match e.axis {
        0 => Vector::new(step, 0.0, 0.0),
        1 => Vector::new(0.0, step, 0.0),
        2 => Vector::new(0.0, 0.0, step),
        _ => Vector::new(0.0, 0.0, 0.0),
    }
}

/// `SI_matrixTranslation(&vector, matrix)` (0x08517e58) into a new local of 64 bytes (the
/// caller releases it).
fn translation_local(m: &mut Mem, v: Vector) -> u32 {
    let at = m.local(64);
    let t = si_math::algebra::matrix_translation(&v);
    for (r, row) in t.iter().enumerate() {
        for (c, x) in row.iter().enumerate() {
            m.set_f32(at + 16 * r as u32 + 4 * c as u32, *x);
        }
    }
    at
}

/// A `short` as the 32-bit word a call pushes (`movsx`).
fn w16(v: i16) -> u32 {
    v as i32 as u32
}

/// Adds the extruded mesh to the node (kind 4) or, with `update`, makes it the node's first
/// definition.
fn put_mesh(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, mesh: u32, update: i16) {
    if update == 0 {
        add_def(m, model, 4, mesh);
    } else {
        substitute(m, o, model, 0, mesh);
    }
}

/// The ten arguments the bevelling routines take: the face or spline, the subdivision, the
/// depth, the step count, the radius, the bevel count, the top, side and bottom flags and the axis.
fn bevel_args(first: u32, e: &Extrusion) -> [u32; 10] {
    [first, w16(e.subdivision16()), e.depth.to_bits(), e.steps as u32, e.radius.to_bits(), w16(e.nb_bevel), w16(e.top), w16(e.side), w16(e.bottom), e.axis as u32]
}

/// Makes the mesh of a NURBS curve extruded [soft 0x08157d2c createMeshFromNurbsExtrusion] (checked, finding 0034):
/// the curve becomes a face (`SI_nurbsConvertToFace (0x084e1450)`), and `meshExtrudeNurbsFace
/// (0x08162b8c)` or, for a bevelled extrusion of a closed curve, `meshBevelExtrudeNurbsFace
/// (0x08169c80)` makes the mesh, which is added to `model` as its kind 4 definition, or
/// substituted for its first with `update`. Answers 0 when no mesh came of a curve that has
/// points (and is closed, with a bevel), else 1.
pub fn create_mesh_from_nurbs_extrusion(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, curve: u32, data: u32, update: i16) -> u16 {
    let e = Extrusion::read(m, data);
    let mut answer: u16 = 1;
    let mut bevel = e.bevel;
    let face = face_allocate(m).addr();
    o.call(m, SI_NURBS_CONVERT_TO_FACE, &[curve, face]);
    if bevel != 0 && m.i16(curve + 0x20) == 0 {
        bevel = 0;
    }
    let mesh;
    if bevel == 0 {
        let sp = m.sp();
        let matrix = translation_local(m, axis_step(&e));
        mesh = crate::extrusion2::mesh_extrude_nurbs_face(m, o, face, e.subdivision16(), matrix, e.steps, e.close);
        m.set_sp(sp);
        if mesh == 0 {
            if m.i32(curve + 4) != 0 {
                answer = 0;
            }
            dispose_face_at(m, face);
            return answer;
        }
    } else {
        mesh = o.call(m, MESH_BEVEL_EXTRUDE_NURBS_FACE, &bevel_args(face, &e)).eax;
        if mesh == 0 {
            if m.i32(curve + 4) != 0 && m.i16(curve + 0x20) != 0 {
                answer = 0;
            }
            dispose_face_at(m, face);
            return answer;
        }
    }
    put_mesh(m, o, model, mesh, update);
    dispose_face_at(m, face);
    1
}

/// Shared tail of the mesh creators: the mesh is added or substituted, the answer is 1.
fn finish_mesh(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, mesh: u32, update: i16) -> bool {
    put_mesh(m, o, model, mesh, update);
    true
}

/// `SI_faceConvertToLinear(face, step, 1)` through the model routines' outside trait.
fn convert_to_linear(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16) -> u32 {
    si_model::mesh_image::face::face_convert_to_linear(m, &mut ModelSide(o), face, step, 1)
}

/// Makes the mesh of a spline or a face extruded [soft 0x08157f60 createMeshFromExtrusionSimple] (checked, finding 0034;
/// `is_spline` is its fifth argument): bevelled through `meshBevelExtrudeNurbsFace
/// (0x08169c80)`, `meshBevelExtrudeFace (0x081673fc)` or `meshBevelExtrudeSpline (0x08167360)`,
/// else through `meshExtrudeFace (0x08162600)` or `meshExtrudeSpline (0x08162598)`. When no
/// mesh comes the shape is converted to linear to tell whether it was degenerate (answer true)
/// or a real failure (false).
pub fn create_mesh_from_extrusion_simple(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, spline: u32, face: u32, data: u32, is_spline: i16, update: i16) -> bool {
    let e = Extrusion::read(m, data);
    let step16 = e.subdivision16();
    let mut bevel = e.bevel;
    if bevel != 0 {
        if is_spline != 0 && m.i16(spline + 8) == 0 {
            bevel = 0;
        }
        if bevel != 0 {
            if is_spline == 0 {
                if m.i16(face + 0x84) == 0 {
                    let mesh = o.call(m, MESH_BEVEL_EXTRUDE_NURBS_FACE, &bevel_args(face, &e)).eax;
                    if mesh != 0 {
                        return finish_mesh(m, o, model, mesh, update);
                    }
                    return true;
                }
                let mesh = o.call(m, MESH_BEVEL_EXTRUDE_FACE, &bevel_args(face, &e)).eax;
                if mesh != 0 {
                    return finish_mesh(m, o, model, mesh, update);
                }
                let lin = convert_to_linear(m, o, face, step16);
                if lin == 0 {
                    return true;
                }
                let answer = m.u16(lin + 0x14) < 3;
                dispose_face_at(m, lin);
                return answer;
            }
            let mesh = o.call(m, MESH_BEVEL_EXTRUDE_SPLINE, &bevel_args(spline, &e)).eax;
            if mesh == 0 {
                if m.i16(spline + 0x10) == 0 {
                    return true;
                }
                return m.i16(spline + 8) == 0;
            }
            return finish_mesh(m, o, model, mesh, update);
        }
    }
    let sp = m.sp();
    let matrix = translation_local(m, axis_step(&e));
    let mesh;
    if is_spline == 0 {
        mesh = crate::extrusion2::mesh_extrude_face(m, o, face, step16, matrix, e.steps, e.close);
        m.set_sp(sp);
        if mesh == 0 {
            return m.i16(face + 0x14) == 0;
        }
    } else {
        mesh = crate::extrusion2::mesh_extrude_spline(m, o, spline, step16, matrix, e.steps, e.close);
        m.set_sp(sp);
        if mesh == 0 {
            let f = face_allocate(m).addr();
            if f == 0 {
                return true;
            }
            si_spline::intersect_image::spline_transfer(m, Ptr32::new(spline), Ptr32::new(f + 4));
            let lin = convert_to_linear(m, o, f, step16);
            let mut answer = true;
            if lin != 0 {
                answer = m.i16(lin + 0x14) == 0;
                dispose_face_at(m, lin);
            }
            dispose_face_at(m, f);
            return answer;
        }
    }
    finish_mesh(m, o, model, mesh, update)
}

/// Makes the mesh of a NURBS face extruded (`createMeshFromNurbsFaceExtrusion (0x081582ec)`; checked on the
/// no-bevel paths and the bevel path with a mesh, not the bevel path without one, so partial):
/// `meshExtrudeNurbsFace (0x08162b8c)`, or with a bevel `meshBevelExtrudeNurbsFace (0x08169c80)`.
/// With no mesh the answer is whether the first curve has no points (or, bevelled, whether the
/// linear face's first curve has fewer than three).
pub fn create_mesh_from_nurbs_face_extrusion(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, face: u32, data: u32, update: i16) -> bool {
    let e = Extrusion::read(m, data);
    let mesh;
    if m.i16(data + 0x1c) == 0 {
        let sp = m.sp();
        let matrix = translation_local(m, axis_step(&e));
        mesh = crate::extrusion2::mesh_extrude_nurbs_face(m, o, face, e.subdivision16(), matrix, e.steps, e.close);
        m.set_sp(sp);
        if mesh == 0 {
            let curves = m.u32(face + 0x80);
            return m.i32(curves + 4) == 0;
        }
    } else {
        mesh = o.call(m, MESH_BEVEL_EXTRUDE_NURBS_FACE, &bevel_args(face, &e)).eax;
        if mesh == 0 {
            let lin = convert_to_linear(m, o, face, e.subdivision16());
            if lin == 0 {
                return true;
            }
            let curves = m.u32(lin + 0x80);
            let n = m.i32(curves + 4);
            dispose_face_at(m, lin);
            return n < 3;
        }
    }
    finish_mesh(m, o, model, mesh, update)
}
