// SPDX-License-Identifier: MIT
//! Corpus checks: run the Rust code over every original file under a directory and count
//! outcomes. The numbers are the evidence quoted in the findings.
//!   si-check headers <dir>     every file's header, by magic (finding 0002)
//!   si-check pictures <dir>    decode every .pic/.icn and every appended icon, encode again,
//!                              compare with the file's bytes (finding 0003)
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            files(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The program's description of a file type, read from the user's install; without one the
/// run stops with the reason (a missing install is not an unknown type).
fn magic_name(magic: u32) -> Option<&'static str> {
    match si_file::magic_type(magic) {
        Ok(name) => name,
        Err(e) => {
            eprintln!("si-check: {e}");
            std::process::exit(2);
        }
    }
}

fn headers(root: &Path) {
    let mut all = Vec::new();
    files(root, &mut all);
    let mut by: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut total = 0;
    for p in &all {
        // the header is all we need: read the first chunk only
        let Ok(mut f) = std::fs::File::open(p) else { continue };
        let mut head = [0u8; si_file::HEADER_SIZE];
        let n = std::io::Read::read(&mut f, &mut head).unwrap_or(0);
        let head = &head[..n];
        let key = if let Some(h) = si_file::read_bin_header(&mut si_file::Stream::new(head)) {
            format!("0x{:08x} {}", h.magic, magic_name(h.magic).unwrap_or("(not in the table)"))
        } else if head.len() >= 4 && si_file::is_ascii_header_tag(u32::from_be_bytes([head[0], head[1], head[2], head[3]])) {
            let t = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
            format!("{} {}", String::from_utf8_lossy(&head[..4]), magic_name(t).unwrap_or(""))
        } else {
            continue;
        };
        total += 1;
        *by.entry(key).or_default().entry(ext(p)).or_default() += 1;
    }
    for (k, exts) in &by {
        let n: usize = exts.values().sum();
        let mut e: Vec<_> = exts.iter().collect();
        e.sort_by(|a, b| b.1.cmp(a.1));
        let list: Vec<String> = e.iter().map(|(x, c)| format!("{x} {c}")).collect();
        println!("{n:6}  {k}: {}", list.join(", "));
    }
    println!("{total} files with a header");
}

fn pictures(root: &Path) {
    let mut all = Vec::new();
    files(root, &mut all);
    let mut res: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut icons_same, mut icons_diff, mut icons_stale) = (0, 0, 0);
    for p in &all {
        let Ok(data) = std::fs::read(p) else { continue };
        let is_pic = matches!(ext(p).as_str(), "pic" | "icn");
        let icon = si_picture::read_icon(&data);
        if let Some((icon, end)) = &icon {
            let off = si_picture::icon_offset(&data).unwrap();
            // rows start after the icon's 12-byte chunk
            if si_picture::encode_rows(icon) == data[off + 12..*end] {
                icons_same += 1;
            } else {
                icons_diff += 1;
            }
            // an icon written over a larger one leaves the old one's tail between its own
            // trailer and the end of the file (the file is never shortened)
            if *end != data.len() - 8 {
                icons_stale += 1;
            }
        }
        if !is_pic {
            continue;
        }
        let f = match si_picture::Picture::read(&data) {
            Ok(f) => f,
            Err(_) => {
                *res.entry("refused").or_default() += 1;
                continue;
            }
        };
        let ends_right = match si_picture::icon_offset(&data) {
            Some(off) => f.end == off,
            None => f.end == data.len(),
        };
        if !ends_right {
            *res.entry("does not end where the file or its icon says").or_default() += 1;
            continue;
        }
        let start = 104 + 4 * f.packets.len();
        let same = si_picture::encode_rows(&f.picture) == data[start..f.end];
        *res.entry(if same { "decoded to the last byte; encodes back identically" } else { "decoded to the last byte; encodes back differently" }).or_default() += 1;
    }
    for (k, n) in &res {
        println!("{n:6}  {k}");
    }
    println!("icons: {icons_same} encode back identically, {icons_diff} differently; {icons_stale} are followed by the tail of an older, larger icon");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("headers"), Some(d)) => headers(Path::new(d)),
        (Some("pictures"), Some(d)) => pictures(Path::new(d)),
        _ => {
            eprintln!("usage: si-check headers|pictures <dir>");
            std::process::exit(2);
        }
    }
}
