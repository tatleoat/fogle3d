// SPDX-License-Identifier: MIT
//! From the records of a model, material or texture file (`si-hrc`, finding 0004) to the
//! renderer's records. Not routines of the original: its readers fill `__SI_Material`,
//! `__SI_LclMap` and `__SI_Texture3D` in place, and what they leave there is what these
//! conversions put into the members this crate has. The two routines the readers call to
//! finish a texture are run: `SI_updTxt2dEffects` and `SI_updTxt3dTurb`.

use crate::shade::{ShadeMap, ShadeMaterial};
use crate::t2d_fct::upd_txt2d_effects;
use crate::t3d_ren::{upd_txt3d_turb, ShadeTex3d};
use crate::tt::TtLib;
use crate::tt_noise::Noise;

/// A 2D texture of a file as the renderer's map record, its effect words set.
pub fn map_from_file(t: &si_hrc::Texture2D) -> ShadeMap {
    let mut m = ShadeMap {
        // (finding 0004's correction: the file's first string is the picture's path, +0x14)
        path: t.name.clone(),
        anim: t.anim,
        method: t.method,
        w2c: t.pixel_interp,
        crop: t.crop_flag,
        crop_x0: t.crop[0],
        crop_x1: t.crop[1],
        crop_y0: t.crop[2],
        crop_y1: t.crop[3],
        uscale: t.uscale,
        vscale: t.vscale,
        uoff: t.uoffset,
        voff: t.voffset,
        rotation: t.rotation,
        translation: t.translation,
        scaling: t.scaling,
        urepeat: t.urepeat,
        vrepeat: t.vrepeat,
        swap_uv: t.swap_uv,
        alt_u: t.alt_u,
        alt_v: t.alt_v,
        band_u: t.wrap_u,
        band_v: t.wrap_v,
        mask: t.mask,
        blend: t.blend,
        bump_alpha: t.channel,
        factors: [t.ambient, t.diffuse, t.specular, t.reflectivity, t.transparency, t.old_factor, t.roughness, t.reflection_map],
        env_rotation: t.env_rotation,
        black_white: t.black_white,
        repeat: t.x1d4,
        box_node: -1,
        ..ShadeMap::default()
    };
    upd_txt2d_effects(&mut m);
    m
}

/// A 3D texture of a file as the renderer's record, made ready (`SI_updTxt3dTurb`). `Err`
/// when the original would write outside its record (a kind that is not 0, 1 or 2; a ramp
/// stop outside 0 … 1).
pub fn tex3d_from_file(t: &si_hrc::Texture3D, noise: &mut Noise, lib: &mut dyn TtLib) -> Result<ShadeTex3d, &'static str> {
    let mut r = ShadeTex3d {
        kind: t.kind,
        spacing: t.spacing,
        taper: t.taper,
        strength: t.strength,
        iterations: t.iterations,
        power: t.power,
        mask: t.mask,
        blend: t.blending,
        channel: t.channel,
        colours: t.colours,
        stops: t.factors,
        scaling: t.scaling,
        rotation: t.rotation,
        translation: t.translation,
        factors: [t.ambient, t.diffuse, t.specular, t.reflection, t.transparency, 0.0, t.roughness],
        ..ShadeTex3d::default()
    };
    if !(0..=8).contains(&r.iterations) && r.kind != 1 {
        return Err("more than eight noise terms: the ninth pair is the ramp's first colour");
    }
    upd_txt3d_turb(&mut r, noise, lib)?;
    Ok(r)
}

/// A material of a file as the renderer's record, without its textures (the caller links
/// the maps and the 3D texture).
pub fn material_from_file(m: &si_hrc::Material) -> ShadeMaterial {
    let mut r = ShadeMaterial {
        model: m.shading,
        ambient: [m.ambient[0], m.ambient[1], m.ambient[2], 0.0],
        diffuse: [m.diffuse[0], m.diffuse[1], m.diffuse[2], 0.0],
        specular: [m.specular[0], m.specular[1], m.specular[2], 0.0],
        reflectivity: m.reflectivity,
        transparency: m.transparency,
        refraction: m.refraction,
        static_blur: m.blur,
        blur_width: m.blur_width,
        vcol_rgb: m.xdc8,
        vcol_alpha: m.xdca,
        ..ShadeMaterial::default()
    };
    r.spec_table.exponent = m.exponent;
    r.blur_table.exponent = m.blur_decay;
    r.jitter_6ac.amplitude = m.x6ac;
    r.jitter_6bc.amplitude = m.x6bc;
    r
}

/// A patch of a model file as the patch code holds one in memory (`SI_Patch`): the types,
/// counts, closed switches, tensions, steps and points; the subdivision is not made yet.
/// For the corpus check and for whoever builds a scene from loaded files.
pub fn surface_from_file(p: &si_hrc::Patch) -> si_spline::patch::Surface {
    let mut s = si_spline::patch::Surface::new();
    let r = &mut s.rec;
    (r.u_type, r.v_type) = (p.kind[0], p.kind[1]);
    (r.nb_u, r.nb_v) = (p.nu, p.nv);
    (r.u_closed, r.v_closed) = (p.close[0], p.close[1]);
    (r.u_tension, r.v_tension) = (p.tension[0], p.tension[1]);
    (r.u_step, r.v_step) = (p.step[0] as u16, p.step[1] as u16);
    s.points = p.points.iter().map(|q| [q[0], q[1], q[2], 1.0]).collect();
    if !s.points.is_empty() {
        // (a placeholder for the block's address: any pointer that is not null)
        s.rec.points = si_types::Ptr32::new(1);
    }
    s
}
