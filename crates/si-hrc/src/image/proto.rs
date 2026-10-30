// SPDX-License-Identifier: MIT
//! The prototype table on the image (finding 0004, "The prototype block").

use super::io::byteswap_fread;
use super::Outside;
use si_mem::{record, Mem, Ptr32};

record! {
    /// The table of token lists of a file being read (`si_Prototypes`): one list per kind
    /// of record, null where the file has none.
    pub struct Prototypes = "si_Prototypes", size 0x70 {
        0x00 lists: [Ptr32<i16>; 28], "by slot; a list ends with a 0 token";
    }
}

/// The slots of [`Prototypes::lists`].
pub mod slot {
    pub const SPLINE: usize = 0;
    pub const SPLINE_KEY: usize = 1;
    pub const HIERARCHY: usize = 2;
    pub const PATCH: usize = 3;
    pub const FACE: usize = 4;
    pub const MESH: usize = 5;
    pub const EDGE: usize = 6;
    pub const POLYGON: usize = 7;
    pub const POLY_NODE: usize = 8;
    pub const VERTEX: usize = 9;
    pub const MATERIAL: usize = 10;
    pub const TEXTURE: usize = 11;
    pub const TEXTURE3D: usize = 12;
    pub const DYNA_MATTER: usize = 15;
    pub const JOINT: usize = 16;
    pub const ENVELOP: usize = 17;
    pub const ENVELOP_VERT: usize = 18;
    pub const CLUSTER: usize = 19;
    pub const METABALL: usize = 20;
    pub const METASYSTEM: usize = 21;
    pub const METAGROUP: usize = 22;
    pub const METAUNBLEND: usize = 23;
    pub const UDF: usize = 24;
    pub const NURBS_CRV: usize = 25;
    pub const NURBS_SRF: usize = 26;
    pub const NURBS_COMP: usize = 27;
}

/// The slot the list of prototype `id` is kept in: ids 3 and 5 to 31; the spline key (18)
/// sits between the spline and the hierarchy.
pub fn slot_of(id: i16) -> Option<usize> {
    match id {
        3 => Some(0),
        18 => Some(1),
        5..=17 => Some(id as usize - 3),
        19..=31 => Some(id as usize - 4),
        _ => None,
    }
}

/// [soft 0x083feddc si_prototypeAllocate]: an empty table.
pub fn prototype_allocate(m: &mut Mem) -> Ptr32<Prototypes> {
    Ptr32::new(m.si_calloc(1, 0x70))
}

/// [soft 0x083fdc40 si_prototypeBinaryRead]: the block that follows the `PROT` tag. Per
/// list a short id and a short count, then that many tokens into a zeroed array of
/// `count` shorts; a list for an id without a slot is read into an array that is freed at
/// once; a second list for an id replaces the pointer (the first array stays allocated).
/// The block ends with an id of 0, or where the file ends before an id.
pub fn prototype_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) {
    // the two shorts the original reads into: the count, then the id
    let frame = m.local(4);
    let (count_at, id_at) = (frame, frame + 2);
    let mut spare = 0u32;
    byteswap_fread(m, o, id_at, 2, 1, file);
    loop {
        if m.i16(id_at) == 0 {
            break;
        }
        byteswap_fread(m, o, count_at, 2, 1, file);
        let count = m.i16(count_at);
        let list = m.si_calloc(count as i32 as u32, 2);
        match slot_of(m.i16(id_at)) {
            Some(k) => m[protos].lists[k] = Ptr32::new(list),
            None => spare = list,
        }
        let mut at = list;
        let mut i: i16 = 0;
        while i < count {
            byteswap_fread(m, o, at, 2, 1, file);
            i += 1;
            at += 2;
        }
        if spare != 0 {
            m.free(spare);
            spare = 0;
        }
        if byteswap_fread(m, o, id_at, 2, 1, file) == 0 {
            break;
        }
    }
    m.release(4);
}

/// [soft 0x083fedf8 si_prototypeDispose]: frees every list, then the table, and clears the
/// caller's pointer.
pub fn prototype_dispose(m: &mut Mem, pp: Ptr32<Ptr32<Prototypes>>) {
    if m[pp].is_null() {
        return;
    }
    for k in 0..28 {
        let table = m[pp];
        let list = m[table].lists[k];
        if !list.is_null() {
            m.free(list.addr());
            let table = m[pp];
            m[table].lists[k] = Ptr32::NULL;
        }
    }
    let table = m[pp];
    if !table.is_null() {
        m.free(table.addr());
        m[pp] = Ptr32::NULL;
    }
    m[pp] = Ptr32::NULL;
}

/// [soft 0x083fe0ac SI_prototypeBinaryWrite]: the table the writers write in front of a
/// record: `"PROT"` (a plain `fwrite` of 4 bytes), then the prototypes in the order 5, 3,
/// 18, 6, 7, 8, 12, 10, 11, 9, 13, 14, 15, 19 to 31, each built in 64 shorts of the
/// routine's stack as its id, its count and its tokens with a closing 0, and written with
/// one swapping write; then a 0 id. Which tokens a list has is decided by comparing
/// `SI_versionOutput` with the dates of finding 0004's table (at 4.0, the only version
/// this build writes, every comparison passes); the prototypes 28 to 31 are written only
/// above 2.65.
pub fn prototype_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    use super::globals::S_PROT;
    use super::wio::{byteswap_fwrite, out_above, out_from};
    const FRAME: u32 = 0x80;
    let local = m.local(FRAME);
    o.fwrite(m, S_PROT, 4, 1, file);
    // each list goes into the stack buffer as the original stores it: id, count (the
    // number of shorts after the count, its closing 0 included), tokens, 0
    let put = |m: &mut Mem, o: &mut dyn Outside, id: i16, tokens: &[u16]| {
        m.set_i16(local, id);
        m.set_i16(local + 2, (tokens.len() + 1) as i16);
        for (k, &t) in tokens.iter().enumerate() {
            m.set_u16(local + 4 + 2 * k as u32, t);
        }
        m.set_i16(local + 4 + 2 * tokens.len() as u32, 0);
        byteswap_fwrite(m, o, local, 2, tokens.len() as u32 + 3, file);
    };
    // 5: the hierarchy
    let mut t: Vec<u16> = vec![1, 2, 3, 0xc, 4, 5, 6, 7, 8, 9, 0xe, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16];
    if out_above(m, 2.65) {
        t.extend([0x17, 0x18]);
    }
    if out_above(m, 2.69904) {
        t.push(0x19);
        if out_from(m, 3.089) {
            t.extend([0x41a, 0x21b]);
        }
    }
    if out_from(m, 3.49089) {
        t.push(0xff1c);
    }
    t.extend([0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22]);
    if out_from(m, 3.93) {
        t.push(0xff24);
    }
    if out_from(m, 3.8109) {
        t.extend([0x221, 0x222]);
    }
    if out_from(m, 3.8209) {
        t.push(0x223);
    }
    t.extend([0x224, 10, 0xb]);
    put(m, o, 5, &t);
    // 3: the spline; 18: the spline key (fixed)
    put(m, o, 3, &[8, 1, 7, 3, 4, 5, 6]);
    put(m, o, 0x12, &[1, 2, 3, 4, 5, 6]);
    // 6: the patch
    let mut t: Vec<u16> = (1..=0x10).collect();
    if out_above(m, 2.65) {
        t.extend(0x11..=0x19);
    }
    put(m, o, 6, &t);
    // 7: the face
    let t: &[u16] = if out_above(m, 2.99903) { &[3, 1, 2, 4] } else { &[1, 2] };
    put(m, o, 7, t);
    // 8: the mesh
    let mut t: Vec<u16> = vec![6, 2];
    if out_from(m, 3.93) {
        t.push(0x413);
    }
    t.extend([7, 8, 9]);
    if out_from(m, 3.8109) {
        t.extend([0x20a, 0x20b, 0x20c, 0x40d, 0x40e, 0x40f, 0x210, 0x211, 0x212]);
    }
    put(m, o, 8, &t);
    // 12: the vertex
    put(m, o, 0xc, &[1, 4]);
    // 10: the polygon
    let mut t: Vec<u16> = vec![2, 3];
    if out_above(m, 2.65) {
        t.push(4);
    }
    put(m, o, 10, &t);
    // 11: the polygon node
    let mut t: Vec<u16> = vec![3, 2, 4, 0x405];
    if out_from(m, 3.93) {
        t.push(0xff06);
    }
    put(m, o, 0xb, &t);
    // 9: the edge
    put(m, o, 9, &[4, 3]);
    // 13: the material
    let mut t: Vec<u16> = vec![1, 2, 0xe, 0xf, 3, 4, 5, 6, 7, 0xd, 0xb, 0x10, 0x11, 0x12, 0x13, 0x14];
    if out_above(m, 2.65) {
        t.extend([0x15, 0x16, 0x17]);
    }
    t.extend([0x218, 0xff19]);
    if out_from(m, 3.8109) {
        t.push(0xff1a);
    }
    t.extend([0x21b, 0x21c, 0x21d]);
    put(m, o, 0xd, &t);
    // 14: the 2D texture
    let mut t: Vec<u16> = (1..=0x10).collect();
    t.extend([0x12, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f]);
    if out_above(m, 2.62) {
        t.extend([0x20, 0x21, 0x22, 0x23]);
        if out_above(m, 2.65) {
            t.extend([0x24, 0x25, 0x32, 0x233, 0x234]);
            if out_above(m, 3.5) {
                t.extend([0x235, 0x236, 0x237]);
            }
        }
    }
    t.extend([0xff35, 0x238]);
    put(m, o, 0xe, &t);
    // 15: the 3D texture
    let mut t: Vec<u16> = vec![1, 2, 3, 4, 5, 0x16, 0x17, 0x18, 0x19, 0x1a, 10, 0x1b, 0x1c, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x13, 0x15, 0x1d, 0x1f, 0x20, 0x21];
    if out_above(m, 2.65) {
        t.extend([0x22, 0x23]);
    }
    t.push(0xff24);
    put(m, o, 0xf, &t);
    // 19 to 27 (fixed)
    put(m, o, 0x13, &[1, 2, 3, 4, 5, 6, 7]);
    put(m, o, 0x14, &[1]);
    put(m, o, 0x15, &[1, 5, 4, 7, 8, 9, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x14, 0x15]);
    put(m, o, 0x16, &[1, 3, 4, 6]);
    put(m, o, 0x17, &[1, 2, 3, 4]);
    put(m, o, 0x18, &[4, 5, 6, 7]);
    put(m, o, 0x19, &[1, 2, 3, 4, 5]);
    put(m, o, 0x1a, &[1, 2]);
    put(m, o, 0x1b, &[1]);
    // 28: the shader reference
    if out_above(m, 2.65) {
        put(m, o, 0x1c, &[1, 2, 3, 4]);
    }
    // 29: the NURBS curve (the constant is loaded first and compared with the version:
    // the same "above")
    if out_above(m, 2.65) {
        let mut t: Vec<u16> = vec![1, 2, 3, 4, 5];
        if out_above(m, 2.69) {
            t.extend([6, 7]);
        }
        if out_above(m, 3.0) {
            t.push(0x208);
        }
        put(m, o, 0x1d, &t);
    }
    // 30: the NURBS surface
    if out_above(m, 2.65) {
        let mut t: Vec<u16> = (1..=7).collect();
        if out_above(m, 2.69) {
            t.extend(8..=0x19);
            if out_above(m, 3.0905) {
                t.push(0xff1a);
            }
        }
        put(m, o, 0x1e, &t);
    }
    // 31: the composite curve
    if out_above(m, 2.65) {
        put(m, o, 0x1f, &[1, 2]);
    }
    // the end of the table
    m.set_i16(local, 0);
    byteswap_fwrite(m, o, local, 2, 1, file);
    m.release(FRAME);
}
