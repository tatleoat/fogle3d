// SPDX-License-Identifier: MIT
//! `app-oraclecheck <oracle dir> <install dir> <data1 dir>`: the image form's start-up held
//! against what the original printed and how it ended, for every run of `soft` the oracle
//! made (finding 0031): `-h` byte for byte, and for every batch command the text up to and
//! including the hand-over to Get Scene ("Reading scene from Database […]"), what the
//! port prints after the load, and the exit status.
//!
//! The port runs `si_app::image::batch::start_up` on the command line of the run, then
//! `finish` with the load answered by the check. The outside world is the container as
//! `re/tools/oracle/inside.sh` sets it up (read, not run): the working directory
//! `/tmp/work`, `HOME`, `SI_DBDIR`, one database in the list (the run's `ORACLE_DBNAME` and
//! `ORACLE_DBPATH`), the install tree at `/opt/soft3d`, read-only databases, uid 1000
//! named `soft`, the clock at the run's start. **Stand-ins of the check** (not ports): the
//! database list that `SI_initDictionaries` reads (one record), `SI_dbChapterCheckAccess`
//! (0: the databases are mounted read-only), `SI_strStripDir` (the name after its last
//! `/`, a new string), and the load: it answers 1 and leaves in `SI_defaultControl` what the
//! options and `SI_callRender` read of the scene's set-up, the output name and the
//! renderer's kind, from the oracle's `scenes.tsv`. Every other routine of another
//! package answers 0. Nothing here reproduces a function of the original.
//!
//! The original's standard output is a pipe (block-buffered: flushed at exit), its
//! standard error stream unbuffered: what the port prints after the load, on either
//! stream, must be found in the run's text after the hand-over (the standard output's
//! "Rendering..." comes at the end, followed by whatever the renderer printed there).
//!
//! `EMUCHECK_MUTATE=1` flips a bit of what the port printed and of its status: every run
//! must then differ.

use si_app::image::batch::{self, ctl};
use si_app::image::g;
use si_mem::{Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One command of a run: its arguments, what it printed, its status, the database the
/// container listed, the run's start time.
struct Command {
    run: String,
    args: Vec<String>,
    printed: Vec<u8>,
    status: i32,
    db: Option<(String, String)>,
    start: i64,
}

/// Seconds since 1970 of `2026-10-14T03:59:28Z`.
fn epoch(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    // days from civil (Howard Hinnant's algorithm)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

/// The year of a time (UTC).
fn year_of(t: i64) -> i64 {
    let days = t.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + (m <= 2) as i64
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from);
    }
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// The commands of one run directory.
fn commands(dir: &Path) -> Vec<Command> {
    let Ok(log) = std::fs::read(dir.join("log.txt")) else { return Vec::new() };
    let run_txt = std::fs::read_to_string(dir.join("run.txt")).unwrap_or_default();
    let env = |key: &str| run_txt.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string));
    let db = env("ORACLE_DBNAME").zip(env("ORACLE_DBPATH"));
    let run = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Vec::new();
    let mut batch_start = 0;
    let mut pos = 0;
    while let Some(nl) = find(&log, b"\n", pos) {
        let line = String::from_utf8_lossy(&log[pos..nl]).into_owned();
        let after = nl + 1;
        pos = after;
        if let Some(t) = line.strip_prefix("oracle: batch start ") {
            batch_start = epoch(t).unwrap_or(0);
            continue;
        }
        // "oracle: start <time>: soft …" or "oracle: [id] start: soft …"
        let (start, cmd, end_marker): (i64, &str, Vec<u8>) = if let Some(rest) = line.strip_prefix("oracle: start ") {
            let Some((t, cmd)) = rest.split_once(": ") else { continue };
            (epoch(t).unwrap_or(0), cmd, b"oracle: end ".to_vec())
        } else if let Some(rest) = line.strip_prefix("oracle: [") {
            let Some((id, cmd)) = rest.split_once("] start: ") else { continue };
            (batch_start, cmd, format!("oracle: [{id}] exit status ").into_bytes())
        } else {
            continue;
        };
        if !(cmd == "soft" || cmd.starts_with("soft ")) {
            continue;
        }
        let Some(end) = find(&log, &end_marker, after) else { continue };
        let tail = &log[end..];
        let status_text = String::from_utf8_lossy(&tail[..tail.iter().position(|&c| c == b'\n').unwrap_or(tail.len())]).into_owned();
        let status = status_text.split("exit status ").nth(1).and_then(|s| s.split([',', ' ']).next()).and_then(|s| s.parse().ok()).unwrap_or(-999);
        let args = cmd.split(' ').map(str::to_string).collect();
        out.push(Command { run: run.clone(), args, printed: log[after..end].to_vec(), status, db: db.clone(), start });
        pos = end;
    }
    out
}

/// The container as the port's outside world.
struct Container<'a> {
    install: &'a Path,
    data: &'a Path,
    db: Option<(String, String)>,
    now: i64,
    scenes: &'a BTreeMap<(String, String), (String, String)>,
    chapter: Vec<u8>,
    db_rec: u32,
    env_cache: BTreeMap<String, u32>,
    printed: [Vec<u8>; 2],
    handed_over: Option<usize>,
    unanswered: BTreeMap<&'static str, usize>,
    /// The scene the load was asked for, and the renderer the check gave it.
    load: Option<(String, i16)>,
}

impl Container<'_> {
    /// A path inside the container, on this machine (the mounts of run.sh).
    fn host(&self, p: &[u8]) -> Option<PathBuf> {
        let p = String::from_utf8_lossy(p).into_owned();
        if let Some(rest) = p.strip_prefix("/opt/soft3d") {
            return Some(self.install.join(rest.trim_start_matches('/')));
        }
        if let (Some(rest), Some((name, _))) = (p.strip_prefix("/data/"), &self.db) {
            let (first, more) = rest.split_once('/').unwrap_or((rest, ""));
            if first == name {
                // the database is mounted from its place under data1; found by name
                let found = find_db(self.data, name)?;
                return Some(found.join(more));
            }
        }
        None
    }
}

fn find_db(data: &Path, name: &str) -> Option<PathBuf> {
    // data1/<collection>/databases/…/<name>
    let mut stack = vec![(data.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        if d.file_name().is_some_and(|n| n == name) && d.join("SCENES").is_dir() {
            return Some(d);
        }
        if depth < 5 {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        stack.push((e.path(), depth + 1));
                    }
                }
            }
        }
    }
    None
}

const ENV: &[(&str, &str)] = &[("HOME", "/tmp/home"), ("TMPDIR", "/tmp"), ("PATH", "/usr/X11R6/bin:/usr/bin:/bin"), ("SI_DBDIR", "/tmp/work"), ("DISPLAY", ":99")];

impl Outside for Container<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
        match r.name {
            "getenv" => {
                let name = String::from_utf8_lossy(&m.cstr(a[0])).into_owned();
                match ENV.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => {
                        let p = *self.env_cache.entry(name).or_insert_with(|| m.place_cstr(v.as_bytes()));
                        Ret::eax(p)
                    }
                    None => Ret::NONE,
                }
            }
            "getcwd" => Ret::eax(batch::libc_getcwd(m, a[0], a[1], b"/tmp/work")),
            "access" => {
                let p = m.cstr(a[0]);
                Ret::int(if self.host(&p).is_some_and(|h| h.exists()) { 0 } else { -1 })
            }
            "time" => {
                if a[0] != 0 {
                    m.set_i32(a[0], self.now as i32);
                }
                Ret::int(self.now as i32)
            }
            "localtime" => {
                let tm = m.place(0x2c, 4);
                m.set_i32(tm + 0x14, (year_of(self.now) - 1900) as i32);
                Ret::eax(tm)
            }
            "geteuid" => Ret::eax(1000),
            "getpwuid" => {
                let name = m.place_cstr(b"soft");
                let pw = m.place(0x1c, 4);
                m.set_u32(pw, name);
                Ret::eax(pw)
            }
            "setlocale" => Ret::eax(if a[1] == 0 { m.place_cstr(b"C") } else { a[1] }),
            "SI_initDictionaries" => {
                if let Some((name, path)) = self.db.clone() {
                    if self.db_rec == 0 {
                        let rec = m.place(0x1c, 4);
                        let (n, p) = (m.place_cstr(name.as_bytes()), m.place_cstr(path.as_bytes()));
                        m.set_u32(rec + 8, n);
                        m.set_u32(rec + 0xc, p);
                        m.set_i16(rec + 0x18, 1);
                        self.db_rec = rec;
                    }
                    m[g::SI_PDBIIN] = self.db_rec;
                    m[g::SI_PDBIOUT] = self.db_rec;
                }
                Ret::NONE
            }
            "SI_dbSetChapter" => {
                self.chapter = m.cstr(a[1]);
                Ret::NONE
            }
            "SI_dbfilepathBuild" => {
                let base = m.cstr(m.u32(a[0] + 0xc));
                let p = m.place_cstr(&[&base[..], b"/", &self.chapter].concat());
                m.set_u32(a[0] + 0x14, p);
                Ret::NONE
            }
            "SI_dbChapterCheckAccess" => Ret::NONE,
            "SI_grafListAdd" => Ret::int(1),
            "SI_newCamera" => Ret::eax(m.place(0x100, 4)),
            "SI_strStripDir" => {
                if a[0] == 0 {
                    return Ret::NONE;
                }
                let s = m.cstr(a[0]);
                let base = s.rsplit(|&c| c == b'/').next().unwrap_or(&[]).to_vec();
                let p = m.malloc(base.len() as u32 + 1);
                m.put_cstr(p, &base);
                Ret::eax(p)
            }
            "SI_dbLoadScene" => {
                // the hand-over: the check's load (see the top of the file)
                self.handed_over = Some(self.printed[1].len());
                let scene = String::from_utf8_lossy(&m.cstr(a[2])).into_owned();
                let db = self.db.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                let (kind, out) = self.scenes.get(&(db, scene.clone())).cloned().unwrap_or_else(|| ("RENDERING_LINE".into(), "default".into()));
                // the renderer's kind: native 4, hidden line 1, mental ray 10; the rotoscope
                // wire frame and the hardware renderer stand for numbers SI_callRender refuses
                let renderer = match kind.as_str() {
                    "RENDERING_LINE" => 4,
                    "HIDDEN_LINE" => 1,
                    "MENTAL_RAY" => 10,
                    _ => 3,
                };
                m.set_i16(g::SI_DEFAULT_CONTROL.0 + ctl::RENDERER, renderer);
                let p = m.malloc(out.len() as u32 + 1);
                m.put_cstr(p, out.as_bytes());
                m.set_u32(g::SI_DEFAULT_CONTROL.0 + ctl::OUT_NAME, p);
                self.load = Some((scene, renderer));
                Ret::int(1)
            }
            "SI_menuTableInit" | "SI_statusInitPath" | "SI_isMr19Enable" | "SI_setmi2Output" | "SI_initWave" | "SI_grafListCreate" | "SI_graflistAddData" | "SI_matterInit" | "SI_metaballInit" | "SI_cusInit" | "SI_cusCallCustomFunctions" | "SI_sequenceWireFrame" | "SI_sequenceHiddenLine" | "SI_sequenceRender" | "SI_sequenceRenderMental" => Ret::NONE,
            other => {
                *self.unanswered.entry(other).or_default() += 1;
                Ret::NONE
            }
        }
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.printed[(stream == 2) as usize].extend_from_slice(text);
    }
}

#[derive(Default)]
struct Tally {
    runs: usize,
    same: usize,
    differ: usize,
}

fn main() {
    // the original's messages come from the user's install (unit rta)
    si_rt::check_or_exit("app-oraclecheck");
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: app-oraclecheck <re/out/oracle> <installed/linux/soft3d> <installed/nt/db/data1> [out.tsv]");
        std::process::exit(2);
    }
    let (oracle, install, data) = (Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]));
    let mutate = std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "1");
    // scenes.tsv: (database's last name, scene) -> (rendering type, output name)
    let mut scenes = BTreeMap::new();
    for line in std::fs::read_to_string(oracle.join("scenes.tsv")).unwrap_or_default().lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 7 {
            let db = f[0].rsplit('/').next().unwrap_or("").to_string();
            scenes.insert((db, f[1].to_string()), (f[3].to_string(), f[6].trim_matches('\'').to_string()));
        }
    }
    let mut runs: Vec<PathBuf> = std::fs::read_dir(oracle.join("runs")).expect("runs").flatten().map(|e| e.path()).collect();
    runs.sort();
    let mut all = Vec::new();
    for r in &runs {
        all.extend(commands(r));
    }
    let mut tsv = String::from("run\tcommand\tkind\tverdict\twhy\n");
    let mut tally: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut unanswered: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut shown = 0;
    // the -h text of help.txt, as a run of its own
    if let Ok(help) = std::fs::read(oracle.join("help.txt")) {
        let text = String::from_utf8_lossy(&help).into_owned();
        if let (Some(s), Some(e)) = (text.find("oracle: start "), text.find("oracle: end ")) {
            let line_end = text[s..].find('\n').map(|k| s + k + 1).unwrap_or(s);
            let t = text[s + 14..].split(": ").next().and_then(epoch).unwrap_or(0);
            all.push(Command { run: "help.txt".into(), args: vec!["soft".into(), "-h".into()], printed: help[line_end..e].to_vec(), status: 0, db: None, start: t });
        }
    }
    for c in &all {
        let kind = if c.args.iter().any(|x| x == "-R") { "batch" } else { "interactive" };
        let mut m = Mem::new();
        let mut w = Container { install, data, db: c.db.clone(), now: c.start, scenes: &scenes, chapter: Vec::new(), db_rec: 0, env_cache: BTreeMap::new(), printed: [Vec::new(), Vec::new()], handed_over: None, unanswered: BTreeMap::new(), load: None };
        // the program as inside.sh starts it: the install's bin directory
        let mut argv: Vec<Vec<u8>> = c.args.iter().map(|s| s.clone().into_bytes()).collect();
        argv[0] = b"/opt/soft3d/Creative-Tools/3D/bin/soft".to_vec();
        let refs: Vec<&[u8]> = argv.iter().map(|v| v.as_slice()).collect();
        let exit = batch::run(&mut m, &mut w, &refs);
        for (k, n) in &w.unanswered {
            *unanswered.entry(k).or_default() += n;
        }
        let mut status = (exit.0 & 0xff) as i32;
        let mut err = w.printed[1].clone();
        let mut out = w.printed[0].clone();
        if mutate {
            status ^= 1;
            if let Some(b) = err.first_mut() {
                *b ^= 1;
            }
            if let Some(b) = out.first_mut() {
                *b ^= 1;
            }
        }
        let orig = &c.printed;
        let why = (|| -> Option<String> {
            if exit == batch::INTERACTIVE {
                return Some("the port reads an interactive command line".into());
            }
            if status != c.status {
                return Some(format!("status: original {}, port {status}", c.status));
            }
            match w.handed_over {
                None => {
                    // the program ended before the load: everything it printed, byte for byte
                    let mut port = err.clone();
                    port.extend_from_slice(&out);
                    (port != *orig).then(|| format!("printed: original {:?}, port {:?}", String::from_utf8_lossy(orig), String::from_utf8_lossy(&port)))
                }
                Some(k) => {
                    let pre = &err[..k];
                    if !orig.starts_with(pre) {
                        return Some(format!("up to the hand-over: original {:?}, port {:?}", String::from_utf8_lossy(&orig[..orig.len().min(pre.len() + 40)]), String::from_utf8_lossy(pre)));
                    }
                    if find(orig, &err[k..], pre.len()).is_none() {
                        return Some(format!("after the load the port prints {:?}, not in the original's text", String::from_utf8_lossy(&err[k..])));
                    }
                    // the standard output comes when the program ends, the renderer's own
                    // lines on it after the port's ("no visible triangles")
                    if find(orig, &out, pre.len()).is_none() {
                        return Some(format!("standard output {:?} is not in the original's text after the hand-over", String::from_utf8_lossy(&out)));
                    }
                    None
                }
            }
        })();
        let t = tally.entry(kind).or_default();
        t.runs += 1;
        let verdict = match &why {
            None => {
                t.same += 1;
                "identical"
            }
            Some(why) => {
                t.differ += 1;
                if shown < 20 && !mutate {
                    shown += 1;
                    eprintln!("{} {}: {why}", c.run, c.args.join(" "));
                }
                "different"
            }
        };
        tsv.push_str(&format!("{}\t{}\t{kind}\t{verdict}\t{}\n", c.run, c.args.join(" "), why.unwrap_or_default().replace(['\t', '\n'], " ")));
    }
    println!("{:<14} {:>7} {:>9} {:>9}", "kind", "runs", "identical", "different");
    for (k, t) in &tally {
        println!("{:<14} {:>7} {:>9} {:>9}", k, t.runs, t.same, t.differ);
    }
    if !unanswered.is_empty() {
        println!("calls the check did not expect: {unanswered:?}");
    }
    if let Some(p) = a.get(4) {
        std::fs::write(p, tsv).expect("tsv");
    }
}
