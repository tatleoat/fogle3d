// SPDX-License-Identifier: MIT
//! `sys_main.c` and `soft_main.c`: the program's entry, its command line, the start-up
//! sequence and the main loop.
//!
//! Licence code is not ported. Where the original's sequence has a licence check, a comment
//! says so and nothing else; where a value depends on one (the constants behind
//! [`App::get_init_value`], the factor [`LICENSED`]), the port uses the value of the full
//! product's path.

use crate::env::{Bytes, Exit, Run, World, P};
use crate::memory::si_calloc;
use crate::render_cmd::{RenderSetup, FAULT};
use crate::server::Clients;
use crate::image::adapter::{place, scratch, AppOutside};
use crate::image::g;
use crate::state::{App, EventRef, NetEntry, SRV_BUF};
use si_rt::message as rt;
use si_x87::F80;

/// A local of `si_runCreative` that the licence checks set to 1 when they pass. It enters
/// later statements as a factor or an increment (the size of the module table, the
/// projection of the main window, the search for the first module). The full product's
/// value.
pub const LICENSED: i32 = 1;

/// The names of the resource files, as the binary's data has them.
pub const SETUP_RSRC: &[u8] = b"Setup.sts";
pub const KEYS_RSRC: &[u8] = b"SIKeys.sks";
pub const DEFAULT_COLOR: &[u8] = b"Default";
pub const RSRC_PATH: &[u8] = b"../rsrc";
pub const REFLECT_MATTER: &[u8] = b"/Reflect";
pub const MODULE_RSRC: [&[u8]; 5] = [b"/Model.rsrc", b"/Motion.rsrc", b"/Actor.rsrc", b"/Matter.rsrc", b"/Tools.rsrc"];
/// How many custom-effect tables each module's canvas is read with (Motion has two: its
/// own and the channels').
pub const MODULE_CUSTOM_TABLES: [i32; 5] = [1, 2, 1, 1, 1];
/// `SI_idString` (0x08644c80): what `-v` prints; the text is read from the install (not kept in this source).
pub fn id_string() -> &'static [u8] {
    rt(0x08644c80)
}

/// [soft 0x080caadc SI_start1], [soft 0x080cd060 SI_end1], [soft 0x080c5cc4 SI_start2],
/// [soft 0x080c725c SI_end2]: the markers at both ends of `sys_main.c` and `soft_main.c`.
pub fn si_start1() -> i32 {
    10
}
pub fn si_end1() -> i32 {
    11
}
pub fn si_start2() -> i32 {
    9
}
pub fn si_end2() -> i32 {
    8
}

/// What the command line of the interactive program asked for (the locals of
/// `si_runCreative` that the option loop fills).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    /// The resource directory: the last argument that is not an option.
    pub resources: Option<Bytes>,
    /// `-s`: load a setup file; its name, `Setup.sts` when none was given.
    pub setup_given: bool,
    pub setup_file: Option<Bytes>,
    /// `-k <file>`: the keys file; `SIKeys.sks` otherwise.
    pub keys_file: Option<Bytes>,
    /// `-L`: use the latest version of the scene named.
    pub latest_version: bool,
    /// `-l` or `-L` without a name: load the scene saved last.
    pub latest_scene: bool,
    /// `-l <scene>`, `-L <scene>`.
    pub scene: Option<Bytes>,
    /// `-d <database>`.
    pub database: Option<Bytes>,
    /// `-C <file>`: the colour map's name without suffix.
    pub color: Option<Bytes>,
}

impl<W: World + RenderSetup> App<W> {
    // adapter: not in place
    /// `set_init_buf (0x080d0e6c)`: the image port ([`crate::image::batch::set_init_buf`]).
    pub fn set_init_buf(&mut self, from: &[u8]) {
        let mut m = scratch();
        let p = m.place_cstr(from);
        crate::image::batch::set_init_buf(&mut m, p);
        self.g.init_buf.copy_from_slice(m.bytes(g::INIT_BUF.0, 7));
    }

    // adapter: not in place
    /// `get_init_value (0x080d0e8c)`: the image port ([`crate::image::batch::get_init_value`])
    /// on the struct form's seven bytes. With the bytes the licensed start-up puts there
    /// ([`SRV_BUF`]) the seven values are 1, 4, 1, 1, 0, 4, 13.
    pub fn get_init_value(&self, index: i32) -> i32 {
        let mut m = scratch();
        m.set_bytes(g::INIT_BUF.0, &self.g.init_buf);
        crate::image::batch::get_init_value(&m, index)
    }

    // adapter: not in place
    /// `getDefaultPath (0x080caae8)`: the image port
    /// ([`crate::image::batch::get_default_path`]); the path it leaves. `None` only when
    /// the 4,096-byte block cannot be had.
    pub fn get_default_path(&mut self, program: &[u8]) -> Option<Bytes> {
        let mut m = scratch();
        let p = place(&mut m, Some(program));
        let out = m.place(4, 4);
        crate::image::batch::get_default_path(&mut m, &mut AppOutside(&mut self.w), p, out);
        match m.u32(out) {
            0 => None,
            buf => Some(m.cstr(buf)),
        }
    }

    // adapter: not in place
    /// `printUsage (0x080cd10c)`: the image port ([`crate::image::batch::print_usage`]).
    pub fn print_usage(&mut self, name: &[u8]) -> Exit {
        let mut m = scratch();
        let n = place(&mut m, Some(name));
        crate::image::batch::print_usage(&mut m, &mut AppOutside(&mut self.w), n)
    }

    /// [soft 0x080d05d0 zbufferWarning]: two lines when the display has no depth buffer.
    pub fn zbuffer_warning(&mut self) {
        self.w.err(rt(0x0867c2a0));
        self.w.err(rt(0x0867c2e0));
    }

    /// [soft 0x080d0d20 copyRightClose]: takes the splash window down. It first waits
    /// until something is in the event queue or 30 seconds have passed (polling `qtest`
    /// and `time` without sleeping). Then the splash window is cleared and closed and the
    /// main window cleared; with no splash window (−1) only the font is set.
    pub fn copy_right_close(&mut self, splash: i32) {
        if splash != -1 {
            let start = self.w.time();
            loop {
                if self.w.qtest() != 0 {
                    break;
                }
                let now = self.w.time();
                if now.wrapping_sub(start) >= 0x1e {
                    break;
                }
            }
            self.w.winset(splash);
            self.w.cpack(0);
            self.w.clear();
            self.w.winpush();
            self.w.drawmode(0x20);
            self.w.color(0);
            self.w.clear();
            self.w.drawmode(0x10);
            if self.g.bit_planes > 8 {
                self.w.drawmode(0x40);
                if self.g.olbit_planes < 3 {
                    self.w.color(0);
                    self.w.clear();
                } else {
                    self.w.frontbuffer(1);
                    self.w.color(0);
                    self.w.clear();
                    self.w.frontbuffer(0);
                }
            }
            self.w.drawmode(0x10);
            self.w.winclose(splash);
            self.w.winset(self.g.sysid);
            self.w.frontbuffer(1);
            self.w.color(0);
            self.w.clear();
            self.w.frontbuffer(0);
        }
        self.w.font(self.g.irisf as i32);
    }

    /// [soft 0x080d0b8c handleSwiftMouse]: loads the optional library `swiftmouse.so` and
    /// looks up its four entry points; if the library or any of them is missing, all four
    /// are null (and the library is closed).
    pub fn handle_swift_mouse(&mut self) {
        const NAMES: [&[u8]; 4] = [b"SI_detectSwiftMouse", b"SI_saveSwiftMouse", b"SI_loadSwiftMouse", b"SI_deleteSwiftMouse"];
        let library = self.w.dlopen(b"swiftmouse.so", 1);
        if library != 0 {
            let mut all = true;
            for (i, name) in NAMES.iter().enumerate() {
                let entry = self.w.dlsym(library, name);
                if entry == 0 {
                    self.w.dlclose(library);
                    all = false;
                    break;
                }
                self.g.swift_mouse[i] = entry;
            }
            if all {
                return;
            }
        }
        self.g.swift_mouse = [0; 4];
    }

    /// [soft 0x080c5c98 SI_checkOptions]: makes the graph lists and sets the first list's
    /// display mode to "nothing shown" (`SI_shwNul`). Its argument, the program's name,
    /// is not used.
    pub fn check_options(&mut self, _program: &[u8]) {
        self.w.SI_grafListCreate();
        self.w.mem_graf_list_show_nothing();
    }

    /// `systemInit` (0x080cd76c): the initialisation that needs the canvases. Not cited as
    /// a whole: its first part, a loop over the windows of the screen layout that prepares
    /// each viewport and gives the perspective window the scene's camera with an aspect
    /// ratio from the screen resolution, is one call on the trait.
    ///
    /// In order: a graph list for the scene; the viewport loop; the schematic parameters;
    /// the camera is added to the first graph list with the history switched on for the
    /// call; manipulation, animation, increment and duplication parameters;
    /// `SI_OGL_ASYNCH_MODE`; the three icon models `/LIGHT_ICON`, `/BULB_ICON`,
    /// `/CAMERA_ICON` from the resource directory; the default camera; materials; a
    /// licence check (not ported); the event positions; meta-balls, dope sheet, audio,
    /// curve scrolling, particles, waves, display modes, field guide.
    pub fn system_init(&mut self, resources: &[u8]) {
        let list = self.w.SI_grafListAdd();
        self.w.mem_set_scene_graf_list(list);
        self.w.systemInit_viewports();
        self.schematic_info_init();
        self.g.history_active = 1;
        self.w.SI_graflistAddData(P, self.g.proc_data.camera, 4, 0);
        self.g.history_active = 0;
        self.manipulation_init();
        self.animation_info_init();
        self.increment_info_init();
        self.duplication_info_init(1);
        self.g.ogl_gdi_synch = self.w.getenv(b"SI_OGL_ASYNCH_MODE").is_some() as i16;
        for (i, name) in [b"/LIGHT_ICON".as_slice(), b"/BULB_ICON", b"/CAMERA_ICON"].iter().enumerate() {
            self.g.icons[i] = self.w.SI_omodelBinaryRead(&[resources, name].concat(), 0, 0);
        }
        self.w.SI_initCamDefault();
        self.w.SI_matterInit();
        // licence check, not ported
        self.w.SI_eventInitPos();
        self.w.SI_metaballInit();
        self.w.SI_dopeSheetInit();
        self.w.SI_audioMngContainerInit();
        self.w.SI_fcvScrollInit();
        self.w.SI_initParticle();
        self.w.SI_initWave();
        self.w.SI_displayModeInit();
        self.w.SI_initFieldGuide();
    }

    /// The path globals both front ends set from the current directory: object and
    /// texture paths, the picture path (the output database's `RENDER_PICTURES` chapter
    /// when it is writable), and the three library paths.
    pub(crate) fn set_paths_from_cwd(&mut self) {
        let cwd = self.g.cwd.clone();
        self.g.object_path = cwd.clone();
        self.g.texture_path = cwd.clone();
        let mut picture = cwd.clone();
        if self.w.mem_have_output_database() {
            self.w.SI_dbSetChapter(P, b"RENDER_PICTURES");
            self.w.SI_dbfilepathBuild(P);
            if self.w.SI_dbChapterCheckAccess(P, 2) != 0 {
                picture = Some(self.w.output_chapter_path());
            }
        }
        self.g.picture_path = picture;
        self.g.material_lib = cwd.clone();
        self.g.texture_lib = cwd.clone();
        self.g.palette_lib = cwd;
    }

    /// The option loop of [soft 0x080cab88 si_runCreative]: every argument after the
    /// program's name, in order. An argument that does not start with `-` is the resource
    /// directory. Only the second character of an option is looked at (`-Rx` is `-R`).
    ///
    /// Options that end the program do so from inside the loop, as in the original: `-h`
    /// (help), `-R` (the batch renderer, with the arguments from `-R` on), `-D` (a
    /// stand-alone custom effect), an unknown option (a message on the standard output,
    /// then the help), and every usage error (a message on the standard error stream).
    /// All of them end with status 0, except `-D`, which ends with what the effect
    /// returns.
    pub fn parse_command_line(&mut self, args: &[Bytes]) -> Run<CommandLine> {
        let mut c = CommandLine { keys_file: Some(KEYS_RSRC.to_vec()), ..CommandLine::default() };
        let argc = args.len();
        let dash = |i: usize| args[i].first() == Some(&b'-');
        let mut i = 1;
        while i < argc {
            let a = &args[i];
            if !dash(i) {
                c.resources = Some(a.clone());
                i += 1;
                continue;
            }
            let letter = a.get(1).copied().unwrap_or(0);
            match letter {
                // product-tier flags: stored, never branched on (see `Globals`)
                b'B' => self.g.base_licence = 1,
                b'G' => {
                    self.g.light_licence = 1;
                    self.g.light_licence2 = 1;
                }
                b'f' => self.g.foreground = 1,
                b'c' => self.g.coredump = 1,
                b'h' => {
                    if &a[2..] == b"dev" {
                        // `-hdev` runs the program's self-check and ends with its result:
                        // licence code, not ported. The port ends without it.
                        return Err(Exit(0));
                    }
                    let name = self.g.program_name.clone();
                    return Err(self.print_usage(&name));
                }
                b's' => {
                    c.setup_given = true;
                    if i + 1 != argc && !dash(i + 1) {
                        if c.resources.is_none() {
                            self.w.err(rt(0x08679840));
                            return Err(Exit(0));
                        }
                        i += 1;
                        c.setup_file = Some(args[i].clone());
                    }
                    if c.setup_file.is_none() {
                        c.setup_file = Some(SETUP_RSRC.to_vec());
                    }
                }
                b'k' => {
                    if i + 1 == argc || dash(i + 1) {
                        self.w.err(&[b"The -".as_slice(), &[letter], rt(0x08679887)].concat());
                        return Err(Exit(0));
                    }
                    // a block of the name's length and five more from SI_calloc; the name
                    // is copied into it
                    if si_calloc(1, args[i + 1].len() + 5).is_none() {
                        return Err(Exit(0));
                    }
                    i += 1;
                    c.keys_file = Some(args[i].clone());
                }
                b'L' | b'l' => {
                    if letter == b'L' {
                        c.latest_version = true;
                    }
                    if i + 1 == argc || dash(i + 1) {
                        c.latest_scene = true;
                    } else {
                        i += 1;
                        c.scene = Some(args[i].clone());
                    }
                }
                b'd' => {
                    if i + 1 == argc || dash(i + 1) {
                        self.w.err(&[b"The -".as_slice(), &[letter], rt(0x08679887)].concat());
                        return Err(Exit(0));
                    }
                    if c.resources.is_none() {
                        self.w.err(rt(0x08679900));
                        return Err(Exit(0));
                    }
                    i += 1;
                    c.database = Some(args[i].clone());
                }
                b'R' => {
                    if c.resources.is_none() {
                        c.resources = self.get_default_path(&args[0]);
                    }
                    if c.resources.is_some() {
                        self.g.resdir = c.resources.clone();
                    }
                    self.stand_alone_render(&args[i..], c.resources.clone())?;
                    return Err(Exit(0));
                }
                b'C' => {
                    if i + 1 != argc && !dash(i + 1) {
                        if c.resources.is_none() {
                            self.w.err(rt(0x08679940));
                            self.w.err(rt(0x08679980));
                            return Err(Exit(0));
                        }
                        i += 1;
                        c.color = Some(self.w.strip_suffix(&args[i]));
                    }
                }
                b'v' => self.w.out(&[id_string(), b"\n"].concat()),
                b'D' => {
                    // licence check, not ported
                    if i > 1 {
                        self.w.err(&[b"The -".as_slice(), &[letter], rt(0x086799e7)].concat());
                        return Err(Exit(0));
                    }
                    if i + 2 >= argc || dash(i + 1) || dash(i + 2) {
                        self.w.err(&[b"The -".as_slice(), &[letter], rt(0x08679a27)].concat());
                        return Err(Exit(0));
                    }
                    let symbol = args[i + 2].clone();
                    let file = &args[i + 1];
                    let base = file.iter().rposition(|&c| c == b'/').map_or(0, |p| p + 1);
                    // the library's name: ".so" added when the file name has no dot, "so"
                    // when it ends with its first dot
                    let library = match file[base..].iter().position(|&c| c == b'.') {
                        None => [&file[..], b".so"].concat(),
                        Some(d) if base + d + 1 == file.len() => [&file[..], b"so"].concat(),
                        Some(_) => file.clone(),
                    };
                    self.g.cus_standalone = 1;
                    // the original overwrites argv[i + 2] with the library's name and hands
                    // the arguments from there on to the effect
                    let status = self.w.SI_cusRunStandalone(&symbol, (argc - i) as i32 - 2, Some(&library));
                    return Err(Exit(status));
                }
                _ => {
                    let name = self.g.program_name.clone();
                    self.w.out(&[&name[..], b": illegal option ", a, b"\n"].concat());
                    return Err(self.print_usage(&name));
                }
            }
            i += 1;
        }
        Ok(c)
    }

    /// [soft 0x080cab88 si_runCreative]: the program. `args` is the whole command line.
    /// Returns (the original returns 1) only if the main loop does, which it does not: the
    /// program ends through `SI_serverExit`, here as an `Exit`.
    ///
    /// The sequence is the numbered list of finding 0019; the comments here give the
    /// numbers.
    pub fn run_creative<C: Clients<W>>(&mut self, clients: &mut C, args: &[Bytes]) -> Run<i32> {
        if args.is_empty() {
            return Err(FAULT);
        }
        // 1. the command line as one string, for the restart offer
        self.replicate_command_line(args);
        // 2. licence check, not ported
        // 3. emergency save off for the length of the start-up
        self.setup_emergency_save(0);
        // 4.
        self.g.init_pose_compensate = self.w.getenv(b"SI_INIT_POSE_COMPENSATE").is_some() as i16;
        // 5. licence checks, not ported (getpid and rand are called in them)
        // 6.
        self.g.username = self.w.name_of_user();
        // 7.
        self.g.program_name = args[0].clone();
        self.w.SI_menuTableInit();
        self.w.SI_statusInitPath();
        // 8. the start-up constants: the full product's values
        self.set_init_buf(&SRV_BUF);
        // 9. licence check, not ported; around it the locale is read, set to "C" and put
        // back
        let locale = self.w.setlocale(6, None);
        let Some(locale) = locale else {
            self.w.out(rt(0x08678fdc));
            return Err(Exit(1));
        };
        self.w.setlocale(6, Some(b"C"));
        self.w.setlocale(6, Some(&locale));
        // 10. mental ray 2.1 output unless 1.9 is enabled
        let mr19 = self.w.SI_isMr19Enable();
        self.w.SI_setmi2Output((mr19 == 0) as i32);
        // 11. the options
        self.g.base_licence = 0;
        self.g.light_licence = 0;
        self.g.light_licence2 = 0;
        let mut c = self.parse_command_line(args)?;
        // 12. the resource directory
        let absolute;
        match &c.resources {
            None => {
                c.resources = self.get_default_path(&args[0]);
                absolute = true;
            }
            Some(r) => absolute = r.first() == Some(&b'/'),
        }
        let res = c.resources.clone().ok_or(FAULT)?;
        if c.color.is_none() {
            c.color = Some(DEFAULT_COLOR.to_vec());
        }
        // 13.
        self.signal_init(self.g.coredump);
        // 14.
        self.g.cwd = self.w.getcwd(0xff);
        self.g.homedir = self.w.getenv(b"HOME");
        if self.g.homedir.is_none() {
            self.g.homedir = self.w.getcwd(0xff);
        }
        // 15.
        self.w.SI_colorMapStore(0);
        // 16. the site's start-up script
        if self.w.access(&[&res[..], b"/soft.init"].concat(), 0) == 0 {
            self.w.SI_system(&[b"csh ".as_slice(), &res, b"/soft.init"].concat());
        }
        // 17. (the length of the resource path divided by a start-up constant that is 1)
        let res_len = (res.len() as u32 / self.get_init_value(0) as u32) as i16;
        self.g.resdir = Some(res.clone());
        self.w.putenv(&[b"SOFT_RSRC=".as_slice(), &res].concat());
        // 18. licence checks, not ported (the product tier, the options)
        self.g.option_rotoscopy = 1;
        // 19.
        self.w.SI_initDictionaries(Some(&res), 1, 1);
        self.check_options(&args[0]);
        // 20. a current directory the C library could not give is a null pointer the
        // original hands to strdup
        let cwd = self.g.cwd.clone().ok_or(FAULT)?;
        self.set_paths_from_cwd();
        // 21.
        let mut g = std::mem::take(&mut self.g);
        let hardware = self.w.check_hardware(&mut g);
        self.g = g;
        if hardware == 0 {
            self.w.err(rt(0x08679b60));
            return Err(Exit(-1));
        }
        if self.w.mem_zbuffer() == 0 {
            self.zbuffer_warning();
        }
        // 22. the main window
        self.w.SI_mouseWarpGet(P, P);
        self.w.prefposition(0, self.g.xmaxscreen as i32, 0, self.g.ymaxscreen as i32);
        self.w.noborder();
        if self.g.foreground != 0 {
            self.w.foreground();
        }
        self.g.sysid = self.w.winopen(b"soft");
        self.w.wintitle(b"soft");
        self.w.mallopt(1, 1000);
        if LICENSED != 0 {
            self.w.doublebuffer();
        }
        self.w.overlay(self.g.olbit_planes);
        self.w.RGBmode();
        self.w.gconfig();
        self.w.frontbuffer(1);
        self.w.RGBcolor(0, 0, 0);
        self.w.clear();
        self.w.frontbuffer(0);
        self.w.cmode();
        self.w.gconfig();
        self.w.frontbuffer(1);
        self.w.color(0);
        self.w.clear();
        self.w.frontbuffer(0);
        self.w.subpixel(1);
        self.w.glcompat(0, 0);
        self.w.SI_graphicInit();
        self.w.qdevice(0x201);
        // 23. the splash window
        let splash = self.w.copyRightDisplay(&res);
        self.g.no_overlay_refresh_fix = 0;
        // 24. the reflection picture: beside the current directory, else in the resources
        let reflect = if self.w.access(&[&cwd[..], REFLECT_MATTER, b".pic"].concat(), 0) == 0 {
            cwd.clone()
        } else if absolute {
            // the original sizes this block with the divided length of step 17
            let _ = res_len;
            res.clone()
        } else {
            [&cwd[..], b"/", &res].concat()
        };
        self.g.reflection_matter = [&reflect[..], REFLECT_MATTER].concat();
        if self.w.access(&[&self.g.reflection_matter[..], b".pic"].concat(), 0) < 0 {
            let why = self.w.last_error();
            self.w.err(&[b"Cannot open file ".as_slice(), &self.g.reflection_matter, b".pic ...(", &why, b")\n"].concat());
            return Err(Exit(-1));
        }
        self.w.SI_readTextureFile(P, 0);
        // 25. fonts
        self.w.fontSoftimageDefine(&[&res[..], b"/Softimage.fnt"].concat(), P, self.g.appf);
        if self.g.irisf != self.g.appf {
            self.w.fontSoftimageDefine(&[&res[..], b"/Softimage15.fnt"].concat(), P, self.g.irisf);
        }
        self.w.fontSoftimageLogoDefine(&[&res[..], b"/Softlogo.fnt"].concat(), self.g.logof);
        // 26. the desktop's colours
        if self.w.SI_canvasReadColor(&[&cwd[..], b"/Desktop.rsrc"].concat(), 0) == 0 {
            let path = [&res[..], b"/Desktop.rsrc"].concat();
            if self.w.SI_canvasReadColor(&path, 1) == 0 {
                let why = self.w.last_error();
                self.w.err(&[b"Cannot open file ".as_slice(), &path, b"...(", &why, b")\n"].concat());
                return Err(Exit(-1));
            }
        }
        // 27. the colour map
        if let Some(color) = &c.color {
            let here = [&cwd[..], b"/", color, b".col"].concat();
            if self.w.access(&here, 0) == 0 {
                if self.w.SI_colorMapLoad(&here) != 0 {
                    self.w.SI_colorGetMap();
                }
            } else {
                let there = [&res[..], b"/", color, b".col"].concat();
                if self.w.access(&there, 0) == 0 {
                    // i_seterror(fprintf): the picture library's error routine
                    self.w.i_seterror(P);
                    if self.w.SI_colorMapLoad(&there) != 0 {
                        self.w.SI_colorGetMap();
                    }
                }
            }
        }
        // 28. the module table: five entries of 16 bytes
        let entries = self.get_init_value(LICENSED + 1) * 5;
        debug_assert_eq!(entries, 5);
        self.g.network = [NetEntry::default(); 5];
        // 29. plug-ins
        self.w.SI_cusInit();
        // 30. the five canvases
        for m in 0..5usize {
            self.g.network[m].module = m as i16;
            let tables = MODULE_CUSTOM_TABLES[m];
            let mut canvas = self.w.SI_canvasRead(&[&cwd[..], MODULE_RSRC[m]].concat(), 0, P, tables, m as i32);
            if canvas == 0 {
                canvas = self.w.SI_canvasRead(&[&res[..], MODULE_RSRC[m]].concat(), 1, P, tables, m as i32);
                if canvas == 0 {
                    self.g.network[m].canvas = 0;
                    return Err(Exit(-1));
                }
            }
            self.g.network[m].canvas = canvas;
            self.w.mem_canvas_set_module(canvas, m as i16);
            if m > 0 {
                self.w.mem_canvas_share(canvas, self.g.network[0].canvas);
            }
            if m == 1 {
                self.w.SI_cusSetupDynamicMenus();
            }
        }
        self.g.network[4].disabled = 0;
        // 31. the screen layouts
        let mut status = self.w.SI_windowReadInit(&[&cwd[..], b"/Windows.rsrc"].concat());
        if status != 0 {
            let path = [&res[..], b"/Windows.rsrc"].concat();
            status = self.w.SI_windowReadInit(&path);
            if status > 0 {
                let why = self.w.last_error();
                self.w.err(&[b"ERROR: ".as_slice(), &path, b" : (", &why, b")\n"].concat());
                return Err(Exit(-1));
            }
            if status == -1 {
                self.w.err(&[b"ERROR: Ressource file corrupted : (".as_slice(), &path, b")\n"].concat());
                return Err(Exit(status));
            }
        }
        // 32. the main window's projection: pixel centres on integers
        let scale = self.get_init_value(3);
        self.w.viewport(0, self.g.xmaxscreen as i32, 0, self.g.ymaxscreen as i32);
        let g = F80::from_i32(scale);
        let half = F80::from_f64(0.5);
        let top = ((F80::from_i32(self.g.ymaxscreen as i32 * LICENSED) + half) * g).to_f32();
        let low = (g * F80::from_f64(-0.5) * F80::from_i32(LICENSED)).to_f32();
        let right = ((half + F80::from_i32(self.g.xmaxscreen as i32 * LICENSED)) * g).to_f32();
        self.w.ortho2(low, right, low, top);
        self.w.writemask(self.g.system_wm as i32);
        self.w.font(self.g.irisf as i32);
        self.w.SI_initDefault();
        // 33. the event queue
        self.w.unqdevice(0x201);
        self.init_event();
        // 34. the tablet
        if self.w.getenv(b"SI_TABLET").is_some() {
            self.g.tablet_active = self.w.SI_tabletInit();
            if self.g.tablet_active <= 0 {
                self.w.SI_tabletDisplayError();
                return Err(Exit(-1));
            }
        } else {
            self.g.tablet_active = 0;
        }
        // 35. the first module that can be chosen
        self.g.active = (self.g.active as i32 + self.get_init_value(4)) as i16;
        loop {
            let entry = *self.g.network.get(self.g.active as usize).ok_or(FAULT)?;
            if entry.disabled != 0 {
                self.g.active = (self.g.active as i32 + LICENSED) as i16;
            } else {
                self.g.active_canvas = entry.canvas;
                break;
            }
        }
        // 36.
        self.system_init(&res);
        // 37.
        self.copy_right_close(splash);
        // 38. the first canvas
        self.client_activate(self.g.active, 1);
        self.w.SI_fsliderDraw(self.g.frame_current);
        self.g.module_redraw = 0;
        self.w.SI_colorMapStore(1);
        self.w.SI_statusDeleteFile();
        self.w.SI_statusOpenFile();
        // 39. the setup file
        if c.setup_given {
            self.w.SI_setupLoadFromServer(&res, c.setup_file.as_deref());
        } else if self.w.access(&[&res[..], b"/", SETUP_RSRC].concat(), 4) == 0 {
            self.w.SI_setupLoadFromServer(&res, Some(SETUP_RSRC));
        }
        // 40. the keys
        self.handle_swift_mouse();
        self.supra_keys_load_from_server(&res, c.keys_file.as_deref());
        // 41. the database of -d
        if let Some(name) = &c.database {
            let db = self.w.SI_dbDirGetDatabaseFromName(P, name);
            if db != 0 && self.w.mem_database_valid(db) {
                self.w.mem_set_input_database(db);
                self.w.mem_set_output_database(db);
            } else {
                self.w.err(&[b"ERROR: input database [".as_slice(), name, rt(0x08679cb9)].concat());
            }
        }
        // 42. the scene of -l / -L
        let mut scene = c.scene.clone();
        if scene.is_none() && c.latest_scene {
            scene = self.w.latest_scene(0, None);
            let shown = scene.clone().unwrap_or_else(|| b"(null)".to_vec());
            self.w.SI_statusDisplayMessage(&[b"Latest scene used: [".as_slice(), &shown, b"]\n"].concat(), 0);
        }
        if let Some(mut scene) = scene {
            if scene.len() >= 4 && scene.ends_with(b".dsc") {
                scene.truncate(scene.len() - 4);
            }
            if c.latest_version {
                self.w.SI_dbSetChapter(P, b"SCENES");
                self.w.SI_dbfilepathBuild(P);
                scene = self.w.latest_version(&scene);
                self.w.SI_statusDisplayMessage(&[b"Latest version of scene used: [".as_slice(), &scene, b"]\n"].concat(), 0);
            }
            if self.w.SI_dbLoadScene(P, P, &scene) == 0 {
                self.w.SI_statusDisplayMessage(&[b"Scene ".as_slice(), &scene, rt(0x08679d48)].concat(), 3);
            } else {
                self.w.SI_sceneRefresh();
            }
        }
        // 43. emergency save on; the last crash
        self.setup_emergency_save((self.g.standalone_renderer == 0) as i16);
        if let Some(dir) = self.last_crash_log(0) {
            self.w.err(&[b"\nSoft last crashed in : ".as_slice(), &dir, b"\n"].concat());
            if self.dialog_request2(rt(0x08679dc4), &dir, rt(0x08679da0), b"Yes", b"No") as i16 == 0 {
                self.w.SI_dbInfoInit(P);
                if let Some(saved) = self.w.latest_scene(2, Some(&dir)) {
                    if self.w.SI_dbLoadScene(P, P, &saved) == 0 {
                        self.w.err(rt(0x08679de0));
                    } else {
                        self.w.SI_sceneRefresh();
                    }
                }
            }
            self.last_crash_log(1);
        }
        // 44. the log
        self.log_open();
        // 45. no database of one's own
        if self.w.SI_dbCheckLocalDatabase(P) == 0
            && self.dialog_request2(rt(0x08679e40), b"", rt(0x08679e16), b"Create", b"Continue") as i16 == 0
        {
            self.w.SI_dialogSetDatabase(P, P, P, P, 1);
            self.w.mem_output_follows_input();
        }
        // 46. the first module's loop, then the main loop
        let mut event = EventRef::Active;
        self.set_module_active(clients, self.g.active, &mut event)?;
        self.run_main_loop(clients, 0, &mut event)?;
        self.log_close();
        Ok(1)
    }

    /// [soft 0x080cd06c setModuleActive]: runs the event loop of module `module` (0 Model,
    /// 1 Motion, 2 Actor, 3 Matter, 4 Tools) and stores what it returns in the network
    /// entry of the module the active canvas belongs to. Any other number is an error:
    /// a message, clean-up and exit.
    pub fn set_module_active<C: Clients<W>>(&mut self, clients: &mut C, module: i16, event: &mut EventRef) -> Run<()> {
        if !(0..=4).contains(&module) {
            self.w.err(format!("Error: unknown module in server...({module})\n").as_bytes());
            self.server_clean(clients);
            return self.server_exit(0, 0);
        }
        let (result, ev) = clients.client(self, module);
        *event = ev;
        let index = self.w.mem_canvas_module(self.g.active_canvas);
        self.g.network.get_mut(index as usize).ok_or(FAULT)?.state = result;
        Ok(())
    }

    /// [soft 0x080caa28 runMainLoop]: the main loop. It looks at the event the last module
    /// loop stopped on:
    ///
    /// - kind 2, another module was chosen: if `SI_active` changed since the last pass,
    ///   its canvas is activated (with the pending redraw flag, which is then cleared);
    ///   then that module's loop runs.
    /// - kind 4, quit: the other modules are told to finish and the program exits through
    ///   `SI_serverExit(0, 0)`.
    /// - anything else: nothing changes and the loop spins for ever in the original. The
    ///   port gives up with [`FAULT`].
    ///
    /// The loop also ends when `done` is not 0 on entry.
    pub fn run_main_loop<C: Clients<W>>(&mut self, clients: &mut C, done: i32, event: &mut EventRef) -> Run<()> {
        let mut done = done;
        let mut shown: i16 = -1;
        while done == 0 {
            match self.event(*event).kind {
                2 => {
                    if self.g.active != shown {
                        self.client_activate(self.g.active, self.g.module_redraw);
                        shown = self.g.active;
                        self.g.module_redraw = 0;
                    }
                    let module = self.g.network.get(self.g.active as usize).ok_or(FAULT)?.module;
                    self.set_module_active(clients, module, event)?;
                }
                4 => {
                    self.server_clean(clients);
                    self.server_exit(0, 0)?;
                    done = 1;
                }
                _ => return Err(FAULT),
            }
        }
        Ok(())
    }

    /// [soft 0x080c5cf0 realmain]: the licence client is set up (not ported; the
    /// environment variable `DISPLAY` and the locale are read for it, and the program
    /// ends with status 1 if the locale cannot be read), then the program runs.
    pub fn realmain<C: Clients<W>>(&mut self, clients: &mut C, args: &[Bytes]) -> Run<i32> {
        // licence check, not ported. (Inside it: softForceSymbols("forceSymbols"), 0x080c52a0,
        // which returns at once for that word; its body only forces 133 functions to be
        // linked in.)
        let Some(locale) = self.w.setlocale(6, None) else {
            self.w.out(rt(0x08678fdc));
            return Err(Exit(1));
        };
        self.w.setlocale(6, Some(b"C"));
        self.w.setlocale(6, Some(&locale));
        self.run_creative(clients, args)?;
        Ok(0)
    }

    /// [soft 0x080c5cd0 main]: `exit(realmain(argc, argv))`: the status the program ends
    /// with.
    pub fn main<C: Clients<W>>(&mut self, clients: &mut C, args: &[Bytes]) -> i32 {
        match self.realmain(clients, args) {
            Ok(status) => status,
            Err(Exit(status)) => status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_rt::text as rt_text;
    use crate::env::Recorder;
    use crate::server::ScriptedClients;

    fn args(list: &[&str]) -> Vec<Bytes> {
        list.iter().map(|a| a.as_bytes().to_vec()).collect()
    }

    fn parse(list: &[&str], script: &str) -> (Run<CommandLine>, App<Recorder>) {
        let mut a = App::new(Recorder::from_script(script.as_bytes()));
        a.g.program_name = b"soft".to_vec();
        let r = a.parse_command_line(&args(list));
        (r, a)
    }

    #[test]
    fn options_of_the_interactive_program() {
        let (c, a) = parse(&["soft", "/res", "-f", "-c", "-B", "-G", "-s", "my.sts", "-k", "k.sks", "-d", "DB", "-L", "sc.dsc", "-C", "blue.col"], "");
        let c = c.unwrap();
        assert_eq!(c.resources.as_deref(), Some(&b"/res"[..]));
        assert_eq!((c.setup_given, c.setup_file.as_deref()), (true, Some(&b"my.sts"[..])));
        assert_eq!(c.keys_file.as_deref(), Some(&b"k.sks"[..]));
        assert_eq!(c.database.as_deref(), Some(&b"DB"[..]));
        assert_eq!((c.latest_version, c.latest_scene, c.scene.as_deref()), (true, false, Some(&b"sc.dsc"[..])));
        assert_eq!(c.color.as_deref(), Some(&b"blue"[..]));
        assert_eq!((a.g.foreground, a.g.coredump, a.g.base_licence, a.g.light_licence, a.g.light_licence2), (1, 1, 1, 1, 1));
    }

    #[test]
    fn defaults_and_options_without_their_argument() {
        let (c, _) = parse(&["soft"], "");
        assert_eq!(c.unwrap(), CommandLine { keys_file: Some(KEYS_RSRC.to_vec()), ..CommandLine::default() });
        // -s, -l and -C may come without a name
        let (c, _) = parse(&["soft", "-l", "-s", "-C"], "");
        let c = c.unwrap();
        assert!(c.latest_scene && c.setup_given && c.color.is_none());
        assert_eq!(c.setup_file.as_deref(), Some(SETUP_RSRC));
        // only the second character counts; the last path wins
        let (c, a) = parse(&["soft", "/a", "-fast", "/b"], "");
        assert_eq!((c.unwrap().resources, a.g.foreground), (Some(b"/b".to_vec()), 1));
    }

    #[test]
    fn usage_errors_end_with_status_0_and_a_message() {
        // the help texts and messages are read from the user's install (unit rta)
        si_rt::skip_without_install!();
        for (list, message) in [
            (&["soft", "-s", "x.sts"][..], rt_text(0x08679840)),
            (&["soft", "-k"], "The -k option need a filename as argument.\n"),
            (&["soft", "-k", "-f"], "The -k option need a filename as argument.\n"),
            (&["soft", "-d"], "The -d option need a filename as argument.\n"),
            (&["soft", "-d", "DB"], rt_text(0x08679900)),
            (&["soft", "-C", "c"], "Please use the -C option as the last option,\nor give the path of the ressource files before.\n"),
            (&["soft", "/res", "-D", "lib", "sym"], "The -D option must be the only one.\n"),
            (&["soft", "-D", "lib"], "The -D option needs a filename and a symbol.\n"),
            (&["soft", "-D", "lib", "-x", "y"], "The -D option needs a filename and a symbol.\n"),
        ] {
            let (r, a) = parse(list, "");
            assert_eq!(r, Err(Exit(0)), "{list:?}");
            assert_eq!(String::from_utf8_lossy(&a.w.stderr), message, "{list:?}");
        }
    }

    #[test]
    fn help_unknown_options_and_the_version() {
        // the help texts and messages are read from the user's install (unit rta)
        si_rt::skip_without_install!();
        let (r, a) = parse(&["soft", "-h"], "time=-1");
        assert_eq!(r, Err(Exit(0)));
        assert!(a.w.stderr.starts_with(b"\nsoft 4.0\n(c) 1986-1993 Avid Technology, Inc.  All rights reserved.\n\nUsage: soft [<resources path>] \n"));
        let (r, a) = parse(&["soft", "-x"], "time=-1");
        assert_eq!(r, Err(Exit(0)));
        assert_eq!(a.w.stdout, b"soft: illegal option -x\n");
        let (r, a) = parse(&["soft", "-"], "time=-1");
        assert_eq!((r, &a.w.stdout[..]), (Err(Exit(0)), &b"soft: illegal option -\n"[..]));
        let (r, a) = parse(&["soft", "-v"], "");
        assert!(r.is_ok());
        assert_eq!(a.w.stdout, [id_string(), b"\n"].concat());
    }

    #[test]
    fn a_stand_alone_effect_gets_its_library_name_completed() {
        for (file, library) in [("fx", "fx.so"), ("/a.b/fx", "/a.b/fx.so"), ("fx.", "fx.so"), ("fx.so", "fx.so"), ("fx.x.y", "fx.x.y")] {
            let (r, a) = parse(&["soft", "-D", file, "run", "one", "two"], "SI_cusRunStandalone=7");
            assert_eq!(r, Err(Exit(7)));
            assert_eq!(a.w.lines(), [format!("SI_cusRunStandalone(\"run\",3,\"{library}\")")]);
            assert_eq!(a.g.cus_standalone, 1);
        }
    }

    #[test]
    fn the_batch_renderer_is_entered_from_the_option_loop() {
        // the help texts and messages are read from the user's install (unit rta)
        si_rt::skip_without_install!();
        let (r, a) = parse(&["soft", "/res", "-R"], "time=-1");
        assert_eq!(r, Err(Exit(0)));
        assert_eq!(a.g.standalone_renderer, 1);
        assert!(String::from_utf8_lossy(&a.w.stderr).contains("Usage: soft [<resources path>] -R <scene>\n"));
        // without a resource path it is found from the program's own path
        let (r, a) = parse(&["/usr/soft/bin/soft", "-R", "scene", "-q"], "$realpath=/usr/soft/bin/soft;$cwd=/w;mem_have_input_database=1;SI_dbLoadScene=1;time=-1");
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(a.g.resdir.as_deref(), Some(&b"/usr/soft/bin/../rsrc"[..]));
        assert!(String::from_utf8_lossy(&a.w.stderr).contains("***Error: Unknown option -q\n"));
    }

    #[test]
    fn the_whole_sequence_runs_against_the_recorder() {
        // the help texts and messages are read from the user's install (unit rta)
        si_rt::skip_without_install!();
        let script = "$cwd=/work;$locale=C;$user=u;SI_checkHardware=1;hw.xmax=1279;hw.ymax=1023;hw.planes=24;hw.irisf=2;hw.appf=1;\
                      SI_canvasReadColor=1;SI_canvasRead=16,17,18,19,20;winopen=1;SI_dbCheckLocalDatabase=1;client.events=2;qtest=0;time=0,40";
        let mut world = Recorder::from_script(script.as_bytes());
        world.files.insert(b"/work/Reflect.pic".to_vec(), Vec::new());
        let mut a = App::new(world);
        let status = a.main(&mut ScriptedClients::default(), &args(&["soft", "/res"]));
        assert_eq!(status, 0);
        let lines = a.w.lines();
        let at = |what: &str| lines.iter().position(|l| l.starts_with(what)).unwrap_or_else(|| panic!("no call of {what}"));
        // the order of a few landmarks of the sequence
        let order = ["SI_menuTableInit", "signal(3,0)", "SI_initDictionaries", "SI_checkHardware", "winopen", "copyRightDisplay", "SI_cusInit",
            "SI_canvasRead(\"/work/Model.rsrc\"", "SI_canvasRead(\"/work/Tools.rsrc\"", "SI_windowReadInit", "ortho2(fbf000000,f449ff000,fbf000000,f447fe000)",
            "SI_initSupraKeys", "systemInit_viewports", "SI_canvasDraw(16,1)", "SI_statusOpenFile", "dlopen", "SI_dbCheckLocalDatabase", "SI_modelClient", "SI_cusExit(4)", "gexit"];
        for pair in order.windows(2) {
            assert!(at(pair[0]) < at(pair[1]), "{} before {}", pair[0], pair[1]);
        }
        assert_eq!(a.g.network.map(|n| (n.module, n.canvas)), [(0, 16), (1, 17), (2, 18), (3, 19), (4, 20)]);
        assert_eq!((a.g.network[0].started, a.g.active, a.g.emergency.enabled), (1, 0, 1));
    }
}
