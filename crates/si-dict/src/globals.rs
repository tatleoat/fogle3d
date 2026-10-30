// SPDX-License-Identifier: MIT
//! The global variables of the dictionary source files, at their addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py`.

use si_types::dict::{AttributeDict, ChapIds, ChapInfo, ElemDict, EnumValDict, RelDict};

si_mem::globals! {
    /// The array of dictionaries (`SI_elemdictAdd` grows it by one record).
    SI_ELEM_DICTS: si_mem::Ptr32<ElemDict> = 0x0867931c, "SI_elemDicts", init 0;
    /// How many slots the array has.
    SI_NB_ELEM_DICTS: i32 = 0x08679318, "SI_nbElemDicts", init 0;
    /// One 16-bit flag per slot: non-zero while the dictionary exists.
    ELEM_DICT_EXISTS: si_mem::Ptr32<i16> = 0x08683b80, "elemDictExists", init 0;
    /// The per-process data: a pointer to a record whose first word is the index of the
    /// current dictionary (`SI_elemdictCusGetRelData` reads it as 32 bits).
    SI_PROC_DATA: si_mem::Ptr32<i32> = 0x08679314, "SI_procData", init 0x08782108;
    /// The relation dictionary.
    SI_RD: RelDict = 0x087815f0, "SI_RD";
    /// The attribute dictionary.
    SI_ATD: AttributeDict = 0x08781438, "SI_ATD";
    /// The enum dictionary.
    SI_EVD: EnumValDict = 0x08781454, "SI_EVD";
    /// The scene dictionary's chapter indices.
    SI_CID: ChapIds = 0x08781660, "SI_cid";
    /// The database dictionary's chapter indices.
    SI_DBCID: ChapIds = 0x08781700, "SI_dbcid";
    /// The database browser's dictionary.
    SI_DBED: ElemDict = 0x087813a0, "SI_dbed";
    /// Name, suffix and tag of the 36 chapters (the table itself is the binary's data; a
    /// crate that needs it stores it).
    SI_CHAP_INFO: [ChapInfo; 36] = 0x086842c0, "SI_chapInfo";
    /// `SI_reldictGetRelIndexFromId`'s memory of its last question: the dictionary …
    LAST_PRD: si_mem::Ptr32<RelDict> = 0x08684560, "last_prd.246", init 0;
    /// … the relation id (0x7fffffff: none) …
    LAST_RELID: i32 = 0x08684564, "last_relid.247", init 0x7fffffff;
    /// … and the answer.
    LAST_INDEX: i32 = 0x08684568, "last_index.248", init -1;
    /// The counter `SI_elemdictGetRelSerialNumber` hands out.
    REL_SERIAL_NUMBER: i32 = 0x08684480, "RelSerialNumber";
    /// The dictionary `SI_elemdictSortChapNames` is sorting (read by its comparison).
    GLB_ED: si_mem::Ptr32<ElemDict> = 0x08787cf4, "glbED";
    /// The chapter `SI_elemlistSortChapterElements` is sorting.
    GLB_EL: si_mem::Ptr32<si_types::dict::ElemList> = 0x08787cf8, "glbEL";
    /// Whether that sort is the hierarchical one.
    GLB_HIER_SORT: i16 = 0x08787cfc, "glbHierSort";
    /// Its direction.
    GLB_SORT_UP: i16 = 0x08787cfe, "glbSortUp";
    /// Its key.
    GLB_SORTTYPE: i32 = 0x08787d00, "glbSorttype";
    /// The two buffers the element comparison prints "prefix-name" into (sort key 10).
    STR1: [u8; 512] = 0x08787d20, "str1";
    STR2: [u8; 512] = 0x08787f20, "str2";
    /// `SI_elemdictFindElemByData`'s memory of its last question: the chapter …
    LAST_PEL: si_mem::Ptr32<si_types::dict::ElemList> = 0x08683bf8, "last_pel.383", init 0;
    /// … the object …
    LAST_DATA: u32 = 0x08683bfc, "last_data.384", init 0;
    /// … and the answer.
    LAST_INDEX_385: i32 = 0x08787ae0, "last_index.385";
    /// Whether look-ups by object may use a chapter's sort table (set by the scene loader).
    SI_USE_ELEM_LIST_SORT_TBL: i16 = 0x08679284, "SI_useElemListSortTbl";
    /// The number `SI_elemdictGetNewNameId` last handed out.
    NUM_510: i32 = 0x08683e28, "num.510", init 0;
    /// The name it answers (a buffer the routine owns).
    NEWNAME_509: [u8; 500] = 0x08787b00, "newname.509";
    /// The five buffers `SI_dbScnGetListNode` has a file name split into: prefix …
    PROJ_246: [u8; 0x200] = 0x087876c0, "proj.246";
    /// … name …
    NAME_247: [u8; 0x200] = 0x087878c0, "name.247";
    /// … version …
    VERS_248: [u8; 10] = 0x08787ac0, "vers.248";
    /// … substitution …
    PREC_249: [u8; 10] = 0x08787aca, "prec.249";
    /// … suffix.
    SUFF_250: [u8; 10] = 0x08787ad4, "suff.250";
    /// Options of the viewer's update; bit 2 is the fast hierarchy update, under which an
    /// update list gets an end mark.
    SI_VIEWER_UPD_OPT: i32 = 0x08679678, "SI_viewerUpdOpt", init 0;
    /// The attribute dictionary whose curve index is being sorted or searched (read by
    /// the two comparison functions).
    GLB_ATD: si_mem::Ptr32<AttributeDict> = 0x08785dc0, "glbATD";
    /// `SI_attdictFindFcurve`'s memory of its last answer: the position in the index …
    LAST_SORT_IND_327: i32 = 0x08682788, "last_sort_ind.327", init -1;
    /// … and the curve id.
    LAST_FCV_ID_328: i32 = 0x0868278c, "last_fcv_id.328", init -1;
    /// What `SI_attdictGetNameFromFcurve` answers for a curve without a name: a pointer
    /// to the literal `notfound`.
    DEFAULT_NAME_347: si_mem::CStr32 = 0x086827b8, "default_name.347", init 0x086827ac;
    /// The manipulation globals; the dictionaries read the 16-bit word at +0x36
    /// ([`GLB_MANIP_36`]).
    SI_GLB_MANIP: [u8; 0x3c] = 0x08781560, "SI_glbManip";
}

/// Offset in `SI_glbManip` of the 16-bit word that, when set, makes a restricted forward
/// sort climb the hierarchy at an entry of relation 110 (`topoSort`).
pub const GLB_MANIP_2E: u32 = 0x2e;

/// Offset in `SI_glbManip` of the 16-bit word that, when 0, makes every change of a
/// relation mark the dictionary's hierarchy dirty (finding 0018 names its address).
pub const GLB_MANIP_36: u32 = 0x36;
