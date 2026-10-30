// SPDX-License-Identifier: MIT
//! The outside world of `si_scene::edfirst`'s routines (vectors of
//! `re/tools/edtools1/emu_gen_edfirst.py`): the dictionary's routines that ran as they are in
//! the emulator, and the recorded stand-ins of the generator's `OUTSIDE`.

use super::{Outside, Recorded, Routine};
use si_mem::{Mem, Ptr32 as P};
use si_scene::edfirst as e;

macro_rules! routines {
    ($($id:ident = $name:literal, $addr:expr;)*) => {
        $(static $id: Routine = Routine { name: $name, addr: $addr };)*
        static STANDINS: &[&Routine] = &[$(&$id),*];
    };
}
routines! {
    R_SORT_DIRTY = "SI_dbSortTblSetDirty", e::SI_DB_SORT_TBL_SET_DIRTY;
    R_CHECK_DUPL = "SI_elemdictCheckDuplElemName", e::SI_ELEMDICT_CHECK_DUPL_ELEM_NAME;
    R_SOFT_NAME = "SI_edSoftElemChangeName", e::SI_ED_SOFT_ELEM_CHANGE_NAME;
    R_CHANGE_NAME = "SI_elemdictChangeName", e::SI_ELEMDICT_CHANGE_NAME;
    R_MODEL_CHANGE_NAME = "SI_modelChangeName", si_scene::edtools::SI_MODEL_CHANGE_NAME;
    R_NODE_FREE = "SI_dbListNodeFree", e::SI_DB_LIST_NODE_FREE;
    R_ICON_FREE = "SI_dbIconFree", e::SI_DB_ICON_FREE;
    R_REMOVE_RELATIONS = "SI_relRemoveRelations", e::SI_REL_REMOVE_RELATIONS;
    R_SORT_ANIM = "SI_elemdictSortAnimElements", e::SI_ELEMDICT_SORT_ANIM_ELEMENTS;
    R_UPDATE_HIER = "SI_elemdictUpdateModelHierarchy", e::SI_ELEMDICT_UPDATE_MODEL_HIERARCHY;
    R_DISPOSE_UDF = "SI_edDisposeUDF", e::SI_ED_DISPOSE_UDF;
    R_ADD_UDF = "SI_edAddUDF", e::SI_ED_ADD_UDF;
    R_REMOVE_MAT_TXT = "edModelRemoveMatTxt", e::ED_MODEL_REMOVE_MAT_TXT;
    R_FIND_GL = "SI_findGlData", e::SI_FIND_GL_DATA;
    R_CONVERT = "SI_dbConvertModelInternals2DB", e::SI_DB_CONVERT_MODEL_INTERNALS2DB;
    R_PROPAGATE = "SI_elemdictPropagatePrfxAndShare", e::SI_ELEMDICT_PROPAGATE_PRFX_AND_SHARE;
    R_CHECK_NAMES = "si_dbCheckChapterDuplicateNames", e::SI_DB_CHECK_CHAPTER_DUPLICATE_NAMES;
    R_MERGE = "si_edMergeAnimFiles", e::SI_ED_MERGE_ANIM_FILES;
    R_SEPARATE = "si_edSeparateAnimFiles", e::SI_ED_SEPARATE_ANIM_FILES;
    R_ACTIVATE = "SI_actionsTmpActivateAll", e::SI_ACTIONS_TMP_ACTIVATE_ALL;
    R_MASTER_KEYS = "SI_actionsAddMasterKeysToHA", e::SI_ACTIONS_ADD_MASTER_KEYS_TO_HA;
    R_ZIP_CLEAR = "SI_patchZipClearIndex", e::SI_PATCH_ZIP_CLEAR_INDEX;
    R_ZIP_SET = "SI_patchZipSetFlag", e::SI_PATCH_ZIP_SET_FLAG;
    R_IK_VALID = "SI_ikValidConstraint", e::SI_IK_VALID_CONSTRAINT;
    R_DEFORMED = "SI_isModelDeformed", e::SI_IS_MODEL_DEFORMED;
    R_DELETE_DEF = "SI_modelDeleteDefinition", e::SI_MODEL_DELETE_DEFINITION;
    R_ANIM_DISPOSE = "SI_edCheckAnimFileDispose", e::SI_ED_CHECK_ANIM_FILE_DISPOSE;
    R_HIER_INFO = "SI_elemdictUpdateHierInfo", si_scene::edtools::SI_ELEMDICT_UPDATE_HIER_INFO;
}

/// Answers a call of this file's routines, or `None`.
pub fn call(rec: &mut Recorded, m: &mut Mem, f: u32, args: &[u32]) -> Option<u32> {
    let a = |k: usize| args.get(k).copied().unwrap_or(0);
    match f {
        e::SI_ELEMDICT_FIND_ELEM_BY_DEL_DATA => return Some(si_dict::elemdict::find_elem_by_del_data(m, P::new(a(0)), a(1) as i32, P::new(a(2))) as u32),
        e::SI_ELEMDICT_FIND_ELEM_BY_DEL_AUX_DATA => return Some(si_dict::elemdict::find_elem_by_del_aux_data(m, P::new(a(0)), a(1) as i32, P::new(a(2))) as u32),
        e::SI_ELEMDICT_REMOVE_RELATION_ID => return Some(si_dict::elemrel::remove_relation_id(m, rec, P::new(a(0)), P::new(a(1)), a(2) as i32, a(3) as i32) as u32),
        e::SI_MODEL_ROOT => {
            // the top of the father chain (+700), as the original's loop
            let mut p = a(0);
            while m.u32(p + 700) != 0 {
                p = m.u32(p + 700);
            }
            return Some(p);
        }
        _ => {}
    }
    let routine = STANDINS.iter().copied().find(|r| r.addr == f)?;
    if !rec.case.calls.iter().any(|c| c.name == routine.name) {
        return None;
    }
    Some(Outside::call(rec, m, routine, args))
}
