// SPDX-License-Identifier: MIT
//! `user.c`: the metaball library's public entry points, the thin layer over `hooks.c`, `update_voxel.c` and `bloom.c` that
//! the scene code calls (`SI_metaSysGetSurface`, `animVClayUpdate`). Records and lists live in the image at the addresses
//! of `volume.rs`; every routine answers what the callee under it answers, as the original leaves it in `eax`.
//!
//! Ported: the eleven routines the ported scene code calls. The other nine of the file (`VCReadFile`, `VCWriteFile`,
//! `VCCopyPrim`, group removal, dissolving, deleting, blending, the two lists) are called by the metaball editing
//! commands only and are left for the modeller package.

use crate::bloom::{mbto_p, VCG_TRI_COUNT, VCG_VTX_COUNT};
use crate::hooks::*;
use crate::update_voxel::rm_all_tri;
use crate::volume::*;
use si_mem::Mem;

/// `update_flag.249` (0x086ee4a4): the function-local static `VCGenPolyData` hands to `VC_mbtoP` and then sets to 1.
pub const UPDATE_FLAG_249: u32 = 0x086e_e4a4;

/// [soft 0x08575ca0 VCInit]: the library's tables, then the 512 voxel heads (12 bytes each: two words cleared, the third
/// left as `malloc` gave it). 0, 0x10 when an allocation fails (the table of voxels is then left at the null answer).
pub fn vc_init(m: &mut Mem) -> i32 {
    let r = init_orig_lists(m);
    if r != 0 {
        return r;
    }
    let p = m.malloc(0x1800);
    m.set_u32(VCG_VOX_LIST, p);
    if p == 0 {
        return 0x10;
    }
    for i in 0..0x200u32 {
        m.set_u32(p + 12 * i, 0);
        m.set_u32(p + 12 * i + 4, 0);
    }
    0
}

/// [soft 0x08575cf4 VCReInit]: the tables again, then every voxel emptied. The first nonzero answer, else 0.
pub fn vc_re_init(m: &mut Mem) -> i32 {
    let r = init_orig_lists(m);
    if r != 0 {
        return r;
    }
    rm_all_tri(m)
}

/// [soft 0x08575d14 VCRmAllTri]: every voxel emptied.
pub fn vc_rm_all_tri(m: &mut Mem) -> i32 {
    rm_all_tri(m)
}

/// [soft 0x08575dc0 VCCreatePrim]: kind 0 (a metaball) is made by `VC_createMB(matrix, handle out)`; any other kind answers 0xe.
pub fn vc_create_prim(m: &mut Mem, kind: i32, matrix: u32, out: u32) -> i32 {
    if kind == 0 {
        create_mb(m, matrix, out)
    } else {
        0xe
    }
}

/// [soft 0x08575dec VCSetPrimParam]: `VC_setMBParam(handle, which, value)`.
pub fn vc_set_prim_param(m: &mut Mem, handle: i32, which: i32, p: u32) -> i32 {
    set_mb_param(m, handle, which, p)
}

/// [soft 0x08575e0c VCGetPrimParam]: `VC_getMBParam(handle, which, where the value goes)`.
pub fn vc_get_prim_param(m: &mut Mem, handle: i32, which: i32, out: u32) -> i32 {
    get_mb_param(m, handle, which, out)
}

/// [soft 0x08575fa8 VCDeletePrim]: `VC_destroyMB(handle)`.
pub fn vc_delete_prim(m: &mut Mem, handle: i32) -> i32 {
    destroy_mb(m, handle)
}

/// [soft 0x08575fc0 VCAddToGroup]: `VC_addMBtoGroup(handle, group)`.
pub fn vc_add_to_group(m: &mut Mem, handle: i32, group: i32) -> i32 {
    add_mb_to_group(m, handle, group)
}

/// [soft 0x08576044 VCUnBlendGroups]: `VC_createMBLink(a, b)` (the name is the original's: a link stops two groups blending).
pub fn vc_unblend_groups(m: &mut Mem, a: i32, b: i32) -> i32 {
    create_mb_link(m, a, b)
}

/// [soft 0x08576060 VCInfoPrim]: answers 1 for a handle outside the table or a free slot; else the first out word is set to
/// 0 and the answer is `VC_getMBGroupID(handle, second out)`.
pub fn vc_info_prim(m: &mut Mem, handle: i32, a: u32, b: u32) -> i32 {
    if handle < 0 || m.i32(VCG_MAX_MBS) <= handle || m.i16(mb_at(m, handle) + mb::FREE) != 0 {
        return 1;
    }
    m.set_i32(a, 0);
    get_mb_group_id(m, handle, b)
}

/// [soft 0x085760e8 VCGenPolyData]: polygonises the field (`VC_mbtoP(level, 1.0, bounds, mode, box, update_flag)`) and copies
/// the result out as fresh blocks: vertices (3 doubles, or 9 when `mode` is 2; 0x48 bytes each) of those the triangles use
/// (count > 0 at +0x78), polygons (0x20 bytes: 3, a block of three renumbered indices, three doubles) of the triangles whose
/// flag at +0x24 is 0. The four out words are the vertex count, the vertices, the polygon count, the polygons. Answers
/// 0, 0x10 (an allocation failed or `VC_mbtoP` said so; the polygon blocks made so far are not freed, as in the original),
/// or 0x14 (no vertices: the out words are zero).
#[allow(clippy::too_many_arguments)]
pub fn vc_gen_poly_data(m: &mut Mem, mode: i32, level: i32, bounds: u32, boxp: u32, nvtx_out: u32, vtx_out: u32, npoly_out: u32, poly_out: u32) -> i32 {
    let flag = m.i16(UPDATE_FLAG_249);
    if mbto_p(m, level, 1.0, bounds, mode, boxp, flag) == 0x10 {
        return 0x10;
    }
    m.set_i16(UPDATE_FLAG_249, 1);
    let nv = m.i32(VCG_VTX_COUNT);
    if nv == 0 {
        for a in [nvtx_out, npoly_out, vtx_out, poly_out] {
            m.set_u32(a, 0);
        }
        return 0x14;
    }
    let map = m.malloc((nv as u32).wrapping_shl(2));
    if map == 0 {
        return 0x10;
    }
    m.set_i32(nvtx_out, 0);
    let vl = m.u32(VCG_VTX_LIST);
    for i in 0..nv.max(0) as u32 {
        if m.i32(vl + i * 0xb4 + 0x78) > 0 {
            let n = m.i32(nvtx_out);
            m.set_i32(map + 4 * i, n);
            m.set_i32(nvtx_out, n.wrapping_add(1));
        } else {
            m.set_i32(map + 4 * i, -1);
        }
    }
    m.set_i32(npoly_out, 0);
    let nt = m.i32(VCG_TRI_COUNT);
    let tl = m.u32(VCG_TRI_LIST);
    for i in 0..nt.max(0) as u32 {
        if m.i16(tl + i * 0x28 + 0x24) == 0 {
            let n = m.i32(npoly_out);
            m.set_i32(npoly_out, n.wrapping_add(1));
        }
    }
    let nvo = m.i32(nvtx_out);
    let vtx = m.malloc((nvo as u32).wrapping_mul(0x48));
    if vtx == 0 {
        m.free(map);
        return 0x10;
    }
    let npo = m.i32(npoly_out);
    let poly = m.malloc((npo as u32).wrapping_shl(5));
    if poly == 0 {
        m.free(map);
        m.free(vtx);
        return 0x10;
    }
    let nv = m.i32(VCG_VTX_COUNT);
    let vl = m.u32(VCG_VTX_LIST);
    let mut dst = vtx;
    for i in 0..nv.max(0) as u32 {
        let src = vl + i * 0xb4;
        if m.i32(src + 0x78) > 0 {
            let words = if mode == 2 { 9 } else { 3 };
            for k in 0..words {
                let v = m.u64(src + 8 * k);
                m.set_u64(dst + 8 * k, v);
            }
            dst += 0x48;
        }
    }
    let nt = m.i32(VCG_TRI_COUNT);
    let tl = m.u32(VCG_TRI_LIST);
    let mut made = 0u32;
    for i in 0..nt.max(0) as u32 {
        let t = tl + i * 0x28;
        if m.i16(t + 0x24) == 0 {
            let rec = poly + made * 0x20;
            m.set_i32(rec, 3);
            let idx = m.malloc(12);
            m.set_u32(rec + 4, idx);
            made += 1;
            for k in 0..m.i32(rec).max(0) as u32 {
                let v = m.u32(t + 4 * k);
                let r = m.u32(map + v.wrapping_mul(4));
                m.set_u32(idx + 4 * k, r);
            }
            for (to, from) in [(8u32, 0xcu32), (0x10, 0x14), (0x18, 0x1c)] {
                let v = m.u64(t + from);
                m.set_u64(rec + to, v);
            }
        }
    }
    m.free(map);
    m.set_u32(vtx_out, vtx);
    m.set_u32(poly_out, poly);
    0
}
