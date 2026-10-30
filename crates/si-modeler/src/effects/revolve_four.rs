// SPDX-License-Identifier: MIT
//! `revolve.c` and the four-sided effect (finding 0034, "geomupd7"): the nodes a revolution makes
//! from a model, and the four-sided surface effect.
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported is called
//! through [`si_mem::Outside`], written `name (0xADDRESS)`.

use crate::effects::revolution::{face_revolution_to_patchs, mesh_revolve_face, mesh_revolve_spline, SI_MODEL_DISPOSE_NODE};
use crate::extrusion::{create_nurbs_hierarchy_from_nurbs_crv, create_nurbs_srf_hierarchy_from_nurbs_face, create_patch_hierarchy_from_face, ModelSide, SplineSide};
use si_mem::{Mem, Outside, Ptr32};
use si_model::mesh_image;
use si_model::model_image::{defs, setup, tree};

/// The name "revol" the revolution's node is allocated with (0x0868d660 in the binary's data).
pub const REVOL_NAME: u32 = 0x0868_d660;

/// Puts the node name of [`REVOL_NAME`] into the image (the program's loader does it in the original).
pub fn store_names(m: &mut Mem) {
    m.put_cstr(REVOL_NAME, b"revol");
}

/// `SI_modelDisposeNode(&node)`: a hook.
fn dispose_node(m: &mut Mem, o: &mut dyn Outside, cell: u32) {
    o.call(m, SI_MODEL_DISPOSE_NODE, &[cell]);
}

/// A copy of a face [soft 0x08434990 SI_faceCopy]: a new face made by `SI_faceAllocate` with the name
/// duplicated (`strdup`, whose block `SI_malloc` stands in for); a face of NURBS curves (+0x84 = 0)
/// gets a zeroed array of the curve records (100 bytes, `SI_calloc`) each filled by `SI_nurbsCrvInit`
/// from the source's, the count (+0x78) copied and +0x84 cleared; a face of splines has its outline
/// transferred (`SI_splineTransfer`), the count copied, and when it is not 0 an array of hole splines
/// (0x74 bytes, `SI_calloc`, each `SI_splineInit` then `SI_splineTransfer`), +0x84 set to 1. Answers
/// 0 when no face could be allocated.
pub fn face_copy(m: &mut Mem, face: u32) -> u32 {
    let out = si_hrc::image::spline::face_allocate(m).addr();
    if out == 0 {
        return 0;
    }
    let name = m.u32(face);
    if name != 0 {
        let mut n = 0u32;
        while m.u8(name.wrapping_add(n)) != 0 {
            n += 1;
        }
        let copy = m.si_malloc(n + 1);
        for k in 0..=n {
            let b = m.u8(name.wrapping_add(k));
            m.set_u8(copy.wrapping_add(k), b);
        }
        m.set_u32(out, copy);
    }
    let count = m.i16(face + 0x78);
    if m.i16(face + 0x84) == 0 {
        let curves = m.si_calloc(count as i32 as u32, 100);
        m.set_u32(out + 0x80, curves);
        let mut k: i16 = 0;
        while k < count {
            let off = (k as i32 * 100) as u32;
            si_hrc::image::nurbs::nurbs_crv_init(m, curves.wrapping_add(off), m.u32(face + 0x80).wrapping_add(off));
            k = k.wrapping_add(1);
        }
        m.set_i16(out + 0x78, count);
        m.set_i16(out + 0x84, 0);
    } else {
        si_spline::intersect_image::spline_transfer(m, Ptr32::new(face + 4), Ptr32::new(out + 4));
        m.set_i16(out + 0x78, count);
        if count != 0 {
            let holes = m.si_calloc(count as i32 as u32, 0x74);
            m.set_u32(out + 0x7c, holes);
            let mut k: i16 = 0;
            while k < m.i16(out + 0x78) {
                let off = (k as i32 * 0x74) as u32;
                si_hrc::image::spline::spline_init(m, holes.wrapping_add(off));
                si_spline::intersect_image::spline_transfer(m, Ptr32::new(m.u32(face + 0x7c).wrapping_add(off)), Ptr32::new(holes.wrapping_add(off)));
                k = k.wrapping_add(1);
            }
        }
        m.set_i16(out + 0x84, 1);
    }
    out
}

/// Sets the kind (+0x30) of the node the patches made to 1, or of its sons but the caps (kind 2)
/// when the node itself has kind 0 and sons.
fn mark_patch_kind(m: &mut Mem, node: u32) {
    let son = m.u32(node + 0x2c4);
    if son == 0 || m.u32(node + 0x30) != 0 {
        m.set_u32(node + 0x30, 1);
    } else {
        let mut at = son;
        while at != 0 {
            if m.u32(at + 0x30) != 2 {
                m.set_u32(at + 0x30, 1);
            }
            at = m.u32(at + 0x2c0);
        }
    }
}

/// The nodes of a revolution of a model createModelFromRevolution (0x08195b7c), partial in the citation sense: the mesh paths are checked, the patch paths (patch flag set) and the update path (existing node) are not. `src` is
/// the model revolved (kind at +0x30: 2 a face, 6 a spline, 9 a NURBS curve; its definition at
/// +0x48, its matrix at +0x1c4), `model` the axis model, `model_at` the address of a
/// `__SI_Model *` (read as the node to update when not null, written on success), `rev` the
/// revolution record (+0 caps, +4 angle, +8 steps, +0xc axis, +0x10 sampling step, +0x14 the
/// patch flag, +0x18 the patch type, +0x1c the definition word).
///
/// The source's geometry is copied and moved by the model's matrix. With the patch flag zero
/// a mesh results (`meshRevolveFace`/`meshRevolveSpline`; a NURBS curve is sampled into a face
/// first), kind 4; otherwise patches or NURBS surfaces and their hierarchy. Where a mesh or
/// surface is not made the node is disposed when it was new. Answers 1 on every path (an
/// unknown kind included, which makes nothing and leaves `*model_at`).
///
/// The sampled face of a NURBS curve and its parameter array are leaked, as in the original; so
/// are the curves' surfaces array of a face of NURBS curves and the face when a surface fails.
pub fn create_model_from_revolution(m: &mut Mem, o: &mut dyn Outside, src: u32, model: u32, model_at: u32, rev: u32) -> i32 {
    let sp = m.sp();
    let r = revolution_in(m, o, src, model, model_at, rev);
    m.set_sp(sp);
    r
}

fn revolution_in(m: &mut Mem, o: &mut dyn Outside, src: u32, model: u32, model_at: u32, rev: u32) -> i32 {
    // the locals of the original's frame: the node, the spline, the face, the curve, the samples
    let node_c = m.local(4);
    let spline_c = m.local(4);
    let face_c = m.local(4);
    let crv_c = m.local(4);
    let arr_c = m.local(4);
    let cnt_c = m.local(4);
    for c in [spline_c, face_c, crv_c, arr_c, cnt_c] {
        m.set_u32(c, 0);
    }
    let patch_type = m.u32(rev + 0x18);
    let angle = f32::from_bits(m.u32(rev + 4));
    let steps = m.u32(rev + 8);
    let axis = m.u32(rev + 0xc);
    let step = m.u16(rev + 0x10) as i16;
    let mut caps = m.u16(rev) as i16;
    let existing = m.u32(model_at);
    m.set_u32(node_c, existing);
    let kind = m.i32(src + 0x30);
    let mat = src + 0x1c4;
    let (is_face, is_spline, is_crv);
    match kind {
        2 => {
            is_face = true;
            is_spline = false;
            is_crv = false;
            let f = face_copy(m, m.u32(src + 0x48));
            m.set_u32(face_c, f);
            if m.i16(f + 0x84) == 0 {
                let mut k: i16 = 0;
                while k < m.i16(f + 0x78) {
                    let c = m.u32(f + 0x80).wrapping_add((k as i32 * 100) as u32);
                    m.set_u32(crv_c, c);
                    si_nurbs::image::crv_apply_matrix(m, c, mat, 0);
                    k = k.wrapping_add(1);
                }
            } else {
                let mut k: i16 = -1;
                while k < m.i16(f + 0x78) {
                    let s = if k == -1 { f + 4 } else { m.u32(f + 0x7c).wrapping_add((k as i32 * 0x74) as u32) };
                    m.set_u32(spline_c, s);
                    si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), Ptr32::new(s), mat);
                    k = k.wrapping_add(1);
                }
            }
        }
        6 => {
            is_face = false;
            is_spline = true;
            is_crv = false;
            let s = si_spline::intersect_image::spline_copy(m, Ptr32::new(m.u32(src + 0x48))).addr();
            m.set_u32(spline_c, s);
            si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), Ptr32::new(s), mat);
        }
        9 => {
            is_face = false;
            is_spline = false;
            is_crv = true;
            let c = si_nurbs::image::crv_copy(m, m.u32(src + 0x48));
            m.set_u32(crv_c, c);
            si_nurbs::image::crv_apply_matrix(m, c, mat, 0);
        }
        _ => return 1,
    }
    let update: i16 = (existing != 0) as i16;
    if existing == 0 {
        let n = si_hrc::image::node::model_allocate(m, REVOL_NAME).addr();
        m.set_u32(node_c, n);
    }
    let patches = m.u16(rev + 0x14) != 0;
    if is_spline {
        let spline = m.u32(spline_c);
        if !patches {
            let mesh = mesh_revolve_spline(m, o, spline, step, steps as i32, angle, axis as i32, model, caps);
            if mesh == 0 {
                if update == 0 {
                    dispose_node(m, o, node_c);
                }
                si_hrc::image::spline::spline_dispose(m, spline_c);
                return 1;
            }
            let node = Ptr32::new(m.u32(node_c));
            if update == 0 {
                setup::add_definition(m, node, 4, mesh);
            } else {
                defs::substitute_idefinition(m, &mut ModelSide(o), node, 0, mesh);
            }
            m.set_u32(node.addr() + 0x30, 4);
        } else {
            let ok = tree::create_patch_from_revolution_from_spline(m, &mut ModelSide(o), node_c, Ptr32::new(spline), model, rev, update);
            if !ok {
                if update == 0 {
                    dispose_node(m, o, node_c);
                }
                si_hrc::image::spline::spline_dispose(m, spline_c);
                return 1;
            }
            let node = m.u32(node_c);
            mark_patch_kind(m, node);
        }
        si_hrc::image::spline::spline_dispose(m, spline_c);
    } else if is_face {
        let face = m.u32(face_c);
        if !patches {
            let mesh = mesh_revolve_face(m, o, face, step, steps as i32, angle, axis as i32, model, caps);
            if mesh == 0 {
                if update == 0 {
                    dispose_node(m, o, node_c);
                }
                mesh_image::face::face_dispose(m, face_c);
                return 1;
            }
            let node = Ptr32::new(m.u32(node_c));
            if update == 0 {
                setup::add_definition(m, node, 4, mesh);
            } else {
                defs::substitute_idefinition(m, &mut ModelSide(o), node, 0, mesh);
            }
            m.set_u32(node.addr() + 0x30, 4);
        } else {
            // a revolution of 360 degrees or more is closed: no caps (the compare is on the float)
            if f64::from(angle) >= 360.0 {
                caps = 0;
            }
            if m.i16(face + 0x84) == 0 {
                let count = m.i16(face + 0x78);
                let srfs = m.si_malloc(((count as i32) << 2) as u32);
                let mut k: i16 = 0;
                while k < m.i16(face + 0x78) {
                    let c = m.u32(face + 0x80).wrapping_add((k as i32 * 100) as u32);
                    m.set_u32(crv_c, c);
                    si_nurbs::image::crv_inverse(m, o, c);
                    let srf = revolve_nurbs_for(m, o, c, steps as i32, patch_type as i32, angle, axis as i32, model, 1, m.u32(rev + 0x1c));
                    m.set_u32(srfs.wrapping_add((k as i32 * 4) as u32), srf);
                    if srf == 0 {
                        if update == 0 {
                            dispose_node(m, o, node_c);
                        }
                        return 1;
                    }
                    k = k.wrapping_add(1);
                }
                let _ = create_nurbs_srf_hierarchy_from_nurbs_face(m, o, srfs, node_c, Ptr32::new(face), update, caps, 0);
            } else {
                let count = m.i16(face + 0x78);
                let list = m.si_malloc(((count as i32).wrapping_mul(4)).wrapping_add(4) as u32);
                face_revolution_to_patchs(m, o, face, patch_type as i32, steps as i16, angle, 1, axis as i32, model, list, m.u32(rev + 0x1c) as i32);
                create_patch_hierarchy_from_face(m, o, list, node_c, Ptr32::new(face), update, caps, 0);
                let node = m.u32(node_c);
                mark_patch_kind(m, node);
            }
        }
        mesh_image::face::face_dispose(m, face_c);
    } else if is_crv {
        let crv = m.u32(crv_c);
        if !patches {
            si_nurbs::image::crv_inverse(m, o, crv);
            let per = m.i32(crv + 0x34);
            if si_nurbs::image::sample_knot_vector(m, crv, per, arr_c, cnt_c).is_err() {
                return 1;
            }
            let n = m.i32(cnt_c);
            let face = si_hrc::image::spline::face_allocate(m).addr();
            let keys = si_hrc::image::spline::spline_key_allocate(m, n);
            m.set_u32(face + 0x18, keys);
            let closed = m.u16(crv + 0x20);
            m.set_u16(face + 0xc, closed);
            m.set_u16(face + 0x14, n as u16);
            let mut k: i16 = 0;
            while (k as i32) < n {
                let t = m.f64(m.u32(arr_c).wrapping_add((k as i32 * 8) as u32));
                let p = match si_nurbs::image::crv_value(m, crv, t, 0) {
                    Ok(p) => p,
                    Err(_) => return 1,
                };
                let at = m.u32(face + 0x18).wrapping_add((k as i32 * 0x30) as u32);
                m.set_f32(at + 0xc, p[0] as f32);
                m.set_f32(at + 0x10, p[1] as f32);
                m.set_f32(at + 0x14, p[2] as f32);
                k = k.wrapping_add(1);
            }
            let mesh = mesh_revolve_face(m, o, face, step, steps as i32, angle, axis as i32, model, caps);
            if mesh == 0 {
                if update == 0 {
                    dispose_node(m, o, node_c);
                }
                si_hrc::image::nurbs::nurbs_crv_dispose(m, crv_c);
                return 1;
            }
            let node = Ptr32::new(m.u32(node_c));
            if update == 0 {
                setup::add_definition(m, node, 4, mesh);
            } else {
                defs::substitute_idefinition(m, &mut ModelSide(o), node, 0, mesh);
            }
            mesh_image::face::face_dispose(m, face_c);
            m.set_u32(node.addr() + 0x30, 4);
        } else {
            if m.u16(crv + 0x20) == 0 {
                caps = 0;
            }
            let srf = revolve_nurbs_for(m, o, crv, steps as i32, patch_type as i32, angle, axis as i32, model, 1, m.u32(rev + 0x1c));
            if srf == 0 {
                if update == 0 {
                    dispose_node(m, o, node_c);
                }
                si_hrc::image::nurbs::nurbs_crv_dispose(m, crv_c);
                return 1;
            }
            let _ = create_nurbs_hierarchy_from_nurbs_crv(m, o, srf, node_c, update, caps, 0);
        }
        si_hrc::image::nurbs::nurbs_crv_dispose(m, crv_c);
    }
    let node = m.u32(node_c);
    m.set_u32(model_at, node);
    1
}

/// `SI_revolveNurbs (0x08198014)` with the callbacks of `si-model` handed our outside world.
#[allow(clippy::too_many_arguments)]
pub fn revolve_nurbs_for(m: &mut Mem, o: &mut dyn Outside, crv: u32, steps: i32, kind: i32, angle: f32, axis: i32, model: u32, scale: i16, def: u32) -> u32 {
    si_model::nurbs_prim_image::revolve_nurbs(m, o, crv, steps, kind, angle, axis, model, scale, def)
}
