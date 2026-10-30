// SPDX-License-Identifier: MIT
//! The geometry stage of the native renderer: source file `geometry.c` of the original
//! (25 functions, 0x08273910 to 0x0827657c of the Linux `soft` binary).
//!
//! Most of that file walks the program's model tree and the renderer's globals, which are not
//! this crate's business. What is here is the part that can be stated over the renderer's own
//! small records, whose layouts the file itself fixes:
//!
//! - the near and far plane clipper (a re-entrant Sutherland–Hodgman pipeline of two stages),
//! - the plane equation and projection axis of a triangle (`barr`),
//! - the two routines that file triangles under scan lines (`si_areaSort`, `si_pointSort`),
//! - the per-triangle bounding boxes of `traceList`,
//! - the arithmetic of the vertex transformation and of motion blur.
//!
//! Functions that reproduce a whole original function carry its citation. Functions described
//! as a "kernel of" an original reproduce only the stated arithmetic of it; the original
//! around them reads structures (`__SI_Model`, `__SI_Material`) that belong to other crates.
//!
//! Pointers of the original are indices here: a triangle names its corners by index into the
//! caller's vertex array, a list link is the index of the next triangle, a vertex's texture
//! coordinates are an index into a pool of (u, v) pairs. Memory the original takes from the
//! renderer's chunk allocator (`SI_chunkMalloc` at 0x0857cb64) is a `Vec` that grows.

use crate::types::*;
use crate::x87::{x, Round, F80};

// ------------------------------------------------------------------------------------ records

/// `SI_RVertex`: a vertex of the renderer, 0x4c bytes (the size `copyModel` and `clipOutput`
/// copy and every vertex loop steps by).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RVertex {
    /// +0x00: eight bytes this source file never looks at. They are copied with the record
    /// and are zero in a vertex the clipper makes. (Other source files' `SI_rvtxSetUV` and
    /// `SI_rvtxGetUV` use them as two floats.)
    pub w00: [u32; 2],
    /// +0x08: position in the model's own space, the input of `SI_rawToWorld`.
    pub raw: Vector,
    /// +0x14: normal in the model's own space, the input of `SI_rawToWorldVect`.
    pub raw_normal: Vector,
    /// +0x20: position after `SI_rawToWorld`. Its z is what the hither and yon planes cut.
    pub world: Vector,
    /// +0x2c: position on the picture plane.
    pub ps: Pspoint,
    /// +0x34: unit normal after `SI_rawToWorldVect`.
    pub normal: Vector,
    /// +0x40: pointer to the vertex's texture coordinates, pairs of floats. Here: the index
    /// of the first pair in the caller's pool.
    pub uv: u32,
    /// +0x44: four bytes this source file never looks at (`si_updateRVertex` of another file
    /// writes them one by one).
    pub w44: u32,
    /// +0x48: a pointer into a second per-vertex array of the model (`copyModel` rebases
    /// it). Opaque here.
    pub p48: u32,
}

/// `SI_Clipvertex`: a vertex on its way through the clipper, 100 bytes: the vertex and six
/// signed distances, one per clipping plane. Only the first two planes exist in this build.
/// The functions take it by value, which is how its size is known: 25 argument slots.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipVertex {
    pub v: RVertex,
    /// +0x4c: `d[0]` = yon − z, `d[1]` = z − hither; inside means not negative.
    pub d: [f32; 6],
}

/// Six consecutive floats of the renderer's global record `SI_database` (0x08790640), at
/// +0x170: the picture window and the depth range.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipBox {
    /// 0x087907b0
    pub xmin: f32,
    /// 0x087907b4
    pub xmax: f32,
    /// 0x087907b8
    pub ymin: f32,
    /// 0x087907bc
    pub ymax: f32,
    /// 0x087907c0: 0 - near (the renderer's set-up writes it so; finding 0012). The field's
    /// name is the wrong way round and stays until the records are consolidated: the code
    /// goes by address.
    pub yon: f32,
    /// 0x087907c4: 0 - far. The name is the wrong way round, as above.
    pub hither: f32,
}

/// `tn`: one triangle of the renderer's lists, 0x60 bytes. The members this source file
/// reads or writes; +0x54 to +0x57 and +0x5a to +0x5b are not touched by it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tn {
    /// +0x00: the corners, pointers to `SI_RVertex`. `None` is a null pointer.
    pub v: [Option<usize>; 3],
    /// +0x0c: the plane's normal, (v1 − v0) × (v2 − v0), not normalised.
    pub n: Vector,
    /// +0x18: `n` · v0.
    pub d: f32,
    /// +0x1c: 1 / `d`.
    pub inv_d: f32,
    /// +0x20: 1, 2 or 3: the axis on which `n` is largest in magnitude.
    pub axis: i16,
    /// +0x24: 1 / that component of `n`.
    pub inv_n: f32,
    /// +0x28, +0x34, +0x40: the edges v2 − v1, v0 − v2, v1 − v0.
    pub edge: [Vector; 3],
    /// +0x4c: pointer to the triangle's material. Opaque here.
    pub material: u32,
    /// +0x50: see the `TN_` constants.
    pub flags: u16,
    /// +0x52
    pub flags2: u16,
    /// +0x58: a word `clip` copies to the triangles it makes.
    pub w58: i16,
    /// +0x5c: the next triangle of the list.
    pub next: Option<usize>,
}

/// The triangle has no usable geometry (a null corner, or a zero normal): every routine of
/// the file skips it.
pub const TN_VOID: u16 = 0x8000;
/// `barr` drops a triangle carrying this bit when its plane faces away (`d` ≥ 0).
pub const TN_ONE_SIDED: u16 = 0x4000;
/// Set together with [`TN_DROPPED`] by `clip` for a material whose word at +0x1e is 6.
pub const TN_MATERIAL_6: u16 = 0x2000;
/// The triangle is not to be filed under scan lines: outside the view, replaced by clipped
/// pieces, facing away, or edge-on.
pub const TN_DROPPED: u16 = 0x1000;

/// The 0x28-byte record that files a triangle, or one of its two halves, under a scan line.
/// The original allocates it from the chunk allocator and writes these members; the words at
/// +0x00, +0x20 and +0x24 are left as allocated.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    /// +0x04: x of the left edge on the scan line the record is filed under.
    pub x0: f32,
    /// +0x08: what is added to `x0` from one scan line to the next one below.
    pub dx0: f32,
    /// +0x0c: x of the right edge, plus one.
    pub x1: f32,
    /// +0x10
    pub dx1: f32,
    /// +0x14: the last scan line.
    pub y_end: i32,
    /// +0x18: the triangle.
    pub tn: usize,
}

/// The scan-line table and what the two filing routines keep beside it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YBins {
    /// The array the pointer at 0x087908ec addresses: one list per scan line. The original
    /// links a new record in at the head; here it is pushed at the end, so the original's
    /// list order is each `Vec` read backwards.
    pub lines: Vec<Vec<Span>>,
    /// Where the original would have written outside the table: a scan line number below
    /// zero or beyond the table (this code does not know the table's size; the caller sets
    /// `lines`). The original has no check and corrupts memory.
    pub stray: Vec<(i32, Span)>,
    /// 0x0879090c: least x, greatest x, least y, greatest y of the triangles accepted.
    pub bounds: [f32; 4],
    /// The global `SI_stats` (0x087911e0) +0x00: triangles that passed the window test.
    pub accepted: i32,
    /// `SI_stats` +0x20: records made.
    pub made: i32,
}

impl YBins {
    fn link(&mut self, line: i32, span: Span) {
        self.made += 1;
        match usize::try_from(line).ok().and_then(|i| self.lines.get_mut(i)) {
            Some(list) => list.push(span),
            // Rust differs here: the original stores through the unchecked index.
            None => self.stray.push((line, span)),
        }
    }

    /// The window test and the bookkeeping both filing routines share. `false`: rejected.
    fn accept(&mut self, win: &ClipBox, xmin: f32, xmax: f32, y_bottom: f32, y_top: f32) -> bool {
        // each test is "jump away when st0 is strictly on the wrong side": a NaN passes
        if xmin > win.xmax || xmax < win.xmin || y_bottom > win.ymax || y_top < win.ymin {
            return false;
        }
        self.accepted += 1;
        let b = &mut self.bounds;
        // the stored value stays only when it is strictly beyond the new one
        b[0] = if b[0] < xmin { b[0] } else { xmin };
        b[1] = if b[1] > xmax { b[1] } else { xmax };
        b[2] = if b[2] < y_bottom { b[2] } else { y_bottom };
        b[3] = if b[3] > y_top { b[3] } else { y_top };
        true
    }
}

// ------------------------------------------------------------------- transForm (kernels only)

/// Whether `transForm` negates a model's normals before transforming them. Kernel of
/// `transForm` at 0x08273acc (not the whole function).
///
/// `scale` is the global scaling of the node the model's `latdep` pointer names (the vector
/// at +0x16c, or the one at +0x3a0 when the global `SI_glbManip` (0x08781560) is 1),
/// `right_hand` is the global `SI_rightHand` (0x086793ec) ≠ 0, `bit3` is bit 3 of the model's
/// flag word at +0x08.
///
/// The product (sx × sy) × sz is formed in the register and compared with zero: a negative
/// product (a mirroring scaling) flips in a right-handed setup, any other product (zero and
/// NaN included) flips in a left-handed one; `bit3` inverts the answer. Exactness: model.
pub fn transform_flips_normals(scale: Vector, right_hand: bool, bit3: bool) -> bool {
    let p = x(scale.x) * scale.y * scale.z;
    let flip = if F80::ZERO.gt(p) { right_hand } else { !right_hand };
    flip != bit3
}

/// The normalisation `transForm` applies to a normal that `SI_rawToWorldVect` has written.
/// Kernel of `transForm` at 0x08273acc.
///
/// length = √((y × y + x × x) + z × z) in the register; each component is divided by it and
/// rounded to float. Only a length that compares equal to zero leaves the vector alone: a
/// NaN length divides. Exactness: model.
pub fn unit_normal(n: Vector) -> Vector {
    let (nx, ny, nz) = (x(n.x), x(n.y), x(n.z));
    let len = ((ny * ny + nx * nx) + nz * nz).sqrt();
    if F80::ZERO.eq(len) {
        return n;
    }
    Vector::new((nx / len).to_f32(), (ny / len).to_f32(), (nz / len).to_f32())
}

/// The normal `transForm` gives all three corners of a triangle when the model's matrix has
/// no inverse. Kernel of `transForm` at 0x08273acc.
///
/// With e = c − b and g = a − b (a, b, c the transformed positions of corners 0, 1, 2, the
/// differences kept in the register): n = e × g, then n / |n| unless |n| compares equal to
/// zero, |n| = √(nz² + (ny² + nx²)).
///
/// One operand is not like the others: in ny = ez × gx − gz × ex the factor ex is the value
/// that was stored to a float local and loaded back, so it is rounded to float, while every
/// other difference is used with its full register precision. Exactness: model.
pub fn face_normal(a: Vector, b: Vector, c: Vector) -> Vector {
    let (ex, ey, ez) = (x(c.x) - b.x, x(c.y) - b.y, x(c.z) - b.z);
    let (gx, gy, gz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
    let nx = ey * gz - ez * gy;
    let ny = ez * gx - gz * ex.to_f32();
    let nz = gy * ex - gx * ey;
    let len = (nz * nz + (ny * ny + nx * nx)).sqrt();
    if F80::ZERO.eq(len) {
        return Vector::new(nx.to_f32(), ny.to_f32(), nz.to_f32());
    }
    Vector::new((nx / len).to_f32(), (ny / len).to_f32(), (nz / len).to_f32())
}

/// A position moved along its normal: amount × normal + position, each component formed in
/// the register and rounded once. Kernel of `transForm` at 0x08273acc, which does this to
/// every vertex (and projects it again) when the model's first material has its 16-bit
/// switch at +0x758 set; `amount` is that material's float at +0x75c. Exactness: model.
pub fn pushed_along_normal(world: Vector, normal: Vector, amount: f32) -> Vector {
    let k = x(amount);
    Vector::new(
        (k * normal.x + world.x).to_f32(),
        (k * normal.y + world.y).to_f32(),
        (k * normal.z + world.z).to_f32(),
    )
}

// --------------------------------------------------------------------------------- clipping

/// Least and greatest of three floats as `clip` and `traceList` find them: compare the second
/// with the first, then the third with the winner; on a tie or a NaN the later value wins.
/// Exactness: copy (comparisons of floats are exact).
pub fn extent3(a: f32, b: f32, c: f32) -> (f32, f32) {
    let m = if b > a { a } else { b };
    let lo = if c > m { m } else { c };
    let m = if b < a { a } else { b };
    let hi = if c < m { m } else { c };
    (lo, hi)
}

/// What `clip` decides for one triangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipFate {
    /// Wholly outside the window or the depth range: the triangle gets [`TN_DROPPED`].
    Reject,
    /// A depth plane passes strictly between its least and greatest z: it is cut, replaced
    /// by a fan of new triangles and gets [`TN_DROPPED`].
    Cut,
    /// Left as it is.
    Keep,
}

/// The decision `clip` takes for a triangle with three corners. Kernel of `clip` at
/// 0x0827400c (not the whole function, which also sets the triangle's flag words from its
/// model and material and builds the clipped pieces).
///
/// The extents are those of the corners' picture-plane x and y and of their `world.z`
/// ([`extent3`]). Rejected when the window's xmax < least x, xmin > greatest x, ymax < least
/// y, ymin > greatest y, hither > greatest z or yon < least z. Otherwise cut when
/// least z < yon < greatest z or least z < hither < greatest z, all strict. A triangle that
/// only touches a depth plane is kept whole. Exactness: copy.
pub fn clip_fate(v: [&RVertex; 3], b: &ClipBox) -> ClipFate {
    let (xlo, xhi) = extent3(v[0].ps.x, v[1].ps.x, v[2].ps.x);
    let (ylo, yhi) = extent3(v[0].ps.y, v[1].ps.y, v[2].ps.y);
    let (zlo, zhi) = extent3(v[0].world.z, v[1].world.z, v[2].world.z);
    if b.xmax < xlo || b.xmin > xhi || b.ymax < ylo || b.ymin > yhi || b.hither > zhi || b.yon < zlo {
        return ClipFate::Reject;
    }
    if (b.yon > zlo && b.yon < zhi) || (zlo < b.hither && b.hither < zhi) {
        return ClipFate::Cut;
    }
    ClipFate::Keep
}

/// Makes a clip vertex of a vertex: the record and its distances to the two depth planes,
/// `d[1]` = z − hither and `d[0]` = yon − z (z is `world.z`), each one float subtraction.
/// [soft 0x082745ec extend]
///
/// The original returns the 100-byte record through a hidden pointer and fills `d[2]` to
/// `d[5]` with whatever its stack held; nothing reads them. Rust differs there: they are zero.
/// Exactness: single.
pub fn extend(v: &RVertex, b: &ClipBox) -> ClipVertex {
    let mut c = ClipVertex { v: *v, d: [0.0; 6] };
    // through the model, not plain `f32` arithmetic: with two NaN operands the unit keeps
    // the one with the larger significand, which a host subtraction does not promise
    c.d[1] = (x(v.world.z) - b.hither).to_f32();
    c.d[0] = (x(b.yon) - v.world.z).to_f32();
    c
}

/// What the clipper needs from outside its own state.
pub struct ClipEnv<'a> {
    /// The depth planes (the window members are not used).
    pub planes: &'a ClipBox,
    /// The pool of texture coordinate pairs that [`RVertex::uv`] indexes. New vertices get
    /// new pairs at its end.
    pub uv: &'a mut Vec<[f32; 2]>,
    /// `SI_worldToPrsp2` (0x08279734, in `matrix.c`): the picture-plane position of a point.
    pub project: &'a mut dyn FnMut(&Vector) -> Pspoint,
}

/// The clipper's state: the file-scope variables `flag`, `f`, `s`, `nMap`, `nOutput` and
/// `outputVerts` of the original, each sized for six planes there as here.
#[derive(Clone, Debug, PartialEq)]
pub struct Clipper {
    /// 0x08799538: per stage, non-zero while no vertex has reached it.
    pub flag: [i16; 6],
    /// 0x087992e0: per stage, the first vertex that reached it.
    pub f: [ClipVertex; 6],
    /// 0x08799560: per stage, the latest vertex that reached it.
    pub s: [ClipVertex; 6],
    /// 0x08799544: how many texture coordinate pairs a vertex has.
    pub n_map: i16,
    /// 0x08799548 and 0x08799546: the vertices that came out of the last stage, in order.
    /// The original keeps six pointers and a count; a triangle cut by two planes gives five
    /// at most.
    pub output: Vec<RVertex>,
    /// `SI_stats` +0x14: vertices made.
    pub made_vertices: i32,
    /// `SI_stats` +0x18: texture coordinate pairs made.
    pub made_pairs: i32,
}

impl Default for Clipper {
    fn default() -> Self {
        Clipper {
            flag: [0; 6],
            f: [ClipVertex::default(); 6],
            s: [ClipVertex::default(); 6],
            n_map: 0,
            output: Vec::new(),
            made_vertices: 0,
            made_pairs: 0,
        }
    }
}

impl Clipper {
    /// Feeds one vertex to stage `n` (0: the yon plane, 1: the hither plane).
    /// [soft 0x082744a8 shClip]
    ///
    /// The first vertex of a stage is only remembered. For a later one, when the product of
    /// its distance and the previous vertex's distance, formed in the register, is below
    /// zero, the crossing point [`Clipper::clip_intersect`]`(p, previous)` is passed on first.
    /// Then `p` becomes the previous vertex, and is passed on itself when its distance is
    /// not negative (a NaN distance is not passed on). A vertex exactly on the plane is
    /// inside and never causes a crossing. Exactness: model (the product cannot underflow
    /// in the register as it could in a float).
    ///
    /// `n` indexes the state unchecked in the original; here an `n` outside 0..6 panics.
    pub fn sh_clip(&mut self, p: ClipVertex, n: i16, env: &mut ClipEnv<'_>) {
        let i = n as usize;
        if self.flag[i] != 0 {
            self.flag[i] = 0;
            self.f[i] = p;
        } else if F80::ZERO.gt(x(p.d[i]) * self.s[i].d[i]) {
            let previous = self.s[i];
            let q = self.clip_intersect(&p, &previous, n, env);
            self.clip_output(q, n, env);
        }
        self.s[i] = p;
        if p.d[i] >= 0.0 {
            self.clip_output(p, n, env);
        }
    }

    /// The point where the segment from `a` to `b` crosses plane `n`, as a new vertex.
    /// [soft 0x08274638 clipIntersect]
    ///
    /// t = a.d / (a.d − b.d), formed in the register and **rounded to float**. Every
    /// interpolated member is then a + (b − a) × t with the difference, the product and the
    /// sum in the register and one rounding to float: the position, then (after the
    /// projection of the position by `SI_worldToPrsp2`) the normal, then the texture
    /// coordinates. The normal is divided by its length √(z² + (x² + y²)), taken of the
    /// unrounded components, unless that length compares equal to zero. Last, the two
    /// distances are computed afresh from the new z as in [`extend`].
    ///
    /// The interpolation runs from `a`: `shClip` passes the new vertex as `a` and the
    /// previous one as `b`, `closeClip` the first as `a` and the last as `b`, so the same
    /// edge cut from its two ends gives different last bits.
    ///
    /// Everything else of the new record is zero: the model-space position and normal, the
    /// words at +0x00, +0x44 and +0x48, and `d[2]` to `d[5]`. Exactness: model.
    ///
    /// Rust differs in one place: the original reads `n_map` pairs through each vertex's
    /// pointer whatever lies there; here a vertex whose pairs are not in the pool panics.
    pub fn clip_intersect(&mut self, a: &ClipVertex, b: &ClipVertex, n: i16, env: &mut ClipEnv<'_>) -> ClipVertex {
        let i = n as usize;
        let t = (x(a.d[i]) / (x(a.d[i]) - b.d[i])).to_f32();
        let mix = |p: f32, q: f32| (x(q) - p) * t + p;
        let mut r = ClipVertex::default();
        r.v.world = Vector::new(
            mix(a.v.world.x, b.v.world.x).to_f32(),
            mix(a.v.world.y, b.v.world.y).to_f32(),
            mix(a.v.world.z, b.v.world.z).to_f32(),
        );
        r.v.ps = (env.project)(&r.v.world);
        let nx = mix(a.v.normal.x, b.v.normal.x);
        let ny = mix(a.v.normal.y, b.v.normal.y);
        let nz = mix(a.v.normal.z, b.v.normal.z);
        let len = (nz * nz + (nx * nx + ny * ny)).sqrt();
        r.v.normal = if F80::ZERO.eq(len) {
            Vector::new(nx.to_f32(), ny.to_f32(), nz.to_f32())
        } else {
            Vector::new((nx / len).to_f32(), (ny / len).to_f32(), (nz / len).to_f32())
        };
        self.made_pairs = self.made_pairs.wrapping_add(self.n_map as i32);
        r.v.uv = env.uv.len() as u32;
        for k in 0..self.n_map.max(0) as usize {
            let p = env.uv[a.v.uv as usize + k];
            let q = env.uv[b.v.uv as usize + k];
            env.uv.push([mix(p[0], q[0]).to_f32(), mix(p[1], q[1]).to_f32()]);
        }
        r.d[1] = (x(r.v.world.z) - env.planes.hither).to_f32();
        r.d[0] = (x(env.planes.yon) - r.v.world.z).to_f32();
        r
    }

    /// Passes a vertex that survived stage `n` on: to the next stage, or after stage 1 to
    /// the output. [soft 0x082747fc clipOutput]
    ///
    /// The output keeps the vertex part only (0x4c bytes, a new chunk allocation in the
    /// original), so the distances are gone. Exactness: copy.
    pub fn clip_output(&mut self, p: ClipVertex, n: i16, env: &mut ClipEnv<'_>) {
        if n == 1 {
            self.made_vertices += 1;
            self.output.push(p.v);
        } else {
            self.sh_clip(p, n.wrapping_add(1), env);
        }
    }

    /// Closes the polygon: for stage `n` and then every later stage, when the stage has seen
    /// a vertex and its first and latest vertices lie on different sides (the same product
    /// test as in [`Clipper::sh_clip`]), the crossing point
    /// [`Clipper::clip_intersect`]`(first, latest)` is passed on; the stage is then marked
    /// empty again. [soft 0x08274874 closeClip]
    ///
    /// Stage `n` is closed before stage `n` + 1, so the closing point of the first plane
    /// still passes through the second. Exactness: model.
    pub fn close_clip(&mut self, n: i16, env: &mut ClipEnv<'_>) {
        let i = n as usize;
        if self.flag[i] == 0 && F80::ZERO.gt(x(self.f[i].d[i]) * self.s[i].d[i]) {
            let (first, latest) = (self.f[i], self.s[i]);
            let q = self.clip_intersect(&first, &latest, n, env);
            self.clip_output(q, n, env);
        }
        self.flag[i] = 1;
        if n != 1 {
            self.close_clip(n.wrapping_add(1), env);
        }
    }

    /// Cuts one triangle the way `clip` does once it has decided to: both stages are marked
    /// empty, `n_map` is set, the output is emptied, the three corners go through
    /// [`extend`] and [`Clipper::sh_clip`] in order, and the polygon is closed. Kernel of
    /// `clip` at 0x0827400c. The result is the clipped polygon in [`Clipper::output`];
    /// [`clip_fan`] says which triangles `clip` makes of it.
    pub fn clip_triangle(&mut self, v: [&RVertex; 3], n_map: i16, env: &mut ClipEnv<'_>) {
        self.flag[0] = 1;
        self.flag[1] = 1;
        self.n_map = n_map;
        self.output.clear();
        for p in v {
            let c = extend(p, env.planes);
            self.sh_clip(c, 0, env);
        }
        self.close_clip(0, env);
    }
}

/// The triangles `clip` makes of a clipped polygon of `n_output` vertices: none when there
/// are two or fewer, else the fan (0, i, i + 1) for i = 1 to `n_output` − 2, in that order.
/// Kernel of `clip` at 0x0827400c.
///
/// (With two or fewer vertices the original leaves the uncut triangle in the list, unmarked.)
pub fn clip_fan(n_output: i16) -> impl Iterator<Item = [usize; 3]> {
    let last = if n_output > 2 { n_output as usize - 1 } else { 0 };
    (1..last.max(1)).map(|i| [0, i, i + 1])
}

// -------------------------------------------------------------------------------- traceList

/// One record of the list `traceList` builds, 0x20 bytes in the original: the triangle's
/// bounding box (least x, greatest x, least y, greatest y, least z, greatest z of its
/// corners' `world` positions), the triangle (+0x18) and the link (+0x1c).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraceItem {
    pub bounds: [f32; 6],
    pub tn: usize,
}

/// What `traceList` adds to: six floats of `SI_database` at +0x2b4 (0x087908f4) and the list
/// whose head is at +0x2e4 (0x08790924).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceList {
    /// The box around everything listed, same order as [`TraceItem::bounds`].
    pub bounds: [f32; 6],
    /// The original links each record in at the head: its list order is this read backwards.
    pub items: Vec<TraceItem>,
    /// `SI_stats` +0x24: records made.
    pub made: i32,
}

/// Lists every triangle of a list with its bounding box (by the function's name, for the ray
/// tracer) and widens the box around all of them. [soft 0x08274978 traceList]
///
/// Per triangle the box is three [`extent3`]. The overall box keeps its stored value only
/// when that is strictly beyond the triangle's (so a NaN replaces it), except its greatest z,
/// where the comparison is taken from the other side to the same effect.
///
/// No flag is looked at: a triangle marked [`TN_VOID`] for a null corner makes the original
/// read through a null pointer. Rust panics there instead. Exactness: copy.
pub fn trace_list(tns: &[Tn], head: Option<usize>, verts: &[RVertex], out: &mut TraceList) {
    let mut cur = head;
    while let Some(i) = cur {
        let tn = &tns[i];
        cur = tn.next;
        let [a, b, c] = corners(tn, verts).map(|v| v.world);
        let (x0, x1) = extent3(a.x, b.x, c.x);
        let (y0, y1) = extent3(a.y, b.y, c.y);
        let (z0, z1) = extent3(a.z, b.z, c.z);
        let g = &mut out.bounds;
        g[0] = if x0 > g[0] { g[0] } else { x0 };
        g[1] = if x1 < g[1] { g[1] } else { x1 };
        g[2] = if y0 > g[2] { g[2] } else { y0 };
        g[3] = if y1 < g[3] { g[3] } else { y1 };
        g[4] = if z0 > g[4] { g[4] } else { z0 };
        g[5] = if g[5] > z1 { g[5] } else { z1 };
        out.made += 1;
        out.items.push(TraceItem { bounds: [x0, x1, y0, y1, z0, z1], tn: i });
    }
}

fn corners<'a>(tn: &Tn, verts: &'a [RVertex]) -> [&'a RVertex; 3] {
    // Rust differs here: the original follows the three pointers without a check
    tn.v.map(|k| &verts[k.expect("triangle with a null corner")])
}

// ------------------------------------------------------------------------------------- barr

/// Computes the plane and the edge vectors of every triangle of a list that is not marked
/// [`TN_VOID`]. [soft 0x08274bac barr]
///
/// With a, b, c the `world` positions of the corners and p = b − a, q = c − a:
///
/// - n.x = py × qz − pz × qy with all four differences at register precision;
/// - n.y = qx × pz − px × qz and n.z = px × qy − qx × py with the six differences **rounded to
///   float first** (they are loaded back from float locals);
/// - d = n.z × a.z + (n.y × a.y + a.x × n.x), with the three components not yet rounded;
/// - when |n.z| + (|n.y| + |n.x|) is below 1e-6 (the double at 0x08654548; n.y and n.z as
///   stored, n.x unrounded) the triangle gets `TN_VOID | TN_DROPPED` and nothing more;
/// - a [`TN_ONE_SIDED`] triangle with d ≥ 0 gets [`TN_DROPPED`];
/// - when |d| is below 1e-6 the triangle gets [`TN_DROPPED`] and `inv_d` is left alone, else
///   `inv_d` = 1 / d;
/// - the edges c − b, a − c, b − a, one float subtraction per component;
/// - `axis` = 1 when |n.x| ≥ |n.y| and |n.x| ≥ |n.z|, else 2 when |n.y| ≥ |n.x| and
///   |n.y| ≥ |n.z|, else 3 (a NaN among them gives 3); `inv_n` = 1 / that component.
///
/// A triangle already marked [`TN_DROPPED`] is still computed. Exactness: model.
pub fn barr(tns: &mut [Tn], head: Option<usize>, verts: &[RVertex]) {
    let eps = x(1e-6f64);
    let mut cur = head;
    while let Some(i) = cur {
        let tn = &mut tns[i];
        cur = tn.next;
        if (tn.flags as i16) < 0 {
            continue;
        }
        let [a, b, c] = corners(tn, verts).map(|v| v.world);
        let px = (x(b.x) - a.x).to_f32();
        let py = x(b.y) - a.y;
        let pz = x(b.z) - a.z;
        let qx = (x(c.x) - a.x).to_f32();
        let qy = x(c.y) - a.y;
        let qz = x(c.z) - a.z;
        let nx = py * qz - pz * qy;
        let ny = x(qx) * pz.to_f32() - x(px) * qz.to_f32();
        let nz = x(px) * qy.to_f32() - x(qx) * py.to_f32();
        tn.n = Vector::new(nx.to_f32(), ny.to_f32(), nz.to_f32());
        tn.d = (nz * a.z + (ny * a.y + x(a.x) * nx)).to_f32();
        let size = x(tn.n.z).abs() + (x(tn.n.y).abs() + nx.abs());
        if size.lt(eps) {
            tn.flags |= TN_VOID | TN_DROPPED;
            continue;
        }
        if tn.flags & TN_ONE_SIDED != 0 && tn.d >= 0.0 {
            tn.flags |= TN_DROPPED;
        }
        if x(tn.d).abs().lt(eps) {
            tn.flags |= TN_DROPPED;
        } else {
            tn.inv_d = (F80::ONE / tn.d).to_f32();
        }
        // one subtraction per component, on the unit: for two NaN operands the result is
        // the NaN with the larger significand (found by the emulation check, finding 0011;
        // plain `f32` subtraction gave the first operand's)
        let sub = |p: Vector, q: Vector| Vector::new((x(p.x) - q.x).to_f32(), (x(p.y) - q.y).to_f32(), (x(p.z) - q.z).to_f32());
        tn.edge = [sub(c, b), sub(a, c), sub(b, a)];
        let (ax, ay, az) = (tn.n.x.abs(), tn.n.y.abs(), tn.n.z.abs());
        if ax >= ay && ax >= az {
            tn.axis = 1;
            tn.inv_n = (F80::ONE / tn.n.x).to_f32();
        } else if ay >= ax && ay >= az {
            tn.axis = 2;
            tn.inv_n = (F80::ONE / tn.n.y).to_f32();
        } else {
            tn.axis = 3;
            tn.inv_n = (F80::ONE / tn.n.z).to_f32();
        }
    }
}

// -------------------------------------------------------------- si_areaSort and si_pointSort

/// The top and bottom corners (greatest and least picture-plane y) as `si_areaSort` picks
/// them, as indices 0..3. Kernel of `si_areaSort` at 0x08274dac.
///
/// Ties: corner 0 is the top when neither other corner is above it; of two equal candidates
/// for the bottom the later corner wins. With a NaN among the three the answer is still two
/// different corners, whichever the comparisons below fall to.
pub fn area_sort_order(y: [f32; 3]) -> (usize, usize) {
    let [y0, y1, y2] = y;
    if y1 <= y0 && y2 <= y0 {
        if y1 >= y2 {
            (0, 2)
        } else {
            (0, 1)
        }
    } else if y1 >= y0 && y2 <= y1 {
        if y2 <= y0 {
            (1, 2)
        } else {
            (1, 0)
        }
    } else if y1 <= y0 {
        (2, 1)
    } else {
        (2, 0)
    }
}

/// The top, middle and bottom corners by picture-plane y as `si_pointSort` orders them.
/// Kernel of `si_pointSort` at 0x082750dc. The comparisons differ from
/// [`area_sort_order`]'s, so ties and NaNs fall differently.
pub fn point_sort_order(y: [f32; 3]) -> (usize, usize, usize) {
    let [y0, y1, y2] = y;
    if y1 <= y0 && y2 <= y0 {
        if y1 >= y2 {
            (0, 1, 2)
        } else {
            (0, 2, 1)
        }
    } else if y0 <= y1 && y2 <= y1 {
        if y0 >= y2 {
            (1, 0, 2)
        } else {
            (1, 2, 0)
        }
    } else if y0 >= y1 {
        (2, 0, 1)
    } else {
        (2, 1, 0)
    }
}

/// Least and greatest x of three corners as both filing routines find them: the first two
/// are compared, then the winner with the third; the later value wins a tie or a NaN.
fn sort_extent(x0: f32, x1: f32, x2: f32) -> (f32, f32) {
    let m = if x0 < x1 { x0 } else { x1 };
    let lo = if m < x2 { m } else { x2 };
    let m = if x0 > x1 { x0 } else { x1 };
    let hi = if m > x2 { m } else { x2 };
    (lo, hi)
}

/// 0.0001, the double at 0x08654550 and again at 0x08654558.
const FLAT: f64 = 0.0001;
/// 2.0, the double at 0x08654560: a height below which an edge is not followed.
const LOW: f64 = 2.0;

/// The scan line a record is filed under: the top scan line of the window, or the one above
/// the given y if that is lower. Both are C casts to `int` (truncations); y + 1 is formed in
/// the register, so 0.99999994 gives 1 and not 2.
fn top_line(ymax_line: i32, y: f32) -> i32 {
    ymax_line.min((x(y) + F80::ONE).to_i32(Round::Zero))
}

fn trunc(v: f32) -> i32 {
    x(v).to_i32(Round::Zero)
}

/// A record that covers a whole box: no slopes, x from the least x to the greatest x + 1.
fn box_span(lo: f32, hi: F80, y_end: i32, tn: usize) -> Span {
    Span { x0: lo, dx0: 0.0, x1: (hi + F80::ONE).to_f32(), dx1: 0.0, y_end, tn }
}

/// Files every triangle of a list under a scan line as its bounding box.
/// [soft 0x08274dac si_areaSort]
///
/// This is not a sort by comparison: each triangle goes to the head of the list of one scan
/// line of [`YBins::lines`], so within a scan line the triangles end up in the reverse of the
/// order they arrived in, and nothing orders them by x or depth.
///
/// Per triangle not marked [`TN_DROPPED`] or [`TN_VOID`]: top and bottom corner by
/// [`area_sort_order`], x extent; rejected when wholly outside the window (strict
/// comparisons, the window's x and y limits); counted and added to the overall bounds;
/// dropped when its height (top y − bottom y, in the register) is below 0.0001; else one
/// record: x0 = least x, x1 = greatest x + 1, no slopes, last line = bottom y truncated,
/// filed under min(window ymax truncated, (top y + 1) truncated).
///
/// The scan line number is not checked against the table (see [`YBins::stray`]): a top y of
/// −2 or less, or a NaN corner, writes outside it in the original. Exactness: model.
pub fn area_sort(tns: &[Tn], head: Option<usize>, verts: &[RVertex], win: &ClipBox, bins: &mut YBins) {
    let ymax_line = trunc(win.ymax);
    let mut cur = head;
    while let Some(i) = cur {
        let tn = &tns[i];
        cur = tn.next;
        if tn.flags & TN_DROPPED != 0 || (tn.flags as i16) < 0 {
            continue;
        }
        let p = corners(tn, verts).map(|v| v.ps);
        let (t, b) = area_sort_order([p[0].y, p[1].y, p[2].y]);
        let (lo, hi) = sort_extent(p[0].x, p[1].x, p[2].x);
        let (yt, yb) = (p[t].y, p[b].y);
        if !bins.accept(win, lo, hi, yb, yt) {
            continue;
        }
        if (x(yt) - yb).lt(x(FLAT)) {
            continue;
        }
        bins.link(top_line(ymax_line, yt), box_span(lo, x(hi), trunc(yb), i));
    }
}

/// Files every triangle of a list under scan lines as one or two trapezoids.
/// [soft 0x082750dc si_pointSort]
///
/// Like [`area_sort`] up to the height test (the corner order is [`point_sort_order`]). With
/// T, M, B the top, middle and bottom corners and dy = T.y − B.y (kept in the register):
///
/// - dy < 0.0001: nothing. dy < 2: the bounding box record of [`area_sort`].
/// - |M.y − B.y| < 0.0001 (flat bottom): one record from T. The edge to the corner with the
///   smaller x of B and M is the left one (B on a tie or NaN goes right); slope = (that x −
///   T.x) / dy; with h = T.y − the scan line it is filed under, x0 = slope × h + T.x and
///   x1 = 1 + (slope × h + T.x) for the right edge. Last line = B.y truncated.
/// - |T.y − M.y| < 0.0001 (flat top): one record to B. When T.x > M.x the left edge starts
///   at M and the right at T, else the other way round; slope = (B.x − start x) / dy,
///   x = slope × h + start x.
/// - otherwise two records. xc = (T.x − B.x) × ((M.y − B.y) / dy) + B.x is the long edge's x
///   at M's height, kept in the register.
///   * Upper, filed like the others, last line = (1 + M.y) truncated. When T.y − M.y < 2 it
///     is the box of T.x, M.x and xc. Else slopes (M.x − T.x) / (T.y − M.y) and
///     (xc − T.x) / (T.y − M.y), the one towards the smaller of M.x and xc on the left
///     (xc on the left only when M.x > xc strictly), x from T as above.
///   * Lower, made only when M.y truncated is not negative, filed under min(window ymax
///     truncated, M.y truncated), last line = B.y truncated. When M.y − B.y < 2 it is the
///     box of M.x, B.x and xc. Else slopes (B.x − M.x) / (M.y − B.y) and (B.x − xc) / (M.y −
///     B.y), M on the left only when M.x < xc strictly, x = slope × (M.y − its scan line) +
///     start x.
///
/// Every slope is stored rounded to float but used unrounded for the x of the same record;
/// each stored x is one rounding of slope × h + start (+ 1 on the right, added last).
/// Exactness: model.
pub fn point_sort(tns: &[Tn], head: Option<usize>, verts: &[RVertex], win: &ClipBox, bins: &mut YBins) {
    let ymax_line = trunc(win.ymax);
    let mut cur = head;
    while let Some(i) = cur {
        let tn = &tns[i];
        cur = tn.next;
        if tn.flags & TN_DROPPED != 0 || (tn.flags as i16) < 0 {
            continue;
        }
        let p = corners(tn, verts).map(|v| v.ps);
        let (t, m, b) = point_sort_order([p[0].y, p[1].y, p[2].y]);
        let (lo, hi) = sort_extent(p[0].x, p[1].x, p[2].x);
        let (top, mid, bot) = (p[t], p[m], p[b]);
        if !bins.accept(win, lo, hi, bot.y, top.y) {
            continue;
        }
        let dy = x(top.y) - bot.y;
        if dy.lt(x(FLAT)) {
            continue;
        }
        let line = top_line(ymax_line, top.y);
        if dy.lt(x(LOW)) {
            bins.link(line, box_span(lo, x(hi), trunc(bot.y), i));
            continue;
        }
        let h = x(top.y) - F80::from_i32(line);
        // one record with both edges running the whole height
        let whole = |left: (F80, f32), right: (F80, f32)| Span {
            x0: (left.0 * h + left.1).to_f32(),
            dx0: left.0.to_f32(),
            x1: (F80::ONE + (right.0 * h + right.1)).to_f32(),
            dx1: right.0.to_f32(),
            y_end: trunc(bot.y),
            tn: i,
        };
        if (x(mid.y) - bot.y).abs().lt(x(FLAT)) {
            let to_b = ((x(bot.x) - top.x) / dy, top.x);
            let to_m = ((x(mid.x) - top.x) / dy, top.x);
            let span = if bot.x < mid.x { whole(to_b, to_m) } else { whole(to_m, to_b) };
            bins.link(line, span);
            continue;
        }
        if (x(top.y) - mid.y).abs().lt(x(FLAT)) {
            let from_m = ((x(bot.x) - mid.x) / dy, mid.x);
            let from_t = ((x(bot.x) - top.x) / dy, top.x);
            let span = if top.x > mid.x { whole(from_m, from_t) } else { whole(from_t, from_m) };
            bins.link(line, span);
            continue;
        }
        let xc = (x(top.x) - bot.x) * ((x(mid.y) - bot.y) / dy) + bot.x;
        // upper half: from the top corner down to the middle one
        let h1 = x(top.y) - mid.y;
        let y_end = (F80::ONE + mid.y).to_i32(Round::Zero);
        let upper = if h1.lt(x(LOW)) {
            let (lo, hi) = extent_with(top.x, mid.x, xc);
            Span { x0: lo, dx0: 0.0, x1: (hi + F80::ONE).to_f32(), dx1: 0.0, y_end, tn: i }
        } else {
            let to_c = (xc - top.x) / h1;
            let to_m = (x(mid.x) - top.x) / h1;
            let (l, r) = if x(mid.x).gt(xc) { (to_c, to_m) } else { (to_m, to_c) };
            Span {
                x0: (l * h + top.x).to_f32(),
                dx0: l.to_f32(),
                x1: (F80::ONE + (r * h + top.x)).to_f32(),
                dx1: r.to_f32(),
                y_end,
                tn: i,
            }
        };
        bins.link(line, upper);
        // lower half: from the middle corner down to the bottom one
        let mid_line = trunc(mid.y);
        if mid_line < 0 {
            continue;
        }
        let line2 = ymax_line.min(mid_line);
        let hm = x(mid.y) - F80::from_i32(line2);
        let h2 = x(mid.y) - bot.y;
        let y_end = trunc(bot.y);
        let lower = if h2.lt(x(LOW)) {
            let (lo, hi) = extent_with(mid.x, bot.x, xc);
            Span { x0: lo, dx0: 0.0, x1: (hi + F80::ONE).to_f32(), dx1: 0.0, y_end, tn: i }
        } else {
            let from_m = (x(bot.x) - mid.x) / h2;
            let from_c = (x(bot.x) - xc) / h2;
            if x(mid.x).lt(xc) {
                Span {
                    x0: (from_m * hm + mid.x).to_f32(),
                    dx0: from_m.to_f32(),
                    x1: ((from_c * hm + xc) + F80::ONE).to_f32(),
                    dx1: from_c.to_f32(),
                    y_end,
                    tn: i,
                }
            } else {
                Span {
                    x0: (xc + from_c * hm).to_f32(),
                    dx0: from_c.to_f32(),
                    x1: ((hm * from_m + mid.x) + F80::ONE).to_f32(),
                    dx1: from_m.to_f32(),
                    y_end,
                    tn: i,
                }
            }
        };
        bins.link(line2, lower);
    }
}

/// Least (rounded to float, as it is stored) and greatest (as it stands in the register) of
/// two floats `p`, `q` and the register value `c`, as `si_pointSort` finds them for a half
/// lower than two scan lines. `c` is the least only when both floats are strictly above it,
/// the greatest only when it is strictly above both.
fn extent_with(p: f32, q: f32, c: F80) -> (f32, F80) {
    let lo = if x(p).gt(c) {
        if x(q).gt(c) {
            c.to_f32()
        } else {
            q
        }
    } else if p < q {
        p
    } else {
        q
    };
    let hi = if c.gt(x(p)) {
        if c.gt(x(q)) {
            c
        } else {
            x(q)
        }
    } else if p > q {
        x(p)
    } else {
        x(q)
    };
    (lo, hi)
}

// ------------------------------------------------------------------------------ motion blur

/// The time at which the shutter closes: shutter / frame rate + `time`, the quotient and the
/// sum in the register, rounded once. Kernel of `si_blurGeometry` at 0x08275a9c; `shutter`
/// is the float at 0x08790df8 (the global `SI_ren` +0x218), `frame_rate` is the global
/// `SI_frameRate` (0x0878204c). Exactness: model.
pub fn blur_end_time(time: f32, shutter: f32, frame_rate: f32) -> f32 {
    (x(shutter) / frame_rate + time).to_f32()
}

/// The greatest distance any vertex moved on the picture plane between two states of a
/// model, or −1 when there is nothing to compare. [soft 0x08276250 maxPsMovement]
///
/// `None` stands for a null model pointer or a null vertex array (+0x22c); the original also
/// answers −1 (the float at 0x08654598) when the two vertex counts (+0x228) differ or are
/// not positive. Per vertex: √(dx × dx + dy × dy) of the `ps` members in the register, and
/// the running maximum is replaced only by a strictly greater value, so a NaN distance is
/// ignored.
///
/// The result is the register value, **not rounded**: `isMoving` compares it as it is with
/// its threshold ([`moved_enough`]) and only then stores it as a float. Exactness: model.
pub fn max_ps_movement(now: Option<&[RVertex]>, before: Option<&[RVertex]>) -> F80 {
    let mut most = x(-1.0f32);
    let (Some(now), Some(before)) = (now, before) else { return most };
    if now.len() != before.len() {
        return most;
    }
    for (p, q) in now.iter().zip(before) {
        let dx = x(p.ps.x) - q.ps.x;
        let dy = x(p.ps.y) - q.ps.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist.gt(most) {
            most = dist;
        }
    }
    most
}

/// What `isMoving` does with the result of [`max_ps_movement`]: when it is strictly above
/// the threshold (the float at 0x08790e00, `SI_ren` +0x220) the model is marked as moving
/// (the word at +0x338 becomes 1) and the value, rounded to float, is stored at +0x340.
/// Kernel of `isMoving` at 0x08275bf0.
pub fn moved_enough(movement: F80, threshold: f32) -> Option<f32> {
    if movement.gt(x(threshold)) {
        Some(movement.to_f32())
    } else {
        None
    }
}

/// The time step between the copies `blurTraverse` draws of a moving model, and the value it
/// hands to `setExistance`. Kernel of `blurTraverse` at 0x08275ce0.
///
/// span = `end` − `start`; step = span / `movement` × `threshold`, all in the register
/// (`movement` is the model's float at +0x340, `threshold` the float at 0x08790e00). The
/// step is stored rounded to float; the second value is the **unrounded** step divided by
/// the span, rounded to float. Exactness: model.
pub fn blur_step(start: f32, end: f32, movement: f32, threshold: f32) -> (f32, f32) {
    let span = x(end) - start;
    let step = span / movement * threshold;
    (step.to_f32(), (step / span).to_f32())
}

/// The times, after `start` and `end` themselves, at which `blurTraverse` moves the scene
/// and draws another copy of a moving model: t = `start` + `step`, then t + `step`, …, each
/// sum rounded to float, for as long as `end` − `step` (in the register) is strictly greater
/// than t. Kernel of `blurTraverse` at 0x08275ce0.
///
/// The original does not guard against a step that is zero or negative: the sequence is
/// then endless, here as there, which is why this is an iterator and not a list.
#[derive(Clone, Copy, Debug)]
pub struct BlurTimes {
    t: f32,
    end: f32,
    step: f32,
}

impl BlurTimes {
    pub fn new(start: f32, end: f32, step: f32) -> BlurTimes {
        BlurTimes { t: start + step, end, step }
    }
}

impl Iterator for BlurTimes {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if !(x(self.end) - self.step).gt(x(self.t)) {
            return None;
        }
        let t = self.t;
        self.t = t + self.step;
        Some(t)
    }
}

/// The value `setExistance` stores in the material of every triangle of a model (the float at
/// +0x74c): its argument held between 0.2 and 0.8. Kernel of `setExistance` at 0x082762e0.
///
/// The argument is compared with the doubles 0.2 (0x086545a0) and 0.8 (0x086545b0) and
/// replaced by the floats 0.2 (0x086545a8) and 0.8 (0x086545b8); a NaN passes unchanged.
/// (The function also sets bit 0x100 of the material's flag word at +0x6e0 unless bit 0x200
/// is set.)
pub fn existance(e: f32) -> f32 {
    if x(e).lt(x(0.2f64)) {
        0.2
    } else if x(e).gt(x(0.8f64)) {
        0.8
    } else {
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P24: f32 = 16_777_216.0; // 2^24

    fn vert(world: (f32, f32, f32), ps: (f32, f32)) -> RVertex {
        RVertex { world: Vector::new(world.0, world.1, world.2), ps: Pspoint { x: ps.0, y: ps.1 }, ..RVertex::default() }
    }

    #[test]
    fn record_sizes() {
        assert_eq!(core::mem::size_of::<RVertex>(), 0x4c);
        assert_eq!(core::mem::size_of::<ClipVertex>(), 100);
        assert_eq!(core::mem::size_of::<ClipBox>(), 24);
        assert_eq!(core::mem::size_of::<Pspoint>(), 8);
    }

    #[test]
    fn flips() {
        let one = Vector::new(1.0, 1.0, 1.0);
        let mirror = Vector::new(1.0, -1.0, 1.0);
        assert!(!transform_flips_normals(one, true, false));
        assert!(transform_flips_normals(one, false, false));
        assert!(transform_flips_normals(mirror, true, false));
        assert!(!transform_flips_normals(mirror, false, false));
        assert!(transform_flips_normals(one, true, true));
        // a zero or NaN product counts as "not negative"
        assert!(!transform_flips_normals(Vector::new(0.0, -1.0, 1.0), true, false));
        assert!(!transform_flips_normals(Vector::new(f32::NAN, -1.0, 1.0), true, false));
        // the product is formed in the register: three tiny factors do not vanish
        let tiny = Vector::new(-1e-30, 1e-30, 1e-30);
        assert!(transform_flips_normals(tiny, true, false));
        assert_eq!(-1e-30f32 * 1e-30 * 1e-30, 0.0);
    }

    #[test]
    fn unit_normal_cases() {
        assert_eq!(unit_normal(Vector::new(3.0, 0.0, 4.0)), Vector::new(0.6, 0.0, 0.8));
        let zero = Vector::new(0.0, -0.0, 0.0);
        assert_eq!(unit_normal(zero).y.to_bits(), (-0.0f32).to_bits());
        // the squares are not rounded to float: 1e-30 squared is no float but is no zero either
        let n = unit_normal(Vector::new(1e-30, 0.0, 0.0));
        assert_eq!(n, Vector::new(1.0, 0.0, 0.0));
        assert_eq!(1e-30f32 * 1e-30, 0.0);
        // and they do not overflow: (3 × 2^100)² is beyond a float
        let big = (2.0f32).powi(100);
        assert_eq!(unit_normal(Vector::new(3.0 * big, 0.0, 4.0 * big)), Vector::new(0.6, 0.0, 0.8));
        assert!((3.0 * big * (3.0 * big)).is_infinite());
    }

    #[test]
    fn face_normal_orientation_and_the_rounded_operand() {
        // e = c − b = (1, 0, 0), g = a − b = (0, 1, 0): e × g = (0, 0, 1)
        let n = face_normal(Vector::new(0.0, 1.0, 0.0), Vector::new(0.0, 0.0, 0.0), Vector::new(1.0, 0.0, 0.0));
        assert_eq!(n, Vector::new(0.0, 0.0, 1.0));
        // degenerate: left unnormalised
        let p = Vector::new(1.0, 2.0, 3.0);
        assert_eq!(face_normal(p, p, p), Vector::new(0.0, 0.0, 0.0));
        // ex = c.x − b.x = 2^24 + 1 needs 25 bits. In ny = ez × gx − gz × ex it is used
        // rounded to float (2^24), in nz = gy × ex − gx × ey it is used exactly.
        // b = (−1, 0, 0), c = (2^24, 0, 0), a = (−1, 1, 1): g = (0, 1, 1), e = (2^24 + 1, 0, 0)
        // n = (0, −2^24, 2^24 + 1) before normalising; had ny used the exact ex, ny = −nz.
        let n = face_normal(Vector::new(-1.0, 1.0, 1.0), Vector::new(-1.0, 0.0, 0.0), Vector::new(P24, 0.0, 0.0));
        assert_eq!(n.x, 0.0);
        assert!(n.z > -n.y, "{:?}", n);
        assert_eq!(n.z.to_bits(), n.y.abs().to_bits() + 1);
    }

    #[test]
    fn pushed() {
        let w = pushed_along_normal(Vector::new(1.0, 2.0, 3.0), Vector::new(0.0, 0.0, 1.0), 0.5);
        assert_eq!(w, Vector::new(1.0, 2.0, 3.5));
        // one rounding: 3 × (2^24 + 2)/… chosen so that rounding the product first changes it:
        // amount × n = 3 × 5592407 = 16777221 (25 bits; as a float it would be 16777220),
        // + 1 = 16777222, a float. Rounding the product first gives 16777220 + 1 → 16777220.
        let w = pushed_along_normal(Vector::new(1.0, 0.0, 0.0), Vector::new(5_592_407.0, 0.0, 0.0), 3.0);
        assert_eq!(w.x, 16_777_222.0);
        assert_eq!(3.0f32 * 5_592_407.0 + 1.0, 16_777_220.0);
    }

    #[test]
    fn extents() {
        assert_eq!(extent3(2.0, 1.0, 3.0), (1.0, 3.0));
        assert_eq!(extent3(3.0, 3.0, 3.0), (3.0, 3.0));
        // ties go to the later value: the sign of zero shows which one was taken
        let (lo, hi) = extent3(0.0, -0.0, 0.0);
        assert_eq!((lo.to_bits(), hi.to_bits()), (0.0f32.to_bits(), 0.0f32.to_bits()));
        let (lo, hi) = extent3(0.0, 0.0, -0.0);
        assert_eq!((lo.to_bits(), hi.to_bits()), ((-0.0f32).to_bits(), (-0.0f32).to_bits()));
        // a NaN wins against what came before and loses against nothing after
        assert_eq!(extent3(1.0, f32::NAN, 2.0), (2.0, 2.0));
        assert!(extent3(1.0, 2.0, f32::NAN).0.is_nan());
        assert_eq!(sort_extent(2.0, 1.0, 3.0), (1.0, 3.0));
        assert_eq!(sort_extent(1.0, f32::NAN, 2.0), (2.0, 2.0));
    }

    fn view() -> ClipBox {
        ClipBox { xmin: 0.0, xmax: 100.0, ymin: 0.0, ymax: 80.0, yon: 50.0, hither: 1.0 }
    }

    #[test]
    fn fates() {
        let b = view();
        let tri = |z: [f32; 3], xs: [f32; 3]| {
            [vert((0.0, 0.0, z[0]), (xs[0], 10.0)), vert((0.0, 0.0, z[1]), (xs[1], 20.0)), vert((0.0, 0.0, z[2]), (xs[2], 30.0))]
        };
        let fate = |t: &[RVertex; 3]| clip_fate([&t[0], &t[1], &t[2]], &b);
        assert_eq!(fate(&tri([2.0, 3.0, 4.0], [1.0, 2.0, 3.0])), ClipFate::Keep);
        assert_eq!(fate(&tri([2.0, 3.0, 4.0], [101.0, 102.0, 103.0])), ClipFate::Reject);
        assert_eq!(fate(&tri([2.0, 3.0, 4.0], [100.0, 102.0, 103.0])), ClipFate::Keep); // touching the window
        assert_eq!(fate(&tri([0.5, 3.0, 4.0], [1.0, 2.0, 3.0])), ClipFate::Cut);
        assert_eq!(fate(&tri([2.0, 3.0, 60.0], [1.0, 2.0, 3.0])), ClipFate::Cut);
        assert_eq!(fate(&tri([0.2, 0.5, 0.9], [1.0, 2.0, 3.0])), ClipFate::Reject);
        assert_eq!(fate(&tri([51.0, 52.0, 60.0], [1.0, 2.0, 3.0])), ClipFate::Reject);
        // touching a depth plane is neither rejected nor cut
        assert_eq!(fate(&tri([0.2, 0.5, 1.0], [1.0, 2.0, 3.0])), ClipFate::Keep);
        assert_eq!(fate(&tri([50.0, 52.0, 60.0], [1.0, 2.0, 3.0])), ClipFate::Keep);
        assert_eq!(fate(&tri([1.0, 3.0, 4.0], [1.0, 2.0, 3.0])), ClipFate::Keep);
    }

    #[test]
    fn extend_distances() {
        let c = extend(&vert((7.0, 8.0, 3.0), (1.0, 2.0)), &view());
        assert_eq!(c.d, [47.0, 2.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.v.world, Vector::new(7.0, 8.0, 3.0));
    }

    /// A projection that is easy to check: x / z, y / z.
    fn divide(w: &Vector) -> Pspoint {
        Pspoint { x: w.x / w.z, y: w.y / w.z }
    }

    fn run_clip_in(b: &ClipBox, tri: &[RVertex; 3], n_map: i16, uv: &mut Vec<[f32; 2]>) -> Clipper {
        let mut project = divide;
        let mut env = ClipEnv { planes: b, uv, project: &mut project };
        let mut c = Clipper::default();
        c.clip_triangle([&tri[0], &tri[1], &tri[2]], n_map, &mut env);
        c
    }

    fn run_clip(tri: &[RVertex; 3], n_map: i16, uv: &mut Vec<[f32; 2]>) -> Clipper {
        run_clip_in(&view(), tri, n_map, uv)
    }

    #[test]
    fn clip_inside_and_outside() {
        let tri = [vert((0.0, 0.0, 2.0), (0.0, 0.0)), vert((4.0, 0.0, 2.0), (2.0, 0.0)), vert((0.0, 4.0, 2.0), (0.0, 2.0))];
        let c = run_clip(&tri, 0, &mut Vec::new());
        assert_eq!(c.output, tri.to_vec());
        assert_eq!((c.made_vertices, c.made_pairs), (3, 0));
        assert_eq!(c.flag[..2], [1, 1]);
        let behind = tri.map(|v| vert((v.world.x, v.world.y, 0.5), (0.0, 0.0)));
        assert!(run_clip(&behind, 0, &mut Vec::new()).output.is_empty());
        let beyond = tri.map(|v| vert((v.world.x, v.world.y, 60.0), (0.0, 0.0)));
        assert!(run_clip(&beyond, 0, &mut Vec::new()).output.is_empty());
        assert_eq!(clip_fan(0).count(), 0);
        assert_eq!(clip_fan(2).count(), 0);
        assert_eq!(clip_fan(3).collect::<Vec<_>>(), vec![[0, 1, 2]]);
        assert_eq!(clip_fan(5).collect::<Vec<_>>(), vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]]);
    }

    #[test]
    fn clip_against_hither() {
        // corner 0 is in front of the hither plane (z = 0 < 1), the other two behind it at z = 2.
        let mut v0 = vert((0.0, 0.0, 0.0), (9.0, 9.0));
        v0.normal = Vector::new(0.0, 0.0, 1.0);
        v0.uv = 0;
        v0.w00 = [7, 7];
        let mut v1 = vert((4.0, 0.0, 2.0), (2.0, 0.0));
        v1.normal = Vector::new(1.0, 0.0, 0.0);
        v1.uv = 1;
        let mut v2 = vert((0.0, 8.0, 2.0), (0.0, 4.0));
        v2.normal = Vector::new(0.0, 0.0, 1.0);
        v2.uv = 2;
        let mut uv = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let c = run_clip(&[v0, v1, v2], 1, &mut uv);
        // order: crossing of v1–v0 (from v1), v1, v2, crossing of v0–v2 (from v0, at the close)
        assert_eq!(c.output.len(), 4);
        let i10 = c.output[0];
        // t = d(v1) / (d(v1) − d(v0)) = 1 / (1 − (−1)) = 0.5
        assert_eq!(i10.world, Vector::new(2.0, 0.0, 1.0));
        assert_eq!(i10.ps, Pspoint { x: 2.0, y: 0.0 });
        // normal (1,0,0) + ((0,0,1) − (1,0,0)) × 0.5 = (0.5, 0, 0.5), normalised
        let h = (x(0.5f32) / (x(0.5f32) * 0.5f32 + x(0.5f32) * 0.5f32).sqrt()).to_f32();
        assert_eq!(i10.normal, Vector::new(h, 0.0, h));
        assert_eq!((i10.w00, i10.raw, i10.raw_normal, i10.w44, i10.p48), ([0, 0], Vector::default(), Vector::default(), 0, 0));
        assert_eq!(uv[i10.uv as usize], [0.5, 0.0]);
        assert_eq!(c.output[1], v1);
        assert_eq!(c.output[2], v2);
        let i02 = c.output[3];
        assert_eq!(i02.world, Vector::new(0.0, 4.0, 1.0));
        assert_eq!(i02.normal, Vector::new(0.0, 0.0, 1.0));
        assert_eq!(uv[i02.uv as usize], [0.0, 0.5]);
        assert_eq!((c.made_vertices, c.made_pairs, uv.len()), (4, 2, 5));
    }

    #[test]
    fn clip_against_both_planes() {
        // yon = 48, hither = 12; z runs from 0 to 64 along x, so both planes cut. The numbers
        // make every parameter t a quarter, so every crossing is exact.
        let b = ClipBox { yon: 48.0, hither: 12.0, ..view() };
        let p = vert((0.0, 0.0, 0.0), (0.0, 0.0));
        let q = vert((64.0, 0.0, 64.0), (0.0, 0.0));
        let r = vert((0.0, 16.0, 0.0), (0.0, 0.0));
        let world = |c: &Clipper| c.output.iter().map(|v| (v.world.x, v.world.y, v.world.z)).collect::<Vec<_>>();
        // Starting at p (inside yon, in front of hither):
        //   yon stage: p, I(q,p) = (48,0,48), I(r,q) = (48,4,48), r
        //   hither stage, fed in that order: I((48,0,48), p) = (12,0,12), (48,0,48), (48,4,48),
        //   I(r, (48,4,48)) = (12,13,12)
        let c = run_clip_in(&b, &[p, q, r], 0, &mut Vec::new());
        assert_eq!(world(&c), vec![(12.0, 0.0, 12.0), (48.0, 0.0, 48.0), (48.0, 4.0, 48.0), (12.0, 13.0, 12.0)]);
        assert_eq!(c.made_vertices, 4);
        // Starting at q (beyond yon): the yon stage's first vertex is outside, so its closing
        // point I(q, p) = (48,0,48) comes last, and still passes the hither stage, where it
        // brings its own crossing I((48,0,48), p) = (12,0,12) with it.
        let c = run_clip_in(&b, &[q, r, p], 0, &mut Vec::new());
        assert_eq!(world(&c), vec![(48.0, 4.0, 48.0), (12.0, 13.0, 12.0), (12.0, 0.0, 12.0), (48.0, 0.0, 48.0)]);
        assert_eq!(c.flag[..2], [1, 1]);
    }

    #[test]
    fn clip_vertex_on_the_plane_is_inside() {
        // d = 0 for the first corner: no crossing is generated on either of its edges
        let tri = [vert((0.0, 0.0, 1.0), (0.0, 0.0)), vert((4.0, 0.0, 2.0), (0.0, 0.0)), vert((0.0, 4.0, 0.5), (0.0, 0.0))];
        let c = run_clip(&tri, 0, &mut Vec::new());
        let zs: Vec<f32> = c.output.iter().map(|v| v.world.z).collect();
        assert_eq!(zs, vec![1.0, 2.0, 1.0]);
        assert_eq!(c.output[2].world.z.to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn clip_parameter_is_rounded_once_from_the_register() {
        let b = ClipBox { yon: 0.0, hither: 0.0, ..ClipBox::default() };
        let mut uv = Vec::new();
        let mut project = |w: &Vector| Pspoint { x: w.x, y: w.y };
        let mut env = ClipEnv { planes: &b, uv: &mut uv, project: &mut project };
        let mut c = Clipper::default();
        // a.d = 1, b.d = −3 × 2^−25: a.d − b.d = 1 + 3 × 2^−25 needs 26 bits. In the register
        // t = 1 / (1 + 3 × 2^−25) = 1 − 3 × 2^−25 + 9 × 2^−50 − …, just above the midpoint of
        // the floats 1 − 2^−23 and 1 − 2^−24: it rounds to 1 − 2^−24. Float arithmetic would
        // first round the denominator to 1 + 2^−23 and arrive at 1 − 2^−23.
        let db = -3.0 * (2.0f32).powi(-25);
        let mut a = ClipVertex::default();
        a.d[0] = 1.0;
        a.v.world = Vector::new(0.0, 0.0, 0.0);
        let mut q = ClipVertex::default();
        q.d[0] = db;
        q.v.world = Vector::new(1.0, 0.0, 0.0);
        let r = c.clip_intersect(&a, &q, 0, &mut env);
        assert_eq!(r.v.world.x.to_bits(), 0x3f7f_ffff);
        assert_eq!((1.0f32 / (1.0f32 - db)).to_bits(), 0x3f7f_fffe);
        // a + (b − a) × t is rounded once: a = −1, b = 2^24, t = 1 gives (2^24 + 1) − 1 = 2^24;
        // with the difference rounded to float it would be 2^24 − 1.
        let mut a = ClipVertex::default();
        a.d[0] = 1.0;
        a.v.world = Vector::new(-1.0, 0.0, 0.0);
        let mut q = ClipVertex::default();
        q.d[0] = 0.0;
        q.v.world = Vector::new(P24, 0.0, 0.0);
        let r = c.clip_intersect(&a, &q, 0, &mut env);
        assert_eq!(r.v.world.x, P24);
        assert_eq!((P24 - -1.0f32) * 1.0 + -1.0f32, P24 - 1.0);
        // the distances of the new vertex come from its rounded z
        assert_eq!(r.d, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn crossing_test_does_not_underflow() {
        // distances −1e−30 and 1e−30: their float product is −0, the register's is negative
        let b = ClipBox { yon: 1e-30, hither: -1.0, ..ClipBox::default() };
        let tri = [vert((0.0, 0.0, 0.0), (0.0, 0.0)), vert((1.0, 0.0, 2e-30), (0.0, 0.0)), vert((0.0, 1.0, 0.0), (0.0, 0.0))];
        let mut uv = Vec::new();
        let mut project = |w: &Vector| Pspoint { x: w.x, y: w.y };
        let mut env = ClipEnv { planes: &b, uv: &mut uv, project: &mut project };
        let mut c = Clipper::default();
        c.clip_triangle([&tri[0], &tri[1], &tri[2]], 0, &mut env);
        assert_eq!(c.output.len(), 4);
        assert_eq!((1e-30f32 * -1e-30f32).to_bits(), (-0.0f32).to_bits());
    }

    fn list(n: usize) -> Vec<Tn> {
        (0..n)
            .map(|i| Tn { v: [Some(3 * i), Some(3 * i + 1), Some(3 * i + 2)], next: if i + 1 < n { Some(i + 1) } else { None }, ..Tn::default() })
            .collect()
    }

    #[test]
    fn barr_plane() {
        let verts = [vert((1.0, 1.0, 5.0), (0.0, 0.0)), vert((3.0, 1.0, 5.0), (0.0, 0.0)), vert((1.0, 4.0, 5.0), (0.0, 0.0))];
        let mut tns = list(1);
        tns[0].flags = TN_ONE_SIDED;
        barr(&mut tns, Some(0), &verts);
        let t = &tns[0];
        // p = (2, 0, 0), q = (0, 3, 0): n = (0, 0, 6), d = 30
        assert_eq!(t.n, Vector::new(0.0, 0.0, 6.0));
        assert_eq!(t.d, 30.0);
        assert_eq!(t.inv_d, 1.0 / 30.0);
        assert_eq!((t.axis, t.inv_n), (3, 1.0 / 6.0));
        assert_eq!(t.edge, [Vector::new(-2.0, 3.0, 0.0), Vector::new(0.0, -3.0, 0.0), Vector::new(2.0, 0.0, 0.0)]);
        assert_eq!(t.flags, TN_ONE_SIDED | TN_DROPPED); // d ≥ 0: faces away
        // the other winding faces the eye
        let mut tns = list(1);
        tns[0].v = [Some(0), Some(2), Some(1)];
        tns[0].flags = TN_ONE_SIDED;
        barr(&mut tns, Some(0), &verts);
        assert_eq!((tns[0].n.z, tns[0].d, tns[0].flags), (-6.0, -30.0, TN_ONE_SIDED));
    }

    #[test]
    fn barr_flags_and_axis_ties() {
        // a zero normal: void, nothing else written
        let verts = [vert((1.0, 1.0, 5.0), (0.0, 0.0)); 3];
        let mut tns = list(1);
        tns[0].inv_d = 9.0;
        barr(&mut tns, Some(0), &verts);
        assert_eq!((tns[0].flags, tns[0].inv_d, tns[0].axis), (TN_VOID | TN_DROPPED, 9.0, 0));
        // a void triangle is skipped altogether, null corners and all
        let mut tns = list(1);
        tns[0].v = [None; 3];
        tns[0].flags = TN_VOID;
        barr(&mut tns, Some(0), &verts);
        assert_eq!(tns[0].n, Vector::default());
        // a plane through the eye: dropped, inv_d untouched, the rest written
        let verts = [vert((0.0, 0.0, 0.0), (0.0, 0.0)), vert((1.0, 0.0, 0.0), (0.0, 0.0)), vert((0.0, 1.0, 0.0), (0.0, 0.0))];
        let mut tns = list(1);
        tns[0].inv_d = 9.0;
        barr(&mut tns, Some(0), &verts);
        assert_eq!((tns[0].flags, tns[0].inv_d, tns[0].axis, tns[0].d), (TN_DROPPED, 9.0, 3, 0.0));
        // |n.x| = |n.y| = |n.z|: the first axis wins
        let verts = [vert((1.0, 0.0, 0.0), (0.0, 0.0)), vert((0.0, 1.0, 0.0), (0.0, 0.0)), vert((0.0, 0.0, 1.0), (0.0, 0.0))];
        let mut tns = list(1);
        barr(&mut tns, Some(0), &verts);
        assert_eq!((tns[0].n, tns[0].axis, tns[0].d), (Vector::new(1.0, 1.0, 1.0), 1, 1.0));
    }

    #[test]
    fn barr_mixes_rounded_and_unrounded_differences() {
        // a = (1, 1, −1), b = (1, 2, 2^24), c = (2, 2, −1): p = (0, 1, 2^24 + 1), q = (1, 1, 0).
        // n.x = py × qz − pz × qy = −(2^24 + 1) exactly in the register (stored: −2^24);
        // n.y = qx × pz − px × qz with pz rounded to float: 2^24;
        // n.z = px × qy − qx × py = −1;
        // d = n.z × a.z + (n.y × a.y + a.x × n.x) = 1 + (2^24 − (2^24 + 1)) = 0.
        // With n.x rounded before use, or pz unrounded in n.y, d would be 1.
        let verts = [vert((1.0, 1.0, -1.0), (0.0, 0.0)), vert((1.0, 2.0, P24), (0.0, 0.0)), vert((2.0, 2.0, -1.0), (0.0, 0.0))];
        let mut tns = list(1);
        barr(&mut tns, Some(0), &verts);
        assert_eq!(tns[0].n, Vector::new(-P24, P24, -1.0));
        assert_eq!(tns[0].d, 0.0);
        assert_eq!(tns[0].flags, TN_DROPPED);
        assert_eq!(tns[0].axis, 1);
    }

    #[test]
    fn corner_orders() {
        // every arrangement of three different heights
        for (y, top, mid, bot) in [
            ([3.0, 2.0, 1.0], 0, 1, 2),
            ([3.0, 1.0, 2.0], 0, 2, 1),
            ([2.0, 3.0, 1.0], 1, 0, 2),
            ([1.0, 3.0, 2.0], 1, 2, 0),
            ([2.0, 1.0, 3.0], 2, 0, 1),
            ([1.0, 2.0, 3.0], 2, 1, 0),
        ] {
            assert_eq!(point_sort_order(y), (top, mid, bot), "{:?}", y);
            assert_eq!(area_sort_order(y), (top, bot), "{:?}", y);
        }
        // ties
        assert_eq!(point_sort_order([1.0, 1.0, 1.0]), (0, 1, 2));
        assert_eq!(area_sort_order([1.0, 1.0, 1.0]), (0, 2));
        assert_eq!(point_sort_order([1.0, 2.0, 2.0]), (1, 2, 0));
        assert_eq!(area_sort_order([1.0, 2.0, 2.0]), (1, 0));
        assert_eq!(point_sort_order([2.0, 2.0, 1.0]), (0, 1, 2));
        assert_eq!(point_sort_order([2.0, 1.0, 2.0]), (0, 2, 1));
        assert_eq!(point_sort_order([1.0, 1.0, 2.0]), (2, 0, 1));
        assert_eq!(area_sort_order([1.0, 1.0, 2.0]), (2, 1));
        // a NaN: still three different corners
        let (t, m, b) = point_sort_order([f32::NAN, 1.0, 2.0]);
        assert!(t != m && m != b && t != b);
        assert_eq!((t, m, b), (2, 1, 0));
        assert_eq!(area_sort_order([f32::NAN, 1.0, 2.0]), (2, 0));
    }

    fn bins(lines: usize) -> YBins {
        YBins { lines: vec![Vec::new(); lines], bounds: [1e9, -1e9, 1e9, -1e9], ..YBins::default() }
    }

    fn tri_ps(p: [(f32, f32); 3]) -> [RVertex; 3] {
        p.map(|q| vert((0.0, 0.0, 0.0), q))
    }

    #[test]
    fn area_sort_files_a_box() {
        let verts = tri_ps([(4.0, 12.5), (10.0, 20.25), (8.0, 4.75)]);
        let tns = list(1);
        let mut b = bins(100);
        area_sort(&tns, Some(0), &verts, &view(), &mut b);
        assert_eq!(b.lines[21], vec![Span { x0: 4.0, dx0: 0.0, x1: 11.0, dx1: 0.0, y_end: 4, tn: 0 }]);
        assert_eq!((b.accepted, b.made), (1, 1));
        assert_eq!(b.bounds, [4.0, 10.0, 4.75, 20.25]);
        // the window's top line caps the scan line
        let mut b = bins(100);
        area_sort(&tns, Some(0), &verts, &ClipBox { ymax: 15.9, ..view() }, &mut b);
        assert_eq!(b.lines[15].len(), 1);
        // outside the window: not even counted
        let mut b = bins(100);
        area_sort(&tns, Some(0), &verts, &ClipBox { xmin: 10.5, ..view() }, &mut b);
        assert_eq!((b.accepted, b.made), (0, 0));
        // flatter than 0.0001: counted, not filed
        let flat = tri_ps([(4.0, 12.0), (10.0, 12.00005), (8.0, 12.0)]);
        let mut b = bins(100);
        area_sort(&tns, Some(0), &flat, &view(), &mut b);
        assert_eq!((b.accepted, b.made), (1, 0));
        // dropped and void triangles are skipped
        let mut b = bins(100);
        let mut marked = list(2);
        marked[0].flags = TN_DROPPED;
        marked[1].flags = TN_VOID;
        marked[1].v = [None; 3];
        area_sort(&marked, Some(0), &verts, &view(), &mut b);
        assert_eq!(b.accepted, 0);
    }

    #[test]
    fn scan_line_is_truncated_from_the_register() {
        // top y = 0.99999994: y + 1 is 1.99999994 in the register (line 1); as a float sum it is 2.
        let y = f32::from_bits(0x3f7f_ffff);
        assert_eq!(top_line(1000, y), 1);
        assert_eq!((y + 1.0) as i32, 2);
        assert_eq!(top_line(1000, -0.5), 0);
        assert_eq!(top_line(1000, -1.5), 0);
        assert_eq!(top_line(1000, -2.5), -1);
        assert_eq!(top_line(7, 20.0), 7);
        assert_eq!(top_line(1000, f32::NAN), i32::MIN);
        // a line outside the table is kept apart instead of written through
        let verts = tri_ps([(4.0, -3.0), (10.0, -2.5), (8.0, -9.0)]);
        let mut b = bins(10);
        area_sort(&list(1), Some(0), &verts, &ClipBox { ymin: -100.0, ..view() }, &mut b);
        assert_eq!(b.stray.len(), 1);
        assert_eq!(b.stray[0].0, -1);
    }

    #[test]
    fn lists_grow_at_the_head() {
        let mut verts = tri_ps([(4.0, 12.0), (10.0, 20.0), (8.0, 4.0)]).to_vec();
        verts.extend(tri_ps([(40.0, 12.0), (50.0, 20.5), (45.0, 4.0)]));
        let tns = list(2);
        let mut b = bins(100);
        area_sort(&tns, Some(0), &verts, &view(), &mut b);
        // both under line 21; the second triangle is the head of the original's list
        assert_eq!(b.lines[21].iter().map(|s| s.tn).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn point_sort_general_triangle() {
        // T = (10, 20), M = (4, 12), B = (8, 4): dy = 16, xc = (10 − 8) × (8 / 16) + 8 = 9.
        let verts = tri_ps([(4.0, 12.0), (10.0, 20.0), (8.0, 4.0)]);
        let tns = list(1);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &view(), &mut b);
        // upper: filed under 21, h = 20 − 21 = −1; left edge to M: slope (4 − 10) / 8,
        // right edge along the long side: slope (9 − 10) / 8.
        assert_eq!(b.lines[21], vec![Span { x0: 10.75, dx0: -0.75, x1: 11.125, dx1: -0.125, y_end: 13, tn: 0 }]);
        // lower: filed under 12, h = 0; left from M: (8 − 4) / 8, right from xc: (8 − 9) / 8
        assert_eq!(b.lines[12], vec![Span { x0: 4.0, dx0: 0.5, x1: 10.0, dx1: -0.125, y_end: 4, tn: 0 }]);
        assert_eq!((b.accepted, b.made), (1, 2));
        // the mirror image: M on the right of the long edge
        let verts = tri_ps([(16.0, 12.0), (10.0, 20.0), (12.0, 4.0)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &view(), &mut b);
        // xc = (10 − 12) × 0.5 + 12 = 11
        assert_eq!(b.lines[21], vec![Span { x0: 9.875, dx0: 0.125, x1: 10.25, dx1: 0.75, y_end: 13, tn: 0 }]);
        assert_eq!(b.lines[12], vec![Span { x0: 11.0, dx0: 0.125, x1: 17.0, dx1: -0.5, y_end: 4, tn: 0 }]);
    }

    #[test]
    fn point_sort_low_halves_and_small_triangles() {
        let tns = list(1);
        // dy < 2: the bounding box, like area_sort
        let verts = tri_ps([(4.0, 12.0), (10.0, 13.5), (8.0, 12.5)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &view(), &mut b);
        assert_eq!(b.lines[14], vec![Span { x0: 4.0, dx0: 0.0, x1: 11.0, dx1: 0.0, y_end: 12, tn: 0 }]);
        // upper half lower than 2 lines: the box of T.x, M.x and xc; lower half sloped
        // T = (10, 20), M = (4, 19), B = (8, 4): xc = 2 × (15 / 16) + 8 = 9.875
        let verts = tri_ps([(10.0, 20.0), (4.0, 19.0), (8.0, 4.0)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &view(), &mut b);
        assert_eq!(b.lines[21], vec![Span { x0: 4.0, dx0: 0.0, x1: 11.0, dx1: 0.0, y_end: 20, tn: 0 }]);
        // lower: h = 19 − 19 = 0; slopes (8 − 4) / 15 and (8 − 9.875) / 15
        let l = b.lines[19][0];
        assert_eq!((l.x0, l.x1, l.y_end), (4.0, 10.875, 4));
        assert_eq!(l.dx0, (x(4.0f32) / x(15.0f32)).to_f32());
        assert_eq!(l.dx1, (x(-1.875f32) / x(15.0f32)).to_f32());
        // a middle corner below line 0: no lower record
        let verts = tri_ps([(10.0, 20.0), (4.0, -1.5), (8.0, -4.0)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &ClipBox { ymin: -10.0, ..view() }, &mut b);
        assert_eq!(b.made, 1);
        // … but −0.5 truncates to 0: a lower record under line 0
        let verts = tri_ps([(10.0, 20.0), (4.0, -0.5), (8.0, -4.0)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &ClipBox { ymin: -10.0, ..view() }, &mut b);
        assert_eq!((b.made, b.lines[0].len()), (2, 1));
    }

    #[test]
    fn point_sort_flat_sides_and_precision() {
        let tns = list(1);
        // flat top: T = (10, 20) and M = (4, 20), B = (8, 4). T.x > M.x: left from M, right from T
        let verts = tri_ps([(10.0, 20.0), (4.0, 20.0), (8.0, 4.0)]);
        let mut b = bins(100);
        point_sort(&tns, Some(0), &verts, &view(), &mut b);
        // h = −1: x0 = (8 − 4) / 16 × −1 + 4, x1 = 1 + ((8 − 10) / 16 × −1 + 10)
        assert_eq!(b.lines[21], vec![Span { x0: 3.75, dx0: 0.25, x1: 11.125, dx1: -0.125, y_end: 4, tn: 0 }]);
        // flat bottom, with numbers that show where the rounding happens:
        // T = (−1, 3), B = (2^24, 1), M = (2^25, 1); dy = 2, filed under 4, h = −1.
        // left slope (2^24 + 1) / 2 = 2^23 + 0.5 in the register (stored: 2^23);
        // x0 = −(2^23 + 0.5) − 1 = −(2^23 + 1.5) → −(2^23 + 2) as a float (tie to even).
        // Float arithmetic would give slope 2^23 and x0 = −(2^23 + 1).
        // right slope (2^25 + 1) / 2 = 2^24 + 0.5 (stored: 2^24);
        // x1 = 1 + (−(2^24 + 0.5) − 1) = −(2^24 + 0.5) → −2^24.
        let verts = tri_ps([(-1.0, 3.0), (P24, 1.0), (2.0 * P24, 1.0)]);
        let mut b = bins(100);
        let wide = ClipBox { xmin: -1e9, xmax: 1e9, ..view() };
        point_sort(&tns, Some(0), &verts, &wide, &mut b);
        assert_eq!(
            b.lines[4],
            vec![Span { x0: -8_388_610.0, dx0: 8_388_608.0, x1: -P24, dx1: P24, y_end: 1, tn: 0 }]
        );
        assert_eq!((P24 - -1.0f32) / 2.0 * -1.0 + -1.0, -8_388_609.0);
    }

    #[test]
    fn trace_list_boxes() {
        let verts = [
            vert((1.0, 5.0, 9.0), (0.0, 0.0)),
            vert((2.0, 4.0, 8.0), (0.0, 0.0)),
            vert((3.0, 6.0, 7.0), (0.0, 0.0)),
            vert((-1.0, 0.0, 20.0), (0.0, 0.0)),
            vert((0.0, 0.5, 21.0), (0.0, 0.0)),
            vert((0.5, 0.25, 22.0), (0.0, 0.0)),
        ];
        let tns = list(2);
        let mut out = TraceList { bounds: [1e9, -1e9, 1e9, -1e9, 1e9, -1e9], ..TraceList::default() };
        trace_list(&tns, Some(0), &verts, &mut out);
        assert_eq!(out.items[0], TraceItem { bounds: [1.0, 3.0, 4.0, 6.0, 7.0, 9.0], tn: 0 });
        assert_eq!(out.items[1], TraceItem { bounds: [-1.0, 0.5, 0.0, 0.5, 20.0, 22.0], tn: 1 });
        assert_eq!(out.bounds, [-1.0, 3.0, 0.0, 6.0, 7.0, 22.0]);
        assert_eq!(out.made, 2);
    }

    #[test]
    fn movement() {
        let a = [vert((0.0, 0.0, 0.0), (1.0, 1.0)), vert((0.0, 0.0, 0.0), (5.0, 5.0))];
        let b = [vert((0.0, 0.0, 0.0), (1.0, 1.0)), vert((0.0, 0.0, 0.0), (2.0, 1.0))];
        assert_eq!(max_ps_movement(Some(&a), Some(&b)).to_f32(), 5.0);
        assert_eq!(max_ps_movement(Some(&a), Some(&a)).to_f32(), 0.0);
        assert_eq!(max_ps_movement(None, Some(&a)).to_f32(), -1.0);
        assert_eq!(max_ps_movement(Some(&a), None).to_f32(), -1.0);
        assert_eq!(max_ps_movement(Some(&a), Some(&b[..1])).to_f32(), -1.0);
        assert_eq!(max_ps_movement(Some(&a[..0]), Some(&b[..0])).to_f32(), -1.0);
        // a NaN distance is ignored
        let n = [vert((0.0, 0.0, 0.0), (f32::NAN, 1.0)), vert((0.0, 0.0, 0.0), (5.0, 5.0))];
        assert_eq!(max_ps_movement(Some(&n), Some(&b)).to_f32(), 5.0);
        // the result is not rounded: √2 from the register is above the float √2, so it passes
        // a threshold of exactly that float; the rounded value would not.
        let u = [vert((0.0, 0.0, 0.0), (1.0, 1.0))];
        let o = [vert((0.0, 0.0, 0.0), (0.0, 0.0))];
        let r = max_ps_movement(Some(&u), Some(&o));
        let root2 = 2.0f32.sqrt();
        assert_eq!(r.to_f32(), root2);
        assert_eq!(moved_enough(r, root2), Some(root2));
        assert_eq!(moved_enough(x(root2), root2), None);
        // the squares do not overflow
        let big = (2.0f32).powi(100);
        let far = [vert((0.0, 0.0, 0.0), (3.0 * big, 4.0 * big))];
        assert_eq!(max_ps_movement(Some(&far), Some(&o)).to_f32(), 5.0 * big);
    }

    #[test]
    fn blur_arithmetic() {
        assert_eq!(blur_end_time(10.0, 0.5, 25.0), 10.02);
        // 3 / 2 + 2^24 = 2^24 + 1.5, above the tie at 2^24 + 1
        assert_eq!(blur_end_time(P24, 3.0, 2.0), P24 + 2.0);
        let (step, e) = blur_step(1.0, 2.0, 8.0, 2.0);
        assert_eq!((step, e), (0.25, 0.25));
        // (1 / 3) × 3 is 1 + 2^−65 before the register rounds it: exactly 1
        assert_eq!(blur_step(0.0, 1.0, 3.0, 3.0), (1.0, 1.0));
        let times: Vec<f32> = BlurTimes::new(1.0, 2.0, 0.25).collect();
        assert_eq!(times, vec![1.25, 1.5]);
        // end − step must be strictly greater
        assert_eq!(BlurTimes::new(0.0, 1.0, 0.5).count(), 0);
        assert_eq!(BlurTimes::new(0.0, 1.0, 0.75).count(), 0);
        assert_eq!(BlurTimes::new(0.0, 1.0, 0.4).collect::<Vec<_>>(), vec![0.4]);
        // a zero step never ends
        assert_eq!(BlurTimes::new(0.0, 1.0, 0.0).take(1000).count(), 1000);
        assert_eq!(BlurTimes::new(0.0, 1.0, f32::NAN).count(), 0);
    }

    #[test]
    fn existance_limits() {
        assert_eq!(existance(0.5), 0.5);
        assert_eq!(existance(0.1), 0.2);
        assert_eq!(existance(0.9), 0.8);
        assert_eq!(existance(-3.0), 0.2);
        assert!(existance(f32::NAN).is_nan());
        // the limits are doubles: the float just below 0.2f32 is still below 0.2
        let below = f32::from_bits(0.2f32.to_bits() - 1);
        assert_eq!(existance(below).to_bits(), 0.2f32.to_bits());
        // 0.2f32 itself is above the double 0.2 and stays; 0.8f32 is above the double 0.8
        assert_eq!(existance(0.2).to_bits(), 0.2f32.to_bits());
        assert_eq!(existance(0.8).to_bits(), 0.8f32.to_bits());
        let under = f32::from_bits(0.8f32.to_bits() - 1);
        assert_eq!(existance(under).to_bits(), under.to_bits());
    }
}
