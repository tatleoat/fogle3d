// SPDX-License-Identifier: MIT
//! `equal-checkkit <directory of vector files>`: `equal_fct.rs` (si_textureEqual, si_texture3DEqual,
//! si_materialEqual) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/si-render/equal/gen_equal.py`, finding 0023, section 36).

use si_checkkit::Kit;
use si_render::blur_image::*;
use si_render::equal_fct::*;
use si_render::light_fct::light_dispose;
use si_render::shd_copy::*;
use si_render::process_materials::{process_materials, set_texture_flag};
use si_render::renutil_tree::*;
use si_render::softvertex_image::*;
use si_render::sun_position::light_sun_get_position;

/// The outside world with `maxPsMovement`'s answer (a float in the register, which the checks
/// record with the C library's mathematics) played back from the tape.
struct Tape<'a>(&'a mut dyn si_mem::Outside);

impl si_mem::Outside for Tape<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        if r.name == "maxPsMovement" {
            let v = si_math::trig::replay::next(r.name, &args.iter().map(|&a| u128::from(a)).collect::<Vec<_>>()).unwrap_or(si_x87::F80::ZERO);
            let (mant, se) = v.to_bits();
            let mut b = [0u8; 10];
            b[..8].copy_from_slice(&mant.to_le_bytes());
            b[8..].copy_from_slice(&se.to_le_bytes());
            return si_mem::Ret::st0(b);
        }
        self.0.call(m, r, args)
    }
}

fn main() {
    Kit::new(&[
        ("si_textureEqual", |m, _o, a| texture_equal(m, a.p::<()>("a").addr(), a.p::<()>("b").addr()).into()),
        ("si_texture3DEqual", |m, _o, a| texture3d_equal(m, a.p::<()>("a").addr(), a.p::<()>("b").addr()).into()),
        ("si_materialEqual", |m, _o, a| material_equal(m, a.p::<()>("a").addr(), a.p::<()>("b").addr()).into()),
        ("SI_processMaterials", |m, o, a| {
            process_materials(m, o, a.p::<()>("dict").addr());
            ().into()
        }),
        ("setTextureFlag", |m, o, a| {
            set_texture_flag(m, o, a.p::<()>("dict").addr(), a.p::<()>("elem").addr(), a.p::<()>("flags").addr());
            ().into()
        }),
        ("animate", |m, o, a| {
            animate(m, o, a.f("time"));
            ().into()
        }),
        ("isMoving", |m, o, a| {
            is_moving(m, &mut Tape(o), a.p::<()>("model").addr(), a.s("flag"));
            ().into()
        }),
        ("saveInitialPositions", |m, o, a| {
            save_initial_positions(m, o, a.p::<()>("model").addr());
            ().into()
        }),
        ("copyModel", |m, o, a| copy_model(m, o, a.p::<()>("model").addr(), a.s("count")).into()),
        ("blurTraverse", |m, o, a| {
            blur_traverse(m, o, a.p::<()>("model").addr(), a.f("start"), a.f("end"));
            ().into()
        }),
        ("si_blurGeometry", |m, o, a| {
            blur_geometry(m, &mut Tape(o), a.f("time"));
            ().into()
        }),
        ("copyMaterialShader", |m, o, a| {
            copy_material_shader(m, o, a.p::<()>("dst").addr(), a.p::<()>("sh").addr());
            ().into()
        }),
        ("SI_copyMaterialShaders", |m, o, a| {
            copy_material_shaders(m, o, a.p::<()>("dst").addr(), a.p::<()>("src").addr());
            ().into()
        }),
        ("copyLightShader", |m, o, a| {
            copy_light_shader(m, o, a.p::<()>("dst").addr(), a.p::<()>("sh").addr());
            ().into()
        }),
        ("SI_copyLightShader", |m, o, a| {
            copy_light_shaders(m, o, a.p::<()>("dst").addr(), a.p::<()>("src").addr());
            ().into()
        }),
        ("SI_deleteMaterialShaders", |m, o, a| {
            delete_material_shaders(m, o, a.p::<()>("mat").addr());
            ().into()
        }),
        ("SI_connectMaterialShaders", |m, o, a| {
            connect_material_shaders(m, o, a.p::<()>("mat").addr());
            ().into()
        }),
        ("SI_lightDispose", |m, o, a| {
            light_dispose(m, o, a.p::<()>("light").addr());
            ().into()
        }),
        ("SI_meta2Mesh", |m, o, a| meta_to_mesh(m, o, a.p::<()>("system").addr()).into()),
        ("SI_convertMeta2Triangles", |m, o, a| {
            convert_meta_to_triangles(
                m, o, a.p::<()>("system").addr(), a.i("material") as u32, a.i("flag"), a.p::<()>("nvert").addr(),
                a.p::<()>("verts").addr(), a.p::<()>("ntri").addr(), a.p::<()>("tris").addr(), a.s("detail"),
            );
            ().into()
        }),
        ("SI_convertMesh2Triangles", |m, o, a| {
            convert_mesh_to_triangles(
                m, o, a.p::<()>("mesh1").addr(), a.p::<()>("mesh2").addr(), a.i("nmat"), a.i("material") as u32, a.i("flag"),
                a.p::<()>("nvert").addr(), a.p::<()>("verts").addr(), a.p::<()>("ntri").addr(), a.p::<()>("tris").addr(),
            );
            ().into()
        }),
        ("SI_convertFace2triangle", |m, o, a| {
            convert_face_to_triangles(
                m, o, a.p::<()>("face").addr(), a.p::<()>("face2").addr(), a.i("material") as u32, a.i("flag"),
                a.p::<()>("count").addr(), a.p::<()>("verts").addr(), a.p::<()>("nvert").addr(), a.p::<()>("tris").addr(),
            );
            ().into()
        }),
        ("branchStatus", |m, _o, a| {
            branch_status(m, a.p::<()>("node").addr());
            ().into()
        }),
        ("si_treestatus", |m, o, _a| {
            tree_status(m, o);
            ().into()
        }),
        ("writeTree", |m, o, a| {
            write_tree(m, o, a.p::<()>("node").addr());
            ().into()
        }),
        ("createScanLine", |m, _o, a| create_scan_line(m, a.i("row"), a.i("width")).into()),
        ("si_incrustration", |m, _o, a| {
            incrustration(m, a.i("width"));
            ().into()
        }),
        ("SI_lightSunGetPosition", |m, o, a| {
            light_sun_get_position(m, o, a.p::<()>("light").addr(), a.p::<()>("out").addr());
            ().into()
        }),
    ])
    // the stand-ins of gen_vertex.py: the mesh allocators, the corners, the contour and the matrix
    .stand_in("SI_meshAllocate", |m, _a| si_mem::Ret::eax(zeroed(m, 0x30)))
    .stand_in("SI_vertexAllocate", |m, a| si_mem::Ret::eax(zeroed(m, a[0].wrapping_mul(0x14))))
    .stand_in("SI_polygonAllocate", |m, a| si_mem::Ret::eax(zeroed(m, a[0].wrapping_mul(0x18))))
    .stand_in("SI_polygonInitialize", |m, a| {
        let c = zeroed(m, a[1].wrapping_mul(0x24));
        for k in 0..a[1].min(3) {
            m.set_u32(c + 0x24 * k, a[2 + k as usize]);
        }
        m.set_u32(a[0] + 0xc, c);
        m.set_u16(a[0] + 0x10, a[1] as u16);
        si_mem::Ret::eax(0)
    })
    .stand_in("SI_matrixZAlign", |m, a| {
        let n = [m.u32(a[0]), m.u32(a[0] + 4), m.u32(a[0] + 8)];
        for r in 0..3u32 {
            for c in 0..4u32 {
                let w = if c < 3 { n[((r + c) % 3) as usize] ^ if (r * 4 + c) & 1 != 0 { 0x8000_0000 } else { 0 } } else { 0 };
                m.set_u32(a[1] + 16 * r + 4 * c, w);
            }
        }
        for (c, v) in [0.25f32, -0.5, 1.0, 1.0].into_iter().enumerate() {
            m.set_f32(a[1] + 48 + 4 * c as u32, v);
        }
        si_mem::Ret::eax(0)
    })
    .stand_in("SI_triangulateContour", |m, a| {
        let n = a[0];
        if n < 3 || n > 0x7fff_ffff {
            return si_mem::Ret::eax(0);
        }
        let nt = n - 2;
        let l = m.malloc(12 * nt);
        for i in 0..nt {
            m.set_u32(l + 12 * i, 0);
            m.set_u32(l + 12 * i + 4, i + 1);
            m.set_u32(l + 12 * i + 8, i + 2);
        }
        m.set_u32(a[4], nt);
        m.set_u32(a[5], l);
        si_mem::Ret::eax(1)
    })
    // the binary's initialised data (eqTime's table)
    .setup(si_render::data::init)
    .normalise_stack_args()
    .given_lib_column()
    .run()
}

/// A zeroed block of `n` bytes (at least 4), as gen_vertex.py's allocators make them.
fn zeroed(m: &mut si_mem::Mem, n: u32) -> u32 {
    let n = n.max(4);
    let p = m.malloc(n);
    for i in 0..n {
        m.set_u8(p + i, 0);
    }
    p
}
