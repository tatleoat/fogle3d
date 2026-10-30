// SPDX-License-Identifier: MIT
//! A wave tied to the model it deforms (`wave_util.c`): `SI_waveAddMdl`, which
//! `SI_scnRelationsFromElemDict` calls for relations 600 and 601 (finding 0018, sections 17
//! and 20) and the wave menu's attaches call with their own fourth argument.
//!
//! Read from the instruction listing (the order of the x87 operations of the translation,
//! the reads of the icon pointer, the copies of the definition). **Compared with the
//! emulated original** (`re/tools/si-scene/emu_gen_wavemdl.py`,
//! `scene-emucheck re/out/si-scene/vec_wavemdl`): made-up waves, models and icons, the three
//! matrix routines run as they are, every other callee a recorded stand-in.

use crate::edtools::ByAddr;
use si_math::types::{Matrix, Vector};
use si_mem::Mem;
use si_x87::{x, F80};

/// `SI_edModelLinkWave (0x08101420)`, `edtools.c`: (model, wave); relation 600 or 601 in the
/// current dictionary when it is not there yet.
pub const SI_ED_MODEL_LINK_WAVE: u32 = 0x0810_1420;
/// `SI_modelUpdateMatrix (0x0846559c)`, `model_b.c`: (node).
pub const SI_MODEL_UPDATE_MATRIX: u32 = 0x0846_559c;
/// `SI_patchCopy (0x0847a2b8)`, `SI_faceCopy (0x08434990)`, `SI_meshCopy (0x084404e0)`,
/// `SI_nurbsSrfCopy (0x084dc1a8)`: (definition); answer the copy.
pub const SI_PATCH_COPY: u32 = 0x0847_a2b8;
pub const SI_FACE_COPY: u32 = 0x0843_4990;
pub const SI_MESH_COPY: u32 = 0x0844_04e0;
pub const SI_NURBS_SRF_COPY: u32 = 0x084d_c1a8;
/// `SI_modelRoot (0x0847838c)`: (node); `SI_modelUpdateDependency (0x0846f200)`: (node, 0).
pub const SI_MODEL_ROOT: u32 = 0x0847_838c;
pub const SI_MODEL_UPDATE_DEPENDENCY: u32 = 0x0846_f200;
/// The short at 0x08781580 (`SI_glbManip` + 0x20) that `SI_matrixExtractXyzRotation` reads.
const KEEP_TURNS: u32 = 0x0878_1580;

fn matrix_at(m: &Mem, a: u32) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a.wrapping_add(16 * i as u32 + 4 * j as u32));
        }
    }
    out
}

fn put_matrix(m: &mut Mem, a: u32, v: &Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, f) in row.iter().enumerate() {
            m.set_f32(a.wrapping_add(16 * i as u32 + 4 * j as u32), *f);
        }
    }
}

/// Ties `wave` to `model` [soft 0x084c9810 SI_waveAddMdl]: `SI_waveAddMdl(wave, model,
/// branch, keep)`.
///
/// Nothing when the wave already has a model (+0x6c). Else +0x70 = 1 for a branch wave (16
/// bits non-zero), 0 for the node's own; +0x6c = the model; `SI_edModelLinkWave(model,
/// wave)`; a node of 12 bytes from `malloc` (the wave, the same flag, null) appended to the
/// model's wave list (+0x2e8; when the list is empty it becomes its head, the node filled
/// first).
///
/// With `keep` 0 (16 bits) the icon (+0x74) is brought into the model's space, each step
/// reading the icon pointer anew as the original does: its scaling (+0x100) divided by the
/// model's global scaling (+0x16c) component by component (rounded to float on each store);
/// its rotation matrix (+0x118) = `SI_matrixMultiplySub3(rotation, inverse of the model's
/// global rotation (+0x178))`, its angles (+0x10c) `SI_matrixExtractXyzRotation` of that
/// (from the angles there before); its translation (+0x158) times the inverse of the model's
/// global matrix (+0x1c4), each coordinate ((x·m0j + y·m1j) + z·m2j) + m3j in the unit and
/// rounded on the store, all three read before the first store; `SI_modelUpdateMatrix(icon)`.
/// The inverses go to one local of 16 floats: when `SI_matrixInverse` fails (no usable
/// pivot) the local keeps what it held, the first time memory nobody wrote (here the
/// image's fill for a local).
///
/// Then the working definition (+0x48), when the node has stored definitions (+0x34, 16
/// bits) and it is the current stored one (the array +0x38, index +0x36), is replaced by a
/// copy: of a mesh (kind 4, +0x30) `SI_meshCopy` of it; of a patch (1), a face (2) or a
/// NURBS surface (10) `SI_patchCopy`, `SI_faceCopy`, `SI_nurbsSrfCopy` **of the first stored
/// definition** (index 0, whatever the current index is). Other kinds keep theirs.
///
/// Last, with `keep` 0 and a model that is not null (tested only here),
/// `SI_modelUpdateDependency(SI_modelRoot(model), 0)`.
pub fn wave_add_mdl<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, wave: u32, model: u32, branch: i16, keep: i16) {
    if m.u32(wave.wrapping_add(0x6c)) != 0 {
        return;
    }
    let flag = (branch != 0) as u32;
    m.set_u32(wave.wrapping_add(0x70), flag);
    m.set_u32(wave.wrapping_add(0x6c), model);
    o.call_addr(m, SI_ED_MODEL_LINK_WAVE, &[model, wave]);
    let head = m.u32(model.wrapping_add(0x2e8));
    if head == 0 {
        let node = m.malloc(0xc);
        m.set_u32(node, wave);
        m.set_u32(node.wrapping_add(4), flag);
        m.set_u32(node.wrapping_add(8), 0);
        m.set_u32(model.wrapping_add(0x2e8), node);
    } else {
        let mut last = head;
        while m.u32(last.wrapping_add(8)) != 0 {
            last = m.u32(last.wrapping_add(8));
        }
        let node = m.malloc(0xc);
        m.set_u32(last.wrapping_add(8), node);
        m.set_u32(node, wave);
        m.set_u32(node.wrapping_add(4), flag);
        m.set_u32(node.wrapping_add(8), 0);
    }
    let icon = |m: &Mem| m.u32(wave.wrapping_add(0x74));
    if keep == 0 {
        for k in 0..3u32 {
            let at = icon(m).wrapping_add(0x100 + 4 * k);
            let v = (x(m.f32(at)) / m.f32(model.wrapping_add(0x16c + 4 * k))).to_f32();
            m.set_f32(at, v);
        }
        let local = m.local(0x40);
        if let Some(inv) = si_math::algebra::matrix_inverse(&matrix_at(m, model.wrapping_add(0x178))) {
            put_matrix(m, local, &inv);
        }
        let rot = icon(m).wrapping_add(0x118);
        let product = si_math::algebra::matrix_multiply_sub3(&matrix_at(m, rot), &matrix_at(m, local));
        put_matrix(m, rot, &product);
        let i = icon(m);
        let (rot, angles) = (i.wrapping_add(0x118), i.wrapping_add(0x10c));
        let before = Vector::new(m.f32(angles), m.f32(angles.wrapping_add(4)), m.f32(angles.wrapping_add(8)));
        let after = si_math::algebra::matrix_extract_xyz_rotation(&matrix_at(m, rot), &before, m.i16(KEEP_TURNS) != 0);
        m.set_f32(angles, after.x);
        m.set_f32(angles.wrapping_add(4), after.y);
        m.set_f32(angles.wrapping_add(8), after.z);
        if let Some(inv) = si_math::algebra::matrix_inverse(&matrix_at(m, model.wrapping_add(0x1c4))) {
            put_matrix(m, local, &inv);
        }
        let g = matrix_at(m, local);
        let t = icon(m).wrapping_add(0x158);
        let (px, py, pz) = (m.f32(t), m.f32(t.wrapping_add(4)), m.f32(t.wrapping_add(8)));
        let coord = |j: usize| (((x(px) * g[0][j] + x(py) * g[1][j]) + x(pz) * g[2][j]) + g[3][j]).to_f32();
        m.set_f32(t, coord(0));
        let at = icon(m).wrapping_add(0x15c);
        m.set_f32(at, coord(1));
        let at = icon(m).wrapping_add(0x160);
        m.set_f32(at, coord(2));
        m.release(0x40);
        let i = icon(m);
        o.call_addr(m, SI_MODEL_UPDATE_MATRIX, &[i]);
    }
    // (the kind first: for the other kinds the definitions are not read at all)
    let kind = m.i32(model.wrapping_add(0x30));
    let copy = match kind {
        4 => Some(SI_MESH_COPY),
        1 => Some(SI_PATCH_COPY),
        2 => Some(SI_FACE_COPY),
        10 => Some(SI_NURBS_SRF_COPY),
        _ => None,
    };
    if let Some(f) = copy {
        if m.u16(model.wrapping_add(0x34)) != 0 {
            let defs = m.u32(model.wrapping_add(0x38));
            let current = m.u32(defs.wrapping_add((m.u16(model.wrapping_add(0x36)) as u32).wrapping_mul(4)));
            let working = m.u32(model.wrapping_add(0x48));
            if working == current {
                let from = if kind == 4 { working } else { m.u32(defs) };
                let c = o.call_addr(m, f, &[from]);
                m.set_u32(model.wrapping_add(0x48), c);
            }
        }
    }
    if keep == 0 && model != 0 {
        let root = o.call_addr(m, SI_MODEL_ROOT, &[model]);
        o.call_addr(m, SI_MODEL_UPDATE_DEPENDENCY, &[root, 0]);
    }
}

// ---- the helpers of `wave_util.c` the wave updates call (read from the instruction
// listing; held against the emulated original through `si_waveSplineUpdate` and on their
// own, `re/tools/si-scene/emu_gen_wave.py`)

/// The wave's frame [soft 0x084c9f04 si_waveComputeVectors]: `(wave, position, u, v,
/// scaling)`. `SI_modelState(icon, scaling, icon + 0x10c, position)` on the wave's icon
/// (+0x74, read once); then, when the wave has an element (`SI_edGetElemFromWave`, its index
/// to a local), `u` = the icon's rotation row 1 (+0x128), `v` = row 0 (+0x118) and `scaling`
/// = the icon's scaling (+0x100), each float moved through the unit, in that order. Without
/// an element only `SI_modelState`'s copies are made.
pub fn wave_compute_vectors<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, wave: u32, position: u32, u: u32, v: u32, scaling: u32) {
    let icon = m.u32(wave.wrapping_add(0x74));
    si_model::model_image::state::model_state(m, icon, scaling, icon.wrapping_add(0x10c), position);
    let index = m.local(4);
    let elem = crate::edtools::ed_get_elem_from_wave(o, m, wave, si_mem::Ptr32::new(index));
    m.release(4);
    if elem.is_null() {
        return;
    }
    for (to, from) in [(u, 0x128u32), (v, 0x118), (scaling, 0x100)] {
        for k in 0..3u32 {
            let f = si_x87::fmov(m.f32(icon.wrapping_add(from + 4 * k)));
            m.set_f32(to.wrapping_add(4 * k), f);
        }
    }
}

/// The value of a wave's graph at `x` [soft 0x084c9f9c si_waveFindGraphValue]: `(graph,
/// x)`, answered in the register. The graph: +0 the count of points (x, y float pairs at
/// the pointer +0x1c), +4 the mode.
///
/// 0 with fewer than two points, or when the last x is not above the first (ordered: an
/// unordered pair goes on). Mode 0: 0 when `x` is below the first x or at or above the last.
/// Mode 1: the span (last − first, in the register) is added while `x` is below the first,
/// then taken away while `x` is at or above the last. Any other mode takes `x` as it is.
/// Then the first point whose x is above `x` (or unordered with it; index 2 at the least,
/// in floats: the second point) is found by walking up from the first, **past the last
/// point if `x` is not below it** (only another mode can get there: the walk reads what
/// lies behind the array until a float above `x`), and the value is
/// `y0 + ((x − x0)·(y1 − y0)) / (x1 − x0)` from that point and the one before, all in the
/// register.
pub fn wave_find_graph_value(m: &Mem, graph: u32, x0: f32) -> F80 {
    let mut v = x(x0);
    let n = m.i32(graph);
    if n <= 1 {
        return F80::ZERO;
    }
    let p = m.u32(graph.wrapping_add(0x1c));
    let pt = |k: i32| m.f32(p.wrapping_add((k as u32).wrapping_mul(4)));
    let (a, b) = (x(pt(0)), x(pt(2 * n - 2)));
    // `fcom`, `and 0x45`, `dec`, `cmp 0x40`, `jb`: below or equal ends it, unordered goes on
    if b.lt(a) || b.eq(a) {
        return F80::ZERO;
    }
    match m.i32(graph.wrapping_add(4)) {
        0 => {
            if v.lt(a) || !(v.lt(b) || v.unordered(b)) {
                return F80::ZERO;
            }
        }
        1 => {
            if v.lt(a) {
                let span = b - a;
                loop {
                    v = v + span;
                    if !v.lt(a) {
                        break;
                    }
                }
            }
            if !(v.lt(b) || v.unordered(b)) {
                let span = b - a;
                loop {
                    v = v - span;
                    if v.lt(b) || v.unordered(b) {
                        break;
                    }
                }
            }
        }
        _ => {}
    }
    let mut i = 0i32;
    // `fcoms`, `and 5`: the walk goes on while `x` is at or above the point's x (ordered)
    while !(v.lt(x(pt(i))) || v.unordered(x(pt(i)))) {
        i += 2;
    }
    if i == 0 {
        i = 2;
    }
    let (xa, ya, yb) = (x(pt(i - 2)), x(pt(i - 1)), x(pt(i + 1)));
    let num = (yb - ya) * (v - xa);
    let dx = x(pt(i)) - xa;
    ya + num / dx
}

/// [soft 0x084ca088 si_waveDotProduct]: `((a.x·b.x + a.y·b.y) + a.z·b.z)` of the vectors at
/// `a` and `b`, in the register.
pub fn wave_dot_product(m: &Mem, a: u32, b: u32) -> F80 {
    let f = |p: u32, k: u32| m.f32(p.wrapping_add(4 * k));
    (x(f(a, 0)) * f(b, 0) + x(f(a, 1)) * f(b, 1)) + x(f(a, 2)) * f(b, 2)
}

/// [soft 0x084ca0b0 si_waveNorm]: the length of the vector at `a`,
/// `sqrt((x·x + y·y) + z·z)` in the register, rounded to a float (stored and loaded again).
pub fn wave_norm(m: &Mem, a: u32) -> F80 {
    let f = |k: u32| x(m.f32(a.wrapping_add(4 * k)));
    let (vx, vy, vz) = (f(0), f(1), f(2));
    x(((vx * vx + vy * vy) + vz * vz).sqrt().to_f32())
}

/// [soft 0x084ca140 si_splineIndexToValue]: `(kind, count, index, out)`, all three 16 bits:
/// the parameter of a spline's point `index`. Kind 0: the index; kind 1: index × 0.33333 (a
/// double constant, rounded once to the float); kinds 2 and 3: 0 for index 0, `count − 3`
/// for the last point (index = count − 1), else index − 1; other kinds store nothing.
pub fn spline_index_to_value(m: &mut Mem, kind: i16, count: i16, index: i16, out: u32) {
    match kind {
        0 => m.set_f32(out, F80::from_i32(index as i32).to_f32()),
        1 => m.set_f32(out, (F80::from_i32(index as i32) * 0.33333f64).to_f32()),
        2 | 3 => {
            if index == 0 {
                m.set_u32(out, 0);
            } else {
                let v = if index as i32 == count as i32 - 1 { count as i32 - 3 } else { index as i32 - 1 };
                m.set_f32(out, F80::from_i32(v).to_f32());
            }
        }
        _ => {}
    }
}
