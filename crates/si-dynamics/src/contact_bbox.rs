// SPDX-License-Identifier: MIT
//! contact.c of the dynamics simulation, the bounding-box support (finding 0035).

use crate::contact::{rf, ri};
use si_mem::Mem;
use si_x87::{x, F80};

/// [soft 0x084ab374 si_dynaEvaluateBboxSupport]: the support point of a box-shaped contact of a joint-driven body; the two
/// vectors are passed by value (six float bit patterns here, the stack arguments at ebp+0x18..0x2c). The rest is
/// described in the finding.
pub fn si_dyna_evaluate_bbox_support(m: &mut Mem, mdl1: u32, mdl2: u32, jv: u32, ci: u32, pa: [u32; 3], pb: [u32; 3]) {
    let fr = m.local(0xcc + 0x30);
    let ebp = fr + 0xcc;
    for (i, w) in pa.iter().chain(pb.iter()).enumerate() {
        m.set_u32(ebp + 0x18 + 4 * i as u32, *w);
    }
    bbox_support_body(m, ebp as i32, mdl1 as i32, mdl2 as i32, jv as i32, ci as i32);
    m.release(0xcc + 0x30);
}

/// The body of si_dynaEvaluateBboxSupport: control flow written from the listing, arithmetic expanded from the
/// symbolic x87 order (re/tools/si-dynamics/x87tpl.py, X87TPL_BYVAL=1: the six vector floats are the stack
/// arguments at ebp+0x18..0x2c); `ebp` is the frame base, slots are ebp-0xNN.
#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::all)]
fn bbox_support_body(m: &mut Mem, ebp: i32, M1: i32, M2: i32, JV: i32, CI: i32) {
    macro_rules! L {
        ($o:expr) => {
            x(m.f32((ebp - $o) as u32))
        };
    }
    fn neg(a: F80) -> F80 {
        -a
    }
    fn abs(a: F80) -> F80 {
        a.abs()
    }
    let z = x(0.0f32);
    let mut vl_18_21 = x(0.0f32);
    let mut vl_2c_53 = x(0.0f32);
    let mut vl_30_53 = x(0.0f32);
    let mut vl_38_65 = x(0.0f32);
    let mut vl_3c_65 = x(0.0f32);
    let mut vl_50_94 = x(0.0f32);
    let mut vl_54_94 = x(0.0f32);
    let mut vl_5c_130 = x(0.0f32);
    let mut vl_60_130 = x(0.0f32);
    let mut vl_8_16 = x(0.0f32);
    let mut vl_ac_97 = x(0.0f32);
    let mut vl_ac_133 = x(0.0f32);
    let mut vl_ac_135 = x(0.0f32);
    let mut vl_c_16 = x(0.0f32);
    let mut vr0_30_108 = x(0.0f32);
    let mut vr0_30_114 = x(0.0f32);
    let mut vr0_34_108 = x(0.0f32);
    let (mut phi1, mut phi2, mut phi3, mut phi4) = (z, z, z, z);
    let (mut phi14, mut phi15, mut phi16, mut phi17, mut phi18) = (z, z, z, z, z);
    let (mut phi23, mut phi24, mut phi25) = (z, z, z);
    let (mut phi26, mut phi27, mut phi28, mut phi29, mut phi30, mut phi31, mut phi32) = (z, z, z, z, z, z, z);
    let (mut phi33, mut phi34, mut phi35, mut phi36, mut phi37) = (z, z, z, z, z);
    let (mut phi38, mut phi39, mut phi40) = (z, z, z);
    let kind = ri(m, M1 + 0x2ec);
    let G = ri(m, CI + 0x10);
    let R4 = ri(m, M2 + 0x2f4);
    let R3 = ri(m, M2 + 0x304);
    m.set_f32((ebp - 0xbc) as u32, (rf(m, R4 + 0xc)).to_f32());
    m.set_f32((ebp - 0xc0) as u32, (rf(m, R4 + 0x10)).to_f32());
    m.set_f32((ebp - 0xc4) as u32, ((x(m.f64(0x08660c90)) * L!(0xc0))).to_f32());
    let src = if R3 == 0 { R4 + 0x20 } else { R3 + 0x64 };
    let w0 = ri(m, src);
    m.set_i32((CI + 0x20) as u32, w0);
    let w1 = ri(m, src + 4);
    m.set_i32((CI + 0x24) as u32, w1);
    let w2 = ri(m, src + 8);
    m.set_i32((CI + 0x28) as u32, w2);
    if kind == 2 {
        let t3 = ((((rf(m, ebp + 0x1c) * rf(m, M1 + 0x188)) + (rf(m, ebp + 0x18) * rf(m, M1 + 0x178))) + (rf(m, ebp + 0x20) * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
        m.set_f32((ebp - 0xc) as u32, (t3).to_f32());
        let t4 = ((((rf(m, ebp + 0x18) * rf(m, M1 + 0x17c)) + (rf(m, ebp + 0x1c) * rf(m, M1 + 0x18c))) + (rf(m, ebp + 0x20) * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
        m.set_f32((ebp - 0x8) as u32, (t4).to_f32());
        phi1 = t4;
        phi2 = t3;
        phi3 = (((rf(m, ebp + 0x20) * rf(m, M1 + 0x1a0)) + ((rf(m, ebp + 0x1c) * rf(m, M1 + 0x190)) + (rf(m, ebp + 0x18) * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
        m.set_f32((ebp - 0x4) as u32, (phi3).to_f32());
        m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x178) * phi2) + (rf(m, G + 0x17c) * phi1)) + (rf(m, G + 0x180) * phi3))).to_f32());
        m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * phi2) + (rf(m, G + 0x18c) * phi1)) + (rf(m, G + 0x190) * phi3))).to_f32());
        phi4 = ((phi3 * rf(m, G + 0x1a0)) + ((phi1 * rf(m, G + 0x19c)) + (phi2 * rf(m, G + 0x198))));
        m.set_f32((ebp - 0x4) as u32, (phi4).to_f32());
        vl_8_16 = L!(0x8);
        vl_c_16 = L!(0xc);
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x17c) * rf(m, ebp + 0x1c)) + (rf(m, G + 0x178) * rf(m, ebp + 0x18))) + (rf(m, G + 0x180) * rf(m, ebp + 0x20)))).to_f32());
            m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * rf(m, ebp + 0x18)) + (rf(m, G + 0x18c) * rf(m, ebp + 0x1c))) + (rf(m, G + 0x190) * rf(m, ebp + 0x20)))).to_f32());
            phi4 = (((rf(m, ebp + 0x1c) * rf(m, G + 0x19c)) + (rf(m, ebp + 0x18) * rf(m, G + 0x198))) + (rf(m, ebp + 0x20) * rf(m, G + 0x1a0)));
            m.set_f32((ebp - 0x4) as u32, (phi4).to_f32());
            vl_8_16 = L!(0x8);
            vl_c_16 = L!(0xc);
        } else {
            let t1 = ((((rf(m, ebp + 0x1c) * rf(m, B + 0x188)) + (rf(m, ebp + 0x18) * rf(m, B + 0x178))) + (rf(m, ebp + 0x20) * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
            m.set_f32((ebp - 0xc) as u32, (t1).to_f32());
            let t2 = ((((rf(m, ebp + 0x18) * rf(m, B + 0x17c)) + (rf(m, ebp + 0x1c) * rf(m, B + 0x18c))) + (rf(m, ebp + 0x20) * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
            m.set_f32((ebp - 0x8) as u32, (t2).to_f32());
            phi1 = t2;
            phi2 = t1;
            phi3 = (((rf(m, ebp + 0x20) * rf(m, B + 0x1a0)) + ((rf(m, ebp + 0x1c) * rf(m, B + 0x190)) + (rf(m, ebp + 0x18) * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
            m.set_f32((ebp - 0x4) as u32, (phi3).to_f32());
            m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x178) * phi2) + (rf(m, G + 0x17c) * phi1)) + (rf(m, G + 0x180) * phi3))).to_f32());
            m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * phi2) + (rf(m, G + 0x18c) * phi1)) + (rf(m, G + 0x190) * phi3))).to_f32());
            phi4 = ((phi3 * rf(m, G + 0x1a0)) + ((phi1 * rf(m, G + 0x19c)) + (phi2 * rf(m, G + 0x198))));
            m.set_f32((ebp - 0x4) as u32, (phi4).to_f32());
            vl_8_16 = L!(0x8);
            vl_c_16 = L!(0xc);
        }
    }
    let H = ri(m, CI + 0x38);
    let t5 = (((rf(m, H + 0x38) * L!(0x8)) + (rf(m, H + 0x34) * L!(0xc))) + (rf(m, H + 0x3c) * L!(0x4)));
    m.set_f32((ebp - 0xc) as u32, (t5).to_f32());
    let t6 = (((rf(m, H + 0x44) * vl_c_16) + (rf(m, H + 0x48) * vl_8_16)) + (rf(m, H + 0x4c) * L!(0x4)));
    m.set_f32((ebp - 0x8) as u32, (t6).to_f32());
    m.set_f32((ebp - 0x4) as u32, (((L!(0x4) * rf(m, H + 0x5c)) + ((vl_8_16 * rf(m, H + 0x58)) + (vl_c_16 * rf(m, H + 0x54))))).to_f32());
    if t6.lt(z) {
        let bc = L!(0xbc);
        let c0 = L!(0xc0);
        let m1 = neg(c0) * t6;
        let v18 = if abs(t5).le(neg(t6) * bc) {
            neg(t5)
        } else if t5.gt(z) {
            neg(if m1.lt(t5) { m1 } else { t5 })
        } else if neg(t5).gt(m1) {
            m1
        } else {
            neg(t5)
        };
        m.set_f32((ebp - 0x18) as u32, v18.to_f32());
        let l4 = L!(0x4);
        let l8 = L!(0x8);
        let v10 = if abs(l4).le(neg(l8) * bc) {
            neg(l4)
        } else if l4.gt(z) {
            (l8 * c0) + l4
        } else {
            l4 - (l8 * c0)
        };
        m.set_f32((ebp - 0x10) as u32, v10.to_f32());
        vl_18_21 = L!(0x18);
        let t7 = neg(L!(0x8));
        m.set_f32((ebp - 0x14) as u32, (t7).to_f32());
        let t8 = ((((t7 * rf(m, H + 0x44)) + (vl_18_21 * rf(m, H + 0x34))) + (L!(0x10) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x18) as u32, (t8).to_f32());
        let t9 = ((((vl_18_21 * rf(m, H + 0x38)) + (t7 * rf(m, H + 0x48))) + (L!(0x10) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x14) as u32, (t9).to_f32());
        let t10 = (((L!(0x10) * rf(m, H + 0x5c)) + ((t7 * rf(m, H + 0x4c)) + (vl_18_21 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x10) as u32, (t10).to_f32());
        if kind == 2 {
            let t14 = ((((t8 * rf(m, G + 0x178)) + (t9 * rf(m, G + 0x188))) + (t10 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((CI + 0x14) as u32, (t14).to_f32());
            let t15 = ((((t8 * rf(m, G + 0x17c)) + (t9 * rf(m, G + 0x18c))) + (t10 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((CI + 0x18) as u32, (t15).to_f32());
            let t16 = (((t10 * rf(m, G + 0x1a0)) + ((t9 * rf(m, G + 0x190)) + (t8 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
            m.set_f32((CI + 0x1c) as u32, (t16).to_f32());
            m.set_f32((CI + 0x14) as u32, ((((rf(m, M1 + 0x178) * t14) + (rf(m, M1 + 0x17c) * t15)) + (rf(m, M1 + 0x180) * t16))).to_f32());
            m.set_f32((CI + 0x18) as u32, ((((rf(m, M1 + 0x188) * t14) + (rf(m, M1 + 0x18c) * t15)) + (rf(m, M1 + 0x190) * t16))).to_f32());
            m.set_f32((CI + 0x1c) as u32, ((((t15 * rf(m, M1 + 0x19c)) + (t14 * rf(m, M1 + 0x198))) + (t16 * rf(m, M1 + 0x1a0)))).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((CI + 0x14) as u32, (((((t8 * rf(m, G + 0x178)) + (t9 * rf(m, G + 0x188))) + (t10 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((CI + 0x18) as u32, (((((t9 * rf(m, G + 0x18c)) + (t8 * rf(m, G + 0x17c))) + (t10 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((CI + 0x1c) as u32, ((((t10 * rf(m, G + 0x1a0)) + ((t9 * rf(m, G + 0x190)) + (t8 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
            } else {
                let t11 = ((((t8 * rf(m, G + 0x178)) + (t9 * rf(m, G + 0x188))) + (t10 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
                m.set_f32((CI + 0x14) as u32, (t11).to_f32());
                let t12 = ((((t8 * rf(m, G + 0x17c)) + (t9 * rf(m, G + 0x18c))) + (t10 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
                m.set_f32((CI + 0x18) as u32, (t12).to_f32());
                let t13 = (((t10 * rf(m, G + 0x1a0)) + ((t9 * rf(m, G + 0x190)) + (t8 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
                m.set_f32((CI + 0x1c) as u32, (t13).to_f32());
                m.set_f32((CI + 0x14) as u32, ((((rf(m, B + 0x178) * t11) + (rf(m, B + 0x17c) * t12)) + (rf(m, B + 0x180) * t13))).to_f32());
                m.set_f32((CI + 0x18) as u32, ((((rf(m, B + 0x188) * t11) + (rf(m, B + 0x18c) * t12)) + (rf(m, B + 0x190) * t13))).to_f32());
                m.set_f32((CI + 0x1c) as u32, ((((t12 * rf(m, B + 0x19c)) + (t11 * rf(m, B + 0x198))) + (t13 * rf(m, B + 0x1a0)))).to_f32());
            }
        }
    }
    let t17 = (((rf(m, JV + 0x1dc) * rf(m, ebp + 0x28)) + (rf(m, JV + 0x1d8) * rf(m, ebp + 0x24))) + (rf(m, JV + 0x1e0) * rf(m, ebp + 0x2c)));
    m.set_f32((ebp - 0x24) as u32, (t17).to_f32());
    let t18 = (((rf(m, JV + 0x1e4) * rf(m, ebp + 0x24)) + (rf(m, JV + 0x1e8) * rf(m, ebp + 0x28))) + (rf(m, JV + 0x1ec) * rf(m, ebp + 0x2c)));
    m.set_f32((ebp - 0x20) as u32, (t18).to_f32());
    let t19 = ((rf(m, ebp + 0x2c) * rf(m, JV + 0x1f8)) + ((rf(m, ebp + 0x28) * rf(m, JV + 0x1f4)) + (rf(m, ebp + 0x24) * rf(m, JV + 0x1f0))));
    m.set_f32((ebp - 0x1c) as u32, (t19).to_f32());
    if kind == 2 {
        let t22 = ((((t17 * rf(m, M1 + 0x178)) + (t18 * rf(m, M1 + 0x188))) + (t19 * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
        m.set_f32((ebp - 0x30) as u32, (t22).to_f32());
        let t23 = ((((t17 * rf(m, M1 + 0x17c)) + (t18 * rf(m, M1 + 0x18c))) + (t19 * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
        m.set_f32((ebp - 0x2c) as u32, (t23).to_f32());
        phi14 = t23;
        phi15 = t22;
        phi16 = (((t19 * rf(m, M1 + 0x1a0)) + ((t18 * rf(m, M1 + 0x190)) + (t17 * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
        m.set_f32((ebp - 0x28) as u32, (phi16).to_f32());
        m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * phi15) + (rf(m, G + 0x17c) * phi14)) + (rf(m, G + 0x180) * phi16))).to_f32());
        m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * phi15) + (rf(m, G + 0x18c) * phi14)) + (rf(m, G + 0x190) * phi16))).to_f32());
        phi17 = (phi16 * rf(m, G + 0x1a0));
        phi18 = ((phi14 * rf(m, G + 0x19c)) + (phi15 * rf(m, G + 0x198)));
        m.set_f32((ebp - 0x28) as u32, ((phi17 + phi18)).to_f32());
        vl_2c_53 = L!(0x2c);
        vl_30_53 = L!(0x30);
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * t17) + (rf(m, G + 0x17c) * t18)) + (rf(m, G + 0x180) * t19))).to_f32());
            m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * t17) + (rf(m, G + 0x18c) * t18)) + (rf(m, G + 0x190) * t19))).to_f32());
            phi17 = (t19 * rf(m, G + 0x1a0));
            phi18 = ((t18 * rf(m, G + 0x19c)) + (t17 * rf(m, G + 0x198)));
            m.set_f32((ebp - 0x28) as u32, ((phi17 + phi18)).to_f32());
            vl_2c_53 = L!(0x2c);
            vl_30_53 = L!(0x30);
        } else {
            let t20 = ((((t17 * rf(m, B + 0x178)) + (t18 * rf(m, B + 0x188))) + (t19 * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
            m.set_f32((ebp - 0x30) as u32, (t20).to_f32());
            let t21 = ((((t17 * rf(m, B + 0x17c)) + (t18 * rf(m, B + 0x18c))) + (t19 * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
            m.set_f32((ebp - 0x2c) as u32, (t21).to_f32());
            phi14 = t21;
            phi15 = t20;
            phi16 = (((t19 * rf(m, B + 0x1a0)) + ((t18 * rf(m, B + 0x190)) + (t17 * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
            m.set_f32((ebp - 0x28) as u32, (phi16).to_f32());
            m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * phi15) + (rf(m, G + 0x17c) * phi14)) + (rf(m, G + 0x180) * phi16))).to_f32());
            m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * phi15) + (rf(m, G + 0x18c) * phi14)) + (rf(m, G + 0x190) * phi16))).to_f32());
            phi17 = (phi16 * rf(m, G + 0x1a0));
            phi18 = ((phi14 * rf(m, G + 0x19c)) + (phi15 * rf(m, G + 0x198)));
            m.set_f32((ebp - 0x28) as u32, ((phi17 + phi18)).to_f32());
            vl_2c_53 = L!(0x2c);
            vl_30_53 = L!(0x30);
        }
    }
    m.set_f32((ebp - 0x30) as u32, ((((rf(m, H + 0x38) * L!(0x2c)) + (rf(m, H + 0x34) * L!(0x30))) + (rf(m, H + 0x3c) * L!(0x28)))).to_f32());
    let t24 = (((rf(m, H + 0x44) * vl_30_53) + (rf(m, H + 0x48) * vl_2c_53)) + (rf(m, H + 0x4c) * L!(0x28)));
    m.set_f32((ebp - 0x2c) as u32, (t24).to_f32());
    m.set_f32((ebp - 0x28) as u32, (((L!(0x28) * rf(m, H + 0x5c)) + ((vl_2c_53 * rf(m, H + 0x58)) + (vl_30_53 * rf(m, H + 0x54))))).to_f32());
    if z.gt(L!(0x8)) {
        m.set_i32((ebp - 0x34) as u32, 0x0);
        m.set_i32((ebp - 0x38) as u32, 0x0);
        m.set_i32((ebp - 0x3c) as u32, 0x0);
        let n = neg(L!(0x8)) * L!(0xbc);
        let phi20;
        if abs(t24).le(n) {
            phi20 = neg(t24);
        } else if t24.gt(z) {
            let t25 = L!(0x8) * L!(0xc0);
            m.set_f32((ebp - 0xc0) as u32, t25.to_f32());
            phi20 = t25;
        } else {
            let t26 = L!(0x8) * L!(0xc0);
            m.set_f32((ebp - 0xc0) as u32, t26.to_f32());
            phi20 = neg(t26);
        }
        m.set_f32((ebp - 0x38) as u32, phi20.to_f32());
        let xx = L!(0x38);
        if z.lt(xx) {
            let a = abs(L!(0x2c));
            if a.le(xx) {
                m.set_f32((ebp - 0xac) as u32, a.to_f32());
                m.set_f32((ebp - 0x38) as u32, a.to_f32());
            } else {
                m.set_f32((ebp - 0x38) as u32, xx.to_f32());
            }
        } else {
            let a = abs(L!(0x2c));
            let phi22 = if a.le(neg(xx)) { a } else { neg(xx) };
            m.set_f32((ebp - 0x38) as u32, neg(phi22).to_f32());
        }
        vl_38_65 = L!(0x38);
        vl_3c_65 = L!(0x3c);
        let t27 = ((((L!(0x38) * rf(m, H + 0x44)) + (L!(0x3c) * rf(m, H + 0x34))) + (L!(0x34) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x3c) as u32, (t27).to_f32());
        let t28 = ((((vl_3c_65 * rf(m, H + 0x38)) + (vl_38_65 * rf(m, H + 0x48))) + (L!(0x34) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x38) as u32, (t28).to_f32());
        let t29 = (((L!(0x34) * rf(m, H + 0x5c)) + ((vl_38_65 * rf(m, H + 0x4c)) + (vl_3c_65 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x34) as u32, (t29).to_f32());
        if kind == 2 {
            let t33 = ((((t27 * rf(m, G + 0x178)) + (t28 * rf(m, G + 0x188))) + (t29 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((ebp - 0x3c) as u32, (t33).to_f32());
            let t34 = ((((t27 * rf(m, G + 0x17c)) + (t28 * rf(m, G + 0x18c))) + (t29 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((ebp - 0x38) as u32, (t34).to_f32());
            m.set_f32((ebp - 0x3c) as u32, ((((rf(m, M1 + 0x17c) * t34) + (rf(m, M1 + 0x178) * t33)) + (rf(m, M1 + 0x180) * ((((t28 * rf(m, G + 0x190)) + (t27 * rf(m, G + 0x180))) + (t29 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            m.set_f32((ebp - 0x38) as u32, ((((rf(m, M1 + 0x188) * t33) + (rf(m, M1 + 0x18c) * t34)) + (rf(m, M1 + 0x190) * ((((t28 * rf(m, G + 0x190)) + (t27 * rf(m, G + 0x180))) + (t29 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            phi23 = (t34 * rf(m, M1 + 0x19c));
            phi24 = (t33 * rf(m, M1 + 0x198));
            phi25 = (((((t28 * rf(m, G + 0x190)) + (t27 * rf(m, G + 0x180))) + (t29 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0)) * rf(m, M1 + 0x1a0));
            m.set_f32((ebp - 0x34) as u32, (((phi23 + phi24) + phi25)).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x3c) as u32, (((((t27 * rf(m, G + 0x178)) + (t28 * rf(m, G + 0x188))) + (t29 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((ebp - 0x38) as u32, (((((t28 * rf(m, G + 0x18c)) + (t27 * rf(m, G + 0x17c))) + (t29 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((ebp - 0x34) as u32, ((((t29 * rf(m, G + 0x1a0)) + ((t28 * rf(m, G + 0x190)) + (t27 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
            } else {
                let t30 = ((((t27 * rf(m, G + 0x178)) + (t28 * rf(m, G + 0x188))) + (t29 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
                m.set_f32((ebp - 0x3c) as u32, (t30).to_f32());
                let t31 = ((((t27 * rf(m, G + 0x17c)) + (t28 * rf(m, G + 0x18c))) + (t29 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
                m.set_f32((ebp - 0x38) as u32, (t31).to_f32());
                let t32 = (((t29 * rf(m, G + 0x1a0)) + ((t28 * rf(m, G + 0x190)) + (t27 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
                m.set_f32((ebp - 0x34) as u32, (t32).to_f32());
                m.set_f32((ebp - 0x3c) as u32, ((((rf(m, B + 0x178) * t30) + (rf(m, B + 0x17c) * t31)) + (rf(m, B + 0x180) * t32))).to_f32());
                m.set_f32((ebp - 0x38) as u32, ((((rf(m, B + 0x188) * t30) + (rf(m, B + 0x18c) * t31)) + (rf(m, B + 0x190) * t32))).to_f32());
                phi23 = (t31 * rf(m, B + 0x19c));
                phi24 = (t30 * rf(m, B + 0x198));
                phi25 = (t32 * rf(m, B + 0x1a0));
                m.set_f32((ebp - 0x34) as u32, (((phi23 + phi24) + phi25)).to_f32());
            }
        }
        m.set_f32((CI + 0x2c) as u32, ((((rf(m, JV + 0x224) * L!(0x38)) + (rf(m, JV + 0x220) * L!(0x3c))) + (rf(m, JV + 0x228) * L!(0x34)))).to_f32());
        m.set_f32((CI + 0x30) as u32, ((((rf(m, JV + 0x22c) * L!(0x3c)) + (rf(m, JV + 0x230) * L!(0x38))) + (rf(m, JV + 0x234) * L!(0x34)))).to_f32());
        m.set_f32((CI + 0x34) as u32, ((((L!(0x38) * rf(m, JV + 0x23c)) + (L!(0x3c) * rf(m, JV + 0x238))) + (L!(0x34) * rf(m, JV + 0x240)))).to_f32());
    }
    let eps = x(m.f64(0x08660c98));
    if m.u16(0x086d7140) != 0 && abs(L!(0x18)).lt(eps) && abs(L!(0x10)).lt(eps) {
        let R0 = ri(m, M2 + 0x2f0);
        if kind == 2 {
            let t37 = ((((rf(m, R0 + 0x34) * rf(m, M1 + 0x188)) + (rf(m, R0 + 0x30) * rf(m, M1 + 0x178))) + (rf(m, R0 + 0x38) * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
            m.set_f32((ebp - 0x54) as u32, (t37).to_f32());
            let t38 = ((((rf(m, R0 + 0x30) * rf(m, M1 + 0x17c)) + (rf(m, R0 + 0x34) * rf(m, M1 + 0x18c))) + (rf(m, R0 + 0x38) * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
            m.set_f32((ebp - 0x50) as u32, (t38).to_f32());
            phi26 = t38;
            phi27 = t37;
            phi28 = (((rf(m, R0 + 0x38) * rf(m, M1 + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, M1 + 0x190)) + (rf(m, R0 + 0x30) * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
            m.set_f32((ebp - 0x4c) as u32, (phi28).to_f32());
            m.set_f32((ebp - 0x54) as u32, ((((rf(m, G + 0x178) * phi27) + (rf(m, G + 0x17c) * phi26)) + (rf(m, G + 0x180) * phi28))).to_f32());
            m.set_f32((ebp - 0x50) as u32, ((((rf(m, G + 0x188) * phi27) + (rf(m, G + 0x18c) * phi26)) + (rf(m, G + 0x190) * phi28))).to_f32());
            phi29 = phi26;
            phi30 = phi28;
            phi31 = phi27;
            m.set_f32((ebp - 0x4c) as u32, (((phi30 * rf(m, G + 0x1a0)) + ((phi29 * rf(m, G + 0x19c)) + (phi31 * rf(m, G + 0x198))))).to_f32());
            vl_50_94 = L!(0x50);
            vl_54_94 = L!(0x54);
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x54) as u32, ((((rf(m, G + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, G + 0x178) * rf(m, R0 + 0x30))) + (rf(m, G + 0x180) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((ebp - 0x50) as u32, ((((rf(m, G + 0x188) * rf(m, R0 + 0x30)) + (rf(m, G + 0x18c) * rf(m, R0 + 0x34))) + (rf(m, G + 0x190) * rf(m, R0 + 0x38)))).to_f32());
                phi29 = rf(m, R0 + 0x34);
                phi30 = rf(m, R0 + 0x38);
                phi31 = rf(m, R0 + 0x30);
                m.set_f32((ebp - 0x4c) as u32, (((phi30 * rf(m, G + 0x1a0)) + ((phi29 * rf(m, G + 0x19c)) + (phi31 * rf(m, G + 0x198))))).to_f32());
                vl_50_94 = L!(0x50);
                vl_54_94 = L!(0x54);
            } else {
                let t35 = ((((rf(m, R0 + 0x34) * rf(m, B + 0x188)) + (rf(m, R0 + 0x30) * rf(m, B + 0x178))) + (rf(m, R0 + 0x38) * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
                m.set_f32((ebp - 0x54) as u32, (t35).to_f32());
                let t36 = ((((rf(m, R0 + 0x30) * rf(m, B + 0x17c)) + (rf(m, R0 + 0x34) * rf(m, B + 0x18c))) + (rf(m, R0 + 0x38) * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
                m.set_f32((ebp - 0x50) as u32, (t36).to_f32());
                phi26 = t36;
                phi27 = t35;
                phi28 = (((rf(m, R0 + 0x38) * rf(m, B + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, B + 0x190)) + (rf(m, R0 + 0x30) * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
                m.set_f32((ebp - 0x4c) as u32, (phi28).to_f32());
                m.set_f32((ebp - 0x54) as u32, ((((rf(m, G + 0x178) * phi27) + (rf(m, G + 0x17c) * phi26)) + (rf(m, G + 0x180) * phi28))).to_f32());
                m.set_f32((ebp - 0x50) as u32, ((((rf(m, G + 0x188) * phi27) + (rf(m, G + 0x18c) * phi26)) + (rf(m, G + 0x190) * phi28))).to_f32());
                phi29 = phi26;
                phi30 = phi28;
                phi31 = phi27;
                m.set_f32((ebp - 0x4c) as u32, (((phi30 * rf(m, G + 0x1a0)) + ((phi29 * rf(m, G + 0x19c)) + (phi31 * rf(m, G + 0x198))))).to_f32());
                vl_50_94 = L!(0x50);
                vl_54_94 = L!(0x54);
            }
        }
        let t39 = (((rf(m, H + 0x38) * L!(0x50)) + (rf(m, H + 0x34) * L!(0x54))) + (rf(m, H + 0x3c) * L!(0x4c)));
        m.set_f32((ebp - 0x54) as u32, (t39).to_f32());
        let t40 = (((rf(m, H + 0x44) * vl_54_94) + (rf(m, H + 0x48) * vl_50_94)) + (rf(m, H + 0x4c) * L!(0x4c)));
        m.set_f32((ebp - 0x50) as u32, (t40).to_f32());
        let t41 = (x(1.0f32) - L!(0xc4));
        m.set_f32((ebp - 0xac) as u32, ((t39 * t41)).to_f32());
        vl_ac_97 = L!(0xac);
        m.set_f32((ebp - 0xac) as u32, ((((L!(0x4c) * rf(m, H + 0x5c)) + ((vl_50_94 * rf(m, H + 0x58)) + (vl_54_94 * rf(m, H + 0x54)))) * t41)).to_f32());
        m.set_f32((ebp - 0x4c) as u32, (L!(0xac)).to_f32());
        m.set_f32((ebp - 0x54) as u32, (vl_ac_97).to_f32());
        let t42 = ((((t40 * rf(m, H + 0x44)) + (vl_ac_97 * rf(m, H + 0x34))) + (L!(0xac) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x54) as u32, (t42).to_f32());
        let t43 = ((((vl_ac_97 * rf(m, H + 0x38)) + (t40 * rf(m, H + 0x48))) + (L!(0xac) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x50) as u32, (t43).to_f32());
        let t44 = (((L!(0xac) * rf(m, H + 0x5c)) + ((t40 * rf(m, H + 0x4c)) + (vl_ac_97 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x4c) as u32, (t44).to_f32());
        if kind == 2 {
            m.set_f32((R0 + 0x30) as u32, (((((t42 * rf(m, G + 0x178)) + (t43 * rf(m, G + 0x188))) + (t44 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
            m.set_f32((R0 + 0x34) as u32, (((((t43 * rf(m, G + 0x18c)) + (t42 * rf(m, G + 0x17c))) + (t44 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
            m.set_f32((R0 + 0x38) as u32, (((((t43 * rf(m, G + 0x190)) + (t42 * rf(m, G + 0x180))) + (t44 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))).to_f32());
            vr0_30_114 = rf(m, R0 + 0x30);
            m.set_f32((R0 + 0x30) as u32, ((((rf(m, M1 + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, M1 + 0x178) * rf(m, R0 + 0x30))) + (rf(m, M1 + 0x180) * rf(m, R0 + 0x38)))).to_f32());
            m.set_f32((R0 + 0x34) as u32, ((((rf(m, M1 + 0x188) * vr0_30_114) + (rf(m, M1 + 0x18c) * rf(m, R0 + 0x34))) + (rf(m, M1 + 0x190) * rf(m, R0 + 0x38)))).to_f32());
            phi32 = ((rf(m, R0 + 0x38) * rf(m, M1 + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, M1 + 0x19c)) + (vr0_30_114 * rf(m, M1 + 0x198))));
            m.set_f32((R0 + 0x38) as u32, (phi32).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((R0 + 0x30) as u32, (((((t42 * rf(m, G + 0x178)) + (t43 * rf(m, G + 0x188))) + (t44 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((R0 + 0x34) as u32, (((((t43 * rf(m, G + 0x18c)) + (t42 * rf(m, G + 0x17c))) + (t44 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                phi32 = ((((t43 * rf(m, G + 0x190)) + (t42 * rf(m, G + 0x180))) + (t44 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0));
                m.set_f32((R0 + 0x38) as u32, (phi32).to_f32());
            } else {
                m.set_f32((R0 + 0x30) as u32, (((((t42 * rf(m, G + 0x178)) + (t43 * rf(m, G + 0x188))) + (t44 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((R0 + 0x34) as u32, (((((t43 * rf(m, G + 0x18c)) + (t42 * rf(m, G + 0x17c))) + (t44 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((R0 + 0x38) as u32, (((((t43 * rf(m, G + 0x190)) + (t42 * rf(m, G + 0x180))) + (t44 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))).to_f32());
                vr0_30_108 = rf(m, R0 + 0x30);
                vr0_34_108 = rf(m, R0 + 0x34);
                m.set_f32((R0 + 0x30) as u32, ((((rf(m, B + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, B + 0x178) * rf(m, R0 + 0x30))) + (rf(m, B + 0x180) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((R0 + 0x34) as u32, ((((rf(m, B + 0x188) * vr0_30_108) + (rf(m, B + 0x18c) * vr0_34_108)) + (rf(m, B + 0x190) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((R0 + 0x38) as u32, ((((vr0_34_108 * rf(m, B + 0x19c)) + (vr0_30_108 * rf(m, B + 0x198))) + (rf(m, R0 + 0x38) * rf(m, B + 0x1a0)))).to_f32());
            }
        }
        let t45 = (((rf(m, JV + 0x1dc) * rf(m, R0 + 0x4c)) + (rf(m, JV + 0x1d8) * rf(m, R0 + 0x48))) + (rf(m, JV + 0x1e0) * rf(m, R0 + 0x50)));
        m.set_f32((ebp - 0x60) as u32, (t45).to_f32());
        let t46 = (((rf(m, JV + 0x1e4) * rf(m, R0 + 0x48)) + (rf(m, JV + 0x1e8) * rf(m, R0 + 0x4c))) + (rf(m, JV + 0x1ec) * rf(m, R0 + 0x50)));
        m.set_f32((ebp - 0x5c) as u32, (t46).to_f32());
        let t47 = ((rf(m, R0 + 0x50) * rf(m, JV + 0x1f8)) + ((rf(m, R0 + 0x4c) * rf(m, JV + 0x1f4)) + (rf(m, R0 + 0x48) * rf(m, JV + 0x1f0))));
        m.set_f32((ebp - 0x58) as u32, (t47).to_f32());
        if kind == 2 {
            let t50 = ((((t45 * rf(m, M1 + 0x178)) + (t46 * rf(m, M1 + 0x188))) + (t47 * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
            m.set_f32((ebp - 0x60) as u32, (t50).to_f32());
            let t51 = ((((t45 * rf(m, M1 + 0x17c)) + (t46 * rf(m, M1 + 0x18c))) + (t47 * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
            m.set_f32((ebp - 0x5c) as u32, (t51).to_f32());
            phi33 = t50;
            phi34 = t51;
            phi35 = (((t47 * rf(m, M1 + 0x1a0)) + ((t46 * rf(m, M1 + 0x190)) + (t45 * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
            m.set_f32((ebp - 0x58) as u32, (phi35).to_f32());
            m.set_f32((ebp - 0x60) as u32, ((((rf(m, G + 0x178) * phi33) + (rf(m, G + 0x17c) * phi34)) + (rf(m, G + 0x180) * phi35))).to_f32());
            m.set_f32((ebp - 0x5c) as u32, ((((rf(m, G + 0x188) * phi33) + (rf(m, G + 0x18c) * phi34)) + (rf(m, G + 0x190) * phi35))).to_f32());
            phi36 = ((phi34 * rf(m, G + 0x19c)) + (phi33 * rf(m, G + 0x198)));
            phi37 = (phi35 * rf(m, G + 0x1a0));
            m.set_f32((ebp - 0x58) as u32, ((phi36 + phi37)).to_f32());
            vl_5c_130 = L!(0x5c);
            vl_60_130 = L!(0x60);
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x60) as u32, ((((rf(m, G + 0x178) * t45) + (rf(m, G + 0x17c) * t46)) + (rf(m, G + 0x180) * t47))).to_f32());
                m.set_f32((ebp - 0x5c) as u32, ((((rf(m, G + 0x188) * t45) + (rf(m, G + 0x18c) * t46)) + (rf(m, G + 0x190) * t47))).to_f32());
                phi36 = ((t46 * rf(m, G + 0x19c)) + (t45 * rf(m, G + 0x198)));
                phi37 = (t47 * rf(m, G + 0x1a0));
                m.set_f32((ebp - 0x58) as u32, ((phi36 + phi37)).to_f32());
                vl_5c_130 = L!(0x5c);
                vl_60_130 = L!(0x60);
            } else {
                let t48 = ((((t45 * rf(m, B + 0x178)) + (t46 * rf(m, B + 0x188))) + (t47 * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
                m.set_f32((ebp - 0x60) as u32, (t48).to_f32());
                let t49 = ((((t45 * rf(m, B + 0x17c)) + (t46 * rf(m, B + 0x18c))) + (t47 * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
                m.set_f32((ebp - 0x5c) as u32, (t49).to_f32());
                phi33 = t48;
                phi34 = t49;
                phi35 = (((t47 * rf(m, B + 0x1a0)) + ((t46 * rf(m, B + 0x190)) + (t45 * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
                m.set_f32((ebp - 0x58) as u32, (phi35).to_f32());
                m.set_f32((ebp - 0x60) as u32, ((((rf(m, G + 0x178) * phi33) + (rf(m, G + 0x17c) * phi34)) + (rf(m, G + 0x180) * phi35))).to_f32());
                m.set_f32((ebp - 0x5c) as u32, ((((rf(m, G + 0x188) * phi33) + (rf(m, G + 0x18c) * phi34)) + (rf(m, G + 0x190) * phi35))).to_f32());
                phi36 = ((phi34 * rf(m, G + 0x19c)) + (phi33 * rf(m, G + 0x198)));
                phi37 = (phi35 * rf(m, G + 0x1a0));
                m.set_f32((ebp - 0x58) as u32, ((phi36 + phi37)).to_f32());
                vl_5c_130 = L!(0x5c);
                vl_60_130 = L!(0x60);
            }
        }
        let t52 = (((rf(m, H + 0x38) * L!(0x5c)) + (rf(m, H + 0x34) * L!(0x60))) + (rf(m, H + 0x3c) * L!(0x58)));
        m.set_f32((ebp - 0x60) as u32, (t52).to_f32());
        let t53 = (((rf(m, H + 0x44) * vl_60_130) + (rf(m, H + 0x48) * vl_5c_130)) + (rf(m, H + 0x4c) * L!(0x58)));
        m.set_f32((ebp - 0x5c) as u32, (t53).to_f32());
        let t54 = (x(1.0f32) - L!(0xc4));
        m.set_f32((ebp - 0xac) as u32, ((t52 * t54)).to_f32());
        vl_ac_133 = L!(0xac);
        m.set_f32((ebp - 0x60) as u32, (L!(0xac)).to_f32());
        m.set_f32((ebp - 0xac) as u32, ((t53 * t54)).to_f32());
        vl_ac_135 = L!(0xac);
        m.set_f32((ebp - 0xac) as u32, ((t54 * ((L!(0x58) * rf(m, H + 0x5c)) + ((vl_60_130 * rf(m, H + 0x54)) + (vl_5c_130 * rf(m, H + 0x58)))))).to_f32());
        m.set_f32((ebp - 0x58) as u32, (L!(0xac)).to_f32());
        m.set_f32((ebp - 0x5c) as u32, (vl_ac_135).to_f32());
        let t55 = ((((vl_ac_133 * rf(m, H + 0x34)) + (vl_ac_135 * rf(m, H + 0x44))) + (L!(0xac) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x60) as u32, (t55).to_f32());
        let t56 = ((((vl_ac_133 * rf(m, H + 0x38)) + (vl_ac_135 * rf(m, H + 0x48))) + (L!(0xac) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x5c) as u32, (t56).to_f32());
        let t57 = (((L!(0xac) * rf(m, H + 0x5c)) + ((vl_ac_135 * rf(m, H + 0x4c)) + (vl_ac_133 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x58) as u32, (t57).to_f32());
        if kind == 2 {
            let t61 = ((((t55 * rf(m, G + 0x178)) + (t56 * rf(m, G + 0x188))) + (t57 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((ebp - 0x60) as u32, (t61).to_f32());
            let t62 = ((((t55 * rf(m, G + 0x17c)) + (t56 * rf(m, G + 0x18c))) + (t57 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((ebp - 0x5c) as u32, (t62).to_f32());
            m.set_f32((ebp - 0x60) as u32, ((((rf(m, M1 + 0x17c) * t62) + (rf(m, M1 + 0x178) * t61)) + (rf(m, M1 + 0x180) * ((((t56 * rf(m, G + 0x190)) + (t55 * rf(m, G + 0x180))) + (t57 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            m.set_f32((ebp - 0x5c) as u32, ((((rf(m, M1 + 0x188) * t61) + (rf(m, M1 + 0x18c) * t62)) + (rf(m, M1 + 0x190) * ((((t56 * rf(m, G + 0x190)) + (t55 * rf(m, G + 0x180))) + (t57 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            phi38 = (t62 * rf(m, M1 + 0x19c));
            phi39 = (t61 * rf(m, M1 + 0x198));
            phi40 = (((((t56 * rf(m, G + 0x190)) + (t55 * rf(m, G + 0x180))) + (t57 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0)) * rf(m, M1 + 0x1a0));
            m.set_f32((ebp - 0x58) as u32, (((phi38 + phi39) + phi40)).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x60) as u32, (((((t55 * rf(m, G + 0x178)) + (t56 * rf(m, G + 0x188))) + (t57 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((ebp - 0x5c) as u32, (((((t56 * rf(m, G + 0x18c)) + (t55 * rf(m, G + 0x17c))) + (t57 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((ebp - 0x58) as u32, ((((t57 * rf(m, G + 0x1a0)) + ((t56 * rf(m, G + 0x190)) + (t55 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
            } else {
                let t58 = ((((t55 * rf(m, G + 0x178)) + (t56 * rf(m, G + 0x188))) + (t57 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
                m.set_f32((ebp - 0x60) as u32, (t58).to_f32());
                let t59 = ((((t55 * rf(m, G + 0x17c)) + (t56 * rf(m, G + 0x18c))) + (t57 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
                m.set_f32((ebp - 0x5c) as u32, (t59).to_f32());
                let t60 = (((t57 * rf(m, G + 0x1a0)) + ((t56 * rf(m, G + 0x190)) + (t55 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
                m.set_f32((ebp - 0x58) as u32, (t60).to_f32());
                m.set_f32((ebp - 0x60) as u32, ((((rf(m, B + 0x178) * t58) + (rf(m, B + 0x17c) * t59)) + (rf(m, B + 0x180) * t60))).to_f32());
                m.set_f32((ebp - 0x5c) as u32, ((((rf(m, B + 0x188) * t58) + (rf(m, B + 0x18c) * t59)) + (rf(m, B + 0x190) * t60))).to_f32());
                phi38 = (t59 * rf(m, B + 0x19c));
                phi39 = (t58 * rf(m, B + 0x198));
                phi40 = (t60 * rf(m, B + 0x1a0));
                m.set_f32((ebp - 0x58) as u32, (((phi38 + phi39) + phi40)).to_f32());
            }
        }
        m.set_f32((R0 + 0x48) as u32, ((((rf(m, JV + 0x224) * L!(0x5c)) + (rf(m, JV + 0x220) * L!(0x60))) + (rf(m, JV + 0x228) * L!(0x58)))).to_f32());
        m.set_f32((R0 + 0x4c) as u32, ((((rf(m, JV + 0x22c) * L!(0x60)) + (rf(m, JV + 0x230) * L!(0x5c))) + (rf(m, JV + 0x234) * L!(0x58)))).to_f32());
        m.set_f32((R0 + 0x50) as u32, ((((L!(0x5c) * rf(m, JV + 0x23c)) + (L!(0x60) * rf(m, JV + 0x238))) + (L!(0x58) * rf(m, JV + 0x240)))).to_f32());
    }
}
