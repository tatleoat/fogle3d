// SPDX-License-Identifier: MIT
//! `mi_patch.c`: the translator's NURBS-surface and patch geometry, written as mental ray
//! scene text (package 63, finding 0036).
//!
//! Every byte the translator writes goes through `SI_mrpipeFPRINTF (0x0842c68c)`, which is
//! not ported: a routine here calls it through [`Outside`] with the format string's address
//! and the argument words, exactly as the original pushes them (a `double` as two words,
//! low word first). The text itself is therefore compared by a check as format address plus
//! words.

use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `SI_mrpipeFPRINTF (0x0842c68c)`: printf on the pipe to mental ray.
pub const MRPIPE_FPRINTF: Routine = Routine::new("SI_mrpipeFPRINTF", 0x0842_c68c);

/// A `double` as the two stack words of a call.
pub(crate) fn dw(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

pub(crate) fn fprintf(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

/// The texts of `mi_patch.c` in `.rodata`, by address.
pub mod text {
    pub const SPACE_A: u32 = 0x086f_17e4; // " "
    pub const NEWLINE_TABS_A: u32 = 0x086f_17e6; // "\n\t\t\t"
    pub const SEP_F_A: u32 = 0x086f_17eb; // "%s%f"
    pub const SPACE_B: u32 = 0x086f_17f0;
    pub const NEWLINE_TABS_B: u32 = 0x086f_17f2;
    pub const SEP_F_B: u32 = 0x086f_17f7;
    pub const NEWLINE: u32 = 0x086f_17fc; // "\n"
}

/// A NURBS curve's knot vector printed (`print_nurbs_curve`) [soft 0x08586670 print_nurbs_curve].
///
/// `dir` is the curve's `SI_NurbsDir`: order at +0, number of knots at +8, knots at +0xc,
/// closed flag at +0x20. An open curve prints its knots with the first and the last repeated;
/// a closed one prints `order-1` extrapolated knots either side (the 80-bit differences are
/// stored to `double` where the original stores them). Six values a line.
pub fn print_nurbs_curve(m: &mut Mem, o: &mut dyn Outside, dir: u32) {
    let n = m.i32(dir + 8);
    let mut count: i32 = 0;
    if m.i16(dir + 0x20) == 0 {
        let mut k: i32 = -1;
        let mut off: i32 = -16;
        let knots = m.u32(dir + 0xc);
        while k <= n {
            // the original steps its pointer: the first and last value are repeated
            let at = if k < 0 {
                knots
            } else if k < n {
                knots.wrapping_add((k as u32).wrapping_mul(8))
            } else {
                knots.wrapping_add(off as u32)
            };
            let sep = if count % 6 != 0 { text::SPACE_B } else { text::NEWLINE_TABS_B };
            let v = dw(m.f64(at));
            fprintf(m, o, &[text::SEP_F_B, sep, v[0], v[1]]);
            off += 8;
            k += 1;
            count += 1;
        }
    } else {
        let order1 = m.i32(dir).wrapping_sub(1);
        let knots = m.u32(dir + 0xc);
        let mut i = order1.wrapping_neg();
        while i < order1.wrapping_add(n) {
            let kn = |m: &Mem, idx: i32| m.f64(knots.wrapping_add((idx as u32).wrapping_mul(8)));
            let v: f64 = if i < 0 {
                // k[0] - (k[n-1] - k[n+i-1]), all in the 80-bit register
                let d = F80::from_f64(kn(m, n - 1)) - F80::from_f64(kn(m, n + i - 1));
                (F80::from_f64(m.f64(knots)) - d).to_f64()
            } else if i < n {
                kn(m, i)
            } else {
                // (k[i-n+1] - k[0]) + k[n-1]
                let d = F80::from_f64(kn(m, i - n + 1)) - F80::from_f64(m.f64(knots));
                (d + F80::from_f64(kn(m, n - 1))).to_f64()
            };
            let sep = if count % 6 != 0 { text::SPACE_A } else { text::NEWLINE_TABS_A };
            let w = dw(v);
            fprintf(m, o, &[text::SEP_F_A, sep, w[0], w[1]]);
            i += 1;
            count += 1;
        }
    }
    fprintf(m, o, &[text::NEWLINE]);
}

/// `SI_SampleKnotVector (0x084d727c)`: `(dir, per, &array, &count)`; ported in `si-nurbs`.
pub const SAMPLE_KNOT_VECTOR: Routine = Routine::new("SI_SampleKnotVector", 0x084d_727c);

/// The translator's globals in `.bss` (their names are the original's symbols).
pub mod g {
    /// `ivert`: the index of the next texture vertex (32 bits).
    pub const IVERT: u32 = 0x0882_9f50;
    /// `tex_nu`: the texture grid's width (16 bits, signed).
    pub const TEX_NU: u32 = 0x0882_9f54;
    /// `tex_nv`: its height (16 bits, signed).
    pub const TEX_NV: u32 = 0x0882_9f56;
    /// `SI_cid.MATERIALS`, `SI_cid.TEXTURES3D`, `SI_cid.TEXTURES2D`: the class ids of the
    /// element dictionary (32 bits each, set when the dictionaries are built).
    /// `nvert`: the number of vertices written so far (32 bits).
    pub const NVERT: u32 = 0x0882_9f4c;
    pub const CID_MATERIALS: u32 = 0x0878_167c;
    pub const CID_TEXTURES3D: u32 = 0x0878_16cc;
    pub const CID_TEXTURES2D: u32 = 0x0878_16c8;
}

/// `SI_elemdictGetFirstRelatedElem (0x08138094)` and `SI_elemdictGetNextRelatedElem
/// (0x08138104)`: `(dict, elem, 1, 3, relation, cid, &related, &index)`; both ported in `si-dict`.
pub const ELEMDICT_FIRST: Routine = Routine::new("SI_elemdictGetFirstRelatedElem", 0x0813_8094);
pub const ELEMDICT_NEXT: Routine = Routine::new("SI_elemdictGetNextRelatedElem", 0x0813_8104);

/// More of the texts, in `.rodata`.
pub mod text2 {
    pub const VOLUME: u32 = 0x086f_1f44; // "volume "
    pub const EMPTY: u32 = 0x086f_1f4c; // ""
    pub const STEXTURE: u32 = 0x086f_1f4d; // "\n\t\t%stexture"
    pub const BEZIER1_FF_A: u32 = 0x086f_1f5a; // "\n\t\t\t_bezier1 %f %f"
    pub const BEZIER1_FF_B: u32 = 0x086f_1f6d; // "\n\t\t\t_bezier1 %f %f\n"
    pub const DDDD: u32 = 0x086f_1f81; // "\t\t\t%d %d %d %d\n"
    pub const BEZIER1_U: u32 = 0x086f_1f91; // "\n\t\t\t_bezier1"
    pub const SPACE_U: u32 = 0x086f_1f9e; // " "
    pub const LINE_U: u32 = 0x086f_1fa0; // "\n\t\t\t\t"
    pub const SEP_F_U: u32 = 0x086f_1fa6; // "%s%f"
    pub const NL_U: u32 = 0x086f_1fab; // "\n"
    pub const BEZIER1_V: u32 = 0x086f_1fad; // "\n\t\t\t_bezier1"
    pub const SPACE_V: u32 = 0x086f_1fba;
    pub const LINE_V: u32 = 0x086f_1fbc;
    pub const SEP_F_V: u32 = 0x086f_1fc2;
    pub const NL_V: u32 = 0x086f_1fc7; // "\n\n"
    pub const SPACE_I: u32 = 0x086f_1fca;
    pub const LINE_I: u32 = 0x086f_1fcc; // "\n\t\t\t"
    pub const SEP_D: u32 = 0x086f_1fd1; // "%s%d"
    pub const NL_END: u32 = 0x086f_1fd6; // "\n"
}

/// A surface's texture reference printed (`print_nurbs_texture_ref`) [soft 0x08589488 print_nurbs_texture_ref].
///
/// `srf` is the `SI_NurbsSrf`, `kind` the texture's kind (7 and 8 print nothing; 1 to 3 are
/// volume textures; 4 prints the surface's two parameter ranges and four consecutive vertex
/// numbers; any other prints both knot samplings and the grid of vertex numbers). The third
/// argument of the original (a `short`) is not used. The knot samples are allocated by
/// `SI_SampleKnotVector` and, as in the original, never freed.
pub fn print_nurbs_texture_ref(m: &mut Mem, o: &mut dyn Outside, srf: u32, kind: i32) {
    use text2 as t;
    let locals = m.local(8);
    let (arr, cnt) = (locals, locals + 4);
    m.set_u32(arr, 0);
    m.set_i32(cnt, 0);
    if (kind.wrapping_sub(7) as u32) > 1 {
        let name = if (kind.wrapping_sub(1) as u32) < 3 { t::VOLUME } else { t::EMPTY };
        fprintf(m, o, &[t::STEXTURE, name]);
        if kind == 4 {
            let a = [dw(m.f64(srf + 0x10)), dw(m.f64(srf + 0x18))];
            fprintf(m, o, &[t::BEZIER1_FF_A, a[0][0], a[0][1], a[1][0], a[1][1]]);
            let b = [dw(m.f64(srf + 0x34)), dw(m.f64(srf + 0x3c))];
            fprintf(m, o, &[t::BEZIER1_FF_B, b[0][0], b[0][1], b[1][0], b[1][1]]);
            let iv = m.u32(g::IVERT);
            fprintf(m, o, &[t::DDDD, iv, iv.wrapping_add(1), iv.wrapping_add(2), iv.wrapping_add(3)]);
            m.set_u32(g::IVERT, iv.wrapping_add(4));
        } else {
            fprintf(m, o, &[t::BEZIER1_U]);
            let per = m.u32(srf + 0x70);
            o.call(m, SAMPLE_KNOT_VECTOR, &[srf, per, arr, cnt]);
            let mut i: i32 = 0;
            while i < m.i16(g::TEX_NU) as i32 {
                let v = dw(m.f64(m.u32(arr).wrapping_add((i as u32).wrapping_mul(8))));
                let sep = if i & 3 != 0 { t::SPACE_U } else { t::LINE_U };
                fprintf(m, o, &[t::SEP_F_U, sep, v[0], v[1]]);
                i += 1;
            }
            fprintf(m, o, &[t::NL_U]);
            fprintf(m, o, &[t::BEZIER1_V]);
            let per = m.u32(srf + 0x74);
            o.call(m, SAMPLE_KNOT_VECTOR, &[srf + 0x24, per, arr, cnt]);
            let mut j: i32 = 0;
            while j < m.i16(g::TEX_NV) as i32 {
                let v = dw(m.f64(m.u32(arr).wrapping_add((j as u32).wrapping_mul(8))));
                let sep = if j & 3 != 0 { t::SPACE_V } else { t::LINE_V };
                fprintf(m, o, &[t::SEP_F_V, sep, v[0], v[1]]);
                j += 1;
            }
            fprintf(m, o, &[t::NL_V]);
            let mut row: i32 = 0;
            while row < m.i16(g::TEX_NV) as i32 {
                let mut col: i32 = 0;
                while col < m.i16(g::TEX_NU) as i32 {
                    let nu = m.i16(g::TEX_NU) as i32;
                    let n = nu.wrapping_mul(row).wrapping_add(m.i32(g::IVERT)).wrapping_add(col);
                    let sep = if col % 10 != 0 { t::SPACE_I } else { t::LINE_I };
                    fprintf(m, o, &[t::SEP_D, sep, n as u32]);
                    col += 1;
                }
                row += 1;
            }
            let add = (m.i16(g::TEX_NU) as i32).wrapping_mul(m.i16(g::TEX_NV) as i32);
            m.set_i32(g::IVERT, m.i32(g::IVERT).wrapping_add(add));
            fprintf(m, o, &[t::NL_END]);
        }
    }
    m.release(8);
}

/// All the texture references of a surface's element (`print_nurb_tcoord_ref`)
/// [soft 0x08589038 print_nurb_tcoord_ref].
///
/// Nothing for a null dictionary or element, or an element without a definition (+4). Else
/// `ivert` is set to `first_vertex`; the surface is the definition's field +0x48. The
/// material related to the element (relation 300) is looked for first; its 3D texture
/// (relation 0x1f5), or else the element's own (0x1f4), is printed with kind 0x29a; then the
/// material's 2D textures (0x191) and the element's own (400), each with the kind in its
/// record at +0x2a.
pub fn print_nurb_tcoord_ref(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, first_vertex: i32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let srf = m.u32(def + 0x48);
    let locals = m.local(8);
    let (mat, idx) = (locals, locals + 4);
    let cid_mat = m.u32(g::CID_MATERIALS);
    o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, mat, 0]);
    m.set_i32(g::IVERT, first_vertex);
    let material = m.u32(mat);
    let cid3 = m.u32(g::CID_TEXTURES3D);
    let mut t = o.call(m, ELEMDICT_FIRST, &[dict, material, 1, 3, 0x1f5, cid3, 0, 0]).eax;
    if t == 0 {
        let cid3 = m.u32(g::CID_TEXTURES3D);
        t = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x1f4, cid3, 0, 0]).eax;
    }
    if t != 0 {
        print_nurbs_texture_ref(m, o, srf, 0x29a);
    }
    for (owner, relation) in [(material, 0x191u32), (elem, 400)] {
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        while t != 0 {
            print_nurbs_texture_ref(m, o, srf, m.i16(t + 0x2a) as i32);
            let cid2 = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        }
    }
    m.release(8);
}

/// `print_compressed_vector (0x08590928)`: three `float`s (one word each), `mi_poly.c`.
pub const PRINT_COMPRESSED_VECTOR: Routine = Routine::new("print_compressed_vector", 0x0859_0928);
/// `textureVert (0x085944c0)`: `(vector, &out[2], lclmap)`, `mi_tools.c`.
pub const TEXTURE_VERT: Routine = Routine::new("textureVert", 0x0859_44c0);

/// The `float` just under 1.0 the translator clamps texture coordinates to (`.rodata`
/// 0x08672558, 0.99999899).
pub const JUST_BELOW_ONE: u32 = 0x3f7f_ffef;

/// A `float` taken through an x87 register and stored again (`fld`, `fstp`): a signalling NaN
/// comes out quiet.
fn via_x87(bits: u32) -> u32 {
    F80::from_f32(f32::from_bits(bits)).to_f32().to_bits()
}

pub(crate) fn cvec(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, c: u32) {
    o.call(m, PRINT_COMPRESSED_VECTOR, &[a, b, c]);
}

/// A patch's texture coordinates printed (`print_patch_texture`) [soft 0x08588bec print_patch_texture].
///
/// `map` is the texture map record (its kind, a `u16`, at +0x2a: 7 and 8 print nothing and
/// answer 0; 4 prints the four corners of the unit square and answers 4), `nu` by `nv` the
/// grid, `vecs` the grid's vectors (twelve bytes each, indexed `(i + j*nv)*3` floats). For any
/// other kind each vector goes through `textureVert`; u and v at or above 1.0 are clamped to
/// [`JUST_BELOW_ONE`] (a NaN is left); w is printed as 0. Answers `nu*nv`. `nvert` counts the
/// vectors.
pub fn print_patch_texture(m: &mut Mem, o: &mut dyn Outside, map: u32, nu: i32, nv: i32, vecs: u32) -> i32 {
    let kind = m.u16(map + 0x2a);
    if kind.wrapping_sub(7) <= 1 {
        return 0;
    }
    if kind == 4 {
        let one = JUST_BELOW_ONE;
        cvec(m, o, 0, one, 0);
        cvec(m, o, one, one, 0);
        cvec(m, o, 0, 0, 0);
        cvec(m, o, one, 0, 0);
        fprintf(m, o, &[0x086f_1eaa]);
        m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(4));
        return 4;
    }
    let total = nu.wrapping_mul(nv);
    let tc = m.local(8);
    let mut i: i32 = 0;
    while i < nv {
        let mut j = nu.wrapping_sub(1);
        let mut idx = i.wrapping_add(j.wrapping_mul(nv)).wrapping_mul(3);
        while j >= 0 {
            o.call(m, TEXTURE_VERT, &[vecs.wrapping_add((idx as u32).wrapping_mul(4)), tc, map]);
            // `fcompp` against 1.0: at or above 1.0, and not unordered
            if m.f32(tc) >= 1.0 {
                m.set_u32(tc, JUST_BELOW_ONE);
            }
            if m.f32(tc + 4) >= 1.0 {
                m.set_u32(tc + 4, JUST_BELOW_ONE);
            }
            let (u, v) = (via_x87(m.u32(tc)), via_x87(m.u32(tc + 4)));
            cvec(m, o, u, v, 0);
            m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
            idx = idx.wrapping_add(nv.wrapping_neg().wrapping_mul(3));
            j -= 1;
        }
        i += 1;
    }
    fprintf(m, o, &[0x086f_1eac]);
    m.release(8);
    total
}

/// A patch's 3D texture coordinates printed (`print_patch_texture3d`)
/// [soft 0x08588d6c print_patch_texture3d]: every vector of the `nu` by `nv` grid in the order
/// of [`print_patch_texture`], unclamped, then a new line. It returns `nu * nv` (the original
/// leaves the product in `eax`).
pub fn print_patch_texture3d(m: &mut Mem, o: &mut dyn Outside, nu: i32, nv: i32, vecs: u32) -> i32 {
    let mut i: i32 = 0;
    while i < nv {
        let mut j = nu.wrapping_sub(1);
        let mut idx = i.wrapping_add(j.wrapping_mul(nv)).wrapping_mul(3);
        while j >= 0 {
            let at = vecs.wrapping_add((idx as u32).wrapping_mul(4));
            let (x, y, z) = (via_x87(m.u32(at)), via_x87(m.u32(at + 4)), via_x87(m.u32(at + 8)));
            cvec(m, o, x, y, z);
            m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
            idx = idx.wrapping_add(nv.wrapping_neg().wrapping_mul(3));
            j -= 1;
        }
        i += 1;
    }
    fprintf(m, o, &[0x086f_1eae]);
    nu.wrapping_mul(nv)
}

/// `SI_memtrackAllocate (0x08599610)`: `(&si_MemParams, size)`, `renu_memory.c`.
pub const MEMTRACK_ALLOCATE: Routine = Routine::new("SI_memtrackAllocate", 0x0859_9610);
/// `SI_nurbsSrfValue (0x084d6728)`: `(&point[3 doubles], srf, u, v)`; ported in `si-nurbs` on its own state.
pub const NURBS_SRF_VALUE: Routine = Routine::new("SI_nurbsSrfValue", 0x084d_6728);
/// `si_MemParams`: the tracked allocator's parameter block (`.bss`).
pub const SI_MEM_PARAMS: u32 = 0x0882_9f14;

/// A surface sampled on its knot grid (`SI_nurbSurface`) [soft 0x08589708 SI_nurbSurface].
///
/// The knot vectors of the two directions are sampled with `per_u` and `per_v` points per
/// span; `*nu_out` and `*nv_out` get the two counts and `*out` a block of `nu*nv` vectors
/// (twelve bytes, three `float`s) from the tracked allocator. The surface is evaluated at
/// every grid point, `v` from the last sample down, `u` from the last sample down in step
/// with `j` (the vector for `(j, i)` lands at `(j*nv + i)*3` floats); each coordinate is
/// stored rounded from `double` to `float`. The two sample arrays are freed.
pub fn si_nurb_surface(m: &mut Mem, o: &mut dyn Outside, srf: u32, per_u: i32, per_v: i32, out: u32, nu_out: u32, nv_out: u32) {
    let l = m.local(16 + 24);
    let (ku, nku, kv, nkv, pt) = (l, l + 4, l + 8, l + 12, l + 16);
    m.set_u32(ku, 0);
    m.set_i32(nku, 0);
    m.set_u32(kv, 0);
    m.set_i32(nkv, 0);
    o.call(m, SAMPLE_KNOT_VECTOR, &[srf, per_u as u32, ku, nku]);
    o.call(m, SAMPLE_KNOT_VECTOR, &[srf + 0x24, per_v as u32, kv, nkv]);
    let (cu, cv) = (m.i32(nku), m.i32(nkv));
    m.set_i32(nu_out, cu);
    m.set_i32(nv_out, cv);
    let block = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, cu.wrapping_mul(cv).wrapping_mul(12) as u32]).eax;
    m.set_u32(out, block);
    let mut i = cv.wrapping_sub(1);
    while i >= 0 {
        let v = m.f64(m.u32(kv).wrapping_add((i as u32).wrapping_mul(8)));
        let mut j: i32 = 0;
        while j < m.i32(nku) {
            let u = m.f64(m.u32(ku).wrapping_add(((m.i32(nku) - 1 - j) as u32).wrapping_mul(8)));
            let (ub, vb) = (dw(u), dw(v));
            o.call(m, NURBS_SRF_VALUE, &[pt, srf, ub[0], ub[1], vb[0], vb[1]]);
            let at = m.u32(out).wrapping_add(((j.wrapping_mul(m.i32(nkv)).wrapping_add(i)) as u32).wrapping_mul(12));
            m.set_f32(at, m.f64(pt) as f32);
            m.set_f32(at + 4, m.f64(pt + 8) as f32);
            m.set_f32(at + 8, m.f64(pt + 16) as f32);
            j += 1;
        }
        i -= 1;
    }
    if m.u32(ku) != 0 {
        m.free(m.u32(ku));
        m.set_u32(ku, 0);
    }
    if m.u32(kv) != 0 {
        m.free(m.u32(kv));
        m.set_u32(kv, 0);
    }
    m.release(16 + 24);
}

/// The routines `print_nurb_tcoord` calls and the globals it reads.
pub const SRF_COPY: Routine = Routine::new("SI_nurbsSrfCopy", 0x084d_c1a8);
pub const SRF_INVERSE: Routine = Routine::new("SI_nurbsSrfInverse", 0x084d_caf8);
pub const SRF_DISPOSE: Routine = Routine::new("SI_nurbsSrfDispose", 0x084d_ba5c);
pub const MEMTRACK_FREE: Routine = Routine::new("SI_memtrackFree", 0x0859_96e0);
pub const UPDATE_TXT_SUPPORT: Routine = Routine::new("updateTxtSupport", 0x0859_4c04);
pub const RELATION_CHECK: Routine = Routine::new("SI_elemdictRelationCheck", 0x0813_7f0c);
/// `SI_glbManip` (32 bits) and `SI_rightHand` (16 bits).
pub const GLB_MANIP: u32 = 0x0878_1560;
pub const RIGHT_HAND: u32 = 0x0867_93ec;
/// "\n# texs\n".
pub const TEXS: u32 = 0x086f_1fd8;

/// A NURBS surface's texture coordinates written (`print_nurb_tcoord`)
/// [soft 0x08589884 print_nurb_tcoord]; returns the number of vectors written.
///
/// Nothing (0) for a null dictionary or element or an element without definition (+4). The
/// handedness is decided by the sign of the product of three scales of the definition's
/// transform (at +0x3a0 with the global manipulation flag set, else +0x16c; a negative
/// product or a NaN-free positive one against `SI_rightHand`, flipped again by bit 3 of the
/// definition's flags). When it comes out flipped the surface (the slot at +0x38, the second
/// when the kind at +0x34 is not 1) is replaced by an inverted copy for the duration. The
/// surface is sampled (`SI_nurbSurface`, with the counts at +0x70/+0x74 of the definition's
/// record at +0x48), `tex_nu` and `tex_nv` set, `# texs` written, then the 3D texture (when
/// the material has relation 0x1f5 or the element 0x1f4) and every 2D texture of the
/// material (0x191) and the element (400) written through `print_patch_texture`, after
/// `updateTxtSupport`. The sampled vectors are freed, a made copy disposed of and the
/// original put back.
pub fn print_nurb_tcoord(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> i32 {
    if dict == 0 || elem == 0 {
        return 0;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return 0;
    }
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| f32::from_bits(m.u32(tf + base + off)) as f64;
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    // the compare is "0 against product": negative only when the product is strictly less
    let mut flip = if prod < 0.0 { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let slot = |m: &Mem| m.u32(def + 0x38).wrapping_add(if m.i16(def + 0x34) == 1 { 0 } else { 4 });
    let mut saved = 0;
    if flip {
        saved = m.u32(slot(m));
        let copy = o.call(m, SRF_COPY, &[saved]).eax;
        let at = slot(m);
        m.set_u32(at, copy);
        let cur = m.u32(slot(m));
        o.call(m, SRF_INVERSE, &[cur, 1, 0]);
    }
    let srf = m.u32(slot(m));
    let dsrf = m.u32(def + 0x48);
    let mut count: i32 = 0;
    let l = m.local(20);
    let (mat, nv, nu, vecs, idx) = (l, l + 4, l + 8, l + 12, l + 16);
    let cid_mat = m.u32(g::CID_MATERIALS);
    o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, mat, 0]);
    let (per_u, per_v) = (m.i32(dsrf + 0x70), m.i32(dsrf + 0x74));
    si_nurb_surface(m, o, srf, per_u, per_v, vecs, nu, nv);
    m.set_i16(g::TEX_NU, m.i32(nu) as i16);
    m.set_i16(g::TEX_NV, m.i32(nv) as i16);
    fprintf(m, o, &[TEXS]);
    let cid3 = m.u32(g::CID_TEXTURES3D);
    let mut has3d = o.call(m, RELATION_CHECK, &[dict, m.u32(mat), 1, 0x1f5, cid3]).eax as u16 != 0;
    if !has3d {
        let cid3 = m.u32(g::CID_TEXTURES3D);
        has3d = o.call(m, RELATION_CHECK, &[dict, elem, 1, 0x1f4, cid3]).eax as u16 != 0;
    }
    if has3d {
        count = print_patch_texture3d(m, o, m.i32(nu), m.i32(nv), m.u32(vecs));
    }
    for (owner_is_mat, relation) in [(true, 0x191u32), (false, 0x190)] {
        let owner = if owner_is_mat { m.u32(mat) } else { elem };
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        while t != 0 {
            o.call(m, UPDATE_TXT_SUPPORT, &[def, t]);
            let n = print_patch_texture(m, o, t, m.i32(nu), m.i32(nv), m.u32(vecs));
            count = count.wrapping_add(n);
            let cid2 = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        }
    }
    o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, m.u32(vecs)]);
    m.release(20);
    if flip {
        let at = slot(m);
        o.call(m, SRF_DISPOSE, &[at]);
        let at = slot(m);
        m.set_u32(at, saved);
    }
    count
}

/// `sfatal (0x08598ba0)`: `(format, ...)`, `mi_tools.c`; reports a fatal translator error.
pub const SFATAL: Routine = Routine::new("sfatal", 0x0859_8ba0);
/// The two static basis matrices of the conversion to Bezier form (16 `float`s each, `.data`).
pub const BSPLINE_MAT: u32 = 0x086f_2000;
pub const CARDINAL_MAT: u32 = 0x086f_2040;
/// The patch record is 0x204 bytes: `+4`/`+8` basis kind of u and v (0 Bezier-like, 1 Bezier,
/// 2 B-spline, 3 cardinal), `+0xc`/`+0xe` point counts, `+0x10`/`+0x12` closed in u and v,
/// `+0x14`/`+0x18` tensions, `+0x1a4` the points (four `float`s each, `nu * nv`, v fastest).
const PATCH_SIZE: u32 = 0x204;

fn alloc_patch(m: &mut Mem, o: &mut dyn Outside, msg: u32) -> u32 {
    let p = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, PATCH_SIZE]).eax;
    if p == 0 {
        o.call(m, SFATAL, &[msg]);
    }
    p
}

fn alloc_points(m: &mut Mem, o: &mut dyn Outside, nu: u16, nv: u16, msg: u32) -> u32 {
    let n = (nu as u32).wrapping_mul(nv as u32).wrapping_shl(4);
    let p = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, n]).eax;
    if p == 0 {
        o.call(m, SFATAL, &[msg]);
    }
    p
}

/// `sum(c[i] * p[i])` the way the x87 does it: 80-bit products added in order, one rounding to
/// `float` at the store.
fn dot4(m: &Mem, c: u32, p: [u32; 4], k: u32) -> f32 {
    let mut acc = F80::from_f32(m.f32(c)) * F80::from_f32(m.f32(p[0] + k));
    for i in 1..4 {
        acc = acc + F80::from_f32(m.f32(c + 4 * i as u32)) * F80::from_f32(m.f32(p[i as usize] + k));
    }
    acc.to_f32()
}

/// The patch with its points closed up (`close_patch`) [soft 0x08589d54 close_patch].
///
/// A copy of the patch whose point grid wraps: in v one more row of points when the patch is
/// closed in v and its basis is 0 (one), 1 when the count is a multiple of 3 (one), 2 or 3
/// (three); the same in u. The new grid takes point `(i mod nu, j mod nv)` of the old one; both
/// closed flags of the new patch are cleared.
pub fn close_patch(m: &mut Mem, o: &mut dyn Outside, patch: u32) -> u32 {
    let new = alloc_patch(m, o, 0x086f_20a0);
    m.copy(new, patch, PATCH_SIZE);
    let grow = |m: &mut Mem, kind_at: u32, count_at: u32, closed_at: u32| {
        if m.u16(patch + closed_at) != 0 {
            let n = m.u16(patch + count_at);
            let add: u16 = match m.i32(patch + kind_at) {
                0 => 1,
                1 => {
                    let d = (((n as u32).wrapping_mul(0xaaab) >> 16) as u16) >> 1;
                    if n == d.wrapping_add(d).wrapping_add(d) { 1 } else { 0 }
                }
                2 | 3 => 3,
                _ => 0,
            };
            if add != 0 {
                m.set_u16(new + count_at, m.u16(new + count_at).wrapping_add(add));
            }
        }
    };
    // v first (the count at +0xe, kind at +8, flag at +0x12), then u (+0xc, +4, +0x10)
    grow(m, 8, 0xe, 0x12);
    grow(m, 4, 0xc, 0x10);
    let (nu, nv) = (m.u16(new + 0xc), m.u16(new + 0xe));
    let pts = alloc_points(m, o, nu, nv, 0x086f_20e0);
    m.set_u32(new + 0x1a4, pts);
    let (ou, ov) = (m.u16(patch + 0xc) as i32, m.u16(patch + 0xe) as i32);
    let src = m.u32(patch + 0x1a4);
    let mut at = pts;
    for i in 0..nu as i32 {
        for j in 0..nv as i32 {
            let from = src.wrapping_add((((i % ou) * ov + (j % ov)) as u32) << 4);
            m.copy(at, from, 16);
            at = at.wrapping_add(16);
        }
    }
    // the new patch is no longer closed (the disassembly of the function ends before these two
    // stores; they follow its last instruction)
    m.set_u16(new + 0x10, 0);
    m.set_u16(new + 0x12, 0);
    new
}

/// A patch converted along u to Bezier form (`convert_bez_u`) [soft 0x0858a10c convert_bez_u].
///
/// `mat` is the 4 by 4 basis matrix (rows of four `float`s). For every window of four rows
/// of control points (one more each time, `nu - 3` windows) the first three rows of `mat`
/// (the last window all four) give the new Bezier rows; the new patch has `3*nu - 8` rows
/// and basis 1.
pub fn convert_bez_u(m: &mut Mem, o: &mut dyn Outside, patch: u32, mat: u32) -> u32 {
    let new = alloc_patch(m, o, 0x086f_21a0);
    m.copy(new, patch, PATCH_SIZE);
    let nu = m.u16(patch + 0xc);
    let nnu = (nu as u32).wrapping_mul(3).wrapping_sub(9).wrapping_add(1) as u16;
    m.set_u16(new + 0xc, nnu);
    let nv = m.u16(new + 0xe);
    let pts = alloc_points(m, o, nnu, nv, 0x086f_21e0);
    m.set_u32(new + 0x1a4, pts);
    let ov = m.u16(patch + 0xe) as u32;
    let mut v = 0u32;
    while v < ov {
        let mut src = v.wrapping_mul(16).wrapping_add(m.u32(patch + 0x1a4));
        let mut dst = v.wrapping_mul(16).wrapping_add(m.u32(new + 0x1a4));
        let mut c = (m.u16(patch + 0xc) as i32).wrapping_sub(4);
        while c >= 0 {
            let stride = (m.u16(patch + 0xe) as u32) << 4;
            let rows = [src, src.wrapping_add(stride), src.wrapping_add(stride * 2), src.wrapping_add(stride * 3)];
            let count = if c == 0 { 4 } else { 3 };
            c -= 1;
            for k in 0..count {
                let row = mat.wrapping_add(16 * k);
                for comp in 0..3u32 {
                    let r = dot4(m, row, rows, comp * 4);
                    m.set_f32(dst + comp * 4, r);
                }
                dst = dst.wrapping_add((m.u16(new + 0xe) as u32) << 4);
            }
            src = src.wrapping_add((m.u16(patch + 0xe) as u32) << 4);
        }
        v += 1;
    }
    m.set_i32(new + 4, 1);
    new
}

/// A patch converted along v to Bezier form (`convert_bez_v`) [soft 0x08589f60 convert_bez_v]:
/// as [`convert_bez_u`] with the roles of the two directions exchanged (windows of four
/// neighbouring points along each row; the new patch has `3*nv - 8` columns and v basis 1).
pub fn convert_bez_v(m: &mut Mem, o: &mut dyn Outside, patch: u32, mat: u32) -> u32 {
    let new = alloc_patch(m, o, 0x086f_2120);
    m.copy(new, patch, PATCH_SIZE);
    let nv = m.u16(patch + 0xe);
    let nnv = (nv as u32).wrapping_mul(3).wrapping_sub(9).wrapping_add(1) as u16;
    m.set_u16(new + 0xe, nnv);
    let nu_new = m.u16(new + 0xc);
    let pts = alloc_points(m, o, nu_new, nnv, 0x086f_2160);
    m.set_u32(new + 0x1a4, pts);
    let mut iu = 0u32;
    while iu < m.u16(patch + 0xc) as u32 {
        let onv = m.u16(patch + 0xe) as u32;
        let mut src = (iu.wrapping_mul(onv) << 4).wrapping_add(m.u32(patch + 0x1a4));
        let mut dst = ((m.u16(new + 0xe) as u32).wrapping_mul(iu) << 4).wrapping_add(m.u32(new + 0x1a4));
        let mut c = (onv as i32).wrapping_sub(4);
        iu += 1;
        while c >= 0 {
            let count = if c == 0 { 4 } else { 3 };
            c -= 1;
            let next = src.wrapping_add(16);
            let rows = [src, src.wrapping_add(16), src.wrapping_add(32), src.wrapping_add(48)];
            for k in 0..count {
                let row = mat.wrapping_add(16 * k);
                for comp in 0..3u32 {
                    let r = dot4(m, row, rows, comp * 4);
                    m.set_f32(dst + comp * 4, r);
                }
                dst = dst.wrapping_add(16);
            }
            src = next;
        }
    }
    m.set_i32(new + 8, 1);
    new
}

/// The message of `convert_to_bezier`'s check: "patch has too few points" (`.rodata`).
const TOO_FEW: u32 = 0x086f_2080;

/// A patch converted to Bezier form in both directions (`convert_to_bezier`)
/// [soft 0x08589bc0 convert_to_bezier]; returns the new patch.
///
/// A patch closed in either direction is closed first (`close_patch`). A patch with a count
/// of one or no more than three (Bezier-like kind 0 needs two, any other four) points
/// reports "patch has too few points" through `sfatal` (a hook here, the original carries
/// on). Kind 2 (B-spline) uses `bspline_mat`; kind 3 (cardinal) sets four entries of
/// `cardinal_mat` from the tension (`t / 3`, the entries at +0x10 and +0x2c negated) and uses
/// it; other kinds are left. The patches made in between are freed.
pub fn convert_to_bezier(m: &mut Mem, o: &mut dyn Outside, patch: u32) -> u32 {
    let (mut cur, mut closed, mut mid) = (patch, 0u32, 0u32);
    if m.u32(cur + 0x10) != 0 {
        cur = close_patch(m, o, cur);
        closed = cur;
    }
    let nu = m.u16(cur + 0xc) as i32;
    let short_u = if m.i32(cur + 4) == 0 { nu <= 1 } else { nu <= 3 };
    let bad = short_u || {
        let nv = m.u16(cur + 0xe) as i32;
        if m.i32(cur + 8) != 0 { nv <= 3 } else { nv <= 1 }
    };
    if bad {
        o.call(m, SFATAL, &[TOO_FEW]);
    }
    for (kind_at, tension_at, u_dir) in [(4u32, 0x14u32, true), (8, 0x18, false)] {
        let mat = match m.i32(cur + kind_at) {
            2 => BSPLINE_MAT,
            3 => {
                // t = tension / 3.0 (the division in 80 bits, stored as float)
                let t = (F80::from_f32(m.f32(cur + tension_at)) / F80::from_f64(3.0)).to_f32();
                m.set_f32(CARDINAL_MAT + 0x24, t);
                m.set_f32(CARDINAL_MAT + 0x18, t);
                m.set_f32(CARDINAL_MAT + 0x2c, -t);
                m.set_f32(CARDINAL_MAT + 0x10, -t);
                CARDINAL_MAT
            }
            _ => continue,
        };
        cur = if u_dir { convert_bez_u(m, o, cur, mat) } else { convert_bez_v(m, o, cur, mat) };
        if u_dir {
            mid = cur;
        }
    }
    // free the patches made on the way, unless they are the result
    for made in [mid, closed] {
        if made != 0 && cur != made {
            let pts = m.u32(made + 0x1a4);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, pts]);
            m.set_u32(made + 0x1a4, 0);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, made]);
        }
    }
    cur
}

/// More callees and data of `print_patch`, by address.
pub const SDEBUG: Routine = Routine::new("sdebug", 0x0859_8cdc);
pub const SWARNING: Routine = Routine::new("swarning", 0x0859_8c44);
pub const MOTION_VECTOR: Routine = Routine::new("motion_vector", 0x0859_0848);
pub const VECTOR_X_MATRIX: Routine = Routine::new("vector_X_matrix", 0x0859_8430);
pub const MODEL_COMPUTE_BOUNDING: Routine = Routine::new("SI_modelComputeBounding", 0x0846_7e70);
pub const MEMTRACK_REALLOC: Routine = Routine::new("SI_memtrackRealloc", 0x0859_9770);
pub const EMPTY_STRING: Routine = Routine::new("SI_emptyString", 0x082c_b774);
pub const MAT_NAME_EXTENSION: Routine = Routine::new("si_getMatNameExtension", 0x0859_5024);

/// `si_Gbl_vert`, `mb_nb_max_vert` and the table of type names.
pub const SI_GBL_VERT: u32 = 0x086e_f9c8;
pub const MB_NB_MAX_VERT: u32 = 0x0882_9f10;
pub const TYPE_NAME: u32 = 0x086f_17c0;
/// The first entries of the type-name pointer table (static data of the program image).
const TYPE_NAME_INIT: [u32; 9] = [0x086f_1780, 0x086f_1787, 0x086f_178e, 0x086f_1796, 0x086f_179f, 0x086f_17a1, 0x086f_17a3, 0x086f_17a5, 0x086f_17a7];

fn type_name(m: &Mem, kind: u32) -> u32 {
    TYPE_NAME_INIT.get(kind as usize).copied().unwrap_or_else(|| m.u32(TYPE_NAME.wrapping_add(kind.wrapping_mul(4))))
}

/// The texts of `print_patch` and its wrapper.
pub mod text4 {
    pub const WARN_POINTS: u32 = 0x086f_1c20;
    pub const FATAL_ALLOC: u32 = 0x086f_1c60;
    pub const NL: u32 = 0x086f_1c86;
    pub const TAB: u32 = 0x086f_1c88;
    pub const NL_TABS2: u32 = 0x086f_1c8a;
    pub const V_D: u32 = 0x086f_1c8e;
    pub const M_D_A: u32 = 0x086f_1c95;
    pub const M_D_B: u32 = 0x086f_1c9b;
    pub const EMPTY_A: u32 = 0x086f_1ca1;
    pub const DASH_A: u32 = 0x086f_1ca2;
    pub const EMPTY_B: u32 = 0x086f_1ca4;
    pub const DEFAULT_A: u32 = 0x086f_1ca5;
    pub const DASH_B: u32 = 0x086f_1cad;
    pub const EMPTY_C: u32 = 0x086f_1caf;
    pub const EMPTY_D: u32 = 0x086f_1cb0;
    pub const SURFACE_A: u32 = 0x086f_1cc0;
    pub const DEFAULT_B: u32 = 0x086f_1ce4;
    pub const DASH_C: u32 = 0x086f_1cec;
    pub const EMPTY_E: u32 = 0x086f_1cee;
    pub const EMPTY_F: u32 = 0x086f_1cef;
    pub const SURFACE_B: u32 = 0x086f_1cf0;
    pub const BEZIER_U: u32 = 0x086f_1d0d;
    pub const TAB_U: u32 = 0x086f_1d24;
    pub const NL_TABS3_U: u32 = 0x086f_1d26;
    pub const KNOT_U: u32 = 0x086f_1d2b;
    pub const NL_U: u32 = 0x086f_1d33;
    pub const BEZIER_V: u32 = 0x086f_1d35;
    pub const TAB_V: u32 = 0x086f_1d4c;
    pub const NL_TABS3_V: u32 = 0x086f_1d4e;
    pub const KNOT_V: u32 = 0x086f_1d53;
    pub const NL_V: u32 = 0x086f_1d5b;
    pub const SPACE_I: u32 = 0x086f_1d5d;
    pub const NL_TABS3_I: u32 = 0x086f_1d5f;
    pub const IDX_D: u32 = 0x086f_1d64;
    pub const NL_END: u32 = 0x086f_1d69;
    pub const TEXTURE: u32 = 0x086f_1d6b;
    pub const BEZ1_A: u32 = 0x086f_1d76;
    pub const BEZ1_B: u32 = 0x086f_1d89;
    pub const QUAD_D: u32 = 0x086f_1d9d;
    pub const APPROX_PARAM: u32 = 0x086f_1dc0;
    pub const VIEW_A: u32 = 0x086f_1df0;
    pub const NO_VIEW_A: u32 = 0x086f_1df6;
    pub const APPROX_SPATIAL: u32 = 0x086f_1e00;
    pub const SIZES_A: u32 = 0x086f_1e24;
    pub const END_A: u32 = 0x086f_1e2b;
    pub const VIEW_B: u32 = 0x086f_1e34;
    pub const NO_VIEW_B: u32 = 0x086f_1e3a;
    pub const APPROX_CURV: u32 = 0x086f_1e40;
    pub const SIZES_B: u32 = 0x086f_1e69;
    pub const END_B: u32 = 0x086f_1e70;
    pub const APPROX_TRIM: u32 = 0x086f_1e80;
    pub const DEBUG: u32 = 0x086f_1be0;
    pub const BASIS3: u32 = 0x086f_1b75;
    pub const BASIS1: u32 = 0x086f_1b8f;
    pub const GROUP: u32 = 0x086f_1ba9;
    pub const END_GROUP: u32 = 0x086f_1bb9;
}

/// A float through the 80-bit register and back (a signalling NaN comes out quiet).
fn quiet(bits: u32) -> u32 {
    via_x87(bits)
}

/// A patch written as mental ray scene text (`print_patch`)
/// [soft 0x0858796c print_patch].
///
/// `patch` is the patch record (0x204 bytes), `mat` the matrix its points are taken through.
/// `no_out` set writes nothing (the vertices are only stored when `motion` is set), `motion`
/// stores the transformed vertices in the global vertex array and in the model's motion record
/// (+0x33c). A patch whose point counts are not whole Bezier spans loses the odd points (a
/// warning), is converted to Bezier form and written as vertices, the two knot lists, the
/// index grid, the texture references, the material's approximation line and the trim
/// approximation. A mirrored model gets an inverted copy of the patch.
pub fn print_patch(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, patch: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let mut motion = motion;
    let mut inst = false;
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    // mirrored: the product of the model's scale against the handedness, flipped by bit 3
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let mut patch = patch;
    let mut saved = 0;
    if flip {
        saved = patch;
        patch = o.call(m, PATCH_COPY, &[patch]).eax;
        o.call(m, PATCH_INVERSE, &[patch, 0]);
    }
    let (t1, t2) = (type_name(m, m.u32(patch + 4)), type_name(m, m.u32(patch + 8)));
    let sizes = [m.u16(patch + 0xc) as u32, m.u16(patch + 0xe) as u32, m.u16(patch + 0x1c) as u32, m.u16(patch + 0x1e) as u32];
    o.call(m, SDEBUG, &[text4::DEBUG, t1, t2, sizes[0], sizes[1], sizes[2], sizes[3]]);
    // locals: the copy of the patch, the transformed centre (also the motion vector), the centre, one vertex in and out
    const L: u32 = 0x204 + 4 * 12 + 8;
    let l = m.local(L);
    let (pc, cen_out, cen_in, v_in, v_out, outp, argslot) = (l, l + 0x204, l + 0x210, l + 0x21c, l + 0x228, l + 0x234, l + 0x238);
    o.call(m, MODEL_COMPUTE_BOUNDING, &[def]);
    let half = F80::from_f32(0.5);
    for (k, (a, b)) in [(0x1c, 0x10), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let s = F80::from_f32(f32::from_bits(m.u32(def + a))) + F80::from_f32(f32::from_bits(m.u32(def + b)));
        m.set_u32(cen_in + 4 * k as u32, (s * half).to_f32().to_bits());
    }
    o.call(m, VECTOR_X_MATRIX, &[cen_out, cen_in, mat]);
    m.copy(pc, patch, 0x204);
    let (kind_u, kind_v) = (m.u32(pc + 4), m.u32(pc + 8));
    let (nu, nv) = (m.u16(pc + 0xc) as i32, m.u16(pc + 0xe) as i32);
    let rem_u = if kind_u == 1 && m.u16(pc + 0x10) == 0 { (nu + 2) % 3 } else { 0 };
    let rem_v = if kind_v == 1 && m.u16(pc + 0x12) == 0 { (nv + 2) % 3 } else { 0 };
    let bad = rem_u != 0 || rem_v != 0;
    if bad {
        o.call(m, SWARNING, &[text4::WARN_POINTS, nu as u32, nv as u32, rem_u as u32, rem_v as u32]);
        m.set_u16(pc + 0xc, (nu - rem_u) as u16);
        m.set_u16(pc + 0xe, (nv - rem_v) as u16);
        let (nu2, nv2) = (m.u16(pc + 0xc) as u32, m.u16(pc + 0xe) as u32);
        let tmp = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, nu2.wrapping_mul(nv2) << 4]).eax;
        if tmp == 0 {
            o.call(m, SFATAL, &[text4::FATAL_ALLOC]);
        }
        m.set_u32(pc + 0x1a4, tmp);
        if nu2 != 0 {
            for i in 0..nu2 {
                for j in 0..nv2 {
                    let dst = tmp.wrapping_add(j.wrapping_add(i.wrapping_mul(nv2)) << 4);
                    let src_pts = m.u32(patch + 0x1a4);
                    let src = src_pts.wrapping_add((m.u16(patch + 0xe) as u32).wrapping_mul(i).wrapping_add(j) << 4);
                    m.copy(dst, src, 16);
                }
            }
        }
    }
    let q = convert_to_bezier(m, o, pc);
    let (qnu, qnv) = (m.u16(q + 0xc) as u32, m.u16(q + 0xe) as u32);
    let npts = qnv.wrapping_mul(qnu);
    if npts > m.u32(MB_NB_MAX_VERT) {
        let hi = ((npts as u64 * 0xd1b7_1759u64) >> 32) as u32 >> 12;
        let cap = hi.wrapping_mul(0x1388).wrapping_add(0x1388);
        let p = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(SI_GBL_VERT), cap.wrapping_mul(0x4c)]).eax;
        m.set_u32(SI_GBL_VERT, p);
        m.set_u32(MB_NB_MAX_VERT, cap);
    }
    if motion != 0 {
        if m.u8(def + 0x3ee) & 1 != 0 {
            inst = true;
            motion = 0;
        }
        if m.u32(def + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(def + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        if motion != 0 {
            let rec = m.u32(def + 0x33c);
            if rec != 0 {
                let new = if m.u32(rec + 0x22c) == 0 {
                    Some(o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, npts.wrapping_mul(0x4c)]).eax)
                } else if m.u32(rec + 0x228) < npts {
                    Some(o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), npts.wrapping_mul(0x4c)]).eax)
                } else {
                    None
                };
                if let Some(new) = new {
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x22c, new);
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x228, npts);
                }
            }
        }
    }
    m.set_i32(g::NVERT, 0);
    if no_out == 0 || motion != 0 {
        let mut j = qnv as i32 - 1;
        while j >= 0 {
            let next = j - 1;
            if qnu != 0 {
                let mut i: u32 = 0;
                loop {
                    let pts = m.u32(q + 0x1a4);
                    let src = pts.wrapping_add((qnv.wrapping_mul(i)) << 4).wrapping_add((j as u32) << 4);
                    for k in 0..3 {
                        m.set_u32(v_in + 4 * k, quiet(m.u32(src + 4 * k)));
                    }
                    o.call(m, VECTOR_X_MATRIX, &[v_out, v_in, mat]);
                    if motion != 0 {
                        let gv = m.u32(SI_GBL_VERT).wrapping_add((m.i32(g::NVERT) as u32).wrapping_mul(0x4c));
                        for k in 0..3 {
                            m.set_u32(gv + 8 + 4 * k, quiet(m.u32(v_out + 4 * k)));
                        }
                    }
                    if !(motion != 0 && no_out != 0) {
                        let (x, y, z) = (quiet(m.u32(v_out)), quiet(m.u32(v_out + 4)), quiet(m.u32(v_out + 8)));
                        cvec(m, o, x, y, z);
                    }
                    m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
                    i += 1;
                    if i >= qnu {
                        break;
                    }
                }
            }
            j = next;
        }
    }
    let cnt = m.i32(g::NVERT) as u32;
    if no_out == 0 {
        fprintf(m, o, &[text4::NL]);
        print_patch_tcoord(m, o, dict, elem);
        let mut moves: i32 = 0;
        if motion != 0 {
            for b in 0..cnt {
                let r = o.call(m, MOTION_VECTOR, &[def, cen_out, b]).eax;
                moves = moves.wrapping_add(r as i32);
            }
        }
        if inst {
            moves = o.call(m, MOTION_VECTOR, &[def, cen_out, 0]).eax as i32;
        }
        let cid_mat = m.u32(g::CID_MATERIALS);
        let matel = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, outp, 0]).eax;
        if matel != 0 && m.u16(matel + 0x1e) == 7 {
            let c = quiet(0x4333_0000);
            cvec(m, o, c, c, c);
            m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
        }
        let mut b: i32 = 0;
        while b < m.i32(g::NVERT) {
            let sep = if b & 3 == 0 { text4::NL_TABS2 } else { text4::TAB };
            fprintf(m, o, &[text4::V_D, sep, b as u32]);
            if moves != 0 && (b as u32) < cnt {
                if motion != 0 {
                    fprintf(m, o, &[text4::M_D_A, (b as u32).wrapping_add(m.u32(g::NVERT))]);
                } else if inst {
                    fprintf(m, o, &[text4::M_D_B, m.u32(g::NVERT)]);
                }
            }
            b += 1;
        }
        let mut name = 0u32;
        let rec = m.u32(outp);
        if rec != 0 {
            let r2 = m.u32(rec);
            name = m.u32(r2 + 0x10);
        }
        if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 {
            name = 0;
        }
        let ext = o.call(m, MAT_NAME_EXTENSION, &[dict, elem]).eax;
        let with_name = matel != 0 && name != 0;
        if ext != 0 {
            let a5 = if owner_name != 0 { text4::DASH_A } else { text4::EMPTY_A };
            let a4 = if owner_name != 0 { owner_name } else { text4::EMPTY_B };
            let a3 = if matel != 0 { matel } else { text4::DEFAULT_A };
            let a2 = if with_name { text4::DASH_B } else { text4::EMPTY_C };
            let a1 = if with_name { name } else { text4::EMPTY_D };
            fprintf(m, o, &[text4::SURFACE_A, a1, a2, a3, a4, a5, ext]);
        } else {
            let b3 = if matel != 0 { matel } else { text4::DEFAULT_B };
            let b2 = if with_name { text4::DASH_C } else { text4::EMPTY_E };
            let b1 = if with_name { name } else { text4::EMPTY_F };
            fprintf(m, o, &[text4::SURFACE_B, b1, b2, b3]);
        }
        if ext != 0 {
            m.free(ext);
        }
        // the two knot lists
        let (ku, kv) = (m.u32(q + 4), m.u32(q + 8));
        let ucnt: i32 = if ku == 0 { qnu as i32 } else { (qnu as i32 + 2) / 3 };
        fprintf(m, o, &[text4::BEZIER_U, if ku == 0 { 1 } else { 3 }, (ucnt - 1) as u32]);
        for k in 0..ucnt {
            let sep = if k == (k / 6) * 6 { text4::NL_TABS3_U } else { text4::TAB_U };
            fprintf(m, o, &[text4::KNOT_U, sep, k as u32]);
        }
        fprintf(m, o, &[text4::NL_U]);
        let vcnt: i32 = if kv == 0 { qnv as i32 } else { (qnv as i32 + 2) / 3 };
        fprintf(m, o, &[text4::BEZIER_V, if kv == 0 { 1 } else { 3 }, (vcnt - 1) as u32]);
        for k in 0..vcnt {
            let sep = if k == (k / 6) * 6 { text4::NL_TABS3_V } else { text4::TAB_V };
            fprintf(m, o, &[text4::KNOT_V, sep, k as u32]);
        }
        fprintf(m, o, &[text4::NL_V]);
        // the index grid, last row and column first
        let mut n: i32 = 0;
        let mut k = qnv as i32 - 1;
        while k >= 0 {
            let next = k - 1;
            let mut e = qnu as i32 - 1;
            while e >= 0 {
                let idx = (qnu as i32).wrapping_mul(k).wrapping_add(e);
                let sep = if n == (n / 12) * 12 { text4::NL_TABS3_I } else { text4::SPACE_I };
                fprintf(m, o, &[text4::IDX_D, sep, idx as u32]);
                n += 1;
                e -= 1;
            }
            k = next;
        }
        fprintf(m, o, &[text4::NL_END]);
        print_patch_tcoord_ref(m, o, dict, elem, ucnt, vcnt, (qnu as i32).wrapping_mul(qnv as i32));
        if matel != 0 && m.u16(matel + 0x1e) == 7 {
            fprintf(m, o, &[text4::TEXTURE]);
            let u = dw(ucnt as f64 - 1.0);
            fprintf(m, o, &[text4::BEZ1_A, 0, 0, u[0], u[1]]);
            let v = dw(vcnt as f64 - 1.0);
            fprintf(m, o, &[text4::BEZ1_B, 0, 0, v[0], v[1]]);
            let last = m.u32(g::NVERT).wrapping_sub(1);
            fprintf(m, o, &[text4::QUAD_D, last, last, last, last]);
        }
        // the approximation selected by the original patch
        let k3 = F80::from_f64(3.0);
        let div3 = |x: u16, third: bool| -> f64 {
            let v = F80::from_i32(x as i32);
            (if third { v / k3 } else { v }).to_f64()
        };
        let sel = m.u16(patch + 0x200);
        match sel {
            1 => {
                let a = div3(m.u16(q + 0x1e), kv != 0);
                let b = div3(m.u16(q + 0x1c), ku != 0);
                let (b, a) = (dw(b), dw(a));
                fprintf(m, o, &[text4::APPROX_PARAM, b[0], b[1], a[0], a[1]]);
            }
            2 => {
                let val = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1ec))).to_f64());
                let view = if m.u16(patch + 0x1e8) != 0 { text4::VIEW_A } else { text4::NO_VIEW_A };
                fprintf(m, o, &[text4::APPROX_SPATIAL, view, val[0], val[1]]);
                if m.u16(patch + 0x1fc) != 0 {
                    fprintf(m, o, &[text4::SIZES_A, m.u16(patch + 0x1f8) as u32, m.u16(patch + 0x1fa) as u32]);
                }
                fprintf(m, o, &[text4::END_A]);
            }
            3 => {
                let f1 = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1f0))).to_f64());
                let f2 = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1f4))).to_f64());
                let view = if m.u16(patch + 0x1e8) != 0 { text4::VIEW_B } else { text4::NO_VIEW_B };
                fprintf(m, o, &[text4::APPROX_CURV, view, f1[0], f1[1], f2[0], f2[1]]);
                if m.u16(patch + 0x1fc) != 0 {
                    fprintf(m, o, &[text4::SIZES_B, m.u16(patch + 0x1f8) as u32, m.u16(patch + 0x1fa) as u32]);
                }
                fprintf(m, o, &[text4::END_B]);
            }
            _ => {}
        }
        // the trim approximation: the larger step over (3 or 1) + (3 or 1)
        let mut den = if ku == 0 { F80::from_i32(1) } else { k3 };
        let big = m.u16(q + 0x1c).max(m.u16(q + 0x1e));
        den = if kv == 0 { den + F80::from_i32(1) } else { den + k3 };
        let t = (F80::from_i32(big as i32) / den + F80::from_f64(1e-6f64)).to_f64();
        let t = dw(t);
        fprintf(m, o, &[text4::APPROX_TRIM, t[0], t[1]]);
        if q != patch && q != pc {
            let pts = m.u32(q + 0x1a4);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, pts]);
            m.set_u32(q + 0x1a4, 0);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, q]);
        }
        if bad {
            let pts = m.u32(pc + 0x1a4);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, pts]);
            m.set_u32(pc + 0x1a4, 0);
        }
    }
    if motion != 0 && m.u32(def + 0x33c) != 0 {
        let gv = m.u32(SI_GBL_VERT);
        let mut b: u32 = 0;
        if b < cnt {
            loop {
                let rec = m.u32(def + 0x33c);
                let dst = m.u32(rec + 0x22c);
                let off = b.wrapping_mul(0x4c);
                for k in 0..3 {
                    m.set_u32(dst.wrapping_add(off).wrapping_add(8 + 4 * k), quiet(m.u32(gv.wrapping_add(off).wrapping_add(8 + 4 * k))));
                }
                b += 1;
                if b >= cnt {
                    break;
                }
            }
        }
    }
    if inst {
        let rec = m.u32(def + 0x33c);
        if rec != 0 {
            for k in 0..3 {
                let rec = m.u32(def + 0x33c);
                m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(cen_out + 4 * k)));
            }
        }
    }
    if flip {
        m.set_u32(argslot, patch);
        o.call(m, PATCH_DISPOSE, &[argslot]);
        let _ = saved;
    }
    m.release(L);
}

/// The patch of an element written as a surface (`print_patch_geometry`)
/// [soft 0x085878d0 print_patch_geometry].
///
/// The basis lines and the group open before, and the group closes after, unless `no_out` is set.
pub fn print_patch_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let patch = m.u32(def + 0x48);
    if no_out == 0 {
        fprintf(m, o, &[text4::BASIS3]);
        fprintf(m, o, &[text4::BASIS1]);
        fprintf(m, o, &[text4::GROUP]);
    }
    print_patch(m, o, dict, elem, patch, mat, no_out, motion);
    if no_out == 0 {
        fprintf(m, o, &[text4::END_GROUP]);
    }
}

/// The texts of `print_nurbs_geometry` in `.rodata`, by address.
pub mod text5 {
    pub const DEBUG: u32 = 0x086f_1800; // "nurbs: %srational, degree %d/%d, nknots %d/%d, npoints %d/%d\n"
    pub const DEBUG_ARG: u32 = 0x086f_17fe; // ""
    pub const BASIS_BEZIER1: u32 = 0x086f_183e;
    pub const RATIONAL_A: u32 = 0x086f_1858; // " rational"
    pub const BASIS_U: u32 = 0x086f_1862;
    pub const RATIONAL_B: u32 = 0x086f_187f;
    pub const BASIS_V: u32 = 0x086f_1889;
    pub const RATIONAL_C: u32 = 0x086f_18a6;
    pub const EMPTY_A: u32 = 0x086f_18b0;
    pub const BASIS_CURVE: u32 = 0x086f_18b1; // "\tbasis _curve%d%s bspline %d\n"
    pub const GROUP: u32 = 0x086f_18cf;
    pub const CURVE_HDR: u32 = 0x086f_18df; // "# curve %d\n"
    pub const TAB_A: u32 = 0x086f_18eb;
    pub const NL_TABS2_A: u32 = 0x086f_18ed;
    pub const V_D: u32 = 0x086f_18f1;
    pub const M_D_A: u32 = 0x086f_18f8;
    pub const M_D_B: u32 = 0x086f_18fe;
    pub const CURVE_NL: u32 = 0x086f_1904; // "\n# curve %d"
    pub const TAB_B: u32 = 0x086f_1910;
    pub const NL_TABS2_B: u32 = 0x086f_1912;
    pub const V_D_B: u32 = 0x086f_1916;
    pub const CURVE_DEF: u32 = 0x086f_191d;
    pub const TAB_C: u32 = 0x086f_193a;
    pub const NL_TABS3_C: u32 = 0x086f_193c;
    pub const S_D_F: u32 = 0x086f_1941;
    pub const SPACE_D: u32 = 0x086f_1949;
    pub const NL_TABS3_D: u32 = 0x086f_194b;
    pub const S_D: u32 = 0x086f_1950;
    pub const EMPTY_B: u32 = 0x086f_1955;
    pub const DASH_B: u32 = 0x086f_1956;
    pub const EMPTY_C: u32 = 0x086f_1958;
    pub const DEFAULT_A: u32 = 0x086f_1959;
    pub const DASH_C: u32 = 0x086f_1961;
    pub const EMPTY_D: u32 = 0x086f_1963;
    pub const EMPTY_E: u32 = 0x086f_1964;
    pub const SURFACE_A: u32 = 0x086f_1980;
    pub const DEFAULT_B: u32 = 0x086f_19a4;
    pub const DASH_D: u32 = 0x086f_19ac;
    pub const EMPTY_F: u32 = 0x086f_19ae;
    pub const EMPTY_G: u32 = 0x086f_19af;
    pub const SURFACE_B: u32 = 0x086f_19b0;
    pub const UNURBS: u32 = 0x086f_19cd;
    pub const VNURBS: u32 = 0x086f_19df;
    pub const TAB_E: u32 = 0x086f_19f1;
    pub const NL_TABS3_E: u32 = 0x086f_19f3;
    pub const S_D_F_B: u32 = 0x086f_19f8;
    pub const NL: u32 = 0x086f_1a0c;
    pub const TEXTURE: u32 = 0x086f_1a0e;
    pub const BEZ1_A: u32 = 0x086f_1a19;
    pub const BEZ1_B: u32 = 0x086f_1a2c;
    pub const QUAD_D: u32 = 0x086f_1a40;
    pub const TRIM: u32 = 0x086f_1a50;
    pub const HOLE: u32 = 0x086f_1a55;
    pub const TRIM_HDR: u32 = 0x086f_1a5a;
    pub const PRE_CONT: u32 = 0x086f_1a60;
    pub const PRE_FIRST: u32 = 0x086f_1a63;
    pub const TRIM_CURVE: u32 = 0x086f_1a64;
    pub const NL_B: u32 = 0x086f_1a76;
    pub const APPROX_PARAM: u32 = 0x086f_1a80;
    pub const VIEW_A: u32 = 0x086f_1ab0;
    pub const NO_VIEW_A: u32 = 0x086f_1ab6;
    pub const APPROX_SPATIAL: u32 = 0x086f_1ac0;
    pub const SIZES_A: u32 = 0x086f_1ae4;
    pub const END_A: u32 = 0x086f_1aeb;
    pub const VIEW_B: u32 = 0x086f_1af4;
    pub const NO_VIEW_B: u32 = 0x086f_1afa;
    pub const APPROX_CURV: u32 = 0x086f_1b00;
    pub const SIZES_B: u32 = 0x086f_1b29;
    pub const END_B: u32 = 0x086f_1b30;
    pub const APPROX_TRIM: u32 = 0x086f_1b40;
    pub const END_GROUP: u32 = 0x086f_1b69;
}

/// A `double` stored to the stack as a `float` through the 80-bit register.
fn f64_to_f32_bits(v: f64) -> u32 {
    F80::from_f64(v).to_f32().to_bits()
}

/// The NURBS surface of an element written as mental ray scene text
/// [soft 0x085867d0 print_nurbs_geometry].
///
/// `srf` is the model's surface (`SI_NurbsSrf`, at definition +0x48): `u` direction at +0, `v` at
/// +0x24 (order, point counts, knots, range, closed flag), the control points (0x24 bytes each:
/// x, y, z, w as doubles) at +0x54, the trim groups at +0x60/+0x64, the mental ray approximation at
/// +0x98. A mirrored model (scale product against the handedness and bit 3 of the flags) gets
/// an inverted copy. The basis lines (one per direction and trim curve) and the group are
/// written unless `no_out`; the surface's vertices (divided by w, taken through `mat`) are
/// written as compressed vectors and stored in the global vertex array and the model's motion
/// record when `motion` is set, followed by the texture vertices, the trim curves' vertices
/// and the `v`/`m` lines, the curve definitions, the surface with its knot vectors and
/// weighted control-point indices, the texture references, the trim groups and the
/// approximation lines.
pub fn print_nurbs_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let mut motion = motion;
    let mut inst = false;
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let mut saved = 0;
    if flip {
        saved = m.u32(def + 0x48);
        let c = o.call(m, SRF_COPY, &[saved]).eax;
        m.set_u32(def + 0x48, c);
        o.call(m, SRF_INVERSE, &[m.u32(def + 0x48), 1, 0]);
    }
    let s = m.u32(def + 0x48);
    let (ou, ov) = (m.i32(s), m.i32(s + 0x24));
    let (nu, nv) = (m.i32(s + 4), m.i32(s + 0x28));
    let ntrim = m.i32(s + 0x60);
    o.call(
        m,
        SDEBUG,
        &[text5::DEBUG, text5::DEBUG_ARG, ou.wrapping_sub(1) as u32, ov.wrapping_sub(1) as u32, m.u32(s + 8), m.u32(s + 0x2c), nu as u32, nv as u32],
    );
    let groups = m.u32(s + 0x64);
    let group = |m: &Mem, g: i32| m.u32(groups.wrapping_add((g as u32).wrapping_mul(4)));
    let curve = |m: &Mem, gr: u32, c: i32| m.u32(m.u32(gr + 8).wrapping_add((c as u32).wrapping_mul(4)));
    if no_out == 0 {
        fprintf(m, o, &[text5::BASIS_BEZIER1]);
        fprintf(m, o, &[text5::BASIS_U, text5::RATIONAL_A, ou.wrapping_sub(1) as u32]);
        fprintf(m, o, &[text5::BASIS_V, text5::RATIONAL_B, ov.wrapping_sub(1) as u32]);
        let mut idx: i32 = 0;
        let mut g = 0;
        while g < ntrim {
            let gr = group(m, g);
            g += 1;
            let mut c = 0;
            while c < m.i32(gr + 4) {
                let cr = curve(m, gr, c);
                let rat = if m.i16(cr + 0x28) != 0 { text5::RATIONAL_C } else { text5::EMPTY_A };
                fprintf(m, o, &[text5::BASIS_CURVE, idx as u32, rat, m.i32(cr).wrapping_sub(1) as u32]);
                idx += 1;
                c += 1;
            }
        }
    }
    let mut motion_w = motion;
    // locals: the centre, the transformed centre (also the motion vector), one vertex in and out, the related element
    const L: u32 = 12 * 4 + 4;
    let l = m.local(L);
    let (cen_in, cen_out, v_in, v_out, outp) = (l, l + 12, l + 24, l + 36, l + 48);
    o.call(m, MODEL_COMPUTE_BOUNDING, &[def]);
    let half = F80::from_f32(0.5);
    for (k, (a, b)) in [(0x1c, 0x10), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let sum = F80::from_f32(f32::from_bits(m.u32(def + a))) + F80::from_f32(f32::from_bits(m.u32(def + b)));
        m.set_u32(cen_in + 4 * k as u32, (sum * half).to_f32().to_bits());
    }
    o.call(m, VECTOR_X_MATRIX, &[cen_out, cen_in, mat]);
    let npts = (m.u32(s + 0x28)).wrapping_mul(m.u32(s + 4));
    if m.u32(MB_NB_MAX_VERT) <= npts && npts.wrapping_sub(m.u32(MB_NB_MAX_VERT)) != 0 {
        let hi = ((npts as u64 * 0xd1b7_1759u64) >> 32) as u32 >> 12;
        let cap = hi.wrapping_mul(5000).wrapping_add(5000);
        let p = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(SI_GBL_VERT), cap.wrapping_mul(0x4c)]).eax;
        m.set_u32(SI_GBL_VERT, p);
        m.set_u32(MB_NB_MAX_VERT, cap);
    }
    if motion != 0 {
        if m.u8(def + 0x3ee) & 1 != 0 {
            inst = true;
            motion_w = 0;
        }
        if m.u32(def + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(def + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        if motion_w != 0 {
            let rec = m.u32(def + 0x33c);
            if rec != 0 {
                let new = if m.u32(rec + 0x22c) == 0 {
                    Some(o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, npts.wrapping_mul(0x4c)]).eax)
                } else if m.u32(rec + 0x228) < npts {
                    Some(o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), npts.wrapping_mul(0x4c)]).eax)
                } else {
                    None
                };
                if let Some(new) = new {
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x22c, new);
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x228, npts);
                }
            }
        }
    }
    motion = motion_w;
    if no_out == 0 {
        fprintf(m, o, &[text5::GROUP]);
    }
    m.set_i32(g::NVERT, 0);
    if no_out == 0 || motion != 0 {
        let mut j = m.i32(s + 0x28) - 1;
        while j >= 0 {
            let n = m.i32(s + 4);
            let mut k = 0;
            while k < n {
                // the points run backwards along each row of the grid
                let idx = (j.wrapping_mul(n)).wrapping_add(n).wrapping_sub(k + 1);
                let p = m.u32(s + 0x54).wrapping_add((idx as u32).wrapping_mul(0x24));
                let w = F80::from_f64(m.f64(p + 0x18));
                let w = if w.eq(F80::from_i32(0)) { F80::from_i32(1) } else { w };
                let inv = F80::from_i32(1) / w;
                let x = (F80::from_f64(m.f64(p)) * inv).to_f32().to_bits();
                let y = (F80::from_f64(m.f64(p + 8)) * inv).to_f32().to_bits();
                let z = (inv * F80::from_f64(m.f64(p + 0x10))).to_f32().to_bits();
                m.set_u32(v_in, x);
                m.set_u32(v_in + 4, y);
                m.set_u32(v_in + 8, z);
                o.call(m, VECTOR_X_MATRIX, &[v_out, v_in, mat]);
                if motion != 0 {
                    let gv = m.u32(SI_GBL_VERT).wrapping_add((m.i32(g::NVERT) as u32).wrapping_mul(0x4c));
                    for q in 0..3 {
                        m.set_u32(gv + 8 + 4 * q, quiet(m.u32(v_out + 4 * q)));
                    }
                }
                if !(motion != 0 && no_out != 0) {
                    let (x, y, z) = (quiet(m.u32(v_out)), quiet(m.u32(v_out + 4)), quiet(m.u32(v_out + 8)));
                    cvec(m, o, x, y, z);
                }
                m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
                k += 1;
            }
            j -= 1;
        }
    }
    let cnt = m.i32(g::NVERT) as u32;
    if no_out == 0 {
        let tcn = print_nurb_tcoord(m, o, dict, elem);
        let mut moves: i32 = 0;
        if motion != 0 {
            for b in 0..cnt {
                let r = o.call(m, MOTION_VECTOR, &[def, cen_out, b]).eax;
                moves = moves.wrapping_add(r as i32);
            }
        }
        if inst {
            moves = o.call(m, MOTION_VECTOR, &[def, cen_out, 0]).eax as i32;
        }
        let cid_mat = m.u32(g::CID_MATERIALS);
        let matel = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, outp, 0]).eax;
        let textured = matel != 0 && m.u16(matel + 0x1e) == 7;
        if textured {
            let c = quiet(0x4333_0000);
            cvec(m, o, c, c, c);
            m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
        }
        // the trim curves' vertices (x, y, 0)
        for gi in 0..ntrim {
            fprintf(m, o, &[text5::CURVE_HDR, gi as u32]);
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                let pts = m.u32(cr + 0x2c);
                for pi in 0..m.i32(cr + 4) {
                    let p = pts.wrapping_add((pi as u32).wrapping_mul(0x24));
                    let (x, y) = (f64_to_f32_bits(m.f64(p)), f64_to_f32_bits(m.f64(p + 8)));
                    cvec(m, o, x, y, 0);
                }
            }
        }
        let mut b: i32 = 0;
        while b < m.i32(g::NVERT) {
            let sep = if b & 3 != 0 { text5::TAB_A } else { text5::NL_TABS2_A };
            fprintf(m, o, &[text5::V_D, sep, b as u32]);
            if moves != 0 && (b as u32) < cnt {
                if motion != 0 {
                    fprintf(m, o, &[text5::M_D_A, (b as u32).wrapping_add(m.u32(g::NVERT))]);
                } else if inst {
                    fprintf(m, o, &[text5::M_D_B, m.u32(g::NVERT)]);
                }
            }
            b += 1;
        }
        let mut ix = cnt.wrapping_add(moves as u32).wrapping_add(tcn as u32);
        if textured {
            ix = ix.wrapping_add(1);
        }
        for gi in 0..ntrim {
            fprintf(m, o, &[text5::CURVE_NL, gi as u32]);
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                for pi in 0..m.i32(cr + 4) {
                    let sep = if pi & 7 != 0 { text5::TAB_B } else { text5::NL_TABS2_B };
                    fprintf(m, o, &[text5::V_D_B, sep, ix]);
                    ix = ix.wrapping_add(1);
                }
            }
        }
        // the curve definitions
        let mut base_ix: i32 = 0;
        let mut cn: i32 = 0;
        for gi in 0..ntrim {
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                fprintf(m, o, &[text5::CURVE_DEF, cn as u32, cn as u32]);
                print_nurbs_curve(m, o, cr);
                let closed = m.i16(cr + 0x20) != 0;
                let n = m.i32(cr + 4);
                let mut t = if closed { 2i32.wrapping_sub(m.i32(cr)) } else { 0 };
                let end = if closed { n.wrapping_add(1) } else { n };
                cn += 1;
                let mut cntr: i32 = 0;
                while t < end {
                    let r = (t.wrapping_add(n)) % n;
                    let vi = (m.i32(g::NVERT)).wrapping_add(base_ix).wrapping_add(r);
                    if m.i16(cr + 0x28) == 0 {
                        let sep = if cntr != (cntr / 12) * 12 { text5::SPACE_D } else { text5::NL_TABS3_D };
                        fprintf(m, o, &[text5::S_D, sep, vi as u32]);
                    } else {
                        let w = dw(m.f64(m.u32(cr + 0x2c).wrapping_add(0x18).wrapping_add((r as u32).wrapping_mul(0x24))));
                        let sep = if cntr & 3 != 0 { text5::TAB_C } else { text5::NL_TABS3_C };
                        fprintf(m, o, &[text5::S_D_F, sep, vi as u32, w[0], w[1]]);
                    }
                    cntr += 1;
                    t += 1;
                }
                base_ix = base_ix.wrapping_add(n);
            }
        }
        // the owner name of the related element, the material name
        let mut name = 0u32;
        let rec = m.u32(outp);
        if rec != 0 {
            let r2 = m.u32(rec);
            name = m.u32(r2 + 0x10);
        }
        if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 {
            name = 0;
        }
        let ext = o.call(m, MAT_NAME_EXTENSION, &[dict, elem]).eax;
        let with_name = matel != 0 && name != 0;
        if ext == 0 {
            let a3 = if matel != 0 { matel } else { text5::DEFAULT_B };
            let a2 = if with_name { text5::DASH_D } else { text5::EMPTY_F };
            let a1 = if with_name { name } else { text5::EMPTY_G };
            fprintf(m, o, &[text5::SURFACE_B, a1, a2, a3]);
        } else {
            let a5 = if owner_name != 0 { text5::DASH_B } else { text5::EMPTY_B };
            let a4 = if owner_name != 0 { owner_name } else { text5::EMPTY_C };
            let a3 = if matel != 0 { matel } else { text5::DEFAULT_A };
            let a2 = if with_name { text5::DASH_C } else { text5::EMPTY_D };
            let a1 = if with_name { name } else { text5::EMPTY_E };
            fprintf(m, o, &[text5::SURFACE_A, a1, a2, a3, a4, a5, ext]);
        }
        if ext != 0 {
            m.free(ext);
        }
        let a = dw(m.f64(s + 0x10));
        let b2 = dw(m.f64(s + 0x18));
        fprintf(m, o, &[text5::UNURBS, a[0], a[1], b2[0], b2[1]]);
        print_nurbs_curve(m, o, s);
        let a = dw(m.f64(s + 0x34));
        let b2 = dw(m.f64(s + 0x3c));
        fprintf(m, o, &[text5::VNURBS, a[0], a[1], b2[0], b2[1]]);
        print_nurbs_curve(m, o, s + 0x24);
        let mut nue = m.i32(s + 4);
        if m.i16(s + 0x20) != 0 {
            nue = m.i32(s).wrapping_sub(2).wrapping_add(nue);
        }
        let mut nve = m.i32(s + 0x28);
        if m.i16(s + 0x44) != 0 {
            nve = m.i32(s + 0x24).wrapping_sub(2).wrapping_add(nve);
        }
        let lo_u = if m.i16(s + 0x20) != 0 { -1 } else { 0 };
        let lo_v = if m.i16(s + 0x44) != 0 { -1 } else { 0 };
        let mut jv = nve;
        loop {
            jv -= 1;
            if lo_v > jv {
                break;
            }
            let nv2 = m.i32(s + 0x28);
            let row = (nv2.wrapping_add(jv)) % nv2;
            let mut cntr: u32 = 0;
            let mut iu = nue;
            loop {
                iu -= 1;
                if lo_u > iu {
                    break;
                }
                let n = m.i32(s + 4);
                let col = (iu.wrapping_add(n)) % n;
                let idx = nv2.wrapping_sub(row + 1).wrapping_mul(n).wrapping_add(n).wrapping_sub(col + 1);
                let w = dw(m.f64(m.u32(s + 0x54).wrapping_add(0x18).wrapping_add((idx as u32).wrapping_mul(0x24))));
                let sep = if cntr & 3 != 0 { text5::TAB_E } else { text5::NL_TABS3_E };
                fprintf(m, o, &[text5::S_D_F_B, sep, (col.wrapping_add(row.wrapping_mul(n))) as u32, w[0], w[1]]);
                cntr += 1;
            }
        }
        fprintf(m, o, &[text5::NL]);
        print_nurb_tcoord_ref(m, o, dict, elem, m.i32(s + 4).wrapping_mul(m.i32(s + 0x28)));
        if textured {
            fprintf(m, o, &[text5::TEXTURE]);
            let u = dw(nue as f64 - 1.0);
            fprintf(m, o, &[text5::BEZ1_A, 0, 0, u[0], u[1]]);
            let v = dw(nve as f64 - 1.0);
            fprintf(m, o, &[text5::BEZ1_B, 0, 0, v[0], v[1]]);
            let last = m.u32(g::NVERT).wrapping_sub(1);
            fprintf(m, o, &[text5::QUAD_D, last, last, last, last]);
        }
        let mut cv: u16 = 0;
        for gi in 0..ntrim {
            let gr = group(m, gi);
            let kind = if m.i16(gr) != 0 { text5::TRIM } else { text5::HOLE };
            fprintf(m, o, &[text5::TRIM_HDR, kind]);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                let pre = if ci != 0 { text5::PRE_CONT } else { text5::PRE_FIRST };
                let (a, b2) = (dw(m.f64(cr + 0x10)), dw(m.f64(cr + 0x18)));
                fprintf(m, o, &[text5::TRIM_CURVE, pre, cv as u32, a[0], a[1], b2[0], b2[1]]);
                cv = cv.wrapping_add(1);
            }
        }
        fprintf(m, o, &[text5::NL_B]);
        let view = m.i16(s + 0x9c) != 0;
        match m.u16(s + 0x98) {
            2 => {
                let val = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa0))).to_f64());
                fprintf(m, o, &[text5::APPROX_SPATIAL, if view { text5::VIEW_A } else { text5::NO_VIEW_A }, val[0], val[1]]);
                if m.i16(s + 0xb0) != 0 {
                    fprintf(m, o, &[text5::SIZES_A, m.u16(s + 0xac) as u32, m.u16(s + 0xae) as u32]);
                }
                fprintf(m, o, &[text5::END_A]);
            }
            1 => {
                let v = (F80::from_i32(m.i32(s + 0x74)) / F80::from_i32(m.i32(s + 0x24).wrapping_sub(1))).to_f64();
                let u = (F80::from_i32(m.i32(s + 0x70)) / F80::from_i32(m.i32(s).wrapping_sub(1))).to_f64();
                let (u, v) = (dw(u), dw(v));
                fprintf(m, o, &[text5::APPROX_PARAM, u[0], u[1], v[0], v[1]]);
            }
            3 => {
                let f1 = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa4))).to_f64());
                let f2 = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa8))).to_f64());
                fprintf(m, o, &[text5::APPROX_CURV, if view { text5::VIEW_B } else { text5::NO_VIEW_B }, f1[0], f1[1], f2[0], f2[1]]);
                if m.i16(s + 0xb0) != 0 {
                    fprintf(m, o, &[text5::SIZES_B, m.u16(s + 0xac) as u32, m.u16(s + 0xae) as u32]);
                }
                fprintf(m, o, &[text5::END_B]);
            }
            _ => {}
        }
        let ratio: f64 = if ntrim < 1 {
            let (mut a, ord) = (m.i32(s + 0x70), 0);
            let _ = ord;
            let b2 = m.i32(s + 0x74);
            let ordv = if a < b2 {
                a = b2;
                m.i32(s + 0x24)
            } else {
                m.i32(s)
            };
            (F80::from_i32(a) / F80::from_i32(ordv.wrapping_sub(1))).to_f64()
        } else {
            f32::from_bits(0x3eaa_aa9f) as f64
        };
        let r = dw(ratio);
        fprintf(m, o, &[text5::APPROX_TRIM, r[0], r[1]]);
        fprintf(m, o, &[text5::END_GROUP]);
    }
    if motion != 0 && m.u32(def + 0x33c) != 0 {
        let gv = m.u32(SI_GBL_VERT);
        let mut b: u32 = 0;
        while b < cnt {
            let rec = m.u32(def + 0x33c);
            let dst = m.u32(rec + 0x22c);
            let off = b.wrapping_mul(0x4c);
            for k in 0..3 {
                m.set_u32(dst.wrapping_add(off).wrapping_add(8 + 4 * k), quiet(m.u32(gv.wrapping_add(off).wrapping_add(8 + 4 * k))));
            }
            b += 1;
        }
    }
    if inst && m.u32(def + 0x33c) != 0 {
        for k in 0..3 {
            let rec = m.u32(def + 0x33c);
            m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(cen_out + 4 * k)));
        }
    }
    if flip {
        o.call(m, SRF_DISPOSE, &[def + 0x48]);
        m.set_u32(def + 0x48, saved);
    }
    m.release(L);
}

/// The texts of `mi2_print_nurbs_geometry` in `.rodata`, by address.
pub mod text6 {
    pub const DEBUG: u32 = 0x086f_2220;
    pub const DEBUG_ARG: u32 = 0x086f_2209;
    pub const BASIS_BEZIER1: u32 = 0x086f_225e;
    pub const RATIONAL_A: u32 = 0x086f_2278;
    pub const BASIS_U: u32 = 0x086f_2282;
    pub const RATIONAL_B: u32 = 0x086f_229f;
    pub const BASIS_V: u32 = 0x086f_22a9;
    pub const RATIONAL_C: u32 = 0x086f_22c6;
    pub const EMPTY_A: u32 = 0x086f_22d0;
    pub const BASIS_CURVE: u32 = 0x086f_22d1;
    pub const GROUP: u32 = 0x086f_22ef;
    pub const CURVE_HDR: u32 = 0x086f_22ff;
    pub const TAB_A: u32 = 0x086f_230b;
    pub const NL_TABS2_A: u32 = 0x086f_230d;
    pub const V_D: u32 = 0x086f_2311;
    pub const M_D_A: u32 = 0x086f_2318;
    pub const M_D_B: u32 = 0x086f_231e;
    pub const CURVE_NL: u32 = 0x086f_2324;
    pub const TAB_B: u32 = 0x086f_2330;
    pub const NL_TABS2_B: u32 = 0x086f_2332;
    pub const V_D_B: u32 = 0x086f_2336;
    pub const CURVE_DEF: u32 = 0x086f_233d;
    pub const TAB_C: u32 = 0x086f_235a;
    pub const NL_TABS3_C: u32 = 0x086f_235c;
    pub const S_D_F: u32 = 0x086f_2361;
    pub const SPACE_D: u32 = 0x086f_2369;
    pub const NL_TABS3_D: u32 = 0x086f_236b;
    pub const S_D: u32 = 0x086f_2370;
    pub const EMPTY_B: u32 = 0x086f_2375;
    pub const DASH_B: u32 = 0x086f_2376;
    pub const EMPTY_C: u32 = 0x086f_2378;
    pub const DEFAULT_A: u32 = 0x086f_2379;
    pub const DASH_C: u32 = 0x086f_2381;
    pub const EMPTY_D: u32 = 0x086f_2383;
    pub const EMPTY_E: u32 = 0x086f_2384;
    pub const SURFACE_A: u32 = 0x086f_23a0;
    pub const DEFAULT_B: u32 = 0x086f_23c4;
    pub const DASH_D: u32 = 0x086f_23cc;
    pub const EMPTY_F: u32 = 0x086f_23ce;
    pub const EMPTY_G: u32 = 0x086f_23cf;
    pub const SURFACE_B: u32 = 0x086f_23d0;
    pub const UNURBS: u32 = 0x086f_23ed;
    pub const VNURBS: u32 = 0x086f_23ff;
    pub const TAB_E: u32 = 0x086f_2411;
    pub const NL_TABS3_E: u32 = 0x086f_2413;
    pub const S_D_F_B: u32 = 0x086f_2418;
    pub const NL: u32 = 0x086f_242c;
    pub const TEXTURE_A: u32 = 0x086f_242e;
    pub const BEZ1_A: u32 = 0x086f_2439;
    pub const BEZ1_B: u32 = 0x086f_244c;
    pub const QUAD_A: u32 = 0x086f_2460;
    pub const TEXTURE_B: u32 = 0x086f_2470;
    pub const BEZ1_C: u32 = 0x086f_247b;
    pub const BEZ1_D: u32 = 0x086f_248e;
    pub const QUAD_B: u32 = 0x086f_24a2;
    pub const TRIM: u32 = 0x086f_24b2;
    pub const HOLE: u32 = 0x086f_24b7;
    pub const TRIM_HDR: u32 = 0x086f_24bc;
    pub const PRE_CONT: u32 = 0x086f_24c2;
    pub const PRE_FIRST: u32 = 0x086f_24c5;
    pub const TRIM_CURVE: u32 = 0x086f_24c6;
    pub const NL_B: u32 = 0x086f_24d8;
    pub const APPROX_PARAM: u32 = 0x086f_24e0;
    pub const VIEW_A: u32 = 0x086f_2510;
    pub const NO_VIEW_A: u32 = 0x086f_2516;
    pub const APPROX_SPATIAL: u32 = 0x086f_2520;
    pub const SIZES_A: u32 = 0x086f_2544;
    pub const END_A: u32 = 0x086f_254b;
    pub const VIEW_B: u32 = 0x086f_2554;
    pub const NO_VIEW_B: u32 = 0x086f_255a;
    pub const APPROX_CURV: u32 = 0x086f_2560;
    pub const SIZES_B: u32 = 0x086f_2589;
    pub const END_B: u32 = 0x086f_2590;
    pub const APPROX_CURVE: u32 = 0x086f_25a0;
    pub const END_GROUP: u32 = 0x086f_25cc;
}

/// `SI_matrixCopy (0x08516224)`: `(src, dst)`, sixteen floats; ported in `si-ascii` (the check answers it).
pub const MATRIX_COPY: Routine = Routine::new("SI_matrixCopy", 0x0851_6224);
/// `invert_matrix (0x0859883c)`: `(out, in)`, `mi_calc.c`.
pub const INVERT_MATRIX: Routine = Routine::new("invert_matrix", 0x0859_883c);
/// `mi2_motion_vector (0x085908a8)`: `(def, &vec, &inverse, index)`, `mi_poly.c`.
pub const MI2_MOTION_VECTOR: Routine = Routine::new("mi2_motion_vector", 0x0859_08a8);

/// The material colour a mental ray 2 surface writes before its vertices, as the first
/// `print_compressed_vector` call of `mi2_print_nurbs_geometry`/`mi2_print_patch`: the material
/// (kind at +0xdc8) gives 255 times its colour (kind 1), 179 plus its colour (kind 3) or 179, the
/// first two clamped to 0..255; then the transparency (+0x744) as 255 times one minus it.
fn mi2_material_vectors(m: &mut Mem, o: &mut dyn Outside, r: u32) {
    let k255 = F80::from_f64(255.0);
    let k179 = F80::from_f32(f32::from_bits(0x4333_0000));
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(r + off)));
    let ty = m.u16(r + 0xdc8);
    let (mut d4, mut d0, mut cc) = (0x4333_0000u32, 0x4333_0000u32, 0x4333_0000u32);
    let mut p5 = F80::from_i32(0);
    let mut clamp = false;
    if ty == 1 {
        p5 = k255 * f(m, 0x720);
        d4 = p5.to_f32().to_bits();
        d0 = (k255 * f(m, 0x724)).to_f32().to_bits();
        cc = (k255 * f(m, 0x728)).to_f32().to_bits();
        clamp = true;
    } else if ty == 3 {
        p5 = f(m, 0x720) + k179;
        d4 = p5.to_f32().to_bits();
        d0 = (f(m, 0x724) + k179).to_f32().to_bits();
        cc = (k179 + f(m, 0x728)).to_f32().to_bits();
        clamp = true;
    }
    if clamp {
        let k255s = 0x437f_0000u32;
        let zero = F80::from_i32(0);
        let ff = |b: u32| F80::from_f32(f32::from_bits(b));
        if p5.gt(k255) {
            d4 = k255s;
        }
        if ff(d4).lt(zero) {
            d4 = 0;
        }
        if k255.lt(ff(d0)) {
            d0 = k255s;
        }
        if zero.gt(ff(d0)) {
            d0 = 0;
        }
        if k255.lt(ff(cc)) {
            cc = k255s;
        }
        if zero.gt(ff(cc)) {
            cc = 0;
        }
    }
    cvec(m, o, quiet(d4), quiet(d0), quiet(cc));
    let one = F80::from_i32(1);
    let t = f(m, 0x744);
    let a: u32 = if m.u16(r + 0xdca) == 2 || t.gt(one) {
        0
    } else if t.lt(F80::from_i32(0)) {
        0x437f_0000
    } else {
        ((one - t) * k255).to_f32().to_bits()
    };
    cvec(m, o, quiet(a), 0, 0);
    m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(2));
}

/// The NURBS surface of an element for mental ray 2 [soft 0x0858a2d4 mi2_print_nurbs_geometry].
///
/// As [`print_nurbs_geometry`] with these differences: the vertices are transformed only when
/// `motion` is set (the vertex array gets the transformed ones, the file the untransformed ones);
/// the inverse of the matrix without its translation goes to `mi2_motion_vector`; a material
/// (not kind 0x10001 at +0xdc8) writes its colour and transparency as two more vertices
/// ([`mi2_material_vectors`]) and two texture blocks; every trim curve gets an
/// `approximate curve parametric` line; the final trim approximation is not written.
pub fn mi2_print_nurbs_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let mut motion = motion;
    let mut inst = false;
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let mut saved = 0;
    if flip {
        saved = m.u32(def + 0x48);
        let c = o.call(m, SRF_COPY, &[saved]).eax;
        m.set_u32(def + 0x48, c);
        o.call(m, SRF_INVERSE, &[m.u32(def + 0x48), 1, 0]);
    }
    let s = m.u32(def + 0x48);
    let (ou, ov) = (m.i32(s), m.i32(s + 0x24));
    let (nu, nv) = (m.i32(s + 4), m.i32(s + 0x28));
    let ntrim = m.i32(s + 0x60);
    o.call(
        m,
        SDEBUG,
        &[text6::DEBUG, text6::DEBUG_ARG, ou.wrapping_sub(1) as u32, ov.wrapping_sub(1) as u32, m.u32(s + 8), m.u32(s + 0x2c), nu as u32, nv as u32],
    );
    let groups = m.u32(s + 0x64);
    let group = |m: &Mem, g: i32| m.u32(groups.wrapping_add((g as u32).wrapping_mul(4)));
    let curve = |m: &Mem, gr: u32, c: i32| m.u32(m.u32(gr + 8).wrapping_add((c as u32).wrapping_mul(4)));
    if no_out == 0 {
        fprintf(m, o, &[text6::BASIS_BEZIER1]);
        fprintf(m, o, &[text6::BASIS_U, text6::RATIONAL_A, ou.wrapping_sub(1) as u32]);
        fprintf(m, o, &[text6::BASIS_V, text6::RATIONAL_B, ov.wrapping_sub(1) as u32]);
        let mut idx: i32 = 0;
        for g in 0..ntrim {
            let gr = group(m, g);
            for c in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, c);
                let rat = if m.i16(cr + 0x28) != 0 { text6::RATIONAL_C } else { text6::EMPTY_A };
                fprintf(m, o, &[text6::BASIS_CURVE, idx as u32, rat, m.i32(cr).wrapping_sub(1) as u32]);
                idx += 1;
            }
        }
    }
    // locals: the centre, the transformed centre (also the motion vector), one vertex in and out, the
    // related element, the matrix copy and its inverse
    const L: u32 = 12 * 4 + 4 + 128;
    let l = m.local(L);
    let (cen_in, cen_out, v_in, v_out, outp, mcopy, minv) = (l, l + 12, l + 24, l + 36, l + 48, l + 52, l + 116);
    o.call(m, MODEL_COMPUTE_BOUNDING, &[def]);
    let half = F80::from_f32(0.5);
    for (k, (a, b)) in [(0x1c, 0x10), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let sum = F80::from_f32(f32::from_bits(m.u32(def + a))) + F80::from_f32(f32::from_bits(m.u32(def + b)));
        m.set_u32(cen_in + 4 * k as u32, (sum * half).to_f32().to_bits());
    }
    o.call(m, VECTOR_X_MATRIX, &[cen_out, cen_in, mat]);
    let npts = (m.u32(s + 0x28)).wrapping_mul(m.u32(s + 4));
    if m.u32(MB_NB_MAX_VERT) <= npts && npts.wrapping_sub(m.u32(MB_NB_MAX_VERT)) != 0 {
        let hi = ((npts as u64 * 0xd1b7_1759u64) >> 32) as u32 >> 12;
        let cap = hi.wrapping_mul(5000).wrapping_add(5000);
        let p = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(SI_GBL_VERT), cap.wrapping_mul(0x4c)]).eax;
        m.set_u32(SI_GBL_VERT, p);
        m.set_u32(MB_NB_MAX_VERT, cap);
    }
    let orig_motion = motion;
    if orig_motion != 0 {
        if m.u8(def + 0x3ee) & 1 != 0 {
            inst = true;
            motion = 0;
        }
        if m.u32(def + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(def + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        if motion != 0 {
            let rec = m.u32(def + 0x33c);
            if rec != 0 {
                let new = if m.u32(rec + 0x22c) == 0 {
                    Some(o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, npts.wrapping_mul(0x4c)]).eax)
                } else if m.u32(rec + 0x228) < npts {
                    Some(o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), npts.wrapping_mul(0x4c)]).eax)
                } else {
                    None
                };
                if let Some(new) = new {
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x22c, new);
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x228, npts);
                }
            }
        }
    }
    if no_out == 0 {
        fprintf(m, o, &[text6::GROUP]);
    }
    m.set_i32(g::NVERT, 0);
    if no_out == 0 || motion != 0 {
        let mut j = m.i32(s + 0x28) - 1;
        while j >= 0 {
            let n = m.i32(s + 4);
            let mut k = 0;
            while k < n {
                let idx = (j.wrapping_mul(n)).wrapping_add(n).wrapping_sub(k + 1);
                let p = m.u32(s + 0x54).wrapping_add((idx as u32).wrapping_mul(0x24));
                let w = F80::from_f64(m.f64(p + 0x18));
                let w = if w.eq(F80::from_i32(0)) { F80::from_i32(1) } else { w };
                let inv = F80::from_i32(1) / w;
                let x = (F80::from_f64(m.f64(p)) * inv).to_f32().to_bits();
                let y = (F80::from_f64(m.f64(p + 8)) * inv).to_f32().to_bits();
                let z = (inv * F80::from_f64(m.f64(p + 0x10))).to_f32().to_bits();
                m.set_u32(v_in, x);
                m.set_u32(v_in + 4, y);
                m.set_u32(v_in + 8, z);
                if motion != 0 {
                    o.call(m, VECTOR_X_MATRIX, &[v_out, v_in, mat]);
                    let gv = m.u32(SI_GBL_VERT).wrapping_add((m.i32(g::NVERT) as u32).wrapping_mul(0x4c));
                    for q in 0..3 {
                        m.set_u32(gv + 8 + 4 * q, quiet(m.u32(v_out + 4 * q)));
                    }
                }
                if no_out == 0 || motion == 0 {
                    cvec(m, o, quiet(x), quiet(y), quiet(z));
                }
                m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
                k += 1;
            }
            j -= 1;
        }
    }
    let cnt = m.i32(g::NVERT) as u32;
    if no_out == 0 {
        let tcn = print_nurb_tcoord(m, o, dict, elem);
        let mut moves: i32 = 0;
        if motion != 0 || inst {
            o.call(m, MATRIX_COPY, &[mat, mcopy]);
            for k in 0..3 {
                m.set_u32(mcopy + 0x30 + 4 * k, 0);
            }
            o.call(m, INVERT_MATRIX, &[minv, mcopy]);
        }
        if motion != 0 {
            for b in 0..cnt {
                let r = o.call(m, MI2_MOTION_VECTOR, &[def, cen_out, minv, b]).eax;
                moves = moves.wrapping_add(r as i32);
            }
        }
        if inst {
            moves = o.call(m, MI2_MOTION_VECTOR, &[def, cen_out, minv, 0]).eax as i32;
        }
        let cid_mat = m.u32(g::CID_MATERIALS);
        let matel = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, outp, 0]).eax;
        let textured = matel != 0 && m.u32(matel + 0xdc8) != 0x10001;
        if textured {
            mi2_material_vectors(m, o, matel);
        }
        for gi in 0..ntrim {
            fprintf(m, o, &[text6::CURVE_HDR, gi as u32]);
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                let pts = m.u32(cr + 0x2c);
                for pi in 0..m.i32(cr + 4) {
                    let p = pts.wrapping_add((pi as u32).wrapping_mul(0x24));
                    let (x, y) = (f64_to_f32_bits(m.f64(p)), f64_to_f32_bits(m.f64(p + 8)));
                    cvec(m, o, x, y, 0);
                }
            }
        }
        let mut b: i32 = 0;
        while b < m.i32(g::NVERT) {
            let sep = if b & 3 != 0 { text6::TAB_A } else { text6::NL_TABS2_A };
            fprintf(m, o, &[text6::V_D, sep, b as u32]);
            if moves != 0 && (b as u32) < cnt {
                if motion != 0 {
                    fprintf(m, o, &[text6::M_D_A, (b as u32).wrapping_add(m.u32(g::NVERT))]);
                } else if inst {
                    fprintf(m, o, &[text6::M_D_B, m.u32(g::NVERT)]);
                }
            }
            b += 1;
        }
        let mut ix = cnt.wrapping_add(moves as u32).wrapping_add(tcn as u32);
        if textured {
            ix = ix.wrapping_add(2);
        }
        for gi in 0..ntrim {
            fprintf(m, o, &[text6::CURVE_NL, gi as u32]);
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                for pi in 0..m.i32(cr + 4) {
                    let sep = if pi & 7 != 0 { text6::TAB_B } else { text6::NL_TABS2_B };
                    fprintf(m, o, &[text6::V_D_B, sep, ix]);
                    ix = ix.wrapping_add(1);
                }
            }
        }
        let mut base_ix: i32 = 0;
        let mut cn: i32 = 0;
        for gi in 0..ntrim {
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                fprintf(m, o, &[text6::CURVE_DEF, cn as u32, cn as u32]);
                print_nurbs_curve(m, o, cr);
                let closed = m.i16(cr + 0x20) != 0;
                let n = m.i32(cr + 4);
                let mut t = if closed { 2i32.wrapping_sub(m.i32(cr)) } else { 0 };
                let end = if closed { n.wrapping_add(1) } else { n };
                cn += 1;
                let mut cntr: i32 = 0;
                while t < end {
                    let r = (t.wrapping_add(n)) % n;
                    let vi = (m.i32(g::NVERT)).wrapping_add(base_ix).wrapping_add(r);
                    if m.i16(cr + 0x28) == 0 {
                        let sep = if cntr != (cntr / 12) * 12 { text6::SPACE_D } else { text6::NL_TABS3_D };
                        fprintf(m, o, &[text6::S_D, sep, vi as u32]);
                    } else {
                        let w = dw(m.f64(m.u32(cr + 0x2c).wrapping_add(0x18).wrapping_add((r as u32).wrapping_mul(0x24))));
                        let sep = if cntr & 3 != 0 { text6::TAB_C } else { text6::NL_TABS3_C };
                        fprintf(m, o, &[text6::S_D_F, sep, vi as u32, w[0], w[1]]);
                    }
                    cntr += 1;
                    t += 1;
                }
                base_ix = base_ix.wrapping_add(n);
            }
        }
        let mut name = 0u32;
        let rec = m.u32(outp);
        if rec != 0 {
            let r2 = m.u32(rec);
            name = m.u32(r2 + 0x10);
        }
        if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 {
            name = 0;
        }
        let ext = o.call(m, MAT_NAME_EXTENSION, &[dict, elem]).eax;
        let with_name = matel != 0 && name != 0;
        if ext == 0 {
            let a3 = if matel != 0 { matel } else { text6::DEFAULT_B };
            let a2 = if with_name { text6::DASH_D } else { text6::EMPTY_F };
            let a1 = if with_name { name } else { text6::EMPTY_G };
            fprintf(m, o, &[text6::SURFACE_B, a1, a2, a3]);
        } else {
            let a5 = if owner_name != 0 { text6::DASH_B } else { text6::EMPTY_B };
            let a4 = if owner_name != 0 { owner_name } else { text6::EMPTY_C };
            let a3 = if matel != 0 { matel } else { text6::DEFAULT_A };
            let a2 = if with_name { text6::DASH_C } else { text6::EMPTY_D };
            let a1 = if with_name { name } else { text6::EMPTY_E };
            fprintf(m, o, &[text6::SURFACE_A, a1, a2, a3, a4, a5, ext]);
        }
        if ext != 0 {
            m.free(ext);
        }
        let a = dw(m.f64(s + 0x10));
        let b2 = dw(m.f64(s + 0x18));
        fprintf(m, o, &[text6::UNURBS, a[0], a[1], b2[0], b2[1]]);
        print_nurbs_curve(m, o, s);
        let a = dw(m.f64(s + 0x34));
        let b2 = dw(m.f64(s + 0x3c));
        fprintf(m, o, &[text6::VNURBS, a[0], a[1], b2[0], b2[1]]);
        print_nurbs_curve(m, o, s + 0x24);
        let mut nue = m.i32(s + 4);
        if m.i16(s + 0x20) != 0 {
            nue = m.i32(s).wrapping_sub(2).wrapping_add(nue);
        }
        let mut nve = m.i32(s + 0x28);
        if m.i16(s + 0x44) != 0 {
            nve = m.i32(s + 0x24).wrapping_sub(2).wrapping_add(nve);
        }
        let lo_u = if m.i16(s + 0x20) != 0 { -1 } else { 0 };
        let lo_v = if m.i16(s + 0x44) != 0 { -1 } else { 0 };
        let mut jv = nve;
        loop {
            jv -= 1;
            if lo_v > jv {
                break;
            }
            let nv2 = m.i32(s + 0x28);
            let row = (nv2.wrapping_add(jv)) % nv2;
            let mut cntr: u32 = 0;
            let mut iu = nue;
            loop {
                iu -= 1;
                if lo_u > iu {
                    break;
                }
                let n = m.i32(s + 4);
                let col = (iu.wrapping_add(n)) % n;
                let idx = nv2.wrapping_sub(row + 1).wrapping_mul(n).wrapping_add(n).wrapping_sub(col + 1);
                let w = dw(m.f64(m.u32(s + 0x54).wrapping_add(0x18).wrapping_add((idx as u32).wrapping_mul(0x24))));
                let sep = if cntr & 3 != 0 { text6::TAB_E } else { text6::NL_TABS3_E };
                fprintf(m, o, &[text6::S_D_F_B, sep, (col.wrapping_add(row.wrapping_mul(n))) as u32, w[0], w[1]]);
                cntr += 1;
            }
        }
        fprintf(m, o, &[text6::NL]);
        print_nurb_tcoord_ref(m, o, dict, elem, m.i32(s + 4).wrapping_mul(m.i32(s + 0x28)));
        if textured {
            fprintf(m, o, &[text6::TEXTURE_A]);
            let u = dw(nue as f64 - 1.0);
            let v = dw(nve as f64 - 1.0);
            fprintf(m, o, &[text6::BEZ1_A, 0, 0, u[0], u[1]]);
            fprintf(m, o, &[text6::BEZ1_B, 0, 0, v[0], v[1]]);
            let l2 = m.u32(g::NVERT).wrapping_sub(2);
            fprintf(m, o, &[text6::QUAD_A, l2, l2, l2, l2]);
            fprintf(m, o, &[text6::TEXTURE_B]);
            fprintf(m, o, &[text6::BEZ1_C, 0, 0, u[0], u[1]]);
            fprintf(m, o, &[text6::BEZ1_D, 0, 0, v[0], v[1]]);
            let l1 = m.u32(g::NVERT).wrapping_sub(1);
            fprintf(m, o, &[text6::QUAD_B, l1, l1, l1, l1]);
        }
        let mut cv: u16 = 0;
        for gi in 0..ntrim {
            let gr = group(m, gi);
            let kind = if m.i16(gr) != 0 { text6::TRIM } else { text6::HOLE };
            fprintf(m, o, &[text6::TRIM_HDR, kind]);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                let pre = if ci != 0 { text6::PRE_CONT } else { text6::PRE_FIRST };
                let (a, b2) = (dw(m.f64(cr + 0x10)), dw(m.f64(cr + 0x18)));
                fprintf(m, o, &[text6::TRIM_CURVE, pre, cv as u32, a[0], a[1], b2[0], b2[1]]);
                cv = cv.wrapping_add(1);
            }
        }
        fprintf(m, o, &[text6::NL_B]);
        let view = m.i16(s + 0x9c) != 0;
        match m.u16(s + 0x98) {
            2 => {
                let val = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa0))).to_f64());
                fprintf(m, o, &[text6::APPROX_SPATIAL, if view { text6::VIEW_A } else { text6::NO_VIEW_A }, val[0], val[1]]);
                if m.i16(s + 0xb0) != 0 {
                    fprintf(m, o, &[text6::SIZES_A, m.u16(s + 0xac) as u32, m.u16(s + 0xae) as u32]);
                }
                fprintf(m, o, &[text6::END_A]);
            }
            1 => {
                let v = (F80::from_i32(m.i32(s + 0x74)) / F80::from_i32(m.i32(s + 0x24).wrapping_sub(1))).to_f64();
                let u = (F80::from_i32(m.i32(s + 0x70)) / F80::from_i32(m.i32(s).wrapping_sub(1))).to_f64();
                let (u, v) = (dw(u), dw(v));
                fprintf(m, o, &[text6::APPROX_PARAM, u[0], u[1], v[0], v[1]]);
            }
            3 => {
                let f1 = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa4))).to_f64());
                let f2 = dw(F80::from_f32(f32::from_bits(m.u32(s + 0xa8))).to_f64());
                fprintf(m, o, &[text6::APPROX_CURV, if view { text6::VIEW_B } else { text6::NO_VIEW_B }, f1[0], f1[1], f2[0], f2[1]]);
                if m.i16(s + 0xb0) != 0 {
                    fprintf(m, o, &[text6::SIZES_B, m.u16(s + 0xac) as u32, m.u16(s + 0xae) as u32]);
                }
                fprintf(m, o, &[text6::END_B]);
            }
            _ => {}
        }
        let mut cn: i32 = 0;
        for gi in 0..ntrim {
            let gr = group(m, gi);
            for ci in 0..m.i32(gr + 4) {
                let cr = curve(m, gr, ci);
                let r = (F80::from_i32(m.i32(cr + 0x34)) / F80::from_i32(m.i32(cr).wrapping_sub(1))).to_f64();
                let r = dw(r);
                fprintf(m, o, &[text6::APPROX_CURVE, r[0], r[1], cn as u32]);
                cn += 1;
            }
        }
        fprintf(m, o, &[text6::END_GROUP]);
    }
    if motion != 0 && m.u32(def + 0x33c) != 0 {
        let gv = m.u32(SI_GBL_VERT);
        let mut b: u32 = 0;
        while b < cnt {
            let rec = m.u32(def + 0x33c);
            let dst = m.u32(rec + 0x22c);
            let off = b.wrapping_mul(0x4c);
            for k in 0..3 {
                m.set_u32(dst.wrapping_add(off).wrapping_add(8 + 4 * k), quiet(m.u32(gv.wrapping_add(off).wrapping_add(8 + 4 * k))));
            }
            b += 1;
        }
    }
    if inst && m.u32(def + 0x33c) != 0 {
        for k in 0..3 {
            let rec = m.u32(def + 0x33c);
            m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(cen_out + 4 * k)));
        }
    }
    if flip {
        o.call(m, SRF_DISPOSE, &[def + 0x48]);
        m.set_u32(def + 0x48, saved);
    }
    m.release(L);
}

/// The texts of `mi2_print_patch` in `.rodata`, by address.
pub mod textp {
    pub const DEBUG: u32 = 0x086f_2640;
    pub const WARN_POINTS: u32 = 0x086f_2680;
    pub const FATAL_ALLOC: u32 = 0x086f_26c0;
    pub const NL: u32 = 0x086f_26e6;
    pub const TAB: u32 = 0x086f_26e8;
    pub const NL_TABS2: u32 = 0x086f_26ea;
    pub const V_D: u32 = 0x086f_26ee;
    pub const M_D_A: u32 = 0x086f_26f5;
    pub const M_D_B: u32 = 0x086f_26fb;
    pub const BEZIER_U: u32 = 0x086f_276d;
    pub const TAB_U: u32 = 0x086f_2784;
    pub const NL_TABS3_U: u32 = 0x086f_2786;
    pub const KNOT_U: u32 = 0x086f_278b;
    pub const NL_U: u32 = 0x086f_2793;
    pub const BEZIER_V: u32 = 0x086f_2795;
    pub const TAB_V: u32 = 0x086f_27ac;
    pub const NL_TABS3_V: u32 = 0x086f_27ae;
    pub const KNOT_V: u32 = 0x086f_27b3;
    pub const NL_V: u32 = 0x086f_27bb;
    pub const SPACE_I: u32 = 0x086f_27bd;
    pub const NL_TABS3_I: u32 = 0x086f_27bf;
    pub const IDX_D: u32 = 0x086f_27c4;
    pub const NL_END: u32 = 0x086f_27c9;
    pub const APPROX_PARAM: u32 = 0x086f_2860;
    pub const VIEW_A: u32 = 0x086f_2890;
    pub const NO_VIEW_A: u32 = 0x086f_2896;
    pub const APPROX_SPATIAL: u32 = 0x086f_28a0;
    pub const SIZES_A: u32 = 0x086f_28c4;
    pub const END_A: u32 = 0x086f_28cb;
    pub const VIEW_B: u32 = 0x086f_28d4;
    pub const NO_VIEW_B: u32 = 0x086f_28da;
    pub const APPROX_CURV: u32 = 0x086f_28e0;
    pub const SIZES_B: u32 = 0x086f_2909;
    pub const END_B: u32 = 0x086f_2910;
    pub const APPROX_TRIM: u32 = 0x086f_2920;
    pub const DASH_A: u32 = 0x086f_2702;
    pub const EMPTY_A: u32 = 0x086f_2701;
    pub const EMPTY_B: u32 = 0x086f_2704;
    pub const DEFAULT_A: u32 = 0x086f_2705;
    pub const DASH_B: u32 = 0x086f_270d;
    pub const EMPTY_C: u32 = 0x086f_270f;
    pub const EMPTY_D: u32 = 0x086f_2710;
    pub const SURFACE_A: u32 = 0x086f_2720;
    pub const DEFAULT_B: u32 = 0x086f_2744;
    pub const DASH_C: u32 = 0x086f_274c;
    pub const EMPTY_E: u32 = 0x086f_274e;
    pub const EMPTY_F: u32 = 0x086f_274f;
    pub const SURFACE_B: u32 = 0x086f_2750;
    pub const TEXTURE_A: u32 = 0x086f_27cb;
    pub const BEZ1_A: u32 = 0x086f_27dd;
    pub const BEZ1_B: u32 = 0x086f_27f0;
    pub const QUAD_A: u32 = 0x086f_2804;
    pub const TEXTURE_B: u32 = 0x086f_2814;
    pub const BEZ1_C: u32 = 0x086f_2826;
    pub const BEZ1_D: u32 = 0x086f_2839;
    pub const QUAD_B: u32 = 0x086f_284d;
    pub const BASIS3: u32 = 0x086f_25d8;
    pub const BASIS1: u32 = 0x086f_25f2;
    pub const GROUP: u32 = 0x086f_260c;
    pub const END_GROUP: u32 = 0x086f_261c;
}

/// A patch written as mental ray 2 scene text [soft 0x0858b9b0 mi2_print_patch].
///
/// As [`print_patch`] with these differences: the vertices are transformed only when `motion` is
/// set (the array gets the transformed ones, the file the untransformed ones); the inverse of the
/// matrix without its translation goes to `mi2_motion_vector`; a material (not kind 0x10001 at
/// +0xdc8) writes its colour and transparency as two more vertices ([`mi2_material_vectors`]) and
/// two texture blocks; the trim approximation stays.
///
pub fn mi2_print_patch(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, patch: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let mut motion = motion;
    let mut inst = false;
    let mut owner_name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            owner_name = n;
        }
    }
    // mirrored: the product of the model's scale against the handedness, flipped by bit 3
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(tf + base + off)));
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    let mut flip = if prod.lt(F80::from_i32(0)) { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let mut patch = patch;
    let mut saved = 0;
    if flip {
        saved = patch;
        patch = o.call(m, PATCH_COPY, &[patch]).eax;
        o.call(m, PATCH_INVERSE, &[patch, 0]);
    }
    let (t1, t2) = (type_name(m, m.u32(patch + 4)), type_name(m, m.u32(patch + 8)));
    let sizes = [m.u16(patch + 0xc) as u32, m.u16(patch + 0xe) as u32, m.u16(patch + 0x1c) as u32, m.u16(patch + 0x1e) as u32];
    o.call(m, SDEBUG, &[textp::DEBUG, t1, t2, sizes[0], sizes[1], sizes[2], sizes[3]]);
    // locals: the copy of the patch, the transformed centre (also the motion vector), the centre, one vertex in and out
    const L: u32 = 0x204 + 4 * 12 + 8 + 128;
    let l = m.local(L);
    let (pc, cen_out, cen_in, v_in, v_out, outp, argslot, mcopy, minv) = (l, l + 0x204, l + 0x210, l + 0x21c, l + 0x228, l + 0x234, l + 0x238, l + 0x23c, l + 0x27c);
    o.call(m, MODEL_COMPUTE_BOUNDING, &[def]);
    let half = F80::from_f32(0.5);
    for (k, (a, b)) in [(0x1c, 0x10), (0x20, 0x14), (0x24, 0x18)].into_iter().enumerate() {
        let s = F80::from_f32(f32::from_bits(m.u32(def + a))) + F80::from_f32(f32::from_bits(m.u32(def + b)));
        m.set_u32(cen_in + 4 * k as u32, (s * half).to_f32().to_bits());
    }
    o.call(m, VECTOR_X_MATRIX, &[cen_out, cen_in, mat]);
    m.copy(pc, patch, 0x204);
    let (kind_u, kind_v) = (m.u32(pc + 4), m.u32(pc + 8));
    let (nu, nv) = (m.u16(pc + 0xc) as i32, m.u16(pc + 0xe) as i32);
    let rem_u = if kind_u == 1 && m.u16(pc + 0x10) == 0 { (nu + 2) % 3 } else { 0 };
    let rem_v = if kind_v == 1 && m.u16(pc + 0x12) == 0 { (nv + 2) % 3 } else { 0 };
    let bad = rem_u != 0 || rem_v != 0;
    if bad {
        o.call(m, SWARNING, &[textp::WARN_POINTS, nu as u32, nv as u32, rem_u as u32, rem_v as u32]);
        m.set_u16(pc + 0xc, (nu - rem_u) as u16);
        m.set_u16(pc + 0xe, (nv - rem_v) as u16);
        let (nu2, nv2) = (m.u16(pc + 0xc) as u32, m.u16(pc + 0xe) as u32);
        let tmp = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, nu2.wrapping_mul(nv2) << 4]).eax;
        if tmp == 0 {
            o.call(m, SFATAL, &[textp::FATAL_ALLOC]);
        }
        m.set_u32(pc + 0x1a4, tmp);
        if nu2 != 0 {
            for i in 0..nu2 {
                for j in 0..nv2 {
                    let dst = tmp.wrapping_add(j.wrapping_add(i.wrapping_mul(nv2)) << 4);
                    let src_pts = m.u32(patch + 0x1a4);
                    let src = src_pts.wrapping_add((m.u16(patch + 0xe) as u32).wrapping_mul(i).wrapping_add(j) << 4);
                    m.copy(dst, src, 16);
                }
            }
        }
    }
    let q = convert_to_bezier(m, o, pc);
    let (qnu, qnv) = (m.u16(q + 0xc) as u32, m.u16(q + 0xe) as u32);
    let npts = qnv.wrapping_mul(qnu);
    if npts > m.u32(MB_NB_MAX_VERT) {
        let hi = ((npts as u64 * 0xd1b7_1759u64) >> 32) as u32 >> 12;
        let cap = hi.wrapping_mul(0x1388).wrapping_add(0x1388);
        let p = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(SI_GBL_VERT), cap.wrapping_mul(0x4c)]).eax;
        m.set_u32(SI_GBL_VERT, p);
        m.set_u32(MB_NB_MAX_VERT, cap);
    }
    if motion != 0 {
        if m.u8(def + 0x3ee) & 1 != 0 {
            inst = true;
            motion = 0;
        }
        if m.u32(def + 0x33c) == 0 {
            let a = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 0x430]).eax;
            m.set_u32(def + 0x33c, a);
            m.set_u32(a.wrapping_add(0x22c), 0);
        }
        if motion != 0 {
            let rec = m.u32(def + 0x33c);
            if rec != 0 {
                let new = if m.u32(rec + 0x22c) == 0 {
                    Some(o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, npts.wrapping_mul(0x4c)]).eax)
                } else if m.u32(rec + 0x228) < npts {
                    Some(o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, m.u32(rec + 0x22c), npts.wrapping_mul(0x4c)]).eax)
                } else {
                    None
                };
                if let Some(new) = new {
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x22c, new);
                    let rec = m.u32(def + 0x33c);
                    m.set_u32(rec + 0x228, npts);
                }
            }
        }
    }
    m.set_i32(g::NVERT, 0);
    if no_out == 0 || motion != 0 {
        let mut j = qnv as i32 - 1;
        while j >= 0 {
            let next = j - 1;
            if qnu != 0 {
                let mut i: u32 = 0;
                loop {
                    let pts = m.u32(q + 0x1a4);
                    let src = pts.wrapping_add((qnv.wrapping_mul(i)) << 4).wrapping_add((j as u32) << 4);
                    for k in 0..3 {
                        m.set_u32(v_in + 4 * k, quiet(m.u32(src + 4 * k)));
                    }
                    if motion != 0 {
                        o.call(m, VECTOR_X_MATRIX, &[v_out, v_in, mat]);
                        let gv = m.u32(SI_GBL_VERT).wrapping_add((m.i32(g::NVERT) as u32).wrapping_mul(0x4c));
                        for k in 0..3 {
                            m.set_u32(gv + 8 + 4 * k, quiet(m.u32(v_out + 4 * k)));
                        }
                    }
                    if no_out == 0 {
                        let (x, y, z) = (quiet(m.u32(v_in)), quiet(m.u32(v_in + 4)), quiet(m.u32(v_in + 8)));
                        cvec(m, o, x, y, z);
                    }
                    m.set_i32(g::NVERT, m.i32(g::NVERT).wrapping_add(1));
                    i += 1;
                    if i >= qnu {
                        break;
                    }
                }
            }
            j = next;
        }
    }
    let cnt = m.i32(g::NVERT) as u32;
    if no_out == 0 {
        fprintf(m, o, &[textp::NL]);
        print_patch_tcoord(m, o, dict, elem);
        let mut moves: i32 = 0;
        if motion != 0 || inst {
            o.call(m, MATRIX_COPY, &[mat, mcopy]);
            for k in 0..3 {
                m.set_u32(mcopy + 0x30 + 4 * k, 0);
            }
            o.call(m, INVERT_MATRIX, &[minv, mcopy]);
        }
        if motion != 0 {
            for b in 0..cnt {
                let r = o.call(m, MI2_MOTION_VECTOR, &[def, cen_out, minv, b]).eax;
                moves = moves.wrapping_add(r as i32);
            }
        }
        if inst {
            moves = o.call(m, MI2_MOTION_VECTOR, &[def, cen_out, minv, 0]).eax as i32;
        }
        let cid_mat = m.u32(g::CID_MATERIALS);
        let matel = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, outp, 0]).eax;
        let textured = matel != 0 && m.u32(matel + 0xdc8) != 0x10001;
        if textured {
            mi2_material_vectors(m, o, matel);
        }
        let mut b: i32 = 0;
        while b < m.i32(g::NVERT) {
            let sep = if b & 3 == 0 { textp::NL_TABS2 } else { textp::TAB };
            fprintf(m, o, &[textp::V_D, sep, b as u32]);
            if moves != 0 && (b as u32) < cnt {
                if motion != 0 {
                    fprintf(m, o, &[textp::M_D_A, (b as u32).wrapping_add(m.u32(g::NVERT))]);
                } else if inst {
                    fprintf(m, o, &[textp::M_D_B, m.u32(g::NVERT)]);
                }
            }
            b += 1;
        }
        let mut name = 0u32;
        let rec = m.u32(outp);
        if rec != 0 {
            let r2 = m.u32(rec);
            name = m.u32(r2 + 0x10);
        }
        if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 {
            name = 0;
        }
        let ext = o.call(m, MAT_NAME_EXTENSION, &[dict, elem]).eax;
        let with_name = matel != 0 && name != 0;
        if ext != 0 {
            let a5 = if owner_name != 0 { textp::DASH_A } else { textp::EMPTY_A };
            let a4 = if owner_name != 0 { owner_name } else { textp::EMPTY_B };
            let a3 = if matel != 0 { matel } else { textp::DEFAULT_A };
            let a2 = if with_name { textp::DASH_B } else { textp::EMPTY_C };
            let a1 = if with_name { name } else { textp::EMPTY_D };
            fprintf(m, o, &[textp::SURFACE_A, a1, a2, a3, a4, a5, ext]);
        } else {
            let b3 = if matel != 0 { matel } else { textp::DEFAULT_B };
            let b2 = if with_name { textp::DASH_C } else { textp::EMPTY_E };
            let b1 = if with_name { name } else { textp::EMPTY_F };
            fprintf(m, o, &[textp::SURFACE_B, b1, b2, b3]);
        }
        if ext != 0 {
            m.free(ext);
        }
        // the two knot lists
        let (ku, kv) = (m.u32(q + 4), m.u32(q + 8));
        let ucnt: i32 = if ku == 0 { qnu as i32 } else { (qnu as i32 + 2) / 3 };
        fprintf(m, o, &[textp::BEZIER_U, if ku == 0 { 1 } else { 3 }, (ucnt - 1) as u32]);
        for k in 0..ucnt {
            let sep = if k == (k / 6) * 6 { textp::NL_TABS3_U } else { textp::TAB_U };
            fprintf(m, o, &[textp::KNOT_U, sep, k as u32]);
        }
        fprintf(m, o, &[textp::NL_U]);
        let vcnt: i32 = if kv == 0 { qnv as i32 } else { (qnv as i32 + 2) / 3 };
        fprintf(m, o, &[textp::BEZIER_V, if kv == 0 { 1 } else { 3 }, (vcnt - 1) as u32]);
        for k in 0..vcnt {
            let sep = if k == (k / 6) * 6 { textp::NL_TABS3_V } else { textp::TAB_V };
            fprintf(m, o, &[textp::KNOT_V, sep, k as u32]);
        }
        fprintf(m, o, &[textp::NL_V]);
        // the index grid, last row and column first
        let mut n: i32 = 0;
        let mut k = qnv as i32 - 1;
        while k >= 0 {
            let next = k - 1;
            let mut e = qnu as i32 - 1;
            while e >= 0 {
                let idx = (qnu as i32).wrapping_mul(k).wrapping_add(e);
                let sep = if n == (n / 12) * 12 { textp::NL_TABS3_I } else { textp::SPACE_I };
                fprintf(m, o, &[textp::IDX_D, sep, idx as u32]);
                n += 1;
                e -= 1;
            }
            k = next;
        }
        fprintf(m, o, &[textp::NL_END]);
        print_patch_tcoord_ref(m, o, dict, elem, ucnt, vcnt, (qnu as i32).wrapping_mul(qnv as i32));
        if textured {
            fprintf(m, o, &[textp::TEXTURE_A]);
            let u = dw(ucnt as f64 - 1.0);
            let v = dw(vcnt as f64 - 1.0);
            fprintf(m, o, &[textp::BEZ1_A, 0, 0, u[0], u[1]]);
            fprintf(m, o, &[textp::BEZ1_B, 0, 0, v[0], v[1]]);
            let l2 = m.u32(g::NVERT).wrapping_sub(2);
            fprintf(m, o, &[textp::QUAD_A, l2, l2, l2, l2]);
            fprintf(m, o, &[textp::TEXTURE_B]);
            fprintf(m, o, &[textp::BEZ1_C, 0, 0, u[0], u[1]]);
            fprintf(m, o, &[textp::BEZ1_D, 0, 0, v[0], v[1]]);
            let l1 = m.u32(g::NVERT).wrapping_sub(1);
            fprintf(m, o, &[textp::QUAD_B, l1, l1, l1, l1]);
        }
        // the approximation selected by the original patch
        let k3 = F80::from_f64(3.0);
        let div3 = |x: u16, third: bool| -> f64 {
            let v = F80::from_i32(x as i32);
            (if third { v / k3 } else { v }).to_f64()
        };
        let sel = m.u16(patch + 0x200);
        match sel {
            1 => {
                let a = div3(m.u16(q + 0x1e), kv != 0);
                let b = div3(m.u16(q + 0x1c), ku != 0);
                let (b, a) = (dw(b), dw(a));
                fprintf(m, o, &[textp::APPROX_PARAM, b[0], b[1], a[0], a[1]]);
            }
            2 => {
                let val = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1ec))).to_f64());
                let view = if m.u16(patch + 0x1e8) != 0 { textp::VIEW_A } else { textp::NO_VIEW_A };
                fprintf(m, o, &[textp::APPROX_SPATIAL, view, val[0], val[1]]);
                if m.u16(patch + 0x1fc) != 0 {
                    fprintf(m, o, &[textp::SIZES_A, m.u16(patch + 0x1f8) as u32, m.u16(patch + 0x1fa) as u32]);
                }
                fprintf(m, o, &[textp::END_A]);
            }
            3 => {
                let f1 = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1f0))).to_f64());
                let f2 = dw(F80::from_f32(f32::from_bits(m.u32(patch + 0x1f4))).to_f64());
                let view = if m.u16(patch + 0x1e8) != 0 { textp::VIEW_B } else { textp::NO_VIEW_B };
                fprintf(m, o, &[textp::APPROX_CURV, view, f1[0], f1[1], f2[0], f2[1]]);
                if m.u16(patch + 0x1fc) != 0 {
                    fprintf(m, o, &[textp::SIZES_B, m.u16(patch + 0x1f8) as u32, m.u16(patch + 0x1fa) as u32]);
                }
                fprintf(m, o, &[textp::END_B]);
            }
            _ => {}
        }
        // the trim approximation: the larger step over (3 or 1) + (3 or 1)
        let mut den = if ku == 0 { F80::from_i32(1) } else { k3 };
        let big = m.u16(q + 0x1c).max(m.u16(q + 0x1e));
        den = if kv == 0 { den + F80::from_i32(1) } else { den + k3 };
        let t = (F80::from_i32(big as i32) / den + F80::from_f64(1e-6f64)).to_f64();
        let t = dw(t);
        fprintf(m, o, &[textp::APPROX_TRIM, t[0], t[1]]);
        if q != patch && q != pc {
            let pts = m.u32(q + 0x1a4);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, pts]);
            m.set_u32(q + 0x1a4, 0);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, q]);
        }
        if bad {
            let pts = m.u32(pc + 0x1a4);
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, pts]);
            m.set_u32(pc + 0x1a4, 0);
        }
    }
    if motion != 0 && m.u32(def + 0x33c) != 0 {
        let gv = m.u32(SI_GBL_VERT);
        let mut b: u32 = 0;
        if b < cnt {
            loop {
                let rec = m.u32(def + 0x33c);
                let dst = m.u32(rec + 0x22c);
                let off = b.wrapping_mul(0x4c);
                for k in 0..3 {
                    m.set_u32(dst.wrapping_add(off).wrapping_add(8 + 4 * k), quiet(m.u32(gv.wrapping_add(off).wrapping_add(8 + 4 * k))));
                }
                b += 1;
                if b >= cnt {
                    break;
                }
            }
        }
    }
    if inst {
        let rec = m.u32(def + 0x33c);
        if rec != 0 {
            for k in 0..3 {
                let rec = m.u32(def + 0x33c);
                m.set_u32(rec + 0x158 + 4 * k, quiet(m.u32(cen_out + 4 * k)));
            }
        }
    }
    if flip {
        m.set_u32(argslot, patch);
        o.call(m, PATCH_DISPOSE, &[argslot]);
        let _ = saved;
    }
    m.release(L);
}

/// The patch of an element written as a surface for mental ray 2 [soft 0x0858b914 mi2_print_patch_geometry].
pub fn mi2_print_patch_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, no_out: i16, motion: i16) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let patch = m.u32(def + 0x48);
    if no_out == 0 {
        fprintf(m, o, &[textp::BASIS3]);
        fprintf(m, o, &[textp::BASIS1]);
        fprintf(m, o, &[textp::GROUP]);
    }
    mi2_print_patch(m, o, dict, elem, patch, mat, no_out, motion);
    if no_out == 0 {
        fprintf(m, o, &[textp::END_GROUP]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::{Ret, Trace};

    fn world() -> (Mem, Trace) {
        let o = Trace::with(|m, r, a| match r.addr {
            0x0859_9610 => Ret::eax(m.malloc(a[1])),
            _ => Ret::NONE,
        });
        (Mem::new(), o)
    }

    /// A patch of `nu` by `nv` points, kinds `uk` and `vk`, closed flags `uc`, `vc`.
    fn patch(m: &mut Mem, uk: i32, vk: i32, nu: u16, nv: u16, uc: u16, vc: u16) -> u32 {
        let p = m.malloc(PATCH_SIZE);
        let pts = m.malloc(nu as u32 * nv as u32 * 16);
        m.set_i32(p + 4, uk);
        m.set_i32(p + 8, vk);
        m.set_u16(p + 0xc, nu);
        m.set_u16(p + 0xe, nv);
        m.set_u16(p + 0x10, uc);
        m.set_u16(p + 0x12, vc);
        m.set_u32(p + 0x1a4, pts);
        for i in 0..nu as u32 * nv as u32 * 4 {
            m.set_f32(pts + 4 * i, i as f32);
        }
        p
    }

    #[test]
    fn close_patch_wraps_the_points_and_opens_the_flags() {
        let (mut m, mut o) = world();
        let p = patch(&mut m, 0, 0, 5, 3, 2, 0);
        let n = close_patch(&mut m, &mut o, p);
        assert_eq!((m.u16(n + 0xc), m.u16(n + 0xe)), (6, 3));
        assert_eq!((m.u16(n + 0x10), m.u16(n + 0x12)), (0, 0));
        // the sixth row is the first again
        let pts = m.u32(n + 0x1a4);
        assert_eq!(m.f32(pts + 5 * 3 * 16), 0.0);
        assert_eq!(m.f32(pts + 3 * 16 + 4), 13.0);
    }

    #[test]
    fn bezier_conversion_of_a_bspline_row_keeps_the_end_rows_count() {
        let (mut m, mut o) = world();
        let p = patch(&mut m, 2, 1, 5, 4, 0, 0);
        for i in 0..16 {
            m.set_f32(BSPLINE_MAT + 4 * i, if i % 5 == 0 { 1.0 } else { 0.0 });
        }
        let n = convert_bez_u(&mut m, &mut o, p, BSPLINE_MAT);
        assert_eq!(m.u16(n + 0xc), 7);
        assert_eq!(m.i32(n + 4), 1);
    }
}

/// More callees of the patch printers (`mi_patch.c`, `model` package).
pub const PATCH_COPY: Routine = Routine::new("SI_patchCopy", 0x0847_a2b8);
pub const PATCH_INVERSE: Routine = Routine::new("SI_patchInverse", 0x0847_af64);
pub const PATCH_DISPOSE: Routine = Routine::new("SI_patchDispose", 0x0847_a530);
/// `SI_patchSurface (0x0847f89c)`: `(patch, &vecs, &nu, &nv)`; the vectors are `malloc`ed.
pub const PATCH_SURFACE: Routine = Routine::new("SI_patchSurface", 0x0847_f89c);

/// A patch's texture coordinates written (`print_patch_tcoord`)
/// [soft 0x085888c0 print_patch_tcoord]; returns the number of vectors written.
///
/// The same as [`print_nurb_tcoord`] for a patch: the handedness decides whether the
/// patch (slot at +0x38, the second when the kind at +0x34 is not 1) is replaced by an
/// inverted copy meanwhile; `SI_patchSurface` gives the vectors and the two counts (16-bit);
/// there is no `# texs` line; the vectors are freed with `free`.
pub fn print_patch_tcoord(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> i32 {
    if dict == 0 || elem == 0 {
        return 0;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return 0;
    }
    let tf = m.u32(def + 0x44);
    let base = if m.i32(GLB_MANIP) == 1 { 0x3a0 } else { 0x16c };
    let f = |m: &Mem, off: u32| f32::from_bits(m.u32(tf + base + off)) as f64;
    let prod = f(m, 0) * f(m, 4) * f(m, 8);
    let right = m.i16(RIGHT_HAND) != 0;
    let mut flip = if prod < 0.0 { right } else { !right };
    if m.u32(def + 8) & 8 != 0 {
        flip = !flip;
    }
    let slot = |m: &Mem| m.u32(def + 0x38).wrapping_add(if m.i16(def + 0x34) == 1 { 0 } else { 4 });
    let mut saved = 0;
    if flip {
        saved = m.u32(slot(m));
        let copy = o.call(m, PATCH_COPY, &[saved]).eax;
        let at = slot(m);
        m.set_u32(at, copy);
        let cur = m.u32(slot(m));
        o.call(m, PATCH_INVERSE, &[cur, 0]);
    }
    let patch = m.u32(slot(m));
    let l = m.local(16);
    let (mat, idx, vecs, nunv) = (l, l + 4, l + 8, l + 12);
    o.call(m, PATCH_SURFACE, &[patch, vecs, nunv, nunv + 2]);
    m.set_u16(g::TEX_NU, m.u16(nunv));
    m.set_u16(g::TEX_NV, m.u16(nunv + 2));
    let cid_mat = m.u32(g::CID_MATERIALS);
    o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, mat, 0]);
    let mut count: i32 = 0;
    let cid3 = m.u32(g::CID_TEXTURES3D);
    let mut has3d = o.call(m, RELATION_CHECK, &[dict, m.u32(mat), 1, 0x1f5, cid3]).eax as u16 != 0;
    if !has3d {
        let cid3 = m.u32(g::CID_TEXTURES3D);
        has3d = o.call(m, RELATION_CHECK, &[dict, elem, 1, 0x1f4, cid3]).eax as u16 != 0;
    }
    if has3d {
        count = print_patch_texture3d(m, o, m.i16(nunv) as i32, m.i16(nunv + 2) as i32, m.u32(vecs));
    }
    for (owner_is_mat, relation) in [(true, 0x191u32), (false, 0x190)] {
        let owner = if owner_is_mat { m.u32(mat) } else { elem };
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        while t != 0 {
            o.call(m, UPDATE_TXT_SUPPORT, &[def, t]);
            let n = print_patch_texture(m, o, t, m.i16(nunv) as i32, m.i16(nunv + 2) as i32, m.u32(vecs));
            count = count.wrapping_add(n);
            let cid2 = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        }
    }
    if m.u32(vecs) != 0 {
        m.free(m.u32(vecs));
        m.set_u32(vecs, 0);
    }
    m.release(16);
    if flip {
        let at = slot(m);
        o.call(m, PATCH_DISPOSE, &[at]);
        let at = slot(m);
        m.set_u32(at, saved);
    }
    count
}

/// The texts of `print_patch_texture_ref` (`.rodata`).
pub mod text3 {
    pub const VOLUME: u32 = 0x086f_1eb0; // "volume "
    pub const EMPTY: u32 = 0x086f_1eb8; // ""
    pub const STEXTURE: u32 = 0x086f_1eb9; // "\n\t\t%stexture"
    pub const BEZIER1_FF_A: u32 = 0x086f_1ec6;
    pub const BEZIER1_FF_B: u32 = 0x086f_1ed9;
    pub const DDDD: u32 = 0x086f_1eed; // "\t\t\t%d %d %d %d\n"
    pub const BEZIER1_U: u32 = 0x086f_1efd;
    pub const SPACE_U: u32 = 0x086f_1f0a;
    pub const LINE_U: u32 = 0x086f_1f0c;
    pub const SEP_F_U: u32 = 0x086f_1f12;
    pub const NL_U: u32 = 0x086f_1f17;
    pub const BEZIER1_V: u32 = 0x086f_1f19;
    pub const SPACE_V: u32 = 0x086f_1f26;
    pub const LINE_V: u32 = 0x086f_1f28;
    pub const SEP_F_V: u32 = 0x086f_1f2e;
    pub const NL_V: u32 = 0x086f_1f33; // "\n\n"
    pub const SPACE_I: u32 = 0x086f_1f36;
    pub const LINE_I: u32 = 0x086f_1f38; // "\n\t\t\t"
    pub const SEP_D: u32 = 0x086f_1f3d;
    pub const NL_END: u32 = 0x086f_1f42;
}

/// A patch texture's reference written (`print_patch_texture_ref`)
/// [soft 0x085891cc print_patch_texture_ref].
///
/// `kind` (7 and 8 print nothing; 1 to 3 are volume textures), the parameter ranges
/// `(u0, v0)` to `(u1, v1)` (the four `double`s of the original, in the order `u0, v0, u1,
/// v1`), and whether the patch is closed in u and in v. Kind 4 writes the two ranges and
/// four consecutive vertex numbers; any other kind writes both knot samplings (the grid
/// counts `tex_nu` and `tex_nv`, one more when closed) and the grid of vertex numbers
/// (ten to a line, wrapping at the counts), and advances `ivert` by `tex_nu * tex_nv`.
/// The original takes one more argument (a `short`) it never reads.
pub fn print_patch_texture_ref(m: &mut Mem, o: &mut dyn Outside, kind: i32, u0: f64, v0: f64, u1: f64, v1: f64, closed_u: i16, closed_v: i16) {
    if (kind.wrapping_sub(7) as u32) <= 1 {
        return;
    }
    let sel = if (kind.wrapping_sub(1) as u32) <= 2 { text3::VOLUME } else { text3::EMPTY };
    fprintf(m, o, &[text3::STEXTURE, sel]);
    if kind == 4 {
        let (a, c) = (dw(u0), dw(u1));
        fprintf(m, o, &[text3::BEZIER1_FF_A, a[0], a[1], c[0], c[1]]);
        let (b, d) = (dw(v0), dw(v1));
        fprintf(m, o, &[text3::BEZIER1_FF_B, b[0], b[1], d[0], d[1]]);
        let iv = m.i32(g::IVERT);
        fprintf(m, o, &[text3::DDDD, iv as u32, iv.wrapping_add(1) as u32, iv.wrapping_add(2) as u32, iv.wrapping_add(3) as u32]);
        m.set_i32(g::IVERT, iv.wrapping_add(4));
        return;
    }
    // one direction's samples: ((i * (hi - lo)) / (n - 1)) + lo, all in the 80-bit register
    let samples = |m: &mut Mem, o: &mut dyn Outside, n: i32, lo: f64, hi: f64, head: u32, space: u32, line: u32, sep_f: u32| {
        fprintf(m, o, &[head]);
        if 0 < n {
            let span = (F80::from_f64(hi) - F80::from_f64(lo)).to_f64();
            let den = (n - 1) as f64;
            for i in 0..n {
                let v = ((F80::from_i32(i) * F80::from_f64(span)) / F80::from_f64(den) + F80::from_f64(lo)).to_f64();
                let sep = if i & 3 != 0 { space } else { line };
                let w = dw(v);
                fprintf(m, o, &[sep_f, sep, w[0], w[1]]);
            }
        }
    };
    let nu = (m.i16(g::TEX_NU) as i32).wrapping_add(if closed_u != 0 { 1 } else { 0 });
    samples(m, o, nu, u0, u1, text3::BEZIER1_U, text3::SPACE_U, text3::LINE_U, text3::SEP_F_U);
    fprintf(m, o, &[text3::NL_U]);
    let nv = (m.i16(g::TEX_NV) as i32).wrapping_add(if closed_v != 0 { 1 } else { 0 });
    samples(m, o, nv, v0, v1, text3::BEZIER1_V, text3::SPACE_V, text3::LINE_V, text3::SEP_F_V);
    fprintf(m, o, &[text3::NL_V]);
    let mut j: i32 = 0;
    loop {
        let mut count: i32 = 0;
        let tv = m.i16(g::TEX_NV) as i32;
        if j >= tv.wrapping_add(if closed_v != 0 { 1 } else { 0 }) {
            break;
        }
        let mut i: i32 = if closed_u != 0 { -1 } else { 0 };
        let next = j + 1;
        while i < m.i16(g::TEX_NU) as i32 {
            let (tu, tv) = (m.i16(g::TEX_NU) as i32, m.i16(g::TEX_NV) as i32);
            let row = tv.wrapping_add(j) % tv;
            let col = tu.wrapping_add(i) % tu;
            let at = row.wrapping_mul(tu).wrapping_add(m.i32(g::IVERT)).wrapping_add(col);
            let sep = if count == (count / 10).wrapping_mul(10) { text3::LINE_I } else { text3::SPACE_I };
            fprintf(m, o, &[text3::SEP_D, sep, at as u32]);
            i += 1;
            count += 1;
        }
        j = next;
    }
    let (tu, tv) = (m.i16(g::TEX_NU) as i32, m.i16(g::TEX_NV) as i32);
    m.set_i32(g::IVERT, m.i32(g::IVERT).wrapping_add(tu.wrapping_mul(tv)));
    fprintf(m, o, &[text3::NL_END]);
}

/// A patch's texture references written (`print_patch_tcoord_ref`)
/// [soft 0x08588e0c print_patch_tcoord_ref].
///
/// Nothing for a null dictionary or element or an element without definition. Else `ivert` is
/// set to `first_vertex` and, as for a NURBS surface, the material's 3D texture (relation
/// 0x1f5, else the element's 0x1f4) is written with kind 0x29a and ranges 0 to `(cu - 1,
/// cv - 1)`, then every 2D texture of the material (0x191) and of the element (400) with
/// the kind in its record at +0x2a. The patch is the definition's field +0x48; its closed
/// flags (+0x10, +0x12) are passed on.
pub fn print_patch_tcoord_ref(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, cu: i32, cv: i32, first_vertex: i32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let def = m.u32(elem + 4);
    if def == 0 {
        return;
    }
    let patch = m.u32(def + 0x48);
    let l = m.local(8);
    let (mat, idx) = (l, l + 4);
    let cid_mat = m.u32(g::CID_MATERIALS);
    o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 300, cid_mat, mat, 0]);
    m.set_i32(g::IVERT, first_vertex);
    let (hu, hv) = ((cu as f64) - 1.0, (cv as f64) - 1.0);
    let cid3 = m.u32(g::CID_TEXTURES3D);
    let mut t = o.call(m, ELEMDICT_FIRST, &[dict, m.u32(mat), 1, 3, 0x1f5, cid3, 0, 0]).eax;
    if t == 0 {
        let cid3 = m.u32(g::CID_TEXTURES3D);
        t = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x1f4, cid3, 0, 0]).eax;
    }
    if t != 0 {
        print_patch_texture_ref(m, o, 0x29a, 0.0, 0.0, hu, hv, m.i16(patch + 0x10), m.i16(patch + 0x12));
    }
    for (owner_is_mat, relation) in [(true, 0x191u32), (false, 0x190)] {
        let owner = if owner_is_mat { m.u32(mat) } else { elem };
        let cid2 = m.u32(g::CID_TEXTURES2D);
        let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        while t != 0 {
            print_patch_texture_ref(m, o, m.i16(t + 0x2a) as i32, 0.0, 0.0, hu, hv, m.i16(patch + 0x10), m.i16(patch + 0x12));
            let cid2 = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, relation, cid2, 0, idx]).eax;
        }
    }
    m.release(8);
}
