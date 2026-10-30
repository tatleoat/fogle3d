// SPDX-License-Identifier: MIT
//! The waves of a model applied to its working definition (`wave_sys.c`):
//! `SI_waveTransform`, which `SI_modelReadjustDefinition` (si-model's `readjust`) calls with
//! the node's or the branch's waves at every update. The other routines of the file that are
//! ported are si-model's (`SI_modelWave`, `SI_modelNodeWave`, `SI_modelBranchWave`) and
//! `wave.rs`' `SI_waveGetIconModel`.
//!
//! Read from the instruction listing (the jump table at 0x08661404 read from the binary).
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_wave.py`): made-up
//! wave lists and models; `si_waveSplineUpdate` and what it calls run as they are (but the
//! element and curve look-ups and the curves' values), every other callee a recorded
//! stand-in.

use crate::wave_update::{wave_spline_update, WaveOutside};
use si_mem::Mem;

/// `si_waveGeomUpdate (0x084c81d8)`, `wave_update.c`: (wave, model, time, check); a mesh's,
/// patch's or NURBS surface's points moved by the wave. Asked for by address (the callers
/// route it to [`crate::wave_geom::wave_geom_update`]; a stand-in in the vectors of this
/// routine).
pub const SI_WAVE_GEOM_UPDATE: u32 = 0x084c_81d8;
/// `SI_patchComputeSurface (0x0847c1d0)`: (patch, 0).
pub const SI_PATCH_COMPUTE_SURFACE: u32 = 0x0847_c1d0;
/// `SI_meshComputesPolygonNormal (0x084599f4)`, `SI_meshComputesVertexNormal (0x08459e8c)`:
/// (mesh, −1).
pub const SI_MESH_COMPUTES_POLYGON_NORMAL: u32 = 0x0845_99f4;
pub const SI_MESH_COMPUTES_VERTEX_NORMAL: u32 = 0x0845_9e8c;
/// `SI_splineTouch (0x0848c924)`: (spline).
pub const SI_SPLINE_TOUCH: u32 = 0x0848_c924;

/// The waves of the list `list` applied to `model` at `time`
/// [soft 0x084c7640 SI_waveTransform]: `SI_waveTransform(list, model, branch, time, check)`.
///
/// The list's nodes are 12 bytes: the wave, its branch flag, the next node. With `branch`
/// (16 bits) set, the nodes whose flag is not 0 are applied, else those whose flag is not 1.
/// By the model's kind (+0x30, read for each node), on its working definition (+0x48, read
/// anew for each call): a patch (1) `si_waveGeomUpdate(wave, model, time, check)` then
/// `SI_patchComputeSurface(definition, 0)`; a mesh (4) `si_waveGeomUpdate` then
/// `SI_meshComputesPolygonNormal(definition, −1)` and `SI_meshComputesVertexNormal(definition,
/// −1)`; a spline (6) [`wave_spline_update`]`(wave, definition, definition, time, check)` then
/// `SI_splineTouch(definition)`; a face or NURBS surface (9, 10) `si_waveGeomUpdate` alone;
/// other kinds nothing. `check` goes on as a word, sign-extended.
pub fn wave_transform<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, list: u32, model: u32, branch: i16, time: f32, check: i16) {
    let ck = check as i32 as u32;
    let mut node = list;
    while node != 0 {
        let wave = m.u32(node);
        let flag = m.u32(node.wrapping_add(4));
        let apply = if branch != 0 { flag != 0 } else { flag != 1 };
        if apply {
            let def = |m: &Mem| m.u32(model.wrapping_add(0x48));
            match m.i32(model.wrapping_add(0x30)) {
                1 => {
                    o.call_addr(m, SI_WAVE_GEOM_UPDATE, &[wave, model, time.to_bits(), ck]);
                    let d = def(m);
                    o.call_addr(m, SI_PATCH_COMPUTE_SURFACE, &[d, 0]);
                }
                4 => {
                    o.call_addr(m, SI_WAVE_GEOM_UPDATE, &[wave, model, time.to_bits(), ck]);
                    let d = def(m);
                    o.call_addr(m, SI_MESH_COMPUTES_POLYGON_NORMAL, &[d, u32::MAX]);
                    let d = def(m);
                    o.call_addr(m, SI_MESH_COMPUTES_VERTEX_NORMAL, &[d, u32::MAX]);
                }
                6 => {
                    let d = def(m);
                    wave_spline_update(o, m, wave, d, d, time, check);
                    let d = def(m);
                    o.call_addr(m, SI_SPLINE_TOUCH, &[d]);
                }
                9 | 10 => {
                    o.call_addr(m, SI_WAVE_GEOM_UPDATE, &[wave, model, time.to_bits(), ck]);
                }
                _ => {}
            }
        }
        node = m.u32(node.wrapping_add(8));
    }
}
