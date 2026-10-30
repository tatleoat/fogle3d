// SPDX-License-Identifier: MIT
//! Compares the ports of `si-app` with the original, routine by routine, on the vectors
//! `re/tools/si-app/emu_app.py` makes with the emulation harness (finding 0011).
//!
//!   app-emucheck <directory of .vec files>
//!
//! A case holds a routine's inputs, a script of answers for everything the routine calls
//! outside itself, and what the original left behind: its result, the memory it wrote, what
//! it printed, and `trace`, the calls it made in order. The port runs on the same inputs
//! with a [`Recorder`] armed with the same script; every one of those must agree. A case in
//! which the original ended the program (`STOP:exit:N`) is compared too: the port must end
//! with the same status after the same calls. Cases that stopped for another reason are
//! not compared.
//!
//! `EMUCHECK_MUTATE=1` flips a bit of what the port produced before each comparison: every
//! compared case must then differ.

use si_app::emergency::strip_trail_slash;
use si_app::env::{Exit, Recorder};
use si_app::error::FmtArg;
use si_app::memory;
use si_app::render_cmd::RenderControl;
use si_app::state::{App, Event, EventRef, NetEntry, ProcData};
use si_app::sysmain;
use si_app::ScriptedClients;
use si_x87::vectors::{Case, Tally};
use std::path::Path;

type A = App<Recorder>;

fn mutate() -> bool {
    std::env::var("EMUCHECK_MUTATE").is_ok()
}

/// The program in its start state, with the case's script and files.
fn app(c: &Case) -> A {
    let mut r = Recorder::from_script(&c.input("script").to_bytes());
    for f in &c.inputs {
        if let Some(path) = f.name.strip_prefix("file:") {
            r.files.insert(path.as_bytes().to_vec(), f.to_bytes());
        }
    }
    App::new(r)
}

fn bytes(c: &Case, name: &str) -> Vec<u8> {
    c.input(name).to_bytes()
}

/// A C string argument: the field's bytes, or `None` for a null pointer.
fn cstr(c: &Case, name: &str) -> Option<Vec<u8>> {
    let f = c.input(name);
    if f.is_null() {
        None
    } else {
        Some(f.to_bytes())
    }
}

/// The bytes of a buffer up to its first zero.
fn upto0(mut b: Vec<u8>) -> Vec<u8> {
    if let Some(z) = b.iter().position(|&c| c == 0) {
        b.truncate(z);
    }
    b
}

fn out_bytes(c: &Case, name: &str) -> Vec<u8> {
    c.try_output(name).map(|f| f.to_bytes()).unwrap_or_default()
}

fn i(c: &Case, name: &str) -> i32 {
    c.input(name).values()[0] as u32 as i32
}

fn i16_in(c: &Case, name: &str) -> i16 {
    c.input(name).values()[0] as u16 as i16
}

fn after32(c: &Case, name: &str) -> i32 {
    c.after(name).values()[0] as u32 as i32
}

fn eax(c: &Case) -> i32 {
    c.output("eax").values()[0] as u32 as i32
}

/// The status the original's `exit` was called with, if the case ended that way.
fn exit_status(c: &Case) -> Option<i32> {
    c.notes.iter().find_map(|n| n.strip_prefix("STOP:exit:")).and_then(|s| s.parse().ok())
}

/// The port's record without the lines that stand for memory accesses.
fn port_trace(a: &A) -> Vec<u8> {
    let mut out = Vec::new();
    for line in a.w.trace.split(|&c| c == b'\n') {
        if line.is_empty() || line.starts_with(b"mem_") {
            continue;
        }
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    if mutate() && !out.is_empty() {
        out[0] ^= 1;
    }
    out
}

/// Collects what differs.
#[derive(Default)]
struct Diff(Vec<String>);

impl Diff {
    fn eq<T: PartialEq + std::fmt::Debug>(&mut self, what: &str, port: T, original: T) {
        if port != original || mutate() {
            self.0.push(format!("{what}: port {port:?}, original {original:?}"));
        }
    }
    fn text(&mut self, what: &str, port: &[u8], original: &[u8]) {
        if port != original || mutate() {
            let at = port.iter().zip(original).position(|(a, b)| a != b).unwrap_or(port.len().min(original.len()));
            let cut = |b: &[u8]| String::from_utf8_lossy(&b[at.saturating_sub(60).min(b.len())..(at + 60).min(b.len())]).into_owned();
            self.0.push(format!("{what} differs at byte {at}: port …{:?}…, original …{:?}…", cut(port), cut(original)));
        }
    }
    /// The trace and both output streams.
    fn outside(&mut self, c: &Case, a: &A) {
        self.text("trace", &port_trace(a), &out_bytes(c, "trace"));
        self.text("stdout", &a.w.stdout, &out_bytes(c, "stdout"));
        self.text("stderr", &a.w.stderr, &out_bytes(c, "stderr"));
    }
    /// How the routine ended: by returning, or with `exit(status)`.
    fn ending(&mut self, c: &Case, port: Option<Exit>) {
        self.eq("exit", port.map(|e| e.0), exit_status(c));
    }
    /// A memory image against the bytes the original wrote (unwritten bytes are skipped).
    fn image(&mut self, what: &str, port: &[u8], c: &Case, name: &str) {
        let w = c.output(name).written();
        self.eq(&format!("{what} length"), port.len(), w.len());
        for (k, (p, o)) in port.iter().zip(&w).enumerate() {
            if let Some(o) = o {
                let p = if mutate() && k == 0 { *p ^ 1 } else { *p };
                if p as u128 != *o {
                    self.0.push(format!("{what}+0x{k:x}: port {p:#x}, original {o:#x}"));
                    break;
                }
            }
        }
    }
    fn files(&mut self, c: &Case, a: &A) {
        for f in &c.outputs {
            if let Some(path) = f.name.strip_prefix("file:") {
                let port = a.w.files.get(path.as_bytes());
                if f.is_null() {
                    self.eq(&format!("file {path} removed"), port.is_none(), true);
                } else {
                    self.text(&format!("file {path}"), port.map(|v| &v[..]).unwrap_or(b"<missing>"), &f.to_bytes());
                }
            }
        }
    }
}

fn pointer_is_null(c: &Case) -> bool {
    matches!(c.output("ptr").words()[0], si_x87::vectors::Word::Value { value: 0, .. })
}

/// Runs the port of one case's routine; `None` when there is no check for the routine.
fn check(c: &Case) -> Option<Diff> {
    let mut d = Diff::default();
    let name = c.name.split('#').next().unwrap();
    match name {
        // ------------------------------------------------------------ constants
        "SI_start1" => d.eq("eax", sysmain::si_start1(), eax(c)),
        "SI_end1" => d.eq("eax", sysmain::si_end1(), eax(c)),
        "SI_start2" => d.eq("eax", sysmain::si_start2(), eax(c)),
        "SI_end2" => d.eq("eax", sysmain::si_end2(), eax(c)),
        "SI_start4" => d.eq("eax", memory::si_start4(), eax(c)),
        "SI_end4" => d.eq("eax", memory::si_end4(), eax(c)),
        "SI_swapSpaceAvailable" => d.eq("eax", memory::swap_space_available(i(c, "n")), eax(c)),
        "SI_threadStorageFree" => d.eq("eax", app(c).thread_storage_free(), eax(c)),
        "SI_threadStorageExit" => d.eq("eax", app(c).thread_storage_exit(), eax(c)),
        "SI_isDialogEditor" => d.eq("ax", app(c).is_dialog_editor(), eax(c) as i16),
        "SI_emergencyCleanup" => {
            let mut a = app(c);
            a.emergency_cleanup(i16_in(c, "flag"));
            d.outside(c, &a);
        }
        // ------------------------------------------------------------ sys_main.c
        "get_init_value" => {
            let mut a = app(c);
            a.g.init_buf.copy_from_slice(&bytes(c, "init")[..7]);
            d.eq("eax", a.get_init_value(i(c, "n")), eax(c));
        }
        "set_init_buf" => {
            let mut a = app(c);
            let before = bytes(c, "init");
            a.g.init_buf.copy_from_slice(&before[..7]);
            a.set_init_buf(&bytes(c, "from"));
            let mut image = a.g.init_buf.to_vec();
            image.push(before[7]);
            d.image("init_buf", &image, c, "init");
        }
        "getDefaultPath" => {
            let mut a = app(c);
            if c.input("out").is_null() {
                d.outside(c, &a);
            } else {
                let got = a.get_default_path(&bytes(c, "prog")).unwrap();
                let heap = c.output("heap0").written();
                let original: Vec<u8> = heap.iter().map_while(|w| w.filter(|&v| v != 0).map(|v| v as u8)).collect();
                d.text("path", &got, &original);
                d.outside(c, &a);
            }
        }
        "SI_standDisplayVersion" => {
            let mut a = app(c);
            a.stand_display_version(&bytes(c, "name"), cstr(c, "extra").as_deref(), cstr(c, "version").as_deref(), i(c, "from"), i(c, "to"));
            d.outside(c, &a);
        }
        "printUsage" => {
            let mut a = app(c);
            let e = a.print_usage(&bytes(c, "name"));
            d.outside(c, &a);
            d.ending(c, Some(e));
        }
        "usage" => {
            let mut a = app(c);
            let e = a.usage(i(c, "code"), &bytes(c, "name"));
            d.outside(c, &a);
            d.ending(c, Some(e));
        }
        "zbufferWarning" => {
            let mut a = app(c);
            a.zbuffer_warning();
            d.outside(c, &a);
        }
        // ------------------------------------------------------------ sys_memory.c
        "SI_malloc" => {
            let size = i(c, "size") as usize;
            match memory::si_malloc(size) {
                None => d.eq("null", pointer_is_null(c), true),
                Some(block) => {
                    d.eq("null", pointer_is_null(c), false);
                    d.image("block", &block, c, "heap0");
                }
            }
        }
        "SI_calloc" => {
            let (count, size) = (i(c, "count") as usize, i(c, "size") as usize);
            match memory::si_calloc(count, size) {
                None => d.eq("null", pointer_is_null(c), true),
                Some(block) => {
                    d.eq("null", pointer_is_null(c), false);
                    d.image("block", &block, c, "heap0");
                }
            }
        }
        "SI_realloc" => {
            let size = i(c, "size") as usize;
            let before = bytes(c, "block");
            let got = memory::si_realloc(Some(before.clone()), size).unwrap();
            let p = c.output("ptr").words()[0].clone();
            if size == 0 {
                d.eq("same block", matches!(&p, si_x87::vectors::Word::Ref { name, offset: 0 } if name == "block"), true);
                d.eq("content", got, before);
            } else {
                d.eq("new block", matches!(&p, si_x87::vectors::Word::Ref { name, .. } if name == "heap0"), true);
                let w = c.output("heap0").written();
                d.eq("length", got.len(), w.len());
                for k in 0..size.min(8) {
                    d.eq("copied byte", Some(got[k] as u128), w[k]);
                }
            }
        }
        "SI_threadStorageInit" => {
            let mut a = app(c);
            let pd = c.input("pd").u32s();
            a.g.proc_data = ProcData { dictionary: pd[0] as i32, graf_list: pd[1] as i32, camera: pd[2] };
            let from = (!c.input("from").is_null()).then(|| {
                let f = c.input("from").u32s();
                ProcData { dictionary: f[0] as i32, graf_list: f[1] as i32, camera: f[2] }
            });
            d.eq("eax", a.thread_storage_init(from.as_ref()), eax(c));
            let after = c.after("pd").u32s();
            d.eq("proc data", [a.g.proc_data.dictionary as u32, a.g.proc_data.graf_list as u32, a.g.proc_data.camera], [after[0], after[1], after[2]]);
        }
        "SI_threadStorageCopy" => {
            let mut a = app(c);
            let pd = c.input("pd").u32s();
            a.g.proc_data = ProcData { dictionary: pd[0] as i32, graf_list: pd[1] as i32, camera: pd[2] };
            if c.input("to").is_null() {
                d.eq("eax", a.thread_storage_copy(None), eax(c));
            } else {
                let mut to = ProcData::default();
                d.eq("eax", a.thread_storage_copy(Some(&mut to)), eax(c));
                let after = c.after("to").u32s();
                d.eq("copy", [to.dictionary as u32, to.graf_list as u32, to.camera], [after[0], after[1], after[2]]);
            }
        }
        // ------------------------------------------------------------ sys_signal.c
        "SI_signalInit" => {
            let mut a = app(c);
            a.g.signal_coredump = i16_in(c, "kept");
            a.signal_init(i16_in(c, "coredump"));
            d.eq("kept", a.g.signal_coredump, c.after("kept").values()[0] as u16 as i16);
            d.outside(c, &a);
        }
        // ------------------------------------------------------------ globals.c
        "SI_manipulationInit" => {
            let mut a = app(c);
            a.manipulation_init();
            d.image("SI_glbManip", &a.g.manip.0, c, "rec");
            d.outside(c, &a);
        }
        "SI_animationInfoInit" => {
            let mut a = app(c);
            a.animation_info_init();
            d.image("SI_glbAnim", &a.g.anim.0, c, "rec");
            d.outside(c, &a);
        }
        "SI_incrementInfoInit" => {
            let mut a = app(c);
            a.increment_info_init();
            d.image("SI_glbIncrem", &a.g.increm.0, c, "rec");
        }
        "SI_duplicationInfoInit" => {
            let mut a = app(c);
            a.duplication_info_init(i16_in(c, "all"));
            d.image("SI_glbDuplic", &a.g.duplic.0, c, "rec");
        }
        "SI_schematicInfoInit" => {
            let mut a = app(c);
            a.schematic_info_init();
            d.image("SI_glbSchem", &a.g.schem.0, c, "rec");
        }
        // ------------------------------------------------------------ emergencySave.c
        "stripTrailSlash" => {
            let before = bytes(c, "text");
            let mut s = upto0(before.clone());
            strip_trail_slash(&mut s);
            d.text("text", &s, &upto0(c.after("text").to_bytes()));
        }
        "SI_replicateCommandLine" => {
            let mut a = app(c);
            let n = i(c, "argc") as usize;
            let args: Vec<Vec<u8>> = (0..n).map(|k| upto0(bytes(c, &format!("a{k}")))).collect();
            a.replicate_command_line(&args);
            d.text("line", a.return_command_line(), &upto0(c.after("line").to_bytes()));
        }
        // ------------------------------------------------------------ SI_error.c
        "SI_error" => {
            let mut a = app(c);
            a.g.error.filter = i(c, "filter");
            a.g.error.output = i(c, "mode");
            let mut args = Vec::new();
            for k in ["a", "b", "c", "d"] {
                if let Some(f) = c.try_input(k) {
                    // a string argument is a byte field, an integer one a single word
                    if c.name.ends_with("#sss") || (c.name.ends_with("#sddd") && k == "a") {
                        args.push(FmtArg::S(f.to_bytes()));
                    } else {
                        args.push(FmtArg::D(f.values()[0] as u32 as i32));
                    }
                }
            }
            // the formatted text goes through printf again: a `%` conversion in an argument
            // reads stray stack words in the original
            let has_conversion = args.iter().any(|x| matches!(x, FmtArg::S(s) if s.windows(2).any(|w| w[0] == b'%' && w[1] != b'%') || s.last() == Some(&b'%')));
            if has_conversion && (a.g.error.output == 0 || a.g.error.output == 1) {
                return None;
            }
            let (code, handler) = a.si_error(i(c, "code"), &args);
            d.eq("eax", code, eax(c));
            if let Some(h) = handler {
                a.w.note("handler", &[h.code.into(), h.kind.into(), h.text[..].into()]);
            }
            d.outside(c, &a);
        }
        "SI_errorGetMsg" => d.text("msg", app(c).error_get_msg(i(c, "code")), &out_bytes(c, "msg")),
        "SI_errorGetType" => d.eq("eax", app(c).error_get_type(i(c, "code")), eax(c)),
        "SI_errorSetHandler" | "SI_errorGetHandler" | "SI_errorSetOutputMode" | "SI_errorGetOutputMode" | "SI_errorSetFilter" | "SI_errorGetFilter" => {
            let mut a = app(c);
            a.g.error.filter = i(c, "filter");
            a.g.error.output = i(c, "mode");
            a.g.error.handler = i(c, "handler") as u32;
            let v = c.try_input("v").map(|f| f.values()[0] as u32).unwrap_or(0);
            let got = match name {
                "SI_errorSetHandler" => a.error_set_handler(v) as i32,
                "SI_errorGetHandler" => a.error_get_handler() as i32,
                "SI_errorSetOutputMode" => a.error_set_output_mode(v as i32),
                "SI_errorGetOutputMode" => a.error_get_output_mode(),
                "SI_errorSetFilter" => a.error_set_filter(v as i32),
                _ => a.error_get_filter(),
            };
            d.eq("eax", got, eax(c));
            d.eq("filter", a.g.error.filter, after32(c, "filter"));
            d.eq("mode", a.g.error.output, after32(c, "mode"));
            d.eq("handler", a.g.error.handler as i32, after32(c, "handler"));
        }
        _ => return more::check(c, name),
    }
    Some(d)
}

mod more;

fn main() {
    // the original's messages come from the user's install (unit rta)
    si_rt::check_or_exit("app-emucheck");
    let dir = std::env::args().nth(1).expect("usage: app-emucheck <directory of .vec files>");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(Path::new(&dir))
        .expect("reading the directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "vec"))
        .collect();
    files.sort();
    std::panic::set_hook(Box::new(|_| {}));
    println!("{}", Tally::header());
    let mut total = Tally::default();
    let mut ok = true;
    for path in &files {
        let file = &path.file_stem().unwrap().to_string_lossy().into_owned();
        let mut t = Tally::default();
        // one case at a time: a vector file can be large
        use std::io::BufRead;
        for line in std::io::BufReader::new(std::fs::File::open(path).expect("opening a vector file")).lines() {
            let line = line.expect("reading a vector file");
            let Some(c) = Case::parse(&line).unwrap_or_else(|e| panic!("{}: {e}", path.display())) else { continue };
            let c = &c;
            let comparable = exit_status(c).is_some() || !c.stopped();
            let verdict = if comparable { std::panic::catch_unwind(|| check(c)).unwrap_or_else(|_| Some(Diff(vec!["the port panicked".into()]))) } else { None };
            match verdict {
                None => t.add(c, None, String::new),
                Some(d) => {
                    // a case that ended in exit is compared in full, so it counts as
                    // identical although the harness marks it as stopped
                    let same = d.0.is_empty();
                    if same && exit_status(c).is_some() && !c.has_note("NA") && !c.has_note("UNINIT") {
                        t.cases += 1;
                        t.identical += 1;
                    } else if !same && c.has_note("UNINIT") {
                        // the original's output depends on memory it never initialised
                        // (the two passes of the harness disagree): nothing to compare
                        t.add(c, None, String::new);
                    } else {
                        t.add(c, Some(same), || format!("{}: {}", c.name, d.0.join("; ")));
                    }
                }
            }
        }
        println!("{}", t.row(file));
        for e in &t.examples {
            println!("    {e}");
        }
        ok &= t.different == 0;
        total.cases += t.cases;
        total.identical += t.identical;
        total.identical_given_calls += t.identical_given_calls;
        total.different += t.different;
        total.skipped += t.skipped;
    }
    println!("{}", total.row("total"));
    let _ = (Event::default(), EventRef::Active, NetEntry::default(), RenderControl::default(), ScriptedClients::default());
    std::process::exit(if ok { 0 } else { 1 });
}
