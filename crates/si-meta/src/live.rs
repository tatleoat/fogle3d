// SPDX-License-Identifier: MIT
//! The first integration of `metaball.c`'s hooks: [`MetaLive`] answers the three
//! transformation setters through `si-model` and hands every other routine to the world
//! it wraps.
//!
//! Nothing here reproduces a function of the original; the setters are cited where they
//! are ported (`si_model::model_image::setters`: [soft 0x08463a74 SI_modelSetScaling],
//! [soft 0x08463f54 SI_modelSetRotation], [soft 0x084640cc SI_modelSetTranslation]).
//! They are what `SI_readMetaCorp` and `metaAdoptShape` use to give a loaded ball its
//! place, so a scene with a `.med` file or an adopted shape depends on them.
//!
//! The other hooks stay with the wrapped world: `SI_edFindFcv`, `SI_fcurveNextKey`,
//! `SI_fcurvePreviousKey` and `SI_windowGetNext` are ported on other structures (the
//! `si-motion` element, `si-fcurve`'s own key vector, `si-ui`'s window list), not on the
//! shared image these functions read, and `SI_schematicSort` / `SI_refreshAll` are
//! unported (windows and drawing).

use si_mem::{Mem, Outside, Ptr32, Ret, Routine};
use si_model::model_image::{self, setters, Callee};

/// The routine addresses answered here.
const SET_SCALING: u32 = 0x08463a74;
const SET_ROTATION: u32 = 0x08463f54;
const SET_TRANSLATION: u32 = 0x084640cc;

/// `si-model`'s outside world over a `si-mem` one: the same words, the same answer.
struct Forward<'a>(&'a mut dyn Outside);

impl model_image::Outside for Forward<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

/// An outside world that runs the model setters itself and passes the rest on.
pub struct MetaLive<O: Outside>(pub O);

impl<O: Outside> Outside for MetaLive<O> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        let setter = match routine.addr {
            SET_SCALING | SET_ROTATION | SET_TRANSLATION if args.len() == 4 => routine.addr,
            _ => return self.0.call(m, routine, args),
        };
        let node = Ptr32::new(args[0]);
        let (v, mode, geom) = (args[1], args[2] as i32, args[3] as i16);
        let mut fwd = Forward(&mut self.0);
        match setter {
            SET_SCALING => setters::set_scaling(m, &mut fwd, node, v, mode, geom),
            SET_ROTATION => setters::set_rotation(m, &mut fwd, node, v, mode, geom),
            _ => setters::set_translation(m, &mut fwd, node, v, mode, geom),
        }
        Ret::NONE
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.0.print(stream, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metaball::MetaCalls;
    use si_mem::Trace;

    #[test]
    fn translation_hook_runs_the_setter_and_forwards_the_rest() {
        let mut m = Mem::new();
        let node = 0x6100_0000u32;
        let v = 0x6100_2000u32;
        for (i, f) in [1.5f32, -2.0, 3.25].iter().enumerate() {
            m.set_u32(v + 4 * i as u32, f.to_bits());
        }
        let mut direct = Mem::new();
        for (i, f) in [1.5f32, -2.0, 3.25].iter().enumerate() {
            direct.set_u32(v + 4 * i as u32, f.to_bits());
        }
        let mut w = MetaLive(Trace::new());
        w.si_model_set_translation(&mut m, node, v, 0, 0);
        setters::set_translation(&mut direct, &mut model_image::NoOutside, Ptr32::new(node), v, 0, 0);
        for off in (0..0x440u32).step_by(4) {
            assert_eq!(m.u32(node + off), direct.u32(node + off), "node +{off:#x}");
        }
        w.si_refresh_counter(&mut m);
        assert_eq!(w.0.names().last().copied(), Some("SI_refreshCounter"));
    }
}
