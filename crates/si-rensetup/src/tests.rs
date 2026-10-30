// SPDX-License-Identifier: MIT
//! Unit tests on made-up data: a scene of a few nodes built by hand, taken through the
//! render dictionary and back.

use crate::edlocal::{get_new_name_id, NewName};
use crate::outside::{Call, Recorder};
use crate::rmain::{create_render_dictionary, delete_render_dictionary, update_selective_lights, RenState};
use crate::util::{build_pic_file_name, check_all_models, clamp_maxcoc, empty_string};
use crate::world::{algo, cam, lig, mat, rel, Env, Image, Obj};
use si_db::ListNode;
use si_scene::dict::{Dict, ElemId};
use si_scene::elemrel;
use si_types::model::Model;
use si_types::Ptr32;

struct Made {
    scene: Dict,
    env: Env,
}

/// A scene dictionary with the rules the set-up uses, as `Relations.rsrc` has them.
fn made() -> Made {
    let (mut scene, mut p) = si_scene::init::empty_scene();
    si_scene::init::create_default_dsc(&mut scene, &p);
    for (id, sc, dc, sg, dg, fan, render, inc) in [
        (110, 8, 8, 1, 1, 1, 1, 0),
        (300, 8, 7, 3, 0, 32767, 1, 0),
        (400, 8, 17, 3, 0, 32767, 1, 0),
        (401, 7, 17, 3, 0, 32767, 1, 0),
        (500, 8, 18, 3, 0, 1, 1, 0),
        (501, 7, 18, 3, 0, 1, 1, 0),
        (550, -10, -10, 3, 0, 32767, 1, 4),
        (20002, 8, 8, 4, 4, 32767, 1, 0),
        (112, 8, 8, 1, 1, 32767, 0, 3),
    ] {
        p.rd.add_relation(b"", id, sc, dc, sg, dg, 1, fan, 0, 0, 0, None, render, inc);
    }
    Made { scene, env: Env { p, ..Env::default() } }
}

impl Made {
    fn model(&mut self, name: &[u8], kind: i32, father: Option<i32>) -> i32 {
        let mut m = Model::allocate(Some(name), &mut 0).0;
        m.kind = kind;
        m.name = Ptr32::new(self.env.heap.strdup(name));
        let h = self.env.heap.alloc(Obj::Model(Box::new(m)));
        let mut node = ListNode::allocate(0);
        node.name = Some(name.to_vec());
        node.full = Some(si_db::format_full_name(&mut node, false));
        let (i, _) = si_scene::init::add_elem(&mut self.scene, &self.env.p, self.env.p.cid.model, Some(node), Ptr32::new(h), Ptr32::NULL, 0, 0);
        self.env.heap.model_mut(h).unwrap().elem_id = i;
        if let Some(f) = father {
            elemrel::add_relation(&mut self.scene, &mut self.env.p, i, f, rel::PARENT, 1, 0);
            let fh = self.scene.ed.lists[self.env.p.cid.model as usize].elems[f as usize].rec.data;
            self.env.heap.model_mut(h).unwrap().father = Ptr32::new(fh.0);
        }
        i
    }

    fn material(&mut self, name: &[u8]) -> i32 {
        let h = self.env.heap.alloc(Obj::Material(Image::new(mat::SIZE)));
        let mut node = ListNode::allocate(0);
        node.name = Some(name.to_vec());
        node.full = Some(si_db::format_full_name(&mut node, false));
        si_scene::init::add_elem(&mut self.scene, &self.env.p, self.env.p.cid.mat, Some(node), Ptr32::new(h), Ptr32::NULL, 0, 0).0
    }

    fn handle(&self, model: i32) -> u32 {
        self.scene.ed.lists[self.env.p.cid.model as usize].elems[model as usize].rec.data.0
    }

    fn finish(&mut self) {
        let cm = self.env.p.cid.model;
        self.scene.ed.update_hier_info(cm, 1, rel::PARENT);
    }
}

fn materials_of(d: &Dict, m: &Made, model: i32) -> Vec<i32> {
    d.ed.lists[m.env.p.cid.model as usize].elems[model as usize].forward[3].rels.iter().filter(|r| r.relid == rel::MAT).map(|r| r.elemid).collect()
}

#[test]
fn materials_come_down_the_hierarchy() {
    let mut m = made();
    let bare = m.model(b"bare", 4, None);
    let bare_son = m.model(b"son", 4, Some(bare));
    let painted = m.model(b"painted", 4, None);
    let painted_son = m.model(b"psn", 4, Some(painted));
    let own = m.model(b"own", 4, Some(painted));
    let red = m.material(b"red");
    let blue = m.material(b"blue");
    let (cm, cmat) = (m.env.p.cid.model, m.env.p.cid.mat);
    elemrel::add_uncomplete_relation(&mut m.scene, &mut m.env.p, cm, cmat, painted, red, rel::MAT, 0, 0, Ptr32::NULL, 1);
    elemrel::add_uncomplete_relation(&mut m.scene, &mut m.env.p, cm, cmat, own, blue, rel::MAT, 0, 0, Ptr32::NULL, 1);
    m.finish();
    let before = m.scene.clone();
    let mut ren = Dict::new();
    let mut rec = Recorder::default();
    let mut st = RenState::default();
    let default = create_render_dictionary(&mut m.scene, &mut ren, &mut m.env, &mut rec, &mut st, true, true).unwrap();
    // one more material: the default, named DEFAULT, whose record is called DEFAULT1
    assert_eq!(default, ElemId::new(cmat, 2));
    assert_eq!(ren.node_of(cmat, 2).unwrap().name.as_deref(), Some(&b"DEFAULT"[..]));
    let record = m.env.heap.image(ren.slot(default).unwrap().rec.data.0).unwrap();
    assert_eq!(&record.bytes[..9], b"DEFAULT1\0");
    assert_eq!(record.i32(mat::ELEM_ID), 2);
    assert_eq!(materials_of(&ren, &m, bare), vec![2]);
    assert_eq!(materials_of(&ren, &m, bare_son), vec![2]);
    assert_eq!(materials_of(&ren, &m, painted), vec![red]);
    assert_eq!(materials_of(&ren, &m, painted_son), vec![red]);
    assert_eq!(materials_of(&ren, &m, own), vec![blue]);
    // where the materials came from
    let work = |i: i32| ren.ed.lists[cm as usize].elems[i as usize].rec.work_id;
    assert_eq!((work(bare), work(bare_son), work(painted_son), work(own)), (-1, bare, painted, -1));
    assert_eq!(st.counts.default_material_given, 1);
    // every copy has a prefix: the scene's name
    assert_eq!(ren.node_of(cm, bare).unwrap().prefix.as_deref(), Some(&b"no_name"[..]));
    // the objects are the scene's own
    assert_eq!(ren.slot(ElemId::new(cm, own)).unwrap().rec.data.0, m.handle(own));
    assert_eq!(check_all_models(&mut ren, &m.env, &mut rec, 1), 0);
    rec.triangles = vec![(m.handle(own), 12)];
    assert_eq!(check_all_models(&mut ren, &m.env, &mut rec, 1), 1);
    assert_eq!(check_all_models(&mut ren, &m.env, &mut rec, 0), 0); // nothing is selected
    delete_render_dictionary(&m.scene, &mut ren, &mut m.env, Some(default), true);
    assert!(ren.ed.lists.is_empty());
    assert!(m.env.heap.image(m.env.heap.objs.handles().last().unwrap()).is_none_or(|i| &i.bytes[..7] != b"DEFAULT"));
    // the scene's relations are as they were
    for (a, b) in m.scene.ed.lists.iter().zip(&before.ed.lists) {
        for (x, y) in a.elems.iter().zip(&b.elems) {
            assert_eq!(x.forward.iter().map(|l| l.rels.clone()).collect::<Vec<_>>(), y.forward.iter().map(|l| l.rels.clone()).collect::<Vec<_>>());
        }
    }
}

#[test]
fn an_instance_becomes_a_hierarchy() {
    let mut m = made();
    let master = m.model(b"cube", 4, None);
    let _part = m.model(b"cube2", 4, Some(master));
    let inst = m.model(b"inst", 0, None);
    let (hm, hi) = (m.handle(master), m.handle(inst));
    m.env.heap.model_mut(hi).unwrap().algo_type = algo::INSTANCE;
    let cm = m.env.p.cid.model;
    elemrel::add_uncomplete_relation(&mut m.scene, &mut m.env.p, cm, cm, inst, master, rel::INST, 0, 0, Ptr32::NULL, 1);
    m.finish();
    // the copy the hierarchy copier will answer with: two nodes that carry the numbers of
    // the master's
    let mut copy = Vec::new();
    for (name, id) in [(&b"cube"[..], 0), (&b"cube2"[..], 1)] {
        let mut c = Model::allocate(Some(name), &mut 0).0;
        c.name = Ptr32::new(m.env.heap.strdup(name));
        c.elem_id = id;
        c.kind = 4;
        copy.push(m.env.heap.alloc(Obj::Model(Box::new(c))));
    }
    m.env.heap.model_mut(copy[0]).unwrap().son = Ptr32::new(copy[1]);
    m.env.heap.model_mut(copy[1]).unwrap().father = Ptr32::new(copy[0]);
    let mut rec = Recorder { expansions: vec![(hm, copy[0])], ..Recorder::default() };
    let mut ren = Dict::new();
    let mut st = RenState::default();
    let default = create_render_dictionary(&mut m.scene, &mut ren, &mut m.env, &mut rec, &mut st, true, true);
    assert_eq!((st.counts.instances_expanded, st.counts.instance_elements_added), (1, 1));
    let models = &ren.ed.lists[cm as usize].elems;
    assert_eq!(models.len(), 4);
    // the instance's element stands for the copy's root, under a new name; the scene's still
    // points at the instance
    assert_eq!(models[inst as usize].rec.data.0, copy[0]);
    assert_eq!(m.handle(inst), hi);
    assert_eq!(ren.node_of(cm, inst).unwrap().name.as_deref(), Some(&b"cube3"[..]));
    assert_eq!(m.env.heap.model(copy[0]).unwrap().elem_id, inst);
    // the node under it got an element of its own, related to it, and kept its number
    assert_eq!(models[3].rec.data.0, copy[1]);
    assert_eq!(ren.node_of(cm, 3).unwrap().name.as_deref(), Some(&b"cube4"[..]));
    assert_eq!(m.env.heap.model(copy[1]).unwrap().elem_id, 1);
    assert_eq!((models[3].rec.hier_level, models[3].rec.root_id), (1, inst));
    assert!(rec.calls.contains(&Call::ExpandUpdateTransfo { master: hm, instance: hi, copy: copy[0] }));
    assert!(crate::instance::instance_elem(inst, &m.scene, &ren, &m.env).is_some());
    assert!(crate::instance::instance_elem(3, &m.scene, &ren, &m.env).is_none());
    delete_render_dictionary(&m.scene, &mut ren, &mut m.env, default, true);
    // the copies' names are freed, the copies are not
    assert!(m.env.heap.model(copy[0]).unwrap().name.is_null() && m.env.heap.model(copy[1]).unwrap().name.is_null());
    assert!(m.env.heap.model_name(hm).is_some());
}

#[test]
fn new_names_count_on() {
    let mut m = made();
    m.model(b"cube", 4, None);
    m.model(b"cube7", 4, None);
    m.model(b"cubes", 4, None);
    let mut st = NewName::default();
    let cm = m.env.p.cid.model;
    assert_eq!(get_new_name_id(&m.scene, &mut st, cm, Some(b"cube12"), false), (b"cube8".to_vec(), 8));
    assert_eq!(get_new_name_id(&m.scene, &mut st, cm, Some(b"ball"), false), (b"ball1".to_vec(), 1));
    assert_eq!(get_new_name_id(&m.scene, &mut st, cm, None, false).0, b"obj1".to_vec());
    st.legacy_loader = true;
    assert_eq!(get_new_name_id(&m.scene, &mut st, cm, Some(b"cube"), false).0, b"cube_2".to_vec());
    assert_eq!(get_new_name_id(&m.scene, &mut st, cm, Some(b"cube"), false).0, b"cube_3".to_vec());
}

#[test]
fn selective_lights_get_a_table() {
    let mut m = made();
    m.model(b"a", 4, None);
    m.model(b"b", 4, None);
    let mut im = Image::new(lig::SIZE);
    im.set_i32(lig::SELECTIVE, 1);
    let light = m.env.heap.alloc(Obj::Light(im));
    let cl = m.env.p.cid.lig;
    si_scene::init::add_elem(&mut m.scene, &m.env.p, cl, None, Ptr32::new(light), Ptr32::NULL, 0, 0);
    update_selective_lights(&m.scene, &mut m.env);
    let im = m.env.heap.image(light).unwrap();
    assert_eq!(im.i32(lig::TABLE_LEN), 2);
    assert_eq!(m.env.heap.words(im.u32(lig::TABLE)).unwrap(), &vec![0, 0, 0]);
}

#[test]
fn names_and_limits() {
    let mut env = Env::default();
    env.g.picture_path = b"/db/RENDER_PICTURES".to_vec();
    assert_eq!(build_pic_file_name(&env, b"shot", 12, false, 0, true), b"/db/RENDER_PICTURES/shot.12.pic");
    assert_eq!(build_pic_file_name(&env, b"/tmp/shot", 3, true, 1, false), b"/tmp/shot.3.1");
    assert!(empty_string(None) && empty_string(Some(b" \t\n")) && !empty_string(Some(b" x")));
    let mut im = Image::new(cam::SIZE);
    im.set_i32(cam::DOF_MODE, 2);
    im.set_f32(cam::MAX_COC, 500.0);
    env.g.scene_camera = env.heap.alloc(Obj::Camera(im));
    env.g.picture_width = 640;
    let mut rec = Recorder::default();
    assert_eq!(clamp_maxcoc(&mut env, &mut rec, true), Some(500.0));
    assert_eq!(env.heap.image(env.g.scene_camera).unwrap().f32(cam::MAX_COC), 128.0);
    assert_eq!(rec.calls, vec![Call::Status(b"Max COC exceeds limit. Clamping at 128.000000".to_vec(), 2)]);
}
