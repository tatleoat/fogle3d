// SPDX-License-Identifier: MIT
//! `settings.c` (the rest): the geometry of a body (volume, centroid, inertia, the transfer between a joint and its
//! child), the allocation, copy and disposal of a model's dynamics records and the validation of their parents.
//!
//! Sources are functions of the Linux `soft`, cited as `[soft 0xADDRESS name]`. Callees of other source files go through
//! the hooks of [`crate::outside`]; the pure routines (matrix copies, the allocators of the records, the bounding) run
//! natively. The float code is the original's: every intermediate is an `F80`, every store a `to_f32`.

use crate::outside::DynaCalls;
use crate::settings::{inertia_tensor_elem_box, inertia_tensor_translate};
use crate::settings_geom::{hierarchy_bounding_compute, hierarchy_overall_bounding_compute, hierarchy_overall_inertia_tensor_compute, model_bounding_box_transfer};
use crate::settings_rec::{fcopy, si_dynachnvar_copy, si_dynachnvar_set_default, si_dynajntprop_copy, si_dynajntvar_set_default, si_dynamatter_copy};
use crate::settings_rec::{si_hierarchy_check_empty_phy_props, si_hierarchy_check_phy_props_consistancy};
use si_mem::{Mem, Outside, Ptr32};
use si_x87::{x, F80};

/// `si_model::model_image::bounds::compute_bounding` of a model address.
fn bound(m: &mut Mem, model: u32) {
    si_model::model_image::bounds::compute_bounding(m, Ptr32::new(model));
}

/// The record-pointer mark `or BYTE PTR [model+0xb], 2` the editor looks at.
fn mark(m: &mut Mem, model: u32) {
    let b = m.u8(model + 0xb);
    m.set_u8(model + 0xb, b | 2);
}

/// [soft 0x0849f40c SI_modelKineStateRecordState]: when the model has a kinematic state record (`+0x2f0`), its global
/// position goes into the record's first three floats (`SI_modelGlobalState`, the scaling and the angles sharing one local)
/// and the 3 x 3 part of the matrix at `+0x178` into the record from `+0xc`.
pub fn si_model_kine_state_record_state(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let fr = m.local(0x20);
    let loc = fr + 0x20 - 0xc;
    let k = m.u32(model + 0x2f0);
    if k != 0 {
        o.si_model_global_state(m, model, loc, loc, k);
        for i in 0..3u32 {
            for j in 0..3u32 {
                fcopy(m, model + 0x178 + 16 * i + 4 * j, k + 0xc + 12 * i + 4 * j);
            }
        }
    }
    m.release(0x20);
}

/// [soft 0x0849fa88 SI_dynajntvarAllocate]: a joint variable record (0x274 bytes) with its defaults; three words are set
/// to 0 first (`+0x260`, `+0x25c`, `+0x264`).
pub fn si_dynajntvar_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(0x274);
    if p != 0 {
        m.set_u32(p + 0x260, 0);
        m.set_u32(p + 0x25c, 0);
        m.set_u32(p + 0x264, 0);
        si_dynajntvar_set_default(m, p);
    }
    p
}

/// [soft 0x0849fad8 SI_dynachnvarAllocate]: a chain variable record (0xbc bytes), `+0xb8` set to 0, then its defaults for
/// the 16-bit word.
pub fn si_dynachnvar_allocate(m: &mut Mem, word: i16) -> u32 {
    let p = m.si_malloc(0xbc);
    if p != 0 {
        m.set_u32(p + 0xb8, 0);
        si_dynachnvar_set_default(m, p, word);
    }
    p
}

/// [soft 0x0849fb58 dynageometryTransfer]: the mass and its inverse (`+4`, `+8` of the matter) are copied from the matter of
/// `p1` to that of `p2`. When `p1`'s child's sibling is `p2` (the two are a body and the joint after it) nothing else is
/// done; when `p2`'s child's sibling is `p1` the inertia of `p1`'s hierarchy is taken to the centre of `p2`'s matter
/// (`hierarchyOverallInertiaTensorCompute`, then `inertiaTensorTranslate` by the centre negated); otherwise the box
/// (`+0x10..+0x24`), the centre and both inertia tensors are copied.
///
/// The record `SI_dynachnvarAllocate(0)` makes when `p1` has no settings is discarded, as in the original (a leak).
pub fn dynageometry_transfer(m: &mut Mem, p1: u32, p2: u32) {
    let fr = m.local(0xcc);
    let ebp = fr + 0xcc;
    let (a, b, c) = (m.u32(p1 + 0x2f4), m.u32(p2 + 0x2f4), m.u32(p1 + 0x304));
    fcopy(m, a + 4, b + 4);
    fcopy(m, a + 8, b + 8);
    let mut cx = false;
    let k2 = m.u32(p2 + 0x2c4);
    if k2 != 0 {
        let s = m.u32(k2 + 0x2c0);
        cx = s != 0 && s == p1;
    }
    let mut dx = false;
    let k1 = m.u32(p1 + 0x2c4);
    if k1 != 0 {
        let s = m.u32(k1 + 0x2c0);
        dx = s != 0 && s == p2;
    }
    if cx {
        if c == 0 {
            si_dynachnvar_allocate(m, 0);
        }
        model_bounding_box_transfer(m, p1, p2);
        let (l30, l54, lbc, lb8) = (ebp - 0x30, ebp - 0x54, ebp - 0xbc, ebp - 0xb8);
        hierarchy_overall_inertia_tensor_compute(m, p1, l30, l54, lbc, lb8);
        fcopy(m, lbc, b + 8);
        fcopy(m, lb8, b + 4);
        for k in 0..9u32 {
            fcopy(m, l54 + 4 * k, b + 0x50 + 4 * k);
        }
        let mut d = [0f32; 3];
        for (i, di) in d.iter_mut().enumerate() {
            *di = (-x(m.f32(b + 0x20 + 4 * i as u32))).to_f32();
        }
        let s = m.f32(b + 8);
        inertia_tensor_translate(m, b + 0x50, s, d, 1, b + 0x2c);
    } else if !dx {
        for k in 0..6u32 {
            fcopy(m, p1 + 0x10 + 4 * k, p2 + 0x10 + 4 * k);
        }
        for k in 0..3u32 {
            fcopy(m, a + 0x20 + 4 * k, b + 0x20 + 4 * k);
        }
        for k in 0..9u32 {
            fcopy(m, a + 0x2c + 4 * k, b + 0x2c + 4 * k);
        }
        for k in 0..9u32 {
            fcopy(m, a + 0x50 + 4 * k, b + 0x50 + 4 * k);
        }
    }
    m.release(0xcc);
}

/// [soft 0x084a02b8 SI_modelVolume]: the volume of a model, in the x87 register (returned as the float it holds). A mesh
/// (kind 4) is measured, a patch (1) or a NURBS surface (10) is converted to a mesh first (the patch with both its
/// subdivisions set to 1 for the call, the surface in a copy of the node with `+0x70` and `+0x74` set to 1), a metaball
/// (8) is a sphere of its radius, and any other kind is the lesser of the sphere of the radius at `+0x2c` and the box
/// at `+0x10..+0x24`. With `flag` the volume is multiplied by the absolute value of the product of the scales
/// (`+0x16c`, `+0x170`, `+0x174`). A volume under 0.005 (the double) becomes the float 0.005.
pub fn si_model_volume(m: &mut Mem, o: &mut dyn Outside, model: u32, flag: i16) -> f32 {
    let fr = m.local(0x3c);
    let ebp = fr + 0x3c;
    let (vol, surf) = (ebp - 0x18, ebp - 0x14);
    let c005 = m.f32(0x0866_0b60);
    m.set_f32(vol, c005);
    let sphere = |m: &Mem, r: F80| {
        let c = x(m.f64(0x0866_0b68));
        r * ((r * c) * r)
    };
    let kind = m.u32(model + 0x30);
    let lookup = |m: &Mem, n: u32| {
        let t = m.u32(n + 0x38);
        let i = m.u16(n + 0x36) as u32;
        m.u32(t + 4 * i)
    };
    match kind {
        4 => {
            let mesh = lookup(m, model);
            o.si_mesh_computes_volume_surface(m, mesh, vol, surf);
        }
        8 => {
            let r = x(m.f64(m.u32(model + 0x48) + 0xcc));
            m.set_f32(vol, sphere(m, r).to_f32());
        }
        1 => {
            let patch = lookup(m, model);
            let (s1, s2) = (m.u16(patch + 0x1c), m.u16(patch + 0x1e));
            m.set_u16(patch + 0x1c, 1);
            m.set_u16(patch + 0x1e, 1);
            let mesh = o.si_patch_convert_to_mesh(m, model, patch);
            m.set_u32(ebp - 0x1c, mesh);
            m.set_u16(patch + 0x1c, s1);
            m.set_u16(patch + 0x1e, s2);
            if mesh != 0 {
                o.si_mesh_computes_volume_surface(m, mesh, vol, surf);
                o.si_mesh_dispose(m, ebp - 0x1c);
            }
        }
        10 => {
            let n = crate::particle_outside::ParticleCalls::si_model_copy_node(&mut *o, m, model, 1);
            m.set_u32(ebp - 0x24, n);
            if n != 0 {
                let srf = lookup(m, n);
                m.set_u32(srf + 0x70, 1);
                m.set_u32(srf + 0x74, 1);
                let mesh = o.si_nurbs_convert_to_mesh(m, n, srf);
                m.set_u32(ebp - 0x20, mesh);
                if mesh != 0 {
                    o.si_mesh_computes_volume_surface(m, mesh, vol, surf);
                    o.si_mesh_dispose(m, ebp - 0x20);
                }
                o.si_model_dispose(m, ebp - 0x24, 0);
            }
        }
        _ => {
            let f = |a: u32| x(m.f32(model + a));
            let boxv = ((f(0x20) - f(0x14)) * (f(0x1c) - f(0x10))) * (f(0x24) - f(0x18));
            let r = x(m.f32(model + 0x2c));
            let sph = sphere(m, r).to_f32();
            // `fcom`: the sphere is kept unless it is greater than the box (an unordered pair keeps it too)
            let v = if x(sph).gt(boxv) { boxv.to_f32() } else { sph };
            m.set_f32(vol, v);
        }
    }
    if flag != 0 {
        let s = ((x(m.f32(model + 0x16c)) * x(m.f32(model + 0x170))) * x(m.f32(model + 0x174))).abs();
        let v = (s * x(m.f32(vol))).to_f32();
        m.set_f32(vol, v);
    }
    let v = x(m.f32(vol));
    // `fcom` with the double 0.005: below it the float constant replaces the volume (unordered keeps it)
    let r = if v.lt(x(m.f64(0x0866_0b70))) { x(m.f32(0x0866_0b60)) } else { v };
    let out = r.to_f32();
    m.set_f32(vol, out);
    m.release(0x3c);
    out
}

/// [soft 0x084a04bc dynaGeometryModelUpdate]: marks the model changed (`+0xb` bit 1) and refreshes its matter. Kind 0 stops
/// there. Kinds 1, 2, 4 and 10 are bounded; a joint (5) gets the box from -0.5 to 0.5 around the origin on y and z and a
/// radius that is its child's `+0x158` (1 when it has none) but at least 0.5; a metaball (8) the box of its radius. The
/// centre of the matter (`+0x20`) is the centroid of a mesh (kind 4) or the middle of the box, then times the scales; the
/// mass is `SI_modelVolume(model, 1)` at `+4` and times the density at `+8`, and the two tensors are the boxes
/// (scaled, around the origin, and shifted by the centre).
pub fn dyna_geometry_model_update(m: &mut Mem, o: &mut dyn Outside, model: u32, matter: u32) {
    let _fr = m.local(0x30);
    let kind = m.u32(model + 0x30);
    mark(m, model);
    if kind == 0 {
        m.release(0x30);
        return;
    }
    match kind {
        1 | 2 | 4 | 10 => bound(m, model),
        8 => {
            let def = m.u32(model + 0x48);
            for (i, neg) in [(0x10u32, true), (0x14, true), (0x18, true), (0x1c, false), (0x20, false), (0x24, false)] {
                let r = x(m.f64(def + 0xcc));
                m.set_f32(model + i, if neg { (-r).to_f32() } else { r.to_f32() });
            }
        }
        5 => {
            m.set_u32(model + 0x10, 0);
            let (c1, c2) = (m.f32(0x0866_0b78), m.f32(0x0866_0b7c));
            m.set_f32(model + 0x18, c1);
            m.set_f32(model + 0x14, c1);
            m.set_f32(model + 0x24, c2);
            m.set_f32(model + 0x20, c2);
            let child = m.u32(model + 0x2c4);
            if child != 0 {
                fcopy(m, child + 0x158, model + 0x1c);
            } else {
                m.set_u32(model + 0x1c, 0x3f80_0000);
            }
            let v = x(m.f32(model + 0x1c));
            let r = if v.gt(x(m.f64(0x0866_0b80))) { v } else { x(m.f32(0x0866_0b7c)) };
            m.set_f32(model + 0x2c, r.to_f32());
            let b = m.u8(model + 8);
            m.set_u8(model + 8, b | 1);
        }
        _ => {}
    }
    if kind == 4 {
        let mesh = m.u32(m.u32(model + 0x38) + 4 * m.u16(model + 0x36) as u32);
        o.si_mesh_centroid(m, mesh, matter + 0x20);
    } else {
        let f = |m: &Mem, a: u32| x(m.f32(model + a));
        let h = x(m.f64(0x0866_0b80));
        let (t1, t2, t3) = (f(m, 0x10) + f(m, 0x1c), f(m, 0x14) + f(m, 0x20), f(m, 0x18) + f(m, 0x24));
        m.set_f32(matter + 0x20, (t1 * h).to_f32());
        m.set_f32(matter + 0x28, (t3 * h).to_f32());
        m.set_f32(matter + 0x24, (h * t2).to_f32());
    }
    for (s, c) in [(0x16cu32, 0x20u32), (0x170, 0x24), (0x174, 0x28)] {
        let v = (x(m.f32(model + s)) * x(m.f32(matter + c))).to_f32();
        m.set_f32(matter + c, v);
    }
    let v = x(si_model_volume(m, o, model, 1));
    m.set_f32(matter + 4, v.to_f32());
    let w = (v * x(m.f32(matter))).to_f32();
    m.set_f32(matter + 8, w);
    // the corners, scaled: (x10 s16c, x14 s170, x18 s174) and (s16c x1c, s170 x20, s174 x24)
    let g = |m: &Mem, a: u32| x(m.f32(model + a));
    let (sx, sy, sz) = (g(m, 0x16c), g(m, 0x170), g(m, 0x174));
    let mut lo = [(g(m, 0x10) * sx).to_f32(), (g(m, 0x14) * sy).to_f32(), (g(m, 0x18) * sz).to_f32()];
    let mut hi = [(sx * g(m, 0x1c)).to_f32(), (sy * g(m, 0x20)).to_f32(), (sz * g(m, 0x24)).to_f32()];
    let scale = m.f32(matter);
    inertia_tensor_elem_box(m, lo, hi, scale, matter + 0x2c);
    let c = |m: &Mem, a: u32| x(m.f32(matter + a));
    lo[0] = (x(lo[0]) - c(m, 0x20)).to_f32();
    lo[1] = (x(lo[1]) - c(m, 0x24)).to_f32();
    hi[0] = (x(hi[0]) - c(m, 0x20)).to_f32();
    hi[1] = (x(hi[1]) - c(m, 0x24)).to_f32();
    lo[2] = (x(lo[2]) - c(m, 0x28)).to_f32();
    hi[2] = (x(hi[2]) - c(m, 0x28)).to_f32();
    let scale = m.f32(matter);
    inertia_tensor_elem_box(m, lo, hi, scale, matter + 0x50);
    m.release(0x30);
}

/// [soft 0x084a072c dynaGeometryHierarchyNodeCompute]: adds the model's matter to the weighted centre `vec` (three floats)
/// and the weight `wt`: the matter's centre (`+0x20`) goes through the model's global matrix (`+0x178 .. +0x1c0`, nothing
/// rounded before the product by the mass at `+8`), then the same for the child and the sibling. A model with a matter is
/// updated first ([`dyna_geometry_model_update`]) unless it is a joint (kind 5) whose child's sibling has a matter of its own.
pub fn dyna_geometry_hierarchy_node_compute(m: &mut Mem, o: &mut dyn Outside, model: u32, vec: u32, wt: u32) {
    let matter = m.u32(model + 0x2f4);
    let mut cond = matter != 0;
    if cond && m.u32(model + 0x30) == 5 {
        let c = m.u32(model + 0x2c4);
        let s = m.u32(c + 0x2c0);
        cond = s == 0 || m.u32(s + 0x2f4) == 0;
    }
    if cond {
        dyna_geometry_model_update(m, o, model, matter);
        let g = |m: &Mem, a: u32| x(m.f32(model + a));
        let (mx, my, mz) = (x(m.f32(matter + 0x20)), x(m.f32(matter + 0x24)), x(m.f32(matter + 0x28)));
        let t1 = (((mz * g(m, 0x198)) + ((my * g(m, 0x188)) + (mx * g(m, 0x178)))) + g(m, 0x1a8)) + g(m, 0x1b8);
        let t2 = (((mz * g(m, 0x19c)) + ((my * g(m, 0x18c)) + (mx * g(m, 0x17c)))) + g(m, 0x1ac)) + g(m, 0x1bc);
        let t3 = (((mz * g(m, 0x1a0)) + ((my * g(m, 0x190)) + (mx * g(m, 0x180)))) + g(m, 0x1b0)) + g(m, 0x1c0);
        let w = x(m.f32(matter + 8));
        for (i, t) in [t1, t2, t3].into_iter().enumerate() {
            let a = vec + 4 * i as u32;
            let v = (t * w + x(m.f32(a))).to_f32();
            m.set_f32(a, v);
        }
        let nw = (x(m.f32(wt)) + w).to_f32();
        m.set_f32(wt, nw);
    }
    let c = m.u32(model + 0x2c4);
    if c != 0 {
        dyna_geometry_hierarchy_node_compute(m, o, c, vec, wt);
    }
    let s = m.u32(model + 0x2c0);
    if s != 0 {
        dyna_geometry_hierarchy_node_compute(m, o, s, vec, wt);
    }
}

/// [soft 0x084a08a8 dynaGeometryHierarchyCompute]: the mass centre of a hierarchy: the weighted sum
/// ([`dyna_geometry_hierarchy_node_compute`]) divided by the weight (unless the weight is 0); the weight goes to `wt_out`.
pub fn dyna_geometry_hierarchy_compute(m: &mut Mem, o: &mut dyn Outside, model: u32, vec_out: u32, wt_out: u32) {
    let fr = m.local(0x30);
    let ebp = fr + 0x30;
    let (sum, w) = (ebp - 0x14, ebp - 0x20);
    for k in 0..3u32 {
        m.set_u32(sum + 4 * k, 0);
    }
    m.set_u32(w, 0);
    dyna_geometry_hierarchy_node_compute(m, o, model, sum, w);
    fcopy(m, w, wt_out);
    if m.f32(w) != 0.0 {
        let inv = x(1.0f32) / x(m.f32(w));
        for k in 0..3u32 {
            let v = (inv * x(m.f32(sum + 4 * k))).to_f32();
            m.set_f32(vec_out + 4 * k, v);
        }
    }
    m.release(0x30);
}

/// [soft 0x084a0920 SI_dynaGeometryUpdate]: refreshes the geometry records of a body. A joint (kind 5) whose child's sibling
/// has a matter takes it over: the bounding of that sibling's hierarchy, its mass centre taken to the joint's frame (the
/// joint's `+0x1b8..` position subtracted, the rotation `+0x178..` applied), the matter copied and moved
/// ([`dynageometry_transfer`]). Otherwise a joint with no such sibling, and every body that is not a joint, is updated; the
/// others compute the overall bounding box, the mass centre and the inertia of their hierarchy into the settings record
/// (`+0x304`: weight `+0x60`, centre `+0x64..`, tensors `+0x70` and `+0x94`).
pub fn si_dyna_geometry_update(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let fr = m.local(0x3c);
    let ebp = fr + 0x3c;
    let matter = m.u32(model + 0x2f4);
    if matter == 0 {
        m.release(0x3c);
        return;
    }
    let g = |m: &Mem, a: u32| x(m.f32(model + a));
    let (v, wv) = (ebp - 0x14, ebp - 0x20);
    let rot = |m: &Mem| -> [[F80; 3]; 3] {
        let f = |a: u32| x(m.f32(model + a));
        [[f(0x178), f(0x17c), f(0x180)], [f(0x188), f(0x18c), f(0x190)], [f(0x198), f(0x19c), f(0x1a0)]]
    };
    if m.u32(model + 0x30) == 5 {
        let c = m.u32(model + 0x2c4);
        let p2 = m.u32(c + 0x2c0);
        if p2 != 0 {
            hierarchy_bounding_compute(m, p2);
            let m2 = m.u32(p2 + 0x2f4);
            if m2 != 0 {
                dyna_geometry_hierarchy_compute(m, o, p2, v, wv);
                let t1 = x(m.f32(v)) - g(m, 0x1b8);
                m.set_f32(matter + 0x20, t1.to_f32());
                let t2 = x(m.f32(v + 4)) - g(m, 0x1bc);
                m.set_f32(matter + 0x24, t2.to_f32());
                let t3 = x(m.f32(v + 8)) - g(m, 0x1c0);
                m.set_f32(matter + 0x28, t3.to_f32());
                let r = rot(m);
                let n1 = (((r[0][0] * t1) + (r[0][1] * t2)) + (r[0][2] * t3)).to_f32();
                let n2 = (((r[1][0] * t1) + (r[1][1] * t2)) + (r[1][2] * t3)).to_f32();
                let n3 = (((t2 * r[2][1]) + (t1 * r[2][0])) + (t3 * r[2][2])).to_f32();
                m.set_f32(matter + 0x20, n1);
                m.set_f32(matter + 0x24, n2);
                m.set_f32(matter + 0x28, n3);
                si_dynamatter_copy(m, m2, matter);
                dynageometry_transfer(m, p2, model);
                let b = m.u8(model + 8);
                m.set_u8(model + 8, b | 1);
                m.release(0x3c);
                return;
            }
        }
        dyna_geometry_model_update(m, o, model, matter);
        m.release(0x3c);
        return;
    }
    let (lo, hi) = (ebp - 0x38, ebp - 0x2c);
    hierarchy_overall_bounding_compute(m, model, lo, hi);
    dyna_geometry_hierarchy_compute(m, o, model, v, wv);
    let prop = m.u32(model + 0x304);
    if prop != 0 {
        let t1 = x(m.f32(v)) - g(m, 0x1b8);
        m.set_f32(prop + 0x64, t1.to_f32());
        let t2 = x(m.f32(v + 4)) - g(m, 0x1bc);
        m.set_f32(prop + 0x68, t2.to_f32());
        let t3 = x(m.f32(v + 8)) - g(m, 0x1c0);
        m.set_f32(prop + 0x6c, t3.to_f32());
        let r = rot(m);
        let n1 = (((r[0][0] * t1) + (r[0][1] * t2)) + (r[0][2] * t3)).to_f32();
        let n2 = (((r[1][0] * t1) + (r[1][1] * t2)) + (r[1][2] * t3)).to_f32();
        let n3 = (((t2 * r[2][1]) + (t1 * r[2][0])) + (t3 * r[2][2])).to_f32();
        m.set_f32(prop + 0x64, n1);
        m.set_f32(prop + 0x68, n2);
        m.set_f32(prop + 0x6c, n3);
        fcopy(m, wv, prop + 0x60);
        let junk = ebp - 0x3c;
        hierarchy_overall_inertia_tensor_compute(m, model, prop + 0x70, prop + 0x94, wv, junk);
    }
    m.release(0x3c);
}

/// [soft 0x084a0b30 SI_dynachnGeometryUpdate]: [`si_dyna_geometry_update`] of every body of the chain (the head is skipped
/// when it is the null of kind 0; links `+0x30c`). For the null head, the weight at `+0x60` of its settings is the sum of the
/// bodies' masses, and the centre at `+0x64..` their mass centre in the head's frame; finally the settings' flag
/// `+4` is set to 1.
pub fn si_dynachn_geometry_update(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let fr = m.local(0x30);
    let _ = fr;
    let start = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    let mut b = start;
    while b != 0 {
        si_dyna_geometry_update(m, o, b);
        b = m.u32(b + 0x30c);
    }
    if m.u32(model + 0x30) == 0 {
        let prop = m.u32(model + 0x304);
        m.set_u32(prop + 0x60, 0);
        let mut s = [F80::ZERO; 3];
        // uninitialised in the original when the chain has no body (the spec always gives it one); 0 here
        let mut ctr = [0f32; 3];
        let mut b = start;
        while b != 0 {
            let matter = m.u32(b + 0x2f4);
            let w = x(m.f32(matter + 8));
            let (mx, my, mz) = (x(m.f32(matter + 0x20)), x(m.f32(matter + 0x24)), x(m.f32(matter + 0x28)));
            let g = |a: u32| x(m.f32(b + a));
            let t1 = (((my * g(0x188)) + (mx * g(0x178))) + (mz * g(0x198))) + g(0x1a8);
            let t2 = (((mx * g(0x17c)) + (my * g(0x18c))) + (mz * g(0x19c))) + g(0x1ac);
            let t3 = ((mz * g(0x1a0)) + ((my * g(0x190)) + (mx * g(0x180)))) + g(0x1b0);
            let (t4, t5, t6) = (t1 + g(0x1b8), t2 + g(0x1bc), t3 + g(0x1c0));
            let (t7, t8, t9) = (t4 * w, t5 * w, t6 * w);
            ctr = [t7.to_f32(), t8.to_f32(), t9.to_f32()];
            s[0] = (t7 + s[0]).to_f32().into();
            s[1] = (t8 + s[1]).to_f32().into();
            s[2] = (t9 + s[2]).to_f32().into();
            let nw = (x(m.f32(prop + 0x60)) + w).to_f32();
            m.set_f32(prop + 0x60, nw);
            b = m.u32(b + 0x30c);
        }
        let w = m.f32(prop + 0x60);
        if w > 0.0 {
            let inv = x(1.0f32) / x(w);
            for k in 0..3 {
                ctr[k] = (inv * s[k]).to_f32();
            }
        }
        let gm = |a: u32| x(m.f32(model + a));
        let p = [
            (x(ctr[0]) - gm(0x1b8)).to_f32(),
            (x(ctr[1]) - gm(0x1bc)).to_f32(),
            (x(ctr[2]) - gm(0x1c0)).to_f32(),
        ];
        let (p64, p68, p6c) = (x(p[0]), x(p[1]), x(p[2]));
        let n1 = (((gm(0x17c) * p68) + (gm(0x178) * p64)) + (gm(0x180) * p6c)).to_f32();
        let n2 = (((gm(0x188) * p64) + (gm(0x18c) * p68)) + (gm(0x190) * p6c)).to_f32();
        let n3 = ((p6c * gm(0x1a0)) + ((p68 * gm(0x19c)) + (p64 * gm(0x198)))).to_f32();
        m.set_f32(prop + 0x64, n1);
        m.set_f32(prop + 0x68, n2);
        m.set_f32(prop + 0x6c, n3);
    }
    let prop = m.u32(model + 0x304);
    m.set_u16(prop + 4, 1);
    m.release(0x30);
}

/// [soft 0x084a10c4 SI_dynaLinkRootToGravity]: when the model has an animation element, the gravity constraint (kind 10) is
/// looked for in its list (`+0x2fc`, next link `+0x3c`); the constraint model for kind 10 (`SI_dynaConstraintGet`) is
/// added to the list unless the model has the constraint already, and selected.
pub fn si_dyna_link_root_to_gravity(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let fr = m.local(0xc);
    let loc = fr + 0xc - 4;
    let mut found = false;
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    if elem != 0 {
        let mut e = m.u32(model + 0x2fc);
        while e != 0 {
            if m.u32(e + 4) == 10 {
                found = true;
            }
            e = m.u32(e + 0x3c);
            if e == 0 || found {
                break;
            }
        }
        let g = o.si_dyna_constraint_get(m, 10, 0);
        if g != 0 {
            if !found {
                let c = o.si_conslist_constr_allocate(m, 10, -1, g);
                if c != 0 {
                    o.si_conslist_add_constraint(m, model, c, 0);
                }
            }
            o.si_ed_select_dyna_constraint(m, model, g, 10);
        }
    }
    m.release(0xc);
}

/// [soft 0x084a1160 SI_modelDynaInfoCopy]: gives `p2` a copy of the dynamics records of `p1` (`+0x2f0` kinematic state, new;
/// `+0x2f4` matter, copied and moved; `+0x2f8` joint properties, copied; `+0x300` joint variables, new; `+0x304`
/// chain variables, copied with the flag 1), marking each changed; and, when `p1` has chain variables, links `p2` to gravity.
pub fn si_model_dyna_info_copy(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32) {
    if m.u32(p1 + 0x304) != 0 {
        si_dyna_link_root_to_gravity(m, o, p2);
    }
    if m.u32(p1 + 0x2f0) != 0 {
        let k = si_kine::image::qstretch::kine_state_allocate(m);
        m.set_u32(p2 + 0x2f0, k);
    }
    if m.u32(p1 + 0x2f4) != 0 {
        let n = si_hrc::image::chunks::dynamatter_allocate(m);
        m.set_u32(p2 + 0x2f4, n);
        let src = m.u32(p1 + 0x2f4);
        si_dynamatter_copy(m, src, n);
        dynageometry_transfer(m, p1, p2);
        mark(m, p2);
    }
    if m.u32(p1 + 0x2f8) != 0 {
        let n = si_hrc::image::chunks::dynajntprop_allocate(m);
        m.set_u32(p2 + 0x2f8, n);
        let src = m.u32(p1 + 0x2f8);
        si_dynajntprop_copy(m, src, n);
        mark(m, p2);
    }
    if m.u32(p1 + 0x300) != 0 {
        let n = si_dynajntvar_allocate(m);
        m.set_u32(p2 + 0x300, n);
    }
    if m.u32(p1 + 0x304) != 0 {
        let w = m.u16(m.u32(p1 + 0x304) + 0x38) as i16;
        let n = si_dynachnvar_allocate(m, w);
        m.set_u32(p2 + 0x304, n);
        let src = m.u32(p1 + 0x304);
        si_dynachnvar_copy(m, src, n, 1);
        mark(m, p2);
    }
}

/// [soft 0x084a1248 SI_modelDynaInfoDispose]: frees the dynamics records of a model. With chain variables and a non-zero
/// `flag` its simulation and transition curves go first; a constraint list is disposed; the records `+0x2f0`, `+0x2f4`,
/// `+0x2f8`, `+0x300`, `+0x304` (the last three marked changed except `+0x300`) and `+0x308` are freed and cleared.
/// When the model has an animation element the function curve flags of the dynamics attributes (0xb2, 0xb6, 0xb3, 0xb4, 0xb5)
/// are set and the element's animation is reset.
pub fn si_model_dyna_info_dispose(m: &mut Mem, o: &mut dyn Outside, model: u32, flag: i16) {
    let fr = m.local(0x14);
    let loc = fr + 0x14 - 4;
    if m.u32(model + 0x304) != 0 && flag != 0 {
        o.si_dyna_simulation_dispose(m, model, 0);
        o.si_dyna_transition_crv_dispose(m, model);
    }
    if m.u32(model + 0x2fc) != 0 {
        o.si_conslist_dispose(m, model, 1, 0);
    }
    for (off, marks) in [(0x2f0u32, false), (0x2f4, true), (0x2f8, true), (0x300, false), (0x304, true), (0x308, false)] {
        let p = m.u32(model + off);
        if p != 0 {
            m.free(p);
            m.set_u32(model + off, 0);
            if marks {
                mark(m, model);
            }
        }
    }
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    if elem != 0 {
        o.si_fcv_table_set(m, 0);
        for id in [0xb2u32, 0xb6, 0xb3, 0xb4, 0xb5] {
            o.si_attdict_set_fcurve_flag(m, 0x0878_1438, id, 1);
        }
        let pd = m.u32(0x0867_9314);
        let n = m.u32(pd);
        let dict = n.wrapping_mul(7).wrapping_mul(16).wrapping_add(m.u32(0x0867_931c));
        let cid = m.u32(0x0878_1680);
        let l = m.u32(loc);
        o.si_elemdict_reset_elem_anim(m, dict, elem, cid, l, 1);
    }
    m.release(0x14);
}

/// [soft 0x084a15a4 SI_dynaPhyPropsSetNewRoot]: a model with a matter that already has chain variables is only linked to
/// gravity; one without them takes them from the nearest ancestor that has chain variables and joint variables:
/// missing records are allocated (chain variables for the word 1 if the model is the null of kind 0), the chain variables and
/// joint properties copied, the model linked to gravity and made the head of its own chain (`+0x314` itself, `+0x30c` and
/// `+0x310` cleared). Returns 1.
pub fn si_dyna_phy_props_set_new_root(m: &mut Mem, o: &mut dyn Outside, model: u32) -> i32 {
    if m.u32(model + 0x2f4) != 0 {
        if m.u32(model + 0x304) != 0 {
            si_dyna_link_root_to_gravity(m, o, model);
        } else {
            let mut p4 = model;
            loop {
                p4 = m.u32(p4 + 0x2bc);
                if p4 == 0 || m.u32(p4 + 0x304) != 0 {
                    break;
                }
            }
            let mut p3 = model;
            if m.u32(model + 0x300) == 0 {
                loop {
                    p3 = m.u32(p3 + 0x2bc);
                    if p3 == 0 || m.u32(p3 + 0x300) != 0 {
                        break;
                    }
                }
            }
            if p4 != 0 && p3 != 0 {
                if m.u32(model + 0x304) == 0 {
                    let w = (m.u32(model + 0x30) == 0) as i16;
                    let n = si_dynachnvar_allocate(m, w);
                    m.set_u32(model + 0x304, n);
                }
                if m.u32(model + 0x2f8) == 0 {
                    let n = si_hrc::image::chunks::dynajntprop_allocate(m);
                    m.set_u32(model + 0x2f8, n);
                }
                if m.u32(model + 0x300) == 0 {
                    let n = si_dynajntvar_allocate(m);
                    m.set_u32(model + 0x300, n);
                }
                if m.u32(model + 0x2f0) == 0 {
                    let n = si_kine::image::qstretch::kine_state_allocate(m);
                    m.set_u32(model + 0x2f0, n);
                }
                let (s, d) = (m.u32(p4 + 0x304), m.u32(model + 0x304));
                si_dynachnvar_copy(m, s, d, 0);
                let (s, d) = (m.u32(p3 + 0x2f8), m.u32(model + 0x2f8));
                si_dynajntprop_copy(m, s, d);
                mark(m, model);
                si_dyna_link_root_to_gravity(m, o, model);
                m.set_u32(model + 0x314, model);
                m.set_u32(model + 0x30c, 0);
                m.set_u32(model + 0x310, 0);
            }
        }
    }
    1
}

/// [soft 0x084a16f8 SI_dynaPhyPropsValidateCopyDelete]: whether a model's physical properties may be copied or deleted
/// (`sel` the selection state, `f` the mode). Always yes for the null of kind 0 with the algorithm 1 or 2 (`+0x2ec`);
/// otherwise by [`si_hierarchy_check_empty_phy_props`]: in mode 0, no for a model with no matter and nothing below it, and
/// no for a model that has a matter, no father but a child while `sel` is 1; in the other modes, no for a model with no matter
/// and nothing below it unless `sel` is 1.
pub fn si_dyna_phy_props_validate_copy_delete(m: &mut Mem, model: u32, sel: i32, f: i16) -> i32 {
    let fr = m.local(0xc);
    let loc = fr + 0xc - 2;
    let r;
    if m.u32(model + 0x30) == 0 && m.u32(model + 0x2ec).wrapping_sub(1) <= 1 {
        r = 1;
    } else {
        let s = si_hierarchy_check_empty_phy_props(m, model, loc) as i16;
        let l = m.u16(loc) as i16;
        r = if f != 0 {
            if m.u32(model + 0x2f4) != 0 || s != 0 || l != 0 || sel == 1 { 1 } else { 0 }
        } else if m.u32(model + 0x2f4) == 0 {
            if s != 0 || l != 0 { 1 } else { 0 }
        } else if m.u32(model + 0x2bc) != 0 || m.u32(model + 0x2c4) == 0 || sel != 1 {
            1
        } else {
            0
        };
    }
    m.release(0xc);
    r
}

/// [soft 0x084a1794 SI_dynaPhyPropsValidateParent]: whether `p2` may become a child of `p1` in the dynamics hierarchy
/// (1 yes without changes, 0 no, -1 the user refused the warning, 1 after the records of `p2` were freed). Yes without
/// change when `p2` has a brother, is the null of kind 0 with the algorithm 1 or 2, or `p1` is the null of kind 0 with the
/// algorithm 3 or 4, or `p2` holds properties. Otherwise the parent is climbed to the top of its chain (a joint of kind 5 ends
/// the climb); both need consistent properties; a running simulation of `p2` is lost after a warning; then the constraint
/// list, the kinematic state of a body, the joint properties and variables and the chain variables of `p2` are freed.
pub fn si_dyna_phy_props_validate_parent(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32) -> i32 {
    let fr = m.local(0xc);
    let loc = fr + 0xc - 2;
    let mut ret = 1;
    let trivial = m.u32(p2 + 0x2c0) != 0
        || (m.u32(p2 + 0x30) == 0 && m.u32(p2 + 0x2ec).wrapping_sub(1) <= 1)
        || (m.u32(p1 + 0x30) == 0 && m.u32(p1 + 0x2ec).wrapping_sub(3) <= 1);
    if !trivial {
        let s = si_hierarchy_check_empty_phy_props(m, p2, loc) as i16;
        if s == 0 {
            if m.u32(p2 + 0x304) == 0 || m.u32(p1 + 0x2f4) != 0 {
                let mut b = p1;
                loop {
                    let f = m.u32(b + 0x2bc);
                    if f == 0 || m.u32(f + 0x30) == 5 {
                        break;
                    }
                    b = f;
                }
                let s = si_hierarchy_check_empty_phy_props(m, b, loc) as i16;
                ret = 0;
                if s == 0 {
                    let mut u = 0;
                    if si_hierarchy_check_phy_props_consistancy(m, 0, b, 1, 1) as i16 != 0 {
                        u = (si_hierarchy_check_phy_props_consistancy(m, 0, p2, 1, 1) as i16 != 0) as i32;
                    }
                    if u != 0 {
                        let sv = m.u32(p2 + 0x304);
                        if sv != 0 && m.u16(sv + 0x12) != 0 {
                            let r = o.si_system_warning(m, 0x086d_71c0, 0x086d_7180, 0x086d_7160) as u16;
                            if r == 0 {
                                m.release(0xc);
                                return -1;
                            }
                            o.si_dyna_simulation_dispose(m, p2, 1);
                            o.si_dyna_transition_crv_dispose(m, p2);
                            crate::particle_outside::ParticleCalls::si_refresh_all(&mut *o, m, 1);
                        }
                        if m.u32(p2 + 0x2fc) != 0 {
                            o.si_conslist_dispose(m, p2, 1, 0);
                        }
                        if m.u32(p2 + 0x30) != 0 {
                            let k = m.u32(p2 + 0x2f0);
                            if k != 0 {
                                m.free(k);
                                m.set_u32(p2 + 0x2f0, 0);
                            }
                        }
                        for (off, marks) in [(0x2f8u32, true), (0x300, false), (0x304, true)] {
                            let p = m.u32(p2 + off);
                            if p != 0 {
                                m.free(p);
                                m.set_u32(p2 + off, 0);
                                if marks {
                                    mark(m, p2);
                                }
                            }
                        }
                        ret = u as i16 as i32;
                    }
                }
            } else {
                ret = 0;
            }
        }
    }
    m.release(0xc);
    ret
}
