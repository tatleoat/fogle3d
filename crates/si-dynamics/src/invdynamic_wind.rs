// SPDX-License-Identifier: MIT
//! invdynamic.c of the dynamics simulation, the wind force on a box and on a mesh (finding 0035).

use crate::contact::{rf, ri};
use si_mem::Mem;
use si_x87::x;

/// [soft 0x0849b9ec invdynaGetBboxWindFrce]: the force and torque of the wind (`wDir`, strength `wAmpl`) on the box of
/// a body, added to the two result vectors `out1` (force) and `out2` (torque). `mdl2` (or `mdl3` when it is null)
/// gives the box (corner floats +0x10 .. +0x24, scale +0x16c .. +0x174). The three face pairs are sampled at their
/// centres and at the two sample positions -1 and 1 along each axis; a sample whose box normal (moved into the frame
/// of `mdl1`, kind 2 through the matrix chain `[mdl3+0x2c4]+0x2c0`, otherwise through the two models' matrices) opposes
/// `wDir` pushes with the wind component times the face area, turned by the matrix at `[mdl3+0x300]+0x1d8`, and
/// the torque is the cross product of the sample point with the push.
pub fn invdyna_get_bbox_wind_frce(m: &mut Mem, mdl1: u32, mdl2: u32, mdl3: u32, out1: u32, out2: u32) {
    let fr = m.local(0xac);
    let ebp = fr + 0xac;
    let mb = if mdl2 == 0 { mdl3 } else { mdl2 };
    bwind_body(m, ebp as i32, mdl1 as i32, mdl2 as i32, mdl3 as i32, mb as i32, out1 as i32, out2 as i32);
    m.release(0xac);
}

/// The body of invdynaGetBboxWindFrce: control flow written from the listing, arithmetic expanded from the symbolic
/// x87 order (bwind_prep.py + x87tpl.py). `M2` is the register ecx (model 2), `JV` the register ebx (model 3), `MB`
/// the model the box is read from (model 2, or model 3 when model 2 is null), `ebp` the frame base; slots are ebp-0xNN.
#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::all)]
fn bwind_body(m: &mut Mem, ebp: i32, M1: i32, M2: i32, JV: i32, MB: i32, out1: i32, out2: i32) {
    macro_rules! L {
        ($o:expr) => {
            x(m.f32((ebp - $o) as u32))
        };
    }
    let z = x(0.0f32);
    let mut vl_20_32 = x(0.0f32);
    let mut vl_24_32 = x(0.0f32);
    let mut vl_5c_44 = x(0.0f32);
    let mut vl_5c_53 = x(0.0f32);
    let mut vl_5c_58 = x(0.0f32);
    let mut vl_60_44 = x(0.0f32);
    let mut vl_60_53 = x(0.0f32);
    let mut vl_60_58 = x(0.0f32);
    let mut phi1 = z;
    m.set_i32((ebp - 0x28) as u32, 0x0);
    m.set_i32((ebp - 0x2c) as u32, 0x0);
    m.set_i32((ebp - 0x30) as u32, 0x0);
    m.set_i32((ebp - 0x34) as u32, 0x0);
    m.set_i32((ebp - 0x38) as u32, 0x0);
    m.set_i32((ebp - 0x3c) as u32, 0x0);
    // the box of the model: size (S8-S10), centre (S11, S12, S16), the face areas (S13, S14, S17)
    m.set_f32((ebp - 0x78) as u32, ((rf(m, MB + 0x1c) - rf(m, MB + 0x10)) * rf(m, MB + 0x16c)).to_f32());
    m.set_f32((ebp - 0x7c) as u32, ((rf(m, MB + 0x20) - rf(m, MB + 0x14)) * rf(m, MB + 0x170)).to_f32());
    m.set_f32((ebp - 0x80) as u32, ((rf(m, MB + 0x24) - rf(m, MB + 0x18)) * rf(m, MB + 0x174)).to_f32());
    let half = x(m.f64(0x0866_0a40));
    m.set_f32((ebp - 0x84) as u32, ((rf(m, MB + 0x10) + rf(m, MB + 0x1c)) * half).to_f32());
    m.set_f32((ebp - 0x88) as u32, ((rf(m, MB + 0x14) + rf(m, MB + 0x20)) * half).to_f32());
    m.set_f32((ebp - 0x18) as u32, (L!(0x7c) * L!(0x80)).to_f32());
    m.set_f32((ebp - 0x14) as u32, (L!(0x78) * L!(0x80)).to_f32());
    m.set_f32((ebp - 0x8c) as u32, (half * (rf(m, MB + 0x18) + rf(m, MB + 0x24))).to_f32());
    m.set_f32((ebp - 0x10) as u32, (L!(0x78) * L!(0x7c)).to_f32());
    let mut axis: i32 = 0;
    loop {
        let c = m.u32(0x0866_0a48);
        m.set_u32((ebp - 0x74) as u32, c);
        m.set_i32((ebp - 0x98) as u32, axis + 1);
        loop {
            m.set_i32((ebp - 0xc) as u32, 0);
            m.set_i32((ebp - 0x8) as u32, 0);
            m.set_i32((ebp - 0x4) as u32, 0);
            // the sample position runs along the axis: local_1c[axis + 3] of the original is the slot ebp - 0xc + 4 * axis
            let v = m.u32((ebp - 0x74) as u32);
            m.set_u32((ebp - 0xc + axis * 4) as u32, v);
            let CI = ebp - 0x18 + axis * 4;
            let kind = ri(m, M1 + 0x2ec);
            m.set_f32((ebp - 0x24) as u32, (L!(0xc)).to_f32());
            m.set_f32((ebp - 0x20) as u32, (L!(0x8)).to_f32());
            m.set_f32((ebp - 0x1c) as u32, (L!(0x4)).to_f32());
            // the stack-held inputs of the transforms below: the values before any of them runs
            vl_20_32 = L!(0x20);
            vl_24_32 = L!(0x24);
            vl_5c_44 = L!(0x5c);
            vl_5c_53 = L!(0x5c);
            vl_5c_58 = L!(0x5c);
            vl_60_44 = L!(0x60);
            vl_60_53 = L!(0x60);
            vl_60_58 = L!(0x60);
            if kind == 2 {
                if M2 != 0 {
                    let R0 = ri(m, ri(m, JV + 0x2c4) + 0x2c0);
                    if M2 != R0 {
                        let t1 = ((((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188))) + (L!(0x4) * rf(m, M2 + 0x198))) + rf(m, M2 + 0x1a8));
                        m.set_f32((ebp - 0x24) as u32, (t1).to_f32());
                        let t2 = ((((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c))) + (L!(0x4) * rf(m, M2 + 0x19c))) + rf(m, M2 + 0x1ac));
                        m.set_f32((ebp - 0x20) as u32, (t2).to_f32());
                        let t3 = (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0));
                        m.set_f32((ebp - 0x1c) as u32, (t3).to_f32());
                        m.set_f32((ebp - 0x24) as u32, ((((rf(m, R0 + 0x178) * t1) + (rf(m, R0 + 0x17c) * t2)) + (rf(m, R0 + 0x180) * t3))).to_f32());
                        m.set_f32((ebp - 0x20) as u32, ((((rf(m, R0 + 0x188) * t1) + (rf(m, R0 + 0x18c) * t2)) + (rf(m, R0 + 0x190) * t3))).to_f32());
                        m.set_f32((ebp - 0x1c) as u32, (((t3 * rf(m, R0 + 0x1a0)) + ((t2 * rf(m, R0 + 0x19c)) + (t1 * rf(m, R0 + 0x198))))).to_f32());
                        vl_20_32 = L!(0x20);
                        vl_24_32 = L!(0x24);
                    }
                    m.set_f32((ebp - 0x24) as u32, (((((L!(0x20) * rf(m, R0 + 0x128)) + (L!(0x24) * rf(m, R0 + 0x118))) + (L!(0x1c) * rf(m, R0 + 0x138))) + rf(m, R0 + 0x148))).to_f32());
                    m.set_f32((ebp - 0x20) as u32, (((((vl_24_32 * rf(m, R0 + 0x11c)) + (vl_20_32 * rf(m, R0 + 0x12c))) + (L!(0x1c) * rf(m, R0 + 0x13c))) + rf(m, R0 + 0x14c))).to_f32());
                    m.set_f32((ebp - 0x1c) as u32, (((((vl_20_32 * rf(m, R0 + 0x130)) + (vl_24_32 * rf(m, R0 + 0x120))) + (L!(0x1c) * rf(m, R0 + 0x140))) + rf(m, R0 + 0x150))).to_f32());
                }
            } else if M2 != JV {
                m.set_f32((ebp - 0x24) as u32, ((((rf(m, JV + 0x17c) * (((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac))) + (rf(m, JV + 0x178) * (((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8)))) + (rf(m, JV + 0x180) * (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0))))).to_f32());
                m.set_f32((ebp - 0x20) as u32, ((((rf(m, JV + 0x188) * (((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8))) + (rf(m, JV + 0x18c) * (((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)))) + (rf(m, JV + 0x190) * (((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0))))).to_f32());
                m.set_f32((ebp - 0x1c) as u32, (((((((L!(0x4) * rf(m, M2 + 0x19c)) + ((L!(0xc) * rf(m, M2 + 0x17c)) + (L!(0x8) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)) * rf(m, JV + 0x19c)) + ((((L!(0x4) * rf(m, M2 + 0x198)) + ((L!(0xc) * rf(m, M2 + 0x178)) + (L!(0x8) * rf(m, M2 + 0x188)))) + rf(m, M2 + 0x1a8)) * rf(m, JV + 0x198))) + ((((L!(0x4) * rf(m, M2 + 0x1a0)) + ((L!(0x8) * rf(m, M2 + 0x190)) + (L!(0xc) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) * rf(m, JV + 0x1a0)))).to_f32());
            }
            let d = ((x(m.f32(0x0882_2018)) * L!(0x24)) + (x(m.f32(0x0882_201c)) * L!(0x20))) + (x(m.f32(0x0882_2020)) * L!(0x1c));
            if d.lt(z) {
                m.set_f32((ebp - 0x60) as u32, (L!(0x84)).to_f32());
                m.set_f32((ebp - 0x5c) as u32, (L!(0x88)).to_f32());
                m.set_f32((ebp - 0x58) as u32, (L!(0x8c)).to_f32());
                if axis == 1 {
                    m.set_f32((ebp - 0x5c) as u32, ((((L!(0x74) * L!(0x7c)) * x(m.f64(0x08660a40))) + L!(0x88))).to_f32());
                } else if axis == 0 {
                    m.set_f32((ebp - 0x60) as u32, ((((L!(0x74) * L!(0x78)) * x(m.f64(0x08660a40))) + L!(0x84))).to_f32());
                } else if axis == 2 {
                    m.set_f32((ebp - 0x58) as u32, ((((L!(0x74) * L!(0x80)) * x(m.f64(0x08660a40))) + L!(0x8c))).to_f32());
                    vl_5c_44 = L!(0x5c);
                    vl_60_44 = L!(0x60);
                }
                vl_20_32 = L!(0x20);
                vl_24_32 = L!(0x24);
                vl_5c_44 = L!(0x5c);
                vl_5c_53 = L!(0x5c);
                vl_5c_58 = L!(0x5c);
                vl_60_44 = L!(0x60);
                vl_60_53 = L!(0x60);
                vl_60_58 = L!(0x60);
                if kind == 2 {
                    if M2 != 0 {
                        let R0 = ri(m, ri(m, JV + 0x2c4) + 0x2c0);
                        if M2 != R0 {
                            let t4 = ((((L!(0x58) * rf(m, M2 + 0x198)) + ((L!(0x5c) * rf(m, M2 + 0x188)) + (L!(0x60) * rf(m, M2 + 0x178)))) + rf(m, M2 + 0x1a8)) + rf(m, M2 + 0x1b8));
                            m.set_f32((ebp - 0x60) as u32, (t4).to_f32());
                            let t5 = ((((L!(0x58) * rf(m, M2 + 0x19c)) + ((vl_5c_44 * rf(m, M2 + 0x18c)) + (vl_60_44 * rf(m, M2 + 0x17c)))) + rf(m, M2 + 0x1ac)) + rf(m, M2 + 0x1bc));
                            m.set_f32((ebp - 0x5c) as u32, (t5).to_f32());
                            let t6 = ((((L!(0x58) * rf(m, M2 + 0x1a0)) + ((vl_5c_44 * rf(m, M2 + 0x190)) + (vl_60_44 * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) + rf(m, M2 + 0x1c0));
                            m.set_f32((ebp - 0x58) as u32, (t6).to_f32());
                            let t7 = (t4 - rf(m, R0 + 0x1b8));
                            m.set_f32((ebp - 0x60) as u32, (t7).to_f32());
                            let t8 = (t5 - rf(m, R0 + 0x1bc));
                            m.set_f32((ebp - 0x5c) as u32, (t8).to_f32());
                            let t9 = (t6 - rf(m, R0 + 0x1c0));
                            m.set_f32((ebp - 0x58) as u32, (t9).to_f32());
                            m.set_f32((ebp - 0x60) as u32, ((((rf(m, R0 + 0x178) * t7) + (rf(m, R0 + 0x17c) * t8)) + (rf(m, R0 + 0x180) * t9))).to_f32());
                            m.set_f32((ebp - 0x5c) as u32, ((((rf(m, R0 + 0x188) * t7) + (rf(m, R0 + 0x18c) * t8)) + (rf(m, R0 + 0x190) * t9))).to_f32());
                            m.set_f32((ebp - 0x58) as u32, ((((t8 * rf(m, R0 + 0x19c)) + (t7 * rf(m, R0 + 0x198))) + (t9 * rf(m, R0 + 0x1a0)))).to_f32());
                            vl_5c_53 = L!(0x5c);
                            vl_60_53 = L!(0x60);
                        }
                        let t10 = ((((L!(0x5c) * rf(m, R0 + 0x128)) + (L!(0x60) * rf(m, R0 + 0x118))) + (L!(0x58) * rf(m, R0 + 0x138))) + rf(m, R0 + 0x148));
                        m.set_f32((ebp - 0x60) as u32, (t10).to_f32());
                        let t11 = ((((vl_60_53 * rf(m, R0 + 0x11c)) + (vl_5c_53 * rf(m, R0 + 0x12c))) + (L!(0x58) * rf(m, R0 + 0x13c))) + rf(m, R0 + 0x14c));
                        m.set_f32((ebp - 0x5c) as u32, (t11).to_f32());
                        let t12 = (((L!(0x58) * rf(m, R0 + 0x140)) + ((vl_5c_53 * rf(m, R0 + 0x130)) + (vl_60_53 * rf(m, R0 + 0x120)))) + rf(m, R0 + 0x150));
                        m.set_f32((ebp - 0x58) as u32, (t12).to_f32());
                        m.set_f32((ebp - 0x60) as u32, ((t10 + rf(m, R0 + 0x158))).to_f32());
                        m.set_f32((ebp - 0x5c) as u32, ((t11 + rf(m, R0 + 0x15c))).to_f32());
                        vl_5c_58 = L!(0x5c);
                        vl_60_58 = L!(0x60);
                        phi1 = (t12 + rf(m, R0 + 0x160));
                        m.set_f32((ebp - 0x58) as u32, phi1.to_f32());
                    }
                } else if M2 != JV {
                    m.set_f32((ebp - 0x60) as u32, ((((rf(m, JV + 0x17c) * (((((L!(0x58) * rf(m, M2 + 0x19c)) + ((L!(0x60) * rf(m, M2 + 0x17c)) + (L!(0x5c) * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)) + rf(m, M2 + 0x1bc)) - rf(m, JV + 0x1bc))) + (rf(m, JV + 0x178) * (((((L!(0x58) * rf(m, M2 + 0x198)) + ((L!(0x5c) * rf(m, M2 + 0x188)) + (L!(0x60) * rf(m, M2 + 0x178)))) + rf(m, M2 + 0x1a8)) + rf(m, M2 + 0x1b8)) - rf(m, JV + 0x1b8)))) + (rf(m, JV + 0x180) * (((((L!(0x58) * rf(m, M2 + 0x1a0)) + ((L!(0x5c) * rf(m, M2 + 0x190)) + (L!(0x60) * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) + rf(m, M2 + 0x1c0)) - rf(m, JV + 0x1c0))))).to_f32());
                    m.set_f32((ebp - 0x5c) as u32, ((((rf(m, JV + 0x188) * (((((L!(0x58) * rf(m, M2 + 0x198)) + ((vl_5c_58 * rf(m, M2 + 0x188)) + (vl_60_58 * rf(m, M2 + 0x178)))) + rf(m, M2 + 0x1a8)) + rf(m, M2 + 0x1b8)) - rf(m, JV + 0x1b8))) + (rf(m, JV + 0x18c) * (((((L!(0x58) * rf(m, M2 + 0x19c)) + ((vl_60_58 * rf(m, M2 + 0x17c)) + (vl_5c_58 * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)) + rf(m, M2 + 0x1bc)) - rf(m, JV + 0x1bc)))) + (rf(m, JV + 0x190) * (((((L!(0x58) * rf(m, M2 + 0x1a0)) + ((vl_5c_58 * rf(m, M2 + 0x190)) + (vl_60_58 * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) + rf(m, M2 + 0x1c0)) - rf(m, JV + 0x1c0))))).to_f32());
                    phi1 = ((((((((L!(0x58) * rf(m, M2 + 0x19c)) + ((vl_60_58 * rf(m, M2 + 0x17c)) + (vl_5c_58 * rf(m, M2 + 0x18c)))) + rf(m, M2 + 0x1ac)) + rf(m, M2 + 0x1bc)) - rf(m, JV + 0x1bc)) * rf(m, JV + 0x19c)) + ((((((L!(0x58) * rf(m, M2 + 0x198)) + ((vl_5c_58 * rf(m, M2 + 0x188)) + (vl_60_58 * rf(m, M2 + 0x178)))) + rf(m, M2 + 0x1a8)) + rf(m, M2 + 0x1b8)) - rf(m, JV + 0x1b8)) * rf(m, JV + 0x198))) + ((((((L!(0x58) * rf(m, M2 + 0x1a0)) + ((vl_5c_58 * rf(m, M2 + 0x190)) + (vl_60_58 * rf(m, M2 + 0x180)))) + rf(m, M2 + 0x1b0)) + rf(m, M2 + 0x1c0)) - rf(m, JV + 0x1c0)) * rf(m, JV + 0x1a0)));
                    m.set_f32((ebp - 0x58) as u32, phi1.to_f32());
                }
                let G = ri(m, JV + 0x300);
                let t13 = ((((x(m.f32(0x08822018)) * L!(0x24)) + (x(m.f32(0x0882201c)) * L!(0x20))) + (x(m.f32(0x08822020)) * L!(0x1c))) * rf(m, CI + 0x0));
                m.set_f32((ebp - 0x90) as u32, ((L!(0x5c) * (t13 * L!(0x1c)))).to_f32());
                let t14 = (L!(0x90) - (L!(0x58) * (t13 * L!(0x20))));
                m.set_f32((ebp - 0x90) as u32, (t14).to_f32());
                m.set_f32((ebp - 0x54) as u32, (t14).to_f32());
                let t15 = (t13 * L!(0x1c));
                m.set_f32((ebp - 0x40) as u32, (t15).to_f32());
                let t16 = (t13 * L!(0x24));
                m.set_f32((ebp - 0x48) as u32, (t16).to_f32());
                let t17 = (t13 * L!(0x20));
                m.set_f32((ebp - 0x44) as u32, (t17).to_f32());
                let t18 = ((L!(0x60) * t17) - (L!(0x5c) * t16));
                m.set_f32((ebp - 0x4c) as u32, (t18).to_f32());
                let t19 = ((L!(0x58) * t16) - (L!(0x60) * t15));
                m.set_f32((ebp - 0x50) as u32, (t19).to_f32());
                let t20 = (((rf(m, G + 0x1d8) * t16) + (rf(m, G + 0x1dc) * t17)) + (rf(m, G + 0x1e0) * t15));
                m.set_f32((ebp - 0x48) as u32, (t20).to_f32());
                let t21 = (((rf(m, G + 0x1e4) * t16) + (rf(m, G + 0x1e8) * t17)) + (rf(m, G + 0x1ec) * t15));
                m.set_f32((ebp - 0x44) as u32, (t21).to_f32());
                let t22 = ((t15 * rf(m, G + 0x1f8)) + ((t17 * rf(m, G + 0x1f4)) + (t16 * rf(m, G + 0x1f0))));
                m.set_f32((ebp - 0x40) as u32, (t22).to_f32());
                m.set_f32((ebp - 0x30) as u32, ((t20 + L!(0x30))).to_f32());
                m.set_f32((ebp - 0x2c) as u32, ((t21 + L!(0x2c))).to_f32());
                m.set_f32((ebp - 0x28) as u32, ((t22 + L!(0x28))).to_f32());
                m.set_f32((ebp - 0x3c) as u32, ((L!(0x90) + L!(0x3c))).to_f32());
                m.set_f32((ebp - 0x38) as u32, ((t19 + L!(0x38))).to_f32());
                m.set_f32((ebp - 0x34) as u32, ((t18 + L!(0x34))).to_f32());
            }
            let nx = L!(0x74) + x(m.f64(0x0866_0a50));
            m.set_f32((ebp - 0x74) as u32, nx.to_f32());
            if x(1.0f32).lt(L!(0x74)) || x(m.f32((ebp - 0x74) as u32)).is_nan() {
                break;
            }
        }
        axis = m.i32((ebp - 0x98) as u32);
        if axis > 2 {
            break;
        }
    }
    // scale by the wind amplitude and add to the two result vectors
    let w = x(m.f32(0x0882_2014));
    let (a, b, c, d, e, f) = (w * L!(0x34), w * L!(0x30), w * L!(0x2c), w * L!(0x28), w * L!(0x3c), w * L!(0x38));
    m.set_f32((ebp - 0x34) as u32, a.to_f32());
    m.set_f32((ebp - 0x30) as u32, b.to_f32());
    m.set_f32((ebp - 0x2c) as u32, c.to_f32());
    m.set_f32((ebp - 0x28) as u32, d.to_f32());
    m.set_f32((ebp - 0x3c) as u32, e.to_f32());
    m.set_f32((ebp - 0x38) as u32, f.to_f32());
    m.set_f32(out1 as u32, (b + rf(m, out1)).to_f32());
    m.set_f32((out1 + 4) as u32, (c + rf(m, out1 + 4)).to_f32());
    m.set_f32((out1 + 8) as u32, (d + rf(m, out1 + 8)).to_f32());
    m.set_f32(out2 as u32, (e + rf(m, out2)).to_f32());
    m.set_f32((out2 + 4) as u32, (f + rf(m, out2 + 4)).to_f32());
    m.set_f32((out2 + 8) as u32, (a + rf(m, out2 + 8)).to_f32());
}
