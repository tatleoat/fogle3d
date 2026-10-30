// SPDX-License-Identifier: MIT
//! Layouts of the element dictionaries: elements, chapters, relations, attributes, enums.
//!
//! A scene is a dictionary (`SI_ElemDict`) of chapters (`SI_ElemList`), each an array of
//! elements (`__SI_Elem`). An element is identified by two numbers: the index of its chapter
//! in the dictionary and its own index in the chapter's array. Relations between elements
//! are stored twice, once on each end, as `SI_ARelation` entries that name the other end by
//! those two numbers.
//!
//! Evidence strings name functions of the Linux `soft` binary; "attribute N" is the
//! parameter id N of the vendor's `Relations.rsrc`, whose table names every id.

use crate::{layout, CStr32, Code, Opaque, Ptr32};

/// The per-element icon record. Only its disposal is read: `SI_dbIconFree` (0x08135a8c)
/// frees a pointer at +8, then the record. Size and other fields not established.
pub enum ElemIcon {}
/// A node of an update list: 0x2c bytes each, with two owned pointers at +0x1c and +0x28
/// that `SI_updlistDispose` (0x08141554) frees. Not laid out here.
pub enum UpdElem {}
/// A condition attached to an attribute group (`SI_AttribCond`). Not laid out here.
pub enum AttribCond {}
/// The database a dictionary was loaded from (`SI_DbInfo`). Not laid out here.
pub enum DbInfo {}

layout! {
    /// One element of a chapter (`__SI_Elem`).
    pub struct Elem = "__SI_Elem", size 0x84,
        "SI_elemlistAddElem allocates 0x84 bytes for the first element and grows the array by 0x84; every element loop steps by 0x84" {
        0x00 node: Ptr32<DbListNode>, Read, "Name record. SI_elemlistAddElem stores its SI_DbListNode argument here; SI_elemdictSetDeleteElem frees it with SI_dbListNodeFree.";
        0x04 data: Ptr32<Opaque>, Read, "The object the element stands for (__SI_Model, SI_Camera, light, __SI_Material, __SI_LclMap, __SI_Texture3D, fcurve, ... by chapter). SI_elemdictFindElemByData compares it; 46 functions read it as a model.";
        0x08 aux_data: Ptr32<Opaque>, Read, "Second object: for scene objects the __SI_GrafNode (grafnodeElemAssoc stores it, SI_elemdictFindElemByAuxData compares it).";
        0x0c schem_x: f32, Read, "Position in the schematic view: SI_elemdictSchemSetPosX; attribute 269 (SchPos); first value of the SCHEM line.";
        0x10 schem_y: f32, Read, "SI_elemdictSchemSetPosY; attribute 270; second value of the SCHEM line.";
        0x14 schem_width: f32, Named, "si_schemSetElemWidth returns it as the stored width when the element is not being laid out.";
        0x18 schem_collapse: i32, Read, "Attribute 271 (SchCollapse) in SI_schemAttrGetValue; third value of the SCHEM line.";
        0x1c wire_color: i32, Read, "Attribute 267 (WireColor): SI_generalAttrSetValue stores SI_color1 + value + SI_select7WM; written as WIRECOL; its low 16 bits are copied to the graf node. Set to SI_grafnodeDefColor() at creation.";
        0x20 wire_type: i32, Read, "Attribute 268 (WireType: 0 hidden, 1 ghosted, 2 visible); 2 at creation; second value of the DISPLAY line; copied to the graf node.";
        0x24 selection: i32, Read, "First value of the DISPLAY line; grafnodeElemAssoc copies it to the graf node's selection state (SelType: 0 none, 1 node, 2 inherited, 3 branch).";
        0x28 dope_state: i16, Unknown, "Read by dopeRelUpdate, which passes on the values 3 and 4; meaning not established.";
        0x2a schem_flags: u8, Named, "Bit 0 is written as the word USR on the SCHEM line; bit 1 is tested by si_schemSetElemWidth. Cleared as 16 bits at creation.";
        0x2b pad_2b: u8, Unknown, "Cleared with schem_flags; no other access found.";
        0x2c schem_bbox: [f32; 4], Named, "Four floats written by SI_schemComputeElemBBox; the element sort uses [2]-[0] and [3]-[1] as extents.";
        0x3c root_srt: Ptr32<f32>, Read, "Nine floats printed as the SRT line by dbWriteRootModelSRT, which then frees them.";
        0x40 dirty: i16, Read, "SI_elemdictSetDirty; 1 at creation.";
        0x42 selected: i16, Named, "SI_elemdictSetSelection sets it in every element; returned as attribute 265 (Selection) when the element has no graf node. The topological sort also uses it as a mark.";
        0x44 shared: i16, Named, "Written by dialogSetShare; SI_generalAttrSetValue refuses to rename an element when it is non-zero.";
        0x46 new_flag: i16, Read, "SI_elemdictSetNewFlag rewrites it; 2 at creation, 1 after a copy between dictionaries or a load, 0 when deleted.";
        0x48 status: i16, Read, "0 = in use, 2 = deleted (the slot may be reused by SI_elemlistAddElem). Every look-up tests it.";
        0x4a subst: i16, Read, "Substitution number: SI_dbfilenameChangePrecision counts it up or down and prints it into the name record.";
        0x4c dsc_id: i32, Read, "Sixth argument of SI_elemdictAddElem: the scene description the element was read for; elemdictGetElemFromWorkId matches it together with work_id.";
        0x50 root_id: i32, Read, "Index of the root of the element's hierarchy (its own index at creation): SI_elemdictUpdateHierInfo. SI_elemdictSetDeleteElem stores the element's own index.";
        0x54 hier_level: i32, Read, "Depth in the hierarchy, 0 for a root: SI_elemdictUpdateHierInfo; attribute 266 (HierLevel).";
        0x58 work_flag: i32, Read, "si_elemdictSetFlag, SI_elemlistClearWorkFlag. The 'low byte' routines address the byte at +0x5b, the low byte on the big-endian machines the code was written for and the high byte here.";
        0x5c work_id: i32, Read, "Seventh argument of SI_elemdictAddElem: the element's number in the description file being read; the writer stores the running count here.";
        0x60 forward: Relations, Read, "Relations of which this element is the source (SI_elemdictAddCompleteRelation adds the entry carrying the data here).";
        0x68 backward: Relations, Read, "Relations of which this element is the destination.";
        0x70 icon: Ptr32<ElemIcon>, Read, "Freed by SI_dbIconFree.";
        0x74 seq_first: i32, Read, "First frame of a picture sequence: si_dbFormatSequenceFrames prints [first - last]. Not set by SI_elemlistAddElem.";
        0x78 seq_last: i32, Read, "Last frame of the sequence.";
        0x7c seq_unknown: i16, Unknown, "Read by SI_dbListPrepareIcons.";
        0x7e seq_braces: i16, Named, "Non-zero makes si_dbFormatSequenceFrames print braces instead of brackets; what the two forms distinguish is not read.";
        0x80 suffix: CStr32, Named, "A string freed with the element; the element sort (key 12) uses it in place of the chapter's file suffix.";
    }
}

layout! {
    /// One chapter of a dictionary (`SI_ElemList`).
    pub struct ElemList = "SI_ElemList", size 0x40,
        "SI_elemdictAddList allocates 0x40 bytes for the first chapter and grows the array by 0x40" {
        0x00 name: CStr32, Read, "Chapter name: stored by SI_elemdictAddList, compared by SI_elemdictFindListByName.";
        0x04 nb_elems: i32, Read, "Number of slots in elems, deleted ones included.";
        0x08 elems: Ptr32<Elem>, Read, "Array of nb_elems elements.";
        0x0c sort: Ptr32<i32>, Read, "Element indices in sorted order, built by SI_elemlistSortChapterElements.";
        0x10 hier_order: Ptr32<i32>, Named, "Built by SI_elemdictSortAnimElements; the hierarchical sort reads element indices through it.";
        0x14 hier_count: Ptr32<i32>, Named, "Built with hier_order; the hierarchical sort reads a run length per root from it.";
        0x18 kind: i16, Named, "1 at creation; SI_elemdictLoadChapters sets 0 for NSELECTIONS and CHANNEL_DRIVERS (chapters with no files) and 2 for OUTSIDE_DB_FILES.";
        0x1a pad_1a: [u8; 2], Unknown, "No access found.";
        0x1c unk_1c: i32, Unknown, "Written by txtlistTransferSelection2ElemList.";
        0x20 unk_20: i32, Unknown, "Cleared at creation only.";
        0x24 unk_24: i32, Unknown, "Read by dbGetIconData.";
        0x28 subdir: CStr32, Named, "Current sub-directory: dbSetSubDirectory; freed with the chapter.";
        0x2c subdirs: Ptr32<CStr32>, Named, "nb_subdirs strings filled by SI_dbChapterGetSubDirs.";
        0x30 nb_subdirs: i32, Named, "Length of subdirs and subdir_aux.";
        0x34 subdir_aux: Ptr32<Ptr32<Opaque>>, Named, "Parallel to subdirs, each entry freed with the chapter.";
        0x38 sort_tbl: Ptr32<SortTbl>, Read, "Only when SI_LOAD_OPTIM is set: a table for finding an element by its data pointer.";
        0x3c suffix: CStr32, Read, "The chapter's file suffix, copied by SI_elemdictLoadChapters from the chapter table.";
    }
}

layout! {
    /// Display options of the database browser, kept twice in a dictionary.
    pub struct ListOptions = "(part of SI_ElemDict)", size 0x18,
        "SI_elemdictInit copies six words from +0x18 to +0x30" {
        0x00 sort_type: i32, Named, "6 at creation; SI_elemdictLoadChapters sets 9, the value the element sort treats as 'by full name'. Written by dialogListOptions.";
        0x04 unk_04: i16, Unknown, "3 at creation; dialogListOptions, dbListFormatItemString.";
        0x06 unk_06: i16, Unknown, "0 at creation; dbListCollapseAll writes it.";
        0x08 unk_08: i16, Unknown, "1 at creation; SI_dialogDbExchange.";
        0x0a unk_0a: i16, Unknown, "0 at creation, 1 after SI_elemdictLoadChapters; the list builders read it.";
        0x0c unk_0c: i32, Unknown, "-1 at creation; SI_dialogDatabases writes it.";
        0x10 unk_10: i16, Unknown, "0 at creation.";
        0x12 unk_12: i16, Unknown, "0 at creation.";
        0x14 unk_14: i16, Unknown, "1 at creation; SI_dbMFilesDelete and others read it.";
        0x16 pad_16: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A dictionary of elements (`SI_ElemDict`).
    pub struct ElemDict = "SI_ElemDict", size 0x70,
        "SI_elemdictAdd grows the global array by 0x70; the symbol SI_dbed, one dictionary, is 0x70 bytes" {
        0x00 nb_lists: i32, Read, "Number of chapters.";
        0x04 lists: Ptr32<ElemList>, Read, "Array of chapters.";
        0x08 chap_sort: Ptr32<i32>, Read, "Chapter indices ordered by name (SI_elemdictSortChapNames).";
        0x0c upd_list: UpdList, Named, "Disposed with SI_updlistDispose; its third word is set to 1 by every routine that adds or removes an element or a relation.";
        0x18 options: ListOptions, Named, "Browser options; the copy written when the dictionary is a scene.";
        0x30 options_db: ListOptions, Named, "The copy written when the dictionary is the database's.";
        0x48 dbinfo: Ptr32<DbInfo>, Named, "SI_dbLoadElementsFromDict stores a SI_DbInfo pointer.";
        0x4c topo_list: UpdList, Named, "Used by the SI_elemdictTopoSort routines; disposed with SI_updlistDispose. Its third word is set like upd_list's.";
        0x58 is_scene: i16, Read, "Third argument of SI_elemdictLoadChapters: 1 for a scene dictionary, 0 for the database dictionary SI_dbed.";
        0x5a rel_dirty: i16, Named, "Set to 1 whenever a relation is added or removed.";
        0x5c nb_fwd_backup: i32, Read, "Length of fwd_backup (SI_edAllocMdlRelBackUp, relBackUpDispose).";
        0x60 fwd_backup: Ptr32<Relations>, Read, "One Relations per model element: a saved copy of forward relations.";
        0x64 nb_back_backup: i32, Read, "Length of back_backup.";
        0x68 back_backup: Ptr32<Relations>, Read, "Saved copy of backward relations.";
        0x6c untold: i16, Unknown, "0 at creation; written by SI_edSwapUnToldRel and dbCheckExtraUnToldRelation.";
        0x6e hier_dirty: i16, Named, "1 at creation and after every change; SI_elemdictAddElem saves model relations only while it is 0.";
    }
}

layout! {
    /// The name of an element, in the parts of a database file name (`SI_DbListNode`).
    pub struct DbListNode = "SI_DbListNode", size 0x34,
        "SI_dbListNodeAllocate allocates 0x34 bytes" {
        0x00 unk_00: i32, Unknown, "0 at creation.";
        0x04 unk_04: Ptr32<Opaque>, Unknown, "0 at creation; freed by SI_dbListNodeFree.";
        0x08 str_08: CStr32, Unknown, "An empty string at creation; sort key 1 compares it.";
        0x0c str_0c: CStr32, Unknown, "Second argument of SI_dbScnGetListNode when not empty; sort key 2 compares it.";
        0x10 prefix: CStr32, Read, "Attribute 261 (Prefix).";
        0x14 name: CStr32, Read, "Attribute 262 (Name).";
        0x18 version: CStr32, Read, "Attribute 263 (Revision), parsed with strtol; the string 1 at creation.";
        0x1c subst: CStr32, Read, "Attribute 264 (Subst), parsed with strtol; the string 0 at creation.";
        0x20 time: i32, Read, "time(NULL) at creation; sort key 6.";
        0x24 fullname: CStr32, Read, "Result of SI_dbFormatFullName; sort key 9.";
        0x28 unk_28: i16, Unknown, "0 at creation.";
        0x2a unk_2a: i16, Unknown, "0 at creation.";
        0x2c mode: u16, Named, "Sort key 11 tests (mode & 0xf000) == 0x4000, the directory test of a file mode.";
        0x2e pad_2e: [u8; 2], Unknown, "No access found.";
        0x30 unk_30: i32, Unknown, "Sort key 13 compares it as an integer. Not set at creation.";
    }
}

layout! {
    /// One direction of an element's relations (`SI_Relations`).
    pub struct Relations = "SI_Relations", size 0x8,
        "embedded twice in __SI_Elem, 8 bytes apart; arrays of them step by 8 in relBackUpDispose" {
        0x00 nb_groups: i32, Read, "Number of lists; si_relAllocate sets 6.";
        0x04 groups: Ptr32<RelList>, Read, "Array of nb_groups lists (si_relAllocate allocates 0x90 bytes).";
    }
}

layout! {
    /// The relations of one group on one side of an element (`SI_RelList`).
    pub struct RelList = "SI_RelList", size 0x18,
        "si_relAllocate initialises six of them in 0x90 bytes, stepping by 0x18" {
        0x00 nb: i32, Read, "Number of relations.";
        0x04 capacity: i32, Read, "si_relAddRelation grows the array by one entry when nb equals it.";
        0x08 cur: i32, Read, "A cursor: SI_elemdictGetCurRelationIndex and SI_elemdictSetCurRelationIndex.";
        0x0c rels: Ptr32<ARelation>, Read, "Array of relations, kept ordered by rank.";
        0x10 unk_10: i32, Unknown, "Cleared with the list; no other access found.";
        0x14 flag: i16, Unknown, "1 whenever the list is reset.";
        0x16 pad_16: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// One relation, as seen from one of its two elements (`SI_ARelation`).
    pub struct ARelation = "SI_ARelation", size 0x20,
        "si_relAddRelation allocates 0x20 bytes for the first and grows by 0x20" {
        0x00 relid: i32, Read, "Relation id, a key of the relation dictionary.";
        0x04 elemid: i32, Read, "Index of the element at the other end.";
        0x08 active_elem: i32, Read, "-1, or the index of an element of the ANIMATION chapter that belongs to the relation and is deleted with it.";
        0x0c serial: i32, Named, "Tenth argument of si_relAddRelation, -1 from the plain entry points; the 'serial relation' routines match on it.";
        0x10 chapid: i16, Read, "Chapter of the element at the other end.";
        0x12 group: i16, Read, "Index of the list, on the other element, that holds the matching entry.";
        0x14 flag: i16, Unknown, "Last argument of SI_elemdictAddCompleteRelation (1 from SI_elemdictAddRelation).";
        0x16 unk_16: i16, Unknown, "0 at creation; tested by si_relAddRelation when a serial id is given.";
        0x18 data: Ptr32<Opaque>, Read, "Relation data, on the source side only; disposed by SI_elemdictDisposeRelationData.";
        0x1c rank: i32, Read, "Rank of the relation's rule; si_relAddRelation inserts by ascending rank.";
    }
}

layout! {
    /// The dictionary of relation kinds (`SI_RelDict`); one global, `SI_RD`.
    pub struct RelDict = "SI_RelDict", size 0x10,
        "the symbol SI_RD is 0x10 bytes; SI_reldictInit sets four words" {
        0x00 nb: i32, Read, "Number of rules.";
        0x04 unk_04: i32, Unknown, "-1 at creation; no other access found.";
        0x08 rules: Ptr32<RelRule>, Read, "Array of rules; the rules of one relation id are consecutive.";
        0x0c sort: Ptr32<SortTbl>, Read, "Rule indices ordered by relation id, built on first look-up.";
    }
}

layout! {
    /// One rule of a relation kind: which chapters it joins and how. No type name of the
    /// original is attached to this record in any signature; the name is ours.
    pub struct RelRule = "(entry of SI_RelDict)", size 0x50,
        "SI_reldictAddRelation allocates 0x50 bytes for the first and grows by 0x50" {
        0x00 relid: i32, Read, "Relation id.";
        0x04 name: CStr32, Read, "Relation name (SI_reldictGetRelName).";
        0x08 src_chap: i16, Read, "Chapter of the source; printed as chp[a->b] by SI_reldictDump. -10 marks an incomplete relation.";
        0x0a dst_chap: i16, Read, "Chapter of the destination.";
        0x0c src_group: i16, Read, "List index on the source; printed as grp[a->b].";
        0x0e dst_group: i16, Read, "List index on the destination.";
        0x10 rank: i16, Read, "Printed as rk(); SI_reldictGetRelRank.";
        0x12 fanout: i16, Read, "Printed as fo().";
        0x14 dope_disp: i16, Read, "Printed as dd(); reldictGetRelDopeDisp.";
        0x16 upd_attr0: i16, Named, "First value of si_reldictGetRelUpdlattr.";
        0x18 upd_attr1: i16, Named, "Second value of si_reldictGetRelUpdlattr.";
        0x1a active: i16, Read, "Printed as act(); SI_reldictIsRelActive(…, 0); 1 at creation.";
        0x1c active_dep: i16, Read, "Printed as actdep(); SI_reldictIsRelActive(…, 1); 1 at creation.";
        0x1e pad_1e: [u8; 2], Unknown, "No access found.";
        0x20 cus_name: CStr32, Named, "First word copied from the SI_cusRelInfo argument (custom effects), null without one; SI_reldictGetRelIndexFromName compares it as a string.";
        0x24 cus_info: [i32; 6], Named, "The next six words of SI_cusRelInfo; zero without one.";
        0x3c cus_3c: i16, Named, "From SI_cusRelInfo.";
        0x3e max_fanout_dst: i16, Read, "SI_reldictGetMaxFanout(…, non-zero); 1 without custom info.";
        0x40 max_fanout_src: i16, Read, "SI_reldictGetMaxFanout(…, 0); 1 without custom info.";
        0x42 cus_42: i16, Named, "From SI_cusRelInfo.";
        0x44 cus_44: i16, Named, "From SI_cusRelInfo.";
        0x46 pad_46: [u8; 2], Unknown, "No access found.";
        0x48 render: i16, Read, "SI_reldictGetRelRenderFlag.";
        0x4a pad_4a: [u8; 2], Unknown, "No access found.";
        0x4c incmode: i32, Read, "SI_reldictGetRelIncmodeFlag.";
    }
}

layout! {
    /// A sorted index beside an array (`SI_SortTbl`).
    pub struct SortTbl = "SI_SortTbl", size 0xc,
        "allocated with SI_calloc(1, 0xc) in SI_reldictAddRelation and SI_elemlistAddElem" {
        0x00 count: i32, Read, "Number of entries in table.";
        0x04 table: Ptr32<i32>, Read, "Indices into the array, ordered by key; a new slot holds -1.";
        0x08 valid: i32, Read, "0 when the table must be rebuilt (every addition clears it).";
    }
}

layout! {
    /// A list of update nodes (`SI_UpdList`).
    pub struct UpdList = "SI_UpdList", size 0xc,
        "two of them lie 12 bytes before their neighbours in SI_ElemDict; SI_elemdictInit sets three words in each" {
        0x00 nb: i32, Read, "Number of nodes (SI_updlistDispose loops over it).";
        0x04 nodes: Ptr32<UpdElem>, Read, "Array of 0x2c-byte nodes.";
        0x08 dirty: i32, Named, "1 at creation and after every change to the dictionary.";
    }
}

layout! {
    /// For each kind of chapter, its index in a dictionary (`SI_ChapIds`); the globals
    /// `SI_cid` (scene) and `SI_dbcid` (database). The first 33 fields are the SDK's
    /// `SAA_ChapIds`; the last three are not in the SDK header.
    pub struct ChapIds = "SI_ChapIds", size 0x90,
        "the symbols SI_cid and SI_dbcid are 0x90 bytes; SI_elemdictLoadChapters writes words 0 to 35" {
        0x00 ani: i32, Read, "ANIMATION.";
        0x04 cam: i32, Read, "CAMERAS.";
        0x08 cls: i32, Read, "CLUSTERS.";
        0x0c dsc: i32, Read, "SCENES.";
        0x10 img: i32, Read, "PICTURES.";
        0x14 lat: i32, Read, "LATTICES.";
        0x18 lig: i32, Read, "LIGHTS.";
        0x1c mat: i32, Read, "MATERIALS.";
        0x20 model: i32, Read, "MODELS.";
        0x24 nsl: i32, Read, "NSELECTIONS.";
        0x28 pal: i32, Read, "PALETTES.";
        0x2c rim: i32, Read, "RENDER_PICTURES.";
        0x30 shd_mat: i32, Read, "MATERIAL_SHADERS.";
        0x34 shd_t2d: i32, Read, "TEXTURE2D_SHADERS.";
        0x38 shd_t3d: i32, Read, "TEXTURE3D_SHADERS.";
        0x3c shd_vol: i32, Read, "VOLUME_SHADERS.";
        0x40 shd_sha: i32, Read, "SHADOW_SHADERS.";
        0x44 shd_lig: i32, Read, "LIGHT_SHADERS.";
        0x48 shd_cam: i32, Read, "CAMERA_SHADERS.";
        0x4c shd_out: i32, Read, "OUTPUT_SHADERS.";
        0x50 shd_env: i32, Read, "ENVIRON_SHADERS.";
        0x54 shp: i32, Read, "SHAPES.";
        0x58 sts: i32, Read, "SETUP_SOFT.";
        0x5c stc: i32, Read, "SETUP_CHANNELS.";
        0x60 sfk: i32, Read, "SETUP_FLOCK.";
        0x64 sky: i32, Read, "STRUCT_KEYS.";
        0x68 t2d: i32, Read, "TEXTURES2D.";
        0x6c t3d: i32, Read, "TEXTURES3D.";
        0x70 wav: i32, Read, "WAVES.";
        0x74 exp: i32, Read, "EXPRESSIONS.";
        0x78 chn_drv: i32, Read, "CHANNEL_DRIVERS.";
        0x7c out: i32, Read, "OUTSIDE_DB_FILES.";
        0x80 spd: i32, Read, "SPREADSHEET_SETUP.";
        0x84 audio: i32, Read, "AUDIO.";
        0x88 shd_mat_photon: i32, Read, "MATERIAL_PHOTON_SHADERS.";
        0x8c shd_lig_photon: i32, Read, "LIGHT_PHOTON_SHADERS.";
    }
}

layout! {
    /// One entry of the chapter table `SI_chapInfo` (36 entries, 432 bytes).
    pub struct ChapInfo = "(entry of SI_chapInfo)", size 0xc,
        "432 bytes for 36 chapters; SI_edGetSuffixFromChapter indexes it in steps of three words" {
        0x00 name: CStr32, Read, "Chapter name, also the name of its directory in a database.";
        0x04 suffix: CStr32, Read, "File suffix (SI_edGetSuffixFromChapter).";
        0x08 tag: u32, Named, "Four characters, or 0: the tag that starts the chapter's ASCII files (SDSC, SCAM, …) as a big-endian word.";
    }
}

layout! {
    /// Names an element, and optionally a part of it, for the attribute routines
    /// (`SI_ElemIdentity`). The SDK's `SAA_Elem` is the same idea with the dictionary first.
    pub struct ElemIdentity = "SI_ElemIdentity", size 0x10,
        "not established: no allocation with this type was found; 16 bytes are the fields seen through parameters of the type" {
        0x00 chapid: i32, Read, "Chapter index; every attribute routine passes it to SI_elemdictGetElem.";
        0x04 elemid: i32, Read, "Element index.";
        0x08 sub_id: i32, Named, "Read by the control-vertex, fcurve-key and action attribute routines: an index inside the element.";
        0x0c sub_id2: i32, Unknown, "Copied by the spreadsheet routines; meaning not read.";
    }
}

layout! {
    /// A typed value (`SI_AttributeValue`), the SDK's `SAA_AttributeValue`.
    pub struct AttributeValue = "SI_AttributeValue", size 0xc,
        "the SDK structure under the i386 ABI: a type and an 8-byte union; the attribute routines write +0, +4 and +8" {
        0x00 value_type: i32, Read, "Copied from the attribute's value type.";
        0x04 value: u32, Read, "The value (boolean, char, short, int, float bits or string pointer); for an enumerated attribute, the enum type index.";
        0x08 value2: u32, Read, "For an enumerated attribute, the value; with value, the two halves of a double.";
    }
}

layout! {
    /// Path to an attribute in the attribute dictionary, and a component of it
    /// (`SI_AttGrpIndex`).
    pub struct AttGrpIndex = "SI_AttGrpIndex", size 0x14,
        "the fields seen: a short count, shorts up to +0x10, and a word at +0x10; an attribute embeds one in 0x14 bytes" {
        0x00 depth: i16, Read, "Number of path entries (SI_attdictAttributeGetByIndex).";
        0x02 path: [i16; 7], Read, "path[0] is a group of the dictionary, the next ones sub-groups, the last (path[depth-1]) an attribute of the group reached. Seven entries is what fits before value_index.";
        0x10 value_index: i32, Read, "Which component of the attribute; checked against its number of values.";
    }
}

layout! {
    /// The dictionary of attributes (`SI_AttributeDict`); one global, `SI_ATD`.
    pub struct AttributeDict = "SI_AttributeDict", size 0x1c,
        "the symbol SI_ATD is 0x1c bytes" {
        0x00 nb_groups: i32, Read, "Number of top-level groups.";
        0x04 groups: Ptr32<AttribGroup>, Read, "Array of groups.";
        0x08 attributes: Ptr32<Attribute>, Read, "All attributes, in one array.";
        0x0c nb_attributes: i32, Read, "Length of attributes.";
        0x10 fcv_index: Ptr32<Opaque>, Named, "Built by SI_attdictSortAttribFromFcurveIds; freed with the dictionary.";
        0x14 unk_14: i32, Unknown, "0 at creation.";
        0x18 dirty: i16, Named, "1 at creation and after every addition.";
        0x1a pad_1a: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A group of attributes, with sub-groups (`__SI_AttribGroup`).
    pub struct AttribGroup = "__SI_AttribGroup", size 0x28,
        "si_attdictAddAttribGroup allocates 0x28 bytes for the first and grows by 0x28" {
        0x00 name: CStr32, Read, "Group name.";
        0x04 flag: i16, Unknown, "1 at creation, 0 for the first group of the dictionary.";
        0x06 pad_06: [u8; 2], Unknown, "No access found.";
        0x08 rel_type: i32, Named, "Fourth argument of si_attdictAddAttribGroup, declared SI_RelType.";
        0x0c nb_attribs: i32, Read, "Number of attributes of the group.";
        0x10 attribs: Ptr32<i32>, Read, "Their indices in the dictionary's attribute array.";
        0x14 cond: Ptr32<AttribCond>, Read, "Condition under which the group applies (the COND(...) of the resource file).";
        0x18 get_value: Ptr32<Code>, Read, "Function that reads an attribute of this group; SI_dummyAttrGetValue at creation.";
        0x1c set_value: Ptr32<Code>, Read, "Function that writes one; SI_dummyAttrSetValue at creation.";
        0x20 nb_subs: i32, Read, "Number of sub-groups.";
        0x24 subs: Ptr32<AttribGroup>, Read, "Array of sub-groups.";
    }
}

layout! {
    /// One attribute (`SI_Attribute`): a line of the resource file.
    pub struct Attribute = "SI_Attribute", size 0x64,
        "si_attdictAddAttribute allocates 100 bytes for the first and grows by 100" {
        0x00 unk_00: i32, Unknown, "0 at creation.";
        0x04 unk_04: i16, Unknown, "0 at creation.";
        0x06 pad_06: [u8; 2], Unknown, "No access found.";
        0x08 index: AttGrpIndex, Named, "The attribute's own path: the fields are cleared in the pattern of SI_AttGrpIndex, and SI_attdictUpdateIndexes fills them.";
        0x1c id: i32, Read, "The id column of the resource line; 20000 when the line gives xx.";
        0x20 name: CStr32, Read, "Attribute name.";
        0x24 description: CStr32, Named, "The quoted text ending the resource line.";
        0x28 access: i16, Read, "From the mode column: bit 1 (value 2) set = writable (the Set routines refuse otherwise); value 4 = animatable (SI_attdictAttributeIsAnimatable).";
        0x2a scope: i16, Named, "From the scope column.";
        0x2c value_type: i32, Read, "Copied into every SI_AttributeValue produced for the attribute; 1 at creation.";
        0x30 enum_type: i16, Read, "For an enumerated attribute, the index of its enum type.";
        0x32 pad_32: [u8; 2], Unknown, "No access found.";
        0x34 range_low: [u32; 2], Named, "Lower bound from the range column (reldictConvertRangeToken); 0 at creation.";
        0x3c range_low_flag: i16, Named, "Belongs to the lower bound; 1 at creation.";
        0x3e pad_3e: [u8; 2], Unknown, "No access found.";
        0x40 range_high: [u32; 2], Named, "Upper bound from the range column; 0 at creation.";
        0x48 range_high_flag: i16, Named, "Belongs to the upper bound; 1 at creation.";
        0x4a pad_4a: [u8; 2], Unknown, "No access found.";
        0x4c group: i16, Named, "From the group column; 5 by default.";
        0x4e rank: i16, Named, "From the rank column; 50 by default.";
        0x50 incmode: i32, Named, "From the inc column; 3 by default.";
        0x54 visible: i16, Named, "From the vis column.";
        0x56 nb_values: i16, Read, "Number of components (the # column); length of the two arrays below.";
        0x58 fcv_ids: Ptr32<i16>, Read, "Parameter id of each component; -1 at creation.";
        0x5c fcv_names: Ptr32<CStr32>, Read, "Short name of each component.";
        0x60 unk_60: i16, Unknown, "0 at creation.";
        0x62 pad_62: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// The dictionary of enumerated types (`SI_EnumValDict`); one global, `SI_EVD`.
    pub struct EnumValDict = "SI_EnumValDict", size 0x8,
        "the symbol SI_EVD is 8 bytes" {
        0x00 nb_types: i32, Read, "Number of types.";
        0x04 types: Ptr32<EnumValType>, Read, "Array of types.";
    }
}

layout! {
    /// One enumerated type (`SI_EnumValType`).
    pub struct EnumValType = "SI_EnumValType", size 0xc,
        "si_enmdictAddEnumType allocates 0xc bytes for the first and grows by 0xc" {
        0x00 nb_values: i32, Read, "Number of values.";
        0x04 values: Ptr32<EnumVal>, Read, "Array of values.";
        0x08 name: CStr32, Read, "Type name.";
    }
}

layout! {
    /// One value of an enumerated type. No type name of the original is attached to it.
    pub struct EnumVal = "(entry of SI_EnumValType)", size 0xc,
        "si_enmdictAddValue allocates 0xc bytes for the first and grows by 0xc" {
        0x00 id: i32, Read, "The number.";
        0x04 code: CStr32, Read, "The short code (SI_enmdictEnumCodeGetByValue returns it).";
        0x08 name: CStr32, Read, "The long name (SI_enmdictEnumValGetNameById returns it).";
    }
}

/// Address of the chapter table in the binary.
pub const SI_CHAP_INFO: u32 = 0x0868_42c0;
/// Address of the pointer to the array of dictionaries.
pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
/// Address of the scene's chapter ids.
pub const SI_CID: u32 = 0x0878_1660;

/// The chapter table [soft 0x086842c0 SI_chapInfo]: name, file suffix and ASCII tag of each
/// of the 36 chapters, in the order `SI_elemdictLoadChapters` adds them to a dictionary, so
/// that an entry's position is also the chapter's index.
pub const CHAPTERS: [(&str, &str, Option<[u8; 4]>); 36] = [
    ("SCENES", "dsc", Some(*b"SDSC")),
    ("ANIMATION", "ani", Some(*b"SANI")),
    ("CAMERAS", "cam", Some(*b"SCAM")),
    ("CLUSTERS", "cls", Some(*b"CLUS")),
    ("PICTURES", "pic", None),
    ("LATTICES", "lat", Some(*b"SLAT")),
    ("LIGHTS", "lig", Some(*b"SLIG")),
    ("MATERIALS", "mtr", Some(*b"SMAT")),
    ("MODELS", "hrc", None),
    ("NSELECTIONS", "", None),
    ("PALETTES", "pal", None),
    ("RENDER_PICTURES", "pic", None),
    ("SETUP_SOFT", "sts", Some(*b"SSET")),
    ("SETUP_CHANNELS", "chn", Some(*b"SCHN")),
    ("SETUP_FLOCK", "flk", Some(*b"SFLK")),
    ("SHAPES", "shp", None),
    ("STRUCT_KEYS", "key", Some(*b"SKEY")),
    ("TEXTURES2D", "txt", Some(*b"ST2D")),
    ("TEXTURES3D", "t3D", Some(*b"ST3D")),
    ("WAVES", "wav", Some(*b"SWAV")),
    ("EXPRESSIONS", "exp", None),
    ("MATERIAL_SHADERS", "shd", Some(*b"SHAT")),
    ("SHADOW_SHADERS", "shd", Some(*b"SHAD")),
    ("VOLUME_SHADERS", "shd", Some(*b"SHVL")),
    ("TEXTURE2D_SHADERS", "shd", Some(*b"SHT2")),
    ("TEXTURE3D_SHADERS", "shd", Some(*b"SHT3")),
    ("LIGHT_SHADERS", "shd", Some(*b"SHIG")),
    ("CAMERA_SHADERS", "shd", Some(*b"SHAM")),
    ("OUTPUT_SHADERS", "shd", Some(*b"SHUT")),
    ("ENVIRON_SHADERS", "shd", Some(*b"SHNV")),
    ("CHANNEL_DRIVERS", "", None),
    ("OUTSIDE_DB_FILES", "all", None),
    ("SPREADSHEET_SETUP", "spd", None),
    ("AUDIO", "aiff", None),
    ("MATERIAL_PHOTON_SHADERS", "shd", Some(*b"SHAP")),
    ("LIGHT_PHOTON_SHADERS", "shd", Some(*b"SHIP")),
];

/// For each chapter of [`CHAPTERS`], the word of [`ChapIds`] that receives its index
/// [soft 0x08133cf0 SI_elemdictLoadChapters].
pub const CHAPTER_ID_SLOT: [usize; 36] = [
    3, 0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 0x16, 0x17, 0x18, 0x15, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x0c, 0x10,
    0x0f, 0x0d, 0x0e, 0x11, 0x12, 0x13, 0x14, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23,
];

/// Values of an element's `status`.
pub const ELEM_VALID: i16 = 0;
pub const ELEM_DELETED: i16 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_slots_are_a_permutation() {
        let mut seen = [false; 36];
        for s in CHAPTER_ID_SLOT {
            assert!(!seen[s]);
            seen[s] = true;
        }
        // MODELS is chapter 8 and lands in the word the SDK calls ChapMod
        assert_eq!(CHAPTERS[8].0, "MODELS");
        assert_eq!(CHAPTER_ID_SLOT[8] * 4, core::mem::offset_of!(ChapIds, model));
        assert_eq!(CHAPTER_ID_SLOT[0] * 4, core::mem::offset_of!(ChapIds, dsc));
        assert_eq!(CHAPTER_ID_SLOT[33] * 4, core::mem::offset_of!(ChapIds, audio));
    }

    #[test]
    fn the_work_flag_low_byte_routines_address_byte_0x5b() {
        assert_eq!(core::mem::offset_of!(Elem, work_flag) + 3, 0x5b);
    }
}
