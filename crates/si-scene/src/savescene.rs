// SPDX-License-Identifier: MIT
//! "Save Scene" from start to end (`dbloadsave.c`, with the routines of `edtools.c`,
//! `elemdict.c`, `scn2db.c` and `dictout_scn.c` it calls).
//!
//! [`save_scene`] is the program's `SI_dbSaveScene`: the elements take their display state
//! back from the scene graph, the scene gets its name and version, every element that has
//! something to save is written ([`crate::dictout`]), and the description is written last.

use crate::animrel::find_elem_by_data;
use crate::calls::{Arg, Channel, Message, Unported};
use crate::dict::{data_ptr, ElemId};
use crate::dictout::{self, Host, NAME_CANCEL_ALL};
use crate::elemdict;
use crate::elemrel;
use crate::elemsort::NameOrder;
use crate::init;
use crate::load::Scene;
use crate::object::Object;
use crate::save;
use crate::scnrel;
use si_db::{DbInfo, ListNode};
use si_types::dict::ELEM_VALID;
use si_types::Ptr32;

fn or_null(s: &Option<Vec<u8>>) -> &[u8] {
    s.as_deref().unwrap_or(si_db::NULL_TEXT)
}

/// The elements take back from their nodes in the scene graph what the user may have
/// changed there [soft 0x08104acc SI_edGetElementsGrafNodeInfo]. In every chapter but
/// AUDIO, for each element in use that has a node:
///
/// - selection (+0x24): the node's own (+8), unless bit 0x1000 of the node's flag word is
///   set; then 1 for bit 0x2000, else 3 for bit 0x4000, else 2 for bit 0x8000, else 0;
/// - display type (+0x20): the node's (+0x0c);
/// - wire colour (+0x1c): the node's 16-bit colour (+0x12), sign-extended.
///
/// And every root of MODELS that has an object (whatever its status) gets nine numbers
/// behind +0x3c, allocated if need be: its node's scaling, rotation and translation. That
/// is what the description writer prints as `SRT` and then frees. (The original counts that
/// allocation in a global counter, which is not reproduced.)
pub fn get_elements_graf_node_info(sc: &mut Scene) {
    let cid = sc.program.cid;
    for chap in 0..sc.dict.ed.lists.len() as i32 {
        if chap == cid.audio {
            continue;
        }
        for i in 0..sc.dict.ed.lists[chap as usize].elems.len() {
            let e = sc.dict.ed.lists[chap as usize].elems[i].rec;
            if e.status == ELEM_VALID && !e.aux_data.is_null() {
                if let Some(n) = sc.graf.get(e.aux_data.0).copied() {
                    let sel = if n.flags & 0x1000 == 0 {
                        n.selection
                    } else if n.flags & 0x2000 != 0 {
                        1
                    } else if n.flags & 0x4000 != 0 {
                        3
                    } else if n.flags & 0x8000 != 0 {
                        2
                    } else {
                        0
                    };
                    let r = &mut sc.dict.ed.lists[chap as usize].elems[i].rec;
                    r.selection = sel;
                    r.wire_type = n.wire_type;
                    r.wire_color = n.wire_color as i32;
                }
            }
            if chap == cid.model && !e.data.is_null() && e.hier_level == 0 {
                let state = match sc.objects.get(e.data.0) {
                    Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).and_then(|t| {
                        t.state[*node].or_else(|| t.node(*node).map(|n| [n.scaling[0], n.scaling[1], n.scaling[2], n.rotation[0], n.rotation[1], n.rotation[2], n.translation[0], n.translation[1], n.translation[2]]))
                    }),
                    // a model of another package (a cube put in place of a missing file):
                    // its numbers are not here; what the loader handed over stands in
                    _ => sc.root_state.get(&ElemId::new(chap, i as i32)).copied(),
                };
                let Some(state) = state else { continue };
                if e.root_srt.is_null() {
                    let h = sc.srt.alloc(state);
                    sc.dict.ed.lists[chap as usize].elems[i].rec.root_srt = Ptr32::new(h);
                } else if let Some(v) = sc.srt.get_mut(e.root_srt.0) {
                    *v = state;
                }
            }
        }
    }
}

/// Marks the elements whose node in the scene graph is selected
/// [soft 0x0810479c SI_edGetSelectedElements]: in every chapter but AUDIO, each element in
/// use gets as its selected word (+0x42) the selection state of a node: its own, or for a
/// shape, a cluster or a lattice that of the model it belongs to (relations 200, 700, and
/// 210 or else 215, backward side); 0 when that element has no node. An element whose model
/// is not found keeps its word. With `from_flags`, a node whose flag word has bit 0x1000 is
/// read as in [`get_elements_graf_node_info`].
pub fn get_selected_elements(sc: &mut Scene, from_flags: bool) {
    let cid = sc.program.cid;
    for chap in 0..sc.dict.ed.lists.len() as i32 {
        if chap == cid.audio {
            continue;
        }
        for i in 0..sc.dict.ed.lists[chap as usize].elems.len() {
            if sc.dict.ed.lists[chap as usize].elems[i].rec.status != ELEM_VALID {
                continue;
            }
            let id = ElemId::new(chap, i as i32);
            let target = if chap == cid.shp {
                elemrel::find_related_elem(&mut sc.dict, id, false, 0, 200, cid.model)
            } else if chap == cid.cls {
                elemrel::find_related_elem(&mut sc.dict, id, false, 0, 700, cid.model)
            } else if chap == cid.lat {
                elemrel::find_related_elem(&mut sc.dict, id, false, 0, 0xd2, cid.model).or_else(|| elemrel::find_related_elem(&mut sc.dict, id, false, 0, 0xd7, cid.model))
            } else {
                Some(id)
            };
            let Some(t) = target else { continue };
            let aux = sc.dict.slot(t).map_or(0, |e| e.rec.aux_data.0);
            let sel = match sc.graf.get(aux) {
                None => 0,
                Some(n) => {
                    if !from_flags || n.flags & 0x1000 == 0 {
                        n.selection as i16
                    } else if n.flags & 0x2000 != 0 {
                        1
                    } else if n.flags & 0x4000 != 0 {
                        3
                    } else if n.flags & 0x8000 != 0 {
                        2
                    } else {
                        0
                    }
                }
            };
            sc.dict.ed.lists[chap as usize].elems[i].rec.selected = sel;
        }
    }
}

/// The model nodes of the scene graph (kinds 2 and 3), as node of the graph and model
/// object: what the two routines below go through (`SI_grafLists` of the current scene).
fn model_graf_nodes(sc: &Scene) -> Vec<(u32, u32)> {
    sc.graf.handles().filter_map(|h| sc.graf.get(h).filter(|n| n.kind == 2 || n.kind == 3).map(|n| (h, n.data.0))).collect()
}

fn model_dirty(sc: &Scene, object: u32) -> u16 {
    match sc.objects.get(object) {
        Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).map_or(0, |t| t.dirty[*node]),
        _ => 0,
    }
}

/// [soft 0x08104064 edSetDirtyFromHierarchy]: a root is dirty when it is or when any element
/// below it is (sons by `SI_edFindSon`: relation 110, backward side, list 1).
fn set_dirty_from_hierarchy(sc: &mut Scene, root: ElemId, elem: ElemId) {
    let dirty = sc.dict.slot(root).is_some_and(|e| e.rec.dirty != 0) || sc.dict.slot(elem).is_some_and(|e| e.rec.dirty != 0);
    if let Some(e) = sc.dict.slot_mut(root) {
        e.rec.dirty = dirty as i16;
    }
    if root.chap == sc.program.cid.model {
        let mut son = elemdict::find_son(&mut sc.dict, &sc.program, elem);
        while let Some(s) = son {
            set_dirty_from_hierarchy(sc, root, s);
            son = elemdict::find_next_son(&mut sc.dict, &sc.program, elem);
        }
    }
}

/// Works out which elements have changed since the last save
/// [soft 0x081040d4 si_edGetDirtyFlags]. **This is what decides which files a save
/// writes**; the Save dialogue calls it before [`save_scene`] (the emergency save and the
/// automatic save do not).
///
/// 1. "Dirty" is cleared on every slot of every chapter except the eleven shader chapters.
/// 2. For each model node of the scene graph: its element takes the node's own dirty word
///    (`__SI_Model` +0x0a); the elements of its materials get bit 0x40 of that word, those
///    of its materials' 2D and 3D textures and of its own global 2D textures and 3D texture
///    get bit 0x80.
/// 3. Each root in use of MODELS is dirty if any element of its hierarchy is.
/// 4. A spreadsheet setup takes its own record's word (+0x6c).
/// 5. **Always dirty**: every slot of ANIMATION, CLUSTERS, SHAPES, LATTICES, CAMERAS,
///    LIGHTS, SETUP_SOFT, SETUP_CHANNELS, SETUP_FLOCK and WAVES.
///
/// The element of a model, material or texture is looked up by its object
/// (`SI_edGetElemFromModel` and its siblings, of `edtools.c`; here the first element in use
/// that points at it).
pub fn get_dirty_flags(sc: &mut Scene, un: &mut dyn Unported) {
    let cid = sc.program.cid;
    let shaders = [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon];
    for chap in 0..sc.dict.ed.lists.len() as i32 {
        if !shaders.contains(&chap) {
            for e in sc.dict.ed.lists[chap as usize].elems.iter_mut() {
                e.rec.dirty = 0;
            }
        }
    }
    let or_bit = |sc: &mut Scene, chap: i32, object: u32, bits: u16| {
        let i = find_elem_by_data(sc, chap, object);
        if i >= 0 {
            sc.dict.ed.lists[chap as usize].elems[i as usize].rec.dirty |= bits as i16;
        }
    };
    for (_, object) in model_graf_nodes(sc) {
        let dirty = model_dirty(sc, object);
        let i = find_elem_by_data(sc, cid.model, object);
        if i >= 0 {
            sc.dict.ed.lists[cid.model as usize].elems[i as usize].rec.dirty = dirty as i16;
        }
        let Some(Object::ModelNode { tree, node }) = sc.objects.get(object).cloned() else { continue };
        let (mats, tex, tex3) = match sc.models.get(tree) {
            Some(t) => (t.mat_links[node].clone(), t.tex_links[node], t.tex3d_links[node]),
            None => continue,
        };
        // a node that keeps its file's materials has records no element points at
        for m in mats.unwrap_or_default() {
            or_bit(sc, cid.mat, m, dirty & 0x40);
            let links = sc.mat_links.get(&m).cloned().unwrap_or_default();
            for t in scnrel::texture_list(sc, links.textures.and_then(|(_, h)| h)) {
                or_bit(sc, cid.t2d, t, dirty & 0x80);
            }
            if let Some(Some(t3)) = links.texture3d {
                or_bit(sc, cid.t3d, t3, dirty & 0x80);
            }
        }
        for t in scnrel::texture_list(sc, tex.and_then(|(_, h)| h)) {
            or_bit(sc, cid.t2d, t, dirty & 0x80);
        }
        if let Some(Some(t3)) = tex3 {
            or_bit(sc, cid.t3d, t3, dirty & 0x80);
        }
    }
    for i in 0..sc.dict.ed.lists[cid.model as usize].elems.len() {
        let e = sc.dict.ed.lists[cid.model as usize].elems[i].rec;
        if e.status == ELEM_VALID && e.hier_level == 0 {
            let id = ElemId::new(cid.model, i as i32);
            set_dirty_from_hierarchy(sc, id, id);
        }
    }
    for i in 0..sc.dict.ed.lists[cid.spd as usize].elems.len() {
        let e = sc.dict.ed.lists[cid.spd as usize].elems[i].rec;
        if e.status == ELEM_VALID && !e.data.is_null() {
            un.call("the dirty word of a spreadsheet setup (+0x6c)", &[Arg::Elem(ElemId::new(cid.spd, i as i32))]);
        }
    }
    for chap in [cid.ani, cid.cls, cid.shp, cid.lat, cid.cam, cid.lig, cid.sts, cid.stc, cid.sfk, cid.wav] {
        for e in sc.dict.ed.lists[chap as usize].elems.iter_mut() {
            e.rec.dirty = 1;
        }
    }
}

/// After a save, the models are clean [soft 0x081045b8 si_edResetDirtyFlags]: the dirty word
/// of every model node of the scene graph is cleared (with `selected_only`, only of nodes
/// whose selection state is not 0). For a full reset, each spreadsheet setup in use takes
/// its element's dirty word (+0x6c) and the opposite as its "saved" word (+4)
/// (the setup's record is another package's: recorded).
pub fn reset_dirty_flags(sc: &mut Scene, selected_only: bool, un: &mut dyn Unported) {
    for (node, object) in model_graf_nodes(sc) {
        if selected_only && sc.graf.get(node).is_some_and(|n| n.selection == 0) {
            continue;
        }
        if let Some(Object::ModelNode { tree, node }) = sc.objects.get(object).cloned() {
            if let Some(t) = sc.models.get_mut(tree) {
                t.dirty[node] = 0;
            }
        }
    }
    if !selected_only {
        let spd = sc.program.cid.spd;
        for i in 0..sc.dict.ed.lists[spd as usize].elems.len() {
            let e = sc.dict.ed.lists[spd as usize].elems[i].rec;
            if e.status == ELEM_VALID && !e.data.is_null() {
                un.call("the dirty and saved words of a spreadsheet setup", &[Arg::Elem(ElemId::new(spd, i as i32)), Arg::Int(e.dirty as i64)]);
            }
        }
    }
}

// adapter: not in place
/// `SI_elemdictChangeDscName` (0x081361d8) on a dictionary in stores: gives the elements
/// of a scene the scene's name as prefix. The routine is `si-dict`'s
/// (`si_dict::elemname::change_dsc_name`), run on an image of the stores
/// ([`on_image_of_stores`]): in every chapter but SCENES and PICTURES, each element in
/// use that is not shared (in EXPRESSIONS: roots only) without a prefix gets `name` (with
/// `only_missing`), and with `replace` one whose prefix is `old` gets it; such an element
/// has its full name rebuilt and its new-flag set to 2, so the save will ask before
/// writing over a file of that name.
pub fn change_dsc_name(sc: &mut Scene, name: Option<&[u8]>, only_missing: bool, replace: bool, old: Option<&[u8]>) {
    let mut un = crate::calls::Recorder::default();
    on_image_of_stores(sc, &mut un, |m, o, ed| {
        // the two strings as the original's callers hold them: in buffers of their own
        let put = |m: &mut si_mem::Mem, s: Option<&[u8]>| -> si_mem::CStr32 {
            match s {
                None => Ptr32::NULL,
                Some(s) => {
                    let p = m.local((s.len() as u32 + 4) & !3);
                    m.put_cstr(p, s);
                    Ptr32::new(p)
                }
            }
        };
        let (name, old) = (put(m, name), put(m, old));
        si_dict::elemname::change_dsc_name(m, o, ed, 0, name, only_missing as i16, replace as i16, old);
        1
    });
}

// adapter: not in place
/// Runs a routine that is on the image (the duplicate-name check of [`crate::dupnames`],
/// `SI_elemdictChangeDscName`) for the saver, whose dictionary is in stores: the stores
/// are copied into a fresh image ([`crate::world::to_image`]), the routine runs there,
/// and, when it answers that it changed something, what such a routine changes (an
/// element's prefix, name and full name, its new-flag) is copied back into the stores.
fn on_image_of_stores(sc: &mut Scene, un: &mut dyn Unported, run: impl FnOnce(&mut si_mem::Mem, &mut crate::world::Out, Ptr32<si_types::dict::ElemDict>) -> usize) -> usize {
    use crate::world;
    let mut m = world::new_image();
    let ed = world::to_image(&mut m, &sc.dict, &sc.program);
    let renamed = run(&mut m, &mut world::Out { sc, un }, ed);
    if renamed > 0 {
        for chap in 0..m[ed].nb_lists {
            for i in 0..world::nb_elems(&m, ed, chap) {
                let id = ElemId::new(chap, i);
                let e = world::elem(&m, ed, id);
                let (new_flag, node) = (m[e].new_flag, m[e].node);
                let Some(store) = sc.dict.ed.lists.get_mut(chap as usize).and_then(|l| l.elems.get_mut(i as usize)) else { continue };
                store.rec.new_flag = new_flag;
                let handle = store.rec.node.0;
                if node.is_null() {
                    continue;
                }
                if let Some(x) = sc.dict.nodes.get_mut(handle) {
                    x.prefix = crate::dbnode::text(&m, m[node].prefix);
                    x.name = crate::dbnode::text(&m, m[node].name);
                    x.full = crate::dbnode::text(&m, m[node].fullname);
                }
            }
        }
    }
    renamed
}

// adapter: not in place
/// `si_dbCheckChapterDuplicateNames` (0x081096f4) on a dictionary in stores; returns how
/// many elements were renamed. The routine is [`crate::dupnames::check_chapter_duplicate_names`].
pub fn check_chapter_duplicate_names(sc: &mut Scene, chap: i32, un: &mut dyn Unported) -> usize {
    on_image_of_stores(sc, un, |m, o, ed| {
        let list = crate::world::list(m, ed, chap);
        crate::dupnames::check_chapter_duplicate_names(m, o, list, chap)
    })
}

// adapter: not in place
/// `SI_dbCheckDuplicateNames` (0x08109be4) on a dictionary in stores; returns how many
/// elements were renamed. The routine is [`crate::dupnames::db_check_duplicate_names`].
pub fn check_duplicate_names(sc: &mut Scene, un: &mut dyn Unported) -> usize {
    on_image_of_stores(sc, un, |m, o, ed| crate::dupnames::db_check_duplicate_names(m, o, ed))
}

/// Copies what names an element from one name record to another
/// [soft 0x08131ba8 SI_dbListNodeCopyInfo]: sub-directory, alias, prefix, name, version,
/// substitution number, time and mode; then the full name is rebuilt. A missing prefix is
/// replaced by **the name of the dictionary's first scene**. Strings the source lacks keep
/// what the destination had, except sub-directory, version and substitution number, which
/// are dropped first. `with_dictionary`: the routine was given a dictionary (without one a
/// missing prefix stays missing).
pub fn list_node_copy_info(sc: &Scene, with_dictionary: bool, src: &ListNode, dst: &mut ListNode) {
    dst.sub_path = src.sub_path.clone();
    dst.version = src.version.clone();
    dst.subst = src.subst.clone();
    if src.s0c.is_some() {
        dst.s0c = src.s0c.clone();
    }
    let prefix = src.prefix.clone().or_else(|| {
        if !with_dictionary {
            return None;
        }
        let l = &sc.dict.ed.lists[sc.program.cid.dsc as usize];
        l.elems.first().and_then(|e| sc.dict.nodes.get(e.rec.node.0)).and_then(|n| n.name.clone())
    });
    if prefix.is_some() {
        dst.prefix = prefix;
    }
    if src.name.is_some() {
        dst.name = src.name.clone();
    }
    dst.time = src.time;
    dst.mode = src.mode;
    dst.full = Some(si_db::format_full_name(dst, sc.program.loading));
}

/// Makes sure the scene's setups are elements that will be saved under the scene's name
/// [soft 0x08106300 SI_elemdictCheckSetupElem]. Nothing without a scene element. With one:
///
/// - **SETUP_SOFT**: its first slot takes the scene's name, version and sub-directory
///   ([`list_node_copy_info`]); an empty chapter gets an element first. If the element has
///   no object it is given one byte (a copy of the string `" "`): the element saver only
///   writes elements that point at something, and frees this again.
/// - **SETUP_CHANNELS**, **SETUP_FLOCK**: the same, but only when the setup is not the
///   default one (→ `SI_chnSetupDefault`, `SI_particleSetupDefault`), and not in the batch
///   renderer unless it is saving in an emergency.
/// - **EXPRESSIONS**: the first root in use takes the scene's name too, and twelve zero
///   bytes as its object if it has none. (The original counts that allocation in a global
///   counter, which is not reproduced.)
pub fn check_setup_elem(sc: &mut Scene, h: &mut Host) {
    let cid = sc.program.cid;
    let emergency = sc.saving.emergency_saving;
    let Some(scene_node) = sc.dict.ed.lists[cid.dsc as usize].elems.first().and_then(|e| sc.dict.nodes.get(e.rec.node.0)).cloned() else {
        // no slot in SCENES; (a first slot without name record stops the original)
        return;
    };
    let give = |sc: &mut Scene, chap: i32| {
        if sc.dict.ed.lists[chap as usize].rec.nb_elems < 1 {
            let mut node = ListNode::allocate(sc.program.now);
            list_node_copy_info(sc, true, &scene_node, &mut node);
            init::add_elem(&mut sc.dict, &sc.program, chap, Some(node), Ptr32::NULL, Ptr32::NULL, 0, -1);
        } else {
            let handle = sc.dict.ed.lists[chap as usize].elems[0].rec.node.0;
            if let Some(mut node) = sc.dict.nodes.get(handle).cloned() {
                list_node_copy_info(sc, true, &scene_node, &mut node);
                *sc.dict.nodes.get_mut(handle).unwrap() = node;
            }
        }
        if sc.dict.ed.lists[chap as usize].elems[0].rec.data.is_null() {
            let p = sc.objects.alloc(Object::Placeholder);
            sc.dict.ed.lists[chap as usize].elems[0].rec.data = data_ptr(p);
        }
    };
    give(sc, cid.sts);
    if h.ui.ask("SI_chnSetupDefault", &[]) == 0 && (!sc.standalone_renderer || emergency) {
        give(sc, cid.stc);
    }
    if h.ui.ask("SI_particleSetupDefault", &[]) == 0 && (!sc.standalone_renderer || emergency) {
        give(sc, cid.sfk);
    }
    let exp = cid.exp as usize;
    if let Some(i) = sc.dict.ed.lists[exp].elems.iter().position(|e| e.rec.status == ELEM_VALID && e.rec.hier_level == 0) {
        let handle = sc.dict.ed.lists[exp].elems[i].rec.node.0;
        if let Some(mut node) = sc.dict.nodes.get(handle).cloned() {
            list_node_copy_info(sc, true, &scene_node, &mut node);
            *sc.dict.nodes.get_mut(handle).unwrap() = node;
        }
        if sc.dict.ed.lists[exp].elems[i].rec.data.is_null() {
            let p = sc.objects.alloc(Object::Placeholder);
            sc.dict.ed.lists[exp].elems[i].rec.data = data_ptr(p);
        }
    }
}

/// Finds the name the scene is saved under [soft 0x081303d8 SI_dbCheckWriteDescriptionFile];
/// false when the user cancelled. The SCENES chapter becomes the database's current chapter
/// and its number of versions is read again from its `Chapter.rsrc`. Unless the caller
/// already has the name (`name_given`), the scene's element goes through
/// [`dictout::find_save_name`] like any other (without a listing of the chapter: the files
/// of this scene are looked up on their own).
pub fn check_write_description_file(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, scene: ElemId, name_given: bool, ask: &mut i16) -> bool {
    let dsc = sc.program.cid.dsc;
    let chapter = sc.dict.ed.lists[dsc as usize].name.clone();
    info.set_chapter(&chapter);
    info.filepath_build();
    si_db::db_info_get_nb_versions(info, &chapter, Some(&*h.fs));
    !(!name_given && dictout::find_save_name(sc, h, info, dsc, scene, ask, true, None) == NAME_CANCEL_ALL)
}

/// Writes the scene description to its file: the part of `SI_dbWriteDescriptionFile`
/// (0x08130460) around what [`save::build_description`] and `si-ascii` do. The file is
/// `<SCENES directory><sub-directory><full name>.dsc`; with piped files it goes into the
/// pipe. **If the file cannot be opened nothing happens and nothing is said**: the routine
/// answers 1 either way, and the elements keep their marks.
pub fn write_description_file(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, scene: ElemId, selected_only: bool) -> bool {
    let dsc = sc.program.cid.dsc;
    let chapter = sc.dict.ed.lists[dsc as usize].name.clone();
    info.set_chapter(&chapter);
    info.filepath_build();
    let n = sc.dict.node_of(scene.chap, scene.index);
    let sub = n.map(|n| or_null(&n.sub_path).to_vec()).unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    let full = n.map(|n| or_null(&n.full).to_vec()).unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    sc.saving.work_fname = [or_null(&info.chapter_path), &sub[..], &full[..]].concat();
    sc.saving.work_fname_ext = [&sc.saving.work_fname[..], b".dsc"].concat();
    let path = sc.saving.work_fname_ext.clone();
    // the writer numbers the elements and clears marks as it goes; none of that happens
    // when the file cannot be opened
    let before = sc.dict.clone();
    let srt_before = sc.srt.clone();
    let d = save::build_description(sc, NameOrder::Linux, selected_only, None, h.un);
    let bytes = d.write(sc.saving.version_output);
    if sc.saving.piped {
        sc.saving.pipe.extend_from_slice(&bytes);
        return true;
    }
    if h.fs.write(&path, &bytes, false) {
        sc.saving.written.push(dictout::Written { elem: scene, path });
    } else {
        sc.dict = before;
        sc.srt = srt_before;
    }
    true
}

/// "Save Scene" [soft 0x080db498 SI_dbSaveScene].
///
/// `emergency`: the program is going down and saves what it has (nothing is asked, every
/// element is written, the marks are left). `selected_only`: only what is selected.
/// `old_prefix`: the scene's previous name when it is being saved under a new one.
/// `name_given`: the caller has already settled the scene's name and version.
///
/// 1. Hybrid custom effects are taken out of the way for the time of the save
///    (→ `si_cusCheckForHybridIcons`: the icons of effects of kind 4 get status 3 and are
///    not saved); the custom effects are told a save starts (→ `SI_cusCallCustomFunctions`
///    12, 7).
/// 2. An emergency save of a dictionary without a scene makes the default scene.
/// 3. [`get_elements_graf_node_info`].
/// 4. For "selected only": [`get_selected_elements`], and the hierarchy information of
///    MODELS, CAMERAS and LIGHTS is recomputed.
/// 5. [`check_write_description_file`]: the scene's name and version. Cancelled there, the
///    save does nothing more (but steps 9 and 10).
/// 6. [`change_dsc_name`] (elements without a prefix get the scene's name),
///    [`check_duplicate_names`], [`check_setup_elem`].
/// 7. **The element files** ([`dictout::save_elements`]). If that fails or is cancelled the
///    description is **not** written: the database then holds new element files that no
///    description names, and the previous description still names the previous versions.
/// 8. **The description** ([`write_description_file`]); then, unless this is an emergency
///    save, the models are marked clean ([`reset_dirty_flags`]).
/// 9. The hybrid effects come back; outside an emergency save the status line says `Hybrid
///    F/X are not saved with the scene.`
/// 10. The custom effects are told the save is over (12, 8).
///
/// Returns whether the description was written (the original returns nothing).
pub fn save_scene(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, emergency: bool, selected_only: bool, old_prefix: Option<&[u8]>, name_given: bool) -> bool {
    let cid = sc.program.cid;
    let mut ask: i16 = 1;
    let mut written = false;
    let hybrid = h.ui.ask("si_cusCheckForHybridIcons", &[Arg::Int(0), Arg::Int(3), Arg::Int(0)]);
    h.un.call("SI_cusCallCustomFunctions", &[Arg::Int(12), Arg::Int(7)]);
    if emergency && sc.dict.ed.lists[cid.dsc as usize].rec.nb_elems < 1 {
        init::create_default_dsc(&mut sc.dict, &sc.program);
    }
    let scene = ElemId::new(cid.dsc, 0);
    get_elements_graf_node_info(sc);
    if selected_only {
        get_selected_elements(sc, false);
        sc.dict.ed.update_hier_info(cid.model, 1, elemrel::REL_PARENT);
        sc.dict.ed.update_hier_info(cid.cam, 1, elemrel::REL_CAM_INT);
        sc.dict.ed.update_hier_info(cid.lig, 1, elemrel::REL_LIG_INT);
    }
    // (a SCENES chapter without a slot makes the original read a null pointer from here on)
    if sc.dict.slot(scene).is_some() && check_write_description_file(sc, h, info, scene, name_given, &mut ask) {
        let name = sc.dict.node_of(scene.chap, scene.index).and_then(|n| n.name.clone());
        change_dsc_name(sc, name.as_deref(), true, old_prefix.is_some(), old_prefix);
        check_duplicate_names(sc, h.un);
        check_setup_elem(sc, h);
        if dictout::save_elements(sc, h, info, selected_only, emergency, -1, &mut ask) {
            write_description_file(sc, h, info, scene, selected_only);
            written = true;
            if !emergency {
                reset_dirty_flags(sc, false, h.un);
            }
        }
    }
    if hybrid != 0 {
        h.un.call("si_cusCheckForHybridIcons", &[Arg::Int(3), Arg::Int(0), Arg::Int(hybrid as i64)]);
        if !emergency {
            sc.messages.push(Message { channel: Channel::Status(2), text: b"Hybrid F/X are not saved with the scene.\n".to_vec() });
        }
    }
    h.un.call("SI_cusCallCustomFunctions", &[Arg::Int(12), Arg::Int(8)]);
    written
}

/// The same into the program's pipe [soft 0x080db654 SI_dbSaveScenePIPE]: every file goes
/// into one stream instead of the database (`SI_pDbiout`), **the description first**, then
/// the element files as in an emergency save (everything, marks left). No name is looked
/// for and no prefix is given; a dictionary without a scene gets the default scene.
pub fn save_scene_pipe(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool) {
    let cid = sc.program.cid;
    let mut ask: i16 = 1;
    sc.saving.piped = true;
    if sc.dict.ed.lists[cid.dsc as usize].rec.nb_elems < 1 {
        init::create_default_dsc(&mut sc.dict, &sc.program);
    }
    let scene = ElemId::new(cid.dsc, 0);
    get_elements_graf_node_info(sc);
    if selected_only {
        get_selected_elements(sc, false);
    }
    check_duplicate_names(sc, h.un);
    check_setup_elem(sc, h);
    write_description_file(sc, h, info, scene, selected_only);
    dictout::save_elements(sc, h, info, selected_only, true, -1, &mut ask);
    sc.saving.piped = false;
}
