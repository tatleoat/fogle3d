// SPDX-License-Identifier: MIT
//! Splines and patches: the program's curve objects (linear, Bézier, B-spline, cardinal)
//! and its surfaces made of them, with their evaluation, length tables, grids, editing
//! primitives and conversions, and the building blocks of curve fitting (finding 0020).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`: 54 of the 59 functions of the original's `spline.c` (two of
//! them, `SI_splineInit` and `SI_splineAllocate`, cited in `si-types` too), 30 of the 37 of
//! `patch.c`, the 3 of `bezier.c`, the 5 of `bez.c` ([`bez_image`]) and 29 of the 29 of `fitCurve.c` (`SI_bezierFitContour` in [`fit_contour_image`]). Every one is compared
//! with the emulated original (finding 0011; `spline-emucheck`).
//!
//! Two kinds of module (finding 0024). The modules of the first two passes work on values
//! of their own ([`Curve`], [`patch::Surface`]). The modules of the third and fourth pass
//! ([`spline_image`], [`fit_image`], [`patch_image`], [`tools_image`], [`contour_image`],
//! [`merge_image`], [`intersect_image`], [`frenet_image`], and of the fifth pass
//! [`convert_image`], [`patch_eval_image`]) work on `si-mem`'s image at the
//! original's 32-bit layout: a function takes the image and the arguments the original
//! takes and makes the original's allocation calls. [`spline_image`] has the image forms
//! of the primitives the others need, its globals and the one trait for callees outside
//! the package.
//!
//! The data model is `si-types`' records of the original's 32-bit layout
//! ([`si_types::geom::Spline`], 0x74 bytes; [`si_types::geom::SplineKey`], 0x30 bytes;
//! [`si_types::geom::Patch`], 0x204 bytes). A [`Curve`] is a spline record with the two
//! arrays it points to held beside it; a [`patch::Surface`] a patch record with its eight.
//!
//! Arithmetic follows the original's x87 instruction sequences on [`si_x87::F80`]: the same
//! operations in the same order, rounded to `float` (or `double`, in the fitting code)
//! exactly where the original stores to memory (finding 0008). Each function's comment says
//! where those stores are.
//!
//! How a curve is evaluated, in short (all read from code):
//!
//! - a curve of `n` segments has the parameter range `[0, n]`; the whole part of a parameter
//!   picks the segment, the fraction `u` is the place on it;
//! - the point is `[u³ u² u 1] × B × G`: `B` the 4 × 4 basis matrix of the curve's type
//!   ([`basis`]), `G` the four rows of geometry of the segment ([`eval::fill_geometry`]);
//! - drawing and the length table step along a segment by forward differences: the matrix
//!   `E(1/step) × B × G` holds the point and its first three differences, and one step adds
//!   each row to the next ([`length::update_length`]).
//!
//! A patch is the same in two directions ([`patch`]); [`patch_grid`] turns it into the
//! display curves, the grid of points and quadrilaterals of the mesh conversion and the
//! renderer's grid of points and normals; [`patch_edit`] and [`tools`] hold the editing
//! routines of patches and splines; [`fit`] the helpers of the Bézier curve fit.

pub mod basis;
pub mod bez_image;
pub mod contour_image;
pub mod convert;
pub mod convert_image;
pub mod edit;
pub mod eval;
pub mod fit;
pub mod fit_image;
pub mod fit_contour_image;
pub mod frenet_image;
pub mod intersect_image;
pub mod length;
pub mod merge_image;
pub mod patch;
pub mod patch_edit;
pub mod patch_eval_image;
pub mod revolution_image;
pub mod patch_grid;
pub mod patch_draw_image;
pub mod patch_image;
pub mod patch_read_image;
pub mod spline_draw_image;
pub mod spline_image;
pub mod tools;
pub mod tools_image;
pub mod zip_image;

pub use si_types::geom::{Spline, SplineKey, Vector};
use si_types::{Ptr32, Zero};

/// The spline types (`SI_SplineType`, the word at +0x0c of a spline; the model file's
/// "type" token, finding 0004).
pub const LINEAR: i32 = 0;
pub const BEZIER: i32 = 1;
pub const BSPLINE: i32 = 2;
pub const CARDINAL: i32 = 3;

/// A key's segment type (+0x2a) that makes the Bézier segment *ending* at that key a
/// straight line: the two inner control points are then taken a third of the way along the
/// chord, whatever the stored handles say (read from `splineFillGeometry`).
pub const SEGMENT_LINEAR: i16 = 2;

/// Bit 0 of a key's flag word (+0x2c): the point is selected (tagged). Read from
/// `SI_splineDeleteSelection`.
pub const KEY_SELECTED: i16 = 1;

/// Bit 0x10 of the spline's word at +0x04, set by every [`edit::touch`]: the spline has
/// changed since something (the drawing code clears it; not read further).
pub const SPLINE_TOUCHED: i16 = 0x10;

/// A placeholder for a pointer the Rust replaces by an owned array: any non-null value.
pub(crate) fn held<T>() -> Ptr32<T> {
    Ptr32::new(1)
}

/// The float at +0x24 of a key, which `si-types` lists as four unknown bytes: 1.0 in a key
/// made by `SI_splineKeyAllocate`, carried along by every routine that copies whole keys
/// and never read by the routines ported here.
pub fn key_weight(k: &SplineKey) -> f32 {
    f32::from_le_bytes(k.unk_24)
}

/// A key as `SI_splineKeyAllocate` initialises it: zero but for the float 1.0 at +0x24.
pub fn new_key() -> SplineKey {
    let mut k = SplineKey::zero();
    k.unk_24 = 1.0f32.to_le_bytes();
    k
}

/// A spline with the arrays it owns.
///
/// `keys` is the whole **allocated** block, not only the keys in use: the original
/// allocates room for a multiple of 16 keys and initialises only as many as were asked for,
/// and some routines read or write a record past the count (the first one after a closed
/// curve's last key, the slot a new point goes into). `rec.nb_keys` (read as an unsigned
/// 16-bit number) says how many are in use. `rec.keys` is null exactly when no block is
/// allocated; `rec.lengths` is null exactly when there is no length table.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub rec: Spline,
    /// The string at `rec.name`, when there is one.
    pub name: Option<Vec<u8>>,
    pub keys: Vec<SplineKey>,
    /// The table at `rec.lengths`: `rec.nb_lengths × segments + 1` cumulated lengths.
    pub lengths: Vec<f32>,
}

impl Default for Curve {
    fn default() -> Self {
        Curve::new()
    }
}

impl Curve {
    /// An initialised spline without a name or keys: what [`Spline::init`]
    /// (`SI_splineInit`) leaves.
    pub fn new() -> Curve {
        let mut rec = Spline::zero();
        rec.init();
        Curve { rec, name: None, keys: Vec::new(), lengths: Vec::new() }
    }

    /// The number of keys in use, as the original reads it nearly everywhere: unsigned.
    pub fn nb(&self) -> usize {
        self.rec.nb_keys as u16 as usize
    }

    /// The keys in use.
    pub fn used(&self) -> &[SplineKey] {
        let n = self.nb().min(self.keys.len());
        &self.keys[..n]
    }

    /// Key `i` of the allocated block. Where the original reads past its block (it has no
    /// bounds), the port reads a zero key; the routines that can do so say when.
    pub(crate) fn key(&self, i: usize) -> SplineKey {
        self.keys.get(i).copied().unwrap_or_else(SplineKey::zero)
    }

    /// A spline of the given type from key positions, with room allocated as the original
    /// allocates it. Handles (`prev`, `next`) are set to the position, as
    /// `SI_splineAddPoint` leaves a new key.
    pub fn from_points(kind: i32, closed: bool, points: &[[f32; 3]]) -> Curve {
        let mut c = Curve::new();
        c.rec.kind = kind;
        c.rec.closed = closed as i16;
        c.keys = edit::key_allocate(points.len() as i32);
        c.rec.keys = if c.keys.is_empty() { Ptr32::NULL } else { held() };
        c.rec.nb_keys = points.len() as u16 as i16;
        for (k, p) in c.keys.iter_mut().zip(points) {
            let v = Vector { x: p[0], y: p[1], z: p[2] };
            (k.prev, k.pos, k.next) = (v, v, v);
        }
        c
    }

    /// The spline a model file holds (`si-hrc`, finding 0004), as the reader
    /// (`splineBinaryRead`, 0x083efe…) leaves it in memory: the record initialised by
    /// `SI_splineInit`, the file's fields over it, the keys in a block from
    /// `SI_splineKeyAllocate`, and then the reader's three last steps:
    ///
    /// 1. a Bézier spline stored in the old layout (control points only, three to a
    ///    segment) is replaced by [`convert::bezier_shrink`] of it. The replacement is a
    ///    newly allocated spline, so **the name read from the file is lost** and the spline
    ///    is called `spl<n>` from the allocator's counter;
    /// 2. the matrix is computed ([`basis::update_matrix`]);
    /// 3. when the phantom word is set, [`edit::all_dummy`] makes sure a B-spline or
    ///    cardinal spline has its three keys.
    ///
    /// `None` where the original breaks down in step 1 (a closed spline of one or two
    /// control points).
    pub fn from_file(s: &si_hrc::Spline, counter: &mut i32, host: &mut dyn Host) -> Option<Curve> {
        let mut c = Curve::new();
        c.name = if s.name.is_empty() { None } else { Some(s.name.clone()) };
        c.rec.name = if c.name.is_some() { held() } else { Ptr32::NULL };
        c.rec.phantom = s.phantom;
        c.rec.closed = s.close;
        c.rec.kind = s.kind;
        c.rec.tension = s.tension;
        c.rec.step = s.step;
        c.keys = edit::key_allocate(s.keys.len() as i32);
        c.rec.keys = if c.keys.is_empty() { Ptr32::NULL } else { held() };
        c.rec.nb_keys = s.keys.len() as u16 as i16;
        for (k, f) in c.keys.iter_mut().zip(&s.keys) {
            k.prev = Vector { x: f.prev[0], y: f.prev[1], z: f.prev[2] };
            k.pos = Vector { x: f.pos[0], y: f.pos[1], z: f.pos[2] };
            k.next = Vector { x: f.next[0], y: f.next[1], z: f.next[2] };
            k.constraint = f.constraint;
            k.segment = f.segment;
            k.flag = f.flag;
        }
        if s.old_points && c.rec.kind == BEZIER {
            c = convert::bezier_shrink(&c, counter)?;
        }
        basis::update_matrix(&mut c);
        if c.rec.phantom != 0 {
            edit::all_dummy(&mut c, host);
        }
        Some(c)
    }
}

/// What the spline routines call in parts of the program that are not ported here: the
/// scene's list of objects, the function curve that drives an object along a path (finding
/// 0017, section 9a) and the index table of the modelling tools. The defaults do nothing,
/// which is the original's behaviour for a spline nothing else refers to.
pub trait Host {
    /// The walk over the current list of objects in `SI_splineTouch`: every model node
    /// (graf kinds 2 and 3) of kind 6 whose definition is this spline gets bit 0x10 set in
    /// both update flag bytes (+0x0a, +0x3fa); every node whose spline deformation record
    /// (model + 0x60 for the node, + 0x64 for the branch) follows a path model (record + 4)
    /// whose definition is this spline has the record's table (+8) freed and cleared.
    fn touched(&mut self, _c: &Curve) {}
    /// `SI_gblIndex` (the pointer at 0x0878896c) is not null: the modelling
    /// tools want to know where each key went.
    fn has_index(&self) -> bool {
        false
    }
    /// `SI_gblIndex[at] = value`.
    fn set_index(&mut self, _at: i32, _value: i32) {}
    /// `SI_gblIndex2` (the pointer at 0x08788970) is not null: a second table of the
    /// modelling tools, for the keys of the second spline of a merge.
    fn has_index2(&self) -> bool {
        false
    }
    /// `SI_gblIndex2[at]`.
    fn index2(&self, _at: i32) -> i32 {
        0
    }
    /// `SI_gblIndex2[at] = value`.
    fn set_index2(&mut self, _at: i32, _value: i32) {}
    /// `SI_findPathFcurveOwner(spline)`: the percentage curve of the first object that
    /// follows this spline, as a handle (0: none).
    fn find_path_fcurve_owner(&mut self, _c: &Curve) -> u32 {
        0
    }
    /// `SI_updateFcurvePathKeys(curve, spline)`.
    fn update_fcurve_path_keys(&mut self, _fcurve: u32, _c: &mut Curve) {}
    /// `SI_checkSplineToFcurve(curve, spline)`.
    fn check_spline_to_fcurve(&mut self, _fcurve: u32, _c: &mut Curve) {}
    /// `SI_refreshFcurve(0)`.
    fn refresh_fcurve(&mut self, _arg: i32) {}
    /// `SI_addFcurvePathKey(curve, point)`.
    fn add_fcurve_path_key(&mut self, _fcurve: u32, _point: i32) {}
    /// `SI_deleteFcurvePathKey(curve, point)`.
    fn delete_fcurve_path_key(&mut self, _fcurve: u32, _point: i16) {}
    /// `SI_invertFcurvePathKeys(curve, spline)`.
    fn invert_fcurve_path_keys(&mut self, _fcurve: u32, _c: &mut Curve) {}
}

/// A host with nothing in it.
pub struct NoHost;
impl Host for NoHost {}

/// `si-math`'s vector from `si-types`' and back: the same three floats.
pub(crate) fn mv(v: Vector) -> si_math::types::Vector {
    si_math::types::Vector::new(v.x, v.y, v.z)
}
