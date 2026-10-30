// SPDX-License-Identifier: MIT
//! `bsceneio.c`: the program's own readers of a camera, a light and a lattice, with the
//! position reader and the two old colour mappings they use.
//!
//! All three readers are loops over 16-bit tokens kept in the file's static `token`
//! ([`TOKEN_BSCENEIO`]), closed by token 0. A token read that fails leaves the variable as
//! it was; when that is not 0 the original runs the same handler again without end, and
//! the port stops there ([`NoReturn::Endless`]).

use super::byteswap::byteswap_fread;
use super::globals::*;
use super::lights::{new_light, scene_graf_list};
use super::off::{camera, cnstr, graf_node, lattice, light, prototypes, sun};
use super::outside::{Outside, SI_ED_ADD_LATTICE, SI_FIND_GL_DATA, SI_GRAFLIST_SET_ACTIVE, SI_MODEL_ADD_I_LATTICE, SI_STATUS_DISPLAY_MESSAGE};
use super::shader::{udf_read, user_data_read};
use super::utilio::{file_skip_field, string_binary_read, value3_binary_read, value_binary_read, vector_binary_read};
use super::{faults, NoReturn, Run, COUNT_LIMIT};
use si_mem::Mem;

/// [soft 0x083f91c0 si_mapLineColour]
/// An old file's line colour as a colour of this display. The second argument is not
/// looked at.
pub fn map_line_colour(m: &Mem, colour: i16, _unused: i16) -> i32 {
    let select7 = m[SI_SELECT7WM];
    let select1 = m[SI_SELECT1WM];
    let color1 = m[SI_COLOR1];
    let out: u16;
    if m[SI_BIT_PLANES] == 8 {
        if (select7 as i32 + 1) < colour as i32 {
            let mut v = select7.wrapping_add(colour as u16).wrapping_sub(0x1ea).wrapping_add(color1);
            if (v as i16 as i32) < select1 as i32 || (select7 as i32) < v as i16 as i32 {
                v = select7;
            }
            out = v;
        } else if colour == 0xf {
            out = select7;
        } else {
            out = colour as u16;
        }
    } else {
        let mut a = colour as u16 & select7;
        let mut b = colour as u16 & m[SI_UNSELECT_WM];
        if (a as i16 as i32) < select1 as i32 || (select7 as i32) < a as i16 as i32 {
            a = select7;
        }
        if (b as i16 as i32) < color1 as i32 || (m[SI_FCURVE_COLOR3] as i32) < b as i16 as i32 {
            b = color1;
        }
        out = b | a;
    }
    out as i16 as i32
}

/// [soft 0x083f9278 SI_mapIndexToWireColour]
/// An old file's colour pair as a wire colour of this display.
pub fn map_index_to_wire_colour(m: &Mem, a: i16, b: i16) -> i32 {
    let color1 = m[SI_COLOR1];
    let out: u16 = if m[SI_BIT_PLANES] == 8 {
        // two for each unit of a positive `b`, counted up in 16 bits
        let twice = if b as i32 > 0 { (b as u16).wrapping_mul(2) } else { 0 };
        twice.wrapping_add(color1)
    } else {
        (b as u16).wrapping_mul(0x40).wrapping_add(a as u16).wrapping_add(color1)
    };
    out as i16 as i32
}

/// [soft 0x083f77b0 oldpositionBinaryRead]
/// A position: an inner token (read into the file's `token`), then for 1 a vector; for 3 a
/// record of 0x1c bytes put at the head of `si_cnstrLst` (the owner, the kind, a 16-bit
/// number and a model's name: "follow that model"), which the scene loader resolves; for
/// 2, 4 and 5 a line on the standard output and nothing read. The arguments after the
/// kind are not looked at.
///
/// The original increments `SI_secureLicAlloc` (0x08679660) before the allocation of case
/// 3. That counter belongs to the licence scheme, which is not ported: the port leaves it
/// alone.
pub fn oldposition_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, position: u32, _fcurve: u32, _model: u32, owner: u32, kind: u32) -> Run<u32> {
    byteswap_fread(m, o, TOKEN_BSCENEIO.addr(), 2, 1, stream);
    match m[TOKEN_BSCENEIO] {
        1 => {
            vector_binary_read(m, o, stream, position);
        }
        2 | 4 | 5 => o.print(m, position_message()),
        3 => {
            let rec = m.si_malloc(cnstr::SIZE);
            let head = m[SI_CNSTR_LST];
            m.set_u32(rec + cnstr::NEXT, head);
            m[SI_CNSTR_LST] = rec;
            m.set_u32(rec + cnstr::OWNER, owner);
            m.set_u32(rec + cnstr::KIND, kind);
            m.set_u32(rec + cnstr::WORD_08, 0);
            m.set_u32(rec + cnstr::WORD_10, 0);
            byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
            let number = m[SI_SVALUE] as i32;
            m.set_i32(rec + cnstr::NUMBER, number);
            let name = string_binary_read(m, o, stream)?;
            m.set_u32(rec + cnstr::NAME, name);
        }
        _ => {}
    }
    Ok(1)
}

/// [soft 0x083f789c SI_cameraBinaryRead]
/// The token stream of a camera file, read into an existing camera record (finding 0005
/// has the table): what the file does not mention keeps its value. Before the first token
/// the words at +0xac and +0xb0 are cleared and the 16-bit word at +0xa8 is set to 1.
///
/// Handed no camera, the original puts a text on the status line and reads 16-bit words
/// until one is zero, stepping over no field; the answer is then 0.
pub fn camera_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, cam: u32) -> Run<u32> {
    if cam == 0 {
        o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[CAMERA_CANNOT_ALLOCATE, 3]);
        loop {
            let got = byteswap_fread(m, o, TOKEN_BSCENEIO.addr(), 2, 1, stream);
            if m[TOKEN_BSCENEIO] == 0 {
                return Ok(0);
            }
            if got == 0 {
                return Err(NoReturn::Endless("SI_cameraBinaryRead"));
            }
        }
    }
    let before = faults(m);
    let sp = m.sp();
    let size_local = m.local(4);
    m.set_u32(cam + camera::WORD_AC, 0);
    m.set_u32(cam + camera::WORD_B0, 0);
    m.set_u16(cam + camera::WORD_A8, 1);
    let run = (|| -> Run<u32> {
        loop {
            let got = byteswap_fread(m, o, TOKEN_BSCENEIO.addr(), 2, 1, stream);
            let token = m[TOKEN_BSCENEIO];
            if token == 0 {
                return Ok(1);
            }
            if got == 0 {
                return Err(NoReturn::Endless("SI_cameraBinaryRead"));
            }
            if faults(m) != before {
                return Err(NoReturn::Fault("SI_cameraBinaryRead"));
            }
            let value_at = match token {
                5 => Some(camera::ROLL),
                7 => Some(camera::NEAR),
                8 => Some(camera::FAR),
                9 => Some(camera::FOCAL_LENGTH),
                10 => Some(camera::F_STOP),
                11 => Some(camera::FOCAL_DISTANCE),
                12 => Some(camera::FIELD_OF_VIEW),
                14 => Some(camera::NEAR_FOCUS),
                15 => Some(camera::FAR_FOCUS),
                16 => Some(camera::OUT_OF_FOCUS),
                17 => Some(camera::MAXIMUM_COC),
                _ => None,
            };
            if let Some(off) = value_at {
                value_binary_read(m, o, stream, cam + off, 0, cam)?;
                continue;
            }
            match token {
                1 | 2 => {
                    read_short(m, o, stream);
                }
                3 => {
                    oldposition_binary_read(m, o, stream, cam + camera::POSITION, 0, cam + camera::POSITION_MODEL, cam, 4)?;
                }
                4 => {
                    oldposition_binary_read(m, o, stream, cam + camera::INTEREST, 0, cam + camera::INTEREST_MODEL, cam, 5)?;
                }
                6 => {
                    value3_binary_read(m, o, stream, cam + camera::YAW, cam + camera::PITCH, cam + camera::ROLL, 0, 0, 0, cam)?;
                }
                13 => {
                    let v = read_short(m, o, stream);
                    m.set_i32(cam + camera::DEPTH_OF_FIELD, v as i32);
                }
                18 => {
                    let v = read_short(m, o, stream);
                    m.set_i16(cam + camera::ENLARGE, v);
                }
                19 => {
                    value_binary_read(m, o, stream, 0, 0, cam)?;
                }
                0x14 | 0x1614 => {
                    let v = read_short(m, o, stream);
                    m.set_i16(cam + camera::PLANE_ACTIVE, v);
                    for off in [camera::PLANE_DISTANCE, camera::PLANE_WIDTH, camera::PLANE_HEIGHT, camera::PLANE_OFFSET_X, camera::PLANE_OFFSET_Y] {
                        value_binary_read(m, o, stream, cam + off, 0, cam)?;
                    }
                }
                0xff15 => {
                    byteswap_fread(m, o, size_local, 4, 1, stream);
                    user_data_read(m, o, stream, cam + camera::USER_DATA)?;
                }
                _ => {
                    file_skip_field(m, o, stream, token as i16)?;
                }
            }
        }
    })();
    m.set_sp(sp);
    run
}

/// The light a reader's handler stores into: `*lights`, or null when either is null.
fn guarded(m: &Mem, lights: u32) -> u32 {
    if lights == 0 {
        0
    } else {
        m.u32(lights)
    }
}

/// A value stored at an offset of the light, or read and dropped without one.
fn value_at(m: &mut Mem, o: &mut dyn Outside, stream: u32, lights: u32, off: u32) -> Run<u32> {
    let l = guarded(m, lights);
    if l == 0 {
        value_binary_read(m, o, stream, 0, 0, 0)
    } else {
        value_binary_read(m, o, stream, l + off, 0, l)
    }
}

/// Three values stored at an offset of the light and the two words behind it.
fn value3_at(m: &mut Mem, o: &mut dyn Outside, stream: u32, lights: u32, off: u32) -> Run<u32> {
    let l = guarded(m, lights);
    if l == 0 {
        value3_binary_read(m, o, stream, 0, 0, 0, 0, 0, 0, 0)
    } else {
        value3_binary_read(m, o, stream, l + off, l + off + 4, l + off + 8, 0, 0, 0, l)
    }
}

/// A 16-bit number read into `si_svalue`.
fn read_short(m: &mut Mem, o: &mut dyn Outside, stream: u32) -> i16 {
    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
    m[SI_SVALUE]
}

/// [soft 0x083f7ce8 SI_lightBinaryRead]
/// The token stream of a light file (finding 0005 has the table). `lights` is where the
/// light's address is kept: the type token (2) allocates the light with `SI_newLight` and
/// stores it there; every other token stores into the light found there, most of them
/// only when there is one.
///
/// Reproduced as the original has it: token 0xff29 (the second shader as this build
/// writes it) is stepped over, because its case can never match; the name read under token
/// 24 is stored in the sun's record and freed when the reader returns; a second name token
/// loses the first name's block; the tokens 27, 29, 34, 38, 41 and 0xff28 store without a
/// test of the light, 24 to 28 without a test of its sun record (a fault of the image
/// where the original dies).
pub fn light_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, lights: u32) -> Run<u32> {
    let before = faults(m);
    let sp = m.sp();
    let size_local = m.local(4);
    let protos = m.local(prototypes::SIZE);
    let mut name = 0u32;
    let mut node = 0u32;
    let run = (|| -> Run<u32> {
        loop {
            let got = byteswap_fread(m, o, TOKEN_BSCENEIO.addr(), 2, 1, stream);
            let token = m[TOKEN_BSCENEIO];
            if token == 0 {
                if name != 0 {
                    m.free(name);
                }
                return Ok(1);
            }
            if got == 0 {
                return Err(NoReturn::Endless("SI_lightBinaryRead"));
            }
            if faults(m) != before {
                return Err(NoReturn::Fault("SI_lightBinaryRead"));
            }
            match token {
                1 => name = string_binary_read(m, o, stream)?,
                2 => {
                    let kind = read_short(m, o, stream);
                    let l = new_light(m, o, kind as i32);
                    m.set_u32(lights, l);
                    if l == 0 {
                        o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[LIGHT_CANNOT_ALLOCATE, 3]);
                    } else {
                        m.set_i16(l + light::TYPE, kind);
                        let l = m.u32(lights);
                        m.set_u16(l + 0x14c, 1);
                        let l = m.u32(lights);
                        m.set_u32(l + 0x150, 0);
                        let l = m.u32(lights);
                        m.set_u32(l + 0x154, 0);
                        if name != 0 {
                            let l = m.u32(lights);
                            si_mem::clib::strncpy(m, l, name, 0x1e);
                            m.free(name);
                            name = 0;
                        }
                        let l = m.u32(lights);
                        let list = scene_graf_list(m);
                        node = o.call(m, &SI_FIND_GL_DATA, &[list, l, 0]);
                    }
                }
                3 => {
                    value3_at(m, o, stream, lights, light::COLOUR)?;
                }
                4 => {
                    value_at(m, o, stream, lights, light::FALL_OFF_START)?;
                }
                5 => {
                    value_at(m, o, stream, lights, light::FALL_OFF_END)?;
                }
                6 => {
                    value_at(m, o, stream, lights, light::SHADOW_FACTOR)?;
                }
                7 | 8 => {
                    let l = guarded(m, lights);
                    if l == 0 {
                        oldposition_binary_read(m, o, stream, 0, 0, 0, 0, 0)?;
                    } else if token == 7 {
                        oldposition_binary_read(m, o, stream, l + light::POSITION, 0, l + light::POSITION_MODEL, l, 6)?;
                    } else {
                        oldposition_binary_read(m, o, stream, l + light::INTEREST, 0, l + light::INTEREST_MODEL, l, 7)?;
                    }
                }
                9 => {
                    value_at(m, o, stream, lights, light::CONE_ANGLE)?;
                }
                10 => {
                    value_at(m, o, stream, lights, light::SPREAD_ANGLE)?;
                }
                11 => {
                    value_binary_read(m, o, stream, 0, 0, 0)?;
                }
                12 => {
                    let v = read_short(m, o, stream);
                    if node != 0 {
                        let c = map_line_colour(m, v, 1);
                        m.set_u16(node + graf_node::COLOUR, c as u16);
                    }
                }
                13 => {
                    let v = read_short(m, o, stream);
                    if node != 0 {
                        m.set_i32(node + graf_node::WORD_0C, v as i32);
                    }
                }
                14 => {
                    let v = read_short(m, o, stream);
                    if node != 0 {
                        let v = v as i32;
                        m.set_i32(node + graf_node::DISPLAY, v);
                        if v == 1 || v == 3 {
                            let list = scene_graf_list(m);
                            o.call(m, &SI_GRAFLIST_SET_ACTIVE, &[list, node]);
                        }
                    }
                }
                15 => {
                    let l = guarded(m, lights);
                    value_binary_read(m, o, stream, 0, 0, l)?;
                }
                16 => {
                    let v = read_short(m, o, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        m.set_i16(l + light::SHADOW_TYPE, v);
                    }
                }
                17..=19 => {
                    let v = read_short(m, o, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let off = match token {
                            17 => light::SHADOW_MAP_RES,
                            18 => light::SHADOW_FILTER_SIZE,
                            _ => light::SHADOW_FILTER_STEP,
                        };
                        m.set_i32(l + off, v as i32);
                    }
                }
                20 => {
                    value_at(m, o, stream, lights, light::SHADOW_FADE_START)?;
                }
                21 => {
                    value_at(m, o, stream, lights, light::SHADOW_FADE_RATE)?;
                }
                22 => {
                    let a = read_short(m, o, stream);
                    let b = read_short(m, o, stream);
                    if node != 0 {
                        let c = map_index_to_wire_colour(m, a, b);
                        m.set_u16(node + graf_node::COLOUR, c as u16);
                    }
                }
                23 => {
                    let v = read_short(m, o, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        m.set_i32(l + light::SELECTIVE, v as i32);
                    }
                    let v = read_short(m, o, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        m.set_i32(l + light::INCLUDE, v as i32);
                    }
                }
                24 => {
                    name = string_binary_read(m, o, stream)?;
                    let l = guarded(m, lights);
                    if l != 0 {
                        let s = m.u32(l + light::SUN);
                        m.set_u32(s + sun::NAME, name);
                    }
                }
                25 | 26 => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let s = m.u32(l + light::SUN);
                        let v = m[SI_IVALUE];
                        m.set_i32(s + if token == 25 { sun::LATITUDE } else { sun::LONGITUDE }, v);
                    }
                }
                27 => {
                    let v = read_short(m, o, stream);
                    // no test of the light
                    let l = m.u32(lights);
                    let s = m.u32(l.wrapping_add(light::SUN));
                    m.set_i16(s.wrapping_add(sun::DAYST), v);
                }
                28 => {
                    byteswap_fread(m, o, SI_LVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let s = m.u32(l + light::SUN);
                        let v = m[SI_LVALUE];
                        m.set_u32(s + sun::DATE_TIME, v);
                    }
                }
                29 | 41 => {
                    m.set_u32(protos + prototypes::UDF, 0);
                    let u = udf_read(m, o, stream, protos)?;
                    // no test of the light
                    let l = m.u32(lights);
                    m.set_u32(l.wrapping_add(if token == 29 { light::SHADER } else { light::SHADER2 }), u);
                }
                30 | 35 | 39 => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let off = match token {
                            30 => light::AREA_LIGHT,
                            35 => light::AREA_LOCAL_COORDS,
                            _ => light::AREA_SELECTED,
                        };
                        let v = m[SI_IVALUE];
                        m.set_i16(l + off, v as i16);
                    }
                }
                31..=33 => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let off = match token {
                            31 => light::AREA_TYPE,
                            32 => light::AREA_U_SAMPLE,
                            _ => light::AREA_V_SAMPLE,
                        };
                        let v = m[SI_IVALUE];
                        m.set_i32(l + off, v);
                    }
                }
                34 => {
                    // no test of the light: the area type is read through it
                    let l = m.u32(lights);
                    match m.i32(l.wrapping_add(light::AREA_TYPE)) {
                        1 => {
                            if lights == 0 || l == 0 {
                                value_binary_read(m, o, stream, 0, 0, 0)?;
                            } else {
                                value_binary_read(m, o, stream, l + light::AREA_GEOMETRY, 0, l)?;
                            }
                        }
                        0 => {
                            for k in 0..4u32 {
                                value3_at(m, o, stream, lights, light::AREA_GEOMETRY + 12 * k)?;
                            }
                        }
                        2 => {
                            if lights == 0 || l == 0 {
                                value3_binary_read(m, o, stream, 0, 0, 0, 0, 0, 0, 0)?;
                            } else {
                                value3_binary_read(m, o, stream, l + 0x190, l + 0x194, l + 0x198, 0, 0, 0, l)?;
                            }
                            value_at(m, o, stream, lights, 0x19c)?;
                        }
                        _ => {}
                    }
                }
                36 => {
                    value3_at(m, o, stream, lights, light::AREA_SCALE)?;
                }
                37 => {
                    value3_at(m, o, stream, lights, light::AREA_ROTATION)?;
                }
                38 => {
                    // no test of the light
                    for row in 0..4u32 {
                        for column in 0..4u32 {
                            let l = m.u32(lights);
                            value_binary_read(m, o, stream, l.wrapping_add(light::AREA_ROTATION_MAT + row * 0x10 + column * 4), 0, l)?;
                        }
                    }
                }
                40 => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let v = m[SI_IVALUE];
                        m.set_u16(l + light::DIFFUSE, (v != 0) as u16);
                    }
                }
                42 | 0x102a => {
                    value3_at(m, o, stream, lights, light::ENERGY)?;
                }
                43 | 0x042b => {
                    value_at(m, o, stream, lights, light::EXPONENT)?;
                }
                44 | 0x042c | 45 | 0x042d => {
                    byteswap_fread(m, o, SI_LVALUE.addr(), 4, 1, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        let v = m[SI_LVALUE];
                        m.set_u32(l + if token == 44 || token == 0x042c { light::CAUSTIC_PHOTON } else { light::GLOBILLUM_PHOTON }, v);
                    }
                }
                46 | 0x022e | 47 | 0x022f => {
                    let v = read_short(m, o, stream);
                    let l = guarded(m, lights);
                    if l != 0 {
                        m.set_i16(l + if token == 46 || token == 0x022e { light::CAUSTIC_ENABLE } else { light::GLOBILLUM_ENABLE }, v);
                    }
                }
                0xff28 => {
                    byteswap_fread(m, o, size_local, 4, 1, stream);
                    // no test of the light
                    let l = m.u32(lights);
                    user_data_read(m, o, stream, l.wrapping_add(light::USER_DATA))?;
                }
                _ => {
                    file_skip_field(m, o, stream, token as i16)?;
                }
            }
        }
    })();
    m.set_sp(sp);
    run
}

/// [soft 0x083f8d20 SI_latticeBinaryRead]
/// The token stream of a lattice file: the lattices of one of a model's two lists (`branch`
/// says which), one after the other, closed by one zero token (finding 0005 has the
/// table). The fields are collected in locals and copied into a new lattice (0x44 bytes,
/// with a block of vectors and a block of 16-bit words) at the points token (4), which
/// hands it to `SI_modelAddILattice (0x08472a0c)`; after the first lattice
/// `SI_edAddLattice (0x0810383c)` is called and `glbBranch` set. The last two arguments
/// are not looked at. A token nobody knows is ignored and nothing is stepped over.
///
/// As the original has it: the locals are not reset between lattices, and the
/// interpolation codes and the three vectors are locals nobody set when the first lattice
/// lacks them. Called from `SI_elemdictReadLattice` they then hold what the header reader
/// left on the stack ([`super::elemdict::elemdict_read_lattice`] reproduces that); called
/// directly, here, what the image's stack holds. Without a model, or when a block
/// cannot be had, the points are read and dropped; a lattice whose word block could not
/// be had stays allocated without its vectors.
///
/// Not done: the original increments `SI_secureLicAlloc` (0x08679660) before each of its
/// three allocations; that counter belongs to the licence scheme, which is not ported. And
/// a number of points whose vectors the file cannot hold (beyond [`COUNT_LIMIT`]) is not
/// allocated for: the port stops.
pub fn lattice_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, model: u32, branch: i16, _fcurve: u32, _owner: u32) -> Run<u32> {
    let sp = m.sp();
    // the three vectors and the three codes: locals of the original that nobody clears
    let scaling = m.local(12);
    let maximum = m.local(12);
    let minimum = m.local(12);
    let codes = m.local(12);
    let run = lattice_read_with(m, o, stream, model, branch, [codes, minimum, maximum, scaling]);
    m.set_sp(sp);
    run
}

/// The lattice reader with its four uncleared locals (the three interpolation codes, the
/// minimum, the maximum, the scaling: 12 bytes each) at addresses the caller chose.
pub(crate) fn lattice_read_with(m: &mut Mem, o: &mut dyn Outside, stream: u32, model: u32, branch: i16, locals: [u32; 4]) -> Run<u32> {
    let before = faults(m);
    let [codes, minimum, maximum, scaling] = locals;
    let (mut active, mut nx, mut ny, mut nz) = (0u16, 0i16, 0i16, 0i16);
    let mut index: i32 = 0;
    {
        loop {
            let got = byteswap_fread(m, o, TOKEN_BSCENEIO.addr(), 2, 1, stream);
            let token = m[TOKEN_BSCENEIO];
            if token == 0 {
                return Ok(1);
            }
            if got == 0 {
                return Err(NoReturn::Endless("SI_latticeBinaryRead"));
            }
            if faults(m) != before {
                return Err(NoReturn::Fault("SI_latticeBinaryRead"));
            }
            match token {
                1 => active = read_short(m, o, stream) as u16,
                2 => {
                    nx = read_short(m, o, stream);
                    ny = read_short(m, o, stream);
                    nz = read_short(m, o, stream);
                }
                3 => {
                    for k in 0..3u32 {
                        let v = read_short(m, o, stream) as i32;
                        m.set_i32(codes + 4 * k, v);
                    }
                }
                6 => {
                    vector_binary_read(m, o, stream, minimum);
                }
                7 => {
                    vector_binary_read(m, o, stream, maximum);
                }
                8 => {
                    vector_binary_read(m, o, stream, scaling);
                }
                4 => {
                    let n = (nx as i32).wrapping_mul(ny as i32).wrapping_mul(nz as i32);
                    if n <= 0 {
                        continue;
                    }
                    if n as u64 * 12 > COUNT_LIMIT as u64 && n as u64 * 12 > o.file_left(m, stream) as u64 {
                        return Err(NoReturn::Count("SI_latticeBinaryRead"));
                    }
                    let mut lat = 0u32;
                    let mut good = false;
                    if model != 0 {
                        lat = m.si_malloc(lattice::SIZE);
                        if lat != 0 {
                            let twice = (n as u32).wrapping_add(n as u32);
                            let keys = m.si_malloc((n as u32).wrapping_add(twice) << 2);
                            m.set_u32(lat + lattice::KEYS, keys);
                            let flags = m.si_malloc(twice);
                            m.set_u32(lat + lattice::FLAGS, flags);
                            good = m.u32(lat + lattice::KEYS) != 0 && m.u32(lat + lattice::FLAGS) != 0;
                        }
                    }
                    if good {
                        let mut key = m.u32(lat + lattice::KEYS);
                        let mut flag = m.u32(lat + lattice::FLAGS);
                        for _ in 0..n {
                            vector_binary_read(m, o, stream, key);
                            m.set_u16(flag, 0);
                            key = key.wrapping_add(12);
                            flag = flag.wrapping_add(2);
                        }
                        m.set_u16(lat + lattice::ACTIVE, active);
                        m.set_i16(lat + lattice::COUNT_X, nx);
                        m.set_i16(lat + lattice::COUNT_Y, ny);
                        m.set_i16(lat + lattice::COUNT_Z, nz);
                        for k in 0..3u32 {
                            let v = m.u32(codes + 4 * k);
                            m.set_u32(lat + lattice::INTERPOLATION + 4 * k, v);
                        }
                        for (from, to) in [(minimum, lattice::MINIMUM), (maximum, lattice::MAXIMUM), (scaling, lattice::SCALING)] {
                            for k in 0..3u32 {
                                let v = m.u32(from + 4 * k);
                                m.set_u32(lat + to + 4 * k, v);
                            }
                        }
                        o.call(m, &SI_MODEL_ADD_I_LATTICE, &[model, branch as u16 as u32, index as u16 as u32, lat]);
                        if model != 0 && index == 0 {
                            m[GLB_BRANCH] = branch;
                            o.call(m, &SI_ED_ADD_LATTICE, &[model, branch as u16 as u32]);
                        }
                    } else {
                        o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[LATTICE_CANNOT_ALLOCATE, 3]);
                        if lat != 0 {
                            let keys = m.u32(lat + lattice::KEYS);
                            if keys != 0 {
                                m.free(keys);
                                m.set_u32(lat + lattice::KEYS, 0);
                            }
                        }
                        for _ in 0..n {
                            vector_binary_read(m, o, stream, 0);
                        }
                    }
                    index = index.wrapping_add(1);
                }
                _ => {}
            }
        }
    }
}
