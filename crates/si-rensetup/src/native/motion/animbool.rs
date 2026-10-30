// SPDX-License-Identifier: MIT
//! The animated boolean (`SI_animBooleanUpdate`, relation 245) in the update: `si-boolean`'s port
//! (`si_boolean::ops::si_anim_boolean_update`, checked against the emulated original by bool2) on the image, with
//! the routines it and the boolean engine call routed to the ports that exist (routing7, finding 0023 section 43).
//!
//! Nothing here reproduces a function of the original except `SI_modelConvertToMesh (0x084587d4)`, which the
//! port of `meshUtil.c` has only in value form: its image form below follows the listing (a switch on the kind at
//! +0x30 over the definition at +0x48: 1 patch, 2 face, 4 mesh copy, 7 metaball system, 10 NURBS surface, any
//! other kind null). The rest is wiring; a routine with no port is answered 0 and counted under its name.

use super::*;
use si_mem::Ret as MRet;
use si_mem::Routine;
use si_model::mesh_image as mi;
use si_math::types::Vector;

/// On (the default); `RENSETUP_NO_ROUTING7=1` leaves `anim_boolean_update` unanswered as before.
pub(super) fn routing7_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_ROUTING7").is_none())
}

fn mat(m: &Mem, a: u32) -> si_math::types::Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + (16 * i + 4 * j) as u32);
        }
    }
    out
}

fn put_mat(m: &mut Mem, a: u32, v: &si_math::types::Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + (16 * i + 4 * j) as u32, *e);
        }
    }
}

/// `si-view`'s outside world for `SI_updateGnFunc`: the patch's surface update is `si-spline`'s port, the rest 0.
struct ViewSide<'a>(&'a RefCell<Calls>);
impl si_view::Outside for ViewSide<'_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_view::Routine, a: &[u32]) -> u32 {
        if r.addr == 0x0847_cee4 {
            si_spline::patch_image::patch_update_surface(m, Ptr32::new(a.first().copied().unwrap_or(0)));
            self.0.borrow_mut().entry(r.name).or_default().0 += 1;
        } else {
            self.0.borrow_mut().entry(r.name).or_default().1 += 1;
        }
        0
    }
}

/// `SI_modelConvertToMesh (0x084587d4)`: see the module comment.
fn model_convert_to_mesh(m: &mut Mem, calls: &RefCell<Calls>, node: u32) -> u32 {
    if node == 0 {
        return 0;
    }
    let def = m.u32(node + 0x48);
    match m.u32(node + 0x30) {
        1 => mi::patch::patch_convert_to_mesh(m, &mut ModelSide(calls), node, def),
        2 => mi::face::face_convert_to_mesh(m, &mut ModelSide(calls), def, 0),
        4 => mi::obj::mesh_copy(m, &mut ModelSide(calls), def),
        7 => si_render::softvertex_image::meta_to_mesh(m, &mut Quiet(calls, "callees of SI_meta2Mesh"), def),
        10 => match mi::nurbs::nurbs_convert_to_mesh(m, &mut ModelSide(calls), node, def) {
            Ok(v) => v,
            Err(_) => {
                calls.borrow_mut().entry("SI_nurbsConvertToMesh (stopped on a trimmed surface)").or_default().1 += 1;
                0
            }
        },
        _ => 0,
    }
}

/// The outside world of `si_boolean::ops::si_anim_boolean_update`, by address.
pub(super) struct AnimBoolSide<'a>(pub &'a RefCell<Calls>);
impl si_mem::Outside for AnimBoolSide<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> MRet {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let v: Option<u32> = match r.addr {
            // ---- si-model
            0x0846_78a8 => {
                si_model::model_image::readjust::delete_definition(m, &mut ReadjustSide(calls), Ptr32::new(w(0)), w(1) as i16);
                Some(0)
            }
            0x0846_7038 => Some(si_model::model_image::setup::add_definition(m, Ptr32::new(w(0)), w(1) as i32, w(2))),
            0x0846_d48c => {
                si_model::model_image::touch::touch_geometry(m, &mut ReadjustSide(calls), Ptr32::new(w(0)));
                Some(0)
            }
            0x0846_1f98 => {
                si_model::model_image::state::copy_transformation(m, &mut ModelSide(calls), Ptr32::new(w(0)), Ptr32::new(w(1)));
                Some(0)
            }
            0x0845_87d4 => Some(model_convert_to_mesh(m, calls, w(0))),
            0x0844_04a0 => {
                mi::obj::mesh_dispose(m, w(0));
                Some(0)
            }
            0x0844_04e0 => Some(mi::obj::mesh_copy(m, &mut ModelSide(calls), w(0))),
            0x0844_3144 => Some(mi::tri::mesh_triangulate(m, &mut ModelSide(calls), w(0), w(1) as i16) as u16 as u32),
            0x0845_8848 => Some(si_model::mesh_util::mesh_planar(&mi::to_value(m, w(0))) as u32),
            0x0845_8c20 => Some(mi::mesh_allocate(m)),
            0x0845_8bc8 => {
                mi::obj::mesh_init_vertex_colors(m, w(0));
                Some(0)
            }
            0x0845_8cf8 => Some(mi::rebuild::update_nodes(m, &mut ModelSide(calls), w(0), w(1) as i16) as u16 as u32),
            0x0845_8d94 => {
                mi::rebuild::update(m, &mut ModelSide(calls), w(0));
                Some(0)
            }
            0x0844_1cd8 => {
                mi::obj::mesh_transform(m, w(0), w(1), w(2));
                Some(0)
            }
            0x0845_3cd0 => Some(mi::obj::nodes_dispose(m, w(0), w(1) as i32, w(2) as i32) as u32),
            0x0845_3d44 => Some(mi::obj::nodes_copy(m, w(0), w(1), w(2), w(3)) as u32),
            0x0845_3538 => Some(mi::edit_fct::node_set_uv(m, w(0) as i32, f32::from_bits(w(1)), f32::from_bits(w(2)), w(3)) as u32),
            0x0845_34d8 => {
                // SI_nodeGetUV(corner, d, &u, &v): the pair at +0x14, or entry d - 1 of the array at +0x20
                let (node, d) = (w(0), w(1) as i32);
                let (u, v) = if d < 1 || m.u32(node + 0x20) == 0 {
                    (m.f32(node + 0x14), m.f32(node + 0x18))
                } else {
                    let p = m.u32(node + 0x20).wrapping_add((d as u32).wrapping_mul(8)).wrapping_sub(8);
                    (m.f32(p), m.f32(p.wrapping_add(4)))
                };
                m.set_f32(w(2), u);
                m.set_f32(w(3), v);
                Some(0)
            }
            0x0845_a694 => {
                let n = w(1) as i16 as i32;
                let corners: Vec<i32> = (0..n.max(0) as usize).map(|k| w(2 + k) as i32).collect();
                mi::rebuild::polygon_initialize(m, w(0), &corners);
                Some(0)
            }
            0x0851_dadc => Some(mi::tri::triangulate_contour(m, w(0) as i32, w(1), w(2) as i32, w(3), w(4), w(5)) as u16 as u32),
            // ---- si-hrc: the allocators
            0x0845_9d48 => Some(si_hrc::image::geom::vertex_allocate(m, w(0) as i32)),
            0x0845_9e20 => Some(si_hrc::image::geom::polygon_allocate(m, w(0) as i32)),
            // ---- si-math
            0x0851_672c => Some(match si_math::algebra::matrix_inverse(&mat(m, w(0))) {
                Some(inv) => {
                    put_mat(m, w(1), &inv);
                    1
                }
                None => 0,
            }),
            0x0851_6244 => {
                let p = si_math::algebra::matrix_multiply(&mat(m, w(0)), &mat(m, w(1)));
                put_mat(m, w(2), &p);
                Some(w(2))
            }
            0x0851_8a3c => {
                let (t, ok) = si_math::algebra::matrix_transpose_inverse(&mat(m, w(0)));
                put_mat(m, w(1), &t);
                Some(ok as u32)
            }
            0x0851_8aa0 => {
                let p = w(0);
                let z = si_math::algebra::matrix_z_align(&Vector { x: m.f32(p), y: m.f32(p + 4), z: m.f32(p + 8) });
                put_mat(m, w(1), &z);
                Some(0)
            }
            // ---- si-view
            0x0832_ee68 => Some(si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0),
            0x0817_6618 => {
                si_view::func::update_gn_func(m, &mut ViewSide(calls), Ptr32::new(w(0)));
                Some(0)
            }
            // ---- si-scene
            0x0810_8490 => {
                si_scene::matconv::db_convert_model_mat_txt2db(&mut Lookups(calls), m, w(0), w(1) as i16, w(2) as i16, w(3));
                Some(0)
            }
            0x0828_4888 => Some(si_scene::matconv::material_copy(&mut Lookups(calls), m, w(0))),
            // ---- si-app's allocator
            0x080d_0ee8 => Some(m.si_malloc(w(0))),
            // ---- si-boolean: its own splitter, and the stand-ins for bool_util.c
            0x0831_3b74 => {
                si_boolean::split::si_split_face(m, self, w(0), w(1), w(2), w(3));
                Some(0)
            }
            _ => None,
        };
        let v = match v {
            Some(v) => Some(v),
            None => si_boolean::ops::bool_util_stand_in(m, r.addr, a),
        };
        let mut t = calls.borrow_mut();
        let e = t.entry(r.name).or_default();
        match v {
            Some(v) => {
                e.0 += 1;
                MRet::eax(v)
            }
            None => {
                e.1 += 1;
                MRet::eax(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::Outside;

    #[test]
    fn routing7_animated_boolean_callees_reach_the_ports() {
        let calls = RefCell::new(Calls::new());
        let mut m = Mem::new();
        let (a, out) = (m.si_malloc(64), m.si_malloc(64));
        for k in 0..16u32 {
            m.set_f32(a + 4 * k, if k % 5 == 0 { 2.0 } else { 0.0 });
        }
        let r = AnimBoolSide(&calls).call(&mut m, Routine::new("SI_matrixInverse", 0x0851_672c), &[a, out]);
        assert_eq!(r.eax, 1);
        assert_eq!(m.f32(out), 0.5);
        // a routine with no port is answered 0 and counted as such
        let r = AnimBoolSide(&calls).call(&mut m, Routine::new("SI_statusDisplayMessage", 0x083c_ca94), &[0, 2]);
        assert_eq!(r.eax, 0);
        let c = calls.borrow();
        assert_eq!(c["SI_matrixInverse"], (1, 0));
        assert_eq!(c["SI_statusDisplayMessage"], (0, 1));
    }
}
