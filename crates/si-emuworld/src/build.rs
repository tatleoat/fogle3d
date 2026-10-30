// SPDX-License-Identifier: MIT
//! One sample scene in the memory image, as far as the ports can build it.
//!
//! Nothing here reproduces a function of the original. Three stages, each on top of the
//! one before; a later stage is glue of this crate between ports that no ported routine
//! connects yet, and says so in [`World::notes`]:
//!
//! 1. **`dict`**: `si-scene`'s "Get Scene" ([`si_scene::load::load_scene`]), without its two
//!    stand-ins for packages that are not ported. The image then holds the dictionaries:
//!    chapters, elements, name records, relations, the relation dictionary, the chapter
//!    indices, the process record, the loader's globals. An element's object is a handle
//!    of the loader (an address of `si-mem`'s handle range, which is no memory).
//! 2. **`objects`**: for every element of the MODELS, MATERIALS, TEXTURES2D and TEXTURES3D
//!    chapters whose file the loader read, the same file is read again by `si-hrc`'s image
//!    readers into the same image, and the element's handle is replaced by the record's
//!    address. A model file is read only when the image readers have a reader for
//!    everything in it (at this crate's writing: polygon meshes and nodes without
//!    geometry). The readers' calls out of their package answer 0 and are counted.
//! 3. **`list`**: `si-view`'s port of the list's builder
//!    (`SI_graflistBuildFromElemDict`) is run on the result, with the three switches the
//!    loader holds at that point of a load. Only elements whose object is in the image
//!    (or that have none by design) are offered to it.

use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_hrc::image as hrc;
use si_mem::{Mem, OnFault, Ptr32};
use si_scene::calls::Recorder;
use si_scene::load::{self, Outcome, Scene};
use si_scene::object::Object;
use si_scene::world::{self, obj_of};
use si_types::dict::{ChapIds, Elem, ElemDict, ElemList, ELEM_VALID};
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};
use std::collections::BTreeMap;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

/// How far a scene is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Dict,
    Objects,
    List,
}

impl Stage {
    pub fn parse(s: &str) -> Option<Stage> {
        match s {
            "dict" => Some(Stage::Dict),
            "objects" => Some(Stage::Objects),
            "list" => Some(Stage::List),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Stage::Dict => "dict",
            Stage::Objects => "objects",
            Stage::List => "list",
        }
    }
}

/// An address of the image worth calling a routine on, for the generators: what it is and
/// a few numbers about it (see [`crate::dump`] for the lines these become).
#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: &'static str,
    pub addr: u32,
    pub more: Vec<String>,
}

/// A scene in its image.
pub struct World {
    /// The loader's state; `sc.mem` is the image.
    pub sc: Scene,
    pub stage: Stage,
    /// Whether the loader read the description.
    pub loaded: bool,
    /// What this crate did that is no port, and what the image lacks: one fact a line.
    pub notes: Vec<String>,
    pub entries: Vec<Entry>,
    /// Counts for the report: name, number.
    pub counts: BTreeMap<String, usize>,
}

/// The file system path a loader's path names.
fn os_path(bytes: &[u8]) -> std::path::PathBuf {
    std::ffi::OsString::from_vec(bytes.to_vec()).into()
}

/// Why `si-hrc`'s image readers cannot read a model file whole, if they cannot: a
/// geometry or a chunk whose reader is not on the image (it would be asked through the
/// readers' outside world, read nothing, and leave the stream where it was). The test is
/// the one `hrc-imagecheck` makes, on the value the loader's own reader gave.
fn unsupported(n: &si_hrc::Node) -> Option<&'static str> {
    let mut why = None;
    n.walk(0, &mut |_, x| {
        let reason = match x.kind {
            1 => Some("a patch"),
            2 => Some("a face"),
            6 => Some("a spline"),
            7 | 8 => Some("a metaball"),
            9 | 10 => Some("a NURBS curve or surface"),
            _ if x.ik.is_some() => Some("an inverse kinematics chunk"),
            _ if x.dynamics.is_some() || x.joint_friction.is_some() => Some("a dynamics chunk"),
            _ if x.envelop.is_some() => Some("an envelope"),
            _ if !x.clusters.is_empty() => Some("a cluster"),
            _ => None,
        };
        if why.is_none() {
            why = reason;
        }
    });
    why
}

/// The nodes of a hierarchy in the image in the order the loader numbers them: a node,
/// the nodes under its first son, then its next brother. Stops at `limit` nodes (a
/// hierarchy that is not a tree would not end).
fn walk_nodes(m: &Mem, root: Ptr32<Model>, limit: usize) -> Vec<Ptr32<Model>> {
    fn go(m: &Mem, mut n: Ptr32<Model>, out: &mut Vec<Ptr32<Model>>, limit: usize) {
        while !n.is_null() && out.len() < limit && m.is_mapped(n.addr(), 0x430) {
            out.push(n);
            let son = m[n].son;
            go(m, son, out, limit);
            n = m[n].brother;
        }
    }
    let mut out = Vec::new();
    go(m, root, &mut out, limit);
    out
}

/// Runs one of `si-hrc`'s readers so that it stops at its first access outside memory,
/// as the original would be killed there. Answers what the reader answered, or `None`
/// when it stopped (the fault is then cleared: the note says what happened).
fn guarded(m: &mut Mem, f: impl FnOnce(&mut Mem) -> u32) -> Option<u32> {
    m.on_fault(OnFault::Stop);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(m)));
    m.on_fault(OnFault::Record);
    match ran {
        Ok(v) if m.fault().is_none() => Some(v),
        _ => {
            m.clear_fault();
            None
        }
    }
}

/// Loads `name` (the scene's file name without `.dsc`) of the database at `db` and builds
/// its image up to `stage`.
pub fn build(db: &Path, name: &[u8], rsrc: &[u8], stage: Stage) -> Result<World, String> {
    let fs = RealFs;
    let mut sc = Scene::start(rsrc).map_err(|e| format!("Relations.rsrc: {e}"))?;
    let mut info = DbInfo::new();
    info.name = db.file_name().map(|n| n.as_bytes().to_vec());
    info.path = Some(db.as_os_str().as_bytes().to_vec());
    let mut log = Log::default();
    si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
    let mut un = Recorder::default();
    let loaded = load::load_scene(&mut sc, &fs, &mut info, name, &mut un);
    let mut w = World { sc, stage, loaded, notes: Vec::new(), entries: Vec::new(), counts: BTreeMap::new() };
    let faults = w.sc.mem.fault_count();
    if faults != 0 {
        w.notes.push(format!("the load recorded {faults} accesses outside memory, the first {:?}", w.sc.mem.fault()));
        w.sc.mem.clear_fault();
    }
    if !w.sc.mem.arena_blocks().is_empty() {
        w.notes.push(format!("the load placed {} buffers in the arena, which is not written out", w.sc.mem.arena_blocks().len()));
    }
    if stage >= Stage::Objects {
        objects(&mut w);
    }
    if stage >= Stage::List {
        list(&mut w);
    }
    entries(&mut w);
    Ok(w)
}

/// Stage 2: see the head of this file.
fn objects(w: &mut World) {
    let mut m = std::mem::take(&mut w.sc.mem);
    hrc::init(&mut m);
    let ed = world::current_dict(&m);
    let cid: ChapIds = world::cid(&m);
    let mut o = hrc::MemOutside::default();
    // the address of each record read, by the loader's handle of the object
    let mut at: BTreeMap<u32, u32> = BTreeMap::new();
    let count = |w: &mut World, what: &str| *w.counts.entry(what.to_string()).or_default() += 1;
    // a file's name as the readers want it: a string of the program's, here a heap block
    // that is freed again (the arena stays free for the buffers of a case)
    let scratch_name = |m: &mut Mem, text: &[u8]| -> u32 {
        let p = m.malloc(text.len() as u32 + 1);
        m.put_cstr(p, text);
        p
    };
    let report = w.sc.report.clone();
    for r in &report {
        let Outcome::Read { path, .. } = &r.outcome else { continue };
        let Some(e) = world::slot(&m, ed, si_scene::dict::ElemId { chap: r.chap, index: r.index }) else { continue };
        let handle = obj_of(m[e].data);
        if handle == 0 || at.contains_key(&handle) {
            continue;
        }
        let kind = if r.chap == cid.model {
            "model"
        } else if r.chap == cid.mat {
            "material"
        } else if r.chap == cid.t2d {
            "2D texture"
        } else if r.chap == cid.t3d {
            "3D texture"
        } else {
            continue;
        };
        let Ok(data) = std::fs::read(os_path(path)) else {
            count(w, &format!("{kind} files that could not be read again"));
            continue;
        };
        let record = match kind {
            "model" => {
                let Some(Object::ModelNode { tree, .. }) = w.sc.objects.get(handle) else { continue };
                let Some(t) = w.sc.models.get(*tree) else { continue };
                if let Some(why) = unsupported(&t.root) {
                    count(w, &format!("model files not read into the image: the hierarchy has {why}, whose reader is not on the image"));
                    continue;
                }
                let nodes = t.names.len();
                let read = guarded(&mut m, |m| {
                    let file = o.files.open_bytes(b"/element.hrc", data.clone());
                    hrc::node::fdmodel_binary_read(m, &mut o, file, 0, 0)
                });
                match read {
                    Some(root) if root != 0 => {
                        let placed = walk_nodes(&m, Ptr32::new(root), nodes + 1);
                        if placed.len() != nodes {
                            count(w, "model files whose hierarchy in the image has another number of nodes than the loader's");
                            continue;
                        }
                        // every node of the tree, by the handle of the object that stands for it
                        let t = w.sc.models.get(*tree).unwrap();
                        for (k, node) in placed.iter().enumerate() {
                            if t.handles[k] != 0 {
                                at.insert(t.handles[k], node.addr());
                            }
                            // what the loader stores in the node when an element claims it
                            if t.elem_ids[k] >= 0 {
                                m[*node].elem_id = t.elem_ids[k];
                            }
                        }
                        *w.counts.entry("model nodes in the image".into()).or_default() += placed.len();
                        root
                    }
                    Some(_) => {
                        count(w, "model files the image reader refused");
                        continue;
                    }
                    None => {
                        count(w, "model files on which the image reader went outside memory");
                        continue;
                    }
                }
            }
            _ => {
                let suffix: &[u8] = match kind {
                    "material" => b".mtr",
                    "2D texture" => b".txt",
                    _ => b".t3D",
                };
                o.files.add(&[b"/element".as_slice(), suffix].concat(), data);
                let name = scratch_name(&mut m, b"/element");
                let read = guarded(&mut m, |m| match kind {
                    "material" => hrc::matio::material_read_binary_file(m, &mut o, name).addr(),
                    "2D texture" => hrc::matio::texture_read_binary_file(m, &mut o, name).addr(),
                    _ => hrc::matio::texture3d_read_binary_file(m, &mut o, name).addr(),
                });
                m.free(name);
                match read {
                    Some(p) if p != 0 => p,
                    Some(_) => {
                        count(w, &format!("{kind} files the image reader refused"));
                        continue;
                    }
                    None => {
                        count(w, &format!("{kind} files on which the image reader went outside memory"));
                        continue;
                    }
                }
            }
        };
        at.entry(handle).or_insert(record);
        count(w, &format!("{kind} files read into the image"));
    }
    // the elements point at the records
    let (mut pointed, mut left) = (0usize, BTreeMap::<Vec<u8>, usize>::new());
    for chap in 0..m[ed].nb_lists {
        let chapter: Ptr32<ElemList> = m[ed].lists.add(chap);
        for i in 0..m[chapter].nb_elems {
            let e: Ptr32<Elem> = m[chapter].elems.add(i);
            let handle = obj_of(m[e].data);
            if handle == 0 {
                continue;
            }
            match at.get(&handle) {
                Some(&a) => {
                    m[e].data = Ptr32::new(a);
                    pointed += 1;
                }
                None => *left.entry(m.cstr(m[chapter].name.addr())).or_default() += 1,
            }
        }
    }
    w.counts.insert("elements whose object is a record of the image".into(), pointed);
    for (chapter, n) in &left {
        w.notes.push(format!("missing: {n} elements of {} keep the loader's handle (their object is not in the image)", String::from_utf8_lossy(chapter)));
    }
    let mut outside: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (routine, _) in &o.calls {
        *outside.entry(routine.name).or_default() += 1;
    }
    for (name, n) in &outside {
        w.notes.push(format!("missing: {n} calls of {name} by the file readers answered 0 (the routine is not on the image)"));
    }
    w.notes.push("glue: the model, material and texture files were read a second time, by si-hrc's image readers, after the load and not at the point of the load where the original reads them: the order of the heap's blocks is not the original's".into());
    w.notes.push("glue: an element's object is the record read from its file; a model node's element number (+0x354) is the loader's".into());
    w.notes.push("missing: the ties the loader makes between model nodes and the scene's material and texture elements (relations 300, 400, 401, 500, 501): a node keeps the materials its own file gave it".into());
    w.notes.push("missing: SI_modelSetState from a root's SRT item; the nodes' global matrices (SI_modelUpdateSonState is not called)".into());
    if let Some(f) = m.fault() {
        w.notes.push(format!("pointing the elements at their records went outside memory: {f:?}"));
        m.clear_fault();
    }
    w.sc.mem = m;
}

/// Stage 3: see the head of this file.
fn list(w: &mut World) {
    use si_view::globals as vg;
    let mut m = std::mem::take(&mut w.sc.mem);
    // the initial values of the globals the list's routines read, but for the three
    // pointers the loader's image already holds and the loader's own switch
    let kept = [si_dict::globals::SI_PROC_DATA.addr(), vg::SI_GRAF_LISTS.addr(), si_dict::globals::SI_ELEM_DICTS.addr(), si_scene::globals::SI_ADD_IN_DICTIONARY.addr()];
    si_mem::init_globals(&mut m, &vg::ALL.iter().copied().filter(|g| !kept.contains(&g.addr)).collect::<Vec<_>>());
    // the display's two words behind a new node's wire colour: the loader's own assumption
    m[vg::SI_SELECT7WM] = w.sc.wire.select7_mask as u16;
    m[vg::SI_COLOR1] = w.sc.wire.color1 as u16;
    let ed = world::current_dict(&m);
    let cid: ChapIds = world::cid(&m);
    // which elements are offered to the builder: in use, and with an object that is null
    // or memory. The stand-in nodes of the loader's own graph are forgotten.
    let offered = [cid.model, cid.dsc, cid.mat, cid.t2d, cid.t3d];
    let mut marked: Vec<Ptr32<Elem>> = Vec::new();
    let mut held_back = 0usize;
    for chap in 0..m[ed].nb_lists {
        let chapter: Ptr32<ElemList> = m[ed].lists.add(chap);
        for i in 0..m[chapter].nb_elems {
            let e: Ptr32<Elem> = m[chapter].elems.add(i);
            if world::graf_of(m[e].aux_data) != 0 {
                m[e].aux_data = Ptr32::NULL;
            }
            if m[e].status != ELEM_VALID {
                continue;
            }
            let data = m[e].data.addr();
            let in_image = data != 0 && m.is_mapped(data, 4);
            let wanted = offered.contains(&chap) && (in_image || chap == cid.dsc);
            if wanted {
                if m[e].new_flag != 1 {
                    marked.push(e);
                }
                m[e].new_flag = 1;
            } else if m[e].new_flag == 1 {
                m[e].new_flag = 0;
                held_back += 1;
            } else {
                held_back += (data != 0) as usize;
            }
        }
    }
    let schem = vg::SI_GLB_SCHEM.addr();
    let add = m[si_scene::globals::SI_ADD_IN_DICTIONARY];
    m[si_scene::globals::SI_ADD_IN_DICTIONARY] = 0;
    m.set_i16(schem + vg::GLB_SCHEM_3C, 1);
    m.set_i16(schem + si_scene::globals::GLB_SCHEM_4E, 1);
    let mut view = si_view::Wired::new(ViewWorld::default());
    si_view::db2scn::graflist_build_from_elem_dict(&mut m, &mut view, ed, si_dict::globals::SI_RD.cast(), si_dict::globals::SI_CID.cast(), 1);
    for e in marked {
        m[e].new_flag = 0;
    }
    m[si_scene::globals::SI_ADD_IN_DICTIONARY] = add;
    m.set_i16(schem + vg::GLB_SCHEM_3C, 0);
    m.set_i16(schem + si_scene::globals::GLB_SCHEM_4E, 0);
    for (name, n) in &view.inner.unanswered {
        w.notes.push(format!("missing: {n} calls of {name} by the list's builder answered 0 (the routine is not wired in)"));
    }
    w.notes.push(format!(
        "glue: the list's builder had two routines of the model tree answered by si-model's ports on the image: SI_modelNextNode ({} calls) by its port, SI_modelNbSubModels ({} calls) by counting with the port of SI_modelNextSon (its own image form is in si-rensetup, which this crate does not depend on); and SI_edAddMaterial, SI_edAddTexture2D, SI_edAddTexture3D ({} calls) answered 1, as they answer while SI_addInDictionary is 0 (finding 0023, section 18)",
        view.inner.next_node, view.inner.nb_sub, view.inner.ed_add
    ));
    w.notes.push("glue: SI_graflistBuildFromElemDict (si-view's port) was called after the load with SI_addInDictionary 0, the two words of SI_glbSchem (+0x3c, +0x4e) 1 and the new-flag 1 on the elements offered, all put back afterwards; SI_select7WM and SI_color1 are the loader's assumption about the display".into());
    w.notes.push(format!("missing: {held_back} elements were not offered to the list's builder (cameras, lights, animation, named selections, waves and every element whose object is not in the image): they have no node in the list"));
    if let Some(f) = m.fault() {
        w.notes.push(format!("building the list went outside memory {} times, the first {f:?}", m.fault_count()));
        m.clear_fault();
    }
    w.sc.mem = m;
}

/// What the list's builder calls outside `si-view`: the two walks of the model tree it
/// needs to bring a hierarchy into the list are answered from `si-model`'s ports on the
/// image; every other routine answers 0 and is counted.
#[derive(Default)]
struct ViewWorld {
    unanswered: BTreeMap<&'static str, usize>,
    next_node: usize,
    nb_sub: usize,
    ed_add: usize,
}

impl si_view::Outside for ViewWorld {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        use si_model::model_image::tree;
        let node: Ptr32<Model> = Ptr32::new(args.first().copied().unwrap_or(0));
        match routine.addr {
            // SI_modelNextNode (0x08478340)
            0x0847_8340 => {
                self.next_node += 1;
                tree::next_node(m, node).addr()
            }
            // SI_modelNbSubModels (0x0846d5ec): the nodes below `node`, counted with
            // SI_modelNextSon's port; answered as the original answers it, in 16 bits
            0x0846_d5ec => {
                self.nb_sub += 1;
                let (mut n, mut at) = (0u32, tree::next_son(m, node, Ptr32::NULL));
                while !at.is_null() && n < 0x7fff && m.fault().is_none() {
                    n += 1;
                    at = tree::next_son(m, node, at);
                }
                n as i16 as i32 as u32
            }
            // SI_edAddMaterial (0x0810203c), SI_edAddTexture2D (0x08102314),
            // SI_edAddTexture3D (0x081027e0): each tests SI_addInDictionary first and
            // answers 1 without doing anything while it is 0, which it is during this call
            // (as si-rensetup's world answers them, finding 0023 section 18)
            0x0810_203c | 0x0810_2314 | 0x0810_27e0 if m[si_scene::globals::SI_ADD_IN_DICTIONARY] == 0 => {
                self.ed_add += 1;
                1
            }
            _ => {
                *self.unanswered.entry(routine.name).or_default() += 1;
                0
            }
        }
    }
}

/// The table of addresses for the generators.
fn entries(w: &mut World) {
    let m = &w.sc.mem;
    let ed: Ptr32<ElemDict> = world::current_dict(m);
    let mut out = vec![Entry { kind: "dict", addr: ed.addr(), more: vec![format!("{}", m[ed].nb_lists)] }];
    let cid: ChapIds = world::cid(m);
    let mut models: Vec<u32> = Vec::new();
    for chap in 0..m[ed].nb_lists.clamp(0, 4096) {
        let chapter: Ptr32<ElemList> = m[ed].lists.add(chap);
        let name = String::from_utf8_lossy(&m.cstr(m[chapter].name.addr())).into_owned();
        out.push(Entry { kind: "chapter", addr: chapter.addr(), more: vec![format!("{chap}"), format!("{}", m[chapter].nb_elems), name] });
        for i in 0..m[chapter].nb_elems.clamp(0, 1 << 20) {
            let e: Ptr32<Elem> = m[chapter].elems.add(i);
            let (data, aux) = (m[e].data.addr(), m[e].aux_data.addr());
            out.push(Entry { kind: "elem", addr: e.addr(), more: vec![format!("{chap}"), format!("{i}"), format!("{}", m[e].status), format!("{data:08x}"), format!("{aux:08x}")] });
            if chap == cid.model && data != 0 && m.is_mapped(data, 0x430) {
                models.push(data);
            }
        }
    }
    for a in &models {
        let node: Ptr32<Model> = Ptr32::new(*a);
        out.push(Entry { kind: "model", addr: *a, more: vec![format!("{:08x}", m[node].father.addr()), format!("{}", m[node].kind), format!("{}", m[node].nb_defs)] });
    }
    if w.stage >= Stage::List {
        let lists = m[si_view::globals::SI_GRAF_LISTS];
        if !lists.is_null() {
            let list: Ptr32<GrafList> = si_view::graflist::scene_list(m);
            out.push(Entry { kind: "graflist", addr: list.addr(), more: Vec::new() });
            let mut n = m[list].first;
            let mut seen = 0;
            while !n.is_null() && seen < 1 << 20 && m.is_mapped(n.addr(), 0x24) {
                let g: GrafNode = m.read(n);
                out.push(Entry { kind: "grafnode", addr: n.addr(), more: vec![format!("{}", g.kind), format!("{:08x}", g.data.addr()), format!("{}", g.nb_sub)] });
                n = g.next;
                seen += 1;
            }
            w.counts.insert("nodes in the graph list".into(), seen);
        }
    }
    w.counts.insert("model nodes that elements point at".into(), models.len());
    w.entries = out;
}
