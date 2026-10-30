// SPDX-License-Identifier: MIT
//! Routines of other source files that the render set-up calls and that no crate had yet:
//! from `elemdict.c` the copy of a dictionary's elements and of its render relations, the
//! look-up of an element by its object and the making of a new name; from `edtools.c` a model
//! put into a dictionary; from `instance.c` the sharing of materials and textures between an
//! instance and its copy; from `model.c` a node's renaming. Each is ported here as far as the
//! set-up needs it, and cited only where the whole routine is reproduced.

use crate::world::{get_elem, rel, Env, Heap, Obj};
use si_db::ListNode;
use si_scene::dict::{Dict, ElemId, Program};
use si_scene::elemrel;
use si_types::dict::{ELEM_DELETED, ELEM_VALID};
use si_types::{Opaque, Ptr32};

/// The index of the first element in use of a chapter whose object is `data`, -1 when there
/// is none: `SI_elemdictFindElemByData` (0x08134364) as it runs when the chapter has no
/// sorted table or `SI_useElemListSortTbl` is off, which is always outside a load made with
/// `SI_LOAD_OPTIM` in the environment. Not cited: the other path (a table sorted by pointer
/// value, bisected, with the last answer kept in three statics) is not ported.
pub fn find_elem_by_data(d: &Dict, chap: i32, data: u32) -> i32 {
    let Some(l) = usize::try_from(chap).ok().and_then(|c| d.ed.lists.get(c)) else { return -1 };
    l.elems.iter().position(|e| e.rec.status == ELEM_VALID && e.rec.data.0 == data).map_or(-1, |i| i as i32)
}

/// The element whose object is `data` and its index
/// [soft 0x08137f94 SI_elemdictGetElemFromData]: -1 and no element for a null object.
pub fn get_elem_from_data(d: &Dict, chap: i32, data: u32) -> (Option<ElemId>, i32) {
    if data == 0 {
        return (None, -1);
    }
    let i = find_elem_by_data(d, chap, data);
    if i > -1 {
        (get_elem(d, chap, i), i)
    } else {
        (None, i)
    }
}

/// Copies what names an element from one name record to another
/// [soft 0x08131ba8 SI_dbListNodeCopyInfo]. The destination's sub-directory, version and
/// substitution number are dropped first; then sub-directory, alias, prefix, name, version
/// and substitution number are copied where the source has them (a string the source lacks
/// keeps what the destination had), time and mode always, and the full name is rebuilt. A
/// source without prefix gets **the name of the first slot of the dictionary's SCENES
/// chapter** as prefix, whatever that slot's status.
pub fn list_node_copy_info(d: Option<&Dict>, p: &Program, src: &ListNode, dst: &mut ListNode) {
    dst.sub_path = src.sub_path.clone();
    dst.version = src.version.clone();
    dst.subst = src.subst.clone();
    if src.s0c.is_some() {
        dst.s0c = src.s0c.clone();
    }
    let prefix = src.prefix.clone().or_else(|| {
        let l = d?.ed.lists.get(usize::try_from(p.cid.dsc).ok()?)?;
        d?.nodes.get(l.elems.first()?.rec.node.0)?.name.clone()
    });
    if prefix.is_some() {
        dst.prefix = prefix;
    }
    if src.name.is_some() {
        dst.name = src.name.clone();
    }
    dst.time = src.time;
    dst.mode = src.mode;
    dst.full = Some(si_db::format_full_name(dst, p.loading));
}

/// Copies every slot of every chapter of `src` into the chapter of the same number of `dst`
/// [soft 0x08137cd0 SI_elemdictCopyElements]. A copy is a new slot at the end of the
/// chapter (freed slots are copied too, so that indices stay the same) with its own name
/// record and empty relation lists; it takes the selected and shared words, the status, the
/// depth and root index, **the object pointer itself** (the object is shared, not copied),
/// the thirteen words from the schematic position to the pointer at +0x3c, and new-flag 1.
/// The object behind `aux_data` (the scene graph's node) is not carried over.
pub fn copy_elements(src: &Dict, dst: &mut Dict, p: &Program) {
    for c in 0..src.ed.rec.nb_lists.max(0) as usize {
        for i in 0..src.ed.lists[c].elems.len() {
            let from = src.ed.lists[c].elems[i].rec;
            let node = match src.nodes.get(from.node.0) {
                Some(n) => {
                    let mut copy = ListNode::allocate(p.now);
                    list_node_copy_info(Some(src), p, n, &mut copy);
                    dst.nodes.alloc(copy)
                }
                None => 0,
            };
            let l = &mut dst.ed.lists[c];
            let k = l.add_elem(Ptr32::new(node), Ptr32::NULL, Ptr32::NULL, 0, 0, true, false, p.load_optim, p.def_color) as usize;
            let to = &mut l.elems[k].rec;
            to.selected = from.selected;
            to.shared = from.shared;
            to.status = from.status;
            to.hier_level = from.hier_level;
            to.root_id = from.root_id;
            to.data = from.data;
            to.new_flag = 1;
            to.schem_x = from.schem_x;
            to.schem_y = from.schem_y;
            to.schem_width = from.schem_width;
            to.schem_collapse = from.schem_collapse;
            to.wire_color = from.wire_color;
            to.wire_type = from.wire_type;
            to.selection = from.selection;
            to.dope_state = from.dope_state;
            to.schem_flags = from.schem_flags;
            to.pad_2b = from.pad_2b;
            to.schem_bbox = from.schem_bbox;
            to.root_srt = from.root_srt;
        }
    }
}

/// Copies the relations a render needs [soft 0x08137ddc SI_elemdictCopyRenderRelations]:
/// every forward entry of every element in use of `src` whose relation has the render flag
/// in the relation dictionary (`RENDERING` on its line of `Relations.rsrc`) is added to
/// `dst` between the slots of the same numbers, under the relation's first rule, without
/// data and active. Serial numbers, data, the inactive mark and the entry's own list are
/// not carried over. Returns how many were handed to the adder.
pub fn copy_render_relations(src: &Dict, dst: &mut Dict, p: &mut Program) -> usize {
    let mut n = 0;
    for c in 0..src.ed.rec.nb_lists.max(0) as usize {
        for i in 0..src.ed.lists[c].elems.len() {
            let e = &src.ed.lists[c].elems[i];
            if e.rec.status != ELEM_VALID {
                continue;
            }
            for g in 0..(e.rec.forward.nb_groups.max(0) as usize).min(e.forward.len()) {
                for r in &e.forward[g].rels {
                    if p.rd.get_rel_render_flag(r.relid) as i16 != 0 {
                        elemrel::add_uncomplete_relation(dst, p, c as i32, r.chapid as i32, i as i32, r.elemid, r.relid, 0, 0, Ptr32::NULL, 1);
                        n += 1;
                    }
                }
            }
        }
    }
    n
}

/// The statics of `SI_elemdictGetNewNameId`: the number given last (`num`, 0x08683e28) and
/// whether `SI_XSI_LEGACYLOADER` is in the environment, which the routine asks each time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NewName {
    pub num: i32,
    pub legacy_loader: bool,
}

/// C's `atoi`: blanks, a sign, digits; the result wraps as `strtol` cut to 32 bits would
/// not, but no name carries a number that long.
fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(i32::MAX as i64 + 1);
        i += 1;
    }
    if neg { (-v).max(i32::MIN as i64) as i32 } else { v.min(i32::MAX as i64) as i32 }
}

/// A name no element of the chapter has yet, and its number
/// [soft 0x081366a0 SI_elemdictGetNewNameId]. The base is `name` without its trailing
/// digits (`obj` when no name is given). Every slot that is not freed and has a name is
/// looked at (its prefix instead when `by_prefix`); where the base is the start of that
/// name and the name is longer, the number that follows (`atoi`) raises the count. The
/// answer is the base followed by the count plus one.
///
/// With `SI_XSI_LEGACYLOADER` in the environment nothing is looked at: the count simply
/// goes on from the last call and the name is `base_count`.
pub fn get_new_name_id(d: &Dict, st: &mut NewName, chap: i32, name: Option<&[u8]>, by_prefix: bool) -> (Vec<u8>, i32) {
    if !st.legacy_loader {
        st.num = 0;
    }
    let base: &[u8] = match name {
        Some(n) => {
            let mut len = n.len();
            while len > 0 && n[len - 1].is_ascii_digit() {
                len -= 1;
            }
            &n[..len]
        }
        None => b"obj",
    };
    if !st.legacy_loader {
        if let Some(l) = usize::try_from(chap).ok().and_then(|c| d.ed.lists.get(c)) {
            for e in &l.elems {
                if e.rec.status == ELEM_DELETED {
                    continue;
                }
                let Some(node) = d.nodes.get(e.rec.node.0) else { continue };
                if node.name.is_none() {
                    continue;
                }
                let Some(s) = (if by_prefix { node.prefix.as_ref() } else { node.name.as_ref() }) else { continue };
                // strncmp(base, s, n) == 0 and strlen(s) > n
                if s.len() > base.len() && s[..base.len()] == *base {
                    let v = atoi(&s[base.len()..]);
                    if v > st.num {
                        st.num = v;
                    }
                }
            }
        }
    }
    st.num = st.num.wrapping_add(1);
    let mut out = base.to_vec();
    if st.legacy_loader {
        out.push(b'_');
    }
    out.extend_from_slice(st.num.to_string().as_bytes());
    (out, st.num)
}

/// [soft 0x0813697c SI_elemdictGetNewName]: the name alone, looked up by element names.
pub fn get_new_name(d: &Dict, st: &mut NewName, chap: i32, name: Option<&[u8]>) -> Vec<u8> {
    get_new_name_id(d, st, chap, name, false).0
}

/// Renames a node [soft 0x0846db14 SI_modelChangeName]: bit 0 of both update words is set
/// and the name replaced by a copy of the new one. (The original leaves the name alone when
/// it is handed the node's own string; a caller here cannot do that.)
pub fn model_change_name(heap: &mut Heap, model: u32, name: &[u8]) {
    let new = heap.strdup(name);
    let Some(m) = heap.model_mut(model) else { return };
    m.update_flags |= 1;
    m.update_flags2 |= 1;
    let old = m.name.0;
    m.name = Ptr32::new(new);
    heap.free(old);
}

/// Gives a copy the materials of the node it was made from
/// [soft 0x081a8310 SI_instanceMaterialShare]: nothing for a node without materials;
/// otherwise the copy gets the count, the current material's number and **an array of its
/// own holding the same material pointers** (the copy's previous array, if any, is neither
/// freed nor looked at). With `regInstMatArrays` set the new array is entered at the head of
/// the list of blocks to free after a frame (`addMemEntry`, 0x081a7310; `memList`).
pub fn instance_material_share(env: &mut Env, from: u32, to: u32) {
    let Some(src) = env.heap.model(from) else { return };
    let (n, cur, arr) = (src.nb_materials, src.cur_material, src.materials.0);
    if n == 0 {
        return;
    }
    // calloc(n, 4) and a copy of n entries; a negative count allocates nothing usable
    let mut copy = vec![0u32; n.max(0) as usize];
    if let Some(a) = env.heap.words(arr) {
        for (i, w) in copy.iter_mut().enumerate() {
            *w = a.get(i).copied().unwrap_or(0);
        }
    }
    let block = env.heap.alloc(Obj::Words(copy));
    if env.g.reg_inst_mat_arrays != 0 {
        env.g.mem_list.insert(0, block);
    }
    if let Some(dst) = env.heap.model_mut(to) {
        dst.nb_materials = n;
        dst.cur_material = cur;
        dst.materials = Ptr32::new(block);
    }
}

/// Gives a copy the textures of the node it was made from
/// [soft 0x081a83a8 SI_instanceTextureShare]: when the node has a 2D texture list or a 3D
/// texture, the copy takes the list's head, the pointer beside it (+0x218) and the count
/// (+0x216) as they are, and the 3D texture when there is one.
pub fn instance_texture_share(heap: &mut Heap, from: u32, to: u32) {
    let Some(src) = heap.model(from).copied() else { return };
    if src.textures.is_null() && src.texture3d.is_null() {
        return;
    }
    let Some(dst) = heap.model_mut(to) else { return };
    dst.textures = src.textures;
    dst.texture_218 = src.texture_218;
    dst.unk_216 = src.unk_216;
    if !src.texture3d.is_null() {
        dst.texture3d = src.texture3d;
    }
}

/// The master of an instance [soft 0x081a74c0 SI_elemdictModelMasterFromInstance]: the
/// object of the first element the instance's element is related to by 20002 in list 4,
/// forward side. Null when the node has no element in use in the dictionary, or no such
/// relation.
pub fn model_master_from_instance(d: &mut Dict, p: &Program, model: u32) -> u32 {
    let (Some(e), _) = get_elem_from_data(d, p.cid.model, model) else { return 0 };
    elemrel::find_related_elem(d, e, true, 4, rel::INST, p.cid.model).map_or(0, |m| crate::world::data_of(d, m))
}

/// What `SI_elemdictAddElem` leaves to its caller in `si-types`: the two routines of
/// `edtools.c` that save the model relations. They are not ported; the request is counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Backups(pub usize);

/// Puts a node into the MODELS chapter of a dictionary
/// [soft 0x081017f4 SI_elemdictAddModel]: a name record with the node's name (with
/// `new_name`, first a name no element has yet, which the node takes too), a new element
/// whose object is the node, the node told its element's index, and the element related to
/// its father's element by 110 when the node has a father. While `SI_addInDictionary` is
/// zero nothing is added: the node is only told the index of the element that already
/// points at it (-1 for none).
///
/// The original ends with a test of the product-tier flag `SI_lightLicense`, under which
/// the new element may be marked uneditable; the full product's path, taken here, does
/// nothing there. The check itself is not ported.
#[allow(clippy::too_many_arguments)]
pub fn add_model(d: &mut Dict, env: &mut Env, names: &mut NewName, model: u32, graf: Ptr32<Opaque>, new_name: bool, backups: &mut Backups) -> i32 {
    let cm = env.p.cid.model;
    if env.p.add_in_dictionary == 0 {
        let i = find_elem_by_data(d, cm, model);
        if let Some(m) = env.heap.model_mut(model) {
            m.elem_id = i;
        }
        return i;
    }
    let mut node = ListNode::allocate(env.p.now);
    if new_name {
        let n = get_new_name(d, names, cm, env.heap.model_name(model));
        model_change_name(&mut env.heap, model, &n);
    }
    node.name = env.heap.model_name(model).map(|s| s.to_vec());
    node.full = Some(si_db::format_full_name(&mut node, env.p.loading));
    let node = d.nodes.alloc(node);
    let added = d.ed.add_elem(cm, Ptr32::new(node), Ptr32::new(model), graf, 0, 0, cm, env.p.backup_enabled, env.p.load_optim, env.p.def_color);
    let index = added.map_or(-1, |a| {
        if a.backup_model_relations {
            backups.0 += 1;
        }
        a.index
    });
    let father = env.heap.model(model).map_or(0, |m| m.father.0);
    if let Some(m) = env.heap.model_mut(model) {
        m.elem_id = index;
    }
    if father != 0 {
        let f = find_elem_by_data(d, cm, father);
        elemrel::add_relation(d, &mut env.p, index, f, rel::PARENT, 1, 0);
    }
    index
}

/// Makes an element of the MODELS chapter stand for another node
/// [soft 0x08101a08 SI_elemdictReplaceModel]: the element's name record takes the node's
/// name (with `new_name`, a name no element has yet, which the node takes too) and its full
/// name is rebuilt; the node is told the element's index; the element's object becomes the
/// node. Nothing for a slot that is freed or out of range. (With `new_name` the original
/// stores the new name's static buffer in the record and then overwrites the pointer with a
/// copy of the node's name: the result is the same string.)
pub fn replace_model(d: &mut Dict, env: &mut Env, names: &mut NewName, index: i32, model: u32, new_name: bool) {
    let cm = env.p.cid.model;
    if model == 0 || index < 0 || get_elem(d, cm, index).is_none() {
        return;
    }
    if d.node_of(cm, index).is_some() {
        d.node_of_mut(cm, index).unwrap().name = None;
        if new_name {
            let n = get_new_name(d, names, cm, env.heap.model_name(model));
            model_change_name(&mut env.heap, model, &n);
        }
        let name = env.heap.model_name(model).map(|s| s.to_vec());
        let loading = env.p.loading;
        let node = d.node_of_mut(cm, index).unwrap();
        node.name = name;
        node.full = Some(si_db::format_full_name(node, loading));
    }
    if let Some(m) = env.heap.model_mut(model) {
        m.elem_id = index;
    }
    d.ed.lists[cm as usize].elems[index as usize].rec.data = Ptr32::new(model);
}
