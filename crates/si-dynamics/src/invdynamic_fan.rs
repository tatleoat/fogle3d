// SPDX-License-Identifier: MIT
//! invdynamic.c of the dynamics simulation, the fan force on a box (finding 0035).

use crate::contact::{rf, ri};
use crate::invdynamic::invdyna_get_bbox_face_fan_frce;
use si_mem::Mem;
use si_x87::x;

/// [soft 0x0849a0cc invdynaGetBboxFanFrce]: the force and torque of the fan `wMdl` (direction `wDir`, strength
/// `wAmpl`) on the box of a body, summed over the six faces of the box and added to the two result vectors `out1`
/// (force) and `out2` (torque). `mdl2` (or `mdl3` when it is null) gives the box (corner floats +0x10 .. +0x24 and
/// scale +0x16c .. +0x174). Each face is sampled at the box's axis planes (the loop runs the plane position from
/// -1 in steps of 2, so a face at each end of each axis), the face's points are moved into the frame of `mdl1`
/// (kind 2 by its matrix at +0x178, kind 12 by the parent's at +0x2bc when set) and handed to
/// `invdynaGetBboxFaceFanFrce`; only faces whose normal opposes `wDir` are used. The force is weighted by the two
/// strength words the face routine returns and the face area, turned by the matrix at `[mdl3+0x300]+0x1d8`, and the
/// torque is the cross product of the face centre (relative to `[mdl3+0x300]+0x244`, turned by +0x220) with it.
pub fn invdyna_get_bbox_fan_frce(m: &mut Mem, mdl1: u32, mdl2: u32, mdl3: u32, out1: u32, out2: u32) {
    let fr = m.local(0x148);
    let ebp = fr + 0x148;
    let jv = if mdl2 == 0 { mdl3 } else { mdl2 };
    bfan_body(m, ebp as i32, mdl1 as i32, mdl2 as i32, jv as i32, mdl3 as i32, out1 as i32, out2 as i32);
    m.release(0x148);
}

/// The body of invdynaGetBboxFanFrce: control flow written from the listing, arithmetic expanded from the symbolic
/// x87 order (bfan_prep.py + x87tpl.py). `JV` is the register ebx (model 2, or model 3 when model 2 is null), `PA`
/// model 3, `ebp` the frame base; slots are ebp-0xNN.
#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::all)]
fn bfan_body(m: &mut Mem, ebp: i32, M1: i32, M2: i32, JV: i32, PA: i32, out1: i32, out2: i32) {
    macro_rules! L {
        ($o:expr) => {
            x(m.f32((ebp - $o) as u32))
        };
    }
    macro_rules! W {
        ($o:expr) => {
            m.u32((ebp - $o) as u32)
        };
    }
    let z = x(0.0f32);
    let mut vl_20_52 = x(0.0f32);
    let mut vl_24_52 = x(0.0f32);
    let mut vl_68_110 = x(0.0f32);
    let mut vl_68_115 = x(0.0f32);
    let mut vl_68_126 = x(0.0f32);
    let mut vl_6c_110 = x(0.0f32);
    let mut vl_6c_115 = x(0.0f32);
    let mut vl_6c_126 = x(0.0f32);
    let mut vl_74_134 = x(0.0f32);
    let mut vl_74_139 = x(0.0f32);
    let mut vl_74_150 = x(0.0f32);
    let mut vl_78_134 = x(0.0f32);
    let mut vl_78_139 = x(0.0f32);
    let mut vl_78_150 = x(0.0f32);
    let mut vl_80_158 = x(0.0f32);
    let mut vl_80_163 = x(0.0f32);
    let mut vl_80_174 = x(0.0f32);
    let mut vl_84_158 = x(0.0f32);
    let mut vl_84_163 = x(0.0f32);
    let mut vl_84_174 = x(0.0f32);
    let (mut phi1, mut phi2, mut phi3, mut phi4) = (z, z, z, z);
    let (mut phi5, mut phi6, mut phi7, mut phi8) = (z, z, z, z);
    let (mut phi9, mut phi10, mut phi11, mut phi12) = (z, z, z, z);
    let kind = ri(m, M1 + 0x2ec);
    m.set_i32((ebp - 0x28) as u32, 0x0);
    m.set_i32((ebp - 0x2c) as u32, 0x0);
    m.set_i32((ebp - 0x30) as u32, 0x0);
    m.set_i32((ebp - 0x34) as u32, 0x0);
    m.set_i32((ebp - 0x38) as u32, 0x0);
    m.set_i32((ebp - 0x3c) as u32, 0x0);
    let t1 = ((rf(m, JV + 0x20) - rf(m, JV + 0x14)) * rf(m, JV + 0x170));
    m.set_f32((ebp - 0x18) as u32, ((t1 * ((rf(m, JV + 0x24) - rf(m, JV + 0x18)) * rf(m, JV + 0x174)))).to_f32());
    let t2 = ((rf(m, JV + 0x1c) - rf(m, JV + 0x10)) * rf(m, JV + 0x16c));
    m.set_f32((ebp - 0x10) as u32, ((t1 * t2)).to_f32());
    m.set_f32((ebp - 0x14) as u32, ((((rf(m, JV + 0x24) - rf(m, JV + 0x18)) * rf(m, JV + 0x174)) * t2)).to_f32());
    m.set_f32((ebp - 0xcc) as u32, ((rf(m, JV + 0x10) * rf(m, JV + 0x16c))).to_f32());
    m.set_f32((ebp - 0xc8) as u32, ((rf(m, JV + 0x14) * rf(m, JV + 0x170))).to_f32());
    m.set_f32((ebp - 0xc4) as u32, ((rf(m, JV + 0x18) * rf(m, JV + 0x174))).to_f32());
    m.set_f32((ebp - 0xd8) as u32, ((rf(m, JV + 0x1c) * rf(m, JV + 0x16c))).to_f32());
    m.set_f32((ebp - 0xd4) as u32, ((rf(m, JV + 0x20) * rf(m, JV + 0x170))).to_f32());
    let s18 = rf(m, JV + 0x24) * rf(m, JV + 0x174);
    m.set_f32((ebp - 0xd0) as u32, s18.to_f32());
    // the corner table: six pointers into the frame (4 points of 3 floats at 0x9c .. 0xc0, the maximum corner at 0xd8, the minimum at 0xcc)
    m.set_u32((ebp - 0x128) as u32, (ebp - 0x9c) as u32);
    m.set_u32((ebp - 0x124) as u32, (ebp - 0xa8) as u32);
    m.set_u32((ebp - 0x114) as u32, (ebp - 0xd8) as u32);
    m.set_u32((ebp - 0x120) as u32, (ebp - 0xb4) as u32);
    m.set_u32((ebp - 0x11c) as u32, (ebp - 0xc0) as u32);
    m.set_u32((ebp - 0x118) as u32, (ebp - 0xcc) as u32);
    m.set_u32((ebp - 0x108) as u32, (ebp - 0xc) as u32);
    let mut axis: i32 = 0;
    loop {
        // the two other axes (L_f4, L_f8)
        if axis == 1 {
            m.set_i32((ebp - 0xf4) as u32, 0);
            m.set_i32((ebp - 0xf8) as u32, 2);
        } else if axis < 2 {
            if axis == 0 {
                m.set_i32((ebp - 0xf4) as u32, 1);
                m.set_i32((ebp - 0xf8) as u32, 2);
            }
        } else if axis == 2 {
            m.set_i32((ebp - 0xf4) as u32, 0);
            m.set_i32((ebp - 0xf8) as u32, 1);
        }
        // the two corners of the box are copied into the four points along these two axes
        {
            let (p114, p118, p11c, p120, p124, p128) = (W!(0x114), W!(0x118), W!(0x11c), W!(0x120), W!(0x124), W!(0x128));
            let e = (m.i32((ebp - 0xf4) as u32) << 2) as u32;
            let v = m.u32(e.wrapping_add(p114));
            m.set_u32(e.wrapping_add(p124), v);
            m.set_u32(e.wrapping_add(p128), v);
            let v = m.u32(e.wrapping_add(p118));
            m.set_u32(e.wrapping_add(p11c), v);
            m.set_u32(e.wrapping_add(p120), v);
            let e = (m.i32((ebp - 0xf8) as u32) << 2) as u32;
            let v = m.u32(e.wrapping_add(p118));
            m.set_u32(e.wrapping_add(p11c), v);
            m.set_u32(e.wrapping_add(p128), v);
            let v = m.u32(e.wrapping_add(p114));
            m.set_u32(e.wrapping_add(p120), v);
            m.set_u32(e.wrapping_add(p124), v);
        }
        let c = m.u32(0x0866_09d8);
        m.set_u32((ebp - 0xfc) as u32, c);
        m.set_i32((ebp - 0x110) as u32, axis * 4);
        m.set_i32((ebp - 0x10c) as u32, axis + 1);
        loop {
            m.set_i32((ebp - 0xc) as u32, 0);
            m.set_i32((ebp - 0x8) as u32, 0);
            m.set_i32((ebp - 0x4) as u32, 0);
            {
                let a = (W!(0x110) as u32).wrapping_add(W!(0x108));
                let v = m.u32((ebp - 0xfc) as u32);
                m.set_u32(a, v);
            }
            let (v0, v1, v2) = (W!(0xc), W!(0x8), W!(0x4));
            m.set_u32((ebp - 0x24) as u32, v0);
            m.set_u32((ebp - 0x20) as u32, v1);
            m.set_u32((ebp - 0x1c) as u32, v2);
            if kind == 2 {
                if M2 != 0 {
                    let R0 = ri(m, ri(m, PA + 0x2c4) + 0x2c0);
                    if M2 != R0 {
                        let t3 = ((((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188))) + (L!(0x4) * rf(m, M2 + 0x198))) + rf(m, M2 + 0x1a8));
                        m.set_f32((ebp - 0x24) as u32, (t3).to_f32());
                        let t4 = ((((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c))) + (L!(0x4) * rf(m, M2 + 0x19c))) + rf(m, M2 + 0x1ac));
                        m.set_f32((ebp - 0x20) as u32, (t4).to_f32());
                        let t5 = (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0));
                        m.set_f32((ebp - 0x1c) as u32, (t5).to_f32());
                        m.set_f32((ebp - 0x24) as u32, ((((rf(m, R0 + 0x178) * t3) + (rf(m, R0 + 0x17c) * t4)) + (rf(m, R0 + 0x180) * t5))).to_f32());
                        m.set_f32((ebp - 0x20) as u32, ((((rf(m, R0 + 0x188) * t3) + (rf(m, R0 + 0x18c) * t4)) + (rf(m, R0 + 0x190) * t5))).to_f32());
                        m.set_f32((ebp - 0x1c) as u32, (((t5 * rf(m, R0 + 0x1a0)) + ((t4 * rf(m, R0 + 0x19c)) + (t3 * rf(m, R0 + 0x198))))).to_f32());
                        vl_20_52 = L!(0x20);
                        vl_24_52 = L!(0x24);
                    }
                    m.set_f32((ebp - 0x24) as u32, (((((L!(0x20) * rf(m, R0 + 0x128)) + (L!(0x24) * rf(m, R0 + 0x118))) + (L!(0x1c) * rf(m, R0 + 0x138))) + rf(m, R0 + 0x148))).to_f32());
                    m.set_f32((ebp - 0x20) as u32, (((((vl_24_52 * rf(m, R0 + 0x11c)) + (vl_20_52 * rf(m, R0 + 0x12c))) + (L!(0x1c) * rf(m, R0 + 0x13c))) + rf(m, R0 + 0x14c))).to_f32());
                    m.set_f32((ebp - 0x1c) as u32, (((((vl_20_52 * rf(m, R0 + 0x130)) + (vl_24_52 * rf(m, R0 + 0x120))) + (L!(0x1c) * rf(m, R0 + 0x140))) + rf(m, R0 + 0x150))).to_f32());
                }
            } else if M2 != PA {
                m.set_f32((ebp - 0x24) as u32, ((((rf(m, PA + 0x17c) * (((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac))) + (rf(m, PA + 0x178) * (((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8)))) + (rf(m, PA + 0x180) * (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0))))).to_f32());
                m.set_f32((ebp - 0x20) as u32, ((((rf(m, PA + 0x188) * (((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8))) + (rf(m, PA + 0x18c) * (((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)))) + (rf(m, PA + 0x190) * (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0))))).to_f32());
                m.set_f32((ebp - 0x1c) as u32, (((((((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)) * rf(m, PA + 0x19c)) + ((((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8)) * rf(m, PA + 0x198))) + ((((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) * rf(m, PA + 0x1a0)))).to_f32());
            }
            let wd0 = x(m.f32(0x0882_2018));
            let wd1 = x(m.f32(0x0882_201c));
            let wd2 = x(m.f32(0x0882_2020));
            let s59 = wd0 * L!(0x24);
            m.set_f32((ebp - 0x100) as u32, s59.to_f32());
            let s60 = (wd1 * L!(0x20)) + L!(0x100);
            m.set_f32((ebp - 0x100) as u32, s60.to_f32());
            let t6 = (wd2 * L!(0x1c)) + L!(0x100);
            m.set_f32((ebp - 0x100) as u32, t6.to_f32());
            if t6.lt(z) {
                // the plane of the face on this axis: the minimum corner, and the maximum one when the step is positive
                let (p114, p118, p11c, p120, p124, p128) = (W!(0x114), W!(0x118), W!(0x11c), W!(0x120), W!(0x124), W!(0x128));
                let o = W!(0x110);
                let (q12c, q130, q134) = (p124.wrapping_add(o), p120.wrapping_add(o), p11c.wrapping_add(o));
                m.set_u32((ebp - 0x12c) as u32, q12c);
                m.set_u32((ebp - 0x130) as u32, q130);
                m.set_u32((ebp - 0x134) as u32, q134);
                let v = m.u32(o.wrapping_add(p118));
                m.set_u32(q134, v);
                m.set_u32(q130, v);
                m.set_u32(q12c, v);
                m.set_u32(o.wrapping_add(p128), v);
                if z.lt(L!(0xfc)) {
                    let v = m.u32(o.wrapping_add(p114));
                    m.set_u32(q134, v);
                    m.set_u32(q130, v);
                    m.set_u32(q12c, v);
                    m.set_u32(o.wrapping_add(p128), v);
                }
                for (d, s) in [(0x60i32, 0x9ci32), (0x5c, 0x98), (0x58, 0x94), (0x6c, 0xa8), (0x68, 0xa4), (0x64, 0xa0), (0x78, 0xb4), (0x74, 0xb0), (0x70, 0xac), (0x84, 0xc0), (0x80, 0xbc), (0x7c, 0xb8)] {
                    let v = W!(s);
                    m.set_u32((ebp - d) as u32, v);
                }
                // the inputs of corners 2-4 as they stand before any corner runs (the stack-held values of the symbolic listing)
                vl_68_110 = L!(0x68);
                vl_68_115 = L!(0x68);
                vl_68_126 = L!(0x68);
                vl_6c_110 = L!(0x6c);
                vl_6c_115 = L!(0x6c);
                vl_6c_126 = L!(0x6c);
                vl_74_134 = L!(0x74);
                vl_74_139 = L!(0x74);
                vl_74_150 = L!(0x74);
                vl_78_134 = L!(0x78);
                vl_78_139 = L!(0x78);
                vl_78_150 = L!(0x78);
                vl_80_158 = L!(0x80);
                vl_80_163 = L!(0x80);
                vl_80_174 = L!(0x80);
                vl_84_158 = L!(0x84);
                vl_84_163 = L!(0x84);
                vl_84_174 = L!(0x84);
                // corner 1 (L_9c .. -> L_60 ..)
                if kind == 2 {
                    let t19 = ((((L!(0x9c) * rf(m, JV + 0x178)) + (L!(0x98) * rf(m, JV + 0x188))) + (L!(0x94) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                    m.set_f32((ebp - 0x60) as u32, (t19).to_f32());
                    let t20 = ((((L!(0x9c) * rf(m, JV + 0x17c)) + (L!(0x98) * rf(m, JV + 0x18c))) + (L!(0x94) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                    m.set_f32((ebp - 0x5c) as u32, (t20).to_f32());
                    let t21 = (((L!(0x94) * rf(m, JV + 0x1a0)) + ((L!(0x98) * rf(m, JV + 0x190)) + (L!(0x9c) * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                    m.set_f32((ebp - 0x58) as u32, (t21).to_f32());
                    let t22 = (t19 + rf(m, JV + 0x1b8));
                    m.set_f32((ebp - 0x60) as u32, (t22).to_f32());
                    let t23 = (t20 + rf(m, JV + 0x1bc));
                    m.set_f32((ebp - 0x5c) as u32, (t23).to_f32());
                    m.set_f32((ebp - 0x60) as u32, ((((rf(m, M1 + 0x17c) * (t23 - rf(m, M1 + 0x1bc))) + (rf(m, M1 + 0x178) * (t22 - rf(m, M1 + 0x1b8)))) + (rf(m, M1 + 0x180) * ((t21 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x5c) as u32, ((((rf(m, M1 + 0x188) * (t22 - rf(m, M1 + 0x1b8))) + (rf(m, M1 + 0x18c) * (t23 - rf(m, M1 + 0x1bc)))) + (rf(m, M1 + 0x190) * ((t21 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x58) as u32, (((((t23 - rf(m, M1 + 0x1bc)) * rf(m, M1 + 0x19c)) + ((t22 - rf(m, M1 + 0x1b8)) * rf(m, M1 + 0x198))) + (((t21 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0)) * rf(m, M1 + 0x1a0)))).to_f32());
                    vl_68_110 = L!(0x68);
                    vl_6c_110 = L!(0x6c);
                } else if kind == 12 {
                    let B = ri(m, M1 + 0x2bc);
                    if B == 0 {
                        let t7 = ((((L!(0x9c) * rf(m, JV + 0x178)) + (L!(0x98) * rf(m, JV + 0x188))) + (L!(0x94) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x60) as u32, (t7).to_f32());
                        let t8 = ((((L!(0x9c) * rf(m, JV + 0x17c)) + (L!(0x98) * rf(m, JV + 0x18c))) + (L!(0x94) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x5c) as u32, (t8).to_f32());
                        let t9 = (((L!(0x94) * rf(m, JV + 0x1a0)) + ((L!(0x98) * rf(m, JV + 0x190)) + (L!(0x9c) * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x58) as u32, (t9).to_f32());
                        m.set_f32((ebp - 0x60) as u32, ((t7 + rf(m, JV + 0x1b8))).to_f32());
                        m.set_f32((ebp - 0x5c) as u32, ((t8 + rf(m, JV + 0x1bc))).to_f32());
                        m.set_f32((ebp - 0x58) as u32, ((t9 + rf(m, JV + 0x1c0))).to_f32());
                    } else {
                        let t10 = ((((L!(0x9c) * rf(m, JV + 0x178)) + (L!(0x98) * rf(m, JV + 0x188))) + (L!(0x94) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x60) as u32, (t10).to_f32());
                        let t11 = ((((L!(0x9c) * rf(m, JV + 0x17c)) + (L!(0x98) * rf(m, JV + 0x18c))) + (L!(0x94) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x5c) as u32, (t11).to_f32());
                        let t12 = (((L!(0x94) * rf(m, JV + 0x1a0)) + ((L!(0x98) * rf(m, JV + 0x190)) + (L!(0x9c) * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x58) as u32, (t12).to_f32());
                        let t13 = (t10 + rf(m, JV + 0x1b8));
                        m.set_f32((ebp - 0x60) as u32, (t13).to_f32());
                        let t14 = (t11 + rf(m, JV + 0x1bc));
                        m.set_f32((ebp - 0x5c) as u32, (t14).to_f32());
                        let t15 = (t12 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x58) as u32, (t15).to_f32());
                        let t16 = (t13 - rf(m, B + 0x1b8));
                        m.set_f32((ebp - 0x60) as u32, (t16).to_f32());
                        let t17 = (t14 - rf(m, B + 0x1bc));
                        m.set_f32((ebp - 0x5c) as u32, (t17).to_f32());
                        let t18 = (t15 - rf(m, B + 0x1c0));
                        m.set_f32((ebp - 0x58) as u32, (t18).to_f32());
                        m.set_f32((ebp - 0x60) as u32, ((((rf(m, B + 0x178) * t16) + (rf(m, B + 0x17c) * t17)) + (rf(m, B + 0x180) * t18))).to_f32());
                        m.set_f32((ebp - 0x5c) as u32, ((((rf(m, B + 0x188) * t16) + (rf(m, B + 0x18c) * t17)) + (rf(m, B + 0x190) * t18))).to_f32());
                        m.set_f32((ebp - 0x58) as u32, (((t18 * rf(m, B + 0x1a0)) + ((t17 * rf(m, B + 0x19c)) + (t16 * rf(m, B + 0x198))))).to_f32());
                    }
                }
                // corner 2 (L_a8 .. -> L_6c ..)
                if kind == 2 {
                    let t36 = ((((L!(0x68) * rf(m, JV + 0x188)) + (L!(0x6c) * rf(m, JV + 0x178))) + (L!(0x64) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                    m.set_f32((ebp - 0x6c) as u32, (t36).to_f32());
                    let t37 = ((((vl_6c_126 * rf(m, JV + 0x17c)) + (vl_68_126 * rf(m, JV + 0x18c))) + (L!(0x64) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                    m.set_f32((ebp - 0x68) as u32, (t37).to_f32());
                    let t38 = (((L!(0x64) * rf(m, JV + 0x1a0)) + ((vl_68_126 * rf(m, JV + 0x190)) + (vl_6c_126 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                    m.set_f32((ebp - 0x64) as u32, (t38).to_f32());
                    let t39 = (t36 + rf(m, JV + 0x1b8));
                    m.set_f32((ebp - 0x6c) as u32, (t39).to_f32());
                    let t40 = (t37 + rf(m, JV + 0x1bc));
                    m.set_f32((ebp - 0x68) as u32, (t40).to_f32());
                    m.set_f32((ebp - 0x6c) as u32, ((((rf(m, M1 + 0x17c) * (t40 - rf(m, M1 + 0x1bc))) + (rf(m, M1 + 0x178) * (t39 - rf(m, M1 + 0x1b8)))) + (rf(m, M1 + 0x180) * ((t38 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x68) as u32, ((((rf(m, M1 + 0x188) * (t39 - rf(m, M1 + 0x1b8))) + (rf(m, M1 + 0x18c) * (t40 - rf(m, M1 + 0x1bc)))) + (rf(m, M1 + 0x190) * ((t38 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    phi1 = ((t39 - rf(m, M1 + 0x1b8)) * rf(m, M1 + 0x198));
                    phi2 = ((t40 - rf(m, M1 + 0x1bc)) * rf(m, M1 + 0x19c));
                    phi3 = (((t38 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0)) * rf(m, M1 + 0x1a0));
                    phi4 = (phi3 + (phi2 + phi1));
                    m.set_f32((ebp - 0x64) as u32, (phi4).to_f32());
                    vl_74_134 = L!(0x74);
                    vl_78_134 = L!(0x78);
                } else if kind == 12 {
                    let B = ri(m, M1 + 0x2bc);
                    if B == 0 {
                        let t24 = ((((L!(0x68) * rf(m, JV + 0x188)) + (L!(0x6c) * rf(m, JV + 0x178))) + (L!(0x64) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x6c) as u32, (t24).to_f32());
                        let t25 = ((((vl_6c_110 * rf(m, JV + 0x17c)) + (vl_68_110 * rf(m, JV + 0x18c))) + (L!(0x64) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x68) as u32, (t25).to_f32());
                        let t26 = (((L!(0x64) * rf(m, JV + 0x1a0)) + ((vl_68_110 * rf(m, JV + 0x190)) + (vl_6c_110 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x64) as u32, (t26).to_f32());
                        m.set_f32((ebp - 0x6c) as u32, ((t24 + rf(m, JV + 0x1b8))).to_f32());
                        m.set_f32((ebp - 0x68) as u32, ((t25 + rf(m, JV + 0x1bc))).to_f32());
                        vl_68_115 = L!(0x68);
                        vl_6c_115 = L!(0x6c);
                        phi4 = (t26 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x64) as u32, (phi4).to_f32());
                        vl_74_134 = L!(0x74);
                        vl_78_134 = L!(0x78);
                    } else {
                        let t27 = ((((L!(0x68) * rf(m, JV + 0x188)) + (L!(0x6c) * rf(m, JV + 0x178))) + (L!(0x64) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x6c) as u32, (t27).to_f32());
                        let t28 = ((((vl_6c_115 * rf(m, JV + 0x17c)) + (vl_68_115 * rf(m, JV + 0x18c))) + (L!(0x64) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x68) as u32, (t28).to_f32());
                        let t29 = (((L!(0x64) * rf(m, JV + 0x1a0)) + ((vl_68_115 * rf(m, JV + 0x190)) + (vl_6c_115 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x64) as u32, (t29).to_f32());
                        let t30 = (t27 + rf(m, JV + 0x1b8));
                        m.set_f32((ebp - 0x6c) as u32, (t30).to_f32());
                        let t31 = (t28 + rf(m, JV + 0x1bc));
                        m.set_f32((ebp - 0x68) as u32, (t31).to_f32());
                        let t32 = (t29 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x64) as u32, (t32).to_f32());
                        let t33 = (t30 - rf(m, B + 0x1b8));
                        m.set_f32((ebp - 0x6c) as u32, (t33).to_f32());
                        let t34 = (t31 - rf(m, B + 0x1bc));
                        m.set_f32((ebp - 0x68) as u32, (t34).to_f32());
                        let t35 = (t32 - rf(m, B + 0x1c0));
                        m.set_f32((ebp - 0x64) as u32, (t35).to_f32());
                        m.set_f32((ebp - 0x6c) as u32, ((((rf(m, B + 0x178) * t33) + (rf(m, B + 0x17c) * t34)) + (rf(m, B + 0x180) * t35))).to_f32());
                        m.set_f32((ebp - 0x68) as u32, ((((rf(m, B + 0x188) * t33) + (rf(m, B + 0x18c) * t34)) + (rf(m, B + 0x190) * t35))).to_f32());
                        vl_68_126 = L!(0x68);
                        vl_6c_126 = L!(0x6c);
                        phi1 = (t33 * rf(m, B + 0x198));
                        phi2 = (t34 * rf(m, B + 0x19c));
                        phi3 = (t35 * rf(m, B + 0x1a0));
                        phi4 = (phi3 + (phi2 + phi1));
                        m.set_f32((ebp - 0x64) as u32, (phi4).to_f32());
                        vl_74_134 = L!(0x74);
                        vl_78_134 = L!(0x78);
                    }
                }
                // corner 3 (L_b4 .. -> L_78 ..)
                if kind == 2 {
                    let t53 = ((((L!(0x74) * rf(m, JV + 0x188)) + (L!(0x78) * rf(m, JV + 0x178))) + (L!(0x70) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                    m.set_f32((ebp - 0x78) as u32, (t53).to_f32());
                    let t54 = ((((vl_78_150 * rf(m, JV + 0x17c)) + (vl_74_150 * rf(m, JV + 0x18c))) + (L!(0x70) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                    m.set_f32((ebp - 0x74) as u32, (t54).to_f32());
                    let t55 = (((L!(0x70) * rf(m, JV + 0x1a0)) + ((vl_74_150 * rf(m, JV + 0x190)) + (vl_78_150 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                    m.set_f32((ebp - 0x70) as u32, (t55).to_f32());
                    let t56 = (t53 + rf(m, JV + 0x1b8));
                    m.set_f32((ebp - 0x78) as u32, (t56).to_f32());
                    let t57 = (t54 + rf(m, JV + 0x1bc));
                    m.set_f32((ebp - 0x74) as u32, (t57).to_f32());
                    m.set_f32((ebp - 0x78) as u32, ((((rf(m, M1 + 0x17c) * (t57 - rf(m, M1 + 0x1bc))) + (rf(m, M1 + 0x178) * (t56 - rf(m, M1 + 0x1b8)))) + (rf(m, M1 + 0x180) * ((t55 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x74) as u32, ((((rf(m, M1 + 0x188) * (t56 - rf(m, M1 + 0x1b8))) + (rf(m, M1 + 0x18c) * (t57 - rf(m, M1 + 0x1bc)))) + (rf(m, M1 + 0x190) * ((t55 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    phi5 = ((t56 - rf(m, M1 + 0x1b8)) * rf(m, M1 + 0x198));
                    phi6 = ((t57 - rf(m, M1 + 0x1bc)) * rf(m, M1 + 0x19c));
                    phi7 = (((t55 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0)) * rf(m, M1 + 0x1a0));
                    phi8 = (phi7 + (phi6 + phi5));
                    m.set_f32((ebp - 0x70) as u32, (phi8).to_f32());
                    vl_80_158 = L!(0x80);
                    vl_84_158 = L!(0x84);
                } else if kind == 12 {
                    let B = ri(m, M1 + 0x2bc);
                    if B == 0 {
                        let t41 = ((((L!(0x74) * rf(m, JV + 0x188)) + (L!(0x78) * rf(m, JV + 0x178))) + (L!(0x70) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x78) as u32, (t41).to_f32());
                        let t42 = ((((vl_78_134 * rf(m, JV + 0x17c)) + (vl_74_134 * rf(m, JV + 0x18c))) + (L!(0x70) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x74) as u32, (t42).to_f32());
                        let t43 = (((L!(0x70) * rf(m, JV + 0x1a0)) + ((vl_74_134 * rf(m, JV + 0x190)) + (vl_78_134 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x70) as u32, (t43).to_f32());
                        m.set_f32((ebp - 0x78) as u32, ((t41 + rf(m, JV + 0x1b8))).to_f32());
                        m.set_f32((ebp - 0x74) as u32, ((t42 + rf(m, JV + 0x1bc))).to_f32());
                        vl_74_139 = L!(0x74);
                        vl_78_139 = L!(0x78);
                        phi8 = (t43 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x70) as u32, (phi8).to_f32());
                        vl_80_158 = L!(0x80);
                        vl_84_158 = L!(0x84);
                    } else {
                        let t44 = ((((L!(0x74) * rf(m, JV + 0x188)) + (L!(0x78) * rf(m, JV + 0x178))) + (L!(0x70) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x78) as u32, (t44).to_f32());
                        let t45 = ((((vl_78_139 * rf(m, JV + 0x17c)) + (vl_74_139 * rf(m, JV + 0x18c))) + (L!(0x70) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x74) as u32, (t45).to_f32());
                        let t46 = (((L!(0x70) * rf(m, JV + 0x1a0)) + ((vl_74_139 * rf(m, JV + 0x190)) + (vl_78_139 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x70) as u32, (t46).to_f32());
                        let t47 = (t44 + rf(m, JV + 0x1b8));
                        m.set_f32((ebp - 0x78) as u32, (t47).to_f32());
                        let t48 = (t45 + rf(m, JV + 0x1bc));
                        m.set_f32((ebp - 0x74) as u32, (t48).to_f32());
                        let t49 = (t46 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x70) as u32, (t49).to_f32());
                        let t50 = (t47 - rf(m, B + 0x1b8));
                        m.set_f32((ebp - 0x78) as u32, (t50).to_f32());
                        let t51 = (t48 - rf(m, B + 0x1bc));
                        m.set_f32((ebp - 0x74) as u32, (t51).to_f32());
                        let t52 = (t49 - rf(m, B + 0x1c0));
                        m.set_f32((ebp - 0x70) as u32, (t52).to_f32());
                        m.set_f32((ebp - 0x78) as u32, ((((rf(m, B + 0x178) * t50) + (rf(m, B + 0x17c) * t51)) + (rf(m, B + 0x180) * t52))).to_f32());
                        m.set_f32((ebp - 0x74) as u32, ((((rf(m, B + 0x188) * t50) + (rf(m, B + 0x18c) * t51)) + (rf(m, B + 0x190) * t52))).to_f32());
                        vl_74_150 = L!(0x74);
                        vl_78_150 = L!(0x78);
                        phi5 = (t50 * rf(m, B + 0x198));
                        phi6 = (t51 * rf(m, B + 0x19c));
                        phi7 = (t52 * rf(m, B + 0x1a0));
                        phi8 = (phi7 + (phi6 + phi5));
                        m.set_f32((ebp - 0x70) as u32, (phi8).to_f32());
                        vl_80_158 = L!(0x80);
                        vl_84_158 = L!(0x84);
                    }
                }
                // corner 4 (L_c0 .. -> L_84 ..)
                if kind == 2 {
                    let t70 = ((((L!(0x80) * rf(m, JV + 0x188)) + (L!(0x84) * rf(m, JV + 0x178))) + (L!(0x7c) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                    m.set_f32((ebp - 0x84) as u32, (t70).to_f32());
                    let t71 = ((((vl_84_174 * rf(m, JV + 0x17c)) + (vl_80_174 * rf(m, JV + 0x18c))) + (L!(0x7c) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                    m.set_f32((ebp - 0x80) as u32, (t71).to_f32());
                    let t72 = (((L!(0x7c) * rf(m, JV + 0x1a0)) + ((vl_80_174 * rf(m, JV + 0x190)) + (vl_84_174 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                    m.set_f32((ebp - 0x7c) as u32, (t72).to_f32());
                    let t73 = (t70 + rf(m, JV + 0x1b8));
                    m.set_f32((ebp - 0x84) as u32, (t73).to_f32());
                    let t74 = (t71 + rf(m, JV + 0x1bc));
                    m.set_f32((ebp - 0x80) as u32, (t74).to_f32());
                    m.set_f32((ebp - 0x84) as u32, ((((rf(m, M1 + 0x17c) * (t74 - rf(m, M1 + 0x1bc))) + (rf(m, M1 + 0x178) * (t73 - rf(m, M1 + 0x1b8)))) + (rf(m, M1 + 0x180) * ((t72 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x80) as u32, ((((rf(m, M1 + 0x188) * (t73 - rf(m, M1 + 0x1b8))) + (rf(m, M1 + 0x18c) * (t74 - rf(m, M1 + 0x1bc)))) + (rf(m, M1 + 0x190) * ((t72 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0))))).to_f32());
                    phi9 = ((t73 - rf(m, M1 + 0x1b8)) * rf(m, M1 + 0x198));
                    phi10 = ((t74 - rf(m, M1 + 0x1bc)) * rf(m, M1 + 0x19c));
                    phi11 = (((t72 + rf(m, JV + 0x1c0)) - rf(m, M1 + 0x1c0)) * rf(m, M1 + 0x1a0));
                    phi12 = (phi11 + (phi10 + phi9));
                    m.set_f32((ebp - 0x7c) as u32, (phi12).to_f32());
                } else if kind == 12 {
                    let B = ri(m, M1 + 0x2bc);
                    if B == 0 {
                        let t58 = ((((L!(0x80) * rf(m, JV + 0x188)) + (L!(0x84) * rf(m, JV + 0x178))) + (L!(0x7c) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x84) as u32, (t58).to_f32());
                        let t59 = ((((vl_84_158 * rf(m, JV + 0x17c)) + (vl_80_158 * rf(m, JV + 0x18c))) + (L!(0x7c) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x80) as u32, (t59).to_f32());
                        let t60 = (((L!(0x7c) * rf(m, JV + 0x1a0)) + ((vl_80_158 * rf(m, JV + 0x190)) + (vl_84_158 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x7c) as u32, (t60).to_f32());
                        m.set_f32((ebp - 0x84) as u32, ((t58 + rf(m, JV + 0x1b8))).to_f32());
                        m.set_f32((ebp - 0x80) as u32, ((t59 + rf(m, JV + 0x1bc))).to_f32());
                        vl_80_163 = L!(0x80);
                        vl_84_163 = L!(0x84);
                        phi12 = (t60 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x7c) as u32, (phi12).to_f32());
                    } else {
                        let t61 = ((((L!(0x80) * rf(m, JV + 0x188)) + (L!(0x84) * rf(m, JV + 0x178))) + (L!(0x7c) * rf(m, JV + 0x198))) + rf(m, JV + 0x1a8));
                        m.set_f32((ebp - 0x84) as u32, (t61).to_f32());
                        let t62 = ((((vl_84_163 * rf(m, JV + 0x17c)) + (vl_80_163 * rf(m, JV + 0x18c))) + (L!(0x7c) * rf(m, JV + 0x19c))) + rf(m, JV + 0x1ac));
                        m.set_f32((ebp - 0x80) as u32, (t62).to_f32());
                        let t63 = (((L!(0x7c) * rf(m, JV + 0x1a0)) + ((vl_80_163 * rf(m, JV + 0x190)) + (vl_84_163 * rf(m, JV + 0x180)))) + rf(m, JV + 0x1b0));
                        m.set_f32((ebp - 0x7c) as u32, (t63).to_f32());
                        let t64 = (t61 + rf(m, JV + 0x1b8));
                        m.set_f32((ebp - 0x84) as u32, (t64).to_f32());
                        let t65 = (t62 + rf(m, JV + 0x1bc));
                        m.set_f32((ebp - 0x80) as u32, (t65).to_f32());
                        let t66 = (t63 + rf(m, JV + 0x1c0));
                        m.set_f32((ebp - 0x7c) as u32, (t66).to_f32());
                        let t67 = (t64 - rf(m, B + 0x1b8));
                        m.set_f32((ebp - 0x84) as u32, (t67).to_f32());
                        let t68 = (t65 - rf(m, B + 0x1bc));
                        m.set_f32((ebp - 0x80) as u32, (t68).to_f32());
                        let t69 = (t66 - rf(m, B + 0x1c0));
                        m.set_f32((ebp - 0x7c) as u32, (t69).to_f32());
                        m.set_f32((ebp - 0x84) as u32, ((((rf(m, B + 0x178) * t67) + (rf(m, B + 0x17c) * t68)) + (rf(m, B + 0x180) * t69))).to_f32());
                        m.set_f32((ebp - 0x80) as u32, ((((rf(m, B + 0x188) * t67) + (rf(m, B + 0x18c) * t68)) + (rf(m, B + 0x190) * t69))).to_f32());
                        vl_80_174 = L!(0x80);
                        vl_84_174 = L!(0x84);
                        phi9 = (t67 * rf(m, B + 0x198));
                        phi10 = (t68 * rf(m, B + 0x19c));
                        phi11 = (t69 * rf(m, B + 0x1a0));
                        phi12 = (phi11 + (phi10 + phi9));
                        m.set_f32((ebp - 0x7c) as u32, (phi12).to_f32());
                    }
                }
                // the force of the fan on this face (a quadrilateral of four points in the fan's frame)
                let thr = m.u32((ebp + W!(0x110) as i32 - 0x18) as u32);
                let v = [W!(0x60), W!(0x5c), W!(0x58), W!(0x6c), W!(0x68), W!(0x64), W!(0x78), W!(0x74), W!(0x70), W!(0x84), W!(0x80), W!(0x7c)];
                invdyna_get_bbox_face_fan_frce(m, M1 as u32, v, thr, (ebp - 0x90) as u32, (ebp - 0xf0) as u32, (ebp - 0xec) as u32, 0);
                let G = ri(m, PA + 0x300);
                let t75 = (L!(0x90) - rf(m, G + 0x244));
                m.set_f32((ebp - 0x90) as u32, (t75).to_f32());
                let t76 = (L!(0x8c) - rf(m, G + 0x248));
                m.set_f32((ebp - 0x8c) as u32, (t76).to_f32());
                let t77 = (L!(0x88) - rf(m, G + 0x24c));
                m.set_f32((ebp - 0x88) as u32, (t77).to_f32());
                let t78 = (((rf(m, G + 0x220) * t75) + (rf(m, G + 0x224) * t76)) + (rf(m, G + 0x228) * t77));
                m.set_f32((ebp - 0x90) as u32, (t78).to_f32());
                let t79 = ((rf(m, G + 0x234) * t77) + ((rf(m, G + 0x22c) * t75) + (rf(m, G + 0x230) * t76)));
                m.set_f32((ebp - 0x8c) as u32, (t79).to_f32());
                let t80 = ((L!(0x100) * L!(0xf0)) * L!(0xec));
                m.set_f32((ebp - 0x104) as u32, ((t79 * (t80 * L!(0x1c)))).to_f32());
                let t82 = (t80 * L!(0x20));
                m.set_f32((ebp - 0x44) as u32, (t82).to_f32());
                let t81 = ((t77 * rf(m, G + 0x240)) + ((t76 * rf(m, G + 0x23c)) + (t75 * rf(m, G + 0x238))));
                m.set_f32((ebp - 0x88) as u32, (t81).to_f32());
                let t83 = (t80 * L!(0x1c));
                m.set_f32((ebp - 0x40) as u32, (t83).to_f32());
                let t84 = (t80 * L!(0x24));
                m.set_f32((ebp - 0x48) as u32, (t84).to_f32());
                let t85 = (L!(0x104) - (t81 * t82));
                m.set_f32((ebp - 0x104) as u32, (t85).to_f32());
                m.set_f32((ebp - 0x54) as u32, (t85).to_f32());
                let t86 = ((t78 * t82) - (t79 * t84));
                m.set_f32((ebp - 0x4c) as u32, (t86).to_f32());
                let t87 = ((t81 * t84) - (t78 * t83));
                m.set_f32((ebp - 0x50) as u32, (t87).to_f32());
                let t88 = (((rf(m, G + 0x1d8) * t84) + (rf(m, G + 0x1dc) * t82)) + (rf(m, G + 0x1e0) * t83));
                m.set_f32((ebp - 0x48) as u32, (t88).to_f32());
                let t89 = (((rf(m, G + 0x1e4) * t84) + (rf(m, G + 0x1e8) * t82)) + (rf(m, G + 0x1ec) * t83));
                m.set_f32((ebp - 0x44) as u32, (t89).to_f32());
                m.set_f32((ebp - 0x30) as u32, ((t88 + L!(0x30))).to_f32());
                m.set_f32((ebp - 0x38) as u32, ((t87 + L!(0x38))).to_f32());
                m.set_f32((ebp - 0x34) as u32, ((t86 + L!(0x34))).to_f32());
                let t90 = ((t83 * rf(m, G + 0x1f8)) + ((t82 * rf(m, G + 0x1f4)) + (t84 * rf(m, G + 0x1f0))));
                m.set_f32((ebp - 0x40) as u32, (t90).to_f32());
                m.set_f32((ebp - 0x2c) as u32, ((t89 + L!(0x2c))).to_f32());
                m.set_f32((ebp - 0x3c) as u32, ((L!(0x104) + L!(0x3c))).to_f32());
                m.set_f32((ebp - 0x28) as u32, ((t90 + L!(0x28))).to_f32());
            }
            // the next step along the axis (-1 .. 1 in steps of 2)
            let nx = L!(0xfc) + x(m.f64(0x0866_09e0));
            m.set_f32((ebp - 0xfc) as u32, nx.to_f32());
            if x(1.0f32).lt(L!(0xfc)) || x(m.f32((ebp - 0xfc) as u32)).is_nan() {
                break;
            }
        }
        axis = m.i32((ebp - 0x10c) as u32);
        if axis > 2 {
            break;
        }
    }
    // add the sums to the two result vectors
    let (a, b, c) = (rf(m, out1) + L!(0x30), rf(m, out1 + 4) + L!(0x2c), rf(m, out1 + 8) + L!(0x28));
    m.set_f32(out1 as u32, a.to_f32());
    m.set_f32((out1 + 4) as u32, b.to_f32());
    m.set_f32((out1 + 8) as u32, c.to_f32());
    let (a, b, c) = (rf(m, out2) + L!(0x3c), rf(m, out2 + 4) + L!(0x38), rf(m, out2 + 8) + L!(0x34));
    m.set_f32(out2 as u32, a.to_f32());
    m.set_f32((out2 + 4) as u32, b.to_f32());
    m.set_f32((out2 + 8) as u32, c.to_f32());
}
