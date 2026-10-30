// SPDX-License-Identifier: MIT
//! The readers of materials, textures and user data, and of the four kinds of file
//! (source files `b_matio.c`, `binary.c`).

use std::collections::BTreeMap;

use si_file::{read_bin_header, BinHeader};

use crate::io::{c_str, ok, Rd};
use crate::model::*;
use crate::proto::{self, Prototypes};

const PROT: i32 = i32::from_be_bytes(*b"PROT");
const HRCH: i32 = i32::from_be_bytes(*b"HRCH");
const MTRL: i32 = i32::from_be_bytes(*b"MTRL");
const TXMP: i32 = i32::from_be_bytes(*b"TXMP");
const TX3D: i32 = i32::from_be_bytes(*b"TX3D");

/// What a read met on its way that a caller may want to know.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notes {
    /// (prototype, token) the reader does not know and passed over by the token's length.
    pub skipped: BTreeMap<(usize, u16), u32>,
    /// (prototype, token) the reader does not know and could not skip (no length in the
    /// token): it reads nothing for them and goes on.
    pub unknown_plain: BTreeMap<(usize, u16), u32>,
    /// Lists that were cut short because the file ended, or whose count the rest of the
    /// file could not hold.
    pub cut: u32,
    /// Hierarchy nodes read.
    pub nodes: u32,
    /// Places where the reader, as the original does, kept something other than the stored
    /// value (or dropped it), by a short description. A file with none of these and only
    /// tokens the writers emit is written back byte for byte.
    pub changed: BTreeMap<&'static str, u32>,
}

/// The three versions the readers consult, globals of the original that outlive a file.
///
/// Which one a reader looks at is fixed by the source file it is in, not by the file being
/// read (finding 0004, "What the listing and the emulator settled", point 2):
/// `si_texture3DBinaryRead` looks at `b_matio.c`'s copy, which only the three material and
/// texture *file* readers set, so a 3D texture inside a model is judged by the version of
/// the last material or texture file read (0 in a fresh program); `si_userDataRead` looks
/// at `binary.c`'s copy, which only the model file reader sets, also inside a material or
/// texture file; `si_udfRead` looks at `SI_versionInput`, which every accepted header sets.
/// A caller that reads several files as one program does passes one `Versions` to
/// [`read_file_with`]; [`read_file`] starts from a fresh program's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Versions {
    /// `SI_versionInput` (0x086793f0): the version of the last header the header reader
    /// accepted; 4.0 at start.
    pub input: f32,
    /// `fileVersion` of `b_matio.c` (0x08817f20): the header version (−1 for a refused
    /// header) of the last material or texture file; 0 at start.
    pub matio: f32,
    /// `fileVersion` of `binary.c` (0x08817f28): the header version of the last model
    /// file whose header was accepted; 0 at start.
    pub binary: f32,
}

impl Default for Versions {
    fn default() -> Self {
        Versions { input: 4.0, matio: 0.0, binary: 0.0 }
    }
}

impl Versions {
    /// All three at one version: what reading a lone file of that version sees when the
    /// globals hold it (the unit tests' case).
    pub fn all(v: f32) -> Self {
        Versions { input: v, matio: v, binary: v }
    }
}

/// A file being read: the stream, the versions the readers consult ([`Versions`]) and the
/// prototype table.
///
/// Throughout, a local variable that the original leaves uninitialised until a read fills
/// it starts at zero here.
pub struct Reader<'a> {
    pub r: Rd<'a>,
    pub versions: Versions,
    pub protos: Prototypes,
    pub notes: Notes,
}

/// `version < c` as the original makes it: the float version loaded into the x87 unit and
/// compared with the double constant (`flds`, `fcompl`). The float nearest a constant that
/// rounds down (2.1 is 2.0999999) is below it.
pub(crate) fn below(v: f32, c: f64) -> bool {
    (v as f64) < c
}

/// [soft 0x083fb934 si_dbfileIsOldUdfVersion]: up to 2.67906, and from 2.69 to below
/// 2.999019, a shader reference is stored in an older form. The version is widened and
/// compared with the doubles at 0x0865c5a0, 0x0865c5a8 and 0x0865c5b0.
pub fn is_old_udf_version(v: f32) -> bool {
    let v = v as f64;
    v <= 2.67906 || (2.69 <= v && v < 2.999019)
}

impl<'a> Reader<'a> {
    /// A reader for data of one version, with every version global at it.
    pub fn new(data: &'a [u8], version: f32, protos: Prototypes) -> Self {
        Reader { r: Rd::new(data), versions: Versions::all(version), protos, notes: Notes::default() }
    }

    /// The next token of a record: from the prototype's list or, when the file declares no
    /// list for this prototype, from the file itself, where every field is then preceded
    /// by its token.
    ///
    /// When that inline read fails at the end of the file the original keeps the previous
    /// token and loops on it for ever; here the record ends.
    pub(crate) fn tok(&mut self, id: usize, i: &mut usize) -> i16 {
        match self.protos.token(id, *i) {
            Some(t) => {
                *i += 1;
                t
            }
            None => {
                let mut t = 0i16;
                self.r.i16(&mut t);
                t
            }
        }
    }

    /// A token this reader has no case for [soft 0x083ff174 si_fileSkipField].
    pub(crate) fn skip(&mut self, id: usize, t: i16) {
        let map = if self.r.skip_field(t) { &mut self.notes.skipped } else { &mut self.notes.unknown_plain };
        *map.entry((id, t as u16)).or_insert(0) += 1;
    }

    /// The readers of patches and composite curves skip *every* unknown token as if its
    /// high byte were 0xff: they read a 4-byte length whatever the token says.
    pub(crate) fn skip_as_long(&mut self, id: usize, t: i16) {
        self.r.skip_field(-1);
        *self.notes.skipped.entry((id, t as u16)).or_insert(0) += 1;
    }

    /// How many records of a counted list to try for.
    ///
    /// The original trusts the count: it allocates that many records and reads each, end of
    /// file or not, which leaves default records behind. A count the rest of the file
    /// cannot hold is cut here to the bytes left, and [`Reader::ended`] stops a list at
    /// the end of the file. Both are noted in [`Notes::cut`].
    pub(crate) fn count(&mut self, n: i64) -> usize {
        if n <= 0 {
            return 0;
        }
        let left = self.r.left();
        if n as u128 > left as u128 {
            self.notes.cut += 1;
            left
        } else {
            n as usize
        }
    }

    pub(crate) fn changed(&mut self, what: &'static str) {
        *self.notes.changed.entry(what).or_insert(0) += 1;
    }

    pub(crate) fn ended(&mut self) -> bool {
        let e = self.r.s.feof();
        if e {
            self.notes.cut += 1;
        }
        e
    }

    /// A list of user-data blocks [soft 0x083ec910 si_userDataRead]: a count, then per block
    /// the tag's length, the tag, a flag, the data's size and the data.
    ///
    /// In a file below 3.82099 the data is read as one item of `size` bytes, which makes
    /// the byte-swapping read reverse it when the size is 2, 4 or 8.
    pub fn user_data(&mut self) -> Vec<UserNode> {
        let mut n = 0i32;
        self.r.i32(&mut n);
        let mut list = Vec::new();
        let mut taglen = 0i32;
        for _ in 0..self.count(n as i64) {
            if self.ended() {
                break;
            }
            let mut node = UserNode::default();
            self.r.i32(&mut taglen);
            node.tag = self.r.blob(taglen);
            self.r.i16(&mut node.flag);
            self.r.i32(&mut node.size);
            node.data = self.data_block(node.size, below(self.versions.binary, 3.82099));
            list.push(node);
        }
        list
    }

    /// `size` bytes of data; `as_one_item` reads them as a single item, which the swapping
    /// read reverses when it is 2, 4 or 8 bytes long and complete.
    pub(crate) fn data_block(&mut self, size: i32, as_one_item: bool) -> Vec<u8> {
        let complete = size >= 0 && size as usize <= self.r.left();
        let mut data = self.r.blob(size);
        if as_one_item && complete && matches!(size, 2 | 4 | 8) {
            data.reverse();
            self.changed("user data of 2, 4 or 8 bytes reversed (old file)");
        }
        data
    }

    /// A shader reference [soft 0x083e9154 si_udfRead].
    ///
    /// Tokens: 1 name, 2 active, 3 internal, 4 order (three 4-byte values). Token 5 ends the
    /// record at once. Without a prototype the tokens are taken to be 1 to 5 in order (they
    /// are *not* read from the file, unlike every other record). An empty name, or the name
    /// `NULL`, gives no reference at all, though the remaining fields are still read.
    pub fn udf(&mut self) -> Option<Udf> {
        if is_old_udf_version(self.versions.input) {
            self.old_udf();
            return None;
        }
        let mut udf = Some(Udf::default());
        let listed = self.protos.get(proto::UDF).is_some();
        let mut i = 0usize;
        let mut t = if listed { self.tok(proto::UDF, &mut i) } else { 1 };
        let mut value = 0i32;
        while t != 0 {
            match t {
                1 => {
                    let s = self.r.string();
                    match &mut udf {
                        Some(u) if !s.is_empty() => u.name = s,
                        _ => udf = None,
                    }
                }
                2 => {
                    self.r.i32(&mut value);
                    if let Some(u) = &mut udf {
                        u.active = value as i16;
                    }
                }
                3 => {
                    self.r.i32(&mut value);
                    if let Some(u) = &mut udf {
                        u.internal = value as i16;
                    }
                }
                4 => {
                    self.r.i32(&mut value);
                    if let Some(u) = &mut udf {
                        u.order = value;
                    }
                }
                5 => return udf,
                _ => self.skip(proto::UDF, t),
            }
            t = if listed { self.tok(proto::UDF, &mut i) } else if t == 5 { 0 } else { t + 1 };
        }
        udf.filter(|u| u.name != b"NULL")
    }

    /// The older form of a shader reference, read and thrown away [soft 0x083e9374 readOldUDF]:
    /// tokens 1, 2, 3 are strings, 4 a short, 5 a 4-byte value; a token 6 ends the record.
    fn old_udf(&mut self) {
        let listed = self.protos.get(proto::UDF).is_some();
        let mut i = 0usize;
        let mut t = if listed { self.tok(proto::UDF, &mut i) } else { 1 };
        while t != 0 {
            match t {
                1..=3 => {
                    self.r.string();
                }
                4 => {
                    self.r.i16(&mut 0);
                }
                5 => {
                    self.r.i32(&mut 0);
                }
                _ => self.skip(proto::UDF, t),
            }
            t = if listed { self.tok(proto::UDF, &mut i) } else { t + 1 };
            if t == 6 {
                t = 0;
            }
        }
    }

    /// A 2D texture [soft 0x083e6b90 si_textureBinaryRead]. (The original fills a texture on
    /// its stack and returns a heap copy of it.)
    pub fn texture(&mut self) -> Texture2D {
        let mut x = Texture2D::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::TEXTURE, &mut i);
        let mut tmp = 0i16; // a scratch short several tokens are read through
        while t != 0 {
            let r = &mut self.r;
            match t as u16 {
                1 => x.name = r.string(),
                2 => {
                    r.i16(&mut tmp);
                    x.anim = tmp;
                }
                3 => ok(r.i16(&mut x.urepeat)),
                4 => ok(r.i16(&mut x.vrepeat)),
                5 => ok(r.f32(&mut x.uscale)),
                6 => ok(r.f32(&mut x.vscale)),
                7 => ok(r.f32(&mut x.uoffset)),
                8 => ok(r.f32(&mut x.voffset)),
                9 => {
                    r.i16(&mut tmp);
                    x.pixel_interp = tmp;
                }
                10 => {
                    r.i16(&mut tmp);
                    x.method = tmp;
                }
                0xb => ok(r.f32(&mut x.ambient)),
                0xc => ok(r.f32(&mut x.diffuse)),
                0xd => ok(r.f32(&mut x.specular)),
                0xe => ok(r.f32(&mut x.roughness)),
                0xf => ok(r.f32(&mut x.transparency)),
                0x10 => ok(r.f32(&mut x.reflection_map)),
                0x11 => ok(r.f32(&mut x.old_factor)),
                0x12 => ok(r.f32(&mut x.env_rotation)),
                0x13 => {
                    x.blend = 1.0;
                    r.i16(&mut tmp);
                    x.mask = tmp;
                }
                0x14 => ok(r.f32(&mut x.blend)),
                0x15 => {
                    r.i16(&mut tmp);
                    x.crop_flag = tmp;
                }
                // each of these four reads two shorts, into overlapping pairs
                0x16 => crop_pair(r, &mut x.crop, 0),
                0x17 => crop_pair(r, &mut x.crop, 2),
                0x18 => crop_pair(r, &mut x.crop, 1),
                0x19 => crop_pair(r, &mut x.crop, 3),
                0x1a => {
                    r.i16(&mut tmp);
                    x.swap_uv = tmp;
                }
                0x1b => {
                    r.i16(&mut tmp);
                    x.alt_u = tmp;
                }
                0x1c => {
                    r.i16(&mut tmp);
                    x.alt_v = tmp;
                }
                0x1d => ok(r.f32(&mut x.reflectivity)),
                0x1e => {
                    r.i16(&mut tmp);
                    x.mask = tmp;
                    if tmp == 4 {
                        self.changed("texture mask 4 (token 0x1e writes it as 3)");
                    }
                }
                0x1f => {
                    r.i16(&mut tmp);
                    x.channel = tmp;
                }
                0x20 => ok(r.floats(&mut x.rotation)),
                0x21 => ok(r.floats(&mut x.scaling)),
                0x22 => ok(r.floats(&mut x.translation)),
                0x23 => ok(r.i16(&mut x.black_white)),
                0x24 => x.udf = self.udf(),
                0x25 => ok(r.i16(&mut x.displacement)),
                0x32 => x.filename = r.string(),
                0x233 => ok(r.i16(&mut x.hw_flag)),
                0x234 => ok(r.i16(&mut x.hw_tiling)),
                0x235 => ok(r.i16(&mut x.x1cc)),
                0x236 => ok(r.i16(&mut x.x1ce)),
                0x237 => ok(r.i16(&mut x.x1d4)),
                0x238 => ok(r.i16(&mut x.mask)),
                0xff35 => {
                    r.i32(&mut 0); // the token's length, not used
                    x.user_data = self.user_data();
                }
                _ => self.skip(proto::TEXTURE, t),
            }
            t = self.tok(proto::TEXTURE, &mut i);
        }
        // The reader fills a working copy on its stack and returns a new texture made from
        // it by `textureTransfer` (0x082ad330), which copies the cropping shorts +0x30 to
        // +0x36 and not the short at +0x38 that token 0x19 also read: the new texture has
        // 0 there (finding 0004, image pass, point 3).
        if x.crop[4] != 0 {
            self.changed("texture: fifth cropping short read and dropped");
        }
        x.crop[4] = 0;
        drop_internal_udf(self, &mut x.udf);
        // mapping methods 7 and 8 take no shading factors
        if (x.method.wrapping_sub(7) as u16) < 2 {
            if [x.blend, x.ambient, x.transparency, x.diffuse, x.reflectivity, x.specular, x.roughness].iter().any(|v| v.to_bits() != 0) {
                self.changed("texture factors cleared (mapping method 7 or 8)");
            }
            x.blend = 0.0;
            x.ambient = 0.0;
            x.transparency = 0.0;
            x.diffuse = 0.0;
            x.reflectivity = 0.0;
            x.specular = 0.0;
            x.roughness = 0.0;
        }
        x
    }

    /// A 3D texture [soft 0x083e77c8 si_texture3DBinaryRead].
    ///
    /// Tokens 7, 8, 9 are the three colours of the oldest form; when token 7 was present the
    /// five-colour ramp is built from them at the end: colours 2 and 4 are the means of
    /// their neighbours, and the three factors derive from the single old one.
    ///
    /// Below `b_matio.c`'s version 2.1 (the double: a file of version 2.1 is below it) the
    /// channel is 2 before the tokens and again after them, whatever token 0x21 stored.
    /// Inside a model that version is the last material or texture file's ([`Versions`]).
    pub fn texture3d(&mut self) -> Texture3D {
        let mut x = Texture3D::default();
        let old = below(self.versions.matio, 2.1);
        if old {
            x.channel = 2;
        }
        let mut old_colours = false;
        let mut i = 0usize;
        let mut t = self.tok(proto::TEXTURE_3D, &mut i);
        let mut tmp = 0i16;
        while t != 0 {
            let r = &mut self.r;
            match t as u16 {
                1 => x.name = r.string(),
                2 => {
                    r.i16(&mut tmp);
                    x.kind = tmp as i32;
                }
                3 => {
                    r.i16(&mut tmp);
                    x.iterations = tmp as i32;
                }
                4 => ok(r.f32(&mut x.strength)),
                5 => ok(r.f32(&mut x.spacing)),
                6 | 0x14 => ok(r.f32(&mut 0.0)), // read and dropped
                7 | 8 | 9 => {
                    let c = &mut x.colours[2 * (t as usize - 7)];
                    let mut rgb = [c[0], c[1], c[2]];
                    r.floats(&mut rgb);
                    *c = [rgb[0], rgb[1], rgb[2], 0.0];
                    old_colours |= t == 7;
                }
                10 => ok(r.f32(&mut x.factors[0])),
                0xb => ok(r.floats(&mut x.scaling)),
                0xc => ok(r.floats(&mut x.rotation)),
                0xd => ok(r.floats(&mut x.translation)),
                0xe => ok(r.f32(&mut x.ambient)),
                0xf => ok(r.f32(&mut x.diffuse)),
                0x10 => ok(r.f32(&mut x.specular)),
                0x11 => ok(r.f32(&mut x.roughness)),
                0x12 => ok(r.f32(&mut x.transparency)),
                0x13 => ok(r.f32(&mut x.reflection)),
                0x15 => ok(r.f32(&mut x.power)),
                0x16..=0x1a => ok(r.floats(&mut x.colours[t as usize - 0x16])),
                0x1b => ok(r.f32(&mut x.factors[1])),
                0x1c => ok(r.f32(&mut x.factors[2])),
                0x1d => ok(r.f32(&mut x.taper)),
                0x1e => {
                    x.blending = 1.0;
                    r.i16(&mut tmp);
                    x.mask = if tmp == 1 { 1 } else { 3 };
                }
                0x1f => ok(r.f32(&mut x.blending)),
                0x20 => ok(r.i16(&mut x.mask)),
                0x21 => ok(r.i16(&mut x.channel)),
                0x22 => x.udf = self.udf(),
                0x23 => ok(r.i16(&mut x.displacement)),
                0xff24 => {
                    r.i32(&mut 0);
                    x.user_data = self.user_data();
                }
                _ => self.skip(proto::TEXTURE_3D, t),
            }
            t = self.tok(proto::TEXTURE_3D, &mut i);
        }
        if old_colours {
            // x87 arithmetic in the original: each sum and product is formed in extended
            // precision and rounded once when stored; doubles give the same result here
            let half = 0.5f64;
            let mean = |a: f32, b: f32| ((a as f64 + b as f64) * half) as f32;
            let c = x.colours;
            x.colours[1] = [mean(c[0][0], c[2][0]), mean(c[0][1], c[2][1]), mean(c[0][2], c[2][2]), 0.0];
            x.colours[3] = [mean(c[2][0], c[4][0]), mean(c[2][1], c[4][1]), mean(c[2][2], c[4][2]), 0.0];
            let f = x.factors[0];
            x.factors[1] = f;
            x.factors[2] = (f as f64 + (1.0 - f as f64) * half) as f32;
            x.factors[0] = (f as f64 * half) as f32;
        }
        if old {
            if x.channel != 2 {
                self.changed("3D texture channel set to 2 (b_matio.c's version below 2.1)");
            }
            x.channel = 2;
        }
        drop_internal_udf(self, &mut x.udf);
        x
    }

    /// A material [soft 0x083e7f90 si_materialBinaryRead]. (The original also links the new
    /// material into a global list of the current process; that is not part of the file.)
    ///
    /// Three tokens set the shading model: 2, and after it 0x218 and 0x21b, which later
    /// versions added so that older programs keep reading token 2. A stored 0x218 of 7
    /// becomes model 1 with both extra shorts set to 2.
    pub fn material(&mut self) -> Material {
        let mut m = Material::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::MATERIAL, &mut i);
        let (mut count, mut external) = (0i16, 0i16);
        while t != 0 {
            let r = &mut self.r;
            match t as u16 {
                1 => m.name = r.string(),
                2 | 0x21b => ok(r.i16(&mut m.shading)),
                3 => ok(r.floats(&mut m.ambient)),
                4 => ok(r.floats(&mut m.diffuse)),
                5 => ok(r.floats(&mut m.specular)),
                6 => ok(r.f32(&mut m.exponent)),
                7 => ok(r.f32(&mut m.transparency)),
                8 => ok(r.f32(&mut m.x6bc)),
                9 => {
                    // the oldest form stores a reflection colour: its mean is kept. (Extended
                    // precision in the original, rounded once on storing; computed in doubles
                    // here, which can differ in the last bit in rare cases.)
                    let mut c = [0f32; 3];
                    r.floats(&mut c);
                    m.reflectivity = ((c[0] as f64 + c[1] as f64 + c[2] as f64) / 3.0) as f32;
                }
                10 => ok(r.f32(&mut m.x6ac)),
                0xb => ok(r.f32(&mut m.refraction)),
                0xd => ok(r.f32(&mut m.reflectivity)),
                0xe => ok(r.i16(&mut m.glow)),
                0xf => ok(r.f32(&mut m.glow_amount)),
                0x10 => {
                    r.i16(&mut count);
                    for _ in 0..self.count(count as i64) {
                        if self.ended() {
                            break;
                        }
                        self.r.i16(&mut external);
                        if external == 0 {
                            let x = self.texture();
                            m.textures.push(x);
                        } else {
                            self.r.string(); // an external texture's name: read and dropped
                            self.changed("external texture or material name dropped");
                        }
                    }
                }
                0x11 => {
                    r.i16(&mut count);
                    for _ in 0..self.count(count as i64) {
                        if self.ended() {
                            break;
                        }
                        self.r.i16(&mut external);
                        if external == 0 {
                            let x = self.texture3d();
                            if m.texture3d.is_none() {
                                m.texture3d = Some(x); // only the first is kept
                            } else {
                                self.changed("3D texture after the first dropped");
                            }
                        } else {
                            self.r.string();
                            self.changed("external texture or material name dropped");
                        }
                    }
                }
                0x12 => ok(r.i16(&mut m.blur)),
                0x13 => ok(r.f32(&mut m.blur_width)),
                0x14 => ok(r.f32(&mut m.blur_decay)),
                0x15 => m.udf_material = self.udf(),
                0x16 => m.udf_volume = self.udf(),
                0x17 => m.udf_shadow = self.udf(),
                0x218 => {
                    r.i16(&mut m.shading);
                    if m.shading == 7 {
                        m.shading = 1;
                        m.xdc8 = 2;
                        m.xdca = 2;
                    }
                }
                0x21c => ok(r.i16(&mut m.xdc8)),
                0x21d => ok(r.i16(&mut m.xdca)),
                0xff19 => {
                    r.i32(&mut 0);
                    m.user_data = self.user_data();
                }
                0xff1a => {
                    r.i32(&mut 0);
                    m.udf_extra = self.udf();
                }
                _ => self.skip(proto::MATERIAL, t),
            }
            t = self.tok(proto::MATERIAL, &mut i);
        }
        m
    }
}

/// The texture readers return a copy of their working texture (`textureTransfer`,
/// `texture3DTransfer`), whose shader reference is copied by `SI_udfCopy` (0x0854c70c):
/// an internal reference (token 3 of 1) is not copied, and the texture has none.
fn drop_internal_udf(r: &mut Reader, udf: &mut Option<Udf>) {
    if udf.as_ref().is_some_and(|u| u.internal == 1) {
        r.changed("texture: internal shader reference not copied");
        *udf = None;
    }
}

fn crop_pair(r: &mut Rd, crop: &mut [i16; 5], at: usize) {
    let (mut a, mut b) = (crop[at], crop[at + 1]);
    r.i16(&mut a);
    r.i16(&mut b);
    crop[at] = a;
    crop[at + 1] = b;
}

/// What one of the four files holds after its header and prototype block.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// `HRCH`: the root node of a model.
    Model(Box<Node>),
    /// `MTRL`
    Material(Material),
    /// `TXMP`
    Texture(Texture2D),
    /// `TX3D`
    Texture3D(Texture3D),
    /// The tag after the prototype block is not the one the reader wants: nothing is read.
    None,
}

#[derive(Clone, Debug)]
pub struct File {
    /// `None` when the header was refused (possible only for the material and texture
    /// readers, which do not check it; the version is then −1).
    pub header: Option<BinHeader>,
    /// Whether the file had a `PROT` block.
    pub has_prototypes: bool,
    pub protos: Prototypes,
    pub content: Content,
    /// Offset of the first byte the reader did not consume.
    pub end: usize,
    pub notes: Notes,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Model,
    Material,
    Texture,
    Texture3D,
}

impl Kind {
    pub fn tag(self) -> &'static [u8; 4] {
        match self {
            Kind::Model => b"HRCH",
            Kind::Material => b"MTRL",
            Kind::Texture => b"TXMP",
            Kind::Texture3D => b"TX3D",
        }
    }
}

/// Reads a model, material, 2D texture or 3D texture file
/// [soft 0x083ec740 SI_fdmodelBinaryRead] [soft 0x083e8b14 SI_materialReadBinaryFile]
/// [soft 0x083e8d30 SI_textureReadBinaryFile] [soft 0x083e8f50 SI_texture3DReadBinaryFile].
/// (`SI_omodelBinaryRead`, 0x083ec5a4, opens `<name>.hrc`, calls the first of these and
/// records the file's name in the root node; that last step is not reproduced.)
///
/// All four have the same shape: the header, a 4-byte tag, the prototype block if the tag
/// is `PROT` followed by another tag, and then the record if that tag is the expected one.
/// The model reader gives up when the header is refused; the other three do not look at
/// the header reader's result and go on with a version of −1. None of them checks the
/// magic against the kind of file, which is why materials and 3D textures under the
/// general magic read as well as those under their own.
///
/// Not reproduced: after a model the original rebuilds old envelopes
/// (`SI_envelopRebuild`), and the model reader's second mode, which re-reads a file over
/// an existing hierarchy and discards what it reads.
pub fn read_file(data: &[u8], kind: Kind) -> Option<File> {
    read_file_with(data, kind, &mut Versions::default())
}

/// [`read_file`] in a program that has read other files before: `versions` holds the
/// version globals as the earlier reads left them and is updated as the original updates
/// them (the header reader sets `input` when it accepts a header; a material or texture
/// file sets `matio` to its version, −1 for a refused header; a model file with an
/// accepted header sets `binary`).
pub fn read_file_with(data: &[u8], kind: Kind, versions: &mut Versions) -> Option<File> {
    let mut r = Rd::new(data);
    let header = read_bin_header(&mut r.s);
    if let Some(h) = &header {
        versions.input = h.version;
    }
    if header.is_none() && kind == Kind::Model {
        return None;
    }
    let version = header.as_ref().map_or(-1.0, |h| h.version);
    if kind == Kind::Model {
        versions.binary = version;
    } else {
        versions.matio = version;
    }
    let mut tag = 0i32;
    r.i32(&mut tag);
    let has_prototypes = tag == PROT;
    let mut protos = Prototypes::default();
    if has_prototypes {
        protos = Prototypes::read(&mut r);
        r.i32(&mut tag);
    }
    let mut rd = Reader { r, versions: *versions, protos, notes: Notes::default() };
    let content = match kind {
        Kind::Model if tag == HRCH => Content::Model(rd.node(None)),
        Kind::Material if tag == MTRL => Content::Material(rd.material()),
        Kind::Texture if tag == TXMP => Content::Texture(rd.texture()),
        Kind::Texture3D if tag == TX3D => Content::Texture3D(rd.texture3d()),
        _ => Content::None,
    };
    Some(File { header, has_prototypes, protos: rd.protos, content, end: rd.r.s.tell(), notes: rd.notes })
}

/// The tag of an old-style user node that marks custom-effect data.
pub(crate) fn is_custom_tag(tag: &[u8]) -> bool {
    matches!(c_str(tag), b"CUSA" | b"CUSB" | b"CUSC" | b"CUSD")
}
