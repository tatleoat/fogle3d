// SPDX-License-Identifier: MIT
//! `particle_data.c` of Softimage|3D 4.0 (0x084c2f30 … 0x084c3fd0): the particle data blocks (finding 0045).
//!
//! A particle data block (`SI_ParticleData`, 12 bytes, our name) is `{ dim: i32, rows: i32, data: *f32 }`: `rows`
//! records of `dim` floats. The functions call the C library's `malloc`, `free`, `fprintf`, `srand48` and `drand48`;
//! the first two are the image's, printing goes through [`Outside::print`], the generator is [`Drand`].

use crate::outside::DynaCalls;
use crate::particle_mf::{
    si_particle_cross_product, si_particle_dot_product, si_particle_norm, si_particle_normalize,
    si_particle_transform_point, Drand,
};
use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

/// The `SI_frameRate` global (a float).
const SI_FRAME_RATE: u32 = 0x0878_204c;

fn f(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// Allocates the data of a block [soft 0x084c3c44 si_particleDataAllocate]: 0 (nothing done) when `dim` or `rows` is
/// not positive, else `malloc(rows * dim * 4)` is stored in the block: -1 when it fails, 1 when it works.
pub fn si_particle_data_allocate(m: &mut Mem, pd: u32) -> i32 {
    let (dim, rows) = (m.i32(pd), m.i32(pd + 4));
    if dim < 1 || rows < 1 {
        return 0;
    }
    let p = m.malloc((rows.wrapping_mul(dim) as u32).wrapping_mul(4));
    m.set_u32(pd + 8, p);
    if p == 0 {
        -1
    } else {
        1
    }
}

/// Zeroes the three words of a block [soft 0x084c3cbc si_particleDataInitialize].
pub fn si_particle_data_initialize(m: &mut Mem, pd: u32) {
    m.set_u32(pd, 0);
    m.set_u32(pd + 4, 0);
    m.set_u32(pd + 8, 0);
}

/// Frees the data of a block and zeroes it [soft 0x084c3c8c si_particleDataFree].
pub fn si_particle_data_free(m: &mut Mem, pd: u32) {
    let p = m.u32(pd + 8);
    if p != 0 {
        m.free(p);
    }
    si_particle_data_initialize(m, pd);
}

/// `fprintf("%f")` of a double: six decimals; the library prints inf / nan in lower case, with the sign of a NaN.
fn fmt_f(v: f64) -> String {
    if v.is_nan() {
        if v.is_sign_negative() { "-nan" } else { "nan" }.to_string()
    } else if v.is_infinite() {
        if v < 0.0 { "-inf" } else { "inf" }.to_string()
    } else {
        format!("{:.6}", v)
    }
}

/// Writes a block as text [soft 0x084c3cdc particleWriteData]: `"%d  %d\n"` of dim and rows, then each row as the
/// floats (as doubles) with `"%f  "` and a newline.
pub fn particle_write_data(m: &mut Mem, o: &mut dyn Outside, file: u32, pd: u32) {
    o.print(file, format!("{}  {}\n", m.i32(pd), m.i32(pd + 4)).as_bytes());
    let mut r = 0i32;
    while r < m.i32(pd + 4) {
        let dim = m.i32(pd);
        let mut c = 0;
        while c < dim {
            let v = m.f32(m.u32(pd + 8).wrapping_add((c.wrapping_add(dim.wrapping_mul(r)) as u32).wrapping_mul(4)));
            o.print(file, format!("{}  ", fmt_f(v as f64)).as_bytes());
            c += 1;
        }
        o.print(file, b"\n");
        r += 1;
    }
}

/// Takes the points of a mesh [soft 0x084c3d60 si_particleMeshToPoint]: the block becomes 3 floats by the mesh's
/// vertex count (+0x14), reallocated when it was another shape (-1 when that fails); each vertex's first three floats
/// (records of 0x14 bytes at mesh+0) are copied. Returns 1.
pub fn si_particle_mesh_to_point(m: &mut Mem, mesh: u32, pd: u32) -> i32 {
    let n = m.i32(mesh + 0x14);
    if m.i32(pd + 4) != n || m.i32(pd) != 3 {
        si_particle_data_free(m, pd);
        m.set_i32(pd + 4, n);
        m.set_i32(pd, 3);
        if si_particle_data_allocate(m, pd) < 0 {
            return -1;
        }
    }
    let mut k = 0u32;
    while (k as i32) < n {
        let src = m.u32(mesh).wrapping_add(0x14 * k);
        let dst = m.u32(pd + 8).wrapping_add(12 * k);
        for c in 0..3 {
            let v = m.u32(src + 4 * c);
            m.set_u32(dst + 4 * c, v);
        }
        k += 1;
    }
    1
}

/// Interpolates two blocks [soft 0x084c3df4 particleDataInterp]: -1 when their shapes differ; the result block is
/// reshaped (freed and reallocated; the allocation's answer is ignored) when it is another shape; the weight t is
/// clamped to [0, 1] (a NaN stays), and each float is `((1 - t) * a) + (t * b)` (80-bit sum, one rounded store),
/// with `1 - t` kept unrounded. Returns 1.
pub fn particle_data_interp(m: &mut Mem, a: u32, b: u32, t: f32, out: u32) -> i32 {
    let (rows, dim) = (m.i32(a + 4), m.i32(a));
    if rows != m.i32(b + 4) || dim != m.i32(b) {
        return -1;
    }
    if m.i32(out + 4) != rows || m.i32(out) != dim {
        si_particle_data_free(m, out);
        m.set_i32(out + 4, rows);
        m.set_i32(out, dim);
        si_particle_data_allocate(m, out);
    }
    let mut t = t;
    if t < 0.0 {
        t = 0.0;
    }
    if 1.0 < t {
        t = 1.0;
    }
    let omt = x(1.0f32) - x(t);
    let total = dim.wrapping_mul(rows);
    let mut k = 0i32;
    while k < total {
        let o = (k as u32).wrapping_mul(4);
        let v = omt * f(m, m.u32(a + 8).wrapping_add(o)) + x(t) * f(m, m.u32(b + 8).wrapping_add(o));
        let dst = m.u32(out + 8).wrapping_add(o);
        m.set_f32(dst, v.to_f32());
        k += 1;
    }
    1
}

/// Transforms every point of a block by a matrix [soft 0x084c3eac si_particleDataSingleTransform]: -1 when the dim is
/// not a multiple of 3 (or the output cannot be allocated); the output takes the input's shape, and each triple is
/// done by [`si_particle_transform_point`]. Returns 1.
pub fn si_particle_data_single_transform(m: &mut Mem, mat: u32, src: u32, dst: u32) -> i32 {
    let dim = m.i32(src);
    if dim != (dim / 3) * 3 {
        return -1;
    }
    let rows = m.i32(src + 4);
    let n = dim.wrapping_mul(rows) / 3;
    if m.i32(dst) != dim || m.i32(dst + 4) != rows {
        si_particle_data_free(m, dst);
        let (d, r) = (m.i32(src), m.i32(src + 4));
        m.set_i32(dst, d);
        m.set_i32(dst + 4, r);
        if si_particle_data_allocate(m, dst) < 0 {
            return -1;
        }
    }
    let mut k = 0u32;
    while (k as i32) < n {
        let (s, d) = (m.u32(src + 8).wrapping_add(12 * k), m.u32(dst + 8).wrapping_add(12 * k));
        si_particle_transform_point(m, mat, s, d);
        k += 1;
    }
    1
}

/// Adds a random offset to every row of a block [soft 0x084c3f64 si_particleDataJitter]: for each row one `drand48`
/// (rounded to f32), and each of the `dim` floats gets `(d * amount[c]) + value` (rounded store).
pub fn si_particle_data_jitter(m: &mut Mem, r: &mut dyn Drand, pd: u32, amount: u32) {
    let (rows, dim) = (m.i32(pd + 4), m.i32(pd));
    let mut base = 0i32;
    let mut row = 0;
    while row < rows {
        let d = x(r.drand48() as f32);
        let mut c = 0;
        while c < dim {
            let a = m.u32(pd + 8).wrapping_add((base.wrapping_add(c) as u32).wrapping_mul(4));
            let v = d * f(m, amount + 4 * c as u32) + f(m, a);
            m.set_f32(a, v.to_f32());
            c += 1;
        }
        base = base.wrapping_add(dim);
        row += 1;
    }
}

/// Takes the triangles of a mesh [soft 0x084c3a24 si_particleMeshToTri]: a mesh (vertices at +0 with 0x14 bytes each,
/// polygons at +4 with 0x18 bytes each: corner records of 0x24 bytes at +0xc, the corner count as a short at +0x10,
/// vertex count +0x14, polygon count +0x18) with a polygon that is not a triangle is copied and triangulated
/// first (disposed at the end). The block becomes 9 floats by the number of triangles; each triangle's three corners'
/// vertex positions are copied. -1 if the allocation fails, else 1.
pub fn si_particle_mesh_to_tri(m: &mut Mem, o: &mut dyn Outside, mesh: u32, pd: u32) -> i32 {
    let mut all_tri = true;
    let np = m.i32(mesh + 0x18);
    if np >= 1 {
        let polys = m.u32(mesh + 4);
        let mut i = 0i32;
        loop {
            if m.i16(polys.wrapping_add(0x10).wrapping_add(0x18u32.wrapping_mul(i as u32))) != 3 {
                all_tri = false;
                break;
            }
            i += 1;
            if np <= i {
                break;
            }
        }
    }
    let slot = m.local(4);
    m.set_u32(slot, mesh);
    if !all_tri {
        let copy = o.si_mesh_copy(m, mesh);
        m.set_u32(slot, copy);
        o.si_mesh_triangulate(m, copy, 1);
    }
    let work = m.u32(slot);
    let n = m.i32(work + 0x18);
    let mut tris = 0i32;
    let polys = m.u32(work + 4);
    let mut i = 0i32;
    while i < n {
        if m.i16(polys.wrapping_add(0x10).wrapping_add(0x18u32.wrapping_mul(i as u32))) == 3 {
            tris += 1;
        }
        i += 1;
    }
    if m.i32(pd) != 9 || m.i32(pd + 4) != tris {
        si_particle_data_free(m, pd);
        m.set_i32(pd, 9);
        m.set_i32(pd + 4, tris);
        if si_particle_data_allocate(m, pd) < 0 {
            if !all_tri {
                o.si_mesh_dispose(m, slot);
            }
            m.release(4);
            return -1;
        }
    }
    let mut at = 0u32;
    let mut i = 0i32;
    while i < n {
        let poly = polys.wrapping_add(0x18u32.wrapping_mul(i as u32));
        if m.i16(poly + 0x10) == 3 {
            let corners = m.u32(poly + 0xc);
            for c in 0..3u32 {
                let vi = m.u32(corners + 0x24 * c).wrapping_mul(0x14);
                for k in 0..3u32 {
                    let v = m.u32(m.u32(work).wrapping_add(vi).wrapping_add(4 * k));
                    let dst = m.u32(pd + 8).wrapping_add(4 * (at + 3 * c + k));
                    m.set_u32(dst, v);
                }
            }
            at += 9;
        }
        i += 1;
    }
    if !all_tri {
        o.si_mesh_dispose(m, slot);
    }
    m.release(4);
    1
}

/// Picks random points on the triangles of a block of triangles (9 floats a row) [soft 0x084c3798 si_particleTriSample]:
/// `out.rows` points (3 floats each, into out's data) are made after `srand48(seed)`. A local table holds one
/// number a triangle: with `uniform` 0 the areas (norm of the cross product of two edges from corner 0, times 0.5,
/// rounded to f32), divided by their 80-bit sum and made cumulative (80-bit running sum, rounded stores); otherwise
/// the rounded `i / n` steps (`(i + 1) * f32(1 / n)`). For each point: `drand48` (f32) picks the first triangle whose
/// table value is not less than it (the last one if none; the original reads one entry past the table when none),
/// two more `drand48` (f32) values u, v are folded by `1 - u`, `1 - v` when their 80-bit sum exceeds 1, the third
/// weight is `(1 - v) - u` (rounded), and each coordinate is `((u * c0) + (v * c1)) + (w * c2)`.
pub fn si_particle_tri_sample(m: &mut Mem, r: &mut dyn Drand, tris: u32, uniform: i32, seed: u32, out: u32) {
    let nout = m.i32(out + 4);
    r.srand48(seed);
    let tab = m.local(12);
    m.set_i32(tab, 1);
    let n = m.i32(tris + 4);
    m.set_i32(tab + 4, n);
    si_particle_data_allocate(m, tab);
    let at = |m: &Mem, i: i32| m.u32(tab + 8).wrapping_add((i as u32).wrapping_mul(4));
    if uniform == 0 {
        let mut i = 0i32;
        while i < m.i32(tris + 4) {
            let p = m.u32(tris + 8).wrapping_add(36u32.wrapping_mul(i as u32));
            let e = m.local(36);
            for k in 0..3u32 {
                let a = (f(m, p + 12 + 4 * k) - f(m, p + 4 * k)).to_f32();
                m.set_f32(e + 4 * k, a);
                let b = (f(m, p + 24 + 4 * k) - f(m, p + 4 * k)).to_f32();
                m.set_f32(e + 12 + 4 * k, b);
            }
            si_particle_cross_product(m, e, e + 12, e + 24);
            let area = si_particle_norm(m, e + 24) * x(m.f64(0x0866_1328));
            m.release(36);
            let a = at(m, i);
            m.set_f32(a, area.to_f32());
            i += 1;
        }
        let rows = m.i32(tris + 4);
        let mut sum = x(0.0f32);
        let mut i = 0;
        while i < rows {
            sum = sum + f(m, at(m, i));
            i += 1;
        }
        let mut i = 0;
        while i < m.i32(tris + 4) {
            let a = at(m, i);
            let v = f(m, a) / sum;
            m.set_f32(a, v.to_f32());
            i += 1;
        }
        let mut acc = x(0.0f32);
        let mut i = 0;
        while i < m.i32(tris + 4) {
            let a = at(m, i);
            acc = acc + f(m, a);
            m.set_f32(a, acc.to_f32());
            i += 1;
        }
    } else {
        let rows = m.i32(tris + 4);
        let step = (x(1.0f32) / x(rows)).to_f32();
        let mut i = 0i32;
        while i < rows {
            let a = at(m, i);
            m.set_f32(a, (x(i + 1) * x(step)).to_f32());
            i = i.wrapping_add(1);
            if i >= m.i32(tris + 4) {
                break;
            }
        }
    }
    let mut base = 0u32;
    let mut s = 0i32;
    while s < nout {
        let rnd = x(r.drand48() as f32);
        s += 1;
        let rows = m.i32(tris + 4);
        let mut j = 0i32;
        if rnd.gt(f(m, at(m, 0))) && j < rows {
            loop {
                j += 1;
                // one entry past the table is read when j reaches `rows` (heap garbage; it cannot change j)
                if !rnd.gt(f(m, at(m, j))) {
                    break;
                }
                if j >= rows {
                    break;
                }
            }
        }
        if j < 0 {
            j = 0;
        }
        if j >= m.i32(tris + 4) {
            j = m.i32(tris + 4) - 1;
        }
        let tri = m.u32(tris + 8).wrapping_add(36u32.wrapping_mul(j as u32));
        let r1 = x(r.drand48() as f32);
        let r2 = x(r.drand48() as f32);
        let (mut u, mut v) = (r1, r2);
        if (r2 + r1).gt(x(1.0f32)) {
            u = x((x(1.0f32) - r1).to_f32());
            v = x((x(1.0f32) - r2).to_f32());
        }
        let w = x(((x(1.0f32) - v) - u).to_f32());
        for k in 0..3u32 {
            let val = (u * f(m, tri + 4 * k) + v * f(m, tri + 12 + 4 * k)) + w * f(m, tri + 24 + 4 * k);
            let dst = m.u32(out + 8).wrapping_add(4 * (base + k));
            m.set_f32(dst, val.to_f32());
        }
        base += 3;
    }
    si_particle_data_free(m, tab);
    m.release(12);
}

/// Builds a frame per sample of a path [soft 0x084c2f30 si_particlePath2Xfm]: `paths` has `dim` (a multiple of 3, at
/// least 6, else -1) floats a row, that is points; `out` becomes `dim / 3 * 16` floats a row, the same row count
/// (allocated, the old data not freed), and for each point a 4 x 4 matrix is written: rows k = (up[k], tangent[k],
/// side[k], position[k]) and a last row (0, 0, 0, 1). The tangent is the difference of the next and this point for the
/// first, the difference of this and the previous for the last, and for the others the sum of the two normalised
/// differences; each is normalised (a failure takes (1, 0, 0)). With `up` null the up vector starts (0, 0, 1) and has the
/// tangent's component removed and is normalised (failure (0, 1, 0)); with an `up` the tangent has the up's component
/// taken off (failure (1, 0, 0)). The side is the cross product of up and tangent. Returns 1.
pub fn si_particle_path2_xfm(m: &mut Mem, paths: u32, out: u32, up_arg: u32) -> i32 {
    let dim = m.i32(paths);
    let npts = dim / 3;
    if dim < 6 || dim != npts * 3 {
        return -1;
    }
    m.set_i32(out, npts << 4);
    let rows = m.i32(paths + 4);
    m.set_i32(out + 4, rows);
    si_particle_data_allocate(m, out);
    // scratch: up at 0, tangent at 12, side at 24, previous difference at 36, next difference at 48
    let s = m.local(60);
    let (up, tan, side, dprev, dnext) = (s, s + 12, s + 24, s + 36, s + 48);
    let mut path = 0i32;
    while path < m.i32(paths + 4) {
        let ibase = path.wrapping_mul(m.i32(paths));
        let obase = path.wrapping_mul(m.i32(out));
        let src = m.u32(paths + 8);
        // first point
        let a = src.wrapping_add((ibase as u32).wrapping_mul(4));
        for k in 0..3u32 {
            let d = (f(m, a + 12 + 4 * k) - f(m, a + 4 * k)).to_f32();
            m.set_f32(tan + 4 * k, d);
        }
        fill_up(m, up, up_arg);
        frame_at(m, out, obase, ibase, paths, up, tan, side, up_arg);
        let mut i = 1i32;
        while i < npts {
            let idx = ibase.wrapping_add(3 * i);
            let oidx = obase.wrapping_add(16 * i);
            let src = m.u32(paths + 8);
            let a = src.wrapping_add((idx as u32).wrapping_mul(4));
            if i == npts - 1 {
                for k in 0..3u32 {
                    let d = (f(m, a + 4 * k) - f(m, a - 12 + 4 * k)).to_f32();
                    m.set_f32(tan + 4 * k, d);
                }
                i = npts;
            } else {
                i += 1;
                for k in 0..3u32 {
                    let d = (f(m, a + 4 * k) - f(m, a - 12 + 4 * k)).to_f32();
                    m.set_f32(dprev + 4 * k, d);
                    let d = (f(m, a + 12 + 4 * k) - f(m, a + 4 * k)).to_f32();
                    m.set_f32(dnext + 4 * k, d);
                }
                // the difference to the previous point is normalised first, then the one to the next
                si_particle_normalize(m, dprev);
                si_particle_normalize(m, dnext);
                for k in 0..3u32 {
                    let t = (f(m, dprev + 4 * k) + f(m, dnext + 4 * k)).to_f32();
                    m.set_f32(tan + 4 * k, t);
                }
            }
            // no fresh up vector here: the original keeps the one the previous frame left
            frame_at(m, out, oidx, idx, paths, up, tan, side, up_arg);
        }
        path += 1;
    }
    m.release(60);
    1
}

/// The up vector at the start of a frame: (0, 0, 1) without an `up`, else a copy of it.
fn fill_up(m: &mut Mem, up: u32, up_arg: u32) {
    if up_arg == 0 {
        m.set_f32(up, 0.0);
        m.set_f32(up + 4, 0.0);
        m.set_f32(up + 8, 1.0);
    } else {
        for k in 0..3 {
            let v = m.u32(up_arg + 4 * k);
            m.set_u32(up + 4 * k, v);
        }
    }
}

/// Orthogonalises and stores one frame of [`si_particle_path2_xfm`] (`tan` holds the raw tangent).
#[allow(clippy::too_many_arguments)]
fn frame_at(m: &mut Mem, out: u32, oidx: i32, pidx: i32, paths: u32, up: u32, tan: u32, side: u32, up_arg: u32) {
    if si_particle_normalize(m, tan) < 0 {
        m.set_f32(tan, 1.0);
        m.set_f32(tan + 8, 0.0);
        m.set_f32(tan + 4, 0.0);
    }
    let dot = si_particle_dot_product(m, up, tan);
    if up_arg == 0 {
        for k in 0..3u32 {
            let v = f(m, up + 4 * k) - dot * f(m, tan + 4 * k);
            m.set_f32(up + 4 * k, v.to_f32());
        }
        if si_particle_normalize(m, up) < 0 {
            m.set_f32(up + 8, 0.0);
            m.set_f32(up, 0.0);
            m.set_f32(up + 4, 1.0);
        }
    } else {
        for k in 0..3u32 {
            let v = f(m, tan + 4 * k) - dot * f(m, up + 4 * k);
            m.set_f32(tan + 4 * k, v.to_f32());
        }
        if si_particle_normalize(m, tan) < 0 {
            m.set_f32(tan + 8, 0.0);
            m.set_f32(tan + 4, 0.0);
            m.set_f32(tan, 1.0);
        }
    }
    si_particle_cross_product(m, up, tan, side);
    let dst = m.u32(out + 8);
    let src = m.u32(paths + 8);
    for k in 0..3u32 {
        let row = dst.wrapping_add((oidx as u32).wrapping_mul(4)).wrapping_add(16 * k);
        let (a, b, c) = (m.u32(up + 4 * k), m.u32(tan + 4 * k), m.u32(side + 4 * k));
        m.set_u32(row, a);
        m.set_u32(row + 4, b);
        m.set_u32(row + 8, c);
        let p = m.u32(src.wrapping_add((pidx as u32).wrapping_add(k).wrapping_mul(4)));
        m.set_u32(row + 12, p);
    }
    let last = dst.wrapping_add((oidx as u32).wrapping_mul(4)).wrapping_add(48);
    m.set_u32(last, 0);
    m.set_u32(last + 4, 0);
    m.set_u32(last + 8, 0);
    m.set_f32(last + 12, 1.0);
}

/// Computes ballistic paths between two sets of points [soft 0x084c3474 si_particleComputePaths]: the status record
/// (+0x8 f32 acceleration g, +0xc the up vector (3 f32), +0x30 sample count, +0x38 start frame, +0x3c stop frame)
/// gives the shape; its up vector is normalised first (failure: message "Invalid up vector!" shown and -1). `from` and
/// `to` must have the same positive row count ("Internal error ..." otherwise), the sample count at least 2, and
/// `(stop - start) / SI_frameRate` (80 bits, compared unrounded with 0.0001, rounded to f32 as T afterwards) must not be
/// less than 0.0001; each failure shows its message at level 3 and returns -1. `out` gets `samples * 3` floats a row
/// (not checked for allocation) with rows = the row count. For each row the offset d = to - from (rounded), its part w
/// perpendicular to the up vector u (d - (u.d) u, rounded; normalised when its length is above 0.0001) and the
/// speeds are made, and sample j at time t = j * T / (n - 1) is `t * (t * 0.5 * (-g u) + v) + from`, with
/// `v = C u + A w`, `A = |w| / T` and `C = f32((u.d + g * 0.5 * T * T) / T)`. Returns 1.
pub fn si_particle_compute_paths(m: &mut Mem, o: &mut dyn Outside, status: u32, from: u32, to: u32, out: u32) -> i32 {
    if si_particle_normalize(m, status + 0xc) < 0 {
        o.si_status_display_message(m, 0x086d_7e00, 3);
        return -1;
    }
    let rows = m.i32(from + 4);
    if rows != m.i32(to + 4) || rows <= 0 {
        o.si_status_display_message(m, 0x086d_7e20, 3);
        return -1;
    }
    let n = m.i32(status + 0x30);
    if n <= 1 {
        o.si_status_display_message(m, 0x086d_7e60, 3);
        return -1;
    }
    let diff = m.i32(status + 0x3c).wrapping_sub(m.i32(status + 0x38));
    let q = x(diff) / x(m.f32(SI_FRAME_RATE));
    let tt = q.to_f32();
    if q.lt(x(m.f64(0x0866_1318))) {
        o.si_status_display_message(m, 0x086d_7ea0, 3);
        return -1;
    }
    m.set_i32(out, n.wrapping_mul(3));
    m.set_i32(out + 4, rows);
    si_particle_data_allocate(m, out);
    // scratch: u at 0, -g*u at 12, w at 24, d at 36, v at 48
    let s = m.local(60);
    let (u, ng, w, d, v) = (s, s + 12, s + 24, s + 36, s + 48);
    for k in 0..3u32 {
        let val = m.u32(status + 0xc + 4 * k);
        m.set_u32(u + 4 * k, val);
    }
    let g = m.f32(status + 8);
    for k in 0..3u32 {
        let val = (-x(g)) * f(m, u + 4 * k);
        m.set_f32(ng + 4 * k, val.to_f32());
    }
    let thresh = x(m.f64(0x0866_1318));
    let half = x(m.f64(0x0866_1320));
    let mut outp = m.u32(out + 8);
    let mut p1 = m.u32(from + 8);
    let mut p2 = m.u32(to + 8);
    let step = x(tt) / x(n - 1);
    let mut row = 0;
    while row < rows {
        row += 1;
        for k in 0..3u32 {
            let val = f(m, p2 + 4 * k) - f(m, p1 + 4 * k);
            m.set_f32(d + 4 * k, val.to_f32());
        }
        let dot = si_particle_dot_product(m, u, d);
        for k in 0..3u32 {
            let val = f(m, d + 4 * k) - dot * f(m, u + 4 * k);
            m.set_f32(w + 4 * k, val.to_f32());
        }
        let len = si_particle_norm(m, w);
        if len.gt(thresh) {
            for k in 0..3u32 {
                let val = f(m, w + 4 * k) / len;
                m.set_f32(w + 4 * k, val.to_f32());
            }
        }
        let a = len / x(tt);
        let btt = (x(g) * half * x(tt)) * x(tt);
        let c = x(((dot + btt) / x(tt)).to_f32());
        for k in 0..3u32 {
            let val = c * f(m, u + 4 * k) + a * f(m, w + 4 * k);
            m.set_f32(v + 4 * k, val.to_f32());
        }
        let mut t = x(0.0f32);
        let mut idx = 0u32;
        let mut j = 0;
        while j < n {
            let th = t * half;
            for k in 0..3u32 {
                let val = f(m, p1 + 4 * k) + t * (f(m, v + 4 * k) + th * f(m, ng + 4 * k));
                m.set_f32(outp + 4 * (idx + k), val.to_f32());
            }
            t = t + step;
            idx += 3;
            j += 1;
        }
        outp = outp.wrapping_add((m.u32(out) << 2) as u32);
        p1 += 12;
        p2 += 12;
    }
    m.release(60);
    1
}
