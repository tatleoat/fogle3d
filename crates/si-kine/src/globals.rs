// SPDX-License-Identifier: MIT
//! The global variables the image-form modules of this crate read or write, at their
//! addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py`.

si_mem::globals! {
    /// The array of graf lists, one of 0x24 bytes per process slot (a pointer; null at
    /// start-up).
    SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists", init 0;
    /// The per-process data: a pointer to a record whose second word is the index of the
    /// current graf list.
    SI_PROC_DATA: si_mem::Ptr32<i32> = 0x08679314, "SI_procData", init 0x08782108;
    /// The default rotation-limit record the set-up dialog edits (`SI_RelRotlimData`).
    ROTLIM_DATA: [u8; 0x44] = 0x08789560, "rotlimData";
    /// The default position-limit record (`SI_RelPoslimData`).
    POSLIM_DATA: [u8; 0x34] = 0x08789520, "poslimData";
    /// The program's scratch text buffer.
    SI_TEMP: [u8; 0x1e] = 0x08780f00, "_SI_temp_";
    /// The animation module's settings, as 16-bit words (the size other crates declare).
    SI_GLB_ANIM: [u16; 0x20] = 0x08781480, "SI_glbAnim";
    /// What the custom effects are told the update is doing: 3 while constraints are run
    /// from a manipulation, 2 otherwise.
    SI_CUS_UPDATE_STATE: i32 = 0x086e49ec, "SI_cusUpdateState", init 2;
    /// The current frame, a whole number.
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    /// Frames a second.
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    /// Not zero when the program runs with its interface (`SI_invKinSetCnsPosition`,
    /// `si_rotlimConstraint`).
    SI_INTERACTIVE: i16 = 0x0867953e, "SI_interactive", init 0;
    /// `SI_invKinIsBreakJnt`'s three statics: the number of curves the element's
    /// expressions drive, the list `SI_getExprFcurveUsed` answered, and its second list.
    NB_FCV_261: i32 = 0x086d72f0, "nb_fcv.261", init 0;
    FCV_262: u32 = 0x086d72f4, "fcv.262", init 0;
    FCV_INFO_263: u32 = 0x086d72f8, "fcvInfo.263", init 0;
    /// The playback step in frames, a whole number.
    SI_FRAME_STEP: i32 = 0x08782054, "SI_frameStep";
}
