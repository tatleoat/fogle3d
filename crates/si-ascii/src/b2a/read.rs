// SPDX-License-Identifier: MIT
//! The readers of the text form: `bin2ascii`'s own routines, read from its disassembly, on
//! the token reader of `a_utilio.c` that they share with `soft` (`si_inToken` and the value
//! readers, ported with the setup file in [`crate::setup`]).
//!
//! Every reader works the same way: a token is in the buffer, a keyword is recognised, its
//! handler reads what follows and leaves the next keyword in the buffer, and the first word
//! that is no keyword ends the record (after one more token if it was the very first). The
//! routines trust the text as the converter's own writer lays it out; where a handler reads
//! one token too many or too few, what follows is misread, and that is reproduced: the
//! light's `AREA_ROTATION_MAT` (and an `AREA_GEOMETRY` of type 0 or 2) read every other
//! token and run on into the following keywords, which ends the light there.
//!
//! The values land in the value forms of `si-hrc` and `si-elemio`, which their binary
//! writers take. Fields of the original's records that those forms do not hold
//! (`ATTENUATION` and `MOTION_TRANSPARENCY` of a material) are read and dropped: no binary
//! writer writes them.

use crate::setup::{value3_ascii_read, value_ascii_read, Reader, Setup, SetupError};
use si_elemio::anim::{HierAnim, ItemData};
use si_elemio::camera::Camera;
use si_elemio::fcurve::{convert_angle_unit_to_degree, Fcurve, Key, StaticVal};
use si_elemio::lattice::Lattice;
use si_elemio::light::Light;
use si_hrc::{Material, Texture3D};

/// Where the readers print: `printf` (the standard output), and the status line
/// (`SI_statusDisplayMessage`, level 3), which the converter has no window for; kept apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Printed {
    pub stdout: Vec<u8>,
    pub status: Vec<Vec<u8>>,
}

impl Printed {
    fn printf(&mut self, fmt: &str, tok: &[u8]) {
        let mut v = Vec::new();
        let (a, b) = fmt.split_once("%s").unwrap_or((fmt, ""));
        v.extend_from_slice(a.as_bytes());
        v.extend_from_slice(tok);
        v.extend_from_slice(b.as_bytes());
        self.stdout.extend_from_slice(&v);
    }
}

/// `strncpy(dst, src, 30)` into a 30-byte name field: up to 30 bytes; the value forms keep
/// the bytes before the first zero.
fn strncpy30(src: &[u8]) -> Vec<u8> {
    src[..src.len().min(30)].to_vec()
}

/// [soft 0x083dcc0c si_newpositionAsciiRead]: `STATIC` and a vector (stored where `dst`
/// points), or `CONSTRAINT`, a 16-bit number and a name, which are read and dropped; then
/// the next token. Any other word prints "PositionAsciiRead: error, unknown token" and only
/// the next token is read. 0 when the file ends at the first token, else 1.
pub(crate) fn newposition_ascii_read(r: &mut Reader, p: &mut Printed, dst: Option<&mut [f32; 3]>) -> i16 {
    if !r.in_token() {
        return 0;
    }
    if r.is("CONSTRAINT") {
        let mut local = 0i16;
        r.in_svalue(&mut local);
        r.string_read();
    } else if r.is("STATIC") {
        r.vector_read(dst);
    } else {
        p.printf("PositionAsciiRead: error, unknown token '%s'.", r.token());
        r.in_token();
        return 1;
    }
    r.in_token();
    1
}

/// `SI_materialAsciiRead` (bin2ascii 0x0824f42c) into a material as `SI_materialAllocate`
/// made it. Always 1. `SPECULAR`'s fourth value and the second of `TRANSPARENCY` and
/// `REFLECTION` are taken from the token the first value's reader left (its last argument
/// 0).
pub(crate) fn material_ascii_read(r: &mut Reader, s: &Setup, m: &mut Material) -> Result<i16, SetupError> {
    let mut first = true;
    let mut dropped = [0.0f32; 4]; // ATTENUATION and MOTION_TRANSPARENCY
    r.in_token();
    loop {
        if r.is("MATERIAL_NAME") {
            if let Some(n) = r.string_read() {
                m.name = strncpy30(&n);
            }
            r.in_token();
        } else if r.is("SHADING_MODEL") {
            r.in_svalue_na();
            m.shading = r.svalue;
            r.in_token();
        } else if r.is("GLOW") {
            r.in_svalue_na();
            m.glow = r.svalue;
            r.in_token();
        } else if r.is("COC") {
            value_ascii_read(r, s, Some(&mut m.glow_amount), true)?;
        } else if r.is("AMBIENT") {
            value3_ascii_read(r, s, Some(&mut m.ambient))?;
        } else if r.is("DIFFUSE") {
            value3_ascii_read(r, s, Some(&mut m.diffuse))?;
        } else if r.is("SPECULAR") {
            value3_ascii_read(r, s, Some(&mut m.specular))?;
            value_ascii_read(r, s, Some(&mut m.exponent), false)?;
        } else if r.is("ATTENUATION") {
            let mut v = [0.0f32; 3];
            value3_ascii_read(r, s, Some(&mut v))?;
            dropped[..3].copy_from_slice(&v);
        } else if r.is("TRANSPARENCY") {
            value_ascii_read(r, s, Some(&mut m.transparency), true)?;
            value_ascii_read(r, s, Some(&mut m.x6bc), false)?;
        } else if r.is("REFLECTION") {
            value_ascii_read(r, s, Some(&mut m.reflectivity), true)?;
            value_ascii_read(r, s, Some(&mut m.x6ac), false)?;
        } else if r.is("REFRACTION") {
            value_ascii_read(r, s, Some(&mut m.refraction), true)?;
        } else if r.is("MOTION_TRANSPARENCY") {
            value_ascii_read(r, s, Some(&mut dropped[3]), true)?;
        } else if r.is("STATIC_BLUR") {
            r.in_token();
            if r.is("YES") {
                m.blur = 1;
            } else if r.is("NO") {
                m.blur = 0;
            }
            r.in_token();
        } else if r.is("STATIC_BLUR_WIDTH") {
            value_ascii_read(r, s, Some(&mut m.blur_width), true)?;
        } else if r.is("DECAY_VALUE") {
            value_ascii_read(r, s, Some(&mut m.blur_decay), true)?;
        } else {
            if first {
                r.in_token();
            }
            return Ok(1);
        }
        first = false;
    }
}

/// Why the 3D texture reader stopped where the original would go on in a way the port does
/// not follow.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TextureStop {
    Setup(SetupError),
    /// `STRENGTH`, `SPACING` or `POWER` copied the reader's local into the texture while it
    /// still held whatever the stack had (a function curve in place of the value, or the end
    /// of the file, before any of the three had set it).
    UninitialisedLocal,
}

impl From<SetupError> for TextureStop {
    fn from(e: SetupError) -> Self {
        TextureStop::Setup(e)
    }
}

/// `SI_texture3DAsciiRead` (bin2ascii 0x08250200) into a 3D texture as `SI_texture3DAllocate`
/// (0x082b8cf0) made it. Always 1.
///
/// The same loop as the material's: a keyword, its value, the next keyword in the buffer;
/// the first word that is no keyword ends the texture (after one more token if it was the
/// very first). `TEXTURE_3D_NAME` takes the quoted string as the name pointer (`''` gives a
/// null pointer, which the binary writer writes as an empty string). `TYPE` and `NB_ITER`
/// store the 16-bit value sign-extended into their 32-bit fields; `USE_ALPHA` and
/// `USE_EFFECT` store it as a short. `REFRACTION` and `STRETCH` are read and dropped (no
/// field).
///
/// `STRENGTH`, `SPACING` and `POWER` are read into one local of the routine and copied from
/// it into the field whatever the value reader did: after a function curve or at the end of
/// the file the field gets the local's previous value, which before the first of the three
/// is uninitialised stack ([`TextureStop::UninitialisedLocal`]). The other values are read
/// straight into their fields and are left alone then.
///
/// (The original tests the texture pointer before each store and reads into nothing when it
/// is null; `main` never passes a null texture.)
pub(crate) fn texture3d_ascii_read(r: &mut Reader, s: &Setup, x: &mut Texture3D) -> Result<i16, TextureStop> {
    let mut first = true;
    let mut local: Option<f32> = None; // ebp-0x4, uninitialised until a number lands in it
    // si_valueAsciiRead(f, &local, 0, texture, 1): its own token read, done here so that what
    // it found can be seen; the value reader then takes the token in the buffer, which is the
    // same. It stores into the local only after a number.
    let through_local = |r: &mut Reader, local: &mut Option<f32>| -> Result<f32, TextureStop> {
        if r.in_token() {
            let curve = r.is("CURVE_TYPE");
            let mut v = local.unwrap_or(0.0);
            value_ascii_read(r, s, Some(&mut v), false)?;
            if !curve {
                *local = Some(v);
            }
        }
        local.ok_or(TextureStop::UninitialisedLocal)
    };
    r.in_token();
    loop {
        if r.is("TEXTURE_3D_NAME") {
            // a null pointer for `''` (the value form keeps it as an empty name)
            x.name = r.string_read().unwrap_or_default();
            r.in_token();
        } else if r.is("TYPE") {
            r.in_svalue_na();
            x.kind = r.svalue as i32;
            r.in_token();
        } else if r.is("NB_ITER") {
            r.in_svalue_na();
            x.iterations = r.svalue as i32;
            r.in_token();
        } else if r.is("STRENGTH") {
            x.strength = through_local(r, &mut local)?;
        } else if r.is("SPACING") {
            x.spacing = through_local(r, &mut local)?;
        } else if let Some(i) = ["COLOUR_1", "COLOUR_2", "COLOUR_3", "COLOUR_4", "COLOUR_5"].iter().position(|w| r.is(w)) {
            let c = &mut x.colours[i];
            let mut v = [c[0], c[1], c[2]];
            value3_ascii_read(r, s, Some(&mut v))?;
            c[..3].copy_from_slice(&v);
        } else if let Some(i) = ["ALPHA_1", "ALPHA_2", "ALPHA_3", "ALPHA_4", "ALPHA_5"].iter().position(|w| r.is(w)) {
            value_ascii_read(r, s, Some(&mut x.colours[i][3]), true)?;
        } else if let Some(i) = ["COLOUR_POS_1", "COLOUR_POS_2", "COLOUR_POS_3"].iter().position(|w| r.is(w)) {
            value_ascii_read(r, s, Some(&mut x.factors[i]), true)?;
        } else if r.is("SCALING") {
            value3_ascii_read(r, s, Some(&mut x.scaling))?;
        } else if r.is("ROTATION") {
            value3_ascii_read(r, s, Some(&mut x.rotation))?;
        } else if r.is("TRANSLATION") {
            value3_ascii_read(r, s, Some(&mut x.translation))?;
        } else if r.is("AMBIENT") {
            value_ascii_read(r, s, Some(&mut x.ambient), true)?;
        } else if r.is("DIFFUSE") {
            value_ascii_read(r, s, Some(&mut x.diffuse), true)?;
        } else if r.is("SPECULAR") {
            value_ascii_read(r, s, Some(&mut x.specular), true)?;
        } else if r.is("ROUGHNESS") {
            value_ascii_read(r, s, Some(&mut x.roughness), true)?;
        } else if r.is("BLEND") {
            value_ascii_read(r, s, Some(&mut x.blending), true)?;
        } else if r.is("TRANSPARENCY") {
            value_ascii_read(r, s, Some(&mut x.transparency), true)?;
        } else if r.is("REFLECTION") {
            value_ascii_read(r, s, Some(&mut x.reflection), true)?;
        } else if r.is("POWER") {
            x.power = through_local(r, &mut local)?;
        } else if r.is("TAPER") {
            value_ascii_read(r, s, Some(&mut x.taper), true)?;
        } else if r.is("REFRACTION") || r.is("STRETCH") {
            value_ascii_read(r, s, None, true)?;
        } else if r.is("USE_ALPHA") {
            r.in_svalue_na();
            x.mask = r.svalue;
            r.in_token();
        } else if r.is("USE_EFFECT") {
            r.in_svalue_na();
            x.channel = r.svalue;
            r.in_token();
        } else {
            if first {
                r.in_token();
            }
            return Ok(1);
        }
        first = false;
    }
}

/// `SI_cameraAsciiRead` (bin2ascii 0x080c9684) into the camera `SI_newCamera("Test", 1)`
/// made. 0 when the file ends after `TYPE` or inside `DEPTH_OF_FIELD` or `ENLARGE`, else 1.
/// The positions' `CONSTRAINT` form is read and dropped.
pub(crate) fn camera_ascii_read(r: &mut Reader, s: &Setup, p: &mut Printed, c: &mut Camera) -> Result<i16, SetupError> {
    let mut first = true;
    c.d_ac = 0;
    c.d_b0 = 0;
    c.w_a8 = 1;
    r.in_token();
    loop {
        if r.is("TYPE") {
            r.in_svalue_na();
            if !r.in_token() {
                return Ok(0);
            }
        } else if r.is("POSITION") {
            newposition_ascii_read(r, p, Some(&mut c.position));
        } else if r.is("INTEREST") {
            newposition_ascii_read(r, p, Some(&mut c.interest));
        } else if r.is("ROLL") {
            value_ascii_read(r, s, Some(&mut c.yaw_pitch_roll[2]), true)?;
        } else if r.is("YAW_PITCH_ROLL") {
            value3_ascii_read(r, s, Some(&mut c.yaw_pitch_roll))?;
        } else if r.is("NEAR") {
            value_ascii_read(r, s, Some(&mut c.near), true)?;
        } else if r.is("FAR") {
            value_ascii_read(r, s, Some(&mut c.far), true)?;
        } else if r.is("FOCAL_LENGTH") {
            value_ascii_read(r, s, Some(&mut c.focal_length), true)?;
        } else if r.is("F_STOP") {
            value_ascii_read(r, s, Some(&mut c.f_stop), true)?;
        } else if r.is("FOCAL_DISTANCE") {
            value_ascii_read(r, s, Some(&mut c.focal_distance), true)?;
        } else if r.is("FIELD_OF_VIEW") {
            value_ascii_read(r, s, Some(&mut c.field_of_view), true)?;
        } else if r.is("DEPTH_OF_FIELD") {
            if !r.in_token() {
                return Ok(0);
            }
            if r.is("OFF") {
                c.depth_of_field = 0;
            } else if r.is("AUTOMATIC") {
                c.depth_of_field = 1;
            } else if r.is("CUSTOM") {
                c.depth_of_field = 2;
            } else {
                p.printf("CameraAsciiRead: error, unknown depth of field si_chrToken '%s'.", r.token());
            }
            if !r.in_token() {
                return Ok(0);
            }
        } else if r.is("NEAR_FOCUS") {
            value_ascii_read(r, s, Some(&mut c.near_focus), true)?;
        } else if r.is("FAR_FOCUS") {
            value_ascii_read(r, s, Some(&mut c.far_focus), true)?;
        } else if r.is("OUT_OF_FOCUS") {
            value_ascii_read(r, s, Some(&mut c.out_of_focus), true)?;
        } else if r.is("MAXIMUM_COC") {
            value_ascii_read(r, s, Some(&mut c.maximum_coc), true)?;
        } else if r.is("HIDDEN") {
            value_ascii_read(r, s, None, true)?;
        } else if r.is("ENLARGE") {
            if !r.in_token() {
                return Ok(0);
            }
            if r.is("ACTIVE") {
                c.enlarge = 1;
            } else if r.is("INACTIVE") {
                c.enlarge = 0;
            } else {
                p.printf("CameraAsciiRead: error, unknown enlarge active si_chrToken '%s'.", r.token());
            }
            r.in_token();
        } else {
            if first {
                r.in_token();
            }
            return Ok(1);
        }
        first = false;
    }
}

/// `SI_latticeAsciiRead` (bin2ascii 0x080c71a8): the lattices of a text, as a list. The
/// fields are gathered in locals and copied into a new lattice at `KEYS`, and are not
/// reset after it. `ACTIVE` and `NB_KEYS` start at zero; the interpolation codes and the
/// three vectors are locals the original never sets (here zero) until the text states
/// them, as every text the converter writes does.
pub(crate) fn lattice_ascii_read(r: &mut Reader, p: &mut Printed) -> Result<Vec<Lattice>, SetupError> {
    let mut list = Vec::new();
    let mut first = true;
    let mut active = 0i16;
    let mut nb = [0i16; 3];
    let mut interp = [0i32; 3];
    let (mut min, mut max, mut scaling) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    r.in_token();
    loop {
        if r.is("ACTIVE") {
            r.in_token();
            if r.is("YES") {
                active = 1;
            } else if r.is("NO") {
                active = 0;
            } else {
                p.printf("LatticeAsciiRead: error, unknown active token '%s'.", r.token());
            }
        } else if r.is("NB_KEYS") {
            for k in nb.iter_mut() {
                r.in_svalue_na();
                *k = r.svalue;
            }
        } else if r.is("INTERPOLATION") {
            for k in interp.iter_mut() {
                r.in_svalue_na();
                *k = r.svalue as i32;
            }
        } else if r.is("MINIMUM") {
            r.vector_read(Some(&mut min));
        } else if r.is("MAXIMUM") {
            r.vector_read(Some(&mut max));
        } else if r.is("SCALING") {
            r.vector_read(Some(&mut scaling));
        } else if r.is("KEYS") {
            let n = (nb[0] as i32).wrapping_mul(nb[1] as i32).wrapping_mul(nb[2] as i32);
            if n > 0 {
                // each point is at least six bytes of text; the original allocates for the
                // count whatever is left
                if n as usize > r.bytes_left() {
                    return Err(SetupError::CountTooLarge { keyword: "KEYS", count: n });
                }
                let mut keys = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let mut v = [0.0f32; 3];
                    r.vector_read(Some(&mut v));
                    keys.push(v);
                }
                list.push(Lattice {
                    active,
                    nb_keys: nb,
                    interpolation: interp,
                    keys,
                    key_flags: vec![0; n as usize],
                    minimum: min,
                    maximum: max,
                    scaling,
                });
            }
        } else {
            if first {
                r.in_token();
            }
            return Ok(list);
        }
        r.in_token();
        first = false;
    }
}

/// `SI_lightAsciiRead` (bin2ascii 0x080c8504). `light` is the light the caller allocated
/// (`SI_lightAllocate(1)`); `LIGHT_TYPE` replaces it with a new light of that type
/// (`SI_lightAllocate`, with the name read so far copied in), as the original replaces the
/// caller's pointer. Always 1.
///
/// Handlers that read one token too many are reproduced: `AREA_ROTATION_MAT` reads its 16
/// values with `si_valueAsciiRead` asking each for a fresh token, so it takes every other
/// token and runs on into `DIFFUSE`, `ENERGY`, `EXPONENT` and the rest, storing 0 for each
/// keyword it meets; the light ends at the token it is left with. `AREA_GEOMETRY` of type 0
/// (four triples) and 2 (a triple and a value) misread likewise. The binary `EXPONENT` of
/// the converter's own text reads as an empty token: 0.
pub(crate) fn light_ascii_read(r: &mut Reader, s: &Setup, p: &mut Printed, light: &mut Light, now: i32) -> Result<i16, SetupError> {
    let mut first = true;
    let mut name: Option<Vec<u8>> = None;
    let mut kind = 0i16; // [ebp-0xa]: unset until a type word is known
    r.in_token();
    loop {
        // `next`: the handler leaves the next keyword in the buffer only after one more token
        let mut next = true;
        if r.is("LIGHT_NAME") {
            name = r.string_read();
        } else if r.is("LIGHT_TYPE") {
            r.in_token();
            if r.is("POINT") {
                kind = 1;
            } else if r.is("SPOT") {
                kind = 2;
            } else if r.is("INFINITE") {
                kind = 3;
            } else if r.is("SUN") {
                kind = 4;
            } else {
                p.printf("LightAsciiRead: error, unknown light type si_chrToken '%s'.", r.token());
            }
            r.in_token();
            let mut l = Light::new(kind as i32, now);
            l.kind = kind;
            l.w_14c = 1;
            l.d_150 = 0;
            l.d_154 = 0;
            l.interest_constraint = None;
            l.position_constraint = None;
            if let Some(n) = name.take() {
                l.set_name(&n);
            }
            *light = l;
            next = false;
        } else if r.is("COLOUR") {
            value3_ascii_read(r, s, Some(&mut light.colour))?;
            next = false;
        } else if r.is("FALL_OFF_START") {
            value_ascii_read(r, s, Some(&mut light.fall_off_start), true)?;
            next = false;
        } else if r.is("FALL_OFF_END") {
            value_ascii_read(r, s, Some(&mut light.fall_off_end), true)?;
            next = false;
        } else if r.is("SHADOW_FACTOR") {
            value_ascii_read(r, s, Some(&mut light.shadow_factor), true)?;
            next = false;
        } else if r.is("POSITION") {
            newposition_ascii_read(r, p, Some(&mut light.position));
            next = false;
        } else if r.is("INTEREST") {
            newposition_ascii_read(r, p, Some(&mut light.interest));
            next = false;
        } else if r.is("CONE_ANGLE") {
            value_ascii_read(r, s, Some(&mut light.cone_angle), true)?;
            next = false;
        } else if r.is("SPREAD_ANGLE") {
            value_ascii_read(r, s, Some(&mut light.spread_angle), true)?;
            next = false;
        } else if r.is("HIDDEN") || r.is("DECAY") {
            value_ascii_read(r, s, None, true)?;
            next = false;
        } else if r.is("SHADOW_TYPE") {
            r.in_svalue_na();
            light.shadow_type = r.svalue;
        } else if r.is("SHADOW_MAP_RES") {
            r.in_svalue_na();
            light.shadow_map_res = r.svalue as i32;
        } else if r.is("SHADOW_FILTER_SIZE") {
            r.in_svalue_na();
            light.shadow_filter_size = r.svalue as i32;
        } else if r.is("SHADOW_FILTER_STEP") {
            r.in_svalue_na();
            light.shadow_filter_step = r.svalue as i32;
        } else if r.is("SHADOW_FADE_START") {
            value_ascii_read(r, s, Some(&mut light.shadow_fade_start), true)?;
            next = false;
        } else if r.is("SHADOW_FADE_RATE") {
            value_ascii_read(r, s, Some(&mut light.shadow_fade_rate), true)?;
            next = false;
        } else if r.is("WIRE_COLOUR") || r.is("LINE_TYPE") {
            r.in_svalue_na();
        } else if r.is("WIRE_COLOUR_INDEX") {
            let (mut a, mut b) = (0i16, 0i16);
            r.in_svalue(&mut a);
            r.in_svalue(&mut b);
        } else if r.is("SELECTED") {
            r.in_token();
        } else if r.is("SELECTIVE") {
            r.in_svalue_na();
            light.selective[0] = r.svalue as i32;
            r.in_svalue_na();
            light.selective[1] = r.svalue as i32;
        } else if r.is("SUN_NAME") {
            // strdup of what the string reader gives, stored through the sun pointer without
            // a test: a light that is not a sun has none, and the original faults there
            let n = r.string_read();
            match light.sun.as_mut() {
                Some(sun) => sun.name = n.unwrap_or_default(),
                None => return Err(SetupError::NullWindow { keyword: "SUN_NAME (a light without a sun record)" }),
            }
        } else if r.is("SUN_LATITUDE") {
            r.in_svalue_na();
            if let Some(sun) = light.sun.as_mut() {
                sun.latitude = r.svalue as i32;
            }
        } else if r.is("SUN_LONGITUDE") {
            r.in_svalue_na();
            if let Some(sun) = light.sun.as_mut() {
                sun.longitude = r.svalue as i32;
            }
        } else if r.is("SUN_DAYST") {
            r.in_token();
            if r.is("YES") {
                if let Some(sun) = light.sun.as_mut() {
                    sun.daylight_saving = 1;
                }
            } else if r.is("NO") {
                match light.sun.as_mut() {
                    Some(sun) => sun.daylight_saving = 0,
                    None => return Err(SetupError::NullWindow { keyword: "SUN_DAYST NO (a light without a sun record)" }),
                }
            }
        } else if r.is("SUN_DATE_TIME") {
            r.in_token();
            if let Some(sun) = light.sun.as_mut() {
                sun.date_time = crate::cnum::atoi(r.token());
            }
        } else if r.is("AREA_LIGHT") {
            r.in_svalue_na();
            light.area_light = r.svalue;
        } else if r.is("AREA_TYPE") {
            r.in_svalue_na();
            light.area_type = r.svalue as i32;
        } else if r.is("AREA_U_SAMPLE") {
            r.in_svalue_na();
            light.area_u_sample = r.svalue as i32;
        } else if r.is("AREA_V_SAMPLE") {
            r.in_svalue_na();
            light.area_v_sample = r.svalue as i32;
        } else if r.is("AREA_GEOMETRY") {
            match light.area_type {
                0 => {
                    for k in 0..4 {
                        let mut v = [0.0f32; 3];
                        value3_ascii_read(r, s, Some(&mut v))?;
                        light.area_geometry[3 * k..3 * k + 3].copy_from_slice(&v);
                    }
                    next = false;
                }
                1 => {
                    value_ascii_read(r, s, Some(&mut light.area_geometry[0]), true)?;
                    next = false;
                }
                2 => {
                    let mut v = [0.0f32; 3];
                    value3_ascii_read(r, s, Some(&mut v))?;
                    light.area_geometry[..3].copy_from_slice(&v);
                    value_ascii_read(r, s, Some(&mut light.area_geometry[3]), true)?;
                    next = false;
                }
                3 => {}
                // the token stays `AREA_GEOMETRY` and its handler runs again, for ever
                _ => return Err(SetupError::SpinsForever { at: "AREA_GEOMETRY of an area type other than 0 to 3" }),
            }
        } else if r.is("AREA_LOCAL_COORDS") {
            r.in_svalue_na();
            light.area_local_coords = r.svalue;
        } else if r.is("AREA_SCALE") {
            value3_ascii_read(r, s, Some(&mut light.area_scale))?;
            next = false;
        } else if r.is("AREA_ROTATION") {
            value3_ascii_read(r, s, Some(&mut light.area_rotation))?;
            next = false;
        } else if r.is("AREA_SELECTED") {
            r.in_svalue_na();
            light.area_selected = r.svalue;
        } else if r.is("AREA_ROTATION_MAT") {
            for k in 0..16 {
                value_ascii_read(r, s, Some(&mut light.area_rotation_mat[k]), true)?;
            }
            next = false;
        } else if r.is("DIFFUSE") {
            r.in_svalue_na();
            light.diffuse = (r.svalue != 0) as i16;
        } else if r.is("ENERGY") {
            value3_ascii_read(r, s, Some(&mut light.energy))?;
            next = false;
        } else if r.is("EXPONENT") {
            value_ascii_read(r, s, Some(&mut light.exponent), true)?;
            next = false;
        } else if r.is("CAUSTIC_PHOTON") {
            r.in_token();
            light.caustic_photon = crate::cnum::atoi(r.token());
        } else if r.is("GLOBILLUM_PHOTON") {
            r.in_token();
            light.globillum_photon = crate::cnum::atoi(r.token());
        } else if r.is("CAUSTIC_ENABLE") {
            r.in_token();
            if r.is("YES") {
                light.caustic_enable = 1;
            } else if r.is("NO") {
                light.caustic_enable = 0;
            }
        } else if r.is("GLOBILLUM_ENABLE") {
            r.in_token();
            if r.is("YES") {
                light.globillum_enable = 1;
            } else if r.is("NO") {
                light.globillum_enable = 0;
            }
        } else {
            if first {
                r.in_token();
            }
            return Ok(1);
        }
        if next {
            r.in_token();
        }
        first = false;
    }
}

/// `fcvkeyAsciiRead` (bin2ascii 0x080c6070; the converter's own, not the one of
/// `asceneio.c`): a blank key (`SI_fkeyInitialize` (0x0820bc50), through `si-elemio`'s
/// `Key::new`), then keyword and value pairs from the word in the buffer on. `KEYTYPE` ends
/// the key; `FLAG` ends it only when `SI_versionInput` is at most 2.02 (it is 4.0 in the
/// converter's text path, which never sets it); any other word ends it without reading on.
/// 0 when the file ends first.
fn fcvkey_ascii_read(r: &mut Reader, p: &mut Printed, k: &mut Key, version_input: f32) -> i16 {
    *k = Key::new();
    loop {
        if r.is("TIME") {
            r.in_fvalue_na();
            k.time = r.fvalue;
        } else if r.is("VALUE") {
            r.in_fvalue_na();
            k.value = r.fvalue;
        } else if r.is("LEFT_SLOPE") {
            r.in_fvalue_na();
            k.left_slope = r.fvalue;
        } else if r.is("RIGHT_SLOPE") {
            r.in_fvalue_na();
            k.right_slope = r.fvalue;
        } else if r.is("INTERPOLATION") {
            if !r.in_token() {
                return 0;
            }
            match r.token() {
                b"CONSTANT" => k.interpolation = 0,
                b"LINEAR" => k.interpolation = 1,
                b"SPLINE" => k.interpolation = 2,
                b"SLOPE0" => k.interpolation = 3,
                b"PLATEAU" => k.interpolation = 4,
                t => {
                    let mut m = b"FcvkeyAsciiRead: error, unknown interp token '".to_vec();
                    m.extend_from_slice(t);
                    m.extend_from_slice(b"'.");
                    p.status.push(m);
                }
            }
        } else if r.is("FLAG") {
            r.in_svalue_na();
            k.flag = r.svalue;
            if !(version_input as f64 > 2.02) {
                r.in_token();
                return 1;
            }
        } else if r.is("SPLINE_INDEX") {
            r.in_svalue_na();
            k.spline_index = r.svalue;
        } else if r.is("KEYTYPE") {
            if !r.in_token() {
                return 0;
            }
            match r.token() {
                b"FREEFORM" => k.key_type = 0,
                b"KEYFRAME" => k.key_type = 1,
                b"KEYPATH" => k.key_type = 2,
                b"RAWDATA" => k.key_type = 3,
                t => {
                    let mut m = b"FcvkeyAsciiRead: error, unknown keytype token '".to_vec();
                    m.extend_from_slice(t);
                    m.extend_from_slice(b"'.");
                    p.status.push(m);
                }
            }
            r.in_token();
            return 1;
        } else {
            return 1;
        }
        if !r.in_token() {
            return 0;
        }
    }
}

/// `fcurveAsciiRead` (bin2ascii 0x080c6448): a curve from the word in the buffer
/// (`CURVE_TYPE`) on. The type is the number after the name (the name is skipped); `KEYS`,
/// its count and the keys end the curve, which is then given to
/// `SI_fcvConvertAngleUnitToDegree` (through `si-elemio`): **an angle curve read from text
/// is multiplied by 180/π**, whatever unit the text was in. Any other word ends the curve
/// without handing it out (the item then holds none). `(1, curve)` or `(0, None)` when the
/// file ends.
fn fcurve_ascii_read(r: &mut Reader, p: &mut Printed, default_interpolation: i16, version_input: f32) -> Result<(i16, Option<Fcurve>), SetupError> {
    let mut c = Fcurve::blank(default_interpolation);
    let mut first = true;
    loop {
        if r.is("CURVE_TYPE") {
            if !r.in_token() {
                return Ok((0, None));
            }
            r.in_svalue_na();
            c.curve_type = r.svalue as i32;
        } else if r.is("KEYS") {
            r.in_svalue_na();
            let n = r.svalue as i32;
            if !r.in_token() {
                return Ok((0, None));
            }
            c.count = n;
            if n > 0 {
                if n as usize > r.bytes_left() {
                    return Err(SetupError::CountTooLarge { keyword: "KEYS", count: n });
                }
                for _ in 0..n {
                    let mut k = Key::new();
                    fcvkey_ascii_read(r, p, &mut k, version_input);
                    k.w_14 = 0;
                    c.keys.push(k);
                }
            }
            convert_angle_unit_to_degree(&mut c);
            return Ok((1, Some(c)));
        } else if r.is("PRE_EXTRAPOLATION") {
            r.in_svalue_na();
            c.pre_extrapolation = r.svalue;
        } else if r.is("POST_EXTRAPOLATION") {
            r.in_svalue_na();
            c.post_extrapolation = r.svalue;
        } else if r.is("FLAGS") {
            r.in_svalue_na();
            c.flags = r.svalue;
        } else if r.is("INTERPOLATION") {
            if !r.in_token() {
                return Ok((0, None));
            }
            match r.token() {
                b"CONSTANT" => c.interpolation = 0,
                b"LINEAR" => c.interpolation = 1,
                b"SPLINE" => c.interpolation = 2,
                b"SLOPE0" => c.interpolation = 3,
                b"PLATEAU" => c.interpolation = 4,
                t => {
                    let mut m = b"FcurveAsciiRead: error, unknown interp token '".to_vec();
                    m.extend_from_slice(t);
                    m.extend_from_slice(b"'.");
                    p.status.push(m);
                }
            }
        } else {
            if first {
                r.in_token();
            }
            // the curve is kept (if it has keys) or disposed of, and not handed out
            return Ok((1, None));
        }
        if !r.in_token() {
            return Ok((0, None));
        }
        first = false;
    }
}

/// `staticvalAsciiRead` (bin2ascii 0x080c5f50): a zeroed value, then `TYPE` and `VALUE` up
/// to `SVAL_END` (or the end of the file), which stays in the buffer. Always 1.
fn staticval_ascii_read(r: &mut Reader) -> StaticVal {
    let mut v = StaticVal::default();
    r.in_token();
    while !(r.is("SVAL_END") || r.token().is_empty()) {
        if r.is("TYPE") {
            r.in_svalue_na();
            v.curve_type = r.svalue as i32;
        } else if r.is("VALUE") {
            r.in_fvalue_na();
            v.value = r.fvalue;
        }
        r.in_token();
    }
    v
}

/// `hieranimAsciiRead` (bin2ascii 0x080c5d64): the element list. `HIER_CHAPNAME` gives the
/// chapter's index (MODELS, 8, without it); then `HIER_NAME` and `HIER_FTH` pairs up to
/// `HIER_END` (each `HIER_FTH` adds an element with the name read last), and one more token.
/// The chapter is kept by name here, as `si-elemio`'s value form holds it: the name of that
/// index in the converter's dictionary, `None` outside it.
fn hieranim_ascii_read(r: &mut Reader, ha: &mut HierAnim) -> Result<i32, SetupError> {
    r.in_token();
    let mut chapter = 8; // SI_cid's word for ChapMod
    if r.is("HIER_CHAPNAME") {
        r.in_svalue_na();
        chapter = r.svalue as i32;
        r.in_token();
    }
    ha.chapter = usize::try_from(chapter).ok().and_then(|i| crate::srel::CHAPTER_NAMES.get(i)).map(|n| n.as_bytes().to_vec());
    let mut name: Option<Vec<u8>> = None;
    while !r.is("HIER_END") {
        if r.is("HIER_NAME") {
            name = r.string_read();
        } else if r.is("HIER_FTH") {
            r.in_svalue_na();
            // SI_hieranimAddElem strdup's the name; a null name would fault there
            let n = name.clone().ok_or(SetupError::NullWindow { keyword: "HIER_FTH without a name" })?;
            ha.add_elem(&n, r.svalue as i32);
        }
        if !r.in_token() && r.token().is_empty() {
            // the loop waits for HIER_END and does not test the read
            return Err(SetupError::SpinsForever { at: "the element list (HIER_END)" });
        }
    }
    r.in_token();
    Ok(chapter)
}

/// One item as `readAsciiAnimation` (bin2ascii 0x080c6830) gives it.
struct AsciiItem {
    owner: i32,
    kind: i32,
    act_rel_id: i32,
    flag: i16,
    data: ItemData,
}

/// `readAsciiAnimation` (bin2ascii 0x080c6830): an item's header from the word in the buffer
/// up to `ANIM_END` (`OWNER_ID`, `ITM_TYPE`, `ACT_RELID`, `ACT_OCCID`, `REL_ACTIV`;
/// `SUBNAME` and `SUBID` read and dropped), then for type 0 one more token and a curve, for
/// type 1 a static value. `None` when the header has no `OWNER_ID` (the word in the buffer
/// is `ANIM_END` or the end of the file at once, for instance). `ACT_OCCID` is left where
/// the caller keeps it (`occ`), not reset per item.
fn read_ascii_animation(r: &mut Reader, p: &mut Printed, occ: &mut i32, default_interpolation: i16, version_input: f32) -> Result<Option<AsciiItem>, SetupError> {
    let mut it = AsciiItem { owner: 0, kind: 0, act_rel_id: -1, flag: 1, data: ItemData::None };
    let mut found = false;
    if !(r.is("ANIM_END") || r.token().is_empty()) {
        loop {
            if r.is("OWNER_ID") {
                r.in_svalue_na();
                it.owner = r.svalue as i32;
                found = true;
            } else if r.is("ITM_TYPE") {
                r.in_svalue_na();
                it.kind = r.svalue as i32;
            } else if r.is("SUBNAME") {
                r.string_read();
            } else if r.is("SUBID") {
                r.in_svalue_na();
            } else if r.is("ACT_RELID") {
                r.in_svalue_na();
                it.act_rel_id = r.svalue as i32;
            } else if r.is("ACT_OCCID") {
                r.in_svalue_na();
                *occ = r.svalue as i32;
            } else if r.is("REL_ACTIV") {
                r.in_svalue_na();
                it.flag = r.svalue;
            }
            r.in_token();
            if r.is("ANIM_END") || r.token().is_empty() {
                break;
            }
        }
    }
    if !found {
        return Ok(None);
    }
    match it.kind {
        0 => {
            r.in_token();
            let (ok, c) = fcurve_ascii_read(r, p, default_interpolation, version_input)?;
            if let Some(c) = c {
                it.data = ItemData::Curve(c);
            }
            if ok == 0 {
                return Ok(None);
            }
        }
        1 => it.data = ItemData::Static(staticval_ascii_read(r)),
        _ => {}
    }
    Ok(Some(it))
}

/// `si_elemdictReadAscAnimations` (bin2ascii 0x080c6ab4): a hierarchy animation
/// (`SI_hieranimAllocate`), its element list, then items until the item reader gives none,
/// each added to its owner (`SI_hieranimAddItem`, through `si-elemio`; an owner outside the
/// list drops the item). Also answers the chapter index. Always 1 in the original.
pub(crate) fn elemdict_read_asc_animations(r: &mut Reader, p: &mut Printed, default_interpolation: i16, version_input: f32) -> Result<(HierAnim, i32), SetupError> {
    let mut ha = HierAnim::new();
    let chapter = hieranim_ascii_read(r, &mut ha)?;
    let mut occ = 0i32; // the caller's local: uninitialised for the first item in the original
    while let Some(it) = read_ascii_animation(r, p, &mut occ, default_interpolation, version_input)? {
        ha.add_item(it.owner, it.kind as i16, it.act_rel_id, occ as i16, it.flag, it.data);
    }
    Ok((ha, chapter))
}
