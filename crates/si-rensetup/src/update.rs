// SPDX-License-Identifier: MIT
//! `rens_update.c`: a render dictionary brought to another frame.

use crate::instance::{instance_elem, update_inst_transforms};
use crate::outside::Outside;
use crate::propagate::{propagate_hidden_flag, propagate_select_flag};
use crate::world::Env;
use si_scene::dict::{Dict, ElemId};
use si_types::dict::{Elem, ELEM_VALID};

/// The thirteen words from +0x0c to +0x3f of an element: schematic position, width and
/// collapse state, wire colour, display type, selection state, the dope state and schematic
/// flags, the schematic box, and the pointer at +0x3c.
fn copy_display_words(from: &Elem, to: &mut Elem) {
    to.schem_x = from.schem_x;
    to.schem_y = from.schem_y;
    to.schem_width = from.schem_width;
    to.schem_collapse = from.schem_collapse;
    to.wire_color = from.wire_color;
    to.wire_type = from.wire_type;
    to.selection = from.selection;
    to.dope_state = from.dope_state;
    to.schem_flags = from.schem_flags;
    to.pad_2b = from.pad_2b;
    to.schem_bbox = from.schem_bbox;
    to.root_srt = from.root_srt;
}

/// Brings the display words of the render dictionary's models and lights up to date with
/// the scene's [soft 0x082c8470 copySchinfo]: slot for slot where both are in use; a render
/// model beyond the scene's count takes them from the scene element of the instance it
/// belongs to. (The original takes the render dictionary to have at least the scene's slots;
/// a slot it lacks is passed over here.)
pub fn copy_schinfo(scene: &Dict, ren: &mut Dict, env: &Env) {
    let cm = env.p.cid.model as usize;
    let n = scene.ed.lists[cm].elems.len();
    for chap in [cm, env.p.cid.lig as usize] {
        if chap == cm {
            for i in 0..n {
                let from = scene.ed.lists[cm].elems[i].rec;
                let Some(to) = ren.ed.lists[cm].elems.get_mut(i) else { break };
                if from.status == ELEM_VALID && to.rec.status == ELEM_VALID {
                    copy_display_words(&from, &mut to.rec);
                }
            }
            for i in n..ren.ed.lists[cm].elems.len() {
                let Some(inst) = instance_elem(i as i32, scene, ren, env) else { continue };
                let from = scene.slot(inst).unwrap().rec;
                let to = &mut ren.ed.lists[cm].elems[i].rec;
                if to.status == ELEM_VALID {
                    copy_display_words(&from, to);
                }
            }
        } else {
            for i in 0..scene.ed.lists[chap].elems.len() {
                let from = scene.ed.lists[chap].elems[i].rec;
                let Some(to) = ren.ed.lists[chap].elems.get_mut(i) else { break };
                if from.status == ELEM_VALID && to.rec.status == ELEM_VALID {
                    copy_display_words(&from, &mut to.rec);
                }
            }
        }
    }
}

/// Hands the hidden and select states of expanded instances down their hierarchies
/// [soft 0x082c85d0 si_renderUpdateInstanceHiddenAndSelect]: for every render model in use
/// that belongs to an instance and is a root, or lies among the first as many slots as the
/// scene has models, the display type and the selection state are given to everything under
/// it.
pub fn render_update_instance_hidden_and_select(scene: &Dict, ren: &mut Dict, env: &Env) {
    let cm = env.p.cid.model;
    let n = scene.ed.lists[cm as usize].rec.nb_elems;
    let mut i = 0;
    while (i as usize) < ren.ed.lists[cm as usize].elems.len() {
        let index = i;
        i += 1;
        if instance_elem(index, scene, ren, env).is_none() {
            continue;
        }
        let rec = ren.ed.lists[cm as usize].elems[index as usize].rec;
        if rec.status == ELEM_VALID && (rec.hier_level == 0 || index < n) {
            propagate_hidden_flag(ren, env, ElemId::new(cm, index));
            propagate_select_flag(ren, env, ElemId::new(cm, index));
        }
    }
}

/// What the mental ray translator calls before it writes a frame
/// [soft 0x082c8420 SI_renderSceneUpdate]: the scene is brought to the time
/// (`SI_sceneUpdateItems(time, 1, camera, 1)`), the display words are copied, the
/// transformations of the render dictionary's nodes are rebuilt, and the hidden and select
/// states of expanded instances are handed down.
pub fn render_scene_update(time: f32, camera: u32, scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside) {
    out.scene_update_items(time, camera);
    copy_schinfo(scene, ren, env);
    update_inst_transforms(scene, ren, env, out);
    render_update_instance_hidden_and_select(scene, ren, env);
}
