// SPDX-License-Identifier: MIT
//! The geometry stage's routines that work on a model node: the part of the original's
//! `geometry.c` that `crates/si-math/src/geometry.rs` left as kernels (finding 0012, section
//! 3a). The ten routines ported there (the clipper's five, `traceList`, `barr`, the two
//! filing routines, `maxPsMovement`) are used from there.
//!
//! The original walks `__SI_Model` records; here a node is a [`RenModel`]: the members of
//! the record these routines read, with the render vertices and triangles of all nodes in
//! the pools of [`ShadeState`] (`verts`, `tns`, `uvs`). What the original takes from chunk
//! memory (the vertices and triangles the clipper makes) is pushed on those pools.

use crate::shade::ShadeState;
use crate::types::{Matrix, Vector};
use si_math::geometry::{
    area_sort, barr, clip_fan, clip_fate, existance, face_normal, point_sort, pushed_along_normal, trace_list, transform_flips_normals,
    unit_normal, ClipBox, ClipEnv, ClipFate, Clipper, Tn, TraceList, YBins, TN_DROPPED, TN_MATERIAL_6, TN_VOID,
};
use si_math::matrix::{invert, mmul, transpose};
use si_math::types::Pspoint;
use si_x87::{fmov, x};

/// The members of `__SI_Model` (0x430 bytes) the geometry stage reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenModel {
    /// +0x08, bit 3: the node's winding is reversed.
    pub reversed: bool,
    /// +0x30: 1 patch, 2 face, 4 mesh, 7 metaball system, 10 NURBS surface are rendered.
    pub kind: i32,
    /// +0x44 `latdep`: the node whose scaling and global matrix `transForm` uses (a node is
    /// usually its own). An index into the node list.
    pub latdep: usize,
    /// +0x16c `gscl`, +0x3a0 `cscl`, +0x1c4 `gmat`: read through `latdep`.
    pub gscl: Vector,
    pub cscl: Vector,
    pub gmat: Matrix,
    /// +0x20c: the node's materials (indices into [`ShadeState::materials`]); `transForm`
    /// looks at the first.
    pub materials: Vec<usize>,
    /// +0x214: flag word OR-ed into every triangle's.
    pub flags: u16,
    /// +0x216: texture maps of the node itself (pairs per vertex it adds to the
    /// material's).
    pub n_maps: i16,
    /// +0x228, +0x22c: the render vertices: count and first index in the pool; `None` is a
    /// null array.
    pub vert_count: i32,
    pub verts: Option<usize>,
    /// +0x230, +0x234: the render triangles: count and first index in the pool; `None` is
    /// a null array. The node's list runs from the first through [`Tn::next`].
    pub tn_count: i32,
    pub tns: Option<usize>,
    /// +0x330: the node's array of texture coordinate pairs that vertex +0x40 points into
    /// (first index in [`ShadeState::uvs`]), and +0x42c: its second per-vertex array (vertex
    /// +0x48), kept apart. Only whether they are there matters to this file.
    pub tcoords: Option<usize>,
    pub uv2: Option<usize>,
    /// +0x2c0 `brother`, +0x2c4 `son`.
    pub brother: Option<usize>,
    pub son: Option<usize>,
    /// +0x354 `elem_id`, resolved: the node the MODELS chapter of the element dictionary
    /// holds under this node's element number (the node itself, or the original of a copy).
    pub registered: usize,
}

/// The kinds `si_traverse` and `blurTraverse` render (jump tables 0x08654518, 0x08654568).
pub fn renderable(kind: i32) -> bool {
    matches!(kind, 1 | 2 | 4 | 7 | 10)
}

/// The geometry stage's own state: the node list and the globals it reads and fills.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub models: Vec<RenModel>,
    /// 0x087907b0…c4: the window in samples and the two depth planes.
    pub window: ClipBox,
    /// The file-scope variables of the clipper (0x087992e0…).
    pub clipper: Clipper,
    /// 0x087908ec, 0x0879090c and `SI_stats` +0x00, +0x20: the scan-line table.
    pub bins: YBins,
    /// 0x087908f4, 0x08790924 and `SI_stats` +0x24: the trace list.
    pub trace: TraceList,
    /// Bit 0 of the byte at 0x08790c7c: ray tracing is on, `process` lists every triangle.
    pub raytrace: bool,
    /// `SI_yBinSort` (0x0878fee0) is `si_areaSort` (`initData`: adaptive supersampling)
    /// instead of `si_pointSort`.
    pub area_sort: bool,
    /// `SI_rightHand` (0x086793ec) ≠ 0 and `SI_glbManip` (0x08781560).
    pub right_hand: bool,
    pub glb_manip: i32,
    /// `SI_stats` +0x1c: triangles made (the converters' and the clipper's fans).
    pub made_tns: i32,
    /// What `SI_rawToWorld`, `SI_worldToPrsp1` and `SI_worldToPrsp2` printed (each prints a
    /// line for a zero divisor).
    pub messages: Vec<&'static str>,
}

/// Vertices and normals of a node into the renderer's space and onto the picture plane
/// [soft 0x08273acc transForm]: `transForm(node)`.
///
/// Nothing happens for a node without a triangle array. Otherwise:
///
/// 1. Whether normals are negated: the product of the three global scaling factors of the
///    `latdep` node (its `cscl` when `SI_glbManip` is 1) against the handedness, inverted
///    by the node's own reversal bit ([`transform_flips_normals`]).
/// 2. The four matrices at 0x087907ec are saved, and the raw-to-world matrix becomes
///    `gmat` × itself (`SI_mmul(latdep->gmat, M, M)`).
/// 3. Every vertex: `SI_rawToWorld(raw, world)`, `SI_worldToPrsp1(world, ps)`.
/// 4. `SI_invert` of that matrix. If it has an inverse: its translation row is cleared,
///    its transpose becomes the matrix of `SI_rawToWorldVect`, and every vertex's raw
///    normal (negated by 1) goes through it and is normalised ([`unit_normal`]). If not:
///    every triangle of the array gives all three of its corners its face normal
///    ([`face_normal`]), in array order, so a shared vertex keeps the last triangle's.
/// 5. The four matrices are put back.
/// 6. With static blur on the node's **first** material (+0x758): every vertex is pushed
///    along its normal by the material's width (+0x75c) and projected again.
///
/// A node with a triangle array and no material is read through a null pointer in step 6;
/// the Rust panics. Class: model.
pub fn trans_form(st: &mut ShadeState, g: &mut Geometry, node: usize) {
    let m = g.models[node].clone();
    let Some(tn0) = m.tns else { return };
    let l = &g.models[m.latdep];
    let scale = if g.glb_manip == 1 { l.cscl } else { l.gscl };
    let flip = transform_flips_normals(Vector::new(fmov(scale.x), fmov(scale.y), fmov(scale.z)), g.right_hand, m.reversed);
    let saved = st.camera.clone();
    st.camera.raw_to_world_mat = mmul(&l.gmat, &st.camera.raw_to_world_mat);
    let count = m.vert_count.max(0) as usize;
    // a count without an array is a read through a null pointer in the original
    let v0 = if count == 0 { 0 } else { m.verts.expect("transForm: vertices counted but no array") };
    for i in v0..v0 + count {
        let mut world = st.verts[i].world;
        g.messages.extend(st.camera.raw_to_world(st.verts[i].raw, &mut world));
        st.verts[i].world = world;
        let mut ps = st.verts[i].ps;
        g.messages.extend(st.camera.world_to_prsp1(world, &mut ps));
        st.verts[i].ps = ps;
    }
    match invert(&st.camera.raw_to_world_mat) {
        Some(mut inv) => {
            inv[3][0] = 0.0;
            inv[3][1] = 0.0;
            inv[3][2] = 0.0;
            st.camera.vect_mat = transpose(&inv);
            for i in v0..v0 + count {
                let r = st.verts[i].raw_normal;
                let n = if flip { Vector::new((-x(r.x)).to_f32(), (-x(r.y)).to_f32(), (-x(r.z)).to_f32()) } else { Vector::new(fmov(r.x), fmov(r.y), fmov(r.z)) };
                let mut out = st.verts[i].normal;
                g.messages.extend(st.camera.raw_to_world_vect(n, &mut out));
                st.verts[i].normal = unit_normal(out);
            }
        }
        None => {
            for t in tn0..tn0 + m.tn_count.max(0) as usize {
                // a null corner is dereferenced by the original
                let c = st.tns[t].v.map(|v| v.expect("transForm: a triangle without a corner"));
                let n = face_normal(st.verts[c[0]].world, st.verts[c[1]].world, st.verts[c[2]].world);
                for v in c {
                    st.verts[v].normal = n;
                }
            }
        }
    }
    st.camera = saved;
    let first = &st.materials[*m.materials.first().expect("transForm: a node without a material")];
    if first.static_blur != 0 {
        let width = first.blur_width;
        for i in v0..v0 + count {
            let world = pushed_along_normal(st.verts[i].world, st.verts[i].normal, width);
            st.verts[i].world = world;
            let mut ps = st.verts[i].ps;
            g.messages.extend(st.camera.world_to_prsp1(world, &mut ps));
            st.verts[i].ps = ps;
        }
    }
}

/// The filing routine `SI_yBinSort` points to, on the list from `head`.
fn y_bin_sort(st: &ShadeState, g: &mut Geometry, head: Option<usize>) {
    if g.area_sort {
        area_sort(&st.tns, head, &st.verts, &g.window, &mut g.bins);
    } else {
        point_sort(&st.tns, head, &st.verts, &g.window, &mut g.bins);
    }
}

/// Flags, rejection and depth clipping of a node's triangles [soft 0x0827400c clip]:
/// `clip(node)`.
///
/// w = the flag word (+0x214) of the node registered under this node's element number.
/// For each triangle of the node's list: its flag word becomes w | the low half of its
/// material's (+0x6e0), and +0x52 becomes (that & 0x100) | the material's static blur word.
/// A material of shading model 6 (shadow object) adds 0x3000 and ends the triangle's
/// treatment; a null corner adds 0x8000 and ends it. Otherwise [`clip_fate`]: rejected
/// (0x1000), kept, or cut by a depth plane. A cut triangle goes through the clipper with
/// `nMap` = the node's own map count (+0x216) + the material's (+0x6e8); if more than two
/// vertices come out, the fan (0, i, i + 1) is made of new triangles that copy the
/// material, +0x58, +0x52 and +0x50, each of which is finished (`barr`) and filed
/// (`*SI_yBinSort`) at once, and the cut triangle gets 0x1000. With two or fewer vertices
/// the cut triangle stays as it is, unmarked. Class: copy (comparisons; the arithmetic is
/// the clipper's).
pub fn clip(st: &mut ShadeState, g: &mut Geometry, node: usize) {
    let w = g.models[g.models[node].registered].flags;
    let own_maps = g.models[node].n_maps;
    let mut cur = g.models[node].tns;
    while let Some(t) = cur {
        cur = st.tns[t].next;
        let m = &st.materials[st.tns[t].material as usize];
        let flags = w | m.flags;
        st.tns[t].flags = flags;
        st.tns[t].flags2 = (flags & 0x100) | m.static_blur as u16;
        if m.model == 6 {
            st.tns[t].flags = flags | TN_MATERIAL_6 | TN_DROPPED;
            continue;
        }
        let [Some(a), Some(b), Some(c)] = st.tns[t].v else {
            st.tns[t].flags |= TN_VOID;
            continue;
        };
        match clip_fate([&st.verts[a], &st.verts[b], &st.verts[c]], &g.window) {
            ClipFate::Keep => {}
            ClipFate::Reject => st.tns[t].flags |= TN_DROPPED,
            ClipFate::Cut => {
                let n_map = own_maps.wrapping_add(m.n_maps);
                let corners = [st.verts[a], st.verts[b], st.verts[c]];
                {
                    let camera = &st.camera;
                    let messages = &mut g.messages;
                    let mut project = |p: &Vector| {
                        let mut out = Pspoint::default();
                        messages.extend(camera.world_to_prsp2(*p, &mut out));
                        out
                    };
                    let mut env = ClipEnv { planes: &g.window, uv: &mut st.uvs, project: &mut project };
                    g.clipper.clip_triangle([&corners[0], &corners[1], &corners[2]], n_map, &mut env);
                }
                let n = g.clipper.output.len() as i16;
                if n > 2 {
                    let base = st.verts.len();
                    st.verts.extend(g.clipper.output.iter().copied());
                    let old = st.tns[t].clone();
                    for f in clip_fan(n) {
                        g.made_tns += 1;
                        let new = st.tns.len();
                        st.tns.push(Tn {
                            v: [Some(base + f[0]), Some(base + f[1]), Some(base + f[2])],
                            material: old.material,
                            w58: old.w58,
                            flags2: old.flags2,
                            flags: old.flags,
                            next: None,
                            ..Tn::default()
                        });
                        barr(&mut st.tns, Some(new), &st.verts);
                        y_bin_sort(st, g, Some(new));
                    }
                    st.tns[t].flags |= TN_DROPPED;
                }
            }
        }
    }
}

/// One node through the pipeline [soft 0x08273fa8 process]: `process(node)`. Nothing for a
/// node whose triangle count is 0; else [`clip`], with ray tracing on `traceList` of the
/// node's list, then `barr` and the filing routine on it. (The clipper's fan triangles are
/// not in the node's list: the trace list gets the uncut triangle, flagged 0x1000.)
pub fn process(st: &mut ShadeState, g: &mut Geometry, node: usize) {
    if g.models[node].tn_count == 0 {
        return;
    }
    clip(st, g, node);
    let head = g.models[node].tns;
    if g.raytrace {
        trace_list(&st.tns, head, &st.verts, &mut g.trace);
    }
    barr(&mut st.tns, head, &st.verts);
    y_bin_sort(st, g, head);
}

/// What the traversals call outside `geometry.c`.
pub trait GeometryEnv {
    /// `SI_cusCheckRenderAbort()` and the interactive abort check (the middle mouse button
    /// or Escape: `SI_voluntaryExit(4, display off)`), made once per node.
    fn check_abort(&mut self) {}
    /// `SI_collect(node)` (`softtriangle.c`): builds the node's render vertices and
    /// triangles.
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize);
}

/// The scene without motion blur [soft 0x08273910 si_traverse]: `si_traverse(node)`. For
/// the node and each following brother: the abort checks; a node of a renderable kind is
/// collected, transformed and processed; then its sons, whatever its kind.
pub fn traverse<E: GeometryEnv + ?Sized>(st: &mut ShadeState, g: &mut Geometry, env: &mut E, node: Option<usize>) {
    let mut cur = node;
    while let Some(n) = cur {
        env.check_abort();
        if renderable(g.models[n].kind) {
            env.collect(st, g, n);
            trans_form(st, g, n);
            process(st, g, n);
        }
        traverse(st, g, env, g.models[n].son);
        cur = g.models[n].brother;
    }
}

/// The share of the shutter interval one copy of a moving node stands for
/// [soft 0x082762e0 setExistance]: `setExistance(node, fraction)`. The fraction is held
/// between 0.2 and 0.8 ([`existance`]) and written to the material (+0x74c) of every
/// triangle of the node's list; unless that material has flag 0x200 it also gets flag
/// 0x100 (the copy shows what is behind it). Class: copy.
pub fn set_existance(st: &mut ShadeState, g: &Geometry, node: usize, fraction: f32) {
    let v = existance(fraction);
    let mut cur = g.models[node].tns;
    while let Some(t) = cur {
        cur = st.tns[t].next;
        let m = &mut st.materials[st.tns[t].material as usize];
        m.work.existence = v;
        if m.flags & 0x200 == 0 {
            m.flags |= 0x100;
        }
    }
}

/// Gives a node's render arrays back [soft 0x082761ac freeModel]: `freeModel(node)` frees
/// and clears, each if it is there and in this order, the vertex array (+0x22c), the second
/// texture coordinate array (+0x42c), the first (+0x330) and the triangle array (+0x234),
/// and zeroes both counts. Here the pools keep the records; the node forgets them. Returns
/// which of the four were freed, in that order.
pub fn free_model(g: &mut Geometry, node: usize) -> [bool; 4] {
    let m = &mut g.models[node];
    let freed = [m.verts.take().is_some(), m.uv2.take().is_some(), m.tcoords.take().is_some(), m.tns.take().is_some()];
    m.vert_count = 0;
    m.tn_count = 0;
    freed
}
