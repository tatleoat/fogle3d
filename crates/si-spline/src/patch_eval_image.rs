// SPDX-License-Identifier: MIT
//! A patch's matrices, its evaluation at a point and its grid of points on the one memory
//! image (finding 0020, sections 10, 11 and 26; finding 0024): what `SI_patchConvertToMesh`
//! builds its mesh from, and the evaluators the renderer and the tools call.
//!
//! The record and the control points are read where they lie ([`ImagePatch`]: a point is
//! read at `points + 16 × index`, also where the original reads outside its block, and a
//! point that is no memory is a fault, as in the original). The arithmetic is the value
//! forms' ([`crate::patch`], [`crate::patch_grid`]), which take their points from the same
//! [`PatchSource`] and were compared first.

use std::cell::Cell;

use si_mem::{Mem, Ptr32};
use si_types::geom::{Patch, Vector};
use si_x87::fmov;

use crate::patch::{update_matrix_rec, PatchSource};
use crate::patch_grid::{grid_cells, grid_dims};

/// A patch on the image as the evaluators see it: the record (read once: none of these
/// routines writes it while it reads) and the address of its control points.
pub struct ImagePatch<'a> {
    m: &'a Mem,
    rec: Patch,
    /// The first point that could not be read: the original faults there.
    missed: Cell<Option<u32>>,
}

impl<'a> ImagePatch<'a> {
    pub fn new(m: &'a Mem, p: Ptr32<Patch>) -> ImagePatch<'a> {
        ImagePatch { m, rec: m.read(p), missed: Cell::new(None) }
    }
}

impl PatchSource for ImagePatch<'_> {
    fn rec(&self) -> &Patch {
        &self.rec
    }
    fn point(&self, index: i32) -> Option<[f32; 4]> {
        let a = self.rec.points.addr().wrapping_add((index as u32).wrapping_mul(16));
        match self.m.try_bytes(a, 16) {
            Some(b) => Some([0, 4, 8, 12].map(|k| f32::from_le_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]))),
            None => {
                if self.missed.get().is_none() {
                    self.missed.set(Some(a));
                }
                None
            }
        }
    }
}

/// Makes the image record the fault the original meets at a point it cannot read.
fn fault_at(m: &mut Mem, missed: Option<u32>) {
    if let Some(a) = missed {
        let _ = m.u32(a);
    }
}

/// The six matrices of a patch [soft 0x0847a680 SI_patchUpdateMatrix], in place: a linear
/// direction's step count (+0x1c, +0x1e) forced to 1, then the two bases (+0x24 the U
/// basis, +0x64 the V basis transposed) and the four difference matrices (+0xa4, +0xe4,
/// +0x124, +0x164) as [`crate::patch::update_matrix`] has them. Both bases go through one
/// local, which the original does not initialise: for a U type outside 0..=3 it holds
/// what the stack held, zeros here.
pub fn patch_update_matrix(m: &mut Mem, p: Ptr32<Patch>) {
    let mut r: Patch = m.read(p);
    update_matrix_rec(&mut r);
    if m[p].u_type == 0 {
        m[p].u_step = 1;
    }
    if m[p].v_type == 0 {
        m[p].v_step = 1;
    }
    m[p].basis = r.basis;
    m[p].matrices = r.matrices;
}

/// The patch as a grid of points [soft 0x0847f89c SI_patchSurface]: `*nu` and `*nv` (16
/// bits) as [`crate::patch_grid::patch_surface`] counts them, written first; then
/// `SI_calloc(nu × nv, 12)` (the two read back as signed 16-bit numbers) into `*out`, and
/// every cell's points written into that block at their places in the grid, 12 bytes a
/// point. Nothing is checked against the block: a count that is zero or negative with
/// cells to write makes the original write through null or past the block, and the image
/// does the same.
pub fn patch_surface(m: &mut Mem, p: Ptr32<Patch>, out: u32, nu_at: u32, nv_at: u32) {
    let (nu, nv) = grid_dims(&m.read(p));
    m.set_i16(nu_at, nu);
    m.set_i16(nv_at, nv);
    let count = (m.i16(nu_at) as i32).wrapping_mul(m.i16(nv_at) as i32);
    let block = m.si_calloc(count as u32, 12);
    m.set_u32(out, block);
    // the points read are the patch's, the points written are in the new block: the
    // writes can be made after the reads
    let (points, missed) = {
        let s = ImagePatch::new(m, p);
        let mut points: Vec<(i32, Vector)> = Vec::new();
        let _ = grid_cells(&s, nv, &mut |index, v| {
            points.push((index, v));
            Some(())
        });
        (points, s.missed.get())
    };
    for (index, v) in points {
        let a = block.wrapping_add((index as u32).wrapping_mul(12));
        m.set_f32(a, v.x);
        m.set_f32(a + 4, v.y);
        m.set_f32(a + 8, v.z);
        if m.fault().is_some() {
            return;
        }
    }
    fault_at(m, missed);
}

fn put(m: &mut Mem, at: u32, v: Vector) {
    m.set_f32(at, v.x);
    m.set_f32(at + 4, v.y);
    m.set_f32(at + 8, v.z);
}

/// The point at the global parameters (u, v) [soft 0x0847dee4 SI_patchValue], stored at
/// `out`; the arithmetic is [`crate::patch::value`]'s. Where the original divides by zero
/// (a direction without points) or does not return (a parameter above 2²⁴) nothing is
/// written; a point that is no memory is a fault.
pub fn patch_value(m: &mut Mem, p: Ptr32<Patch>, u: f32, v: f32, out: u32) {
    let (r, missed) = {
        let s = ImagePatch::new(m, p);
        (crate::patch::value(&s, u, v), s.missed.get())
    };
    match r {
        Some(q) => put(m, out, q),
        None => fault_at(m, missed),
    }
}

/// Point and derivatives at (u, v) [soft 0x0847ff50 SI_patchValueAndTangent]: 1 with the
/// point at `point` and the two derivatives at `du`, `dv`; 0 and nothing written for a
/// patch without a segment in a direction ([`crate::patch::value_and_tangent`]).
pub fn patch_value_and_tangent(m: &mut Mem, p: Ptr32<Patch>, u: f32, v: f32, point: u32, du: u32, dv: u32) -> i32 {
    let (r, missed) = {
        let s = ImagePatch::new(m, p);
        (crate::patch::value_and_tangent(&s, u, v), s.missed.get())
    };
    match r {
        Some(t) => {
            put(m, point, t.point);
            put(m, du, t.tu);
            put(m, dv, t.tv);
            1
        }
        None => {
            fault_at(m, missed);
            0
        }
    }
}

/// Point and unit normal at (u, v) [soft 0x0847fe84 SI_patchValueAndNormal]: always 1.
/// When the tangent routine refuses, the original leaves the point alone and computes the
/// normal from locals nobody set; nothing is written here then.
pub fn patch_value_and_normal(m: &mut Mem, p: Ptr32<Patch>, u: f32, v: f32, point: u32, normal: u32) -> i32 {
    let (r, missed) = {
        let s = ImagePatch::new(m, p);
        (crate::patch::value_and_normal(&s, u, v), s.missed.get())
    };
    match r {
        Some((q, n)) => {
            put(m, point, q);
            put(m, normal, n);
        }
        None => fault_at(m, missed),
    }
    1
}

/// The texture coordinates of a polygon of the mesh conversion
/// [soft 0x084868c8 patchToMeshSetPolygonUV]: for each of the polygon's corners (the
/// 16-bit count at +0x10, unsigned), the node record of 0x24 bytes at +0xc gets two
/// floats of `uv` (moved through the unit) at +0x14 and +0x18.
pub fn patch_to_mesh_set_polygon_uv(m: &mut Mem, poly: u32, uv: u32) {
    let mut node = m.u32(poly + 0xc);
    let mut at = uv;
    let mut i = 0u32;
    while i < m.u16(poly + 0x10) as u32 {
        let a = fmov(m.f32(at));
        m.set_f32(node + 0x14, a);
        let b = fmov(m.f32(at + 4));
        m.set_f32(node + 0x18, b);
        at += 8;
        node = node.wrapping_add(0x24);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::Surface;

    #[test]
    fn the_image_grid_is_the_value_grid() {
        let pts: Vec<[f32; 3]> = (0..20).map(|i| [(i / 5) as f32, (i % 5) as f32, ((i * 7) % 3) as f32]).collect();
        let mut s = Surface::from_points([2, 3], [false, true], 4, 5, &pts);
        s.rec.v_tension = 0.5;
        crate::patch::update_matrix(&mut s);
        let want = crate::patch_grid::patch_surface(&s).unwrap();
        let mut m = Mem::new();
        let block = m.calloc(20, 16);
        for (i, p) in s.points.iter().enumerate() {
            for k in 0..4 {
                m.set_f32(block + 16 * i as u32 + 4 * k as u32, p[k]);
            }
        }
        let mut rec = s.rec;
        rec.points = Ptr32::new(block);
        let p: Ptr32<Patch> = Ptr32::new(m.calloc(1, Patch::SIZE as u32));
        m.write(p, rec);
        let outs = m.calloc(3, 4);
        patch_surface(&mut m, p, outs, outs + 4, outs + 6);
        assert_eq!((m.i16(outs + 4), m.i16(outs + 6)), (want.nu, want.nv));
        let grid = m.u32(outs);
        for (i, q) in want.points.iter().enumerate() {
            assert_eq!(m.f32(grid + 12 * i as u32), q.x);
            assert_eq!(m.f32(grid + 12 * i as u32 + 8), q.z);
        }
        patch_value(&mut m, p, 1.25, 0.5, outs);
        assert_eq!(m.f32(outs), crate::patch::value(&s, 1.25, 0.5).unwrap().x);
        assert!(m.fault().is_none());
    }
}
