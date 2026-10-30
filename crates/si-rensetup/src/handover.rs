// SPDX-License-Identifier: MIT
//! What the native renderer is handed (finding 0023, section 1.1).
//!
//! `SI_image(camera, name, name, lights, roots)` (0x08257a70) is called by `SI_frameRender`
//! (0x082903bc, `matter.c`) and `SI_sequenceRender` (0x082a2000, `rnd_dial.c`). Neither is
//! ported; both build the same arguments from the scene graph, and no render dictionary is
//! involved. [`NativeRender`] names the call; [`from_dictionary`] builds its arguments from a
//! scene's dictionary instead of the graph list (`graflist.c`, not ported), in the
//! dictionary's order, so the *order* of lights and roots is not the original's (the original
//! chains them last first, in graph order).

use crate::world::{Env, Heap};
use si_scene::dict::Dict;
use si_types::dict::ELEM_VALID;

/// The arguments of `SI_image`, with the two chains as lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandOver {
    /// The scene's camera (`SI_procData`→2).
    pub camera: u32,
    /// The lights, each with its "on" word (`light` +0x176: the display type is "visible").
    /// The original links them through +0x140.
    pub lights: Vec<(u32, bool)>,
    /// The scene's root nodes. The original hangs each under a dummy node of kind -1 and
    /// links the dummies through their brother pointers; the renderer walks down from there.
    pub roots: Vec<u32>,
    /// Per model node, the `visible` word (+0x2b8) the callers set before the call: the
    /// display type of the node's graph node is not "hidden".
    pub visible: Vec<(u32, bool)>,
    /// Root nodes that are instances (algorithm 15): the original renders a copy of the
    /// master's hierarchy in their place (`SI_modelExpandAllWithoutGeometry`, `instance.c`).
    pub instance_roots: Vec<u32>,
}

/// The native renderer's entry.
pub trait NativeRender {
    /// `SI_image`: renders the frames `SI_ren` (a copy of `SI_defaultControl`) asks for and
    /// answers 0, or 0x7f for a failure or an abort.
    fn image(&mut self, heap: &mut Heap, hand_over: &HandOver) -> i32;
}

/// The hand-over built from a scene's dictionary: every light element in use with a record,
/// every model element in use with a node; a model's display type is its element's (+0x20,
/// which `SI_edGetElementsGrafNodeInfo` keeps equal to the graph node's).
pub fn from_dictionary(scene: &Dict, env: &Env) -> HandOver {
    let c = env.p.cid;
    let mut h = HandOver { camera: env.g.scene_camera, ..HandOver::default() };
    for e in &scene.ed.lists[c.lig as usize].elems {
        let light = e.rec.data.0;
        if e.rec.status == ELEM_VALID && env.heap.image(light).is_some() && !h.lights.iter().any(|(l, _)| *l == light) {
            h.lights.push((light, e.rec.wire_type == 2));
        }
    }
    for e in &scene.ed.lists[c.model as usize].elems {
        let Some(m) = env.heap.model(e.rec.data.0) else { continue };
        if e.rec.status != ELEM_VALID {
            continue;
        }
        h.visible.push((e.rec.data.0, e.rec.wire_type != 0));
        if m.father.is_null() {
            h.roots.push(e.rec.data.0);
            if m.algo_type == crate::world::algo::INSTANCE {
                h.instance_roots.push(e.rec.data.0);
            }
        }
    }
    h
}
