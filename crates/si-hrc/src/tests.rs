// SPDX-License-Identifier: MIT
//! Tests on made-up data only; the original files are checked by `hrc-check`.

use crate::io::{Rd, Wr};
use crate::model::*;
use crate::proto::{self, Prototypes};
use crate::read::{read_file, read_file_with, Content, Kind, Reader};
use crate::write::{user_data_size, write_as_read, write_content, write_material, write_model, Writer};

/// A file with a prototype block of our choosing.
fn file(magic: u32, version: f32, protos: &[(i16, &[u16])], tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    si_file::write_bin_header(&mut out, magic, version, None);
    if !protos.is_empty() {
        out.extend_from_slice(b"PROT");
        for (id, tokens) in protos {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&(tokens.len() as i16).to_be_bytes());
            for t in *tokens {
                out.extend_from_slice(&t.to_be_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    out
}

fn material_of(data: &[u8]) -> (Material, usize, crate::read::Notes) {
    let f = read_file(data, Kind::Material).unwrap();
    match f.content {
        Content::Material(m) => (m, f.end, f.notes),
        _ => panic!("no material"),
    }
}

#[test]
fn skip_field_three_kinds() {
    // plain: nothing can be skipped
    let mut r = Rd::new(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(!r.skip_field(0x0042));
    assert_eq!(r.s.tell(), 0);
    // short length: the high byte is the number of bytes
    assert!(r.skip_field(0x0342));
    assert_eq!(r.s.tell(), 3);
    // long length: four bytes of length, then the data
    let mut r = Rd::new(&[0, 0, 0, 2, 9, 9, 7]);
    assert!(r.skip_field(0xff42u16 as i16));
    assert_eq!(r.s.tell(), 6);
    // a length beyond the file reads to its end and sets the flag
    let mut r = Rd::new(&[0, 0, 1, 0, 9]);
    assert!(r.skip_field(-1));
    assert!(r.s.feof());
    // so does a negative one (the original overruns its buffer there)
    let mut r = Rd::new(&[0xff, 0xff, 0xff, 0xff, 9, 9]);
    r.skip_field(-1);
    assert!(r.s.feof());
}

#[test]
fn a_cut_field_keeps_its_unswapped_leading_bytes() {
    let mut r = Rd::new(&[0x12, 0x34, 0x56]);
    let mut v = 0x0a0b_0c0di32;
    assert!(!r.i32(&mut v));
    // little-endian memory 0d 0c 0b 0a with the first three bytes overwritten, not swapped
    assert_eq!(v, 0x0a56_3412);
    let mut r = Rd::new(&[0, 0, 0, 7]);
    assert!(r.i32(&mut v));
    assert_eq!(v, 7);
}

#[test]
fn unknown_tokens_of_a_material() {
    // 0x63: plain and unknown, nothing is read; 0x263: two bytes skipped; 0xff63: length 3
    let body = [b'm', 0, /* 0x263 */ 0xaa, 0xbb, /* 0xff63 */ 0, 0, 0, 3, 1, 2, 3, /* shading */ 0, 9];
    let data = file(si_file::MAGIC_MATERIAL, 3.8, &[(13, &[1, 0x63, 0x263, 0xff63, 2, 0])], b"MTRL", &body);
    let (m, end, notes) = material_of(&data);
    assert_eq!((m.name.as_slice(), m.shading, end), (&b"m"[..], 9, data.len()));
    assert_eq!(notes.unknown_plain.get(&(proto::MATERIAL, 0x63)), Some(&1));
    assert_eq!(notes.skipped.len(), 2);
    // the same token id with another length byte is another token: 0x202 is not token 2
    let data = file(si_file::MAGIC_MATERIAL, 3.8, &[(13, &[0x202, 0])], b"MTRL", &[0, 9]);
    let (m, end, _) = material_of(&data);
    assert_eq!((m.shading, end), (4, data.len())); // skipped by its length, default kept
}

#[test]
fn tokens_inline_when_there_is_no_prototype() {
    // no PROT block: every field is preceded by its token, and a zero token ends the record
    let body = [0, 1, b'a', b'b', 0, 0, 2, 0, 5, 0, 6, 0x41, 0x20, 0, 0, 0, 0];
    let data = file(si_file::MAGIC_GENERAL, 2.0, &[], b"MTRL", &body);
    let (m, end, _) = material_of(&data);
    assert_eq!((m.name.as_slice(), m.shading, m.exponent, end), (&b"ab"[..], 5, 10.0, data.len()));
}

#[test]
fn material_shading_is_stored_three_times() {
    let data = file(si_file::MAGIC_MATERIAL, 3.8, &[(13, &[2, 0x218, 0])], b"MTRL", &[0, 1, 0, 7]);
    let (m, _, _) = material_of(&data);
    assert_eq!((m.shading, m.xdc8, m.xdca), (1, 2, 2));
    let protos = Prototypes::current(4.0);
    let mut w = Writer::new(&protos);
    w.material(&Material { name: Vec::new(), shading: 7, ..Material::default() }, false);
    // name, token 2 (7 written as 1), ... ; the last three shorts are 0x21b, 0x21c, 0x21d
    assert_eq!(&w.w.out[..3], &[0, 0, 1]);
    assert_eq!(&w.w.out[w.w.out.len() - 6..], &[0, 7, 0, 1, 0, 1]);
}

#[test]
fn oldest_material_reflection_colour_becomes_its_mean() {
    let mut body = Vec::new();
    for v in [0.25f32, 0.5, 0.75] {
        body.extend_from_slice(&v.to_be_bytes());
    }
    let data = file(si_file::MAGIC_GENERAL, 2.0, &[(13, &[9, 0])], b"MTRL", &body);
    assert_eq!(material_of(&data).0.reflectivity, 0.5);
}

#[test]
fn texture_crop_tokens_hold_two_shorts_each_and_overlap() {
    let mut body = Vec::new();
    for v in [1i16, 2, 3, 4, 5, 6, 7, 8] {
        body.extend_from_slice(&v.to_be_bytes());
    }
    let data = file(si_file::MAGIC_TEXTURE_2D, 3.8, &[(14, &[0x16, 0x17, 0x18, 0x19, 0])], b"TXMP", &body);
    let f = read_file(&data, Kind::Texture).unwrap();
    assert_eq!(f.end, data.len());
    let Content::Texture(x) = &f.content else { panic!() };
    // 0x16 -> [0],[1]; 0x17 -> [2],[3]; 0x18 -> [1],[2]; 0x19 -> [3],[4], and [4] is not
    // carried into the texture the reader returns
    assert_eq!(x.crop, [1, 5, 6, 7, 0]);
    assert_eq!(f.notes.changed.get("texture: fifth cropping short read and dropped"), Some(&1));
    // what the program itself wrote reads back to what it had in memory (which never has
    // anything but 0 in the fifth short once read)
    let mut y = Texture2D::default();
    y.crop = [10, 20, 30, 40, 0];
    let back = read_file(&crate::write::write_texture(&y, 4.0), Kind::Texture).unwrap();
    assert_eq!(back.content, Content::Texture(y));
}

/// The version tests compare the float version with a double constant: the float nearest
/// a constant that rounds down is below it (finding 0006 section 10, finding 0004).
#[test]
fn versions_are_compared_as_floats_against_doubles() {
    use crate::read::below;
    // 2.1 rounds down to 2.0999999 as a float, 3.50009 to 3.5000899, 2.6 to 2.5999999
    assert!(below(2.1f32, 2.1) && below(3.50009f32, 3.50009) && below(2.6f32, 2.6));
    // 3.49089 and 3.82099 round up: a file of that version is not below them
    assert!(!below(3.49089f32, 3.49089) && !below(3.82099f32, 3.82099));
    assert!(!below(2.11f32, 2.1));
    // shader references: 2.67906 rounds down, so its float is "up to" it as well
    assert!(crate::read::is_old_udf_version(2.67906f32));
    // 2.999019 rounds down: below it, old
    assert!(crate::read::is_old_udf_version(2.999019f32));
    // 2.69 rounds up: its float is not below 2.69 either way
    assert!(crate::read::is_old_udf_version(2.69f32));
}

/// A 3D texture's channel hangs on `b_matio.c`'s version: in a file of version 2.1 (below
/// the double) it is 2 whatever token 0x21 says; inside a model in a fresh program (that
/// version is 0) as well; and after a material file of 3.7 has been read, the model's
/// 3D textures keep what they store.
#[test]
fn texture3d_channel_follows_the_material_files_version() {
    let body = [0u8, 1];
    let t3d = |v: f32| file(si_file::MAGIC_TEXTURE_3D, v, &[(15, &[0x21, 0])], b"TX3D", &body);
    let channel = |f: crate::read::File| match f.content {
        Content::Texture3D(x) => x.channel,
        _ => panic!(),
    };
    assert_eq!(channel(read_file(&t3d(2.1), Kind::Texture3D).unwrap()), 2);
    assert_eq!(channel(read_file(&t3d(2.11), Kind::Texture3D).unwrap()), 1);
    // a node with one inline 3D texture: hierarchy token 9, count 1, external 0
    let mut node = vec![0u8, 1, 0, 0];
    node.extend_from_slice(&body);
    let model = file(si_file::MAGIC_GENERAL, 3.7, &[(5, &[9, 0]), (15, &[0x21, 0])], b"HRCH", &node);
    let t3d_of = |f: crate::read::File| match f.content {
        Content::Model(n) => n.texture3d.unwrap().channel,
        _ => panic!(),
    };
    assert_eq!(t3d_of(read_file(&model, Kind::Model).unwrap()), 2);
    let mut versions = crate::read::Versions::default();
    let material = file(si_file::MAGIC_MATERIAL, 3.7, &[(13, &[0])], b"MTRL", &[]);
    read_file_with(&material, Kind::Material, &mut versions).unwrap();
    assert_eq!((versions.matio, versions.input), (3.7, 3.7));
    assert_eq!(t3d_of(read_file_with(&model, Kind::Model, &mut versions).unwrap()), 1);
    assert_eq!(versions.binary, 3.7);
}

#[test]
fn texture_methods_7_and_8_lose_their_factors() {
    let mut body = vec![0, 8];
    body.extend_from_slice(&0.5f32.to_be_bytes());
    let data = file(si_file::MAGIC_TEXTURE_2D, 3.8, &[(14, &[10, 0xb, 0])], b"TXMP", &body);
    let f = read_file(&data, Kind::Texture).unwrap();
    let Content::Texture(x) = &f.content else { panic!() };
    assert_eq!((x.method, x.ambient, x.diffuse, x.blend), (8, 0.0, 0.0, 0.0));
    assert_eq!(f.notes.changed.len(), 1);
}

#[test]
fn shader_reference_old_and_new() {
    let protos = |tokens: &[u16]| {
        let mut w = Wr::default();
        for v in [28u16, tokens.len() as u16].iter().chain(tokens).chain(&[0u16]) {
            w.u16(*v);
        }
        Prototypes::read(&mut Rd::new(&w.out))
    };
    // current form: name, three 4-byte values
    let data = [b's', 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9];
    let mut r = Reader::new(&data, 3.8, protos(&[1, 2, 3, 4, 0]));
    assert_eq!(r.udf(), Some(Udf { name: b"s".to_vec(), active: 1, internal: 0, order: 9 }));
    assert_eq!(r.r.s.tell(), data.len());
    // an empty name or the name NULL: the fields are read, the reference is dropped
    for name in [&b""[..], b"NULL"] {
        let mut d = name.to_vec();
        d.extend_from_slice(&[0; 13]);
        let mut r = Reader::new(&d, 3.8, protos(&[1, 2, 3, 4, 0]));
        assert_eq!(r.udf(), None);
        assert_eq!(r.r.s.tell(), d.len());
    }
    // token 5 ends the record on the spot
    let mut r = Reader::new(&data, 3.8, protos(&[1, 5, 2, 0]));
    assert_eq!(r.udf().unwrap().active, 0);
    assert_eq!(r.r.s.tell(), 2);
    // no prototype: tokens 1 to 4 are assumed, nothing inline
    let mut r = Reader::new(&data, 3.8, Prototypes::default());
    assert_eq!(r.udf().unwrap().order, 9);
    // older versions: three strings, a short and a 4-byte value, all dropped
    let old = [b'a', 0, b'b', 0, 0, 0, 1, 0, 0, 0, 2];
    for version in [2.65f32, 2.6991] {
        let mut r = Reader::new(&old, version, protos(&[1, 2, 3, 4, 5, 0]));
        assert_eq!(r.udf(), None);
        assert_eq!(r.r.s.tell(), old.len());
    }
    assert!(!crate::read::is_old_udf_version(2.68) && !crate::read::is_old_udf_version(3.0));
}

#[test]
fn user_data_blocks() {
    let list = vec![
        UserNode { tag: b"_tag".to_vec(), flag: 1, size: 3, data: vec![1, 2, 3] },
        UserNode { tag: b"CUSELEMCB".to_vec(), flag: 0, size: 1, data: vec![9] }, // never written
        UserNode { tag: b"x".to_vec(), flag: 0, size: 4, data: vec![1, 2, 3, 4] },
    ];
    assert_eq!(user_data_size(&list), (4 + (4 + 3 + 10) + (1 + 4 + 10), 2));
    // mode 1 (other writers) also leaves out the custom-effect blocks
    let custom = [list[0].clone(), UserNode { tag: b"CUSB".to_vec(), flag: 1, size: 0, data: Vec::new() }];
    assert_eq!((user_data_size(&custom).1, crate::write::user_data_size_mode(&custom, 1).1), (2, 1));
    let protos = Prototypes::default();
    let mut w = Writer::new(&protos);
    w.w.i32(2);
    w.user_data_blocks(&list);
    assert_eq!(w.w.out.len() as i32, user_data_size(&list).0);
    let mut r = Reader::new(&w.w.out, 4.0, Prototypes::default());
    assert_eq!(r.user_data(), vec![list[0].clone(), list[2].clone()]);
    // below 3.82099 a block of 2, 4 or 8 bytes comes out reversed
    let mut r = Reader::new(&w.w.out, 3.7, Prototypes::default());
    let old = r.user_data();
    assert_eq!((old[0].data.as_slice(), old[1].data.as_slice()), (&[1u8, 2, 3][..], &[4u8, 3, 2, 1][..]));
}

fn node_from(version: f32, hier: &[u16], more: &[(i16, &[u16])], body: &[u8]) -> (Box<Node>, crate::read::File) {
    let mut protos: Vec<(i16, &[u16])> = vec![(5, hier)];
    protos.extend_from_slice(more);
    let data = file(si_file::MAGIC_GENERAL, version, &protos, b"HRCH", body);
    let f = read_file(&data, Kind::Model).unwrap();
    assert_eq!(f.end, data.len(), "the record must end with the file");
    let Content::Model(n) = f.content.clone() else { panic!() };
    (n, f)
}

#[test]
fn old_user_nodes_and_their_tags() {
    // one node tagged ABCD with 2 bytes, one tagged CUSA with 1
    let body = [0, 2, b'A', b'B', b'C', b'D', 0, 0, 0, 2, 7, 8, b'C', b'U', b'S', b'A', 0, 0, 0, 1, 5];
    let (n, _) = node_from(3.4909, &[0x16, 0], &[], &body);
    assert_eq!(n.user_nodes[0], UserNode { tag: b"_DCBA".to_vec(), flag: 1, size: 2, data: vec![8, 7] });
    assert_eq!(n.user_nodes[1].tag, b"CUSA");
    let (n, f) = node_from(3.7, &[0x16, 0], &[], &body);
    assert_eq!(n.user_nodes[0], UserNode { tag: b"ABCD".to_vec(), flag: 1, size: 2, data: vec![7, 8] });
    // and that form is written back as it came
    assert_eq!(write_as_read(&f).unwrap().w.out, file(si_file::MAGIC_GENERAL, 3.7, &[(5, &[0x16, 0])], b"HRCH", &body));
}

#[test]
fn hierarchy_reader_normalisations() {
    // shape interpolation -1 is read as 0
    let (n, f) = node_from(2.65, &[0xc, 0], &[], &[0xff, 0xff]);
    assert_eq!(n.shape_interp, 0);
    assert_eq!(f.notes.changed.len(), 1);
    // dynamics: a flag, one more short, then the record
    let mut body = vec![0, 1, 0, 0];
    body.extend_from_slice(&2.5f32.to_be_bytes());
    let (n, _) = node_from(3.8, &[0xe, 0], &[(19, &[1, 0])], &body);
    assert_eq!(n.dynamics.unwrap().density, 2.5);
    // a mesh node: colour forced below 3.71, material index out of range reset
    let mut body = vec![0, 4, 0, 0, 0, 1, 0, 1];
    body.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0, 5]);
    let more: &[(i16, &[u16])] = &[(8, &[8, 0]), (10, &[2, 3, 0]), (11, &[0x405, 0])];
    let (n, _) = node_from(3.7, &[2, 3, 0], more, &body);
    let Some(Definition::Mesh(m)) = n.definitions.first() else { panic!() };
    assert_eq!((m.polygons[0].nodes[0].colour, m.polygons[0].material), (0xff11_2233, 0));
    let (n, _) = node_from(3.71, &[2, 3, 0], more, &body);
    let Some(Definition::Mesh(m)) = n.definitions.first() else { panic!() };
    assert_eq!(m.polygons[0].nodes[0].colour, 0x0011_2233);
}

#[test]
fn envelope_below_3_49089_is_dropped_on_a_chain_root() {
    // algorithm 1 on a null, then an envelope with only its level
    let body = [0, 1, 0, 9, 0, 1, 0, 0, 0, 3];
    let (n, _) = node_from(3.0905, &[0x10, 0x11, 0], &[(21, &[1, 0])], &body);
    assert_eq!((n.envelop_type, n.envelop.is_none()), (9, true));
    let (n, _) = node_from(3.5, &[0x10, 0x11, 0], &[(21, &[1, 0])], &body);
    assert_eq!(n.envelop.as_ref().unwrap().level, 3);
    // reading an envelope gives the node an initial transformation copied from its own
    assert_eq!(n.init_transfo, Some([[1.0; 3], [0.0; 3], [0.0; 3]]));
}

#[test]
fn son_and_brother_and_the_parents_initial_transformation() {
    // root (algorithm 1, a chain root) -> son with an envelope whose token 0x17 is a rotation
    // for the parent; the son has a brother
    let mut body = vec![b'r', 0, /* algorithm */ 0, 1, /* no envelope */ 0, 0, 0, 0, /* son */ 0, 1];
    body.extend_from_slice(&[b's', 0, 0, 0, /* envelope */ 0, 0, 0, 1]);
    for v in [1.0f32, 2.0, 3.0] {
        body.extend_from_slice(&v.to_be_bytes());
    }
    body.extend_from_slice(&[/* no son */ 0, 0, /* brother */ 0, 1]);
    body.extend_from_slice(&[b'b', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[/* the root has no brother */ 0, 0]);
    let (n, f) = node_from(3.8, &[1, 0x10, 0x11, 0xa, 0xb, 0], &[(21, &[0x17, 0])], &body);
    assert_eq!(f.notes.nodes, 3);
    assert_eq!(n.init_transfo.unwrap()[1], [1.0, 2.0, 3.0]);
    let son = n.son.as_ref().unwrap();
    assert_eq!((son.name.as_slice(), son.brother.as_ref().unwrap().name.as_slice()), (&b"s"[..], &b"b"[..]));
    let mut order = Vec::new();
    n.walk(0, &mut |d, x| order.push((d, x.name.clone())));
    assert_eq!(order, [(0, b"r".to_vec()), (1, b"s".to_vec()), (1, b"b".to_vec())]);
}

#[test]
fn textures_on_a_model_get_their_wrap() {
    // a closed patch (U) with a UV texture scaled 3 in U: wraps in U, scale kept within 1
    let mut body = vec![0, 1, /* patch: close u */ 0, 1, /* textures */ 0, 1, 0, 0, 0, 4];
    body.extend_from_slice(&3.0f32.to_be_bytes());
    let more: &[(i16, &[u16])] = &[(6, &[4, 0]), (14, &[10, 5, 0])];
    let (n, f) = node_from(3.8, &[2, 3, 8, 0], more, &body);
    let x = &n.textures[0];
    assert_eq!((x.method, x.wrap_u, x.wrap_v, x.uscale), (4, 1, 0, 1.0));
    assert_eq!(f.notes.changed.len(), 1);
    // on a face the UV method becomes method 6
    let body2 = [&[0u8, 2][..], &[0, 1, 0, 0, 0, 4], &0.5f32.to_be_bytes()].concat();
    let (n, _) = node_from(3.8, &[2, 8, 0], more, &body2);
    assert_eq!((n.textures[0].method, n.textures[0].wrap_u), (6, 1));
}

fn curve(points: usize) -> NurbsCurve {
    NurbsCurve {
        order: 3, npoints: points as i32,
        points: (0..points).map(|i| NurbsPoint { xyzw: [i as f64, 0.5, -1.0, 1.0], flag: i as i32 }).collect(),
        nknots: 4, knots: vec![0.0, 0.25, 0.5, 1.0], start: 0.0, end: 1.0, closed: 1, uniformity: 2, closing: 0,
        step: 7, keyframed: 1,
    }
}

fn spline(name: &[u8]) -> Spline {
    let key = SplineKey { prev: [1.0, 2.0, 3.0], pos: [4.0, 5.0, 6.0], next: [7.0, 8.0, 9.0], constraint: 1, segment: 2, flag: 3 };
    Spline { name: name.to_vec(), phantom: 1, close: 1, kind: 3, nkeys: 2, keys: vec![key, SplineKey::default()], tension: 0.5, step: 4, old_points: false }
}

fn texture(name: &[u8]) -> Texture2D {
    Texture2D {
        name: name.to_vec(), filename: b"pic/wood".to_vec(), anim: 1, uscale: 0.5, voffset: 0.25, rotation: [0.1, 0.2, 0.3],
        udf: Some(Udf { name: b"shader".to_vec(), active: 1, internal: 0, order: 2 }), mask: 4, hw_tiling: 1, x1d4: 5,
        user_data: vec![UserNode { tag: b"_t".to_vec(), flag: 1, size: 2, data: vec![1, 2] }],
        ..Texture2D::default()
    }
}

/// A hierarchy that uses every prototype and every token of the version 4.0 table.
pub(crate) fn everything() -> Box<Node> {
    let ud = |tag: &[u8]| UserNode { tag: tag.to_vec(), flag: 0, size: 3, data: vec![7, 8, 9] };
    let material = Material {
        name: b"mat".to_vec(), shading: 3, glow: 1, glow_amount: 0.5, ambient: [0.1, 0.2, 0.3], exponent: 20.0,
        reflectivity: 0.25, refraction: 1.5, textures: vec![texture(b"t1"), texture(b"t2")],
        texture3d: Some(Texture3D { name: b"marble".to_vec(), kind: 1, iterations: 3, taper: 0.5, udf: None,
                                    user_data: vec![ud(b"_3d")], ..Texture3D::default() }),
        blur: 1, udf_volume: Some(Udf { name: b"vol".to_vec(), active: 1, internal: 1, order: 0 }),
        udf_extra: Some(Udf { name: b"extra".to_vec(), active: 0, internal: 0, order: 4 }),
        user_data: vec![ud(b"_m1"), ud(b"_m2")], ..Material::default()
    };
    let poly = |a: i32, b: i32, c: i32| Polygon {
        normal: [0.0; 3], nnodes: 3,
        nodes: [a, b, c].iter().map(|&v| PolyNode { vertex: v, normal: [0.0, 1.0, 0.0], uv: [0.5, 0.25], colour: 0x8011_2233, more_uv: vec![[0.1, 0.9]] }).collect(),
        material: 0, flag: 2,
    };
    let mesh = Mesh {
        angle: 45.0, nvertices: 4,
        vertices: (0..4).map(|i| Vertex { pos: [i as f32, 0.0, 1.0], flag: i as i16 }).collect(),
        npolygons: 2, polygons: vec![poly(0, 1, 2), poly(0, 2, 3)],
        nedges: 2, edges: vec![Edge { v: [0, 1], flag: 1 }, Edge { v: [1, 2], flag: 0 }],
        flag: 0x805, x38: 1, approx: [1.0, 2.0, 3.0], x48: 2, x4a: 3, x4c: 4, x50: 5, x52: 6, compute_normals: false,
    };
    let envelop = Envelop {
        level: 2, lists: vec![vec![EnvelopVertex { index: 3, ratio: 0.5, thickness: 0.25, weight: 50.0 }], vec![]],
        joint_area: 0.5, in_type: 1, in_curve: Some((0..200).map(|i| i as f32).collect()), inner: [0.1, 0.9, 0.5, 2.0],
        out_type: 2, out_curve: None, outer: [0.2, 0.8, 0.4, 3.0], ..Envelop::default()
    };
    let mut root = Box::new(Node {
        name: b"root".to_vec(), flag8: true, kind: 4, definitions: vec![Definition::Mesh(mesh)],
        scaling: [1.0, 2.0, 3.0], rotation: [0.1, 0.2, 0.3], translation: [4.0, 5.0, 6.0],
        nb_materials: 1, materials: vec![Some(material)], nb_textures: 1, textures: vec![texture(b"model-texture")],
        texture3d: Some(Texture3D::default()), shape_interp: 2, algorithm: 3,
        dynamics: Some(DynaMatter { density: 2.0, ..DynaMatter::default() }), joint_friction: Some([1.0, 2.0, 3.0]),
        chain_dynamics: true, ik: Some(InvKin { kind: 0x14, nb_joints: 2, rotation: [0.0, 0.5, 0.0] }),
        envelop_type: 6, envelop: Some(envelop), init_transfo: Some([[1.0; 3], [0.5; 3], [0.25; 3]]),
        clusters: vec![Cluster { name: b"c1".to_vec(), count: 2, indices: vec![5, 6], state: 1 }],
        user_nodes: vec![UserNode { tag: b"ABCD".to_vec(), flag: 1, size: 2, data: vec![1, 2] }, ud(b"_new")],
        visibility: 5, motion_blur: 1, matte: 0.5, garbage_matte: 1, x404: 1, x406: 2,
        ud_clusters: [Some(Cluster { name: Vec::new(), count: 2, indices: vec![0, 3], state: 0 }), None,
                      Some(Cluster { name: Vec::new(), count: 1, indices: vec![1], state: 0 })],
        ud_arrays: [Some(vec![vec![ud(b"_v")], vec![]]), None, None],
        x420: 3, uv_sets: 2, uv_domains: Some(vec![vec![11, 12]]), ..Node::default()
    });
    let shapes = vec![
        (1, Definition::Patch(Patch { kind: [1, 2], nu: 2, nv: 2, points: vec![[1.0, 2.0, 3.0]; 4], tags: vec![0, 1, 0, 1],
                                      close: [1, 0], tension: [0.5, 0.25], caps: [1, 0, 1, 0], faceted: 1, ..Patch::default() })),
        // a face's splines have no name in the program (SI_splineTransfer copies none)
        (2, Definition::Face(Face { exterior: spline(b""), count: 1, holes: vec![spline(b"")], nurbs: Vec::new(), is_spline: 1 })),
        (2, Definition::Face(Face { exterior: Spline::default(), count: 2, holes: Vec::new(), nurbs: vec![curve(2), curve(3)], is_spline: 0 })),
        (6, Definition::Spline(spline(b"curve"))),
        (8, Definition::Metaball(Metaball { ratio: 0.75, weight: 3.0, exponent: [1.0, 2.0, 3.0], ..Metaball::default() })),
        (9, Definition::NurbsCurve(curve(4))),
        (10, Definition::NurbsSurface(NurbsSurface {
            order: [3, 4], count: [2, 2], points: vec![NurbsPoint::default(); 4], closed: [1, 0], uniformity: 2,
            nknots: [2, 3], knots: [vec![0.0, 1.0], vec![0.0, 0.5, 1.0]], range: [0.0, 1.0, 0.0, 2.0], border: 1,
            ntrims: 1, trims: vec![NurbsComposite { boundary: 1, ncurves: 2, curves: vec![curve(2), curve(2)] }],
            nprojected: 2, projected: vec![NurbsComposite { boundary: 0, ncurves: 1, curves: vec![curve(3)] }, NurbsComposite::default()],
            steps: [5, 6], caps: [0, 1, 0, 1], ..NurbsSurface::default()
        })),
        (0, Definition::Mesh(Mesh::default())), // a null: the shape is not used
        (5, Definition::Mesh(Mesh::default())), // a joint
    ];
    // sons: one per shape, chained as brothers
    let mut next: Option<Box<Node>> = None;
    for (k, (kind, shape)) in shapes.into_iter().enumerate().rev() {
        let definitions = if kind == 0 || kind == 5 { Vec::new() } else { vec![shape] };
        next = Some(Box::new(Node { name: format!("son{k}").into_bytes(), kind, definitions, brother: next, ..Node::default() }));
    }
    root.son = next;
    root
}

#[test]
fn program_output_round_trip() {
    let root = everything();
    let bytes = write_model(&root);
    assert_eq!(&bytes[..8], &[0x53, 0x80, 0xf6, 0x34, 0x40, 0x80, 0, 0]);
    // a program that has read a material or texture file of 4.0 before: the 3D textures
    // of the model keep their channel (in a fresh one they would get channel 2)
    let f = read_file_with(&bytes, Kind::Model, &mut crate::read::Versions::all(4.0)).unwrap();
    assert_eq!(f.end, bytes.len());
    assert!(f.notes.skipped.is_empty() && f.notes.unknown_plain.is_empty() && f.notes.cut == 0);
    assert_eq!(f.protos, Prototypes::current(4.0));
    let Content::Model(back) = &f.content else { panic!() };
    assert_eq!(back, &root);
    assert_eq!(f.notes.nodes, 10);
    // written with the table it was read with, it is the same file
    let own = write_as_read(&f).unwrap();
    assert!(own.unwritable.is_empty() && own.null_pointers == 0);
    assert_eq!(own.w.out, bytes);
    assert_eq!(write_content(&f.content).unwrap(), bytes);
}

#[test]
fn metaball_lists_come_back_reversed() {
    let system = MetaSystem {
        wire_res: 0.5, render_res: 0.125, show: 1,
        groups: vec![MetaGroup { name: b"g1".to_vec(), primitives: vec![b"a".to_vec(), b"b".to_vec()] },
                     MetaGroup { name: b"g2".to_vec(), primitives: vec![b"c".to_vec()] }],
        unblends: vec![[b"g1".to_vec(), b"g2".to_vec()], [b"g2".to_vec(), b"g1".to_vec()]],
    };
    let root = Node { name: b"m".to_vec(), kind: 7, definitions: vec![Definition::MetaSystem(system.clone())], ..Node::default() };
    let read = |n: &Node| match read_file(&write_model(n), Kind::Model).unwrap().content {
        Content::Model(n) => n,
        _ => panic!(),
    };
    let once = read(&root);
    let Some(Definition::MetaSystem(m)) = once.definitions.first() else { panic!() };
    assert_eq!(m.groups[0].name, b"g2");
    assert_eq!(m.groups[1].primitives, [b"b".to_vec(), b"a".to_vec()]);
    assert_eq!(m.unblends[0], [b"g2".to_vec(), b"g1".to_vec()]);
    // a second pass restores the order
    assert_eq!(read(&once).definitions, vec![Definition::MetaSystem(system)]);
}

#[test]
fn material_file_has_no_texture_lists() {
    let Some(Some(m)) = everything().materials.first().cloned() else { panic!() };
    let bytes = write_material(&m, 4.0);
    assert_eq!(&bytes[..4], &[0x53, 0x80, 0xf6, 0x40]);
    let f = read_file(&bytes, Kind::Material).unwrap();
    assert_eq!(f.end, bytes.len());
    let Content::Material(back) = f.content else { panic!() };
    assert_eq!(back, Material { textures: Vec::new(), texture3d: None, ..m });
}

#[test]
fn wrong_tag_or_header() {
    let data = file(si_file::MAGIC_MATERIAL, 3.8, &[(13, &[1, 0])], b"MTRL", b"x\0");
    // the model reader finds no HRCH tag and reads nothing
    let f = read_file(&data, Kind::Model).unwrap();
    assert_eq!(f.content, Content::None);
    // the model reader refuses a file without a header; the others carry on with version -1
    assert!(read_file(b"plain text, not a database file", Kind::Model).is_none());
    let f = read_file(b"plain text, not a database file", Kind::Texture).unwrap();
    assert!(f.header.is_none() && f.content == Content::None && f.end == 4);
}

#[test]
fn a_file_cut_short_ends_its_lists() {
    let bytes = write_model(&everything());
    for cut in [1200usize, bytes.len() / 2, bytes.len() - 3] {
        let f = read_file(&bytes[..cut], Kind::Model).unwrap();
        assert!(f.end <= cut);
        assert!(matches!(f.content, Content::Model(_)));
    }
    // cut inside the prototype block: the tag that should follow it is never found
    assert_eq!(read_file(&bytes[..200], Kind::Model).unwrap().content, Content::None);
    // a count far beyond the file allocates nothing: 0x7fffffff vertices
    let body = [0, 4, 0x7f, 0xff, 0xff, 0xff, 0, 0];
    let data = file(si_file::MAGIC_GENERAL, 3.8, &[(5, &[2, 3, 0]), (8, &[7, 0]), (12, &[1, 4, 0])], b"HRCH", &body);
    let f = read_file(&data, Kind::Model).unwrap();
    assert!(f.notes.cut > 0);
}

/// A texture's internal shader reference is not copied into the texture the reader
/// returns (`SI_udfCopy`), and a face's splines lose their names (`SI_splineTransfer`).
#[test]
fn what_the_readers_copies_leave_out() {
    let x = Texture3D { udf: Some(Udf { name: b"noise".to_vec(), active: 1, internal: 1, order: 0 }), channel: 2, ..Texture3D::default() };
    let f = read_file(&crate::write::write_texture3d(&x, 4.0), Kind::Texture3D).unwrap();
    let Content::Texture3D(y) = &f.content else { panic!() };
    assert_eq!(y.udf, None);
    let t = Texture2D { udf: Some(Udf { name: b"s".to_vec(), active: 0, internal: 1, order: 0 }), ..Texture2D::default() };
    let f = read_file(&crate::write::write_texture(&t, 4.0), Kind::Texture).unwrap();
    let Content::Texture(y) = &f.content else { panic!() };
    assert_eq!(y.udf, None);
    let face = Face { exterior: Spline { name: b"outline".to_vec(), ..Spline::default() }, count: 0, holes: Vec::new(), nurbs: Vec::new(), is_spline: 1 };
    let root = Node { name: b"f".to_vec(), kind: 2, definitions: vec![Definition::Face(face)], ..Node::default() };
    let f = read_file(&write_model(&root), Kind::Model).unwrap();
    let Content::Model(n) = &f.content else { panic!() };
    let Some(Definition::Face(back)) = n.definitions.first() else { panic!() };
    assert!(back.exterior.name.is_empty());
}
