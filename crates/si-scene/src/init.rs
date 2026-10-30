// SPDX-License-Identifier: MIT
//! Start-up of the dictionaries (`init_dict.c`, `reldict.c`, `elemdict.c`).

use crate::dict::{node_ptr, Dict, Program};
use si_ascii::srel;
use si_db::{format_full_name, ListNode};
use si_types::dict::DbListNode;
use si_types::{Opaque, Ptr32};

/// Adds an element to a chapter and returns its index (`SI_elemdictAddElem`, ported in
/// `si-types`), on the dictionary with its name records: `node` is the element's name record (null when
/// `None`). The port of the routine itself is `si-types`'; see there for the slot reuse.
///
/// When the hierarchy is clean and the model relations are being backed up, the original
/// saves them at this point (`SI_edAllocMdlRelBackUp`, `SI_edMdlRelBackUp`, of `edtools.c`,
/// not ported): the second value says so.
pub fn add_elem(d: &mut Dict, p: &Program, chap: i32, node: Option<ListNode>, data: Ptr32<Opaque>, aux: Ptr32<Opaque>, dsc_id: i32, work_id: i32) -> (i32, bool) {
    let handle: Ptr32<DbListNode> = match node {
        Some(n) => node_ptr(d.nodes.alloc(n)),
        None => Ptr32::NULL,
    };
    match d.ed.add_elem(chap, handle, data, aux, dsc_id, work_id, p.cid.model, p.backup_enabled, p.use_elem_list_sort_tbl, p.def_color) {
        Some(a) => (a.index, a.backup_model_relations),
        None => {
            // the original indexes the chapter array unchecked
            d.nodes.free(handle.0);
            (-1, false)
        }
    }
}

/// The scene the program starts with: one element in SCENES with an empty prefix, the name
/// `no_name`, version 1-0, and -1 as its number [soft 0x081369cc SI_elemdictCreateDefaultDsc].
/// Nothing is added when the chapter already holds an element (deleted slots count).
/// Returns the index of the element: the new one's, or 0.
pub fn create_default_dsc(d: &mut Dict, p: &Program) -> i32 {
    let has = d.ed.lists.get(p.cid.dsc as usize).map_or(0, |l| l.rec.nb_elems);
    if has >= 1 {
        return 0;
    }
    let mut node = ListNode::allocate(p.now);
    node.prefix = Some(Vec::new());
    node.name = Some(b"no_name".to_vec());
    node.full = Some(format_full_name(&mut node, p.loading));
    add_elem(d, p, p.cid.dsc, Some(node), Ptr32::NULL, Ptr32::NULL, 0, -1).0
}

/// What reading the relations resource file leaves behind.
pub struct Start {
    pub dict: Dict,
    pub program: Program,
    /// The file as `si-ascii` read it (operators, attributes, enumerations).
    pub relations: srel::Relations,
    pub info: srel::ReadInfo,
}

/// The dictionaries as the program has them once it has started: the scene dictionary with
/// its 36 chapters and the default scene, and the relation dictionary read from
/// `Relations.rsrc` (`rsrc` is the file's content).
///
/// This is the part of `SI_initDictionaries` (0x0810824c) and `SI_dbLoadRelationDictionary`
/// (0x0813ea84) that concerns the scene dictionary: initialise `SI_RD`, add the first
/// dictionary, load its chapters into `SI_cid`, create the default description, read the
/// resource file. The rules are added to the relation dictionary in the order and with the
/// arguments of the reader's calls of `SI_reldictAddRelation` (`si-ascii` keeps them in
/// that order); a chapter name the file uses beyond the 36 becomes a new chapter, as the
/// reader makes it. Not done here: the database dictionary `SI_dbed`, the list of
/// databases, and `SI_attdictSetFctsPointers` (the get and set routines of attribute
/// groups, which belong to the packages that own the objects).
///
/// `Err` when the original would not come back from the reader (see `srel::Error`).
pub fn start(rsrc: &[u8]) -> Result<Start, srel::Error> {
    let (mut dict, mut program) = empty_scene();
    create_default_dsc(&mut dict, &program);
    let (relations, info) = srel::read(rsrc)?;
    for name in relations.chapters.iter().skip(36) {
        dict.ed.add_list(name);
    }
    for r in &relations.relations {
        program.rd.add_relation(
            &r.name,
            r.id,
            r.src_chapter as i32,
            r.dst_chapter as i32,
            r.src_group as i32,
            r.dst_group as i32,
            r.rank as i32,
            r.fanout as i32,
            r.dope_display,
            r.upd[0] as i32,
            r.upd[1] as i32,
            None,
            r.render,
            r.incmode,
        );
    }
    Ok(Start { dict, program, relations, info })
}

/// Start-up of the dictionaries in the image: what `SI_initDictionaries` (0x0810824c) does
/// with them, in its order. `SI_reldictInit(&SI_RD)`; no dictionary yet; `SI_elemdictAdd`
/// makes the scene dictionary, whose index becomes the first word of the per-process data
/// (the second is cleared); `SI_elemdictInit` once more; `SI_elemdictLoadChapters` with
/// `SI_cid`; `SI_elemdictCreateDefaultDsc`; the relation dictionary is read
/// (`SI_dbLoadRelationDictionary`, 0x0813ea84); then the database browser's dictionary
/// `SI_dbed` is initialised and gets its chapters with `SI_dbcid`.
///
/// The five dictionary routines are `si-dict`'s. The resource reader is not on the image:
/// `si-ascii` reads `rsrc` into values, and what the reader does to the dictionaries is
/// applied from them here, in the order of its calls (a chapter name beyond the 36
/// becomes a new chapter under a copy of the name; each rule is added by
/// `SI_reldictAddRelation` with a copy of its name). Not done: the list of databases,
/// `SI_threadStorageInit`. The enum and the attribute dictionary (`SI_EVD`, `SI_ATD`) are
/// filled the same way ([`put_enums`], [`put_attributes`]), the curve index is built by
/// `SI_attdictSortAttribFromFcurveIds`, the attributes get their paths
/// (`SI_attdictUpdateIndexes`) and the groups their functions' addresses
/// (`SI_attdictSetFctsPointers`).
///
/// `Err` when the original would not come back from the reader (see `srel::Error`).
// adapter: not in place (the reader of `Relations.rsrc`)
pub fn start_image(m: &mut si_mem::Mem, o: &mut dyn si_dict::Outside, rsrc: &[u8]) -> Result<(si_mem::Ptr32<si_types::dict::ElemDict>, srel::Relations, srel::ReadInfo), srel::Error> {
    use si_dict::globals::{SI_CID, SI_DBCID, SI_DBED, SI_ELEM_DICTS, SI_NB_ELEM_DICTS, SI_PROC_DATA, SI_RD};
    use si_dict::{elemdict, elemmisc, elemname, reldict};
    reldict::init(m, SI_RD);
    m[SI_NB_ELEM_DICTS] = 0;
    let index = elemmisc::add(m);
    let proc = m[SI_PROC_DATA];
    m[proc] = index;
    m[proc.add(1)] = 0;
    let ed = m[SI_ELEM_DICTS].add(index);
    elemdict::init(m, ed);
    elemmisc::load_chapters(m, o, ed, SI_CID, 1);
    elemname::create_default_dsc(m, o, ed);
    let (relations, info) = srel::read(rsrc)?;
    for name in relations.chapters.iter().skip(36) {
        let copy = crate::dbnode::new_string(m, name);
        elemdict::add_list(m, ed, copy);
    }
    for r in &relations.relations {
        crate::world::with_text(m, &r.name, |m, name| {
            reldict::add_relation(
                m,
                SI_RD,
                name,
                r.id,
                r.src_chapter as i32,
                r.dst_chapter as i32,
                r.src_group as i32,
                r.dst_group as i32,
                r.rank as i32,
                r.fanout as i32,
                r.dope_display,
                r.upd[0] as i32,
                r.upd[1] as i32,
                1,
                si_mem::Ptr32::NULL,
                r.render,
                r.incmode,
            )
        });
    }
    // `SI_reldictReadRelations` (0x0813e318), for a file of the three-section layout:
    // the enum and the attribute dictionary are initialised, the reader fills them, the
    // curve index is built, the attributes get their paths. Then, for either layout,
    // `SI_dbLoadRelationDictionary` installs the groups' functions.
    if info.layout == srel::Layout::Sections {
        si_dict::enmdict::init(m, si_dict::globals::SI_EVD);
        attdict::init(m, SI_ATD);
        put_enums(m, &relations);
        put_attributes(m, &relations);
        attdict::sort_attrib_from_fcurve_ids(m, SI_ATD);
        attdict::update_indexes(m, SI_ATD);
    }
    attdict::set_fcts_pointers(m, SI_ATD);
    elemdict::init(m, SI_DBED);
    elemmisc::load_chapters(m, o, SI_DBED, SI_DBCID, 0);
    Ok((ed, relations, info))
}

use si_dict::attdict;
use si_dict::globals::SI_ATD;

/// What the reader of `Relations.rsrc` hands the enum dictionary, applied from
/// `si-ascii`'s values: `si_enmdictAddEnumType` for each `ENUM` block and
/// `si_enmdictAddValue` for each of its lines, with copies of the texts. (Part of the
/// adapter [`start_image`]: the order of the blocks is this function's.)
fn put_enums(m: &mut si_mem::Mem, relations: &srel::Relations) {
    use si_dict::enmdict;
    use si_dict::globals::SI_EVD;
    for (t, ty) in relations.enums.iter().enumerate() {
        let name = crate::dbnode::new_string(m, &ty.name);
        enmdict::add_enum_type(m, SI_EVD, name);
        for v in &ty.values {
            let code = crate::dbnode::new_string(m, &v.code);
            let name = crate::dbnode::new_string(m, &v.name);
            let slot = m[SI_EVD].types.add(t as i32);
            enmdict::add_value(m, slot, v.id, code, name);
        }
    }
}

/// The group of `SI_ATD` a path of indices leads to.
fn group_at(m: &si_mem::Mem, path: &[usize]) -> si_mem::Ptr32<si_types::dict::AttribGroup> {
    let mut g = m[SI_ATD].groups.add(path[0] as i32);
    for &i in &path[1..] {
        g = m[g].subs.add(i as i32);
    }
    g
}

/// A condition as the record the attribute dictionary reads and frees (`SI_AttribCond`:
/// 12 bytes and its array of numbers); 0 for none.
fn put_cond(m: &mut si_mem::Mem, cond: &Option<srel::AttribCond>) -> u32 {
    let Some(c) = cond else { return 0 };
    let rec: si_mem::Ptr32<si_dict::records::AttribCond> = si_mem::Ptr32::new(m.calloc(1, 0xc));
    let values: si_mem::Ptr32<i32> = si_mem::Ptr32::new(m.calloc(c.values.len().max(1) as u32, 4));
    for (k, v) in c.values.iter().enumerate() {
        m[values.add(k as i32)] = *v;
    }
    m[rec].attr = c.attribute;
    m[rec].values = values;
    m[rec].nb = c.values.len() as i32;
    rec.addr()
}

/// What the reader of `Relations.rsrc` hands the attribute dictionary, applied from
/// `si-ascii`'s values with `si-dict`'s routines: every group (`si_attdictAddAttribGroup`,
/// `si_attdictAddAttribSubGroup`, with a copy of its name and its condition as a record),
/// then every attribute in the order of the dictionary's one array: a record on the
/// stack with the initial values (`si_attdictInitAttribute`), the line's columns, its
/// components (`si_attdictAllocateFcurveArray`, the ids and copies of the names), added
/// to its group by `si_attdictAddAttribute`.
///
/// Part of the adapter [`start_image`]: the reader makes the groups as it meets them,
/// between the attributes, so the order of the blocks here is this function's; what the
/// records hold is the reader's. An attribute no group lists (none in the program's
/// file) is added through a group of its own on the stack, to keep its place in the array.
fn put_attributes(m: &mut si_mem::Mem, relations: &srel::Relations) {
    use si_mem::Ptr32;
    use si_types::dict::{AttribGroup, Attribute};
    fn groups(m: &mut si_mem::Mem, path: &mut Vec<usize>, list: &[srel::AttribGroup], owner: &mut Vec<Option<Vec<usize>>>) {
        for (i, g) in list.iter().enumerate() {
            let name = crate::dbnode::new_string(m, &g.name);
            let cond = put_cond(m, &g.cond);
            if path.is_empty() {
                attdict::add_attrib_group(m, SI_ATD, name, cond, g.rel_type);
            } else {
                let parent = group_at(m, path);
                attdict::add_attrib_sub_group(m, parent, name, cond);
            }
            path.push(i);
            for &a in &g.attributes {
                if let Some(slot) = owner.get_mut(a) {
                    *slot = Some(path.clone());
                }
            }
            groups(m, path, &g.subgroups, owner);
            path.pop();
        }
    }
    let mut owner: Vec<Option<Vec<usize>>> = vec![None; relations.attributes.len()];
    groups(m, &mut Vec::new(), &relations.groups, &mut owner);
    for (i, a) in relations.attributes.iter().enumerate() {
        let local: Ptr32<Attribute> = Ptr32::new(m.local(100));
        attdict::init_attribute(m, local);
        m[local].unk_00 = a.owner_kind;
        m[local].unk_04 = a.owner_id;
        m[local].id = a.id;
        m[local].name = crate::dbnode::new_string(m, &a.name);
        m[local].description = match &a.description {
            Some(d) => crate::dbnode::new_string(m, d),
            None => Ptr32::NULL,
        };
        m[local].access = a.access;
        m[local].scope = a.scope;
        m[local].value_type = a.kind;
        m[local].enum_type = a.enum_type;
        m[local].range_low = [a.min.to_bits(), a.min_kind as u32];
        m[local].range_low_flag = a.min_inclusive;
        m[local].range_high = [a.max.to_bits(), a.max_kind as u32];
        m[local].range_high_flag = a.max_inclusive;
        m[local].group = a.group;
        m[local].rank = a.rank;
        m[local].incmode = a.incmode;
        m[local].visible = a.count;
        m[local].unk_60 = a.dope_display;
        attdict::allocate_fcurve_array(m, local, a.fcurves as i32);
        for k in 0..a.fcurves.max(0) as usize {
            if let Some(&id) = a.fcurve_ids.get(k) {
                let ids = m[local].fcv_ids;
                m[ids.add(k as i32)] = id;
            }
            if let Some(Some(name)) = a.fcurve_names.get(k) {
                let copy = crate::dbnode::new_string(m, name);
                let names = m[local].fcv_names;
                m[names.add(k as i32)] = copy;
            }
        }
        match &owner[i] {
            Some(path) => {
                let g = group_at(m, path);
                attdict::add_attribute(m, SI_ATD, g, local);
            }
            None => {
                let g: Ptr32<AttribGroup> = Ptr32::new(m.local(0x28));
                attdict::init_attrib_group(m, g);
                attdict::add_attribute(m, SI_ATD, g, local);
                let list = m[g].attribs;
                m.free(list.addr());
                m.release(0x28);
            }
        }
        m.release(100);
    }
}

/// A scene dictionary with its 36 chapters and nothing in them, and the globals around it
/// with an empty relation dictionary.
pub fn empty_scene() -> (Dict, Program) {
    let mut d = Dict::new();
    let mut p = Program::new();
    d.ed.load_chapters(&mut p.cid, 1, None);
    (d, p)
}

/// Adds an element named by a file name without suffix (`prefix-name.version-subst`), the
/// way the description reader names one.
pub fn add_named(d: &mut Dict, p: &Program, chap: i32, full_name: &[u8]) -> i32 {
    let node = si_db::scn_get_list_node(full_name, None, false, p.now);
    add_elem(d, p, chap, Some(node), Ptr32::NULL, Ptr32::NULL, 0, 0).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_scene() {
        let (mut d, p) = empty_scene();
        assert_eq!(d.ed.lists.len(), 36);
        assert_eq!(create_default_dsc(&mut d, &p), 0);
        let n = d.node_of(p.cid.dsc, 0).unwrap();
        assert_eq!(n.full.as_deref(), Some(&b"no_name.1-0"[..]));
        let e = &d.ed.lists[p.cid.dsc as usize].elems[0].rec;
        assert_eq!((e.work_id, e.new_flag, e.status), (-1, 2, 0));
        // a second call adds nothing
        assert_eq!(create_default_dsc(&mut d, &p), 0);
        assert_eq!(d.ed.lists[p.cid.dsc as usize].elems.len(), 1);
    }
}
