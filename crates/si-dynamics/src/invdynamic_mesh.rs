// SPDX-License-Identifier: MIT
//! invdynamic.c, the wind force on a mesh (finding 0035).

use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

fn f(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// [soft 0x0849c3f0 invdynaGetMeshWindFrce]: the wind force (`wDir`, `wAmpl`) and torque on the mesh of `mdl2`
/// (the geometry slot `[mdl2+0x38][u16 mdl2+0x36]`), added to `out1` and `out2`. Each polygon whose normal (moved into
/// the frame of `mdl3`, by the matrix chain `[mdl3+0x2c4]+0x2c0` when `mdl1` is of kind 2, otherwise by the two models'
/// matrices) opposes the wind pushes with the wind component times the polygon surface (`SI_meshPolygonVolumeSurface`);
/// the push acts at the polygon centroid (`SI_meshPolygonCentroid`), turned by the matrix at `[mdl3+0x300]+0x1d8`,
/// and the torque is the cross product of the centroid with the push. The sums are scaled by `wAmpl` at the end.
pub fn invdyna_get_mesh_wind_frce(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, mdl3: u32, out1: u32, out2: u32) {
    let fr = m.local(0x8c);
    let ebp = fr + 0x8c;
    let l = |k: u32| ebp - k;
    for k in [0x1cu32, 0x20, 0x24, 0x28, 0x2c, 0x30] {
        m.set_u32(l(k), 0);
    }
    let mesh = m.u32(m.u32(mdl2 + 0x38).wrapping_add(4 * m.u16(mdl2 + 0x36) as u32));
    let z = x(0.0f32);
    let w = |m: &Mem, k: u32| f(m, 0x0882_2018 + 4 * k);
    let mut edi: i32 = 0;
    while edi < m.i32(mesh + 0x18) {
        let pv = m.u32(mesh + 4).wrapping_add((edi as u32).wrapping_mul(24));
        let (vx, vy, vz) = (f(m, pv), f(m, pv + 4), f(m, pv + 8));
        m.set_f32(l(0xc), vx.to_f32());
        m.set_f32(l(8), vy.to_f32());
        m.set_f32(l(4), vz.to_f32());
        let r0 = |m: &Mem| m.u32(m.u32(mdl3 + 0x2c4) + 0x2c0);
        if m.u32(mdl1 + 0x2ec) == 2 {
            if mdl2 != 0 {
                if mdl2 != r0(m) {
                    let t1 = (((vx * f(m, mdl2 + 0x178)) + (vy * f(m, mdl2 + 0x188))) + (vz * f(m, mdl2 + 0x198))) + f(m, mdl2 + 0x1a8);
                    m.set_f32(l(0xc), t1.to_f32());
                    let t2 = (((vx * f(m, mdl2 + 0x17c)) + (vy * f(m, mdl2 + 0x18c))) + (vz * f(m, mdl2 + 0x19c))) + f(m, mdl2 + 0x1ac);
                    m.set_f32(l(8), t2.to_f32());
                    let t3 = ((vz * f(m, mdl2 + 0x1a0)) + ((vy * f(m, mdl2 + 0x190)) + (vx * f(m, mdl2 + 0x180)))) + f(m, mdl2 + 0x1b0);
                    m.set_f32(l(4), t3.to_f32());
                    let r = r0(m);
                    let a = ((f(m, r + 0x178) * t1) + (f(m, r + 0x17c) * t2)) + (f(m, r + 0x180) * t3);
                    m.set_f32(l(0xc), a.to_f32());
                    let b = ((f(m, r + 0x188) * t1) + (f(m, r + 0x18c) * t2)) + (f(m, r + 0x190) * t3);
                    m.set_f32(l(8), b.to_f32());
                    let c = (t3 * f(m, r + 0x1a0)) + ((t2 * f(m, r + 0x19c)) + (t1 * f(m, r + 0x198)));
                    m.set_f32(l(4), c.to_f32());
                }
                let r = r0(m);
                let (a, b, c) = (f(m, l(0xc)), f(m, l(8)), f(m, l(4)));
                let nc = (((b * f(m, r + 0x128)) + (a * f(m, r + 0x118))) + (c * f(m, r + 0x138))) + f(m, r + 0x148);
                let n8 = (((a * f(m, r + 0x11c)) + (b * f(m, r + 0x12c))) + (c * f(m, r + 0x13c))) + f(m, r + 0x14c);
                let n4 = (((b * f(m, r + 0x130)) + (a * f(m, r + 0x120))) + (c * f(m, r + 0x140))) + f(m, r + 0x150);
                m.set_f32(l(0xc), nc.to_f32());
                m.set_f32(l(8), n8.to_f32());
                m.set_f32(l(4), n4.to_f32());
            }
        } else if mdl3 != mdl2 {
            let xw = ((vz * f(m, mdl2 + 0x198)) + ((vx * f(m, mdl2 + 0x178)) + (vy * f(m, mdl2 + 0x188)))) + f(m, mdl2 + 0x1a8);
            let yw = ((vz * f(m, mdl2 + 0x19c)) + ((vy * f(m, mdl2 + 0x18c)) + (vx * f(m, mdl2 + 0x17c)))) + f(m, mdl2 + 0x1ac);
            let zw = ((vz * f(m, mdl2 + 0x1a0)) + ((vy * f(m, mdl2 + 0x190)) + (vx * f(m, mdl2 + 0x180)))) + f(m, mdl2 + 0x1b0);
            let a = ((f(m, mdl3 + 0x17c) * yw) + (f(m, mdl3 + 0x178) * xw)) + (f(m, mdl3 + 0x180) * zw);
            m.set_f32(l(0xc), a.to_f32());
            let b = ((f(m, mdl3 + 0x188) * xw) + (f(m, mdl3 + 0x18c) * yw)) + (f(m, mdl3 + 0x190) * zw);
            m.set_f32(l(8), b.to_f32());
            let c = ((yw * f(m, mdl3 + 0x19c)) + (xw * f(m, mdl3 + 0x198))) + (zw * f(m, mdl3 + 0x1a0));
            m.set_f32(l(4), c.to_f32());
        }
        let dot = ((w(m, 0) * f(m, l(0xc))) + (w(m, 1) * f(m, l(8)))) + (w(m, 2) * f(m, l(4)));
        if dot.lt(z) {
            o.si_mesh_polygon_centroid(m, mesh, edi, l(0x18));
            o.si_mesh_polygon_volume_surface(m, mesh, edi, 1, l(0x60), l(0x5c));
            if m.u32(mdl1 + 0x2ec) == 2 {
                if mdl2 != 0 {
                    let edx = m.u32(mdl3 + 0x2c4);
                    if mdl2 != m.u32(edx + 0x2c0) {
                        let (p18, p14, p10) = (f(m, l(0x18)), f(m, l(0x14)), f(m, l(0x10)));
                        let t4 = (((p10 * f(m, mdl2 + 0x198)) + ((p14 * f(m, mdl2 + 0x188)) + (p18 * f(m, mdl2 + 0x178)))) + f(m, mdl2 + 0x1a8)) + f(m, mdl2 + 0x1b8);
                        let t5 = (((p10 * f(m, mdl2 + 0x19c)) + ((p14 * f(m, mdl2 + 0x18c)) + (p18 * f(m, mdl2 + 0x17c)))) + f(m, mdl2 + 0x1ac)) + f(m, mdl2 + 0x1bc);
                        let t6 = (((p10 * f(m, mdl2 + 0x1a0)) + ((p14 * f(m, mdl2 + 0x190)) + (p18 * f(m, mdl2 + 0x180)))) + f(m, mdl2 + 0x1b0)) + f(m, mdl2 + 0x1c0);
                        let r = m.u32(edx + 0x2c0);
                        let t7 = t4 - f(m, r + 0x1b8);
                        let t8 = t5 - f(m, r + 0x1bc);
                        let t9 = t6 - f(m, r + 0x1c0);
                        let s29 = ((f(m, r + 0x178) * t7) + (f(m, r + 0x17c) * t8)) + (f(m, r + 0x180) * t9);
                        let s30 = ((f(m, r + 0x188) * t7) + (f(m, r + 0x18c) * t8)) + (f(m, r + 0x190) * t9);
                        let s31 = ((t8 * f(m, r + 0x19c)) + (t7 * f(m, r + 0x198))) + (t9 * f(m, r + 0x1a0));
                        m.set_f32(l(0x18), s29.to_f32());
                        m.set_f32(l(0x14), s30.to_f32());
                        m.set_f32(l(0x10), s31.to_f32());
                    }
                    let r = m.u32(m.u32(mdl3 + 0x2c4) + 0x2c0);
                    let (p18, p14, p10) = (f(m, l(0x18)), f(m, l(0x14)), f(m, l(0x10)));
                    let t10 = (((p14 * f(m, r + 0x128)) + (p18 * f(m, r + 0x118))) + (p10 * f(m, r + 0x138))) + f(m, r + 0x148);
                    let t11 = (((p18 * f(m, r + 0x11c)) + (p14 * f(m, r + 0x12c))) + (p10 * f(m, r + 0x13c))) + f(m, r + 0x14c);
                    let t12 = ((p10 * f(m, r + 0x140)) + ((p14 * f(m, r + 0x130)) + (p18 * f(m, r + 0x120)))) + f(m, r + 0x150);
                    m.set_f32(l(0x18), (t10 + f(m, r + 0x158)).to_f32());
                    m.set_f32(l(0x14), (t11 + f(m, r + 0x15c)).to_f32());
                    m.set_f32(l(0x10), (t12 + f(m, r + 0x160)).to_f32());
                }
            } else if mdl2 != mdl3 {
                let (p18, p14, p10) = (f(m, l(0x18)), f(m, l(0x14)), f(m, l(0x10)));
                let xw = ((((p10 * f(m, mdl2 + 0x198)) + ((p14 * f(m, mdl2 + 0x188)) + (p18 * f(m, mdl2 + 0x178)))) + f(m, mdl2 + 0x1a8)) + f(m, mdl2 + 0x1b8)) - f(m, mdl3 + 0x1b8);
                let yw = ((((p10 * f(m, mdl2 + 0x19c)) + ((p14 * f(m, mdl2 + 0x18c)) + (p18 * f(m, mdl2 + 0x17c)))) + f(m, mdl2 + 0x1ac)) + f(m, mdl2 + 0x1bc)) - f(m, mdl3 + 0x1bc);
                let zw = ((((p10 * f(m, mdl2 + 0x1a0)) + ((p14 * f(m, mdl2 + 0x190)) + (p18 * f(m, mdl2 + 0x180)))) + f(m, mdl2 + 0x1b0)) + f(m, mdl2 + 0x1c0)) - f(m, mdl3 + 0x1c0);
                let a = ((f(m, mdl3 + 0x17c) * yw) + (f(m, mdl3 + 0x178) * xw)) + (f(m, mdl3 + 0x180) * zw);
                m.set_f32(l(0x18), a.to_f32());
                let b = ((f(m, mdl3 + 0x188) * xw) + (f(m, mdl3 + 0x18c) * yw)) + (f(m, mdl3 + 0x190) * zw);
                m.set_f32(l(0x14), b.to_f32());
                let c = ((yw * f(m, mdl3 + 0x19c)) + (xw * f(m, mdl3 + 0x198))) + (zw * f(m, mdl3 + 0x1a0));
                m.set_f32(l(0x10), c.to_f32());
            }
            // the push: minus the wind component times the polygon surface, at the centroid
            let k = -((-dot) * f(m, l(0x5c)));
            let (nc, n8, n4) = (f(m, l(0xc)), f(m, l(8)), f(m, l(4)));
            let (p18, p14, p10) = (f(m, l(0x18)), f(m, l(0x14)), f(m, l(0x10)));
            let a1 = k * n4;
            let b1 = p14 * a1;
            let k8 = k * n8;
            m.set_f32(l(0x68), b1.to_f32());
            let c1 = p10 * k8;
            let kc = k * nc;
            let t14 = f(m, l(0x68)) - c1;
            let d1 = p14 * kc;
            m.set_f32(l(0x68), t14.to_f32());
            m.set_f32(l(0x48), t14.to_f32());
            let t18 = (p18 * k8) - d1;
            let t19 = (p10 * kc) - (p18 * a1);
            let q = m.u32(mdl3 + 0x300);
            let t20 = ((f(m, q + 0x1d8) * kc) + (f(m, q + 0x1dc) * k8)) + (f(m, q + 0x1e0) * a1);
            let t21 = ((f(m, q + 0x1e4) * kc) + (f(m, q + 0x1e8) * k8)) + (f(m, q + 0x1ec) * a1);
            let t22 = (a1 * f(m, q + 0x1f8)) + ((k8 * f(m, q + 0x1f4)) + (kc * f(m, q + 0x1f0)));
            let n24 = t20 + f(m, l(0x24));
            let n20 = t21 + f(m, l(0x20));
            let n1c = t22 + f(m, l(0x1c));
            m.set_f32(l(0x24), n24.to_f32());
            m.set_f32(l(0x20), n20.to_f32());
            m.set_f32(l(0x1c), n1c.to_f32());
            let n30 = f(m, l(0x68)) + f(m, l(0x30));
            let n2c = t19 + f(m, l(0x2c));
            let n28 = t18 + f(m, l(0x28));
            m.set_f32(l(0x30), n30.to_f32());
            m.set_f32(l(0x2c), n2c.to_f32());
            m.set_f32(l(0x28), n28.to_f32());
        }
        edi += 1;
    }
    let wa = f(m, 0x0882_2014);
    let e28 = wa * f(m, l(0x28));
    let e24 = wa * f(m, l(0x24));
    let e20 = wa * f(m, l(0x20));
    let e1c = wa * f(m, l(0x1c));
    let e30 = wa * f(m, l(0x30));
    let e2c = wa * f(m, l(0x2c));
    for (k, e) in [(0x28u32, e28), (0x24, e24), (0x20, e20), (0x1c, e1c), (0x30, e30), (0x2c, e2c)] {
        m.set_f32(l(k), e.to_f32());
    }
    let (o1x, o1y, o1z) = (e24 + f(m, out1), e20 + f(m, out1 + 4), e1c + f(m, out1 + 8));
    m.set_f32(out1, o1x.to_f32());
    m.set_f32(out1 + 4, o1y.to_f32());
    m.set_f32(out1 + 8, o1z.to_f32());
    let (o2x, o2y, o2z) = (e30 + f(m, out2), e2c + f(m, out2 + 4), e28 + f(m, out2 + 8));
    m.set_f32(out2, o2x.to_f32());
    m.set_f32(out2 + 4, o2y.to_f32());
    m.set_f32(out2 + 8, o2z.to_f32());
    m.release(0x8c);
}
