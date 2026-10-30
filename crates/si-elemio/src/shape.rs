// SPDX-License-Identifier: MIT
//! Shape files (`.shp`, magic 0x5380f642): the shapes of one model, which shape animation
//! blends between. Finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `binary.c` (the reader), `binary_b.c` (the writer),
//! `utilio.c` (the prototype block) and the element dictionary's file (the two functions that
//! are handed the file name).
//!
//! A model keeps a list of *definitions* of its geometry, all of the model's one kind: the
//! first is the shape the model file gave it, the others are its further shapes; and beside
//! the list a *current* definition, the one being shown. A shape file stores the current
//! definition and the list without its first entry.
//!
//! | part | bytes | |
//! |---|---|---|
//! | header | 88 | magic 0x5380f642, version, comment |
//! | tag | 4 | `PROT` if a prototype block follows |
//! | prototype block | | see [`crate::proto`]; then a second tag |
//! | (tag) | 4 | the kind: `PTCH`, `FACE`, `MESH`, `SPLN`, `NCRV` or `NSRF` |
//! | count | 2 | if negative, the current definition comes first |
//! | number | 2 | only if the count was −2: stored in the model at +0x2e2 |
//! | current definition | | only if the count was negative: one record of the model's kind |
//! | count | 2 | only if the first was negative: the real count |
//! | definitions | | that many records of the model's kind |
//!
//! The kind of the records is the *model's* (`__SI_Model` +0x30), not the tag's: the reader
//! uses the tag only to refuse the list of definitions when it names another kind, and the
//! current definition is read as a record of the model's kind whatever the tag says.

use crate::geom::{definition_min_size, read_geometry_in, write_geometry_in, GeomState, Geometry};
use crate::io::{fread_i16, fread_u32, End, Reader, Writer};
use crate::proto::{self, Prototypes};
use si_file::{write_db_header, MAGIC_SHAPE};

/// The tag the writer puts before the definitions of a model type, which the reader
/// compares with: 1 `PTCH`, 2 `FACE`, 4 `MESH`, 6 `SPLN`, 9 `NCRV`, 10 `NSRF`. No other
/// type has one.
pub fn tag_of(model_type: i32) -> Option<[u8; 4]> {
    Some(*match model_type {
        1 => b"PTCH",
        2 => b"FACE",
        4 => b"MESH",
        6 => b"SPLN",
        9 => b"NCRV",
        10 => b"NSRF",
        _ => return None,
    })
}

/// The model type a tag stands for: what a model must be for a file with this tag to give
/// it its definitions. (The original never asks this; it is for a reader with no model.)
pub fn model_type_of(tag: u32) -> Option<i32> {
    [1, 2, 4, 6, 9, 10].into_iter().find(|&t| tag_of(t).map(u32::from_be_bytes) == Some(tag))
}

/// What a shape file reads from and writes to its model (`__SI_Model`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeModel {
    /// +0x30: the model's type (`SI_ModelType`): 1 patch, 2 face, 4 mesh, 6 spline, 9 NURBS
    /// curve, 10 NURBS surface
    pub model_type: i32,
    /// +0x38, with the count at +0x34 (16 bits): the definitions. The first is the model's
    /// own shape and does not go into a shape file.
    pub definitions: Vec<Geometry>,
    /// +0x3c: the current definition
    pub current: Option<Geometry>,
    /// +0x2e2: the number a shape file stores after a count of −2. Named from code: the
    /// routines that blend a model's shapes by their weights (`si_meshWeightInterpolate`
    /// and its like for the other kinds) divide each weight by the sum of the weights when
    /// this is zero and take the weights as they are otherwise; the Shape Info dialog sets
    /// it from a choice. Zero in every file of the corpus.
    pub weight_mode: i16,
    /// The model's number of UV domains (+0x424), and with it the one program static the
    /// geometry readers use.
    pub state: GeomState,
}

impl ShapeModel {
    /// A model of a type with no definitions yet and one UV domain.
    pub fn new(model_type: i32) -> ShapeModel {
        ShapeModel { model_type, definitions: Vec::new(), current: None, weight_mode: 0, state: GeomState::default() }
    }
}

/// What reading a shape file gives besides what it puts in the model.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRead {
    /// The header's version, −1.0 if the header was not accepted.
    pub version: f32,
    pub comment: Vec<u8>,
    /// The file began with a prototype block.
    pub has_prototypes: bool,
    /// The prototypes read, empty if there was no block.
    pub prototypes: Prototypes,
    /// Where the prototype block ended (the offset of the second tag), if there was one.
    pub prototypes_end: usize,
    /// The tag in front of the count.
    pub tag: u32,
    /// The first count was negative: the file gave a current definition.
    pub has_current: bool,
    /// The count of definitions, as last read.
    pub count: i16,
    /// What the original returns: true unless the model is of a kind with a tag and the
    /// file's tag is another (the definitions are then not read).
    pub accepted: bool,
    /// The model had no current definition after the read and got a copy of its second
    /// definition (`SI_meshCopy` and its like for the other kinds).
    pub current_copied: bool,
    pub end: End,
    /// Offset of the first byte not read.
    pub pos: usize,
    pub clamped: bool,
}

/// Reads a shape file into a model [soft 0x083f1798 SI_shapeBinaryRead], as the program
/// loads one [soft 0x08110128 SI_elemdictReadShape] (which only passes the name and the
/// model on).
///
/// In order: the header, whose version becomes `fileVersion`; a tag; if it is `PROT`, the
/// prototype block and another tag; a 16-bit count. If the count is negative: if it is −2, a
/// 16-bit number for the model's +0x2e2; then one definition of the model's kind
/// ([`read_geometry_in`]), which becomes the current one; then the count again. (Any
/// negative count brings a current definition, not only −2; the writer writes −2 or none.)
///
/// Then the definitions, by the model's type: for a type that has a tag (1, 2, 4, 6, 9, 10),
/// if the file's tag is that tag, `count` records are read and each is added to the model's
/// list; if it is another tag nothing more is read and the result is "not accepted". For any
/// other type one field with a 32-bit size is skipped, once, whatever the count.
///
/// Last, a model that still has no current definition gets a copy of its second definition
/// (the first of the file's, if the model came with its own shape). The original takes the
/// second entry of the list without looking at the list's length; with fewer than two there
/// is none to take, and here the model then stays without.
///
/// The tag and the count are read into variables the original does not initialise; here
/// they start at zero, so a file that ends before them has tag 0 and no definitions.
///
/// A count of definitions that the bytes left cannot hold is cut to what they can and
/// `clamped` set (by the least a definition takes with the file's prototypes; if that is
/// nothing, none is read). If a record's reader does not come to an end ([`End::Eof`])
/// reading stops there with what was read.
///
/// What the original does besides, not reproduced: the patch that becomes current is passed
/// to `SI_patchUpdateSurface`; adding a definition (`SI_modelAddDefinition`) also renames
/// the model's parts (`SI_modelChangeName`); and the model's file name (+0x2d4) is replaced
/// by the cleaned-up name of this file (`si_filenameCleanup`).
pub fn read_file(r: &mut Reader, model: &mut ShapeModel) -> ShapeRead {
    let (version, comment) = crate::io::read_header(r);
    let mut out = ShapeRead {
        version,
        comment,
        has_prototypes: false,
        prototypes: Prototypes::new(),
        prototypes_end: 0,
        tag: 0,
        has_current: false,
        count: 0,
        accepted: true,
        current_copied: false,
        end: End::Zero,
        pos: 0,
        clamped: false,
    };
    fread_u32(&mut r.s, &mut out.tag);
    r.st.file_version = version;
    if out.tag == proto::TAG {
        out.has_prototypes = true;
        proto::read_prototypes(r, &mut out.prototypes);
        out.prototypes_end = r.s.tell();
        fread_u32(&mut r.s, &mut out.tag);
    }
    fread_i16(&mut r.s, &mut out.count);
    if out.count < 0 {
        out.has_current = true;
        if out.count == -2 {
            fread_i16(&mut r.s, &mut out.count);
            model.weight_mode = out.count;
        }
        let (g, end) = read_geometry_in(r, &out.prototypes, model.model_type, &mut model.state);
        if let Some(g) = g {
            model.current = Some(g);
        }
        out.end = end;
        if end == End::Zero {
            fread_i16(&mut r.s, &mut out.count);
        }
    }
    if out.end == End::Zero {
        match tag_of(model.model_type) {
            Some(tag) if u32::from_be_bytes(tag) == out.tag => {
                let want = out.count.max(0) as usize;
                let have = match definition_min_size(&out.prototypes, model.model_type) {
                    0 => 0,
                    min => (r.s.len() - r.s.tell()) / min,
                };
                if want > have {
                    r.clamped = true;
                }
                for _ in 0..want.min(have) {
                    let (g, end) = read_geometry_in(r, &out.prototypes, model.model_type, &mut model.state);
                    if let Some(g) = g {
                        // SI_modelAddDefinition: the list grows by one
                        model.definitions.push(g);
                    }
                    if end == End::Eof {
                        out.end = End::Eof;
                        break;
                    }
                }
                if out.end == End::Zero && model.current.is_none() {
                    model.current = model.definitions.get(1).cloned();
                    out.current_copied = model.current.is_some();
                }
            }
            Some(_) => out.accepted = false,
            None => {
                r.skip_field(0xffff);
            }
        }
    }
    out.pos = r.s.tell();
    out.clamped = r.clamped;
    out
}

/// Writes what follows the header of a shape file (part of
/// [soft 0x083f6314 SI_shapeBinaryWrite]): the prototype block, always; then, for a model
/// type that has a tag, the tag; if the model has a current definition, the count −2, the
/// model's +0x2e2 and that definition; the number of definitions less one; and the
/// definitions from the second on. For any other model type nothing follows the block.
///
/// The count is the model's 16-bit count less one, so a model with no definition at all
/// writes −1; the reader takes any negative count as the announcement of a current
/// definition, so such a file, if it had none, does not read back.
///
/// The original picks each record's writer by the model's type and trusts the definitions
/// to be of that kind; here each definition is written as what it is. Its index into the
/// list is a signed short compared with the unsigned count, so a list of more than 32,768
/// entries sends it before the list's start; here the entries up to that index are written.
pub fn write_body(w: &mut Writer, model: &ShapeModel) {
    proto::write_prototypes(w);
    let Some(tag) = tag_of(model.model_type) else { return };
    w.out.extend_from_slice(&tag);
    let uv_domains = model.state.uv_domains;
    if let Some(current) = model.current.as_ref() {
        w.put_u16(-2i16 as u16);
        w.put_u16(model.weight_mode as u16);
        write_geometry_in(w, current, uv_domains);
    }
    w.put_u16((model.definitions.len() as u16).wrapping_sub(1));
    for g in model.definitions.iter().take(0x8000).skip(1) {
        write_geometry_in(w, g, uv_domains);
    }
}

/// A shape file as the program saves it [soft 0x083f6314 SI_shapeBinaryWrite]
/// [soft 0x0811094c SI_elemdictWriteShape]: the header with the output version and the
/// default comment, then [`write_body`]. (The original also replaces the model's file name,
/// as the reader does, and reports a file it cannot open or flush through
/// `SI_saveElemErrorCode`: 1 and 2.)
pub fn write_file(w: &mut Writer, model: &ShapeModel) {
    write_db_header(&mut w.out, w.version_output, MAGIC_SHAPE);
    write_body(w, model);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Mesh, Patch, PolyNode, Polygon, Spline, SplineKey, Vertex};

    fn spline(name: &[u8], n: usize) -> Geometry {
        let keys = (0..n)
            .map(|i| SplineKey {
                previous_tangent: [i as f32, 0.5, -1.0],
                position: [i as f32 + 0.25, 2.0, 3.0],
                next_tangent: [0.0, i as f32, 9.0],
                f_24: 1.0,
                constraint: 1,
                segment: 2,
                flag: i as i16,
            })
            .collect();
        Geometry::Spline(Spline { name: name.to_vec(), phantom: 0, close: 1, kind: 3, nb_keys: n as u16, keys, tension: 0.5, step: 7, old_points: false })
    }

    fn model_of(model_type: i32, own: Geometry) -> ShapeModel {
        let mut m = ShapeModel::new(model_type);
        m.definitions.push(own);
        m
    }

    #[test]
    fn round_trip_of_a_spline_model() {
        let mut m = model_of(6, spline(b"own", 1));
        m.definitions.push(spline(b"one", 2));
        m.definitions.push(spline(b"two", 3));
        m.current = Some(spline(b"now", 2));
        m.weight_mode = 5;
        let mut w = Writer::new();
        write_file(&mut w, &m);
        let at = w.out.windows(4).position(|x| x == b"SPLN").unwrap();
        // −2, the number, the current spline, the count 2
        assert_eq!(&w.out[at + 4..at + 8], &[0xff, 0xfe, 0, 5]);
        let mut back = model_of(6, spline(b"own", 1));
        let mut r = Reader::new(&w.out);
        let f = read_file(&mut r, &mut back);
        assert_eq!((f.end, f.pos, f.accepted, f.count, f.has_current), (End::Zero, w.out.len(), true, 2, true));
        assert_eq!(f.tag, u32::from_be_bytes(*b"SPLN"));
        assert_eq!(f.prototypes_end, at);
        assert_eq!(r.st.file_version, 4.0);
        back.state = m.state.clone();
        assert_eq!(back, m);
    }

    #[test]
    fn a_model_without_a_current_definition_gets_a_copy_of_the_second() {
        let mut m = model_of(6, spline(b"own", 0));
        m.definitions.push(spline(b"one", 1));
        let mut w = Writer::new();
        write_file(&mut w, &m);
        let mut back = model_of(6, spline(b"own", 0));
        let f = read_file(&mut Reader::new(&w.out), &mut back);
        assert!(f.current_copied && !f.has_current);
        assert_eq!(back.current, Some(spline(b"one", 1)));
        // a model that came with no shape of its own has no second definition to copy
        let mut bare = ShapeModel::new(6);
        let f = read_file(&mut Reader::new(&w.out), &mut bare);
        assert_eq!((bare.definitions.len(), bare.current.is_none(), f.current_copied), (1, true, false));
    }

    #[test]
    fn the_tag_must_be_the_models() {
        let mut m = model_of(6, spline(b"own", 0));
        m.definitions.push(spline(b"one", 1));
        let mut w = Writer::new();
        write_file(&mut w, &m);
        // read as a patch model: the tag is SPLN, so the definitions are refused
        let mut other = model_of(1, Geometry::Patch(Patch::default()));
        let f = read_file(&mut Reader::new(&w.out), &mut other);
        assert!(!f.accepted);
        assert_eq!(other.definitions.len(), 1);
        assert_eq!(model_type_of(f.tag), Some(6));
        // a model of a kind with no tag skips one long field and is "accepted"
        let mut null = ShapeModel::new(3);
        let mut b = w.out[..w.out.windows(4).position(|x| x == b"SPLN").unwrap() + 4].to_vec();
        b.extend_from_slice(&[0, 4, 0, 0, 0, 2, 9, 9, 7]);
        let f = read_file(&mut Reader::new(&b), &mut null);
        assert_eq!((f.accepted, f.count, f.pos), (true, 4, b.len() - 1));
        // and its writer stops after the prototype block
        let mut w3 = Writer::new();
        write_file(&mut w3, &null);
        assert_eq!(&w3.out[w3.out.len() - 2..], &[0, 0]);
        assert!(!w3.out.windows(4).any(|x| x == b"SPLN"));
    }

    #[test]
    fn any_negative_count_announces_a_current_definition() {
        // no prototype block: the tag, the count −1, then a spline with its field numbers
        // from the file (name, then the closing zero), then the count 0
        let mut b = Vec::new();
        write_db_header(&mut b, 4.0, MAGIC_SHAPE);
        b.extend_from_slice(b"SPLN");
        b.extend_from_slice(&[0xff, 0xff, 0, 8, b'a', 0, 0, 0, 0, 0]);
        let mut m = ShapeModel::new(6);
        let f = read_file(&mut Reader::new(&b), &mut m);
        assert_eq!((f.has_current, f.has_prototypes, f.end, f.pos), (true, false, End::Zero, b.len()));
        assert_eq!(m.weight_mode, 0);
        match m.current {
            Some(Geometry::Spline(s)) => assert_eq!(s.name, b"a"),
            other => panic!("{other:?}"),
        }
        // a model with no definitions writes the count −1
        let mut w = Writer::new();
        write_body(&mut w, &ShapeModel::new(6));
        assert_eq!(&w.out[w.out.len() - 6..], b"SPLN\xff\xff");
    }

    #[test]
    fn a_mesh_model_keeps_its_uv_domains() {
        let node = |v: i32| PolyNode { vertex: v, normal: [0.0, 1.0, 0.0], uv: [0.25, 0.75], colour: 0xff01_0203, more_uv: vec![[1.5, 2.5], [3.5, 4.5]] };
        let mesh = Mesh {
            nb_vertices: 3,
            vertices: (0..3).map(|i| Vertex { position: [i as f32, 0.0, 1.0], flag: 1 }).collect(),
            nb_polygons: 1,
            polygons: vec![Polygon { normal: [0.0; 3], nb_nodes: 3, nodes: vec![node(0), node(1), node(2)], material: 2, flag: 4 }],
            normals_read: true,
            uv_domains: 3,
            ..Mesh::default()
        };
        let mut m = model_of(4, Geometry::Mesh(Mesh::default()));
        m.state.uv_domains = 3;
        m.definitions.push(Geometry::Mesh(mesh.clone()));
        let mut w = Writer::new();
        write_file(&mut w, &m);
        let mut back = model_of(4, Geometry::Mesh(Mesh::default()));
        let f = read_file(&mut Reader::new(&w.out), &mut back);
        assert_eq!((f.end, f.pos), (End::Zero, w.out.len()));
        // the file's field 0x413 set the model's count before the nodes were read
        assert_eq!(back.state.uv_domains, 3);
        // the mesh written had no edge list; one read back has an (empty) one
        let expect = Mesh { edge_field: true, ..mesh };
        assert_eq!(back.definitions[1], Geometry::Mesh(expect));
    }

    #[test]
    fn a_count_beyond_the_file_is_cut() {
        let mut b = Vec::new();
        write_db_header(&mut b, 4.0, MAGIC_SHAPE);
        b.extend_from_slice(b"SPLN");
        b.extend_from_slice(&[0x7f, 0xff, 0, 0]);
        let mut m = ShapeModel::new(6);
        let mut r = Reader::new(&b);
        let f = read_file(&mut r, &mut m);
        assert!(f.clamped);
        // two bytes are left, and a spline with no prototype takes two at the least
        assert_eq!(m.definitions.len(), 1);
        // records that take no bytes (a prototype with nothing in it) are not multiplied
        let mut b = Vec::new();
        write_db_header(&mut b, 4.0, MAGIC_SHAPE);
        b.extend_from_slice(b"PROT\x00\x03\x00\x01\x00\x00\x00\x00SPLN\x7f\xff\x00\x00\x00\x00");
        let mut m = ShapeModel::new(6);
        let f = read_file(&mut Reader::new(&b), &mut m);
        assert_eq!((f.clamped, f.count, m.definitions.len(), f.pos), (true, 0x7fff, 0, b.len() - 4));
        // an empty file: no tag, no count, nothing read
        let mut m = ShapeModel::new(6);
        let f = read_file(&mut Reader::new(&[]), &mut m);
        assert_eq!((f.version, f.tag, f.accepted, m.definitions.len()), (-1.0, 0, false, 0));
    }

    #[test]
    fn cut_and_damaged_files_are_read_without_trouble() {
        use crate::geom::{Face, NurbsComposite, NurbsCurve, NurbsPoint, NurbsSurface};
        let curve = |n: i32| NurbsCurve {
            nb_points: n,
            points: (0..n).map(|i| NurbsPoint { xyzw: [i as f64, 1.0, 2.0, 1.0], flag: i }).collect(),
            nb_knots: n + 4,
            knots: (0..n + 4).map(|i| i as f64).collect(),
            ..NurbsCurve::default()
        };
        let comp = NurbsComposite { is_boundary: 1, nb_curves: 2, curves: vec![curve(2), curve(3)] };
        let surface = NurbsSurface {
            nb_points: [2, 2],
            point_count: 4,
            points: vec![NurbsPoint::default(); 4],
            nb_knots: [6, 6],
            knots: [vec![0.5; 6], vec![1.5; 6]],
            nb_trims: 1,
            trims: vec![comp.clone()],
            nb_projected: 1,
            projected: vec![comp],
            ..NurbsSurface::default()
        };
        let node = |v: i32| PolyNode { vertex: v, more_uv: vec![[0.5, 0.5]], ..PolyNode::default() };
        let mesh = Mesh {
            nb_vertices: 3,
            vertices: vec![Vertex::default(); 3],
            nb_polygons: 1,
            polygons: vec![Polygon { normal: [0.0; 3], nb_nodes: 3, nodes: vec![node(0), node(1), node(2)], material: 0, flag: 0 }],
            nb_edges: 3,
            edges: vec![crate::geom::Edge::default(); 3],
            edge_field: true,
            normals_read: true,
            uv_domains: 2,
            ..Mesh::default()
        };
        let patch = Patch { nb_u: 2, nb_v: 3, points: vec![[1.0; 4]; 6], tags: vec![1; 6], ..Patch::default() };
        let face = Face { is_spline: 0, count: 2, curves: vec![curve(2), curve(4)], ..Face::default() };
        let holes = match spline(b"h", 3) {
            Geometry::Spline(s) => Face { count: 2, holes: vec![s.clone(), s.clone()], exterior: s, ..Face::default() },
            _ => unreachable!(),
        };
        let all = [
            (10, Geometry::NurbsSurface(surface)),
            (4, Geometry::Mesh(mesh)),
            (1, Geometry::Patch(patch)),
            (2, Geometry::Face(face)),
            (2, Geometry::Face(holes)),
            (9, Geometry::NurbsCurve(curve(5))),
            (6, spline(b"s", 4)),
        ];
        let mut seed = 0x2545_f491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for (model_type, g) in all {
            let mut m = model_of(model_type, g.clone());
            m.state.uv_domains = 2;
            m.definitions.push(g.clone());
            m.definitions.push(g.clone());
            m.current = Some(g);
            let mut w = Writer::new();
            write_file(&mut w, &m);
            // whole: it reads back
            let mut back = ShapeModel::new(model_type);
            let f = read_file(&mut Reader::new(&w.out), &mut back);
            assert_eq!((f.end, f.pos, f.clamped, back.definitions.len()), (End::Zero, w.out.len(), false, 2), "type {model_type}");
            assert_eq!(back.current, m.current, "type {model_type}");
            // cut at every length, with and without its prototype block
            let block_end = f.prototypes_end;
            for n in 0..w.out.len() {
                let mut back = ShapeModel::new(model_type);
                let mut r = Reader::new(&w.out[..n]);
                let f = read_file(&mut r, &mut back);
                assert!(f.pos <= n && back.definitions.len() <= 2);
                if n > block_end {
                    let mut bare = w.out[..88].to_vec();
                    bare.extend_from_slice(&w.out[block_end..n]);
                    read_file(&mut Reader::new(&bare), &mut ShapeModel::new(model_type));
                }
            }
            // damaged: bytes changed at random, counts included
            for _ in 0..400 {
                let mut b = w.out.clone();
                for _ in 0..1 + next() % 4 {
                    let at = 88 + next() as usize % (b.len() - 88);
                    b[at] = next() as u8;
                }
                let mut back = ShapeModel::new(model_type);
                let f = read_file(&mut Reader::new(&b), &mut back);
                assert!(f.pos <= b.len());
            }
        }
    }
}
