// SPDX-License-Identifier: MIT
//! `metaball.c` on the image (finding 0015): the small routines `SI_metaballTouch`
//! (0x0845abf8) calls. A metaball record (`SI_Metaball`) has no `si-types` layout; the
//! fields these routines read are addressed by the named offsets below.

use si_math::types::Matrix;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{x, F80};

use super::{at, mx};

/// `SI_Metaball` + 0x08: the metaball's node (an `__SI_Model`).
pub const BALL_MODEL: u32 = 0x08;
/// `SI_Metaball` + 0x0c: a 4 x 4 float matrix, the ball's own placement.
pub const BALL_MATRIX: u32 = 0x0c;
/// `SI_Metaball` + 0xcc: a double, the ball's shrink factor (1 - it is how much a scaled
/// box gives up).
pub const BALL_SHRINK: u32 = 0xcc;
/// `SI_MetaSystem` as a node: the kind (`__SI_Model` + 0x30) that marks a metaball system.
const SYSTEM_KIND: i32 = 7;

/// The corners of the unit box [`boxCoord`, `.rodata` 0x086d5b60]: eight triples of
/// floats, every coordinate -1 or +1, x varying slowest.
const BOX_COORD: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, 1.0, 1.0],
    [1.0, -1.0, -1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, -1.0],
    [1.0, 1.0, 1.0],
];

/// The metaball system node a metaball hangs under [soft 0x0845cd40 SI_metaballGetParentSystem]:
/// the first of its node's fathers (the node's father, then that one's, ...) whose kind
/// (`+0x30`) is 7; 0 when the chain ends first. The node itself is not asked.
pub fn get_parent_system(m: &mut Mem, ball: u32) -> u32 {
    let mut node = Ptr32::<Model>::new(m.u32(ball.wrapping_add(BALL_MODEL)));
    loop {
        node = m[node].father;
        if node.is_null() {
            return 0;
        }
        if m[node].kind == SYSTEM_KIND {
            return node.addr();
        }
    }
}

/// `if (v < *dst) *dst = v` on a double in memory: the register value is compared with the
/// whole double (nothing is kept when unordered), and stored as a double.
fn keep_min(m: &mut Mem, dst: u32, v: F80) {
    if v.lt(x(m.f64(dst))) {
        m.set_f64(dst, v.to_f64());
    }
}

/// `if (v > *dst) *dst = v`, likewise.
fn keep_max(m: &mut Mem, dst: u32, v: F80) {
    if v.gt(x(m.f64(dst))) {
        m.set_f64(dst, v.to_f64());
    }
}

/// Widens the box at `bbox` (six doubles: minima x, y, z, then maxima x, y, z) by the eight
/// corners of the unit box put through the metaball's matrix [soft 0x0845c568 metaballCheckBoundingBox].
///
/// The matrix is the ball's own (`ball + 0x0c`) times the global matrix of the node its
/// geometry depends on (`latdep`, the model's +0x44); when the ball hangs under a metaball
/// system, that product is then multiplied by the inverse of the system's global matrix
/// (the product goes into itself). **A system matrix without an inverse leaves the
/// inverse as the stack had it** (counted, [`crate::env::stale`]).
///
/// With `scale` above 0 (a NaN is not above) each corner is first multiplied by
/// `1 - (1 - shrink) * scale` (computed at 80 bits from the double at +0xcc and the float)
/// and the three products are stored as floats. Each corner then goes through the matrix at
/// 80 bits, `((x m0j + y m1j) + z m2j) + m3j` for the three columns, and the results are
/// rounded to floats; each is compared with the box's double **as it is** (no rounding of
/// the double to a float). The minima are tried before the maxima, each in x, y, z order.
pub fn check_bounding_box(m: &mut Mem, ball: u32, bbox: u32, scale: f32) {
    let t = m.local(0x40);
    let model = Ptr32::<Model>::new(m.u32(ball.wrapping_add(BALL_MODEL)));
    let latdep = m[model].latdep;
    mx::multiply(m, ball.wrapping_add(BALL_MATRIX), at!(latdep, gmat), t);
    let system = get_parent_system(m, ball);
    if system != 0 {
        let inv = m.local(0x40);
        if !mx::inverse(m, at!(Ptr32::<Model>::new(system), gmat), inv) {
            crate::env::stale::note();
        }
        mx::multiply(m, t, inv, t);
        m.release(0x40);
    }
    let tm: Matrix = super::mat(m, t);
    m.release(0x40);
    let scaled = x(0.0f32).lt(x(scale));
    for corner in BOX_COORD {
        let [mut cx, mut cy, mut cz] = corner;
        if scaled {
            // 1 - (1 - shrink) * scale, one register value; the products are stored (rounded)
            let s = x(1.0f32) - (x(1.0f32) - x(m.f64(ball.wrapping_add(BALL_SHRINK)))) * x(scale);
            cx = (s * x(cx)).to_f32();
            cy = (s * x(cy)).to_f32();
            cz = (s * x(cz)).to_f32();
        }
        let (px, py, pz) = (x(cx), x(cy), x(cz));
        let ox = ((px * tm[0][0] + py * tm[1][0]) + pz * tm[2][0]) + tm[3][0];
        let oy = ((px * tm[0][1] + py * tm[1][1]) + pz * tm[2][1]) + tm[3][1];
        let oz = ((px * tm[0][2] + py * tm[1][2]) + pz * tm[2][2]) + tm[3][2];
        // stored as floats, loaded again
        let (ox, oy, oz) = (x(ox.to_f32()), x(oy.to_f32()), x(oz.to_f32()));
        keep_min(m, bbox, ox);
        keep_min(m, bbox.wrapping_add(8), oy);
        keep_min(m, bbox.wrapping_add(0x10), oz);
        keep_max(m, bbox.wrapping_add(0x18), ox);
        keep_max(m, bbox.wrapping_add(0x20), oy);
        keep_max(m, bbox.wrapping_add(0x28), oz);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_math::types::IDENTITY;

    fn node(m: &mut Mem, kind: i32) -> Ptr32<Model> {
        let n: Ptr32<Model> = Ptr32::new(m.place(0x430, 8));
        m[n].latdep = n;
        m[n].kind = kind;
        m[n].gmat = IDENTITY;
        n
    }

    fn ball(m: &mut Mem, model: Ptr32<Model>) -> u32 {
        let b = m.place(0xd8, 8);
        m.set_u32(b + BALL_MODEL, model.addr());
        super::super::put_mat(m, b + BALL_MATRIX, &IDENTITY);
        m.set_f64(b + BALL_SHRINK, 0.5);
        b
    }

    fn start_box(m: &mut Mem, bx: u32) {
        for (i, v) in [1.0e10, 1.0e10, 1.0e10, -1.0e10, -1.0e10, -1.0e10].iter().enumerate() {
            m.set_f64(bx + 8 * i as u32, *v);
        }
    }

    #[test]
    fn the_system_is_the_first_father_of_kind_7() {
        let mut m = Mem::new();
        let (a, b, c) = (node(&mut m, 7), node(&mut m, 4), node(&mut m, 7));
        m[a].father = b;
        m[b].father = c;
        let ball = ball(&mut m, a);
        // the ball's own node is not asked
        assert_eq!(get_parent_system(&mut m, ball), c.addr());
        m[c].kind = 1;
        assert_eq!(get_parent_system(&mut m, ball), 0);
    }

    #[test]
    fn a_unit_box_widens_the_box_and_a_scale_shrinks_it() {
        let mut m = Mem::new();
        let n = node(&mut m, 8);
        m[n].gmat[3][0] = 10.0;
        let b = ball(&mut m, n);
        let bx = m.place(48, 8);
        start_box(&mut m, bx);
        check_bounding_box(&mut m, b, bx, 0.0);
        let got: Vec<f64> = (0..6).map(|i| m.f64(bx + 8 * i)).collect();
        assert_eq!(got, [9.0, -1.0, -1.0, 11.0, 1.0, 1.0]);
        // a scale of 1 with a shrink of 0.5 halves the box; a NaN scale is no scale
        start_box(&mut m, bx);
        check_bounding_box(&mut m, b, bx, 1.0);
        let got: Vec<f64> = (0..6).map(|i| m.f64(bx + 8 * i)).collect();
        assert_eq!(got, [9.5, -0.5, -0.5, 10.5, 0.5, 0.5]);
        start_box(&mut m, bx);
        check_bounding_box(&mut m, b, bx, f32::NAN);
        assert_eq!(m.f64(bx), 9.0);
        assert!(m.fault().is_none());
    }
}
