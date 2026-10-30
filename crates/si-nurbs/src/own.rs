// SPDX-License-Identifier: MIT
//! The one module of this crate that owns what a record points to: a curve's knots and
//! points, a surface's two knot vectors, points, trim and projection lists; and with them
//! the original's allocation, copy and disposal routines of `nrb_b.c`.
//!
//! A record keeps its pointer fields as markers: null when the array is absent, any other
//! value when the array is held beside the record. Everything else in the crate works on
//! the views [`crate::CrvRef`] and [`crate::SrfRef`] that [`Curve::view`] and
//! [`Surface::view`] hand out, so that the storage can be exchanged for the shared memory
//! image (`si-mem`) here and nowhere else.
//!
//! Fields this module reads that `si-types` names differently (finding 0025 has the
//! evidence): in a curve, `length_data[0]` is the number of entries of the two length
//! tables, `length_data[1]` and `[2]` are their pointers (+0x54, +0x58), `unk_5c[0]` is a
//! pointer the disposal frees (+0x5c); in a surface, `lines0` (+0x80) is the **number** of
//! display lines and `lines1` (+0x84) the array of 20-byte line records, and `unk_c8[4]`
//! (+0xd8) is the root of the bounding-box tree.

use crate::{held, CrvRef, NurbsCrv, NurbsDir, NurbsPoint, NurbsSrf, SrfRef};
use si_types::{Double, Opaque, Ptr32, Zero};

// ------------------------------------------------------------------------------ records

/// A composite curve (`SI_NurbsCompCrv`, 0x18 bytes): a list of curves that together form
/// one trim loop or one projected curve of a surface. Laid out here; `si-types` has only
/// the name.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompCrv {
    /// +0x00: the model file's "boundary" token (finding 0004). Copied by
    /// `SI_nurbsSrfTrmCpy` only when the composite has curves.
    pub boundary: i16,
    pub pad_02: [u8; 2],
    /// +0x04: number of curves.
    pub nb_curves: i32,
    /// +0x08: array of `nb_curves` pointers to curves.
    pub curves: Ptr32<Ptr32<NurbsCrv>>,
    /// +0x0c: the trim line computed from the curves (`SI_trimLine`, the trimming package).
    pub trim_line: Ptr32<Opaque>,
    /// +0x10: cleared by every routine that makes a composite; not read in these files.
    pub unk_10: u32,
    /// +0x14: a block the disposal frees; cleared by every routine that makes a composite.
    pub block_14: Ptr32<Opaque>,
}

const _: () = {
    assert!(core::mem::size_of::<CompCrv>() == 0x18);
    assert!(core::mem::offset_of!(CompCrv, nb_curves) == 0x04);
    assert!(core::mem::offset_of!(CompCrv, curves) == 0x08);
    assert!(core::mem::offset_of!(CompCrv, trim_line) == 0x0c);
    assert!(core::mem::offset_of!(CompCrv, unk_10) == 0x10);
    assert!(core::mem::offset_of!(CompCrv, block_14) == 0x14);
};

/// Offsets of the fields named differently in `si-types`, for the finding and for the day
/// the records are corrected there.
pub mod off {
    /// Curve: number of entries of the two length tables.
    pub const CRV_NB_LENGTH: usize = 0x50;
    /// Curve: length table, parameters (doubles).
    pub const CRV_LENGTH_PARAM: usize = 0x54;
    /// Curve: length table, cumulated lengths (doubles).
    pub const CRV_LENGTH_DIST: usize = 0x58;
    /// Curve: a block freed by `SI_nurbsCrvDispose`.
    pub const CRV_BLOCK_5C: usize = 0x5c;
    /// Surface: number of display lines.
    pub const SRF_NB_LINES: usize = 0x80;
    /// Surface: the display lines, 20 bytes each.
    pub const SRF_LINES: usize = 0x84;
    /// Surface: root of the bounding-box tree (`GenTree`), built by the intersection code.
    pub const SRF_BBOX_TREE: usize = 0xd8;
}

// ------------------------------------------------------------------------------ owners

/// A curve record with what it points to.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub rec: NurbsCrv,
    /// +0x0c
    pub knots: Option<Vec<f64>>,
    /// +0x2c
    pub points: Option<Vec<NurbsPoint>>,
    /// +0x54: `nb_length` doubles.
    pub length_param: Option<Vec<f64>>,
    /// +0x58: `nb_length` doubles.
    pub length_dist: Option<Vec<f64>>,
}

/// The part of a trim line (`SI_trimLine`, a record of the trimming package) that
/// `nrb_b.c` reads and writes: four doubles and a singly linked list of nodes that begin
/// with two doubles (a point in the surface's parameter plane, by the exchange
/// [`srf_swap_uv`] makes; the head of the list is at +0x08, a node's link at +0x14).
#[derive(Clone, Debug, PartialEq)]
pub struct TrimLine {
    /// +0x10, +0x18, +0x20, +0x28
    pub box4: [f64; 4],
    pub nodes: Vec<[f64; 2]>,
}

/// A composite curve with its curves.
#[derive(Clone, Debug, PartialEq)]
pub struct CompCurve {
    pub rec: CompCrv,
    /// +0x08: `None` when the pointer is null.
    pub curves: Option<Vec<Option<Box<Curve>>>>,
    /// +0x0c
    pub trim_line: Option<Box<TrimLine>>,
}

/// A surface record with what it points to.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub rec: NurbsSrf,
    /// +0x0c
    pub knots_u: Option<Vec<f64>>,
    /// +0x30
    pub knots_v: Option<Vec<f64>>,
    /// +0x54: `rec.nb_points` control points, V major.
    pub points: Option<Vec<NurbsPoint>>,
    /// +0x64: `rec.nb_trims` composites.
    pub trims: Option<Vec<Option<Box<CompCurve>>>>,
    /// +0x6c: `rec.nb_projected` composites.
    pub projected: Option<Vec<Option<Box<CompCurve>>>>,
    /// +0x84: the display lines, five words each (the drawing package's; only moved here).
    pub lines: Option<Vec<[u32; 5]>>,
}

/// What the routines of this module call outside the four source files. An emulation check
/// records these calls; a program supplies the real routines.
pub trait Host {
    /// `SI_nurbsSrfDisposeLines` (0x084e5468, `nrb_draw.c`): frees
    /// the display lines of a surface.
    fn srf_dispose_lines(&mut self, _srf: &mut Surface) {}
    /// `SI_genTreeDestroy(&srf->tree, SI_bezPatchDestroy)`: frees the bounding-box tree.
    fn gen_tree_destroy(&mut self, _srf: &mut Surface) {}
    /// `SI_trmCvrRve`: the trimming package's reversal of a trim line, called for every trim
    /// with a trim line of a surface whose U and V are exchanged, after the exchange of the
    /// line's coordinates.
    fn trm_cvr_rve(&mut self, _trim: usize, _line: &mut TrimLine) {}
    /// `SI_gblIndex` (0x0878896c): when not null, the table of integers in which the
    /// reordering routines record where each point went.
    fn gbl_index(&mut self) -> Option<&mut Vec<i32>> {
        None
    }
    /// `SI_findNURBSPathFcurveOwner(curve)` then `SI_invertFcurvePathKeys(owner, 0, curve)`,
    /// for a keyframed curve that is reversed (the animation package's).
    fn invert_path_keys(&mut self) {}
}

/// A host that does nothing.
pub struct NoHost;
impl Host for NoHost {}

/// What a disposal freed, in the original's order, by name: `knots`, `pts`, `b5c` (the
/// block at +0x5c), `lp` and `ld` (the length tables), `crv` for a curve; `curves` (the
/// pointer array), `tn0`, `tn1`, … (trim line nodes), `tl`, `b14`, `comp` for a composite,
/// its curves' blocks prefixed `c0.`, `c1.`, …; `ku`, `kv`, `pts`, `trims`, `projected`
/// (the two pointer arrays), `srf` for a surface, its composites' blocks prefixed `t0.`,
/// `p0.`, ….
pub type Freed = Vec<String>;

// ------------------------------------------------------------------------------ allocators

/// [soft 0x084dc47c SI_nurbsPointsAllocate]: `n` control points (0, 0, 0, 1) with flag 0;
/// nothing for a count of zero or less (`SI_malloc` of zero bytes, or of a negative size
/// taken as unsigned, returns null).
pub fn points_allocate(n: i32) -> Option<Vec<NurbsPoint>> {
    if n <= 0 || (n as i64) * 36 > i32::MAX as i64 {
        return None;
    }
    let mut p = NurbsPoint::zero();
    p.w = Double(1.0);
    Some(vec![p; n as usize])
}

/// [soft 0x084dc500 si_knotsAllocate]: `n` doubles, cleared (`SI_calloc`: null for a count
/// of zero; a negative count fails in the C library).
pub fn knots_allocate(n: i32) -> Option<Vec<f64>> {
    if n <= 0 || (n as i64) * 8 > i32::MAX as i64 {
        return None;
    }
    Some(vec![0.0; n as usize])
}

impl Curve {
    /// A new curve [soft 0x084db5a0 SI_nurbsCrvAllocate] (`si-types` has the record's
    /// defaults: order 4, step 10, closing 1; the three padding words are heap content in
    /// the original and zero here).
    pub fn allocate() -> Curve {
        Curve { rec: NurbsCrv::allocate(), knots: None, points: None, length_param: None, length_dist: None }
    }

    /// A curve around a record and arrays, with the record's pointer fields made to agree.
    pub fn from_parts(rec: NurbsCrv, knots: Option<Vec<f64>>, points: Option<Vec<NurbsPoint>>) -> Curve {
        let mut c = Curve { rec, knots, points, length_param: None, length_dist: None };
        c.sync();
        c
    }

    /// Makes the record's pointer fields say which arrays are held.
    pub fn sync(&mut self) {
        self.rec.dir.knots = mark(self.knots.is_some());
        self.rec.points = mark(self.points.is_some());
        self.rec.length_data[1] = mark::<Opaque>(self.length_param.is_some()).addr();
        self.rec.length_data[2] = mark::<Opaque>(self.length_dist.is_some()).addr();
    }

    /// Makes the knot and point pointers of the record say which of the two arrays are held
    /// (after a routine replaced them), leaving the other pointer fields alone.
    pub fn sync_arrays(&mut self) {
        self.rec.dir.knots = mark(self.knots.is_some());
        self.rec.points = mark(self.points.is_some());
    }

    /// The numerical routines' view. Absent arrays read as empty.
    pub fn view(&self) -> CrvRef<'_> {
        CrvRef { rec: &self.rec, knots: self.knots.as_deref().unwrap_or(&[]), points: self.points.as_deref().unwrap_or(&[]) }
    }

    /// +0x50
    pub fn nb_length(&self) -> i32 {
        self.rec.length_data[0] as i32
    }
}

fn mark<T: ?Sized>(some: bool) -> Ptr32<T> {
    if some {
        held()
    } else {
        Ptr32::NULL
    }
}

impl Surface {
    /// A new surface [soft 0x084db66c SI_nurbsSrfAllocate] (`si-types` has the defaults).
    pub fn allocate() -> Surface {
        Surface { rec: NurbsSrf::allocate(), knots_u: None, knots_v: None, points: None, trims: None, projected: None, lines: None }
    }

    pub fn from_parts(rec: NurbsSrf, knots_u: Option<Vec<f64>>, knots_v: Option<Vec<f64>>, points: Option<Vec<NurbsPoint>>) -> Surface {
        let mut s = Surface { rec, knots_u, knots_v, points, trims: None, projected: None, lines: None };
        s.sync();
        s
    }

    pub fn sync(&mut self) {
        self.rec.u.knots = mark(self.knots_u.is_some());
        self.rec.v.knots = mark(self.knots_v.is_some());
        self.rec.points = mark(self.points.is_some());
        self.rec.trims = mark(self.trims.is_some());
        self.rec.projected = mark(self.projected.is_some());
        self.rec.lines1 = mark(self.lines.is_some());
    }

    pub fn view(&self) -> SrfRef<'_> {
        SrfRef {
            rec: &self.rec,
            knots_u: self.knots_u.as_deref().unwrap_or(&[]),
            knots_v: self.knots_v.as_deref().unwrap_or(&[]),
            points: self.points.as_deref().unwrap_or(&[]),
        }
    }

    /// +0x80: the number of display lines.
    pub fn nb_lines(&self) -> i32 {
        self.rec.lines0.addr() as i32
    }
}

// ------------------------------------------------------------------------------ curves

/// [soft 0x084dbbf4 SI_nurbsCrvCopy]: a new curve with the source's shape, parameters,
/// knots, points and length tables. Not copied: the word at +0x48 (cleared), the block at
/// +0x5c and the word at +0x60 (as the allocator left them: null and zero).
///
/// The knots are copied only when their count is above zero and the points only when the
/// **source's** point count is; the original then copies as many as the count says, reading
/// past a shorter source array. `None` where it would: the port does not invent the bytes.
pub fn crv_copy(src: &Curve) -> Option<Curve> {
    let mut c = Curve::allocate();
    let s = &src.rec;
    c.rec.dir.order = s.dir.order;
    c.rec.dir.nb_points = s.dir.nb_points;
    c.rec.dir.nb_knots = s.dir.nb_knots;
    if s.dir.nb_knots > 0 {
        c.knots = knots_allocate(s.dir.nb_knots);
        copy_knots(c.knots.as_mut()?, src.knots.as_deref()?, s.dir.nb_knots)?;
    }
    c.rec.dir.range_start = s.dir.range_start;
    c.rec.dir.range_end = s.dir.range_end;
    c.rec.dir.closed = s.dir.closed;
    c.rec.unk_28 = s.unk_28;
    c.rec.step = s.step;
    c.rec.closing = s.closing;
    c.rec.param = s.param;
    c.rec.unk_44 = s.unk_44;
    c.rec.flags_38 = s.flags_38;
    c.rec.unk_3c = s.unk_3c;
    c.rec.keyframed = s.keyframed;
    if s.dir.nb_points > 0 {
        c.points = points_allocate(c.rec.dir.nb_points);
        copy_points(c.points.as_mut()?, src.points.as_deref()?, c.rec.dir.nb_points)?;
    }
    c.rec.unk_48 = 0;
    c.rec.unk_4e = s.unk_4e;
    if src.nb_length() > 0 {
        copy_length(&mut c, src)?;
    }
    c.sync();
    Some(c)
}

/// [soft 0x084dbd80 SI_nurbsCrvInit]: the same into an existing record. What `dst` held is
/// not freed (its arrays are simply replaced: the original leaks them); `closing` (+0x30),
/// +0x48, +0x5c and +0x60 keep `dst`'s values; `dst`'s points are left alone when the source
/// has none; the length tables are cleared when the source has none.
pub fn crv_init(dst: &mut Curve, src: &Curve) -> Option<()> {
    let s = &src.rec;
    dst.rec.dir.order = s.dir.order;
    dst.rec.dir.nb_points = s.dir.nb_points;
    dst.rec.dir.range_start = s.dir.range_start;
    dst.rec.dir.range_end = s.dir.range_end;
    dst.rec.dir.closed = s.dir.closed;
    dst.rec.dir.nb_knots = s.dir.nb_knots;
    dst.rec.unk_28 = s.unk_28;
    dst.rec.step = s.step;
    dst.rec.param = s.param;
    dst.rec.unk_44 = s.unk_44;
    dst.rec.flags_38 = s.flags_38;
    dst.rec.unk_3c = s.unk_3c;
    dst.rec.keyframed = s.keyframed;
    dst.knots = knots_allocate(s.dir.nb_knots);
    if s.dir.nb_knots > 0 {
        copy_knots(dst.knots.as_mut()?, src.knots.as_deref()?, s.dir.nb_knots)?;
    }
    if s.dir.nb_points > 0 {
        dst.points = points_allocate(dst.rec.dir.nb_points);
        copy_points(dst.points.as_mut()?, src.points.as_deref()?, dst.rec.dir.nb_points)?;
    }
    dst.rec.unk_4e = s.unk_4e;
    if src.nb_length() < 1 {
        dst.rec.length_data[0] = 0;
        dst.length_dist = None;
        dst.length_param = None;
    } else {
        copy_length(dst, src)?;
    }
    dst.sync();
    Some(())
}

fn copy_knots(dst: &mut [f64], src: &[f64], n: i32) -> Option<()> {
    let n = n as usize;
    dst.get_mut(..n)?.copy_from_slice(src.get(..n)?);
    Some(())
}

fn copy_points(dst: &mut [NurbsPoint], src: &[NurbsPoint], n: i32) -> Option<()> {
    let n = n as usize;
    dst.get_mut(..n)?.copy_from_slice(src.get(..n)?);
    Some(())
}

fn copy_length(dst: &mut Curve, src: &Curve) -> Option<()> {
    let n = src.nb_length();
    dst.rec.length_data[0] = n as u32;
    let mut d = knots_allocate(n)?;
    let mut p = knots_allocate(n)?;
    copy_knots(&mut d, src.length_dist.as_deref()?, n)?;
    copy_knots(&mut p, src.length_param.as_deref()?, n)?;
    dst.length_dist = Some(d);
    dst.length_param = Some(p);
    Some(())
}

/// [soft 0x084db870 SI_nurbsCrvDispose]: frees knots, points, the block at +0x5c, then
/// (only when the length count is above zero) the two length tables, +0x54 before +0x58,
/// then the record, and clears the caller's pointer. Returns what was freed, in order.
pub fn crv_dispose(slot: &mut Option<Box<Curve>>) -> Freed {
    let mut f = Vec::new();
    let Some(c) = slot.take() else { return f };
    let mut push = |held: bool, name: &str| {
        if held {
            f.push(name.to_string());
        }
    };
    push(c.knots.is_some(), "knots");
    push(c.points.is_some(), "pts");
    push(c.rec.unk_5c[0] != 0, "b5c");
    if c.nb_length() > 0 {
        push(c.length_param.is_some(), "lp");
        push(c.length_dist.is_some(), "ld");
    }
    push(true, "crv");
    f
}

// ------------------------------------------------------------------------------ composites

/// A composite as `SI_malloc(0x18)` leaves it before anything is set: the first and the last
/// byte carry the allocator's marks (0x6f, 0x6b), the rest is heap content (zero here).
fn comp_raw() -> CompCrv {
    CompCrv {
        boundary: 0x6f,
        pad_02: [0; 2],
        nb_curves: 0,
        curves: Ptr32::NULL,
        trim_line: Ptr32::NULL,
        unk_10: 0,
        block_14: Ptr32::new(0x6b00_0000),
    }
}

/// [soft 0x084dc3fc SI_nurbsCompCopy]: a new composite with copies of the curves. **Not
/// set by the original**: the boundary flag (+0x00), the curve array when there is no
/// curve (+0x08) and the trim line (+0x0c); they hold what the heap held, with the
/// allocator's mark 0x6f in the first byte. The port leaves them zero but for that mark.
pub fn comp_copy(src: &CompCurve) -> Option<CompCurve> {
    let mut rec = comp_raw();
    rec.nb_curves = src.rec.nb_curves;
    let mut curves = None;
    if rec.nb_curves > 0 {
        let mut v = Vec::new();
        let from = src.curves.as_ref()?;
        for i in 0..rec.nb_curves as usize {
            v.push(Some(Box::new(crv_copy(from.get(i)?.as_deref()?)?)));
        }
        curves = Some(v);
    }
    rec.unk_10 = 0;
    rec.block_14 = Ptr32::NULL;
    rec.curves = mark(curves.is_some());
    Some(CompCurve { rec, curves, trim_line: None })
}

/// [soft 0x084ddef8 SI_nurbsCompFromCurve]: a composite of one curve, a copy of the given
/// one. The boundary flag and the trim line are not set (see [`comp_copy`]).
pub fn comp_from_curve(crv: &Curve) -> Option<CompCurve> {
    let mut rec = comp_raw();
    rec.nb_curves = 1;
    let curves = Some(vec![Some(Box::new(crv_copy(crv)?))]);
    rec.unk_10 = 0;
    rec.block_14 = Ptr32::NULL;
    rec.curves = held();
    Some(CompCurve { rec, curves, trim_line: None })
}

/// [soft 0x084dbb14 SI_nurbsCompDispose]: each curve, the curve array, the trim line (its
/// list of nodes, then the line), the block at +0x14, the record.
pub fn comp_dispose(slot: &mut Option<Box<CompCurve>>) -> Freed {
    let mut f = Vec::new();
    let Some(mut c) = slot.take() else { return f };
    if let Some(curves) = c.curves.as_mut() {
        for i in 0..c.rec.nb_curves.max(0) as usize {
            let Some(cv) = curves.get_mut(i) else { break };
            f.extend(crv_dispose(cv).into_iter().map(|n| format!("c{i}.{n}")));
        }
        f.push("curves".to_string());
    }
    if let Some(line) = c.trim_line.as_ref() {
        for n in 0..line.nodes.len() {
            f.push(format!("tn{n}"));
        }
        f.push("tl".to_string());
    }
    if !c.rec.block_14.is_null() {
        f.push("b14".to_string());
    }
    f.push("comp".to_string());
    f
}

// ------------------------------------------------------------------------------ surfaces

/// The common part of [`srf_trm_cpy`] and [`srf_proj_cpy`]: a new array of composites, each
/// with copies of the source's curves. Here the boundary flag is copied (only when the
/// composite has curves) and the three words from +0x0c are cleared.
fn comp_list_copy(src: &[Option<Box<CompCurve>>], n: i32) -> Option<Vec<Option<Box<CompCurve>>>> {
    let mut out = Vec::new();
    for i in 0..n as usize {
        let from = src.get(i)?.as_deref()?;
        let mut rec = comp_raw();
        rec.nb_curves = from.rec.nb_curves;
        let mut curves = None;
        if rec.nb_curves < 1 {
            rec.curves = Ptr32::NULL;
        } else {
            rec.boundary = from.rec.boundary;
            let mut v = Vec::new();
            let fc = from.curves.as_ref()?;
            for j in 0..rec.nb_curves as usize {
                v.push(Some(Box::new(crv_copy(fc.get(j)?.as_deref()?)?)));
            }
            curves = Some(v);
            rec.curves = held();
        }
        rec.trim_line = Ptr32::NULL;
        rec.unk_10 = 0;
        rec.block_14 = Ptr32::NULL;
        out.push(Some(Box::new(CompCurve { rec, curves, trim_line: None })));
    }
    Some(out)
}

/// [soft 0x084dbf08 SI_nurbsSrfTrmCpy]: gives `dst` copies of `src`'s trims. First the
/// display lines of `dst` are disposed of (the drawing package's routine); the border flag
/// (+0x5c) and the count are copied; with no trim the array pointer of `dst` is left as it
/// was.
pub fn srf_trm_cpy(host: &mut dyn Host, dst: &mut Surface, src: &Surface) -> Option<()> {
    host.srf_dispose_lines(dst);
    dst.rec.border = src.rec.border;
    dst.rec.nb_trims = src.rec.nb_trims;
    if dst.rec.nb_trims > 0 {
        dst.trims = Some(comp_list_copy(src.trims.as_deref()?, dst.rec.nb_trims)?);
        dst.rec.trims = held();
    }
    Some(())
}

/// [soft 0x084dc058 SI_nurbsSrfProjCpy]: the same for the projected curves (+0x68, +0x6c):
/// the border flag is copied here too, but the display lines are **not** disposed of.
pub fn srf_proj_cpy(dst: &mut Surface, src: &Surface) -> Option<()> {
    dst.rec.border = src.rec.border;
    dst.rec.nb_projected = src.rec.nb_projected;
    if dst.rec.nb_projected > 0 {
        dst.projected = Some(comp_list_copy(src.projected.as_deref()?, dst.rec.nb_projected)?);
        dst.rec.projected = held();
    }
    Some(())
}

/// [soft 0x084dc1a8 SI_nurbsSrfCopy]: a new surface with the source's two directions,
/// points, trims, projected curves, display and approximation parameters. Not copied: the
/// display lines, the word at +0x58 is copied but the border goes through the trim copy,
/// the two words at +0xb4, the doubles at +0xb8 and the words from +0xc8 (as allocated).
pub fn srf_copy(host: &mut dyn Host, src: &Surface) -> Option<Surface> {
    let mut d = Surface::allocate();
    let s = &src.rec;
    d.rec.u.order = s.u.order;
    d.rec.u.nb_points = s.u.nb_points;
    d.rec.unk_58 = s.unk_58;
    d.rec.u.nb_knots = s.u.nb_knots;
    if s.u.nb_knots > 0 {
        d.knots_u = knots_allocate(s.u.nb_knots);
        copy_knots(d.knots_u.as_mut()?, src.knots_u.as_deref()?, s.u.nb_knots)?;
    }
    d.rec.u.range_start = s.u.range_start;
    d.rec.u.range_end = s.u.range_end;
    d.rec.u.closed = s.u.closed;
    d.rec.v.order = s.v.order;
    d.rec.v.nb_points = s.v.nb_points;
    d.rec.v.nb_knots = s.v.nb_knots;
    if s.v.nb_knots > 0 {
        d.knots_v = knots_allocate(s.v.nb_knots);
        copy_knots(d.knots_v.as_mut()?, src.knots_v.as_deref()?, s.v.nb_knots)?;
    }
    d.rec.v.range_start = s.v.range_start;
    d.rec.v.range_end = s.v.range_end;
    d.rec.v.closed = s.v.closed;
    d.rec.unk_50 = s.unk_50;
    d.rec.param = s.param;
    d.rec.u_step = s.u_step;
    d.rec.v_step = s.v_step;
    d.rec.u_nb_curves = s.u_nb_curves;
    d.rec.v_nb_curves = s.v_nb_curves;
    d.rec.nb_points = s.nb_points;
    if s.nb_points > 0 {
        d.points = points_allocate(s.nb_points);
        copy_points(d.points.as_mut()?, src.points.as_deref()?, s.nb_points)?;
    }
    d.sync();
    srf_trm_cpy(host, &mut d, src)?;
    srf_proj_cpy(&mut d, src)?;
    d.rec.u_bottom = s.u_bottom;
    d.rec.u_top = s.u_top;
    d.rec.v_bottom = s.v_bottom;
    d.rec.v_top = s.v_top;
    d.rec.unk_90 = s.unk_90;
    d.rec.mr_approx = s.mr_approx;
    d.rec.mr_view = s.mr_view;
    d.rec.mr_edge = s.mr_edge;
    d.rec.mr_ucurv = s.mr_ucurv;
    d.rec.mr_vcurv = s.mr_vcurv;
    d.rec.mr_min = s.mr_min;
    d.rec.mr_max = s.mr_max;
    d.rec.mr_rec = s.mr_rec;
    Some(d)
}

fn comp_list_dispose(list: &mut Option<Vec<Option<Box<CompCurve>>>>, n: i32, tag: &str, array: &str, f: &mut Freed) {
    if let Some(v) = list.as_mut() {
        for i in 0..n.max(0) as usize {
            let Some(c) = v.get_mut(i) else { break };
            f.extend(comp_dispose(c).into_iter().map(|n| format!("{tag}{i}.{n}")));
        }
        f.push(array.to_string());
    }
    *list = None;
}

/// [soft 0x084db944 SI_nurbsSrfTrmDispose]: the display lines are disposed of with the
/// bounding-box tree hidden from that routine (the pointer at +0xd8 is cleared around the
/// call and put back), then every trim, then the array; the count becomes 0.
pub fn srf_trm_dispose(host: &mut dyn Host, srf: &mut Surface) -> Freed {
    let tree = srf.rec.unk_c8[4];
    srf.rec.unk_c8[4] = 0;
    host.srf_dispose_lines(srf);
    srf.rec.unk_c8[4] = tree;
    let mut f = Vec::new();
    let n = srf.rec.nb_trims;
    comp_list_dispose(&mut srf.trims, n, "t", "trims", &mut f);
    srf.rec.nb_trims = 0;
    srf.rec.trims = Ptr32::NULL;
    f
}

/// [soft 0x084db9d0 SI_nurbsSrfProjDispose]: the same for the projected curves.
pub fn srf_proj_dispose(host: &mut dyn Host, srf: &mut Surface) -> Freed {
    let tree = srf.rec.unk_c8[4];
    srf.rec.unk_c8[4] = 0;
    host.srf_dispose_lines(srf);
    srf.rec.unk_c8[4] = tree;
    let mut f = Vec::new();
    let n = srf.rec.nb_projected;
    comp_list_dispose(&mut srf.projected, n, "p", "projected", &mut f);
    srf.rec.nb_projected = 0;
    srf.rec.projected = Ptr32::NULL;
    f
}

/// [soft 0x084dba5c SI_nurbsSrfDispose]: the two knot vectors, the points, the trims, the
/// projected curves, the bounding-box tree (`SI_genTreeDestroy` with `SI_bezPatchDestroy`),
/// the record.
pub fn srf_dispose(host: &mut dyn Host, slot: &mut Option<Box<Surface>>) -> Freed {
    let mut f = Vec::new();
    let Some(mut s) = slot.take() else { return f };
    if s.knots_u.take().is_some() {
        f.push("ku".to_string());
    }
    if s.knots_v.take().is_some() {
        f.push("kv".to_string());
    }
    if s.points.take().is_some() {
        f.push("pts".to_string());
    }
    s.sync();
    f.extend(srf_trm_dispose(host, &mut s));
    f.extend(srf_proj_dispose(host, &mut s));
    host.gen_tree_destroy(&mut s);
    f.push("srf".to_string());
    f
}

/// [soft 0x084dde94 SI_nurbsSrfAddTrim]: appends a copy of the composite to the trims. The
/// array is made anew when the count is below 1 (what the pointer held is dropped: the
/// original leaks it).
pub fn srf_add_trim(srf: &mut Surface, comp: &CompCurve) -> Option<()> {
    if srf.rec.nb_trims < 1 {
        srf.trims = Some(vec![None]);
    } else {
        let v = srf.trims.as_mut()?;
        v.resize(srf.rec.nb_trims as usize + 1, None);
    }
    let copy = comp_copy(comp)?;
    let at = srf.rec.nb_trims.max(0) as usize;
    *srf.trims.as_mut()?.get_mut(at)? = Some(Box::new(copy));
    srf.rec.nb_trims = srf.rec.nb_trims.wrapping_add(1);
    srf.rec.trims = held();
    Some(())
}

/// [soft 0x084da69c SI_nurbsSrfNoVlines]: how many display lines run along V: for an open
/// U direction `(knots − (2 × (order − 2) + 1)) × curves + 1`, for a closed one
/// `(knots − 1) × curves`, with `curves` the word at +0x7c.
pub fn srf_no_vlines(rec: &NurbsSrf) -> i32 {
    lines_of(&rec.u, rec.v_nb_curves)
}

/// [soft 0x084da6cc SI_nurbsSrfNoUlines]: the same from the V direction and +0x78.
pub fn srf_no_ulines(rec: &NurbsSrf) -> i32 {
    lines_of(&rec.v, rec.u_nb_curves)
}

fn lines_of(d: &NurbsDir, curves: i32) -> i32 {
    if d.closed == 0 {
        d.nb_knots.wrapping_sub(d.order.wrapping_sub(2).wrapping_mul(2).wrapping_add(1)).wrapping_mul(curves).wrapping_add(1)
    } else {
        d.nb_knots.wrapping_sub(1).wrapping_mul(curves)
    }
}

/// [soft 0x084dc5f4 SI_nurbsSrfSwapUV]: exchanges the two directions of a surface.
///
/// Returns 0 and changes nothing when the surface has no point (or the new point array
/// cannot be allocated). Otherwise:
///
/// 1. the points go into a new array transposed, old (i, j) to index `i × nv + j`, and
///    where `SI_gblIndex` is set each new index is appended to it in the old order;
/// 2. the two direction blocks are exchanged, and with them the steps (+0x70, +0x74), the
///    curve counts (+0x78, +0x7c), the capping flags (+0x88 with +0x8c, +0x8a with +0x8e)
///    and the two words at +0x90;
/// 3. without trims and with display lines, the line records are reordered so that the
///    lines that ran along V come first (counts by [`srf_no_vlines`] and [`srf_no_ulines`]
///    of the **exchanged** record);
/// 4. for every trim: x and y of every control point of its curves are exchanged, and when
///    the composite has a trim line, the line's doubles at +0x10 and +0x28 and those at
///    +0x20 and +0x18 are exchanged, the two doubles of every node of its list are
///    exchanged, and the trimming package's `SI_trmCvrRve` is called on it (the host's).
/// 5. the old point array is freed.
pub fn srf_swap_uv(host: &mut dyn Host, srf: &mut Surface) -> Option<i32> {
    if srf.rec.nb_points == 0 {
        return Some(0);
    }
    let Some(mut new) = points_allocate(srf.rec.nb_points) else { return Some(0) };
    let (nu, nv) = (srf.rec.u.nb_points, srf.rec.v.nb_points);
    {
        let old = srf.points.as_deref().unwrap_or(&[]);
        let mut next = 0usize;
        let mut j = 0i32;
        // the loops re-read the counts from the record; nothing here changes them
        while j < nv {
            let mut i = 0i32;
            while i < nu {
                let to = i.wrapping_mul(nv).wrapping_add(j);
                let p = *old.get(next)?;
                *new.get_mut(usize::try_from(to).ok()?)? = p;
                if let Some(e) = host.gbl_index().and_then(|t| t.get_mut(next)) {
                    *e = to;
                }
                next += 1;
                i += 1;
            }
            j += 1;
        }
    }
    // the direction blocks change places whole, knot pointers included
    core::mem::swap(&mut srf.rec.u, &mut srf.rec.v);
    core::mem::swap(&mut srf.knots_u, &mut srf.knots_v);
    core::mem::swap(&mut srf.rec.u_step, &mut srf.rec.v_step);
    core::mem::swap(&mut srf.rec.u_nb_curves, &mut srf.rec.v_nb_curves);
    core::mem::swap(&mut srf.rec.u_bottom, &mut srf.rec.v_bottom);
    core::mem::swap(&mut srf.rec.u_top, &mut srf.rec.v_top);
    srf.rec.unk_90.swap(0, 1);
    if srf.rec.nb_trims == 0 && srf.nb_lines() > 0 {
        let n = srf.nb_lines();
        let old = srf.lines.take().unwrap_or_default();
        let mut out = vec![[0u32; 5]; n as usize];
        for k in 0..n {
            let nvl = srf_no_vlines(&srf.rec);
            let from = if k < nvl { k.wrapping_add(srf_no_ulines(&srf.rec)) } else { k.wrapping_sub(nvl) };
            out[k as usize] = *old.get(usize::try_from(from).ok()?)?;
        }
        srf.lines = Some(out);
    }
    let nt = srf.rec.nb_trims;
    if nt > 0 {
        let trims = srf.trims.as_mut()?;
        for t in 0..nt as usize {
            let comp = trims.get_mut(t)?.as_deref_mut()?;
            for c in 0..comp.rec.nb_curves.max(0) as usize {
                let cv = comp.curves.as_mut()?.get_mut(c)?.as_deref_mut()?;
                let n = cv.rec.dir.nb_points;
                if n > 0 {
                    for p in cv.points.as_mut()?.get_mut(..n as usize)? {
                        core::mem::swap(&mut p.x, &mut p.y);
                    }
                }
            }
            if let Some(line) = comp.trim_line.as_deref_mut() {
                line.box4.swap(0, 3);
                line.box4.swap(2, 1);
                for n in line.nodes.iter_mut() {
                    n.swap(0, 1);
                }
                host.trm_cvr_rve(t, line);
            }
        }
    }
    srf.points = Some(new);
    srf.rec.points = held();
    srf.rec.lines1 = mark(srf.lines.is_some());
    Some(1)
}

// ------------------------------------------------------------------------------ from files

fn point_from_file(p: &si_hrc::NurbsPoint) -> NurbsPoint {
    let mut q = NurbsPoint::zero();
    q.x = Double(p.xyzw[0]);
    q.y = Double(p.xyzw[1]);
    q.z = Double(p.xyzw[2]);
    q.w = Double(p.xyzw[3]);
    q.flag = p.flag;
    q
}

impl Curve {
    /// A curve as the model reader leaves it in memory (`nurbsCrvBinaryRead`, finding 0004:
    /// the allocator's record, then the file's tokens).
    pub fn from_file(c: &si_hrc::NurbsCurve) -> Curve {
        let mut rec = NurbsCrv::allocate();
        rec.dir.order = c.order;
        rec.dir.nb_points = c.npoints;
        rec.dir.nb_knots = c.nknots;
        rec.dir.range_start = Double(c.start);
        rec.dir.range_end = Double(c.end);
        rec.dir.closed = c.closed;
        rec.param = c.uniformity;
        rec.closing = c.closing;
        rec.step = c.step;
        rec.keyframed = c.keyframed;
        let knots = (!c.knots.is_empty()).then(|| c.knots.clone());
        let points = (!c.points.is_empty()).then(|| c.points.iter().map(point_from_file).collect());
        Curve::from_parts(rec, knots, points)
    }
}

impl CompCurve {
    /// A composite as the model reader leaves it: the boundary flag and the curves.
    pub fn from_file(c: &si_hrc::NurbsComposite) -> CompCurve {
        let mut rec = comp_raw();
        rec.boundary = c.boundary;
        rec.nb_curves = c.ncurves;
        rec.block_14 = Ptr32::NULL;
        let curves: Vec<Option<Box<Curve>>> = c.curves.iter().map(|k| Some(Box::new(Curve::from_file(k)))).collect();
        rec.curves = mark(!curves.is_empty());
        CompCurve { rec, curves: (!curves.is_empty()).then_some(curves), trim_line: None }
    }
}

impl Surface {
    /// A surface as the model reader leaves it in memory (`nurbsSrfBinaryRead`).
    pub fn from_file(f: &si_hrc::NurbsSurface) -> Surface {
        let mut rec = NurbsSrf::allocate();
        rec.u.order = f.order[0];
        rec.v.order = f.order[1];
        rec.u.nb_points = f.count[0];
        rec.v.nb_points = f.count[1];
        rec.nb_points = f.points.len() as i32;
        rec.u.closed = f.closed[0];
        rec.v.closed = f.closed[1];
        rec.param = f.uniformity;
        rec.u.nb_knots = f.nknots[0];
        rec.v.nb_knots = f.nknots[1];
        rec.u.range_start = Double(f.range[0]);
        rec.u.range_end = Double(f.range[1]);
        rec.v.range_start = Double(f.range[2]);
        rec.v.range_end = Double(f.range[3]);
        rec.border = f.border;
        rec.nb_trims = f.ntrims;
        rec.nb_projected = f.nprojected;
        rec.u_step = f.steps[0];
        rec.v_step = f.steps[1];
        rec.u_nb_curves = f.curves[0];
        rec.v_nb_curves = f.curves[1];
        rec.u_bottom = f.caps[0];
        rec.u_top = f.caps[1];
        rec.v_bottom = f.caps[2];
        rec.v_top = f.caps[3];
        rec.unk_90 = f.closing;
        let list = |l: &[si_hrc::NurbsComposite]| -> Option<Vec<Option<Box<CompCurve>>>> {
            (!l.is_empty()).then(|| l.iter().map(|c| Some(Box::new(CompCurve::from_file(c)))).collect())
        };
        let mut s = Surface::from_parts(
            rec,
            (!f.knots[0].is_empty()).then(|| f.knots[0].clone()),
            (!f.knots[1].is_empty()).then(|| f.knots[1].clone()),
            (!f.points.is_empty()).then(|| f.points.iter().map(point_from_file).collect()),
        );
        s.trims = list(&f.trims);
        s.projected = list(&f.projected);
        s.sync();
        s
    }
}
