// SPDX-License-Identifier: MIT
//! The state scene set-up computes, and the set-up routines that are plain arithmetic on it:
//! `si_setBounds` of the original's `update.c` and, kept beside it because they fill the
//! same record, `si_initViewport`, `initBuffers` and `initFilter` of `initialize.c` (finding
//! 0012, section 4). `SI_camera` is in [`crate::camera`].
//!
//! The original keeps all of this in two global records: `SI_database` (0x08790640), which
//! starts with a copy of the scene's camera, and `SI_ren` (0x08790be0), the render options.
//! Here [`Settings`] is what the caller of `SI_image` puts in `SI_ren`, and [`View`] what
//! set-up derives from it and the camera.

use crate::bsptree::{init_ancestry, BspBuild, BspParams, BspStats};
use crate::camera::{camera, Camera};
use crate::geometry::{traverse, Geometry, GeometryEnv};
use crate::render::{filter_for_level, Sampler, Slice, Supersample};
use crate::shade::ShadeState;
use crate::trace::{Bsp, Link};
use crate::geometry::RenModel;
use crate::types::Rgbazo;
use si_math::types::Matrix;
use si_x87::{fmov, x, Round};

/// The render options set-up reads (`SI_ren` and the camera copy).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    /// 0x0879117a, 0x0879117c: the picture's width and height in pixels.
    pub width: i16,
    pub height: i16,
    /// 0x08790d34: pixel ratio.
    pub pixel_ratio: f32,
    /// 0x08790d38: sampling level L.
    pub level: i16,
    /// 0x08790d1c, 0x08790d20: field rendering, and which field.
    pub field: i16,
    pub field_no: i16,
    /// 0x08790e04: a part of the picture only.
    pub crop: i32,
    /// 0x08790e0c, 0x08790e10, 0x08790e14, 0x08790e18: the crop's four floats: what is cut
    /// off on the left, on the right, at the top and at the bottom (by their use in
    /// `si_initViewport`: added to the picture's least x and y, taken off its greatest).
    pub crop_left: f32,
    pub crop_right: f32,
    pub crop_top: f32,
    pub crop_bottom: f32,
    /// 0x08790e1c, 0x08790e1e: the crop's width and height in pixels.
    pub crop_width: i16,
    pub crop_height: i16,
    /// 0x08790e22, 0x08790e28: a resumed picture and its last line still to render.
    pub resume: i16,
    pub resume_line: i32,
}

/// What set-up derives (`SI_database` from +0x140 on, and a few words of `SI_ren`).
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// 0x08790780: the picture: x min (0), x max (width − 1), y min (0), y max.
    pub picture: [f32; 4],
    /// 0x08790790: the rendered region in samples: x min, x max, y min, y max.
    pub region: [i32; 4],
    /// 0x087907a0: the same in pixels.
    pub region_px: [i32; 4],
    /// 0x087907b0: the window in samples: x min, x max, y min, y max.
    pub window: [f32; 4],
    /// 0x087907c0: 0 − the camera's near distance; 0x087907c4: 0 − its far distance. (The
    /// clipper of `si_math::geometry` calls them `yon` and `hither`; by their values the
    /// first is the near plane's z and the second the far plane's.)
    pub minus_near: f32,
    pub minus_far: f32,
    /// 0x087907c8: the first ray's direction x, y, z and the steps per sample in x and y.
    pub dir: [f32; 5],
    /// 0x0879090c: the picture-plane box of what the filing routines accepted: x min, x max,
    /// y min, y max; `si_setBounds` turns it from samples into pixels with a margin of one.
    pub bounds: [f32; 4],
    /// 0x08790934: 0.5 with field rendering, else 0 (no reader found in the files read).
    pub field_half: f32,
    /// 0x08790938, 0x0879093c: the working filter's width 2L − 1 and its weights.
    pub filter_size: i16,
    pub filter: [[f32; 11]; 11],
    /// 0x08790b20: the length of the row buffers.
    pub buffer_len: i32,
    /// 0x08790b28, 0x08790b2c: depth of field: the number of blur lines and half of it.
    pub dof_lines: i32,
    pub dof_half: i32,
}

impl Default for View {
    fn default() -> View {
        View {
            picture: [0.0; 4],
            region: [0; 4],
            region_px: [0; 4],
            window: [0.0; 4],
            minus_near: 0.0,
            minus_far: 0.0,
            dir: [0.0; 5],
            bounds: [0.0; 4],
            field_half: 0.0,
            filter_size: 0,
            filter: [[0.0; 11]; 11],
            buffer_len: 0,
            dof_lines: 0,
            dof_half: 0,
        }
    }
}

/// The picture, the window in samples and the two depth planes
/// [soft 0x082768c4 si_initViewport]: `si_initViewport()`.
///
/// Picture: x from 0 to width − 1, y from 0 to height − 1; with field rendering the last
/// line is height / 2 − 1 (16-bit signed division) and the float at 0x08790934 is 0.5, else
/// 0. With L the sampling level, the window is in samples:
///
/// - whole picture: x from 0 to r32((x max × L + 2L) − 2), y from 0 to r32((2L + L × y max)
///   − 2);
/// - a crop: x min = r32(((0 + left) × L + 2L) − 2), y min = r32(((0 + bottom) × L + 2L) −
///   **3**), x max = r32(((x max − right) × L + 2L) − **1**), y max = r32((L × (y max − top)
///   + 2L) − 2): the four margins are not alike;
/// - a resumed picture: y max = r32((L × last line + 2L) − 2) instead.
///
/// The depth planes: 0x087907c4 = 0 − far, 0x087907c0 = 0 − near. Class: model.
pub fn init_viewport(cam: &Camera, s: &Settings, v: &mut View) {
    v.picture[0] = 0.0;
    v.picture[1] = (x(s.width as i32) - si_x87::F80::ONE).to_f32();
    v.picture[2] = 0.0;
    if s.field != 0 {
        let h = s.height;
        // (height + its sign bit) >> 1 in sixteen bits: a signed division by two
        let half = (h.wrapping_add(((h as u16) >> 15) as i16) >> 1) as i32 - 1;
        v.picture[3] = x(half).to_f32();
        v.field_half = 0.5;
    } else {
        v.picture[3] = x(s.height as i32 - 1).to_f32();
        v.field_half = 0.0;
    }
    let l = x(s.level as i32);
    let two_l = l + l;
    let [p0, p1, p2, p3] = v.picture;
    if s.crop != 0 {
        v.window[0] = (((x(p0) + s.crop_left) * l + two_l) - 2.0f64).to_f32();
        v.window[2] = (((x(p2) + s.crop_bottom) * l + two_l) - 3.0f64).to_f32();
        v.window[1] = (((x(p1) - s.crop_right) * l + two_l) - si_x87::F80::ONE).to_f32();
        v.window[3] = ((l * (x(p3) - s.crop_top) + two_l) - 2.0f64).to_f32();
    } else {
        v.window[0] = 0.0;
        v.window[2] = 0.0;
        v.window[1] = ((x(p1) * l + two_l) - 2.0f64).to_f32();
        v.window[3] = ((two_l + l * p3) - 2.0f64).to_f32();
    }
    if s.resume != 0 {
        let lines = (s.level as i32).wrapping_mul(s.resume_line);
        v.window[3] = ((x(lines) + two_l) - 2.0f64).to_f32();
    }
    v.minus_far = (x(0.0f32) - cam.far).to_f32();
    v.minus_near = (x(0.0f32) - cam.near).to_f32();
}

/// How large `initBuffers` makes each buffer [soft 0x08276ab4 initBuffers]: `initBuffers()`
/// takes them all from chunk memory, in this order: the scan-line table of
/// trunc(window y max + 100) pointers and the column table of trunc(window x max + 100)
/// pointers (both counts cut to sixteen bits, both tables cleared), the line of samples
/// (48 bytes per column: two samples), then with w = (short)(width + 10): the four byte
/// lines red, green, blue, alpha (w bytes each), the depth line (4w), and the four
/// outgoing byte lines (w each). None but the two tables is cleared.
///
/// It also adds to three counters of `SI_stats`: +0x04 both table lengths, +0x08 twice the
/// column count, +0x10 four times w.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BufferSizes {
    /// Entries of the scan-line table (0x087908ec).
    pub scan_lines: i16,
    /// Entries of the column table (0x087908f0).
    pub columns: i16,
    /// Bytes of each byte line; the sample line holds 2 × `columns` samples.
    pub line: i16,
}

/// See [`BufferSizes`]. `stats` are the three counters at `SI_stats` +0x04, +0x08, +0x10.
pub fn init_buffers(v: &View, width: i16, stats: &mut [i32; 3]) -> BufferSizes {
    let scan_lines = (x(v.window[3]) + 100.0f32).to_i32(Round::Zero) as i16;
    stats[0] = stats[0].wrapping_add(scan_lines as i32);
    let columns = (x(v.window[1]) + 100.0f32).to_i32(Round::Zero) as i16;
    stats[0] = stats[0].wrapping_add(columns as i32);
    stats[1] = stats[1].wrapping_add(2 * columns as i32);
    let line = (width as u16).wrapping_add(10) as i16;
    stats[2] = stats[2].wrapping_add(4 * line as i32);
    BufferSizes { scan_lines, columns, line }
}

/// The working filter for the sampling level [soft 0x08276ca0 initFilter]: `initFilter()`.
/// Nothing at level 1 (the width word keeps its value). Else the width becomes 2L − 1 and
/// that many rows and columns of level L's block of the static table `filters` are copied
/// to the working filter; the rest of the 11 × 11 stays. A level above 5 reads past the
/// table in the original; here that panics. Class: copy.
pub fn init_filter(level: i16, v: &mut View) {
    if level == 1 {
        return;
    }
    v.filter_size = (level as i32 * 2 - 1) as i16;
    if v.filter_size <= 0 {
        return;
    }
    let table = filter_for_level(level);
    let n = v.filter_size as usize;
    for i in 0..n {
        for j in 0..n {
            v.filter[i][j] = fmov(table[i][j]);
        }
    }
}

/// The region to render [soft 0x0826f318 si_setBounds]: `si_setBounds()`, called when the
/// geometry has been filed.
///
/// 1. With depth of field (camera +0x6c): half the blur line count is taken off all four
///    members of the picture-plane box of what was filed (0x0879090c).
/// 2. The box goes from samples to pixels with a margin: r32(min / L − 1), r32(max / L +
///    1).
/// 3. The region in pixels (0x087907a0) is that box, or with a crop the crop's rectangle
///    (left, left + width, bottom, bottom + height), held to the picture: each least value
///    is 0 if below 0, else truncated; each greatest is the picture's when above it, else
///    its own, truncated. A resumed picture's last line replaces y max.
/// 4. The region in samples (0x08790790) is the same; the row buffers get x max + 10
///    entries. Above level 1: min × L, and max × L + (2L − 1) − 1 with the filter width as
///    `initFilter` left it.
/// 5. With depth of field the blur line count is added to both greatest values and to the
///    buffer length.
///
/// Class: model.
pub fn set_bounds(dof: i32, s: &Settings, v: &mut View) {
    if dof != 0 {
        let half = x(v.dof_half);
        for b in &mut v.bounds {
            *b = (x(*b) - half).to_f32();
        }
    }
    let l = x(s.level as i32);
    let one = si_x87::F80::ONE;
    v.bounds = [((x(v.bounds[0]) / l) - one).to_f32(), ((x(v.bounds[1]) / l) + one).to_f32(), ((x(v.bounds[2]) / l) - one).to_f32(), ((x(v.bounds[3]) / l) + one).to_f32()];
    let rect = if s.crop != 0 {
        [fmov(s.crop_left), (x(s.crop_left) + x(s.crop_width as i32)).to_f32(), fmov(s.crop_bottom), (x(s.crop_bottom) + x(s.crop_height as i32)).to_f32()]
    } else {
        v.bounds
    };
    let low = |a: f32| if x(a).lt(x(0.0f32)) { 0 } else { x(a).to_i32(Round::Zero) };
    let high = |a: f32, limit: f32| if x(a).gt(x(limit)) { x(limit).to_i32(Round::Zero) } else { x(a).to_i32(Round::Zero) };
    v.region_px = [low(rect[0]), high(rect[1], v.picture[1]), low(rect[2]), high(rect[3], v.picture[3])];
    if s.resume != 0 {
        v.region_px[3] = s.resume_line;
    }
    v.region = v.region_px;
    v.buffer_len = v.region_px[1].wrapping_add(10);
    if s.level > 1 {
        let l = s.level as i32;
        let size = v.filter_size as i32;
        v.region[0] = v.region_px[0].wrapping_mul(l);
        v.region[2] = v.region_px[2].wrapping_mul(l);
        v.region[1] = size.wrapping_add(v.region_px[1].wrapping_mul(l)).wrapping_sub(1);
        v.region[3] = size.wrapping_add(v.region_px[3].wrapping_mul(l)).wrapping_sub(1);
    }
    if dof != 0 {
        v.region[1] = v.region[1].wrapping_add(v.dof_lines);
        v.region[3] = v.region[3].wrapping_add(v.dof_lines);
        v.buffer_len = v.buffer_len.wrapping_add(v.dof_lines);
    }
}

/// More of `SI_ren`: the options `initData` and `SI_update` read beside [`Settings`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// 0x08790c7c: bit 0: ray tracing is needed. `initData` clears the word; the material
    /// and light set-up of each frame set the bit.
    pub raytrace: u16,
    /// 0x08790c7e: 2 when a sequence is rendered (the scene is animated).
    pub sequence: i16,
    /// 0x08790d3a, 0x08790d3c, 0x08790d4c: adaptive supersampling, its sub-samples per
    /// pixel side and its contrast threshold.
    pub adaptive: i16,
    pub sub: i16,
    pub contrast: f32,
    /// 0x08790df0, 0x08790df4, 0x08790dfc: motion blur, the shutter time and the movement
    /// per copy in pixels.
    pub blur: i16,
    pub shutter: f32,
    pub blur_step: f32,
    /// 0x08790dc6, 0x08790dc8, 0x08790dca: timing, tree and memory statistics.
    pub timing: i16,
    pub tree_stats: i16,
    pub mem_stats: i16,
    /// 0x08790dd4, 0x08790dda: the first frame and the current one.
    pub first_frame: i16,
    pub frame: i16,
    /// 0x08791166: 2D textures on.
    pub textures_2d: i16,
    /// 0x08790d5c: the output name.
    pub name: Vec<u8>,
    /// 0x08781d9a: the picture width of `SI_defaultControl` (the controls the render
    /// commands copy over `SI_ren`), which `si_initDof` multiplies the lens's circle of
    /// confusion with.
    pub default_width: i16,
}

/// What `si_initDof` leaves beside the three depths ([`crate::shade::ShadeGlobals`]) and
/// the line counts ([`View::dof_lines`], [`View::dof_half`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DofSetup {
    /// 0x08790b24: the limit `dofSubdivid` refines below.
    pub limit: f32,
    /// 0x08790b30: the default blur radius.
    pub radius: i32,
    /// 0x08790b38: the filter tables, `[radius][row][column]`.
    pub filters: Vec<Vec<Vec<f32>>>,
}

/// The seventeen counters of `SI_stats` (0x087911e0) by their index (offset / 4). Several
/// live in the records of the routines that count: `SI_stats` +0x00 and +0x20 in
/// [`si_math::geometry::YBins`], +0x24 in [`si_math::geometry::TraceList`] (and
/// [`BspStats::links`] during the build), +0x14 and +0x18 in the clipper, +0x1c in
/// [`Geometry::made_tns`], +0x28 and +0x2c in [`Prepared::tree_counts`]; the rest are here.
pub mod stat {
    /// +0x04: entries of the scan-line and column tables.
    pub const BINS: usize = 1;
    /// +0x08: samples of the scan-line buffers.
    pub const SCANLINE: usize = 2;
    /// +0x0c: bytes of the depth of field rows. `initData` does not clear this one.
    pub const DOF: usize = 3;
    /// +0x10: bytes of the output lines.
    pub const OUTPUT: usize = 4;
    /// +0x34: texture map bytes / 5; +0x38 "frame buffer null" bytes; +0x3c shadow depth
    /// buffer cells; +0x40 bytes of motion blur copies.
    pub const TEXTURE_MAPS: usize = 13;
    pub const FB_NULL: usize = 14;
    pub const SHADOW_DEPTH: usize = 15;
    pub const COPIES: usize = 16;
}

/// Everything scene set-up fills for one frame: the state the samplers and the shading
/// model then read.
#[derive(Clone, Debug, Default)]
pub struct Prepared {
    pub camera: Camera,
    pub settings: Settings,
    pub options: Options,
    pub view: View,
    /// Triangles, vertices, materials, lights, the four matrices, the shading globals.
    pub st: ShadeState,
    /// The nodes, the scan-line table, the trace list.
    pub g: Geometry,
    /// 0x087907e8: the root node.
    pub root: Option<usize>,
    /// 0x08790928, 0x0879092c: the tree.
    pub bsp: Bsp,
    pub bsp_params: BspParams,
    /// `SI_stats` +0x28, +0x2c: inner nodes and cells made.
    pub tree_counts: [i32; 2],
    /// The counters of `SI_stats` that no other record holds ([`stat`]).
    pub stats: [i32; 17],
    /// 0x08790d40: 0.33 × the contrast threshold; 0x08790e00: sampling level × the blur
    /// step; 0x08790df8: a copy of the shutter time.
    pub contrast_third: f32,
    pub blur_threshold: f32,
    pub shutter_copy: f32,
    /// 0x08790b88: the blank sample; 0x08790bb8: the empty picture-plane box `SI_update`
    /// starts each frame's bounds from.
    pub blank: Rgbazo,
    pub empty_bounds: [f32; 4],
    /// The sizes `initBuffers` chose.
    pub buffers: BufferSizes,
    /// 0x08790ce4, 0x08790cec, 0x08790d10: what the render set-up gives `si_initFade`.
    pub fade: crate::camera::Fade,
    /// What `si_initDof` made for this frame.
    pub dof: DofSetup,
}

impl Prepared {
    /// The frame loops' state from the prepared scene. Not a routine of the original: its
    /// loops read the same globals. The scan-line records become [`Slice`]s (each line's
    /// list in the original's order: the record filed last comes first); the output
    /// buffers, the tags and the depth of field tables are left for the caller.
    pub fn sampler(&self) -> Sampler {
        let v = &self.view;
        let mut slices = Vec::new();
        let mut scan_lines = vec![None; self.g.bins.lines.len()];
        for (y, list) in self.g.bins.lines.iter().enumerate() {
            let mut next = None;
            for s in list {
                slices.push(Slice { key: 0.0, x0: s.x0, dx0: s.dx0, x1: s.x1, dx1: s.dx1, y_end: s.y_end, tn: s.tn, next, link: None, by_depth: None });
                next = Some(slices.len() - 1);
            }
            scan_lines[y] = next;
        }
        let columns = (self.buffers.columns.max(0) as usize).max((x(v.window[1]) + 100.0f32).to_i32(Round::Zero).max(0) as usize);
        Sampler {
            level: self.settings.level,
            region: v.region,
            region_y_px: [v.region_px[2], v.region_px[3]],
            picture_y: [v.picture[2], v.picture[3]],
            window_x: [v.window[0], v.window[1]],
            ss: Supersample { sub: self.options.sub, dir: v.dir, contrast: self.options.contrast, background: self.st.glb.background, unset: self.st.glb.unset, ..Supersample::default() },
            blank: self.blank,
            buffer_len: v.buffer_len,
            dof_on: self.camera.dof,
            crop: self.settings.crop,
            crop_x: self.settings.crop_left,
            crop_y: self.settings.crop_bottom,
            crop_width: self.settings.crop_width,
            resume: self.settings.resume,
            field: self.settings.field,
            field_no: self.settings.field_no,
            scan_lines,
            columns: vec![None; columns],
            slices,
            line: vec![Rgbazo::default(); 2 * columns],
            ..Sampler::default()
        }
    }

    /// `si_lights()` on this state ([`crate::lights::lights`]): for an [`UpdateEnv::lights`]
    /// that has the depth-map pass to offer. The ray tracing word (0x08790c7c) is one word
    /// in the original; its bit 0 is mirrored into the two places the geometry stage and
    /// the shading model read it from.
    pub fn lights(&mut self, env: &mut dyn crate::lights::LightsEnv) -> Result<Vec<&'static str>, &'static str> {
        let r = crate::lights::lights(&mut self.st, &mut self.options.raytrace, self.settings.level, env);
        self.g.raytrace = self.options.raytrace & 1 != 0;
        self.st.glb.raytrace = self.g.raytrace;
        r
    }

    /// `SI_materials()` on this state ([`crate::materials::materials`]): for an
    /// [`UpdateEnv::materials`]. `materials` and `models` are the two chapters of the
    /// dictionary (a model element's record is its index in [`ShadeState::models`], that is
    /// its element number); `node_of` gives the node the dictionary holds for an element
    /// number. The ray tracing word (0x08790c7c), the frame (0x08790dda) and each model's
    /// flag word (+0x214) are one place in the original and two or three here: they are
    /// carried over before and after.
    pub fn materials<E: crate::materials::MaterialsEnv>(
        &mut self,
        materials: &[crate::materials::DictElem],
        models: &[crate::materials::DictElem],
        node_of: &dyn Fn(usize) -> Option<usize>,
        env: &mut E,
    ) -> Result<(), crate::materials::SetupError> {
        self.st.glb.raytrace = self.options.raytrace & 1 != 0;
        self.st.glb.frame = self.options.frame;
        self.st.glb.textures_2d = self.options.textures_2d;
        let r = crate::materials::materials(&mut self.st, materials, models, env);
        if self.st.glb.raytrace {
            self.options.raytrace |= 1;
        }
        self.g.raytrace = self.options.raytrace & 1 != 0;
        for e in models {
            if let (0, Some(elem)) = (e.unused, e.data) {
                if let Some(node) = node_of(elem) {
                    self.g.models[node].flags = self.st.models[elem].flags;
                }
            }
        }
        r
    }

    /// `updateTxt3DTransf(root)` on this state ([`update_txt3d_transf`]): for an
    /// [`UpdateEnv::update_txt3d_transf`]. `own_tex3d` gives a node's own 3D texture (+0x224).
    pub fn update_txt3d_transf(&mut self, own_tex3d: &dyn Fn(usize) -> u32) -> usize {
        update_txt3d_transf(&mut self.st, &self.g.models, self.root, own_tex3d)
    }

    /// `si_initFade()` on this state ([`crate::camera::init_fade`]); `lines` are the four
    /// byte lines at 0x08790b40. Returns the background bytes (0x08790d14).
    pub fn init_fade(&mut self, lines: [&mut [u8]; 4]) -> [u8; 4] {
        crate::camera::init_fade(&mut self.st.glb, &mut self.fade, self.blank, self.settings.width, lines)
    }

    /// `SI_stats` as seventeen words.
    pub fn stats_words(&self) -> [i32; 17] {
        let mut w = self.stats;
        w[0] = self.g.bins.accepted;
        w[5] = self.g.clipper.made_vertices;
        w[6] = self.g.clipper.made_pairs;
        w[7] = self.g.made_tns;
        w[8] = self.g.bins.made;
        w[9] = self.g.trace.made;
        w[10] = self.tree_counts[0];
        w[11] = self.tree_counts[1];
        // +0x34: the pixels of the texture blocks (SI_readTextureFile, global2dMapCrop)
        w[13] = self.st.texture_pixels;
        w
    }
}

/// What `initData` and `SI_update` call that is not ported with them. Each method names
/// the original it stands for; the defaults do nothing.
pub trait UpdateEnv: GeometryEnv {
    /// `fopen("<name>.stats", "w")` in `initData`: false when the file cannot be opened.
    fn stats_open(&mut self, _name: &[u8]) -> bool {
        true
    }
    /// `fprintf` to the statistics file (and the `fflush` after the last line).
    fn stats_write(&mut self, _text: &str) {}
    /// `SI_statusDisplayMessage(text, 2)`.
    fn status_message(&mut self, _text: &str) {}
    /// `SI_defaultLight` (0x08781e00, 0x240 bytes): the light `initData` copies into chunk
    /// memory when the scene has none.
    fn default_light(&mut self) -> crate::shade::ShadeLight {
        crate::shade::ShadeLight::default()
    }
    /// `SI_sceneUpdateItems(time, 1, &SI_database, 1)` with `SI_frameCurrent` set to the
    /// frame for the call: the scene at that time (sequence mode only).
    fn scene_update_items(&mut self, _p: &mut Prepared, _time: f32) {}
    /// `SI_edGetElementsGrafNodeInfo(&SI_elemDicts[SI_procData[0]])`.
    fn graf_node_info(&mut self, _p: &mut Prepared) {}
    /// `si_initFade()` and `si_initDof()` (`camera.c`).
    fn init_fade(&mut self, _p: &mut Prepared) {}
    fn init_dof(&mut self, _p: &mut Prepared) {}
    /// `SI_materials()`, `SI_getMapsList()` (whose result goes to 0x087907e0) and
    /// `SI_textures(1)` (`materials.c`).
    fn materials(&mut self, _p: &mut Prepared) {}
    fn maps_list(&mut self, _p: &mut Prepared) {}
    fn textures(&mut self, _p: &mut Prepared) {}
    /// `si_lights()` (`lights.c`).
    fn lights(&mut self, _p: &mut Prepared) {}
    /// `si_traverseExpandedInstance(root)` and `si_blurGeometry(time)` (`geometry.c`).
    fn traverse_expanded_instance(&mut self, _p: &mut Prepared) {}
    fn blur_geometry(&mut self, _p: &mut Prepared, _time: f32) {}
    /// `updateTxt3DTransf(root)`.
    fn update_txt3d_transf(&mut self, _p: &mut Prepared) {}
    /// `si_treestatus()` (`renutil.c`).
    fn tree_status(&mut self, _p: &mut Prepared) {}
}

/// The scene at one time [soft 0x0826edf0 SI_update]: `SI_update(time)`, once per frame or
/// field. In this order:
///
/// 1. The counter of triangles filed (`SI_stats` +0x00) and of list entries (+0x24) are
///    cleared, the picture-plane box and the scene box are set empty, the trace list and
///    the tree's root are cleared.
/// 2. In sequence mode the scene is brought to `time`. Then the dictionary's node
///    information, `si_initViewport`, `si_initFade`, `si_initDof`, `SI_camera`,
///    `SI_materials`, `SI_getMapsList`, with 2D textures `SI_textures(1)`, `si_lights`.
/// 3. The scan-line table is cleared from entry 0 up to the window's last line (nothing
///    when that is below 0 or not a number; the index is a 16-bit counter compared as a
///    float).
/// 4. The geometry: expanded instances, then `si_blurGeometry(time)` in sequence mode with
///    motion blur, else `si_traverse(root)`.
/// 5. `updateTxt3DTransf(root)`, `si_setBounds`.
/// 6. With ray tracing on and at least one triangle filed: `si_initAncestry`, then
///    `si_bsptree(trace list, its length, scene box, ancestry, 2)` gives the root; with
///    tree statistics on, on the first frame, `si_treestatus`.
/// 7. With memory statistics on, on the first frame: two counters are brought up to date
///    (depth of field rows: region x max × blur lines × 24 bytes; scan-line samples: (x
///    max + 1) × sub² for the adaptive sampler, 2 × the row length above level 1) and the
///    report is written to the statistics file ([`stats_report`]).
///
/// A fatal error of `SI_camera` ends the routine (in the original, the render).
pub fn update(p: &mut Prepared, env: &mut dyn UpdateEnv, time: f32) -> Result<(), &'static str> {
    p.g.bins.accepted = 0;
    p.g.trace.made = 0;
    p.g.bins.bounds = p.empty_bounds;
    p.g.trace.bounds = p.bsp_params.empty_box;
    p.g.trace.items.clear();
    p.bsp = Bsp::default();
    if p.options.sequence == 2 {
        env.scene_update_items(p, time);
    }
    env.graf_node_info(p);
    init_viewport(&p.camera, &p.settings, &mut p.view);
    p.g.window = si_math::geometry::ClipBox { xmin: p.view.window[0], xmax: p.view.window[1], ymin: p.view.window[2], ymax: p.view.window[3], yon: p.view.minus_near, hither: p.view.minus_far };
    p.st.camera.near_z = p.view.minus_near;
    env.init_fade(p);
    env.init_dof(p);
    // Rust differs here: the original's scan-line table keeps the length `initBuffers` gave
    // it (window y max + 100, before `si_initDof` grows the window by its blur lines). With
    // more blur lines than that (mode 1 makes their number grow with the picture's width:
    // over 100 from about 2000 pixels wide) the clearing below, the filing of the scan-line
    // records and `SI_sample`'s reads of the table go past its end into the column table
    // and the blocks after it, which the original corrupts. Here the table is lengthened to
    // cover the grown window, by the same rule (+100) the column table gets in
    // [`Prepared::sampler`]. When the window's last line is inside the table (every picture
    // the original renders without overrunning) the table is left exactly as it was.
    let last_line = x(p.view.window[3]).to_i32(Round::Zero);
    if usize::try_from(last_line).is_ok_and(|l| l >= p.g.bins.lines.len()) {
        let len = (x(p.view.window[3]) + 100.0f32).to_i32(Round::Zero).max(0) as usize;
        p.g.bins.lines.resize(len, Vec::new());
    }
    camera(&p.camera, &p.settings, &mut p.view, &mut p.st.camera)?;
    env.materials(p);
    env.maps_list(p);
    if p.options.textures_2d != 0 {
        env.textures(p);
    }
    env.lights(p);
    let last = x(p.view.window[3]);
    if x(0.0f32).le(last) {
        let mut i: i32 = 0;
        loop {
            // (an index beyond the table is a wild store in the original)
            if let Some(line) = usize::try_from(i as i16).ok().and_then(|k| p.g.bins.lines.get_mut(k)) {
                line.clear();
            }
            i += 1;
            if !x(i as i16 as i32).le(last) {
                break;
            }
        }
    }
    env.traverse_expanded_instance(p);
    if p.options.sequence == 2 && p.options.blur != 0 {
        env.blur_geometry(p, time);
    } else {
        let root = p.root;
        traverse(&mut p.st, &mut p.g, env, root);
    }
    env.update_txt3d_transf(p);
    p.view.bounds = p.g.bins.bounds;
    set_bounds(p.camera.dof, &p.settings, &mut p.view);
    p.g.bins.bounds = p.view.bounds;
    let first = p.options.frame == p.options.first_frame;
    if p.options.raytrace & 1 != 0 && p.g.bins.accepted != 0 {
        let mut counts = BspStats { links: p.g.trace.made, nodes: p.tree_counts[0], leaves: p.tree_counts[1] };
        let scene = p.g.trace.bounds;
        let anc = init_ancestry(&mut p.bsp, &scene, &mut counts);
        // the trace list: every record was put at the head
        let n = p.g.trace.items.len();
        let base = p.bsp.links.len();
        for (i, item) in p.g.trace.items.iter().enumerate() {
            p.bsp.links.push(Link { bound: item.bounds, tn: item.tn, next: if i == 0 { None } else { Some(base + i - 1) } });
        }
        let head = if n == 0 { None } else { Some(base + n - 1) };
        let count = counts.links;
        let root = BspBuild { bsp: &mut p.bsp, tns: &p.st.tns, verts: &p.st.verts, params: p.bsp_params, stats: &mut counts }.bsptree(head, count, &scene, &anc, 2);
        p.bsp.root = root;
        p.g.trace.made = counts.links;
        p.tree_counts = [counts.nodes, counts.leaves];
        if p.options.tree_stats != 0 && first {
            env.tree_status(p);
        }
    }
    if p.options.mem_stats != 0 && first {
        let x_max = p.view.region[1];
        p.stats[stat::DOF] = p.stats[stat::DOF].wrapping_add(x_max.wrapping_mul(p.view.dof_lines).wrapping_mul(24));
        if p.options.adaptive == 1 {
            let sub = p.options.sub as i32;
            p.stats[stat::SCANLINE] = p.stats[stat::SCANLINE].wrapping_add(x_max.wrapping_add(1).wrapping_mul(sub).wrapping_mul(sub));
        } else if p.settings.level > 1 {
            p.stats[stat::SCANLINE] = p.stats[stat::SCANLINE].wrapping_add(p.view.buffer_len.wrapping_mul(2));
        }
        let text = stats_report(&p.stats_words());
        env.stats_write(&text);
    }
    Ok(())
}

/// The memory report `SI_update` writes to the statistics file, from the counters of
/// `SI_stats`: each line the count and the bytes it stands for (a vertex 76, a triangle
/// 96, a scan-line sample 24, a bin 4, a texture vertex 8, an edge pair 40, a triangle
/// link 32, a leaf 36, an inner node 20), and their total. All sums are 32-bit.
pub fn stats_report(s: &[i32; 17]) -> String {
    let m = |v: i32, k: i32| v.wrapping_mul(k);
    let total = [m(s[5], 76), m(s[7], 96), m(s[13], 5), m(s[2], 24), s[4], s[3], m(s[15], 4), s[14], m(s[1], 4), m(s[6], 8), m(s[8], 40), m(s[9], 32), m(s[11], 36), m(s[10], 20)]
        .iter()
        .fold(0i32, |a, b| a.wrapping_add(*b));
    let mut t = String::from(si_rt::text(0x0869_ca00));
    t += &format!("    {:>10} vertices:         {:>10} bytes\n", s[5], m(s[5], 76));
    t += &format!("    {:>10} triangles:        {:>10} bytes\n", s[7], m(s[7], 96));
    t += &format!("               texture maps:     {:>10} bytes\n", m(s[13], 5));
    t += si_rt::text(0x0869_cae0);
    t += si_rt::text(0x0869_cb00);
    t += &format!("               scanline:           {:>10} bytes\n", m(s[2], 24));
    t += &format!("               output:             {:>10} bytes\n", s[4]);
    t += &format!("               frame buffer null:  {:>10} bytes\n", s[14]);
    t += &format!("               shadow depth buffer:{:>10} bytes\n", m(s[15], 4));
    t += &format!("               depth of field:     {:>10} bytes\n", s[3]);
    t += si_rt::text(0x0869_cc4f);
    t += &format!("               bins:             {:>10} bytes\n", m(s[1], 4));
    t += &format!("    {:>10} texture vertices: {:>10} bytes\n", s[6], m(s[6], 8));
    t += &format!("    {:>10} edgepairs:        {:>10} bytes\n", s[8], m(s[8], 40));
    t += si_rt::text(0x0869_cd07);
    t += &format!("    {:>10} triangle links:   {:>10} bytes\n", s[9], m(s[9], 32));
    t += &format!("    {:>10} bsp leaves:       {:>10} bytes\n", s[11], m(s[11], 36));
    t += &format!("    {:>10} internal nodes:   {:>10} bytes\n", s[10], m(s[10], 20));
    t += &format!("\nTotal                            {:>10} bytes\n\n", total);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_of_a_whole_picture() {
        let cam = Camera { near: 0.5, far: 100.0, ..Camera::default() };
        let s = Settings { width: 320, height: 240, level: 2, ..Settings::default() };
        let mut v = View::default();
        init_viewport(&cam, &s, &mut v);
        assert_eq!(v.picture, [0.0, 319.0, 0.0, 239.0]);
        // 2L + (width − 1) × L − 2
        assert_eq!(v.window, [0.0, 640.0, 0.0, 480.0]);
        assert_eq!((v.minus_near, v.minus_far), (-0.5, -100.0));
        let mut stats = [0; 3];
        let b = init_buffers(&v, s.width, &mut stats);
        assert_eq!(b, BufferSizes { scan_lines: 580, columns: 740, line: 330 });
        assert_eq!(stats, [1320, 1480, 1320]);
    }

    #[test]
    fn a_field_has_half_the_lines() {
        let s = Settings { width: 10, height: 9, level: 1, field: 1, ..Settings::default() };
        let mut v = View::default();
        init_viewport(&Camera::default(), &s, &mut v);
        assert_eq!(v.picture[3], 3.0);
        assert_eq!(v.field_half, 0.5);
    }

    #[test]
    fn bounds_become_the_region() {
        let s = Settings { width: 100, height: 80, level: 2, ..Settings::default() };
        let mut v = View::default();
        init_viewport(&Camera::default(), &s, &mut v);
        init_filter(2, &mut v);
        assert_eq!(v.filter_size, 3);
        v.bounds = [10.0, 61.0, -7.0, 300.0];
        set_bounds(0, &s, &mut v);
        assert_eq!(v.bounds, [4.0, 31.5, -4.5, 151.0]);
        assert_eq!(v.region_px, [4, 31, 0, 79]);
        assert_eq!(v.region, [8, 64, 0, 160]);
        assert_eq!(v.buffer_len, 41);
    }
}

/// The 3D textures' matrices for this frame [soft 0x0826f5ec updateTxt3DTransf]:
/// `updateTxt3DTransf(node)`, called by `SI_update` with the root after the geometry stage.
///
/// For the node and each of its brothers (+0x2c0), then recursively for its sons (+0x2c4):
/// for every material of the node (+0x206, +0x20c) that has a 3D texture (+0x6fc), and for
/// the node's own 3D texture (+0x224): the texture's `SI_Txt3d` gets, through
/// `SI_ttSetTform`, the matrix (the renderer's inverse view matrix, 0x087907ec, as it was
/// when the routine was entered) × inverse(the global matrix of the node's `latdep` node,
/// +0x44 → +0x1c4) × the texture's own matrix (+0xb4): a point in the renderer's space goes
/// to the scene's, to the node's, to the texture's.
///
/// When the node's matrix has no inverse `SI_matrixInverse` leaves its result, a local of
/// this routine, as it was: **the inverse of the last node of the same depth that had one**
/// (brothers share the invocation; the sons of two brothers get the same stack frame), and
/// before any, whatever the stack held (the Rust leaves the texture's matrix alone then).
/// `own_tex3d` gives a node's own 3D texture (index + 1 into [`ShadeState::tex3ds`]; 0:
/// none). Returns how many times a texture was left alone for want of any inverse (the
/// original computes with stack words there). Class: model.
pub fn update_txt3d_transf(st: &mut ShadeState, models: &[RenModel], root: Option<usize>, own_tex3d: &dyn Fn(usize) -> u32) -> usize {
    let view = st.camera.world_to_raw_mat;
    // the local that holds the inverse, one per depth of the recursion
    let mut kept: Vec<Option<Matrix>> = Vec::new();
    let mut unset = 0usize;
    fn walk(st: &mut ShadeState, models: &[RenModel], view: &Matrix, own_tex3d: &dyn Fn(usize) -> u32, mut node: Option<usize>, depth: usize, kept: &mut Vec<Option<Matrix>>, unset: &mut usize) {
        if kept.len() <= depth {
            kept.push(None);
        }
        while let Some(n) = node {
            let m = &models[n];
            let set = |st: &mut ShadeState, tex: u32, kept: &mut Vec<Option<Matrix>>, unset: &mut usize| {
                if let Some(inverse) = si_math::algebra::matrix_inverse(&models[m.latdep].gmat) {
                    kept[depth] = Some(inverse);
                }
                let Some(inverse) = kept[depth] else {
                    *unset += 1;
                    return;
                };
                let to_node = si_math::algebra::matrix_multiply(view, &inverse);
                let t = &mut st.tex3ds[tex as usize - 1];
                let whole = si_math::algebra::matrix_multiply(&to_node, &t.matrix);
                if let Some(txt) = t.txt.as_mut() {
                    crate::tt::set_tform(txt, &whole);
                }
            };
            for &mat in &m.materials {
                let tex = st.materials[mat].tex3d;
                if tex != 0 {
                    set(st, tex, kept, unset);
                }
            }
            let own = own_tex3d(n);
            if own != 0 {
                set(st, own, kept, unset);
            }
            walk(st, models, view, own_tex3d, m.son, depth + 1, kept, unset);
            node = m.brother;
        }
    }
    walk(st, models, &view, own_tex3d, root, 0, &mut kept, &mut unset);
    unset
}
