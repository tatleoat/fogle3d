// SPDX-License-Identifier: MIT
//! The update functions of the effect models whose geometry is made by a relation: skinning
//! (`cross.c`), extraction (`curve.c`) and the lattice / surface-tangent routines of `deformation.c`,
//! with the callees they need that no other crate has. Finding 0034, section "geomupd1".
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`deformation`] | `deformation.c`, `edtools.c` | `SI_modelRemoveLattice`, `SI_surfaceValueAndTangent`, the dictionary delete flag |
//! | [`extraction`] | `curve.c` | `SI_extractionUpdate`, `createModelFromExtraction` |
//! | [`skin`] | `cross.c` | `SI_skinningUpdate` and the skins of splines and patches |

pub mod deformation;
pub mod extraction;
pub mod skin;

use si_mem::{Mem, Outside, Ptr32, Routine};

/// `SI_statusDisplayMessage (0x083cca94)`, the status line (not render; a hook).
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `SI_edRelaxRelFromDelModel (0x080ff5f0)`, the editor's relation clean-up (not render; a hook).
pub const SI_ED_RELAX_REL_FROM_DEL_MODEL: Routine = Routine::new("SI_edRelaxRelFromDelModel", 0x080f_f5f0);
/// `sleep (0x080c4468)`, the C library's.
pub const SLEEP: Routine = Routine::new("sleep", 0x080c_4468);
/// `SI_mdlStatusMessages (0x0818129c)`, the modeller's message line (not render; a hook).
pub const SI_MDL_STATUS_MESSAGES: Routine = Routine::new("SI_mdlStatusMessages", 0x0818_129c);

/// The graph-list globals: `SI_procData` and `SI_grafLists`.
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

struct ViewSide<'a>(&'a mut dyn Outside);
impl si_view::Outside for ViewSide<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(routine.name, routine.addr), args).eax
    }
}

/// `SI_updateGnFunc(SI_findGlData(SI_grafLists + SI_procData[1] * 0x24, model, 2))`: the display
/// node of a model brought up to date (the original loads both globals again at the use).
pub(crate) fn update_display_node(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let list = Ptr32::new(m.u32(SI_GRAF_LISTS).wrapping_add((m.i32(m.u32(SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24)));
    let shown = si_view::graflist::find_gl_data(m, list, Ptr32::new(model), 2);
    si_view::func::update_gn_func(m, &mut ViewSide(o), shown);
}

/// `SI_modelTouchGeometry (0x0846d48c)` (ported in `si-model`; a hook here, as the original's callers of it are).
pub const SI_MODEL_TOUCH_GEOMETRY: Routine = Routine::new("SI_modelTouchGeometry", 0x0846_d48c);
/// `SI_modelMultiplyDefinition (0x0846d3a4)` (ported in `si-model`; a hook here).
pub const SI_MODEL_MULTIPLY_DEFINITION: Routine = Routine::new("SI_modelMultiplyDefinition", 0x0846_d3a4);

pub(crate) fn touch_geometry(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    o.call(m, SI_MODEL_TOUCH_GEOMETRY, &[model]);
}
