// SPDX-License-Identifier: MIT
//! The sixth pass's ports against the original's standalone programs (finding 0014, sixth
//! pass; outputs: finding 0031, second and third passes).
//!
//!   (ulimit -v 2000000; imgfmt-oracle6 re/out/oracle/tools)
//!
//! - `lin2pict <in> <out> <width> [-m]` calls `SI_linWritePict(in, out, width, -m ? 0x80 :
//!   0)` from its `main` (read from its instructions); the same-code scan finds the routine
//!   and the line-file reader the same as `soft`'s. The port's file is compared byte for
//!   byte.
//!
//! Prints one block a program and variant.

use std::fs;
use std::path::Path;

use si_imgfmt::{artisan, pict, wavefront};
use si_picture::Picture;

struct Row {
    id: String,
    input: String,
    args: Vec<String>,
    status: String,
    outputs: Vec<String>,
}

fn manifest(dir: &Path) -> Vec<Row> {
    let Ok(text) = fs::read_to_string(dir.join("manifest.tsv")) else { return Vec::new() };
    text.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let outputs = f.get(5).map_or(Vec::new(), |o| {
                o.split(',').filter(|s| !s.is_empty()).map(|s| s.rsplit_once(':').map_or(s, |x| x.0).to_string()).collect()
            });
            Row {
                id: f[0].to_string(),
                input: f[1].to_string(),
                args: f.get(2).map_or(Vec::new(), |a| a.split(' ').map(str::to_string).collect()),
                status: f.get(3).unwrap_or(&"").to_string(),
                outputs,
            }
        })
        .collect()
}

#[derive(Default)]
struct Count {
    identical: usize,
    differing: usize,
    both_refuse: usize,
    other: usize,
    notes: Vec<String>,
}

impl Count {
    fn print(&self, title: &str) {
        println!("{title}");
        println!("  identical {}  differing {}  both refuse {}  other {}", self.identical, self.differing, self.both_refuse, self.other);
        for n in self.notes.iter().take(12) {
            println!("    {n}");
        }
    }

    fn compare(&mut self, id: &str, ours: &[u8], theirs: &[u8]) {
        if ours == theirs {
            self.identical += 1;
        } else {
            self.differing += 1;
            let first = ours.iter().zip(theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
            self.notes.push(format!("{id}: differs from byte {first} (sizes {} and {})", ours.len(), theirs.len()));
        }
    }
}

fn lin2pict(root: &Path) {
    for variant in ["w500", "w500-m"] {
        let dir = root.join("lin2pict").join(variant);
        let mut c = Count::default();
        for row in manifest(&dir) {
            let input = fs::read(root.join(&row.input)).unwrap_or_default();
            let out_name = row.args.get(1).cloned().unwrap_or_default();
            let width: i32 = row.args.get(2).and_then(|w| w.parse().ok()).unwrap_or(0);
            let mac = if row.args.iter().any(|a| a == "-m") { 0x80 } else { 0 };
            let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
            match (pict::lin_write_pict(&input, out_name.as_bytes(), width, mac), theirs) {
                (Ok(Ok(ours)), Some(theirs)) => c.compare(&row.id, &ours, &theirs),
                (Ok(Err(_)), None) => c.both_refuse += 1,
                (r, t) => {
                    c.other += 1;
                    c.notes.push(format!("{}: port {:?}, lin2pict {} (status {})", row.id, r.map(|x| x.map(|v| v.len())), if t.is_some() { "wrote" } else { "wrote nothing" }, row.status));
                }
            }
        }
        c.print(&format!("lin2pict {variant} (SI_linWritePict)"));
    }
}

/// The comment field of a picture file, up to its first zero byte.
fn comment_of(pic_file: &[u8]) -> Vec<u8> {
    pic_file.get(8..88).map_or(Vec::new(), |c| c.iter().take_while(|&&b| b != 0).copied().collect())
}

/// Compares a picture file the port wrote (some bytes undefined) with the original's: equal
/// on every defined byte, and the count of undefined ones.
fn compare_masked(c: &mut Count, id: &str, ours: &[Option<u8>], theirs: &[u8], masked: &mut usize) {
    if ours.len() == theirs.len() && ours.iter().zip(theirs).all(|(a, b)| a.map_or(true, |a| a == *b)) {
        if ours.iter().any(|b| b.is_none()) {
            c.other += 1;
            *masked += 1;
        } else {
            c.identical += 1;
        }
    } else {
        c.differing += 1;
        let first = ours.iter().zip(theirs).position(|(a, b)| a.is_some_and(|a| a != *b)).unwrap_or(ours.len().min(theirs.len()));
        c.notes.push(format!("{id}: differs from byte {first} (sizes {} and {})", ours.len(), theirs.len()));
    }
}

fn artisan_pair(root: &Path) {
    let dir = root.join("soft2art").join("default");
    let mut c = Count::default();
    for row in manifest(&dir) {
        let input = fs::read(root.join(&row.input)).unwrap_or_default();
        let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
        match (artisan::export_artisan(&input), theirs) {
            (Ok(Ok(ours)), Some(theirs)) => c.compare(&row.id, &ours, &theirs),
            (Ok(Err(_)), None) => c.both_refuse += 1,
            (r, t) => {
                c.other += 1;
                c.notes.push(format!("{}: port {:?}, soft2art {} (status {})", row.id, r.map(|x| x.map(|v| v.len())), if t.is_some() { "wrote" } else { "wrote nothing" }, row.status));
            }
        }
    }
    c.print("soft2art (SI_pictureExportArtisan)");
    let dir = root.join("art2soft").join("default");
    let mut c = Count::default();
    let mut masked = 0;
    for row in manifest(&dir) {
        let input = fs::read(root.join(&row.input)).unwrap_or_default();
        let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
        let Some(theirs) = theirs else {
            c.other += 1;
            c.notes.push(format!("{}: art2soft wrote nothing (status {})", row.id, row.status));
            continue;
        };
        match artisan::import_artisan(&input, &comment_of(&theirs)) {
            Ok(artisan::Imported { file: Some(f), result: Ok(()) }) => compare_masked(&mut c, &row.id, &f, &theirs, &mut masked),
            other => {
                c.other += 1;
                c.notes.push(format!("{}: port {:?}", row.id, other.map(|i| i.result)));
            }
        }
        // the in-memory reader on the same file gives the planes the import wrote
        if let Ok((true, p)) = artisan::read_artisan(&input) {
            let q = Picture::read(&theirs).map(|f| f.picture).ok();
            if q.as_ref().map(|q| q.planes != p.planes || q.width != p.width || q.height != p.height).unwrap_or(true) {
                c.notes.push(format!("{}: pictureReadArtisan's planes differ from art2soft's picture", row.id));
            }
        }
    }
    c.print(&format!("art2soft (SI_pictureImportArtisan; \"other\" = identical outside the header's padding word: {masked})"));
}

/// The clock's and the environment's answers as the converter's file holds them: the login
/// name at 0x1f0 and the date at 0x210 ("October 14, 2026").
fn wave_env(file: &[u8]) -> Option<wavefront::Environment> {
    let text = |at: usize, n: usize| -> Vec<u8> { file.get(at..at + n).unwrap_or(&[]).iter().take_while(|&&b| b != 0).copied().collect() };
    let login = text(0x1f0, 32);
    let date = String::from_utf8(text(0x210, 20)).ok()?;
    let (m, rest) = date.split_once(' ')?;
    let (d, y) = rest.split_once(", ")?;
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    Some(wavefront::Environment {
        month: MONTHS.iter().position(|x| *x == m)?,
        day: d.parse().ok()?,
        year: y.parse().ok()?,
        login: if login == b"(null)" { None } else { Some(login) },
    })
}

fn wavefront_pair(root: &Path) {
    for variant in ["default", "t2", "w"] {
        let dir = root.join("soft2wave").join(variant);
        let rep = root.join("soft2wave").join(format!("{variant}-repeat"));
        let mut c = Count::default();
        let (mut undefined_total, mut changed_defined, mut heap_decided, mut other_run) = (0usize, 0usize, 0usize, 0usize);
        for row in manifest(&dir) {
            let input = fs::read(root.join(&row.input)).unwrap_or_default();
            let name = row.args.get(1).cloned().unwrap_or_default();
            let kind: i32 = row.args.iter().position(|a| a == "-t").and_then(|i| row.args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
            let active = row.args.iter().any(|a| a == "-w");
            let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
            let pic = Picture::read(&input).map(|f| f.picture);
            match (pic, theirs) {
                (Err(_), None) => c.both_refuse += 1,
                (Ok(p), Some(theirs)) => {
                    let Some(env) = wave_env(&theirs) else {
                        c.other += 1;
                        c.notes.push(format!("{}: no date or login in the converter's file", row.id));
                        continue;
                    };
                    // where the coder compares the heap byte after a plane, try that byte equal
                    // to the plane's last one and not equal, plane by plane
                    let mut made = wavefront::write_wavefront(&p, name.as_bytes(), active, kind, &env);
                    if made.is_err() {
                        let last = |c: usize| p.planes[c].as_ref().and_then(|v| v.last().copied()).unwrap_or(0);
                        for combo in 0..16u32 {
                            let past = [0, 1, 2, 3].map(|c| Some(if combo >> c & 1 == 0 { last(c) } else { !last(c) }));
                            if let Ok(Some(o)) = wavefront::write_wavefront_past(&p, name.as_bytes(), active, kind, &env, past) {
                                if o.len() == theirs.len() && o.iter().zip(&theirs).all(|(a, b)| a.map_or(true, |a| a == *b)) {
                                    c.notes.push(format!("{}: identical given the heap bytes after the planes {:?}", row.id, [0, 1, 2, 3].map(|c| if past[c] == Some(last(c)) { "equal to the last pixel" } else { "another value" })));
                                    heap_decided += 1;
                                    made = Ok(Some(o));
                                    break;
                                }
                            }
                        }
                    }
                    match made {
                        Ok(Some(ours)) => {
                            let same = ours.len() == theirs.len() && ours.iter().zip(&theirs).all(|(a, b)| a.map_or(true, |a| a == *b));
                            if same {
                                let u = ours.iter().filter(|b| b.is_none()).count();
                                undefined_total += u;
                                c.other += 1;
                                // the original's second run: every byte that changed is one the port calls undefined
                                let rid = row.id.replace(&format!(".{variant}."), &format!(".{variant}-repeat."));
                                let ext = if kind == 2 { "rlb" } else { "rla" };
                                if let Ok(again) = fs::read(rep.join(format!("{rid}.{ext}"))) {
                                    if again.len() == theirs.len() {
                                        changed_defined += again.iter().zip(&theirs).zip(&ours).enumerate().filter(|(i, ((a, b), o))| a != b && o.is_some() && !(0x90..0x110).contains(i)).count();
                                    }
                                }
                            } else {
                                // the original's other run of the same command
                                let rid = row.id.replace(&format!(".{variant}."), &format!(".{variant}-repeat."));
                                let ext = if kind == 2 { "rlb" } else { "rla" };
                                let again = fs::read(rep.join(format!("{rid}.{ext}"))).unwrap_or_default();
                                let first = ours.iter().zip(&theirs).position(|(a, b)| a.is_some_and(|a| a != *b)).unwrap_or(ours.len().min(theirs.len()));
                                if again.len() == ours.len() && ours.iter().zip(&again).enumerate().all(|(i, (a, b))| (0x90..0x110).contains(&i) || a.map_or(true, |a| a == *b)) {
                                    c.other += 1;
                                    other_run += 1;
                                    c.notes.push(format!("{}: this run's file differs from byte {first} (sizes {} and {}, status {}); the original's other run gives the port's file", row.id, ours.len(), theirs.len(), row.status));
                                } else {
                                    c.differing += 1;
                                    c.notes.push(format!("{}: differs from byte {first} (sizes {} and {})", row.id, ours.len(), theirs.len()));
                                }
                            }
                        }
                        Ok(None) => {
                            c.other += 1;
                            c.notes.push(format!("{}: the port writes nothing", row.id));
                        }
                        Err(f) => {
                            c.other += 1;
                            c.notes.push(format!("{}: the port stops: {}", row.id, f.0));
                        }
                    }
                }
                (p, t) => {
                    c.other += 1;
                    c.notes.push(format!("{}: port {}, soft2wave {} (status {})", row.id, if p.is_ok() { "reads" } else { "refuses" }, if t.is_some() { "wrote" } else { "wrote nothing" }, row.status));
                }
            }
        }
        c.print(&format!("soft2wave {variant} (SI_pictureWriteWavefront; \"other\" = identical outside the header's stack bytes, {undefined_total} bytes in all; defined bytes outside the name that the original's second run changed: {changed_defined}; decided by a heap byte: {heap_decided}; only the original's other run identical: {other_run})"));
    }
    for variant in ["default", "t2", "w"] {
        let dir = root.join("wave2soft").join(variant);
        let mut c = Count::default();
        let mut masked = 0;
        for row in manifest(&dir) {
            let input = fs::read(root.join(&row.input)).unwrap_or_default();
            let kind: i32 = row.args.iter().position(|a| a == "-t").and_then(|i| row.args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
            let active = row.args.iter().any(|a| a == "-w");
            let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
            match (wavefront::read_wavefront(&input, active, kind), theirs) {
                (Ok(p), Some(theirs)) => {
                    let mut ours: Vec<Option<u8>> = p.write(Some(&comment_of(&theirs)), false).into_iter().map(Some).collect();
                    if ours.len() > 104 {
                        ours[102] = None;
                        ours[103] = None;
                    }
                    compare_masked(&mut c, &row.id, &ours, &theirs, &mut masked);
                }
                (Err(f), None) => {
                    c.both_refuse += 1;
                    c.notes.push(format!("{}: both stop (port: {}; wave2soft status {})", row.id, f.0, row.status));
                }
                (r, t) => {
                    c.other += 1;
                    c.notes.push(format!("{}: port {:?}, wave2soft {} (status {})", row.id, r.map(|p| (p.width, p.height)), if t.is_some() { "wrote" } else { "wrote nothing" }, row.status));
                }
            }
        }
        c.print(&format!("wave2soft {variant} (SI_pictureReadWavefront; \"other\" = identical outside the padding word: {masked})"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(args.get(1).map(String::as_str).unwrap_or("re/out/oracle/tools"));
    lin2pict(root);
    artisan_pair(root);
    wavefront_pair(root);
}
