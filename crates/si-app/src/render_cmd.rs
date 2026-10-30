// SPDX-License-Identifier: MIT
//! The batch renderer's front end: `standAloneRender` and its helpers in `sys_main.c`.
//! `soft -R <scene> [options]` loads a scene without opening a window, applies the options
//! to the render set-up stored with the scene and renders the sequence.

use crate::env::{Bytes, Exit, Run, World, P};
use crate::image::adapter::{place, scratch, AppOutside};
use crate::image::{batch, g};
use crate::state::App;
use si_x87::F80;

/// What the original does when it reads an argument that is not there: `argv[argc]` is a
/// null pointer, and it is dereferenced. The port ends the sequence with this status
/// instead of faulting.
pub const FAULT: Exit = Exit(-11);

/// `SI_defaultControl` (0x08781800, 0x600 bytes): the render set-up. Only the fields the
/// front end writes are known; the offsets are the original's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderControl {
    /// +0x0fa: the renderer: 0 wire frame, 1 and 2 hidden line, 4 native, 9 mental ray
    /// 1.9, 10 mental ray 2.1.
    pub renderer: i16,
    /// +0x178, +0x17a: set to 0 and 1 by `SI_callRender`.
    pub word178: i16,
    pub word17a: i16,
    /// +0x17c: the output file name (`-n`, or the old name without its directory after
    /// `-o` and `-D`).
    pub out_name: Option<Bytes>,
    /// +0x180, +0x184: the pre-frame and post-frame scripts (`-p`, `-P`).
    pub pre_script: Option<Bytes>,
    pub post_script: Option<Bytes>,
    /// +0x1f4, +0x1f6, +0x1f8: first frame, last frame, step (`-s`).
    pub first: i16,
    pub last: i16,
    pub step: i16,
    /// +0x20e: skip pictures that exist (`-j`).
    pub skip_existing: i16,
    /// +0x226: render a sub-region (`-b`); +0x22c left, +0x230 one minus right, +0x238
    /// bottom, +0x234 one minus top.
    pub subregion: i16,
    pub sub_left: f32,
    pub sub_right_inv: f32,
    pub sub_top_inv: f32,
    pub sub_bottom: f32,
    /// +0x244: complete unfinished frames (`-c`).
    pub complete: i16,
    /// +0x24c: render the depth channel (`-z`).
    pub z_channel: i16,
    /// +0x3d8: write a `.mi` file (`-m`, `-M`); +0x3da: one file per frame (`-M`).
    pub mi_output: i16,
    pub mi_per_frame: i16,
    /// +0x494: the `.mi` file's name.
    pub mi_name: Option<Bytes>,
    /// +0x586 … +0x58e: the global switches of `-G`: 2D texture, 3D texture, reflection,
    /// refraction, shadow.
    pub switches: [i16; 5],
    /// +0x59a, +0x59c: the picture's width and height (`-r`).
    pub xres: i16,
    pub yres: i16,
}

/// C's `atoi` cut to a `short`, as the options are stored.
fn atoi16(s: &[u8]) -> i16 {
    atoi(s) as i16
}

/// C's `atoi` (`strtol(s, NULL, 10)` cut to an `int`).
pub fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    let v = if neg { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `sscanf(text, "%d%n", &value, &used) == 1 && used == strlen(text)`: the whole text is
/// one decimal integer (blanks in front and a sign are allowed, as `%d` allows them).
pub(crate) fn whole_integer(text: &[u8]) -> bool {
    let mut i = 0;
    while i < text.len() && (text[i] == b' ' || (9..=13).contains(&text[i])) {
        i += 1;
    }
    if i < text.len() && (text[i] == b'-' || text[i] == b'+') {
        i += 1;
    }
    let digits = i;
    while i < text.len() && text[i].is_ascii_digit() {
        i += 1;
    }
    i > digits && i == text.len()
}

/// C's `atof` for what a command line holds: the longest decimal floating-point prefix.
/// (Hexadecimal floats, `inf` and `nan` are not recognised here; `strtod` does.)
pub fn atof(s: &[u8]) -> f64 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let start = i;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut digits = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < s.len() && (s[j] == b'-' || s[j] == b'+') {
            j += 1;
        }
        if j < s.len() && s[j].is_ascii_digit() {
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    std::str::from_utf8(&s[start..i]).ok().and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0)
}

/// The renderer's call that reads its set-up questions' answers: it takes them from the
/// standard input, or with `-l` from the command line, moving the argument cursor.
#[allow(non_snake_case)]
pub trait RenderSetup {
    /// `readSetup(stream)`: `echo` is false when the questions go to `/dev/null`. `next`
    /// is the index of the current argument and `left` the count of arguments left, as the
    /// original's `argv` and `argc` globals (0x087821cc, 0x087821c8).
    fn readSetup(&mut self, echo: bool, args: &[Bytes], next: &mut usize, left: &mut i32);
}

impl RenderSetup for crate::env::Recorder {
    fn readSetup(&mut self, echo: bool, _args: &[Bytes], next: &mut usize, left: &mut i32) {
        let used = self.call("readSetup", &[(echo as i32).into()]) as i32;
        *next = (*next as i64 + used as i64) as usize;
        *left -= used;
    }
}

impl<W: World> App<W> {
    // adapter: not in place
    /// `SI_standDisplayVersion (0x080ca910)`: the image port
    /// ([`crate::image::batch::stand_display_version`]) on a scratch image.
    pub fn stand_display_version(&mut self, name: &[u8], extra: Option<&[u8]>, version: Option<&[u8]>, from: i32, to: i32) {
        let mut m = scratch();
        let (n, e, v) = (place(&mut m, Some(name)), place(&mut m, extra), place(&mut m, version));
        batch::stand_display_version(&mut m, &mut AppOutside(&mut self.w), n, e, v, from, to);
    }

    // adapter: not in place
    /// `usage (0x080cdafc)`: the image port ([`crate::image::batch::usage`]).
    pub fn usage(&mut self, code: i32, name: &[u8]) -> Exit {
        let mut m = scratch();
        let n = place(&mut m, Some(name));
        batch::usage(&mut m, &mut AppOutside(&mut self.w), code, n)
    }

    // adapter: not in place
    /// `error (0x080d059c)`: the image port ([`crate::image::batch::error`]).
    pub fn render_error(&mut self, message: &[u8], detail: &[u8]) -> Exit {
        let mut m = scratch();
        let name = place(&mut m, Some(&self.g.program_name));
        m[g::PROGRAM_NAME] = name.0;
        let (a, b) = (place(&mut m, Some(message)), place(&mut m, Some(detail)));
        batch::error(&mut m, &mut AppOutside(&mut self.w), a, b)
    }

    // adapter: not in place
    /// `testsubRegion (0x080cf4f8)`: the image port ([`crate::image::batch::test_sub_region`])
    /// on the four numbers of the struct form's set-up.
    pub fn test_sub_region(&mut self) -> Run<()> {
        use crate::image::batch::ctl;
        let mut m = scratch();
        let name = place(&mut m, Some(&self.g.program_name));
        m[g::PROGRAM_NAME] = name.0;
        let c = &self.g.default_control;
        let base = g::SI_DEFAULT_CONTROL.0;
        for (at, v) in [(ctl::SUB_LEFT, c.sub_left), (ctl::SUB_RIGHT_INV, c.sub_right_inv), (ctl::SUB_TOP_INV, c.sub_top_inv), (ctl::SUB_BOTTOM, c.sub_bottom)] {
            m.set_f32(base + at, v);
        }
        batch::test_sub_region(&mut m, &mut AppOutside(&mut self.w))
    }

    /// [soft 0x080cdffc standAloneRender]: the batch renderer. `args[0]` is the `-R`
    /// argument itself, `args[1]` the scene; `resources` the resource directory.
    ///
    /// 1. Without a scene: usage, status 0.
    /// 2. `<resources>/soft.init`, if it exists, is run with `csh` (through `SI_system`).
    /// 3. Dictionaries (twice, with the path globals set from the current directory in
    ///    between), waves, the graph lists, a camera `Perspective_def`, the manipulation
    ///    and animation parameters, materials, meta-balls, custom effects.
    /// 4. A `.dsc` at the end of the scene's name is cut off. A first pass over the
    ///    options takes `-d <database>` (input and output database), `-L` (latest
    ///    version) and counts `-D`.
    /// 5. The scene is loaded; failure ends with status −1.
    /// 6. The options are applied in order (see the finding for the table). An unknown
    ///    option, or an argument that is not an option, is an error: usage, status 1.
    /// 7. The renderer runs ([`App::call_render`]) and the routine returns.
    ///
    /// The renderer number is adjusted by 13 minus `get_init_value(6)` before the render:
    /// a constant kept behind the licence checks, 0 on the full product's path.
    pub fn stand_alone_render(&mut self, args: &[Bytes], resources: Option<Bytes>) -> Run<()>
    where
        W: RenderSetup,
    {
        let mut latest = false;
        let mut mr_given = false;
        let mut out_db_given = 0;
        self.g.standalone_renderer = 1;
        let mut next = 0usize; // argv
        let mut left = args.len() as i32; // argc
        let name = self.g.program_name.clone();
        if left == 1 {
            return Err(self.usage(0, &name));
        }
        // sprintf("%s/soft.init", NULL) prints "(null)" in the C library
        let res: Bytes = resources.clone().unwrap_or_else(|| b"(null)".to_vec());
        let init = [&res[..], b"/soft.init"].concat();
        if self.w.access(&init, 0) == 0 {
            self.w.SI_system(&[b"csh ".as_slice(), &res, b"/soft.init"].concat());
        }
        next += 1;
        let mut scene = args.get(next).cloned().ok_or(FAULT)?;
        left -= 1;
        self.w.SI_initDictionaries(resources.as_deref(), 1, 1);
        if !self.w.mem_have_input_database() {
            return Err(Exit(-1));
        }
        self.g.cwd = self.w.getcwd(0xff);
        self.set_paths_from_cwd();
        self.w.SI_initDictionaries(resources.as_deref(), 1, 1);
        if !self.w.mem_have_input_database() {
            return Err(Exit(-1));
        }
        self.w.SI_initWave();
        self.w.SI_grafListCreate();
        let list = self.w.SI_grafListAdd();
        if list > 0 {
            self.w.mem_set_scene_graf_list(list);
        }
        let camera = self.w.SI_newCamera(b"Perspective_def", 1);
        self.g.proc_data.camera = camera;
        self.w.SI_graflistAddData(P, camera, 4, 0);
        self.manipulation_init();
        self.animation_info_init();
        self.w.SI_matterInit();
        self.w.SI_metaballInit();
        self.w.SI_cusInit();
        if scene.len() >= 4 && scene.ends_with(b".dsc") {
            // shorter names make the original compare bytes in front of the string
            scene.truncate(scene.len() - 4);
        }

        // first pass: the databases and -L
        let (mut p, mut n) = (next, left);
        loop {
            n -= 1;
            if n <= 0 {
                break;
            }
            p += 1;
            let a = args.get(p).ok_or(FAULT)?;
            if a.first() != Some(&b'-') {
                continue;
            }
            match a.get(1).copied().unwrap_or(0) {
                b'L' => latest = true,
                b'D' => {
                    out_db_given += 1;
                    p += 1;
                    n -= 1;
                }
                b'd' => {
                    p += 1;
                    let db_name = args.get(p).ok_or(FAULT)?.clone();
                    n -= 1;
                    let db = self.w.SI_dbDirGetDatabaseFromName(P, &db_name);
                    if db != 0 && self.w.mem_database_valid(db) {
                        self.w.mem_set_input_database(db);
                        self.w.mem_set_output_database(db);
                    } else {
                        self.w.err(&[b"ERROR: input database [".as_slice(), &db_name, b"] cannot be accessed\n"].concat());
                    }
                }
                _ => {}
            }
        }
        if latest {
            self.w.SI_dbSetChapter(P, b"SCENES");
            self.w.SI_dbfilepathBuild(P);
            scene = self.w.latest_version(&scene);
            self.w.err(&[b"Latest version of scene used: [".as_slice(), &scene, b"]\n"].concat());
        }
        let path = self.w.database_path(0);
        self.w.err(&[b"Reading scene from Database [".as_slice(), &path, b"]\n"].concat());
        if self.w.SI_dbLoadScene(P, P, &scene) == 0 {
            let db_name = self.w.database_name(0);
            self.w.err(&[b"ERROR: Scene [".as_slice(), &scene, b"] could not be found in database [", &db_name, b"]\n"].concat());
            return Err(Exit(-1));
        }

        // second pass: everything else, in order
        loop {
            left -= 1;
            if left <= 0 {
                break;
            }
            next += 1;
            let a = args.get(next).ok_or(FAULT)?.clone();
            let at = |i: usize| args.get(next + i).map(|s| s.first() == Some(&b'-'));
            if a.first() != Some(&b'-') {
                return Err(self.render_error(b"Unknown option", &a));
            }
            match a.get(1).copied().unwrap_or(0) {
                b'd' => {
                    left -= 1;
                    next += 1;
                }
                b'L' => {}
                b's' => {
                    let mut ok = true;
                    if left <= 3 {
                        ok = false;
                    } else {
                        let (d1, d2, d3) = (at(1).ok_or(FAULT)?, at(2).ok_or(FAULT)?, at(3).ok_or(FAULT)?);
                        if d1 || d2 || d3 {
                            if d3 {
                                ok = false;
                            } else {
                                if d1 && !whole_integer(&args[next + 1][1..]) {
                                    ok = false;
                                }
                                if ok && d2 && !whole_integer(&args[next + 2][1..]) {
                                    return Err(self.render_error(b"-s option needs three integer parameters", b""));
                                }
                            }
                        }
                    }
                    if !ok {
                        return Err(self.render_error(b"-s option needs three integer parameters", b""));
                    }
                    self.g.default_control.first = atoi16(&args[next + 1]);
                    self.g.default_control.last = atoi16(&args[next + 2]);
                    self.g.default_control.step = atoi16(&args[next + 3]);
                    next += 3;
                    left -= 3;
                }
                b'f' => {
                    if left <= 1 || at(1).ok_or(FAULT)? {
                        return Err(self.render_error(b"-f option needs one integer parameter", b""));
                    }
                    next += 1;
                    let v = atoi16(&args[next]);
                    self.g.report_frequency = if v > 0 { v } else { 10 };
                    left -= 1;
                }
                b'r' => {
                    if left <= 2 || at(1).ok_or(FAULT)? || at(2).ok_or(FAULT)? {
                        return Err(self.render_error(b"-r option needs two integer parameters", b""));
                    }
                    let x = atoi16(&args[next + 1]);
                    let y = atoi16(&args[next + 2]);
                    next += 2;
                    self.g.default_control.xres = x;
                    self.g.default_control.yres = y;
                    if !(x > 0 && y > 0) {
                        return Err(self.render_error(b"-r option needs two positive integer parameters", b""));
                    }
                    left -= 2;
                }
                b'v' => self.g.verbose_option = 1,
                b'l' => {
                    if self.g.line_option == 0 {
                        self.g.line_option = 1;
                        // without -v the questions go to /dev/null (to the standard output
                        // if that cannot be opened)
                        let echo = self.g.verbose_option != 0 || !self.w.write_file(b"/dev/null", b"", false);
                        self.w.readSetup(echo, args, &mut next, &mut left);
                    }
                }
                b'S' => self.w.readSetup(true, args, &mut next, &mut left),
                b'G' => {
                    if left <= 1 || at(1).ok_or(FAULT)? {
                        return Err(self.render_error(b"-G option need a parameter", b""));
                    }
                    next += 1;
                    let digits = args[next].clone();
                    if digits.len() != 5 {
                        return Err(self.render_error(b"-G option, the parameter need 5 digits", b""));
                    }
                    for (k, d) in digits.iter().enumerate() {
                        self.g.default_control.switches[k] = (*d != b'0') as i16;
                    }
                    left -= 1;
                }
                b'j' => self.g.default_control.skip_existing = 1,
                b'c' => self.g.default_control.complete = 1,
                b'z' => self.g.default_control.z_channel = 1,
                b'o' => {
                    next += 1;
                    if out_db_given == 0 {
                        let dir = args.get(next).ok_or(FAULT)?.clone();
                        self.g.picture_path = Some(dir);
                        let old = self.g.default_control.out_name.take();
                        self.g.default_control.out_name = Some(self.w.strip_dir(old.as_deref()).ok_or(FAULT)?);
                    }
                    left -= 1;
                }
                b'n' => {
                    next += 1;
                    self.g.default_control.out_name = Some(args.get(next).ok_or(FAULT)?.clone());
                    left -= 1;
                }
                b'b' => {
                    let r = self.g.default_control.renderer;
                    if r != 4 && r != 9 && r != 10 {
                        return Err(self.render_error(b"-b option needs mental ray or internal renderer stored with scene", b""));
                    }
                    if left <= 4 {
                        return Err(self.usage(0, &name));
                    }
                    let c = &mut self.g.default_control;
                    c.subregion = 1;
                    c.sub_left = atof(&args[next + 1]) as f32;
                    c.sub_right_inv = (F80::ONE - F80::from_f32(atof(&args[next + 2]) as f32)).to_f32();
                    c.sub_bottom = atof(&args[next + 3]) as f32;
                    c.sub_top_inv = (F80::ONE - F80::from_f32(atof(&args[next + 4]) as f32)).to_f32();
                    next += 4;
                    left -= 4;
                    self.test_sub_region()?;
                }
                b'D' => {
                    next += 1;
                    let db_name = args.get(next).ok_or(FAULT)?.clone();
                    left -= 1;
                    let db = self.w.SI_dbDirGetDatabaseFromName(P, &db_name);
                    if db != 0 && self.w.mem_database_valid(db) {
                        self.w.mem_set_output_database(db);
                        self.w.SI_dbSetChapter(P, b"RENDER_PICTURES");
                        self.w.SI_dbfilepathBuild(P);
                        if self.w.SI_dbChapterCheckAccess(P, 2) != 0 {
                            self.g.picture_path = Some(self.w.output_chapter_path());
                        } else {
                            self.g.picture_path = self.g.cwd.clone();
                            let n = self.w.database_name(1);
                            self.w.err(&[b"ERROR: chapter RENDER_PICTURES of database ".as_slice(), &n, b" not found:\n"].concat());
                            let pp = self.g.picture_path.clone().unwrap_or_else(|| b"(null)".to_vec());
                            self.w.err(&[b" Rendering images to ".as_slice(), &pp, b"\n"].concat());
                        }
                    } else {
                        self.g.picture_path = self.g.cwd.clone();
                        self.w.err(&[b"ERROR: output database [".as_slice(), &db_name, b"] cannot be accessed\n"].concat());
                        let pp = self.g.picture_path.clone().unwrap_or_else(|| b"(null)".to_vec());
                        self.w.err(&[b" Rendering images to ".as_slice(), &pp, b"\n"].concat());
                    }
                    let old = self.g.default_control.out_name.take();
                    self.g.default_control.out_name = Some(self.w.strip_dir(old.as_deref()).ok_or(FAULT)?);
                }
                b't' => self.w.SI_camAndScnToOrigin(),
                b'M' | b'm' => {
                    if &a[1..] == b"mr" {
                        left -= 1;
                        if left <= 0 {
                            return Err(self.render_error(b"you must specify mental ray version [19 | 21] when using the -mr option", b""));
                        }
                        next += 1;
                        let v = args.get(next).ok_or(FAULT)?;
                        if v == b"19" {
                            self.w.SI_setmi2Output(0);
                            self.g.default_control.renderer = 9;
                            mr_given = true;
                        } else if v == b"21" {
                            self.w.SI_setmi2Output(1);
                            self.g.default_control.renderer = 10;
                            mr_given = true;
                        } else {
                            return Err(self.render_error(b"you must specify mental ray version [19 | 21] when using the -mr option", b""));
                        }
                    } else {
                        if !mr_given {
                            match self.g.default_control.renderer {
                                10 => self.w.SI_setmi2Output(1),
                                9 => self.w.SI_setmi2Output(0),
                                _ => {
                                    self.w.SI_setmi2Output(1);
                                    self.g.default_control.renderer = 10;
                                }
                            }
                        }
                        let c = &mut self.g.default_control;
                        c.mi_output = 1;
                        c.mi_per_frame = (a[1] == b'M') as i16;
                        if left - 1 > 0 && !at(1).ok_or(FAULT)? {
                            left -= 1;
                            next += 1;
                            self.g.default_control.mi_name = Some(args[next].clone());
                        } else {
                            self.g.default_control.mi_name = Some(b"out.mi".to_vec());
                        }
                    }
                }
                b'P' | b'p' => {
                    let pre = a[1] == b'p';
                    if left - 1 <= 0 || at(1).ok_or(FAULT)? {
                        return Err(self.usage(0, &name));
                    }
                    next += 1;
                    let script = args[next].clone();
                    left -= 1;
                    if self.w.access(&script, 4) != 0 {
                        let which: &[u8] = if pre { b"Pre" } else { b"Post" };
                        self.w.err(&[b"Unknown ".as_slice(), which, b"-frame script ", &script, b"\n"].concat());
                    } else if pre {
                        self.g.default_control.pre_script = Some(script);
                    } else {
                        self.g.default_control.post_script = Some(script);
                    }
                }
                b'h' => return Err(self.usage(0, &name)),
                _ => return Err(self.render_error(b"Unknown option", &a)),
            }
        }
        // 13 - get_init_value(6): 0 on the full product's path
        let adjust = 13 - self.get_init_value(6);
        self.g.default_control.renderer = (self.g.default_control.renderer as i32 + adjust) as i16;
        self.call_render();
        Ok(())
    }
}

/// A usage line with the program's name put in for `%s`.
pub fn line_with_name(line: &[u8], name: &[u8]) -> Bytes {
    match line.windows(2).position(|w| w == b"%s") {
        Some(i) => [&line[..i], name, &line[i + 2..]].concat(),
        None => line.to_vec(),
    }
}
