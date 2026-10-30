// SPDX-License-Identifier: MIT
//! Checks of the tenth pass (finding 0012, section 19), on the vectors of
//! `re/tools/si-render/emu/gen_pass10.py`: the routines below a picture file on the memory
//! image (`byteswap.c`, `rle.c`, `si_fatal`), the chunk memory in sequences, the jitter
//! tables, the wrap flags of a 2D texture and the dictionary's two picture routines.
//!
//! A case is loaded into a `si_mem::Mem` as the seventh pass's are (`outer::load`), the
//! port runs on the image, and the image is held against everything the case says the
//! original left (`outer::compare`). What the original asked of the C library's file
//! calls and of routines of other files is replayed from the case's record, in the one
//! order the original made the calls in ([`Shared`]); the files themselves are a
//! [`MemFiles`], which has the harness's rules.
//!
//! The four older vector files of `rle.c` (`gen_out.py`) are run through the image form
//! as well ([`old_mixed`], [`old_write`], [`old_init`], [`old_close`]), beside the form on
//! values that they have always checked.

use std::cell::RefCell;
use std::rc::Rc;

use crate::outer;
use crate::prep::mutate;
use si_mem::{Mem, Names};
use si_render::host::{FileHost, MemFiles};
use si_render::img::{f, g, Outside};
use si_render::{byteswap, data, renutil_image, rle_image};
use si_x87::vectors::{Call, Case, Check, Verdict, Word};

/// An argument as the port passed it.
enum Arg {
    W(u32),
    T(Vec<u8>),
}

/// The recorded calls of a case, replayed to the port from both sides it calls out
/// through.
pub struct Shared<'a> {
    c: &'a Case,
    next: usize,
    pub wrong: Option<String>,
}

/// The calls `si_x87::trig` replays itself: they are not this tape's.
fn is_libm(name: &str) -> bool {
    matches!(name, "sin" | "cos" | "tan" | "asin" | "acos" | "sinf" | "cosf" | "tanf" | "asinf" | "acosf" | "atanf" | "exp" | "expf" | "log" | "log10" | "pow" | "powf" | "cbrt" | "fpatan")
}

impl<'a> Shared<'a> {
    pub fn new(c: &'a Case) -> Rc<RefCell<Shared<'a>>> {
        let mut s = Shared { c, next: 0, wrong: None };
        s.skip();
        Rc::new(RefCell::new(s))
    }

    fn skip(&mut self) {
        while self.c.calls.get(self.next).is_some_and(|k| is_libm(&k.name)) {
            self.next += 1;
        }
    }

    fn fail(&mut self, text: String) -> u32 {
        if self.wrong.is_none() {
            self.wrong = Some(text);
        }
        0
    }

    /// The next recorded call must be `name` with these arguments; its recorded answer.
    fn replay(&mut self, name: &str, got: &[Arg]) -> u32 {
        let at = self.next;
        let Some(k) = self.c.calls.get(at) else {
            return self.fail(format!("call {at}: the port calls {name}, the original nothing more"));
        };
        self.next += 1;
        self.skip();
        if k.name != name {
            return self.fail(format!("call {at}: original {}, port {name}", k.name));
        }
        // EMUCHECK_MUTATE=calls: one bit of every argument the port passes is flipped
        // before it is held against the record (every case with calls must then differ)
        let flip = std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "calls");
        let flipped: Vec<Arg> = got
            .iter()
            .map(|a| match a {
                Arg::W(v) => Arg::W(if flip { v ^ 1 } else { *v }),
                Arg::T(t) => {
                    let mut t = t.clone();
                    if flip {
                        t.push(b'!');
                    }
                    Arg::T(t)
                }
            })
            .collect();
        let got = &flipped[..];
        for (i, (want, got)) in k.args.iter().zip(got).enumerate() {
            let same = match (want, got) {
                (Word::Ref { name: buffer, offset: 0 }, Arg::T(t)) if buffer.starts_with("str") => &self.c.output(buffer).to_bytes() == t,
                // a pointer into the case's memory or to a local: not held
                (Word::Ref { .. }, Arg::W(_)) => true, // (held by the caller, which knows the buffers)
                (w, Arg::W(v)) => w.value().map(|x| x as u32) == Some(*v),
                // (a null text is recorded as the number 0)
                (w, Arg::T(t)) => w.value() == Some(0) && t.is_empty(),
            };
            if !same {
                let shown = match got {
                    Arg::W(v) => format!("{v:#010x}"),
                    Arg::T(t) => format!("{:?}", String::from_utf8_lossy(t)),
                };
                return self.fail(format!("call {at} {name}: argument {i}: original {want:?}, port {shown}"));
            }
        }
        k.result.value().unwrap_or(0) as u32
    }

    /// The recorded call the port is about to make, if it is `name`: whether its argument
    /// `i` was recorded as a text.
    fn text_at(&self, i: usize) -> bool {
        matches!(self.c.calls.get(self.next).and_then(|k: &Call| k.args.get(i)), Some(Word::Ref { name, .. }) if name.starts_with("str"))
    }

    pub fn finish(&self) -> Result<(), String> {
        if let Some(w) = &self.wrong {
            return Err(w.clone());
        }
        if self.next != self.c.calls.len() {
            return Err(format!("calls: original {}, port {}", self.c.calls.len(), self.next));
        }
        Ok(())
    }
}

/// The routines of other files, by address.
fn callee(addr: u32) -> Option<&'static str> {
    Some(match addr {
        f::SI_FATAL => "si_fatal",
        f::SI_ADD_ICON_TO_PIC => "SI_addIconToPic",
        f::FONT => "font",
        f::RGBCOLOR => "RGBcolor",
        f::STRWIDTH => "strwidth",
        f::GETDISPLAYMODE => "getdisplaymode",
        f::VIEWPORT => "viewport",
        f::CMOV2I => "cmov2i",
        f::CHARSTR => "charstr",
        f::SWAPBUFFERS => "swapbuffers",
        f::SI_GET_STATUS_FILE => "SI_getStatusFile",
        f::PRINTF => "printf",
        f::FPRINTF => "fprintf",
        f::FFLUSH => "fflush",
        f::SLEEP => "sleep",
        f::LONGJMP => "longjmp",
        f::SI_WARNING => "SI_warning",
        f::SI_DBFILENAME_REMOVE_PATH => "SI_dbfilenameRemovePath",
        f::SI_DBFILENAME_ADD_SUFFIX => "SI_dbfilenameAddSuffix",
        f::SI_DB_SCN_GET_LIST_NODE => "SI_dbScnGetListNode",
        f::SI_DB_LIST_NODE_FREE => "SI_dbListNodeFree",
        f::SI_ELEMDICT_ADD_ELEM => "SI_elemdictAddElem",
        f::SI_ELEMDICT_GET_ELEM => "SI_elemdictGetElem",
        f::SI_ELEMDICT_FIND_ELEM_BY_DATA => "SI_elemdictFindElemByData",
        f::SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG => "SI_elemdictElementSetDeleteFlag",
        f::PUSHVIEWPORT => "pushviewport",
        f::QTEST => "qtest",
        f::QREAD => "qread",
        f::GETBUTTON => "getbutton",
        f::SI_TABLET_GET_DATA => "SI_tabletGetData",
        f::SI_TABLET_GET_BUTTON_UP_EVENT => "SI_tabletGetButtonUpEvent",
        f::POPVIEWPORT => "popviewport",
        f::FRONTBUFFER => "frontbuffer",
        f::TIME => "time",
        f::TIMES => "times",
        f::GETENV => "getenv",
        f::SI_STATUS_DISPLAY_MESSAGE => "SI_statusDisplayMessage",
        _ => return None,
    })
}

/// The port's calls of other files' routines, answered from the record.
pub struct Hooks<'a> {
    pub shared: Rc<RefCell<Shared<'a>>>,
    /// The case's buffers (name, address), for the recorded arguments and answers that
    /// are pointers.
    names: Vec<(String, u32)>,
}

impl<'a> Hooks<'a> {
    pub fn new(shared: Rc<RefCell<Shared<'a>>>) -> Hooks<'a> {
        Hooks { shared, names: Vec::new() }
    }

    /// A recorded word as the address it stands for in the port's image.
    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Ref { name, offset } => {
                let base = match name.strip_prefix("heap").and_then(|k| k.parse::<usize>().ok()) {
                    Some(k) => m.heap_blocks().get(k).map(|b| b.0),
                    None => self.names.iter().find(|n| n.0 == *name).map(|n| n.1),
                };
                base.map(|b| b.wrapping_add(*offset))
            }
            w => w.value().map(|v| v as u32),
        }
    }
}

impl Outside for Hooks<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        let Some(name) = callee(addr) else {
            return self.shared.borrow_mut().fail(format!("the port calls {addr:#010x}, which the check does not know"));
        };
        // a stand-in of the generator's that allocates does so here too, at the same point
        let made = (name == "SI_dbfilenameAddSuffix").then(|| {
            let mut t = m.cstr(args[0]);
            t.push(b'.');
            t.extend_from_slice(&m.cstr(args[1]));
            let p = m.malloc(t.len() as u32 + 1);
            m.put_cstr(p, &t);
            p
        });
        let recorded: Option<Call> = {
            let s = self.shared.borrow();
            s.c.calls.get(s.next).cloned()
        };
        // arguments recorded as pointers into the case's memory
        if let Some(k) = &recorded {
            for (i, a) in args.iter().enumerate() {
                if let Some(w @ Word::Ref { name: buffer, .. }) = k.args.get(i) {
                    if !buffer.starts_with("str") && k.name == name && self.resolve(m, w) != Some(*a) {
                        return self.shared.borrow_mut().fail(format!("{name}: argument {i}: original {w:?}, port {a:#010x}"));
                    }
                }
            }
        }
        let got: Vec<Arg> = {
            let s = self.shared.borrow();
            args.iter().enumerate().map(|(i, a)| if s.text_at(i) { Arg::T(m.cstr(*a)) } else { Arg::W(*a) }).collect()
        };
        let answer = self.shared.borrow_mut().replay(name, &got);
        // `time(&t)` and `times(&tms)`: the generator's stand-ins store what they answer
        // through the pointer (the time, and `tms_utime`), and log no argument
        if matches!(name, "time" | "times") && args.first().is_some_and(|p| *p != 0) {
            m.set_u32(args[0], answer);
        }
        match (&recorded, made) {
            (_, Some(p)) => p,
            // a stand-in that answers with a pointer recorded it as one argument more
            (Some(k), None) if k.name == name && k.args.len() == args.len() + 1 => self.resolve(m, &k.args[args.len()]).unwrap_or(0),
            _ => answer,
        }
    }
}

/// The port's file calls: `access`, `creat`, `close` and `unlink` answered from the record
/// (the generator planned their answers), the rest by a file system in memory.
pub struct Files<'a> {
    shared: Rc<RefCell<Shared<'a>>>,
    pub mem: MemFiles,
}

impl FileHost for Files<'_> {
    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        self.shared.borrow_mut().replay("access", &[Arg::T(path.to_vec()), Arg::W(mode as u32)]) as i32
    }
    fn fopen(&mut self, path: &[u8], mode: &[u8]) -> u32 {
        self.mem.fopen(path, mode)
    }
    fn fwrite(&mut self, file: u32, bytes: &[u8]) -> u32 {
        self.mem.fwrite(file, bytes)
    }
    fn fread(&mut self, file: u32, n: u32) -> Vec<u8> {
        self.mem.fread(file, n)
    }
    fn rewind(&mut self, file: u32) {
        self.mem.rewind(file)
    }
    fn fflush(&mut self, file: u32) -> i32 {
        self.mem.fflush(file)
    }
    fn ferror(&mut self, file: u32) -> i32 {
        self.mem.ferror(file)
    }
    fn fclose(&mut self, file: u32) -> i32 {
        self.mem.fclose(file)
    }
    fn creat(&mut self, path: &[u8], mode: u32) -> i32 {
        self.shared.borrow_mut().replay("creat", &[Arg::T(path.to_vec()), Arg::W(mode)]) as i32
    }
    fn close(&mut self, fd: i32) -> i32 {
        self.shared.borrow_mut().replay("close", &[Arg::W(fd as u32)]) as i32
    }
    fn unlink(&mut self, path: &[u8]) -> i32 {
        self.shared.borrow_mut().replay("unlink", &[Arg::T(path.to_vec())]) as i32
    }
    fn rename(&mut self, from: &[u8], to: &[u8]) -> i32 {
        self.mem.rename(from, to)
    }
    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        self.mem.read_file(path)
    }
    fn write_file(&mut self, path: &[u8], data: &[u8]) -> bool {
        self.mem.write_file(path, data)
    }
}

/// The case without the fields that are not memory of the image (a `FILE` argument's
/// content, position and state; what a later call of a chain changed) and without its
/// calls, which [`Shared`] replays.
fn quiet(c: &Case, drop: &[&str]) -> Case {
    let keep = |f: &&si_x87::vectors::Field| !drop.contains(&f.name.as_str());
    Case { name: c.name.clone(), inputs: c.inputs.iter().filter(keep).cloned().collect(), outputs: c.outputs.iter().filter(keep).cloned().collect(), calls: Vec::new(), notes: c.notes.clone() }
}

fn same_bytes(what: &str, want: &[u8], got: &[u8]) -> Result<(), String> {
    if want == got {
        return Ok(());
    }
    let at = want.iter().zip(got).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
    Err(format!("{what}: original {} bytes, port {}; first difference at {at}", want.len(), got.len()))
}

/// A picture's bytes against the original's, the comment of a cropped picture's header
/// compared up to its terminating zero (what follows is the original's stack).
fn same_picture(what: &str, want: &[Option<u128>], got: &[u8], cropped: bool) -> Result<(), String> {
    if want.len() != got.len() {
        return Err(format!("{what}: original {} bytes, port {}", want.len(), got.len()));
    }
    let end = if cropped && got.len() >= 88 { 8 + got[8..88].iter().position(|&b| b == 0).unwrap_or(80) + 1 } else { 0 };
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        if cropped && i >= end.min(88) && i < 88 {
            continue;
        }
        if let Some(w) = w {
            if *w as u8 != *g {
                return Err(format!("{what}: byte {i}: original {:02x}, port {g:02x}", *w as u8));
            }
        }
    }
    Ok(())
}

/// Loads the case, runs the port with the record behind it, compares the image and the
/// calls; `after` compares what is not memory.
pub(crate) fn run(
    c: &Case,
    drop: &[&str],
    port: impl FnOnce(&mut Mem, &Names, &mut Hooks, &mut Files) -> Result<Option<(&'static str, u32)>, String>,
    after: impl FnOnce(&Mem, &Files) -> Result<(), String>,
) -> Verdict {
    let q = quiet(c, drop);
    match outer::load(&q) {
        Ok((m, names)) => run_loaded(c, &q, m, names, port, after),
        Err(e) => Verdict::Different(format!("the case does not load: {e}")),
    }
}

/// [`run`] on an image the caller loaded; `q` is the case as far as it is memory.
fn run_loaded(
    c: &Case,
    q: &Case,
    mut m: Mem,
    names: Names,
    port: impl FnOnce(&mut Mem, &Names, &mut Hooks, &mut Files) -> Result<Option<(&'static str, u32)>, String>,
    after: impl FnOnce(&Mem, &Files) -> Result<(), String>,
) -> Verdict {
    data::place_texts(&mut m);
    m.set_strict(true);
    let snapshot = m.snapshot();
    let shared = Shared::new(c);
    let mut hooks = Hooks { shared: shared.clone(), names: names.iter().map(|(n, a, _)| (n.to_string(), a)).collect() };
    let mut files = Files { shared: shared.clone(), mem: MemFiles::new() };
    for field in &c.inputs {
        if let Some(path) = field.name.strip_prefix("file:") {
            files.mem.files.insert(path.as_bytes().to_vec(), field.to_bytes());
        }
    }
    let result = match port(&mut m, &names, &mut hooks, &mut files) {
        Ok(r) => r,
        Err(e) => return Verdict::Different(e),
    };
    let mut tape = outer::Tape::new(q, &names, None);
    let calls = shared.borrow().finish();
    match outer::compare(q, &mut m, &names, &snapshot, result, &mut tape) {
        Verdict::Same => {}
        // (a routine whose only product is its calls: what the calls' self-test makes of
        // them decides)
        Verdict::Skip if calls.is_err() => return Verdict::Different(calls.unwrap_err()),
        other => return other,
    }
    if let Err(e) = calls {
        return Verdict::Different(e);
    }
    match after(&m, &files) {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}

pub(crate) fn at(m: &Mem, names: &Names, name: &str) -> u32 {
    names.addr(m, name).unwrap_or_else(|| panic!("the case has no buffer `{name}`"))
}

const FILE_FIELDS: [&str; 3] = ["f", "f.pos", "f.state"];

/// The files the case says the routine wrote, created or removed, against the host's.
fn same_files(c: &Case, files: &Files, cropped: bool) -> Result<(), String> {
    for field in &c.outputs {
        let Some(path) = field.name.strip_prefix("file:") else { continue };
        let have = files.mem.files.get(path.as_bytes());
        match (field.is_null(), have) {
            (true, None) => {}
            (true, Some(_)) => return Err(format!("{path}: removed in the original, there in the port")),
            (false, None) => return Err(format!("{path}: written in the original, missing in the port")),
            (false, Some(got)) => {
                let want: Vec<Option<u128>> = field.to_bytes().iter().map(|b| Some(*b as u128)).collect();
                same_picture(path, &want, got, cropped)?;
            }
        }
    }
    Ok(())
}

fn swap_check(c: &Case, port: fn(&mut Mem, u32)) -> Verdict {
    run(
        c,
        &[],
        |m, names, _, _| {
            let p = at(m, names, "p");
            port(m, p);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_byteswap_fwrite(c: &Case) -> Verdict {
    run(
        c,
        &FILE_FIELDS,
        |m, names, _, files| {
            let file = files.mem.open_named(b"f", &c.input("f").to_bytes(), b"w");
            let p = at(m, names, "p");
            let r = byteswap::byteswap_fwrite(m, files, p, c.input("size").u32(), c.input("count").u32(), file);
            Ok(Some(("eax", r)))
        },
        |_, files| same_bytes("file", &c.output("f").to_bytes(), &files.mem.files[&b"f".to_vec()]),
    )
}

fn check_fatal(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let (a, b) = (at(m, names, "a"), at(m, names, "b"));
            renutil_image::fatal(m, hooks, a, b, c.input("code").u32());
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_init_keep(c: &Case) -> Verdict {
    let cropped = c.input("G08790e04").i32() != 0;
    run(
        c,
        &[],
        |m, _, hooks, files| Ok(Some(("eax", rle_image::rle_init(m, hooks, files, c.input("keep").i16())))),
        |_, files| same_files(c, files, cropped),
    )
}

/// A whole picture: `si_rleInit`, then as the generator's chain one `SI_rleWrite` a line
/// (top line first) and `SI_rleClose`. The image is compared at the end; `pFile` and the
/// swap routines' `tmp`, which the chain changes after the case's memory was read, are
/// compared where the generator read them: after `si_rleInit`.
fn check_frame(c: &Case) -> Verdict {
    let cropped = c.input("G08790e04").i32() != 0;
    run(
        c,
        &["G0869c440", "G08817f38", "pfile_after", "picture"],
        |m, names, hooks, files| {
            let r = rle_image::rle_init(m, hooks, files, c.input("keep").i16());
            let want = c.output("G0869c440").u32();
            if m.u32(g::P_FILE) != want {
                return Err(format!("pFile after si_rleInit: original {want:#x}, port {:#x}", m.u32(g::P_FILE)));
            }
            if let Some(Some(t)) = c.try_output("G08817f38").map(|f| f.written()[0]) {
                if m.u8(byteswap::TMP) != t as u8 {
                    return Err(format!("tmp after si_rleInit: original {t:#x}, port {:#x}", m.u8(byteswap::TMP)));
                }
            }
            let plan = c.input("plan").i32s();
            let (width, height) = (plan[0], plan[1]);
            if m.u32(g::P_FILE) != 0 {
                let lines = at(m, names, "lines");
                for k in 0..height {
                    let base = lines + 4 * 320 * k as u32;
                    rle_image::rle_write(m, hooks, files, height - 1 - k, width, base, base + 320, base + 640, base + 960);
                }
            }
            rle_image::rle_close(m, hooks, files, plan[2] as i16, plan[3] as i16);
            Ok(Some(("eax", r)))
        },
        |m, files| {
            let want = c.output("pfile_after").u32();
            if m.u32(g::P_FILE) != want {
                return Err(format!("pFile at the end: original {want:#x}, port {:#x}", m.u32(g::P_FILE)));
            }
            let path = m.cstr(g::FRAME_NAME);
            let have = files.mem.files.get(&path);
            let field = c.output("picture");
            match (field.is_null(), have) {
                (true, None) => {
                    NO_PICTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                }
                (true, Some(_)) => Err("the picture: none in the original, one in the port".into()),
                (false, None) => Err("the picture: one in the original, none in the port".into()),
                (false, Some(got)) => {
                    let mut got = got.clone();
                    if mutate() {
                        if let Some(b) = got.last_mut() {
                            *b ^= 1;
                        }
                    }
                    same_picture("the picture", &field.written(), &got, cropped)?;
                    // what the port wrote is a picture the project's reader takes
                    read_back(&got, c)
                }
            }
        },
    )
}

/// The finished picture through `si-picture`'s reader: its size is the header's (the
/// controls' size; the height halved with fields), it has the four planes, and every line
/// the header's height covers comes back as it was handed to `SI_rleWrite`.
fn read_back(file: &[u8], c: &Case) -> Result<(), String> {
    let plan = c.input("plan").i32s();
    let (width, height) = (plan[0] as usize, plan[1] as usize);
    let fields = c.input("G08790d1c").i16() != 0;
    let want_height = if fields { height >> 1 } else { height };
    if want_height == 0 {
        // one line rendered in fields: the header says no line at all, which the reader
        // (like the original's) refuses as a size
        NO_LINE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Ok(());
    }
    let p = si_picture::Picture::read(file).map_err(|e| format!("the reader refuses the port's picture: {e:?}"))?;
    if (p.picture.width as usize, p.picture.height as usize) != (width, want_height) {
        return Err(format!("the reader gives {} x {}, the header was written for {width} x {want_height}", p.picture.width, p.picture.height));
    }
    let lines = c.input("lines").to_bytes();
    for (plane, got) in p.picture.planes.iter().enumerate() {
        let Some(got) = got else { return Err(format!("the reader finds no plane {plane}")) };
        for k in 0..want_height.min(height) {
            let base = 4 * 320 * k + 320 * plane;
            if got[k * width..(k + 1) * width] != lines[base..base + width] {
                return Err(format!("line {k}, plane {plane}: the reader gives other bytes than were written"));
            }
        }
    }
    READ_BACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Pictures of the port's that the project's reader read back whole; cases without a
/// picture (the frame was skipped: the picture existed and `keep` was set, or `access`
/// said so); pictures whose header says no line.
pub static READ_BACK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static NO_PICTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static NO_LINE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn report() {
    let get = |c: &std::sync::atomic::AtomicUsize| c.load(std::sync::atomic::Ordering::Relaxed);
    let (n, none, flat) = (get(&READ_BACK), get(&NO_PICTURE), get(&NO_LINE));
    if n + none + flat != 0 {
        println!("si_rleInit#frame: {n} pictures the port wrote were read back by si-picture's reader (size, four planes, every line as written); {none} cases have no picture (the frame is skipped); {flat} pictures have a header of no line (one line in field mode), which the reader refuses");
    }
}

// ------------------------------------------------- the older vectors, on the image form

pub(crate) fn verdict(r: Result<(), String>) -> Verdict {
    match r {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}

fn place(m: &mut Mem, c: &Case, name: &str) -> u32 {
    let field = c.input(name);
    if field.is_null() {
        0
    } else {
        let bytes = field.to_bytes();
        let a = m.place(bytes.len().max(1) as u32, 4);
        m.set_bytes(a, &bytes);
        a
    }
}

/// `writeChanalMixed` of `gen_out.py` through [`rle_image::write_chanal_mixed`].
pub fn old_mixed(c: &Case) -> Verdict {
    let mut m = Mem::new();
    let mut files = MemFiles::new();
    let file = files.open_named(b"f", b"", b"w");
    let planes = [place(&mut m, c, "a"), place(&mut m, c, "b"), place(&mut m, c, "c"), place(&mut m, c, "d")];
    rle_image::write_chanal_mixed(&mut m, &mut files, file, c.input("count").i32(), c.input("channels").i32(), planes);
    verdict(same_bytes("file (image form)", &c.output("f").u8s(), &files.files[&b"f".to_vec()]))
}

struct NoCalls;
impl Outside for NoCalls {
    fn call(&mut self, _: &mut Mem, addr: u32, _: &[u32]) -> u32 {
        panic!("the image form calls {addr:#010x}, the original called nothing")
    }
}

/// `SI_rleWrite` of `gen_out.py` through [`rle_image::rle_write`].
pub fn old_write(c: &Case) -> Verdict {
    let mut m = Mem::new();
    data::place_texts(&mut m);
    let mut files = MemFiles::new();
    let file = files.open_named(b"f", b"", b"w");
    m.set_u32(g::P_FILE, file);
    m.set_i16(g::WITH_ALPHA, c.input("g_alpha").i16());
    let p = [place(&mut m, c, "r"), place(&mut m, c, "g"), place(&mut m, c, "b"), place(&mut m, c, "a")];
    rle_image::rle_write(&mut m, &mut NoCalls, &mut files, c.input("y").i32(), c.input("width").i32(), p[0], p[1], p[2], p[3]);
    verdict(same_bytes("file (image form)", &c.output("f").u8s(), &files.files[&b"f".to_vec()]))
}

/// `si_rleInit` of `gen_out.py` through [`rle_image::rle_init`]: the globals stored where
/// the spec line of that generator has them.
pub fn old_init(c: &Case) -> Verdict {
    let mut m = Mem::new();
    data::place_texts(&mut m);
    m.set_i16(g::WIDTH, c.input("g_w").i16());
    m.set_i16(g::HEIGHT, c.input("g_h").i16());
    m.set_i32(g::CROP, c.input("g_crop").i32());
    m.set_i16(g::CROP_WIDTH, c.input("g_cw").i16());
    m.set_i16(g::CROP_HEIGHT, c.input("g_ch").i16());
    m.set_f32(g::CROP_LEFT, c.input("g_cx").f32());
    m.set_f32(g::CROP_BOTTOM, c.input("g_cy").f32());
    m.set_i16(g::FIELDS, c.input("g_field").i16());
    m.set_i16(g::FIELD_NOW, c.input("g_fno").i16());
    m.set_i16(g::FRAME_NOW, c.input("g_frame").i16());
    m.set_f32(g::PIXEL_RATIO, c.input("g_ratio").f32());
    let (name, path) = (place(&mut m, c, "name"), place(&mut m, c, "path"));
    m.set_u32(g::NAME, name);
    m.set_u32(g::SI_PICTURE_PATH, path);
    let shared = Shared::new(c);
    let mut hooks = Hooks::new(shared.clone());
    let mut files = Files { shared: shared.clone(), mem: MemFiles::new() };
    let got = rle_image::rle_init(&mut m, &mut hooks, &mut files, c.input("keep").i16());
    verdict((|| {
        if let Some(f) = m.fault() {
            return Err(format!("the image form faulted: {f:?}"));
        }
        shared.borrow().finish()?;
        if c.output("eax").u32() != got {
            return Err(format!("eax (image form): original {}, port {got}", c.output("eax").u32()));
        }
        let name: Vec<u8> = c.output("g_fname").written().iter().map_while(|w| w.map(|v| v as u8)).take_while(|b| *b != 0).collect();
        same_bytes("frameName (image form)", &name, &m.cstr(g::FRAME_NAME))?;
        if c.output("g_alpha").i16() != m.i16(g::WITH_ALPHA) {
            return Err("withAlpha (image form)".into());
        }
        same_files(c, &files, c.input("g_crop").i32() != 0)
    })())
}

/// `SI_rleClose` of `gen_out.py` through [`rle_image::rle_close`].
pub fn old_close(c: &Case) -> Verdict {
    let mut m = Mem::new();
    data::place_texts(&mut m);
    let mut files = MemFiles::new();
    for field in &c.inputs {
        if let Some(path) = field.name.strip_prefix("file:") {
            files.files.insert(path.as_bytes().to_vec(), field.to_bytes());
        }
    }
    let file = files.open_named(b"f", &c.input("f").to_bytes(), b"w");
    m.set_u32(g::P_FILE, file);
    let name: Vec<u8> = c.input("g_fname").u8s().into_iter().take_while(|b| *b != 0).collect();
    m.put_cstr(g::FRAME_NAME, &name);
    rle_image::rle_close(&mut m, &mut NoCalls, &mut files, c.input("complete").i16(), c.input("icon").i16());
    verdict((|| {
        if let Some(f) = m.fault() {
            return Err(format!("the image form faulted: {f:?}"));
        }
        if m.u32(g::P_FILE) != 0 {
            return Err("pFile is not null afterwards (image form)".into());
        }
        for field in &c.outputs {
            if let Some(path) = field.name.strip_prefix("file:") {
                same_bytes("the picture (image form)", &field.to_bytes(), files.files.get(path.as_bytes()).map(Vec::as_slice).unwrap_or(&[]))?;
            }
        }
        Ok(())
    })())
}

// -------------------------------------------------------------------- mem_chunk.c

/// The outputs of the chunk vectors that are not memory: where the buffers lay.
const CHUNK_NOTES: [&str; 2] = ["at", "at_heap"];

fn mem_word(w: &Word) -> si_mem::Word {
    match w {
        Word::Value { bytes, value } => si_mem::Word::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => si_mem::Word::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => si_mem::Word::Ref { name: name.clone(), offset: *offset },
    }
}

/// The eleven vector files of `gen_chunk.py` through the routines' form on the image: the
/// record, the test blocks and the heap blocks are the case's memory, **at the addresses
/// the emulator had them** (the vector's `at`): the routines compare and store addresses,
/// and a record word that points one past a block is printed as a number.
pub fn old_chunk(c: &Case) -> Verdict {
    use si_render::mem_chunk_image as k;
    const PLACED: [&str; 5] = ["buf", "s0", "s1", "s2", "s3"];
    const VALUES: [&str; 4] = ["size", "slot", "mark", "count"];
    let name = c.name.split('#').next().unwrap_or("").to_string();
    let mut drop: Vec<&str> = CHUNK_NOTES.to_vec();
    drop.extend(VALUES);
    let q = quiet(c, &drop);
    // the buffers the vector names an address for, in its order; then the others
    let ats = c.try_output("at").map(|f| f.u32s()).unwrap_or_default();
    let mut held: Vec<(String, Option<u32>, Option<Vec<si_mem::Word>>)> = Vec::new();
    let present: Vec<&str> = PLACED.iter().copied().filter(|n| q.try_input(n).is_some() || q.try_output(n).is_some()).collect();
    for field in q.inputs.iter().chain(q.outputs.iter().filter(|f| q.try_input(&f.name).is_none())) {
        if matches!(field.name.as_str(), "eax" | "ax" | "freed") || field.name.starts_with("heap") || held.iter().any(|h| h.0 == field.name) {
            continue;
        }
        let at = present.iter().position(|n| *n == field.name).and_then(|i| ats.get(i).copied());
        let words = if field.is_null() {
            None
        } else if q.try_input(&field.name).is_some() {
            Some(field.words().iter().map(mem_word).collect())
        } else {
            Some(field.words().iter().map(|w| si_mem::Word::Unwritten { bytes: w.bytes() }).collect())
        };
        held.push((field.name.clone(), at, words));
    }
    // (the placed ones first, so that the loader's own choices lie behind them)
    held.sort_by_key(|h| h.1.is_none());
    let mut m = Mem::new();
    let fields: Vec<si_mem::FieldAt> = held.iter().map(|(n, a, w)| (n.as_str(), *a, w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Verdict::Different(format!("the case does not load: {e}")),
    };
    run_loaded(
        c,
        &q,
        m,
        names,
        |m, names, _, _| {
            Ok(match name.as_str() {
                "padSize" => {
                    k::pad_size(m, at(m, names, "p"));
                    None
                }
                "alignPointer" => {
                    k::align_pointer(m, at(m, names, "p"));
                    None
                }
                "SI_chunkMalloc" => Some(("eax", k::chunk_malloc(m, at(m, names, "buf"), c.input("size").i32()))),
                "chunkAllocSlot" => Some(("ax", k::chunk_alloc_slot(m, at(m, names, "buf"), c.input("slot").i32(), c.input("size").i32()))),
                "SI_chunkSaveCurrent" => {
                    let out = if c.try_input("out").is_some_and(|f| f.is_null()) { 0 } else { at(m, names, "out") };
                    Some(("eax", k::chunk_save_current(m, at(m, names, "buf"), out)))
                }
                "SI_chunkReclaim" => Some(("eax", k::chunk_reclaim(m, at(m, names, "buf"), c.input("mark").i32()))),
                "SI_chunkInitialize" => {
                    k::chunk_initialize(m, at(m, names, "buf"));
                    None
                }
                "SI_chunkFreeAll" => {
                    k::chunk_free_all(m, at(m, names, "buf"));
                    None
                }
                "SI_chunkDestroy" => {
                    k::chunk_destroy(m, at(m, names, "pp"));
                    None
                }
                "SI_chunkAllocate" => Some(("eax", k::chunk_allocate(m, c.input("count").i32(), c.input("size").i32()))),
                other => return Err(format!("no image form for {other}")),
            })
        },
        |_, _| Ok(()),
    )
}

/// A series of requests on a buffer `SI_chunkAllocate` made (the generator's `post`): the
/// answers, the mark numbers, and at the end every heap block and the list of freed ones.
fn check_chunk_seq(c: &Case) -> Verdict {
    use si_render::mem_chunk_image as k;
    run(
        c,
        &["results", "marks"],
        |m, _, _, _| {
            let buf = k::chunk_allocate(m, c.input("count").i32(), c.input("size").i32());
            let plan = c.input("plan").i32s();
            let (want, want_marks) = (c.output("results").u32s(), c.output("marks").u32s());
            let (mut results, mut marks) = (Vec::new(), Vec::new());
            // the two cells the generator keeps in buffers of the case are locals here
            let (mark, cell) = (m.local(4), m.local(4));
            if buf != 0 {
                for op in plan[..plan.len() - 1].chunks(2) {
                    let r = match op[0] {
                        0 => k::chunk_malloc(m, buf, op[1]),
                        1 => {
                            m.set_i32(mark, 0x7777);
                            let r = k::chunk_save_current(m, buf, mark);
                            marks.push(m.u32(mark));
                            r
                        }
                        2 => k::chunk_reclaim(m, buf, op[1]),
                        3 => {
                            k::chunk_initialize(m, buf);
                            0
                        }
                        _ => {
                            k::chunk_free_all(m, buf);
                            0
                        }
                    };
                    results.push(r);
                }
                if plan[plan.len() - 1] != 0 {
                    m.set_u32(cell, buf);
                    k::chunk_destroy(m, cell);
                    results.push(m.u32(cell));
                }
            }
            if mutate() {
                if let Some(r) = results.last_mut() {
                    *r ^= 1;
                }
            }
            if results != want {
                let i = results.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(results.len().min(want.len()));
                return Err(format!("answers: {} in the original, {} in the port; first difference at step {i}", want.len(), results.len()));
            }
            if marks != want_marks {
                return Err(format!("mark numbers: original {want_marks:?}, port {marks:?}"));
            }
            Ok(Some(("eax", buf)))
        },
        |_, _| Ok(()),
    )
}

/// The C library's `drand48` state at the start of a case (the harness's).
const DRAND_START: u64 = 0x1234_abcd_330e;

/// The renderer's arena behind `SI_chunkMalloc`; `SI_warning` from the record.
struct Arena<'a>(Hooks<'a>);
impl Outside for Arena<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match addr {
            f::SI_CHUNK_MALLOC => si_render::mem_chunk_image::chunk_malloc(m, args[0], args[1] as i32),
            _ => self.0.call(m, addr, args),
        }
    }
}

/// `SI_chunkAllocate`, then `si_initJitter` twice with the arena for real: the two records
/// and every heap block (the arena's record and its blocks, the tables in them).
fn check_chunk_jitter(c: &Case) -> Verdict {
    use si_render::mem_chunk_image as k;
    run(
        c,
        &["j_after", "j2_after"],
        |m, names, hooks, _| {
            let buf = k::chunk_allocate(m, c.input("count").i32(), c.input("size").i32());
            if buf != m.u32(g::SI_REN_CHUNK) {
                return Err(format!("the buffer: {buf:#x}, the case planned {:#x}", m.u32(g::SI_REN_CHUNK)));
            }
            let plan = c.input("plan").i32s();
            let mut rng = si_render::jitter::Drand48(DRAND_START);
            let mut arena = Arena(Hooks::new(hooks.shared.clone()));
            for (count, name) in [(plan[0], "j"), (plan[1], "j2")] {
                // the record is a buffer of the case that the chain changed after the case's
                // memory was read: here a copy of it
                let record = m.local(12);
                m.copy(record, at(m, names, name), 12);
                si_render::jitter_image::init_jitter(m, &mut arena, &mut || rng.next(), count, record);
                let want = c.output(&format!("{name}_after")).written();
                let got = [m.u32(record) as u128, m.u32(record + 4) as u128, (m.u16(record + 8) ^ mutate() as u16) as u128, m.u16(record + 10) as u128];
                for (i, (w, g)) in want.iter().zip(got).enumerate() {
                    if w.is_some_and(|w| w != g) {
                        return Err(format!("{name} word {i}: original {:#x}, port {g:#x}", w.unwrap()));
                    }
                }
            }
            Ok(Some(("eax", buf)))
        },
        |_, _| Ok(()),
    )
}

/// The harness's heap behind `SI_chunkMalloc` (the generator of `gen_outer.py` answered it
/// with a block of `malloc` and recorded the call); `SI_warning` from the record.
struct Blocks<'a>(Rc<RefCell<Shared<'a>>>);
impl Outside for Blocks<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match addr {
            f::SI_CHUNK_MALLOC => {
                self.0.borrow_mut().replay("SI_chunkMalloc", &[Arg::W(args[0]), Arg::W(args[1])]);
                m.malloc(args[1])
            }
            _ => Hooks::new(self.0.clone()).call(m, addr, args),
        }
    }
}

/// `si_initJitter` of `gen_outer.py` through [`si_render::jitter_image::init_jitter`].
pub fn old_jitter(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let mut rng = si_render::jitter::Drand48(DRAND_START);
            let j = at(m, names, "j");
            let mut blocks = Blocks(hooks.shared.clone());
            si_render::jitter_image::init_jitter(m, &mut blocks, &mut || rng.next(), c.input("count").i32(), j);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

// ------------------------------------------------------- t2d_dial.c and edtools.c

/// The C library's `stderr` as the harness has it: the third handle.
const STDERR_HANDLE: u32 = 0x6f00_0200;

fn check_wrap(c: &Case) -> Verdict {
    use si_render::t2d_dial as t;
    let name = c.name.split('#').next().unwrap_or("").to_string();
    run(
        c,
        &[],
        |m, names, hooks, _| {
            m.set_u32(t::STDERR, STDERR_HANDLE);
            let buffer = |m: &Mem, n: &str| if c.try_input(n).is_some_and(|f| !f.is_null()) { at(m, names, n) } else { 0 };
            let tex = buffer(m, "tex");
            match name.as_str() {
                "updDoWrap" => t::upd_do_wrap(m, hooks, buffer(m, "node"), tex),
                "SI_txt2dSetScale" => t::set_scale(m, tex),
                _ => t::set_wrap(m, hooks, buffer(m, "node"), tex),
            }
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_add_picture(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            m.put_cstr(si_render::edtools::SUFFIX_PIC, b"pic");
            let (dict, name) = (at(m, names, "dict"), at(m, names, "name"));
            Ok(Some(("eax", si_render::edtools::elemdict_add_picture(m, hooks, dict, name, c.input("record").u32(), c.input("byfile").i16()))))
        },
        |_, _| Ok(()),
    )
}

fn check_remove_picture(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let (dict, name) = (at(m, names, "dict"), at(m, names, "name"));
            let record = match &c.input("record").words()[0] {
                Word::Ref { name, offset } => at(m, names, name) + offset,
                w => w.value().unwrap_or(0) as u32,
            };
            Ok(Some(("eax", si_render::edtools::elemdict_picture_elem_remove(m, hooks, dict, record, name))))
        },
        |_, _| Ok(()),
    )
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_swap2byte" => |c: &Case| swap_check(c, byteswap::swap2byte),
        "SI_swap4byte" => |c: &Case| swap_check(c, byteswap::swap4byte),
        "SI_swap8byte" => |c: &Case| swap_check(c, byteswap::swap8byte),
        "SI_fix_SI_FileChunk" => |c: &Case| swap_check(c, byteswap::fix_file_chunk),
        "SI_fix_SI_PictChunk" => |c: &Case| swap_check(c, byteswap::fix_pict_chunk),
        "SI_byteswap_fwrite" => check_byteswap_fwrite,
        "si_fatal" => check_fatal,
        "si_rleInit#keep" => check_init_keep,
        "si_rleInit#frame" => check_frame,
        "SI_chunkAllocate#seq" => check_chunk_seq,
        "updDoWrap" | "SI_txt2dSetScale" | "SI_txt2dSetWrap" | "SI_txt2dSetWrap#null" => check_wrap,
        "SI_elemdictAddPicture" => check_add_picture,
        "SI_elemdictPictureElemRemove" => check_remove_picture,
        "SI_chunkAllocate#jitter" => check_chunk_jitter,
        _ => return None,
    })
}
