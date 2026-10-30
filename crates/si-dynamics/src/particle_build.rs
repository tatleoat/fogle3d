// SPDX-License-Identifier: MIT
//! The two animation builders of the particle code of Softimage|3D 4.0 (finding 0045): `particleCreateHrcAnimation` (0x084c0e24) makes
//! a hierarchy of copies of a model that flies from a source to a destination model, one copy per particle, with keys
//! at the key-shape frames; `particleCreateShapeAnimation` (0x084c1de8) makes one mesh model of all the copies whose
//! vertices are animated by shape keys. Both read the same status record (our names, offsets in bytes):
//! +4 name, +0xc up vector, +0x18 and +0x1c source and destination seeds, +0x20 seed step, +0x24 particle count,
//! +0x28 and +0x2c points instead of triangle samples (source, destination), +0x30 key shape count, +0x34 shape mode,
//! +0x38 start frame, +0x3c stop frame, +0x44 frame step, +0x54 key shapes, +0x58 uniform sampling, +0x5c use the up
//! vector, +0x60 copy materials, +0x68 +0x6c +0x70 jitter scale, rotation, translation (amounts +0x78 +0x84 +0x90),
//! +0x74 data jitter (amount +0x9c), +0xac material update, +0xb0 key frames relative, +0xb4 source, +0xb8 destination
//! and +0xbc template model. A model has its name at +4, kind +0x30, geometry +0x48, materials +0x206 (short count),
//! +0x208 (current), +0x20c (list), siblings +0x2c0 and children +0x2c4.
//!
//! The callees of other files are reached through [`ParticleCalls`]; `SI_frameToSec`, `sprintf`, `strdup`, `free`
//! and `srand48` run here as the program uses them.

use crate::particle_data::*;
use crate::outside::DynaCalls;
use crate::particle_mf::*;
use crate::particle_outside::ParticleCalls;
use crate::particle_mrg::{si_particle_duplicate_mesh, si_particle_material_copy};
use crate::particle_util::{particle_make_gmat, particle_transfer_gmat};
use si_mem::{Mem, Outside};
use si_x87::{x, Round, F80};

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_FRAME_CURRENT: u32 = 0x0878_2040;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const SI_CID_MODELS: u32 = 0x0878_1680;
const HALF_HRC: u32 = 0x0866_1308;
const HALF_SHAPE: u32 = 0x0866_1310;

fn graf_list(m: &Mem) -> u32 {
    m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24))
}

fn frame(m: &Mem) -> i32 {
    m.i32(SI_FRAME_CURRENT)
}

fn set_frame(m: &mut Mem, v: i32) {
    m.set_i32(SI_FRAME_CURRENT, v);
}

/// `SI_frameToSec`: the frame over the frame rate.
fn frame_to_sec(m: &Mem, f: i32) -> F80 {
    F80::from_i32(f) / x(m.f32(SI_FRAME_RATE))
}

fn update(m: &mut Mem, o: &mut dyn Outside, model: u32, node: u32) {
    let t = frame_to_sec(m, frame(m)).to_f32();
    o.si_model_update(m, model, t, node);
}

/// `sprintf(buf, "%s_%d", name, n)`.
fn name_n(m: &mut Mem, buf: u32, name: u32, n: i32) {
    let mut s = if name == 0 { b"(null)".to_vec() } else { m.cstr(name) };
    s.push(b'_');
    s.extend_from_slice(n.to_string().as_bytes());
    m.put_cstr(buf, &s);
}

fn mesh_of(m: &Mem, model: u32) -> u32 {
    m.u32(model + 0x48)
}

/// The key-shape step: `(float)(stop - start) / (float)(keys - 1)`, rounded to a float.
fn key_step(m: &Mem, st: u32) -> f32 {
    let span = x(x(m.i32(st + 0x3c).wrapping_sub(m.i32(st + 0x38))).to_f32());
    (span / F80::from_i32(m.i32(st + 0x30) - 1)).to_f32()
}

/// `(int)((float)kk * step + half)` truncated.
fn key_frame(m: &Mem, kk: i32, step: f32, half: u32) -> i32 {
    (F80::from_i32(kk) * x(step) + x(m.f64(half))).to_i32(Round::Zero)
}

/// Samples a model's points or triangles into a block (the source, destination or key points); 1 on success, else
/// the message is shown and 0 returned. `msg` 0 means a failed triangle conversion (no message).
fn sample(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, model: u32, points: bool, tri: u32, seed: u32, out: u32, msg: u32) -> bool {
    if !points {
        if si_particle_mesh_to_tri(m, o, mesh_of(m, model), tri) < 0 {
            return false;
        }
        let uniform = m.i32(st + 0x58);
        si_particle_tri_sample(m, r, tri, uniform, seed, out);
    } else if si_particle_mesh_to_point(m, mesh_of(m, model), out) < 0 {
        o.si_status_display_message(m, msg, 3);
        return false;
    }
    true
}

/// Fills a data block header: dimension 3 and a row count.
fn pd_init(m: &mut Mem, pd: u32, rows: i32) {
    m.set_i32(pd, 3);
    m.set_i32(pd + 4, rows);
}

/// The keys of the paths (key shapes): for each key shape after the first the destination is sampled again at its
/// frame and the difference to the first destination added to the paths.
fn key_paths(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, k: i32, n10: u32, tri_d: u32, dst: u32, key: u32, paths: u32, gm: u32, half: u32, msg: u32) -> bool {
    let keys = m.i32(st + 0x30);
    let count = m.i32(st + 0x24);
    let step = key_step(m, st);
    let mut kk = 1;
    while kk < keys {
        let tr = key_frame(m, kk, step, half);
        let f = if m.i32(st + 0xb0) == 0 {
            k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)).wrapping_add(tr)
        } else {
            tr.wrapping_add(m.i32(st + 0x38))
        };
        set_frame(m, f);
        update(m, o, m.u32(st + 0xb8), n10);
        let seed = k.wrapping_mul(m.i32(st + 0x20)).wrapping_add(m.i32(st + 0x1c)) as u32;
        let (b8, pts) = (m.u32(st + 0xb8), m.i32(st + 0x2c) != 0);
        if !sample(m, o, r, st, b8, pts, tri_d, seed, key, msg) {
            return false;
        }
        particle_transfer_gmat(m, b8, gm);
        si_particle_data_single_transform(m, gm, key, key);
        let (kd, dd, pd) = (m.u32(key + 8), m.u32(dst + 8), m.u32(paths + 8));
        for j in 0..count {
            let idx = j.wrapping_mul(keys).wrapping_add(kk);
            for c in 0..3u32 {
                let a = pd.wrapping_add((idx as u32).wrapping_mul(12)).wrapping_add(4 * c);
                let v = (x(m.f32(kd + 12 * j as u32 + 4 * c)) - x(m.f32(dd + 12 * j as u32 + 4 * c))) + x(m.f32(a));
                m.set_f32(a, v.to_f32());
            }
        }
        kk += 1;
    }
    true
}

/// The jitter matrix of one particle: identity, then each enabled jitter multiplied in.
fn jitter_matrix(m: &mut Mem, r: &mut dyn Drand, st: u32, jm: u32, jt: u32) {
    si_particle_make_identity(m, jm);
    if m.i32(st + 0x68) != 0 {
        si_particle_jitter_scale_matrix(m, r, st + 0x78, jt);
        si_particle_matrix_multiply(m, jm, jt, jm);
    }
    if m.i32(st + 0x6c) != 0 {
        si_particle_jitter_rotate_matrix(m, r, st + 0x84, jt);
        si_particle_matrix_multiply(m, jm, jt, jm);
    }
    if m.i32(st + 0x70) != 0 {
        si_particle_jitter_translate_matrix(m, r, st + 0x90, jt);
        si_particle_matrix_multiply(m, jm, jt, jm);
    }
}

/// Marks the first key of a curve found by `SI_edFindFcv` as the start of its range (two shorts).
fn mark_fcv(m: &mut Mem, f: u32) {
    if f != 0 {
        m.set_u16(f + 0x1e, 1);
        let g = m.u32(f + 8);
        m.set_u16(g + 0xe, 1);
    }
}

/// The material loop of a copy: for each material of the template the current material is selected, the template's
/// four material words are saved and restored around the update and the transfer, and the material is keyed.
fn material_keys(m: &mut Mem, o: &mut dyn Outside, st: u32, bc: u32, n11: u32, cur: u32, nmat: i32) {
    let mut mi = 0;
    while mi < nmat {
        m.set_u16(cur + 0x208, mi as u16);
        let mat = m.u32(m.u32(cur + 0x20c) + 4 * mi as u32);
        let sv = [m.u32(mat + 0xdb0), m.u32(mat + 0xdb4), m.u32(mat + 0xdb8), m.u32(mat + 0xdbc)];
        if m.i32(st + 0xac) == 0 {
            update(m, o, bc, n11);
        }
        let src = m.u32(m.u32(bc + 0x20c) + 4 * mi as u32);
        let dst = m.u32(m.u32(cur + 0x20c) + 4 * mi as u32);
        o.si_material_transfer(m, src, dst, 0);
        let mat = m.u32(m.u32(cur + 0x20c) + 4 * mi as u32);
        for (i, v) in sv.iter().enumerate() {
            m.set_u32(mat + 0xdb0 + 4 * i as u32, *v);
        }
        o.si_savekey_material(m, cur);
        mi += 1;
    }
}

fn free_all(m: &mut Mem, pds: &[u32]) {
    for &p in pds {
        si_particle_data_free(m, p);
    }
}

/// Makes the hierarchy animation of a status record [soft 0x084c0e24 particleCreateHrcAnimation]: the model that
/// holds the copies, or 0 when a step fails (the frame is restored). `arg` is the number appended to the name.
pub fn particle_create_hrc_animation(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, arg: i32) -> u32 {
    let sp = m.sp();
    let res = hrc(m, o, r, st, arg);
    m.set_sp(sp);
    res
}

fn hrc(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, arg: i32) -> u32 {
    let saved = frame(m);
    let loc = m.local(7 * 12 + 3 * 64 + 80 + 4);
    let (src, dst, key, tri_s, tri_d, paths, xfm) = (loc, loc + 12, loc + 24, loc + 36, loc + 48, loc + 60, loc + 72);
    let (gm, jm, jt) = (loc + 84, loc + 148, loc + 212);
    let buf = loc + 276;
    let outw = loc + 356;
    for p in [src, dst, key, tri_s, tri_d, paths, xfm] {
        si_particle_data_initialize(m, p);
    }
    if m.i32(st + 0x30) < 2 {
        o.si_status_display_message(m, 0x086d_7c3b, 3);
        return 0;
    }
    let list = graf_list(m);
    let (b4, b8, bc) = (m.u32(st + 0xb4), m.u32(st + 0xb8), m.u32(st + 0xbc));
    let n9 = o.si_find_gl_data(m, list, b4, 0);
    let n10 = o.si_find_gl_data(m, list, b8, 0);
    let n11 = o.si_find_gl_data(m, list, bc, 0);
    let k = arg.wrapping_sub(1);
    let count = m.i32(st + 0x24);
    let keys = m.i32(st + 0x30);
    set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)));
    update(m, o, b4, n9);
    if m.i32(st + 0x54) == 1 && m.i32(st + 0xb0) != 0 {
        set_frame(m, m.i32(st + 0x38));
    }
    update(m, o, b8, n10);
    pd_init(m, src, count);
    pd_init(m, dst, count);
    if si_particle_data_allocate(m, src) < 0 || si_particle_data_allocate(m, dst) < 0 {
        set_frame(m, saved);
        return 0;
    }
    let step = k.wrapping_mul(m.i32(st + 0x20));
    let (pts_s, pts_d) = (m.i32(st + 0x28) != 0, m.i32(st + 0x2c) != 0);
    if !sample(m, o, r, st, b4, pts_s, tri_s, step.wrapping_add(m.i32(st + 0x18)) as u32, src, 0x086d_7c60) {
        set_frame(m, saved);
        return 0;
    }
    particle_transfer_gmat(m, b4, gm);
    si_particle_data_single_transform(m, gm, src, src);
    if !sample(m, o, r, st, b8, pts_d, tri_d, step.wrapping_add(m.i32(st + 0x1c)) as u32, dst, 0x086d_7ca0) {
        set_frame(m, saved);
        return 0;
    }
    particle_transfer_gmat(m, b8, gm);
    si_particle_data_single_transform(m, gm, dst, dst);
    if si_particle_compute_paths(m, o, st, src, dst, paths) < 0 {
        free_all(m, &[src, dst]);
        set_frame(m, saved);
        return 0;
    }
    if m.i32(st + 0x54) == 1 {
        pd_init(m, key, count);
        si_particle_data_allocate(m, key);
        if !key_paths(m, o, r, st, k, n10, tri_d, dst, key, paths, gm, HALF_HRC, 0x086d_7ce0) {
            set_frame(m, saved);
            return 0;
        }
    }
    let up = if m.i32(st + 0x5c) == 0 { 0 } else { st + 0xc };
    if si_particle_path2_xfm(m, paths, xfm, up) < 0 {
        free_all(m, &[tri_s, tri_d, src, dst, paths]);
        set_frame(m, saved);
        return 0;
    }
    name_n(m, buf, m.u32(st + 4), arg);
    let model28 = o.si_model_allocate(m, buf);
    let list = graf_list(m);
    let p14 = o.si_graflist_add_data(m, list, model28, 2, 1);
    let nmat = m.u16(bc + 0x206) as i16 as i32;
    let saved2 = frame(m);
    if nmat > 0 {
        set_frame(m, m.i32(st + 0x38));
        update(m, o, bc, n11);
    }
    set_frame(m, saved2);
    for i in 0..count {
        let node = o.si_model_copy_node(m, bc, 1);
        name_n(m, buf, m.u32(node + 4), i);
        if m.u32(node + 4) != 0 {
            let p = m.u32(node + 4);
            m.free(p);
            m.set_u32(node + 4, 0);
        }
        let d = m.strdup(buf);
        m.set_u32(node + 4, d);
        let list = graf_list(m);
        let gn = o.si_graflist_add_data(m, list, node, 2, 1);
        o.si_model_create_parent(m, p14, gn, 1);
        if nmat > 0 {
            si_particle_material_copy(m, o, bc, node);
        }
    }
    let jit = m.i32(st + 0x68) != 0 || m.i32(st + 0x6c) != 0 || m.i32(st + 0x70) != 0;
    if jit {
        r.srand48(k.wrapping_mul(m.i32(st + 0x20)).wrapping_add(m.i32(st + 0x18)) as u32);
    }
    let elem = ParticleCalls::si_ed_get_elem_from_model(o, m, bc, outw);
    let models = m.u32(SI_CID_MODELS);
    let grp = o.si_ed_check_group_relations(m, elem, models as i32, 2) != 0;
    set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)));
    let step = key_step(m, st);
    let xd = m.u32(xfm + 8);
    for kk in 0..keys {
        let sv = frame(m);
        set_frame(m, sv.wrapping_sub(k.wrapping_mul(m.i32(st + 0x44))));
        update(m, o, bc, n11);
        set_frame(m, sv);
        let mut cur = m.u32(model28 + 0x2c4);
        for j in 0..count {
            particle_transfer_gmat(m, bc, gm);
            if jit {
                jitter_matrix(m, r, st, jm, jt);
            }
            let p = xd.wrapping_add(((keys.wrapping_mul(16).wrapping_mul(j)).wrapping_add(kk.wrapping_mul(16)) as u32).wrapping_mul(4));
            si_particle_matrix_multiply(m, p, gm, gm);
            if jit {
                si_particle_matrix_multiply(m, gm, jm, gm);
            }
            particle_make_gmat(m, o, cur, gm);
            o.si_savekey_object_all(m, cur);
            if kk == 0 {
                let e = ParticleCalls::si_ed_get_elem_from_model(o, m, cur, outw);
                if e != 0 {
                    let f = ParticleCalls::si_ed_find_fcv(o, m, e, 1, 0x70);
                    mark_fcv(m, f);
                }
            }
            if grp {
                let (kind, g1, g2) = (m.u32(bc + 0x30), m.u32(bc + 0x48), m.u32(cur + 0x48));
                match kind {
                    2 => o.si_face_transfer_points(m, g1, g2),
                    1 => o.si_patch_transfer_points(m, g1, g2),
                    4 => {
                        o.si_mesh_transfer_vertices(m, g1, g2);
                        o.si_mesh_computes_polygon_normal(m, g2, -1);
                        o.si_mesh_compute_hard_edge(m, g2, -1);
                        o.si_mesh_computes_vertex_normal(m, g2, -1);
                    }
                    _ => {}
                }
                o.si_savekey_model(m, cur, 2, 0xc8, 8, 1, 0.0, 0, 0);
                if kk == 0 {
                    let e = o.si_ed_get_elem_from_shape(m, cur, outw);
                    if e != 0 {
                        let f = ParticleCalls::si_ed_find_fcv(o, m, e, 0, 0xc9);
                        mark_fcv(m, f);
                    }
                }
                m.set_u16(cur + 0x2e0, 1);
            }
            if m.i32(st + 0x60) != 0 && nmat > 0 {
                material_keys(m, o, st, bc, n11, cur, nmat);
            }
            cur = m.u32(cur + 0x2c0);
        }
        let tr = key_frame(m, kk + 1, step, HALF_HRC);
        set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)).wrapping_add(tr));
    }
    free_all(m, &[src, dst, key, tri_s, tri_d, paths, xfm]);
    set_frame(m, saved);
    update(m, o, b4, n9);
    update(m, o, b8, n10);
    update(m, o, bc, n11);
    model28
}

/// Makes the shape animation of a status record [soft 0x084c1de8 particleCreateShapeAnimation]: one mesh model that
/// holds a copy of the template mesh for every particle, keyed per frame; 0 when a step fails.
pub fn particle_create_shape_animation(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, arg: i32) -> u32 {
    let sp = m.sp();
    let res = shape(m, o, r, st, arg);
    m.set_sp(sp);
    res
}

fn shape(m: &mut Mem, o: &mut dyn Outside, r: &mut dyn Drand, st: u32, arg: i32) -> u32 {
    let saved = frame(m);
    let loc = m.local(9 * 12 + 3 * 64 + 80 + 4);
    let (src, dst, key, tri_s, tri_d, paths, curp, xfm, outp) =
        (loc, loc + 12, loc + 24, loc + 36, loc + 48, loc + 60, loc + 72, loc + 84, loc + 96);
    let (gm, jm, jt) = (loc + 108, loc + 172, loc + 236);
    let buf = loc + 300;
    let outw = loc + 380;
    for p in [src, dst, key, tri_s, tri_d, paths, curp, xfm, outp] {
        si_particle_data_initialize(m, p);
    }
    if m.i32(st + 0x30) < 2 {
        o.si_status_display_message(m, 0x086d_7d0f, 3);
        return 0;
    }
    let list = graf_list(m);
    let (b4, b8, bc) = (m.u32(st + 0xb4), m.u32(st + 0xb8), m.u32(st + 0xbc));
    let n9 = o.si_find_gl_data(m, list, b4, 0);
    let n10 = o.si_find_gl_data(m, list, b8, 0);
    let n11 = o.si_find_gl_data(m, list, bc, 0);
    let k = arg.wrapping_sub(1);
    let count = m.i32(st + 0x24);
    let keys = m.i32(st + 0x30);
    set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)));
    update(m, o, b4, n9);
    if m.i32(st + 0x54) == 1 && m.i32(st + 0xb0) != 0 {
        set_frame(m, m.i32(st + 0x38));
    }
    update(m, o, b8, n10);
    pd_init(m, src, count);
    pd_init(m, dst, count);
    if si_particle_data_allocate(m, src) < 0 || si_particle_data_allocate(m, dst) < 0 {
        set_frame(m, saved);
        return 0;
    }
    let step = k.wrapping_mul(m.i32(st + 0x20));
    let (pts_s, pts_d) = (m.i32(st + 0x28) != 0, m.i32(st + 0x2c) != 0);
    if !sample(m, o, r, st, b4, pts_s, tri_s, step.wrapping_add(m.i32(st + 0x18)) as u32, src, 0x086d_7d40) {
        set_frame(m, saved);
        return 0;
    }
    particle_transfer_gmat(m, b4, gm);
    si_particle_data_single_transform(m, gm, src, src);
    if !sample(m, o, r, st, b8, pts_d, tri_d, step.wrapping_add(m.i32(st + 0x1c)) as u32, dst, 0x086d_7d80) {
        set_frame(m, saved);
        return 0;
    }
    particle_transfer_gmat(m, b8, gm);
    si_particle_data_single_transform(m, gm, dst, dst);
    if si_particle_compute_paths(m, o, st, src, dst, paths) < 0 {
        free_all(m, &[src, dst]);
        set_frame(m, saved);
        return 0;
    }
    if m.i32(st + 0x54) == 1 {
        pd_init(m, key, count);
        si_particle_data_allocate(m, key);
        if !key_paths(m, o, r, st, k, n10, tri_d, dst, key, paths, gm, HALF_SHAPE, 0x086d_7dc0) {
            set_frame(m, saved);
            return 0;
        }
    }
    let up = if m.i32(st + 0x5c) == 0 { 0 } else { st + 0xc };
    if si_particle_path2_xfm(m, paths, xfm, up) < 0 {
        free_all(m, &[tri_s, tri_d, src, dst, paths]);
        set_frame(m, saved);
        return 0;
    }
    let nverts = m.i32(mesh_of(m, bc) + 0x14);
    let dup = si_particle_duplicate_mesh(m, o, count, mesh_of(m, bc));
    if dup == 0 {
        set_frame(m, saved);
        return 0;
    }
    name_n(m, buf, m.u32(st + 4), arg);
    let model = o.si_model_allocate(m, buf);
    m.set_u32(model + 0x30, 4);
    o.si_model_add_definition(m, model, 4, dup);
    let list = graf_list(m);
    o.si_graflist_add_data(m, list, model, 2, 1);
    let nmat = m.u16(bc + 0x206) as i16 as i32;
    if nmat > 0 {
        let sv = frame(m);
        set_frame(m, m.i32(st + 0x38));
        update(m, o, bc, n11);
        si_particle_material_copy(m, o, bc, model);
        set_frame(m, sv);
    }
    pd_init(m, curp, nverts);
    pd_init(m, outp, nverts);
    si_particle_data_allocate(m, curp);
    si_particle_data_allocate(m, outp);
    let jit = m.i32(st + 0x68) != 0 || m.i32(st + 0x6c) != 0 || m.i32(st + 0x70) != 0;
    if jit {
        r.srand48(k.wrapping_mul(m.i32(st + 0x20)).wrapping_add(m.i32(st + 0x18)) as u32);
    }
    set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)));
    let step = key_step(m, st);
    let xd = m.u32(xfm + 8);
    for kk in 0..keys {
        let sv = frame(m);
        set_frame(m, sv.wrapping_sub(k.wrapping_mul(m.i32(st + 0x44))));
        update(m, o, bc, n11);
        si_particle_mesh_to_point(m, mesh_of(m, bc), curp);
        particle_transfer_gmat(m, bc, gm);
        si_particle_data_single_transform(m, gm, curp, curp);
        set_frame(m, sv);
        for j in 0..count {
            if jit {
                jitter_matrix(m, r, st, jm, jt);
            }
            let p = xd.wrapping_add(((keys.wrapping_mul(16).wrapping_mul(j)).wrapping_add(kk.wrapping_mul(16)) as u32).wrapping_mul(4));
            if jit {
                si_particle_matrix_multiply(m, p, jm, jm);
                si_particle_data_single_transform(m, jm, curp, outp);
            } else {
                si_particle_data_single_transform(m, p, curp, outp);
            }
            if m.i32(st + 0x74) != 0 {
                si_particle_data_jitter(m, r, outp, st + 0x9c);
            }
            copy_points(m, outp, m.u32(dup), nverts, j);
        }
        o.si_mesh_computes_polygon_normal(m, dup, -1);
        o.si_mesh_compute_hard_edge(m, dup, -1);
        o.si_mesh_computes_vertex_normal(m, dup, -1);
        o.si_savekey_model(m, model, 2, 0xc8, 8, 1, 0.0, 0, 0);
        if kk == 0 {
            let e = o.si_ed_get_elem_from_shape(m, model, outw);
            if e != 0 {
                let f = ParticleCalls::si_ed_find_fcv(o, m, e, 0, 0xc9);
                mark_fcv(m, f);
            }
        }
        if m.i32(st + 0x60) != 0 && nmat > 0 {
            material_keys(m, o, st, bc, n11, model, nmat);
        }
        let tr = key_frame(m, kk + 1, step, HALF_SHAPE);
        set_frame(m, k.wrapping_mul(m.i32(st + 0x44)).wrapping_add(m.i32(st + 0x38)).wrapping_add(tr));
    }
    m.set_u16(model + 0x2e0, 1);
    set_frame(m, saved);
    update(m, o, b4, n9);
    update(m, o, b8, n10);
    update(m, o, bc, n11);
    si_particle_mesh_to_point(m, mesh_of(m, bc), curp);
    let mesh2 = m.u32(model + 0x3c);
    for j in 0..count {
        copy_points(m, curp, m.u32(mesh2), nverts, j);
    }
    o.si_mesh_computes_polygon_normal(m, mesh2, -1);
    o.si_mesh_compute_hard_edge(m, mesh2, -1);
    o.si_mesh_computes_vertex_normal(m, mesh2, -1);
    free_all(m, &[src, dst, key, tri_s, tri_d, paths, curp, xfm, outp]);
    model
}

/// Copies a block's points into the vertices of copy `j` of a duplicated mesh (three floats each, 0x14 bytes apart).
fn copy_points(m: &mut Mem, pd: u32, verts: u32, nverts: i32, j: i32) {
    let data = m.u32(pd + 8);
    for v in 0..nverts {
        let d = verts.wrapping_add((nverts.wrapping_mul(j).wrapping_add(v) as u32).wrapping_mul(0x14));
        for c in 0..3u32 {
            let w = m.u32(data + 12 * v as u32 + 4 * c);
            m.set_u32(d + 4 * c, w);
        }
    }
}
