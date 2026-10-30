// SPDX-License-Identifier: MIT
//! `settings.c` (finding 0035): the bounding boxes and inertia tensors of a model hierarchy. A model's box is `+0x10 .. +0x24`
//! (lowest, highest corner), its scale `+0x16c`, two 4 x 4 matrices at `+0x178` and `+0x1b8` (the translation of the second is
//! `+0x1b8 .. +0x1c0`); the child link is `+0x2c4`, the sibling link `+0x2c0`.
//!
//! Float code was compiled for the x87 unit: sums are formed in 80 bits and rounded to f32 at a store.

use crate::settings::inertia_tensor_elem_box_factors;
use si_mem::Mem;
use si_x87::{x, F80};

fn g(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// The box of `p1` carried into the frame of `p2` [soft 0x0849e4dc modelBoundingBoxTransfer]: the eight corners of `p1`'s
/// scaled box go through its matrix (`+0x178`, `+0x1b8`) and back through the transpose of `p2`'s (the sums formed in 80
/// bits), and the lowest and highest corners and the largest distance from the origin are written to `p2 + 0x10 .. 0x2c`.
pub fn model_bounding_box_transfer(m: &mut Mem, p1: u32, p2: u32) {
    let rf = |m: &Mem, a: u32| m.f32(a);
    let a = [(g(m, p1 + 0x10) * g(m, p1 + 0x16c)).to_f32(), (g(m, p1 + 0x16c) * g(m, p1 + 0x1c)).to_f32()];
    let b = [(g(m, p1 + 0x14) * g(m, p1 + 0x170)).to_f32(), (g(m, p1 + 0x170) * g(m, p1 + 0x20)).to_f32()];
    let c = [(g(m, p1 + 0x18) * g(m, p1 + 0x174)).to_f32(), (g(m, p1 + 0x174) * g(m, p1 + 0x24)).to_f32()];
    let hi = rf(m, 0x0866_0b30);
    let lo = rf(m, 0x0866_0b34);
    let (mut l18, mut l14, mut l10) = (hi, hi, hi);
    let (mut l24, mut l20, mut l1c, mut l4c) = (lo, lo, lo, lo);
    for i in 0..2 {
        for j in 0..2 {
            for k in 0..2 {
                let (pa, pb, pc) = (x(a[i]), x(b[j]), x(c[k]));
                let pt = |m: &Mem, o: [u32; 5]| {
                    (((pc * g(m, p1 + o[0]) + (pb * g(m, p1 + o[1]) + pa * g(m, p1 + o[2]))) + g(m, p1 + o[3])) + g(m, p1 + o[4]))
                        - g(m, p2 + o[4])
                };
                let xx = pt(m, [0x198, 0x188, 0x178, 0x1a8, 0x1b8]);
                let yy = pt(m, [0x19c, 0x18c, 0x17c, 0x1ac, 0x1bc]);
                let zz = pt(m, [0x1a0, 0x190, 0x180, 0x1b0, 0x1c0]);
                let t1 = (g(m, p2 + 0x17c) * yy + g(m, p2 + 0x178) * xx) + g(m, p2 + 0x180) * zz;
                let l8 = ((g(m, p2 + 0x188) * xx + g(m, p2 + 0x18c) * yy) + g(m, p2 + 0x190) * zz).to_f32();
                let l4 = ((yy * g(m, p2 + 0x19c) + xx * g(m, p2 + 0x198)) + zz * g(m, p2 + 0x1a0)).to_f32();
                let lc = t1.to_f32();
                if lc < l18 {
                    l18 = lc;
                }
                if l8 < l14 {
                    l14 = l8;
                }
                if l4 < l10 {
                    l10 = l4;
                }
                if lc > l24 {
                    l24 = lc;
                }
                if l8 > l20 {
                    l20 = l8;
                }
                if l4 > l1c {
                    l1c = l4;
                }
                let r = ((x(l14) * x(l14) + x(l18) * x(l18)) + x(l10) * x(l10)).sqrt().to_f32();
                if r > l4c {
                    l4c = r;
                }
                let r = ((x(l20) * x(l20) + x(l24) * x(l24)) + x(l1c) * x(l1c)).sqrt().to_f32();
                if r > l4c {
                    l4c = r;
                }
            }
        }
    }
    for (o, v) in [(0x10, l18), (0x14, l14), (0x18, l10), (0x1c, l24), (0x20, l20), (0x24, l1c), (0x28, l4c), (0x2c, l4c)] {
        m.set_f32(p2 + o, v);
    }
}

/// The bounding of every model of a hierarchy [soft 0x0849e808 hierarchyBoundingCompute]: `SI_modelComputeBounding` of the
/// model, then of its child and of its sibling (native: `model.c`).
pub fn hierarchy_bounding_compute(m: &mut Mem, model: u32) {
    si_model::model_image::bounds::compute_bounding(m, si_mem::Ptr32::new(model));
    let c = m.u32(model + 0x2c4);
    if c != 0 {
        hierarchy_bounding_compute(m, c);
    }
    let s = m.u32(model + 0x2c0);
    if s != 0 {
        hierarchy_bounding_compute(m, s);
    }
}

/// One model of the overall box [soft 0x0849e850 hierarchyNodeOverallBoundingCompute]: the two corners of `node`'s box (taken
/// to the root's frame unless the node is the root, in the order and rounding of the original) widen the box `lo` / `hi`
/// (three floats each); then its child and sibling.
pub fn hierarchy_node_overall_bounding_compute(m: &mut Mem, root: u32, node: u32, lo: u32, hi: u32) {
    let (a, gr) = (node, root);
    let (t1, t2, t3) = (g(m, a + 0x10) * g(m, a + 0x16c), g(m, a + 0x14) * g(m, a + 0x170), g(m, a + 0x18) * g(m, a + 0x174));
    let l1c = (g(m, a + 0x16c) * g(m, a + 0x1c)).to_f32();
    let l20 = (g(m, a + 0x170) * g(m, a + 0x20)).to_f32();
    let l24 = (g(m, a + 0x174) * g(m, a + 0x24)).to_f32();
    let (lc, l8, l4, l18, l14, l10);
    if node == root {
        lc = t1.to_f32();
        l8 = t2.to_f32();
        l4 = t3.to_f32();
        l18 = l1c;
        l14 = l20;
        l10 = l24;
    } else {
        // the corner of the lowest values: the products stay unrounded
        let xx = (((t3 * g(m, a + 0x198) + (t1 * g(m, a + 0x178) + t2 * g(m, a + 0x188))) + g(m, a + 0x1a8)) + g(m, a + 0x1b8)) - g(m, gr + 0x1b8);
        let yy = (((t3 * g(m, a + 0x19c) + (t2 * g(m, a + 0x18c) + t1 * g(m, a + 0x17c))) + g(m, a + 0x1ac)) + g(m, a + 0x1bc)) - g(m, gr + 0x1bc);
        let zz = (((t3 * g(m, a + 0x1a0) + (t2 * g(m, a + 0x190) + t1 * g(m, a + 0x180))) + g(m, a + 0x1b0)) + g(m, a + 0x1c0)) - g(m, gr + 0x1c0);
        lc = ((g(m, gr + 0x178) * xx + g(m, gr + 0x17c) * yy) + g(m, gr + 0x180) * zz).to_f32();
        l8 = ((g(m, gr + 0x18c) * yy + g(m, gr + 0x188) * xx) + g(m, gr + 0x190) * zz).to_f32();
        l4 = (zz * g(m, gr + 0x1a0) + (yy * g(m, gr + 0x19c) + xx * g(m, gr + 0x198))).to_f32();
        // the corner of the highest values: the partial z sum goes through a float
        let (r4, r5, r6) = (x(l1c), x(l20), x(l24));
        let s3 = (r4 * g(m, a + 0x180)).to_f32();
        let s20 = (r5 * g(m, a + 0x190)).to_f32();
        let s3 = (x(s3) + x(s20)).to_f32();
        let s24 = (r6 * g(m, a + 0x1a0)).to_f32();
        let l3c = ((((x(s3) + x(s24)) + g(m, a + 0x1b0)) + g(m, a + 0x1c0)) - g(m, gr + 0x1c0)).to_f32();
        let z2 = x(l3c);
        let x2 = (((r6 * g(m, a + 0x198) + (r4 * g(m, a + 0x178) + r5 * g(m, a + 0x188))) + g(m, a + 0x1a8)) + g(m, a + 0x1b8)) - g(m, gr + 0x1b8);
        let y2 = ((((r4 * g(m, a + 0x17c) + r5 * g(m, a + 0x18c)) + r6 * g(m, a + 0x19c)) + g(m, a + 0x1ac)) + g(m, a + 0x1bc)) - g(m, gr + 0x1bc);
        l18 = (g(m, gr + 0x180) * z2 + (g(m, gr + 0x178) * x2 + g(m, gr + 0x17c) * y2)).to_f32();
        l10 = (g(m, gr + 0x1a0) * z2 + (y2 * g(m, gr + 0x19c) + x2 * g(m, gr + 0x198))).to_f32();
        l14 = ((g(m, gr + 0x18c) * y2 + g(m, gr + 0x188) * x2) + g(m, gr + 0x190) * z2).to_f32();
    }
    for (k, v) in [lc, l8, l4].into_iter().enumerate() {
        if v < m.f32(lo + 4 * k as u32) {
            m.set_f32(lo + 4 * k as u32, v);
        }
    }
    for (k, v) in [l18, l14, l10].into_iter().enumerate() {
        if v > m.f32(hi + 4 * k as u32) {
            m.set_f32(hi + 4 * k as u32, v);
        }
    }
    let c = m.u32(node + 0x2c4);
    if c != 0 {
        hierarchy_node_overall_bounding_compute(m, root, c, lo, hi);
    }
    let s = m.u32(node + 0x2c0);
    if s != 0 {
        hierarchy_node_overall_bounding_compute(m, root, s, lo, hi);
    }
}

/// The box of a whole hierarchy [soft 0x0849ebc4 hierarchyOverallBoundingCompute]: the bounding of every model, then
/// `lo` set to the constant 1e10 and `hi` to -1e10 in all three coordinates and widened by every model.
pub fn hierarchy_overall_bounding_compute(m: &mut Mem, root: u32, lo: u32, hi: u32) {
    hierarchy_bounding_compute(m, root);
    let (a, b) = (m.u32(0x0866_0b38), m.u32(0x0866_0b3c));
    for k in 0..3 {
        m.set_u32(lo + 4 * k, a);
        m.set_u32(hi + 4 * k, b);
    }
    hierarchy_node_overall_bounding_compute(m, root, root, lo, hi);
}

/// One model of the overall inertia [soft 0x0849ec10 hierNodeOverallInertiaTensorCompute]: for a node with a mass record
/// (`+0x2f4`) under a root with a settings record (`+0x304`), the box of the node in the root's frame (via
/// [`model_bounding_box_transfer`], the root's own box restored afterwards) gives six factors that are added, scaled by the
/// mass, to `out[0..6]`; the box moved by the settings' centre (`+0x64 .. 0x6c`) gives six more for `out[6..12]`; `out[12]`
/// adds the mass record's third word and `out[13]` its second. Then the children, unless the node is a joint (kinds 1, 2)
/// that is not the root and has no parent link.
pub fn hier_node_overall_inertia_tensor_compute(m: &mut Mem, root: u32, node: u32, out: [u32; 14]) {
    let rec = m.u32(node + 0x2f4);
    let sett = m.u32(root + 0x304);
    if rec != 0 && sett != 0 {
        let fr = m.local(0x50);
        let l = |k: u32| fr + 0x50 - k;
        let (lc, l8, l4, l18, l14, l10) = (l(0xc), l(8), l(4), l(0x18), l(0x14), l(0x10));
        if node == root {
            m.set_f32(lc, (g(m, root + 0x10) * g(m, root + 0x16c)).to_f32());
            m.set_f32(l8, (g(m, root + 0x14) * g(m, root + 0x170)).to_f32());
            m.set_f32(l4, (g(m, root + 0x18) * g(m, root + 0x174)).to_f32());
            m.set_f32(l18, (g(m, root + 0x16c) * g(m, root + 0x1c)).to_f32());
            m.set_f32(l14, (g(m, root + 0x170) * g(m, root + 0x20)).to_f32());
            m.set_f32(l10, (g(m, root + 0x174) * g(m, root + 0x24)).to_f32());
        } else {
            let saved: Vec<u32> = (0..6).map(|k| m.u32(root + 0x10 + 4 * k)).collect();
            let (s2c, s28) = (m.u32(root + 0x2c), m.u32(root + 0x28));
            model_bounding_box_transfer(m, node, root);
            for (k, a) in [lc, l8, l4, l18, l14, l10].into_iter().enumerate() {
                let v = m.u32(root + 0x10 + 4 * k as u32);
                m.set_u32(a, v);
            }
            for (k, v) in saved.into_iter().enumerate() {
                m.set_u32(root + 0x10 + 4 * k as u32, v);
            }
            m.set_u32(root + 0x2c, s2c);
            m.set_u32(root + 0x28, s28);
        }
        let outs = [l(0x48), l(0x44), l(0x40), l(0x3c), l(0x38), l(0x34)];
        let load = |m: &Mem| ([m.f32(lc), m.f32(l8), m.f32(l4)], [m.f32(l18), m.f32(l14), m.f32(l10)]);
        let (v1, v2) = load(m);
        inertia_tensor_elem_box_factors(m, v1, v2, outs);
        for k in 0..6 {
            let p = m.u32(node + 0x2f4);
            let v = (g(m, p) * g(m, outs[k]) + g(m, out[k])).to_f32();
            m.set_f32(out[k], v);
        }
        for (a, d) in [(lc, 0x64), (l8, 0x68), (l4, 0x6c), (l18, 0x64), (l14, 0x68), (l10, 0x6c)] {
            let v = (g(m, a) - g(m, m.u32(root + 0x304) + d)).to_f32();
            m.set_f32(a, v);
        }
        let (v1, v2) = load(m);
        inertia_tensor_elem_box_factors(m, v1, v2, outs);
        for k in 0..6 {
            let p = m.u32(node + 0x2f4);
            let v = (g(m, p) * g(m, outs[k]) + g(m, out[6 + k])).to_f32();
            m.set_f32(out[6 + k], v);
        }
        let p = m.u32(node + 0x2f4);
        let v = (g(m, out[12]) + g(m, p + 8)).to_f32();
        m.set_f32(out[12], v);
        let p = m.u32(node + 0x2f4);
        let v = (g(m, out[13]) + g(m, p + 4)).to_f32();
        m.set_f32(out[13], v);
        m.release(0x50);
    }
    if node == root || m.u32(node + 0x30) != 0 || m.u32(node + 0x2ec).wrapping_sub(1) > 1 {
        let c = m.u32(node + 0x2c4);
        if c != 0 {
            hier_node_overall_inertia_tensor_compute(m, root, c, out);
        }
        let s = m.u32(node + 0x2c0);
        if s != 0 {
            hier_node_overall_inertia_tensor_compute(m, root, s, out);
        }
    }
}

/// The inertia tensors of a whole hierarchy [soft 0x0849ef7c hierarchyOverallInertiaTensorCompute]: two 3 x 3 tensors at `t1`
/// and `t2` and the two sums `s1`, `s2` (cleared first) of [`hier_node_overall_inertia_tensor_compute`]. The original stores
/// the off-diagonal words of the second tensor's lower half into `t1` (+0xc, +0x18, +0x1c) as well as into `t2`; the port
/// does the same.
pub fn hierarchy_overall_inertia_tensor_compute(m: &mut Mem, root: u32, t1: u32, t2: u32, s1: u32, s2: u32) {
    let fr = m.local(0x30);
    let l = |k: u32| fr + 0x30 - k;
    for k in [0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 4, 8, 0xc, 0x10, 0x14, 0x18] {
        m.set_u32(l(k), 0);
    }
    m.set_u32(s2, 0);
    m.set_u32(s1, 0);
    let outs = [l(0x30), l(0x2c), l(0x28), l(0x24), l(0x20), l(0x1c), l(0x18), l(0x14), l(0x10), l(0xc), l(8), l(4), s1, s2];
    hier_node_overall_inertia_tensor_compute(m, root, root, outs);
    let f = |m: &Mem, k: u32| g(m, l(k));
    let set = |m: &mut Mem, a: u32, v: F80| m.set_f32(a, v.to_f32());
    let (v2c, v28, v30) = (f(m, 0x2c), f(m, 0x28), f(m, 0x30));
    set(m, t1, v2c + v28);
    set(m, t1 + 0x20, v2c + v30);
    set(m, t1 + 0x10, v28 + v30);
    let v = m.f32(l(0x1c));
    m.set_f32(t1 + 0xc, v);
    m.set_f32(t1 + 4, v);
    let v = m.f32(l(0x20));
    m.set_f32(t1 + 0x18, v);
    m.set_f32(t1 + 8, v);
    let v = m.f32(l(0x24));
    m.set_f32(t1 + 0x1c, v);
    m.set_f32(t1 + 0x14, v);
    let (v14, v10, v18) = (f(m, 0x14), f(m, 0x10), f(m, 0x18));
    set(m, t2, v14 + v10);
    set(m, t2 + 0x20, v14 + v18);
    set(m, t2 + 0x10, v10 + v18);
    let v = m.f32(l(0xc));
    m.set_f32(t1 + 0xc, v);
    m.set_f32(t2 + 4, v);
    let v = m.f32(l(8));
    m.set_f32(t1 + 0x18, v);
    m.set_f32(t2 + 8, v);
    let v = m.f32(l(4));
    m.set_f32(t1 + 0x1c, v);
    m.set_f32(t2 + 0x14, v);
    m.release(0x30);
}
