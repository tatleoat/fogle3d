// SPDX-License-Identifier: MIT
//! The small routines of `nrb_misc.c`: control points through a matrix, a point moved, points
//! and knots handed from one curve or surface to another of the same shape, a control vertex
//! read out.
//!
//! A matrix is the program's 4 × 4 of floats with **row vectors** (finding 0008): a
//! homogeneous point `(x, y, z, w)` becomes `p × M`, coordinate `q` being
//! `((x × m[0][q] + y × m[1][q]) + z × m[2][q]) + w × m[3][q]`, the products and sums in the
//! register and one rounding to `double`.

use crate::own::{Host, Surface};
use crate::{co, NurbsCrv, NurbsDir, NurbsPoint, NurbsSrf};
use si_types::Double;
use si_x87::F80;

/// Bits 0x60 of the highest byte of a control point's flag word (+0x23), of a curve's
/// +0x38 word (+0x3b) and of a surface's +0x58 word (+0x5b): set by every routine here that
/// changes points ("the points have moved"; what clears them is the display code's, not
/// read).
pub const MOVED: u32 = 0x6000_0000;

/// The error code of the two control-vertex readers for an index above the count.
pub const BAD_INDEX: i32 = 0x3b;

fn through(p: &NurbsPoint, m: &[[f32; 4]; 4]) -> [f64; 4] {
    core::array::from_fn(|q| {
        let f = |r: usize| F80::from_f32(m[r][q]);
        (((co(p, 0) * f(0) + co(p, 1) * f(1)) + co(p, 2) * f(2)) + co(p, 3) * f(3)).to_f64()
    })
}

fn put4(p: &mut NurbsPoint, v: [f64; 4]) {
    p.x = Double(v[0]);
    p.y = Double(v[1]);
    p.z = Double(v[2]);
    p.w = Double(v[3]);
}

/// [soft 0x084ea6ec SI_nrbPtsMultiplyMatrix]: `n` points of `src` through a matrix into
/// `dst` (the four doubles; flag words of `dst` are left alone). `src` and `dst` may be the
/// same array in the original; here the caller passes a copy for that.
pub fn pts_multiply_matrix(src: &[NurbsPoint], m: &[[f32; 4]; 4], dst: &mut [NurbsPoint], n: i32) -> Option<()> {
    for i in 0..n.max(0) as usize {
        let v = through(src.get(i)?, m);
        put4(dst.get_mut(i)?, v);
    }
    Some(())
}

/// [soft 0x084e9750 SI_nurbsCrvApplyMatrix]: every control point of a curve through a
/// matrix, or with `mask` not 0 only the points whose flag word has one of its bits. A
/// transformed point gets [`MOVED`] in its flag word, and so does the curve's +0x38 word,
/// whether any point was transformed or not.
pub fn crv_apply_matrix(rec: &mut NurbsCrv, points: &mut [NurbsPoint], m: &[[f32; 4]; 4], mask: i32) -> Option<()> {
    for i in 0..rec.dir.nb_points.max(0) as usize {
        let p = points.get_mut(i)?;
        if mask == 0 || (p.flag as u32 & mask as u32) != 0 {
            let v = through(p, m);
            put4(p, v);
            p.flag = (p.flag as u32 | MOVED) as i32;
        }
    }
    rec.flags_38 |= MOVED;
    Some(())
}

/// [soft 0x084e9eec SI_nurbsCrvMovePoint]: puts control point `i` at the cartesian place
/// (x, y, z): each coordinate times the point's weight, which is kept. Marks point and
/// curve [`MOVED`]. Returns 1. No test of the index.
pub fn crv_move_point(rec: &mut NurbsCrv, points: &mut [NurbsPoint], i: i32, x: f64, y: f64, z: f64) -> Option<i32> {
    let p = points.get_mut(usize::try_from(i).ok()?)?;
    let w = F80::from_f64(p.w.0);
    p.flag = (p.flag as u32 | MOVED) as i32;
    p.x = Double((F80::from_f64(x) * w).to_f64());
    p.y = Double((F80::from_f64(y) * w).to_f64());
    p.z = Double((F80::from_f64(z) * w).to_f64());
    rec.flags_38 |= MOVED;
    Some(1)
}

/// [soft 0x084ea038 SI_nurbsCrvTransferPoints]: copies the source's control points (as many
/// as the **source** counts, whole records) over the destination's and marks the
/// destination curve [`MOVED`].
pub fn crv_transfer_points(src: &NurbsCrv, src_points: &[NurbsPoint], dst: &mut NurbsCrv, dst_points: &mut [NurbsPoint]) -> Option<()> {
    dst.flags_38 |= MOVED;
    let n = src.dir.nb_points.max(0) as usize;
    dst_points.get_mut(..n)?.copy_from_slice(src_points.get(..n)?);
    Some(())
}

/// [soft 0x084ea108 SI_nurbsTransferDirKnots]: copies the source direction's knots (as many
/// as the source counts) and its range into the destination. Counts, order and the closed
/// flag are not touched.
pub fn transfer_dir_knots(src: &NurbsDir, src_knots: &[f64], dst: &mut NurbsDir, dst_knots: &mut [f64]) -> Option<()> {
    let n = src.nb_knots.max(0) as usize;
    dst_knots.get_mut(..n)?.copy_from_slice(src_knots.get(..n)?);
    dst.range_start = src.range_start;
    dst.range_end = src.range_end;
    Some(())
}

/// [soft 0x084ea00c SI_nurbsCrvTransferPointsKnots]: [`crv_transfer_points`], then
/// [`transfer_dir_knots`].
pub fn crv_transfer_points_knots(src: &NurbsCrv, src_knots: &[f64], src_points: &[NurbsPoint], dst: &mut NurbsCrv, dst_knots: &mut [f64], dst_points: &mut [NurbsPoint]) -> Option<()> {
    crv_transfer_points(src, src_points, dst, dst_points)?;
    transfer_dir_knots(&src.dir, src_knots, &mut dst.dir, dst_knots)
}

/// [soft 0x084ea0c4 SI_nurbsSrfTransferPoints]: the same for a surface: the source's total
/// of points, and [`MOVED`] in the destination's +0x58 word.
pub fn srf_transfer_points(src: &NurbsSrf, src_points: &[NurbsPoint], dst: &mut NurbsSrf, dst_points: &mut [NurbsPoint]) -> Option<()> {
    dst.unk_58 = (dst.unk_58 as u32 | MOVED) as i32;
    let n = src.nb_points.max(0) as usize;
    dst_points.get_mut(..n)?.copy_from_slice(src_points.get(..n)?);
    Some(())
}

/// [soft 0x084ea07c SI_nurbsSrfTransferPointsKnots]: disposes of the destination's display
/// lines (`SI_nurbsSrfDisposeLines`), then points, then the knots and range of U and of V.
pub fn srf_transfer_points_knots(host: &mut dyn Host, src: &Surface, dst: &mut Surface) -> Option<()> {
    host.srf_dispose_lines(dst);
    let sv = src.view();
    srf_transfer_points(sv.rec, sv.points, &mut dst.rec, dst.points.as_deref_mut().unwrap_or(&mut []))?;
    transfer_dir_knots(&sv.rec.u, sv.knots_u, &mut dst.rec.u, dst.knots_u.as_deref_mut().unwrap_or(&mut []))?;
    transfer_dir_knots(&sv.rec.v, sv.knots_v, &mut dst.rec.v, dst.knots_v.as_deref_mut().unwrap_or(&mut []))
}

/// A control vertex as the two readers return it (`SI_CtrlVertex`, 0x24 bytes): the
/// cartesian position, the weight, the flag word.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CtrlVertex {
    pub x: Double,
    pub y: Double,
    pub z: Double,
    pub w: Double,
    pub flag: i32,
}

const _: () = assert!(core::mem::size_of::<CtrlVertex>() == 0x24);

fn ctrl_vertex(points: &[NurbsPoint], count: i32, i: i32) -> Option<Result<CtrlVertex, i32>> {
    if count < i {
        return Some(Err(BAD_INDEX));
    }
    let p = points.get(usize::try_from(i).ok()?)?;
    let inv = F80::ONE / F80::from_f64(p.w.0);
    let c = |q: usize| Double((co(p, q) * inv).to_f64());
    Some(Ok(CtrlVertex { x: c(0), y: c(1), z: c(2), w: Double(si_x87::fmov64(p.w.0)), flag: p.flag }))
}

/// [soft 0x084ea7a0 SI_nurbsCrvGetCtrlVertex]: control point `i` of a curve with its
/// position divided by its weight (`1 / w` in the register, **no test of the weight**).
/// `Err(0x3b)` when `i` is **above** the point count; `i` equal to the count passes the test
/// and reads one record past the points (`None` here).
pub fn crv_get_ctrl_vertex(rec: &NurbsCrv, points: &[NurbsPoint], i: i32) -> Option<Result<CtrlVertex, i32>> {
    ctrl_vertex(points, rec.dir.nb_points, i)
}

/// [soft 0x084ea800 SI_nurbsSrfGetCtrlVertex]: the same for a surface, against its total.
pub fn srf_get_ctrl_vertex(rec: &NurbsSrf, points: &[NurbsPoint], i: i32) -> Option<Result<CtrlVertex, i32>> {
    ctrl_vertex(points, rec.nb_points, i)
}

/// [soft 0x084ea3e4 getPointInterval] (static, on the image): `getPointInterval(dir, i,
/// &first, &last, &count, &start)`, the knot spans a control point of the direction at `dir`
/// bears on, for `markDirtyCcrvs (0x084ea5ec)` (which display lines a moved point
/// invalidates; not ported). The original writes through the four pointers in this order,
/// re-reading what it wrote:
///
/// - `*start` = `order − 2` for an open direction, 0 for a closed one;
/// - **open**: `*first` = `order − 2` and `*last` = `order − 1 + i` when `i` is below
///   `order − 2`, else `*first` = `i` and `*last` = `i + order − 1`; `*last` lowered to
///   `knots − order` when above it; `*count` = `*last − *first + 1`;
/// - **closed**: `*count` = `order`; `*first` = `i`, or `knots − 2` for a negative `i`;
///   `*last` = `(*first − 1 + order) % (knots − 1)`, C's remainder (of a negative number,
///   negative).
///
/// The arithmetic is 32-bit and wraps. A closed direction of one knot divides by zero (the
/// original then dies of `SIGFPE`): `Err`, and so for the one quotient that traps beside it.
pub fn get_point_interval(m: &mut si_mem::Mem, dir: u32, i: i32, first: u32, last: u32, count: u32, start: u32) -> Result<(), crate::image::Halt> {
    let closed = m.i16(dir + 0x20) != 0;
    m.set_i32(start, if closed { 0 } else { m.i32(dir).wrapping_sub(2) });
    if !closed {
        let below = m.i32(dir).wrapping_sub(2);
        if i < below {
            m.set_i32(first, below);
            m.set_i32(last, below.wrapping_add(i).wrapping_add(1));
        } else {
            m.set_i32(first, i);
            m.set_i32(last, i.wrapping_add(m.i32(dir)).wrapping_sub(1));
        }
        let most = m.i32(dir + 8).wrapping_sub(m.i32(dir));
        if m.i32(last) > most {
            m.set_i32(last, most);
        }
        let n = m.i32(last).wrapping_sub(m.i32(first)).wrapping_add(1);
        m.set_i32(count, n);
    } else {
        m.set_i32(count, m.i32(dir));
        if i < 0 {
            m.set_i32(first, m.i32(dir + 8).wrapping_sub(2));
        } else {
            m.set_i32(first, i);
        }
        let a = m.i32(first).wrapping_sub(1).wrapping_add(m.i32(dir));
        let d = m.i32(dir + 8).wrapping_sub(1);
        if d == 0 || (a == i32::MIN && d == -1) {
            return Err(crate::image::Halt::Refused("getPointInterval"));
        }
        m.set_i32(last, a % d);
    }
    Ok(())
}
