// SPDX-License-Identifier: MIT
//! `envelopset.c`: assigning the points of a skin to the bones of a skeleton, and their
//! weights. The first pass ported the geometric core of the assignment (where a point lies
//! along a bone, [`comp_ratio`]), the two lists the file builds while it assigns, and the
//! routines that change one weight and bring the others back to a sum of 100. The second
//! pass (finding 0029, section 14) the assignment itself (by bounding volume and by
//! distance, local and global skeletons), making and unmaking skeletons, assigning anew,
//! and editing weights. What is left of the file (adding parts, the display colours, the
//! dialog) is listed in section 16.

use crate::env_globals::{PREC, SI_OBJECT_WM, SI_SELECT1_WM, SI_SELECT2_WM, SI_SELECT4_WM, SI_SELECT5_WM, SI_ASSGN_JNTS_DEF_NB, SI_ED_SUMATRA_TO_3X, SI_SELECT3_WM, SI_SELECT6_WM, SI_SELECT7_WM, SI_SELECT_STEP, SI_UNSELECT_WM, SKEL};
use crate::env_outside::{SI_FIND_GL_DATA, SI_MESH_POLYGON_CENTROID, SI_MODEL_GLOBAL_STATE};
use crate::env_records::{EnvInfo, EnvList, EnvVert};
use crate::envtools::{envelop_copy_def, envelop_disp_def, envelop_vert_allocate};
use crate::globals::{CID_MODELS, SI_CID, SI_GRAF_LISTS, SI_PROC_DATA, SI_RD};
use crate::outside::{Outside, SI_ELEMDICT_ADD_RELATION, SI_ELEMDICT_FIND_ELEM_BY_DATA};
use crate::{cur_dict, rd3, V3};
use si_math::algebra::{matrix_inverse, vector_normalize};
use si_math::types::{Matrix, Vector};
use si_mem::{record, Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, trig, x, F80};

type Node = Ptr32<Model>;

/// A bone is shorter than this when the square of its length is below it (the double at
/// 0x08661028).
pub const SHORT: f64 = 0.0001;
pub const SHORT_AT: u32 = 0x0866_1028;
/// The sum the weights of a point are brought to, and the most one weight may be (the
/// doubles at 0x08661080 and 0x086610f8, the float at 0x08661100).
pub const HUNDRED: f64 = 100.0;
pub const HUNDRED_AT: [u32; 2] = [0x0866_1080, 0x0866_10f8];
pub const HUNDRED_F: f32 = 100.0;
pub const HUNDRED_F_AT: u32 = 0x0866_1100;
/// A weight changed "by a percentage" is changed by weight × amount × 0.01 (the double at
/// 0x086610f0).
pub const PERCENT: f64 = 0.01;
pub const PERCENT_AT: u32 = 0x0866_10f0;

record! {
    /// One bone of a list the assignment builds (`jntlist`, 0x14 bytes from `SI_malloc`):
    /// the skeleton's list [`crate::env_globals::SKEL`] and the two lists of
    /// [`env_to_jnt_assign`].
    pub struct JntList = "jntlist", size 0x14 {
        0x00 bone: Ptr32<Model>, "the bone; 0 at creation";
        0x04 end: Ptr32<Model>, "in the skeleton's list, the node the bone runs to (its global translation is the bone's far end); 0 at creation";
        0x08 ratio: f32, "in the list of nearest bones, where the point lies along the bone (compRatio); 0 at creation";
        0x0c dist: f32, "in the list of nearest bones, the point's distance from the bone; 0 at creation";
        0x10 next: Ptr32<JntList>, "the next element";
    }
}

record! {
    /// One vertex entry of a list the weight routines build (`vertlist`, 8 bytes from
    /// `SI_malloc`).
    pub struct VertList = "vertlist", size 0x08 {
        0x00 vert: Ptr32<EnvVert>, "the vertex entry of a bone's list";
        0x04 next: Ptr32<VertList>, "the next element";
    }
}

si_mem::globals! {
    /// `envelopset.c`'s running sum of the weights of one point over its bones, while one
    /// of them is changed (`SI_envelopModifyWeight`, `SI_envelopVertWeightBalance`).
    TOTWGHT: f32 = 0x086d7328, "totwght", init 0;
}

/// Writes the initial value of the global declared here (0 in the binary).
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
}

/// A new element of a list of bones [soft 0x084b7a90 skelJntAllocate]: 0x14 bytes from
/// `SI_malloc`, all 0; written without a test for null.
pub fn skel_jnt_allocate(m: &mut Mem) -> Ptr32<JntList> {
    let j: Ptr32<JntList> = Ptr32::new(m.si_malloc(0x14));
    m[j].ratio = 0.0;
    m[j].dist = 0.0;
    m[j].bone = Ptr32::NULL;
    m[j].end = Ptr32::NULL;
    m[j].next = Ptr32::NULL;
    j
}

/// Frees a list of bones and clears the pointer to it [soft 0x084b7ac8 skelJntDispose].
pub fn skel_jnt_dispose(m: &mut Mem, slot: Ptr32<Ptr32<JntList>>) {
    let mut j = m[slot];
    if j.is_null() {
        return;
    }
    loop {
        let next = m[j].next;
        m.free(j.addr());
        j = next;
        if j.is_null() {
            break;
        }
    }
    m[slot] = Ptr32::NULL;
}

/// A new element of a list of vertex entries [soft 0x084b7b08 vrtlstAllocate]: 8 bytes
/// from `SI_malloc`, both words 0; written without a test for null.
pub fn vrtlst_allocate(m: &mut Mem) -> Ptr32<VertList> {
    let v: Ptr32<VertList> = Ptr32::new(m.si_malloc(8));
    m[v].vert = Ptr32::NULL;
    m[v].next = Ptr32::NULL;
    v
}

/// Frees a list of vertex entries and clears the pointer to it
/// [soft 0x084b7b2c vrtlstDispose].
pub fn vrtlst_dispose(m: &mut Mem, slot: Ptr32<Ptr32<VertList>>) {
    let mut v = m[slot];
    if v.is_null() {
        return;
    }
    loop {
        let next = m[v].next;
        m.free(v.addr());
        v = next;
        if v.is_null() {
            break;
        }
    }
    m[slot] = Ptr32::NULL;
}

/// Where a point lies along a bone, and how far from it [soft 0x084b8914 compRatio].
///
/// With u = end − start (each coordinate rounded to float) and d = point − start (kept in
/// the register): the answer is t = float(D / |u|²), D = float(float(d.x u.x) + d.y u.y)
/// + d.z u.z, the place of the point's projection on the bone's line in bone lengths (not
/// clamped). The distance stored is to the nearest point of the segment: |d| for t below
/// 0, |point − end| for t above 1, else |point − (start + t u)| (the foot's x with t
/// unrounded and kept in the register, its y and z rounded to float, z made with t rounded
/// first). Lengths are float(sqrt((y² + x²) + z²)). For a bone shorter than 0.0001 (|u|²
/// below it): distance |d|, answer 0.
pub fn comp_ratio(m: &mut Mem, point: [f32; 3], start: [f32; 3], end: [f32; 3], dist: Ptr32<f32>) -> F80 {
    let d = [x(point[0]) - start[0], x(point[1]) - start[1], x(point[2]) - start[2]];
    let u = [(x(end[0]) - start[0]).to_f32(), (x(end[1]) - start[1]).to_f32(), (x(end[2]) - start[2]).to_f32()];
    let length = |e: [F80; 3]| ((e[1] * e[1] + e[0] * e[0]) + e[2] * e[2]).sqrt().to_f32();
    let q = x(u[2]) * x(u[2]) + (x(u[1]) * x(u[1]) + x(u[0]) * x(u[0]));
    if q.lt(x(SHORT)) {
        m[dist] = length(d);
        return x(0.0f32);
    }
    let s = (d[0] * u[0]).to_f32();
    let s = (d[1] * u[1] + s).to_f32();
    let t = (d[2] * u[2] + s) / q;
    let t32 = t.to_f32();
    let foot_x = t * u[0] + start[0];
    let foot_y = (t * u[1] + start[1]).to_f32();
    let foot_z = (x(t32) * u[2] + start[2]).to_f32();
    let e = if x(t32).lt(x(0.0f32)) {
        d
    } else if x(1.0f32).lt(x(t32)) {
        [x(point[0]) - end[0], x(point[1]) - end[1], x(point[2]) - end[2]]
    } else {
        [x(point[0]) - foot_x, x(point[1]) - foot_y, x(point[2]) - foot_z]
    };
    m[dist] = length(e);
    x(t32)
}

/// Brings the weights of a point's other bones down so that the sum is 100 again, and frees
/// the list of them [soft 0x084ba374 SI_envelopVertWeightBalance].
///
/// `weight` is the new weight of the bone that was changed, the sum of the weights before
/// is in `totwght`, and `list` holds the other bones' vertex entries. When the sum is above
/// 100, r = float((sum − 100) / (sum − weight)) and each entry's weight becomes
/// float(w − w × r). `totwght` is cleared and the list freed either way.
pub fn envelop_vert_weight_balance(m: &mut Mem, weight: f32, list: Ptr32<Ptr32<VertList>>) {
    let total = x(m[TOTWGHT]);
    if x(HUNDRED).lt(total) {
        let r = ((total - HUNDRED) / (total - weight)).to_f32();
        let mut p = m[list];
        while !p.is_null() {
            let v = m[p].vert;
            let w = m[v].weight;
            m[v].weight = (x(w) - x(w) * r).to_f32();
            p = m[p].next;
        }
    }
    m[TOTWGHT] = 0.0;
    vrtlst_dispose(m, list);
}

/// One vertex entry met while a point's weight on bone `changed` is set
/// [soft 0x084bb6ec SI_envelopModifyWeight]. `bone` is the bone the entry belongs to (null:
/// the entry's weight becomes 0); `mode` 1 adds the amount at `amount` to the entry's
/// weight, 2 adds that percentage of it, anything else sets the weight to the amount.
///
/// For the bone that is changed: the new weight (written back to `amount`, at least 0, at
/// most 100) becomes the entry's and is added to `totwght`. For any other bone the entry's
/// weight is added to `totwght` and the entry goes in front of the list `list` (for
/// [`envelop_vert_weight_balance`]).
pub fn envelop_modify_weight(m: &mut Mem, v: Ptr32<EnvVert>, changed: Ptr32<Model>, bone: Ptr32<Model>, mode: i32, amount: Ptr32<f32>, list: Ptr32<Ptr32<VertList>>) {
    if bone.is_null() {
        m[v].weight = 0.0;
        return;
    }
    if changed != bone {
        m[TOTWGHT] = (x(m[TOTWGHT]) + m[v].weight).to_f32();
        let n = vrtlst_allocate(m);
        m[n].vert = v;
        let head = m[list];
        if !head.is_null() {
            m[n].next = head;
        }
        m[list] = n;
        return;
    }
    if mode == 1 {
        m[amount] = (x(m[v].weight) + m[amount]).to_f32();
    } else if mode == 2 {
        let w = x(m[v].weight);
        m[amount] = (w + (w * m[amount]) * PERCENT).to_f32();
    }
    let a = x(m[amount]);
    if !(x(0.0f32).lt(a) || x(0.0f32).unordered(a)) {
        m[amount] = 0.0;
    } else if !(a.lt(x(HUNDRED)) || a.unordered(x(HUNDRED))) {
        m[amount] = HUNDRED_F;
    }
    let a = m[amount];
    m[v].weight = si_x87::fmov(a);
    m[TOTWGHT] = (x(a) + m[TOTWGHT]).to_f32();
}

// ------------------------------------------------------------ second pass: automatic assignment

/// The corners of the box a mesh volume's points span start at these (the floats at
/// 0x08660fe8 and 0x08660fec).
pub const BIG: f32 = 100000000.0;
pub const BIG_AT: u32 = 0x0866_0fe8;
pub const MINUS_BIG: f32 = -100000000.0;
pub const MINUS_BIG_AT: u32 = 0x0866_0fec;
/// A volume's extent is half its scaling along each axis (the double at 0x08660ff0).
pub const BOUND_HALF: f64 = 0.5;
pub const BOUND_HALF_AT: u32 = 0x0866_0ff0;
/// A point's distance from a bone is at least this (the float at 0x08661030).
pub const MIN_DIST: f32 = 0.0001;
pub const MIN_DIST_AT: u32 = 0x0866_1030;
/// Weights go with the inverse of the distance to this power (the double at 0x08661038).
pub const POWER: f64 = 4.0;
pub const POWER_AT: u32 = 0x0866_1038;
/// The weight of a point given to its one nearest bone (the float at 0x08661040).
pub const WHOLE_F: f32 = 100.0;
pub const WHOLE_F_AT: u32 = 0x0866_1040;
/// The sum the shared weights of a point are made to have (the double at 0x08661048).
pub const SHARED_SUM: f64 = 100.0;
pub const SHARED_SUM_AT: u32 = 0x0866_1048;
/// A bone that takes more than this share of a point gives it its colour (the double at
/// 0x08661050).
pub const COLOUR_ABOVE: f64 = 50.0;
pub const COLOUR_ABOVE_AT: u32 = 0x0866_1050;

/// The list of a skin in a bone's chain of lists, or null; the bone's record is read
/// without a test for null.
fn skin_list(m: &Mem, bone: Node, skin: Node) -> Ptr32<EnvList> {
    let e: Ptr32<EnvInfo> = m[bone].env_skel_jnt.cast();
    let mut l = m[e].lists;
    while !l.is_null() && m[l].owner != skin {
        l = m[l].next;
    }
    l
}

/// The local point scaled by the volume's scaling, each product kept in the register.
fn scaled(p: V3, s: si_types::geom::Vector) -> [F80; 3] {
    [x(p[0]) * s.x, x(p[1]) * s.y, x(p[2]) * s.z]
}

/// float(0.5 × s), as the original stores it before it compares.
fn half_f(s: f32) -> f32 {
    (x(BOUND_HALF) * s).to_f32()
}

/// Algorithm 18: the box of half extents 0.5 × scaling (not rounded) about the origin.
fn in_box(p: V3, s: si_types::geom::Vector) -> bool {
    let q = scaled(p, s);
    q[0].abs().le(x(BOUND_HALF) * s.x) && (x(BOUND_HALF) * s.y).ge(q[1].abs()) && (x(BOUND_HALF) * s.z).ge(q[2].abs())
}

/// Algorithm 19: a round cylinder along x: |x| ≤ float(0.5 sx), and the distance from the
/// axis, float(√(y² + z²)), at most float(0.5 sy).
fn in_cylinder(p: V3, s: si_types::geom::Vector) -> bool {
    let q = scaled(p, s);
    let r = (q[1] * q[1] + q[2] * q[2]).sqrt().to_f32();
    let (a, b) = (half_f(s.x), half_f(s.y));
    q[0].abs().le(x(a)) && x(r).le(x(b))
}

/// Algorithm 20: a ball: float(√((x² + y²) + z²)) at most float(sx × 0.5).
fn in_ball(p: V3, s: si_types::geom::Vector) -> bool {
    let q = scaled(p, s);
    let d = ((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2]).sqrt().to_f32();
    x(d).le(x((x(s.x) * BOUND_HALF).to_f32()))
}

/// Algorithms 21, 23, 25: an elliptic cylinder along x: |x| ≤ a and y²/b² + z²/c² ≤ 1, with
/// a, b, c = float(0.5 × scaling) and the quotients and their sum in the register.
fn in_elliptic_cylinder(p: V3, s: si_types::geom::Vector) -> bool {
    let q = scaled(p, s);
    let (a, b, c) = (half_f(s.x), half_f(s.y), half_f(s.z));
    let sum = (q[1] * q[1]) / (x(b) * x(b)) + (q[2] * q[2]) / (x(c) * x(c));
    q[0].abs().le(x(a)) && sum.le(F80::ONE)
}

/// Algorithms 22, 24, 26: an ellipsoid: (x²/a² + y²/b²) + z²/c² ≤ 1.
fn in_ellipsoid(p: V3, s: si_types::geom::Vector) -> bool {
    let q = scaled(p, s);
    let (a, b, c) = (half_f(s.x), half_f(s.y), half_f(s.z));
    let t = ((q[0] * q[0]) / (x(a) * x(a)) + (q[1] * q[1]) / (x(b) * x(b))) + (q[2] * q[2]) / (x(c) * x(c));
    t.le(F80::ONE)
}

/// What a mesh volume says about the point (algorithms 27, 28, 29), into `flag`.
///
/// The box the volume meshes' points span is grown by this mesh's points (`lo`, `hi` are
/// kept for the whole call, from ±1e8: a later mesh's box includes the earlier meshes'
/// points). A point outside the box leaves the flag alone. Inside it, each polygon's
/// centroid (`SI_meshPolygonCentroid`) is taken: when the unit vector from the point to
/// the centroid makes a negative dot product with the polygon's normal (the first three
/// floats of the polygon), the point is beyond that face, `beyond` is set and the search
/// stops. The flag becomes itself or 1 unless `beyond` is set. `beyond` is never cleared
/// in a call: once one mesh finds the point beyond a face, every later mesh skips its faces
/// and leaves its flag as it was.
#[allow(clippy::too_many_arguments)]
fn mesh_volume(m: &mut Mem, o: &mut dyn Outside, def: u32, p: V3, lo: &mut V3, hi: &mut V3, beyond: &mut bool, flag: &mut i16, centroid: u32) {
    let verts = m.u32(def);
    let mut v = 0i32;
    while v < m.i32(def.wrapping_add(0x14)) {
        let at = verts.wrapping_add((v as u32).wrapping_mul(0x14));
        for k in 0..3 {
            let c = m.f32(at.wrapping_add(4 * k as u32));
            if x(c).lt(x(lo[k])) {
                lo[k] = fmov(c);
            }
            if x(c).gt(x(hi[k])) {
                hi[k] = fmov(c);
            }
        }
        v += 1;
    }
    for k in 0..3 {
        if !(x(p[k]).ge(x(lo[k])) && x(p[k]).le(x(hi[k]))) {
            return;
        }
    }
    let old = *flag;
    let count = m.i32(def.wrapping_add(0x18));
    if count > 0 && !*beyond {
        let mut q = 0i32;
        loop {
            o.call(m, &SI_MESH_POLYGON_CENTROID, &[def, q as u32, centroid]);
            let c = rd3(m, centroid);
            let mut d = Vector { x: (x(c[0]) - p[0]).to_f32(), y: (x(c[1]) - p[1]).to_f32(), z: (x(c[2]) - p[2]).to_f32() };
            vector_normalize(&mut d);
            let n = rd3(m, m.u32(def.wrapping_add(4)).wrapping_add((q as u32).wrapping_mul(0x18)));
            let dot = (x(d.x) * n[0] + x(d.y) * n[1]) + x(d.z) * n[2];
            if dot.lt(F80::ZERO) {
                *beyond = true;
            }
            q += 1;
            if q >= m.i32(def.wrapping_add(0x18)) || *beyond {
                break;
            }
        }
    }
    *flag = if *beyond { old } else { old | 1 };
}

/// Whether a point lies in the volumes a bone's sons describe [soft 0x084b7b68 vrtInJntBound].
///
/// Every son whose algorithm is 18 to 29 is a volume; the others are passed over. The point
/// is taken into the son's frame (`SI_matrixInverse` of the global matrix of the node the
/// son's +0x44 names; when there is no inverse the local keeps what it held: the previous
/// son's inverse, or for the first what the stack held) and tested against the volume
/// ([`in_box`], [`in_cylinder`], [`in_ball`], [`in_elliptic_cylinder`], [`in_ellipsoid`],
/// [`mesh_volume`]). The volumes fall in three groups, by what being inside them does:
///
/// - *keeping* volumes (18, 19, 20, 23, 24, 28): when a bone has any, a point inside none of
///   them is refused;
/// - *refusing* volumes (21, 22, 27): a point inside one is refused;
/// - *taking* volumes (25, 26, 29): a point inside one is the bone's.
///
/// `*out` is 1 when the point is refused (0 for a bone without sons); the answer is 1 when
/// it is inside a taking volume and not refused.
pub fn vrt_in_jnt_bound(m: &mut Mem, o: &mut dyn Outside, point: V3, bone: Node, out: Ptr32<i16>) -> i16 {
    let (mut keep, mut refuse, mut take) = (0i16, 0i16, 0i16);
    let mut keeping = false;
    let mut beyond = false;
    m[out] = 0;
    let mut lo = [BIG; 3];
    let mut hi = [MINUS_BIG; 3];
    let mut s = m[bone].son;
    if s.is_null() {
        return 0;
    }
    // SI_matrixInverse's destination: one local for the whole walk
    let inv: Ptr32<Matrix> = Ptr32::new(m.local(0x40));
    let centroid = m.local(0xc);
    while !s.is_null() {
        let k = (m[s].algo_type as u32).wrapping_sub(18);
        if k <= 11 {
            let latdep = m[s].latdep;
            let g = m[latdep].gmat;
            if let Some(i) = matrix_inverse(&g) {
                m.write(inv, i);
            }
            let i = m.read(inv);
            let (a, b, c) = (x(point[0]), x(point[1]), x(point[2]));
            let p = [
                (((b * i[1][0] + a * i[0][0]) + c * i[2][0]) + i[3][0]).to_f32(),
                (((b * i[1][1] + a * i[0][1]) + c * i[2][1]) + i[3][1]).to_f32(),
                ((c * i[2][2] + (b * i[1][2] + a * i[0][2])) + i[3][2]).to_f32(),
            ];
            let sc = m[s].scal;
            let def = m[s].definition.addr();
            match k {
                0 | 1 | 2 | 5 | 6 => {
                    keeping = true;
                    let inside = match k {
                        0 => in_box(p, sc),
                        1 => in_cylinder(p, sc),
                        2 => in_ball(p, sc),
                        5 => in_elliptic_cylinder(p, sc),
                        _ => in_ellipsoid(p, sc),
                    };
                    if inside {
                        keep = 1;
                    }
                }
                3 | 4 => {
                    if if k == 3 { in_elliptic_cylinder(p, sc) } else { in_ellipsoid(p, sc) } {
                        refuse = 1;
                    }
                }
                7 | 8 => {
                    if if k == 7 { in_elliptic_cylinder(p, sc) } else { in_ellipsoid(p, sc) } {
                        take = 1;
                    }
                }
                9 => mesh_volume(m, o, def, p, &mut lo, &mut hi, &mut beyond, &mut refuse, centroid),
                10 => {
                    keeping = true;
                    mesh_volume(m, o, def, p, &mut lo, &mut hi, &mut beyond, &mut keep, centroid);
                }
                _ => mesh_volume(m, o, def, p, &mut lo, &mut hi, &mut beyond, &mut take, centroid),
            }
        }
        s = m[s].brother;
    }
    m.release(0xc);
    m.release(0x40);
    let refused = (refuse != 0 || (keeping && keep == 0)) as i16;
    m[out] = refused;
    if take != 0 {
        refused ^ 1
    } else {
        0
    }
}

/// The display colour a bone gives the point being assigned: the 16-bit word at +0x12 of
/// the bone's display node in the scene's graph list (`SI_findGlData`), masked with
/// `SI_select7WM`.
fn bone_colour(m: &mut Mem, o: &mut dyn Outside, bone: Node, flag: Ptr32<u16>) {
    let proc = m[SI_PROC_DATA];
    let list = m[proc].list as u32;
    let gl = m[SI_GRAF_LISTS].addr().wrapping_add(list.wrapping_mul(0x24));
    let g = o.call(m, &SI_FIND_GL_DATA, &[gl, bone.addr(), 0]);
    let c = m.u16(g.wrapping_add(0x12));
    m[flag] = c & m[SI_SELECT7_WM];
}

/// A vertex entry for the point `index` at the head of a bone's list.
fn push_vert(m: &mut Mem, l: Ptr32<EnvList>, v: Ptr32<EnvVert>) {
    let head = m[l].verts;
    if !head.is_null() {
        m[v].next = head;
    }
    m[l].verts = v;
}

/// Assigns one point of a skin to bones of the skeleton [soft 0x084b8ac8 envToJntAssign].
///
/// The bones are the elements of `skel` ([`SKEL`]) but chain roots and effectors (nulls of
/// algorithm 1 to 4) and roots of word 11. For each, [`comp_ratio`] gives the point's place
/// along the bone (from the bone's global translation to its end node's) and its distance
/// from it; a distance not above 0 becomes 0.0001.
///
/// With `mode` 2 (by volumes): a bone whose volumes take the point ([`vrt_in_jnt_bound`])
/// gets a vertex entry in its list for the skin (if it has one) with weight float(d⁴), and
/// float(1 / weight) is added to the sum; it goes on the list of taking bones, and the list
/// of nearest bones is thrown away. A bone that does not take it, while no bone has, goes
/// into the list of nearest bones (sorted by distance, a tie after the earlier) unless it
/// refuses the point or `SI_AssgnJntsDefNb` is not above 0. Afterwards, with taking bones,
/// each one's new entry gets weight float(float(100 / sum) / d⁴); otherwise the first
/// `SI_AssgnJntsDefNb` nearest bones share the point the same way (each gets an entry with
/// weight float(float(100 / sum) / d⁴), the sum taken over those bones, a bone without a
/// list for the skin counting). A bone whose share is above 50 gives `*flag` its colour
/// ([`bone_colour`]), unless `SI_edSumatraTo3x` is set.
///
/// Any other mode: the nearest bone (the first of equal distances) gets the entry, with
/// weight 100, and gives the colour. When it has no list for the skin the entry is made and
/// left unlinked. With no bone at all the original reads through null.
///
/// `*flag` starts as `SI_select7WM`. Both lists are freed at the end.
pub fn env_to_jnt_assign(m: &mut Mem, o: &mut dyn Outside, skin: Node, index: i32, point: V3, mode: i32, flag: Ptr32<u16>) {
    let mut sum: f32 = 0.0;
    let mut nearest = Node::NULL;
    // the two list heads, the distance compRatio stores and vrtInJntBound's answer
    let heads = m.local(8);
    let (taking, nearby): (Ptr32<Ptr32<JntList>>, Ptr32<Ptr32<JntList>>) = (Ptr32::new(heads), Ptr32::new(heads + 4));
    m[taking] = Ptr32::NULL;
    m[nearby] = Ptr32::NULL;
    let dist_at: Ptr32<f32> = Ptr32::new(m.local(4));
    let refused: Ptr32<i16> = Ptr32::new(m.local(4));
    m[refused] = 0;
    m[flag] = m[SI_SELECT7_WM];
    let mut v: Ptr32<EnvVert> = Ptr32::NULL;
    let mut cur = m[SKEL];
    while !cur.is_null() {
        let bone = m[cur].bone;
        let passed = (m[bone].kind == 0 && (m[bone].algo_type as u32).wrapping_sub(1) <= 3) || m[bone].envelop == 11;
        if !passed {
            let end = m[cur].end;
            let (start, stop) = (m[bone].gtrs, m[end].gtrs);
            let ratio = comp_ratio(m, point, [start.x, start.y, start.z], [stop.x, stop.y, stop.z], dist_at).to_f32();
            if x(m[dist_at]).le(F80::ZERO) {
                m[dist_at] = MIN_DIST;
            }
            let dist = m[dist_at];
            if mode == 2 {
                if vrt_in_jnt_bound(m, o, point, bone, refused) != 0 {
                    let l = skin_list(m, bone, skin);
                    if !l.is_null() {
                        v = envelop_vert_allocate(m);
                        m[v].index = index;
                        m[v].ratio = fmov(ratio);
                        m[v].dist = fmov(dist);
                        m[v].weight = trig::pow(dist as f64, POWER).to_f32();
                        push_vert(m, l, v);
                        let inv = (F80::ONE / m[v].weight).to_f32();
                        sum = (x(inv) + sum).to_f32();
                        let j = skel_jnt_allocate(m);
                        m[j].bone = bone;
                        let head = m[taking];
                        if !head.is_null() {
                            m[j].next = head;
                        }
                        m[taking] = j;
                        if !m[nearby].is_null() {
                            skel_jnt_dispose(m, nearby);
                        }
                    }
                } else if m[taking].is_null() && m[SI_ASSGN_JNTS_DEF_NB] > 0 && m[refused] == 0 {
                    let j = skel_jnt_allocate(m);
                    m[j].bone = bone;
                    m[j].ratio = fmov(ratio);
                    m[j].dist = fmov(dist);
                    let head = m[nearby];
                    if head.is_null() {
                        m[nearby] = j;
                    } else if x(dist).lt(x(m[head].dist)) {
                        m[j].next = head;
                        m[nearby] = j;
                    } else {
                        let mut c = head;
                        loop {
                            let n = m[c].next;
                            if n.is_null() {
                                m[c].next = j;
                                break;
                            }
                            if x(dist).lt(x(m[n].dist)) {
                                m[j].next = n;
                                m[c].next = j;
                                break;
                            }
                            c = n;
                        }
                    }
                }
            } else if nearest.is_null() {
                nearest = bone;
                v = envelop_vert_allocate(m);
                m[v].index = index;
                m[v].ratio = fmov(ratio);
                m[v].dist = fmov(dist);
                m[v].weight = WHOLE_F;
            } else if x(m[v].dist).gt(x(dist)) {
                nearest = bone;
                m[v].ratio = fmov(ratio);
                m[v].dist = fmov(dist);
            }
        }
        cur = m[cur].next;
    }
    if mode == 2 {
        if !m[taking].is_null() {
            let k = (x(SHARED_SUM) / sum).to_f32();
            let mut c = m[taking];
            while !c.is_null() {
                let bone = m[c].bone;
                let l = skin_list(m, bone, skin);
                if !l.is_null() {
                    let e = m[l].verts;
                    m[e].weight = (x(k) / m[e].weight).to_f32();
                    if m[SI_ED_SUMATRA_TO_3X] == 0 && x(m[e].weight).gt(x(COLOUR_ABOVE)) {
                        bone_colour(m, o, bone, flag);
                    }
                }
                c = m[c].next;
            }
        } else if !m[nearby].is_null() && m[SI_ASSGN_JNTS_DEF_NB] > 0 {
            let mut i = 0;
            let mut c = m[nearby];
            loop {
                let w = trig::pow(m[c].dist as f64, POWER);
                sum = (x((F80::ONE / w).to_f32()) + sum).to_f32();
                i += 1;
                c = m[c].next;
                if i >= m[SI_ASSGN_JNTS_DEF_NB] || c.is_null() {
                    break;
                }
            }
            let k = (x(SHARED_SUM) / sum).to_f32();
            let mut i = 0;
            let mut c = m[nearby];
            while i < m[SI_ASSGN_JNTS_DEF_NB] && !c.is_null() {
                let bone = m[c].bone;
                let l = skin_list(m, bone, skin);
                if !l.is_null() {
                    let e = envelop_vert_allocate(m);
                    m[e].index = index;
                    m[e].ratio = fmov(m[c].ratio);
                    m[e].dist = fmov(m[c].dist);
                    m[e].weight = (x(k) / trig::pow(m[c].dist as f64, POWER)).to_f32();
                    push_vert(m, l, e);
                    if m[SI_ED_SUMATRA_TO_3X] == 0 && x(m[e].weight).gt(x(COLOUR_ABOVE)) {
                        bone_colour(m, o, bone, flag);
                    }
                }
                i += 1;
                c = m[c].next;
            }
        }
    } else {
        let l = skin_list(m, nearest, skin);
        if !l.is_null() {
            push_vert(m, l, v);
            bone_colour(m, o, nearest, flag);
        }
    }
    skel_jnt_dispose(m, taking);
    skel_jnt_dispose(m, nearby);
    m.release(4);
    m.release(4);
    m.release(8);
}

/// A point of a skin's geometry taken through the skin's global matrix: per coordinate
/// float(((x M0j + y M1j) + z M2j) + M3j).
fn to_world(m: &Mem, skin: Node, p: V3) -> V3 {
    let g = m[skin].gmat;
    let (a, b, c) = (x(p[0]), x(p[1]), x(p[2]));
    let col = |j: usize| (((a * g[0][j] + b * g[1][j]) + c * g[2][j]) + g[3][j]).to_f32();
    [col(0), col(1), col(2)]
}

/// A NURBS control point (x, y, z, w doubles) as a position: each coordinate divided by the
/// weight in the register and rounded to float.
fn nurbs_position(m: &Mem, cp: u32) -> V3 {
    let w = m.f64(cp.wrapping_add(0x18));
    [(x(m.f64(cp)) / w).to_f32(), (x(m.f64(cp.wrapping_add(8))) / w).to_f32(), (x(m.f64(cp.wrapping_add(0x10))) / w).to_f32()]
}

/// Assigns every point of a skin [soft 0x084b90b4 envAssign] (`mode` as for
/// [`env_to_jnt_assign`]).
///
/// A node carried rigidly (word 7) is assigned as one point, number −1, at its global
/// translation, and nothing else is done. Otherwise the skin's element is related to itself
/// by a relation of type 0xcf (`SI_elemdictAddRelation`: the envelope is switched on), and
/// each point of the skin's current stored definition (a patch's nu × nv points, a mesh's
/// vertices, a spline's keys (the key's own point), a NURBS curve's or surface's control
/// points divided by their weights) is taken through the skin's global matrix
/// ([`to_world`]), assigned, and given its colour (`SI_envelopVrtColSet`, behind the
/// trait). Other kinds of node have no points. Then the working definition is given up
/// and made anew from the stored one (`SI_envelopDispDef`, `SI_envelopCopyDef` with 1).
pub fn env_assign(m: &mut Mem, o: &mut dyn Outside, skin: Node, mode: i32) {
    if skin.is_null() {
        return;
    }
    let flag: Ptr32<u16> = Ptr32::new(m.local(4));
    if m[skin].envelop == 7 {
        let t = m[skin].gtrs;
        env_to_jnt_assign(m, o, skin, -1, [t.x, t.y, t.z], mode, flag);
        m.release(4);
        return;
    }
    let dict = cur_dict(m);
    let chapter = m[SI_CID][CID_MODELS] as u32;
    let elem = o.call(m, &SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, chapter, skin.addr()]);
    let dict = cur_dict(m);
    o.call(m, &SI_ELEMDICT_ADD_RELATION, &[dict, SI_RD.addr(), elem, elem, 0xcf, 1, 0]);
    // the stored definition, read in the five kinds that have points only
    let stored = |m: &Mem| m.u32(m[skin].defs.addr().wrapping_add(4 * m[skin].cur_def as u32));
    let one = |m: &mut Mem, o: &mut dyn Outside, i: i32, p: V3| {
        let q = to_world(m, skin, p);
        env_to_jnt_assign(m, o, skin, i, q, mode, flag);
        let f = m[flag] as u32;
        envelop_vrt_col_set(m, skin, i, f as u16);
    };
    match m[skin].kind {
        1 => {
            let def = stored(m);
            let mut i = 0i32;
            while i < m.u16(def.wrapping_add(0xc)) as i32 * m.u16(def.wrapping_add(0xe)) as i32 {
                let p = rd3(m, m.u32(def.wrapping_add(0x1a4)).wrapping_add((i as u32).wrapping_mul(0x10)));
                one(m, o, i, p);
                i += 1;
            }
        }
        4 => {
            let def = stored(m);
            let mut i = 0i32;
            while i < m.i32(def.wrapping_add(0x14)) {
                let p = rd3(m, m.u32(def).wrapping_add((i as u32).wrapping_mul(0x14)));
                one(m, o, i, p);
                i += 1;
            }
        }
        6 => {
            let def = stored(m);
            let mut i = 0i32;
            while i < m.u16(def.wrapping_add(0x10)) as i32 {
                let p = rd3(m, m.u32(def.wrapping_add(0x14)).wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0xc));
                one(m, o, i, p);
                i += 1;
            }
        }
        9 => {
            let def = stored(m);
            let mut i = 0i32;
            while i < m.i32(def.wrapping_add(4)) {
                let p = nurbs_position(m, m.u32(def.wrapping_add(0x2c)).wrapping_add((i as u32).wrapping_mul(0x24)));
                one(m, o, i, p);
                i += 1;
            }
        }
        10 => {
            let def = stored(m);
            let mut i = 0i32;
            while i < m.i32(def.wrapping_add(0x48)) {
                let p = nurbs_position(m, m.u32(def.wrapping_add(0x54)).wrapping_add((i as u32).wrapping_mul(0x24)));
                one(m, o, i, p);
                i += 1;
            }
        }
        _ => {}
    }
    envelop_disp_def(m, o, skin);
    envelop_copy_def(m, o, skin, 1);
    m.release(4);
}

// ------------------------------------------------------------ second pass: making skeletons and skins

/// The address of the scene's graph list: `SI_grafLists` + `SI_procData`->list × 0x24.
fn graf_list(m: &Mem) -> u32 {
    let proc = m[SI_PROC_DATA];
    let list = m[proc].list as u32;
    m[SI_GRAF_LISTS].addr().wrapping_add(list.wrapping_mul(0x24))
}

/// Puts a bone's colour on its display node (the word at +0x12 keeps its
/// `SI_unselectWM` bits and takes `colour`) and answers the next bone's colour:
/// `SI_select3WM` after `SI_select6WM`, else `colour` + `SI_selectStep`.
fn paint(m: &mut Mem, o: &mut dyn Outside, node: Node, colour: u16) -> u16 {
    let gl = graf_list(m);
    let g = o.call(m, &SI_FIND_GL_DATA, &[gl, node.addr(), 0]);
    let c = m.u16(g.wrapping_add(0x12));
    m.set_u16(g.wrapping_add(0x12), (c & m[SI_UNSELECT_WM]) | colour);
    if colour == m[SI_SELECT6_WM] {
        m[SI_SELECT3_WM]
    } else {
        colour.wrapping_add(m[SI_SELECT_STEP])
    }
}

/// Notes a node's present local transformation as its rest one: the record at +0x328
/// (`SI_calloc(1, 0x24)` when there is none) takes the local scaling, the rotation angles
/// and the translation, word for word.
fn note_rest(m: &mut Mem, node: Node) {
    if m[node].init_srt.is_null() {
        let a = m.si_calloc(1, 0x24);
        m[node].init_srt = Ptr32::new(a);
    }
    for (to, from) in [(0u32, 0x100u32), (0xc, 0x10c), (0x18, 0x158)] {
        let srt = m[node].init_srt.addr();
        for k in 0..3u32 {
            let v = m.u32(node.addr().wrapping_add(from + 4 * k));
            m.set_u32(srt.wrapping_add(to + 4 * k), v);
        }
    }
}

/// Whether a node is one of the volumes of [`vrt_in_jnt_bound`] (algorithm 18 to 29).
fn is_volume(m: &Mem, node: Node) -> bool {
    (m[node].algo_type as u32).wrapping_sub(18) <= 11
}

/// Puts a bone in front of `skel` (with the node it runs to: its first son for a joint,
/// kind 5, else itself) and notes the rest transformation of its volume sons.
fn enlist_bone(m: &mut Mem, node: Node) {
    let j = skel_jnt_allocate(m);
    m[j].bone = node;
    m[j].end = if m[node].kind == 5 { m[node].son } else { node };
    let head = m[SKEL];
    if !head.is_null() {
        m[j].next = head;
    }
    let mut s = m[node].son;
    m[SKEL] = j;
    while !s.is_null() {
        if is_volume(m, s) {
            note_rest(m, s);
        }
        s = m[s].brother;
    }
}

/// The lists of the skins of a branch [soft 0x084b9738 envelopListBuild]: every node of the
/// branch (and its brothers) whose word is 0 or `word`, that is not the main node of a
/// global envelope, and that can be a skin of that word (for 7, any node of algorithm 0; for
/// 5 and 6, a node of algorithm 0 of kind 0, 1, 4, 6, 9 or 10) takes the word and a list in
/// front of `*head`; for 7 it also gets a rest transformation filled by
/// `SI_modelGlobalState`. The sons of a node are visited only when it was taken.
pub fn envelop_list_build(m: &mut Mem, o: &mut dyn Outside, node: Node, word: i32, head: Ptr32<Ptr32<EnvList>>) {
    let w = m[node].envelop;
    let mut taken = false;
    if (w == 0 || w == word) && crate::envtools::main_glob_env(m, node) as i16 == 0 {
        let algo = m[node].algo_type;
        taken = if word == 7 && algo == 0 {
            true
        } else {
            (word as u32).wrapping_sub(5) <= 1 && algo == 0 && matches!(m[node].kind, 0 | 1 | 4 | 6 | 9 | 10)
        };
    }
    if taken {
        m[node].envelop = word;
        if word == 7 {
            let srt = m.si_calloc(1, 0x24);
            m[node].init_srt = Ptr32::new(srt);
            o.call(m, &SI_MODEL_GLOBAL_STATE, &[node.addr(), srt, srt.wrapping_add(0xc), srt.wrapping_add(0x18)]);
        }
        let l = crate::envtools::envelop_list_allocate(m, node);
        let h = m[head];
        if !h.is_null() {
            m[l].next = h;
            m[h].prev = l;
        }
        m[l].prev = Ptr32::NULL;
        m[head] = l;
        let son = m[node].son;
        if !son.is_null() {
            envelop_list_build(m, o, son, word, head);
        }
    }
    let b = m[node].brother;
    if !b.is_null() {
        envelop_list_build(m, o, b, word, head);
    }
}

/// The list `skel` of the bones of a skeleton [soft 0x084b9848 envelopSkelBuild]: every node
/// of the branch whose record has the level expected (the walk follows first sons one level
/// down, and brothers when `brothers` is set), but, for a global skin (word 6 or 7), roots
/// of word 11 and chain roots whose first joint has word 4. Each goes in front of the list
/// ([`enlist_bone`]).
pub fn envelop_skel_build(m: &mut Mem, skin: Node, node: Node, level: i32, brothers: i16) {
    'mine: {
        if crate::envtools::envelop_level(m, node) != level {
            break 'mine;
        }
        if (m[skin].envelop as u32).wrapping_sub(6) <= 1 {
            if m[node].envelop == 11 {
                break 'mine;
            }
            if crate::envtools::is_chain_root(m, node) {
                let son = m[node].son;
                if m[son].envelop == 4 {
                    break 'mine;
                }
            }
        }
        enlist_bone(m, node);
        let son = m[node].son;
        if !son.is_null() {
            envelop_skel_build(m, skin, son, level.wrapping_add(1), 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_skel_build(m, skin, b, level, 1);
    }
}

/// Makes a branch a skeleton [soft 0x084b99d8 SI_envelopSkelSet]: every node whose word is 0
/// or 1 and that is no volume gets the word 9, a new record, its rest transformation
/// ([`note_rest`]) and, but chain roots and effectors, the next colour ([`paint`]); then its
/// sons. Brothers are visited when `brothers` is set. A null node is read through, as the
/// original does.
pub fn envelop_skel_set(m: &mut Mem, o: &mut dyn Outside, node: Node, colour: u16, brothers: i16) {
    let mut colour = colour;
    if !node.is_null() && (m[node].envelop as u32) <= 1 && !is_volume(m, node) {
        m[node].envelop = 9;
        let e = crate::envtools::envelop_info_allocate(m);
        m[node].env_skel_jnt = e.cast();
        if !(m[node].kind == 0 && (m[node].algo_type as u32).wrapping_sub(1) <= 3) {
            colour = paint(m, o, node, colour);
        }
        note_rest(m, node);
        let son = m[node].son;
        if !son.is_null() {
            envelop_skel_set(m, o, son, colour, 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_skel_set(m, o, b, colour, 1);
    }
}

/// Takes the skeleton off a branch [soft 0x084b9b50 SI_envelopSkelReset]: a node of word 9
/// whose record (if any) has no lists gets the word 0, the colour `SI_select7WM` (over its
/// `SI_unselectWM` bits), loses its rest transformation (`free`) and its record
/// (`SI_envelopInfoDispose`); then its sons. Brothers are visited when `brothers` is set.
pub fn envelop_skel_reset(m: &mut Mem, o: &mut dyn Outside, node: Node, brothers: i16) {
    if m[node].envelop == 9 && {
        let e = crate::envtools::info(m, node);
        e.is_null() || m[e].lists.is_null()
    } {
        m[node].envelop = 0;
        let gl = graf_list(m);
        let g = o.call(m, &SI_FIND_GL_DATA, &[gl, node.addr(), 0]);
        let c = m.u16(g.wrapping_add(0x12));
        m.set_u16(g.wrapping_add(0x12), (c & m[SI_UNSELECT_WM]) | m[SI_SELECT7_WM]);
        let srt = m[node].init_srt;
        if !srt.is_null() {
            m.free(srt.addr());
            m[node].init_srt = Ptr32::NULL;
        }
        if !m[node].env_skel_jnt.is_null() {
            crate::envtools::envelop_info_dispose(m, si_mem::field!(node, Model, env_skel_jnt).cast());
        }
        let son = m[node].son;
        if !son.is_null() {
            envelop_skel_reset(m, o, son, 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_skel_reset(m, o, b, 1);
    }
}

/// Gives the bones of a branch their records and lists for the skins of `lists`
/// [soft 0x084b9c3c envelopAssignSkelSet], down to the level before `limit`.
///
/// A node is taken when its word is 0, 1, 4, 9, 10 or 11, it is no volume, and its record,
/// if it has one of a level not below 0, has the level expected. A node without such a
/// record gets its word: below the top, 9 for a word 0 or 1; at the top of a local skeleton
/// (`local` set) a chain root gets 9 and its first joint 4, any other node 11; at the top of
/// a global one a chain root gets 4, any other node 10. A node without record gets one (and,
/// but chain roots and effectors, the next colour); a record of level below 0 takes the
/// level. A record without lists gets one list per element of `lists` (for the same skin),
/// in order; one with lists has every list's vertex entries freed. Then the rest
/// transformation is noted, the bone is put in `skel` ([`enlist_bone`]), and the sons are
/// visited one level down; brothers when `brothers` is set.
#[allow(clippy::too_many_arguments)]
pub fn envelop_assign_skel_set(m: &mut Mem, o: &mut dyn Outside, node: Node, lists: Ptr32<EnvList>, level: i32, limit: i32, colour: u16, local: i16, brothers: i16) {
    use crate::envtools::{envelop_info_allocate, envelop_list_allocate, envelop_vert_list_dispose, info};
    let mut colour = colour;
    let mut last: Ptr32<EnvList> = Ptr32::NULL;
    if level >= limit {
        return;
    }
    'mine: {
        let w = m[node].envelop;
        if !((w as u32) <= 1 || matches!(w, 9 | 4 | 10 | 11)) {
            break 'mine;
        }
        let algo = m[node].algo_type;
        if (algo as u32).wrapping_sub(18) <= 11 {
            break 'mine;
        }
        let old = info(m, node);
        let mut words = true;
        if !old.is_null() && m[old].level >= 0 {
            if m[old].level != level {
                break 'mine;
            }
            words = false;
        }
        if words {
            let chain_root = m[node].kind == 0 && (algo as u32).wrapping_sub(1) <= 1;
            if level != 0 {
                if (w as u32) <= 1 {
                    m[node].envelop = 9;
                }
            } else if local != 0 {
                if chain_root {
                    let son = m[node].son;
                    m[son].envelop = 4;
                    m[node].envelop = 9;
                } else {
                    m[node].envelop = 11;
                }
            } else if chain_root {
                m[node].envelop = 4;
            } else {
                m[node].envelop = 10;
            }
        }
        if old.is_null() {
            let e = envelop_info_allocate(m);
            m[node].env_skel_jnt = e.cast();
            if !(m[node].kind == 0 && (m[node].algo_type as u32).wrapping_sub(1) <= 3) {
                colour = paint(m, o, node, colour);
            }
        }
        let e = info(m, node);
        if m[e].level < 0 {
            m[e].level = level;
        }
        let e = info(m, node);
        let mine = m[e].lists;
        if mine.is_null() {
            let mut t = lists;
            while !t.is_null() {
                let owner = m[t].owner;
                let nl = envelop_list_allocate(m, owner);
                let e = info(m, node);
                if m[e].lists.is_null() {
                    m[e].lists = nl;
                } else {
                    m[last].next = nl;
                    m[nl].prev = last;
                }
                last = nl;
                t = m[t].next;
            }
        } else {
            let mut l = mine;
            while !l.is_null() {
                envelop_vert_list_dispose(m, si_mem::field!(l, EnvList, verts));
                l = m[l].next;
            }
        }
        note_rest(m, node);
        enlist_bone(m, node);
        let son = m[node].son;
        if !son.is_null() {
            envelop_assign_skel_set(m, o, son, lists, level.wrapping_add(1), limit, colour, local, 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_assign_skel_set(m, o, b, lists, level, limit, colour, local, 1);
    }
}

/// Makes a node a skin of a skeleton and assigns its points [soft 0x084b9ff0 SI_envelopSkelAssign].
///
/// Nothing unless both are given and the skin's word is 5 to 8. A skin of word 7 gets a
/// rest transformation from `SI_modelGlobalState`; any other a list, the first of the list
/// of skins. Its sons become skins of the same word ([`envelop_list_build`]). The skeleton
/// gets its records ([`envelop_assign_skel_set`] from level 0 to `limit`, colours from
/// `SI_select3WM`): for a global skin (6, 7) the root and each of its brothers that is no
/// root of word 11 and no chain root whose first joint has word 4, as a global skeleton; for
/// a local skin the root alone, as a local one. Then every skin of the list is assigned
/// ([`env_assign`] with `mode`), and `skel` is freed. The list of skins is not.
pub fn envelop_skel_assign(m: &mut Mem, o: &mut dyn Outside, skin: Node, root: Node, limit: i32, mode: i32) {
    let head: Ptr32<Ptr32<EnvList>> = Ptr32::new(m.local(4));
    m[head] = Ptr32::NULL;
    if skin.is_null() || root.is_null() || (m[skin].envelop as u32).wrapping_sub(5) > 3 {
        m.release(4);
        return;
    }
    if m[skin].envelop == 7 {
        let srt = m.si_calloc(1, 0x24);
        m[skin].init_srt = Ptr32::new(srt);
        o.call(m, &SI_MODEL_GLOBAL_STATE, &[skin.addr(), srt, srt.wrapping_add(0xc), srt.wrapping_add(0x18)]);
    } else {
        let l = crate::envtools::envelop_list_allocate(m, skin);
        let h = m[head];
        if !h.is_null() {
            m[l].next = h;
            m[h].prev = l;
        }
        m[l].prev = Ptr32::NULL;
        m[head] = l;
    }
    let son = m[skin].son;
    if !son.is_null() {
        let w = m[skin].envelop;
        envelop_list_build(m, o, son, w, head);
    }
    if (m[skin].envelop as u32).wrapping_sub(6) <= 1 {
        let mut b = root;
        while !b.is_null() {
            let mut passed = false;
            if (m[skin].envelop as u32).wrapping_sub(6) <= 1 {
                if m[b].envelop == 11 {
                    passed = true;
                } else if crate::envtools::is_chain_root(m, b) {
                    let s = m[b].son;
                    passed = m[s].envelop == 4;
                }
            }
            if !passed {
                let (l, c) = (m[head], m[SI_SELECT3_WM]);
                envelop_assign_skel_set(m, o, b, l, 0, limit, c, 0, 0);
            }
            b = m[b].brother;
        }
    } else {
        let (l, c) = (m[head], m[SI_SELECT3_WM]);
        envelop_assign_skel_set(m, o, root, l, 0, limit, c, 1, 0);
    }
    let mut l = m[head];
    while !l.is_null() {
        let owner = m[l].owner;
        if !owner.is_null() {
            env_assign(m, o, owner, mode);
        }
        l = m[l].next;
    }
    skel_jnt_dispose(m, SKEL);
    m.release(4);
}

/// Assigns the points of a skin anew [soft 0x084ba188 SI_envelopReassign].
///
/// The method is asked of the user (`SI_dialogEnvelopInit(0, 0)`; 0, nothing is done)
/// unless a scene is being converted (`SI_edSumatraTo3x`), when it is 2 (by volumes).
/// Nothing either without a skeleton root. The bones go into `skel`
/// ([`envelop_skel_build`] from the root at level 0, with brothers for a global skin), the
/// skeleton is put at rest ([`crate::envtools::envelop_tmp_reset`] with 1). Then:
///
/// - `hier` set: every skin in the root's lists (the root's record read without a test)
///   whose display node (`SI_findGlData`) exists and has its word at +0x08 set: each bone of
///   `skel` loses its vertex entries for that skin, and the skin is assigned ([`env_assign`]);
/// - else the same for `skin` alone, without asking about its display node.
///
/// The skeleton is brought back ([`crate::envtools::envelop_tmp_reset`] with 2), the skins
/// deformed (`SI_envelopHierUpdate` of the root, or `SI_envelopCompUpdate` of the skin) and
/// `skel` freed.
pub fn envelop_reassign(m: &mut Mem, o: &mut dyn Outside, skin: Node, hier: i16) {
    use crate::env_outside::SI_DIALOG_ENVELOP_INIT;
    use crate::envtools::{envelop_skel_root, envelop_tmp_reset, envelop_vert_list_dispose, info};
    let mut mode: i32 = 2;
    if m[SI_ED_SUMATRA_TO_3X] == 0 {
        mode = o.call(m, &SI_DIALOG_ENVELOP_INIT, &[0, 0]) as i32;
        if mode == 0 {
            return;
        }
    }
    let root = envelop_skel_root(m, skin);
    if root.is_null() {
        return;
    }
    let global = ((m[skin].envelop as u32).wrapping_sub(6) <= 1) as i16;
    envelop_skel_build(m, skin, root, 0, global);
    envelop_tmp_reset(m, o, skin, 1);
    // every bone of `skel` gives up its vertex entries for one skin
    let forget = |m: &mut Mem, owner: Node| {
        let mut s = m[SKEL];
        while !s.is_null() {
            let bone = m[s].bone;
            let l = skin_list(m, bone, owner);
            if !l.is_null() {
                envelop_vert_list_dispose(m, si_mem::field!(l, EnvList, verts));
            }
            s = m[s].next;
        }
    };
    if hier != 0 {
        let e = info(m, root);
        let mut l = m[e].lists;
        while !l.is_null() {
            let owner = m[l].owner;
            if !owner.is_null() {
                let gl = graf_list(m);
                let g = o.call(m, &SI_FIND_GL_DATA, &[gl, owner.addr(), 0]);
                if g != 0 && m.u32(g.wrapping_add(8)) != 0 {
                    forget(m, owner);
                    let owner = m[l].owner;
                    env_assign(m, o, owner, mode);
                }
            }
            l = m[l].next;
        }
    } else {
        forget(m, skin);
        env_assign(m, o, skin, mode);
    }
    envelop_tmp_reset(m, o, skin, 2);
    if hier != 0 {
        crate::envelop::envelop_hier_update(m, o, root);
    } else {
        crate::envelop::envelop_comp_update(m, o, skin);
    }
    skel_jnt_dispose(m, SKEL);
}

// ------------------------------------------------------------ second pass: editing weights

/// A distance along a bone's reach below this is raised to [`REACH_FLOOR_F`] (the double at
/// 0x08661088).
pub const REACH_FLOOR: f64 = 0.0001;
pub const REACH_FLOOR_AT: u32 = 0x0866_1088;
/// (The float at 0x08661090.)
pub const REACH_FLOOR_F: f32 = 0.0001;
pub const REACH_FLOOR_F_AT: u32 = 0x0866_1090;
/// A bone given more than this weight on a point gives it its colour (the double at
/// 0x08661098, a second copy of [`COLOUR_ABOVE`]).
pub const COLOUR_ABOVE_W: f64 = 50.0;
pub const COLOUR_ABOVE_W_AT: u32 = 0x0866_1098;

/// The global position of a point of a skin's first stored definition
/// (`SI_modelGetVertexPtr`, behind the trait, then [`to_world`]); the word the routine
/// stores through its fourth argument is not read.
fn vertex_world(m: &mut Mem, o: &mut dyn Outside, skin: Node, index: i32) -> V3 {
    use crate::env_outside::SI_MODEL_GET_VERTEX_PTR;
    let out = m.local(4);
    let def0 = m.u32(m[skin].defs.addr());
    let kind = m[skin].kind as u32;
    let p = o.call(m, &SI_MODEL_GET_VERTEX_PTR, &[def0, kind, index as u32, out]);
    let q = to_world(m, skin, rd3(m, p));
    m.release(4);
    q
}

/// The far end of a bone, as the weight routines take it: a joint's (kind 5) first son, any
/// other bone itself.
fn bone_end(m: &Mem, bone: Node) -> Node {
    if m[bone].kind == 5 {
        m[bone].son
    } else {
        bone
    }
}

fn gtrs(m: &Mem, node: Node) -> V3 {
    let t = m[node].gtrs;
    [t.x, t.y, t.z]
}

/// A vertex entry for a point a bone had none for [soft 0x084ba59c SI_envelopWeightCreateReference]:
/// its place along the bone and distance from it ([`comp_ratio`] from the bone's global
/// translation to its end's, [`bone_end`]), the weight at `amount`, at the head of the
/// list `l`; the weight is added to `totwght`.
pub fn envelop_weight_create_reference(m: &mut Mem, bone: Node, index: i32, amount: Ptr32<f32>, point: V3, l: Ptr32<EnvList>) {
    let end = bone_end(m, bone);
    let dist: Ptr32<f32> = Ptr32::new(m.local(4));
    let (s, e) = (gtrs(m, bone), gtrs(m, end));
    let ratio = comp_ratio(m, point, s, e, dist).to_f32();
    let v = envelop_vert_allocate(m);
    m[v].index = index;
    m[v].ratio = fmov(ratio);
    m[v].dist = fmov(m[dist]);
    m[v].weight = fmov(m[amount]);
    let head = m[l].verts;
    if !head.is_null() {
        m[v].next = head;
    }
    m[l].verts = v;
    m[TOTWGHT] = (x(m[TOTWGHT]) + m[amount]).to_f32();
    m.release(4);
}

/// One point's weight on bone `target`, over the bones of a branch
/// [soft 0x084ba3e8 vertWeightUpdate].
///
/// A bone whose record has the level expected (but, for a global skin, a root of word 11 and
/// a chain root whose first joint has word 4, which are passed over with their sons) and a
/// list for the skin: the first vertex entry of the point `index` (for a rigid node, word 7,
/// the first entry whatever its number) goes through [`envelop_modify_weight`]; when
/// `target` itself has no entry for the point and `mode` is 0 or 1, it gets one
/// ([`envelop_weight_create_reference`]). Then the sons, one level down, and the brothers
/// when `brothers` is set.
#[allow(clippy::too_many_arguments)]
pub fn vert_weight_update(m: &mut Mem, skin: Node, node: Node, target: Node, index: i32, point: V3, mode: i32, amount: Ptr32<f32>, list: Ptr32<Ptr32<VertList>>, level: i32, brothers: i16) {
    'mine: {
        if crate::envtools::envelop_level(m, node) != level {
            break 'mine;
        }
        let w = m[skin].envelop;
        if (w as u32).wrapping_sub(6) <= 1 {
            if m[node].envelop == 11 {
                break 'mine;
            }
            if crate::envtools::is_chain_root(m, node) {
                let s = m[node].son;
                if m[s].envelop == 4 {
                    break 'mine;
                }
            }
        }
        let l = skin_list(m, node, skin);
        if !l.is_null() {
            let mut v = m[l].verts;
            while !v.is_null() && m[v].index != index && w != 7 {
                v = m[v].next;
            }
            if !v.is_null() {
                envelop_modify_weight(m, v, node, target, mode, amount, list);
            }
            if node == target && v.is_null() && (mode as u32) <= 1 {
                envelop_weight_create_reference(m, target, index, amount, point, l);
            }
        }
        let son = m[node].son;
        if !son.is_null() {
            vert_weight_update(m, skin, son, target, index, point, mode, amount, list, level.wrapping_add(1), 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        vert_weight_update(m, skin, b, target, index, point, mode, amount, list, level, 1);
    }
}

/// Sets a point's weight on one bone and balances the others
/// [soft 0x084ba664 SI_envelopVertWeightUpdate]. Nothing without a skin.
///
/// The point is taken from the skin's first stored definition into global space
/// ([`vertex_world`]). With `by_dist` set the weight is scaled by where the point lies:
/// d = `reach` − the point's distance from `target` (unrounded; below 0.0001 it becomes
/// 0.0001, else it is rounded to float), weight float((d × `scale`) × `weight`). The weight
/// goes over the skeleton from `node` ([`vert_weight_update`], brothers for a global skin of
/// word 6), the others are balanced ([`envelop_vert_weight_balance`]), and the point's
/// display colour is set (`SI_envelopVrtColSet`): `SI_objectWM` when there is no `target`,
/// the target's colour ([`bone_colour`] as the assignment takes it) when its new weight is
/// above 50.
#[allow(clippy::too_many_arguments)]
pub fn envelop_vert_weight_update(m: &mut Mem, o: &mut dyn Outside, skin: Node, node: Node, target: Node, index: i32, weight: f32, mode: i32, by_dist: i16, reach: f32, scale: f32) {
    let list: Ptr32<Ptr32<VertList>> = Ptr32::new(m.local(4));
    m[list] = Ptr32::NULL;
    if skin.is_null() {
        m.release(4);
        return;
    }
    let q = vertex_world(m, o, skin, index);
    let amount: Ptr32<f32> = Ptr32::new(m.local(4));
    let a = if by_dist != 0 {
        let end = bone_end(m, target);
        let d: Ptr32<f32> = Ptr32::new(m.local(4));
        let (s, e) = (gtrs(m, target), gtrs(m, end));
        comp_ratio(m, q, s, e, d);
        let t = x(reach) - m[d];
        m[d] = t.to_f32();
        if t.lt(x(REACH_FLOOR)) {
            m[d] = REACH_FLOOR_F;
        }
        let r = ((x(m[d]) * scale) * weight).to_f32();
        m.release(4);
        r
    } else {
        fmov(weight)
    };
    m[amount] = a;
    let brothers = (m[skin].envelop == 6) as i16;
    vert_weight_update(m, skin, node, target, index, q, mode, amount, list, 0, brothers);
    let a = fmov(m[amount]);
    envelop_vert_weight_balance(m, a, list);
    if target.is_null() {
        let c = m[crate::env_globals::SI_OBJECT_WM] as u32;
        envelop_vrt_col_set(m, skin, index, c as u16);
    } else if x(m[amount]).gt(x(COLOUR_ABOVE_W)) {
        let gl = graf_list(m);
        let g = o.call(m, &SI_FIND_GL_DATA, &[gl, target.addr(), 0]);
        let c = m.u16(g.wrapping_add(0x12)) & m[SI_SELECT7_WM];
        envelop_vrt_col_set(m, skin, index, c);
    }
    m.release(4);
    m.release(4);
}

/// The same for a node carried rigidly [soft 0x084ba8b8 SI_envelopWeightUpdate]: the point is
/// the node's rest translation (+0x18 of its rest transformation), number −1, the walk from
/// `node` at level 0 with brothers. Nothing without a node or a rest transformation.
pub fn envelop_weight_update(m: &mut Mem, skin: Node, node: Node, target: Node, weight: f32, mode: i32) {
    let list: Ptr32<Ptr32<VertList>> = Ptr32::new(m.local(4));
    m[list] = Ptr32::NULL;
    let srt = if skin.is_null() { Ptr32::NULL } else { m[skin].init_srt };
    if !srt.is_null() {
        let point = rd3(m, srt.addr().wrapping_add(0x18));
        // the weight argument's own slot is what the walk changes
        let amount: Ptr32<f32> = Ptr32::new(m.local(4));
        m[amount] = weight;
        vert_weight_update(m, skin, node, target, -1, point, mode, amount, list, 0, 1);
        let a = fmov(m[amount]);
        envelop_vert_weight_balance(m, a, list);
        m.release(4);
    }
    m.release(4);
}

/// Widens a range by one point's distance from a bone
/// [soft 0x084ba930 SI_envelopVertDistMinMaxCompute]: the point of the skin's first stored
/// definition in global space ([`vertex_world`]), its distance from the bone
/// ([`comp_ratio`], to [`bone_end`]); `*max` takes it when it is greater, `*min` when it is
/// smaller (neither for a NaN).
pub fn envelop_vert_dist_min_max_compute(m: &mut Mem, o: &mut dyn Outside, skin: Node, bone: Node, index: i32, min: Ptr32<f32>, max: Ptr32<f32>) {
    let q = vertex_world(m, o, skin, index);
    let end = bone_end(m, bone);
    let d: Ptr32<f32> = Ptr32::new(m.local(4));
    let (s, e) = (gtrs(m, bone), gtrs(m, end));
    comp_ratio(m, q, s, e, d);
    let dist = m[d];
    if x(dist).gt(x(m[max])) {
        m[max] = fmov(dist);
    }
    if x(dist).lt(x(m[min])) {
        m[min] = fmov(dist);
    }
    m.release(4);
}

// ------------------------------------------------ third pass, deform2: adding parts to a skeleton

/// Whether `target` is in the branch of `node` [soft 0x084baa9c envPrecInListFind]: 1 when
/// `node` is `target`; else, for a node whose word is 5 to 8 that is not the main node of a
/// global envelope, `prec` ([`crate::env_globals::PREC`]) is set to the node and its first
/// son is searched; then its brothers; 0 when nothing matches. (`prec` keeps the last such
/// node met, whatever the answer.)
pub fn env_prec_in_list_find(m: &mut Mem, node: Node, target: Node) -> i16 {
    if node == target {
        return 1;
    }
    if (m[node].envelop as u32).wrapping_sub(5) < 4 && crate::envtools::main_glob_env(m, node) == 0 {
        m[PREC] = node;
        let son = m[node].son;
        if !son.is_null() && env_prec_in_list_find(m, son, target) != 0 {
            return 1;
        }
    }
    let brother = m[node].brother;
    if !brother.is_null() && env_prec_in_list_find(m, brother, target) != 0 {
        return 1;
    }
    0
}

/// A skin whose geometry changed is assigned anew [soft 0x084baf64 SI_envelopNewVertAssign]:
/// the message "Envelope geometry has changed, so it will be reassigned." (level 2) goes to
/// the status line (`SI_statusDisplayMessage (0x083cca94)`, behind the trait), then
/// [`envelop_reassign`] without the hierarchy.
pub fn envelop_new_vert_assign(m: &mut Mem, o: &mut dyn Outside, skin: Node) {
    o.call(m, &crate::outside::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_7480, 2]);
    envelop_reassign(m, o, skin, 0);
}

/// Strings the two dialogs of the adding routines are given (addresses of the binary's
/// data, passed through untouched): title, question, answers.
const ASK_GLOBAL: [u32; 5] = [0x086d_73a0, 0x086d_7360, 0x086d_7333, 0x086d_7330, 0x086d_732c];
const ASK_LOCAL: [u32; 5] = [0x086d_7440, 0x086d_7400, 0x086d_73ce, 0x086d_73cb, 0x086d_73c7];

/// Takes a new son of a skeleton's node into the skeleton [soft 0x084bad94
/// SI_envelopAddNewSkelParts]: `child` (word 0 or 9, and not the chain root with a first
/// joint of word 4) is asked about (`SI_dialogRequest2 (0x08141d5c)` with the texts for the
/// main node of a global envelope, or for a part of a skeleton) and, if the user agrees,
/// becomes part of the skeleton: through `envelopAssignSkelSet` with the lists of the
/// skeleton's root (global) or of `node` (one level below its own), or, for a node without a
/// level, through `SI_envelopSkelSet`. `skel` is freed at the end of every path that got
/// beyond the first two tests.
pub fn envelop_add_new_skel_parts(m: &mut Mem, o: &mut dyn Outside, node: Node, child: Node) {
    use crate::env_outside::SI_DIALOG_REQUEST2;
    use crate::envtools::{envelop_level, envelop_skel_root, info, is_chain_root, main_glob_env, skel_root_get};
    let w = m[child].envelop;
    if w != 0 && w != 9 {
        return;
    }
    if is_chain_root(m, child) && m[m[child].son].envelop == 4 {
        return;
    }
    'done: {
        let colour = m[SI_SELECT3_WM];
        if main_glob_env(m, node) as i16 != 0 {
            let ok = o.call(m, &SI_DIALOG_REQUEST2, &ASK_GLOBAL) as u16;
            if ok == 0 {
                break 'done;
            }
            let root = envelop_skel_root(m, node);
            if root.is_null() {
                break 'done;
            }
            let lists = m[info(m, root)].lists;
            envelop_assign_skel_set(m, o, child, lists, 0, 0x7fff_ffff, colour, 0, 0);
        } else {
            if node.is_null() || info(m, node).is_null() {
                break 'done;
            }
            let k = m[node].envelop;
            if k != 9 && k != 4 && k != 10 && k != 11 {
                break 'done;
            }
            let ok = o.call(m, &SI_DIALOG_REQUEST2, &ASK_LOCAL) as u16;
            if ok == 0 {
                break 'done;
            }
            let level = envelop_level(m, node);
            if level <= -1 {
                envelop_skel_set(m, o, child, colour, 0);
                break 'done;
            }
            let lists = m[info(m, node)].lists;
            let root = skel_root_get(m, node);
            if root.is_null() {
                break 'done;
            }
            let local = (m[root].envelop == 11 || m[m[root].son].envelop == 4) as i16;
            envelop_assign_skel_set(m, o, child, lists, level.wrapping_add(1), 0x7fff_ffff, colour, local, 0);
        }
    }
    skel_jnt_dispose(m, SKEL);
}

/// Takes a new son of a skin's node into the skeleton's lists [soft 0x084bab1c
/// SI_envelopAddNewParts]: unless `node`'s word is 5, 6 or 8 and `child` is no null-algorithm
/// node of kind 0, 1, 4, 6, 9 or 10, the skeleton of `node` is built ([`envelop_skel_build`])
/// and the user asked how to assign (`SI_dialogEnvelopInit`; 0 ends it). If `node`'s first
/// son leads to `child` ([`env_prec_in_list_find`]) or there is none, `child` gets a list
/// ([`envelop_list_build`]) on each bone whose lists include `node`'s (or on every bone for
/// word 7), spliced in front of that of `node` (or alone); the skeleton is brought to its
/// rest pose ([`crate::envtools::envelop_tmp_reset`] 1), the new lists' skins are assigned
/// ([`env_assign`] with the dialog's answer) and the pose is brought back (2). A skin that
/// gets no list ends the walk with `skel` freed; a refusal after the build leaves `skel`.
pub fn envelop_add_new_parts(m: &mut Mem, o: &mut dyn Outside, node: Node, child: Node) {
    let head: Ptr32<Ptr32<EnvList>> = Ptr32::new(m.local(4));
    m[head] = Ptr32::NULL;
    add_new_parts_body(m, o, node, child, head);
    m.release(4);
}

fn add_new_parts_body(m: &mut Mem, o: &mut dyn Outside, node: Node, child: Node, head: Ptr32<Ptr32<EnvList>>) {
    use crate::envtools::{envelop_skel_root, envelop_tmp_reset, info};
    let k0 = m[node].envelop;
    if ((k0 as u32).wrapping_sub(5) <= 1 || k0 == 8)
        && (m[child].algo_type != 0 || !matches!(m[child].kind, 0 | 1 | 4 | 6 | 9 | 10))
    {
        return;
    }
    let root = envelop_skel_root(m, node);
    if root.is_null() {
        return;
    }
    let mode = o.call(m, &crate::env_outside::SI_DIALOG_ENVELOP_INIT, &[0, 0]) as i32;
    if mode == 0 {
        return;
    }
    envelop_skel_build(m, node, root, 0, ((m[node].envelop as u32).wrapping_sub(6) < 2) as i16);
    m[PREC] = node;
    let son = m[node].son;
    if !son.is_null() && env_prec_in_list_find(m, son, child) == 0 {
        return;
    }
    let mut first_owner = Node::NULL;
    let mut bone = m[SKEL];
    while !bone.is_null() {
        let bone_info = info(m, m[bone].bone);
        let k = m[node].envelop;
        let mut l = m[bone_info].lists;
        if !l.is_null() {
            let prec = m[PREC];
            let mut cur = l;
            l = loop {
                let owner = m[cur].owner;
                if owner == prec {
                    break cur;
                }
                if owner == child {
                    break Ptr32::NULL;
                }
                cur = m[cur].next;
                if cur.is_null() {
                    break Ptr32::NULL;
                }
            };
        }
        if !l.is_null() || k == 7 {
            envelop_list_build(m, o, child, k, head);
            let h = m[head];
            if h.is_null() {
                skel_jnt_dispose(m, SKEL);
                return;
            }
            first_owner = m[h].owner;
            if l.is_null() {
                m[bone_info].lists = h;
            } else {
                let before = m[l].prev;
                if !before.is_null() {
                    m[before].next = h;
                    m[h].prev = before;
                } else {
                    m[bone_info].lists = h;
                }
                let mut last = h;
                while !m[last].next.is_null() {
                    last = m[last].next;
                }
                m[last].next = l;
                m[l].prev = last;
                m[head] = Ptr32::NULL;
            }
        }
        bone = m[bone].next;
    }
    envelop_tmp_reset(m, o, node, 1);
    let first = m[SKEL];
    let mut l = m[info(m, m[first].bone)].lists;
    while !l.is_null() {
        if m[l].owner == first_owner {
            while m[l].owner != child {
                let owner = m[l].owner;
                env_assign(m, o, owner, mode);
                l = m[l].next;
            }
            env_assign(m, o, child, mode);
            break;
        }
        l = m[l].next;
    }
    envelop_tmp_reset(m, o, node, 2);
    skel_jnt_dispose(m, SKEL);
}

// ------------------------------------------------ fourth pass, deform2: the colour of a point

/// The seven colours a point can be marked with, in order of their number 1 to 7 (the
/// marks are `SI_select1WM` to `SI_select7WM`).
fn mark_colours(m: &Mem) -> [u16; 7] {
    [m[SI_SELECT1_WM], m[SI_SELECT2_WM], m[SI_SELECT3_WM], m[SI_SELECT4_WM], m[SI_SELECT5_WM], m[SI_SELECT6_WM], m[SI_SELECT7_WM]]
}

/// Where the mark of a point lies, in the skin's stored definition [the shared part of
/// `SI_envelopVrtColGet` and `SI_envelopVrtColSet`]: the address of the 16-bit flag whose
/// bits 8 to 10 hold the mark (the 32-bit flag's low half for a NURBS point), or 0 for a kind
/// that has none.
fn mark_at(m: &Mem, skin: Node, index: i32) -> u32 {
    let def = m.u32(m[skin].defs.addr().wrapping_add(4 * m[skin].cur_def as u32));
    let i = index as u32;
    match m[skin].kind {
        1 => m.u32(def.wrapping_add(0x1a8)).wrapping_add(i.wrapping_mul(2)),
        4 => m.u32(def).wrapping_add(i.wrapping_mul(0x14)).wrapping_add(0x12),
        6 => m.u32(def.wrapping_add(0x14)).wrapping_add(i.wrapping_mul(0x30)).wrapping_add(0x2c),
        9 => m.u32(def.wrapping_add(0x2c)).wrapping_add(i.wrapping_mul(0x24)).wrapping_add(0x20),
        10 => m.u32(def.wrapping_add(0x54)).wrapping_add(i.wrapping_mul(0x24)).wrapping_add(0x20),
        _ => 0,
    }
}

/// The colour a point is marked with [soft 0x084bb390 SI_envelopVrtColGet]: the mark in bits
/// 8 to 10 of the point's flag (a patch point's 16-bit flag, a mesh vertex's at +0x12, a
/// spline key's at +0x2c, a NURBS control point's 32-bit flag at +0x20) names one of the
/// colours `SI_select1WM` to `SI_select7WM`; any other mark (0), a null skin or another kind
/// answers `SI_objectWM`.
pub fn envelop_vrt_col_get(m: &Mem, skin: Node, index: i32) -> u16 {
    if skin.is_null() {
        return m[SI_OBJECT_WM];
    }
    let at = mark_at(m, skin, index);
    if at == 0 {
        return m[SI_OBJECT_WM];
    }
    let v = match m[skin].kind {
        9 | 10 => m.u32(at) & 0x700,
        _ => m.u16(at) as u32 & 0x700,
    };
    if v == 0 {
        m[SI_OBJECT_WM]
    } else {
        mark_colours(m)[(v >> 8) as usize - 1]
    }
}

/// Marks a point with a colour [soft 0x084baf90 SI_envelopVrtColSet]: bits 8 to 11 of the
/// point's flag are cleared (a 16-bit and `0xf0ff` for a patch point, a mesh vertex at +0x12
/// and a spline key at +0x2c; the byte at +0x21 and `0xf0` for a NURBS curve's or surface's
/// control point), then, if `colour` (16 bits) equals `SI_select1WM` to `SI_select7WM` (the
/// first that does), the number of that mark is or-ed into the byte above the low one. Other
/// kinds do nothing.
pub fn envelop_vrt_col_set(m: &mut Mem, skin: Node, index: i32, colour: u16) {
    let kind = m[skin].kind;
    let at = mark_at(m, skin, index);
    if at == 0 {
        return;
    }
    let wide = kind == 9 || kind == 10;
    if wide {
        m.and_u8(at.wrapping_add(1), 0xf0);
    } else {
        let v = m.u16(at) & 0xf0ff;
        m.set_u16(at, v);
    }
    let marks = mark_colours(m);
    if let Some(n) = marks.iter().position(|&c| c == colour) {
        let hi = at.wrapping_add(1);
        let v = m.u8(hi) | (n as u8 + 1);
        m.set_u8(hi, v);
    }
}
