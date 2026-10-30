// SPDX-License-Identifier: MIT
//! `SI_processMaterials` and `setTextureFlag` of `materials.c` (finding 0023, section 36) on the
//! memory image: the pass that, before a frame, turns the render set-up's switches and each
//! material's colours into the derived fields the shaders read (the scaled colours at
//! +0x700..+0x73c, the reflection / transparency / refraction amounts at +0x740..+0x74c, the
//! need flags at +0x6e0) and scales every texture's colours by the ambient mean. The other
//! functions of the file are ported in [`crate::materials`] on a structured state; these two
//! work on the original's records.
//!
//! Callees in other files are hooks: the element dictionary's `SI_elemdictGetFirstRelatedElem`
//! and `SI_elemdictGetNextRelatedElem` (ported in `si-dict`, not a dependency of this crate),
//! `decayTable` and `si_initJitter` (ported in [`crate::jitter`] / [`crate::jitter_image`] on
//! their own record forms, not on the image). Class: model (the register arithmetic).

use si_mem::Mem;
use si_x87::{x, F80};

si_mem::outside_calls! {
    /// What the two functions call.
    pub trait MaterialsCalls {
        /// `SI_elemdictGetFirstRelatedElem (0x08138094)`: `(dict, elem, forward = 1, group = 3,
        /// relation, chapter, element out = 0, cursor)`; answers the related object or null.
        fn get_first_related(dict: u32, elem: u32, forward: i32, group: i32, relation: i32, chapter: u32, out: u32, cursor: u32) -> u32 = "SI_elemdictGetFirstRelatedElem", 0x08138094;
        /// `SI_elemdictGetNextRelatedElem (0x08138104)`: the same arguments.
        fn get_next_related(dict: u32, elem: u32, forward: i32, group: i32, relation: i32, chapter: u32, out: u32, cursor: u32) -> u32 = "SI_elemdictGetNextRelatedElem", 0x08138104;
        /// `decayTable (0x082595e0)`: fills the table at the address.
        fn decay_table(table: u32) = "decayTable", 0x082595e0;
        /// `si_initJitter (0x082596b4)`: `(count, jitter record)`.
        fn init_jitter(count: i32, jitter: u32) = "si_initJitter", 0x082596b4;
    }
}

/// The ambient colour (three floats, 0x08790ddc).
const AMBIENT: u32 = 0x0879_0ddc;
/// Switches of the render set-up (16 bits each).
const W_8791166: u32 = 0x0879_1166;
const W_879116A: u32 = 0x0879_116a;
const W_879116C: u32 = 0x0879_116c;
const W_8790DEC: u32 = 0x0879_0dec;
/// A byte whose bit 0 is set when any map reflects or transmits through a map.
const BYTE_8790C7C: u32 = 0x0879_0c7c;
/// `SI_cid.MODELS`, `.MATERIALS`, `.TEXTURES2D`, `.TEXTURES3D`.
const CID_MODELS: u32 = 0x0878_1680;
const CID_MATERIALS: u32 = 0x0878_167c;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_TEXTURES3D: u32 = 0x0878_16cc;

/// (ambient r + g + b) / 3 rounded to a float (the double 3.0 at 0x08654218 / 0x086541f0).
fn ambient_mean(m: &Mem) -> f32 {
    ((x(m.f32(AMBIENT)) + m.f32(AMBIENT + 4) + m.f32(AMBIENT + 8)) / 3.0f64).to_f32()
}

/// A float copied through the register: a signalling NaN becomes quiet, nothing else changes.
fn copy_f32(m: &mut Mem, from: u32, to: u32) {
    let v = x(m.f32(from)).to_f32();
    m.set_f32(to, v);
}

/// Scales the colours of a 2D map: +0x148 = (0.003922 × +0x128) × mean, then +0x14c..+0x164 =
/// 0.003922 × +0x12c.. (each product rounded in the register, then to a float).
fn scale_map(m: &mut Mem, p: u32, c: F80, mean: f32) {
    m.set_f32(p + 0x148, (c * m.f32(p + 0x128) * mean).to_f32());
    for k in 0..7 {
        let v = (c * m.f32(p + 0x12c + 4 * k)).to_f32();
        m.set_f32(p + 0x14c + 4 * k, v);
    }
}

/// A 3D texture: +0x128 = mean × +0x108, +0x12c..+0x140 = +0x10c..+0x120.
fn scale_3d(m: &mut Mem, t: u32, mean: f32) {
    let v = (x(mean) * m.f32(t + 0x108)).to_f32();
    m.set_f32(t + 0x128, v);
    for k in 0..6 {
        copy_f32(m, t + 0x10c + 4 * k, t + 0x12c + 4 * k);
    }
}

/// What the maps of one owner say (the locals the original keeps in 16-bit words).
#[derive(Default)]
struct Needs {
    reflect: bool,
    transp: bool,
    refract: bool,
}

/// Sets the texture flags of a model [soft 0x0825b5a4 setTextureFlag]: `(dict, model, flags)`;
/// the 16-bit `*flags` is cleared, then filled as the 2D maps of relation 0x190 say; the byte at
/// 0x08790c7c gets bit 0 when the flags carry 0x220.
pub fn set_texture_flag(m: &mut Mem, o: &mut dyn si_mem::Outside, dict: u32, model: u32, flags: u32) {
    use MaterialsCalls as _;
    let cursor = m.local(4);
    m.set_u16(flags, 0);
    let mean = ambient_mean(m);
    let c = x(0.003922f64);
    let mut n = Needs::default();
    let mut shiny = 0u32;
    let b0 = |m: &mut Mem, v: u8| {
        let a = m.u8(flags);
        m.set_u8(flags, a | v);
    };
    let b1 = |m: &mut Mem, v: u8| {
        let a = m.u8(flags + 1);
        m.set_u8(flags + 1, a | v);
    };
    let tex2d = m.u32(CID_TEXTURES2D);
    let mut p = o.get_first_related(m, dict, model, 1, 3, 0x190, tex2d, 0, cursor);
    while p != 0 {
        scale_map(m, p, c, mean);
        if m.u16(p + 0x114) != 0 {
            b1(m, 4);
        }
        if m.u16(p + 0x118) != 0 {
            n.reflect = true;
        }
        if m.u16(p + 0x11a) != 0 {
            n.transp = true;
            b1(m, 4);
        }
        if m.u16(p + 0x11e) != 0 {
            n.refract = true;
        }
        if m.u16(W_8791166) != 0 && m.u16(W_879116A) != 0 && m.u16(p + 0x124) != 0 {
            shiny = p;
        } else {
            b0(m, 4);
        }
        let tex2d = m.u32(CID_TEXTURES2D);
        p = o.get_next_related(m, dict, model, 1, 3, 0x190, tex2d, 0, cursor);
    }
    if n.reflect {
        if m.u16(W_8791166) != 0 && m.u16(W_879116A) != 0 && shiny != 0 && m.u16(shiny + 0x2a) == 8 {
            b0(m, 0x10);
        } else {
            b0(m, 0x20);
        }
    }
    if n.transp {
        if n.refract {
            b1(m, 2);
        } else {
            b1(m, 1);
        }
    }
    if m.u16(flags) & 0x220 != 0 {
        let a = m.u8(BYTE_8790C7C);
        m.set_u8(BYTE_8790C7C, a | 1);
    }
    if m.u16(W_8790DEC) != 0 {
        let w = m.u16(flags);
        if w & 0x100 == 0 {
            m.set_u16(flags, w | 0x4000);
        }
    }
    m.release(4);
}

/// Prepares every material and model texture of a scene [soft 0x0825a874 SI_processMaterials]:
/// `(dict)`.
///
/// For each material element (the chapter `SI_cid.MATERIALS` of the dictionary at +4: 64-byte
/// chapter records, count at +4, 0x84-byte elements at +8; an element with +0x48 set or no object
/// is passed over) the need flags are worked out and the derived fields written; then the model
/// chapter's elements get `setTextureFlag` and their first 3D texture scaled. Only the first 3D
/// texture of an owner is looked at; all 2D maps are.
pub fn process_materials(m: &mut Mem, o: &mut dyn si_mem::Outside, dict: u32) {
    use MaterialsCalls as _;
    let mean = ambient_mean(m);
    let chapters = m.u32(dict + 4);
    let models = chapters.wrapping_add(m.u32(CID_MODELS) << 6);
    let mats = chapters.wrapping_add(m.u32(CID_MATERIALS) << 6);
    let n_mats = m.i32(mats + 4);
    let cursor = m.local(4);
    let mut i = 0i32;
    while i < n_mats {
        let elem = m.u32(mats + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        i += 1;
        if m.u16(elem + 0x48) != 0 {
            continue;
        }
        let mat = m.u32(elem + 4);
        if mat == 0 {
            continue;
        }
        let mut n = Needs::default();
        let mut edi: u32 = 0;
        if m.u16(W_879116A) != 0 {
            n.reflect = x(m.f32(mat + 0x6a8)).gt(x(0.01f64));
        }
        n.transp = x(m.f32(mat + 0x6b8)).gt(x(0.01f64)) || m.u16(mat + 0xdca) != 1;
        if m.u16(W_879116C) != 0 {
            n.refract = !x(m.f32(mat + 0x6dc)).eq(x(1.0f32));
        }
        m.set_u32(mat + 0x6f4, 0);
        let g = [m.f32(AMBIENT), m.f32(AMBIENT + 4), m.f32(AMBIENT + 8)];
        m.set_f32(mat + 0x700, (x(m.f32(mat + 0x34)) * g[0]).to_f32());
        m.set_f32(mat + 0x704, (x(m.f32(mat + 0x38)) * g[1]).to_f32());
        let third = x(m.f32(mat + 0x3c)) * g[2];
        let w = m.u32(mat + 0x700);
        m.set_u32(mat + 0x710, w);
        let w = m.u32(mat + 0x704);
        m.set_u32(mat + 0x714, w);
        m.set_f32(mat + 0x708, third.to_f32());
        let w = m.u32(mat + 0x708);
        m.set_u32(mat + 0x718, w);
        let w = m.u32(mat + 0x70c);
        m.set_u32(mat + 0x71c, w);
        for k in 0..8 {
            let w = m.u32(mat + 0x44 + 4 * k);
            m.set_u32(mat + 0x720 + 4 * k, w);
        }
        let refl = if m.u16(W_879116A) != 0 { x(m.f32(mat + 0x6a8)) } else { F80::from_i32(0) };
        m.set_u32(mat + 0x74c, 0x3f80_0000);
        m.set_f32(mat + 0x740, refl.to_f32());
        m.set_f32(mat + 0x744, (x(1.0f32) - m.f32(mat + 0x6b8)).to_f32());
        let ior = if m.u16(W_879116C) != 0 { x(m.f32(mat + 0x6dc)) } else { x(1.0f32) };
        m.set_f32(mat + 0x748, ior.to_f32());
        if m.u16(mat + 0x1e).wrapping_sub(4) <= 1 {
            o.decay_table(m, mat + 0x64);
        }
        if m.u16(mat + 0x758) != 0 {
            o.decay_table(m, mat + 0x760);
        }
        if x(0.1f64).lt(x(m.f32(mat + 0x6ac))) {
            o.init_jitter(m, 0xc9, mat + 0x6ac);
        }
        if x(0.1f64).lt(x(m.f32(mat + 0x6bc))) {
            o.init_jitter(m, 0xdf, mat + 0x6bc);
        }
        // the first 3D texture
        let tex3d = m.u32(CID_TEXTURES3D);
        let t = o.get_first_related(m, dict, elem, 1, 3, 0x1f5, tex3d, 0, 0);
        if t != 0 {
            scale_3d(m, t, mean);
            if m.u16(t + 0xf6) != 0 {
                edi |= 0x400;
            }
            if m.u16(t + 0xfa) != 0 {
                n.reflect = true;
            }
            if m.u16(t + 0xfc) != 0 {
                n.transp = true;
                edi |= 0x400;
            }
            if m.u16(t + 0x100) != 0 {
                n.refract = true;
            }
            edi |= 4;
        }
        // the 2D maps
        let c = x(0.003922f64);
        let tex2d = m.u32(CID_TEXTURES2D);
        let mut p = o.get_first_related(m, dict, elem, 1, 3, 0x191, tex2d, 0, cursor);
        while p != 0 {
            scale_map(m, p, c, mean);
            if m.u16(p + 0x114) != 0 {
                edi |= 0x400;
            }
            if m.u16(p + 0x118) != 0 {
                n.reflect = true;
            }
            if m.u16(p + 0x11a) != 0 {
                n.transp = true;
                edi |= 0x400;
            }
            if m.u16(p + 0x11e) != 0 {
                n.refract = true;
            }
            if m.u16(W_8791166) != 0 && m.u16(W_879116A) != 0 && m.u16(p + 0x124) != 0 {
                m.set_u32(mat + 0x6f4, p);
            } else {
                edi |= 4;
            }
            let tex2d = m.u32(CID_TEXTURES2D);
            p = o.get_next_related(m, dict, elem, 1, 3, 0x191, tex2d, 0, cursor);
        }
        if n.reflect {
            let sh = m.u32(mat + 0x6f4);
            if m.u16(W_8791166) != 0 && m.u16(W_879116A) != 0 && sh != 0 && m.u16(sh + 0x2a) == 8 {
                edi |= 0x10;
            } else {
                edi |= 0x20;
            }
        }
        if n.transp {
            edi |= if n.refract { 0x200 } else { 0x100 };
        }
        if m.u16(mat + 0x758) != 0 {
            edi |= 0x40;
        }
        if edi & 0x220 != 0 {
            let a = m.u8(BYTE_8790C7C);
            m.set_u8(BYTE_8790C7C, a | 1);
        }
        if m.u16(mat + 0x20) != 1 {
            edi |= 2;
        }
        if m.u16(W_8790DEC) != 0 && edi & 0x100 == 0 {
            edi |= 0x4000;
        }
        m.set_u32(mat + 0x6e0, edi as u16 as i16 as i32 as u32);
    }
    // the models
    let n_models = m.i32(models + 4);
    let flags = m.local(2);
    let mut i = 0i32;
    while i < n_models {
        let elem = m.u32(models + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        i += 1;
        m.set_u16(flags, 0);
        if m.u16(elem + 0x48) != 0 {
            continue;
        }
        let tex2d = m.u32(CID_TEXTURES2D);
        if o.get_first_related(m, dict, elem, 1, 3, 0x190, tex2d, 0, 0) != 0 {
            set_texture_flag(m, o, dict, elem, flags);
        }
        let tex3d = m.u32(CID_TEXTURES3D);
        let t = o.get_first_related(m, dict, elem, 1, 3, 0x1f4, tex3d, 0, 0);
        if t != 0 {
            let mut n = Needs::default();
            scale_3d(m, t, mean);
            let or0 = |m: &mut Mem, v: u8| {
                let a = m.u8(flags);
                m.set_u8(flags, a | v);
            };
            let or1 = |m: &mut Mem, v: u8| {
                let a = m.u8(flags + 1);
                m.set_u8(flags + 1, a | v);
            };
            if m.u16(t + 0xf6) != 0 {
                or1(m, 4);
            }
            if m.u16(t + 0xfa) != 0 {
                n.reflect = true;
            }
            if m.u16(t + 0xfc) != 0 {
                n.transp = true;
                or1(m, 4);
            }
            if m.u16(t + 0x100) != 0 {
                n.refract = true;
            }
            if n.reflect {
                or0(m, 0x20);
            }
            if n.transp {
                or1(m, if n.refract { 2 } else { 1 });
            }
        }
        let w = m.u16(flags) | 4;
        m.set_u16(flags, w);
        if w & 0x220 != 0 {
            let a = m.u8(BYTE_8790C7C);
            m.set_u8(BYTE_8790C7C, a | 1);
        }
    }
    m.release(2);
    m.release(4);
}
