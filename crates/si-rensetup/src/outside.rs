// SPDX-License-Identifier: MIT
//! The calls the render set-up makes into parts of the program that are other packages':
//! the scene graph, the model code, materials, custom effects, the windows, the status line,
//! the scene update. One trait, [`Outside`], with the original's routine named at each
//! method; [`Recorder`] answers them with what a check told it to and writes each call down.

use crate::world::Heap;
use si_scene::dict::Dict;

/// The rest of the program as the render set-up calls it.
pub trait Outside {
    /// `SI_statusDisplayMessage(text, level)` (0x083cca94).
    fn status_display_message(&mut self, text: &[u8], level: i32);
    /// `usleep`.
    fn usleep(&mut self, micros: u32);
    /// `printf`: text on standard output.
    fn print(&mut self, text: &[u8]);
    /// `SI_edGetElementsGrafNodeInfo(dictionary)` (0x08104acc; ported in `si-scene` for its
    /// own scene record): every element with a node of the scene graph takes the node's
    /// selection state, display type and wire colour. `render` says which dictionary.
    fn ed_get_elements_graf_node_info(&mut self, d: &mut Dict, render: bool);
    /// `SI_materialAllocate()` (0x08288c74): a new material with the defaults of
    /// `SI_materialInitialize`; the answer is its handle in the heap.
    fn material_allocate(&mut self, heap: &mut Heap) -> u32;
    /// `SI_modelUpdateSonState(father, model, flag)` (0x084662f8; ported in `si-model`):
    /// the node's matrices are rebuilt from its local scaling, rotation and translation
    /// and its father's global ones.
    fn model_update_son_state(&mut self, heap: &mut Heap, father: u32, model: u32, flag: i16);
    /// `SI_modelExpandAllWithoutGeometryAbsolute(master, NULL, 1)` (0x081a8be0, `instance.c`):
    /// a copy of the master's hierarchy whose nodes point at the master's geometry,
    /// materials and textures, instances nested in it expanded too (through the current
    /// dictionary, `current`). The answer is the copy's root, null for failure.
    fn model_expand_all_without_geometry_absolute(&mut self, heap: &mut Heap, current: &mut Dict, master: u32) -> u32;
    /// `SI_expandUpdateTransfo(master, instance, copy)` (0x08276364, `renutil.c`): the copy
    /// takes the instance's transformation and visibility and its matrices are rebuilt.
    fn expand_update_transfo(&mut self, heap: &mut Heap, current: &mut Dict, master: u32, instance: u32, copy: u32);
    /// `SI_modelNbTriangles(model)` (0x0846cd80).
    fn model_nb_triangles(&mut self, heap: &Heap, model: u32) -> i32;
    /// `SI_printWarnUV(name)`: the warning that a UV-mapped texture is inherited by a model
    /// that cannot carry texture coordinates.
    fn print_warn_uv(&mut self, name: Option<&[u8]>);
    /// `SI_cusGetEffectByIcon(model, &status)` (0x0854ac2c): `None` when the node is not the
    /// icon of an installed effect; otherwise what the effect's pointer at +0x104 leads to:
    /// `None` for a null pointer, else the word it points at.
    fn cus_get_effect_by_icon(&mut self, model: u32) -> Option<Option<i32>>;
    /// `SI_elemdictTopoSort(&SI_RD, current, &current->upd_list, 0, 0)` (0x08140828; ported
    /// in `si-scene`).
    fn topo_sort(&mut self, current: &mut Dict);
    /// `SI_dbCheckExtraUnToldRelation(current, a, b, c)` (0x08104fb8, `edtools.c`).
    fn db_check_extra_untold_relation(&mut self, current: &mut Dict, a: i16, b: i16, c: i16);
    /// `SI_sceneUpdateItems(time, 1, camera, 1)` (0x082221a0; ported in `si-motion`): the
    /// scene brought to a time.
    fn scene_update_items(&mut self, time: f32, camera: u32);
    /// `SI_windowGetHilite(&grafport)` (0x083d63e0; `si-ui`): the camera (+0x2c) of the
    /// highlighted window, `None` when no window is highlighted.
    fn window_hilite_camera(&mut self) -> Option<u32>;
    /// The windows of `SI_template` (0x08782080): per window whether it is open (+8 of its
    /// 0x2b8-byte record) and its camera (+0x2c). Count at +84, array at +96.
    fn template_windows(&mut self) -> Vec<(i16, u32)>;
    /// `SI_elemdictPictureElemRemove(dictionary, picture, NULL)` (0x08102c34).
    fn picture_elem_remove(&mut self, d: &mut Dict, picture: u32);
    /// `SI_udfResetActive()` (`udf_fct.c`): no shader is marked as used.
    fn udf_reset_active(&mut self);
    /// `SI_udfIsActive(shader)`: 1 for a shader that is switched on.
    fn udf_is_active(&mut self, shader: u32) -> i16;
    /// `SI_udfSetAsSelected(shader)`: the shader is marked as used by the render.
    fn udf_set_as_selected(&mut self, shader: u32);
}

/// One recorded call.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Status(Vec<u8>, i32),
    Usleep(u32),
    Print(Vec<u8>),
    GrafNodeInfo { render: bool },
    MaterialAllocate(u32),
    UpdateSonState { father: u32, model: u32, flag: i16 },
    ExpandAll { master: u32, copy: u32 },
    ExpandUpdateTransfo { master: u32, instance: u32, copy: u32 },
    NbTriangles(u32),
    WarnUv(Option<Vec<u8>>),
    EffectByIcon(u32),
    TopoSort,
    UnToldRelation(i16, i16, i16),
    SceneUpdateItems(u32, u32),
    WindowHilite,
    PictureElemRemove(u32),
    UdfResetActive,
    UdfIsActive(u32),
    UdfSetAsSelected(u32),
}

/// An [`Outside`] that writes every call down and answers from what it was given.
#[derive(Default)]
pub struct Recorder {
    pub calls: Vec<Call>,
    /// The materials `SI_materialAllocate` hands out, first first. When they run out a
    /// zeroed record is made.
    pub materials: Vec<u32>,
    /// The copy `SI_modelExpandAllWithoutGeometryAbsolute` answers for a master.
    pub expansions: Vec<(u32, u32)>,
    /// What `SI_modelNbTriangles` answers for a node; 0 for the others.
    pub triangles: Vec<(u32, i32)>,
    /// The nodes that are icons of installed effects, with the word behind +0x104.
    pub effects: Vec<(u32, Option<i32>)>,
    pub hilite_camera: Option<u32>,
    pub windows: Vec<(i16, u32)>,
    /// What `SI_udfIsActive` answers for a shader record; 0 for the others.
    pub active: Vec<(u32, i16)>,
}

impl Outside for Recorder {
    fn status_display_message(&mut self, text: &[u8], level: i32) {
        self.calls.push(Call::Status(text.to_vec(), level));
    }

    fn usleep(&mut self, micros: u32) {
        self.calls.push(Call::Usleep(micros));
    }

    fn print(&mut self, text: &[u8]) {
        self.calls.push(Call::Print(text.to_vec()));
    }

    fn ed_get_elements_graf_node_info(&mut self, _d: &mut Dict, render: bool) {
        self.calls.push(Call::GrafNodeInfo { render });
    }

    fn material_allocate(&mut self, heap: &mut Heap) -> u32 {
        let h = if self.materials.is_empty() {
            heap.alloc(crate::world::Obj::Material(crate::world::Image::new(crate::world::mat::SIZE)))
        } else {
            self.materials.remove(0)
        };
        self.calls.push(Call::MaterialAllocate(h));
        h
    }

    fn model_update_son_state(&mut self, _heap: &mut Heap, father: u32, model: u32, flag: i16) {
        self.calls.push(Call::UpdateSonState { father, model, flag });
    }

    fn model_expand_all_without_geometry_absolute(&mut self, _heap: &mut Heap, _current: &mut Dict, master: u32) -> u32 {
        let at = self.expansions.iter().position(|(m, _)| *m == master);
        let copy = at.map_or(0, |i| self.expansions.remove(i).1);
        self.calls.push(Call::ExpandAll { master, copy });
        copy
    }

    fn expand_update_transfo(&mut self, _heap: &mut Heap, _current: &mut Dict, master: u32, instance: u32, copy: u32) {
        self.calls.push(Call::ExpandUpdateTransfo { master, instance, copy });
    }

    fn model_nb_triangles(&mut self, _heap: &Heap, model: u32) -> i32 {
        self.calls.push(Call::NbTriangles(model));
        self.triangles.iter().find(|(m, _)| *m == model).map_or(0, |(_, n)| *n)
    }

    fn print_warn_uv(&mut self, name: Option<&[u8]>) {
        self.calls.push(Call::WarnUv(name.map(|n| n.to_vec())));
    }

    fn cus_get_effect_by_icon(&mut self, model: u32) -> Option<Option<i32>> {
        self.calls.push(Call::EffectByIcon(model));
        self.effects.iter().find(|(m, _)| *m == model).map(|(_, w)| *w)
    }

    fn topo_sort(&mut self, _current: &mut Dict) {
        self.calls.push(Call::TopoSort);
    }

    fn db_check_extra_untold_relation(&mut self, _current: &mut Dict, a: i16, b: i16, c: i16) {
        self.calls.push(Call::UnToldRelation(a, b, c));
    }

    fn scene_update_items(&mut self, time: f32, camera: u32) {
        self.calls.push(Call::SceneUpdateItems(time.to_bits(), camera));
    }

    fn window_hilite_camera(&mut self) -> Option<u32> {
        self.calls.push(Call::WindowHilite);
        self.hilite_camera
    }

    fn template_windows(&mut self) -> Vec<(i16, u32)> {
        self.windows.clone()
    }

    fn picture_elem_remove(&mut self, _d: &mut Dict, picture: u32) {
        self.calls.push(Call::PictureElemRemove(picture));
    }

    fn udf_reset_active(&mut self) {
        self.calls.push(Call::UdfResetActive);
    }

    fn udf_is_active(&mut self, shader: u32) -> i16 {
        self.calls.push(Call::UdfIsActive(shader));
        self.active.iter().find(|(s, _)| *s == shader).map_or(0, |(_, a)| *a)
    }

    fn udf_set_as_selected(&mut self, shader: u32) {
        self.calls.push(Call::UdfSetAsSelected(shader));
    }
}
