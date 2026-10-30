// SPDX-License-Identifier: MIT
//! Runs the readers (and, with `--write`, the writers) over every `.hrc`, `.mtr`, `.txt`
//! and `.t3D` file under a directory and reports counts.
//!
//! ```text
//!   hrc-check <dir>            how each file's read ends, per file type
//!   hrc-check --write <dir>    also the round trip and the byte-for-byte re-encoding
//!   hrc-check --list <dir>     as the first, and names every file that does not end cleanly
//!   hrc-check --tree <file>    the hierarchy of one model, a line per node
//!   hrc-check --obj <file> <out.obj>   its polygon meshes as Wavefront OBJ (local coordinates)
//!   hrc-check --diff <file> [own]      where the rewritten file differs
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{proto, read_file, write_as_read, write_content, Content, Definition, Kind, Prototypes};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn kind_of(p: &Path) -> Option<(Kind, &'static str)> {
    match p.extension()?.to_str()? {
        "hrc" => Some((Kind::Model, "hrc")),
        "mtr" => Some((Kind::Material, "mtr")),
        "txt" => Some((Kind::Texture, "txt")),
        "t3D" => Some((Kind::Texture3D, "t3D")),
        _ => None,
    }
}

#[derive(Default)]
struct Tally {
    files: u32,
    ends: BTreeMap<String, (u32, String)>,
    skipped: BTreeMap<(usize, u16), u32>,
    unknown_plain: BTreeMap<(usize, u16), u32>,
    cut: u32,
    changed: BTreeMap<&'static str, u32>,
    nodes: u32,
    node_kinds: BTreeMap<i32, u32>,
    versions: BTreeMap<String, u32>,
    // --write
    written: u32,
    same_data: u32,
    stable: u32,
    reread_to_end: u32,
    null_pointers: u32,
    data_differs: BTreeMap<String, (u32, String)>,
    current_table: u32,
    identical_as_program: u32,
    own_identical: u32,
    own_identical_by_version: BTreeMap<String, (u32, u32)>,
    own_differs: BTreeMap<String, (u32, String)>,
    own_unwritable: u32,
    unwritable: BTreeMap<(usize, u16), u32>,
}

/// Where two byte strings first differ.
fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

fn main() {
    // brother chains recurse once per node
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|a| a == "--list");
    let write = args.iter().any(|a| a == "--write");
    let current = Prototypes::current(4.0);
    match (args.first().map(String::as_str), args.len()) {
        (Some("--diff"), 2..) => return diff(&args[1], args.len() > 2),
        (Some("--tree"), 2..) => return tree(&args[1], None),
        (Some("--obj"), 3..) => return tree(&args[1], Some(&args[2])),
        _ => {}
    }
    let Some(dir) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: hrc-check [--list] [--write] <dir>");
        return 2;
    };
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let mut tallies: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for p in &files {
        let Some((kind, ext)) = kind_of(p) else { continue };
        let Ok(data) = std::fs::read(p) else { continue };
        let t = tallies.entry(ext).or_default();
        t.files += 1;
        let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
        let note = |t: &mut Tally, how: String| {
            if list && !how.starts_with("ends") {
                println!("{how}\t{rel}");
            }
            let e = t.ends.entry(how).or_insert((0, rel.clone()));
            e.0 += 1;
        };
        if data.is_empty() {
            note(t, "empty file".into());
            continue;
        }
        let Some(f) = read_file(&data, kind) else {
            note(t, "no binary header: not read".into());
            continue;
        };
        if f.header.is_none() {
            note(t, "no binary header (a text file of another subsystem): nothing read".into());
            continue;
        }
        let version = f.header.as_ref().unwrap().version;
        let how = if matches!(f.content, Content::None) {
            "header accepted, but no record tag: nothing read".to_string()
        } else if f.end == data.len() {
            "ends at the end of the file".to_string()
        } else if si_picture::icon_offset(&data) == Some(f.end) {
            "ends where the appended icon starts".to_string()
        } else if f.end > data.len() {
            "reads past the end".to_string()
        } else {
            let left = data.len() - f.end;
            match si_picture::icon_offset(&data) {
                Some(off) if off > f.end => format!("stops {} bytes before the icon (version {})", off - f.end, version),
                Some(_) => format!("stops inside the icon (version {})", version),
                None if f.notes.cut > 0 => format!("file ends inside a list (version {})", version),
                None => format!("stops {} bytes before the end (version {})", left.min(99_999), version),
            }
        };
        let clean = how.starts_with("ends");
        note(t, how);
        if clean {
            *t.versions.entry(format!("{version}")).or_insert(0) += 1;
        }
        for (k, v) in &f.notes.skipped {
            *t.skipped.entry(*k).or_insert(0) += v;
        }
        for (k, v) in &f.notes.unknown_plain {
            *t.unknown_plain.entry(*k).or_insert(0) += v;
        }
        t.cut += f.notes.cut;
        for k in f.notes.changed.keys() {
            *t.changed.entry(k).or_insert(0) += 1;
        }
        t.nodes += f.notes.nodes;
        if let Content::Model(root) = &f.content {
            root.walk(0, &mut |_, n| *t.node_kinds.entry(n.kind).or_insert(0) += 1);
        }
        if !write || !clean {
            continue;
        }
        // 1. what the program writes for this content, read again and written again
        let bytes1 = write_content(&f.content).unwrap();
        t.written += 1;
        let again = read_file(&bytes1, kind).unwrap();
        if again.end == bytes1.len() {
            t.reread_to_end += 1;
        }
        let reasons = |m: &BTreeMap<&'static str, u32>| -> String {
            if m.is_empty() { "NOT EXPLAINED".to_string() } else { m.keys().copied().collect::<Vec<_>>().join("; ") }
        };
        if again.content == f.content || format!("{:?}", again.content) == format!("{:?}", f.content) {
            t.same_data += 1;
        } else {
            t.data_differs.entry(reasons(&f.notes.changed)).or_insert((0, rel.clone())).0 += 1;
        }
        if write_content(&again.content).unwrap() == bytes1 {
            t.stable += 1;
        }
        if f.protos.block == current.block {
            t.current_table += 1;
        }
        if bytes1 == data[..f.end] {
            t.identical_as_program += 1;
        }
        // 2. the file's own table through the same per-token code
        let w = write_as_read(&f).unwrap();
        t.null_pointers += (w.null_pointers > 0) as u32;
        let by_version = t.own_identical_by_version.entry(format!("{version}")).or_insert((0, 0));
        by_version.1 += 1;
        if !w.unwritable.is_empty() {
            t.own_unwritable += 1;
            for (k, v) in &w.unwritable {
                *t.unwritable.entry(*k).or_insert(0) += v.min(&1);
            }
        } else if w.w.out == data[..f.end] {
            t.own_identical += 1;
            by_version.0 += 1;
        } else {
            let at = first_difference(&w.w.out, &data[..f.end]);
            let what = if at < 88 { "in the header".to_string() } else { reasons(&f.notes.changed) };
            if list {
                println!("differs at {at} of {}\t{rel}", f.end);
            }
            let e = t.own_differs.entry(what).or_insert((0, format!("{rel} at byte {at}")));
            e.0 += 1;
        }
    }
    let mut bad = 0;
    for (ext, t) in &tallies {
        let clean: u32 = t.ends.iter().filter(|(k, _)| k.starts_with("ends")).map(|(_, v)| v.0).sum();
        println!(".{ext}: {} files, {} read to their exact end", t.files, clean);
        bad += t.files - clean;
        for (how, (n, example)) in &t.ends {
            if how.starts_with("ends") {
                println!("  {n:6}  {how}");
            } else {
                println!("  {n:6}  {how}   e.g. {example}");
            }
        }
        let show = |m: &BTreeMap<(usize, u16), u32>| {
            m.iter().map(|((p, tok), n)| format!("{} 0x{:x}: {}", proto::name(*p), tok, n)).collect::<Vec<_>>().join(", ")
        };
        if !t.skipped.is_empty() {
            println!("  tokens skipped by their length: {}", show(&t.skipped));
        }
        if !t.unknown_plain.is_empty() {
            println!("  unknown tokens without a length (nothing read for them): {}", show(&t.unknown_plain));
        }
        if t.cut > 0 {
            println!("  lists cut at the end of a file: {}", t.cut);
        }
        for (what, n) in &t.changed {
            println!("  {n:6}  files where the reader changes or drops a stored value: {what}");
        }
        if t.nodes > 0 {
            let kinds: Vec<String> = t.node_kinds.iter().map(|(k, n)| format!("{k}: {n}")).collect();
            println!("  hierarchy nodes: {} (by type: {})", t.nodes, kinds.join(", "));
        }
        let versions: Vec<String> = t.versions.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        println!("  versions of the files read cleanly: {}", versions.join(", "));
        if !write {
            continue;
        }
        println!("  written as the program writes (version 4.0 table): {}", t.written);
        println!("    {:6}  read again to the exact end", t.reread_to_end);
        println!("    {:6}  read again give the same data", t.same_data);
        println!("    {:6}  written a second time give the same bytes", t.stable);
        println!("    {:6}  are byte for byte the original file ({} files carry the version 4.0 table)", t.identical_as_program, t.current_table);
        for (why, (n, example)) in &t.data_differs {
            println!("    {n:6}  give other data: {why}   e.g. {example}");
        }
        println!("  written with the file's own table:");
        println!("    {:6}  byte for byte the original file", t.own_identical);
        println!("    {:6}  have a table with tokens this build never writes: {}", t.own_unwritable, show(&t.unwritable));
        for (what, (n, example)) in &t.own_differs {
            println!("    {n:6}  differ: {what}   e.g. {example}");
        }
        let by: Vec<String> = t.own_identical_by_version.iter().map(|(k, (a, b))| format!("{k}: {a}/{b}")).collect();
        println!("    identical by version: {}", by.join(", "));
        if t.null_pointers > 0 {
            println!("    files where the writer met a null pointer: {}", t.null_pointers);
        }
    }
    (bad > 0) as i32
}

/// `hrc-check --diff <file> [own]`: for one file, where the data read back from the
/// program's output differs from the data read from the file (as lines of the debug
/// printout), or with `own`, where the bytes written with the file's own table differ.
fn diff(path: &str, own: bool) -> i32 {
    let p = Path::new(path);
    let Some((kind, _)) = kind_of(p) else { return 2 };
    let data = std::fs::read(p).unwrap();
    let Some(f) = read_file(&data, kind) else { return 2 };
    if own {
        let w = write_as_read(&f).unwrap();
        let at = first_difference(&w.w.out, &data[..f.end]);
        println!("file ends its record at {}, written {} bytes, first difference at {at}", f.end, w.w.out.len());
        let lo = at.saturating_sub(24);
        println!("file:    {:02x?}", &data[lo..(at + 24).min(data.len())]);
        println!("written: {:02x?}", &w.w.out[lo..(at + 24).min(w.w.out.len())]);
        println!("unwritable: {:?}", w.unwritable);
        return 0;
    }
    let bytes = write_content(&f.content).unwrap();
    let again = read_file(&bytes, kind).unwrap();
    let a = format!("{:#?}", f.content);
    let b = format!("{:#?}", again.content);
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    println!("{} and {} lines", a.len(), b.len());
    let mut shown = 0;
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            let from = i.saturating_sub(3);
            for k in from..i {
                println!("    {}", a[k]);
            }
            println!("  - {x}\n  + {y}");
            shown += 1;
            if shown == 1 {
                break;
            }
        }
    }
    0
}

/// `--tree`: one line per node; with an output path, the meshes as OBJ instead.
fn tree(path: &str, obj: Option<&str>) -> i32 {
    let data = std::fs::read(path).unwrap();
    let Some(f) = read_file(&data, Kind::Model) else { return 2 };
    let Content::Model(root) = &f.content else { return 2 };
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut out = String::new();
    let mut base = 1usize;
    root.walk(0, &mut |depth, n| {
        let what = match n.definitions.first() {
            Some(Definition::Mesh(m)) => format!("mesh: {} vertices, {} polygons, {} edges", m.vertices.len(), m.polygons.len(), m.edges.len()),
            Some(Definition::Patch(p)) => format!("patch: {} x {} points", p.nu, p.nv),
            Some(Definition::Face(x)) => format!("face: {} holes or curves", x.count),
            Some(Definition::Spline(s)) => format!("spline: {} keys", s.keys.len()),
            Some(Definition::NurbsCurve(c)) => format!("nurbs curve: {} points", c.points.len()),
            Some(Definition::NurbsSurface(s)) => format!("nurbs surface: {} x {} points", s.count[0], s.count[1]),
            Some(Definition::MetaSystem(_)) => "metasystem".to_string(),
            Some(Definition::Metaball(_)) => "metaball".to_string(),
            None => match n.kind {
                0 => "null".to_string(),
                5 => "joint".to_string(),
                k => format!("type {k}"),
            },
        };
        if obj.is_none() {
            println!("{}{} [{}]  materials: {}", "  ".repeat(depth), text(&n.name), what, n.materials.len());
        } else if let Some(Definition::Mesh(m)) = n.definitions.first() {
            out += &format!("o {}\n", text(&n.name));
            for v in &m.vertices {
                out += &format!("v {} {} {}\n", v.pos[0], v.pos[1], v.pos[2]);
            }
            for p in &m.polygons {
                let ids: Vec<String> = p.nodes.iter().map(|d| (base as i64 + d.vertex as i64).to_string()).collect();
                out += &format!("f {}\n", ids.join(" "));
            }
            base += m.vertices.len();
        }
    });
    if let Some(path) = obj {
        std::fs::write(path, out).unwrap();
    }
    0
}
