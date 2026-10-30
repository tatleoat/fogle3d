// SPDX-License-Identifier: MIT
//! Materials and textures before a frame: the original's `materials.c` (finding 0012,
//! section 13). `SI_materials` turns every material record into what the shading routine
//! reads (the working block, the flag word, the count of 2D maps, the tables) and gives
//! every 2D map its picture; `SI_textures` loads the pictures and works out each map's
//! region and scale; `updateDefaultMaterial` does the first for the material a node
//! without one is rendered with.
//!
//! `decayTable`, `unirand` and `si_initJitter` of the same source file are in
//! [`crate::jitter`] (first pass).
//!
//! Records are the plain structures of [`crate::shade`]; every member names its offset in
//! the original record. The element dictionary's chapters come in as slices of
//! [`DictElem`]: what the original reads of an element is its data pointer (+0x04) and the
//! word at +0x48 that marks an element not in use.
//!
//! Three routines of other source files that `SI_materials` calls are ported here because
//! nothing else has them: `SI_global2dMapInsert`, `SI_global2dMapRemove` (`t2d_dial.c`)
//! and `SI_getMapsList` (`mat_global.c`).

use crate::jitter::{decay_table, init_jitter};
use crate::shade::{ShadeMaterial, ShadeState};
use si_x87::{x, F80, Round};

/// 0x08654198, 0x086541d8, 0x086541f0, 0x08654218 (double): the three components of the
/// ambience are averaged.
const THREE: f64 = 3.0;
/// 0x086541a0, 0x086541f8 (double): a reflectivity or transparency above this counts.
const SOME: f64 = 0.01;
/// 0x086541a8, 0x086541e8, 0x08654200 (double): a jitter amplitude above this gets a table.
const JITTER_MIN: f64 = 0.1;
/// 0x086541b0, 0x086541e0, 0x08654208, 0x08654220 (double, bits 0x3f701083dbc23316): the
/// factor from a byte to a unit value: 0.003922, not 1/255.
pub const BYTE_UNIT: f64 = 0.003922;
/// 0x08654210 (double): the slack of the "region wraps" test of `SI_processTextures`.
const WRAP_EPS: f64 = 1e-6;

/// Flag bits of a material's word at +0x6e0 and a model's at +0x214 as `SI_materials`
/// sets them (the readers' view is [`crate::shade::flag`]).
pub mod mflag {
    pub const SMOOTH: u16 = 0x0002;
    pub const TEXTURED: u16 = 0x0004;
    /// Reflects, and the reflection map is of method 8 (no ray is traced).
    pub const REFLECT_MAP_ONLY: u16 = 0x0010;
    pub const REFLECT_TRACE: u16 = 0x0020;
    pub const STATIC_BLUR: u16 = 0x0040;
    pub const TRANSPARENT: u16 = 0x0100;
    pub const REFRACTS: u16 = 0x0200;
    /// A texture drives the diffuse colour or the transparency: `si_sTrace` runs the
    /// texture stage for a shadow ray that passes the surface.
    pub const TEXTURED_SHADOW: u16 = 0x0400;
    pub const W_DEC: u16 = 0x4000;
}

/// `glb_map` (0x28 bytes, `SI_calloc(1, 0x28)` in `SI_global2dMapInsert`): one picture as
/// the 2D maps share it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlbMap {
    /// +0x00: the picture's name (a copy of the map's +0x20).
    pub name: Vec<u8>,
    /// +0x04: copies of the map's crop switch (+0x2e) and, +0x06, +0x08, +0x0a, +0x0c, of
    /// its four bounds (+0x30 first column, +0x32 last column, +0x34 first row, +0x36 last
    /// row).
    pub crop: i16,
    pub crop_x0: i16,
    pub crop_x1: i16,
    pub crop_y0: i16,
    pub crop_y1: i16,
    /// +0x14: how many maps use the record.
    pub users: i16,
    /// +0x16, +0x18: width and height in pixels (of the cropped picture once cropped).
    pub width: i16,
    pub height: i16,
    /// +0x1c: the pixels, or none before the picture is read.
    pub pixels: Option<TexPixels>,
    /// +0x24: the next record of the picture list.
    pub next: Option<usize>,
    /// The record was freed by `SI_global2dMapRemove` (the slot stays so that indices hold).
    pub freed: bool,
}

/// A picture as the look-up reads it: five bytes a pixel (red, green, blue, alpha,
/// intensity) in a block of (width + 3) × (height + 3) pixels: one row above the picture
/// (a copy of its last row), two below (copies of its first two), one column before each
/// row (the row's last pixel) and two after (its first two). The original's pointer at
/// `glb_map` +0x1c points at the picture's first pixel, (width + 4) pixels into the block,
/// so that a pixel index of −1 … width + 1 and a row of −1 … height + 1 need no test.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TexPixels {
    /// The block, its length rounded up to a multiple of four bytes as the original asks
    /// for it.
    pub block: Vec<u8>,
    /// Pixels from the start of the block to the picture's first pixel: width + 4.
    pub origin: usize,
}

impl TexPixels {
    /// The pixel `index` pixels after the picture's first (the original's
    /// `pointer + index × 5`). `None` where the original would read outside its block.
    pub fn at(&self, index: i32) -> Option<[u8; 5]> {
        let at = (index as i64 + self.origin as i64).checked_mul(5)?;
        if at < 0 {
            return None;
        }
        let at = at as usize;
        self.block.get(at..at + 5).map(|b| [b[0], b[1], b[2], b[3], b[4]])
    }
}

/// What the original reads of an element of a dictionary chapter (0x84 bytes): +0x04 its
/// data pointer, +0x48 a word that is not 0 for an element not in use, +0x5b a byte the
/// preview's texture marking sets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DictElem {
    pub data: Option<usize>,
    pub unused: i16,
    pub dirty: u8,
}

/// What `SI_materials` and `SI_processTextures` call outside `materials.c`.
pub trait MaterialsEnv {
    /// `drand48()` for `si_initJitter`.
    fn drand48(&mut self) -> f64;
    /// `pow(base, exponent)` of the C library for `decayTable`, with whether it set `errno`
    /// to a domain or range error.
    fn pow(&self, base: f64, exponent: f64) -> (F80, bool) {
        let v = si_x87::trig::pow(base, exponent);
        let d = v.to_f64();
        (v, !d.is_finite() || (d == 0.0 && base != 0.0))
    }
    /// `SI_setFileName(map, frame)` (0x082b0200, `t2d_dial.c`): the name of the map's picture
    /// for a frame, stored at the map's +0x20. The default has the routine's two cases that
    /// need no file: one picture (+0x28 = 0): the path at +0x14; a numbered sequence (+0x28
    /// = 1): the path up to its last `.` (the whole path when it has none), then `.` and the
    /// frame number in decimal. A script of names (+0x28 = 2) is a text file whose line
    /// `frame` names the picture: not ported; no name here.
    fn set_file_name(&mut self, st: &mut ShadeState, map: usize, frame: i16) {
        let m = &mut st.maps[map];
        m.file = match m.anim {
            0 => Some(m.path.clone()),
            1 => {
                let stem = m.path.iter().rposition(|b| *b == b'.').map_or(&m.path[..], |dot| &m.path[..dot]);
                Some([stem, format!(".{frame}").as_bytes()].concat())
            }
            _ => None,
        };
    }
    /// `SI_elemdictAddPicture(dictionary, name, record, 0)` and
    /// `SI_elemdictPictureElemRemove(dictionary, record, name)`: the PICTURES chapter of the
    /// element dictionary follows the picture list.
    fn dict_add_picture(&mut self, _st: &ShadeState, _picture: usize) {}
    fn dict_remove_picture(&mut self, _st: &ShadeState, _picture: usize) {}
    /// `SI_global2dMapInsert(map, frame)` and `SI_global2dMapRemove(&map->glb)` as
    /// `SI_materials` and `setTxtFlag` call them. The defaults are the ports,
    /// [`global_2d_map_insert`] and [`global_2d_map_remove`].
    fn map_insert(&mut self, st: &mut ShadeState, map: usize, frame: i16) -> bool
    where
        Self: Sized,
    {
        global_2d_map_insert(st, map, frame, self)
    }
    fn map_remove(&mut self, st: &mut ShadeState, map: usize)
    where
        Self: Sized,
    {
        global_2d_map_remove(st, map, self)
    }
    /// `SI_readTextureFile(picture, count)` as `SI_processTextures` calls it. The default is
    /// the port, [`read_texture_file`].
    fn read_texture(&mut self, st: &mut ShadeState, picture: usize, count: i16) -> Result<(), SetupError>
    where
        Self: Sized,
    {
        read_texture_file(st, picture, count, self)
    }
    /// `si_initJitter(count, jitter)` for the material's two jitter records (`which` 0: the
    /// one at +0x6ac, 1: at +0x6bc). The default is the port, [`init_jitter`], with
    /// [`MaterialsEnv::sampling_level`] and [`MaterialsEnv::drand48`].
    fn init_jitter(&mut self, count: i32, jitter: &mut crate::jitter::Jitter, _material: usize, _which: usize) {
        let level = self.sampling_level();
        init_jitter(count, jitter, level, &mut || self.drand48());
    }
    /// The bytes of the picture file `<name>.pic` (an absolute name, or under
    /// `SI_texturePath`), or of `<SI_resdir>/noIcon.pic` when `default` is set; `None` when
    /// `SI_pictureOpenFile` fails.
    fn picture_file(&mut self, _name: &[u8], _default: bool) -> Option<Vec<u8>> {
        None
    }
    /// `SI_statusDisplayMessage(text, 3)`.
    fn status(&mut self, _text: &str) {}
    /// `si_initJitter`'s sampling level (the short at 0x08790d38).
    fn sampling_level(&self) -> i16 {
        1
    }
}

/// `si_fatal("Invalid 2D texture file name: ", <the map's path>, 2)`: what `SI_materials`
/// and `setTxtFlag` end the render with when a map has no file name.
pub fn msg_no_file() -> &'static str {
    si_rt::text(0x0869_be40)
}
/// `si_fatal` texts of `SI_readTextureFile`.
pub fn msg_no_default() -> &'static str {
    si_rt::text(0x0869_bf20)
}
pub fn msg_no_memory() -> &'static str {
    si_rt::text(0x0869_bf60)
}

/// Why set-up stopped: a fatal text of the original (it leaves by `longjmp`), or a read
/// through a null pointer.
#[derive(Clone, Debug, PartialEq)]
pub enum SetupError {
    Fatal(String),
    /// The original reads or writes through a null pointer here (named).
    Null(&'static str),
}

// --------------------------------------------------------------------------- the picture list

/// The head of the picture list [soft 0x0829252c SI_getMapsList]: the word at
/// `SI_grafLists` + 0x1c + 0x24 × the current process's number. Class: copy.
pub fn get_maps_list(st: &ShadeState) -> Option<usize> {
    st.picture_head
}

/// Gives a 2D map its picture record [soft 0x082ae114 SI_global2dMapInsert]:
/// `SI_global2dMapInsert(map, frame)`, 1 when the map has one afterwards.
///
/// `SI_setFileName(map, frame)` first; no name (+0x20 null): 0. The picture list is
/// searched for a record of that name which is either uncropped like the map (the map's
/// +0x2e and the record's +0x04 both 0) or cropped to the same four bounds; such a record
/// gets one more user. Otherwise a new record is made (name, crop switch and bounds copied,
/// one user, no pixels), entered in the dictionary and put at the head of the list. The
/// map's +0x24 is the record. Class: integer.
pub fn global_2d_map_insert(st: &mut ShadeState, map: usize, frame: i16, env: &mut dyn MaterialsEnv) -> bool {
    env.set_file_name(st, map, frame);
    let m = st.maps[map].clone();
    let Some(name) = m.file else { return false };
    let mut cur = st.picture_head;
    while let Some(p) = cur {
        let g = &mut st.pictures[p];
        cur = g.next;
        if g.name != name {
            continue;
        }
        let same = if m.crop == 0 {
            g.crop == 0
        } else {
            g.crop != 0 && m.crop_x0 == g.crop_x0 && m.crop_x1 == g.crop_x1 && m.crop_y0 == g.crop_y0 && m.crop_y1 == g.crop_y1
        };
        if same {
            g.users = g.users.wrapping_add(1);
            st.maps[map].glb = Some(p);
            return true;
        }
    }
    let p = st.pictures.len();
    st.pictures.push(GlbMap {
        name,
        crop: m.crop,
        crop_x0: m.crop_x0,
        crop_x1: m.crop_x1,
        crop_y0: m.crop_y0,
        crop_y1: m.crop_y1,
        users: 1,
        next: st.picture_head,
        ..GlbMap::default()
    });
    env.dict_add_picture(st, p);
    st.picture_head = Some(p);
    st.maps[map].glb = Some(p);
    true
}

/// Takes a map's picture record away [soft 0x082ae294 SI_global2dMapRemove]:
/// `SI_global2dMapRemove(&map->glb)`. Nothing for a null record. The record loses a user;
/// when that was its last it leaves the dictionary and the list, and its pixels, its name
/// and the record itself are freed. The map's pointer is cleared either way. Class:
/// integer.
pub fn global_2d_map_remove(st: &mut ShadeState, map: usize, env: &mut dyn MaterialsEnv) {
    let Some(p) = st.maps[map].glb else { return };
    let before = st.pictures[p].users;
    st.pictures[p].users = before.wrapping_sub(1);
    if before == 1 {
        env.dict_remove_picture(st, p);
        if st.picture_head == Some(p) {
            st.picture_head = st.pictures[p].next;
        } else {
            let mut cur = st.picture_head;
            while let Some(q) = cur {
                if st.pictures[q].next == Some(p) {
                    st.pictures[q].next = st.pictures[p].next;
                    break;
                }
                cur = st.pictures[q].next;
            }
        }
        let g = &mut st.pictures[p];
        g.pixels = None;
        g.name = Vec::new();
        g.freed = true;
    }
    st.maps[map].glb = None;
}

// ------------------------------------------------------------------------------ SI_materials

/// The mean of the scene's ambience as every routine of the file forms it: ((r + g) + b)
/// / 3.0 in the register, rounded to float.
fn mean_ambience(st: &ShadeState) -> f32 {
    let a = st.glb.ambience;
    (((x(a[0]) + a[1]) + a[2]) / THREE).to_f32()
}

/// The working factors of one 2D map (+0x148 … +0x164) from its eight factors: each times
/// the double 0.003922, the first also times the mean ambience; each rounded once.
fn map_factors(st: &mut ShadeState, map: usize, ambience: f32) {
    let m = &mut st.maps[map];
    let k = x(BYTE_UNIT);
    m.work[0] = ((k * m.factors[0]) * ambience).to_f32();
    for i in 1..6 {
        m.work[i] = (k * m.factors[i]).to_f32();
    }
    m.bump_amount = (k * m.factors[6]).to_f32();
    m.env_scale = (k * m.factors[7]).to_f32();
}

/// The working factors of a 3D texture (+0x128 … +0x140): the ambient factor times the
/// mean ambience, the other six copied.
fn tex3d_factors(st: &mut ShadeState, tex: usize, ambience: f32) {
    let t = &mut st.tex3ds[tex];
    t.work[0] = (x(ambience) * t.factors[0]).to_f32();
    for i in 1..7 {
        t.work[i] = t.factors[i];
    }
}

/// The three "what does it need" words of a material or model while its textures are
/// walked: reflects, is transparent, refracts (the original's array of ten shorts, of which
/// entries 3, 4 and 6 are used).
#[derive(Clone, Copy, Default)]
struct Needs {
    reflect: bool,
    transp: bool,
    refract: bool,
}

/// One 2D map of a list, as `SI_materials` and `setTxtFlag` treat it (the two loop bodies
/// are the same instructions): the working factors; an animated map (+0x28 not 0) gives its
/// picture back; a map without a picture gets one, and no file name is fatal; then the
/// flags. Returns whether the map is the material's reflection map (it then does not make
/// the material "textured").
fn one_map<E: MaterialsEnv>(st: &mut ShadeState, map: usize, ambience: f32, flags: &mut u16, needs: &mut Needs, env: &mut E) -> Result<bool, SetupError> {
    map_factors(st, map, ambience);
    if st.maps[map].anim != 0 {
        env.map_remove(st, map);
    }
    if st.maps[map].glb.is_none() {
        let frame = st.glb.frame;
        if !env.map_insert(st, map, frame) {
            return Err(SetupError::Fatal(format!("{}{}", msg_no_file(), String::from_utf8_lossy(&st.maps[map].path))));
        }
    }
    let m = &st.maps[map];
    if m.fx_diffuse != 0 {
        *flags |= mflag::TEXTURED_SHADOW;
    }
    if m.fx_reflect != 0 {
        needs.reflect = true;
    }
    if m.fx_transp != 0 {
        needs.transp = true;
        *flags |= mflag::TEXTURED_SHADOW;
    }
    if m.fx_11e != 0 {
        needs.refract = true;
    }
    if st.glb.textures_2d == 0 || st.glb.reflection == 0 || m.skip == 0 {
        *flags |= mflag::TEXTURED;
        Ok(false)
    } else {
        Ok(true)
    }
}

/// The flags that follow from the three words: reflection (0x10 when 2D textures and
/// reflection are on and the material's reflection map is of method 8, else 0x20) and
/// transparency (0x200 with refraction, else 0x100).
fn need_flags(st: &ShadeState, needs: Needs, reflect_map: Option<usize>, flags: &mut u16) {
    if needs.reflect {
        let map_only = st.glb.textures_2d != 0 && st.glb.reflection != 0 && reflect_map.is_some_and(|m| st.maps[m].method == 8);
        *flags |= if map_only { mflag::REFLECT_MAP_ONLY } else { mflag::REFLECT_TRACE };
    }
    if needs.transp {
        *flags |= if needs.refract { mflag::REFRACTS } else { mflag::TRANSPARENT };
    }
}

/// The part of `SI_materials`, `SI_processMaterials` and `updateDefaultMaterial` that is
/// the same in all three: the working block from the material and the switches, the two
/// decay tables, the two jitter tables.
fn working_block<E: MaterialsEnv>(st: &mut ShadeState, mat: usize, env: &mut E) {
    let g = st.glb.clone();
    let m: &mut ShadeMaterial = &mut st.materials[mat];
    m.work.base_ambient[0] = (x(m.ambient[0]) * g.ambience[0]).to_f32();
    m.work.base_ambient[1] = (x(m.ambient[1]) * g.ambience[1]).to_f32();
    m.work.base_ambient[2] = (x(m.ambient[2]) * g.ambience[2]).to_f32();
    // +0x70c is not written: the fourth ambient float of both copies is what was there
    m.work.ambient = m.work.base_ambient;
    m.work.diffuse = m.diffuse;
    m.work.specular = m.specular;
    m.work.reflectivity = if g.reflection != 0 { m.reflectivity } else { 0.0 };
    m.work.existence = 1.0;
    m.work.opacity = (F80::ONE - m.transparency).to_f32();
    m.work.refraction = if g.refraction != 0 { m.refraction } else { 1.0 };
    if m.model == 4 || m.model == 5 {
        decay_table(&mut m.spec_table, &|b, e| env.pow(b, e));
    }
    if m.static_blur != 0 {
        decay_table(&mut m.blur_table, &|b, e| env.pow(b, e));
    }
    if x(JITTER_MIN).lt(x(m.jitter_6ac.amplitude)) {
        env.init_jitter(0xc9, &mut m.jitter_6ac, mat, 0);
    }
    if x(JITTER_MIN).lt(x(m.jitter_6bc.amplitude)) {
        env.init_jitter(0xdf, &mut m.jitter_6bc, mat, 1);
    }
}

/// Brings the default material up to date [soft 0x0825a62c updateDefaultMaterial]: the
/// working block of `SI_defaultMaterial` (0x0869dde0; here [`ShadeState::default_material`])
/// as for any material, and its flag word: 2, or 0x4002 when the switch at 0x08790dec is
/// set. Its textures are not looked at: the default material is never textured. Class:
/// single (libm in the tables).
pub fn update_default_material<E: MaterialsEnv>(st: &mut ShadeState, env: &mut E) {
    let Some(mat) = st.default_material else { return };
    working_block(st, mat, env);
    st.materials[mat].flags = if st.glb.w_dec != 0 { mflag::W_DEC | mflag::SMOOTH } else { mflag::SMOOTH };
}

/// One list of 2D maps for one material of a model [soft 0x0825a3dc setTxtFlag]:
/// `setTxtFlag(first map, &flags, model, material)`. The flag word starts from 0. Every map
/// of the list (through +0x16c) is treated as in `SI_materials`; a map that is a reflection
/// map while 2D textures and reflection are on becomes **the material's** reflection map
/// (+0x6f4), any other sets flag 4. Then the reflection flag (0x10 when the material's
/// reflection map is of method 8 and both switches are on, else 0x20), the transparency
/// flag (0x200 with refraction, else 0x100), bit 0 of the ray tracing word (0x08790c7c) for
/// 0x20 or 0x200, and 0x4000 when the switch at 0x08790dec is set and 0x100 is not. The
/// model is not read. A null material is written or read through as soon as a reflection
/// map or a reflecting map is met with both switches on. Class: single.
pub fn set_txt_flag<E: MaterialsEnv>(st: &mut ShadeState, maps: Option<usize>, material: Option<usize>, env: &mut E) -> Result<u16, SetupError> {
    let mut flags = 0u16;
    let mut needs = Needs::default();
    let ambience = mean_ambience(st);
    let mut cur = maps;
    while let Some(map) = cur {
        if one_map(st, map, ambience, &mut flags, &mut needs, env)? {
            let mat = material.ok_or(SetupError::Null("setTxtFlag: the material of a reflection map"))?;
            st.materials[mat].reflect_map = Some(map);
        }
        cur = st.maps[map].next;
    }
    let reflect_map = if needs.reflect && st.glb.textures_2d != 0 && st.glb.reflection != 0 {
        st.materials[material.ok_or(SetupError::Null("setTxtFlag: the material's reflection map"))?].reflect_map
    } else {
        None
    };
    need_flags(st, needs, reflect_map, &mut flags);
    if flags & (mflag::REFLECT_TRACE | mflag::REFRACTS) != 0 {
        st.glb.raytrace = true;
    }
    if st.glb.w_dec != 0 && flags & mflag::TRANSPARENT == 0 {
        flags |= mflag::W_DEC;
    }
    Ok(flags)
}

/// Prepares every material and every model for shading [soft 0x08258e40 SI_materials].
/// `materials` and `models` are the MATERIALS and MODELS chapters of the current
/// dictionary (chapter numbers at 0x0878167c and 0x08781680).
///
/// 1. `updateDefaultMaterial()`.
/// 2. Every material element in use whose record is not null:
///    - the three "needs" words: reflects when reflection is on (0x0879116a) and the
///      reflectivity (+0x6a8) is above 0.01; transparent when the transparency (+0x6b8) is
///      above 0.01 or the vertex alpha mode (+0xdca) is not 1; refracts when refraction is
///      on (0x0879116c) and the index (+0x6dc) is not 1;
///    - the reflection map (+0x6f4) is cleared; the working block, the tables and the
///      jitter as [`update_default_material`] describes;
///    - its 3D texture (+0x6fc): the working factors; its diffuse word (+0xf6) gives flag
///      0x400, its reflectivity word (+0xfa) "reflects", its transparency word (+0xfc)
///      "transparent" and 0x400, the word at +0x100 "refracts"; flag 4;
///    - every 2D map of its list (+0x6ec): as [`set_txt_flag`] treats one; the maps are
///      counted and the count stored at +0x6e8;
///    - the flag word (+0x6e0): the reflection and transparency flags, 0x40 with static
///      blur (+0x758), 2 unless the word at +0x20 is 1, 0x4000 by the switch; bit 0 of the
///      ray tracing word for 0x20 or 0x200.
/// 3. Every model element in use (its record is read without a null test): with a list of
///    its own maps (+0x21c), `setTxtFlag(list, &flags, model, material)` once per material
///    of the node, each call starting the flag word again from 0 (so the word is that of
///    the last material, and a node without materials leaves its maps unprepared); its own
///    3D texture (+0x224) as a material's, but without flag 4 and with the reflection flag
///    always 0x20; the node's flag word (+0x214) is the result **with flag 4 set for every
///    model**.
///
/// Class: single (libm in the tables and the jitter).
pub fn materials<E: MaterialsEnv>(st: &mut ShadeState, materials: &[DictElem], models: &[DictElem], env: &mut E) -> Result<(), SetupError> {
    let ambience = mean_ambience(st);
    update_default_material(st, env);
    for e in materials {
        let (0, Some(mat)) = (e.unused, e.data) else { continue };
        let mut flags = 0u16;
        let mut needs = Needs::default();
        {
            let g = &st.glb;
            let m = &st.materials[mat];
            if g.reflection != 0 {
                needs.reflect = x(m.reflectivity).gt(x(SOME));
            }
            needs.transp = x(m.transparency).gt(x(SOME)) || m.vcol_alpha != 1;
            if g.refraction != 0 {
                needs.refract = !F80::ONE.eq(x(m.refraction));
            }
        }
        st.materials[mat].reflect_map = None;
        working_block(st, mat, env);
        if st.materials[mat].tex3d != 0 {
            let tex = st.materials[mat].tex3d as usize - 1;
            tex3d_factors(st, tex, ambience);
            let t = &st.tex3ds[tex];
            if t.fx_diffuse != 0 {
                flags = mflag::TEXTURED_SHADOW;
            }
            if t.fx_reflect != 0 {
                needs.reflect = true;
            }
            if t.fx_transp != 0 {
                needs.transp = true;
                flags = mflag::TEXTURED_SHADOW;
            }
            if t.fx_100 != 0 {
                needs.refract = true;
            }
            flags |= mflag::TEXTURED;
        }
        let mut count = 0i16;
        let mut cur = st.materials[mat].maps;
        while let Some(map) = cur {
            if one_map(st, map, ambience, &mut flags, &mut needs, env)? {
                st.materials[mat].reflect_map = Some(map);
            }
            count = count.wrapping_add(1);
            cur = st.maps[map].next;
        }
        st.materials[mat].n_maps = count;
        need_flags(st, needs, st.materials[mat].reflect_map, &mut flags);
        let m = &mut st.materials[mat];
        if m.static_blur != 0 {
            flags |= mflag::STATIC_BLUR;
        }
        if flags & (mflag::REFLECT_TRACE | mflag::REFRACTS) != 0 {
            st.glb.raytrace = true;
        }
        if m.w20 != 1 {
            flags |= mflag::SMOOTH;
        }
        if st.glb.w_dec != 0 && flags & mflag::TRANSPARENT == 0 {
            flags |= mflag::W_DEC;
        }
        m.flags = flags;
    }
    for e in models {
        if e.unused != 0 {
            continue;
        }
        let model = e.data.ok_or(SetupError::Null("SI_materials: a model element without a record"))?;
        let mut flags = 0u16;
        let (own, list) = (st.models[model].maps, st.models[model].materials.clone());
        if own.is_some() {
            for material in list {
                flags = set_txt_flag(st, own, material, env)?;
            }
        }
        if st.models[model].tex3d != 0 {
            let tex = st.models[model].tex3d as usize - 1;
            let mut needs = Needs::default();
            tex3d_factors(st, tex, ambience);
            let t = &st.tex3ds[tex];
            if t.fx_diffuse != 0 {
                flags |= mflag::TEXTURED_SHADOW;
            }
            if t.fx_reflect != 0 {
                needs.reflect = true;
            }
            if t.fx_transp != 0 {
                needs.transp = true;
                flags |= mflag::TEXTURED_SHADOW;
            }
            if t.fx_100 != 0 {
                needs.refract = true;
            }
            if needs.reflect {
                flags |= mflag::REFLECT_TRACE;
            }
            if needs.transp {
                flags |= if needs.refract { mflag::REFRACTS } else { mflag::TRANSPARENT };
            }
        }
        if flags & (mflag::REFLECT_TRACE | mflag::REFRACTS) != 0 {
            st.glb.raytrace = true;
        }
        st.models[model].flags = flags | mflag::TEXTURED;
    }
    Ok(())
}

// ------------------------------------------------------------------------------- the pictures

/// The preview's two switches: `SI_previewSelectionCall` (a preview of the selection is
/// being rendered) and `SI_PreviewOptimize` (0x08791640: 1 unless the environment has
/// `SI_PREVIEW_OPT_DEFEAT`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Preview {
    pub selection_call: i16,
    pub optimize: i16,
}

/// The file's static initialiser [soft 0x0825b7ac __static_initialization_and_destruction_0],
/// run before `main` by [soft 0x0825b7e4 _GLOBAL_.I.SI_materials] (the constructor keyed to `SI_materials`) with
/// (1, 0xffff): `SI_PreviewOptimize` = 1 when `getenv("SI_PREVIEW_OPT_DEFEAT")` is null,
/// else 0. `defeat` is whether the variable is set. Class: copy.
pub fn static_initialization(defeat: bool) -> i16 {
    i16::from(!defeat)
}

/// The four chapters of the element dictionary the texture routines walk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureDict {
    /// Chapter 0x08781670: one element per picture ([`ShadeState::pictures`]).
    pub pictures: Vec<DictElem>,
    /// Chapter 0x087816c8: one element per 2D map ([`ShadeState::maps`]).
    pub maps: Vec<DictElem>,
    /// Chapter 0x08781680: the models; [`ModelElem::w24`] is the element's word at +0x24.
    pub models: Vec<ModelElem>,
}

/// A model's element: +0x04, +0x48 as [`DictElem`], and the word at +0x24 (not 0 for the
/// models the preview's texture marking looks at; its setter was not read).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelElem {
    pub elem: DictElem,
    pub w24: u32,
}

/// Clears the "needed" byte (+0x5b) of every picture element and every 2D map element in
/// use whose record is not null [soft 0x0825b3d4 resetTextureDirtyFlags]. Class: integer.
pub fn reset_texture_dirty_flags(dict: &mut TextureDict) {
    for e in dict.pictures.iter_mut().chain(dict.maps.iter_mut()) {
        if e.data.is_some() && e.unused == 0 {
            e.dirty = 0;
        }
    }
}

/// Marks the maps of one list and their pictures as needed
/// [soft 0x0825b528 markTextureImages]: `markTextureImages(dictionary, first map)`. For
/// every map of the list that has a picture record (+0x24) and an element in use in the 2D
/// map chapter (`SI_elemdictGetElemFromData`): the element's byte at +0x5b = 1, and the same
/// for the picture's element. Class: integer.
pub fn mark_texture_images(st: &ShadeState, dict: &mut TextureDict, maps: Option<usize>) {
    let mut cur = maps;
    while let Some(map) = cur {
        cur = st.maps[map].next;
        let Some(glb) = st.maps[map].glb else { continue };
        let Some(e) = dict.maps.iter_mut().find(|e| e.data == Some(map)) else { continue };
        if e.unused != 0 {
            continue;
        }
        e.dirty = 1;
        if let Some(p) = dict.pictures.iter_mut().find(|e| e.data == Some(glb)) {
            if p.unused == 0 {
                p.dirty = 1;
            }
        }
    }
}

/// Marks what the marked models use [soft 0x0825b46c markModelTextureImages]: for every
/// model element in use with a record and a non-zero word at +0x24: the node's own maps
/// (+0x21c), then the maps (+0x6ec) of each of its materials. (A null entry of the
/// material array is read through.) Class: integer.
pub fn mark_model_texture_images(st: &ShadeState, dict: &mut TextureDict) -> Result<(), SetupError> {
    for i in 0..dict.models.len() {
        let e = dict.models[i];
        let (Some(model), 0) = (e.elem.data, e.elem.unused) else { continue };
        if e.w24 == 0 {
            continue;
        }
        mark_texture_images(st, dict, st.models[model].maps);
        for k in 0..st.models[model].materials.len() {
            let mat = st.models[model].materials[k].ok_or(SetupError::Null("markModelTextureImages: a null material"))?;
            mark_texture_images(st, dict, st.materials[mat].maps);
        }
    }
    Ok(())
}

/// Loads the pictures and prepares every 2D map's region
/// [soft 0x0825af78 SI_processTextures]: `SI_processTextures(dictionary, load)`.
///
/// With `load` not 0: while a preview of the selection is rendered with the optimisation
/// on, the "needed" bytes are cleared and set again from the marked models; then every
/// picture element in use with a record and (outside such a preview, or needed) no pixels
/// yet is read ([`read_texture_file`] with 1).
///
/// Then every 2D map element in use (under the same preview rule) whose record has a
/// picture (+0x24):
///
/// - +0x78 = the extent in u (+0x58) is below 0. Region in u: from the offset (+0x68) to
///   offset + extent, the smaller first (+0x38, +0x40; the sum rounded to float); the
///   extent's size goes on. The same in v (+0x7a; +0x5c, +0x6c; +0x3c, +0x44).
/// - pixels per unit: +0x60 = (picture width × repetitions in u (+0xf8)) / size in u, +0x64
///   = (picture height × repetitions in v (+0xfa)) / size in v; with u and v exchanged
///   (+0xfc) the height goes with u and the width with v. Integer products, one division
///   each, rounded to float.
/// - the fractions: +0x48, +0x50, +0x4c, +0x54, +0x70, +0x74 = value − trunc(value) of
///   +0x38, +0x40, +0x3c, +0x44, +0x68, +0x6c; one below 0 gets + 1, **and the two upper
///   ends (+0x50, +0x54) also when they are 0**.
/// - +0x106 (the region runs over the seam in u) = the surface is closed in u (+0x102) and
///   the upper fraction is not above the lower + 1e-6 (the double, in the register); +0x108
///   the same in v (+0x104). With that and a negative extent the offset's fraction (+0x70,
///   +0x74) gets + 1.
///
/// Class: single.
pub fn process_textures<E: MaterialsEnv>(st: &mut ShadeState, dict: &mut TextureDict, load: i16, preview: Preview, env: &mut E) -> Result<(), SetupError> {
    let gated = preview.selection_call != 0 && preview.optimize != 0;
    if load != 0 {
        if gated {
            reset_texture_dirty_flags(dict);
            mark_model_texture_images(st, dict)?;
        }
        for i in 0..dict.pictures.len() {
            let e = dict.pictures[i];
            let (Some(p), 0) = (e.data, e.unused) else { continue };
            if (preview.selection_call == 0 || e.dirty != 0 || preview.optimize == 0) && st.pictures[p].pixels.is_none() {
                env.read_texture(st, p, 1)?;
            }
        }
    }
    for i in 0..dict.maps.len() {
        let e = dict.maps[i];
        let (0, Some(map)) = (e.unused, e.data) else { continue };
        if !(preview.selection_call == 0 || e.dirty != 0 || preview.optimize == 0) {
            continue;
        }
        let Some(glb) = st.maps[map].glb else { continue };
        let (pw, ph) = (st.pictures[glb].width as i32, st.pictures[glb].height as i32);
        let m = &mut st.maps[map];
        m.neg_u = i16::from(x(m.uscale).lt(x(0.0f32)));
        let size_u = if m.neg_u != 0 {
            m.umax = m.uoff;
            m.umin = (x(m.uoff) + m.uscale).to_f32();
            -x(m.uscale)
        } else {
            m.umin = m.uoff;
            m.umax = (x(m.uoff) + m.uscale).to_f32();
            x(m.uscale)
        };
        m.neg_v = i16::from(x(m.vscale).lt(x(0.0f32)));
        let size_v = if m.neg_v != 0 {
            m.vmax = m.voff;
            m.vmin = (x(m.voff) + m.vscale).to_f32();
            -x(m.vscale)
        } else {
            m.vmin = m.voff;
            m.vmax = (x(m.voff) + m.vscale).to_f32();
            x(m.vscale)
        };
        let (along_u, along_v) = if m.swap_uv != 0 { (ph, pw) } else { (pw, ph) };
        m.pix_u = (x(along_u.wrapping_mul(m.urepeat as i32)) / size_u).to_f32();
        m.pix_v = (x(along_v.wrapping_mul(m.vrepeat as i32)) / size_v).to_f32();
        let frac = |v: f32| (x(v) - x(x(v).to_i32(Round::Zero))).to_f32();
        m.u2min = frac(m.umin);
        m.u2max = frac(m.umax);
        m.v2min = frac(m.vmin);
        m.v2max = frac(m.vmax);
        m.uoff_frac = frac(m.uoff);
        m.voff_frac = frac(m.voff);
        let up = |v: f32| (x(v) + F80::ONE).to_f32();
        if x(m.u2min).lt(x(0.0f32)) {
            m.u2min = up(m.u2min);
        }
        if x(m.u2max).le(x(0.0f32)) {
            m.u2max = up(m.u2max);
        }
        if x(m.v2min).lt(x(0.0f32)) {
            m.v2min = up(m.v2min);
        }
        if x(m.v2max).le(x(0.0f32)) {
            m.v2max = up(m.v2max);
        }
        if x(m.uoff_frac).lt(x(0.0f32)) {
            m.uoff_frac = up(m.uoff_frac);
        }
        if x(m.voff_frac).lt(x(0.0f32)) {
            m.voff_frac = up(m.voff_frac);
        }
        m.wrap_u = i16::from(m.band_u != 0 && x(m.u2max).le(x(WRAP_EPS) + m.u2min));
        m.wrap_v = i16::from(m.band_v != 0 && x(m.v2max).le(x(WRAP_EPS) + m.v2min));
        if m.wrap_u != 0 && m.neg_u != 0 {
            m.uoff_frac = up(m.uoff_frac);
        }
        if m.wrap_v != 0 && m.neg_v != 0 {
            m.voff_frac = up(m.voff_frac);
        }
    }
    Ok(())
}

/// The texture pass of `SI_update` [soft 0x0825a5fc SI_textures]: `SI_textures(load)` is
/// `SI_processTextures(the current process's dictionary, load)`. Class: copy.
pub fn textures<E: MaterialsEnv>(st: &mut ShadeState, dict: &mut TextureDict, load: i16, preview: Preview, env: &mut E) -> Result<(), SetupError> {
    process_textures(st, dict, load, preview, env)
}

/// `SI_malloc` writes `o` into a block's first byte and `k` into its last (finding 0014).
/// The first is a pixel's byte and is overwritten; the last survives when the block's size
/// was rounded up past the pixels.
fn allocator_mark(block: &mut [u8], used: usize) {
    if block.len() > used {
        *block.last_mut().expect("not empty") = 0x6b;
    }
}

/// The fifth byte of a texture pixel: (least of red, green, blue + greatest) >> 1.
fn intensity(r: u8, g: u8, b: u8) -> u8 {
    ((r.min(g).min(b) as u32 + r.max(g).max(b) as u32) >> 1) as u8
}

/// Reads a picture into a texture's pixel block [soft 0x082598b0 SI_readTextureFile]:
/// `SI_readTextureFile(picture record, count)`, always 1.
///
/// The file is `<name>.pic` for a name that starts with `/`, else
/// `<SI_texturePath>/<name>.pic`. When it cannot be opened the status line gets `Cannot
/// find <name>, replaced by default texture.` and `<SI_resdir>/noIcon.pic` is read instead
/// (fatal when that fails too). Width and height go to +0x16 and +0x18. One line buffer per
/// plane the file's packets name (red, green, blue start as 0, alpha as 0xad); a plane the
/// file lacks reads as 0, **alpha as 0xad (173)**. The block ([`TexPixels`]) is filled row
/// by row, top row first, each pixel with its intensity, each row with its last pixel in
/// front and its first two behind; then the two rows below and the row above are copied.
/// A row the file does not hold repeats what the line buffers kept. With `count` not 0 the
/// counter at 0x08791214 grows by the block's pixels, and a picture with the crop switch
/// (+0x04) is cropped ([`global_2d_map_crop`]).
///
/// Where the Rust differs: a picture one pixel wide has its second extra column read from
/// the byte behind the one-byte line buffers in the original (unset memory); here the
/// first pixel again. A width or height below 1 (the open routine lets a negative one
/// through) is refused as the allocation failure the original most likely meets. Class:
/// integer.
pub fn read_texture_file(st: &mut ShadeState, picture: usize, count: i16, env: &mut dyn MaterialsEnv) -> Result<(), SetupError> {
    use si_picture::OpenPicture;
    let name = st.pictures[picture].name.clone();
    let shown = String::from_utf8_lossy(&name).into_owned();
    let mut data = env.picture_file(&name, false);
    if data.as_deref().is_none_or(|d| OpenPicture::open_signed(d).is_err()) {
        env.status(&format!("Cannot find {shown}, replaced by default texture."));
        data = env.picture_file(&name, true);
        if data.as_deref().is_none_or(|d| OpenPicture::open_signed(d).is_err()) {
            return Err(SetupError::Fatal(format!("{}noIcon.pic", msg_no_default())));
        }
    }
    let data = data.expect("tested above");
    let mut file = OpenPicture::open_signed(&data).expect("tested above");
    let (w16, h16) = (file.picture.width, file.picture.height);
    st.pictures[picture].width = w16;
    st.pictures[picture].height = h16;
    let (w, h) = (w16 as i32, h16 as i32);
    if w < 1 || h < 1 {
        return Err(SetupError::Fatal(format!(" {}", msg_no_memory())));
    }
    let (w, h) = (w as usize, h as usize);
    let mut rows: [Option<Vec<u8>>; 4] = Default::default();
    for p in &file.packets {
        for (i, bit) in [0x80u8, 0x40, 0x20, 0x10].into_iter().enumerate() {
            if p.channel & bit != 0 {
                rows[i] = Some(vec![if i == 3 { 0xad } else { 0 }; w]);
            }
        }
    }
    let stride = w + 3;
    let mut bytes = stride * (h + 3) * 5;
    if bytes % 4 != 0 {
        bytes += 4 - bytes % 4;
    }
    let mut block = vec![0u8; bytes];
    if count != 0 {
        st.texture_pixels = st.texture_pixels.wrapping_add((stride * (h + 3)) as i32);
    }
    let pixel = |rows: &[Option<Vec<u8>>; 4], at: usize| -> [u8; 5] {
        let get = |i: usize, none: u8| rows[i].as_ref().map_or(none, |r| r.get(at).copied().unwrap_or(r[0]));
        let (r, g, b, a) = (get(0, 0), get(1, 0), get(2, 0), get(3, 0xad));
        [r, g, b, a, intensity(r, g, b)]
    };
    for y in 0..h {
        file.scanline_read(&mut rows);
        let row = (y + 1) * stride * 5;
        block[row..row + 5].copy_from_slice(&pixel(&rows, w - 1));
        for xx in 0..w {
            block[row + (xx + 1) * 5..row + (xx + 2) * 5].copy_from_slice(&pixel(&rows, xx));
        }
        block[row + (w + 1) * 5..row + (w + 2) * 5].copy_from_slice(&pixel(&rows, 0));
        block[row + (w + 2) * 5..row + (w + 3) * 5].copy_from_slice(&pixel(&rows, 1));
    }
    // the two rows below, pixel by pixel from the first row on (so a picture of one row
    // repeats it twice), then the row above from the last row
    for k in 0..2 * stride * 5 {
        block[(h + 1) * stride * 5 + k] = block[stride * 5 + k];
    }
    for k in 0..stride * 5 {
        block[k] = block[h * stride * 5 + k];
    }
    allocator_mark(&mut block, stride * (h + 3) * 5);
    st.pictures[picture].pixels = Some(TexPixels { block, origin: w + 4 });
    if count != 0 && st.pictures[picture].crop != 0 {
        global_2d_map_crop(st, picture, count);
    }
    Ok(())
}

/// Cuts a texture's pixel block down to its crop rectangle
/// [soft 0x0825a1ac global2dMapCrop]: `global2dMapCrop(picture record, count)`. Nothing
/// unless 0 ≤ first column (+0x06) ≤ last column (+0x08) < width and 0 ≤ first row (+0x0a)
/// ≤ last row (+0x0c) < height. A new block for the rectangle with the same border: each
/// row copied, its last pixel put in front and its first two behind; the row above from
/// the last row; the two rows below from the block's rows 1 and 2 **in one overlapping
/// copy**, so a rectangle one row high gets its second row below from memory nobody wrote
/// (zero here). Width and height become the rectangle's; with `count` not 0 the counter at
/// 0x08791214 is corrected and the old block freed (with 0 it is left allocated). Class:
/// integer.
pub fn global_2d_map_crop(st: &mut ShadeState, picture: usize, count: i16) {
    let g = &st.pictures[picture];
    let (w, h) = (g.width, g.height);
    let (x0, x1, y0, y1) = (g.crop_x0, g.crop_x1, g.crop_y0, g.crop_y1);
    if !(x0 >= 0 && x1 < w && x0 <= x1 && y0 >= 0 && y1 < h && y0 <= y1) {
        return;
    }
    let Some(old) = g.pixels.as_ref() else { return };
    let (nw, nh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    let (stride, old_stride) = (nw + 3, w as usize + 3);
    let pixels = stride * (nh + 3);
    let mut bytes = pixels * 5;
    if bytes % 4 != 0 {
        bytes += 4 - bytes % 4;
    }
    let mut block = vec![0u8; bytes];
    for y in 0..nh {
        let src = (old.origin + x0 as usize + old_stride * (y0 as usize + y)) * 5;
        let dst = ((y + 1) * stride + 1) * 5;
        block[dst..dst + nw * 5].copy_from_slice(&old.block[src..src + nw * 5]);
        block.copy_within(dst + (nw - 1) * 5..dst + nw * 5, dst - 5);
        block.copy_within(dst..dst + 5, dst + nw * 5);
        block.copy_within(dst + 5..dst + 10, dst + (nw + 1) * 5);
    }
    block.copy_within(stride * nh * 5..stride * (nh + 1) * 5, 0);
    block.copy_within(stride * 5..stride * 3 * 5, stride * (nh + 1) * 5);
    allocator_mark(&mut block, pixels * 5);
    if count != 0 {
        st.texture_pixels = st.texture_pixels.wrapping_sub((old_stride * (h as usize + 3)) as i32).wrapping_add(pixels as i32);
    }
    let g = &mut st.pictures[picture];
    g.width = nw as i16;
    g.height = nh as i16;
    g.pixels = Some(TexPixels { block, origin: nw + 4 });
}

/// Frees every texture's pixels after the last frame [soft 0x0825a7e0 si_cleanUpTexture]:
/// for every picture element in use with a record that has pixels, the block is freed and
/// the pointer cleared. The records and the list stay. Class: integer.
pub fn clean_up_texture(st: &mut ShadeState, pictures: &[DictElem]) {
    for e in pictures {
        if let (Some(p), 0) = (e.data, e.unused) {
            st.pictures[p].pixels = None;
        }
    }
}
