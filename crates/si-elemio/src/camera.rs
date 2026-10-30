// SPDX-License-Identifier: MIT
//! Camera files (`.cam`, magic 0x5380f636): finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `bsceneio.c` (the program's reader), `b_structio.c`
//! (the writer, and the reader of the stand-alone converter), the element dictionary's file
//! (the functions that open the file) and the camera's own file (`SI_resetCamera`).
//!
//! Field names: the keywords are the ones the vendor's converter `bin2ascii` prints for each
//! field (`SI_cameraAsciiWrite`, read from its disassembly); the projection plane's names are
//! from the SDK header `SAA_gen.h` (`SAA_cameraGetProjPlane…`).

use crate::io::{Constraint, End, Reader, UserNode, Writer};
use si_file::{write_bin_header, write_db_header, MAGIC_CAMERA};

/// `SI_fmtAspect[13]` as shipped (0x08679574, a writable table): the aspect ratio of the
/// picture format a new camera starts with.
pub const DEFAULT_FORMAT_ASPECT: f32 = 1.18;

/// The camera record (`SI_Camera`, 0x13c bytes), as far as a camera file and
/// `SI_resetCamera` reach into it. Fields whose meaning has not been read are named by their
/// offset. Everything else in the record is left out; the original allocates the record with
/// `malloc`, so those parts hold stale memory there.
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// +0x00 (a string pointer). Not stored in a camera file.
    pub name: Vec<u8>,
    /// +0x0a
    pub w_0a: i16,
    /// +0x0c `SI_CameraMode`: 1 for the perspective camera
    pub mode: i16,
    /// +0x10 `POSITION`
    pub position: [f32; 3],
    /// +0x1c `INTEREST`: the point looked at
    pub interest: [f32; 3],
    /// +0x28, +0x2c, +0x30 `YAW_PITCH_ROLL`; the third is also written alone as `ROLL`.
    /// Radians.
    pub yaw_pitch_roll: [f32; 3],
    /// +0x34 `NEAR`: near clipping distance
    pub near: f32,
    /// +0x38 `FAR`
    pub far: f32,
    /// +0x3c
    pub w_3c: i16,
    /// +0x40 `FIELD_OF_VIEW`, radians for a perspective camera
    pub field_of_view: f32,
    /// +0x44
    pub w_44: i16,
    /// +0x48 (reset to 0, 0, 1)
    pub v_48: [f32; 3],
    /// +0x54: index into the picture format table (reset to 13)
    pub format: i32,
    /// +0x58: that format's aspect ratio
    pub format_aspect: f32,
    /// +0x5c, +0x60 (reset to 1.0)
    pub f_5c: f32,
    pub f_60: f32,
    /// +0x64: the factor between the projection plane's height and width (reset to 1.0)
    pub f_64: f32,
    /// +0x68 (reset to 1.0)
    pub f_68: f32,
    /// +0x6c `DEPTH_OF_FIELD`: 0 `OFF`, 1 `AUTOMATIC`, 2 `CUSTOM`
    pub depth_of_field: i32,
    /// +0x70 `FOCAL_LENGTH`
    pub focal_length: f32,
    /// +0x74 `FOCAL_DISTANCE`
    pub focal_distance: f32,
    /// +0x7c `NEAR_FOCUS`
    pub near_focus: f32,
    /// +0x84 `FAR_FOCUS`
    pub far_focus: f32,
    /// +0x8c `OUT_OF_FOCUS`
    pub out_of_focus: f32,
    /// +0x94 `F_STOP`
    pub f_stop: f32,
    /// +0x98 `MAXIMUM_COC`: largest circle of confusion
    pub maximum_coc: f32,
    /// +0x9c `ENLARGE`: non-zero `ACTIVE`
    pub enlarge: i16,
    /// +0xa0 is a model pointer in the original, filled in later from this
    pub position_constraint: Option<Constraint>,
    /// +0xa4 likewise for the interest
    pub interest_constraint: Option<Constraint>,
    /// +0xa8 (set to 1 by the reset and at the start of every read)
    pub w_a8: i16,
    /// +0xac, +0xb0 (zeroed by the reset and at the start of every read)
    pub d_ac: i32,
    pub d_b0: i32,
    /// +0xb4
    pub w_b4: i16,
    /// +0x108 projection plane: active flag
    pub proj_plane_active: i16,
    /// +0x10c projection plane: distance
    pub proj_plane_distance: f32,
    /// +0x110, +0x114 projection plane: size (width, height)
    pub proj_plane_size: [f32; 2],
    /// +0x118, +0x11c projection plane: offset
    pub proj_plane_offset: [f32; 2],
    /// +0x138 user data attached by plug-ins
    pub user_data: Vec<UserNode>,
}

impl Camera {
    /// A new camera [soft 0x080d12dc SI_newCamera]: the name, then the reset.
    pub fn new(name: &[u8], mode: i16) -> Camera {
        let mut c = Camera {
            name: name.to_vec(),
            w_0a: 0,
            mode: 0,
            position: [0.0; 3],
            interest: [0.0; 3],
            yaw_pitch_roll: [0.0; 3],
            near: 0.0,
            far: 0.0,
            w_3c: 0,
            field_of_view: 0.0,
            w_44: 0,
            v_48: [0.0; 3],
            format: 0,
            format_aspect: 0.0,
            f_5c: 0.0,
            f_60: 0.0,
            f_64: 0.0,
            f_68: 0.0,
            depth_of_field: 0,
            focal_length: 0.0,
            focal_distance: 0.0,
            near_focus: 0.0,
            far_focus: 0.0,
            out_of_focus: 0.0,
            f_stop: 0.0,
            maximum_coc: 0.0,
            enlarge: 0,
            position_constraint: None,
            interest_constraint: None,
            w_a8: 0,
            d_ac: 0,
            d_b0: 0,
            w_b4: 0,
            proj_plane_active: 0,
            proj_plane_distance: 0.0,
            proj_plane_size: [0.0; 2],
            proj_plane_offset: [0.0; 2],
            user_data: Vec::new(),
        };
        c.reset(mode, DEFAULT_FORMAT_ASPECT);
        c
    }

    /// The camera `SI_cameraReadBinaryFile` starts from: `SI_newCamera("Perspective def", 1)`.
    pub fn perspective_default() -> Camera {
        Camera::new(b"Perspective def", 1)
    }

    /// [soft 0x080d10e0 SI_resetCamera]. `format_aspect` is the current value of
    /// `SI_fmtAspect[13]` ([`DEFAULT_FORMAT_ASPECT`] unless the program changed its table).
    /// The name is kept.
    pub fn reset(&mut self, mode: i16, format_aspect: f32) {
        self.w_0a = 0;
        self.mode = mode;
        self.field_of_view = if mode == 1 { 0.785_398_2 } else { 20.0 }; // 0x3f490fdb, a quarter of pi
        self.format = 13;
        self.format_aspect = format_aspect;
        self.f_5c = 1.0;
        self.f_60 = 1.0;
        self.focal_length = 50.0;
        self.f_stop = 5.6;
        self.focal_distance = 20.0;
        self.near_focus = 15.0;
        self.far_focus = 25.0;
        self.out_of_focus = 40.0;
        self.maximum_coc = 20.0;
        self.f_64 = 1.0;
        self.f_68 = 1.0;
        self.near = 0.1;
        self.far = 32768.0;
        self.v_48 = [0.0, 0.0, 1.0];
        self.position = [0.0, 0.0, 100.0];
        self.w_3c = 0;
        self.depth_of_field = 0;
        self.enlarge = 0;
        self.w_44 = 0;
        self.interest = [0.0; 3];
        self.yaw_pitch_roll = [0.0; 3];
        self.d_ac = 0;
        self.d_b0 = 0;
        self.w_b4 = 0;
        self.position_constraint = None;
        self.interest_constraint = None;
        self.w_a8 = 1;
        self.reset_proj_plane();
        self.user_data = Vec::new();
    }

    /// [soft 0x080d1264 SI_resetCamProjPlane]: the plane at the focal length, as high as the
    /// field of view makes it there, not active, no offset.
    ///
    /// Order in the original: `tan` of half the field of view (the half is exact) comes back
    /// on the x87 stack at extended precision, is multiplied by twice the distance and the
    /// product stored as a single; the width is that single times the factor at +0x64. Here
    /// the tangent and the product are doubles. The two can differ in the last bit of the
    /// single only if the exact product lies within about 2⁻⁵² of a rounding boundary; this
    /// is not checked against the original (nothing is executed).
    pub fn reset_proj_plane(&mut self) {
        self.proj_plane_distance = self.focal_length;
        self.proj_plane_active = 0;
        let t = (self.field_of_view as f64 * 0.5).tan();
        let d = self.proj_plane_distance as f64;
        let height = ((d + d) * t) as f32;
        self.proj_plane_size[1] = height;
        self.proj_plane_offset = [0.0, 0.0];
        self.proj_plane_size[0] = (height as f64 * self.f_64 as f64) as f32;
    }
}

/// Which of the two readers: they differ in how a position is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The program's reader (`bsceneio.c`): a position may follow a model.
    Program,
    /// The converter's reader (`b_structio.c`, also linked into the program): a position is
    /// a vector or is skipped.
    Converter,
}

/// Reads the token stream of a camera into `cam` [soft 0x083f789c SI_cameraBinaryRead]
/// (`Variant::Program`) or [soft 0x083e990c b2aCameraBinaryRead] (`Variant::Converter`).
/// Fields the stream does not mention keep the values `cam` came with.
///
/// | token | field |
/// |---|---|
/// | 0 | end |
/// | 1, 2 | a 16-bit number, read and dropped |
/// | 3 | position: `POSITION` |
/// | 4 | position: `INTEREST` |
/// | 5 | value: `ROLL` |
/// | 6 | three values: `YAW_PITCH_ROLL` |
/// | 7 | value: `NEAR` |
/// | 8 | value: `FAR` |
/// | 9 | value: `FOCAL_LENGTH` |
/// | 10 | value: `F_STOP` |
/// | 11 | value: `FOCAL_DISTANCE` |
/// | 12 | value: `FIELD_OF_VIEW` |
/// | 13 | 16-bit: `DEPTH_OF_FIELD` |
/// | 14 | value: `NEAR_FOCUS` |
/// | 15 | value: `FAR_FOCUS` |
/// | 16 | value: `OUT_OF_FOCUS` |
/// | 17 | value: `MAXIMUM_COC` |
/// | 18 | 16-bit: `ENLARGE` |
/// | 19 | value, read and dropped |
/// | 0x14, 0x1614 | 16-bit and five values: the projection plane (active, distance, width, height, offset x, offset y) |
/// | 0xff15 | 32-bit size, then a user-data list |
/// | other | skipped by the token's size byte |
///
/// A "value" is [`Reader::value`]; a position is [`Reader::old_position`] or
/// [`Reader::position`].
///
/// Before the first token the reader zeroes +0xac and +0xb0 and sets +0xa8 to 1.
/// The original, given no camera, reads 16-bit words until a zero word and reports failure;
/// that case is not offered here.
pub fn read_tokens(r: &mut Reader, cam: &mut Camera, variant: Variant) -> End {
    cam.d_ac = 0;
    cam.d_b0 = 0;
    cam.w_a8 = 1;
    loop {
        let ok = match variant {
            Variant::Program => crate::io::fread_u16(&mut r.s, &mut r.st.token_bsceneio),
            Variant::Converter => crate::io::fread_u16(&mut r.s, &mut r.st.token_bstructio),
        };
        let token = match variant {
            Variant::Program => r.st.token_bsceneio,
            Variant::Converter => r.st.token_bstructio,
        };
        r.note("camera", token, ok);
        if token == 0 {
            return End::Zero;
        }
        if !ok {
            return End::Eof;
        }
        match token {
            1 | 2 => {
                r.svalue();
            }
            3 => read_position(r, variant, &mut cam.position, &mut cam.position_constraint, 4),
            4 => read_position(r, variant, &mut cam.interest, &mut cam.interest_constraint, 5),
            5 => r.value(Some(&mut cam.yaw_pitch_roll[2])),
            6 => r.value3(Some(&mut cam.yaw_pitch_roll)),
            7 => r.value(Some(&mut cam.near)),
            8 => r.value(Some(&mut cam.far)),
            9 => r.value(Some(&mut cam.focal_length)),
            10 => r.value(Some(&mut cam.f_stop)),
            11 => r.value(Some(&mut cam.focal_distance)),
            12 => r.value(Some(&mut cam.field_of_view)),
            13 => cam.depth_of_field = r.svalue() as i32,
            14 => r.value(Some(&mut cam.near_focus)),
            15 => r.value(Some(&mut cam.far_focus)),
            16 => r.value(Some(&mut cam.out_of_focus)),
            17 => r.value(Some(&mut cam.maximum_coc)),
            18 => cam.enlarge = r.svalue(),
            19 => r.value(None),
            0x14 | 0x1614 => {
                cam.proj_plane_active = r.svalue();
                r.value(Some(&mut cam.proj_plane_distance));
                r.value(Some(&mut cam.proj_plane_size[0]));
                r.value(Some(&mut cam.proj_plane_size[1]));
                r.value(Some(&mut cam.proj_plane_offset[0]));
                r.value(Some(&mut cam.proj_plane_offset[1]));
            }
            0xff15 => {
                let mut size = 0u32;
                crate::io::fread_u32(&mut r.s, &mut size);
                // the original stores the new list's head in the camera without freeing an
                // earlier list
                cam.user_data = r.user_data();
            }
            _ => {
                r.skip_field(token);
            }
        }
    }
}

fn read_position(r: &mut Reader, variant: Variant, v: &mut [f32; 3], c: &mut Option<Constraint>, graf_type: i32) {
    match variant {
        Variant::Program => {
            if let Some(found) = r.old_position(Some(v), graf_type) {
                *c = Some(found);
            }
        }
        Variant::Converter => r.position(Some(v)),
    }
}

/// Writes the token stream of a camera [soft 0x083e9480 SI_cameraBinaryWrite]: tokens 3 to
/// 18 in order, the projection plane under token 0x1614, the user data under 0xff15 if the
/// camera has any, and the zero token. Positions are always written as vectors: a position
/// that follows a model is saved as the position it had.
///
/// `DEPTH_OF_FIELD` is written from the low 16 bits of the 32-bit field.
pub fn write_tokens(w: &mut Writer, cam: &Camera) {
    w.token_bstructio(3);
    w.position(&cam.position);
    w.token_bstructio(4);
    w.position(&cam.interest);
    w.token_bstructio(5);
    w.value(cam.yaw_pitch_roll[2]);
    w.token_bstructio(6);
    w.value3(&cam.yaw_pitch_roll);
    w.token_bstructio(7);
    w.value(cam.near);
    w.token_bstructio(8);
    w.value(cam.far);
    w.token_bstructio(9);
    w.value(cam.focal_length);
    w.token_bstructio(10);
    w.value(cam.f_stop);
    w.token_bstructio(11);
    w.value(cam.focal_distance);
    w.token_bstructio(12);
    w.value(cam.field_of_view);
    w.token_bstructio(13);
    w.svalue(cam.depth_of_field as i16);
    w.token_bstructio(14);
    w.value(cam.near_focus);
    w.token_bstructio(15);
    w.value(cam.far_focus);
    w.token_bstructio(16);
    w.value(cam.out_of_focus);
    w.token_bstructio(17);
    w.value(cam.maximum_coc);
    w.token_bstructio(18);
    w.svalue(cam.enlarge);
    w.token_bstructio(0x1614);
    w.svalue(cam.proj_plane_active);
    w.value(cam.proj_plane_distance);
    w.value(cam.proj_plane_size[0]);
    w.value(cam.proj_plane_size[1]);
    w.value(cam.proj_plane_offset[0]);
    w.value(cam.proj_plane_offset[1]);
    if !cam.user_data.is_empty() {
        w.token_bstructio(0xff15);
        w.user_data(&cam.user_data);
    }
    w.token_bstructio(0);
}

/// What reading a camera file gives besides the camera.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraFile {
    /// The header's version, −1.0 if the header was not accepted (the original reads on from
    /// the start of the file in that case).
    pub version: f32,
    pub comment: Vec<u8>,
    pub end: End,
    /// Offset of the first byte not read.
    pub pos: usize,
    /// A size in the file was cut to the file's length (see [`Reader::clamped`]).
    pub clamped: bool,
}

/// Reads a camera file into `cam` the way the program loads one
/// [soft 0x0810fdf0 SI_elemdictReadCamera]: the header, whatever its magic, then the token
/// stream into the scene's camera.
pub fn read_file(r: &mut Reader, cam: &mut Camera) -> CameraFile {
    read_file_as(r, cam, Variant::Program)
}

/// Reads a camera file the way the converter's entry point does
/// [soft 0x083ec1c8 SI_cameraReadBinaryFile]: a new "Perspective def" camera, the header, the
/// token stream with the converter's reader. (The original builds the file name from a base
/// name and `.cam`; here the bytes are given.)
pub fn read_file_converter(r: &mut Reader) -> (Camera, CameraFile) {
    let mut cam = Camera::perspective_default();
    let f = read_file_as(r, &mut cam, Variant::Converter);
    (cam, f)
}

fn read_file_as(r: &mut Reader, cam: &mut Camera, variant: Variant) -> CameraFile {
    let (version, comment) = crate::io::read_header(r);
    let end = read_tokens(r, cam, variant);
    CameraFile { version, comment, end, pos: r.s.tell(), clamped: r.clamped }
}

/// A camera file as the program saves it [soft 0x08110498 SI_elemdictWriteCamera]: a header
/// with the camera magic, the output version and the default comment, then the tokens.
pub fn write_file(w: &mut Writer, cam: &Camera) {
    write_db_header(&mut w.out, w.version_output, MAGIC_CAMERA);
    write_tokens(w, cam);
}

/// A camera file as the converter's entry point writes it
/// [soft 0x083ec0fc SI_cameraWriteBinaryFile]: the same, with the header's version given by
/// the caller. The token writer still follows `w.version_output`.
pub fn write_file_converter(w: &mut Writer, version: f32, cam: &Camera) {
    write_bin_header(&mut w.out, MAGIC_CAMERA, version, None);
    write_tokens(w, cam);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Camera {
        let mut c = Camera::perspective_default();
        c.position = [1.0, -2.0, 3.5];
        c.interest = [0.25, 0.5, 0.75];
        c.yaw_pitch_roll = [0.1, 0.2, 0.3];
        c.field_of_view = 0.725;
        c.depth_of_field = 2;
        c.enlarge = 1;
        c.proj_plane_active = 1;
        c.proj_plane_offset = [0.5, -0.5];
        c
    }

    #[test]
    fn defaults() {
        let c = Camera::perspective_default();
        assert_eq!(c.position, [0.0, 0.0, 100.0]);
        assert_eq!((c.near, c.far, c.focal_length, c.f_stop), (0.1, 32768.0, 50.0, 5.6));
        assert_eq!(c.field_of_view.to_bits(), 0x3f49_0fdb);
        assert_eq!(c.proj_plane_distance, 50.0);
        // 2 × 50 × tan(pi / 8)
        assert!((c.proj_plane_size[1] - 41.421356).abs() < 1e-4);
        assert_eq!(c.proj_plane_size[0], c.proj_plane_size[1]);
        assert_eq!(Camera::new(b"x", 2).field_of_view, 20.0);
    }

    #[test]
    fn round_trip() {
        let cam = sample();
        let mut w = Writer::new();
        write_file(&mut w, &cam);
        // header 88; two positions 2 × 16; eleven single values 11 × 8; the value triple 16;
        // two shorts 2 × 4; the projection plane 2 + 2 + 5 × 6; the zero token 2
        assert_eq!(w.out.len(), 268);
        let mut r = Reader::new(&w.out);
        let mut got = Camera::new(b"Perspective def", 1);
        let f = read_file(&mut r, &mut got);
        assert_eq!((f.version, f.end, f.pos), (4.0, End::Zero, w.out.len()));
        assert_eq!(got, cam);
        let (got, f) = read_file_converter(&mut Reader::new(&w.out));
        assert_eq!((got, f.end), (cam, End::Zero));
    }

    #[test]
    fn a_position_that_follows_a_model() {
        let mut body = vec![0, 3, 0, 3, 0, 7];
        body.extend_from_slice(b"target\0");
        body.extend_from_slice(&[0, 0]);
        let mut cam = Camera::perspective_default();
        let mut r = Reader::new(&body);
        assert_eq!(read_tokens(&mut r, &mut cam, Variant::Program), End::Zero);
        let c = cam.position_constraint.unwrap();
        assert_eq!((c.graf_type, c.index, c.name.as_slice()), (4, 7, &b"target"[..]));
        assert_eq!(cam.position, [0.0, 0.0, 100.0]);
        // the converter's reader has no such form: inner token 3 has no size byte, nothing is
        // skipped, and the stream is misread from there
        let mut cam = Camera::perspective_default();
        let mut r = Reader::new(&body);
        read_tokens(&mut r, &mut cam, Variant::Converter);
        assert_eq!(cam.position_constraint, None);
    }

    #[test]
    fn unknown_tokens_and_the_end_of_the_file() {
        // token 0x0263 carries two bytes; then NEAR; then the file ends without a zero token
        let body = [2, 0x63, 9, 9, 0, 7, 0, 1, 0x40, 0, 0, 0];
        let mut cam = Camera::perspective_default();
        let mut r = Reader::new(&body);
        assert_eq!(read_tokens(&mut r, &mut cam, Variant::Program), End::Eof);
        assert_eq!(cam.near, 2.0);
        // an empty stream ends at once if the token variable is zero, as after any complete read
        let mut r = Reader::new(&[]);
        assert_eq!(read_tokens(&mut r, &mut cam, Variant::Program), End::Zero);
        // old and new token for the projection plane
        let mut w = Writer::new();
        write_tokens(&mut w, &sample());
        let at = w.out.windows(2).position(|x| x == [0x16, 0x14]).unwrap();
        w.out[at] = 0;
        let mut got = Camera::perspective_default();
        assert_eq!(read_tokens(&mut Reader::new(&w.out), &mut got, Variant::Program), End::Zero);
        assert_eq!(got, sample());
    }

    #[test]
    fn user_data() {
        let mut cam = sample();
        cam.user_data = vec![UserNode { name: b"tag".to_vec(), kind: 1, data: vec![1, 2, 3] }];
        let mut w = Writer::new();
        write_tokens(&mut w, &cam);
        let mut got = Camera::perspective_default();
        let mut r = Reader::new(&w.out);
        r.st.file_version = 4.0;
        assert_eq!(read_tokens(&mut r, &mut got, Variant::Program), End::Zero);
        assert_eq!(got, cam);
        assert_eq!(r.s.tell(), w.out.len());
    }
}
