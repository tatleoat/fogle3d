// SPDX-License-Identifier: MIT
//! Unit tests of the image form on made-up files (written by this crate's own writers).

use super::material::mat;
use super::node::node;
use super::texture::{lclmap, t3d};
use super::{geom::mesh, matio, node as reader, MemOutside};
use crate::{Definition, Material, Mesh, Node, PolyNode, Polygon, Texture2D, Texture3D, Udf, UserNode, Vertex};
use si_mem::Mem;

fn image() -> (Mem, MemOutside) {
    let mut m = Mem::new();
    super::init(&mut m);
    (m, MemOutside::default())
}

#[test]
fn a_material_file_becomes_a_material_record() {
    si_rt::skip_without_install!();
    let (mut m, mut o) = image();
    let value = Material {
        name: b"brass".to_vec(),
        shading: 2,
        diffuse: [0.25, 0.5, 0.75],
        transparency: 0.125,
        udf_material: Some(Udf { name: b"marble".to_vec(), active: 1, internal: 0, order: 3 }),
        user_data: vec![UserNode { tag: b"_tag".to_vec(), flag: 1, size: 3, data: vec![7, 8, 9] }],
        ..Material::default()
    };
    o.files.add(b"/db/el.mtr", crate::write_material(&value, 4.0));
    let name = m.place_cstr(b"/db/el");
    let p = matio::material_read_binary_file(&mut m, &mut o, name).addr();
    assert!(m.fault().is_none(), "{:?}", m.fault());
    assert_ne!(p, 0);
    assert_eq!(m.cstr(p + mat::NAME), b"brass");
    assert_eq!(m.i16(p + mat::SHADING), 2);
    assert_eq!([m.f32(p + mat::DIFFUSE), m.f32(p + mat::DIFFUSE + 4), m.f32(p + mat::DIFFUSE + 8)], [0.25, 0.5, 0.75]);
    assert_eq!(m.f32(p + mat::TRANSPARENCY), 0.125);
    let udf = super::udf::Udf::SIZE; // the record's size is asserted by `record!`
    assert_eq!(udf, 0x3c);
    let u = si_mem::Ptr32::<super::udf::Udf>::new(m.u32(p + mat::UDF_MATERIAL));
    assert_eq!((m.cstr(m[u].name.addr()), m[u].active, m[u].order), (b"marble".to_vec(), 1, 3));
    let d = si_mem::Ptr32::<super::userdata::UserNode>::new(m.u32(p + mat::USER_DATA));
    assert_eq!((m.cstr(m[d].tag.addr()), m[d].size, m.bytes(m[d].data.addr(), 3).to_vec()), (b"_tag".to_vec(), 3, vec![7, 8, 9]));
    // the stream was closed and the prototype table freed: the table's block is among the frees
    assert!(o.files.stream(o.files.last()).unwrap().closed);
    assert!(!m.freed().is_empty());
    // the shader reference is in the list of references alive. The list has two slots:
    // the file stores three more references with an empty name, each of which is
    // allocated (the first of them in a new, second slot) and disposed of at once
    assert_eq!(m[super::globals::SI_NUM_UDF], 2);
    assert_eq!(m.u32(m[super::globals::SI_UDF_LIST]), u.addr());
    assert_eq!(m.u32(m[super::globals::SI_UDF_LIST] + 4), 0);
}

#[test]
fn a_texture_keeps_the_working_copy_defaults_and_loses_the_fifth_crop_short() {
    si_rt::skip_without_install!();
    let (mut m, mut o) = image();
    let value = Texture2D { name: b"pic".to_vec(), filename: b"wood".to_vec(), crop: [1, 2, 3, 4, 5], reflectivity: 0.5, ..Texture2D::default() };
    o.files.add(b"t.txt", crate::write_texture(&value, 4.0));
    let name = m.place_cstr(b"t");
    let p = matio::texture_read_binary_file(&mut m, &mut o, name).addr();
    assert!(m.fault().is_none(), "{:?}", m.fault());
    assert_eq!(m.cstr(m.u32(p + lclmap::PICTURE)), b"pic");
    assert_eq!(m.cstr(m.u32(p + lclmap::NAME)), b"wood");
    assert_eq!([m.i16(p + 0x30), m.i16(p + 0x32), m.i16(p + 0x34), m.i16(p + 0x36)], [1, 2, 3, 4]);
    // token 25 reads a fifth short into the working copy; the copy routine leaves it behind
    assert_eq!(m.i16(p + 0x38), 0);
    assert_eq!(m.f32(p + lclmap::REFLECTIVITY), 0.5);
    assert_eq!(m.u16(p + lclmap::USES_REFLECTIVITY), 1);
    assert_eq!(m.f32(p + lclmap::SCALE_U), 1.0);
}

#[test]
fn a_3d_texture_of_version_2_1_gets_channel_2() {
    si_rt::skip_without_install!();
    for (version, channel) in [(2.1f32, 2), (2.2, 1), (4.0, 1)] {
        let (mut m, mut o) = image();
        o.files.add(b"t.t3D", crate::write_texture3d(&Texture3D { name: b"cloud".to_vec(), ..Texture3D::default() }, version));
        let name = m.place_cstr(b"t");
        let p = matio::texture3d_read_binary_file(&mut m, &mut o, name).addr();
        assert!(m.fault().is_none(), "{:?}", m.fault());
        assert_eq!(m.cstr(m.u32(p + t3d::NAME)), b"cloud");
        // the float 2.1 is below the double 2.1 the reader compares with
        assert_eq!(m.i16(p + t3d::CHANNEL), channel, "version {version}");
    }
}

#[test]
fn a_model_file_becomes_nodes_and_a_mesh() {
    si_rt::skip_without_install!();
    let (mut m, mut o) = image();
    let quad = Polygon { nnodes: 3, nodes: (0..3).map(|v| PolyNode { vertex: v, ..PolyNode::default() }).collect(), material: 9, ..Polygon::default() };
    let geometry = Mesh {
        nvertices: 3,
        vertices: vec![Vertex { pos: [0.0, 0.0, 0.0], flag: 0 }, Vertex { pos: [1.0, 0.0, 0.0], flag: 0 }, Vertex { pos: [0.0, 2.0, 0.0], flag: 0 }],
        npolygons: 1,
        polygons: vec![quad],
        ..Mesh::default()
    };
    let son = Node { name: b"tri".to_vec(), kind: 4, definitions: vec![Definition::Mesh(geometry)], translation: [1.0, 2.0, 3.0], ..Node::default() };
    let root = Node { name: b"root".to_vec(), son: Some(Box::new(son)), ..Node::default() };
    let file = o.files.open_bytes(b"model", crate::write_model(&root));
    let p = reader::fdmodel_binary_read(&mut m, &mut o, file, 0, 0);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    assert_eq!(m.cstr(m.u32(p + node::NAME)), b"root");
    assert_eq!(m.i32(p + node::TYPE), 0);
    let s = m.u32(p + node::SON);
    assert_eq!((m.cstr(m.u32(s + node::NAME)), m.i32(s + node::TYPE), m.u32(s + node::FATHER)), (b"tri".to_vec(), 4, p));
    assert_eq!(m.f32(s + node::TRANSLATION + 8), 3.0);
    let g = m.u32(s + node::GEOMETRY);
    assert_eq!((m.i32(g + mesh::NB_VERTICES), m.i32(g + mesh::NB_POLYGONS)), (3, 1));
    assert_eq!(m.f32(m.u32(g + mesh::VERTICES) + 2 * mesh::VERTEX + 4), 2.0);
    let poly = m.u32(g + mesh::POLYGONS);
    assert_eq!(m.u16(poly + mesh::POLYGON_NB_NODES), 3);
    // the node has no material: an index that is out of range becomes 0
    assert_eq!(m.i16(poly + mesh::POLYGON_MATERIAL), 0);
    assert_eq!(m.i32(m.u32(poly + mesh::POLYGON_NODES) + 2 * mesh::NODE), 2);
    // what the reader asked of the parts that are not on the image: the rebuild of the
    // mesh, the name table, the matrices of both nodes, the envelopes
    let asked: Vec<&str> = o.calls.iter().map(|c| c.0.name).collect();
    for name in ["SI_meshUpdateNodes", "SI_modelChangeName", "SI_modelUpdateMatrix", "SI_modelUpdateSonState", "SI_envelopRebuild"] {
        assert!(asked.contains(&name), "{name} was not asked");
    }
}

#[test]
fn a_file_that_ends_in_a_field_leaves_the_bytes_that_arrived() {
    si_rt::skip_without_install!();
    let (mut m, mut o) = image();
    let p = m.place_bytes(&[0x11, 0x22, 0x33, 0x44]);
    let file = o.files.open_bytes(b"f", vec![0xaa, 0xbb, 0xcc]);
    // a 4-byte item of which three bytes arrive: stored as they came, not swapped
    assert_eq!(super::io::byteswap_fread(&mut m, &mut o, p, 4, 1, file), 0);
    assert_eq!(m.bytes(p, 4), [0xaa, 0xbb, 0xcc, 0x44]);
}

fn read_model(root: &Node) -> (Mem, MemOutside, u32) {
    let (mut m, mut o) = image();
    let file = o.files.open_bytes(b"model", crate::write_model(root));
    let p = reader::fdmodel_binary_read(&mut m, &mut o, file, 0, 0);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    assert_ne!(p, 0);
    (m, o, p)
}

#[test]
fn a_face_holds_its_outline_and_holes_as_records_inside_it_without_names() {
    si_rt::skip_without_install!();
    use super::spline::{face, spline};
    use crate::{Face, Spline, SplineKey};
    let key = |x: f32| SplineKey { prev: [x, 0.0, 0.0], pos: [x, 1.0, 0.0], next: [x, 2.0, 0.0], constraint: 1, segment: 2, flag: 3 };
    let outline = Spline { name: b"outline".to_vec(), kind: 2, close: 1, nkeys: 3, keys: vec![key(1.0), key(2.0), key(3.0)], step: 7, ..Spline::default() };
    let hole = Spline { name: b"hole".to_vec(), kind: 0, close: 1, nkeys: 2, keys: vec![key(5.0), key(6.0)], ..Spline::default() };
    let f = Face { exterior: outline, count: 1, holes: vec![hole], ..Face::default() };
    let (m, o, p) = read_model(&Node { name: b"f".to_vec(), kind: 2, definitions: vec![Definition::Face(f)], ..Node::default() });
    let g = m.u32(p + node::GEOMETRY);
    assert_eq!((m.i16(g + face::IS_SPLINE), m.i16(g + face::NB_HOLES)), (1, 1));
    let s = g + face::OUTLINE;
    // the outline was read as a spline of its own, copied into the face and freed: the
    // copy has the keys and no name
    assert_eq!((m.u32(s + spline::NAME), m.i32(s + spline::TYPE), m.u16(s + spline::NB_KEYS), m.i16(s + spline::STEP)), (0, 2, 3, 7));
    let k = m.u32(s + spline::KEYS) + 2 * spline::KEY;
    assert_eq!((m.f32(k), m.f32(k + spline::KEY_POSITION + 4), m.f32(k + spline::KEY_NEXT + 4), m.i16(k + spline::KEY_FLAG)), (3.0, 1.0, 2.0, 3));
    let h = m.u32(g + face::HOLES);
    assert_eq!((m.u32(h + spline::NAME), m.u16(h + spline::NB_KEYS), m.f32(m.u32(h + spline::KEYS) + spline::KEY)), (0, 2, 6.0));
    // each spline read had its matrix asked for; the two splines read were disposed of
    assert_eq!(o.calls.iter().filter(|c| c.0.name == "SI_splineUpdateMatrix").count(), 2);
    assert_eq!(m[super::globals::SPLINE_COUNT], 2);
}

#[test]
fn a_patch_and_a_nurbs_surface_become_their_records() {
    si_rt::skip_without_install!();
    use super::nurbs::{crv, srf};
    use super::patch::patch;
    use crate::{NurbsPoint, NurbsSurface, Patch};
    let value = Patch { kind: [1, 2], nu: 2, nv: 2, points: vec![[1.0; 3], [2.0; 3], [3.0; 3], [4.0; 3]], tags: vec![0, 1, 0, 1], close: [0, 5], ..Patch::default() };
    let (m, _, p) = read_model(&Node { name: b"p".to_vec(), kind: 1, definitions: vec![Definition::Patch(value)], ..Node::default() });
    let g = m.u32(p + node::GEOMETRY);
    assert_eq!((m.i32(g + patch::TYPE_U), m.i32(g + patch::TYPE_V), m.u16(g + patch::NB_U), m.u16(g + patch::NB_V)), (1, 2, 2, 2));
    // a closed flag that is not 0 becomes 1; a point is a vector and a 1; the tags follow
    assert_eq!((m.i16(g + patch::CLOSED_U), m.i16(g + patch::CLOSED_V)), (0, 1));
    let points = m.u32(g + patch::POINTS);
    assert_eq!((m.f32(points + 0x30), m.f32(points + 0x3c), m.i16(m.u32(g + patch::TAGS) + 6)), (4.0, 1.0, 1));

    let point = |x: f64| NurbsPoint { xyzw: [x, 2.0 * x, 0.0, 1.0], flag: 1 };
    let value = NurbsSurface { count: [2, 1], points: vec![point(1.0), point(2.0)], nknots: [3, 0], knots: [vec![0.0, 0.5, 1.0], Vec::new()], ..NurbsSurface::default() };
    let (m, _, p) = read_model(&Node { name: b"s".to_vec(), kind: 10, definitions: vec![Definition::NurbsSurface(value)], ..Node::default() });
    let g = m.u32(p + node::GEOMETRY);
    assert_eq!((m.i32(g + srf::NB_POINTS), m.i32(g + srf::V + srf::NB_POINTS), m.i32(g + srf::TOTAL_POINTS)), (2, 1, 2));
    let second = m.u32(g + srf::POINTS) + crv::POINT;
    assert_eq!((m.f64(second), m.f64(second + 8), m.f64(second + 0x18), m.i32(second + crv::POINT_FLAG)), (2.0, 4.0, 1.0, 1));
    assert_eq!(m.f64(m.u32(g + srf::KNOTS) + 8), 0.5);
}

#[test]
fn the_chunks_of_a_node_hang_on_it() {
    si_rt::skip_without_install!();
    use super::chunks::{dyna, envelop, invkin, kluster, more};
    use crate::{Cluster, DynaMatter, Envelop, EnvelopVertex, InvKin};
    let vertex = |index: i32| EnvelopVertex { index, ratio: 0.5, ..EnvelopVertex::default() };
    let root = Node {
        name: b"joint".to_vec(),
        kind: 5,
        scaling: [2.0, 3.0, 4.0],
        dynamics: Some(DynaMatter { density: 0.75, collision: 3, ..DynaMatter::default() }),
        joint_friction: Some([1.0, 2.0, 3.0]),
        ik: Some(InvKin { kind: 30, nb_joints: 3, rotation: [0.0, 0.25, 0.0] }),
        envelop_type: 1,
        envelop: Some(Envelop { level: 2, lists: vec![vec![vertex(4)], vec![vertex(5), vertex(6)]], ..Envelop::default() }),
        clusters: vec![Cluster { name: b"left".to_vec(), count: 2, indices: vec![7, 9], state: 0 }],
        ..Node::default()
    };
    let (m, o, p) = read_model(&root);
    let d = m.u32(p + node::DYNA_MATTER);
    assert_eq!((m.f32(d + dyna::DENSITY), m.f32(d + dyna::RIGIDITY), m.i32(d + dyna::COLLISION), m.f32(d + dyna::MATRIX_B + 0x10)), (0.75, 1.0, 3, 1.0));
    assert_eq!(m.f32(m.u32(p + more::DYNA_JOINT) + 8), 3.0);
    // a chain of three joints: type 30 and two matrices of doubles, 2 x 3 and 3 x 2
    let ik = m.u32(p + node::INV_KIN);
    assert_eq!((m.i32(ik + invkin::TYPE), m.i32(ik + invkin::NB_JOINTS), m.f32(ik + invkin::ROTATION + 4), m.i16(ik + invkin::FLAG)), (30, 3, 0.25, 1));
    let (a, b) = (m.u32(ik + invkin::MATRIX_A), m.u32(ik + invkin::MATRIX_B));
    assert_eq!((m.i32(a), m.i32(a + 4), m.i32(b), m.i32(b + 4)), (2, 3, 3, 2));
    // the envelope: two lists linked both ways; the node got an initial transformation
    let e = m.u32(p + node::ENVELOPE);
    let first = m.u32(e + envelop::LISTS);
    let second = m.u32(first + envelop::LIST_NEXT);
    assert_eq!((m.i32(e + envelop::LEVEL), m.u32(second + envelop::LIST_PREVIOUS), m.u32(second + envelop::LIST_NEXT)), (2, first, 0));
    let v = m.u32(m.u32(second + envelop::LIST_FIRST) + envelop::VERT_NEXT);
    assert_eq!((m.i32(v + envelop::VERT_INDEX), m.f32(v + envelop::VERT_RATIO), m.f32(v + envelop::VERT_WEIGHT)), (6, 0.5, 100.0));
    assert_eq!(m.f32(m.u32(p + node::INITIAL_SRT) + 4), 3.0);
    let k = m.u32(p + more::CLUSTERS);
    assert_eq!((m.cstr(m.u32(k + kluster::NAME)), m.i32(k + kluster::COUNT), m.i32(m.u32(k + kluster::INDICES) + 4), m.u32(k + kluster::NEXT)), (b"left".to_vec(), 2, 9, 0));
    assert!(o.calls.iter().any(|c| c.0.name == "SI_edAddCluster"));
}

/// The outside world of the writer test: [`MemOutside`], and the chain dynamics' working
/// state (`SI_dynachnvarAllocate`, `settings.c`) allocated, zeroed, so that the node keeps
/// its "has chain dynamics" pointer.
struct Writing(MemOutside);

impl super::Outside for Writing {
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        self.0.fopen(m, path, mode)
    }
    fn fclose(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.0.fclose(m, file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.0.fread(m, ptr, size, n, file)
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.0.fgetc(m, file)
    }
    fn rewind(&mut self, m: &mut Mem, file: u32) {
        self.0.rewind(m, file)
    }
    fn bytes_left(&mut self, m: &mut Mem, file: u32) -> u64 {
        self.0.bytes_left(m, file)
    }
    fn fwrite(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.0.fwrite(m, ptr, size, n, file)
    }
    fn fputc(&mut self, m: &mut Mem, c: i32, file: u32) -> i32 {
        self.0.fputc(m, c, file)
    }
    fn ferror(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.0.ferror(m, file)
    }
    fn call(&mut self, m: &mut Mem, routine: &'static super::Routine, args: &[u32]) -> u32 {
        if routine.name == "SI_dynachnvarAllocate" {
            let p = m.si_malloc(0xbc);
            for k in 0..0xbc {
                m.set_u8(p + k, 0);
            }
            return p;
        }
        self.0.call(m, routine, args)
    }
}

/// The writers on the image against the value form's writer, on a model that uses every
/// prototype and token of the 4.0 table (`crate::tests::everything`): the value form writes
/// it, the image form reads it and writes it again, and the bytes must be the same. This
/// reaches the paths of the writers the files the original saved do not (NURBS curves,
/// faces of NURBS curves, composite curves, metaballs, the user data of components and the
/// texture sets): there it holds two ports against each other, not against the original.
/// Left out of the model what the reader changes or the outside world would make: the
/// mesh's edges (rebuilt by `SI_meshUpdateNodes`, not on the image), a 3D texture's
/// channel other than 2 (a fresh image's `b_matio.c` version is 0).
#[test]
fn the_writers_write_back_what_the_readers_read() {
    si_rt::skip_without_install!();
    let mut root = crate::tests::everything();
    if let Some(Definition::Mesh(mesh)) = root.definitions.first_mut() {
        mesh.nedges = 0;
        mesh.edges.clear();
    }
    if let Some(x) = &mut root.texture3d {
        x.channel = 2;
    }
    // a face's splines have no name in the program (SI_splineTransfer copies none)
    let mut son = root.son.as_deref_mut();
    while let Some(n) = son {
        if let Some(Definition::Face(f)) = n.definitions.first_mut() {
            f.exterior.name.clear();
            for h in &mut f.holes {
                h.name.clear();
            }
        }
        son = n.brother.as_deref_mut();
    }
    for x in root.materials.iter_mut().flatten() {
        if let Some(t) = &mut x.texture3d {
            t.channel = 2;
        }
    }
    let bytes = crate::write_model(&root);
    let (mut m, o) = image();
    let mut o = Writing(o);
    let file = o.0.files.open_bytes(b"/db/m.hrc", bytes.clone());
    let p = reader::fdmodel_binary_read(&mut m, &mut o, file, 0, 0);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    assert_ne!(p, 0);
    let out = o.0.files.open_for_writing(b"/db/out.hrc");
    assert_eq!(super::modelwrite::fdmodel_binary_write(&mut m, &mut o, out, p, 0), 1);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    let written = &o.0.files.stream(out).unwrap().data;
    let at = written.iter().zip(&bytes).position(|(a, b)| a != b);
    if let Ok(dir) = std::env::var("HRC_TEST_DUMP") {
        std::fs::write(format!("{dir}/value.hrc"), &bytes).unwrap();
        std::fs::write(format!("{dir}/image.hrc"), written).unwrap();
    }
    assert_eq!((at, written.len()), (None, bytes.len()), "first difference at {at:?}");
    // the same through the file writer, which opens and closes the file
    let name = m.place_cstr(b"/db/again");
    assert_eq!(super::modelwrite::omodel_binary_write(&mut m, &mut o, name, p, 0), 1);
    assert_eq!(o.0.files.file(b"/db/again.hrc"), Some(&bytes));
}

/// A material and a 2D and a 3D texture file the same way, through the file writers with
/// `SI_versionOutput`.
#[test]
fn material_and_texture_files_write_back() {
    si_rt::skip_without_install!();
    let material = Material {
        name: b"brass".to_vec(),
        shading: 7,
        xdc8: 2,
        xdca: 2,
        diffuse: [0.25, 0.5, 0.75],
        udf_material: Some(Udf { name: b"marble".to_vec(), active: 1, internal: 0, order: 3 }),
        udf_extra: Some(Udf { name: b"fourth".to_vec(), active: 0, internal: 1, order: 2 }),
        user_data: vec![UserNode { tag: b"_tag".to_vec(), flag: 1, size: 3, data: vec![7, 8, 9] }, UserNode { tag: b"CUSELEMCB".to_vec(), flag: 0, size: 1, data: vec![1] }],
        ..Material::default()
    };
    let texture = Texture2D { name: b"pic".to_vec(), filename: b"wood".to_vec(), crop: [1, 2, 3, 4, 0], mask: 4, method: 3, user_data: vec![UserNode { tag: b"_t".to_vec(), flag: 0, size: 3, data: vec![1, 2, 3] }], ..Texture2D::default() };
    let texture3d = Texture3D { name: b"clouds".to_vec(), channel: 1, udf: Some(Udf { name: b"noise".to_vec(), active: 1, internal: 0, order: 5 }), ..Texture3D::default() };
    let cases: [(&[u8], Vec<u8>); 3] = [(b".mtr", crate::write_material(&material, 4.0)), (b".txt", crate::write_texture(&texture, 4.0)), (b".t3D", crate::write_texture3d(&texture3d, 4.0))];
    for (suffix, bytes) in cases {
        let (mut m, mut o) = image();
        o.files.add(&[&b"/db/in"[..], suffix].concat(), bytes.clone());
        let input = m.place_cstr(b"/db/in");
        let output = m.place_cstr(b"/db/out");
        let version = m[super::globals::SI_VERSION_OUTPUT];
        let ok = match suffix {
            b".mtr" => {
                let p = matio::material_read_binary_file(&mut m, &mut o, input).addr();
                super::matwrite::material_write_binary_file(&mut m, &mut o, output, version, p)
            }
            b".txt" => {
                let p = matio::texture_read_binary_file(&mut m, &mut o, input).addr();
                super::matwrite::texture_write_binary_file(&mut m, &mut o, output, version, p)
            }
            _ => {
                let p = matio::texture3d_read_binary_file(&mut m, &mut o, input).addr();
                super::matwrite::texture3d_write_binary_file(&mut m, &mut o, output, version, p)
            }
        };
        assert_eq!(ok, 1);
        assert!(m.fault().is_none(), "{:?}", m.fault());
        let written = o.files.file(&[&b"/db/out"[..], suffix].concat()).unwrap();
        let at = written.iter().zip(&bytes).position(|(a, b)| a != b);
        assert_eq!((at, written.len()), (None, bytes.len()), "{} first difference at {at:?}", String::from_utf8_lossy(suffix));
    }
}

#[test]
fn a_shader_s_name_is_replaced_by_a_copy() {
    si_rt::skip_without_install!();
    use super::udf::{udf_set_name, Udf};
    let (mut m, _) = image();
    let udf = si_mem::Ptr32::<Udf>::new(m.calloc(1, 0x3c));
    let name = m.malloc(6);
    m.put_cstr(name, b"phong");
    // no name yet: nothing is freed
    assert_eq!(udf_set_name(&mut m, udf, name), 1);
    let first = m[udf].name.addr();
    assert_ne!(first, name);
    assert_eq!((m.cstr(first), m[udf].x0e), (b"phong".to_vec(), 1));
    m[udf].x0e = 0;
    assert_eq!(udf_set_name(&mut m, udf, name), 1);
    assert!(m.is_freed(first));
    assert_ne!(m[udf].name.addr(), first);
    assert_eq!(m[udf].x0e, 1);
}
