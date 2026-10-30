// SPDX-License-Identifier: MIT
//! `mental-checkkit <directory of vector files>`: the checks of package 63 on `si-checkkit`.

use si_checkkit::Kit;
use si_mental::mi_patch as p;
use si_mental::mi_poly as q;

fn main() {
    Kit::new(&[
        ("print_nurbs_curve", |m, o, a| { p::print_nurbs_curve(m, o, a.v("dir")); ().into() }),
        ("print_nurbs_texture_ref", |m, o, a| { p::print_nurbs_texture_ref(m, o, a.v("srf"), a.i("kind")); ().into() }),
        ("print_nurb_tcoord_ref", |m, o, a| { p::print_nurb_tcoord_ref(m, o, a.v("dict"), a.v("elem"), a.i("first")); ().into() }),
        ("print_patch_texture", |m, o, a| p::print_patch_texture(m, o, a.v("map"), a.i("nu"), a.i("nv"), a.v("vecs")).into()),
        ("SI_nurbSurface", |m, o, a| { p::si_nurb_surface(m, o, a.v("srf"), a.i("per_u"), a.i("per_v"), a.v("out"), a.v("nu"), a.v("nv")); ().into() }),
        ("print_patch_texture3d", |m, o, a| p::print_patch_texture3d(m, o, a.i("nu"), a.i("nv"), a.v("vecs")).into()),
        ("close_patch", |m, o, a| p::close_patch(m, o, a.v("patch")).into()),
        ("convert_bez_u", |m, o, a| p::convert_bez_u(m, o, a.v("patch"), a.v("mat")).into()),
        ("convert_bez_v", |m, o, a| p::convert_bez_v(m, o, a.v("patch"), a.v("mat")).into()),
        ("convert_to_bezier", |m, o, a| p::convert_to_bezier(m, o, a.v("patch")).into()),
        ("print_patch_tcoord", |m, o, a| p::print_patch_tcoord(m, o, a.v("dict"), a.v("elem")).into()),
        ("print_patch_texture_ref", |m, o, a| {
            let d = |n: &str| a.case().input(&format!("v_{n}")).f64();
            p::print_patch_texture_ref(m, o, a.i("kind"), d("u0"), d("v0"), d("u1"), d("v1"), a.s("fu"), a.s("fv"));
            ().into()
        }),
        ("print_patch", |m, o, a| { p::print_patch(m, o, a.v("dict"), a.v("elem"), a.v("patch"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("print_patch_geometry", |m, o, a| { p::print_patch_geometry(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("mi2_print_nurbs_geometry", |m, o, a| { p::mi2_print_nurbs_geometry(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("mi2_print_patch", |m, o, a| { p::mi2_print_patch(m, o, a.v("dict"), a.v("elem"), a.v("patch"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("mi2_print_patch_geometry", |m, o, a| { p::mi2_print_patch_geometry(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("print_compressed_vector", |m, o, a| { q::print_compressed_vector(m, o, a.v("a"), a.v("b"), a.v("c")); ().into() }),
        ("calculate_motion_vector", |m, _o, a| { q::calculate_motion_vector(m, a.v("model"), a.v("point"), a.v("index"), a.v("out")); ().into() }),
        ("motion_vector", |m, o, a| q::motion_vector(m, o, a.v("model"), a.v("point"), a.v("index")).into()),
        ("mi2_motion_vector", |m, o, a| q::mi2_motion_vector(m, o, a.v("model"), a.v("point"), a.v("inverse"), a.v("index")).into()),
        ("texture3d_point", |m, o, a| q::texture3d_point(m, o, a.v("tex"), a.v("point")).into()),
        ("calc_poly_textverts", |m, o, a| { q::calc_poly_textverts(m, o, a.v("dict"), a.v("elem"), a.v("tex"), a.v("poly")); ().into() }),
        ("calc_tri_textverts", |m, o, a| { q::calc_tri_textverts(m, o, a.v("dict"), a.v("elem"), a.v("tex"), a.v("tri")); ().into() }),
        ("texture2d_point", |m, o, a| q::texture2d_point(m, o, a.v("dict"), a.v("tex"), a.v("pos"), a.v("rvtx"), a.v("node"), a.v("index"), a.v("elem")).into()),
        ("prepare_tcoords_array", |m, o, a| q::prepare_tcoords_array(m, o, a.v("dict"), a.v("elem"), a.v("tex"), a.i("n")).into()),
        ("print_nurbs_geometry", |m, o, a| { p::print_nurbs_geometry(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.s("no_out"), a.s("motion")); ().into() }),
        ("print_patch_tcoord_ref", |m, o, a| { p::print_patch_tcoord_ref(m, o, a.v("dict"), a.v("elem"), a.i("cu"), a.i("cv"), a.i("first")); ().into() }),
        ("print_mesh_geometry", |m, o, a| { q::print_mesh_geometry(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.i("shadow"), a.s("no_out"), a.s("motion")); ().into() }),
        ("print_triangles", |m, o, a| { q::print_triangles(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.i("unused"), a.s("no_out"), a.s("motion")); ().into() }),
        ("print_nurb_tcoord", |m, o, a| p::print_nurb_tcoord(m, o, a.v("dict"), a.v("elem")).into()),
    ])
    .route(route)
    .effect(effect)
    .run()
}

/// The ported callees the original runs for real in the emulator: the crate's Rust answers them.
fn route(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> Option<si_mem::Ret> {
    match r.addr {
        // SI_SampleKnotVector(dir, per, &array, &count)
        0x084d_727c => {
            si_nurbs::image::sample_knot_vector(m, a[0], a[1] as i32, a[2], a[3]).ok()?;
            Some(si_mem::Ret::NONE)
        }
        // SI_memtrackAllocate(params, n): a block of the harness's heap, null for n = 0 (the recorded call is
        // used up without its arguments being held; the block's size is held with the heap)
        0x0859_9610 => Some(si_mem::Ret::eax(if a[1] != 0 { m.malloc(a[1]) } else { 0 })),
        // SI_memtrackRealloc(params, p, n): the harness's realloc, at most 24 KB of the block made
        0x0859_9770 => Some(si_mem::Ret::eax(m.realloc(a[1], a[2].min(24576)))),
        // SI_meshCopy(mesh): a made copy of the 0x60 bytes of the mesh header
        0x0844_04e0 => {
            let p = m.malloc(0x60);
            for k in 0..0x18u32 {
                let w = m.u32(a[0] + 4 * k);
                m.set_u32(p + 4 * k, w);
            }
            Some(si_mem::Ret::eax(p))
        }
        // si_getMatNameExtension(dict, elem): a made string when bit 0 of the element's kind is set
        0x0859_5024 => {
            if m.u16(a[1] + 0x2a) & 1 != 0 {
                let q = m.malloc(8);
                m.set_u32(q, u32::from_le_bytes(*b"ab\0\0"));
                Some(si_mem::Ret::eax(q))
            } else {
                Some(si_mem::Ret::eax(0))
            }
        }
        _ => None,
    }
}

/// What the check's stand-ins do to the image (the same as `re/tools/si-mental/gen.py`'s `CUSTOM`).
fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], _answer: u32) -> Result<(), String> {
    match r.addr {
        0x0829_b584 => {
            // SI_txtProjectionInfo(model, lclmap, &bound, &width, &center[3]): center = (1, 1.5, 2)
            for (k, v) in [1.0f32, 1.5, 2.0].into_iter().enumerate() {
                m.set_f32(a[4] + 4 * k as u32, v);
            }
        }
        0x082b_3090 => {
            // SI_mapTcoordCompute(lclmap, x, y, z, &out[2], ...): out = (x, y * 0.5 + z)
            let f = |w: u32| f32::from_bits(w) as f64;
            m.set_f32(a[4], f(a[1]) as f32);
            m.set_f32(a[4] + 4, (f(a[2]) * 0.5 + f(a[3])) as f32);
        }
        0x0845_3668 => {
            // SI_meshGetTxtUVDomain(model, id, &domain): domain = id & 3
            m.set_u32(a[2], a[1] & 3);
        }
        0x0845_34d8 | 0x0826_d510 => {
            // SI_nodeGetUV(node, domain, &u, &v) / SI_rvtxGetUV(vertex, ...): u = w[0] + domain * 0.25, v = w[1]
            let off = if r.addr == 0x0845_34d8 { 4 } else { 0x30 };
            let (u, v) = (f32::from_bits(m.u32(a[0] + off)) as f64, f32::from_bits(m.u32(a[0] + off + 4)) as f64);
            m.set_f32(a[2], (u + a[1] as f64 * 0.25) as f32);
            m.set_f32(a[3], v as f32);
        }
        0x0859_44c0 => {
            // textureVert(vec, &out[2], map): out = (vec.x, vec.y)
            let (x, y) = (m.u32(a[0]), m.u32(a[0] + 4));
            m.set_u32(a[1], x);
            m.set_u32(a[1] + 4, y);
        }
        0x084d_6728 => {
            // SI_nurbsSrfValue(&out[3], srf, u, v): (2u+v, u-3v, u*v)
            let u = f64::from_bits(a[2] as u64 | (a[3] as u64) << 32);
            let v = f64::from_bits(a[4] as u64 | (a[5] as u64) << 32);
            m.set_f64(a[0], 2.0 * u + v);
            m.set_f64(a[0] + 8, u - 3.0 * v);
            m.set_f64(a[0] + 16, u * v);
        }
        0x0847_f89c => {
            // SI_patchSurface(patch, &vecs, &nu, &nv): the patch's counts, nu*nv vectors of (i*0.37 - 3)
            let (nu, nv) = (m.u16(a[0] + 0xc), m.u16(a[0] + 0xe));
            let n = nu as u32 * nv as u32;
            let ptr = if n != 0 { m.malloc(n * 12) } else { 0 };
            for i in 0..n * 3 {
                m.set_f32(ptr + 4 * i, (i as f64 * 0.37 - 3.0) as f32);
            }
            m.set_u32(a[1], ptr);
            m.set_i16(a[2], nu as i16);
            m.set_i16(a[3], nv as i16);
        }
        0x0813_8094 => {
            // SI_elemdictGetFirstRelatedElem(dict, owner, 1, 3, relation, cid, &out, &idx): *out = owner
            if a.len() > 6 && a[6] != 0 {
                m.set_u32(a[6], a[1]);
            }
        }
        0x0851_672c => {
            // SI_matrixInverse(src, dst): when it succeeded, dst[i] = src[i] * 0.5 + 0.125 * (i + 1)
            if _answer & 0xffff != 0 {
                for k in 0..16u32 {
                    let v = f32::from_bits(m.u32(a[0] + 4 * k)) as f64;
                    m.set_f32(a[1] + 4 * k, (v * 0.5 + 0.125 * (k + 1) as f64) as f32);
                }
            }
        }
        0x0851_66ec => {
            // SI_matrixTranspose(src, dst)
            for k in 0..16u32 {
                let v = m.u32(a[0] + 4 * ((k % 4) * 4 + k / 4));
                m.set_u32(a[1] + 4 * k, v);
            }
        }
        0x0813_8190 => {
            // SI_elemdictGetNthRelatedElem(..., &out): *out = the answer
            if a.len() > 7 && a[7] != 0 {
                m.set_u32(a[7], _answer);
            }
        }
        0x0859_8430 => {
            // vector_X_matrix(&out[3], &in[3], mat): out = in * 1.5 + (0.25, 0.5, 0.75)
            for k in 0..3u32 {
                let v = f32::from_bits(m.u32(a[1] + 4 * k)) as f64;
                m.set_f32(a[0] + 4 * k, (v * 1.5 + 0.25 * (k + 1) as f64) as f32);
            }
        }
        0x0851_6224 => {
            // SI_matrixCopy(src, dst): sixteen floats
            for k in 0..16u32 {
                let v = m.u32(a[0] + 4 * k);
                m.set_u32(a[1] + 4 * k, v);
            }
        }
        0x0859_883c => {
            // invert_matrix(out, in): out[i] = in[i] * 0.5 + 0.125 * (i + 1)
            for k in 0..16u32 {
                let v = f32::from_bits(m.u32(a[1] + 4 * k)) as f64;
                m.set_f32(a[0] + 4 * k, (v * 0.5 + 0.125 * (k + 1) as f64) as f32);
            }
        }
        0x0859_08a8 => {
            // mi2_motion_vector(def, &out[3], &inverse, index): out = (index * 0.5 + 1, 2, 3.5)
            m.set_f32(a[1], (a[3] as f64 * 0.5 + 1.0) as f32);
            m.set_f32(a[1] + 4, 2.0);
            m.set_f32(a[1] + 8, 3.5);
        }
        0x0859_0848 => {
            // motion_vector(def, &out[3], index): out = (index * 0.5 + 1, 2, 3.5)
            m.set_f32(a[1], (a[2] as f64 * 0.5 + 1.0) as f32);
            m.set_f32(a[1] + 4, 2.0);
            m.set_f32(a[1] + 8, 3.5);
        }
        _ => {}
    }
    Ok(())
}
