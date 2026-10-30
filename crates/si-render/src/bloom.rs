// SPDX-License-Identifier: MIT
//! `bloom.c`: the polygonisation of the metaball field (finding 0012, section 28): the cube walk that turns the
//! density function into triangles, and the vertex and triangle tables it fills. Doubles are stored in the image; sums
//! and products stay in the 80-bit register unless the original stores them.

use crate::bi_search::bi_search;
use crate::hooks::{VCG_TRI_LIST, VCG_VOX_LIST, VCG_VTX_LIST};
use crate::range::{check_upbv, extract_zslab, influ_test};
use crate::update_voxel::rm_tri_in_upbv;
use crate::vceval::{eval_dens, eval_norm, eval_surf};
use crate::volume::*;
use si_mem::Mem;
use si_x87::{Round, F80};

pub const VCG_VTX_COUNT: u32 = 0x0882_99d4;
pub const VCG_TRI_COUNT: u32 = 0x0882_99dc;
/// `vcS_cubeDim1`, `vcS_cubeVoxRatio`: cells per cube edge plus one, and cells per voxel edge.
pub const VCS_CUBE_DIM1: u32 = 0x0882_9a04;
pub const VCS_CUBE_VOX_RATIO: u32 = 0x0882_9a08;
pub const VCS_VTX_SAVE1: u32 = 0x0882_9a14;
pub const VCS_VTX_SAVE2: u32 = 0x0882_9a18;
/// `vcS_mbList`: the index list (count, size, pointer) the polygonisation evaluates.
pub const VCS_MB_LIST: u32 = 0x0882_9a1c;
pub const VCS_NORM_DELTA: u32 = 0x0882_9a30;
pub const POLY_TYPE: u32 = 0x0882_9a38;
pub const VCS_VTX_EMPTY_START: u32 = 0x0882_9a5c;
pub const VCS_TRI_EMPTY_START: u32 = 0x0882_9a60;
pub const VCS_TRI_MAX: u32 = 0x0882_9a64;
pub const VCS_VTX_MAX: u32 = 0x0882_9a68;
/// `vcS_edgeTable`: for each of 12 cube edges' endpoint pairs a word offset into the edge-mark array.
pub const VCS_EDGE_TABLE: u32 = 0x0882_9b80;

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f64(m.f64(a))
}

/// [soft 0x08577680 VC_createTri]: `VC_createTri(vertices, cell)`: enters the triangle of the three vertex indices at the first
/// free triangle slot: back references in the vertices, the normal (cross product), the voxel's triangle list. 0, or 0x10 when
/// an allocation fails.
pub fn create_tri(m: &mut Mem, tri: u32, cell: u32) -> i32 {
    let vtx = m.u32(VCG_VTX_LIST);
    let mut start = m.i32(VCS_TRI_EMPTY_START);
    let count = m.i32(VCG_TRI_COUNT);
    if start < count {
        let list = m.u32(VCG_TRI_LIST);
        let mut s = m.i16(list.wrapping_add((start as u32).wrapping_mul(0x28)).wrapping_add(0x24));
        while s != 1 {
            start += 1;
            m.set_i32(VCS_TRI_EMPTY_START, start);
            if start >= count {
                break;
            }
            s = m.i16(list.wrapping_add((start as u32).wrapping_mul(0x28)).wrapping_add(0x24));
        }
    }
    let start = m.i32(VCS_TRI_EMPTY_START);
    for i in 0..3u32 {
        let v = vtx.wrapping_add((m.u32(tri + 4 * i)).wrapping_mul(0xb4));
        let k = m.u32(v + 0x78);
        m.set_i32(v + 0x7c + 4 * k, start);
        m.set_u32(v + 0x78, k + 1);
    }
    let t = m.u32(VCG_TRI_LIST).wrapping_add((start as u32).wrapping_mul(0x28));
    for i in 0..3u32 {
        let x = m.u32(tri + 4 * i);
        m.set_u32(t + 4 * i, x);
    }
    let vp = |m: &Mem, i: u32| vtx.wrapping_add(m.u32(tri + 4 * i).wrapping_mul(0xb4));
    let (p0, p1, p2) = (vp(m, 0), vp(m, 1), vp(m, 2));
    let sub = |m: &Mem, a: u32, b: u32, k: u32| f(m, a + 8 * k) - f(m, b + 8 * k);
    // (the register keeps the unrounded difference of the second and third coordinates: dy1, dz1, e, z)
    let dx1 = sub(m, p1, p0, 0).to_f64();
    let dy1r = sub(m, p1, p0, 1);
    let dz1r = sub(m, p1, p0, 2);
    let dy1 = dy1r.to_f64();
    let dz1 = dz1r.to_f64();
    let dx2 = sub(m, p2, p0, 0).to_f64();
    let er = sub(m, p2, p0, 1);
    let e = er.to_f64();
    let zr = sub(m, p2, p0, 2);
    let z = zr.to_f64();
    m.set_f64(t + 0x0c, (dz1r * er - dy1r * zr).to_f64());
    let c = |v: f64| F80::from_f64(v);
    m.set_f64(t + 0x14, (c(z) * c(dx1) - c(dx2) * c(dz1)).to_f64());
    m.set_i16(t + 0x24, 0);
    m.set_f64(t + 0x1c, (c(dx2) * c(dy1) - c(e) * c(dx1)).to_f64());
    let ratio = m.i32(VCS_CUBE_VOX_RATIO);
    let mut cz = m.i32(cell + 8) / ratio;
    let mut cy = m.i32(cell + 4) / ratio;
    let mut cx = m.i32(cell) / ratio;
    cz = cz.min(7);
    cy = cy.min(7);
    cx = cx.min(7);
    let vox = m.u32(VCG_VOX_LIST).wrapping_add(((cz * 0x40 + cy * 8 + cx) as u32).wrapping_mul(12));
    let n = m.i32(vox);
    let start = m.i32(VCS_TRI_EMPTY_START);
    if n == 0 {
        let p = m.malloc(2000);
        m.set_u32(vox + 8, p);
        if p == 0 {
            return 0x10;
        }
        m.set_i32(vox + 4, 500);
        m.set_i32(p, start);
        m.set_i32(vox, 1);
    } else if n == m.i32(vox + 4) {
        let old = m.u32(vox + 8);
        let p = m.realloc(old, ((n + 500) as u32).wrapping_mul(4));
        m.set_u32(vox + 8, p);
        if p == 0 {
            return 0x10;
        }
        m.set_i32(vox + 4, n + 500);
        m.set_i32(p + 4 * n as u32, start);
        m.set_i32(vox, n + 1);
    } else {
        let p = m.u32(vox + 8);
        m.set_i32(p + 4 * n as u32, start);
        m.set_i32(vox, n + 1);
    }
    if m.i32(VCS_TRI_EMPTY_START) == m.i32(VCG_TRI_COUNT) {
        m.set_i32(VCG_TRI_COUNT, start + 1);
    }
    if m.i32(VCG_TRI_COUNT) == m.i32(VCS_TRI_MAX) {
        let tc = m.i32(VCG_TRI_COUNT);
        m.set_i32(VCS_TRI_MAX, tc + 10000);
        let old = m.u32(VCG_TRI_LIST);
        let p = m.realloc(old, ((tc + 10000) as u32).wrapping_mul(0x28));
        m.set_u32(VCG_TRI_LIST, p);
        if p == 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x085774b4 VC_decompPoly]: `VC_decompPoly(corners, n, cell)`: a polygon of 3 to 7 vertex indices as triangles
/// (a fan or a strip by the count); the first failing `VC_createTri` answer, else 0. Other counts: 0.
pub fn decomp_poly(m: &mut Mem, poly: u32, n: i32, cell: u32) -> i32 {
    let tris: &[[u32; 3]] = match n {
        3 => &[[0, 1, 2]],
        4 => &[[0, 1, 2], [2, 3, 0]],
        5 => &[[1, 2, 3], [3, 4, 1], [4, 0, 1]],
        6 => &[[0, 1, 5], [1, 2, 5], [2, 4, 5], [2, 3, 4]],
        7 => &[[0, 1, 6], [1, 5, 6], [1, 2, 5], [2, 4, 5], [2, 3, 4]],
        _ => return 0,
    };
    let loc = m.local(12);
    let mut r = 0;
    for t in tris {
        for (k, ix) in t.iter().enumerate() {
            let v = m.u32(poly + 4 * ix);
            m.set_u32(loc + 4 * k as u32, v);
        }
        r = create_tri(m, loc, cell);
        if r != 0 {
            break;
        }
    }
    m.release(12);
    r
}

/// [soft 0x08577920 VC_createVtx]: `VC_createVtx(point, index)`: vertex record `index` gets the point, no triangles, and (for a
/// surface-type polygonisation) the surface point and the gradient normal; the table grows by 5000 when full. 0 or 0x10.
pub fn create_vtx(m: &mut Mem, pt: u32, index: i32) -> i32 {
    let rec = |m: &Mem| m.u32(VCG_VTX_LIST).wrapping_add((index as u32).wrapping_mul(0xb4));
    let r = rec(m);
    for k in 0..3u32 {
        let v = m.f64(pt + 8 * k);
        m.set_f64(r + 8 * k, v);
    }
    m.set_i32(r + 0x78, 0);
    if m.i32(POLY_TYPE) != 0 {
        let s = m.local(0x18);
        eval_surf(m, VCS_MB_LIST, pt, s);
        let r = rec(m);
        for k in 0..3u32 {
            let v = m.f64(s + 8 * k);
            m.set_f64(r + 0x30 + 8 * k, v);
        }
        let nrm = m.local(0x18);
        let step = m.f64(VCS_NORM_DELTA);
        eval_norm(m, VCS_MB_LIST, pt, nrm, step, 10);
        let r = rec(m);
        for k in 0..3u32 {
            let v = m.f64(nrm + 8 * k);
            m.set_f64(r + 0x18 + 8 * k, v);
        }
        m.release(0x30);
    }
    if index == m.i32(VCG_VTX_COUNT) {
        m.set_i32(VCG_VTX_COUNT, index + 1);
    }
    if m.i32(VCG_VTX_COUNT) == m.i32(VCS_VTX_MAX) {
        let c = m.i32(VCG_VTX_COUNT);
        m.set_i32(VCS_VTX_MAX, c + 5000);
        let old = m.u32(VCG_VTX_LIST);
        let p = m.realloc(old, ((c + 5000) as u32).wrapping_mul(0xb4));
        m.set_u32(VCG_VTX_LIST, p);
        if p == 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x08577ba4 VC_vtxLookup]: `VC_vtxLookup(edge, cell, out)`: the vertex index of the cube edge between the two corners
/// `edge[0]`, `edge[1]` (corner numbers 0..7) of the cell at `cell` (x, y): the saved one, or a new one at the first
/// free vertex slot (answer 1, stored in `*out`); 0 when saved. Corner pairs whose common bits are above 6 are
/// not edges of the walk: 0, `*out` not written. The edge number is garbage in the original when both corners differ from
/// their common bits (it is never called so); here 0.
pub fn vtx_lookup(m: &mut Mem, edge: u32, cell: u32, out: u32) -> i32 {
    let (a, b) = (m.i32(edge), m.i32(edge + 4));
    let (mut x, mut y) = (m.i32(cell), m.i32(cell + 4));
    let and = a & b;
    let da = a.wrapping_sub(and);
    let db = b.wrapping_sub(and);
    let which = if da == 0 { db >> 1 } else if db == 0 { da >> 1 } else { 0 };
    match and as u32 {
        3 => {
            x += 1;
            y += 1;
        }
        1 | 5 => x += 1,
        2 | 6 => y += 1,
        _ => {}
    }
    let at = x.wrapping_add(y.wrapping_mul(m.i32(VCS_CUBE_DIM1)));
    let save = if (and as u32) < 4 {
        VCS_VTX_SAVE1
    } else if (and as u32) - 4 <= 2 {
        VCS_VTX_SAVE2
    } else {
        return 0;
    };
    let tab = m.u32(save);
    let row = m.u32(tab + 4 * which as u32);
    let slot = row.wrapping_add((at as u32).wrapping_mul(4));
    let cur = m.i32(slot);
    if cur >= 0 {
        m.set_i32(out, cur);
        return 0;
    }
    let mut start = m.i32(VCS_VTX_EMPTY_START);
    let count = m.i32(VCG_VTX_COUNT);
    if start < count {
        let list = m.u32(VCG_VTX_LIST);
        if m.i32(list.wrapping_add((start as u32).wrapping_mul(0xb4)).wrapping_add(0x78)) != 0 {
            loop {
                start += 1;
                if start >= count || m.i32(list.wrapping_add((start as u32).wrapping_mul(0xb4)).wrapping_add(0x78)) == 0 {
                    break;
                }
            }
            m.set_i32(VCS_VTX_EMPTY_START, start);
        }
    }
    let tab = m.u32(save);
    let row = m.u32(tab + 4 * which as u32);
    let s = m.i32(VCS_VTX_EMPTY_START);
    m.set_i32(row.wrapping_add((at as u32).wrapping_mul(4)), s);
    m.set_i32(out, s);
    m.set_i32(VCS_VTX_EMPTY_START, s + 1);
    1
}

/// [soft 0x08577d00 VC_markEdge]: `VC_markEdge(corners, marks)`: sets `marks[t] = 1` for each of the 12 cube edges whose two corner
/// values (8 words) differ, `t` the edge's word in `vcS_edgeTable`.
pub fn mark_edge(m: &mut Mem, c: u32, marks: u32) {
    // (corner, corner, offset in the edge table), in the original's order
    const E: [(u32, u32, u32); 12] = [
        (0, 1, 4), (0, 2, 8), (0, 4, 16), (5, 1, 164), (5, 7, 188), (5, 4, 176),
        (2, 0, 64), (2, 3, 76), (2, 6, 88), (7, 5, 244), (7, 3, 236), (7, 6, 248),
    ];
    for (a, b, off) in E {
        if m.i32(c + 4 * a) != m.i32(c + 4 * b) {
            let t = m.u32(VCS_EDGE_TABLE + off);
            m.set_i32(marks.wrapping_add(4 * t), 1);
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------
pub const VCS_CUBE_DIM: u32 = 0x0882_9a00;
pub const VCS_DENS_S1: u32 = 0x0882_9a0c;
pub const VCS_DENS_S2: u32 = 0x0882_9a10;
pub const VCS_MAXMB_COPY: u32 = 0x0882_9a20;
pub const VCS_MAXMB_ARRAY: u32 = 0x0882_9a24;
pub const VCS_ZSTEP: u32 = 0x0882_9a28;
pub const VCS_AT_ZSLICE: u32 = 0x0882_9a3c;
pub const VCS_UPDATE_FLAG: u32 = 0x0882_9a40;
/// `vcS_IntUpBv`: six words (the low corner x, y, z at 0x44, 0x48, 0x4c; the high corner at 0x50, 0x54, 0x58).
pub const VCS_INT_UPBV: u32 = 0x0882_9a44;
pub const VCS_TRACE_TABLE: u32 = 0x0882_9a80;
pub const VCS_EDGE_TO_VTX: u32 = 0x0882_9c80;
pub const VCS_NORM_MAGIC: u32 = 0x086e_e58c;
pub const FIRST_INIT_MBTOP: u32 = 0x086e_e590;

/// [soft 0x08577a2c VC_getPt]: `VC_getPt(out, edge, corners, values)`: the point on the cube edge between corners `edge[0]` and
/// `edge[1]` where the linear interpolation of `values` reaches 1; for a surface-type polygonisation moved by
/// `VC_bi_search` to the density 1 when the density there is not on the interpolated side.
pub fn get_pt(m: &mut Mem, out: u32, edge: u32, corners: u32, vals: u32) {
    let (i1, i2) = (m.u32(edge), m.u32(edge + 4));
    let va = f(m, vals.wrapping_add(8 * i1));
    let vb = f(m, vals.wrapping_add(8 * i2));
    let loc = m.local(0x30);
    // the two corners' coordinates, as the original keeps them: a at +0, b at +0x18
    for k in 0..3u32 {
        let a = m.f64(corners.wrapping_add(24 * i1 + 8 * k));
        m.set_f64(loc + 8 * k, a);
        let b = m.f64(corners.wrapping_add(24 * i2 + 8 * k));
        m.set_f64(loc + 0x18 + 8 * k, b);
    }
    let (ca, cb) = (loc, loc + 0x18);
    let one = F80::ONE;
    let t = if !va.is_nan() && !vb.is_nan() && vb.eq(va) { F80::ZERO } else { (one - va) / (vb - va) };
    let u = one - t;
    for k in 0..3u32 {
        let r = (t * f(m, cb + 8 * k)) + (u * f(m, ca + 8 * k));
        m.set_f64(out + 8 * k, r.to_f64());
    }
    if m.i32(POLY_TYPE) != 0 {
        let d = eval_dens(m, VCS_MB_LIST, out);
        if d.lt(one) {
            if vb.gt(one) {
                bi_search(m, VCS_MB_LIST, out, cb);
            } else if one.lt(va) {
                bi_search(m, VCS_MB_LIST, out, ca);
            }
        } else if vb.lt(one) {
            bi_search(m, VCS_MB_LIST, cb, out);
        } else if one.gt(va) {
            bi_search(m, VCS_MB_LIST, ca, out);
        }
    }
    m.release(0x30);
}

/// [soft 0x08577294 VC_traceCube]: `VC_traceCube(flags, cell, corners, values)`: the triangles of one cube: its edges whose
/// corner flags differ are walked around each face loop, a vertex made (or found) for each, and the polygon
/// of 3 to 12 vertices decomposed into triangles. 0, or the first failing answer (0x10).
pub fn trace_cube(m: &mut Mem, flags: u32, cell: u32, corners: u32, vals: u32) -> i32 {
    let base = m.local(0x30 + 0x120 + 0x30 + 8);
    let (marks, ver, ids, out) = (base, base + 0x30, base + 0x150, base + 0x180);
    for k in 0..12u32 {
        m.set_i32(marks + 4 * k, 0);
    }
    mark_edge(m, flags, marks);
    let tt = |m: &Mem, a: i32, b: i32| m.i32(VCS_TRACE_TABLE + 4 * (a * 8 + b) as u32);
    let mut e = 0i32;
    let r = 'outer: loop {
        if m.i32(marks + 4 * e as u32) == 1 {
            let mut n = 0i32;
            let mut cur = e;
            loop {
                m.set_i32(marks + 4 * cur as u32, 0);
                let pair = VCS_EDGE_TO_VTX + (cur as u32) * 8;
                let slot = ver + 24 * n as u32;
                if vtx_lookup(m, pair, cell, out) == 1 {
                    get_pt(m, slot, pair, corners, vals);
                    let r = create_vtx(m, slot, m.i32(out));
                    if r != 0 {
                        break 'outer r;
                    }
                    let o = m.u32(out);
                    m.set_u32(ids + 4 * n as u32, o);
                } else {
                    let list = m.u32(VCG_VTX_LIST);
                    let v = list.wrapping_add(m.u32(out).wrapping_mul(0xb4));
                    for k in 0..3u32 {
                        let x = m.f64(v + 8 * k);
                        m.set_f64(slot + 8 * k, x);
                    }
                    let o = m.u32(out);
                    m.set_u32(ids + 4 * n as u32, o);
                }
                let a = m.i32(pair);
                let b = m.i32(pair + 4);
                let (mut i4, mut i3) = (a, b);
                let pf = |m: &Mem, c: i32| m.i32(flags.wrapping_add(4 * c as u32));
                if pf(m, a) == 0 && pf(m, b) == 1 {
                    i3 = a;
                    i4 = b;
                }
                n += 1;
                let mut x = tt(m, i4, i3);
                let mut v = pf(m, x);
                let mut i1;
                loop {
                    i1 = x;
                    if v == 1 {
                        break;
                    }
                    let y = tt(m, i3, i1);
                    v = pf(m, y);
                    x = y;
                    i3 = i1;
                }
                cur = m.i32(VCS_EDGE_TABLE + 4 * (i3 * 8 + i1) as u32);
                if cur == e {
                    break;
                }
            }
            if n > 2 {
                let r = decomp_poly(m, ids, n, cell);
                if r != 0 {
                    break 'outer r;
                }
            }
        }
        e += 1;
        if e > 11 {
            break 0;
        }
    };
    m.release(0x30 + 0x120 + 0x30 + 8);
    r
}

/// [soft 0x08576df4 VC_leaf_action]: `VC_leaf_action(centre, dx, dy, cell, size)`: the four squares around `centre` (two by two,
/// `size` cells apart) of one z slice: the densities at their eight corners (this slice's from the cache `vcS_densS2`, the slice
/// below from `vcS_densS1`, evaluated where the cache is empty), and `VC_traceCube` for each cube whose corners
/// are not all on one side of density 1. 0, or the first failing answer.
pub fn leaf_action(m: &mut Mem, p1: u32, dx: f64, dy: f64, cell: u32, s: i32) -> i32 {
    let frame = m.local(0x210);
    let (pos, scr, d, flags, fp, g, ct) = (frame, frame + 0x60, frame + 0xc0, frame + 0x100, frame + 0x120, frame + 0x140, frame + 0x200);
    let z = m.f64(p1 + 16);
    for k in 0..4u32 {
        m.set_f64(pos + 24 * k + 16, z);
    }
    let (fdx, fdy) = (F80::from_f64(dx), F80::from_f64(dy));
    let (cx, cy) = (m.i32(cell), m.i32(cell + 4));
    let c_empty = F80::from_f64(1_000_000.0);
    let zstep = |m: &Mem| f(m, VCS_ZSTEP);
    let mut j = 0i32;
    let r = 'outer: loop {
        let dya = (F80::from_i32(j - 1) * fdy).to_f64();
        let mut i = 0i32;
        while i < 2 {
            let x0 = cx.wrapping_add((i - 1).wrapping_mul(s));
            let y0 = cy.wrapping_add((j - 1).wrapping_mul(s));
            let x1 = cx.wrapping_add(i.wrapping_mul(s));
            let y1 = cy.wrapping_add(j.wrapping_mul(s));
            let px = f(m, p1);
            let py = f(m, p1 + 8);
            let sx = (F80::from_i32(i - 1) * fdx + px).to_f64();
            let tx = (px + F80::from_i32(i) * fdx).to_f64();
            let uy = (py + F80::from_f64(dya)).to_f64();
            let vy = (F80::from_i32(j) * fdy + py).to_f64();
            for (k, (x, y)) in [(sx, uy), (tx, uy), (sx, vy), (tx, vy)].into_iter().enumerate() {
                m.set_f64(pos + 24 * k as u32, x);
                m.set_f64(pos + 24 * k as u32 + 8, y);
            }
            for (k, v) in [x0, y0, x1, y0, x0, y1, x1, y1].into_iter().enumerate() {
                m.set_i32(fp + 4 * k as u32, v);
            }
            let take = if m.i32(VCS_UPDATE_FLAG) == 1 {
                !(m.i32(VCS_INT_UPBV + 0xc) < x1 || x0 < m.i32(VCS_INT_UPBV) || m.i32(VCS_INT_UPBV + 0x10) < y1) && y0 >= m.i32(VCS_INT_UPBV + 4)
            } else {
                true
            };
            if take {
                for k in 0..4u32 {
                    let (cxk, cyk) = (m.i32(fp + 8 * k), m.i32(fp + 8 * k + 4));
                    let at = (cyk.wrapping_mul(m.i32(VCS_CUBE_DIM1)).wrapping_add(cxk) as u32).wrapping_mul(8);
                    let pk = pos + 24 * k;
                    let c2 = m.u32(VCS_DENS_S2).wrapping_add(at);
                    let v2 = f(m, c2);
                    if v2.lt(c_empty) {
                        m.set_f64(d + 8 * (k + 4), v2.to_f64());
                    } else {
                        let v = eval_dens(m, VCS_MB_LIST, pk).to_f64();
                        m.set_f64(d + 8 * (k + 4), v);
                        let c2 = m.u32(VCS_DENS_S2).wrapping_add(at);
                        m.set_f64(c2, v);
                    }
                    let c1 = m.u32(VCS_DENS_S1).wrapping_add(at);
                    let v1 = f(m, c1);
                    if v1.lt(c_empty) {
                        m.set_f64(d + 8 * k, v1.to_f64());
                    } else {
                        let sk = scr + 24 * k;
                        let (a, b, c) = (m.f64(pk), m.f64(pk + 8), f(m, pk + 16) - zstep(m));
                        m.set_f64(sk, a);
                        m.set_f64(sk + 8, b);
                        m.set_f64(sk + 16, c.to_f64());
                        let v = eval_dens(m, VCS_MB_LIST, sk).to_f64();
                        m.set_f64(d + 8 * k, v);
                        let c1 = m.u32(VCS_DENS_S1).wrapping_add(at);
                        m.set_f64(c1, v);
                    }
                }
                for k in 0..8u32 {
                    let below = F80::ONE.gt(f(m, d + 8 * k));
                    m.set_i32(flags + 4 * k, below as i32);
                }
                let f0 = m.i32(flags);
                if (1..8u32).any(|k| m.i32(flags + 4 * k) != f0) {
                    m.set_i32(ct, x0);
                    m.set_i32(ct + 4, y0);
                    m.set_i32(ct + 8, m.i32(VCS_AT_ZSLICE).wrapping_sub(1));
                    for k in 0..4u32 {
                        let (a, b) = (m.f64(pos + 24 * k), m.f64(pos + 24 * k + 8));
                        let pz = f(m, pos + 24 * k + 16);
                        for (idx, zv) in [(k, (pz - zstep(m)).to_f64()), (k + 4, pz.to_f64())] {
                            m.set_f64(g + 24 * idx, a);
                            m.set_f64(g + 24 * idx + 8, b);
                            m.set_f64(g + 24 * idx + 16, zv);
                        }
                    }
                    let r = trace_cube(m, flags, ct, g, d);
                    if r != 0 {
                        break 'outer r;
                    }
                }
            }
            i += 1;
        }
        j += 1;
        if j > 1 {
            break 0;
        }
    };
    m.release(0x210);
    r
}

/// [soft 0x08576c2c VC_quad_divide]: `VC_quad_divide(centre, dx, dy, cell, size)`: the quadtree descent: a square of more than one cell
/// that lies in the updated box and that some metaball can reach is split into its four halves (the halved steps
/// are stored back over the arguments), a one-cell square goes to `VC_leaf_action`. The four recursive answers are
/// dropped; only the leaf's answer is returned (0 otherwise).
pub fn quad_divide(m: &mut Mem, p1: u32, dx: f64, dy: f64, cell: u32, n: i32) -> i32 {
    if n < 2 {
        return leaf_action(m, p1, dx, dy, cell, n);
    }
    let ok = if m.i32(VCS_UPDATE_FLAG) == 1 { check_upbv(m, VCS_INT_UPBV, cell, n) as i16 == 1 } else { true };
    if !ok || influ_test(m, VCS_MB_LIST, p1, dx, dy) as i16 != 1 {
        return 0;
    }
    let half = F80::from_f64(0.5);
    let dxh_ext = F80::from_f64(dx) * half;
    let dyh_ext = half * F80::from_f64(dy);
    let (dxh, dyh) = (dxh_ext.to_f64(), dyh_ext.to_f64());
    let h = (n + ((n as u32) >> 31) as i32) >> 1;
    let loc = m.local(0x20);
    let (c, cl) = (loc, loc + 0x18);
    let z = m.f64(p1 + 16);
    m.set_f64(c + 16, z);
    let (px, py) = (f(m, p1), f(m, p1 + 8));
    let (cx, cy) = (m.i32(cell), m.i32(cell + 4));
    let (fx, fy) = (F80::from_f64(dxh), F80::from_f64(dyh));
    // (x, y, cell x, cell y) of the four sub squares, in the original's order
    let subs = [
        ((px - dxh_ext).to_f64(), (py - fy).to_f64(), cx.wrapping_sub(h), cy.wrapping_sub(h)),
        ((fx + px).to_f64(), (py - fy).to_f64(), cx.wrapping_add(h), cy.wrapping_sub(h)),
        ((px - fx).to_f64(), (fy + py).to_f64(), cx.wrapping_sub(h), cy.wrapping_add(h)),
        ((fx + px).to_f64(), (fy + py).to_f64(), cx.wrapping_add(h), cy.wrapping_add(h)),
    ];
    let _ = dyh_ext;
    for (x, y, ix, iy) in subs {
        m.set_f64(c, x);
        m.set_f64(c + 8, y);
        m.set_i32(cl, ix);
        m.set_i32(cl + 4, iy);
        quad_divide(m, c, dxh, dyh, cl, h);
    }
    m.release(0x20);
    0
}

/// [soft 0x085763e0 VC_InitMbtoP]: `VC_InitMbtoP()`: the vertex and triangle tables (900000 and 400000 bytes, every triangle slot
/// marked free), their counts and sizes, and the cube's traversal, edge-number and edge-corner tables. 0x13 on the
/// second call (the flag), 0x10 when an allocation fails.
pub fn init_mbto_p(m: &mut Mem) -> i32 {
    if m.i32(FIRST_INIT_MBTOP) == 0 {
        return 0x13;
    }
    m.set_i32(FIRST_INIT_MBTOP, 0);
    let p = m.malloc(900_000);
    m.set_u32(VCG_VTX_LIST, p);
    if p == 0 {
        return 0x10;
    }
    let t = m.malloc(400_000);
    m.set_u32(VCG_TRI_LIST, t);
    // (the original marks the slots before testing the pointer)
    for i in (0..10_000u32).rev() {
        m.set_i16(t.wrapping_add(i * 0x28 + 0x24), 1);
    }
    if t == 0 {
        return 0x10;
    }
    m.set_i32(VCG_VTX_COUNT, 0);
    m.set_i32(VCG_TRI_COUNT, 0);
    m.set_i32(VCS_VTX_MAX, 5000);
    m.set_i32(VCS_TRI_MAX, 10000);
    const T: [(u32, i32); 72] = [
    (0x08829a84, 3), (0x08829a88, 6), (0x08829a90, 5), (0x08829aa0, 4), (0x08829aac, 2), (0x08829ab4, 7), 
    (0x08829ac0, 1), (0x08829acc, 7), (0x08829ad8, 4), (0x08829ae8, 0), (0x08829ae4, 5), (0x08829afc, 6), 
    (0x08829b00, 2), (0x08829b14, 1), (0x08829b18, 7), (0x08829b24, 0), (0x08829b30, 6), (0x08829b3c, 3), 
    (0x08829b48, 3), (0x08829b50, 0), (0x08829b5c, 5), (0x08829b6c, 1), (0x08829b74, 4), (0x08829b78, 2), 
    (0x08829b84, 0), (0x08829ba0, 0), (0x08829bac, 1), (0x08829be4, 1), (0x08829be8, 2), (0x08829bcc, 2), 
    (0x08829bc0, 3), (0x08829b88, 3), (0x08829b90, 4), (0x08829c00, 4), (0x08829bb4, 5), (0x08829c24, 5), 
    (0x08829bfc, 6), (0x08829c6c, 6), (0x08829bd8, 7), (0x08829c48, 7), (0x08829c14, 8), (0x08829c30, 8), 
    (0x08829c3c, 9), (0x08829c74, 9), (0x08829c78, 10), (0x08829c5c, 10), (0x08829c50, 11), (0x08829c18, 11), 
    (0x08829c80, 0), (0x08829c84, 1), (0x08829c88, 1), (0x08829c8c, 3), (0x08829c90, 3), (0x08829c94, 2), 
    (0x08829c98, 2), (0x08829c9c, 0), (0x08829ca0, 0), (0x08829ca4, 4), (0x08829ca8, 1), (0x08829cac, 5), 
    (0x08829cb0, 3), (0x08829cb4, 7), (0x08829cb8, 2), (0x08829cbc, 6), (0x08829cc0, 4), (0x08829cc4, 5), 
    (0x08829cc8, 5), (0x08829ccc, 7), (0x08829cd0, 7), (0x08829cd4, 6), (0x08829cd8, 6), (0x08829cdc, 4)];
    for (a, v) in T {
        m.set_i32(a, v);
    }
    0
}

/// [soft 0x08576760 VC_mbtoP]: `VC_mbtoP(level, scale, bounds, polyType, box, update)`: the polygonisation of the whole field:
/// `2^level` cells per side, the slices walked from bottom to top with two density layers and three layers of
/// saved vertices, each slice by `VC_quad_divide` from the middle of the bounds. 0 or the first failing answer
/// (0x10 when an allocation fails: the buffers made so far are not freed, as in the original).
pub fn mbto_p(m: &mut Mem, level: i32, scale: f64, bounds: u32, poly_type: i32, boxp: u32, update: i16) -> i32 {
    m.set_i32(VCS_VTX_EMPTY_START, 0);
    m.set_i32(VCS_TRI_EMPTY_START, 0);
    // (1 doubled `level` times; 1 for a level of 0 or less)
    let mut cube = 1i32;
    for _ in 0..level.max(0) {
        cube = cube.wrapping_add(cube);
    }
    m.set_i32(VCS_CUBE_DIM, cube);
    m.set_i32(VCS_CUBE_VOX_RATIO, (if cube < 0 { cube.wrapping_add(7) } else { cube }) >> 3);
    m.set_i32(POLY_TYPE, poly_type);
    m.set_i32(VCS_UPDATE_FLAG, update as i32);
    let c = F80::from_i32(cube);
    let nz = (c * F80::from_f64(scale)).to_i32(Round::Zero);
    let b = |m: &Mem, k: u32| f(m, bounds + 8 * k);
    let xs = (b(m, 3) - b(m, 0)) / c;
    let ys = (b(m, 4) - b(m, 1)) / c;
    let zs = (b(m, 5) - b(m, 2)) / F80::from_i32(nz);
    let steps = m.local(0x18);
    m.set_f64(steps, xs.to_f64());
    m.set_f64(steps + 8, ys.to_f64());
    m.set_f64(steps + 16, zs.to_f64());
    let mut mx = if xs.gt(ys) { xs } else { ys };
    if zs.gt(mx) {
        mx = zs;
    }
    m.set_f64(VCS_ZSTEP, zs.to_f64());
    let nd = mx / F80::from_i32(m.i32(VCS_NORM_MAGIC));
    m.set_f64(VCS_NORM_DELTA, nd.to_f64());
    let half = (cube + ((cube as u32) >> 31) as i32) >> 1;
    let h = F80::from_i32(half);
    let dxp = (xs * h).to_f64();
    let dyp = (ys * h).to_f64();
    let centre = m.local(0x18);
    m.set_f64(centre, ((xs * h) + b(m, 0)).to_f64());
    m.set_f64(centre + 8, ((ys * h) + b(m, 1)).to_f64());
    let cellp = m.local(8);
    m.set_i32(cellp, half);
    m.set_i32(cellp + 4, half);
    let r = mbto_p_body(m, steps, centre, cellp, dxp, dyp, half, cube, nz, bounds, boxp);
    m.release(0x18 + 0x18 + 8);
    r
}

fn mbto_p_body(m: &mut Mem, steps: u32, centre: u32, cellp: u32, dxp: f64, dyp: f64, half: i32, cube: i32, nz: i32, bounds: u32, boxp: u32) -> i32 {
    let maxmb = m.u32(VCG_MAX_MBS);
    m.set_u32(VCS_MAXMB_COPY, maxmb);
    let p = m.malloc(maxmb << 2);
    m.set_u32(VCS_MAXMB_ARRAY, p);
    if p == 0 {
        return 0x10;
    }
    let dim1 = cube.wrapping_add(1);
    m.set_i32(VCS_CUBE_DIM1, dim1);
    let cells = dim1.wrapping_mul(dim1);
    let cu = cells as u32;
    let p = m.malloc(cu.wrapping_mul(8));
    m.set_u32(VCS_DENS_S1, p);
    if p == 0 {
        return 0x10;
    }
    let p = m.malloc(cu.wrapping_mul(8));
    m.set_u32(VCS_DENS_S2, p);
    if p == 0 {
        return 0x10;
    }
    let empty = 1_000_000.0f64;
    for i in 0..cells.max(0) as u32 {
        let (a, b) = (m.u32(VCS_DENS_S1), m.u32(VCS_DENS_S2));
        m.set_f64(a + 8 * i, empty);
        m.set_f64(b + 8 * i, empty);
    }
    let s1 = m.malloc(12);
    m.set_u32(VCS_VTX_SAVE1, s1);
    for k in 0..3u32 {
        let p = m.malloc(cu.wrapping_mul(4));
        m.set_u32(m.u32(VCS_VTX_SAVE1) + 4 * k, p);
    }
    if m.u32(m.u32(VCS_VTX_SAVE1) + 8) == 0 {
        return 0x10;
    }
    let s2 = m.malloc(8);
    m.set_u32(VCS_VTX_SAVE2, s2);
    for k in 0..2u32 {
        let p = m.malloc(cu.wrapping_mul(4));
        m.set_u32(m.u32(VCS_VTX_SAVE2) + 4 * k, p);
    }
    if m.u32(m.u32(VCS_VTX_SAVE2) + 4) == 0 {
        return 0x10;
    }
    for k in 0..3u32 {
        let row = m.u32(m.u32(VCS_VTX_SAVE1) + 4 * k);
        for i in 0..cells.max(0) as u32 {
            m.set_i32(row + 4 * i, -1);
        }
    }
    for k in 0..2u32 {
        let row = m.u32(m.u32(VCS_VTX_SAVE2) + 4 * k);
        for i in 0..cells.max(0) as u32 {
            m.set_i32(row + 4 * i, -1);
        }
    }
    if m.i32(VCS_UPDATE_FLAG) == 0 {
        let r = init_mbto_p(m);
        if r != 0 {
            return r;
        }
    } else {
        let cr = m.i32(VCS_CUBE_VOX_RATIO);
        rm_tri_in_upbv(m, cr, steps, bounds, boxp, VCS_INT_UPBV);
    }
    let mut z = 1i32;
    while z < nz {
        m.set_i32(VCS_AT_ZSLICE, z);
        let zv = (F80::from_i32(z) * f(m, steps + 16) + f(m, bounds + 16)).to_f64();
        m.set_f64(centre + 16, zv);
        let run = m.i32(VCS_UPDATE_FLAG) == 0 || !(z > m.i32(VCS_INT_UPBV + 0x14) || z - 1 < m.i32(VCS_INT_UPBV + 8));
        if run {
            let zstep = m.f64(VCS_ZSTEP);
            extract_zslab(m, VCS_MB_LIST, zv, zstep);
            let (a, b) = (m.u32(VCS_DENS_S1), m.u32(VCS_DENS_S2));
            m.set_u32(VCS_DENS_S1, b);
            m.set_u32(VCS_DENS_S2, a);
            for i in 0..cells.max(0) as u32 {
                m.set_f64(a + 8 * i, empty);
            }
            let (v1, v2) = (m.u32(VCS_VTX_SAVE1), m.u32(VCS_VTX_SAVE2));
            for k in 0..2u32 {
                let (r1, r2) = (m.u32(v1 + 4 * k), m.u32(v2 + 4 * k));
                for i in 0..cells.max(0) as u32 {
                    let x = m.u32(r2 + 4 * i);
                    m.set_u32(r1 + 4 * i, x);
                    m.set_i32(r2 + 4 * i, -1);
                }
            }
            let r3 = m.u32(v1 + 8);
            for i in 0..cells.max(0) as u32 {
                m.set_i32(r3 + 4 * i, -1);
            }
            if m.i32(VCS_MB_LIST) > 0 {
                let r = quad_divide(m, centre, dxp, dyp, cellp, half);
                if r != 0 {
                    return r;
                }
            }
        }
        z += 1;
    }
    free_buffers(m);
    0
}

fn free_buffers(m: &mut Mem) {
    for g in [VCS_DENS_S1, VCS_DENS_S2] {
        let p = m.u32(g);
        m.free(p);
    }
    let s1 = m.u32(VCS_VTX_SAVE1);
    for k in 0..3u32 {
        let p = m.u32(s1 + 4 * k);
        m.free(p);
    }
    m.free(s1);
    let s2 = m.u32(VCS_VTX_SAVE2);
    for k in 0..2u32 {
        let p = m.u32(s2 + 4 * k);
        m.free(p);
    }
    m.free(s2);
    let p = m.u32(VCS_MAXMB_ARRAY);
    m.free(p);
    m.set_u32(VCS_MAXMB_COPY, 0);
}
