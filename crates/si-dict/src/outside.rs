// SPDX-License-Identifier: MIT
//! The parts of the program the dictionaries call that are not on the image yet.
//!
//! Nothing here reproduces a function of the original. A routine of another source file
//! that a dictionary function calls is named by a [`Routine`] (its name and its address in
//! the Linux binary) and reached through [`Outside::call`] with the original's arguments as
//! 32-bit stack words in C order: the calling convention of `si_mem::Registry`, so that the
//! day the routine exists on the image an implementation of the trait can hand the call to
//! it by address. Each routine has a typed wrapper below, which is what the ported code
//! calls.

use si_mem::Mem;

/// A routine of the original outside the dictionary source files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// Whoever answers for the routines that are not on the image.
pub trait Outside {
    /// `routine(args…)`: what the original would leave in `eax`.
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32;

    /// A call through a function pointer the dictionaries were handed (a callback): the
    /// code address and the arguments. An address of [`ALL`] goes to [`Outside::call`];
    /// any other answers 0.
    fn call_addr(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match ALL.iter().find(|r| r.addr == addr) {
            Some(r) => self.call(m, r, args),
            None => 0,
        }
    }

    /// The C library's `getenv(name)`: the address of the variable's value, or null (the
    /// dictionaries only ever test the answer for null). Not set, by default.
    fn getenv(&mut self, _m: &mut Mem, _name: &[u8]) -> u32 {
        0
    }

    /// What the dictionaries `printf` (their dumps and two error messages).
    fn stdout(&mut self, _text: &[u8]) {}

    /// What the dictionaries `fprintf(stderr, …)`.
    fn stderr(&mut self, _text: &[u8]) {}
}

/// Answers 0 to everything and remembers what was asked.
#[derive(Default)]
pub struct NoOutside {
    pub calls: Vec<(&'static Routine, Vec<u32>)>,
}

impl Outside for NoOutside {
    fn call(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.calls.push((routine, args.to_vec()));
        0
    }
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine the dictionaries reach through [`Outside`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    /// `SI_getLoadOptim (0x080d94b0)`, `dbloadsave.c`: whether `SI_LOAD_OPTIM` was in the
    /// environment (asked once, then remembered).
    SI_GET_LOAD_OPTIM = "SI_getLoadOptim", 0x080d94b0;
    /// `SI_grafnodeDefColor (0x0832bff4)`, `graflist.c`: the default wire colour.
    SI_GRAFNODE_DEF_COLOR = "SI_grafnodeDefColor", 0x0832bff4;
    /// `SI_elemdictDisposeRelationData (0x08111b7c)`: frees the data of a relation entry
    /// by the relation's kind.
    SI_ELEMDICT_DISPOSE_RELATION_DATA = "SI_elemdictDisposeRelationData", 0x08111b7c;
    /// `SI_dbListNodeFree (0x08122450)`, `dbexternal.c`: frees a name record and its strings.
    SI_DB_LIST_NODE_FREE = "SI_dbListNodeFree", 0x08122450;
    /// `SI_dbListNodeAllocate (0x0812250c)`, `dbexternal.c`: a new name record.
    SI_DB_LIST_NODE_ALLOCATE = "SI_dbListNodeAllocate", 0x0812250c;
    /// `SI_dbFormatFullName (0x081223dc)`, `dbexternal.c`: a name record's full name, in a
    /// new block.
    SI_DB_FORMAT_FULL_NAME = "SI_dbFormatFullName", 0x081223dc;
    /// `SI_edAllocMdlRelBackUp (0x08105cb0)`, `edtools.c`: room for the saved relations of
    /// the model elements.
    SI_ED_ALLOC_MDL_REL_BACK_UP = "SI_edAllocMdlRelBackUp", 0x08105cb0;
    /// `SI_edMdlRelBackUp (0x08105d7c)`, `edtools.c`: saves one model element's relations.
    SI_ED_MDL_REL_BACK_UP = "SI_edMdlRelBackUp", 0x08105d7c;
    /// `SI_dbnameCheckValidity (0x08121bdc)`, `dbexternal.c`: makes a name fit for a file
    /// name, in place, and answers it.
    SI_DBNAME_CHECK_VALIDITY = "SI_dbnameCheckValidity", 0x08121bdc;
    /// `SI_edGetChapterFromSuffix (0x08120a7c)`, `dbexternal.c`: the chapter whose files
    /// have this suffix, or a negative number.
    SI_ED_GET_CHAPTER_FROM_SUFFIX = "SI_edGetChapterFromSuffix", 0x08120a7c;
    /// `SI_edGetSuffixFromChapter (0x08120a48)`, `dbexternal.c`: a chapter's file suffix.
    SI_ED_GET_SUFFIX_FROM_CHAPTER = "SI_edGetSuffixFromChapter", 0x08120a48;
    /// `dbfilenameExpand (0x081228c0)`, `dbexternal.c`: splits a file name into prefix,
    /// name, version, substitution and suffix (five buffers of the caller).
    DBFILENAME_EXPAND = "dbfilenameExpand", 0x081228c0;
    /// The C library's `time` (the binary's stub of it).
    TIME = "time", 0x080c4328;
    /// `SI_cusResortUpdList (0x0852dd6c)`, `custom_dso.c`: lets custom effects reorder an
    /// update list.
    SI_CUS_RESORT_UPD_LIST = "SI_cusResortUpdList", 0x0852dd6c;
    /// `SI_dbCheckExtraUnToldRelation (0x08104fb8)`, `edtools.c`: adds or removes the
    /// relations a scene implies without storing them.
    SI_DB_CHECK_EXTRA_UNTOLD_RELATION = "SI_dbCheckExtraUnToldRelation", 0x08104fb8;
    /// `SI_edGetFcvFromRelation (0x080ee6f0)`, `edanim.c`: the curve a relation entry owns.
    SI_ED_GET_FCV_FROM_RELATION = "SI_edGetFcvFromRelation", 0x080ee6f0;
    /// `SI_edSetUneditable (0x081074c0)`, `edtools.c`: marks an element as not editable
    /// (the light product). The check also uses its address as a made-up callback.
    SI_ED_SET_UNEDITABLE = "SI_edSetUneditable", 0x081074c0;
    /// `SI_edCheckForUneditable (0x08107a44)`, `edtools.c`.
    SI_ED_CHECK_FOR_UNEDITABLE = "SI_edCheckForUneditable", 0x08107a44;
    /// `SI_generalAttrGetValue (0x080f6460)`, `attrib.c`: reads an attribute of the group
    /// `General_Element` from an element. One of the 28 reading functions a group of the
    /// attribute dictionary can point at (`si_dict::rodata::ATT_FCTS` has them all); it is
    /// named here because `SI_attdictEvalCond` calls through a group's pointer and the
    /// check stands in for this one.
    SI_GENERAL_ATTR_GET_VALUE = "SI_generalAttrGetValue", 0x080f6460;
    /// `SI_actionsDispose (0x081df310)`, `actions.c`: empties an action list.
    SI_ACTIONS_DISPOSE = "SI_actionsDispose", 0x081df310;
    /// `SI_hieranimInit (0x08138aec)`, `hierani.c`: an empty animation record for a chapter
    /// (ported on the image by `si-elemio`; `hieranim::dispose` calls it by address).
    SI_HIERANIM_INIT = "SI_hieranimInit", 0x08138aec;
    /// `SI_actionsTmpActivateAll (0x081dfa68)`, `actions.c`: turns every action of a list
    /// on or off for a moment.
    SI_ACTIONS_TMP_ACTIVATE_ALL = "SI_actionsTmpActivateAll", 0x081dfa68;
    /// `SI_actionsAddMasterKeysToHA (0x081e73b4)`, `actedit.c`.
    SI_ACTIONS_ADD_MASTER_KEYS_TO_HA = "SI_actionsAddMasterKeysToHA", 0x081e73b4;
    /// `SI_actionsInitPathAnim (0x081dfaec)`, `actions.c`: restores the path data of the
    /// actions after a load.
    SI_ACTIONS_INIT_PATH_ANIM = "SI_actionsInitPathAnim", 0x081dfaec;
}

/// `SI_getLoadOptim (0x080d94b0)`.
pub fn si_get_load_optim(o: &mut dyn Outside, m: &mut Mem) -> i16 {
    o.call(m, &SI_GET_LOAD_OPTIM, &[]) as i16
}

/// `SI_grafnodeDefColor (0x0832bff4)`.
pub fn si_grafnode_def_color(o: &mut dyn Outside, m: &mut Mem) -> i32 {
    o.call(m, &SI_GRAFNODE_DEF_COLOR, &[]) as i32
}

/// `SI_elemdictDisposeRelationData (0x08111b7c)`.
pub fn si_elemdict_dispose_relation_data<T>(o: &mut dyn Outside, m: &mut Mem, rel: si_mem::Ptr32<T>) {
    o.call(m, &SI_ELEMDICT_DISPOSE_RELATION_DATA, &[rel.addr()]);
}

/// `SI_dbListNodeFree (0x08122450)`.
pub fn si_db_list_node_free<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>) {
    o.call(m, &SI_DB_LIST_NODE_FREE, &[node.addr()]);
}

/// `SI_dbListNodeAllocate (0x0812250c)`.
pub fn si_db_list_node_allocate<T>(o: &mut dyn Outside, m: &mut Mem) -> si_mem::Ptr32<T> {
    si_mem::Ptr32::new(o.call(m, &SI_DB_LIST_NODE_ALLOCATE, &[]))
}

/// `SI_dbFormatFullName (0x081223dc)`.
pub fn si_db_format_full_name<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>) -> si_mem::CStr32 {
    si_mem::Ptr32::new(o.call(m, &SI_DB_FORMAT_FULL_NAME, &[node.addr()]))
}

/// `SI_edAllocMdlRelBackUp (0x08105cb0)`.
pub fn si_ed_alloc_mdl_rel_back_up<T>(o: &mut dyn Outside, m: &mut Mem, ed: si_mem::Ptr32<T>) {
    o.call(m, &SI_ED_ALLOC_MDL_REL_BACK_UP, &[ed.addr()]);
}

/// `SI_edMdlRelBackUp (0x08105d7c)`.
pub fn si_ed_mdl_rel_back_up<T, U>(o: &mut dyn Outside, m: &mut Mem, ed: si_mem::Ptr32<T>, elem: si_mem::Ptr32<U>, index: i32) {
    o.call(m, &SI_ED_MDL_REL_BACK_UP, &[ed.addr(), elem.addr(), index as u32]);
}

/// `SI_dbnameCheckValidity (0x08121bdc)`.
pub fn si_dbname_check_validity(o: &mut dyn Outside, m: &mut Mem, name: u32, is_prefix: i16) -> u32 {
    o.call(m, &SI_DBNAME_CHECK_VALIDITY, &[name, is_prefix as i32 as u32])
}

/// `SI_edGetChapterFromSuffix (0x08120a7c)`.
pub fn si_ed_get_chapter_from_suffix<T>(o: &mut dyn Outside, m: &mut Mem, ed: si_mem::Ptr32<T>, suffix: u32) -> i32 {
    o.call(m, &SI_ED_GET_CHAPTER_FROM_SUFFIX, &[ed.addr(), suffix]) as i32
}

/// `SI_edGetSuffixFromChapter (0x08120a48)`.
pub fn si_ed_get_suffix_from_chapter(o: &mut dyn Outside, m: &mut Mem, chap: i32) -> si_mem::CStr32 {
    si_mem::Ptr32::new(o.call(m, &SI_ED_GET_SUFFIX_FROM_CHAPTER, &[chap as u32]))
}

/// `dbfilenameExpand (0x081228c0)`.
pub fn dbfilename_expand(o: &mut dyn Outside, m: &mut Mem, name: u32, parts: [u32; 5]) {
    o.call(m, &DBFILENAME_EXPAND, &[name, parts[0], parts[1], parts[2], parts[3], parts[4]]);
}

/// `time(NULL)`.
pub fn time(o: &mut dyn Outside, m: &mut Mem) -> i32 {
    o.call(m, &TIME, &[0]) as i32
}

/// `SI_cusResortUpdList (0x0852dd6c)`.
pub fn si_cus_resort_upd_list<T>(o: &mut dyn Outside, m: &mut Mem, list: si_mem::Ptr32<T>) {
    o.call(m, &SI_CUS_RESORT_UPD_LIST, &[list.addr()]);
}

/// `SI_dbCheckExtraUnToldRelation (0x08104fb8)`.
pub fn si_db_check_extra_untold_relation<T>(o: &mut dyn Outside, m: &mut Mem, ed: si_mem::Ptr32<T>, a: i32, b: i32, c: i32) {
    o.call(m, &SI_DB_CHECK_EXTRA_UNTOLD_RELATION, &[ed.addr(), a as u32, b as u32, c as u32]);
}

/// `SI_edGetFcvFromRelation (0x080ee6f0)`.
pub fn si_ed_get_fcv_from_relation<T>(o: &mut dyn Outside, m: &mut Mem, rel: si_mem::Ptr32<T>) -> u32 {
    o.call(m, &SI_ED_GET_FCV_FROM_RELATION, &[rel.addr()])
}

/// `SI_edSetUneditable (0x081074c0)`.
pub fn si_ed_set_uneditable<T, U>(o: &mut dyn Outside, m: &mut Mem, ed: si_mem::Ptr32<T>, elem: si_mem::Ptr32<U>) {
    o.call(m, &SI_ED_SET_UNEDITABLE, &[ed.addr(), elem.addr()]);
}

/// `SI_edCheckForUneditable (0x08107a44)`.
pub fn si_ed_check_for_uneditable<T>(o: &mut dyn Outside, m: &mut Mem, elem: si_mem::Ptr32<T>, chap: i32) -> i16 {
    o.call(m, &SI_ED_CHECK_FOR_UNEDITABLE, &[elem.addr(), chap as u32]) as i16
}

/// `SI_actionsDispose (0x081df310)`.
pub fn si_actions_dispose(o: &mut dyn Outside, m: &mut Mem, list: u32) {
    o.call(m, &SI_ACTIONS_DISPOSE, &[list]);
}

/// `SI_hieranimInit (0x08138aec)`.
pub fn si_hieranim_init(o: &mut dyn Outside, m: &mut Mem, anim: u32, chapter: i32) {
    o.call(m, &SI_HIERANIM_INIT, &[anim, chapter as u32]);
}

/// `SI_actionsTmpActivateAll (0x081dfa68)(ed, actions, on)`.
pub fn si_actions_tmp_activate_all(o: &mut dyn Outside, m: &mut Mem, ed: u32, actions: u32, on: i16) {
    o.call(m, &SI_ACTIONS_TMP_ACTIVATE_ALL, &[ed, actions, on as i32 as u32]);
}

/// `SI_actionsAddMasterKeysToHA (0x081e73b4)(ed, elem, flag)`.
pub fn si_actions_add_master_keys_to_ha(o: &mut dyn Outside, m: &mut Mem, ed: u32, elem: u32, flag: i16) {
    o.call(m, &SI_ACTIONS_ADD_MASTER_KEYS_TO_HA, &[ed, elem, flag as i32 as u32]);
}

/// `SI_actionsInitPathAnim (0x081dfaec)(ed, actions)`.
pub fn si_actions_init_path_anim(o: &mut dyn Outside, m: &mut Mem, ed: u32, actions: u32) {
    o.call(m, &SI_ACTIONS_INIT_PATH_ANIM, &[ed, actions]);
}
