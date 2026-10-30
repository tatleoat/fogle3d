// SPDX-License-Identifier: MIT
//! `particle_util.c` of Softimage|3D 4.0 (0x084c0c10 … 0x084c2eb0): the pickers and the small helpers of the particle
//! animation (finding 0045). The two big builders (`particleCreateHrcAnimation`, `particleCreateShapeAnimation`) are
//! reached through hooks on [`ParticleCalls`] until they are ported. Particle status record (our names): +0x34 a flag
//! (non-zero: animate a model, not a mesh), +0xb4 source, +0xb8 destination and +0xbc third item. A model has its kind
//! at +0x30 (3 or 4: a mesh) and the matrix of 16 floats at +0x1c4.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_RD: u32 = 0x0878_15f0;

/// Waits for the user to pick a model [soft 0x084c0c10 particleGetMdl]: events are read until one that is neither a
/// pick (kind 5) nor kind 0, or whose code (short at +6) is 0x65, which gives 0. A pick on a graph node of kind 2 or 3
/// keeps its item and flashes the node (5 times); the loop ends when an item is held.
pub fn particle_get_mdl(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let mut held = 0u32;
    loop {
        let ev = o.si_get_event(m);
        let kind = m.i32(ev);
        if (kind != 5 && kind != 0) || m.u16(ev + 6) == 0x65 {
            return 0;
        }
        if kind == 5 {
            let node = o.si_gl_select(m, m.u32(ev + 0xc));
            if node != 0 && m.u32(node).wrapping_sub(2) < 2 {
                held = m.u32(node + 4);
                o.si_flash_object(m, node, 5);
            }
        }
        if held != 0 {
            return held;
        }
    }
}

/// Asks for the source mesh, the destination mesh and the third item of a particle animation
/// [soft 0x084c0c84 si_particleGetMeshes]: -1 when a pick is cancelled (0 returned by the picker) or wrong (a message
/// at level 3: "Must select a mesh for the source." / "... for the destination." / "Animate by Transforms must be
/// "on" ..."), else 1 with the three items stored at +0xb4, +0xb8 and +0xbc. A mesh is a model of kind 3 or 4; the third
/// item may be any model when the flag at +0x34 is set, else a mesh.
pub fn si_particle_get_meshes(m: &mut Mem, o: &mut dyn Outside, status: u32) -> i32 {
    o.si_status_display_mouse(m, 0x086d_7b14, 0x086d_7b07, 0x086d_7b00);
    o.si_status_display_message(m, 0x086d_7b21, 1);
    let src = particle_get_mdl(m, o);
    let err: u32;
    if src == 0 {
        return -1;
    }
    if m.u32(src + 0x30).wrapping_sub(3) < 2 {
        m.set_u32(status + 0xb4, src);
        o.si_status_display_message(m, 0x086d_7b80, 1);
        let dst = particle_get_mdl(m, o);
        if dst == 0 {
            return -1;
        }
        if m.u32(dst + 0x30).wrapping_sub(3) < 2 {
            m.set_u32(status + 0xb8, dst);
            let prompt = if m.i32(status + 0x34) == 0 { 0x086d_7bdd } else { 0x086d_7bc8 };
            o.si_status_display_message(m, prompt, 1);
            let third = particle_get_mdl(m, o);
            if third == 0 {
                return -1;
            }
            if m.i32(status + 0x34) != 0 || m.i32(third + 0x30) == 4 || m.i32(third + 0x30) == 3 {
                m.set_u32(status + 0xbc, third);
                return 1;
            }
            err = 0x086d_7c00;
        } else {
            err = 0x086d_7ba0;
        }
    } else {
        err = 0x086d_7b40;
    }
    o.si_status_display_message(m, err, 3);
    -1
}

/// Sorts the element dictionary of the current process and builds the animation of the particles
/// [soft 0x084c0d94 si_particleCreateAnimation]: `SI_relTopoSortInit(dict, 1)` and `SI_elemdictTopoSort(SI_RD, dict,
/// dict + 0xc, 0, 0)` with `dict = SI_elemDicts + procData[0] * 0x70`, then the builder for a model (flag at +0x34 set)
/// or for a shape.
pub fn si_particle_create_animation(m: &mut Mem, o: &mut dyn Outside, status: u32, arg: u32) {
    let dict = m.u32(SI_ELEM_DICTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA)).wrapping_mul(0x70));
    o.si_rel_topo_sort_init(m, dict, 1);
    let dict = m.u32(SI_ELEM_DICTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA)).wrapping_mul(0x70));
    o.si_elemdict_topo_sort(m, SI_RD, dict, dict.wrapping_add(0xc), 0, 0);
    if m.i32(status + 0x34) == 0 {
        o.particle_create_shape_animation(m, status, arg);
    } else {
        o.particle_create_hrc_animation(m, status, arg);
    }
}

/// Copies the matrix of a model into a 16-float block, transposed [soft 0x084c2e50 particleTransferGmat]: element
/// `[row][col]` of the block (row = the outer counter, col = the inner one) is the model's word at
/// `+0x1c4 + row * 4 + col * 0x10`.
pub fn particle_transfer_gmat(m: &mut Mem, model: u32, out: u32) {
    for row in 0..4u32 {
        for col in 0..4u32 {
            let v = m.u32(model + 0x1c4 + row * 4 + col * 0x10);
            m.set_u32(out + row * 16 + col * 4, v);
        }
    }
}

/// Sets a model's matrix from a 16-float block, then its state from the matrix [soft 0x084c2e98 particleMakeGmat]: the
/// inverse of [`particle_transfer_gmat`] (block word `[row][col]` to the model's `+0x1c4 + row * 4 + col * 0x10`), then
/// `SI_matrixExtractTransformations(model + 0x1c4, scale, rotation, translation)` with the rotation vector zeroed first
/// and `SI_modelSetState(model, scale, rotation, translation)`.
pub fn particle_make_gmat(m: &mut Mem, o: &mut dyn Outside, model: u32, block: u32) {
    for row in 0..4u32 {
        for col in 0..4u32 {
            let v = m.u32(block + row * 16 + col * 4);
            m.set_u32(model + 0x1c4 + row * 4 + col * 0x10, v);
        }
    }
    let loc = m.local(36);
    let (sc, rot, tr) = (loc, loc + 12, loc + 24);
    m.set_u32(rot, 0);
    m.set_u32(rot + 4, 0);
    m.set_u32(rot + 8, 0);
    o.si_matrix_extract_transformations(m, model + 0x1c4, sc, rot, tr);
    o.si_model_set_state(m, model, sc, rot, tr);
    m.release(36);
}
