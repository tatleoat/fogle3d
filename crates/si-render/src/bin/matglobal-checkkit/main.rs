// SPDX-License-Identifier: MIT
//! `matglobal-checkkit <directory of vector files>`: `mat_global.c` on `si-checkkit` against the
//! emulated original (specs under `re/tools/si-render/matglobal/`, finding 0012, section 27, and
//! 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_mem::{Mem, Ret, Routine};
use si_render::mat_global::*;

/// The log buffer of `gen_mat.py`: a count, then 40-byte entries (tag, 36 bytes).
const LOG: u32 = 0x0883_0000;
const LOG_ENTRIES: u32 = 48;

fn append(m: &mut Mem, tag: u32, data: &[u8]) {
    let n = m.u32(LOG);
    if n >= LOG_ENTRIES {
        return;
    }
    let at = LOG + 4 + 40 * n;
    m.set_u32(at, tag);
    let mut b = [0u8; 36];
    let k = data.len().min(36);
    b[..k].copy_from_slice(&data[..k]);
    m.set_bytes(at + 4, &b);
    m.set_u32(LOG, n + 1);
}

/// What the generator's stand-ins leave behind (see `gen_mat.py`): the texts and the colour
/// handed to `c3f` go to the log, `SI_colorConvert` stores [b, r, g, r].
fn effect(m: &mut Mem, r: Routine, args: &[u32], _answer: u32) -> Result<(), String> {
    match r.name {
        "SI_setTexed" | "SI_setItemTitle" => {
            let mut t = m.cstr(args[2]);
            t.truncate(35);
            append(m, args[1] + if r.name == "SI_setTexed" { 0 } else { 0x100 }, &t);
        }
        "c3f" => {
            let b = m.bytes(args[0], 12).to_vec();
            append(m, 0xc3f, &b);
        }
        "SI_colorConvert" => {
            for (i, w) in [args[2], args[0], args[1], args[0]].into_iter().enumerate() {
                m.set_u32(args[5] + 4 * i as u32, w);
            }
        }
        _ => {}
    }
    Ok(())
}

/// `SI_getTexedFloat`: the float at dialog + 0x40 + 4 * item, in `st0`.
fn get_float(m: &mut Mem, args: &[u32]) -> Ret {
    let v = si_x87::F80::from_f32(m.f32(args[0] + 0x40 + 4 * args[1]));
    let (mant, se) = v.to_bits();
    let mut b = [0u8; 10];
    b[..8].copy_from_slice(&mant.to_le_bytes());
    b[8..].copy_from_slice(&se.to_le_bytes());
    Ret::st0(b)
}

fn main() {
    Kit::new(&[
        ("lightType", |m, o, a| {
            light_type(m, o, a.p::<()>("dlg").addr(), a.i("kind"));
            ().into()
        }),
        ("arealightType", |m, o, a| {
            arealight_type(m, o, a.p::<()>("dlg").addr(), a.i("kind"));
            ().into()
        }),
        ("shadowType", |m, o, a| {
            shadow_type(m, o, a.p::<()>("dlg").addr(), a.i("kind"));
            ().into()
        }),
        ("mrUdfAddToDict", |m, o, a| {
            mr_udf_add_to_dict(m, o, a.p::<()>("light").addr(), a.p::<()>("other").addr());
            ().into()
        }),
        ("putLight", |m, o, a| {
            put_light(m, o, a.p::<()>("dlg").addr(), a.p::<()>("light").addr());
            ().into()
        }),
        ("putEditLight", |m, o, a| {
            put_edit_light(m, o, a.p::<()>("dlg").addr(), a.p::<()>("light").addr(), a.p::<()>("elem").addr());
            ().into()
        }),
        ("colorSliderSet", |m, o, a| {
            color_slider_set(m, o, a.p::<()>("dlg").addr());
            ().into()
        }),
        ("modelAssociateLight", |m, o, a| {
            model_associate_light(m, o, a.p::<()>("node").addr());
            ().into()
        }),
        ("modelDisassociateLight", |m, o, a| {
            model_disassociate_light(m, o, a.p::<()>("node").addr());
            ().into()
        }),
        ("SI_selLightHandler", |m, o, a| {
            sel_light_handler(m, o, a.p::<()>("ev").addr(), a.i("mode"));
            ().into()
        }),
        ("SI_setSelSelection", |m, o, _| set_sel_selection(m, o).into()),
        ("SI_selectiveLightHandler", |m, o, a| selective_light_handler(m, o, a.p::<()>("ev").addr()).into()),
        ("SI_lightCopy", |m, o, a| light_copy(m, o, a.p::<()>("src").addr()).into()),
        ("SI_drawInfLight", |m, o, a| {
            draw_inf_light(m, o, a.p::<()>("light").addr());
            ().into()
        }),
        ("SI_drawSun", |m, o, a| {
            draw_sun(m, o, a.p::<()>("light").addr());
            ().into()
        }),
        ("refreshLightArea", |m, o, a| refresh_light_area(m, o, a.p::<()>("dlg").addr()).into()),
    ])
    .setup(|m| m.set_bytes(0x0869_efc0, b"H\0L\0S\0HLS\0H\0S\0V\0HSV\0R\0G\0B\0RGB\0"))
    .stand_in("SI_getTexedFloat", get_float)
    .effect(effect)
    .run()
}
