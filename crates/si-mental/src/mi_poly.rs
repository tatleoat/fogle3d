// SPDX-License-Identifier: MIT
//! `mi_poly.c`: the translator's polygon geometry for mental ray, the vector writer and the
//! motion vectors (package 63, finding 0036).
//!
//! Like [`crate::mi_patch`], a routine here writes through `SI_mrpipeFPRINTF` and
//! `SI_mrpipeFPUTC`, which are not ported: they are called through [`Outside`].

use crate::mi_patch::{
    cvec, dw, fprintf, g, ELEMDICT_FIRST, ELEMDICT_NEXT, MB_NB_MAX_VERT, MEMTRACK_ALLOCATE, MEMTRACK_FREE, MEMTRACK_REALLOC, SFATAL, SI_GBL_VERT, SI_MEM_PARAMS,
    VECTOR_X_MATRIX,
};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `getenv` in the program's import table.
pub const GETENV: Routine = Routine::new("getenv", 0x080c_3bb8);
/// `SI_mrpipeFPUTC (0x0842c610)`: one byte on the pipe to mental ray.
pub const MRPIPE_FPUTC: Routine = Routine::new("SI_mrpipeFPUTC", 0x0842_c610);

/// `did_getenv` and `ascii_vecs`, the function's two statics (16 bits each).
const DID_GETENV: u32 = 0x086f_2c80;
const ASCII_VECS: u32 = 0x086f_2c82;
/// The scratch byte of `SI_swap4byte`.
const SWAP_TMP: u32 = 0x0881_7f38;
/// `tcoords` (a pointer) and `tcoords_size` (32 bits), the texture coordinate array.
pub const TCOORDS: u32 = 0x086f_2960;
pub const TCOORDS_SIZE: u32 = 0x086f_2964;

/// A float through the 80-bit register and back (a signalling NaN comes out quiet).
fn quiet(bits: u32) -> u32 {
    F80::from_f32(f32::from_bits(bits)).to_f32().to_bits()
}

/// A vector written as binary or, with `SI_ASCII_VECTORS` in the environment, as text
/// [soft 0x08590928 print_compressed_vector].
///
/// The first call looks the variable up. Text: three `%f`; binary: a backquote, the three
/// floats byte-swapped (so most significant byte first), a backquote and a newline.
pub fn print_compressed_vector(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, c: u32) {
    if m.u16(DID_GETENV) == 0 {
        let p = o.call(m, GETENV, &[0x086f_2c84]).eax;
        m.set_u16(ASCII_VECS, (p != 0) as u16);
        m.set_u16(DID_GETENV, 1);
    }
    if m.u16(ASCII_VECS) != 0 {
        let d = |bits: u32| dw(F80::from_f32(f32::from_bits(bits)).to_f64());
        let (x, y, z) = (d(a), d(b), d(c));
        fprintf(m, o, &[0x086f_2c95, x[0], x[1], y[0], y[1], z[0], z[1]]);
    } else {
        let words = [quiet(a).swap_bytes(), quiet(b).swap_bytes(), quiet(c).swap_bytes()];
        // SI_swap4byte (0x083f9350) leaves its scratch byte, a static, at byte 1 of the last float
        m.set_u8(SWAP_TMP, (quiet(c) >> 8) as u8);
        fprintf(m, o, &[0x086f_2ca2]);
        for w in words {
            for i in 0..4 {
                o.call(m, MRPIPE_FPUTC, &[(w >> (8 * i)) & 0xff]);
            }
        }
        fprintf(m, o, &[0x086f_2ca4]);
    }
}

/// How far a vertex moved since the last frame, as the vector that undoes it
/// [soft 0x085907ac calculate_motion_vector].
///
/// `flags` (the `u16` at +0x3ee of the model): bit 0 is an instance, which compares `point`
/// against the centre kept in the motion record (+0x33c; +0x158); bit 1 compares vertex `index`
/// of the global array against the one kept in the record (+0x22c; 0x4c bytes each, at +8).
/// The vector is the negated difference; with neither bit nothing is written.
pub fn calculate_motion_vector(m: &mut Mem, model: u32, point: u32, index: u32, out: u32) {
    let flags = m.u16(model + 0x3ee);
    let f = |m: &Mem, p: u32| F80::from_f32(f32::from_bits(m.u32(p)));
    if flags & 1 == 0 {
        if flags & 2 == 0 {
            return;
        }
        let rec = m.u32(model + 0x33c);
        let old = m.u32(rec + 0x22c);
        let gv = m.u32(SI_GBL_VERT);
        let off = index.wrapping_mul(0x4c);
        for k in 0..3u32 {
            let d = f(m, gv.wrapping_add(off).wrapping_add(8 + 4 * k)) - f(m, old.wrapping_add(off).wrapping_add(8 + 4 * k));
            m.set_u32(out + 4 * k, (-d).to_f32().to_bits());
        }
    } else {
        let rec = m.u32(model + 0x33c);
        for k in 0..3u32 {
            let d = f(m, point + 4 * k) - f(m, rec + 0x158 + 4 * k);
            m.set_u32(out + 4 * k, (-d).to_f32().to_bits());
        }
    }
}

/// A moving model's motion vector for one vertex, written [soft 0x08590848 motion_vector].
///
/// Answers 1 when the model has motion flags and a motion record, else 0 and nothing is written.
pub fn motion_vector(m: &mut Mem, o: &mut dyn Outside, model: u32, point: u32, index: u32) -> u32 {
    if m.u16(model + 0x3ee) == 0 || m.u32(model + 0x33c) == 0 {
        return 0;
    }
    let l = m.local(12);
    calculate_motion_vector(m, model, point, index, l);
    let (a, b, c) = (m.u32(l), m.u32(l + 4), m.u32(l + 8));
    m.release(12);
    cvec(m, o, quiet(a), quiet(b), quiet(c));
    1
}

/// As [`motion_vector`], the vector taken through a matrix first (`inverse`)
/// [soft 0x085908a8 mi2_motion_vector].
pub fn mi2_motion_vector(m: &mut Mem, o: &mut dyn Outside, model: u32, point: u32, inverse: u32, index: u32) -> u32 {
    if m.u16(model + 0x3ee) == 0 || m.u32(model + 0x33c) == 0 {
        return 0;
    }
    let l = m.local(24);
    calculate_motion_vector(m, model, point, index, l + 12);
    o.call(m, VECTOR_X_MATRIX, &[l, l + 12, inverse]);
    let (a, b, c) = (m.u32(l), m.u32(l + 4), m.u32(l + 8));
    m.release(24);
    cvec(m, o, quiet(a), quiet(b), quiet(c));
    1
}

/// A 3D texture's point written [soft 0x0859076c texture3d_point]: answers whether `texture` is set.
pub fn texture3d_point(m: &mut Mem, o: &mut dyn Outside, texture: u32, point: u32) -> bool {
    if texture != 0 {
        let (a, b, c) = (m.u32(point), m.u32(point + 4), m.u32(point + 8));
        cvec(m, o, quiet(a), quiet(b), quiet(c));
    }
    texture != 0
}

/// The size of the texture coordinate array a polygon needs, and the array itself
/// [soft 0x08590314 prepare_tcoords_array].
///
/// Counts the 2D textures of kinds 4 to 6 related to `tex` (relation 0x191) and to `elem` (400);
/// `n` coordinates each must fit in `tcoords`, which is reallocated to a multiple of 64 entries
/// of eight bytes when it is too small. Answers the count (0 when there is none).
pub fn prepare_tcoords_array(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, tex: u32, n: i32) -> i32 {
    if dict == 0 || elem == 0 || m.u32(elem + 4) == 0 {
        return 0;
    }
    let idx = m.local(4);
    let mut count: i32 = 0;
    for (owner, relation) in [(tex, 0x191u32), (elem, 400)] {
        if owner == 0 {
            continue;
        }
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        while t != 0 {
            if (m.u16(t + 0x2a).wrapping_sub(4)) < 3 {
                count += 1;
            }
            let cid2 = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        }
    }
    m.release(4);
    if count == 0 {
        return 0;
    }
    let need = count.wrapping_mul(n);
    if need <= m.i32(TCOORDS_SIZE) {
        return count;
    }
    let old = m.u32(TCOORDS);
    if old != 0 {
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, old]);
    }
    let size = (need.wrapping_add(0x40) as u32) & 0xffff_ffc0;
    m.set_u32(TCOORDS_SIZE, size);
    let p = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, size << 3]).eax;
    m.set_u32(TCOORDS, p);
    if p == 0 {
        o.call(m, SFATAL, &[0x086f_2c62]);
    }
    count
}

/// `SI_txtProjectionInfo (0x0829b584)`: `(model, lclmap, &bound, &width, &center)`, ported in `si-render`.
pub const TXT_PROJECTION_INFO: Routine = Routine::new("SI_txtProjectionInfo", 0x0829_b584);
/// `SI_mapTcoordCompute (0x082b3090)`: `(lclmap, x, y, z, &out[2], &bound, &width, &center)`, ported in `si-render`.
pub const MAP_TCOORD_COMPUTE: Routine = Routine::new("SI_mapTcoordCompute", 0x082b_3090);
/// `SI_nodeGetUV (0x084534d8)`: `(node, domain, &u, &v)`, ported in `si-render`.
pub const NODE_GET_UV: Routine = Routine::new("SI_nodeGetUV", 0x0845_34d8);
/// `SI_rvtxGetUV (0x0826d510)`: `(vertex, domain, &u, &v)`, ported in `si-render`.
pub const RVTX_GET_UV: Routine = Routine::new("SI_rvtxGetUV", 0x0826_d510);
/// `SI_meshGetTxtUVDomain (0x08453668)`: `(model, id, &domain)`, ported in `si-render`.
pub const MESH_GET_TXT_UV_DOMAIN: Routine = Routine::new("SI_meshGetTxtUVDomain", 0x0845_3668);

/// The texture projection's globals: the bound (`SI_Bound`), the width and the centre (three floats each).
const BOUND: u32 = 0x0882_9f70;
const WIDTH: u32 = 0x0882_9f58;
const CENTER: u32 = 0x0882_9f64;

/// What the texture builders do with a local map after `SI_txtProjectionInfo`: the map's
/// 3x3 matrix (+0xb8..) times its offset (+0xa0..) is added to the centre.
fn project_centre(m: &mut Mem, o: &mut dyn Outside, model: u32, lm: u32) {
    o.call(m, TXT_PROJECTION_INFO, &[model, lm, BOUND, WIDTH, CENTER]);
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(lm + off)));
    let (a0, a4, a8) = (f(m, 0xa0), f(m, 0xa4), f(m, 0xa8));
    let t1 = (f(m, 0xbc) * a4 + f(m, 0xb8) * a0) + f(m, 0xc0) * a8;
    let t2 = (f(m, 0xc8) * a0 + f(m, 0xcc) * a4) + f(m, 0xd0) * a8;
    let t3 = a8 * f(m, 0xe0) + (a4 * f(m, 0xdc) + a0 * f(m, 0xd8));
    for (k, t) in [t1, t2, t3].into_iter().enumerate() {
        let c = F80::from_f32(f32::from_bits(m.u32(CENTER + 4 * k as u32)));
        m.set_u32(CENTER + 4 * k as u32, (t + c).to_f32().to_bits());
    }
}

/// The texture coordinates of one vertex or node of a 2D texture of kind 4 (UV from the mesh) or
/// another kind (projected from the position at `pos`); `out` takes u and v.
fn quarter_half(m: &Mem, p: u32, bits: (bool, bool)) -> (bool, bool) {
    // x <= 0.25 and x >= 0.75, ordered
    let x = F80::from_f32(f32::from_bits(m.u32(p)));
    let le = x.lt(F80::from_f64(0.25)) || x.eq(F80::from_f64(0.25));
    let ge = x.gt(F80::from_f64(0.75)) || x.eq(F80::from_f64(0.75));
    (bits.0 || le, bits.1 || ge)
}

/// The two-phase walk over the 2D textures of a polygon's material and then its model:
/// `(owner, relation)` for the next call.
struct Walk {
    first: bool,
    p: u32,
    idx: u32,
}

impl Walk {
    fn start(m: &mut Mem, o: &mut dyn Outside, dict: u32, tex: u32, idx: u32) -> Walk {
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let p = o.call(m, ELEMDICT_FIRST, &[dict, tex, 1, 3, 0x191, cid2, 0, idx]).eax;
        Walk { first: true, p, idx }
    }

    /// The next 2D texture, or `None` after both lists; `elem` is the model's element.
    fn current(&mut self, m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> Option<u32> {
        if !(self.first || self.p != 0) {
            return None;
        }
        if self.p == 0 {
            self.first = false;
            let cid2 = m.u32(g::CID_TEXTURES2D);
            self.p = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 400, cid2, 0, self.idx]).eax;
            if self.p == 0 {
                return None;
            }
        }
        Some(self.p)
    }

    fn advance(&mut self, m: &mut Mem, o: &mut dyn Outside, dict: u32, tex: u32, elem: u32) {
        let (owner, relation) = if self.first { (tex, 0x191) } else { (elem, 400) };
        let cid2 = m.u32(g::CID_TEXTURES2D);
        self.p = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, self.idx]).eax;
    }
}

/// The texture coordinates of a polygon's corners, for every 2D texture of kinds 4 to 6
/// [soft 0x0858fb50 calc_poly_textverts].
///
/// `tex` is the material's element, `poly` the polygon (nodes at +0xc, 0x24 bytes each, count a
/// `u16` at +0x10). Texture k of the lists (material's first, then the model's) fills
/// `tcoords[k*8 + n*count_textures*8]` with u and v per corner (UV of the mesh for kind 4,
/// else computed from the corner's position). When some u are at most 0.25 and some at least 0.75
/// (the texture wraps), every u at most 0.5 gets 1 added.
pub fn calc_poly_textverts(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, tex: u32, poly: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return;
    }
    let count = prepare_tcoords_array(m, o, dict, elem, tex, m.u16(poly + 0x10) as i32);
    if count == 0 {
        return;
    }
    // the original's frame: 0x6c bytes of locals and three saved registers below the saved frame pointer
    let l = m.local(0x7c);
    let ebp = l + 0x78;
    let (idx, dom, uv) = (ebp - 0x2c, ebp - 0x30, ebp - 0x38);
    let mut w = Walk::start(m, o, dict, tex, idx);
    let mut k: i32 = 0;
    while let Some(lm) = w.current(m, o, dict, elem) {
        if m.u16(lm + 0x2a).wrapping_sub(4) < 3 {
            project_centre(m, o, model, lm);
            let (mut small, mut big) = (false, false);
            let base = m.u32(TCOORDS).wrapping_add((k as u32).wrapping_mul(8));
            let mut dst = base;
            let mut node = m.u32(poly + 0xc);
            k += 1;
            let n = m.u16(poly + 0x10);
            for _ in 0..n {
                if m.i32(node) >= 0 {
                    let mut mesh = m.u32(model + 0x3c);
                    if mesh == 0 {
                        mesh = m.u32(m.u32(model + 0x38));
                    }
                    let v = m.u32(mesh).wrapping_add((m.i32(node) as u32).wrapping_mul(0x14));
                    if m.u16(lm + 0x2a) == 4 {
                        o.call(m, MESH_GET_TXT_UV_DOMAIN, &[model, m.u32(lm + 0x190), dom]);
                        o.call(m, NODE_GET_UV, &[node, m.u32(dom), uv, uv + 4]);
                        m.set_u32(dst, m.u32(uv));
                        let r = F80::from_i32(1) - F80::from_f32(f32::from_bits(m.u32(uv + 4)));
                        m.set_u32(dst + 4, r.to_f32().to_bits());
                    } else {
                        let (x, y, z) = (m.u32(v), m.u32(v + 4), m.u32(v + 8));
                        o.call(m, MAP_TCOORD_COMPUTE, &[lm, x, y, z, dst, BOUND, WIDTH, CENTER]);
                        (small, big) = quarter_half(m, dst, (small, big));
                    }
                    dst = dst.wrapping_add((count as u32).wrapping_mul(8));
                }
                node = node.wrapping_add(0x24);
            }
            if small && big {
                let mut p = base;
                for _ in 0..n {
                    let x = F80::from_f32(f32::from_bits(m.u32(p)));
                    if x.lt(F80::from_f64(0.5)) || x.eq(F80::from_f64(0.5)) {
                        m.set_u32(p, (x + F80::from_i32(1)).to_f32().to_bits());
                    }
                    p = p.wrapping_add((count as u32).wrapping_mul(8));
                }
            }
        }
        w.advance(m, o, dict, tex, elem);
    }
    m.release(0x7c);
}

/// The same for a triangle of render vertices (`tn`: three pointers) [soft 0x0858fecc calc_tri_textverts].
///
/// A mirrored model (the product of its scale against the handedness, flipped by bit 3 of the
/// model's flags) takes the corners in the other order. Kind 4 reads the vertex's UV (from the
/// mesh's domain when the model's type, +0x30, is 4, else domain 0) and writes 1-u, 1-v.
pub fn calc_tri_textverts(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, tex: u32, tri: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return;
    }
    let tf = m.u32(model + 0x44);
    let base = if m.i32(crate::mi_patch::GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(crate::mi_patch::RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(model + 8) & 8 != 0 {
        flip = !flip;
    }
    let count = prepare_tcoords_array(m, o, dict, elem, tex, 3);
    if count == 0 {
        return;
    }
    // the original's frame: 0x7c bytes of locals and three saved registers below the saved frame pointer
    let l = m.local(0x8c);
    let ebp = l + 0x88;
    let (idx, dom) = (ebp - 0x38, ebp - 0x3c);
    let mut w = Walk::start(m, o, dict, tex, idx);
    let mut k: i32 = 0;
    while let Some(lm) = w.current(m, o, dict, elem) {
        if m.u16(lm + 0x2a).wrapping_sub(4) < 3 {
            project_centre(m, o, model, lm);
            let (mut small, mut big) = (false, false);
            let base = m.u32(TCOORDS).wrapping_add((k as u32).wrapping_mul(8));
            let mut dst = base;
            k += 1;
            for i in 0..3 {
                let corner = match i {
                    0 => {
                        if flip {
                            m.u32(tri + 8)
                        } else {
                            m.u32(tri)
                        }
                    }
                    1 => m.u32(tri + 4),
                    _ => {
                        if flip {
                            m.u32(tri)
                        } else {
                            m.u32(tri + 8)
                        }
                    }
                };
                if m.u16(lm + 0x2a) == 4 {
                    if m.i32(model + 0x30) == 4 {
                        o.call(m, MESH_GET_TXT_UV_DOMAIN, &[model, m.u32(lm + 0x190), dom]);
                    } else {
                        m.set_u32(dom, 0);
                    }
                    o.call(m, RVTX_GET_UV, &[corner, m.u32(dom), dst, dst + 4]);
                    for q in 0..2 {
                        let r = F80::from_i32(1) - F80::from_f32(f32::from_bits(m.u32(dst + 4 * q)));
                        m.set_u32(dst + 4 * q, r.to_f32().to_bits());
                    }
                } else {
                    let (x, y, z) = (m.u32(corner + 0x20), m.u32(corner + 0x24), m.u32(corner + 0x28));
                    o.call(m, MAP_TCOORD_COMPUTE, &[lm, x, y, z, dst, BOUND, WIDTH, CENTER]);
                }
                (small, big) = quarter_half(m, dst, (small, big));
                dst = dst.wrapping_add((count as u32).wrapping_mul(8));
            }
            if small && big {
                let mut p = base;
                for _ in 0..3 {
                    let x = F80::from_f32(f32::from_bits(m.u32(p)));
                    if x.lt(F80::from_f64(0.5)) || x.eq(F80::from_f64(0.5)) {
                        m.set_u32(p, (x + F80::from_i32(1)).to_f32().to_bits());
                    }
                    p = p.wrapping_add((count as u32).wrapping_mul(8));
                }
            }
        }
        w.advance(m, o, dict, tex, elem);
    }
    m.release(0x8c);
}

/// The texture coordinates of one point for every 2D texture of the material (or, when `tex` is
/// null, the model) [soft 0x08590468 texture2d_point].
///
/// Textures of kinds 7 and 8 are skipped; each other one is counted. Kinds 4 to 6 write the next
/// entry of `tcoords` (`*index` is advanced), kind 4 otherwise takes UV from the vertex `rvtx` or
/// the node, the rest are projected from `pos`. Answers the count.
pub fn texture2d_point(m: &mut Mem, o: &mut dyn Outside, dict: u32, tex: u32, pos: u32, rvtx: u32, node: u32, index: u32, elem: u32) -> i32 {
    let mut count = 0i32;
    if dict == 0 || elem == 0 || m.u32(elem + 4) == 0 {
        return 0;
    }
    let model = m.u32(elem + 4);
    let (owner, relation) = if tex != 0 { (tex, 0x191u32) } else { (elem, 400) };
    // the original's frame: 0x5c bytes of locals and three saved registers below the saved frame pointer
    let l = m.local(0x6c);
    let ebp = l + 0x68;
    let (idx, uv) = (ebp - 0x2c, ebp - 0x34);
    let cid2 = m.u32(g::CID_TEXTURES2D);
    let mut lm = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
    while lm != 0 {
        if m.u16(lm + 0x2a).wrapping_sub(7) > 1 {
            count += 1;
            project_centre(m, o, model, lm);
            let kind = m.u16(lm + 0x2a);
            if kind.wrapping_sub(4) < 3 {
                let i = m.u32(index);
                let e = m.u32(TCOORDS).wrapping_add(i.wrapping_mul(8));
                cvec(m, o, quiet(m.u32(e)), quiet(m.u32(e + 4)), 0);
                m.set_u32(index, i.wrapping_add(1));
            } else {
                if kind == 4 {
                    if rvtx != 0 {
                        let dom = ebp - 0x38;
                        o.call(m, MESH_GET_TXT_UV_DOMAIN, &[model, m.u32(lm + 0x190), dom]);
                        o.call(m, RVTX_GET_UV, &[rvtx, m.u32(dom), uv, uv + 4]);
                        let r = F80::from_i32(1) - F80::from_f32(f32::from_bits(m.u32(uv)));
                        m.set_u32(uv, r.to_f32().to_bits());
                        let r = F80::from_i32(1) - F80::from_f32(f32::from_bits(m.u32(uv + 4)));
                        m.set_u32(uv + 4, r.to_f32().to_bits());
                    } else if node != 0 {
                        let (dom, nu, nv) = (ebp - 0x3c, ebp - 0x44, ebp - 0x40);
                        o.call(m, MESH_GET_TXT_UV_DOMAIN, &[model, m.u32(lm + 0x190), dom]);
                        o.call(m, NODE_GET_UV, &[node, m.u32(dom), nu, nv]);
                        m.set_u32(uv, m.u32(nu));
                        let r = F80::from_i32(1) - F80::from_f32(f32::from_bits(m.u32(nv)));
                        m.set_u32(uv + 4, r.to_f32().to_bits());
                    }
                } else {
                    let (x, y, z) = (m.u32(pos), m.u32(pos + 4), m.u32(pos + 8));
                    o.call(m, MAP_TCOORD_COMPUTE, &[lm, x, y, z, uv, BOUND, WIDTH, CENTER]);
                }
                cvec(m, o, quiet(m.u32(uv)), quiet(m.u32(uv + 4)), 0);
            }
        }
        let cid2 = m.u32(g::CID_TEXTURES2D);
        lm = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
    }
    m.release(0x6c);
    count
}

// ---------------------------------------------------------------------------------------------
// print_mesh_geometry

/// Mesh and matrix routines of other files, called through [`Outside`].
pub const MESH_COPY: Routine = Routine::new("SI_meshCopy", 0x0844_04e0);
pub const MESH_INVERSE: Routine = Routine::new("SI_meshInverse", 0x0844_462c);
pub const MESH_DISPOSE: Routine = Routine::new("SI_meshDispose", 0x0844_04a0);
pub const FINDBOUND: Routine = Routine::new("findbound", 0x0859_4978);
pub const RELATIONS_COUNT: Routine = Routine::new("SI_elemdictRelationsCount", 0x0813_5700);
pub const ELEMDICT_NTH: Routine = Routine::new("SI_elemdictGetNthRelatedElem", 0x0813_8190);
pub const NORMALIZE_VECTOR: Routine = Routine::new("normalize_vector", 0x0859_835c);
pub const MATRIX_INVERSE: Routine = Routine::new("SI_matrixInverse", 0x0851_672c);
pub const MATRIX_TRANSPOSE: Routine = Routine::new("SI_matrixTranspose", 0x0851_66ec);
pub const MATRIX_COPY_R: Routine = Routine::new("SI_matrixCopy", 0x0851_6224);
pub const INVERT_MATRIX_R: Routine = Routine::new("invert_matrix", 0x0859_883c);
pub const SDEBUG_R: Routine = Routine::new("sdebug", 0x0859_8cdc);
pub const SWARNING_R: Routine = Routine::new("swarning", 0x0859_8c44);
/// `mi2output` (16 bits; `SI_getmi2Output (0x08580cfc)` returns it sign-extended), `SI_identite`, the width
/// (three floats) of the bound, and the width's neighbour `center` already named above.
pub const MI2OUTPUT: u32 = 0x086e_f9c0;
/// Bytes the original has pushed below its locals at the call of `calc_poly_textverts` in `print_mesh_geometry`.
const PAD_CALC: u32 = 0x24;
const PAD_T2: u32 = 0x24;
pub const SI_IDENTITE: u32 = 0x086d_c160;

fn f32f(m: &Mem, a: u32) -> F80 {
    F80::from_f32(f32::from_bits(m.u32(a)))
}

/// The x87 status bits C3, C2, C0 (mask 0x45) of `a` compared with `b`.
fn cmp45(a: F80, b: F80) -> u8 {
    use std::cmp::Ordering::*;
    match a.compare(b) {
        Some(Greater) => 0,
        Some(Less) => 1,
        Some(Equal) => 0x40,
        None => 0x45,
    }
}

/// The two floats' 8-bit colour values of the mental ray vertex in the second mode of
/// `print_mesh_geometry`; see the callers.
const K255: u32 = 0x437f_0000;

/// Clamp r, g, b (bits of f32) into 0..255 the way the original does: above 255 gives 255.0, below 0 gives
/// the integer 0, NaN is left alone.
fn clamp255(v: &mut [u32; 3]) {
    let top = F80::from_f64(255.0);
    for x in v.iter_mut() {
        let f = F80::from_f32(f32::from_bits(*x));
        if cmp45(top, f) == 1 {
            *x = K255;
        }
        let f = F80::from_f32(f32::from_bits(*x));
        if cmp45(f, F80::from_i32(0)) == 1 {
            *x = 0;
        }
    }
}

/// The 3-D mesh of a model written as a mental ray polygon group [soft 0x0858cc30 print_mesh_geometry].
///
/// `mat` is the model's matrix, `shadow` selects the shadow or the visible polygons, `no_out` only
/// collects (writes nothing), `motion` keeps the vertex positions for the next frame's motion vectors.
/// Reproduced as the original runs: the mirrored copies of the meshes (restored at the end), the vertex
/// list (`v` lines through `print_compressed_vector`), the polygon records (`# n`, `v`, `n`, `t`, `m` lines),
/// the material names, the holes and, in mental ray 2 output, the approximation line.
pub fn print_mesh_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, shadow: i32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return;
    }
    // the original's frame: 0x1ec bytes of locals and three saved registers below the saved frame pointer
    let l = m.local(0x1fc);
    let ebp = l + 0x1f8;
    let (no_out, motion) = (no_out as u16, motion as u16);
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    let tf = m.u32(model + 0x44);
    let base = if m.i32(crate::mi_patch::GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(crate::mi_patch::RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(model + 8) & 8 != 0 {
        flip = !flip;
    }
    let (mut save1, mut save2, mut was3c) = (0u32, 0u32, 0u16);
    if flip {
        let c = m.u32(model + 0x3c);
        if c != 0 {
            save1 = c;
            let n = o.call(m, MESH_COPY, &[c]).eax;
            m.set_u32(model + 0x3c, n);
            o.call(m, MESH_INVERSE, &[n]);
            was3c = 1;
        } else {
            let slot = m.u32(model + 0x38);
            save1 = m.u32(slot);
            let n = o.call(m, MESH_COPY, &[save1]).eax;
            let slot = m.u32(model + 0x38);
            m.set_u32(slot, n);
            let slot = m.u32(model + 0x38);
            let arg = m.u32(slot);
            o.call(m, MESH_INVERSE, &[arg]);
            was3c = 0;
        }
        save2 = m.u32(model + 0x48);
        let n = o.call(m, MESH_COPY, &[save2]).eax;
        m.set_u32(model + 0x48, n);
        o.call(m, MESH_INVERSE, &[n]);
    }
    let mesh = m.u32(model + 0x48);
    o.call(m, FINDBOUND, &[model]);
    for (k, (hi, lo)) in [(0x1cu32, 0x10u32), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let d = f32f(m, model + hi) - f32f(m, model + lo);
        m.set_u32(0x0882_9f58 + 4 * k as u32, d.to_f32().to_bits());
    }
    let half = F80::from_f32(0.5);
    for (k, (hi, lo)) in [(0x1cu32, 0x10u32), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let s = f32f(m, model + hi) + f32f(m, model + lo);
        m.set_u32(CENTER + 4 * k as u32, (s * half).to_f32().to_bits());
    }
    let (motc, minv, mcopy) = (ebp - 0x3c, ebp - 0xbc, ebp - 0x7c);
    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[motc, CENTER, mat]);
    let mi2 = m.i16(MI2OUTPUT);
    if mi2 != 0 && motion != 0 {
        o.call(m, MATRIX_COPY_R, &[mat, mcopy]);
        for k in 0..3 {
            m.set_u32(ebp - 0x4c + 4 * k, 0);
        }
        o.call(m, INVERT_MATRIX_R, &[minv, mcopy]);
    }
    let nverts = m.i32(mesh + 0x14);
    let mut nv = nverts; // [ebp-0x190]
    let mut npv: i32 = 0; // [ebp-0x194]: the nodes counted over all polygons
    let cid_mat = m.u32(g::CID_MATERIALS);
    let nmat = o.call(m, RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 300, cid_mat]).eax;
    let npolys = |m: &Mem| m.i32(mesh + 0x18);
    let nth = |m: &mut Mem, o: &mut dyn Outside, poly: u32, out: u32| -> u32 {
        let cid_mat = m.u32(g::CID_MATERIALS);
        let idx = m.i16(poly + 0x12) as i32 as u32;
        o.call(m, ELEMDICT_NTH, &[dict, elem, idx, 1, 3, 300, cid_mat, out]).eax
    };
    // selected by `shadow`: no material takes every polygon unless shadow is 1; else shadow must equal (kind == 6)
    let wanted = |m: &Mem, matl: u32| -> bool {
        if matl == 0 {
            shadow != 1
        } else {
            shadow == (m.u16(matl + 0x1e) == 6) as i32
        }
    };
    let (mut vert_map, mut recs) = (0u32, 0u32);
    if no_out == 0 {
        fprintf(m, o, &[0x086f_2968]);
        let mut poly = m.u32(mesh + 4);
        for _ in 0..npolys(m).max(0) {
            let matl = if nmat != 0 { nth(m, o, poly, 0) } else { 0 };
            if wanted(m, matl) {
                npv = npv.wrapping_add(m.u16(poly + 0x10) as i32);
            }
            poly = poly.wrapping_add(0x18);
        }
        vert_map = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, (nverts as u32) << 2]).eax;
        let ok = vert_map != 0 && {
            recs = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, (npv as u32) << 3]).eax;
            recs != 0
        };
        if !ok {
            o.call(m, SFATAL, &[0x086f_2976]);
        }
    }
    let mut matl = if nmat != 0 { o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, 0, 0]).eax } else { 0 };
    if matl != 0 && m.u16(matl + 0x758) != 0 {
        nv = 0;
        let mut poly = m.u32(mesh + 4);
        for _ in 0..npolys(m).max(0) {
            let mut node = m.u32(poly + 0xc);
            for _ in 0..m.u16(poly + 0x10) {
                if m.i32(node) >= 0 {
                    nv += 1;
                }
                node = node.wrapping_add(0x24);
            }
            poly = poly.wrapping_add(0x18);
        }
    }
    if m.i32(MB_NB_MAX_VERT) < nv {
        let q = ((nv as i64 * 0x68db_8badi64) >> 32) as i32 >> 11;
        let cap = (q - (nv >> 31)).wrapping_mul(0x1388).wrapping_add(0x1388);
        let p = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(SI_GBL_VERT), (cap as u32).wrapping_mul(0x4c)]).eax;
        m.set_u32(SI_GBL_VERT, p);
        m.set_u32(MB_NB_MAX_VERT, cap as u32);
    }
    if motion != 0 {
        if m.u32(model + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(model + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        let rec = m.u32(model + 0x33c);
        if m.u32(rec + 0x22c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, (nv as u32).wrapping_mul(0x4c)]).eax;
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x22c, a);
            let rec = m.u32(model + 0x33c);
            m.set_i32(rec + 0x228, nv);
        } else if m.i32(rec + 0x228) < nv {
            let a = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), (nv as u32).wrapping_mul(0x4c)]).eax;
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x22c, a);
            let rec = m.u32(model + 0x33c);
            m.set_i32(rec + 0x228, nv);
        }
    }
    let blur = matl != 0 && m.u16(matl + 0x758) != 0 && (no_out == 0 || motion != 0);
    let (vout, vin) = (ebp - 0xc, ebp - 0x24);
    let sdebug_str = if shadow != 0 { 0x086f_298fu32 } else { 0x086f_2997 };
    let sdebug_str2 = if shadow != 0 { 0x086f_29cdu32 } else { 0x086f_29d5 };
    if blur {
        let mut poly = m.u32(mesh + 4);
        for _ in 0..npolys(m).max(0) {
            let scale = f32f(m, matl + 0x75c);
            let scale = F80::from_f32(scale.to_f32());
            let mut node = m.u32(poly + 0xc);
            for _ in 0..m.u16(poly + 0x10) {
                let idx = m.i32(node);
                if idx >= 0 {
                    let v = m.u32(mesh).wrapping_add((idx as u32).wrapping_mul(0x14));
                    for k in 0..3u32 {
                        let r = scale * f32f(m, node + 8 + 4 * k) + f32f(m, v + 4 * k);
                        m.set_u32(vin + 4 * k, r.to_f32().to_bits());
                    }
                    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[vout, vin, mat]);
                    let mut skip = false;
                    if motion != 0 {
                        let gv = m.u32(SI_GBL_VERT).wrapping_add((idx as u32).wrapping_mul(0x4c));
                        for k in 0..3u32 {
                            m.set_u32(gv + 8 + 4 * k, quiet(m.u32(vout + 4 * k)));
                        }
                        skip = no_out != 0;
                    }
                    if !skip {
                        if mi2 != 0 {
                            cvec(m, o, quiet(m.u32(vin)), quiet(m.u32(vin + 4)), quiet(m.u32(vin + 8)));
                        } else {
                            cvec(m, o, quiet(m.u32(vout)), quiet(m.u32(vout + 4)), quiet(m.u32(vout + 8)));
                        }
                    }
                }
                node = node.wrapping_add(0x24);
            }
            poly = poly.wrapping_add(0x18);
        }
        let np = npolys(m);
        o.call(m, SDEBUG_R, &[0x086f_29a0, sdebug_str, np as u32, npv as u32]);
    } else if no_out == 0 || motion != 0 {
        for i in 0..nv.max(0) as u32 {
            let src = m.u32(mesh).wrapping_add(i.wrapping_mul(0x14));
            o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[vout, src, mat]);
            let mut skip = false;
            if motion != 0 {
                let gv = m.u32(SI_GBL_VERT).wrapping_add(i.wrapping_mul(0x4c));
                for k in 0..3u32 {
                    m.set_u32(gv + 8 + 4 * k, quiet(m.u32(vout + 4 * k)));
                }
                skip = no_out != 0;
            }
            if !skip {
                m.set_u32(vert_map.wrapping_add(i << 2), 0xffff_ffff);
                if mi2 != 0 {
                    let s = m.u32(mesh).wrapping_add(i.wrapping_mul(0x14));
                    cvec(m, o, quiet(m.u32(s)), quiet(m.u32(s + 4)), quiet(m.u32(s + 8)));
                } else {
                    cvec(m, o, quiet(m.u32(vout)), quiet(m.u32(vout + 4)), quiet(m.u32(vout + 8)));
                }
            }
        }
        let np = npolys(m);
        o.call(m, SDEBUG_R, &[0x086f_29e0, sdebug_str2, np as u32, npv as u32]);
    }
    if no_out == 0 {
        let (inv, tr) = (ebp - 0xfc, ebp - 0x13c);
        if mi2 == 0 {
            let ok = o.call(m, MATRIX_INVERSE, &[mat, inv]).eax as u16;
            if ok == 0 {
                o.call(m, MATRIX_COPY_R, &[SI_IDENTITE, inv]);
                o.call(m, SWARNING_R, &[0x086f_2a20]);
            }
            o.call(m, MATRIX_TRANSPOSE, &[inv, tr]);
        }
        let cid3 = m.u32(g::CID_TEXTURES3D);
        let (texout, texidx, nout) = (ebp - 0x164, ebp - 0x168, ebp - 0x16c);
        let mut cur = recs; // [ebp-0x1a0]
        let mut counter: i32 = 0; // [ebp-0x1a4]
        let mut vcount: i32 = nv; // [ebp-0x1ac]
        let mut avg: u32 = 0;
        let mut poly = m.u32(mesh + 4);
        for _ in 0..npolys(m) {
            let tex;
            if nmat != 0 {
                matl = nth(m, o, poly, texout);
                tex = m.u32(texout);
            } else {
                matl = 0;
                tex = 0;
                m.set_u32(texout, 0);
            }
            if wanted(m, matl) {
                fprintf(m, o, &[0x086f_2a56, vcount as u32]);
                m.local(PAD_CALC);
                calc_poly_textverts(m, o, dict, elem, tex, poly);
                m.release(PAD_CALC);
                let mut node = m.u32(poly + 0xc);
                m.set_u32(texidx, 0);
                let n = m.u16(poly + 0x10);
                for _ in 0..n {
                    let idx = m.i32(node);
                    if idx >= 0 {
                        m.set_u8(cur + 4, 1);
                        m.set_u8(cur + 6, 0);
                        m.set_u8(cur + 5, 0);
                        let slot = vert_map.wrapping_add((idx as u32) << 2);
                        m.set_u32(cur, m.u32(slot));
                        m.set_u32(slot, counter as u32);
                        let nrm = ebp - 0x18;
                        let src = if m.u8(mesh + 0x20) & 2 != 0 { poly } else { node + 8 };
                        if mi2 == 0 {
                            o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[nrm, src, tr]);
                        } else {
                            for k in 0..3 {
                                m.set_u32(nrm + 4 * k, m.u32(src + 4 * k));
                            }
                        }
                        o.call(m, NORMALIZE_VECTOR, &[nrm]);
                        cvec(m, o, quiet(m.u32(nrm)), quiet(m.u32(nrm + 4)), quiet(m.u32(nrm + 8)));
                        let vp = {
                            let mm = m.u32(model + 0x3c);
                            if mm != 0 {
                                m.u32(mm)
                            } else {
                                let a = m.u32(model + 0x38);
                                let b = m.u32(a);
                                m.u32(b)
                            }
                        };
                        let pt = vp.wrapping_add((idx as u32).wrapping_mul(0x14));
                        if tex != 0 {
                            let t3 = o.call(m, ELEMDICT_FIRST, &[dict, tex, 1, 3, 0x1f5, cid3, 0, 0]).eax;
                            let r = texture3d_point(m, o, t3, pt) as u8;
                            m.set_u8(cur + 6, m.u8(cur + 6).wrapping_add(r));
                        }
                        if m.u8(cur + 6) == 0 {
                            let t3 = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x1f4, cid3, 0, 0]).eax;
                            let r = texture3d_point(m, o, t3, pt) as u8;
                            m.set_u8(cur + 6, m.u8(cur + 6).wrapping_add(r));
                        }
                        if tex != 0 {
                            m.local(PAD_T2);
                            let r = texture2d_point(m, o, dict, tex, pt, 0, node, texidx, elem) as u8;
                            m.release(PAD_T2);
                            m.set_u8(cur + 6, m.u8(cur + 6).wrapping_add(r));
                        }
                        m.local(PAD_T2);
                            let r = texture2d_point(m, o, dict, 0, pt, 0, node, texidx, elem) as u8;
                            m.release(PAD_T2);
                        m.set_u8(cur + 6, m.u8(cur + 6).wrapping_add(r));
                        let mv = if mi2 == 0 {
                            o.call(m, crate::mi_patch::MOTION_VECTOR, &[model, motc, idx as u32]).eax
                        } else {
                            o.call(m, crate::mi_patch::MI2_MOTION_VECTOR, &[model, motc, minv, idx as u32]).eax
                        };
                        m.set_u8(cur + 5, mv as u8);
                        let mi2b = m.i16(MI2OUTPUT);
                        if mi2b == 0 && matl != 0 && m.u32(matl + 0xdc8) != 0x10001 {
                            // the average of the alpha bytes of the polygon's nodes (a division by zero faults in the original)
                            let mut sum: i32 = 0;
                            let mut cnt: i32 = 0;
                            let mut nd = m.u32(poly + 0xc);
                            for _ in 0..m.u16(poly + 0x10) {
                                if m.i32(nd) >= 0 {
                                    sum += m.u8(nd + 0x1f) as i32;
                                    cnt += 1;
                                }
                                nd = nd.wrapping_add(0x24);
                            }
                            avg = (!((sum / cnt.max(1)) as u8)) as u32;
                        }
                        if matl != 0 && m.u32(matl + 0xdc8) != 0x10001 {
                            let mi2c = m.i16(MI2OUTPUT);
                            if mi2c == 0 {
                                let hi = avg << 8;
                                let c = |k: u32| F80::from_i64((m.u8(node + 0x1c + k) as u32 | hi) as i64).to_f32().to_bits();
                                cvec(m, o, c(0), c(1), c(2));
                                m.set_u8(cur + 6, m.u8(cur + 6).wrapping_add(1));
                            } else {
                                mi2_colour(m, o, cur, matl, node);
                            }
                        }
                        let adv = (m.u8(cur + 4) as i8 as i32) + (m.u8(cur + 5) as i8 as i32) + (m.u8(cur + 6) as i8 as i32);
                        vcount = vcount.wrapping_add(adv);
                        cur = cur.wrapping_add(8);
                        counter += 1;
                    }
                    node = node.wrapping_add(0x24);
                }
            }
            poly = poly.wrapping_add(0x18);
        }
        // second pass: the polygon records
        cur = recs;
        let mut vnum: i32 = 0; // [ebp-0x1a8]
        vcount = nv;
        counter = 0;
        poly = m.u32(mesh + 4);
        for _ in 0..npolys(m) {
            matl = if nmat != 0 { nth(m, o, poly, 0) } else { 0 };
            if wanted(m, matl) {
                fprintf(m, o, &[0x086f_2a5c, counter as u32]);
                let mut node = m.u32(poly + 0xc);
                for _ in 0..m.u16(poly + 0x10) {
                    let idx = m.i32(node);
                    if idx >= 0 {
                        let val = if blur {
                            let v = vnum;
                            vnum += 1;
                            v
                        } else {
                            idx
                        };
                        fprintf(m, o, &[0x086f_2a62, val as u32]);
                        if m.u8(cur + 4) != 0 {
                            fprintf(m, o, &[0x086f_2a69, vcount as u32]);
                            vcount += 1;
                        }
                        let nt = m.u8(cur + 6) as i8;
                        let mut k = 0i32;
                        while k < nt as i32 {
                            let ch = if k == 0 { 0x09u32 } else { 0x20 };
                            fprintf(m, o, &[0x086f_2a73, ch, vcount as u32]);
                            vcount += 1;
                            k += 1;
                        }
                        if m.u8(cur + 5) != 0 {
                            fprintf(m, o, &[0x086f_2a7a, vcount as u32]);
                            vcount += 1;
                        }
                        cur = cur.wrapping_add(8);
                        counter += 1;
                        fprintf(m, o, &[0x086f_2a81]);
                    }
                    node = node.wrapping_add(0x24);
                }
            }
            poly = poly.wrapping_add(0x18);
        }
        fprintf(m, o, &[0x086f_2a83]);
        // third pass: the surface statements
        poly = m.u32(mesh + 4);
        let mut hnum: i32 = 0; // [ebp-0x1ac]
        for _ in 0..npolys(m) {
            let mut hole = false;
            let mut name = 0u32;
            if nmat != 0 {
                matl = nth(m, o, poly, nout);
                let r = m.u32(nout);
                if r != 0 {
                    let own = m.u32(r);
                    name = m.u32(own + 0x10);
                }
                if o.call(m, crate::mi_patch::EMPTY_STRING, &[name]).eax as u16 != 0 {
                    name = 0;
                }
            } else {
                matl = 0;
            }
            if wanted(m, matl) {
                if m.u8(poly + 0x15) & 8 == 0 {
                    let ext = o.call(m, crate::mi_patch::MAT_NAME_EXTENSION, &[dict, elem]).eax;
                    let named = matl != 0 && name != 0;
                    if ext != 0 {
                        let a1 = if named { name } else { 0x086f_2a94 };
                        let a2 = if named { 0x086f_2a91 } else { 0x086f_2a93 };
                        let a3 = if matl != 0 { matl } else { 0x086f_2a89 };
                        let a4 = if owner_name != 0 { owner_name } else { 0x086f_2a88 };
                        let a5 = if owner_name != 0 { 0x086f_2a86 } else { 0x086f_2a85 };
                        fprintf(m, o, &[0x086f_2a95, a1, a2, a3, a4, a5, ext]);
                    } else {
                        let a3 = if matl != 0 { matl } else { 0x086f_2aaa };
                        let a2 = if named { 0x086f_2ab2 } else { 0x086f_2ab4 };
                        let a1 = if named { name } else { 0x086f_2ab5 };
                        fprintf(m, o, &[0x086f_2ab6, a1, a2, a3]);
                    }
                    if ext != 0 {
                        m.free(ext);
                    }
                }
                let mut nd = m.u32(poly + 0xc);
                let mut cnt = m.u16(poly + 0x10) as i32;
                let mut flags = m.u16(poly + 0x14);
                // GCC's loop over the nodes: valid ones print " idx"; a polygon that starts with a hole
                // restarts at its first valid node (the registers drift as in the original)
                'walk: {
                    if cnt == 0 {
                        break 'walk;
                    }
                    let mut valid = m.i32(nd) >= 0;
                    loop {
                        if !valid {
                            if cnt <= 3 {
                                break 'walk;
                            }
                            let (mut ebx, mut edx) = (1i32, 0u32);
                            while ebx <= 3 {
                                if m.i32(nd.wrapping_add(0x24 * ebx as u32)) < 0 {
                                    nd = nd.wrapping_add(edx);
                                    cnt = cnt.wrapping_add(1).wrapping_sub(ebx);
                                }
                                edx = edx.wrapping_add(0x24);
                                ebx += 1;
                            }
                            hole = true;
                        } else {
                            if hole {
                                if m.u8(poly + 0x15) & 8 == 0 {
                                    fprintf(m, o, &[0x086f_2ac4]);
                                }
                                hole = false;
                            }
                            if m.u8(poly + 0x15) & 8 == 0 {
                                fprintf(m, o, &[0x086f_2ace, hnum as u32]);
                            }
                            hnum += 1;
                            flags = m.u16(poly + 0x14);
                        }
                        nd = nd.wrapping_add(0x24);
                        cnt -= 1;
                        if cnt == 0 {
                            break 'walk;
                        }
                        valid = m.i32(nd) >= 0;
                        if !valid && cnt <= 3 {
                            break 'walk;
                        }
                    }
                }
                if flags & 0x800 == 0 {
                    fprintf(m, o, &[0x086f_2ad2]);
                }
            }
            poly = poly.wrapping_add(0x18);
        }
        if m.i16(MI2OUTPUT) != 0 && m.u16(mesh + 0x52) != 0 && m.u16(mesh + 0x50).wrapping_sub(2) < 2 {
            let view = m.u16(mesh + 0x38) != 0;
            if m.u16(mesh + 0x50) == 2 {
                let d = dw(f32::from_bits(m.u32(mesh + 0x3c)) as f64);
                fprintf(m, o, &[0x086f_2adb, if view { 0x086f_2ad4 } else { 0x086f_2ada }, d[0], d[1]]);
            } else {
                let (d1, d2) = (dw(f32::from_bits(m.u32(mesh + 0x40)) as f64), dw(f32::from_bits(m.u32(mesh + 0x44)) as f64));
                fprintf(m, o, &[0x086f_2b00, if view { 0x086f_2af7 } else { 0x086f_2afd }, d1[0], d1[1], d2[0], d2[1]]);
            }
            if m.u16(mesh + 0x4c) != 0 {
                fprintf(m, o, &[0x086f_2b21, m.u16(mesh + 0x48) as u32, m.u16(mesh + 0x4a) as u32]);
            }
            fprintf(m, o, &[0x086f_2b28]);
        }
        fprintf(m, o, &[0x086f_2b2a]);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, vert_map]);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, recs]);
        let tc = m.u32(TCOORDS);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, tc]);
        m.set_u32(TCOORDS, 0);
        m.set_u32(TCOORDS_SIZE, 0);
    }
    if motion != 0 && m.u32(model + 0x33c) != 0 {
        let gv = m.u32(SI_GBL_VERT);
        for i in 0..nv.max(0) as u32 {
            let rec = m.u32(model + 0x33c);
            let old = m.u32(rec + 0x22c).wrapping_add(i.wrapping_mul(0x4c));
            for k in 0..3u32 {
                let v = quiet(m.u32(gv.wrapping_add(i.wrapping_mul(0x4c)).wrapping_add(8 + 4 * k)));
                m.set_u32(old + 8 + 4 * k, v);
            }
        }
        for k in 0..3u32 {
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(motc + 4 * k)));
        }
    }
    if flip {
        if was3c == 1 {
            o.call(m, MESH_DISPOSE, &[model + 0x3c]);
            m.set_u32(model + 0x3c, save1);
        } else {
            let slot = m.u32(model + 0x38);
            o.call(m, MESH_DISPOSE, &[slot]);
            let slot = m.u32(model + 0x38);
            m.set_u32(slot, save1);
        }
        o.call(m, MESH_DISPOSE, &[model + 0x48]);
        m.set_u32(model + 0x48, save2);
    }
    m.release(0x1fc);
}

/// The colour and alpha of one vertex in mental ray 2 output (the second half of the colour block of
/// [`print_mesh_geometry`]): the vertex colour `node + 0x1c..0x1e`, modified by the material (`matl + 0xdc8`:
/// 1 replaces it by 255 times the material's colour, 3 adds the material's colour), clamped to 0..255 for those
/// two; then the alpha from `matl + 0xdca` and the transparency `matl + 0x744`. Two `print_compressed_vector`
/// calls, each adds one to `rec[6]`.
fn mi2_colour(m: &mut Mem, o: &mut dyn Outside, rec: u32, matl: u32, node: u32) {
    let kind = m.u16(matl + 0xdc8);
    let mut c = [0u32; 3];
    let k255 = F80::from_f64(255.0);
    if kind == 1 {
        for (k, off) in [0x720u32, 0x724, 0x728].into_iter().enumerate() {
            c[k] = (k255 * f32f(m, matl + off)).to_f32().to_bits();
        }
        clamp255(&mut c);
    } else {
        for k in 0..3u32 {
            c[k as usize] = F80::from_i64(m.u8(node + 0x1c + k) as i64).to_f32().to_bits();
        }
        if kind == 3 {
            for (k, off) in [0x720u32, 0x724, 0x728].into_iter().enumerate() {
                let s = F80::from_f32(f32::from_bits(c[k])) + f32f(m, matl + off);
                c[k] = s.to_f32().to_bits();
            }
            clamp255(&mut c);
        }
    }
    cvec(m, o, quiet(c[0]), quiet(c[1]), quiet(c[2]));
    m.set_u8(rec + 6, m.u8(rec + 6).wrapping_add(1));
    let t = f32f(m, matl + 0x744);
    let one = F80::from_i32(1);
    let zero = F80::from_i32(0);
    let a: u32 = match m.u16(matl + 0xdca) {
        1 => {
            if cmp45(t, one) == 0 {
                0
            } else if cmp45(t, zero) == 1 {
                K255
            } else {
                ((one - t) * k255).to_f32().to_bits()
            }
        }
        2 => F80::from_i64((!m.u8(node + 0x1f)) as i64).to_f32().to_bits(),
        _ => {
            let b = F80::from_i64(m.u8(node + 0x1f) as i64);
            if cmp45(t, one) == 0 {
                (k255 - b).to_f32().to_bits()
            } else if cmp45(t, zero) == 1 {
                K255
            } else {
                (k255 - b * t).to_f32().to_bits()
            }
        }
    };
    cvec(m, o, quiet(a), 0, 0);
    m.set_u8(rec + 6, m.u8(rec + 6).wrapping_add(1));
}

// ---------------------------------------------------------------------------------------------
// print_triangles

/// `SI_convertFace2triangle (0x0826c144)`, `SI_convertMeta2Triangles (0x0826d2f8)` and `SI_removeTriangle (0x085efc38)`:
/// other files, called through [`Outside`].
pub const CONVERT_FACE2TRIANGLE: Routine = Routine::new("SI_convertFace2triangle", 0x0826_c144);
pub const CONVERT_META2TRIANGLES: Routine = Routine::new("SI_convertMeta2Triangles", 0x0826_d2f8);
pub const REMOVE_TRIANGLE: Routine = Routine::new("SI_removeTriangle", 0x085e_fc38);
/// `SI_busyCursor` (16 bits).
pub const SI_BUSY_CURSOR: u32 = 0x0867_92b2;
/// Bytes below the locals at the call of `calc_tri_textverts` in `print_triangles`.
const PAD_TRI: u32 = 0x24;

/// A float through the x87 (`fld`, `fchs`, `fstp`).
fn negq(bits: u32) -> u32 {
    (-F80::from_f32(f32::from_bits(bits))).to_f32().to_bits()
}

/// The triangle mesh of a model written as a mental ray polygon group [soft 0x0858e6a8 print_triangles].
///
/// A face or metaball model (kinds 2 and 7) is first turned into triangles by `SI_convertFace2triangle` /
/// `SI_convertMeta2Triangles` (the count at +0x230, the 0x60-byte records at +0x234, each three vertex
/// pointers). Then, in the original's order: the triangle normal and the vertex lines (`v`), the normals
/// that differ from the first vertex's, the texture coordinates through the builders, the motion vectors
/// and the vertex colour and alpha; then the `# n` / `v` / `n` / `t` / `m` index lines and the material
/// names. `no_out` only collects, `motion` keeps the positions for the next frame. A mirrored model walks
/// each triangle the other way round and negates the vertex normals while it is printed.
pub fn print_triangles(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, _unused: i32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return;
    }
    // the original's frame: 0x15c bytes of locals and three saved registers below the saved frame pointer
    let l = m.local(0x16c);
    let ebp = l + 0x168;
    let (no_out, motion) = (no_out as u16, motion as u16);
    let busy = m.u16(SI_BUSY_CURSOR);
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    let tf = m.u32(model + 0x44);
    let base = if m.i32(crate::mi_patch::GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    for k in 0..3u32 {
        let v = m.u32(tf + base + 4 * k);
        m.set_u32(ebp - 0xc8 + 4 * k, v);
    }
    let prod = f32f(m, ebp - 0xc8) * f32f(m, ebp - 0xc4) * f32f(m, ebp - 0xc0);
    let right = m.i16(crate::mi_patch::RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u8(model + 8) & 8 != 0 {
        flip = !flip;
    }
    let out_elem = ebp - 0xf4;
    m.set_u32(out_elem, 0);
    let cid_mat = m.u32(g::CID_MATERIALS);
    let nmat = o.call(m, RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 300, cid_mat]).eax;
    let (matl, mut name2);
    if nmat != 0 {
        let cid_mat = m.u32(g::CID_MATERIALS);
        matl = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, out_elem, 0]).eax;
        let r = m.u32(out_elem);
        name2 = if r != 0 {
            let own = m.u32(r);
            m.u32(own + 0x10)
        } else {
            0
        };
        if o.call(m, crate::mi_patch::EMPTY_STRING, &[name2]).eax as u16 != 0 {
            name2 = 0;
        }
    } else {
        matl = 0;
        name2 = 0;
    }
    for (k, (hi, lo)) in [(0x1cu32, 0x10u32), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let d = f32f(m, model + hi) - f32f(m, model + lo);
        m.set_u32(WIDTH + 4 * k as u32, d.to_f32().to_bits());
    }
    let half = F80::from_f32(0.5);
    for (k, (hi, lo)) in [(0x1cu32, 0x10u32), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let s = f32f(m, model + hi) + f32f(m, model + lo);
        m.set_u32(CENTER + 4 * k as u32, (s * half).to_f32().to_bits());
    }
    let (motc, minv, mcopy) = (ebp - 0x3c, ebp - 0xbc, ebp - 0x7c);
    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[motc, CENTER, mat]);
    let mi2 = m.i16(MI2OUTPUT);
    if mi2 != 0 && motion != 0 {
        o.call(m, MATRIX_COPY_R, &[mat, mcopy]);
        for k in 0..3 {
            m.set_u32(ebp - 0x4c + 4 * k, 0);
        }
        o.call(m, INVERT_MATRIX_R, &[minv, mcopy]);
    }
    m.set_u16(SI_BUSY_CURSOR, 0);
    let kind = m.i32(model + 0x30);
    let (p228, p22c, p230, p234) = (model + 0x228, model + 0x22c, model + 0x230, model + 0x234);
    if kind == 2 {
        let mut slot = m.u32(model + 0x38);
        if m.u16(model + 0x34) != 1 {
            slot = slot.wrapping_add(4);
        }
        let a0 = m.u32(slot);
        let (mesh, m354) = (m.u32(model + 0x48), m.u32(model + 0x354));
        o.call(m, CONVERT_FACE2TRIANGLE, &[a0, mesh, matl, m354, p228, p22c, p230, p234]);
    } else if kind == 7 {
        let (mesh, m354) = (m.u32(model + 0x48), m.u32(model + 0x354));
        o.call(m, CONVERT_META2TRIANGLES, &[mesh, matl, m354, p228, p22c, p230, p234, 1]);
    }
    m.set_u16(SI_BUSY_CURSOR, busy);
    let kname = if m.i32(model + 0x30) == 2 { 0x086f_2b36u32 } else { 0x086f_2b3b };
    let (n230, n228) = (m.u32(p230), m.u32(p228));
    o.call(m, SDEBUG_R, &[0x086f_2b60, kname, n230, n228]);
    if no_out == 0 {
        fprintf(m, o, &[0x086f_2b85]);
    }
    let n3 = m.u32(p230).wrapping_mul(3);
    let a1 = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, n3]).eax;
    let mut a2 = 0u32;
    if a1 != 0 {
        let n3 = m.u32(p230).wrapping_mul(3);
        a2 = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, n3]).eax;
    }
    if a1 == 0 || a2 == 0 {
        o.call(m, SFATAL, &[0x086f_2b93]);
    }
    let mut tri = m.u32(p234);
    let n0 = ebp - 0x30; // the first vertex's normal
    let nrm = ebp - 0x18;
    if no_out == 0 {
        let v0 = m.u32(tri);
        for k in 0..3 {
            let w = m.u32(v0 + 0x14 + 4 * k);
            m.set_u32(n0 + 4 * k, w);
        }
        if mi2 != 0 {
            for k in 0..3 {
                let w = m.u32(n0 + 4 * k);
                m.set_u32(nrm + 4 * k, w);
            }
        } else {
            let v0 = m.u32(tri);
            o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[ebp - 0xc, v0 + 8, mat]);
            let v0 = m.u32(tri);
            crate::mi_calc::vector_plus_vector(m, ebp - 0x24, n0, v0 + 8);
            o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[nrm, ebp - 0x24, mat]);
            crate::mi_calc::vector_minus_vector(m, nrm, nrm, ebp - 0xc);
        }
        o.call(m, NORMALIZE_VECTOR, &[nrm]);
        let (x, y, z) = (m.u32(nrm), m.u32(nrm + 4), m.u32(nrm + 8));
        if flip {
            cvec(m, o, negq(x), negq(y), negq(z));
        } else {
            cvec(m, o, quiet(x), quiet(y), quiet(z));
        }
    }
    if motion != 0 {
        let ntri = m.u32(p230);
        let want = ntri.wrapping_mul(3) as i32;
        if m.u32(model + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(model + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        let rec = m.u32(model + 0x33c);
        if m.u32(rec + 0x22c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, ntri.wrapping_mul(0xe4)]).eax;
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x22c, a);
            let rec = m.u32(model + 0x33c);
            m.set_i32(rec + 0x228, want);
        } else if m.i32(rec + 0x228) < want {
            let a = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), ntri.wrapping_mul(0xe4)]).eax;
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x22c, a);
            let rec = m.u32(model + 0x33c);
            m.set_i32(rec + 0x228, want);
        }
    }
    let mut flags1 = a1;
    let mut counter: i32 = 1;
    let mut shared: i32 = 0;
    let blur = matl != 0 && m.u16(matl + 0x758) != 0;
    let scale_loc = ebp - 0x11c;
    if blur {
        let v = m.u32(matl + 0x75c);
        m.set_u32(scale_loc, v);
    }
    let mut saved_counter: i32 = 0; // [ebp-0x134]
    let mi2b = |m: &Mem| m.i16(MI2OUTPUT) != 0;
    if !(no_out != 0 && m.u8(model + 0x3ee) & 2 == 0) {
        let tex_idx = ebp - 0xf8;
        let mut i: i32 = 0;
        while i < m.i32(p230) {
            let (v0p, v1p, v2p) = (m.u32(tri), m.u32(tri + 4), m.u32(tri + 8));
            let (e1, e2) = (ebp - 0x24, ebp - 0xc);
            if flip {
                crate::mi_calc::vector_minus_vector(m, e1, v1p + 8, v2p + 8);
                crate::mi_calc::vector_minus_vector(m, e2, v0p + 8, v2p + 8);
            } else {
                crate::mi_calc::vector_minus_vector(m, e1, v1p + 8, v0p + 8);
                crate::mi_calc::vector_minus_vector(m, e2, v2p + 8, v0p + 8);
            }
            if no_out == 0 {
                crate::mi_calc::vector_x_vector(m, nrm, e1, e2);
                o.call(m, NORMALIZE_VECTOR, &[nrm]);
                let tex = m.u32(out_elem);
                m.local(PAD_TRI);
                calc_tri_textverts(m, o, dict, elem, tex, tri);
                m.release(PAD_TRI);
                if i % 10 == 0 {
                    fprintf(m, o, &[0x086f_2bab, counter as u32]);
                }
            }
            m.set_u32(tex_idx, 0);
            let mut j: i32 = 0;
            while j <= 2 {
                let (v0p, v1p, v2p) = (m.u32(tri), m.u32(tri + 4), m.u32(tri + 8));
                let edi = match j {
                    0 => {
                        if flip {
                            v2p
                        } else {
                            v0p
                        }
                    }
                    1 => v1p,
                    _ => {
                        if flip {
                            v0p
                        } else {
                            v2p
                        }
                    }
                };
                if flip {
                    for k in 0..3 {
                        let w = negq(m.u32(edi + 0x14 + 4 * k));
                        m.set_u32(edi + 0x14 + 4 * k, w);
                    }
                }
                let cur = ebp - 0xc;
                if blur {
                    let s = f32f(m, scale_loc);
                    for k in 0..3u32 {
                        let v = (s * f32f(m, edi + 0x14 + 4 * k) + f32f(m, edi + 8 + 4 * k)).to_f32().to_bits();
                        m.set_u32(ebp - 0x24 + 4 * k, v);
                    }
                    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[cur, ebp - 0x24, mat]);
                    if no_out == 0 {
                        let src = if mi2b(m) { ebp - 0x24 } else { cur };
                        cvec(m, o, quiet(m.u32(src)), quiet(m.u32(src + 4)), quiet(m.u32(src + 8)));
                    }
                    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[cur, edi + 8, mat]);
                } else {
                    o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[cur, edi + 8, mat]);
                    if no_out == 0 {
                        let src = if mi2b(m) { edi + 8 } else { cur };
                        cvec(m, o, quiet(m.u32(src)), quiet(m.u32(src + 4)), quiet(m.u32(src + 8)));
                    }
                }
                counter += 1;
                let mut t: i32 = 0;
                if no_out == 0 {
                    let eq = (0..3u32).all(|k| f32f(m, n0 + 4 * k).compare(f32f(m, edi + 0x14 + 4 * k)) == Some(std::cmp::Ordering::Equal));
                    let idx = (i as u32).wrapping_mul(3).wrapping_add(j as u32);
                    m.set_u8(a2.wrapping_add(idx), eq as u8);
                    if eq {
                        shared += 1;
                    } else {
                        if mi2b(m) {
                            for k in 0..3 {
                                let w = m.u32(edi + 0x14 + 4 * k);
                                m.set_u32(nrm + 4 * k, w);
                            }
                        } else {
                            crate::mi_calc::vector_plus_vector(m, ebp - 0x24, edi + 0x14, edi + 8);
                            o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[nrm, ebp - 0x24, mat]);
                            crate::mi_calc::vector_minus_vector(m, nrm, nrm, cur);
                        }
                        o.call(m, NORMALIZE_VECTOR, &[nrm]);
                        cvec(m, o, quiet(m.u32(nrm)), quiet(m.u32(nrm + 4)), quiet(m.u32(nrm + 8)));
                        counter += 1;
                    }
                    let esi = edi + 0x20;
                    let cid3 = m.u32(g::CID_TEXTURES3D);
                    if matl != 0 {
                        let tx = m.u32(out_elem);
                        let f = o.call(m, ELEMDICT_FIRST, &[dict, tx, 1, 3, 0x1f5, cid3, 0, 0]).eax;
                        t = texture3d_point(m, o, f, esi) as i32;
                    }
                    if t == 0 {
                        let cid3 = m.u32(g::CID_TEXTURES3D);
                        let f = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x1f4, cid3, 0, 0]).eax;
                        t = texture3d_point(m, o, f, esi) as i32;
                    }
                    if matl != 0 {
                        let tx = m.u32(out_elem);
                        m.local(PAD_T2);
                        let r = texture2d_point(m, o, dict, tx, esi, edi, 0, tex_idx, elem);
                        m.release(PAD_T2);
                        t = t.wrapping_add(r);
                    }
                    m.local(PAD_T2);
                    let r = texture2d_point(m, o, dict, 0, esi, edi, 0, tex_idx, elem);
                    m.release(PAD_T2);
                    t = t.wrapping_add(r);
                }
                if motion != 0 {
                    let rec = m.u32(model + 0x33c);
                    let idx = (i as u32).wrapping_mul(3).wrapping_add(j as u32);
                    let vp = idx.wrapping_mul(0x4c).wrapping_add(m.u32(rec + 0x22c));
                    let ebx = vp.wrapping_add(8);
                    if no_out == 0 {
                        if m.u8(model + 0x3ee) & 2 != 0 {
                            if mi2b(m) {
                                for k in 0..3u32 {
                                    let d = -(f32f(m, cur + 4 * k) - f32f(m, ebx + 4 * k));
                                    m.set_u32(ebp - 0xe4 + 4 * k, d.to_f32().to_bits());
                                }
                                o.call(m, crate::mi_patch::VECTOR_X_MATRIX, &[ebp - 0xf0, ebp - 0xe4, minv]);
                                cvec(m, o, quiet(m.u32(ebp - 0xf0)), quiet(m.u32(ebp - 0xec)), quiet(m.u32(ebp - 0xe8)));
                            } else {
                                let s = f32f(m, scale_loc);
                                let mut r = [0u32; 3];
                                for k in 0..3u32 {
                                    let d = -(f32f(m, cur + 4 * k) - f32f(m, ebx + 4 * k)) * s;
                                    r[k as usize] = d.to_f32().to_bits();
                                }
                                cvec(m, o, r[0], r[1], r[2]);
                            }
                        } else if mi2b(m) {
                            o.call(m, crate::mi_patch::MI2_MOTION_VECTOR, &[model, motc, minv, idx]);
                        } else {
                            o.call(m, crate::mi_patch::MOTION_VECTOR, &[model, motc, idx]);
                        }
                    }
                    for k in 0..3u32 {
                        let w = quiet(m.u32(cur + 4 * k));
                        m.set_u32(ebx + 4 * k, w);
                    }
                }
                m.set_u8(flags1, t as u8);
                flags1 = flags1.wrapping_add(1);
                counter = counter.wrapping_add(t).wrapping_add(motion as i16 as i32);
                if flip {
                    for k in 0..3 {
                        let w = negq(m.u32(edi + 0x14 + 4 * k));
                        m.set_u32(edi + 0x14 + 4 * k, w);
                    }
                }
                j += 1;
            }
            i += 1;
            tri = tri.wrapping_add(0x60);
        }
        if matl != 0 && m.u32(matl + 0xdc8) != 0x10001 {
            let ty = m.u16(matl + 0xdc8);
            let mut c = [0u32; 3];
            if ty == 1 {
                let k = F80::from_f64(255.0);
                for q in 0..3u32 {
                    c[q as usize] = (k * f32f(m, matl + 0x720 + 4 * q)).to_f32().to_bits();
                }
                clamp255(&mut c);
            } else {
                let k = F80::from_f32(179.0);
                c = [k.to_f32().to_bits(); 3];
                if ty == 3 {
                    for q in 0..3u32 {
                        c[q as usize] = (f32f(m, matl + 0x720 + 4 * q) + k).to_f32().to_bits();
                    }
                    clamp255(&mut c);
                }
            }
            for q in 0..3u32 {
                m.set_u32(ebp - 0xf0 + 4 * q, c[q as usize]);
            }
            cvec(m, o, quiet(c[0]), quiet(c[1]), quiet(c[2]));
            saved_counter = counter;
            let alpha: u32 = if m.u16(matl + 0xdca) == 2 {
                0
            } else {
                let a = f32f(m, matl + 0x744);
                if cmp45(a, F80::from_i32(1)) == 0 {
                    0
                } else if cmp45(a, F80::from_i32(0)) == 1 {
                    K255
                } else {
                    ((F80::from_i32(1) - a) * F80::from_f64(255.0)).to_f32().to_bits()
                }
            };
            m.set_u32(ebp - 0xf0, alpha);
            cvec(m, o, quiet(alpha), 0, 0);
        }
    }
    if no_out != 0 {
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, a1]);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, a2]);
    } else {
        fprintf(m, o, &[0x086f_2bb1]);
        let mut fp = a1;
        counter = 1;
        let mut i: i32 = 0;
        while i < m.i32(p230) {
            fprintf(m, o, &[0x086f_2bb3, (i as u32).wrapping_mul(3)]);
            let mut j: i32 = 0;
            while j <= 2 {
                fprintf(m, o, &[0x086f_2bb9, counter as u32]);
                counter += 1;
                let idx = (i as u32).wrapping_mul(3).wrapping_add(j as u32);
                let v = if m.u8(a2.wrapping_add(idx)) == 0 {
                    let c = counter;
                    counter += 1;
                    c
                } else {
                    0
                };
                fprintf(m, o, &[0x086f_2bc0, v as u32]);
                let cnt = m.u8(fp) as i8 as i32;
                let mut s = 0;
                while s < cnt {
                    // the two characters at 0x086f2bc6 are a space and a tab (not in the check's image)
                    let ch: i32 = if s == 0 { 9 } else { 0x20 };
                    fprintf(m, o, &[0x086f_2bc9, ch as u32, counter as u32]);
                    counter += 1;
                    s += 1;
                }
                fp = fp.wrapping_add(1);
                if motion != 0 && m.u16(model + 0x3ee) != 0 {
                    fprintf(m, o, &[0x086f_2bd0, counter as u32]);
                    counter += 1;
                }
                if matl != 0 && m.u32(matl + 0xdc8) != 0x10001 {
                    fprintf(m, o, &[0x086f_2bd6, saved_counter as u32, saved_counter.wrapping_add(1) as u32]);
                }
                fprintf(m, o, &[0x086f_2be1]);
                j += 1;
            }
            i += 1;
        }
        fprintf(m, o, &[0x086f_2be3]);
        counter = 0;
        let mut i: i32 = 0;
        while i < m.i32(p230) {
            let ext = o.call(m, crate::mi_patch::MAT_NAME_EXTENSION, &[dict, elem]).eax;
            let named = matl != 0 && name2 != 0;
            let c = counter as u32;
            if ext != 0 {
                let e = if named { name2 } else { 0x086f_2bf4 };
                let d = if named { 0x086f_2bf1 } else { 0x086f_2bf3 };
                let cc = if matl != 0 { matl } else { 0x086f_2be9 };
                let b = if owner_name != 0 { owner_name } else { 0x086f_2be8 };
                let a = if owner_name != 0 { 0x086f_2be6 } else { 0x086f_2be5 };
                fprintf(m, o, &[0x086f_2c00, e, d, cc, b, a, ext, c, c.wrapping_add(1), c.wrapping_add(2)]);
            } else {
                let cc = if matl != 0 { matl } else { 0x086f_2c1f };
                let d = if named { 0x086f_2c27 } else { 0x086f_2c29 };
                let e = if named { name2 } else { 0x086f_2c2a };
                fprintf(m, o, &[0x086f_2c2b, e, d, cc, c, c.wrapping_add(1), c.wrapping_add(2)]);
            }
            if ext != 0 {
                m.free(ext);
            }
            i += 1;
            counter = counter.wrapping_add(3);
        }
        fprintf(m, o, &[0x086f_2c43]);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, a1]);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, a2]);
        o.call(m, SDEBUG_R, &[0x086f_2c4f, shared as u32]);
        let tc = m.u32(TCOORDS);
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, tc]);
        m.set_u32(TCOORDS, 0);
        m.set_u32(TCOORDS_SIZE, 0);
    }
    if motion != 0 && m.u32(model + 0x33c) != 0 {
        for k in 0..3u32 {
            let rec = m.u32(model + 0x33c);
            m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(motc + 4 * k)));
        }
    }
    o.call(m, REMOVE_TRIANGLE, &[model, 0]);
    m.release(0x16c);
}
