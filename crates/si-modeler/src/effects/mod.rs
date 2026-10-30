// SPDX-License-Identifier: MIT
//! The render-path callees the modelling effects reach (package geomupd2, finding 0034 "geomupd2"):
//! the update routines of the extrusion, inverse, four-sided and trim effects and the index
//! modifications. One module per original source file.
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`func`] | `func.c` | `SI_adjustGnFunc` |
//! | [`index`] | `index.c` | the index of modifications (allocate, free, propagate) |
//! | [`lin_patch`] | `extrusion.c` | a face extruded along a polyline path into patches or a mesh (unit lin1) |
//! | [`mod_effect`] | `mod_effect.c` | `SI_inverseUpdate`, its relation test and `SI_modelSymmetry` |
//! | [`mod_global`] | `mod_global.c` | `SI_updateScalingMethod` |
//!
//! A routine of the original that is not ported (the status line, the mouse, the sleep, the
//! element-dictionary glue) is called through [`si_mem::Outside`], written `name (0xADDRESS)`.

use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::model_image::{Callee as ModelCallee, Outside as ModelOutside};

pub mod extrude_linear;
pub mod extrude_model;
pub mod extrude_nurbs;
pub mod extrude_patch;
pub mod extrusion3;
pub mod four_patch;
pub mod four_nurbs;
pub mod func;
pub mod index;
pub mod lin_nurbs;
pub mod lin_patch;
pub mod mod_effect;
pub mod mod_global;
pub mod revolution;
pub mod revolve_four;
pub mod trim_dial;

/// `SI_statusDisplayMessage (0x083cca94)`, `status.c`: a text for the status line (UI, a hook).
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `SI_edRelaxRelFromDelModel (0x080ff5f0)`, `edtools.c`.
pub const SI_ED_RELAX_REL_FROM_DEL_MODEL: Routine = Routine::new("SI_edRelaxRelFromDelModel", 0x080f_f5f0);
/// `sleep (0x080c4468)`, the C library's.
pub const SLEEP: Routine = Routine::new("sleep", 0x080c_4468);
/// `SI_mdlStatusMessages (0x0818129c)`, `modeler.c`: the modeller's status-line text (UI, a hook).
pub const SI_MDL_STATUS_MESSAGES: Routine = Routine::new("SI_mdlStatusMessages", 0x0818_129c);
/// `SI_refreshAll (0x08325dc8)`, `draw.c`: a redraw of every view (UI, a hook).
pub const SI_REFRESH_ALL: Routine = Routine::new("SI_refreshAll", 0x0832_5dc8);
/// `SI_edGetElemFromModel (0x080fcf20)`, `edtools.c`.
pub const SI_ED_GET_ELEM_FROM_MODEL: Routine = Routine::new("SI_edGetElemFromModel", 0x080f_cf20);

/// `SI_procData` (a pointer to two ints: the element dictionary and the graph list in use).
pub(crate) const SI_PROC_DATA: u32 = 0x0867_9314;
/// `SI_grafLists` (a pointer to the table of display lists, 0x24 bytes each).
pub(crate) const SI_GRAF_LISTS: u32 = 0x0867_9498;
/// `SI_elemDicts` (a pointer to the table of element dictionaries, 0x70 bytes each).
pub(crate) const SI_ELEM_DICTS: u32 = 0x0867_931c;
/// The MODELS chapter number (`SI_cid.MODELS`).
pub(crate) const CID_MODELS: u32 = 0x0878_1680;

/// The display list of the current scene: `SI_grafLists + 0x24 * SI_procData[1]`.
pub(crate) fn graf_list(m: &Mem) -> Ptr32<si_types::scene::GrafList> {
    Ptr32::new(m.u32(SI_GRAF_LISTS).wrapping_add((m.i32(m.u32(SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24)))
}

/// The element dictionary in use: `SI_elemDicts + 0x70 * SI_procData[0]` (the original forms
/// it as `(n * 8 - n) * 0x10`).
pub(crate) fn elem_dict(m: &Mem) -> u32 {
    let n = m.i32(m.u32(SI_PROC_DATA));
    m.u32(SI_ELEM_DICTS).wrapping_add(((n.wrapping_mul(8)).wrapping_sub(n) as u32).wrapping_mul(0x10))
}

/// The routines of `si-model` call out through their own trait; this hands them to ours.
pub(crate) struct ModelS<'a>(pub(crate) &'a mut dyn Outside);
impl ModelOutside for ModelS<'_> {
    fn call(&mut self, m: &mut Mem, callee: &ModelCallee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

/// The same for `si-view`.
pub(crate) struct ViewS<'a>(pub(crate) &'a mut dyn Outside);
impl si_view::Outside for ViewS<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(routine.name, routine.addr), args).eax
    }
}

/// The same for `si-deform` (its random numbers are never asked for here).
pub(crate) struct DeformS<'a>(pub(crate) &'a mut dyn Outside);
impl si_deform::Outside for DeformS<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_deform::Routine, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(routine.name, routine.addr), args).eax
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

/// `SI_showActivity (0x083d1a60)`, `ui_misc.c`: the busy cursor (UI, a hook).
pub const SI_SHOW_ACTIVITY: Routine = Routine::new("SI_showActivity", 0x083d_1a60);

/// The same for `si-spline`.
pub(crate) struct SplineS<'a>(pub(crate) &'a mut dyn Outside);
impl si_spline::spline_image::Outside for SplineS<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_spline::spline_image::Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}
