// SPDX-License-Identifier: MIT
//! `scene-check <dir> saveall`: every scene that loads completely is saved with the port of
//! "Save Scene" into a file system in memory laid over the real one (nothing is ever written
//! to disk), and every file the save writes is compared with the file it was loaded from.
//!
//! Two saves per scene:
//!
//! 1. **Content**: every element is marked dirty and names are kept, so each file is written
//!    under the name it was read from and can be compared byte for byte.
//! 2. **Names**: the scene is loaded again and saved the way the Save dialogue does it (the
//!    dirty marks worked out by `si_edGetDirtyFlags`, versions going up): which files are
//!    written, and whether each new name is the element's highest version on disk plus one.

use si_db::{Bytes, DbInfo, FileSystem, FileSystemMut, RealFs, Stat};
use si_scene::calls::{Arg, Ask, Call, Recorder};
use si_scene::dictout::Host;
use si_scene::load::{Outcome, Scene};
use si_scene::object::Object;
use si_scene::scnrel::{self, Parts};
use si_scene::{dictout, savescene};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The real file system for reading, a map in memory for everything written.
#[derive(Default)]
pub struct OverlayFs {
    pub files: HashMap<Bytes, Bytes>,
    pub dirs: BTreeSet<Bytes>,
    pub removed: BTreeSet<Bytes>,
    /// Shell command lines the save wanted to run; none is carried out.
    pub commands: Vec<Bytes>,
}

fn normal(path: &[u8]) -> Bytes {
    let mut out = Vec::with_capacity(path.len());
    if path.first() == Some(&b'/') {
        out.push(b'/');
    }
    for part in path.split(|&c| c == b'/').filter(|p| !p.is_empty()) {
        if !out.is_empty() && out.last() != Some(&b'/') {
            out.push(b'/');
        }
        out.extend_from_slice(part);
    }
    out
}

impl FileSystem for OverlayFs {
    fn read_dir(&self, path: &[u8]) -> Option<Vec<Bytes>> {
        let dir = normal(path);
        let mut names: Vec<Bytes> = RealFs.read_dir(path).unwrap_or_default();
        let known = RealFs.stat(path).is_some() || self.dirs.contains(&dir);
        for p in self.files.keys().chain(self.dirs.iter()) {
            if p.len() > dir.len() + 1 && p.starts_with(&dir) && p[dir.len()] == b'/' && !p[dir.len() + 1..].contains(&b'/') {
                let name = p[dir.len() + 1..].to_vec();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names.retain(|n| !self.removed.contains(&normal(&[&dir[..], b"/", n].concat())));
        known.then_some(names)
    }

    fn stat(&self, path: &[u8]) -> Option<Stat> {
        let p = normal(path);
        if self.removed.contains(&p) {
            return None;
        }
        if let Some(d) = self.files.get(&p) {
            return Some(Stat { mode: si_db::S_IFREG | 0o644, uid: 0, size: d.len() as i32, mtime: 0 });
        }
        if self.dirs.contains(&p) {
            return Some(Stat { mode: si_db::S_IFDIR | 0o755, uid: 0, size: 0, mtime: 0 });
        }
        RealFs.stat(path)
    }

    fn access(&self, path: &[u8], mode: i32) -> bool {
        let p = normal(path);
        if self.removed.contains(&p) {
            return false;
        }
        if self.files.contains_key(&p) || self.dirs.contains(&p) {
            return true;
        }
        // the sample databases are read-only to this check, but the program would have been
        // able to write to its own database: what exists counts as writable
        if mode & si_db::W_OK != 0 {
            return RealFs.access(path, si_db::F_OK);
        }
        RealFs.access(path, mode)
    }

    fn read(&self, path: &[u8]) -> Option<Bytes> {
        let p = normal(path);
        if self.removed.contains(&p) {
            return None;
        }
        self.files.get(&p).cloned().or_else(|| RealFs.read(path))
    }
}

impl FileSystemMut for OverlayFs {
    fn mkdir(&mut self, path: &[u8]) -> bool {
        self.dirs.insert(normal(path))
    }
    fn write(&mut self, path: &[u8], data: &[u8], append: bool) -> bool {
        let p = normal(path);
        // the directory must exist, as for `fopen`
        let dir_ok = match p.iter().rposition(|&c| c == b'/') {
            Some(i) => self.dirs.contains(&p[..i]) || RealFs.stat(&p[..i]).is_some_and(|s| s.is_dir()),
            None => true,
        };
        if !dir_ok {
            return false;
        }
        self.removed.remove(&p);
        if append {
            let mut old = self.files.get(&p).cloned().or_else(|| RealFs.read(path)).unwrap_or_default();
            old.extend_from_slice(data);
            self.files.insert(p, old);
        } else {
            self.files.insert(p, data.to_vec());
        }
        true
    }
    fn unlink(&mut self, path: &[u8]) -> bool {
        let p = normal(path);
        let was = self.files.remove(&p).is_some() || RealFs.stat(path).is_some();
        self.removed.insert(p);
        was
    }
    fn chmod(&mut self, _path: &[u8], _mode: u32) -> bool {
        true
    }
    fn rename(&mut self, from: &[u8], to: &[u8]) -> bool {
        match self.read(from) {
            Some(d) => {
                self.unlink(from);
                self.write(to, &d, false)
            }
            None => false,
        }
    }
    fn system(&mut self, command: &[u8]) -> i32 {
        self.commands.push(command.to_vec());
        1
    }
}

/// Answers the save's questions for the checks: nothing is cancelled; the channel and flock
/// setups are "the default ones" exactly when the scene read none; every old version counts
/// as still named by a scene (nothing is deleted).
pub struct CheckUi {
    pub has_channel_setup: bool,
    pub has_flock_setup: bool,
    pub asked: Vec<Call>,
}

impl Ask for CheckUi {
    fn ask(&mut self, name: &'static str, args: &[Arg]) -> i32 {
        self.asked.push(Call { name, args: args.to_vec() });
        match name {
            "SI_chnSetupDefault" => !self.has_channel_setup as i32,
            "SI_particleSetupDefault" => !self.has_flock_setup as i32,
            "si_cusCheckForHybridIcons" => 0,
            // "Save" in the "exists in database" dialogue
            "SI_dialogRequest34" => 1,
            n if n.starts_with("SI_dbfileCheckElement") => 1,
            // the writers of packages that are not ported: taken as having succeeded
            "SI_udfWrite" | "SI_dbwaveAsciiWrite" | "SI_elemdictWriteExpressions" | "SI_sprdSetupWrite" | "SI_chnSetupSave" | "SI_particleSetupSave" | "SI_setupFileSavePath"
            | "SI_omodelBinaryWrite" | "SI_shapeBinaryWrite" | "SI_clusterAnimBinaryWrite" | "SI_elemdictWriteLight" | "SI_elemdictWriteCamera" | "SI_elemdictWriteLattice"
            | "SI_materialWriteBinaryFile" | "SI_textureWriteBinaryFile" | "SI_texture3DWriteBinaryFile" => 1,
            _ => 0,
        }
    }
}

#[derive(Default)]
pub struct ChapterTotals {
    pub written: usize,
    pub identical: usize,
    /// Identical once the appended icon is left out.
    pub icon_only: usize,
    /// Identical but for the version number in the file's header.
    pub version_only: usize,
    /// Equal to what this build writes for the file's own content (and not to the file).
    pub as_rewritten: usize,
    /// The versions of the files in those two classes.
    pub versions: BTreeMap<String, usize>,
    /// The scene changed the content: the causes are listed.
    pub changed: usize,
    pub new_files: usize,
    /// Files compared whose original carries an appended icon.
    pub with_icon: usize,
    pub causes: BTreeMap<String, usize>,
    pub examples: BTreeMap<String, Vec<String>>,
    pub not_written: BTreeMap<String, usize>,
    /// Files the load read that the save did not write, by reason.
    pub skipped: BTreeMap<String, usize>,
}

#[derive(Default)]
pub struct Totals {
    pub scenes: usize,
    pub by_chapter: BTreeMap<String, ChapterTotals>,
    pub commands: usize,
    pub asked: BTreeMap<&'static str, usize>,
    pub calls: BTreeMap<&'static str, usize>,
    pub aborted: Vec<String>,
    // the second save
    pub named: BTreeMap<String, (usize, usize, usize)>,
    pub name_faults: Vec<String>,
    pub purge_candidates: usize,
    pub curves: (usize, usize, usize),
}

fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

fn body(data: &[u8]) -> &[u8] {
    match si_picture::icon_offset(data) {
        Some(off) if off <= data.len() => &data[..off],
        _ => data,
    }
}

/// The version in a binary database file's header (big-endian float after the magic).
fn header_version(data: &[u8]) -> String {
    if data.len() >= 8 {
        format!("{}", f32::from_bits(u32::from_be_bytes([data[4], data[5], data[6], data[7]])))
    } else {
        "?".into()
    }
}

/// What this build writes for a file's own content, with nothing of the scene around it.
fn rewrite_alone(sc: &Scene, chapter: &str, elem: si_scene::dict::ElemId, orig: &[u8]) -> Option<Vec<u8>> {
    use si_elemio::io::{Reader, Writer};
    let data = sc.dict.slot(elem).map_or(0, |e| e.rec.data.0);
    match chapter {
        "SHAPES" => {
            // the element points at its model: the file is read for a model of that kind
            let Some(Object::ModelNode { tree, node }) = sc.objects.get(data) else { return None };
            let kind = sc.models.get(*tree)?.kinds[*node];
            let mut r = Reader::new(orig);
            let mut model = si_elemio::shape::ShapeModel::new(kind);
            si_elemio::shape::read_file(&mut r, &mut model);
            let mut w = Writer::with_version(4.0);
            si_elemio::shape::write_file(&mut w, &model);
            Some(w.out)
        }
        "CLUSTERS" => {
            let Some(Object::Cluster { model, cluster }) = sc.objects.get(data).cloned() else { return None };
            let Some(Object::ModelNode { tree, node }) = sc.objects.get(model) else { return None };
            let n = sc.models.get(*tree)?.node(*node)?;
            let mut klusters: Vec<si_elemio::cluster::Kluster> =
                n.clusters.iter().map(|c| si_elemio::cluster::Kluster { name: si_hrc::io::c_str(&c.name).to_vec(), nb_points: c.count, data: None }).collect();
            let mut r = Reader::new(orig);
            let (mut an, mut geom) = (None, si_elemio::geom::GeomState::default());
            let f = si_elemio::cluster::read_file(&mut r, n.kind, &mut klusters, &mut an, &mut geom);
            let mut w = Writer::with_version(4.0);
            si_elemio::cluster::write_file(&mut w, f.read.current.as_ref(), n.kind, klusters.get(cluster)?, an.as_ref(), n.uv_sets);
            Some(w.out)
        }
        "MODELS" => si_hrc::read_file(orig, si_hrc::Kind::Model).and_then(|f| si_hrc::write_content(&f.content)),
        "MATERIALS" => si_hrc::read_file(orig, si_hrc::Kind::Material).and_then(|f| si_hrc::write_content(&f.content)),
        "TEXTURES2D" => si_hrc::read_file(orig, si_hrc::Kind::Texture).and_then(|f| si_hrc::write_content(&f.content)),
        "TEXTURES3D" => si_hrc::read_file(orig, si_hrc::Kind::Texture3D).and_then(|f| si_hrc::write_content(&f.content)),
        "CAMERAS" => {
            let mut r = Reader::new(orig);
            let mut cam = si_elemio::camera::Camera::perspective_default();
            si_elemio::camera::read_file(&mut r, &mut cam);
            let mut w = Writer::with_version(4.0);
            si_elemio::camera::write_file(&mut w, &cam);
            Some(w.out)
        }
        "LIGHTS" => {
            let mut r = Reader::new(orig);
            let f = si_elemio::light::read_file(&mut r, sc.program.now);
            let l = f.read.light?;
            let mut w = Writer::with_version(4.0);
            si_elemio::light::write_file(&mut w, &l);
            Some(w.out)
        }
        "LATTICES" => {
            let mut r = Reader::new(orig);
            let f = si_elemio::lattice::read_file(&mut r, true);
            let mut w = Writer::with_version(4.0);
            let _ = si_elemio::lattice::write_file(&mut w, &f.read.lattices);
            Some(w.out)
        }
        "ANIMATION" => {
            let mut r = Reader::new(orig);
            r.st.frame_rate = sc.statics.frame_rate;
            let f = si_elemio::anim::read_file(&mut r, 0);
            let mut w = Writer::with_version(4.0);
            w.st.frame_rate = sc.statics.frame_rate;
            let chapter = f.anim.chapter.clone().unwrap_or_else(|| b"MODELS".to_vec());
            si_elemio::anim::write_ani_file(&mut w, Some(&chapter), &f.anim, si_elemio::anim::ItemHeaders::Stored, false);
            Some(w.out)
        }
        "SETUP_SOFT" => si_ascii::setup::read(orig).ok().map(|(s, _)| si_ascii::setup::write(&s, &si_ascii::setup::WriteOptions::default())),
        "SETUP_CHANNELS" => si_ascii::chn::read(orig).ok().and_then(|(s, _)| si_ascii::chn::write(&s, &si_ascii::chn::WriteContext::default()).ok()),
        "SETUP_FLOCK" => si_ascii::flk::read(orig).ok().map(|(s, _)| si_ascii::flk::write(&s, 4.0)),
        _ => None,
    }
}

/// Why a written model file differs from this build's plain re-write of the file it was
/// read from: which of the things the load changed in the hierarchy make a difference.
fn model_causes(sc: &Scene, data: u32, alone: &[u8]) -> String {
    let Some(Object::ModelNode { tree, .. }) = sc.objects.get(data) else { return "the element's object is not a model of this crate".into() };
    let write = |parts: Parts| scnrel::assemble_model_parts(sc, *tree, parts).map(|(root, _)| si_hrc::write_model(&root));
    let mut why = Vec::new();
    if write(Parts { names: true, ..Parts::NONE }).as_deref() != Some(alone) {
        why.push("node names taken from the elements");
    }
    if write(Parts { state: true, ..Parts::NONE }).as_deref() != Some(alone) {
        why.push("the root's transformation is the description's SRT");
    }
    if write(Parts { links: true, ..Parts::NONE }).as_deref() != Some(alone) {
        why.push("materials and textures are those of the element files");
    }
    if why.is_empty() {
        "differs from the plain re-write for no part alone".into()
    } else {
        why.join(" + ")
    }
}

/// Why a written file of another chapter differs from the plain re-write of its file.
fn scene_cause(sc: &Scene, chapter: &str, elem: si_scene::dict::ElemId, alone: &[u8], written: &[u8]) -> String {
    match chapter {
        "MATERIALS" | "TEXTURES2D" | "TEXTURES3D" | "LIGHTS" => {
            // the object's name is the element's (cleaned of characters the database layer
            // does not allow in a name)
            let name = sc.dict.node_of(elem.chap, elem.index).and_then(|n| n.name.clone()).unwrap_or_default();
            let at = first_diff(alone, written);
            let lo = at.saturating_sub(name.len() + 2);
            if written[lo..(at + name.len() + 2).min(written.len())].windows(name.len().max(1)).any(|w| w == &name[..name.len().min(29)]) {
                return "the record's name is the element's name (cut to 29 characters, characters the database layer does not allow replaced)".into();
            }
            format!("content changed by the scene (first difference from the plain re-write at byte {at})")
        }
        "ANIMATION" => {
            use si_elemio::io::Reader;
            let read = |d: &[u8]| {
                let mut r = Reader::new(d);
                r.st.frame_rate = sc.statics.frame_rate;
                si_elemio::anim::read_file(&mut r, 0).anim
            };
            let (a, b) = (read(alone), read(written));
            let by_name = |h: &si_elemio::anim::HierAnim, part: bool| {
                let mut v: Vec<(Vec<u8>, Vec<si_elemio::anim::Item>)> =
                    h.elems.iter().map(|e| (if part { si_db::filename_expand(&e.name).name } else { e.name.clone() }, e.items.clone())).collect();
                v.sort_by(|x, y| x.0.cmp(&y.0));
                v
            };
            if a.actions != b.actions || a.sources != b.sources {
                "the action lists differ".into()
            } else if by_name(&a, false) == by_name(&b, false) {
                "the same elements with the same curves, listed in the dictionary's order (the file has the saving session's)".into()
            } else if by_name(&a, true) == by_name(&b, true) {
                "the same curves; elements are named by their present full names (the file names them by another prefix)".into()
            } else {
                format!("other curves or elements (first difference from the plain re-write at byte {})", first_diff(alone, written))
            }
        }
        _ => format!("content changed by the scene (first difference from the plain re-write at byte {})", first_diff(alone, written)),
    }
}

fn chapter_name(sc: &Scene, chap: i32) -> String {
    String::from_utf8_lossy(&sc.dict.ed.lists[chap as usize].name).into_owned()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The first save: content. `sc` is a scene just loaded from `info`.
/// `dsc_orig` is the scene's description as read; `dsc_ref` what its save is compared with
/// (the file with the relations the original's load adds: `with_relation_set`).
pub fn content_check(sc: &mut Scene, info: &mut DbInfo, label: &str, dsc_orig: &[u8], dsc_ref: &[u8], t: &mut Totals) {
    t.scenes += 1;
    let before = {
        let check = si_scene::save::check_against(sc, dsc_ref);
        let mut un = Recorder::default();
        let plain = si_scene::save::write_description(sc, si_scene::save::Build::Linux, 4.0, &mut un);
        DscBefore { check, plain }
    };
    // every element is to be written: the check's own choice
    for l in sc.dict.ed.lists.iter_mut() {
        for e in l.elems.iter_mut() {
            e.rec.dirty = 1;
        }
    }
    let curves = sc.dict.ed.lists[sc.program.cid.ani as usize].elems.iter().filter(|e| e.rec.status == 0 && e.rec.hier_level != 0).count();
    let tied = sc.dict.ed.lists[sc.program.cid.ani as usize].elems.iter().filter(|e| e.rec.status == 0 && e.rec.hier_level != 0 && e.backward.iter().any(|l| !l.rels.is_empty())).count();
    t.curves.0 += curves;
    t.curves.1 += tied;
    sc.saving.keep_names = true;
    let mut fs = OverlayFs::default();
    let mut un = Recorder::default();
    let mut ui = CheckUi { has_channel_setup: sc.channel_setup.is_some(), has_flock_setup: sc.flock_setup.is_some(), asked: Vec::new() };
    let read_paths: Vec<(i32, i32, Vec<u8>)> = sc
        .report
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Read { path, .. } | Outcome::Located { path, .. } => Some((r.chap, r.index, path.clone())),
            _ => None,
        })
        .collect();
    let scene_path = {
        let n = sc.dict.node_of(sc.program.cid.dsc, 0);
        n.and_then(|n| n.full.clone()).unwrap_or_default()
    };
    let ok = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        savescene::save_scene(sc, &mut h, info, false, false, None, false)
    };
    if !ok {
        t.aborted.push(format!("{label}: the save stopped before the description"));
    }
    t.commands += fs.commands.len();
    for c in &ui.asked {
        *t.asked.entry(c.name).or_default() += 1;
    }
    for c in &un.calls {
        *t.calls.entry(c.name).or_default() += 1;
    }
    let written: Vec<dictout::Written> = sc.saving.written.clone();
    let mut written_paths = BTreeSet::new();
    for w in &written {
        written_paths.insert(normal(&w.path));
        let chapter = chapter_name(sc, w.elem.chap);
        let data = fs.files.get(&normal(&w.path)).cloned().unwrap_or_default();
        let orig = RealFs.read(&w.path).map(|o| if chapter == "SCENES" && o == dsc_orig { dsc_ref.to_vec() } else { o });
        let elem_data = sc.dict.slot(w.elem).map_or(0, |e| e.rec.data.0);
        let alone_owned = orig.as_deref().and_then(|o| rewrite_alone(sc, &chapter, w.elem, o));
        let ct = t.by_chapter.entry(chapter.clone()).or_default();
        ct.written += 1;
        // the same bytes but for the version number in the header (bytes 4 to 7 of a binary
        // file; the first line of a text file)
        let same_but_version = |a: &[u8], b: &[u8]| -> bool {
            if a.len() >= 8 && b.len() >= 8 && a[..4] == b[..4] && a[0] == 0x53 && a[1] == 0x80 {
                a.len() == b.len() && a[8..] == b[8..]
            } else {
                let cut = |t: &[u8]| t.iter().position(|&c| c == b'\n').map_or(t.len(), |p| p + 1);
                a[cut(a).min(a.len())..] == b[cut(b).min(b.len())..]
            }
        };
        let cause: String = match &orig {
            None => {
                ct.new_files += 1;
                "no file of that name was read (a new file)".into()
            }
            Some(o) if *o == data => {
                ct.identical += 1;
                continue;
            }
            Some(o) if chapter == "SCENES" => {
                let _ = scene_path;
                let c = dsc_cause(&data, o, &before);
                if c.is_empty() {
                    ct.version_only += 1;
                    continue;
                }
                ct.changed += 1;
                c
            }
            Some(o) => {
                let b = body(o);
                ct.with_icon += (b.len() != o.len()) as usize;
                let icon = if b.len() != o.len() { "; the file also carries an icon, and a save writes none" } else { "" };
                if b == &data[..] {
                    ct.icon_only += 1;
                    continue;
                }
                let version = header_version(o);
                if same_but_version(b, &data) {
                    ct.version_only += 1;
                    *ct.versions.entry(version).or_default() += 1;
                    if !icon.is_empty() {
                        *ct.causes.entry("(of those identical but for the header's version: files that also carry an icon, which a save does not write)".into()).or_default() += 1;
                    }
                    continue;
                }
                match alone_owned.as_deref() {
                    Some(alone) if alone == &data[..] => {
                        ct.as_rewritten += 1;
                        *ct.versions.entry(version).or_default() += 1;
                        continue;
                    }
                    Some(alone) => {
                        ct.changed += 1;
                        if let Ok(dir) = std::env::var("SCENE_CHECK_DUMP") {
                            let name = text(&w.path).rsplit('/').next().unwrap_or("x").to_string();
                            let _ = std::fs::write(format!("{dir}/{name}.written"), &data);
                            let _ = std::fs::write(format!("{dir}/{name}.alone"), alone);
                            let _ = std::fs::write(format!("{dir}/{name}.orig"), o);
                        }
                        let why = if chapter == "MODELS" {
                            model_causes(sc, elem_data, alone)
                        } else {
                            scene_cause(sc, &chapter, w.elem, alone, &data)
                        };
                        format!("{why}{icon}")
                    }
                    None => {
                        ct.changed += 1;
                        format!("differs (no plain re-write to compare with), first at byte {} of {}", first_diff(b, &data), b.len())
                    }
                }
            }
        };
        *ct.causes.entry(cause.clone()).or_default() += 1;
        let ex = ct.examples.entry(cause).or_default();
        if ex.len() < 3 {
            ex.push(format!("{label}: {}", text(&w.path).rsplit('/').next().unwrap_or("")));
        }
    }
    for (id, writer) in &sc.saving.not_written {
        let chapter = chapter_name(sc, id.chap);
        *t.by_chapter.entry(chapter).or_default().not_written.entry(writer.to_string()).or_default() += 1;
    }
    // files the load read that the save did not write
    for (chap, index, path) in read_paths {
        if written_paths.contains(&normal(&path)) {
            continue;
        }
        let chapter = chapter_name(sc, chap);
        let e = sc.dict.ed.lists[chap as usize].elems.get(index as usize).map(|e| e.rec);
        let reason = match e {
            None => "the element is gone",
            Some(e) if e.status != 0 => "the element was deleted by the load",
            Some(_) if written.iter().any(|w| w.elem.chap == chap && w.elem.index == index) => "written under another name (a setup takes the scene's name)",
            Some(_) if sc.saving.not_written.iter().any(|(id, _)| id.chap == chap && id.index == index) => "its writer is another package's (counted above)",
            Some(e) if e.data.is_null() => "the element has no object (its reader is another package's, or the setup is the default one), so the saver passes it over",
            Some(e) if e.hier_level != 0 => "not a root",
            Some(_) => "written under another name",
        };
        *t.by_chapter.entry(chapter).or_default().skipped.entry(reason.to_string()).or_default() += 1;
    }
}

/// A line without one item: the keyword and the values that follow it.
fn strip_item(line: &[u8], word: &[u8], values: usize) -> Vec<u8> {
    let tokens: Vec<&[u8]> = line.split(|&c| c == b' ').collect();
    match tokens.iter().position(|t| *t == word) {
        Some(p) => {
            let mut kept: Vec<&[u8]> = tokens[..p].to_vec();
            kept.extend_from_slice(&tokens[(p + 1 + values).min(tokens.len())..]);
            kept.join(&b' ')
        }
        None => line.to_vec(),
    }
}

/// The lines of a description with where each stands: section and chapter.
fn dsc_lines(t: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let (mut section, mut chapter) = (String::new(), String::new());
    for l in t.split(|&c| c == b'\n').skip(1) {
        let words: Vec<&[u8]> = l.split(|&c| c == b' ').filter(|w| !w.is_empty()).collect();
        match words.first().copied() {
            Some(b"ELEMENTS") | Some(b"RELATIONS") | Some(b"ENVIRONMENT") => {
                section = text(words[0]);
                chapter.clear();
            }
            Some(b"PLAYCTRL") => chapter = "PLAYCTRL".into(),
            Some(b"CHAPTER") => {
                chapter = if section == "RELATIONS" { format!("{} {}", text(words.get(1).copied().unwrap_or(b"")), text(words.get(3).copied().unwrap_or(b""))) } else { text(words.get(1).copied().unwrap_or(b"")) };
                // the header's own count changes with its lines; it is not compared
                continue;
            }
            _ => {}
        }
        out.push((format!("{section}/{chapter}"), l.to_vec()));
    }
    out
}

/// What the description check found for a scene before it was saved, and the description as
/// the dictionary gave it then (this build's words and name order, version 4).
pub struct DscBefore {
    pub check: si_scene::save::SaveCheck,
    pub plain: Vec<u8>,
}

/// Why a written description differs from the file; empty when only the version in the
/// header does.
fn dsc_cause(written: &[u8], orig: &[u8], before: &DscBefore) -> String {
    use si_scene::save::Build;
    let rest = |t: &[u8]| t.iter().position(|&c| c == b'\n').map_or(Vec::new(), |p| t[p + 1..].to_vec());
    if rest(written) == rest(orig) {
        return String::new();
    }
    let mut why: Vec<String> = Vec::new();
    let r = &before.check;
    // 1. what the file itself is, against this build's writer (the description check)
    match r.same_replayed {
        None => why.push("one of the ten descriptions the description check cannot reproduce (finding 0018, 7.2)".into()),
        Some(_) => {
            if r.same.is_none() {
                why.push("relation and environment lines follow the element order of the session that saved".into());
            }
            let has = |x: Build| r.fits.contains(&x);
            if !(has(Build::Linux) || has(Build::LinuxNamesNtNumbers)) {
                why.push(if has(Build::Nt) || has(Build::NtNamesLinuxNumbers) { "the NT build's name order".into() } else { "the plain strcmp name order of versions 2.61 to 2.65".into() });
            }
            if !(has(Build::Linux) || has(Build::NtNamesLinuxNumbers) || has(Build::Old)) {
                why.push("the NT build's three-digit exponents".into());
            }
            if r.no_mprflg {
                why.push("no MPRFLG in files of versions before 3.704".into());
            }
        }
    }
    // 2. what the save changed in the dictionary before it wrote
    let (w, p) = (dsc_lines(written), dsc_lines(&before.plain));
    let mut count: BTreeMap<(String, Vec<u8>), i32> = BTreeMap::new();
    for (c, l) in &w {
        *count.entry((c.clone(), l.clone())).or_default() += 1;
    }
    for (c, l) in &p {
        *count.entry((c.clone(), l.clone())).or_default() -= 1;
    }
    let mut places: BTreeSet<String> = BTreeSet::new();
    let mut srt_only = true;
    for ((c, l), n) in &count {
        if *n == 0 {
            continue;
        }
        if c == "ENVIRONMENT/MODELS" {
            // the same line without its SRT item on the other side?
            let stripped = strip_item(l, b"SRT", 9);
            let twin = count.iter().any(|((c2, l2), n2)| c2 == c && *n2 == -*n && l2 != l && strip_item(l2, b"SRT", 9) == stripped);
            if twin {
                places.insert("SRT".into());
                continue;
            }
        }
        srt_only = false;
        places.insert(c.clone());
    }
    let _ = srt_only;
    for pl in &places {
        let cause = if pl == "SRT" {
            "SRT printed for every root model (the dictionary had it for some only)".to_string()
        } else if pl.starts_with("ELEMENTS/SETUP_") {
            "the setup elements take the scene's name".to_string()
        } else if pl.contains("EXPRESSIONS") {
            "the first root expression takes the scene's name (its chapter is numbered anew)".to_string()
        } else if pl.starts_with("ELEMENTS/") {
            format!("element names changed by the save in {}", &pl[9..])
        } else {
            format!("other lines in {pl}")
        };
        if !why.contains(&cause) {
            why.push(cause);
        }
    }
    if why.is_empty() {
        why.push("differs from the file for a reason none of the tests names".into());
    }
    why.join("; ")
}

/// The second save: names. `sc` is a scene just loaded again.
pub fn name_check(sc: &mut Scene, info: &mut DbInfo, label: &str, t: &mut Totals) {
    let mut fs = OverlayFs::default();
    let mut un = Recorder::default();
    let mut ui = CheckUi { has_channel_setup: sc.channel_setup.is_some(), has_flock_setup: sc.flock_setup.is_some(), asked: Vec::new() };
    // as the Save dialogue does: the dirty marks first
    savescene::get_dirty_flags(sc, &mut un);
    let roots: BTreeMap<i32, usize> = (0..sc.dict.ed.lists.len() as i32)
        .map(|c| (c, sc.dict.ed.lists[c as usize].elems.iter().filter(|e| e.rec.status == 0 && e.rec.hier_level == 0).count()))
        .collect();
    {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        savescene::save_scene(sc, &mut h, info, false, false, None, false);
    }
    t.purge_candidates += ui.asked.iter().filter(|c| c.name.starts_with("SI_dbfileCheckElement")).count();
    t.commands += fs.commands.len();
    let mut per: BTreeMap<i32, usize> = BTreeMap::new();
    for w in &sc.saving.written {
        *per.entry(w.elem.chap).or_default() += 1;
        // the new file must not exist on disk, and its version must be the highest of the
        // element's files on disk plus one (1 when there is none)
        let path = text(&w.path);
        if RealFs.stat(&w.path).is_some() {
            t.name_faults.push(format!("{label}: {path} exists on disk"));
            continue;
        }
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", &path));
        let stem = file.rsplit_once('.').map_or(file, |x| x.0);
        let parts = si_db::filename_expand(stem.as_bytes());
        let prefixed = si_db::format_prefix_name(&Some(parts.prefix.clone()), &Some(parts.name.clone()));
        let mut highest = 0;
        if let Some(names) = RealFs.read_dir(dir.as_bytes()) {
            for n in names {
                if si_db::file_compare_prefix(&n, &prefixed, Some(b"0")) {
                    let v = si_db::filename_expand(&n);
                    highest = highest.max(v.version);
                }
            }
        }
        let got = parts.version;
        if got != highest + 1 {
            t.name_faults.push(format!("{label}: {path}: version {got}, highest on disk {highest}"));
        }
    }
    for (chap, n) in roots {
        if n == 0 && !per.contains_key(&chap) {
            continue;
        }
        let e = t.named.entry(chapter_name(sc, chap)).or_default();
        e.0 += n;
        e.1 += per.get(&chap).copied().unwrap_or(0);
        e.2 += sc.saving.not_written.iter().filter(|(id, _)| id.chap == chap).count();
    }
}

pub fn print(t: &Totals, list: bool) {
    println!("\n== saving everything ==");
    println!("{} scenes saved into memory; every file written is compared with the file it was loaded from", t.scenes);
    println!("curve elements of the animation files: {}; tied to the element they animate by a relation: {}", t.curves.0, t.curves.1);
    println!("columns: files written | byte-identical to the file | identical once the appended icon is left out | identical but for the version in the header |");
    println!("         equal to this build's plain re-write of the file's content (older version or other build: other tokens) | content changed by the scene | new file");
    println!("{:16} {:>7} {:>9} {:>6} {:>8} {:>10} {:>8} {:>5}", "chapter", "written", "identical", "icon", "version", "re-write", "changed", "new");
    let mut sum = [0usize; 7];
    for (c, ct) in &t.by_chapter {
        if ct.written != 0 {
            let row = [ct.written, ct.identical, ct.icon_only, ct.version_only, ct.as_rewritten, ct.changed, ct.new_files];
            println!("{c:16} {:7} {:9} {:6} {:8} {:10} {:8} {:5}", row[0], row[1], row[2], row[3], row[4], row[5], row[6]);
            for k in 0..7 {
                sum[k] += row[k];
            }
        }
    }
    println!("{:16} {:7} {:9} {:6} {:8} {:10} {:8} {:5}", "total", sum[0], sum[1], sum[2], sum[3], sum[4], sum[5], sum[6]);
    println!("files compared whose original carries an appended icon (a save writes none): {}", t.by_chapter.values().map(|c| c.with_icon).sum::<usize>());
    println!("versions of the files that differ only as any re-save by this build does:");
    for (c, ct) in &t.by_chapter {
        if !ct.versions.is_empty() {
            println!("  {c:16} {}", ct.versions.iter().map(|(v, n)| format!("{v}: {n}")).collect::<Vec<_>>().join(", "));
        }
    }
    println!("causes where the scene changed the content:");
    for (c, ct) in &t.by_chapter {
        for (cause, n) in &ct.causes {
            println!("  {c:14} {n:5}  {cause}");
            if list {
                for e in ct.examples.get(cause).into_iter().flatten() {
                    println!("                        e.g. {e}");
                }
            }
        }
    }
    println!("files not written because the writer or the object is another package's:");
    for (c, ct) in &t.by_chapter {
        for (wr, n) in &ct.not_written {
            println!("  {c:22} {n:5}  {wr}");
        }
    }
    println!("files read by the load that the save did not write:");
    for (c, ct) in &t.by_chapter {
        for (r, n) in &ct.skipped {
            println!("  {c:22} {n:5}  {r}");
        }
    }
    println!("saves that stopped before the description: {}", t.aborted.len());
    for a in &t.aborted {
        println!("  {a}");
    }
    println!("questions put to the user or to packages not ported:");
    for (q, n) in &t.asked {
        println!("  {n:6}  {q}");
    }
    println!("shell commands the original would have run (refused): {}", t.commands);
    if list {
        println!("calls into packages not ported during the saves:");
        for (q, n) in &t.calls {
            println!("  {n:6}  {q}");
        }
    }
    println!("\n== saving as the Save dialogue does (dirty marks from si_edGetDirtyFlags, versions going up) ==");
    println!("{:22} {:>7} {:>7} {:>11}", "chapter", "roots", "written", "not ported");
    println!("(roots: elements in use at depth 0; written: files; not ported: files whose writer is another package's)");
    for (c, (roots, written, np)) in &t.named {
        println!("{c:22} {roots:7} {written:7} {np:11}");
    }
    println!("old versions that would be up for removal (each kept: taken as still named by a scene): {}", t.purge_candidates);
    println!("new files whose name is not the element's highest version on disk plus one: {}", t.name_faults.len());
    for f in t.name_faults.iter().take(if list { 200 } else { 5 }) {
        println!("  {f}");
    }
}
