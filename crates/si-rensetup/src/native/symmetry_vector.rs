// SPDX-License-Identifier: MIT
//! `symmetry.c`, the vector a model's symmetry gives: `si_symmetryVector` and its older twin
//! `si_symmetryVectorOLD`. Both take two models and a component (0 scaling, 1 rotation
//! angles, 2 position) and answer, in a vector, the mirrored value of the second
//! model's component for the first, seen in the plane of the symmetry record. They work on
//! the models' words in the image (scaling at +0x100 / +0x16c (local / global), local matrix +0x118,
//! global matrix +0x178, global translation +0x1b8, father +0x2bc) and call no routine of
//! another package: the matrix routines are `si-math`'s, the two walks up the hierarchy are
//! below.
//!
//! Where the x87 unit's order matters (finding 0023, section 47): a sum of three products is
//! formed in the register as `(p0 + p1) + p2` and rounded to a float once; the positions
//! `second - ancestor` stay unrounded in the register for the three sums that use them
//! (`fst` to the output word is overwritten later); the global position adds the matrix's
//! fourth row and then its translation to the unrounded sum, rounding once per component.
//!
//! **Compared with the emulated original** (finding 0023, section 47): see
//! `re/tools/sym1/emu_gen_symmetry_vector.py` and `rensetup-emucheck`.
//!
//! Where the original reads memory it did not set, Rust writes zeros and says so at the spot:
//! a component other than 0, 1 and 2 (the output vector is the stack's old content), a
//! plane other than 0, 1 and 2 (the three reflection factors), a singular inverse in the old
//! routine (its destination matrix), and a null record with two different models (the
//! original reads the word at address 8).

use super::globals::SI_GLB_MANIP_KEEP_TURNS;
use si_math::algebra::{matrix_copy, matrix_extract_xyz_rotation, matrix_inverse, matrix_multiply, matrix_transpose, matrix_xyz_rotation};
use si_math::types::{Matrix, Vector};
use si_math::x87::x;
use si_mem::Mem;

si_mem::globals! {
    /// The plane used when a symmetry has no record (`symPlane`, a short; the record's own
    /// first word, which this one stands in for). 0, 1 and 2 are the three planes.
    SYM_PLANE: i16 = 0x08699a24, "symPlane", init 2;
}

const GLOBAL_MATRIX: u32 = 0x178;
const LOCAL_MATRIX: u32 = 0x118;
const GLOBAL_TRANSLATION: u32 = 0x1b8;
const FATHER: u32 = 0x2bc;

fn mat(m: &Mem, a: u32) -> Matrix {
    let mut r = [[0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 16 * i as u32 + 4 * j as u32);
        }
    }
    r
}

fn vec3(m: &Mem, a: u32) -> Vector {
    Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) }
}

/// `fld` / `fchs` / `fstp`: the negated float, a signalling NaN coming out quiet.
fn neg(v: f32) -> f32 {
    (-x(v)).to_f32()
}

/// `SI_matrixExtractXyzRotation(matrix, &angles)` with the angles zeroed before the call.
fn extract(m: &Mem, matrix: &Matrix) -> Vector {
    matrix_extract_xyz_rotation(matrix, &Vector { x: 0.0, y: 0.0, z: 0.0 }, m[SI_GLB_MANIP_KEEP_TURNS] != 0)
}

/// modelDepth [soft 0x08473fa0 modelDepth]: the number of nodes from a model up to its root
/// (0 for null).
fn model_depth(m: &Mem, a: u32) -> i32 {
    if a == 0 {
        return 0;
    }
    let mut d = 0;
    let mut p = a;
    loop {
        p = m.u32(p + FATHER);
        d += 1;
        if p == 0 {
            return d;
        }
    }
}

/// The deepest model both are below, a model being its own ancestor; 0 if either is null or
/// they are in different trees [soft 0x08474304 SI_modelCommonAncestor].
pub fn model_common_ancestor(m: &Mem, a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 {
        return 0;
    }
    if a == b {
        return a;
    }
    let (mut a, mut b) = (a, b);
    let (da, db) = (model_depth(m, a), model_depth(m, b));
    if da > db {
        for _ in 0..da - db {
            a = m.u32(a + FATHER);
        }
    } else if db > da {
        for _ in 0..db - da {
            b = m.u32(b + FATHER);
        }
    }
    while a != b {
        a = m.u32(a + FATHER);
        b = m.u32(b + FATHER);
    }
    a
}

/// The plane and the ancestor both routines start with.
fn prelude(m: &Mem, a: u32, b: u32, data: u32) -> (i16, u32) {
    let plane = if data != 0 { m.i16(data) } else { m[SYM_PLANE] };
    // adapter-free difference: with a null record and two different models the original
    // reads the word at address 8 (a fault); Rust takes it as not 1.
    let anc = if a != b && data != 0 && m.i16(data + 8) == 1 { model_common_ancestor(m, a, b) } else { 0 };
    (plane, anc)
}

/// Component 0: the second model's scaling (`flag` set: the one at +0x16c, else +0x100),
/// three words copied.
fn rotation_words(m: &Mem, b: u32, flag: i16) -> [u32; 3] {
    let o = if flag != 0 { 0x16c } else { 0x100 };
    [m.u32(b + o), m.u32(b + o + 4), m.u32(b + o + 8)]
}

/// Component 2, shared by both routines: the second model's position, relative to the common
/// ancestor when there is one, the plane's reflection applied, and carried into the ancestor's
/// frame again.
fn position(m: &Mem, b: u32, flag: i16, plane: i16, anc: u32) -> [u32; 3] {
    let mut v: [u32; 3];
    if anc != 0 {
        let g = mat(m, anc + GLOBAL_MATRIX);
        let d = [
            x(m.f32(b + GLOBAL_TRANSLATION)) - m.f32(anc + GLOBAL_TRANSLATION),
            x(m.f32(b + GLOBAL_TRANSLATION + 4)) - m.f32(anc + GLOBAL_TRANSLATION + 4),
            x(m.f32(b + GLOBAL_TRANSLATION + 8)) - m.f32(anc + GLOBAL_TRANSLATION + 8),
        ];
        // each component: (d0 g[0][c] + d1 g[1][c]) + d2 g[2][c]; rows 0 and 1 of the first
        // two sums are formed as g[c][0] d0 + g[c][1] d1 in the original's row order
        let row = |r: usize| (((d[0] * g[r][0]) + (d[1] * g[r][1])) + (d[2] * g[r][2])).to_f32().to_bits();
        v = [row(0), row(1), row(2)];
    } else {
        let o = if flag != 0 { 0x158 } else { GLOBAL_TRANSLATION };
        v = [m.u32(b + o), m.u32(b + o + 4), m.u32(b + o + 8)];
    }
    match plane {
        0 => v[2] = neg(f32::from_bits(v[2])).to_bits(),
        1 => v[1] = neg(f32::from_bits(v[1])).to_bits(),
        2 => v[0] = neg(f32::from_bits(v[0])).to_bits(),
        _ => {}
    }
    if anc != 0 {
        let g = mat(m, anc + GLOBAL_MATRIX);
        let (px, py, pz) = (f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2]));
        // t_c = ((p.x g[0][c] + p.y g[1][c]) + p.z g[2][c]) + g[3][c], then + the translation
        let col = |c: usize, t: u32| {
            let s = ((x(px) * g[0][c] + x(py) * g[1][c]) + x(pz) * g[2][c]) + g[3][c];
            (s + m.f32(anc + t)).to_f32().to_bits()
        };
        v = [col(0, GLOBAL_TRANSLATION), col(1, GLOBAL_TRANSLATION + 4), col(2, GLOBAL_TRANSLATION + 8)];
    }
    v
}

fn put(m: &mut Mem, out: u32, v: [u32; 3]) {
    m.set_u32(out, v[0]);
    m.set_u32(out + 4, v[1]);
    m.set_u32(out + 8, v[2]);
}

/// The vector the symmetry gives for the second model's component `which`, in `out`
/// (three floats) [soft 0x08244968 si_symmetryVector]; the model `a` is the one asking,
/// `b` the one it is symmetric to, `data` the symmetry record (null: the plane `symPlane`),
/// `flag` the "global" switch. Component 1 with `flag` set below a common ancestor is the
/// old routine's work.
pub fn symmetry_vector(m: &mut Mem, a: u32, b: u32, which: i32, data: u32, flag: i16, out: u32) {
    let (plane, anc) = prelude(m, a, b, data);
    if which == 1 && flag != 0 && anc != 0 {
        symmetry_vector_old(m, a, b, 1, data, flag, out);
        return;
    }
    let v: [u32; 3] = match which {
        0 => rotation_words(m, b, flag),
        1 => {
            let (m124, m164) = if anc != 0 {
                let r = matrix_xyz_rotation(&vec3(m, data + 0x24));
                (r, matrix_copy(&r))
            } else if flag != 0 {
                (matrix_xyz_rotation(&vec3(m, data + 0xc)), matrix_xyz_rotation(&vec3(m, data + 0x18)))
            } else {
                let r = matrix_xyz_rotation(&vec3(m, data + 0x18));
                (r, matrix_copy(&r))
            };
            let m64 = if anc != 0 {
                let t = matrix_multiply(&mat(m, b + GLOBAL_MATRIX), &matrix_transpose(&mat(m, anc + GLOBAL_MATRIX)));
                matrix_multiply(&m124, &matrix_transpose(&t))
            } else {
                let base = if flag != 0 { LOCAL_MATRIX } else { GLOBAL_MATRIX };
                matrix_multiply(&mat(m, b + base), &matrix_transpose(&m124))
            };
            // the reflection factors: (1, 1, 0) for plane 0, (1, 0, 1) for 1, (0, 1, 1) for 2
            // (another plane: the original's stack, zeros here)
            let f: [f32; 3] = match plane {
                0 => [1.0, 1.0, 0.0],
                1 => [1.0, 0.0, 1.0],
                2 => [0.0, 1.0, 1.0],
                _ => [0.0; 3],
            };
            let row = |r: usize| ((x(m164[r][0]) * f[0] + x(m164[r][1]) * f[1]) + x(m164[r][2]) * f[2]).to_f32();
            let w = [row(0), row(1), row(2)];
            let mut ang = extract(m, &m64);
            let half = x(0.5f64);
            if x(w[0]).abs().gt(half) {
                ang.x = neg(ang.x);
            }
            if x(w[1]).abs().gt(half) {
                ang.y = neg(ang.y);
            }
            if x(w[2]).abs().gt(half) {
                ang.z = neg(ang.z);
            }
            let me4 = matrix_xyz_rotation(&ang);
            let mut r = if anc != 0 {
                let t = matrix_multiply(&m124, &mat(m, anc + GLOBAL_MATRIX));
                matrix_multiply(&matrix_transpose(&me4), &t)
            } else {
                matrix_multiply(&me4, &m124)
            };
            if anc != 0 && flag != 0 {
                let father = m.u32(a + FATHER);
                if father != 0 {
                    r = matrix_multiply(&r, &matrix_transpose(&mat(m, father + GLOBAL_MATRIX)));
                }
            }
            let v = extract(m, &r);
            [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
        }
        2 => position(m, b, flag, plane, anc),
        // another component: the original copies its uninitialised stack; zeros here
        _ => [0; 3],
    };
    put(m, out, v);
}

/// The older routine, which `si_symmetryVector` hands component 1 with `flag` below a common
/// ancestor [soft 0x082444e4 si_symmetryVectorOLD]: the angles come from the second model's
/// matrix times the inverse of the ancestor's, the plane flips two of them, and with an
/// ancestor they are carried through its matrix again (and the asking model's father's
/// inverse with `flag`).
pub fn symmetry_vector_old(m: &mut Mem, a: u32, b: u32, which: i32, data: u32, flag: i16, out: u32) {
    let (plane, anc) = prelude(m, a, b, data);
    let v: [u32; 3] = match which {
        0 => rotation_words(m, b, flag),
        1 => {
            // a singular inverse leaves the destination (the stack) untouched: zeros here
            let inv = |matrix: &Matrix| matrix_inverse(matrix).unwrap_or([[0.0; 4]; 4]);
            let mut ang = if anc != 0 {
                let p = matrix_multiply(&mat(m, b + GLOBAL_MATRIX), &inv(&mat(m, anc + GLOBAL_MATRIX)));
                extract(m, &p)
            } else {
                extract(m, &mat(m, b + if flag != 0 { LOCAL_MATRIX } else { GLOBAL_MATRIX }))
            };
            match plane {
                0 => {
                    ang.x = neg(ang.x);
                    ang.y = neg(ang.y);
                }
                1 => {
                    ang.x = neg(ang.x);
                    ang.z = neg(ang.z);
                }
                2 => {
                    ang.y = neg(ang.y);
                    ang.z = neg(ang.z);
                }
                _ => {}
            }
            if anc != 0 {
                let m8c = matrix_xyz_rotation(&ang);
                let mut r = matrix_multiply(&m8c, &mat(m, anc + GLOBAL_MATRIX));
                if flag != 0 {
                    let father = m.u32(a + FATHER);
                    if father != 0 {
                        r = matrix_multiply(&r, &inv(&mat(m, father + GLOBAL_MATRIX)));
                    }
                }
                let v = extract(m, &r);
                [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
            } else {
                [ang.x.to_bits(), ang.y.to_bits(), ang.z.to_bits()]
            }
        }
        2 => position(m, b, flag, plane, anc),
        _ => [0; 3],
    };
    put(m, out, v);
}
