// SPDX-License-Identifier: MIT
//! "Get Scene": from a scene's name to a complete element dictionary **in the memory
//! image** (`dbloadsave.c`, `dictin_scn.c`, `dictin.c`, `physicalio.c`).
//!
//! Every function here takes the image (`si_mem::Mem`), the scene's side tables
//! ([`Scene`]: what is not in the image yet) and the arguments the original takes: the
//! dictionary as `Ptr32<ElemDict>`, an element as `Ptr32<Elem>` with its index. The
//! dictionary routines are `si-dict`'s; name records are made and changed by
//! [`crate::dbnode`]; the original's allocation calls are made in its order. An element's
//! object is still a value of the package that reads its file, held in [`Scene::objects`]
//! under an address of the handle range ([`crate::world`]).
//!
//! The sequence, in the original's order (finding 0018 has it as numbered steps):
//!
//! 1. [`db_load_scene`]: the scene becomes an element of the SCENES chapter.
//! 2. [`expand_descriptions`]: each new scene's description file is read into the dictionary
//!    ([`expand_dsc_file`]): elements, sub-scenes, relations, display state.
//! 3. [`read_elements`]: the element files are found and read, chapter by chapter, MODELS
//!    first.
//! 4. The rest of [`load_elements_from_dict`]: hierarchy information, the animation files
//!    expanded into one element per curve, and then the part of the program that is not
//!    ported yet (the scene graph, custom effects, the schematic view …), recorded as calls
//!    on [`Unported`]; the clean-up of the dictionary; the topological sort.
//!
//! The scene description is parsed by `si-ascii` (every token consumed as the original
//! consumes it) and applied here to the dictionary, call for call as the original's reader
//! makes them ([`apply_elements`], [`apply_relations`], [`apply_environment`]: the
//! dictionary's side of the five `dbRead…` routines, which are not cited).
//!
//! [`load_scene`] is the entry for callers that are not in place: it runs the load on the
//! scene's image and then copies the dictionaries into the stores the saver and
//! `si-rensetup` still read.

use crate::calls::{Arg, Channel, Message, Unported};
use crate::dbnode;
use crate::dict::{Arena, Dict, ElemId, Program};
use crate::globals::{GLB_SCHEM_48, GLB_SCHEM_4A, GLB_SCHEM_4E, LOADING, SI_ADD_IN_DICTIONARY, SI_DB_WORK_FNAME, SI_DB_WORK_FNAME_EXT, SI_DEFAULT_GMATTE, SI_GLB_ANIM, SI_GLB_SCHEM, SI_STANDALONE_RENDERER};
use crate::object::{ModelTree, Object, ShapeOfModel};
use crate::world::{self, cid, current_dict, elem, find_list_by_name, find_related, full_name, graf_ptr, list, node_text, slot, with_text, Out};
use si_ascii::dsc::{self, Description, PlayControl, Recorded, WireMap};
use si_ascii::reldata::{self, RelData};
use si_db::{DbInfo, FileSystem, Log, Session};
use si_dict::globals::{GLB_MANIP_36, SI_CID, SI_GLB_MANIP, SI_RD, SI_USE_ELEM_LIST_SORT_TBL};
use si_dict::{elemdict, elemmisc, elemrel, relations, reldict, toposort};
use si_elemio::io::Reader;
use si_elemio::{anim, camera, cluster, lattice, light, shape};
use si_mem::{field, CStr32, Mem, Opaque, Ptr32};
use si_model::model_image::Outside;
use si_types::dict::{DbListNode, Elem, ElemDict, ElemList, ELEM_DELETED, ELEM_VALID};

/// Relation ids the loader names.
pub const REL_PARENT: i32 = 0x6e; // 110
pub const REL_CAM_INT: i32 = 0x456; // 1110
pub const REL_LIG_INT: i32 = 0x83e; // 2110

/// What became of one element a scene description names, for the checks. Not part of the
/// original: it keeps no such record.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Its file was found and its reader took it. `fallback`: found under the unversioned
    /// name (`prefix-name.suffix`) because the named version is not there; `sub_dir`: found
    /// in another sub-directory of the chapter than the element's own.
    Read { path: Vec<u8>, fallback: bool, sub_dir: bool, complete: bool },
    /// Found, but its reader belongs to a package that is not ported: the file was located
    /// and the call recorded.
    Located { path: Vec<u8>, reader: &'static str },
    /// A node of a root's file: claimed by name from the hierarchy.
    Node,
    /// A node that should be in a root's file and is not ("A model node named … was
    /// expected").
    NodeMissing,
    /// A camera or light that is not a root: it gets its object from its root.
    PartOfRoot,
    /// No file under any of the names tried.
    Missing { tried: Vec<u8> },
    /// A file whose header or content the reader refused.
    Invalid { path: Vec<u8> },
    /// Read onto a model that the relations do not give (shapes, clusters, lattices).
    NoModel,
    /// A chapter the loader does not read from disk (pictures, named selections).
    NotRead,
}

/// One line of the report.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemReport {
    pub chap: i32,
    pub index: i32,
    pub name: Vec<u8>,
    pub outcome: Outcome,
}

/// What the dictionaries ask of the rest of the program during a load and the scene
/// answers for it.
#[derive(Clone, Copy, Debug, Default)]
pub struct OutsideAnswers {
    /// What `time(NULL)` returns: the creation time of new name records.
    pub now: i32,
    /// What `SI_grafnodeDefColor` (0x0832bff4) returns, `SI_select7WM + SI_color1`: the
    /// wire colour of a new element. Both globals come from the display at start-up.
    pub def_color: i32,
    /// `SI_getLoadOptim()` (0x080d94b0): the environment variable `SI_LOAD_OPTIM` was set.
    pub load_optim: bool,
}

/// The program's state as far as loading and saving a scene go: the memory image with the
/// dictionaries, and what is not in the image yet.
pub struct Scene {
    /// The image: the scene dictionary (`SI_elemDicts[*SI_procData]`,
    /// [`crate::world::current_dict`]) with its chapters, elements, relation lists and
    /// name records, the relation dictionary `SI_RD`, the chapter indices `SI_cid`, the
    /// loader's globals. A load builds the scene here.
    pub mem: Mem,
    /// What the dictionaries ask of the outside ([`crate::world::Out`]).
    pub answers: OutsideAnswers,
    /// The records of relation data in the image's heap that stand for a value of
    /// [`Scene::rel_data`]: record address, handle ([`crate::world::rel_data_record`]).
    pub rel_records: std::collections::BTreeMap<u32, u32>,
    // adapter: not in place
    /// Relation data the saver of the stores writes as the description held it, by entry:
    /// the fittings' the load gave to fitting records ([`crate::world::fitting_data_before`])
    /// and the expressions' the regeneration replaced.
    pub fitting_data: std::collections::BTreeMap<crate::world::FitKey, u32>,
    /// `SI_elemdictDisposeRelationData(relation)`: the relation's id and its data, for
    /// each entry whose data the dictionaries disposed of.
    pub disposed_rel_data: Vec<(i32, Ptr32<Opaque>)>,
    // adapter: not in place
    /// The dictionaries as stores, for the code of this crate that is not on the image
    /// (the saver with the description writer's sort) and for `si-rensetup`:
    /// copied from the image when a load ends ([`Scene::sync_stores`]). The image is the
    /// scene; these are a copy, and what the saver changes in them does not reach the
    /// image.
    pub dict: Dict,
    // adapter: not in place
    /// The globals around the dictionary as the store form has them, copied likewise.
    /// Four of its values are read when a load starts ([`Scene::push_settings`]): the
    /// time, the default wire colour, the load optimisation and the back-up switch.
    pub program: Program,
    /// The objects elements point at.
    pub objects: Arena<Object>,
    /// The model files in memory.
    pub models: Arena<ModelTree>,
    /// Relation data (`SI_ARelation` +0x18).
    pub rel_data: Arena<RelData>,
    /// The nine numbers of a root model's `SRT` item (`__SI_Elem` +0x3c).
    pub srt: Arena<[f32; 9]>,
    /// What shape files put into their models.
    pub shapes: Vec<ShapeOfModel>,
    /// The scene's camera (`SI_procData` + 8): a handle into `objects`.
    pub camera: u32,
    /// The display globals behind `WIRECOL`.
    pub wire: WireMap,
    /// The play control globals (`SI_frameStart` …).
    pub play: PlayControl,
    /// The version of the description read last.
    pub dsc_version: f32,
    /// What the original printed.
    pub messages: Vec<Message>,
    /// The statics of the element readers, which persist from file to file.
    pub statics: si_elemio::Statics,
    /// The table of `SI_cusIconSetSerialId`: icon element, serial number.
    pub icon_serials: Vec<(i32, i32)>,
    /// `SI_standaloneRenderer`.
    pub standalone_renderer: bool,
    /// `SI_defaultGMatte`.
    pub default_gmatte: bool,
    /// The environment variable `SI_XSI_LEGACYLOADER` is set.
    pub legacy_loader: bool,
    /// The hierarchical order of the ANIMATION chapter (`ElemList` +0x10, +0x14).
    pub anim_order: Option<(Vec<i32>, Vec<i32>)>,
    /// The hierarchical order of the MODELS chapter (`SI_elemdictSortHierElements`).
    pub model_order: Option<(Vec<i32>, Vec<i32>)>,
    /// The update list (`SI_ElemDict` +0x0c) as the topological sort left it.
    pub upd_list: crate::updlist::UpdListStore,
    /// Stand-in for the scene graph, which is not ported: the scaling, rotation and
    /// translation the loader hands to each root model (`SI_modelSetState`) from the `SRT`
    /// item. The writer prints the model's state from here.
    pub root_state: std::collections::BTreeMap<ElemId, [f32; 9]>,
    /// The nodes of the scene graph that elements point at (`__SI_Elem` +0x08). The graph
    /// itself (`graflist.c`) is not ported: these are the nodes [`graflist_build_from_elem_dict`]
    /// makes or finds, kept so that the writer can print their flag word (`MPRFLG`).
    pub graf: Arena<si_types::scene::GrafNode>,
    /// Two stand-ins for packages that are not ported, off unless a check turns them on.
    pub assume: Assume,
    /// The relation lines the description reader drops by design (120, 121, 207, 208), as
    /// read: first chapter, first element, second chapter, second element, relation id,
    /// rule number, active.
    pub dropped_lines: Vec<(i32, i32, i32, i32, i32, i32, bool)>,
    /// For the checks: what became of each element of the last load.
    pub report: Vec<ElemReport>,
    /// For the checks: relation lines the dictionary refused.
    pub refused_relations: Vec<(i32, i32, i32)>,
    /// For the checks: how many elements `SI_dbCheckDuplicateNames` renamed.
    pub renamed_duplicates: usize,
    /// What materials point at once tied to texture elements, by the material's handle.
    pub mat_links: std::collections::BTreeMap<u32, crate::object::MaterialLinks>,
    /// The link of each 2D texture record to the next of its list (`__SI_LclMap` +0x16c),
    /// by handle; a record without an entry ends its list.
    pub tex_next: std::collections::BTreeMap<u32, u32>,
    /// What cluster files put into their models.
    pub clusters: Vec<crate::object::ClustersOfModel>,
    /// What lattice files put into their models.
    pub lattices: Vec<crate::object::LatticesOfModel>,
    /// The three setups as last read (`SI_elemdictReadSetup` and its two siblings fill
    /// program globals; these are those globals as far as `si-ascii` keeps them).
    pub setup: Option<Box<si_ascii::setup::Setup>>,
    pub channel_setup: Option<Box<si_ascii::chn::ChannelSetup>>,
    pub flock_setup: Option<Box<si_ascii::flk::Flock>>,
    /// The databases elements were read from or saved to: a name record's first word
    /// (`SI_DbListNode` +0x00) is a pointer to the database record's path string, kept here
    /// as the position in this list plus one.
    pub db_paths: Vec<Vec<u8>>,
    /// The state of a save in progress and the save preferences ([`crate::dictout`]).
    pub saving: crate::dictout::SaveState,
    /// The elements' contents in the image: which record stands for which value above,
    /// and what the readers on the image did ([`crate::image`]).
    pub contents: crate::image::Contents,
}

/// Stand-ins for two things other packages do during a load, so that a load can be checked
/// against a save before those packages exist. Both are off by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assume {
    /// `SI_cusPrepareEffect` finds the custom effect a relation names (its description file
    /// is installed) and so the relation dictionary has a rule for it. With this on, a rule
    /// with the two names is added when there is none: relation 20000, rank 50 (the rank of
    /// every custom effect line, finding 0006) and no limit on the fan-out.
    pub effects_installed: bool,
    /// The relations the description reader drops (120 `InvKin`, 121 `JntModel`, 207
    /// `FlexEnv`, 208 `EnvShp`) are put back after the element files are read by the code
    /// that owns them (`SI_envelopRelationSet` and the inverse kinematics code), from what
    /// the model files hold. With this on, they are put back from the description's own
    /// lines instead, just before `SI_envelopRelationSet` (`si-deform`'s port) runs; it then
    /// adds a skin's 207 only where the description had none.
    pub model_relations_from_description: bool,
}

/// The wire colour globals of a display with more than 8 bit planes on which the default
/// pair is (0, 7): `SI_color1` 0, the unselected colours in the low six bits, the selected
/// ones above. The real values come from the display at start-up and are not in the binary
/// (finding 0006); with these the two `WIRECOL` numbers convert both ways.
pub const WIRE_DEFAULT: WireMap = WireMap { bit_planes: 24, color1: 0, unselect_mask: 0x3f, select7_mask: 0x1c0 };

impl Scene {
    /// The state after start-up ([`crate::init::start_image`]): the default scene, the
    /// relation dictionary read from `Relations.rsrc`, in the image.
    pub fn start(rsrc: &[u8]) -> Result<Scene, si_ascii::srel::Error> {
        let mut sc = Scene::empty();
        let mut m = std::mem::take(&mut sc.mem);
        let mut un = crate::calls::Recorder::default();
        let started = crate::init::start_image(&mut m, &mut Out { sc: &mut sc, un: &mut un }, rsrc);
        if started.is_ok() {
            crate::image::start_graph(&mut m, &mut sc, &mut un);
        }
        sc.mem = m;
        started?;
        sc.sync_stores();
        Ok(sc)
    }

    // adapter: not in place
    /// A scene around dictionaries built in stores (the unit tests do that): they are
    /// copied into a fresh image ([`crate::world::to_image`]).
    pub fn from_parts(dict: Dict, mut program: Program) -> Scene {
        let mut sc = Scene::empty();
        program.def_color = sc.wire.default_wire();
        world::to_image(&mut sc.mem, &dict, &program);
        let mut m = std::mem::take(&mut sc.mem);
        crate::image::start_graph(&mut m, &mut sc, &mut crate::calls::Recorder::default());
        sc.mem = m;
        sc.dict = dict;
        sc.program = program;
        sc
    }

    /// A scene with a fresh image and nothing in it.
    fn empty() -> Scene {
        let wire = WIRE_DEFAULT;
        let mut objects = Arena::new();
        let camera = objects.alloc(Object::Camera(Box::new(camera::Camera::perspective_default())));
        let mut program = Program::new();
        program.def_color = wire.default_wire();
        Scene {
            mem: world::new_image(),
            answers: OutsideAnswers { now: 0, def_color: wire.default_wire(), load_optim: false },
            rel_records: Default::default(),
            fitting_data: Default::default(),
            disposed_rel_data: Vec::new(),
            dict: Dict::new(),
            program,
            objects,
            models: Arena::new(),
            rel_data: Arena::new(),
            srt: Arena::new(),
            shapes: Vec::new(),
            camera,
            wire,
            play: PlayControl { start: 0, end: 0, current: 0, step: 0, step_adaptive: 0, pause: 0, rate: 0.0 },
            dsc_version: -1.0,
            messages: Vec::new(),
            statics: si_elemio::Statics::default(),
            icon_serials: Vec::new(),
            standalone_renderer: false,
            default_gmatte: false,
            legacy_loader: false,
            anim_order: None,
            model_order: None,
            upd_list: crate::updlist::UpdListStore::default(),
            root_state: Default::default(),
            graf: Arena::new(),
            assume: Assume::default(),
            dropped_lines: Vec::new(),
            report: Vec::new(),
            refused_relations: Vec::new(),
            renamed_duplicates: 0,
            mat_links: Default::default(),
            tex_next: Default::default(),
            clusters: Vec::new(),
            lattices: Vec::new(),
            setup: None,
            channel_setup: None,
            flock_setup: None,
            db_paths: Vec::new(),
            saving: crate::dictout::SaveState::default(),
            contents: crate::image::Contents::default(),
        }
    }

    // adapter: not in place
    /// What a caller that is not in place set in the store form of the globals, written
    /// where the image form reads it: the time, the default wire colour and the load
    /// optimisation as the outside's answers; the back-up switch (`SI_glbManip` +0x36),
    /// `SI_standaloneRenderer` and `SI_defaultGMatte` in the image.
    pub fn push_settings(&mut self, m: &mut Mem) {
        self.answers = OutsideAnswers { now: self.program.now, def_color: self.program.def_color, load_optim: self.program.load_optim };
        m.set_i16(SI_GLB_MANIP.addr() + GLB_MANIP_36, self.program.backup_enabled as i16);
        m[SI_STANDALONE_RENDERER] = self.standalone_renderer as i16;
        m[SI_DEFAULT_GMATTE] = self.default_gmatte as i16;
    }

    // adapter: not in place
    /// Copies the dictionaries of the image into the stores ([`crate::world::to_stores`]).
    pub fn sync_stores(&mut self) {
        let ed = current_dict(&self.mem);
        let s = world::to_stores(&self.mem, ed, self);
        self.dict = s.dict;
        self.program = s.program;
        self.upd_list = s.upd_list;
        self.model_order = s.model_order;
        self.anim_order = s.anim_order;
        self.srt = s.srt;
    }

    /// The mark a name record gets for a database (`SI_DbListNode` +0x00): the original
    /// stores the pointer to the database record's path. The database record is a value
    /// of `si-db` and has no address: the mark is the path's position in
    /// [`Scene::db_paths`] plus one. Two records with the same path are one database here.
    pub fn db_mark(&mut self, info: &DbInfo) -> i32 {
        let path = info.path.clone().unwrap_or_default();
        match self.db_paths.iter().position(|p| *p == path) {
            Some(i) => i as i32 + 1,
            None => {
                self.db_paths.push(path);
                self.db_paths.len() as i32
            }
        }
    }

    fn status(&mut self, level: i32, text: Vec<u8>) {
        self.messages.push(Message { channel: Channel::Status(level), text });
    }

    /// The object an element of the stores points at.
    pub fn object_of(&self, id: ElemId) -> Option<&Object> {
        self.objects.get(self.dict.slot(id)?.rec.data.0)
    }
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `%s` of a string of the image: its text, or `(null)` for a null pointer.
fn or_null(m: &Mem, p: CStr32) -> Vec<u8> {
    dbnode::text(m, p).unwrap_or_else(|| si_db::NULL_TEXT.to_vec())
}

/// For the checks: notes what became of an element, under its full name.
fn report(m: &Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, id: ElemId, outcome: Outcome) {
    let name = full_name(m, ed, id).unwrap_or_default();
    sc.report.push(ElemReport { chap: id.chap, index: id.index, name, outcome });
}

/// A path as the file search leaves it in one of its two work buffers (0x400 bytes).
fn set_work_name(m: &mut Mem, buffer: Ptr32<[u8; 0x400]>, path: &[u8]) {
    // Where Rust must differ: the original prints a longer path over what follows the
    // buffer; here it is cut.
    m.put_cstr(buffer.addr(), &path[..path.len().min(0x3ff)]);
}

// adapter: not in place
/// Runs the load of a scene on the scene's image and copies the dictionaries into the
/// stores afterwards: [`db_load_scene`] for callers that hold a [`Scene`] and work on its
/// stores (the saver's checks, `si-rensetup`). `name` is the scene's file name without
/// its suffix (`prefix-name.version-subst`).
pub fn load_scene(sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, name: &[u8], un: &mut dyn Unported) -> bool {
    let mut m = std::mem::take(&mut sc.mem);
    sc.push_settings(&mut m);
    let ed = current_dict(&m);
    let ok = db_load_scene(&mut m, sc, fs, info, ed, name, un);
    sc.mem = m;
    sc.sync_stores();
    ok
}

// adapter: not in place
/// [`load_elements_from_dict`] on the scene's image, for callers that work on the stores.
pub fn load_elements(sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, with_descriptions: bool, latest: bool, untold: bool, un: &mut dyn Unported) -> bool {
    let mut m = std::mem::take(&mut sc.mem);
    sc.push_settings(&mut m);
    let ed = current_dict(&m);
    let ok = load_elements_from_dict(&mut m, sc, fs, info, ed, with_descriptions, latest, untold, un);
    sc.mem = m;
    sc.sync_stores();
    ok
}

/// Loads a scene of a database into the dictionary [soft 0x080da548 SI_dbLoadScene].
///
/// `name` is the scene's file name without its suffix (`prefix-name.version-subst`). The
/// default scene makes room (`SI_elemdictCheckEmptyDescription`); the scene becomes an
/// element of SCENES with new-flag 1, which is what marks it for [`expand_descriptions`];
/// then everything is [`load_elements_from_dict`], with descriptions, without asking for
/// the latest versions. Two words of the schematic's globals frame the load: +0x48 is
/// cleared and restored from +0x4a at the end, +0x3c ("a scene is being loaded") is set.
pub fn db_load_scene(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, name: &[u8], un: &mut dyn Unported) -> bool {
    elemmisc::check_empty_description(m, &mut Out { sc, un }, ed);
    m.set_i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_48, 0);
    m[LOADING] = 1;
    let untold = m[ed].hier_dirty != 0 || m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_36) == 0;
    // the name is the caller's string
    let node = with_text(m, name, |m, p| si_dict::elemname::db_scn_get_list_node(m, &mut Out { sc, un }, p, Ptr32::NULL, 0));
    let dsc = m[SI_CID].dsc;
    let index = elemdict::add_elem(m, &mut Out { sc, un }, ed, dsc, node, Ptr32::NULL, Ptr32::NULL, 0, 0);
    let e = elemdict::get_elem(m, ed, dsc, index);
    m[e].new_flag = 1;
    let ok = load_elements_from_dict(m, sc, fs, info, ed, true, false, untold, un);
    let saved = m.i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_4A);
    m.set_i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_48, saved);
    ok
}

/// The element of a chapter that a description's number names
/// [soft 0x081297b0 elemdictGetElemFromWorkId]: the first element in use that was created
/// by a load (new-flag 1) for description `dsc_id` with number `work_id`. In the SCENES
/// chapter the first element in use, whatever the numbers. The original stores the index
/// through its last argument (-1 for none) and answers the element.
pub fn get_elem_from_work_id(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, dsc_id: i32, work_id: i32) -> Option<i32> {
    // Where Rust must differ: the original does not test the chapter. A relation or
    // environment block that names a chapter the dictionary lacks makes it go through
    // whatever lies 0x40 bytes before the chapter array (index -1); here that finds
    // nothing.
    if chap < 0 || chap >= m[ed].nb_lists {
        return None;
    }
    let l = list(m, ed, chap);
    let dsc = m[SI_CID].dsc;
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status == ELEM_VALID && (chap == dsc || (m[e].new_flag == 1 && m[e].work_id == work_id && m[e].dsc_id == dsc_id)) {
            return Some(i);
        }
    }
    None
}

/// Reads the description of every scene element a load just added (new-flag 1, in use, not
/// yet read: work flag 0) [soft 0x0812bfd8 SI_elemdictExpandDescriptions]. The scenes that
/// exist when the routine starts are looked at; sub-scenes are read by the recursion of
/// [`expand_dsc_file`]. False as soon as one description cannot be read.
pub fn expand_descriptions(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, un: &mut dyn Unported) -> bool {
    let dsc = m[SI_CID].dsc;
    let n = m[list(m, ed, dsc)].nb_elems;
    for i in 0..n {
        let dsc = m[SI_CID].dsc;
        let e = elemdict::get_elem(m, ed, dsc, i);
        if !e.is_null() && m[e].status == ELEM_VALID && m[e].new_flag == 1 && m[e].work_flag == 0 && !expand_dsc_file(m, sc, fs, Some(info), ed, i, e, 0, un) {
            return false;
        }
    }
    true
}

/// A scene with the same full name that has been read already
/// [soft 0x0812b8dc elemdictAlreadyExist]: the first other element in use of the chapter
/// whose work flag is set, with its index.
fn already_exist(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>) -> Option<(Ptr32<Elem>, i32)> {
    let l = list(m, ed, chap);
    for i in 0..m[l].nb_elems {
        let p = m[l].elems.add(i);
        if m[p].status == ELEM_VALID && p != e && m[p].work_flag != 0 {
            let (a, b) = (m[m[p].node].fullname, m[m[e].node].fullname);
            if si_mem::clib::strcmp(m, a.addr(), b.addr()) == 0 {
                return Some((p, i));
            }
        }
    }
    None
}

/// The thirteen words from +0x0c to +0x3f of an element: schematic position, width and
/// collapse state, wire colour and type, selection, the two words at +0x28, the box, and
/// the pointer to the root's nine numbers (shared, as the original shares it).
fn copy_display_state(m: &mut Mem, src: Ptr32<Elem>, dst: Ptr32<Elem>) {
    for k in 0..13 {
        let w = m.u32(src.addr() + 0x0c + 4 * k);
        m.set_u32(dst.addr() + 0x0c + 4 * k, w);
    }
}

/// A sub-scene included a second time is not read again: every element that was read for
/// the first inclusion (`from`) is added once more for this one (`to`), with the including
/// scene's alias, the same number, display state and depth
/// [soft 0x0812b960 elemdictDuplicateDscContent].
///
/// The loops run over the chapters as they grow: the copies, which belong to `to`, are
/// passed over. For each copy the alias and the source's full name are duplicated first
/// (two blocks that nothing frees), then the name record is made from them. A deleted
/// slot whose description number is `from` is copied too: the original then reads through
/// the null pointer the slot holds for its name record.
fn duplicate_dsc_content(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, scene_elem: Ptr32<Elem>, to: i32, from: i32, un: &mut dyn Unported) {
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let mut i = 0;
        while i < m[list(m, ed, chap)].nb_elems {
            let src = elem(m, ed, ElemId::new(chap, i));
            if m[src].dsc_id == from {
                let scene_alias = m[m[scene_elem].node].str_0c;
                let alias = if scene_alias.is_null() { 0 } else { m.strdup(scene_alias.addr()) };
                let full = m[m[src].node].fullname;
                let name = m.strdup(full.addr());
                let node = si_dict::elemname::db_scn_get_list_node(m, &mut Out { sc, un }, Ptr32::new(name), Ptr32::new(alias), 0);
                // Where Rust must differ: adding the element moves the chapter's array,
                // and the original goes on reading the source slot at its old address
                // (freed memory). The source is looked up again here.
                let work_id = m[src].work_id;
                let index = elemdict::add_elem(m, &mut Out { sc, un }, ed, chap, node, Ptr32::NULL, Ptr32::NULL, to, work_id);
                let src = elem(m, ed, ElemId::new(chap, i));
                let copy = elemdict::get_elem(m, ed, chap, index);
                copy_display_state(m, src, copy);
                m[copy].hier_level = m[src].hier_level;
                m[copy].new_flag = 1;
            }
            i += 1;
        }
        chap += 1;
    }
}

/// The second half for a sub-scene included twice: the display state and the forward
/// relations of the first inclusion's elements are given to the second's
/// [soft 0x0812ba8c elemdictDuplicateDscRelAndEnv].
///
/// Per chapter the routine finds the first slot of each inclusion and walks the two runs in
/// step. Kept oddities: nothing is done for a chapter in which the first inclusion's run
/// starts at slot 0 (the test is "above zero"); the walk ends when the first run ends, and
/// the relations are added with the plain routine (no data, no serial number), under the
/// rule number the relation dictionary gives for the two chapters and lists.
fn duplicate_dsc_rel_and_env(m: &mut Mem, ed: Ptr32<ElemDict>, from: i32, to: i32) {
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let l = list(m, ed, chap);
        let first_of = |m: &Mem, id: i32| (0..m[l].nb_elems).find(|&i| m[m[l].elems.add(i)].dsc_id == id).unwrap_or(-1);
        let mut a = first_of(m, from);
        let mut b = first_of(m, to);
        if b > -1 && a > 0 {
            loop {
                let n = m[l].nb_elems;
                // Where Rust must differ: when the first run ends the chapter the original
                // reads the slot behind the array, and writes behind it when the second
                // run is the shorter one; here the walk ends with the array.
                if a >= n || b >= n || m[m[l].elems.add(a)].dsc_id != from {
                    break;
                }
                let src = m[l].elems.add(a);
                let dst = m[l].elems.add(b);
                copy_display_state(m, src, dst);
                let mut g = 0;
                while g < m[src].forward.nb_groups {
                    let rl = m[src].forward.groups.add(g);
                    let mut k = 0;
                    while k < m[rl].nb {
                        let r = m.read(m[rl].rels.add(k));
                        // (the original reads the element at the other end without
                        // testing that there is one)
                        let other = elemdict::get_elem(m, ed, r.chapid as i32, r.elemid);
                        if !other.is_null() {
                            let wid = m[other].work_id;
                            let to_dsc = m[dst].dsc_id;
                            let target = get_elem_from_work_id(m, ed, r.chapid as i32, to_dsc, wid).unwrap_or(-1);
                            let scratch: Ptr32<i32> = Ptr32::new(m.local(4));
                            let rule = reldict::get_rel_sub(m, SI_RD, r.relid, chap, r.chapid as i32, g, r.group as i32, scratch);
                            m.release(4);
                            elemrel::add_relation(m, ed, SI_RD, b, target, r.relid, rule, 0);
                        }
                        k += 1;
                    }
                    g += 1;
                }
                a += 1;
                b += 1;
            }
        }
        chap += 1;
    }
}

/// Reads one scene description into the dictionary [soft 0x0812bcfc SI_elemdictExpandDscFile].
///
/// 1. More than one level of sub-scene is refused with a message.
/// 2. The file is `<database>/SCENES/<sub-directory><full name>.dsc`; the path is left in
///    the file search's work buffer (`SI_dBWorkFnameExt`).
/// 3. The scene's element gets work flag 1 ("read").
/// 4. ELEMENTS: one element per line, numbered per chapter from 0, belonging to description
///    `scene_index` ([`apply_elements`]).
/// 5. Each element this added to SCENES is a sub-scene: read the same way one level down,
///    or, if a scene of that name was read before, copied from it.
/// 6. RELATIONS and ENVIRONMENT ([`apply_relations`], [`apply_environment`]).
/// 7. Sub-scenes that were copies get their relations and display state copied.
///
/// False when the file cannot be opened or the dictionary has no SCENES chapter, and for a
/// file `si-ascii` reports the original would not return from.
#[allow(clippy::too_many_arguments)]
pub fn expand_dsc_file(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: Option<&mut DbInfo>, ed: Ptr32<ElemDict>, scene_index: i32, scene_elem: Ptr32<Elem>, level: i32, un: &mut dyn Unported) -> bool {
    if level >= 2 {
        sc.messages.push(Message { channel: Channel::Stderr, text: si_rt::message(0x0868_3440).to_vec() });
        return false;
    }
    let Some(info) = info else { return false }; // without a database the original opens whatever the work buffer holds
    info.set_chapter(b"SCENES");
    info.filepath_build();
    let node = m[scene_elem].node;
    let path = concat(&[info.chapter_path.as_deref().unwrap_or(si_db::NULL_TEXT), &or_null(m, m[node].str_08), &or_null(m, m[node].fullname), b".dsc"]);
    set_work_name(m, SI_DB_WORK_FNAME_EXT, &path);
    let Some(data) = fs.read(&path) else { return false };
    let scenes = find_list_by_name(m, ed, b"SCENES");
    if scenes == -1 {
        return false;
    }
    let e = elemdict::get_elem(m, ed, scenes, scene_index);
    elemdict::set_flag(m, e, 1);
    let alias = m[m[e].node].str_0c;
    let Ok((desc, _)) = dsc::read(&data) else {
        sc.messages.push(Message { channel: Channel::Stderr, text: b"(the original does not return from this file)\n".to_vec() });
        return false;
    };
    sc.dsc_version = desc.version;
    // the newer-version check of the reader: a warning in the interactive program; the batch
    // renderer asks an installable hook, which can abort the read
    if m[LOADING] != 0 && desc.version as f64 > dsc::NEWER_VERSION_ABOVE {
        un.call("SI_dialogRequestLong: newer version", &[Arg::Int((desc.version * 1000.0) as i64)]);
    }
    for msg in &desc.messages {
        let text: &[u8] = match msg {
            dsc::Message::Element1NotFound => si_rt::message(0x0868_32b7),
            dsc::Message::Element2NotFound => si_rt::message(0x0868_32d5),
            dsc::Message::ElementNotFound => si_rt::message(0x0868_33d1),
            dsc::Message::CustomIconNotFound => si_rt::message(0x0868_3340),
            dsc::Message::InvalidRelationData => si_rt::message(0x0868_2340),
        };
        sc.messages.push(Message { channel: Channel::Stderr, text: text.to_vec() });
    }
    apply_elements(m, sc, ed, &desc, scene_index, alias, un);
    let dsc_chap = m[SI_CID].dsc;
    let n = m[list(m, ed, dsc_chap)].nb_elems;
    for i in 0..n {
        let dsc_chap = m[SI_CID].dsc;
        let p = elemdict::get_elem(m, ed, dsc_chap, i);
        // (the original reads the slot without testing that it is there: a freed slot of
        // the SCENES chapter is a null pointer it goes through)
        if p.is_null() {
            continue;
        }
        if m[p].dsc_id == scene_index && m[p].work_flag == 0 {
            match already_exist(m, ed, dsc_chap, p) {
                None => {
                    expand_dsc_file(m, sc, fs, Some(info), ed, i, p, level + 1, un);
                }
                Some((_, first)) => duplicate_dsc_content(m, sc, ed, p, i, first, un),
            }
        }
    }
    apply_relations(m, sc, ed, &desc, scene_index, un);
    apply_environment(m, sc, ed, &desc, scene_index, un);
    let dsc_chap = m[SI_CID].dsc;
    let n = m[list(m, ed, dsc_chap)].nb_elems;
    for i in 0..n {
        let p = elemdict::get_elem(m, ed, dsc_chap, i);
        if p.is_null() {
            continue;
        }
        if m[p].dsc_id == scene_index && m[p].work_flag == 0 {
            if let Some((_, first)) = already_exist(m, ed, dsc_chap, p) {
                duplicate_dsc_rel_and_env(m, ed, first, i);
                elemdict::set_flag(m, p, 1);
            }
        }
    }
    true
}

/// The ELEMENTS section applied to the dictionary: what `dbReadDescriptionElements`
/// (0x0812a4a0) and `dbReadChapterElements` (0x0812a180) do with each line once `si-ascii`
/// has parsed it, with the blocks those two make on the way.
///
/// - Each `CHAPTER` word: the name is duplicated and handed to `SI_elemdictAddList`, which
///   keeps it for a chapter the dictionary does not have and otherwise leaves it lost.
/// - Per element line: the name is duplicated; so is the alias after `ALIAS`, and the alias
///   the record gets (the scene's own alias followed by the line's: a sub-scene's elements
///   are prefixed). The name is split into a name record (`SI_dbScnGetListNode`); in the
///   PICTURES chapter the name is a path: the record is made from its last component and
///   a copy of the whole path becomes the record's sub-directory (the empty string the
///   record had there is not freed). Then the three copies are freed.
/// - The element is added with the description's index and its number in the chapter, its
///   depth is 1 for a line without `ROOT` in MODELS, LIGHTS, CAMERAS and EXPRESSIONS and 0
///   otherwise, its substitution number is the one in its name, its new-flag 1.
///
/// The chapters are gone through in the file's order. The reader's token buffer and its
/// own stack buffers are not in the image.
pub fn apply_elements(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, desc: &Description, dsc_id: i32, scene_alias: CStr32, un: &mut dyn Unported) {
    let mut seen: Vec<i32> = Vec::new();
    for &ci in &desc.element_order {
        if seen.contains(&ci) {
            continue;
        }
        seen.push(ci);
        let Some(c) = desc.chapters.get(ci as usize) else { continue };
        let copy = dbnode::new_string(m, &c.name);
        let chap = elemdict::add_list(m, ed, copy);
        let img = m[SI_CID].img;
        for e in &c.elements {
            let name = dbnode::new_string(m, &e.name);
            let line_alias = match &e.alias {
                Some(a) => dbnode::new_string(m, a),
                None => Ptr32::NULL,
            };
            let alias: CStr32 = if scene_alias.is_null() && line_alias.is_null() {
                Ptr32::NULL
            } else {
                let mut text = dbnode::text(m, scene_alias).unwrap_or_default();
                text.extend_from_slice(&dbnode::text(m, line_alias).unwrap_or_default());
                dbnode::new_string(m, &text)
            };
            let (picture_path, split_from) = if chap == img {
                let whole = m.strdup(name.addr());
                let tail = si_db::filename_remove_path(&e.name);
                (whole, name.addr() + (e.name.len() - tail.len()) as u32)
            } else {
                (0, name.addr())
            };
            let node = si_dict::elemname::db_scn_get_list_node(m, &mut Out { sc, un }, Ptr32::new(split_from), alias, 0);
            if chap == img {
                m[node].str_08 = Ptr32::new(picture_path);
            }
            m.free(name.addr());
            m.free(alias.addr());
            m.free(line_alias.addr());
            let index = elemdict::add_elem(m, &mut Out { sc, un }, ed, chap, node, Ptr32::NULL, Ptr32::NULL, dsc_id, e.work_id);
            let el = elemdict::get_elem(m, ed, chap, index);
            m[el].hier_level = e.not_root;
            m[el].subst = si_mem::clib::strtol10(m, m[node].subst.addr()) as i16;
            m[el].new_flag = 1;
        }
    }
}

/// Stand-in for the custom effect code ([`Assume::effects_installed`]): for each effect the
/// description's relations name (the two names in their data), the relation dictionary gets
/// as many consecutive rules of relation 20000 with those names as the highest rule number
/// used, each of rank 50 without a fan-out limit. An installed effect's description file
/// declares its relations in one run like this (finding 0006, section 7).
fn install_effects(m: &mut Mem, ed: Ptr32<ElemDict>, desc: &Description) {
    let mut effects: Vec<(Vec<u8>, Vec<u8>, i32, i32, i32)> = Vec::new();
    for b in &desc.relations {
        let (l1, l2) = (find_list_by_name(m, ed, &b.name1), find_list_by_name(m, ed, &b.name2));
        for r in &b.relations {
            if let Some(RelData::Custom(c)) = &r.data {
                if let (Some(en), Some(mn)) = (&c.ename, &c.mname) {
                    match effects.iter_mut().find(|e| &e.0 == en && &e.1 == mn) {
                        Some(e) => e.2 = e.2.max(r.subdef),
                        None => effects.push((en.clone(), mn.clone(), r.subdef.max(1), l1, l2)),
                    }
                }
            }
        }
    }
    for (en, mn, rules, l1, l2) in effects {
        let known = with_text(m, &mn, |m, effect| with_text(m, &en, |m, name| reldict::get_rel_index_from_name(m, SI_RD, effect, name)));
        if known >= 0 {
            continue;
        }
        // the rules share one copy of the effect's name
        let effect = dbnode::new_string(m, &mn);
        let info: Ptr32<si_dict::records::CusRelInfo> = Ptr32::new(m.local(0x28));
        m.set_fill_bytes(info.addr(), 0, 0x28);
        m[info].name = effect;
        m[info].max_fanout_dst = 1;
        m[info].max_fanout_src = 1;
        for _ in 0..rules {
            with_text(m, &en, |m, name| reldict::add_relation(m, SI_RD, name, reldata::REL_CUSTOM, l1, l2, 4, 4, 0x32, 0x7fff, 0, 0, 0, 1, info, 0, 3));
        }
        m.release(0x28);
    }
}

/// The serial number of a custom effect's icon [SI_cusIconSetSerialId, 0x08535dc0, of the
/// custom effect code, reproduced here because the description reader depends on it]: one
/// fresh serial number per icon element, remembered until the relations section ends.
fn icon_serial(m: &mut Mem, sc: &mut Scene, icon: i32) -> i32 {
    if let Some(&(_, s)) = sc.icon_serials.iter().find(|(i, _)| *i == icon) {
        return s;
    }
    let s = relations::get_rel_serial_number(m);
    sc.icon_serials.push((icon, s));
    s
}

/// The data of a relation line as the entry's pointer: the record of the image the
/// original's reader makes ([`world::rel_data_record`]).
fn store_data(m: &mut Mem, sc: &mut Scene, data: &Option<RelData>) -> Ptr32<Opaque> {
    match data {
        Some(d) => world::rel_data_record(m, sc, d.clone()),
        None => Ptr32::NULL,
    }
}

/// The two lists rule number `rule` of a relation names (`SI_reldictGetRelAttributes`
/// with six locals of the caller); 0 and 0 for a relation the dictionary does not have
/// (the original's locals then hold what the stack held).
fn rule_groups(m: &mut Mem, relid: i32, rule: i32) -> (i32, i32) {
    let base = m.local(24);
    m.set_fill_bytes(base, 0, 24);
    let w = |k: u32| Ptr32::<i32>::new(base + 4 * k);
    reldict::get_rel_attributes(m, SI_RD, relid, rule, 0, w(0), w(1), w(2), w(3), w(4), w(5));
    let out = (m[w(2)], m[w(3)]);
    m.release(24);
    out
}

/// The RELATIONS section applied to the dictionary: what `dbReadDescriptionRelations`
/// (0x0812a9fc) and `dbReadSubRelations` (0x0812a788) do with each line.
///
/// A line names element `id1` of the block's first chapter, element `id2` of its second and
/// a relation id. The first element is the relation's source, the second its destination.
///
/// | Relation | Recorded with |
/// |---|---|
/// | 120, 121, 207, 208 | nothing |
/// | 20010 expression | a fresh serial number; both lists are the group of the attribute that owns the animated parameter (`SI_attdictGetGroupFromFcurve` on `SI_ATD`); rule 1 |
/// | 247, and 246 from version 2.99913 | a fresh serial number; source list 2, destination list 1; rule 1 |
/// | 551 | the serial number in the file; the lists of the rule the line's number names; no data, flag 1, no fan-out check |
/// | 20000 with data | the effect is prepared (`SI_cusPrepareEffect`, not ported); the icon is looked up among the models by the number in the data and the data's first word becomes its element index; the serial number is the icon's ([`icon_serial`]); the lists are the data's fourth and fifth values |
/// | others | the chapters of the block where the relation's rule leaves them open, else the rule's own; the rule is the line's number |
///
/// A sub-relation line adds a relation from the same source to the element it names, with
/// the same id and serial number: for an expression in the source list of the main line and
/// the destination list of its own parameter; for 246 and 247 in lists 2 and 0 under rule 2;
/// otherwise as an open-chapter relation under rule 0.
///
/// When the section ends the table of icon serial numbers is cleared.
pub fn apply_relations(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, desc: &Description, dsc_id: i32, un: &mut dyn Unported) {
    let model = m[SI_CID].model;
    if sc.assume.effects_installed {
        install_effects(m, ed, desc);
    }
    for b in &desc.relations {
        // the block's chapters as the dictionary knows them
        let list1 = find_list_by_name(m, ed, &b.name1);
        let list2 = find_list_by_name(m, ed, &b.name2);
        for r in &b.relations {
            let (Some(e1), Some(e2)) = (get_elem_from_work_id(m, ed, list1, dsc_id, r.elem1), get_elem_from_work_id(m, ed, list2, dsc_id, r.elem2)) else {
                continue;
            };
            let flag = r.active as i16;
            let mut serial = if r.relid == reldata::REL_SHADER_ARRAY { r.serial } else { -1 };
            let mut group = 0; // uninitialised in the original unless the line is an expression
            let mut ok = true;
            let mut recorded = true;
            match r.recorded {
                Recorded::Dropped => {
                    recorded = false;
                    sc.dropped_lines.push((list1, e1, list2, e2, r.relid, r.subdef, r.active));
                }
                Recorded::IconMissing => {
                    serial = -1;
                    recorded = false;
                }
                Recorded::Serial if r.relid == reldata::REL_EXP => {
                    let param = match &r.data {
                        Some(RelData::Expression(e)) => e.fcurve,
                        _ => 0,
                    };
                    group = si_dict::attdict::get_group_from_fcurve(m, si_dict::globals::SI_ATD, param);
                    serial = relations::get_rel_serial_number(m);
                    let data = store_data(m, sc, &r.data);
                    ok = elemrel::add_serial_relation(m, ed, SI_RD, list1, list2, group, group, e1, e2, r.relid, serial, 1, 0, data, flag, 1) != 0;
                }
                Recorded::Serial if r.relid == 0xf6 || r.relid == 0xf7 => {
                    serial = relations::get_rel_serial_number(m);
                    let data = store_data(m, sc, &r.data);
                    ok = elemrel::add_serial_relation(m, ed, SI_RD, list1, list2, 2, 1, e1, e2, r.relid, serial, 1, 0, data, flag, 1) != 0;
                }
                Recorded::Serial if r.relid == reldata::REL_SHADER_ARRAY => {
                    let (g1, g2) = rule_groups(m, r.relid, r.subdef);
                    ok = elemrel::add_serial_relation(m, ed, SI_RD, list1, list2, g1, g2, e1, e2, r.relid, serial, r.subdef, r.sub_index, Ptr32::NULL, 1, 0) != 0;
                }
                Recorded::Serial => {
                    // a custom effect with data
                    let Some(RelData::Custom(c)) = &r.data else { continue };
                    let mut c = c.clone();
                    if c.ename.is_some() {
                        un.call(
                            "SI_cusPrepareEffect",
                            &[Arg::Str(c.ename.clone().unwrap_or_default()), Arg::Str(c.mname.clone().unwrap_or_default())],
                        );
                    }
                    let icon_wid = si_ascii::cnum::atoi(c.icon_id.as_deref().unwrap_or(b""));
                    match get_elem_from_work_id(m, ed, model, dsc_id, icon_wid) {
                        None => {
                            // si-ascii found the icon in the file's own list; the dictionary
                            // does not have it (it cannot happen for a file read whole)
                            serial = -1;
                            recorded = false;
                        }
                        Some(icon) => {
                            c.icon_elem = icon;
                            serial = icon_serial(m, sc, icon);
                            let (g1, g2) = (c.group_10, c.group_14);
                            let data = world::rel_data_record(m, sc, RelData::Custom(c));
                            ok = elemrel::add_serial_relation(m, ed, SI_RD, list1, list2, g1, g2, e1, e2, r.relid, serial, r.subdef, 0, data, flag, 1) != 0;
                        }
                    }
                }
                Recorded::Plain => {
                    let data = match &r.data {
                        Some(RelData::NamedSelection(n)) => {
                            // the data names an element by its number in the file: the reader
                            // stores the element's index in the dictionary
                            let mut n = n.clone();
                            n.chapter = find_list_by_name(m, ed, &n.chapter_name);
                            n.elem = get_elem_from_work_id(m, ed, n.chapter, dsc_id, si_ascii::cnum::atoi(&n.elem_id)).unwrap_or(-1);
                            world::rel_data_record(m, sc, RelData::NamedSelection(n))
                        }
                        other => store_data(m, sc, other),
                    };
                    ok = if reldict::check_uncomplete_relation(m, SI_RD, r.relid) != 0 {
                        elemrel::add_uncomplete_relation(m, ed, SI_RD, list1, list2, e1, e2, r.relid, r.subdef, r.sub_index, data, flag) != 0
                    } else {
                        elemrel::add_complete_relation(m, ed, SI_RD, e1, e2, r.relid, r.subdef, r.sub_index, data, flag) != 0
                    };
                }
            }
            if recorded && !ok {
                sc.refused_relations.push((r.relid, list1, list2));
            }
            if let Some(subs) = &r.subrels {
                // one data block serves the lines that follow it: the pointer is shared
                let mut last: Option<Ptr32<Opaque>> = None;
                for s in subs.iter() {
                    let chap = find_list_by_name(m, ed, desc.chapters.get(s.chapter as usize).map_or(&b""[..], |c| &c.name));
                    let Some(sub) = get_elem_from_work_id(m, ed, chap, dsc_id, s.elem) else { continue };
                    let data = match (&last, s.own_data) {
                        (Some(p), false) => *p,
                        _ => {
                            let p = store_data(m, sc, &s.data);
                            last = Some(p);
                            p
                        }
                    };
                    let ok = if r.relid == reldata::REL_EXP {
                        let param = match &s.data {
                            Some(RelData::Expression(e)) => e.fcurve,
                            _ => 0,
                        };
                        let g = si_dict::attdict::get_group_from_fcurve(m, si_dict::globals::SI_ATD, param);
                        elemrel::add_serial_relation(m, ed, SI_RD, list1, chap, group, g, e1, sub, r.relid, serial, 1, 0, data, flag, 1) != 0
                    } else if r.relid == 0xf6 || r.relid == 0xf7 {
                        elemrel::add_serial_relation(m, ed, SI_RD, list1, chap, 2, 0, e1, sub, r.relid, serial, 2, 0, data, flag, 1) != 0
                    } else {
                        elemrel::add_uncomplete_relation(m, ed, SI_RD, list1, chap, e1, sub, r.relid, 0, 0, data, flag) != 0
                    };
                    if !ok {
                        sc.refused_relations.push((r.relid, list1, chap));
                    }
                }
            }
        }
    }
    sc.icon_serials.clear(); // SI_cusIconClearSerialIdTable
}

/// The ENVIRONMENT section applied to the dictionary: what `dbReadDescriptionEnvironment`
/// (0x0812b498) stores for each element line, and the play control block.
///
/// `SCHEM` goes to +0x0c, +0x10, +0x18; `USR` sets bit 0 of +0x2a; `WIRECOL` becomes one
/// colour-map index at +0x1c; `DISPLAY` goes to +0x24 and +0x20; `SRT` to a block of nine
/// floats at +0x3c (`dbReadRootModelSRT`, 0x08129bec: `SI_malloc(0x24)`); `MPRFLG` is
/// stored **as a float** at +0x2c, from where the loader later moves it into the flag word
/// of the element's node.
///
/// `SETUP <name>`, the old way of naming the scene's setup file: when the dictionary's
/// SETUP_SOFT chapter has no slot, an element of that name is added to it, with
/// description index and number 0 and new-flag 1. (`si-ascii` hands such a name on as an
/// element of the file's SETUP_SOFT chapter; it is one when the ELEMENTS section has no
/// such chapter.)
pub fn apply_environment(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, desc: &Description, dsc_id: i32, un: &mut dyn Unported) {
    if !desc.element_order.contains(&dsc::SETUP_SOFT) {
        if let Some(e) = desc.chapters.get(dsc::SETUP_SOFT as usize).and_then(|c| c.elements.first()) {
            let sts = m[SI_CID].sts;
            if m[list(m, ed, sts)].nb_elems == 0 {
                let node = with_text(m, &e.name, |m, p| si_dict::elemname::db_scn_get_list_node(m, &mut Out { sc, un }, p, Ptr32::NULL, 0));
                let index = elemdict::add_elem(m, &mut Out { sc, un }, ed, sts, node, Ptr32::NULL, Ptr32::NULL, 0, 0);
                let el = elemdict::get_elem(m, ed, sts, index);
                m[el].new_flag = 1;
            }
        }
    }
    for b in &desc.environment {
        let chap = find_list_by_name(m, ed, &b.name);
        let Some(c) = desc.chapters.get(b.list as usize) else { continue };
        for &i in &b.elems {
            let Some(e) = c.elements.get(i) else { continue };
            let Some(idx) = get_elem_from_work_id(m, ed, chap, dsc_id, e.work_id) else { continue };
            let wire = sc.wire.index_to_wire(e.wire.0 as i16, e.wire.1 as i16);
            let el = elem(m, ed, ElemId::new(chap, idx));
            m[el].schem_x = e.schem_x;
            m[el].schem_y = e.schem_y;
            m[el].schem_collapse = e.collapsed;
            if e.usr {
                m[el].schem_flags |= 1;
            }
            m[el].wire_color = wire;
            m[el].selection = e.selected;
            m[el].wire_type = e.line_type;
            if let Some(s) = e.srt {
                let block: Ptr32<f32> = Ptr32::new(m.si_malloc(0x24));
                m[el].root_srt = block;
                if !block.is_null() {
                    for (k, v) in s.iter().enumerate() {
                        m[block.add(k as i32)] = *v;
                    }
                }
            }
            if let Some(v) = e.mprflg {
                m[el].schem_bbox[0] = v;
            }
        }
    }
    if let Some(p) = &desc.play {
        sc.play = p.clone();
        // the frame rate is a program global (`SI_frameRate`) the action readers and
        // writers of the animation files use
        sc.statics.frame_rate = p.rate;
        // adapter: not in place
        // `SI_dbReadPlayControlInfo` (0x08110dd0) reads the block into the play control's
        // globals themselves, while the description is read and before any element file:
        // `si-ascii` reads it into a value, which is stored here where the original stores
        // it. (A block without one of its keywords leaves that global as it was in the
        // original; the value has 0 there.)
        play_to_image(m, p);
    }
}

/// The play control's globals (`SI_frameStart` …, `SI_stepAdaptive`), as
/// `SI_dbReadPlayControlInfo` leaves them.
pub fn play_to_image(m: &mut Mem, p: &PlayControl) {
    use crate::globals::{SI_FRAME_CURRENT, SI_FRAME_END, SI_FRAME_PAUSE, SI_FRAME_RATE, SI_FRAME_START, SI_FRAME_STEP, SI_STEP_ADAPTIVE};
    m[SI_FRAME_START] = p.start;
    m[SI_FRAME_END] = p.end;
    m[SI_FRAME_CURRENT] = p.current;
    m[SI_FRAME_STEP] = p.step;
    m[SI_STEP_ADAPTIVE] = p.step_adaptive;
    m[SI_FRAME_PAUSE] = p.pause;
    m[SI_FRAME_RATE] = p.rate;
}

/// The two save options of a setup file: `prefMenuRead` (0x083e3768) gathers the PREFERENCE
/// section's `SAVE_MAT_TXT_NOT_SEPARATELY` and `SAVE_NOT_RM_OLD_VERSIONS` and, when the
/// section ends at an unknown word, hands them to `SI_elemdictSetSaveOptions` (0x0812e0d0),
/// which stores them in `si_saveOptNoMatTxt` and `si_saveOptNoOldVer` (0x08684062,
/// 0x08684060): here [`crate::dictout::SaveState::no_mat_txt`] and `no_old_ver`, which the
/// save reads (`si_elemCheckRestrictedSave`, the removal of old versions). Both are cited in
/// `si-ascii`'s reader, which is run over the two values as they are, so that a file
/// without the section, or ending inside it, leaves them unchanged as in the original.
fn save_options_from_setup(sc: &mut Scene, data: &[u8]) {
    let mut s = si_ascii::setup::Setup::default();
    s.pref.save_mat_txt_not_separately = sc.saving.no_mat_txt;
    s.pref.save_not_rm_old_versions = sc.saving.no_old_ver;
    let _ = si_ascii::setup::read_into(data, &mut s, &Default::default());
    sc.saving.no_mat_txt = s.pref.save_mat_txt_not_separately;
    sc.saving.no_old_ver = s.pref.save_not_rm_old_versions;
}

/// The preferences of a setup file in the image's globals (`prefMenuRead`, 0x083e3768, and
/// the setter it calls for some, `SI_setPrefFlag`, 0x08416e7c): the words of `SI_glbManip`
/// and `SI_glbAnim` a setup file carries, and `SI_rightHand`. The file is read over a state
/// made of the image's words, so that a keyword the file lacks leaves its word as it is
/// and a file cut short keeps what was stored up to there, as in the original.
///
/// Not stored: `SI_glbManip` +0x2a (set by `PREF_CLASSICAL_SCALING` when it changes the
/// method, never under the stand-alone renderer; the loader records the dialogue that
/// reads it), `SI_hrchCpst`, `SI_busyCursor`, `SI_zoomONCursor`, the three display
/// switches: nothing on the image reads them. The save options go to the save's state
/// ([`save_options_from_setup`]).
// adapter: not in place
pub fn preferences_to_image(m: &mut Mem, data: &[u8]) {
    use crate::globals::SI_RIGHT_HAND;
    let (gm, ga) = (SI_GLB_MANIP.addr(), SI_GLB_ANIM.addr());
    let mut over = si_ascii::setup::Setup::default();
    {
        let p = &mut over.pref;
        p.classical_scaling = m.i32(gm);
        p.uniform_scaling = m.i32(gm + 0x04);
        p.rotation = m.i32(gm + 0x08);
        p.translation = m.i32(gm + 0x0c);
        p.manipulation = m.i32(gm + 0x10);
        p.mode = m.i32(gm + 0x14);
        p.multi_node = m.i16(gm + 0x1c);
        p.hierarchy_manipulation = m.i16(gm + 0x1e);
        p.continuous_rotation = m.i16(gm + 0x20);
        p.modelling_relation = m.i16(gm + 0x22);
        p.auto_cluster_ctr = m.i16(gm + 0x28);
        p.user_transfo = m.i16(gm + 0x32);
        p.swiftkeys_all_modules = m.i16(gm + 0x34);
        p.image_lock = m.i16(ga);
        p.highlight_key = m.i16(ga + 0x02);
        p.confirm_keyedit = m.i16(ga + 0x04);
        p.fcv_interp_type = m.i16(ga + 0x30);
        p.dope_snap_keys = m.i16(ga + 0x32);
        p.invkin_auto_cns = m.i32(ga + 0x34);
        p.seq_act_fcv = m.i16(ga + 0x3a);
        p.fit_slp_to_fcv_uni_slp = m.i16(ga + 0x3e);
        p.right_handed_surface = m[SI_RIGHT_HAND];
    }
    let _ = si_ascii::setup::read_into(data, &mut over, &Default::default());
    let p = &over.pref;
    m.set_i32(gm, p.classical_scaling);
    m.set_i32(gm + 0x04, p.uniform_scaling);
    m.set_i32(gm + 0x08, p.rotation);
    m.set_i32(gm + 0x0c, p.translation);
    m.set_i32(gm + 0x10, p.manipulation);
    m.set_i32(gm + 0x14, p.mode);
    m.set_i16(gm + 0x1c, p.multi_node);
    m.set_i16(gm + 0x1e, p.hierarchy_manipulation);
    m.set_i16(gm + 0x20, p.continuous_rotation);
    m.set_i16(gm + 0x22, p.modelling_relation);
    m.set_i16(gm + 0x28, p.auto_cluster_ctr);
    m.set_i16(gm + 0x32, p.user_transfo);
    m.set_i16(gm + 0x34, p.swiftkeys_all_modules);
    m.set_i16(ga, p.image_lock);
    m.set_i16(ga + 0x02, p.highlight_key);
    m.set_i16(ga + 0x04, p.confirm_keyedit);
    m.set_i16(ga + 0x30, p.fcv_interp_type);
    m.set_i16(ga + 0x32, p.dope_snap_keys);
    m.set_i32(ga + 0x34, p.invkin_auto_cns);
    m.set_i16(ga + 0x3a, p.seq_act_fcv);
    m.set_i16(ga + 0x3e, p.fit_slp_to_fcv_uni_slp);
    m[SI_RIGHT_HAND] = p.right_handed_surface;
}

fn to_read(m: &Mem, e: Ptr32<Elem>) -> bool {
    m[e].status == ELEM_VALID && m[e].new_flag == 1 && m[e].data.is_null() && m[e].hier_level == 0
}

/// How many elements of a chapter a load has to read: those in use that a load created
/// (new-flag 1), that have no object yet and are roots (depth 0)
/// [soft 0x08128198 elemdictReadChapterCount].
pub fn read_chapter_count(m: &Mem, l: Ptr32<ElemList>) -> i32 {
    (0..m[l].nb_elems).filter(|&i| to_read(m, m[l].elems.add(i))).count() as i32
}

/// Reads the element files [soft 0x081283bc SI_elemdictReadElements]. Nothing happens when
/// no chapter has anything to read. The order of the chapters:
///
/// 1. MODELS;
/// 2. every other chapter in dictionary order, except PICTURES, RENDER_PICTURES, ANIMATION,
///    SCENES and SETUP_SOFT;
/// 3. ANIMATION;
/// 4. SETUP_SOFT.
///
/// So models are in memory before what is read onto them (shapes, clusters, lattices), and
/// animation comes after everything it can animate. Pictures are not read here.
pub fn read_elements(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, un: &mut dyn Unported) {
    let mut total = 0;
    let mut c = 0;
    while c < m[ed].nb_lists {
        total += read_chapter_count(m, list(m, ed, c));
        c += 1;
    }
    if total == 0 {
        return;
    }
    un.call("SI_dialogInProgressOpen", &[Arg::Str(b"Read Elements".to_vec())]);
    let model = m[SI_CID].model;
    read_chapter_elements(m, sc, fs, info, ed, model, un);
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let cid = cid(m);
        if ![cid.model, cid.img, cid.rim, cid.ani, cid.dsc, cid.sts].contains(&chap) {
            read_chapter_elements(m, sc, fs, info, ed, chap, un);
        }
        chap += 1;
    }
    let ani = m[SI_CID].ani;
    read_chapter_elements(m, sc, fs, info, ed, ani, un);
    let sts = m[SI_CID].sts;
    read_chapter_elements(m, sc, fs, info, ed, sts, un);
    un.call("SI_dialogInProgressClose", &[]);
}

/// The sub-directories a chapter's record holds (`SI_ElemList` +0x2c, +0x30), as values.
fn sub_dirs_of(m: &Mem, l: Ptr32<ElemList>) -> Vec<Vec<u8>> {
    let (dirs, n) = (m[l].subdirs, m[l].nb_subdirs);
    if dirs.is_null() {
        return Vec::new();
    }
    (0..n).filter_map(|i| dbnode::text(m, m[dirs.add(i)])).collect()
}

// adapter: not in place
/// `SI_dbChapterGetSubDirs` (0x0812508c) on the chapter's record: the list it holds is
/// freed as the original frees it (each string, the array, the block at +0x34) and the
/// chapter's sub-directories are listed anew. The scan itself is `si-db`'s, on values;
/// what it finds is stored here as one array of pointers and one block a path. (The
/// original's scan grows the array itself and makes a block at +0x34 whose content is not
/// established: that one stays null. Its copy of the scan's starting path, made and freed
/// around the scan, is not made.)
pub(crate) fn chapter_get_sub_dirs(m: &mut Mem, sc: &Scene, fs: &dyn FileSystem, info: &DbInfo, l: Ptr32<ElemList>) {
    let old = m[l].subdirs;
    for i in 0..m[l].nb_subdirs {
        let s = m[old.add(i)];
        if !s.is_null() {
            m.free(s.addr());
            m[old.add(i)] = Ptr32::NULL;
        }
    }
    if !old.is_null() {
        m.free(old.addr());
    }
    let aux = m[l].subdir_aux;
    if !aux.is_null() {
        m.free(aux.addr());
    }
    m[l].nb_subdirs = 0;
    m[l].subdirs = Ptr32::NULL;
    m[l].subdir_aux = Ptr32::NULL;
    let session = Session { fs, now: sc.answers.now, loading: m[LOADING] != 0 };
    let (dirs, _) = si_db::chapter_get_sub_dirs(&session, info, true, None);
    if dirs.is_empty() {
        return;
    }
    let array: Ptr32<CStr32> = Ptr32::new(m.malloc(4 * dirs.len() as u32));
    for (i, d) in dirs.iter().enumerate() {
        let s = dbnode::new_string(m, d);
        m[array.add(i as i32)] = s;
    }
    m[l].subdirs = array;
    m[l].nb_subdirs = dirs.len() as i32;
}

/// Reads the elements of one chapter in index order
/// [soft 0x081281d8 elemdictReadChapterElements]. The chapter's sub-directories are listed first
/// (`SI_dbChapterGetSubDirs`, all levels, into the chapter's record): a file is looked for
/// in each of them when it is not where the element says. An element whose reader reports
/// failure is flagged deleted and then freed (`SI_edElementSetDeleteFlag` twice, with
/// `SI_addInDictionary` set for the two calls; only the lattice reader can report that).
pub fn read_chapter_elements(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, chap: i32, un: &mut dyn Unported) {
    let l = list(m, ed, chap);
    let name = m.cstr(m[l].name.addr());
    info.set_chapter(&name);
    info.filepath_build();
    let count = read_chapter_count(m, l);
    if count == 0 {
        return;
    }
    chapter_get_sub_dirs(m, sc, fs, info, l);
    let mut done = 1;
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if to_read(m, e) {
            // "Read <chapter> <n>" or, in the batch renderer, "Read <chapter>", with the
            // fraction done times a constant
            un.call("SI_dialogInProgressSet", &[Arg::Str(concat(&[b"Read ", &name])), Arg::Int(done), Arg::Int(count as i64)]);
            done += 1;
            if !read_an_element(m, sc, fs, info, ed, chap, e, i, un) {
                let saved = m[SI_ADD_IN_DICTIONARY];
                m[SI_ADD_IN_DICTIONARY] = 1;
                set_delete_flag(m, sc, ed, chap, i, un);
                m[SI_ADD_IN_DICTIONARY] = saved;
            }
        }
        i += 1;
    }
}

/// The two calls of `SI_edElementSetDeleteFlag` (0x080fd910, `edtools.c`) the loader makes
/// for an element whose reader failed: first the element is flagged (its relations are
/// removed at both ends, status 1, depth 0, root index its own), then the flagged slot is
/// freed (status 2, name record and icon freed, pointers cleared). Not a port of that
/// routine: only these two paths, without its refresh of the model hierarchy and of the
/// animation order.
fn set_delete_flag(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, chap: i32, index: i32, un: &mut dyn Unported) {
    let Some(e) = slot(m, ed, ElemId::new(chap, index)) else { return };
    if m[e].status != ELEM_VALID {
        return;
    }
    relations::rel_remove_relations(m, &mut Out { sc, un }, ed, e, chap, index, 1);
    relations::rel_remove_relations(m, &mut Out { sc, un }, ed, e, chap, index, 0);
    m[e].root_id = index;
    m[e].hier_level = 0;
    m[e].status = ELEM_DELETED;
    let node = m[e].node;
    m[e].node = Ptr32::NULL;
    m[e].data = Ptr32::NULL;
    m[e].aux_data = Ptr32::NULL;
    m[e].icon = Ptr32::NULL;
    dbnode::list_node_free(m, node);
    m[ed].hier_dirty = 1;
    m[ed].upd_list.dirty = 1;
    m[ed].rel_dirty = 1;
}

/// The node of a hierarchy that an element name designates and that no element of this
/// load has claimed yet [soft 0x081278d0 modelGetFromHierarchy]: the first, in the order
/// node, sons, brothers, whose name equals `name` and whose element index is unset, or
/// names an element that is not of this load (new-flag other than 1) or has no object.
/// A node with a null name is passed over with a status message.
///
/// The hierarchy is the model file's value in [`Scene::models`], not a record of the
/// image; the element the node names is read in the image.
pub fn model_get_from_hierarchy(m: &Mem, sc: &Scene, ed: Ptr32<ElemDict>, tree: u32, start: usize, name: &[u8]) -> Option<usize> {
    let model = m[SI_CID].model;
    let t = sc.models.get(tree)?;
    for i in start..t.names.len() {
        // a node read from a file always has a name here (the reader makes an empty one):
        // the null-name message cannot arise
        if t.names[i] == name {
            let id = t.elem_ids[i];
            if id < 0 {
                return Some(i);
            }
            // (the original indexes the MODELS chapter with the node's number unchecked)
            match slot(m, ed, ElemId::new(model, id)) {
                Some(e) if m[e].new_flag == 1 && !m[e].data.is_null() => {}
                _ => return Some(i),
            }
        }
    }
    None
}

/// Gives the elements of a model hierarchy their nodes
/// [soft 0x0812799c elemdictSetDataFromHierarchy]. For the root (`first`) the node is the root of the file
/// just read. For every other element the node is looked up by the element's name in the
/// whole hierarchy ([`model_get_from_hierarchy`]); if there is none the element gets no
/// object and a message says so. The node learns its element's index; the element takes
/// the root's "shared" mark. Then the same for each element that names this one as its
/// parent (relation 110 in list 1 on the backward side), in list order.
///
/// Recorded, not done: `fixPatch` and `SI_patchUpdateSurface` on a patch node of the old
/// 1 × 1 kind.
#[allow(clippy::too_many_arguments)]
fn set_data_from_hierarchy(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, l: Ptr32<ElemList>, root: Ptr32<Elem>, e: Ptr32<Elem>, index: i32, tree: u32, first: bool, un: &mut dyn Unported) {
    let model = m[SI_CID].model;
    let id = ElemId::new(model, index);
    if !first {
        let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
        let found = model_get_from_hierarchy(m, sc, ed, tree, 0, &name);
        match found {
            None => {
                let root_name = sc.models.get(tree).map(|t| t.names[0].clone()).unwrap_or_default();
                sc.status(2, concat(&[b"A model node named [", &name, b"] was expected under root [", &root_name, b"]: it couldn't be found"]));
                m[e].data = Ptr32::NULL;
                report(m, sc, ed, id, Outcome::NodeMissing);
            }
            Some(node) => {
                let handle = sc.objects.alloc(Object::ModelNode { tree, node });
                let gm = m[SI_DEFAULT_GMATTE] != 0;
                let t = sc.models.get_mut(tree).unwrap();
                t.elem_ids[node] = index;
                t.handles[node] = handle;
                if gm {
                    t.garbage_matte[node] = 1;
                }
                // the same two stores into the node of the image (+0x354, +0x3f8)
                let record = t.image.get(node).copied().unwrap_or(0);
                let patch_1x1 = t.kinds[node] == 1;
                crate::image::claim_node(m, sc, handle, record, index, gm);
                if patch_1x1 {
                    un.call("fixPatch / SI_patchUpdateSurface (if the patch is of type 1 x 1)", &[Arg::Ptr(handle)]);
                }
                m[e].data = sc.obj_addr(handle);
                report(m, sc, ed, id, Outcome::Node);
            }
        }
        m[e].shared = m[root].shared;
        elemdict::sort_tbl_set_dirty(m, l);
    } else {
        let gm = m[SI_DEFAULT_GMATTE] != 0;
        let t = sc.models.get_mut(tree).unwrap();
        t.elem_ids[0] = index;
        if gm {
            t.garbage_matte[0] = 1;
        }
        let (record, handle) = (t.image.first().copied().unwrap_or(0), t.handles[0]);
        crate::image::claim_node(m, sc, handle, record, index, gm);
    }
    let mut son = find_related(m, ed, e, 0, 1, REL_PARENT, model, true);
    while let Some((s, sid)) = son {
        set_data_from_hierarchy(m, sc, ed, l, root, s, sid.index, tree, false, un);
        son = find_related(m, ed, e, 0, 1, REL_PARENT, model, false);
    }
}

/// The path the file search tried last, which is what the "Couldn't read file" message
/// shows (the global `SI_dBWorkFnameExt`): the unversioned name in the last sub-directory
/// tried.
fn last_path_tried(info: &DbInfo, suffix: &[u8], sub_dirs: &[Vec<u8>], node: &si_db::ListNode) -> Vec<u8> {
    let or = |s: &Option<Vec<u8>>| s.clone().unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    if let Some(sub) = node.sub_path.as_deref() {
        if matches!(sub.first(), Some(b'/' | b'~' | b'$')) {
            return concat(&[sub, b"/", &or(&node.full)]);
        }
    }
    let plain = match &node.prefix {
        None => or(&node.name),
        Some(p) => concat(&[p, b"-", &or(&node.name)]),
    };
    let place: &[u8] = sub_dirs.last().map_or(b"", |d| d.as_slice());
    concat(&[&or(&info.chapter_path), place, &plain, b".", suffix])
}

// adapter: not in place
/// Writes into a name record of the image what a routine of `si-db` changed in its value:
/// each string that differs is freed and made anew, each number that differs is stored.
fn write_back(m: &mut Mem, node: Ptr32<DbListNode>, before: &si_db::ListNode, after: &si_db::ListNode) {
    macro_rules! string {
        ($value:ident, $field:ident) => {
            if before.$value != after.$value {
                dbnode::set_string(m, field!(node, DbListNode, $field).cast(), after.$value.as_deref());
            }
        };
    }
    string!(s04, unk_04);
    string!(sub_path, str_08);
    string!(s0c, str_0c);
    string!(prefix, prefix);
    string!(name, name);
    string!(version, version);
    string!(subst, subst);
    string!(full, fullname);
    if before.w00 != after.w00 {
        m[node].unk_00 = after.w00;
    }
    if before.time != after.time {
        m[node].time = after.time;
    }
    if before.mode != after.mode {
        m[node].mode = after.mode;
    }
    if before.w28 != after.w28 {
        m[node].unk_28 = after.w28;
    }
    if before.w2a != after.w2a {
        m[node].unk_2a = after.w2a;
    }
    if before.size != after.size {
        m[node].unk_30 = after.size;
    }
}

/// What the file search found: the path without and with suffix, and what it says about
/// where.
struct Found {
    path: Vec<u8>,
    path_ext: Vec<u8>,
    sub_dir: Option<Vec<u8>>,
    fallback: bool,
}

// adapter: not in place
/// `SI_dbFileFindInChapter` (0x08126420) for an element of the image: the search is
/// `si-db`'s, on the name record's value and the chapter's sub-directories as values. What
/// it changes in the name record is written back; the two paths it leaves in its work
/// buffers (`SI_dBWorkFname`, `SI_dBWorkFnameExt`) are stored there; the element's
/// "shared" mark is set when the file lies in a shared directory. `Err` holds the path
/// tried last, which is then in `SI_dBWorkFnameExt`.
fn file_find_in_chapter(m: &mut Mem, fs: &dyn FileSystem, info: &mut DbInfo, l: Ptr32<ElemList>, chap: i32, e: Ptr32<Elem>) -> Option<Result<Found, Vec<u8>>> {
    let node_ptr = m[e].node;
    let before = dbnode::to_value(m, node_ptr)?;
    let mut node = before.clone();
    let chap_name = m.cstr(m[l].name.addr());
    let sub_dirs = sub_dirs_of(m, l);
    let found = si_db::file_find_in_chapter(fs, info, &chap_name, chap, &sub_dirs, &mut node);
    write_back(m, node_ptr, &before, &node);
    Some(match found {
        None => {
            let suffix = m.cstr(dbnode::ed_get_suffix_from_chapter(m, chap).addr());
            let tried = last_path_tried(info, &suffix, &sub_dirs, &node);
            set_work_name(m, SI_DB_WORK_FNAME_EXT, &tried);
            Err(tried)
        }
        Some(f) => {
            set_work_name(m, SI_DB_WORK_FNAME, &f.path);
            set_work_name(m, SI_DB_WORK_FNAME_EXT, &f.path_ext);
            if f.share {
                m[e].shared = 1;
            }
            // found under the unversioned name: the path does not hold the full name
            let fallback = !f.path.ends_with(before.full.as_deref().unwrap_or(si_db::NULL_TEXT));
            Ok(Found { path: f.path, path_ext: f.path_ext, sub_dir: f.sub_dir, fallback })
        }
    })
}

/// Puts something in the place of an element whose file is missing or was refused
/// [soft 0x0810f948 SI_elemdictReplaceMissingElement]. Nothing is done for an element that
/// has an object.
///
/// Two status messages: "Couldn't read file <path>" (level 3; the path is the one the file
/// search left in `SI_dBWorkFnameExt`) and, except for a cluster, "Missing <chapter>
/// [<full name>] …": "could not be replaced: removed from scene" for cameras, lattices,
/// shapes, waves and animation, "has been replaced by a default element" for everything
/// else.
///
/// | Chapter | What happens |
/// |---|---|
/// | MODELS | every element of the hierarchy gets a cube ([`replace_missing_hierarchy`]) |
/// | MATERIALS, TEXTURES2D, TEXTURES3D | a copy of the default material or texture |
/// | LIGHTS | a new light; a light whose interest is another element gets a spot and shares it with that element |
/// | CLUSTERS | the model's cluster whose name ends the element's name becomes the object; else "could not be replaced" |
/// | CAMERAS, LATTICES, SHAPES, WAVES, SPREADSHEET_SETUP, ANIMATION | the element is deleted |
/// | others | nothing |
///
/// The default objects belong to packages that are not ported: the calls are recorded and
/// the element gets an [`Object::Foreign`].
pub fn replace_missing_element(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>, index: i32, un: &mut dyn Unported) {
    let cid = cid(m);
    let id = ElemId::new(chap, index);
    if !m[e].data.is_null() {
        return;
    }
    let path = m.cstr(SI_DB_WORK_FNAME_EXT.addr());
    sc.status(3, concat(&[b"Couldn't read file ", &path]));
    let l = list(m, ed, chap);
    let chap_name = m.cstr(m[l].name.addr());
    let full = or_null(m, m[m[e].node].fullname);
    let removed = [cid.cam, cid.lat, cid.shp, cid.wav, cid.ani].contains(&chap);
    if chap != cid.cls {
        let tail: &[u8] = if removed { b"] could not be replaced: removed from scene" } else { b"] has been replaced by a default element" };
        sc.status(2, concat(&[b"Missing ", &chap_name, b" [", &full, tail]));
    }
    let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
    let foreign = |m: &mut Mem, sc: &mut Scene, made_by: &'static str| {
        let h = sc.objects.alloc(Object::Foreign { made_by, path: path.clone() });
        m[e].data = sc.obj_addr(h);
        elemdict::sort_tbl_set_dirty(m, l);
        h
    };
    if chap == cid.model {
        replace_missing_hierarchy(m, sc, ed, None, e, index, un);
        un.call("SI_modelUpdateSonState", &[Arg::Elem(id), Arg::Int(1)]);
    } else if chap == cid.mat {
        foreign(m, sc, "SI_materialCopy(SI_defaultMaterial)");
        un.call("SI_edSoftElemChangeName", &[Arg::Elem(id), Arg::Str(name)]);
    } else if chap == cid.t2d {
        foreign(m, sc, "SI_txt2dCopy(SI_defaultMap)");
        un.call("SI_edSoftElemChangeName", &[Arg::Elem(id), Arg::Str(name)]);
    } else if chap == cid.t3d {
        foreign(m, sc, "SI_txt3dCopy(SI_defaultTxt3d)");
        un.call("SI_edSoftElemChangeName", &[Arg::Elem(id), Arg::Str(name)]);
    } else if chap == cid.lig {
        un.call("SI_edSoftElemChangeName", &[Arg::Elem(id), Arg::Str(name)]);
        // SI_edGetLigFromLigInt (0x080fd150, edtools.c): the element related to this one by
        // 2110 in list 1 on the backward side, or this element itself
        let other = find_related(m, ed, e, 0, 1, REL_LIG_INT, cid.lig, true);
        match other {
            Some((o, oid)) if oid.index != index => {
                let h = foreign(m, sc, "SI_newLight(2)");
                if m[o].status != ELEM_DELETED {
                    m[o].data = sc.obj_addr(h);
                }
            }
            _ => {
                foreign(m, sc, "SI_newLight(1)");
            }
        }
    } else if chap == cid.cls {
        // the cluster of the related model whose name ends the element's name
        let related = find_related(m, ed, e, 0, 0, 700, cid.model, true);
        let mut found = false;
        if let Some((mp, _)) = related {
            if let Some(Object::ModelNode { tree, node }) = sc.objects.get(sc.obj(m[mp].data)).cloned() {
                let clusters: Vec<Vec<u8>> = sc.models.get(tree).and_then(|t| t.node(node)).map(|n| n.clusters.iter().map(|c| si_hrc::io::c_str(&c.name).to_vec()).collect()).unwrap_or_default();
                if clusters.iter().any(|c| c.len() < name.len() && name.ends_with(c)) {
                    foreign(m, sc, "the model's cluster of that name");
                    found = true;
                }
            }
        }
        let tail: &[u8] = if found { b"] has been replaced by a default element" } else { b"] could not be replaced: removed from scene" };
        sc.status(2, concat(&[b"Missing ", &chap_name, b" [", &full, tail]));
    } else if removed || chap == cid.spd {
        elemdict::set_delete_elem(m, &mut Out { sc, un }, ed, chap, e, index, -1, 0, 1);
    }
}

/// A cube for every element of a hierarchy whose file is missing
/// [soft 0x0810f710 elemdictReplaceMissingHierarchy]: a new model with a cube mesh, placed
/// at the element's schematic position (relative to its parent's), made a son of the
/// parent's model; the element's relations 112, 120, 121, 200, 700, 220, 221, 230, 231 and
/// 240 are removed (what a cube cannot honour); then the same for each element that names
/// this one as its parent. The models belong to a package that is not ported: recorded.
fn replace_missing_hierarchy(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, parent: Option<ElemId>, e: Ptr32<Elem>, index: i32, un: &mut dyn Unported) {
    let model = m[SI_CID].model;
    let id = ElemId::new(model, index);
    if m[e].data.is_null() {
        let h = sc.objects.alloc(Object::Foreign { made_by: "SI_modelAllocate + SI_meshCube", path: Vec::new() });
        m[e].data = sc.obj_addr(h);
        let l = list(m, ed, model);
        elemdict::sort_tbl_set_dirty(m, l);
        un.call("SI_modelAllocate / SI_meshCube / SI_modelAddDefinition", &[Arg::Elem(id)]);
        if let Some(p) = parent {
            un.call("SI_modelCreateParentship", &[Arg::Elem(p), Arg::Elem(id)]);
        }
        for relid in [0x70, 0x78, 0x79, 200, 700, 0xdc, 0xdd, 0xe6, 0xe7, 0xf0] {
            elemrel::remove_relation_id(m, &mut Out { sc, un }, ed, SI_RD, index, relid);
        }
        let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
        un.call("SI_edSoftElemChangeName", &[Arg::Elem(id), Arg::Str(name)]);
    }
    un.call("SI_edFindSon", &[Arg::Elem(id)]);
    let mut son = find_related(m, ed, e, 0, 1, REL_PARENT, model, true);
    while let Some((s, sid)) = son {
        replace_missing_hierarchy(m, sc, ed, Some(id), s, sid.index, un);
        son = find_related(m, ed, e, 0, 1, REL_PARENT, model, false);
    }
}

/// Finds and reads the file of one element [soft 0x08127b00 elemdictReadAnElement].
///
/// 1. Except for a named selection (which has no file), the file is looked for
///    (`SI_dbFileFindInChapter`): the named version in the element's own sub-directory, in
///    the chapter itself, in each sub-directory of the chapter; then the same places again
///    under the unversioned name. If it is nowhere, [`replace_missing_element`] and done.
///    If it was found in another sub-directory, the name record's sub-directory is freed
///    and becomes a copy of that one.
/// 2. The reader of the element's chapter is called (the table in finding 0018) with the
///    path the search left in its work buffer.
/// 3. A model, material, texture, animation, lattice, light or camera file whose reader
///    returns nothing is reported ("Invalid file header or content: <path>", level 3, and a
///    pause of one second) and replaced like a missing one.
/// 4. In a chapter kept in a database, the name record takes the file's modification time
///    and the database's path.
///
/// False only when a lattice file gave its model no lattice: the caller then deletes the
/// element.
///
/// The readers are the packages' own, on values: what they return is put in
/// [`Scene::objects`] and the element holds its address.
#[allow(clippy::too_many_arguments)]
pub fn read_an_element(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>, index: i32, un: &mut dyn Unported) -> bool {
    let cid = cid(m);
    let id = ElemId::new(chap, index);
    let l = list(m, ed, chap);
    let (mut path, mut path_ext) = (Vec::new(), Vec::new());
    let (mut fallback, mut in_sub_dir) = (false, false);
    if chap != cid.nsl {
        match file_find_in_chapter(m, fs, info, l, chap, e) {
            // (an element without a name record: the original reads through the null pointer)
            None => return true,
            Some(Err(tried)) => {
                report(m, sc, ed, id, Outcome::Missing { tried });
                replace_missing_element(m, sc, ed, chap, e, index, un);
                return true;
            }
            Some(Ok(f)) => {
                fallback = f.fallback;
                if let Some(sd) = &f.sub_dir {
                    let node = m[e].node;
                    let old = m[node].str_08;
                    if !old.is_null() {
                        m.free(old.addr());
                        m[node].str_08 = Ptr32::NULL;
                    }
                    m[node].str_08 = dbnode::new_string(m, sd);
                    in_sub_dir = true;
                }
                path = f.path;
                path_ext = f.path_ext;
            }
        }
    }
    elemdict::sort_tbl_set_dirty(m, l);
    let read_ok = |path: &[u8], complete: bool| Outcome::Read { path: path.to_vec(), fallback, sub_dir: in_sub_dir, complete };
    let data = fs.read(&path_ext);
    let mut invalid = false;
    let mut outcome: Option<Outcome> = None;
    let checked = [cid.model, cid.mat, cid.t2d, cid.t3d, cid.ani, cid.lat, cid.lig, cid.cam].contains(&chap);
    if chap == cid.model {
        // [soft 0x0810fdd4 SI_elemdictReadModel]
        match data.as_deref().and_then(|d| si_hrc::read_file(d, si_hrc::Kind::Model)) {
            Some(si_hrc::File { content: si_hrc::Content::Model(root), end, .. }) => {
                let complete = end == icon_limit(data.as_deref().unwrap());
                let tree = sc.models.alloc(ModelTree::new(root));
                let handle = sc.objects.alloc(Object::ModelNode { tree, node: 0 });
                sc.models.get_mut(tree).unwrap().handles[0] = handle;
                // the hierarchy in the image: SI_elemdictReadModel(&SI_dBWorkFname)
                crate::image::read_model(m, sc, un, fs, tree, &path_ext);
                m[e].data = sc.obj_addr(handle);
                if sc.models.get(tree).unwrap().kinds[0] == 1 {
                    un.call("fixPatch / SI_patchUpdateSurface (if the patch is of type 1 x 1)", &[Arg::Ptr(handle)]);
                }
                report(m, sc, ed, id, read_ok(&path_ext, complete));
                set_data_from_hierarchy(m, sc, ed, l, e, e, index, tree, true, un);
            }
            _ => invalid = true,
        }
    } else if chap == cid.ani || chap == cid.sky {
        // SI_elemdictReadAnimations (`si-elemio`), for animation and struct key files
        match &data {
            Some(d) => {
                let mut r = Reader::new(d);
                r.st = sc.statics.clone();
                let f = anim::read_file(&mut r, 0);
                sc.statics = r.st.clone();
                if f.magic.is_some() {
                    let complete = f.hier_end == si_elemio::End::Zero && f.items_end == si_elemio::End::Zero && f.pos == icon_limit(d);
                    let h = sc.objects.alloc(Object::Animation(Box::new(f.anim)));
                    // the file in the image: SI_elemdictReadAnimations(dictionary, &SI_dBWorkFnameExt)
                    crate::image::read_animations(m, sc, un, fs, ed, h, &path_ext);
                    m[e].data = sc.obj_addr(h);
                    outcome = Some(read_ok(&path_ext, complete));
                } else {
                    invalid = true;
                }
            }
            None => invalid = true,
        }
    } else if chap == cid.shp {
        // SI_elemdictReadShape onto the model the shape belongs to (relation 200 on the
        // backward side); the element then points at the model. The file is read twice: into
        // values (`si-elemio`'s reader, `Scene::shapes`) and onto the model's node in the image
        // (`crate::image::read_shape`, `si-hrc`'s SI_shapeBinaryRead; off with
        // `SCENE_NO_SHAPES_ON_IMAGE=1`)
        match find_related(m, ed, e, 0, 0, 200, cid.model, true) {
            Some((mp, _)) => {
                let mdata = m[mp].data;
                crate::image::read_shape(m, sc, un, fs, mdata.addr(), &path_ext);
                let kind = match sc.objects.get(sc.obj(mdata)) {
                    Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).map(|t| t.kinds[*node]),
                    _ => None,
                };
                if let (Some(d), Some(kind)) = (&data, kind) {
                    let mut r = Reader::new(d);
                    r.st = sc.statics.clone();
                    let mut model = shape::ShapeModel::new(kind);
                    let res = shape::read_file(&mut r, &mut model);
                    sc.statics = r.st.clone();
                    let complete = res.accepted && res.end == si_elemio::End::Zero;
                    sc.shapes.push(ShapeOfModel { model: sc.obj(mdata), shapes: model, accepted: res.accepted });
                    outcome = Some(read_ok(&path_ext, complete));
                } else {
                    outcome = Some(Outcome::NoModel);
                }
                m[e].data = mdata;
            }
            None => outcome = Some(Outcome::NoModel),
        }
    } else if chap == cid.cls {
        // SI_elemdictReadClusters (0x08110144) onto the model (relation 700); the
        // element itself gets no object here
        match find_related(m, ed, e, 0, 0, 700, cid.model, true) {
            None => {
                sc.messages.push(Message { channel: Channel::Stdout, text: si_rt::message(0x0868_3020).to_vec() });
                outcome = Some(Outcome::NoModel);
            }
            Some((mp, mid)) => {
                let mdata = sc.obj(m[mp].data);
                let kind = match sc.objects.get(mdata) {
                    Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).map_or(0, |t| t.kinds[*node]),
                    _ => 0,
                };
                // the original names the file `<path>.cls` and looks at its start: a text
                // tag sends it to the old text reader (`SI_oclusterAsciiRead`, not ported)
                if let Some(d) = &data {
                    let magic = si_db::file_read_magic(&mut si_file::Stream::new(d));
                    if magic.ascii {
                        un.call("SI_oclusterAsciiRead", &[Arg::Str(path.clone()), Arg::Elem(mid)]);
                        outcome = Some(Outcome::Located { path: path_ext.clone(), reader: "SI_oclusterAsciiRead" });
                    } else {
                        let mut r = Reader::new(d);
                        r.st = sc.statics.clone();
                        let mut geom = si_elemio::geom::GeomState::default();
                        // the reader works on the model's clusters: their names and numbers
                        // of points are the model file's, the key data is kept beside the
                        // model (`Scene::clusters`)
                        let place = crate::scnrel::clusters_of_model(sc, mdata);
                        let (mut klusters, mut an) = match place {
                            Some(p) => (std::mem::take(&mut sc.clusters[p].klusters), sc.clusters[p].anim),
                            None => (Vec::new(), None),
                        };
                        let f = cluster::read_file(&mut r, kind, &mut klusters, &mut an, &mut geom);
                        if let Some(p) = place {
                            sc.clusters[p].klusters = klusters;
                            sc.clusters[p].anim = an;
                            if f.read.current.is_some() {
                                sc.clusters[p].current = f.read.current.clone();
                            }
                        }
                        sc.statics = r.st.clone();
                        outcome = Some(read_ok(&path_ext, f.read.ok && f.read.end == si_elemio::End::Zero));
                    }
                }
            }
        }
    } else if chap == cid.lig {
        // SI_elemdictReadLight (`si-elemio`); the light is shared with the element
        // related by 2110 in list 1 on the backward side
        match &data {
            Some(d) => {
                let mut r = Reader::new(d);
                r.st = sc.statics.clone();
                let f = light::read_file(&mut r, sc.answers.now);
                sc.statics = r.st.clone();
                let complete = f.read.end == si_elemio::End::Zero && f.pos == icon_limit(d);
                match f.read.light {
                    Some(lgt) => {
                        let h = sc.objects.alloc(Object::Light(Box::new(lgt)));
                        // the light in the image: SI_elemdictReadLight(&SI_dBWorkFnameExt)
                        crate::image::read_light(m, sc, un, fs, h, &path_ext);
                        m[e].data = sc.obj_addr(h);
                        if let Some((o, oid)) = find_related(m, ed, e, 0, 1, REL_LIG_INT, cid.lig, true) {
                            m[o].data = sc.obj_addr(h);
                            report(m, sc, ed, oid, Outcome::PartOfRoot);
                        }
                        outcome = Some(read_ok(&path_ext, complete));
                    }
                    None => invalid = true,
                }
            }
            None => invalid = true,
        }
    } else if chap == cid.lat {
        // SI_elemdictReadLattice (`si-elemio`) onto the model related by 210 (its node
        // lattices) or else 215 (its branch lattices); the element points at the model's
        // first lattice of that list
        let mut branch = false;
        let mut related = find_related(m, ed, e, 0, 0, 0xd2, cid.model, true);
        if related.is_none() {
            related = find_related(m, ed, e, 0, 0, 0xd7, cid.model, true);
            branch = true;
        }
        match (related, &data) {
            (Some((mp, _)), Some(d)) => {
                let mut r = Reader::new(d);
                r.st = sc.statics.clone();
                let f = lattice::read_file(&mut r, true);
                sc.statics = r.st.clone();
                let complete = f.read.end == si_elemio::End::Zero;
                // every lattice of the file goes into the model's list (node lattices or
                // branch lattices), behind what is there; the writer writes that list
                let mdata = sc.obj(m[mp].data);
                match sc.lattices.iter_mut().find(|x| x.model == mdata && x.branch == branch) {
                    Some(x) => x.list.extend(f.read.lattices.iter().cloned()),
                    None => sc.lattices.push(crate::object::LatticesOfModel { model: mdata, branch, list: f.read.lattices.clone() }),
                }
                match f.read.lattices.into_iter().next() {
                    Some(lat) => {
                        let h = sc.objects.alloc(Object::Lattice(Box::new(lat)));
                        // the lattices in the image, onto the model's node there:
                        // SI_elemdictReadLattice(&SI_dBWorkFnameExt, branch, node)
                        let node = m[mp].data.addr();
                        crate::image::read_lattice(m, sc, un, fs, h, node, branch, &path_ext);
                        m[e].data = sc.obj_addr(h);
                        outcome = Some(read_ok(&path_ext, complete));
                    }
                    None => {
                        report(m, sc, ed, id, Outcome::Invalid { path: path_ext.clone() });
                        return false;
                    }
                }
            }
            (Some(_), None) => invalid = true,
            (None, _) => {
                outcome = Some(Outcome::NoModel);
                invalid = true;
            }
        }
    } else if chap == cid.cam {
        // SI_elemdictReadCamera (`si-elemio`): every camera file is read into the
        // scene's one camera, and the element points at it
        match &data {
            Some(d) => {
                let mut r = Reader::new(d);
                r.st = sc.statics.clone();
                let handle = sc.camera;
                let mut cam = match sc.objects.get(handle) {
                    Some(Object::Camera(c)) => (**c).clone(),
                    _ => camera::Camera::perspective_default(),
                };
                let f = camera::read_file(&mut r, &mut cam);
                sc.statics = r.st.clone();
                if let Some(Object::Camera(c)) = sc.objects.get_mut(handle) {
                    **c = cam;
                }
                // into the scene's camera of the image: SI_elemdictReadCamera(&SI_dBWorkFnameExt)
                crate::image::read_camera(m, sc, un, fs, &path_ext);
                m[e].data = sc.obj_addr(handle);
                outcome = Some(read_ok(&path_ext, f.end == si_elemio::End::Zero && f.pos == icon_limit(d)));
            }
            None => invalid = true,
        }
    } else if chap == cid.mat || chap == cid.t2d || chap == cid.t3d {
        // [soft 0x0810fef0 SI_elemdictReadMaterial] [soft 0x0810ff08 SI_elemdictReadTexture2D]
        // [soft 0x0810ff20 SI_elemdictReadTexture3D]
        let kind = if chap == cid.mat {
            si_hrc::Kind::Material
        } else if chap == cid.t2d {
            si_hrc::Kind::Texture
        } else {
            si_hrc::Kind::Texture3D
        };
        match data.as_deref().and_then(|d| si_hrc::read_file(d, kind).map(|f| (f, icon_limit(d)))) {
            Some((f, limit)) => {
                let complete = f.end == limit;
                let obj = match f.content {
                    si_hrc::Content::Material(x) => Some(Object::Material(Box::new(x))),
                    si_hrc::Content::Texture(t) => Some(Object::Texture2D(Box::new(t))),
                    si_hrc::Content::Texture3D(t) => Some(Object::Texture3D(Box::new(t))),
                    _ => None,
                };
                match obj {
                    Some(o) => {
                        let h = sc.objects.alloc(o);
                        // the record in the image: SI_elemdictReadMaterial, …Texture2D or
                        // …Texture3D (&SI_dBWorkFname)
                        let which = if chap == cid.mat {
                            crate::image::Matter::Material
                        } else if chap == cid.t2d {
                            crate::image::Matter::Texture2D
                        } else {
                            crate::image::Matter::Texture3D
                        };
                        crate::image::read_matter(m, sc, un, fs, which, h, &path_ext);
                        m[e].data = sc.obj_addr(h);
                        if chap == cid.t2d {
                            // SI_texture2DVerifyPicturePath(&SI_dbdir, database, dictionary,
                            // element, 0, 0) on the texture record of the image (`t2dpath.rs`;
                            // the list of databases is si-db's value)
                            let map = m[e].data.addr();
                            if map != 0 && si_mem::handle::Kind::of(map).is_none() {
                                let mut cx = crate::t2dpath::Ctx { sc: &mut *sc, fs, info: &mut *info, un: &mut *un };
                                crate::t2dpath::verify_in_load(&mut cx, m, ed, e);
                                *sc.contents.ported.entry("SI_texture2DVerifyPicturePath").or_default() += 1;
                            } else {
                                un.call("SI_texture2DVerifyPicturePath", &[Arg::Elem(id), Arg::Int(0), Arg::Int(0)]);
                            }
                        }
                        outcome = Some(read_ok(&path_ext, complete));
                    }
                    None => invalid = true,
                }
            }
            None => invalid = true,
        }
    } else if [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon]
        .contains(&chap)
    {
        // SI_elemdictReadShader (0x0810ff38): a shader record initialised from the file
        // (`SI_udfAllocate`, `SI_udfInitialize`, not ported); it learns its element's index
        let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
        un.call("SI_udfInitialize", &[Arg::Str(name), Arg::Str(path_ext.clone()), Arg::Chap(chap)]);
        let h = sc.objects.alloc(Object::Foreign { made_by: "SI_udfInitialize", path: path_ext.clone() });
        m[e].data = sc.obj_addr(h);
        outcome = Some(Outcome::Located { path: path_ext.clone(), reader: "SI_udfInitialize" });
    } else if chap == cid.wav {
        // [soft 0x0810ff7c SI_elemdictReadWave]: `SI_dbwaveAsciiRead` on the image (`wave.rs`,
        // finding 0018, section 20); the element's object is the last wave of the file (the
        // stores keep a value tied to it, for the saver)
        let wave = {
            let mut o = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: Some(fs), db: None };
            crate::wave::db_wave_ascii_read(&mut o, m, data.as_deref())
        };
        let h = sc.objects.alloc(Object::Foreign { made_by: "SI_dbwaveAsciiRead", path: path_ext.clone() });
        if wave != 0 {
            m[e].data = Ptr32::new(wave);
            sc.tie(h, wave);
            *sc.contents.ported.entry("SI_dbwaveAsciiRead").or_default() += 1;
            outcome = Some(read_ok(&path_ext, true));
        } else {
            m[e].data = Ptr32::NULL;
            outcome = Some(Outcome::Located { path: path_ext.clone(), reader: "SI_dbwaveAsciiRead" });
        }
    } else if chap == cid.exp {
        // SI_elemdictReadExpressions (0x0810ff94): the file holds one `EXPRESSION <full
        // name>` block per expression; each is read into the element of that name, whose
        // object becomes the expression's tree. `si-expr`'s reader on the image (its
        // scanner and parser run on values: `// adapter: not in place` there).
        match data.as_deref().map(|d| si_expr::image::io::elemdict_read_expressions(m, ed, d)) {
            Some(Ok(c)) => {
                // the stores have no expression trees: each tree record of the chapter gets
                // a value of its own (in the order of the records' addresses, as the image's
                // look-ups by object order them), so that the stores' elements point at
                // something where the image's do
                let l = list(m, ed, chap);
                let mut trees: Vec<u32> = (0..m[l].nb_elems).map(|i| m[m[l].elems.add(i)].data.addr()).filter(|&a| a != 0 && !sc.contents.handle_at.contains_key(&a)).collect();
                trees.sort_unstable();
                trees.dedup();
                for a in trees {
                    let h = sc.objects.alloc(Object::Foreign { made_by: "SI_elemdictReadExpressions", path: path_ext.clone() });
                    sc.tie(h, a);
                }
                *sc.contents.ported.entry("SI_elemdictReadExpressions").or_default() += 1;
                *sc.contents.records.entry("expressions read").or_default() += c.stored;
                *sc.contents.records.entry("expressions the parser refused").or_default() += c.refused;
                outcome = Some(read_ok(&path_ext, true));
            }
            Some(Err(_)) => {
                *sc.contents.records.entry("expression files read to an unfinished block").or_default() += 1;
                outcome = Some(read_ok(&path_ext, false));
            }
            None => outcome = Some(Outcome::Missing { tried: path_ext.clone() }),
        }
    } else if chap == cid.sts || chap == cid.stc || chap == cid.sfk {
        // SI_elemdictReadSetup, SI_elemdictReadChannelSetup, SI_elemdictReadParticleSetup
        // (0x08111b00, 0x08111b34, 0x08111b64): the three setups load into program globals.
        // `si-ascii` has the readers; here the file is read to see that it reads, and what
        // it would set is dropped; the element gets no object
        let complete = match &data {
            Some(d) if chap == cid.sts => {
                // Of what the setup reader stores in program globals, the image gets the
                // preferences the update and the renderer read (`preferences_to_image`):
                // among them `SI_rightHand` (0x086793ec, a short), which `prefMenuRead`
                // (0x083e3768) sets through `SI_setPrefFlag (0x08416e7c)(0xa0, 6, value)`,
                // whose case 6 stores the value's low 16 bits there (listing). The value
                // the stores keep is read as before.
                preferences_to_image(m, d);
                save_options_from_setup(sc, d);
                match si_ascii::setup::read(d) {
                    Ok((s, _)) => {
                        sc.setup = Some(Box::new(s));
                        true
                    }
                    Err(_) => false,
                }
            }
            Some(d) if chap == cid.stc => match si_ascii::chn::read(d) {
                Ok((s, _)) => {
                    sc.channel_setup = Some(Box::new(s));
                    true
                }
                Err(_) => false,
            },
            Some(d) => match si_ascii::flk::read(d) {
                Ok((s, _)) => {
                    sc.flock_setup = Some(Box::new(s));
                    true
                }
                Err(_) => false,
            },
            None => false,
        };
        outcome = Some(read_ok(&path_ext, complete));
    } else if chap == cid.spd {
        un.call("SI_sprdSetupRead", &[Arg::Str(path_ext.clone())]);
        outcome = Some(Outcome::Located { path: path_ext.clone(), reader: "SI_sprdSetupRead" });
    } else if chap == cid.nsl {
        let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
        let h = sc.objects.alloc(Object::NSelection { name });
        m[e].data = sc.obj_addr(h);
        outcome = Some(Outcome::NotRead);
    } else {
        // any other chapter: found, and nothing is read
        report(m, sc, ed, id, Outcome::Located { path: path_ext.clone(), reader: "none" });
        return true;
    }
    if invalid && checked && m[e].data.is_null() {
        sc.status(3, concat(&[b"Invalid file header or content: ", &path_ext]));
        un.call("sleep", &[Arg::Int(1)]);
        report(m, sc, ed, id, Outcome::Invalid { path: path_ext.clone() });
        replace_missing_element(m, sc, ed, chap, e, index, un);
        return true;
    }
    if let Some(o) = outcome {
        report(m, sc, ed, id, o);
    }
    if m[l].kind == 1 {
        let mtime = fs.stat(&path_ext).map_or(0, |s| s.mtime);
        // +0x00 of the name record: the path of the database the element was read from (a
        // pointer into the database record, not a copy): `Scene::db_mark`
        let mark = sc.db_mark(info);
        let node = m[e].node;
        if !node.is_null() {
            m[node].time = mtime;
            m[node].unk_00 = mark;
        }
    }
    true
}

/// Where the data of an element file ends: at an appended icon (finding 0003), or at the
/// end of the file. Looked at only by the checks, to say whether a reader took a file to
/// its exact end.
fn icon_limit(data: &[u8]) -> usize {
    match si_picture::icon_offset(data) {
        Some(off) if off <= data.len() => off,
        _ => data.len(),
    }
}

/// Moves each root element to the latest version on disk
/// [soft 0x0812c05c SI_elemdictGetLatestVersions]: for every chapter kept in a database (or
/// the one chapter given), every element in use that a load created and that has no icon
/// yet. A status line "[<old full name>] updated with version (<n>)" for each one moved,
/// then "Changed <n> elements for latest version" or "Latest versions already used for all
/// elements". Returns how many moved.
///
/// The scene loader does not call this (it asks for the versions the description names);
/// the "Get" dialog can. The search for the latest version is `si-db`'s, on the name
/// record's value; what it changes is written back into the record.
pub fn get_latest_versions(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, only_chap: i32) -> i32 {
    let mut changed = 0;
    let session = Session { fs, now: sc.answers.now, loading: m[LOADING] != 0 };
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let l = list(m, ed, chap);
        if (only_chap == -1 || only_chap == chap) && m[l].kind == 1 {
            let name = m.cstr(m[l].name.addr());
            info.set_chapter(&name);
            info.filepath_build();
            let mut i = 0;
            while i < m[l].nb_elems {
                let e = m[l].elems.add(i);
                i += 1;
                if m[e].status != ELEM_VALID || m[e].new_flag != 1 || !m[e].icon.is_null() {
                    continue;
                }
                let node_ptr = m[e].node;
                let Some(before) = dbnode::to_value(m, node_ptr) else { continue };
                let mut node = before.clone();
                // `sscanf(version, "%d", &v)`: unset when the text is not a number; 0 here
                let was = si_db::scan_int(node.version.as_deref().unwrap_or(si_db::NULL_TEXT)).map_or(0, |(v, _)| v);
                let mut log = Log::default();
                let (latest, _, _) = si_db::file_find_elem_latest_version(&session, info, &mut node, true, &mut log);
                write_back(m, node_ptr, &before, &node);
                if latest > -1 && latest != was {
                    sc.status(2, concat(&[b"[", before.full.as_deref().unwrap_or(si_db::NULL_TEXT), b"] updated with version (", latest.to_string().as_bytes(), b")"]));
                    changed += 1;
                }
            }
        }
        chap += 1;
    }
    if changed < 1 {
        sc.status(2, si_rt::message(0x0868_3500).to_vec());
    } else {
        sc.status(2, format!("Changed {changed} elements for latest version").into_bytes());
    }
    changed
}

/// One element of the ANIMATION chapter per curve of each animation file just read
/// [soft 0x0812867c SI_elemdictExpandAnimations]: [`expand_an_animation`] for each root
/// animation element of this load (new-flag 1, depth 0).
///
/// The new elements have new-flag 2, so the loop, which runs over the chapter as it grows,
/// passes over them.
pub fn expand_animations(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, un: &mut dyn Unported) {
    let mut i = 0;
    loop {
        let ani = m[SI_CID].ani;
        let l = list(m, ed, ani);
        if i >= m[l].nb_elems {
            break;
        }
        let e = m[l].elems.add(i);
        if m[e].new_flag == 1 && m[e].hier_level == 0 {
            expand_an_animation(m, sc, ed, e, i, un);
        }
        i += 1;
    }
}

/// The curves of one animation file as elements [soft 0x081284a0 SI_elemdictExpandAnAnimation].
/// Nothing for an element without an object.
///
/// For each element of the file's hierarchy animation the element's name is split as a
/// file name is (`dbfilenameExpand`, into five buffers of the routine's own); then for
/// each of its items (curves and static values alike) a name record is made
/// (`SI_dbListNodeAllocate`): its prefix a copy of the *name part* of the hierarchy
/// element's name, its name a copy of the name of the item's curve id (`SI_fcvGetName`),
/// its version and substitution number copies of the animation file's (the two the new
/// record had are freed), the time, and its full name. The new element's object is the
/// item, its depth one more than the file's, its root index the file's element.
///
/// (An item that holds nothing, of a type other than 0 and 1, makes the original read the
/// id through a null pointer; here it gets the name of id 0. A curve id without a name
/// and an animation element without version or substitution number make it copy a null
/// string; here the record's string stays null.)
fn expand_an_animation(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, index: i32, un: &mut dyn Unported) {
    let data = m[e].data;
    if data.is_null() {
        return;
    }
    let node = m[e].node;
    let (version, subst) = (m[node].version, m[node].subst);
    let level = m[e].hier_level;
    let anim_handle = sc.obj(data);
    // the file's `SI_HierAnim` in the image, when the reader on the image made it
    // (`image::read_animations`): its element count (+0x00), its elements (+0x04, 0x28
    // bytes each: the name +0x00, the count of items +0x08, the items' array +0x18)
    let anim_rec = if world::obj_of(data) == 0 { data.addr() } else { 0 };
    let curves: Vec<(Vec<u8>, Vec<i32>)> = match sc.objects.get(anim_handle) {
        Some(Object::Animation(a)) => a
            .elems
            .iter()
            .map(|he| {
                let ids = he
                    .items
                    .iter()
                    .map(|it| match &it.data {
                        anim::ItemData::Curve(c) => c.curve_type,
                        anim::ItemData::Static(v) => v.curve_type,
                        anim::ItemData::None => 0,
                    })
                    .collect();
                (he.name.clone(), ids)
            })
            .collect(),
        _ => Vec::new(),
    };
    for (k, (name, ids)) in curves.into_iter().enumerate() {
        // prefix, name, version, substitution, suffix: 256, 256, 12, 12 and 12 bytes
        let suff = m.local(12);
        let prec = m.local(12);
        let vers = m.local(12);
        let nam = m.local(0x100);
        let proj = m.local(0x100);
        with_text(m, &name, |m, p| dbnode::filename_expand(m, p, [proj, nam, vers, prec, suff]));
        for (item, id) in ids.into_iter().enumerate() {
            let n = dbnode::list_node_allocate(m, &mut Out { sc, un });
            m[n].str_0c = Ptr32::NULL;
            m[n].prefix = Ptr32::new(m.strdup(nam));
            // `strdup(SI_fcvGetName(curve))`: the curve is not in the image, so the
            // routine's body is run on its parameter id. (A component without a name
            // would make the original copy a null pointer; here the name stays null.)
            let name = si_dict::attdict::get_name_from_fcurve(m, si_dict::globals::SI_ATD, id);
            m[n].name = if name.is_null() { Ptr32::NULL } else { Ptr32::new(m.strdup(name.addr())) };
            for field in [field!(n, DbListNode, version), field!(n, DbListNode, subst)] {
                let old = m[field];
                if !old.is_null() {
                    m.free(old.addr());
                    m[field] = Ptr32::NULL;
                }
            }
            if !version.is_null() {
                m[n].version = Ptr32::new(m.strdup(version.addr()));
            }
            if !subst.is_null() {
                m[n].subst = Ptr32::new(m.strdup(subst.addr()));
            }
            m[n].time = si_dict::outside::time(&mut Out { sc, un }, m);
            m[n].fullname = dbnode::format_full_name(m, n);
            // the object is the item itself: a part of the animation file's object. In the
            // image that is the word the original hands to `SI_elemdictAddElem`, the item's
            // pointer in the element's array (`*(items + 4 × item)`): the curve, or the
            // static value, the reader on the image made. It stands for the value's item
            // (`Scene::tie`), which the saver and the stores keep using. Where the file is
            // not in the image, or its element or item is not there (the two readers
            // disagree), the element keeps the value's handle and is counted.
            let h = sc.objects.alloc(Object::Curve { anim: anim_handle, elem: k, item });
            let mut item_addr = 0;
            if anim_rec != 0 && (k as i32) < m.i32(anim_rec) {
                let hel = m.u32(anim_rec + 4).wrapping_add(k as u32 * 0x28);
                if (item as i32) < m.i32(hel + 8) {
                    item_addr = m.u32(m.u32(hel + 0x18).wrapping_add(4 * item as u32));
                }
            }
            if item_addr != 0 {
                sc.tie(h, item_addr);
            } else {
                *sc.contents.not_in_image.entry("ANIMATION (a curve's item not in the image)").or_default() += 1;
            }
            let ani = m[SI_CID].ani;
            let data = sc.obj_addr(h);
            let idx = elemdict::add_elem(m, &mut Out { sc, un }, ed, ani, n, data, Ptr32::NULL, 0, 0);
            let ne = elemdict::get_elem(m, ed, ani, idx);
            m[ne].hier_level = level + 1;
            m[ne].root_id = index;
        }
        m.release(12 + 12 + 12 + 0x100 + 0x100);
    }
}

/// Everything after the scene's element exists [soft 0x080d9924 SI_dbLoadElementsFromDict].
///
/// `with_descriptions` is the original's third argument (read the descriptions; also "a
/// whole scene is being loaded"), `latest` its fourth (move to the latest versions),
/// `untold` its fifth. The routine's own work on the dictionary and on its globals is done
/// here; every call into a package that is not ported is made on `un`, in order, with its
/// arguments. False only when a description could not be read: nothing else is then done
/// (and the load optimisation's switch and the schematic's mark stay as they were set).
///
/// `SI_lightLicense` is a product-tier flag: the full product's path is taken
/// (`flagWrathUnsupportedElem` is not called).
#[allow(clippy::too_many_arguments)]
pub fn load_elements_from_dict(m: &mut Mem, sc: &mut Scene, fs: &dyn FileSystem, info: &mut DbInfo, ed: Ptr32<ElemDict>, with_descriptions: bool, latest: bool, untold: bool, un: &mut dyn Unported) -> bool {
    let scene = with_descriptions;
    let standalone = m[SI_STANDALONE_RENDERER] != 0;
    if !standalone && scene {
        un.call("SI_windowResetAll", &[]);
    }
    m[SI_USE_ELEM_LIST_SORT_TBL] = si_dict::outside::si_get_load_optim(&mut Out { sc, un }, m);
    m[SI_ADD_IN_DICTIONARY] = 0;
    m[LOADING] = scene as i16;
    // the schematic's "loading" mark (finding 0006: `SCHEM_UPDATE`)
    m.set_i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_4E, 1);
    if scene {
        un.call("SI_edCameraRemoveShader", &[]);
        un.call("SI_edOutputRemoveShader", &[]);
        un.call("SI_edAtmosphereRemoveShader", &[]);
        un.call("SI_userDataDispose (the scene camera's user data)", &[]);
        if !expand_descriptions(m, sc, fs, info, ed, un) {
            m[SI_ADD_IN_DICTIONARY] = 1;
            m[LOADING] = 0;
            return false;
        }
        if latest {
            get_latest_versions(m, sc, fs, info, ed, -1);
        }
    }
    read_elements(m, sc, fs, info, ed, un);
    if !scene {
        // elements fetched one by one lose their shaders: SI_edModelRemoveShader and its
        // like for materials, textures and lights, per element of this load with an
        // object, chapter by chapter in dictionary order
        let cid = cid(m);
        let mut chap = 0;
        while chap < m[ed].nb_lists {
            let l = list(m, ed, chap);
            let mut i = 0;
            while i < m[l].nb_elems {
                let e = m[l].elems.add(i);
                if !m[e].data.is_null() && m[e].status == ELEM_VALID && m[e].new_flag == 1 && [cid.model, cid.mat, cid.t2d, cid.t3d, cid.lig].contains(&chap) {
                    un.call("SI_ed<kind>RemoveShader", &[Arg::Elem(ElemId::new(chap, i))]);
                }
                i += 1;
            }
            chap += 1;
        }
    }
    let cid = cid(m);
    elemdict::update_hier_info(m, ed, cid.model, 1, REL_PARENT);
    elemdict::update_hier_info(m, ed, cid.cam, 1, REL_CAM_INT);
    elemdict::update_hier_info(m, ed, cid.lig, 1, REL_LIG_INT);
    expand_animations(m, sc, ed, un);
    // the stand-in graph, for the stores the saver reads, then the scene's graph list in
    // the image (`si-view`'s port)
    graflist_build_from_elem_dict(m, sc, ed, scene, un);
    sc.contents.graph = crate::image::build_graph(m, sc, ed, scene, un);
    if scene {
        un.call("SI_edAddModel (for each model node of the scene graph without an element)", &[]);
        // the number MPRFLG left at +0x2c goes to the flag word of the element's node, and
        // the field is cleared: chapters WAVES, CAMERAS, MODELS, NSELECTIONS, LIGHTS;
        // elements in use of this load that have an object and a node
        let mut chap = 0;
        while chap < m[ed].nb_lists {
            if [cid.wav, cid.cam, cid.model, cid.nsl, cid.lig].contains(&chap) {
                let l = list(m, ed, chap);
                for i in 0..m[l].nb_elems {
                    let e = m[l].elems.add(i);
                    // the element's node: the image's, or the stand-in's where the image's
                    // list lacks one that the original has (`Contents::standin`)
                    let standin = sc.contents.standin.get(&(chap, i)).copied();
                    if (!m[e].aux_data.is_null() || standin.is_some()) && !m[e].data.is_null() && m[e].status == ELEM_VALID && m[e].new_flag == 1 {
                        // a C cast of the float to an integer, kept as 16 bits
                        let v = crate::elemsort::c_cast_to_int(m[e].schem_bbox[0] as f64) as i16;
                        m[e].schem_bbox[0] = 0.0;
                        // into the flag word (+0x16) of the node of the image's list, and of the stand-in
                        let node: Ptr32<si_types::scene::GrafNode> = m[e].aux_data.cast();
                        if !node.is_null() {
                            m[node].flags = v as u16;
                        }
                        if let Some(n) = standin.and_then(|h| sc.graf.get_mut(h)) {
                            n.flags = v as u16;
                        }
                    }
                }
            }
            chap += 1;
        }
    }
    crate::animrel::set_animation_relations(m, sc, ed, !scene, un);
    // [soft 0x08129244 SI_elemdictRestoreActions] on the image (si-dict::dictin); its three
    // calls into the actions code are recorded by the loader's outside world
    si_dict::dictin::restore_actions(m, &mut Out { sc, un }, ed);
    un.call("si_elemdictRemoveExtraCam", &[Arg::Int(untold as i64)]);
    m[SI_ADD_IN_DICTIONARY] = 1;
    m[LOADING] = 0;
    un.call("SI_shapeCleanAll", &[]);
    crate::scnrel::ed_update_cluster_pointers(m, sc, ed);
    un.call("si_edUpdateWeightFcvNames", &[]);
    // SI_expressionsRegenerate (0x082567b4): the expressions read above tied to their
    // relations, while the elements' new flags are still 1 (`si-expr`'s port on the image)
    expressions_regenerate(m, sc, ed, un);
    if scene {
        // the stand-in first (the description's lines, `Assume`), so that the routine finds
        // a skin's relation 0xcf where the description had one
        if sc.assume.model_relations_from_description {
            for (c1, e1, c2, e2, relid, rule, active) in std::mem::take(&mut sc.dropped_lines) {
                if reldict::check_uncomplete_relation(m, SI_RD, relid) != 0 {
                    elemrel::add_uncomplete_relation(m, ed, SI_RD, c1, c2, e1, e2, relid, rule, 0, Ptr32::NULL, active as i16);
                } else {
                    elemrel::add_complete_relation(m, ed, SI_RD, e1, e2, relid, rule, 0, Ptr32::NULL, active as i16);
                }
            }
        }
        // SI_envelopRelationSet (0x084bd5a8): si-deform's port, through the loader's dispatch
        // (`crate::image::RelSide`); recorded instead under `SCENE_NO_ENVELOP_RELATION_SET=1`
        if crate::image::envelop_relation_set_on() {
            let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
            p.by_addr(m, 0x084b_d5a8, "SI_envelopRelationSet", &[ed.addr()]);
        } else {
            un.call("SI_envelopRelationSet", &[]);
        }
        // the scene's own structures are tied from the dictionary's relations, one relation
        // id at a time: chapter, list, relation id
        for (chap, group, relid) in scn_relations_order(&cid) {
            // what a save reads back, on the handles the saver keeps (the store form) …
            crate::scnrel::scn_relations_from_elem_dict(m, sc, ed, chap, group as i32, relid as i32);
            // … and the routine itself on the image, where its objects are records of it
            scn_relations_on_image(m, sc, ed, chap, group as i32, relid as i32, un);
        }
    }
    if model_update_dep_on() {
        let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
        model_update_dep(&mut p, m, ed, cid.model, update_dependency_on(), |p, name| {
            p.un.call(name, &[]);
        });
    } else {
        un.call("SI_modelUpdateDep", &[]);
    }
    if scene {
        m[LOADING] = 1;
        scn_relations_on_image(m, sc, ed, cid.model, 4, 20000, un);
        m[LOADING] = 0;
    }
    // SI_dbCheckConvertModelsMatTxt (0x08109664): the material conversion of the new
    // hierarchies (matconv.rs), through the loader's dispatch with the scene's database for
    // the picture search of the 2D textures it makes elements. `SCENE_NO_MAT_CONVERSION=1`
    // records the call instead and keeps the adapter of before: only that picture search
    // (`t2dpath::verify_node_textures`; SCENE_NO_NODE_PICTURES=1 leaves it out too)
    if std::env::var_os("SCENE_NO_MAT_CONVERSION").is_none() {
        let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: Some(fs), db: Some(&mut *info) };
        crate::matconv::db_check_convert_models_mat_txt(&mut p, m, ed.addr());
        // the store form of what it made: the records read back into values, the nodes
        // into stand-ins (`image::tie_converted`)
        crate::image::tie_converted(m, sc, ed);
    } else {
        un.call("SI_dbCheckConvertModelsMatTxt", &[]);
        if std::env::var_os("SCENE_NO_NODE_PICTURES").is_none() {
            let mut cx = crate::t2dpath::Ctx { sc: &mut *sc, fs, info: &mut *info, un: &mut *un };
            crate::t2dpath::verify_node_textures(&mut cx, m, ed);
        }
    }
    un.call("si_edUpdateUdfPointers", &[Arg::Int(scene as i64)]);
    elemmisc::remove_all_sub_scenes(m, &mut Out { sc, un }, ed);
    elemmisc::remove_all_invalid_elems(m, &mut Out { sc, un }, ed);
    if !un.dynamic_chains_update(m) {
        un.call("SI_dynamicChainsUpdate", &[]);
    }
    if scene {
        update_root_model_srt(m, sc, ed, un);
    }
    elemdict::update_hier_info(m, ed, cid.model, 1, REL_PARENT);
    elemdict::update_hier_info(m, ed, cid.cam, 1, REL_CAM_INT);
    elemdict::update_hier_info(m, ed, cid.lig, 1, REL_LIG_INT);
    toposort::sort_hier_elements(m, ed, cid.model);
    let ani_list = list(m, ed, m[SI_CID].ani);
    elemmisc::sort_anim_elements(m, &mut Out { sc, un }, ani_list);
    if scene {
        un.call("SI_cusRestoreCustomEffects", &[]);
        un.call("cusModifyRelations", &[]);
        m[LOADING] = 1;
        un.call("SI_nselectionUpdate", &[]);
        m[LOADING] = 0;
    }
    if !standalone {
        un.call("SI_schemProcessAllNewElems", &[Arg::Int(!scene as i64)]);
        un.call("SI_schemComputeHierarchiesBBox", &[Arg::Int(0), Arg::Int(0)]);
    }
    un.call("SI_chnUpdateItems", &[Arg::Int(1)]);
    if !standalone && scene {
        un.call("SI_sprdRemoveExtraSetups", &[]);
    }
    un.call("SI_chnSetupMerge", &[]);
    un.call("SI_globalMapListBuild", &[]);
    if !untold && m[ed].hier_dirty != 0 {
        si_dict::outside::si_db_check_extra_untold_relation(&mut Out { sc, un }, m, ed, 1, 0, 1);
        si_dict::outside::si_db_check_extra_untold_relation(&mut Out { sc, un }, m, ed, 0, 0, 0);
    }
    un.call("SI_sceneUVDomainInit", &[]);
    sc.renamed_duplicates += crate::dupnames::db_check_duplicate_names(m, &mut Out { sc, un }, ed);
    if !standalone {
        if scene {
            un.call("SI_windowRestoreAll", &[]);
        }
        un.call("SI_playboxUpdate", &[]);
        un.call("SI_graflistCheckMulti", &[]);
    }
    report_the_rest(m, sc, ed);
    elemmisc::set_new_flag(m, ed, 1, 0);
    toposort::topo_sort_dict(m, &mut Out { sc, un }, SI_RD, ed, field!(ed, ElemDict, upd_list), 0, 0);
    un.call("SI_sceneConstraintInit", &[]);
    un.call("SI_dopeFlagsSet", &[Arg::Int(1), Arg::Int(1), Arg::Int(1), Arg::Int(1)]);
    // a word of the animation globals is cleared (here and once more below), and the
    // schematic's mark
    m.set_u32(SI_GLB_ANIM.addr() + 0x2c, 0);
    m.set_i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_4E, 0);
    // the "this scene uses another scaling method" dialogue when the setup read on the way
    // changed the classical-scaling preference (`SI_glbManip` +0x2a); the setup reader is
    // not on the image and does not set that word
    un.call("SI_updateScalingMethod", &[Arg::Int(0)]);
    if !un.scene_refresh(m) {
        un.call("SI_sceneRefresh", &[]);
    }
    m.set_u32(SI_GLB_ANIM.addr() + 0x2c, 0);
    // the dictionary remembers the database it was loaded from (+0x48): the database
    // record is a value of `si-db` and has no address; the field is left as it is
    m[SI_USE_ELEM_LIST_SORT_TBL] = 0;
    true
}

/// `SI_expressionsRegenerate` (0x082567b4) on the image: `si-expr`'s port, with the
/// loader's dispatch for what it calls outside (`image::ExprSide`). The relations 20010
/// whose data it replaces by a new record keep their value for the saver: the new record
/// stands for the value the old one stood for (`Scene::rel_records`).
fn expressions_regenerate(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, un: &mut dyn Unported) {
    // the expression relations' data as the description held it, by entry, for the saver
    // of the stores (the regeneration gives the relations of one expression new records)
    for c in 0..m[ed].nb_lists {
        let l = list(m, ed, c);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            let side = m[e].forward;
            if side.groups.is_null() {
                continue;
            }
            for g in 0..side.nb_groups {
                let rl = m.read(side.groups.add(g));
                let nths = world::entry_nths(m, &rl);
                for k in 0..rl.nb.max(0) {
                    let r = m.read(rl.rels.add(k));
                    let h = world::rel_data_of(sc, r.data);
                    if r.relid == reldata::REL_EXP && h != 0 {
                        sc.fitting_data.insert(world::FitKey { chap: c, elem: i, group: g, relid: r.relid, other_chap: r.chapid, other: r.elemid, nth: nths[k as usize] }, h);
                    }
                }
            }
        }
    }
    let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
    si_expr::image::regen::expressions_regenerate(m, &mut crate::image::ExprSide(&mut p), ed);
    *sc.contents.ported.entry("SI_expressionsRegenerate").or_default() += 1;
    // a record the regeneration made (`SI_ExprRelData`) stands for the value its first
    // words hold, which is what the original's writer prints of it: the parameter (+0)
    // and, for 0x236, the name (+0x10); the saver of the stores reads the description's
    // value of the entry first (`world::to_stores`)
    for c in 0..m[ed].nb_lists {
        let l = list(m, ed, c);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            for side in [m[e].forward, m[e].backward] {
                if side.groups.is_null() {
                    continue;
                }
                for g in 0..side.nb_groups {
                    let rl = m.read(side.groups.add(g));
                    for k in 0..rl.nb.max(0) {
                        let rp = rl.rels.add(k);
                        let data = m[rp].data.addr();
                        if m[rp].relid == reldata::REL_EXP && data != 0 && !sc.rel_records.contains_key(&data) && si_mem::handle::Kind::of(data).is_none() {
                            let fcurve = m.i32(data);
                            let name = (fcurve == 0x236 && m.u32(data + 0x10) != 0).then(|| m.cstr(m.u32(data + 0x10)));
                            let h = sc.rel_data.alloc(RelData::Expression(reldata::Expression { fcurve, name }));
                            sc.rel_records.insert(data, h);
                        }
                    }
                }
            }
        }
    }
}

/// A new node of the scene graph for an object (stand-in for `SI_graflistAddData`,
/// `graflist.c`, not ported): the kind, the object, visible, the default wire colour.
fn graf_add(sc: &mut Scene, data: Ptr32<Opaque>, kind: i32) -> u32 {
    let mut n = si_types::scene::GrafNode::allocate();
    n.kind = kind;
    // the stand-in keeps the object as the scene's arenas number it
    n.data = Ptr32::new(sc.obj(data));
    n.wire_color = sc.answers.def_color as i16;
    sc.graf.alloc(n)
}

/// The node of an object, of a given kind or of any kind when 0 (stand-in for
/// `SI_findGlData`). The original only looks: the nodes of a model's sub-nodes, of the
/// camera and of the lights are made by the packages that make those objects. Those are not
/// ported, so a node that is not there yet is made here.
fn graf_find(sc: &mut Scene, data: Ptr32<Opaque>, kind: i32) -> u32 {
    let found = sc.graf.handles().find(|&h| sc.graf.get(h).is_some_and(|n| n.data.0 == sc.obj(data) && (kind == 0 || n.kind == kind)));
    match found {
        Some(h) => h,
        None => graf_add(sc, data, if kind == 0 { si_types::scene::GRAF_MODEL_NODE } else { kind }),
    }
}

/// [soft 0x08109d88 grafnodeElemAssoc]: the element points at the node; when a whole scene
/// is loaded the node takes the element's wire colour (16 bits), selection and display type.
/// The node is a stand-in of [`Scene::graf`]; the element is the image's.
fn grafnode_elem_assoc(m: &mut Mem, sc: &mut Scene, node: u32, e: Ptr32<Elem>, scene: bool) {
    if node == 0 {
        return;
    }
    m[e].aux_data = graf_ptr(node);
    if scene {
        let (wire, sel, typ) = (m[e].wire_color as i16, m[e].selection, m[e].wire_type);
        if let Some(n) = sc.graf.get_mut(node) {
            n.wire_color = wire;
            n.selection = sel;
            n.wire_type = typ;
        }
    }
}

/// Gives the objects of a load their nodes in the scene graph and ties each node to its
/// element (`SI_graflistBuildFromElemDict`, 0x08109ec8). Every element with new-flag 1 is
/// looked at, chapter by chapter in dictionary order, whatever its status:
///
/// | Chapter | Node |
/// |---|---|
/// | MODELS | for a model with an object and no father: a new node of kind 2 (the nodes of its sub-models come with it) |
/// | SCENES | a new node of kind 13 with no object |
/// | NSELECTIONS | a new node of kind 12 |
/// | MATERIALS | a new node of kind 14; the material learns its element's index |
/// | TEXTURES2D | a new node of kind 15; the texture learns its element's index and takes the element's name if it has none |
/// | TEXTURES3D | a new node of kind 16 |
/// | ANIMATION | a new node of kind 18 |
/// | CAMERAS | the scene camera's node: kind 5 for a root (the interest), kind 4 otherwise. **An element without an object gets the scene's camera**: this is how the camera element that is not a root gets its object |
/// | LIGHTS | for a light with an object: the light learns its element's index; its node is of kind 7 or 6 for a spot (root or not), 8 for a point light, 19 infinite, 20 sun |
/// | WAVES | for a wave with an object: its node of kind 11 |
///
/// Then every model element of the load whose model has a father is tied to that model's
/// node. When single elements are fetched instead of a scene the routine also completes
/// hierarchies and creates the camera or light of an interest
/// (`elemdictCompleteHierarchy`, `SI_edCreateObjectFromInterest`): recorded, not done.
///
/// The graph is a stand-in here ([`graf_add`], [`graf_find`]); what the routine does to the
/// dictionary is reproduced, what it writes into the objects (the material's and the
/// texture's element index) is not: the routine is therefore not counted as ported.
pub fn graflist_build_from_elem_dict(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, scene: bool, un: &mut dyn Unported) {
    use si_types::scene::*;
    let cid = cid(m);
    let has_father = |sc: &Scene, data: Ptr32<Opaque>, hier_level: i32| match sc.objects.get(sc.obj(data)) {
        Some(Object::ModelNode { node, .. }) => *node != 0,
        // a cube put in place of a missing file: a root when its element is one
        _ => hier_level != 0,
    };
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let mut i = 0;
        while i < m[list(m, ed, chap)].nb_elems {
            let id = ElemId::new(chap, i);
            let e = elem(m, ed, id);
            let (data, hier_level) = (m[e].data, m[e].hier_level);
            i += 1;
            if m[e].new_flag != 1 {
                continue;
            }
            if chap == cid.model {
                if !data.is_null() && !has_father(sc, data, hier_level) {
                    let n = graf_add(sc, data, GRAF_MODEL_ROOT);
                    grafnode_elem_assoc(m, sc, n, e, scene);
                    un.call("SI_modelUpdateDependency", &[Arg::Elem(id)]);
                    if !scene {
                        un.call("elemdictCompleteHierarchy", &[Arg::Elem(id)]);
                    }
                }
            } else if chap == cid.dsc {
                let n = graf_add(sc, Ptr32::NULL, 0xd);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.nsl {
                let n = graf_add(sc, data, 0xc);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.mat {
                // the original writes the index into the material without testing the pointer
                let n = graf_add(sc, data, GRAF_MATERIAL);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.t2d {
                let n = graf_add(sc, data, GRAF_TEXTURE_2D);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.t3d {
                let n = graf_add(sc, data, GRAF_TEXTURE_3D);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.ani {
                let n = graf_add(sc, data, 0x12);
                grafnode_elem_assoc(m, sc, n, e, scene);
            } else if chap == cid.cam {
                let kind = if hier_level == 0 { GRAF_CAMERA_ROOT } else { GRAF_CAMERA };
                let cam = sc.obj_addr(sc.camera);
                let n = graf_find(sc, cam, kind);
                if m[e].data.is_null() {
                    m[e].data = cam;
                }
                if m[e].aux_data.is_null() {
                    grafnode_elem_assoc(m, sc, n, e, scene);
                }
                if hier_level == 0 && !scene {
                    un.call("SI_edCreateObjectFromInterest", &[Arg::Elem(id)]);
                }
            } else if chap == cid.lig {
                if data.is_null() {
                    continue;
                }
                let light_kind = match sc.objects.get(sc.obj(data)) {
                    Some(Object::Light(x)) => x.kind,
                    // a light made for a missing file: SI_newLight(2) is a spot
                    Some(Object::Foreign { made_by, .. }) if made_by.contains("(2)") => 2,
                    _ => 1,
                };
                let kind = match light_kind {
                    2 => {
                        if hier_level == 0 {
                            GRAF_LIGHT_ROOT
                        } else {
                            GRAF_LIGHT
                        }
                    }
                    1 => 8,
                    3 => 0x13,
                    4 => 0x14,
                    _ => 0, // the original's variable is not set for another type
                };
                if kind == GRAF_LIGHT && scene {
                    // the light takes the element's name (30 bytes)
                    let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
                    if let Some(Object::Light(x)) = sc.objects.get_mut(sc.obj(data)) {
                        x.name = [0; 30];
                        let k = name.len().min(30);
                        x.name[..k].copy_from_slice(&name[..k]);
                    }
                }
                let n = graf_find(sc, data, kind);
                grafnode_elem_assoc(m, sc, n, e, scene);
                if kind == GRAF_LIGHT_ROOT && !scene {
                    un.call("SI_edCreateObjectFromInterest", &[Arg::Elem(id)]);
                }
            } else if chap == cid.wav && !data.is_null() {
                let n = graf_find(sc, data, GRAF_WAVE);
                grafnode_elem_assoc(m, sc, n, e, scene);
            }
        }
        chap += 1;
    }
    let l = list(m, ed, cid.model);
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        let (data, hier_level) = (m[e].data, m[e].hier_level);
        if m[e].new_flag == 1 && !data.is_null() && has_father(sc, data, hier_level) {
            let n = graf_find(sc, data, 0);
            grafnode_elem_assoc(m, sc, n, e, true);
        }
    }
}

/// For the checks: an outcome for every element of this load that the readers did not
/// visit. Pictures are not read by the loader. A camera, an expression or a model that is
/// not a root and was not reached from its root has no file of its own: it is part of its
/// root's (the camera's interest is the root of the pair; the expressions of a scene are
/// blocks of the root expression's file).
fn report_the_rest(m: &Mem, sc: &mut Scene, ed: Ptr32<ElemDict>) {
    let cid = cid(m);
    let seen: std::collections::BTreeSet<(i32, i32)> = sc.report.iter().map(|r| (r.chap, r.index)).collect();
    for chap in 0..m[ed].nb_lists {
        if chap == cid.dsc {
            continue;
        }
        let l = list(m, ed, chap);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            if m[e].new_flag != 1 || seen.contains(&(chap, i)) || m[e].status != ELEM_VALID {
                continue;
            }
            let outcome = if chap == cid.img || chap == cid.rim {
                Outcome::NotRead
            } else if m[e].hier_level != 0 && chap == cid.model {
                Outcome::NodeMissing
            } else if m[e].hier_level != 0 {
                Outcome::PartOfRoot
            } else {
                Outcome::NotRead
            };
            report(m, sc, ed, ElemId::new(chap, i), outcome);
        }
    }
}

/// Hands each root model the scaling, rotation and translation its `SRT` item gave, and
/// frees the nine numbers [soft 0x0810b290 si_edUpdateRootModelSRT]: for every model element
/// in use of this load whose pointer at +0x3c is set, `SI_modelSetState(model, scaling,
/// rotation, translation)` (of the model code, not ported: recorded, and the numbers kept in
/// `Scene::root_state`), then the block is freed and the pointer cleared.
pub fn update_root_model_srt(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, un: &mut dyn Unported) {
    let model = m[SI_CID].model;
    let l = list(m, ed, model);
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        let block = m[e].root_srt;
        if m[e].status == ELEM_VALID && m[e].new_flag == 1 && !block.is_null() {
            let id = ElemId::new(model, i);
            let mut v = [0f32; 9];
            for (k, x) in v.iter_mut().enumerate() {
                *x = m[block.add(k as i32)];
            }
            // SI_modelSetState (0x084638a8, `si-model` has it): the nine numbers are
            // stored in the node as they are, bits 1 to 3 of its two update words are
            // set, and the sons' state is updated (recorded)
            un.call("SI_modelSetState", &[Arg::Elem(id)]);
            // on the node of the image: SI_modelSetState(node, srt, srt + 3, srt + 6)
            let node = m[e].data.addr();
            if node != 0 && crate::world::obj_of(m[e].data) == 0 {
                let mut o = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
                let b = block.addr();
                crate::image::model_set_state(&mut o, m, node, b, b + 0xc, b + 0x18);
            }
            if let Some(Object::ModelNode { tree, node }) = sc.objects.get(sc.obj(m[e].data)).cloned() {
                if let Some(t) = sc.models.get_mut(tree) {
                    t.state[node] = Some(v);
                    t.dirty[node] |= 0xe;
                }
            }
            sc.root_state.insert(id, v);
            m.free(block.addr());
            m[e].root_srt = Ptr32::NULL;
        }
    }
}

/// `SI_scnRelationsFromElemDict(dictionary, &SI_RD, chapter, list, relation)` on the image
/// ([`crate::db2scn`]), where every pointer it follows is a record of the image
/// ([`crate::db2scn::not_on_image`]); otherwise the call is recorded, with the reason, as
/// before. Its callees are answered by [`crate::image::Prog`].
pub fn scn_relations_on_image(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, chap: i32, group: i32, relid: i32, un: &mut dyn Unported) {
    // SCNREL_ONLY=<id>,<id>…: only these relation ids on the image (the rest recorded), to
    // measure what each changes downstream
    let only = std::env::var("SCNREL_ONLY").ok().map(|v| v.split(',').any(|x| x.parse::<i32>().ok() == Some(relid)));
    match if only == Some(false) { Some("left out by SCNREL_ONLY") } else { crate::db2scn::not_on_image(m, ed.addr(), chap, group, relid) } {
        None => {
            *sc.contents.scnrel_run.entry(relid).or_default() += 1;
            let before = world::fitting_data_before(m, sc, ed, chap, group, relid);
            let mut o = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
            crate::db2scn::scn_relations_from_elem_dict(&mut o, m, ed.addr(), SI_RD.addr(), chap, group, relid);
            for (key, h, rp) in before {
                if m[rp].data.is_null() {
                    // the record was freed: it no longer stands for the value
                    sc.rel_records.retain(|_, v| *v != h);
                    sc.fitting_data.insert(key, h);
                }
            }
        }
        Some(why) => {
            *sc.contents.scnrel_refused.entry((relid, why)).or_default() += 1;
            un.call("SI_scnRelationsFromElemDict", &[Arg::Chap(chap), Arg::Int(group as i64), Arg::Int(relid as i64)]);
        }
    }
}

/// [soft 0x083fbeb0 SI_modelUpdateDep] on the image, which the load calls once the
/// relations tied the fittings and waves to their models: for each element of the MODELS
/// chapter (`chap`, the original's `SI_cid` word) in use (status 0), new (flag 1), a root
/// (hierarchy level 0) with an object, the dependencies of its tree again
/// (`SI_modelUpdateDependency(root, null)`: working definitions for the nodes now deformed,
/// `latdep` the node whose branch deformation holds them); then node by node
/// (`SI_modelNextNode`): the node's lattices (+0x50) and the branch's (+0x58) touched; the
/// branch's spline fitting (+0x64), then the node's (+0x60): its matrix
/// (`SI_fittingUpdateMatrix`, `si-deform`'s) and the touch; the node's patch fitting
/// (+0x2d8), then the branch's (+0x2dc) likewise (`SI_pfittingUpdateMatrix`). The pointers
/// are tested as they are (an inactive fitting too). The touches are [`deformation_touch`].
///
/// Glue, not of the original: a root whose object is the loader's handle, not a record of
/// the image, is passed over and `record` is told `SI_modelUpdateDep`; with `dependency`
/// false (`SCENE_NO_UPDATE_DEPENDENCY=1` in the load) `SI_modelUpdateDependency` is told to
/// `record` instead of run. `SCENE_NO_MODEL_UPDATE_DEP=1` records the whole call instead,
/// as before. Checked against the original by `model-scenecheck` on
/// `re/tools/si-model/emu_scene8u.py`'s vectors.
pub fn model_update_dep<O: Outside>(o: &mut O, m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32, dependency: bool, record: fn(&mut O, &'static str)) {
    use si_model::model_image::{readjust, tree};
    let chapter = m.u32(ed.addr() + 4).wrapping_add((chap as u32) << 6);
    let mut i = 0;
    while i < m.i32(chapter + 4) {
        let e = m.u32(chapter + 8).wrapping_add(0x84 * i as u32);
        i += 1;
        let root = m.u32(e + 4);
        if m.i16(e + 0x48) != 0 || m.i16(e + 0x46) != 1 || m.i32(e + 0x54) != 0 || root == 0 {
            continue;
        }
        if si_mem::handle::Kind::of(root).is_some() {
            record(o, "SI_modelUpdateDep");
            continue;
        }
        if dependency {
            readjust::update_dependency(m, o, Ptr32::new(root), 0);
        } else {
            record(o, "SI_modelUpdateDependency");
        }
        let mut n = root;
        while n != 0 {
            if m.u32(n + 0x50) != 0 {
                deformation_touch(o, m, n, 0);
            }
            if m.u32(n + 0x58) != 0 {
                deformation_touch(o, m, n, 1);
            }
            for (at, branch) in [(0x64u32, 1i16), (0x60, 0)] {
                let f = m.u32(n + at);
                if f != 0 {
                    si_deform::fitting::fitting_update_matrix(m, Ptr32::new(f));
                    deformation_touch(o, m, n, branch);
                }
            }
            for (at, branch) in [(0x2d8u32, 0i16), (0x2dc, 1)] {
                let f = m.u32(n + at);
                if f != 0 {
                    si_deform::fitting::pfitting_update_matrix(m, Ptr32::new(f));
                    deformation_touch(o, m, n, branch);
                }
            }
            n = tree::next_node(m, Ptr32::new(n)).addr();
        }
    }
}

/// [soft 0x08153140 SI_latticeTouch], [soft 0x081535c8 SI_splineFitTouch] and
/// [soft 0x08153578 SI_patchFitTouch] (`deformation.c`), three routines with one body:
/// the node's working definition readjusted (`SI_modelReadjustDefinition`), and with
/// `branch` not 0 that of every node below it (`SI_modelNextSon`).
pub fn deformation_touch(o: &mut dyn Outside, m: &mut Mem, node: u32, branch: i16) {
    use si_model::model_image::{readjust, tree};
    readjust::readjust_definition(m, o, Ptr32::new(node));
    if branch != 0 {
        let mut s = Ptr32::NULL;
        loop {
            s = tree::next_son(m, Ptr32::new(node), s);
            if s.is_null() {
                break;
            }
            readjust::readjust_definition(m, o, s);
        }
    }
}

fn update_dependency_on() -> bool {
    std::env::var_os("SCENE_NO_UPDATE_DEPENDENCY").is_none()
}

/// `SCENE_NO_MODEL_UPDATE_DEP=1` records `SI_modelUpdateDep` instead of running
/// [`model_update_dep`], as before it was ported.
fn model_update_dep_on() -> bool {
    std::env::var_os("SCENE_NO_MODEL_UPDATE_DEP").is_none()
}

/// The calls of `SI_scnRelationsFromElemDict` the loader makes, in order: chapter, list,
/// relation id.
pub fn scn_relations_order(cid: &si_types::dict::ChapIds) -> Vec<(i32, i64, i64)> {
    let (m, l, c, w, mat) = (cid.model, cid.lig, cid.cam, cid.wav, cid.mat);
    vec![
        (m, 1, 0x6f), (m, 1, 800), (m, 1, 0x321), (m, 1, 0x322), (m, 1, 0x323), (m, 1, 0x324), (m, 1, 0x325), (m, 1, 0x326),
        (m, 1, 0x327), (l, 1, 0x83f), (l, 1, 0x844), (c, 1, 0x457), (c, 1, 0x45c), (m, 1, 0x70), (l, 1, 0x840), (l, 1, 0x842),
        (c, 1, 0x458), (c, 1, 0x45a), (m, 2, 0xdc), (m, 2, 0xdd), (m, 2, 0xe6), (m, 2, 0xe7), (m, 2, 600), (m, 2, 0x259),
        (w, 1, 0x264), (m, 3, 300), (mat, 3, 0x191), (mat, 3, 0x1f5), (m, 3, 400), (m, 3, 500), (l, 3, 0x898), (cid.nsl, 0, 0x2711),
    ]
}
