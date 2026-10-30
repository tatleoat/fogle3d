// SPDX-License-Identifier: MIT
//! The prototype block (`PROT`) that opens the files holding geometry (models, shapes,
//! clusters): for each kind of record, the order in which its fields are stored. Finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `utilio.c`.
//!
//! A geometry record has no tokens in front of its fields. Instead the file states once, up
//! front, which fields each kind of record has and in which order: a *prototype*, a list of
//! field numbers ending with zero. A reader walks the list and reads each field in turn; a
//! field number it does not know is stepped over by the number's size byte, as for tokens.

use crate::io::{fread_i16, Reader};

/// `PROT`
pub const TAG: u32 = u32::from_be_bytes(*b"PROT");

/// The prototypes of one file (`si_Prototypes`, 0x70 bytes: 28 list pointers).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prototypes {
    /// By slot: the list at offset `4 × slot` of the original record. `None` is a null
    /// pointer: a reader then takes its field numbers from the file, one before each field.
    pub slots: [Option<Vec<i16>>; 28],
}

/// The slot of a prototype id: ids 3 and 0x12 have the first two, 5 to 0x11 and 0x13 to 0x1f
/// follow in order. Other ids (1, 2, 4, above 0x1f) have none: their lists are read and
/// dropped.
pub fn slot_of(id: i16) -> Option<usize> {
    match id {
        3 => Some(0),
        0x12 => Some(1),
        5..=0x11 => Some(id as usize - 3),
        0x13..=0x1f => Some(id as usize - 4),
        _ => None,
    }
}

impl Prototypes {
    /// An empty set [soft 0x083feddc si_prototypeAllocate].
    pub fn new() -> Prototypes {
        Prototypes::default()
    }

    /// The list for a prototype id, if the file gave one.
    pub fn by_id(&self, id: i16) -> Option<&[i16]> {
        slot_of(id).and_then(|s| self.slots[s].as_deref())
    }

    /// The list at a byte offset of the original record (as the readers address it).
    pub fn by_offset(&self, offset: usize) -> Option<&[i16]> {
        self.slots.get(offset / 4).and_then(|s| s.as_deref())
    }
}

/// Reads the prototype lists that follow the `PROT` tag
/// [soft 0x083fdc40 si_prototypeBinaryRead]: per list a 16-bit id, a 16-bit count and that
/// many 16-bit field numbers (the last of them the closing zero); an id of zero ends the
/// block, and so does the end of the file.
///
/// A list given twice replaces the earlier one (which the original leaks). A count larger
/// than the rest of the file is cut to it and the reader's `clamped` is set (the original
/// allocates by the count; a negative count makes it allocate nothing useful and then write
/// through the null pointer or loop zero times).
pub fn read_prototypes(r: &mut Reader, p: &mut Prototypes) {
    let mut id = 0i16;
    fread_i16(&mut r.s, &mut id);
    loop {
        if id == 0 {
            return;
        }
        let mut count = 0i16;
        fread_i16(&mut r.s, &mut count);
        let want = count.max(0) as usize;
        let have = (r.s.len() - r.s.tell()) / 2;
        if want > have {
            r.clamped = true;
        }
        let mut list = vec![0i16; want.min(have)];
        for v in list.iter_mut() {
            fread_i16(&mut r.s, v);
        }
        if want > have {
            r.s.seek_end();
            r.s.getc();
        }
        if let Some(s) = slot_of(id) {
            p.slots[s] = Some(list);
        }
        if !fread_i16(&mut r.s, &mut id) {
            return;
        }
    }
}

/// Walks a prototype list the way every record reader does: the field numbers in order up to
/// the closing zero. The original indexes the list without a bound, so a list with no zero
/// runs on into whatever follows it in memory; here the end of the list ends the walk.
pub fn fields(list: &[i16]) -> impl Iterator<Item = i16> + '_ {
    list.iter().copied().take_while(|&f| f != 0)
}

use crate::io::Writer;

/// The prototypes this build writes for an output version, in the order it writes them:
/// each id with its field numbers, the closing zero included
/// [soft 0x083fe0ac SI_prototypeBinaryWrite].
///
/// The version is the float `SI_versionOutput` compared with double constants (0x0865c638
/// to 0x0865c698); "above" is a strict comparison, "from" includes the constant.
///
/// | id | record | always | by version |
/// |---|---|---|---|
/// | 5 | hierarchy | 1 2 3 0xc 4 5 6 7 8 9 0xe 0x10 to 0x16 … 0xff1d to 0xff22 … 0x224 0xa 0xb | above 2.65: 0x17 0x18; above 2.69904: 0x19, and with it from 3.089: 0x41a 0x21b; from 3.49089: 0xff1c; (then 0xff1d to 0xff22); from 3.93: 0xff24; from 3.8109: 0x221 0x222; from 3.8209: 0x223 |
/// | 3 | spline | 8 1 7 3 4 5 6 | |
/// | 0x12 | spline key | 1 2 3 4 5 6 | |
/// | 6 | patch | 1 to 0x10 | above 2.65: 0x11 to 0x19 |
/// | 7 | face | | above 2.99903: 3 1 2 4; otherwise 1 2 |
/// | 8 | mesh | 6 2 … 7 8 9 | from 3.93: 0x413 (after 2); from 3.8109: 0x20a 0x20b 0x20c 0x40d 0x40e 0x40f 0x210 0x211 0x212 |
/// | 0xc | vertex | 1 4 | |
/// | 0xa | polygon | 2 3 | above 2.65: 4 |
/// | 0xb | polygon node | 3 2 4 0x405 | from 3.93: 0xff06 |
/// | 9 | edge | 4 3 | |
/// | 0xd | material | 1 2 0xe 0xf 3 4 5 6 7 0xd 0xb 0x10 to 0x14 … 0x218 0xff19 … 0x21b 0x21c 0x21d | above 2.65: 0x15 0x16 0x17; from 3.8109: 0xff1a |
/// | 0xe | texture | 1 to 0x10, 0x12, 0x14 to 0x1f … 0xff35 0x238 | above 2.62: 0x20 to 0x23, and with them above 2.65: 0x24 0x25 0x32 0x233 0x234, and with those above 3.5: 0x235 0x236 0x237 |
/// | 0xf | 3D texture | 1 2 3 4 5 0x16 to 0x1a 0xa 0x1b 0x1c 0xb to 0x13 0x15 0x1d 0x1f 0x20 0x21 … 0xff24 | above 2.65: 0x22 0x23 |
/// | 0x13 | dynamic matter | 1 to 7 | |
/// | 0x14 | joint | 1 | |
/// | 0x15 | envelope | 1 5 4 7 8 9 0xb to 0x12 0x14 0x15 | |
/// | 0x16 | envelope vertex | 1 3 4 6 | |
/// | 0x17 | cluster | 1 2 3 4 | |
/// | 0x18 | metaball | 4 5 6 7 | |
/// | 0x19 | metasystem | 1 2 3 4 5 | |
/// | 0x1a | metagroup | 1 2 | |
/// | 0x1b | meta unblend | 1 | |
/// | 0x1c | custom shader | | above 2.65: 1 2 3 4 |
/// | 0x1d | NURBS curve | | above 2.65: 1 2 3 4 5, with above 2.69: 6 7, and above 3.0: 0x208 |
/// | 0x1e | NURBS surface | | above 2.65: 1 to 7, and with them above 2.69: 8 to 0x19, and with those above 3.0905: 0xff1a |
/// | 0x1f | NURBS composite curve | | above 2.65: 1 2 |
///
/// The record names are those of the SDK's page on the hierarchy file ("Content of the
/// Prototype Block in v3.5").
pub fn output_lists(version_output: f32) -> Vec<(i16, Vec<i16>)> {
    let v = version_output as f64;
    let mut out: Vec<(i16, Vec<i16>)> = Vec::new();
    let mut list = |id: i16, parts: &[(bool, &[u16])]| {
        let mut l: Vec<i16> = Vec::new();
        for &(on, numbers) in parts {
            if on {
                l.extend(numbers.iter().map(|&n| n as i16));
            }
        }
        l.push(0);
        out.push((id, l));
    };
    list(
        5,
        &[
            (true, &[1, 2, 3, 0xc, 4, 5, 6, 7, 8, 9, 0xe, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16]),
            (v > 2.65, &[0x17, 0x18]),
            (v > 2.69904, &[0x19]),
            // tested only when the test before it passed, which a pass of this one implies
            (v >= 3.089, &[0x41a, 0x21b]),
            (v >= 3.49089, &[0xff1c]),
            (true, &[0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22]),
            (v >= 3.93, &[0xff24]),
            (v >= 3.8109, &[0x221, 0x222]),
            (v >= 3.8209, &[0x223]),
            (true, &[0x224, 0xa, 0xb]),
        ],
    );
    list(3, &[(true, &[8, 1, 7, 3, 4, 5, 6])]);
    list(0x12, &[(true, &[1, 2, 3, 4, 5, 6])]);
    list(
        6,
        &[
            (true, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10]),
            (v > 2.65, &[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19]),
        ],
    );
    let face_new = v > 2.99903;
    list(7, &[(face_new, &[3, 1, 2, 4]), (!face_new, &[1, 2])]);
    list(
        8,
        &[
            (true, &[6, 2]),
            (v >= 3.93, &[0x413]),
            (true, &[7, 8, 9]),
            (v >= 3.8109, &[0x20a, 0x20b, 0x20c, 0x40d, 0x40e, 0x40f, 0x210, 0x211, 0x212]),
        ],
    );
    list(0xc, &[(true, &[1, 4])]);
    list(0xa, &[(true, &[2, 3]), (v > 2.65, &[4])]);
    list(0xb, &[(true, &[3, 2, 4, 0x405]), (v >= 3.93, &[0xff06])]);
    list(9, &[(true, &[4, 3])]);
    list(
        0xd,
        &[
            (true, &[1, 2, 0xe, 0xf, 3, 4, 5, 6, 7, 0xd, 0xb, 0x10, 0x11, 0x12, 0x13, 0x14]),
            (v > 2.65, &[0x15, 0x16, 0x17]),
            (true, &[0x218, 0xff19]),
            (v >= 3.8109, &[0xff1a]),
            (true, &[0x21b, 0x21c, 0x21d]),
        ],
    );
    list(
        0xe,
        &[
            (true, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x12, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f]),
            (v > 2.62, &[0x20, 0x21, 0x22, 0x23]),
            // each of the next two is tested only when the one before it passed, which its
            // own pass implies
            (v > 2.65, &[0x24, 0x25, 0x32, 0x233, 0x234]),
            (v > 3.5, &[0x235, 0x236, 0x237]),
            (true, &[0xff35, 0x238]),
        ],
    );
    list(
        0xf,
        &[
            (true, &[1, 2, 3, 4, 5, 0x16, 0x17, 0x18, 0x19, 0x1a, 0xa, 0x1b, 0x1c, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x13, 0x15, 0x1d, 0x1f, 0x20, 0x21]),
            (v > 2.65, &[0x22, 0x23]),
            (true, &[0xff24]),
        ],
    );
    list(0x13, &[(true, &[1, 2, 3, 4, 5, 6, 7])]);
    list(0x14, &[(true, &[1])]);
    list(0x15, &[(true, &[1, 5, 4, 7, 8, 9, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x14, 0x15])]);
    list(0x16, &[(true, &[1, 3, 4, 6])]);
    list(0x17, &[(true, &[1, 2, 3, 4])]);
    list(0x18, &[(true, &[4, 5, 6, 7])]);
    list(0x19, &[(true, &[1, 2, 3, 4, 5])]);
    list(0x1a, &[(true, &[1, 2])]);
    list(0x1b, &[(true, &[1])]);
    if v > 2.65 {
        list(0x1c, &[(true, &[1, 2, 3, 4])]);
        list(0x1d, &[(true, &[1, 2, 3, 4, 5]), (v > 2.69, &[6, 7]), (v > 3.0, &[0x208])]);
        list(
            0x1e,
            &[
                (true, &[1, 2, 3, 4, 5, 6, 7]),
                (v > 2.69, &[8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19]),
                // tested only when the test before it passed, which a pass of this one implies
                (v > 3.0905, &[0xff1a]),
            ],
        );
        list(0x1f, &[(true, &[1, 2])]);
    }
    out
}

/// Writes the prototype block [soft 0x083fe0ac SI_prototypeBinaryWrite]: the four characters
/// `PROT`, then for each prototype of [`output_lists`] its id, the count of its numbers (the
/// closing zero included) and the numbers, all 16-bit; a zero id ends the block.
///
/// The block is the same in every file a build writes for one output version, whatever the
/// file holds: a shape file of splines also carries the lists of materials and textures.
pub fn write_prototypes(w: &mut Writer) {
    w.out.extend_from_slice(b"PROT");
    for (id, list) in output_lists(w.version_output) {
        w.put_u16(id as u16);
        w.put_u16(list.len() as u16);
        for n in list {
            w.put_u16(n as u16);
        }
    }
    w.put_u16(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        assert_eq!(slot_of(3), Some(0));
        assert_eq!(slot_of(0x12), Some(1));
        assert_eq!(slot_of(5), Some(2));
        assert_eq!(slot_of(0x11), Some(14));
        assert_eq!(slot_of(0x13), Some(15));
        assert_eq!(slot_of(0x1c), Some(24)); // offset 0x60: the custom shader's list
        assert_eq!(slot_of(0x1f), Some(27));
        for id in [0, 1, 2, 4, 0x20, -1] {
            assert_eq!(slot_of(id), None);
        }
    }

    #[test]
    fn block() {
        // id 3 with three numbers, an unknown id 4, id 0x12, the closing zero, then data
        let b = [0, 3, 0, 3, 0, 8, 0, 1, 0, 0, 0, 4, 0, 1, 0, 0, 0, 0x12, 0, 2, 0, 5, 0, 0, 0, 0, 9, 9];
        let mut r = Reader::new(&b);
        let mut p = Prototypes::new();
        read_prototypes(&mut r, &mut p);
        assert_eq!(r.s.tell(), b.len() - 2);
        assert_eq!(p.by_id(3), Some(&[8i16, 1, 0][..]));
        assert_eq!(p.by_offset(4), Some(&[5i16, 0][..]));
        assert_eq!(p.by_id(4), None);
        assert_eq!(fields(p.by_id(3).unwrap()).collect::<Vec<_>>(), [8, 1]);
        // the end of the file ends the block
        let mut r = Reader::new(&b[..10]);
        let mut p = Prototypes::new();
        read_prototypes(&mut r, &mut p);
        assert!(p.by_id(3).is_some());
        // a count beyond the file
        let mut r = Reader::new(&[0, 3, 0x7f, 0xff, 0, 1]);
        let mut p = Prototypes::new();
        read_prototypes(&mut r, &mut p);
        assert!(r.clamped);
        assert_eq!(p.by_id(3), Some(&[1i16][..]));
    }

    fn written(version: f32, id: i16) -> Option<Vec<u16>> {
        output_lists(version).into_iter().find(|(i, _)| *i == id).map(|(_, l)| l.into_iter().map(|n| n as u16).collect())
    }

    #[test]
    fn the_block_this_build_writes() {
        let mut w = Writer::new();
        write_prototypes(&mut w);
        assert_eq!(&w.out[..4], b"PROT");
        // the first list: id 5 with its count, the closing zero included
        let hierarchy = written(4.0, 5).unwrap();
        assert_eq!(&w.out[4..8], &[0, 5, 0, hierarchy.len() as u8]);
        assert_eq!(hierarchy.len(), 38);
        assert_eq!(&hierarchy[20..], &[0x19, 0x41a, 0x21b, 0xff1c, 0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22, 0xff24, 0x221, 0x222, 0x223, 0x224, 0xa, 0xb, 0]);
        assert_eq!(&w.out[w.out.len() - 2..], &[0, 0]);
        // it reads back whole, into the slots the record readers use
        let mut r = Reader::new(&w.out[4..]);
        let mut p = Prototypes::new();
        read_prototypes(&mut r, &mut p);
        assert_eq!(r.s.tell(), w.out.len() - 4);
        let ids: Vec<i16> = output_lists(4.0).iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [5, 3, 0x12, 6, 7, 8, 0xc, 0xa, 0xb, 9, 0xd, 0xe, 0xf, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f]);
        for (id, list) in output_lists(4.0) {
            assert_eq!(p.by_id(id), Some(&list[..]), "id {id:#x}");
        }
        assert_eq!(p.by_offset(0x14), Some(&[6i16, 2, 0x413, 7, 8, 9, 0x20a, 0x20b, 0x20c, 0x40d, 0x40e, 0x40f, 0x210, 0x211, 0x212, 0][..]));
        assert_eq!(fields(p.by_offset(0x20).unwrap()).map(|n| n as u16).collect::<Vec<_>>(), [3, 2, 4, 0x405, 0xff06]);
    }

    #[test]
    fn the_lists_by_output_version() {
        // the float 2.65 is above the double it is compared with; 2.64 is not
        assert_eq!(written(2.65, 6).unwrap().len(), 26);
        assert_eq!(written(2.64, 6).unwrap().len(), 17);
        assert_eq!(written(2.64, 0xa).unwrap(), [2, 3, 0]);
        assert_eq!(written(2.65, 0xa).unwrap(), [2, 3, 4, 0]);
        assert!(written(2.64, 0x1c).is_none() && written(2.64, 0x1f).is_none());
        assert_eq!(output_lists(2.64).len(), 22);
        // the face: two fields up to 2.99903, four above
        assert_eq!(written(2.9, 7).unwrap(), [1, 2, 0]);
        assert_eq!(written(3.0, 7).unwrap(), [3, 1, 2, 4, 0]);
        // mesh and node
        assert_eq!(written(3.9, 8).unwrap(), [6, 2, 7, 8, 9, 0x20a, 0x20b, 0x20c, 0x40d, 0x40e, 0x40f, 0x210, 0x211, 0x212, 0]);
        assert_eq!(written(3.8, 8).unwrap(), [6, 2, 7, 8, 9, 0]);
        assert_eq!(written(3.9, 0xb).unwrap(), [3, 2, 4, 0x405, 0]);
        assert_eq!(written(2.0, 0xb).unwrap(), [3, 2, 4, 0x405, 0]);
        // the hierarchy's nested and independent tests
        assert_eq!(&written(3.0, 5).unwrap()[18..], &[0x17, 0x18, 0x19, 0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22, 0x224, 0xa, 0xb, 0]);
        assert_eq!(&written(3.5, 5).unwrap()[21..25], &[0x41a, 0x21b, 0xff1c, 0xff1d]);
        assert_eq!(&written(2.0, 5).unwrap()[18..], &[0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22, 0x224, 0xa, 0xb, 0]);
        // material, texture, the NURBS records
        assert_eq!(&written(3.8, 0xd).unwrap()[19..], &[0x218, 0xff19, 0x21b, 0x21c, 0x21d, 0]);
        assert_eq!(&written(4.0, 0xd).unwrap()[19..], &[0x218, 0xff19, 0xff1a, 0x21b, 0x21c, 0x21d, 0]);
        assert_eq!(&written(2.6, 0xe).unwrap()[29..], &[0xff35, 0x238, 0]);
        assert_eq!(&written(2.64, 0xe).unwrap()[29..], &[0x20, 0x21, 0x22, 0x23, 0xff35, 0x238, 0]);
        assert_eq!(&written(3.6, 0xe).unwrap()[33..], &[0x24, 0x25, 0x32, 0x233, 0x234, 0x235, 0x236, 0x237, 0xff35, 0x238, 0]);
        assert_eq!(written(2.68, 0x1d).unwrap(), [1, 2, 3, 4, 5, 0]);
        assert_eq!(written(3.0, 0x1d).unwrap(), [1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!(written(3.01, 0x1d).unwrap(), [1, 2, 3, 4, 5, 6, 7, 0x208, 0]);
        assert_eq!(written(2.68, 0x1e).unwrap(), [1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!(written(3.09, 0x1e).unwrap().len(), 26);
        assert_eq!(written(3.1, 0x1e).unwrap().last_chunk::<2>(), Some(&[0xff1a, 0]));
    }
}
