// SPDX-License-Identifier: MIT
//! `si_initDof`, `shadowBuffer` and `zSample` bound into the frame (finding 0012, section
//! 17).
//!
//! // adapter: not in place
//!
//! The three are ported in place, on the memory image; `SI_update` and everything it calls
//! besides are still ported on this crate's own records. Until those are converted, what
//! stands here carries a frame's state into an image, runs the routine there and carries
//! its results back. The image is one of this module's own, made for the call: it holds
//! only what the routine reads. Nothing here reproduces a function of the original.

use crate::adapter::{camera_from_image, camera_to_image, controls_from_image, controls_to_image};
use crate::camera::camera;
use crate::geometry::{traverse, Geometry, GeometryEnv};
use crate::img::{f, g, Outside};
use crate::shade::ShadeState;
use crate::softtriangle::{Scene, Tessellators};
use crate::update::{init_viewport, set_bounds, BufferSizes, DofSetup, Options, Prepared, Settings, View};
use si_math::types::Matrix;
use si_mem::{Map, Mem};
use std::collections::HashMap;

fn matrix_at(m: &Mem, at: u32) -> Matrix {
    core::array::from_fn(|i| core::array::from_fn(|j| m.f32(at + 16 * i as u32 + 4 * j as u32)))
}

fn store_matrix(m: &mut Mem, at: u32, v: &Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(at + 16 * i as u32 + 4 * j as u32, *e);
        }
    }
}

/// The token this module leaves in `SI_renChunk`: chunk memory is the image's heap here.
const CHUNK: u32 = 0x00c0_ffee;

/// What `si_initDof` calls: chunk memory (the heap), the environment (no variable is set:
/// the environment of the render is not part of a scene), the status line.
struct DofCalls<'a> {
    notes: &'a mut Vec<String>,
}

impl Outside for DofCalls<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match addr {
            f::SI_CHUNK_MALLOC => m.malloc(args[1]),
            f::GETENV => 0,
            f::SI_STATUS_DISPLAY_MESSAGE => {
                self.notes.push(String::from_utf8_lossy(&m.cstr(args[0])).into_owned());
                0
            }
            _ => 0,
        }
    }
}

/// `si_initDof()` where `SI_update` calls it: the camera copy's depth of field members, the
/// sampling level, the two picture widths and the window's last column and line go into an
/// image, [`crate::camera::init_dof`] runs there, and what it left comes back: the three
/// depths and the factor the shading model reads (0x087906c0, c8, d0; 0x08790b2c), the
/// line counts (0x08790b28, 0x08790b2c), the grown window (0x087907b4, 0x087907bc; the
/// clipper's window is the same two words), the limit, the default radius and the filter
/// tables (0x08790b24, 0x08790b30, 0x08790b38).
pub fn init_dof(p: &mut Prepared, notes: &mut Vec<String>) {
    // (0x087906ac is the camera copy's word at +0x6c)
    p.st.glb.dof = p.camera.dof;
    if p.camera.dof == 0 {
        return;
    }
    let mut m = Mem::with_map(Map::wide());
    camera_to_image(&mut m, g::SI_DATABASE, &p.camera);
    controls_to_image(&mut m, &p.settings, &p.options);
    m.set_f32(0x0879_07b4, p.view.window[1]);
    m.set_f32(0x0879_07bc, p.view.window[3]);
    m.set_u32(g::SI_REN_CHUNK, CHUNK);
    crate::camera::init_dof(&mut m, &mut DofCalls { notes });
    p.st.glb.dof_z0 = m.f32(0x0879_06c0);
    p.st.glb.dof_z1 = m.f32(0x0879_06c8);
    p.st.glb.dof_z2 = m.f32(0x0879_06d0);
    p.st.glb.dof_scale = m.i32(0x0879_0b2c);
    p.view.dof_lines = m.i32(0x0879_0b28);
    p.view.dof_half = m.i32(0x0879_0b2c);
    p.view.window[1] = m.f32(0x0879_07b4);
    p.view.window[3] = m.f32(0x0879_07bc);
    p.g.window.xmax = p.view.window[1];
    p.g.window.ymax = p.view.window[3];
    let lines = p.view.dof_lines.max(0) as u32;
    let table = m.u32(0x0879_0b38);
    let filters = (0..(p.view.dof_half.max(-2) + 2) as u32)
        .map(|k| {
            let rows = m.u32(table + 4 * k);
            (0..lines).map(|i| (0..lines).map(|j| m.f32(m.u32(rows + 4 * i) + 4 * j)).collect()).collect()
        })
        .collect();
    p.dof = DofSetup { limit: m.f32(0x0879_0b24), radius: m.i32(0x0879_0b30), filters };
}

/// The converters that are ported, and nothing for the others: what a depth-map pass
/// tessellates with unless its caller has more.
pub struct Ported;
impl Tessellators for Ported {}

struct Collect<'a>(&'a mut Scene, &'a mut dyn Tessellators);
impl GeometryEnv for Collect<'_> {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.0.collect(st, g, node, &mut *self.1);
    }
}

/// A frame's state as a light's depth-map pass needs it: what `shadowBuffer` saves, changes
/// and puts back lives in copies here.
pub struct ShadowPass<'a> {
    /// The scene's source geometry and the nodes (the pass works on copies: what the
    /// original makes during it comes from chunk memory it gives back, or is made again
    /// by the frame's own traversal).
    pub scene: &'a Scene,
    pub g: &'a Geometry,
    pub settings: Settings,
    pub options: Options,
    pub camera: crate::camera::Camera,
    pub root: Option<usize>,
    /// The frame's tables: the depth-map pass uses the frame's column table.
    pub buffers: BufferSizes,
    pub empty_bounds: [f32; 4],
    /// `SI_stats` +0x3c: the counter of depth cells.
    pub depth_cells: &'a mut i32,
    pub notes: &'a mut Vec<String>,
    /// The C library's `drand48` state (for `si_initJitter`).
    pub rng: &'a mut crate::jitter::Drand48,
    /// The converters the pass's traversal calls through `SI_collect`.
    pub tess: &'a mut dyn Tessellators,
}

/// The pass's own copies while `shadowBuffer` runs.
struct Pass<'a> {
    st: ShadeState,
    g: Geometry,
    scene: Scene,
    view: View,
    settings: Settings,
    camera: crate::camera::Camera,
    root: Option<usize>,
    columns: i16,
    notes: &'a mut Vec<String>,
    tess: &'a mut dyn Tessellators,
    fatal: bool,
}

impl Pass<'_> {
    /// The scan-line records, their triangles and the triangles' corners as the original
    /// has them in memory, for `zSample`: a `slice` of 0x28 bytes per record, linked through
    /// +0x1c from the table at 0x087908ec in the order the filing routines leave (the
    /// record filed last first); a `tn` of 0x60 bytes per triangle met (the members
    /// `zSample` and `SI_Intersect2` read: corners, normal, 1 / d, axis, edges, model
    /// number); an `SI_RVertex` of 0x4c bytes per corner met (its position, +0x20). The
    /// column table (0x087908f0) is the frame's in the original; here it gets the frame's
    /// length or the map's window, whichever is longer.
    fn lay_out(&self, m: &mut Mem) {
        let table = m.u32(0x0879_08ec);
        let entries = (m.heap_blocks().iter().find(|b| b.0 == table).map_or(0, |b| b.1) / 4) as usize;
        let mut tn_at: HashMap<usize, u32> = HashMap::new();
        let mut vert_at: HashMap<usize, u32> = HashMap::new();
        for (y, list) in self.g.bins.lines.iter().enumerate().take(entries) {
            let mut next = 0u32;
            for s in list {
                let tn = match tn_at.get(&s.tn) {
                    Some(a) => *a,
                    None => {
                        let t = &self.st.tns[s.tn];
                        let a = m.calloc(1, 0x60);
                        for (k, v) in t.v.iter().enumerate() {
                            let va = match v {
                                None => 0,
                                Some(i) => *vert_at.entry(*i).or_insert_with(|| {
                                    let va = m.calloc(1, 0x4c);
                                    let w = self.st.verts[*i].world;
                                    m.set_f32(va + 0x20, w.x);
                                    m.set_f32(va + 0x24, w.y);
                                    m.set_f32(va + 0x28, w.z);
                                    va
                                }),
                            };
                            m.set_u32(a + 4 * k as u32, va);
                        }
                        for (o, v) in [(0x0c, t.n), (0x28, t.edge[0]), (0x34, t.edge[1]), (0x40, t.edge[2])] {
                            m.set_f32(a + o, v.x);
                            m.set_f32(a + o + 4, v.y);
                            m.set_f32(a + o + 8, v.z);
                        }
                        m.set_f32(a + 0x18, t.d);
                        m.set_f32(a + 0x1c, t.inv_d);
                        m.set_i16(a + 0x20, t.axis);
                        m.set_f32(a + 0x24, t.inv_n);
                        m.set_u16(a + 0x50, t.flags);
                        m.set_u16(a + 0x52, t.flags2);
                        m.set_i16(a + 0x58, t.w58);
                        tn_at.insert(s.tn, a);
                        a
                    }
                };
                let a = m.calloc(1, 0x28);
                m.set_f32(a + 0x04, s.x0);
                m.set_f32(a + 0x08, s.dx0);
                m.set_f32(a + 0x0c, s.x1);
                m.set_f32(a + 0x10, s.dx1);
                m.set_i32(a + 0x14, s.y_end);
                m.set_u32(a + 0x18, tn);
                m.set_u32(a + 0x1c, next);
                next = a;
            }
            m.set_u32(table + 4 * y as u32, next);
        }
        let window = si_x87::x(self.view.window[1]).to_i32(si_x87::Round::Zero).max(0) as u32 + 100;
        let columns = m.calloc(window.max(self.columns.max(0) as u32), 4);
        m.set_u32(0x0879_08f0, columns);
    }
}

impl Outside for Pass<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match addr {
            f::SI_CHUNK_MALLOC => return m.malloc(args[1]),
            f::SI_CHUNK_SAVE_CURRENT => m.set_u32(args[1], 1),
            f::SI_CHUNK_RECLAIM => {}
            f::SI_INIT_VIEWPORT => {
                // the controls and the camera copy as shadowBuffer has just made them
                (self.settings, _) = controls_from_image(m);
                self.camera = camera_from_image(m, g::SI_DATABASE);
                init_viewport(&self.camera, &self.settings, &mut self.view);
                for (i, v) in self.view.window.iter().enumerate() {
                    m.set_f32(0x0879_07b0 + 4 * i as u32, *v);
                }
                m.set_f32(0x0879_07c0, self.view.minus_near);
                m.set_f32(0x0879_07c4, self.view.minus_far);
            }
            f::SI_CAMERA => {
                if let Err(text) = camera(&self.camera, &self.settings, &mut self.view, &mut self.st.camera) {
                    self.notes.push(format!("fatal: {text}"));
                    self.fatal = true;
                }
                store_matrix(m, g::SI_DATABASE + 0x1ac, &self.st.camera.world_to_raw_mat);
                store_matrix(m, g::SI_DATABASE + 0x1ec, &self.st.camera.raw_to_world_mat);
                store_matrix(m, g::SI_DATABASE + 0x22c, &self.st.camera.vect_mat);
                store_matrix(m, g::SI_DATABASE + 0x26c, &self.st.camera.prsp_mat);
                for (i, v) in self.view.dir.iter().enumerate() {
                    m.set_f32(0x0879_07c8 + 4 * i as u32, *v);
                }
            }
            // si_traverseExpandedInstance (0x082739e0) is not ported
            f::SI_TRAVERSE_EXPANDED_INSTANCE => {}
            f::SI_TRAVERSE => {
                if self.fatal {
                    return 0;
                }
                let v = &self.view;
                self.g.window = si_math::geometry::ClipBox { xmin: v.window[0], xmax: v.window[1], ymin: v.window[2], ymax: v.window[3], yon: v.minus_near, hither: v.minus_far };
                self.st.camera.near_z = v.minus_near;
                // the table shadowBuffer has just taken and cleared
                let table = m.u32(0x0879_08ec);
                let entries = m.heap_blocks().iter().find(|b| b.0 == table).map_or(0, |b| b.1) / 4;
                self.g.bins.lines = vec![Vec::new(); entries as usize];
                self.g.raytrace = m.u16(0x0879_0c7c) & 1 != 0;
                self.st.glb.raytrace = self.g.raytrace;
                let root = self.root;
                traverse(&mut self.st, &mut self.g, &mut Collect(&mut self.scene, &mut *self.tess), root);
            }
            f::SI_SET_BOUNDS => {
                self.view.bounds = self.g.bins.bounds;
                set_bounds(m.i32(g::SI_DATABASE + 0x6c), &self.settings, &mut self.view);
                for (i, v) in self.view.region.iter().enumerate() {
                    m.set_i32(0x0879_0790 + 4 * i as u32, *v);
                }
            }
            f::Z_SAMPLE => {
                if !self.fatal {
                    self.lay_out(m);
                    crate::lights::z_sample(m, self, args[0]);
                }
            }
            _ => {}
        }
        0
    }
}

impl crate::lights::LightsEnv for ShadowPass<'_> {
    /// `shadowBuffer(light)` where `si_lights` calls it: the controls, the camera copy, the
    /// renderer's inverse view matrix, the counter of depth cells and the light's record go
    /// into an image; [`crate::lights::shadow_buffer`] runs there, its callees answered by
    /// the ports on copies of the frame's state seen from the light (`si_initViewport`,
    /// `SI_camera`, `si_traverse`, `si_setBounds`) and by [`crate::lights::z_sample`] on
    /// the scan-line records laid out in the image; the light's two matrices and the map's
    /// rows come back.
    fn shadow_buffer(&mut self, st: &mut ShadeState, light: usize) {
        let mut m = Mem::with_map(Map::wide());
        controls_to_image(&mut m, &self.settings, &self.options);
        camera_to_image(&mut m, g::SI_DATABASE, &self.camera);
        store_matrix(&mut m, g::SI_DATABASE + 0x1ac, &st.camera.world_to_raw_mat);
        m.set_i32(g::SI_STATS + 0x3c, *self.depth_cells);
        m.set_u32(g::SI_REN_CHUNK, CHUNK);
        m.set_u32(g::DB_ROOT, self.root.map_or(0, |r| r as u32 + 1));
        let l = &st.lights[light];
        let rec = m.calloc(1, 0x240);
        for (o, v) in [(0x34, l.pos_given), (0x58, l.interest)] {
            m.set_f32(rec + o, v.x);
            m.set_f32(rec + o + 4, v.y);
            m.set_f32(rec + o + 8, v.z);
        }
        m.set_f32(rec + 0x70, l.cone);
        m.set_f32(rec + 0x74, l.spread);
        store_matrix(&mut m, rec + 0x94, &l.depth_mat);
        store_matrix(&mut m, rec + 0xd4, &l.xy_mat);
        m.set_i32(rec + 0x118, l.map_size);
        m.set_i32(rec + 0x15c, l.selective);
        m.set_i32(rec + 0x160, l.select_id);
        let table = m.calloc(l.select_table.len().max(1) as u32, 4);
        for (i, v) in l.select_table.iter().enumerate() {
            m.set_i32(table + 4 * i as u32, *v);
        }
        m.set_u32(rec + 0x168, table);
        let mut g2 = self.g.clone();
        g2.bins = Default::default();
        g2.bins.bounds = self.empty_bounds;
        g2.trace = Default::default();
        let mut pass = Pass { st: st.clone(), g: g2, scene: self.scene.clone(), view: View::default(), settings: self.settings, camera: self.camera, root: self.root, columns: self.buffers.columns, notes: &mut *self.notes, tess: &mut *self.tess, fatal: false };
        crate::lights::shadow_buffer(&mut m, &mut pass, rec);
        if let Some(fault) = m.fault() {
            pass.notes.push(format!("shadowBuffer: the image faulted: {fault:?}"));
        }
        *self.depth_cells = m.i32(g::SI_STATS + 0x3c);
        let size = m.i32(rec + 0x118).max(0) as u32;
        let rows = m.u32(rec + 0x114);
        let l = &mut st.lights[light];
        l.depth_mat = matrix_at(&m, rec + 0x94);
        l.xy_mat = matrix_at(&m, rec + 0xd4);
        l.zmap = (0..size).map(|y| (0..size).map(|x| m.f32(m.u32(rows + 4 * y) + 4 * x)).collect()).collect();
    }

    fn drand48(&mut self) -> f64 {
        self.rng.next()
    }
}
