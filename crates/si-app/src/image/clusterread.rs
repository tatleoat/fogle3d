// SPDX-License-Identifier: MIT
//! The ascii cluster reader of `asceneio.c` (finding 0019, section 13d): one animated
//! node's block of a `.cls` cluster file, and the block of keys inside it.
//!
//! The reader is a loop over words: each word names what follows (`name`, `numCVs`,
//! `numKeys`, `indexes`, `Cluster_key`) or ends the node (`End`); a word it does not know is
//! reported on the status line and ends the node as well. Everything is read with the C
//! library's `fscanf`, which here is a call out of the image ([`FSCANF`]) that stores its
//! results through the pointers it is given; the locals those pointers point into are
//! frames on the image's stack, so a value that one `fscanf` left in a local is still
//! there when a later one fails, as in the original.
//!
//! The cluster allocators, `SI_klusterGetByName`, `SI_clusterDataDispose` and
//! `SI_statusDisplayMessage` belong to other source files and are reached through
//! [`Outside::call`] (`si-elemio`, `si-deform`, `si-anim` hold their ports; this crate
//! cannot depend on them without a cycle). The key reader moves each float through
//! `fld`/`fstp`, so a signalling NaN comes out quiet ([`super::fileswap::quiet`]).
//!
//! Where Rust differs: the original's local `indexes` count stays uninitialised for a
//! model whose type word (+0x30) is not 1, 2, 4 or 6 (and the floats of a key stay
//! uninitialised when the first `fscanf` of a record fails); here they start at 0.
//! The message buffer of an unknown word is 256 bytes in the original and the format adds
//! 50 characters to a word of up to 255, which overruns it into the word's own buffer; the
//! message buffer here is as long as the text needs (the word is not read again).

use super::fileswap::quiet;
use si_mem::{Mem, Outside, Routine};

/// `fscanf (0x080c4058)`, the C library's.
pub const FSCANF: Routine = Routine::new("fscanf", 0x080c_4058);
const SI_CLUSTER_KEY_ALLOCATE: Routine = Routine::new("SI_clusterKeyAllocate", 0x081e_7f78);
const SI_CLUSTER_ALLOCATE: Routine = Routine::new("SI_clusterAllocate", 0x081e_7e40);
const SI_CLUSTER_DATA_ALLOCATE: Routine = Routine::new("SI_clusterDataAllocate", 0x081e_7ecc);
const SI_CLUSTER_DATA_DISPOSE: Routine = Routine::new("SI_clusterDataDispose", 0x081e_7f0c);
const SI_KLUSTER_GET_BY_NAME: Routine = Routine::new("SI_klusterGetByName", 0x0843_b0b4);
const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);

/// The formats of `asceneio.c` these two routines read with (in `.rodata`, 0x086cbd2c to 0x086cbda0).
pub mod fmt {
    pub const G3_TYPE4: u32 = 0x086c_bd2c;
    pub const G4: u32 = 0x086c_bd35;
    pub const G3_A: u32 = 0x086c_bd41;
    pub const G3_B: u32 = 0x086c_bd4a;
    pub const G3_C: u32 = 0x086c_bd53;
    pub const WORD: u32 = 0x086c_bd5c;
    pub const NAME_WORD: u32 = 0x086c_bd64;
    pub const INT_CVS: u32 = 0x086c_bd6e;
    pub const INT_KEYS: u32 = 0x086c_bd79;
    pub const INT_INDEX: u32 = 0x086c_bd84;
}

/// The `.rodata` texts of the two routines (address, bytes with the terminator).
pub const TEXTS: &[(u32, &[u8])] = &[
    (fmt::G3_TYPE4, b"%g %g %g\0"),
    (fmt::G4, b"%g %g %g %g\0"),
    (fmt::G3_A, b"%g %g %g\0"),
    (fmt::G3_B, b"%g %g %g\0"),
    (fmt::G3_C, b"%g %g %g\0"),
    (fmt::WORD, b"%s\0"),
    (0x086c_bd5f, b"name\0"),
    (fmt::NAME_WORD, b"%s\0"),
    (0x086c_bd67, b"numCVs\0"),
    (fmt::INT_CVS, b"%d\0"),
    (0x086c_bd71, b"numKeys\0"),
    (fmt::INT_KEYS, b"%d\0"),
    (0x086c_bd7c, b"indexes\0"),
    (fmt::INT_INDEX, b"%d\0"),
    (0x086c_bd87, b"Cluster_key\0"),
    (0x086c_bd93, b"End\0"),
    (0x086c_bda0, b"ClusterAnimNodeAsciiRead: Error, unknown token %s\0"),
];

/// Stores three or four floats through `fld`/`fstp` at `at` and answers the next address.
fn put_floats(m: &mut Mem, at: u32, from: u32, n: u32) -> u32 {
    for k in 0..n {
        let v = quiet(m.u32(from + 4 * k));
        m.set_u32(at + 4 * k, v);
    }
    at + 4 * n
}

/// [soft 0x083dda00 clusterKeyAsciiRead]: reads `count` keys of a cluster of type `kind`
/// from the stream and appends them as a new key block to the end of the list at +0x18 of
/// `data`. A type of 1 reads four floats a key (stored in the order x y z w); 4 reads three;
/// 2 and 6 read three records of three floats; any other type reads nothing. The block is
/// the one `SI_clusterKeyAllocate(count, kind)` made (its first word is the key array); when
/// there is no `data` or the allocation fails the numbers are read and dropped. 0 for no
/// stream, else 1.
pub fn cluster_key_ascii_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, data: u32, count: i32, kind: i32) -> i32 {
    if stream == 0 {
        return 0;
    }
    // the four locals the `fscanf`s store into: x y z at +0, +4, +8 and w at +12
    let frame = m.local(16);
    let (mut four, mut three) = (0u32, 0u32);
    if data != 0 {
        let block = o.call(m, SI_CLUSTER_KEY_ALLOCATE, &[count as u32, kind as u32]).eax;
        if block != 0 {
            let mut at = m.u32(data + 0x18);
            if at == 0 {
                m.set_u32(data + 0x18, block);
            } else {
                while m.u32(at + 4) != 0 {
                    at = m.u32(at + 4);
                }
                m.set_u32(at + 4, block);
            }
            match kind {
                1 => four = m.u32(block),
                2 | 4 | 6 => three = m.u32(block),
                _ => {}
            }
        }
    }
    let mut n = count;
    while n > 0 {
        let three_floats = |m: &mut Mem, o: &mut dyn Outside, f: u32, to: &mut u32| {
            o.call(m, FSCANF, &[stream, f, frame, frame + 4, frame + 8]);
            if *to != 0 {
                *to = put_floats(m, *to, frame, 3);
            }
        };
        match kind {
            2 | 6 => {
                three_floats(m, o, fmt::G3_A, &mut three);
                three_floats(m, o, fmt::G3_B, &mut three);
                three_floats(m, o, fmt::G3_C, &mut three);
            }
            1 => {
                o.call(m, FSCANF, &[stream, fmt::G4, frame, frame + 4, frame + 8, frame + 12]);
                if four != 0 {
                    four = put_floats(m, four, frame, 4);
                }
            }
            4 => three_floats(m, o, fmt::G3_TYPE4, &mut three),
            _ => {}
        }
        n -= 1;
    }
    m.release(16);
    1
}

/// Is the NUL-terminated word at `p` exactly `word`?
fn is(m: &Mem, p: u32, word: &[u8]) -> bool {
    m.cstr(p) == word
}

/// [soft 0x083ddc00 clusterAnimNodeAsciiRead]: one `Cluster_node` block of a cluster file
/// for `model`: the model's cluster list is made if it has none (the field at +0x344, its
/// +4 set to 1); a new cluster data record is read into. `name` finds the model's cluster of
/// that name (`SI_klusterGetByName`), replaces its data (+0x18; the old is disposed) by the
/// record and takes the cluster's vertex count (+4) as `numCVs`; `numCVs` reads the count
/// itself; `numKeys` stores into the record's +0x14 when it is not negative; `indexes`
/// reads and drops `numCVs` numbers (twice as many for a type-2 model, none for a type
/// other than 1, 2, 4, 6 unless an earlier `indexes` set it); `Cluster_key` reads a block of
/// keys ([`cluster_key_ascii_read`]); `End` or an unknown word (reported with level 3) ends
/// the block, and then the record's +4 is set to its +0x18. 0 for no stream, else 1.
pub fn cluster_anim_node_ascii_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, model: u32) -> i32 {
    if stream == 0 {
        return 0;
    }
    // word 0x00..0x100, numCVs 0x100, numKeys 0x104, throw-away 0x108
    const FRAME: u32 = 0x10c;
    let frame = m.local(FRAME);
    let (word, num_cvs, num_keys, junk) = (frame, frame + 0x100, frame + 0x104, frame + 0x108);
    m.set_u32(num_cvs, 0);
    if model != 0 && m.u32(model + 0x344) == 0 {
        let list = o.call(m, SI_CLUSTER_ALLOCATE, &[]).eax;
        m.set_u32(model + 0x344, list);
        if list != 0 {
            m.set_u32(list + 4, 1);
        }
    }
    let data = o.call(m, SI_CLUSTER_DATA_ALLOCATE, &[]).eax;
    let mut indexes: i32 = 0;
    loop {
        o.call(m, FSCANF, &[stream, fmt::WORD, word]);
        if is(m, word, b"name") {
            o.call(m, FSCANF, &[stream, fmt::NAME_WORD, word]);
            if data != 0 && m.u8(word) != 0 {
                let k = o.call(m, SI_KLUSTER_GET_BY_NAME, &[model, word]).eax;
                if k != 0 {
                    if m.u32(k + 0x18) != 0 {
                        o.call(m, SI_CLUSTER_DATA_DISPOSE, &[k + 0x18]);
                    }
                    let n = m.u32(k + 4);
                    m.set_u32(k + 0x18, data);
                    m.set_u32(num_cvs, n);
                }
            }
        } else if is(m, word, b"numCVs") {
            o.call(m, FSCANF, &[stream, fmt::INT_CVS, num_cvs]);
        } else if is(m, word, b"numKeys") {
            o.call(m, FSCANF, &[stream, fmt::INT_KEYS, num_keys]);
            if data != 0 && (m.u32(num_keys) as i32) >= 0 {
                let v = m.u32(num_keys);
                m.set_u32(data + 0x14, v);
            }
        } else if is(m, word, b"indexes") {
            let cvs = m.u32(num_cvs) as i32;
            match m.u32(model + 0x30) as i32 {
                2 => indexes = cvs.wrapping_mul(2),
                1 | 4 | 6 => indexes = cvs,
                _ => {}
            }
            let mut k = indexes;
            while k > 0 {
                o.call(m, FSCANF, &[stream, fmt::INT_INDEX, junk]);
                k -= 1;
            }
        } else if is(m, word, b"Cluster_key") {
            let (cvs, kind) = (m.u32(num_cvs) as i32, m.u32(model + 0x30) as i32);
            cluster_key_ascii_read(m, o, stream, data, cvs, kind);
        } else if is(m, word, b"End") {
            break;
        } else {
            let text = [&b"ClusterAnimNodeAsciiRead: Error, unknown token "[..], &m.cstr(word)].concat();
            let msg = m.local(text.len() as u32 + 1);
            m.put_cstr(msg, &text);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
            m.release(text.len() as u32 + 1);
            break;
        }
    }
    if data != 0 {
        let first = m.u32(data + 0x18);
        m.set_u32(data + 4, first);
    }
    m.release(FRAME);
    1
}
