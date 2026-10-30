// SPDX-License-Identifier: MIT
//! Unit tests of the native hand-over on made-up data. They show that the routines run on
//! the image and do what finding 0023 says; the comparison with the original is
//! `rensetup-emucheck` (finding 0023, section 17).

use super::globals::*;
use super::outside::*;
use super::rec::{ctl, flag, light, ALGO_INSTANCE};
use super::{instance, matter, rnd_dial};
use si_view::graflist;
use si_mem::{FnPtr, Mem, Ptr32};
use si_types::dict::{Elem, ElemDict, ElemList};
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};

/// The rest of the program for these tests: a node allocator, the instance relation as a
/// table, and a log of every call.
#[derive(Default)]
struct Mock {
    log: Vec<(u32, Vec<u32>)>,
    /// (model, its element).
    elems: Vec<(u32, u32)>,
    /// (instance's element, master's element).
    masters: Vec<(u32, u32)>,
}

impl Outside for Mock {
    fn call(&mut self, m: &mut Mem, f: FnPtr, args: &[u32]) -> u32 {
        self.log.push((f.0, args.to_vec()));
        if f == SI_MODEL_ALLOCATE {
            let p = m.calloc(1, 0x430);
            m.set_u32(p + 0x44, p);
            m.set_i32(p + 0x354, -1);
            if args[0] != 0 {
                let name = m.strdup(args[0]);
                m.set_u32(p + 4, name);
            }
            p
        } else if f == SI_ELEMDICT_GET_ELEM_FROM_DATA {
            self.elems.iter().find(|e| e.0 == args[2]).map_or(0, |e| e.1)
        } else if f == SI_ELEMDICT_FIND_RELATED_ELEM && args[2] == 1 {
            self.masters.iter().find(|e| e.0 == args[1]).map_or(0, |e| e.1)
        } else {
            0
        }
    }
}

struct World {
    m: Mem,
    list: Ptr32<GrafList>,
    elems: Ptr32<Elem>,
    nb_elems: i32,
    mock: Mock,
}

fn world() -> World {
    let mut m = Mem::new();
    init(&mut m);
    let list: Ptr32<GrafList> = Ptr32::new(m.place(0x24, 8));
    m[SI_GRAF_LISTS] = list;
    let dict: Ptr32<ElemDict> = Ptr32::new(m.place(0x70, 8));
    m[SI_ELEM_DICTS] = dict;
    let chapters: Ptr32<ElemList> = Ptr32::new(m.place(0x40, 8));
    m[dict].lists = chapters;
    let elems: Ptr32<Elem> = Ptr32::new(m.place(0x84 * 8, 8));
    m[chapters].elems = elems;
    let camera = m.place(0x13c, 8);
    let proc = m[SI_PROC_DATA];
    m[proc].camera = Ptr32::new(camera);
    World { m, list, elems, nb_elems: 0, mock: Mock::default() }
}

impl World {
    fn model(&mut self) -> Ptr32<Model> {
        let p: Ptr32<Model> = Ptr32::new(self.m.place(0x430, 8));
        self.m[p].latdep = p;
        let e = self.elems.add(self.nb_elems);
        self.m[e].data = p.cast();
        self.m[p].elem_id = self.nb_elems;
        self.nb_elems += 1;
        let chapter = self.m[self.m[SI_ELEM_DICTS]].lists;
        self.m[chapter].nb_elems = self.nb_elems;
        self.mock.elems.push((p.0, e.0));
        p
    }

    /// A graf node at the end of the scene's list.
    fn graf(&mut self, kind: i32, data: u32, selection: i32, wire_type: i32) -> Ptr32<GrafNode> {
        let g: Ptr32<GrafNode> = Ptr32::new(self.m.place(0x24, 8));
        self.m[g].kind = kind;
        self.m[g].data = Ptr32::new(data);
        self.m[g].selection = selection;
        self.m[g].wire_type = wire_type;
        let mut last = self.m[self.list].first;
        if last.is_null() {
            self.m[self.list].first = g;
        } else {
            while !self.m[last].next.is_null() {
                last = self.m[last].next;
            }
            self.m[last].next = g;
        }
        g
    }

    fn instance_of(&mut self, instance: Ptr32<Model>, master: Ptr32<Model>) {
        self.m[instance].algo_type = ALGO_INSTANCE;
        let (i, k) = (self.m[instance].elem_id, self.m[master].elem_id);
        self.mock.masters.push((self.elems.add(i).0, self.elems.add(k).0));
    }

    fn calls(&self, f: FnPtr) -> Vec<&Vec<u32>> {
        self.mock.log.iter().filter(|c| c.0 == f.0).map(|c| &c.1).collect()
    }
}

#[test]
fn the_callee_table_has_one_name_for_each_address() {
    for (i, a) in CALLEES.iter().enumerate() {
        assert!(si_mem::is_code(a.0) || ["sleep", "access", "getcwd", "getenv", "usleep", "tempnam", "unlink"].contains(&a.1), "{} is not in .text", a.1);
        assert!(CALLEES[..i].iter().all(|b| b.0 != a.0 && b.1 != a.1), "{} twice", a.1);
    }
    assert_eq!(name_of(SI_IMAGE), Some("SI_image"));
}

#[test]
fn a_graf_node_is_found_by_object_and_kind() {
    si_rt::skip_without_install!();
    let mut w = world();
    let (a, b) = (w.model(), w.model());
    let ga = w.graf(2, a.0, 0, 2);
    let gb = w.graf(3, b.0, 0, 2);
    assert_eq!(graflist::find_gl_data(&w.m, w.list, a.cast(), 0), ga);
    // kind 3 counts as 2 on both sides
    assert_eq!(graflist::find_gl_data(&w.m, w.list, b.cast(), 2), gb);
    assert_eq!(graflist::find_gl_data(&w.m, w.list, a.cast(), 3), ga);
    assert!(graflist::find_gl_data(&w.m, w.list, a.cast(), 6).is_null());
    assert!(graflist::find_gl_data(&w.m, w.list, Ptr32::new(0x1234), 0).is_null());
    assert!(w.m.fault().is_none());
}

#[test]
fn a_shared_material_array_is_registered_and_freed() {
    si_rt::skip_without_install!();
    let mut w = world();
    let (from, to) = (w.model(), w.model());
    let array: Ptr32<Ptr32<si_types::scene::Material>> = Ptr32::new(w.m.place(8, 8));
    w.m[array] = Ptr32::new(0x5000_0010);
    w.m[array.add(1)] = Ptr32::new(0x5000_0020);
    w.m[from].materials = array;
    w.m[from].nb_materials = 2;
    w.m[from].cur_material = 1;
    instance::set_register_inst_mat_arrays_bool(&mut w.m, 1);
    instance::instance_material_share(&mut w.m, from, to);
    let copy = w.m[to].materials;
    assert_ne!(copy, array);
    assert_eq!((w.m[to].nb_materials, w.m[to].cur_material), (2, 1));
    assert_eq!((w.m[copy].0, w.m[copy.add(1)].0), (0x5000_0010, 0x5000_0020));
    let entry = w.m[MEM_LIST];
    assert_eq!(w.m[entry].block.0, copy.0);
    instance::delete_mem_references(&mut w.m);
    assert!(w.m[MEM_LIST].is_null());
    assert_eq!(w.m[REG_INST_MAT_ARRAYS], 0);
    assert_eq!(w.m.freed(), &[copy.0, entry.0]);
    // without the switch nothing is entered
    instance::instance_material_share(&mut w.m, from, to);
    assert!(w.m[MEM_LIST].is_null());
    assert!(w.m.fault().is_none());
}

#[test]
fn the_search_for_an_instance_passes_over_visited_ones() {
    si_rt::skip_without_install!();
    let mut w = world();
    let (root, son, grandson) = (w.model(), w.model(), w.model());
    w.m[root].son = son;
    w.m[son].father = root;
    w.m[son].son = grandson;
    w.m[grandson].father = son;
    w.m[son].algo_type = ALGO_INSTANCE;
    w.m[grandson].algo_type = ALGO_INSTANCE;
    assert_eq!(instance::find_instance_in_hierarchy(&mut w.m, root, Ptr32::NULL, 0), son);
    w.m[son].flags_08 |= flag::VISITED;
    assert_eq!(instance::find_instance_in_hierarchy(&mut w.m, root, Ptr32::NULL, 0), grandson);
    assert_eq!(instance::find_instance_in_hierarchy(&mut w.m, root, Ptr32::NULL, 1), son);
    w.m[grandson].flags_08 |= flag::VISITED;
    assert!(instance::find_instance_in_hierarchy(&mut w.m, root, Ptr32::NULL, 0).is_null());
    assert_eq!(w.m.sp(), si_mem::STACK_START);
    assert!(w.m.fault().is_none());
}

#[test]
fn the_sub_region_becomes_pixels() {
    si_rt::skip_without_install!();
    let mut w = world();
    let c = SI_REN.0;
    w.m.set_i16(c + ctl::WIDTH, 640);
    w.m.set_i16(c + ctl::HEIGHT, 480);
    for (offset, fraction) in [(ctl::CROP_LEFT, 0.25f32), (ctl::CROP_RIGHT, 0.25), (ctl::CROP_TOP, 0.5), (ctl::CROP_BOTTOM, 0.25)] {
        w.m.set_f32(c + offset, fraction);
    }
    matter::update_subregion_values(&mut w.mock, &mut w.m, c);
    assert_eq!([ctl::CROP_LEFT, ctl::CROP_RIGHT, ctl::CROP_TOP, ctl::CROP_BOTTOM].map(|o| w.m.f32(c + o)), [160.0, 160.0, 240.0, 120.0]);
    assert_eq!((w.m.i16(c + ctl::CROP_WIDTH), w.m.i16(c + ctl::CROP_HEIGHT)), (320, 120));
    assert!(w.mock.log.is_empty());
    // a second call takes the pixel counts for fractions: the width comes out as
    // 640 - 102400 - 102400, whose low 16 bits are negative, so the status line speaks
    // and the width becomes 1; the height, 480 - 115200 - 57600, has low 16 bits that
    // read as 24288 and is kept
    matter::update_subregion_values(&mut w.mock, &mut w.m, c);
    assert_eq!((w.m.i16(c + ctl::CROP_WIDTH), w.m.i16(c + ctl::CROP_HEIGHT)), (1, 24288));
    assert_eq!(w.calls(SI_STATUS_DISPLAY_MESSAGE).len(), 1);
    assert_eq!(w.m.sp(), si_mem::STACK_START);
    assert!(w.m.fault().is_none());
}

#[test]
fn the_render_command_hands_over_roots_an_instance_copy_and_lights() {
    si_rt::skip_without_install!();
    let mut w = world();
    let (master, child, inst) = (w.model(), w.model(), w.model());
    w.m[master].son = child;
    w.m[child].father = master;
    w.m[master].kind = 4;
    w.m[child].kind = 2;
    w.instance_of(inst, master);
    w.m[inst].gscl = si_types::geom::Vector { x: 2.0, y: 2.0, z: 2.0 };
    w.m[inst].grot = si_math::types::IDENTITY;
    w.m[inst].gtrs = si_types::geom::Vector { x: 5.0, y: 0.0, z: 0.0 };
    w.graf(2, master.0, 0, 2);
    w.graf(3, child.0, 0, 0);
    let lamp = w.m.place(0x240, 8);
    w.graf(6, lamp, 0, 2);
    w.graf(2, inst.0, 1, 1);
    let hidden_lamp = w.m.place(0x240, 8);
    w.graf(8, hidden_lamp, 0, 1);
    w.m.set_u16(SI_DEFAULT_CONTROL.0 + ctl::CROP, 1);

    let status = rnd_dial::sequence_render(&mut w.mock, &mut w.m);
    assert_eq!(status, 0);
    assert!(w.m.fault().is_none(), "{:?}", w.m.fault());
    // the controls were copied and the renderer's crop switch cleared
    assert_eq!(w.m.u16(SI_REN.0 + ctl::CROP), 0);
    assert_eq!(w.m[REG_INST_MAT_ARRAYS], 1);
    // visible: the display type is not "hidden"
    assert_eq!((w.m[master].visible, w.m[child].visible, w.m[inst].visible), (1, 0, 1));

    let image = w.calls(SI_IMAGE);
    assert_eq!(image.len(), 1);
    let (lights, roots) = (image[0][3], Ptr32::<Model>::new(image[0][4]));
    assert_eq!(image[0][0], w.m[w.m[SI_PROC_DATA]].camera.0);
    // lights: last of the list first; on only with display type 2
    assert_eq!(lights, hidden_lamp);
    assert_eq!(w.m.u32(hidden_lamp + light::NEXT), lamp);
    assert_eq!((w.m.u16(hidden_lamp + light::ON), w.m.u16(lamp + light::ON)), (0, 1));
    // roots, last made first: the instance itself, the copy of its master, the master
    let over_instance = roots;
    let over_copy = w.m[over_instance].brother;
    let over_master = w.m[over_copy].brother;
    assert!(w.m[over_master].brother.is_null());
    assert_eq!((w.m[over_instance].son, w.m[over_master].son), (inst, master));
    assert_eq!((w.m[over_instance].kind, w.m[over_instance].algo_type), (-1, 0));
    assert_eq!(w.m[over_copy].algo_type, ALGO_INSTANCE);
    let copy = w.m[over_copy].son;
    assert_eq!(w.m[copy].father, over_copy);
    assert_eq!((w.m[copy].kind, w.m[copy].elem_id), (4, w.m[inst].elem_id));
    assert_ne!(w.m[copy].flags_08 & flag::COPY, 0);
    // the copy stands where the instance stands; its son is the copy of the master's son
    assert_eq!((w.m[copy].scal.x, w.m[copy].trans.x), (2.0, 5.0));
    let copied_child = w.m[copy].son;
    assert_eq!((w.m[copied_child].kind, w.m[copied_child].father, w.m[copied_child].elem_id), (2, copy, w.m[child].elem_id));
    // visible of the copies: the instance is shown (ghosted), the master's son is hidden
    assert_eq!((w.m[copy].visible, w.m[copied_child].visible), (1, 0));
    assert_ne!(w.m[inst].flags_08 & flag::VISITED, 0);
    // the three dummies are freed afterwards, the copies are not
    assert_eq!(w.m.freed(), &[over_instance.0, over_copy.0, over_master.0]);
    assert_eq!(w.calls(SI_CHUNK_FREE_ALL).len(), 1);
    assert_eq!(w.m.sp(), si_mem::STACK_START);
}

#[test]
fn the_preview_refuses_a_picture_larger_than_the_screen() {
    si_rt::skip_without_install!();
    let mut w = world();
    let a = w.model();
    w.graf(2, a.0, 1, 2);
    w.m[SI_XMAXSCREEN] = 1279;
    w.m[SI_YMAXSCREEN] = 1023;
    let dc = SI_DEFAULT_CONTROL.0;
    w.m.set_i16(dc + ctl::PREVIEW_BLOCK + (ctl::WIDTH - ctl::SWITCHES), 2000);
    w.m.set_i16(dc + ctl::PREVIEW_BLOCK + (ctl::HEIGHT - ctl::SWITCHES), 100);
    w.m.set_u16(dc + ctl::SUBREGION_226, 1);
    matter::frame_render(&mut w.mock, &mut w.m, 0, 0, 0);
    assert!(w.calls(SI_IMAGE).is_empty());
    let said = w.calls(SI_STATUS_DISPLAY_MESSAGE);
    assert_eq!(said.len(), 1);
    // the text was in a local of the routine: gone with it, but the bytes are still there
    assert_eq!(w.m.cstr(said[0][0]), b" Image too large for frame buffer output: max. 1280 x 1024.\n");
    // the sub-region switch stays cleared on this path
    assert_eq!(w.m.u16(dc + ctl::SUBREGION_226), 0);

    // a picture that fits is rendered: one frame, the current one, level 1
    w.m.set_i16(dc + ctl::PREVIEW_BLOCK + (ctl::WIDTH - ctl::SWITCHES), 320);
    w.m.set_u16(dc + ctl::SUBREGION_226, 1);
    w.m[SI_FRAME_CURRENT] = 12;
    w.mock.log.clear();
    matter::frame_render(&mut w.mock, &mut w.m, 0, 0, 0);
    let ren = SI_REN.0;
    assert_eq!(w.calls(SI_IMAGE).len(), 1);
    assert_eq!((w.m.u16(ren + ctl::FRAME_FIRST), w.m.u16(ren + ctl::FRAME_LAST), w.m.u16(ren + ctl::FRAME_STEP)), (12, 12, 1));
    assert_eq!((w.m.u16(ren + ctl::LEVEL), w.m.u16(ren + ctl::ADAPTIVE), w.m.u16(ren + ctl::DISPLAY)), (1, 0, 1));
    // centred: (1280 - 320) / 2 and (1024 - 100) / 2, each less one after the half is added
    assert_eq!((w.m.i16(ren + ctl::WINDOW_X), w.m.i16(ren + ctl::WINDOW_Y)), (479, 461));
    assert_eq!(w.m.u16(dc + ctl::SUBREGION_226), 1);
    assert_eq!(w.m[a].visible, 1);
    assert!(w.m.fault().is_none(), "{:?}", w.m.fault());
}

#[test]
fn a_new_node_has_its_name_its_number_and_itself_as_dependency() {
    si_rt::skip_without_install!();
    let mut m = Mem::new();
    init(&mut m);
    m[MODEL_ALLOCATE_COUNT] = 41;
    let unnamed = super::model_b::model_allocate(&mut m, Ptr32::NULL);
    assert_eq!(m.cstr(m[unnamed].name.0), b"obj42");
    assert_eq!((m[unnamed].serial, m[unnamed].elem_id, m[unnamed].visibility, m[unnamed].nb_uv), (42, -1, 7, 1));
    assert_eq!(m[unnamed].latdep, unnamed);
    assert_eq!(m[unnamed].gmat[3][3], 1.0);
    let text = m.place_cstr(b"cube");
    let named = super::model_b::model_allocate(&mut m, Ptr32::new(text));
    assert_eq!(m.cstr(m[named].name.0), b"cube");
    assert_ne!(m[named].name.0, text);
    assert_eq!(m[MODEL_ALLOCATE_COUNT], 43);
    assert!(m.fault().is_none());
}

#[test]
fn an_envelope_record_is_copied_with_its_lists_and_curves() {
    si_rt::skip_without_install!();
    use super::envtools::*;
    use super::rec::{EnvInfo, EnvList, EnvVert};
    let mut m = Mem::new();
    init(&mut m);
    let from = envelop_info_allocate(&mut m);
    assert_eq!((m[from].level, m[from].matrix[2][2], m[from].inner), (-1, 1.0, [0.0, 1.0, 0.0, 1.0]));
    m[from].level = 2;
    let list = envelop_list_allocate(&mut m, 0x1234);
    let vert = envelop_vert_allocate(&mut m);
    m[vert].index = 7;
    m[list].verts = vert;
    m[from].lists = list;
    m[from].in_curve = Ptr32::new(m.si_malloc(0x320));
    let slot = m[from].in_curve.add(199);
    m[slot] = 0.5;
    let copy: Ptr32<EnvInfo> = envelop_info_copy(&mut m, from, 1);
    let copied: Ptr32<EnvList> = m[copy].lists;
    let v: Ptr32<EnvVert> = m[copied].verts;
    // the list's first word is not copied; the vertex is, with the weight it was made with
    assert_eq!((m[copy].level, m[copied].owner, m[v].index, m[v].weight), (2, 0, 7, 100.0));
    assert_eq!(m[m[copy].in_curve.add(199)], 0.5);
    assert!(m[copy].out_curve.is_null());
    let bare = envelop_info_copy(&mut m, from, 0);
    assert!(m[bare].lists.is_null() && m[bare].level == -1);
    assert!(m.fault().is_none());
}

#[test]
fn an_inverse_kinematics_record_is_copied_and_disposed_of_block_for_block() {
    use super::invkin::*;
    let mut m = Mem::new();
    let from = inv_kin_allocate(&mut m);
    let matrix = general_matrix_allocate_dbl(&mut m, 2, 3);
    let cell = m[m[matrix].data.add(1)].add(2);
    m.set_u64(cell.0, 1.5f64.to_bits());
    m.set_u32(from.0 + ik::MATRIX_A, matrix.0);
    let before = m.heap_blocks().len();
    let copy = inv_kin_copy(&mut m, from);
    // the record, the matrix record, its row array and two rows
    assert_eq!(m.heap_blocks().len() - before, 5);
    let copied: Ptr32<GenMatD> = Ptr32::new(m.u32(copy.0 + ik::MATRIX_A));
    assert_eq!(f64::from_bits(m.u64(m[m[copied].data.add(1)].add(2).0)), 1.5);
    assert_eq!((m.u16(copy.0 + 0x3c), m.u32(copy.0 + ik::MATRIX_B)), (1, 0));
    let slot: Ptr32<Ptr32<si_mem::Opaque>> = m.place_record(copy);
    inv_kin_dispose(&mut m, slot);
    assert!(m[slot].is_null());
    assert_eq!(m.freed().len(), 5);
    assert!(m.fault().is_none());
}

#[test]
fn the_world_answers_the_hand_over_with_the_ports_on_the_image() {
    si_rt::skip_without_install!();
    use super::world::{NoWatch, World};
    let mut m = Mem::new();
    init(&mut m);
    let mut o = World::new(NoWatch);
    let node = o.call(&mut m, SI_MODEL_ALLOCATE, &[0]);
    assert_eq!(m.cstr(m.u32(node + 4)), b"obj1");
    // si-model's update on the image: a root's global matrix is its local one
    m.set_f32(node + 0x158, 3.0);
    o.call(&mut m, SI_MODEL_UPDATE_SON_STATE, &[0, node, 1]);
    assert_eq!(m.f32(node + 0x1c4 + 0x30), 3.0);
    // a callee nobody has on the image answers 0 and is counted
    assert_eq!(o.call(&mut m, SI_DO_SUBREGION, &[]), 0);
    assert_eq!(o.hooks.get(&SI_DO_SUBREGION.0), Some(&1));
    // the render window is this package's port: with neither display nor picture file,
    // an empty status line (a hook) and 0x7e
    assert_eq!(o.call(&mut m, SI_OPEN_RENDER_WINDOW, &[SI_REN.0]), 0x7e);
    assert_eq!(o.ported.get(&SI_OPEN_RENDER_WINDOW.0), Some(&1));
    assert_eq!(o.hooks.get(&SI_STATUS_DISPLAY_MESSAGE.0), Some(&1));
    assert_eq!(o.ported.get(&SI_MODEL_UPDATE_SON_STATE.0), Some(&1));
    assert!(m.fault().is_none(), "{:?}", m.fault());
}

#[test]
fn the_nodes_below_a_node_are_counted_and_a_light_has_its_kind() {
    si_rt::skip_without_install!();
    use super::edtools::{light_get_graf_type, model_nb_sub_models};
    let mut w = world();
    let (root, a, b, c) = (w.model(), w.model(), w.model(), w.model());
    for (father, son) in [(root, a), (a, c)] {
        w.m[father].son = son;
        w.m[son].father = father;
    }
    w.m[a].brother = b;
    w.m[b].father = root;
    assert_eq!([root, a, b, c].map(|n| model_nb_sub_models(&w.m, n)), [3, 1, 0, 0]);
    let lamp = w.m.place(0x240, 8);
    for (ty, kind) in [(1, 8), (2, 6), (3, 0x13), (4, 0x14), (0, 6), (9, 6)] {
        w.m.set_i16(lamp + 0x1e, ty);
        assert_eq!(light_get_graf_type(&w.m, lamp), kind);
    }
    assert!(w.m.fault().is_none());
}

#[test]
fn the_render_controls_start_as_the_program_leaves_them() {
    si_rt::skip_without_install!();
    let mut m = Mem::new();
    init(&mut m);
    m[SI_XMAXSCREEN] = 1279;
    m[SI_YMAXSCREEN] = 1023;
    let dc = SI_DEFAULT_CONTROL.0;
    matter::ren_control_init(&mut m, dc);
    matter::matter_fade_init(&mut m);
    matter::fog_init_default(&mut m);
    assert_eq!((m.i16(dc + ctl::WIDTH), m.i16(dc + ctl::HEIGHT)), (500, 424));
    assert_eq!((m.i16(dc + ctl::FRAME_FIRST), m.i16(dc + ctl::FRAME_LAST), m.i16(dc + ctl::FRAME_STEP)), (1, 100, 1));
    assert_eq!((m.i16(dc + ctl::DISPLAY), m.i16(dc + ctl::PICTURE_FILE)), (1, 1));
    assert_eq!(m.cstr(m.u32(dc + 0x17c)), b"default");
    assert_eq!((m.cstr(m.u32(dc + 0x180)), m.cstr(m.u32(dc + 0x184))), (Vec::new(), Vec::new()));
    assert_eq!(m.cstr(m.u32(dc + 0x2a0 + 0x1f4)), b"out.mi");
    assert_eq!(m.cstr(dc + 0x2a0 + 0x34), b"default");
    // (1279 - 500) / 2 and (1023 - 424) / 2
    assert_eq!((m.i16(dc + 0x5b2), m.i16(dc + 0x5b4)), (389, 299));
    assert_eq!((m.f32(dc + 0x104), m.f32(dc + 0x260)), (20.0, 0.2));
    assert!(m.fault().is_none());
}

#[test]
fn routing3_every_routed_address_reaches_its_port() {
    si_rt::skip_without_install!();
    use super::world::{NoWatch, World, ROUTING3};
    let mut m = Mem::new();
    init(&mut m);
    let mut o = World::new(NoWatch);
    // with null arguments the ports may fault on the image (recorded, not a panic); what is
    // asked here is that none of them is left as a hook
    for &a in ROUTING3 {
        let mut m2 = Mem::new();
        init(&mut m2);
        o.call(&mut m2, si_mem::FnPtr::new(a), &[0, 0, 0, 0]);
        assert_eq!(o.ported.get(&a), Some(&1), "0x{a:08x} is not routed");
        assert!(!o.hooks.contains_key(&a), "0x{a:08x} was a hook");
    }
    for &a in si_deform::routed::ROUTED {
        let mut m2 = Mem::new();
        init(&mut m2);
        let mut w = World::new(NoWatch);
        w.call(&mut m2, si_mem::FnPtr::new(a), &[0, 0, 0, 0]);
        if a == 0x0850_20a8 {
            // SI_nurbsUpdateFrenet is first answered by si-nurbs' router (it is in both ROUTED lists); that port refuses a
            // null/empty curve, as the original stops there, and a refusal goes on to the inner world, which records a
            // hook. So here the address must be in si-nurbs' list, and the call is not a miss of the dispatch.
            assert!(si_nurbs::routed::ROUTED.contains(&a));
            continue;
        }
        assert!(w.ported.contains_key(&a) && !w.hooks.contains_key(&a), "0x{a:08x} (si-deform's router) is a hook");
    }
    // the ones with a visible result
    let node = o.call(&mut m, SI_MODEL_ALLOCATE, &[0]);
    assert_eq!(o.call(&mut m, si_mem::FnPtr::new(0x084b_b94c), &[node]), 0);
    assert_eq!(o.call(&mut m, si_mem::FnPtr::new(0x081a_751c), &[node]), 0);
    assert_ne!(o.call(&mut m, si_mem::FnPtr::new(0x084a_d8f0), &[]), 0);
    let cam = o.call(&mut m, si_mem::FnPtr::new(0x080d_12dc), &[0, 0]);
    assert_ne!(cam, 0);
}

#[test]
fn routing4_every_routed_address_reaches_its_port() {
    si_rt::skip_without_install!();
    use super::world::{NoWatch, World, ROUTING4};
    // with null arguments the ports may fault on the image (recorded, not a panic); what is
    // asked here is that none of them is left as a hook
    for &a in ROUTING4 {
        let mut m = Mem::new();
        init(&mut m);
        let mut o = World::new(NoWatch);
        o.call(&mut m, si_mem::FnPtr::new(a), &[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(o.ported.get(&a), Some(&1), "0x{a:08x} is not routed");
        assert!(!o.hooks.contains_key(&a), "0x{a:08x} was a hook");
    }
}

#[test]
fn routing4_lattice_tests_agree() {
    si_rt::skip_without_install!();
    // SI_modelLattice / SI_modelTreeLattice: si-model's tests of the lattice's first word and
    // si-deform's of `active` (the same field) give the same answers on a made-up node
    use si_mem::Ptr32;
    for (n1, w1, n0, w0) in [(0u16, 0u16, 0u16, 0u16), (1, 1, 0, 0), (1, 0, 1, 1), (1, 0, 1, 0), (0, 0, 1, 7), (1, 5, 1, 7)] {
        let mut m = Mem::new();
        init(&mut m);
        let node = m.si_malloc(0x430);
        let (a1, a0) = (m.si_malloc(8), m.si_malloc(8));
        let (l1, l0) = (m.si_malloc(0x44), m.si_malloc(0x44));
        m.set_u32(a1, l1);
        m.set_u32(a0, l0);
        m.set_i16(l1, w1 as i16);
        m.set_i16(l0, w0 as i16);
        m.set_u16(node + 0x54, n1);
        m.set_u32(node + 0x58, a1);
        m.set_u16(node + 0x4c, n0);
        m.set_u32(node + 0x50, a0);
        let (s1, s2) = (m.si_malloc(2), m.si_malloc(2));
        m.set_i16(s1, 9);
        m.set_i16(s2, 9);
        let a = si_model::model_image::defs::lattice(&mut m, Ptr32::new(node), s1);
        let b = si_deform::lattice::model_lattice(&mut m, Ptr32::new(node), Ptr32::new(s2)).addr();
        assert_eq!((a, m.i16(s1)), (b, m.i16(s2)));
        let a = si_model::model_image::readjust::tree_lattice(&m, Ptr32::new(node));
        let b = si_deform::lattice::model_tree_lattice(&m, Ptr32::new(node)).addr();
        assert_eq!(a, b);
    }
}

#[test]
fn routing6_wave_callees_reach_the_model_ports() {
    si_rt::skip_without_install!();
    // routing6: SI_modelNodeWave / SI_modelBranchWave given to the model image's outside world
    // are answered by si-model's ports (the same value as calling them), not by a hook
    use si_model::model_image::callee as mc;
    for flags in [0u32, 1, 0x40, 0xffff_ffff] {
        let mut m = Mem::new();
        init(&mut m);
        let node = m.si_malloc(0x430);
        m.set_u32(node + 0x30, flags);
        for (c, direct) in [
            (&mc::SI_MODEL_NODE_WAVE, si_model::model_image::readjust::node_wave(&mut m, Ptr32::new(node))),
            (&mc::SI_MODEL_BRANCH_WAVE, si_model::model_image::readjust::branch_wave(&mut m, Ptr32::new(node))),
        ] {
            assert_eq!(super::motion::routing6_model_ports(&mut m, c, &[node]), Some(direct));
        }
    }
    // a callee that is not routed here stays None (a hook)
    let mut m = Mem::new();
    init(&mut m);
    assert_eq!(super::motion::routing6_model_ports(&mut m, &mc::SI_MODEL_ALLOCATE, &[0]), None);
}

#[test]
fn routea_light_allocate_arm_gives_the_ports_own_memory() {
    si_rt::skip_without_install!();
    // routeA: the world's arm for SI_lightAllocate against a direct call
    // of the ported function on an identical image: same answer, same bytes in the touched records, same faults
    use super::world::{NoWatch, World};
    use si_mem::FnPtr;
    // SI_lightAllocate, each light kind
    for kind in 0..4 {
        let (mut a, mut b) = (Mem::new(), Mem::new());
        init(&mut a);
        init(&mut b);
        let mut w = World::new(NoWatch);
        let ra = w.call(&mut a, FnPtr::new(0x0828_1d34), &[kind]);
        let rb = si_elemio::image::lights::light_allocate(&mut b, &mut super::world::ElemioSide(&mut World::new(NoWatch)), kind as i32);
        assert_eq!(ra, rb);
        assert_ne!(ra, 0);
        assert_eq!(a.bytes(ra, 0x240), b.bytes(rb, 0x240), "light kind {kind}");
        assert_eq!(a.fault_count(), b.fault_count());
        assert_eq!(w.ported.get(&0x0828_1d34), Some(&1));
    }
}

#[test]
fn lic2_every_start_up_address_reaches_its_port() {
    si_rt::skip_without_install!();
    use super::world::{NoWatch, World, LIC2};
    for &a in LIC2 {
        let mut m = Mem::new();
        init(&mut m);
        let mut o = World::new(NoWatch);
        o.call(&mut m, FnPtr::new(a), &[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(o.ported.get(&a), Some(&1), "0x{a:08x} is not routed");
        assert!(!o.hooks.contains_key(&a), "0x{a:08x} was a hook");
    }
}

#[test]
fn lic2_the_worlds_matter_init_leaves_what_the_native_start_up_leaves() {
    si_rt::skip_without_install!();
    // si-matter's SI_matterInit through the World (its callees routed to the ports) against
    // this package's `matter::matter_init`, without the font file (the World's fopen is a
    // hook answering 0; the native read answers None): the same image apart from the stack
    use super::world::{place_start_data, NoWatch, World, MATTER_INIT_POINTERS, MATTER_INIT_TEXTS};
    let fresh = || {
        let mut m = Mem::new();
        init(&mut m);
        m[SI_XMAXSCREEN] = 1279;
        m[SI_YMAXSCREEN] = 1023;
        place_start_data(&mut m, MATTER_INIT_TEXTS, MATTER_INIT_POINTERS);
        m
    };
    let (mut a, mut b) = (fresh(), fresh());
    let (sa, sb) = (a.snapshot(), b.snapshot());
    let mut w = World::new(NoWatch);
    w.call(&mut a, FnPtr::new(0x0829_1bec), &[]);
    matter::matter_init(&mut b, &|_| None);
    let (ca, cb) = (a.changed(&sa), b.changed(&sb));
    assert!(!ca.is_empty());
    assert_eq!(ca, cb);
    assert_eq!(a.fault_count(), b.fault_count());
    for x in [0x0829_1bec, 0x0828_19c8, 0x0828_205c, 0x082a_5920, 0x0829_12e4, 0x0829_1bd4, 0x0829_6200, 0x0857_ca00] {
        assert_eq!(w.ported.get(&x), Some(&1), "0x{x:08x}");
    }
    // the font file: opened by name, through the hooks
    assert_eq!(w.hooks.get(&0x080c_4e18), Some(&1));
    assert_eq!(a.cstr(a.u32(0x0879_cc04)), b"Hello World!");
}

#[test]
fn lic2_start_data_keeps_what_the_image_holds() {
    use super::world::{place_start_data, CUS_INIT_POINTERS, CUS_INIT_TEXTS};
    let mut m = Mem::new();
    m.set_u32(0x0867_935c, 0x1234);
    m.put_cstr(0x086d_f459, b"X");
    place_start_data(&mut m, CUS_INIT_TEXTS, CUS_INIT_POINTERS);
    assert_eq!(m.u32(0x0867_935c), 0x1234);
    assert_eq!(m.cstr(0x086d_f459), b"X");
    assert_eq!(m.u32(0x0867_9390), 0x0867_9370);
    assert_eq!(m.cstr(m.u32(0x0867_9390)), b"/usr/softimage/custom/motion");
    assert_eq!(m.cstr(0x086d_f511), b"SI_CHNLDRIVER");
}
