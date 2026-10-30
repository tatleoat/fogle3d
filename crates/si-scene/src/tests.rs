// SPDX-License-Identifier: MIT
//! Loading and saving on a made-up database held in memory.

use crate::calls::{Channel, Recorder};
use crate::dict::ElemId;
use crate::init;
use crate::load::{self, Outcome, Scene};
use crate::object::Object;
use crate::save::{self, Build};
use si_db::{DbInfo, MemFs};
use si_types::dict::{ELEM_DELETED, ELEM_VALID};

const DSC: &str = "SDSC3.71\nELEMENTS \n    CHAPTER CAMERAS NBELEM 2     \n       t-Camera1.1-0 ; \n       t-cam_int1.1-0 ROOT ; \n    EndOfCHAPTER \n\n    CHAPTER MATERIALS NBELEM 1     \n       t-mat.1-0 ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS NBELEM 3     \n       t-B.1-0 ROOT ; \n       t-a.2-0 ROOT ; \n       t-kid.2-0 ; \n    EndOfCHAPTER \n\n    CHAPTER SETUP_SOFT NBELEM 1     \n       t.1-0 ; \n    EndOfCHAPTER \n\nEndOfELEMENTS \n\nRELATIONS \n    CHAPTER MODELS CHAPTER MODELS \n       2 1 110 ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS CHAPTER MATERIALS \n       1 0 300 ; \n    EndOfCHAPTER \n\n    CHAPTER CAMERAS CHAPTER CAMERAS \n       0 1 1110 ; \n    EndOfCHAPTER \n\nEndOfRELATIONS \n\nENVIRONMENT \n    CHAPTER CAMERAS \n       0 SCHEM 0 -2 0 MPRFLG 0 ; \n       1 SCHEM 0 0 0 MPRFLG 0 ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS \n       0 SCHEM 1 0 0 SRT 1 1 1 0 0 0 0 2.5 0 MPRFLG 0 ; \n       1 SCHEM 2 0 0 USR SRT 1 1 1 0 0 0 0 0 0 MPRFLG 0 ; \n       2 SCHEM 3 0 0 WIRECOL 1 7 DISPLAY 1 2 MPRFLG 0 ; \n    EndOfCHAPTER \n\n    CHAPTER MATERIALS \n       0 SCHEM 5 0 0 MPRFLG 0 ; \n    EndOfCHAPTER \n\n    PLAYCTRL \n       FRAME 1 100 1 1 \n       PAUSE 0 \n       RATE 30 \n    EndOfPLAYCTRL \nEndOfENVIRONMENT \n";

fn model(names: &[&str]) -> Vec<u8> {
    // a root with the other names as a chain of sons
    let mut node: Option<Box<si_hrc::Node>> = None;
    for n in names.iter().rev() {
        let mut x = si_hrc::Node::default();
        x.name = n.as_bytes().to_vec();
        x.son = node.take();
        node = Some(Box::new(x));
    }
    si_hrc::write_model(&node.unwrap())
}

fn database(with: &[&str]) -> MemFs {
    let mut fs = MemFs::new();
    for d in ["SCENES", "MODELS", "MATERIALS", "CAMERAS", "SETUP_SOFT"] {
        fs.add_dir(format!("/db/{d}").as_bytes());
    }
    let mut cam = si_elemio::Writer::new();
    si_elemio::camera::write_file(&mut cam, &si_elemio::camera::Camera::perspective_default());
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("SCENES/t-shot.1-0.dsc", DSC.as_bytes().to_vec()),
        ("MODELS/t-B.1-0.hrc", model(&["B"])),
        ("MODELS/t-a.2-0.hrc", model(&["a", "kid"])),
        ("MATERIALS/t-mat.1-0.mtr", si_hrc::write_material(&si_hrc::Material::default(), 4.0)),
        ("CAMERAS/t-cam_int1.1-0.cam", cam.out.clone()),
        ("SETUP_SOFT/t.1-0.sts", b"SSET4\nEOF ".to_vec()),
    ];
    for (name, data) in files {
        if with.iter().any(|w| name.contains(w)) || with.is_empty() {
            fs.add_file(format!("/db/{name}").as_bytes(), &data);
        }
    }
    fs
}

fn scene() -> Scene {
    let (mut d, mut p) = init::empty_scene();
    init::create_default_dsc(&mut d, &p);
    let (m, c) = (p.cid.model, p.cid.cam);
    p.rd.add_relation(b"Parent", 110, m, m, 1, 1, 10, 1, 0, 0, 0, None, 0, 3);
    p.rd.add_relation(b"Mat", 300, m, p.cid.mat, 3, 0, 20, 0x7fff, 0, 1, 0, None, 1, 3);
    p.rd.add_relation(b"CamInt", 1110, c, c, 1, 1, 10, 1, 0, 0, 0, None, 0, 3);
    Scene::from_parts(d, p)
}

fn info() -> DbInfo {
    let mut i = DbInfo::new();
    i.path = Some(b"/db".to_vec());
    i
}

fn outcome(sc: &Scene, chap: i32, name: &str) -> Outcome {
    sc.report.iter().find(|r| r.chap == chap && r.name == name.as_bytes()).map(|r| r.outcome.clone()).unwrap_or(Outcome::NotRead)
}

#[test]
fn a_scene_loads_into_the_dictionary() {
    si_rt::skip_without_install!();
    let fs = database(&[]);
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    let cid = sc.program.cid;
    // the default scene made room for the loaded one
    let scenes = &sc.dict.ed.lists[cid.dsc as usize];
    assert_eq!(scenes.elems.len(), 1);
    assert_eq!(sc.dict.node_of(cid.dsc, 0).unwrap().full.as_deref(), Some(&b"t-shot.1-0"[..]));
    // elements are numbered in file order; the load's marks are cleared at the end
    let models = &sc.dict.ed.lists[cid.model as usize];
    assert_eq!(models.elems.len(), 3);
    assert!(models.elems.iter().all(|e| e.rec.new_flag == 0 && e.rec.status == ELEM_VALID && !e.rec.data.is_null()));
    assert_eq!(models.elems.iter().map(|e| (e.rec.hier_level, e.rec.root_id)).collect::<Vec<_>>(), [(0, 0), (0, 1), (1, 1)]);
    // the son was claimed from its root's file by name
    assert!(matches!(sc.object_of(ElemId::new(cid.model, 2)), Some(Object::ModelNode { node: 1, .. })));
    assert_eq!(outcome(&sc, cid.model, "t-kid.2-0"), Outcome::Node);
    // both camera elements point at the scene's one camera
    let cams = &sc.dict.ed.lists[cid.cam as usize];
    assert_eq!(cams.elems[0].rec.data, cams.elems[1].rec.data);
    assert_eq!(cams.elems[0].rec.data.0, sc.camera);
    // relations at both ends
    assert_eq!(models.elems[2].forward[1].rels[0].elemid, 1);
    assert_eq!(models.elems[1].backward[1].rels[0].elemid, 2);
    assert_eq!(sc.dict.ed.lists[cid.mat as usize].elems[0].backward[0].rels[0].elemid, 1);
    // display state; the SRT numbers went to the root models, MPRFLG to the nodes
    assert_eq!(models.elems[1].rec.schem_flags & 1, 1);
    assert_eq!((models.elems[2].rec.selection, models.elems[2].rec.wire_type), (1, 2));
    assert!(models.elems.iter().all(|e| e.rec.root_srt.is_null() && !e.rec.aux_data.is_null()));
    // the boxes MPRFLG left cleared are then measured by the material conversion
    // (SI_dbCheckConvertModelsMatTxt: a root whose box is four zeros, SI_schemComputeElemBBox)
    let boxes: Vec<_> = models.elems.iter().map(|e| e.rec.schem_bbox).collect();
    assert_eq!(boxes, [[1.0, 0.0, 3.0, 0.5], [2.0, 0.0, 7.0, 0.5], [3.0, 0.0, 5.0, 0.5]]);
    assert_eq!(sc.root_state[&ElemId::new(cid.model, 0)][7], 2.5);
    assert_eq!((sc.play.start, sc.play.end, sc.play.rate), (1, 100, 30.0));
    // the order of the calls into the rest of the program is on record
    let names = un.names();
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap_or_else(|| panic!("no call of {n}"));
    assert!(at("SI_windowResetAll") < at("SI_dialogInProgressOpen"));
    assert!(at("SI_dialogInProgressClose") < at("SI_modelUpdateDependency"));
    // SI_envelopRelationSet between the two runs on the image (si-deform's port), and so
    // does SI_modelUpdateDep (load::model_update_dep): no longer on record
    // SI_elemdictRestoreActions runs on the image (si-dict::dictin); the made-up scene has
    // no animation root with a record, so none of its three calls is on record
    assert!(!names.contains(&"SI_elemdictRestoreActions"));
    assert!(!names.contains(&"SI_modelUpdateDep"));
    assert!(at("SI_sceneUVDomainInit") < at("SI_sceneConstraintInit"));
    // SI_scnRelationsFromElemDict 33 times: on the image (finding 0018, section 17) or,
    // where a pointer it follows is not a record of the image, recorded
    let recorded = names.iter().filter(|n| **n == "SI_scnRelationsFromElemDict").count();
    assert_eq!(recorded + sc.contents.scnrel_run.values().sum::<usize>(), 33);
    // the topological sort ran, chapter by chapter: the camera's relation to its interest;
    // then the son's parent relation followed by its transformation
    assert_eq!(
        sc.upd_list.nodes.iter().map(|n| (n.chapid as i32, n.elemid, n.relid)).collect::<Vec<_>>(),
        [(cid.cam, 0, 1110), (cid.model, 2, 110), (cid.model, 2, -100)]
    );
    assert_eq!(sc.dict.ed.rec.upd_list.dirty, 0);
    assert!(sc.messages.is_empty());
}

#[test]
fn a_loaded_scene_writes_back_to_the_same_text() {
    si_rt::skip_without_install!();
    let fs = database(&[]);
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    let out = save::write_description(&mut sc, Build::Linux, 3.71, &mut un);
    assert_eq!(String::from_utf8_lossy(&out), DSC);
    // this build's own header
    let out = save::write_description(&mut sc, Build::Linux, 4.0, &mut un);
    assert!(out.starts_with(b"SDSC4\nELEMENTS "));
    // the NT build sorts without regard to case: `t-a` goes before `t-B`, and every number
    // that refers to a model changes with it
    let nt = String::from_utf8(save::write_description(&mut sc, Build::Nt, 3.71, &mut un)).unwrap();
    assert!(nt.contains("       t-a.2-0 ROOT ; \n       t-B.1-0 ROOT ; \n       t-kid.2-0 ; \n"));
    assert!(nt.contains("    CHAPTER MODELS CHAPTER MODELS \n       2 0 110 ; \n"));
    assert!(nt.contains("    CHAPTER MODELS CHAPTER MATERIALS \n       0 0 300 ; \n"));
    // the lines stay in the dictionary's order: the model numbered 1 comes first
    assert!(nt.contains("    CHAPTER MODELS \n       1 SCHEM 1 0 0 SRT"));
}

#[test]
fn a_setup_that_keeps_materials_inside_the_models_restricts_the_description() {
    si_rt::skip_without_install!();
    let mut fs = database(&[]);
    fs.add_file(b"/db/SETUP_SOFT/t.1-0.sts", b"SSET4\nPREFERENCE SAVE_MAT_TXT_NOT_SEPARATELY 1 EOF ");
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    assert_eq!((sc.saving.no_mat_txt, sc.saving.no_old_ver), (1, 0));
    // the material of one model, without shaders, textures, animation or expressions, is
    // left out of the ELEMENTS, RELATIONS and ENVIRONMENT sections
    let out = String::from_utf8(save::write_description(&mut sc, Build::Linux, 3.71, &mut un)).unwrap();
    let expect = DSC
        .replace("    CHAPTER MATERIALS NBELEM 1     \n       t-mat.1-0 ; \n    EndOfCHAPTER \n\n", "")
        .replace("    CHAPTER MODELS CHAPTER MATERIALS \n       1 0 300 ; \n    EndOfCHAPTER \n\n", "")
        .replace("    CHAPTER MATERIALS \n       0 SCHEM 5 0 0 MPRFLG 0 ; \n    EndOfCHAPTER \n\n", "");
    assert_ne!(expect, DSC);
    assert_eq!(out, expect);
}

#[test]
fn missing_files() {
    si_rt::skip_without_install!();
    // no material, no camera, and the second model's file is not there
    let fs = database(&["dsc", "t-B", "sts"]);
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    let cid = sc.program.cid;
    let status: Vec<String> = sc.messages.iter().filter(|m| matches!(m.channel, Channel::Status(_))).map(|m| String::from_utf8_lossy(&m.text).into_owned()).collect();
    assert_eq!(
        status,
        [
            "Couldn't read file /db/MODELS/t-a.hrc",
            "Missing MODELS [t-a.2-0] has been replaced by a default element",
            "Couldn't read file /db/CAMERAS/t-cam_int1.cam",
            "Missing CAMERAS [t-cam_int1.1-0] could not be replaced: removed from scene",
            "Couldn't read file /db/MATERIALS/t-mat.mtr",
            "Missing MATERIALS [t-mat.1-0] has been replaced by a default element",
        ]
    );
    // the model and its son each got a cube; the material a default; the camera's root is gone
    let models = &sc.dict.ed.lists[cid.model as usize];
    assert!(models.elems.iter().all(|e| e.rec.status == ELEM_VALID && !e.rec.data.is_null()));
    assert!(matches!(sc.object_of(ElemId::new(cid.model, 2)), Some(Object::Foreign { .. })));
    assert!(matches!(sc.object_of(ElemId::new(cid.mat, 0)), Some(Object::Foreign { .. })));
    let cams = &sc.dict.ed.lists[cid.cam as usize];
    assert_eq!((cams.elems[0].rec.status, cams.elems[1].rec.status), (ELEM_VALID, ELEM_DELETED));
    // the camera that was not a root got the scene's camera all the same, and lost its relation
    assert_eq!(cams.elems[0].rec.data.0, sc.camera);
    assert!(cams.elems[0].forward[1].rels.is_empty());
    assert!(matches!(outcome(&sc, cid.cam, "t-cam_int1.1-0"), Outcome::Missing { .. }));
}

#[test]
fn a_node_that_is_not_in_its_roots_file() {
    si_rt::skip_without_install!();
    let mut fs = database(&[]);
    fs.add_file(b"/db/MODELS/t-a.2-0.hrc", &model(&["a", "other"]));
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    let cid = sc.program.cid;
    assert_eq!(sc.messages.len(), 1);
    assert_eq!(String::from_utf8_lossy(&sc.messages[0].text), "A model node named [kid] was expected under root [a]: it couldn't be found");
    // an element of this load without an object does not survive it
    assert_eq!(sc.dict.ed.lists[cid.model as usize].elems[2].rec.status, ELEM_DELETED);
    assert!(sc.dict.ed.lists[cid.model as usize].elems[1].backward[1].rels.is_empty());
}

#[test]
fn an_unversioned_file_and_a_sub_directory_are_found() {
    si_rt::skip_without_install!();
    let mut fs = database(&["dsc", "t-B", "mat", "cam", "sts"]);
    // the second model exists only without a version, in a sub-directory of the chapter
    fs.add_dir(b"/db/MODELS/old");
    fs.add_file(b"/db/MODELS/old/t-a.hrc", &model(&["a", "kid"]));
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    let cid = sc.program.cid;
    match outcome(&sc, cid.model, "t-a.2-0") {
        Outcome::Read { path, fallback, sub_dir, complete } => {
            assert_eq!(path, b"/db/MODELS/old/t-a.hrc");
            assert!(fallback && sub_dir && complete);
        }
        other => panic!("{other:?}"),
    }
    // the name record remembers where the file was found and keeps the name it was given
    let n = sc.dict.node_of(cid.model, 1).unwrap();
    assert_eq!((n.sub_path.as_deref(), n.full.as_deref()), (Some(&b"old/"[..]), Some(&b"t-a.2-0"[..])));
    assert!(sc.messages.is_empty());
}

#[test]
fn a_scene_without_its_description_does_not_load() {
    si_rt::skip_without_install!();
    let fs = database(&["hrc"]);
    let mut sc = scene();
    let mut un = Recorder::default();
    assert!(!load::load_scene(&mut sc, &fs, &mut info(), b"t-shot.1-0", &mut un));
    // the scene's element stays, unread; nothing else was done
    let cid = sc.program.cid;
    assert_eq!(sc.dict.ed.lists[cid.dsc as usize].elems[0].rec.new_flag, 1);
    assert_eq!(sc.dict.ed.lists[cid.model as usize].elems.len(), 0);
    assert!(!un.names().contains(&"SI_sceneRefresh"));
    assert_eq!(sc.program.add_in_dictionary, 1);
}

fn save_host<'a>(fs: &'a mut MemFs, un: &'a mut Recorder, ui: &'a mut crate::calls::Answers) -> crate::dictout::Host<'a> {
    crate::dictout::Host { fs, un, ui }
}

#[test]
fn a_loaded_scene_is_saved_under_new_versions() {
    si_rt::skip_without_install!();
    let mut fs = database(&[]);
    let mut sc = scene();
    let mut info = info();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info, b"t-shot.1-0", &mut un));
    let cid = sc.program.cid;
    // what the Save dialogue works out: the two root models were given their SRT by the
    // load and are dirty; the camera is always dirty; the material is not
    crate::savescene::get_dirty_flags(&mut sc, &mut un);
    let dirty = |sc: &Scene, chap: i32, i: usize| sc.dict.ed.lists[chap as usize].elems[i].rec.dirty;
    assert_eq!((dirty(&sc, cid.model, 0), dirty(&sc, cid.model, 1), dirty(&sc, cid.mat, 0), dirty(&sc, cid.cam, 1)), (1, 1, 0, 1));
    // the channel and flock setups are the default ones (answer 1); the writers of other
    // packages succeed
    let mut ui = crate::calls::Answers { default: 1, ..Default::default() };
    let written = {
        let mut h = save_host(&mut fs, &mut un, &mut ui);
        crate::savescene::save_scene(&mut sc, &mut h, &mut info, false, false, None, false)
    };
    assert!(written);
    let files: Vec<String> = sc.saving.written.iter().map(|w| String::from_utf8_lossy(&w.path).into_owned()).collect();
    assert_eq!(
        files,
        [
            "/db/MODELS/t-B.2-0.hrc",
            "/db/MODELS/t-a.3-0.hrc",
            "/db/CAMERAS/t-cam_int1.2-0.cam",
            // the setup takes the scene's name; no file of that name was there: version 1
            "/db/SETUP_SOFT/t-shot.1-0.sts",
            // the description last, one version up
            "/db/SCENES/t-shot.2-0.dsc"
        ]
    );
    // the old files are still there, the material was not written again
    use si_db::FileSystem;
    assert!(fs.read(b"/db/MODELS/t-B.1-0.hrc").is_some() && fs.read(b"/db/MATERIALS/t-mat.2-0.mtr").is_none());
    // the root's transformation in the new model file is the description's SRT
    let hrc = fs.read(b"/db/MODELS/t-B.2-0.hrc").unwrap();
    match si_hrc::read_file(&hrc, si_hrc::Kind::Model).map(|f| f.content) {
        Some(si_hrc::Content::Model(root)) => assert_eq!(root.translation, [0.0, 2.5, 0.0]),
        _ => panic!("the saved model does not read back"),
    }
    // the new description names the new versions, and SRT for both roots
    let dsc = String::from_utf8_lossy(&fs.read(b"/db/SCENES/t-shot.2-0.dsc").unwrap()).into_owned();
    assert!(dsc.starts_with("SDSC4\n") && dsc.contains("t-B.2-0 ROOT ;") && dsc.contains("t-a.3-0 ROOT ;") && dsc.contains("t-shot.1-0 ;"));
    assert_eq!(dsc.matches(" SRT ").count(), 2);
    // after the save everything is clean and knows its database
    assert!(sc.dict.ed.lists.iter().flat_map(|l| l.elems.iter()).all(|e| e.rec.dirty == 0 && e.rec.new_flag == 0));
    assert_eq!(sc.dict.node_of(cid.model, 0).unwrap().w00, 1);
    // nothing was handed to a shell
    assert!(fs.commands.is_empty());
    // a second save right away writes what is always dirty, and the models stay
    crate::savescene::get_dirty_flags(&mut sc, &mut un);
    sc.saving.written.clear();
    {
        let mut h = save_host(&mut fs, &mut un, &mut ui);
        crate::savescene::save_scene(&mut sc, &mut h, &mut info, false, false, None, false);
    }
    let files: Vec<String> = sc.saving.written.iter().map(|w| String::from_utf8_lossy(&w.path).into_owned()).collect();
    assert_eq!(files, ["/db/CAMERAS/t-cam_int1.3-0.cam", "/db/SETUP_SOFT/t-shot.2-0.sts", "/db/SCENES/t-shot.3-0.dsc"]);
}

#[test]
fn a_save_that_cannot_write_an_element_stops_before_the_description() {
    si_rt::skip_without_install!();
    let mut fs = database(&[]);
    let mut sc = scene();
    let mut info = info();
    let mut un = Recorder::default();
    assert!(load::load_scene(&mut sc, &fs, &mut info, b"t-shot.1-0", &mut un));
    crate::savescene::get_dirty_flags(&mut sc, &mut un);
    // the camera's writer is taken away: the element's object is not a camera of this crate
    let cam = sc.dict.ed.lists[sc.program.cid.cam as usize].elems[1].rec.data.0;
    *sc.objects.get_mut(cam).unwrap() = Object::Foreign { made_by: "test", path: Vec::new() };
    // every question is answered 0: the foreign writer fails
    let mut ui = crate::calls::Answers::default();
    let written = {
        let mut h = save_host(&mut fs, &mut un, &mut ui);
        crate::savescene::save_scene(&mut sc, &mut h, &mut info, false, false, None, false)
    };
    assert!(!written);
    // the models written before the failure stay; there is no new description
    use si_db::FileSystem;
    assert!(fs.read(b"/db/MODELS/t-B.2-0.hrc").is_some() && fs.read(b"/db/SCENES/t-shot.2-0.dsc").is_none());
    // the dialogue: chapter and message of the error code, the file, "Save aborted."
    let d = ui.asked.iter().find(|c| c.name == "SI_dialogRequestLong").expect("the failure dialogue");
    assert_eq!(d.args[1], crate::calls::Arg::Str(b"Cannot write file:".to_vec()));
    assert_eq!(d.args[2], crate::calls::Arg::Str(b"/db/CAMERAS/t-cam_int1.2-0.cam.".to_vec()));
    // the marks are not cleared: the models are still dirty for the next try
    assert_ne!(sc.dict.ed.lists[sc.program.cid.model as usize].elems[0].rec.dirty, 0);
}

#[test]
fn duplicate_names_are_renamed() {
    si_rt::skip_without_install!();
    let (mut d, p) = init::empty_scene();
    let m = p.cid.model;
    for n in ["s-a.1-0", "s-a.2-0", "s-a_1.1-0", "s-b.1-0"] {
        init::add_named(&mut d, &p, m, n.as_bytes());
    }
    let mut sc = Scene::from_parts(d, p);
    let mut un = Recorder::default();
    assert_eq!(crate::savescene::check_duplicate_names(&mut sc, &mut un), 1);
    // the second `s-a` becomes `a_2`: one duplicate, and `a_1` is taken already
    let names: Vec<Vec<u8>> = (0..4).map(|i| sc.dict.node_of(m, i).unwrap().full.clone().unwrap()).collect();
    assert_eq!(names, [b"s-a.1-0".to_vec(), b"s-a_2.2-0".to_vec(), b"s-a_1.1-0".to_vec(), b"s-b.1-0".to_vec()]);
    assert_eq!(sc.dict.ed.lists[m as usize].elems[1].rec.new_flag, 2);
}

#[test]
fn the_animation_relation_of_a_chapter() {
    let (_, p) = init::empty_scene();
    let c = p.cid;
    let mut m = si_mem::Mem::new();
    m.write(si_dict::globals::SI_CID, c);
    let rel = |chap| crate::dbvsfcv::rel_get_anim_f_rel_from_chapter(&m, chap);
    assert_eq!((rel(c.model), rel(c.wav), rel(c.cls), rel(c.mat), rel(c.dsc), rel(c.t2d), rel(c.t3d), rel(c.cam), rel(c.lig), rel(c.lat), rel(c.shp)), (15000, 15001, 15002, 15003, 15004, 15005, 15006, 15007, 15008, 15009, 15010));
    assert_eq!(rel(c.exp), -1);
}

#[test]
fn the_loaders_dispatch_runs_si_motions_copy_of_a_textures_animation() {
    si_rt::skip_without_install!();
    // SI_texture2DLoadFcurveAll by address (finding 0018, section 23.12): two textures
    // without elements, so nothing is copied; its calls out reach the loader's dispatch by
    // the original's addresses, the look-ups to their ports, the curve table to a hook
    let mut sc = scene();
    let mut m = std::mem::take(&mut sc.mem);
    let mut un = crate::calls::Recorder::default();
    let (a, b) = (m.si_calloc(1, 0x1d8), m.si_calloc(1, 0x1d8));
    let mut p = crate::image::Prog { sc: &mut sc, un: &mut un, fs: None, db: None };
    p.by_addr(&mut m, crate::edtools::SI_TEXTURE2D_LOAD_FCURVE_ALL, "SI_texture2DLoadFcurveAll", &[a, b]);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    let (ported, hooks) = (&sc.contents.ported, &sc.contents.hooks);
    assert_eq!(ported.get("SI_texture2DLoadFcurveAll"), Some(&1));
    assert_eq!(ported.get("SI_edGetElemFromTexture2D"), Some(&2));
    assert_eq!(hooks.get("SI_setFcvTableEntriesFromChap"), Some(&1));
    assert_eq!(hooks.get("SI_elemdictCopyElemAnim"), None);
}
