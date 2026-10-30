// SPDX-License-Identifier: MIT
//! The renderer's light records from the scene's lights: the original's `lights.c`
//! (finding 0012, sections 4 and 16). `si_lights` is ported on this crate's records; the
//! depth-map pass of a light comes in through [`LightsEnv`]. Its outer routine
//! `shadowBuffer` and the scan loop `zSample` are ported in place, on the memory image
//! ([`shadow_buffer`], [`z_sample`]).

use crate::jitter::init_jitter;
use crate::shade::ShadeState;
use crate::types::Vector;
use si_math::matrix::{invert, transpose};
use si_x87::{trig, x, F80};

/// What `si_lights` calls that is not ported with it.
pub trait LightsEnv {
    /// `shadowBuffer(light)` (0x082778fc), ported in place ([`shadow_buffer`]): renders
    /// the scene from the light into its depth map (+0x114) and sets the two matrices of
    /// `si_cameraToLight` (+0x94, +0xd4). [`crate::bound::ShadowPass`] is the environment
    /// that runs it; the default does nothing (a light then casts no depth-map shadow).
    fn shadow_buffer(&mut self, _st: &mut ShadeState, _light: usize) {}
    /// `drand48()` of the C library, for `si_initJitter`.
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

/// The text `si_lights` hands to `si_fatal` when the view matrix has no inverse.
pub fn fatal_no_inverse() -> &'static str {
    si_rt::text(0x0869_db80)
}
/// The double 10000 (0x08654600): a fall-off start more than 0.01 below it switches the
/// fall-off on. The double 0.01 (0x08654608) is also the least jitter amplitude that gets a
/// table.
const FALLOFF_OFF: f64 = 10000.0;
const EPS: f64 = 0.01;

/// The lights of the scene in the renderer's space [soft 0x082775e0 si_lights]:
/// `si_lights()`, for every light of the list at 0x087907e4:
///
/// 1. Kinds 3 and 4 (no position, a direction only): the view matrix (0x0879082c) is
///    inverted (fatal when it has no inverse: [`FATAL_NO_INVERSE`]), the inverse's
///    translation row cleared, its transpose stored as the matrix for directions
///    (0x0879086c, **left like that**); the light's position (+0x34) goes through it to
///    +0x40, and so does a local "origin" (which starts as zero and is transformed in
///    place once per such light); d = +0x40 − origin, each component rounded to float. Any
///    other kind: the position goes through the view matrix to +0x40 and d is that point.
/// 2. +0x4c = d / √((d.y² + d.x²) + d.z²), each quotient rounded once; d as it is when the
///    length compares equal to 0.
/// 3. A spot (kind 2): the interest (+0x58) through the view matrix; e = that − +0x40 kept
///    in the register; +0x64 = e / |e| (or e rounded, for a length of 0); +0x78 =
///    `cosf`(r32(0.5 × cone)), +0x7c = `cosf`(r32((cone + spread) × 0.5)), +0x80 = the second
///    cosine as the library left it in the register minus the first as stored.
/// 4. The 16-bit word at +0x13c is cleared and gets bit 0 when 10000 − the fall-off start
///    (+0x84) is above 0.01.
/// 5. Shadow kind (+0x8c) 2: bit 0 of the ray tracing word (0x08790c7c) is set. 3 or 4:
///    `shadowBuffer(light)`, then +0x120 = the soft size (+0x11c) halved towards zero and
///    +0x12c = r32(the float at +0x128 × 4).
/// 6. A jitter amplitude (+0x130) above 0.01: `si_initJitter(237, light + 0x130)`.
///
/// Returns what the transformations printed (a zero divisor each). Class: libm (`cosf`).
pub fn lights(st: &mut ShadeState, raytrace: &mut u16, level: i16, env: &mut dyn LightsEnv) -> Result<Vec<&'static str>, &'static str> {
    let mut messages = Vec::new();
    let mut origin = Vector::default();
    let mut cur = st.light_head;
    while let Some(li) = cur {
        let kind = st.lights[li].kind;
        let given = st.lights[li].pos_given;
        let mut pos = st.lights[li].pos;
        let d = if kind == 3 || kind == 4 {
            let mut inv = invert(&st.camera.raw_to_world_mat).ok_or(fatal_no_inverse())?;
            inv[3][0] = 0.0;
            inv[3][1] = 0.0;
            inv[3][2] = 0.0;
            st.camera.vect_mat = transpose(&inv);
            messages.extend(st.camera.raw_to_world_vect(given, &mut pos));
            let o = origin;
            messages.extend(st.camera.raw_to_world_vect(o, &mut origin));
            Vector::new((x(pos.x) - origin.x).to_f32(), (x(pos.y) - origin.y).to_f32(), (x(pos.z) - origin.z).to_f32())
        } else {
            messages.extend(st.camera.raw_to_world(given, &mut pos));
            pos
        };
        st.lights[li].pos = pos;
        let len = ((x(d.y) * d.y + x(d.x) * d.x) + x(d.z) * d.z).sqrt();
        st.lights[li].dir = if x(0.0f32).eq(len) { d } else { Vector::new((x(d.x) / len).to_f32(), (x(d.y) / len).to_f32(), (x(d.z) / len).to_f32()) };
        if kind == 2 {
            let mut aim = Vector::default();
            messages.extend(st.camera.raw_to_world(st.lights[li].interest, &mut aim));
            let e = [x(aim.x) - pos.x, x(aim.y) - pos.y, x(aim.z) - pos.z];
            let len = ((e[1] * e[1] + e[0] * e[0]) + e[2] * e[2]).sqrt();
            st.lights[li].spot_dir = if x(0.0f32).eq(len) {
                Vector::new(e[0].to_f32(), e[1].to_f32(), e[2].to_f32())
            } else {
                Vector::new((e[0] / len).to_f32(), (e[1] / len).to_f32(), (e[2] / len).to_f32())
            };
            let (cone, spread) = (st.lights[li].cone, st.lights[li].spread);
            let inner = trig::cosf((x(0.5f64) * cone).to_f32()).to_f32();
            let outer: F80 = trig::cosf(((x(cone) + spread) * x(0.5f64)).to_f32());
            st.lights[li].cone_cos = inner;
            st.lights[li].spread_cos = outer.to_f32();
            st.lights[li].cone_div = (outer - inner).to_f32();
        }
        st.lights[li].falloff = (x(FALLOFF_OFF) - st.lights[li].falloff_start).gt(x(EPS));
        match st.lights[li].shadow {
            2 => *raytrace |= 1,
            3 | 4 => {
                env.shadow_buffer(st, li);
                let full = st.lights[li].soft_full;
                st.lights[li].soft_size = full.wrapping_add((full as u32 >> 31) as i32) >> 1;
                st.lights[li].soft_scale = (x(st.lights[li].soft_w128) * x(4.0f64)).to_f32();
            }
            _ => {}
        }
        if x(st.lights[li].jitter.amplitude).gt(x(EPS)) {
            let mut jitter = std::mem::take(&mut st.lights[li].jitter);
            init_jitter(0xed, &mut jitter, level, &mut || env.drand48());
            st.lights[li].jitter = jitter;
        }
        cur = st.lights[li].next;
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shade::ShadeLight;

    struct Maps(Vec<usize>);
    impl LightsEnv for Maps {
        fn shadow_buffer(&mut self, _: &mut ShadeState, light: usize) {
            self.0.push(light);
        }
    }

    fn state(lights: Vec<ShadeLight>) -> ShadeState {
        let mut st = ShadeState::default();
        let ident = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        st.camera.raw_to_world_mat = ident;
        st.camera.raw_to_world_mat[3] = [0.0, 0.0, -10.0, 1.0];
        st.camera.vect_mat = ident;
        st.light_head = Some(0);
        st.lights = lights;
        st
    }

    #[test]
    fn a_point_light_and_a_direction() {
        si_rt::skip_without_install!();
        let point = ShadeLight { kind: 0, pos_given: Vector::new(3.0, 0.0, 14.0), falloff_start: 9999.0, shadow: 2, next: Some(1), ..ShadeLight::default() };
        let sun = ShadeLight { kind: 3, pos_given: Vector::new(0.0, 2.0, 0.0), falloff_start: 10000.0, shadow: 4, soft_full: -3, soft_w128: 0.5, ..ShadeLight::default() };
        let mut st = state(vec![point, sun]);
        let (mut rt, mut env) = (0u16, Maps(Vec::new()));
        assert_eq!(lights(&mut st, &mut rt, 1, &mut env), Ok(Vec::new()));
        // through the view: (3, 0, 4), the direction from the eye
        assert_eq!(st.lights[0].pos, Vector::new(3.0, 0.0, 4.0));
        assert_eq!(st.lights[0].dir, Vector::new(0.6, 0.0, 0.8));
        assert!(st.lights[0].falloff);
        assert_eq!(rt, 1);
        // a direction ignores the view's translation
        assert_eq!(st.lights[1].dir, Vector::new(0.0, 1.0, 0.0));
        assert!(!st.lights[1].falloff);
        assert_eq!(env.0, vec![1]);
        assert_eq!((st.lights[1].soft_size, st.lights[1].soft_scale), (-1, 2.0));
    }

    #[test]
    fn a_view_without_inverse_is_fatal_for_a_direction() {
        si_rt::skip_without_install!();
        let mut st = state(vec![ShadeLight { kind: 4, ..ShadeLight::default() }]);
        st.camera.raw_to_world_mat[1] = [0.0; 4];
        assert_eq!(lights(&mut st, &mut 0, 1, &mut Maps(Vec::new())), Err(fatal_no_inverse()));
    }
}


// ------------------------------------------------------------- in place, on the image

/// 0x08654618 (double) and 0x08654620 (float): the least sum of cone and spread, and what
/// a smaller one is replaced by (the float nearest 0.0001).
const ANGLE_LEAST: f64 = 0.0001;
const ANGLE_FLOOR: f32 = f32::from_bits(0x38d1_b717);
/// 0x08654624, 0x08654628, 0x0865462c: the light's view from 0.1 to 100000, and the depth
/// a cell of the map starts with.
const MAP_NEAR: f32 = f32::from_bits(0x3dcc_cccd);
const MAP_FAR: f32 = 100000.0;
const MAP_EMPTY: f32 = -100000.0;
/// `shadowmap` (0x087997c0): the file's working copy of a light's depth-map record (the
/// 0x9c bytes at light +0x94: two matrices, at +0x80 the map's rows, at +0x84 its size).
pub const SHADOWMAP: u32 = 0x0879_97c0;

/// The depth map of a light [soft 0x082778fc shadowBuffer]: `shadowBuffer(light)`, ported
/// in place: the scene is prepared once more, seen from the light, and `zSample` fills the
/// map.
///
/// 1. The light's record (+0x94, 0x9c bytes) is copied to `shadowmap`. `SI_ren` (0x600
///    bytes), `SI_database` (0x58c) and `SI_stats` (0x44) are saved in locals.
/// 2. The controls become those of a square picture of the map's size (width and height:
///    the size's low sixteen bits; pixel ratio 1; both crop words 0; sampling level 1; the
///    ray tracing word 0), the camera copy the light: position = light +0x34, interest =
///    +0x58, roll 0, near 0.1, far 100000, no depth of field, field of view = r32(cone +
///    spread) (+0x70, +0x74), or the float nearest 0.0001 when the sum (unrounded) is
///    below 0.0001 in size.
/// 3. The saved counter of depth cells (`SI_stats` +0x3c) grows by size². The map comes
///    from chunk memory: `size` row pointers, each row `size` floats of −100000.
/// 4. `si_initViewport`, `SI_camera`. Then the record's first matrix = the renderer's
///    inverse view matrix **as it was saved** (0x087907ec) × the light's view matrix
///    (0x0879082c): from the renderer's space to the light's; its second = that × the
///    light's perspective matrix (0x087908ac).
/// 5. The chunk mark is saved; a scan-line table of trunc((window y max + 10) × 4) bytes is
///    taken and its entries 0 … y max cleared (none when y max is below 0 or not a
///    number); `si_traverseExpandedInstance(root)`, `si_traverse(root)`, `si_setBounds`,
///    `zSample(light)`.
/// 6. The three records are put back, the chunk mark reclaimed (the map's rows lie below
///    it and stay), and `shadowmap` is copied back to the light.
///
/// Callees through [`crate::img::Outside`]: `SI_chunkMalloc (0x0857cb64)`,
/// `SI_chunkSaveCurrent (0x0857ccc4)`, `SI_chunkReclaim (0x0857cd00)`, `si_initViewport
/// (0x082768c4)`, `SI_camera (0x08272450)`, `si_traverseExpandedInstance (0x082739e0)`,
/// `si_traverse (0x08273910)`, `si_setBounds (0x0826f318)`, `zSample (0x08277c94)`.
/// `si_ident` and `SI_mmul` are `si-math`'s. Class: model.
pub fn shadow_buffer(m: &mut si_mem::Mem, env: &mut dyn crate::img::Outside, light: u32) {
    use crate::img::{f, g};
    use si_math::matrix::{ident, mmul};
    use si_math::types::Matrix;
    use si_x87::Round;
    let matrix = |m: &si_mem::Mem, at: u32| -> Matrix { core::array::from_fn(|i| core::array::from_fn(|j| m.f32(at + 16 * i as u32 + 4 * j as u32))) };
    let store = |m: &mut si_mem::Mem, at: u32, v: &Matrix| {
        for (i, row) in v.iter().enumerate() {
            for (j, e) in row.iter().enumerate() {
                m.set_f32(at + 16 * i as u32 + 4 * j as u32, *e);
            }
        }
    };
    m.copy(SHADOWMAP, light + 0x94, 0x9c);
    let sp = m.sp();
    let (saved_ren, saved_db, saved_stats, mark) = (m.local(0x600), m.local(0x58c), m.local(0x44), m.local(4));
    m.copy(saved_ren, g::SI_REN, 0x600);
    m.copy(saved_db, g::SI_DATABASE, 0x58c);
    m.copy(saved_stats, g::SI_STATS, 0x44);
    let size16 = m.u16(SHADOWMAP + 0x84);
    m.set_u32(0x0879_0d34, 0x3f80_0000);
    m.set_u16(g::CROP, 0);
    m.set_u16(g::CROP_HIGH, 0);
    m.set_u16(g::HEIGHT, size16);
    m.set_u16(g::WIDTH, size16);
    m.set_i16(0x0879_0d38, 1);
    m.set_u16(0x0879_0c7c, 0);
    m.copy(g::SI_DATABASE + 0x10, light + 0x34, 12);
    m.copy(g::SI_DATABASE + 0x1c, light + 0x58, 12);
    let angle = x(m.f32(light + 0x70)) + x(m.f32(light + 0x74));
    m.set_f32(g::SI_DATABASE + 0x40, angle.to_f32());
    if angle.abs().lt(x(ANGLE_LEAST)) {
        m.set_f32(g::SI_DATABASE + 0x40, ANGLE_FLOOR);
    }
    m.set_u32(g::SI_DATABASE + 0x30, 0);
    m.set_f32(g::SI_DATABASE + 0x34, MAP_NEAR);
    m.set_f32(g::SI_DATABASE + 0x38, MAP_FAR);
    let size = m.i32(SHADOWMAP + 0x84);
    m.set_i32(g::SI_DATABASE + 0x6c, 0);
    let cells = m.i32(saved_stats + 0x3c).wrapping_add(size.wrapping_mul(size));
    m.set_i32(saved_stats + 0x3c, cells);
    let chunk = m.u32(g::SI_REN_CHUNK);
    let rows = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (size as u32) << 2]);
    m.set_u32(SHADOWMAP + 0x80, rows);
    let mut i: i32 = 0;
    while i < m.i32(SHADOWMAP + 0x84) {
        let chunk = m.u32(g::SI_REN_CHUNK);
        let row = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (m.i32(SHADOWMAP + 0x84) as u32) << 2]);
        let slot = m.u32(SHADOWMAP + 0x80).wrapping_add(4 * i as u32);
        m.set_u32(slot, row);
        let mut j: i32 = 0;
        while j < m.i32(SHADOWMAP + 0x84) {
            let row = m.u32(m.u32(SHADOWMAP + 0x80).wrapping_add(4 * i as u32));
            m.set_f32(row.wrapping_add(4 * j as u32), MAP_EMPTY);
            j += 1;
        }
        i += 1;
    }
    env.call(m, f::SI_INIT_VIEWPORT, &[]);
    env.call(m, f::SI_CAMERA, &[]);
    let to_light = mmul(&mmul(&ident(), &matrix(m, saved_db + 0x1ac)), &matrix(m, g::SI_DATABASE + 0x1ec));
    store(m, SHADOWMAP, &to_light);
    let onto_map = mmul(&to_light, &matrix(m, g::SI_DATABASE + 0x26c));
    store(m, SHADOWMAP + 0x40, &onto_map);
    let chunk = m.u32(g::SI_REN_CHUNK);
    env.call(m, f::SI_CHUNK_SAVE_CURRENT, &[chunk, mark]);
    const Y_MAX: u32 = 0x0879_07bc;
    const TABLE: u32 = 0x0879_08ec;
    let bytes = ((x(m.f32(Y_MAX)) + x(10.0f32)) * x(4.0f32)).to_i32(Round::Zero);
    let chunk = m.u32(g::SI_REN_CHUNK);
    let table = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, bytes as u32]);
    m.set_u32(TABLE, table);
    let y_max = x(m.f32(Y_MAX));
    if x(0.0f32).le(y_max) {
        let mut k: i32 = 0;
        loop {
            let t = m.u32(TABLE);
            m.set_u32(t.wrapping_add(4 * k as u32), 0);
            k += 1;
            if !x(k).le(x(m.f32(Y_MAX))) {
                break;
            }
        }
    }
    let root = m.u32(g::DB_ROOT);
    env.call(m, f::SI_TRAVERSE_EXPANDED_INSTANCE, &[root]);
    let root = m.u32(g::DB_ROOT);
    env.call(m, f::SI_TRAVERSE, &[root]);
    env.call(m, f::SI_SET_BOUNDS, &[]);
    env.call(m, f::Z_SAMPLE, &[light]);
    m.copy(g::SI_REN, saved_ren, 0x600);
    m.copy(g::SI_DATABASE, saved_db, 0x58c);
    m.copy(g::SI_STATS, saved_stats, 0x44);
    let (chunk, mark) = (m.u32(g::SI_REN_CHUNK), m.u32(mark));
    env.call(m, f::SI_CHUNK_RECLAIM, &[chunk, mark]);
    m.copy(light + 0x94, SHADOWMAP, 0x9c);
    m.set_sp(sp);
}

/// 0x08654638 (float): the key the head of the depth order starts with. 0x0865463c (float)
/// and 0x08654640 (double): 100000, the distance a ray starts with and the depth of a cell
/// nothing covers twice. 0x08654648, 0x08654650 (doubles): the half and the offset of the
/// depth that is stored.
const SORT_HEAD: f32 = -100000.0;
const RAY_FAR: f32 = 100000.0;
const NOTHING: f64 = 100000.0;
const HALF: f64 = 0.5;
const OFFSET: f64 = 0.0001;

/// The scan loop of a light's depth map [soft 0x08277c94 zSample]: `zSample(light)`,
/// ported in place. It is `SI_sample`'s loop over lines and columns without shading: per
/// cell of the map the two nearest surfaces are found and a depth between them is stored.
///
/// Set-up: three list heads of 0x28 bytes from chunk memory (the active slices, linked
/// through +0x1c; the cell's slices, through +0x20; the depth order, through +0x24, its
/// key −100000); a ray in a local with origin 0.
///
/// Per line y from y max (0x0879079c) down to y min (0x08790798):
///
/// 1. The active list: a slice whose last line (+0x14) is above y leaves; the others step
///    their two edges (+0x04 += +0x08, +0x0c += +0x10, each rounded to float); the slices
///    the scan-line table (0x087908ec) files under y join at the tail.
/// 2. The column table (0x087908f0): entries 0 … x max of the window (0x087907b4) are
///    cleared (none when x max is below 0 or not a number); every active slice is put at
///    the head of column trunc(x0), x0 held to 0 … x max (**0, not the window's x min as
///    in `SI_sample`**, and the index is the 32-bit integer).
/// 3. The line's direction: x = r32(x min's low sixteen bits × dx + x0) (0x087907d4,
///    0x087907c8), y = r32(the line's low sixteen bits × dy + y0) (0x087907d8,
///    0x087907cc), z = 0x087907d0.
/// 4. Per column x from x min (0x08790790) to x max (0x08790794): slices whose right edge
///    (the low sixteen bits of trunc(+0x0c)) lies left of x leave the cell's list, the
///    column's slices join at the tail. With a list: the direction is normalised (left as
///    it is when its length compares equal to 0); every slice gets its key ((direction ·
///    the triangle's normal) × the triangle's 1 / d: 1 / distance) at +0x00 and is put in
///    the depth order, nearest first; when slices joined at this column the cell's list is
///    relinked in the reverse of that order. Then down the depth order `SI_Intersect2(ray,
///    slice)` with the ray's distance at 100000: a hit counts when the light is not
///    selective (+0x15c) or the table at +0x168 holds the light's value (+0x160) under the
///    triangle's model number (tn +0x58). Two distances are kept in the register, both
///    starting at 100000: a hit nearer than the nearest becomes the nearest and the old
///    nearest the second; else one nearer than the second becomes the second.
///    The cell `rows[y][x]` of `shadowmap` (0x08799840) gets r32(z × 100000) when the two
///    compare equal or the second is still 100000 (nothing, or one surface only), else
///    r32((nearest + ((second − nearest) × 0.5 + 0.0001)) × z). The direction's x grows by
///    dx, rounded to float.
///
/// A cell without a slice is not written. `SI_Intersect2` is called on the image
/// ([`crate::inimage::intersect2_at`]); `SI_chunkMalloc (0x0857cb64)` through
/// [`crate::img::Outside`]. Class: model.
pub fn z_sample(m: &mut si_mem::Mem, env: &mut dyn crate::img::Outside, light: u32) {
    use crate::img::{f, g};
    use si_x87::Round;
    const TABLE: u32 = 0x0879_08ec;
    const COLUMNS: u32 = 0x0879_08f0;
    const X_MIN: u32 = 0x0879_0790;
    const X_MAX: u32 = 0x0879_0794;
    const Y_MIN: u32 = 0x0879_0798;
    const Y_MAX: u32 = 0x0879_079c;
    const WINDOW_X_MAX: u32 = 0x0879_07b4;
    const DIR: u32 = 0x0879_07c8;
    let table = m.u32(TABLE);
    let columns = m.u32(COLUMNS);
    let at4 = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(4));
    let chunk = m.u32(g::SI_REN_CHUNK);
    let active = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, 0x28]);
    m.set_u32(active.wrapping_add(0x1c), 0);
    let chunk = m.u32(g::SI_REN_CHUNK);
    let cell = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, 0x28]);
    m.set_u32(cell.wrapping_add(0x20), 0);
    let chunk = m.u32(g::SI_REN_CHUNK);
    let order = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, 0x28]);
    m.set_f32(order, SORT_HEAD);
    let sp = m.sp();
    let ray = m.local(0x48);
    m.set_u32(ray + 8, 0);
    m.set_u32(ray + 4, 0);
    m.set_u32(ray, 0);
    let x_first16 = m.u16(X_MIN) as i16;
    let mut y16 = m.u16(Y_MAX);
    let mut y = m.i32(Y_MAX);
    if y >= m.i32(Y_MIN) {
        loop {
            // 1. the active list
            let mut prev = active;
            let mut s = m.u32(active + 0x1c);
            while s != 0 {
                if m.i32(s + 0x14) > y {
                    let next = m.u32(s + 0x1c);
                    m.set_u32(prev + 0x1c, next);
                } else {
                    let x0 = (x(m.f32(s + 0x04)) + m.f32(s + 0x08)).to_f32();
                    m.set_f32(s + 0x04, x0);
                    let x1 = (x(m.f32(s + 0x0c)) + m.f32(s + 0x10)).to_f32();
                    m.set_f32(s + 0x0c, x1);
                    prev = s;
                }
                s = m.u32(s + 0x1c);
            }
            let filed = m.u32(at4(table, y));
            m.set_u32(prev + 0x1c, filed);
            // 2. the column table
            if x(0.0f32).le(x(m.f32(WINDOW_X_MAX))) {
                let mut i: i32 = 0;
                loop {
                    m.set_u32(at4(columns, i), 0);
                    i = i.wrapping_add(1);
                    if !x(i).le(x(m.f32(WINDOW_X_MAX))) {
                        break;
                    }
                }
            }
            let mut s = m.u32(active + 0x1c);
            while s != 0 {
                let v = x(m.f32(s + 0x04));
                let held = if v.lt(x(0.0f32)) {
                    x(0.0f32)
                } else {
                    let x_max = x(m.f32(WINDOW_X_MAX));
                    if x_max.lt(v) {
                        x_max
                    } else {
                        v
                    }
                };
                let column = held.to_i32(Round::Zero);
                let head = m.u32(at4(columns, column));
                m.set_u32(s + 0x20, head);
                m.set_u32(at4(columns, column), s);
                s = m.u32(s + 0x1c);
            }
            // 3. the line's direction
            let mut dir_x = (x(x_first16 as i32) * m.f32(DIR + 0x0c) + m.f32(DIR)).to_f32();
            let dir_y = (x(y16 as i16 as i32) * m.f32(DIR + 0x10) + m.f32(DIR + 0x04)).to_f32();
            let dir_z = x(m.f32(DIR + 0x08)).to_f32();
            m.set_u32(cell + 0x20, 0);
            let mut col = m.i32(X_MIN);
            if col <= m.i32(X_MAX) {
                loop {
                    // 4. the cell's list
                    let mut prev = cell;
                    let mut s = m.u32(cell + 0x20);
                    while s != 0 {
                        let right = x(m.f32(s + 0x0c)).to_i32(Round::Zero) as i16 as i32;
                        if right >= col {
                            prev = s;
                        } else {
                            let next = m.u32(s + 0x20);
                            m.set_u32(prev + 0x20, next);
                        }
                        s = m.u32(s + 0x20);
                    }
                    let joined = m.u32(at4(columns, col));
                    m.set_u32(prev + 0x20, joined);
                    if m.u32(cell + 0x20) != 0 {
                        let mut d = [dir_x, dir_y, dir_z];
                        let len = ((x(d[1]) * d[1] + x(d[0]) * d[0]) + x(d[2]) * d[2]).sqrt();
                        if !x(0.0f32).eq(len) {
                            d = [(x(d[0]) / len).to_f32(), (x(d[1]) / len).to_f32(), (x(d[2]) / len).to_f32()];
                        }
                        m.set_u32(order + 0x24, 0);
                        let mut s = m.u32(cell + 0x20);
                        while s != 0 {
                            let tn = m.u32(s + 0x18);
                            let key = ((x(d[0]) * m.f32(tn + 0x0c) + x(d[1]) * m.f32(tn + 0x10)) + x(d[2]) * m.f32(tn + 0x14)) * m.f32(tn + 0x1c);
                            m.set_f32(s, key.to_f32());
                            let mut before = order;
                            let mut at;
                            loop {
                                at = m.u32(before + 0x24);
                                if at == 0 || !key.lt(x(m.f32(at))) {
                                    break;
                                }
                                before = at;
                            }
                            m.set_u32(before + 0x24, s);
                            m.set_u32(s + 0x24, at);
                            s = m.u32(s + 0x20);
                        }
                        if m.u32(at4(columns, col)) != 0 {
                            let mut last = 0u32;
                            let mut at = m.u32(order + 0x24);
                            while at != 0 {
                                m.set_u32(at + 0x20, last);
                                last = at;
                                at = m.u32(last + 0x24);
                            }
                            m.set_u32(cell + 0x20, last);
                        }
                        for (k, v) in d.iter().enumerate() {
                            m.set_f32(ray + 0x0c + 4 * k as u32, *v);
                        }
                        m.set_f32(ray + 0x24, RAY_FAR);
                        let (mut second, mut nearest) = (x(NOTHING), x(NOTHING));
                        let mut s = m.u32(order + 0x24);
                        while s != 0 {
                            m.set_u32(ray + 0x34, 0);
                            let r = crate::inimage::intersect2_at(m, ray, s);
                            // (the original leaves the loop on an answer of 1, which
                            // SI_Intersect2 never gives)
                            if r == 1 {
                                break;
                            }
                            let tn = m.u32(ray + 0x34);
                            if r != 2 && tn != 0 {
                                let model = m.i16(tn + 0x58) as i32;
                                let lit = m.i32(light + 0x15c) == 0 || m.u32(at4(m.u32(light + 0x168), model)) == m.u32(light + 0x160);
                                if lit {
                                    let t = x(m.f32(ray + 0x24));
                                    if nearest.gt(t) {
                                        second = nearest;
                                        nearest = t;
                                    } else if t.lt(second) {
                                        second = t;
                                    }
                                }
                            }
                            s = m.u32(s + 0x24);
                        }
                        let z = m.f32(ray + 0x14);
                        let depth = if second.eq(nearest) || x(NOTHING).eq(second) {
                            (x(z) * RAY_FAR).to_f32()
                        } else {
                            ((nearest + ((second - nearest) * x(HALF) + x(OFFSET))) * z).to_f32()
                        };
                        let row = m.u32(at4(m.u32(SHADOWMAP + 0x80), y));
                        m.set_f32(at4(row, col), depth);
                    }
                    dir_x = (x(dir_x) + m.f32(DIR + 0x0c)).to_f32();
                    col = col.wrapping_add(1);
                    if col > m.i32(X_MAX) {
                        break;
                    }
                }
            }
            y16 = y16.wrapping_sub(1);
            y = y.wrapping_sub(1);
            if y < m.i32(Y_MIN) {
                break;
            }
        }
    }
    m.set_sp(sp);
}
