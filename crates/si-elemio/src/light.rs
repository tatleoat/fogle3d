// SPDX-License-Identifier: MIT
//! Light files (`.lig`, magic 0x5380f639): finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `bsceneio.c` (the program's reader and the colour
//! mapping), `b_structio.c` (the writer, and the reader of the stand-alone converter), the
//! element dictionary's file (the functions that open the file) and the light's own files
//! (`si_lightInitialize`, `si_initSunDefault`).
//!
//! Field names: the keywords the vendor's converter `bin2ascii` prints for each field
//! (`SI_lightAsciiWrite`, read from its disassembly).

use crate::io::{fread_u16, fread_u32, Constraint, End, Reader, Udf, UserNode, Writer};
use si_file::{write_bin_header, write_db_header, MAGIC_LIGHT};

/// Light types (`LIGHT_TYPE`).
pub const POINT: i16 = 1;
pub const SPOT: i16 = 2;
pub const INFINITE: i16 = 3;
pub const SUN: i16 = 4;

/// The record a sun light points to (20 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    /// +0x00 `SUN_NAME`: a place name
    pub name: Vec<u8>,
    /// +0x04 `SUN_LATITUDE`
    pub latitude: i32,
    /// +0x08 `SUN_LONGITUDE`
    pub longitude: i32,
    /// +0x0c `SUN_DAYST`: daylight saving time, non-zero `YES`
    pub daylight_saving: i16,
    /// +0x10 `SUN_DATE_TIME`: a C `time_t`
    pub date_time: i32,
}

/// The light record (`light`, 0x240 bytes, allocated zeroed), as far as a light file and
/// `si_lightInitialize` reach into it. Fields whose meaning has not been read are named by
/// their offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// +0x00 `LIGHT_NAME`: 30 bytes inside the record, zero-padded; a name of 30 bytes or
    /// more has no terminator.
    pub name: [u8; 30],
    /// +0x1e `LIGHT_TYPE`: 1 `POINT`, 2 `SPOT`, 3 `INFINITE`, 4 `SUN`
    pub kind: i16,
    /// +0x24 `COLOUR`: red, green, blue
    pub colour: [f32; 3],
    /// +0x30
    pub d_30: i32,
    /// +0x34 `POSITION`
    pub position: [f32; 3],
    /// +0x40 … +0x54: six words, zeroed
    pub d_40: [i32; 6],
    /// +0x58 `INTEREST`
    pub interest: [f32; 3],
    /// +0x64, +0x68, +0x6c: zeroed
    pub d_64: [i32; 3],
    /// +0x70 `CONE_ANGLE` (spot), radians
    pub cone_angle: f32,
    /// +0x74 `SPREAD_ANGLE` (spot), radians
    pub spread_angle: f32,
    /// +0x78, +0x80: zeroed
    pub d_78: i32,
    pub d_80: i32,
    /// +0x84 `FALL_OFF_START`
    pub fall_off_start: f32,
    /// +0x88 `FALL_OFF_END`
    pub fall_off_end: f32,
    /// +0x8c `SHADOW_TYPE`
    pub shadow_type: i16,
    /// +0x90 `SHADOW_FACTOR`
    pub shadow_factor: f32,
    /// +0x118 `SHADOW_MAP_RES`
    pub shadow_map_res: i32,
    /// +0x11c `SHADOW_FILTER_SIZE`
    pub shadow_filter_size: i32,
    /// +0x124 `SHADOW_FILTER_STEP`
    pub shadow_filter_step: i32,
    /// +0x128 `SHADOW_FADE_START`
    pub shadow_fade_start: f32,
    /// +0x12c `SHADOW_FADE_RATE`
    pub shadow_fade_rate: f32,
    /// +0x130, +0x134
    pub d_130: i32,
    pub d_134: i32,
    /// +0x138, +0x13c (16-bit)
    pub w_138: i16,
    pub w_13c: i16,
    /// +0x140
    pub d_140: i32,
    /// +0x144 is a model pointer in the original, filled in later from this
    pub position_constraint: Option<Constraint>,
    /// +0x148 likewise for the interest
    pub interest_constraint: Option<Constraint>,
    /// +0x14c (set to 1 by the initialiser and again when a file's type token is read)
    pub w_14c: i16,
    /// +0x150, +0x154 (zeroed likewise)
    pub d_150: i32,
    pub d_154: i32,
    /// +0x158
    pub w_158: i16,
    /// +0x15c, +0x160 `SELECTIVE`: two numbers
    pub selective: [i32; 2],
    /// +0x164, +0x168
    pub d_164: i32,
    pub d_168: i32,
    /// +0x16c: the sun record of a sun light
    pub sun: Option<Sun>,
    /// +0x170 (set to −1)
    pub d_170: i32,
    /// +0x174, +0x176 (set to 1)
    pub w_174: i16,
    pub w_176: i16,
    /// +0x178 a custom shader
    pub shader: Option<Udf>,
    /// +0x17c a second custom shader
    pub shader2: Option<Udf>,
    /// +0x180 `DIFFUSE`
    pub diffuse: i16,
    /// +0x182 `AREA_LIGHT`
    pub area_light: i16,
    /// +0x184 `AREA_TYPE`: selects how `AREA_GEOMETRY` is stored
    pub area_type: i32,
    /// +0x188 `AREA_U_SAMPLE`
    pub area_u_sample: i32,
    /// +0x18c `AREA_V_SAMPLE`
    pub area_v_sample: i32,
    /// +0x190 `AREA_GEOMETRY`: twelve floats. Type 0 uses all (four points); type 1 the
    /// first; type 2 the first four.
    pub area_geometry: [f32; 12],
    /// +0x1c0 `AREA_LOCAL_COORDS`
    pub area_local_coords: i16,
    /// +0x1c4 `AREA_SCALE`
    pub area_scale: [f32; 3],
    /// +0x1d0 `AREA_ROTATION`
    pub area_rotation: [f32; 3],
    /// +0x1dc `AREA_SELECTED`
    pub area_selected: i16,
    /// +0x1e0 `AREA_ROTATION_MAT`: 4 × 4, row by row
    pub area_rotation_mat: [f32; 16],
    /// +0x220 user data attached by plug-ins
    pub user_data: Vec<UserNode>,
    /// +0x224 `ENERGY`
    pub energy: [f32; 3],
    /// +0x230 `EXPONENT`
    pub exponent: f32,
    /// +0x234 `CAUSTIC_PHOTON`
    pub caustic_photon: i32,
    /// +0x238 `GLOBILLUM_PHOTON`
    pub globillum_photon: i32,
    /// +0x23c `CAUSTIC_ENABLE`
    pub caustic_enable: i16,
    /// +0x23e `GLOBILLUM_ENABLE`
    pub globillum_enable: i16,
}

const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

impl Light {
    /// A zeroed record given its defaults [soft 0x08281d34 SI_lightAllocate]
    /// [soft 0x082819c8 si_lightInitialize]. A sun light gets its sun record
    /// [soft 0x082a6f60 si_initSunDefault], whose date is the current time: `now` here.
    pub fn new(kind: i32, now: i32) -> Light {
        let mut name = [0u8; 30];
        name[..7].copy_from_slice(b"default");
        let (position, sun) = match kind {
            2 => ([0.0, 0.0, 1.0], None),
            1 => ([0.0, 0.0, 0.0], None),
            3 => ([1.0, 1.0, 1.0], None),
            4 => (
                [1.0, 1.0, 1.0],
                Some(Sun { name: b"Location".to_vec(), latitude: 0, longitude: 0, daylight_saving: 1, date_time: now }),
            ),
            _ => ([0.0, 0.0, 0.0], None), // left as allocated: zero
        };
        Light {
            name,
            kind: kind as i16,
            colour: [1.0; 3],
            d_30: 0,
            position,
            d_40: [0; 6],
            interest: [0.0; 3],
            d_64: [0; 3],
            cone_angle: f32::from_bits(0x3f06_0a85),   // 0.523598, 30 degrees
            spread_angle: f32::from_bits(0x3db2_b884), // 0.087266, 5 degrees
            d_78: 0,
            d_80: 0,
            fall_off_start: 10000.0,
            fall_off_end: 10000.0,
            shadow_type: 1,
            shadow_factor: 0.75,
            shadow_map_res: 500,
            shadow_filter_size: 10,
            shadow_filter_step: 2,
            shadow_fade_start: 0.5,
            shadow_fade_rate: 1.0,
            d_130: 0,
            d_134: 0,
            w_138: 0,
            w_13c: 0,
            d_140: 0,
            position_constraint: None,
            interest_constraint: None,
            w_14c: 1,
            d_150: 0,
            d_154: 0,
            w_158: 0,
            selective: [0, 1],
            d_164: 0,
            d_168: 0,
            sun,
            d_170: -1,
            w_174: 1,
            w_176: 1,
            shader: None,
            shader2: None,
            diffuse: 0,
            area_light: 0,
            area_type: 3,
            area_u_sample: 3,
            area_v_sample: 3,
            area_geometry: [0.0; 12],
            area_local_coords: 0,
            area_scale: [1.0; 3],
            area_rotation: [0.0; 3],
            area_selected: 0,
            area_rotation_mat: IDENTITY, // SI_matrixCopy from SI_identite (0x086dc160)
            user_data: Vec::new(),
            energy: [10000.0; 3],
            exponent: 2.0,
            caustic_photon: 10000,
            globillum_photon: 10000,
            caustic_enable: 0,
            globillum_enable: 0,
        }
    }

    /// The name up to its terminator, or all 30 bytes.
    pub fn name_bytes(&self) -> &[u8] {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(30);
        &self.name[..n]
    }

    /// `strncpy(light, name, 30)`
    pub fn set_name(&mut self, name: &[u8]) {
        let n = name.iter().position(|&b| b == 0).unwrap_or(name.len()).min(30);
        self.name = [0; 30];
        self.name[..n].copy_from_slice(&name[..n]);
    }

    /// The kind of display handle a light gets [soft 0x082800fc SI_lightGetGrafType]:
    /// 8 point, 6 spot (and anything unknown), 0x13 infinite, 0x14 sun.
    pub fn graf_type(&self) -> i32 {
        match self.kind {
            1 => 8,
            3 => 0x13,
            4 => 0x14,
            _ => 6,
        }
    }
}

/// What the older files say about the light's display handle (`__SI_GrafNode`), which the
/// program's reader applies to the handle it finds for the new light. The writer has no
/// tokens for them. Each is the raw value from the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightDisplay {
    /// Token 12: a line colour, stored into the handle's +0x12 through [`map_line_colour`].
    pub line_colour: Option<i16>,
    /// Token 13: stored into the handle's +0x0c.
    pub d_0c: Option<i16>,
    /// Token 14: stored into the handle's +0x08; the values 1 and 3 also make the handle the
    /// active one of its list.
    pub d_08: Option<i16>,
    /// Token 0x16: two numbers, stored into +0x12 through [`map_index_to_wire_colour`].
    pub wire_colour: Option<(i16, i16)>,
}

/// The display's colour layout, which the two mappings consult (globals of the display code).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColourMap {
    /// `SI_bitPlanes` (0x08781350)
    pub bit_planes: i32,
    /// `SI_select1WM` (0x0878162c), `SI_select7WM` (0x08781638), `SI_unselectWM` (0x087817c4)
    pub select1_wm: u16,
    pub select7_wm: u16,
    pub unselect_wm: u16,
    /// `SI_color1` (0x08781354), `SI_fcurveColor3` (0x08781460)
    pub color1: u16,
    pub fcurve_color3: u16,
}

/// An old file's line colour as an index of the current display
/// [soft 0x083f91c0 si_mapLineColour] (its second parameter is not used).
pub fn map_line_colour(c: i16, m: &ColourMap) -> i16 {
    let in_select = |v: i16| (v as i32) >= m.select1_wm as i32 && (v as i32) <= m.select7_wm as i32;
    if m.bit_planes == 8 {
        if (c as i32) > m.select7_wm as i32 + 1 {
            let v = (c as u16).wrapping_add(0xfe16).wrapping_add(m.color1).wrapping_add(m.select7_wm) as i16;
            if in_select(v) { v } else { m.select7_wm as i16 }
        } else if c == 0xf {
            m.select7_wm as i16
        } else {
            c
        }
    } else {
        let mut hi = (c as u16 & m.select7_wm) as i16;
        let mut lo = (c as u16 & m.unselect_wm) as i16;
        if !in_select(hi) {
            hi = m.select7_wm as i16;
        }
        if (lo as i32) < m.color1 as i32 || (lo as i32) > m.fcurve_color3 as i32 {
            lo = m.color1 as i16;
        }
        lo | hi
    }
}

/// A wire colour index as a colour of the current display
/// [soft 0x083f9278 SI_mapIndexToWireColour].
pub fn map_index_to_wire_colour(a: i16, b: i16, m: &ColourMap) -> i16 {
    if m.bit_planes == 8 {
        // the original adds 2 in a loop, `b` times
        let twice = if b > 0 { (b as u16).wrapping_mul(2) } else { 0 };
        twice.wrapping_add(m.color1) as i16
    } else {
        (b as u16).wrapping_mul(0x40).wrapping_add(a as u16).wrapping_add(m.color1) as i16
    }
}

/// The reverse [soft 0x083f92bc SI_mapWireToIndexColour]: with `which` zero the second number
/// of [`map_index_to_wire_colour`], otherwise the first (zero on an 8-plane display).
pub fn map_wire_to_index_colour(c: i16, which: i16, m: &ColourMap) -> i16 {
    if m.bit_planes == 8 {
        if which == 0 && c > 1 {
            // the original counts how often 2 can be taken off while more than 1 remains
            c / 2
        } else {
            0
        }
    } else if which == 0 {
        ((c as u16 & m.select7_wm) as i16) >> 6
    } else {
        (c as u16 & m.unselect_wm).wrapping_sub(m.color1) as i16
    }
}

pub use crate::camera::Variant;

/// What a light token stream gave.
#[derive(Clone, Debug, PartialEq)]
pub struct LightRead {
    /// The light, created when the type token (2) is read. `None` if there was none.
    pub light: Option<Light>,
    /// Display attributes from old files (program's reader only).
    pub display: LightDisplay,
    pub end: End,
    /// The stream held a field for the light before the type token, or a sun field for a
    /// light that is not a sun, at a spot where the original does not check its pointer: it
    /// would fault there. Here the field is read and dropped.
    pub fault: bool,
}

/// Reads the token stream of a light [soft 0x083f7ce8 SI_lightBinaryRead]
/// (`Variant::Program`) or [soft 0x083eaa88 b2aLightBinaryRead] (`Variant::Converter`).
///
/// | token | field |
/// |---|---|
/// | 0 | end |
/// | 1 | string: `LIGHT_NAME` (kept until the type token arrives) |
/// | 2 | 16-bit: `LIGHT_TYPE`. Creates the light with its defaults and gives it the name |
/// | 3 | three values: `COLOUR` |
/// | 4 | value: `FALL_OFF_START` |
/// | 5 | value: `FALL_OFF_END` |
/// | 6 | value: `SHADOW_FACTOR` |
/// | 7 | position: `POSITION` |
/// | 8 | position: `INTEREST` |
/// | 9 | value: `CONE_ANGLE` |
/// | 10 | value: `SPREAD_ANGLE` |
/// | 11, 15 | value, read and dropped |
/// | 12 | 16-bit: line colour of the display handle (program's reader; the converter drops it) |
/// | 13 | 16-bit: display handle +0x0c (likewise) |
/// | 14 | 16-bit: display handle +0x08 (likewise) |
/// | 16 | 16-bit: `SHADOW_TYPE` |
/// | 17 | 16-bit: `SHADOW_MAP_RES` |
/// | 18 | 16-bit: `SHADOW_FILTER_SIZE` |
/// | 19 | 16-bit: `SHADOW_FILTER_STEP` |
/// | 20 | value: `SHADOW_FADE_START` |
/// | 21 | value: `SHADOW_FADE_RATE` |
/// | 22 | two 16-bit: wire colour of the display handle (likewise) |
/// | 23 | two 16-bit: `SELECTIVE` |
/// | 24 | string: `SUN_NAME` |
/// | 25 | 32-bit: `SUN_LATITUDE` |
/// | 26 | 32-bit: `SUN_LONGITUDE` |
/// | 27 | 16-bit: `SUN_DAYST` |
/// | 28 | 32-bit: `SUN_DATE_TIME` |
/// | 29 | custom shader ([`Reader::udf`]) |
/// | 30 | 32-bit, kept as 16: `AREA_LIGHT` |
/// | 31 | 32-bit: `AREA_TYPE` |
/// | 32 | 32-bit: `AREA_U_SAMPLE` |
/// | 33 | 32-bit: `AREA_V_SAMPLE` |
/// | 34 | `AREA_GEOMETRY`, by the area type read so far: 0 four value triples; 1 one value; 2 a value triple and a value; anything else nothing |
/// | 35 | 32-bit, kept as 16: `AREA_LOCAL_COORDS` |
/// | 36 | three values: `AREA_SCALE` |
/// | 37 | three values: `AREA_ROTATION` |
/// | 38 | sixteen values: `AREA_ROTATION_MAT` |
/// | 39 | 32-bit, kept as 16: `AREA_SELECTED` |
/// | 40 | 32-bit, kept as "is not zero": `DIFFUSE` |
/// | 41 | second custom shader |
/// | 42, 0x102a | three values: `ENERGY` |
/// | 43, 0x042b | value: `EXPONENT` |
/// | 44, 0x042c | 32-bit: `CAUSTIC_PHOTON` |
/// | 45, 0x042d | 32-bit: `GLOBILLUM_PHOTON` |
/// | 46, 0x022e | 16-bit: `CAUSTIC_ENABLE` |
/// | 47, 0x022f | 16-bit: `GLOBILLUM_ENABLE` |
/// | 0xff28 | 32-bit size, then a user-data list |
/// | other | skipped by the token's size byte |
///
/// The writer puts the second custom shader under token 0xff29, but this reader does not
/// know that token: its `case` label is the constant −215 (0xffffff29) and the token is
/// widened without sign, so the two never compare equal. The field is stepped over by its
/// size, and a second shader written by this build is lost when the file is read back.
///
/// `now` is the time a new sun record is stamped with (see [`Light::new`]).
pub fn read_tokens(r: &mut Reader, variant: Variant, now: i32) -> LightRead {
    let mut out = LightRead { light: None, display: LightDisplay::default(), end: End::Zero, fault: false };
    // the name string read last (one local variable): the light's name until the type token takes
    // it, later the sun's name
    let mut pending_name: Option<Vec<u8>> = None;
    loop {
        let ok = match variant {
            Variant::Program => fread_u16(&mut r.s, &mut r.st.token_bsceneio),
            Variant::Converter => fread_u16(&mut r.s, &mut r.st.token_bstructio),
        };
        let token = match variant {
            Variant::Program => r.st.token_bsceneio,
            Variant::Converter => r.st.token_bstructio,
        };
        r.note("light", token, ok);
        if token == 0 {
            out.end = End::Zero;
            return out;
        }
        if !ok {
            out.end = End::Eof;
            return out;
        }
        let program = variant == Variant::Program;
        match token {
            1 => pending_name = Some(r.string()),
            2 => {
                let kind = r.svalue();
                // the program's reader makes the light with `SI_newLight` (0x0828014c), which
                // also enters it in the display list of the current window; that list is
                // not modelled
                let mut l = Light::new(kind as i32, now);
                l.kind = kind;
                l.w_14c = 1;
                l.d_150 = 0;
                l.d_154 = 0;
                if let Some(n) = pending_name.take() {
                    l.set_name(&n);
                }
                // a second type token replaces the light; the original leaks the first
                out.light = Some(l);
                out.display = LightDisplay::default();
            }
            3 => r.value3(out.light.as_mut().map(|l| &mut l.colour)),
            4 => r.value(out.light.as_mut().map(|l| &mut l.fall_off_start)),
            5 => r.value(out.light.as_mut().map(|l| &mut l.fall_off_end)),
            6 => r.value(out.light.as_mut().map(|l| &mut l.shadow_factor)),
            7 | 8 => {
                let graf_type = if token == 7 { 6 } else { 7 };
                match (out.light.as_mut(), variant) {
                    (Some(l), Variant::Program) => {
                        let (v, c) = if token == 7 {
                            (&mut l.position, &mut l.position_constraint)
                        } else {
                            (&mut l.interest, &mut l.interest_constraint)
                        };
                        if let Some(found) = r.old_position(Some(v), graf_type) {
                            *c = Some(found);
                        }
                    }
                    (Some(l), Variant::Converter) => r.position(Some(if token == 7 { &mut l.position } else { &mut l.interest })),
                    // without a light the original passes null pointers and kind 0; a model
                    // reference read then goes on the constraint list with a null owner
                    (None, Variant::Program) => {
                        r.old_position(None, 0);
                    }
                    (None, Variant::Converter) => r.position(None),
                }
            }
            9 => r.value(out.light.as_mut().map(|l| &mut l.cone_angle)),
            10 => r.value(out.light.as_mut().map(|l| &mut l.spread_angle)),
            11 | 15 => r.value(None),
            12 => {
                let v = r.svalue();
                if program && out.light.is_some() {
                    out.display.line_colour = Some(v);
                }
            }
            13 => {
                let v = r.svalue();
                if program && out.light.is_some() {
                    out.display.d_0c = Some(v);
                }
            }
            14 => {
                let v = r.svalue();
                if program && out.light.is_some() {
                    out.display.d_08 = Some(v);
                }
            }
            16 => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.shadow_type = v;
                }
            }
            17 => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.shadow_map_res = v as i32;
                }
            }
            18 => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.shadow_filter_size = v as i32;
                }
            }
            19 => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.shadow_filter_step = v as i32;
                }
            }
            20 => r.value(out.light.as_mut().map(|l| &mut l.shadow_fade_start)),
            21 => r.value(out.light.as_mut().map(|l| &mut l.shadow_fade_rate)),
            22 => {
                let a = r.svalue();
                let b = r.svalue();
                if program && out.light.is_some() {
                    out.display.wire_colour = Some((a, b));
                }
            }
            23 => {
                let a = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.selective[0] = a as i32;
                }
                let b = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.selective[1] = b as i32;
                }
            }
            24 => {
                // The string becomes that "last name" variable as well as the sun's name, and it is
                // freed when the reader returns: the original leaves the sun record pointing
                // at freed memory. Here the sun keeps the string.
                let s = r.string();
                pending_name = Some(s.clone());
                if let Some(l) = out.light.as_mut() {
                    match l.sun.as_mut() {
                        Some(sun) => sun.name = s,
                        None => out.fault = true,
                    }
                }
            }
            25 | 26 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    match l.sun.as_mut() {
                        Some(sun) if token == 25 => sun.latitude = v,
                        Some(sun) => sun.longitude = v,
                        None => out.fault = true,
                    }
                }
            }
            27 => {
                // no check at all in the original, not even for the light
                let v = r.svalue();
                match out.light.as_mut().and_then(|l| l.sun.as_mut()) {
                    Some(sun) => sun.daylight_saving = v,
                    None => out.fault = true,
                }
            }
            28 => {
                let v = r.lvalue();
                if let Some(l) = out.light.as_mut() {
                    match l.sun.as_mut() {
                        Some(sun) => sun.date_time = v,
                        None => out.fault = true,
                    }
                }
            }
            29 | 41 => {
                // stored without a check for the light; the earlier shader is not freed
                let u = r.udf();
                match out.light.as_mut() {
                    Some(l) if token == 29 => l.shader = u,
                    Some(l) => l.shader2 = u,
                    None => out.fault = true,
                }
            }
            30 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_light = v as i16;
                }
            }
            31 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_type = v;
                }
            }
            32 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_u_sample = v;
                }
            }
            33 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_v_sample = v;
                }
            }
            34 => match out.light.as_mut() {
                // the area type is taken from the light without a check
                None => out.fault = true,
                Some(l) => match l.area_type {
                    0 => {
                        for i in 0..4 {
                            let mut v = [0.0f32; 3];
                            v.copy_from_slice(&l.area_geometry[3 * i..3 * i + 3]);
                            r.value3(Some(&mut v));
                            l.area_geometry[3 * i..3 * i + 3].copy_from_slice(&v);
                        }
                    }
                    1 => r.value(Some(&mut l.area_geometry[0])),
                    2 => {
                        let mut v = [l.area_geometry[0], l.area_geometry[1], l.area_geometry[2]];
                        r.value3(Some(&mut v));
                        l.area_geometry[..3].copy_from_slice(&v);
                        r.value(Some(&mut l.area_geometry[3]));
                    }
                    _ => {}
                },
            },
            35 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_local_coords = v as i16;
                }
            }
            36 => r.value3(out.light.as_mut().map(|l| &mut l.area_scale)),
            37 => r.value3(out.light.as_mut().map(|l| &mut l.area_rotation)),
            38 => {
                // the program's reader does not check for the light here; the converter's does
                if out.light.is_none() && program {
                    out.fault = true;
                }
                for i in 0..16 {
                    r.value(out.light.as_mut().map(|l| &mut l.area_rotation_mat[i]));
                }
            }
            39 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.area_selected = v as i16;
                }
            }
            40 => {
                let v = r.ivalue();
                if let Some(l) = out.light.as_mut() {
                    l.diffuse = (v != 0) as i16;
                }
            }
            42 | 0x102a => r.value3(out.light.as_mut().map(|l| &mut l.energy)),
            43 | 0x042b => r.value(out.light.as_mut().map(|l| &mut l.exponent)),
            44 | 0x042c => {
                let v = r.lvalue();
                if let Some(l) = out.light.as_mut() {
                    l.caustic_photon = v;
                }
            }
            45 | 0x042d => {
                let v = r.lvalue();
                if let Some(l) = out.light.as_mut() {
                    l.globillum_photon = v;
                }
            }
            46 | 0x022e => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.caustic_enable = v;
                }
            }
            47 | 0x022f => {
                let v = r.svalue();
                if let Some(l) = out.light.as_mut() {
                    l.globillum_enable = v;
                }
            }
            0xff28 => {
                let mut size = 0u32;
                fread_u32(&mut r.s, &mut size);
                let nodes = r.user_data();
                match out.light.as_mut() {
                    Some(l) => l.user_data = nodes,
                    None => out.fault = true,
                }
            }
            _ => {
                r.skip_field(token);
            }
        }
    }
}

/// Writes the token stream of a light [soft 0x083e9d58 SI_lightBinaryWrite] (its third
/// parameter, a display handle, is not used).
///
/// In order: name (1), type (2), colour (3), fall-off start and end (4, 5), shadow factor
/// (6), position and interest (7, 8, always as vectors); for a spot the two angles (9, 10);
/// the shadow fields (16 to 21); `SELECTIVE` (23); for a sun the five sun fields (24 to 28).
/// If the output version is above 2.65: the custom shader (29), the area light fields (30 to
/// 37, then 39 before 38) and `DIFFUSE` (40); and, unless the output version is below
/// 3.8109, the photon fields under their sized tokens (0x102a, 0x042b, 0x042c, 0x042d,
/// 0x022e, 0x022f). Then the user data (0xff28) if the light has any, the second custom
/// shader (0xff29, with its byte size) if there is one and the version is not below 3.8109,
/// and the zero token.
///
/// The name is written as a C string starting at the record's first byte. A name that fills
/// all 30 bytes has no terminator there, so the original runs on into the type field: on the
/// little-endian builds one more byte, the type's low byte, is written before the type's
/// zero high byte ends the string. That is reproduced.
///
/// A sun light must have its sun record (the original dereferences the pointer unchecked);
/// one without is written here with an empty name and zeros.
pub fn write_tokens(w: &mut Writer, l: &Light) {
    w.token_bstructio(1);
    let mut name = l.name_bytes().to_vec();
    if name.len() == 30 {
        let k = l.kind.to_le_bytes();
        // beyond the type's two bytes lie two bytes this record does not model; they are
        // taken as zero, which ends the string
        name.extend(k.iter().take_while(|&&b| b != 0));
    }
    w.string(Some(&name));
    w.token_bstructio(2);
    w.svalue(l.kind);
    w.token_bstructio(3);
    w.value3(&l.colour);
    w.token_bstructio(4);
    w.value(l.fall_off_start);
    w.token_bstructio(5);
    w.value(l.fall_off_end);
    w.token_bstructio(6);
    w.value(l.shadow_factor);
    w.token_bstructio(7);
    w.position(&l.position);
    w.token_bstructio(8);
    w.position(&l.interest);
    if l.kind == SPOT {
        w.token_bstructio(9);
        w.value(l.cone_angle);
        w.token_bstructio(10);
        w.value(l.spread_angle);
    }
    w.token_bstructio(16);
    w.svalue(l.shadow_type);
    w.token_bstructio(17);
    w.svalue(l.shadow_map_res as i16);
    w.token_bstructio(18);
    w.svalue(l.shadow_filter_size as i16);
    w.token_bstructio(19);
    w.svalue(l.shadow_filter_step as i16);
    w.token_bstructio(20);
    w.value(l.shadow_fade_start);
    w.token_bstructio(21);
    w.value(l.shadow_fade_rate);
    w.token_bstructio(23);
    w.svalue(l.selective[0] as i16);
    w.svalue(l.selective[1] as i16);
    if l.kind == SUN {
        let empty = Sun { name: Vec::new(), latitude: 0, longitude: 0, daylight_saving: 0, date_time: 0 };
        let sun = l.sun.as_ref().unwrap_or(&empty);
        w.token_bstructio(24);
        w.string(Some(&sun.name));
        w.token_bstructio(25);
        w.ivalue(sun.latitude);
        w.token_bstructio(26);
        w.ivalue(sun.longitude);
        w.token_bstructio(27);
        w.svalue(sun.daylight_saving);
        w.token_bstructio(28);
        w.lvalue(sun.date_time);
    }
    // the float version against double constants (0x0865bbf8, 0x0865bc00)
    let version = w.version_output as f64;
    if version > 2.65 {
        w.token_bstructio(29);
        w.udf(l.shader.as_ref());
        w.token_bstructio(30);
        w.ivalue(l.area_light as i32);
        w.token_bstructio(31);
        w.ivalue(l.area_type);
        w.token_bstructio(32);
        w.ivalue(l.area_u_sample);
        w.token_bstructio(33);
        w.ivalue(l.area_v_sample);
        w.token_bstructio(34);
        match l.area_type {
            1 => w.value(l.area_geometry[0]),
            0 => {
                for i in 0..4 {
                    w.value3(&[l.area_geometry[3 * i], l.area_geometry[3 * i + 1], l.area_geometry[3 * i + 2]]);
                }
            }
            2 => {
                w.value3(&[l.area_geometry[0], l.area_geometry[1], l.area_geometry[2]]);
                w.value(l.area_geometry[3]);
            }
            _ => {}
        }
        w.token_bstructio(35);
        w.ivalue(l.area_local_coords as i32);
        w.token_bstructio(36);
        w.value3(&l.area_scale);
        w.token_bstructio(37);
        w.value3(&l.area_rotation);
        w.token_bstructio(39);
        w.ivalue(l.area_selected as i32);
        w.token_bstructio(38);
        for &m in &l.area_rotation_mat {
            w.value(m);
        }
        w.token_bstructio(40);
        w.ivalue(l.diffuse as i32);
        if !(version < 3.8109) {
            w.token_bstructio(0x102a);
            w.value3(&l.energy);
            w.token_bstructio(0x042b);
            w.value(l.exponent);
            w.token_bstructio(0x042c);
            w.lvalue(l.caustic_photon);
            w.token_bstructio(0x042d);
            w.lvalue(l.globillum_photon);
            w.token_bstructio(0x022e);
            w.svalue(l.caustic_enable);
            w.token_bstructio(0x022f);
            w.svalue(l.globillum_enable);
        }
    }
    if !l.user_data.is_empty() {
        w.token_bstructio(0xff28);
        w.user_data(&l.user_data);
    }
    if !(version < 3.8109) {
        if let Some(u) = l.shader2.as_ref() {
            w.token_bstructio(0xff29);
            // the size of what follows: the name with its terminator and three integers
            let n = u.name.iter().position(|&b| b == 0).unwrap_or(u.name.len());
            w.put_i32(13 + n as i32);
            w.udf(Some(u));
        }
    }
    w.token_bstructio(0);
}

/// What reading a light file gives besides the light.
#[derive(Clone, Debug, PartialEq)]
pub struct LightFile {
    /// The header's version, −1.0 if the header was not accepted.
    pub version: f32,
    pub comment: Vec<u8>,
    pub read: LightRead,
    /// Offset of the first byte not read.
    pub pos: usize,
    pub clamped: bool,
}

/// Reads a light file the way the program loads one [soft 0x0810fe78 SI_elemdictReadLight]:
/// the header, then the token stream. (The original hands the reader an uninitialised light
/// pointer; a stream whose first light field precedes its type token would write through it.
/// Here there is no light until the type token.)
pub fn read_file(r: &mut Reader, now: i32) -> LightFile {
    read_file_as(r, Variant::Program, now)
}

/// Reads a light file the way the converter's entry point does
/// [soft 0x083ec348 SI_lightReadBinaryFile].
pub fn read_file_converter(r: &mut Reader, now: i32) -> LightFile {
    read_file_as(r, Variant::Converter, now)
}

fn read_file_as(r: &mut Reader, variant: Variant, now: i32) -> LightFile {
    let (version, comment) = crate::io::read_header(r);
    let read = read_tokens(r, variant, now);
    LightFile { version, comment, read, pos: r.s.tell(), clamped: r.clamped }
}

/// A light file as the program saves it [soft 0x08110570 SI_elemdictWriteLight].
pub fn write_file(w: &mut Writer, l: &Light) {
    write_db_header(&mut w.out, w.version_output, MAGIC_LIGHT);
    write_tokens(w, l);
}

/// A light file as the converter's entry point writes it
/// [soft 0x083ec274 SI_lightWriteBinaryFile]: the header's version is the caller's.
pub fn write_file_converter(w: &mut Writer, version: f32, l: &Light) {
    write_bin_header(&mut w.out, MAGIC_LIGHT, version, None);
    write_tokens(w, l);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: i32) -> Light {
        let mut l = Light::new(kind, 1_000_000);
        l.set_name(b"key");
        l.colour = [0.5, 0.25, 1.0];
        l.position = [1.0, 2.0, 3.0];
        l.interest = [-1.0, 0.0, 4.0];
        l.selective = [1, 0];
        l.shader = Some(Udf { name: b"flare".to_vec(), w0c: 1, w10: 2, d14: 3 });
        l.area_light = 1;
        l.diffuse = 1;
        l.caustic_enable = 1;
        l
    }

    #[test]
    fn defaults() {
        let l = Light::new(2, 0);
        assert_eq!(l.name_bytes(), b"default");
        assert_eq!(l.position, [0.0, 0.0, 1.0]);
        assert!((l.cone_angle - 30f32.to_radians()).abs() < 1e-6);
        assert!((l.spread_angle - 5f32.to_radians()).abs() < 1e-6);
        assert_eq!(l.area_rotation_mat[5], 1.0);
        assert_eq!(Light::new(4, 77).sun.unwrap().date_time, 77);
        assert_eq!(Light::new(3, 0).position, [1.0; 3]);
        assert_eq!((Light::new(1, 0).graf_type(), Light::new(2, 0).graf_type(), Light::new(4, 0).graf_type()), (8, 6, 0x14));
    }

    #[test]
    fn round_trip_every_type_and_area_form() {
        for kind in 1..=4 {
            for area_type in 0..=3 {
                let mut l = sample(kind);
                l.area_type = area_type;
                for (i, g) in l.area_geometry.iter_mut().enumerate() {
                    // only the floats the area type stores survive
                    let stored = match area_type { 0 => 12, 1 => 1, 2 => 4, _ => 0 };
                    *g = if i < stored { i as f32 + 0.5 } else { 0.0 };
                }
                let mut w = Writer::new();
                write_file(&mut w, &l);
                let mut r = Reader::new(&w.out);
                let f = read_file(&mut r, 1_000_000);
                assert_eq!((f.read.end, f.pos, f.read.fault), (End::Zero, w.out.len(), false));
                assert_eq!(f.read.light.as_ref(), Some(&l), "kind {kind} area {area_type}");
                let f = read_file_converter(&mut Reader::new(&w.out), 1_000_000);
                assert_eq!(f.read.light.as_ref(), Some(&l));
            }
        }
    }

    #[test]
    fn version_branches() {
        let l = sample(2);
        let len = |v: f32| {
            let mut w = Writer::with_version(v);
            write_tokens(&mut w, &l);
            w.out.len()
        };
        let full = len(4.0);
        // the six photon fields: 2+14, 2+6, 2+4, 2+4, 2+2, 2+2
        assert_eq!(full - len(3.8), 44);
        // 2.65 as a float is above the double 2.65: the area fields are written
        assert_eq!(len(2.65), len(3.8));
        assert!(len(2.64) < len(2.65));
        // the same tokens are read back under their short form
        let mut w = Writer::new();
        write_tokens(&mut w, &l);
        for (sized, plain) in [(0x102au16, 42u16), (0x042b, 43), (0x042c, 44), (0x042d, 45), (0x022e, 46), (0x022f, 47)] {
            let at = w.out.windows(2).rposition(|x| x == sized.to_be_bytes()).unwrap();
            w.out[at..at + 2].copy_from_slice(&plain.to_be_bytes());
        }
        assert_eq!(read_tokens(&mut Reader::new(&w.out), Variant::Program, 1_000_000).light, Some(l));
    }

    #[test]
    fn the_second_shader_is_written_but_not_read_back() {
        let mut l = sample(1);
        l.shader2 = Some(Udf { name: b"emit".to_vec(), w0c: 0, w10: 0, d14: 9 });
        let mut w = Writer::new();
        write_tokens(&mut w, &l);
        let at = w.out.windows(2).position(|x| x == [0xff, 0x29]).unwrap();
        assert_eq!(&w.out[at + 2..at + 6], &[0, 0, 0, 17]);
        let got = read_tokens(&mut Reader::new(&w.out), Variant::Program, 1_000_000);
        assert_eq!(got.end, End::Zero);
        let mut expect = l.clone();
        expect.shader2 = None;
        assert_eq!(got.light, Some(expect));
        // under the unsized token 41 it is read
        let mut b = w.out[..at].to_vec();
        b.extend_from_slice(&[0, 41]);
        b.extend_from_slice(&w.out[at + 6..]);
        assert_eq!(read_tokens(&mut Reader::new(&b), Variant::Program, 1_000_000).light, Some(l));
    }

    #[test]
    fn a_thirty_byte_name_runs_into_the_type() {
        let mut l = sample(2);
        l.set_name(&[b'n'; 40]);
        assert_eq!(l.name_bytes().len(), 30);
        let mut w = Writer::new();
        write_tokens(&mut w, &l);
        assert_eq!(&w.out[2..34], &[[b'n'; 30].as_slice(), &[2, 0]].concat()[..]);
        let got = read_tokens(&mut Reader::new(&w.out), Variant::Program, 0).light.unwrap();
        assert_eq!(got.name, [b'n'; 30]);
    }

    #[test]
    fn display_tokens_and_faults() {
        // name, type, then the three old display tokens and the wire colour
        let mut b = vec![0, 1, b'a', 0, 0, 2, 0, 1, 0, 12, 0, 5, 0, 13, 0, 6, 0, 14, 0, 3, 0, 22, 0, 1, 0, 2, 0, 0];
        let got = read_tokens(&mut Reader::new(&b), Variant::Program, 0);
        assert_eq!(got.display, LightDisplay { line_colour: Some(5), d_0c: Some(6), d_08: Some(3), wire_colour: Some((1, 2)) });
        assert_eq!(read_tokens(&mut Reader::new(&b), Variant::Converter, 0).display, LightDisplay::default());
        // a sun field for a point light
        b.truncate(8);
        b.extend_from_slice(&[0, 27, 0, 1, 0, 0]);
        let got = read_tokens(&mut Reader::new(&b), Variant::Program, 0);
        assert!(got.fault);
        assert_eq!(got.end, End::Zero);
    }

    #[test]
    fn colour_maps() {
        let m = ColourMap { bit_planes: 24, select1_wm: 0x40, select7_wm: 0x1c0, unselect_wm: 0x3f, color1: 8, fcurve_color3: 30 };
        assert_eq!(map_index_to_wire_colour(3, 2, &m), 0x80 + 3 + 8);
        assert_eq!(map_wire_to_index_colour(0x80 + 11, 0, &m), 2);
        assert_eq!(map_wire_to_index_colour(0x80 + 11, 1, &m), 3);
        assert_eq!(map_line_colour(0x80 + 11, &m), 0x80 + 11);
        // out-of-range parts fall back
        assert_eq!(map_line_colour(1, &m), 0x1c0 | 8);
        let m8 = ColourMap { bit_planes: 8, ..m };
        assert_eq!(map_index_to_wire_colour(3, 2, &m8), 4 + 8);
        assert_eq!(map_wire_to_index_colour(5, 0, &m8), 2);
        assert_eq!(map_wire_to_index_colour(5, 1, &m8), 0);
        assert_eq!(map_line_colour(0xf, &m8), 0x1c0);
        assert_eq!(map_line_colour(7, &m8), 7);
    }
}
