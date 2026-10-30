// SPDX-License-Identifier: MIT
//! A wave moving a spline's points (`wave_update.c`): `si_waveSplineUpdate`, which
//! `SI_waveTransform` (`crate::wave_sys`) calls for a spline model at every update of its
//! deformations. `si_waveGeomUpdate` (0x084c81d8), the same for meshes, patches and NURBS
//! surfaces, is `crate::wave_geom`'s.
//!
//! Read from the instruction listing (the order of the x87 operations, which values are kept
//! in the register and which are rounded to a float on a store). **Compared with the emulated
//! original** (`re/tools/si-scene/emu_gen_wave.py`, `scene-emucheck re/out/si-scene/vec_wave`):
//! made-up waves, graphs, icons and splines; `si_waveComputeVectors`, the four helpers of
//! `wave_util.c`, `SI_modelState` and `SI_edGetElemFromWave` run as they are, the element
//! look-up, the curves' look-up and their values are recorded stand-ins.

use crate::edtools::ByAddr;
use crate::wave_mdl::{wave_compute_vectors, wave_dot_product, wave_find_graph_value, wave_norm};
use si_mem::Mem;
use si_x87::{x, Round, F80};

/// `SI_edFindFcv (0x080edb20)`, `edtools.c`: (element, group, relation id).
pub const SI_ED_FIND_FCV: u32 = 0x080e_db20;
/// `SI_fcurveValue (0x082002dc)`, `fcurve.c`: (curve, time, check); answers in the register.
pub const SI_FCURVE_VALUE: u32 = 0x0820_02dc;
/// `SI_frameRate` (0x0878204c), a float.
pub const SI_FRAME_RATE: u32 = 0x0878_204c;
/// The double 0.5 at 0x08661430 and 0.0001 at 0x08661438 (`.rodata`).
pub(crate) const HALF: f64 = 0.5;
const SMALL: f64 = 0.0001;

/// The outside world of the wave updates: the routines at an address as [`ByAddr`] has them,
/// and those that answer in the x87 register.
pub trait WaveOutside: ByAddr {
    /// The routine at `f` with the stack words `args`; the answer is what it leaves in st0.
    fn call_f80(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> F80;
}

/// The wave's frame on the stack, at the original's offsets from `ebp`: what a callee writes
/// through a pointer, and the three values the original may read before writing them.
struct Frame(u32);
impl Frame {
    /// `ebp − off`.
    fn at(&self, off: u32) -> u32 {
        self.0 + 0x80 - off
    }
}
const POSITION: u32 = 0x0c;
const U: u32 = 0x18;
const V: u32 = 0x24;
const SCALING: u32 = 0x30;
const DELTA: u32 = 0x3c;
const DIR: u32 = 0x48;
const INV: u32 = 0x4c;
const INDEX: u32 = 0x64;
/// The amplitude (`-0x74`), the distance the travel is taken from (`-0x80`).
const AMP: u32 = 0x74;
const ALONG: u32 = 0x80;

/// The spline points `input` (+0x10 the count, 16 bits; +0x14 the points, 0x30 bytes each:
/// three positions of three floats) moved by `wave` into `output` (+0x14) at `time`
/// [soft 0x084c7bb0 si_waveSplineUpdate]: `si_waveSplineUpdate(wave, input, output, time,
/// check)`.
///
/// Nothing without the wave's element (`SI_edGetElemFromWave`, its index to a local) or
/// without either of its curves (`SI_edFindFcv(element, 0, 0x266)` and `0x267`, both looked
/// up first). The frame is `(int)(time · SI_frameRate + 0.5)`, truncated, in the register;
/// before the wave's first frame (+0x10) nothing. Not cycling (+0x28 = 0): after its last
/// frame (+0x14) nothing. Cycling: nothing for a period (last − first) below 1; else while
/// the frame is after the last, the period is taken off it and `period / SI_frameRate` (in
/// the register) off the time (a float local, rounded each time).
///
/// Then `si_waveComputeVectors(wave, P, U, V, S)`, the amplitude `A` = curve 0x266's value
/// and the frequency `B` = 0x267's (`SI_fcurveValue(curve, time, check)`, the time as
/// called, rounded to floats). For wave types (+8) 0, 1 and 2 the amplitude is `A · S.y`
/// (rounded; it is also the local the points use). The travel `τ` = time − first frame /
/// `SI_frameRate` and `φ = (τ·0.5·accel (+0x1c) + speed (+0x18))·τ`, rounded once.
/// The direction is U for mode (+0xc) 0, V for mode 1, else not set.
///
/// For each point and each of its three positions: `D` = position − P (floats); `d` = D·U in
/// the register, the distance `|d|` rounded. At or beyond the wave's reach (+0x24) the
/// position is left. Else by type:
/// - 1: `e` = D·V; D −= d·U + e·V (x with `e` in the register, y and z with `e` rounded);
///   `r` = |D| (divided by S.z unless S.z is 0) rounded; the amplitude local becomes
///   `A·graph(+0x4c, r)` rounded, and with that value 0 (in the register) or mode 2 the
///   position is left; the distance along is `e` rounded.
/// - 0: D −= d·U, the distance along is |D|; mode 2 leaves the position; mode 1 takes D's
///   direction (1/|D| rounded, times D, each rounded) when |D| > 0.0001, else 0.
/// - 2: the distance along is |D| (D as it is) and the direction D's, or 0, as above.
/// - other: nothing of this (the amplitude and the distance along are what the locals hold:
///   the original reads them unwritten for the first point; see [`Frame`]).
///
/// Then `h = amp · graph(+0x2c, B · (along − φ))` with `along − φ` rounded; when the reach's
/// start (+0x20) is below the distance, `h` is scaled by `(reach − distance) / (reach −
/// start)`; the output position = input + h·direction, each component rounded on the store.
///
/// Where the original reads a local it has not written (the direction for modes other than
/// 0 and 1 with types 0 and 1; the amplitude and the distance along for other types; U and
/// V when `si_waveComputeVectors` finds no element though the first look-up found one) the
/// port reads the same local of its own frame on the image, which holds the fill (not
/// compared: the emulated original's stack holds other bytes).
pub fn wave_spline_update<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, wave: u32, input: u32, output: u32, time: f32, check: i16) {
    let f = Frame(m.local(0x80));
    wave_spline_update_in(o, m, &f, wave, input, output, time, check);
    m.release(0x80);
}

#[allow(clippy::too_many_arguments)]
fn wave_spline_update_in<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, f: &Frame, wave: u32, input: u32, output: u32, time: f32, check: i16) {
    let elem = crate::edtools::ed_get_elem_from_wave(o, m, wave, si_mem::Ptr32::new(f.at(INDEX)));
    if elem.is_null() {
        return;
    }
    let curve_a = o.call_addr(m, SI_ED_FIND_FCV, &[elem.0, 0, 0x266]);
    let curve_b = o.call_addr(m, SI_ED_FIND_FCV, &[elem.0, 0, 0x267]);
    if curve_a == 0 || curve_b == 0 {
        return;
    }
    let mut t = time;
    let rate = m.f32(SI_FRAME_RATE);
    let mut frame = (x(time) * rate + HALF).to_i32(Round::Zero);
    let first = m.i32(wave.wrapping_add(0x10));
    if frame < first {
        return;
    }
    if m.u32(wave.wrapping_add(0x28)) != 0 {
        let last = m.i32(wave.wrapping_add(0x14));
        let period = last.wrapping_sub(first);
        let span = F80::from_i32(period) / rate;
        if period <= 0 {
            return;
        }
        while frame > last {
            frame = frame.wrapping_sub(period);
            t = (x(t) - span).to_f32();
        }
    } else if frame > m.i32(wave.wrapping_add(0x14)) {
        return;
    }
    let (p, u, v, s) = (f.at(POSITION), f.at(U), f.at(V), f.at(SCALING));
    wave_compute_vectors(o, m, wave, p, u, v, s);
    let ck = check as i32 as u32;
    let amp_a = o.call_f80(m, SI_FCURVE_VALUE, &[curve_a, time.to_bits(), ck]).to_f32();
    let freq = o.call_f80(m, SI_FCURVE_VALUE, &[curve_b, time.to_bits(), ck]).to_f32();
    let kind = m.i32(wave.wrapping_add(8));
    let mut amp_a = amp_a;
    if (0..=2).contains(&kind) {
        let v = (x(amp_a) * m.f32(s + 4)).to_f32();
        m.set_f32(f.at(AMP), v);
        amp_a = v;
    }
    let tau = x(t) - F80::from_i32(m.i32(wave.wrapping_add(0x10))) / m.f32(SI_FRAME_RATE);
    let phase = ((tau * HALF * m.f32(wave.wrapping_add(0x1c)) + m.f32(wave.wrapping_add(0x18))) * tau).to_f32();
    let count = m.u16(input.wrapping_add(0x10)) as i32;
    let pin = m.u32(input.wrapping_add(0x14));
    let pout = m.u32(output.wrapping_add(0x14));
    let dir = f.at(DIR);
    match m.i32(wave.wrapping_add(0xc)) {
        0 => copy3(m, u, dir),
        1 => copy3(m, v, dir),
        _ => {}
    }
    let d = f.at(DELTA);
    let fl = |m: &Mem, a: u32| m.f32(a);
    for i in 0..count {
        m.set_i32(f.at(INDEX), i);
        let row = (i as u32).wrapping_mul(0x30);
        for k in 0..3u32 {
            let at = |c: u32| pin.wrapping_add(row + 0xc * k + 4 * c);
            for c in 0..3u32 {
                let val = (x(m.f32(at(c))) - m.f32(p + 4 * c)).to_f32();
                m.set_f32(d + 4 * c, val);
            }
            let dot = wave_dot_product(m, d, u);
            let dist = if dot.lt(F80::ZERO) { (-dot).to_f32() } else { dot.to_f32() };
            let reach = m.f32(wave.wrapping_add(0x24));
            // `fcomps`, `and 5`, `je`: at or beyond the reach (ordered) leaves the position
            if !(x(dist).lt(x(reach)) || x(dist).unordered(x(reach))) {
                continue;
            }
            let mode = |m: &Mem| m.i32(wave.wrapping_add(0xc));
            // what the original does after the type's part: `true` goes on to the move
            let go_on = match m.i32(wave.wrapping_add(8)) {
                1 => {
                    let e = wave_dot_product(m, d, v);
                    let ef = e.to_f32();
                    m.set_f32(f.at(ALONG), ef);
                    let sx = e * fl(m, v) + dot * fl(m, u);
                    m.set_f32(d, (x(fl(m, d)) - sx).to_f32());
                    let sy = x(ef) * fl(m, v + 4) + dot * fl(m, u + 4);
                    let sz = dot * fl(m, u + 8) + x(ef) * fl(m, v + 8);
                    let ny = (x(fl(m, d + 4)) - sy).to_f32();
                    let nz = (x(fl(m, d + 8)) - sz).to_f32();
                    m.set_f32(d + 4, ny);
                    m.set_f32(d + 8, nz);
                    let n = wave_norm(m, d);
                    let sz_ = x(fl(m, s + 8));
                    let r = if sz_.eq(F80::ZERO) { n } else { n / sz_ };
                    let g = wave_find_graph_value(m, wave.wrapping_add(0x4c), r.to_f32());
                    let a = x(amp_a) * g;
                    m.set_f32(f.at(AMP), a.to_f32());
                    // `fucompp`, `and 0x45`, `cmp 0x40`: exactly 0 leaves the position
                    !a.eq(F80::ZERO) && mode(m) != 2
                }
                0 => {
                    for c in 0..3u32 {
                        let val = (x(fl(m, d + 4 * c)) - dot * fl(m, u + 4 * c)).to_f32();
                        m.set_f32(d + 4 * c, val);
                    }
                    let n = wave_norm(m, d).to_f32();
                    m.set_f32(f.at(ALONG), n);
                    match mode(m) {
                        1 => {
                            direction(m, f.at(INV), dir, d, x(n));
                            true
                        }
                        2 => false,
                        _ => true,
                    }
                }
                2 => {
                    let n = wave_norm(m, d);
                    m.set_f32(f.at(ALONG), n.to_f32());
                    direction(m, f.at(INV), dir, d, n);
                    true
                }
                _ => true,
            };
            if !go_on {
                continue;
            }
            let along = (x(fl(m, f.at(ALONG))) - phase).to_f32();
            m.set_f32(f.at(ALONG), along);
            let arg = (x(freq) * along).to_f32();
            let g = wave_find_graph_value(m, wave.wrapping_add(0x2c), arg);
            let mut h = x(fl(m, f.at(AMP))) * g;
            let start = x(m.f32(wave.wrapping_add(0x20)));
            if start.lt(x(dist)) {
                let reach = x(m.f32(wave.wrapping_add(0x24)));
                h = h * ((reach - dist) / (reach - start));
            }
            for c in 0..3u32 {
                let off = row + 0xc * k + 4 * c;
                let val = if c < 2 { h * fl(m, dir + 4 * c) + fl(m, pin.wrapping_add(off)) } else { x(fl(m, pin.wrapping_add(off))) + h * fl(m, dir + 8) };
                m.set_f32(pout.wrapping_add(off), val.to_f32());
            }
        }
    }
}

/// `u` copied to `to` (each float moved through the unit).
pub(crate) fn copy3(m: &mut Mem, from: u32, to: u32) {
    for c in 0..3u32 {
        let v = si_x87::fmov(m.f32(from + 4 * c));
        m.set_f32(to + 4 * c, v);
    }
}

/// The direction of `d` whose length is `n` (in the register): above 0.0001, `1/n` rounded
/// to a float (the local at `inv_at`) and each component of `d` times it, rounded (x, then z, then y stored);
/// otherwise (and unordered) 0, 0, 0.
pub(crate) fn direction(m: &mut Mem, inv_at: u32, dir: u32, d: u32, n: F80) {
    if n.gt(x(SMALL)) {
        let inv = (F80::ONE / n).to_f32();
        m.set_f32(inv_at, inv);
        let vx = (x(inv) * m.f32(d)).to_f32();
        m.set_f32(dir, vx);
        let vz = (x(inv) * m.f32(d + 8)).to_f32();
        let vy = (x(inv) * m.f32(d + 4)).to_f32();
        m.set_f32(dir + 8, vz);
        m.set_f32(dir + 4, vy);
    } else {
        m.set_u32(dir + 8, 0);
        m.set_u32(dir + 4, 0);
        m.set_u32(dir, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA;
    use crate::wave_mdl::{spline_index_to_value, wave_find_graph_value, wave_norm};
    use crate::wave_sys::{self as ws, wave_transform};
    use si_dict::globals::SI_ELEM_DICTS;
    use si_mem::Ptr32;

    /// Every call logged: the element 0x5000, the curves 0x6000 (amplitude 2) and 0x6100
    /// (frequency 1).
    #[derive(Default)]
    struct Log(Vec<(u32, Vec<u32>)>);
    impl ByAddr for Log {
        fn call_addr(&mut self, _: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SI_ELEMDICT_GET_ELEM_FROM_DATA => 0x5000,
                SI_ED_FIND_FCV => 0x6000 + if args[2] == 0x267 { 0x100 } else { 0 },
                _ => 0,
            }
        }
    }
    impl WaveOutside for Log {
        fn call_f80(&mut self, _: &mut Mem, f: u32, args: &[u32]) -> F80 {
            self.0.push((f, args.to_vec()));
            x(if args[0] == 0x6000 { 2.0f32 } else { 1.0 })
        }
    }

    fn image() -> Mem {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        let dicts = m.malloc(0x70);
        m[SI_ELEM_DICTS] = Ptr32::new(dicts);
        m.set_f32(SI_FRAME_RATE, 30.0);
        m
    }

    fn floats(m: &mut Mem, v: &[f32]) -> u32 {
        let a = m.malloc(4 * v.len() as u32);
        for (k, f) in v.iter().enumerate() {
            m.set_f32(a + 4 * k as u32, *f);
        }
        a
    }

    fn graph(m: &mut Mem, at: u32, mode: i32, pts: &[f32]) {
        let p = floats(m, pts);
        m.set_i32(at, pts.len() as i32 / 2);
        m.set_i32(at + 4, mode);
        m.set_u32(at + 0x1c, p);
    }

    #[test]
    fn a_graph_clamps_cycles_or_runs_on() {
        let mut m = image();
        let g = m.malloc(0x20);
        let v = |m: &Mem, x0: f32| wave_find_graph_value(m, g, x0).to_f32();
        graph(&mut m, g, 0, &[0.0, 0.0, 1.0, 10.0, 2.0, 0.0]);
        assert_eq!((v(&m, 0.5), v(&m, 1.5), v(&m, 2.0), v(&m, -1.0)), (5.0, 5.0, 0.0, 0.0));
        m.set_i32(g + 4, 1);
        assert_eq!((v(&m, 2.5), v(&m, -0.5), v(&m, 6.25)), (5.0, 5.0, 2.5));
        m.set_i32(g + 4, 2);
        assert_eq!(v(&m, -1.0), -10.0);
        // one point, or the last not above the first: 0
        m.set_i32(g, 1);
        assert_eq!(v(&m, 0.5), 0.0);
        graph(&mut m, g, 0, &[1.0, 3.0, 1.0, 4.0]);
        assert_eq!(v(&m, 1.0), 0.0);
    }

    #[test]
    fn a_spline_index_becomes_its_parameter() {
        let mut m = image();
        let out = m.malloc(4);
        let p = |m: &mut Mem, kind: i16, count: i16, index: i16| {
            m.set_f32(out, -7.0);
            spline_index_to_value(m, kind, count, index, out);
            m.f32(out)
        };
        assert_eq!(p(&mut m, 0, 5, 3), 3.0);
        assert_eq!(p(&mut m, 1, 5, 3), (3.0f64 * 0.33333) as f32);
        assert_eq!((p(&mut m, 2, 5, 0), p(&mut m, 2, 5, 4), p(&mut m, 3, 5, 2)), (0.0, 2.0, 1.0));
        assert_eq!((p(&mut m, 4, 5, 2), p(&mut m, -1, 5, 2)), (-7.0, -7.0));
    }

    #[test]
    fn the_norm_is_rounded_to_a_float() {
        let mut m = image();
        let a = floats(&mut m, &[3.0, 4.0, 0.0]);
        assert_eq!(wave_norm(&m, a).to_f32(), 5.0);
        let b = floats(&mut m, &[1.0, 1.0, 0.0]);
        assert_eq!(wave_norm(&m, b).to_bits(), x(core::f32::consts::SQRT_2).to_bits());
    }

    /// A wave of type 2 at the origin (U = y, V = x, scaling 1), reach 10, amplitude 2, a
    /// flat profile of 1: the points at z = 3 and x = 1 on the plane y = 0 go 2 further away
    /// from the icon; a point beyond the reach (y = 20) stays.
    fn spline_world(m: &mut Mem) -> (u32, u32) {
        let icon = m.malloc(0x170);
        for (off, v) in [(0x100u32, 1.0f32), (0x104, 1.0), (0x108, 1.0), (0x118, 1.0), (0x12c, 1.0)] {
            m.set_f32(icon + off, v);
        }
        let wave = m.malloc(0x84);
        m.set_i32(wave + 8, 2);
        m.set_i32(wave + 0x14, 100);
        m.set_f32(wave + 0x24, 10.0);
        graph(m, wave + 0x2c, 0, &[-100.0, 1.0, 100.0, 1.0]);
        m.set_u32(wave + 0x74, icon);
        let pts = floats(m, &[0.0, 0.0, 3.0, 0.0, 20.0, 0.0, 1.0, 0.0, 0.0]);
        let spline = m.malloc(0x20);
        m.set_u16(spline + 0x10, 1);
        m.set_u32(spline + 0x14, pts);
        (wave, pts)
    }

    #[test]
    fn a_spline_point_moves_along_its_direction_within_the_reach() {
        let mut m = image();
        let (wave, pts) = spline_world(&mut m);
        let spline = m.malloc(0x20);
        m.set_u16(spline + 0x10, 1);
        m.set_u32(spline + 0x14, pts);
        let mut o = Log::default();
        wave_spline_update(&mut o, &mut m, wave, spline, spline, 0.0, 0);
        let got: Vec<f32> = (0..9).map(|k| m.f32(pts + 4 * k)).collect();
        assert_eq!(got, [0.0, 0.0, 5.0, 0.0, 20.0, 0.0, 3.0, 0.0, 0.0]);
        // before the wave's first frame nothing is asked for its curves' values
        m.set_i32(wave + 0x10, 5);
        let mut o = Log::default();
        wave_spline_update(&mut o, &mut m, wave, spline, spline, 0.0, 0);
        assert!(o.0.iter().all(|c| c.0 != SI_FCURVE_VALUE));
        assert_eq!(m.f32(pts + 8), 5.0);
    }

    #[test]
    fn a_transform_applies_the_waves_of_its_side_by_kind() {
        let mut m = image();
        let model = m.malloc(0x50);
        m.set_i32(model + 0x30, 9);
        m.set_u32(model + 0x48, 0x7000);
        let mut list = 0;
        for (wave, flag) in [(0x3300u32, 2u32), (0x3200, 1), (0x3100, 0)] {
            let node = m.malloc(12);
            m.set_u32(node, wave);
            m.set_u32(node + 4, flag);
            m.set_u32(node + 8, list);
            list = node;
        }
        let waves = |o: &Log| o.0.iter().map(|c| c.1[0]).collect::<Vec<_>>();
        let mut o = Log::default();
        wave_transform(&mut o, &mut m, list, model, 0, 1.0, -1);
        assert_eq!(waves(&o), [0x3100, 0x3300]);
        assert_eq!(o.0[0].1, [0x3100, model, 1.0f32.to_bits(), u32::MAX]);
        let mut o = Log::default();
        wave_transform(&mut o, &mut m, list, model, 0x100, 1.0, 0);
        assert_eq!(waves(&o), [0x3200, 0x3300]);
        m.set_i32(model + 0x30, 4);
        let mut o = Log::default();
        wave_transform(&mut o, &mut m, list, model, 1, 1.0, 0);
        let calls: Vec<(u32, u32)> = o.0.iter().map(|c| (c.0, c.1[0])).collect();
        assert_eq!(calls[..3], [(ws::SI_WAVE_GEOM_UPDATE, 0x3200), (ws::SI_MESH_COMPUTES_POLYGON_NORMAL, 0x7000), (ws::SI_MESH_COMPUTES_VERTEX_NORMAL, 0x7000)]);
        m.set_i32(model + 0x30, 2);
        let mut o = Log::default();
        wave_transform(&mut o, &mut m, list, model, 1, 1.0, 0);
        assert!(o.0.is_empty());
    }
}
