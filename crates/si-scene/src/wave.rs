// SPDX-License-Identifier: MIT
//! A WAVES element's file read into the image (`wave.c`; finding 0018, section 20):
//! `SI_dbwaveAsciiRead` and what it calls, the wave records (`wave_list_struct`, 0x84
//! bytes from `malloc`) chained on `si_waveStatus`, each with its icon model (a node with
//! one mesh made from the binary's tables, `wave_tables.rs`) put into the scene's graph list.
//!
//! Read from the decompile and the instruction listings (the icon's polygon tables and the
//! variadic corners of `SI_polygonInitialize` from the listing). Every routine of another
//! source file goes through the loader's dispatch (`crate::image::Prog`): `si-hrc`'s image
//! forms of the node, mesh, vertex, polygon and definition allocators; the loader's
//! adapters of `SI_matrixXyzRotation`, `SI_modelUpdateMatrix` and the mesh rebuild; `si-view`'s
//! `SI_graflistAddData`; `SI_edAddWave` and `SI_backSelected` by address. Unchecked: not
//! compared with the emulated original.
//!
//! **The file is read as `fscanf` reads it** ([`Scan`]): `%s` a run of non-blank bytes,
//! `%d` and `%f` a number after blanks (`%f` correctly rounded to a float, as glibc's
//! `strtof`); a conversion that fails stores nothing and leaves the stream at the byte it
//! stopped on.

use crate::calls::{Channel, Message};
use crate::edtools::ByAddr;
use crate::image::Prog;
use crate::wave_tables as t;
use si_mem::{Mem, Ptr32};
use si_x87::x;

/// `si_waveStatus` (0x08822120, `.bss`): +0 the count of waves made (16 bits), +0x7c the
/// first wave (the chain runs through each wave's +0x78), +0x88 and +0x8c set to 1 by
/// `SI_initWave` (+0x8c is its "done" mark), +0x90 the number the renaming appends.
pub const SI_WAVE_STATUS: u32 = 0x0882_2120;
/// `SI_pipedFiles` (0x086ceba8): when set the reader takes `SI_pipe` instead of opening the
/// file (0 in the binary and in a batch render).
const SI_PIPED_FILES: u32 = 0x086c_eba8;

/// The `.rodata` text the icon node is named with (`"wave"`, 0x086d9307), which
/// `SI_modelAllocate` copies; [`store`] puts it into the image.
const WAVE_NAME: u32 = 0x086d_9307;

/// Routines called by address.
pub const SI_ED_ADD_WAVE: u32 = 0x0810_33b4;
pub const SI_BACK_SELECTED: u32 = 0x0832_60a0;
pub const SI_MATRIX_XYZ_ROTATION: u32 = 0x0851_882c;
pub const SI_MODEL_UPDATE_MATRIX: u32 = 0x0846_559c;
pub const SI_MESH_UPDATE: u32 = 0x0845_8d94;
pub const SI_GRAFLIST_ADD_DATA: u32 = 0x0832_d240;
/// `SI_grafLists` (0x08679498), `SI_procData` (0x08679314).
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_PROC_DATA: u32 = 0x0867_9314;

/// The wave record's words this file writes and reads.
mod w {
    pub const NUMBER: u32 = 0x00;
    pub const NAME: u32 = 0x04;
    pub const TYPE: u32 = 0x08;
    pub const DISPLACEMENT: u32 = 0x0c;
    pub const FRAME_START: u32 = 0x10;
    pub const FRAME_END: u32 = 0x14;
    pub const VELOCITY: u32 = 0x18;
    pub const ACCELERATION: u32 = 0x1c;
    pub const FALLOFF_START: u32 = 0x20;
    pub const FALLOFF_END: u32 = 0x24;
    pub const PERIODIC: u32 = 0x28;
    pub const NB_AMPLITUDE: u32 = 0x2c;
    pub const AMP_EXTRAP: u32 = 0x30;
    pub const MIN_X: u32 = 0x38;
    pub const MAX_X: u32 = 0x3c;
    pub const MIN_Y: u32 = 0x40;
    pub const MAX_Y: u32 = 0x44;
    pub const AMPLITUDE: u32 = 0x48;
    pub const NB_PROFILE: u32 = 0x4c;
    pub const PROFILE: u32 = 0x68;
    pub const ICON: u32 = 0x74;
    pub const NEXT: u32 = 0x78;
}

/// The `.rodata` texts this file needs in the image.
pub fn store(m: &mut Mem) {
    m.put_cstr(WAVE_NAME, b"wave");
}

/// `fscanf` on the bytes of a file.
pub struct Scan<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

fn blank(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

impl Scan<'_> {
    fn skip(&mut self) {
        while self.pos < self.data.len() && blank(self.data[self.pos]) {
            self.pos += 1;
        }
    }

    /// `%s`: `None` at the end of the file (where `fscanf` answers `EOF`).
    pub fn token(&mut self) -> Option<Vec<u8>> {
        self.skip();
        let start = self.pos;
        while self.pos < self.data.len() && !blank(self.data[self.pos]) {
            self.pos += 1;
        }
        (self.pos > start).then(|| self.data[start..self.pos].to_vec())
    }

    /// `%d`: a sign and decimal digits; the value kept to 32 bits.
    pub fn int(&mut self) -> Option<i32> {
        self.skip();
        let start = self.pos;
        let mut neg = false;
        if self.pos < self.data.len() && matches!(self.data[self.pos], b'+' | b'-') {
            neg = self.data[self.pos] == b'-';
            self.pos += 1;
        }
        let digits = self.pos;
        let mut v: i64 = 0;
        while self.pos < self.data.len() && self.data[self.pos].is_ascii_digit() {
            v = (v * 10 + (self.data[self.pos] - b'0') as i64).min(1 << 40);
            self.pos += 1;
        }
        if self.pos == digits {
            // glibc puts back the byte it stopped on, not the sign it took
            self.pos = if digits > start { digits } else { start };
            return None;
        }
        let v = if neg { -v } else { v };
        Some(v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }

    /// `%f`: digits with a point and an exponent, rounded once to a float.
    pub fn float(&mut self) -> Option<f32> {
        self.skip();
        let start = self.pos;
        let d = self.data;
        let mut p = self.pos;
        if p < d.len() && matches!(d[p], b'+' | b'-') {
            p += 1;
        }
        let mantissa = p;
        while p < d.len() && d[p].is_ascii_digit() {
            p += 1;
        }
        if p < d.len() && d[p] == b'.' {
            p += 1;
            while p < d.len() && d[p].is_ascii_digit() {
                p += 1;
            }
        }
        if p == mantissa || (p == mantissa + 1 && d[mantissa] == b'.') {
            self.pos = start;
            return None;
        }
        if p < d.len() && (d[p] | 0x20) == b'e' {
            let mut q = p + 1;
            if q < d.len() && matches!(d[q], b'+' | b'-') {
                q += 1;
            }
            if q < d.len() && d[q].is_ascii_digit() {
                while q < d.len() && d[q].is_ascii_digit() {
                    q += 1;
                }
                p = q;
            }
        }
        self.pos = p;
        std::str::from_utf8(&d[start..p]).ok()?.parse::<f32>().ok()
    }
}

/// `SI_initWave (0x084c5c70)`: once (+0x8c clear), the count 0, no wave, +0x88 and +0x8c 1,
/// the renaming number 0.
pub fn init_wave(m: &mut Mem) {
    let s = SI_WAVE_STATUS;
    if m.u32(s + 0x8c) == 0 {
        m.set_i16(s, 0);
        m.set_u32(s + 0x7c, 0);
        m.set_u32(s + 0x88, 1);
        m.set_u32(s + 0x8c, 1);
        m.set_u32(s + 0x90, 0);
    }
}

/// `si_waveAllocate (0x084c5cb4)(status)`: the count up; a record of 0x84 bytes from
/// `malloc` put at the end of the chain (the link is stored before the test, so a failed
/// `malloc` leaves null there); its number the count, its name `wave<n>` in 0x50 bytes of
/// `malloc`, falloffs 10000, velocity 10, frames 1 to 100, the rest of what follows zero
/// (the extents +0x38 … +0x44, the profile's range +0x58 … +0x64 and +0x70 are left as
/// `malloc` gave them); then `SI_edAddWave(null, wave)`.
pub fn wave_allocate(o: &mut Prog<'_>, m: &mut Mem, status: u32) -> u32 {
    let count = m.i16(status).wrapping_add(1);
    m.set_i16(status, count);
    let mut at = status + 4;
    while m.u32(at + w::NEXT) != 0 {
        at = m.u32(at + w::NEXT);
    }
    let p = m.malloc(0x84);
    m.set_u32(at + w::NEXT, p);
    if p == 0 {
        return 0;
    }
    m.set_i16(p + w::NUMBER, m.i16(status));
    m.set_u32(p + 0x6c, 0);
    m.set_u32(p + w::NEXT, 0);
    let name = m.malloc(0x50);
    m.set_u32(p + w::NAME, name);
    let mut text = b"wave".to_vec();
    text.extend_from_slice(&si_mem::clib::decimal(m.i16(p) as i32));
    m.put_cstr(name, &text);
    m.set_f32(p + w::FALLOFF_START, 10000.0);
    m.set_f32(p + w::FALLOFF_END, 10000.0);
    m.set_f32(p + w::VELOCITY, 10.0);
    for (off, v) in [(0x1c, 0), (0x08, 0), (0x0c, 0), (0x10, 1), (0x14, 100), (0x28, 0), (0x50, 0), (0x54, 0), (0x4c, 0), (0x68, 0), (0x30, 0), (0x34, 0), (0x2c, 0), (0x48, 0), (0x74, 0)] {
        m.set_u32(p + off, v);
    }
    o.call_addr(m, SI_ED_ADD_WAVE, &[0, p]);
    p
}

/// `si_waveReadWave (0x084c9180)(stream, wave, &scale, &rotate, &translate)`: keyword after
/// keyword (`%s`, each compared whole) until the end of the file or `End`, answering 1
/// either way. `name` reads the wave's name into its block (then every **other** wave of
/// the chain with that name is renamed `<name>.<n>`, `n` the status's number, counted
/// up; its old name freed); `type`, `displacement`, `frame_start`, `frame_end`, `periodic`,
/// `amp_extrap` read a `%d`, `velocity`, `acceleration`, `falloff_start`, `falloff_end` a
/// `%f`; `amplitude` the number of pairs, and when it is above 1 the pairs (`%f  %f`) into
/// a block of `malloc` with their extents (a value at or below the smallest so far, or not
/// a number, becomes the smallest; only one above it can become the largest); `scale`,
/// `rotate`, `translate` three `%f` into the caller's vectors. Another word: the warning
/// `Warning: invalid keyword %s in wave file.` on the standard error.
///
/// Where Rust differs: the original reads a word into 40 bytes of its stack and a name
/// into the 0x50 bytes of the name's block, and the new name of a renamed wave into
/// strlen + 3 bytes; longer words would overrun them (none in the corpus); here they are
/// stored whole into the image.
pub fn wave_read_wave(o: &mut Prog<'_>, m: &mut Mem, f: &mut Scan<'_>, wave: u32, scale: u32, rotate: u32, translate: u32) -> i16 {
    loop {
        let Some(word) = f.token() else { return 1 };
        let int_at = |m: &mut Mem, f: &mut Scan<'_>, at: u32| {
            if let Some(v) = f.int() {
                m.set_i32(at, v);
            }
        };
        let float_at = |m: &mut Mem, f: &mut Scan<'_>, at: u32| {
            if let Some(v) = f.float() {
                m.set_f32(at, v);
            }
        };
        match word.as_slice() {
            b"name" => {
                if let Some(name) = f.token() {
                    m.put_cstr(m.u32(wave + w::NAME), &name);
                }
                let mut other = m.u32(SI_WAVE_STATUS + 0x7c);
                while other != 0 {
                    let a = m.cstr(m.u32(other + w::NAME));
                    let b = m.cstr(m.u32(wave + w::NAME));
                    if a == b && other != wave {
                        let p = m.malloc(b.len() as u32 + 3);
                        let n = m.i32(SI_WAVE_STATUS + 0x90);
                        let mut text = b.clone();
                        text.push(b'.');
                        text.extend_from_slice(&si_mem::clib::decimal(n));
                        m.put_cstr(p, &text);
                        m.set_i32(SI_WAVE_STATUS + 0x90, n.wrapping_add(1));
                        m.free(m.u32(other + w::NAME));
                        m.set_u32(other + w::NAME, p);
                    }
                    other = m.u32(other + w::NEXT);
                }
            }
            b"type" => int_at(m, f, wave + w::TYPE),
            b"displacement" => int_at(m, f, wave + w::DISPLACEMENT),
            b"frame_start" => int_at(m, f, wave + w::FRAME_START),
            b"frame_end" => int_at(m, f, wave + w::FRAME_END),
            b"velocity" => float_at(m, f, wave + w::VELOCITY),
            b"acceleration" => float_at(m, f, wave + w::ACCELERATION),
            b"falloff_start" => float_at(m, f, wave + w::FALLOFF_START),
            b"falloff_end" => float_at(m, f, wave + w::FALLOFF_END),
            b"periodic" => int_at(m, f, wave + w::PERIODIC),
            b"amp_extrap" => int_at(m, f, wave + w::AMP_EXTRAP),
            b"amplitude" => {
                int_at(m, f, wave + w::NB_AMPLITUDE);
                let n = m.i32(wave + w::NB_AMPLITUDE);
                if n > 1 {
                    let p = m.malloc((n as u32) << 3);
                    m.set_u32(wave + w::AMPLITUDE, p);
                    float_at(m, f, p);
                    float_at(m, f, p + 4);
                    let (mut min_x, mut min_y) = (m.f32(p), m.f32(p + 4));
                    let (mut max_x, mut max_y) = (min_x, min_y);
                    // the two locals the pairs are read into: their first contents are the
                    // stack's, here 0 (a pair that does not convert keeps the previous one)
                    let (mut a, mut b) = (0f32, 0f32);
                    for i in 1..n as u32 {
                        if let Some(v) = f.float() {
                            a = v;
                            if let Some(v) = f.float() {
                                b = v;
                            }
                        }
                        // `fcom`: "not above" (below, equal or unordered) takes the new value
                        if a > min_x {
                            if max_x < a {
                                max_x = a;
                            }
                        } else {
                            min_x = a;
                        }
                        if b > min_y {
                            if max_y < b {
                                max_y = b;
                            }
                        } else {
                            min_y = b;
                        }
                        m.set_f32(p + 8 * i, a);
                        m.set_f32(p + 8 * i + 4, b);
                    }
                    m.set_f32(wave + w::MIN_X, min_x);
                    m.set_f32(wave + w::MAX_X, max_x);
                    m.set_f32(wave + w::MIN_Y, min_y);
                    m.set_f32(wave + w::MAX_Y, max_y);
                }
            }
            b"scale" | b"rotate" | b"translate" => {
                let v = match word.as_slice() {
                    b"scale" => scale,
                    b"rotate" => rotate,
                    _ => translate,
                };
                for k in 0..3 {
                    match f.float() {
                        Some(x) => m.set_f32(v + 4 * k, x),
                        None => break,
                    }
                }
            }
            b"End" => return 1,
            _ => {
                let mut text = b"Warning: invalid keyword ".to_vec();
                text.extend_from_slice(&word);
                text.extend_from_slice(b" in wave file.\n");
                o.sc.messages.push(Message { channel: Channel::Stderr, text });
            }
        }
    }
}

/// `si_waveInitHorizProf (0x084c66e0)(wave)`: for a wave of type 1, the horizontal profile:
/// six points (`malloc` of 48 bytes) (0, 1), (1.6, 1), (1.7, 0.85), (1.8, 0.5), (1.9, 0.15),
/// (2, 0) over the range −2 … 2, 0 … 1.
pub fn wave_init_horiz_prof(m: &mut Mem, wave: u32) {
    if m.i32(wave + w::TYPE) != 1 {
        return;
    }
    m.set_i32(wave + w::NB_PROFILE, 6);
    m.set_f32(wave + 0x58, -2.0);
    m.set_f32(wave + 0x5c, 2.0);
    m.set_u32(wave + 0x60, 0);
    m.set_f32(wave + 0x64, 1.0);
    let p = m.malloc((m.i32(wave + w::NB_PROFILE) as u32) << 3);
    m.set_u32(wave + w::PROFILE, p);
    let words: [u32; 12] = [0, 0x3f80_0000, 0x3fcc_cccd, 0x3f80_0000, 0x3fd9_999a, 0x3f59_999a, 0x3fe6_6666, 0x3f00_0000, 0x3ff3_3333, 0x3e19_999a, 0x4000_0000, 0];
    for (k, v) in words.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *v);
    }
}

/// `SI_polygonInitialize (0x0845a694)(polygon, n, corners …)`: the count (16 bits, +0x10),
/// a corner array from `SI_polygonNodeAllocate` (+0x0c), each corner's vertex the first
/// word of its 0x24-byte record.
fn polygon_initialize(m: &mut Mem, poly: u32, corners: &[i32]) {
    let n = corners.len() as i32;
    m.set_i16(poly + 0x10, n as i16);
    let nodes = si_hrc::image::geom::polygon_node_allocate(m, n);
    m.set_u32(poly + 0x0c, nodes);
    let nodes = m.u32(poly + 0x0c);
    for (k, v) in corners.iter().enumerate() {
        m.set_i32(nodes + 0x24 * k as u32, *v);
    }
}

/// The polygons of a table from polygon `first` on, each a triangle when its corners 2 and
/// 3 are the same, else a quadrangle, every corner moved by `base`.
fn polygons(m: &mut Mem, mesh: u32, first: u32, table: &[[u32; 4]], base: i32) {
    for (k, p) in table.iter().enumerate() {
        let poly = m.u32(mesh + 4) + 0x18 * (first + k as u32);
        let c: Vec<i32> = p.iter().map(|v| base.wrapping_add(*v as i32)).collect();
        polygon_initialize(m, poly, if p[2] == p[3] { &c[..3] } else { &c[..4] });
    }
}

/// The vertices of a table from vertex `first` on (three floats each, 0x14 bytes a vertex).
fn vertices(m: &mut Mem, mesh: u32, first: u32, table: &[[u32; 3]]) {
    for (k, v) in table.iter().enumerate() {
        let at = m.u32(mesh) + 0x14 * (first + k as u32);
        for (j, word) in v.iter().enumerate() {
            m.set_u32(at + 4 * j as u32, *word);
        }
    }
}

/// [soft 0x084c61b8 si_waveCreateIconMdl] `(wave)`: a node named `wave` (`SI_modelAllocate`) with
/// one mesh (`SI_meshAllocate`): the shape of the wave's type (circular 0, planar 1,
/// spherical 2: vertices and polygons from the binary's tables), then the direction arrow
/// (32 vertices, 32 polygons) and the "up" arrow (27 vertices, 28 polygons). Of the up
/// arrow's polygons, the original decides "triangle" from the **direction** arrow's table
/// and takes a triangle's corners from it too (the listing reads `wave_drctn_poly` there);
/// so does this. `SI_meshUpdate`, bit 3 of the mesh's byte +0x20 cleared, the mesh the
/// node's definition (`SI_modelAddDefinition`, kind 4), the node the wave's icon (+0x74).
/// Null when the node or the mesh cannot be made.
///
/// Where Rust differs: for another type the original goes on with two counts it never set
/// and a mesh without vertices or polygons; here the icon is not made (null).
///
/// Held against the emulated original with `SI_meshUpdate` a recorded stub on both sides (the
/// port's mesh update is a boxed mesh and takes other heap blocks): finding 0018, section 24.
pub fn wave_create_icon_mdl(o: &mut Prog<'_>, m: &mut Mem, wave: u32) -> u32 {
    let model = si_hrc::image::node::model_allocate(m, WAVE_NAME).addr();
    if model == 0 {
        return 0;
    }
    let mesh = si_hrc::image::geom::mesh_allocate(m, o).addr();
    if mesh == 0 {
        return 0;
    }
    m.set_i32(mesh + 0x14, 0x3b);
    m.set_i32(mesh + 0x18, 0x3c);
    // (vertices, polygons of the shape; its vertex and polygon counts)
    let (nv, np, shape_v, shape_p): (i32, i32, u32, u32) = match m.i32(wave + w::TYPE) {
        1 => (0x4f, 0x48, 20, 12),
        0 => (0x54, 0x3c + 0x18, 25, 24),
        2 => (0x75, 0x7c, 58, 64),
        _ => return 0,
    };
    m.set_i32(mesh + 0x14, nv);
    if m.i32(wave + w::TYPE) != 0 {
        m.set_i32(mesh + 0x18, np);
    }
    let v = si_hrc::image::geom::vertex_allocate(m, nv);
    m.set_u32(mesh, v);
    match m.i32(wave + w::TYPE) {
        1 => vertices(m, mesh, 0, &t::WAVE_PLANAR_VTX[..]),
        0 => vertices(m, mesh, 0, &t::WAVE_CIRCULAR_VTX[..]),
        _ => vertices(m, mesh, 0, &t::WAVE_SPHERICAL_VTX[..]),
    }
    if m.i32(wave + w::TYPE) == 0 {
        // the circular shape: the polygon count grows by 0x18 from the 0x3c set above
        let n = m.i32(mesh + 0x18);
        m.set_i32(mesh + 0x18, n.wrapping_add(0x18));
    }
    let p = si_hrc::image::geom::polygon_allocate(m, m.i32(mesh + 0x18));
    m.set_u32(mesh + 4, p);
    match m.i32(wave + w::TYPE) {
        1 => polygons(m, mesh, 0, &t::WAVE_PLANAR_POLY[..], 0),
        0 => polygons(m, mesh, 0, &t::WAVE_CIRCULAR_POLY[..], 0),
        _ => polygons(m, mesh, 0, &t::WAVE_SPHERICAL_POLY[..], 0),
    }
    // the direction arrow after the shape, the up arrow after it
    vertices(m, mesh, shape_v, &t::WAVE_DRCTN_VTX[..]);
    polygons(m, mesh, shape_p, &t::WAVE_DRCTN_POLY[..], shape_v as i32);
    let (up_v, up_p) = (shape_v + 32, shape_p + 32);
    vertices(m, mesh, up_v, &t::WAVE_UP_VTX[..]);
    for (k, q) in t::WAVE_UP_POLY.iter().enumerate() {
        let d = t::WAVE_DRCTN_POLY[k];
        let poly = m.u32(mesh + 4) + 0x18 * (up_p + k as u32);
        let b = up_v as i32;
        if d[2] == d[3] {
            polygon_initialize(m, poly, &[b.wrapping_add(d[0] as i32), b.wrapping_add(d[1] as i32), b.wrapping_add(d[2] as i32)]);
        } else {
            polygon_initialize(m, poly, &[b.wrapping_add(q[0] as i32), b.wrapping_add(q[1] as i32), b.wrapping_add(q[2] as i32), b.wrapping_add(q[3] as i32)]);
        }
    }
    o.call_addr(m, SI_MESH_UPDATE, &[mesh]);
    let flags = m.u8(mesh + 0x20);
    m.set_u8(mesh + 0x20, flags & 0xf7);
    si_hrc::image::node::model_add_definition(m, o, model, 4, mesh);
    m.set_u32(wave + w::ICON, model);
    model
}

/// `si_waveResizeIcon (0x084c9e30)(wave, factors)`: the icon's first definition's
/// vertices (+0x48 of the node; count +0x14, array +0, 0x14 bytes a vertex) scaled: type 1
/// x by factor x and y by factor y; type 0 x and z by factor x, y by factor y; type 2 all
/// three by factor y; another type not at all.
pub fn wave_resize_icon(m: &mut Mem, wave: u32, factors: u32) {
    let mesh = m.u32(m.u32(wave + w::ICON) + 0x48);
    let n = m.i32(mesh + 0x14);
    let (fx, fy) = (m.f32(factors), m.f32(factors + 4));
    let scale: [f32; 3] = match m.i32(wave + w::TYPE) {
        1 => [fx, fy, 1.0],
        0 => [fx, fy, fx],
        2 => [fy, fy, fy],
        _ => return,
    };
    let ty = m.i32(wave + w::TYPE);
    for k in 0..n.max(0) as u32 {
        let v = m.u32(mesh) + 0x14 * k;
        for (j, s) in scale.iter().enumerate() {
            if ty == 1 && j == 2 {
                continue;
            }
            let at = v + 4 * j as u32;
            m.set_f32(at, (x(m.f32(at)) * *s).to_f32());
        }
    }
}

/// `SI_waveGetIconModel (0x084c6f00)(wave)`: the wave's icon (+0x74).
pub fn wave_get_icon_model(m: &Mem, wave: u32) -> u32 {
    m.u32(wave + w::ICON)
}

/// `SI_dbwaveAsciiRead (0x084c7980)(path)`, which `SI_elemdictReadWave (0x0810ff7c)` is a
/// call of: `SI_initWave`; the file (`data`, `None` where `fopen` fails: null); for every
/// `Wave` word of it a wave (`si_waveAllocate`) read (`si_waveReadWave`), its profile
/// (`si_waveInitHorizProf`), its icon (`si_waveCreateIconMdl`; without one the message
/// `Could not create icon for wave.` is printed **into the file's stream**, the file closed
/// and null answered) given the read scaling, rotation (and its matrix,
/// `SI_matrixXyzRotation` into +0x118) and translation, `SI_modelUpdateMatrix`, its size
/// (`si_waveResizeIcon` by the amplitude's extents (x, y, x)), `SI_backSelected(0, 1)`, and
/// the wave put into the current graph list as a node of kind 0xb
/// (`SI_graflistAddData(list, wave, 0xb, 1)`). The answer is the last wave read.
///
/// Where Rust differs: with no `Wave` word the original answers a word of its stack it
/// never set; here null. The three vectors are stack words too, read by the icon whether
/// the file set them or not; here they start at 0. With `SI_pipedFiles` set the original
/// reads `SI_pipe` (never in a batch render); here nothing is read and null is answered.
pub fn db_wave_ascii_read(o: &mut Prog<'_>, m: &mut Mem, data: Option<&[u8]>) -> u32 {
    if m.u32(SI_WAVE_STATUS + 0x8c) == 0 {
        init_wave(m);
    }
    if m.u32(SI_PIPED_FILES) != 0 {
        return 0;
    }
    let Some(data) = data else { return 0 };
    let mut f = Scan { data, pos: 0 };
    let mut last = 0u32;
    let vectors = m.local(0x30);
    for k in 0..12 {
        m.set_u32(vectors + 4 * k, 0);
    }
    let (scale, rotate, translate, size) = (vectors, vectors + 0xc, vectors + 0x18, vectors + 0x24);
    while let Some(word) = f.token() {
        if word != b"Wave" {
            continue;
        }
        let wave = wave_allocate(o, m, SI_WAVE_STATUS);
        last = wave;
        if wave_read_wave(o, m, &mut f, wave, scale, rotate, translate) == 0 {
            last = 0;
            break;
        }
        wave_init_horiz_prof(m, wave);
        let icon = wave_create_icon_mdl(o, m, wave);
        if icon == 0 {
            last = 0;
            break;
        }
        for (dst, src) in [(0x10c, rotate), (0x100, scale), (0x158, translate)] {
            for k in 0..3 {
                m.set_u32(icon + dst + 4 * k, m.u32(src + 4 * k));
            }
        }
        o.call_addr(m, SI_MATRIX_XYZ_ROTATION, &[rotate, icon + 0x118]);
        o.call_addr(m, SI_MODEL_UPDATE_MATRIX, &[icon]);
        let dx = (x(m.f32(wave + w::MAX_X)) - x(m.f32(wave + w::MIN_X))).to_f32();
        let dy = (x(m.f32(wave + w::MAX_Y)) - x(m.f32(wave + w::MIN_Y))).to_f32();
        m.set_f32(size, dx);
        m.set_f32(size + 4, dy);
        m.set_f32(size + 8, dx);
        wave_resize_icon(m, wave, size);
        o.call_addr(m, SI_BACK_SELECTED, &[0, 1]);
        let list = m.u32(SI_GRAF_LISTS).wrapping_add((m.i32(m.u32(SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24));
        o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, wave, 0xb, 1]);
    }
    m.release(0x30);
    let _ = Ptr32::<u8>::NULL;
    last
}
