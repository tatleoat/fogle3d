// SPDX-License-Identifier: MIT
//! `shdb_init.c`: the preview scene's defaults and the parser of a shader's `_SHADER_PREVIEWER`
//! options into it (finding 0041).
//!
//! The scene record (named offsets, layout not established): +0x10 .. light/camera floats,
//! +0x20/+0x24 two ints, +0x28/+0x2c and +0x34/+0x38 window sizes (shorts at +0x30), +0x3c/+0x3e render
//! flags, +0x40 the sphere definition, +0x48 the plane definition (+0x60 default material,
//! +0x9c checker material), +0xe4 the cutplane, +0x10c the pointer to the light.

use crate::shdb_support::{si_shdb_free_all_nodes, CHILD, NEXT};
use crate::through_x87;
use si_mem::{Mem, Outside, Routine};

/// `SI_statusDisplayMessage (0x083cca94)`: (text, kind). Not ported.
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `strErrParse`, `strWarnParse` (pointer variables).
const STR_ERR_PARSE: u32 = 0x086e_5610;
const STR_WARN_PARSE: u32 = 0x086e_5654;
/// The preview light (.bss) and its default (.data, 10 words).
const PREVIEW_LIGHT: u32 = 0x0882_4820;
const DEFAULT_LIGHT: u32 = 0x086e_5680;
const MTL_NAME: u32 = 0x086e_56a8;

/// A float copied through the x87 stack.
fn fcopy(m: &mut Mem, dst: u32, bits: u32) {
    m.set_u32(dst, through_x87(bits));
}

/// The float constant at `addr` (.rodata) stored through the x87 stack to each of `dst`.
fn fconst(m: &mut Mem, addr: u32, dst: &[u32]) {
    let b = m.u32(addr);
    for &d in dst {
        fcopy(m, d, b);
    }
}

/// `fistp` with the truncating rounding mode: out-of-range and NaN give the integer indefinite.
fn fistp_trunc(bits: u32) -> i32 {
    let f = f32::from_bits(bits);
    if f.is_nan() || f >= 2147483648.0 || f < -2147483648.0 {
        i32::MIN
    } else {
        f as i32
    }
}

/// Copies `n` words from node +4.. to `dst`.. (plain integer moves, no x87).
fn words(m: &mut Mem, node: u32, dst: u32, n: u32) {
    for i in 0..n {
        let v = m.u32(node + 4 + 4 * i);
        m.set_u32(dst + 4 * i, v);
    }
}

/// [soft 0x0854f030 si_shdbParseAll]: parses the shader text of `scene` (+0x24 -> record whose +8 is the
/// text) into `out` (root +0, error count +4, error strings +8) and shows the messages.
/// Answers 0 without a shader, 1 without text, else 1 when the parse reports no error status.
pub fn si_shdb_parse_all(m: &mut Mem, o: &mut dyn Outside, scene: u32, out: u32) -> i32 {
    if scene == 0 {
        return 0;
    }
    let def = m.u32(scene + 0x24);
    if def == 0 {
        return 0;
    }
    let text = m.u32(def + 8);
    if text == 0 {
        return 1;
    }
    let status = crate::shdb_support::si_shdb_parse(m, o, text, out + 4, out + 8, out) as i16;
    if status != 0 {
        let s = m.u32(STR_ERR_PARSE);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, 3]);
    } else if m.i32(out + 4) > 0 {
        let s = m.u32(STR_WARN_PARSE);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, 2]);
    }
    let n = m.i32(out + 4);
    let mut i = 0;
    while i < n {
        let errs = m.u32(out + 8);
        let s = m.u32(errs + 4 * i as u32);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, if status != 0 { 3 } else { 2 }]);
        i += 1;
    }
    (status == 0) as i32
}

/// [soft 0x0854f228 initPlaneDefaultMaterial]
pub fn init_plane_default_material(m: &mut Mem, p: u32) {
    fconst(m, 0x0866_36fc, &[p, p + 8, p + 4]);
    m.set_u32(p + 0xc, 0);
    fconst(m, 0x0866_3700, &[p + 0x10, p + 0x18, p + 0x14]);
    m.set_u32(p + 0x1c, 0);
    fconst(m, 0x0866_3704, &[p + 0x20, p + 0x28, p + 0x24]);
    m.set_u32(p + 0x2c, 0);
    m.set_u32(p + 0x30, 0);
    fconst(m, 0x0866_3708, &[p + 0x34]);
    m.set_u32(p + 0x38, 0);
}

/// [soft 0x0854f28c initPlaneCheckerMaterial]
pub fn init_plane_checker_material(m: &mut Mem, p: u32) {
    fconst(m, 0x0866_370c, &[p, p + 8, p + 4]);
    m.set_u32(p + 0xc, 0);
    fconst(m, 0x0866_370c, &[p + 0x10, p + 0x18]);
    m.set_u32(p + 0x14, 0);
    m.set_u32(p + 0x1c, 0);
    m.set_u32(p + 0x20, 0);
    m.set_u32(p + 0x28, 0);
    fconst(m, 0x0866_370c, &[p + 0x24]);
    m.set_u32(p + 0x2c, 0);
    fconst(m, 0x0866_3710, &[p + 0x38, p + 0x40, p + 0x3c]);
    m.set_u32(p + 0x44, 0);
    fconst(m, 0x0866_3714, &[p + 0x30]);
    fconst(m, 0x0866_3718, &[p + 0x34]);
}

/// [soft 0x0854f0e0 si_shdbInitScene]: the defaults of the preview scene at `p`.
pub fn si_shdb_init_scene(m: &mut Mem, p: u32) {
    fconst(m, 0x0866_36e0, &[p + 0x10]);
    m.set_u32(p + 0x14, 0);
    fconst(m, 0x0866_36e4, &[p + 0x18]);
    m.set_u32(p + 0x1c, 0x3f80_0000);
    fconst(m, 0x0866_36e8, &[p + 0x40]);
    fconst(m, 0x0866_36ec, &[p + 0x44]);
    fconst(m, 0x0866_36f0, &[p + 0x4c]);
    fconst(m, 0x0866_36f4, &[p + 0x50, p + 0x54]);
    m.set_u16(p + 0x30, 1);
    m.set_u32(p + 0x34, 0x12c);
    m.set_u32(p + 0x38, 0x12c);
    m.set_u32(p + 0x28, 0x80);
    m.set_u32(p + 0x2c, 0x80);
    m.set_u32(p + 0x20, 0xffff_fffe);
    m.set_u32(p + 0x24, 0xffff_ffff);
    m.set_u16(p + 0x48, 0);
    m.set_u32(p + 0x58, MTL_NAME);
    m.set_u16(p + 0x5c, 0);
    init_plane_default_material(m, p + 0x60);
    init_plane_checker_material(m, p + 0x9c);
    m.set_u32(p + 0xec, 0);
    m.set_u32(p + 0xf0, 0);
    m.set_u32(p + 0xf4, 0x3f80_0000);
    m.set_u32(p + 0xf8, 0x3f80_0000);
    m.set_u32(p + 0xfc, 0x3f80_0000);
    fconst(m, 0x0866_36f8, &[p + 0x100, p + 0x104]);
    fconst(m, 0x0866_36e8, &[p + 0x108]);
    m.set_u16(p + 0xe4, 0);
    m.set_u32(p + 0xe8, 2);
    for i in 0..10 {
        let v = m.u32(DEFAULT_LIGHT + 4 * i);
        m.set_u32(PREVIEW_LIGHT + 4 * i, v);
    }
    m.set_u32(p + 0x10c, PREVIEW_LIGHT);
    m.set_u16(p + 0x3c, 0);
    m.set_u16(p + 0x3e, 0);
}

/// [soft 0x0854f3ec setPreviewSphereOptions]: `sphere` (two floats), the material record `mtl`, the
/// node list `node` (walked by the first-child link).
pub fn set_preview_sphere_options(m: &mut Mem, sphere: u32, mtl: u32, mut node: u32) {
    while node != 0 {
        let b = |m: &Mem, i: u32| m.u32(node + i);
        match m.u32(node) {
            8 => { let v = b(m, 4); fcopy(m, sphere + 4, v) }
            9 => { let v = b(m, 4); fcopy(m, sphere, v) }
            0x10 => { let v = b(m, 4); fcopy(m, mtl + 0x6b8, v) }
            0x11 => words(m, node, mtl + 0x34, 4),
            0x12 => words(m, node, mtl + 0x44, 4),
            0x13 => words(m, node, mtl + 0x54, 4),
            0x14 => { let v = b(m, 4); fcopy(m, mtl + 0x64, v) }
            0x15 => { let v = b(m, 4); fcopy(m, mtl + 0x6a8, v) }
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f4c4 setPreviewPlaneOptions]
pub fn set_preview_plane_options(m: &mut Mem, p: u32, mut node: u32) {
    while node != 0 {
        let ty = m.u32(node);
        let w1 = m.u32(node + 4);
        match ty {
            2 => m.set_u16(p, 1),
            8 => {
                let w2 = m.u32(node + 8);
                fcopy(m, p + 8, w1);
                fcopy(m, p + 0xc, w2);
            }
            9 => fcopy(m, p + 4, w1),
            0xf | 0x17 => {
                if m.u16(p + 0x14) == 0 {
                    if ty == 0x17 {
                        init_plane_checker_material(m, p + 0x54);
                    }
                } else if ty == 0xf {
                    init_plane_default_material(m, p + 0x18);
                }
                m.set_u16(p + 0x14, (ty == 0x17) as u16);
            }
            0x10 => fcopy(m, p + 0x50, w1),
            0x11 => words(m, node, p + 0x18, 4),
            0x12 => {
                if m.u16(p + 0x14) == 0 {
                    words(m, node, p + 0x28, 4);
                } else {
                    words(m, node, p + 0x8c, 4);
                }
            }
            0x13 => words(m, node, p + 0x38, 4),
            0x14 => fcopy(m, p + 0x4c, w1),
            0x15 => fcopy(m, p + 0x48, w1),
            0x18 => words(m, node, p + 0x74, 4),
            0x19 => words(m, node, p + 0x54, 4),
            0x1a => words(m, node, p + 0x64, 4),
            0x1b => fcopy(m, p + 0x84, w1),
            0x1c => fcopy(m, p + 0x88, w1),
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f6a0 setPreviewCutplaneOptions]
pub fn set_preview_cutplane_options(m: &mut Mem, p: u32, mut node: u32) {
    while node != 0 {
        let w1 = m.u32(node + 4);
        match m.u32(node) {
            3 => m.set_u16(p, 1),
            9 => {
                fcopy(m, p + 8, w1);
                m.set_u32(p + 4, 0);
            }
            12 => {
                fcopy(m, p + 0xc, w1);
                m.set_u32(p + 4, 1);
            }
            10 => words(m, node, p + 0x10, 3),
            11 => {
                words(m, node, p + 0x1c, 3);
                m.set_u32(p + 4, 2);
            }
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f724 setPreviewLightOptions]
pub fn set_preview_light_options(m: &mut Mem, p: u32, mut node: u32) {
    while node != 0 {
        match m.u32(node) {
            13 => words(m, node, p + 0x10, 4),
            7 => words(m, node, p + 4, 3),
            14 => {
                let v = m.u16(node + 4);
                m.set_u16(p + 0x20, v);
            }
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f78c setPreviewWindowOptions]: float pairs truncated to integers.
pub fn set_preview_window_options(m: &mut Mem, p: u32, mut node: u32) {
    while node != 0 {
        let (a, b) = (m.u32(node + 4), m.u32(node + 8));
        match m.u32(node) {
            7 => {
                m.set_i32(p + 0x34, fistp_trunc(a));
                m.set_i32(p + 0x38, fistp_trunc(b));
            }
            8 => {
                m.set_i32(p + 0x28, fistp_trunc(a));
                m.set_i32(p + 0x2c, fistp_trunc(b));
            }
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f84c setPreviewRenderOptions]
pub fn set_preview_render_options(m: &mut Mem, p: u32, mut node: u32) {
    while node != 0 {
        let w = m.u32(node + 4);
        match m.u32(node) {
            0x1d => m.set_u32(p + 0x20, w),
            0x1e => m.set_u32(p + 0x24, w),
            0x1f => m.set_u16(p + 0x3c, w as u16),
            0x20 => m.set_u16(p + 0x3e, w as u16),
            _ => {}
        }
        node = m.u32(node + CHILD);
    }
}

/// [soft 0x0854f300 si_shdbSetOptions]: parses the shader's options and applies each top-level
/// section to the scene `p` (`mtl` goes to the sphere's material); the error count and strings go
/// through `nerrs`/`errs`. Answers 0 when the parse failed.
pub fn si_shdb_set_options(m: &mut Mem, o: &mut dyn Outside, p: u32, shader: u32, mtl: u32, nerrs: u32, errs: u32) -> i32 {
    let out = m.local(12);
    m.set_u32(out, 0);
    m.set_u32(out + 4, 0);
    m.set_u32(out + 8, 0);
    let ok = si_shdb_parse_all(m, o, shader, out) as i16;
    let (ne, es) = (m.u32(out + 4), m.u32(out + 8));
    m.set_u32(nerrs, ne);
    m.set_u32(errs, es);
    if ok == 0 {
        si_shdb_free_all_nodes(m);
        m.release(12);
        return 0;
    }
    let mut node = m.u32(out);
    while node != 0 {
        match m.u32(node).wrapping_sub(1) {
            0 => set_preview_sphere_options(m, p + 0x40, mtl, node),
            1 => set_preview_plane_options(m, p + 0x48, node),
            2 => set_preview_cutplane_options(m, p + 0xe4, node),
            3 => {
                let l = m.u32(p + 0x10c);
                set_preview_light_options(m, l, node)
            }
            4 => set_preview_window_options(m, p, node),
            5 => set_preview_render_options(m, p, node),
            _ => {}
        }
        node = m.u32(node + NEXT);
    }
    si_shdb_free_all_nodes(m);
    m.release(12);
    1
}
