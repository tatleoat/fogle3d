// SPDX-License-Identifier: MIT
//! The motion-blur walk of `geometry.c` (finding 0023, section 36) on the renderer's structured
//! state: `si_blurGeometry`, `saveInitialPositions`, `isMoving`, `blurTraverse` and `copyModel`.
//! [`crate::blur_image`] has the same walk on the memory image of the original's render models
//! (`__SI_Model`); the embedding of the renderer in [`crate::adapter`] works on [`RenModel`]s and
//! the pools of [`ShadeState`] (the scene is read from the image once per `SI_update`), so the walk
//! is repeated here on those, calling the geometry stage's routines in [`crate::geometry`].
//!
//! What the original does to the image and the structured state cannot do alone is the scene
//! itself: `animate(time)` brings the scene to another time and the models read afterwards
//! are those of that time. [`BlurWorld::animate`] is the embedding's answer: it makes that
//! call on the image and reads the nodes again.
//!
//! Per-model members of the record that only this walk uses (+0x338 moving, +0x33c copy,
//! +0x340 movement) are kept in [`BlurState`], by node index.
//! Class: model (the register arithmetic is [`si_math::geometry`]'s).

use crate::geometry::{free_model, process, renderable, set_existance, trans_form, GeometryEnv};
use crate::shade::ShadeState;
use crate::geometry::Geometry;
use si_math::geometry::{blur_end_time, blur_step, max_ps_movement};
use si_x87::x;

/// What the walk needs of the embedding beyond [`GeometryEnv::collect`].
pub trait BlurWorld: GeometryEnv {
    /// The `SI_frameRate` global (0x0878204c).
    fn frame_rate(&mut self) -> f32;
    /// `animate(time)` [soft 0x08275bb0 animate] and the nodes read again: `SI_frameCurrent`
    /// (0x08782040) is set to the render's frame (0x08790dda) for `SI_sceneUpdateItems(time, 1,
    /// &SI_database, 1)` and put back. With `expanded`, `si_traverseExpandedInstance(root)`
    /// follows (the part of `si_blurGeometry` that does it after the first `animate`).
    /// Afterwards the node sources and the matrices of the models in `g` are those of `time`.
    fn animate(&mut self, g: &mut Geometry, time: f32, expanded: bool);
    /// `SI_modelMove (0x08229a90)` of the node.
    fn model_move(&mut self, node: usize) -> u32;
    /// `SI_modelConstraintMove (0x08229aec)` of the node.
    fn model_constraint_move(&mut self, node: usize) -> u32;
    /// How many texture coordinate pairs the node's last `SI_collect` made (+0x334).
    fn pair_count(&self, node: usize) -> usize;
}

/// Members of the render model only the blur walk reads, per node.
struct BlurState {
    /// +0x33c: the copy made at the start of the shutter.
    copy: Vec<Option<usize>>,
    /// +0x338
    moving: Vec<bool>,
    /// +0x340
    movement: Vec<f32>,
    /// the pairs of the node's arrays as `isMoving` or the latest `SI_collect` left them (+0x334)
    pairs: Vec<usize>,
    threshold: f32,
}

/// The blur of a frame [soft 0x08275a9c si_blurGeometry]: `si_blurGeometry(time)`: the shutter's
/// end is time + shutter ÷ frame rate (`shutter_copy`, 0x08790df8; 0x0878204c); the scene is saved
/// at the start, animated to the end, its expanded instances traversed (the caller's condition,
/// sequence mode 2 with motion blur, holds: it is the only caller), the moving models marked and the
/// copies drawn. The copies the walk adds to `g.models` are taken off again at the end (the
/// pools keep their arrays, as `freeModel` leaves them in the structured state).
pub fn blur_geometry<W: BlurWorld + ?Sized>(st: &mut ShadeState, g: &mut Geometry, w: &mut W, root: Option<usize>, shutter: f32, threshold: f32, time: f32) {
    let t0 = x(time).to_f32();
    let rate = w.frame_rate();
    let end = blur_end_time(t0, shutter, rate);
    let n = g.models.len();
    let mut b = BlurState { copy: vec![None; n], moving: vec![false; n], movement: vec![0.0; n], pairs: vec![0; n], threshold };
    save_initial_positions(st, g, w, &mut b, root);
    w.animate(g, end, true);
    is_moving(st, g, w, &mut b, root, 0);
    blur_traverse(st, g, w, &mut b, root, t0, end);
    g.models.truncate(n);
}

/// Saves every model's positions at the start of the shutter [soft 0x08275b28 saveInitialPositions]:
/// `saveInitialPositions(model)`; a dummy (kind -1) passes on to its son and brother; any other
/// is collected, transformed and copied (`copyModel(model, 0)`, kept as its copy), then its son
/// and brother follow.
fn save_initial_positions<W: BlurWorld + ?Sized>(st: &mut ShadeState, g: &mut Geometry, w: &mut W, b: &mut BlurState, model: Option<usize>) {
    let Some(n) = model else { return };
    let (son, brother) = (g.models[n].son, g.models[n].brother);
    if g.models[n].kind == -1 {
        save_initial_positions(st, g, w, b, son);
        save_initial_positions(st, g, w, b, brother);
        return;
    }
    w.collect(st, g, n);
    b.pairs[n] = w.pair_count(n);
    trans_form(st, g, n);
    b.copy[n] = Some(copy_model(st, g, n, b.pairs[n]));
    save_initial_positions(st, g, w, b, son);
    save_initial_positions(st, g, w, b, brother);
}

/// Marks the models that move between the start and end of the shutter [soft 0x08275bf0 isMoving]:
/// `isMoving(model, flag)`: `flag` is non-zero when an ancestor moved. Each model's moving mark is
/// cleared; a dummy (kind -1) passes over to its son and brother with no flag; any other asks
/// `SI_modelMove` and `SI_modelConstraintMove` and, if either or the flag says it moved, collects
/// and transforms it and compares the largest movement from its saved copy with the threshold
/// (0x08790e00): above it, the movement is stored as a float and the mark is set. The son then
/// gets the combined flag (low 16 bits, sign-extended), the brother the one given.
fn is_moving<W: BlurWorld + ?Sized>(st: &mut ShadeState, g: &mut Geometry, w: &mut W, b: &mut BlurState, model: Option<usize>, flag: i16) {
    let Some(n) = model else { return };
    let (son, brother) = (g.models[n].son, g.models[n].brother);
    b.moving[n] = false;
    if g.models[n].kind == -1 {
        is_moving(st, g, w, b, son, 0);
        is_moving(st, g, w, b, brother, 0);
        return;
    }
    let a = w.model_move(n);
    let c = w.model_constraint_move(n);
    let combined = u32::from(flag as u16) | a | c;
    let si = combined as u16 as i16;
    if si != 0 {
        w.collect(st, g, n);
        b.pairs[n] = w.pair_count(n);
        trans_form(st, g, n);
        let range = |i: Option<usize>, g: &Geometry| i.and_then(|k| g.models[k].verts.map(|v| (v, g.models[k].vert_count.max(0) as usize)));
        let (now, before) = (range(Some(n), g), range(b.copy[n], g));
        let slice = |r: Option<(usize, usize)>| r.map(|(v, c)| &st.verts[v..v + c]);
        let r = max_ps_movement(slice(now), slice(before));
        if r.gt(x(b.threshold)) {
            b.movement[n] = r.to_f32();
            b.moving[n] = true;
        }
    }
    is_moving(st, g, w, b, son, si);
    is_moving(st, g, w, b, brother, flag);
}

/// Copies a render model with its vertices and triangles (copyModel, 0x08275ea4): the record,
/// and when it has triangles the vertices, the triangles (corners moved to the copy's vertices,
/// the chain running forward and cleared at the last) and the texture coordinates (the
/// vertices' pairs moved to the copy's), appended to the pools; the second per-vertex array
/// (+0x42c) is the node's and stays. The original also adds the bytes to a counter of memory
/// used (0x08791220) when asked: not kept here. Answers the copy's index in `g.models`.
fn copy_model(st: &mut ShadeState, g: &mut Geometry, node: usize, pairs: usize) -> usize {
    let mut c = g.models[node].clone();
    if let Some(t0) = g.models[node].tns {
        let (v0, nv) = (g.models[node].verts.unwrap_or(0), g.models[node].vert_count.max(0) as usize);
        let nt = g.models[node].tn_count.max(0) as usize;
        let vb = st.verts.len();
        let old_verts: Vec<_> = st.verts[v0..v0 + nv].to_vec();
        st.verts.extend(old_verts);
        let tb = st.tns.len();
        for i in 0..nt {
            let mut t = st.tns[t0 + i].clone();
            t.v = t.v.map(|k| k.map(|k| k.wrapping_sub(v0).wrapping_add(vb)));
            t.next = (i + 1 < nt).then_some(tb + i + 1);
            st.tns.push(t);
        }
        c.verts = (nv > 0).then_some(vb);
        c.tns = Some(tb);
        if pairs != 0 {
            let old_u0 = g.models[node].tcoords.unwrap_or(0);
            let ub = st.uvs.len();
            let copy: Vec<[f32; 2]> = st.uvs[old_u0..old_u0 + pairs].to_vec();
            st.uvs.extend(copy);
            for v in &mut st.verts[vb..vb + nv] {
                if v.uv != crate::softvertex::NO_UV {
                    v.uv = v.uv.wrapping_sub(old_u0 as u32).wrapping_add(ub as u32);
                }
            }
            c.tcoords = Some(ub);
        }
    }
    g.models.push(c);
    g.models.len() - 1
}

/// Draws the blur copies of a model and of its brothers [soft 0x08275ce0 blurTraverse]:
/// `blurTraverse(model, start, end)`; a loop over the brothers, each also followed down its sons.
///
/// First the abort check ([`GeometryEnv::check_abort`]: `SI_cusCheckRenderAbort`, and, unless
/// 0x08679400, the tablet and the buttons 0x66 and 7, which end the render). A model whose kind
/// is 1, 2, 4, 7 or 10 and that does not move gets `process(copy)` and `freeModel`; one that
/// moves is drawn with a degree of existence at the step ([`blur_step`]), then
/// at each time of [`si_math::geometry::BlurTimes`] a fresh `animate` + `SI_collect` + `copyModel`
/// + `transForm` + `process` + `freeModel`.
fn blur_traverse<W: BlurWorld + ?Sized>(st: &mut ShadeState, g: &mut Geometry, w: &mut W, b: &mut BlurState, model: Option<usize>, start: f32, end: f32) {
    let mut cur = model;
    while let Some(n) = cur {
        w.check_abort();
        if renderable(g.models[n].kind) {
            if !b.moving[n] {
                if let Some(c) = b.copy[n] {
                    process(st, g, c);
                }
                free_model(g, n);
            } else {
                let (step, fraction) = blur_step(start, end, b.movement[n], b.threshold);
                set_existance(st, g, n, fraction);
                if let Some(c) = b.copy[n] {
                    process(st, g, c);
                }
                let c = copy_model(st, g, n, b.pairs[n]);
                trans_form(st, g, c);
                process(st, g, c);
                free_model(g, n);
                let mut t = (x(start) + step).to_f32();
                while (x(end) - step).gt(x(t)) {
                    w.animate(g, t, false);
                    w.collect(st, g, n);
                    b.pairs[n] = w.pair_count(n);
                    let c = copy_model(st, g, n, b.pairs[n]);
                    trans_form(st, g, c);
                    process(st, g, c);
                    free_model(g, n);
                    t = (x(t) + step).to_f32();
                }
            }
        }
        let son = g.models[n].son;
        blur_traverse(st, g, w, b, son, start, end);
        cur = g.models[n].brother;
    }
}
