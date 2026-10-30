// SPDX-License-Identifier: MIT
//! The text form of the binary element files, as the vendor's converter `bin2ascii` writes
//! it (finding 0006, section 9).
//!
//! `bin2ascii` is a sibling of `soft` (finding 0001): its `main` reads a material, texture,
//! lattice, camera, light, animation or struct key file with `soft`'s binary readers and
//! prints it with writers of its own (`SI_materialAsciiWrite`, …), which `soft` does not
//! have; or, given such a text, reads it with its own text readers and writes the binary
//! file with `soft`'s writers. Routines of `bin2ascii` that are not in `soft` are named
//! `name (bin2ascii 0xADDRESS)`; routines it shares with `soft` are cited at their `soft`
//! address. The binary readers are those of `si-hrc` (materials, textures) and `si-elemio`
//! (the rest), in their value form.
//!
//! - [`write`]: the writers, one per kind of element.
//! - [`curves`]: the curve type names the animation text prints, from the relations
//!   resources.
//! - [`main`]: what `bin2ascii <in> <out> [-k]` does, on files given by a [`Host`].

pub mod curves;
pub mod read;
pub mod write;

use crate::setup::Out;
pub use curves::CurveNames;
use si_elemio::io::Reader;

pub use write::VERSION_OUTPUT;

/// The general magic number: a file of that magic takes its kind from its name's suffix.
pub const MAGIC_GENERAL: i32 = 0x5380_f634;
pub const MAGIC_ANIMATION: i32 = 0x5380_f635;
pub const MAGIC_CAMERA: i32 = 0x5380_f636;
pub const MAGIC_LATTICE: i32 = 0x5380_f638;
pub const MAGIC_LIGHT: i32 = 0x5380_f639;
pub const MAGIC_MATERIAL: i32 = 0x5380_f640;
pub const MAGIC_TEXTURE_2D: i32 = 0x5380_f643;
pub const MAGIC_TEXTURE_3D: i32 = 0x5380_f644;
pub const MAGIC_STRUCT_KEYS: i32 = 0x5380_f646;

const fn tag(t: &[u8; 4]) -> i32 {
    i32::from_be_bytes(*t)
}

pub const TAG_SANI: i32 = tag(b"SANI");
pub const TAG_SCAM: i32 = tag(b"SCAM");
pub const TAG_SKEY: i32 = tag(b"SKEY");
pub const TAG_SLAT: i32 = tag(b"SLAT");
pub const TAG_SLIG: i32 = tag(b"SLIG");
pub const TAG_SMAT: i32 = tag(b"SMAT");
pub const TAG_ST2D: i32 = tag(b"ST2D");
pub const TAG_ST3D: i32 = tag(b"ST3D");

/// The text tags `dbFileReadMagic` takes, each followed by a version token; `SSCN` is taken
/// without one.
const TEXT_TAGS: [&[u8; 4]; 14] =
    [b"SKEY", b"SCHN", b"SANI", b"CLUS", b"SCAM", b"SDSC", b"SDRV", b"SFLK", b"SLIG", b"SLAT", b"SMAT", b"ST2D", b"SSET", b"ST3D"];

/// The files the converter reads, by the names it builds.
pub trait Host {
    /// The bytes of a file, `None` if `fopen` would fail.
    fn read(&self, path: &[u8]) -> Option<Vec<u8>>;
}

/// What the converter's run needs besides its files.
pub struct Env {
    /// The attribute dictionary's curve names (`SI_initDictionaries` reads
    /// `$SI_LOCATION/3D/rsrc/Relations.rsrc`).
    pub curves: CurveNames,
    /// The time a sun light gets when it is made (`si_initSunDefault`): the run's clock.
    pub now: i32,
    /// The interpolation a new function curve starts with (`SI_glbAnim` + 0x30). In
    /// `bin2ascii` `SI_glbAnim` is in `.bss` and nothing on `main`'s path was seen to set
    /// it: 0.
    pub default_interpolation: i16,
}

/// What one run left behind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// The exit status as the shell sees it: 1 is the normal end, 255 (`exit(-1)`) a
    /// failure.
    pub status: i32,
    /// What it printed on the standard output, and on the standard error.
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The output file, if it was created, and the name it was created under (the binary
    /// writers add their kind's suffix to the name they are given).
    pub output: Option<Vec<u8>>,
    pub output_name: Vec<u8>,
    /// What the text readers would have shown on the status line (the converter has none).
    pub status_line: Vec<Vec<u8>>,
}

/// A run the port cannot make: the original's path through a routine not ported.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done(Run),
    NotPorted(&'static str),
}

/// `dbFileReadMagic` (bin2ascii 0x080c2d1c): the file's magic number, whether it is a text
/// file, and its version (−1.0 unless read).
///
/// A binary magic number from 0x5380f634 to 0x5380f646 is followed by the version as a
/// big-endian float. A text tag of [`TEXT_TAGS`] is followed by a token, scanned with
/// `sscanf("%f")`; `SSCN` is text without a version. Anything else rewinds the file and
/// answers −1. Returns the offset reading stopped at as the last value.
pub fn db_file_read_magic(data: &[u8]) -> (i32, bool, f32, usize) {
    let mut s = si_file::Stream::new(data);
    // SI_byteswap_fread into a local: on a file shorter than four bytes the local keeps
    // what the stack held; here 0
    let magic = s.be_u32().unwrap_or(0) as i32;
    if (magic.wrapping_sub(MAGIC_GENERAL) as u32) <= 0x12 {
        let v = s.be_f32().unwrap_or(-1.0); // SI_float_fread leaves −1.0 on a short read
        return (magic, false, v, s.tell());
    }
    if TEXT_TAGS.iter().any(|t| tag(t) == magic) {
        let mut f = crate::token::AsciiFile::new(&data[4..]);
        f.token_read();
        let v = crate::cnum::scan_f(f.token()).unwrap_or(-1.0);
        return (magic, true, v, 4 + f.tell());
    }
    if magic == tag(b"SSCN") {
        return (magic, true, -1.0, 4);
    }
    (-1, false, -1.0, 0)
}

/// `getBasicName` (bin2ascii 0x080c2fb8): the name without what follows its last dot (the
/// whole path is searched, directories included); the whole name when there is no dot;
/// `None` (a null pointer) when the only dot is the first character.
pub fn get_basic_name(path: &[u8]) -> Option<Vec<u8>> {
    match path.iter().rposition(|&b| b == b'.') {
        None => Some(path.to_vec()),
        Some(0) => None,
        Some(i) => Some(path[..i].to_vec()),
    }
}

/// `getFnameSuffix` (bin2ascii 0x080c306c): what follows the last dot; the whole name when
/// there is none; `None` when the name is empty or ends with its dot.
pub fn get_fname_suffix(path: &[u8]) -> Option<Vec<u8>> {
    match path.iter().rposition(|&b| b == b'.') {
        _ if path.is_empty() => None,
        Some(i) if i + 1 == path.len() => None,
        Some(i) => Some(path[i + 1..].to_vec()),
        None => Some(path.to_vec()),
    }
}

fn cat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut v = a.to_vec();
    v.extend_from_slice(b);
    v
}

/// The old-file test of si_texture3DBinaryRead (0x083e77c8), as the original makes it:
/// `fld DWORD fileVersion; fcomp QWORD 2.1`, the single against the double in the x87 unit.
/// A file of version 2.1 (the single 0x40066666, 2.0999999…) is below the double 2.1, so it
/// is old: the channel is set to 2 before the tokens and again after them. `si-hrc`'s reader
/// rounds the constant to a single first and takes such a file for a new one; 59 of the 125
/// sample 3D textures have that version. Corrected here (the channel is the only field the
/// test decides) until `si-hrc` compares as the original does.
fn texture3d_old_channel(x: &mut si_hrc::Texture3D, version: f32) {
    if (version as f64) < 2.1 {
        x.channel = 2;
    }
}

/// What `dataWriteAsciiFile` is given.
pub enum Data<'a> {
    Material(&'a si_hrc::Material),
    Camera(&'a si_elemio::camera::Camera),
    Light(&'a si_elemio::light::Light),
    Lattices(&'a [si_elemio::lattice::Lattice]),
    /// A 3D texture, and its name pointer (`None` when null).
    Texture3D(&'a si_hrc::Texture3D, Option<&'a [u8]>),
}

/// `dataWriteAsciiFile` (bin2ascii 0x080c2e80): the header (the tag and the version,
/// SI_dbfileWriteAscHeader (0x083fb900) through `si-file`), then the element's writer
/// chosen by the tag. Answers the file and the writer's result. (The file is opened with
/// `"w"`; when that fails the original prints "Unable to open file" and answers 0. The 2D
/// texture's writer, `SI_textureAsciiWrite`, is not ported; nothing calls this with one
/// here.)
pub fn data_write_ascii_file(data: Data, tag: i32, version: f32, lattice_count: i32) -> (Vec<u8>, i16) {
    let mut o = Out::new(0);
    si_file::write_asc_header(&mut o.buf, version, tag as u32);
    let r = match data {
        Data::Material(m) => write::material_ascii_write(&mut o, Some(m)),
        Data::Camera(c) => write::camera_ascii_write(&mut o, Some(c)),
        Data::Light(l) => write::light_ascii_write(&mut o, Some(l)),
        Data::Lattices(l) => write::lattice_ascii_write(&mut o, lattice_count, Some(l)),
        Data::Texture3D(x, name) => write::texture3d_ascii_write(&mut o, Some(x), name),
    };
    (o.buf, r)
}

/// `si_elemdictWriteAscAnimations` (bin2ascii 0x080c6b60): the header with the tag and the
/// **output** version (the version argument is not used: `-k` does not reach an animation's
/// text), then the hierarchy animation ([`write::hieranim_ascii_write`]). Always 1 once
/// the file is open.
pub fn elemdict_write_asc_animations(ha: &si_elemio::anim::HierAnim, chapter: i32, tag: i32, names: &mut CurveNames) -> (Vec<u8>, i16) {
    let mut o = Out::new(0);
    si_file::write_asc_header(&mut o.buf, VERSION_OUTPUT, tag as u32);
    write::hieranim_ascii_write(&mut o, ha, chapter, names);
    (o.buf, 1)
}

/// The chapter a hierarchy animation belongs to, as `si_hieranimBinaryRead`
/// (0x08139094) sets it in the converter's element dictionary (`SI_elemdictInit`
/// and `SI_elemdictLoadChapters` in `main`: the 36 chapters, nothing else): the index of
/// the chapter the file names (SI_elemdictFindListByName (0x081325f4), −1 if none
/// has that name), or without a name the MODELS chapter (the word of `SI_cid` for
/// `ChapMod`, 8).
pub fn chapter_index(name: Option<&[u8]>) -> i32 {
    match name {
        None => 8,
        Some(n) => crate::srel::Relations::new().find_chapter(n),
    }
}

/// `elemdictReadAnimations` (bin2ascii 0x080c2a78): `SI_elemdictReadAnimations` of `soft`
/// without the action lists and without the pipe: the header, the element list, the items
/// until the item reader finds none. Through `si-elemio`'s reader, which goes on to the
/// action lists; the text does not print them. `None` (and a message on the standard
/// error) when the file cannot be opened.
fn elemdict_read_animations(host: &dyn Host, env: &Env, path: &[u8], run: &mut Run) -> Option<si_elemio::anim::HierAnim> {
    let Some(data) = host.read(path) else {
        run.stderr.extend_from_slice(&cat(&cat(b"ERROR: Cannot open file ", path), b"\n"));
        return None;
    };
    let mut r = Reader::new(&data);
    let f = si_elemio::anim::read_file(&mut r, env.default_interpolation);
    Some(f.anim)
}

/// `main` (bin2ascii 0x080c311c) for `bin2ascii <input> <output> [-k]`.
///
/// The input is opened and its magic read ([`db_file_read_magic`]). Without `-k` the
/// version written is this build's (4.0), with it the file's. A file of the general magic
/// takes its kind from its name's suffix (`mtr`, `t2d`, `t3d`, `lat`, `cam`, `lig`, `ani`,
/// `key`). A binary file is closed again and read anew by the kind's reader:
///
/// - material, 2D texture, 3D texture, lattice, camera, light: by `soft`'s
///   `SI_<kind>ReadBinaryFile`, given the input's name without its suffix: the reader adds
///   `.mtr`, `.txt`, `.t3D`, `.lat`, `.cam`, `.lig` and opens that. **A 3D texture file
///   named `….t3d` is therefore not found** (the reader looks for `….t3D`), nor a 2D texture
///   file named other than `….txt`;
/// - animation and struct key files: by `elemdictReadAnimations`, given the input's name.
///
/// Then the text is written. Messages: "Unknown input data file type", "Problem reading
/// input file" (the reader gave nothing), "Problem writing output file" (the writer
/// answered 0); the exit status is 1 when both succeeded, 255 otherwise.
pub fn main(host: &dyn Host, env: &mut Env, input: &[u8], output: &[u8], keep: bool) -> Outcome {
    let mut run = Run::default();
    let Some(data) = host.read(input) else {
        run.stdout = cat(&cat(b"Unable to open file ", input), b"\n");
        run.status = 255; // exit(getopt's −1)
        return Outcome::Done(run);
    };
    let (mut magic, is_text, file_version, _) = db_file_read_magic(&data);
    let version = if keep { file_version } else { VERSION_OUTPUT };
    if magic == MAGIC_GENERAL {
        // a name with no suffix gives a null pointer to strcmp in the original
        let Some(suffix) = get_fname_suffix(input) else { return Outcome::NotPorted("a general-magic file without a suffix") };
        for (s, m) in [
            (&b"mtr"[..], MAGIC_MATERIAL),
            (b"t2d", MAGIC_TEXTURE_2D),
            (b"t3d", MAGIC_TEXTURE_3D),
            (b"lat", MAGIC_LATTICE),
            (b"cam", MAGIC_CAMERA),
            (b"lig", MAGIC_LIGHT),
            (b"ani", MAGIC_ANIMATION),
            (b"key", MAGIC_STRUCT_KEYS),
        ] {
            if suffix == s {
                magic = m;
                break;
            }
        }
    }
    if is_text {
        let (read_ok, write_ok) = match text_to_binary(env, &data, magic, version, input, output, &mut run) {
            Ok(x) => x,
            Err(why) => return Outcome::NotPorted(why),
        };
        return end_of_main(run, read_ok, write_ok, input, output);
    }
    let base = get_basic_name(input);
    let open = |ext: &[u8]| -> Option<Vec<u8>> {
        // sprintf("%s.ext", base): a null base prints as "(null)"
        let name = cat(&cat(base.as_deref().unwrap_or(b"(null)"), b"."), ext);
        host.read(&name)
    };
    let mut read_ok = false;
    let mut write_ok = false;
    match magic {
        MAGIC_MATERIAL => {
            // SI_materialReadBinaryFile (0x083e8b14), si-hrc's reader
            let mat = open(b"mtr").and_then(|d| si_hrc::read_file(&d, si_hrc::Kind::Material)).and_then(|f| match f.content {
                si_hrc::Content::Material(m) => Some(m),
                _ => None,
            });
            if let Some(m) = mat {
                read_ok = true;
                let (text, r) = data_write_ascii_file(Data::Material(&m), TAG_SMAT, version, 0);
                run.output = Some(text);
                run.output_name = output.to_vec();
                write_ok = r != 0;
            }
        }
        MAGIC_TEXTURE_2D => {
            // SI_textureReadBinaryFile (0x083e8d30) opens `<base>.txt`
            let found = open(b"txt").and_then(|d| si_hrc::read_file(&d, si_hrc::Kind::Texture)).map(|f| !matches!(f.content, si_hrc::Content::None));
            if found == Some(true) {
                return Outcome::NotPorted("the 2D texture writer SI_textureAsciiWrite");
            }
        }
        MAGIC_TEXTURE_3D => {
            // SI_texture3DReadBinaryFile (0x083e8f50), si-hrc's reader: `<base>.t3D`, and the
            // texture only when the tag after the prototypes is `TX3D` (else a null pointer)
            let file = open(b"t3D").and_then(|d| si_hrc::read_file(&d, si_hrc::Kind::Texture3D));
            if let Some(mut f) = file {
                let file_version = f.header.as_ref().map_or(-1.0, |h| h.version);
                if let si_hrc::Content::Texture3D(x) = &mut f.content {
                    texture3d_old_channel(x, file_version);
                }
                if let si_hrc::Content::Texture3D(x) = &f.content {
                    read_ok = true;
                    // the name pointer stays null unless the file's table has token 1
                    let named = f.protos.get(si_hrc::proto::TEXTURE_3D).is_some_and(|t| t.contains(&1));
                    let name = if named { Some(&x.name[..]) } else { None };
                    let (text, r) = data_write_ascii_file(Data::Texture3D(x, name), TAG_ST3D, version, 0);
                    run.output = Some(text);
                    run.output_name = output.to_vec();
                    write_ok = r != 0;
                }
            }
        }
        MAGIC_LATTICE => {
            // SI_latticeReadBinaryFile (0x083ebf60), si-elemio's reader
            if let Some(d) = open(b"lat") {
                let f = si_elemio::lattice::read_file_converter(&mut Reader::new(&d));
                // the reader answers its list, which it allocates with the first lattice;
                // with none it answers a local it never set
                if f.read.lattices.is_empty() {
                    return Outcome::NotPorted("a lattice file without a lattice (the reader answers an unset local)");
                }
                read_ok = true;
                let (text, r) = data_write_ascii_file(Data::Lattices(&f.read.lattices), TAG_SLAT, version, f.read.count);
                run.output = Some(text);
                run.output_name = output.to_vec();
                write_ok = r != 0;
            }
        }
        MAGIC_CAMERA => {
            // SI_cameraReadBinaryFile (0x083ec1c8), si-elemio's reader
            if let Some(d) = open(b"cam") {
                let (cam, _) = si_elemio::camera::read_file_converter(&mut Reader::new(&d));
                read_ok = true;
                let (text, r) = data_write_ascii_file(Data::Camera(&cam), TAG_SCAM, version, 0);
                run.output = Some(text);
                run.output_name = output.to_vec();
                write_ok = r != 0;
            }
        }
        MAGIC_LIGHT => {
            // SI_lightReadBinaryFile (0x083ec348), si-elemio's reader
            if let Some(d) = open(b"lig") {
                let f = si_elemio::light::read_file_converter(&mut Reader::new(&d), env.now);
                if let Some(l) = f.read.light.as_ref() {
                    read_ok = true;
                    let (text, r) = data_write_ascii_file(Data::Light(l), TAG_SLIG, version, 0);
                    run.output = Some(text);
                    run.output_name = output.to_vec();
                    write_ok = r != 0;
                }
            }
        }
        MAGIC_ANIMATION | MAGIC_STRUCT_KEYS => {
            if let Some(ha) = elemdict_read_animations(host, env, input, &mut run) {
                read_ok = true;
                let t = if magic == MAGIC_ANIMATION { TAG_SANI } else { TAG_SKEY };
                let chapter = chapter_index(ha.chapter.as_deref());
                let (text, r) = elemdict_write_asc_animations(&ha, chapter, t, &mut env.curves);
                run.output = Some(text);
                run.output_name = output.to_vec();
                write_ok = r != 0;
            }
        }
        _ => {
            run.stdout.extend_from_slice(&cat(&cat(b"Unknown input data file type: ", input), b"\n"));
        }
    }
    end_of_main(run, read_ok, write_ok, input, output)
}

/// The end of `main`: "Problem reading input file" when the reader gave nothing, else
/// "Problem writing output file" when the writer answered 0; exit status 1 when both
/// succeeded, 255 otherwise.
fn end_of_main(mut run: Run, read_ok: bool, write_ok: bool, input: &[u8], output: &[u8]) -> Outcome {
    if !read_ok {
        run.stdout.extend_from_slice(&cat(&cat(b"Problem reading input file ", input), b"\n"));
    } else if !write_ok {
        run.stdout.extend_from_slice(&cat(&cat(b"Problem writing output file ", output), b"\n"));
    }
    run.status = if read_ok && write_ok { 1 } else { 255 };
    Outcome::Done(run)
}

/// `fgets(buf, n, f)`: up to `n − 1` bytes, to and with the first newline.
fn fgets(f: &mut crate::token::AsciiFile, n: usize) {
    for _ in 0..n.saturating_sub(1) {
        let c = f.getc();
        if c == -1 || c == b'\n' as i32 {
            break;
        }
    }
}

/// The text half of `main` (bin2ascii 0x080c311c): the rest of the header line is skipped
/// (`fgets` of up to 999 bytes into `SI_dbInToken`), the kind's text reader fills a fresh
/// element, and `soft`'s binary writer writes it under the output name with the kind's
/// suffix added:
///
/// | tag | element | reader | writer (through) | file |
/// |---|---|---|---|---|
/// | `SMAT` | `SI_materialAllocate` | [`read::material_ascii_read`] | `SI_materialWriteBinaryFile` (`si-hrc`) | `<out>.mtr` |
/// | `SCAM` | `SI_newCamera("Test", 1)` | [`read::camera_ascii_read`] | `SI_cameraWriteBinaryFile` (`si-elemio`) | `<out>.cam` |
/// | `SLIG` | `SI_lightAllocate(1)` | [`read::light_ascii_read`] | `SI_lightWriteBinaryFile` (`si-elemio`) | `<out>.lig` |
/// | `SLAT` | none | [`read::lattice_ascii_read`] | `SI_latticeWriteBinaryFile` (`si-elemio`) | `<out>.lat` |
/// | `SANI`, `SKEY` | `SI_hieranimAllocate(0)` | [`read::elemdict_read_asc_animations`] | `hieraniWriteBinaryFile` (here) | `<out>.ani`, `<out>.key` |
/// | `ST3D` | `SI_texture3DAllocate` | [`read::texture3d_ascii_read`] | `SI_texture3DWriteBinaryFile` (`si-hrc`) | `<out>.t3D` |
///
/// The 2D texture reader is not ported; any other text tag is "Unknown input data file
/// type".
fn text_to_binary(env: &mut Env, data: &[u8], magic: i32, version: f32, input: &[u8], output: &[u8], run: &mut Run) -> Result<(bool, bool), &'static str> {
    use crate::setup::{Reader as TextReader, Session, Setup};
    let mut f = crate::token::AsciiFile::new(data);
    f.seek(4);
    if magic != tag(b"SSCN") {
        f.token_read(); // what dbFileReadMagic read the version from
    }
    fgets(&mut f, 1000);
    let session = Session::default();
    let setup = Setup::default();
    let mut r = TextReader::new(&mut f, &session);
    let mut p = read::Printed::default();
    let read_ok;
    let mut write_ok = false;
    // a text on which the original would loop for ever or fault
    let fail = |_e: crate::setup::SetupError| "a text the original's reader does not come back from";
    let mut w = si_elemio::io::Writer::new();
    let suffix: &[u8];
    match magic {
        TAG_SMAT => {
            let mut m = si_hrc::Material::default();
            match read::material_ascii_read(&mut r, &setup, &mut m) {
                Ok(ok) => read_ok = ok != 0,
                Err(e) => return Err(fail(e)),
            }
            suffix = b".mtr";
            if read_ok {
                // SI_materialWriteBinaryFile (0x083e8a04), si-hrc's writer
                w.out = si_hrc::write_material(&m, version);
                write_ok = true;
            }
        }
        TAG_SCAM => {
            let mut c = si_elemio::camera::Camera::new(b"Test", 1);
            match read::camera_ascii_read(&mut r, &setup, &mut p, &mut c) {
                Ok(ok) => read_ok = ok != 0,
                Err(e) => return Err(fail(e)),
            }
            suffix = b".cam";
            if read_ok {
                // SI_cameraWriteBinaryFile (0x083ec0fc), si-elemio's writer
                si_elemio::camera::write_file_converter(&mut w, version, &c);
                write_ok = true;
            }
        }
        TAG_SLIG => {
            let mut l = si_elemio::light::Light::new(1, env.now);
            match read::light_ascii_read(&mut r, &setup, &mut p, &mut l, env.now) {
                Ok(ok) => read_ok = ok != 0,
                Err(e) => return Err(fail(e)),
            }
            suffix = b".lig";
            if read_ok {
                // SI_lightWriteBinaryFile (0x083ec274), si-elemio's writer
                si_elemio::light::write_file_converter(&mut w, version, &l);
                write_ok = true;
            }
        }
        TAG_SLAT => {
            let list = match read::lattice_ascii_read(&mut r, &mut p) {
                Ok(l) => l,
                Err(e) => return Err(fail(e)),
            };
            suffix = b".lat";
            // the reader answers its list, null when it made no lattice
            read_ok = !list.is_empty();
            if read_ok {
                // SI_latticeWriteBinaryFile (0x083ec014), si-elemio's writer
                write_ok = si_elemio::lattice::write_file_converter(&mut w, version, &list).is_ok();
            }
        }
        TAG_SANI | TAG_SKEY => {
            let (ha, chapter) = match read::elemdict_read_asc_animations(&mut r, &mut p, env.default_interpolation, 4.0) {
                Ok(x) => x,
                Err(e) => return Err(fail(e)),
            };
            if ha.chapter.is_none() {
                return Err("a chapter index outside the dictionary (the writer reads its name from beyond the list)");
            }
            let _ = chapter;
            read_ok = true;
            let key = magic == TAG_SKEY;
            suffix = if key { b".key" } else { b".ani" };
            hierani_write_binary_file(&mut w, key, version, &ha);
            write_ok = true;
        }
        TAG_ST3D => {
            // SI_texture3DAllocate (0x082b8cf0): SI_texture3DInitialize's values (+0x114 is
            // left as malloc gives it; the text always sets it)
            let mut x = si_hrc::Texture3D::default();
            match read::texture3d_ascii_read(&mut r, &setup, &mut x) {
                Ok(ok) => read_ok = ok != 0,
                Err(read::TextureStop::Setup(e)) => return Err(fail(e)),
                Err(read::TextureStop::UninitialisedLocal) => {
                    return Err("a texture value taken from the reader's uninitialised local")
                }
            }
            suffix = b".t3D";
            if read_ok {
                // SI_texture3DWriteBinaryFile (0x083e8e48), si-hrc's writer
                w.out = si_hrc::write_texture3d(&x, version);
                write_ok = true;
            }
        }
        TAG_ST2D => return Err("the 2D texture text reader SI_textureAsciiRead"),
        _ => {
            run.stdout.extend_from_slice(&cat(&cat(b"Unknown input data file type: ", input), b"\n"));
            return Ok((false, false));
        }
    }
    run.stdout.extend_from_slice(&p.stdout);
    run.status_line = p.status;
    if read_ok {
        run.output = Some(w.out);
        run.output_name = cat(output, suffix);
    }
    Ok((read_ok, write_ok))
}

/// `hieraniWriteBinaryFile` (bin2ascii 0x080c2c54) after its `fopen`: the header
/// (si_newheaderBinaryWrite (0x083fb88c) through `si-file`: the struct key or animation
/// magic, the version given, the default comment), then `fullHieranimBinaryWrite`
/// (bin2ascii 0x080c2ba0): the element list with the chapter's name
/// (si_hieranimBinaryWrite (0x08138fbc)) and every item of every element in order
/// (SI_elemdictWriteAnimation (0x08110a94), both through `si-elemio`), each with its
/// element's index as owner and 1 as its flag.
pub fn hierani_write_binary_file(w: &mut si_elemio::io::Writer, key: bool, version: f32, ha: &si_elemio::anim::HierAnim) {
    let magic = if key { MAGIC_STRUCT_KEYS } else { MAGIC_ANIMATION };
    si_file::write_bin_header(&mut w.out, magic as u32, version, None);
    si_elemio::anim::write_hieranim(w, ha.chapter.as_deref(), ha);
    for (i, e) in ha.elems.iter().enumerate() {
        for it in &e.items {
            si_elemio::anim::write_item(w, &it.data, i as i32, it.kind as i32, it.act_rel_id, it.act_occ_id as i32, 1);
        }
    }
}

#[cfg(test)]
mod tests;
