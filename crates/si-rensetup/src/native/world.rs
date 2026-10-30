// SPDX-License-Identifier: MIT
//! The program as far as it is on the image: one outside world in which every call by
//! address goes to the port that exists for it, whichever crate has it, and every other
//! call is a hook (finding 0023, section 18).
//!
//! Nothing in this file reproduces a function of the original.
//!
//! [`World`] implements this package's [`Outside`] and, through the four small wrappers
//! at the end, the traits of the same shape that `si-view`, `si-model`, `si-dict` and
//! `si-render` call out through, so that a routine of one crate reaches a routine of
//! another by the original's address:
//!
//! | Callee | Port |
//! |---|---|
//! | `instance.c` (three routines other packages call), `SI_modelAllocate`, `SI_modelAlgoTypeCopy`, the inverse kinematics record's copy and disposal, the routines of [`super::edtools`], the render window ([`super::renu_display`]) | this package |
//! | the dictionary's look-ups, relations and hierarchy | `si-dict` |
//! | the name record's allocation and full name | `si-scene` (`dbnode`) |
//! | `SI_modelUpdateSonState`, `SI_modelNextNode`, `SI_modelNextSon`, `SI_modelChangeName` | `si-model` (`model_image`) |
//! | `SI_findGlData`, `SI_graflistAddData`, `SI_schematicNoRefresh` and the four routines of `si-view`'s own seam | `si-view` (run under `si_view::Wired`) |
//! | `SI_image` | `si-render` (`rnd_main::image` under `adapter::Render`) |
//! | `SI_sceneUpdateItems` | `si-motion` (through [`super::motion`]); `SI_activationFcvCheckTime` too |
//! | `SI_edGetElementsGrafNodeInfo` and the update's eight look-ups of `edtools.c` | `si-scene` (`edtools`) |
//! | `SI_malloc`, `SI_realloc` | `si-mem` |
//! | the start-up's `SI_metaballInit` | `si-meta` (lic1) |
//! | the start-up's `SI_matterInit`, with `si_lightInitialize` (`si-elemio`), `SI_chunkAllocate` (`si-render`) and the five `matter.c` routines it calls (this package's [`super::matter`]) | `si-matter` (lic2) |
//! | the start-up's `SI_cusInit` | `si-custom` (lic2) |
//!
//! The two lic2 start-up ports read texts and pointers of the binary's `.data` from the image;
//! [`place_start_data`] puts them in first ([`MATTER_INIT_TEXTS`], [`CUS_INIT_TEXTS`] and the
//! pointer lists).
//!
//! Everything else is a hook: [`Watch::hook`] may answer it; otherwise it answers 0. Both
//! kinds of call are counted by address ([`World::ported`], [`World::hooks`]).
//!
//! Hooks that are answered here as the original answers them **on the path a loaded
//! scene takes**, each with the reason at its line: the three `SI_edAdd…` routines of
//! materials and textures (1 while `SI_addInDictionary` is 0).

use super::outside::*;
use super::{edtools, instance, invkin, model_b, pre_render, preview, renu_display, renu_filepath, renu_subregion, rens_main, sys_render};
use si_mem::{FnPtr, Mem, Ptr32};
use std::collections::BTreeMap;

/// What `SI_image` did, for whoever embeds the world.
#[derive(Clone, Debug, Default)]
pub struct ImageReport {
    /// The five arguments: camera, two words of the graf list, lights, roots.
    pub args: [u32; 5],
    /// What it answered.
    pub status: u32,
    /// Triangles filed by the last `SI_update` (`SI_stats` +0x00).
    pub triangles: i32,
    /// What the renderer's set-up said and could not do.
    pub notes: Vec<String>,
    /// The pictures handed to the picture writer: name, lines, bytes of the first line's
    /// red channel, closed.
    pub pictures: Vec<(Vec<u8>, usize, usize, bool)>,
}

/// The embedding program's view of a run.
pub trait Watch {
    /// A render command has reached `SI_image (0x08257a70)`; the renderer has not run yet.
    fn before_image(&mut self, _m: &mut Mem, _args: &[u32; 5]) {}
    /// `SI_image` has answered.
    fn after_image(&mut self, _m: &mut Mem, _report: &ImageReport) {}
    /// A callee nobody has on the image. `Some(answer)` answers it; `None` lets it answer 0.
    fn hook(&mut self, _m: &mut Mem, _f: FnPtr, _args: &[u32]) -> Option<u32> {
        None
    }
}

/// The picture files the renderer may ask for: the program's default picture
/// (`<SI_resdir>/noIcon.pic`), if the embedding program has it, and, when `files` is set,
/// a texture's picture from the file system by the name `SI_readTextureFile` builds
/// (finding 0012, section 13): `<name>.pic` for a name that starts with `/`, else
/// `<SI_texturePath>/<name>.pic` (finding 0023, section 22).
#[derive(Default)]
pub struct Pictures {
    /// The bytes of `noIcon.pic`.
    pub default: Option<Vec<u8>>,
    /// Texture pictures are read from the file system.
    pub files: bool,
    /// What `SI_texturePath` points to.
    pub texture_path: Vec<u8>,
    /// What was asked for: the name, and whether it was the default picture.
    pub asked: Vec<(Vec<u8>, bool)>,
}

impl si_render::frame::PictureFiles for Pictures {
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        use std::os::unix::ffi::OsStrExt;
        self.asked.push((name.to_vec(), default));
        if default {
            return self.default.clone();
        }
        if !self.files {
            return None;
        }
        let mut p = Vec::new();
        if name.first() != Some(&b'/') {
            p.extend_from_slice(&self.texture_path);
            p.push(b'/');
        }
        p.extend_from_slice(name);
        p.extend_from_slice(b".pic");
        std::fs::read(std::ffi::OsStr::from_bytes(&p)).ok()
    }
}

/// Watches nothing.
#[derive(Default)]
pub struct NoWatch;
impl Watch for NoWatch {}

/// The outside world of a program run: see the head of this file.
pub struct World<W: Watch> {
    pub watch: W,
    /// Whether `SI_image` is `si-render`'s port (else it is a hook like any other).
    pub render: bool,
    /// Calls that went to a port, by the callee's address.
    pub ported: BTreeMap<u32, usize>,
    /// Calls nobody has a port for, by the callee's address.
    pub hooks: BTreeMap<u32, usize>,
    /// What each call of `SI_image` did.
    pub images: Vec<ImageReport>,
    /// The picture files the renderer is given.
    pub pictures: Pictures,
    /// The innermost callee during whose run the image recorded its first fault.
    pub fault_in: Option<u32>,
    /// Whether `SI_sceneUpdateItems (0x082221a0)` is answered by `si-motion`'s port
    /// ([`super::motion`]); else it is a hook like any other.
    pub motion: bool,
    /// Whether `SI_edGetElementsGrafNodeInfo (0x08104acc)` is answered by `si-scene`'s
    /// routine on the image; else it is a hook.
    pub graf_info: bool,
    /// What the scene's update called out of `si-motion`, by the host method's name.
    pub motion_calls: super::motion::Calls,
    /// Whether [`World::more`] answers (else its routines are hooks, as before routing3).
    pub routing: bool,
    routing_skip: Vec<u32>,
}

impl<W: Watch> World<W> {
    pub fn new(watch: W) -> World<W> {
        World { watch, render: true, ported: BTreeMap::new(), hooks: BTreeMap::new(), images: Vec::new(), pictures: Pictures::default(), fault_in: None, motion: true, graf_info: true, motion_calls: Default::default(), routing: std::env::var_os("RENSETUP_NO_ROUTING3").is_none(), routing_skip: std::env::var("RENSETUP_ROUTING3_SKIP").map(|v| v.split(',').filter_map(|t| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect()).unwrap_or_default() }
    }

    /// `si-view`'s routines run with its seam closed and this world behind it.
    fn view(&mut self) -> si_view::Wired<ViewSide<'_, Self>> {
        si_view::Wired::new(ViewSide(self))
    }

    /// `SI_image (0x08257a70)`: `si-render`'s port, with this world as everything it
    /// calls outside its own package and [`World::pictures`] as its picture files.
    fn image(&mut self, m: &mut Mem, a: [u32; 5]) -> u32 {
        self.watch.before_image(m, &a);
        let mut files = std::mem::take(&mut self.pictures);
        let mut report = ImageReport { args: a, ..ImageReport::default() };
        {
            let mut rest = RenderSide(self);
            let mut r = si_render::adapter::Render::new(&mut rest, &mut files);
            report.status = si_render::rnd_main::image(m, &mut r, a[0], a[1], a[2], a[3], a[4]);
            report.triangles = r.p.g.bins.accepted;
            report.notes = std::mem::take(&mut r.notes);
            report.pictures = r.pictures.iter().map(|p| (p.name.clone(), p.lines.len(), p.lines.first().map_or(0, |l| l.red.len()), p.closed)).collect();
        }
        self.pictures = files;
        self.watch.after_image(m, &report);
        let status = report.status;
        self.images.push(report);
        status
    }

    /// The port behind an address, if there is one.
    fn port(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> Option<u32> {
        let arg = |k: usize| a.get(k).copied().unwrap_or(0);
        let p = |k: usize| arg(k);
        Some(match f {
            // ---- this package: instance.c as other packages call it
            0x081a_9080 => {
                instance::instance_dispose(self, m, Ptr32::new(p(0)), arg(1) as i16);
                0
            }
            0x081a_7354 => {
                instance::delete_mem_references(m);
                0
            }
            0x081a_7490 => instance::model_master_from_instance(self, m, Ptr32::new(p(0))).0,
            // ---- this package: callees of other source files it has on the image
            x if x == SI_MODEL_ALLOCATE.0 => model_b::model_allocate(m, Ptr32::new(p(0))).0,
            x if x == SI_MODEL_ALGO_TYPE_COPY.0 => {
                model_b::model_algo_type_copy(m, Ptr32::new(p(0)), Ptr32::new(p(1)));
                0
            }
            x if x == SI_INV_KIN_COPY.0 => invkin::inv_kin_copy(m, Ptr32::new(p(0))).0,
            x if x == SI_INV_KIN_DISPOSE.0 => {
                invkin::inv_kin_dispose(m, Ptr32::new(p(0)));
                0
            }
            x if x == SI_ED_GET_ELEM_FROM_MODEL.0 => edtools::ed_get_elem_from_model(self, m, Ptr32::new(p(0)), Ptr32::new(p(1))).0,
            0x080f_cec0 => edtools::ed_find_son(self, m, Ptr32::new(p(0))).0,
            0x080f_cef0 => edtools::ed_find_next_son(self, m, Ptr32::new(p(0))).0,
            0x080f_d3e0 => edtools::ed_get_elem_from_texture2d(self, m, p(0), Ptr32::new(p(1))).0,
            0x080f_d698 => {
                edtools::elemdict_update_model_hierarchy(self, m, Ptr32::new(p(0)));
                0
            }
            0x0810_17f4 => {
                edtools::elemdict_add_model(self, m, Ptr32::new(p(0)), Ptr32::new(p(1)), Ptr32::new(p(2)), arg(3) as i16);
                0
            }
            0x0810_1ad4 => {
                edtools::ed_add_model(self, m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16);
                0
            }
            0x0846_d5ec => edtools::model_nb_sub_models(m, Ptr32::new(p(0))) as u32,
            0x0828_00fc => edtools::light_get_graf_type(m, p(0)) as u32,
            // ---- this package: the render set-up's display side (finding 0018). Each of these
            // was compared with the emulated original; the routines that drive the display
            // (drawRegion, the dragging loops, the render-screen routines) have no arm and stay hooks.
            0x0830_a620 => {
                pre_render::get_rnd_box(m, p(0));
                0
            }
            0x0830_a6a0 => {
                pre_render::draw_rnd_box(self, m, p(0));
                0
            }
            0x0830_a6d8 => pre_render::abortion(self, m),
            0x0842_f730 => sys_render::init_stand_alone_render(self, m, p(0)),
            0x0842_faa8 => renu_filepath::mrfile_convert_to_unix(self, m, p(0), p(1), arg(2) as i32),
            0x082c_a448 => {
                renu_subregion::compute_bounds(m, p(0));
                0
            }
            0x0831_8750 => {
                preview::lookup_color(m, p(0), f32::from_bits(arg(1)), p(2));
                0
            }
            0x0831_80dc => {
                preview::preview_null(m, p(4));
                0
            }
            0x0831_80f8 => {
                preview::preview_marble(m, p(0), p(1), arg(2) as i32, arg(3) as i32, p(4));
                0
            }
            0x0831_8194 => {
                preview::preview_wood(m, p(0), p(1), arg(2) as i32, arg(3) as i32, p(4));
                0
            }
            0x0831_8238 => {
                preview::preview_cloud(m, p(0), p(1), arg(2) as i32, arg(3) as i32, p(4));
                0
            }
            0x0831_8058 => {
                preview::ttp_tform_pt(m, p(0), p(1), p(2));
                0
            }
            0x0831_7fb4 => {
                preview::ttp_init_pixel_sph_txt3d(m, p(0), arg(1) as i16);
                0
            }
            0x082c_57e4 => rens_main::prepare_rendering(self, m, p(0), p(1), p(2), arg(3) as i16),
            0x082c_5ef0 => {
                rens_main::mental_preview(self, m, arg(0) as i16, arg(1));
                0
            }
            0x082c_5ddc => rens_main::sequence_render_mental(self, m, arg(0) as i16),
            // renu_display.c: the render window
            x if x == SI_OPEN_RENDER_WINDOW.0 => renu_display::open_render_window(self, m, p(0)),
            x if x == SI_CLOSE_RENDER_WINDOW.0 => {
                renu_display::close_render_window(self, m, p(0));
                0
            }
            // SI_chunkFreeAll (0x0857cb00) on `SI_renChunk`, now a record of the image
            // (`SI_matterInit`'s last step): si-render's port of mem_chunk.c
            x if x == SI_CHUNK_FREE_ALL.0 && p(0) != 0 && m.is_mapped(p(0), 0x40) => si_render::mem_chunk_image::call(m, x, &[p(0)]).unwrap_or(0),
            // SI_resetRenderWinOpen (0x082c9c00)
            0x082c_9c00 => {
                renu_display::reset_render_win_open(m);
                0
            }
            // ---- si-dict
            x if x == SI_ELEMDICT_GET_ELEM_FROM_DATA.0 => si_dict::elemmisc::get_elem_from_data(m, &mut DictSide(self), Ptr32::new(p(0)), arg(1) as i32, Ptr32::new(p(2)), Ptr32::new(p(3))).0,
            x if x == SI_ELEMDICT_FIND_RELATED_ELEM.0 => {
                si_dict::elemrel::find_related_elem(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16, arg(3) as i32, arg(4) as i32, arg(5) as i32, Ptr32::new(p(6)), Ptr32::new(p(7))).0
            }
            x if x == SI_ELEMDICT_FIND_NEXT_RELATED_ELEM.0 => {
                si_dict::elemrel::find_next_related_elem(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16, arg(3) as i32, arg(4) as i32, arg(5) as i32, Ptr32::new(p(6)), Ptr32::new(p(7))).0
            }
            x if x == SI_ELEMDICT_RELATIONS_COUNT.0 => {
                si_dict::elemrel::relations_count(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16, arg(3) as i32, arg(4) as i32, arg(5) as i32, arg(6) as i32) as u32
            }
            x if x == SI_ELEMDICT_ADD_RELATION.0 => si_dict::elemrel::add_relation(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i32, arg(3) as i32, arg(4) as i32, arg(5) as i32, arg(6) as i32) as u32,
            x if x == SI_ELEMDICT_REMOVE_RELATION_ID.0 => si_dict::elemrel::remove_relation_id(m, &mut DictSide(self), Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i32, arg(3) as i32) as u32,
            x if x == SI_ELEMDICT_FIND_ELEM_BY_DATA.0 => si_dict::elemdict::find_elem_by_data(m, &mut DictSide(self), Ptr32::new(p(0)), arg(1) as i32, Ptr32::new(p(2))) as u32,
            x if x == SI_ELEMDICT_GET_ELEM.0 => si_dict::elemdict::get_elem(m, Ptr32::new(p(0)), arg(1) as i32, arg(2) as i32).0,
            x if x == SI_ELEMDICT_FIND_SON.0 => si_dict::elemmisc::find_son(m, Ptr32::new(p(0)), Ptr32::new(p(1))).0,
            x if x == SI_ELEMDICT_FIND_NEXT_SON.0 => si_dict::elemmisc::find_next_son(m, Ptr32::new(p(0)), Ptr32::new(p(1))).0,
            x if x == SI_ELEMDICT_UPDATE_HIER_INFO.0 => {
                si_dict::elemdict::update_hier_info(m, Ptr32::new(p(0)), arg(1) as i32, arg(2) as i32, arg(3) as i32);
                0
            }
            x if x == SI_ELEMDICT_SORT_HIER_ELEMENTS.0 => {
                si_dict::toposort::sort_hier_elements(m, Ptr32::new(p(0)), arg(1) as i32);
                0
            }
            x if x == SI_ELEMDICT_ADD_ELEM.0 => {
                si_dict::elemdict::add_elem(m, &mut DictSide(self), Ptr32::new(p(0)), arg(1) as i32, Ptr32::new(p(2)), Ptr32::new(p(3)), Ptr32::new(p(4)), arg(5) as i32, arg(6) as i32) as u32
            }
            x if x == SI_ELEMDICT_GET_NEW_NAME.0 => si_dict::elemname::get_new_name(m, &mut DictSide(self), Ptr32::new(p(0)), arg(1) as i32, Ptr32::new(p(2))).0,
            // SI_elemlistClearLowByteWorkFlag (0x08132c70)
            0x0813_2c70 => {
                si_dict::elemdict::elemlist_clear_low_byte_work_flag(m, Ptr32::new(p(0)), arg(1) as i32);
                0
            }
            // ---- si-scene: the name records
            x if x == SI_DB_LIST_NODE_ALLOCATE.0 => si_scene::dbnode::list_node_allocate(m, &mut DictSide(self)).0,
            x if x == SI_DB_FORMAT_FULL_NAME.0 => si_scene::dbnode::format_full_name(m, Ptr32::new(p(0))).0,
            // ---- si-model
            x if x == SI_MODEL_UPDATE_SON_STATE.0 => {
                si_model::model_image::state::update_son_state(m, &mut ModelSide(self), Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16);
                0
            }
            x if x == SI_MODEL_SET_STATE.0 => {
                si_model::model_image::setters::set_state(m, &mut ModelSide(self), Ptr32::new(p(0)), p(1), p(2), p(3));
                0
            }
            x if x == SI_MODEL_COPY_TRANSFORMATION.0 => {
                si_model::model_image::state::copy_transformation(m, &mut ModelSide(self), Ptr32::new(p(0)), Ptr32::new(p(1)));
                0
            }
            x if x == SI_MODEL_NEXT_SON.0 => si_model::model_image::tree::next_son(m, Ptr32::new(p(0)), Ptr32::new(p(1))).0,
            // SI_modelNextNode (0x08478340)
            0x0847_8340 => si_model::model_image::tree::next_node(m, Ptr32::new(p(0))).0,
            x if x == SI_MODEL_CHANGE_NAME.0 => {
                si_model::model_image::defs::change_name(m, Ptr32::new(p(0)), p(1));
                0
            }
            // ---- si-view
            // SI_findGlData (0x0832ee68)
            0x0832_ee68 => si_view::graflist::find_gl_data(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i32).0,
            x if x == SI_GRAFLIST_ADD_DATA.0 => si_view::graflist::graflist_add_data(m, &mut self.view(), Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i32, arg(3) as i16).0,
            x if x == SI_SCHEMATIC_NO_REFRESH.0 => {
                si_view::graflist::schematic_no_refresh(m, arg(0) as i16);
                0
            }
            x if si_view::wired::SEAM.iter().any(|r| r.addr == x) => {
                let routine = si_view::wired::SEAM.iter().copied().find(|r| r.addr == x).unwrap();
                si_view::Outside::call(&mut self.view(), m, routine, a)
            }
            // ---- si-render
            x if x == SI_IMAGE.0 && self.render => self.image(m, [p(0), p(1), p(2), p(3), p(4)]),
            // ---- si-motion: SI_sceneUpdateItems (0x082221a0)(time, flag, camera, copy the
            // camera), which si-render's SI_update calls in sequence mode
            0x0822_21a0 if self.motion => {
                super::motion::scene_update_items(m, f32::from_bits(arg(0)), arg(1) as i16, arg(2), arg(3) as i16, &mut self.motion_calls);
                0
            }
            // ---- si-motion: SI_modelMove (0x08229a90)(model), SI_modelConstraintMove
            // (0x08229aec)(model), which si-render's isMoving (motion blur) asks
            0x0822_9a90 if self.motion => super::motion::model_move(m, arg(0), &mut self.motion_calls),
            0x0822_9aec if self.motion => super::motion::model_constraint_move(m, arg(0), &mut self.motion_calls),
            // ---- geometry.c: si_traverseExpandedInstance (0x082739e0)(root),
            // SI_expandUpdateTransfo (0x08276364)(master, instance, copy)
            0x0827_39e0 => {
                super::geometry::traverse_expanded_instance(self, m, Ptr32::new(p(0)));
                0
            }
            0x0827_6364 => {
                super::geometry::expand_update_transfo(self, m, Ptr32::new(p(0)), Ptr32::new(p(1)), Ptr32::new(p(2)));
                0
            }
            // ---- si-scene: the look-ups of edtools.c the update calls
            0x080f_cb5c => si_scene::edtools::db_chapter2graf_type(&mut super::edtools::SceneSide(self), m, arg(0) as i32, arg(1) as i32) as u32,
            0x0810_6bbc => si_scene::edtools::ed_is_item_constrained(&mut super::edtools::SceneSide(self), m, p(0), p(1), arg(2) as i32, arg(3) as i32, arg(4) as i16, f32::from_bits(arg(5))) as u32,
            0x080f_cfb0 => si_scene::edtools::ed_get_elem_from_camera(&mut super::edtools::SceneSide(self), m, p(0), Ptr32::new(p(1))).0,
            0x080f_d0bc => si_scene::edtools::ed_get_elem_from_light(&mut super::edtools::SceneSide(self), m, p(0), Ptr32::new(p(1))).0,
            0x080f_d040 => si_scene::edtools::ed_get_cam_from_cam_int(&mut super::edtools::SceneSide(self), m, arg(0) as i32) as u32,
            0x080f_d150 => si_scene::edtools::ed_get_lig_from_lig_int(&mut super::edtools::SceneSide(self), m, arg(0) as i32) as u32,
            0x080f_d4dc => si_scene::edtools::ed_get_elem_from_shape(&mut super::edtools::SceneSide(self), m, p(0), Ptr32::new(p(1))).0,
            0x080f_d47c => si_scene::edtools::ed_get_elem_from_lattice(&mut super::edtools::SceneSide(self), m, p(0), arg(1) as i16, Ptr32::new(p(2))).0,
            // SI_elemdictGetCurRelation (0x08134db0)
            0x0813_4db0 => si_dict::elemrel::get_cur_relation(m, Ptr32::new(p(0)), Ptr32::new(p(1)), arg(2) as i16, arg(3) as i32).0,
            // SI_activationFcvCheckTime (0x0822aa64): si-motion's port
            0x0822_aa64 => super::motion::activation(m, p(0), arg(1), p(2)),
            // ---- si-scene: SI_edGetElementsGrafNodeInfo (0x08104acc)(dictionary), which
            // si-render's SI_update calls after the scene's update
            0x0810_4acc if self.graf_info => {
                si_scene::edtools::ed_get_elements_graf_node_info(m, Ptr32::new(p(0)));
                0
            }
            // ---- si-mem: the allocator by address
            // SI_malloc (0x080d0ee8), SI_realloc (0x080d0f28)
            0x080d_0ee8 => m.si_malloc(arg(0)),
            0x080d_0f28 => m.si_realloc(arg(0), arg(1)),
            // ---- hooks answered as the original answers them while a scene is loaded
            // SI_edAddMaterial (0x0810203c), SI_edAddTexture2D (0x08102314),
            // SI_edAddTexture3D (0x081027e0): each tests SI_addInDictionary first and
            // answers 1 without doing anything when it is 0 (read from code; the paths
            // that add an element are not ported, so a call with the switch set is a hook)
            0x0810_203c | 0x0810_2314 | 0x0810_27e0 if m[super::globals::SI_ADD_IN_DICTIONARY] == 0 => 1,
            // ---- routing3: ports of other crates that were still answered 0 here
            x if self.routing => return self.more(m, x, a),
            _ => return None,
        })
    }

    /// The routines whose port a crate has on the image and nothing above wires
    /// (routing3, finding 0023 section 38): the model library's copies, cut and parenting,
    /// the NURBS point transforms, the material copies and shader references, the name
    /// record's copy, the graf lists, the camera constructor, this package's own two
    /// predicates, and, last, whatever `si-deform`'s router answers. `None` leaves the call
    /// a hook. Nothing here reproduces a function of the original. `RENSETUP_NO_ROUTING3=1`
    /// turns all of it off; `RENSETUP_ROUTING3_SKIP=0xADDR,...` leaves single addresses.
    fn more(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> Option<u32> {
        if self.routing_skip.contains(&f) {
            return None;
        }
        let arg = |k: usize| a.get(k).copied().unwrap_or(0);
        use si_model::model_image::{defs, setup, tree};
        Some(match f {
            // si-model (model.c, hierarchy library)
            0x0847_32fc => {
                defs::inverse(m, &mut ModelSide(self), Ptr32::new(arg(0)));
                0
            }
            0x0846_2f1c => {
                setup::transfo_anim_copy(m, &mut ModelSide(self), Ptr32::new(arg(0)), Ptr32::new(arg(1)));
                0
            }
            0x0846_33e8 => {
                setup::copy_from_setup(m, &mut ModelSide(self), Ptr32::new(arg(0)), Ptr32::new(arg(1)));
                0
            }
            0x0846_d838 => tree::cut(m, &mut ModelSide(self), arg(0), arg(1)),
            0x0817_b500 => {
                tree::create_parent(m, &mut ModelSide(self), arg(0), arg(1), arg(2) as i16);
                0
            }
            // si-nurbs (nrb_misc.c)
            0x084e_9750 => {
                si_nurbs::image::crv_apply_matrix(m, arg(0), arg(1), arg(2) as i32);
                0
            }
            0x084e_9644 => {
                si_nurbs::image::srf_apply_matrix(m, &mut si_scene::envside::NurbsSide(&mut ModelSide(self)), arg(0), arg(1), arg(2) as i32);
                0
            }
            // si-nurbs, the rest that calls each other (`si_nurbs::routed::ROUTED`: knot insertions, SwapUV,
            // SI_nurbsSrfInverse, blend/merge/extend, intersections ...), but those answering in st0
            0x0850_cc48 => return None,
            f if si_nurbs::routed::ROUTED.contains(&f) => {
                let mut side = si_scene::envside::NurbsSide(&mut ModelSide(self));
                return si_nurbs::routed::Routed::new(&mut side).route(m, f, a).map(|r| r.eax);
            }
            // si-scene (mat_dial.c, edtools.c)
            0x0828_4888 => si_scene::matconv::material_copy(&mut super::edtools::SceneSide(self), m, arg(0)),
            0x0828_4504 => {
                si_scene::matconv::material_transfer(&mut super::edtools::SceneSide(self), m, arg(0), arg(1), arg(2) as i16);
                0
            }
            0x0810_0c34 => {
                si_scene::edtools::ed_model_set_material(&mut super::edtools::SceneSide(self), m, Ptr32::new(arg(0)), arg(1));
                0
            }
            // si-render (shd_fct.c)
            0x082a_6808 => {
                si_render::shd_copy::copy_material_shaders(m, &mut MemSide(self), arg(0), arg(1));
                0
            }
            0x082a_69dc => {
                si_render::shd_copy::delete_material_shaders(m, &mut MemSide(self), arg(0));
                0
            }
            0x082a_6ac0 => {
                si_render::shd_copy::connect_material_shaders(m, &mut MemSide(self), arg(0));
                0
            }
            // si-dict (elemname.c)
            0x0813_1ba8 => {
                si_dict::elemname::db_list_node_copy_info(m, &mut DictSide(self), Ptr32::new(arg(0)), Ptr32::new(arg(1)), Ptr32::new(arg(2)));
                0
            }
            // si-view (graf lists)
            0x0832_c1bc => {
                si_view::graflist::list::graf_list_create(m);
                0
            }
            0x0832_c204 => si_view::graflist::list::graf_list_add(m) as u32,
            // si-elemio (camera constructor)
            0x080d_12dc => si_elemio::image::camfct::new_camera(m, &mut ElemioSide(self), arg(0), arg(1) as i32),
            // si-hrc (invkin.c allocator)
            0x084a_d8f0 => si_hrc::image::chunks::inv_kin_allocate(m),
            // si-mental (mi_mr_main.c)
            0x0858_0cfc => si_mental::mi_mr_main::si_get_mi2_output(m),
            0x0858_0ec4 => si_mental::mi_mr_main::si_spdl_use_namesrv_img_ref(m, &mut MemSide(self)),
            0x0858_1c78 => {
                si_mental::mi_mr_main::si_cleanup_translator(m, &mut MemSide(self), arg(0));
                0
            }
            // routing4: the lattice routines (si-deform; si-model's two tests are the same, both
            // checked against the original), the metaball routines, the mesh conversions and the
            // look-ups of the database side that have an image form
            0x0843_ff1c => si_deform::lattice::model_lattice(m, Ptr32::new(arg(0)), Ptr32::new(arg(1))).addr(),
            0x0843_ff88 => si_deform::lattice::model_tree_lattice(m, Ptr32::new(arg(0))).addr(),
            0x0843_de60 => {
                si_deform::lattice::lattice_copy_from_setup(m, &mut DeformSide(self), Ptr32::new(arg(0)), Ptr32::new(arg(1)));
                0
            }
            0x0845_c078 => {
                si_meta::surface::si_meta_group_copy(m, arg(0), arg(1));
                0
            }
            0x0845_abf8 => {
                si_meta::metaball::metaball_touch(m, &mut MemSide(self), arg(0), arg(1));
                0
            }
            0x0845_e3b4 => {
                si_meta::metaball::meta_system_validate_groups(m, arg(0));
                0
            }
            0x0845_e210 => {
                si_meta::metaball::meta_system_resolve_group_name(m, &mut MemSide(self), arg(0));
                0
            }
            // lic1: SI_metaballInit (0x0845abc8), si-meta's port (SI_initStandAloneRender's start-up calls it), and its
            // VCInit (0x08575ca0), si-render's
            0x0845_abc8 => {
                metaball_library_initial_data(m);
                si_meta::init::si_metaball_init(m, &mut MemSide(self));
                0
            }
            0x0857_5ca0 => si_render::user::vc_init(m) as u32,
            // lic2: the start-up's SI_matterInit (0x08291bec, si-matter's port; `standAloneRender` and
            // `SI_initStandAloneRender` call it) and the routines it calls that have ports: si_lightInitialize
            // (si-elemio), si_linkDefaultMaterial, si_initRenderGprops, SI_renControlInit, SI_matterFadeInit and
            // fogInit (this package's `matter`), SI_chunkAllocate (si-render). Its font file is read through
            // fopen/fscanf/fclose, which stay hooks: the font is read only when the embedding program's `Watch`
            // answers them (rensetup-check's own start-up reads it with `matter::matter_init`). The binary's data it
            // reads is placed first ([`place_start_data`]).
            0x0829_1bec => {
                place_start_data(m, MATTER_INIT_TEXTS, MATTER_INIT_POINTERS);
                si_matter::matter_copy::si_matter_init(m, &mut MemSide(self));
                0
            }
            0x0828_19c8 => {
                si_elemio::image::lights::light_initialize(m, &mut ElemioSide(self), arg(0), arg(1) as i32);
                0
            }
            0x0828_205c => {
                super::matter::link_default_material(m);
                0
            }
            0x082a_5920 => {
                super::matter::init_render_gprops(m, arg(0));
                0
            }
            0x0829_12e4 => {
                super::matter::ren_control_init(m, arg(0));
                0
            }
            0x0829_1bd4 => {
                super::matter::matter_fade_init(m);
                0
            }
            0x0829_6200 => {
                super::matter::fog_init_default(m);
                0
            }
            0x0857_ca00 => si_render::mem_chunk_image::chunk_allocate(m, arg(0) as i32, arg(1) as i32),
            // lic2: the start-up's SI_cusInit (0x085325c4, si-custom's port): the custom effects' status records and
            // the scan of their directories (getenv, scandir and the C library's files stay hooks), after the
            // binary's data it reads ([`place_start_data`])
            0x0853_25c4 => {
                place_start_data(m, CUS_INIT_TEXTS, CUS_INIT_POINTERS);
                place_start_messages(m, CUS_INIT_MESSAGES);
                si_custom::custom_init::si_cus_init(m, &mut MemSide(self));
                0
            }
            // wire5: the rest of the metaball library's entry points si-meta calls through its hooks (user.c, si-render's),
            // and the two custom-effect routines SI_graflistDeleteGrafnode calls (custom_del.rs of si-custom)
            0x0857_5cf4 => si_render::user::vc_re_init(m) as u32,
            0x0857_5d14 => si_render::user::vc_rm_all_tri(m) as u32,
            0x0857_5dc0 => si_render::user::vc_create_prim(m, arg(0) as i32, arg(1), arg(2)) as u32,
            0x0857_5dec => si_render::user::vc_set_prim_param(m, arg(0) as i32, arg(1) as i32, arg(2)) as u32,
            0x0857_5e0c => si_render::user::vc_get_prim_param(m, arg(0) as i32, arg(1) as i32, arg(2)) as u32,
            0x0857_5fa8 => si_render::user::vc_delete_prim(m, arg(0) as i32) as u32,
            0x0857_5fc0 => si_render::user::vc_add_to_group(m, arg(0) as i32, arg(1) as i32) as u32,
            0x0857_6044 => si_render::user::vc_unblend_groups(m, arg(0) as i32, arg(1) as i32) as u32,
            0x0857_6060 => si_render::user::vc_info_prim(m, arg(0) as i32, arg(1), arg(2)) as u32,
            0x0857_60e8 => si_render::user::vc_gen_poly_data(m, arg(0) as i32, arg(1) as i32, arg(2), arg(3), arg(4), arg(5), arg(6), arg(7)) as u32,
            0x0854_a094 => si_custom::custom_del::si_cus_delete_icon(m, &mut MemSide(self), arg(0)) as u32,
            0x0854_bc44 => {
                si_custom::custom_del::si_cus_elem_callback_update_effects_for_deletion(m, arg(0), arg(1));
                0
            }
            0x0832_9898 => {
                si_model::model_image::fastplay::set_dirty(m, &mut ModelSide(self), arg(0), arg(1) as i32, arg(2) as i32);
                0
            }
            0x0849_f9f8 => si_kine::image::qstretch::kine_state_allocate(m),
            0x084b_bf8c => {
                si_deform::envtools::envelop_info_dispose(m, Ptr32::new(arg(0)));
                0
            }
            0x0847_b69c => si_model::mesh_image::patch::patch_convert_to_mesh(m, &mut ModelSide(self), arg(0), arg(1)),
            0x0843_5960 => si_model::mesh_image::face::face_convert_to_mesh(m, &mut ModelSide(self), arg(0), arg(1) as i16),
            // (a trimmed surface stops the port: left to the hook as before)
            0x084d_f320 => return si_model::mesh_image::nurbs::nurbs_convert_to_mesh(m, &mut ModelSide(self), arg(0), arg(1)).ok(),
            0x0844_3144 => si_model::mesh_image::tri::mesh_triangulate(m, &mut ModelSide(self), arg(0), arg(1) as i16) as u16 as u32,
            0x083c_f154 => si_dict::elemsort::str_strip_digit(m, Ptr32::new(arg(0))).addr(),
            0x0813_3bc8 => {
                si_dict::elemdict::elemlist_init(m, Ptr32::new(arg(0)));
                0
            }
            0x0813_5fd4 => {
                si_dict::elemdict::elemlist_dispose(m, &mut DictSide(self), Ptr32::new(arg(0)), arg(1) as i16);
                0
            }
            0x0813_27c4 => si_dict::elemdict::elemlist_add_elem(
                m,
                &mut DictSide(self),
                Ptr32::new(arg(0)),
                Ptr32::new(arg(1)),
                Ptr32::new(arg(2)),
                Ptr32::new(arg(3)),
                arg(4) as i32,
                arg(5) as i32,
                arg(6) as i16,
                arg(7) as i16,
            ) as u32,
            0x0813_2b1c => si_dict::elemdict::elemlist_get_elem(m, Ptr32::new(arg(0)), arg(1) as i32).addr(),
            0x0813_19f0 => si_dict::elemname::db_scn_get_list_node(m, &mut DictSide(self), Ptr32::new(arg(0)), Ptr32::new(arg(1)), arg(2) as i16).addr(),
            0x0813_7884 => {
                si_dict::elemname::reset_origin_db(m, Ptr32::new(arg(0)), Ptr32::new(arg(1)));
                0
            }
            0x0812_0a48 => si_scene::dbnode::ed_get_suffix_from_chapter(m, arg(0) as i32).addr(),
            0x0812_1bdc => si_scene::dbnode::name_check_validity(m, Ptr32::new(arg(0)), arg(1) as i16).addr(),
            0x0812_1ca8 => {
                si_scene::dbnode::format_prefix_name(m, Ptr32::new(arg(0)), Ptr32::new(arg(1)), Ptr32::new(arg(2)));
                0
            }
            // routeA: ports of other crates that the render path's hooks still answered 0 (the render walk's
            // light constructor)
            0x0828_1d34 => si_elemio::image::lights::light_allocate(m, &mut ElemioSide(self), arg(0) as i32),
            // this package
            0x081a_751c => instance::model_is_master(self, m, Ptr32::new(arg(0))) as u32,
            0x084b_b94c => super::envtools::main_glob_env(m, Ptr32::new(arg(0))) as u32,
            // si-deform's router: dictionary, geometry copies and disposals, model tree
            x if si_deform::routed::ROUTED.contains(&x) => {
                let mut side = DeformSide(self);
                let mut r = si_deform::routed::Routed::new(&mut side);
                return r.route(m, x, a);
            }
            _ => return None,
        })
    }
}

/// routing4: the addresses `World::more` answers beyond [`ROUTING3`].
pub const ROUTING4: &[u32] = &[
    0x0843_ff1c, 0x0843_ff88, 0x0843_de60, 0x0845_c078, 0x0845_abf8, 0x0845_e3b4, 0x0845_e210, 0x0832_9898, 0x0849_f9f8, 0x084b_bf8c, 0x0847_b69c,
    0x0843_5960, 0x0844_3144, 0x083c_f154, 0x0813_3bc8, 0x0813_5fd4, 0x0813_27c4, 0x0813_2b1c, 0x0813_19f0, 0x0813_7884, 0x0812_0a48, 0x0812_1bdc,
    0x0812_1ca8, 0x0828_1d34, 0x0845_abc8, 0x0857_5ca0,
    0x0857_5cf4, 0x0857_5d14, 0x0857_5dc0, 0x0857_5dec, 0x0857_5e0c, 0x0857_5fa8, 0x0857_5fc0, 0x0857_6044, 0x0857_6060, 0x0857_60e8, 0x0854_a094,
    0x0854_bc44,
];

/// lic2: the start-up routines `World::more` answers by a port (`SI_matterInit`, `SI_cusInit`), and
/// those of `SI_matterInit`'s callees that have ports (finding 0038, lic2).
pub const LIC2: &[u32] = &[0x0829_1bec, 0x0828_19c8, 0x0828_205c, 0x082a_5920, 0x0829_12e4, 0x0829_1bd4, 0x0829_6200, 0x0857_ca00, 0x0853_25c4];

/// lic2: the texts of the binary's `.data` (with their zero) that si-matter's `SI_matterInit` reads
/// from the image: `incrustInit`'s font name text, the `fopen` mode, the `fscanf` format, and the
/// font file's name that `SI_softFont` points at. Held against the binary by
/// `re/tools/lic2/start_data.py`.
pub const MATTER_INIT_TEXTS: &[(u32, &[u8])] =
    &[(0x086a_17f8, b"Hello World!\0"), (0x086a_180a, b"r\0"), (0x086a_180c, b"%hx\0"), (0x0867_94b8, b"/Softimage.fnt\0")];

/// lic2: `SI_softFont` (0x086794c8) and its initial value.
pub const MATTER_INIT_POINTERS: &[(u32, u32)] = &[(0x0867_94c8, 0x0867_94b8)];

/// lic2: the texts of the binary's `.data` that si-custom's `SI_cusInit` reads from the image: the
/// eleven environment variable names, the six default directories, the four "Insufficient memory"
/// messages, the `fopen` mode and the three pieces of `SI_cusScan`'s warning.
pub const CUS_INIT_TEXTS: &[(u32, &[u8])] = &[
    (0x086d_f459, b"SI_CUSTOM_MODEL\0"),
    (0x086d_f469, b"CUSTOM_MODEL\0"),
    (0x086d_f47d, b"SI_CUSTOM_MOTION\0"),
    (0x086d_f48e, b"CUSTOM_MOTION\0"),
    (0x086d_f4a3, b"SI_CUSTOM_MATTER\0"),
    (0x086d_f4b4, b"CUSTOM_MATTER\0"),
    (0x086d_f4c8, b"SI_CUSTOM_TOOLS\0"),
    (0x086d_f4d8, b"CUSTOM_TOOLS\0"),
    (0x086d_f4eb, b"SI_CUSTOM_ACTOR\0"),
    (0x086d_f4fb, b"CUSTOM_ACTOR\0"),
    (0x086d_f511, b"SI_CHNLDRIVER\0"),
    (0x0867_9340, b"/usr/softimage/custom/model\0"),
    (0x0867_9370, b"/usr/softimage/custom/motion\0"),
    (0x086e_46f2, b"/usr/softimage/custom/matter\0"),
    (0x086e_4890, b"/usr/softimage/custom/tools\0"),
    (0x086e_46a0, b"/usr/softimage/custom/actor\0"),
    (0x0867_92d6, b"/usr/softimage/chnlDriver/bin\0"),
    (0x086d_f528, b"r\0"),
    (0x086d_f52a, b".cus\n\0"),
    (0x086d_f547, b"WARNING: Ignoring \0"),
];

/// The texts of [`CUS_INIT_TEXTS`]'s kind that are messages: the four "Insufficient memory" texts and the
/// "conflicts" piece of `SI_cusScan`'s warning. Their bytes are read from the user's install (`si-rt`)
/// when placed, not held here (release model).
pub const CUS_INIT_MESSAGES: &[u32] = &[0x086d_f360, 0x086d_f3a0, 0x086d_f3e0, 0x086d_f420, 0x086d_f530];

/// lic2: the six default directory pointers of `SI_cusInit` (model, motion, matter, tools, actor,
/// channel driver) and their initial values.
pub const CUS_INIT_POINTERS: &[(u32, u32)] = &[
    (0x0867_935c, 0x0867_9340),
    (0x0867_9390, 0x0867_9370),
    (0x086e_4710, 0x086e_46f2),
    (0x086e_48ac, 0x086e_4890),
    (0x086e_46bc, 0x086e_46a0),
    (0x0867_92f4, 0x0867_92d6),
];

/// lic2: puts the binary's initial `.data` a start-up port reads into a native image, which holds
/// only what the crates place (the emulator's image holds the whole segment): each text where its
/// first byte is still 0, each pointer word where it is still 0, so that what the image already
/// holds (an earlier placement, a value the program set) stays.
pub fn place_start_data(m: &mut Mem, texts: &[(u32, &[u8])], pointers: &[(u32, u32)]) {
    for &(at, text) in texts {
        if m.u8(at) == 0 {
            m.set_bytes(at, text);
        }
    }
    for &(at, v) in pointers {
        if m.u32(at) == 0 {
            m.set_u32(at, v);
        }
    }
}

/// Like [`place_start_data`] for [`CUS_INIT_MESSAGES`]: each message (with its zero) read from the install.
pub fn place_start_messages(m: &mut Mem, addrs: &[u32]) {
    for &at in addrs {
        if m.u8(at) == 0 {
            m.put_cstr(at, si_rt::message(at));
        }
    }
}

/// The routines [`World::more`] answers by a port of its own (those of `si-deform`'s router
/// are `si_deform::routed::ROUTED`).
pub const ROUTING3: &[u32] = &[
    0x0847_32fc, 0x0846_2f1c, 0x0846_33e8, 0x0846_d838, 0x0817_b500, 0x084e_9750, 0x084e_9644, 0x0828_4888, 0x0828_4504, 0x0810_0c34, 0x082a_6808,
    0x082a_69dc, 0x082a_6ac0, 0x0858_0cfc, 0x0858_0ec4, 0x0858_1c78, 0x0813_1ba8, 0x0832_c1bc, 0x0832_c204, 0x080d_12dc, 0x084a_d8f0, 0x081a_751c, 0x084b_b94c,
];

impl<W: Watch> Outside for World<W> {
    fn call(&mut self, m: &mut Mem, f: FnPtr, args: &[u32]) -> u32 {
        // two of si-view's seam have no port yet: they fall through to the hooks
        let seam_gap = [0x0833_bd74, 0x0833_c5dc].contains(&f.0);
        if !seam_gap {
            let faults = m.fault_count();
            if let Some(answer) = self.port(m, f.0, args) {
                *self.ported.entry(f.0).or_default() += 1;
                if faults == 0 && m.fault_count() != 0 && self.fault_in.is_none() {
                    self.fault_in = Some(f.0);
                }
                return answer;
            }
        }
        *self.hooks.entry(f.0).or_default() += 1;
        self.watch.hook(m, f, args).unwrap_or(0)
    }
}

/// The name of a routine by its address, from the tables of the crates wired here.
pub fn name_of(addr: u32) -> String {
    if let Some(n) = super::outside::name_of(FnPtr::new(addr)) {
        return n.to_string();
    }
    if let Some(r) = si_view::outside::ALL.iter().find(|r| r.addr == addr) {
        return r.name.to_string();
    }
    if let Some(r) = si_dict::outside::ALL.iter().find(|r| r.addr == addr) {
        return r.name.to_string();
    }
    if let Some(c) = si_model::model_image::callee::ALL.iter().find(|c| c.addr == addr) {
        return c.name.to_string();
    }
    if let Some(n) = OTHER_NAMES.iter().find(|n| n.0 == addr) {
        return n.1.to_string();
    }
    format!("0x{addr:08x}")
}

/// Callees of `si-render`'s `SI_image` that the other tables do not name (`si-render`
/// has addresses only), and routines of this package as other packages call them: each
/// `name (0xADDRESS)`, held against the symbol table by `re/tools/si-rensetup/check_names.py`.
pub const OTHER_NAMES: &[(u32, &str)] = &[
    (0x080c_3bb8, "getenv"),
    (0x0826_0b78, "SI_dtime"),
    (0x0834_7290, "SI_tagMdlRm"),
    (0x0834_72f8, "SI_tagMdlUpdate"),
    (0x080c_48b8, "printf"),
    (0x080c_4958, "fclose"),
    (0x080c_3c58, "mallopt"),
    (0x080d_1038, "SI_signalInit"),
    (0x0820_f470, "SI_sceneRefresh"),
    (0x0860_e12c, "qreset"),
    (0x0860_e1bc, "qread"),
    (0x0861_3094, "gflush"),
    (0x0860_ec44, "font"),
    (0x0861_9ba4, "getorigin"),
    (0x0861_1a50, "lrectwrite"),
    (0x0861_2158, "sboxs"),
    (0x0861_81f0, "getdisplaymode"),
    (0x0826_072c, "SI_warning"),
    (0x082a_d9f8, "SI_txt2dSetWrap"),
    (0x0810_2ab0, "SI_elemdictAddPicture"),
    (0x0810_2c34, "SI_elemdictPictureElemRemove"),
    (0x0812_6748, "SI_system"),
    (0x085c_fac4, "SI_fsDitherInit"),
    (0x085d_2970, "SI_pictureOpenFile"),
    (0x085d_2900, "SI_pictDisplayError"),
    (0x085d_2cfc, "SI_scanlineRead"),
    (0x0826_d580, "SI_tagAllocate"),
    (0x0826_d5c8, "SI_tagAllocateChannels"),
    (0x0826_d950, "si_tagWriteFileHeader"),
    (0x0833_bd74, "SI_schemComputeHierarchiesBBox"),
    (0x080c_3858, "rename"),
    (0x080c_3c78, "unlink"),
    (0x080c_3a98, "localtime"),
    (0x0810_203c, "SI_edAddMaterial"),
    (0x0810_2314, "SI_edAddTexture2D"),
    (0x0810_27e0, "SI_edAddTexture3D"),
    (0x0813_2c70, "SI_elemlistClearLowByteWorkFlag"),
    (0x0847_8340, "SI_modelNextNode"),
    (0x0832_ee68, "SI_findGlData"),
    (0x081a_9080, "SI_instanceDispose"),
    (0x081a_7354, "SI_deleteMemReferences"),
    (0x081a_7490, "SI_modelMasterFromInstance"),
    (0x080f_cec0, "SI_edFindSon"),
    (0x080f_cef0, "SI_edFindNextSon"),
    (0x080f_d3e0, "SI_edGetElemFromTexture2D"),
    (0x080f_d698, "SI_elemdictUpdateModelHierarchy"),
    (0x0810_17f4, "SI_elemdictAddModel"),
    (0x0810_1ad4, "SI_edAddModel"),
    (0x0846_d5ec, "SI_modelNbSubModels"),
    (0x0828_00fc, "SI_lightGetGrafType"),
    (0x082c_9c00, "SI_resetRenderWinOpen"),
    (0x080d_0ee8, "SI_malloc"),
    (0x080d_0f28, "SI_realloc"),
    (0x0822_21a0, "SI_sceneUpdateItems"),
    (0x0822_9a90, "SI_modelMove"),
    (0x0822_9aec, "SI_modelConstraintMove"),
    (0x0810_4acc, "SI_edGetElementsGrafNodeInfo"),
    (0x080f_cb5c, "SI_dbChapter2GrafType"),
    (0x0810_6bbc, "SI_edIsItemConstrained"),
    (0x080f_cfb0, "SI_edGetElemFromCamera"),
    (0x080f_d0bc, "SI_edGetElemFromLight"),
    (0x080f_d040, "SI_edGetCamFromCamInt"),
    (0x080f_d150, "SI_edGetLigFromLigInt"),
    (0x080f_d4dc, "SI_edGetElemFromShape"),
    (0x080f_d47c, "SI_edGetElemFromLattice"),
    (0x0813_4db0, "SI_elemdictGetCurRelation"),
    (0x0822_aa64, "SI_activationFcvCheckTime"),
];

/// This world as `si-view` calls out.
pub struct ViewSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_view::Outside for ViewSide<'_, O> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(routine.addr), args)
    }
}

/// This world as `si-model`'s image module calls out.
pub struct ModelSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_model::model_image::Outside for ModelSide<'_, O> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(callee.addr), args)
    }
}

/// This world as the dictionaries call out.
pub struct DictSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_dict::Outside for DictSide<'_, O> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_dict::Routine, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(routine.addr), args)
    }
}

/// This world as `si-render`'s routines on the image call out. No routine here leaves
/// through `longjmp`.
pub struct RenderSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_render::img::Outside for RenderSide<'_, O> {
    fn call(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(f), args)
    }
}

/// This world as the crates with `si_mem`'s calling convention call out (`shd_fct.c`).
pub struct MemSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_mem::Outside for MemSide<'_, O> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        si_mem::Ret::eax(self.0.call(m, FnPtr::new(r.addr), args))
    }
}

/// The metaball library's four first-time switches with the binary's initial value 1, which the image the loader
/// makes does not hold (`first_init.196` 0x086ee66e of `VC_initOrigLists`, `first_init.192` 0x086ee66c of
/// `VC_initLocalLists`, `first_init.33` 0x086eeb40 of `VC_initSplitData`, `first_init.192` 0x086eed2c of
/// `VC_initEvalContrib`): with 0 `VCInit` takes the image for one whose tables were made already and frees what is
/// not there. Only on an image whose library has no tables yet (`vcG_MB` 0x088299c4 and `vcG_voxList` 0x088299d0
/// null), which is the state the program starts in.
pub fn metaball_library_initial_data(m: &mut Mem) {
    if m.u32(0x0882_99c4) == 0 && m.u32(0x0882_99d0) == 0 {
        for a in [0x086e_e66e, 0x086e_e66c, 0x086e_eb40, 0x086e_ed2c] {
            m.set_i16(a, 1);
        }
    }
}

/// The stand-alone renderer's `SI_metaballInit (0x0845abc8)` for a start-up that is not run through [`World`]
/// (`rensetup-check`, `render-frames`): si-meta's port, with `VCInit (0x08575ca0)` si-render's and the failure line on
/// stdout. Call it after `SI_matterInit`'s work and with `SI_standaloneRenderer` (0x08679400) already set, as
/// `standAloneRender (0x080cdffc)` does: it then takes the write bit off the metaball attributes of `SI_ATD`.
pub fn start_up_metaball_init(m: &mut Mem) {
    metaball_library_initial_data(m);
    struct Start;
    impl si_mem::Outside for Start {
        fn call(&mut self, m: &mut Mem, r: si_mem::Routine, _args: &[u32]) -> si_mem::Ret {
            match r.addr {
                0x0857_5ca0 => si_mem::Ret::int(si_render::user::vc_init(m)),
                _ => si_mem::Ret::NONE,
            }
        }
        fn print(&mut self, stream: u32, text: &[u8]) {
            use std::io::Write;
            if stream == 2 {
                let _ = std::io::stderr().write_all(text);
            } else {
                let _ = std::io::stdout().write_all(text);
            }
        }
    }
    si_meta::init::si_metaball_init(m, &mut Start);
}

/// This world as `si-elemio`'s image routines call out.
pub struct ElemioSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_elemio::image::Outside for ElemioSide<'_, O> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_elemio::image::Routine, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(routine.addr), args)
    }
    fn file_left(&mut self, _: &mut Mem, _: u32) -> u32 {
        0
    }
}

/// This world as `si-deform`'s router calls out (what no port answers goes on to the hooks).
pub struct DeformSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_deform::outside::Outside for DeformSide<'_, O> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_deform::outside::Routine, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(routine.addr), args)
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}
