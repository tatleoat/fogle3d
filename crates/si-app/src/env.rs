// SPDX-License-Identifier: MIT
//! Everything outside this crate that the start-up code and the main loop call: one trait per
//! subsystem, and one recording stand-in ([`Recorder`]) that implements them all.
//!
//! A trait method carries the name of the original function it stands for, so a recorded
//! call reads like the original's call and the real part can be plugged in under the same
//! name. The recorder writes every call down with its arguments, in order, and answers from
//! a script; `app-emucheck` compares that record with the one the harness makes of the
//! original (`re/tools/si-app/emu_app.py` writes its lines by the same rules).
//!
//! The process never ends inside a port: the original's `exit(n)` is the error value
//! [`Exit`], handed up to whoever started the sequence.

use std::collections::{BTreeMap, HashMap, VecDeque};

/// The original called `exit` with this status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit(pub i32);

/// The result of a routine that may end the program.
pub type Run<T> = Result<T, Exit>;

/// A C string held by the program: bytes, no zero byte inside.
pub type Bytes = Vec<u8>;

/// A pointer the program only passes on: a dialog, a canvas, a database record.
/// 0 is the null pointer.
pub type Handle = u32;

/// What a signal is set to ([soft 0x080d1038 SI_signalInit] uses these two).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalAction {
    /// `SIG_DFL`: the system's default, a core file for the faults.
    Default,
    /// The program's handler: `SI_serverExit(signal, 0)`.
    ServerExit,
}

// ------------------------------------------------------------------------------------------
// arguments as they are written into the record

/// One argument of a recorded call.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i64),
    /// A C string; `None` is a null pointer.
    Str(Option<Bytes>),
    /// A float, recorded by its bits.
    F32(f32),
    /// A pointer into the caller's own memory: recorded as `*`, not compared.
    Ptr,
}

/// A pointer to a record of a part that is not ported (a database record, a dictionary, a
/// table of custom effects): the call is made with it, the port has nothing to say about
/// what it points to. Recorded as `*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P;

impl From<P> for Arg {
    fn from(_: P) -> Arg {
        Arg::Ptr
    }
}
impl From<bool> for Arg {
    fn from(v: bool) -> Arg {
        Arg::Int(v as i64)
    }
}
impl From<i32> for Arg {
    fn from(v: i32) -> Arg {
        Arg::Int(v as i64)
    }
}
impl From<i16> for Arg {
    fn from(v: i16) -> Arg {
        Arg::Int(v as i64)
    }
}
impl From<u16> for Arg {
    fn from(v: u16) -> Arg {
        Arg::Int(v as i64)
    }
}
impl From<u32> for Arg {
    /// Handles and other words are recorded as the signed number the stack word is.
    fn from(v: u32) -> Arg {
        Arg::Int(v as i32 as i64)
    }
}
impl From<f32> for Arg {
    fn from(v: f32) -> Arg {
        Arg::F32(v)
    }
}
impl From<&[u8]> for Arg {
    fn from(v: &[u8]) -> Arg {
        Arg::Str(Some(v.to_vec()))
    }
}
impl From<Option<&[u8]>> for Arg {
    fn from(v: Option<&[u8]>) -> Arg {
        Arg::Str(v.map(|s| s.to_vec()))
    }
}

/// Text as the record shows it: printable ASCII as it is, `"`, `\` and everything else as
/// `\xNN`.
pub fn quote(text: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &c in text {
        if (0x20..0x7f).contains(&c) && c != b'"' && c != b'\\' {
            out.push(c);
        } else {
            out.extend_from_slice(format!("\\x{c:02x}").as_bytes());
        }
    }
    out.push(b'"');
}

impl Arg {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Arg::Int(v) => out.extend_from_slice(v.to_string().as_bytes()),
            Arg::Str(None) => out.extend_from_slice(b"null"),
            Arg::Str(Some(s)) => quote(s, out),
            Arg::F32(f) => out.extend_from_slice(format!("f{:08x}", f.to_bits()).as_bytes()),
            Arg::Ptr => out.push(b'*'),
        }
    }
}

/// A recorded call's answer as the type the trait method returns.
pub trait FromAnswer {
    fn from_answer(v: i64) -> Self;
}
impl FromAnswer for i32 {
    fn from_answer(v: i64) -> i32 {
        v as i32
    }
}
impl FromAnswer for i16 {
    /// A `short` result is the low half of eax.
    fn from_answer(v: i64) -> i16 {
        v as i16
    }
}
impl FromAnswer for u32 {
    fn from_answer(v: i64) -> u32 {
        v as u32
    }
}
impl FromAnswer for bool {
    fn from_answer(v: i64) -> bool {
        v != 0
    }
}

// ------------------------------------------------------------------------------------------
// the recorder

/// The stand-in for every part that is not ported: writes each call down and answers from a
/// script.
///
/// The script is text, `name=v,v,…;name=v;$key=text;…`. A name's values answer its calls in
/// order and the last one repeats; a name without values answers 0. `$key=text` entries are
/// the strings some calls answer with: `$env.NAME` an environment variable, `$cwd`,
/// `$realpath`, `$home`, `$user`, `$locale`, `$tempnam`, `$tmpnam`, `$popen`, `$ctime`.
/// Files live in [`Recorder::files`] by name, as the harness's per-case files do.
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    /// The calls, one per line: `name(argument,argument,…)`.
    pub trace: Vec<u8>,
    script: HashMap<String, VecDeque<i64>>,
    strs: HashMap<String, Bytes>,
    /// The files that exist, by path.
    pub files: BTreeMap<Bytes, Bytes>,
    /// Paths written, created or removed, in order of first change.
    pub changed: Vec<Bytes>,
    /// What went to the standard output and the standard error stream.
    pub stdout: Bytes,
    pub stderr: Bytes,
    /// The module of each canvas, as start-up stores it in the canvas records.
    pub canvas_modules: HashMap<Handle, i16>,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// A recorder that answers from `script` (see the type's description).
    pub fn from_script(script: &[u8]) -> Recorder {
        let mut r = Recorder::default();
        for entry in script.split(|&c| c == b';') {
            let Some(eq) = entry.iter().position(|&c| c == b'=') else { continue };
            let (name, value) = (&entry[..eq], &entry[eq + 1..]);
            let name = String::from_utf8_lossy(name).into_owned();
            if let Some(key) = name.strip_prefix('$') {
                r.strs.insert(key.to_string(), value.to_vec());
            } else {
                let values = String::from_utf8_lossy(value)
                    .split(',')
                    .filter(|t| !t.is_empty())
                    .map(|t| t.trim().parse::<i64>().unwrap_or_else(|_| panic!("script value `{t}` of {name}")))
                    .collect();
                r.script.insert(name, values);
            }
        }
        r
    }

    /// Sets the answers of one name.
    pub fn answer(&mut self, name: &str, values: &[i64]) -> &mut Recorder {
        self.script.insert(name.to_string(), values.iter().copied().collect());
        self
    }

    /// Sets a string answer (`env.NAME`, `cwd`, …).
    pub fn string(&mut self, key: &str, value: &[u8]) -> &mut Recorder {
        self.strs.insert(key.to_string(), value.to_vec());
        self
    }

    /// The next answer of `name`: its values in order, the last one for ever; 0 without any.
    pub fn next(&mut self, name: &str) -> i64 {
        match self.script.get_mut(name) {
            Some(q) if q.len() > 1 => q.pop_front().unwrap(),
            Some(q) => q.front().copied().unwrap_or(0),
            None => 0,
        }
    }

    /// The string answer `key`, if the script has one.
    pub fn str_of(&self, key: &str) -> Option<Bytes> {
        self.strs.get(key).cloned()
    }

    /// Writes a call down without answering it.
    pub fn note(&mut self, name: &str, args: &[Arg]) {
        self.trace.extend_from_slice(name.as_bytes());
        self.trace.push(b'(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.trace.push(b',');
            }
            a.write(&mut self.trace);
        }
        self.trace.extend_from_slice(b")\n");
    }

    /// Writes a call down and answers it from the script.
    pub fn call(&mut self, name: &str, args: &[Arg]) -> i64 {
        self.note(name, args);
        self.next(name)
    }

    /// The record as lines of text.
    pub fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.trace).lines().map(str::to_string).collect()
    }

    fn touch(&mut self, path: &[u8]) {
        if !self.changed.iter().any(|p| p == path) {
            self.changed.push(path.to_vec());
        }
    }
}

/// Declares a subsystem trait whose methods are plain calls of original functions, and
/// implements it for [`Recorder`]: each call is written down under the method's name and
/// answered from the script.
macro_rules! subsystem {
    ($(#[$m:meta])* pub trait $t:ident { $( $(#[$fm:meta])* fn $f:ident ( $($a:ident : $ty:ty),* $(,)? ) $(-> $r:ty)? ; )* }) => {
        $(#[$m])*
        #[allow(non_snake_case)]
        pub trait $t {
            $( $(#[$fm])* fn $f(&mut self $(, $a: $ty)*) $(-> $r)?; )*
        }
        #[allow(non_snake_case)]
        impl $t for Recorder {
            $( fn $f(&mut self $(, $a: $ty)*) $(-> $r)? {
                let v = self.call(stringify!($f), &[$(Arg::from($a)),*]);
                subsystem!(@ret v $(, $r)?)
            } )*
        }
    };
    (@ret $v:ident) => { { let _ = $v; } };
    (@ret $v:ident, $r:ty) => { <$r as FromAnswer>::from_answer($v) };
}

// ------------------------------------------------------------------------------------------
// the C library and the system

/// The C library and the operating system as the ported code uses them. Nothing here
/// starts a process, installs a handler or touches a real file in the recorder: `system`
/// and `popen` are written down and answered.
pub trait Sys {
    /// `getenv`.
    fn getenv(&mut self, name: &[u8]) -> Option<Bytes>;
    /// `getpid`.
    fn getpid(&mut self) -> i32;
    /// `getcwd(buffer, size)`: the directory, or `None` when it does not fit or fails.
    fn getcwd(&mut self, size: usize) -> Option<Bytes>;
    /// `access(path, mode)`: 0 or −1.
    fn access(&mut self, path: &[u8], mode: i32) -> i32;
    /// `realpath(path, buffer)`.
    fn realpath(&mut self, path: &[u8]) -> Option<Bytes>;
    /// `time(NULL)`.
    fn time(&mut self) -> i32;
    /// `localtime(&t)->tm_year`.
    fn local_year(&mut self, t: i32) -> i32;
    /// `ctime(&t)`: 24 characters and a new line.
    fn ctime(&mut self, t: i32) -> Bytes;
    /// `signal(number, action)`.
    fn signal(&mut self, number: i32, action: SignalAction);
    /// `system(command)`: the command is not run by the recorder.
    fn system(&mut self, command: &[u8]) -> i32;
    /// `popen(command, "r")`, one `fgets` of up to 0x2000 bytes, `pclose`: the line read,
    /// or `None` when the pipe could not be opened.
    fn popen_line(&mut self, command: &[u8]) -> Option<Bytes>;
    /// `putenv`.
    fn putenv(&mut self, assignment: &[u8]);
    /// `unlink`.
    fn unlink(&mut self, path: &[u8]) -> i32;
    /// `stat(path)`: `Some(true)` for a directory, `None` when the call fails.
    fn stat_is_dir(&mut self, path: &[u8]) -> Option<bool>;
    /// `tempnam(directory, prefix)`.
    fn tempnam(&mut self, directory: &[u8], prefix: &[u8]) -> Option<Bytes>;
    /// `tmpnam(buffer)`.
    fn tmpnam(&mut self) -> Bytes;
    /// `getpwuid(getuid())->pw_dir`; `None` when there is no such entry.
    fn home_of_user(&mut self) -> Option<Bytes>;
    /// `getpwuid(geteuid())->pw_name`.
    fn name_of_user(&mut self) -> Bytes;
    /// `setlocale(category, locale)`.
    fn setlocale(&mut self, category: i32, locale: Option<&[u8]>) -> Option<Bytes>;
    /// `mallopt`.
    fn mallopt(&mut self, parameter: i32, value: i32);
    /// `dlopen`, `dlsym`, `dlclose`.
    fn dlopen(&mut self, name: &[u8], mode: i32) -> Handle;
    fn dlsym(&mut self, library: Handle, name: &[u8]) -> Handle;
    fn dlclose(&mut self, library: Handle);
    /// A call of an entry point of a loaded library with one text argument.
    fn dlsym_call(&mut self, entry: Handle, text: &[u8]);
    /// `strerror(errno)`.
    fn last_error(&mut self) -> Bytes;
    /// `setbell(1)` and `ringbell()` of the GL layer's stubs are not system calls; `usleep`
    /// is.
    fn usleep(&mut self, microseconds: u32);

    /// Bytes for the standard output, the standard error stream.
    fn out(&mut self, text: &[u8]);
    fn err(&mut self, text: &[u8]);

    /// The whole content of a file (`fopen(path, "r")` and the reads), if it exists.
    fn read_file(&mut self, path: &[u8]) -> Option<Bytes>;
    /// `fopen(path, "w")` (or `"a"` with `append`) followed by `text`; false when the file
    /// cannot be opened.
    fn write_file(&mut self, path: &[u8], text: &[u8], append: bool) -> bool;
}

impl Sys for Recorder {
    fn getenv(&mut self, name: &[u8]) -> Option<Bytes> {
        self.note("getenv", &[name.into()]);
        self.strs.get(&format!("env.{}", String::from_utf8_lossy(name))).cloned()
    }
    fn getpid(&mut self) -> i32 {
        self.call("getpid", &[]) as i32
    }
    fn getcwd(&mut self, size: usize) -> Option<Bytes> {
        self.note("getcwd", &[(size as i32).into()]);
        // the directory with its zero byte must fit, as in the C library
        self.strs.get("cwd").filter(|d| d.len() < size).cloned()
    }
    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        self.note("access", &[path.into(), mode.into()]);
        if self.files.contains_key(path) {
            0
        } else {
            -1
        }
    }
    fn realpath(&mut self, path: &[u8]) -> Option<Bytes> {
        self.note("realpath", &[path.into()]);
        self.strs.get("realpath").cloned()
    }
    fn time(&mut self) -> i32 {
        self.call("time", &[]) as i32
    }
    fn local_year(&mut self, t: i32) -> i32 {
        self.call("localtime", &[t.into()]) as i32
    }
    fn ctime(&mut self, t: i32) -> Bytes {
        self.note("ctime", &[t.into()]);
        self.strs.get("ctime").cloned().unwrap_or_else(|| b"Thu Jan  1 00:00:00 1970\n".to_vec())
    }
    fn signal(&mut self, number: i32, action: SignalAction) {
        let a: i32 = match action {
            SignalAction::Default => 0,
            SignalAction::ServerExit => 1,
        };
        self.note("signal", &[number.into(), a.into()]);
    }
    fn system(&mut self, command: &[u8]) -> i32 {
        self.call("system", &[command.into()]) as i32
    }
    fn popen_line(&mut self, command: &[u8]) -> Option<Bytes> {
        self.note("popen", &[command.into()]);
        self.strs.get("popen").cloned()
    }
    fn putenv(&mut self, assignment: &[u8]) {
        self.note("putenv", &[assignment.into()]);
    }
    fn unlink(&mut self, path: &[u8]) -> i32 {
        self.note("unlink", &[path.into()]);
        if self.files.remove(path).is_some() {
            self.touch(path);
            0
        } else {
            -1
        }
    }
    fn stat_is_dir(&mut self, path: &[u8]) -> Option<bool> {
        match self.call("stat", &[path.into()]) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn tempnam(&mut self, directory: &[u8], prefix: &[u8]) -> Option<Bytes> {
        self.note("tempnam", &[directory.into(), prefix.into()]);
        self.strs.get("tempnam").cloned()
    }
    fn tmpnam(&mut self) -> Bytes {
        self.note("tmpnam", &[]);
        self.strs.get("tmpnam").cloned().unwrap_or_default()
    }
    fn home_of_user(&mut self) -> Option<Bytes> {
        self.note("getpwuid", &[]);
        self.strs.get("home").cloned()
    }
    fn name_of_user(&mut self) -> Bytes {
        self.note("getpwuid", &[]);
        self.strs.get("user").cloned().unwrap_or_default()
    }
    fn setlocale(&mut self, category: i32, locale: Option<&[u8]>) -> Option<Bytes> {
        self.note("setlocale", &[category.into(), locale.into()]);
        self.strs.get("locale").cloned()
    }
    fn mallopt(&mut self, parameter: i32, value: i32) {
        self.note("mallopt", &[parameter.into(), value.into()]);
    }
    fn dlopen(&mut self, name: &[u8], mode: i32) -> Handle {
        self.call("dlopen", &[name.into(), mode.into()]) as u32
    }
    fn dlsym(&mut self, library: Handle, name: &[u8]) -> Handle {
        self.call("dlsym", &[library.into(), name.into()]) as u32
    }
    fn dlclose(&mut self, library: Handle) {
        self.note("dlclose", &[library.into()]);
    }
    fn dlsym_call(&mut self, entry: Handle, text: &[u8]) {
        self.note("library", &[entry.into(), text.into()]);
    }
    fn last_error(&mut self) -> Bytes {
        self.strs.get("strerror").cloned().unwrap_or_else(|| b"No such file or directory".to_vec())
    }
    fn usleep(&mut self, microseconds: u32) {
        self.note("usleep", &[microseconds.into()]);
    }
    fn out(&mut self, text: &[u8]) {
        self.stdout.extend_from_slice(text);
    }
    fn err(&mut self, text: &[u8]) {
        self.stderr.extend_from_slice(text);
    }
    fn read_file(&mut self, path: &[u8]) -> Option<Bytes> {
        self.files.get(path).cloned()
    }
    fn write_file(&mut self, path: &[u8], text: &[u8], append: bool) -> bool {
        let f = self.files.entry(path.to_vec()).or_default();
        if !append {
            f.clear();
        }
        f.extend_from_slice(text);
        self.touch(path);
        true
    }
}

// ------------------------------------------------------------------------------------------
// the IRIS GL layer (finding 0016; `crates/si-gl` is its port)

subsystem! {
    /// The IRIS GL entry points the start-up code, the event reader and the clean-up call.
    /// `si-gl`'s `IrisGl` has every one of them under the same name; this crate does not
    /// depend on it so that the sequence can be checked without a display.
    pub trait Gl {
        fn prefposition(x1: i32, x2: i32, y1: i32, y2: i32);
        fn noborder();
        fn foreground();
        fn winopen(name: &[u8]) -> i32;
        fn wintitle(name: &[u8]);
        fn winset(window: i32);
        fn winclose(window: i32);
        fn winpush();
        fn doublebuffer();
        fn overlay(planes: i32);
        fn RGBmode();
        fn cmode();
        fn gconfig();
        fn frontbuffer(on: i32);
        fn RGBcolor(r: i32, g: i32, b: i32);
        fn cpack(colour: i32);
        fn color(index: i32);
        fn clear();
        fn subpixel(on: i32);
        fn glcompat(mode: i32, value: i32);
        fn viewport(left: i32, right: i32, bottom: i32, top: i32);
        fn ortho2(left: f32, right: f32, bottom: f32, top: f32);
        fn writemask(mask: i32);
        fn font(index: i32);
        fn drawmode(mode: i32);
        fn swapbuffers();
        fn gexit();
        fn curson();
        fn setbell(mode: i32);
        fn ringbell();
        fn qdevice(device: i32);
        fn unqdevice(device: i32);
        fn tie(button: i32, valuator1: i32, valuator2: i32);
        fn qtest() -> i32;
        fn getgdesc(inquiry: i32) -> i32;
        fn strwidth(text: &[u8]) -> i32;
        fn sboxfi(x1: i32, y1: i32, x2: i32, y2: i32);
    }
}

/// The IRIS GL calls that hand something back through a pointer.
pub trait GlQueue {
    /// `qread(&value)`: the device, and the value written through the pointer.
    fn qread(&mut self) -> (i32, i16);
    /// `qcontrol(0x15, 0, NULL, 2, out)`: the two shorts the layer reports as the key
    /// repeat settings.
    fn qcontrol_get(&mut self) -> [i16; 2];
    /// `qcontrol(0x16, 2, in, 0, NULL)` with a long of 0: key repeat off.
    fn qcontrol_set(&mut self, value: i32);
}

impl GlQueue for Recorder {
    fn qread(&mut self) -> (i32, i16) {
        // the script's `qread` values come in pairs: device, value
        self.note("qread", &[Arg::Ptr]);
        let device = self.next("qread") as i32;
        let value = self.next("qread.value") as i16;
        (device, value)
    }
    fn qcontrol_get(&mut self) -> [i16; 2] {
        self.note("qcontrol", &[0x15.into(), 0.into(), 0.into(), 2.into(), Arg::Ptr]);
        [self.next("qcontrol.0") as i16, self.next("qcontrol.1") as i16]
    }
    fn qcontrol_set(&mut self, _value: i32) {
        self.note("qcontrol", &[0x16.into(), 2.into(), Arg::Ptr, 0.into(), 0.into()]);
    }
}

// ------------------------------------------------------------------------------------------
// the user interface above the GL layer

subsystem! {
    /// The program's own user interface: canvases and their resource files, dialogs, the
    /// status line, colour maps, fonts, windows. None of it is ported. Methods whose names
    /// begin with `mem_` are not calls of the original: they stand for a direct read or
    /// write of a field of a record that belongs to this part.
    pub trait Ui {
        // start-up
        fn SI_menuTableInit();
        fn SI_statusInitPath();
        fn SI_colorMapStore(which: i32);
        fn SI_colorMapRestore(which: i32);
        fn SI_colorMapLoad(path: &[u8]) -> i16;
        fn SI_colorGetMap();
        fn i_seterror(handler: P);
        fn SI_mouseWarpGet(first: P, second: P);
        fn SI_mouseWarpSet(first: i32, second: i32);
        fn SI_graphicInit();
        fn copyRightDisplay(resources: &[u8]) -> i32;
        fn SI_readTextureFile(name: P, flag: i32);
        fn fontSoftimageDefine(path: &[u8], info: P, font: i16);
        fn fontSoftimageLogoDefine(path: &[u8], font: i16);
        fn SI_canvasReadColor(path: &[u8], report: i32) -> i16;
        fn SI_canvasRead(path: &[u8], report: i32, custom: P, custom_count: i32, module: i32) -> Handle;
        fn SI_windowReadInit(path: &[u8]) -> i32;
        fn SI_initDefault();
        fn SI_initSupraKeys();
        fn SI_tabletInit() -> i32;
        fn SI_tabletDisplayError();
        fn SI_tabletGetData();
        fn SI_fsliderDraw(frame: i32);
        fn SI_statusDeleteFile();
        fn SI_statusOpenFile();
        fn si_statusCloseFile();
        fn SI_statusDisplayMessage(text: &[u8], level: i32);
        fn SI_setupLoadFromServer(resources: &[u8], file: Option<&[u8]>);
        fn SI_cursorChange(cursor: i32);
        fn SI_stopActivity();
        // canvases and the module box
        fn si_modeBoxInit();
        fn SI_modeBoxRefresh();
        fn SI_canvasDraw(canvas: Handle, redraw: i16);
        fn SI_refreshSystem(port: Handle, list: Handle, what: i32);
        fn mem_canvas_right_edge(canvas: Handle) -> i16;
        fn mem_canvas_share(canvas: Handle, first: Handle);
        // dialogs
        fn SI_dialogOpen(id: i32) -> Handle;
        fn SI_dialogClose(dialog: Handle);
        fn SI_dialogFree(dialog: Handle);
        fn SI_dialogDraw(dialog: Handle);
        fn SI_dialDraw(dialog: Handle, flag: i32);
        fn SI_dialogGetEvent(dialog: Handle) -> i16;
        fn SI_setItemTitle(dialog: Handle, item: i32, text: &[u8]);
        fn SI_setTexed(dialog: Handle, item: i32, text: &[u8]);
        fn SI_sliderSet(dialog: Handle, item: i32, text: &[u8], redraw: i32);
        fn mem_dialog_set_default(dialog: Handle, button: i16);
        fn si_dialogCloseId(id: i32);
        fn si_dialIsOpenWinId(window: i32) -> i16;
        fn SI_dialogSetDatabase(dictionary: P, directory: P, input: P, chapters: P, flag: i32);
        // windows
        fn mem_template_set_window(window: i16);
        fn mem_template_3d_redraws() -> i16;
        fn mem_set_rgb_active(on: i32);
        fn mem_zbuffer() -> i32;
        // events above the queue
        fn si_resetKeyboard();
        fn checkKeyboard() -> i32;
        fn SI_getQuickKeySequence(mode: i32, index: i32) -> i32;
        fn SI_eventInitPos();
    }
}

/// The user-interface calls that hand something back through a pointer.
pub trait UiText {
    /// `SI_getTexed(dialog, item, &text)`: the text of an editable item, a fresh copy.
    fn get_texed(&mut self, dialog: Handle, item: i32) -> Option<Bytes>;
    /// The name of a canvas (the string at +0x04 of the record).
    fn mem_canvas_name(&mut self, canvas: Handle) -> Bytes;
    /// The module a canvas belongs to (the short at +0x00 of the record), read and set.
    fn mem_canvas_module(&mut self, canvas: Handle) -> i16;
    fn mem_canvas_set_module(&mut self, canvas: Handle, module: i16);
    /// `SI_windowGetNext(&port)`: the next floating window, or `None` at the end of the
    /// list: its record (which may be null), whether it is open (+0x64) and its IRIS GL
    /// window (+0x50).
    fn window_get_next(&mut self) -> Option<(Handle, i32, i32)>;
    /// `SI_checkHardware()`: looks at the display and fills in the screen size, the bit
    /// planes, the write mask and the font numbers; 0 when the display cannot be used.
    fn check_hardware(&mut self, g: &mut crate::state::Globals) -> i16;
}

impl UiText for Recorder {
    fn get_texed(&mut self, dialog: Handle, item: i32) -> Option<Bytes> {
        self.note("SI_getTexed", &[dialog.into(), item.into(), Arg::Ptr]);
        self.strs.get("texed").cloned()
    }
    fn mem_canvas_name(&mut self, canvas: Handle) -> Bytes {
        // the scripted canvases are named after their handles
        format!("canvas{canvas}").into_bytes()
    }
    fn mem_canvas_module(&mut self, canvas: Handle) -> i16 {
        match self.canvas_modules.get(&canvas) {
            Some(m) => *m,
            None => self.next("mem_canvas_module") as i16,
        }
    }
    fn mem_canvas_set_module(&mut self, canvas: Handle, module: i16) {
        self.canvas_modules.insert(canvas, module);
    }
    fn window_get_next(&mut self) -> Option<(Handle, i32, i32)> {
        self.note("SI_windowGetNext", &[Arg::Ptr]);
        if self.next("SI_windowGetNext") == 0 {
            return None;
        }
        Some((self.next("window.port") as u32, self.next("window.open") as i32, self.next("window.id") as i32))
    }
    fn check_hardware(&mut self, g: &mut crate::state::Globals) -> i16 {
        let ok = self.call("SI_checkHardware", &[]) as i16;
        g.xmaxscreen = self.next("hw.xmax") as i16;
        g.ymaxscreen = self.next("hw.ymax") as i16;
        g.bit_planes = self.next("hw.planes") as i32;
        g.olbit_planes = self.next("hw.olplanes") as i32;
        g.system_wm = self.next("hw.wm") as u16;
        g.appf = self.next("hw.appf") as i16;
        g.irisf = self.next("hw.irisf") as i16;
        g.logof = self.next("hw.logof") as i16;
        ok
    }
}

// ------------------------------------------------------------------------------------------
// the five modules

subsystem! {
    /// What the frame asks of the modules and of the scene besides the module loops.
    pub trait Modules {
        fn SI_scnGetMode() -> i32;
        fn SI_actorGetMode() -> i16;
        fn SI_mtrGetMode() -> i32;
        fn SI_toolsGetMode() -> i32;
        fn mem_mdl_mod() -> i32;
        // systemInit and the stand-alone renderer
        fn SI_grafListCreate();
        fn SI_grafListAdd() -> i32;
        fn mem_set_scene_graf_list(list: i32);
        fn mem_graf_list_show_nothing();
        fn systemInit_viewports();
        fn SI_omodelBinaryRead(path: &[u8], model: Handle, flag: i32) -> Handle;
        fn SI_initCamDefault();
        fn SI_matterInit();
        fn SI_metaballInit();
        fn SI_dopeSheetInit();
        fn SI_audioMngContainerInit();
        fn SI_fcvScrollInit();
        fn SI_initParticle();
        fn SI_initWave();
        fn SI_displayModeInit();
        fn SI_initFieldGuide();
        fn SI_newCamera(name: &[u8], flag: i32) -> Handle;
        fn SI_graflistAddData(list: P, camera: Handle, kind: i32, flag: i32);
        fn SI_camAndScnToOrigin();
        fn SI_sceneRefresh();
    }
}

// ------------------------------------------------------------------------------------------
// the database and the scene

subsystem! {
    /// Dictionaries, databases and scenes (`si-db` and `si-scene` hold the ports of most of
    /// these; the frame only needs them called in order).
    pub trait Database {
        fn SI_initDictionaries(resources: Option<&[u8]>, a: i32, b: i32);
        fn mem_have_input_database() -> bool;
        fn mem_have_output_database() -> bool;
        fn mem_database_valid(database: Handle) -> bool;
        fn mem_set_input_database(database: Handle);
        fn mem_set_output_database(database: Handle);
        fn mem_output_follows_input();
        fn SI_dbSetChapter(database: P, chapter: &[u8]);
        fn SI_dbfilepathBuild(database: P);
        fn SI_dbChapterCheckAccess(database: P, mode: i32) -> i16;
        fn SI_dbDirGetDatabaseFromName(directory: P, name: &[u8]) -> Handle;
        fn SI_dbCheckLocalDatabase(directory: P) -> i16;
        fn SI_dbInfoInit(database: P);
        fn SI_dbLoadScene(dictionary: P, database: P, scene: &[u8]) -> i16;
        fn SI_dbSaveScene(database: P, dictionary: P, a: i32, b: i32, name: Option<&[u8]>, c: i32) -> i16;
        fn SI_dbCreateExtDatabase(database: P);
        fn SI_system(command: &[u8]) -> i32;
    }
}

/// The database calls that answer with text.
pub trait DatabaseText {
    /// The chapter directory of the output database (`+0x14` of its record) after
    /// `SI_dbfilepathBuild`.
    fn output_chapter_path(&mut self) -> Bytes;
    /// The name (`+0x08`) and the path (`+0x0c`) of a database record: 0 the input, 1 the
    /// output database.
    fn database_name(&mut self, which: i32) -> Bytes;
    fn database_path(&mut self, which: i32) -> Bytes;
    /// `SI_dbGetLatestScene(database)`; database 0 is the input database, 2 a record made
    /// for the directory given.
    fn latest_scene(&mut self, database: i32, directory: Option<&[u8]>) -> Option<Bytes>;
    /// `SI_dbfileFindLatestVersion(input database, name)`.
    fn latest_version(&mut self, name: &[u8]) -> Bytes;
    /// `SI_strStripSuffix`, `SI_strStripDir` ([`si-db`] has the ports).
    fn strip_suffix(&mut self, name: &[u8]) -> Bytes;
    fn strip_dir(&mut self, name: Option<&[u8]>) -> Option<Bytes>;
}

impl DatabaseText for Recorder {
    fn output_chapter_path(&mut self) -> Bytes {
        self.strs.get("dbout.chapter").cloned().unwrap_or_default()
    }
    fn database_name(&mut self, which: i32) -> Bytes {
        self.strs.get(if which == 0 { "dbin.name" } else { "dbout.name" }).cloned().unwrap_or_default()
    }
    fn database_path(&mut self, which: i32) -> Bytes {
        self.strs.get(if which == 0 { "dbin.path" } else { "dbout.path" }).cloned().unwrap_or_default()
    }
    fn latest_scene(&mut self, database: i32, directory: Option<&[u8]>) -> Option<Bytes> {
        self.note("SI_dbGetLatestScene", &[database.into(), directory.into()]);
        self.strs.get("latest.scene").cloned()
    }
    fn latest_version(&mut self, name: &[u8]) -> Bytes {
        self.note("SI_dbfileFindLatestVersion", &[0.into(), name.into()]);
        self.strs.get("latest.version").cloned().unwrap_or_else(|| name.to_vec())
    }
    fn strip_suffix(&mut self, name: &[u8]) -> Bytes {
        self.note("SI_strStripSuffix", &[name.into()]);
        match name.iter().rposition(|&c| c == b'.') {
            Some(i) => name[..i].to_vec(),
            None => name.to_vec(),
        }
    }
    fn strip_dir(&mut self, name: Option<&[u8]>) -> Option<Bytes> {
        self.note("SI_strStripDir", &[name.into()]);
        name.map(|n| match n.iter().rposition(|&c| c == b'/') {
            Some(i) => n[i + 1..].to_vec(),
            None => n.to_vec(),
        })
    }
}

// ------------------------------------------------------------------------------------------
// plug-ins

subsystem! {
    /// Custom effects (`SI_cus…`): finding 0001, "Plug-ins".
    pub trait Custom {
        fn SI_cusInit();
        fn SI_cusSetupDynamicMenus();
        fn SI_cusExit(reason: i32);
        fn SI_cusDsoGetRefreshFlag() -> bool;
        fn SI_cusCallCustomFunctions(state: i32, what: i32);
        fn SI_cusRunStandalone(symbol: &[u8], count: i32, first: Option<&[u8]>) -> i32;
    }
}

// ------------------------------------------------------------------------------------------
// the renderers

subsystem! {
    /// The renderers as the batch front end drives them.
    pub trait Renderer {
        fn SI_isMr19Enable() -> i16;
        fn SI_setmi2Output(on: i32);
        fn SI_sequenceWireFrame();
        fn SI_sequenceHiddenLine(mode: i32);
        fn SI_sequenceRender();
        fn SI_sequenceRenderMental(flag: i32);
    }
}

/// Everything outside this crate, in one bound.
pub trait World: Sys + Gl + GlQueue + Ui + UiText + Modules + Database + DatabaseText + Custom + Renderer + crate::event::EventHandlers + crate::userpref::KeyTables {}
impl<T: Sys + Gl + GlQueue + Ui + UiText + Modules + Database + DatabaseText + Custom + Renderer + crate::event::EventHandlers + crate::userpref::KeyTables> World for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_answers_in_order_and_repeats_the_last() {
        let mut r = Recorder::from_script(b"qtest=3,0;$env.SI_LOG=6;getpid=41");
        assert_eq!(r.qtest(), 3);
        assert_eq!(r.qtest(), 0);
        assert_eq!(r.qtest(), 0);
        assert_eq!(r.getenv(b"SI_LOG"), Some(b"6".to_vec()));
        assert_eq!(r.getenv(b"HOME"), None);
        assert_eq!(r.getpid(), 41);
        assert_eq!(r.lines(), ["qtest()", "qtest()", "qtest()", "getenv(\"SI_LOG\")", "getenv(\"HOME\")", "getpid()"]);
    }

    #[test]
    fn arguments_are_written_as_the_harness_writes_them() {
        let mut r = Recorder::new();
        r.SI_setItemTitle(0x7000, 2, b"a \"b\"\n");
        r.ortho2(-0.5, 1279.5, -0.5, 1023.5);
        r.SI_cusRunStandalone(b"f", -1, None);
        assert_eq!(
            r.lines(),
            ["SI_setItemTitle(28672,2,\"a \\x22b\\x22\\x0a\")", "ortho2(fbf000000,f449ff000,fbf000000,f447fe000)", "SI_cusRunStandalone(\"f\",-1,null)"]
        );
    }
}
