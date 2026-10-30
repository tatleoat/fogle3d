// SPDX-License-Identifier: MIT
//! The element dictionaries with their storage (`elemdict.c`, `relations.c`): creating
//! dictionaries, chapters and elements, finding them, and the relation lists of an element.
//!
//! The original keeps each array in a `malloc`ed block that the record points to. Here the
//! record (`rec`, the layout of [`crate::dict`]) sits beside a `Vec`, and the record's count
//! is kept equal to the `Vec`'s length. The pointers an element carries to things this crate
//! does not own (its name record, its object, its graf node, its icon) stay opaque
//! [`Ptr32`] values supplied by the caller.

use crate::dict::{
    ARelation, ChapIds, DbListNode, Elem, ElemDict, ElemIcon, ElemIdentity, ElemList, ListOptions, RelList, Relations,
    UpdList, CHAPTERS, CHAPTER_ID_SLOT, ELEM_DELETED, ELEM_VALID,
};
use crate::reldict::{RelDictStore, SortTable};
use crate::{Opaque, Ptr32, Zero};

impl UpdList {
    /// [soft 0x08141468 SI_updlistInit]
    pub fn init(&mut self) {
        self.nb = 0;
        self.nodes = Ptr32::NULL;
        self.dirty = 1;
    }
}

impl ElemDict {
    /// [soft 0x081359bc SI_elemdictInit]. The second copy of the options is a copy of the
    /// first, padding included.
    pub fn init(&mut self) {
        self.nb_lists = 0;
        self.lists = Ptr32::NULL;
        self.chap_sort = Ptr32::NULL;
        self.is_scene = 1;
        self.dbinfo = Ptr32::NULL;
        let pad = self.options.pad_16;
        self.options = ListOptions {
            sort_type: 6,
            unk_04: 3,
            unk_06: 0,
            unk_08: 1,
            unk_0a: 0,
            unk_0c: -1,
            unk_10: 0,
            unk_12: 0,
            unk_14: 1,
            pad_16: pad,
        };
        self.options_db = self.options;
        self.upd_list.init();
        self.topo_list.init();
        self.rel_dirty = 1;
        self.nb_fwd_backup = 0;
        self.fwd_backup = Ptr32::NULL;
        self.nb_back_backup = 0;
        self.back_backup = Ptr32::NULL;
        self.untold = 0;
        self.hier_dirty = 1;
    }
}

impl Elem {
    /// [soft 0x081326b0 SI_elemdictInitElem]. `def_color` is what `SI_grafnodeDefColor`
    /// (0x0832bff4) returns: the sum of two globals, `SI_select7WM + SI_color1`. Every
    /// field is set.
    pub fn init(&mut self, def_color: i32) {
        *self = Elem::zero();
        self.wire_color = def_color;
        self.wire_type = 2;
        self.dirty = 1;
        self.new_flag = 2;
    }

    /// [soft 0x08132be8 SI_elemdictSetDirty]
    pub fn set_dirty(&mut self, dirty: i32) {
        self.dirty = dirty as i16;
    }

    /// [soft 0x08132bfc SI_elemdictSchemSetPosX]
    pub fn schem_set_pos_x(&mut self, x: f32) {
        self.schem_x = x;
    }

    /// [soft 0x08132c0c SI_elemdictSchemSetPosY]
    pub fn schem_set_pos_y(&mut self, y: f32) {
        self.schem_y = y;
    }

    /// [soft 0x08132c1c si_elemdictSetFlag]
    pub fn set_flag(&mut self, flag: i16) {
        self.work_flag = flag as i32;
    }
}

impl ElemList {
    /// [soft 0x08133bc8 SI_elemlistInit]. It leaves `pad_1a` as it was.
    pub fn init(&mut self) {
        let pad = self.pad_1a;
        *self = ElemList::zero();
        self.pad_1a = pad;
        self.kind = 1;
    }
}

/// True when two identities name the same element
/// [soft 0x08137c9c SI_elemdictCompareElemIdent]; false when either is missing.
pub fn compare_elem_ident(a: Option<&ElemIdentity>, b: Option<&ElemIdentity>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.chapid == b.chapid && a.elemid == b.elemid,
        _ => false,
    }
}

/// One relation list with its relations.
#[derive(Clone, Debug, PartialEq)]
pub struct RelListStore {
    /// `nb` always equals `rels.len()`.
    pub rec: RelList,
    pub rels: Vec<ARelation>,
}

impl RelListStore {
    /// An empty list as `si_relAllocate` leaves it.
    fn new() -> Self {
        let mut rec = RelList::zero();
        rec.flag = 1;
        RelListStore { rec, rels: Vec::new() }
    }
}

/// The six relation lists of one direction, as [soft 0x081392a0 si_relAllocate] creates
/// them: empty, flag 1. The original does not set the padding of each list; it is zero.
pub fn rel_allocate() -> Vec<RelListStore> {
    (0..6).map(|_| RelListStore::new()).collect()
}

/// Position of the first relation with this id in list `group`, or -1
/// [soft 0x08139b24 si_relCheck]. The original does not check `group` against the number
/// of lists; here a list that does not exist holds nothing.
pub fn rel_check(lists: &[RelListStore], group: i32, relid: i32) -> i32 {
    let Some(list) = usize::try_from(group).ok().and_then(|g| lists.get(g)) else { return -1 };
    list.rels.iter().position(|r| r.relid == relid).map_or(-1, |i| i as i32)
}

/// What [`rel_add_relation`] is given besides the list.
#[derive(Clone, Copy, Debug)]
pub struct NewRelation {
    pub relid: i32,
    /// Chapter and index of the element at the other end.
    pub chapid: i32,
    pub elemid: i32,
    /// List, on the other element, that holds the matching entry.
    pub group: i32,
    pub rank: i32,
    /// The rule's fan-out (the sixth output of `SI_reldictGetRelAttributes`).
    pub fanout: i32,
    /// Ninth argument of the original: with a fan-out of 2 or more, the insertion stops
    /// once this many relations of the same kind have been passed.
    pub limit: i32,
    /// -1, or a serial id.
    pub serial: i32,
    /// Eleventh argument: when set, an entry equal to its predecessor is refused once the
    /// fan-out is reached.
    pub check_fanout: bool,
    pub data: Ptr32<Opaque>,
    pub flag: i16,
}

/// Inserts a relation into a list, keeping the list ordered by rank
/// [soft 0x0813987c si_relAddRelation]. Returns false, as the original returns 0, when the
/// relation is refused: it is already there for a single-rule kind, or the fan-out is
/// reached. The list's capacity is counted as the original counts it: it grows by one,
/// before the checks, when the list is full.
///
/// Relation ids 20000, 10000, 20010, 246, 247 and 551 are exempt from the duplicate check,
/// and 20000 is always treated as having one rule.
pub fn rel_add_relation(rd: &mut RelDictStore, list: &mut RelListStore, n: &NewRelation) -> bool {
    let nb_before = list.rec.nb;
    // the original grows the array when it has none (an empty list) or when it is full
    if list.rels.is_empty() || nb_before == list.rec.capacity {
        list.rec.capacity += 1;
    }
    let mut at = 0i32;
    if nb_before >= 1 {
        let mut same_kind = 0;
        let mut seen_kind = false;
        let mut seen_serial = false;
        let nb_rules = if n.relid == 20000 { 1 } else { rd.get_rel_nb_rules(n.relid) };
        let nb = list.rec.nb;
        'scan: loop {
            let r = list.rels[at as usize];
            if n.rank < r.rank {
                break 'scan;
            }
            let mut check_serial = n.serial != -1;
            if n.rank == r.rank {
                if n.relid == r.relid && n.chapid == r.chapid as i32 && n.group == r.group as i32 {
                    same_kind += 1;
                    seen_kind = true;
                    if nb_rules == 1 && n.elemid == r.elemid && ![20000, 10000, 0x4e2a, 0xf6, 0xf7, 0x227].contains(&n.relid) {
                        return false;
                    }
                    if !(n.fanout < 2 || n.limit < 1 || same_kind < n.limit) {
                        break 'scan;
                    }
                } else if n.serial != -1 {
                    check_serial = true;
                } else {
                    if n.relid != r.relid {
                        if seen_kind {
                            break 'scan;
                        }
                    } else {
                        seen_kind = true;
                        if n.chapid < r.chapid as i32 {
                            break 'scan;
                        }
                    }
                    check_serial = false;
                }
            }
            if check_serial {
                if r.serial == n.serial {
                    seen_serial = true;
                } else if seen_serial || (r.unk_16 != 0 && n.rank == r.rank) {
                    break 'scan;
                }
            }
            at += 1;
            if nb <= at {
                break 'scan;
            }
        }
        if n.check_fanout && at > 0 {
            let p = list.rels[at as usize - 1];
            if n.rank == p.rank && n.relid == p.relid && n.chapid == p.chapid as i32 && n.group == p.group as i32
                && n.fanout <= same_kind
            {
                return false;
            }
        }
    }
    let mut r = ARelation::zero();
    r.relid = n.relid;
    r.chapid = n.chapid as i16;
    r.elemid = n.elemid;
    r.group = n.group as i16;
    r.flag = n.flag;
    r.data = n.data;
    r.active_elem = -1;
    r.serial = n.serial;
    r.rank = n.rank;
    list.rels.insert(at as usize, r);
    list.rec.nb += 1;
    true
}

/// One element with its relation lists.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemStore {
    /// `forward.nb_groups` and `backward.nb_groups` always equal the lengths below.
    pub rec: Elem,
    pub forward: Vec<RelListStore>,
    pub backward: Vec<RelListStore>,
}

impl ElemStore {
    fn relations(&self, forward: bool) -> &Vec<RelListStore> {
        if forward { &self.forward } else { &self.backward }
    }

    /// The cursor of list `group` on the forward side (`forward`) or the backward side
    /// [soft 0x08138224 SI_elemdictGetCurRelationIndex]; -1 when the element has no lists
    /// on that side. The original does not check `group`; a list that does not exist
    /// answers -1 here.
    pub fn get_cur_relation_index(&self, forward: bool, group: i32) -> i32 {
        usize::try_from(group).ok().and_then(|g| self.relations(forward).get(g)).map_or(-1, |l| l.rec.cur)
    }

    /// [soft 0x0813825c SI_elemdictSetCurRelationIndex]
    pub fn set_cur_relation_index(&mut self, forward: bool, group: i32, value: i32) {
        let lists = if forward { &mut self.forward } else { &mut self.backward };
        if let Some(l) = usize::try_from(group).ok().and_then(|g| lists.get_mut(g)) {
            l.rec.cur = value;
        }
    }

    /// True when another element refers to this one: any backward list holds a relation;
    /// with `ignore_two`, relations 110 and 207 in the second list do not count
    /// [soft 0x08136d10 SI_elemdictIsElemUsed]. The original reads six backward lists
    /// without checking that the element has them; here an element without lists is unused.
    pub fn is_elem_used(&self, ignore_two: bool) -> bool {
        let mut used = false;
        let mut i = 0;
        while i < 6 && !used {
            let Some(list) = self.backward.get(i) else { break };
            if i == 1 && ignore_two {
                used = list.rels.iter().any(|r| r.relid != 0x6e && r.relid != 0xcf);
                i = 2;
            } else {
                used = list.rec.nb > 0;
                i += 1;
            }
        }
        used
    }
}

/// One chapter with its elements.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemListStore {
    /// `nb_elems` always equals `elems.len()`.
    pub rec: ElemList,
    pub name: Vec<u8>,
    pub suffix: Option<Vec<u8>>,
    pub elems: Vec<ElemStore>,
    /// Only with the load optimisation: one slot per element, -1 until the table is built.
    pub sort_tbl: Option<SortTable>,
}

/// What disposing of elements frees besides the arrays: the name records and icons the
/// elements pointed to, for the caller who owns them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Freed {
    pub nodes: Vec<Ptr32<DbListNode>>,
    pub icons: Vec<Ptr32<ElemIcon>>,
    /// The per-element suffix strings (`Elem::suffix`).
    pub suffixes: Vec<Ptr32<u8>>,
}

impl ElemListStore {
    fn new(name: &[u8]) -> Self {
        let mut rec = ElemList::zero();
        rec.init();
        ElemListStore { rec, name: name.to_vec(), suffix: None, elems: Vec::new(), sort_tbl: None }
    }

    /// Adds an element and returns its index [soft 0x081327c4 SI_elemlistAddElem].
    ///
    /// With `reuse`, the first deleted slot is taken instead of growing the array. With
    /// `alloc_rel`, a new slot gets its two sets of six relation lists, and a reused slot
    /// keeps the lists it has; without it the element has no lists (the original then
    /// drops a reused slot's lists without freeing them).
    ///
    /// `load_optim` is what `SI_getLoadOptim` (0x080d94b0) returns: whether the
    /// environment variable `SI_LOAD_OPTIM` was set when first asked. With it,
    /// the chapter's sorted table is created or marked stale and grows with the array.
    ///
    /// The original does not set the sequence fields (`seq_first` to `seq_braces`): a
    /// reused slot keeps the previous element's, as here; a new slot has whatever the heap
    /// held, and zero here.
    #[allow(clippy::too_many_arguments)]
    pub fn add_elem(
        &mut self, node: Ptr32<DbListNode>, data: Ptr32<Opaque>, aux_data: Ptr32<Opaque>, dsc_id: i32, work_id: i32,
        alloc_rel: bool, reuse: bool, load_optim: bool, def_color: i32,
    ) -> i32 {
        if load_optim {
            self.sort_tbl.get_or_insert_with(SortTable::default).valid = 0;
        }
        let reused = if reuse { self.elems.iter().position(|e| e.rec.status == ELEM_DELETED) } else { None };
        let index = match reused {
            Some(i) => i,
            None => {
                self.elems.push(ElemStore { rec: Elem::zero(), forward: Vec::new(), backward: Vec::new() });
                if load_optim {
                    self.sort_tbl.as_mut().unwrap().table.push(-1);
                }
                self.elems.len() - 1
            }
        };
        let e = &mut self.elems[index];
        let (seq_first, seq_last, seq_unknown, seq_braces) =
            (e.rec.seq_first, e.rec.seq_last, e.rec.seq_unknown, e.rec.seq_braces);
        let (fwd, back) = (e.rec.forward, e.rec.backward);
        e.rec.init(def_color);
        e.rec.node = node;
        e.rec.data = data;
        e.rec.aux_data = aux_data;
        e.rec.dsc_id = dsc_id;
        e.rec.root_id = index as i32;
        e.rec.work_id = work_id;
        e.rec.seq_first = seq_first;
        e.rec.seq_last = seq_last;
        e.rec.seq_unknown = seq_unknown;
        e.rec.seq_braces = seq_braces;
        if !alloc_rel {
            e.forward.clear();
            e.backward.clear();
        } else if reused.is_none() {
            e.forward = rel_allocate();
            e.backward = rel_allocate();
            e.rec.forward.nb_groups = 6;
            e.rec.backward.nb_groups = 6;
        } else {
            e.rec.forward = fwd;
            e.rec.backward = back;
        }
        if reused.is_none() {
            self.rec.nb_elems += 1;
        }
        index as i32
    }

    /// The element at `index`, unless the index is out of range or the slot is deleted
    /// [soft 0x08132b1c SI_elemlistGetElem].
    pub fn get_elem(&self, index: i32) -> Option<&ElemStore> {
        let e = self.elems.get(usize::try_from(index).ok()?)?;
        (e.rec.status != ELEM_DELETED).then_some(e)
    }

    pub fn get_elem_mut(&mut self, index: i32) -> Option<&mut ElemStore> {
        let e = self.elems.get_mut(usize::try_from(index).ok()?)?;
        (e.rec.status != ELEM_DELETED).then_some(e)
    }

    /// Number of slots in use [soft 0x08133cc0 SI_elemlistCountValidElems].
    pub fn count_valid_elems(&self) -> i32 {
        self.elems.iter().filter(|e| e.rec.status == ELEM_VALID).count() as i32
    }

    /// Marks the sorted table stale [soft 0x0813425c SI_dbSortTblSetDirty].
    pub fn sort_tbl_set_dirty(&mut self) {
        if let Some(t) = &mut self.sort_tbl {
            t.valid = 0;
        }
    }

    /// Frees the elements and what hangs off them [soft 0x08135ca0
    /// SI_elemlistDisposeElements]. With `keep_selected`, the name record of an element
    /// whose `selected` word is non-zero is not freed (and the pointer to it is lost).
    /// The chapter keeps its name, its kind, its sub-directory and its suffix.
    pub fn dispose_elements(&mut self, keep_selected: bool, freed: &mut Freed) {
        for e in &self.elems {
            if !e.rec.icon.is_null() {
                freed.icons.push(e.rec.icon);
            }
            if !e.rec.suffix.is_null() {
                freed.suffixes.push(e.rec.suffix);
            }
            if (!keep_selected || e.rec.selected == 0) && !e.rec.node.is_null() {
                freed.nodes.push(e.rec.node);
            }
        }
        self.elems.clear();
        self.sort_tbl = None;
        self.rec.nb_elems = 0;
        self.rec.elems = Ptr32::NULL;
        self.rec.sort = Ptr32::NULL;
        self.rec.hier_order = Ptr32::NULL;
        self.rec.hier_count = Ptr32::NULL;
        self.rec.subdirs = Ptr32::NULL;
        self.rec.nb_subdirs = 0;
        self.rec.subdir_aux = Ptr32::NULL;
        self.rec.sort_tbl = Ptr32::NULL;
    }

    /// [soft 0x08135fd4 SI_elemlistDispose]: the elements, then the sub-directory and the
    /// suffix strings. The chapter's name is not freed.
    pub fn dispose(&mut self, keep_selected: bool, freed: &mut Freed) {
        self.dispose_elements(keep_selected, freed);
        self.rec.subdir = Ptr32::NULL;
        self.rec.suffix = Ptr32::NULL;
        self.suffix = None;
    }
}

/// The row of the chapter table that belongs to a chapter index
/// [soft 0x08120794 SI_chapInfoOffsetFromChapId]. The index is compared with the fields of
/// the *scene's* chapter ids in a fixed order, so the answer depends on those, and two
/// quirks follow once they are loaded: PICTURES answers with the row of RENDER_PICTURES,
/// and NSELECTIONS, which the chain does not test, has no row.
pub fn chap_info_offset_from_chap_id(cid: &ChapIds, chap: i32) -> Option<usize> {
    let chain: [(i32, usize); 35] = [
        (cid.model, 8),
        (cid.ani, 1),
        (cid.cam, 2),
        (cid.shp, 0xf),
        (cid.cls, 3),
        (cid.lig, 6),
        (cid.lat, 5),
        (cid.mat, 7),
        (cid.t2d, 0x11),
        (cid.t3d, 0x12),
        (cid.dsc, 0),
        (cid.pal, 10),
        (cid.wav, 0x13),
        (cid.exp, 0x14),
        (cid.sts, 0xc),
        (cid.stc, 0xd),
        (cid.sfk, 0xe),
        (cid.sky, 0x10),
        (cid.img, 0xb),
        (cid.rim, 0xb),
        (cid.chn_drv, 0x1e),
        (cid.out, 0x1f),
        (cid.spd, 0x20),
        (cid.audio, 0x21),
        (cid.shd_mat, 0x15),
        (cid.shd_mat_photon, 0x22),
        (cid.shd_sha, 0x16),
        (cid.shd_vol, 0x17),
        (cid.shd_t2d, 0x18),
        (cid.shd_t3d, 0x19),
        (cid.shd_lig, 0x1a),
        (cid.shd_lig_photon, 0x23),
        (cid.shd_cam, 0x1b),
        (cid.shd_out, 0x1c),
        (cid.shd_env, 0x1d),
    ];
    chain.iter().find(|(id, _)| *id == chap).map(|(_, row)| *row)
}

/// The file suffix of a chapter [soft 0x08120a48 SI_edGetSuffixFromChapter]: the suffix of
/// its row of the chapter table, or `???` when it has no row.
pub fn get_suffix_from_chapter(cid: &ChapIds, chap: i32) -> &'static [u8] {
    match chap_info_offset_from_chap_id(cid, chap) {
        Some(row) => CHAPTERS[row].1.as_bytes(),
        None => b"???",
    }
}

/// What [`ElemDictStore::add_elem`] reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Added {
    pub index: i32,
    /// The original, at this point, saves the model relations (`SI_edAllocMdlRelBackUp`
    /// then `SI_edMdlRelBackUp`, of `edtools.c`) and does not mark the hierarchy dirty.
    /// Those two belong to another package: the caller runs them when this is set.
    pub backup_model_relations: bool,
}

/// One dictionary with its chapters.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemDictStore {
    /// `nb_lists` always equals `lists.len()`.
    pub rec: ElemDict,
    pub lists: Vec<ElemListStore>,
    /// Chapter indices ordered by name, once [`ElemDictStore::sort_chap_names`] has run.
    pub chap_sort: Vec<i32>,
    /// Saved relations of model elements (`fwd_backup`, `back_backup`), filled by another
    /// package; kept here so that disposal can drop them.
    pub fwd_backup: Vec<Vec<RelListStore>>,
    pub back_backup: Vec<Vec<RelListStore>>,
}

impl Default for ElemDictStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ElemDictStore {
    /// An initialised, empty dictionary [soft 0x081359bc SI_elemdictInit].
    pub fn new() -> Self {
        let mut rec = ElemDict::zero();
        rec.init();
        ElemDictStore { rec, lists: Vec::new(), chap_sort: Vec::new(), fwd_backup: Vec::new(), back_backup: Vec::new() }
    }

    /// Index of the chapter with this name, or -1 [soft 0x081325f4
    /// SI_elemdictFindListByName]. Names are compared exactly, except that when the name
    /// asked for is PICTURES or RENDER_PICTURES in any letter case, a chapter matching it
    /// without regard to case is accepted too.
    pub fn find_list_by_name(&self, name: &[u8]) -> i32 {
        let any_case = name.eq_ignore_ascii_case(b"PICTURES") || name.eq_ignore_ascii_case(b"RENDER_PICTURES");
        self.lists
            .iter()
            .position(|l| l.name == name || (any_case && l.name.eq_ignore_ascii_case(name)))
            .map_or(-1, |i| i as i32)
    }

    /// Adds a chapter unless one has the name, and returns the chapter's index: the new
    /// one's, or the existing one's [soft 0x08133c40 SI_elemdictAddList].
    pub fn add_list(&mut self, name: &[u8]) -> i32 {
        let found = self.find_list_by_name(name);
        if found != -1 {
            return found;
        }
        let index = self.rec.nb_lists;
        self.lists.push(ElemListStore::new(name));
        self.rec.nb_lists += 1;
        index
    }

    /// Creates the 36 chapters, records each one's index in `cid`, and gives each its file
    /// suffix [soft 0x08133cf0 SI_elemdictLoadChapters].
    ///
    /// `is_scene` is 1 for a scene dictionary and 0 for the database dictionary; it is
    /// stored, and selects which copy of the browser options gets sort type 9.
    /// The suffixes are looked up through the *scene's* chapter ids, a global of the
    /// original: pass them as `scene_cid`, or `None` when `cid` is the scene's own (the
    /// original's first call, where the ids just written are the ones consulted).
    pub fn load_chapters(&mut self, cid: &mut ChapIds, is_scene: i16, scene_cid: Option<&ChapIds>) {
        let mut ids = [0i32; 36];
        for (row, (name, _, _)) in CHAPTERS.iter().enumerate() {
            ids[CHAPTER_ID_SLOT[row]] = self.add_list(name.as_bytes());
        }
        *cid = chap_ids_from_words(&ids);
        // chapters with no files of their own, and the one for files outside a database
        for (id, kind) in [(cid.nsl, 0), (cid.chn_drv, 0), (cid.out, 2)] {
            if let Some(l) = usize::try_from(id).ok().and_then(|i| self.lists.get_mut(i)) {
                l.rec.kind = kind;
            }
        }
        self.rec.is_scene = is_scene;
        if is_scene == 0 {
            self.rec.options.unk_08 = 0;
            self.rec.options_db.sort_type = 9;
            self.rec.options_db.unk_0a = 1;
        } else {
            self.rec.options.sort_type = 9;
            self.rec.options.unk_0a = 1;
        }
        let lookup: ChapIds = match scene_cid {
            Some(c) => *c,
            None => *cid,
        };
        for (i, l) in self.lists.iter_mut().enumerate() {
            l.suffix = Some(get_suffix_from_chapter(&lookup, i as i32).to_vec());
        }
    }

    /// [soft 0x08132b04 SI_elemdictGetChapName]. The original does not check the index.
    pub fn get_chap_name(&self, chap: i32) -> Option<&[u8]> {
        Some(&self.lists.get(usize::try_from(chap).ok()?)?.name)
    }

    /// Builds the list of chapter indices ordered by name [soft 0x0813259c
    /// SI_elemdictSortChapNames] [soft 0x08132564 elemdictCompareChapNames]. The original
    /// sorts with `qsort` and `strcmp`; chapter names are distinct, so the order of equal
    /// names, which `qsort` does not define, does not arise.
    pub fn sort_chap_names(&mut self) {
        if self.lists.is_empty() {
            return;
        }
        let mut order: Vec<i32> = (0..self.lists.len() as i32).collect();
        order.sort_by(|a, b| self.lists[*a as usize].name.cmp(&self.lists[*b as usize].name));
        self.chap_sort = order;
    }

    fn list(&self, chap: i32) -> Option<&ElemListStore> {
        self.lists.get(usize::try_from(chap).ok()?)
    }

    fn list_mut(&mut self, chap: i32) -> Option<&mut ElemListStore> {
        self.lists.get_mut(usize::try_from(chap).ok()?)
    }

    /// The element `index` of chapter `chap`, unless out of range or deleted
    /// [soft 0x08132b50 SI_elemdictGetElem]. The original does not check the chapter.
    pub fn get_elem(&self, chap: i32, index: i32) -> Option<&ElemStore> {
        self.list(chap)?.get_elem(index)
    }

    pub fn get_elem_mut(&mut self, chap: i32, index: i32) -> Option<&mut ElemStore> {
        self.list_mut(chap)?.get_elem_mut(index)
    }

    /// Adds an element to a chapter, reusing a deleted slot when there is one
    /// [soft 0x08132a5c SI_elemdictAddElem].
    ///
    /// Both update lists are marked dirty. Then, when the chapter is the models' and the
    /// hierarchy is clean and `backup_enabled` (a 16-bit global at 0x08781596, whose
    /// meaning is not read) and the new element has an object, the original saves the
    /// model relations: that is reported in the result and left to the caller. Otherwise
    /// the hierarchy is marked dirty.
    #[allow(clippy::too_many_arguments)]
    pub fn add_elem(
        &mut self, chap: i32, node: Ptr32<DbListNode>, data: Ptr32<Opaque>, aux_data: Ptr32<Opaque>, dsc_id: i32,
        work_id: i32, model_chap: i32, backup_enabled: bool, load_optim: bool, def_color: i32,
    ) -> Option<Added> {
        self.rec.upd_list.dirty = 1;
        self.rec.topo_list.dirty = 1;
        let hier_clean = self.rec.hier_dirty == 0;
        let index = self.list_mut(chap)?.add_elem(node, data, aux_data, dsc_id, work_id, true, true, load_optim, def_color);
        if chap == model_chap && hier_clean && backup_enabled && self.get_elem(chap, index).is_some_and(|e| !e.rec.data.is_null()) {
            return Some(Added { index, backup_model_relations: true });
        }
        self.rec.hier_dirty = 1;
        Some(Added { index, backup_model_relations: false })
    }

    /// [soft 0x08132c30 SI_elemlistClearWorkFlag]
    pub fn clear_work_flag(&mut self, chap: i32) {
        if let Some(l) = self.list_mut(chap) {
            l.elems.iter_mut().for_each(|e| e.rec.work_flag = 0);
        }
    }

    /// Clears the byte at +0x5b of every element of a chapter [soft 0x08132c70
    /// SI_elemlistClearLowByteWorkFlag]. That byte is the low byte of the flag on the
    /// big-endian machines the code was written for; on this build it is the high byte.
    pub fn clear_low_byte_work_flag(&mut self, chap: i32) {
        if let Some(l) = self.list_mut(chap) {
            l.elems.iter_mut().for_each(|e| e.rec.work_flag &= 0x00ff_ffff);
        }
    }

    /// ANDs the byte at +0x5b of every element of a chapter with `mask`
    /// [soft 0x08132cac SI_elemlistLowByteWorkFlagAND].
    pub fn low_byte_work_flag_and(&mut self, chap: i32, mask: i16) {
        if let Some(l) = self.list_mut(chap) {
            let m = ((mask as u8 as u32) << 24) | 0x00ff_ffff;
            l.elems.iter_mut().for_each(|e| e.rec.work_flag = (e.rec.work_flag as u32 & m) as i32);
        }
    }

    /// Sets `new_flag` to `to` in every element, of every chapter, whose flag is `from`
    /// (or in all of them when `from` is negative) [soft 0x08136c48 SI_elemdictSetNewFlag].
    /// Deleted slots are not skipped.
    pub fn set_new_flag(&mut self, from: i32, to: i32) {
        for e in self.lists.iter_mut().flat_map(|l| l.elems.iter_mut()) {
            if from < 0 || e.rec.new_flag as i32 == from {
                e.rec.new_flag = to as i16;
            }
        }
    }

    /// Sets `selected` in every element of every chapter [soft 0x08136cb0
    /// SI_elemdictSetSelection].
    pub fn set_selection(&mut self, selected: i16) {
        self.lists.iter_mut().flat_map(|l| l.elems.iter_mut()).for_each(|e| e.rec.selected = selected);
    }

    fn find_in(&self, chap: i32, test: impl Fn(&Elem) -> bool) -> i32 {
        self.list(chap).and_then(|l| l.elems.iter().position(|e| test(&e.rec))).map_or(-1, |i| i as i32)
    }

    /// First element in use whose second object is `aux_data`, or -1
    /// [soft 0x081345a0 SI_elemdictFindElemByAuxData].
    pub fn find_elem_by_aux_data(&self, chap: i32, aux_data: Ptr32<Opaque>) -> i32 {
        self.find_in(chap, |e| e.status == ELEM_VALID && e.aux_data == aux_data)
    }

    /// First slot not deleted whose object is `data`, or -1
    /// [soft 0x081345e8 SI_elemdictFindElemByDelData].
    pub fn find_elem_by_del_data(&self, chap: i32, data: Ptr32<Opaque>) -> i32 {
        self.find_in(chap, |e| e.status != ELEM_DELETED && e.data == data)
    }

    /// First slot not deleted whose second object is `aux_data`, or -1
    /// [soft 0x0813462c SI_elemdictFindElemByDelAuxData].
    pub fn find_elem_by_del_aux_data(&self, chap: i32, aux_data: Ptr32<Opaque>) -> i32 {
        self.find_in(chap, |e| e.status != ELEM_DELETED && e.aux_data == aux_data)
    }

    /// First element in use for which `test` answers true, or -1
    /// [soft 0x08134530 SI_elemdictFindElemByCB].
    pub fn find_elem_by_cb(&self, chap: i32, test: impl Fn(&Elem) -> bool) -> i32 {
        self.find_in(chap, |e| e.status == ELEM_VALID && test(e))
    }

    /// Recomputes, for every element of a chapter, the root of its hierarchy and its depth,
    /// the hierarchy being the relation `relid` kept in list `group`
    /// [soft 0x08134cc8 SI_elemdictUpdateHierInfo] [soft 0x08134c34 elemUpdateHierInfo].
    ///
    /// Every slot first becomes its own root at depth 0. Then each element in use that has
    /// the relation on its backward side and not on its forward side is a root, and the
    /// elements named by its backward relations of that kind are its sons, one level
    /// deeper, recursively. The original recurses without a guard and does not check a
    /// son's index: a cycle among the relations never ends there. Here a depth beyond the
    /// number of elements stops the walk, and a son outside the chapter is skipped.
    pub fn update_hier_info(&mut self, chap: i32, group: i32, relid: i32) {
        let Some(list) = self.list_mut(chap) else { return };
        for (i, e) in list.elems.iter_mut().enumerate() {
            e.rec.hier_level = 0;
            e.rec.root_id = i as i32;
        }
        for i in 0..list.elems.len() {
            let e = &list.elems[i];
            if e.rec.status != ELEM_VALID {
                continue;
            }
            let fwd = rel_check(&e.forward, group, relid);
            let back = rel_check(&e.backward, group, relid);
            if back > -1 && fwd < 0 {
                elem_update_hier_info(list, group, relid, i, i as i32, 0);
            }
        }
    }

    /// Frees the chapters and everything in them and re-initialises the dictionary
    /// [soft 0x0813606c SI_elemdictDispose] [soft 0x08136128 relBackUpDispose]. The update
    /// lists end up as `SI_updlistDispose` (0x08141554) leaves them, empty and dirty; their
    /// nodes belong to another package and are not held here.
    pub fn dispose(&mut self, keep_selected: bool) -> Freed {
        let mut freed = Freed::default();
        for l in &mut self.lists {
            l.dispose(keep_selected, &mut freed);
        }
        self.lists.clear();
        self.chap_sort.clear();
        self.fwd_backup.clear();
        self.back_backup.clear();
        self.rec.init();
        freed
    }
}

/// One step of the hierarchy walk [soft 0x08134c34 elemUpdateHierInfo].
fn elem_update_hier_info(list: &mut ElemListStore, group: i32, relid: i32, elem: usize, root: i32, level: i32) {
    list.elems[elem].rec.hier_level = level;
    list.elems[elem].rec.root_id = root;
    if level as usize > list.elems.len() {
        return; // a cycle: the original would recurse until the stack is gone
    }
    let Some(sons) = usize::try_from(group).ok().and_then(|g| list.elems[elem].backward.get(g)) else { return };
    let sons: Vec<i32> = sons.rels.iter().filter(|r| r.relid == relid).map(|r| r.elemid).collect();
    for son in sons {
        if let Ok(s) = usize::try_from(son) {
            if s < list.elems.len() {
                elem_update_hier_info(list, group, relid, s, root, level + 1);
            }
        }
    }
}

fn chap_ids_from_words(w: &[i32; 36]) -> ChapIds {
    ChapIds {
        ani: w[0], cam: w[1], cls: w[2], dsc: w[3], img: w[4], lat: w[5], lig: w[6], mat: w[7], model: w[8],
        nsl: w[9], pal: w[10], rim: w[11], shd_mat: w[12], shd_t2d: w[13], shd_t3d: w[14], shd_vol: w[15],
        shd_sha: w[16], shd_lig: w[17], shd_cam: w[18], shd_out: w[19], shd_env: w[20], shp: w[21], sts: w[22],
        stc: w[23], sfk: w[24], sky: w[25], t2d: w[26], t3d: w[27], wav: w[28], exp: w[29], chn_drv: w[30],
        out: w[31], spd: w[32], audio: w[33], shd_mat_photon: w[34], shd_lig_photon: w[35],
    }
}

/// The program's dictionaries: the array `SI_elemDicts`, its length `SI_nbElemDicts`, and
/// the parallel array of flags `elemDictExists`.
#[derive(Default)]
pub struct Dictionaries {
    pub dicts: Vec<ElemDictStore>,
    pub exists: Vec<i16>,
}

impl Dictionaries {
    /// Appends an initialised dictionary and returns its index [soft 0x08137b00
    /// SI_elemdictAdd]. (The original returns -1 when the array cannot grow.)
    pub fn add(&mut self) -> i32 {
        self.dicts.push(ElemDictStore::new());
        self.exists.push(1);
        self.dicts.len() as i32 - 1
    }

    /// Disposes of dictionary `index` and marks it as gone; its slot stays
    /// [soft 0x08137bdc SI_elemdictRemove]. Refused (false) for the current dictionary
    /// (`current`, the first word of the per-process data `SI_procData`) and for an index
    /// out of range. A dictionary already gone is accepted and nothing happens.
    pub fn remove(&mut self, index: i32, current: i32) -> bool {
        if index == current || index < 0 || self.dicts.len() as i32 <= index {
            return false;
        }
        if self.exists[index as usize] == 1 {
            self.dicts[index as usize].dispose(false);
            self.exists[index as usize] = 0;
        }
        true
    }

    /// Number of dictionaries that exist [soft 0x08137c40 SI_elemdictGetNb].
    pub fn get_nb(&self) -> i32 {
        self.exists.iter().filter(|e| **e == 1).count() as i32
    }

    /// [soft 0x08137c70 SI_elemdictExists]
    pub fn exists(&self, index: i32) -> bool {
        usize::try_from(index).ok().and_then(|i| self.exists.get(i)).is_some_and(|e| *e != 0)
    }
}

/// The record a relation direction has in an element, for a set of lists.
pub fn relations_record(lists: &[RelListStore]) -> Relations {
    Relations { nb_groups: lists.len() as i32, groups: Ptr32::NULL }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: i32 = 71;

    fn scene() -> (ElemDictStore, ChapIds) {
        let mut d = ElemDictStore::new();
        let mut cid = ChapIds::zero();
        d.load_chapters(&mut cid, 1, None);
        (d, cid)
    }

    fn add(d: &mut ElemDictStore, chap: i32, data: u32) -> i32 {
        d.add_elem(chap, Ptr32::new(0x1000 + data), Ptr32::new(data), Ptr32::NULL, 0, 0, -1, false, false, COLOR).unwrap().index
    }

    #[test]
    fn a_new_dictionary_is_as_the_original_initialises_it() {
        let d = ElemDictStore::new();
        let r = &d.rec;
        assert_eq!((r.nb_lists, r.is_scene, r.rel_dirty, r.hier_dirty, r.untold), (0, 1, 1, 1, 0));
        assert_eq!((r.upd_list.dirty, r.topo_list.dirty), (1, 1));
        assert_eq!((r.options.sort_type, r.options.unk_04, r.options.unk_08, r.options.unk_0c, r.options.unk_14), (6, 3, 1, -1, 1));
        assert_eq!(r.options, r.options_db);
    }

    #[test]
    fn chapters_load_in_table_order_with_the_ids_of_the_sdk_header() {
        let (d, cid) = scene();
        assert_eq!(d.rec.nb_lists, 36);
        assert_eq!((cid.dsc, cid.ani, cid.model, cid.sts, cid.shp, cid.audio, cid.shd_lig_photon), (0, 1, 8, 12, 15, 33, 35));
        assert_eq!((cid.shd_mat, cid.shd_sha, cid.shd_vol, cid.shd_t2d), (21, 22, 23, 24));
        assert_eq!(d.get_chap_name(cid.model), Some(&b"MODELS"[..]));
        assert_eq!(d.find_list_by_name(b"TEXTURES3D"), 18);
        assert_eq!(d.find_list_by_name(b"pictures"), 4); // the two picture chapters match in any case
        assert_eq!(d.find_list_by_name(b"models"), -1);
        // kinds and suffixes, with the two quirks of the suffix look-up
        assert_eq!((d.lists[cid.nsl as usize].rec.kind, d.lists[cid.out as usize].rec.kind, d.lists[8].rec.kind), (0, 2, 1));
        assert_eq!(d.lists[cid.model as usize].suffix.as_deref(), Some(&b"hrc"[..]));
        assert_eq!(d.lists[cid.img as usize].suffix.as_deref(), Some(&b"pic"[..]));
        assert_eq!(d.lists[cid.nsl as usize].suffix.as_deref(), Some(&b"???"[..]));
        assert_eq!(d.lists[cid.t3d as usize].suffix.as_deref(), Some(&b"t3D"[..]));
        // a scene gets sort type 9 in the first copy of the options, a database in the second
        assert_eq!((d.rec.options.sort_type, d.rec.options_db.sort_type, d.rec.options.unk_0a), (9, 6, 1));
        let mut db = ElemDictStore::new();
        let mut dbcid = ChapIds::zero();
        db.load_chapters(&mut dbcid, 0, Some(&cid));
        assert_eq!(dbcid, cid);
        assert_eq!((db.rec.is_scene, db.rec.options.sort_type, db.rec.options_db.sort_type, db.rec.options.unk_08), (0, 6, 9, 0));
        assert_eq!(db.rec.options_db.unk_0a, 1);
        // adding an existing chapter returns its index and adds nothing
        let mut d = d;
        assert_eq!(d.add_list(b"MODELS"), 8);
        assert_eq!(d.rec.nb_lists, 36);
    }

    #[test]
    fn chapter_names_sort() {
        let (mut d, cid) = scene();
        d.sort_chap_names();
        assert_eq!(d.chap_sort[0], cid.ani); // ANIMATION
        assert_eq!(d.chap_sort[1], cid.audio);
        assert_eq!(*d.chap_sort.last().unwrap(), cid.wav);
    }

    #[test]
    fn elements_are_added_found_and_their_slots_reused() {
        let (mut d, cid) = scene();
        d.rec.upd_list.dirty = 0;
        d.rec.hier_dirty = 0;
        let a = add(&mut d, cid.model, 1);
        let b = add(&mut d, cid.model, 2);
        assert_eq!((a, b, d.lists[8].rec.nb_elems), (0, 1, 2));
        assert_eq!((d.rec.upd_list.dirty, d.rec.topo_list.dirty, d.rec.hier_dirty), (1, 1, 1));
        let e = d.get_elem(cid.model, b).unwrap();
        assert_eq!((e.rec.wire_color, e.rec.wire_type, e.rec.dirty, e.rec.new_flag, e.rec.status), (COLOR, 2, 1, 2, 0));
        assert_eq!((e.rec.root_id, e.rec.work_id, e.rec.forward.nb_groups, e.forward.len(), e.backward.len()), (1, 0, 6, 6, 6));
        assert_eq!(e.forward[0].rec.flag, 1);
        assert!(d.get_elem(cid.model, 2).is_none() && d.get_elem(cid.model, -1).is_none());
        // delete a slot by hand, as SI_elemdictSetDeleteElem marks it, and add again
        d.lists[8].elems[0].rec.status = ELEM_DELETED;
        d.lists[8].elems[0].rec.seq_first = 5;
        assert!(d.get_elem(cid.model, 0).is_none());
        assert_eq!(d.lists[8].count_valid_elems(), 1);
        assert_eq!(d.find_elem_by_del_data(cid.model, Ptr32::new(1)), -1);
        let c = add(&mut d, cid.model, 3);
        assert_eq!((c, d.lists[8].rec.nb_elems), (0, 2));
        let e = d.get_elem(cid.model, 0).unwrap();
        assert_eq!((e.rec.data, e.rec.status, e.rec.seq_first, e.forward.len()), (Ptr32::new(3), 0, 5, 6));
        assert_eq!(d.find_elem_by_del_data(cid.model, Ptr32::new(2)), 1);
        assert_eq!(d.find_elem_by_aux_data(cid.model, Ptr32::NULL), 0);
        assert_eq!(d.find_elem_by_cb(cid.model, |e| e.data == Ptr32::new(2)), 1);
        assert_eq!(d.find_elem_by_cb(cid.model, |_| false), -1);
    }

    #[test]
    fn the_model_relations_backup_is_asked_for_only_when_the_hierarchy_is_clean() {
        let (mut d, cid) = scene();
        let args = |d: &mut ElemDictStore, chap, data: u32, enabled| {
            d.add_elem(chap, Ptr32::NULL, Ptr32::new(data), Ptr32::NULL, 0, 0, cid.model, enabled, false, COLOR).unwrap()
        };
        assert!(!args(&mut d, cid.model, 1, true).backup_model_relations); // hierarchy dirty after init
        d.rec.hier_dirty = 0;
        assert!(args(&mut d, cid.model, 2, true).backup_model_relations);
        assert_eq!(d.rec.hier_dirty, 0); // left clean
        assert!(!args(&mut d, cid.model, 0, true).backup_model_relations); // no object
        d.rec.hier_dirty = 0;
        assert!(!args(&mut d, cid.cam, 3, true).backup_model_relations); // another chapter
        d.rec.hier_dirty = 0;
        assert!(!args(&mut d, cid.model, 4, false).backup_model_relations); // global flag off
        assert_eq!(d.rec.hier_dirty, 1);
    }

    #[test]
    fn work_flags_and_bulk_setters() {
        let (mut d, cid) = scene();
        for i in 0..3 {
            add(&mut d, cid.lig, i + 1);
        }
        for e in &mut d.lists[cid.lig as usize].elems {
            e.rec.set_flag(-1);
        }
        d.low_byte_work_flag_and(cid.lig, 0x0f);
        assert_eq!(d.lists[cid.lig as usize].elems[0].rec.work_flag as u32, 0x0fff_ffff);
        d.clear_low_byte_work_flag(cid.lig);
        assert_eq!(d.lists[cid.lig as usize].elems[1].rec.work_flag, 0x00ff_ffff);
        d.clear_work_flag(cid.lig);
        assert_eq!(d.lists[cid.lig as usize].elems[2].rec.work_flag, 0);
        d.set_new_flag(2, 1);
        d.set_selection(3);
        let e = &d.lists[cid.lig as usize].elems[0].rec;
        assert_eq!((e.new_flag, e.selected), (1, 3));
        d.set_new_flag(-1, 0);
        assert_eq!(d.lists[cid.lig as usize].elems[2].rec.new_flag, 0);
        let mut e = Elem::zero();
        e.set_dirty(0x10005);
        e.schem_set_pos_x(1.5);
        e.schem_set_pos_y(-2.0);
        assert_eq!((e.dirty, e.schem_x, e.schem_y), (5, 1.5, -2.0));
    }

    fn relation(relid: i32, elemid: i32, rank: i32) -> NewRelation {
        NewRelation { relid, chapid: 8, elemid, group: 0, rank, fanout: 1, limit: 0, serial: -1, check_fanout: true, data: Ptr32::NULL, flag: 1 }
    }

    fn rd() -> RelDictStore {
        let mut rd = RelDictStore::new();
        rd.add_relation(b"parent", 100, 8, 8, 0, 0, 5, 1, 0, 0, 0, None, 0, 0);
        rd.add_relation(b"multi", 200, 8, 8, 0, 0, 3, 1, 0, 0, 0, None, 0, 0);
        rd.add_relation(b"multi", 200, 8, 7, 0, 1, 3, 1, 0, 0, 0, None, 0, 0);
        rd
    }

    #[test]
    fn relations_are_kept_in_rank_order_and_duplicates_refused() {
        let mut rd = rd();
        let mut list = rel_allocate().remove(0);
        assert!(rel_add_relation(&mut rd, &mut list, &relation(100, 1, 5)));
        assert_eq!((list.rec.nb, list.rec.capacity), (1, 1));
        // a lower rank goes in front, a higher one behind
        assert!(rel_add_relation(&mut rd, &mut list, &relation(200, 2, 3)));
        assert!(rel_add_relation(&mut rd, &mut list, &relation(300, 3, 9)));
        assert_eq!(list.rels.iter().map(|r| r.relid).collect::<Vec<_>>(), [200, 100, 300]);
        assert_eq!((list.rec.nb, list.rec.capacity), (3, 3));
        let r = list.rels[1];
        assert_eq!((r.elemid, r.chapid, r.group, r.active_elem, r.serial, r.rank, r.flag), (1, 8, 0, -1, -1, 5, 1));
        // the same single-rule relation to the same element is refused; the capacity has grown
        assert!(!rel_add_relation(&mut rd, &mut list, &relation(100, 1, 5)));
        assert_eq!((list.rec.nb, list.rec.capacity), (3, 4));
        // a relation with two rules may repeat, but not past the fan-out when it is checked
        assert!(!rel_add_relation(&mut rd, &mut list, &relation(200, 2, 3)));
        let mut again = relation(200, 2, 3);
        again.check_fanout = false;
        assert!(rel_add_relation(&mut rd, &mut list, &again));
        assert_eq!(list.rels.iter().map(|r| r.relid).collect::<Vec<_>>(), [200, 200, 100, 300]);
        assert_eq!(rel_check(std::slice::from_ref(&list), 0, 100), 2);
        assert_eq!(rel_check(std::slice::from_ref(&list), 0, 400), -1);
        assert_eq!(rel_check(std::slice::from_ref(&list), 3, 100), -1);
    }

    #[test]
    fn hierarchy_information_follows_the_backward_relations() {
        let (mut d, cid) = scene();
        let mut rdict = rd();
        for i in 0..5 {
            add(&mut d, cid.model, i + 1);
        }
        // 1 and 2 are sons of 0; 3 is a son of 2; 4 stands alone. A son is the source of the
        // relation and its father the destination.
        for (son, father) in [(1, 0), (2, 0), (3, 2)] {
            let l = &mut d.lists[8];
            // as SI_elemdictAddCompleteRelation does it: the fan-out is checked on the source
            // side only, so a father may gather any number of sons
            assert!(rel_add_relation(&mut rdict, &mut l.elems[son].forward[0], &relation(100, father, 5)));
            let mut back = relation(100, son as i32, 5);
            back.check_fanout = false;
            assert!(rel_add_relation(&mut rdict, &mut l.elems[father as usize].backward[0], &back));
        }
        d.update_hier_info(cid.model, 0, 100);
        let got: Vec<(i32, i32)> = d.lists[8].elems.iter().map(|e| (e.rec.root_id, e.rec.hier_level)).collect();
        assert_eq!(got, [(0, 0), (0, 1), (0, 1), (0, 2), (4, 0)]);
        let e = &d.lists[8].elems[0];
        assert!(e.is_elem_used(false) && !d.lists[8].elems[4].is_elem_used(false));
        assert_eq!(e.get_cur_relation_index(false, 0), 0);
        let e = &mut d.lists[8].elems[0];
        e.set_cur_relation_index(false, 0, 7);
        assert_eq!((e.get_cur_relation_index(false, 0), e.get_cur_relation_index(true, 0), e.get_cur_relation_index(true, 9)), (7, 0, -1));
    }

    #[test]
    fn an_element_is_used_when_something_points_at_it() {
        let mut rdict = rd();
        let mut e = ElemStore { rec: Elem::zero(), forward: rel_allocate(), backward: rel_allocate() };
        assert!(!e.is_elem_used(true));
        assert!(rel_add_relation(&mut rdict, &mut e.backward[1], &relation(0x6e, 1, 0)));
        assert!(e.is_elem_used(false));
        assert!(!e.is_elem_used(true)); // relations 110 and 207 in the second list do not count
        assert!(rel_add_relation(&mut rdict, &mut e.backward[1], &relation(0x70, 2, 0)));
        assert!(e.is_elem_used(true));
        let none = ElemStore { rec: Elem::zero(), forward: Vec::new(), backward: Vec::new() };
        assert!(!none.is_elem_used(false));
    }

    #[test]
    fn disposal_reports_what_the_caller_must_free() {
        let (mut d, cid) = scene();
        add(&mut d, cid.model, 1);
        add(&mut d, cid.model, 2);
        d.lists[8].elems[1].rec.selected = 1;
        d.lists[8].elems[0].rec.icon = Ptr32::new(0x77);
        d.lists[8].elems[1].rec.suffix = Ptr32::new(0x88);
        let freed = d.dispose(true);
        assert_eq!(freed.suffixes, [Ptr32::new(0x88)]);
        assert_eq!(freed.nodes, [Ptr32::new(0x1001)]);
        assert_eq!(freed.icons, [Ptr32::new(0x77)]);
        assert_eq!((d.rec.nb_lists, d.lists.len(), d.rec.hier_dirty), (0, 0, 1));
    }

    #[test]
    fn the_global_array_of_dictionaries() {
        let mut all = Dictionaries::default();
        assert_eq!((all.add(), all.add(), all.add()), (0, 1, 2));
        assert_eq!(all.get_nb(), 3);
        assert!(!all.remove(0, 0)); // the current one
        assert!(!all.remove(3, 0) && !all.remove(-1, 0));
        assert!(all.remove(1, 0));
        assert!(all.remove(1, 0)); // already gone: accepted
        assert_eq!((all.get_nb(), all.exists(1), all.exists(2), all.exists(5)), (2, false, true, false));
        assert_eq!(all.add(), 3); // slots are not reused
    }

    #[test]
    fn identities_compare_by_chapter_and_element() {
        let a = ElemIdentity { chapid: 8, elemid: 2, sub_id: 0, sub_id2: 0 };
        let mut b = a;
        b.sub_id = 9;
        assert!(compare_elem_ident(Some(&a), Some(&b)));
        b.elemid = 3;
        assert!(!compare_elem_ident(Some(&a), Some(&b)));
        assert!(!compare_elem_ident(None, Some(&b)));
    }

    #[test]
    fn the_load_optimisation_keeps_a_table_slot_per_element() {
        let mut l = ElemListStore::new(b"X");
        l.add_elem(Ptr32::NULL, Ptr32::new(1), Ptr32::NULL, 0, 0, true, true, true, COLOR);
        l.add_elem(Ptr32::NULL, Ptr32::new(2), Ptr32::NULL, 0, 0, false, true, true, COLOR);
        let t = l.sort_tbl.as_ref().unwrap();
        assert_eq!((t.table.len(), t.valid, t.count), (2, 0, 0));
        assert_eq!((l.elems[1].forward.len(), l.elems[1].rec.forward.nb_groups), (0, 0));
        l.sort_tbl.as_mut().unwrap().valid = 1;
        l.sort_tbl_set_dirty();
        assert_eq!(l.sort_tbl.as_ref().unwrap().valid, 0);
    }
}
