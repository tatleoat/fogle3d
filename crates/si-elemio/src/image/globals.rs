// SPDX-License-Identifier: MIT
//! The global and static variables of the readers' source files, at their addresses in the
//! binary, and the texts of the binary's data they use.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-elemio`; the texts and the constants by
//! `re/tools/si-elemio/check_data.py`.
//!
//! Every source file of the readers has a static `token` of its own (finding 0005); a
//! value is read into `si_svalue`, `si_fvalue`, `si_ivalue` or `si_lvalue` and stored from
//! there. A read that fails leaves the variable as it was, so what these hold when a call
//! starts is part of a reader's input.

use si_mem::Mem;

si_mem::globals! {
    /// `bsceneio.c`'s `token`: the camera, light and lattice readers and the position
    /// reader they call.
    TOKEN_BSCENEIO: u16 = 0x08817f34, "token";
    /// `utilio.c`'s `token`: the inner token of a value.
    TOKEN_UTILIO: u16 = 0x08818160, "token";
    /// `fcurveio.c`'s `token`: the curve, key and static value readers.
    TOKEN_FCURVEIO: u16 = 0x08817f3c, "token";
    /// `hierani.c`'s `token` and its own `si_svalue`: the element list of an animation
    /// file.
    TOKEN_HIERANI: u16 = 0x08788120, "token";
    SI_SVALUE_HIERANI: i16 = 0x08788122, "si_svalue";
    /// `physicalio.c`'s `token`: the header of an animation file's item.
    TOKEN_PHYSICALIO: u16 = 0x08785ba4, "token";
    /// `dictin.c`'s `file`: the stream of the animation file being read.
    FILE_DICTIN: u32 = 0x08787000, "file";
    /// A text buffer of 512 bytes; the animation reader has the file's comment put there.
    WORK_STR: [u8; 512] = 0x08786e00, "workStr";
    /// The MODELS chapter's index in the scene dictionary (the word at +0x20 of `SI_cid`,
    /// 0x08781660): the chapter of an element list that names none.
    SI_CID_MODELS: i32 = 0x08781680, "";
    /// The scene's frame rate, as a float's bits.
    SI_FRAME_RATE: u32 = 0x0878204c, "SI_frameRate";
    /// `byteswap.c`'s `tmp`: the byte the swaps keep aside.
    TMP: u8 = 0x08817f38, "tmp";
    /// The float last read, as its bits.
    SI_FVALUE: u32 = 0x08817f40, "si_fvalue";
    /// The 32-bit integer last read.
    SI_IVALUE: i32 = 0x08817f44, "si_ivalue";
    /// The 16-bit integer last read.
    SI_SVALUE: i16 = 0x08817f48, "si_svalue";
    /// The `long` last read.
    SI_LVALUE: u32 = 0x08817f4c, "si_lvalue";
    /// `binary.c`'s `fileVersion`: the version of the model, shape or cluster file read
    /// last, as a float's bits. The user data reader tests it.
    FILE_VERSION_BINARY: u32 = 0x08817f28, "fileVersion";
    /// The version of the file being read, as a float's bits (`SI_dbfileReadBinHeader`
    /// sets it). 4.0 in the binary's data.
    SI_VERSION_INPUT: u32 = 0x086793f0, "SI_versionInput", init 0x40800000;
    /// The version the last file's header gave, as a float's bits (the element readers of
    /// `elemdict.c` hand its address to the header reader).
    IN_VERSION: u32 = 0x08785ba0, "inVersion";
    /// Not 0 while the files come through a pipe; `SI_pipe` is then the stream every
    /// reader reads.
    SI_PIPED_FILES: i16 = 0x086ceba8, "SI_pipedFiles";
    SI_PIPE: u32 = 0x086cebac, "SI_pipe";
    /// A function the header reader calls for a file of a version above 4.0001; null in
    /// the binary's data.
    SCENE_VERSION_CHECK: u32 = 0x086cebc0, "scene_version_check", init 0;
    /// The list (node or branch) the lattice reader gave its first lattice to.
    GLB_BRANCH: i16 = 0x086ce960, "glbBranch", init 0;
    /// The 16-bit word at +0x30 of `SI_glbAnim` (0x08781480): the interpolation a new
    /// curve of an ordinary type starts with.
    SI_GLB_ANIM_30: u16 = 0x087814b0, "";
    /// How many curves carry the "dirty" mark while a custom value callback is installed
    /// (`SI_cusValNumCallback` > 0): of custom curves (type 0x236), and of all others.
    SI_FCURVE_DIRTY: i32 = 0x086e49dc, "SI_fcurveDirty", init 0;
    SI_CUS_FCURVE_DIRTY: i32 = 0x086e49e0, "SI_cusFcurveDirty", init 0;
    SI_CUS_VAL_NUM_CALLBACK: i32 = 0x086e49e8, "SI_cusValNumCallback", init 0;
    /// The list of positions that follow a model, newest first (records of 0x1c bytes:
    /// [`super::off::cnstr`]); the scene loader resolves and frees it.
    SI_CNSTR_LST: u32 = 0x086ceb9c, "si_cnstrLst", init 0;
    /// The array of graph lists, 0x24 bytes each.
    SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists", init 0;
    /// The per-process record; its word at +4 is the scene's graph list's number.
    SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
    /// The identity matrix, sixteen floats: its first word, and the three other words that
    /// are not zero in the binary's data (the diagonal).
    SI_IDENTITE: u32 = 0x086dc160, "SI_identite", init 0x3f800000;
    SI_IDENTITE_11: u32 = 0x086dc174, "", init 0x3f800000;
    SI_IDENTITE_22: u32 = 0x086dc188, "", init 0x3f800000;
    SI_IDENTITE_33: u32 = 0x086dc19c, "", init 0x3f800000;
    /// The word at +0x34 of `SI_fmtAspect` (0x08679540): the float a new camera's member
    /// at +0x58 starts as (1.18).
    SI_FMT_ASPECT_34: u32 = 0x08679574, "", init 0x3f970a3d;
    /// The display's colour layout, which the two old colour mappings read.
    SI_BIT_PLANES: i32 = 0x08781350, "SI_bitPlanes";
    SI_COLOR1: u16 = 0x08781354, "SI_color1";
    SI_FCURVE_COLOR3: u16 = 0x08781460, "SI_fcurveColor3";
    SI_SELECT1WM: u16 = 0x0878162c, "SI_select1WM";
    SI_SELECT7WM: u16 = 0x08781638, "SI_select7WM";
    SI_UNSELECT_WM: u16 = 0x087817c4, "SI_unselectWM";
}

/// The messages of the binary's data that the readers hand to a routine outside the crate by
/// address: their text is the original's own, read from the user's install when [`init`] copies
/// it into the image.
pub const MESSAGES: [u32; 3] = [LIGHT_CANNOT_ALLOCATE, CAMERA_CANNOT_ALLOCATE, LATTICE_CANNOT_ALLOCATE];

/// The texts of the binary's data that the readers hand to a routine outside the crate by
/// address, or copy.
pub const TEXTS: &[(u32, &[u8])] = &[
    (MODE_CAMERA, b"rb"),
    (MODE_LIGHT, b"rb"),
    (MODE_LATTICE, b"rb"),
    (MODE_ANIMATION, b"rb"),
    (DEFAULT_NAME, b"default"),
    (SUN_PLACE, b"Location"),
    (NULL_NAME, b"NULL"),
];

/// The status line's text when the camera reader is handed no camera.
pub const CAMERA_CANNOT_ALLOCATE: u32 = 0x086ceaa0;
/// The status line's text when a lattice cannot be allocated or there is no model.
pub const LATTICE_CANNOT_ALLOCATE: u32 = 0x086ceb20;
/// The mode the three element readers open their file with.
pub const MODE_CAMERA: u32 = 0x08681197;
pub const MODE_LIGHT: u32 = 0x0868119a;
pub const MODE_LATTICE: u32 = 0x086811ce;
pub const MODE_ANIMATION: u32 = 0x08682f62;
/// What the animation reader prints on the standard error when it cannot open its file,
/// in front of the path and a line end (the format at 0x08682f65 is this, `%s` and `\n`).
pub const CANNOT_OPEN: &[u8] = b"ERROR: Cannot open file ";
/// The status line's text when a light cannot be allocated.
pub const LIGHT_CANNOT_ALLOCATE: u32 = 0x086ceae0;
/// The name of a new light.
pub const DEFAULT_NAME: u32 = 0x0869f320;
/// The place name of a new sun.
pub const SUN_PLACE: u32 = 0x086a2f30;
/// The shader name that means "no shader".
pub const NULL_NAME: u32 = 0x086ce56b;

/// What the position reader prints for its inner tokens 2, 4 and 5 (the binary holds the
/// same text three times: 0x086ce980, 0x086cea40, 0x086ce9e0).
pub fn position_message() -> &'static [u8] {
    si_rt::message(0x086c_e980)
}

/// Writes the initial values of this crate's globals and its texts into `.data`: what the
/// binary's file holds there. An image that is to run a reader needs this once.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    for addr in MESSAGES {
        m.put_cstr(addr, si_rt::message(addr));
    }
    for (addr, text) in TEXTS {
        m.put_cstr(*addr, text);
    }
}
