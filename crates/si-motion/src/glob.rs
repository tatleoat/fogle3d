// SPDX-License-Identifier: MIT
//! Global variables of the original that the ported routines read and write, by the address
//! each has in the Linux `soft` binary (symbol table; `re/out/linux/soft.nm.txt`).
//!
//! Names without a symbol of their own are fields of a named global (`SI_glbAnim` + offset)
//! or statics of a source file; the comment says which and how the meaning was established.

use crate::mem::MemExt;
use si_mem::{Addr, Mem};

/// `SI_procData`: a **pointer** to the current process record. Word 0 of the record is the
/// index of the current element dictionary, word 1 the index of the current graf list,
/// word 2 a pointer to the current camera (finding 0010; the camera word read from
/// `SI_sceneUpdateItems`).
pub const SI_PROC_DATA: Addr = 0x0867_9314;
/// `SI_nbElemDicts`.
pub const SI_NB_ELEM_DICTS: Addr = 0x0867_9318;
/// `SI_elemDicts`: a pointer to the array of dictionaries (0x70 bytes each).
pub const SI_ELEM_DICTS: Addr = 0x0867_931c;
/// `SI_standaloneRenderer`: non-zero in a batch render.
pub const SI_STANDALONE_RENDERER: Addr = 0x0867_9400;
/// `SI_grafLists`: a pointer to the array of graf lists (0x24 bytes each); the first word
/// of a list (+4) is its first graf node (read from `animCameraParametersUpdate`).
pub const SI_GRAF_LISTS: Addr = 0x0867_9498;
/// `SI_viewerUpdOpt`: bit 2 selects the "fast" hierarchy update of `SI_animTransfoUpdate`.
pub const SI_VIEWER_UPD_OPT: Addr = 0x0867_9678;
/// `SI_useHiddenForMatte`.
pub const SI_USE_HIDDEN_FOR_MATTE: Addr = 0x0867_967c;
/// `SI_keyTypePropagMode` (16 bits): while set, every key written gets the flag 0x20
/// (finding 0009).
pub const SI_KEY_TYPE_PROPAG_MODE: Addr = 0x0869_5e8e;
/// `SI_fcurveDirty`, `SI_cusFcurveDirty`: how many curves (of a custom effect: type 566) were
/// changed while `SI_cusValNumCallback` was positive (finding 0009, "Dirty counting").
pub const SI_FCURVE_DIRTY: Addr = 0x086e_49dc;
pub const SI_CUS_FCURVE_DIRTY: Addr = 0x086e_49e0;
/// `SI_cusValNumCallback`.
pub const SI_CUS_VAL_NUM_CALLBACK: Addr = 0x086e_49e8;
/// `fromRendering`, a static of `motionUpdate.c`: 1 while `SI_sceneUpdateItems` was called
/// with a camera (by the renderers).
pub const FROM_RENDERING: Addr = 0x0869_76a0;
/// `refreshObj`, a static of `motionUpdate.c` [soft 0x08221760 si_setRefreshObj].
pub const REFRESH_OBJ: Addr = 0x0869_76a4;
/// `SI_identite`: the identity matrix (writable data).
pub const SI_IDENTITE: Addr = 0x086d_c160;
/// `SI_origin`: a zero vector.
pub const SI_ORIGIN: Addr = 0x086d_c1e0;
/// `SI_xAxis`: the vector (1, 0, 0), writable data.
pub const SI_X_AXIS: Addr = 0x086d_c1ec;
/// `SI_ATD`, the attribute dictionary.
pub const SI_ATD: Addr = 0x0878_1438;
/// `SI_glbAnim`: the animation module's settings (`SI_animationInfoInit`, 0x080d335c, gives
/// the start-up values quoted with each field of [`anim`]).
pub const SI_GLB_ANIM: Addr = 0x0878_1480;
/// `SI_glbManip`: the manipulation settings; its first word is the transformation method
/// (0 the usual one, 1 classical: finding 0015).
pub const SI_GLB_MANIP: Addr = 0x0878_1560;
/// `SI_RD`, the relation dictionary.
pub const SI_RD: Addr = 0x0878_15f0;
/// `SI_cid`: the scene dictionary's chapter ids, 36 words in the order of [`cid`].
pub const SI_CID: Addr = 0x0878_1660;
/// `SI_defaultControl`: the scene's render control record (depth fading, fog …).
pub const SI_DEFAULT_CONTROL: Addr = 0x0878_1800;
/// `SI_frameCurrent`, `SI_frameEnd`, `SI_framePause`, `SI_frameRate` (a `float`),
/// `SI_frameStart`, `SI_frameStep`.
pub const SI_FRAME_CURRENT: Addr = 0x0878_2040;
pub const SI_FRAME_END: Addr = 0x0878_2044;
pub const SI_FRAME_PAUSE: Addr = 0x0878_2048;
pub const SI_FRAME_RATE: Addr = 0x0878_204c;
pub const SI_FRAME_START: Addr = 0x0878_2050;
pub const SI_FRAME_STEP: Addr = 0x0878_2054;
/// A word tested by `SI_checkActiveTime` before it asks the dope sheet whether the element
/// is active (0x0878faa8; no symbol).
pub const DOPE_ACTIVE_CHECK: Addr = 0x0878_faa8;
/// `SI_updateOption`: 0 update everything, 1 selected elements only, 2 a variant of 1 (see
/// `SI_checkActiveTime`).
pub const SI_UPDATE_OPTION: Addr = 0x0878_fb24;
/// `curUpdateList`, a static of `motionUpdate.c` [soft 0x0822b074 si_updListSetCurrent].
pub const CUR_UPDATE_LIST: Addr = 0x0878_fb40;

/// Fields of `SI_glbAnim` (offsets from [`SI_GLB_ANIM`]). Start-up values from
/// `SI_animationInfoInit`.
pub mod anim {
    /// +0x02, 16 bits, 0: when set, `SI_updListUpdate` ends with `SI_sceneKeySetColor`.
    pub const KEY_COLOR: u32 = 0x02;
    /// +0x06, 16 bits: raised around the node loop of an update (by `SI_updListUpdate` when
    /// +0x38 is set, by `SI_sceneUpdateItems` when it is not).
    pub const IN_UPDATE: u32 = 0x06;
    /// +0x08, 16 bits, 0: dynamics simulation on; passed to `SI_sceneUpdateDynamics`.
    pub const DYNAMICS: u32 = 0x08;
    /// +0x10, 32 bits, 0: an interaction mode; 5 suppresses the kinematic state update.
    pub const MODE: u32 = 0x10;
    /// +0x14, 32 bits, 0.
    pub const MODE2: u32 = 0x14;
    /// +0x18, `float`, 0.15: key tolerance in frames (finding 0009).
    pub const KEY_TOLERANCE: u32 = 0x18;
    /// +0x2c, 32 bits, 0: when zero the custom effects are run by the update.
    pub const NO_CUSTOM: u32 = 0x2c;
    /// +0x30, 16 bits, 2: default interpolation of new curves (finding 0009).
    pub const DEFAULT_INTERP: u32 = 0x30;
    /// +0x38, 16 bits: 1 unless the environment variable `SI_NO_ENVELOP_OPTIM` is set.
    pub const ENVELOP_OPTIM: u32 = 0x38;
}

/// Index of each chapter's word in `SI_cid` (finding 0010, "Chapters": the SDK's
/// `SAA_ChapIds` order). `SI_CID + 4 * cid::MODELS` holds the chapter index of MODELS.
pub mod cid {
    pub const ANIMATION: u32 = 0;
    pub const CAMERAS: u32 = 1;
    pub const CLUSTERS: u32 = 2;
    pub const SCENES: u32 = 3;
    pub const PICTURES: u32 = 4;
    pub const LATTICES: u32 = 5;
    pub const LIGHTS: u32 = 6;
    pub const MATERIALS: u32 = 7;
    pub const MODELS: u32 = 8;
    pub const NSELECTIONS: u32 = 9;
    pub const SHAPES: u32 = 21;
    pub const STRUCT_KEYS: u32 = 25;
    pub const TEXTURES2D: u32 = 26;
    pub const TEXTURES3D: u32 = 27;
    pub const WAVES: u32 = 28;
    pub const EXPRESSIONS: u32 = 29;
    pub const CHANNEL_DRIVERS: u32 = 30;
}

/// Chapter indices in a dictionary as `SI_elemdictLoadChapters` lays them out (the row of
/// `SI_chapInfo`; finding 0010), in the order of the words of `SI_cid`.
pub const STANDARD_CID: [i32; 36] = [1, 2, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11, 21, 24, 25, 23, 22, 26, 27, 28, 29, 15, 12, 13, 14,
                                     16, 17, 18, 19, 20, 30, 31, 32, 33, 34, 35];

/// The chapter index of `which` (one of [`cid`]) in the scene dictionary: `SI_cid[which]`.
pub fn chap(m: &Mem, which: u32) -> i32 {
    m.i32(SI_CID + 4 * which)
}

/// The current element dictionary: `SI_elemDicts + SI_procData[0] * 0x70`.
pub fn cur_dict(m: &Mem) -> Addr {
    let pd = m.ptr(SI_PROC_DATA);
    let i = m.i32(pd);
    m.ptr(SI_ELEM_DICTS).wrapping_add((i as u32).wrapping_mul(0x70))
}

/// The current graf list: `SI_grafLists + SI_procData[1] * 0x24`.
pub fn cur_graf_list(m: &Mem) -> Addr {
    let pd = m.ptr(SI_PROC_DATA);
    let i = m.i32(pd + 4);
    m.ptr(SI_GRAF_LISTS).wrapping_add((i as u32).wrapping_mul(0x24))
}

pub fn frame_rate(m: &Mem) -> f32 {
    m.f32(SI_FRAME_RATE)
}

/// Stores what the program has in its data segment once it is up, as far as the ported
/// routines look at it: the standard chapter ids, a process record (16 bytes, placed in the
/// image's arena; its address is returned) selecting dictionary 0 and graf list 0, 30 frames
/// per second (`SI_initPlayControl`), the identity matrix, and the start-up values of
/// `SI_glbAnim` (`SI_animationInfoInit`: tolerance 0.15, Hermite). The dictionary array
/// itself (`SI_elemDicts`) is left null for the caller to build.
///
/// The image is made strict (`Mem::set_strict`): from here on an access in the arena or the
/// heap outside a live block is a recorded fault, which is how the tests of this crate
/// notice an overrun or the use of a freed block.
pub fn init_defaults(m: &mut Mem) -> Addr {
    m.set_strict(true);
    let proc_data = m.place(16, 8);
    m.set_ptr(SI_PROC_DATA, proc_data);
    for (i, c) in STANDARD_CID.iter().enumerate() {
        m.set_i32(SI_CID + 4 * i as u32, *c);
    }
    m.set_f32(SI_FRAME_RATE, 30.0);
    m.set_matrix(SI_IDENTITE, &si_math::types::IDENTITY);
    m.set_f32(SI_X_AXIS, 1.0);
    m.set_f32(SI_GLB_ANIM + anim::KEY_TOLERANCE, 0.15);
    m.set_i16(SI_GLB_ANIM + anim::DEFAULT_INTERP, 2);
    m.set_i16(SI_GLB_ANIM + anim::ENVELOP_OPTIM, 1);
    m.set_i32(REFRESH_OBJ, 1);
    proc_data
}
