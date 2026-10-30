// SPDX-License-Identifier: MIT
//! Corpus check for the element files (finding 0005): runs the readers and writers of
//! `si-elemio` over every `.cam`, `.lig`, `.lat`, `.ani`, `.key`, `.cls` and `.shp` file under
//! a directory and counts outcomes.
//!
//!   elem-check <dir> [--list] [--tokens] [--only <ext>]
//!
//! Per type it reports
//! - how many files the reader takes to their exact end: the zero token read and the next
//!   byte being the end of the file or the start of an appended icon;
//! - the round trip: read, write as the program writes for version 4.0, read that again,
//!   compare the two records;
//! - how many files come out byte for byte when written again, with the header's version and
//!   comment taken from the file, and how many when written as version 4.0 (the bytes after
//!   the header compared).
//!
//! `--list` names every file that is an exception; `--tokens` prints which tokens occur.
use si_elemio::io::{End, Reader, Writer};
use si_elemio::{anim, camera, lattice, light};
use std::collections::{BTreeMap, BTreeSet};
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

/// A cheap content hash (FNV-1a, 64 bits) to count distinct files: the same file ships under
/// several platforms.
fn hash(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ (data.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// What one file did.
#[derive(Default)]
struct Outcome {
    version: f32,
    /// The reader came to its end at the zero token.
    zero: bool,
    /// Where it stopped, and where the data ends (end of file or start of the icon).
    pos: usize,
    limit: usize,
    clamped: bool,
    /// Read, write for 4.0, read again: same record, and the second read ends at the end.
    round_trip: bool,
    /// Written with the file's own version and comment: the same bytes as the file.
    same_own: bool,
    /// Written for 4.0: the same bytes after the header.
    same_v4: bool,
    /// Why the bytes differ, if they do.
    why: Option<String>,
    /// Any further remark for the list.
    note: Option<String>,
}

type Census = BTreeMap<(&'static str, u16), usize>;

fn first_diff(a: &[u8], b: &[u8]) -> String {
    let n = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    format!("first difference at offset {n} (file {} bytes, written {} bytes)", a.len(), b.len())
}

fn limit_of(data: &[u8]) -> usize {
    match si_picture::icon_offset(data) {
        Some(off) if off <= data.len() => off,
        _ => data.len(),
    }
}

fn check_camera(data: &[u8], census: &mut Census) -> Outcome {
    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let mut r = Reader::new(data);
    r.census = Some(std::mem::take(census));
    let mut cam = camera::Camera::perspective_default();
    let f = camera::read_file(&mut r, &mut cam);
    *census = r.census.take().unwrap();
    o.version = f.version;
    o.zero = f.end == End::Zero;
    o.pos = f.pos;
    o.clamped = f.clamped;
    // the converter's reader gives the same camera from a file without a model reference
    let (conv, cf) = camera::read_file_converter(&mut Reader::new(data));
    if conv != cam || cf.pos != f.pos || cf.end != f.end {
        o.note = Some("the converter's reader differs".to_string());
    }
    // round trip at 4.0
    let mut w = Writer::new();
    camera::write_file(&mut w, &cam);
    let mut again = camera::Camera::perspective_default();
    let mut r2 = Reader::new(&w.out);
    let f2 = camera::read_file(&mut r2, &mut again);
    o.round_trip = again == cam && f2.end == End::Zero && f2.pos == w.out.len();
    o.same_v4 = data.len() >= 88 && w.out[88..] == data[88..o.limit.max(88)];
    // with the file's own version and comment
    let mut w = Writer::with_version(f.version);
    si_file::write_bin_header(&mut w.out, si_file::MAGIC_CAMERA, f.version, Some(&f.comment));
    camera::write_tokens(&mut w, &cam);
    o.same_own = w.out[..] == data[..o.limit];
    if !o.same_own {
        // files from before the projection plane existed lack its token: 34 bytes
        let mut without = w.out.clone();
        let cut = without.windows(2).rposition(|x| x == [0x16, 0x14]);
        if let Some(at) = cut {
            without.drain(at..at + 34);
        }
        o.why = Some(if without[..] == data[..o.limit] {
            "the file has no projection plane token (0x1614); otherwise identical".to_string()
        } else {
            first_diff(&data[..o.limit], &w.out)
        });
    }
    o
}

/// The time stamp given to a new sun record in the checks (the original takes the clock).
const NOW: i32 = 0;

fn check_light(data: &[u8], census: &mut Census) -> Outcome {
    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let mut r = Reader::new(data);
    r.census = Some(std::mem::take(census));
    let f = light::read_file(&mut r, NOW);
    *census = r.census.take().unwrap();
    o.version = f.version;
    o.zero = f.read.end == End::Zero;
    o.pos = f.pos;
    o.clamped = f.clamped;
    let Some(l) = f.read.light.as_ref() else {
        o.note = Some("no light in the file".to_string());
        return o;
    };
    if f.read.fault {
        o.note = Some("a field the original would fault on".to_string());
    }
    let conv = light::read_file_converter(&mut Reader::new(data), NOW);
    if conv.read.light != f.read.light || conv.pos != f.pos || conv.read.end != f.read.end {
        o.note = Some("the converter's reader differs".to_string());
    }
    let mut w = Writer::new();
    light::write_file(&mut w, l);
    let f2 = light::read_file(&mut Reader::new(&w.out), NOW);
    o.round_trip = f2.read.light.as_ref() == Some(l) && f2.read.end == End::Zero && f2.pos == w.out.len();
    o.same_v4 = data.len() >= 88 && w.out[88..] == data[88..o.limit.max(88)];
    let mut w = Writer::with_version(f.version);
    si_file::write_bin_header(&mut w.out, si_file::MAGIC_LIGHT, f.version, Some(&f.comment));
    light::write_tokens(&mut w, l);
    o.same_own = w.out[..] == data[..o.limit];
    if !o.same_own {
        let file = &data[..o.limit];
        // the two known causes: the file predates the DIFFUSE token (6 bytes before the zero
        // token), or it predates the whole area block although its header says 2.65
        let mut no_diffuse = w.out.clone();
        if let Some(at) = no_diffuse.len().checked_sub(8).filter(|&at| no_diffuse[at..at + 2] == [0, 0x28]) {
            no_diffuse.drain(at..at + 6);
        }
        let mut old = Writer::with_version(2.64);
        si_file::write_bin_header(&mut old.out, si_file::MAGIC_LIGHT, f.version, Some(&f.comment));
        light::write_tokens(&mut old, l);
        o.why = Some(if no_diffuse[..] == *file {
            "the file has no DIFFUSE token (0x28); otherwise identical".to_string()
        } else if old.out[..] == *file {
            "the file has no shader and area tokens (0x1d to 0x28) although its version is 2.65; otherwise identical".to_string()
        } else {
            first_diff(file, &w.out)
        });
    }
    o
}

/// The frame rate the checks assume for the action lists: the program's default
/// (`SI_initPlayControl` sets 30).
const FRAME_RATE: f32 = 30.0;
/// The default interpolation the checks assume for a curve whose file does not state one.
const DEFAULT_INTERPOLATION: i16 = 2;

fn check_anim(data: &[u8], census: &mut Census) -> Outcome {
    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let mut r = Reader::new(data);
    r.st.frame_rate = FRAME_RATE;
    r.census = Some(std::mem::take(census));
    let f = anim::read_file(&mut r, DEFAULT_INTERPOLATION);
    *census = r.census.take().unwrap();
    o.version = f.version;
    o.zero = f.hier_end == End::Zero && f.items_end == End::Zero;
    o.pos = f.pos;
    o.clamped = f.clamped;
    let is_key = f.magic == Some(si_file::MAGIC_STRUCT_KEYS);
    let mut notes = Vec::new();
    if f.dropped_items != 0 {
        notes.push(format!("{} items with an owner outside the element list", f.dropped_items));
    }
    let write = |version: f32, header: Option<(u32, f32, &[u8])>, headers: anim::ItemHeaders| -> Vec<u8> {
        let mut w = Writer::with_version(version);
        w.st.frame_rate = FRAME_RATE;
        match header {
            Some((magic, v, comment)) => si_file::write_bin_header(&mut w.out, magic, v, Some(comment)),
            None => si_file::write_db_header(&mut w.out, version, if is_key { si_file::MAGIC_STRUCT_KEYS } else { si_file::MAGIC_ANIMATION }),
        }
        if is_key {
            anim::write_key_body(&mut w, f.anim.chapter.as_deref(), &f.anim, headers);
        } else {
            anim::write_ani_body(&mut w, f.anim.chapter.as_deref(), &f.anim, headers);
        }
        w.out
    };
    // round trip with the program's writer of an existing hierarchy animation
    let out = write(4.0, None, anim::ItemHeaders::Program);
    let mut r2 = Reader::new(&out);
    r2.st.frame_rate = FRAME_RATE;
    let f2 = anim::read_file(&mut r2, DEFAULT_INTERPOLATION);
    o.round_trip = f2.anim == f.anim && f2.pos == out.len();
    if !o.round_trip {
        // the same with each item's own header fields
        let out = write(4.0, None, anim::ItemHeaders::Stored);
        let mut r2 = Reader::new(&out);
        r2.st.frame_rate = FRAME_RATE;
        let f3 = anim::read_file(&mut r2, DEFAULT_INTERPOLATION);
        notes.push(if f3.anim == f.anim && f3.pos == out.len() {
            "round trip: the program's writer of an existing animation drops the items' action numbers and token 7; equal when they are written".to_string()
        } else if f3.anim.elems == f.anim.elems {
            "round trip: action times change by the half-frame shift".to_string()
        } else {
            "round trip: records differ".to_string()
        });
    }
    let out = write(4.0, None, anim::ItemHeaders::Stored);
    o.same_v4 = data.len() >= 88 && out[88..] == data[88..o.limit.max(88)];
    let magic = f.magic.unwrap_or(if is_key { si_file::MAGIC_STRUCT_KEYS } else { si_file::MAGIC_ANIMATION });
    let out = write(f.version, Some((magic, f.version, &f.comment)), anim::ItemHeaders::Stored);
    o.same_own = out[..] == data[..o.limit];
    if !o.same_own {
        let file = &data[..o.limit];
        // known causes, tried one at a time
        let no_lists = out.len() >= 8 && out[out.len() - 8..] == [0, 10, 0, 0x0b, 0, 0x1e, 0, 0x1f] && out[..out.len() - 8] == *file;
        let no_second = out.len() >= 4 && out[out.len() - 4..] == [0, 0x1e, 0, 0x1f] && out[..out.len() - 4] == *file;
        o.why = Some(if no_lists {
            "the file has no action lists (tokens 10 to 0x1f); otherwise identical".to_string()
        } else if no_second {
            "the file has no second action list (tokens 0x1e, 0x1f); otherwise identical".to_string()
        } else {
            // the action times are stored half a frame inside their spans: reading takes the
            // half frame off and writing puts it back, which need not give the same single.
            // With a frame rate of zero there is no shift.
            let mut r0 = Reader::new(data);
            let f0 = anim::read_file(&mut r0, DEFAULT_INTERPOLATION);
            let mut w0 = Writer::with_version(f.version);
            si_file::write_bin_header(&mut w0.out, magic, f.version, Some(&f.comment));
            if is_key {
                anim::write_key_body(&mut w0, f0.anim.chapter.as_deref(), &f0.anim, anim::ItemHeaders::Stored);
            } else {
                anim::write_ani_body(&mut w0, f0.anim.chapter.as_deref(), &f0.anim, anim::ItemHeaders::Stored);
            }
            if w0.out[..] == *file {
                let n = file.iter().zip(&out).filter(|(a, b)| a != b).count();
                format!("action times differ after the half-frame shift there and back ({n} bytes); identical with no shift (frame rate zero)")
                    .replace(&format!("({n} bytes)"), "(a few low bytes)")
            } else {
                first_diff(file, &out)
            }
        });
    }
    if !notes.is_empty() {
        o.note = Some(notes.join("; "));
    }
    o
}

fn check_lattice(data: &[u8], census: &mut Census) -> Outcome {
    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let mut r = Reader::new(data);
    r.census = Some(std::mem::take(census));
    let f = lattice::read_file(&mut r, true);
    *census = r.census.take().unwrap();
    o.version = f.version;
    o.zero = f.read.end == End::Zero;
    o.pos = f.pos;
    o.clamped = f.clamped;
    o.note = Some(format!("{} lattices", f.read.lattices.len())).filter(|_| f.read.lattices.len() != 1);
    {
        // the converter's token reader on the same stream (its file entry point expects a
        // tag the database files do not have)
        let mut rc = Reader::new(data);
        si_elemio::io::read_header(&mut rc);
        let conv = lattice::read_tokens(&mut rc, lattice::Variant::Converter, false);
        if conv.lattices != f.read.lattices || rc.s.tell() != f.pos {
            o.note = Some("the converter's reader differs".to_string());
        }
    }
    let mut w = Writer::new();
    if lattice::write_file(&mut w, &f.read.lattices).is_err() {
        o.note = Some("no lattice in the file: the writer refuses an empty list".to_string());
        return o;
    }
    let f2 = lattice::read_file(&mut Reader::new(&w.out), true);
    o.round_trip = f2.read.lattices == f.read.lattices && f2.read.end == End::Zero && f2.pos == w.out.len();
    o.same_v4 = data.len() >= 88 && w.out[88..] == data[88..o.limit.max(88)];
    let mut w = Writer::with_version(f.version);
    si_file::write_bin_header(&mut w.out, si_file::MAGIC_LATTICE, f.version, Some(&f.comment));
    let _ = lattice::write_tokens(&mut w, &f.read.lattices);
    o.same_own = w.out[..] == data[..o.limit];
    if !o.same_own {
        o.why = Some(first_diff(&data[..o.limit], &w.out));
    }
    o
}

/// Shape files. The kind of a shape file's records is its model's, and a check has no model:
/// it takes the model to be of the kind the file's tag names, with one definition of its own
/// (a new, empty one) as a model has from its model file.
///
/// No file in the corpus was written by this build, and a build writes the same prototype
/// block into every file, so the bytes written with a file's own version differ from the file
/// at least in that block. The reason given says which lists differ (`+` a field this build
/// adds, `-` one it lacks) and whether the records after the block are the file's bytes. For
/// records that are not, the check writes them once more by the file's own lists (a writer the
/// original does not have: its record writers follow one fixed order), which tells a
/// difference in what is stored from a difference in the data.
fn check_cluster(data: &[u8], census: &mut Census) -> Outcome {
    use si_elemio::cluster::{self, Kluster};
    use si_elemio::geom::GeomState;
    use si_elemio::proto::{self, Prototypes};

    /// Where the prototype block of a cluster file ends (the offset of its token stream).
    fn stream_start(bytes: &[u8]) -> usize {
        let mut r = Reader::new(bytes);
        si_elemio::io::read_header(&mut r);
        let mut tag = 0u32;
        si_elemio::io::fread_u32(&mut r.s, &mut tag);
        if tag == proto::TAG {
            proto::read_prototypes(&mut r, &mut Prototypes::new());
        }
        r.s.tell()
    }

    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let mut r = Reader::new(data);
    r.census = Some(std::mem::take(census));
    let mut anim = None;
    // The positions of a cluster key are sized by the model's kind, which the file does not
    // state. No cluster file on the discs holds a key or a shape, so the kind given here
    // (4, a polygon mesh) is never used; a file that did hold one is reported, not guessed.
    let f = cluster::read_file(&mut r, 4, &mut [], &mut anim, &mut GeomState::default());
    *census = r.census.take().unwrap();
    o.version = f.version;
    o.zero = f.read.end == End::Zero && f.read.ok;
    o.pos = f.pos;
    o.clamped = f.clamped;
    if f.read.current.is_some() || anim.is_some() || f.read.fault {
        o.note = Some("holds a shape or a named cluster: needs the model it belongs to".to_string());
        return o;
    }
    o.note = Some("empty: a prototype block and the zero token".to_string());
    // what the writer produces for a cluster without key data on a model without a current
    // shape: header, prototype block, zero token
    let none = Kluster { name: Vec::new(), nb_points: 0, data: None };
    let mut w = Writer::new();
    cluster::write_file(&mut w, None, 4, &none, None, 1);
    let mut anim2 = None;
    let mut r2 = Reader::new(&w.out);
    let f2 = cluster::read_file(&mut r2, 4, &mut [], &mut anim2, &mut GeomState::default());
    o.round_trip = f2.read.end == End::Zero && f2.pos == w.out.len() && f2.read.current.is_none() && anim2.is_none();
    o.same_v4 = data.len() >= 88 && w.out[88..] == data[88..o.limit.max(88)];
    let mut w = Writer::with_version(f.version);
    si_file::write_bin_header(&mut w.out, si_file::MAGIC_CLUSTER, f.version, Some(&f.comment));
    proto::write_prototypes(&mut w);
    cluster::write_tokens(&mut w, None, 4, &none, None, 1);
    o.same_own = w.out[..] == data[..o.limit];
    if !o.same_own {
        let file = &data[..o.limit];
        let (a, b) = (stream_start(file), stream_start(&w.out));
        o.why = Some(if file[..88] == w.out[..88] && file[a..] == w.out[b..] {
            "prototype lists differ from the ones this build writes for that version; header and token stream identical".to_string()
        } else {
            first_diff(file, &w.out)
        });
    }
    o
}

fn check_shape(data: &[u8], census: &mut Census) -> Outcome {
    use si_elemio::geom::{Face, Geometry, Mesh, NurbsCurve, Patch, Spline};
    use si_elemio::proto::{self, Prototypes};
    use si_elemio::shape::{self, ShapeModel};

    fn vec3(w: &mut Writer, v: &[f32]) {
        for &c in &v[..3] {
            w.put_f32(c);
        }
    }
    fn list<'a>(p: &'a Prototypes, offset: usize, what: &str) -> Result<&'a [i16], String> {
        p.by_offset(offset).ok_or(format!("the file has no prototype for {what}"))
    }
    fn spline_by(w: &mut Writer, s: &Spline, p: &Prototypes) -> Result<(), String> {
        for f in proto::fields(list(p, 0, "a spline")?) {
            match f {
                1 => w.put_u16(s.kind as u16),
                2 => {
                    w.put_u16(s.nb_keys);
                    for k in s.keys.iter().take((s.nb_keys as i16).max(0) as usize) {
                        vec3(w, &k.position);
                    }
                }
                3 => w.put_u16(s.close as u16),
                4 => w.put_f32(s.tension),
                5 => w.put_u16(s.step as u16),
                6 => w.put_u16(s.phantom as u16),
                7 => {
                    w.put_u16(s.nb_keys);
                    let kl = list(p, 4, "a spline key")?;
                    for k in s.keys.iter().take(s.nb_keys as usize) {
                        for kf in proto::fields(kl) {
                            match kf {
                                1 => vec3(w, &k.previous_tangent),
                                2 => vec3(w, &k.position),
                                3 => vec3(w, &k.next_tangent),
                                4 => w.put_u16(k.constraint as u16),
                                5 => w.put_u16(k.segment as u16),
                                6 => w.put_u16(k.flag as u16),
                                _ => return Err(format!("spline key field {kf:#x}")),
                            }
                        }
                    }
                }
                8 => w.string(Some(&s.name)),
                _ => return Err(format!("spline field {f:#x}")),
            }
        }
        Ok(())
    }
    fn curve_by(w: &mut Writer, c: &NurbsCurve, p: &Prototypes, version: f32) -> Result<(), String> {
        let f64s = |w: &mut Writer, v: &[f64]| v.iter().for_each(|d| w.out.extend_from_slice(&d.to_bits().to_be_bytes()));
        for f in proto::fields(list(p, 0x64, "a NURBS curve")?) {
            match f {
                1 => w.put_i32(c.order),
                2 => {
                    w.put_i32(c.nb_points);
                    for q in c.points.iter().take(c.nb_points.max(0) as usize) {
                        f64s(w, &q.xyzw);
                        if (version as f64) >= 2.6 {
                            w.put_i32(q.flag);
                        }
                    }
                }
                3 => w.put_u16(c.closed as u16),
                4 => {
                    w.put_i32(c.uniformity);
                    w.put_i32(c.nb_knots);
                    f64s(w, &c.knots[..c.knots.len().min(c.nb_knots.max(0) as usize)]);
                }
                5 => f64s(w, &[c.start, c.end]),
                6 => w.put_i32(c.display_steps),
                7 => w.put_i32(c.closing),
                8 | 0x208 => w.put_u16(c.key_framed as u16),
                _ => return Err(format!("NURBS curve field {f:#x}")),
            }
        }
        Ok(())
    }
    fn face_by(w: &mut Writer, x: &Face, p: &Prototypes, version: f32) -> Result<(), String> {
        for f in proto::fields(list(p, 0x10, "a face")?) {
            match f {
                1 if x.is_spline != 0 => spline_by(w, &x.exterior, p)?,
                2 if x.is_spline != 0 => {
                    w.put_u16(x.count as u16);
                    for s in x.holes.iter().take(x.count.max(0) as usize) {
                        spline_by(w, s, p)?;
                    }
                }
                3 => w.put_u16(x.is_spline as u16),
                4 if x.is_spline == 0 => {
                    w.put_u16(x.count as u16);
                    for c in x.curves.iter().take(x.count.max(0) as usize) {
                        curve_by(w, c, p, version)?;
                    }
                }
                1 | 2 | 4 => {}
                _ => return Err(format!("face field {f:#x}")),
            }
        }
        Ok(())
    }
    fn patch_by(w: &mut Writer, x: &Patch, p: &Prototypes) -> Result<(), String> {
        let n = x.point_count().max(0) as usize;
        for f in proto::fields(list(p, 0x0c, "a patch")?) {
            match f {
                1 => w.put_u16(x.type_u as u16),
                2 => w.put_u16(x.type_v as u16),
                3 => {
                    w.put_u16(x.nb_u);
                    w.put_u16(x.nb_v);
                    x.points.iter().take(n).for_each(|q| vec3(w, q));
                }
                4 => w.put_u16(x.close_u as u16),
                5 => w.put_u16(x.close_v as u16),
                6 => w.put_f32(x.tension_u),
                7 => w.put_f32(x.tension_v),
                8 => w.put_u16(x.step_u as u16),
                9 => w.put_u16(x.step_v as u16),
                10 => w.put_u16(x.curves_u as u16),
                11 => w.put_u16(x.curves_v as u16),
                12 => w.put_u16(x.u_bottom as u16),
                13 => w.put_u16(x.u_top as u16),
                14 => w.put_u16(x.v_bottom as u16),
                15 => w.put_u16(x.v_top as u16),
                16 => w.put_u16(x.faceted as u16),
                17 => w.put_u16(x.approx_type as u16),
                18 => w.put_u16(x.view_dependent as u16),
                19 => w.put_f32(x.max_edge_length),
                20 => w.put_f32(x.curvature_u),
                21 => w.put_f32(x.curvature_v),
                22 => w.put_u16(x.min_recursion as u16),
                23 => w.put_u16(x.max_recursion as u16),
                24 => w.put_u16(x.recursion as u16),
                25 => x.tags.iter().take(n).for_each(|&t| w.put_u16(t as u16)),
                _ => return Err(format!("patch field {f:#x}")),
            }
        }
        Ok(())
    }
    fn mesh_by(w: &mut Writer, m: &Mesh, p: &Prototypes, uv_domains: i32) -> Result<(), String> {
        for f in proto::fields(list(p, 0x14, "a mesh")?) {
            match f as u16 {
                2 => w.put_f32(m.angle),
                6 => w.put_u16(m.flag),
                0x413 => w.put_i32(uv_domains),
                3 | 7 => {
                    if f == 3 {
                        w.put_u16(m.nb_vertices as u16);
                    } else {
                        w.put_i32(m.nb_vertices);
                    }
                    let vl = list(p, 0x24, "a vertex")?;
                    for v in m.vertices.iter().take(m.nb_vertices.max(0) as usize) {
                        for vf in proto::fields(vl) {
                            match vf {
                                1 => vec3(w, &v.position),
                                4 => w.put_u16(v.flag as u16),
                                _ => return Err(format!("vertex field {vf:#x}")),
                            }
                        }
                    }
                }
                4 | 8 => {
                    if f == 4 {
                        w.put_u16(m.nb_polygons as u16);
                    } else {
                        w.put_i32(m.nb_polygons);
                    }
                    let pl = list(p, 0x1c, "a polygon")?;
                    for poly in m.polygons.iter().take(m.nb_polygons.max(0) as usize) {
                        for pf in proto::fields(pl) {
                            match pf {
                                1 => vec3(w, &poly.normal),
                                2 => {
                                    w.put_u16(poly.nb_nodes);
                                    let nl = list(p, 0x20, "a polygon node")?;
                                    for n in poly.nodes.iter().take(poly.nb_nodes as usize) {
                                        for nf in proto::fields(nl) {
                                            match nf as u16 {
                                                1 => w.put_u16(n.vertex as u16),
                                                2 => vec3(w, &n.normal),
                                                3 => w.put_i32(n.vertex),
                                                4 => {
                                                    w.put_f32(n.uv[0]);
                                                    w.put_f32(n.uv[1]);
                                                }
                                                0x405 => w.put_i32(n.colour as i32),
                                                0xff06 => {
                                                    w.put_i32(uv_domains.wrapping_sub(1).wrapping_mul(8));
                                                    for uv in &n.more_uv {
                                                        w.put_f32(uv[0]);
                                                        w.put_f32(uv[1]);
                                                    }
                                                }
                                                _ => return Err(format!("polygon node field {nf:#x}")),
                                            }
                                        }
                                    }
                                }
                                3 => w.put_u16(poly.material as u16),
                                4 => w.put_u16(poly.flag as u16),
                                _ => return Err(format!("polygon field {pf:#x}")),
                            }
                        }
                    }
                }
                5 | 9 => {
                    if f == 5 {
                        w.put_u16(m.nb_edges as u16);
                    } else {
                        w.put_i32(m.nb_edges);
                    }
                    let el = list(p, 0x18, "an edge")?;
                    for e in m.edges.iter().take(m.nb_edges.max(0) as usize) {
                        for ef in proto::fields(el) {
                            match ef {
                                1 => {
                                    w.put_u16(e.vertex[0] as u16);
                                    w.put_u16(e.vertex[1] as u16);
                                }
                                2 | 5 => {}
                                3 => w.put_u16(e.flag as u16),
                                4 => {
                                    w.put_i32(e.vertex[0]);
                                    w.put_i32(e.vertex[1]);
                                }
                                _ => return Err(format!("edge field {ef:#x}")),
                            }
                        }
                    }
                }
                0x20a => w.put_u16(m.w_52 as u16),
                0x20b => w.put_u16(m.w_50 as u16),
                0x20c => w.put_u16(m.w_38 as u16),
                0x40d => w.put_f32(m.f_3c),
                0x40e => w.put_f32(m.f_40),
                0x40f => w.put_f32(m.f_44),
                0x210 => w.put_u16(m.w_48 as u16),
                0x211 => w.put_u16(m.w_4a as u16),
                0x212 => w.put_u16(m.w_4c as u16),
                _ => return Err(format!("mesh field {f:#x}")),
            }
        }
        Ok(())
    }
    fn geometry_by(w: &mut Writer, g: &Geometry, p: &Prototypes, uv_domains: i32, version: f32) -> Result<(), String> {
        match g {
            Geometry::Spline(s) => spline_by(w, s, p),
            Geometry::Face(x) => face_by(w, x, p, version),
            Geometry::Patch(x) => patch_by(w, x, p),
            Geometry::Mesh(m) => mesh_by(w, m, p, uv_domains),
            Geometry::NurbsCurve(c) => curve_by(w, c, p, version),
            Geometry::NurbsSurface(_) => Err("a NURBS surface (no writer by lists in this check)".to_string()),
        }
    }
    /// What follows the prototype block, with every record written by the given lists.
    fn body_by(model: &ShapeModel, tag: u32, p: &Prototypes, version: f32) -> Result<Vec<u8>, String> {
        let mut w = Writer::with_version(version);
        w.out.extend_from_slice(&tag.to_be_bytes());
        let uv = model.state.uv_domains;
        if let Some(c) = model.current.as_ref() {
            w.put_u16(0xfffe);
            w.put_u16(model.weight_mode as u16);
            geometry_by(&mut w, c, p, uv, version)?;
        }
        w.put_u16((model.definitions.len() as u16).wrapping_sub(1));
        for g in model.definitions.iter().skip(1) {
            geometry_by(&mut w, g, p, uv, version)?;
        }
        Ok(w.out)
    }
    /// How the file's lists differ from the ones written: per id, `+` what was written and is
    /// not in the file, `-` what the file has and was not written.
    fn list_differences(file: &Prototypes, written: &[(i16, Vec<i16>)]) -> Vec<String> {
        let mut out = Vec::new();
        for id in 1..0x20i16 {
            let ours = written.iter().find(|(i, _)| *i == id).map(|(_, l)| l.as_slice());
            let theirs = file.by_id(id);
            if ours == theirs {
                continue;
            }
            let (a, b) = (ours.unwrap_or(&[]), theirs.unwrap_or(&[]));
            let mut parts: Vec<String> = a.iter().filter(|x| !b.contains(x)).map(|x| format!("+{:x}", *x as u16)).collect();
            parts.extend(b.iter().filter(|x| !a.contains(x)).map(|x| format!("-{:x}", *x as u16)));
            if parts.is_empty() {
                parts.push("order".to_string());
            }
            out.push(format!("{id:x}({})", parts.join(" ")));
        }
        out
    }

    let mut o = Outcome { limit: limit_of(data), ..Outcome::default() };
    let file = &data[..o.limit];
    // a first pass with a model of no kind, for the tag
    let probe = shape::read_file(&mut Reader::new(file), &mut ShapeModel::new(0));
    o.version = probe.version;
    let Some(model_type) = shape::model_type_of(probe.tag) else {
        o.pos = probe.pos;
        o.note = Some("the tag names no kind of geometry".to_string());
        return o;
    };
    let fresh = || {
        let mut m = ShapeModel::new(model_type);
        let own = Geometry::allocate(model_type, &mut m.state);
        m.definitions.extend(own);
        m
    };
    let mut model = fresh();
    let mut r = Reader::new(file);
    r.census = Some(std::mem::take(census));
    let f = shape::read_file(&mut r, &mut model);
    *census = r.census.take().unwrap();
    o.zero = f.end == End::Zero && f.accepted;
    o.pos = f.pos;
    o.clamped = f.clamped;
    let kind = String::from_utf8_lossy(&probe.tag.to_be_bytes()).into_owned();
    o.note = Some(format!("{kind}: {} definitions{}", model.definitions.len() - 1, if f.has_current { " and the current one" } else { "" }));
    // round trip as the program writes for 4.0
    let mut w = Writer::new();
    shape::write_file(&mut w, &model);
    let mut again = fresh();
    let f2 = shape::read_file(&mut Reader::new(&w.out), &mut again);
    o.round_trip = again == model && f2.end == End::Zero && f2.accepted && f2.pos == w.out.len();
    o.same_v4 = data.len() >= 88 && w.out[88..] == file[88.min(file.len())..];
    // with the file's own version and comment
    let mut w = Writer::with_version(f.version);
    si_file::write_bin_header(&mut w.out, si_file::MAGIC_SHAPE, f.version, Some(&f.comment));
    shape::write_body(&mut w, &model);
    o.same_own = w.out[..] == *file;
    if !o.same_own {
        // the two files in three parts: header, prototype block, the rest
        let written_lists = proto::output_lists(f.version);
        let block: usize = 6 + written_lists.iter().map(|(_, l)| 4 + 2 * l.len()).sum::<usize>();
        let (ours_rest, file_rest) = (&w.out[(88 + block).min(w.out.len())..], &file[f.prototypes_end.min(file.len())..]);
        let lists = if !f.has_prototypes {
            "the file has no prototype block".to_string()
        } else {
            let d = list_differences(&f.prototypes, &written_lists);
            if d.is_empty() { "prototype block identical".to_string() } else { format!("prototype lists differ from this build's: {}", d.join(" ")) }
        };
        let header = if w.out.len() >= 88 && file.len() >= 88 && w.out[..88] == file[..88] { "" } else { "header differs; " };
        // one known cause of a difference in the records: in a file older than 3.71 the
        // reader sets the top byte of every node colour to 0xff. Read with the version
        // taken as 3.71, the colours stay as stored.
        let unforced = || {
            let mut patched = file.to_vec();
            if patched.len() >= 8 {
                patched[4..8].copy_from_slice(&3.71f32.to_bits().to_be_bytes());
            }
            let mut m = fresh();
            shape::read_file(&mut Reader::new(&patched), &mut m);
            m
        };
        let colour_bytes = |b: &[u8]| b.iter().zip(file_rest).filter(|(x, y)| x != y).count();
        let records = if ours_rest == file_rest {
            "records identical".to_string()
        } else {
            let mut wu = Writer::with_version(f.version);
            shape::write_body(&mut wu, &unforced());
            let by = body_by(&model, f.tag, &f.prototypes, f.version);
            if wu.out[block.min(wu.out.len())..] == *file_rest && ours_rest.len() == file_rest.len() {
                o.note = Some(format!("{}; {} colour bytes changed by the reader", o.note.take().unwrap_or_default(), colour_bytes(ours_rest)));
                "records identical but for the node colours, whose top byte the reader sets to 0xff in a file below 3.71".to_string()
            } else {
                match by {
                    Ok(b) if b[..] == *file_rest => "records identical when written by the file's own lists".to_string(),
                    Ok(b) => match body_by(&unforced(), f.tag, &f.prototypes, f.version) {
                        Ok(u) if u[..] == *file_rest && b.len() == file_rest.len() => {
                            o.note = Some(format!("{}; {} colour bytes changed by the reader", o.note.take().unwrap_or_default(), colour_bytes(&b)));
                            "records identical when written by the file's own lists, but for the node colours, whose top byte the reader sets to 0xff in a file below 3.71".to_string()
                        }
                        _ => format!("records differ even by the file's own lists: {}", first_diff(file_rest, &b)),
                    },
                    Err(e) => format!("records differ, and cannot be written by the file's lists: {e}"),
                }
            }
        };
        o.why = Some(format!("{header}{lists}; {records}"));
    }
    o
}

#[derive(Default)]
struct Tally {
    files: usize,
    distinct: BTreeSet<u64>,
    exact: usize,
    round_trip: usize,
    same_own: usize,
    same_v4: usize,
    versions: BTreeMap<String, [usize; 4]>, // files, exact, same_own, same_v4
    reasons: BTreeMap<String, usize>,
    list: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = None;
    let (mut list, mut tokens, mut only) = (false, false, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => list = true,
            "--tokens" => tokens = true,
            "--only" => {
                i += 1;
                only = args.get(i).cloned();
            }
            a => root = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let Some(root) = root else {
        eprintln!("usage: elem-check <dir> [--list] [--tokens] [--only <ext>]");
        std::process::exit(2);
    };
    let mut all = Vec::new();
    files(&root, &mut all);
    let kinds: [(&str, &str, fn(&[u8], &mut Census) -> Outcome); 7] = [
        ("cam", "camera", check_camera),
        ("lig", "light", check_light),
        ("lat", "lattice", check_lattice),
        ("ani", "animation", check_anim),
        ("key", "struct keys", check_anim),
        ("cls", "cluster", check_cluster),
        ("shp", "shape", check_shape),
    ];
    let mut census = Census::new();
    for (e, name, check) in kinds {
        if only.as_deref().is_some_and(|o| o != e) {
            continue;
        }
        let mut t = Tally::default();
        for p in all.iter().filter(|p| ext(p) == e) {
            let Ok(data) = std::fs::read(p) else { continue };
            t.files += 1;
            t.distinct.insert(hash(&data));
            let o = check(&data, &mut census);
            let exact = o.zero && o.pos == o.limit && !o.clamped;
            let v = t.versions.entry(format!("{:.5}", o.version)).or_default();
            v[0] += 1;
            v[1] += exact as usize;
            v[2] += o.same_own as usize;
            v[3] += o.same_v4 as usize;
            t.exact += exact as usize;
            t.round_trip += o.round_trip as usize;
            t.same_own += o.same_own as usize;
            t.same_v4 += o.same_v4 as usize;
            let mut remarks = Vec::new();
            if !exact {
                let why = if !o.zero {
                    format!("file ends before the zero token (stopped at {} of {})", o.pos, o.limit)
                } else if o.clamped {
                    "a size in the file exceeds the file".to_string()
                } else {
                    format!("reader stops at {} of {}", o.pos, o.limit)
                };
                *t.reasons.entry(format!("not exact: {}", why.split(" (").next().unwrap().split(" at ").next().unwrap())).or_default() += 1;
                remarks.push(why);
            }
            if !o.round_trip {
                *t.reasons.entry("round trip differs".to_string()).or_default() += 1;
                remarks.push("round trip differs".to_string());
            }
            if let Some(n) = &o.note {
                for part in n.split("; ") {
                    let key: String = part.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
                    *t.reasons.entry(format!("note: {key}")).or_default() += 1;
                }
                remarks.push(n.clone());
            }
            if !o.same_own {
                if let Some(w) = &o.why {
                    let key = if w.starts_with("first difference") { "bytes differ: other".to_string() } else { format!("bytes differ: {w}") };
                    *t.reasons.entry(key).or_default() += 1;
                    remarks.push(format!("bytes: {w}"));
                }
            }
            if !remarks.is_empty() {
                t.list.push(format!("{}: {}", p.display(), remarks.join("; ")));
            }
        }
        println!("== {name} (.{e}): {} files, {} distinct contents", t.files, t.distinct.len());
        println!("   read to the exact end:            {:5} of {}", t.exact, t.files);
        println!("   round trip at 4.0 (record equal): {:5} of {}", t.round_trip, t.files);
        println!("   byte-identical, own version:      {:5} of {}", t.same_own, t.files);
        println!("   byte-identical body, as 4.0:      {:5} of {}", t.same_v4, t.files);
        println!("   by header version (files / exact end / identical own version / identical as 4.0):");
        let mut vs: Vec<_> = t.versions.iter().collect();
        vs.sort_by(|a, b| a.0.parse::<f64>().unwrap_or(0.0).total_cmp(&b.0.parse::<f64>().unwrap_or(0.0)));
        for (v, c) in vs {
            println!("      {v:>9}: {:4} / {:4} / {:4} / {:4}", c[0], c[1], c[2], c[3]);
        }
        for (r, n) in &t.reasons {
            println!("   {n:5}  {r}");
        }
        if list {
            for l in &t.list {
                println!("   - {l}");
            }
        }
    }
    if tokens {
        println!("== tokens dispatched (reader, token: times)");
        for ((reader, tok), n) in &census {
            println!("   {reader:<12} 0x{tok:04x}: {n}");
        }
    }
}
