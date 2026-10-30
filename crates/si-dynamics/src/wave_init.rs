// SPDX-License-Identifier: MIT
//! `wave_init.c` of Softimage|3D 4.0 (0x084c5c70 …): the setup of the wave code (finding 0045). A wave record is 0x84
//! bytes (our names): +0 the number (u16), +4 the name, +8 the profile kind, +0xc..+0x14 the frame range, +0x18 the
//! velocity, +0x20 / +0x24 the falloff range, +0x4c the amplitude count, +0x58..+0x64 the amplitude range, +0x68 the
//! amplitude pairs, +0x6c the attached model, +0x74 the icon model, +0x78 the next wave. The status record
//! (0x08822120): a u16 counter at +0 and the first link at +4.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

const WAVE_STATUS: u32 = 0x0882_2120;

/// Sets the wave globals up once [soft 0x084c5c70 SI_initWave]: when the flag at 0x088221ac is clear the counter
/// (u16 at 0x08822120), 0x0882219c and 0x088221b0 are cleared, 0x088221a8 and the flag are set to 1.
pub fn si_init_wave(m: &mut Mem) {
    if m.u32(0x0882_21ac) == 0 {
        m.set_u16(WAVE_STATUS, 0);
        m.set_u32(0x0882_219c, 0);
        m.set_u32(0x0882_21a8, 1);
        m.set_u32(0x0882_21ac, 1);
        m.set_u32(0x0882_21b0, 0);
    }
}

/// A new wave [soft 0x084c5cb4 si_waveAllocate]: the counter of `status` is raised, a 0x84-byte record is linked at the
/// end of the chain (links at +0x78, first at status+4) and filled (number = counter, name `wave<number>` in a 0x50
/// block, frame range 1..100, falloff 10000, velocity 10, the rest 0), and the editor is told. 0 when malloc fails
/// (the tail already points to the null).
pub fn si_wave_allocate(m: &mut Mem, o: &mut dyn Outside, status: u32) -> u32 {
    let c = m.u16(status).wrapping_add(1);
    m.set_u16(status, c);
    let mut tail = status + 4;
    while m.u32(tail + 0x78) != 0 {
        tail = m.u32(tail + 0x78);
    }
    let w = m.malloc(0x84);
    m.set_u32(tail + 0x78, w);
    if w == 0 {
        return 0;
    }
    m.set_u16(w, m.u16(status));
    m.set_u32(w + 0x6c, 0);
    m.set_u32(w + 0x78, 0);
    let name = m.malloc(0x50);
    m.set_u32(w + 4, name);
    let mut text = format!("wave{}", m.u16(status) as i16).into_bytes();
    text.push(0);
    m.set_bytes(name, &text);
    let big = m.f32(0x0866_13d8);
    m.set_f32(w + 0x20, big);
    m.set_f32(w + 0x24, big);
    let vel = m.f32(0x0866_13dc);
    m.set_f32(w + 0x18, vel);
    m.set_u32(w + 0x1c, 0);
    m.set_u32(w + 8, 0);
    m.set_u32(w + 0xc, 0);
    m.set_u32(w + 0x10, 1);
    m.set_u32(w + 0x14, 0x64);
    for off in [0x28, 0x50, 0x54, 0x4c, 0x68, 0x30, 0x34, 0x2c, 0x48, 0x74] {
        m.set_u32(w + off, 0);
    }
    o.si_ed_add_wave(m, 0, w);
    w
}

/// The default amplitude profile of a horizontal wave [soft 0x084c66e0 si_waveInitHorizProf]: only for profile kind 1.
/// The count (+0x4c) is 6, the range (+0x58, +0x5c) is the two image constants, +0x60 is 0, +0x64 is 1.0, and a block
/// of 6 pairs (48 bytes) at +0x68 holds (0, 1.0) and the image constants of 0x086613e8 .. 0x08661400, then (2.0, 0).
pub fn si_wave_init_horiz_prof(m: &mut Mem, wave: u32) {
    if m.i32(wave + 8) != 1 {
        return;
    }
    m.set_u32(wave + 0x4c, 6);
    let (lo, hi) = (m.f32(0x0866_13e0), m.f32(0x0866_13e4));
    m.set_f32(wave + 0x58, lo);
    m.set_f32(wave + 0x5c, hi);
    m.set_u32(wave + 0x60, 0);
    m.set_u32(wave + 0x64, 0x3f80_0000);
    let b = m.malloc(48);
    m.set_u32(wave + 0x68, b);
    m.set_u32(b, 0);
    m.set_u32(b + 4, 0x3f80_0000);
    let v = m.f32(0x0866_13e8);
    m.set_f32(b + 8, v);
    m.set_u32(b + 0xc, 0x3f80_0000);
    for (k, a) in [0x0866_13ec_u32, 0x0866_13f0, 0x0866_13f4, 0x0866_13f8, 0x0866_13fc, 0x0866_1400, 0x0866_13e4]
        .iter()
        .enumerate()
    {
        let v = m.f32(*a);
        m.set_f32(b + 0x10 + 4 * k as u32, v);
    }
    m.set_u32(b + 0x2c, 0);
}

/// Samples the spline of a path model into the amplitude graph of a wave [soft 0x084c5dcc si_waveAmpCreate]: only for a
/// model of kind 6 and a non-null graph. The spline `*(model+0x38)[u16 model+0x36]` has `n = SI_splineNbSegment` segments
/// and gets `8n` points (a block of `64n` bytes), at the parameters `0`, `k * step` (the sum of 80-bit additions of the
/// float `step = n / (8n - 1)`, each rounded to a float for the call) and `n`. Each point is carried through the model's
/// matrix (+0x1c4) and its (x, y) kept. The y range is tracked (min with a strict `<`, max with a strict `>`, on the
/// unrounded value). Unless x strictly rises (a strictly falling x is flipped end for end) the block is freed and 0 is
/// returned. The old block (graph+0x1c) is freed; the graph gets the count (+0), the block (+0x1c), the first and
/// last x (+0xc, +0x10) and the y range (+0x14, +0x18). Returns the count.
pub fn si_wave_amp_create(m: &mut Mem, o: &mut dyn Outside, graph: u32, model: u32) -> u32 {
    if m.i32(model + 0x30) != 6 || graph == 0 {
        return 0;
    }
    let spline = m.u32(m.u32(model + 0x38).wrapping_add(m.u16(model + 0x36) as u32 * 4));
    let nseg = o.si_spline_nb_segment(m, spline);
    if nseg <= 0 {
        return 0;
    }
    let n = (nseg as u32).wrapping_mul(8);
    let arr = m.malloc((nseg as u32).wrapping_shl(6));
    if arr == 0 {
        return 0;
    }
    let last = n.wrapping_sub(1) as i32;
    let step = (x(nseg as f32) / x(last)).to_f32();
    let out = m.local(12);
    let mx = |m: &Mem, i: u32| x(m.f32(model + 0x1c4 + 4 * i));
    // (x' rounded, y' unrounded) of the point just sampled into `out`
    let xform = |m: &Mem| {
        let (px, py, pz) = (x(m.f32(out)), x(m.f32(out + 4)), x(m.f32(out + 8)));
        let nx = (((py * mx(m, 4) + px * mx(m, 0)) + pz * mx(m, 8)) + mx(m, 12)).to_f32();
        let ny = ((px * mx(m, 1) + py * mx(m, 5)) + pz * mx(m, 9)) + mx(m, 13);
        (nx, ny)
    };
    o.si_spline_value(m, spline, 0.0, out);
    let (x0, y0) = xform(m);
    m.set_f32(arr, x0);
    let y0 = y0.to_f32();
    m.set_f32(arr + 4, y0);
    let (mut min, mut max) = (y0, y0);
    let mut t = x(step);
    let put = |m: &mut Mem, i: u32, px: f32, py: F80, min: &mut f32, max: &mut f32| {
        m.set_f32(arr + i * 8, px);
        m.set_f32(arr + i * 8 + 4, py.to_f32());
        if py.lt(x(*min)) {
            *min = py.to_f32();
        }
        if py.gt(x(*max)) {
            *max = py.to_f32();
        }
    };
    for i in 1..last.max(1) as u32 {
        o.si_spline_value(m, spline, t.to_f32(), out);
        let (px, py) = xform(m);
        put(m, i, px, py, &mut min, &mut max);
        t = t + x(step);
    }
    o.si_spline_value(m, spline, nseg as f32, out);
    let (px, py) = xform(m);
    put(m, n - 1, px, py, &mut min, &mut max);
    let falling = x(m.f32(arr + 8)).lt(x(m.f32(arr)));
    let mut ok = true;
    for i in 0..last.max(0) as u32 {
        let (a, b) = (x(m.f32(arr + i * 8)), x(m.f32(arr + i * 8 + 8)));
        if !falling {
            if b.le(a) {
                ok = false;
                break;
            }
        } else if !b.lt(a) && !b.unordered(a) {
            ok = false;
            break;
        }
    }
    if !ok {
        m.release(12);
        m.free(arr);
        return 0;
    }
    if falling {
        for i in 0..n / 2 {
            let j = n - 1 - i;
            let (ax, ay) = (m.u32(arr + i * 8), m.u32(arr + i * 8 + 4));
            let (bx, by) = (m.u32(arr + j * 8), m.u32(arr + j * 8 + 4));
            m.set_u32(arr + i * 8, bx);
            m.set_u32(arr + i * 8 + 4, by);
            m.set_u32(arr + j * 8, ax);
            m.set_u32(arr + j * 8 + 4, ay);
        }
    }
    m.release(12);
    let old = m.u32(graph + 0x1c);
    if old != 0 {
        m.free(old);
    }
    m.set_u32(graph, n);
    m.set_u32(graph + 0x1c, arr);
    let first = m.f32(arr);
    m.set_f32(graph + 0xc, first);
    let lastx = m.f32(arr + n * 8 - 8);
    m.set_f32(graph + 0x10, lastx);
    m.set_f32(graph + 0x14, min);
    m.set_f32(graph + 0x18, max);
    n
}
