// SPDX-License-Identifier: MIT
//! Metaballs and metasystems on the image: the allocators and the group list of
//! `metaball.c`, and `metaballBinaryRead` and `metaSystemBinaryRead` of `binary.c` (finding
//! 0004, "Metaballs").
//!
//! `si-types` has none of these records; their fields are addressed by the offsets of
//! [`ball`] and [`system`] below, each read from the listing.

use super::globals::SI_IDENTITE;
use super::io::{byteswap_fread, file_skip_field, matrix_fread, string_binary_read, three_doubles_fread};
use super::matio::next_token;
use super::outside::Outside;
use super::proto::{slot, Prototypes};
use super::texture::matrix_copy;
use si_mem::{Mem, Ptr32};

/// Offsets in a metaball (0xf8 bytes).
pub mod ball {
    /// The node the metaball belongs to.
    pub const MODEL: u32 = 0x08;
    pub const MATRIX: u32 = 0x0c;
    /// Doubles: the ratio (0.5 in a new one), the weight (2) and the three exponents (2).
    pub const RATIO: u32 = 0xcc;
    pub const WEIGHT: u32 = 0xd4;
    pub const EXPONENT: u32 = 0xdc;
    pub const SIZE: u32 = 0xf8;
}

/// Offsets in a metasystem (`SI_MetaSystem`, 0x68 bytes), in one of its groups (0x14
/// bytes), in a group's primitive (0x14) and in an unblend pair (0x18).
pub mod system {
    /// Six doubles: three times the largest float and three times its negative (an empty
    /// bounding box).
    pub const BOX: u32 = 0x04;
    /// Floats: the wireframe and the rendering resolution (0.5 and 0.25 in a new one).
    pub const WIRE_RESOLUTION: u32 = 0x34;
    pub const RENDER_RESOLUTION: u32 = 0x38;
    pub const MODEL: u32 = 0x3c;
    pub const SHOW: u32 = 0x42;
    /// The first group and the first unblend pair: the newest of each.
    pub const GROUPS: u32 = 0x5c;
    pub const UNBLENDS: u32 = 0x60;
    pub const SIZE: u32 = 0x68;
    /// A group: its name (a copy), its first primitive, the group after it and the one
    /// before it.
    pub const GROUP_NAME: u32 = 0x00;
    pub const GROUP_PRIMITIVES: u32 = 0x08;
    pub const GROUP_NEXT: u32 = 0x0c;
    pub const GROUP_PREVIOUS: u32 = 0x10;
    /// A primitive: a zero (the model, once the names are resolved), the primitive after
    /// it, the one before it, the name of its model.
    pub const PRIMITIVE_NEXT: u32 = 0x04;
    pub const PRIMITIVE_PREVIOUS: u32 = 0x08;
    pub const PRIMITIVE_NAME: u32 = 0x0c;
    /// An unblend pair: two zeros, the pair before it, the pair after it, two names.
    pub const UNBLEND_PREVIOUS: u32 = 0x08;
    pub const UNBLEND_NEXT: u32 = 0x0c;
    pub const UNBLEND_NAMES: u32 = 0x10;
}

/// [soft 0x0845aa90 SI_metaballAllocate]: a new metaball of the node `model`: ratio 0.5,
/// weight 2, exponents 2 (the doubles at 0x0865fb10 and 0x0865fb18), the identity matrix,
/// a zero at +0xf4. The first eight bytes and +0x4c to +0xcb are not written.
pub fn metaball_allocate(m: &mut Mem, model: u32) -> u32 {
    let p = m.si_malloc(ball::SIZE);
    if p != 0 {
        m.set_f64(p + ball::WEIGHT, 2.0);
        m.set_f64(p + ball::RATIO, 0.5);
        for k in 0..3 {
            m.set_f64(p + ball::EXPONENT + 8 * k, 2.0);
        }
        m.set_u32(p + ball::MODEL, model);
        matrix_copy(m, SI_IDENTITE.addr(), p + ball::MATRIX);
        m.set_u32(p + 0xf4, 0);
    }
    p
}

/// [soft 0x0845c78c SI_metaSystemAllocate]: a new metasystem: 1 in its first short, the
/// empty bounding box (the doubles at 0x0865fb60 and 0x0865fb68), the resolutions 0.5 and
/// 0.25 (the floats at 0x0865fb70), no node, no groups, no pairs. Not written: +2, +0x40,
/// +0x48 to +0x4b, +0x64 to +0x67.
pub fn meta_system_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(system::SIZE);
    if p != 0 {
        for k in 0..3 {
            m.set_f64(p + system::BOX + 8 * k, f32::MAX as f64);
            m.set_f64(p + system::BOX + 0x18 + 8 * k, -(f32::MAX as f64));
        }
        m.set_f32(p + system::WIRE_RESOLUTION, 0.5);
        m.set_f32(p + system::RENDER_RESOLUTION, 0.25);
        m.set_u16(p, 1);
        m.set_u32(p + system::MODEL, 0);
        for off in [0x42, 0x44, 0x46] {
            m.set_u16(p + off, 0);
        }
        for off in [0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60] {
            m.set_u32(p + off, 0);
        }
    }
    p
}

/// [soft 0x0845dd04 SI_metaSystemAddGroup]: the group of the metasystem named `name`,
/// stored in `*group`: the one it has (the answer is its place in the list), or a new one
/// with a copy of the name at the head of the list (the answer is 0). If the new group
/// cannot be allocated, `*group` is null and the answer −1.
pub fn meta_system_add_group(m: &mut Mem, sys: u32, name: u32, group: u32) -> i32 {
    let mut g = m.u32(sys + system::GROUPS);
    let mut place = 0;
    while g != 0 {
        if si_mem::clib::strcmp(m, name, m.u32(g + system::GROUP_NAME)) == 0 {
            m.set_u32(group, g);
            return place;
        }
        g = m.u32(g + system::GROUP_NEXT);
        place += 1;
    }
    let g = m.si_malloc(0x14);
    if g == 0 {
        m.set_u32(group, 0);
        return -1;
    }
    let copy = m.strdup(name);
    m.set_u32(g + system::GROUP_NAME, copy);
    m.set_u32(g + system::GROUP_PRIMITIVES, 0);
    let head = m.u32(sys + system::GROUPS);
    m.set_u32(g + system::GROUP_NEXT, head);
    m.set_u32(g + system::GROUP_PREVIOUS, 0);
    if m.u32(sys + system::GROUPS) != 0 {
        let head = m.u32(sys + system::GROUPS);
        m.set_u32(head + system::GROUP_PREVIOUS, g);
    }
    m.set_u32(sys + system::GROUPS, g);
    m.set_u32(group, g);
    0
}

/// [soft 0x083f23ec metaballBinaryRead]: a metaball of the node `model`: the ratio (4)
/// and the weight (5), a double each; the three exponents (6); the matrix (7); and three
/// old tokens (1, 2, 3) whose four bytes are read and dropped.
pub fn metaball_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, model: u32) -> u32 {
    let p = metaball_allocate(m, model);
    if p == 0 {
        return 0;
    }
    const FRAME: u32 = 8;
    let frame = m.local(FRAME);
    let (scratch4, token_at) = (frame, frame + 4);
    let list_at = protos.addr().wrapping_add(4 * slot::METABALL as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 | 2 | 3 => {
                byteswap_fread(m, o, scratch4, 4, 1, file);
            }
            4 => {
                byteswap_fread(m, o, p + ball::RATIO, 8, 1, file);
            }
            5 => {
                byteswap_fread(m, o, p + ball::WEIGHT, 8, 1, file);
            }
            6 => {
                three_doubles_fread(m, o, p + ball::EXPONENT, 0x18, 1, file);
            }
            7 => {
                matrix_fread(m, o, p + ball::MATRIX, 0x40, 1, file);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    p
}

/// [soft 0x083f257c metaSystemBinaryRead]: the metasystem of the node `model`.
///
/// Tokens: the two resolutions (1, 2: four bytes each), the show flag (3), the groups (4)
/// and the unblend pairs (5).
///
/// A group is the tokens of the metagroup's list: its name (1), which finds or makes the
/// group ([`meta_system_add_group`]; the string read is freed, the group has a copy), and
/// its primitives (2): a short count and that many names of models, each kept in a new
/// record of 0x14 bytes put at the head of the group's primitives. Without a group (no
/// name token came first, or it could not be allocated) the names are read and dropped.
///
/// An unblend pair is a new record of 0x18 bytes at the head of the system's pairs, and
/// the tokens of the metaunblend's list: 1 is two names.
///
/// The counts are read into a local short that the next count overwrites.
pub fn meta_system_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, model: u32) -> u32 {
    let s = meta_system_allocate(m);
    if s == 0 {
        return 0;
    }
    m.set_u32(s + system::MODEL, model);
    // the frame: the group being read (handed to the group routine by address), the two
    // counts, the token
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (group_at, count_at, inner_at, token_at) = (frame, frame + 4, frame + 6, frame + 8);
    let table = protos.addr();
    let list_at = table.wrapping_add(4 * slot::METASYSTEM as u32);
    let group_list_at = table.wrapping_add(4 * slot::METAGROUP as u32);
    let unblend_list_at = table.wrapping_add(4 * slot::METAUNBLEND as u32);
    let dropped_string = |m: &mut Mem, o: &mut dyn Outside| {
        let text = string_binary_read(m, o, file);
        if text != 0 {
            m.free(text);
        }
    };
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 => {
                byteswap_fread(m, o, s + system::WIRE_RESOLUTION, 4, 1, file);
            }
            2 => {
                byteswap_fread(m, o, s + system::RENDER_RESOLUTION, 4, 1, file);
            }
            3 => {
                byteswap_fread(m, o, s + system::SHOW, 2, 1, file);
            }
            4 => {
                byteswap_fread(m, o, count_at, 2, 1, file);
                let mut i: i32 = 0;
                while i < m.i16(count_at) as i32 {
                    m.set_u32(group_at, 0);
                    let mut inner: i16 = 0;
                    next_token(m, o, file, group_list_at, token_at, &mut inner);
                    i += 1;
                    while m.i16(token_at) != 0 {
                        match m.i16(token_at) {
                            1 => {
                                let name = string_binary_read(m, o, file);
                                meta_system_add_group(m, s, name, group_at);
                                if name != 0 {
                                    m.free(name);
                                }
                            }
                            2 => {
                                byteswap_fread(m, o, inner_at, 2, 1, file);
                                let mut k: i32 = 0;
                                while k < m.i16(inner_at) as i32 {
                                    let primitive = if m.u32(group_at) == 0 { 0 } else { m.si_malloc(0x14) };
                                    if primitive == 0 {
                                        dropped_string(m, o);
                                    } else {
                                        m.set_u32(primitive, 0);
                                        let name = string_binary_read(m, o, file);
                                        m.set_u32(primitive + system::PRIMITIVE_NAME, name);
                                        m.set_u32(primitive + system::PRIMITIVE_PREVIOUS, 0);
                                        let group = m.u32(group_at);
                                        let head = m.u32(group + system::GROUP_PRIMITIVES);
                                        m.set_u32(primitive + system::PRIMITIVE_NEXT, head);
                                        if head != 0 {
                                            m.set_u32(head + system::PRIMITIVE_PREVIOUS, primitive);
                                        }
                                        let group = m.u32(group_at);
                                        m.set_u32(group + system::GROUP_PRIMITIVES, primitive);
                                    }
                                    k += 1;
                                }
                            }
                            t => {
                                file_skip_field(m, o, file, t);
                            }
                        }
                        next_token(m, o, file, group_list_at, token_at, &mut inner);
                    }
                }
            }
            5 => {
                byteswap_fread(m, o, count_at, 2, 1, file);
                let mut i: i32 = 0;
                while i < m.i16(count_at) as i32 {
                    let pair = m.si_malloc(0x18);
                    if pair != 0 {
                        for off in [0x00, 0x04, 0x10, 0x14, 0x08] {
                            m.set_u32(pair + off, 0);
                        }
                        let head = m.u32(s + system::UNBLENDS);
                        m.set_u32(pair + system::UNBLEND_NEXT, head);
                        if head != 0 {
                            m.set_u32(head + system::UNBLEND_PREVIOUS, pair);
                        }
                        m.set_u32(s + system::UNBLENDS, pair);
                    }
                    let mut inner: i16 = 0;
                    next_token(m, o, file, unblend_list_at, token_at, &mut inner);
                    i += 1;
                    while m.i16(token_at) != 0 {
                        match m.i16(token_at) {
                            1 => {
                                if pair == 0 {
                                    dropped_string(m, o);
                                    dropped_string(m, o);
                                } else {
                                    let name = string_binary_read(m, o, file);
                                    m.set_u32(pair + system::UNBLEND_NAMES, name);
                                    let name = string_binary_read(m, o, file);
                                    m.set_u32(pair + system::UNBLEND_NAMES + 4, name);
                                }
                            }
                            t => {
                                file_skip_field(m, o, file, t);
                            }
                        }
                        next_token(m, o, file, unblend_list_at, token_at, &mut inner);
                    }
                }
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    s
}
