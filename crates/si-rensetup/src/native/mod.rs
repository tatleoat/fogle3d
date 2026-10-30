// SPDX-License-Identifier: MIT
//! **Compared with the emulated original** (finding 0023, sections 17 and 18): every
//! routine of this module that carries a citation `[soft 0xADDRESS name]` was run against
//! the original routine stepped through in the emulation harness, on made-up scenes laid
//! out in the image, and agrees in every compared case (`rensetup-emucheck`, the vectors of
//! `re/tools/si-rensetup/emu_gen_native.py`, `emu_gen_env.py` and `emu_gen_pass4.py`).
//!
//! The link from a loaded scene to the native renderer's entry, on the shared memory
//! image (findings 0023, sections 16 to 18, and 0024).
//!
//! Two commands hand the scene graph's own nodes to the renderer: the Matter module's
//! preview (`SI_frameRender`) and the Render dialogue (`SI_sequenceRender`). Both copy the
//! render controls to the renderer's own, set every model's `visible` word from the scene
//! graph's list, chain a dummy root over each hierarchy and a copy of the master's
//! hierarchy for each instance, chain the lights, and call `SI_image (0x08257a70)`.
//!
//! Everything here takes the image and the arguments the original takes; globals are at
//! their addresses ([`globals`]); records are `si-types`' layouts or [`rec`]'s; the
//! original's allocation calls are made on `si-mem`'s heap. Calls into other packages go
//! through the one trait of [`outside`]; [`world`] is the implementation that reaches the
//! ports other crates have on the image.
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`globals`] | | the globals, message texts and tables of `.data`, at their addresses |
//! | [`rec`] | | records and named offsets `si-types` does not have |
//! | [`outside`] | | the trait every callee of another package is behind |
//! | [`instance`] | `instance.c` (all 27 functions) | instances and masters: the relation, the copy of a master's hierarchy without geometry, expanding an instance into real models, swapping a master, the cycle test |
//! | [`geometry`] | `geometry.c` (3 of 59) | the per-frame refresh of the copies chained for instances |
//! | [`matter`] | `matter.c` (8 of 56) | the preview command, the sub-region's pixel values, and the render controls' start-up state |
//! | [`rnd_dial`] | `rnd_dial.c` (2 of 58) | the render command and its sub-region question |
//! | [`renu_display`] | `renu_display.c` (all 3) | the render window: opened, closed, its switch reset |
//! | [`edtools`] | `edtools.c` (7), `model.c` (1), `mat_global.c` (1) | a model node and its element; the count of nodes below a node; a light's kind of graf node |
//! | [`envtools`] | `envtools.c` (8 of 39) | what the copy of a node keeps of its master's envelope |
//! | [`classic`] | `classic.c` (3 of 34) | the compensation records of the classic manipulation mode: allocate, copy, dispose |
//! | [`skeleton`] | `skeleton.c` (1 of 11) | the root of a node's chain |
//! | [`model_b`] | `model_b.c` (1 of 4), `settings.c` (1 of 51) | a new node; the copy of a node's algorithm |
//! | [`invkin`] | `invkin.c` (3 of 18), `algebra.c` (3) | the copy and disposal of an inverse kinematics record |
//! | [`world`] | | the program as far as it is on the image: every call by address handed to the port that exists for it (`si-dict`, `si-model`, `si-view`, `si-render`, `si-scene`, `si-motion`, this package) |
//! | [`motion`] | | the scene brought to the frame's time: `si-motion`'s `SI_sceneUpdateItems` with what it calls handed to the other crates' ports by the host method's name |
//! | [`bridge`] | | adapter: what a scene loaded by `si-scene` still lacks in the image |
//!
//! `graflist.c`'s `SI_findGlData` and the list's builder are `si-view`'s; this module calls
//! them (the port this package had in `native/graflist.rs` is gone).
//!
//! The first pass's modules of this crate (`crate::instance`, `crate::edlocal`, …) work on
//! stores of their own and are not used here.

pub mod bridge;
pub mod classic;
pub mod edtools;
pub mod envtools;
pub mod geometry;
pub mod globals;
pub mod instance;
pub mod invkin;
pub mod matter;
pub mod model_b;
pub mod motion;
pub mod outside;
pub mod pre_render;
pub mod preview;
pub mod prv_main;
pub mod rec;
pub mod rens_main;
pub mod renu_display;
pub mod renu_filepath;
pub mod renu_subregion;
pub mod rnd_dial;
pub mod skeleton;
pub mod symmetry;
pub mod symmetry_vector;
pub mod sys_render;
pub mod world;

#[cfg(test)]
mod tests;

use globals::{SI_ELEM_DICTS, SI_GRAF_LISTS, SI_PROC_DATA};
use si_mem::{Mem, Ptr32};
use si_types::dict::ElemDict;
use si_types::scene::GrafList;

/// The scene's graf list, as every routine here names it: `SI_grafLists` plus 0x24 times
/// the index in the process record.
pub fn scene_graf_list(m: &Mem) -> Ptr32<GrafList> {
    let index = m[m[SI_PROC_DATA]].graf_list;
    m[SI_GRAF_LISTS].add(index)
}

/// The current dictionary: `SI_elemDicts` plus 0x70 times the index in the process record.
pub fn current_dict(m: &Mem) -> Ptr32<ElemDict> {
    let index = m[m[SI_PROC_DATA]].dict;
    m[SI_ELEM_DICTS].add(index)
}

use outside::{Outside, SI_MODEL_ALLOCATE};
use rec::{flag, ALGO_INSTANCE};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// A dummy root as both render commands make it: a new node (`SI_modelAllocate(NULL)`) of
/// kind −1 with `visible` 1.
fn dummy_root<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> Ptr32<Model> {
    let dummy: Ptr32<Model> = Ptr32::new(o.call(m, SI_MODEL_ALLOCATE, &[0]));
    m[dummy].kind = -1;
    m[dummy].visible = 1;
    dummy
}

/// What both render commands do for one root model of the scene's graf list (`gn`, kind
/// 2): the part of `SI_frameRender` and of `SI_sequenceRender` that is the same
/// instructions in both, kept in one place. `visible` is each command's own test of a
/// graf node (see the callers).
///
/// The instance marks are cleared. A dummy root is made. When the root is an instance
/// with a master: it is marked visited, its master's hierarchy is copied (deep), the copy
/// takes the instance's "inverse" flag (bit 8) and what `SI_expandCopyInfoFromInstance`
/// gives it, the dummy becomes an instance root (algorithm 15) over the copy and is
/// chained, and a second dummy is made over the instance itself. Then every instance
/// still unvisited inside the hierarchy gets a dummy of its own; with a master, the same
/// copy is made (its `visible` from the instance's own graf node, which is read through a
/// null pointer when there is none) and that dummy chained; without one the dummy is
/// left behind, unchained. Last, the dummy over the root itself is chained.
fn chain_root<O: Outside + ?Sized>(
    o: &mut O,
    m: &mut Mem,
    gn: Ptr32<GrafNode>,
    roots: &mut Ptr32<Model>,
    visible: impl Fn(&Mem, Ptr32<GrafNode>) -> i16,
) {
    instance::instance_initialize_visit(m);
    let mut found = false;
    let mut dummy = dummy_root(o, m);
    let node: Ptr32<Model> = m[gn].data.cast();
    let hang = |o: &mut O, m: &mut Mem, dummy: Ptr32<Model>, instance: Ptr32<Model>, master: Ptr32<Model>, v: i16, roots: &mut Ptr32<Model>| {
        let copy = instance::model_expand_all_without_geometry(o, m, master, Ptr32::NULL, v, 1);
        if m[instance].flags_08 & flag::INVERSE != 0 {
            m[copy].flags_08 |= flag::INVERSE;
        }
        instance::expand_copy_info_from_instance(o, m, instance, copy);
        m[dummy].algo_type = ALGO_INSTANCE;
        m[dummy].son = copy;
        m[copy].father = dummy;
        m[dummy].brother = *roots;
        *roots = dummy;
    };
    if m[node].algo_type == ALGO_INSTANCE {
        m[node].flags_08 |= flag::VISITED;
        let master = instance::model_master_from_instance(o, m, node);
        if !master.is_null() {
            let v = visible(m, gn);
            hang(o, m, dummy, node, master, v, roots);
            dummy = dummy_root(o, m);
            m[dummy].son = node;
        }
    }
    let kept = dummy;
    loop {
        let inst = instance::find_instance_in_hierarchy(m, node, Ptr32::NULL, 0);
        if inst.is_null() {
            break;
        }
        found = true;
        m[inst].flags_08 |= flag::VISITED;
        dummy = dummy_root(o, m);
        let master = instance::model_master_from_instance(o, m, inst);
        if !master.is_null() {
            // SI_findGlData (0x0832ee68) is `si-view`'s
            let own = si_view::graflist::find_gl_data(m, scene_graf_list(m), inst.cast(), 0);
            let v = visible(m, own);
            hang(o, m, dummy, inst, master, v, roots);
        }
    }
    if found {
        dummy = kept;
    }
    m[dummy].son = node;
    m[dummy].brother = *roots;
    *roots = dummy;
}

/// Frees the chain of dummy roots after the render, as both commands do: each dummy's
/// name, then the dummy. The copies under the instance dummies are not freed here.
fn free_dummies(m: &mut Mem, mut roots: Ptr32<Model>) {
    while !roots.is_null() {
        let dummy = roots;
        roots = m[dummy].brother;
        let name = m[dummy].name;
        if !name.is_null() {
            m.free(name.0);
            m[dummy].name = Ptr32::NULL;
        }
        m.free(dummy.0);
    }
}

/// A light of the graf list (kinds 6, 8, 0x13, 0x14) is switched on when its display type
/// is 2 and put at the head of the chain handed to the renderer.
fn chain_light(m: &mut Mem, gn: Ptr32<GrafNode>, lights: &mut u32) {
    let light = m[gn].data.0;
    let on = (m[gn].wire_type == 2) as u16;
    m.set_u16(light + rec::light::ON, on);
    m.set_u32(light + rec::light::NEXT, *lights);
    *lights = light;
}

/// The graf node kinds of the four sorts of light.
fn is_light(kind: i32) -> bool {
    matches!(kind, 6 | 8 | 0x13 | 0x14)
}
