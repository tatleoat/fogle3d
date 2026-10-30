// SPDX-License-Identifier: MIT
//! `trimDial.c`: the update of the trim effect (a NURBS surface cut by projected curves).

use super::{elem_dict, graf_list, ModelS, ViewS, CID_MODELS};
use super::{SI_ED_GET_ELEM_FROM_MODEL, SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_SHOW_ACTIVITY, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use si_dict::elemrel;
use si_mem::{Mem, Outside, Ptr32};
use si_model::model_image::{defs, touch};
use si_nurbs::trim::{self, CreateArgs};
use si_types::model::Model;

/// The binary's text "Your relation has been deleted" for the surface (0x086dab40) and for a curve (0x086dab60).
pub const MSG_SURFACE: u32 = 0x086d_ab40;
pub const MSG_CURVE: u32 = 0x086d_ab60;
/// `SI_bezPatchDestroy`, the destructor `SI_genTreeDestroy` is given for the surface's patch tree.
const BEZ_PATCH_DESTROY: u32 = 0x0850_3504;

fn six(m: &Mem, at: u32) -> [u32; 6] {
    let mut v = [0u32; 6];
    for (i, w) in v.iter_mut().enumerate() {
        *w = m.u32(at + 4 * i as u32);
    }
    v
}

/// The update of a trim effect, `upd` its update node (the model at +8; at +0x1c the list of
/// inputs, 0x14 bytes each, the surface model first; +0x18 the count of inputs, a signed 16-bit)
/// SI_trimUpdate (0x084fcb08; trimming branch checked, 1 of 2000 cases differs in a si-nurbs callee's uninitialised block; finding 0034 trim1). Every curve input after the surface is projected on a copy of
/// the surface and trimmed by the relation's settings; the copy replaces the model's first
/// definition. A relation gone from the surface or a curve is reported through the status line
/// (`SI_statusDisplayMessage (0x083cca94)`, `SI_edRelaxRelFromDelModel (0x080ff5f0)` and `sleep`).
pub fn trim_update(m: &mut Mem, o: &mut dyn Outside, upd: u32) {
    let sp = m.sp();
    trim_update_inner(m, o, upd);
    m.set_sp(sp);
}

fn trim_update_inner(m: &mut Mem, o: &mut dyn Outside, upd: u32) {
    let copy_pp = m.local(4);
    let trim_pp = m.local(4);
    let l8 = m.local(4);
    let first = m.local(4);
    let last = first + 2;
    m.set_u32(trim_pp, 0);
    let inputs = m.u32(upd + 0x1c);
    if m.i16(inputs + 0xc) == 0 {
        return;
    }
    let model = m.u32(upd + 8);
    if model == 0 {
        return;
    }
    let srf_model = m.u32(inputs + 8);
    if srf_model == 0 {
        return;
    }
    let srf = m.u32(srf_model + 0x48);
    if m.i32(srf_model + 0x30) != 10 || srf == 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_SURFACE, 2]);
        o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[srf_model, 0x101]);
        o.call(m, SLEEP, &[1]);
        return;
    }
    let copy = si_nurbs::image::srf_copy(m, o, srf);
    m.set_u32(copy_pp, copy);
    let count = m.i16(upd + 0x18) as i32;
    let mut i: i32 = 2;
    while i < count {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let inputs = m.u32(upd + 0x1c);
        let ent = inputs.wrapping_add((i as u32).wrapping_mul(0x14));
        let crv_model = m.u32(ent + 8);
        if m.i16(ent + 0xc) == 0 || crv_model == 0 {
            si_nurbs::image::srf_dispose(m, o, copy_pp);
            return;
        }
        let crv = m.u32(crv_model + 0x48);
        if m.i32(crv_model + 0x30) != 9 || crv == 0 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_CURVE, 2]);
            o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[srf_model, 0x101]);
            o.call(m, SLEEP, &[1]);
            si_nurbs::image::srf_dispose(m, o, copy_pp);
            return;
        }
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let crv_elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[crv_model, l8]).eax;
        let esi = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[model, l8]).eax;
        let chap = m.i32(CID_MODELS);
        let ed = Ptr32::new(elem_dict(m));
        let mut rel = elemrel::find_related_elem(m, ed, Ptr32::new(esi), 1, 2, 0x101, chap, Ptr32::new(l8), Ptr32::new(l8)).addr();
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        while rel != 0 && crv_elem != rel {
            let ed = Ptr32::new(elem_dict(m));
            rel = elemrel::find_next_related_elem(m, ed, Ptr32::new(esi), 1, 2, 0x101, chap, Ptr32::new(l8), Ptr32::new(l8)).addr();
        }
        if rel == 0 {
            si_nurbs::image::srf_dispose(m, o, copy_pp);
            return;
        }
        let ed = Ptr32::new(elem_dict(m));
        let cur = elemrel::get_cur_relation(m, ed, Ptr32::new(esi), 1, 2).addr();
        if m.i16(cur + 0x14) == 0 {
            si_nurbs::image::srf_dispose(m, o, copy_pp);
            return;
        }
        let r = m.u32(cur + 0x18);
        let mat_a = srf_model + 0x1c4;
        let tree = srf + 0xd8;
        if m.u32(tree) != 0 {
            let other = m.u32(m.u32(tree) + 4);
            if !same_matrix(m, mat_a, other) {
                si_gen::tree::destroy(m, o, tree, BEZ_PATCH_DESTROY);
            }
        }
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let args = CreateArgs {
            trimming: m.u32(r + 0x38),
            projection: m.u32(r),
            coord: m.u32(r + 0x34),
            center: six(m, r + 0x1c),
            dir: six(m, r + 4),
            mat_a,
            mat_b: crv_model + 0x1c4,
            step: m.u32(crv + 0x34),
            surface_op: m.u32(r + 0x44),
        };
        let t = trim::create_p(m, o, m.u32(copy_pp), crv, &args);
        m.set_u32(trim_pp, t);
        m.set_u32(t + 4, tree);
        if trim::preview(m, o, t) as u16 == 0 {
            trim::destroy(m, o, trim_pp);
        } else {
            let list = m.u32(m.u32(t) + 0xd0);
            let mut e = si_gen::list::iter_init(m, list, first, last);
            let mut k: i32 = 0;
            o.call(m, SI_SHOW_ACTIVITY, &[]);
            let flags = m.u32(r + 0x40);
            if m.i16(first) != 1 && 0 < m.i32(r + 0x3c) {
                loop {
                    let data = si_gen::list::elem_get_data(m, e);
                    let v = m.u16(flags.wrapping_add((k as u32).wrapping_mul(2)));
                    k += 1;
                    if v == 1 {
                        m.set_u8(data + 0x24, m.u8(data + 0x24) | 1);
                    }
                    e = si_gen::list::iter_next(m, list, first, last);
                    if !(m.i16(first) != 1 && k < m.i32(r + 0x3c)) {
                        break;
                    }
                }
            }
            trim::show_preview(m, m.u32(trim_pp), 0);
            trim::trim(m, o, m.u32(trim_pp));
            trim::destroy(m, o, trim_pp);
        }
        i += 2;
    }
    o.call(m, SI_SHOW_ACTIVITY, &[]);
    let copy = m.u32(copy_pp);
    defs::substitute_idefinition(m, &mut ModelS(o), Ptr32::new(model), 0, copy);
    touch::touch_geometry(m, &mut ModelS(o), Ptr32::<Model>::new(model));
    let gl = graf_list(m);
    let gn = si_view::graflist::find_gl_data(m, gl, Ptr32::new(model), 2);
    si_view::func::update_gn_func(m, &mut ViewS(o), gn);
}

/// `SI_matrixEqual (0x085161c4)` on two matrices of the image.
fn same_matrix(m: &Mem, a: u32, b: u32) -> bool {
    let mut x = [[0f32; 4]; 4];
    let mut y = [[0f32; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            x[r][c] = f32::from_bits(m.u32(a + 16 * r as u32 + 4 * c as u32));
            y[r][c] = f32::from_bits(m.u32(b + 16 * r as u32 + 4 * c as u32));
        }
    }
    si_math::algebra::matrix_equal(&x, &y)
}
