// SPDX-License-Identifier: MIT
//! `app-imagecheck <dir>`: the image form of the batch path (`si_app::image`, finding 0019,
//! section 12) against the vectors `re/tools/si-app/gen_image.py` writes: one routine of the
//! original per case, stepped in the emulation harness on made-up worlds.
//!
//! Per case: a fresh image with the module's `.data` values, the case's buffers where the
//! harness put them (`si_mem::load_case_at`), the port run with an outside world that
//! replays the calls the original made (each call's name and arguments held against the
//! recorded one, the recorded answer handed back, and the stand-in's side effect done the
//! same way: `getcwd`'s block, `time`'s store, `realpath`'s text, `readSetup`'s move of
//! `argv`). Compared: the result or the exit status, every buffer and heap block the
//! original's case reports, the blocks freed, the printed text, the calls. Nothing here
//! reproduces a function of the original.
//!
//! `EMUCHECK_MUTATE=1` flips a bit of everything the port produced before the comparison;
//! `EMUCHECK_MUTATE=calls` a bit of the first argument of every call. Every compared case
//! must then differ (every case with a call, for the second).

use si_app::image::{batch, globals_c, g, hardware, leftovers, setup, system};
use si_app::Exit;
use si_mem::vecfmt::{load_case_at, parse_words, FieldAt, Names, Word};
use si_mem::{CStr32, Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;

/// One recorded call: `@name(args)=answer`.
#[derive(Clone, Debug)]
struct RecCall {
    name: String,
    args: Vec<Word>,
    ret: Word,
}

#[derive(Clone, Debug, Default)]
struct Case {
    name: String,
    inputs: Vec<(String, Option<Vec<Word>>)>,
    outputs: Vec<(String, Option<Vec<Word>>)>,
    calls: Vec<RecCall>,
    notes: Vec<String>,
}

impl Case {
    fn input(&self, name: &str) -> Option<&[Word]> {
        self.inputs.iter().find(|(n, _)| n == name).and_then(|(_, w)| w.as_deref())
    }
    fn output(&self, name: &str) -> Option<&Option<Vec<Word>>> {
        self.outputs.iter().find(|(n, _)| n == name).map(|(_, w)| w)
    }
    fn note(&self, prefix: &str) -> Option<&str> {
        self.notes.iter().find(|n| n.starts_with(prefix)).map(|s| s.as_str())
    }
}

/// Splits at the blanks that are not inside a quoted text.
fn tokens(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), None::<usize>, false);
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'"' && (i == 0 || b[i - 1] != b'\\') {
            quoted = !quoted;
        }
        if c == b' ' && !quoted {
            if let Some(s) = start.take() {
                out.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

fn parse(line: &str) -> Result<Option<Case>, String> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut c = Case::default();
    let mut part = 0;
    for (k, t) in tokens(line).into_iter().enumerate() {
        if k == 0 {
            c.name = t.to_string();
            continue;
        }
        match (part, t) {
            (_, "=>") => part = 1,
            (_, "!") => part = 2,
            (2, note) => c.notes.push(note.to_string()),
            (_, call) if call.starts_with('@') => {
                let open = call.find('(').ok_or("call without `(`")?;
                let close = call.rfind(")=").ok_or("call without `)=`")?;
                let args = &call[open + 1..close];
                c.calls.push(RecCall {
                    name: call[1..open].to_string(),
                    args: if args.is_empty() { Vec::new() } else { parse_words(args)?.unwrap_or_default() },
                    ret: parse_words(&call[close + 2..])?.and_then(|w| w.into_iter().next()).unwrap_or(Word::Value { bytes: 4, value: 0 }),
                });
            }
            (p, field) => {
                let (name, value) = field.split_once('=').ok_or_else(|| format!("no `=` in `{field}`"))?;
                let words = parse_words(value)?;
                if p == 0 {
                    c.inputs.push((name.to_string(), words));
                } else {
                    c.outputs.push((name.to_string(), words));
                }
            }
        }
    }
    Ok(Some(c))
}

/// The address of a global buffer `gXXXXXXXX[_label]`.
fn global_at(name: &str) -> Option<u32> {
    let hex = name.strip_prefix('g')?.get(..8)?;
    if name.len() > 9 && name.as_bytes()[9] != b'_' {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn is_buffer(name: &str) -> bool {
    !(name.starts_with("p_") || name.starts_with("v_") || name.starts_with("file:"))
}

fn place(m: &mut Mem, c: &Case) -> Result<Names, String> {
    let fields: Vec<FieldAt<'_>> = c.inputs.iter().filter(|(n, _)| is_buffer(n)).map(|(n, w)| (n.as_str(), global_at(n), w.as_deref())).collect();
    load_case_at(m, &fields)
}

fn word_value(m: &Mem, names: &Names, w: &Word) -> Option<u32> {
    match w {
        Word::Value { value, .. } => Some(*value as u32),
        Word::Unwritten { .. } => None,
        Word::Ref { name, offset } => names.addr(m, name).map(|a| a.wrapping_add(*offset)),
    }
}

fn arg(m: &Mem, names: &Names, c: &Case, name: &str) -> u32 {
    for prefix in ["p_", "v_"] {
        if let Some(w) = c.input(&format!("{prefix}{name}")).and_then(|w| w.first()) {
            return word_value(m, names, w).unwrap_or(0);
        }
    }
    0
}

const LOCAL: u32 = 0x57ac_57ac;

/// The outside world that replays the original's calls.
struct Replay<'c> {
    calls: &'c [RecCall],
    k: usize,
    names: Names,
    cwd: Vec<u8>,
    real: Vec<u8>,
    /// The release `uname` leaves in its buffer.
    uname: Vec<u8>,
    /// The standard input (`fgets` takes lines from it) and where it has been read to.
    stdin: Vec<u8>,
    stdin_at: usize,
    diff: Option<String>,
    printed: [Vec<u8>; 2],
    mutate_calls: bool,
}

impl Replay<'_> {
    fn differ(&mut self, text: String) {
        if self.diff.is_none() {
            self.diff = Some(text);
        }
    }
}

impl Outside for Replay<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let mut args = args.to_vec();
        if matches!(r.name, "SI_setCamOrthoView" | "SI_setCamView") {
            // the stand-in also logged what the two pointers point to: three floats each
            for p in [args[1], args[2]] {
                for i in 0..3 {
                    let v = m.u32(p + 4 * i);
                    args.push(v);
                }
            }
        }
        if self.mutate_calls && !args.is_empty() {
            args[0] ^= 1;
        }
        let k = self.k;
        self.k += 1;
        let Some(rec) = self.calls.get(k) else {
            self.differ(format!("call {k}: the port calls {} after the original's last call", r.name));
            return Ret::NONE;
        };
        if rec.name != r.name {
            self.differ(format!("call {k}: original {}, port {}", rec.name, r.name));
        } else if rec.args.len() != args.len() {
            self.differ(format!("call {k} {}: {} arguments, port {}", r.name, rec.args.len(), args.len()));
        } else {
            for (i, (w, got)) in rec.args.iter().zip(&args).enumerate() {
                if let Some(want) = word_value(m, &self.names, w) {
                    let mask = if w.bytes() >= 4 { u32::MAX } else { (1u32 << (8 * w.bytes())) - 1 };
                    if want != LOCAL && want & mask != got & mask {
                        self.differ(format!("call {k} {} argument {i}: original {w}, port {got:#x}", r.name));
                    }
                }
            }
        }
        let ret = word_value(m, &self.names, &rec.ret);
        // what the stand-in stored through the caller's local (`NAME.out(...)` follows the call)
        let out_name = format!("{}.out", r.name);
        if self.calls.get(self.k).is_some_and(|c| c.name == out_name) {
            let outs = self.calls[self.k].args.clone();
            self.k += 1;
            if let Some(at) = rec.args.iter().position(|w| matches!(w, Word::Value { value, .. } if *value as u32 == LOCAL)) {
                let mut p = args[at];
                for w in &outs {
                    match w {
                        Word::Value { bytes: 2, value } => {
                            m.set_u16(p, *value as u16);
                            p += 2;
                        }
                        w => {
                            m.set_u32(p, word_value(m, &self.names, w).unwrap_or(0));
                            p += 4;
                        }
                    }
                }
            }
        }
        match r.name {
            "getcwd" => {
                let got = batch::libc_getcwd(m, args[0], args[1], &self.cwd);
                let want = word_value(m, &self.names, &rec.ret).unwrap_or(0);
                if got != want {
                    self.differ(format!("call {k} getcwd: original answered {want:#x}, the port's block is {got:#x}"));
                }
                Ret::eax(got)
            }
            "time" => {
                let v = ret.unwrap_or(0);
                if args[0] != 0 {
                    m.set_u32(args[0], v);
                }
                Ret::eax(v)
            }
            "realpath" => {
                let v = ret.unwrap_or(0);
                if v != 0 {
                    m.put_cstr(args[1], &self.real);
                }
                Ret::eax(v)
            }
            "uname" => {
                // the stand-in stored the release (third name of 65 bytes) in the caller's buffer
                if ret.unwrap_or(0) as i32 >= 0 {
                    m.put_cstr(args[0] + 130, &self.uname);
                }
                Ret::eax(ret.unwrap_or(0))
            }
            "fgets" => {
                // the next line of the standard input, at most n - 1 bytes, the line feed kept
                let (buf, n) = (args[0], args[1] as usize);
                let mut line = Vec::new();
                while line.len() + 1 < n && self.stdin_at + line.len() < self.stdin.len() {
                    let c = self.stdin[self.stdin_at + line.len()];
                    line.push(c);
                    if c == b'\n' {
                        break;
                    }
                }
                if line.is_empty() {
                    return Ret::eax(0);
                }
                self.stdin_at += line.len();
                m.put_cstr(buf, &line);
                Ret::eax(buf)
            }
            "SI_audioMngAtDelete" => {
                // the stand-in unlinks the head: the container now holds the node's next
                m.set_u32(system::AUDIO_CONTAINER, args[1]);
                Ret::eax(ret.unwrap_or(0))
            }
            "readSetup" => {
                let used = ret.unwrap_or(0);
                m[g::ARGV] = m[g::ARGV].wrapping_add(4 * used);
                m[g::ARGC] -= used as i32;
                Ret::eax(used)
            }
            _ => Ret::eax(ret.unwrap_or(0)),
        }
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.printed[(stream == 2) as usize].extend_from_slice(text);
    }
}

enum Outcome {
    Returned(Option<u32>),
    Exited(i32),
}

fn run(m: &mut Mem, o: &mut Replay, c: &Case, names: &Names) -> Result<Outcome, String> {
    let a = |m: &Mem, n: &str| arg(m, names, c, n);
    let s = |m: &Mem, n: &str| CStr32::new(arg(m, names, c, n));
    let done = Outcome::Returned(None);
    let exit = |r: Result<(), Exit>| match r {
        Ok(()) => Outcome::Returned(None),
        Err(e) => Outcome::Exited(e.0),
    };
    Ok(match c.name.as_str() {
        "SI_manipulationInit" => {
            globals_c::manipulation_init(m, o);
            done
        }
        "SI_animationInfoInit" => {
            globals_c::animation_info_init(m, o);
            done
        }
        "SI_incrementInfoInit" => {
            globals_c::increment_info_init(m);
            done
        }
        "SI_schematicInfoInit" => {
            globals_c::schematic_info_init(m);
            done
        }
        "SI_duplicationInfoInit" => {
            let all = a(m, "all") as i16;
            globals_c::duplication_info_init(m, all);
            done
        }
        "SI_checkHardware" => Outcome::Returned(Some(hardware::check_hardware(m, o) as u16 as u32)),
        "SI_getRawFcurvePointer" => Outcome::Returned(Some(leftovers::get_raw_fcurve_pointer(m))),
        "SI_getRawFcurvePointerPointer" => Outcome::Returned(Some(leftovers::get_raw_fcurve_pointer_pointer())),
        "setT2dMapDomainID" => {
            leftovers::set_t2d_map_domain_id(m, o, a(m, "id") as i32);
            done
        }
        "si_getLayoutWindow" => Outcome::Returned(Some(leftovers::get_layout_window(m, o, a(m, "layout") as i16))),
        "resetSize" => {
            leftovers::reset_size(m, o);
            done
        }
        "resetRotation" => {
            leftovers::reset_rotation(m, o);
            done
        }
        "SI_resetCounter" => {
            leftovers::reset_counter(m, o);
            done
        }
        "SI_refreshCounter" => {
            leftovers::refresh_counter(m, o);
            done
        }
        "SI_displayModelScal" => Outcome::Returned(Some(leftovers::display_model_scal(m, o, a(m, "out"), a(m, "model"), a(m, "show") as i16))),
        "set_init_buf" => {
            let from = a(m, "from");
            batch::set_init_buf(m, from);
            done
        }
        "get_init_value" => Outcome::Returned(Some(batch::get_init_value(m, a(m, "index") as i32) as u32)),
        "SI_replicateCommandLine" => {
            let (argc, argv) = (a(m, "argc") as i32, a(m, "argv"));
            Outcome::Returned(Some(batch::replicate_command_line(m, argc, argv)))
        }
        "SI_setupEmergencySave#off" => {
            batch::setup_emergency_save_off(m);
            done
        }
        "getDefaultPath" => {
            let (p, out) = (s(m, "prog"), a(m, "out"));
            batch::get_default_path(m, o, p, out);
            done
        }
        "SI_standDisplayVersion" => {
            let (n, e, v) = (s(m, "name"), s(m, "extra"), s(m, "version"));
            let (from, to) = (a(m, "from") as i32, a(m, "to") as i32);
            batch::stand_display_version(m, o, n, e, v, from, to);
            done
        }
        "usage" => {
            let (code, n) = (a(m, "code") as i32, s(m, "name"));
            Outcome::Exited(batch::usage(m, o, code, n).0)
        }
        "printUsage" => {
            let n = s(m, "name");
            Outcome::Exited(batch::print_usage(m, o, n).0)
        }
        "error" => {
            let (x, y) = (s(m, "message"), s(m, "detail"));
            Outcome::Exited(batch::error(m, o, x, y).0)
        }
        "testsubRegion" => exit(batch::test_sub_region(m, o)),
        "newValue" => match setup::new_value(m, o) {
            Ok(v) => Outcome::Returned(Some(v as u32)),
            Err(e) => Outcome::Exited(e.0),
        },
        "readSetup" => exit(setup::read_setup(m, o, a(m, "stream"))),
        "cleanUpProjLists" => {
            system::clean_up_proj_lists(m, o);
            done
        }
        "SI_updateDeformation" => Outcome::Returned(Some(system::update_deformation(m, o) as u32)),
        "SI_pickGrafNode" => {
            let (out, flags) = (a(m, "out"), a(m, "flags") as i32);
            Outcome::Returned(Some(system::pick_graf_node(m, o, out, flags)))
        }
        "verifyChecksum" => Outcome::Returned(Some(setup::verify_checksum() as u32)),
        "mdlZoom" => {
            let i = |m: &Mem, n: &str| a(m, n) as i32;
            let (w, r) = (a(m, "win"), [i(m, "vx0"), i(m, "vx1"), i(m, "vy0"), i(m, "vy1"), i(m, "rx0"), i(m, "rx1"), i(m, "ry0"), i(m, "ry1")]);
            Outcome::Returned(Some(system::mdl_zoom(m, o, w, r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], i(m, "button")) as u32))
        }
        "SI_deleteAll" => {
            system::delete_all(m, o);
            done
        }
        "SI_callRender" => Outcome::Returned(Some(batch::call_render(m, o) as u32)),
        "standAloneRender" => {
            let (argc, argv, res) = (a(m, "argc") as i32, a(m, "argv"), s(m, "res"));
            exit(batch::stand_alone_render(m, o, argc, argv, res))
        }
        other => return Err(format!("no port for `{other}`")),
    })
}

#[derive(Default)]
struct Count {
    cases: usize,
    same: usize,
    differ: usize,
    stopped_both: usize,
    not_compared: usize,
    undeclared: usize,
}

enum Verdict {
    Same,
    Differ(String),
    StoppedBoth,
    NotCompared,
}

fn check(c: &Case, mutate: &str) -> Verdict {
    if c.note("UNINIT").is_some() {
        return Verdict::NotCompared;
    }
    let mut m = Mem::new();
    si_app::image::init_data(&mut m);
    m.set_u32(g::STDIN, g::STDIN_HANDLE);
    m.set_u32(g::STDOUT, g::STDOUT_HANDLE);
    m.set_u32(g::STDERR, g::STDERR_HANDLE);
    let names = match place(&mut m, c) {
        Ok(n) => n,
        Err(e) => return Verdict::Differ(format!("the case cannot be placed: {e}")),
    };
    let text = |n: &str| c.input(n).map(|w| w.iter().filter_map(|w| if let Word::Value { value, .. } = w { Some(*value as u8) } else { None }).take_while(|&b| b != 0).collect::<Vec<u8>>()).unwrap_or_default();
    let mut o = Replay { calls: &c.calls, k: 0, names: names.clone(), cwd: text("cwdtext"), real: text("realtext"), uname: text("unametext"), stdin: text("stdin"), stdin_at: 0, diff: None, printed: [Vec::new(), Vec::new()], mutate_calls: mutate == "calls" };
    let outcome = match run(&mut m, &mut o, c, &names) {
        Ok(x) => x,
        Err(e) => return Verdict::Differ(e),
    };
    // where the original stopped other than through exit, the port must have faulted
    if let Some(stop) = c.note("STOP:") {
        if !stop.starts_with("STOP:exit:") {
            return if m.fault().is_some() || matches!(outcome, Outcome::Exited(x) if x == si_app::render_cmd::FAULT.0) {
                Verdict::StoppedBoth
            } else {
                Verdict::Differ(format!("the original stopped ({stop}), the port did not"))
            };
        }
    }
    let mutate1 = mutate == "1";
    let mut outcome = outcome;
    if mutate1 {
        // a bit of everything the port produced
        outcome = match outcome {
            Outcome::Returned(Some(v)) => Outcome::Returned(Some(v ^ 1)),
            // a routine that returned with nothing else to compare: the way it ended
            Outcome::Returned(None) => Outcome::Exited(1),
            Outcome::Exited(x) => Outcome::Exited(x ^ 1),
        };
        for (name, words) in &c.outputs {
            if words.as_ref().is_some_and(|w| !w.is_empty()) {
                if let Some(addr) = names.addr(&m, name).filter(|a| *a != 0) {
                    let b = m.u8(addr);
                    m.set_u8(addr, b ^ 1);
                }
            }
        }
        for p in o.printed.iter_mut() {
            if let Some(b) = p.first_mut() {
                *b ^= 1;
            }
        }
    }
    if let Some(d) = o.diff.take() {
        return Verdict::Differ(d);
    }
    if o.k < c.calls.len() {
        return Verdict::Differ(format!("the original made {} calls, the port {}", c.calls.len(), o.k));
    }
    // the result, or how the program ended
    match (c.note("STOP:exit:"), &outcome) {
        (Some(stop), Outcome::Exited(x)) => {
            if stop != format!("STOP:exit:{x}") {
                return Verdict::Differ(format!("original {stop}, port exit {x}"));
            }
        }
        (Some(stop), Outcome::Returned(_)) => return Verdict::Differ(format!("original {stop}, the port returned")),
        (None, Outcome::Exited(x)) => return Verdict::Differ(format!("the original returned, the port exits with {x}")),
        (None, Outcome::Returned(v)) => {
            for key in ["eax", "ptr"] {
                if let Some(Some(w)) = c.output(key) {
                    let want = w.first().and_then(|w| word_value(&m, &names, w));
                    if want.is_some() && want != *v {
                        return Verdict::Differ(format!("{key}: original {want:x?}, port {v:x?}"));
                    }
                }
            }
        }
    }
    // memory: the case's buffers and the heap
    let n_heap = c.outputs.iter().filter(|(n, _)| n.starts_with("heap") && n[4..].bytes().all(|b| b.is_ascii_digit())).count();
    if n_heap != m.heap_blocks().len() {
        return Verdict::Differ(format!("heap blocks: original {n_heap}, port {}", m.heap_blocks().len()));
    }
    for (name, words) in &c.outputs {
        match name.as_str() {
            "eax" | "ptr" | "stderr" | "stdout" | "freed" => continue,
            _ => {}
        }
        let Some(words) = words else { continue };
        let Some(addr) = names.addr(&m, name) else {
            return Verdict::Differ(format!("output `{name}`: no such buffer here"));
        };
        if let Err(e) = m.compare_words(addr, words, &names) {
            return Verdict::Differ(format!("{name}: {e}"));
        }
    }
    let freed: Vec<Option<u32>> = c.output("freed").and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| word_value(&m, &names, w)).collect()).unwrap_or_default();
    let port_freed: Vec<Option<u32>> = m.freed().iter().map(|p| Some(*p)).collect();
    if freed != port_freed {
        return Verdict::Differ(format!("freed: original {freed:x?}, port {port_freed:x?}"));
    }
    for (key, k) in [("stdout", 0), ("stderr", 1)] {
        let want: Vec<u8> = c.output(key).and_then(|w| w.as_ref()).map(|w| w.iter().map(|w| if let Word::Value { value, .. } = w { *value as u8 } else { 0 }).collect()).unwrap_or_default();
        if want != o.printed[k] {
            return Verdict::Differ(format!("{key}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(&o.printed[k])));
        }
    }
    if m.fault().is_some() {
        return Verdict::Differ(format!("the port faulted: {:?}", m.fault()));
    }
    Verdict::Same
}

fn main() {
    // the original's messages come from the user's install (unit rta)
    si_rt::check_or_exit("app-imagecheck");
    let dir = std::env::args().nth(1).expect("usage: app-imagecheck <vector directory>");
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("vector directory").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vec")).collect();
    files.sort();
    let mut table: BTreeMap<String, Count> = BTreeMap::new();
    let mut shown = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).expect("vector file");
        for line in text.lines() {
            let c = match parse(line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}: {e}", f.display()),
            };
            let t = table.entry(c.name.clone()).or_default();
            t.cases += 1;
            if c.note("GW:").is_some() {
                t.undeclared += 1;
            }
            match check(&c, &mutate) {
                Verdict::Same => t.same += 1,
                Verdict::StoppedBoth => t.stopped_both += 1,
                Verdict::NotCompared => t.not_compared += 1,
                Verdict::Differ(why) => {
                    t.differ += 1;
                    if shown < 40 && mutate.is_empty() {
                        shown += 1;
                        eprintln!("{}: {why}", c.name);
                    }
                }
            }
        }
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}", "routine", "cases", "identical", "different", "stopped", "not comp.");
    let mut total = Count::default();
    for (name, t) in &table {
        println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}{}", name, t.cases, t.same, t.differ, t.stopped_both, t.not_compared, if t.undeclared > 0 { format!("   {} with undeclared global writes", t.undeclared) } else { String::new() });
        total.cases += t.cases;
        total.same += t.same;
        total.differ += t.differ;
        total.stopped_both += t.stopped_both;
        total.not_compared += t.not_compared;
    }
    println!("{:<34} {:>7} {:>9} {:>9} {:>9} {:>9}", "total", total.cases, total.same, total.differ, total.stopped_both, total.not_compared);
    if !mutate.is_empty() {
        println!("mutation {mutate}: {} of {} compared cases differ", total.differ, total.cases - total.not_compared - total.stopped_both);
    }
}
