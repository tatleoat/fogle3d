// SPDX-License-Identifier: MIT
//! The prototype block: for each kind of record, the list of tokens a file stores for it
//! (source file `utilio.c`).

use crate::io::{Rd, Wr};

pub const SPLINE: usize = 3;
pub const HIERARCHY: usize = 5;
pub const PATCH: usize = 6;
pub const FACE: usize = 7;
pub const MESH: usize = 8;
pub const EDGE: usize = 9;
pub const POLYGON: usize = 10;
pub const POLY_NODE: usize = 11;
pub const VERTEX: usize = 12;
pub const MATERIAL: usize = 13;
pub const TEXTURE: usize = 14;
pub const TEXTURE_3D: usize = 15;
pub const SPLINE_KEY: usize = 18;
pub const DYNA_MATTER: usize = 19;
pub const JOINT: usize = 20;
pub const ENVELOP: usize = 21;
pub const ENVELOP_VERTEX: usize = 22;
pub const CLUSTER: usize = 23;
pub const METABALL: usize = 24;
pub const METASYSTEM: usize = 25;
pub const METAGROUP: usize = 26;
pub const METAUNBLEND: usize = 27;
pub const UDF: usize = 28;
pub const NURBS_CURVE: usize = 29;
pub const NURBS_SURFACE: usize = 30;
pub const NURBS_COMPOSITE: usize = 31;

pub fn name(id: usize) -> &'static str {
    match id {
        SPLINE => "Spline",
        HIERARCHY => "Hierarchy",
        PATCH => "Patch",
        FACE => "Face",
        MESH => "Mesh",
        EDGE => "Edge",
        POLYGON => "Polygon",
        POLY_NODE => "Poly_node",
        VERTEX => "Vertex",
        MATERIAL => "Material",
        TEXTURE => "Texture",
        TEXTURE_3D => "3D texture",
        16 => "(16)",
        17 => "(17)",
        SPLINE_KEY => "Spline key",
        DYNA_MATTER => "Dyna_matter",
        JOINT => "Joint",
        ENVELOP => "Envelop",
        ENVELOP_VERTEX => "Envelop vertex",
        CLUSTER => "Cluster",
        METABALL => "Metaball",
        METASYSTEM => "Metasystem",
        METAGROUP => "Metagroup",
        METAUNBLEND => "Metaunblend",
        UDF => "Udf",
        NURBS_CURVE => "Nurbs curve",
        NURBS_SURFACE => "Nurbs surface",
        NURBS_COMPOSITE => "Nurbs composite curve",
        _ => "?",
    }
}

/// The original's table of 28 pointers, one token list per prototype id 3 and 5 to 31
/// [soft 0x083feddc si_prototypeAllocate] [soft 0x083fedf8 si_prototypeDispose], plus the
/// block as the file had it, so that it can be written back unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prototypes {
    lists: [Option<Vec<i16>>; 32],
    /// Every entry of the block in file order: id, stored count, tokens read.
    pub block: Vec<(i16, i16, Vec<i16>)>,
}

impl Prototypes {
    pub fn get(&self, id: usize) -> Option<&[i16]> {
        self.lists[id].as_deref()
    }

    /// Token number `i` of a prototype. A list with no terminating zero is read past its
    /// end by the original (whatever follows it on the heap); here it ends.
    pub fn token(&self, id: usize, i: usize) -> Option<i16> {
        self.lists[id].as_ref().map(|l| l.get(i).copied().unwrap_or(0))
    }

    fn set(&mut self, id: i16, count: i16, tokens: Vec<i16>) {
        // ids 3 and 5 to 31 have a slot; a list for any other id is read and thrown away
        if id == 3 || (5..=31).contains(&id) {
            self.lists[id as usize] = Some(tokens.clone());
        }
        self.block.push((id, count, tokens));
    }

    /// Reads the block that follows the `PROT` tag [soft 0x083fdc40 si_prototypeBinaryRead]:
    /// id, count, `count` tokens, repeated until an id of 0 or the end of the file.
    ///
    /// A list is `calloc(count, 2)`, so tokens the file ends before stay zero. A count the
    /// rest of the file cannot hold is cut to what could be there plus the zero that ends
    /// it (the original allocates the full count). A second list for the same id replaces
    /// the first (which the original leaks).
    pub fn read(r: &mut Rd) -> Prototypes {
        let mut p = Prototypes::default();
        let mut id = 0i16; // uninitialised in the original if the very first read fails
        r.i16(&mut id);
        while id != 0 {
            let mut count = 0i16;
            r.i16(&mut count);
            let n = (count.max(0) as usize).min(r.left() / 2 + 1);
            let mut tokens = vec![0i16; n];
            for t in tokens.iter_mut() {
                r.i16(t);
            }
            p.set(id, count, tokens);
            if !r.i16(&mut id) {
                break;
            }
        }
        p
    }

    /// The block exactly as read (without the `PROT` tag).
    pub fn write_as_read(&self, w: &mut Wr) {
        for (id, count, tokens) in &self.block {
            w.i16(*id);
            w.i16(*count);
            for t in tokens {
                w.i16(*t);
            }
        }
        w.i16(0);
    }

    /// The table the program writes [soft 0x083fe0ac SI_prototypeBinaryWrite], as a function
    /// of the output version (the global `SI_versionOutput`, 4.0 in this build). The
    /// thresholds are doubles in the original, compared as floats except in the two places
    /// marked.
    pub fn current(v: f32) -> Prototypes {
        let above = |c: f64| (c as f32) < v;
        let from = |c: f64| (c as f32) <= v;
        let mut p = Prototypes::default();
        let mut put = |id: usize, mut t: Vec<u16>| {
            t.push(0);
            let t: Vec<i16> = t.into_iter().map(|x| x as i16).collect();
            p.set(id as i16, t.len() as i16, t);
        };

        let mut h: Vec<u16> = vec![1, 2, 3, 0xc, 4, 5, 6, 7, 8, 9, 0xe, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16];
        if above(2.65) {
            h.extend([0x17, 0x18]);
        }
        if above(2.69904) {
            h.push(0x19);
            if from(3.089) {
                h.extend([0x41a, 0x21b]);
            }
        }
        if from(3.49089) {
            h.push(0xff1c);
        }
        h.extend([0xff1d, 0xff1e, 0xff1f, 0xff20, 0xff21, 0xff22]);
        if from(3.93) {
            h.push(0xff24);
        }
        if from(3.8109) {
            h.extend([0x221, 0x222]);
        }
        if from(3.8209) {
            h.push(0x223);
        }
        h.extend([0x224, 10, 11]);
        put(HIERARCHY, h);

        put(SPLINE, vec![8, 1, 7, 3, 4, 5, 6]);
        put(SPLINE_KEY, vec![1, 2, 3, 4, 5, 6]);

        let mut t: Vec<u16> = (1..=0x10).collect();
        if above(2.65) {
            t.extend(0x11..=0x19);
        }
        put(PATCH, t);

        put(FACE, if v <= 2.99903f64 as f32 { vec![1, 2] } else { vec![3, 1, 2, 4] });

        let mut t = vec![6, 2];
        if from(3.93) {
            t.push(0x413);
        }
        t.extend([7, 8, 9]);
        if from(3.8109) {
            t.extend([0x20a, 0x20b, 0x20c, 0x40d, 0x40e, 0x40f, 0x210, 0x211, 0x212]);
        }
        put(MESH, t);

        put(VERTEX, vec![1, 4]);

        let mut t = vec![2, 3];
        if above(2.65) {
            t.push(4);
        }
        put(POLYGON, t);

        let mut t = vec![3, 2, 4, 0x405];
        if from(3.93) {
            t.push(0xff06);
        }
        put(POLY_NODE, t);

        put(EDGE, vec![4, 3]);

        let mut t = vec![1, 2, 0xe, 0xf, 3, 4, 5, 6, 7, 0xd, 0xb, 0x10, 0x11, 0x12, 0x13, 0x14];
        if above(2.65) {
            t.extend([0x15, 0x16, 0x17]);
        }
        t.extend([0x218, 0xff19]);
        if 3.8109f64 <= v as f64 {
            t.push(0xff1a); // compared as doubles
        }
        t.extend([0x21b, 0x21c, 0x21d]);
        put(MATERIAL, t);

        let mut t: Vec<u16> = (1..=0x10).collect();
        t.push(0x12);
        t.extend(0x14..=0x1f);
        if above(2.62) {
            t.extend([0x20, 0x21, 0x22, 0x23]);
            if above(2.65) {
                t.extend([0x24, 0x25, 0x32, 0x233, 0x234]);
                if above(3.5) {
                    t.extend([0x235, 0x236, 0x237]);
                }
            }
        }
        t.extend([0xff35, 0x238]);
        put(TEXTURE, t);

        let mut t = vec![1, 2, 3, 4, 5, 0x16, 0x17, 0x18, 0x19, 0x1a, 10, 0x1b, 0x1c, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10,
                         0x11, 0x12, 0x13, 0x15, 0x1d, 0x1f, 0x20, 0x21];
        if above(2.65) {
            t.extend([0x22, 0x23]);
        }
        t.push(0xff24);
        put(TEXTURE_3D, t);

        put(DYNA_MATTER, vec![1, 2, 3, 4, 5, 6, 7]);
        put(JOINT, vec![1]);
        put(ENVELOP, vec![1, 5, 4, 7, 8, 9, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x14, 0x15]);
        put(ENVELOP_VERTEX, vec![1, 3, 4, 6]);
        put(CLUSTER, vec![1, 2, 3, 4]);
        put(METABALL, vec![4, 5, 6, 7]);
        put(METASYSTEM, vec![1, 2, 3, 4, 5]);
        put(METAGROUP, vec![1, 2]);
        put(METAUNBLEND, vec![1]);
        if above(2.65) {
            put(UDF, vec![1, 2, 3, 4]);
        }
        if 2.65f64 < v as f64 {
            // compared as doubles
            let mut t = vec![1, 2, 3, 4, 5];
            if above(2.69) {
                t.extend([6, 7]);
            }
            if above(3.0) {
                t.push(0x208);
            }
            put(NURBS_CURVE, t);
        }
        if above(2.65) {
            let mut t: Vec<u16> = (1..=7).collect();
            if above(2.69) {
                t.extend(8..=0x19);
                if above(3.0905) {
                    t.push(0xff1a);
                }
            }
            put(NURBS_SURFACE, t);
            put(NURBS_COMPOSITE, vec![1, 2]);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_table_for_this_build() {
        let p = Prototypes::current(4.0);
        let hex = |id: usize| p.get(id).unwrap().iter().map(|t| format!("{:x}", *t as u16)).collect::<Vec<_>>().join(" ");
        assert_eq!(hex(HIERARCHY), "1 2 3 c 4 5 6 7 8 9 e 10 11 12 13 14 15 16 17 18 19 41a 21b ff1c ff1d ff1e ff1f ff20 ff21 ff22 ff24 221 222 223 224 a b 0");
        assert_eq!(hex(MESH), "6 2 413 7 8 9 20a 20b 20c 40d 40e 40f 210 211 212 0");
        assert_eq!(hex(POLY_NODE), "3 2 4 405 ff06 0");
        assert_eq!(hex(MATERIAL), "1 2 e f 3 4 5 6 7 d b 10 11 12 13 14 15 16 17 218 ff19 ff1a 21b 21c 21d 0");
        assert_eq!(hex(NURBS_SURFACE).split(' ').count(), 27);
        // 26 prototypes, in the order the program writes them
        let ids: Vec<i16> = p.block.iter().map(|b| b.0).collect();
        assert_eq!(ids, [5, 3, 18, 6, 7, 8, 12, 10, 11, 9, 13, 14, 15, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]);
        // the count stored with a list includes its terminating zero
        assert!(p.block.iter().all(|(_, n, t)| *n as usize == t.len() && t.last() == Some(&0)));
    }

    #[test]
    fn older_output_versions_drop_tokens() {
        let p = Prototypes::current(2.6);
        assert!(p.get(UDF).is_none() && p.get(NURBS_CURVE).is_none());
        assert_eq!(p.get(FACE).unwrap(), [1, 2, 0]);
        assert_eq!(p.get(POLYGON).unwrap(), [2, 3, 0]);
    }

    #[test]
    fn block_round_trip_and_odd_ids() {
        let mut w = Wr::default();
        // an id without a slot (4), a known one, a second list for the same id, the end
        for v in [4i16, 2, 9, 0, 3, 3, 8, 1, 0, 3, 2, 7, 0, 0] {
            w.i16(v);
        }
        let mut r = Rd::new(&w.out);
        let p = Prototypes::read(&mut r);
        assert_eq!(r.s.tell(), w.out.len());
        assert_eq!(p.get(SPLINE).unwrap(), [7, 0]); // the later list wins
        assert_eq!(p.block.len(), 3);
        let mut back = Wr::default();
        p.write_as_read(&mut back);
        assert_eq!(back.out, w.out);
        // a count larger than the file: zeros from calloc, nothing more
        let mut r = Rd::new(&[0, 3, 0x7f, 0xff, 0, 8]);
        let p = Prototypes::read(&mut r);
        assert_eq!(p.get(SPLINE).unwrap()[0], 8);
        assert!(p.get(SPLINE).unwrap().len() <= 2);
    }
}
