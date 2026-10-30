// SPDX-License-Identifier: MIT
//! `curve.c`: the update of an extraction relation (a curve taken out of a patch, a face, a spline or a NURBS
//! surface) and the model it makes. Finding 0034, section "geomupd1".
//!
//! The extraction record (`SI_ExtractData`, 0x18 bytes, from `createModelFromExtraction`): +0 a short (the direction
//! of a patch's or surface's isoparametric curve), +4 a word (a NURBS surface: 0 an isoparametric curve, 1 a trim
//! curve cut at the border, anything else a projection curve), +8 a double (the parameter), +0x10 the index of the
//! curve of a face, spline or trim, +0x14 the index of the trim curve.

use super::{touch_geometry, update_display_node, SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_MODEL_MULTIPLY_DEFINITION, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use crate::extrusion::ModelSide;
use si_mem::{Mem, Outside, Ptr32};
use si_model::model_image::defs;
use si_types::model::Model;

/// [soft 0x0814e16c createModelFromExtraction] `(source, target, data)`: makes the curve `data` names out of the
/// definition of the model `source` (a patch, kind 1; a face, kind 2, or a spline of a face; or a NURBS surface, kind
/// 10), puts it into `target` as its first definition (`SI_modelSubstituteIDefinition`) and carries it by the
/// source's matrix (+0x1c4, `SI_modelMultiplyDefinition`). Answers 1, or 0 when nothing could be extracted (a curve
/// index out of range, a source of another kind, the evaluator refusing).
///
/// Kind 2 (the record at +0x48): a face with its flag (+0x84) clear copies the NURBS curve `index` of its array (+0x80,
/// 100 bytes each, at most `count - 1`, the count a short at +0x78); with the flag set it copies the spline `index`
/// (the face's own at 0, else the `index`th of the array at +0x7c, 0x74 bytes each, from the second). Kind 1 extracts a
/// spline of the patch (`SI_patchExtractSpline`, the parameter narrowed to a float). Kind 10 mode 0 extracts the
/// isoparametric curve (`SI_nurbsSrfExtractCurve`); modes 1 and 2 take the trim curves cut at the border or the
/// projection curves, extract the one at +0x14 when it is in 0..count, and dispose of all of the table.
/// A spline result is preferred to a curve result when both exist (only kind 2 sets one of each, never both).
pub fn create_model_from_extraction(m: &mut Mem, o: &mut dyn Outside, source: u32, target: u32, data: u32) -> Result<u32, si_nurbs::image::Halt> {
    let mut spline: u32 = 0;
    let mut curve: u32 = 0;
    let index = m.i32(data + 0x10);
    let trim_index = m.i32(data + 0x14);
    let kind = m.i32(source + 0x30);
    if kind == 2 {
        let face = m.u32(source + 0x48);
        if m.i16(face + 0x84) == 0 {
            if (m.i16(face + 0x78) as i32).wrapping_sub(1) < index {
                return Ok(0);
            }
            let at = (index as u32).wrapping_mul(100).wrapping_add(m.u32(face + 0x80));
            curve = si_nurbs::image::crv_copy(m, at);
        } else {
            let sp;
            if index == 0 {
                sp = face + 4;
            } else {
                if (m.i16(face + 0x78) as i32) < index {
                    return Ok(0);
                }
                sp = m.u32(face + 0x7c).wrapping_sub(0x74).wrapping_add((index as u32).wrapping_mul(0x74));
            }
            spline = si_hrc::image::spline::spline_copy(m, sp).addr();
        }
    } else if kind < 3 {
        if kind == 1 {
            // the double narrowed to a float (the compiler stored it as one)
            let t = m.f64(data + 8) as f32;
            spline = si_spline::patch_image::patch_extract_spline(m, Ptr32::new(m.u32(source + 0x48)), m.i16(data), t).addr();
        }
    } else if kind == 10 {
        let srf = m.u32(source + 0x48);
        if m.i32(data + 4) == 0 {
            let t = m.f64(data + 8);
            curve = si_nurbs::image::srf_extract_curve(m, srf, t, m.i16(data) as i32, 0)?;
        } else {
            let count = m.local(4);
            let table = if m.i32(data + 4) == 1 {
                si_nurbs::trimutils3::trim_remv_pts_on_border(m, o, srf, count)
            } else {
                si_nurbs::trimutils2::srf_get_proj_crv(m, o, srf, count)
            };
            let n = m.i32(count);
            m.release(4);
            let in_range = trim_index < n && trim_index >= 0;
            if in_range {
                curve = si_nurbs::trimutils3::srf_extract_trim_curve(m, o, srf, trim_index as i16, table);
            }
            let mut i = 0;
            while i < n {
                si_hrc::image::nurbs::nurbs_crv_dispose(m, table.wrapping_add(4 * i as u32));
                i += 1;
            }
            if table != 0 {
                m.free(table);
            }
            if !in_range {
                return Ok(0);
            }
        }
    }
    if spline == 0 {
        spline = curve;
        if curve == 0 {
            return Ok(0);
        }
    }
    defs::substitute_idefinition(m, &mut ModelSide(o), Ptr32::<Model>::new(target), 0, spline);
    o.call(m, SI_MODEL_MULTIPLY_DEFINITION, &[target, source + 0x1c4]);
    Ok(1)
}

/// [soft 0x0814e734 SI_extractionUpdate] `(node, time)`: the update function of an extraction relation. `node` is the
/// relation's update node: +8 the model that is made (the target), +0x1c the table of the relation's inputs (the
/// first, at +8 of its first entry, is the source model), +0x24 the extraction record. Nothing happens when any of the
/// three is missing. A source of kind 1, 2 or 10 is extracted ([`create_model_from_extraction`]): on success the
/// target is touched (`SI_modelTouchGeometry`) and its display node brought up to date (`SI_findGlData`,
/// `SI_updateGnFunc`); on failure the message "Your relation has been deleted" is shown (level 2) and the relation
/// is relaxed (`SI_edRelaxRelFromDelModel(source, 0x100)`). A source of another kind shows another message of the same
/// kind, relaxes the relation the same way, and sleeps a second. The message and relaxation routines are the
/// editor's and answer through `o`; the original's evaluation error of the evaluator (a fault) is a `Halt` here.
pub fn extraction_update(m: &mut Mem, o: &mut dyn Outside, node: u32, _time: f32) -> Result<(), si_nurbs::image::Halt> {
    let target = m.u32(node + 8);
    if target == 0 {
        return Ok(());
    }
    let source = m.u32(m.u32(node + 0x1c) + 8);
    if source == 0 || m.u32(node + 0x24) == 0 {
        return Ok(());
    }
    let kind = m.i32(source + 0x30);
    if kind.wrapping_sub(1) as u32 >= 2 && kind != 10 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x0868_87c0, 2]);
        o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[source, 0x100]);
        o.call(m, SLEEP, &[1]);
        return Ok(());
    }
    let done = create_model_from_extraction(m, o, source, target, m.u32(node + 0x24))? as u16;
    if done == 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x0868_87e0, 2]);
        o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[source, 0x100]);
    } else {
        touch_geometry(m, o, target);
        update_display_node(m, o, target);
    }
    Ok(())
}
