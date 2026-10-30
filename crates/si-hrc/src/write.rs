// SPDX-License-Identifier: MIT
//! The writers (source files `binary_b.c`, `b_matio.c`, and the metaball writers in
//! `binary.c`).
//!
//! The original writers are fixed sequences of fields, with a version test here and there,
//! that match the prototype table the program writes in front of them
//! [soft 0x083fe0ac SI_prototypeBinaryWrite]. Here each writer walks the table and emits one
//! token at a time, which for the table of version 4.0 (the only version this build writes)
//! gives the original's sequence. The version tests inside the original writers are
//! therefore not separate code here: with an output version of 4.0 they all pass.
//!
//! Walking a table also lets a file be written back with the table it came with
//! ([`write_as_read`]), which the original never does; the check binary uses it to compare
//! bytes.

use std::collections::BTreeMap;

use si_file::{write_bin_header, write_db_header, write_general_header, MAGIC_MATERIAL, MAGIC_TEXTURE_2D, MAGIC_TEXTURE_3D, VERSION_OUTPUT};

use crate::io::{c_len, c_str, Wr};
use crate::model::*;
use crate::proto::{self, Prototypes};
use crate::read::{Content, File};

pub struct Writer<'p> {
    pub w: Wr,
    pub protos: &'p Prototypes,
    /// (prototype, token) pairs the table asked for that no writer of this build emits.
    pub unwritable: BTreeMap<(usize, u16), u32>,
    /// Places where the original would have followed a null pointer.
    pub null_pointers: u32,
}

/// Whether a block is written at all: never one tagged `CUSELEMCB` or `CUSNEXTELEMCB`, and
/// in mode 1 not the custom-effect blocks `CUSA` to `CUSD` either.
fn counted_in(n: &UserNode, mode: i16) -> bool {
    let tag = c_str(&n.tag);
    if mode == 1 && crate::read::is_custom_tag(tag) {
        return false;
    }
    tag != b"CUSELEMCB" && tag != b"CUSNEXTELEMCB"
}

fn counted(n: &UserNode) -> bool {
    counted_in(n, 0)
}

/// The size a list of user-data blocks takes in a file, and how many blocks are written
/// [soft 0x083f3780 si_userDataSize]: 4 for the count, and per block the tag plus the data
/// plus 10. Models, materials and textures use mode 0.
pub fn user_data_size_mode(list: &[UserNode], mode: i16) -> (i32, i32) {
    let mut size = 4i32;
    let mut count = 0i32;
    for n in list.iter().filter(|n| counted_in(n, mode)) {
        size = size.wrapping_add(c_len(&n.tag) as i32).wrapping_add(n.size).wrapping_add(10);
        count += 1;
    }
    (size, count)
}

pub fn user_data_size(list: &[UserNode]) -> (i32, i32) {
    user_data_size_mode(list, 0)
}

/// [soft 0x083f3904 userDataClusterSize]
fn ud_cluster_size(n: &Node, c: Option<&Cluster>, vertex: bool) -> i32 {
    match c {
        None => 0,
        Some(c) => (if vertex && n.kind == 2 { c.count.wrapping_mul(8) } else { c.count.wrapping_mul(4) }).wrapping_add(9),
    }
}

impl<'p> Writer<'p> {
    pub fn new(protos: &'p Prototypes) -> Self {
        Writer { w: Wr::default(), protos, unwritable: BTreeMap::new(), null_pointers: 0 }
    }

    /// The tokens of a prototype, without the terminating zero.
    fn tokens(&mut self, id: usize) -> &'p [i16] {
        match self.protos.get(id) {
            Some(l) => &l[..l.iter().position(|&t| t == 0).unwrap_or(l.len())],
            None => &[],
        }
    }

    fn cannot(&mut self, id: usize, t: i16) {
        *self.unwritable.entry((id, t as u16)).or_insert(0) += 1;
    }

    fn bytes_padded(&mut self, data: &[u8], n: usize) {
        // the original writes n bytes from the buffer whatever its real size
        let have = data.len().min(n);
        self.w.bytes(&data[..have]);
        self.w.out.resize(self.w.out.len() + (n - have), 0);
    }

    pub fn user_data_blocks(&mut self, list: &[UserNode]) {
        self.user_data_blocks_mode(list, 0)
    }

    /// The blocks of a list [soft 0x083f3940 si_userDataWrite].
    pub fn user_data_blocks_mode(&mut self, list: &[UserNode], mode: i16) {
        for n in list.iter().filter(|n| counted_in(n, mode)) {
            let tag = c_str(&n.tag);
            self.w.i32(tag.len() as i32);
            self.w.bytes(tag);
            self.w.i16(n.flag);
            self.w.i32(n.size);
            self.bytes_padded(&n.data, n.size.max(0) as usize);
        }
    }

    /// A user-data token of a material or texture: the length, the count, the blocks.
    fn user_data_token(&mut self, list: &[UserNode]) {
        let (size, count) = user_data_size(list);
        self.w.i32(size);
        self.w.i32(count);
        self.user_data_blocks(list);
    }

    /// [soft 0x083e9068 si_udfWrite]: a missing reference is an empty name and three zeros.
    pub fn udf(&mut self, u: Option<&Udf>) {
        for &t in self.tokens(proto::UDF) {
            match t {
                1 => self.w.string(u.map(|u| u.name.as_slice())),
                2 => self.w.i32(u.map_or(0, |u| u.active as i32)),
                3 => self.w.i32(u.map_or(0, |u| u.internal as i32)),
                4 => self.w.i32(u.map_or(0, |u| u.order)),
                _ => self.cannot(proto::UDF, t),
            }
        }
    }

    /// [soft 0x083e73f8 si_textureBinaryWrite]. Token 0x1e writes a mask of 4 as 3; token
    /// 0x238 writes the same field unchanged.
    pub fn texture(&mut self, x: &Texture2D) {
        for &t in self.tokens(proto::TEXTURE) {
            let w = &mut self.w;
            match t as u16 {
                1 => w.string(Some(&x.name)),
                2 => w.i16(x.anim),
                3 => w.i16(x.urepeat),
                4 => w.i16(x.vrepeat),
                5 => w.f32(x.uscale),
                6 => w.f32(x.vscale),
                7 => w.f32(x.uoffset),
                8 => w.f32(x.voffset),
                9 => w.i16(x.pixel_interp),
                10 => w.i16(x.method),
                0xb => w.f32(x.ambient),
                0xc => w.f32(x.diffuse),
                0xd => w.f32(x.specular),
                0xe => w.f32(x.roughness),
                0xf => w.f32(x.transparency),
                0x10 => w.f32(x.reflection_map),
                0x12 => w.f32(x.env_rotation),
                0x14 => w.f32(x.blend),
                0x15 => w.i16(x.crop_flag),
                // two shorts each, from overlapping places
                0x16 => {
                    w.i16(x.crop[0]);
                    w.i16(x.crop[1]);
                }
                0x17 => {
                    w.i16(x.crop[2]);
                    w.i16(x.crop[3]);
                }
                0x18 => {
                    w.i16(x.crop[1]);
                    w.i16(x.crop[2]);
                }
                0x19 => {
                    w.i16(x.crop[3]);
                    w.i16(x.crop[4]);
                }
                0x1a => w.i16(x.swap_uv),
                0x1b => w.i16(x.alt_u),
                0x1c => w.i16(x.alt_v),
                0x1d => w.f32(x.reflectivity),
                0x1e => w.i16(if x.mask == 4 { 3 } else { x.mask }),
                0x1f => w.i16(x.channel),
                0x20 => w.floats(&x.rotation),
                0x21 => w.floats(&x.scaling),
                0x22 => w.floats(&x.translation),
                0x23 => w.i16(x.black_white),
                0x24 => self.udf(x.udf.as_ref()),
                0x25 => w.i16(x.displacement),
                0x32 => w.string(Some(&x.filename)),
                0x233 => w.i16(x.hw_flag),
                0x234 => w.i16(x.hw_tiling),
                0x235 => w.i16(x.x1cc),
                0x236 => w.i16(x.x1ce),
                0x237 => w.i16(x.x1d4),
                0xff35 => self.user_data_token(&x.user_data),
                0x238 => w.i16(x.mask),
                _ => self.cannot(proto::TEXTURE, t),
            }
        }
    }

    /// [soft 0x083e7cf4 si_texture3DBinaryWrite]
    pub fn texture3d(&mut self, x: &Texture3D) {
        for &t in self.tokens(proto::TEXTURE_3D) {
            let w = &mut self.w;
            match t as u16 {
                1 => w.string(Some(&x.name)),
                2 => w.i16(x.kind as i16),
                3 => w.i16(x.iterations as i16),
                4 => w.f32(x.strength),
                5 => w.f32(x.spacing),
                0x16..=0x1a => w.floats(&x.colours[t as usize - 0x16]),
                10 => w.f32(x.factors[0]),
                0x1b => w.f32(x.factors[1]),
                0x1c => w.f32(x.factors[2]),
                0xb => w.floats(&x.scaling),
                0xc => w.floats(&x.rotation),
                0xd => w.floats(&x.translation),
                0xe => w.f32(x.ambient),
                0xf => w.f32(x.diffuse),
                0x10 => w.f32(x.specular),
                0x11 => w.f32(x.roughness),
                0x12 => w.f32(x.transparency),
                0x13 => w.f32(x.reflection),
                0x15 => w.f32(x.power),
                0x1d => w.f32(x.taper),
                0x1f => w.f32(x.blending),
                0x20 => w.i16(x.mask),
                0x21 => w.i16(x.channel),
                0x22 => self.udf(x.udf.as_ref()),
                0x23 => w.i16(x.displacement),
                0xff24 => self.user_data_token(&x.user_data),
                _ => self.cannot(proto::TEXTURE_3D, t),
            }
        }
    }

    /// [soft 0x083e8604 si_materialBinaryWrite]. `with_textures` is its third argument: set
    /// for a material inside a model, clear for a material file, which then gets two zero
    /// counts in place of the texture lists.
    ///
    /// The shading model goes out three times: token 2 with model 7 written as 1, token
    /// 0x218 as 7 unless both extra shorts are 1, and token 0x21b as it is.
    pub fn material(&mut self, m: &Material, with_textures: bool) {
        for &t in self.tokens(proto::MATERIAL) {
            let w = &mut self.w;
            match t as u16 {
                1 => w.string(Some(&m.name)),
                2 => w.i16(if m.shading == 7 { 1 } else { m.shading }),
                0xe => w.i16(m.glow),
                0xf => w.f32(m.glow_amount),
                3 => w.floats(&m.ambient),
                4 => w.floats(&m.diffuse),
                5 => w.floats(&m.specular),
                6 => w.f32(m.exponent),
                7 => w.f32(m.transparency),
                0xd => w.f32(m.reflectivity),
                0xb => w.f32(m.refraction),
                0x10 => {
                    if with_textures {
                        w.i16(m.textures.len() as i16);
                        for x in &m.textures {
                            self.w.i16(0);
                            self.texture(x);
                        }
                    } else {
                        w.i16(0);
                    }
                }
                0x11 => match &m.texture3d {
                    Some(x) if with_textures => {
                        w.i16(1);
                        w.i16(0);
                        self.texture3d(x);
                    }
                    _ => w.i16(0),
                },
                0x12 => w.i16(m.blur),
                0x13 => w.f32(m.blur_width),
                0x14 => w.f32(m.blur_decay),
                0x15 => self.udf(m.udf_material.as_ref()),
                0x16 => self.udf(m.udf_volume.as_ref()),
                0x17 => self.udf(m.udf_shadow.as_ref()),
                0x218 => w.i16(if (m.xdc8, m.xdca) != (1, 1) { 7 } else { m.shading }),
                0xff19 => self.user_data_token(&m.user_data),
                0xff1a => {
                    let name = m.udf_extra.as_ref().map_or(0, |u| c_len(&u.name));
                    w.i32(0xd + name as i32);
                    self.udf(m.udf_extra.as_ref());
                }
                0x21b => w.i16(m.shading),
                0x21c => w.i16(m.xdc8),
                0x21d => w.i16(m.xdca),
                _ => self.cannot(proto::MATERIAL, t),
            }
        }
    }

    /// [soft 0x083f59e0 splineBinaryWrite]. The key loop counts down a signed short.
    pub fn spline(&mut self, s: &Spline) {
        for &t in self.tokens(proto::SPLINE) {
            match t {
                8 => self.w.string(Some(&s.name)),
                1 => self.w.i16(s.kind as i16),
                7 => {
                    self.w.u16(s.nkeys);
                    for k in 0..(s.nkeys as i16).max(0) as usize {
                        let key = s.keys.get(k).copied().unwrap_or_default();
                        for &u in self.tokens(proto::SPLINE_KEY) {
                            match u {
                                1 => self.w.floats(&key.prev),
                                2 => self.w.floats(&key.pos),
                                3 => self.w.floats(&key.next),
                                4 => self.w.i16(key.constraint),
                                5 => self.w.i16(key.segment),
                                6 => self.w.i16(key.flag),
                                _ => self.cannot(proto::SPLINE_KEY, u),
                            }
                        }
                    }
                }
                3 => self.w.i16(s.close),
                4 => self.w.f32(s.tension),
                5 => self.w.i16(s.step),
                6 => self.w.i16(s.phantom),
                _ => self.cannot(proto::SPLINE, t),
            }
        }
    }

    /// [soft 0x083f58fc faceBinaryWrite]
    pub fn face(&mut self, f: &Face) {
        for &t in self.tokens(proto::FACE) {
            match t {
                3 => self.w.i16(f.is_spline),
                1 if f.is_spline != 0 => self.spline(&f.exterior),
                2 if f.is_spline != 0 => {
                    self.w.i16(f.count);
                    for k in 0..f.count.max(0) as usize {
                        match f.holes.get(k) {
                            Some(s) => self.spline(s),
                            None => self.null_pointers += 1,
                        }
                    }
                }
                4 if f.is_spline == 0 => {
                    self.w.i16(f.count);
                    for k in 0..f.count.max(0) as usize {
                        match f.nurbs.get(k) {
                            Some(c) => self.nurbs_curve(c),
                            None => self.null_pointers += 1,
                        }
                    }
                }
                1 | 2 | 4 => {}
                _ => self.cannot(proto::FACE, t),
            }
        }
    }

    /// [soft 0x083f5b00 patchBinaryWrite]
    pub fn patch(&mut self, p: &Patch) {
        let total = p.nu as usize * p.nv as usize;
        for &t in self.tokens(proto::PATCH) {
            let w = &mut self.w;
            match t {
                1 | 2 => w.i16(p.kind[t as usize - 1] as i16),
                3 => {
                    w.u16(p.nu);
                    w.u16(p.nv);
                    for k in 0..total {
                        w.floats(&p.points.get(k).copied().unwrap_or_default());
                    }
                }
                4 | 5 => w.i16(p.close[t as usize - 4]),
                6 | 7 => w.f32(p.tension[t as usize - 6]),
                8 | 9 => w.i16(p.step[t as usize - 8]),
                10 | 0xb => w.i16(p.curves[t as usize - 10]),
                0xc..=0xf => w.i16(p.caps[t as usize - 0xc]),
                0x10 => w.i16(p.faceted),
                0x11 => w.i16(p.approx),
                0x12 => w.i16(p.view),
                0x13 => w.f32(p.edge),
                0x14 | 0x15 => w.f32(p.curvature[t as usize - 0x14]),
                0x16 => w.i16(p.rec_min),
                0x17 => w.i16(p.rec_max),
                0x18 => w.i16(p.recursion),
                0x19 => {
                    for k in 0..total {
                        w.i16(p.tags.get(k).copied().unwrap_or(0));
                    }
                }
                _ => self.cannot(proto::PATCH, t),
            }
        }
    }

    /// [soft 0x083f54f4 meshBinaryWrite]. `n` is the node, which holds the number of UV sets.
    pub fn mesh(&mut self, m: &Mesh, n: &Node) {
        for &t in self.tokens(proto::MESH) {
            match t as u16 {
                6 => self.w.i16(m.flag),
                2 => self.w.f32(m.angle),
                0x413 => self.w.i32(n.uv_sets),
                7 => {
                    self.w.i32(m.nvertices);
                    for k in 0..m.nvertices.max(0) as usize {
                        let v = m.vertices.get(k).copied().unwrap_or_default();
                        for &u in self.tokens(proto::VERTEX) {
                            match u {
                                1 => self.w.floats(&v.pos),
                                4 => self.w.i16(v.flag),
                                _ => self.cannot(proto::VERTEX, u),
                            }
                        }
                    }
                }
                8 => {
                    self.w.i32(m.npolygons);
                    let none = Polygon::default();
                    for k in 0..m.npolygons.max(0) as usize {
                        self.polygon(m.polygons.get(k).unwrap_or(&none), n);
                    }
                }
                9 => {
                    self.w.i32(m.nedges);
                    for k in 0..m.nedges.max(0) as usize {
                        let e = m.edges.get(k).copied().unwrap_or_default();
                        for &u in self.tokens(proto::EDGE) {
                            match u {
                                4 => {
                                    self.w.i32(e.v[0]);
                                    self.w.i32(e.v[1]);
                                }
                                3 => self.w.i16(e.flag),
                                _ => self.cannot(proto::EDGE, u),
                            }
                        }
                    }
                }
                0x20a => self.w.i16(m.x52),
                0x20b => self.w.i16(m.x50),
                0x20c => self.w.i16(m.x38),
                0x40d..=0x40f => self.w.f32(m.approx[t as usize - 0x40d]),
                0x210 => self.w.i16(m.x48),
                0x211 => self.w.i16(m.x4a),
                0x212 => self.w.i16(m.x4c),
                _ => self.cannot(proto::MESH, t),
            }
        }
    }

    fn polygon(&mut self, p: &Polygon, n: &Node) {
        for &t in self.tokens(proto::POLYGON) {
            match t {
                2 => {
                    self.w.u16(p.nnodes);
                    let none = PolyNode::default();
                    for k in 0..p.nnodes as usize {
                        let d = p.nodes.get(k).unwrap_or(&none);
                        for &u in self.tokens(proto::POLY_NODE) {
                            match u as u16 {
                                3 => self.w.i32(d.vertex),
                                2 => self.w.floats(&d.normal),
                                4 => self.w.floats(&d.uv),
                                0x405 => self.w.u32(d.colour),
                                0xff06 => {
                                    let sets = n.uv_sets.wrapping_sub(1);
                                    self.w.i32(sets.wrapping_mul(8));
                                    for s in 0..sets.max(0) as usize {
                                        let uv = d.more_uv.get(s).copied();
                                        if uv.is_none() {
                                            self.null_pointers += 1;
                                        }
                                        self.w.floats(&uv.unwrap_or_default());
                                    }
                                }
                                _ => self.cannot(proto::POLY_NODE, u),
                            }
                        }
                    }
                }
                3 => self.w.i16(p.material),
                4 => self.w.i16(p.flag),
                _ => self.cannot(proto::POLYGON, t),
            }
        }
    }

    /// [soft 0x083f2520 si_metaballBinaryWrite]
    pub fn metaball(&mut self, b: &Metaball) {
        for &t in self.tokens(proto::METABALL) {
            match t {
                4 => self.w.f64(b.ratio),
                5 => self.w.f64(b.weight),
                6 => self.w.doubles(&b.exponent),
                7 => self.w.floats(&b.matrix),
                _ => self.cannot(proto::METABALL, t),
            }
        }
    }

    /// [soft 0x083f29ac si_metaSystemBinaryWrite]. The lists go out in their order in
    /// memory, which is the reverse of the order they were read in.
    ///
    /// The original first checks which primitives of a group still are metaballs under the
    /// system (`SI_metaSystemValidateGroups`) and writes only those, by the name of the
    /// model each was resolved to. That needs the scene; here every name is written as read.
    pub fn metasystem(&mut self, m: &MetaSystem) {
        for &t in self.tokens(proto::METASYSTEM) {
            match t {
                1 => self.w.f32(m.wire_res),
                2 => self.w.f32(m.render_res),
                3 => self.w.i16(m.show),
                4 => {
                    self.w.i16(m.groups.len() as i16);
                    for g in &m.groups {
                        for &u in self.tokens(proto::METAGROUP) {
                            match u {
                                1 => self.w.string(Some(&g.name)),
                                2 => {
                                    self.w.i16(g.primitives.len() as i16);
                                    for p in &g.primitives {
                                        self.w.string(Some(p));
                                    }
                                }
                                _ => self.cannot(proto::METAGROUP, u),
                            }
                        }
                    }
                }
                5 => {
                    self.w.i16(m.unblends.len() as i16);
                    for pair in &m.unblends {
                        for &u in self.tokens(proto::METAUNBLEND) {
                            match u {
                                1 => {
                                    self.w.string(Some(&pair[0]));
                                    self.w.string(Some(&pair[1]));
                                }
                                _ => self.cannot(proto::METAUNBLEND, u),
                            }
                        }
                    }
                }
                _ => self.cannot(proto::METASYSTEM, t),
            }
        }
    }

    /// [soft 0x083f6d0c nurbsCrvBinaryWrite] (its writing mode; its counting mode is
    /// [`nurbs_curve_size`]).
    pub fn nurbs_curve(&mut self, c: &NurbsCurve) {
        for &t in self.tokens(proto::NURBS_CURVE) {
            let w = &mut self.w;
            match t {
                1 => w.i32(c.order),
                2 => {
                    w.i32(c.npoints);
                    for k in 0..c.npoints.max(0) as usize {
                        let p = c.points.get(k).copied().unwrap_or_default();
                        w.doubles(&p.xyzw);
                        w.i32(p.flag);
                    }
                }
                3 => w.i16(c.closed),
                4 => {
                    w.i32(c.uniformity);
                    w.i32(c.nknots);
                    for k in 0..c.nknots.max(0) as usize {
                        w.f64(c.knots.get(k).copied().unwrap_or(0.0));
                    }
                }
                5 => {
                    w.f64(c.start);
                    w.f64(c.end);
                }
                6 => w.i32(c.step),
                7 => w.i32(c.closing),
                0x208 => w.i16(c.keyframed),
                _ => self.cannot(proto::NURBS_CURVE, t),
            }
        }
    }

    /// [soft 0x083f76c0 nurbsCompBinaryWrite] (its writing mode)
    pub fn nurbs_composite(&mut self, c: &NurbsComposite) {
        for &t in self.tokens(proto::NURBS_COMPOSITE) {
            match t {
                1 => self.w.i16(c.boundary),
                2 => {
                    self.w.i32(c.ncurves);
                    for k in 0..c.ncurves.max(0) as usize {
                        match c.curves.get(k) {
                            Some(x) => self.nurbs_curve(x),
                            None => self.null_pointers += 1,
                        }
                    }
                }
                _ => self.cannot(proto::NURBS_COMPOSITE, t),
            }
        }
    }

    fn composites(&mut self, count: i32, list: &[NurbsComposite]) {
        for k in 0..count.max(0) as usize {
            if let Some(c) = list.get(k) {
                self.nurbs_composite(c); // a null entry writes nothing
            }
        }
    }

    /// [soft 0x083f7038 nurbsSrfBinaryWrite]. The length in front of the projected curves
    /// (token 0xff1a) comes from a counting pass over them.
    pub fn nurbs_surface(&mut self, s: &NurbsSurface) {
        for &t in self.tokens(proto::NURBS_SURFACE) {
            let w = &mut self.w;
            match t as u16 {
                1 => {
                    w.i32(s.order[0]);
                    w.i32(s.order[1]);
                }
                2 => {
                    w.i32(s.count[0]);
                    w.i32(s.count[1]);
                    for k in 0..s.count[0].wrapping_mul(s.count[1]).max(0) as usize {
                        let p = s.points.get(k).copied().unwrap_or_default();
                        w.doubles(&p.xyzw);
                        w.i32(p.flag);
                    }
                }
                3 => {
                    w.i16(s.closed[0]);
                    w.i16(s.closed[1]);
                }
                4 => {
                    w.i32(s.uniformity);
                    for d in 0..2 {
                        w.i32(s.nknots[d]);
                        for k in 0..s.nknots[d].max(0) as usize {
                            w.f64(s.knots[d].get(k).copied().unwrap_or(0.0));
                        }
                    }
                }
                5 => w.doubles(&s.range),
                6 => w.i16(s.border),
                7 => {
                    w.i32(s.ntrims);
                    self.composites(s.ntrims, &s.trims);
                }
                8 | 9 => w.i32(s.steps[t as usize - 8]),
                10 | 0xb => w.i32(s.curves[t as usize - 10]),
                0xc..=0xf => w.i16(s.caps[t as usize - 0xc]),
                0x10 | 0x11 => w.i32(s.closing[t as usize - 0x10]),
                0x12 => w.i16(s.approx),
                0x13 => w.i16(s.view),
                0x14 => w.f32(s.edge),
                0x15 | 0x16 => w.f32(s.curvature[t as usize - 0x15]),
                0x17 => w.i16(s.rec_min),
                0x18 => w.i16(s.rec_max),
                0x19 => w.i16(s.recursion),
                0xff1a => {
                    let mut size = 4i32;
                    for k in 0..s.nprojected.max(0) as usize {
                        if let Some(c) = s.projected.get(k) {
                            size = size.wrapping_add(nurbs_composite_size(c));
                        }
                    }
                    w.i32(size);
                    w.i32(s.nprojected);
                    self.composites(s.nprojected, &s.projected);
                }
                _ => self.cannot(proto::NURBS_SURFACE, t),
            }
        }
    }

    /// [soft 0x083f5da0 dynamatterWriteBinaryChunk]
    fn dyna_matter(&mut self, d: &DynaMatter) {
        for &t in self.tokens(proto::DYNA_MATTER) {
            match t {
                1 => self.w.f32(d.density),
                2 => self.w.f32(d.roughness),
                3 => self.w.f32(d.elasticity),
                4 => self.w.f32(d.rigidity),
                5 => self.w.f32(d.plasticity),
                6 => self.w.f32(d.kinetic),
                7 => self.w.i32(d.collision),
                _ => self.cannot(proto::DYNA_MATTER, t),
            }
        }
    }

    /// [soft 0x083f5f68 envelopWriteBinaryChunk] [soft 0x083f5f18 envelopVertWriteBinaryChunk].
    /// A deformation curve is always written as 200 values, whatever was read.
    fn envelop(&mut self, e: &Envelop) {
        for &t in self.tokens(proto::ENVELOP) {
            match t {
                1 => self.w.i32(e.level),
                5 => self.w.f32(e.joint_area),
                4 => self.w.i32(e.in_type),
                7..=9 => self.w.f32(e.inner[t as usize - 7]),
                0xb => self.w.f32(e.inner[3]),
                0xc => self.w.i32(e.out_type),
                0xd..=0x10 => self.w.f32(e.outer[t as usize - 0xd]),
                0x11 | 0x12 => match if t == 0x11 { &e.in_curve } else { &e.out_curve } {
                    None => self.w.i16(0),
                    Some(c) => {
                        self.w.i16(200);
                        for k in 0..200 {
                            // a curve read with fewer values is overrun here by the original
                            self.w.f32(c.get(k).copied().unwrap_or(0.0));
                        }
                    }
                },
                0x14 => self.w.i16(e.lists.len() as i16),
                0x15 => {
                    for list in &e.lists {
                        self.w.i32(list.len() as i32);
                        for v in list {
                            for &u in self.tokens(proto::ENVELOP_VERTEX) {
                                match u {
                                    1 => self.w.i32(v.index),
                                    3 => self.w.f32(v.ratio),
                                    4 => self.w.f32(v.thickness),
                                    6 => self.w.f32(v.weight),
                                    _ => self.cannot(proto::ENVELOP_VERTEX, u),
                                }
                            }
                        }
                    }
                }
                _ => self.cannot(proto::ENVELOP, t),
            }
        }
    }

    /// [soft 0x083f5e4c clusterWriteBinaryChunk] for a cluster of the node's list, which
    /// has a prototype.
    fn cluster(&mut self, c: &Cluster, n: &Node) {
        for &t in self.tokens(proto::CLUSTER) {
            match t {
                1 => self.w.string(Some(&c.name)),
                2 => self.w.i32(c.count),
                3 => self.cluster_indices(c, n.kind == 2),
                4 => self.w.i32(c.state),
                _ => self.cannot(proto::CLUSTER, t),
            }
        }
    }

    fn cluster_indices(&mut self, c: &Cluster, doubled: bool) {
        let total = if doubled { c.count.wrapping_mul(2) } else { c.count };
        for k in 0..total.max(0) as usize {
            self.w.i32(c.indices.get(k).copied().unwrap_or(0));
        }
    }

    /// The cluster of a user-data set: the same routine [soft 0x083f5e4c clusterWriteBinaryChunk]
    /// called outside any prototype, so always name (null: one zero byte), count, indices,
    /// state. Only the vertex set doubles the indices on a face.
    fn ud_cluster(&mut self, set: usize, n: &Node) {
        let c = n.ud_clusters[set].as_ref();
        self.w.i32(ud_cluster_size(n, c, set == 0));
        if let Some(c) = c {
            self.w.string(Some(&c.name));
            self.w.i32(c.count);
            self.cluster_indices(c, set == 0 && n.kind == 2);
            self.w.i32(c.state);
        }
    }

    /// [soft 0x083f3b0c userDataArrayWrite]: nothing but a zero length unless the set has
    /// both its cluster and its array; otherwise the length, the cluster's count, and per
    /// component its number of blocks and the blocks.
    fn ud_array(&mut self, set: usize, n: &Node) {
        let (Some(c), Some(a)) = (&n.ud_clusters[set], &n.ud_arrays[set]) else {
            self.w.i32(0);
            return;
        };
        let count = c.count.max(0) as usize;
        let empty = Vec::new();
        let lists: Vec<&Vec<UserNode>> = (0..count).map(|k| a.get(k).unwrap_or(&empty)).collect();
        let size = lists.iter().fold(4i32, |s, l| s.wrapping_add(user_data_size(l).0));
        self.w.i32(size);
        self.w.i32(c.count);
        for l in lists {
            self.w.i32(user_data_size(l).1);
            self.user_data_blocks(l);
        }
    }

    /// One node and what hangs below and beside it [soft 0x083f3c20 SI_modelBinaryWriteNode].
    /// `sons` and `brothers` are its third and fourth arguments: the root is written with
    /// its sons but without its brothers; every node below it with both.
    ///
    /// Reproduced for a plain save: the options that convert every shape to a triangle mesh
    /// (`SI_saveTriMesh`) or write shapes in global coordinates (its sixth argument), the
    /// custom-effect hook for algorithm 13 and the bookkeeping of UV domain ids are not.
    pub fn node(&mut self, n: &Node, sons: bool, brothers: bool) {
        for &t in self.tokens(proto::HIERARCHY) {
            match t as u16 {
                1 => self.w.string(Some(&n.name)),
                2 => self.w.i16(n.kind as i16),
                3 => match (n.kind, n.definitions.first()) {
                    (1, Some(Definition::Patch(p))) => self.patch(p),
                    (2, Some(Definition::Face(f))) => self.face(f),
                    (4, Some(Definition::Mesh(m))) => self.mesh(m, n),
                    (6, Some(Definition::Spline(s))) => self.spline(s),
                    (7, Some(Definition::MetaSystem(m))) => self.metasystem(m),
                    (8, Some(Definition::Metaball(b))) => self.metaball(b),
                    (9, Some(Definition::NurbsCurve(c))) => self.nurbs_curve(c),
                    (10, Some(Definition::NurbsSurface(s))) => self.nurbs_surface(s),
                    (1 | 2 | 4 | 6..=10, _) => self.null_pointers += 1,
                    _ => {}
                },
                0xc => self.w.i16(n.shape_interp),
                4 => self.w.floats(&n.scaling),
                5 => self.w.floats(&n.rotation),
                6 => self.w.floats(&n.translation),
                7 => {
                    self.w.i16(n.nb_materials);
                    for k in 0..n.nb_materials.max(0) as usize {
                        self.w.i16(0);
                        match n.materials.get(k) {
                            Some(Some(m)) => self.material(m, true),
                            // a material that was external in the file is a null entry:
                            // the original writer would read through it
                            _ => self.null_pointers += 1,
                        }
                    }
                }
                8 => {
                    self.w.i16(n.textures.len() as i16);
                    for x in &n.textures {
                        self.w.i16(0);
                        self.texture(x);
                    }
                }
                9 => match &n.texture3d {
                    None => self.w.i16(0),
                    Some(x) => {
                        self.w.i16(1);
                        self.w.i16(0);
                        self.texture3d(x);
                    }
                },
                0xe => {
                    self.w.i16(n.dynamics.is_some() as i16);
                    if let Some(d) = &n.dynamics {
                        self.w.i16(0);
                        self.dyna_matter(d);
                    }
                }
                0x10 => self.w.i16(match n.algorithm as i16 {
                    2 => 1,
                    0xc => 0,
                    a => a,
                }),
                0x11 => {
                    self.w.i16(n.envelop_type as i16);
                    self.w.i16(n.envelop.is_some() as i16);
                    if let Some(e) = &n.envelop {
                        self.envelop(e);
                    }
                }
                0x12 => self.w.i16(n.chain_dynamics as i16),
                0x13 => {
                    // [soft 0x083f5e28 dynajntpropWriteBinaryChunk]
                    self.w.i16(n.joint_friction.is_some() as i16);
                    if let Some(f) = &n.joint_friction {
                        for &u in self.tokens(proto::JOINT) {
                            match u {
                                1 => self.w.floats(f),
                                _ => self.cannot(proto::JOINT, u),
                            }
                        }
                    }
                }
                0x14 => {
                    // [soft 0x084aea20 SI_invKinWriteBinaryChunk]
                    self.w.i16(n.ik.is_some() as i16);
                    if let Some(k) = &n.ik {
                        self.w.i32(k.kind);
                        self.w.i32(k.nb_joints);
                        self.w.floats(&k.rotation);
                    }
                }
                0x15 => {
                    // [soft 0x083f5edc clustersWriteBinaryChunk] [soft 0x0843d8f0 SI_clusterCount]
                    self.w.i16(n.clusters.len() as i16);
                    for c in &n.clusters {
                        self.cluster(c, n);
                    }
                }
                0x16 => {
                    // the user nodes whose tag does not start with an underscore, in the
                    // old form: the first four bytes of the tag, the size, the data
                    let old: Vec<&UserNode> = n.user_nodes.iter().filter(|u| counted(u) && u.tag.first() != Some(&b'_')).collect();
                    self.w.i16(old.len() as i16);
                    for u in old {
                        self.bytes_padded(&u.tag, 4);
                        self.w.i32(u.size);
                        self.bytes_padded(&u.data, u.size.max(0) as usize);
                    }
                }
                0x17 => self.w.i16(n.visibility),
                0x18 => self.w.i16(n.motion_blur),
                0x19 => {
                    self.w.i16(n.init_transfo.is_some() as i16);
                    if let Some(it) = &n.init_transfo {
                        for v in it {
                            self.w.floats(v);
                        }
                    }
                }
                0x41a => self.w.f32(n.matte),
                0x21b => self.w.i16(n.garbage_matte),
                0xff1c => {
                    // the user nodes whose tag starts with an underscore, in the new form.
                    // The original tests the tag of the list's *first* node against
                    // "__NodeInfo" for every node, and writes none of them if it matches.
                    let none = n.user_nodes.first().is_some_and(|u| c_str(&u.tag) == b"__NodeInfo");
                    let new: Vec<&UserNode> = n.user_nodes.iter().filter(|u| !none && u.tag.first() == Some(&b'_')).collect();
                    let size = new.iter().fold(4i32, |s, u| s.wrapping_add(c_len(&u.tag) as i32).wrapping_add(u.size).wrapping_add(10));
                    self.w.i32(size);
                    self.w.i32(new.len() as i32);
                    for u in new {
                        let tag = c_str(&u.tag);
                        self.w.i32(tag.len() as i32);
                        self.w.bytes(tag);
                        self.w.i16(u.flag);
                        self.w.i32(u.size);
                        self.bytes_padded(&u.data, u.size.max(0) as usize);
                    }
                }
                0xff1d => self.ud_cluster(0, n),
                0xff1e => self.ud_array(0, n),
                0xff1f => self.ud_cluster(1, n),
                0xff20 => self.ud_array(1, n),
                0xff21 => self.ud_cluster(2, n),
                0xff22 => self.ud_array(2, n),
                0xff24 => {
                    let sets = if n.uv_sets > 1 { (n.uv_sets - 1) as usize } else { 0 };
                    let empty = Vec::new();
                    let lists: Vec<&Vec<i32>> = (0..sets).map(|k| n.uv_domains.as_ref().and_then(|d| d.get(k)).unwrap_or(&empty)).collect();
                    self.w.i32(lists.iter().map(|l| 4 + 4 * l.len() as i32).sum());
                    for l in lists {
                        self.w.i32(l.len() as i32);
                        for id in l {
                            self.w.i32(*id);
                        }
                    }
                }
                0x221 => self.w.i16(n.x404),
                0x222 => self.w.i16(n.x406),
                0x223 => self.w.i16(n.x420),
                0x224 => self.w.i16(n.flag8 as i16),
                0xa => {
                    let son = if sons { n.son.as_deref() } else { None };
                    self.w.i16(son.is_some() as i16);
                    if let Some(s) = son {
                        self.node(s, true, true);
                    }
                }
                0xb => {
                    let brother = if brothers { n.brother.as_deref() } else { None };
                    self.w.i16(brother.is_some() as i16);
                    if let Some(b) = brother {
                        self.node(b, true, true);
                    }
                }
                _ => self.cannot(proto::HIERARCHY, t),
            }
        }
    }
}

/// The counting mode of [soft 0x083f6d0c nurbsCrvBinaryWrite]: the bytes a curve takes with
/// this build's layout (36 per point, 8 per knot, 48 for the rest).
pub fn nurbs_curve_size(c: &NurbsCurve) -> i32 {
    let mut n = 8i32;
    if c.npoints > 0 {
        n = n.wrapping_add(c.npoints.wrapping_mul(0x24));
    }
    n.wrapping_add(10).wrapping_add(c.nknots.wrapping_mul(8)).wrapping_add(26)
}

/// The counting mode of [soft 0x083f76c0 nurbsCompBinaryWrite].
pub fn nurbs_composite_size(c: &NurbsComposite) -> i32 {
    let mut n = 6i32;
    for k in 0..c.ncurves.max(0) as usize {
        if let Some(x) = c.curves.get(k) {
            n = n.wrapping_add(nurbs_curve_size(x));
        }
    }
    n
}

const HRCH: &[u8; 4] = b"HRCH";

/// A model file as the program saves it [soft 0x083ec3f0 SI_fdmodelBinaryWrite]: a general
/// header at version 4.0 with the default comment, `PROT` and the current table, `HRCH` and
/// the root node. (`SI_omodelBinaryWrite`, 0x083f3430, does the same into `<name>.hrc` after
/// recording that name in the root node, and can ask for global coordinates.)
pub fn write_model(root: &Node) -> Vec<u8> {
    let protos = Prototypes::current(VERSION_OUTPUT);
    let mut w = Writer::new(&protos);
    write_general_header(&mut w.w.out, None);
    w.w.bytes(b"PROT");
    protos.write_as_read(&mut w.w);
    w.w.bytes(HRCH);
    w.node(root, true, false);
    w.w.out
}

/// A material file [soft 0x083e8a04 SI_materialWriteBinaryFile]: the material magic, the
/// version the caller gives, `PROT`, `MTRL` and the material without its texture lists.
pub fn write_material(m: &Material, version: f32) -> Vec<u8> {
    let protos = Prototypes::current(VERSION_OUTPUT);
    let mut w = Writer::new(&protos);
    write_db_header(&mut w.w.out, version, MAGIC_MATERIAL);
    w.w.bytes(b"PROT");
    protos.write_as_read(&mut w.w);
    w.w.bytes(b"MTRL");
    w.material(m, false);
    w.w.out
}

/// A 2D texture file [soft 0x083e8c28 SI_textureWriteBinaryFile].
pub fn write_texture(x: &Texture2D, version: f32) -> Vec<u8> {
    let protos = Prototypes::current(VERSION_OUTPUT);
    let mut w = Writer::new(&protos);
    write_db_header(&mut w.w.out, version, MAGIC_TEXTURE_2D);
    w.w.bytes(b"PROT");
    protos.write_as_read(&mut w.w);
    w.w.bytes(b"TXMP");
    w.texture(x);
    w.w.out
}

/// A 3D texture file [soft 0x083e8e48 SI_texture3DWriteBinaryFile].
pub fn write_texture3d(x: &Texture3D, version: f32) -> Vec<u8> {
    let protos = Prototypes::current(VERSION_OUTPUT);
    let mut w = Writer::new(&protos);
    write_db_header(&mut w.w.out, version, MAGIC_TEXTURE_3D);
    w.w.bytes(b"PROT");
    protos.write_as_read(&mut w.w);
    w.w.bytes(b"TX3D");
    w.texture3d(x);
    w.w.out
}

/// What the program writes for the content of a file that was read: the model, or the
/// material or texture at version 4.0.
pub fn write_content(c: &Content) -> Option<Vec<u8>> {
    match c {
        Content::Model(root) => Some(write_model(root)),
        Content::Material(m) => Some(write_material(m, VERSION_OUTPUT)),
        Content::Texture(x) => Some(write_texture(x, VERSION_OUTPUT)),
        Content::Texture3D(x) => Some(write_texture3d(x, VERSION_OUTPUT)),
        Content::None => None,
    }
}

/// Not something the program does: writes a file back with its own header and its own
/// prototype table, each token through the same code as [`write_content`]. If the readers
/// and writers agree on every field, the result equals the file up to where its record
/// ends. Returns the writer too, for the tokens its table has that this build never writes.
pub fn write_as_read<'p>(f: &'p File) -> Option<Writer<'p>> {
    let h = f.header.as_ref()?;
    let mut w = Writer::new(&f.protos);
    write_bin_header(&mut w.w.out, h.magic, h.version, Some(&h.comment));
    if f.has_prototypes {
        w.w.bytes(b"PROT");
        f.protos.write_as_read(&mut w.w);
    }
    match &f.content {
        Content::Model(root) => {
            w.w.bytes(HRCH);
            w.node(root, true, false);
        }
        Content::Material(m) => {
            w.w.bytes(b"MTRL");
            w.material(m, false);
        }
        Content::Texture(x) => {
            w.w.bytes(b"TXMP");
            w.texture(x);
        }
        Content::Texture3D(x) => {
            w.w.bytes(b"TX3D");
            w.texture3d(x);
        }
        Content::None => return None,
    }
    Some(w)
}
