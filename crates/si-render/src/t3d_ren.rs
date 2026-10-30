// SPDX-License-Identifier: MIT
//! A 3D texture applied to a material: the original's `t3d_ren.c`, and the routine of
//! `t3d_fct.c` that makes a 3D texture record ready (finding 0012, section 14).

use crate::shade::ShadeState;
use crate::tt::{apply_txt3d, make_cloud, make_marble, make_wood, set_color_map, set_tform, TtLib, Txt3d};
use crate::tt_noise::Noise;
use crate::types::Vector;
use si_math::algebra::matrix_xyz_rotation;
use si_math::types::Matrix;
use si_x87::{x, F80};

/// 0x08656338 (float): half of (least + greatest) is the intensity.
const HALF: f32 = 0.5;
/// 0x08656340 (double): a perturbed normal shorter than this is not normalised.
const EPS: f64 = 1e-4;
/// 0x08656320, 0x08656328 (double): a factor beyond these drives its channel.
const SOME: f64 = 1e-4;

/// The members of `__SI_Texture3D` (0x158 bytes) the renderer reads and writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadeTex3d {
    /// +0x0c: 0 marble, 1 wood, 2 cloud.
    pub kind: i32,
    /// +0x10 spacing, +0x14 taper (an angle, radians), +0x18 strength, +0x1c number of noise
    /// terms, +0x20 power.
    pub spacing: f32,
    pub taper: f32,
    pub strength: f32,
    pub iterations: i32,
    pub power: f32,
    /// +0x24: what scales the blend: 1 the colour's alpha, 2 its intensity, else nothing.
    pub mask: i16,
    /// +0x28: the blend factor.
    pub blend: f32,
    /// +0x2c: what drives reflectivity, transparency and roughness: 2 the intensity,
    /// anything else the alpha.
    pub channel: i16,
    /// +0x30: the ramp's five colours; +0x80: where the three inner ones stand (0 … 1).
    pub colours: [[f32; 4]; 5],
    pub stops: [f32; 3],
    /// +0x8c: the texture made ready (`SI_Txt3d *`).
    pub txt: Option<Txt3d>,
    /// +0x90 scaling, +0x9c rotation (radians), +0xa8 translation.
    pub scaling: [f32; 3],
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    /// +0xb4: the texture's own matrix: the rotation's rows times the scaling, the
    /// translation in row 3.
    pub matrix: Matrix,
    /// +0xf4, +0xf6, +0xf8, +0xfa, +0xfc, +0x102: the texture drives the ambient colour,
    /// the diffuse colour, the specular colour, the reflectivity, the transparency, the
    /// normal (`SI_updTxt3dTurb`); +0x100: a word `SI_materials` reads as "drives
    /// refraction" and nothing writes.
    pub fx_ambient: i16,
    pub fx_diffuse: i16,
    pub fx_specular: i16,
    pub fx_reflect: i16,
    pub fx_transp: i16,
    pub fx_100: i16,
    pub fx_rough: i16,
    /// +0x108 … +0x120: the seven factors: ambient, diffuse, specular, reflection,
    /// transparency, one no token sets (+0x11c), roughness.
    pub factors: [f32; 7],
    /// +0x128 … +0x140: the same as `SI_materials` prepares them (the ambient one times the
    /// mean ambience).
    pub work: [f32; 7],
}

/// Makes a 3D texture record ready [soft 0x082b8d20 SI_updTxt3dTurb]:
/// `SI_updTxt3dTurb(texture)`. The old `SI_Txt3d` is freed; a new one is made by kind
/// (`SI_ttMakeMarble(spacing, taper, strength, iterations, power)`, `SI_ttMakeWood(spacing,
/// taper, strength, power)`, `SI_ttMakeCloud(iterations, power)`; none for another kind,
/// and the calls below then go through a null pointer); `SI_ttSetColorMap` with the five
/// colours and three stops; the matrix (+0xb4) = `SI_matrixXyzRotation(rotation)` with the
/// first three floats of row k multiplied by the k-th scaling factor and the translation
/// in row 3; `SI_ttSetTform`; and the six effect words: a factor above 0.0001 for ambient,
/// diffuse and specular, beyond ±0.0001 for transparency, reflection and roughness.
/// Class: libm.
pub fn upd_txt3d_turb(t: &mut ShadeTex3d, noise: &mut Noise, lib: &mut dyn TtLib) -> Result<(), &'static str> {
    t.txt = match t.kind {
        0 => Some(make_marble(noise, t.spacing, t.taper, t.strength, t.iterations, t.power, lib)),
        1 => Some(make_wood(noise, t.spacing, t.taper, t.strength, t.power, lib)),
        2 => Some(make_cloud(noise, t.iterations, t.power, lib)),
        _ => None,
    };
    let txt = t.txt.as_mut().ok_or("SI_updTxt3dTurb: a kind that is not 0, 1 or 2 leaves a null texture")?;
    set_color_map(txt, &t.colours, t.stops)?;
    let mut m = matrix_xyz_rotation(&Vector::new(t.rotation[0], t.rotation[1], t.rotation[2]));
    for r in 0..3 {
        for c in 0..3 {
            m[r][c] = (x(m[r][c]) * t.scaling[r]).to_f32();
        }
    }
    m[3][0] = t.translation[0];
    m[3][1] = t.translation[1];
    m[3][2] = t.translation[2];
    t.matrix = m;
    set_tform(txt, &m);
    let above = |v: f32| i16::from(x(SOME).lt(x(v)));
    let beyond = |v: f32| i16::from(x(v).lt(x(-SOME)) || x(v).gt(x(SOME)));
    t.fx_ambient = above(t.factors[0]);
    t.fx_diffuse = above(t.factors[1]);
    t.fx_specular = above(t.factors[2]);
    t.fx_transp = beyond(t.factors[4]);
    t.fx_reflect = beyond(t.factors[3]);
    t.fx_rough = beyond(t.factors[6]);
    Ok(())
}

/// Least + greatest of a colour's red, green and blue, as the comparison tree of
/// `SI_renderTxt3d` picks the two (four copies of it in the routine).
fn least_and_greatest(c: &[f32; 4]) -> F80 {
    let (r, g, b) = (x(c[0]), x(c[1]), x(c[2]));
    if g.gt(r) {
        if !b.gt(r) {
            b + g
        } else if b.gt(g) {
            b + r
        } else {
            r + g
        }
    } else if !b.gt(g) {
        b + r
    } else if !b.gt(r) {
        r + g
    } else {
        b + g
    }
}

/// Blends a 3D texture into a material's working block and, with a normal, perturbs it
/// [soft 0x082b97d0 SI_renderTxt3d]: `SI_renderTxt3d(material, point, &normal)`; the
/// texture is the material's (+0x6fc), the normal may be null.
///
/// 1. C = `SI_ttApplyTxt3d(texture, point)`; I = (least + greatest of its red, green,
///    blue) × 0.5, in the register.
/// 2. The blend B = the texture's blend factor (+0x28), with mask (+0x24) 1 times C's
///    alpha, with mask 2 times I; **kept in the register**. R = 1 − B rounded to float.
/// 3. Ambient (+0xf4), diffuse (+0xf6), specular (+0xf8): each component of the working
///    colour becomes ((factor × C) × B) + (R × component), the factors at +0x128, +0x12c,
///    +0x130.
/// 4. h = I when the channel word (+0x2c) is 2, else C's alpha; N = 1 − h rounded to
///    float. Reflectivity (+0xfa; factor f at +0x134): ((f × h) × B) + (R × old), or for f
///    below 0 (((−f) × N) × B) + (R × old). Opacity (+0xfc; f at +0x138): (B × (1 − f × h))
///    + (old × R), or for f below 0 (B × ((N × f) + 1)) + (old × R).
/// 5. With a normal and the roughness word (+0x102): the texture at the point moved by 1
///    along x, along y and along z gives three more heights (I or alpha as in 4); the
///    normal becomes normal + roughness (+0x140) × (height − h) per axis, each sum rounded
///    to float on the store but used unrounded for the length; when the length (rounded to
///    float) is above 0.0001 in size the three are divided by it.
///
/// The texture's colour for a kind that is not 0, 1 or 2 is not written by
/// `SI_ttApplyTxt3d`; `unset` stands for those four floats. Class: model (libm inside the
/// texture).
pub fn render_txt3d(st: &mut ShadeState, material: usize, p: Vector, normal: Option<&mut Vector>, lib: &mut dyn TtLib) {
    let tex = st.materials[material].tex3d as usize - 1;
    let t = &st.tex3ds[tex];
    let txt = t.txt.as_ref().expect("SI_renderTxt3d: a 3D texture that was not made ready");
    let unset = [st.glb.unset; 4];
    let c = apply_txt3d(txt, &st.noise, [p.x, p.y, p.z], lib).unwrap_or(unset);
    let b: F80 = match t.mask {
        2 => x(t.blend) * (least_and_greatest(&c) * HALF),
        1 => x(t.blend) * c[3],
        _ => x(t.blend),
    };
    let r = (F80::ONE - b).to_f32();
    let w = &mut st.materials[material].work;
    let mix = |factor: f32, s: f32, old: f32| (((x(factor) * s) * b) + (x(r) * old)).to_f32();
    if t.fx_ambient != 0 {
        for k in 0..3 {
            w.ambient[k] = mix(t.work[0], c[k], w.ambient[k]);
        }
    }
    if t.fx_diffuse != 0 {
        for k in 0..3 {
            w.diffuse[k] = mix(t.work[1], c[k], w.diffuse[k]);
        }
    }
    if t.fx_specular != 0 {
        for k in 0..3 {
            w.specular[k] = mix(t.work[2], c[k], w.specular[k]);
        }
    }
    let height = |c: &[f32; 4]| if t.channel == 2 { least_and_greatest(c) * HALF } else { x(c[3]) };
    let h = height(&c);
    let n = (F80::ONE - h).to_f32();
    let negative = |f: f32| x(f).lt(x(0.0f32)) || x(f).unordered(x(0.0f32));
    if t.fx_reflect != 0 {
        let f = t.work[3];
        let v = if negative(f) { (-x(f)) * n } else { x(f) * h };
        w.reflectivity = ((v * b) + (x(r) * w.reflectivity)).to_f32();
    }
    if t.fx_transp != 0 {
        let f = t.work[4];
        let v = if negative(f) { (x(n) * f) + F80::ONE } else { F80::ONE - (x(f) * h) };
        w.opacity = ((b * v) + (x(w.opacity) * r)).to_f32();
    }
    let Some(normal) = normal else { return };
    if t.fx_rough == 0 {
        return;
    }
    let moved = [
        [(F80::ONE + p.x).to_f32(), p.y, p.z],
        [p.x, (F80::ONE + p.y).to_f32(), p.z],
        [p.x, p.y, (F80::ONE + p.z).to_f32()],
    ];
    let c1 = apply_txt3d(txt, &st.noise, moved[0], lib).unwrap_or(unset);
    let c2 = apply_txt3d(txt, &st.noise, moved[1], lib).unwrap_or(unset);
    let c3 = apply_txt3d(txt, &st.noise, moved[2], lib).unwrap_or(unset);
    let rough = t.work[6];
    let (dx, dy, dz) = if t.channel == 2 {
        (((least_and_greatest(&c1) * HALF) - h) * rough, ((least_and_greatest(&c2) * HALF) - h) * rough, ((x(HALF) * least_and_greatest(&c3)) - h) * rough)
    } else {
        (x(rough) * (x(c1[3]) - h), x(rough) * (x(c2[3]) - h), x(rough) * (x(c3[3]) - h))
    };
    let t1 = dx + normal.x;
    let t2 = dy + normal.y;
    let t3 = dz + normal.z;
    *normal = Vector::new(t1.to_f32(), t2.to_f32(), t3.to_f32());
    let len = ((t3 * t3) + ((t1 * t1) + (t2 * t2))).sqrt().to_f32();
    if x(len).abs().gt(x(EPS)) {
        *normal = Vector::new((t1 / len).to_f32(), (t2 / len).to_f32(), (t3 / len).to_f32());
    }
}
