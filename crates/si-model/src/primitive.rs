// SPDX-License-Identifier: MIT
//! Polygon mesh primitives: part of the original's `primitive.c` (finding 0015).
//!
//! Each function builds the vertices and polygons exactly as the original does (same
//! order, same arithmetic) and ends, as it does, with `SI_meshUpdate`
//! ([`crate::mesh_b::update`]). Ported: cube, rectangular grid, sphere, tetrahedron,
//! octahedron, torus, cone, cylinder, icosahedron, dodecahedron. Not ported (the finding lists
//! them): the spline, patch and NURBS primitives.
//!
//! Exactness classes as in `si-math` (finding 0008). The sphere and the tetrahedron go
//! through the C library's sine and cosine (class libm).

use si_math::algebra::{matrix_x_rotation, matrix_xyz_rotation, matrix_y_rotation};
use si_math::types::{Matrix, Vector};
use si_x87::{trig, x, F80};

use crate::mesh::{Mesh, Polygon, Vertex};
use crate::mesh_b::{polygon_initialize, update, MeshEnv};

/// `SI_nbPolMax` (0x086d3920) as the binary initialises it: the grid and the sphere refuse
/// to make more polygons than this.
pub const NB_POL_MAX: i32 = 60000;

/// π and 2π as the doubles of this source file (0x08660400, 0x08660408).
const PI_D: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
const TWO_PI_D: f64 = f64::from_bits(0x4019_21fb_5444_2d18);

fn vertex(pos: [f32; 3]) -> Vertex {
    Vertex { pos, star: Vec::new(), flag: 0 }
}

fn polygon(corners: &[i32]) -> Polygon {
    let mut p = Polygon::default();
    polygon_initialize(&mut p, corners);
    p
}

/// A point through a matrix as these routines write it out:
/// `((y·m[1][j] + x·m[0][j]) + z·m[2][j]) + m[3][j]`, in the register, rounded once.
fn point_through(p: [f32; 3], m: &Matrix) -> [f32; 3] {
    let c = |j: usize| (((x(p[1]) * m[1][j] + x(p[0]) * m[0][j]) + x(p[2]) * m[2][j]) + m[3][j]).to_f32();
    [c(0), c(1), c(2)]
}

/// A box [soft 0x0848a7c0 SI_meshCube]: six floats, the far corner `(x1, y1, z1)` first and
/// the near corner `(x0, y0, z0)` second, copied into eight vertices without any
/// arithmetic. Class: copy.
///
/// Vertex `k` takes x from bit 2 of `k` (0: `x0`, 1: `x1`), y from bit 1, z from bit 0. Six
/// quadrilaterals: (0,1,3,2), (1,5,7,3), (5,4,6,7), (4,0,2,6), (4,5,1,0), (2,3,7,6), which
/// face outwards when the first corner is the larger one. After the update the mesh flag
/// loses bit 0x8 again (0x804: the next normal pass turns the automatic discontinuity back
/// on, as for any new mesh).
///
/// The Get > Primitive > Cube dialog (`SI_dialogGetCube`) calls this with its length field
/// times 0.5 three times and times −0.5 three times, for a node named `cube`.
pub fn mesh_cube(x1: f32, y1: f32, z1: f32, x0: f32, y0: f32, z0: f32, env: &mut MeshEnv) -> Mesh {
    let mut m = Mesh::default();
    for k in 0..8 {
        m.vertices.push(vertex([if k & 4 != 0 { x1 } else { x0 }, if k & 2 != 0 { y1 } else { y0 }, if k & 1 != 0 { z1 } else { z0 }]));
    }
    for q in [[0, 1, 3, 2], [1, 5, 7, 3], [5, 4, 6, 7], [4, 0, 2, 6], [4, 5, 1, 0], [2, 3, 7, 6]] {
        m.polygons.push(polygon(&q));
    }
    update(&mut m, env);
    m.flags &= !8;
    m
}

/// A flat grid in the XZ plane, centred on the origin [soft 0x08488eec SI_meshGrid]:
/// `nu` by `nv` quadrilaterals of size `du` by `dv`. `None` when `nu × nv` exceeds
/// `nb_pol_max` (`SI_nbPolMax`). Class: model.
///
/// Vertices in rows of constant x, `(nu + 1)` rows of `(nv + 1)`:
///
/// - the first x is `(−nu × du) × 0.5` rounded to float; each further row adds `du` **in the
///   register**, so the x values are the rounded partial sums of an unrounded running sum;
/// - within a row z starts from `(−nv × dv) × 0.5` (kept unrounded between rows, rounded to
///   float at the start of each) and adds `dv` in the register the same way.
///
/// Polygon `(i, j)` is `(a, a + 1, b + 1, b)` with `a = i (nv + 1) + j` and
/// `b = (i + 1)(nv + 1) + j`: its normal points to +y for positive steps.
pub fn mesh_grid(nu: i16, nv: i16, du: f32, dv: f32, nb_pol_max: i32, env: &mut MeshEnv) -> Option<Mesh> {
    let (nu, nv) = (nu as i32, nv as i32);
    if nu * nv > nb_pol_max {
        return None;
    }
    let mut m = Mesh::default();
    let x0 = ((F80::from_i32(-nu) * du) * 0.5f64).to_f32();
    if nu >= 0 {
        let z_start = x(0.5f64) * (F80::from_i32(-nv) * dv);
        let mut xr = x(x0);
        for _ in 0..=nu {
            let z0 = z_start.to_f32();
            let next = xr + du;
            if nv >= 0 {
                let mut zr = x(z0);
                for _ in 0..=nv {
                    m.vertices.push(vertex([xr.to_f32(), 0.0, zr.to_f32()]));
                    zr = zr + dv;
                }
            }
            xr = next;
        }
    }
    for i in 0..nu.max(0) {
        for j in 0..nv.max(0) {
            let (a, b) = (i * (nv + 1) + j, (i + 1) * (nv + 1) + j);
            m.polygons.push(polygon(&[a, a + 1, b + 1, b]));
        }
    }
    update(&mut m, env);
    Some(m)
}

/// A sphere of latitude bands and meridians [soft 0x08486df8 SI_meshSphere]: `bands`
/// divisions from pole to pole (the original's second argument), `meridians` around (its
/// third). `None` when `bands × meridians` exceeds `nb_pol_max`. Class: libm.
///
/// `(bands − 1) × meridians + 2` vertices: 0 is the south pole `(0, −r, 0)`, 1 the north
/// pole `(0, r, 0)`; then meridian by meridian, south to north.
///
/// - The first meridian lies in the XY plane: with `step` = π / bands rounded to float and
///   the angle a running sum of `step` kept in the register (never rounded), vertex `k`
///   is `(sin(a) × r, −cos(a) × r, 0)`, the angle rounded to double for the two library
///   calls and each product rounded once.
/// - Every further meridian is the one before it turned about Y by 2π / meridians (rounded
///   to float, `SI_matrixYRotation`), point by point: the rotation is applied
///   `meridians − 1` times in a chain, not computed from the angle each time.
///
/// Polygons, meridian by meridian (`j`, with `jn` the next one, the first after the last):
/// a triangle at the south pole `(jn·w + 2, j·w + 2, 0)`, then `bands − 2` quadrilaterals
/// `(j·w + 2 + i, jn·w + 2 + i, jn·w + 3 + i, j·w + 3 + i)`, then a triangle at the north
/// pole `(j·w + bands, jn·w + bands, 1)`, where `w = bands − 1`. After the update the mesh
/// flag loses bit 0x8, as for the cube.
///
/// With fewer than 2 bands or no meridian the original writes outside the arrays it has
/// allocated; here that is `None`.
pub fn mesh_sphere(radius: f32, bands: i16, meridians: i16, nb_pol_max: i32, env: &mut MeshEnv) -> Option<Mesh> {
    let (n2, n3) = (bands as i32, meridians as i32);
    if n2 * n3 > nb_pol_max || n2 < 2 || n3 < 1 {
        return None;
    }
    let w = n2 - 1;
    let mut m = Mesh::default();
    m.vertices = vec![vertex([0.0; 3]); (w * n3 + 2) as usize];
    m.vertices[0].pos = [0.0, -radius, 0.0];
    m.vertices[1].pos = [0.0, radius, 0.0];
    let step = (x(PI_D) / F80::from_i32(n2)).to_f32();
    let mut angle = x(step);
    for k in 2..=n2 {
        let a = angle.to_f64();
        let sx = (trig::sin(a) * radius).to_f32();
        let sy = (-trig::cos(a) * x(radius)).to_f32();
        m.vertices[k as usize].pos = [sx, sy, 0.0];
        angle = angle + step;
    }
    let turn = matrix_y_rotation((x(TWO_PI_D) / F80::from_i32(n3)).to_f32());
    for j in 0..n3 - 1 {
        for i in 0..w {
            let from = m.vertices[(j * w + i + 2) as usize].pos;
            m.vertices[((j + 1) * w + i + 2) as usize].pos = point_through(from, &turn);
        }
    }
    for j in 0..n3 {
        let jn = if j == n3 - 1 { 0 } else { j + 1 };
        m.polygons.push(polygon(&[jn * w + 2, j * w + 2, 0]));
        for i in 0..n2 - 2 {
            m.polygons.push(polygon(&[j * w + 2 + i, jn * w + 2 + i, jn * w + 3 + i, j * w + 3 + i]));
        }
        m.polygons.push(polygon(&[w * j + n2, jn * w + n2, 1]));
    }
    update(&mut m, env);
    m.flags &= !8;
    Some(m)
}

/// A regular tetrahedron [soft 0x08489a58 SI_meshTetrahedron]. Class: libm.
///
/// With `a` = |size| × 0.57735 (a double constant of five digits, 0x08660540, not 1/√3),
/// rounded to float, the four corners `(a, a, a)`, `(a, −a, −a)`, `(−a, a, −a)`,
/// `(−a, −a, a)` of a cube are turned by the rotation of the angles (π/4, 0, −0.61548)
/// (floats 0x08660548 and 0x0866054c), which stands the solid on a face. Polygons
/// (3,2,1), (2,3,0), (1,0,3), (0,1,2).
pub fn mesh_tetrahedron(size: f32, env: &mut MeshEnv) -> Mesh {
    let a = (x(size).abs() * 0.57735f64).to_f32();
    let mut m = Mesh::default();
    let turn = matrix_xyz_rotation(&Vector::new(f32::from_bits(0x3f49_0fdb), 0.0, f32::from_bits(0xbf1d_9019)));
    for p in [[a, a, a], [a, -a, -a], [-a, a, -a], [-a, -a, a]] {
        m.vertices.push(vertex(point_through(p, &turn)));
    }
    for t in [[3, 2, 1], [2, 3, 0], [1, 0, 3], [0, 1, 2]] {
        m.polygons.push(polygon(&t));
    }
    update(&mut m, env);
    m
}

/// A regular octahedron [soft 0x08489c10 SI_meshOctahedron]: with r = |size|, the vertices
/// `(r,0,0)`, `(−r,0,0)`, `(0,r,0)`, `(0,−r,0)`, `(0,0,r)`, `(0,0,−r)` and the triangles
/// (0,2,4), (2,0,5), (3,0,4), (0,3,5), (2,1,4), (1,2,5), (1,3,4), (3,1,5). Class: copy.
pub fn mesh_octahedron(size: f32, env: &mut MeshEnv) -> Mesh {
    let r = size.abs();
    let mut m = Mesh::default();
    for p in [[r, 0.0, 0.0], [-r, 0.0, 0.0], [0.0, r, 0.0], [0.0, -r, 0.0], [0.0, 0.0, r], [0.0, 0.0, -r]] {
        m.vertices.push(vertex(p));
    }
    for t in [[0, 2, 4], [2, 0, 5], [3, 0, 4], [0, 3, 5], [2, 1, 4], [1, 2, 5], [1, 3, 4], [3, 1, 5]] {
        m.polygons.push(polygon(&t));
    }
    update(&mut m, env);
    m
}

/// The chain of Y rotations the revolved solids share: the vertices `[2 + j·w + i]` of meridian
/// `j + 1` are those of meridian `j` through `turn`, point by point, `n − 1` times.
fn turn_meridians(m: &mut Mesh, n: i32, w: i32, turn: &Matrix) {
    for j in 0..n - 1 {
        for i in 0..w {
            let from = m.vertices[(j * w + i + 2) as usize].pos;
            m.vertices[((j + 1) * w + i + 2) as usize].pos = point_through(from, turn);
        }
    }
}

/// The polygons of a cone or a cylinder, meridian by meridian (`w` vertices per meridian
/// besides the two poles 0 and 1): a triangle at pole 0, `w − 1` quadrilaterals, a triangle at
/// pole 1.
fn pole_to_pole_polygons(m: &mut Mesh, n: i32, w: i32) {
    for j in 0..n {
        let jn = if j == n - 1 { 0 } else { j + 1 };
        let (a, b) = (w * j, w * jn);
        m.polygons.push(polygon(&[b + 2, a + 2, 0]));
        for k in 0..w - 1 {
            m.polygons.push(polygon(&[a + 2 + k, b + 2 + k, b + 3 + k, a + 3 + k]));
        }
        m.polygons.push(polygon(&[w + 1 + a, w + 1 + b, 1]));
    }
}

/// A torus [soft 0x08487228 SI_meshTorus]: `radius` of the tube, `offset` of its centre from
/// the axis, `around` points on the tube's circle (the original's third argument) and `turns`
/// copies of it about Y (its fourth). `None` when `around × turns` exceeds `nb_pol_max`, and
/// (the original allocates a negative count) for a non-positive count. Class: libm.
///
/// Vertex `j·around + i` of the first circle is `(cos a · r + offset, sin a · r, 0)` with the
/// angle `a` a running sum of `2π / around` (rounded to float once) kept in the register; the
/// other circles are the one before turned about Y by `2π / turns` (float), point by point.
/// Polygon `(j, i)` is `(i + around·j, i + around·jn, in + around·jn, in + around·j)` with `jn`
/// and `in` the next turn and point, wrapping.
pub fn mesh_torus(radius: f32, offset: f32, around: i16, turns: i16, nb_pol_max: i32, env: &mut MeshEnv) -> Option<Mesh> {
    let (n3, n4) = (around as i32, turns as i32);
    if n3 * n4 > nb_pol_max || n3 < 1 || n4 < 1 {
        return None;
    }
    let mut m = Mesh::default();
    m.vertices = vec![vertex([0.0; 3]); (n3 * n4) as usize];
    let step = (x(TWO_PI_D) / F80::from_i32(n3)).to_f32();
    let mut angle = x(0.0f64);
    for i in 0..n3 {
        let a = angle.to_f64();
        let px = (trig::cos(a) * radius + offset).to_f32();
        let py = (trig::sin(a) * radius).to_f32();
        m.vertices[i as usize].pos = [px, py, 0.0];
        angle = angle + step;
    }
    let turn = matrix_y_rotation((x(TWO_PI_D) / F80::from_i32(n4)).to_f32());
    for j in 0..n4 - 1 {
        for i in 0..n3 {
            let from = m.vertices[(j * n3 + i) as usize].pos;
            m.vertices[((j + 1) * n3 + i) as usize].pos = point_through(from, &turn);
        }
    }
    for j in 0..n4 {
        let jn = if j == n4 - 1 { 0 } else { j + 1 };
        for i in 0..n3 {
            let inx = if i == n3 - 1 { 0 } else { i + 1 };
            m.polygons.push(polygon(&[i + n3 * j, i + n3 * jn, inx + n3 * jn, inx + n3 * j]));
        }
    }
    update(&mut m, env);
    Some(m)
}

/// A cone [soft 0x08487514 SI_meshCone]: `height`, base `radius`, `slices` around (the
/// original's third argument), `base` rings on the base disc (fourth) and `side` rings on the
/// slanted side (fifth). `None` when `(base + side) × slices` exceeds `nb_pol_max` or a count
/// is below its minimum (slices 1, base and side 0, together at least 1; the original
/// allocates a negative count). Class: libm (the rotation).
///
/// Vertex 0 is the base centre `(0, −h/3, 0)` (the quotient rounded to float, then the value
/// the rest of the arithmetic starts from) and vertex 1 the apex `(0, 2h/3, 0)`. The first
/// meridian: `base` points out along the base, x a running sum of `r / base` in the register
/// (`y` constant), then `side − 1` points up the side, x falling by `r / side` and y rising by
/// `h / side`, both continuing from the register values (never rounded between steps). The
/// other meridians turn the first about Y by `2π / slices` (float), point by point.
pub fn mesh_cone(height: f32, radius: f32, slices: i16, base: i16, side: i16, nb_pol_max: i32, env: &mut MeshEnv) -> Option<Mesh> {
    let (n3, n4, n5) = (slices as i32, base as i32, side as i32);
    if (n4 + n5) * n3 > nb_pol_max || n3 < 1 || n4 < 0 || n5 < 0 || n4 + n5 < 1 {
        return None;
    }
    let w = n4 + n5 - 1;
    let mut m = Mesh::default();
    // one slot of slack: with no side ring the base loop writes one point more than a meridian
    // holds (into the next meridian's first slot, which the turning overwrites, or past the end
    // of the array when there is only one meridian: heap the original owns, so dropped here)
    m.vertices = vec![vertex([0.0; 3]); (w * n3 + 3) as usize];
    let low = (-x(height) / 3.0f64).to_f32();
    m.vertices[0].pos = [0.0, low, 0.0];
    m.vertices[1].pos = [0.0, (x(low) * -2.0f32).to_f32(), 0.0];
    let mut xr = x(0.0f64);
    let mut yr = x(low);
    if n4 > 0 {
        let d = x(radius) / F80::from_i32(n4);
        for k in 0..n4 {
            xr = xr + d;
            m.vertices[(2 + k) as usize].pos = [xr.to_f32(), yr.to_f32(), 0.0];
        }
    }
    if n5 - 1 > 0 {
        let dy = x(height) / F80::from_i32(n5);
        let dx = -x(radius) / F80::from_i32(n5);
        for k in 0..n5 - 1 {
            yr = yr + dy;
            xr = xr + dx;
            m.vertices[(2 + n4 + k) as usize].pos = [xr.to_f32(), yr.to_f32(), 0.0];
        }
    }
    let turn = matrix_y_rotation((x(TWO_PI_D) / F80::from_i32(n3)).to_f32());
    turn_meridians(&mut m, n3, w, &turn);
    m.vertices.truncate((w * n3 + 2) as usize);
    pole_to_pole_polygons(&mut m, n3, w);
    // the original allocates (base + side) * slices polygons but writes w + 1 = base + side per
    // meridian only when w >= 1; with w = 0 it writes two triangles per meridian into room for
    // one (a heap overrun it never counts): the mesh it updates has the allocated count
    m.polygons.truncate(((n4 + n5) * n3) as usize);
    update(&mut m, env);
    Some(m)
}

/// A cylinder [soft 0x08487900 SI_meshCylinder]: `height`, `radius`, `slices` around (third
/// argument), `cap` rings on each end disc (fourth) and `side` rings on the wall (fifth).
/// `None` when `(side + 2·cap) × slices` exceeds `nb_pol_max` or a count is below its minimum.
/// Class: libm (the rotation).
///
/// Vertex 0 is the bottom centre `(0, −h/2, 0)` and vertex 1 the top centre `(0, h/2, 0)`. The
/// first meridian: `cap` points out along the bottom (x a running sum of `r / cap` in the
/// register), `side` points up the wall (y a running sum of `h / side` starting from the
/// bottom's value, in the register), then `cap − 1` points in along the top (x falling by
/// `r / cap` from the register value). The other meridians turn it about Y as the cone's.
pub fn mesh_cylinder(height: f32, radius: f32, slices: i16, cap: i16, side: i16, nb_pol_max: i32, env: &mut MeshEnv) -> Option<Mesh> {
    let (n3, n4, n5) = (slices as i32, cap as i32, side as i32);
    if (n5 + 2 * n4) * n3 > nb_pol_max || n3 < 1 || n4 < 0 || n5 < 0 || n5 + 2 * n4 < 1 {
        return None;
    }
    let w = n5 + 2 * n4 - 1;
    let mut m = Mesh::default();
    // one slot of slack, as in the cone: with no cap ring the wall loop writes one point more
    // than a meridian holds
    m.vertices = vec![vertex([0.0; 3]); (w * n3 + 3) as usize];
    let low = (-x(height) * 0.5f64).to_f32();
    m.vertices[0].pos = [0.0, low, 0.0];
    m.vertices[1].pos = [0.0, -low, 0.0];
    let mut xr = x(0.0f64);
    let mut yr = x(low);
    let d = if n4 > 0 { x(radius) / F80::from_i32(n4) } else { x(0.0f64) };
    for k in 0..n4 {
        xr = xr + d;
        m.vertices[(2 + k) as usize].pos = [xr.to_f32(), yr.to_f32(), 0.0];
    }
    if n5 > 0 {
        let dy = x(height) / F80::from_i32(n5);
        for k in 0..n5 {
            yr = yr + dy;
            m.vertices[(2 + n4 + k) as usize].pos = [xr.to_f32(), yr.to_f32(), 0.0];
        }
    }
    for k in 0..n4 - 1 {
        xr = xr - d;
        m.vertices[(2 + n4 + n5 + k) as usize].pos = [xr.to_f32(), yr.to_f32(), 0.0];
    }
    let turn = matrix_y_rotation((x(TWO_PI_D) / F80::from_i32(n3)).to_f32());
    turn_meridians(&mut m, n3, w, &turn);
    m.vertices.truncate((w * n3 + 2) as usize);
    pole_to_pole_polygons(&mut m, n3, w);
    // as in the cone: with no side ring and no cap ring the original writes two polygons per meridian into room for one
    m.polygons.truncate(((n5 + 2 * n4) * n3) as usize);
    update(&mut m, env);
    Some(m)
}

/// The rotation about X that stands the icosahedron and the dodecahedron on a vertex, applied
/// to every vertex in place.
fn stand_up(m: &mut Mesh, angle: f32) {
    let turn = matrix_x_rotation(angle);
    for v in &mut m.vertices {
        v.pos = point_through(v.pos, &turn);
    }
}

/// A regular icosahedron [soft 0x08489dd4 SI_meshIcosahedron]: with `s = |size|`,
/// `b = s / 1.902076` and `a = s · 1.618034 / 1.902076` (each rounded to float), twelve
/// vertices `(±a, ±b, 0)`, `(±b, 0, ±a)`, `(0, ±a, ±b)` turned about X by 0.5535741448402405
/// (a float constant), twenty triangles, then the mesh's smoothing angle is set to 40 degrees
/// and its cosine to `cos(0.6981316999999999)` (a double constant, rounded to float) before
/// the update. Class: libm.
pub fn mesh_icosahedron(size: f32, env: &mut MeshEnv) -> Mesh {
    let s = x(size).abs();
    let b = (s / 1.902076f64).to_f32();
    let a = ((s * 1.618034f64) / 1.902076f64).to_f32();
    let mut m = Mesh::default();
    for p in [
        [a, b, 0.0], [-a, b, 0.0], [a, -b, 0.0], [-a, -b, 0.0],
        [b, 0.0, a], [b, 0.0, -a], [-b, 0.0, a], [-b, 0.0, -a],
        [0.0, a, b], [0.0, -a, b], [0.0, a, -b], [0.0, -a, -b],
    ] {
        m.vertices.push(vertex(p));
    }
    stand_up(&mut m, 0.5535741448402405f32);
    for t in [
        [0, 8, 4], [0, 5, 10], [2, 4, 9], [2, 11, 5], [1, 6, 8], [1, 10, 7], [3, 9, 6], [3, 7, 11], [0, 10, 8], [1, 8, 10],
        [2, 9, 11], [3, 11, 9], [4, 2, 0], [5, 0, 2], [6, 1, 3], [7, 3, 1], [8, 6, 4], [9, 4, 6], [10, 5, 7], [11, 7, 5],
    ] {
        m.polygons.push(polygon(&t));
    }
    m.angle = 40.0;
    m.cos_angle = trig::cos(0.6981316999999999f64).to_f32();
    update(&mut m, env);
    m
}

/// A regular dodecahedron [soft 0x0848a26c SI_meshDodecahedron] of circumradius `size` (the
/// argument is used as it is, not as its absolute value). Class: model (all the arithmetic is
/// in the register, with float stores in between where the original has them).
///
/// With `g = ((√5 + 1) · 0.5)` rounded to float, `p = (0.5 · √(3 − g))` rounded to float,
/// `q = (((√(g + 2) · (g + 1)) + 2p) · 0.25)` rounded to float, `r1 = ((g − 1) · q)` rounded,
/// `r2 = ((q − p) · (g − 1))` rounded and `f = size / √(p² + q²)` rounded, the twenty
/// vertices are built from `P = p·f`, `Q = q·f`, `G = (g·p)·f`, `R1 = r1·f`, `R2 = r2·f` (each
/// product in the register): the twelve `(0, ±P, ±Q)`, `(±P, ±Q, 0)`, `(±Q, 0, ±P)` and the
/// eight `(±G, Q − R2 | R2 − Q, ±R1)`. Twelve pentagons; then the whole is turned about X by
/// 0.553574800491333 (float constant) like the icosahedron.
pub fn mesh_dodecahedron(size: f32, env: &mut MeshEnv) -> Mesh {
    let g = ((x(5.0f64).sqrt() + 1.0f64) * 0.5f64).to_f32();
    let p = (x(0.5f64) * (x(3.0f64) - g).sqrt()).to_f32();
    let q = (((x(g) + 2.0f64).sqrt() * (x(g) + 1.0f64) + (x(p) + p)) * 0.25f64).to_f32();
    let gm1 = x(g) - 1.0f64;
    let r1 = (gm1 * q).to_f32();
    let r2 = ((x(q) - p) * gm1).to_f32();
    let gp = x(g) * p;
    let f = (x(size) / (x(p) * p + x(q) * q).sqrt()).to_f32();
    let pf = x(p) * f;
    let qf = x(q) * f;
    let gpf = gp * f;
    let r2f = x(r2) * f;
    let r1f = x(r1) * f;
    let (t2, t3, t6, t8) = (pf.to_f32(), qf.to_f32(), gpf.to_f32(), r1f.to_f32());
    let t7 = (qf - r2f).to_f32();
    let t10 = (r2f - qf).to_f32();
    let mut m = Mesh::default();
    for v in [
        [0.0, -t2, t3], [0.0, t2, t3], [0.0, -t2, -t3], [0.0, t2, -t3],
        [t2, t3, 0.0], [-t2, t3, 0.0], [t2, -t3, 0.0], [-t2, -t3, 0.0],
        [t3, 0.0, t2], [t3, 0.0, -t2], [-t3, 0.0, t2], [-t3, 0.0, -t2],
        [t6, t7, t8], [-t6, t7, t8], [t6, t10, t8], [-t6, t10, t8],
        [t6, t7, -t8], [-t6, t7, -t8], [t6, t10, -t8], [-t6, t10, -t8],
    ] {
        m.vertices.push(vertex(v));
    }
    for t in [
        [0, 14, 8, 12, 1], [0, 1, 13, 10, 15], [4, 5, 13, 1, 12], [6, 14, 0, 15, 7], [18, 9, 8, 14, 6], [9, 16, 4, 12, 8],
        [19, 7, 15, 10, 11], [17, 11, 10, 13, 5], [2, 3, 16, 9, 18], [3, 17, 5, 4, 16], [2, 19, 11, 17, 3], [2, 18, 6, 7, 19],
    ] {
        m.polygons.push(polygon(&t));
    }
    stand_up(&mut m, 0.553574800491333f32);
    update(&mut m, env);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(m: &Mesh) -> bool {
        m.edges[1..].iter().all(|e| e.p0 != -1 && e.p1 != -1)
    }

    /// Every polygon's normal points away from the origin.
    fn outward(m: &Mesh) -> bool {
        m.polygons.iter().all(|p| {
            let c = m.vertices[p.nodes[0].vertex as usize].pos;
            p.normal[0] * c[0] + p.normal[1] * c[1] + p.normal[2] * c[2] > 0.0
        })
    }

    #[test]
    fn cube_as_the_dialog_makes_it() {
        let mut env = MeshEnv::default();
        let m = mesh_cube(1.0, 1.0, 1.0, -1.0, -1.0, -1.0, &mut env);
        assert!(env.messages.is_empty());
        assert_eq!((m.nb_vertices(), m.nb_polygons(), m.nb_edges()), (8, 6, 12));
        assert_eq!(m.vertices[0].pos, [-1.0, -1.0, -1.0]);
        assert_eq!(m.vertices[1].pos, [-1.0, -1.0, 1.0]);
        assert_eq!(m.vertices[6].pos, [1.0, 1.0, -1.0]);
        assert!(closed(&m) && outward(&m));
        assert_eq!(m.polygons[0].normal, [-1.0, 0.0, 0.0]);
        assert_eq!(m.flags, 0x804);
        // 90 degrees is a discontinuity at the default 60: corners take the face normals
        assert!(m.polygons.iter().all(|p| p.nodes.iter().all(|c| c.normal == p.normal)));
    }

    #[test]
    fn grid_sums_its_steps_in_the_register() {
        let mut env = MeshEnv::default();
        let m = mesh_grid(2, 3, 1.0, 0.5, NB_POL_MAX, &mut env).unwrap();
        assert_eq!((m.nb_vertices(), m.nb_polygons(), m.nb_edges()), (12, 6, 17));
        let pos: Vec<[f32; 3]> = m.vertices.iter().map(|v| v.pos).collect();
        assert_eq!(pos[0], [-1.0, 0.0, -0.75]);
        assert_eq!(pos[3], [-1.0, 0.0, 0.75]);
        assert_eq!(pos[11], [1.0, 0.0, 0.75]);
        assert_eq!(m.polygons[0].nodes.iter().map(|c| c.vertex).collect::<Vec<_>>(), [0, 1, 5, 4]);
        assert!(m.polygons.iter().all(|p| p.normal == [0.0, 1.0, 0.0]));
        // a step that is not a binary fraction: the stored values are rounded partial sums
        // of one unrounded running sum, not repeated float additions
        let m = mesh_grid(10, 1, 0.1, 1.0, NB_POL_MAX, &mut env).unwrap();
        let mut run = -0.5f64; // (−10 × 0.1f) × 0.5 rounded to float is −0.5
        let step = 0.1f32 as f64;
        let mut float_sum = -0.5f32;
        let mut differs = false;
        for i in 0..=10 {
            let stored = m.vertices[i * 2].pos[0];
            assert_eq!(stored, run as f32, "row {i}");
            differs |= stored != float_sum;
            run += step;
            float_sum += 0.1;
        }
        assert!(differs);
        assert!(mesh_grid(300, 300, 1.0, 1.0, NB_POL_MAX, &mut env).is_none());
        assert_eq!(mesh_grid(0, 5, 1.0, 1.0, NB_POL_MAX, &mut env).unwrap().nb_vertices(), 6);
    }

    #[test]
    fn sphere_counts_order_and_closure() {
        let mut env = MeshEnv::default();
        let m = mesh_sphere(2.0, 4, 6, NB_POL_MAX, &mut env).unwrap();
        assert!(env.messages.is_empty());
        assert_eq!((m.nb_vertices(), m.nb_polygons()), (3 * 6 + 2, 4 * 6));
        assert_eq!(m.nb_edges(), 6 * 4 + 6 * 3);
        assert_eq!(m.vertices[0].pos, [0.0, -2.0, 0.0]);
        assert_eq!(m.vertices[1].pos, [0.0, 2.0, 0.0]);
        // first meridian, south to north, in the XY plane
        let p = m.vertices[2].pos;
        assert!((p[0] - 2.0f32.sqrt()).abs() < 1e-6 && (p[1] + 2.0f32.sqrt()).abs() < 1e-6 && p[2] == 0.0);
        assert!((m.vertices[3].pos[0] - 2.0).abs() < 1e-6 && m.vertices[3].pos[1].abs() < 1e-6);
        assert!(m.vertices[4].pos[1] > 1.4);
        assert!(closed(&m) && outward(&m));
        assert_eq!(m.polygons[0].nodes.iter().map(|c| c.vertex).collect::<Vec<_>>(), [5, 2, 0]);
        assert_eq!(m.polygons[1].nodes.iter().map(|c| c.vertex).collect::<Vec<_>>(), [2, 5, 6, 3]);
        assert_eq!(m.polygons[3].nodes.iter().map(|c| c.vertex).collect::<Vec<_>>(), [4, 7, 1]);
        // the last meridian joins the first
        assert_eq!(m.polygons[20].nodes.iter().map(|c| c.vertex).collect::<Vec<_>>(), [2, 17, 0]);
        // all on the sphere, to the accuracy of a chain of float rotations
        for v in &m.vertices {
            let l = (v.pos[0] * v.pos[0] + v.pos[1] * v.pos[1] + v.pos[2] * v.pos[2]).sqrt();
            assert!((l - 2.0).abs() < 1e-5);
        }
        assert_eq!(m.flags, 0x804);
        assert!(mesh_sphere(1.0, 1, 8, NB_POL_MAX, &mut env).is_none());
        assert!(mesh_sphere(1.0, 300, 300, NB_POL_MAX, &mut env).is_none());
    }

    #[test]
    fn tetrahedron_and_octahedron() {
        let mut env = MeshEnv::default();
        let t = mesh_tetrahedron(-1.0, &mut env);
        assert_eq!((t.nb_vertices(), t.nb_polygons(), t.nb_edges()), (4, 4, 6));
        assert!(closed(&t) && outward(&t));
        // stood on a face: three corners share one height, the fourth is above on the axis
        let ys: Vec<f32> = t.vertices.iter().map(|v| v.pos[1]).collect();
        let low = ys.iter().cloned().fold(f32::MAX, f32::min);
        assert_eq!(ys.iter().filter(|&&y| (y - low).abs() < 1e-5).count(), 3);
        let top = t.vertices.iter().find(|v| (v.pos[1] - low).abs() > 1e-3).unwrap();
        assert!(top.pos[0].abs() < 1e-5 && top.pos[2].abs() < 1e-5 && (top.pos[1] - 0.99999).abs() < 1e-4);
        let o = mesh_octahedron(-3.0, &mut env);
        assert_eq!((o.nb_vertices(), o.nb_polygons(), o.nb_edges()), (6, 8, 12));
        assert_eq!(o.vertices[3].pos, [0.0, -3.0, 0.0]);
        assert!(closed(&o) && outward(&o));
        assert!(env.messages.is_empty());
        // new meshes keep 0x800 and gain 0x8 on their first normal pass
        assert_eq!((t.flags, o.flags), (0x80c, 0x80c));
    }

    #[test]
    fn revolved_solids_and_platonic_solids() {
        let mut env = MeshEnv::default();
        // torus: around x turns vertices and quads, closed, every vertex on the tube's circle
        let t = mesh_torus(1.0, 3.0, 6, 8, NB_POL_MAX, &mut env).unwrap();
        assert_eq!((t.nb_vertices(), t.nb_polygons(), t.nb_edges()), (48, 48, 96));
        assert!(closed(&t));
        assert!(mesh_torus(1.0, 3.0, 0, 8, NB_POL_MAX, &mut env).is_none());
        // cone: apex and base centre, w = base + side - 1 per meridian
        let c = mesh_cone(3.0, 1.0, 5, 2, 3, NB_POL_MAX, &mut env).unwrap();
        assert_eq!((c.nb_vertices(), c.nb_polygons()), (4 * 5 + 2, 5 * 5));
        assert_eq!(c.vertices[0].pos, [0.0, -1.0, 0.0]);
        assert_eq!(c.vertices[1].pos, [0.0, 2.0, 0.0]);
        assert!(closed(&c));
        // cylinder
        let y = mesh_cylinder(2.0, 1.0, 6, 2, 3, NB_POL_MAX, &mut env).unwrap();
        assert_eq!((y.nb_vertices(), y.nb_polygons()), (6 * 6 + 2, 6 * 7));
        assert_eq!((y.vertices[0].pos, y.vertices[1].pos), ([0.0, -1.0, 0.0], [0.0, 1.0, 0.0]));
        assert!(closed(&y));
        // icosahedron and dodecahedron: closed, outward, equal edge lengths
        let i = mesh_icosahedron(2.0, &mut env);
        assert_eq!((i.nb_vertices(), i.nb_polygons(), i.nb_edges()), (12, 20, 30));
        assert!(closed(&i) && outward(&i));
        assert_eq!(i.angle, 40.0);
        let d = mesh_dodecahedron(2.0, &mut env);
        assert_eq!((d.nb_vertices(), d.nb_polygons(), d.nb_edges()), (20, 12, 30));
        assert!(closed(&d) && outward(&d));
        for v in &d.vertices {
            let l = (v.pos[0] * v.pos[0] + v.pos[1] * v.pos[1] + v.pos[2] * v.pos[2]).sqrt();
            assert!((l - 2.0).abs() < 1e-4, "{l}");
        }
    }
}
