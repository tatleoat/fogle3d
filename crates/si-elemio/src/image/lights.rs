// SPDX-License-Identifier: MIT
//! A new light: `light_fct.c` (the record and its defaults) and `mat_global.c` (the light
//! as the program makes one, with its node in the scene's graph list).

use super::globals::{DEFAULT_NAME, SI_GRAF_LISTS, SI_IDENTITE, SI_PROC_DATA, SUN_PLACE};
use super::off::{light, sun};
use super::outside::{Outside, SI_GRAFLIST_ADD_DATA, TIME};
use si_mem::Mem;

/// The floats `si_lightInitialize` loads from the binary's constants (0x086549f0 …
/// 0x08654a04), as their bits: a spot's cone (30°) and spread (5°) in radians, the fall-off
/// distances and the photon energy, the shadow factor, the shadow fade's start, the
/// caustic exponent.
pub const CONE_BITS: u32 = 0x3f06_0a85;
pub const SPREAD_BITS: u32 = 0x3db2_b884;
pub const TEN_THOUSAND_BITS: u32 = 0x461c_4000;
pub const SHADOW_FACTOR_BITS: u32 = 0x3f40_0000;
pub const HALF_BITS: u32 = 0x3f00_0000;
pub const TWO_BITS: u32 = 0x4000_0000;
const ONE: u32 = 0x3f80_0000;

/// [soft 0x082800fc SI_lightGetGrafType]
/// The kind of graph node a light gets: 8 point, 6 spot (and anything else), 0x13
/// infinite, 0x14 sun.
pub fn light_get_graf_type(m: &Mem, l: u32) -> u32 {
    match m.i16(l + light::TYPE) {
        1 => 8,
        3 => 0x13,
        4 => 0x14,
        _ => 6,
    }
}

/// [soft 0x08516224 SI_matrixCopy]
/// Sixteen 32-bit words, in ascending order.
pub fn matrix_copy(m: &mut Mem, from: u32, to: u32) {
    for k in 0..16u32 {
        let w = m.u32(from.wrapping_add(4 * k));
        m.set_u32(to.wrapping_add(4 * k), w);
    }
}

/// [soft 0x082a6f60 si_initSunDefault]
/// The defaults of a sun's record: the place "Location" (a copy), latitude and longitude
/// 0, daylight saving time on, the current time (`time (0x080c4328)` stores it). A light
/// without a sun record gets a cleared one first.
pub fn init_sun_default(m: &mut Mem, o: &mut dyn Outside, l: u32) {
    if m.u32(l + light::SUN) == 0 {
        let s = m.si_calloc(1, sun::SIZE);
        m.set_u32(l + light::SUN, s);
        if s == 0 {
            return;
        }
    }
    let name = m.strdup(SUN_PLACE);
    let s = m.u32(l + light::SUN);
    m.set_u32(s + sun::NAME, name);
    let s = m.u32(l + light::SUN);
    m.set_u32(s + sun::LATITUDE, 0);
    let s = m.u32(l + light::SUN);
    m.set_u32(s + sun::LONGITUDE, 0);
    let s = m.u32(l + light::SUN);
    m.set_u16(s + sun::DAYST, 1);
    let s = m.u32(l + light::SUN);
    o.call(m, &TIME, &[s + sun::DATE_TIME]);
}

/// [soft 0x082819c8 si_lightInitialize]
/// The defaults of a light of a type (finding 0005 has the table). Nothing for a null
/// record. A sun gets its record (`SI_malloc(0x14)`) and that record's defaults.
pub fn light_initialize(m: &mut Mem, o: &mut dyn Outside, l: u32, kind: i32) {
    if l == 0 {
        return;
    }
    let w = m.u32(DEFAULT_NAME);
    m.set_u32(l, w);
    let w = m.u32(DEFAULT_NAME + 4);
    m.set_u32(l + 4, w);
    m.set_u16(l + light::TYPE, kind as u16);
    m.set_u32(l + light::SUN, 0);
    match kind {
        2 => {
            m.set_u32(l + 0x34, 0);
            m.set_u32(l + 0x38, 0);
            m.set_u32(l + 0x3c, ONE);
        }
        1 => {
            m.set_u32(l + 0x34, 0);
            m.set_u32(l + 0x38, 0);
            m.set_u32(l + 0x3c, 0);
        }
        3 => {
            m.set_u32(l + 0x34, ONE);
            m.set_u32(l + 0x38, ONE);
            m.set_u32(l + 0x3c, ONE);
        }
        4 => {
            m.set_u32(l + 0x34, ONE);
            m.set_u32(l + 0x38, ONE);
            m.set_u32(l + 0x3c, ONE);
            let s = m.si_malloc(sun::SIZE);
            m.set_u32(l + light::SUN, s);
            init_sun_default(m, o, l);
        }
        _ => {}
    }
    for off in [0x24, 0x28, 0x2c] {
        m.set_u32(l + off, ONE);
    }
    for off in [0x30, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c] {
        m.set_u32(l + off, 0);
    }
    m.set_u32(l + light::CONE_ANGLE, CONE_BITS);
    m.set_u32(l + light::SPREAD_ANGLE, SPREAD_BITS);
    m.set_u32(l + 0x78, 0);
    m.set_u32(l + 0x80, 0);
    m.set_u32(l + light::FALL_OFF_START, TEN_THOUSAND_BITS);
    m.set_u32(l + light::FALL_OFF_END, TEN_THOUSAND_BITS);
    m.set_u32(l + light::SHADOW_FACTOR, SHADOW_FACTOR_BITS);
    m.set_u32(l + 0x130, 0);
    m.set_u16(l + 0x176, 1);
    m.set_u32(l + 0x134, 0);
    m.set_u16(l + 0x138, 0);
    m.set_u16(l + light::SHADOW_TYPE, 1);
    m.set_u32(l + light::SHADOW_MAP_RES, 500);
    m.set_u32(l + light::SHADOW_FILTER_SIZE, 10);
    m.set_u32(l + light::SHADOW_FADE_START, HALF_BITS);
    m.set_u32(l + light::SHADOW_FADE_RATE, ONE);
    m.set_u32(l + 0x150, 0);
    m.set_u32(l + 0x154, 0);
    for off in [0x1c4, 0x1c8, 0x1cc] {
        m.set_u32(l + off, ONE);
    }
    for off in [0x1d0, 0x1d4, 0x1d8] {
        m.set_u32(l + off, 0);
    }
    m.set_u32(l + light::SHADOW_FILTER_STEP, 2);
    m.set_u16(l + 0x13c, 0);
    m.set_u32(l + light::NEXT, 0);
    m.set_u32(l + light::SELECTIVE, 0);
    m.set_u32(l + light::INCLUDE, 1);
    m.set_u32(l + 0x164, 0);
    m.set_u32(l + 0x168, 0);
    m.set_u32(l + light::POSITION_MODEL, 0);
    m.set_u32(l + light::INTEREST_MODEL, 0);
    m.set_u16(l + 0x158, 0);
    m.set_u16(l + 0x14c, 1);
    m.set_u32(l + 0x170, 0xffff_ffff);
    m.set_u16(l + 0x174, 1);
    m.set_u16(l + light::AREA_LIGHT, 0);
    m.set_u32(l + light::AREA_TYPE, 3);
    m.set_u32(l + light::AREA_U_SAMPLE, 3);
    m.set_u32(l + light::AREA_V_SAMPLE, 3);
    m.set_u16(l + light::AREA_LOCAL_COORDS, 0);
    m.set_u16(l + light::AREA_SELECTED, 0);
    matrix_copy(m, SI_IDENTITE.addr(), l + light::AREA_ROTATION_MAT);
    m.set_u16(l + light::DIFFUSE, 0);
    m.set_u32(l + light::USER_DATA, 0);
    for off in [0x224, 0x228, 0x22c] {
        m.set_u32(l + off, TEN_THOUSAND_BITS);
    }
    m.set_u32(l + light::EXPONENT, TWO_BITS);
    m.set_u32(l + light::CAUSTIC_PHOTON, 10000);
    m.set_u32(l + light::GLOBILLUM_PHOTON, 10000);
    m.set_u16(l + light::CAUSTIC_ENABLE, 0);
    m.set_u16(l + light::GLOBILLUM_ENABLE, 0);
    m.set_u32(l + light::SHADER, 0);
    m.set_u32(l + light::SHADER2, 0);
}

/// [soft 0x08281d34 SI_lightAllocate]
/// A cleared light record of 0x240 bytes with the defaults of its type; null when there is
/// no memory.
pub fn light_allocate(m: &mut Mem, o: &mut dyn Outside, kind: i32) -> u32 {
    let l = m.si_calloc(1, light::SIZE);
    if l != 0 {
        light_initialize(m, o, l, kind);
    }
    l
}

/// [soft 0x0828014c SI_newLight]
/// A light of a type as the program makes one: the record, and for the types 1 to 4 its
/// node in the scene's graph list (`SI_graflistAddData (0x0832d240)`, of the kind
/// `SI_lightGetGrafType` answers).
pub fn new_light(m: &mut Mem, o: &mut dyn Outside, kind: i32) -> u32 {
    let l = light_allocate(m, o, kind);
    if l == 0 {
        return 0;
    }
    m.set_u16(l + light::TYPE, kind as u16);
    let graf_kind = light_get_graf_type(m, l);
    if (1..=4).contains(&m.i16(l + light::TYPE)) {
        let list = scene_graf_list(m);
        o.call(m, &SI_GRAFLIST_ADD_DATA, &[list, l, graf_kind, 0]);
    }
    m.set_u32(l + light::SHADER, 0);
    m.set_u32(l + light::SHADER2, 0);
    l
}

/// `SI_grafLists + SI_procData->[1] * 0x24`: the scene's graph list.
pub(crate) fn scene_graf_list(m: &Mem) -> u32 {
    let proc_data = m[SI_PROC_DATA];
    let lists = m[SI_GRAF_LISTS];
    lists.wrapping_add(m.u32(proc_data.wrapping_add(4)).wrapping_mul(0x24))
}
