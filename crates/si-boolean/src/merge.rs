// SPDX-License-Identifier: MIT
//! `merge.c` (finding 0037, "bool3"): `SI_meshMerge`, the geometry of the modeller's Merge command. The dialog
//! (`displayMergeDialog`) and the command (`SI_mergePerform`) are modeller code and not ported.

use crate::boolean::*;
use crate::mesh::*;
use crate::ops::*;
use si_mem::{Mem, Outside, Routine};
use si_x87::{fmov, x};

pub const SI_REMOVE_FACETED: Routine = Routine::new("SI_removeFaceted", 0x0845_8770);
pub const SI_MAX_NB_POL_WARNING: Routine = Routine::new("SI_maxNbPolWarning", 0x0844_8afc);
pub const SI_EDGE_ALLOCATE: Routine = Routine::new("SI_edgeAllocate", 0x0844_1fb8);
/// `"Too many elements to generate a single mesh ..."` (the message for the status line).
const MSG_TOO_MANY: u32 = 0x086b_9920;

/// A float moved through the x87 as the original copies it.
fn cp32(m: &mut Mem, to: u32, from: u32) {
    let v = fmov(m.f32(from));
    m.set_f32(to, v);
}

/// Disposes the first `count` meshes of the array (`SI_meshDispose` takes the address of each pointer).
fn tail_dispose(m: &mut Mem, o: &mut dyn Outside, arr: u32, count: i32) {
    for k in 0..count.max(0) as u32 {
        call(m, o, SI_MESH_DISPOSE, &[arr + k * 4]);
    }
}

/// Copies the 0x24-byte-node polygon `src` (header 0x18 bytes at `src`) to `dst` with its contour nodes shifted:
/// vertex indices by `vo`, edge indices by `eo` (keeping the sign), material by `mo`.
fn copy_polygon(m: &mut Mem, dst: u32, src: u32, vo: i32, eo: i32, mo: i32) {
    m.copy(dst, src, 0x18);
    let mat = (mo as u16).wrapping_add(m.u16(src + 0x12));
    m.set_u16(dst + 0x12, mat);
    let cnt = m.u16(src + 0x10) as u32;
    let rounded = cnt.wrapping_sub((cnt.wrapping_sub(1) & 3).wrapping_sub(3));
    let nodes = m.si_calloc(rounded, 0x24);
    m.set_u32(dst + 0xc, nodes);
    let sn = m.u32(src + 0xc);
    for k in 0..cnt {
        let (s, d) = (sn.wrapping_add(k * 0x24), nodes.wrapping_add(k * 0x24));
        let v = m.i32(s);
        m.set_i32(d, if v == -1 { v } else { v.wrapping_add(vo) });
        let e = m.i32(s + 4);
        m.set_i32(d + 4, if e == 0 { 0 } else if e < 0 { e.wrapping_sub(eo) } else { e.wrapping_add(eo) });
        cp32(m, d + 8, s + 8);
        cp32(m, d + 0xc, s + 0xc);
        cp32(m, d + 0x10, s + 0x10);
    }
}

/// The same for a vertex (header 0x14 bytes, 0xc-byte nodes: vertex, edge, polygon).
fn copy_vertex(m: &mut Mem, dst: u32, src: u32, vo: i32, eo: i32, po: i32) {
    m.copy(dst, src, 0x14);
    let cnt = m.u16(src + 0x10) as u32;
    let rounded = cnt.wrapping_sub((cnt.wrapping_sub(1) & 3).wrapping_sub(3));
    let nodes = m.si_calloc(rounded, 0xc);
    m.set_u32(dst + 0xc, nodes);
    let sn = m.u32(src + 0xc);
    for k in 0..cnt {
        let (s, d) = (sn.wrapping_add(k * 0xc), nodes.wrapping_add(k * 0xc));
        let v = m.i32(s);
        m.set_i32(d, if v == -1 { v } else { v.wrapping_add(vo) });
        let e = m.i32(s + 4);
        m.set_i32(d + 4, if e == 0 { 0 } else if e < 0 { e.wrapping_sub(eo) } else { e.wrapping_add(eo) });
        let p = m.i32(s + 8);
        m.set_i32(d + 8, if p == -1 { p } else { p.wrapping_add(po) });
    }
}

/// Address of edge `i` (1-based) of the paged edge table `table`: pages of 0x400 edges of 0x14 bytes.
fn edge_at(m: &Mem, table: u32, i: i32) -> u32 {
    let page = m.u32(table.wrapping_add(((i >> 10) as u32).wrapping_mul(4)));
    page.wrapping_add(((i & 0x3ff) as u32).wrapping_mul(0x14))
}

/// `SI_meshMerge(n, models, mode, dist)` [soft 0x08315c64 SI_meshMerge]: merges the meshes of `n` models, each
/// transformed by its model's matrix (`+0x1c4` of the model, with the transposed inverse for the normals), into one
/// new mesh; returns it, or 0 when a model gives no mesh (`SI_maxNbPolWarning`) or the total is too big (a status
/// message). Mode 0 only joins the arrays (indices shifted by what the earlier meshes hold; the polygon material
/// numbers by the earlier models' material counts); any other mode runs the boolean engine on the meshes in turn
/// (the first alone, the others classified against the tree) and builds the result with `si_buildMesh`. The
/// result takes the first mesh's `+0xc` and `+0x10` floats and the flags in `+0x20` of the meshes (bit 2 if any
/// has it, bit 1 only if all have it, the rest of the bits cleared except as the combination gives). The array of
/// meshes is not freed on the normal path (the original leaks it); the meshes themselves are disposed.
pub fn si_mesh_merge(m: &mut Mem, o: &mut dyn Outside, n: i32, models: u32, mode: i16, dist: f64) -> u32 {
    if n <= 0 {
        return 0;
    }
    let sq = (x(dist) * x(dist)).to_f64();
    set_merge_dist(m, sq);
    let arr = m.si_malloc((n as u32).wrapping_mul(4));
    // the original's frame, addressed from its ebp
    let fr = m.local(0x130);
    let ebp = fr + 0x130;
    let (a_flags, a_f0c, a_f10) = (ebp - 0xde, ebp - 0xd8, ebp - 0xdc);
    let out_slot = ebp - 0xcc;
    m.set_u32(out_slot, 0); // uninitialised in the original, written by every path that returns it
    let mut ok = 0i32; // successful conversions (e8)
    let mut failed = 0i32;
    for k in 0..n {
        let model = m.u32(models.wrapping_add(k as u32 * 4));
        let mesh = call(m, o, SI_MODEL_CONVERT_TO_MESH, &[model]);
        m.set_u32(arr.wrapping_add(ok as u32 * 4), mesh);
        if mesh == 0 {
            failed += 1;
        } else {
            let tmp = ebp - 0x40;
            call(m, o, SI_MATRIX_TRANSPOSE_INVERSE, &[model + 0x1c4, tmp]);
            call(m, o, SI_MESH_TRANSFORM, &[mesh, model + 0x1c4, tmp]);
            if k == 0 {
                let first = m.u32(arr);
                let fl = m.u16(first + 0x20);
                m.set_u16(a_flags, fl);
                cp32(m, a_f0c, first + 0xc);
                cp32(m, a_f10, first + 0x10);
            } else {
                let mine = m.u16(mesh + 0x20);
                let cur = m.u16(a_flags);
                m.set_u16(a_flags, (cur & 4) | (cur & 2 & mine) | (mine & 4));
            }
            ok += 1;
        }
    }
    let ret;
    if failed != 0 {
        call(m, o, SI_MAX_NB_POL_WARNING, &[]);
        tail_dispose(m, o, arr, ok);
        if arr != 0 {
            m.free(arr);
        }
        m.release(0x130);
        return 0;
    }
    let flags = |m: &Mem| m.u16(a_flags);
    if mode == 0 {
        let (mut np, mut ne, mut nv) = (0u32, 0u32, 0u32);
        for k in 0..ok as u32 {
            let mesh = m.u32(arr + k * 4);
            np = np.wrapping_add(m.u32(mesh + 0x18));
            ne = ne.wrapping_add(m.u32(mesh + 0x1c));
            nv = nv.wrapping_add(m.u32(mesh + 0x14));
        }
        if np > 0x8000_0000 || ne > 0x4000_0000 || nv > 0x8000_0000 {
            call(m, o, SI_STATUS_DISPLAY_MESSAGE, &[MSG_TOO_MANY, 1]);
            tail_dispose(m, o, arr, ok);
            m.release(0x130);
            return 0;
        }
        let out = call(m, o, SI_MESH_ALLOCATE, &[]);
        m.set_u32(out_slot, out);
        m.set_u32(out + 0x18, np);
        m.set_u32(out + 0x1c, ne);
        m.set_u32(out + 0x14, nv);
        let polys = call(m, o, SI_POLYGON_ALLOCATE, &[np]);
        m.set_u32(out + 4, polys);
        let edges = call(m, o, SI_EDGE_ALLOCATE, &[ne]);
        m.set_u32(out + 8, edges);
        let verts = call(m, o, SI_VERTEX_ALLOCATE, &[nv]);
        m.set_u32(out, verts);
        let (mut po, mut eo, mut vo, mut mo) = (0u32, 0u32, 0u32, 0i32);
        for k in 0..ok as u32 {
            let mesh = m.u32(arr + k * 4);
            if flags(m) & 2 == 0 && m.u8(mesh + 0x20) & 2 != 0 {
                call(m, o, SI_REMOVE_FACETED, &[mesh]);
            }
            let model = m.u32(models + k * 4);
            let src_polys = m.u32(mesh + 4);
            let dst_polys = m.u32(out + 4);
            for i in 0..m.i32(mesh + 0x18) as u32 {
                let d = dst_polys.wrapping_add(po.wrapping_add(i).wrapping_mul(0x18));
                copy_polygon(m, d, src_polys.wrapping_add(i * 0x18), vo as i32, eo as i32, mo);
            }
            let (st, dt) = (m.u32(mesh + 8), m.u32(out + 8));
            let mut i = 1i32;
            while i <= m.i32(mesh + 0x1c) {
                let d = edge_at(m, dt, (eo as i32).wrapping_add(i));
                let s = edge_at(m, st, i);
                let a = m.i32(s).wrapping_add(vo as i32);
                m.set_i32(d, a);
                let b = m.i32(s + 4).wrapping_add(vo as i32);
                m.set_i32(d + 4, b);
                for off in [8u32, 0xc] {
                    let v = m.i32(s + off);
                    m.set_i32(d + off, if v == -1 { v } else { v.wrapping_add(po as i32) });
                }
                let sv = m.u16(s + 0x10);
                let dv = if sv == 0 {
                    0
                } else if (sv as i16) < 0 {
                    sv.wrapping_sub(vo as u16)
                } else {
                    sv.wrapping_add(vo as u16)
                };
                m.set_u16(d + 0x10, dv);
                let t = m.u16(s + 0x12);
                m.set_u16(d + 0x12, t);
                i += 1;
            }
            let (sv, dv) = (m.u32(mesh), m.u32(out));
            for i in 0..m.i32(mesh + 0x14) as u32 {
                let d = dv.wrapping_add(vo.wrapping_add(i).wrapping_mul(0x14));
                copy_vertex(m, d, sv.wrapping_add(i * 0x14), vo as i32, eo as i32, po as i32);
            }
            po = po.wrapping_add(m.u32(mesh + 0x18));
            eo = eo.wrapping_add(m.u32(mesh + 0x1c));
            vo = vo.wrapping_add(m.u32(mesh + 0x14));
            mo = mo.wrapping_add(m.i16(model + 0x206) as i32);
        }
    } else {
        init_boolean(m);
        set_classifying(m, 0);
        let mut tree = 0u32;
        let mut mo = 0i32;
        let group = ebp - 0x40;
        for k in 0..ok as u32 {
            if k != 0 {
                set_classifying(m, 1);
            }
            let mesh = m.u32(arr + k * 4);
            if flags(m) & 2 == 0 && m.u8(mesh + 0x20) & 2 != 0 {
                call(m, o, SI_REMOVE_FACETED, &[mesh]);
            }
            if call(m, o, SI_MESH_PLANAR, &[mesh]) & 0xffff == 0 {
                call(m, o, SI_MESH_TRIANGULATE, &[mesh, 1]);
                set_cleanup(m, 1, 1);
            }
            let list = si_generate_face_list(m, o, mesh, 0, ebp - 0x80, ebp - 0xc0);
            m.set_u32(ebp - 0x10, list);
            if list != 0 {
                let mut f = list;
                while f != 0 {
                    let v = m.i32(f + 0x5c).wrapping_add(mo);
                    m.set_i32(f + 0x5c, v);
                    f = m.u32(f + 0x78);
                }
                compute_face_group_size(m, group);
                tree = si_build_bsp(m, o, tree, group, 1);
            }
            let model = m.u32(models + k * 4);
            mo = mo.wrapping_add(m.i16(model + 0x206) as i32);
        }
        let (first, last) = (ebp - 0xc8, ebp - 0xc4);
        si_get_resulting_faces(m, tree, first, last);
        crate::clean::si_clean_up_faces(m, o, first);
        crate::clean::si_clean_up_edges(m, first);
        si_build_mesh(m, o, out_slot, m.u32(first));
        si_release_mem(m);
    }
    tail_dispose(m, o, arr, ok);
    let out = m.u32(out_slot);
    if out != 0 {
        cp32(m, out + 0xc, a_f0c);
        cp32(m, out + 0x10, a_f10);
        let fl = m.u16(a_flags);
        m.set_u16(out + 0x20, fl);
        call(m, o, SI_MESH_INIT_VERTEX_COLORS, &[out]);
    }
    ret = out;
    m.release(0x130);
    ret
}
