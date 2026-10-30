// SPDX-License-Identifier: MIT
//! Unit tests of the image form, on made-up data only. What holds the ports against the
//! original is `elemio-emucheck`; these show the pieces working together and pin a few of
//! the odd cases.

use super::host::Files;
use super::outside::{self as out, Outside, Routine};
use super::*;
use si_mem::Mem;

/// A world of the simplest kind: the streams, a shader record of cleared bytes, the
/// lattices handed over, and a note of every other call.
struct World {
    files: Files,
    lattices: Vec<u32>,
    calls: Vec<&'static str>,
}

impl World {
    fn new() -> World {
        World { files: Files::new(), lattices: Vec::new(), calls: Vec::new() }
    }
}

impl Outside for World {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        if let Some(v) = self.files.call(m, routine, args) {
            return v;
        }
        self.calls.push(routine.name);
        if routine.addr == out::SI_UDF_ALLOCATE.addr {
            return m.si_calloc(1, 0x3c);
        }
        if routine.addr == out::SI_MODEL_ADD_I_LATTICE.addr {
            self.lattices.push(args[3]);
        }
        if routine.addr == out::TIME.addr && args[0] != 0 {
            m.set_u32(args[0], 1_000_000_000);
            return 1_000_000_000;
        }
        0
    }

    fn file_left(&mut self, _m: &mut Mem, file: u32) -> u32 {
        self.files.left(file)
    }
}

fn image() -> Mem {
    let mut m = Mem::new();
    globals::init(&mut m);
    m
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn value(v: f32) -> Vec<u8> {
    [&be16(1)[..], &v.to_be_bytes()].concat()
}

fn value3(v: [f32; 3]) -> Vec<u8> {
    let mut o = be16(1).to_vec();
    for x in v {
        o.extend_from_slice(&x.to_be_bytes());
    }
    o
}

fn header(magic: u32, version: f32, comment: &[u8]) -> Vec<u8> {
    let mut o = magic.to_be_bytes().to_vec();
    o.extend_from_slice(&version.to_be_bytes());
    let mut c = comment.to_vec();
    c.resize(80, 0);
    o.extend_from_slice(&c);
    o
}

/// A spot light's stream: name, type, colour, position, cone angle.
fn spot() -> Vec<u8> {
    let mut s = be16(1).to_vec();
    s.extend_from_slice(b"key\0");
    s.extend_from_slice(&be16(2));
    s.extend_from_slice(&be16(2));
    s.extend_from_slice(&be16(3));
    s.extend_from_slice(&value3([1.0, 0.5, 0.25]));
    s.extend_from_slice(&be16(7));
    s.extend_from_slice(&value3([10.0, 20.0, 30.0]));
    s.extend_from_slice(&be16(9));
    s.extend_from_slice(&value(0.75));
    s
}

#[test]
fn a_light_file_builds_the_light_record() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let mut data = spot();
    data.extend_from_slice(&be16(0));
    let f = w.files.open(data);
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(bsceneio::light_binary_read(&mut m, &mut w, f, place), Ok(1));
    let l = m.u32(place);
    assert_eq!(m.block_size(l), Some(off::light::SIZE));
    assert_eq!(m.cstr(l), b"key");
    assert_eq!(m.i16(l + off::light::TYPE), 2);
    assert_eq!([m.f32(l + 0x24), m.f32(l + 0x28), m.f32(l + 0x2c)], [1.0, 0.5, 0.25]);
    assert_eq!([m.f32(l + 0x34), m.f32(l + 0x38), m.f32(l + 0x3c)], [10.0, 20.0, 30.0]);
    assert_eq!(m.f32(l + off::light::CONE_ANGLE), 0.75);
    // what the file does not state is the new light's default
    assert_eq!(m.u32(l + off::light::SPREAD_ANGLE), lights::SPREAD_BITS);
    assert_eq!(m.i32(l + off::light::SHADOW_MAP_RES), 500);
    assert_eq!(m.f32(l + off::light::AREA_ROTATION_MAT), 1.0);
    // the light got its graph node, was looked up there, and the name's block is freed
    assert_eq!(w.calls, ["SI_graflistAddData", "SI_findGlData"]);
    assert_eq!(m.freed().len(), 1);
    assert!(m.fault().is_none());
    assert_eq!(m[globals::TOKEN_BSCENEIO], 0);
}

#[test]
fn a_file_that_ends_early_stops_at_the_token_read_that_fails() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let f = w.files.open(spot());
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(bsceneio::light_binary_read(&mut m, &mut w, f, place), Err(NoReturn::Endless("SI_lightBinaryRead")));
    // everything up to there is in the image: the light, with its last field, and the token
    // the original would meet again for ever
    let l = m.u32(place);
    assert_eq!(m.f32(l + off::light::CONE_ANGLE), 0.75);
    assert_eq!(m[globals::TOKEN_BSCENEIO], 9);
    // an empty file after a complete one returns at once: the token is still 0
    let (mut m, mut w) = (image(), World::new());
    let f = w.files.open(Vec::new());
    assert_eq!(bsceneio::light_binary_read(&mut m, &mut w, f, place), Ok(1));
}

#[test]
fn a_token_in_front_of_the_type_token_stores_nothing_and_an_unguarded_one_is_a_fault() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let mut data = be16(9).to_vec();
    data.extend_from_slice(&value(0.5));
    data.extend_from_slice(&be16(0));
    let f = w.files.open(data);
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(bsceneio::light_binary_read(&mut m, &mut w, f, place), Ok(1));
    assert_eq!(m.u32(place), 0);
    assert!(m.heap_blocks().is_empty());
    // token 27 (a sun's daylight saving time) stores through the light without a test
    let mut data = be16(27).to_vec();
    data.extend_from_slice(&be16(1));
    data.extend_from_slice(&be16(0));
    data.extend_from_slice(&be16(0));
    let f = w.files.open(data);
    assert!(bsceneio::light_binary_read(&mut m, &mut w, f, place).is_ok());
    assert!(m.fault().is_some());
}

#[test]
fn a_camera_file_is_read_into_the_scene_camera() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let name = m.place_cstr(b"Camera");
    let cam = camfct::new_camera(&mut m, &mut w, name, 1);
    assert_eq!(m.cstr(m.u32(cam)), b"Camera");
    assert_eq!(m.f32(cam + off::camera::FOCAL_LENGTH), 50.0);
    assert_eq!(m.f32(cam + off::camera::PLANE_DISTANCE), 50.0);
    assert!((m.f32(cam + off::camera::PLANE_HEIGHT) - 41.421356).abs() < 1e-4);
    let proc_data = m[globals::SI_PROC_DATA];
    m.set_u32(proc_data + 8, cam);
    let mut data = header(0x5380_f636, 3.71, b"");
    data.extend_from_slice(&be16(7));
    data.extend_from_slice(&value(0.5));
    data.extend_from_slice(&be16(0x1614));
    data.extend_from_slice(&be16(1));
    for v in [1.0f32, 2.0, 3.0, 4.0, 5.0] {
        data.extend_from_slice(&value(v));
    }
    data.extend_from_slice(&be16(0));
    w.files.add_named(b"/db/c.cam", data);
    let path = m.place_cstr(b"/db/c.cam");
    assert_eq!(elemdict::elemdict_read_camera(&mut m, &mut w, path), Ok(cam));
    assert_eq!(m.f32(cam + off::camera::NEAR), 0.5);
    assert_eq!(m.f32(cam + off::camera::FAR), 32768.0);
    assert_eq!(m.i16(cam + off::camera::PLANE_ACTIVE), 1);
    assert_eq!(m.f32(cam + off::camera::PLANE_OFFSET_Y), 5.0);
    assert_eq!(f32::from_bits(m[globals::SI_VERSION_INPUT]), 3.71);
    assert_eq!(f32::from_bits(m[globals::IN_VERSION]), 3.71);
    // a file that is not there
    let path = m.place_cstr(b"/db/none.cam");
    assert_eq!(elemdict::elemdict_read_camera(&mut m, &mut w, path), Ok(0));
    assert!(m.fault().is_none());
}

#[test]
fn a_lattice_gets_the_headers_bytes_for_what_its_file_omits() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let comment: Vec<u8> = (0..80u8).map(|k| 0x80 + k).collect();
    let mut data = header(0x5380_f638, 3.0, &comment);
    data.extend_from_slice(&be16(2));
    for n in [1u16, 1, 2] {
        data.extend_from_slice(&be16(n));
    }
    data.extend_from_slice(&be16(4));
    for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
        data.extend_from_slice(&v.to_be_bytes());
    }
    data.extend_from_slice(&be16(0));
    w.files.add_named(b"/db/l.lat", data);
    let path = m.place_cstr(b"/db/l.lat");
    assert_eq!(elemdict::elemdict_read_lattice(&mut m, &mut w, path, 1, 0x6100_0100), Ok(1));
    let l = w.lattices[0];
    assert_eq!([m.i16(l + 2), m.i16(l + 4), m.i16(l + 6)], [1, 1, 2]);
    let keys = m.u32(l + off::lattice::KEYS);
    assert_eq!(m.f32(keys + 12), 4.0);
    // the second and third interpolation code: the header's magic number and version
    assert_eq!(m.u32(l + 0x0c), 0x5380_f638);
    assert_eq!(m.f32(l + 0x10), 3.0);
    // minimum, maximum and scaling: the comment's bytes 8 to 43
    assert_eq!(m.bytes(l + off::lattice::MINIMUM, 36), &comment[8..44]);
    assert_eq!(m[globals::GLB_BRANCH], 1);
    assert_eq!(w.calls, ["SI_modelAddILattice", "SI_edAddLattice"]);
}

/// An animation file: one element, one curve of two keys, an empty first action list.
fn animation() -> Vec<u8> {
    let mut d = header(0x5380_f635, 3.71, b"");
    d.extend_from_slice(&be16(3));
    d.extend_from_slice(b"MODELS\0");
    d.extend_from_slice(&be16(1));
    d.extend_from_slice(b"cube\0");
    d.extend_from_slice(&be16(2));
    d.extend_from_slice(&be16(0xffff));
    d.extend_from_slice(&be16(0));
    // the item's header
    for t in [1u16, 0, 4, 0, 7, 1, 0] {
        d.extend_from_slice(&be16(t));
    }
    // the curve: type 12, two keys
    d.extend_from_slice(&be16(1));
    d.extend_from_slice(&be16(12));
    d.extend_from_slice(&be16(8));
    d.extend_from_slice(&2u32.to_be_bytes());
    d.extend_from_slice(&be16(7));
    for (time, v) in [(0.0f32, 1.5f32), (1.0, 2.5)] {
        d.extend_from_slice(&be16(1));
        d.extend_from_slice(&time.to_be_bytes());
        d.extend_from_slice(&be16(2));
        d.extend_from_slice(&v.to_be_bytes());
        d.extend_from_slice(&be16(0));
    }
    d.extend_from_slice(&be16(0));
    d
}

#[test]
fn an_animation_file_builds_the_hierarchy_animation() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    m[globals::SI_GLB_ANIM_30] = 2;
    let mut data = animation();
    data.extend_from_slice(&be16(10));
    data.extend_from_slice(&be16(0x0b));
    w.files.add_named(b"/db/a.ani", data);
    let path = m.place_cstr(b"/db/a.ani");
    let a = anim::elemdict_read_animations(&mut m, &mut w, 0, path).unwrap();
    assert_eq!(m.u32(a + off::hier_anim::NB_ELEMS), 1);
    let e = m.u32(a + off::hier_anim::ELEMS);
    assert_eq!(m.cstr(m.u32(e)), b"cube");
    assert_eq!(m.i32(e + off::hier_elem::FATHER), -1);
    assert_eq!(m.u32(e + off::hier_elem::NB_ITEMS), 1);
    assert_eq!(m.i32(m.u32(e + off::hier_elem::REL_IDS)), -1);
    let curve = m.u32(m.u32(e + off::hier_elem::DATA));
    assert_eq!(m.i32(curve + off::fcurve::TYPE), 12);
    assert_eq!(m.i32(curve + off::fcurve::NB_KEYS), 2);
    assert_eq!(m.i16(curve + off::fcurve::INTERPOLATION), 2);
    let keys = m.u32(curve + off::fcurve::KEYS);
    // the key array is allocated for a multiple of eight keys
    assert_eq!(m.block_size(keys), Some(8 * off::fkey::SIZE));
    assert_eq!(m.f32(keys + off::fkey::SIZE + off::fkey::VALUE), 2.5);
    assert_eq!(m.u32(keys + off::fkey::SIZE + off::fkey::CURVE), curve);
    assert_eq!(m.i16(keys + off::fkey::SPLINE_INDEX), -1);
    assert_eq!(m.u32(a + off::hier_anim::ACTIONS), 0);
    assert_eq!(w.calls, ["SI_elemdictFindListByName"]);
    assert!(m.fault().is_none());
    // the stream was closed
    assert!(w.files.stream(si_mem::Kind::File.nth(3)).unwrap().closed);
}

#[test]
fn a_file_that_ends_between_two_keys_ends_its_curve_and_one_that_ends_inside_a_key_does_not() {
    si_rt::skip_without_install!();
    // the count says two keys, the file holds one: the second stays blank and the curve
    // ends, because the key's closing zero is the token the curve's loop sees
    let (mut m, mut w) = (image(), World::new());
    let mut d = be16(8).to_vec();
    d.extend_from_slice(&2u32.to_be_bytes());
    d.extend_from_slice(&be16(7));
    d.extend_from_slice(&be16(2));
    d.extend_from_slice(&7.0f32.to_be_bytes());
    d.extend_from_slice(&be16(0));
    let whole = d.clone();
    let f = w.files.open(d);
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(fcurve::fcurve_binary_read(&mut m, &mut w, f, place, 0), Ok(1));
    let keys = m.u32(m.u32(place) + off::fcurve::KEYS);
    assert_eq!(m.f32(keys + off::fkey::VALUE), 7.0);
    assert_eq!(m.f32(keys + off::fkey::SIZE + off::fkey::VALUE), 0.0);
    assert_eq!(m.i16(keys + off::fkey::SIZE + off::fkey::SPLINE_INDEX), -1);
    // cut inside the key
    let (mut m, mut w) = (image(), World::new());
    let f = w.files.open(whole[..whole.len() - 2].to_vec());
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(fcurve::fcurve_binary_read(&mut m, &mut w, f, place, 0), Err(NoReturn::Endless("fcvkeyBinaryRead")));
}

#[test]
fn an_action_list_is_read_in_the_order_of_time() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    m[globals::SI_FRAME_RATE] = 25.0f32.to_bits();
    let a = anim::hieranim_allocate(&mut m, 0);
    let mut d = Vec::new();
    for (name, start, end) in [(&b"late"[..], 5.02f32, 5.98f32), (b"early", 1.02, 1.98)] {
        d.extend_from_slice(&be16(0x0c));
        d.extend_from_slice(name);
        d.push(0);
        d.extend_from_slice(&be16(0x12));
        d.extend_from_slice(&start.to_be_bytes());
        d.extend_from_slice(&be16(0x13));
        d.extend_from_slice(&end.to_be_bytes());
        d.extend_from_slice(&be16(0x11));
    }
    d.extend_from_slice(&be16(0x0b));
    let f = w.files.open(d);
    assert_eq!(anim::actions_binary_read(&mut m, &mut w, a, f, 0), Ok(()));
    let list = a + off::hier_anim::ACTIONS;
    assert_eq!(m.u32(list), 2);
    let e = m.u32(list + 4);
    assert_eq!(m.cstr(m.u32(e)), b"early");
    assert_eq!(m.cstr(m.u32(e + 0x30)), b"late");
    // half a frame (0.02 s at 25 frames a second) outwards
    assert!((m.f32(e + off::actions_elem::START) - 1.0).abs() < 1e-6);
    assert!((m.f32(e + off::actions_elem::END) - 2.0).abs() < 1e-6);
    assert_eq!(m.i16(e + off::actions_elem::INTERPOLATION), 5);
    assert_eq!(m.u32(e + off::actions_elem::FLAGS), 10);
    assert!(m.fault().is_none());
}

#[test]
fn a_text_of_256_characters_stops_the_reader() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let mut d = vec![b'x'; 255];
    d.push(0);
    let f = w.files.open(d);
    let s = utilio::string_binary_read(&mut m, &mut w, f).unwrap();
    assert_eq!(m.strlen(s), 255);
    let mut d = vec![b'x'; 256];
    d.push(0);
    let f = w.files.open(d);
    assert_eq!(utilio::string_binary_read(&mut m, &mut w, f), Err(NoReturn::Stack("SI_stringBinaryRead")));
    // the end of the file ends a text as a zero byte does
    let f = w.files.open(b"abc".to_vec());
    let s = utilio::string_binary_read(&mut m, &mut w, f).unwrap();
    assert_eq!(m.cstr(s), b"abc");
}

#[test]
fn the_streams_behave_as_the_c_librarys() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    let f = w.files.open(vec![1, 2, 3, 4, 5]);
    assert_eq!(f, 0x6f00_0300);
    let buffer = m.place(8, 4);
    // two whole items of two bytes; the fifth byte arrives too and is no item
    assert_eq!(byteswap::byteswap_fread(&mut m, &mut w, buffer, 2, 3, f), 2);
    assert_eq!(m.bytes(buffer, 5), [2, 1, 4, 3, 5]);
    assert!(w.files.stream(f).unwrap().eof);
    assert_eq!(w.file_left(&mut m, f), 0);
    assert_eq!(out::fgetc(&mut w, &mut m, f), -1);
}

#[test]
fn a_count_beyond_the_file_is_not_allocated_for() {
    si_rt::skip_without_install!();
    let (mut m, mut w) = (image(), World::new());
    // a curve that says it has a million keys in a file of a few bytes
    let mut d = be16(8).to_vec();
    d.extend_from_slice(&1_000_000u32.to_be_bytes());
    d.extend_from_slice(&be16(7));
    d.extend_from_slice(&be16(0));
    let f = w.files.open(d);
    let place = m.place(4, 4);
    m.set_u32(place, 0);
    assert_eq!(fcurve::fcurve_binary_read(&mut m, &mut w, f, place, 0), Err(NoReturn::Count("SI_fcurveBinaryRead")));
}
