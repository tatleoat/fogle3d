// SPDX-License-Identifier: MIT
//! The shading model: the original's `shade.c` (finding 0012, "The shading model").
//!
//! Five functions: `SI_shade` (what a ray that has hit a triangle is worth), `reflect` and
//! `refract` (the two secondary rays it spawns, each of which calls `SI_shade` again),
//! `layerFog` and `si_setMaterialFromVertexColor`. All five are ported here.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`. The
//! arithmetic repeats the x87 instruction sequence: the same operations in the same order on
//! [`F80`] register values, rounded where the original stores. `SI_shade` contains the code
//! that applies one 2D texture map twice (once for the material's maps, once for the model's)
//! and the loop over the lights three times (Lambert, Phong, Blinn); the two map copies are
//! instruction for instruction the same, the three light loops are not: where they differ the
//! difference is reproduced and named.
//!
//! What `SI_shade` calls outside this source file comes in through [`ShadeEnv`]: the BSP cell
//! search and the two ray tracers (`trace.c`), the texture look-up `si_BlookUp` (`renutil.c`)
//! and the two functions that apply a texture sample to a material (`t2d_ren.c`,
//! `t3d_ren.c`); the three texture methods default to the ports. The scene comes in as [`ShadeState`]: plain vectors where the original has
//! pointers into the model dictionary, the light list and chunk memory.

use crate::types::{RVertex, Ray, Rgbazo, Tn, Vector};
use crate::pool::Pool;
use std::sync::LazyLock;
use si_math::algebra::area_triangle_3d;
use si_math::matrix::{camera_to_light, RenderCamera};
use si_math::types::Matrix;
use si_x87::{fx, trig, x, Round, F80};

// ---------------------------------------------------------------------------------- constants

/// The tolerance of nearly every test in the file: the **double** 0.0001 (0x08654318 in
/// `SI_shade`, 0x08654398 in `reflect`, 0x086543b8 in `layerFog`).
const EPS: f64 = 1e-4;
/// 0x08654320, float: the number of steps of a decay table per unit of cosine.
const TABLE_SCALE: f32 = 200.0;
/// 0x08654328, double: at or above this opacity nothing behind the surface is looked at.
const OPAQUE: f64 = 0.999;
/// 0x08654338, double: a specular factor below this adds nothing.
const SPEC_MIN: f64 = 1e-6;
/// 0x08654330, float: the distance given to a light that has only a direction.
const FAR_LIGHT: f32 = 100000.0;
/// 0x08654310, float: "no value yet" in the sixth member of a sample.
const NO_O: f32 = -100000.0;
/// 0x0869c680 `poisson`: forty doubles. Despite the name they are the cumulative normal
/// distribution Φ(z) for z = 0, 0.1, …, 3.9 to four decimals; `layerFog` uses them as the
/// fog's density profile across the layer.
/// They are read from the user's program at run time ([`crate::data::POISSON_TABLE`], decoded once).
pub static POISSON: LazyLock<[f64; 40]> = LazyLock::new(|| {
    let b = crate::data::POISSON_TABLE.get();
    core::array::from_fn(|i| f64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap()))
});

/// Flag bits of the word `SI_shade` forms from the material's word at +0x6e0 and the model's
/// at +0x214 (the same word `clip` puts in each triangle).
pub mod flag {
    /// Normals are interpolated between the corners (otherwise the first corner's is used).
    pub const SMOOTH: u16 = 0x0002;
    /// The material has textures to apply.
    pub const TEXTURED: u16 = 0x0004;
    /// Either bit: the surface reflects.
    pub const REFLECTS: u16 = 0x0030;
    /// Any of these: the surface may let the background through.
    pub const TRANSPARENT: u16 = 0x0340;
    /// With this bit a transparent surface sends a refracted ray even for a primary ray.
    pub const REFRACTS: u16 = 0x0200;
}

// ------------------------------------------------------------------------------------ records

/// The twenty floats of `__SI_Material` at +0x700: the values the shading model computes with.
/// The texture functions write them; `SI_shade` saves the block on entry and puts it back
/// before it returns, and puts the saved values back around every recursive call.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MatWork {
    /// +0x700: the ambient term as `SI_materials` prepared it; four floats.
    pub base_ambient: [f32; 4],
    /// +0x710: the ambient term in use (the starting colour of models 2 to 5).
    pub ambient: [f32; 4],
    /// +0x720: diffuse colour in use (the whole colour of model 1).
    pub diffuse: [f32; 4],
    /// +0x730: specular colour in use.
    pub specular: [f32; 4],
    /// +0x740: reflectivity in use.
    pub reflectivity: f32,
    /// +0x744: opacity in use: 1 − transparency.
    pub opacity: f32,
    /// +0x748: index of refraction in use.
    pub refraction: f32,
    /// +0x74c: the fraction of the shutter interval this copy of a moving object stands for
    /// (`setExistance`); multiplies the opacity.
    pub existence: f32,
}

/// `SI_DecayTable`: a float exponent and 200 pairs (value, step to the next value), 0x644
/// bytes; filled by `decayTable` with `pow(i / 200, exponent)`. Looked up as
/// `value[i] + frac × step[i]` with `i = trunc(c × 200)`.
#[derive(Clone, Debug, PartialEq)]
pub struct DecayTable {
    /// +0x00
    pub exponent: f32,
    /// +0x04: (value, step) for i = 0..200.
    pub pairs: Vec<[f32; 2]>,
    /// The two floats that follow the table in the material. A cosine of exactly 1 gives
    /// index 200, one past the end, and the original reads these: for the specular table
    /// they are the material's floats at +0x6a8 (its reflectivity) and +0x6ac; for the
    /// static blur table those at +0xda4 and +0xda8.
    pub after: [f32; 2],
}

impl Default for DecayTable {
    fn default() -> DecayTable {
        DecayTable { exponent: 0.0, pairs: vec![[0.0; 2]; 200], after: [0.0; 2] }
    }
}

impl DecayTable {
    /// Entry `i` as the original addresses it (table + 4 + 8 i), including the entry one past
    /// the end. Any other index would be a read outside the material record; the Rust
    /// panics there (only a NaN cosine or one above 1.005 can produce it).
    fn entry(&self, i: i16) -> [f32; 2] {
        match i {
            0..=199 => self.pairs[i as usize],
            200 => self.after,
            _ => panic!("decay table index {i} outside the material record"),
        }
    }

    /// `value[i] + (c × 200 − i) × step[i]`, unrounded, with `i = (short) trunc(c × 200)`:
    /// the look-up as every use in `SI_shade` codes it.
    pub(crate) fn lookup(&self, c: F80) -> F80 {
        let t = c * TABLE_SCALE;
        let i = t.to_i32(Round::Zero) as i16;
        let [value, step] = self.entry(i);
        (t - x(i as i32)) * step + value
    }
}

/// The members of `__SI_Material` (0xdcc bytes) the shading model reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeMaterial {
    /// +0x1e: shading model: 1 constant, 2 (not in the SDK's list), 3 Lambert, 4 Phong,
    /// 5 Blinn; 6 (shadow object) and 7 (vertex colour) fall through the switch.
    pub model: i16,
    /// +0x44: diffuse colour, four floats.
    pub diffuse: [f32; 4],
    /// +0x54: specular colour, four floats.
    pub specular: [f32; 4],
    /// +0x64: the specular decay table (exponent at +0x64, pairs to +0x6a8).
    pub spec_table: DecayTable,
    /// +0x6a8
    pub reflectivity: f32,
    /// +0x6b8
    pub transparency: f32,
    /// +0x6dc
    pub refraction: f32,
    /// +0x6e0: flag word (low half), see [`flag`].
    pub flags: u16,
    /// +0x6ec: head of the material's list of 2D texture maps (index into
    /// [`ShadeState::maps`]).
    pub maps: Option<usize>,
    /// +0x6f4: the reflection map, or none.
    pub reflect_map: Option<usize>,
    /// +0x6fc: the 3D texture, 0 for none (opaque here: only tested and handed on).
    pub tex3d: u32,
    /// +0x700
    pub work: MatWork,
    /// +0x6e8: the number of 2D texture maps of the material (`clip` adds the node's own
    /// count to get the pairs of texture coordinates a vertex carries).
    pub n_maps: i16,
    /// +0x758: static blur flag.
    pub static_blur: i16,
    /// +0x75c: static blur width: how far `transForm` pushes the vertices of a node whose
    /// first material has the flag along their normals.
    pub blur_width: f32,
    /// +0x760: the static blur decay table.
    pub blur_table: DecayTable,
    /// +0xdc8: what vertex colours do to the diffuse colour: 1 nothing, 3 add, else replace.
    pub vcol_rgb: i16,
    /// +0xdca: what vertex alpha does to the opacity: 1 nothing, 3 multiply, else replace.
    pub vcol_alpha: i16,
    /// +0x20: 1: the surface is faceted (`SI_materials` sets flag 2, "interpolate normals",
    /// for every other value). No file token sets it; the allocation routine leaves 0.
    pub w20: i16,
    /// +0x34: ambient colour, four floats (the fourth is never read by the renderer).
    pub ambient: [f32; 4],
    /// +0x6ac: an `SI_Jitter` (amplitude, table, counter) `SI_materials` fills with 201 rows
    /// when the amplitude is above 0.1. The amplitude comes from the old material token 10
    /// only (finding 0004), which no sample file has: its consumer was not found.
    pub jitter_6ac: crate::jitter::Jitter,
    /// +0x6bc: the same with 223 rows; amplitude from the old token 8.
    pub jitter_6bc: crate::jitter::Jitter,
}

/// The members of `__SI_LclMap` (0x1d8 bytes) that `SI_shade` and `reflect` read: one 2D
/// texture as applied to one material or model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeMap {
    /// +0x2a: mapping method: 1, 2, 3 the three planar projections, 4 the model's own UV
    /// coordinates, 5 and 6 (cylindrical and spherical by the vendor's shader source), 7 on a
    /// reflection map: ray-traced reflection first.
    pub method: i16,
    /// +0x2c: handed to `si_BlookUp`.
    pub w2c: i16,
    /// +0x38, +0x3c: least u and v of the mapped region; +0x40, +0x44: greatest.
    pub umin: f32,
    pub vmin: f32,
    pub umax: f32,
    pub vmax: f32,
    /// +0x48, +0x4c, +0x50, +0x54: a second pair of limits (u, v, u, v) used when the region
    /// wraps (`wrap_u`, `wrap_v`) or is given as a band (`band_u`, `band_v`).
    pub u2min: f32,
    pub v2min: f32,
    pub u2max: f32,
    pub v2max: f32,
    /// +0x102, +0x104
    pub band_u: i16,
    pub band_v: i16,
    /// +0x106, +0x108
    pub wrap_u: i16,
    pub wrap_v: i16,
    /// +0x110: 1: the bump height is the sample's fourth value (alpha), otherwise its fifth
    /// (intensity).
    pub bump_alpha: i16,
    /// +0x120: non-zero: the map perturbs the normal.
    pub bump: i16,
    /// +0x124: non-zero: `SI_shade` skips the map (its coordinate pair is still counted).
    pub skip: i16,
    /// +0x160: bump amplitude.
    pub bump_amount: f32,
    /// +0x164: reflection map: factor on the sampled colour.
    pub env_scale: f32,
    /// +0x168: reflection map: rotation added to the azimuth, radians.
    pub env_rotation: f32,
    /// +0x16c: next map of the list.
    pub next: Option<usize>,
    /// +0x1d4: with method 4: non-zero takes u and v modulo 1 by `floor` and skips the region
    /// test.
    pub repeat: i16,
    /// +0x7c: three floats `SI_textureVerts` multiplies the translation by, component by
    /// component (their setter was not read; the projection's size, by their use: a guess).
    pub v7c: [f32; 3],
    /// +0xa0: the texture's translation (token of the texture file, finding 0004).
    pub translation: [f32; 3],
    /// +0xb8: a matrix `SI_txtProjectionInfo` stores (the inverse of the rotation built
    /// from the angles at +0x94); `SI_textureVerts` and `SI_mapTcoordCompute` use its first
    /// three rows and columns.
    pub tmat: [[f32; 4]; 4],
    /// +0x190: the texture's identifier in the node's texture coordinate sets
    /// (`SI_meshGetTxtUVDomain`).
    pub id: i32,
    /// +0x14: the picture's path as the texture file gives it (token 1), without `.pic`.
    pub path: Vec<u8>,
    /// +0x20: the picture's name for the frame being rendered (`SI_setFileName`); what the
    /// picture list is keyed by.
    pub file: Option<Vec<u8>>,
    /// +0x24: the loaded picture (`glb_map *`): an index into [`ShadeState::pictures`].
    pub glb: Option<usize>,
    /// +0x28: 0 one picture, 1 a numbered sequence, 2 a script of names.
    pub anim: i16,
    /// +0x2e: non-zero: only a rectangle of the picture is used; +0x30, +0x32: its first and
    /// last column; +0x34, +0x36: its first and last row.
    pub crop: i16,
    pub crop_x0: i16,
    pub crop_x1: i16,
    pub crop_y0: i16,
    pub crop_y1: i16,
    /// +0x58, +0x5c: the picture's extent in u and v (negative: mirrored).
    pub uscale: f32,
    pub vscale: f32,
    /// +0x60, +0x64: pixels per unit of u and of v (`SI_processTextures`).
    pub pix_u: f32,
    pub pix_v: f32,
    /// +0x68, +0x6c: where the picture starts in u and v.
    pub uoff: f32,
    pub voff: f32,
    /// +0x70, +0x74: the fractions of the two (`SI_processTextures`).
    pub uoff_frac: f32,
    pub voff_frac: f32,
    /// +0x78, +0x7a: the extent in u, in v is negative (`SI_processTextures`).
    pub neg_u: i16,
    pub neg_v: i16,
    /// +0x88: the centre of the projection's box (`SI_txtProjectionInfo`).
    pub centre: [f32; 3],
    /// +0x94: the projection's rotation, radians; +0xac: its scaling.
    pub rotation: [f32; 3],
    pub scaling: [f32; 3],
    /// +0xf8, +0xfa: repetitions of the picture in u and v.
    pub urepeat: i16,
    pub vrepeat: i16,
    /// +0xfc: u and v change places in the picture.
    pub swap_uv: i16,
    /// +0xfe, +0x100: every second repetition is mirrored, in u, in v.
    pub alt_u: i16,
    pub alt_v: i16,
    /// +0x10a: what scales the blend: 1 the sample's alpha, 2 its intensity, 4 "multiply the
    /// diffuse colour", anything else nothing.
    pub mask: i16,
    /// +0x10c: the blend factor.
    pub blend: f32,
    /// +0x112, +0x114, +0x116, +0x118, +0x11a: the map drives the ambient colour, the
    /// diffuse colour, the specular colour, the reflectivity, the transparency
    /// (`SI_updTxt2dEffects`); +0x11e: a word `SI_materials` reads as "drives refraction"
    /// and nothing writes.
    pub fx_ambient: i16,
    pub fx_diffuse: i16,
    pub fx_specular: i16,
    pub fx_reflect: i16,
    pub fx_transp: i16,
    pub fx_11e: i16,
    /// +0x128 … +0x144: the eight factors of the texture file: ambient, diffuse, specular,
    /// reflectivity, transparency, one no token of this build writes, roughness,
    /// reflection map.
    pub factors: [f32; 8],
    /// +0x148 … +0x15c: the first six as `SI_materials` prepares them (times 0.003922, the
    /// ambient one also times the mean scene ambience); the last two are
    /// [`ShadeMap::bump_amount`] (+0x160) and [`ShadeMap::env_scale`] (+0x164).
    pub work: [f32; 6],
    /// +0x18e: 1: "black and white" key: a sample at or below 0.001 or at or above 254.999 in
    /// all three colours makes the surface fully transparent.
    pub black_white: i16,
    /// +0x1a4 … +0x1b8: the projection box kept from the last `SI_txtProjectionInfo` (x min,
    /// y min, z min, x max, y max, z max); +0x1bc: the node number it was made for; +0x1c0,
    /// +0x1c4: the two radii.
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub box_node: i32,
    pub box_r5: f32,
    pub box_r6: f32,
}

/// The members of `__SI_Model` the shading model reads, for the model whose element index a
/// triangle carries at +0x58.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeModel {
    /// +0x214: flag word OR-ed with the material's.
    pub flags: u16,
    /// +0x21c: head of the model's own list of 2D texture maps.
    pub maps: Option<usize>,
    /// +0x224: the model's own 3D texture, 0 for none.
    pub tex3d: u32,
    /// +0x206, +0x20c: the node's materials (`None` is a null entry of the array).
    pub materials: Vec<Option<usize>>,
    /// The element's word at +0x48 in the MODELS chapter: non-zero: the element is not in
    /// use and `SI_materials` passes it over.
    pub unused: i16,
}

/// The members of `light` (0x240 bytes) the shading model reads. Positions and directions are
/// in the renderer's world space (camera space), as `si_lights` leaves them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeLight {
    /// +0x1e: type. 2 is a spot light; 3 and 4 have a direction only (infinite and sun);
    /// anything else is a point light.
    pub kind: i16,
    /// +0x24: colour, four floats.
    pub color: [f32; 4],
    /// +0x40: position.
    pub pos: Vector,
    /// +0x4c: unit direction towards the light, for the kinds without a position (and for
    /// every kind in shading model 2).
    pub dir: Vector,
    /// +0x64: unit direction a spot light points in.
    pub spot_dir: Vector,
    /// +0x78: cosine below which the cone starts to fade; +0x7c: cosine at which it is dark;
    /// +0x80: the divisor of the fade.
    pub cone_cos: f32,
    pub spread_cos: f32,
    pub cone_div: f32,
    /// +0x84, +0x88: distance at which the light starts to fall off and at which it is gone.
    pub falloff_start: f32,
    pub falloff_end: f32,
    /// +0x8c: shadows: 2 ray traced, 3 depth map, 4 soft depth map; anything else none.
    pub shadow: i16,
    /// +0x90: the factor on the light where it is in shadow (umbra).
    pub umbra: f32,
    /// +0x94, +0xd4: the two matrices of `si_cameraToLight`: depth, then x and y.
    pub depth_mat: Matrix,
    pub xy_mat: Matrix,
    /// +0x114: the depth map, one row per y.
    pub zmap: Vec<Vec<f32>>,
    /// +0x118: its resolution.
    pub map_size: i32,
    /// +0x120: half width of the soft shadow's window, in map cells.
    pub soft_size: i32,
    /// +0x124: step between the cells looked at.
    pub soft_step: i32,
    /// +0x12c: factor on the lit fraction of the window.
    pub soft_scale: f32,
    /// +0x13c bit 0: the light falls off with distance.
    pub falloff: bool,
    /// +0x140: next light of the list at 0x087907e4.
    pub next: Option<usize>,
    /// +0x15c: non-zero: the light lights only some models; +0x160: the value a model's
    /// entry in the table at +0x168 must have.
    pub selective: i32,
    pub select_id: i32,
    /// +0x168: one entry per model element index.
    pub select_table: Vec<i32>,
    /// +0x176: zero: the light is off.
    pub on: i16,
    /// What `si_lights` reads and the shading model does not: +0x34 the position and +0x58
    /// the interest as the scene gives them; +0x70 the cone angle and +0x74 the spread, in
    /// radians; +0x11c the soft shadow's window in map cells (halved into +0x120); +0x128 a
    /// float whose fourfold goes to +0x12c; +0x130 the jitter record.
    pub pos_given: Vector,
    pub interest: Vector,
    pub cone: f32,
    pub spread: f32,
    pub soft_full: i32,
    pub soft_w128: f32,
    pub jitter: crate::jitter::Jitter,
    /// Not a member of the original: the number (1, 2, ...) of the add-on's light array this light belongs to
    /// (`soft-arrays.txt` beside the database, [`crate::softarray`]), whose shadow rays are answered by
    /// [`ShadeEnv::s_array`]; 0 for every other light.
    pub array: u32,
}

/// The renderer globals `shade.c` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeGlobals {
    /// 0x087906ac: non-zero: depth of field; the sixth member of a sample is computed.
    pub dof: i32,
    /// 0x087906c0, 0x087906c8, 0x087906d0: three depths (z, negative in front of the eye) of
    /// the depth of field set-up, and 0x08790b2c, an integer factor.
    pub dof_z0: f32,
    pub dof_z1: f32,
    pub dof_z2: f32,
    pub dof_scale: i32,
    /// 0x08790c7c bit 0: ray tracing is on (the BSP tree exists).
    pub raytrace: bool,
    /// 0x08790ce0: non-zero: fade (depth cue). 0x08790ce8, 0x08790cf0: the distances between
    /// which it grows; 0x08790cf4: the factor per unit of distance.
    pub fade: i16,
    pub fade_start: f32,
    pub fade_end: f32,
    pub fade_scale: f32,
    /// 0x08790cf8: six floats. The colour a secondary ray that hits nothing comes back
    /// with; its first three are also the fade colour and its fourth (0x08790d04) the
    /// greatest fade.
    pub background: Rgbazo,
    /// 0x08790dc4: greatest depth of secondary rays.
    pub max_depth: i16,
    /// 0x08790e3c: non-zero: layer fog. 0x08790e40 density, 0x08790e44 base height,
    /// 0x08790e48 thickness, 0x08790e4c and 0x08790e50 the distances between which it acts,
    /// 0x08790e64 its colour.
    pub fog: i16,
    pub fog_density: f32,
    pub fog_base: f32,
    pub fog_thickness: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
    /// 0x08791164: textures are applied at all; 0x08791166: 2D ones; 0x08791168: 3D ones.
    pub textures: i16,
    pub textures_2d: i16,
    pub textures_3d: i16,
    /// 0x0879116a: reflection is computed; 0x0879116c: refraction; 0x0879116e: shadows.
    pub reflection: i16,
    pub refraction: i16,
    pub shadows: i16,
    /// 0x0869bc88 `si_renNoSpecAlpha`: non-zero: a highlight does not raise the alpha.
    pub no_spec_alpha: i16,
    /// 0x08790ddc: the scene's ambience, three floats: every material's ambient colour is
    /// multiplied by it, and its mean scales a texture's ambient factor.
    pub ambience: [f32; 3],
    /// 0x08790dec: non-zero: `SI_materials` gives every material that is not transparent
    /// through flag 0x100 the flag 0x4000.
    pub w_dec: i16,
    /// 0x08790dda: the frame being rendered (the short `SI_materials` hands to
    /// `SI_global2dMapInsert`).
    pub frame: i16,
    /// Not a global of the original: the value this port returns where the original
    /// returns a stack word it never set (the sixth result float of `SI_shade` without depth
    /// of field or for a secondary ray; alpha, depth and the sixth of `reflect`'s environment
    /// path). 0 by default. A check runs the port with two different values to find which
    /// results depend on such a word, as the emulation harness does with its two fill
    /// patterns.
    pub unset: f32,
}

/// Everything `shade.c` reads through pointers.
#[derive(Clone, Debug, Default)]
pub struct ShadeState {
    pub glb: ShadeGlobals,
    /// The four matrices at 0x087907ec (`reflect` uses the first for directions; `layerFog`
    /// its second column for heights).
    pub camera: RenderCamera,
    /// The triangle and vertex pools ([`Pool`]: shared by the band threads' copies until one
    /// writes; unit blurmem1).
    pub tns: Pool<Tn>,
    pub verts: Pool<RVertex>,
    /// The (u, v) pairs the vertices' pointers at +0x40 point into.
    pub uvs: Pool<[f32; 2]>,
    /// `Tn::material` is an index into this.
    pub materials: Vec<ShadeMaterial>,
    pub maps: Vec<ShadeMap>,
    /// `Tn::w58` is an index into this: the element index in the MODELS chapter.
    pub models: Vec<ShadeModel>,
    pub lights: Vec<ShadeLight>,
    /// 0x087907e4: head of the light list.
    pub light_head: Option<usize>,
    /// The `glb_map` records: one per picture in use ([`crate::materials::GlbMap`]).
    pub pictures: Pool<crate::materials::GlbMap>,
    /// The head of the picture list (`SI_getMapsList`; `SI_grafLists` +0x1c).
    pub picture_head: Option<usize>,
    /// The `__SI_Texture3D` records; a material's or model's `tex3d` is an index + 1.
    pub tex3ds: Vec<crate::t3d_ren::ShadeTex3d>,
    /// The 3D textures' table of random numbers (`tt_noise.c`).
    pub noise: crate::tt_noise::Noise,
    /// `SI_defaultMaterial` (0x0869dde0, 0xdcc bytes): the record a node without a material
    /// is rendered with, as an index into `materials`.
    pub default_material: Option<usize>,
    /// 0x08791214: the pixels of all texture blocks read with counting on (`SI_stats`
    /// +0x34 by its address).
    pub texture_pixels: i32,
    /// Not the original's: the first address the original would have read without it being
    /// mapped (a null pointer followed), where its process ends in a segmentation fault.
    /// The port goes on without the read; the frame's runner ends the frame on seeing it.
    pub fault: Option<u32>,
}

/// What `SI_shade`, `reflect` and `refract` call outside `shade.c`. Not ported with this
/// file; each method names the original it stands for.
pub trait ShadeEnv {
    /// `si_search(point, root)` with the root at 0x08790928 (the port is
    /// [`crate::trace::search`]): the BSP cell the point lies in, as the value stored in a
    /// ray's member at +0x38.
    fn search(&mut self, st: &ShadeState, p: Vector) -> u32;
    /// `SI_renderTxt3d(material, point, &normal)`: applies the material's 3D texture
    /// ([`ShadeMaterial::tex3d`]) at a point; may change the working block and, when a
    /// normal is given, the normal. The default is the port,
    /// [`crate::t3d_ren::render_txt3d`], with the library's `sinf` as [`si_x87::trig`] has
    /// it.
    fn render_txt3d(&mut self, st: &mut ShadeState, material: usize, p: Vector, normal: Option<&mut Vector>) {
        crate::t3d_ren::render_txt3d(st, material, p, normal, &mut crate::tt::TrigLib);
    }
    /// `si_BlookUp(u, v, map, map->w2c, &sample, bump, &du, &dv)`: the texture at (u, v) as
    /// five floats (red, green, blue, alpha, intensity) and, when `bump` is set, the two
    /// neighbouring samples the bump mapping differences. The default is the port,
    /// [`crate::renutil::blookup`].
    fn blookup(&mut self, st: &ShadeState, u: f32, v: f32, map: usize, bump: bool) -> [[f32; 5]; 3] {
        crate::renutil::blookup(st, u, v, map, st.maps[map].w2c, bump)
    }
    /// `SI_renderTxt2d(material, map, sample)`: blends a sample into the material's working
    /// block. The default is the port, [`crate::t2d_ren::render_txt2d`].
    fn render_txt2d(&mut self, st: &mut ShadeState, material: usize, map: usize, sample: [f32; 5]) {
        crate::t2d_ren::render_txt2d(st, material, map, sample);
    }
    /// `si_sTrace(&ray, tn, distance, light)`: what is left of a light's
    /// three colour components along a shadow ray (four floats, the fourth unused here).
    /// The port is [`crate::trace::s_trace`]; it marks the triangles it meets and may change
    /// the working blocks of the transparent materials it passes, hence the mutable state.
    fn s_trace(&mut self, st: &mut ShadeState, ray: &mut Ray, tn: usize, distance: f32, light: usize) -> [f32; 4];
    /// The shadow ray of a light of the add-on's light arrays (not the original's: [`crate::softarray`]); the
    /// default traces it as [`ShadeEnv::s_trace`] does.
    fn s_array(&mut self, st: &mut ShadeState, ray: &mut Ray, tn: usize, distance: f32, light: usize) -> [f32; 4] {
        self.s_trace(st, ray, tn, distance, light)
    }
    /// `si_trace(ray, tn)` (the port is [`crate::trace::trace`]): follows a secondary ray
    /// from triangle `tn`;
    /// true when it hits something, with the hit recorded in the ray.
    fn trace(&mut self, st: &ShadeState, ray: &mut Ray, tn: usize) -> bool;
    /// `SI_chunkMalloc(SI_renChunk, 0x48)`: `reflect` and `refract` take the secondary ray
    /// they trace from the renderer's chunk memory (`mem_chunk`); it is given back when the
    /// sampler that started the primary ray moves the fill pointer back. Here the ray is a
    /// local, so the default does nothing; an environment that keeps the arena's books
    /// allocates here. The original writes through the result without testing it.
    fn ray_alloc(&mut self) {}
}

// ------------------------------------------------------------------------------------ helpers

/// `if (v < 0) v = 0; else if (v > 1) v = 1;` as compiled: a NaN is left alone.
fn clamp01(v: f32) -> f32 {
    if x(0.0f32).gt(x(v)) {
        0.0
    } else if x(1.0f32).lt(x(v)) {
        1.0
    } else {
        v
    }
}

/// Divides three register values by `len` unless `len` compares equal to zero, and rounds
/// each to float (the normalisation every vector in the file ends with).
fn unit_or_rounded(c: [F80; 3], len: F80) -> Vector {
    if x(0.0f32).eq(len) {
        Vector::new(c[0].to_f32(), c[1].to_f32(), c[2].to_f32())
    } else {
        Vector::new((c[0] / len).to_f32(), (c[1] / len).to_f32(), (c[2] / len).to_f32())
    }
}

fn corners(st: &ShadeState, tn: &Tn) -> [RVertex; 3] {
    // a triangle that was hit has its three corners; `clip` voids one with a null corner
    tn.v.map(|i| st.verts[i.expect("a hit triangle has three corners")])
}

fn ray_dist(ray: &Ray) -> f32 {
    f32::from_bits(ray.w40)
}

// -------------------------------------------------------------- si_setMaterialFromVertexColor

/// Sets the diffuse colour and the opacity of a material's working block from the colours
/// stored in the hit triangle's corners [soft 0x0826904c si_setMaterialFromVertexColor].
///
/// The weights are ratios of `SI_areaTriangle3D` values in world space: w0 = area(P, v1, v2)
/// / area(v0, v1, v2) and w1 = area(v0, P, v2) / area(v0, v1, v2), both kept in the
/// register, and w2 = (1 − w0) − w1 rounded to float. A component is
/// `((c0 w0 + c1 w1) + c2 w2) / 255` of the corners' bytes at +0x44, +0x45, +0x46; mode 3
/// (`vcol_rgb`) adds the present diffuse value, mode 1 leaves the colour alone. The alpha
/// byte at +0x47 is summed in the order `(w1 c1 + w0 c0) + w2 c2`; mode 3 (`vcol_alpha`)
/// multiplies the present opacity by it. A degenerate triangle divides by zero and stores
/// what comes out. Class: model.
pub fn set_material_from_vertex_color(st: &mut ShadeState, ray: &Ray, p: Vector) {
    let tn = &st.tns[ray.tn.expect("the ray has hit a triangle")];
    let [v0, v1, v2] = corners(st, tn);
    let mat = tn.material as usize;
    let whole = area_triangle_3d(v0.world, v1.world, v2.world);
    let w0 = x(area_triangle_3d(p, v1.world, v2.world)) / whole;
    let w1 = x(area_triangle_3d(v0.world, p, v2.world)) / whole;
    let w2 = ((F80::ONE - w0) - w1).to_f32();
    let byte = |v: &RVertex, n: u32| x(((v.w44 >> (8 * n)) & 0xff) as i32);
    let m = &mut st.materials[mat];
    if m.vcol_rgb != 1 {
        for n in 0..3u32 {
            let mut c = ((byte(&v0, n) * w0 + byte(&v1, n) * w1) + byte(&v2, n) * w2) / 255.0f32;
            if m.vcol_rgb == 3 {
                c = c + m.work.diffuse[n as usize];
            }
            m.work.diffuse[n as usize] = c.to_f32();
        }
    }
    if m.vcol_alpha != 1 {
        let mut a = ((w1 * byte(&v1, 3) + w0 * byte(&v0, 3)) + x(w2) * byte(&v2, 3)) / 255.0f32;
        if m.vcol_alpha == 3 {
            a = a * m.work.opacity;
        }
        m.work.opacity = a.to_f32();
    }
}

// ----------------------------------------------------------------------------------- layerFog

/// Blends the layer fog into a colour [soft 0x08268c90 layerFog]: `layerFog(ray, point,
/// &colour)`, called by `SI_shade` when the ray has travelled further than the fog's start
/// distance.
///
/// Heights are along the second column of the matrix at 0x087907ec (world to the scene's
/// own space). With h0 the height of the hit point (rounded to double), s = −(direction ·
/// that column) and h1 = s × t + h0 the height where the ray started:
///
/// - nothing is done when both heights are above the layer's top (base + thickness) or
///   both below its base;
/// - the length in fog is min(t, end) − start, less the stretch before the ray enters the
///   layer and the stretch after it leaves, each (limit − h0) / s; a negative length does
///   nothing;
/// - each end's distance from the middle of the layer, in thirtieths of half the
///   thickness, truncated and held to 39, indexes [`POISSON`]. On opposite sides of the
///   middle (or when the start's index is 0) the mean is over 1 − Φ for the start's indices
///   0..=i1 and Φ for the hit's 0..=i0, divided by i0 + i1 + 2; on the same side it is the
///   mean of 1 − Φ between the two indices;
/// - amount = ((mean × density) × 2 × length) / 10, held to 0..1; colour = amount × fog
///   colour + (1 − amount) × colour.
///
/// Everything from the heights to the amount stays in the register. An index below zero
/// (a NaN height) reads outside the table in the original; here it panics. Class: model.
pub fn layer_fog(st: &ShadeState, ray: &Ray, p: Vector, color: &mut [f32; 3]) {
    let g = &st.glb;
    let m = &st.camera.world_to_raw_mat;
    let mut d = ray.dir;
    let len = ((x(d.y) * d.y + x(d.x) * d.x) + x(d.z) * d.z).sqrt().to_f32();
    if x(len).abs().gt(x(EPS)) {
        d = Vector::new(d.x / len, d.y / len, d.z / len);
    }
    let h0 = (((x(p.z) * m[2][1]) + ((x(p.y) * m[1][1]) + (x(p.x) * m[0][1]))) + m[3][1]).to_f64();
    let s = -((x(m[2][1]) * d.z) + ((x(m[1][1]) * d.y) + (x(m[0][1]) * d.x)));
    let base = x(g.fog_base);
    let top = x(g.fog_base) + g.fog_thickness;
    let h1 = s * ray.t + h0;
    if top.lt(x(h0)) && top.lt(h1) {
        return;
    }
    if x(h0).lt(base) && h1.lt(base) {
        return;
    }
    let reach = if x(ray.t).gt(x(g.fog_end as f64)) { x(g.fog_end as f64) } else { x(ray.t) };
    let mut length = reach - g.fog_start as f64;
    let mut hit_h = x(h0);
    if x(h0).lt(base) {
        length = length - (base - h0) / s;
        hit_h = base;
    } else if x(h0).gt(top) {
        length = length - (top - h0) / s;
        hit_h = top;
    }
    if h1.lt(base) {
        length = length - (x(ray.t) - (base - h0) / s);
    } else if h1.gt(top) {
        length = length - (x(ray.t) - (top - h0) / s);
    }
    if x(0.0f32).gt(length) {
        return;
    }
    let mid = x(g.fog_thickness as f64) * 0.5f64 + g.fog_base;
    let step = (mid - g.fog_base) / 30.0f64;
    let index = |h: F80| ((h - mid).abs() / step).to_i32(Round::Zero).min(39);
    let i1 = index(h1);
    let i0 = index(hit_h);
    let phi = |k: i32| {
        assert!((0..40).contains(&k), "layer fog index {k} outside the table");
        POISSON[k as usize]
    };
    // the original compares "start is below the middle" with "hit is at or above it"
    let start_below = !h1.ge(mid);
    let hit_above = hit_h.ge(mid);
    let (sum, count) = if start_below == hit_above || i1 == 0 {
        let mut far = x(0.0f32);
        for k in 0..=i1 {
            far = far + (F80::ONE - phi(k));
        }
        let mut near = x(0.0f32);
        for k in 0..=i0 {
            near = near + phi(k);
        }
        (far + near, i0 + i1 + 2)
    } else {
        let n = (i1 - i0).abs() + 1;
        let from = if i1 < i0 { i1 } else { i0 };
        let mut sum = x(0.0f32);
        for k in from..from + n {
            sum = sum + (F80::ONE - phi(k));
        }
        (sum, n)
    };
    let mean = (sum / x(count)) * g.fog_density as f64;
    let amount = ((mean + mean) * length) / 10.0f64;
    let amount = if x(0.0f32).gt(amount) {
        x(0.0f32)
    } else if x(1.0f32).lt(amount) {
        F80::ONE
    } else {
        amount
    };
    let rest = F80::ONE - amount;
    color[0] = (amount * g.fog_color[0] + rest * color[0]).to_f32();
    color[1] = (amount * g.fog_color[1] + rest * color[1]).to_f32();
    color[2] = (rest * color[2] + amount * g.fog_color[2]).to_f32();
}

// ------------------------------------------------------------------------- reflect and refract

/// The colour seen in the mirror direction [soft 0x082685f0 reflect]:
/// `reflect(ray, tn, point, normal, background)`.
///
/// Returns the global background ([`ShadeGlobals::background`]) when the ray is already
/// deeper than the limit or when c = normal · direction is not below 0.0001 (the ray comes
/// from behind). Otherwise the mirror direction is direction + normal × float(−2 c),
/// normalised. With no reflection map on the material, or one of method 7, a new ray (depth
/// + 1, same cell, same travelled distance) is traced and, if it hits, shaded with
/// `background`; that is the result. If it hits nothing and there is a map (or the map is
/// not of method 7 to begin with) the map is looked up as an environment: the direction
/// taken back to the scene's space and normalised, u = ((atan2(z, x) + rotation) / π + 1) / 2,
/// v = acos(y) / π, the sample times the map's factor. In that path the original never
/// writes the alpha, the depth and the sixth value of its result: it clamps whatever its
/// stack frame held and returns it; here they are [`ShadeGlobals::unset`]. (`SI_shade` reads
/// only the colour of a `reflect` result; the alpha matters only through `refract`'s
/// total-reflection path.) Class: model; libm in the environment
/// look-up (`acosf`, whose result is used as the register holds it; that a C library returns
/// it rounded to float is measured: [`si_x87::trig::acosf`]).
pub fn reflect<E: ShadeEnv>(st: &mut ShadeState, env: &mut E, ray: &Ray, tn: usize, p: Vector, n: Vector, background: Rgbazo) -> Rgbazo {
    let depth = ray.w3e;
    if depth > st.glb.max_depth {
        return st.glb.background;
    }
    let d = ray.dir;
    let c = x(n.z) * d.z + (x(n.y) * d.y + x(n.x) * d.x);
    if c.ge(x(EPS)) {
        return st.glb.background;
    }
    let k = (c * -2.0f64).to_f32();
    let rx = x(d.x) + x(n.x) * k;
    let rz = x(n.z) * k + d.z;
    let ry = x(d.y) + x(k) * n.y;
    let mut r = unit_or_rounded([rx, ry, rz], (rz * rz + (rx * rx + ry * ry)).sqrt());
    let map = st.materials[st.tns[tn].material as usize].reflect_map;
    if map.map_or(true, |m| st.maps[m].method == 7) {
        env.ray_alloc();
        let mut new = Ray { origin: p, dir: r, w38: ray.w38, w3e: depth + 1, w3c: ray.w3c, w40: ray.w40, ..Ray::default() };
        if env.trace(st, &mut new, tn) {
            return shade(st, env, &mut new, background, 0);
        }
    }
    let Some(map) = map else {
        return st.glb.background;
    };
    r = st.camera.world_to_raw(r);
    r = unit_or_rounded([x(r.x), x(r.y), x(r.z)], ((x(r.y) * r.y + x(r.x) * r.x) + x(r.z) * r.z).sqrt());
    let azimuth = trig::fpatan(x(r.z), x(r.x)).to_f32();
    let pi = core::f64::consts::PI;
    let u = ((((x(azimuth) + st.maps[map].env_rotation) / pi) + 1.0f32) * 0.5f64).to_f32();
    // (what `acosf` leaves in the register is divided as it is: no store in between)
    let v = (trig::acosf(r.y) / pi).to_f32();
    let sample = env.blookup(st, u, v, map, false)[0];
    let scale = st.maps[map].env_scale;
    let unset = st.glb.unset;
    Rgbazo { r: sample[0] * scale, g: sample[1] * scale, b: sample[2] * scale, a: unset, z: unset, o: unset }
}

/// The colour seen through a transparent surface [soft 0x082689b4 refract]:
/// `refract(ray, tn, point, normal)`.
///
/// Returns the global background when the ray is deeper than the limit. With c = normal ·
/// direction: c < 0 is a ray entering (ratio = the material's index of refraction, c
/// negated, depth unchanged); otherwise the ray is leaving (ratio = float(1 / index), the
/// normal negated, depth + 1). Then with q = direction / c and s = q + normal:
/// k = |q|² × ratio² − |s|². k ≤ 0 (or NaN) is total reflection: the result is
/// `reflect(ray, tn, point, normal, global background)` with the normal as negated. Else the
/// refracted direction is float(1 / √k) × float(s) − normal, normalised; the new ray is
/// traced and, if it hits, shaded against the global background; if not, the global
/// background is the result. Class: model.
pub fn refract<E: ShadeEnv>(st: &mut ShadeState, env: &mut E, ray: &Ray, tn: usize, p: Vector, n: Vector) -> Rgbazo {
    let mut depth = ray.w3e;
    if depth > st.glb.max_depth {
        return st.glb.background;
    }
    let d = ray.dir;
    let index = st.materials[st.tns[tn].material as usize].work.refraction;
    let mut n = n;
    let mut c = (x(n.y) * d.y + x(n.x) * d.x) + x(n.z) * d.z;
    let ratio;
    if x(0.0f32).gt(c) {
        ratio = index;
        c = -c;
    } else {
        depth += 1;
        n = Vector::new(-n.x, -n.y, -n.z);
        ratio = (F80::ONE / index).to_f32();
    }
    let qx = x(d.x) / c;
    let qy = x(d.y) / c;
    let sx = qx + n.x;
    let sy = qy + n.y;
    let qz = x(d.z) / c;
    let sz = qz + n.z;
    let k = ((qz * qz + (qy * qy + qx * qx)) * (x(ratio) * ratio)) - (sz * sz + (sy * sy + sx * sx));
    if !k.gt(x(0.0f32)) {
        let bg = st.glb.background;
        return reflect(st, env, ray, tn, p, n, bg);
    }
    let f = (F80::ONE / k.sqrt()).to_f32();
    let tx = x(f) * sx.to_f32() - n.x;
    let ty = x(f) * sy.to_f32() - n.y;
    let tz = x(f) * sz.to_f32() - n.z;
    let dir = unit_or_rounded([tx, ty, tz], (tz * tz + (ty * ty + tx * tx)).sqrt());
    env.ray_alloc();
    let mut new = Ray { origin: p, dir, w38: ray.w38, w3e: depth, w3c: ray.w3c, w40: ray.w40, ..Ray::default() };
    if env.trace(st, &mut new, tn) {
        let bg = st.glb.background;
        shade(st, env, &mut new, bg, 0)
    } else {
        st.glb.background
    }
}

// ----------------------------------------------------------------------------------- SI_shade

/// One 2D texture map applied at the hit point: the body of the two loops of `SI_shade` over
/// the material's maps (0x082636c0) and over the model's (0x08264420), which are the same
/// instructions. `pair` is the index of the map's (u, v) pair in each corner's list; `b` the
/// hit's three weights as stored in the ray, `inv` 1 / their sum as a double.
#[allow(clippy::too_many_arguments)]
fn apply_map<E: ShadeEnv>(st: &mut ShadeState, env: &mut E, mat: usize, map: usize, pair: usize, v: &[RVertex; 3], b: [f32; 3], inv: f64, normal: &mut Vector) {
    let m = st.maps[map].clone();
    if m.skip != 0 {
        return;
    }
    // Each corner's pairs are read through its pointer at +0x40 without a test (0x082636ec,
    // and 0x0826444c for the node's own maps), v first. A corner that was given no pairs
    // (a map list whose count, material +0x6e8 plus node +0x216, `si_tcoordNeeded` found 0)
    // has null there: the original reads address 8 × pair + 4 and is stopped by a
    // segmentation fault (`SI_serverExit` through the handler `SI_signalInit` installs).
    // The Rust cannot fault: it notes the address the original reads ([`ShadeState::fault`])
    // and leaves the map out; whoever runs the frame ends it there.
    if v.iter().any(|c| c.uv == crate::softvertex::NO_UV) {
        st.fault.get_or_insert((8 * pair as u32).wrapping_add(4));
        return;
    }
    let uv = [st.uvs[v[0].uv as usize + pair], st.uvs[v[1].uv as usize + pair], st.uvs[v[2].uv as usize + pair]];
    let ([u0, v0], [u1, v1], [u2, v2]) = (uv[0], uv[1], uv[2]);
    let uf = ((x(b[2]) * u2 + (x(b[1]) * u1 + x(b[0]) * u0)) * inv).to_f32();
    let vf = ((x(b[2]) * v2 + (x(b[1]) * v1 + x(b[0]) * v0)) * inv).to_f32();
    let (mut u, mut vv) = (x(uf), x(vf));
    let tiled = m.method == 4 && m.repeat != 0;
    if !(1..=3).contains(&m.method) {
        if m.method == 4 {
            if m.repeat != 0 {
                u = x((x(uf) - x(x(uf).round_int(Round::Down).to_f64())).to_f32());
                vv = x((x(vf) - x(x(vf).round_int(Round::Down).to_f64())).to_f32());
            } else {
                if m.wrap_u != 0 {
                    u = x(uf) - x(x(uf).to_i32(Round::Zero));
                }
                if m.wrap_v != 0 {
                    vv = x(vf) - x(x(vf).to_i32(Round::Zero));
                }
            }
        } else {
            u = x(uf) - x(x(uf).to_i32(Round::Zero));
            vv = x(vf) - x(x(vf).to_i32(Round::Zero));
        }
    }
    if !tiled {
        // inside the mapped region in u, then in v; a NaN is outside
        let inside = |c: F80, lo: f32, hi: f32, wrap: i16, band: i16, lo2: f32, hi2: f32| {
            (c.ge(x(lo)) && c.le(x(hi))) || (wrap != 0 && (c.ge(x(lo2)) || c.le(x(hi2)))) || (band != 0 && c.ge(x(lo2)) && c.le(x(hi2)))
        };
        if !inside(u, m.umin, m.umax, m.wrap_u, m.band_u, m.u2min, m.u2max) || !inside(vv, m.vmin, m.vmax, m.wrap_v, m.band_v, m.v2min, m.v2max) {
            return;
        }
    }
    if m.wrap_u != 0 && u.lt(x(m.u2max)) {
        u = u + 1.0f32;
    }
    if m.wrap_v != 0 && vv.lt(x(m.v2max)) {
        vv = vv + 1.0f32;
    }
    let [sample, du, dv] = env.blookup(st, u.to_f32(), vv.to_f32(), map, m.bump != 0);
    env.render_txt2d(st, mat, map, sample);
    if m.bump == 0 {
        return;
    }
    // two directions on the surface: `t1` along u, `t2` along v
    let n = *normal;
    let (t1, t2);
    match m.method {
        1 => {
            t1 = Vector::new(1.0, 0.0, 0.0);
            t2 = Vector::new(0.0, 1.0, 0.0);
        }
        2 => {
            t1 = Vector::new(1.0, 0.0, 0.0);
            t2 = Vector::new(0.0, 0.0, 1.0);
        }
        3 => {
            t1 = Vector::new(0.0, 1.0, 0.0);
            t2 = Vector::new(0.0, 0.0, 1.0);
        }
        _ => {
            let (a, bb, c) = (v[0].raw, v[1].raw, v[2].raw);
            let raw;
            if (4..=6).contains(&m.method) {
                // the edge along which u does not change, pointing towards greater v
                let sub = |p: Vector, q: Vector| [x(p.x) - q.x, x(p.y) - q.y, x(p.z) - q.z];
                raw = if x(EPS).gt((x(u0) - u1).abs()) {
                    if v1 < v0 { sub(a, bb) } else { sub(bb, a) }
                } else if (x(u0) - u2).abs().lt(x(EPS)) {
                    if v2 < v0 { sub(a, c) } else { sub(c, a) }
                } else if v2 < v1 {
                    sub(bb, c)
                } else {
                    sub(c, bb)
                };
            } else {
                // the direction of constant u through one corner; x and y use the ratio
                // rounded to float, z the ratio as it stands in the register
                let (k, p, q, r) = if x(EPS).lt((x(u1) - u2).abs()) {
                    (x(u0) / (x(u2) - u1), a, bb, c)
                } else if (x(u0) - u2).abs().gt(x(EPS)) {
                    (x(u1) / (x(u0) - u2), bb, c, a)
                } else if (x(u1) - u0).abs().gt(x(EPS)) {
                    (x(u2) / (x(u1) - u0), c, a, bb)
                } else {
                    return;
                };
                let kf = k.to_f32();
                raw = [(x(p.x) - q.x) - (x(r.x) - q.x) * kf, (x(p.y) - q.y) - (x(r.y) - q.y) * kf, (x(p.z) - q.z) - k * (x(r.z) - q.z)];
            }
            let e = Vector::new(raw[0].to_f32(), raw[1].to_f32(), raw[2].to_f32());
            let len = ((x(e.y) * e.y + x(e.x) * e.x) + x(e.z) * e.z).sqrt();
            let mut e = unit_or_rounded([x(e.x), x(e.y), x(e.z)], len);
            if !(4..=6).contains(&m.method) && x(e.x).lt(x(0.0f32)) {
                e = Vector::new(-e.x, -e.y, -e.z);
            }
            t2 = e;
            t1 = Vector::new((x(n.y) * e.z - x(e.y) * n.z).to_f32(), (x(n.z) * e.x - x(e.z) * n.x).to_f32(), (x(n.x) * e.y - x(n.y) * e.x).to_f32());
        }
    }
    // the height differences, as integers, times the map's amplitude
    let h = if m.bump_alpha == 1 { 3 } else { 4 };
    let d1 = (x(du[h]) - sample[h]).to_i32(Round::Zero);
    let d2 = (x(dv[h]) - sample[h]).to_i32(Round::Zero);
    let a1 = x(d1) * m.bump_amount;
    let a2 = x(d2) * m.bump_amount;
    let w = ((x(1.0f64) - a1) - a2).to_f32();
    let nx = a2 * t2.x + (a1 * t1.x + x(w) * n.x);
    let nz = a2 * t2.z + (a1 * t1.z + x(w) * n.z);
    let ny = (a1 * t1.y + x(w) * n.y) + a2 * t2.y;
    *normal = unit_or_rounded([nx, ny, nz], (nz * nz + (ny * ny + nx * nx)).sqrt());
}

/// The three ways `SI_shade` carries normal · light from the test to the diffuse term.
#[derive(Clone, Copy, PartialEq)]
enum Lit {
    Lambert,
    Phong,
    Blinn,
}

/// What a ray that has hit a triangle is worth [soft 0x08263040 SI_shade]:
/// `SI_shade(ray, background, primary)`, returning six floats: red, green, blue, alpha, the
/// hit's depth and the depth of field value.
///
/// In order (finding 0012 has the formulas):
///
/// 1. With fade or layer fog on, the ray's travelled distance (+0x40) becomes its own
///    length for a primary ray, and grows by it otherwise.
/// 2. The material's working block is saved. The flag word is the material's OR the model's.
/// 3. The hit point is origin + t × direction; its z, rounded, is the result's depth. With
///    depth of field on, a primary ray gets its sixth value from that z (unrounded) and the
///    three focus depths, replaced by the background's when that is larger in size.
/// 4. A primary ray with ray tracing on has its BSP cell looked up.
/// 5. The ray's three hit weights are multiplied by the triangle's `inv_n` and stored back.
/// 6. The normal: the first corner's, or (flag 2) the weighted sum of the three, normalised.
/// 7. With textures (flag 4 and the global switch): the working block is reset from the
///    material; vertex colours are applied; the 3D texture; each 2D map of the material,
///    then of the model.
/// 8. The normal is turned towards the viewer when the triangle's plane normal · direction
///    is above 0.0001.
/// 9. Transparency (flags 0x340): opacity = working opacity × existence, times the static
///    blur table's value at |direction · normal| when static blur is on. Below 0.999, and
///    for a secondary ray or with flag 0x200, and with refraction on, the background becomes
///    `refract`'s result. Below 0.0001 the background, clamped, is the answer.
/// 10. The shading model (see the finding): colour and alpha from the lights.
/// 11. Reflection (flags 0x30): colour = (1 − r) × colour + r × `reflect`'s.
/// 12. Fade, then layer fog.
/// 13. A transparent surface is blended over the background by its opacity.
/// 14. Colour and alpha are clamped to 0..1 and the working block is put back.
///
/// Shading model 2 leaves at the end of step 10 with its colour clamped: no reflection,
/// fade, fog or blending. A model outside 1..5 keeps whatever the four result floats held
/// on the stack; the Rust has zeros there.
///
/// Class: model (libm inside `reflect`'s environment look-up).
pub fn shade<E: ShadeEnv>(st: &mut ShadeState, env: &mut E, ray: &mut Ray, background: Rgbazo, primary: i16) -> Rgbazo {
    let mut bg = background;
    if st.glb.fade != 0 || st.glb.fog != 0 {
        let d = if primary != 0 { ray.t } else { ray_dist(ray) + ray.t };
        ray.w40 = d.to_bits();
    }
    let tn_index = ray.tn.expect("the ray has hit a triangle");
    let tn = st.tns[tn_index].clone();
    let mat = tn.material as usize;
    let saved = st.materials[mat].work;
    let model = st.models[tn.w58 as usize].clone();
    let flags = st.materials[mat].flags | model.flags;
    let transparent = flags & flag::TRANSPARENT != 0;

    // 3. the hit point
    let px = (x(ray.t) * ray.dir.x + ray.origin.x).to_f32();
    let py = (x(ray.t) * ray.dir.y + ray.origin.y).to_f32();
    let pz80 = x(ray.t) * ray.dir.z + ray.origin.z;
    let p = Vector::new(px, py, pz80.to_f32());
    let mut out_o = st.glb.unset; // the original leaves this stack slot unwritten without depth of field
    if st.glb.dof != 0 && primary != 0 {
        let g = &st.glb;
        out_o = if x(g.dof_z0).lt(pz80) {
            let t = (pz80 - g.dof_z0) * x(g.dof_scale);
            if (-x(g.dof_z2)).lt(t) { -g.dof_z2 } else { t.to_f32() }
        } else if x(g.dof_z1).lt(pz80) {
            0.0
        } else if x(g.dof_z2).lt(pz80) {
            (x(g.dof_z2) * ((pz80 - g.dof_z1) / (x(g.dof_z2) - g.dof_z1))).to_f32()
        } else {
            g.dof_z2
        };
        if !x(NO_O).eq(x(bg.o)) {
            let size = if x(0.0f32).le(x(bg.o)) { x(bg.o) } else { -x(bg.o) };
            let mine = if x(out_o).lt(x(0.0f32)) || x(out_o).unordered(x(0.0f32)) { -x(out_o) } else { x(out_o) };
            if mine.lt(size) {
                out_o = bg.o;
            }
        }
    }
    if st.glb.raytrace && primary & 1 != 0 {
        ray.w38 = env.search(st, p);
    }

    // 5. the weights
    let b0 = x(ray.hit.x) * tn.inv_n;
    let b1 = x(ray.hit.y) * tn.inv_n;
    let b2 = x(ray.hit.z) * tn.inv_n;
    ray.hit = Vector::new(b0.to_f32(), b1.to_f32(), b2.to_f32());
    let inv = (x(1.0f64) / ((b0 + b1) + b2)).to_f64();
    let v = corners(st, &tn);

    // 6. the normal
    let mut n = if flags & flag::SMOOTH != 0 {
        let nx = (b0 * v[0].normal.x + b1 * v[1].normal.x) + b2 * v[2].normal.x;
        let ny = (b0 * v[0].normal.y + b1 * v[1].normal.y) + b2 * v[2].normal.y;
        let nz = b2 * v[2].normal.z + (b1 * v[1].normal.z + b0 * v[0].normal.z);
        unit_or_rounded([nx, ny, nz], ((ny * ny + nx * nx) + nz * nz).sqrt())
    } else {
        v[0].normal
    };

    // 7. textures
    let textured = flags & flag::TEXTURED != 0 && st.glb.textures != 0;
    if textured {
        let m = &mut st.materials[mat];
        m.work.ambient = m.work.base_ambient;
        m.work.diffuse = m.diffuse;
        m.work.specular = m.specular;
        m.work.reflectivity = m.reflectivity;
        m.work.refraction = m.refraction;
        m.work.opacity = 1.0 - m.transparency;
    }
    if !(st.materials[mat].vcol_rgb == 1 && st.materials[mat].vcol_alpha == 1) {
        set_material_from_vertex_color(st, ray, p);
    }
    if textured {
        if st.glb.textures_3d != 0 {
            if st.materials[mat].tex3d != 0 {
                env.render_txt3d(st, mat, p, Some(&mut n));
            } else if model.tex3d != 0 {
                st.materials[mat].tex3d = model.tex3d;
                env.render_txt3d(st, mat, p, Some(&mut n));
                st.materials[mat].tex3d = 0;
            }
        }
        if st.glb.textures_2d != 0 {
            let b = [ray.hit.x, ray.hit.y, ray.hit.z];
            let mut pair = 0usize;
            for head in [st.materials[mat].maps, model.maps] {
                let mut cur = head;
                while let Some(map) = cur {
                    apply_map(st, env, mat, map, pair, &v, b, inv, &mut n);
                    pair += 1;
                    cur = st.maps[map].next;
                }
            }
        }
    }

    // 8. towards the viewer
    let facing = (x(tn.n.x) * ray.dir.x + x(tn.n.y) * ray.dir.y) + x(tn.n.z) * ray.dir.z;
    let nf = if facing.gt(x(EPS)) { Vector::new(-n.x, -n.y, -n.z) } else { n };

    // 9. transparency
    let mut opacity = 1.0f32;
    if transparent {
        let m = &st.materials[mat];
        opacity = m.work.opacity * m.work.existence;
        if m.static_blur != 0 {
            let d = ray.dir;
            let d = unit_or_rounded([x(d.x), x(d.y), x(d.z)], ((x(d.y) * d.y + x(d.x) * d.x) + x(d.z) * d.z).sqrt());
            let c = (x(d.x) * nf.x + x(d.y) * nf.y) + x(d.z) * nf.z;
            let c = if x(0.0f32).le(c) { c } else { -c };
            opacity = (m.blur_table.lookup(c) * opacity).to_f32();
        }
        if x(opacity).lt(x(OPAQUE)) && (flags & flag::REFRACTS != 0 || primary == 0) && st.glb.refraction != 0 {
            let current = st.materials[mat].work;
            st.materials[mat].work = saved;
            bg = refract(st, env, ray, tn_index, p, n);
            st.materials[mat].work = current;
        }
        if x(opacity).lt(x(EPS)) {
            st.materials[mat].work = saved;
            return Rgbazo { r: clamp01(bg.r), g: clamp01(bg.g), b: clamp01(bg.b), a: clamp01(bg.a), z: bg.z, o: bg.o };
        }
    }

    // 10. the shading model
    let covered = ((x(opacity) + bg.a) - x(bg.a) * opacity).to_f32();
    let (mut color, mut alpha) = ([0.0f32; 3], 0.0f32);
    match st.materials[mat].model {
        1 => {
            let w = &st.materials[mat].work;
            color = [w.diffuse[0], w.diffuse[1], w.diffuse[2]];
            alpha = covered;
        }
        2 => {
            let w = st.materials[mat].work;
            color = [w.ambient[0], w.ambient[1], w.ambient[2]];
            alpha = covered;
            let flat = v[0].normal;
            let mut cur = st.light_head;
            while let Some(li) = cur {
                let lt = &st.lights[li];
                cur = lt.next;
                if !light_applies(lt, tn.w58) {
                    continue;
                }
                let d = (x(flat.x) * lt.dir.x + x(flat.y) * lt.dir.y) + x(flat.z) * lt.dir.z;
                if d.lt(x(EPS)) {
                    continue;
                }
                for k in 0..3 {
                    color[k] = ((x(w.diffuse[k]) * lt.color[k]) * d + color[k]).to_f32();
                }
            }
            st.materials[mat].work = saved;
            return Rgbazo { r: clamp01(color[0]), g: clamp01(color[1]), b: clamp01(color[2]), a: clamp01(alpha), z: p.z, o: out_o };
        }
        3 => {
            let w = &st.materials[mat].work;
            color = [w.ambient[0], w.ambient[1], w.ambient[2]];
            alpha = covered;
            lights(st, env, ray, tn_index, mat, saved, p, nf, Lit::Lambert, &mut color, &mut alpha);
        }
        4 | 5 => {
            let w = &st.materials[mat].work;
            color = [w.ambient[0], w.ambient[1], w.ambient[2]];
            alpha = opacity;
            let kind = if st.materials[mat].model == 4 { Lit::Phong } else { Lit::Blinn };
            lights(st, env, ray, tn_index, mat, saved, p, nf, kind, &mut color, &mut alpha);
        }
        _ => {}
    }

    // 11. reflection
    if flags & flag::REFLECTS != 0 && st.glb.reflection != 0 {
        let r = st.materials[mat].work.reflectivity;
        let keep = (x(1.0f32) - r).to_f32();
        let current = st.materials[mat].work;
        st.materials[mat].work = saved;
        let seen = reflect(st, env, ray, tn_index, p, nf, bg);
        st.materials[mat].work = current;
        color[0] = (x(r) * seen.r + x(keep) * color[0]).to_f32();
        color[2] = (x(keep) * color[2] + x(r) * seen.b).to_f32();
        color[1] = (x(keep) * color[1] + x(r) * seen.g).to_f32();
    }

    // 12. fade, layer fog
    let g = &st.glb;
    if g.fade != 0 && x(ray_dist(ray)).gt(x(g.fade_start)) {
        let f = if x(ray_dist(ray)).gt(x(g.fade_end)) { x(g.background.a) } else { (x(ray_dist(ray)) - g.fade_start) * g.fade_scale };
        let keep = (x(1.0f32) - f).to_f32();
        color[0] = (f * g.background.r + x(keep) * color[0]).to_f32();
        color[1] = (x(keep) * color[1] + f * g.background.g).to_f32();
        color[2] = (x(keep) * color[2] + f * g.background.b).to_f32();
    }
    if g.fog != 0 && x(g.fog_start).lt(x(ray_dist(ray))) {
        layer_fog(st, ray, p, &mut color);
    }

    // 13. over the background
    if transparent && x(opacity).lt(x(OPAQUE)) {
        let through = (x(1.0f32) - opacity).to_f32();
        color[0] = (x(opacity) * color[0] + x(through) * bg.r).to_f32();
        color[2] = (x(opacity) * color[2] + x(through) * bg.b).to_f32();
        color[1] = (x(opacity) * color[1] + x(through) * bg.g).to_f32();
        alpha = (x(alpha) + (x(1.0f32) - alpha) * bg.a).to_f32();
    }

    // 14.
    st.materials[mat].work = saved;
    Rgbazo { r: clamp01(color[0]), g: clamp01(color[1]), b: clamp01(color[2]), a: clamp01(alpha), z: p.z, o: out_o }
}

/// The two tests at the head of every light loop: a selective light lights only the models
/// whose table entry matches; a light that is off lights nothing.
fn light_applies(lt: &ShadeLight, model: i16) -> bool {
    if lt.selective != 0 && lt.select_table[model as usize] != lt.select_id {
        return false;
    }
    lt.on != 0
}

/// The loop over the lights of shading models 3, 4 and 5 (0x08265ab0, 0x08266534,
/// 0x08267182 in `SI_shade`): diffuse and specular contributions added to `color`, the
/// highlight's share added to `alpha`.
///
/// Three copies of one source text that the compiler did not make alike; `kind` selects what
/// differs:
///
/// - Lambert rejects a spot light's ray when its cosine is **at or below** the cone's outer
///   cosine, Phong and Blinn only when it is below.
/// - Lambert and Phong keep normal · light in the register from the test to the diffuse
///   term; Blinn stores it to a float first (and adds its third product to the first two
///   already rounded to float).
#[allow(clippy::too_many_arguments)]
fn lights<E: ShadeEnv>(st: &mut ShadeState, env: &mut E, ray: &Ray, tn_index: usize, mat: usize, saved: MatWork, p: Vector, n: Vector, kind: Lit, color: &mut [f32; 3], alpha: &mut f32) {
    let model = st.tns[tn_index].w58;
    let eps = x(EPS);
    let mut cur = st.light_head;
    while let Some(li) = cur {
        // (a copy of the light without its depth map, which the shadow test below reads in
        // place: copying the map at every shading call made a frame with a depth-map light
        // take minutes; nothing here writes the light)
        let zmap = std::mem::take(&mut st.lights[li].zmap);
        let lt = st.lights[li].clone();
        st.lights[li].zmap = zmap;
        cur = lt.next;
        if !light_applies(&lt, model) {
            continue;
        }
        // direction and distance
        let (l, dist);
        if (lt.kind.wrapping_sub(3) as u16) <= 1 {
            l = lt.dir;
            let abs = |c: f32| if x(c).ge(x(0.0f32)) { x(c) } else { -x(c) };
            // a direction with all three components below the tolerance is no light
            if eps.gt(abs(l.x)) && abs(l.y).lt(eps) && eps.gt(abs(l.z)) {
                continue;
            }
            dist = x(FAR_LIGHT);
        } else {
            let (ax, ay, az) = (lt.pos.x, lt.pos.y, lt.pos.z);
            let len = fx!((sqrt (add (mul (sub (f az) (f p.z)) (sub (f az) (f p.z))) (add (mul (sub (f ay) (f p.y)) (sub (f ay) (f p.y))) (mul (sub (f ax) (f p.x)) (sub (f ax) (f p.x)))))));
            l = Vector::new(fx!(f32: (div (sub (f ax) (f p.x)) (x len))), fx!(f32: (div (sub (f ay) (f p.y)) (x len))), fx!(f32: (div (sub (f az) (f p.z)) (x len))));
            dist = len;
        }
        let ndl = if kind == Lit::Blinn {
            let two = fx!(f32: (add (mul (f n.x) (f l.x)) (mul (f n.y) (f l.y))));
            fx!((add (mul (f n.z) (f l.z)) (f two)))
        } else {
            fx!((add (add (mul (f n.x) (f l.x)) (mul (f n.y) (f l.y))) (mul (f n.z) (f l.z))))
        };
        if ndl.lt(eps) {
            continue;
        }
        let ndl = if kind == Lit::Blinn { x(ndl.to_f32()) } else { ndl };
        let mut col = [lt.color[0], lt.color[1], lt.color[2]];
        let scale = |col: &mut [f32; 3], k: f32| {
            for c in col.iter_mut() {
                *c = (x(*c) * k).to_f32();
            }
        };
        if lt.falloff {
            if x(lt.falloff_end).lt(dist) {
                continue;
            }
            if x(lt.falloff_start).lt(dist) {
                let k = (x(1.0f64) - (dist - lt.falloff_start) / (x(lt.falloff_end) - lt.falloff_start)).to_f32();
                scale(&mut col, k);
            }
        }
        if lt.kind == 2 {
            let c = -((x(lt.spot_dir.x) * l.x + x(lt.spot_dir.y) * l.y) + x(lt.spot_dir.z) * l.z);
            if c.lt(x(0.0f32)) {
                continue;
            }
            let outside = if kind == Lit::Lambert { c.le(x(lt.spread_cos)) } else { c.lt(x(lt.spread_cos)) };
            if outside {
                continue;
            }
            if x(lt.cone_cos).gt(c) {
                let k = (x(1.0f64) - (c - lt.cone_cos) / lt.cone_div).to_f32();
                scale(&mut col, k);
            }
        }
        // shadows
        if st.glb.shadows != 0 {
            match lt.shadow {
                2 => {
                    // the shadow ray sees the material as it was on entry
                    let mut sray = Ray { origin: p, dir: l, w38: ray.w38, ..Ray::default() };
                    let current = st.materials[mat].work;
                    st.materials[mat].work = saved;
                    let left = if lt.array != 0 { env.s_array(st, &mut sray, tn_index, dist.to_f32(), li) } else { env.s_trace(st, &mut sray, tn_index, dist.to_f32(), li) };
                    st.materials[mat].work = current;
                    for k in 0..3 {
                        col[k] = (x(col[k]) * left[k]).to_f32();
                    }
                }
                3 | 4 => {
                    let mut q = Vector::default();
                    let _ = camera_to_light(p, &lt.depth_mat, &lt.xy_mat, &mut q);
                    let ix = x(q.x).to_i32(Round::Zero);
                    let iy = x(q.y).to_i32(Round::Zero);
                    // a cell outside the map is a wild read in the original; here it is lit
                    let zmap = &st.lights[li].zmap;
                    let cell = |cx: i32, cy: i32| -> Option<f32> { zmap.get(usize::try_from(cy).ok()?)?.get(usize::try_from(cx).ok()?).copied() };
                    if cell(ix, iy).is_some_and(|z| x(q.z).lt(x(z))) {
                        let factor = if lt.shadow == 3 {
                            x(lt.umbra)
                        } else {
                            let x0 = (ix - lt.soft_size).max(0);
                            let mut x1 = lt.soft_size + 1 + ix;
                            if lt.map_size <= x1 {
                                x1 = lt.map_size - 1;
                            }
                            let y0 = (iy - lt.soft_size).max(0);
                            let mut y1 = lt.soft_size + 1 + iy;
                            if lt.map_size <= y1 {
                                y1 = lt.map_size - 1;
                            }
                            let (mut lit, mut total) = (0i32, 0i32);
                            let mut cy = y0;
                            while cy < y1 {
                                let mut cx = x0;
                                while cx < x1 {
                                    if cell(cx, cy).is_some_and(|z| x(q.z).gt(x(z))) {
                                        lit += 1;
                                    }
                                    total += 1;
                                    cx += lt.soft_step;
                                }
                                cy += lt.soft_step;
                            }
                            let f = (((x(lit) * lt.soft_scale) / x(total)) * (F80::ONE - lt.umbra) + lt.umbra).to_f32();
                            if x(f).lt(x(0.0f32)) {
                                x(0.0f32)
                            } else if x(f).gt(x(1.0f32)) {
                                F80::ONE
                            } else {
                                x(f)
                            }
                        };
                        for c in col.iter_mut() {
                            *c = (x(*c) * factor).to_f32();
                        }
                    }
                }
                _ => {}
            }
        }
        // diffuse
        let w = st.materials[mat].work;
        for k in 0..3 {
            color[k] = fx!(f32: (add (mul (mul (f w.diffuse[k]) (f col[k])) (x ndl)) (f color[k])));
        }
        if kind == Lit::Lambert {
            continue;
        }
        // specular: the half vector between the light and the ray reversed
        let d = ray.dir;
        let hx = x(l.x) - d.x;
        let hy = x(l.y) - d.y;
        let hz = x(l.z) - d.z;
        let h = unit_or_rounded([hx, hy, hz], fx!((sqrt (add (mul (x hz) (x hz)) (add (mul (x hy) (x hy)) (mul (x hx) (x hx)))))));
        let ndh = fx!((add (add (mul (f n.x) (f h.x)) (mul (f n.y) (f h.y))) (mul (f n.z) (f h.z))));
        let table = &st.materials[mat].spec_table;
        if kind == Lit::Phong {
            if ndh.lt(eps) {
                continue;
            }
            let s = table.lookup(ndh);
            if s.lt(x(SPEC_MIN)) {
                continue;
            }
            for k in 0..3 {
                color[k] = fx!(f32: (add (mul (mul (f w.specular[k]) (f col[k])) (x s)) (f color[k])));
            }
            if st.glb.no_spec_alpha == 0 {
                let a = x(*alpha) + ((((x(w.specular[0]) + w.specular[1]) + w.specular[2]) / 3.0f32) * *alpha) * s;
                *alpha = if a.gt(x(1.0f32)) { 1.0 } else { a.to_f32() };
            }
            continue;
        }
        // Blinn: the same table value, times a geometry term, over normal · view
        let ndh64 = ndh.to_f64();
        if x(ndh64).lt(eps) {
            continue;
        }
        let s = table.lookup(ndh);
        let s64 = s.to_f64();
        if x(s64).lt(x(SPEC_MIN)) {
            continue;
        }
        let (vx, vy, vz) = (-x(d.x), -x(d.y), -x(d.z));
        let view = unit_or_rounded([vx, vy, vz], (vz * vz + (vy * vy + vx * vx)).sqrt());
        let vdh = x(view.z) * h.z + (x(view.y) * h.y + x(view.x) * h.x);
        let abs = |c: F80| if c.ge(x(0.0f32)) { c } else { -c };
        if eps.gt(abs(vdh)) {
            continue;
        }
        let ndv = x(view.z) * n.z + (x(view.y) * n.y + x(view.x) * n.x);
        if abs(ndv).lt(eps) {
            continue;
        }
        let twice = x(ndh64) + ndh64;
        let g1 = ((ndv * twice) / vdh).to_f32();
        let g2 = ((twice * ndl) / vdh).to_f32();
        let g1 = if x(g1).gt(x(1.0f32)) { F80::ONE } else { x(g1) };
        let geo = if g1.lt(x(g2)) { g1 } else { x(g2) };
        let sr = ((x(w.specular[0]) * s) * geo) / ndv;
        color[0] = (sr * col[0] + color[0]).to_f32();
        let sg = ((x(w.specular[1]) * s) * geo) / ndv;
        color[1] = (sg * col[1] + color[1]).to_f32();
        let sb = (geo * (s * w.specular[2])) / ndv;
        color[2] = (sb * col[2] + color[2]).to_f32();
        if st.glb.no_spec_alpha == 0 {
            let sum = ((sg + sr) + sb).to_f64();
            let a = (x(*alpha) + ((x(sum) / 3.0f64) * *alpha) * s64).to_f32();
            *alpha = if x(a).gt(x(1.0f32)) { 1.0 } else { a };
        }
    }
}

#[cfg(test)]
mod tests;
