// SPDX-License-Identifier: MIT
//! `scene-dump`: sample scenes built in the memory image by the ports and written out as
//! the starting memory of an emulation check (finding 0011).
//!
//!   scene-dump <corpus dir> <out dir> [--stage dict|objects|list] [--only <text>]
//!              [--max <n>] [--rsrc <Relations.rsrc>]
//!
//! Every scene description (`SCENES/*.dsc`) under the corpus directory, in the order of
//! their paths; `--only` keeps those whose path contains the text, `--max` the first n.
//! Per scene `<out dir>/<number>.bin` and `.idx` (see `si_emuworld::dump`), and one line
//! on standard output. One scene is in memory at a time. Run under the memory cap:
//! `(ulimit -v 2000000; timeout 900 scene-dump …)`. The files are derived from the
//! proprietary samples: they belong under `re/out/` and are never committed.

use si_emuworld::build::{build, Stage};
use si_emuworld::dump;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

fn main() {
    // model hierarchies recurse once per node in the readers
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap_or(3));
}

fn run() -> i32 {
    let mut positional: Vec<String> = Vec::new();
    let (mut stage, mut only, mut max, mut rsrc) = (Stage::List, None::<String>, usize::MAX, None::<PathBuf>);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--stage" => match it.next().as_deref().and_then(Stage::parse) {
                Some(s) => stage = s,
                None => {
                    eprintln!("--stage dict|objects|list");
                    return 2;
                }
            },
            "--only" => only = it.next(),
            "--max" => max = it.next().and_then(|n| n.parse().ok()).unwrap_or(usize::MAX),
            "--rsrc" => rsrc = it.next().map(PathBuf::from),
            _ => positional.push(a),
        }
    }
    if positional.len() != 2 {
        eprintln!("usage: scene-dump <corpus dir> <out dir> [--stage dict|objects|list] [--only <text>] [--max <n>] [--rsrc <Relations.rsrc>]");
        return 2;
    }
    let (dir, out) = (PathBuf::from(&positional[0]), PathBuf::from(&positional[1]));
    let mut files = Vec::new();
    walk(&dir, &mut files);
    let rsrc_path = rsrc.or_else(|| files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned());
    let Some(rsrc_path) = rsrc_path else {
        eprintln!("no Relations.rsrc under {} (give one with --rsrc)", dir.display());
        return 2;
    };
    let Ok(rsrc) = std::fs::read(&rsrc_path) else {
        eprintln!("cannot read {}", rsrc_path.display());
        return 2;
    };
    let scenes: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "dsc") && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "SCENES"))
        .filter(|p| only.as_ref().is_none_or(|o| p.to_string_lossy().contains(o.as_str())))
        .collect();
    println!("relations resource file: {}", rsrc_path.display());
    println!("rsrc\t{}", rsrc_path.display());
    let mut written = 0usize;
    for path in scenes {
        if written >= max {
            break;
        }
        let data = std::fs::read(path).unwrap_or_default();
        if !data.starts_with(b"SDSC") {
            continue;
        }
        let db = path.parent().unwrap().parent().unwrap();
        let name = path.file_stem().unwrap().as_bytes().to_vec();
        let label = path.strip_prefix(&dir).unwrap_or(path).display().to_string();
        let w = match build(db, &name, &rsrc, stage) {
            Ok(w) => w,
            Err(e) => {
                println!("{label}\tnot built: {e}");
                continue;
            }
        };
        let file = format!("{written:04}");
        match dump::write(&w, &label, db, &name, &rsrc_path, &out, &file) {
            Ok(bytes) => {
                let h = w.sc.mem.heap_stats();
                let counts: Vec<String> = w.counts.iter().map(|(k, v)| format!("{v} {k}")).collect();
                println!("{file}\t{label}\tloaded {}\t{bytes} bytes\theap {} live blocks {}\t{}", w.loaded, h.used, h.live_blocks, counts.join("; "));
                written += 1;
            }
            Err(e) => println!("{label}\tnot written: {e}"),
        }
    }
    println!("{written} scenes written to {}", out.display());
    0
}
