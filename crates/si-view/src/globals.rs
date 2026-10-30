// SPDX-License-Identifier: MIT
//! The global variables of the graph list's source files, at their addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py`.

use crate::records::ProcData;
use si_mem::Ptr32;
use si_types::scene::{GrafList, GrafNode};

si_mem::globals! {
    /// The array of graph lists, 0x24 bytes each (`SI_grafListAdd` grows it by one).
    SI_GRAF_LISTS: Ptr32<GrafList> = 0x08679498, "SI_grafLists", init 0;
    /// How many lists the array has.
    SI_NB_GRAF_LISTS: i32 = 0x08679494, "SI_nbGrafLists", init 0;
    /// One 16-bit flag per list: 1 while the list exists.
    GRAF_LIST_EXISTS: Ptr32<i16> = 0x086bae64, "grafListExists", init 0;
    /// Set while the schematic view must not be laid out again (`SI_schematicNoRefresh`).
    SCHEM_NO_REFRESH: i16 = 0x086bae60, "schemNoRefresh", init 0;
    /// The opposite of it, for the schematic view's own code.
    SI_SCHEM_SLIDE_ITEM: i16 = 0x086bb424, "SI_schemSlideItem", init 1;
    /// The per-process record: current dictionary, current graph list, camera.
    SI_PROC_DATA: Ptr32<ProcData> = 0x08679314, "SI_procData", init 0x08782108;
    /// A write mask of the wire-frame planes; with `SI_color1` the default wire colour.
    SI_SELECT7WM: u16 = 0x08781638, "SI_select7WM";
    /// A colour index; see `SI_select7WM`.
    SI_COLOR1: u16 = 0x08781354, "SI_color1";
    /// The node `SI_selectGrafnode` selected last while no second one was selected.
    LAST_SELECTED: Ptr32<GrafNode> = 0x086bae68, "lastSelected", init 0;
    /// Set once a second node was selected since `SI_selectedString` cleared it.
    MULTIPLE_SELECTED: i16 = 0x086bae6c, "multipleSelected", init 0;
    /// The squared distance in pixels within which a point counts as hit.
    HIT_SIZE: i32 = 0x086bacc0, "hitSize", init 0x32;
    /// The scene's graph list as the Motion module keeps it.
    SI_SCNGL: Ptr32<GrafList> = 0x08695e9c, "SI_scngl", init 0;
    /// The array of element dictionaries, 0x70 bytes each (`si-dict` owns it; the graph list
    /// hands the current one's address to the editing layer).
    SI_ELEM_DICTS: Ptr32<si_types::dict::ElemDict> = 0x0867931c, "SI_elemDicts", init 0;
    /// The scene dictionary's chapter indices, one word per chapter of the chapter table.
    SI_CID: [i32; 36] = 0x08781660, "SI_cid";
    /// The manipulation globals (60 bytes). The graph list reads the words at
    /// [`GLB_MANIP_1C`] and [`GLB_MANIP_32`] and keeps a node at [`GLB_MANIP_18`].
    SI_GLB_MANIP: [u8; 0x3c] = 0x08781560, "SI_glbManip";
    /// The schematic view's globals (80 bytes). The graph list reads the layout direction
    /// in its first word and the words at [`GLB_SCHEM_3C`], [`GLB_SCHEM_3E`], [`GLB_SCHEM_48`].
    SI_GLB_SCHEM: [u8; 0x50] = 0x087fb960, "SI_glbSchem";
    /// Whether a new element's name must be made unique (handed to the editing layer).
    SI_UNIQUE_NAME_FLAG: i16 = 0x08679448, "SI_uniqueNameFlag", init 1;
    /// Set while an old scene is converted on loading (24 bytes; the graph list reads the
    /// first 16-bit word).
    SI_ED_SUMATRA_TO_3X: [u8; 24] = 0x08680e50, "SI_edSumatraTo3x";
    /// Set while an undo or redo is carried out.
    SI_HISTORY_ACTIVE: i16 = 0x0867965c, "SI_historyActive", init 0;
    /// Set while the builder of a loaded scene's list completes a hierarchy's elements
    /// (`SI_graflistBuildFromElemDict` sets it to 1 around `elemdictCompleteHierarchy`).
    SI_ADD_IN_DICTIONARY: i16 = 0x086792b0, "SI_addInDictionary", init 1;
    /// Whether a node's transformation is composed with its father's when it is cut loose
    /// (`SI_graflistDeleteNode`).
    SI_HRCH_CPST: i16 = 0x0867949c, "SI_hrchCpst", init 1;
    /// The default control record (0x600 bytes). Emptying a scene frees the two blocks at
    /// [`DEFAULT_CONTROL_3EC`] and [`DEFAULT_CONTROL_3F0`] and clears five of its words.
    SI_DEFAULT_CONTROL: [u8; 0x600] = 0x08781800, "SI_defaultControl";
    /// The status line's text as `SI_selectedString` builds it: a static of that routine,
    /// 0x400 bytes.
    FINAL_STRING: [u8; 0x400] = 0x087fb0e0, "finalString.484";
    /// How many named selections were made (`SI_nselectionCreate` counts them up) …
    NSELCOUNT: i32 = 0x086bb520, "nselcount", init 0;
    /// … and how many tags.
    NTAGCOUNT: i32 = 0x086bb524, "ntagcount", init 0;
    /// Whether selecting a named selection first empties the list of selected curves.
    SI_CLEAR_SELECTED_FCV: i16 = 0x08695e96, "SI_clearSelectedFcv", init 1;
    /// The relation dictionary (`si-dict` owns it; its address is handed on).
    SI_RD: si_types::dict::RelDict = 0x087815f0, "SI_RD";
    /// The dictionary `schemCompareElems` reads while a layout sorts the sons of an element …
    SCHEM_SORT_PED: Ptr32<si_types::dict::ElemDict> = 0x087fb4e0, "schem_sort_ped";
    /// … and the relation list whose entries are being sorted.
    SCHEM_SORT_PRL: Ptr32<si_types::dict::RelList> = 0x087fb4e4, "schem_sort_prl";
    /// Set in the stand-alone renderer, which has no views.
    SI_STANDALONE_RENDERER: i16 = 0x08679400, "SI_standaloneRenderer", init 0;
}

/// Offsets in `SI_defaultControl` of two words `SI_sceneDeleteAllNotInHistory` clears …
pub const DEFAULT_CONTROL_3E4: u32 = 0x3e4;
pub const DEFAULT_CONTROL_3E8: u32 = 0x3e8;
/// … of two heap blocks it frees …
pub const DEFAULT_CONTROL_3EC: u32 = 0x3ec;
pub const DEFAULT_CONTROL_3F0: u32 = 0x3f0;
/// … and of a fifth word it clears.
pub const DEFAULT_CONTROL_44C: u32 = 0x44c;

/// Offset in `SI_glbSchem` of a 16-bit word that, when set, keeps new nodes from getting
/// the schematic view's default colour.
pub const GLB_SCHEM_3C: u32 = 0x3c;
/// Offset of the 16-bit word that must be set for that colour to be given.
pub const GLB_SCHEM_3E: u32 = 0x3e;
/// Offset of the 16-bit word that says a schematic view is laid out (its elements are
/// moved when a hierarchy changes).
pub const GLB_SCHEM_48: u32 = 0x48;

/// Offset in `SI_glbManip` of a 32-bit mode word that `SI_graflistDeleteGrafnode` turns
/// from 3 to 2 when the node at [`GLB_MANIP_18`] is deleted.
pub const GLB_MANIP_08: u32 = 0x08;
/// Offset of a second mode word (3 becomes 4 then) …
pub const GLB_MANIP_0C: u32 = 0x0c;
/// … and of the word that follows it into 4.
pub const GLB_MANIP_14: u32 = 0x14;
/// Offset in `SI_glbManip` of a graph node the manipulation code refers to (cleared when
/// that node is deleted).
pub const GLB_MANIP_18: u32 = 0x18;
/// Offset of the 16-bit switch under which, in multiple selection, a click selects plain
/// nodes and never branches.
pub const GLB_MANIP_1C: u32 = 0x1c;
/// Offset of the 16-bit switch under which `SI_nselectionSelectContent` lets
/// `SI_graflistCheckMulti` choose the selection mode afterwards.
pub const GLB_MANIP_30: u32 = 0x30;
/// Offset of the 16-bit switch under which selecting a node with flag 0x0800 calls
/// `SI_setUItoPrefFlags (0x08424f98)`.
pub const GLB_MANIP_32: u32 = 0x32;

/// Rows of `SI_cid` the graph list reads. What chapter each is, is read from the routines
/// the value is handed to; the names are ours.
pub mod cid {
    /// 0x08781678: the chapter of the lights (`SI_edGetElemFromLight` answers its elements).
    pub const LIGHTS: usize = 6;
    /// 0x08781680: the chapter of the model nodes.
    pub const MODELS: usize = 8;
    /// 0x08781684: the chapter whose elements stand for several others through their first
    /// forward relation list (named selections: graph nodes of kind 0xc).
    pub const GROUPS: usize = 9;
    /// The other rows, by the chapter table's order (finding 0026; `si_types::dict::ChapIds`).
    pub const ANIMATION: usize = 0;
    pub const CAMERAS: usize = 1;
    pub const CLUSTERS: usize = 2;
    pub const LATTICES: usize = 5;
    pub const SHAPES: usize = 21;
    pub const SCENES: usize = 3;
    pub const MATERIALS: usize = 7;
    pub const NSELECTIONS: usize = 9;
    pub const MATERIAL_SHADERS: usize = 12;
    pub const TEXTURE2D_SHADERS: usize = 13;
    pub const TEXTURE3D_SHADERS: usize = 14;
    pub const VOLUME_SHADERS: usize = 15;
    pub const SHADOW_SHADERS: usize = 16;
    pub const LIGHT_SHADERS: usize = 17;
    pub const CAMERA_SHADERS: usize = 18;
    pub const OUTPUT_SHADERS: usize = 19;
    pub const STRUCT_KEYS: usize = 25;
    pub const TEXTURES2D: usize = 26;
    pub const TEXTURES3D: usize = 27;
    pub const WAVES: usize = 28;
    pub const EXPRESSIONS: usize = 29;
    pub const MATERIAL_PHOTON_SHADERS: usize = 34;
    pub const LIGHT_PHOTON_SHADERS: usize = 35;
}

/// The address of the current element dictionary: `SI_elemDicts` + 0x70 × the process
/// record's dictionary index (the original computes it in every routine that needs it).
pub fn current_dict(m: &si_mem::Mem) -> u32 {
    let proc = m[SI_PROC_DATA];
    m[SI_ELEM_DICTS].addr().wrapping_add((m[proc].dict as u32).wrapping_mul(0x70))
}
