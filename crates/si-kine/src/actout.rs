// SPDX-License-Identifier: MIT
//! The callees outside the package that `actor.c`, `character.c` and `cha_libkeys.c`
//! (`actor`, `character`, `cha_libkeys` modules) reach through [`Outside`]: a name and an
//! address in the Linux binary each, the original's arguments as stack words in C order,
//! and a typed wrapper per routine where the answer is used.
//!
//! Nothing here reproduces a function of the original.

use si_mem::Mem;

use crate::outside::{Outside, Routine};

macro_rules! routines {
    ( $( $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine these modules reach through [`Outside`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    // ---- actor.c: the buttons, the menus, the selection, the status line
    SI_BUTTON_OFF = "SI_buttonOff", 0x0837fa74;
    SI_BUTTON_KEEP_HILITE = "SI_buttonKeepHilite", 0x0837fa1c;
    SI_SUPER_MODE_OFF = "SI_superModeOff", 0x0837fae0;
    SI_SCHEM_TURN_OFF_OPN_CLS = "SI_schemTurnOffOpnCls", 0x083508ac;
    SI_ACTOR_STATUS = "SI_actorStatus", 0x081c9364;
    SI_MENU_STATE_SET = "SI_menuStateSet", 0x083a3d34;
    SI_INIT_SHOW_FLAG = "SI_initShowFlag", 0x084176b0;
    SI_SET_SINGLE_SELECTION = "SI_setSingleSelection", 0x08181028;
    SI_SET_MULTI_SELECTION = "SI_setMultiSelection", 0x0818112c;
    SI_NUMERIC_SRT_HANDLE = "SI_numericSrtHandle", 0x083a6b34;
    SI_CUT_SELECTED = "SI_cutSelected", 0x0817bdd0;
    SI_CONTROL_OFF = "SI_controlOff", 0x0837efb0;
    SI_GET_CURRENT_SWIFT_KEY = "SI_getCurrentSwiftKey", 0x08393a18;
    SI_CHANGE_CURRENT_SWIFT_KEY_MODULE = "SI_changeCurrentSwiftKeyModule", 0x08393a28;
    SI_GET_QUICK_DESCRIPTION = "SI_getQuickDescription", 0x083a9608;
    SPRINTF = "sprintf", 0x080c4f58;
    // ---- character.c
    SI_RELDICT_GET_REL_INDEX_FROM_ID = "SI_reldictGetRelIndexFromId", 0x08139f68;
    SI_DOPE_FCV_TABLE_SET = "SI_dopeFcvTableSet", 0x081fa4b0;
    SI_ATTDICT_SET_FCURVE_FLAG = "SI_attdictSetFcurveFlag", 0x0811ff18;
    SI_ELEMDICT_ACTIVE_FCV_OPER = "SI_elemdictActiveFcvOper", 0x0822c8a4;
    SI_DOPE_IS_REL_ACTIVE = "SI_dopeIsRelActive", 0x081fa240;
    SI_DOPE_IS_ELEM_ACTIVE = "SI_dopeIsElemActive", 0x081fa1d0;
    SI_SAVEKEY_MODEL = "SI_savekeyModel", 0x08219fe0;
    SI_FCURVE_CHECK_REMOVE_KEY3 = "SI_fcurveCheckRemoveKey3", 0x082197e4;
    SI_SYMMETRY_GET_ELEM = "SI_symmetryGetElem", 0x082434a0;
    SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    // ---- cha_libkeys.c
    SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM = "SI_elemdictGetElemFromHierAnim", 0x081286e4;
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
    SI_HIERANIM_GET_TYPE = "SI_hieranimGetType", 0x08138de0;
    SI_GRAFLIST_COUNT_SEL_HIERARCHIES = "SI_graflistCountSelHierarchies", 0x0832dc0c;
    SI_GRAFLIST_TYPE_COUNT = "SI_graflistTypeCount", 0x0832dc74;
    SI_ED_CHECK_STRUCTURE_KEY = "SI_edCheckStructureKey", 0x08106238;
    SI_ELEMDICT_READ_ELEMENTS = "SI_elemdictReadElements", 0x081283bc;
    SI_ELEMDICT_EXPAND_ANIMATIONS = "SI_elemdictExpandAnimations", 0x0812867c;
    SI_GRAFLIST_BUILD_FROM_ELEMDICT = "SI_graflistBuildFromElemDict", 0x08109ec8;
    DIALOG_GET_ANIMATION = "dialogGetAnimation", 0x081dda18;
    DIALOG_GET_ACTIONS = "dialogGetActions", 0x081de22c;
    DIALOG_GET_MASTERS = "dialogGetMasters", 0x081de900;
    SI_GET_CONSTRAINT_UPDATE_LIST = "SI_getConstraintUpdateList", 0x081f5bf0;
    SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
    SI_ACTIONS_DISPOSE = "SI_actionsDispose", 0x081df310;
    SI_ED_GET_SELECTED_ELEMENTS = "SI_edGetSelectedElements", 0x0810479c;
    SI_ELEMDICT_SET_ANIM_RELATION = "SI_elemdictSetAnimRelation", 0x08128c3c;
    SI_ELEMDICT_MAP_STRUCT_KEY = "SI_elemdictMapStructKey", 0x08128a9c;
    SI_MODEL_UPDATE_SON_STATE = "SI_modelUpdateSonState", 0x084662f8;
    SI_FRAME_TO_SEC = "SI_frameToSec", 0x08201554;
    SI_SCENE_CONSTR_UPDATE = "SI_sceneConstrUpdate", 0x081f56b4;
    SI_ELEMDICT_SET_NEW_FLAG = "SI_elemdictSetNewFlag", 0x08136c48;
    SI_ELEMDICT_REMOVE_ALL_EMPTY_ANIM = "SI_elemdictRemoveAllEmptyAnim", 0x08137490;
    SI_ELEMDICT_SORT_ANIM_ELEMENTS = "SI_elemdictSortAnimElements", 0x08137980;
    SI_HIERANIM_DISPOSE = "SI_hieranimDispose", 0x08138b5c;
    SI_ELEMDICT_SET_DELETE_ELEM = "SI_elemdictSetDeleteElem", 0x08135ad4;
    SI_ELEMDICT_FIND_ELEM_BY_DATA = "SI_elemdictFindElemByData", 0x08134364;
    SI_FCURVE_REMOVE = "SI_fcurveRemove", 0x081ff0a0;
    SI_SCENE_KEY_EDIT = "SI_sceneKeyEdit", 0x0821d17c;
    SI_REFRESH_ALL = "SI_refreshAll", 0x08325dc8;
    SI_SCENE_REFRESH = "SI_sceneRefresh", 0x0820f470;
}

/// A call whose answer is not used.
pub fn call(o: &mut dyn Outside, m: &mut Mem, r: &'static Routine, args: &[u32]) {
    o.call(m, r, args);
}

/// A call whose answer is `eax`.
pub fn call_eax(o: &mut dyn Outside, m: &mut Mem, r: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, r, args).eax
}

// ---- the string literals the three modules hand on by address (held against the binary by
// re/tools/actor3/check_strings.py)
pub const S_SELECT_FIRST: u32 = 0x0869_44c0;
pub const S_SINGLE_ANIM: u32 = 0x0869_4500;
pub const S_SINGLE_ACTIONS: u32 = 0x0869_47c0;
pub const S_TYPE_MISMATCH_ANIM: u32 = 0x0869_4540;
pub const S_TYPE_MISMATCH_ACTIONS: u32 = 0x0869_4840;
pub const S_NO_MATCH: u32 = 0x0869_4580;
pub const S_MATCHES_FMT: u32 = 0x0869_45c0;
pub const S_NO_ACTIONS: u32 = 0x0869_4800;
pub const S_NO_ACTIVE_RELATION: u32 = 0x0869_4440;
pub const S_SWIFT_KEY_FMT: u32 = 0x0869_2280;

/// (address, text) of every literal above.
pub const STRINGS: &[(u32, &str)] = &[
    (S_SELECT_FIRST, "Select at least one item first..."),
    (S_SINGLE_ANIM, "Select one or more items from a SINGLE hierarchy."),
    (S_SINGLE_ACTIONS, "Select one or more items from a SINGLE hierarchy."),
    (S_TYPE_MISMATCH_ANIM, "Type of animation element does not match selected element"),
    (S_TYPE_MISMATCH_ACTIONS, "Type of animation element does not match selected element"),
    (S_NO_MATCH, "No match with any selected element"),
    (S_MATCHES_FMT, "%d matche(s) out of %d item selected"),
    (S_NO_ACTIONS, "No actions in this animation file."),
    (S_NO_ACTIVE_RELATION, "No active relation. Nothing keyed."),
    (S_SWIFT_KEY_FMT, "Swift key %s not valid in Actor"),
];

/// Stores the literals in the image.
pub fn store(m: &mut Mem) {
    for &(addr, text) in STRINGS {
        if addr >= si_mem::RODATA.0 && addr - si_mem::RODATA.0 < si_mem::RODATA.1 {
            m.put_rom_cstr(addr, text.as_bytes());
        } else {
            m.put_cstr(addr, text.as_bytes());
        }
    }
}
