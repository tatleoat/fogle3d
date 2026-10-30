// SPDX-License-Identifier: MIT
//! What the metaball volume sources (`hooks.c`, `range.c`, `vceval.c`, `bi_search.c`, ...) share:
//! the globals of the volume code at their addresses, the metaball record's offsets, and the blending
//! functions (finding 0012, section 28).

use si_mem::Mem;
use si_x87::F80;

pub const VCG_NUM_MBS: u32 = 0x086e_e48c;
pub const VCG_MAX_MBS: u32 = 0x086e_e490;
pub const VCG_NUM_EDGES: u32 = 0x086e_e494;
pub const VCG_MAX_EDGES: u32 = 0x086e_e498;
pub const VCG_NUM_GROUPS: u32 = 0x086e_e49c;
pub const VCG_MB: u32 = 0x0882_99c4;
pub const VCG_EDGE_LIST: u32 = 0x0882_99c8;
pub const VCG_GROUP_LIST: u32 = 0x0882_99cc;
/// `first_init` of `hooks.c` (two statics: `.192` for the local lists, `.196` for the originals).
pub const HOOKS_FIRST_192: u32 = 0x086e_e66c;
pub const HOOKS_FIRST_196: u32 = 0x086e_e66e;
/// `first_init` of `vceval.c`.
pub const EVAL_FIRST_INIT: u32 = 0x086e_ed2c;
pub const LISTALL_MBS_LOCLIST: u32 = 0x0882_9ce0;
pub const LISTALL_GROUPS_LOCLIST: u32 = 0x0882_9ce4;
pub const LIST_MBLINKS_LOCLIST: u32 = 0x0882_9ce8;
/// `vcS_locMBList`: count, size, the indices (a `vcIndexListStruct`: count, size, pointer).
pub const LOC_MB_LIST: u32 = 0x0882_9d24;
/// `vcS_tmpCblist` (a `vcContribListStruct`: count, indices, densities) and `vcS_cblist`.
pub const TMP_CBLIST: u32 = 0x0882_9d30;
pub const CBLIST: u32 = 0x0882_9d3c;
/// The scratch arrays `VC_initEvalContrib` allocates: tmp indices, tmp densities, indices, densities.
pub const EVAL_TMP_IDX: u32 = 0x0882_9d34;
pub const EVAL_TMP_DENS: u32 = 0x0882_9d38;
pub const EVAL_IDX: u32 = 0x0882_9d40;
pub const EVAL_DENS: u32 = 0x0882_9d44;
/// The eight corners (three doubles each) of the cube `VC_getBoundingVolume` transforms.
pub const CORNERS: u32 = 0x086e_e960;
pub const HOOKS_BLOB_SIZE: u32 = 0x1e4;

/// Offsets in a metaball record (0x1e4 bytes, at `vcG_MB + index * 0x1e4`); read from the code.
pub mod mb {
    pub const EXTENT: u32 = 0x08; // three half extents (f64)
    pub const THRESH: u32 = 0x20; // f64
    pub const KSCALE: u32 = 0x28; // f64
    pub const FLAG30: u32 = 0x30; // i16: 1 when the exponents differ from 2
    pub const EXPON: u32 = 0x34; // three f64
    pub const MAT_IN: u32 = 0x4c; // 4x4 f64 as given
    pub const MAT: u32 = 0xcc; // 4x4 f64 copy
    pub const INV: u32 = 0x14c; // inverse of the upper left, 4x4 f64
    pub const NEG_T: u32 = 0x1ac; // minus the translation, three f64
    pub const COLOR: u32 = 0x1cc; // pointer to 9 doubles (three vectors)
    pub const ROUGH: u32 = 0x1d0; // f64
    pub const GROUP: u32 = 0x1dc; // i32
    pub const FREE: u32 = 0x1e0; // i16: 1 when the slot is free
}

/// [soft 0x08575b50 VC_wyvill]: the blending function of the distance `x`, in `st0`: with `s = x*x`,
/// `(1 - s*2.444444) + s*s*1.888888 - s*s*s*0.444444`, every intermediate in the 80-bit register (the three
/// constants are doubles, the argument a double on the stack).
pub fn wyvill(x: f64) -> F80 {
    let s = F80::from_f64(x) * F80::from_f64(x);
    let a = s * F80::from_f64(2.444444);
    let s2 = s * s;
    let s3 = s * s2;
    ((F80::ONE - a) + s2 * F80::from_f64(1.888888)) - s3 * F80::from_f64(0.444444)
}

/// [soft 0x08575b84 VC_wyvill2]: the same blending function of the squared distance `x` itself (no squaring first):
/// `(1 - x*2.444444) + x*x*1.888888 - x*x*x*0.444444`, in `st0`.
pub fn wyvill2(x: f64) -> F80 {
    let x = F80::from_f64(x);
    let a = x * F80::from_f64(2.444444);
    let x2 = x * x;
    let x3 = x * x2;
    ((F80::ONE - a) + x2 * F80::from_f64(1.888888)) - x3 * F80::from_f64(0.444444)
}

/// [soft 0x08575bb8 VC_ratioFromWeight]: the ratio (in `st0`) whose `1 / wyvill` is the weight `w`: 0 for a weight
/// of 1 or less; else 15 bisections of the interval from 0 to 1 starting at `1 - 1/w`, each step's test
/// `1 / wyvill(mid) < w` (a NaN fails it), the interval's ends and the middle held in the 80-bit register, the
/// argument of `wyvill` rounded to a double.
pub fn ratio_from_weight(w: f64) -> F80 {
    let w = F80::from_f64(w);
    let one = F80::ONE;
    if !w.gt(one) && !w.unordered(one) {
        return F80::ZERO;
    }
    let (mut lo, mut hi) = (F80::ZERO, one);
    let mut mid = one - one / w;
    for _ in 0..15 {
        let q = one / wyvill(mid.to_f64());
        if q.lt(w) {
            lo = mid;
        } else {
            hi = mid;
        }
        mid = (lo + hi) * F80::from_f64(0.5);
    }
    mid
}

/// [soft 0x08575c58 VC_weightFromRatio]: `1 / wyvill(r)` in `st0` for a ratio above 0 (or NaN), else 999.
pub fn weight_from_ratio(r: f64) -> F80 {
    if r <= 0.0 {
        return F80::from_f64(999.0);
    }
    F80::ONE / wyvill(r)
}

/// The record of metaball `i`.
pub fn mb_at(m: &Mem, i: i32) -> u32 {
    m.u32(VCG_MB).wrapping_add((i as u32).wrapping_mul(HOOKS_BLOB_SIZE))
}
