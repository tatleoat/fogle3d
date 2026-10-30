// SPDX-License-Identifier: MIT
//! `extrusion.c` (fourth slice): the model an extrusion effect makes, and the update of the effect.
//! Finding 0034, "geomupd3".
//!
//! `SI_extrusionUpdate` takes the effect's update node, `createModelFromExtrusion` copies the shape
//! to extrude (a face, a spline or a NURBS curve), moves it into the model's frame and hands it to
//! one of the builders: a mesh (kind 4), a patch hierarchy (kind 1) or a NURBS surface hierarchy
//! (kind 10). The status line, the sleep and the relation clean-up are hooks (UI), written
//! `name (0xADDRESS)`.

use super::{ModelS, SplineS, ViewS, graf_list};
use super::{SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_MDL_STATUS_MESSAGES, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use crate::extrusion as x;
use crate::extrusion2 as x2;
use si_math::types::Vector;
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::model_image::{defs, setup, touch};
use si_nurbs::image::Halt;
use si_x87::F80;

/// `SI_modelDisposeNode (0x08461a54)`, `model.c`: frees a node with what it owns (called through a hook).
pub const SI_MODEL_DISPOSE_NODE: Routine = Routine::new("SI_modelDisposeNode", 0x0846_1a54);
/// `sprintf (0x080c4f58)`, the C library's.
pub const SPRINTF: Routine = Routine::new("sprintf", 0x080c_4f58);

/// The node name "extru" (the binary's string at 0x08689fb8, put into the image by `store_names` of
/// [`crate::extrusion`]).
const NAME_EXTRU: u32 = 0x0868_9fb8;

const MSG_UNABLE_CONTOUR: u32 = 0x0868_9f80;
const MSG_INCOMPLETE_NURBS: u32 = 0x0868_9fc0;
const MSG_NOT_ENOUGH_KEEP_CURVE: u32 = 0x0868_a2e0;
const MSG_NOT_ENOUGH_KEEP_SPLINE: u32 = 0x0868_a320;
const MSG_NOT_ENOUGH_PATH_SPLINE: u32 = 0x0868_a380;
const MSG_NOT_ENOUGH_PATH_CURVE: u32 = 0x0868_a3c0;
const MSG_NOT_PERFORMED: u32 = 0x0868_a400;
/// `SI_nbPolMax` (0x086d3920), the polygon limit the not-performed message names.
const SI_NB_POL_MAX: u32 = 0x086d_3920;

fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// A matrix of 16 floats on the stack (the caller releases the stack).
fn matrix_local(m: &mut Mem, v: Vector) -> u32 {
    let at = m.local(64);
    let t = si_math::algebra::matrix_translation(&v);
    for (r, row) in t.iter().enumerate() {
        for (c, x) in row.iter().enumerate() {
            m.set_f32(at + 16 * r as u32 + 4 * c as u32, *x);
        }
    }
    at
}

fn dispose_spline_at(m: &mut Mem, spline: u32) {
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, spline);
    si_hrc::image::spline::spline_dispose(m, slot);
    m.set_sp(sp);
}

fn dispose_crv_at(m: &mut Mem, crv: u32) {
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, crv);
    si_hrc::image::nurbs::nurbs_crv_dispose(m, slot);
    m.set_sp(sp);
}

/// `SI_statusDisplayMessage(text, level)` then `sleep(1)`.
fn say(m: &mut Mem, o: &mut dyn Outside, text: u32, level: u32) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, level]);
    o.call(m, SLEEP, &[1]);
}

/// The kind of the nodes under a new model (and the model itself when it has none or has a kind of its
/// own): the tail the three non-mesh branches of [`create_model_from_extrusion`] share.
fn mark_kinds(m: &mut Mem, node: u32, kind: i32) {
    let son = m.u32(node + 0x2c4);
    if son == 0 || m.i32(node + 0x30) != 0 {
        m.set_i32(node + 0x30, kind);
    } else {
        let mut s = son;
        while s != 0 {
            if m.i32(s + 0x30) != 2 {
                m.set_i32(s + 0x30, kind);
            }
            s = m.u32(s + 0x2c0);
        }
    }
}

fn put_mesh(m: &mut Mem, o: &mut dyn Outside, node: u32, mesh: u32, update: bool) {
    if update {
        defs::substitute_idefinition(m, &mut ModelS(o), Ptr32::new(node), 0, mesh);
    } else {
        setup::add_definition(m, Ptr32::new(node), 4, mesh);
    }
}

fn dispose_node_at(m: &mut Mem, o: &mut dyn Outside, slot: u32) {
    o.call(m, SI_MODEL_DISPOSE_NODE, &[slot]);
}

/// The nodes of a NURBS surface extruded from a curve [soft 0x081584c4 createNurbsFromExtrusionFromNurbs]
/// (`pp` the address of the model pointer, `crv` the curve, `path` the path model, `data` the
/// extrusion record, `update` the update flag): along a path (axis 3) the surface comes from
/// `nurbsExtrudeNurbsOnNurbs (0x0815e3f0)` or, for a path of order 2 or a spline without curvature,
/// `nurbsExtrudeNurbsOnLinearNurbs (0x0815c194)`; else from `SI_nurbsExtrudeToNurbs (0x0815b6d8)` with the
/// depth negated. Answers 0 when no surface came (the node is disposed when new, and, along a path, the
/// curve too), else 1.
pub fn create_nurbs_from_extrusion_from_nurbs(m: &mut Mem, o: &mut dyn Outside, pp: u32, crv: u32, path: u32, data: u32, update: i16) -> Result<u16, Halt> {
    let mut caps = m.i16(data);
    let axis = m.i32(data + 0x10);
    let srf;
    if axis == 3 {
        let kind = m.i32(path + 0x30);
        let def = m.u32(path + 0x48);
        let closed = if kind == 6 { m.i16(def + 8) } else { m.i16(def + 0x20) };
        if m.i16(crv + 0x20) == 0 || closed != 0 {
            caps = 0;
        }
        srf = if (kind == 6 && m.i32(def + 0xc) == 0) || (kind == 9 && m.i32(def) == 2) {
            super::lin_nurbs::nurbs_extrude_nurbs_on_linear_nurbs(m, o, crv, path)?
        } else {
            super::extrude_nurbs::nurbs_extrude_nurbs_on_nurbs(m, o, crv, path)?
        };
        if srf == 0 {
            if update == 0 {
                dispose_node_at(m, o, pp);
            }
            // the original passes the address of its own argument: the curve is freed, the caller's pointer stays
            dispose_crv_at(m, crv);
            return Ok(0);
        }
    } else {
        if m.i16(crv + 0x20) == 0 {
            caps = 0;
        }
        let depth = -through_x87(m.f32(data + 4));
        srf = super::extrusion3::nurbs_extrude_to_nurbs(m, crv, m.i32(data + 0x18), depth, m.i32(data + 8), axis, m.u32(data + 0x34));
        if srf == 0 {
            return Ok(0);
        }
    }
    x::create_nurbs_hierarchy_from_nurbs_crv(m, o, srf, pp, update, caps, 1)?;
    Ok(1)
}

/// The nodes of a patch extruded from a spline [soft 0x081585e8 createPatchFromExtrusionFromSpline]:
/// along a path `patchExtrudeSplineOnSpline (0x0815bef0)`, else `SI_splineExtrudeToPatch (0x0815bafc)`;
/// answers 0 when the latter makes none.
pub fn create_patch_from_extrusion_from_spline(m: &mut Mem, o: &mut dyn Outside, pp: u32, spline: u32, path: u32, data: u32, update: i16) -> u16 {
    let mut caps = m.i16(data);
    let axis = m.i32(data + 0x10);
    let patch;
    if axis == 3 {
        if m.i16(spline + 8) == 0 || m.i16(path + 8) != 0 {
            caps = 0;
        }
        patch = super::extrude_patch::patch_extrude_spline_on_spline(m, o, spline, path);
    } else {
        if m.i16(spline + 8) == 0 {
            caps = 0;
        }
        patch = super::extrusion3::spline_extrude_to_patch(m, o, spline, m.i32(data + 0x18), f32::from_bits(m.u32(data + 4)), m.i32(data + 8), axis, m.u32(data + 0x34));
        if patch == 0 {
            return 0;
        }
    }
    si_model::model_image::tree::create_patch_hierarchy_from_spline(m, &mut ModelS(o), Ptr32::new(patch), pp, update, caps, 1);
    1
}

/// The nodes of a patch hierarchy extruded from a face [soft 0x0815868c createPatchFromExtrusionFromFace]:
/// along a path `patchExtrudeFaceOnLinearSpline (0x0815f500)` or `patchExtrudeFaceOnSpline (0x08161540)`
/// (the answer is a list of patches, the first null meaning failure), else one patch for the outline
/// and for each hole by `faceExtrudeToPatchs (0x0815be20)`.
pub fn create_patch_from_extrusion_from_face(m: &mut Mem, o: &mut dyn Outside, pp: u32, face: u32, path: u32, data: u32, update: i16) -> u16 {
    let depth = m.u32(data + 4);
    let axis = m.i32(data + 0x10);
    let steps = m.i32(data + 8);
    let mut caps = m.i16(data);
    let list;
    if axis == 3 {
        if m.i16(path + 8) != 0 {
            caps = 0;
        }
        list = if m.i32(path + 0xc) == 0 {
            super::lin_patch::patch_extrude_face_on_linear_spline(m, o, face, path)
        } else {
            super::extrude_patch::patch_extrude_face_on_spline(m, o, face, path)
        };
        if m.u32(list) == 0 {
            return 0;
        }
    } else {
        list = m.si_malloc((m.i16(face + 0x78) as i32 as u32).wrapping_mul(4).wrapping_add(4));
        super::extrude_patch::face_extrude_to_patchs(m, o, face, m.i32(data + 0x18), f32::from_bits(depth), steps, list, axis, m.u32(data + 0x34));
    }
    x::create_patch_hierarchy_from_face(m, o, list, pp, Ptr32::new(face), update, caps, 1);
    1
}

/// The nodes of a NURBS surface hierarchy extruded from a face of NURBS curves
/// [soft 0x0815876c createNurbsSrfFromExtrusionFromFace]: one surface for each curve, along the path
/// model (`nurbsExtrudeNurbsOnLinearNurbs (0x0815c194)` or `nurbsExtrudeNurbsOnNurbs (0x0815e3f0)`) or by
/// `SI_nurbsExtrudeToNurbs (0x0815b6d8)`. Answers 0 when one of them fails.
pub fn create_nurbs_srf_from_extrusion_from_face(m: &mut Mem, o: &mut dyn Outside, pp: u32, face: u32, path: u32, data: u32, update: i16) -> Result<u16, Halt> {
    let depth = f32::from_bits(m.u32(data + 4));
    let axis = m.i32(data + 0x10);
    let steps = m.i32(data + 8);
    let mut caps = m.i16(data);
    let kind = m.i32(data + 0x18);
    let srfs = m.si_malloc((m.i16(face + 0x78) as i32 as u32) << 2);
    if axis == 3 {
        let pk = m.i32(path + 0x30);
        let def = m.u32(path + 0x48);
        let closed = if pk == 6 { m.i16(def + 8) } else { m.i16(def + 0x20) };
        if closed != 0 {
            caps = 0;
        }
        let mut i: i32 = 0;
        while i < m.i16(face + 0x78) as i32 {
            let crv = m.u32(face + 0x80).wrapping_add((i as u32).wrapping_mul(100));
            let linear = (m.i32(path + 0x30) == 6 && m.i32(m.u32(path + 0x48) + 0xc) == 0) || (m.i32(path + 0x30) == 9 && m.i32(m.u32(path + 0x48)) == 2);
            let s = if linear {
                super::lin_nurbs::nurbs_extrude_nurbs_on_linear_nurbs(m, o, crv, path)?
            } else {
                super::extrude_nurbs::nurbs_extrude_nurbs_on_nurbs(m, o, crv, path)?
            };
            m.set_u32(srfs.wrapping_add(4 * i as u32), s);
            if s == 0 {
                return Ok(0);
            }
            i += 1;
        }
    } else {
        let mut i: i32 = 0;
        while i < m.i16(face + 0x78) as i32 {
            let crv = m.u32(face + 0x80).wrapping_add((i as u32).wrapping_mul(100));
            let s = super::extrusion3::nurbs_extrude_to_nurbs(m, crv, kind, depth, steps, axis, m.u32(data + 0x34));
            m.set_u32(srfs.wrapping_add(4 * i as u32), s);
            if s == 0 {
                return Ok(0);
            }
            i += 1;
        }
    }
    x::create_nurbs_srf_hierarchy_from_nurbs_face(m, o, srfs, pp, Ptr32::new(face), update, caps, 1)?;
    Ok(1)
}

/// The model an extrusion makes [soft 0x0815890c createModelFromExtrusion]. `src` is the model whose
/// shape (kind 2 face, 6 spline, 9 NURBS curve) is extruded, `path` the model of the path (axis 3),
/// `pp` the address of the pointer to the extrusion's model (null: a new one is made), `path_spline` a
/// copy of the path's spline, `data` the extrusion record. The shape is copied and moved into the world
/// by the model's matrix; the output (a mesh, a patch hierarchy or a NURBS surface hierarchy) follows
/// the record's patch flag. Answers 1 when the model was made or there was nothing to make, 0 on a
/// real failure; a source of another kind answers 1 and leaves `*pp`.
pub fn create_model_from_extrusion(m: &mut Mem, o: &mut dyn Outside, src: u32, path: u32, pp: u32, path_spline: u32, data: u32) -> Result<i16, Halt> {
    let sp0 = m.sp();
    let r = create_model_inner(m, o, src, path, pp, path_spline, data);
    m.set_sp(sp0);
    r
}

fn create_model_inner(m: &mut Mem, o: &mut dyn Outside, src: u32, path: u32, pp: u32, path_spline: u32, data: u32) -> Result<i16, Halt> {
    let axis = m.i32(data + 0x10);
    let subdiv = m.u32(data + 0xc) as i16;
    let steps = m.i32(data + 8);
    let close = m.i16(data);
    // the node pointer, the face pointer, the spline pointer and the curve pointer live on the stack
    let node_slot = m.local(4);
    let face_slot = m.local(4);
    let spline_slot = m.local(4);
    let crv_slot = m.local(4);
    let node0 = m.u32(pp);
    m.set_u32(node_slot, node0);
    m.set_u32(crv_slot, 0);
    let mut v = Vector::new(0.0, 0.0, 0.0);
    let depth = f32::from_bits(m.u32(data + 4));
    match axis {
        0 => v.x = depth,
        1 => v.y = depth,
        2 => v.z = depth,
        _ => {}
    }
    let mat = matrix_local(m, v);
    let (mut is_spline, mut is_face, mut is_nurbs) = (false, false, false);
    if path != 0 && m.i32(path + 0x30) == 9 && (m.i32(src + 0x30) == 6 || (m.i32(src + 0x30) == 2 && m.i16(m.u32(src + 0x48) + 0x84) != 0)) {
        m.set_u32(pp, 0);
        say(m, o, MSG_UNABLE_CONTOUR, 3);
        return Ok(1);
    }
    let kind = m.i32(src + 0x30);
    if kind == 2 {
        is_face = true;
        let face = super::revolve_four::face_copy(m, m.u32(src + 0x48));
        m.set_u32(face_slot, face);
        if m.i16(face + 0x84) == 0 {
            let mut i: i32 = 0;
            while i < m.i16(face + 0x78) as i32 {
                let crv = m.u32(face + 0x80).wrapping_add((i as u32).wrapping_mul(100));
                m.set_u32(crv_slot, crv);
                si_nurbs::image::crv_apply_matrix(m, crv, src + 0x1c4, 0);
                i += 1;
            }
        } else if (m.u8(face + 0x79) & 0x80) == 0 {
            let mut i: i32 = -1;
            loop {
                let s = if i == -1 { face + 4 } else { m.u32(face + 0x7c).wrapping_add((i as u32).wrapping_mul(0x74)) };
                m.set_u32(spline_slot, s);
                si_spline::convert_image::spline_multiply_definition(m, &mut SplineS(o), Ptr32::new(s), src + 0x1c4);
                i += 1;
                if i >= m.i16(face + 0x78) as i32 {
                    break;
                }
            }
        }
    } else if kind == 6 {
        is_spline = true;
        let s = si_hrc::image::spline::spline_copy(m, m.u32(src + 0x48)).addr();
        m.set_u32(spline_slot, s);
        si_spline::convert_image::spline_multiply_definition(m, &mut SplineS(o), Ptr32::new(s), src + 0x1c4);
    } else {
        if kind != 9 {
            return Ok(1);
        }
        is_nurbs = true;
        let c = si_nurbs::image::crv_copy(m, m.u32(src + 0x48));
        m.set_u32(crv_slot, c);
        si_nurbs::image::crv_apply_matrix(m, c, src + 0x1c4, 0);
    }
    let mut answer: i16 = 1;
    let existing = m.u32(node_slot) != 0;
    if !existing {
        let n = si_hrc::image::node::model_allocate(m, NAME_EXTRU).addr();
        m.set_u32(node_slot, n);
    }
    let update = existing as i16;
    let face = m.u32(face_slot);
    let face_is_poly = is_face && m.i16(face + 0x84) != 0;
    // `node` below is re-read from its slot where the original does
    if m.i16(data + 0x14) == 0 {
        if axis == 3 {
            if !is_spline {
                if is_face {
                    let mesh;
                    if m.i16(face + 0x84) == 0 {
                        mesh = x2::mesh_extrude_nurbs_face_on_curve(m, o, face, subdiv, path, steps, close)?;
                        if mesh == 0 && m.i32(m.u32(face + 0x80) + 4) != 0 {
                            answer = 0;
                        }
                    } else {
                        mesh = if m.i32(path_spline + 0xc) == 0 {
                            super::lin_patch::mesh_extrude_face_on_linear_spline(m, o, face, subdiv, path_spline, steps, close)
                        } else {
                            x2::mesh_extrude_face_on_spline(m, o, face, subdiv, path_spline, steps, close)
                        };
                        if mesh == 0 && m.i16(face + 0x14) != 0 && m.i16(path_spline + 0x10) != 0 {
                            answer = 0;
                        }
                    }
                    if mesh != 0 {
                        let node = m.u32(node_slot);
                        put_mesh(m, o, node, mesh, update != 0);
                        x::dispose_face_at(m, face);
                        m.set_i32(m.u32(node_slot) + 0x30, 4);
                        m.set_u32(pp, m.u32(node_slot));
                        return Ok(answer);
                    }
                    if update == 0 {
                        dispose_node_at(m, o, node_slot);
                    }
                    x::dispose_face_at(m, face);
                    return Ok(answer);
                } else if is_nurbs {
                    let crv = m.u32(crv_slot);
                    let mesh = x2::mesh_extrude_nurbs_on_nurbs(m, o, crv, subdiv, path, steps, close)?;
                    if mesh == 0 {
                        if m.i32(crv + 4) != 0 {
                            answer = 0;
                        }
                        if update == 0 {
                            dispose_node_at(m, o, node_slot);
                        }
                        dispose_crv_at(m, crv);
                        return Ok(answer);
                    }
                    let node = m.u32(node_slot);
                    put_mesh(m, o, node, mesh, update != 0);
                    dispose_crv_at(m, crv);
                    m.set_i32(m.u32(node_slot) + 0x30, 4);
                } else {
                    m.set_u32(pp, m.u32(node_slot));
                    return Ok(answer);
                }
            } else {
                let sp = m.u32(spline_slot);
                let mesh = x2::mesh_extrude_spline_on_spline(m, o, sp, subdiv, path_spline, steps, close);
                if mesh == 0 {
                    if sp != 0 && path_spline != 0 && m.i16(sp + 0x10) != 0 && m.i16(path_spline + 0x10) != 0 {
                        answer = 0;
                    }
                    if update == 0 {
                        dispose_node_at(m, o, node_slot);
                    }
                    dispose_spline_at(m, sp);
                    return Ok(answer);
                }
                let node = m.u32(node_slot);
                put_mesh(m, o, node, mesh, update != 0);
                dispose_spline_at(m, sp);
                m.set_i32(m.u32(node_slot) + 0x30, 4);
            }
        } else if is_spline || face_is_poly {
            let node = m.u32(node_slot);
            let ok = x::create_mesh_from_extrusion_simple(m, o, Ptr32::new(node), m.u32(spline_slot), face, data, is_spline as i16, update);
            answer = ok as i16;
            if m.u32(node + 0x38) == 0 {
                if update == 0 {
                    dispose_node_at(m, o, node_slot);
                }
                if !is_spline {
                    x::dispose_face_at(m, face);
                } else {
                    dispose_spline_at(m, m.u32(spline_slot));
                }
                return Ok(answer);
            }
            if !is_spline {
                x::dispose_face_at(m, face);
            } else {
                dispose_spline_at(m, m.u32(spline_slot));
            }
        } else if is_nurbs {
            let node = m.u32(node_slot);
            let crv = m.u32(crv_slot);
            answer = x::create_mesh_from_nurbs_extrusion(m, o, Ptr32::new(node), crv, data, update as i16) as i16;
            if m.u32(node + 0x38) == 0 {
                if update == 0 {
                    dispose_node_at(m, o, node_slot);
                }
                dispose_crv_at(m, crv);
                return Ok(answer);
            }
            dispose_crv_at(m, crv);
        } else if is_face && m.i16(face + 0x84) == 0 {
            let node = m.u32(node_slot);
            answer = x::create_mesh_from_nurbs_face_extrusion(m, o, Ptr32::new(node), face, data, update) as i16;
            if m.u32(node + 0x38) == 0 {
                if update == 0 {
                    dispose_node_at(m, o, node_slot);
                }
                x::dispose_face_at(m, face);
                return Ok(answer);
            }
            x::dispose_face_at(m, face);
        }
    } else if !is_spline {
        if is_face {
            if m.i16(face + 0x84) == 0 {
                answer = create_nurbs_srf_from_extrusion_from_face(m, o, node_slot, face, path, data, update)? as i16;
                x::dispose_face_at(m, face);
                if answer == 0 {
                    return Ok(1);
                }
                mark_kinds(m, m.u32(node_slot), 10);
            } else {
                answer = create_patch_from_extrusion_from_face(m, o, node_slot, face, path_spline, data, update) as i16;
                x::dispose_face_at(m, face);
                if answer == 0 {
                    return Ok(1);
                }
                mark_kinds(m, m.u32(node_slot), 1);
            }
        } else if is_nurbs {
            let crv = m.u32(crv_slot);
            if crv == 0 || m.i32(crv + 4) < m.i32(crv) {
                say(m, o, MSG_INCOMPLETE_NURBS, 3);
                return Ok(answer);
            }
            if axis == 3 {
                si_nurbs::image::crv_inverse(m, &mut *o, crv);
            } else {
                si_nurbs::image::crv_apply_matrix(m, crv, mat, 0);
            }
            answer = create_nurbs_from_extrusion_from_nurbs(m, o, node_slot, crv, path, data, update)? as i16;
            dispose_crv_at(m, m.u32(crv_slot));
            if answer == 0 {
                return Ok(1);
            }
            mark_kinds(m, m.u32(node_slot), 10);
        }
    } else {
        answer = create_patch_from_extrusion_from_spline(m, o, node_slot, m.u32(spline_slot), path_spline, data, update) as i16;
        dispose_spline_at(m, m.u32(spline_slot));
        if answer == 0 {
            return Ok(1);
        }
        mark_kinds(m, m.u32(node_slot), 1);
    }
    m.set_u32(pp, m.u32(node_slot));
    Ok(answer)
}

/// The update of an extrusion effect [soft 0x0815b3a4 SI_extrusionUpdate]. `upd` is the update node: the
/// model at +8, at +0x1c the list of inputs (+0xc a count, +8 the shape's model, +0x30 the path's model,
/// +0x34 its count), the extrusion record at +0x24. A shape or a path with too few points is reported
/// through the status line and the relation 250 dropped (`SI_edRelaxRelFromDelModel (0x080ff5f0)`); else
/// the model is rebuilt by [`create_model_from_extrusion`], its geometry touched and the graph nodes of
/// it and of its sons updated.
pub fn extrusion_update(m: &mut Mem, o: &mut dyn Outside, upd: u32, _time: f32) -> Result<(), Halt> {
    let sp = m.sp();
    let r = extrusion_update_inner(m, o, upd);
    m.set_sp(sp);
    r
}

fn extrusion_update_inner(m: &mut Mem, o: &mut dyn Outside, upd: u32) -> Result<(), Halt> {
    let buf = m.local(256);
    let model_slot = m.local(4);
    let spline_slot = m.local(4);
    let mut path_model = 0u32;
    m.set_u32(spline_slot, 0);
    let inputs = m.u32(upd + 0x1c);
    if m.i16(inputs + 0xc) == 0 {
        return Ok(());
    }
    let model = m.u32(upd + 8);
    m.set_u32(model_slot, model);
    if model == 0 {
        return Ok(());
    }
    let src = m.u32(inputs + 8);
    if src == 0 {
        return Ok(());
    }
    let data = m.u32(upd + 0x24);
    if data == 0 {
        return Ok(());
    }
    let relax_src = |m: &mut Mem, o: &mut dyn Outside| {
        o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[src, 0xfa]);
    };
    if m.i32(src + 0x30) == 9 {
        let crv = m.u32(src + 0x48);
        if m.i32(crv + 4) < m.i32(crv) {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_NOT_ENOUGH_KEEP_CURVE, 2]);
            o.call(m, SLEEP, &[1]);
            o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
            relax_src(m, o);
            return Ok(());
        }
    } else if m.i32(src + 0x30) == 6 {
        let s = m.u32(src + 0x48);
        if m.u16(s + 0x10) < 2 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_NOT_ENOUGH_KEEP_SPLINE, 2]);
            o.call(m, SLEEP, &[1]);
            o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
            relax_src(m, o);
            return Ok(());
        }
    }
    let mut keep = 0u32;
    if m.i32(data + 0x10) == 3 {
        if m.i16(inputs + 0x34) == 0 {
            return Ok(());
        }
        path_model = m.u32(inputs + 0x30);
        if path_model == 0 {
            return Ok(());
        }
        let def = m.u32(path_model + 0x48);
        if def == 0 {
            return Ok(());
        }
        if m.i32(path_model + 0x30) == 6 {
            keep = si_hrc::image::spline::spline_copy(m, def).addr();
            m.set_u32(spline_slot, keep);
            si_spline::convert_image::spline_multiply_definition(m, &mut SplineS(o), Ptr32::new(keep), path_model + 0x1c4);
            let (nb, kind) = (m.u16(keep + 0x10), m.u32(keep + 0xc));
            if (nb < 2 && kind < 2) || (nb < 4 && kind.wrapping_sub(2) < 2) {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_NOT_ENOUGH_PATH_SPLINE, 2]);
                o.call(m, SLEEP, &[1]);
                o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
                o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[path_model, 0xfa]);
                relax_src(m, o);
                dispose_spline_at(m, keep);
                return Ok(());
            }
        } else if m.i32(path_model + 0x30) == 9 && m.i32(def + 4) < m.i32(def) {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_NOT_ENOUGH_PATH_CURVE, 2]);
            o.call(m, SLEEP, &[1]);
            o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
            o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[path_model, 0xfa]);
            relax_src(m, o);
            return Ok(());
        }
    }
    if m.u32(src + 0x48) == 0 {
        return Ok(());
    }
    let done = create_model_from_extrusion(m, o, src, path_model, model_slot, keep, data)?;
    if keep != 0 {
        dispose_spline_at(m, keep);
    }
    if done == 0 {
        let nb_pol_max = m.u32(SI_NB_POL_MAX);
        o.call(m, SPRINTF, &[buf, MSG_NOT_PERFORMED, nb_pol_max]);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
        o.call(m, SLEEP, &[1]);
        return Ok(());
    }
    let node = m.u32(model_slot);
    touch_and_update(m, o, node);
    let mut s = m.u32(node + 0x2c4);
    while s != 0 {
        touch_and_update(m, o, s);
        s = m.u32(s + 0x2c0);
    }
    Ok(())
}

fn touch_and_update(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    touch::touch_geometry(m, &mut ModelS(o), Ptr32::new(node));
    let gl = graf_list(m);
    let gn = si_view::graflist::find_gl_data(m, gl, Ptr32::new(node), 2);
    si_view::func::update_gn_func(m, &mut ViewS(o), gn);
}
