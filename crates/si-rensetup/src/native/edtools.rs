// SPDX-License-Identifier: MIT
//! `edtools.c`, `model.c` and `mat_global.c`: the small routines between the scene graph's
//! list, the dictionary and the renderer (finding 0023, section 18.5). They are
//! `si-scene`'s now (`si_scene::edtools`, where they are cited), because the loader builds
//! the list and needs them; these are the same signatures with this package's outside
//! world, so that the command, the world and `rensetup-emucheck` call them as before. The
//! first pass has `SI_elemdictAddModel` on stores ([`crate::edlocal`]).
//!
//! Nothing in this file reproduces a function of the original.

use super::outside::Outside;
use si_mem::{FnPtr, Mem, Ptr32};
use si_types::dict::{Elem, ElemDict};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// This package's outside world as `si-scene`'s routines call out.
pub struct SceneSide<'a, O: Outside + ?Sized>(pub &'a mut O);
impl<O: Outside + ?Sized> si_scene::edtools::ByAddr for SceneSide<'_, O> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        self.0.call(m, FnPtr::new(f), args)
    }
}

/// `SI_edGetElemFromModel (0x080fcf20)`: `si_scene::edtools::ed_get_elem_from_model`.
pub fn ed_get_elem_from_model<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, index: Ptr32<i32>) -> Ptr32<Elem> {
    si_scene::edtools::ed_get_elem_from_model(&mut SceneSide(o), m, model, index)
}

/// `SI_edFindSon (0x080fcec0)`.
pub fn ed_find_son<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, elem: Ptr32<Elem>) -> Ptr32<Elem> {
    si_scene::edtools::ed_find_son(&mut SceneSide(o), m, elem)
}

/// `SI_edFindNextSon (0x080fcef0)`.
pub fn ed_find_next_son<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, elem: Ptr32<Elem>) -> Ptr32<Elem> {
    si_scene::edtools::ed_find_next_son(&mut SceneSide(o), m, elem)
}

/// `SI_edGetElemFromTexture2D (0x080fd3e0)`.
pub fn ed_get_elem_from_texture2d<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, texture: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    si_scene::edtools::ed_get_elem_from_texture2d(&mut SceneSide(o), m, texture, index)
}

/// `SI_elemdictUpdateModelHierarchy (0x080fd698)`.
pub fn elemdict_update_model_hierarchy<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, dict: Ptr32<ElemDict>) {
    si_scene::edtools::elemdict_update_model_hierarchy(&mut SceneSide(o), m, dict)
}

/// `SI_elemdictAddModel (0x081017f4)`.
pub fn elemdict_add_model<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, dict: Ptr32<ElemDict>, model: Ptr32<Model>, gn: Ptr32<GrafNode>, unique: i16) {
    si_scene::edtools::elemdict_add_model(&mut SceneSide(o), m, dict, model, gn, unique)
}

/// `SI_edAddModel (0x08101ad4)`.
pub fn ed_add_model<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, gn: Ptr32<GrafNode>, unique: i16) {
    si_scene::edtools::ed_add_model(&mut SceneSide(o), m, model, gn, unique)
}

/// `SI_modelNbSubModels (0x0846d5ec)`.
pub fn model_nb_sub_models(m: &Mem, node: Ptr32<Model>) -> i32 {
    si_scene::edtools::model_nb_sub_models(m, node)
}

/// `SI_lightGetGrafType (0x082800fc)`.
pub fn light_get_graf_type(m: &Mem, light: u32) -> i32 {
    si_scene::edtools::light_get_graf_type(m, light)
}
