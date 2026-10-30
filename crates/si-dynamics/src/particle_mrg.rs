// SPDX-License-Identifier: MIT
//! `particle_mrg.c` of Softimage|3D 4.0 (0x084c4a50 … 0x084c5290): copies of meshes and materials for the particle
//! animation (finding 0045). The allocators and copiers of other files are reached through [`ParticleCalls`].
//!
//! Layouts used (our names): a mesh has vertices (0x14 bytes each) at +0, polygons (0x18 bytes) at +4, words at +0xc and
//! +0x10 that are copied, the vertex count +0x14, the polygon count +0x18 and a short of flags at +0x20. A polygon
//! has three words at +0..+8, the corner records (0x24 bytes) at +0xc, a short corner count at +0x10, a short at +0x12
//! and one at +0x14. A model has its material count (short) at +0x206, the material list at +0x20c, a short at +0x208,
//! its 2D texture list at +0x218/+0x21c and its 3D texture at +0x224; a texture continues in a list through +0x16c.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The graph list of the current process record: `*SI_grafLists + SI_procData->[+4] * 0x24` (both are pointer variables).
fn graf_list(m: &Mem) -> u32 {
    m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24))
}

/// Makes a mesh of `count` copies of a mesh [soft 0x084c4a50 si_particleDuplicateMesh]: 0 when `count` is not positive.
/// The new mesh takes the words at +0xc and +0x10, the flags with bits 0 and 2 cleared (`& 0xfffa`), `count` times the
/// vertices (first three words and the short at +0x12 of each record) and polygons (each corner record's vertex index
/// moved by the copy number times the vertex count, three words at +8, the texture coordinates through the
/// corner-copy routine, the word at +0x1c) and has its derived data rebuilt.
pub fn si_particle_duplicate_mesh(m: &mut Mem, o: &mut dyn Outside, count: i32, src: u32) -> u32 {
    if count < 1 {
        return 0;
    }
    let mesh = o.si_mesh_allocate(m);
    let w = m.u32(src + 0xc);
    m.set_u32(mesh + 0xc, w);
    let w = m.u32(src + 0x10);
    m.set_u32(mesh + 0x10, w);
    let fl = m.u16(src + 0x20) & 0xfffa;
    m.set_u16(mesh + 0x20, fl);
    let nv = m.i32(src + 0x14);
    m.set_i32(mesh + 0x14, count.wrapping_mul(nv));
    let verts = o.si_vertex_allocate(m, count.wrapping_mul(nv));
    m.set_u32(mesh, verts);
    let mut c = 0i32;
    while c < count {
        let mut k = 0i32;
        let mut sofs = 0u32;
        let mut dofs = (c.wrapping_mul(nv) as u32).wrapping_mul(0x14);
        while k < nv {
            let dst = dofs.wrapping_add(m.u32(mesh));
            let sv = m.u32(src);
            for j in 0..3 {
                let v = m.u32(sv.wrapping_add(4 * j).wrapping_add(sofs));
                m.set_u32(dst + 4 * j, v);
            }
            dofs = dofs.wrapping_add(0x14);
            let v = m.u16(sv.wrapping_add(0x12).wrapping_add(sofs));
            m.set_u16(dst + 0x12, v);
            sofs = sofs.wrapping_add(0x14);
            k += 1;
        }
        c += 1;
    }
    let np = m.i32(src + 0x18);
    m.set_i32(mesh + 0x18, count.wrapping_mul(np));
    let polys = o.si_polygon_allocate(m, count.wrapping_mul(np));
    m.set_u32(mesh + 4, polys);
    let mut c = 0i32;
    while c < count {
        let mut p = 0i32;
        while p < np {
            let dp = m.u32(mesh + 4).wrapping_add(((c.wrapping_mul(np).wrapping_add(p)) as u32).wrapping_mul(0x18));
            let sp = m.u32(src + 4).wrapping_add((p as u32).wrapping_mul(0x18));
            let nc = m.u16(sp + 0x10) as u32;
            m.set_u16(dp + 0x10, nc as u16);
            m.set_u32(dp + 0xc, 0);
            o.si_nodes_allocate(m, 0, dp + 0xc, 0, nc);
            let mut left = nc;
            let mut ofs = 0u32;
            while left != 0 {
                let sn = m.u32(sp + 0xc);
                let dn = m.u32(dp + 0xc);
                let vi = c.wrapping_mul(nv).wrapping_add(m.i32(sn.wrapping_add(ofs)));
                m.set_i32(dn.wrapping_add(ofs), vi);
                for j in [8u32, 0xc, 0x10] {
                    let v = m.u32(sn.wrapping_add(ofs).wrapping_add(j));
                    m.set_u32(dn.wrapping_add(ofs).wrapping_add(j), v);
                }
                o.si_nodes_copy_uv(m, 0, 0, dn.wrapping_add(ofs), sn.wrapping_add(ofs));
                let dn = m.u32(dp + 0xc);
                let sn = m.u32(sp + 0xc);
                let v = m.u32(sn.wrapping_add(0x1c).wrapping_add(ofs));
                m.set_u32(dn.wrapping_add(0x1c).wrapping_add(ofs), v);
                ofs = ofs.wrapping_add(0x24);
                left = left.wrapping_sub(1);
            }
            let v = m.u16(sp + 0x14);
            m.set_u16(dp + 0x14, v);
            let v = m.u16(sp + 0x12);
            m.set_u16(dp + 0x12, v);
            for j in [0u32, 4, 8] {
                let v = m.u32(sp + j);
                m.set_u32(dp + j, v);
            }
            p += 1;
        }
        c += 1;
    }
    o.si_mesh_update(m, mesh);
    mesh
}

/// Copies the materials and textures of a model to another [soft 0x084c4d20 si_particleMaterialCopy]: -1 when the
/// source has no material list or count; else 1. When the destination has no materials yet it takes the count, a
/// `malloc`ed list of copies (each added to the graph list with kind 0xe, set on the model and given the source's
/// shaders) and the short at +0x208. The destination's 2D texture list and 3D texture are disposed, those of the source are
/// copied (kinds 0xf and 0x10), and then each material's own 2D list (+0x6ec, tail +0x6f0) and 3D texture (+0x6fc) are
/// copied the same way. Written as the listing runs; the callees are hooks.
pub fn si_particle_material_copy(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    let mut cur = 0u32;
    if m.u32(a + 0x20c) == 0 || (m.u16(a + 0x206) as i16) < 1 {
        return -1;
    }
    if m.u16(b + 0x206) == 0 {
        let n = m.u16(a + 0x206);
        m.set_u16(b + 0x206, n);
        let p = m.malloc(((n as i16 as i32) << 2) as u32);
        m.set_u32(b + 0x20c, p);
        let mut i = 0u32;
        while (i as i32) < m.u16(b + 0x206) as i16 as i32 {
            let sm = m.u32(m.u32(a + 0x20c) + 4 * i);
            let nm = o.si_material_copy(m, sm);
            m.set_u32(m.u32(b + 0x20c) + 4 * i, nm);
            let gl = graf_list(m);
            o.si_graflist_add_data(m, gl, nm, 0xe, 0);
            o.si_ed_model_set_material(m, b, nm);
            let (d, s) = (m.u32(m.u32(b + 0x20c) + 4 * i), m.u32(m.u32(a + 0x20c) + 4 * i));
            o.si_copy_material_shaders(m, d, s);
            i += 1;
        }
        let v = m.u16(a + 0x208);
        m.set_u16(b + 0x208, v);
    }
    let mut t = m.u32(b + 0x21c);
    while t != 0 {
        let next = m.u32(t + 0x16c);
        o.si_ed_dispose_texture2d(m, t);
        t = next;
    }
    m.set_u32(b + 0x21c, 0);
    m.set_u32(b + 0x218, 0);
    if m.u32(b + 0x224) != 0 {
        let t3 = m.u32(b + 0x224);
        o.si_ed_model_relax_gtexture3d(m, b, t3);
        let t3 = m.u32(b + 0x224);
        if o.si_texture3d_still_used(m, t3) as i16 == 0 {
            let t3 = m.u32(b + 0x224);
            o.si_ed_dispose_texture3d(m, t3);
        }
        m.set_u32(b + 0x224, 0);
    }
    let mut t = m.u32(a + 0x21c);
    while t != 0 {
        if m.u32(b + 0x21c) == 0 {
            let c = o.si_txt2d_copy(m, t);
            m.set_u32(b + 0x21c, c);
            o.si_txt2d_set_wrap(m, b, c);
            cur = m.u32(b + 0x21c);
            m.set_u32(b + 0x218, cur);
            let gl = graf_list(m);
            o.si_graflist_add_data(m, gl, cur, 0xf, 0);
            let c = m.u32(b + 0x21c);
            o.si_ed_model_set_gtexture2d(m, b, c);
            let (c, sh) = (m.u32(b + 0x21c), m.u32(t + 0x198));
            o.si_copy_t2d_shader(m, c, sh);
        }
        let c = o.si_txt2d_copy(m, t);
        m.set_u32(cur + 0x16c, c);
        o.si_txt2d_set_wrap(m, b, c);
        let gl = graf_list(m);
        let c = m.u32(cur + 0x16c);
        o.si_graflist_add_data(m, gl, c, 0xf, 0);
        let c = m.u32(cur + 0x16c);
        o.si_ed_model_set_gtexture2d(m, b, c);
        let (c, sh) = (m.u32(cur + 0x16c), m.u32(t + 0x198));
        o.si_copy_t2d_shader(m, c, sh);
        cur = m.u32(cur + 0x16c);
        t = m.u32(t + 0x16c);
    }
    if m.u32(a + 0x224) != 0 {
        let src = m.u32(a + 0x224);
        let c = o.si_txt3d_copy(m, src);
        m.set_u32(b + 0x224, c);
        let gl = graf_list(m);
        o.si_graflist_add_data(m, gl, c, 0x10, 0);
        let c = m.u32(b + 0x224);
        o.si_ed_model_set_gtexture3d(m, b, c);
        let (c, sh) = (m.u32(b + 0x224), m.u32(m.u32(a + 0x224) + 0x14c));
        o.si_copy_t3d_shader(m, c, sh);
    }
    let mut i = 0u32;
    while (i as i32) < m.u16(a + 0x206) as i16 as i32 {
        let sm = m.u32(m.u32(a + 0x20c) + 4 * i);
        let dm = m.u32(m.u32(b + 0x20c) + 4 * i);
        i += 1;
        let mut t = m.u32(sm + 0x6ec);
        while t != 0 {
            if m.u32(dm + 0x6ec) == 0 {
                let c = o.si_txt2d_copy(m, t);
                m.set_u32(dm + 0x6ec, c);
                o.si_txt2d_set_wrap(m, b, c);
                let gl = graf_list(m);
                let c = m.u32(dm + 0x6ec);
                o.si_graflist_add_data(m, gl, c, 0xf, 0);
                let c = m.u32(dm + 0x6ec);
                o.si_ed_model_set_ltexture2d(m, dm, c);
                let (c, sh) = (m.u32(dm + 0x6ec), m.u32(t + 0x198));
                o.si_copy_t2d_shader(m, c, sh);
                cur = m.u32(dm + 0x6ec);
                m.set_u32(dm + 0x6f0, cur);
            } else {
                let c = o.si_txt2d_copy(m, t);
                m.set_u32(cur + 0x16c, c);
                o.si_txt2d_set_wrap(m, b, c);
                let gl = graf_list(m);
                let c = m.u32(cur + 0x16c);
                o.si_graflist_add_data(m, gl, c, 0xf, 0);
                let c = m.u32(cur + 0x16c);
                o.si_ed_model_set_ltexture2d(m, dm, c);
                let (c, sh) = (m.u32(cur + 0x16c), m.u32(t + 0x198));
                o.si_copy_t2d_shader(m, c, sh);
                cur = m.u32(cur + 0x16c);
            }
            t = m.u32(t + 0x16c);
        }
        if m.u32(sm + 0x6fc) != 0 {
            let src = m.u32(sm + 0x6fc);
            let c = o.si_txt3d_copy(m, src);
            m.set_u32(dm + 0x6fc, c);
            let gl = graf_list(m);
            o.si_graflist_add_data(m, gl, c, 0x10, 0);
            let c = m.u32(dm + 0x6fc);
            o.si_ed_model_set_ltexture3d(m, dm, c);
            let (c, sh) = (m.u32(dm + 0x6fc), m.u32(m.u32(sm + 0x6fc) + 0x14c));
            o.si_copy_t3d_shader(m, c, sh);
        }
    }
    1
}
