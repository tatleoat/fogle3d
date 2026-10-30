// SPDX-License-Identifier: MIT
//! The setup file (`.sts`, tag `SSET`): reader and writer (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `asceneio.c` (reader), `setup.c` (writer),
//! `a_utilio.c` (the token and value readers both share).
//!
//! A setup file is the program's session state as text: which things are shown, the
//! preferences, the four view windows and what each displays, the render set-up and the
//! preview set-up. The original reads it straight into its globals. Here the globals the file
//! touches are the plain structures of [`Setup`], field for field (types as in the original:
//! `i16` where it stores a short, `i32` an int, `f32` a single), and the reader stores into
//! them exactly as the original stores into its memory: a keyword that is absent leaves its
//! field alone, a value the original rejects leaves what the original leaves.
//!
//! What the original consults that is not in the file is passed in instead ([`Session`]):
//! whether it runs as the stand-alone renderer, whether the licence is the "light" one,
//! whether mental ray 1.9 is installed, an environment variable, whether the mouse dialog
//! exists. One thing the writer depends on is not in the file either: the indent level its
//! predecessors left behind ([`WriteOptions::indent`]).
//!
//! Not reproduced, and said where it matters: the audio manager behind `AUDIOCONTAINER`, the
//! spreadsheet module behind `SPREADSHEET_SETUP`, `SI_setCamView` behind the old
//! `MAXIMUM_VIEW`, the menus the reader updates on the side, and the window template file a
//! fresh program takes its cameras from. The corpus checks are in [`check`].
//!
//! The file is not read with the token reader of the other ASCII files. `asceneio.c` has its
//! own, simpler one (`si_inToken`): tokens end at a space, a tab or a newline only, so a `;`
//! and a carriage return are ordinary characters. Only the header goes through
//! [`crate::token::read_asc_header`].

use crate::cnum;
use crate::token::{read_asc_header, AsciiFile};

#[path = "setup_layout.rs"]
mod layout;
#[path = "setup_render.rs"]
mod render;

pub use layout::{
    Camera, Controller, DepthCue, DrawOver, Layout, MemoCamera, ShadeSetup, Slot, Spreadsheet, ViewOptions, Window,
    CAMERA_IDS, SCENE_CAMERA, SLOT_DOPESHEET, SLOT_FCURVE, SLOT_FRONT, SLOT_PERSPECTIVE, SLOT_RIGHT, SLOT_SCHEMATIC,
    SLOT_SPREADSHEET, SLOT_TOP, WINDOW_FRONT, WINDOW_PERSPECTIVE, WINDOW_RIGHT, WINDOW_TOP,
};
pub use render::{Fcurve, FcurveKey, MentalRay, Preview, RenderControl, Udf, FORMAT_NAMES, RENDER_TYPE_NAMES};
pub(crate) use render::{value3_ascii_read, value_ascii_read};

/// The tag of a setup file, as the header writer gets it (0x53534554).
pub const TAG_SSET: u32 = u32::from_be_bytes(*b"SSET");

/// C's `EOF`.
const EOF: i32 = -1;

// ------------------------------------------------------------------------------------------
// what the caller supplies and what it gets back
// ------------------------------------------------------------------------------------------

/// State of the running program that the reader and the writer consult and that no setup
/// file carries.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// `SI_standaloneRenderer` (0x08679400): the program runs without its interface. The
    /// window layout is then skipped by the reader, and show flags and layout are not written
    /// (unless the save is an emergency one).
    pub standalone_renderer: bool,
    /// `SI_emergencySaving()`: with the stand-alone renderer, show flags and layout are
    /// written after all.
    pub emergency_saving: bool,
    /// `SI_lightLicense` (0x08679320): a spreadsheet view is replaced by the schematic.
    pub light_license: bool,
    /// What `SI_isMr19Enable` (0x08580d0c) answers: the environment names a mental ray 1.9
    /// renderer that exists on disk. Without it `MENTAL_RAY` reads as `MENTAL_RAY_2X`.
    pub mental_ray_19: bool,
    /// `getenv("SI_SHADE_OPTIMIZE_EDITING")` is set: `OPTIMISE_EDITING` then reads as 1
    /// whatever the file says.
    pub shade_optimize_editing_env: bool,
    /// The mouse preferences dialog (`SI_dialogGet(0x744)`) exists. Three of the four mouse
    /// values live in its fields; without the dialog they are read and dropped, and the
    /// writer leaves the whole section out.
    pub mouse_dialog: bool,
}

impl Default for Session {
    /// The interactive program, with mental ray 1.9 absent.
    fn default() -> Self {
        Session {
            standalone_renderer: false,
            emergency_saving: false,
            light_license: false,
            mental_ray_19: false,
            shade_optimize_editing_env: false,
            mouse_dialog: true,
        }
    }
}

/// How to write.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteOptions {
    /// The version printed in the header: `SI_versionOutput` (0x086793f4), 4.0 in this build.
    pub version: f32,
    /// `indentLength` (0x086cb960) when the save starts. It is a global shared by every ASCII
    /// writer and the setup writer does not leave it as it found it: the picture-format
    /// section lowers it by one (not below zero) and the preview section raises it by one. So
    /// it is 0 for the first save of a session and 1 for every later one, as far as the
    /// setup writer alone is concerned.
    pub indent: i32,
    /// `SI_audioMngContainer` flag at 0x087fba10: tracks have a volume, so `VOLUME` is written.
    pub audio_volume: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions { version: si_file::VERSION_OUTPUT, indent: 0, audio_volume: true }
    }
}

/// Where the original shows a line in its status bar and carries on. The texts are the
/// program's; here only the place and the offending token are kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// The keyword whose value was not understood.
    pub keyword: &'static str,
    pub token: Vec<u8>,
}

/// What a read did, beyond the structure it filled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadInfo {
    /// The header's tag, or `None` when the file does not start with one of the seven tags of
    /// the ASCII header reader. The original does not look at the result: such a file is
    /// read from its first byte with version −1.0.
    pub tag: Option<u32>,
    /// The version of the header, as passed down to the sections.
    pub version: f32,
    /// Nothing but blanks is left after the reader returned.
    pub all_tokens_consumed: bool,
    /// Offset at which the reader stopped.
    pub end: usize,
    /// Tokens asked for.
    pub tokens_read: usize,
    /// Tokens asked for when the file had ended. A file that closes with the word `EOF` is
    /// read without any; one that does not costs the reader two (the last section finds the
    /// end, then the main loop does).
    pub reads_past_end: usize,
    /// The reader left through the word `EOF`.
    pub ended_on_eof_word: bool,
    /// The sections in the order met.
    pub sections: Vec<&'static str>,
    /// Words the main loop did not know and skipped, one token each.
    pub unknown_words: Vec<Vec<u8>>,
    /// Status-bar messages the original would have shown.
    pub messages: Vec<Message>,
    /// Places where the original writes outside an array or reads memory it never set, and
    /// this reader does something defined instead (said at each spot in the code).
    pub deviations: Vec<&'static str>,
    /// Function curves read as values. The original allocates each, fills it, and returns
    /// without keeping the pointer: the curve is lost and the value it stood for is left as
    /// it was.
    pub lost_fcurves: Vec<Fcurve>,
}

/// The cases in which this reader stops where the original would not come back.
#[derive(Clone, Debug, PartialEq)]
pub enum SetupError {
    /// The original loops for ever: the loop named waits for a word and does not test whether
    /// the token reader still delivers.
    SpinsForever { at: &'static str },
    /// The original dereferences a null window here and dies: the keyword stands in the block
    /// of a window the program does not have (`L1_…`), or before any `WINDOW_TYPE`.
    NullWindow { keyword: &'static str },
    /// A count from the file is larger than the bytes left in it. The original allocates for
    /// it regardless.
    CountTooLarge { keyword: &'static str, count: i32 },
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SetupError::SpinsForever { at } => write!(f, "the original never returns: {at} waits for a word past the end of the file"),
            SetupError::NullWindow { keyword } => write!(f, "the original crashes: {keyword} with no window"),
            SetupError::CountTooLarge { keyword, count } => write!(f, "{keyword} {count}: more than the file can hold"),
        }
    }
}

impl std::error::Error for SetupError {}

// ------------------------------------------------------------------------------------------
// the structure
// ------------------------------------------------------------------------------------------

/// Names of the show flags in menu order: the index is the item of the Show menu (0x96) and
/// the argument of `SI_setShowFlag`.
pub const SHOW_NAMES: [&str; 18] = [
    "SHW_POINTS", "SHW_LINES", "SHW_CENTER", "SHW_NULL", "SHW_NORMAL", "SHW_TAG", "SHW_LINK", "SHW_CONE",
    "SHW_LATTICE", "SHW_POLYGON", "SHW_EDGE", "SHW_SURFACE", "SHW_SKELETON", "SHW_CONTROLS", "SHW_MODIF",
    "SHW_UNSEL_POINT", "SHW_UNSEL_LINE", "SHW_UNSEL_TAG",
];

/// `SHOW_FLAG`. Each flag lives twice in the original: as a global the drawing code tests
/// (`SI_shwPnt` 0x08679528, `SI_shwLin`, …) and as the state of an item of the Show menu,
/// which is what the writer prints.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowFlags {
    /// The globals, by index of [`SHOW_NAMES`].
    pub flags: [i16; 18],
    /// The menu states. Not touched by the reader under the stand-alone renderer.
    pub menu: [i16; 18],
}

/// `PREFERENCE`: manipulation, animation and saving preferences (`SI_glbManip` 0x08781560,
/// `SI_glbAnim` 0x08781480 and a few loose globals).
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    /// `PREF_BUSY_CURSOR` (`SI_busyCursor` 0x086792b2)
    pub busy_cursor: i16,
    /// `PREF_CLASSICAL_SCALING` (0x08781560): stored as 0 or 1 whatever the number.
    pub classical_scaling: i32,
    /// Set to 1 when `PREF_CLASSICAL_SCALING` changed the value above (0x0878158a); the scene
    /// loader looks at it afterwards. Never under the stand-alone renderer.
    pub classical_scaling_changed: i16,
    /// `PREF_UNIFORM_SCALING` (0x08781564)
    pub uniform_scaling: i32,
    /// `PREF_HIERARCHY_COMPENSATION` (`SI_hrchCpst` 0x0867949c)
    pub hierarchy_compensation: i16,
    /// `PREF_MODE` (0x08781574). `PREF_TRANSLATION` overwrites it.
    pub mode: i32,
    /// `PREF_ROTATION` (0x08781568): 3 is stored as 2.
    pub rotation: i32,
    /// `PREF_TRANSLATION` (0x0878156c): 3 is stored as 4.
    pub translation: i32,
    /// `PREF_MANIPULATION` (0x08781570)
    pub manipulation: i32,
    /// `PREF_MULTI_NODE` (0x0878157c)
    pub multi_node: i16,
    /// `PREF_HIERARCHY_MANIPULATION` (0x0878157e)
    pub hierarchy_manipulation: i16,
    /// `PREF_CONTINUOUS_ROTATION` (0x08781580)
    pub continuous_rotation: i16,
    /// `PREF_MODELLING_RELATION` (0x08781582)
    pub modelling_relation: i16,
    /// `PREF_IMAGE_LOCK` (0x08781480)
    pub image_lock: i16,
    /// `PREF_HIGHLIGHT_KEY` (0x08781482)
    pub highlight_key: i16,
    /// `PREF_CONFIRM_KEYEDIT` (0x08781484)
    pub confirm_keyedit: i16,
    /// `PREF_RIGHT_HANDED_SURFACE` (`SI_rightHand` 0x086793ec)
    pub right_handed_surface: i16,
    /// `PREF_ZOOM_ON_CURSOR` (`SI_zoomONCursor` 0x0867945c)
    pub zoom_on_cursor: i16,
    /// `PREF_USERTRANSFO` (0x08781592)
    pub user_transfo: i16,
    /// `SAVE_MAT_TXT_NOT_SEPARATELY` (`si_saveOptNoMatTxt`)
    pub save_mat_txt_not_separately: i16,
    /// `SAVE_NOT_RM_OLD_VERSIONS` (`si_saveOptNoOldVer`)
    pub save_not_rm_old_versions: i16,
    /// `PREF_FCV_INTERP_TYPE` (0x087814b0): also the interpolation a new function curve gets.
    pub fcv_interp_type: i16,
    /// `PREF_FCV_EDT_UPD_MODE` (`SI_refreshElemFcvOption` 0x08695e8c)
    pub fcv_edt_upd_mode: i16,
    /// `PREF_DOPE_SNAP_KEYS` (0x087814b2)
    pub dope_snap_keys: i16,
    /// `PREF_SEQ_ACT_FCV` (0x087814ba)
    pub seq_act_fcv: i16,
    /// `PREF_AUTO_CLUSTER_CTR` (0x08781588)
    pub auto_cluster_ctr: i16,
    /// `PREF_INVKIN_AUTO_CNS` (0x087814b4)
    pub invkin_auto_cns: i32,
    /// `PREF_SHOW_DIRECTION_CONSTRAINT` (`SI_shwDircns` 0x08679518)
    pub show_direction_constraint: i16,
    /// `PREF_SHOW_CRITICAL_ZONE` (`SI_shwCZone` 0x08679530)
    pub show_critical_zone: i16,
    /// `PREF_SWIFTKEYS_ALL_MODULES` (0x08781594)
    pub swiftkeys_all_modules: i16,
    /// `PREF_FIT_SLP_TO_FCV_UNI_SLP` (0x087814be)
    pub fit_slp_to_fcv_uni_slp: i16,
}

impl Default for Preferences {
    /// What the program starts with: the fields of the manipulation and animation records as
    /// their initialisers set them `SI_manipulationInit` (0x080d3210, in part)
    /// `SI_animationInfoInit` (0x080d335c, in part) (of those two functions only the fields a setup
    /// file carries are here), and the static initial values of the loose globals.
    fn default() -> Self {
        Preferences {
            busy_cursor: 1,
            classical_scaling: 0,
            classical_scaling_changed: 0,
            uniform_scaling: 0,
            hierarchy_compensation: 1,
            mode: 4,
            rotation: 2,
            translation: 4,
            manipulation: 0,
            multi_node: 0,
            hierarchy_manipulation: 0,
            continuous_rotation: 1,
            modelling_relation: 0,
            image_lock: 0,
            highlight_key: 0,
            confirm_keyedit: 1,
            right_handed_surface: 1,
            zoom_on_cursor: 0,
            user_transfo: 0,
            save_mat_txt_not_separately: 0,
            save_not_rm_old_versions: 0,
            fcv_interp_type: 2,
            fcv_edt_upd_mode: 0,
            dope_snap_keys: 1,
            seq_act_fcv: 0,
            auto_cluster_ctr: 0,
            invkin_auto_cns: 0,
            show_direction_constraint: 1,
            show_critical_zone: 1,
            swiftkeys_all_modules: 0,
            fit_slp_to_fcv_uni_slp: 0,
        }
    }
}

/// `PREFERENCE_AUTO_SAVE`
#[derive(Clone, Debug, PartialEq)]
pub struct AutoSave {
    /// `AUTO_SAVE_ACTIVE` (`SI_backupActive` 0x086cad74)
    pub active: i16,
    /// `AUTO_SAVE_PERIOD`, in seconds (`SI_autosaveperiode` 0x088177c4); the file gives minutes.
    pub period: i32,
}

impl Default for AutoSave {
    /// Off (the static initial value), with a period of one minute: the least the start-up
    /// leaves (`si_autosaveInit`, 0x083d172c, takes the minutes from the auto-save dialog,
    /// whose own default was not read, and makes them at least 1).
    fn default() -> Self {
        AutoSave { active: 0, period: 60 }
    }
}

/// `PREFERENCE_MOUSE`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseDial {
    /// `DIAL_MOUSE_POS` (0x087820f6)
    pub pos: i16,
    /// `DIAL_MOUSE_WARP`: the number in text field 6 of the mouse dialog. The reader puts it
    /// there printed with `%d` after cutting the value to an int and then to a short.
    pub warp: i16,
    /// `DIAL_MOUSE_DISP`: text field 9, likewise.
    pub disp: i16,
    /// `DIAL_MOUSE_BELL`: state item 0x10 of the dialog, 0 or 1.
    pub bell: i16,
}

/// `PREFERENCE_INCREMENTATION` (`SI_glbIncrem` 0x08781520): four steps each for scaling,
/// rotation and translation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Increments {
    pub scaling: [f32; 4],
    pub rotation: [f32; 4],
    pub translation: [f32; 4],
}

/// The thirty keywords of `DUPLICATE_SETUP` in the order written, with the offset of each in
/// `SI_glbDuplic` (0x087814c0). All are shorts except `DUP_CONNECTION_TO`, an int.
pub const DUPLICATE_NAMES: [(&str, u8); 30] = [
    ("DUP_MATERIAL_DUP", 0x00), ("DUP_MATERIAL_SHR", 0x02), ("DUP_MATERIAL_ANIM", 0x04),
    ("DUP_TXT2DLCL_DUP", 0x06), ("DUP_TXT2DLCL_SHR", 0x08), ("DUP_TXT2DLCL_ANIM", 0x0a),
    ("DUP_TXT3DLCL_DUP", 0x0c), ("DUP_TXT3DLCL_SHR", 0x0e), ("DUP_TXT3DLCL_ANIM", 0x10),
    ("DUP_TXT2DGLB_DUP", 0x12), ("DUP_TXT2DGLB_SHR", 0x14), ("DUP_TXT2DGLB_ANIM", 0x16),
    ("DUP_TXT3DGLB_DUP", 0x18), ("DUP_TXT3DGLB_SHR", 0x1a), ("DUP_TXT3DGLB_ANIM", 0x1c),
    ("DUP_LATTICE_DUP", 0x1e), ("DUP_LATTICE_ANIM", 0x20), ("DUP_KLUSTER_DUP", 0x22), ("DUP_KLUSTER_ANIM", 0x24),
    ("DUP_USERDATA_DUP", 0x26), ("DUP_TRANSFO_ANIM", 0x28), ("DUP_SHAPE_ANIM", 0x2a), ("DUP_DEFORM_ANIM", 0x2c),
    ("DUP_CONNECTION_TO", 0x30), ("DUP_CREATE_NULL", 0x34), ("DUP_GEOMETRY_CURR", 0x36), ("DUP_POLY_MERGE", 0x38),
    ("DUP_LIGHT_ANIM", 0x50), ("DUP_ALL_TEXTURE2D_ANIM", 0x52), ("DUP_ALL_TEXTURE3D_ANIM", 0x54),
];

/// Index of `DUP_CONNECTION_TO` in [`DUPLICATE_NAMES`].
const DUP_CONNECTION_TO: usize = 23;

/// `DUPLICATE_SETUP`: what Duplicate copies, shares and animates.
#[derive(Clone, Debug, PartialEq)]
pub struct Duplicate {
    /// By index of [`DUPLICATE_NAMES`]. Entry 23 is the int `DUP_CONNECTION_TO`, which the
    /// reader fills with a sign-extended short.
    pub values: [i32; 30],
}

impl Default for Duplicate {
    /// `SI_duplicationInfoInit` (0x080d34f8, in part) with its argument non-zero, as the start-up
    /// calls it; only the fields a setup file carries.
    fn default() -> Self {
        Duplicate { values: [1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0] }
    }
}

/// `SCHEMATIC` (`SI_glbSchem` 0x087fb960)
#[derive(Clone, Debug, PartialEq)]
pub struct Schematic {
    /// `SCHEM_ORGANIZATION_TYPE` (+0x00)
    pub organization_type: i32,
    /// `SCHEM_REPARTITION_TYPE` (+0x04)
    pub repartition_type: i32,
    /// `SCHEM_REL_DISPLAY` (+0x08)
    pub rel_display: i32,
    /// `SCHEM_SORT_TYPE` (+0x14)
    pub sort_type: i32,
    /// `SCHEM_PREFIX_OPTION` (+0x18)
    pub prefix_option: i32,
    /// `SCHEM_AUTO_CLEAN` (+0x1e)
    pub auto_clean: i16,
    /// `SCHEM_RECUP_EMPTIES` (+0x20)
    pub recup_empties: i16,
    /// +0x3c: not in the file. The database loader raises it while it loads; while it is up,
    /// `SCHEM_UPDATE` leaves [`Schematic::update`] alone.
    pub loading: i16,
    /// `SCHEM_USE_DEF_COLORS` (+0x3e)
    pub use_def_colors: i16,
    /// `SCHEM_AUTO_SCROLL` (+0x42)
    pub auto_scroll: i16,
    /// `SCHEM_PARALLEL_LINKS` (+0x44)
    pub parallel_links: i16,
    /// The update flag in force (+0x48): what the writer prints as `SCHEM_UPDATE`.
    pub update: i16,
    /// `SCHEM_UPDATE` as read (+0x4a).
    pub update_wanted: i16,
    /// `SCHEM_GRID_LOCK` (+0x4c)
    pub grid_lock: i16,
}

impl Default for Schematic {
    /// `SI_schematicInfoInit` (0x080d364c, in part), the fields a setup file carries.
    fn default() -> Self {
        Schematic {
            organization_type: 0,
            repartition_type: 0,
            rel_display: 0,
            sort_type: 0,
            prefix_option: 2,
            auto_clean: 0,
            recup_empties: 0,
            loading: 0,
            use_def_colors: 1,
            auto_scroll: 0,
            parallel_links: 0,
            update: 1,
            update_wanted: 0,
            grid_lock: 0,
        }
    }
}

/// `SHADE_SETUP_GLB`: display properties shared by all shaded views (`SI_GPROPS` 0x086fd720),
/// and a second copy the hardware renderer keeps for itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadeGlb {
    /// `BLENDING` (+0x00): 0 `NONE`, 1 anything else (written `ALPHA`).
    pub blending: i32,
    /// `REPEAT` (+0x04): 1 `GRID`, 0 anything else (written `NONE`).
    pub repeat: i16,
    /// `LOD` (+0x08)
    pub lod: i32,
    /// `TXT_DPY_GLOBAL` (+0x18)
    pub txt_dpy_global: i16,
    /// `TXT_DPY_LOCAL` (+0x1a)
    pub txt_dpy_local: i16,
    /// `TXT_DPY_PREC` (+0x1c)
    pub txt_dpy_prec: i32,
    /// `TXT_DPY_LOCAL_MODE` (+0x20)
    pub txt_dpy_local_mode: i32,
    /// `TEXTURE_PXL_INTERPOLATION_MODE` (+0x24): 0 `RESPECT_TXT_PROPERTY`, 1 `ALWAYS_ON`,
    /// 2 `ALWAYS_OFF`.
    pub texture_pxl_interpolation_mode: i32,
    /// `OPTIMISE_EDITING` (+0x44)
    pub optimise_editing: i16,
}

impl Default for ShadeGlb {
    /// The static initial values of `SI_GPROPS`.
    fn default() -> Self {
        ShadeGlb {
            blending: 0,
            repeat: 0,
            lod: 0,
            txt_dpy_global: 1,
            txt_dpy_local: 1,
            txt_dpy_prec: 1,
            txt_dpy_local_mode: 0,
            texture_pxl_interpolation_mode: 0,
            optimise_editing: 0,
        }
    }
}

/// `DOPESHEET`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dopesheet {
    /// `DS_RIPPLING` (0x0878fb1c)
    pub rippling: i16,
    /// `DS_DISPLAY_ACTIONS` (0x0878fada)
    pub display_actions: i16,
    /// `DS_ANCHOR_KEYS` (0x0878fb1e)
    pub anchor_keys: i16,
    /// `DS_AFFECT_KEYS` (0x0878fb22)
    pub affect_keys: i16,
}

/// Low, medium or high, as three of the audio keywords give it. The reader hands the audio
/// manager three flags, at most one of them set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Level {
    Low,
    Medium,
    /// Also what the writer prints when neither of the other two flags is up.
    #[default]
    High,
    /// The word was none of the three: all three flags down.
    Unset,
}

/// One `AUDIOMARKER`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioMarker {
    pub name: Option<Vec<u8>>,
    pub start: f32,
    pub end: f32,
    pub active: bool,
}

/// One `AUDIOTRACK` with the settings that follow it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioTrack {
    pub name: Option<Vec<u8>>,
    pub path: Option<Vec<u8>>,
    pub active: bool,
    /// `STARTOFFSET`: a time and an int.
    pub start_offset: f32,
    pub start_offset_frame: i32,
    /// `PREPOSTROLL`
    pub pre_roll: i32,
    pub post_roll: i32,
    /// `WAVEDISPLAYQUALITY`: a level and an int.
    pub wave_display_quality: Level,
    pub wave_display_value: i32,
    /// `TRACKHEIGHT`
    pub track_height: Level,
    /// `PBBUFFERING`: a level and an int.
    pub pb_buffering: Level,
    pub pb_buffering_value: i32,
    /// `DISPLAYMARKERFCV`: `NONE` (here `Low`), `ACTIVE` (`Medium`) or `ALL` (`High`).
    pub display_marker_fcv: Level,
    /// `VOLUME`
    pub volume: f32,
    pub markers: Vec<AudioMarker>,
}

/// `AUDIOCONTAINER`. In the original this section drives the audio manager, which opens the
/// sound files; the manager is not reproduced. What is kept is what the reader hands it, as
/// if every track loaded, and the writer prints from that.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub tracks: Vec<AudioTrack>,
}

/// Everything a setup file sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub show: ShowFlags,
    pub pref: Preferences,
    pub autosave: AutoSave,
    pub mouse: MouseDial,
    pub increments: Increments,
    pub duplicate: Duplicate,
    pub schematic: Schematic,
    /// `SI_fmtAspect` (0x08679540): the aspect ratio of each of the fifteen picture formats.
    /// The last two are the custom ones and are overwritten when a file gives an aspect or a
    /// size. The format in force and its numbers are fields of the scene camera
    /// ([`Layout::scene_camera`]).
    pub format_aspect: [f32; 15],
    pub shade_glb: ShadeGlb,
    pub audio: Audio,
    /// `ACTIVE_MODULE` (`SI_active` 0x08679480)
    pub active_module: i16,
    pub layout: Layout,
    /// `RENDERING_TYPE` … `DITHERING` and `GLOBAL_SWITCHES` (`SI_defaultControl` 0x08781800)
    pub render: RenderControl,
    /// The display properties of the hardware renderer (`SI_renderGprops` 0x08782120): the
    /// `TEXTURE` … lines inside the render section.
    pub render_shade: ShadeSetup,
    /// Its copy of the global properties (`hwGlbGprops`): `SHADE_SETUP_GLB` inside the render
    /// section.
    pub render_shade_glb: ShadeGlb,
    pub preview: Preview,
    /// `mi2output`: 1 when the mental ray in use is the 2.x one.
    pub mi2_output: i16,
    pub dopesheet: Dopesheet,
}

impl Default for Setup {
    /// The state the reader starts from. Fields get the program's start-up value where that
    /// was read from an initialiser (each `Default` says which); the rest are zero. The view
    /// windows in particular start empty here, while the program fills them from a window
    /// template file that is not read.
    fn default() -> Self {
        Setup {
            show: ShowFlags::default(),
            pref: Preferences::default(),
            autosave: AutoSave::default(),
            mouse: MouseDial::default(),
            increments: Increments::default(),
            duplicate: Duplicate::default(),
            schematic: Schematic::default(),
            format_aspect: [1.37, 1.6667, 1.85, 2.19, 1.364, 1.364, 1.3333, 1.3333, 1.777778, 1.5, 1.0, 1.5, 1.25, 1.18, 1.0],
            shade_glb: ShadeGlb::default(),
            audio: Audio::default(),
            active_module: 0,
            layout: Layout::default(),
            render: RenderControl::default(),
            render_shade: ShadeSetup::for_renderer(),
            render_shade_glb: ShadeGlb::default(),
            preview: Preview::default(),
            mi2_output: 0,
            dopesheet: Dopesheet::default(),
        }
    }
}

// ------------------------------------------------------------------------------------------
// the token reader of asceneio.c
// ------------------------------------------------------------------------------------------

/// A setup file being read: the stream, the token buffer `si_tline` (0x08817f60, which
/// `si_chrToken` points at), and the three globals the "Na" value readers leave their result
/// in (`si_ivalue` 0x08817f44, `si_fvalue` 0x08817f40, `si_svalue` 0x08817f48).
pub(crate) struct Reader<'a, 'f> {
    f: &'f mut AsciiFile<'a>,
    /// The token as a C string: the bytes up to the first NUL.
    tok: Vec<u8>,
    pub ivalue: i32,
    pub fvalue: f32,
    pub svalue: i16,
    pub session: &'f Session,
    pub info: ReadInfo,
}

impl<'a, 'f> Reader<'a, 'f> {
    pub(crate) fn new(f: &'f mut AsciiFile<'a>, session: &'f Session) -> Self {
        Reader { f, tok: Vec::new(), ivalue: 0, fvalue: 0.0, svalue: 0, session, info: ReadInfo::default() }
    }

    /// `strcmp(si_chrToken, word) == 0`
    pub fn is(&self, word: &str) -> bool {
        self.tok == word.as_bytes()
    }

    pub fn token(&self) -> &[u8] {
        &self.tok
    }

    /// Bytes not yet read.
    pub fn bytes_left(&self) -> usize {
        self.f.len() - self.f.tell()
    }

    pub fn message(&mut self, keyword: &'static str) {
        self.info.messages.push(Message { keyword, token: self.tok.clone() });
    }

    /// Reads the next token [soft 0x083dbeb0 si_inToken]: spaces, tabs and newlines are
    /// skipped, then everything up to the next of those (or the end of the file) is the
    /// token. False only when the file ends before a token starts; the buffer is then empty.
    /// A token that runs into the end of the file is a token like any other.
    ///
    /// The original writes into a 256-byte buffer with no bound check: a longer token runs
    /// into the globals behind it. Here the token just grows.
    pub fn in_token(&mut self) -> bool {
        self.info.tokens_read += 1;
        self.tok.clear();
        let first = loop {
            let c = self.f.getc();
            if c == EOF {
                self.info.reads_past_end += 1;
                return false;
            }
            if !(c == 0x20 || c == 9 || c == 0xa) {
                break c;
            }
        };
        let mut buf = vec![first as u8];
        loop {
            let c = self.f.getc();
            if c == EOF || c == 0x20 || c == 9 || c == 0xa {
                break;
            }
            buf.push(c as u8);
        }
        // what strcmp, atoi and atof see ends at the first NUL
        if let Some(n) = buf.iter().position(|&b| b == 0) {
            buf.truncate(n);
        }
        self.tok = buf;
        true
    }

    /// A token as an int, stored through the pointer [soft 0x083dbf44 si_inIvalue]. At the
    /// end of the file nothing is stored.
    pub fn in_ivalue(&mut self, dst: &mut i32) -> bool {
        if !self.in_token() {
            return false;
        }
        *dst = cnum::atoi(&self.tok);
        true
    }

    /// A token as an int, left in `si_ivalue` [soft 0x083dc064 si_inIvalueNa]. At the end of
    /// the file the global keeps its last value, and the callers use it all the same.
    pub fn in_ivalue_na(&mut self) -> bool {
        let mut v = self.ivalue;
        let ok = self.in_ivalue(&mut v);
        self.ivalue = v;
        ok
    }

    /// A token as a single [soft 0x083dc184 si_inFvalue]: `atof`, then stored as a float.
    pub fn in_fvalue(&mut self, dst: &mut f32) -> bool {
        if !self.in_token() {
            return false;
        }
        *dst = cnum::atof(&self.tok) as f32;
        true
    }

    /// A token as a single, left in `si_fvalue` [soft 0x083dc2a4 si_inFvalueNa].
    pub fn in_fvalue_na(&mut self) -> bool {
        let mut v = self.fvalue;
        let ok = self.in_fvalue(&mut v);
        self.fvalue = v;
        ok
    }

    /// A token as a short [soft 0x083dc3c4 si_inSvalue]: `atoi`, the low 16 bits kept.
    pub fn in_svalue(&mut self, dst: &mut i16) -> bool {
        if !self.in_token() {
            return false;
        }
        *dst = cnum::atoi(&self.tok) as i16;
        true
    }

    /// A token as a short, left in `si_svalue` [soft 0x083dc4e4 si_inSvalueNa].
    pub fn in_svalue_na(&mut self) -> bool {
        let mut v = self.svalue;
        let ok = self.in_svalue(&mut v);
        self.svalue = v;
        ok
    }

    /// `si_inSvalueNa` followed by the sign-extending load most callers do.
    pub fn short(&mut self) -> i16 {
        self.in_svalue_na();
        self.svalue
    }

    /// `si_inFvalueNa` and the value it leaves.
    pub fn float(&mut self) -> f32 {
        self.in_fvalue_na();
        self.fvalue
    }

    /// A token and whether it is `word`: the two-way choices (`YES` or anything else).
    pub fn next_is(&mut self, word: &str) -> bool {
        self.in_token();
        self.is(word)
    }

    /// A quoted string [soft 0x083dc604 si_stringAsciiRead].
    ///
    /// After blanks, the first byte is taken to be the opening quote whatever it is, and
    /// bytes are gathered up to the next `'` or the end of the file; spaces and newlines
    /// inside belong to the string. The result is what stands between the first byte and the
    /// closing one. `None` is the original's null pointer: the file ended before the string
    /// started, or the string is empty (`''`).
    ///
    /// The string is gathered in the token buffer, 239 bytes at most: from the 238th
    /// character on, each new one overwrites the last, so a longer string keeps its first
    /// 237 characters. The buffer then holds the string with its quotes: that is the current
    /// token afterwards.
    pub fn string_read(&mut self) -> Option<Vec<u8>> {
        self.info.tokens_read += 1;
        self.tok.clear();
        let first = loop {
            let c = self.f.getc();
            if c == EOF {
                self.info.reads_past_end += 1;
                return None;
            }
            if !(c == 0x20 || c == 9 || c == 0xa) {
                break c;
            }
        };
        let mut t = vec![first as u8];
        let mut i = 1usize;
        let term = loop {
            let c = self.f.getc();
            if c == EOF || c == 0x27 {
                break c;
            }
            if i < t.len() {
                t[i] = c as u8;
            } else {
                t.push(c as u8);
            }
            i += 1;
            if i > 0xee {
                i = 0xee;
            }
        };
        t.truncate(i);
        t.push(term as u8); // the closing quote, or 0xff when the file ended
        if let Some(n) = t.iter().position(|&b| b == 0) {
            t.truncate(n);
        }
        self.tok = t;
        if self.tok.len() >= 2 && self.tok[0] == 0x27 && self.tok[1] == 0x27 {
            return None;
        }
        if self.tok.len() < 2 {
            // A NUL right after the opening byte (or in its place): the original copies into
            // an allocation of one or zero bytes and writes its terminator in front of it.
            self.info.deviations.push("string with a NUL at its start");
            return Some(Vec::new());
        }
        Some(self.tok[1..self.tok.len() - 1].to_vec())
    }

    /// Three singles [soft 0x083dcb18 si_vectorAsciiRead]. They are read into locals and
    /// copied out only if the caller gave somewhere to put them. A component the file ends
    /// before is uninitialised stack in the original; here it is 0.
    pub fn vector_read(&mut self, dst: Option<&mut [f32; 3]>) -> bool {
        let mut v = [0.0f32; 3];
        for x in v.iter_mut() {
            self.in_fvalue(x);
        }
        match dst {
            Some(d) => {
                *d = v;
                true
            }
            None => false,
        }
    }
}

/// `(int)x` as the original's x87 code does it (`fistp` with rounding set to truncate): a
/// value that does not fit, or a NaN, gives 0x80000000.
pub(crate) fn c_float_to_int(x: f32) -> i32 {
    let t = (x as f64).trunc();
    if t.is_nan() || t < -2147483648.0 || t > 2147483647.0 {
        i32::MIN
    } else {
        t as i32
    }
}

// ------------------------------------------------------------------------------------------
// the reader
// ------------------------------------------------------------------------------------------

/// Reads a setup file into the program's start-up state, as the interactive program.
pub fn read(data: &[u8]) -> Result<(Setup, ReadInfo), SetupError> {
    let mut setup = Setup::default();
    let info = read_into(data, &mut setup, &Session::default())?;
    Ok((setup, info))
}

/// Reads a setup file over an existing state [soft 0x083be024 SI_setupFileLoadPath].
///
/// The header is read and its result ignored: a file with another tag, or with none, is
/// handed to the section reader all the same (from its first byte and with version −1.0 if
/// the tag is not one of the seven). What the original does around this (cursor, opening the
/// file, error text) is the caller's business.
///
/// On an error the state holds what was stored up to there.
pub fn read_into(data: &[u8], setup: &mut Setup, session: &Session) -> Result<ReadInfo, SetupError> {
    let mut f = AsciiFile::new(data);
    let header = read_asc_header(&mut f);
    let version = header.map_or(-1.0, |h| h.1);
    let mut r = Reader::new(&mut f, session);
    r.info.tag = header.map(|h| h.0);
    r.info.version = version;
    let result = load_setup(&mut r, setup, version);
    let mut info = r.info;
    info.end = f.tell();
    info.all_tokens_consumed = f.only_blanks_left();
    result.map(|_| info)
}

/// The section loop [soft 0x083e2f8c SI_asceneLoadSetup].
///
/// Each word that opens a section calls that section's reader. A section reader returns when
/// it meets a word it does not know, leaving that word in the token buffer, and the loop
/// looks at it again without reading; the one-line sections (`PREFERENCE_PICT_FMT`,
/// `ACTIVE_MODULE`) and words the loop does not know cost one fresh token instead. The loop
/// ends at the word `EOF` or when the file does. The very first token is not compared with
/// `EOF`.
fn load_setup(r: &mut Reader, s: &mut Setup, version: f32) -> Result<(), SetupError> {
    if !r.in_token() {
        return Ok(());
    }
    let mut first = true;
    loop {
        if !first && r.is("EOF") {
            r.info.ended_on_eof_word = true;
            break;
        }
        first = false;
        let mut need = true;
        if r.is("SHOW_FLAG") {
            r.info.sections.push("SHOW_FLAG");
            show_flag_read(r, s);
            need = false;
        } else if r.is("PREFERENCE") {
            r.info.sections.push("PREFERENCE");
            pref_menu_read(r, s);
            need = false;
        } else if r.is("PREFERENCE_MOUSE") {
            r.info.sections.push("PREFERENCE_MOUSE");
            mouse_dial_read(r, s);
            need = false;
        } else if r.is("PREFERENCE_AUTO_SAVE") {
            r.info.sections.push("PREFERENCE_AUTO_SAVE");
            autosave_pref_read(r, s);
            need = false;
        } else if r.is("PREFERENCE_INCREMENTATION") {
            r.info.sections.push("PREFERENCE_INCREMENTATION");
            increment_pref_dial_read(r, s);
            need = false;
        } else if r.is("SCHEMATIC") {
            r.info.sections.push("SCHEMATIC");
            schematic_pref_read(r, s);
            need = false;
        } else if r.is("DUPLICATE_SETUP") {
            r.info.sections.push("DUPLICATE_SETUP");
            duplicate_setup_read(r, s);
            need = false;
        } else if r.is("PREFERENCE_PICT_FMT") {
            r.info.sections.push("PREFERENCE_PICT_FMT");
            picture_format_read(r, s);
        } else if r.is("SHADE_SETUP_GLB") {
            r.info.sections.push("SHADE_SETUP_GLB");
            let session = r.session;
            shade_setup_glb_read(r, &mut s.shade_glb, session);
            need = false;
        } else if r.is("AUDIOCONTAINER") {
            r.info.sections.push("AUDIOCONTAINER");
            audio_setup_read(r, s);
            need = false;
        } else if r.is("ACTIVE_MODULE") {
            r.info.sections.push("ACTIVE_MODULE");
            r.in_svalue(&mut s.active_module);
            // Module numbers moved twice. The version is a single; the limits are doubles
            // (0x0865b768, 0x0865b770, 0x0865b778), so a header that says 2.64 is above 2.64.
            let v = version as f64;
            if v > 2.613 && v < 2.64 {
                if s.active_module == 4 {
                    s.active_module = 5;
                }
            } else if v > 2.64 && v < 2.99907 && s.active_module > 3 {
                s.active_module -= 1;
            }
        } else if r.is("WINDOW_LAYOUT") {
            r.info.sections.push("WINDOW_LAYOUT");
            if !r.session.standalone_renderer {
                layout::layout_read(r, s)?;
                need = false;
            }
        } else if r.is("RENDERING_TYPE") {
            r.info.sections.push("RENDERING_TYPE");
            if render::render_read(r, s)? == 0 {
                // the word that ended the render section is looked at next, unread
                continue;
            }
        } else if r.is("GLOBAL_SWITCHES") {
            r.info.sections.push("GLOBAL_SWITCHES");
            render::render_globals_read(r, &mut s.render);
            need = false;
        } else if r.is("PREVIEW") {
            r.info.sections.push("PREVIEW");
            render::preview_read(r, s);
            need = false;
        } else if r.is("DOPESHEET") {
            r.info.sections.push("DOPESHEET");
            dopesheet_pref_read(r, s);
            need = false;
        } else {
            // a getc and, unless the file has ended, an ungetc of the same byte: no effect
            let c = r.f.getc();
            if c != EOF {
                let at = r.f.tell() - 1;
                r.f.seek(at);
            }
            // (the empty token the end of the file leaves comes through here too)
            if !r.tok.is_empty() {
                let word = r.tok.clone();
                r.info.unknown_words.push(word);
            }
        }
        if need && !r.in_token() {
            break;
        }
    }
    // (a blank status line, and a notice if an audio track failed to load: not reproduced)
    Ok(())
}

/// [soft 0x083e33b8 showFlagAsciiRead]: `SHW_…` and a number each, until another word.
fn show_flag_read(r: &mut Reader, s: &mut Setup) {
    r.in_token();
    loop {
        let Some(i) = SHOW_NAMES.iter().position(|n| r.is(n)) else { return };
        let v = r.short();
        set_show_flag(&mut s.show, i, v, r.session);
        if !r.in_token() {
            return;
        }
    }
}

/// [soft 0x083be5bc SI_setShowFlag] for the Show menu: the item's state in the menu of each
/// of the four modules (not under the stand-alone renderer, which has no menus), and the
/// global. The original also copies flag 3 into the current graphics list; that is not kept.
/// (The menu must have the item for its state to be stored; it is taken to.)
fn set_show_flag(show: &mut ShowFlags, index: usize, value: i16, session: &Session) {
    if !session.standalone_renderer {
        show.menu[index] = value;
    }
    show.flags[index] = value;
}

/// [soft 0x083e3768 prefMenuRead] with the flag setters it calls
/// `SI_setPrefFlag` (0x08416e7c, in part) `SI_skeletonFlagSet` (0x081c9ddc, in part)
/// `SI_actorDirectionFlagSet` (0x081c9e78, in part) `SI_motionDirectionFlagSet` (0x0823d974, in part)
/// [soft 0x0812e0d0 SI_elemdictSetSaveOptions] (their menu updates left out).
///
/// The two `SAVE_…` values are gathered in locals that start at 0 and are stored when the
/// section ends at an unknown word: a section without them resets both. If the section ends
/// with the file instead, they are not stored at all.
fn pref_menu_read(r: &mut Reader, s: &mut Setup) {
    let p = &mut s.pref;
    let mut no_mat_txt = 0i16;
    let mut no_old_ver = 0i16;
    loop {
        if !r.in_token() {
            return;
        }
        if r.is("PREF_BUSY_CURSOR") {
            p.busy_cursor = r.short();
        } else if r.is("PREF_CLASSICAL_SCALING") {
            let v = r.short();
            if !r.session.standalone_renderer && ((v != 0 && p.classical_scaling == 0) || (v == 0 && p.classical_scaling == 1)) {
                p.classical_scaling_changed = 1;
            }
            p.classical_scaling = (v != 0) as i32;
        } else if r.is("PREF_UNIFORM_SCALING") {
            p.uniform_scaling = r.short() as i32;
        } else if r.is("PREF_HIERARCHY_COMPENSATION") {
            p.hierarchy_compensation = r.short();
        } else if r.is("PREF_MODE") {
            p.mode = r.short() as i32;
        } else if r.is("PREF_ROTATION") {
            p.rotation = r.short() as i32;
            if p.rotation == 3 {
                p.rotation = 2;
            }
        } else if r.is("PREF_TRANSLATION") {
            p.translation = r.short() as i32;
            if p.translation == 3 {
                p.translation = 4;
            }
            p.mode = if p.translation == 4 { 4 } else { 2 };
        } else if r.is("PREF_MANIPULATION") {
            p.manipulation = r.short() as i32;
        } else if r.is("PREF_MULTI_NODE") {
            p.multi_node = r.short();
        } else if r.is("PREF_HIERARCHY_MANIPULATION") {
            p.hierarchy_manipulation = r.short();
        } else if r.is("PREF_CONTINUOUS_ROTATION") {
            p.continuous_rotation = r.short();
        } else if r.is("PREF_MODELLING_RELATION") {
            p.modelling_relation = r.short();
        } else if r.is("PREF_IMAGE_LOCK") {
            p.image_lock = r.short();
        } else if r.is("PREF_HIGHLIGHT_KEY") {
            p.highlight_key = r.short();
        } else if r.is("PREF_CONFIRM_KEYEDIT") {
            p.confirm_keyedit = r.short();
        } else if r.is("PREF_RIGHT_HANDED_SURFACE") {
            p.right_handed_surface = r.short();
        } else if r.is("PREF_ZOOM_ON_CURSOR") {
            p.zoom_on_cursor = r.short();
        } else if r.is("PREF_USERTRANSFO") {
            p.user_transfo = r.short();
        } else if r.is("SAVE_MAT_TXT_NOT_SEPARATELY") {
            no_mat_txt = r.short();
        } else if r.is("SAVE_NOT_RM_OLD_VERSIONS") {
            no_old_ver = r.short();
        } else if r.is("PREF_FCV_INTERP_TYPE") {
            p.fcv_interp_type = r.short();
        } else if r.is("PREF_FCV_EDT_UPD_MODE") {
            p.fcv_edt_upd_mode = r.short();
        } else if r.is("PREF_AUTO_CLUSTER_CTR") {
            p.auto_cluster_ctr = r.short();
        } else if r.is("PREF_INVKIN_AUTO_CNS") {
            p.invkin_auto_cns = r.short() as i32;
        } else if r.is("PREF_SHOW_DIRECTION_CONSTRAINT") {
            p.show_direction_constraint = r.short();
        } else if r.is("PREF_SHOW_CRITICAL_ZONE") {
            p.show_critical_zone = r.short();
        } else if r.is("PREF_DOPE_SNAP_KEYS") {
            p.dope_snap_keys = r.short();
        } else if r.is("PREF_SEQ_ACT_FCV") {
            p.seq_act_fcv = r.short();
        } else if r.is("PREF_FIT_SLP_TO_FCV_UNI_SLP") {
            p.fit_slp_to_fcv_uni_slp = r.short();
        } else if r.is("PREF_SWIFTKEYS_ALL_MODULES") {
            p.swiftkeys_all_modules = r.short();
        } else {
            p.save_mat_txt_not_separately = no_mat_txt;
            p.save_not_rm_old_versions = no_old_ver;
            return;
        }
    }
}

/// [soft 0x083e3e50 mouseDialRead]. Position goes to a global; warp, displacement and bell go
/// into the mouse dialog, if there is one. When the section ends at an unknown word the
/// original applies warp and bell to the X server (only if the file gave them); that is not
/// reproduced.
fn mouse_dial_read(r: &mut Reader, s: &mut Setup) {
    let m = &mut s.mouse;
    if !r.in_token() {
        return;
    }
    loop {
        if r.is("DIAL_MOUSE_POS") {
            m.pos = r.short();
        } else if r.is("DIAL_MOUSE_WARP") {
            let v = r.float();
            if r.session.mouse_dialog {
                m.warp = c_float_to_int(v) as i16;
            }
        } else if r.is("DIAL_MOUSE_DISP") {
            let v = r.float();
            if r.session.mouse_dialog {
                m.disp = c_float_to_int(v) as i16;
            }
        } else if r.is("DIAL_MOUSE_BELL") {
            let v = r.short();
            if r.session.mouse_dialog {
                m.bell = (v != 0) as i16;
            }
        } else {
            return;
        }
        if !r.in_token() {
            return;
        }
    }
}

/// [soft 0x083e40c8 autosavePrefRead]. `AUTO_SAVE_ACTIVE` first puts the period back to 60
/// seconds, so the order of the two lines matters. A period outside 1 to 32767 minutes is
/// refused with a message. (Both lines also restart the auto-save clock.)
fn autosave_pref_read(r: &mut Reader, s: &mut Setup) {
    let a = &mut s.autosave;
    loop {
        if !r.in_token() {
            return;
        }
        if r.is("AUTO_SAVE_ACTIVE") {
            a.period = 60;
            r.in_token();
            if r.is("ACTIVE") {
                a.active = 1;
            } else if r.is("INACTIVE") {
                a.active = 0;
            } else {
                r.message("AUTO_SAVE_ACTIVE");
                a.active = 0;
            }
        } else if r.is("AUTO_SAVE_PERIOD") {
            r.in_ivalue_na();
            let minutes = r.ivalue;
            if (minutes.wrapping_sub(1) as u32) <= 0x7ffe {
                a.period = minutes * 60;
            } else {
                r.message("AUTO_SAVE_PERIOD");
            }
        } else {
            return;
        }
    }
}

/// [soft 0x083e4230 incrementPrefDialRead]: three keywords, four singles each.
fn increment_pref_dial_read(r: &mut Reader, s: &mut Setup) {
    loop {
        if !r.in_token() {
            return;
        }
        let dst = if r.is("PREF_SCALING_INCREMENT") {
            &mut s.increments.scaling
        } else if r.is("PREF_ROTATION_INCREMENT") {
            &mut s.increments.rotation
        } else if r.is("PREF_TRANSLATION_INCREMENT") {
            &mut s.increments.translation
        } else {
            return;
        };
        for x in dst.iter_mut() {
            *x = r.float();
        }
    }
}

/// [soft 0x083e43b0 duplicateSetupRead]: thirty keywords, a number each.
fn duplicate_setup_read(r: &mut Reader, s: &mut Setup) {
    loop {
        if !r.in_token() {
            return;
        }
        let Some(i) = DUPLICATE_NAMES.iter().position(|(n, _)| r.is(n)) else { return };
        s.duplicate.values[i] = r.short() as i32;
    }
}

/// [soft 0x083e49bc schematicPrefRead]
fn schematic_pref_read(r: &mut Reader, s: &mut Setup) {
    let c = &mut s.schematic;
    loop {
        if !r.in_token() {
            return;
        }
        if r.is("SCHEM_ORGANIZATION_TYPE") {
            c.organization_type = r.short() as i32;
        } else if r.is("SCHEM_REPARTITION_TYPE") {
            c.repartition_type = r.short() as i32;
        } else if r.is("SCHEM_SORT_TYPE") {
            c.sort_type = r.short() as i32;
        } else if r.is("SCHEM_REL_DISPLAY") {
            c.rel_display = r.short() as i32;
        } else if r.is("SCHEM_USE_DEF_COLORS") {
            c.use_def_colors = r.short();
        } else if r.is("SCHEM_AUTO_SCROLL") {
            c.auto_scroll = r.short();
        } else if r.is("SCHEM_PARALLEL_LINKS") {
            c.parallel_links = r.short();
        } else if r.is("SCHEM_UPDATE") {
            c.update_wanted = r.short();
            if c.loading == 0 {
                c.update = c.update_wanted;
            }
        } else if r.is("SCHEM_PREFIX_OPTION") {
            c.prefix_option = r.short() as i32;
        } else if r.is("SCHEM_AUTO_CLEAN") {
            c.auto_clean = r.short();
        } else if r.is("SCHEM_RECUP_EMPTIES") {
            c.recup_empties = r.short();
        } else if r.is("SCHEM_GRID_LOCK") {
            c.grid_lock = r.short();
        } else {
            return;
        }
    }
}

/// [soft 0x083e4c4c pictureFormatRead]: `PREFERENCE_PICT_FMT` and four numbers on one line,
/// the picture format of the scene camera, its custom aspect, width and height. The camera's
/// aspect ratio becomes that of the format.
fn picture_format_read(r: &mut Reader, s: &mut Setup) {
    if !r.in_token() {
        return;
    }
    let format = cnum::atoi(r.token());
    let cam = s.layout.scene_camera_mut();
    cam.format = format;
    match s.format_aspect.get(format as usize) {
        Some(&a) if format >= 0 => s.layout.scene_camera_mut().aspect = a,
        // The original indexes the table of fifteen with whatever number the file gives and
        // takes the four bytes it finds there.
        _ => r.info.deviations.push("PREFERENCE_PICT_FMT: format outside the table"),
    }
    let cam = s.layout.scene_camera_mut();
    r.in_fvalue(&mut cam.format_aspect);
    r.in_fvalue(&mut cam.format_width);
    r.in_fvalue(&mut cam.format_height);
}

/// [soft 0x083e60d8 shadeSetupGlbRead]. The ints go through a local that is uninitialised in
/// the original if the file ends at that point; here it starts at 0.
pub(crate) fn shade_setup_glb_read(r: &mut Reader, g: &mut ShadeGlb, session: &Session) {
    if !r.in_token() {
        return;
    }
    let mut v = 0i32;
    loop {
        if r.is("TEXTURE_PXL_INTERPOLATION_MODE") {
            r.in_token();
            if r.is("RESPECT_TXT_PROPERTY") {
                g.texture_pxl_interpolation_mode = 0;
            } else if r.is("ALWAYS_ON") {
                g.texture_pxl_interpolation_mode = 1;
            } else if r.is("ALWAYS_OFF") {
                g.texture_pxl_interpolation_mode = 2;
            }
        } else if r.is("BLENDING") {
            g.blending = if r.next_is("NONE") { 0 } else { 1 };
        } else if r.is("REPEAT") {
            g.repeat = r.next_is("GRID") as i16;
        } else if r.is("LOD") {
            r.in_ivalue(&mut v);
            g.lod = v;
        } else if r.is("TXT_DPY_GLOBAL") {
            r.in_ivalue(&mut v);
            g.txt_dpy_global = v as i16;
        } else if r.is("TXT_DPY_LOCAL") {
            r.in_ivalue(&mut v);
            g.txt_dpy_local = v as i16;
        } else if r.is("TXT_DPY_PREC") {
            r.in_ivalue(&mut v);
            g.txt_dpy_prec = v;
        } else if r.is("TXT_DPY_LOCAL_MODE") {
            r.in_ivalue(&mut v);
            g.txt_dpy_local_mode = v;
        } else if r.is("OPTIMISE_EDITING") {
            r.in_ivalue(&mut v);
            g.optimise_editing = if session.shade_optimize_editing_env { 1 } else { v as i16 };
        } else {
            return;
        }
        if !r.in_token() {
            return;
        }
    }
}

/// One of `LOW`, `MEDIUM`, `HIGH` (or of three other words in the same roles).
fn level(r: &Reader, low: &str, medium: &str, high: &str) -> Level {
    if r.is(low) {
        Level::Low
    } else if r.is(medium) {
        Level::Medium
    } else if r.is(high) {
        Level::High
    } else {
        Level::Unset
    }
}

/// [soft 0x083e63ac audioSetupRead], as far as the file goes: every token is consumed as the
/// original consumes it, and what it passes to the audio manager is kept in [`Audio`]. The
/// manager itself (which opens the sound file and may refuse it) is not reproduced: every
/// track counts as loaded.
///
/// Settings that come before the first `AUDIOTRACK` go to a track pointer the original never
/// set; here they are read and dropped.
fn audio_setup_read(r: &mut Reader, s: &mut Setup) {
    let mut have_track = false;
    loop {
        if !r.in_token() {
            return;
        }
        if r.is("AUDIOTRACK") {
            let name = r.string_read();
            let path = r.string_read();
            let active = r.next_is("ACTIVE");
            s.audio.tracks.push(AudioTrack { name, path, active, ..AudioTrack::default() });
            have_track = true;
            continue;
        }
        let mut dropped = AudioTrack::default();
        let t = if have_track { s.audio.tracks.last_mut().unwrap() } else { &mut dropped };
        if r.is("STARTOFFSET") {
            // both go through locals: uninitialised in the original if the file ends here
            let (mut a, mut b) = (0.0f32, 0i32);
            r.in_fvalue(&mut a);
            r.in_ivalue(&mut b);
            t.start_offset = a;
            t.start_offset_frame = b;
        } else if r.is("PREPOSTROLL") {
            let (mut a, mut b) = (0i32, 0i32);
            r.in_ivalue(&mut a);
            r.in_ivalue(&mut b);
            t.pre_roll = a;
            t.post_roll = b;
        } else if r.is("AUDIOMARKER") {
            let name = r.string_read();
            let (mut a, mut b) = (0.0f32, 0.0f32);
            r.in_fvalue(&mut a);
            r.in_fvalue(&mut b);
            let active = r.next_is("ACTIVE");
            t.markers.push(AudioMarker { name, start: a, end: b, active });
        } else if r.is("WAVEDISPLAYQUALITY") {
            r.in_token();
            t.wave_display_quality = level(r, "LOW", "MEDIUM", "HIGH");
            let mut n = 0i32;
            r.in_ivalue(&mut n);
            t.wave_display_value = n;
        } else if r.is("TRACKHEIGHT") {
            r.in_token();
            t.track_height = level(r, "LOW", "MEDIUM", "HIGH");
        } else if r.is("PBBUFFERING") {
            r.in_token();
            t.pb_buffering = level(r, "LOW", "MEDIUM", "HIGH");
            let mut n = 0i32;
            r.in_ivalue(&mut n);
            t.pb_buffering_value = n;
        } else if r.is("DISPLAYMARKERFCV") {
            r.in_token();
            t.display_marker_fcv = level(r, "NONE", "ACTIVE", "ALL");
        } else if r.is("VOLUME") {
            let mut v = 0.0f32;
            r.in_fvalue(&mut v);
            t.volume = v;
        } else {
            return;
        }
    }
}

/// [soft 0x083e6a98 dopesheetPrefRead]
fn dopesheet_pref_read(r: &mut Reader, s: &mut Setup) {
    let d = &mut s.dopesheet;
    loop {
        if !r.in_token() {
            return;
        }
        if r.is("DS_RIPPLING") {
            d.rippling = r.short();
        } else if r.is("DS_DISPLAY_ACTIONS") {
            d.display_actions = r.short();
        } else if r.is("DS_ANCHOR_KEYS") {
            d.anchor_keys = r.short();
        } else if r.is("DS_AFFECT_KEYS") {
            d.affect_keys = r.short();
        } else {
            return;
        }
    }
}

// ------------------------------------------------------------------------------------------
// the writer
// ------------------------------------------------------------------------------------------

/// The file being written, with the indent state all ASCII writers share: `indentLength`
/// (0x086cb960) and the string built from it, `SI_indentString` (0x086ceba0).
pub(crate) struct Out {
    pub buf: Vec<u8>,
    indent: i32,
    indent_string: Vec<u8>,
}

impl Out {
    pub(crate) fn new(indent: i32) -> Self {
        Out { buf: Vec::new(), indent, indent_string: Vec::new() }
    }

    /// Changes the indent level by `n` and rebuilds the indent string
    /// [soft 0x083dc75c SI_setIndent]: a newline followed by three spaces per level. The
    /// level does not go below zero. Called with 0 it only rebuilds the string, which every
    /// section does first.
    pub fn set_indent(&mut self, n: i32) {
        self.indent += n;
        if self.indent <= 0 {
            self.indent = 0;
        }
        self.indent_string = vec![b'\n'];
        self.indent_string.resize(1 + 3 * self.indent as usize, b' ');
    }

    /// `fprintf(f, "%s", SI_indentString)`: a new line, indented.
    pub fn nl(&mut self) {
        self.buf.extend_from_slice(&self.indent_string);
    }

    pub fn raw(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// `fprintf(f, "%s ", word)`
    pub fn word(&mut self, w: &str) {
        self.buf.extend_from_slice(w.as_bytes());
        self.buf.push(b' ');
    }

    /// A new line and a keyword.
    pub fn key(&mut self, w: &str) {
        self.nl();
        self.word(w);
    }

    /// `fprintf(f, "%d ", v)`
    pub fn int(&mut self, v: i32) {
        self.buf.extend_from_slice(v.to_string().as_bytes());
        self.buf.push(b' ');
    }

    /// `fprintf(f, "%.7g ", v)`: the single is widened to double on the way
    /// (also [soft 0x083dc810 SI_valueAsciiWrite] for a value with no function curve).
    pub fn g(&mut self, v: f32) {
        self.g64(v as f64);
    }

    pub fn g64(&mut self, v: f64) {
        self.buf.extend_from_slice(cnum::fmt_g(v, 7).as_bytes());
        self.buf.push(b' ');
    }

    /// Three values [soft 0x083dc954 SI_value3AsciiWrite].
    pub fn g3(&mut self, v: &[f32; 3]) {
        for &x in v {
            self.g(x);
        }
    }

    /// `fprintf(f, "'%s' ", s)`. For a null pointer the C library prints `(null)`, so that is
    /// what the file gets, and what the next read takes for the string.
    pub fn quoted(&mut self, s: Option<&[u8]>) {
        self.buf.push(b'\'');
        self.buf.extend_from_slice(s.unwrap_or(b"(null)"));
        self.buf.extend_from_slice(b"' ");
    }

    /// A vector on a line of its own, one level in [soft 0x083dcb7c SI_vectorAsciiWrite].
    pub fn vector(&mut self, v: &[f32; 3]) {
        self.set_indent(1);
        self.nl();
        self.g3(v);
        self.set_indent(-1);
    }

    /// One of two words.
    pub fn choice(&mut self, cond: bool, yes: &str, no: &str) {
        self.word(if cond { yes } else { no });
    }
}

/// Writes a setup file as the interactive program does.
pub fn write(setup: &Setup, options: &WriteOptions) -> Vec<u8> {
    write_with(setup, options, &Session::default())
}

/// Writes a setup file [soft 0x083be420 SI_setupFileSavePath]: the header, the sections in
/// their fixed order, and the word `EOF`. The file does not end with a newline.
///
/// Under the stand-alone renderer the show flags and the window layout are left out, unless
/// the save is an emergency one.
pub fn write_with(setup: &Setup, options: &WriteOptions, session: &Session) -> Vec<u8> {
    let mut o = Out::new(options.indent);
    si_file::write_asc_header(&mut o.buf, options.version, TAG_SSET);
    let interface = !session.standalone_renderer || session.emergency_saving;
    if interface {
        show_flag_write(&mut o, setup);
    }
    pref_menu_write(&mut o, setup);
    autosave_pref_write(&mut o, setup);
    mouse_dial_write(&mut o, setup, session);
    increment_pref_dial_write(&mut o, setup);
    duplicate_setup_write(&mut o, setup);
    schematic_pref_write(&mut o, setup);
    picture_format_write(&mut o, setup);
    shade_setup_glb_write(&mut o, &setup.shade_glb);
    sound_setup_write(&mut o, setup, options);
    if interface {
        layout::layout_write(&mut o, setup);
    }
    render::render_write(&mut o, setup);
    render::preview_write(&mut o, setup);
    dopesheet_pref_write(&mut o, setup);
    o.word("EOF");
    o.buf
}

/// [soft 0x083c2b10 showFlagWrite]: the state of the eighteen items of the Show menu, as
/// `SI_menuStateGet` (0x083a4384) finds it in the menu of the first module.
fn show_flag_write(o: &mut Out, s: &Setup) {
    o.set_indent(0);
    o.word("SHOW_FLAG");
    o.nl();
    o.set_indent(1);
    for (i, name) in SHOW_NAMES.iter().enumerate() {
        o.key(name);
        o.int(s.show.menu[i] as i32);
    }
    o.raw("\n");
    o.raw("\n");
    o.set_indent(-1);
}

/// [soft 0x083c2d2c prefMenuWrite] [soft 0x0812e0ec SI_elemdictGetSaveOptions]
fn pref_menu_write(o: &mut Out, s: &Setup) {
    let p = &s.pref;
    o.set_indent(0);
    o.word("PREFERENCE");
    o.set_indent(1);
    let first: [(&str, i32); 18] = [
        ("PREF_BUSY_CURSOR", p.busy_cursor as i32),
        ("PREF_CLASSICAL_SCALING", p.classical_scaling),
        ("PREF_UNIFORM_SCALING", p.uniform_scaling),
        ("PREF_MODE", p.mode),
        ("PREF_HIERARCHY_COMPENSATION", p.hierarchy_compensation as i32),
        ("PREF_ROTATION", p.rotation),
        ("PREF_TRANSLATION", p.translation),
        ("PREF_MANIPULATION", p.manipulation),
        ("PREF_MULTI_NODE", p.multi_node as i32),
        ("PREF_HIERARCHY_MANIPULATION", p.hierarchy_manipulation as i32),
        ("PREF_CONTINUOUS_ROTATION", p.continuous_rotation as i32),
        ("PREF_MODELLING_RELATION", p.modelling_relation as i32),
        ("PREF_IMAGE_LOCK", p.image_lock as i32),
        ("PREF_HIGHLIGHT_KEY", p.highlight_key as i32),
        ("PREF_CONFIRM_KEYEDIT", p.confirm_keyedit as i32),
        ("PREF_RIGHT_HANDED_SURFACE", p.right_handed_surface as i32),
        ("PREF_ZOOM_ON_CURSOR", p.zoom_on_cursor as i32),
        ("PREF_USERTRANSFO", p.user_transfo as i32),
    ];
    for (k, v) in first {
        o.key(k);
        o.int(v);
    }
    let rest: [(&str, i32); 12] = [
        ("SAVE_MAT_TXT_NOT_SEPARATELY", p.save_mat_txt_not_separately as i32),
        ("SAVE_NOT_RM_OLD_VERSIONS", p.save_not_rm_old_versions as i32),
        ("PREF_FCV_INTERP_TYPE", p.fcv_interp_type as i32),
        ("PREF_FCV_EDT_UPD_MODE", p.fcv_edt_upd_mode as i32),
        ("PREF_DOPE_SNAP_KEYS", p.dope_snap_keys as i32),
        ("PREF_SEQ_ACT_FCV", p.seq_act_fcv as i32),
        ("PREF_AUTO_CLUSTER_CTR", p.auto_cluster_ctr as i32),
        ("PREF_INVKIN_AUTO_CNS", p.invkin_auto_cns),
        ("PREF_SHOW_DIRECTION_CONSTRAINT", p.show_direction_constraint as i32),
        ("PREF_SHOW_CRITICAL_ZONE", p.show_critical_zone as i32),
        ("PREF_SWIFTKEYS_ALL_MODULES", p.swiftkeys_all_modules as i32),
        ("PREF_FIT_SLP_TO_FCV_UNI_SLP", p.fit_slp_to_fcv_uni_slp as i32),
    ];
    for (k, v) in rest {
        o.key(k);
        o.int(v);
    }
    o.nl();
    o.set_indent(-1);
    o.raw("\n");
    o.raw("\n");
}

/// [soft 0x083c3550 autosavePrefWrite]: the period in whole minutes.
fn autosave_pref_write(o: &mut Out, s: &Setup) {
    o.set_indent(0);
    o.word("PREFERENCE_AUTO_SAVE");
    o.set_indent(1);
    o.key("AUTO_SAVE_ACTIVE");
    o.choice(s.autosave.active == 1, "ACTIVE", "INACTIVE");
    o.key("AUTO_SAVE_PERIOD");
    o.int(s.autosave.period / 60);
    o.nl();
    o.set_indent(-1);
    o.raw("\n");
}

/// [soft 0x083c3674 mouseDialWrite]: nothing but an empty line when the mouse dialog does not
/// exist. Warp and displacement are read back from the dialog's text fields as numbers
/// (`SI_getTexedFloat`, taken to convert the text the reader put there) and printed as such.
fn mouse_dial_write(o: &mut Out, s: &Setup, session: &Session) {
    if session.mouse_dialog {
        o.set_indent(0);
        o.word("PREFERENCE_MOUSE");
        o.set_indent(1);
        o.key("DIAL_MOUSE_POS");
        o.int(s.mouse.pos as i32);
        o.key("DIAL_MOUSE_WARP");
        o.g(s.mouse.warp as f32);
        o.key("DIAL_MOUSE_DISP");
        o.g(s.mouse.disp as f32);
        o.key("DIAL_MOUSE_BELL");
        o.int(s.mouse.bell as i32);
        o.set_indent(-1);
    }
    o.raw("\n");
}

/// [soft 0x083c3818 incrementPrefDialWrite]
fn increment_pref_dial_write(o: &mut Out, s: &Setup) {
    o.raw("\n");
    o.set_indent(0);
    o.word("PREFERENCE_INCREMENTATION");
    o.set_indent(1);
    for (k, v) in [
        ("PREF_SCALING_INCREMENT", &s.increments.scaling),
        ("PREF_ROTATION_INCREMENT", &s.increments.rotation),
        ("PREF_TRANSLATION_INCREMENT", &s.increments.translation),
    ] {
        o.key(k);
        o.set_indent(1);
        o.nl();
        for &x in v {
            o.g(x);
        }
        o.set_indent(-1);
    }
    o.set_indent(-1);
    o.raw("\n");
}

/// [soft 0x083c3aa8 duplicateSetupWrite]
fn duplicate_setup_write(o: &mut Out, s: &Setup) {
    o.raw("\n");
    o.set_indent(0);
    o.word("DUPLICATE_SETUP");
    o.set_indent(1);
    for (i, (name, _)) in DUPLICATE_NAMES.iter().enumerate() {
        o.key(name);
        // all but the connection choice are shorts
        o.int(if i == DUP_CONNECTION_TO { s.duplicate.values[i] } else { s.duplicate.values[i] as i16 as i32 });
    }
    o.set_indent(-1);
    o.raw("\n");
}

/// [soft 0x083c4e2c schematicPrefWrite]
fn schematic_pref_write(o: &mut Out, s: &Setup) {
    let c = &s.schematic;
    o.raw("\n");
    o.set_indent(0);
    o.word("SCHEMATIC");
    o.set_indent(1);
    for (k, v) in [
        ("SCHEM_ORGANIZATION_TYPE", c.organization_type),
        ("SCHEM_REPARTITION_TYPE", c.repartition_type),
        ("SCHEM_SORT_TYPE", c.sort_type),
        ("SCHEM_REL_DISPLAY", c.rel_display),
        ("SCHEM_USE_DEF_COLORS", c.use_def_colors as i32),
        ("SCHEM_AUTO_SCROLL", c.auto_scroll as i32),
        ("SCHEM_PARALLEL_LINKS", c.parallel_links as i32),
        ("SCHEM_UPDATE", c.update as i32),
        ("SCHEM_PREFIX_OPTION", c.prefix_option),
        ("SCHEM_AUTO_CLEAN", c.auto_clean as i32),
        ("SCHEM_RECUP_EMPTIES", c.recup_empties as i32),
        ("SCHEM_GRID_LOCK", c.grid_lock as i32),
    ] {
        o.key(k);
        o.int(v);
    }
    o.nl();
    o.set_indent(-1);
    o.raw("\n");
}

/// [soft 0x083c42b8 pictureFormatWrite]. It lowers the indent level by one without having
/// raised it.
fn picture_format_write(o: &mut Out, s: &Setup) {
    let cam = s.layout.scene_camera();
    o.raw("\n");
    o.set_indent(0);
    o.word("PREFERENCE_PICT_FMT");
    o.int(cam.format);
    o.g(cam.format_aspect);
    o.g(cam.format_width);
    o.g(cam.format_height);
    o.set_indent(-1);
}

/// [soft 0x083c564c shadeSetupGlbWrite]
pub(crate) fn shade_setup_glb_write(o: &mut Out, g: &ShadeGlb) {
    o.raw("\n");
    o.set_indent(0);
    o.key("SHADE_SETUP_GLB");
    o.set_indent(1);
    o.key("BLENDING");
    o.choice(g.blending != 0, "ALPHA", "NONE");
    o.key("REPEAT");
    o.choice(g.repeat != 0, "GRID", "NONE");
    o.key("LOD");
    o.int(g.lod);
    o.key("TXT_DPY_GLOBAL");
    o.int(g.txt_dpy_global as i32);
    o.key("TXT_DPY_LOCAL");
    o.int(g.txt_dpy_local as i32);
    o.key("TXT_DPY_PREC");
    o.int(g.txt_dpy_prec);
    o.key("TXT_DPY_LOCAL_MODE");
    o.int(g.txt_dpy_local_mode);
    o.key("OPTIMISE_EDITING");
    o.int(g.optimise_editing as i32);
    o.key("TEXTURE_PXL_INTERPOLATION_MODE");
    match g.texture_pxl_interpolation_mode {
        0 => o.word("RESPECT_TXT_PROPERTY"),
        1 => o.word("ALWAYS_ON"),
        2 => o.word("ALWAYS_OFF"),
        _ => {}
    }
    o.set_indent(-1);
}

/// [soft 0x083c5ae4 soundSetupWrite], from the tracks as [`Audio`] holds them: nothing at all
/// when there is none. In the original the levels are bits of the track's flags, tested low
/// first, then medium, with high as the fallback; the start offset comes from the manager.
fn sound_setup_write(o: &mut Out, s: &Setup, options: &WriteOptions) {
    if !s.audio.tracks.is_empty() {
        o.raw("\n");
        o.set_indent(0);
        o.key("AUDIOCONTAINER");
    }
    let level = |l: Level, names: [&'static str; 3]| match l {
        Level::Low => names[0],
        Level::Medium => names[1],
        Level::High | Level::Unset => names[2],
    };
    const LMH: [&str; 3] = ["LOW", "MEDIUM", "HIGH"];
    for t in &s.audio.tracks {
        o.set_indent(1);
        o.key("AUDIOTRACK");
        o.quoted(t.name.as_deref());
        o.quoted(t.path.as_deref());
        o.choice(t.active, "ACTIVE", "INACTIVE");
        o.set_indent(1);
        o.key("STARTOFFSET");
        o.g(t.start_offset);
        o.int(t.start_offset_frame);
        o.key("PREPOSTROLL");
        o.int(t.pre_roll);
        o.int(t.post_roll);
        o.key("WAVEDISPLAYQUALITY");
        o.word(level(t.wave_display_quality, LMH));
        o.int(t.wave_display_value);
        o.key("PBBUFFERING");
        o.word(level(t.pb_buffering, LMH));
        o.int(t.pb_buffering_value);
        o.key("TRACKHEIGHT");
        o.word(level(t.track_height, LMH));
        o.key("DISPLAYMARKERFCV");
        o.word(level(t.display_marker_fcv, ["NONE", "ACTIVE", "ALL"]));
        if options.audio_volume {
            o.key("VOLUME");
            o.g(t.volume);
        }
        for m in &t.markers {
            o.key("AUDIOMARKER");
            o.quoted(m.name.as_deref());
            o.g(m.start);
            o.g(m.end);
            o.choice(m.active, "ACTIVE", "INACTIVE");
        }
        o.set_indent(-1);
        o.set_indent(-1);
    }
}

/// [soft 0x083c5960 dopesheetPrefWrite]
fn dopesheet_pref_write(o: &mut Out, s: &Setup) {
    let d = &s.dopesheet;
    o.raw("\n");
    o.set_indent(0);
    o.word("DOPESHEET");
    o.set_indent(1);
    o.key("DS_RIPPLING");
    o.int(d.rippling as i32);
    o.key("DS_DISPLAY_ACTIONS");
    o.int(d.display_actions as i32);
    o.key("DS_ANCHOR_KEYS");
    o.int(d.anchor_keys as i32);
    o.key("DS_AFFECT_KEYS");
    o.int(d.affect_keys as i32);
    o.nl();
    o.set_indent(-1);
    o.raw("\n");
}

#[path = "setup_check.rs"]
pub mod check;

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
