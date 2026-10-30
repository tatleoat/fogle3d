// SPDX-License-Identifier: MIT
//! What stands beside the memory image while a scene is loaded: the answers the
//! dictionaries ask of the rest of the program, the addresses of things that are not in
//! the image yet, and the seams to the code of this crate that still works on stores.
//!
//! The dictionaries (`SI_ElemDict` with its chapters, elements, relation lists and name
//! records, the relation dictionary `SI_RD`, the chapter indices `SI_cid`) live in
//! `si-mem`'s image at the original's layout, and every routine of `elemdict.c`,
//! `relations.c`, `reldict.c`, `toposort.c` and `updlist.c` is `si-dict`'s. What an
//! element points at (a model node, a camera, a material …), the nodes of the scene graph
//! and the data of a relation are still Rust values of the packages that read them: each
//! has an address in `si-mem`'s handle range, which is never mapped, so that following one
//! through the image is a recorded fault.
//!
//! Six functions of the original are reproduced here, each cited where it stands:
//! `SI_edGetFcvFromRelation`, `SI_elemdictDisposeRelationData` and the four routines the
//! latter hands a relation's data to. The rest of the file reproduces none in full.

use crate::calls::{Arg, Channel, Message, Unported};
use crate::dbnode;
use crate::dict::{Arena, Dict, ElemId, Program};
use crate::load::Scene;
use crate::updlist::{UpdArg as StoreArg, UpdListStore, UpdNode as StoreNode};
use si_ascii::reldata::RelData;
use si_dict::globals::{SI_CID, SI_ELEM_DICTS, SI_GLB_MANIP, SI_PROC_DATA, SI_RD, SI_USE_ELEM_LIST_SORT_TBL, GLB_MANIP_36, REL_SERIAL_NUMBER};
use si_dict::outside::{self as out, Outside, Routine};
use si_dict::records::{CustomRelData, UpdArg, UpdNode};
use si_dict::{elemdict, elemrel};
use si_mem::handle::Kind;
use si_mem::{field, CStr32, Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, ChapIds, DbListNode, Elem, ElemDict, ElemList, RelList, RelRule, Relations, SortTbl, ELEM_DELETED};
use si_types::reldict::{RelDictStore, SortTable};
use si_types::store::{ElemDictStore, ElemListStore, ElemStore, RelListStore};

/// The kind of address an object read from an element file has (what `__SI_Elem` +0x04
/// holds): the `n`-th object of [`Scene::objects`] is the kind's `n`-th handle.
pub const OBJECTS: Kind = Kind::Other(4);
/// The same for a node of the scene graph ([`Scene::graf`], `__SI_Elem` +0x08).
pub const GRAF_NODES: Kind = Kind::Other(5);
/// The same for the data of a relation ([`Scene::rel_data`], `SI_ARelation` +0x18) that is
/// not a record of the image; the loader makes every relation's data a record
/// ([`rel_data_record`]), so only callers that build a dictionary in stores use these.
pub const REL_DATA: Kind = Kind::Other(6);

fn addr_of(kind: Kind, handle: u32) -> u32 {
    if handle == 0 {
        0
    } else {
        kind.nth(handle - 1)
    }
}

fn handle_of(kind: Kind, addr: u32) -> u32 {
    kind.number(addr).map_or(0, |n| n + 1)
}

/// The address an element holds for an object of [`Scene::objects`] (null for handle 0).
pub fn obj_ptr(handle: u32) -> Ptr32<Opaque> {
    Ptr32::new(addr_of(OBJECTS, handle))
}

/// The object an element's address stands for: its handle in [`Scene::objects`], 0 for
/// null and for an address that is no object.
pub fn obj_of(p: Ptr32<Opaque>) -> u32 {
    handle_of(OBJECTS, p.addr())
}

/// The address an element holds for a node of the scene graph.
pub fn graf_ptr(handle: u32) -> Ptr32<Opaque> {
    Ptr32::new(addr_of(GRAF_NODES, handle))
}

pub fn graf_of(p: Ptr32<Opaque>) -> u32 {
    handle_of(GRAF_NODES, p.addr())
}

/// The address a relation entry holds for data that is not a record of the image.
pub fn rel_data_ptr(handle: u32) -> Ptr32<Opaque> {
    Ptr32::new(addr_of(REL_DATA, handle))
}

/// The data a relation entry points at, as a handle of [`Scene::rel_data`]: by the table of
/// the records of the image that stand for a value ([`rel_data_record`]), or by its
/// address in the handle range. 0 for null and for anything else.
pub fn rel_data_of(sc: &Scene, p: Ptr32<Opaque>) -> u32 {
    match handle_of(REL_DATA, p.addr()) {
        0 => sc.rel_records.get(&p.addr()).copied().unwrap_or(0),
        h => h,
    }
}

/// The data of a relation line as the record `SI_elemdictReadRelationData (0x081144e8)`
/// leaves in the heap, made from the value `si-ascii`'s reader read (unchecked as a whole;
/// the allocators and the layouts were read from the decompile and the listing):
///
/// | Relation | Record |
/// |---|---|
/// | 102, 103, 116, 150, 160, 239, 250 to 253, 255 to 257 (`reldata::Record`) | `SI_calloc(1, size)` (each allocator, e.g. `SI_extrusionDataAllocate`, is one `SI_calloc`), the record's bytes; for 257 the `PROJ_CURVE` array `SI_calloc(n, 2)` at +0x40 |
/// | 220, 221, 230, 231, 800, 807 | `SI_calloc(n, 4)` (`elemdictRelDataAllocate`: 7, 9, 16, 1 floats), the floats read |
/// | 245 | `SI_animBoolAllocate`: `SI_calloc(1, 0x14)`; +0 active (16 bits), +4 operation, +8 material, +0xc, +0xe, +0x10 three 16-bit flags |
/// | 247 | `SI_calloc(1, 0x40)`: the matrix, rows first |
/// | 10000 | `SI_calloc(1, 0x10)`: chapter, element, parameter, rank |
/// | 20000 | `SI_calloc(1, 0x28)`: +0 the icon's element, +4 `strdup` of the icon's id, +8 … +0x1c the six numbers, +0x20 and +0x24 the names (`SI_malloc(strlen + 1)`) |
/// | 20010 | `SI_calloc(1, 0x14)`: +0 the parameter, +0x10 the name for parameter 0x236 |
///
/// The value stays in [`Scene::rel_data`], found again by the record's address
/// ([`rel_data_of`]), for the saver and the stores. Where the original's generic kinds
/// write more floats than their block holds (a file listing more values than the kind
/// has), the original overruns its allocation; here only the block's floats are written.
/// The custom effect's blocks are made in the order record, id, `ENAME`, `MNAME`; the
/// original makes the two names in the order of the keywords in the file.
// adapter: not in place (the reader runs on values; the record is laid from the value
// when the line is applied, where the original's reader has just made it)
pub fn rel_data_record(m: &mut Mem, sc: &mut Scene, value: RelData) -> Ptr32<Opaque> {
    let put_f32s = |m: &mut Mem, p: u32, v: &[f32]| {
        for (k, x) in v.iter().enumerate() {
            m.set_f32(p + 4 * k as u32, *x);
        }
    };
    let rec = match &value {
        RelData::Record(r) => {
            let p = m.si_calloc(1, r.image.len() as u32);
            if p != 0 {
                m.set_bytes(p, &r.image);
                if let Some(v) = &r.proj_curves {
                    let a = if v.is_empty() { 0 } else { m.si_calloc(v.len() as u32, 2) };
                    if a != 0 {
                        for (k, x) in v.iter().enumerate() {
                            m.set_i16(a + 2 * k as u32, *x);
                        }
                    }
                    m.set_u32(p + 0x40, a);
                }
            }
            p
        }
        RelData::Floats { capacity, values, .. } => {
            let p = m.si_calloc(*capacity as u32, 4);
            if p != 0 {
                put_f32s(m, p, &values[..values.len().min(*capacity)]);
            }
            p
        }
        RelData::Matrix(rows) => {
            let p = m.si_calloc(1, 0x40);
            if p != 0 {
                put_f32s(m, p, &rows.concat());
            }
            p
        }
        RelData::AnimBool(b) => {
            let p = m.si_calloc(1, 0x14);
            if p != 0 {
                m.set_i16(p, b.active);
                m.set_i32(p + 4, b.operation);
                m.set_i32(p + 8, b.material);
                m.set_i16(p + 0xc, b.remove_edges);
                m.set_i16(p + 0xe, b.remove_vertices);
                m.set_i16(p + 0x10, b.material_animated);
            }
            p
        }
        RelData::NamedSelection(n) => {
            let p = m.si_calloc(1, 0x10);
            if p != 0 {
                for (k, v) in [n.chapter, n.elem, n.fcurve, n.rank].into_iter().enumerate() {
                    m.set_i32(p + 4 * k as u32, v);
                }
            }
            p
        }
        RelData::Expression(e) => {
            let p = m.si_calloc(1, 0x14);
            if p != 0 {
                m.set_i32(p, e.fcurve);
                if let Some(name) = &e.name {
                    let s = m.malloc(name.len() as u32 + 1);
                    m.put_cstr(s, name);
                    m.set_u32(p + 0x10, s);
                }
            }
            p
        }
        RelData::Custom(c) => {
            let rec: Ptr32<CustomRelData> = Ptr32::new(m.si_calloc(1, 0x28));
            let p = rec.addr();
            if p != 0 {
                m.set_i32(p, c.icon_elem);
                if let Some(id) = &c.icon_id {
                    // strdup: the C library's malloc
                    let s = m.malloc(id.len() as u32 + 1);
                    m.put_cstr(s, id);
                    m.set_u32(p + 4, s);
                }
                for (k, v) in [c.value_08, c.value_0c, c.group_10, c.group_14, c.rocc, c.aocc].into_iter().enumerate() {
                    m.set_i32(p + 8 + 4 * k as u32, v);
                }
                let si_string = |m: &mut Mem, s: &Option<Vec<u8>>| match s {
                    Some(s) => {
                        let a = m.si_malloc(s.len() as u32 + 1);
                        m.put_cstr(a, s);
                        Ptr32::new(a)
                    }
                    None => Ptr32::NULL,
                };
                m[rec].name = si_string(m, &c.ename);
                m[rec].effect = si_string(m, &c.mname);
            }
            p
        }
    };
    let handle = sc.rel_data.alloc(value);
    if rec != 0 {
        sc.rel_records.insert(rec, handle);
    }
    Ptr32::new(rec)
}

/// `SI_cid`, copied out.
pub fn cid(m: &Mem) -> ChapIds {
    m.read(SI_CID)
}

/// The scene dictionary: the record of the array `SI_elemDicts` that the first word of
/// the per-process data names (`SI_elemDicts[*SI_procData]`, as `SI_edGetFcvFromRelation`
/// and `SI_elemdictCusGetRelData` reach it).
pub fn current_dict(m: &Mem) -> Ptr32<ElemDict> {
    let index = m[m[SI_PROC_DATA]];
    m[SI_ELEM_DICTS].add(index)
}

/// `&ed->lists[chap]`, as the original computes it: no test of the index.
pub fn list(m: &Mem, ed: Ptr32<ElemDict>, chap: i32) -> Ptr32<ElemList> {
    m[ed].lists.add(chap)
}

/// `&ed->lists[chap].elems[index]`, as the original computes it: no test of either index.
pub fn elem(m: &Mem, ed: Ptr32<ElemDict>, id: ElemId) -> Ptr32<Elem> {
    m[list(m, ed, id.chap)].elems.add(id.index)
}

/// The same with both indices tested: `None` for a chapter or a slot the dictionary does
/// not have.
pub fn slot(m: &Mem, ed: Ptr32<ElemDict>, id: ElemId) -> Option<Ptr32<Elem>> {
    if id.chap < 0 || id.chap >= m[ed].nb_lists {
        return None;
    }
    let l = list(m, ed, id.chap);
    (id.index >= 0 && id.index < m[l].nb_elems).then(|| m[l].elems.add(id.index))
}

/// Number of slots of a chapter (0 for a chapter the dictionary does not have).
pub fn nb_elems(m: &Mem, ed: Ptr32<ElemDict>, chap: i32) -> i32 {
    if chap < 0 || chap >= m[ed].nb_lists {
        return 0;
    }
    m[list(m, ed, chap)].nb_elems
}

/// A chapter's name.
pub fn chap_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32) -> Vec<u8> {
    m.cstr(m[list(m, ed, chap)].name.addr())
}

/// A string field of a name record as a value; `None` for no record or a null string.
pub fn node_text(m: &Mem, node: Ptr32<DbListNode>, pick: fn(&DbListNode) -> CStr32) -> Option<Vec<u8>> {
    if node.is_null() {
        return None;
    }
    dbnode::text(m, pick(&m[node]))
}

/// The full name of an element (`prefix-name.version-subst`), for the checks' reports.
pub fn full_name(m: &Mem, ed: Ptr32<ElemDict>, id: ElemId) -> Option<Vec<u8>> {
    let e = slot(m, ed, id)?;
    node_text(m, m[e].node, |n| n.fullname)
}

/// Runs `f` with a text as a C string on the image's stack, where the original passes a
/// string it holds in a local buffer or a token of the file it is reading.
pub fn with_text<R>(m: &mut Mem, text: &[u8], f: impl FnOnce(&mut Mem, CStr32) -> R) -> R {
    let size = (text.len() as u32 + 1 + 3) & !3;
    let p = m.local(size);
    m.put_cstr(p, text);
    let r = f(m, Ptr32::new(p));
    m.release(size);
    r
}

/// `SI_elemdictFindListByName` on a text that is not in the image.
pub fn find_list_by_name(m: &mut Mem, ed: Ptr32<ElemDict>, name: &[u8]) -> i32 {
    with_text(m, name, |m, p| elemdict::find_list_by_name(m, ed, p))
}

/// `SI_elemdictFindRelatedElem` with its two result words on the stack, as the original's
/// callers pass the addresses of two locals: the element at the other end and its index.
#[allow(clippy::too_many_arguments)]
pub fn find_related(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, first: bool) -> Option<(Ptr32<Elem>, ElemId)> {
    let oc: Ptr32<i32> = Ptr32::new(m.local(4));
    let oi: Ptr32<i32> = Ptr32::new(m.local(4));
    let found = if first {
        elemrel::find_related_elem(m, ed, e, forward, group, relid, chap, oc, oi)
    } else {
        elemrel::find_next_related_elem(m, ed, e, forward, group, relid, chap, oc, oi)
    };
    let out = (!found.is_null()).then(|| (found, ElemId::new(m[oc], m[oi])));
    m.release(8);
    out
}

/// What `SI_attdictGetInfoFromFcurve` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FcvInfo {
    pub relid: i32,
    pub group: i32,
    pub rule: i32,
    pub limit: i32,
}

/// `SI_attdictGetInfoFromFcurve(&SI_ATD, id, …)` with its four result words on the stack,
/// as the original's callers pass the addresses of four locals: what ties a curve to its
/// element.
pub fn fcv_info(m: &mut Mem, id: i32) -> FcvInfo {
    let w: [Ptr32<i32>; 4] = std::array::from_fn(|_| Ptr32::new(m.local(4)));
    si_dict::attdict::get_info_from_fcurve(m, si_dict::globals::SI_ATD, id, w[0], w[1], w[2], w[3]);
    let info = FcvInfo { relid: m[w[0]], group: m[w[1]], rule: m[w[2]], limit: m[w[3]] };
    m.release(16);
    info
}

/// `SI_fcvGetRelInfo(id, …)` likewise: relation id, rule number and count limit.
pub fn fcv_rel_info(m: &mut Mem, id: i32) -> (i32, i32, i32) {
    let w: [Ptr32<i32>; 3] = std::array::from_fn(|_| Ptr32::new(m.local(4)));
    crate::dbvsfcv::fcv_get_rel_info(m, id, w[0], w[1], w[2]);
    let out = (m[w[0]], m[w[1]], m[w[2]]);
    m.release(12);
    out
}

/// The dictionaries' outside world during a load: the scene's side tables and whoever
/// records the calls into packages that are not ported.
pub struct Out<'a> {
    pub sc: &'a mut Scene,
    pub un: &'a mut dyn Unported,
}

impl Outside for Out<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        let a = |k: usize| args.get(k).copied().unwrap_or(0);
        match routine.addr {
            x if x == out::SI_GET_LOAD_OPTIM.addr => self.sc.answers.load_optim as u32,
            x if x == out::SI_GRAFNODE_DEF_COLOR.addr => self.sc.answers.def_color as u32,
            x if x == out::TIME.addr => self.sc.answers.now as u32,
            x if x == out::SI_ELEMDICT_DISPOSE_RELATION_DATA.addr => {
                dispose_relation_data(m, self.sc, Ptr32::new(a(0)));
                0
            }
            x if x == out::SI_DB_LIST_NODE_FREE.addr => {
                dbnode::list_node_free(m, Ptr32::new(a(0)));
                0
            }
            x if x == out::SI_DB_LIST_NODE_ALLOCATE.addr => dbnode::list_node_allocate(m, self).addr(),
            x if x == out::SI_DB_FORMAT_FULL_NAME.addr => dbnode::format_full_name(m, Ptr32::new(a(0))).addr(),
            x if x == out::SI_DBNAME_CHECK_VALIDITY.addr => dbnode::name_check_validity(m, Ptr32::new(a(0)), a(1) as i16).addr(),
            x if x == out::SI_ED_GET_CHAPTER_FROM_SUFFIX.addr => dbnode::ed_get_chapter_from_suffix(m, Ptr32::new(a(0)), Ptr32::new(a(1))) as u32,
            x if x == out::SI_ED_GET_SUFFIX_FROM_CHAPTER.addr => dbnode::ed_get_suffix_from_chapter(m, a(0) as i32).addr(),
            x if x == out::DBFILENAME_EXPAND.addr => {
                dbnode::filename_expand(m, Ptr32::new(a(0)), [a(1), a(2), a(3), a(4), a(5)]);
                0
            }
            x if x == out::SI_ED_GET_FCV_FROM_RELATION.addr => ed_get_fcv_from_relation(m, Ptr32::new(a(0))).addr(),
            // the back-up of the model elements' relations (`untold.rs`), which
            // SI_elemdictAddElem makes when the dictionary's hierarchy is not dirty
            x if x == out::SI_ED_ALLOC_MDL_REL_BACK_UP.addr => {
                crate::untold::ed_alloc_mdl_rel_back_up(m, a(0));
                0
            }
            x if x == out::SI_ED_MDL_REL_BACK_UP.addr => {
                crate::untold::ed_mdl_rel_back_up(m, a(0), a(1), a(2) as i32);
                0
            }
            x if x == crate::dupnames::SI_ED_SOFT_ELEM_CHANGE_NAME.addr => {
                soft_elem_change_name(m, self.sc, Ptr32::new(a(0)), a(1) as i32, Ptr32::new(a(2)), self.un);
                0
            }
            x if x == crate::dupnames::SI_ED_ANIM_ELEM_CHANGE_NAME.addr => {
                // si_edAnimElemChangeName (0x080eeae4), `edanim.c`: not ported; recorded
                // with the element, its old name and its new one
                let (e, chap): (Ptr32<Elem>, i32) = (Ptr32::new(a(0)), a(1) as i32);
                let index = e.offset_from(m[list(m, current_dict(m), chap)].elems) as i32;
                let old = dbnode::text(m, Ptr32::new(a(2))).unwrap_or_default();
                let new = dbnode::text(m, Ptr32::new(a(4))).unwrap_or_default();
                self.un.call("si_edAnimElemChangeName", &[Arg::Elem(ElemId::new(chap, index)), Arg::Str(old), Arg::Str(new)]);
                0
            }
            x if x == out::SI_CUS_RESORT_UPD_LIST.addr => {
                self.un.call("SI_cusResortUpdList", &[]);
                0
            }
            x if x == out::SI_DB_CHECK_EXTRA_UNTOLD_RELATION.addr => {
                // `edtools.c`'s port on the image (finding 0018, section 19), with the
                // loader's dispatch for what it calls
                let mut p = crate::image::Prog { sc: &mut *self.sc, un: &mut *self.un, fs: None, db: None };
                crate::untold::db_check_extra_untold_relation(&mut p, m, a(0), a(1) as i16, a(2) as i16, a(3) as i16);
                0
            }
            // The three calls of `SI_elemdictRestoreActions` (`si_dict::dictin`, finding 0010):
            // `si-anim` has the routines on the image, but they need a world of their own
            // there (the loader has none yet), so they are recorded with their arguments.
            x if x == out::SI_ACTIONS_TMP_ACTIVATE_ALL.addr => {
                self.un.call("SI_actionsTmpActivateAll", &[Arg::Int(a(1) as i64), Arg::Int(a(2) as i16 as i64)]);
                0
            }
            x if x == out::SI_ACTIONS_ADD_MASTER_KEYS_TO_HA.addr => {
                let chap = m[SI_CID].ani;
                let index = Ptr32::<Elem>::new(a(1)).offset_from(m[list(m, current_dict(m), chap)].elems) as i32;
                self.un.call("SI_actionsAddMasterKeysToHA", &[Arg::Elem(ElemId::new(chap, index)), Arg::Int(a(2) as i16 as i64)]);
                0
            }
            x if x == out::SI_ACTIONS_INIT_PATH_ANIM.addr => {
                self.un.call("SI_actionsInitPathAnim", &[Arg::Int(a(1) as i64)]);
                0
            }
            // SI_edSetUneditable, SI_edCheckForUneditable (the light product): not ported,
            // nothing done
            _ => 0,
        }
    }

    /// The one variable the dictionaries ask for is `SI_XSI_LEGACYLOADER`, and they only
    /// test the answer for null: when it is set the answer is the address of the literal
    /// that names it.
    fn getenv(&mut self, _m: &mut Mem, name: &[u8]) -> u32 {
        if name == b"SI_XSI_LEGACYLOADER" && self.sc.legacy_loader {
            si_dict::rodata::S_LEGACY.addr()
        } else {
            0
        }
    }

    fn stdout(&mut self, text: &[u8]) {
        self.sc.messages.push(Message { channel: Channel::Stdout, text: text.to_vec() });
    }

    fn stderr(&mut self, text: &[u8]) {
        self.sc.messages.push(Message { channel: Channel::Stderr, text: text.to_vec() });
    }
}

/// [soft 0x080ee6f0 SI_edGetFcvFromRelation]: the object of the animation element a
/// relation entry owns (its word at +8 is that element's index in the ANIMATION chapter of
/// the current dictionary, `SI_elemDicts[*SI_procData]`), null for an entry that owns none
/// (a negative index). No test of the index against the chapter.
pub fn ed_get_fcv_from_relation(m: &Mem, rel: Ptr32<ARelation>) -> Ptr32<Opaque> {
    let active = m[rel].active_elem;
    if active < 0 {
        return Ptr32::NULL;
    }
    let ed = current_dict(m);
    let ani = m[SI_CID].ani;
    m[elem(m, ed, ElemId::new(ani, active))].data
}

/// Offsets in the data of an expression's relation (`SI_ExprRelData`) as far as
/// `SI_exprRelDataDispose` reads it. The record is not laid out: the parameter id, a
/// block that belongs to the custom-effect curve id 0x236, the number of slots of the
/// array (16 bits) and the array of blocks.
const EXPR_ID: u32 = 0x00;
const EXPR_CUSTOM: u32 = 0x10;
const EXPR_NB: u32 = 0x16;
const EXPR_ARRAY: u32 = 0x1c;
/// Offset in the data of a trimming relation (`__SI_TrimmingData`, not laid out) of the
/// one block it owns.
const TRIM_OWNED: u32 = 0x40;
/// Offset in a custom effect's relation data of the first of the three blocks
/// `SI_cusRelDataDestroy` frees (the two others are [`CustomRelData`]'s names).
const CUS_OWNED: u32 = 0x04;

/// `free(*slot); *slot = NULL` when the slot holds a pointer.
fn free_slot(m: &mut Mem, slot: u32) {
    let p = m.u32(slot);
    if p != 0 {
        m.free(p);
        m.set_u32(slot, 0);
    }
}

/// [soft 0x0814e140 SI_extractDataDispose]: frees the block a pointer points at and clears
/// the pointer. `pp` is the pointer's address; nothing for a null address or a null pointer.
pub fn extract_data_dispose(m: &mut Mem, pp: u32) {
    if pp != 0 {
        free_slot(m, pp);
    }
}

/// [soft 0x084fcabc SI_trimmingDataDispose]: frees the block a trimming's data owns
/// (+0x40), then the data, and clears the pointer at `pp`.
pub fn trimming_data_dispose(m: &mut Mem, pp: u32) {
    if pp != 0 && m.u32(pp) != 0 {
        let data = m.u32(pp);
        free_slot(m, data + TRIM_OWNED);
        free_slot(m, pp);
    }
}

/// [soft 0x0852e33c SI_cusRelDataDestroy]: frees the three blocks a custom effect's
/// relation data owns (+0x04, then the two names at +0x20 and +0x24), each cleared after
/// its `free`, then the record, and clears the pointer at `pp`. The record is not tested
/// for null: a null pointer behind `pp` makes the original read address 4.
pub fn cus_rel_data_destroy(m: &mut Mem, pp: u32) {
    if pp == 0 {
        return;
    }
    let data = m.u32(pp);
    free_slot(m, data + CUS_OWNED);
    free_slot(m, field!(Ptr32::<CustomRelData>::new(data), CustomRelData, name).addr());
    free_slot(m, field!(Ptr32::<CustomRelData>::new(data), CustomRelData, effect).addr());
    free_slot(m, pp);
    m.set_u32(pp, 0);
}

/// [soft 0x08249c74 SI_exprRelDataDispose]: frees the data of an expression's relation:
/// every block of its array (the count is the 16-bit word at +0x16), the array, for the
/// custom-effect curve id (0x236) the block at +0x10, and the record. An array pointer
/// that is null leaves the slots alone; the array is read again after every slot.
pub fn expr_rel_data_dispose(m: &mut Mem, data: u32) {
    if data == 0 {
        return;
    }
    let mut array = m.u32(data + EXPR_ARRAY);
    if array != 0 {
        let mut i = 0;
        while i < m.i16(data + EXPR_NB) as i32 {
            free_slot(m, m.u32(data + EXPR_ARRAY).wrapping_add(i as u32 * 4));
            array = m.u32(data + EXPR_ARRAY);
            i += 1;
        }
        if array != 0 {
            m.free(array);
            m.set_u32(data + EXPR_ARRAY, 0);
        }
    }
    if m.i32(data + EXPR_ID) == 0x236 {
        free_slot(m, data + EXPR_CUSTOM);
    }
    m.free(data);
}

/// What `SI_elemdictDisposeRelationData` does with the data of an entry, by the
/// relation's id: the routine's own switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposal {
    /// `free`.
    Free,
    Nothing,
    /// `SI_extractDataDispose`, `SI_trimmingDataDispose`, `SI_cusRelDataDestroy`,
    /// `SI_exprRelDataDispose`.
    Extract,
    Trimming,
    Custom,
    Expr,
    /// No case: the message on the standard error.
    Message,
}

fn disposal(m: &Mem, rel: Ptr32<ARelation>) -> Disposal {
    match m[rel].relid {
        0x66 | 0x67 | 0x74 | 0x96 | 0xa0 | 0xef | 0xf5 | 0xf7 | 0xfa | 0xfb | 0xfc | 0xfd | 0xff => Disposal::Free,
        // (the original tests the pointer once more before these)
        0xdc | 0xdd | 0xe6 | 0xe7 | 800 | 0x327 => Disposal::Free,
        0x7c => Disposal::Nothing,
        0x100 => Disposal::Extract,
        0x101 => Disposal::Trimming,
        20000 => Disposal::Custom,
        0x4e2a => {
            if m[crate::globals::SI_ED_SUMATRA_TO_3X] != 0 {
                Disposal::Nothing
            } else if m[rel].chapid as i32 == m[SI_CID].exp {
                Disposal::Expr
            } else {
                Disposal::Free
            }
        }
        _ => Disposal::Message,
    }
}

fn needs_freeing(relid: i32) -> Vec<u8> {
    format!("Data of Relid {relid} needs to be freed\n").into_bytes()
}

/// [soft 0x08111b7c SI_elemdictDisposeRelationData]: disposes of the data of a relation
/// entry by the relation's id and clears the entry's pointer. Nothing for an entry
/// without data.
///
/// | Relation | What is done |
/// |---|---|
/// | 102, 103, 116, 150, 160, 239, 245, 247, 250 to 253, 255 | `free` |
/// | 220, 221, 230, 231, 800, 807 | `free` |
/// | 124 | nothing |
/// | 256, 257 | `SI_extractDataDispose`, `SI_trimmingDataDispose` on a copy of the pointer |
/// | 20000 | `SI_cusRelDataDestroy` on a copy of the pointer |
/// | 20010 | nothing while `SI_edSumatraTo3x` is set; `SI_exprRelDataDispose` for an entry whose chapter is EXPRESSIONS; else `free` |
/// | any other | the message `Data of Relid <n> needs to be freed` on the standard error |
pub fn elemdict_dispose_relation_data(m: &mut Mem, o: &mut dyn Outside, rel: Ptr32<ARelation>) {
    let data = m[rel].data.addr();
    if data == 0 {
        return;
    }
    match disposal(m, rel) {
        Disposal::Free => m.free(data),
        Disposal::Nothing => {}
        Disposal::Expr => expr_rel_data_dispose(m, data),
        Disposal::Message => o.stderr(&needs_freeing(m[rel].relid)),
        kind => {
            // these three take the address of a local that holds the pointer
            let local = m.local(4);
            m.set_u32(local, data);
            match kind {
                Disposal::Extract => extract_data_dispose(m, local),
                Disposal::Trimming => trimming_data_dispose(m, local),
                _ => cus_rel_data_destroy(m, local),
            }
            m.release(4);
        }
    }
    m[rel].data = Ptr32::NULL;
}

// adapter: not in place
/// `SI_elemdictDisposeRelationData` for the entries of a loaded scene. Data that is a
/// record of the image goes to [`elemdict_dispose_relation_data`]. Data that is a value
/// of the description's reader (an address of the handle range: the reader's own blocks
/// are not in the image) cannot be freed there: the routine's switch is followed, the
/// value stays with the scene, and the entry's pointer is cleared. What was disposed of
/// either way is noted in [`Scene::disposed_rel_data`].
pub fn dispose_relation_data(m: &mut Mem, sc: &mut Scene, rel: Ptr32<ARelation>) {
    let data = m[rel].data;
    if data.is_null() {
        return;
    }
    let relid = m[rel].relid;
    let kind = disposal(m, rel);
    if !matches!(kind, Disposal::Nothing | Disposal::Message) {
        sc.disposed_rel_data.push((relid, data));
    }
    if Kind::of(data.addr()).is_some() {
        if kind == Disposal::Message {
            sc.messages.push(Message { channel: Channel::Stderr, text: needs_freeing(relid) });
        }
        m[rel].data = Ptr32::NULL;
        return;
    }
    // the record no longer stands for its value once it is given up
    sc.rel_records.remove(&data.addr());
    struct Err<'a>(&'a mut Scene);
    impl Outside for Err<'_> {
        fn call(&mut self, _m: &mut Mem, _routine: &'static Routine, _args: &[u32]) -> u32 {
            0
        }
        fn stderr(&mut self, text: &[u8]) {
            self.0.messages.push(Message { channel: Channel::Stderr, text: text.to_vec() });
        }
    }
    elemdict_dispose_relation_data(m, &mut Err(sc), rel);
}

/// `SI_edSoftElemChangeName (0x080fe96c)` for an element of the image: nothing for a null
/// element, an element without object or a null name; else what the routine reads from
/// the dictionary (the element's place in its chapter of the current dictionary, its
/// object and its node of the scene graph, and for a cluster the name of the model it is
/// related to by relation 700) is handed to the routine's port on the objects
/// ([`crate::dictout::rename_object`]).
pub fn soft_elem_change_name(m: &mut Mem, sc: &mut Scene, e: Ptr32<Elem>, chap: i32, name: CStr32, un: &mut dyn Unported) {
    if e.is_null() || m[e].data.is_null() || name.is_null() {
        return;
    }
    let cid = cid(m);
    let ed = current_dict(m);
    let index = e.offset_from(m[list(m, ed, chap)].elems) as i32;
    let owner = if chap == cid.cls {
        find_related(m, ed, e, 0, 0, 700, cid.model, true).map(|(model, _)| node_text(m, m[model].node, |n| n.name).unwrap_or_default())
    } else {
        None
    };
    let text = m.cstr(name.addr());
    let (data, aux) = (sc.obj(m[e].data), sc.graf_handle(m[e].aux_data));
    crate::dictout::rename_object(sc, &cid, ElemId::new(chap, index), data, aux, &text, owner, un);
}

/// Set: [`new_image`] makes an image whose heap hands out blocks as the emulation harness
/// does ([`si_mem::HeapPolicy::Harness`]: none used again, `realloc` always a new block), so
/// that a check can pair the blocks a port takes with those the emulated original takes
/// (`re/tools/si-scene/worldcheck`, finding 0018, section 20). A check's switch: the
/// program's runs keep the default.
pub static HARNESS_HEAP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The image a scene is loaded into: zero, with the constants and the initial values of
/// the globals the two crates own.
pub fn new_image() -> Mem {
    let mut m = if HARNESS_HEAP.load(std::sync::atomic::Ordering::Relaxed) {
        Mem::with_heap(si_mem::Map::program(), si_mem::HeapPolicy::Harness)
    } else {
        Mem::program()
    };
    // the initial values of the globals of the crates whose routines run on this image
    // (they declare some of the same globals, with the binary's one initial value)
    si_model::model_image::init(&mut m);
    si_hrc::image::init(&mut m);
    si_elemio::image::globals::init(&mut m);
    si_mem::init_globals(&mut m, si_view::globals::ALL);
    si_mem::init_globals(&mut m, crate::image::ALL);
    si_mem::init_globals(&mut m, si_dict::globals::ALL);
    si_dict::rodata::store(&mut m);
    crate::globals::store(&mut m);
    crate::wave::store(&mut m);
    // the renderer's part of the binary's data segment (`SI_rightHand` 1, `mailboxCurrent`,
    // `cellVolumn`, `ptrDefault`, `SI_defaultMaterial`, the drand48 seed, the filter tables,
    // the texts): the process holds it from its start, before anything is loaded
    // (finding 0012, section 21; finding 0018, section 18). Without it `SI_rightHand` reads
    // 0 and every normal of a frame is turned round. It skips the words that are set
    // already (`SI_procData`) and the loader's switch `SI_addInDictionary`.
    si_render::data::init(&mut m);
    // the expression evaluator's statics and tables (`si_expr::image::rec::init_globals`:
    // the binary's initial values, finding 0013, section 12)
    si_expr::image::rec::init_globals(&mut m);
    m
}

/// A string of the stores as a block of the image; null for none.
fn put_str(m: &mut Mem, s: Option<&[u8]>) -> CStr32 {
    match s {
        None => Ptr32::NULL,
        Some(s) => dbnode::new_string(m, s),
    }
}

// adapter: not in place
/// Copies dictionaries held in stores into an image: the relation dictionary into `SI_RD`,
/// the chapter indices into `SI_cid`, the element dictionary into the first record of
/// `SI_elemDicts`, made current. For code that builds a dictionary in stores (the unit
/// tests, callers not converted yet) and then has a scene loaded into it. The blocks are
/// made in an order of this adapter's own, not by the original's allocation calls. An
/// element's object, its node of the scene graph and a relation's data are handles of the
/// stores' arenas there, and become addresses of the handle range here.
pub fn to_image(m: &mut Mem, d: &Dict, p: &Program) -> Ptr32<ElemDict> {
    m.write(SI_CID, p.cid);
    m[REL_SERIAL_NUMBER] = p.rel_serial;
    m[SI_USE_ELEM_LIST_SORT_TBL] = p.use_elem_list_sort_tbl as i16;
    m.set_i16(SI_GLB_MANIP.addr() + GLB_MANIP_36, p.backup_enabled as i16);
    m[crate::globals::LOADING] = p.loading as i16;
    m[crate::globals::SI_ADD_IN_DICTIONARY] = p.add_in_dictionary;
    // the relation dictionary
    let n = p.rd.rules.len() as u32;
    let rules: Ptr32<RelRule> = if n == 0 { Ptr32::NULL } else { Ptr32::new(m.calloc(n, 0x50)) };
    for (i, r) in p.rd.rules.iter().enumerate() {
        let mut rec = *r;
        rec.name = put_str(m, p.rd.names[i].as_deref());
        rec.cus_name = put_str(m, p.rd.cus_names[i].as_deref());
        m.write(rules.add(i as i32), rec);
    }
    m[SI_RD] = p.rd.rec;
    m[SI_RD].nb = n as i32;
    m[SI_RD].rules = rules;
    m[SI_RD].sort = Ptr32::NULL;
    if let Some(t) = &p.rd.sort {
        m[SI_RD].sort = put_sort_tbl(m, t);
    }
    // the element dictionary
    let ed: Ptr32<ElemDict> = Ptr32::new(m.calloc(1, 0x70));
    m.write(ed, d.ed.rec);
    let lists: Ptr32<ElemList> = Ptr32::new(m.calloc(d.ed.lists.len().max(1) as u32, 0x40));
    m[ed].nb_lists = d.ed.lists.len() as i32;
    m[ed].lists = lists;
    m[ed].chap_sort = Ptr32::NULL;
    m[ed].upd_list.nb = 0;
    m[ed].upd_list.nodes = Ptr32::NULL;
    m[ed].upd_list.dirty = 1;
    m[ed].topo_list.nb = 0;
    m[ed].topo_list.nodes = Ptr32::NULL;
    m[ed].topo_list.dirty = 1;
    m[ed].fwd_backup = Ptr32::NULL;
    m[ed].back_backup = Ptr32::NULL;
    m[ed].nb_fwd_backup = 0;
    m[ed].nb_back_backup = 0;
    for (c, l) in d.ed.lists.iter().enumerate() {
        let lp = lists.add(c as i32);
        let mut rec = l.rec;
        rec.name = put_str(m, Some(&l.name));
        rec.suffix = put_str(m, l.suffix.as_deref());
        rec.nb_elems = l.elems.len() as i32;
        rec.sort = Ptr32::NULL;
        rec.hier_order = Ptr32::NULL;
        rec.hier_count = Ptr32::NULL;
        rec.subdir = Ptr32::NULL;
        rec.subdirs = Ptr32::NULL;
        rec.subdir_aux = Ptr32::NULL;
        rec.nb_subdirs = 0;
        rec.sort_tbl = match &l.sort_tbl {
            Some(t) => put_sort_tbl(m, t),
            None => Ptr32::NULL,
        };
        let elems: Ptr32<Elem> = if l.elems.is_empty() { Ptr32::NULL } else { Ptr32::new(m.calloc(l.elems.len() as u32, 0x84)) };
        rec.elems = elems;
        m.write(lp, rec);
        for (i, e) in l.elems.iter().enumerate() {
            let mut rec = e.rec;
            rec.node = match d.nodes.get(e.rec.node.0) {
                Some(n) => dbnode::from_value(m, n),
                None => Ptr32::NULL,
            };
            rec.data = obj_ptr(e.rec.data.0);
            rec.aux_data = graf_ptr(e.rec.aux_data.0);
            rec.root_srt = Ptr32::NULL;
            rec.icon = Ptr32::NULL;
            rec.suffix = Ptr32::NULL;
            for (side, stores) in [(0, &e.forward), (1, &e.backward)] {
                let groups: Ptr32<RelList> = if stores.is_empty() { Ptr32::NULL } else { Ptr32::new(m.calloc(stores.len() as u32, 0x18)) };
                for (g, s) in stores.iter().enumerate() {
                    let mut lrec = s.rec;
                    lrec.nb = s.rels.len() as i32;
                    lrec.capacity = lrec.capacity.max(lrec.nb);
                    lrec.rels = Ptr32::NULL;
                    if !s.rels.is_empty() {
                        let rels: Ptr32<ARelation> = Ptr32::new(m.calloc(lrec.capacity as u32, 0x20));
                        for (k, r) in s.rels.iter().enumerate() {
                            let mut r = *r;
                            r.data = rel_data_ptr(r.data.0);
                            m.write(rels.add(k as i32), r);
                        }
                        lrec.rels = rels;
                    }
                    m.write(groups.add(g as i32), lrec);
                }
                let side_rec = Relations { nb_groups: stores.len() as i32, groups };
                if side == 0 {
                    rec.forward = side_rec;
                } else {
                    rec.backward = side_rec;
                }
            }
            m.write(elems.add(i as i32), rec);
        }
    }
    // made the one dictionary of the program, and the current one
    m[si_dict::globals::SI_NB_ELEM_DICTS] = 1;
    m[SI_ELEM_DICTS] = ed;
    let exists: Ptr32<i16> = Ptr32::new(m.calloc(1, 2));
    m[exists] = 1;
    m[si_dict::globals::ELEM_DICT_EXISTS] = exists;
    let proc = m[SI_PROC_DATA];
    m[proc] = 0;
    ed
}

fn put_sort_tbl(m: &mut Mem, t: &SortTable) -> Ptr32<SortTbl> {
    let tbl: Ptr32<SortTbl> = Ptr32::new(m.calloc(1, 0xc));
    let table: Ptr32<i32> = Ptr32::new(m.calloc(t.table.len().max(1) as u32, 4));
    for (i, v) in t.table.iter().enumerate() {
        m[table.add(i as i32)] = *v;
    }
    m[tbl].count = t.count;
    m[tbl].table = table;
    m[tbl].valid = t.valid;
    tbl
}

/// What [`to_stores`] makes of an image.
pub struct Stores {
    pub dict: Dict,
    pub program: Program,
    pub upd_list: UpdListStore,
    /// The hierarchical orders of the MODELS and ANIMATION chapters (`ElemList` +0x10,
    /// +0x14), where the chapter has them.
    pub model_order: Option<(Vec<i32>, Vec<i32>)>,
    pub anim_order: Option<(Vec<i32>, Vec<i32>)>,
    /// The nine numbers of root models' `SRT` items still in the image (`__SI_Elem` +0x3c).
    pub srt: Arena<[f32; 9]>,
}

fn relations_to_stores(m: &Mem, sc: &Scene, side: Relations, owner: (i32, i32, bool)) -> Vec<RelListStore> {
    if side.groups.is_null() {
        return Vec::new();
    }
    (0..side.nb_groups)
        .map(|g| {
            let l = side.groups.add(g);
            let mut rec = m.read(l);
            let nths = if rec.rels.is_null() || rec.nb <= 0 { Vec::new() } else { entry_nths(m, &rec) };
            let rels: Vec<ARelation> = if rec.rels.is_null() || rec.nb <= 0 {
                Vec::new()
            } else {
                (0..rec.nb)
                    .map(|k| {
                        let mut r = m.read(rec.rels.add(k));
                        r.data = Ptr32::new(rel_data_of(sc, r.data));
                        if owner.2 && (r.data.is_null() || r.relid == 20010) {
                            // a fitting's data the load gave to its fitting record, an
                            // expression's the regeneration replaced: the description's
                            let key = FitKey { chap: owner.0, elem: owner.1, group: g, relid: r.relid, other_chap: r.chapid, other: r.elemid, nth: nths[k as usize] };
                            if let Some(&h) = sc.fitting_data.get(&key) {
                                r.data = Ptr32::new(h);
                            }
                        }
                        r
                    })
                    .collect()
            };
            // the stores hold no addresses of the image: the array's is dropped
            rec.rels = Ptr32::NULL;
            RelListStore { rec, rels }
        })
        .collect()
}

fn order_to_stores(m: &Mem, l: Ptr32<ElemList>) -> Option<(Vec<i32>, Vec<i32>)> {
    let (order, count, n) = (m[l].hier_order, m[l].hier_count, m[l].nb_elems);
    if order.is_null() || count.is_null() {
        return None;
    }
    Some(((0..n).map(|i| m[order.add(i)]).collect(), (0..n).map(|i| m[count.add(i)]).collect()))
}

/// A relation entry by what names it: the element that holds it (forward side), the list,
/// the relation and the element at the other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FitKey {
    pub chap: i32,
    pub elem: i32,
    pub group: i32,
    pub relid: i32,
    pub other_chap: i16,
    pub other: i32,
    /// Which of the entries with all the above it is, in the list's order.
    pub nth: u32,
}

/// The occurrence number of each entry of a list among the entries with the same relation
/// and the same other end, in the list's order (for [`FitKey::nth`]).
pub fn entry_nths(m: &Mem, rl: &RelList) -> Vec<u32> {
    let mut seen: Vec<(i32, i16, i32)> = Vec::new();
    (0..rl.nb.max(0))
        .map(|k| {
            let r = m.read(rl.rels.add(k));
            let key = (r.relid, r.chapid, r.elemid);
            let n = seen.iter().filter(|&&x| x == key).count() as u32;
            seen.push(key);
            n
        })
        .collect()
}

// adapter: not in place
/// The data of the fittings' relations (220, 221, 230, 231) before `SI_scnRelationsFromElemDict`
/// gives it to the fitting record and frees it (finding 0018, 17.3): the original's save
/// makes the data again from the fitting record; the saver of the stores reads the value
/// the description held, so [`to_stores`] puts it back where the image's entry has none.
/// `entries` are those the routine is about to see: (key, data's handle).
pub fn fitting_data_before(m: &Mem, sc: &Scene, ed: Ptr32<ElemDict>, chap: i32, group: i32, relid: i32) -> Vec<(FitKey, u32, Ptr32<ARelation>)> {
    let mut out = Vec::new();
    if ![0xdc, 0xdd, 0xe6, 0xe7].contains(&relid) || chap < 0 || chap >= m[ed].nb_lists {
        return out;
    }
    let l = list(m, ed, chap);
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        let side = m[e].forward;
        if side.groups.is_null() || group >= side.nb_groups {
            continue;
        }
        let rl = m.read(side.groups.add(group));
        let nths = entry_nths(m, &rl);
        for k in 0..rl.nb.max(0) {
            let rp = rl.rels.add(k);
            let r = m.read(rp);
            let h = rel_data_of(sc, r.data);
            if r.relid == relid && h != 0 {
                out.push((FitKey { chap, elem: i, group, relid, other_chap: r.chapid, other: r.elemid, nth: nths[k as usize] }, h, rp));
            }
        }
    }
    out
}

// adapter: not in place
/// The dictionaries of an image as the stores the rest of this crate still works on (the
/// saver) and `si-rensetup` reads: the
/// reverse of [`to_image`]. Every pointer of the image becomes what the stores hold in
/// its place: a name record a handle of [`Dict::nodes`], an object and a node of the
/// scene graph their handles in the scene's arenas, a relation's data its handle, a root
/// model's nine numbers a handle of [`Stores::srt`]; pointers to arrays are null.
pub fn to_stores(m: &Mem, ed: Ptr32<ElemDict>, sc: &Scene) -> Stores {
    let cid = cid(m);
    let mut program = Program::new();
    program.cid = cid;
    program.rel_serial = m[REL_SERIAL_NUMBER];
    program.def_color = sc.answers.def_color;
    program.load_optim = sc.answers.load_optim;
    program.use_elem_list_sort_tbl = m[SI_USE_ELEM_LIST_SORT_TBL] != 0;
    program.loading = m[crate::globals::LOADING] != 0;
    program.backup_enabled = m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_36) != 0;
    program.add_in_dictionary = m[crate::globals::SI_ADD_IN_DICTIONARY];
    program.now = sc.answers.now;
    // the relation dictionary
    let mut rd = RelDictStore::new();
    rd.rec = m.read(SI_RD);
    for i in 0..rd.rec.nb {
        let mut r = m.read(rd.rec.rules.add(i));
        rd.names.push(dbnode::text(m, r.name));
        rd.cus_names.push(dbnode::text(m, r.cus_name));
        r.name = Ptr32::NULL;
        r.cus_name = Ptr32::NULL;
        rd.rules.push(r);
    }
    if !rd.rec.sort.is_null() {
        let t = m.read(rd.rec.sort);
        rd.sort = Some(SortTable { count: t.count, table: (0..rd.rec.nb).map(|i| m[t.table.add(i)]).collect(), valid: t.valid });
    }
    rd.rec.rules = Ptr32::NULL;
    rd.rec.sort = Ptr32::NULL;
    program.rd = rd;
    // the element dictionary
    let mut dict = Dict::new();
    let mut srt = Arena::new();
    let mut rec = m.read(ed);
    let nb_lists = rec.nb_lists;
    let lists_ptr = rec.lists;
    // the update list
    let mut upd_list = UpdListStore::default();
    for i in 0..rec.upd_list.nb {
        let n: Ptr32<UpdNode> = rec.upd_list.nodes.cast::<UpdNode>().add(i);
        let node = m.read(n);
        let args = (0..node.nb_args as i32)
            .map(|k| {
                let a: UpdArg = m.read(node.args.add(k));
                StoreArg { chapid: a.chapid, group: a.group, elemid: a.elemid, data: Ptr32::new(sc.obj(a.data)), flag: a.active, fcurve: Ptr32::new(sc.obj(a.active_data)) }
            })
            .collect();
        upd_list.nodes.push(StoreNode {
            chapid: node.chapid,
            group: node.group,
            elemid: node.elemid,
            data: Ptr32::new(sc.obj(node.data)),
            relid: node.relid,
            args,
            flag: node.active,
            rel_data: Ptr32::new(rel_data_of(sc, node.rel_data)),
        });
    }
    rec.lists = Ptr32::NULL;
    rec.chap_sort = Ptr32::NULL;
    rec.upd_list.nodes = Ptr32::NULL;
    rec.topo_list.nodes = Ptr32::NULL;
    rec.fwd_backup = Ptr32::NULL;
    rec.back_backup = Ptr32::NULL;
    let mut store = ElemDictStore { rec, lists: Vec::new(), chap_sort: Vec::new(), fwd_backup: Vec::new(), back_backup: Vec::new() };
    let (mut model_order, mut anim_order) = (None, None);
    for c in 0..nb_lists {
        let lp = lists_ptr.add(c);
        let mut lrec = m.read(lp);
        if c == cid.model {
            model_order = order_to_stores(m, lp);
        }
        if c == cid.ani {
            anim_order = order_to_stores(m, lp);
        }
        let name = m.cstr(lrec.name.addr());
        let suffix = dbnode::text(m, lrec.suffix);
        let sort_tbl = (!lrec.sort_tbl.is_null()).then(|| {
            let t = m.read(lrec.sort_tbl);
            SortTable { count: t.count, table: (0..lrec.nb_elems).map(|i| m[t.table.add(i)]).collect(), valid: t.valid }
        });
        let mut elems = Vec::with_capacity(lrec.nb_elems.max(0) as usize);
        for i in 0..lrec.nb_elems {
            let mut e = m.read(lrec.elems.add(i));
            let forward = relations_to_stores(m, sc, e.forward, (c, i, true));
            let backward = relations_to_stores(m, sc, e.backward, (c, i, false));
            e.node = match dbnode::to_value(m, e.node) {
                Some(n) => crate::dict::node_ptr(dict.nodes.alloc(n)),
                None => Ptr32::NULL,
            };
            e.data = Ptr32::new(sc.obj(e.data));
            e.aux_data = Ptr32::new(match sc.graf_handle(e.aux_data) {
                0 => sc.contents.standin.get(&(c, i)).copied().unwrap_or(0),
                h => h,
            });
            e.root_srt = if e.root_srt.is_null() {
                Ptr32::NULL
            } else {
                let mut v = [0f32; 9];
                for (k, x) in v.iter_mut().enumerate() {
                    *x = m[e.root_srt.add(k as i32)];
                }
                Ptr32::new(srt.alloc(v))
            };
            e.forward.groups = Ptr32::NULL;
            e.backward.groups = Ptr32::NULL;
            e.icon = Ptr32::NULL;
            e.suffix = Ptr32::NULL;
            elems.push(ElemStore { rec: e, forward, backward });
        }
        lrec.name = Ptr32::NULL;
        lrec.suffix = Ptr32::NULL;
        lrec.elems = Ptr32::NULL;
        lrec.sort = Ptr32::NULL;
        lrec.hier_order = Ptr32::NULL;
        lrec.hier_count = Ptr32::NULL;
        lrec.subdir = Ptr32::NULL;
        lrec.subdirs = Ptr32::NULL;
        lrec.subdir_aux = Ptr32::NULL;
        lrec.sort_tbl = Ptr32::NULL;
        store.lists.push(ElemListStore { rec: lrec, name, suffix, elems, sort_tbl });
    }
    dict.ed = store;
    dict.disposed.rel_data = sc.disposed_rel_data.iter().map(|&(relid, p)| (relid, Ptr32::new(rel_data_of(sc, p)))).collect();
    Stores { dict, program, upd_list, model_order, anim_order, srt }
}

/// A slot's status as the routines that go through `SI_elemdictGetElem` see it: the slot,
/// unless it is out of range or freed (status 2).
pub fn get_elem(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, index: i32) -> Option<Ptr32<Elem>> {
    let e = slot(m, ed, ElemId::new(chap, index))?;
    (m[e].status != ELEM_DELETED).then_some(e)
}

/// `&elem->forward.groups[group]` or the backward one, null when the side has no such
/// list (the original indexes without the test).
pub fn rel_list(m: &Mem, e: Ptr32<Elem>, forward: bool, group: i32) -> Ptr32<RelList> {
    let side: Relations = if forward { m[e].forward } else { m[e].backward };
    if group < 0 || group >= side.nb_groups || side.groups.is_null() {
        return Ptr32::NULL;
    }
    side.groups.add(group)
}

/// The address of one side's record of an element.
pub fn side_ptr(e: Ptr32<Elem>, forward: bool) -> Ptr32<Relations> {
    if forward {
        field!(e, Elem, forward)
    } else {
        field!(e, Elem, backward)
    }
}
