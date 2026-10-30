// SPDX-License-Identifier: MIT
//! Checks of the sixth pass (finding 0012, sections 13 and 14) on the vectors of
//! `re/tools/si-render/emu/gen_tex.py` and `gen_mat.py`: materials and textures before a
//! frame, the 2D look-up and blend, the 3D textures, the two projection routines.
//!
//! A record of the original is read here by offset from the bytes the vector holds
//! ([`Image`]); what the port leaves is written back into those bytes by offset and the
//! whole record compared with the original's, so a word the original changed and the port
//! does not know shows as a difference.
use crate::outer::Loaded;
use crate::prep::mutate;
use crate::verdict;
use si_render::materials::GlbMap;
use si_render::renutil::blookup;
use si_render::shade::{ShadeMap, ShadeMaterial, ShadeState};
use si_render::drand48::{drand48, erand48, makeseed, srand48, Seed};
use si_render::polygon::txt_projection_info;
use si_render::softtriangle::model_compute_rot_bounding;
use si_render::t2d_fct::{map_tcoord_compute, upd_txt2d_effects};
use si_render::t2d_ren::render_txt2d;
use si_render::update::update_txt3d_transf;
use si_render::t3d_ren::{render_txt3d, upd_txt3d_turb, ShadeTex3d};
use si_render::tt::{apply_txt3d, lookup_color, make_cloud, make_marble, make_wood, set_color_map, set_tform, TrigLib, Txt3d};
use si_render::tt_noise::{get_noise, init_noise, Noise};
use si_render::types::Vector;
use si_x87::vectors::{Case, Check, Field, Verdict, Word};
use si_x87::F80;

/// A buffer of a case as bytes in memory order, with the pointers it holds.
#[derive(Clone, Debug, Default)]
pub struct Image {
    pub bytes: Vec<u8>,
    /// Byte offset of a pointer word → the buffer it points into and the offset there.
    pub refs: Vec<(usize, String, u32)>,
    /// Bytes of words the original never wrote (an output's dashes).
    pub unwritten: Vec<bool>,
}

impl Image {
    pub fn of(f: &Field) -> Image {
        let mut im = Image::default();
        for w in f.words() {
            let at = im.bytes.len();
            let n = w.bytes() as usize;
            match w {
                Word::Value { value, .. } => im.bytes.extend_from_slice(&value.to_le_bytes()[..n]),
                Word::Unwritten { .. } => im.bytes.extend(std::iter::repeat(0).take(n)),
                Word::Ref { name, offset } => {
                    im.refs.push((at, name.clone(), *offset));
                    im.bytes.extend_from_slice(&[0; 4]);
                }
            }
            im.unwritten.extend(std::iter::repeat(matches!(w, Word::Unwritten { .. })).take(n));
        }
        im
    }
    pub fn i16(&self, at: usize) -> i16 {
        i16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])
    }
    pub fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.bytes[at], self.bytes[at + 1], self.bytes[at + 2], self.bytes[at + 3]])
    }
    pub fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }
    pub fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    pub fn f32s<const N: usize>(&self, at: usize) -> [f32; N] {
        core::array::from_fn(|i| self.f32(at + 4 * i))
    }
    /// The pointer at `at`: the buffer's name and the offset into it.
    pub fn ptr(&self, at: usize) -> Option<(&str, u32)> {
        self.refs.iter().find(|r| r.0 == at).map(|r| (r.1.as_str(), r.2))
    }
    /// Makes the word at `at` a pointer into a buffer of the case, or null.
    pub fn set_ptr(&mut self, at: usize, to: Option<(&str, u32)>) {
        self.refs.retain(|r| r.0 != at);
        self.put_u32(at, 0);
        if let Some((name, offset)) = to {
            self.refs.push((at, name.to_string(), offset));
        }
    }
    pub fn put_i16(&mut self, at: usize, v: i16) {
        self.bytes[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put_u32(&mut self, at: usize, v: u32) {
        self.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put_f32(&mut self, at: usize, v: f32) {
        self.put_u32(at, v.to_bits());
    }
    pub fn put_f32s(&mut self, at: usize, v: &[f32]) {
        for (i, x) in v.iter().enumerate() {
            self.put_f32(at + 4 * i, *x);
        }
    }
}

/// How the pointer words of a record are compared: given the offset, the original's pointer
/// and the port's (each a buffer name and an offset, or none); `None`: not compared.
pub type Ptrs<'a> = &'a dyn Fn(usize, Option<(&str, u32)>, Option<(&str, u32)>) -> Option<bool>;

/// Pointers the routine under test leaves alone: the same before and after.
pub fn kept(_: usize, original: Option<(&str, u32)>, port: Option<(&str, u32)>) -> Option<bool> {
    Some(original == port)
}

/// Compares what the port left of a record (`port`: the input's bytes with the port's
/// values written over them) with the original's, word by word of the original's layout.
pub fn same_image(what: &str, original: &Field, port: &Image, ptrs: Ptrs) -> Result<(), String> {
    let mut at = 0usize;
    for w in original.words() {
        let n = w.bytes() as usize;
        match w {
            Word::Value { value, .. } => {
                let mut got = [0u8; 16];
                got[..n].copy_from_slice(&port.bytes[at..at + n]);
                let mut got = u128::from_le_bytes(got);
                if mutate() {
                    got ^= 1;
                }
                if port.ptr(at).is_some() {
                    if ptrs(at, None, port.ptr(at)) == Some(false) {
                        return Err(format!("{what} +{at:#x}: original {value:#x}, port a pointer {:?}", port.ptr(at)));
                    }
                } else if got != *value {
                    return Err(format!("{what} +{at:#x}: original {value:0w$x}, port {got:0w$x}", w = 2 * n));
                }
            }
            Word::Unwritten { .. } => {
                if mutate() {
                    return Err(format!("{what} +{at:#x}: unwritten"));
                }
            }
            Word::Ref { name, offset } => {
                if ptrs(at, Some((name.as_str(), *offset)), port.ptr(at)) == Some(false) {
                    return Err(format!("{what} +{at:#x}: original &{name}+{offset:#x}, port {:?}", port.ptr(at)));
                }
            }
        }
        at += n;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------- records

/// `__SI_LclMap`: the case's buffer `name`, read from the case loaded into an image
/// (`si_render::inimage::map_at`; the pointers are the caller's).
pub fn map_of(l: &Loaded, name: &str) -> ShadeMap {
    si_render::inimage::map_at(&l.m, l.at(name))
}

/// The members of a 2D map the set-up routines write, back into its bytes.
pub fn put_map(m: &ShadeMap, im: &mut Image) {
    im.put_f32s(0x38, &[m.umin, m.vmin, m.umax, m.vmax, m.u2min, m.v2min, m.u2max, m.v2max]);
    im.put_f32s(0x60, &[m.pix_u, m.pix_v]);
    im.put_f32s(0x70, &[m.uoff_frac, m.voff_frac]);
    im.put_i16(0x78, m.neg_u);
    im.put_i16(0x7a, m.neg_v);
    im.put_f32s(0x7c, &m.v7c);
    im.put_f32s(0x88, &m.centre);
    for r in 0..4 {
        im.put_f32s(0xb8 + 16 * r, &m.tmat[r]);
    }
    im.put_i16(0x106, m.wrap_u);
    im.put_i16(0x108, m.wrap_v);
    for (k, v) in [m.fx_ambient, m.fx_diffuse, m.fx_specular, m.fx_reflect, m.fx_transp].into_iter().enumerate() {
        im.put_i16(0x112 + 2 * k, v);
    }
    im.put_i16(0x120, m.bump);
    im.put_i16(0x124, m.skip);
    im.put_f32s(0x148, &m.work);
    im.put_f32(0x160, m.bump_amount);
    im.put_f32(0x164, m.env_scale);
    im.put_f32s(0x1a4, &m.box_min);
    im.put_f32s(0x1b0, &m.box_max);
    im.put_u32(0x1bc, m.box_node as u32);
    im.put_f32(0x1c0, m.box_r5);
    im.put_f32(0x1c4, m.box_r6);
}

/// `__SI_Material`: the case's buffer `name`, read from the loaded case
/// (`si_render::inimage::material_at`; the lists are the caller's).
pub fn material_of(l: &Loaded, name: &str) -> ShadeMaterial {
    si_render::inimage::material_at(&l.m, l.at(name))
}

/// A material's working block, back into its bytes.
pub fn put_material(m: &ShadeMaterial, im: &mut Image) {
    let w = &m.work;
    im.put_f32s(0x700, &w.base_ambient);
    im.put_f32s(0x710, &w.ambient);
    im.put_f32s(0x720, &w.diffuse);
    im.put_f32s(0x730, &w.specular);
    im.put_f32s(0x740, &[w.reflectivity, w.opacity, w.refraction, w.existence]);
}

/// What `SI_materials` writes of a material besides the working block: the flag word, the
/// map count, the two tables.
pub fn put_material_setup(m: &ShadeMaterial, im: &mut Image) {
    put_material(m, im);
    for (i, p) in m.spec_table.pairs.iter().enumerate() {
        im.put_f32s(0x68 + 8 * i, p);
    }
    im.put_u32(0x6e0, m.flags as i16 as i32 as u32);
    im.put_i16(0x6e8, m.n_maps);
    for (i, p) in m.blur_table.pairs.iter().enumerate() {
        im.put_f32s(0x764 + 8 * i, p);
    }
}

/// A `glb_map` read from the loaded case (`si_render::inimage::glb_at`); the pixel block
/// runs to the end of the buffer the pointer at +0x1c points into.
pub fn glb_of(l: &Loaded, c: &Case, name: &str) -> GlbMap {
    let a = l.at(name);
    let p = l.m.u32(a + 0x1c);
    let bytes = l.name_of(p).map(|(buffer, offset)| {
        let start = offset as i64 - (l.m.i16(a + 0x16) as i64 + 4) * 5;
        (c.input(&buffer).to_bytes().len() as i64 - start).max(0) as u32
    });
    si_render::inimage::glb_at(&l.m, a, bytes)
}

// ------------------------------------------------------------------------------- the checks

/// `si_BlookUp`: the three samples, each word the original wrote.
fn check_blookup(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    let l = Loaded::of(c);
    let mut m = map_of(&l, "map");
    m.glb = Some(0);
    st.maps.push(m);
    st.pictures.push(glb_of(&l, c, "glb"));
    let got = blookup(&st, c.input("u").f32(), c.input("v").f32(), 0, c.input("interp").i16(), c.input("bump").i16() != 0);
    verdict((|| {
        for (k, name) in ["s", "su", "sv"].iter().enumerate() {
            let want = c.output(name).written();
            for i in 0..5 {
                let g = got[k][i].to_bits() ^ u32::from(mutate());
                match want[i] {
                    Some(v) if v as u32 != g => return Err(format!("{name}[{i}]: original {:08x}, port {g:08x}", v as u32)),
                    // the neighbours' colours are not written; the port has zeros there
                    None if k == 0 || mutate() => return Err(format!("{name}[{i}]: the original did not write it")),
                    _ => {}
                }
            }
        }
        Ok(())
    })())
}

/// `SI_renderTxt2d`: the whole material record afterwards.
fn check_render_txt2d(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    let before = Image::of(c.input("mat"));
    let l = Loaded::of(c);
    st.materials.push(material_of(&l, "mat"));
    st.maps.push(map_of(&l, "map"));
    let s = c.input("s").f32s();
    render_txt2d(&mut st, 0, 0, [s[0], s[1], s[2], s[3], s[4]]);
    let mut port = before;
    put_material(&st.materials[0], &mut port);
    verdict(same_image("material", c.after("mat"), &port, &kept))
}

// ------------------------------------------------------------------------------ 3D textures

fn seed_of(f: &Field) -> [u16; 3] {
    let w = f.u16s();
    [w[0], w[1], w[2]]
}

fn same_seed(c: &Case, name: &str, got: [u16; 3]) -> Result<(), String> {
    let want = c.after(name).written();
    for k in 0..3 {
        let g = got[k] ^ u16::from(mutate());
        if want[k].map(|v| v as u16) != Some(g) {
            return Err(format!("{name}[{k}]: original {:?}, port {g:04x}", want[k]));
        }
    }
    Ok(())
}

fn same_st0(c: &Case, got: F80) -> Result<(), String> {
    let got = if mutate() { got + F80::ONE } else { got };
    if c.output("st0").same_f80(got) {
        Ok(())
    } else {
        Err(format!("st0: original {:?}, port {got:?}", c.output("st0").words()))
    }
}

/// The first 0x98 bytes of an `SI_Txt3d` as the port has them, over `base` (the bytes of
/// the fields the port does not model).
fn put_txt_head(t: &Txt3d, im: &mut Image) {
    im.put_u32(0, t.kind as u32);
    im.put_i16(4, t.has_tform);
    for r in 0..4 {
        im.put_f32s(8 + 16 * r, &t.tform[r]);
    }
    im.put_f32s(0x48, &[t.scale, t.taper, t.strength]);
    im.put_u32(0x54, t.terms as u32);
    for (i, p) in t.term.iter().enumerate() {
        im.put_f32s(0x58 + 8 * i, p);
    }
}

/// The head of a texture a maker returned: every word the original wrote (the fourth
/// column of the matrix and, for a cloud, the three sine parameters are what the allocator
/// left; the terms past the count likewise).
fn same_made(c: &Case, t: &Txt3d) -> Result<(), String> {
    let head = c.output("head");
    let mut port = Image::of(head);
    // the port's zeros stand where the original wrote nothing: compare written words only
    put_txt_head(t, &mut port);
    let mut at = 0usize;
    for w in head.words() {
        let n = w.bytes() as usize;
        if let Word::Value { value, .. } = w {
            let mut got = [0u8; 16];
            got[..n].copy_from_slice(&port.bytes[at..at + n]);
            let got = u128::from_le_bytes(got) ^ u128::from(mutate());
            // the allocator's mark in the record's first byte is overwritten by the kind
            if got != *value {
                return Err(format!("texture +{at:#x}: original {value:0w$x}, port {got:0w$x}", w = 2 * n));
            }
        }
        at += n;
    }
    Ok(())
}

fn check_make(c: &Case, kind: i32) -> Verdict {
    let mut noise = Noise::default();
    let f = |n: &str| c.input(n).f32();
    let t = match kind {
        0 => make_marble(&mut noise, f("spacing"), f("taper"), f("strength"), c.input("terms").i32(), f("power"), &mut TrigLib),
        1 => make_wood(&mut noise, f("spacing"), f("taper"), f("strength"), f("power"), &mut TrigLib),
        _ => make_cloud(&mut noise, c.input("terms").i32(), f("power"), &mut TrigLib),
    };
    verdict(same_made(c, &t))
}

fn check_set_tform(c: &Case) -> Verdict {
    let before = Image::of(c.input("t"));
    let mut t = Txt3d { tform: core::array::from_fn(|r| before.f32s(8 + 16 * r)), ..Txt3d::default() };
    let m = c.input("m").f32s();
    set_tform(&mut t, &core::array::from_fn(|r| core::array::from_fn(|k| m[4 * r + k])));
    let mut port = before;
    port.put_i16(4, t.has_tform);
    for r in 0..4 {
        port.put_f32s(8 + 16 * r, &t.tform[r]);
    }
    verdict(same_image("texture", c.after("t"), &port, &kept))
}

fn rgba5(f: &[f32]) -> [[f32; 4]; 5] {
    core::array::from_fn(|i| core::array::from_fn(|k| f[4 * i + k]))
}

/// `SI_ttSetColorMap`, then `lookupColor` on the ramp it left.
fn check_set_color_map(c: &Case) -> Verdict {
    let mut t = Txt3d::default();
    let f = c.input("f").f32s();
    let r = set_color_map(&mut t, &rgba5(&c.input("c").f32s()), [f[0], f[1], f[2]]);
    verdict((|| {
        r.map_err(|e| format!("the port refuses: {e}"))?;
        let stops: Vec<u32> = t.stops.iter().map(|v| *v as u32).collect();
        if !c.output("stops").same_u32(&stops) {
            return Err(format!("stops: original {:?}, port {stops:?}", c.output("stops").words()));
        }
        let ramp = c.output("ramp").words();
        for e in ramp.chunks(5) {
            let i = e[0].value().unwrap() as usize;
            for k in 0..4 {
                let g = t.ramp[i][k].to_bits() ^ u32::from(mutate());
                match e[1 + k].value() {
                    Some(v) if v as u32 != g => return Err(format!("ramp[{i}][{k}]: original {:08x}, port {g:08x}", v as u32)),
                    // an entry the original did not write (stops out of order) has no value
                    _ => {}
                }
            }
        }
        // the look-ups read whatever the ramp holds; where the original left entries
        // unwritten the port's zeros would differ, so only ramps written whole are looked up
        if ramp.iter().any(|w| w.value().is_none()) {
            return Ok(());
        }
        for e in c.output("looks").words().chunks(5) {
            let v = f32::from_bits(e[0].value().unwrap() as u32);
            let got = lookup_color(&t.ramp, v);
            for k in 0..4 {
                let g = got[k].to_bits() ^ u32::from(mutate());
                if e[1 + k].value().map(|x| x as u32) != Some(g) {
                    return Err(format!("lookupColor({v}) [{k}]: original {:?}, port {g:08x}", e[1 + k]));
                }
            }
        }
        Ok(())
    })())
}

/// `__SI_Texture3D`: the case's buffer `name`, read from the loaded case
/// (`si_render::inimage::tex3d_at`), without the texture made ready (+0x8c): the checks
/// make it or say what it holds.
pub fn tex3d_of(l: &Loaded, name: &str) -> ShadeTex3d {
    let mut t = si_render::inimage::tex3d_at(&l.m, l.at(name));
    t.txt = None;
    t
}

/// What the set-up routines write of a 3D texture, back into its bytes.
pub fn put_tex3d(t: &ShadeTex3d, im: &mut Image) {
    for r in 0..4 {
        im.put_f32s(0xb4 + 16 * r, &t.matrix[r]);
    }
    for (off, v) in [(0xf4, t.fx_ambient), (0xf6, t.fx_diffuse), (0xf8, t.fx_specular), (0xfa, t.fx_reflect), (0xfc, t.fx_transp), (0x102, t.fx_rough)] {
        im.put_i16(off, v);
    }
    im.put_f32s(0x128, &t.work);
}

/// `SI_updTxt3dTurb` on a made-up record, then on the state it left: six points through
/// `SI_ttApplyTxt3d` and five through `SI_renderTxt3d` on one material.
fn check_upd_txt3d(c: &Case) -> Verdict {
    let before = Image::of(c.input("tex"));
    let l = Loaded::of(c);
    let mut tex = tex3d_of(&l, "tex");
    let mut st = ShadeState::default();
    si_render::tt_noise::init_noise(&mut st.noise);
    // the fourth column of the rotation's matrix is the identity's; the port's matrix has it
    let made = upd_txt3d_turb(&mut tex, &mut st.noise, &mut TrigLib);
    verdict((|| {
        made.map_err(|e| format!("the port refuses: {e}"))?;
        let mut port = before.clone();
        put_tex3d(&tex, &mut port);
        // the pointer to the made texture is new
        same_image("texture record", c.after("tex"), &port, &|at, o, _| if at == 0x8c { Some(o.is_some()) } else { None })?;
        let txt = tex.txt.clone().unwrap();
        same_made(c, &txt)?;
        let stops: Vec<u32> = txt.stops.iter().map(|v| *v as u32).collect();
        if !c.output("istops").same_u32(&stops) {
            return Err(format!("stops: original {:?}, port {stops:?}", c.output("istops").words()));
        }
        // the second form, on the image (`t3d_fct_image`): the case loaded, the routine run
        // there, the record and the block it made read back from the image. It runs on a
        // thread of its own with the case's tape of library calls from the start (the tape
        // of this thread is the value form's); the calls of the frame's look-ups after it
        // are left on that tape.
        let txt_value = txt.clone();
        std::thread::scope(|scope| scope.spawn(|| -> Result<(), String> {
            let txt = txt_value;
            si_x87::trig::replay::start(c.tape());
            let mut l2 = Loaded::of(c);
            let at = l2.at("tex");
            si_render::t3d_fct_image::upd_txt3d_turb(&mut l2.m, at, &mut TrigLib);
            if let Some(f) = l2.m.fault() {
                return Err(format!("on the image: a fault: {f:?}"));
            }
            let mut port = before.clone();
            let mem = l2.m.bytes(at, port.bytes.len() as u32).to_vec();
            for (i, b) in mem.iter().enumerate() {
                if !port.refs.iter().any(|r| i >= r.0 && i < r.0 + 4) {
                    port.bytes[i] = *b;
                }
            }
            same_image("texture record on the image", c.after("tex"), &port, &|at, o, _| if at == 0x8c { Some(o.is_some()) } else { None })?;
            let made = l2.m.u32(at + 0x8c);
            if made == 0 || l2.m.block_size(made) != Some(0x40a4) {
                return Err("on the image: +0x8c is not a block of 0x40a4 bytes".into());
            }
            let txt2 = si_render::inimage::txt3d_at(&l2.m, made);
            same_made(c, &txt2).map_err(|e| format!("on the image: {e}"))?;
            let stops2: Vec<u32> = txt2.stops.iter().map(|v| *v as u32).collect();
            if !c.output("istops").same_u32(&stops2) {
                return Err(format!("on the image: stops: original {:?}, port {stops2:?}", c.output("istops").words()));
            }
            // the ramp: every entry the value port holds (the original's written ones)
            if txt2.ramp != txt.ramp {
                let k = txt2.ramp.iter().zip(&txt.ramp).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits)).unwrap_or(0);
                if txt2.ramp[k].map(f32::to_bits) != txt.ramp[k].map(f32::to_bits) {
                    return Err(format!("on the image: ramp entry {k}: {:?} against the value port's {:?}", txt2.ramp[k], txt.ramp[k]));
                }
            }
            match si_x87::trig::replay::finish() {
                Err(e) if e.starts_with("call ") => Err(format!("on the image: the library calls: {e}")),
                _ => Ok(()),
            }
        }).join().unwrap_or_else(|_| Err("on the image: the port panicked".into())))?;
        let word = |w: &Word| w.value().map(|v| v as u32);
        for e in c.output("applied").words().chunks(7) {
            let p: [f32; 3] = core::array::from_fn(|k| f32::from_bits(word(&e[k]).unwrap()));
            let got = apply_txt3d(&txt, &st.noise, p, &mut TrigLib).ok_or("the port has no colour")?;
            for k in 0..4 {
                let g = got[k].to_bits() ^ u32::from(mutate());
                if word(&e[3 + k]) != Some(g) {
                    return Err(format!("SI_ttApplyTxt3d({p:?}) [{k}]: original {:?}, port {g:08x}", e[3 + k]));
                }
            }
        }
        let mat = Image::of(c.input("mat"));
        let mut m = material_of(&l, "mat");
        m.tex3d = 1;
        st.materials.push(m);
        st.tex3ds.push(tex);
        let n = c.input("nrm").f32s();
        let mut normal = Vector::new(n[0], n[1], n[2]);
        for (call, e) in c.output("rendered").words().chunks(27).enumerate() {
            let p: [f32; 3] = core::array::from_fn(|k| f32::from_bits(word(&e[k]).unwrap()));
            if call == 3 {
                normal = Vector::new(0.0, 0.0, 0.0);
            }
            let with_normal = word(&e[3]) == Some(1);
            render_txt3d(&mut st, 0, Vector::new(p[0], p[1], p[2]), if with_normal { Some(&mut normal) } else { None }, &mut TrigLib);
            let mut img = mat.clone();
            put_material(&st.materials[0], &mut img);
            for k in 0..20 {
                let g = img.u32(0x700 + 4 * k) ^ u32::from(mutate());
                if word(&e[4 + k]) != Some(g) {
                    return Err(format!("SI_renderTxt3d call {call}: working block +{:#x}: original {:?}, port {g:08x}", 4 * k, e[4 + k]));
                }
            }
            for (k, g) in [normal.x, normal.y, normal.z].into_iter().enumerate() {
                let g = g.to_bits() ^ u32::from(mutate());
                if word(&e[24 + k]) != Some(g) {
                    return Err(format!("SI_renderTxt3d call {call}: normal[{k}]: original {:?}, port {g:08x}", e[24 + k]));
                }
            }
        }
        Ok(())
    })())
}

// ------------------------------------------------------------------------- the projections

fn vec3_of(f: &Field) -> Vector {
    let v = f.f32s();
    Vector::new(v[0], v[1], v[2])
}

fn same_f32s(what: &str, original: &Field, port: &[f32]) -> Result<(), String> {
    let want = original.written();
    for (i, g) in port.iter().enumerate() {
        let g = g.to_bits() ^ u32::from(mutate());
        match want[i] {
            Some(v) if v as u32 != g => return Err(format!("{what}[{i}]: original {:08x}, port {g:08x}", v as u32)),
            None if mutate() => return Err(format!("{what}[{i}]: not written")),
            _ => {}
        }
    }
    Ok(())
}

fn check_map_tcoord(c: &Case) -> Verdict {
    let m = map_of(&Loaded::of(c), "map");
    let before = c.input("pair").f32s();
    let mut pair = [before[0], before[1]];
    let b = c.input("bound").f32s();
    let bits = map_tcoord_compute(&m, vec3_of(c.input("p")), &mut pair, &[b[0], b[1], b[2], b[3], b[4], b[5]], &vec3_of(c.input("size")), &vec3_of(c.input("centre")));
    verdict((|| {
        let want = c.output("ax").values()[0] as u16;
        if want != bits ^ u16::from(mutate()) {
            return Err(format!("seam bits: original {want}, port {bits}"));
        }
        same_f32s("pair", c.after("pair"), &pair)
    })())
}

/// The positions (+0x20) of the first `n` render vertices of the buffer `verts` (19 words a
/// vertex).
fn positions(c: &Case, n: usize) -> Vec<Vector> {
    let w = c.input("verts").words();
    (0..n).map(|i| { let at = 19 * i + 8; let f = |k: usize| f32::from_bits(w[at + k].value().unwrap() as u32); Vector::new(f(0), f(1), f(2)) }).collect()
}

fn check_rot_bounding(c: &Case) -> Verdict {
    let model = Image::of(c.input("model"));
    let n = model.i32(0x228);
    let points: Vec<Vector> = if n != 0 {
        positions(c, n as usize)
    } else if model.i32(0x30) == 4 {
        // the mesh that lends positions: +0x3c, else the first definition when there is
        // one, the second otherwise
        let name = match model.ptr(0x3c) {
            Some((name, _)) => name.to_string(),
            None if model.i16(0x34) == 1 => "mesh".to_string(),
            None => "mesh2".to_string(),
        };
        let count = c.input(&name).words()[5].value().unwrap() as usize;
        let w = c.input("mv").words();
        (0..count).map(|i| { let f = |k: usize| f32::from_bits(w[5 * i + k].value().unwrap() as u32); Vector::new(f(0), f(1), f(2)) }).collect()
    } else {
        Vec::new()
    };
    let with_radii = c.input("g_opt").u8s()[0] & 0x20 == 0;
    let got = model_compute_rot_bounding(&mut points.into_iter(), &vec3_of(c.input("rot")), with_radii);
    verdict((|| {
        same_f32s("least", c.output("lo"), &got.least.to_array())?;
        same_f32s("greatest", c.output("hi"), &got.greatest.to_array())?;
        let (r5, r6) = (c.output("r5").written()[0], c.output("r6").written()[0]);
        match got.radii {
            Some((a, b)) => {
                same_f32s("radius 5", c.output("r5"), &[a])?;
                same_f32s("radius 6", c.output("r6"), &[b])?;
                if r5.is_none() || r6.is_none() {
                    return Err("the original did not write the radii".into());
                }
            }
            None if r5.is_some() || r6.is_some() => return Err("the original wrote the radii".into()),
            None => {}
        }
        Ok(())
    })())
}

fn check_projection_info(c: &Case) -> Verdict {
    let before = Image::of(c.input("map"));
    let mut m = map_of(&Loaded::of(c), "map");
    let model = Image::of(c.input("model"));
    let points = positions(c, model.i32(0x228) as usize);
    let b = c.input("bound").f32s();
    let mut bound = [b[0], b[1], b[2], b[3], b[4], b[5]];
    let (mut size, mut centre) = (vec3_of(c.input("size")), vec3_of(c.input("centre")));
    let mut bounding = |rot: &Vector| model_compute_rot_bounding(&mut points.iter().copied(), rot, true);
    txt_projection_info(&mut m, model.i32(0x354), &mut bounding, 0.0, &mut bound, &mut size, &mut centre);
    let mut port = before;
    put_map(&m, &mut port);
    verdict((|| {
        same_image("map", c.after("map"), &port, &kept)?;
        same_f32s("bound", c.after("bound"), &bound)?;
        same_f32s("size", c.after("size"), &size.to_array())?;
        same_f32s("centre", c.after("centre"), &centre.to_array())
    })())
}

fn check_effects(c: &Case) -> Verdict {
    let before = Image::of(c.input("map"));
    let mut m = map_of(&Loaded::of(c), "map");
    upd_txt2d_effects(&mut m);
    let mut port = before;
    put_map(&m, &mut port);
    verdict(same_image("map", c.after("map"), &port, &kept))
}

/// `updateTxt3DTransf` on three nodes (a root, its brother, its son), two materials and
/// three 3D textures: the heads of the three `SI_Txt3d` records afterwards.
fn check_txt3d_transf(c: &Case) -> Verdict {
    use si_render::geometry::RenModel;
    let mut st = ShadeState::default();
    let v = c.input("g_view").f32s();
    st.camera.world_to_raw_mat = core::array::from_fn(|r| core::array::from_fn(|k| v[4 * r + k]));
    let index = |p: Option<(&str, u32)>, prefix: &str| p.and_then(|(n, _)| n.strip_prefix(prefix)).and_then(|d| d.parse::<usize>().ok());
    let l = Loaded::of(c);
    for k in 0..3 {
        let mut t = tex3d_of(&l, &format!("t{k}"));
        let head = Image::of(c.input(&format!("x{k}")));
        t.txt = Some(Txt3d { has_tform: head.i16(4), tform: core::array::from_fn(|r| head.f32s(8 + 16 * r)), ..Txt3d::default() });
        st.tex3ds.push(t);
    }
    for k in 0..2 {
        let im = Image::of(c.input(&format!("m{k}")));
        st.materials.push(ShadeMaterial { tex3d: index(im.ptr(0x6fc), "t").map_or(0, |i| i as u32 + 1), ..ShadeMaterial::default() });
    }
    // nodes 0 to 2, then the two records whose matrices they use
    let mut models = Vec::new();
    let mut own = Vec::new();
    let names = ["node", "n1", "n2"];
    for (k, name) in names.iter().enumerate() {
        if c.input(name).is_null() {
            models.push(RenModel::default());
            own.push(0);
            continue;
        }
        let im = Image::of(c.input(name));
        let array = c.input(&format!("marr{k}")).words();
        let node = |p: Option<(&str, u32)>| p.and_then(|(n, _)| names.iter().position(|x| *x == n));
        models.push(RenModel {
            latdep: 3 + index(im.ptr(0x44), "lat").unwrap(),
            materials: (0..im.i16(0x206).max(0) as usize).map(|i| match &array[i] { Word::Ref { name, .. } => name[1..].parse().unwrap(), _ => 0 }).collect(),
            brother: node(im.ptr(0x2c0)),
            son: node(im.ptr(0x2c4)),
            ..RenModel::default()
        });
        own.push(index(im.ptr(0x224), "t").map_or(0, |i| i as u32 + 1));
    }
    for k in 0..2 {
        let im = Image::of(c.input(&format!("lat{k}")));
        models.push(RenModel { gmat: core::array::from_fn(|r| im.f32s(0x1c4 + 16 * r)), ..RenModel::default() });
    }
    let root = if c.input("node").is_null() { None } else { Some(0) };
    if update_txt3d_transf(&mut st, &models, root, &|n| own[n]) != 0 {
        // a node without an inverse and none kept at its depth: the original multiplies by
        // what its stack held (the same words in both of the harness's runs when callees
        // of the level above left them)
        return Verdict::Skip;
    }
    verdict((|| {
        for k in 0..3 {
            let name = format!("x{k}");
            let mut port = Image::of(c.input(&name));
            let t = st.tex3ds[k].txt.as_ref().unwrap();
            port.put_i16(4, t.has_tform);
            for r in 0..4 {
                for col in 0..3 {
                    port.put_f32(8 + 16 * r + 4 * col, t.tform[r][col]);
                }
            }
            same_image(&name, c.after(&name), &port, &kept)?;
        }
        Ok(())
    })())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_BlookUp" | "si_BlookUp#alt" => check_blookup,
        "SI_renderTxt2d" => check_render_txt2d,
        "makeseed" => |c: &Case| {
            let mut s = seed_of(c.input("s"));
            makeseed(&mut s);
            verdict(same_seed(c, "s", s))
        },
        "erand48" => |c: &Case| {
            let mut s = seed_of(c.input("s"));
            let v = erand48(&mut s);
            verdict(same_seed(c, "s", s).and_then(|_| same_st0(c, v)))
        },
        "SI_srand48" => |c: &Case| {
            let mut s = Seed(seed_of(c.input("g_seed")));
            srand48(&mut s, c.input("seed").i32());
            verdict(same_seed(c, "g_seed", s.0))
        },
        "SI_drand48" => |c: &Case| {
            let mut s = Seed(seed_of(c.input("g_seed")));
            let v = drand48(&mut s);
            verdict(same_seed(c, "g_seed", s.0).and_then(|_| same_st0(c, v)))
        },
        "SI_ttnInitNoise" => |c: &Case| {
            let mut n = Noise { seed: Seed(seed_of(c.input("g_seed"))), ..Noise::default() };
            init_noise(&mut n);
            let table: Vec<f32> = if mutate() { n.table.iter().map(|v| f32::from_bits(v.to_bits() ^ 1)).collect() } else { n.table.clone() };
            verdict(same_seed(c, "g_seed", n.seed.0).and_then(|_| {
                if !c.output("table").same_f32(&table) || c.output("first_call").u16s() != [n.first_call as u16] {
                    return Err("the table differs".to_string());
                }
                Ok(())
            }))
        },
        "SI_ttnInitNoise#again" => |c: &Case| {
            // `first_call` is clear: nothing happens (the seed is an in-out buffer: it must
            // come back as it went in)
            let mut n = Noise { seed: Seed(seed_of(c.input("g_seed"))), first_call: 0, table: vec![0.5; 4] };
            init_noise(&mut n);
            verdict(same_seed(c, "g_seed", n.seed.0).and_then(|_| if n.table == [0.5; 4] { Ok(()) } else { Err("the table was filled again".into()) }))
        },
        "SI_ttnGetNoise" => |c: &Case| {
            NOISE.with(|n| verdict(same_st0(c, get_noise(n, c.input("x").f32(), c.input("y").f32(), c.input("z").f32()))))
        },
        "SI_ttMakeMarble" => |c: &Case| check_make(c, 0),
        "SI_ttMakeWood" => |c: &Case| check_make(c, 1),
        "SI_ttMakeCloud" => |c: &Case| check_make(c, 2),
        "SI_ttSetTform" => check_set_tform,
        "SI_ttSetColorMap" => check_set_color_map,
        "SI_updTxt3dTurb" => check_upd_txt3d,
        "updateTxt3DTransf" => check_txt3d_transf,
        "SI_mapTcoordCompute" => check_map_tcoord,
        "SI_modelComputeRotBounding" => check_rot_bounding,
        "SI_txtProjectionInfo" => check_projection_info,
        "SI_updTxt2dEffects" => check_effects,
        _ => return None,
    })
}

thread_local! {
    /// The noise table as the port fills it (the vectors of `SI_ttnGetNoise` were made on
    /// the table the original's `SI_ttnInitNoise` leaves, which its own check compares).
    static NOISE: Noise = {
        let mut n = Noise::default();
        init_noise(&mut n);
        n
    };
}
