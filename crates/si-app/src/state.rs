// SPDX-License-Identifier: MIT
//! The program's top-level state: the globals the start-up code, the main loop and the
//! services of this crate read and write, as one typed record. Each field names the
//! original's symbol and address (Linux `soft`), and holds the value the binary's data
//! segment starts with.

use crate::env::{Bytes, Handle, World};
use crate::globals::{AnimationInfo, DuplicationInfo, IncrementInfo, ManipulationInfo, SchematicInfo};
use crate::render_cmd::RenderControl;

/// One entry of `SI_network` (0x086cad78 points at five of them, 0x10 bytes each): a module
/// of the program as the "server" sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetEntry {
    /// +0x00: 0 until the module's canvas has been activated once.
    pub started: i16,
    /// +0x02: what the module's client returned last.
    pub state: i16,
    /// +0x04: the module: 0 Model, 1 Motion, 2 Actor, 3 Matter, 4 Tools.
    pub module: i16,
    /// +0x08: the module's canvas (`SI_Canvas *`).
    pub canvas: Handle,
    /// +0x0c: not 0 when the module cannot be chosen.
    pub disabled: i16,
}

/// An event of the program (`SI_Event`, 0x18 bytes: [soft 0x08393bc4 newEvent] allocates
/// that much).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// +0x00 the kind (`SI_EventType`): see [`crate::event`].
    pub kind: i32,
    /// +0x04, +0x06, +0x08, +0x0a: four shorts.
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
    /// +0x0c: a pointer (the window an event belongs to; a text for a posted event).
    pub data: u32,
    /// +0x10: not written by the routines of this crate.
    pub word10: u32,
    /// +0x14: the device `qread` returned.
    pub device: i32,
}

/// Which event record a routine handed back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRef {
    /// `SI_activeEvent` (0x087817fc): the record of the event read from the queue.
    Active,
    /// `dEvt` (0x08815a10): the copy of a posted event.
    Posted,
}

/// The state of the event reader (the user interface's `event.c`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventState {
    /// `qE` (0x08815920): up to ten posted events, the newest first.
    pub posted: [Event; 10],
    /// `nbE` (0x086be9a4): how many are posted.
    pub posted_count: i16,
    /// `isE` (0x086be9a2): not 0 while there is a posted event.
    pub is_posted: i16,
    /// `dEvt` (0x08815a10): the posted event handed out last.
    pub taken: Event,
    /// `*SI_activeEvent` (0x087817fc).
    pub active: Event,
    /// `swiftmode` (0x086be9a8): −1, or the quick-key mode whose key sequence stands for
    /// the next event.
    pub swiftmode: i16,
    /// `doubleClick` (0x086be9a0).
    pub double_click: i16,
    /// `gbRedrawRotoscopy` (0x086ba860).
    pub redraw_rotoscopy: i16,
    /// `gn3DRedrawsNeeded` (0x086ba864).
    pub redraws_3d: i16,
    /// `SI_supraMode` (0x08679404).
    pub supra_mode: i32,
    /// `SI_deviceUse` (0x08817758).
    pub device_use: i32,
    /// `savedKeywarp` and `keywarp`: the key repeat settings saved once.
    pub saved_keywarp: i16,
    pub keywarp: [i16; 2],
    /// `supraKeys`: the devices of the supra keys, ended by 0.
    pub supra_keys: Vec<i16>,
    /// `SI_buttonLeft`, `SI_buttonMiddle`, `SI_buttonRight`, `SI_coordx`, `SI_coordy`: the
    /// IRIS GL devices of the mouse (103, 102, 101, 266, 267 on a stock set-up).
    pub button_left: i16,
    pub button_middle: i16,
    pub button_right: i16,
    pub coord_x: i16,
    pub coord_y: i16,
}

impl Default for EventState {
    fn default() -> EventState {
        EventState {
            posted: [Event::default(); 10],
            posted_count: 0,
            is_posted: 0,
            taken: Event::default(),
            active: Event::default(),
            swiftmode: -1,
            double_click: 0,
            redraw_rotoscopy: 0,
            redraws_3d: 0,
            supra_mode: 0,
            device_use: 0,
            saved_keywarp: 0,
            keywarp: [0; 2],
            supra_keys: Vec::new(),
            button_left: 103,
            button_middle: 102,
            button_right: 101,
            coord_x: 266,
            coord_y: 267,
        }
    }
}

/// `SI_procData` (0x08679314 points at it): three words that every "process" of the
/// program has of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcData {
    /// +0x00: which element dictionary (index into `SI_elemDicts`, 0x70 bytes each).
    pub dictionary: i32,
    /// +0x04: which graph list (index into `SI_grafLists`, 0x24 bytes each).
    pub graf_list: i32,
    /// +0x08: the scene's camera.
    pub camera: Handle,
}

/// Where the error service sends its text (`SI_ErrorOutput`).
pub const ERROR_TO_STDOUT: i32 = 0;
pub const ERROR_TO_STDERR: i32 = 1;
pub const ERROR_TO_HANDLER: i32 = 2;

/// The state of the error service (`SI_error.c`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErrorState {
    /// `si_errorFilter` (0x086852a8): errors of a lower type are dropped.
    pub filter: i32,
    /// `si_errorOutput` (0x0868716c).
    pub output: i32,
    /// `si_errorHandler` (0x08687168): a function pointer, kept as a number.
    pub handler: u32,
}

/// The state of the log (`log.c`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogState {
    /// `needToLog` (0x086cf240): the value of `SI_LOG`, if it was set.
    pub need_to_log: Option<Bytes>,
    /// `file` (0x086cf244): the name of the open log file.
    pub file: Option<Bytes>,
    /// `checkNeedToLog` (0x086cf248): starts as −1.
    pub check_need_to_log: i32,
    /// `logToScreen`, `logMouse`, `logKeyboard` (0x086cf24c, …4e, …50).
    pub to_screen: i16,
    pub mouse: i16,
    pub keyboard: i16,
}

impl Default for LogState {
    fn default() -> LogState {
        LogState { need_to_log: None, file: None, check_need_to_log: -1, to_screen: 0, mouse: 0, keyboard: 0 }
    }
}

/// How the crash dialogue is shown (`SI_CrashUiModes`).
pub const CRASH_UI_OWN: i32 = 0;
pub const CRASH_UI_XCONFIRM: i32 = 1;
pub const CRASH_UI_NONE: i32 = 2;

/// `emergencyDetails` (0x08810880, `SI_EmergencySave`, 0x5040 bytes) and the statics of
/// `emergencySave.c`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmergencySave {
    /// +0x0000: emergency save is on.
    pub enabled: i16,
    /// +0x0004: a database record (`SI_DbInfo`) for the save; +0x000c of the structure is
    /// its path, a copy of `save_dir`.
    pub db_path: Option<Bytes>,
    /// +0x002c: how many times the handler was entered.
    pub entered: i32,
    /// +0x0030: a save is under way.
    pub saving: i16,
    /// +0x0032: the directory asked for (4,095 bytes).
    pub wanted_dir: Bytes,
    /// +0x1031: the directory the scene will be saved in.
    pub save_dir: Bytes,
    /// +0x2030: the name of the crash log.
    pub crash_log: Bytes,
    /// +0x302f, +0x402e: the two sound files.
    pub sound_crash: Bytes,
    pub sound_saved: Bytes,
    /// +0x5030: the handler is installed (the original keeps the function pointer).
    pub handler: bool,
    /// +0x5034, +0x5036, +0x5038: `sfplay` and both sounds, `xconfirm`, `gclear` are there.
    pub have_sfplay: i16,
    pub have_xconfirm: i16,
    pub have_gclear: i16,
    /// +0x503c: always 3.
    pub word503c: i32,
    /// `preClear` (0x086be2c0), `playSoundCues` (0x086be2c2), `crashUIMode` (0x086be2c4).
    pub pre_clear: i16,
    pub play_sound_cues: i16,
    pub crash_ui_mode: i32,
    /// `immediateAbort.292` (0x086be5a2).
    pub immediate_abort: i16,
    /// `commandLine` (0x0880e880): the command line as one string.
    pub command_line: Bytes,
}

impl Default for EmergencySave {
    fn default() -> EmergencySave {
        EmergencySave {
            enabled: 0,
            db_path: None,
            entered: 0,
            saving: 0,
            wanted_dir: Vec::new(),
            save_dir: Vec::new(),
            crash_log: Vec::new(),
            sound_crash: Vec::new(),
            sound_saved: Vec::new(),
            handler: false,
            have_sfplay: 0,
            have_xconfirm: 0,
            have_gclear: 0,
            word503c: 0,
            pre_clear: 0,
            play_sound_cues: 1,
            crash_ui_mode: CRASH_UI_XCONFIRM,
            immediate_abort: 0,
            command_line: Vec::new(),
        }
    }
}

/// The globals.
#[derive(Clone, Debug, PartialEq)]
pub struct Globals {
    /// `programName` (0x087821c4): `argv[0]`.
    pub program_name: Bytes,
    /// `init_buf` (0x0867970f): seven bytes behind [`crate::sysmain::get_init_value`].
    pub init_buf: [u8; 7],
    /// `coredump` (0x08679720): option `-c`.
    pub coredump: i16,
    /// `coredump.237` (0x0867cc40): the value `SI_signalInit` remembers.
    pub signal_coredump: i16,
    /// `SI_foreground` (0x08781462): option `-f`.
    pub foreground: i16,
    /// `SI_baseLicense` (0x08781464): option `-B`. A product-tier flag: this crate stores
    /// it and never branches on it.
    pub base_licence: i16,
    /// `SI_lightLicense`, `SI_lightLicense2` (0x08679320, 0x08679322): option `-G`.
    /// Product-tier flags, stored only.
    pub light_licence: i16,
    pub light_licence2: i16,
    /// `SI_standaloneRenderer` (0x08679400): the program is the batch renderer.
    pub standalone_renderer: i16,
    /// `SI_initPoseCompensate` (0x087817a0).
    pub init_pose_compensate: i16,
    /// `SI_optionRotoscopy` (0x087815d0).
    pub option_rotoscopy: i16,
    /// `SI_cusStandalone` (0x086e49f0): option `-D`.
    pub cus_standalone: i16,
    /// `SI_noOverlayRefreshFix` (0x08679402).
    pub no_overlay_refresh_fix: i16,
    /// `SI_oglGdiSynch` (0x08679680): set from `SI_OGL_ASYNCH_MODE` by `systemInit`.
    pub ogl_gdi_synch: i16,
    /// `SI_username` (0x087817c8).
    pub username: Bytes,
    /// `SI_resdir` (0x08781628): the resource directory.
    pub resdir: Option<Bytes>,
    /// `SI_cwd` (0x087fba8c), `SI_homedir` (0x0878159c).
    pub cwd: Option<Bytes>,
    pub homedir: Option<Bytes>,
    /// `SI_objectPath` (0x086793b8), `SI_texturePath` (0x08679434), `SI_picturePath`
    /// (0x086793cc), `SI_materialLib` (0x08679328), `SI_textureLib` (0x08679430),
    /// `SI_paletteLib` (0x086793c8).
    pub object_path: Option<Bytes>,
    pub texture_path: Option<Bytes>,
    pub picture_path: Option<Bytes>,
    pub material_lib: Option<Bytes>,
    pub texture_lib: Option<Bytes>,
    pub palette_lib: Option<Bytes>,
    /// `SI_reflectionMatter` (0x08781600): the reflection picture's path without suffix.
    pub reflection_matter: Bytes,
    /// `SI_sysid` (0x0867940c): the main window's IRIS GL identifier.
    pub sysid: i32,
    /// `SI_tabletActive` (0x086793f8).
    pub tablet_active: i32,
    /// Screen and colour facts `SI_checkHardware` leaves: `SI_xmaxscreen` (0x08781794),
    /// `SI_ymaxscreen` (0x08781796), `SI_bitPlanes`, `SI_olbitPlanes` (0x087815b4),
    /// `SI_systemWM` (0x087817ac), `SI_background`, the fonts `SI_appf` (0x08781644),
    /// `SI_irisf` (0x08781790), `SI_logof` (0x08781792), and `SI_frameCurrent`
    /// (0x08782040).
    pub xmaxscreen: i16,
    pub ymaxscreen: i16,
    pub bit_planes: i32,
    pub olbit_planes: i32,
    pub system_wm: u16,
    pub background: i32,
    pub appf: i16,
    pub irisf: i16,
    pub logof: i16,
    pub frame_current: i32,
    /// `SI_oldmouse1`, `SI_oldmouse2` (0x087815b8, …bc): the pointer's speed settings
    /// before the program changed them.
    pub oldmouse: [i32; 2],
    /// `SI_network` (0x086cad78), `SI_active` (0x08679480), `moduleRedraw` (0x087821c0),
    /// `SI_activeCanvas` (0x087817f4).
    pub network: [NetEntry; 5],
    pub active: i16,
    pub module_redraw: i16,
    pub active_canvas: Handle,
    /// `first_time.243` (0x086c72e4) of `SI_clientActivate`; `safetyNet` (0x086c72e0) of
    /// `SI_serverExit`; `SI_serverBack` (0x0878163c) and the two highlight colours it is
    /// copied to.
    pub first_time: i16,
    pub safety_net: i16,
    pub server_back: i16,
    pub button_back_hilite: [i16; 2],
    /// `SI_gotcha` (0x08679538): a module client keeps its event instead of reading one.
    pub gotcha: i16,
    /// `modelX`, `motionX`, `actorX`, `matterX`, `toolsX`, `serverY`: where the module
    /// names are on the canvas.
    pub module_x: [i16; 5],
    pub server_y: i16,
    pub proc_data: ProcData,
    pub events: EventState,
    pub error: ErrorState,
    pub log: LogState,
    pub emergency: EmergencySave,
    /// `inProgressDial` (0x08685254).
    pub in_progress_dial: Handle,
    /// `SI_historyActive` (0x0867965c).
    pub history_active: i16,
    /// `SI_defaultControl` (0x08781800) and `SI_ren` (0x08790be0): the render set-up and
    /// the copy a render works on.
    pub default_control: RenderControl,
    pub ren: RenderControl,
    /// `lineOption` (0x08679750), `verboseOption` (0x08679752) of the batch renderer.
    pub line_option: i16,
    pub verbose_option: i16,
    /// `SI_reportFrequency` (0x0869bc80).
    pub report_frequency: i16,
    /// `SI_cusUpdateState`.
    pub cus_update_state: i32,
    /// The swift-mouse library's four entry points, or 0.
    pub swift_mouse: [Handle; 4],
    /// `SI_lightIcon`, `SI_bulbIcon`, `SI_cameraIcon`.
    pub icons: [Handle; 3],
    pub manip: ManipulationInfo,
    pub anim: AnimationInfo,
    pub increm: IncrementInfo,
    pub duplic: DuplicationInfo,
    pub schem: SchematicInfo,
}

/// `init_buf` as the binary's data segment has it (0x0867970f) and `srv_buf` (0x08679717).
pub const INIT_BUF: [u8; 7] = [0x10, 0x2c, 0x0c, 0x1e, 0x41, 0x0f, 0x2a];
pub const SRV_BUF: [u8; 7] = [0x0d, 0x10, 0x0e, 0x0f, 0x0f, 0x14, 0x1e];

impl Default for Globals {
    fn default() -> Globals {
        Globals {
            program_name: Vec::new(),
            init_buf: INIT_BUF,
            coredump: 0,
            signal_coredump: 0,
            foreground: 0,
            base_licence: 0,
            light_licence: 0,
            light_licence2: 0,
            standalone_renderer: 0,
            init_pose_compensate: 0,
            option_rotoscopy: 0,
            cus_standalone: 0,
            no_overlay_refresh_fix: 0,
            ogl_gdi_synch: 0,
            username: Vec::new(),
            resdir: None,
            cwd: None,
            homedir: None,
            object_path: None,
            texture_path: None,
            picture_path: None,
            material_lib: None,
            texture_lib: None,
            palette_lib: None,
            reflection_matter: Vec::new(),
            sysid: 0,
            tablet_active: 0,
            xmaxscreen: 0,
            ymaxscreen: 0,
            bit_planes: 0,
            olbit_planes: 0,
            system_wm: 0,
            background: 0,
            appf: 0,
            irisf: 0,
            logof: 0,
            frame_current: 0,
            oldmouse: [0; 2],
            network: [NetEntry::default(); 5],
            active: 0,
            module_redraw: 0,
            active_canvas: 0,
            first_time: 1,
            safety_net: 0,
            server_back: 0,
            button_back_hilite: [0; 2],
            gotcha: 0,
            module_x: [0; 5],
            server_y: 0,
            proc_data: ProcData::default(),
            events: EventState::default(),
            error: ErrorState::default(),
            log: LogState::default(),
            emergency: EmergencySave::default(),
            in_progress_dial: 0,
            history_active: 0,
            default_control: RenderControl::default(),
            ren: RenderControl::default(),
            line_option: 0,
            verbose_option: 0,
            report_frequency: 0,
            cus_update_state: 0,
            swift_mouse: [0; 4],
            icons: [0; 3],
            manip: ManipulationInfo::default(),
            anim: AnimationInfo::default(),
            increm: IncrementInfo::default(),
            duplic: DuplicationInfo::default(),
            schem: SchematicInfo::default(),
        }
    }
}

/// The program: its globals and the world outside this crate.
pub struct App<W: World> {
    pub g: Globals,
    pub w: W,
}

impl<W: World> App<W> {
    /// The state a freshly loaded program has.
    pub fn new(world: W) -> App<W> {
        App { g: Globals::default(), w: world }
    }
}
