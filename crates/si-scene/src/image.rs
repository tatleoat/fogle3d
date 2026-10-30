// SPDX-License-Identifier: MIT
//! The contents of a scene's elements in the memory image (finding 0018, section 16).
//!
//! The file readers of `si-hrc` (materials, textures, models) and `si-elemio` (lights, the
//! camera) exist on the image; this module is what the loader needs to call them:
//!
//! - [`Prog`]: the outside world of those readers and of `si-view`'s list during a load.
//!   A call by address goes to the port that exists for it (`si-model`, `si-view`,
//!   `si-dict`, `si-hrc`, `si-render`'s `SI_updTxt3dTurb` and `SI_txt2dSetWrap`,
//!   [`crate::edtools`], the material conversion's routines ([`crate::matconv`]),
//!   [`crate::dbnode`]; since finding 0029's third pass `si-spline`'s
//!   `SI_splineUpdateMatrix` for every spline the readers make, and `si-deform`'s
//!   `SI_envelopRebuild` at the end of `SI_fdmodelBinaryRead`, whose calls out go first to
//!   the ports [`crate::envside`] names, [`EnvSide`]); `si-motion`'s copies of an
//!   animation, whose calls out go through [`MotionSide`]; three routines that exist only on
//!   values are answered by adapters ([`model_update_matrix`], [`matrix_xyz_rotation`],
//!   [`mesh_rebuild`]); everything else is a hook that answers 0 and is counted
//!   ([`Contents::hooks`]).
//! - [`Contents`]: which record of the image stands for which value of
//!   [`Scene::objects`]. The value readers still run beside the image readers, because
//!   the saver and the first pass of `si-rensetup` work on their values; an element's
//!   object **in the image** is the record the image reader made.
//! - Six routines of the original around the readers, each cited where it stands:
//!   `SI_omodelBinaryRead`, `si_filenameCleanup`, `SI_modelSetState` and the three thin
//!   `SI_elemdictRead…` of materials and textures (with `SI_elemdictReadModel`).

use crate::calls::{Channel, Message, Unported};
use crate::edtools::{self, ByAddr};
use crate::load::Scene;
use crate::world::{current_dict, obj_of, obj_ptr, Out};
use si_db::FileSystem;
use si_hrc::image::globals::{SI_PIPE, SI_PIPED_FILES};
use si_mem::{Mem, Opaque, Ptr32};
use si_types::model::Model;
use std::collections::BTreeMap;

si_mem::globals! {
    /// The directory model files are named relative to (`SI_omodelBinaryRead` strips it
    /// from a root's file name). Start-up sets it; nothing here does: null.
    SI_OBJECT_PATH: u32 = 0x086793b8, "SI_objectPath", init 0;
}

/// `"%s.hrc"` and `"rb"`, the two literals of `SI_omodelBinaryRead`.
pub const S_HRC_FORMAT: u32 = 0x086ce6b4;
pub const S_RB_HRC: u32 = 0x086ce6bb;

/// What stands for what, and what the readers did.
#[derive(Default)]
pub struct Contents {
    /// For the checks: the PICTURES elements whose path (+0x08 of the name record)
    /// `SI_texture2DVerifyPicturePath` replaced by where it found the picture, with the
    /// path before (`crate::t2dpath`).
    pub pictures_replaced: Vec<(i32, Option<Vec<u8>>)>,
    /// The record of the image that stands for a value of [`Scene::objects`], by handle …
    pub image_of: BTreeMap<u32, u32>,
    /// … and the value's handle by the record's address.
    pub handle_at: BTreeMap<u32, u32>,
    /// The node of the image's graph list that stands for a node of the stand-in graph
    /// ([`Scene::graf`]), by the stand-in's handle, and back.
    pub graf_image: BTreeMap<u32, u32>,
    pub graf_handle: BTreeMap<u32, u32>,
    /// The streams of the reader that is running.
    pub hrc: si_hrc::image::Files,
    pub eio: si_elemio::image::host::Files,
    /// Records the image readers made, by chapter.
    pub records: BTreeMap<&'static str, usize>,
    /// Files on which the image reader and the value reader did not agree whether the
    /// file is one: what, and the path.
    pub disagreed: Vec<(&'static str, Vec<u8>)>,
    /// Calls that went to a port or an adapter, by the callee's name.
    pub ported: BTreeMap<&'static str, usize>,
    /// Calls nobody has on the image (answered 0), by the callee's name.
    pub hooks: BTreeMap<&'static str, usize>,
    /// Model files whose read reached a reader that is not on the image (the stream is
    /// then out of step and the hierarchy is not used), by the first such reader.
    pub models_unread: BTreeMap<&'static str, usize>,
    /// Set by [`Prog`] while a model file is read: the first reader that was missing.
    pub missing_reader: Option<&'static str>,
    /// Model files whose hierarchy in the image did not pair with the value reader's.
    pub models_unpaired: usize,
    /// Meshes rebuilt by the adapter, and polygons the rebuild could not file.
    pub meshes_rebuilt: usize,
    pub polygons_not_filed: usize,
    /// Status lines the image readers asked for (`SI_statusDisplayMessage`).
    pub status: Vec<(i32, Vec<u8>)>,
    /// Elements whose object is not a record of the image, by what made it.
    pub not_in_image: BTreeMap<&'static str, usize>,
    /// Readers that would not have come back (`si-elemio`'s `NoReturn`), with the path.
    pub no_return: Vec<(String, Vec<u8>)>,
    /// What the graph list's builder left, against the stand-in graph.
    pub graph: GraphPairs,
    /// The stand-in node of each element that has one, by chapter and index: the nodes
    /// the stand-in makes and the image's list does not have (a wave's: its reader, which
    /// would add it, is not on the image) are still the saver's.
    pub standin: BTreeMap<(i32, i32), u32>,
    /// The light records in the order their files were read (each entered the graph list
    /// at its head when its file was read).
    pub lights_read: Vec<u32>,
    /// Calls of `SI_scnRelationsFromElemDict` run on the image, by relation id, and those
    /// left recorded, by relation id and the reason ([`crate::db2scn::not_on_image`]).
    pub scnrel_run: BTreeMap<i32, usize>,
    pub scnrel_refused: BTreeMap<(i32, &'static str), usize>,
}

impl Scene {
    /// The value an element's object stands for: its handle in [`Scene::objects`], by the
    /// address in the handle range or, for a record of the image, by the table; 0 for
    /// null and for anything else.
    pub fn obj(&self, p: Ptr32<Opaque>) -> u32 {
        match obj_of(p) {
            0 => self.contents.handle_at.get(&p.addr()).copied().unwrap_or(0),
            h => h,
        }
    }

    /// What an element holds for a value of [`Scene::objects`]: the record of the image
    /// that stands for it, or the address in the handle range when there is none.
    pub fn obj_addr(&self, handle: u32) -> Ptr32<Opaque> {
        match self.contents.image_of.get(&handle) {
            Some(&a) => Ptr32::new(a),
            None => obj_ptr(handle),
        }
    }

    /// The node of the stand-in graph ([`Scene::graf`]) an element's second pointer
    /// stands for: by the address in the handle range or, for a node of the image's
    /// list, by the table; 0 for null and for anything else.
    pub fn graf_handle(&self, p: Ptr32<Opaque>) -> u32 {
        match crate::world::graf_of(p) {
            0 => self.contents.graf_handle.get(&p.addr()).copied().unwrap_or(0),
            h => h,
        }
    }

    /// What an element holds for a node of the stand-in graph: the node of the image's
    /// list paired with it, or the address in the handle range when there is none.
    pub fn graf_addr(&self, handle: u32) -> Ptr32<Opaque> {
        match self.contents.graf_image.get(&handle) {
            Some(&a) => Ptr32::new(a),
            None => crate::world::graf_ptr(handle),
        }
    }

    /// Notes that `record` of the image stands for the value `handle`.
    pub fn tie(&mut self, handle: u32, record: u32) {
        if handle != 0 && record != 0 {
            self.contents.image_of.insert(handle, record);
            self.contents.handle_at.insert(record, handle);
        }
    }
}

/// The outside world of the element readers and of the graph list while a scene loads.
pub struct Prog<'a> {
    pub sc: &'a mut Scene,
    pub un: &'a mut dyn Unported,
    /// Where `fopen` finds files; `None` outside a read.
    pub fs: Option<&'a dyn FileSystem>,
    /// The scene's database, for the picture search of the 2D textures the load's material
    /// conversion makes elements (`SI_texture2DVerifyPicturePath`, through the adapter
    /// [`crate::t2dpath::verify_in_load`]); `None` elsewhere, where that call is a hook.
    pub db: Option<&'a mut si_db::DbInfo>,
}

/// The readers of `binary.c` and of the chunks' own files: a call of one of these through
/// the trait means the reader is not on the image and the stream is out of step.
const READERS: &[u32] = &[
    0x083f_0690, 0x083f_043c, 0x083f_00dc, 0x083f_257c, 0x083f_23ec, 0x083f_2b38, 0x083f_2d90, 0x083f_1674, 0x083f_0b08, 0x083f_0be4, 0x083f_0e98,
    0x0849_94b0, 0x084a_db78, 0x084b_be28,
];

impl Prog<'_> {
    fn count(&mut self, name: &'static str, ported: bool) {
        let table = if ported { &mut self.sc.contents.ported } else { &mut self.sc.contents.hooks };
        *table.entry(name).or_default() += 1;
    }

    /// The routine at `f` with the stack words `a`: the port or the adapter that exists
    /// for it, else a hook that answers 0.
    pub fn by_addr(&mut self, m: &mut Mem, f: u32, name: &'static str, a: &[u32]) -> u32 {
        self.un.port_call(m, f, a, false);
        let answer = match self.port(m, f, a) {
            Some(v) => Some(v),
            None => self.un.answer(m, f, a),
        };
        self.un.port_call(m, f, a, true);
        match answer {
            Some(v) => {
                self.count(name, true);
                v
            }
            None => {
                if READERS.contains(&f) && self.sc.contents.missing_reader.is_none() {
                    self.sc.contents.missing_reader = Some(name);
                }
                self.count(name, false);
                0
            }
        }
    }

    fn port(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> Option<u32> {
        use si_model::model_image::{defs, state, tree};
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        Some(match f {
            // SI_modelKineStateAllocate (0x0849f9f8): the kinematic state record a dynamics chain's root gets
            // when the file's token 0x12 says it has dynamics (si-kine's port on the image)
            0x0849_f9f8 => si_kine::image::qstretch::kine_state_allocate(m),
            // ---- si-model, on the image
            // SI_modelChangeName (0x0846db14)
            0x0846_db14 => {
                defs::change_name(m, Ptr32::new(w(0)), w(1));
                0
            }
            // SI_modelUpdateSonState (0x084662f8)
            0x0846_62f8 => {
                state::update_son_state(m, self, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16);
                0
            }
            // SI_modelUpdateDependency (0x0846f200), which SI_graflistBuildFromElemDict calls
            // for every model it enters: a deformed node gets a working definition of its own
            // (+0x48), so that its deformations do not move the stored one; and SI_patchCopy
            // (0x0847a2b8), which it calls for a patch (si-spline). `SCENE_NO_UPDATE_DEPENDENCY=1`
            // leaves both hooks (answered 0), as before they were routed
            0x0846_f200 if update_dependency_on() => {
                si_model::model_image::readjust::update_dependency(m, self, Ptr32::new(w(0)), w(1));
                0
            }
            0x0847_a680 => {
                si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::new(w(0)));
                0
            }
            0x0847_a2b8 if update_dependency_on() => si_spline::patch_image::patch_copy(m, Ptr32::new(w(0))).addr(),
            // SI_modelNextNode (0x08478340), SI_modelNextSon (0x0846d70c), SI_modelRoot (0x0847838c)
            0x0847_8340 => tree::next_node(m, Ptr32::new(w(0))).0,
            0x0846_d70c => tree::next_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).0,
            0x0847_838c => tree::root(m, Ptr32::new(w(0))).0,
            // SI_modelAddILattice (0x08472a0c)
            0x0847_2a0c => {
                defs::add_ilattice(m, Ptr32::new(w(0)), w(1) as i16, w(2) as i16, w(3));
                0
            }
            // ---- this crate: edtools.c and the name records
            // SI_edGetElemFromModel (0x080fcf20), SI_edFindSon (0x080fcec0),
            // SI_edFindNextSon (0x080fcef0), SI_edGetElemFromTexture2D (0x080fd3e0)
            0x080f_cf20 => edtools::ed_get_elem_from_model(self, m, Ptr32::new(w(0)), Ptr32::new(w(1))).0,
            0x080f_cec0 => edtools::ed_find_son(self, m, Ptr32::new(w(0))).0,
            0x080f_cef0 => edtools::ed_find_next_son(self, m, Ptr32::new(w(0))).0,
            0x080f_d3e0 => edtools::ed_get_elem_from_texture2d(self, m, w(0), Ptr32::new(w(1))).0,
            // SI_elemdictUpdateModelHierarchy (0x080fd698), SI_elemdictAddModel (0x081017f4),
            // SI_edAddModel (0x08101ad4)
            0x080f_d698 => {
                edtools::elemdict_update_model_hierarchy(self, m, Ptr32::new(w(0)));
                0
            }
            0x0810_17f4 => {
                edtools::elemdict_add_model(self, m, Ptr32::new(w(0)), Ptr32::new(w(1)), Ptr32::new(w(2)), w(3) as i16);
                0
            }
            0x0810_1ad4 => {
                edtools::ed_add_model(self, m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16);
                0
            }
            // SI_modelNbSubModels (0x0846d5ec), SI_lightGetGrafType (0x082800fc)
            0x0846_d5ec => edtools::model_nb_sub_models(m, Ptr32::new(w(0))) as u32,
            0x0828_00fc => edtools::light_get_graf_type(m, w(0)) as u32,
            // SI_dbListNodeAllocate (0x0812250c), SI_dbFormatFullName (0x081223dc)
            0x0812_250c => crate::dbnode::list_node_allocate(m, &mut Out { sc: self.sc, un: self.un }).0,
            0x0812_23dc => crate::dbnode::format_full_name(m, Ptr32::new(w(0))).0,
            // ---- si-dict
            // SI_elemdictFindElemByData (0x08134364), SI_elemdictGetElem (0x08132b50)
            0x0813_4364 => si_dict::elemdict::find_elem_by_data(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2))) as u32,
            0x0813_2b50 => si_dict::elemdict::get_elem(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32).0,
            // SI_elemdictGetElemFromData (0x08137f94)
            0x0813_7f94 => si_dict::elemmisc::get_elem_from_data(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), Ptr32::new(w(3))).0,
            // SI_elemdictFindSon (0x08137ff4), SI_elemdictFindNextSon (0x08138030)
            0x0813_7ff4 => si_dict::elemmisc::find_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).0,
            0x0813_8030 => si_dict::elemmisc::find_next_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).0,
            // SI_elemdictFindRelatedElem (0x0813508c), SI_elemdictFindNextRelatedElem (0x08135154)
            0x0813_508c => si_dict::elemrel::find_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0,
            0x0813_5154 => si_dict::elemrel::find_next_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0,
            // what si_elemCheckRestrictedSave (crate::dictout) calls: SI_elemdictFindSerialRelation
            // (0x0813335c), SI_elemdictGetCurRelationIndex (0x08138224),
            // SI_elemdictSetCurRelationIndex (0x0813825c), SI_relGetAnimFRelFromChapter (0x0810f540)
            crate::dictout::SI_ELEMDICT_FIND_SERIAL_RELATION => {
                si_dict::elemrel::find_serial_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, Ptr32::new(w(3)), w(4) as i32, w(5) as i16, w(6) as i16).0
            }
            crate::dictout::SI_ELEMDICT_GET_CUR_RELATION_INDEX => si_dict::elemmisc::get_cur_relation_index(m, Ptr32::new(w(0)), w(1) as i16, w(2) as i32) as u32,
            crate::dictout::SI_ELEMDICT_SET_CUR_RELATION_INDEX => {
                si_dict::elemmisc::set_cur_relation_index(m, Ptr32::new(w(0)), w(1) as i16, w(2) as i32, w(3) as i32);
                0
            }
            crate::dictout::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER => crate::dbvsfcv::rel_get_anim_f_rel_from_chapter(m, w(0) as i32) as u32,
            // SI_elemdictUpdateHierInfo (0x08134cc8), SI_elemdictSortHierElements (0x08140f30)
            0x0813_4cc8 => {
                si_dict::elemdict::update_hier_info(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32, w(3) as i32);
                0
            }
            0x0814_0f30 => {
                si_dict::toposort::sort_hier_elements(m, Ptr32::new(w(0)), w(1) as i32);
                0
            }
            // SI_elemdictFindListByName (0x081325f4)
            0x0813_25f4 => si_dict::elemdict::find_list_by_name(m, Ptr32::new(w(0)), Ptr32::new(w(1))) as u32,
            // what the material conversion's routines call by address: SI_elemdictGetNewName
            // (0x0813697c), SI_elemdictAddElem (0x08132a5c), SI_elemdictChangeName (0x08136450),
            // SI_elemdictAddUnCompleteRelation (0x08132fb4), SI_elemdictSetDirty (0x08132be8),
            // SI_attdictSetFcurveFlag (0x0811ff18)
            edtools::SI_ELEMDICT_GET_NEW_NAME => si_dict::elemname::get_new_name(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2))).addr(),
            edtools::SI_ELEMDICT_ADD_ELEM => {
                let (ed, node, data, aux) = (Ptr32::new(w(0)), Ptr32::new(w(2)), Ptr32::new(w(3)), Ptr32::new(w(4)));
                si_dict::elemdict::add_elem(m, &mut Out { sc: self.sc, un: self.un }, ed, w(1) as i32, node, data, aux, w(5) as i32, w(6) as i32) as u32
            }
            crate::matconv::SI_ELEMDICT_CHANGE_NAME => {
                let (ed, e) = (Ptr32::new(w(0)), Ptr32::new(w(2)));
                si_dict::elemname::change_name(m, &mut Out { sc: self.sc, un: self.un }, ed, w(1) as i32, e, Ptr32::new(w(3)), Ptr32::new(w(4))) as u32
            }
            edtools::SI_ELEMDICT_ADD_UN_COMPLETE_RELATION => {
                let (sc, dc, src, dst, relid, rule, pos) = (w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32, w(7) as i32, w(8) as i32);
                si_dict::elemrel::add_uncomplete_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), sc, dc, src, dst, relid, rule, pos, Ptr32::new(w(9)), w(10) as i16) as u32
            }
            edtools::SI_ELEMDICT_SET_DIRTY => {
                si_dict::elemdict::set_dirty(m, Ptr32::new(w(0)), w(1) as i32);
                0
            }
            edtools::SI_ATTDICT_SET_FCURVE_FLAG => si_dict::attdict::set_fcurve_flag(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32) as u32,
            // SI_elemdictRelationsCount (0x08135700), SI_elemdictDisconnectANDConnectRel (0x08133780)
            0x0813_5700 => si_dict::elemrel::relations_count(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32,
            edtools::SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL => {
                let (ed, rd, from, to) = (Ptr32::new(w(0)), Ptr32::new(w(1)), Ptr32::new(w(3)), Ptr32::new(w(4)));
                si_dict::elemrel::disconnect_and_connect_rel(m, &mut Out { sc: self.sc, un: self.un }, ed, rd, w(2) as i32, from, to, w(5) as i32) as u32
            }
            // ---- si-view
            // SI_findGlData (0x0832ee68), SI_graflistAddData (0x0832d240),
            // SI_graflistSetActive (0x0832c488), SI_schematicNoRefresh (0x0832bfd0)
            0x0832_ee68 => si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0,
            0x0832_d240 => si_view::graflist::graflist_add_data(m, &mut si_view::Wired::new(ViewSide(self)), Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i16).0,
            0x0832_c488 => {
                si_view::graflist::graflist_set_active(m, Ptr32::new(w(0)), Ptr32::new(w(1)));
                0
            }
            0x0832_bfd0 => {
                si_view::graflist::schematic_no_refresh(m, w(0) as i16);
                0
            }
            // the five routines of si-view's seam: SI_graflistDeleteNode (0x0832d75c),
            // SI_nselectionSelectContent (0x083465a4), SI_schematicHierOperation
            // (0x0833c5dc), si_schemElemOper (0x0833c15c), SI_schemComputeHierarchiesBBox
            // (0x0833bd74)
            0x0832_d75c | 0x0834_65a4 | 0x0833_c5dc | 0x0833_c15c | 0x0833_bd74 => {
                let routine = si_view::wired::SEAM.iter().copied().find(|r| r.addr == f)?;
                si_view::Outside::call(&mut si_view::Wired::new(ViewSide(self)), m, routine, a)
            }
            // SI_edGetElemFromScene (0x080fd620, this crate) and SI_elemdictCreateDefaultDsc
            // (0x081369cc, si-dict), which SI_schemComputeHierarchiesBBox calls
            SI_ED_GET_ELEM_FROM_SCENE => edtools::ed_get_elem_from_scene(self, m, Ptr32::new(w(0))).0,
            SI_ELEMDICT_CREATE_DEFAULT_DSC => si_dict::elemname::create_default_dsc(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0))).addr(),
            // SI_schemComputeElemBBox (0x0833b864)
            0x0833_b864 => {
                let (ed, elem) = (Ptr32::new(w(0)), Ptr32::new(w(1)));
                si_view::graphsort::schem_compute_elem_bbox(m, &mut si_view::Wired::new(ViewSide(self)), ed, elem, w(2) as i32, w(3), w(4), w(5) as i16);
                0
            }
            // ---- si-hrc: SI_udfAllocate (0x0854c2f4), SI_udfDispose (0x0854c3c0)
            0x0854_c2f4 => si_hrc::image::udf::udf_allocate(m, self).0,
            0x0854_c3c0 => si_hrc::image::udf::udf_dispose(m, self, Ptr32::new(w(0))) as u32,
            // SI_udfCopy (0x0854c70c), SI_userDataCopyFromSetup (0x084630dc), which the material
            // conversion's routines call
            crate::matconv::SI_UDF_COPY => si_hrc::image::udf::udf_copy(m, self, Ptr32::new(w(0))).addr(),
            crate::matconv::SI_USER_DATA_COPY_FROM_SETUP => {
                si_hrc::image::userdata::user_data_copy_from_setup(m, Ptr32::new(w(0)), Ptr32::new(w(1)));
                0
            }
            // ---- this crate: the disposals of SI_scnRelationsFromElemDict (dispose.rs)
            // SI_materialDispose (0x08284900), SI_txt2dDispose (0x082ae014), SI_txt3dFree (0x082b4054)
            0x0828_4900 => {
                crate::dispose::material_dispose(self, m, w(0), w(1));
                0
            }
            0x082a_e014 => {
                crate::dispose::txt2d_dispose(self, m, w(0), w(1));
                0
            }
            0x082b_4054 => {
                crate::dispose::txt3d_free(self, m, w(0), w(1));
                0
            }
            // ---- si-view: SI_graflistRemoveGrafnode (0x0832c9ac)
            0x0832_c9ac => si_view::graflist::graflist_remove_grafnode(m, &mut si_view::Wired::new(ViewSide(self)), Ptr32::new(w(0)), Ptr32::new(w(1))) as u32,
            // ---- si-spline: SI_splineUpdateMatrix (0x0848abd0), which `splineBinaryRead` and
            // `SI_bezierShrink` call for every spline they make (the step matrix times the
            // type's basis, into +0x24; finding 0029, third pass)
            0x0848_abd0 => {
                si_spline::spline_image::spline_update_matrix(m, Ptr32::new(w(0)));
                0
            }
            // ---- si-deform: SI_envelopRelationSet (0x084bd5a8), which the load calls on the
            // dictionary loaded into: the skins' relations 0xcf, the switch `SI_envelopActive`
            // reads (the load records the call instead under `SCENE_NO_ENVELOP_RELATION_SET=1`)
            0x084b_d5a8 => {
                si_deform::envtools::envelop_relation_set(m, &mut RelSide(self), w(0));
                0
            }
            // ---- si-deform: SI_envelopRebuild (0x084bd2a0), which `SI_fdmodelBinaryRead` calls
            // on the hierarchy it read (finding 0029, third pass); its calls out go to the
            // ports `crate::envside` names, the rest here
            0x084b_d2a0 => {
                si_deform::envtools::envelop_rebuild(m, &mut si_deform::routed::Routed::new(&mut EnvSide(self)), Ptr32::new(w(0)), w(1) as i16, w(2) as i16);
                0
            }
            // ---- si-deform: SI_fittingAllocate (0x0843a130), SI_pfittingAllocate (0x0843a384)
            0x0843_a130 => si_deform::fitting::fitting_allocate(m).0,
            0x0843_a384 => si_deform::fitting::pfitting_allocate(m).0,
            // ---- si-render: SI_updTxt3dTurb (0x082b8d20), which the 3D texture readers call
            // (through `texture3DTransfer`) for every 3D texture they make: the turbulence
            // object at +0x8c, made on the image with the original's allocation calls and
            // the C library's mathematics as `si-x87` has it
            0x082b_8d20 => {
                si_render::t3d_fct_image::upd_txt3d_turb(m, w(0), &mut si_render::tt::TrigLib);
                0
            }
            // ---- what the untold relations call (`untold.rs`, finding 0018, section 19)
            // si-dict: SI_elemdictAddRelation, SI_elemdictRemoveRelationPair,
            // SI_elemdictRemoveRelationId, SI_reldictIsRelActive
            crate::untold::SI_ELEMDICT_ADD_RELATION => si_dict::elemrel::add_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32,
            crate::untold::SI_ELEMDICT_REMOVE_RELATION_PAIR => {
                si_dict::elemrel::remove_relation_pair(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32
            }
            crate::untold::SI_ELEMDICT_REMOVE_RELATION_ID => si_dict::elemrel::remove_relation_id(m, &mut Out { sc: self.sc, un: self.un }, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32) as u32,
            crate::untold::SI_RELDICT_IS_REL_ACTIVE => si_dict::reldict::is_rel_active(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i16) as u32,
            // si-kine: SI_skeChainRoot, SI_invKinIsBreakJnt
            crate::untold::SI_SKE_CHAIN_ROOT => si_kine::onimage::ske_chain_root(m, Ptr32::new(w(0))).addr(),
            crate::untold::SI_INV_KIN_IS_BREAK_JNT => si_kine::image::invkin::inv_kin_is_break_jnt(m, &mut KineSide(self), Ptr32::new(w(0)), w(1) as i16, f32::from_bits(w(2))) as u32,
            // si-deform: SI_mainGlobEnv, SI_envelopLevel, SI_envelopActive
            crate::untold::SI_MAIN_GLOB_ENV => si_deform::envtools::main_glob_env(m, Ptr32::new(w(0))) as u32,
            crate::untold::SI_ENVELOP_LEVEL => si_deform::envtools::envelop_level(m, Ptr32::new(w(0))) as u32,
            crate::untold::SI_ENVELOP_ACTIVE => si_deform::envtools::envelop_active(m, &mut si_deform::routed::Routed::new(&mut DeformSide(self)), Ptr32::new(w(0))) as u32,
            // this crate: SI_edIsItemConstrained (0x08106bbc), SI_edGetElemFromCamera
            // (0x080fcfb0), SI_edGetElemFromLight (0x080fd0bc)
            crate::untold::SI_ED_IS_ITEM_CONSTRAINED => edtools::ed_is_item_constrained(self, m, w(0), w(1), w(2) as i32, w(3) as i32, w(4) as i16, f32::from_bits(w(5))) as u16 as u32,
            0x080f_cfb0 => edtools::ed_get_elem_from_camera(self, m, w(0), Ptr32::new(w(1))).0,
            0x080f_d0bc => edtools::ed_get_elem_from_light(self, m, w(0), Ptr32::new(w(1))).0,
            // ---- si-mem: the allocator by address: SI_malloc (0x080d0ee8), SI_realloc (0x080d0f28)
            0x080d_0ee8 => m.si_malloc(w(0)),
            0x080d_0f28 => m.si_realloc(w(0), w(1)),
            // ---- this crate: SI_edAddMaterial (0x0810203c), SI_edAddTexture2D (0x08102314),
            // SI_edAddTexture3D (0x081027e0), which si-view's SI_graflistAddData calls; each
            // answers 1 at once while SI_addInDictionary is 0, as during a load
            0x0810_203c => edtools::ed_add_material(self, m, w(0), w(1) as i16, w(2)) as u32,
            0x0810_2314 => edtools::ed_add_texture2d(self, m, w(0), w(1) as i16, w(2)) as u32,
            0x0810_27e0 => edtools::ed_add_texture3d(self, m, w(0), w(1) as i16, w(2)) as u32,
            // SI_edDisconnectSharedMaterialOr3DTxt (0x0810080c), which the conversion
            // (matconv.rs, SI_dbConvertModelMatTxt2DB) calls by address
            crate::matconv::SI_ED_DISCONNECT_SHARED_MATERIAL_OR3D_TXT => {
                edtools::ed_disconnect_shared_material_or3d_txt(self, m, w(0), w(1), w(2) as i32, w(3), w(4) as i32, w(5) as i16, w(6) as i32, w(7) as i16);
                0
            }
            // ---- this crate: what the material conversion's routines call by address
            // (matconv.rs, custom.rs, edtools.rs; finding 0018, section 23)
            crate::matconv::SI_MATERIAL_TRANSFER => {
                crate::matconv::material_transfer(self, m, w(0), w(1), w(2) as i16);
                0
            }
            crate::matconv::SI_TXT2D_TRANSFER => {
                crate::matconv::txt2d_transfer(self, m, w(0), w(1), w(2) as i32);
                0
            }
            crate::matconv::SI_TXT3D_TRANSFER => {
                crate::matconv::txt3d_transfer(self, m, w(0), w(1), w(2) as i32);
                0
            }
            edtools::SI_MATERIAL_COPY => crate::matconv::material_copy(self, m, w(0)),
            edtools::SI_TXT2D_COPY => crate::matconv::txt2d_copy(self, m, w(0)),
            edtools::SI_TXT3D_COPY => crate::matconv::txt3d_copy(self, m, w(0)),
            edtools::SI_COPY_T2D_SHADER => {
                crate::matconv::copy_t2d_shader(self, m, w(0), w(1));
                0
            }
            edtools::SI_COPY_T3D_SHADER => {
                crate::matconv::copy_t3d_shader(self, m, w(0), w(1));
                0
            }
            crate::matconv::SI_ED_ADD_UDF => {
                edtools::ed_add_udf(self, m, w(0), w(1) as i32, w(2));
                0
            }
            edtools::SI_UDF_ATTACH_ARRAY_ELEMENTS => {
                crate::custom::udf_attach_array_elements(self, m, w(0));
                0
            }
            // ---- si-motion: the copies of an animation the conversion makes
            // (SI_texture2DLoadFcurveAll, SI_texture3DLoadFcurveAll, SI_elemdictCopyElemAnim),
            // their calls out through [`MotionSide`] (finding 0018, section 23.12)
            edtools::SI_TEXTURE2D_LOAD_FCURVE_ALL => {
                si_motion::animcopy::texture2d_load_fcurve_all(m, &mut MotionSide(self), w(0), w(1));
                0
            }
            edtools::SI_TEXTURE3D_LOAD_FCURVE_ALL => {
                si_motion::animcopy::texture3d_load_fcurve_all(m, &mut MotionSide(self), w(0), w(1));
                0
            }
            edtools::SI_ELEMDICT_COPY_ELEM_ANIM => {
                let (ed, from, to) = (w(0), w(1), w(2));
                si_motion::animcopy::elemdict_copy_elem_anim(m, &mut MotionSide(self), ed, from, to, w(3) as i32, w(4) as i32, w(5) as i32) as u32
            }
            // what they call that has a port: SI_fcvGetRelInfo (0x0810f698, this crate),
            // SI_attdictGetFcurveFlag (0x0811ff64) and SI_reldictGetRelName (0x0813a0f4)
            // (si-dict), SI_edGetElemFromMaterial (0x080fd288) and SI_edGetElemFromTexture3D
            // (0x080fd414) (this crate)
            SI_FCV_GET_REL_INFO => {
                crate::dbvsfcv::fcv_get_rel_info(m, w(0) as i32, Ptr32::new(w(1)), Ptr32::new(w(2)), Ptr32::new(w(3)));
                0
            }
            SI_ATTDICT_GET_FCURVE_FLAG => si_dict::attdict::get_fcurve_flag(m, Ptr32::new(w(0)), w(1) as i32) as u32,
            SI_RELDICT_GET_REL_NAME => si_dict::reldict::get_rel_name(m, Ptr32::new(w(0)), w(1) as i32).addr(),
            SI_ED_GET_ELEM_FROM_MATERIAL => edtools::ed_get_elem_from_material(self, m, w(0), Ptr32::new(w(1))).0,
            SI_ED_GET_ELEM_FROM_TEXTURE3D => edtools::ed_get_elem_from_texture3d(self, m, w(0), Ptr32::new(w(1))).0,
            // ---- si-dict: SI_elemdictGetRelationFromIds (0x08134e9c), which SI_envelopActive
            // calls for the untold relations, and SI_edSetUDFRelation (finding 0018, section
            // 23.12); `SCENE_NO_RELATION_FROM_IDS=1` leaves it a hook (answered 0), as before
            edtools::SI_ELEMDICT_GET_RELATION_FROM_IDS if relation_from_ids_on() => {
                let (ed, e) = (Ptr32::new(w(0)), Ptr32::new(w(1)));
                si_dict::elemrel::get_relation_from_ids(m, ed, e, w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32).addr()
            }
            // ---- this crate: SI_texture2DVerifyPicturePath (0x082b1528), which SI_edAddTexture2D
            // calls (&SI_dbdir, SI_pDbiin, dictionary, element, 0, 0) for each 2D texture the
            // load's material conversion makes an element; the list of databases is si-db's
            // value, laid out for the call by the load's adapter (`t2dpath::verify_in_load`)
            edtools::SI_TEXTURE2D_VERIFY_PICTURE_PATH if self.db.is_some() && self.fs.is_some() => {
                let (fs, info) = (self.fs?, self.db.as_deref_mut()?);
                let mut cx = crate::t2dpath::Ctx { sc: &mut *self.sc, fs, info, un: &mut *self.un };
                crate::t2dpath::verify_in_load(&mut cx, m, Ptr32::new(w(2)), Ptr32::new(w(3))) as u32
            }
            // ---- si-render: SI_txt2dSetWrap (0x082ad9f8), which the model reader calls for
            // every 2D texture of a node, and the conversion's copies
            edtools::SI_TXT2D_SET_WRAP => {
                si_render::t2d_dial::set_wrap(m, &mut RenderSide(self), w(0), w(1));
                0
            }
            // ---- what the wave reader calls (`wave.rs`, finding 0018, section 20)
            // SI_edAddWave (0x081033b4): tests SI_addInDictionary first and does nothing
            // when it is 0, as during a load (read from code; with the switch set a hook)
            crate::wave::SI_ED_ADD_WAVE if m[crate::globals::SI_ADD_IN_DICTIONARY] == 0 => 0,
            // SI_meshUpdate (0x08458d94): SI_meshUpdateNodes(mesh, 1) without its answer
            // (read from code: the same calls in the same order), the mesh adapter below
            crate::wave::SI_MESH_UPDATE if mesh_update_on() => {
                mesh_rebuild(m, self.sc, 0x0845_8cf8, w(0), 1);
                0
            }
            // ---- this crate: SI_waveAddMdl (0x084c9810), which SI_scnRelationsFromElemDict calls
            // for relations 600 and 601 (wave_mdl.rs): the model's wave list (+0x2e8) and its
            // working definition copied; and the copies it asks for that have a port on the
            // image, SI_meshCopy (0x084404e0, si-model) and SI_nurbsSrfCopy (0x084dc1a8, si-nurbs)
            // (SI_faceCopy has none: a hook, as for SI_modelUpdateDependency).
            // `SCENE_NO_WAVE_ADD_MDL=1` leaves the three hooks (answered 0), as before
            crate::db2scn::SI_WAVE_ADD_MDL if wave_add_mdl_on() => {
                crate::wave_mdl::wave_add_mdl(self, m, w(0), w(1), w(2) as i16, w(3) as i16);
                0
            }
            crate::wave_mdl::SI_MESH_COPY if wave_add_mdl_on() => si_model::mesh_image::obj::mesh_copy(m, self, w(0)),
            crate::wave_mdl::SI_NURBS_SRF_COPY if wave_add_mdl_on() => si_nurbs::image::srf_copy(m, &mut crate::envside::NurbsSide(self), w(0)),
            // ---- adapters: routines that exist on values only
            // SI_modelUpdateMatrix (0x0846559c)
            0x0846_559c => {
                model_update_matrix(m, Ptr32::new(w(0)));
                0
            }
            // SI_matrixXyzRotation (0x0851882c)
            0x0851_882c => {
                matrix_xyz_rotation(m, w(0), w(1));
                0
            }
            // SI_meshUpdateNodes (0x08458cf8), SI_meshComputesPolygonNormal (0x084599f4),
            // SI_meshComputeHardEdge (0x0845a6e4), SI_meshComputesVertexNormal (0x08459e8c)
            0x0845_8cf8 | 0x0845_99f4 | 0x0845_a6e4 | 0x0845_9e8c => mesh_rebuild(m, self.sc, f, w(0), w(1)),
            _ => return None,
        })
    }

    fn open(&mut self, m: &mut Mem, path: u32) -> Option<(Vec<u8>, Vec<u8>)> {
        let path = m.cstr(path);
        let data = self.fs?.read(&path)?;
        Some((path, data))
    }
}

impl ByAddr for Prog<'_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        self.by_addr(m, f, name_of(f), args)
    }
}

/// `SCENE_NO_UPDATE_DEPENDENCY=1` leaves `SI_modelUpdateDependency` and `SI_patchCopy` hooks
/// (answered 0) during a load, as before they were routed.
fn update_dependency_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_UPDATE_DEPENDENCY").is_none())
}

/// `SCENE_NO_WAVE_ADD_MDL=1` leaves `SI_waveAddMdl`, `SI_meshCopy` and `SI_nurbsSrfCopy`
/// hooks (answered 0) during a load, as before they were routed.
fn wave_add_mdl_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_WAVE_ADD_MDL").is_none())
}

/// Set by a check of the routine that calls `SI_meshUpdate` (`si_waveCreateIconMdl`, re/tools/scene3):
/// the mesh update is then a hook (answered 0), to hold that routine apart from the mesh code, whose
/// boxed mesh does not take the original's heap blocks. Never set by the program.
pub static MESH_UPDATE_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn mesh_update_on() -> bool {
    !MESH_UPDATE_OFF.load(std::sync::atomic::Ordering::Relaxed)
}

/// `SCENE_NO_RELATION_FROM_IDS=1` leaves `SI_elemdictGetRelationFromIds` a hook (answered 0)
/// during a load, as before it was routed.
fn relation_from_ids_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_RELATION_FROM_IDS").is_none())
}

/// `SCENE_NO_ENVELOP_RELATION_SET=1`: the load records `SI_envelopRelationSet` as a call
/// not ported, as before it was routed.
pub(crate) fn envelop_relation_set_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_ENVELOP_RELATION_SET").is_none())
}

/// The name of a routine [`edtools`] calls by address.
fn name_of(f: u32) -> &'static str {
    match f {
        edtools::SI_ELEMDICT_FIND_ELEM_BY_DATA => "SI_elemdictFindElemByData",
        edtools::SI_ELEMDICT_GET_ELEM => "SI_elemdictGetElem",
        edtools::SI_ELEMDICT_FIND_SON => "SI_elemdictFindSon",
        edtools::SI_ELEMDICT_FIND_NEXT_SON => "SI_elemdictFindNextSon",
        edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA => "SI_elemdictGetElemFromData",
        edtools::SI_ELEMDICT_UPDATE_HIER_INFO => "SI_elemdictUpdateHierInfo",
        edtools::SI_ELEMDICT_SORT_HIER_ELEMENTS => "SI_elemdictSortHierElements",
        edtools::SI_DB_LIST_NODE_ALLOCATE => "SI_dbListNodeAllocate",
        edtools::SI_ELEMDICT_GET_NEW_NAME => "SI_elemdictGetNewName",
        edtools::SI_ELEMDICT_ADD_UN_COMPLETE_RELATION => "SI_elemdictAddUnCompleteRelation",
        edtools::SI_ELEMDICT_SET_DIRTY => "SI_elemdictSetDirty",
        edtools::SI_ELEMDICT_GET_RELATION_FROM_IDS => "SI_elemdictGetRelationFromIds",
        edtools::SI_MODEL_CHANGE_NAME => "SI_modelChangeName",
        edtools::SI_DB_FORMAT_FULL_NAME => "SI_dbFormatFullName",
        edtools::SI_ELEMDICT_ADD_ELEM => "SI_elemdictAddElem",
        edtools::SI_ELEMDICT_ADD_RELATION => "SI_elemdictAddRelation",
        SI_MODEL_UPDATE_SON_STATE => "SI_modelUpdateSonState",
        // what SI_scnRelationsFromElemDict and the three disposals call
        crate::db2scn::SI_NSELECTION_ALLOCATE => "SI_nselectionAllocate",
        crate::db2scn::SI_MATERIAL_DISPOSE => "SI_materialDispose",
        crate::db2scn::SI_TXT2D_DISPOSE => "SI_txt2dDispose",
        crate::db2scn::SI_TXT3D_FREE => "SI_txt3dFree",
        crate::db2scn::SI_FITTING_ALLOCATE => "SI_fittingAllocate",
        crate::db2scn::SI_PFITTING_ALLOCATE => "SI_pfittingAllocate",
        crate::db2scn::SI_WAVE_ADD_MDL => "SI_waveAddMdl",
        crate::db2scn::SI_REL_GET_DYN_CONS_TYPE => "si_relGetDynConsType",
        crate::db2scn::SI_CONSLIST_FIND_PRIORITY => "SI_conslistFindPriority",
        crate::db2scn::SI_CONSLIST_CONSTR_ALLOCATE => "SI_conslistConstrAllocate",
        crate::db2scn::SI_CONSLIST_ADD_CONSTRAINT => "SI_conslistAddConstraint",
        crate::db2scn::SI_CUS_CREATE_RELATIONS => "SI_cusCreateRelations",
        crate::dispose::SI_REMOVE_TRIANGLE => "SI_removeTriangle",
        crate::untold::SI_ELEMDICT_REMOVE_RELATION_PAIR => "SI_elemdictRemoveRelationPair",
        crate::untold::SI_ELEMDICT_REMOVE_RELATION_ID => "SI_elemdictRemoveRelationId",
        crate::untold::SI_ELEMDICT_FIND_RELATED_ELEM => "SI_elemdictFindRelatedElem",
        crate::untold::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM => "SI_elemdictFindNextRelatedElem",
        crate::untold::SI_RELDICT_IS_REL_ACTIVE => "SI_reldictIsRelActive",
        crate::untold::SI_SKE_CHAIN_ROOT => "SI_skeChainRoot",
        crate::untold::SI_INV_KIN_IS_BREAK_JNT => "SI_invKinIsBreakJnt",
        crate::untold::SI_MAIN_GLOB_ENV => "SI_mainGlobEnv",
        crate::untold::SI_ENVELOP_LEVEL => "SI_envelopLevel",
        crate::untold::SI_ENVELOP_ACTIVE => "SI_envelopActive",
        crate::untold::SI_ED_IS_ITEM_CONSTRAINED => "SI_edIsItemConstrained",
        crate::untold::SI_ED_MDL_CHAP_REL_BACK_UP => "SI_edMdlChapRelBackUp",
        crate::untold::SI_ED_SWAP_UN_TOLD_REL => "SI_edSwapUnToldRel",
        crate::wave::SI_ED_ADD_WAVE => "SI_edAddWave",
        crate::wave::SI_BACK_SELECTED => "SI_backSelected",
        crate::wave::SI_MESH_UPDATE => "SI_meshUpdate",
        crate::wave::SI_MATRIX_XYZ_ROTATION => "SI_matrixXyzRotation",
        crate::wave::SI_MODEL_UPDATE_MATRIX => "SI_modelUpdateMatrix",
        crate::wave::SI_GRAFLIST_ADD_DATA => "SI_graflistAddData",
        // what SI_waveAddMdl calls
        crate::wave_mdl::SI_ED_MODEL_LINK_WAVE => "SI_edModelLinkWave",
        crate::wave_mdl::SI_PATCH_COPY => "SI_patchCopy",
        crate::wave_mdl::SI_FACE_COPY => "SI_faceCopy",
        crate::wave_mdl::SI_MESH_COPY => "SI_meshCopy",
        crate::wave_mdl::SI_NURBS_SRF_COPY => "SI_nurbsSrfCopy",
        crate::wave_mdl::SI_MODEL_ROOT => "SI_modelRoot",
        crate::wave_mdl::SI_MODEL_UPDATE_DEPENDENCY => "SI_modelUpdateDependency",
        0x080f_cfb0 => "SI_edGetElemFromCamera",
        0x080f_d0bc => "SI_edGetElemFromLight",
        crate::dispose::SI_FIND_GL_DATA => "SI_findGlData",
        crate::dispose::SI_ED_DISPOSE_TEXTURE2D => "SI_edDisposeTexture2D",
        crate::dispose::SI_GRAFLIST_REMOVE_GRAFNODE => "SI_graflistRemoveGrafnode",
        crate::dispose::SI_TEXTURE3D_STILL_USED => "SI_texture3DStillUsed",
        crate::dispose::SI_ED_DISPOSE_TEXTURE3D => "SI_edDisposeTexture3D",
        crate::dispose::SI_MODEL_RESET_MATERIAL => "SI_modelResetMaterial",
        crate::dispose::SI_MODEL_RESET_TEXTURE => "SI_modelResetTexture",
        crate::dispose::SI_MODEL_RESET_TEXTURE3D => "SI_modelResetTexture3D",
        crate::dispose::SI_UDF_DISPOSE => "SI_udfDispose",
        crate::dispose::SI_USER_DATA_DISPOSE => "SI_userDataDispose",
        crate::dispose::SI_GLOBAL2D_MAP_REMOVE => "SI_global2dMapRemove",
        crate::dispose::SI_CROP_VIEW_FREE => "si_cropViewFree",
        // what the material conversion's routines call (finding 0018, section 23)
        edtools::SI_SCHEMATIC_HIER_OPERATION => "SI_schematicHierOperation",
        edtools::SI_ATTDICT_SET_FCURVE_FLAG => "SI_attdictSetFcurveFlag",
        edtools::SI_MATERIAL_COPY => "SI_materialCopy",
        edtools::SI_TXT2D_COPY => "SI_txt2dCopy",
        edtools::SI_TXT3D_COPY => "SI_txt3dCopy",
        edtools::SI_TXT2D_SET_WRAP => "SI_txt2dSetWrap",
        edtools::SI_COPY_T2D_SHADER => "SI_copyT2DShader",
        edtools::SI_COPY_T3D_SHADER => "SI_copyT3DShader",
        edtools::SI_TEXTURE2D_LOAD_FCURVE_ALL => "SI_texture2DLoadFcurveAll",
        edtools::SI_TEXTURE3D_LOAD_FCURVE_ALL => "SI_texture3DLoadFcurveAll",
        edtools::SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL => "SI_elemdictDisconnectANDConnectRel",
        edtools::SI_ELEMDICT_COPY_ELEM_ANIM => "SI_elemdictCopyElemAnim",
        edtools::SI_TEXTURE2D_VERIFY_PICTURE_PATH => "SI_texture2DVerifyPicturePath",
        edtools::SI_UDF_ATTACH_ARRAY_ELEMENTS => "SI_udfAttachArrayElements",
        crate::matconv::SI_MATERIAL_TRANSFER => "SI_materialTransfer",
        crate::matconv::SI_TXT2D_TRANSFER => "SI_txt2dTransfer",
        crate::matconv::SI_TXT3D_TRANSFER => "SI_txt3dTransfer",
        crate::matconv::SI_UDF_COPY => "SI_udfCopy",
        crate::matconv::SI_ED_ADD_UDF => "SI_edAddUDF",
        crate::matconv::SI_ELEMDICT_CHANGE_NAME => "SI_elemdictChangeName",
        crate::matconv::SI_ED_SOFT_ELEM_CHANGE_NAME => "SI_edSoftElemChangeName",
        crate::matconv::SI_USER_DATA_COPY_FROM_SETUP => "SI_userDataCopyFromSetup",
        SI_ED_GET_ELEM_FROM_MATERIAL => "SI_edGetElemFromMaterial",
        SI_ED_GET_ELEM_FROM_TEXTURE3D => "SI_edGetElemFromTexture3D",
        SI_ED_GET_ELEM_FROM_SCENE => "SI_edGetElemFromScene",
        SI_ELEMDICT_CREATE_DEFAULT_DSC => "SI_elemdictCreateDefaultDsc",
        crate::dictout::SI_ELEMDICT_FIND_SERIAL_RELATION => "SI_elemdictFindSerialRelation",
        crate::dictout::SI_ELEMDICT_GET_CUR_RELATION_INDEX => "SI_elemdictGetCurRelationIndex",
        crate::dictout::SI_ELEMDICT_SET_CUR_RELATION_INDEX => "SI_elemdictSetCurRelationIndex",
        crate::dictout::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER => "SI_relGetAnimFRelFromChapter",
        _ => "(by address)",
    }
}

/// `si-expr`'s calls out during a load (`SI_expressionsRegenerate`), handed to the loader's
/// dispatch by address; `getenv` answers null (no `SI_DO_NOT_REMOVE_ORPHANED_EXP`), what is
/// printed goes to the scene's messages.
pub struct ExprSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_mem::Outside for ExprSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        match r.name {
            "getenv" | "printf" => si_mem::Ret::NONE,
            name => si_mem::Ret::int(self.0.by_addr(m, r.addr, if name.is_empty() { "(by address)" } else { name }, args) as i32),
        }
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        let channel = if stream == 2 { crate::calls::Channel::Stderr } else { crate::calls::Channel::Stdout };
        self.0.sc.messages.push(crate::calls::Message { channel, text: text.to_vec() });
    }
}

/// `si-kine`'s calls out, handed to the loader's dispatch by address.
pub struct KineSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_kine::outside::Outside for KineSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_kine::outside::Routine, args: &[u32]) -> si_kine::outside::Ret {
        si_kine::outside::Ret::int(self.0.by_addr(m, r.addr, r.name, args))
    }
}

/// `si-deform`'s calls out, likewise.
pub struct DeformSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_deform::outside::Outside for DeformSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_deform::outside::Routine, args: &[u32]) -> u32 {
        self.0.by_addr(m, r.addr, r.name, args)
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

/// `si-deform`'s calls out from the envelope's routines (`SI_envelopRebuild` and what it
/// calls): the ports [`crate::envside::envelope_call`] names, else the loader's dispatch.
pub struct EnvSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_deform::outside::Outside for EnvSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_deform::outside::Routine, args: &[u32]) -> u32 {
        if let Some(v) = crate::envside::envelope_call(&mut *self.0, m, r.addr, args) {
            self.0.count(r.name, true);
            return v;
        }
        self.0.by_addr(m, r.addr, r.name, args)
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

/// `si-deform`'s calls out from `SI_envelopRelationSet`: `SI_elemdictAddRelation`
/// (0x08132f78) by `si-dict`'s port, the rest by the loader's dispatch (both look-ups are
/// `si-dict`'s ports there).
pub struct RelSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_deform::outside::Outside for RelSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_deform::outside::Routine, args: &[u32]) -> u32 {
        if r.addr == si_deform::outside::SI_ELEMDICT_ADD_RELATION.addr {
            let w = |k: usize| args.get(k).copied().unwrap_or(0);
            self.0.count(r.name, true);
            return si_dict::elemrel::add_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32;
        }
        self.0.by_addr(m, r.addr, r.name, args)
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

impl si_hrc::image::Outside for Prog<'_> {
    fn fopen(&mut self, m: &mut Mem, path: u32, _mode: u32) -> u32 {
        match self.open(m, path) {
            Some((path, data)) => self.sc.contents.hrc.open_bytes(&path, data),
            None => 0,
        }
    }
    fn fclose(&mut self, _m: &mut Mem, file: u32) -> i32 {
        self.sc.contents.hrc.fclose(file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.sc.contents.hrc.fread(m, ptr, size, n, file)
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.sc.contents.hrc.fgetc(m, file)
    }
    fn rewind(&mut self, _m: &mut Mem, file: u32) {
        self.sc.contents.hrc.rewind(file)
    }
    fn bytes_left(&mut self, _m: &mut Mem, file: u32) -> u64 {
        self.sc.contents.hrc.bytes_left(file)
    }
    fn call(&mut self, m: &mut Mem, routine: &'static si_hrc::image::Routine, args: &[u32]) -> u32 {
        self.by_addr(m, routine.addr, routine.name, args)
    }
}

impl si_elemio::image::Outside for Prog<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_elemio::image::Routine, args: &[u32]) -> u32 {
        use si_elemio::image::outside as eo;
        let a = |k: usize| args.get(k).copied().unwrap_or(0);
        if routine.addr == eo::FOPEN.addr {
            return match self.open(m, a(0)) {
                Some((_, data)) => self.sc.contents.eio.open(data),
                None => 0,
            };
        }
        if let Some(v) = self.sc.contents.eio.call(m, routine, args) {
            return v;
        }
        if routine.addr == eo::TIME.addr {
            let now = self.sc.answers.now as u32;
            if a(0) != 0 {
                m.set_u32(a(0), now);
            }
            return now;
        }
        if routine.addr == eo::SI_STATUS_DISPLAY_MESSAGE.addr {
            let text = m.cstr(a(0));
            self.sc.contents.status.push((a(1) as i32, text));
            self.count(routine.name, false);
            return 0;
        }
        self.by_addr(m, routine.addr, routine.name, args)
    }
    fn print(&mut self, _m: &mut Mem, text: &[u8]) {
        self.sc.messages.push(Message { channel: Channel::Stdout, text: text.to_vec() });
    }
    fn print_err(&mut self, _m: &mut Mem, text: &[u8]) {
        self.sc.messages.push(Message { channel: Channel::Stderr, text: text.to_vec() });
    }
    fn file_left(&mut self, _m: &mut Mem, file: u32) -> u32 {
        self.sc.contents.eio.left(file)
    }
}

impl si_model::model_image::Outside for Prog<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
        // the callee's name lives as long as its table; a callback has only an address
        let name = si_model::model_image::callee::ALL.iter().find(|c| c.addr == callee.addr).map_or("(through a pointer)", |c| c.name);
        self.by_addr(m, callee.addr, name, args)
    }
}

impl si_view::Outside for Prog<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.by_addr(m, routine.addr, routine.name, args)
    }
}

/// [`Prog`] as `si-view` calls out, by value, so that it can stand inside `si_view::Wired`.
pub struct ViewSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_view::Outside for ViewSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.0.by_addr(m, routine.addr, routine.name, args)
    }
}

/// [`Prog`] as `si-render`'s routines on the image call out (`SI_txt2dSetWrap`: only
/// `fprintf`, a hook).
pub struct RenderSide<'a, 'b>(pub &'a mut Prog<'b>);
impl si_render::img::Outside for RenderSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        let name = if f == si_render::img::f::FPRINTF { "fprintf" } else { "(by address)" };
        self.0.by_addr(m, f, name, args)
    }
}

/// `SI_fcvGetRelInfo (0x0810f698)`, `dbvsfcv.c`: (curve type, &relation, &rule, &limit).
pub const SI_FCV_GET_REL_INFO: u32 = 0x0810_f698;
/// `SI_attdictGetFcurveFlag (0x0811ff64)`, `attdict.c`: (attribute dictionary, curve type).
pub const SI_ATTDICT_GET_FCURVE_FLAG: u32 = 0x0811_ff64;
/// `SI_reldictGetRelName (0x0813a0f4)`, `reldict.c`: (relation dictionary, relation).
pub const SI_RELDICT_GET_REL_NAME: u32 = 0x0813_a0f4;
/// `SI_edGetElemFromMaterial (0x080fd288)`, `edtools.c`: (material, &index).
pub const SI_ED_GET_ELEM_FROM_MATERIAL: u32 = 0x080f_d288;
/// `SI_edGetElemFromTexture3D (0x080fd414)`, `edtools.c`: (texture, &index).
pub const SI_ED_GET_ELEM_FROM_TEXTURE3D: u32 = 0x080f_d414;
/// `SI_edFcvAdd (0x080ee650)`, `edanim.c`: (curve, element, chapter, index). Not ported.
pub const SI_ED_FCV_ADD: u32 = 0x080e_e650;
/// `SI_fastSetDirty (0x08329898)`, `fastplay.c`: (port, −1, 1). Not ported.
pub const SI_FAST_SET_DIRTY: u32 = 0x0832_9898;
/// `SI_setFcvTableEntriesFromChap (0x08234e3c)`: (chapter). Not ported.
pub const SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP: u32 = 0x0823_4e3c;
/// `SI_edGetElemFromScene (0x080fd620)`, `edtools.c`: (&index).
pub const SI_ED_GET_ELEM_FROM_SCENE: u32 = 0x080f_d620;
/// `SI_elemdictCreateDefaultDsc (0x081369cc)`, `elemname.c`: (dictionary).
pub const SI_ELEMDICT_CREATE_DEFAULT_DSC: u32 = 0x0813_69cc;
/// `SI_statusDisplayMessage (0x083cca94)`, `status.c`: (text, kind). Not ported.
pub const SI_STATUS_DISPLAY_MESSAGE: u32 = 0x083c_ca94;

/// [`Prog`] as `si-motion`'s copies of an animation call out (`SI_texture2DLoadFcurveAll`,
/// `SI_texture3DLoadFcurveAll`, `SI_elemdictCopyElemAnim`; `animcopy.rs`): the methods of
/// its host traits these three reach (directly or through `SI_fcurveRemoveAllKeys`,
/// `SI_fcurveTransfer` and `SI_fcurveAllocate`, which `si-motion` has on the image) go to
/// the loader's dispatch by the original's address, with the original's arguments. A status
/// line goes into the scene's status lines, as the element readers' do. The traits' other
/// methods, which these three do not reach, keep their default (answer 0, not counted).
pub struct MotionSide<'a, 'b>(pub &'a mut Prog<'b>);

impl MotionSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, f: u32, name: &'static str, a: &[u32]) -> u32 {
        self.0.by_addr(m, f, name, a)
    }
    fn status(&mut self, m: &mut Mem, text: u32, kind: i32) {
        let text = m.cstr(text);
        self.0.sc.contents.status.push((kind, text));
        self.0.count("SI_statusDisplayMessage", false);
    }
}

impl si_motion::ed::EdHost for MotionSide<'_, '_> {}

impl si_motion::key::KeyHost for MotionSide<'_, '_> {
    fn fcv_get_rel_info(&mut self, m: &mut Mem, kind: i32, relid: u32, a: u32, b: u32) {
        self.call(m, SI_FCV_GET_REL_INFO, "SI_fcvGetRelInfo", &[kind as u32, relid, a, b]);
    }
    fn ed_fcv_add(&mut self, m: &mut Mem, curve: u32, elem: u32, chap: i32, index: i32) -> i32 {
        self.call(m, SI_ED_FCV_ADD, "SI_edFcvAdd", &[curve, elem, chap as u32, index as u32]) as i32
    }
    fn status_display_message(&mut self, m: &mut Mem, text: u32, kind: i32) {
        self.status(m, text, kind);
    }
    fn ed_get_elem_from_material(&mut self, m: &mut Mem, mat: u32, index: u32) -> u32 {
        self.call(m, SI_ED_GET_ELEM_FROM_MATERIAL, "SI_edGetElemFromMaterial", &[mat, index])
    }
    fn ed_get_elem_from_texture2d(&mut self, m: &mut Mem, tex: u32, index: u32) -> u32 {
        self.call(m, 0x080f_d3e0, "SI_edGetElemFromTexture2D", &[tex, index])
    }
    fn ed_get_elem_from_texture3d(&mut self, m: &mut Mem, tex: u32, index: u32) -> u32 {
        self.call(m, SI_ED_GET_ELEM_FROM_TEXTURE3D, "SI_edGetElemFromTexture3D", &[tex, index])
    }
}

impl si_motion::update::UpdateHost for MotionSide<'_, '_> {
    fn status_display_message(&mut self, m: &mut Mem, text: u32, kind: i32) {
        self.status(m, text, kind);
    }
}

impl si_motion::reset::ResetHost for MotionSide<'_, '_> {
    fn attdict_get_fcurve_flag(&mut self, m: &mut Mem, atd: u32, kind: i32) -> i16 {
        self.call(m, SI_ATTDICT_GET_FCURVE_FLAG, "SI_attdictGetFcurveFlag", &[atd, kind as u32]) as i16
    }
    fn fast_set_dirty(&mut self, m: &mut Mem, port: u32, a: i32, b: i32) {
        self.call(m, SI_FAST_SET_DIRTY, "SI_fastSetDirty", &[port, a as u32, b as u32]);
    }
    fn set_fcv_table_entries_from_chap(&mut self, m: &mut Mem, chap: i32) {
        self.call(m, SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP, "SI_setFcvTableEntriesFromChap", &[chap as u32]);
    }
}

impl si_motion::utils::UtilsHost for MotionSide<'_, '_> {}

impl si_motion::animcopy::CopyHost for MotionSide<'_, '_> {
    fn reldict_get_rel_name(&mut self, m: &mut Mem, reldict: u32, relid: i32) -> u32 {
        self.call(m, SI_RELDICT_GET_REL_NAME, "SI_reldictGetRelName", &[reldict, relid as u32])
    }
}

// ------------------------------------------------------------------------------ adapters

/// `SI_modelUpdateMatrix (0x0846559c)` for a node of the image.
// adapter: not in place
/// `si-model` has the routine on its own store of nodes (`transform::update_matrix`): the
/// node's record is copied into such a store, the routine runs there, and the global
/// matrix it built is stored back.
pub fn model_update_matrix(m: &mut Mem, node: Ptr32<Model>) {
    let mut models = si_model::node::Models::default();
    let id = models.allocate(None);
    *models.m_mut(id) = m.read(node);
    si_model::transform::update_matrix(&mut models, id);
    m[node].gmat = models.m(id).gmat;
}

/// `SI_matrixXyzRotation (0x0851882c)`: the matrix of three angles, stored at `matrix`.
// adapter: not in place
/// `si-math` has the routine on values: the angles are read from the image and the
/// sixteen floats it answers are stored.
pub fn matrix_xyz_rotation(m: &mut Mem, angles: u32, matrix: u32) {
    let rot = si_math::types::Vector { x: m.f32(angles), y: m.f32(angles + 4), z: m.f32(angles + 8) };
    let r = si_math::algebra::matrix_xyz_rotation(&rot);
    for (i, row) in r.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(matrix + 16 * i as u32 + 4 * j as u32, *v);
        }
    }
}

/// The offsets of a mesh's records (`si-hrc`'s, the layout of finding 0015).
use si_hrc::image::geom::mesh as mo;

/// A mesh of the image as `si-model`'s value: vertices with their stars, polygons with
/// their corners, the edge blocks. `sets` is the node's number of texture coordinate sets.
fn mesh_from_image(m: &Mem, a: u32) -> si_model::mesh::Mesh {
    use si_model::mesh::{Edge, Mesh, PolyNode, Polygon, Star, Vertex};
    let f3 = |m: &Mem, at: u32| [m.f32(at), m.f32(at + 4), m.f32(at + 8)];
    let (verts, polys, edges) = (m.u32(a + mo::VERTICES), m.u32(a + mo::POLYGONS), m.u32(a + mo::EDGES));
    let mut mesh = Mesh::default();
    mesh.angle = m.f32(a + mo::ANGLE);
    mesh.cos_angle = m.f32(a + mo::ANGLE_COS);
    mesh.flags = m.u16(a + mo::FLAGS);
    let nv = if verts == 0 { 0 } else { m.i32(a + mo::NB_VERTICES).max(0) as u32 };
    for k in 0..nv {
        let v = verts + mo::VERTEX * k;
        let (star, n) = (m.u32(v + mo::VERTEX_STAR), m.u16(v + mo::VERTEX_NB_STAR) as u32);
        let n = if star == 0 { 0 } else { n };
        mesh.vertices.push(Vertex {
            pos: f3(m, v),
            star: (0..n).map(|e| Star { vertex: m.i32(star + 12 * e), edge: m.i32(star + 12 * e + 4), polygon: m.i32(star + 12 * e + 8) }).collect(),
            flag: m.i16(v + mo::VERTEX_FLAG),
        });
    }
    let np = if polys == 0 { 0 } else { m.i32(a + mo::NB_POLYGONS).max(0) as u32 };
    for k in 0..np {
        let p = polys + mo::POLYGON * k;
        let (nodes, n) = (m.u32(p + mo::POLYGON_NODES), m.u16(p + mo::POLYGON_NB_NODES) as u32);
        let n = if nodes == 0 { 0 } else { n };
        mesh.polygons.push(Polygon {
            normal: f3(m, p),
            nodes: (0..n)
                .map(|c| {
                    let node = nodes + mo::NODE * c;
                    PolyNode { vertex: m.i32(node), edge: m.i32(node + 4), normal: f3(m, node + mo::NODE_NORMAL), uv: [m.f32(node + mo::NODE_UV), m.f32(node + mo::NODE_UV + 4)], color: m.u32(node + mo::NODE_COLOUR), extra_uv: Vec::new() }
                })
                .collect(),
            material: m.i16(p + mo::POLYGON_MATERIAL),
            flag: m.i16(p + mo::POLYGON_FLAG),
        });
    }
    if edges != 0 {
        mesh.edges.push(Edge::default());
        for n in 1..=m.i32(a + 0x1c).max(0) {
            let r = si_hrc::image::geom::edge_at(m, edges, n);
            mesh.edges.push(Edge { v0: m.i32(r), v1: m.i32(r + 4), p0: m.i32(r + 8), p1: m.i32(r + 12), x10: m.i16(r + 0x10), flag: m.i16(r + mo::EDGE_FLAG) });
        }
    }
    mesh
}

/// What a rebuild changed, stored into the mesh of the image: each vertex's star and flag,
/// each polygon's normal and flag, each corner's edge and normal, the edge blocks with
/// their count, the mesh's flags.
fn mesh_to_image(m: &mut Mem, a: u32, mesh: &si_model::mesh::Mesh) {
    let put3 = |m: &mut Mem, at: u32, v: [f32; 3]| {
        for (i, x) in v.iter().enumerate() {
            m.set_f32(at + 4 * i as u32, *x);
        }
    };
    let verts = m.u32(a + mo::VERTICES);
    for (k, v) in mesh.vertices.iter().enumerate() {
        let at = verts + mo::VERTEX * k as u32;
        let (old, old_n) = (m.u32(at + mo::VERTEX_STAR), m.u16(at + mo::VERTEX_NB_STAR) as usize);
        let mut star = old;
        if old_n != v.star.len() || (old == 0 && !v.star.is_empty()) {
            if old != 0 {
                m.free(old);
            }
            star = if v.star.is_empty() { 0 } else { m.calloc(v.star.len() as u32, 12) };
            m.set_u32(at + mo::VERTEX_STAR, star);
            m.set_u16(at + mo::VERTEX_NB_STAR, v.star.len() as u16);
        }
        for (e, st) in v.star.iter().enumerate() {
            let s = star + 12 * e as u32;
            m.set_i32(s, st.vertex);
            m.set_i32(s + 4, st.edge);
            m.set_i32(s + 8, st.polygon);
        }
        m.set_i16(at + mo::VERTEX_FLAG, v.flag);
    }
    let polys = m.u32(a + mo::POLYGONS);
    for (k, p) in mesh.polygons.iter().enumerate() {
        let at = polys + mo::POLYGON * k as u32;
        put3(m, at, p.normal);
        m.set_i16(at + mo::POLYGON_FLAG, p.flag);
        let nodes = m.u32(at + mo::POLYGON_NODES);
        for (c, n) in p.nodes.iter().enumerate() {
            let node = nodes + mo::NODE * c as u32;
            m.set_i32(node + 4, n.edge);
            put3(m, node + mo::NODE_NORMAL, n.normal);
        }
    }
    let ne = mesh.edges.len().saturating_sub(1) as u32;
    let (old_table, old_ne) = (m.u32(a + mo::EDGES), m.i32(a + 0x1c).max(0) as u32);
    let blocks = |n: u32| (n >> 10) + 1;
    let mut table = old_table;
    if !mesh.edges.is_empty() && (old_table == 0 || blocks(old_ne) != blocks(ne)) {
        if old_table != 0 {
            for b in 0..blocks(old_ne) {
                let block = m.u32(old_table + 4 * b);
                if block != 0 {
                    m.free(block);
                }
            }
            m.free(old_table);
        }
        table = m.calloc(blocks(ne), 4);
        for b in 0..blocks(ne) {
            let block = m.calloc(1024, mo::EDGE);
            m.set_u32(table + 4 * b, block);
        }
        m.set_u32(a + mo::EDGES, table);
    }
    for (n, e) in mesh.edges.iter().enumerate().skip(1) {
        let r = si_hrc::image::geom::edge_at(m, table, n as i32);
        m.set_i32(r, e.v0);
        m.set_i32(r + 4, e.v1);
        m.set_i32(r + 8, e.p0);
        m.set_i32(r + 12, e.p1);
        m.set_i16(r + 0x10, e.x10);
        m.set_i16(r + mo::EDGE_FLAG, e.flag);
    }
    if !mesh.edges.is_empty() {
        m.set_i32(a + 0x1c, ne as i32);
    }
    m.set_u16(a + mo::FLAGS, mesh.flags);
}

/// `SI_meshUpdateNodes (0x08458cf8)` and the three normal passes
/// `SI_meshComputesPolygonNormal (0x084599f4)`, `SI_meshComputeHardEdge (0x0845a6e4)`,
/// `SI_meshComputesVertexNormal (0x08459e8c)` for a mesh of the image.
// adapter: not in place
/// `si-model` has the four on a mesh of its own (`mesh_b`, finding 0015): the mesh of the
/// image is copied into one, the routine runs there, and what it made is stored back
/// (stars, polygon and corner normals, the corners' edges, the edge blocks, the flags).
/// The blocks are the original's sizes (a star of 12 bytes an entry, a table of block
/// pointers, 1,024 edges of 20 bytes a block) but **their order in the heap is this
/// adapter's**, and what the routine leaves beside them is not stored: the wire-frame
/// lines (+0x24, +0x28), the two lists of selected polygons (+0x30, +0x34). A status line
/// of the rebuild is counted, not shown. Answers 1, or 0 when a polygon could not be
/// filed (`SI_meshUpdateNodes`).
pub fn mesh_rebuild(m: &mut Mem, sc: &mut Scene, routine: u32, mesh_at: u32, arg: u32) -> u32 {
    use si_model::mesh_b;
    if mesh_at == 0 {
        return 0;
    }
    let mut mesh = mesh_from_image(m, mesh_at);
    let mut answer = 1;
    match routine {
        0x0845_8cf8 => {
            let mut env = mesh_b::MeshEnv { model_name: None, index3: None, messages: Vec::new() };
            let node = m[si_hrc::image::globals::CURR_MDL];
            if node != 0 {
                let name = m.u32(node + 4);
                env.model_name = (name != 0).then(|| m.cstr(name));
            }
            sc.contents.meshes_rebuilt += 1;
            if !mesh_b::update_nodes(&mut mesh, arg as i16 != 0, &mut env) {
                sc.contents.polygons_not_filed += 1;
                answer = 0;
            }
            for text in env.messages {
                sc.contents.status.push((2, text.into_bytes()));
            }
        }
        0x0845_99f4 => mesh_b::computes_polygon_normal(&mut mesh, arg as i32),
        0x0845_a6e4 => mesh_b::compute_hard_edge(&mut mesh, arg as i32),
        _ => mesh_b::computes_vertex_normal(&mut mesh, arg as i32),
    }
    mesh_to_image(m, mesh_at, &mesh);
    answer
}

// ------------------------------------------------------- the routines around the readers

/// [soft 0x083f6238 si_filenameCleanup]: a copy (`strdup`) of `name` without the directory
/// `base` in front: when `base` is not null, is shorter than `name`, begins `name`, and a
/// `/` follows it there, the copy starts behind that `/`.
pub fn filename_cleanup(m: &mut Mem, name: u32, base: u32) -> u32 {
    let mut from = name;
    if base != 0 {
        let (n, b) = (m.cstr(name), m.cstr(base));
        if b.len() < n.len() && n.starts_with(&b) && n[b.len()] == b'/' {
            from = name + b.len() as u32 + 1;
        }
    }
    m.strdup(from)
}

/// `SI_fdmodelBinaryRead (0x083ec740)`, `si-hrc`'s, named for [`si_hrc::image::Outside::stands_in`]
/// (the vectors of [`omodel_binary_read`] were recorded with it as a stand-in).
pub const SI_FDMODEL_BINARY_READ: si_hrc::image::Routine = si_hrc::image::Routine { name: "SI_fdmodelBinaryRead", addr: 0x083e_c740 };

/// [soft 0x083ec5a4 SI_omodelBinaryRead]: the hierarchy of the model file `name` + `.hrc`.
///
/// The path is printed into 256 bytes of stack; the file is opened unless files come from
/// a pipe (`SI_pipedFiles`, then `SI_pipe` is the stream); no stream, null. The hierarchy
/// is read by `SI_fdmodelBinaryRead(stream, model, mode)`. For a hierarchy that was read
/// with `mode` 0 the block at the root's +0 is freed if there is one and becomes the
/// file's name relative to `SI_objectPath` ([`filename_cleanup`]). The stream is closed
/// unless it is the pipe.
///
/// A path of exactly 256 bytes puts its terminator on the word behind the buffer, a saved
/// register the routine does not read again (emulated: such a case runs on as any other);
/// the port's frame has a word there too. A longer one runs over the frame pointer and
/// the return address: the port records a fault and answers null.
pub fn omodel_binary_read(m: &mut Mem, o: &mut dyn si_hrc::image::Outside, name: u32, model: u32, mode: i16) -> u32 {
    const FRAME: u32 = 0x104;
    let path = m.local(FRAME);
    let mut text = m.cstr(name);
    text.extend_from_slice(b".hrc");
    if text.len() > 0x100 {
        m.record_fault(path + 0x101, text.len() as u32 - 0x100, si_mem::Access::Write);
        m.release(FRAME);
        return 0;
    }
    m.put_cstr(path, &text);
    let mut file = m[SI_PIPE];
    if m[SI_PIPED_FILES] == 0 {
        file = o.fopen(m, path, S_RB_HRC);
    }
    let mut root = 0;
    if file != 0 {
        root = if o.stands_in(&SI_FDMODEL_BINARY_READ) {
            o.call(m, &SI_FDMODEL_BINARY_READ, &[file, model, mode as i32 as u32])
        } else {
            si_hrc::image::node::fdmodel_binary_read(m, o, file, model, mode)
        };
        if root != 0 && mode == 0 {
            let old = m.u32(root);
            if old != 0 {
                m.free(old);
                m.set_u32(root, 0);
            }
            let base = m[SI_OBJECT_PATH];
            let copy = filename_cleanup(m, name, base);
            m.set_u32(root, copy);
        }
        if m[SI_PIPED_FILES] == 0 {
            o.fclose(m, file);
        }
    }
    m.release(FRAME);
    root
}

/// [soft 0x0810fdd4 SI_elemdictReadModel]: `SI_omodelBinaryRead(path, null, 0)`.
pub fn elemdict_read_model(m: &mut Mem, o: &mut dyn si_hrc::image::Outside, path: u32) -> u32 {
    omodel_binary_read(m, o, path, 0, 0)
}

/// [soft 0x0810fef0 SI_elemdictReadMaterial]: `SI_materialReadBinaryFile(path)`.
pub fn elemdict_read_material(m: &mut Mem, o: &mut dyn si_hrc::image::Outside, path: u32) -> u32 {
    si_hrc::image::matio::material_read_binary_file(m, o, path).addr()
}

/// [soft 0x0810ff08 SI_elemdictReadTexture2D]: `SI_textureReadBinaryFile(path)`.
pub fn elemdict_read_texture2d(m: &mut Mem, o: &mut dyn si_hrc::image::Outside, path: u32) -> u32 {
    si_hrc::image::matio::texture_read_binary_file(m, o, path).addr()
}

/// [soft 0x0810ff20 SI_elemdictReadTexture3D]: `SI_texture3DReadBinaryFile(path)`.
pub fn elemdict_read_texture3d(m: &mut Mem, o: &mut dyn si_hrc::image::Outside, path: u32) -> u32 {
    si_hrc::image::matio::texture3d_read_binary_file(m, o, path).addr()
}

/// `SI_modelUpdateSonState (0x084662f8)`, as [`model_set_state`] calls it.
pub const SI_MODEL_UPDATE_SON_STATE: u32 = 0x0846_62f8;

/// [soft 0x084638a8 SI_modelSetState]: bits 1 to 3 of the node's two update words (+0x0a
/// as a byte, +0x3fa as a word) are set, the three vectors are stored as the node's local
/// scaling (+0x100), rotation angles (+0x10c) and translation (+0x158), and the node is
/// updated under its father (`SI_modelUpdateSonState(father, node, 1)`).
pub fn model_set_state<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, node: u32, scaling: u32, rotation: u32, translation: u32) {
    let low = m.u8(node + 0x0a) | 0xe;
    m.set_u8(node + 0x0a, low);
    let word = m.u16(node + 0x3fa) | 0xe;
    m.set_u16(node + 0x3fa, word);
    for (to, from) in [(0x100, scaling), (0x10c, rotation), (0x158, translation)] {
        for k in 0..3 {
            let v = m.u32(from + 4 * k);
            m.set_u32(node + to + 4 * k, v);
        }
    }
    let father = m.u32(node + 0x2bc);
    o.call_addr(m, SI_MODEL_UPDATE_SON_STATE, &[father, node, 1]);
}

// --------------------------------------------------------------- what the loader calls

/// Which of the three readers of `b_matio.c`.
#[derive(Clone, Copy, PartialEq)]
pub enum Matter {
    Material,
    Texture2D,
    Texture3D,
}

/// The nodes of a hierarchy of the image in the order the loader numbers them: a node,
/// the nodes under its first son, then its next brother.
pub fn hierarchy(m: &Mem, root: u32) -> Vec<u32> {
    fn walk(m: &Mem, first: u32, out: &mut Vec<u32>, depth: u32) {
        let mut n = first;
        while n != 0 && out.len() < 1_000_000 {
            out.push(n);
            let son = m.u32(n + 0x2c4);
            if son != 0 && depth < 10_000 {
                walk(m, son, out, depth + 1);
            }
            n = m.u32(n + 0x2c0);
        }
    }
    let mut out = Vec::new();
    walk(m, root, &mut out, 0);
    out
}

/// The scene dictionary, for the routines here that take none.
pub fn dict(m: &Mem) -> Ptr32<si_types::dict::ElemDict> {
    current_dict(m)
}

/// A fresh table of streams for one reader call (the tables keep every stream's bytes).
fn prog<'a>(sc: &'a mut Scene, un: &'a mut dyn Unported, fs: &'a dyn FileSystem) -> Prog<'a> {
    sc.contents.hrc = si_hrc::image::Files::new();
    sc.contents.eio = si_elemio::image::host::Files::new();
    Prog { sc, un, fs: Some(fs), db: None }
}

/// One of the three readers of `b_matio.c` on the file the search left in
/// `SI_dBWorkFname`; the record it answers stands for the value `handle`. `path` is the
/// file's, for the report.
pub fn read_matter(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, which: Matter, handle: u32, path: &[u8]) -> u32 {
    let name = crate::globals::SI_DB_WORK_FNAME.addr();
    let mut o = prog(sc, un, fs);
    let (record, what) = match which {
        Matter::Material => (elemdict_read_material(m, &mut o, name), "MATERIALS"),
        Matter::Texture2D => (elemdict_read_texture2d(m, &mut o, name), "TEXTURES2D"),
        Matter::Texture3D => (elemdict_read_texture3d(m, &mut o, name), "TEXTURES3D"),
    };
    if record == 0 {
        sc.contents.disagreed.push((what, path.to_vec()));
    } else {
        *sc.contents.records.entry(what).or_default() += 1;
        sc.tie(handle, record);
    }
    record
}

/// `SI_elemdictReadModel(&SI_dBWorkFname)`: the hierarchy of the model file in the image,
/// paired node for node with the value reader's (`tree` of [`Scene::models`]). A file
/// whose read reached a reader that is not on the image, or whose hierarchy does not pair,
/// is counted and its nodes are left alone (the elements then keep the values' handles).
pub fn read_model(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, tree: u32, path: &[u8]) -> u32 {
    let name = crate::globals::SI_DB_WORK_FNAME.addr();
    sc.contents.missing_reader = None;
    let root = {
        let mut o = prog(sc, un, fs);
        elemdict_read_model(m, &mut o, name)
    };
    if let Some(reader) = sc.contents.missing_reader.take() {
        *sc.contents.models_unread.entry(reader).or_default() += 1;
        return 0;
    }
    if root == 0 {
        sc.contents.disagreed.push(("MODELS", path.to_vec()));
        return 0;
    }
    let nodes = hierarchy(m, root);
    let Some(t) = sc.models.get_mut(tree) else { return 0 };
    let names_agree = nodes.len() == t.names.len() && nodes.iter().zip(&t.names).all(|(&n, name)| m.u32(n + 4) != 0 && m.cstr(m.u32(n + 4)) == *name);
    if !names_agree {
        sc.contents.models_unpaired += 1;
        return 0;
    }
    *sc.contents.records.entry("MODELS (files)").or_default() += 1;
    *sc.contents.records.entry("MODELS (nodes)").or_default() += nodes.len();
    t.image = nodes;
    let handle = t.handles[0];
    sc.tie(handle, root);
    root
}

/// What `elemdictSetDataFromHierarchy` stores into a node an element claims, on the node
/// of the image: the element's index (+0x354) and, with `SI_defaultGMatte`, the garbage
/// matte word (+0x3f8). The node then stands for the value `handle`.
pub fn claim_node(m: &mut Mem, sc: &mut Scene, handle: u32, record: u32, index: i32, garbage_matte: bool) {
    if record == 0 {
        return;
    }
    m.set_i32(record + 0x354, index);
    if garbage_matte {
        m.set_i16(record + 0x3f8, 1);
    }
    sc.tie(handle, record);
}

/// `SI_elemdictReadLight(&SI_dBWorkFnameExt)`: the light in the image (its reader has
/// asked the graph list for the light's node); it stands for the value `handle`.
pub fn read_light(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, handle: u32, path: &[u8]) -> u32 {
    let name = crate::globals::SI_DB_WORK_FNAME_EXT.addr();
    let run = {
        let mut o = prog(sc, un, fs);
        si_elemio::image::elemdict::elemdict_read_light(m, &mut o, name)
    };
    match run {
        Ok(0) => {
            sc.contents.disagreed.push(("LIGHTS", path.to_vec()));
            0
        }
        Ok(light) => {
            *sc.contents.records.entry("LIGHTS").or_default() += 1;
            sc.contents.lights_read.push(light);
            sc.tie(handle, light);
            light
        }
        Err(e) => {
            sc.contents.no_return.push((format!("{e:?}"), path.to_vec()));
            0
        }
    }
}

/// `SI_elemdictReadAnimations(dictionary, &SI_dBWorkFnameExt)`: the animation or struct
/// key file in the image (its `SI_HierAnim`, the curves of its items, its two action
/// lists); the record stands for the value `handle`. The curves' elements that the
/// expansion makes keep the value's items (finding 0018, section 17).
pub fn read_animations(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, ed: Ptr32<si_types::dict::ElemDict>, handle: u32, path: &[u8]) -> u32 {
    let name = crate::globals::SI_DB_WORK_FNAME_EXT.addr();
    let run = {
        let mut o = prog(sc, un, fs);
        si_elemio::image::anim::elemdict_read_animations(m, &mut o, ed.addr(), name)
    };
    match run {
        Ok(0) => {
            sc.contents.disagreed.push(("ANIMATION", path.to_vec()));
            0
        }
        Ok(anim) => {
            *sc.contents.records.entry("ANIMATION (files)").or_default() += 1;
            sc.tie(handle, anim);
            anim
        }
        Err(e) => {
            sc.contents.no_return.push((format!("{e:?}"), path.to_vec()));
            0
        }
    }
}

/// `SCENE_NO_SHAPES_ON_IMAGE=1`: the load reads shape files into values only
/// ([`Scene::shapes`]), as before `SI_shapeBinaryRead` was on the image; the model's node in
/// the image then keeps the one definition its model file gave it.
pub(crate) fn shapes_on_image() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_SHAPES_ON_IMAGE").is_none())
}

/// [`Prog`] as the shape reader calls out: the routines it calls that the loader's dispatch
/// does not route (`SI_patchUpdateSurface`, and the copies that make a current definition
/// for a file without one: `SI_meshCopy`, `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`) answered by
/// their ports here, everything else by the loader's dispatch, so that nothing changes for
/// the other readers.
struct ShapeSide<'a, 'b>(&'a mut Prog<'b>);
impl si_hrc::image::Outside for ShapeSide<'_, '_> {
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        si_hrc::image::Outside::fopen(self.0, m, path, mode)
    }
    fn fclose(&mut self, m: &mut Mem, file: u32) -> i32 {
        si_hrc::image::Outside::fclose(self.0, m, file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        si_hrc::image::Outside::fread(self.0, m, ptr, size, n, file)
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        si_hrc::image::Outside::fgetc(self.0, m, file)
    }
    fn rewind(&mut self, m: &mut Mem, file: u32) {
        si_hrc::image::Outside::rewind(self.0, m, file)
    }
    fn bytes_left(&mut self, m: &mut Mem, file: u32) -> u64 {
        si_hrc::image::Outside::bytes_left(self.0, m, file)
    }
    fn call(&mut self, m: &mut Mem, routine: &'static si_hrc::image::Routine, args: &[u32]) -> u32 {
        use si_hrc::image::shape as sh;
        let w = |k: usize| args.get(k).copied().unwrap_or(0);
        let answer = match routine.addr {
            a if a == sh::SI_PATCH_UPDATE_SURFACE.addr => {
                si_spline::patch_image::patch_update_surface(m, Ptr32::new(w(0)));
                Some(0)
            }
            a if a == sh::SI_MESH_COPY.addr => Some(si_model::mesh_image::obj::mesh_copy(m, &mut *self.0, w(0))),
            a if a == sh::SI_NURBS_CRV_COPY.addr => Some(si_nurbs::image::crv_copy(m, w(0))),
            a if a == sh::SI_NURBS_SRF_COPY.addr => Some(si_nurbs::image::srf_copy(m, &mut crate::envside::NurbsSide(&mut *self.0), w(0))),
            _ => None,
        };
        match answer {
            Some(v) => {
                self.0.count(routine.name, true);
                v
            }
            None => si_hrc::image::Outside::call(self.0, m, routine, args),
        }
    }
}

/// `SI_elemdictReadShape(&SI_dBWorkFname, node)` [soft 0x08110128 SI_elemdictReadShape],
/// which is `SI_shapeBinaryRead(name, node)` ([`si_hrc::image::shape::shape_binary_read`]):
/// the shapes of the file added to the node's definitions in the image. Nothing when the
/// node is not a record of the image (counted) or under `SCENE_NO_SHAPES_ON_IMAGE=1`.
pub fn read_shape(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, node: u32, path: &[u8]) -> i32 {
    if !shapes_on_image() {
        return 0;
    }
    if node == 0 || si_mem::handle::Kind::of(node).is_some() {
        *sc.contents.not_in_image.entry("SHAPES (the model's node is not in the image)").or_default() += 1;
        return 0;
    }
    let name = crate::globals::SI_DB_WORK_FNAME.addr();
    sc.contents.missing_reader = None;
    let answer = {
        let mut o = prog(sc, un, fs);
        si_hrc::image::shape::shape_binary_read(m, &mut ShapeSide(&mut o), name, node) as i32
    };
    if let Some(reader) = sc.contents.missing_reader.take() {
        *sc.contents.models_unread.entry(reader).or_default() += 1;
    }
    if answer == 0 {
        sc.contents.disagreed.push(("SHAPES", path.to_vec()));
    } else {
        *sc.contents.records.entry("SHAPES").or_default() += 1;
    }
    answer
}

/// `SI_elemdictReadLattice(&SI_dBWorkFnameExt, branch, node)`: the lattices of the file
/// added to the node's list of node lattices (+0x50) or of branch lattices (+0x58) in the
/// image; the element's object is the list's first lattice, which stands for the value
/// `handle`. Null when the node is not a record of the image, when the reader answered
/// nothing or the list stayed empty (the caller then reports as it does for the value).
pub fn read_lattice(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, handle: u32, node: u32, branch: bool, path: &[u8]) -> u32 {
    if node == 0 || si_mem::handle::Kind::of(node).is_some() {
        *sc.contents.not_in_image.entry("LATTICES (the model's node is not in the image)").or_default() += 1;
        return 0;
    }
    let name = crate::globals::SI_DB_WORK_FNAME_EXT.addr();
    let run = {
        let mut o = prog(sc, un, fs);
        si_elemio::image::elemdict::elemdict_read_lattice(m, &mut o, name, branch as i16, node)
    };
    match run {
        Ok(_) => {
            let list = m.u32(node + if branch { 0x58 } else { 0x50 });
            let first = if list == 0 { 0 } else { m.u32(list) };
            if first == 0 {
                sc.contents.disagreed.push(("LATTICES", path.to_vec()));
            } else {
                *sc.contents.records.entry("LATTICES").or_default() += 1;
                sc.tie(handle, first);
            }
            first
        }
        Err(e) => {
            sc.contents.no_return.push((format!("{e:?}"), path.to_vec()));
            0
        }
    }
}

/// `SI_elemdictReadCamera(&SI_dBWorkFnameExt)`: the file read into the scene's camera of
/// the image (the third word of the process record).
pub fn read_camera(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, fs: &dyn FileSystem, path: &[u8]) -> u32 {
    let name = crate::globals::SI_DB_WORK_FNAME_EXT.addr();
    let run = {
        let mut o = prog(sc, un, fs);
        si_elemio::image::elemdict::elemdict_read_camera(m, &mut o, name)
    };
    match run {
        Ok(0) => {
            sc.contents.disagreed.push(("CAMERAS", path.to_vec()));
            0
        }
        Ok(camera) => {
            *sc.contents.records.entry("CAMERAS").or_default() += 1;
            camera
        }
        Err(e) => {
            sc.contents.no_return.push((format!("{e:?}"), path.to_vec()));
            0
        }
    }
}

/// The scene's graph list and camera as the program has them once it has started.
// adapter: not in place
/// Which routine of the start-up makes them was not read. Done here, with the ports that
/// exist: the array of lists with its first list (`SI_grafListCreate`), a camera
/// (`SI_newCamera("Camera", 1)`: the name is this adapter's), stored as the process
/// record's third word, and the camera's two nodes (`SI_graflistAddData(list, camera, 4,
/// 0)`). The camera stands for the value [`Scene::camera`].
pub fn start_graph(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported) {
    si_view::graflist::graf_list_create(m);
    // the display's two words behind a new node's wire colour: the loader's assumption
    // about the display (`load::WIRE_DEFAULT`), as `SI_grafnodeDefColor` answers it
    m[si_view::globals::SI_SELECT7WM] = sc.wire.select7_mask as u16;
    m[si_view::globals::SI_COLOR1] = sc.wire.color1 as u16;
    m.put_cstr(S_HRC_FORMAT, b"%s.hrc");
    m.put_cstr(S_RB_HRC, b"rb");
    let camera = {
        let mut o = Prog { sc, un, fs: None, db: None };
        let name = m.local(8);
        m.put_cstr(name, b"Camera");
        let camera = si_elemio::image::camfct::new_camera(m, &mut o, name, 1);
        m.release(8);
        let proc = m[si_dict::globals::SI_PROC_DATA];
        m.set_u32(proc.addr() + 8, camera);
        let list = scene_graf_list(m);
        si_view::graflist::graflist_add_data(m, &mut si_view::Wired::new(ViewSide(&mut o)), list, Ptr32::new(camera), 4, 0);
        camera
    };
    let handle = sc.camera;
    sc.tie(handle, camera);
}

/// The scene's graph list: `SI_grafLists` plus 0x24 times the index in the process record.
pub fn scene_graf_list(m: &Mem) -> Ptr32<si_types::scene::GrafList> {
    let proc = m[si_view::globals::SI_PROC_DATA];
    let index = m[proc].list;
    m[si_view::globals::SI_GRAF_LISTS].add(index)
}

/// What the graph list's builder found and made, beside the stand-in graph.
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphPairs {
    /// Elements whose node in the image and node of the stand-in were paired.
    pub paired: usize,
    /// Elements with a node in the image only, and with a stand-in node only.
    pub image_only: usize,
    pub standin_only: usize,
}

/// An object made for an element whose file is missing (`SI_elemdictReplaceMissingElement`
/// records these as [`crate::object::Object::Foreign`]), in the image, for the chapters
/// whose objects the graph list's builder reads or writes.
// adapter: not in place
/// The original copies the default material or texture, makes a light with
/// `SI_newLight`, or a cube model. Here: a material, 2D or 3D texture from its allocator
/// (`SI_materialAllocate`, `SI_textureAllocate`, `SI_texture3DAllocate`: not the copy), a
/// light from `SI_newLight` with the type the loader asked for, a model node from
/// `SI_modelAllocate` with the element's name, no geometry and no father (the cube's
/// parentship, `SI_modelCreateParentship`, is not made). Null for anything else.
pub fn place_foreign(m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, chap: i32, e: Ptr32<si_types::dict::Elem>) -> u32 {
    let cid = crate::world::cid(m);
    let handle = sc.obj(m[e].data);
    let Some(crate::object::Object::Foreign { made_by, .. }) = sc.objects.get(handle) else { return 0 };
    let made_by: &'static str = made_by;
    let record = if chap == cid.mat {
        si_hrc::image::material::material_allocate(m).addr()
    } else if chap == cid.t2d {
        si_hrc::image::texture::texture_allocate(m).addr()
    } else if chap == cid.t3d {
        si_hrc::image::texture::texture3d_allocate(m).addr()
    } else if chap == cid.lig {
        let kind = if made_by.contains("(2)") { 2 } else { 1 };
        let mut o = Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
        si_elemio::image::lights::new_light(m, &mut o, kind)
    } else if chap == cid.model {
        let name = m[m[e].node].name;
        si_hrc::image::node::model_allocate(m, name.addr()).addr()
    } else {
        0
    };
    if record != 0 {
        *sc.contents.not_in_image.entry(made_by).or_default() += 1;
        sc.tie(handle, record);
        m[e].data = Ptr32::new(record);
    }
    record
}

/// `SI_graflistBuildFromElemDict (0x08109ec8)`, `si-view`'s port, where the loader calls
/// it: every element of this load gets its node of the scene's graph list in the image.
///
/// The stand-in graph of the stores ([`Scene::graf`]) was built just before, for the
/// saver: its nodes are taken off the elements first and paired with the image's
/// afterwards ([`Contents::graf_image`]). An element of the chapters whose objects the
/// builder reads that has no record in the image gets one ([`place_foreign`]).
pub fn build_graph(m: &mut Mem, sc: &mut Scene, ed: Ptr32<si_types::dict::ElemDict>, scene: bool, un: &mut dyn Unported) -> GraphPairs {
    let cid = crate::world::cid(m);
    let mut standin: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    for chap in 0..m[ed].nb_lists {
        let l = crate::world::list(m, ed, chap);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            let h = crate::world::graf_of(m[e].aux_data);
            if h != 0 {
                standin.insert((chap, i), h);
                m[e].aux_data = Ptr32::NULL;
            }
            let needs = [cid.model, cid.mat, cid.t2d, cid.t3d, cid.lig].contains(&chap);
            if needs && m[e].new_flag == 1 && obj_of(m[e].data) != 0 {
                place_foreign(m, sc, un, chap, e);
            }
        }
    }
    {
        let mut o = Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
        let mut view = si_view::Wired::new(ViewSide(&mut o));
        // (the waves' nodes, kind 0xb, are the wave reader's: `wave.rs` adds each wave to
        // the list as it reads it, as the original does; the builder only looks them up)
        si_view::db2scn::graflist_build_from_elem_dict(m, &mut view, ed, si_dict::globals::SI_RD.cast(), si_dict::globals::SI_CID.cast(), scene as i16);
    }
    sc.contents.standin = standin.clone();
    let mut pairs = GraphPairs::default();
    for chap in 0..m[ed].nb_lists {
        let l = crate::world::list(m, ed, chap);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            let real = m[e].aux_data.addr();
            match (real, standin.get(&(chap, i)).copied()) {
                (0, None) => {}
                (0, Some(_)) => pairs.standin_only += 1,
                (_, None) => pairs.image_only += 1,
                (r, Some(h)) => {
                    pairs.paired += 1;
                    sc.contents.graf_image.insert(h, r);
                    sc.contents.graf_handle.insert(r, h);
                }
            }
        }
    }
    pairs
}

// adapter: not in place
/// The elements the load's material conversion ([`crate::matconv`]) made, in the store
/// form. The conversion gives a node's own materials and textures (and their shaders)
/// elements of their own: each element's object is the node's record in the image, and
/// its second pointer the node `SI_graflistAddData` made for it in the image's list. No
/// file was read into those records, so no value stood for them and the stores held null.
///
/// Here, after the conversion: an object of the MATERIALS, TEXTURES2D or TEXTURES3D chapter
/// that is a record of the image without a value is read back into one (`si-hrc`'s
/// `image::values`, the record as it is) and tied to it, as [`read_matter`] ties what a file
/// read made; a shader's record gets an object of another package
/// ([`crate::object::Object::Foreign`]), as a shader read from its file does. Such an
/// element's node of the image's list without a stand-in gets one ([`Scene::graf`]): the
/// node as the image holds it, its object as the store form numbers it, no function and no
/// links; the two are paired as [`build_graph`] pairs them.
pub fn tie_converted(m: &Mem, sc: &mut Scene, ed: Ptr32<si_types::dict::ElemDict>) {
    use crate::object::Object;
    use si_hrc::image::values::{material_of, texture3d_of, texture_of};
    let cid = crate::world::cid(m);
    let shaders = [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon];
    for chap in 0..m[ed].nb_lists {
        if ![cid.mat, cid.t2d, cid.t3d].contains(&chap) && !shaders.contains(&chap) {
            continue;
        }
        let l = crate::world::list(m, ed, chap);
        for i in 0..m[l].nb_elems {
            let e = m[l].elems.add(i);
            let record = m[e].data.addr();
            if record != 0 && sc.obj(m[e].data) == 0 && si_mem::handle::Kind::of(record).is_none() {
                let value = if chap == cid.mat {
                    Object::Material(Box::new(material_of(m, record)))
                } else if chap == cid.t2d {
                    Object::Texture2D(Box::new(texture_of(m, record)))
                } else if chap == cid.t3d {
                    Object::Texture3D(Box::new(texture3d_of(m, record)))
                } else {
                    Object::Foreign { made_by: "SI_edAddUDF (the load's material conversion)", path: Vec::new() }
                };
                let h = sc.objects.alloc(value);
                sc.tie(h, record);
            }
            let node = m[e].aux_data.addr();
            if node != 0 && sc.graf_handle(m[e].aux_data) == 0 && si_mem::handle::Kind::of(node).is_none() {
                let mut n: si_types::scene::GrafNode = m.read(Ptr32::new(node));
                n.data = Ptr32::new(sc.obj(n.data));
                n.func = Ptr32::NULL;
                n.next = Ptr32::NULL;
                n.prev = Ptr32::NULL;
                let h = sc.graf.alloc(n);
                sc.contents.graf_image.insert(h, node);
                sc.contents.graf_handle.insert(node, h);
            }
        }
    }
}
