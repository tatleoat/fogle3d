// SPDX-License-Identifier: MIT
//! The global variables scene loading and saving read and write, at their addresses in the
//! binary, beside the ones `si-dict` declares ([`si_dict::globals`]).
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-scene`.

use si_mem::Ptr32;

si_mem::globals! {
    /// Cleared while `SI_dbLoadElementsFromDict` runs and set again when it ends.
    SI_ADD_IN_DICTIONARY: i16 = 0x086792b0, "SI_addInDictionary", init 1;
    /// The program runs as the batch renderer: no dialogue, no window, no schematic view.
    SI_STANDALONE_RENDERER: i16 = 0x08679400, "SI_standaloneRenderer", init 0;
    /// A model read from a file gets its garbage-matte flag set.
    SI_DEFAULT_GMATTE: i16 = 0x08682f60, "SI_defaultGMatte", init 0;
    /// The schematic view's globals; the loader reads and writes three 16-bit words of
    /// them ([`GLB_SCHEM_LOADING`], [`GLB_SCHEM_48`], [`GLB_SCHEM_4A`]).
    SI_GLB_SCHEM: [u8; 0x50] = 0x087fb960, "SI_glbSchem";
    /// The animation globals; the loader clears the 32-bit word at +0x2c when a load ends.
    SI_GLB_ANIM: [u8; 0x40] = 0x08781480, "SI_glbAnim";
    /// What `SI_procData` points at: the index of the current dictionary, a word the
    /// start-up clears, and the scene's camera.
    SI_PROC_DATA_STORAGE: [u32; 3] = 0x08782108, "SI_procDataStorage";
    /// The buffer `SI_dbFormatFullName` prints a name into before it copies it
    /// (`dbexternal.c`'s own `workStr`; the binary has seventeen statics of that name).
    WORK_STR: [u8; 0x400] = 0x08786a00, "workStr";
    /// Set while a scene of the later program is being converted (the converter is not
    /// ported): `SI_elemdictDisposeRelationData` then leaves an expression's data alone.
    /// Tested as 16 bits.
    SI_ED_SUMATRA_TO_3X: i16 = 0x08680e50, "SI_edSumatraTo3x", init 0;
    /// The setup's `SAVE_MAT_TXT_NOT_SEPARATELY` (`SI_elemdictSetSaveOptions`): materials and
    /// textures of one model only are saved inside the model's file
    /// (`si_elemCheckRestrictedSave`, `crate::dictout`). The save's adapter copies the value
    /// the scene keeps (`SaveState::no_mat_txt`) here before it asks.
    SI_SAVE_OPT_NO_MAT_TXT: i16 = 0x08684062, "si_saveOptNoMatTxt", init 0;
    /// The path the file search of the database layer tried last, without suffix …
    SI_DB_WORK_FNAME: [u8; 0x400] = 0x08788140, "SI_dBWorkFname";
    /// … and with it: what "Couldn't read file" shows.
    SI_DB_WORK_FNAME_EXT: [u8; 0x400] = 0x08788540, "SI_dBWorkFnameExt";
    /// `first_call` of `tt_noise.c`: 1 in the binary, until `SI_ttnInitNoise` has filled
    /// the noise table, which the first 3D texture a load reads does (`SI_updTxt3dTurb`,
    /// `si-render`'s port, through the texture readers).
    TT_NOISE_FIRST_CALL: i16 = 0x086b9a80, "first_call.225", init 1;
    /// `refreshObj` (`motionUpdate.c`): 1 in the binary, changed only by playback
    /// (`SI_sceneStepFrame`); while it is set, `SI_sceneUpdate` updates the scene, as the
    /// load's `SI_sceneRefresh` does (finding 0018, section 19).
    REFRESH_OBJ: i32 = 0x086976a4, "refreshObj", init 1;
    /// `SI_rightHand`: the coordinate system's handedness, 1 in the binary; a setup file's
    /// `PREF_RIGHT_HANDED_SURFACE` sets it (`SI_setPrefFlag` case 6). The renderer turns
    /// every normal round when it is 0.
    SI_RIGHT_HAND: i16 = 0x086793ec, "SI_rightHand", init 1;
    /// The play control (`SI_dbReadPlayControlInfo` reads a description's `PLAYCTRL` block
    /// into them): the range, the current frame, the step, the pause, the frames per
    /// second, and whether the step is adaptive (a step of 0). Zero in the binary
    /// (`.bss`); the start-up's play dialogue sets them in the interactive program.
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    SI_FRAME_END: i32 = 0x08782044, "SI_frameEnd";
    SI_FRAME_PAUSE: i32 = 0x08782048, "SI_framePause";
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    SI_FRAME_START: i32 = 0x08782050, "SI_frameStart";
    SI_FRAME_STEP: i32 = 0x08782054, "SI_frameStep";
    SI_STEP_ADAPTIVE: i16 = 0x0878fb28, "SI_stepAdaptive";
}

/// Offset in `SI_glbSchem` of the 16-bit word "a scene is being loaded": `SI_dbLoadScene`
/// sets it, `SI_dbnameCheckValidity` reads it (names keep their odd characters while it is
/// set), the description reader warns about newer files under it.
pub const GLB_SCHEM_LOADING: u32 = 0x3c;
/// Offset of a 16-bit word `SI_dbLoadScene` clears for the time of the load …
pub const GLB_SCHEM_48: u32 = 0x48;
/// … and of the word it is restored from at the end.
pub const GLB_SCHEM_4A: u32 = 0x4a;

/// Offset of the 16-bit word that is 1 while `SI_dbLoadElementsFromDict` runs (finding
/// 0006: the schematic's `SCHEM_UPDATE`).
pub const GLB_SCHEM_4E: u32 = 0x4e;

/// The address of the "a scene is being loaded" word.
pub const LOADING: Ptr32<i16> = Ptr32::new(0x087fb960 + GLB_SCHEM_LOADING);

/// The three literals `SI_dbListNodeAllocate` copies into a new name record: the empty
/// string, `1` and `0` (they lie in `.data`, one after the other).
pub const S_NODE_EMPTY: u32 = 0x0868294d;
pub const S_NODE_ONE: u32 = 0x0868294e;
pub const S_NODE_ZERO: u32 = 0x08682950;
/// The string `???` that `SI_edGetSuffixFromChapter` answers for a chapter the table lacks.
pub const S_NO_SUFFIX: u32 = 0x086827d0;
/// The stem `t2d` of the name `SI_txt2dTransfer` gives a texture that has none
/// (`crate::matconv::txt2d_transfer`; in `.data`).
pub const S_T2D: u32 = 0x086a4109;
/// The stem `t2d` of the name `SI_edAddTexture2D` gives a texture that has none
/// (`crate::edtools::ed_add_texture2d`; in `.data`, its own copy of the literal).
pub const S_T2D_ED: u32 = 0x08680bc8;
/// The stem `t3d` of the name `SI_txt3dTransfer` makes for a texture that has none
/// (`crate::matconv::txt3d_transfer`; in `.data`).
pub const S_T3D: u32 = 0x086a4dd9;

/// Writes the initial values of this crate's globals and the literals its routines read by
/// address.
pub fn store(m: &mut si_mem::Mem) {
    si_mem::init_globals(m, ALL);
    m.set_bytes(S_NODE_EMPTY, b"\0");
    m.set_bytes(S_NODE_ONE, b"1\0");
    m.set_bytes(S_NODE_ZERO, b"0\0");
    m.set_bytes(S_NO_SUFFIX, b"???\0");
    m.set_bytes(S_T2D, b"t2d\0");
    m.set_bytes(S_T2D_ED, b"t2d\0");
    m.set_bytes(S_T3D, b"t3d\0");
}
