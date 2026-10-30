// SPDX-License-Identifier: MIT
//! The motion-blur walk of `geometry.c` (finding 0023, section 36) on the memory image:
//! `animate`, `isMoving`, `saveInitialPositions`, `copyModel`, `blurTraverse` and
//! `si_blurGeometry`. They work on the original's render models (`__SI_Model`, 0x430 bytes)
//! and call the geometry stage's other routines (`transForm`, `process`, `freeModel`,
//! `setExistance`, ported in [`crate::geometry`] on a structured state, and `maxPsMovement`
//! in `si-math`) and the scene's (`SI_collect`, `SI_modelMove`, `SI_sceneUpdateItems`) as
//! hooks. The arithmetic kernels are the ones [`si_math::geometry`] names
//! (`moved_enough`, `blur_step`, `BlurTimes`); here they run on the image.
//! Class: model (the register arithmetic).

use si_mem::{clib, Mem};
use si_x87::x;

/// The value an x87-returning callee left in `st0`, as the 80-bit register.
fn st0(r: si_mem::Ret) -> si_x87::F80 {
    r.st0.map_or(si_x87::F80::ZERO, |b| si_x87::F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]])))
}

si_mem::outside_calls! {
    /// What the blur walk calls.
    pub trait BlurCalls {
        /// `SI_sceneUpdateItems (0x082221a0)`: `(time, 1, &database, 1)`.
        fn scene_update_items(time: f32, a: i32, database: u32, b: i32) = "SI_sceneUpdateItems", 0x082221a0;
        /// `SI_modelMove (0x08229a90)`: whether the model's own motion changed.
        fn model_move(model: u32) -> u32 = "SI_modelMove", 0x08229a90;
        /// `SI_modelConstraintMove (0x08229aec)`.
        fn model_constraint_move(model: u32) -> u32 = "SI_modelConstraintMove", 0x08229aec;
        /// `SI_collect (0x082799c0)`: gathers the model's render vertices.
        fn collect(model: u32) = "SI_collect", 0x082799c0;
        /// `transForm (0x08273acc)`, ported in [`crate::geometry`].
        fn trans_form(model: u32) = "transForm", 0x08273acc;
        /// `process (0x08273fa8)`, ported in [`crate::geometry`].
        fn process(model: u32) = "process", 0x08273fa8;
        /// `freeModel (0x082761ac)`, ported in [`crate::geometry`].
        fn free_model(model: u32) = "freeModel", 0x082761ac;
        /// `setExistance (0x082762e0)`, ported in [`crate::geometry`].
        fn set_existance(model: u32, fraction: f32) = "setExistance", 0x082762e0;
        /// `maxPsMovement (0x08276250)`: the largest screen movement; a float in the register.
        fn max_ps_movement(model: u32, before: u32) -> si_mem::Ret = "maxPsMovement", 0x08276250;
        /// `si_traverseExpandedInstance (0x082739e0)`, ported in `si-rensetup`.
        fn traverse_expanded_instance(root: u32) = "si_traverseExpandedInstance", 0x082739e0;
        /// `SI_chunkMalloc (0x0857cb64)`: `(chunk, bytes)`.
        fn chunk_malloc(chunk: u32, bytes: u32) -> u32 = "SI_chunkMalloc", 0x0857cb64;
        /// `SI_cusCheckRenderAbort (0x08535d30)`.
        fn check_render_abort() = "SI_cusCheckRenderAbort", 0x08535d30;
        /// `SI_tabletGetButtonUpEvent (0x083cec2c)`.
        fn tablet_button_up() = "SI_tabletGetButtonUpEvent", 0x083cec2c;
        /// `getbutton (0x0860e684)`: the GL layer.
        fn getbutton(button: i32) -> u32 = "getbutton", 0x0860e684;
        /// `SI_voluntaryExit (0x08260918)`: `(4, flag)`.
        fn voluntary_exit(code: i32, flag: u32) = "SI_voluntaryExit", 0x08260918;
    }
}

const FRAME_CURRENT: u32 = 0x0878_2040;
const FRAME_NOW: u32 = 0x0879_0dda;
const DATABASE: u32 = 0x0879_0640;
const DB_ROOT: u32 = 0x0879_07e8;
const FRAME_RATE: u32 = 0x0878_204c;
const SHUTTER: u32 = 0x0879_0df8;
const THRESHOLD: u32 = 0x0879_0e00;
const REN_CHUNK: u32 = 0x0869_bc84;
const STANDALONE: u32 = 0x0867_9400;
const TABLET_ACTIVE: u32 = 0x0867_93f8;
const TABLET_BUTTON: u32 = 0x0867_94b4;
const BLUR_FLAG_7E: u32 = 0x0879_0c7e;
const BLUR_FLAG_F0: u32 = 0x0879_0df0;
const ABORT_FLAG: u32 = 0x0879_0d58;
const MEMORY_USED: u32 = 0x0879_1220;

/// Fields of the render model this walk reads.
const KIND: u32 = 0x30;
const NVERT: u32 = 0x228;
const VERTS: u32 = 0x22c;
const NTRI: u32 = 0x230;
const TRIS: u32 = 0x234;
const TCOUNT: u32 = 0x334;
const TCOORDS: u32 = 0x330;
const MOVING: u32 = 0x338;
const COPY: u32 = 0x33c;
const MOVEMENT: u32 = 0x340;
const BROTHER: u32 = 0x2c0;
const SON: u32 = 0x2c4;
const UV2: u32 = 0x42c;
const UV2_PER: u32 = 0x424;

/// Evaluates the scene at a time [soft 0x08275bb0 animate]: `animate(time)`; the current frame
/// is set to the render's frame for the update and put back.
pub fn animate(m: &mut Mem, o: &mut dyn si_mem::Outside, time: f32) {
    use BlurCalls as _;
    let time = x(time).to_f32();
    let saved = m.u32(FRAME_CURRENT);
    m.set_u32(FRAME_CURRENT, i32::from(m.i16(FRAME_NOW)) as u32);
    o.scene_update_items(m, time, 1, DATABASE, 1);
    m.set_u32(FRAME_CURRENT, saved);
}

/// Marks the models that move between the start and end of the shutter [soft 0x08275bf0 isMoving]:
/// `isMoving(model, flag)`: `flag` is 1 when an ancestor moved. Each model's +0x338 is cleared;
/// a dummy (kind -1) passes over to its son and brother with no flag; any other asks
/// `SI_modelMove` and `SI_modelConstraintMove` and, if either or the flag says it moved, collects
/// and transforms it and compares the largest movement from its saved copy (+0x33c) with the
/// threshold (0x08790e00): above it, +0x340 is the movement as a float and +0x338 is 1. The son then
/// gets the combined flag (low 16 bits, sign-extended), the brother the one given.
pub fn is_moving(m: &mut Mem, o: &mut dyn si_mem::Outside, model: u32, flag: i16) {
    use BlurCalls as _;
    if model == 0 {
        return;
    }
    m.set_u16(model + MOVING, 0);
    if m.i32(model + KIND) == -1 {
        is_moving(m, o, m.u32(model + SON), 0);
        is_moving(m, o, m.u32(model + BROTHER), 0);
        return;
    }
    let a = o.model_move(m, model);
    let b = o.model_constraint_move(m, model);
    let combined = u32::from(flag as u16) | a | b;
    let si = combined as u16 as i16;
    if si != 0 {
        o.collect(m, model);
        o.trans_form(m, model);
        let copy = m.u32(model + COPY);
        let r = st0(o.max_ps_movement(m, model, copy));
        if r.gt(x(m.f32(THRESHOLD))) {
            m.set_f32(model + MOVEMENT, r.to_f32());
            m.set_u16(model + MOVING, 1);
        }
    }
    is_moving(m, o, m.u32(model + SON), si);
    is_moving(m, o, m.u32(model + BROTHER), flag);
}

/// Copies a render model with its vertices and triangles [soft 0x08275ea4 copyModel]:
/// `copyModel(model, count)` answers the copy. All blocks come from `SI_chunkMalloc(SI_renChunk, ..)`:
/// the 0x430-byte record itself; when it has triangles (+0x234), the vertices (76 bytes each), the
/// triangles (96 bytes each) with their vertex pointers moved to the copy and their +0x5c chain
/// running forward (the last cleared), the second per-vertex array (+0x42c, 8 bytes per entry
/// and `+0x424` entries a vertex) and the texture coordinates (+0x330, 8 bytes each, `+0x334` of
/// them) with the vertices' pointers into them moved. With `count` set, the bytes are added to the
/// counter at 0x08791220. A record without triangles is a plain copy of the 0x430 bytes.
pub fn copy_model(m: &mut Mem, o: &mut dyn si_mem::Outside, model: u32, count: i16) -> u32 {
    use BlurCalls as _;
    let chunk = m.u32(REN_CHUNK);
    let new = o.chunk_malloc(m, chunk, 0x430);
    clib::memcpy(m, new, model, 0x430);
    if m.u32(model + TRIS) == 0 {
        return new;
    }
    let nv = m.u32(model + NVERT);
    let vsize = nv.wrapping_mul(76);
    let chunk = m.u32(REN_CHUNK);
    let vb = o.chunk_malloc(m, chunk, vsize);
    let src = m.u32(model + VERTS);
    clib::memcpy(m, vb, src, vsize);
    let nt = m.u32(model + NTRI);
    let tsize = nt.wrapping_mul(96);
    let chunk = m.u32(REN_CHUNK);
    let tb = o.chunk_malloc(m, chunk, tsize);
    let src = m.u32(model + TRIS);
    clib::memcpy(m, tb, src, tsize);
    let uv2 = if m.u32(model + UV2) != 0 {
        let n = nv.wrapping_mul(m.u32(model + UV2_PER)).wrapping_shl(3);
        let chunk = m.u32(REN_CHUNK);
        let b = o.chunk_malloc(m, chunk, n);
        let src = m.u32(model + UV2);
        clib::memcpy(m, b, src, n);
        b
    } else {
        0
    };
    let old_v = m.u32(model + VERTS);
    let ntri = m.i32(model + NTRI);
    if ntri > 0 {
        let mut prev = 0u32;
        let mut t = tb;
        for _ in 0..ntri {
            for f in [0u32, 4, 8] {
                let v = m.u32(t + f).wrapping_sub(old_v).wrapping_add(vb);
                m.set_u32(t + f, v);
            }
            m.set_u32(t + 0x5c, prev);
            prev = t;
            t = t.wrapping_add(0x60);
        }
        let mut next = tb.wrapping_add(0x60);
        for i in 0..ntri as u32 {
            m.set_u32(tb.wrapping_add(i.wrapping_mul(0x60)).wrapping_add(0x5c), next);
            next = next.wrapping_add(0x60);
        }
    }
    m.set_u32(tb.wrapping_add(((ntri as u32).wrapping_mul(3)).wrapping_shl(5)).wrapping_sub(4), 0);
    let w = m.u32(model + NVERT);
    m.set_u32(new + NVERT, w);
    m.set_u32(new + VERTS, vb);
    let w = m.u32(model + NTRI);
    m.set_u32(new + NTRI, w);
    m.set_u32(new + TRIS, tb);
    let tcount = m.u32(model + TCOUNT);
    if tcount != 0 {
        let size = tcount << 3;
        let chunk = m.u32(REN_CHUNK);
        let tc = o.chunk_malloc(m, chunk, size);
        let src = m.u32(model + TCOORDS);
        clib::memcpy(m, tc, src, size);
        let old_tc = m.u32(model + TCOORDS);
        let n = m.i32(model + NVERT);
        let (mut nvp, mut ovp) = (vb, old_v);
        for _ in 0..n.max(0) {
            let v = m.u32(ovp + 0x40).wrapping_sub(old_tc).wrapping_add(tc);
            m.set_u32(nvp + 0x40, v);
            nvp = nvp.wrapping_add(0x4c);
            ovp = ovp.wrapping_add(0x4c);
        }
        m.set_u32(new + TCOORDS, tc);
    }
    if uv2 != 0 {
        let old_uv2 = m.u32(model + UV2);
        let mut i = 0u32;
        while i < m.u32(model + NVERT) {
            let off = i.wrapping_mul(0x4c);
            let v = m.u32(old_v.wrapping_add(off).wrapping_add(0x48)).wrapping_sub(old_uv2).wrapping_add(uv2);
            m.set_u32(vb.wrapping_add(off).wrapping_add(0x48), v);
            i += 1;
        }
        m.set_u32(new + UV2, uv2);
    }
    if count != 0 {
        let add = m.u32(model + NTRI).wrapping_mul(96).wrapping_add(0x430).wrapping_add(m.u32(model + NVERT).wrapping_mul(76));
        let used = m.u32(MEMORY_USED).wrapping_add(add);
        m.set_u32(MEMORY_USED, used);
    }
    new
}

/// Saves every model's positions at the start of the shutter [soft 0x08275b28 saveInitialPositions]:
/// `saveInitialPositions(model)`; a dummy passes on to its son and brother; any other is collected,
/// transformed and copied (`copyModel(model, 0)`, kept at +0x33c), then its son and brother follow.
pub fn save_initial_positions(m: &mut Mem, o: &mut dyn si_mem::Outside, model: u32) {
    use BlurCalls as _;
    if model == 0 {
        return;
    }
    if m.i32(model + KIND) == -1 {
        save_initial_positions(m, o, m.u32(model + SON));
        save_initial_positions(m, o, m.u32(model + BROTHER));
        return;
    }
    o.collect(m, model);
    o.trans_form(m, model);
    let c = copy_model(m, o, model, 0);
    m.set_u32(model + COPY, c);
    save_initial_positions(m, o, m.u32(model + SON));
    save_initial_positions(m, o, m.u32(model + BROTHER));
}

/// Draws the blur copies of a model and of its brothers [soft 0x08275ce0 blurTraverse]:
/// `blurTraverse(model, start, end)`; a loop over the brothers, each also followed down its sons.
///
/// First the abort check: `SI_cusCheckRenderAbort`, and (unless 0x08679400) the tablet and the
/// buttons 0x66 and 7, which end the render through `SI_voluntaryExit(4, ..)`. A model whose kind
/// is 1, 2, 4, 7 or 10 and that does not move gets `process(copy)` and `freeModel`; one that
/// moves is drawn with a degree of existence at the step ([`si_math::geometry::blur_step`]), then
/// at each time of [`si_math::geometry::BlurTimes`] a fresh `animate` + `SI_collect` + `copyModel`
/// + `transForm` + `process` + `freeModel`.
pub fn blur_traverse(m: &mut Mem, o: &mut dyn si_mem::Outside, model: u32, start: f32, end: f32) {
    use BlurCalls as _;
    let mut model = model;
    while model != 0 {
        o.check_render_abort(m);
        if m.u16(STANDALONE) == 0 {
            if m.u32(TABLET_ACTIVE) != 0 {
                o.tablet_button_up(m);
            }
            let stop = o.getbutton(m, 0x66) != 0 || m.u32(TABLET_BUTTON) == 0x66 || o.getbutton(m, 7) != 0;
            if stop {
                let flag = u32::from(m.u16(ABORT_FLAG) == 0);
                o.voluntary_exit(m, 4, flag);
            }
        }
        let k = m.u32(model + KIND).wrapping_add(1);
        if k <= 11 && matches!(k, 2 | 3 | 5 | 8 | 11) {
            if m.u16(model + MOVING) == 0 {
                let c = m.u32(model + COPY);
                o.process(m, c);
                o.free_model(m, model);
            } else {
                let span = x(end) - start;
                let movement = m.f32(model + MOVEMENT);
                let thr = m.f32(THRESHOLD);
                let step_reg = span / movement * thr;
                let step = step_reg.to_f32();
                let fraction = (step_reg / span).to_f32();
                o.set_existance(m, model, fraction);
                let c = m.u32(model + COPY);
                o.process(m, c);
                let b = copy_model(m, o, model, 1);
                o.trans_form(m, b);
                o.process(m, b);
                o.free_model(m, model);
                let mut t = (x(start) + step).to_f32();
                while (x(end) - step).gt(x(t)) {
                    animate(m, o, t);
                    o.collect(m, model);
                    let b = copy_model(m, o, model, 1);
                    o.trans_form(m, b);
                    o.process(m, b);
                    o.free_model(m, model);
                    t = (x(t) + step).to_f32();
                }
            }
        }
        let son = m.u32(model + SON);
        blur_traverse(m, o, son, start, end);
        model = m.u32(model + BROTHER);
    }
}

/// The blur of a frame [soft 0x08275a9c si_blurGeometry]: `si_blurGeometry(time)`: the shutter's
/// end is time + shutter ÷ frame rate (0x08790df8, 0x0878204c); the scene is saved at the start,
/// animated to the end, its expanded instances traversed (when 0x08790c7e is 2 and 0x08790df0 set),
/// the moving models marked and the copies drawn.
pub fn blur_geometry(m: &mut Mem, o: &mut dyn si_mem::Outside, time: f32) {
    use BlurCalls as _;
    let t0 = x(time).to_f32();
    let end = (x(m.f32(SHUTTER)) / m.f32(FRAME_RATE) + t0).to_f32();
    let root = m.u32(DB_ROOT);
    save_initial_positions(m, o, root);
    animate(m, o, end);
    if m.u16(BLUR_FLAG_7E) == 2 && m.u16(BLUR_FLAG_F0) != 0 {
        let root = m.u32(DB_ROOT);
        o.traverse_expanded_instance(m, root);
    }
    let root = m.u32(DB_ROOT);
    is_moving(m, o, root, 0);
    let root = m.u32(DB_ROOT);
    blur_traverse(m, o, root, t0, end);
}
