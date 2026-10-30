// SPDX-License-Identifier: MIT
//! `update_voxel.c`: taking the triangles of a voxel (or of a box of voxels) out of the mesh lists again
//! (finding 0012, section 28). The vertex list has 0xb4-byte records (a triangle count at 0x78 and the triangle
//! numbers from 0x7c), the triangle list 0x28-byte records (three vertex numbers, a "removed" flag at 0x24), the voxel
//! list 0xc-byte records (count, size, pointer to triangle numbers); 8 x 8 x 8 voxels.

use crate::hooks::{VCG_TRI_LIST, VCG_VOX_LIST, VCG_VTX_LIST};
use si_mem::Mem;
use si_x87::{Round, F80};

/// [soft 0x0857b390 VC_rmVertex]: `VC_rmVertex(triangle, vertex)`: takes the triangle off the vertex's list (the last one
/// moves into its place). 0x15 when the vertex does not list it.
pub fn rm_vertex(m: &mut Mem, tri: i32, vtx: i32) -> i32 {
    let base = m.u32(VCG_VTX_LIST).wrapping_add((vtx as u32).wrapping_mul(0xb4));
    let n = m.i32(base + 0x78);
    let mut i = 0i32;
    if 0 < n {
        let mut v = m.i32(base + 0x7c);
        while v != tri {
            i += 1;
            if i >= n {
                break;
            }
            v = m.i32(base + 0x7c + 4 * i as u32);
        }
    }
    let n = m.i32(base + 0x78);
    if i == n {
        return 0x15;
    }
    m.set_i32(base + 0x78, n.wrapping_sub(1));
    let last = m.i32(base + 0x7c - 4 + 4 * n as u32);
    m.set_i32(base + 0x7c + 4 * i as u32, last);
    0
}

/// [soft 0x0857b3fc VC_rmTriangle]: `VC_rmTriangle(triangle)`: off the lists of its three vertices, flagged removed.
/// The last non-zero status of the three.
pub fn rm_triangle(m: &mut Mem, tri: i32) -> i32 {
    let mut status = 0;
    for k in 0..3u32 {
        let t = m.u32(VCG_TRI_LIST).wrapping_add((tri as u32).wrapping_mul(0x28));
        let v = m.i32(t + 4 * k);
        let r = rm_vertex(m, tri, v);
        if r != 0 {
            status = r;
        }
    }
    let t = m.u32(VCG_TRI_LIST).wrapping_add((tri as u32).wrapping_mul(0x28));
    m.set_i16(t + 0x24, 1);
    status
}

/// [soft 0x0857b46c VC_rm_tri_in_voxel]: `VC_rm_tri_in_voxel(voxel)`: removes every triangle of the voxel and frees its list.
pub fn rm_tri_in_voxel(m: &mut Mem, vox: i32) -> i32 {
    let mut status = 0;
    let e = m.u32(VCG_VOX_LIST).wrapping_add((vox as u32).wrapping_mul(0xc));
    let n = m.i32(e);
    if n != 0 {
        let mut i = 0;
        while i < n {
            let e = m.u32(VCG_VOX_LIST).wrapping_add((vox as u32).wrapping_mul(0xc));
            let t = m.i32(m.u32(e + 8) + 4 * i as u32);
            let r = rm_triangle(m, t);
            if r != 0 {
                status = r;
            }
            i += 1;
        }
        let e = m.u32(VCG_VOX_LIST).wrapping_add((vox as u32).wrapping_mul(0xc));
        m.set_i32(e, 0);
        m.set_i32(e + 4, 0);
        let p = m.u32(e + 8);
        m.free(p);
    }
    status
}

/// A coordinate of the box in voxels: the difference, divided by the step (both in the 80-bit register), cut to an integer.
fn cell(hi: f64, lo: f64, step: f64) -> i32 {
    let d = F80::from_f64(hi) - F80::from_f64(lo);
    (d / F80::from_f64(step)).to_i32(Round::Zero)
}

/// [soft 0x0857b500 VC_rm_tri_in_upbv]: `VC_rm_tri_in_upbv(cells, step, low, box, out)`: removes the triangles of the voxels the box
/// (six doubles: low corner, high corner) meets, where `cells` voxels make one unit of the coarse grid; `out` gets the box
/// in grid units (six ints). `cells` must not be 0.
pub fn rm_tri_in_upbv(m: &mut Mem, cells: i32, step: u32, low: u32, bx: u32, out: u32) -> i32 {
    let f = |a: u32| m.f64(a);
    let q = [
        cell(f(bx), f(low), f(step)),
        cell(f(bx + 8), f(low + 8), f(step + 8)),
        cell(f(bx + 16), f(low + 16), f(step + 16)),
        cell(f(bx + 24), f(low), f(step)),
        cell(f(bx + 32), f(low + 8), f(step + 8)),
        cell(f(bx + 40), f(low + 16), f(step + 16)),
    ];
    let mut c = q.map(|v| v.wrapping_div(cells));
    for k in 0..3 {
        m.set_i32(out + 4 * k, c[k as usize].wrapping_mul(cells));
        m.set_i32(out + 12 + 4 * k, c[3 + k as usize].wrapping_mul(cells).wrapping_add(cells));
    }
    for k in 0..3 {
        c[k] = c[k].max(0);
        c[3 + k] = c[3 + k].min(7);
    }
    let mut status = 0;
    let mut z = c[2];
    while z <= c[5] {
        let mut y = c[1];
        while y <= c[4] {
            let mut x = c[0];
            while x <= c[3] {
                let r = rm_tri_in_voxel(m, x + z * 0x40 + y * 8);
                if r != 0 {
                    status = r;
                }
                x += 1;
            }
            y += 1;
        }
        z += 1;
    }
    status
}

/// [soft 0x0857b744 VC_rmAllTri]: `VC_rmAllTri()`: empties all 512 voxels.
pub fn rm_all_tri(m: &mut Mem) -> i32 {
    let mut status = 0;
    for z in 0..8 {
        for y in 0..8 {
            for x in 0..8 {
                let r = rm_tri_in_voxel(m, x + z * 0x40 + y * 8);
                if r != 0 {
                    status = r;
                }
            }
        }
    }
    status
}
