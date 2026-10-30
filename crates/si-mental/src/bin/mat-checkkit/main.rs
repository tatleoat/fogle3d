// SPDX-License-Identifier: MIT
//! `mat-checkkit <directory of vector files>`: the checks of `mi_material.c` (finding 0036) on `si-checkkit`.
//! Vectors: `re/tools/si-mental/genmat.sh` (specs by `mkmat.py`).

use si_checkkit::Kit;
use si_mental::mi_material as q;
use si_mental::mi_texture as q_t;

fn main() {
    Kit::new(&[
        ("check_selective_lights", |m, o, a| q::check_selective_lights(m, o, a.v("dict")).into()),
        ("init_materials", |m, o, a| { q::init_materials(m, o, a.v("dict")); ().into() }),
        ("mi_print_soft_fog", |m, o, _a| { q::mi_print_soft_fog(m, o); ().into() }),
        ("print_light_list", |m, o, a| { q::print_light_list(m, o, a.v("dict"), a.i("index") as u32, a.i("first") as u32); ().into() }),
        ("print_default_material", |m, o, a| { q::print_default_material(m, o, a.v("dict"), a.v("elem")); ().into() }),
        ("print_3d_map", |m, o, a| q::print_3d_map(m, o, a.i("mode") as u32, a.v("tex"), a.v("fp"), a.v("name"), a.v("a"), a.v("b")).into()),
        ("print_2d_map", |m, o, a| q::print_2d_map(m, o, a.i("mode") as u32, a.v("tex"), a.v("a"), a.v("b"), a.v("c")).into()),
        ("print_extension", |m, o, a| { q::print_extension(m, o, a.v("dict"), a.v("elem"), a.v("owner"), a.v("sel"), a.i("extra") as u32, a.i("twin") as u16); ().into() }),
        ("print_mat_with_extension", |m, o, a| { q::print_mat_with_extension(m, o, a.v("dict"), a.v("mat"), a.v("owner"), a.v("done")); ().into() }),
        ("flagModelWithSelectiveLight", |m, o, a| { q::flag_model_with_selective_light(m, o, a.v("dict")); ().into() }),
        ("print_all_material", |m, o, a| { q::print_all_material(m, o, a.v("dict"), a.i("a") as u32); ().into() }),
        ("mi_print_soft_material", |m, o, a| {
            let w = [a.v("dict"), a.v("elem"), a.v("mat"), a.v("name1"), a.v("name2"), a.v("arg1c"), a.v("tex20"), a.v("tex24"), a.v("tex28"), a.i("n2c") as u32, a.i("n30") as u32, a.i("n34") as u32, a.i("last") as u32];
            q::mi_print_soft_material(m, o, &w);
            ().into()
        }),
        ("need_texture", |m, _o, a| q_t::need_texture(m, a.v("tex")).into()),
        ("need_3D_texture", |m, _o, a| q_t::need_3d_texture(m, a.v("tex")).into()),
        ("SI_initTxtSeqCounter", |m, _o, _a| { q_t::si_init_txt_seq_counter(m); ().into() }),
        ("find_empty_entry", |m, _o, _a| q_t::find_empty_entry(m).into()),
        ("lookup_2D_texture_list", |m, _o, a| q_t::lookup_2d_texture_list(m, a.v("name")).into()),
        ("mi2_lookup_2D_texture_list", |m, _o, a| q_t::mi2_lookup_2d_texture_list(m, a.v("name")).into()),
        ("calc_2D_matrix", |m, o, a| { q_t::calc_2d_matrix(m, o, a.v("mat"), a.v("tex")); ().into() }),
        ("calc_3D_matrix", |m, o, a| { q_t::calc_3d_matrix(m, o, a.v("mat"), a.v("tex")); ().into() }),
        ("print_matrix", |m, o, a| { q_t::print_matrix(m, o, a.v("mat")); ().into() }),
        ("SI_init_MRSoftMaterial", |m, o, a| { q::si_init_mr_soft_material(m, o, a.v("dict"), a.v("elem"), a.v("out")); ().into() }),
    ])
    .stand_in("strdup", |m, a| si_mem::Ret::eax(si_mem::clib::strdup(m, a[0])))
    .stand_in("free", |m, a| { m.free(a[0]); si_mem::Ret::NONE })
    .stand_in("getenv", getenv)
    .effect(effect)
    .normalise_stack_args()
    .run()
}

/// What the check's stand-ins do to the image (the same as `re/tools/si-mental/genmat.py`).
fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], answer: u32) -> Result<(), String> {
    match r.addr {
        // the related-element walkers: the answer is also written to the cursor (argument 6)
        0x0813_8094 | 0x0813_8104 | 0x0813_508c | 0x0813_5154 => {
            if a.len() > 6 && a[6] != 0 {
                m.set_u32(a[6], answer);
            }
        }
        // SI_strAppend(&p, s): *p = a made block holding the old string and s
        0x083c_f280 => {
            let p = m.u32(a[0]);
            let mut old = if p != 0 { m.cstr(p) } else { Vec::new() };
            // the port's image holds no data section: the one string appended is "_selective" (0x086ef7fc)
            if a[1] == 0x086e_f7fc {
                old.extend_from_slice(b"_selective");
            } else if a[1] != 0 {
                old.extend(m.cstr(a[1]));
            }
            let n = m.malloc(old.len() as u32 + 1);
            old.push(0);
            m.set_bytes(n, &old);
            m.set_u32(a[0], n);
        }
        // init_matrix(m): the identity
        0x0859_83ec => {
            for i in 0..16u32 {
                m.set_u32(a[0] + 4 * i, if i % 5 == 0 { 0x3f80_0000 } else { 0 });
            }
        }
        // SI_textureInitialize(rec): the stand-in zero-fills the 0x1d8-byte record
        0x082b_2508 => m.set_bytes(a[0], &[0u8; 0x1d8]),
        _ => {}
    }
    Ok(())
}

/// getenv as genm.py's `genv` answers it: two bits of the knob word at 0x08829ff0, selected by `(name >> 2) & 3`:
/// 0 null, 1 "", 2 "x", 3 "a -b".
fn getenv(m: &mut si_mem::Mem, a: &[u32]) -> si_mem::Ret {
    let knob = m.u32(0x0882_9ff0);
    let v = (knob >> (2 * ((a[0] >> 2) & 3))) & 3;
    if v == 0 {
        return si_mem::Ret::eax(0);
    }
    let p = m.malloc(16);
    m.set_bytes(p, [&b""[..], b"", b"x\0", b"a -b\0"][v as usize]);
    if v < 2 {
        m.set_u8(p, 0);
    }
    si_mem::Ret::eax(p)
}
