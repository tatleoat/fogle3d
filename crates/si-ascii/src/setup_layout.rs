// SPDX-License-Identifier: MIT
//! The window layout of a setup file: `WINDOW_LAYOUT` blocks, their cameras, memo cameras,
//! draw-over pictures, depth cue and shading set-up (reader in `asceneio.c`, writer in
//! `setup.c`; see the parent module).
//!
//! The program has four view windows. Each can show any of nine views (front, top, right,
//! perspective, function curves, schematic, dope sheet, spreadsheet and one unnamed), and
//! keeps a record per view — a "slot" — with that view's camera, grid, rulers, controller,
//! four memo cameras and shading properties. The window itself points at the records of the
//! view it shows and holds copies of a few of that slot's values. A setup file describes, per
//! window, the view shown and its numbers, plus the memo cameras of all its views.

use super::{c_float_to_int, Out, Reader, Session, Setup, SetupError};

/// The slots of a window, by the index `SI_camIdConvert` gives the camera's id.
pub const SLOT_FRONT: usize = 0;
pub const SLOT_TOP: usize = 1;
pub const SLOT_RIGHT: usize = 2;
pub const SLOT_PERSPECTIVE: usize = 3;
pub const SLOT_FCURVE: usize = 5;
pub const SLOT_SCHEMATIC: usize = 6;
pub const SLOT_DOPESHEET: usize = 7;
pub const SLOT_SPREADSHEET: usize = 8;

/// The id of the camera of each slot `SI_camIdConvert` (0x080d6f94, in part) (the function goes the
/// other way: id to slot; an id it does not know gives slot 0 and a line on stderr).
pub const CAMERA_IDS: [i16; 9] = [2000, 2001, 2002, 2003, 2009, 2004, 2007, 2008, 2010];

/// The windows in the order the program keeps them, which is the order they are written in.
pub const WINDOW_FRONT: usize = 0;
pub const WINDOW_TOP: usize = 1;
pub const WINDOW_RIGHT: usize = 2;
pub const WINDOW_PERSPECTIVE: usize = 3;

/// Window ids (the short at +0x04 of a window), in that order.
const WINDOW_IDS: [i16; 4] = [1000, 1001, 1002, 1003];
const WINDOW_NAMES: [&str; 4] = ["L0_FRONT", "L0_TOP", "L0_RIGHT", "L0_PERSPECTIVE"];

/// The scene camera, the one the renderer looks through and whose record also holds the
/// picture format (`SI_procData` + 8 points at it): window and slot. The writer leaves the
/// position and interest of this camera out of the layout, since they belong to the scene.
///
/// That it is the perspective view of the perspective window is read from the files (every
/// one of them omits the position there and nowhere else), not from the code that sets the
/// pointer.
pub const SCENE_CAMERA: (usize, usize) = (WINDOW_PERSPECTIVE, SLOT_PERSPECTIVE);

/// A view's camera (`SI_Camera`, 0x13c bytes; the fields a setup file touches).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Camera {
    /// +0x04: which view this is, one of [`CAMERA_IDS`].
    pub id: i16,
    /// +0x06: for the schematic view, what it shows: 1 lights, 2 models, 4 motion, 8 matter;
    /// 0 is the plain schematic (`SCHEMATIC_MODES`).
    pub schematic_modes: i16,
    /// +0x10 `CAMERA_POSITION`
    pub position: [f32; 3],
    /// +0x1c `CAMERA_INTEREST`
    pub interest: [f32; 3],
    /// +0x34 `CAMERA_NEAR`
    pub near: f32,
    /// +0x38 `CAMERA_FAR`
    pub far: f32,
    /// +0x40 `CAMERA_FOV`
    pub fov: f32,
    /// +0x54: the picture format, an index into [`super::FORMAT_NAMES`] (scene camera only:
    /// `PREFERENCE_PICT_FMT`, `FORMAT_TYPE`).
    pub format: i32,
    /// +0x58: the aspect ratio of the custom-aspect format (`ASPECT`).
    pub format_aspect: f32,
    /// +0x5c, +0x60: the size of the custom-size format (`WIDTH`, `HEIGHT`).
    pub format_width: f32,
    pub format_height: f32,
    /// +0x64 `CAMERA_ASPECT`. For the scene camera the picture-format readers overwrite it
    /// with the aspect ratio of the format.
    pub aspect: f32,
    /// `MINIMUM_VIEW` and `MAXIMUM_VIEW`, two corners of a box the view is to show. The
    /// original turns them into a position, an interest and a field of view at once
    /// (`SI_setCamView`, 0x080d6048); that function is not reproduced, so the box is kept
    /// here as given and the camera is left as it was.
    pub view_box: Option<([f32; 3], [f32; 3])>,
}

/// Grid, axes and snapping of a view (`SI_GridDef`, 0x30 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewOptions {
    /// +0x00 `GRID_VISIBLE`
    pub grid_visible: i16,
    /// +0x02 `AXIS_VISIBLE`
    pub axis_visible: i16,
    /// +0x04 `RULER_VISIBLE`
    pub ruler_visible: i16,
    /// +0x06 `FIELD_VISIBLE`
    pub field_visible: i16,
    /// +0x08, +0x0a, +0x0c `X_AXIS_LOCK`, `Y_AXIS_LOCK`, `Z_AXIS_LOCK`
    pub axis_lock: [i16; 3],
    /// +0x10 `DELTA`
    pub delta: [f32; 3],
    /// +0x1c `DISPLAY`
    pub display: [f32; 3],
    /// +0x28 `MAGNET`
    pub magnet: i32,
    /// +0x2c `MAGNET_TYPE`
    pub magnet_type: i16,
}

/// The camera controller of a view (`SI_Ctrlcam`).
#[derive(Clone, Debug, PartialEq)]
pub struct Controller {
    /// +0x00 `CONTROLER`
    pub active: i16,
    /// +0x04 `CONTROLER_STEP`
    pub step: f32,
}

impl Default for Controller {
    /// `SI_ctrlCamInit` (0x08400498, in part), the two fields above.
    fn default() -> Self {
        Controller { active: 0, step: 0.0 }
    }
}

/// A remembered camera (0x34 bytes). Every view has four.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoCamera {
    /// +0x00 `ACTIVE` or `INACTIVE`
    pub active: i16,
    /// +0x08 `MC_CAM_POSITION`
    pub position: [f32; 3],
    /// +0x14 `MC_CAM_INTEREST`
    pub interest: [f32; 3],
    /// +0x20 `MC_CAM_ROLL`
    pub roll: f32,
    /// +0x24 `MC_CAM_FOV`
    pub fov: f32,
    /// +0x28 `MC_CAM_ANGLE`
    pub angle: f32,
    /// +0x2c, +0x30 `MC_CAM_CLIP`: near and far by their place; no code read names them.
    pub clip: [f32; 2],
}

/// The picture or movie drawn over a view (0x60 bytes; at +0x70 of the window and at +0x24
/// of each slot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawOver {
    /// +0x00: `DO_DRAW_OVER`, and the older `PICTURE`.
    pub name: Option<Vec<u8>>,
    /// +0x04: `DO_SEQUENCE` `YES`/`NO`; the first number of the older `PICTURE_FRAME`.
    pub sequence: i16,
    /// +0x08, +0x0c, +0x10, +0x14, +0x18: the numbers after `DO_SEQUENCE` fill the first
    /// three and the last; `PICTURE_FRAME` fills all five (in the order +0x08, +0x0c, +0x14,
    /// +0x10, +0x18). What each means was not read from code.
    pub frames: [i32; 5],
    /// +0x50 `DO_OPTION`
    pub option: i16,
    /// +0x52 `DO_USEMOVIE`
    pub use_movie: i16,
    /// +0x54 `DO_MOVIENAME`
    pub movie_name: Option<Vec<u8>>,
    /// +0x58, +0x5c: cleared whenever a name is read.
    pub loaded: [i32; 2],
}

/// Depth cue of a window (+0x270 … +0x28c). Apart from the switch the names are guesses from
/// the values in the files (two depths in z-buffer units, three shorts that look like a
/// colour).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DepthCue {
    /// +0x270: first number of `DC_DEPTHCUE`.
    pub active: i16,
    /// +0x274 `DC_RANGE`
    pub range: i32,
    /// +0x278, +0x27c
    pub near: i32,
    pub far: i32,
    /// +0x288, +0x28a, +0x28c
    pub colour: [i16; 3],
}

/// Display properties of a shaded view (`SI_GraphicProps`, 0xa0 bytes; the fields a setup
/// file touches): the `TEXTURE` … `GHOST_FOLLOW_SELECTION` lines.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadeSetup {
    /// +0x00 `SHOW_ICON`
    pub show_icon: i16,
    /// +0x02 `ANTIALIAS`
    pub antialias: i16,
    /// +0x04 `SHOW_GMATTE`
    pub show_gmatte: i16,
    /// +0x0c `OPTIMISATION`: 0 `NONE`, 1 `SHADE`, 2 anything else (written `WIRE`).
    pub optimisation: i32,
    /// +0x10 `TEXTURE`
    pub texture: i16,
    /// +0x12 `AFFECTED_MODEL`: 1 `SELECTED`, 0 anything else (written `ALL`).
    pub affected_model: i16,
    /// +0x8c `HIGHLIGHT_WIDTH`
    pub highlight_width: f32,
    /// +0x90 `SHADED_GHOSTS`
    pub shaded_ghosts: i16,
    /// +0x92 `PSEUDO_WIRE`
    pub pseudo_wire: i16,
    /// +0x98 `PSEUDO_WIRE_MODE`
    pub pseudo_wire_mode: i32,
    /// +0x9c `UNSELECTED_HIGHLIGHTS`
    pub unselected_highlights: i16,
    /// +0x9e `GHOST_FOLLOW_SELECTION`
    pub ghost_follow_selection: i16,
}

impl Default for ShadeSetup {
    /// `SI_GpropsInit` (0x085f15f0, in part), the fields above, with `SI_SHADE_HIGHLIGHT_WIDTH`
    /// not set in the environment (it would replace the highlight width).
    fn default() -> Self {
        ShadeSetup {
            show_icon: 0,
            antialias: 0,
            show_gmatte: 0,
            optimisation: 2,
            texture: 0,
            affected_model: 1,
            highlight_width: 1.0,
            shaded_ghosts: 0,
            pseudo_wire: 0,
            pseudo_wire_mode: 0,
            unselected_highlights: 0,
            ghost_follow_selection: 0,
        }
    }
}

impl ShadeSetup {
    /// The hardware renderer's properties as they start `si_initRenderGprops` (0x082a5920, in part):
    /// the general defaults, then textures on (unless `SI_GL_RENDER_TXT_OFF` is set) and all
    /// models affected.
    pub fn for_renderer() -> Self {
        ShadeSetup { texture: 1, affected_model: 0, ..ShadeSetup::default() }
    }
}

/// What a window keeps per view (0x8c bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    /// +0x00 (pointer)
    pub camera: Camera,
    /// +0x04 (pointer)
    pub view: ViewOptions,
    /// +0x08, +0x0c (pointers): the "active" short of the view's two rulers, which
    /// `RULER_VISIBLE` also sets `SI_rulerSetActive` (0x083aca68, in part).
    pub rulers_active: [i16; 2],
    /// +0x10 (pointer)
    pub controller: Controller,
    /// +0x14 (pointer to four)
    pub memo: [MemoCamera; 4],
    /// +0x18: the window size wanted for this view (see [`Window::size`]).
    pub size: i32,
    /// +0x1c, +0x20: display type.
    pub display_type: [i32; 2],
    /// +0x24
    pub draw_over: DrawOver,
    /// +0x88 (pointer)
    pub shade: ShadeSetup,
}

/// The spreadsheet of a window showing one (+0x2ac). How it comes to exist was not read
/// (`SI_spdRestoreSprdsheetByName`, 0x085ab400): no window has one here unless the caller
/// provides it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spreadsheet {
    /// +0x00 of the record: its name, written after `SPREADSHEET_SETUP`.
    pub name: Option<Vec<u8>>,
    /// +0x70 `SPD_UPDATE_SCENE`
    pub update_scene: i16,
    /// +0x72 `SPD_UPDATE_SPREADSHEET`
    pub update_spreadsheet: i16,
}

/// One of the four view windows (`__SI_Grafport`, 0x2b8 bytes; the fields a setup file
/// touches).
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// +0x04
    pub id: i16,
    /// +0x0c `ZOOM`
    pub zoom: i16,
    /// +0x10 `WINDOW_SIZE`: 0 normal, 1 large, 2 tall. A copy of the current slot's.
    pub size: i32,
    /// The slot whose camera, grid, rulers, controller, memo cameras and shading properties
    /// the window points at (+0x2c, +0x34, +0x48, +0x4c, +0x294, +0x298, +0x2a0).
    pub current: usize,
    /// +0x5c: the slot shown before the last change of view.
    pub previous: i16,
    /// +0x64, +0x68 `DISPLAY_TYPE` (both get the number; the first is written). Copies of
    /// the current slot's, taken when the view changes.
    pub display_type: [i32; 2],
    /// +0x70: copy of the current slot's, likewise.
    pub draw_over: DrawOver,
    /// +0x25c `FAST_PLAY`
    pub fast_play: i16,
    /// +0x270
    pub depth_cue: DepthCue,
    /// +0x280, +0x284: copies of the camera's near and far planes, set with them.
    pub near: f32,
    pub far: f32,
    /// The schematic's mode menu (+0x29c): the state of its four items (lights, models,
    /// motion, matter), which the reader raises `SI_setMenuFlag` (0x083a45ec, in part), and the
    /// item touched last.
    pub schematic_menu: [i16; 4],
    pub schematic_menu_last: i16,
    /// +0x2ac
    pub spreadsheet: Option<Spreadsheet>,
    /// The name given after `SPREADSHEET_SETUP`, which the original hands to
    /// `SI_spdRestoreSprdsheetByName` (not reproduced).
    pub spreadsheet_wanted: Option<Vec<u8>>,
    /// +0x30
    pub slots: [Slot; 9],
}

impl Window {
    /// A window as the window template reader (`windowReadFile`, 0x083d215c) leaves it before
    /// it reads the template's own numbers: every slot with its own zeroed records, the
    /// second display type 1, the function-curve and spreadsheet views large, memo cameras
    /// and controller initialised, shading properties at their defaults; window `index`
    /// shows view `index`. The cameras, grids and rulers then come from the template file,
    /// which is not read here: they stay zero.
    fn new(index: usize) -> Self {
        let mut slots: [Slot; 9] = Default::default();
        for (k, s) in slots.iter_mut().enumerate() {
            s.camera.id = CAMERA_IDS[k];
            s.display_type = [0, 1];
        }
        slots[SLOT_FCURVE].size = 1;
        slots[SLOT_SPREADSHEET].size = 1;
        let mut w = Window {
            id: WINDOW_IDS[index],
            zoom: 0,
            size: 0,
            current: index,
            previous: 0,
            display_type: [0, 0],
            draw_over: DrawOver::default(),
            fast_play: 0,
            depth_cue: DepthCue::default(),
            near: 0.0,
            far: 0.0,
            schematic_menu: [0; 4],
            schematic_menu_last: 0,
            spreadsheet: None,
            spreadsheet_wanted: None,
            slots,
        };
        // the template reader ends by making slot `index` current (with no camera before it,
        // the "previous" slot is recorded as 6)
        w.set_current(index);
        w.previous = 6;
        w
    }

    /// Makes `slot` the view shown `SI_setCurrentCam` (0x083d4180, in part): the window points at
    /// that slot's records and takes copies of its display type, draw-over record and size.
    /// Nothing is copied back to the slot that was shown. (The window's title, taken from
    /// the view menu, is not kept.)
    pub fn set_current(&mut self, slot: usize) {
        self.previous = self.current as i16;
        self.current = slot;
        let s = &self.slots[slot];
        self.display_type = s.display_type;
        self.draw_over = s.draw_over.clone();
        self.size = s.size;
    }

    pub fn camera(&self) -> &Camera {
        &self.slots[self.current].camera
    }

    fn cur(&mut self) -> &mut Slot {
        &mut self.slots[self.current]
    }
}

/// The four windows.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub windows: [Window; 4],
}

impl Default for Layout {
    fn default() -> Self {
        Layout { windows: [Window::new(0), Window::new(1), Window::new(2), Window::new(3)] }
    }
}

impl Layout {
    pub fn scene_camera(&self) -> &Camera {
        &self.windows[SCENE_CAMERA.0].slots[SCENE_CAMERA.1].camera
    }

    pub fn scene_camera_mut(&mut self) -> &mut Camera {
        &mut self.windows[SCENE_CAMERA.0].slots[SCENE_CAMERA.1].camera
    }
}

// ------------------------------------------------------------------------------------------
// reader
// ------------------------------------------------------------------------------------------

/// The window a `WINDOW_TYPE` name stands for `si_getLayoutWindow` (0x083f90c8, in part): the
/// reader asks for 1, 3, 5 or 7 and gets the window whose id is 1000, 1002, 1001 or 1003.
/// (In the original the answer is null if the window list has no such window. Here all four
/// always exist.)
fn layout_window(code: i32) -> Option<usize> {
    match code {
        1 => Some(WINDOW_FRONT),
        3 => Some(WINDOW_RIGHT),
        5 => Some(WINDOW_TOP),
        7 => Some(WINDOW_PERSPECTIVE),
        _ => None,
    }
}

/// `YES` is 1, anything else 0.
fn yes(r: &mut Reader) -> i16 {
    r.next_is("YES") as i16
}

/// One `WINDOW_LAYOUT` block [soft 0x083de590 layoutAsciiRead].
///
/// `WINDOW_TYPE` chooses the window, `CAMERA_TYPE` the view it shows, and the lines after go
/// to that window and to the records of that view. After `MEMO_CAMERA`, each `MC_…` word
/// starts the four memo cameras of one view. The block ends at the first word that is none
/// of these.
///
/// A block for a window the program does not have (`L1_…`, a second layout of older
/// versions) is read and dropped, each keyword consuming what it would have stored. Four
/// keywords do not check for that and make the original crash: `CONTROLER`,
/// `CONTROLER_STEP`, `TEXTURE` and the two `SPD_…` ones; so does a memo camera.
pub(crate) fn layout_read(r: &mut Reader, s: &mut Setup) -> Result<(), SetupError> {
    let session: &Session = r.session;
    let layout = &mut s.layout;
    let mut win: Option<usize> = None;
    let mut first = true;
    let mut memo = false;
    let mut have_token = false;
    // locals of the original that outlive one keyword; uninitialised there until
    // MINIMUM_VIEW has been seen
    let mut view_min = [0.0f32; 3];
    r.in_token();
    loop {
        if r.is("WINDOW_TYPE") {
            r.in_token();
            if session.standalone_renderer {
                win = None;
            } else if r.is("L0_FRONT") {
                win = layout_window(1);
            } else if r.is("L0_RIGHT") {
                win = layout_window(3);
            } else if r.is("L0_TOP") {
                win = layout_window(5);
            } else if r.is("L0_PERSPECTIVE") {
                win = layout_window(7);
            } else if r.is("L1_TOP") || r.is("L1_FRONT") || r.is("L1_RIGHT") || r.is("L1_PERSPECTIVE") {
                win = None;
            }
            // any other name: the window chosen earlier in this block stays, and is reset again
            if let Some(w) = win {
                let w = &mut layout.windows[w];
                w.cur().view.axis_lock = [0; 3];
                w.display_type = [0, 0];
                w.cur().display_type = [0, 0];
            }
        } else if r.is("CAMERA_TYPE") {
            r.in_token();
            if let Some(w) = win {
                let w = &mut layout.windows[w];
                let schematic = |w: &mut Window, modes: i16, item: Option<usize>| {
                    w.set_current(SLOT_SCHEMATIC);
                    w.cur().camera.schematic_modes = modes;
                    if let Some(i) = item {
                        w.schematic_menu[i] = 1;
                        w.schematic_menu_last = i as i16;
                    }
                };
                if r.is("FRONT") {
                    w.set_current(SLOT_FRONT);
                } else if r.is("RIGHT") {
                    w.set_current(SLOT_RIGHT);
                } else if r.is("TOP") {
                    w.set_current(SLOT_TOP);
                } else if r.is("PERSPECTIVE") {
                    w.set_current(SLOT_PERSPECTIVE);
                } else if r.is("FCURVE") {
                    w.set_current(SLOT_FCURVE);
                } else if r.is("SPREADSHEET") {
                    w.set_current(if session.light_license { SLOT_SCHEMATIC } else { SLOT_SPREADSHEET });
                } else if r.is("DOPESHEET") {
                    w.set_current(SLOT_DOPESHEET);
                } else if r.is("SCHEMATIC_PLAIN") || r.is("SCHEMATIC") {
                    schematic(w, 0, None);
                } else if r.is("SCHEMATIC_MODEL") {
                    schematic(w, 2, Some(1));
                } else if r.is("SCHEMATIC_MOTION") {
                    schematic(w, 4, Some(2));
                } else if r.is("SCHEMATIC_MATTER") {
                    schematic(w, 8, Some(3));
                }
            }
        } else if r.is("SCHEMATIC_MODES") {
            // words up to PLAIN; neither loop tests whether the file has ended
            r.in_token();
            if let Some(w) = win {
                let w = &mut layout.windows[w];
                w.cur().camera.schematic_modes = 0;
                while !r.is("PLAIN") {
                    let item = if r.is("LIGHT") {
                        Some(0)
                    } else if r.is("MODEL") {
                        Some(1)
                    } else if r.is("MOTION") {
                        Some(2)
                    } else if r.is("MATTER") {
                        Some(3)
                    } else {
                        None
                    };
                    if let Some(i) = item {
                        w.cur().camera.schematic_modes |= 1 << i;
                        w.schematic_menu[i] = 1;
                        w.schematic_menu_last = i as i16;
                    }
                    if !r.in_token() {
                        return Err(SetupError::SpinsForever { at: "SCHEMATIC_MODES" });
                    }
                }
            } else {
                while !r.is("PLAIN") {
                    if !r.in_token() {
                        return Err(SetupError::SpinsForever { at: "SCHEMATIC_MODES" });
                    }
                }
            }
        } else if r.is("SPREADSHEET_SETUP") {
            r.in_token();
            // handed to the spreadsheet module with the window, null or not
            if let Some(w) = win {
                layout.windows[w].spreadsheet_wanted = Some(r.token().to_vec());
            }
        } else if r.is("SPD_UPDATE_SCENE") || r.is("SPD_UPDATE_SPREADSHEET") {
            let scene = r.is("SPD_UPDATE_SCENE");
            let v = r.short();
            let Some(w) = win else {
                return Err(SetupError::NullWindow { keyword: if scene { "SPD_UPDATE_SCENE" } else { "SPD_UPDATE_SPREADSHEET" } });
            };
            if let Some(sp) = layout.windows[w].spreadsheet.as_mut() {
                if scene {
                    sp.update_scene = v;
                } else {
                    sp.update_spreadsheet = v;
                }
            }
        } else if r.is("CAMERA_POSITION") {
            r.vector_read(win.map(|w| &mut layout.windows[w].cur().camera.position));
        } else if r.is("CAMERA_INTEREST") {
            r.vector_read(win.map(|w| &mut layout.windows[w].cur().camera.interest));
        } else if r.is("CAMERA_FOV") {
            let v = r.float();
            if let Some(w) = win {
                layout.windows[w].cur().camera.fov = v;
            }
        } else if r.is("CAMERA_ASPECT") {
            let v = r.float();
            if let Some(w) = win {
                layout.windows[w].cur().camera.aspect = v;
            }
        } else if r.is("CAMERA_NEAR") {
            let v = r.float();
            if let Some(w) = win {
                layout.windows[w].near = v;
                layout.windows[w].cur().camera.near = v;
            }
        } else if r.is("CAMERA_FAR") {
            let v = r.float();
            if let Some(w) = win {
                layout.windows[w].far = v;
                layout.windows[w].cur().camera.far = v;
            }
        } else if r.is("MINIMUM_VIEW") {
            r.vector_read(Some(&mut view_min));
        } else if r.is("MAXIMUM_VIEW") {
            let mut view_max = [0.0f32; 3];
            r.vector_read(Some(&mut view_max));
            if let Some(w) = win {
                // SI_setCamView(camera, &min, &max, 0) in the original: not reproduced
                layout.windows[w].cur().camera.view_box = Some((view_min, view_max));
            }
        } else if r.is("ZOOM") {
            let v = yes(r);
            if let Some(w) = win {
                layout.windows[w].zoom = v;
            }
        } else if r.is("WINDOW_SIZE") {
            r.in_token();
            if let Some(w) = win {
                let size = if r.is("TALL_SIZE") {
                    2
                } else if r.is("LARGE_SIZE") {
                    1
                } else {
                    0
                };
                let w = &mut layout.windows[w];
                w.size = size;
                w.cur().size = size;
            }
        } else if r.is("GRID_VISIBLE") {
            let v = yes(r);
            if let Some(w) = win {
                layout.windows[w].cur().view.grid_visible = v;
            }
        } else if r.is("AXIS_VISIBLE") {
            let v = yes(r);
            if let Some(w) = win {
                layout.windows[w].cur().view.axis_visible = v;
            }
        } else if r.is("RULER_VISIBLE") {
            let v = yes(r);
            if let Some(w) = win {
                let slot = layout.windows[w].cur();
                slot.view.ruler_visible = v;
                slot.rulers_active = [v, v];
            }
        } else if r.is("FIELD_VISIBLE") {
            let v = yes(r);
            if let Some(w) = win {
                layout.windows[w].cur().view.field_visible = v;
            }
        } else if r.is("X_AXIS_LOCK") || r.is("Y_AXIS_LOCK") || r.is("Z_AXIS_LOCK") {
            let axis = (r.token()[0] - b'X') as usize;
            let v = yes(r);
            if let Some(w) = win {
                layout.windows[w].cur().view.axis_lock[axis] = v;
            }
        } else if r.is("DELTA") {
            r.vector_read(win.map(|w| &mut layout.windows[w].cur().view.delta));
        } else if r.is("DISPLAY") {
            r.vector_read(win.map(|w| &mut layout.windows[w].cur().view.display));
        } else if r.is("MAGNET") {
            let v = r.short();
            if let Some(w) = win {
                layout.windows[w].cur().view.magnet = v as i32;
            }
        } else if r.is("MAGNET_TYPE") {
            let v = r.short();
            if let Some(w) = win {
                layout.windows[w].cur().view.magnet_type = v;
            }
        } else if r.is("FAST_PLAY") {
            // read as a number, though the writer prints YES or NO: what comes back is 0
            let v = r.short();
            if let Some(w) = win {
                layout.windows[w].fast_play = v;
            }
        } else if r.is("DISPLAY_TYPE") {
            let v = r.short() as i32;
            if let Some(w) = win {
                layout.windows[w].display_type = [v, v];
            }
        } else if r.is("PICTURE") {
            let name = r.string_read();
            if let Some(w) = win {
                layout.windows[w].draw_over.name = name;
            }
        } else if r.is("PICTURE_FRAME") {
            let mut v = [0i16; 6];
            for x in v.iter_mut() {
                *x = r.short();
            }
            if let Some(w) = win {
                let d = &mut layout.windows[w].draw_over;
                d.sequence = v[0];
                d.frames[0] = v[1] as i32;
                d.frames[1] = v[2] as i32;
                d.frames[3] = v[3] as i32;
                d.frames[2] = v[4] as i32;
                d.frames[4] = v[5] as i32;
            }
        } else if r.is("DEPTH_CUE") {
            // the older form: two depths as reals, cut to ints, then four shorts
            let a = r.float();
            let b = r.float();
            let mut v = [0i16; 4];
            for x in v.iter_mut() {
                *x = r.short();
            }
            if let Some(w) = win {
                let d = &mut layout.windows[w].depth_cue;
                d.near = c_float_to_int(a);
                d.far = c_float_to_int(b);
                d.active = v[0];
                d.colour = [v[1], v[2], v[3]];
            }
        } else if r.is("DO_DRAW_OVER") {
            let name = r.string_read();
            if let Some(w) = win {
                let d = &mut layout.windows[w].draw_over;
                d.name = name;
                d.loaded = [0, 0];
            }
        } else if r.is("DO_SEQUENCE") {
            r.in_token();
            let seq = if r.is("YES") {
                Some(1)
            } else if r.is("NO") {
                Some(0)
            } else {
                None
            };
            let mut v = [0i16; 4];
            for x in v.iter_mut() {
                *x = r.short();
            }
            if let Some(w) = win {
                let d = &mut layout.windows[w].draw_over;
                if let Some(q) = seq {
                    d.sequence = q;
                }
                d.frames[0] = v[0] as i32;
                d.frames[1] = v[1] as i32;
                d.frames[2] = v[2] as i32;
                d.frames[4] = v[3] as i32;
            }
        } else if r.is("DO_MOVIENAME") {
            let name = r.string_read();
            if let Some(w) = win {
                let d = &mut layout.windows[w].draw_over;
                d.movie_name = name;
                d.loaded = [0, 0];
            }
        } else if r.is("DO_USEMOVIE") {
            r.in_token();
            if let Some(w) = win {
                if r.is("YES") {
                    layout.windows[w].draw_over.use_movie = 1;
                } else if r.is("NO") {
                    layout.windows[w].draw_over.use_movie = 0;
                }
            }
        } else if r.is("DO_OPTION") {
            let v = r.short();
            if let Some(w) = win {
                layout.windows[w].draw_over.option = v;
            }
        } else if r.is("DC_DEPTHCUE") {
            // with a window: a short, two ints, three shorts. Without one the two in the
            // middle are read as reals; the tokens consumed are the same.
            if let Some(w) = win {
                let d = &mut layout.windows[w].depth_cue;
                d.active = r.short();
                r.in_ivalue(&mut d.near);
                r.in_ivalue(&mut d.far);
                for x in d.colour.iter_mut() {
                    *x = r.short();
                }
            } else {
                r.short();
                r.float();
                r.float();
                r.short();
                r.short();
                r.short();
            }
        } else if r.is("DC_RANGE") {
            let v = r.short();
            if let Some(w) = win {
                layout.windows[w].depth_cue.range = v as i32;
            }
        } else if r.is("CONTROLER") {
            let v = r.next_is("ACTIVE") as i16;
            let Some(w) = win else { return Err(SetupError::NullWindow { keyword: "CONTROLER" }) };
            layout.windows[w].cur().controller.active = v;
        } else if r.is("CONTROLER_STEP") {
            let v = r.float();
            let Some(w) = win else { return Err(SetupError::NullWindow { keyword: "CONTROLER_STEP" }) };
            layout.windows[w].cur().controller.step = v;
        } else if r.is("TEXTURE") {
            let Some(w) = win else { return Err(SetupError::NullWindow { keyword: "TEXTURE" }) };
            shade_setup_read(r, Some(&mut layout.windows[w].cur().shade));
            have_token = true;
        } else if r.is("MEMO_CAMERA") {
            memo = true;
        } else {
            if !memo {
                if first {
                    r.in_token();
                }
                return Ok(());
            }
            match win {
                None => {
                    // Any word at all is taken for the start of memo cameras, and they go
                    // through a null pointer.
                    have_token = memo_cam_read(r, None)?;
                    if !have_token && r.bytes_left() == 0 {
                        return Err(SetupError::SpinsForever { at: "MEMO_CAMERA without a window" });
                    }
                }
                Some(w) => {
                    let slot = if r.is("MC_FRONT") {
                        Some(SLOT_FRONT)
                    } else if r.is("MC_TOP") {
                        Some(SLOT_TOP)
                    } else if r.is("MC_RIGHT") {
                        Some(SLOT_RIGHT)
                    } else if r.is("MC_PERS") {
                        Some(SLOT_PERSPECTIVE)
                    } else if r.is("MC_FCURVE") {
                        Some(SLOT_FCURVE)
                    } else if r.is("MC_SCHEM") {
                        Some(SLOT_SCHEMATIC)
                    } else if r.is("MC_DOPE") {
                        Some(SLOT_DOPESHEET)
                    } else {
                        None
                    };
                    match slot {
                        Some(k) => have_token = memo_cam_read(r, Some(&mut layout.windows[w].slots[k].memo))?,
                        None => {
                            // the memo cameras are over; the word is looked at once more and
                            // then ends the block
                            memo = false;
                            have_token = true;
                        }
                    }
                }
            }
        }
        if !have_token {
            r.in_token();
        }
        have_token = false;
        first = false;
    }
}

/// The memo cameras of one view [soft 0x083dfc3c layoutAsciiMemoCamRead]: a count, then per
/// camera its number, `ACTIVE` or not, and six keywords in fixed order, each with its values.
///
/// One token is read where each keyword is expected, whether or not it is that keyword. The
/// last, `MC_CAM_CLIP`, is missing in older files: the token read in its place is then the
/// number of the next camera (so that is not read again) or, after the last camera, the word
/// that follows the block. The result says whether such a token is waiting.
///
/// A view has four memo cameras; the original stores as many as the count says. Here those
/// beyond the fourth are read and dropped.
fn memo_cam_read(r: &mut Reader, mut memo: Option<&mut [MemoCamera; 4]>) -> Result<bool, SetupError> {
    let mut need_index = true;
    let n = r.short() as i32;
    for i in 0..n {
        if need_index {
            r.in_svalue_na();
        }
        let active = r.next_is("ACTIVE") as i16;
        let Some(all) = memo.as_deref_mut() else {
            return Err(SetupError::NullWindow { keyword: "MEMO_CAMERA" });
        };
        let mut spare = MemoCamera::default();
        let m = match all.get_mut(i as usize) {
            Some(m) => m,
            None => {
                r.info.deviations.push("more than four memo cameras for a view");
                &mut spare
            }
        };
        m.active = active;
        if r.next_is("MC_CAM_POSITION") {
            r.vector_read(Some(&mut m.position));
        }
        if r.next_is("MC_CAM_INTEREST") {
            r.vector_read(Some(&mut m.interest));
        }
        if r.next_is("MC_CAM_ROLL") {
            m.roll = r.float();
        }
        if r.next_is("MC_CAM_FOV") {
            m.fov = r.float();
        }
        if r.next_is("MC_CAM_ANGLE") {
            m.angle = r.float();
        }
        if r.next_is("MC_CAM_CLIP") {
            need_index = true;
            m.clip[0] = r.float();
            m.clip[1] = r.float();
        } else {
            need_index = false;
        }
    }
    Ok(!need_index)
}

/// The shading properties of a view or of the hardware renderer
/// [soft 0x083de0b0 shadeSetupRead]: from the word in the token buffer on, keyword and value
/// pairs until a word that is none of them (which is left in the buffer). With no record to
/// fill, the value is skipped.
pub(crate) fn shade_setup_read(r: &mut Reader, mut props: Option<&mut ShadeSetup>) {
    loop {
        let p = props.as_deref_mut();
        if r.is("TEXTURE") {
            match p {
                Some(p) => p.texture = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("AFFECTED_MODEL") {
            match p {
                Some(p) => p.affected_model = r.next_is("SELECTED") as i16,
                None => {
                    r.in_token();
                }
            }
        } else if r.is("SHOW_ICON") {
            match p {
                Some(p) => p.show_icon = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("HIGHLIGHT_WIDTH") {
            match p {
                Some(p) => {
                    // through a local: uninitialised in the original if the file ends here
                    let mut v = 0.0f32;
                    r.in_fvalue(&mut v);
                    p.highlight_width = v;
                }
                None => {
                    r.in_fvalue_na();
                }
            }
        } else if r.is("SHADED_GHOSTS") {
            match p {
                Some(p) => p.shaded_ghosts = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("GHOST_FOLLOW_SELECTION") {
            match p {
                Some(p) => p.ghost_follow_selection = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("PSEUDO_WIRE") {
            match p {
                Some(p) => p.pseudo_wire = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("PSEUDO_WIRE_MODE") {
            let v = r.short();
            if let Some(p) = p {
                p.pseudo_wire_mode = v as i32;
            }
        } else if r.is("UNSELECTED_HIGHLIGHTS") {
            match p {
                Some(p) => p.unselected_highlights = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("SHOW_GMATTE") {
            match p {
                Some(p) => p.show_gmatte = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("ANTIALIAS") {
            match p {
                Some(p) => p.antialias = yes(r),
                None => {
                    r.in_token();
                }
            }
        } else if r.is("OPTIMISATION") {
            match p {
                Some(p) => {
                    r.in_token();
                    p.optimisation = if r.is("NONE") {
                        0
                    } else if r.is("SHADE") {
                        1
                    } else {
                        2
                    };
                }
                None => {
                    r.in_token();
                }
            }
        } else {
            return;
        }
        r.in_token();
    }
}

// ------------------------------------------------------------------------------------------
// writer
// ------------------------------------------------------------------------------------------

/// The active module and one block per window [soft 0x083c1778 layoutAsciiWrite], the
/// windows in the order of the program's window array (`SI_windowGetNext`, 0x083d6484).
///
/// `FAST_PLAY` is written as `YES` or `NO` and read as a number: it always comes back 0.
pub(crate) fn layout_write(o: &mut Out, s: &Setup) {
    o.set_indent(0);
    o.raw("\n");
    o.key("ACTIVE_MODULE");
    o.int(s.active_module as i32);
    for (index, w) in s.layout.windows.iter().enumerate() {
        o.key("WINDOW_LAYOUT");
        o.set_indent(1);
        // (a window with no camera gets the bare keyword; every window has one here)
        let slot = &w.slots[w.current];
        o.key("WINDOW_TYPE");
        if let Some(k) = WINDOW_IDS.iter().position(|&id| id == w.id) {
            o.word(WINDOW_NAMES[k]);
        }
        layout_camera_write(o, w, (index, w.current) == SCENE_CAMERA);
        o.key("DISPLAY_TYPE");
        o.int(w.display_type[0]);
        o.key("ZOOM");
        o.choice(w.zoom == 1, "YES", "NO");
        o.key("WINDOW_SIZE");
        match w.size {
            0 => o.word("NORMAL_SIZE"),
            1 => o.word("LARGE_SIZE"),
            2 => o.word("TALL_SIZE"),
            _ => {}
        }
        let v = &slot.view;
        for (k, flag) in [
            ("GRID_VISIBLE", v.grid_visible),
            ("AXIS_VISIBLE", v.axis_visible),
            ("RULER_VISIBLE", v.ruler_visible),
            ("FIELD_VISIBLE", v.field_visible),
            ("X_AXIS_LOCK", v.axis_lock[0]),
            ("Y_AXIS_LOCK", v.axis_lock[1]),
            ("Z_AXIS_LOCK", v.axis_lock[2]),
        ] {
            o.key(k);
            o.choice(flag == 1, "YES", "NO");
        }
        o.key("DELTA");
        o.vector(&v.delta);
        o.key("DISPLAY");
        o.vector(&v.display);
        o.key("MAGNET");
        o.int(v.magnet);
        o.key("MAGNET_TYPE");
        o.int(v.magnet_type as i32);
        o.key("FAST_PLAY");
        o.choice(w.fast_play == 1, "YES", "NO");
        o.key("CONTROLER");
        o.choice(slot.controller.active != 0, "ACTIVE", "INACTIVE");
        o.key("CONTROLER_STEP");
        o.g(slot.controller.step);
        draw_over_write(o, w);
        depthcue_write(o, w);
        shade_setup_write(o, &slot.shade);
        memo_cam_write(o, w);
        o.set_indent(-1);
    }
}

/// The view a window shows and its camera [soft 0x083c26ac layoutCameraAsciiWrite]. The
/// unnamed view (and any id outside the list) gets no name after `CAMERA_TYPE`. Position and
/// interest are left out for the scene camera.
fn layout_camera_write(o: &mut Out, w: &Window, is_scene_camera: bool) {
    let cam = w.camera();
    o.key("CAMERA_TYPE");
    match cam.id.wrapping_sub(2000) {
        0 => o.word("FRONT"),
        1 => o.word("TOP"),
        2 => o.word("RIGHT"),
        3 => o.word("PERSPECTIVE"),
        4 => o.word("FCURVE"),
        8 => o.word("DOPESHEET"),
        10 => {
            o.word("SPREADSHEET");
            o.word("SPREADSHEET_SETUP");
            if let Some(sp) = &w.spreadsheet {
                // (the original looks the name up among the saved spreadsheet set-ups and
                // prints the name of the one it finds; taken to be the same name)
                if let Some(name) = &sp.name {
                    o.buf.extend_from_slice(name);
                    o.buf.push(b' ');
                }
                o.word("SPD_UPDATE_SCENE");
                o.int(sp.update_scene as i32);
                o.word("SPD_UPDATE_SPREADSHEET");
                o.int(sp.update_spreadsheet as i32);
            }
        }
        7 => {
            o.word("SCHEMATIC");
            o.key("SCHEMATIC_MODES");
            for (bit, name) in [(1, "LIGHT"), (2, "MODEL"), (4, "MOTION"), (8, "MATTER")] {
                // the low byte of the flags is tested
                if cam.schematic_modes as u8 & bit != 0 {
                    o.word(name);
                }
            }
            o.word("PLAIN");
        }
        _ => {}
    }
    o.set_indent(1);
    if !is_scene_camera {
        o.key("CAMERA_POSITION");
        o.vector(&cam.position);
        o.key("CAMERA_INTEREST");
        o.vector(&cam.interest);
    }
    o.key("CAMERA_FOV");
    o.g(cam.fov);
    o.key("CAMERA_ASPECT");
    o.g(cam.aspect);
    o.key("CAMERA_NEAR");
    o.g(cam.near);
    o.key("CAMERA_FAR");
    o.g(cam.far);
    o.set_indent(-1);
}

/// The draw-over picture of a window, if it has a picture name or a movie name
/// [soft 0x083c1f0c setupDrawOverWrite]. (Not for a window with id 1004, which the four
/// here never have.)
fn draw_over_write(o: &mut Out, w: &Window) {
    let d = &w.draw_over;
    if (d.name.is_none() && d.movie_name.is_none()) || w.id == 0x3ec {
        return;
    }
    o.set_indent(0);
    o.key("DO_DRAW_OVER");
    o.quoted(d.name.as_deref());
    o.set_indent(1);
    o.key("DO_SEQUENCE");
    o.choice(d.sequence == 1, "YES", "NO");
    o.nl();
    o.int(d.frames[0]);
    o.int(d.frames[1]);
    o.int(d.frames[2]);
    o.int(d.frames[4]);
    o.key("DO_MOVIENAME");
    o.quoted(d.movie_name.as_deref());
    o.key("DO_USEMOVIE");
    o.choice(d.use_movie == 1, "YES", "NO");
    o.key("DO_OPTION");
    o.int(d.option as i32);
    o.set_indent(-1);
}

/// [soft 0x083c2130 setupDepthcueWrite]
fn depthcue_write(o: &mut Out, w: &Window) {
    if w.id == 0x3ec {
        return;
    }
    let d = &w.depth_cue;
    o.set_indent(0);
    o.key("DC_DEPTHCUE");
    o.set_indent(1);
    o.nl();
    o.int(d.active as i32);
    o.int(d.near);
    o.int(d.far);
    for c in d.colour {
        o.int(c as i32);
    }
    o.key("DC_RANGE");
    o.int(d.range);
    o.set_indent(-1);
}

/// [soft 0x083c51ec shadeSetupWrite]
pub(crate) fn shade_setup_write(o: &mut Out, p: &ShadeSetup) {
    o.set_indent(0);
    o.key("TEXTURE");
    o.choice(p.texture != 0, "YES", "NO");
    o.key("AFFECTED_MODEL");
    o.choice(p.affected_model != 0, "SELECTED", "ALL");
    o.key("SHOW_ICON");
    o.choice(p.show_icon != 0, "YES", "NO");
    o.key("SHOW_GMATTE");
    o.choice(p.show_gmatte != 0, "YES", "NO");
    o.key("ANTIALIAS");
    o.choice(p.antialias != 0, "YES", "NO");
    o.key("OPTIMISATION");
    o.word(match p.optimisation {
        0 => "NONE",
        2 => "WIRE",
        _ => "SHADE",
    });
    o.key("HIGHLIGHT_WIDTH");
    o.g(p.highlight_width);
    o.key("SHADED_GHOSTS");
    o.choice(p.shaded_ghosts != 0, "YES", "NO");
    o.key("PSEUDO_WIRE");
    o.choice(p.pseudo_wire != 0, "YES", "NO");
    o.key("PSEUDO_WIRE_MODE");
    o.int(p.pseudo_wire_mode);
    o.key("UNSELECTED_HIGHLIGHTS");
    o.choice(p.unselected_highlights != 0, "YES", "NO");
    o.key("GHOST_FOLLOW_SELECTION");
    o.choice(p.ghost_follow_selection != 0, "YES", "NO");
}

/// The memo cameras of a window [soft 0x083c2280 setupMemoCamWrite]: for each view that has
/// at least one active memo camera, the view's `MC_…` name, the count 4, and the four
/// cameras in full. `MEMO_CAMERA` is written before the first such view only. A view without
/// an `MC_…` name (the unnamed one) would get its cameras with no name in front.
///
/// The original takes the number of views from the window's view menu (its item count less
/// two); it is taken to be the nine slots less the spreadsheet, the last view the files show
/// (`MC_DOPE`).
fn memo_cam_write(o: &mut Out, w: &Window) {
    let mut first = true;
    for (k, slot) in w.slots.iter().enumerate().take(8) {
        if !slot.memo.iter().any(|m| m.active != 0) {
            continue;
        }
        if first {
            first = false;
            o.set_indent(0);
            o.key("MEMO_CAMERA");
            o.set_indent(1);
        }
        o.nl();
        match k {
            SLOT_FRONT => o.word("MC_FRONT"),
            SLOT_TOP => o.word("MC_TOP"),
            SLOT_RIGHT => o.word("MC_RIGHT"),
            SLOT_PERSPECTIVE => o.word("MC_PERS"),
            SLOT_FCURVE => o.word("MC_FCURVE"),
            SLOT_SCHEMATIC => o.word("MC_SCHEM"),
            SLOT_DOPESHEET => o.word("MC_DOPE"),
            _ => {}
        }
        o.nl();
        o.int(4);
        for (i, m) in slot.memo.iter().enumerate() {
            o.nl();
            o.int(i as i32);
            o.nl();
            o.choice(m.active != 0, "ACTIVE", "INACTIVE");
            o.key("MC_CAM_POSITION");
            o.g3(&m.position);
            o.key("MC_CAM_INTEREST");
            o.g3(&m.interest);
            o.key("MC_CAM_ROLL");
            o.g(m.roll);
            o.key("MC_CAM_FOV");
            o.g(m.fov);
            o.key("MC_CAM_ANGLE");
            o.g(m.angle);
            o.key("MC_CAM_CLIP");
            o.g(m.clip[0]);
            o.g(m.clip[1]);
        }
    }
    o.nl();
    o.set_indent(-1);
}
