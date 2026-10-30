// SPDX-License-Identifier: MIT
//! Made-up cases for the converter's helpers and writers.

use super::*;
use si_elemio::anim::{HierAnim, ItemData};
use si_elemio::fcurve::{Fcurve, Key, StaticVal};
use std::collections::BTreeMap;

struct Files(BTreeMap<Vec<u8>, Vec<u8>>);

impl Host for Files {
    fn read(&self, path: &[u8]) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

fn env() -> Env {
    Env { curves: CurveNames::new(&crate::srel::Relations::new()), now: 0, default_interpolation: 0 }
}

#[test]
fn names_and_suffixes() {
    assert_eq!(get_basic_name(b"/in/db/d0001.ani"), Some(b"/in/db/d0001".to_vec()));
    assert_eq!(get_basic_name(b"a.b/c"), Some(b"a".to_vec()));
    assert_eq!(get_basic_name(b"noext"), Some(b"noext".to_vec()));
    assert_eq!(get_basic_name(b".hidden"), None);
    assert_eq!(get_fname_suffix(b"x.t3D"), Some(b"t3D".to_vec()));
    assert_eq!(get_fname_suffix(b"x."), None);
    assert_eq!(get_fname_suffix(b"plain"), Some(b"plain".to_vec()));
    assert_eq!(get_fname_suffix(b""), None);
}

#[test]
fn magic_of_binary_and_text() {
    let mut b = vec![0x53, 0x80, 0xf6, 0x40];
    b.extend_from_slice(&3.0905f32.to_be_bytes());
    assert_eq!(db_file_read_magic(&b), (MAGIC_MATERIAL, false, 3.0905, 8));
    let (m, text, v, _) = db_file_read_magic(b"SMAT3.5\n");
    assert_eq!((m, text, v), (TAG_SMAT, true, 3.5));
    assert_eq!(db_file_read_magic(b"abcd").0, -1);
}

#[test]
fn a_curve_item_and_a_static_item() {
    let mut ha = HierAnim::new();
    ha.add_elem(b"cube", -1);
    let mut c = Fcurve::blank(2);
    c.curve_type = 145;
    c.count = 1;
    c.keys.push(Key { time: 1.5, value: 2.0, left_slope: 0.25, right_slope: 0.25, flag: 1, interpolation: 2, ..Key::new() });
    ha.add_item(0, 0, -1, 0, 1, ItemData::Curve(c));
    ha.add_item(0, 1, 3, 2, 1, ItemData::Static(StaticVal { curve_type: 7, value: 0.5 }));
    let (text, r) = elemdict_write_asc_animations(&ha, 8, TAG_SANI, &mut env().curves);
    assert_eq!(r, 1);
    let want = "SANI4\n\n   HIER_CHAPNAME 8 \n   HIER_NAME 'cube' 0 \n   HIER_FTH -1 \n   HIER_END \n   \n   \
OWNER_ID 0 ITM_TYPE 0 ANIM_END \n      CURVE_TYPE notfound 145 \n      PRE_EXTRAPOLATION 0     POST_EXTRAPOLATION 0 \
\n      FLAGS 0 \n      INTERPOLATION SPLINE \n      KEYS 1 \n         \n         TIME 1.5     VALUE 2 \
\n         LEFT_SLOPE 0.25     RIGHT_SLOPE 0.25 \n         INTERPOLATION SPLINE     FLAG 1 \n         KEYTYPE FREEFORM \
\n      \n   OWNER_ID 0 ITM_TYPE 1 ACT_RELID 3 ACT_OCCID 2 ANIM_END TYPE 7     VALUE 0.5     SVAL_END \n   ";
    assert_eq!(String::from_utf8_lossy(&text), want);
}

#[test]
fn a_camera_from_a_file() {
    // a camera file with only the near distance: the rest are SI_newCamera's defaults
    let mut f = Vec::new();
    si_file::write_bin_header(&mut f, 0x5380_f636, 3.5, None);
    f.extend_from_slice(&[0, 7, 0, 1]);
    f.extend_from_slice(&0.25f32.to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    let host = Files([(b"/d/c.cam".to_vec(), f)].into_iter().collect());
    let Outcome::Done(run) = main(&host, &mut env(), b"/d/c.cam", b"/o", true) else { panic!() };
    assert_eq!(run.status, 1);
    let text = String::from_utf8(run.output.unwrap()).unwrap();
    assert!(text.starts_with("SCAM3.5\n\n   POSITION STATIC \n      0 0 100 \n   INTEREST STATIC \n      0 0 0 \n"), "{text}");
    assert!(text.contains("\n   NEAR 0.25 \n"));
    assert!(text.ends_with("\n   ENLARGE INACTIVE "));
}

#[test]
fn a_texture_named_t3d_is_not_found() {
    let mut f = Vec::new();
    si_file::write_bin_header(&mut f, 0x5380_f644, 4.0, None);
    let host = Files([(b"/d/x.t3d".to_vec(), f)].into_iter().collect());
    let Outcome::Done(run) = main(&host, &mut env(), b"/d/x.t3d", b"/o", false) else { panic!() };
    assert_eq!((run.status, run.output, run.stdout), (255, None, b"Problem reading input file /d/x.t3d\n".to_vec()));
}

#[test]
fn the_light_reader_runs_on_from_its_rotation_matrix() {
    // a light as the converter writes it (EXPONENT left out: the rest is plain text)
    let mut l = si_elemio::light::Light::new(2, 0);
    l.set_name(b"key");
    l.diffuse = 1;
    l.energy = [5.0, 6.0, 7.0];
    let (text, _) = data_write_ascii_file(Data::Light(&l), TAG_SLIG, 4.0, 0);
    let host = Files([(b"/t/l.lig".to_vec(), text)].into_iter().collect());
    let Outcome::Done(run) = main(&host, &mut env(), b"/t/l.lig", b"/o/x", false) else { panic!() };
    assert_eq!((run.status, run.output_name.as_slice()), (1, &b"/o/x.lig"[..]));
    // read back with si-elemio: the fields before the matrix survive, the matrix holds every
    // other value and then zeros for the keywords it ran into, and what follows is lost
    let back = run.output.unwrap();
    let f = si_elemio::light::read_file_converter(&mut si_elemio::io::Reader::new(&back), 0);
    let got = f.read.light.unwrap();
    assert_eq!((got.name_bytes(), got.kind, got.cone_angle), (&b"key"[..], 2, l.cone_angle));
    // the identity read at every other place: 1, 0, 0, 0, 0, 1, 0, 0
    assert_eq!(&got.area_rotation_mat[..8], &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    // m[8] = DIFFUSE (0), m[9] = ENERGY (0), m[10] = 6, m[11] = EXPONENT (0)
    assert_eq!(&got.area_rotation_mat[8..12], &[0.0, 0.0, 6.0, 0.0]);
    assert_eq!((got.diffuse, got.energy, got.exponent), (0, [10000.0; 3], 2.0));
}

#[test]
fn an_angle_curve_comes_back_in_degrees() {
    let mut ha = HierAnim::new();
    ha.chapter = Some(b"MODELS".to_vec());
    ha.add_elem(b"cube", -1);
    let mut c = Fcurve::blank(1);
    c.curve_type = 0x19; // an angle
    c.count = 1;
    c.keys.push(Key { time: 0.0, value: 1.0, interpolation: 1, ..Key::new() });
    ha.add_item(0, 0, -1, 0, 1, ItemData::Curve(c));
    let (text, _) = elemdict_write_asc_animations(&ha, 8, TAG_SANI, &mut env().curves);
    let host = Files([(b"/t/a.ani".to_vec(), text)].into_iter().collect());
    let Outcome::Done(run) = main(&host, &mut env(), b"/t/a.ani", b"/o/a", false) else { panic!() };
    let back = run.output.unwrap();
    let f = si_elemio::anim::read_file(&mut si_elemio::io::Reader::new(&back), 0);
    let ItemData::Curve(got) = &f.anim.elems[0].items[0].data else { panic!() };
    assert_eq!(got.keys[0].value, 57.29578);
    assert_eq!(f.anim.chapter.as_deref(), Some(&b"MODELS"[..]));
}

#[test]
fn texture3d_text_reads_back_as_written() {
    use crate::setup::{Reader as TextReader, Session, Setup};
    let mut x = si_hrc::Texture3D::default();
    x.name = b"marble".to_vec();
    x.kind = 2;
    x.iterations = 6;
    x.strength = 3.25;
    x.colours[1] = [0.1, 0.2, 0.3, 0.059];
    x.factors = [0.2426778, 0.3, 0.4];
    x.rotation = [0.0, 0.0, 0.7853982];
    x.reflection = 0.5;
    x.mask = 1;
    x.channel = 2;
    let (text, r) = super::data_write_ascii_file(super::Data::Texture3D(&x, Some(&x.name)), super::TAG_ST3D, 4.0, 0);
    assert_eq!(r, 1);
    let t = String::from_utf8(text.clone()).unwrap();
    assert!(t.starts_with("ST3D4\n\n   TEXTURE_3D_NAME 'marble' \n   TYPE 2 \n   STRENGTH 3.25 \n   NB_ITER 6 \n"));
    assert!(t.contains("\n   COLOUR_2 0.1 0.2 0.3 \n   ALPHA_2 0.059 \n"));
    assert!(t.ends_with("\n   USE_ALPHA 1 \n   USE_EFFECT 2 "));
    // and back: the header line is skipped as main skips it
    let mut f = crate::token::AsciiFile::new(&text);
    let nl = text.iter().position(|&b| b == b'\n').unwrap();
    f.seek(nl + 1);
    let session = Session::default();
    let mut rd = TextReader::new(&mut f, &session);
    let mut y = si_hrc::Texture3D::default();
    assert_eq!(super::read::texture3d_ascii_read(&mut rd, &Setup::default(), &mut y), Ok(1));
    assert_eq!(y, x);
    // a null name prints as glibc prints it
    let (text, _) = super::data_write_ascii_file(super::Data::Texture3D(&x, None), super::TAG_ST3D, 4.0, 0);
    assert!(String::from_utf8(text).unwrap().contains("TEXTURE_3D_NAME '(null)' "));
}

#[test]
fn texture3d_reader_takes_strength_spacing_power_through_one_local() {
    use crate::setup::{Reader as TextReader, Session, Setup};
    let session = Session::default();
    let run = |text: &'static [u8]| {
        let mut f = crate::token::AsciiFile::new(text);
        let mut rd = TextReader::new(&mut f, &session);
        let mut y = si_hrc::Texture3D::default();
        let r = super::read::texture3d_ascii_read(&mut rd, &Setup::default(), &mut y);
        (r, y)
    };
    // a curve in place of POWER's value: POWER gets what STRENGTH left in the local
    let (r, y) = run(b"STRENGTH 2.5 POWER CURVE_TYPE x KEYS 0 TAPER 1 END");
    assert_eq!(r, Ok(1));
    assert_eq!((y.strength, y.power, y.taper), (2.5, 2.5, 1.0));
    // before any number: the original copies uninitialised stack
    let (r, _) = run(b"POWER CURVE_TYPE x KEYS 0 END");
    assert_eq!(r, Err(super::read::TextureStop::UninitialisedLocal));
    // REFRACTION and STRETCH are read and dropped; the first unknown word ends the texture
    let (r, y) = run(b"REFRACTION 9 STRETCH 8 USE_EFFECT 3 BOGUS");
    assert_eq!((r, y.channel), (Ok(1), 3));
}

#[test]
fn texture3d_version_2_1_is_old() {
    // the single 2.1 is below the double 2.1: the channel is forced to 2
    let mut x = si_hrc::Texture3D::default();
    super::texture3d_old_channel(&mut x, 2.1f32);
    assert_eq!(x.channel, 2);
    let mut x = si_hrc::Texture3D::default();
    super::texture3d_old_channel(&mut x, 2.11f32);
    assert_eq!(x.channel, 1);
}
