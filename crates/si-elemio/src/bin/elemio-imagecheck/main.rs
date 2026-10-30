// SPDX-License-Identifier: MIT
//! `elemio-imagecheck <dir> [--one-image] [--list]`: the corpus check of the image form.
//!
//! Every camera (`.cam`), light (`.lig`), lattice (`.lat`), animation (`.ani`) and struct
//! key (`.key`) file under a directory is read **into a memory image** by the element
//! dictionary's reader of the image form (`si_elemio::image`), under `Mem::program()` (the
//! heap that uses freed room again), and what the reader built there is held field for
//! field against what the value form of this crate reads from the same bytes.
//!
//! Counted per kind: files; files the reader returned from; faults of the image; files
//! whose records agree; and every disagreement with where it lies. After each file the
//! allocator's tables are verified (`Mem::heap_verify`).
//!
//! By default each file gets a fresh image, as the value form gets fresh statics.
//! `--one-image` reads all files into one image one after the other, as a session of the
//! program would, and reports the heap at the end.
//!
//! `IMAGECHECK_MUTATE=1` flips one bit of the record each reader built before it is read
//! back (a camera's near distance, a light's colour, a lattice's first count, the father
//! of an animation's first element): every file that built such a record must then differ.
//!
//! **Everything in this file is `// adapter: not in place`**: nothing here reproduces a
//! function of the original. `World` answers the routines the readers call outside the
//! crate with stand-ins of the simplest kind (said at each), and `back.rs` reads the
//! image's records back into the value form's types for the comparison.

mod back;

use si_elemio::image::host::Files;
use si_elemio::image::outside::{self as out, Outside, Routine};
use si_elemio::image::{anim, camfct, elemdict, globals, NoReturn};
use si_elemio::io::{Reader, Statics};
use si_mem::Mem;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The time a new sun is given (no sample file is a sun).
const NOW: u32 = 0;

/// The world of a corpus run. // adapter: not in place
pub struct World {
    pub files: Files,
    /// The lattices `SI_modelAddILattice` was handed, in order: the model's list is not on
    /// the image (the routine is `si-model`'s to port).
    pub lattices: Vec<u32>,
    /// The chapter name `SI_elemdictFindListByName` was asked for.
    pub chapter: Option<Vec<u8>>,
    /// The action list that holds the selection (the program's variable at 0x0878fb10).
    selected: u32,
    /// Calls of routines answered with 0 and nothing done.
    pub unanswered: BTreeMap<&'static str, u32>,
}

impl World {
    fn new() -> World {
        World { files: Files::new(), lattices: Vec::new(), chapter: None, selected: 0, unanswered: BTreeMap::new() }
    }
}

impl Outside for World {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        if let Some(v) = self.files.call(m, routine, args) {
            return v;
        }
        let a = |k: usize| args.get(k).copied().unwrap_or(0);
        match routine.addr {
            x if x == out::TIME.addr => {
                if a(0) != 0 {
                    m.set_u32(a(0), NOW);
                }
                NOW
            }
            // a custom shader record: 60 cleared bytes (the size the value form documents);
            // the real routine also files the record in the program's list of shaders
            x if x == out::SI_UDF_ALLOCATE.addr => m.si_calloc(1, 0x3c),
            x if x == out::SI_UDF_DISPOSE.addr => {
                if a(0) != 0 {
                    let name = m.u32(a(0));
                    if name != 0 {
                        m.free(name);
                    }
                    m.free(a(0));
                }
                0
            }
            x if x == out::SI_MODEL_ADD_I_LATTICE.addr => {
                self.lattices.push(a(3));
                0
            }
            // the chapter's index: the MODELS chapter's, whatever the name
            x if x == out::SI_ELEMDICT_FIND_LIST_BY_NAME.addr => {
                self.chapter = Some(m.cstr(a(1)));
                m[globals::SI_CID_MODELS] as u32
            }
            // selection as the value form models it: a selection in another list is
            // cleared first, then the element's bit 0 is set
            x if x == out::SI_ACTIONS_ELEM_SELECT.addr => {
                let (list, elem) = (a(0), a(1));
                if self.selected != 0 && self.selected != list {
                    let other = self.selected;
                    let n = m.u32(other);
                    let elems = m.u32(other + 4);
                    for k in 0..n {
                        m.and_u8(elems + k * 0x30 + 0x18, 0xfe);
                    }
                }
                m.or_u8(elem + 0x18, 1);
                self.selected = list;
                0
            }
            _ => {
                *self.unanswered.entry(routine.name).or_default() += 1;
                0
            }
        }
    }

    fn file_left(&mut self, _m: &mut Mem, file: u32) -> u32 {
        self.files.left(file)
    }
}

fn files(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            files(&e.path(), found);
        } else if t.is_file() {
            found.push(e.path());
        }
    }
}

#[derive(Default)]
struct Tally {
    files: u32,
    returned: u32,
    no_return: u32,
    faults: u32,
    agree: u32,
    differ: u32,
    notes: Vec<String>,
    causes: BTreeMap<String, u32>,
}

/// A fresh image as a session of the program has it before a file is read: this crate's
/// initial data, the curve interpolation the program starts with (2) and the frame rate
/// the value form's check assumes (30).
fn fresh() -> Mem {
    let mut m = Mem::program();
    globals::init(&mut m);
    m[globals::SI_GLB_ANIM_30] = 2;
    m[globals::SI_FRAME_RATE] = 30.0f32.to_bits();
    m
}

fn text(m: &mut Mem, s: &[u8]) -> u32 {
    let p = m.malloc(s.len() as u32 + 1);
    m.put_cstr(p, s);
    p
}

/// The mutation switch: one bit of the byte at `a`.
fn flip(m: &mut Mem, a: u32) {
    if std::env::var("IMAGECHECK_MUTATE").is_ok_and(|v| !v.is_empty() && v != "0") {
        let b = m.u8(a);
        m.set_u8(a, b ^ 1);
    }
}

/// Where two descriptions first differ, with what stands there.
fn first_difference(a: &str, b: &str) -> String {
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let from = a[..at].rfind(|c: char| c == ',' || c == '{').map_or(0, |k| k + 1);
    let cut = |s: &str| s[from.min(s.len())..].chars().take(110).collect::<String>();
    format!("value form `{}` / image `{}`", cut(a).trim(), cut(b).trim())
}

/// The field a difference lies in: the last `name:` before it.
fn field_of(a: &str, b: &str) -> String {
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let head = &a[..at.min(a.len())];
    let end = head.rfind(": ").unwrap_or(0);
    let start = head[..end].rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |k| k + 1);
    head[start..end].to_string()
}

fn one(kind: &str, data: &[u8], m: &mut Mem, t: &mut Tally, name: &str, list: bool) {
    t.files += 1;
    let mut w = World::new();
    let path_text = format!("/corpus/{}", t.files);
    w.files.add_named(path_text.as_bytes(), data.to_vec());
    let path = text(m, path_text.as_bytes());
    let faults_before = m.fault_count();
    let statics = Statics { frame_rate: 30.0, ..Statics::default() };
    // (value form, image form) as descriptions of the records
    let outcome: Result<(String, String), NoReturn> = match kind {
        "cam" => {
            let mut value = si_elemio::camera::Camera::perspective_default();
            si_elemio::camera::read_file(&mut Reader::with_statics(data, statics), &mut value);
            let cam_name = text(m, b"Perspective def");
            let cam = camfct::new_camera(m, &mut w, cam_name, 1);
            let proc_data = m[globals::SI_PROC_DATA];
            m.set_u32(proc_data + 8, cam);
            elemdict::elemdict_read_camera(m, &mut w, path).map(|got| {
                flip(m, got + 0x34);
                (format!("{value:?}"), format!("{:?}", back::camera_at(m, got)))
            })
        }
        "lig" => {
            let value = si_elemio::light::read_file(&mut Reader::with_statics(data, statics), NOW as i32);
            elemdict::elemdict_read_light(m, &mut w, path).map(|got| {
                if got != 0 {
                    flip(m, got + 0x24);
                }
                (format!("{:?}", value.read.light), format!("{:?}", if got == 0 { None } else { Some(back::light_at(m, got)) }))
            })
        }
        "lat" => {
            let value = si_elemio::lattice::read_file(&mut Reader::with_statics(data, statics), true);
            // any address that is not null stands for the model
            elemdict::elemdict_read_lattice(m, &mut w, path, 0, 0x6100_0000).map(|_| {
                if let Some(&first) = w.lattices.first() {
                    flip(m, first + 2);
                }
                let got: Vec<_> = w.lattices.iter().map(|&l| back::lattice_at(m, l)).collect();
                (format!("{:?}", value.read.lattices), format!("{got:?}"))
            })
        }
        _ => {
            let value = si_elemio::anim::read_file(&mut Reader::with_statics(data, statics), 2);
            anim::elemdict_read_animations(m, &mut w, 0, path).map(|got| {
                if m.u32(got) != 0 {
                    let first = m.u32(got + 4);
                    flip(m, first + 4);
                }
                (format!("{:?}", value.anim), format!("{:?}", back::anim_at(m, got, w.chapter.clone())))
            })
        }
    };
    if m.fault_count() != faults_before {
        t.faults += 1;
        t.notes.push(format!("{name}: fault of the image {:?}", m.fault()));
    }
    if let Err(e) = m.heap_verify() {
        t.notes.push(format!("{name}: the heap does not verify: {e}"));
    }
    if !w.unanswered.is_empty() && list {
        t.notes.push(format!("{name}: calls answered with 0: {:?}", w.unanswered));
    }
    match outcome {
        Err(e) => {
            t.no_return += 1;
            t.notes.push(format!("{name}: the reader does not return: {e:?}"));
        }
        Ok((value, image)) => {
            t.returned += 1;
            if value == image {
                t.agree += 1;
            } else {
                t.differ += 1;
                *t.causes.entry(field_of(&value, &image)).or_default() += 1;
                if list || t.differ <= 3 {
                    t.notes.push(format!("{name}: {}", first_difference(&value, &image)));
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.iter().find(|a| !a.starts_with("--")).expect("usage: elemio-imagecheck <dir> [--one-image] [--list]");
    let one_image = args.iter().any(|a| a == "--one-image");
    let list = args.iter().any(|a| a == "--list");
    let mut found = Vec::new();
    files(Path::new(dir), &mut found);
    let kinds = ["cam", "lig", "lat", "ani", "key"];
    let mut tallies: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut shared = fresh();
    for path in &found {
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(kind) = kinds.iter().find(|k| **k == ext) else { continue };
        let Ok(data) = std::fs::read(path) else { continue };
        let t = tallies.entry(kind).or_default();
        let name = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        if one_image {
            one(kind, &data, &mut shared, t, &name, list);
        } else {
            let mut m = fresh();
            one(kind, &data, &mut m, t, &name, list);
        }
    }
    println!("{:<6} {:>6} {:>9} {:>10} {:>7} {:>7} {:>7}", "kind", "files", "returned", "no-return", "faults", "agree", "differ");
    let mut total = Tally::default();
    for kind in kinds {
        let Some(t) = tallies.get(kind) else { continue };
        println!("{:<6} {:>6} {:>9} {:>10} {:>7} {:>7} {:>7}", kind, t.files, t.returned, t.no_return, t.faults, t.agree, t.differ);
        for (cause, n) in &t.causes {
            println!("    differ in `{cause}`: {n}");
        }
        for note in &t.notes {
            println!("    {note}");
        }
        total.files += t.files;
        total.returned += t.returned;
        total.no_return += t.no_return;
        total.faults += t.faults;
        total.agree += t.agree;
        total.differ += t.differ;
    }
    println!("{:<6} {:>6} {:>9} {:>10} {:>7} {:>7} {:>7}", "all", total.files, total.returned, total.no_return, total.faults, total.agree, total.differ);
    if one_image {
        let s = shared.heap_stats();
        println!("one image for all files: heap {} bytes at most, {} live blocks of {} bytes at the end, {} allocations, {} frees, {} reallocs in place, {} moved", s.peak, s.live_blocks, s.live_bytes, s.mallocs, s.frees, s.reallocs_in_place, s.reallocs_moved);
    }
    std::process::exit(if total.differ == 0 && total.faults == 0 && total.no_return == 0 { 0 } else { 1 });
}
