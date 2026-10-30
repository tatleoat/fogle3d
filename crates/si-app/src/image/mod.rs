// SPDX-License-Identifier: MIT
//! The batch renderer's start-up on the shared image (finding 0019, section 12): what
//! `soft <resources> -R <scene> [options]` does from `si_runCreative` to the hand-over to
//! Get Scene, and the options it applies to the render set-up after the load, as ported
//! functions on `si-mem`'s image (finding 0024).
//!
//! The rest of this crate keeps the program's globals in one Rust record (`App`, finding
//! 0019, section 10); this module is the part that is in place: every global it reads or
//! writes is at the original's address ([`g`]), every record at the original's layout,
//! every allocation is the original's, through the image's allocator. Calls to what is not
//! on the image go through [`si_mem::Outside`], with the callees declared once in
//! [`calls::BatchCalls`] (name, address, arguments as the original pushes them).
//!
//! - [`globals_c`]: `globals.c`'s five initialisers ([`globals_c::manipulation_init`] and
//!   [`globals_c::animation_info_init`] are on the batch path).
//! - [`batch`]: `sys_main.c`'s batch renderer (`standAloneRender` and the routines it
//!   calls), `SI_callRender`, `SI_replicateCommandLine`, and the start-up's path to it
//!   (the part of `si_runCreative` that runs before `-R`, not cited: it cannot be
//!   emulated, finding 0019, section 9.3).
//! - The entry points: [`batch::start_up`] (everything up to and including the
//!   "Reading scene from Database" line, then the caller loads the scene) and
//!   [`batch::finish`] (the load's outcome, the options on the render set-up, and
//!   `SI_callRender`); [`batch::run`] is both with the load as a call.
//!
//! Licence code is not ported: where the path has a licence check the code says "licence
//! check, not ported" and takes the full product's path.

pub(crate) mod adapter;
pub mod batch;
pub mod calls;
pub mod clusterread;
pub mod fileswap;
pub mod globals_c;
pub mod hardware;
pub mod leftovers;
pub mod setup;
pub mod system;

pub use batch::{finish, run, start_up, HandOver};

/// The globals of the batch path, at their addresses in the Linux binary.
pub mod g {
    si_mem::globals! {
        /// `SI_standaloneRenderer`: the program is the batch renderer.
        SI_STANDALONE_RENDERER: i16 = 0x08679400, "SI_standaloneRenderer";
        /// `programName`: `argv[0]`.
        PROGRAM_NAME: u32 = 0x087821c4, "programName";
        /// `argc` and `argv` of `standAloneRender`: the options still to read and where the
        /// next is (`readSetup` moves both).
        ARGC: i32 = 0x087821c8, "argc";
        ARGV: u32 = 0x087821cc, "argv";
        /// `string`: the buffer `sprintf` composes the start-up script's path in.
        STRING: [u8; 0x400] = 0x087821e0, "string";
        /// `SI_cwd`: `getcwd(NULL, 255)`.
        SI_CWD: u32 = 0x087fba8c, "SI_cwd";
        /// The path globals: copies of the current directory, the picture path of the
        /// output database's `RENDER_PICTURES` chapter.
        SI_OBJECT_PATH: u32 = 0x086793b8, "SI_objectPath";
        SI_TEXTURE_PATH: u32 = 0x08679434, "SI_texturePath";
        SI_PICTURE_PATH: u32 = 0x086793cc, "SI_picturePath";
        SI_MATERIAL_LIB: u32 = 0x08679328, "SI_materialLib";
        SI_TEXTURE_LIB: u32 = 0x08679430, "SI_textureLib";
        SI_PALETTE_LIB: u32 = 0x086793c8, "SI_paletteLib";
        /// The input and output databases (`SI_DbInfo *`: +0x08 name, +0x0c path, +0x14
        /// the path of the chapter set last, +0x18 not 0 when valid).
        SI_PDBIIN: u32 = 0x08782058, "SI_pDbiin";
        SI_PDBIOUT: u32 = 0x0878205c, "SI_pDbiout";
        /// `SI_dbdir`: the list of databases (only its address is passed).
        SI_DBDIR: [u8; 0xc] = 0x08781390, "SI_dbdir";
        /// The graph lists (0x24 bytes each), the scene's list, the per-process record
        /// (+0 the element dictionary's index, +4 the graph list's, +8 the scene's camera),
        /// the element dictionaries (0x70 bytes each).
        SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists";
        SI_SCNGL: u32 = 0x08695e9c, "SI_scngl";
        SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x0878_2108;
        SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts";
        /// `SI_resdir`: a copy of the resource directory.
        SI_RESDIR: u32 = 0x08781628, "SI_resdir";
        /// `globals.c`'s parameter records (finding 0019, section 6.5).
        SI_GLB_MANIP: [u8; 0x3c] = 0x08781560, "SI_glbManip";
        SI_GLB_ANIM: [u8; 0x40] = 0x08781480, "SI_glbAnim";
        SI_GLB_INCREM: [u8; 0x30] = 0x08781520, "SI_glbIncrem";
        SI_GLB_DUPLIC: [u8; 0x58] = 0x087814c0, "SI_glbDuplic";
        SI_GLB_SCHEM: [u8; 0x50] = 0x087fb960, "SI_glbSchem";
        /// The render set-up and the copy a render works on (offsets in [`super::batch::ctl`]).
        SI_DEFAULT_CONTROL: [u8; 0x600] = 0x08781800, "SI_defaultControl";
        SI_REN: [u8; 0x600] = 0x08790be0, "SI_ren";
        /// `-l` was seen, `-v` was seen.
        LINE_OPTION: i16 = 0x08679750, "lineOption";
        VERBOSE_OPTION: i16 = 0x08679752, "verboseOption";
        /// `-f`: report every n scan lines.
        SI_REPORT_FREQUENCY: i16 = 0x0869bc80, "SI_reportFrequency", init 10;
        /// What custom effects are told the program is doing.
        SI_CUS_UPDATE_STATE: i32 = 0x086e49ec, "SI_cusUpdateState", init 2;
        /// `-D` before `-R`: a custom effect run without the program.
        SI_CUS_STANDALONE: i16 = 0x086e49f0, "SI_cusStandalone";
        /// Seven bytes behind `get_init_value`, and the table the start-up copies them
        /// from (finding 0019, section 2.3); eight bytes each with the zero after them.
        INIT_BUF: [u8; 8] = 0x0867970f, "init_buf", init 0x002a_0f41_1e0c_2c10;
        SRV_BUF: [u8; 8] = 0x08679717, "srv_buf", init 0x001e_140f_0f0e_100d;
        /// The command line as one string, for the offer to restart after a crash.
        COMMAND_LINE: [u8; 0x2000] = 0x0880e880, "commandLine";
        /// The names of the keys file and the setup file, and of the colour map.
        SI_USER_PREF_KEYS_RSRC: u32 = 0x0867942c, "SI_userPrefKeysRsrc", init 0x0867_9420;
        SI_SETUP_RSRC: u32 = 0x0867941c, "SI_setupRsrc", init 0x0867_9410;
        DEFAULT_COLOR: u32 = 0x0867973c, "defaultColor", init 0x0867_9732;
        /// What the start-up sets before the options are read.
        SI_USERNAME: u32 = 0x087817c8, "SI_username";
        SI_INIT_POSE_COMPENSATE: i16 = 0x087817a0, "SI_initPoseCompensate";
        /// Product-tier flags (`-B`, `-G`): stored, never branched on.
        SI_BASE_LICENSE: i16 = 0x08781464, "SI_baseLicense";
        SI_LIGHT_LICENSE: i16 = 0x08679320, "SI_lightLicense";
        SI_LIGHT_LICENSE2: i16 = 0x08679322, "SI_lightLicense2";
        /// `-f`, `-c`.
        SI_FOREGROUND: i16 = 0x08781462, "SI_foreground";
        COREDUMP: i16 = 0x08679720, "coredump";
        /// `emergencyDetails` (finding 0019, section 7.3): only the fields the start-up
        /// writes with emergency save off are named in [`super::batch`].
        EMERGENCY_DETAILS: [u8; 0x5040] = 0x08810880, "emergencyDetails";
    }

    /// The C library's `stdout` and `stderr` as the program holds them (copies the dynamic
    /// linker makes in `.bss`; the binary has no symbol of its own for them). The handles
    /// are `si-mem`'s and the harness's (finding 0024, section 4.8).
    pub const STDIN: u32 = 0x08780e90;
    pub const STDIN_HANDLE: u32 = 0x6f00_0000;
    pub const STDOUT: u32 = 0x08780c84;
    pub const STDERR: u32 = 0x08780c88;
    pub const STDOUT_HANDLE: u32 = 0x6f00_0100;
    pub const STDERR_HANDLE: u32 = 0x6f00_0200;
}

/// The texts of `.data` the batch path hands to a callee or copies (each at its address in
/// the binary; `re/tools/si-app/check_image.py` holds them against the binary's bytes).
/// Format strings are not here: the port formats with Rust's own code.
pub const TEXTS: &[(u32, &[u8])] = &[
    // binary_b.c: the user-data node setT2dMapDomainID makes; counter.c: the panel's blank texts
    (0x086ce7dc, b"_UVDomainID_"),
    (0x0867cc44, b"  ..."),
    (0x0867cc4a, b"  ..."),
    (0x0867cc50, b"  ..."),
    (0x0867cc56, b"  ..."),
    (0x0867cc5c, b"  ..."),
    (0x0867cc62, b"  ..."),
    (0x0867cc68, b"  ..."),
    (0x0867cc6e, b"  ..."),
    (0x0867cc74, b"  ..."),
    // system.c: SI_deleteAll's warning
    (0x086d1e60, b"WARNING : The scene has been modified..."),
    (0x086d1e89, b" "),
    (0x086d1e8b, b"Delete anyway ?"),
    (0x086d1ea0, b"You will lose all your work..."),
    // globals.c: the environment variables of SI_manipulationInit and SI_animationInfoInit
    (0x0867cd34, b"SI_NO_INTERACTIVE_FCV_UPD"),
    (0x0867cd4e, b"SI_IK_OLD_JOINT_ROLL"),
    (0x0867cd63, b"SI_MANIPULATION_OPTIM"),
    (0x0867cd79, b"SI_NO_OPTIM_UNTOLD_REL"),
    (0x0867cd90, b"SI_CLASSICAL_SCL_FIX"),
    (0x0867cda5, b"SI_ENV_RND_FST_JNT"),
    (0x0867cdb8, b"SI_NO_ENVELOP_OPTIM"),
    // sys_main.c: si_runCreative
    (0x0867978a, b"SI_INIT_POSE_COMPENSATE"),
    (0x08679822, b"C"),
    (0x08679410, b"Setup.sts"),
    (0x08679420, b"SIKeys.sks"),
    (0x08679732, b"Default"),
    // sys_main.c: standAloneRender
    (0x0867b4c0, b"RENDER_PICTURES"),
    (0x0867b4d0, b"Perspective_def"),
    (0x0867b52f, b"SCENES"),
    (0x0867b707, b"w"),
    (0x0867b709, b"/dev/null"),
    (0x0867b7c2, b"RENDER_PICTURES"),
    (0x0867b948, b"out.mi"),
    // sys_main.c: readSetup, the picture formats (SI_fmtName points at these)
    (0x0867957c, b"35mm 1.37/1"),
    (0x08679588, b"35mm 1.66/1"),
    (0x08679594, b"35mm 1.85/1"),
    (0x086795a0, b"70mm Panavision"),
    (0x086795b0, b"70mm IMAX"),
    (0x086795ba, b"70mm OMNIMAX"),
    (0x086795c7, b"NTSC"),
    (0x086795cc, b"PAL"),
    (0x086795d0, b"HDTV"),
    (0x086795d5, b"Slide 35mm"),
    (0x086795e0, b"6cm X 6cm"),
    (0x086795ea, b"6cm X 9cm"),
    (0x086795f4, b"4 X 5"),
    (0x086795fa, b"Custom aspect"),
    (0x08679608, b"Custom size"),
];

/// Tables of `.data` the set-up questions read: `SI_fmtAspect` (fifteen `float`s, the last
/// two rewritten by `readSetup`) and `SI_fmtName` (fifteen pointers into [`TEXTS`]); held
/// against the binary by `check_image.py`.
/// Constants of the binary's read-only data the code uses as Rust values (the image has
/// nothing to write them into); `check_image.py` holds them against the binary.
pub const RODATA: &[(u32, &[u8])] = &[
    // system.c: the limit mdlZoom clamps the corners it gets back to (a float)
    (0x0865d9a4, &[0xcc, 0xcc, 0x4c, 0x7f]),
];

pub const TABLES: &[(u32, &[u8])] = &[
    (0x08679540, &[0x29, 0x5c, 0xaf, 0x3f, 0x6d, 0x56, 0xd5, 0x3f, 0xcd, 0xcc, 0xec, 0x3f, 0xf6, 0x28, 0x0c, 0x40, 0x8d, 0x97, 0xae, 0x3f, 0x8d, 0x97, 0xae, 0x3f, 0x93, 0xa9, 0xaa, 0x3f, 0x93, 0xa9, 0xaa, 0x3f, 0x3b, 0x8e, 0xe3, 0x3f, 0x00, 0x00, 0xc0, 0x3f, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0xc0, 0x3f, 0x00, 0x00, 0xa0, 0x3f, 0x3d, 0x0a, 0x97, 0x3f, 0x00, 0x00, 0x80, 0x3f]),
    (0x08679620, &[0x7c, 0x95, 0x67, 0x08, 0x88, 0x95, 0x67, 0x08, 0x94, 0x95, 0x67, 0x08, 0xa0, 0x95, 0x67, 0x08, 0xb0, 0x95, 0x67, 0x08, 0xba, 0x95, 0x67, 0x08, 0xc7, 0x95, 0x67, 0x08, 0xcc, 0x95, 0x67, 0x08, 0xd0, 0x95, 0x67, 0x08, 0xd5, 0x95, 0x67, 0x08, 0xe0, 0x95, 0x67, 0x08, 0xea, 0x95, 0x67, 0x08, 0xf4, 0x95, 0x67, 0x08, 0xfa, 0x95, 0x67, 0x08, 0x08, 0x96, 0x67, 0x08]),
];

/// Writes what the binary's `.data` holds for this module: the initial values of its
/// globals and the texts of [`TEXTS`]. A caller that starts an image for the program
/// calls it once, before [`batch::start_up`] (which calls it itself).
pub fn init_data(m: &mut si_mem::Mem) {
    si_mem::init_globals(m, g::ALL);
    for (at, text) in TEXTS {
        m.put_cstr(*at, text);
    }
    for (at, text) in fileswap::TEXTS {
        m.put_cstr(*at, text);
    }
    for (at, bytes) in TABLES {
        for (i, b) in bytes.iter().enumerate() {
            m.set_u8(*at + i as u32, *b);
        }
    }
}
