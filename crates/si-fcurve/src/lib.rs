// SPDX-License-Identifier: MIT
//! Function curves ("fcurves"): the animation curves every animated parameter of
//! Softimage|3D runs on (finding 0009).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `fcurve.c`, `fcv_fct.c`, `fkey.c`, `fkey_fct.c`, and
//! one function of `fcurveEdit.c` that the others call (`SI_fcurveSortKeys`).
//!
//! - this file: the curve and key records, creation, disposal, snapshot (`fcv_fct.c`,
//!   `fkey_fct.c`);
//! - [`eval`]: what value a curve has at a time;
//! - [`edit`]: key insertion, removal and modification, and the slopes;
//! - [`fkey`]: single keys and the two key clipboards (`fkey.c`);
//! - [`editor`]: tags, locks and key types set from the curve editor and the menus;
//! - [`update`]: the scene refresh after an edit (the glue at the end of `fcv_fct.c`);
//! - [`x87`]: the 80-bit arithmetic the original computes with.
//!
//! Times are seconds, stored as `float`; a frame number is a time multiplied by the frame
//! rate. The original takes `fcurve *` and tests it for NULL in most functions; here a curve
//! is a `&Fcurve`, and where the NULL case has a result of its own the doc comment says so.

pub mod edit;
pub mod eval;
pub mod fkey;
pub mod update;
pub mod editor;
pub mod x87;

pub use eval::Undefined;
pub use x87::X87;

/// Interpolation kinds, of a key (the segment that follows it) and of a curve (the kind its
/// new keys get). Names from the SDK (`SAA_FcurveInterpType`); the evaluator treats 2, 3 and
/// 4 alike, they differ in how the slopes are derived.
pub mod interp {
    pub const CONSTANT: i16 = 0;
    pub const LINEAR: i16 = 1;
    pub const HERMITE: i16 = 2;
    pub const SLOPE0: i16 = 3;
    pub const PLATEAU: i16 = 4;
}

/// Extrapolation kinds (`SAA_FcurveExtrapType`).
pub mod extrap {
    pub const CONSTANT: i16 = 0;
    pub const GRADIENT: i16 = 1;
    pub const CYCLE: i16 = 2;
}

/// Bits of [`Key::slope_type`] (`SAA_FKeySlopeType`).
pub mod slope {
    /// The slopes were set by hand: the automatic computation leaves them alone.
    pub const USER: i16 = 1;
    /// Left and right slope differ (a broken tangent).
    pub const DOUBLE: i16 = 2;
}

/// [`Key::flags`]: the key type in the low four bits (`SAA_FKeyType`), and flag bits above.
pub mod keyflag {
    pub const TYPE_MASK: u16 = 0x0f;
    pub const FREE_FORM: u16 = 0;
    /// The curve editor calls these keys locked.
    pub const KEY_FRAME: u16 = 1;
    pub const KEY_PATH: u16 = 2;
    pub const RAW: u16 = 3;
    /// "Struct key" (`si_fcvCheckStructKey`).
    pub const STRUCT: u16 = 0x10;
    /// Set on every key initialised while `SI_keyTypePropagMode` is on.
    pub const PROPAG: u16 = 0x20;
    /// Preserved by every operation that rewrites the type; never set or tested in these
    /// source files.
    pub const BIT_40: u16 = 0x40;
    /// What locking a key leaves of its flags: type 1, the three flag bits kept.
    pub const fn locked(f: u16) -> u16 {
        f & 0x70 | 1
    }
}

/// Curve types (`SI_FcurveType`) that these source files single out. The numbers are read
/// from the code; the names are those of the vendor's `Relations.rsrc`.
pub mod kind {
    pub const SHAPE_WEIGHT: i32 = 154;
    pub const NODE_VISIBILITY: i32 = 175;
    pub const LIGHT_VISIBILITY: i32 = 176;
    pub const CAMERA_VISIBILITY: i32 = 177;
    pub const ALGO_TYPE: i32 = 178;
    pub const ROOT_CONSTRAINT: i32 = 182;
    pub const CLUSTER_WEIGHT: i32 = 184;
    pub const SUN_TIME: i32 = 189;
    pub const RELATION_ACTIVE: i32 = 196;
    pub const BRANCH_VISIBILITY: i32 = 273;
    pub const NODE_MATTE: i32 = 371;
    pub const BRANCH_MATTE: i32 = 372;
    pub const CAMERA_PP_ACTIVE: i32 = 378;
    /// A custom effect's parameter: counted in its own dirty counter.
    pub const CUSTOM: i32 = 566;
    pub const CAUSTIC_TRANSMITTER: i32 = 620;
    /// 621 to 624: caustic energy (three) and exponent. Negative values evaluate to 0.
    pub const ENERGY_FIRST: i32 = 621;
    pub const ENERGY_LAST: i32 = 624;
    pub const CAUSTIC_PHOTONS: i32 = 625;
    pub const GI_PHOTONS: i32 = 626;
    pub const CAUSTIC: i32 = 627;
    pub const GLOBAL_ILLUMINATION: i32 = 628;
    pub const CAUSTIC_RECEIVER: i32 = 631;
    pub const GI_TRANSMITTER: i32 = 632;
    pub const GI_RECEIVER: i32 = 633;
    pub const WIREFRAME_DISPLAY: i32 = 676;
}

/// One key: the original's 36-byte `SI_Fcvkey`, without its two padding words and without
/// the pointer at +0x10 to the curve whose array holds the record (here a key in a curve's
/// array belongs to that curve, a key on its own to none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    /// +0x00, seconds.
    pub time: f32,
    /// +0x04
    pub value: f32,
    /// +0x08, value per second, of the segment that starts at this key.
    pub right_slope: f32,
    /// +0x0c, bits of [`slope`].
    pub slope_type: i16,
    /// +0x0e, [`interp`] of the segment that starts at this key.
    pub interp: i16,
    /// +0x14, the editor's selection mark ("tagged" in the SDK).
    pub tagged: i16,
    /// +0x18, value per second, of the segment that ends at this key.
    pub left_slope: f32,
    /// +0x1c, see [`keyflag`].
    pub flags: u16,
    /// +0x1e, "spline key index for keypaths" (name from `Relations.rsrc`); −1 if none.
    pub spline_index: i16,
    /// +0x20, initialised to −1, copied with the key; not used in these source files.
    pub field_20: i16,
}

impl Key {
    /// A fresh key [soft 0x0820bcc4 si_fkeyNew] [soft 0x0820bc50 SI_fkeyInitialize].
    ///
    /// The original's initialiser does not touch the interpolation word, so a new key carries
    /// whatever `malloc` returned there until its creator sets it; here it is 0.
    pub fn new() -> Key {
        let mut k = Key { time: 0.0, value: 0.0, right_slope: 0.0, slope_type: 0, interp: 0, tagged: 0, left_slope: 0.0,
                          flags: 0, spline_index: 0, field_20: 0 };
        k.initialize();
        k
    }

    /// [soft 0x0820bc50 SI_fkeyInitialize]: every field but `interp`.
    pub fn initialize(&mut self) {
        self.time = 0.0;
        self.value = 0.0;
        self.right_slope = 0.0;
        self.left_slope = 0.0;
        self.slope_type = 0;
        self.tagged = 0;
        self.flags = 0;
        self.spline_index = -1;
        self.field_20 = -1;
    }

    /// A copy on its own [soft 0x0820bc9c SI_fkeyCopy] [soft 0x0820bcf0 fkeyTransfer]: every
    /// field; the copy belongs to no curve.
    pub fn copy(&self) -> Key {
        *self
    }

    pub fn key_type(&self) -> u16 {
        self.flags & keyflag::TYPE_MASK
    }
}

impl Default for Key {
    fn default() -> Self {
        Key::new()
    }
}

/// How many key records the original allocates for `n` keys: `n` rounded up to a multiple
/// of 8 (the expression `n - (((n - 1) & 7) - 7)` in every allocation).
pub fn capacity(n: i32) -> i32 {
    n.wrapping_sub(((n.wrapping_sub(1)) & 7) - 7)
}

/// A function curve: the original's 100-byte `fcurve`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fcurve {
    /// +0x00, the parameter this curve animates (`SI_FcurveType`, see [`kind`]).
    pub kind: i32,
    /// +0x04, the object the curve belongs to (a pointer in the original; opaque here).
    pub owner: u32,
    /// +0x08 and +0x0c: the key array and its count. `None` is a NULL array; an array can
    /// also be present and empty (after its last key was removed one by one), and several
    /// functions tell the two apart.
    pub keys: Option<Vec<Key>>,
    /// +0x10, the time of the last evaluation, after clamping (−100000 at first).
    pub last_time: f32,
    /// +0x14, the value of the last evaluation.
    pub last_value: f32,
    /// +0x18, [`extrap`] before the first key.
    pub pre_extrap: i16,
    /// +0x1a, [`extrap`] after the last key.
    pub post_extrap: i16,
    /// +0x1c, a word the ASCII file format calls FLAGS; not interpreted in these files.
    pub flags: i16,
    /// +0x1e, [`interp`] of the curve.
    pub interp: i16,
    /// +0x20, the activation flag: an inactive curve evaluates at time 0.
    pub active: i16,
    /// +0x24, +0x28: when they differ by more than 1e-5, evaluation clamps the time to
    /// this interval.
    pub range_start: f32,
    pub range_end: f32,
    /// +0x2c, written together with `active` by the attribute setter; not read here.
    pub active_copy: i16,
    /// +0x2e, the track name, a C string (34 bytes up to the next field).
    pub name: [u8; 34],
    /// +0x50, index of the key the last evaluation or insertion ended on: where the next
    /// search starts.
    pub cache: i32,
    /// +0x54, the snapshot taken by [`Fcurve::snapshot`].
    pub aux: Option<Box<Fcurve>>,
    /// +0x58, +0x5c: least and greatest key value (see [`Fcurve::min_max_value`]).
    pub min_value: f32,
    pub max_value: f32,
    /// +0x60, set and cleared by the parameter get/set functions of other files.
    pub field_60: i16,
    /// +0x62, "changed since the custom-value callbacks last ran".
    pub dirty: i16,
}

/// What these source files call in the rest of the program. Every method has a default that
/// does nothing, so a host implements only what it has.
pub trait Host {
    /// `SI_fastSetDirty(NULL, -1, 1)`: the fast-playback cache is stale.
    fn fast_set_dirty(&mut self) {}
    /// `SI_statusDisplayMessage(text, kind)`.
    fn status_message(&mut self, _text: &str, _kind: i16) {}
    /// `SI_dialogRequest2("Do you want to replace the locked key?", "", "", "No", "Yes")`:
    /// non-zero to replace.
    fn replace_locked_key(&mut self) -> i16 {
        0
    }
    /// Text the original writes to standard output with `printf`.
    fn printf(&mut self, _text: &str) {}
    /// `si_fcvScnglDelete(curve)`: the curve leaves the curve editor's list.
    fn scngl_delete(&mut self, _curve: &Fcurve) {}
    /// `SI_fcurveSaveHistory(SI_edGetElemFromAnimFile(curve, …))`: an undo record.
    fn save_history(&mut self, _curve: &Fcurve) {}
}

/// The curves of one scene element (a model, a camera, a light), as the element dictionary
/// hands them out. The element dictionary belongs to other source files; the group and
/// attribute numbers are passed on as the original passes them.
pub trait ElementCurves {
    /// `SI_edFindFcv(elem, group, id)`.
    fn find(&mut self, group: i32, id: i32) -> Option<&mut Fcurve>;
    /// `SI_edFindFcv(elem, group, id)` and then `SI_edFindNextFcv` with the same arguments
    /// until it returns NULL: `f` is called for each curve in turn.
    fn each(&mut self, group: i32, id: i32, f: &mut dyn FnMut(&mut Fcurve));
    /// `SI_edFindRangeFcv(elem, group, first, last)` and then `SI_edFindRangeNextFcv` with the
    /// same arguments until it returns NULL.
    fn each_in_range(&mut self, group: i32, first: i32, last: i32, f: &mut dyn FnMut(&mut Fcurve));
    /// Whether the object has its path number `which`: the pointer at +0x2c8 of a model, at
    /// +0xa0 and +0xa4 of a camera, at +0x144 and +0x148 of a light.
    fn has_path(&self, _which: usize) -> bool {
        false
    }
    /// `si_keySplineToFcurve(SI_edFindFcv(elem, 1, id), path->spline)` for path `which`.
    fn key_spline_to_fcurve(&mut self, _id: i32, _which: usize) {}
    /// `SI_checkSplineToFcurve(SI_edFindFcv(elem, 1, id), path->spline)` for path `which`.
    fn check_spline_to_fcurve(&mut self, _id: i32, _which: usize) {}
}

/// A host that does nothing.
pub struct NoHost;
impl Host for NoHost {}

/// Number of slots of the per-type key clipboard (`fkeyTimeBuffer`, one per curve type).
pub const TIME_BUFFER_SLOTS: usize = 0x2a7;

/// The global variables these source files read and write, and the host.
pub struct Env {
    /// `SI_frameRate` (0x0878204c), frames per second.
    pub frame_rate: f32,
    /// `SI_frameCurrent` (0x08782040).
    pub frame_current: i32,
    /// `SI_glbAnim` + 0x18 (0x08781498): how close, in frames, two key times must be to
    /// count as the same key.
    pub key_tolerance: f32,
    /// `SI_glbAnim` + 0x30 (0x087814b0): the interpolation new curves get.
    pub default_interp: i16,
    /// `SI_keyTypePropagMode` (0x08695e8e).
    pub key_type_propag_mode: i16,
    /// `SI_useHiddenForMatte` (0x0867967c).
    pub use_hidden_for_matte: i16,
    /// `SI_cusValNumCallback` (0x086e49e8): while positive, changed curves are counted.
    pub cus_val_num_callback: i32,
    /// `SI_fcurveDirty` (0x086e49dc) and `SI_cusFcurveDirty` (0x086e49e0).
    pub fcurve_dirty: i32,
    pub cus_fcurve_dirty: i32,
    /// `SI_standaloneRenderer`: non-zero in a batch render, which has no curve editor.
    pub standalone_renderer: i32,
    /// `SI_updateOption` (0x0878fb24): non-zero to update edited elements one by one.
    pub update_option: i32,
    /// `SI_refreshElemFcvOption` (0x08695e8c): non-zero to refresh the scene after curve edits.
    pub refresh_elem_fcv_option: i32,
    /// `fkeyBuffer` and `fkeyBufferSize` (0x08695e80): the key clipboard of the curve editor.
    pub fkey_buffer: Option<Vec<Key>>,
    /// `fkeyTimeBuffer` (0x08789b20): one key per curve type; `None` where the original's
    /// record has a NULL curve pointer.
    pub fkey_time_buffer: Vec<Option<Key>>,
    pub host: Box<dyn Host>,
}

impl Env {
    /// The values the program starts with: tolerance 0.15 frame and Hermite interpolation
    /// (stored by `SI_animationInfoInit`, 0x080d335c), 30 frames per second (stored by
    /// `SI_initPlayControl`, 0x083d1b40, when nothing else sets a rate), everything else zero.
    pub fn new() -> Env {
        Env {
            frame_rate: 30.0,
            frame_current: 0,
            key_tolerance: 0.15,
            default_interp: interp::HERMITE,
            key_type_propag_mode: 0,
            use_hidden_for_matte: 0,
            cus_val_num_callback: 0,
            fcurve_dirty: 0,
            cus_fcurve_dirty: 0,
            standalone_renderer: 0,
            update_option: 0,
            refresh_elem_fcv_option: 0,
            fkey_buffer: None,
            fkey_time_buffer: vec![None; TIME_BUFFER_SLOTS],
            host: Box::new(NoHost),
        }
    }
}

impl Default for Env {
    fn default() -> Self {
        Env::new()
    }
}

impl Fcurve {
    /// [soft 0x0820a9d0 SI_fcurveInitialize]: every field but the type.
    pub fn initialize(&mut self) {
        self.range_end = 0.0;
        self.range_start = 0.0;
        self.last_time = -100000.0;
        self.last_value = 0.0;
        self.min_value = 0.0;
        self.max_value = 0.0;
        self.owner = 0;
        self.keys = None;
        self.pre_extrap = extrap::CONSTANT;
        self.post_extrap = extrap::CONSTANT;
        self.interp = interp::HERMITE;
        self.flags = 0;
        self.active = 1;
        self.active_copy = 1;
        self.cache = 0;
        self.aux = None;
        self.field_60 = 0;
        self.name[0] = 0;
        self.dirty = 0;
    }

    /// A new curve [soft 0x0820a810 SI_fcurveAllocate].
    ///
    /// Its interpolation depends on the type: constant for the on/off and counting
    /// parameters, linear for the sun's time, slope-0 for shape and cluster weights, the
    /// user's default for everything else.
    pub fn allocate(kind: i32, owner: u32, env: &Env) -> Fcurve {
        let mut f = Fcurve { kind: 0, owner: 0, keys: None, last_time: 0.0, last_value: 0.0, pre_extrap: 0, post_extrap: 0,
                             flags: 0, interp: 0, active: 0, range_start: 0.0, range_end: 0.0, active_copy: 0,
                             name: [0; 34], cache: 0, aux: None, min_value: 0.0, max_value: 0.0, field_60: 0, dirty: 0 };
        f.initialize();
        f.interp = match kind {
            0xaf | 0x111 | 0xb1 | 0xb0 | 0x17a | 0xb2 | 0x173 | 0x174 | 0xb6 | 0xc4 | 0x26c | 0x277 | 0x278 | 0x279 | 0x273
            | 0x274 | 0x2a4 | 0x271 | 0x272 => interp::CONSTANT,
            0xbd => interp::LINEAR,
            0x9a | 0xb8 => interp::SLOPE0,
            _ => env.default_interp,
        };
        f.kind = kind;
        f.owner = owner;
        f
    }

    /// Number of keys (+0x0c).
    pub fn nkeys(&self) -> i32 {
        self.keys.as_ref().map_or(0, |k| k.len() as i32)
    }

    /// The keys; empty for a NULL array.
    pub fn k(&self) -> &[Key] {
        self.keys.as_deref().unwrap_or(&[])
    }

    /// The idiom that follows most changes: while custom-value callbacks are registered, a
    /// curve not yet marked is marked and counted, custom-effect curves in their own counter.
    pub(crate) fn mark_dirty(&mut self, env: &mut Env) {
        if env.cus_val_num_callback > 0 && self.dirty == 0 {
            self.dirty = 1;
            if self.kind == kind::CUSTOM {
                env.cus_fcurve_dirty += 1;
            } else {
                env.fcurve_dirty += 1;
            }
        }
    }

    /// Frees a curve and clears the pointer to it [soft 0x0820a928 SI_fcurveDispose]: a
    /// curve marked dirty leaves its counter, and its snapshot is disposed of the same way.
    pub fn dispose(slot: &mut Option<Box<Fcurve>>, env: &mut Env) {
        let Some(f) = slot.as_mut() else { return };
        if env.cus_val_num_callback > 0 && f.dirty & 1 != 0 {
            if f.kind == kind::CUSTOM {
                env.cus_fcurve_dirty -= 1;
            } else {
                env.fcurve_dirty -= 1;
            }
        }
        f.keys = None;
        if f.aux.is_some() {
            Fcurve::dispose(&mut f.aux, env);
        }
        *slot = None;
    }

    /// Disposes of a curve that the curve editor may be showing
    /// [soft 0x081ff0a0 SI_fcurveRemove].
    pub fn remove(slot: &mut Option<Box<Fcurve>>, env: &mut Env) {
        let Some(f) = slot.as_ref() else { return };
        if env.standalone_renderer == 0 {
            env.host.scngl_delete(f);
        }
        Fcurve::dispose(slot, env);
        env.host.fast_set_dirty();
    }

    /// Makes `self` a copy of `src` with its own key array and no snapshot
    /// [soft 0x081ff0e4 SI_fcurveTransfer]. `self` keeps its dirty mark and gets one if it
    /// had none. (The original overwrites the destination's array and snapshot pointers
    /// without freeing them; with `src` NULL it only signals the host.)
    pub fn transfer_from(&mut self, src: &Fcurve, env: &mut Env) {
        let dirty = self.dirty;
        *self = Fcurve { aux: None, ..src.clone() };
        // SI_malloc returns NULL for a size of zero
        if src.nkeys() == 0 {
            self.keys = None;
        }
        self.dirty = dirty;
        self.mark_dirty(env);
        env.host.fast_set_dirty();
    }

    /// Drops the snapshot [soft 0x0820ab28 SI_fcvFreeze].
    pub fn freeze(&mut self, env: &mut Env) {
        if self.aux.is_some() {
            Fcurve::dispose(&mut self.aux, env);
        }
    }

    /// Replaces the snapshot by a copy of the curve as it is now
    /// [soft 0x0820ab4c SI_fcvSnapshot]: type, owner, extrapolation, interpolation, name and
    /// keys; everything else as in a new curve.
    ///
    /// A curve without keys cannot be copied (the original's allocation of zero records
    /// yields NULL, which it takes for failure): the old snapshot then stays.
    pub fn snapshot(&mut self, env: &mut Env) {
        let mut s = Fcurve::allocate(self.kind, self.owner, env);
        s.pre_extrap = self.pre_extrap;
        s.post_extrap = self.post_extrap;
        s.interp = self.interp;
        if self.nkeys() == 0 {
            return;
        }
        // strcpy: up to and including the terminator
        let n = self.name.iter().position(|&b| b == 0).map_or(self.name.len(), |p| p + 1);
        s.name[..n].copy_from_slice(&self.name[..n]);
        s.keys = self.keys.clone();
        if self.aux.is_some() {
            Fcurve::dispose(&mut self.aux, env);
        }
        self.aux = Some(Box::new(s));
    }

    /// Exchanges the curve with its snapshot [soft 0x0820ac4c SI_fcvSwapWithAux]: all 100
    /// bytes, dirty marks and evaluation caches included; the snapshot stays attached to the
    /// curve and has none of its own.
    ///
    /// The original does not correct the keys' pointers to their curve after the exchange;
    /// that has no counterpart here.
    pub fn swap_with_aux(&mut self) {
        let Some(mut aux) = self.aux.take() else { return };
        std::mem::swap(self, &mut *aux);
        // what was the snapshot's own snapshot pointer is overwritten; the original loses it
        aux.aux = None;
        self.aux = Some(aux);
    }

    /// Radians to degrees, for the curve types that hold angles
    /// [soft 0x0820aa60 SI_fcvConvertAngleUnitToDegree]: value and both slopes of every key
    /// times 57.29577951308232, each product formed in the register and stored as `float`.
    pub fn convert_angle_unit_to_degree(&mut self) {
        if !matches!(self.kind, 0x19 | 0x1a | 0xb | 0xc | 0x2c | 0x48 | 0x5e | 0x5f | 0x60 | 0x6e | 0x75 | 0xa6 | 0xa7 | 0xa8
                                | 0x7c | 0x7d | 0x7e | 0x85 | 0x86 | 0x87) {
            return;
        }
        let k = X87::from_f64(57.29577951308232);
        for key in self.keys.iter_mut().flatten() {
            key.value = (k * X87::from_f32(key.value)).to_f32();
            key.left_slope = (k * X87::from_f32(key.left_slope)).to_f32();
            key.right_slope = (k * X87::from_f32(key.right_slope)).to_f32();
        }
    }
}

#[cfg(test)]
mod tests;
