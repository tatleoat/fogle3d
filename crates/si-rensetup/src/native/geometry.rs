// SPDX-License-Identifier: MIT
//! `geometry.c`: the per-frame refresh of the copies the render commands chain for
//! instances.
//!
//! `SI_sequenceRender` and `SI_frameRender` make, once, a copy of each instance's master
//! hierarchy (without geometry) under a dummy root with algorithm 15. Before every frame
//! the renderer walks those dummy roots and, for each, copies the master's present
//! geometry pointers and local transformations into its copy, then rebuilds the copy's
//! matrices. Nested instances inside a master are followed by the recursion.
//!
//! The dictionary, the model library and the abort checks are other packages: they are
//! called through [`Outside`].

use super::current_dict;
use super::globals::*;
use super::instance::{instance_copy_transformation, model_master_from_instance};
use super::outside::*;
use super::rec::ALGO_INSTANCE;
use si_mem::{field, Mem, Ptr32};
use si_types::dict::Elem;
use si_types::model::Model;
use si_x87::x87::F80;

type Node = Ptr32<Model>;

/// The algorithm number the master carries while it is given the instance's place.
const ALGO_SWAP: i32 = 0x1e;

/// What one level of the recursion takes from the image's stack. The original's frames are
/// smaller and its stack larger (see `instance.rs`); here the walk ends when the image's
/// stack is used up, with the fault `Mem::local` records.
const FRAME: u32 = 0x200;

/// A float loaded into the unit and stored again: the same bits unless it is a signalling
/// NaN, which comes back quiet.
fn through(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// Refreshes the copies under the renderer's chain of dummy roots
/// [soft 0x082739e0 si_traverseExpandedInstance].
///
/// For each root of the chain (through the brother word): the abort check; in the program
/// (not the stand-alone renderer) the tablet's pending button event is fetched when the
/// tablet is active, and the buttons 0x66 and 7 are looked at: either of them, or the
/// tablet's last button being 0x66, calls `SI_voluntaryExit(4, whether SI_ren + 0x178 is
/// 0)`, which does not return in the original (here the walk would go on, so only the
/// calls up to it are the original's). A root that is an instance dummy (algorithm 15) has
/// its copy as son; the copy's element number names the instance in the current
/// dictionary's models chapter; when the instance has a master, `SI_expandUpdateTransfo
/// (master, instance, copy)` follows. A missing element is read through a null pointer, as
/// in the original.
pub fn traverse_expanded_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, root: Node) {
    let mut node = root;
    while !node.is_null() {
        o.call(m, SI_CUS_CHECK_RENDER_ABORT, &[]);
        if m[SI_STANDALONE_RENDERER] == 0 {
            if m[SI_TABLET_ACTIVE] != 0 {
                o.call(m, SI_TABLET_GET_BUTTON_UP_EVENT, &[]);
            }
            let stop = o.call(m, GETBUTTON, &[0x66]) != 0 || m[SI_TABLET_BUTTON] == 0x66 || o.call(m, GETBUTTON, &[7]) != 0;
            if stop {
                let flag = (m.u16(SI_REN.0 + 0x178) == 0) as u32;
                o.call(m, SI_VOLUNTARY_EXIT, &[4, flag]);
            }
        }
        if m[node].algo_type == ALGO_INSTANCE {
            let copy = m[node].son;
            let number = m[copy].elem_id as u32;
            let dict = current_dict(m);
            let chapter = m[SI_CID].model as u32;
            let elem: Ptr32<Elem> = Ptr32::new(o.call(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chapter, number]));
            let instance: Node = m[elem].data.cast();
            let master = model_master_from_instance(o, m, instance);
            if !master.is_null() {
                expand_update_transfo(o, m, master, instance, copy);
            }
        }
        node = m[node].brother;
    }
}

/// `expandUpdateTransfo(master, instance, copy)`, then the copy's matrices are rebuilt as
/// a root's: `SI_modelUpdateSonState(NULL, copy, 1)` [soft 0x08276364 SI_expandUpdateTransfo].
pub fn expand_update_transfo<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, instance: Node, copy: Node) {
    expand_update_transfo_walk(o, m, master, instance, copy);
    o.call(m, SI_MODEL_UPDATE_SON_STATE, &[0, copy.0, 1]);
}

/// Copies a master's present state into the copy of an instance
/// [soft 0x08276394 expandUpdateTransfo]; answers the last node of the copy it visited.
///
/// The copy takes the instance's place in space (`SI_instanceCopyTransformation`), the
/// instance's `visible` word and the master's geometry pointer. The master's local
/// scaling, angles and translation are kept in locals (each float through the unit); the
/// master, with its algorithm word set to 0x1e meanwhile, is given the instance's global
/// placement (`SI_modelCopyTransformation(instance, master)`). Then both hierarchies are
/// walked together with `SI_modelNextSon` (the copy's walk starts at the copy itself as
/// "no previous node"): a master node that is an instance is followed into its own
/// master's hierarchy with the copy's node as the new copy (the walk ends if it has no
/// master); any other node gives its copy the nine local words (scaling, angles,
/// translation) and the geometry pointer, as plain words. The walk ends when either
/// hierarchy does. Last the master gets its saved local values back through
/// `SI_modelSetState` (algorithm 0x1e meanwhile again) and its own algorithm word.
pub fn expand_update_transfo_walk<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, instance: Node, copy: Node) -> Node {
    let sp = m.sp();
    if m.local(FRAME) == 0 {
        m.set_sp(sp);
        return Node::NULL;
    }
    instance_copy_transformation(o, m, instance, copy);
    let visible = m[instance].visible;
    m[copy].visible = visible;
    let definition = m[master].definition;
    m[copy].definition = definition;

    let scal = m.local(12);
    let rot = m.local(12);
    let trans = m.local(12);
    let (ms, mr, mt) = (field!(master, Model, scal).0, field!(master, Model, vrot).0, field!(master, Model, trans).0);
    for i in 0..3u32 {
        let v = through(m.f32(ms + 4 * i));
        m.set_f32(scal + 4 * i, v);
    }
    for i in 0..3u32 {
        let v = through(m.f32(mr + 4 * i));
        m.set_f32(rot + 4 * i, v);
    }
    for i in 0..3u32 {
        let v = through(m.f32(mt + 4 * i));
        m.set_f32(trans + 4 * i, v);
    }
    let algo = m[master].algo_type;
    m[master].algo_type = ALGO_SWAP;
    o.call(m, SI_MODEL_COPY_TRANSFORMATION, &[instance.0, master.0]);
    m[master].algo_type = algo;

    let mut p2: Node = Ptr32::new(o.call(m, SI_MODEL_NEXT_SON, &[master.0, 0]));
    let mut p3 = copy;
    while !p2.is_null() {
        let previous = if p3 == copy { Node::NULL } else { p3 };
        p3 = Ptr32::new(o.call(m, SI_MODEL_NEXT_SON, &[copy.0, previous.0]));
        if p3.is_null() {
            break;
        }
        if m[p2].algo_type == ALGO_INSTANCE {
            let inner = model_master_from_instance(o, m, p2);
            if inner.is_null() {
                break;
            }
            p3 = expand_update_transfo_walk(o, m, inner, p2, p3);
        } else {
            for off in [0x100, 0x104, 0x108, 0x10c, 0x110, 0x114, 0x158, 0x15c, 0x160, 0x48] {
                let w = m.u32(p2.0 + off);
                m.set_u32(p3.0 + off, w);
            }
        }
        p2 = Ptr32::new(o.call(m, SI_MODEL_NEXT_SON, &[master.0, p2.0]));
    }
    m[master].algo_type = ALGO_SWAP;
    o.call(m, SI_MODEL_SET_STATE, &[master.0, scal, rot, trans]);
    m[master].algo_type = algo;
    m.set_sp(sp);
    p3
}
