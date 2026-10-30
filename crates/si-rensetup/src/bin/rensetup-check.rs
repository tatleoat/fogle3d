// SPDX-License-Identifier: MIT
//! Corpus check of the render set-up (finding 0023).
//!
//! `rensetup-check <dir>` loads every scene `si-scene` can load under a directory, makes its
//! render dictionary (instances expanded, default material, materials handed down), sizes
//! the selective lights, runs one frame's update, deletes the dictionary again, and reports
//! counts; it also builds what the native renderer would be handed.
//! `rensetup-check rules <Relations.rsrc>` prints the rules of the relation dictionary as
//! the emulation generator wants them.

use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_rensetup::edlocal::{instance_material_share, instance_texture_share, model_master_from_instance};
use si_rensetup::outside::{Outside, Recorder};
use si_rensetup::world::{algo, rel, Env, Heap, Obj};
use si_rensetup::{bridge, handover, rmain, update, util};
use si_scene::dict::Dict;
use si_scene::load::{self, Scene};
use si_types::dict::ELEM_VALID;
use si_types::model::Model;
use si_types::Ptr32;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn rules(rsrc: &PathBuf) {
    let data = std::fs::read(rsrc).expect("Relations.rsrc");
    let start = si_scene::init::start(&data).expect("Relations.rsrc");
    // id, source and destination chapter, source and destination list, rank, fan-out, dope
    // sheet display, the two update values, active, active for dependents, render, increment
    for r in &start.program.rd.rules {
        println!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            r.relid, r.src_chap, r.dst_chap, r.src_group, r.dst_group, r.rank, r.fanout, r.dope_disp, r.upd_attr0, r.upd_attr1, r.active,
            r.active_dep, r.render, r.incmode
        );
    }
}

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

/// The rest of the program for a scene that was only loaded: the calls are written down and
/// answered with nothing, except the copy of a hierarchy, which is made by a stand-in.
#[derive(Default)]
struct Corpus {
    rec: Recorder,
    nodes_copied: usize,
}

/// A stand-in for `SI_modelExpandAllWithoutGeometryAbsolute` (`instance.c`, not ported): a
/// copy of a node and of everything under it, each copy carrying its original's element
/// number and pointing at its geometry, materials and textures; an instance met on the way
/// is replaced by the copy of its master. It follows the description in finding 0023, not
/// the routine's instructions.
fn copy_tree(env_heap: &mut Heap, current: &mut Dict, p: &si_scene::dict::Program, g: &mut si_rensetup::world::Globals, node: u32, father: u32, depth: usize, n: &mut usize) -> u32 {
    let Some(src) = env_heap.model(node).copied() else { return 0 };
    if depth > 64 {
        return 0;
    }
    let mut source = node;
    let mut from = src;
    if depth > 0 && src.algo_type == algo::INSTANCE {
        let master = model_master_from_instance(current, p, node);
        if let Some(m) = env_heap.model(master).copied() {
            source = master;
            from = m;
        }
    }
    let name = env_heap.model_name(source).map(|s| s.to_vec());
    let mut c = Model::allocate(name.as_deref(), &mut 0).0;
    c.flags_08 |= 0x40;
    c.kind = from.kind;
    c.nb_defs = from.nb_defs;
    c.defs = from.defs;
    c.org_def = from.org_def;
    c.definition = from.definition;
    c.elem_id = src.elem_id;
    c.scal = src.scal;
    c.vrot = src.vrot;
    c.trans = src.trans;
    c.visibility = from.visibility;
    c.motion_blur = from.motion_blur;
    c.father = Ptr32::new(father);
    let name = name.map_or(0, |s| env_heap.strdup(&s));
    c.name = Ptr32::new(name);
    let copy = env_heap.alloc(Obj::Model(Box::new(c)));
    *n += 1;
    let mut env = Env { p: si_scene::dict::Program::new(), heap: std::mem::take(env_heap), g: std::mem::take(g) };
    instance_material_share(&mut env, source, copy);
    instance_texture_share(&mut env.heap, source, copy);
    *env_heap = env.heap;
    *g = env.g;
    let mut son = from.son.0;
    let mut last = 0;
    while son != 0 {
        let next = env_heap.model(son).map_or(0, |m| m.brother.0);
        let s = copy_tree(env_heap, current, p, g, son, copy, depth + 1, n);
        if s != 0 {
            if last == 0 {
                env_heap.model_mut(copy).unwrap().son = Ptr32::new(s);
            } else {
                env_heap.model_mut(last).unwrap().brother = Ptr32::new(s);
            }
            last = s;
        }
        son = next;
    }
    copy
}

struct Out<'a> {
    c: &'a mut Corpus,
    p: si_scene::dict::Program,
    g: si_rensetup::world::Globals,
}

impl Outside for Out<'_> {
    fn status_display_message(&mut self, text: &[u8], level: i32) {
        self.c.rec.status_display_message(text, level)
    }
    fn usleep(&mut self, micros: u32) {
        self.c.rec.usleep(micros)
    }
    fn print(&mut self, text: &[u8]) {
        self.c.rec.print(text)
    }
    fn ed_get_elements_graf_node_info(&mut self, d: &mut Dict, render: bool) {
        self.c.rec.ed_get_elements_graf_node_info(d, render)
    }
    fn material_allocate(&mut self, heap: &mut Heap) -> u32 {
        self.c.rec.material_allocate(heap)
    }
    fn model_update_son_state(&mut self, heap: &mut Heap, father: u32, model: u32, flag: i16) {
        self.c.rec.model_update_son_state(heap, father, model, flag)
    }
    fn model_expand_all_without_geometry_absolute(&mut self, heap: &mut Heap, current: &mut Dict, master: u32) -> u32 {
        copy_tree(heap, current, &self.p, &mut self.g, master, 0, 0, &mut self.c.nodes_copied)
    }
    fn expand_update_transfo(&mut self, heap: &mut Heap, current: &mut Dict, master: u32, instance: u32, copy: u32) {
        // the copy takes the instance's place: its local transformation and visibility
        if let Some(i) = heap.model(instance).copied() {
            if let Some(c) = heap.model_mut(copy) {
                c.scal = i.scal;
                c.vrot = i.vrot;
                c.trans = i.trans;
                c.visible = i.visible;
            }
        }
        self.c.rec.expand_update_transfo(heap, current, master, instance, copy)
    }
    fn model_nb_triangles(&mut self, heap: &Heap, model: u32) -> i32 {
        // a node with geometry counts as having triangles
        heap.model(model).map_or(0, |m| m.nb_defs as i32)
    }
    fn print_warn_uv(&mut self, name: Option<&[u8]>) {
        self.c.rec.print_warn_uv(name)
    }
    fn cus_get_effect_by_icon(&mut self, _model: u32) -> Option<Option<i32>> {
        None
    }
    fn topo_sort(&mut self, current: &mut Dict) {
        self.c.rec.topo_sort(current)
    }
    fn db_check_extra_untold_relation(&mut self, current: &mut Dict, a: i16, b: i16, c: i16) {
        self.c.rec.db_check_extra_untold_relation(current, a, b, c)
    }
    fn scene_update_items(&mut self, time: f32, camera: u32) {
        self.c.rec.scene_update_items(time, camera)
    }
    fn window_hilite_camera(&mut self) -> Option<u32> {
        None
    }
    fn template_windows(&mut self) -> Vec<(i16, u32)> {
        Vec::new()
    }
    fn picture_elem_remove(&mut self, d: &mut Dict, picture: u32) {
        self.c.rec.picture_elem_remove(d, picture)
    }
    fn udf_reset_active(&mut self) {
        self.c.rec.udf_reset_active()
    }
    fn udf_is_active(&mut self, shader: u32) -> i16 {
        // every shader record of a loaded scene counts as switched on
        self.c.rec.udf_is_active(shader);
        1
    }
    fn udf_set_as_selected(&mut self, shader: u32) {
        self.c.rec.udf_set_as_selected(shader)
    }
}

#[derive(Default, Clone)]
struct Totals {
    scenes: usize,
    loaded: usize,
    /// Scenes not loaded completely, rendered all the same (unless RENSETUP_SKIP_INCOMPLETE=1).
    rendered_incomplete: usize,
    prepared: usize,
    by_chapter: BTreeMap<String, (usize, usize)>,
    scene_elements: usize,
    render_elements: usize,
    relations_scene: usize,
    relations_copied: usize,
    relations_render: usize,
    instances_in_scenes: usize,
    scenes_with_instances: usize,
    instances_expanded: usize,
    nodes_copied: usize,
    elements_added: usize,
    roots_given_default: usize,
    own_materials_only: usize,
    own_materials_only_roots: usize,
    models_with_default: usize,
    models_inheriting: usize,
    models_with_material: usize,
    models: usize,
    uv_warnings: usize,
    shaders_selected: usize,
    selective_lights: usize,
    something_to_render: usize,
    son_state_calls: usize,
    flags: [usize; 8],
    hand_over_roots: usize,
    hand_over_lights: usize,
    hand_over_instance_roots: usize,
    scene_changed: usize,
    left_after_delete: usize,
    failures: Vec<String>,
    native: Native,
}

/// The second way through a scene (finding 0023, sections 17 and 18): the image the loader
/// built, completed by `native::bridge`, the list built by `si-view`'s port of
/// `SI_graflistBuildFromElemDict`, and `SI_sequenceRender` run on it with `si-render`'s
/// `SI_image` behind it.
#[derive(Default, Clone)]
struct Native {
    scenes: usize,
    roots: usize,
    instance_roots: usize,
    nodes_allocated: usize,
    lights: usize,
    lights_on: usize,
    lights_not_listed: usize,
    nodes_shown: usize,
    nodes_visible: usize,
    nodes_without_element: usize,
    frees_by_command: usize,
    frees_by_renderer: usize,
    relations: usize,
    /// Graf nodes of the scenes' lists, by kind.
    kinds: BTreeMap<i32, usize>,
    /// What the adapter supplied.
    model_nodes: usize,
    boxes: usize,
    copies_moved: usize,
    pictures_asked: usize,
    elements_pointed: usize,
    elements_left: usize,
    left_by_chapter: BTreeMap<String, usize>,
    /// `SI_image`: answers by value, triangles, pictures, lines, what the set-up said.
    statuses: BTreeMap<u32, usize>,
    command_statuses: BTreeMap<i32, usize>,
    triangles: i64,
    pictures: usize,
    lines: usize,
    notes: BTreeMap<String, usize>,
    messages: BTreeMap<String, usize>,
    /// Calls by the callee's name: to a port, and to nobody.
    ported: BTreeMap<String, usize>,
    hooks: BTreeMap<String, usize>,
    /// What the scene's update (`SI_sceneUpdateItems`) called out of `si-motion`, by the
    /// host method's name: answered by a port, answered 0.
    motion_calls: BTreeMap<String, (usize, usize)>,
    /// The load's `SI_sceneRefresh` run on the image, and the calls its update made.
    load_refreshed: usize,
    load_motion_calls: BTreeMap<String, (usize, usize)>,
}

/// One root of the chain `SI_image` was handed.
struct Root {
    dummy: u32,
    son: u32,
    instance: bool,
    elem_id: i32,
}

/// What the world's watch saw of one run of the command.
#[derive(Default)]
struct Seen {
    handed: Vec<[u32; 5]>,
    roots: Vec<Root>,
    lights: Vec<(u32, bool, i32)>,
    shown: usize,
    visible: usize,
    without_element: usize,
    kinds: BTreeMap<i32, usize>,
    error: Option<String>,
    opened: usize,
    closed: usize,
    frees_before: u64,
    frees_after: u64,
    allocated_before: usize,
    messages: Vec<String>,
    boxes: usize,
    copies_moved: usize,
}

impl si_rensetup::native::world::Watch for Seen {
    fn before_image(&mut self, m: &mut si_mem::Mem, a: &[u32; 5]) {
        use si_rensetup::native::{rec, scene_graf_list};
        use si_types::scene::GrafNode;
        self.handed.push(*a);
        self.frees_before = m.heap_stats().frees as u64;
        let fail = |s: &mut Seen, text: &str| {
            if s.error.is_none() {
                s.error = Some(text.to_string());
            }
        };
        // the lights: the chain through +0x140; each is on exactly when its node's display
        // type is 2
        let mut l = a[3];
        while l != 0 && self.lights.len() < 100_000 {
            let node = si_view::graflist::find_gl_data(m, scene_graf_list(m), si_mem::Ptr32::new(l), si_rensetup::native::edtools::light_get_graf_type(m, l));
            let wire = if node.is_null() { -1 } else { m[node].wire_type };
            self.lights.push((l, m.u16(l + rec::light::ON) != 0, wire));
            l = m.u32(l + rec::light::NEXT);
        }
        // the roots: a dummy of kind -1 over every root of the list, and one over the copy
        // of the master for every instance
        let mut d: si_mem::Ptr32<Model> = si_mem::Ptr32::new(a[4]);
        while !d.is_null() {
            if self.roots.len() > 100_000 || m[d].kind != -1 || m[d].visible != 1 || m[d].son.is_null() {
                fail(self, "the chain of roots is not a chain of dummies");
                break;
            }
            let son = m[d].son;
            let instance = m[d].algo_type == rec::ALGO_INSTANCE;
            if instance && (m[son].father != d || m[son].flags_08 & rec::flag::COPY == 0) {
                fail(self, "an instance's dummy is not over a copy");
            }
            if instance {
                // the copy stands where its instance stands (SI_expandCopyInfoFromInstance,
                // then si-model's SI_modelUpdateSonState as a root): its global translation
                // is the instance's, bit for bit, and its global matrix carries it
                use si_rensetup::native::{current_dict, globals::SI_CID};
                let chapter = m[current_dict(m)].lists.add(m[SI_CID].model);
                let inst: si_mem::Ptr32<Model> = m[m[chapter].elems.add(m[son].elem_id)].data.cast();
                let (a, b) = (m[son].gtrs, m[inst].gtrs);
                let same = [(a.x, b.x), (a.y, b.y), (a.z, b.z)].iter().all(|p| p.0.to_bits() == p.1.to_bits());
                let row = m[son].gmat[3];
                if !same || [row[0], row[1], row[2]].map(f32::to_bits) != [a.x, a.y, a.z].map(f32::to_bits) {
                    fail(self, "the copy of a master does not stand where its instance stands");
                }
                self.copies_moved += (a.x != 0.0 || a.y != 0.0 || a.z != 0.0) as usize;
            }
            self.roots.push(Root { dummy: d.0, son: son.0, instance, elem_id: m[son].elem_id });
            d = m[d].brother;
        }
        // the list: every model node's `visible` word is its node's display type
        let mut gn: si_mem::Ptr32<GrafNode> = m[scene_graf_list(m)].first;
        while !gn.is_null() {
            let kind = m[gn].kind;
            *self.kinds.entry(kind).or_default() += 1;
            if kind == 2 || kind == 3 {
                let model: si_mem::Ptr32<Model> = m[gn].data.cast();
                let wire = m[gn].wire_type;
                if (m[model].visible != 0) != (wire != 0) {
                    fail(self, "a node's visible word is not its display type");
                }
                self.shown += 1;
                self.visible += (wire != 0) as usize;
                self.without_element += (m[model].elem_id < 0) as usize;
            }
            gn = m[gn].next;
        }
    }

    fn after_image(&mut self, m: &mut si_mem::Mem, _: &si_rensetup::native::world::ImageReport) {
        self.frees_after = m.heap_stats().frees as u64;
    }

    fn hook(&mut self, m: &mut si_mem::Mem, f: si_mem::FnPtr, a: &[u32]) -> Option<u32> {
        use si_rensetup::native::outside as out;
        if f == out::SI_OPEN_RENDER_WINDOW {
            self.opened += 1;
        } else if f == out::SI_CLOSE_RENDER_WINDOW {
            self.closed += 1;
        } else if f == out::SI_STATUS_DISPLAY_MESSAGE {
            self.messages.push(String::from_utf8_lossy(&m.cstr(a[0])).into_owned());
        } else if f == out::SI_IMAGE {
            // RENSETUP_NO_RENDER: the renderer's entry is a hook; what it was handed is noted
            // as when it runs, and it answers "nothing to render"
            let mut args = [0u32; 5];
            args.copy_from_slice(&a[..5]);
            si_rensetup::native::world::Watch::before_image(self, m, &args);
            self.frees_after = m.heap_stats().frees as u64;
            return Some(0x7e);
        } else if f == out::SI_DO_SUBREGION {
            // the user at the sub-region box: the check accepts the box as the scene's
            // set-up has it (0 would be "cancelled", and the command would end there)
            self.boxes += 1;
            return Some(1);
        }
        None
    }
}

/// What the check stands in for of the machine the program runs on.
struct Host {
    /// The program's default picture (`<SI_resdir>/noIcon.pic`).
    no_icon: Option<Vec<u8>>,
    /// The scene's database: its RENDER_PICTURES chapter is where `SI_picturePath` points
    /// at start-up when the database can be written to (the check writes nothing).
    database: Vec<u8>,
    /// `RENSETUP_PICTURES=<dir>`: the frame is left as a picture file below `<dir>`, in the
    /// scene's own directory `<label>` (see [`Pictures`]); the scene's line of `index.tsv`
    /// is left in `row`.
    pictures: Option<(PathBuf, String)>,
    row: std::cell::RefCell<Option<String>>,
}

/// `RENSETUP_PICTURES=<dir>`: the port's pictures of the sample scenes, for comparison
/// with the original's batch renderer (finding 0023, section 20; finding 0012, 20.3). Each
/// scene's first frame goes through the render command and `SI_image` as in the check,
/// and the frame is written as a picture file through the machine's files
/// (`si_render::adapter::write_pictures_under`): `<dir>/<label>/<name>.<frame>.pic`, the
/// picture path being `/<label>` and `<name>` the scene's output name without its
/// directory, as `soft -R <scene> -D <database>` takes it (finding 0019, 3.2). `<label>` is
/// the scene's name, or `<scene>@<database>` for a second scene of the same name.
/// `<dir>/index.tsv`: database, scene, directory, the command's answer, `SI_image`'s
/// answer, triangles filed, lines written, the files on disk, and what ended a scene
/// early; the same columns as `re/tools/si-render/pictures`, which
/// `re/tools/si-render/oracle_compare.py` reads. Nothing here reproduces a function of the
/// original.
struct Pictures {
    root: PathBuf,
    used: std::collections::BTreeSet<String>,
    index: std::fs::File,
    written: usize,
}

impl Pictures {
    fn new(root: PathBuf) -> Pictures {
        use std::io::Write;
        std::fs::create_dir_all(&root).expect("the pictures' directory");
        let mut index = std::fs::File::create(root.join("index.tsv")).expect("index.tsv");
        writeln!(index, "database\tscene\tdirectory\tcommand\tSI_image\ttriangles\tlines\tfiles\tended\tkinds\tnotes\trelations refused").unwrap();
        Pictures { root, used: Default::default(), index, written: 0 }
    }

    /// The directory of a scene: its name, or `<scene>@<database>` when taken.
    fn label(&mut self, scene: &str, db: &Path) -> String {
        let mut label = scene.to_string();
        if !self.used.insert(label.clone()) {
            label = format!("{scene}@{}", db.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            self.used.insert(label.clone());
        }
        let _ = std::fs::create_dir_all(self.root.join(&label));
        label
    }

    /// A scene's line: `done` is what the render left (command, `SI_image`, triangles,
    /// lines, files asked for, end), or why there was none.
    fn line(&mut self, db_label: &str, scene: &str, label: &str, done: Result<String, String>) {
        use std::io::Write;
        match done {
            Ok(row) => {
                let mut f = row.split('\t').map(str::to_string).collect::<Vec<_>>();
                let (asked, end) = (f.remove(4), f.remove(4));
                // what is on disk in the scene's directory (an output name that is a path
                // of its own writes elsewhere, or fails)
                let mut on_disk: Vec<String> = std::fs::read_dir(self.root.join(label)).map(|rd| rd.flatten().map(|e| format!("{label}/{}", e.file_name().to_string_lossy())).collect()).unwrap_or_default();
                on_disk.sort();
                if on_disk.is_empty() && !asked.is_empty() {
                    on_disk.push(format!("(asked: {asked})"));
                } else {
                    self.written += on_disk.len();
                }
                writeln!(self.index, "{db_label}\t{scene}\t{label}\t{}\t{}\t{}\t{}\t{}\t{end}\t{}\t{}\t{}", f[0], f[1], f[2], f[3], on_disk.join(" "), f[4], f[5], f[6]).unwrap();
            }
            Err(e) => writeln!(self.index, "{db_label}\t{scene}\t{label}\t\t\t\t\t\t{e}").unwrap(),
        }
    }
}

/// The screen the check assumes (`SI_xmaxscreen`, `SI_ymaxscreen`): 1280 × 1024.
const SCREEN: (i16, i16) = (1279, 1023);

/// What the program's start-up leaves in the image before a scene is loaded, as far as it
/// is on the image: the screen's size (the GL layer's, the check's assumption) and the
/// Matter module's start-up, `SI_matterInit (0x08291bec)` (the default light, the render
/// controls; `native::matter::matter_init` says what of it is not done). The image itself,
/// with the binary's data segment, is `si_scene::load::Scene::start`'s.
/// `RENSETUP_NO_DEFAULT_LIGHT=1` leaves the default light out (to measure what it changes).
fn start_up(m: &mut si_mem::Mem, resdir: &[u8]) {
    use si_rensetup::native::{globals as g, matter};
    // the two directories of the batch renderer's start-up (finding 0018, section 19):
    // `SI_resdir`, the resource directory (`si_runCreative` before `standAloneRender`), and
    // `SI_texturePath = strdup(SI_cwd)` (`standAloneRender`): the check's working directory
    let cwd = std::env::current_dir().map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    for (at, text) in [(si_scene::t2dpath::SI_RESDIR, resdir), (si_scene::t2dpath::SI_TEXTURE_PATH, &cwd[..])] {
        let p = m.malloc(text.len() as u32 + 1);
        m.put_cstr(p, text);
        m.set_u32(at, p);
    }
    // this package's globals of `.data` and its texts (the output name `default` that
    // SI_renControlInit copies among them); the scene's own words are left as they are
    g::init_loaded(m);
    m[g::SI_XMAXSCREEN] = SCREEN.0;
    m[g::SI_YMAXSCREEN] = SCREEN.1;
    // `SI_manipulationInit (0x080d3210)` and `SI_animationInfoInit (0x080d335c)`, which the
    // batch renderer's start-up calls before `SI_matterInit` (finding 0019): the values
    // si-app's ports of the two give with none of their environment variables set
    // (`SI_NO_INTERACTIVE_FCV_UPD`, `SI_IK_OLD_JOINT_ROLL`, `SI_MANIPULATION_OPTIM`,
    // `SI_NO_OPTIM_UNTOLD_REL`, `SI_CLASSICAL_SCL_FIX`, `SI_ENV_RND_FST_JNT`,
    // `SI_NO_ENVELOP_OPTIM`). si-app keeps the two records in its own state, not in the
    // image: this writes them where the image has them (`SI_glbManip` 0x08781560,
    // `SI_glbAnim` 0x08781480). The loader then stores a setup file's preferences over them.
    let (gm, ga) = (0x0878_1560u32, 0x0878_1480u32);
    for (o, v) in [(0x00u32, 0u32), (0x04, 0), (0x08, 2), (0x0c, 4), (0x10, 0), (0x14, 4), (0x18, 0)] {
        m.set_u32(gm + o, v);
    }
    for (o, v) in [(0x1cu32, 0i16), (0x30, 0), (0x1e, 0), (0x20, 1), (0x22, 0), (0x24, 1), (0x26, 1), (0x28, 0), (0x2a, 0), (0x2c, 0), (0x2e, 0), (0x32, 0), (0x34, 0), (0x36, 1), (0x38, 0)] {
        m.set_i16(gm + o, v);
    }
    m.set_f32(ga + 0x18, 0.15);
    m.set_f32(ga + 0x28, 0.01);
    for (o, v) in [(0x00u32, 0i16), (0x02, 0), (0x04, 1), (0x08, 0), (0x0a, 0), (0x0c, 0), (0x14, 0), (0x2c, 0), (0x1c, 1), (0x1e, 0), (0x20, 1), (0x22, 1), (0x24, 0), (0x26, 1), (0x30, 2), (0x32, 1), (0x3c, 0), (0x3a, 0), (0x2e, 0), (0x38, 1), (0x3e, 0)] {
        m.set_i16(ga + o, v);
    }
    m.set_u32(ga + 0x10, 0);
    m.set_u32(ga + 0x34, 0);
    matter::matter_init(m, &|p: &[u8]| std::fs::read(std::ffi::OsStr::from_bytes(p)).ok());
    if std::env::var_os("RENSETUP_NO_DEFAULT_LIGHT").is_some() {
        m.set_bytes(matter::SI_DEFAULT_LIGHT, &[0; 0x240]);
    }
}

fn native_one(sc: &mut Scene, _env: &si_rensetup::world::Env, ho: &handover::HandOver, n: &mut Native, host: &Host) -> Result<(), String> {
    use si_mem::Ptr32;
    use si_rensetup::native::globals as g;
    use si_rensetup::native::outside::{self as out};
    use si_rensetup::native::rec::ctl;
    use si_rensetup::native::world::{name_of, World};
    use si_rensetup::native::{bridge as place, rec, rnd_dial};
    // the image the loader built the scene in
    let mut m = std::mem::take(&mut sc.mem);
    let mut world = World::new(Seen::default());
    // RENSETUP_NO_RENDER=1: SI_image is a hook (the hand-over is checked without the renderer)
    world.render = std::env::var_os("RENSETUP_NO_RENDER").is_none();
    // RENSETUP_NO_MOTION=1, RENSETUP_NO_GRAF_INFO=1: SI_sceneUpdateItems, or
    // SI_edGetElementsGrafNodeInfo, is a hook (to measure what each changes)
    world.motion = std::env::var_os("RENSETUP_NO_MOTION").is_none();
    world.graf_info = std::env::var_os("RENSETUP_NO_GRAF_INFO").is_none();
    world.pictures.default = host.no_icon.clone();
    // texture pictures from the file system (RENSETUP_NO_PICTURE_FILES=1: the default only)
    world.pictures.files = std::env::var_os("RENSETUP_NO_PICTURE_FILES").is_none();
    world.pictures.texture_path = match m.u32(si_scene::t2dpath::SI_TEXTURE_PATH) {
        0 => Vec::new(),
        p => m.cstr(p),
    };
    let r = (|| -> Result<(), String> {
        let m = &mut m;
        let placed = place::place_scene(m, &mut world, sc)?;
        // the render controls: their start-up state (SI_matterInit, made at start-up with
        // the image: `start_up` below), then the scene's set-up
        if let Some(setup) = &sc.setup {
            place::controls_from_setup(m, &setup.render);
        }
        // SI_picturePath, as si_runCreative (0x080cab88) leaves it for an output database
        // it may write to: a copy of the path of its RENDER_PICTURES chapter
        let mut path = [&host.database[..], b"/RENDER_PICTURES"].concat();
        if let Some((_, label)) = &host.pictures {
            // RENSETUP_PICTURES: the scene's directory below the pictures' directory, and
            // the output name without its directory, as `soft -R <scene> -D <database>`
            // takes it (finding 0019, 3.2): a name that is a path of the authors' machines
            // would otherwise not open, here as in the original
            path = format!("/{label}").into_bytes();
            let at = g::SI_DEFAULT_CONTROL.0 + 0x17c;
            let p = m.u32(at);
            if p != 0 {
                if let Some(k) = m.cstr(p).iter().rposition(|&c| c == b'/') {
                    m.set_u32(at, p + k as u32 + 1);
                }
            }
        }
        let local = m.local(path.len() as u32 + 1);
        m.put_cstr(local, &path);
        let copy = m.strdup(local);
        m.release(path.len() as u32 + 1);
        m[g::SI_PICTURE_PATH] = copy;
        // the check renders the first frame of the scene's range only, unless told otherwise
        if std::env::var_os("RENSETUP_ALL_FRAMES").is_none() || host.pictures.is_some() {
            let first = m.i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_FIRST);
            m.set_i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_LAST, first);
        }
        if let Some(f) = m.fault() {
            return Err(format!("native: setting the controls faulted: {f:?}"));
        }
        let count = |w: &World<Seen>, f: si_mem::FnPtr| w.ported.get(&f.0).copied().unwrap_or(0);
        world.watch.allocated_before = count(&world, out::SI_MODEL_ALLOCATE);
        let frees_start = m.heap_stats().frees as u64;
        if let Some((root, _)) = &host.pictures {
            si_render::adapter::write_pictures_under(Some(root.clone()));
        }
        let status = rnd_dial::sequence_render(&mut world, m);
        si_render::adapter::write_pictures_under(None);
        let frees_end = m.heap_stats().frees as u64;
        if host.pictures.is_some() {
            // the scene's line of index.tsv: command, SI_image, triangles, lines, the files
            // asked for, what ended the frame early (a fault of the image, a fatal note)
            let image = world.images.first();
            let mut end = String::new();
            if let Some(f) = m.fault() {
                end = format!("fault {:#x} ({:?})", f.addr, f.access);
            }
            if let Some(n) = image.and_then(|i| i.notes.iter().find(|n| n.starts_with("fatal") || n.starts_with("fault"))) {
                end = if end.is_empty() { n.clone() } else { format!("{end}; {n}") };
            }
            let asked: Vec<String> = image.map(|i| i.pictures.iter().map(|p| String::from_utf8_lossy(&p.0).into_owned()).collect()).unwrap_or_default();
            // the three columns `re/tools/si-render/classify.py` reads: the model nodes in
            // use by kind (+0x30: count / with a geometry +0x48), what the renderer's set-up
            // said (each note once, with its count), and the loader's
            // SI_scnRelationsFromElemDict calls it could not make on the image
            let mut kinds: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
            let models = m[m[placed.dict].lists.add(m[g::SI_CID].model)];
            for i in 0..models.nb_elems {
                let e = models.elems.add(i);
                let node: Ptr32<Model> = m[e].data.cast();
                if m[e].status == ELEM_VALID && !node.is_null() && si_scene::world::obj_of(m[e].data) == 0 {
                    let k = kinds.entry(m.i32(node.0 + 0x30)).or_default();
                    k.0 += 1;
                    k.1 += (m.u32(node.0 + 0x48) != 0) as usize;
                }
            }
            let kinds = kinds.iter().map(|(k, (n, g))| format!("{k}:{n}/{g}")).collect::<Vec<_>>().join(" ");
            let mut seen: Vec<(String, usize)> = Vec::new();
            for n in image.map(|i| i.notes.clone()).unwrap_or_default() {
                let n = n.replace(['\t', '\n'], " ");
                match seen.iter_mut().find(|s| s.0 == n) {
                    Some(s) => s.1 += 1,
                    None => seen.push((n, 1)),
                }
            }
            let notes = seen.iter().map(|(n, c)| if *c > 1 { format!("{n} (x{c})") } else { n.clone() }).collect::<Vec<_>>().join(" | ");
            let refused = sc.contents.scnrel_refused.iter().map(|((id, why), n)| format!("{id}x{n}: {why}")).collect::<Vec<_>>().join(" | ");
            *host.row.borrow_mut() = Some(format!(
                "{:#x}\t{:#x}\t{}\t{}\t{}\t{end}\t{kinds}\t{notes}\t{refused}",
                status,
                image.map_or(0, |i| i.status),
                image.map_or(0, |i| i.triangles),
                image.map_or(0, |i| i.pictures.iter().map(|p| p.1).sum::<usize>()),
                asked.join(" ")
            ));
        }
        if let Some(f) = m.fault() {
            return Err(format!("native: the command faulted: {f:?}, first seen in {}; the renderer's notes: {:?}", world.fault_in.map_or("the command itself".to_string(), name_of), world.images.first().map(|i| &i.notes)));
        }
        // the render window: opened and closed once each (`renu_display.c`, this package's
        // port on the image since the sixth pass; a hook the watch counted before)
        let (opened, closed) = (count(&world, out::SI_OPEN_RENDER_WINDOW) + world.watch.opened, count(&world, out::SI_CLOSE_RENDER_WINDOW) + world.watch.closed);
        let seen = &world.watch;
        if seen.handed.len() != 1 || world.images.len() != world.render as usize || opened != 1 || closed != 1 {
            return Err(format!("native: {} calls of SI_image, window opened {} and closed {} times", seen.handed.len(), opened, closed));
        }
        if let Some(e) = &seen.error {
            return Err(format!("native: {e}"));
        }
        let [camera, _, _, _, _] = seen.handed[0];
        if camera != placed.camera || camera == 0 {
            return Err("native: another camera than the scene's".into());
        }
        // the lights: every light of the list once, in the order their files were read
        // (each entered the list at its head, the command chains each at the chain's head)
        let listed: Vec<u32> = placed.lights.iter().copied().filter(|&l| (1..=4).contains(&m.i16(l + rec::light::TYPE))).collect();
        let chain: Vec<u32> = seen.lights.iter().map(|l| l.0).collect();
        if chain != listed {
            return Err(format!("native: {} lights handed over, the scene has {} (or in another order)", chain.len(), listed.len()));
        }
        for &(_, on, wire) in &seen.lights {
            if on != (wire == 2) {
                return Err("native: a light's switch is not its display type".into());
            }
            n.lights_on += on as usize;
        }
        // the roots: every root of the scene once, in the order of their elements in the
        // MODELS chapter (the builder adds each at the list's head, the command chains
        // each at the chain's head); the copies of masters among them
        let plain: Vec<&Root> = seen.roots.iter().filter(|r| !r.instance).collect();
        let (mut want, mut have): (Vec<u32>, Vec<u32>) = (placed.roots.iter().map(|r| r.0).collect(), plain.iter().map(|r| r.son).collect());
        want.sort();
        have.sort();
        if want != have {
            return Err(format!("native: {} roots handed over, the scene has {}", plain.len(), placed.roots.len()));
        }
        if plain.windows(2).any(|w| w[0].elem_id >= w[1].elem_id) || plain.iter().any(|r| r.elem_id < 0) {
            return Err("native: the roots are not handed over in the order of their elements".into());
        }
        // against the first pass's model of the same hand-over
        if ho.roots.len() != plain.len() || ho.lights.len() != chain.len() {
            return Err(format!("native: the first pass expects {} roots and {} lights", ho.roots.len(), ho.lights.len()));
        }
        // the dummies handed over are freed afterwards
        if seen.roots.iter().any(|r| !m.is_freed(r.dummy)) {
            return Err("native: a dummy root was not freed".into());
        }
        let none = si_rensetup::native::world::ImageReport { status: 0x7e, ..Default::default() };
        let image = world.images.first().unwrap_or(&none);
        let allocated = count(&world, out::SI_MODEL_ALLOCATE) - seen.allocated_before;
        n.scenes += 1;
        n.roots += plain.len();
        n.instance_roots += seen.roots.len() - plain.len();
        n.nodes_allocated += allocated - seen.roots.len();
        n.lights += chain.len();
        n.lights_not_listed += placed.lights.len() - listed.len();
        n.nodes_shown += seen.shown;
        n.nodes_visible += seen.visible;
        n.nodes_without_element += seen.without_element;
        n.frees_by_renderer += (seen.frees_after - seen.frees_before) as usize;
        n.frees_by_command += ((frees_end - frees_start) - (seen.frees_after - seen.frees_before)) as usize;
        for (k, c) in &seen.kinds {
            *n.kinds.entry(*k).or_default() += c;
        }
        n.model_nodes += placed.nodes;
        n.boxes += seen.boxes;
        n.copies_moved += seen.copies_moved;
        n.pictures_asked += world.pictures.asked.len();
        n.elements_pointed += placed.elements_in_image;
        n.elements_left += placed.elements_left;
        for (name, c) in &placed.left_by_chapter {
            *n.left_by_chapter.entry(String::from_utf8_lossy(name).into_owned()).or_default() += c;
        }
        *n.statuses.entry(image.status).or_default() += 1;
        if std::env::var_os("RENSETUP_LIST").is_some() {
            // RENSETUP_LIST=1: per scene, what SI_image answered, the triangles it filed, and
            // the model nodes in use by kind (+0x30) with the count of those that have a
            // geometry (+0x48 not null)
            let models = m[m[placed.dict].lists.add(m[g::SI_CID].model)];
            let mut kinds: std::collections::BTreeMap<i32, (usize, usize)> = Default::default();
            for i in 0..models.nb_elems {
                let e = models.elems.add(i);
                let node: Ptr32<Model> = m[e].data.cast();
                if m[e].status == ELEM_VALID && !node.is_null() && si_scene::world::obj_of(m[e].data) == 0 {
                    let k = kinds.entry(m.i32(node.0 + 0x30)).or_default();
                    k.0 += 1;
                    k.1 += (m.u32(node.0 + 0x48) != 0) as usize;
                }
            }
            println!("SCENE\t{:#x}\t{}\t{:?}", image.status, image.triangles, kinds);
        }
        *n.command_statuses.entry(status).or_default() += 1;
        n.triangles += image.triangles as i64;
        n.pictures += image.pictures.len();
        n.lines += image.pictures.iter().map(|p| p.1).sum::<usize>();
        for note in &image.notes {
            *n.notes.entry(note.clone()).or_default() += 1;
        }
        for text in &seen.messages {
            *n.messages.entry(text.trim().to_string()).or_default() += 1;
        }
        // relations 20002 of the scene's dictionary
        let models = m[m[placed.dict].lists.add(m[g::SI_CID].model)];
        for i in 0..models.nb_elems {
            let e = models.elems.add(i);
            if m[e].status == ELEM_VALID {
                let node: Ptr32<Model> = m[e].data.cast();
                if !node.is_null() && m[node].algo_type == rec::ALGO_INSTANCE {
                    let master = si_rensetup::native::instance::model_master_from_instance(&mut world, m, node);
                    n.relations += (!master.is_null()) as usize;
                }
            }
        }
        for (a, c) in &world.ported {
            *n.ported.entry(name_of(*a)).or_default() += c;
        }
        for (a, c) in &world.hooks {
            *n.hooks.entry(name_of(*a)).or_default() += c;
        }
        for (k, (p, h)) in &world.motion_calls {
            let e = n.motion_calls.entry(k.to_string()).or_default();
            e.0 += p;
            e.1 += h;
        }
        if let Some(f) = m.fault() {
            return Err(format!("native: the checks faulted: {f:?}"));
        }
        Ok(())
    })();
    sc.mem = m;
    r
}

fn relations(d: &Dict) -> usize {
    d.ed.lists.iter().flat_map(|l| l.elems.iter()).filter(|e| e.rec.status == ELEM_VALID).map(|e| e.forward.iter().map(|l| l.rels.len()).sum::<usize>()).sum()
}

/// A fingerprint of a dictionary's elements and relations: the scene's must be the same
/// after the render dictionary has come and gone, cursors and marks aside.
fn fingerprint(d: &Dict) -> Vec<(i32, i32, u32, i16, i32, i32, Vec<(i32, i32, i16)>)> {
    let mut v = Vec::new();
    for (c, l) in d.ed.lists.iter().enumerate() {
        for (i, e) in l.elems.iter().enumerate() {
            let rels = e.forward.iter().chain(e.backward.iter()).flat_map(|l| l.rels.iter().map(|r| (r.relid, r.elemid, r.chapid))).collect();
            v.push((c as i32, i as i32, e.rec.data.0, e.rec.status, e.rec.wire_type, e.rec.selection, rels));
        }
    }
    v
}

fn one_scene(sc: &mut Scene, t: &mut Totals, label: &str, host: &Host) -> Result<(), String> {
    let mut scene = std::mem::take(&mut sc.dict);
    let (mut env, bs) = bridge::env_of(sc);
    let c = env.p.cid;
    t.instances_in_scenes += bs.instances;
    if bs.instances > 0 {
        t.scenes_with_instances += 1;
    }
    // models whose materials are still only in their node (the load step that makes them
    // elements, SI_dbCheckConvertModelsMatTxt, is not ported in si-scene)
    for e in &scene.ed.lists[c.model as usize].elems {
        let has = e.forward.iter().flat_map(|l| l.rels.iter()).any(|r| r.relid == rel::MAT);
        if e.rec.status == ELEM_VALID && !has && env.heap.model(e.rec.data.0).is_some_and(|m| m.nb_materials > 0) {
            t.own_materials_only += 1;
            if e.rec.hier_level < 1 {
                t.own_materials_only_roots += 1;
            }
        }
    }
    let before = fingerprint(&scene);
    let ho = handover::from_dictionary(&scene, &env);
    native_one(sc, &env, &ho, &mut t.native, host)?;
    t.hand_over_roots += ho.roots.len();
    t.hand_over_lights += ho.lights.len();
    t.hand_over_instance_roots += ho.instance_roots.len();
    let mut corpus = Corpus::default();
    let mut out = Out { c: &mut corpus, p: si_scene::dict::Program::new(), g: Default::default() };
    out.p.cid = c;
    let mut ren = Dict::new();
    let mut st = rmain::RenState::default();
    let default_elem = rmain::create_render_dictionary(&mut scene, &mut ren, &mut env, &mut out, &mut st, true, true);
    let Some(default_elem) = default_elem else { return Err("no default material element".into()) };
    rmain::update_selective_lights(&ren, &mut env);
    let ctl = si_rensetup::udf::ScanControl { textures_2d: 1, textures_3d: 1, reflection: 1, atmosphere: 1, w25c: 0 };
    si_rensetup::udf::mental_shader_scan(&mut ren, &env, &mut out, &ctl, true);
    t.shaders_selected += out.c.rec.calls.iter().filter(|k| matches!(k, si_rensetup::outside::Call::UdfSetAsSelected(_))).count();
    if util::check_all_models(&mut ren, &env, &mut out, 1) != 0 {
        t.something_to_render += 1;
    }
    update::render_scene_update(0.0, env.g.scene_camera, &mut scene, &mut ren, &mut env, &mut out);
    // counts
    t.scene_elements += scene.ed.lists.iter().map(|l| l.elems.iter().filter(|e| e.rec.status == ELEM_VALID).count()).sum::<usize>();
    for (ci, l) in ren.ed.lists.iter().enumerate() {
        let n = l.elems.iter().filter(|e| e.rec.status == ELEM_VALID).count();
        let s = scene.ed.lists[ci].elems.iter().filter(|e| e.rec.status == ELEM_VALID).count();
        let e = t.by_chapter.entry(String::from_utf8_lossy(&scene.ed.lists[ci].name).into_owned()).or_default();
        e.0 += s;
        e.1 += n;
        t.render_elements += n;
    }
    t.relations_scene += relations(&scene);
    t.relations_copied += st.counts.relations_copied;
    t.relations_render += relations(&ren);
    t.instances_expanded += st.counts.instances_expanded;
    t.elements_added += st.counts.instance_elements_added;
    t.roots_given_default += st.counts.default_material_given;
    for e in &ren.ed.lists[c.model as usize].elems {
        if e.rec.status != ELEM_VALID {
            continue;
        }
        t.models += 1;
        let mats: Vec<i32> = e.forward.iter().flat_map(|l| l.rels.iter()).filter(|r| r.relid == rel::MAT && r.chapid as i32 == c.mat).map(|r| r.elemid).collect();
        if mats.contains(&default_elem.index) {
            t.models_with_default += 1;
        }
        if !mats.is_empty() {
            t.models_with_material += 1;
        }
        if e.rec.work_id != -1 {
            t.models_inheriting += 1;
        }
    }
    for l in &ren.ed.lists {
        for e in &l.elems {
            if e.rec.status == ELEM_VALID {
                t.flags[(e.rec.work_flag & 7) as usize] += 1;
            }
        }
    }
    t.selective_lights += ren.ed.lists[c.lig as usize].elems.iter().filter(|e| env.heap.words(env.heap.image(e.rec.data.0).map_or(0, |im| im.u32(si_rensetup::world::lig::TABLE))).is_some()).count();
    rmain::delete_render_dictionary(&scene, &mut ren, &mut env, Some(default_elem), true);
    t.left_after_delete += ren.ed.lists.len();
    if fingerprint(&scene) != before {
        t.scene_changed += 1;
        return Err(format!("{label}: the scene's dictionary changed"));
    }
    t.nodes_copied += corpus.nodes_copied;
    t.uv_warnings += corpus.rec.calls.iter().filter(|k| matches!(k, si_rensetup::outside::Call::WarnUv(_))).count();
    t.son_state_calls += corpus.rec.calls.iter().filter(|k| matches!(k, si_rensetup::outside::Call::UpdateSonState { .. })).count();
    Ok(())
}

/// What a line of the load's report says the load missed, or None when it missed nothing.
fn missed(o: &load::Outcome) -> Option<String> {
    Some(match o {
        load::Outcome::Missing { tried } => format!("file missing (last tried {})", String::from_utf8_lossy(tried)),
        load::Outcome::Invalid { path } => format!("file refused ({})", String::from_utf8_lossy(path)),
        load::Outcome::NodeMissing => "node missing in the root's file".into(),
        load::Outcome::NoModel => "no model to read it onto".into(),
        _ => return None,
    })
}

fn corpus(dir: &Path) {
    let mut files = Vec::new();
    walk(dir, &mut files);
    let Some(rsrc_path) = files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned() else {
        eprintln!("no Relations.rsrc under {}", dir.display());
        std::process::exit(2);
    };
    let rsrc = std::fs::read(&rsrc_path).expect("Relations.rsrc");
    println!("relations resource file: {}", rsrc_path.display());
    // the program's default picture, beside the relations' resource file
    // (RENSETUP_NO_ICON=1 withholds it, to see what the renderer does without)
    let no_icon = std::fs::read(rsrc_path.with_file_name("noIcon.pic")).ok().filter(|_| std::env::var_os("RENSETUP_NO_ICON").is_none());
    println!("default picture noIcon.pic: {}", if no_icon.is_some() { "found beside it" } else { "not found" });
    // the resource directory, absolute, as `SI_resdir` holds it
    let resdir = rsrc_path.parent().and_then(|d| d.canonicalize().ok()).map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    let scenes: Vec<&PathBuf> =
        files.iter().filter(|p| p.extension().is_some_and(|e| e == "dsc") && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "SCENES")).collect();
    let fs = RealFs;
    let mut t = Totals::default();
    let hook = std::panic::take_hook();
    if std::env::var_os("RENSETUP_PANIC").is_none() {
        std::panic::set_hook(Box::new(|_| {}));
    }
    // RENSETUP_ONLY=<text>: only the scenes whose path holds the text (for a look at one)
    let only = std::env::var("RENSETUP_ONLY").ok();
    // RENSETUP_PICTURES=<dir>: the frames as picture files (see `Pictures`)
    let mut pictures = std::env::var_os("RENSETUP_PICTURES").map(|d| Pictures::new(PathBuf::from(d)));
    for path in scenes {
        if only.as_deref().is_some_and(|o| !path.to_string_lossy().contains(o)) {
            continue;
        }
        let data = std::fs::read(path).unwrap_or_default();
        if !data.starts_with(b"SDSC") {
            continue;
        }
        t.scenes += 1;
        let db = path.parent().unwrap().parent().unwrap();
        let name = path.file_stem().unwrap().as_bytes().to_vec();
        let label = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let db_label = db.strip_prefix(dir).unwrap_or(db).display().to_string();
        let scene_name = String::from_utf8_lossy(&name).into_owned();
        let picture_label = pictures.as_mut().map(|p| p.label(&scene_name, db));
        let mut sc = Scene::start(&rsrc).expect("Relations.rsrc");
        start_up(&mut sc.mem, &resdir);
        // the stores' copy of the globals after the start-up (the load writes the back-up
        // switch `SI_glbManip` +0x36 from it: 1 after `SI_manipulationInit`, which selects
        // the optimised untold relations, finding 0018, section 19)
        sc.program.backup_enabled = sc.mem.i16(0x0878_1560 + 0x36) != 0;
        // `standAloneRender` sets SI_standaloneRenderer to 1 before it loads the scene
        // (RENSETUP_INTERACTIVE=1 leaves it 0, as the check had it before)
        sc.standalone_renderer = std::env::var_os("RENSETUP_INTERACTIVE").is_none();
        // the flag is set before the start-up's calls, and `SI_metaballInit (0x0845abc8)` follows `SI_matterInit`:
        // in the stand-alone renderer it takes the write bit off the metaball attributes of the dictionary
        // (RENSETUP_NO_METABALL_INIT=1 leaves it out, as the check had it before)
        sc.mem.set_i16(0x0867_9400, sc.standalone_renderer as i16);
        if std::env::var_os("RENSETUP_NO_METABALL_INIT").is_none() {
            si_rensetup::native::world::start_up_metaball_init(&mut sc.mem);
        }
        // one process per scene, as the original's batch renderer: the C library's
        // generator as after start-up
        si_rensetup::native::motion::start_process();
        sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
        let mut info = DbInfo::new();
        info.name = db.file_name().map(|n| n.as_bytes().to_vec());
        info.path = Some(db.as_os_str().as_bytes().to_vec());
        let mut log = Log::default();
        si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
        // the load's SI_sceneRefresh run on the image (RENSETUP_NO_LOAD_REFRESH=1: recorded)
        let mut lc = si_rensetup::native::motion::LoadCalls { refresh: std::env::var_os("RENSETUP_NO_LOAD_REFRESH").is_none(), ..Default::default() };
        let ok = load::load_scene(&mut sc, &fs, &mut info, &name, &mut lc);
        t.native.load_refreshed += lc.refreshed as usize;
        for (k, (p, h)) in &lc.calls {
            let e = t.native.load_motion_calls.entry(k.to_string()).or_default();
            e.0 += p;
            e.1 += h;
        }
        let complete = ok && sc.report.iter().all(|r| !matches!(r.outcome, load::Outcome::Missing { .. } | load::Outcome::Invalid { .. } | load::Outcome::NodeMissing | load::Outcome::NoModel));
        if !complete {
            // rendered anyway, as the original carries on past these (finding 0018, section
            // 7.1; finding 0023, section 29), after a line for each piece the load missed;
            // RENSETUP_SKIP_INCOMPLETE=1 skips the scene instead, as before
            if std::env::var_os("RENSETUP_SKIP_INCOMPLETE").is_some() {
                if let (Some(p), Some(l)) = (pictures.as_mut(), &picture_label) {
                    p.line(&db_label, &scene_name, l, Err("not loaded completely".into()));
                }
                continue;
            }
            println!("{label}: not loaded completely{}, rendered anyway:", if ok { "" } else { " (the load answered false)" });
            for r in &sc.report {
                if let Some(what) = missed(&r.outcome) {
                    let chap = si_db::chap_info_from_chap_id(r.chap).map_or_else(|| r.chap.to_string(), |c| c.directory.to_string());
                    println!("  {chap} [{}]: {what}", String::from_utf8_lossy(&r.name));
                }
            }
            t.rendered_incomplete += 1;
        } else {
            t.loaded += 1;
        }
        // the totals before this scene: a scene that panics adds nothing to them
        let kept = t.clone();
        let mut tt = std::mem::take(&mut t);
        let host = Host {
            no_icon: no_icon.clone(),
            database: db.as_os_str().as_bytes().to_vec(),
            pictures: pictures.as_ref().zip(picture_label.clone()).map(|(p, l)| (p.root.clone(), l)),
            row: Default::default(),
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let r = one_scene(&mut sc, &mut tt, &label, &host);
            (tt, r)
        }));
        si_render::adapter::write_pictures_under(None);
        if let (Some(p), Some(l)) = (pictures.as_mut(), &picture_label) {
            let row = host.row.borrow_mut().take();
            let done = match (&r, row) {
                (_, Some(row)) => Ok(row),
                (Err(e), None) => Err(format!("panic: {}", e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
                (Ok((_, Err(e))), None) => Err(e.clone()),
                (Ok((_, Ok(()))), None) => Err("no render".into()),
            };
            p.line(&db_label, &scene_name, l, done);
        }
        match r {
            Ok((tt, Ok(()))) => {
                t = tt;
                t.prepared += 1;
            }
            Ok((tt, Err(e))) => {
                t = tt;
                t.failures.push(format!("{label}: {e}"));
            }
            Err(_) => {
                t = kept;
                t.failures.push(format!("{label}: the set-up panicked"));
            }
        }
    }
    std::panic::set_hook(hook);
    if let Some(p) = &pictures {
        println!("picture files written {}; index: {}", p.written, p.root.join("index.tsv").display());
    }
    println!("scene descriptions                              {}", t.scenes);
    println!("loaded completely by si-scene                   {}", t.loaded);
    if t.rendered_incomplete != 0 {
        println!("not loaded completely, rendered anyway          {}", t.rendered_incomplete);
    }
    println!("render dictionary made, updated, deleted        {}", t.prepared);
    println!("elements in use: scenes {}, render dictionaries {}", t.scene_elements, t.render_elements);
    println!("  {:<26} {:>8} {:>8}", "chapter", "scene", "render");
    for (name, (s, r)) in &t.by_chapter {
        if *s != 0 || *r != 0 {
            println!("  {:<26} {:>8} {:>8}", name, s, r);
        }
    }
    println!("relations (forward entries): scenes {}, handed to the copy {}, in the render dictionaries {}", t.relations_scene, t.relations_copied, t.relations_render);
    println!("instance nodes in the scenes {} (in {} scenes); expanded {}; nodes copied {}; elements added {}", t.instances_in_scenes, t.scenes_with_instances, t.instances_expanded, t.nodes_copied, t.elements_added);
    println!("model elements in the render dictionaries       {}", t.models);
    println!("  roots without material, given the default     {}", t.roots_given_default);
    println!("    of them with materials in their node only   {}", t.own_materials_only_roots);
    println!("  related to the default material in the end    {}", t.models_with_default);
    println!("  with materials in their node only (any depth) {}", t.own_materials_only);
    println!("  that took their materials from an ancestor    {}", t.models_inheriting);
    println!("  with a material in the end                    {}", t.models_with_material);
    println!("warnings for inherited UV textures              {}", t.uv_warnings);
    println!("shader records the scan marks as used           {}", t.shaders_selected);
    println!("light elements with a selective table           {}", t.selective_lights);
    println!("scenes with something to render                 {}", t.something_to_render);
    println!("node matrix updates asked for in one frame      {}", t.son_state_calls);
    println!("transmit flags (bits 1, 2, 4) of render elements: {:?}", t.flags);
    println!("native hand-over: {} root hierarchies ({} of them instances), {} lights", t.hand_over_roots, t.hand_over_instance_roots, t.hand_over_lights);
    let n = &t.native;
    println!("on the loader's image: SI_sequenceRender run on {} scenes; SI_image handed {} roots, {} copies of masters, {} lights ({} switched on)", n.scenes, n.roots, n.instance_roots, n.lights, n.lights_on);
    println!("  model nodes with a graf node {} ({} visible, {} without an element); instances with a master {}; nodes allocated for copies {}; lights of a type that enters no list {}", n.nodes_shown, n.nodes_visible, n.nodes_without_element, n.relations, n.nodes_allocated, n.lights_not_listed);
    println!("  copies of masters standing where their instance stands {} of {} ({} of them away from the origin)", n.instance_roots, n.instance_roots, n.copies_moved);
    println!("  blocks freed by the command itself {}, by the renderer's exit {}", n.frees_by_command, n.frees_by_renderer);
    println!("  graf nodes by kind: {}", n.kinds.iter().map(|(k, c)| format!("{k}: {c}")).collect::<Vec<_>>().join(", "));
    println!("  sub-region boxes asked of the user (accepted by the check) {}; picture files asked of the host {}", n.boxes, n.pictures_asked);
    println!("  in the image as the loader left it: {} model nodes of the hierarchies read; {} elements with their object a record of the image, {} with the loader's handle", n.model_nodes, n.elements_pointed, n.elements_left);
    println!("  elements left with the loader's handle, by chapter: {}", n.left_by_chapter.iter().map(|(k, c)| format!("{k} {c}")).collect::<Vec<_>>().join(", "));
    println!("  SI_image answered: {}; the command: {}", n.statuses.iter().map(|(k, c)| format!("0x{k:x} {c} times")).collect::<Vec<_>>().join(", "), n.command_statuses.iter().map(|(k, c)| format!("0x{k:x} {c} times")).collect::<Vec<_>>().join(", "));
    println!("  triangles filed {}; pictures handed to the picture writer {} ({} lines)", n.triangles, n.pictures, n.lines);
    println!("  what the renderer's set-up said:");
    for (text, count) in &n.notes {
        println!("    {:>8}  {}", count, text);
    }
    println!("  status line:");
    for (text, count) in &n.messages {
        println!("    {:>8}  {}", count, text);
    }
    println!("  calls answered by a port on the image:");
    for (name, count) in &n.ported {
        println!("    {:<36} {:>10}", name, count);
    }
    println!("  calls of routines that are not on the image (hooks, answered 0):");
    for (name, count) in &n.hooks {
        println!("    {:<36} {:>10}", name, count);
    }
    println!("  calls the scene's update made out of si-motion (host method: answered by a port, answered 0):");
    for (name, (p, h)) in &n.motion_calls {
        println!("    {:<36} {:>10} {:>10}", name, p, h);
    }
    println!("the load's SI_sceneRefresh run on the image in {} scenes; the calls its update made out of si-motion (answered by a port, answered 0):", n.load_refreshed);
    for (name, (p, h)) in &n.load_motion_calls {
        println!("    {:<36} {:>10} {:>10}", name, p, h);
    }
    println!("scene dictionaries changed by it all            {}", t.scene_changed);
    println!("chapters left in deleted render dictionaries    {}", t.left_after_delete);
    println!("failures                                        {}", t.failures.len());
    for f in &t.failures {
        println!("  {f}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("rules") if args.len() == 2 => rules(&PathBuf::from(&args[1])),
        Some(dir) if args.len() == 1 => corpus(Path::new(dir)),
        _ => {
            eprintln!("usage: rensetup-check <dir> | rensetup-check rules <Relations.rsrc>");
            std::process::exit(2);
        }
    }
}
