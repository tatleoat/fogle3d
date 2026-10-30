// SPDX-License-Identifier: MIT
//! Cluster files (`.cls`, magic 0x5380f637): the shape keys of one cluster of a model's
//! points. Finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `binary.c` (reader), `binary_b.c` (writer), the
//! element dictionary's file (the functions that open the file) and the cluster's own file
//! (the allocators).
//!
//! A cluster is a named subset of a model's points. A cluster file holds, for one cluster,
//! a list of keys, each a full set of positions for the cluster's points, and optionally the
//! model's current shape. What a position is depends on the model's kind, which the file
//! does not state: the reader takes it from the model the file is loaded onto.
//!
//! None of the 22 cluster files on the discs holds any of this: each is a header, a prototype
//! block and the closing zero token. The record layouts below are from the code alone.

use crate::geom::{read_geometry_in, write_geometry_in, GeomState, Geometry};
use crate::io::{fread_i16, fread_i32, fread_u16, End, Reader, Writer};
use crate::proto::{read_prototypes, write_prototypes, Prototypes, TAG as PROT};
use si_file::{write_db_header, MAGIC_CLUSTER};

/// The positions of one key, by the model's kind.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyPoints {
    /// Patches and NURBS (kinds 1, 9, 10): one homogeneous point (x, y, z, w) per point.
    Homogeneous(Vec<[f32; 4]>),
    /// Faces and splines (kinds 2, 6): three vectors per point.
    Triples(Vec<[[f32; 3]; 3]>),
    /// Polygon meshes (kind 4): one vector per point.
    Vectors(Vec<[f32; 3]>),
}

impl KeyPoints {
    fn new(model_type: i32) -> Option<KeyPoints> {
        match model_type {
            1 | 9 | 10 => Some(KeyPoints::Homogeneous(Vec::new())),
            2 | 6 => Some(KeyPoints::Triples(Vec::new())),
            4 => Some(KeyPoints::Vectors(Vec::new())),
            _ => None,
        }
    }
}

/// The key data of one cluster (`SI_ClusterData`, 28 bytes). The key list is a linked list
/// in the original (+0x18, each key 8 bytes: its buffer and the next key).
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterData {
    /// +0x00 (token 5): 1 in a new record
    pub d_00: i32,
    /// +0x08 (token 6)
    pub w_08: i16,
    /// +0x14 (token 3): the number of keys the file announces
    pub nb_keys: i32,
    /// +0x18 the keys that were read
    pub keys: Vec<KeyPoints>,
}

impl ClusterData {
    /// A new record [soft 0x081e7ecc SI_clusterDataAllocate]: +0x00 is 1, the rest of what
    /// it sets is zero. (It leaves +0x0c and +0x10 as allocated; they are not kept here.)
    pub fn new() -> ClusterData {
        ClusterData { d_00: 1, w_08: 0, nb_keys: 0, keys: Vec::new() }
    }
}

impl Default for ClusterData {
    fn default() -> Self {
        ClusterData::new()
    }
}

/// The record a model holds for its cluster animation as a whole (model + 0x344, 12 bytes)
/// [soft 0x081e7e40 SI_clusterAllocate]: all zero when new.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClusterAnim {
    /// +0x04 (token 7)
    pub d_04: i32,
    /// +0x08 (token 8)
    pub w_08: i16,
}

/// One cluster of a model (`kluster`), as far as the file reader and writer use it.
#[derive(Clone, Debug, PartialEq)]
pub struct Kluster {
    /// +0x00 the cluster's name
    pub name: Vec<u8>,
    /// +0x04 the number of points in the cluster
    pub nb_points: i32,
    /// +0x18 its key data
    pub data: Option<ClusterData>,
}

/// What a cluster token stream did.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterRead {
    /// The model's current shape (model + 0x3c), if the stream held one (token 1).
    pub current: Option<Geometry>,
    /// The original's result: false when an allocation failed, which here means a cluster
    /// with no points was given a key.
    pub ok: bool,
    pub end: End,
    /// A field arrived for which the original holds a null pointer and does not check
    /// (tokens 5 and 6 before any known cluster was named, 7 and 8 before any name): it
    /// would fault. Here the field is read and dropped.
    pub fault: bool,
}

/// Reads the token stream of a cluster file onto a model's clusters
/// [soft 0x083f1d20 SI_clusterAnimBinaryRead] (the part after the header and the prototype
/// block).
///
/// | token | field |
/// |---|---|
/// | 0 | end |
/// | 1 | one definition of the model's kind: the model's current shape |
/// | 2 | string: a cluster's name. If not empty, the model gets a new cluster animation record, and if the model has a cluster of that name, that cluster gets new, empty key data, which the following tokens fill |
/// | 3 | 32-bit: the number of keys; then that many keys, each: token 4, a 32-bit number of points, and that many positions (16 bytes each for a patch or NURBS model, 36 for a face or spline, 12 for a mesh). A key under another token is skipped by its size byte |
/// | 5 | 32-bit: key data +0x00 |
/// | 6 | 16-bit: key data +0x08 |
/// | 7 | 32-bit: cluster animation record +0x04 |
/// | 8 | 16-bit: cluster animation record +0x08 |
/// | other | skipped by the token's size byte |
///
/// A name the model does not have does not reset the "current key data": tokens 3, 5 and 6
/// that follow go on writing into the key data of the cluster named before (its key count
/// and two words), while the positions themselves are read and dropped. That is reproduced.
///
/// For a model of a kind without geometry here (not 1, 2, 4, 6, 9, 10) token 1 and each
/// position are "skipped" as a field of token 0xffff: a 32-bit size and that many bytes.
///
/// The original gives each key room for the cluster's own number of points and then stores
/// as many positions as the file says: more than that overruns the buffer. Here the key
/// holds what the file has. The number of positions is checked against the file's length
/// before anything is read (`clamped`).
///
/// After a mesh definition the original compares it with the model's own mesh and replaces
/// it by a copy of that if the counts differ; that belongs to the model code and is not
/// done here.
///
/// `clusters` are the model's clusters and `anim` its cluster animation record (model +
/// 0x344), which is replaced by a new one each time a cluster is named. `geom` is what the
/// geometry readers share with the model and the program (see [`GeomState`]).
pub fn read_tokens(
    r: &mut Reader,
    protos: &Prototypes,
    model_type: i32,
    clusters: &mut [Kluster],
    anim: &mut Option<ClusterAnim>,
    geom: &mut GeomState,
) -> ClusterRead {
    let mut out = ClusterRead { current: None, ok: true, end: End::Zero, fault: false };
    let mut cur_cluster: Option<usize> = None; // the cluster named last, if the model has it
    let mut cur_data: Option<usize> = None; // whose key data the fields go to
    let mut ok = fread_u16(&mut r.s, &mut r.st.token_binary);
    loop {
        let token = r.st.token_binary;
        r.note("cluster", token, ok);
        if token == 0 || !out.ok {
            out.end = End::Zero;
            return out;
        }
        if !ok {
            out.end = End::Eof;
            return out;
        }
        match token {
            1 => {
                // for a kind without geometry this skips a field of token 0xffff
                let (g, end) = read_geometry_in(r, protos, model_type, geom);
                out.current = g;
                if end == End::Eof {
                    out.end = End::Eof;
                    return out;
                }
            }
            2 => {
                let name = r.string();
                if !name.is_empty() {
                    *anim = Some(ClusterAnim::default());
                    cur_cluster = clusters.iter().position(|k| k.name == name);
                    if let Some(i) = cur_cluster {
                        clusters[i].data = Some(ClusterData::new());
                        cur_data = Some(i);
                    }
                }
            }
            3 => {
                let mut count = 0i32;
                fread_i32(&mut r.s, &mut count);
                if let Some(d) = cur_data.and_then(|i| clusters[i].data.as_mut()) {
                    d.nb_keys = count;
                }
                for _ in 0..count.max(0) {
                    if r.s.feof() || r.s.tell() >= r.s.len() {
                        // the original would go on reading nothing, `count` times
                        r.clamped = true;
                        break;
                    }
                    fread_u16(&mut r.s, &mut r.st.token_binary);
                    if r.st.token_binary != 4 {
                        let t = r.st.token_binary;
                        r.skip_field(t);
                        continue;
                    }
                    let mut n = 0i32;
                    fread_i32(&mut r.s, &mut n);
                    // a key is made only for a known cluster with key data
                    let mut key = None;
                    if let (Some(c), Some(_)) = (cur_cluster, cur_data) {
                        // [soft 0x081e7f78 SI_clusterKeyAllocate]: none for a cluster
                        // without points, or for a kind without positions
                        key = if clusters[c].nb_points < 1 { None } else { KeyPoints::new(model_type) };
                        if key.is_none() {
                            out.ok = false;
                        }
                    }
                    let per = match model_type {
                        1 | 9 | 10 => 16,
                        2 | 6 => 36,
                        4 => 12,
                        _ => 4,
                    };
                    let room = (r.s.len() - r.s.tell()) / per;
                    let n = if n.max(0) as usize > room {
                        r.clamped = true;
                        room
                    } else {
                        n.max(0) as usize
                    };
                    for _ in 0..n {
                        match model_type {
                            1 | 9 | 10 => {
                                let p = read_floats::<4>(r);
                                if let Some(KeyPoints::Homogeneous(v)) = key.as_mut() {
                                    v.push(p);
                                }
                            }
                            2 | 6 => {
                                let a = read_floats::<3>(r);
                                let b = read_floats::<3>(r);
                                let c = read_floats::<3>(r);
                                if let Some(KeyPoints::Triples(v)) = key.as_mut() {
                                    v.push([a, b, c]);
                                }
                            }
                            4 => {
                                let p = read_floats::<3>(r);
                                if let Some(KeyPoints::Vectors(v)) = key.as_mut() {
                                    v.push(p);
                                }
                            }
                            _ => {
                                r.skip_field(0xffff);
                            }
                        }
                    }
                    if let (Some(k), Some(d)) = (key, cur_data.and_then(|i| clusters[i].data.as_mut())) {
                        d.keys.push(k);
                    }
                }
            }
            5 => {
                let mut v = 0i32;
                match cur_data.and_then(|i| clusters[i].data.as_mut()) {
                    Some(d) => {
                        fread_i32(&mut r.s, &mut d.d_00);
                    }
                    None => {
                        fread_i32(&mut r.s, &mut v);
                        out.fault = true;
                    }
                }
            }
            6 => {
                let mut v = 0i16;
                match cur_data.and_then(|i| clusters[i].data.as_mut()) {
                    Some(d) => {
                        fread_i16(&mut r.s, &mut d.w_08);
                    }
                    None => {
                        fread_i16(&mut r.s, &mut v);
                        out.fault = true;
                    }
                }
            }
            7 => {
                let mut v = 0i32;
                match anim.as_mut() {
                    Some(a) => {
                        fread_i32(&mut r.s, &mut a.d_04);
                    }
                    None => {
                        fread_i32(&mut r.s, &mut v);
                        out.fault = true;
                    }
                }
            }
            8 => {
                let mut v = 0i16;
                match anim.as_mut() {
                    Some(a) => {
                        fread_i16(&mut r.s, &mut a.w_08);
                    }
                    None => {
                        fread_i16(&mut r.s, &mut v);
                        out.fault = true;
                    }
                }
            }
            _ => {
                r.skip_field(token);
            }
        }
        ok = fread_u16(&mut r.s, &mut r.st.token_binary);
    }
}

/// `N` floats read in one `fread` into a local record, each swapped if the whole record
/// arrived [soft 0x083f9d54 SI_Vector_fread] [soft 0x083f9fb0 SI_Hvector_fread]. The locals
/// are uninitialised in the original; here a record that could not be read whole is zero
/// where no byte arrived.
fn read_floats<const N: usize>(r: &mut Reader) -> [f32; N] {
    let mut raw = [0u8; 16];
    let whole = r.s.read_record(&mut raw[..4 * N]);
    let mut out = [0f32; N];
    for (i, o) in out.iter_mut().enumerate() {
        let b = [raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]];
        *o = f32::from_bits(if whole { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) });
    }
    out
}

/// What reading a cluster file gives.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterFile {
    /// The header's version, −1.0 if the header was not accepted.
    pub version: f32,
    pub comment: Vec<u8>,
    /// Whether the four bytes after the header were `PROT`.
    pub has_prototypes: bool,
    pub prototypes: Prototypes,
    pub read: ClusterRead,
    /// Offset of the first byte not read.
    pub pos: usize,
    pub clamped: bool,
}

/// Reads a cluster file onto a model's clusters [soft 0x083f1d20 SI_clusterAnimBinaryRead]:
/// the header; four bytes which, if they are `PROT`, open a prototype block (any other four
/// bytes are simply dropped); the token stream. The header's version becomes `fileVersion`.
///
/// The original does nothing (and reports failure) for a model without clusters; the caller
/// decides that here. The program gets here through `SI_elemdictReadClusters` (0x08110144,
/// not ported), which first looks at the start of the file: one that begins with a text tag
/// instead of a binary magic goes to the reader of the old text format
/// (`SI_oclusterAsciiRead`).
pub fn read_file(
    r: &mut Reader,
    model_type: i32,
    clusters: &mut [Kluster],
    anim: &mut Option<ClusterAnim>,
    geom: &mut GeomState,
) -> ClusterFile {
    let (version, comment) = crate::io::read_header(r);
    let mut tag = 0u32;
    crate::io::fread_u32(&mut r.s, &mut tag);
    r.st.file_version = version;
    let mut prototypes = Prototypes::new();
    let has_prototypes = tag == PROT;
    if has_prototypes {
        read_prototypes(r, &mut prototypes);
    }
    let read = read_tokens(r, &prototypes, model_type, clusters, anim, geom);
    ClusterFile { version, comment, has_prototypes, prototypes, read, pos: r.s.tell(), clamped: r.clamped }
}

/// Why the writer refused (`SI_saveElemErrorCode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// Code 4: no cluster.
    NoCluster,
    /// Code 6: no model.
    NoModel,
}

/// Writes a cluster file for one cluster of a model
/// [soft 0x083f68cc SI_clusterAnimBinaryWrite], reached through
/// [soft 0x08110968 SI_elemdictWriteClusters]: the header, the prototype block for the
/// output version, the model's current shape under token 1 if it has one, and, if the
/// cluster has key data: its name (2); the key count field and the keys (3, each key under
/// token 4 with the cluster's number of points and that many positions); the two words of
/// the key data (5, 6) and of the model's cluster animation record (7, 8); the zero token.
///
/// Each key is written with the cluster's own number of points, whatever the key holds: a
/// key with fewer positions is padded here with zeros (the original reads past its buffer).
/// The writer walks the key list itself and does not look at the key count field it writes.
/// For a face or spline model the three vectors of a position are written in the order they
/// are stored. A model without a cluster animation record makes the original read through a
/// null pointer for tokens 7 and 8; here zeros are written.
pub fn write_file(w: &mut Writer, current: Option<&Geometry>, model_type: i32, cluster: &Kluster, anim: Option<&ClusterAnim>, uv_domains: i32) {
    write_db_header(&mut w.out, w.version_output, MAGIC_CLUSTER);
    write_prototypes(w);
    write_tokens(w, current, model_type, cluster, anim, uv_domains);
}

/// The token stream of a cluster file (see [`write_file`]). `uv_domains` is the model's
/// number of UV domains, which a mesh definition is written for.
pub fn write_tokens(w: &mut Writer, current: Option<&Geometry>, model_type: i32, cluster: &Kluster, anim: Option<&ClusterAnim>, uv_domains: i32) {
    let tok = |w: &mut Writer, t: u16| {
        w.st.token_binary_b = t;
        w.put_u16(t);
    };
    if let Some(g) = current {
        tok(w, 1);
        if matches!(model_type, 1 | 2 | 4 | 6 | 9 | 10) {
            write_geometry_in(w, g, uv_domains);
        }
    }
    if let Some(d) = cluster.data.as_ref() {
        tok(w, 2);
        w.string(Some(&cluster.name));
        tok(w, 3);
        w.put_i32(d.nb_keys);
        let n = cluster.nb_points.max(0) as usize;
        for key in &d.keys {
            tok(w, 4);
            w.put_i32(cluster.nb_points);
            for i in 0..n {
                match (model_type, key) {
                    (1 | 9 | 10, KeyPoints::Homogeneous(v)) => {
                        for c in v.get(i).unwrap_or(&[0.0; 4]) {
                            w.put_f32(*c);
                        }
                    }
                    (2 | 6, KeyPoints::Triples(v)) => {
                        for p in v.get(i).unwrap_or(&[[0.0; 3]; 3]) {
                            for c in p {
                                w.put_f32(*c);
                            }
                        }
                    }
                    (4, KeyPoints::Vectors(v)) => {
                        for c in v.get(i).unwrap_or(&[0.0; 3]) {
                            w.put_f32(*c);
                        }
                    }
                    _ => {}
                }
            }
        }
        tok(w, 5);
        w.put_i32(d.d_00);
        tok(w, 6);
        w.put_u16(d.w_08 as u16);
        let a = anim.copied().unwrap_or_default();
        tok(w, 7);
        w.put_i32(a.d_04);
        tok(w, 8);
        w.put_u16(a.w_08 as u16);
    }
    tok(w, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster(kind: i32) -> Kluster {
        let keys = (0..2)
            .map(|k| {
                let f = k as f32;
                match kind {
                    1 => KeyPoints::Homogeneous(vec![[f, 1.0, 2.0, 1.0], [3.0, f, 5.0, 1.0], [6.0, 7.0, f, 0.5]]),
                    2 | 6 => KeyPoints::Triples(vec![[[f; 3], [1.0; 3], [2.0; 3]], [[3.0; 3], [f; 3], [5.0; 3]], [[6.0; 3], [7.0; 3], [f; 3]]]),
                    _ => KeyPoints::Vectors(vec![[f, 1.0, 2.0], [3.0, f, 5.0], [6.0, 7.0, f]]),
                }
            })
            .collect();
        Kluster { name: b"lip".to_vec(), nb_points: 3, data: Some(ClusterData { d_00: 2, w_08: 1, nb_keys: 2, keys }) }
    }

    #[test]
    fn round_trip_for_each_kind_of_position() {
        for kind in [1, 2, 4, 6] {
            let c = cluster(kind);
            let anim = ClusterAnim { d_04: 9, w_08: 3 };
            let mut w = Writer::new();
            write_tokens(&mut w, None, kind, &c, Some(&anim), 1);
            let per = match kind { 1 => 16, 4 => 12, _ => 36 };
            assert_eq!(w.out.len(), 2 + 4 + 2 + 4 + 2 * (2 + 4 + 3 * per) + 6 + 4 + 6 + 4 + 2);
            let mut model = [Kluster { name: b"lip".to_vec(), nb_points: 3, data: None }];
            let mut r = Reader::new(&w.out);
            let mut got_anim = None;
            let got = read_tokens(&mut r, &Prototypes::new(), kind, &mut model, &mut got_anim, &mut GeomState::default());
            assert_eq!((got.end, got.ok, got.fault, r.s.tell()), (End::Zero, true, false, w.out.len()));
            assert_eq!(got_anim, Some(anim));
            assert_eq!(model[0], c, "kind {kind}");
        }
    }

    #[test]
    fn a_name_the_model_lacks_writes_into_the_cluster_before() {
        let mut w = Writer::new();
        write_tokens(&mut w, None, 4, &cluster(4), None, 1);
        w.out.truncate(w.out.len() - 2);
        // a second cluster the model does not have: its count and words land in the first
        let mut other = cluster(4);
        other.name = b"brow".to_vec();
        other.data.as_mut().unwrap().d_00 = 77;
        other.data.as_mut().unwrap().keys.truncate(1);
        other.data.as_mut().unwrap().nb_keys = 1;
        write_tokens(&mut w, None, 4, &other, None, 1);
        let mut model = [Kluster { name: b"lip".to_vec(), nb_points: 3, data: None }];
        let mut r = Reader::new(&w.out);
        let got = read_tokens(&mut r, &Prototypes::new(), 4, &mut model, &mut None, &mut GeomState::default());
        assert_eq!((got.end, r.s.tell()), (End::Zero, w.out.len()));
        let d = model[0].data.as_ref().unwrap();
        assert_eq!((d.d_00, d.nb_keys, d.keys.len()), (77, 1, 2));
    }

    #[test]
    fn empty_file_and_faults() {
        // what every cluster file on the discs is: prototypes and a zero token
        let mut w = Writer::new();
        write_db_header(&mut w.out, 3.7, MAGIC_CLUSTER);
        w.out.extend_from_slice(b"PROT");
        w.out.extend_from_slice(&[0, 3, 0, 2, 0, 8, 0, 0, 0, 0]);
        w.out.extend_from_slice(&[0, 0]);
        let mut r = Reader::new(&w.out);
        let f = read_file(&mut r, 4, &mut [], &mut None, &mut GeomState::default());
        assert_eq!((f.version, f.has_prototypes, f.read.end, f.pos), (3.7, true, End::Zero, w.out.len()));
        assert_eq!(r.st.file_version, 3.7);
        // token 5 with no cluster named
        let got = read_tokens(&mut Reader::new(&[0, 5, 0, 0, 0, 1, 0, 0]), &Prototypes::new(), 4, &mut [], &mut None, &mut GeomState::default());
        assert!(got.fault);
        // a key for a cluster without points fails the read
        let mut w = Writer::new();
        write_tokens(&mut w, None, 4, &cluster(4), None, 1);
        let mut model = [Kluster { name: b"lip".to_vec(), nb_points: 0, data: None }];
        let got = read_tokens(&mut Reader::new(&w.out), &Prototypes::new(), 4, &mut model, &mut None, &mut GeomState::default());
        assert!(!got.ok);
    }
}
