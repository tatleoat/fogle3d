// SPDX-License-Identifier: MIT
//! `curveOrient.c` (package 15, finding 0044 section "Trim6"): the orientation test of trim-curve loops, on the memory image.
//!
//! Records (names ours): a CurveOrient (0x28 bytes, `SI_calloc(1, 0x28)`): +0 / +4 the first and last intersection of the loop,
//! +8 their number, +0xc a 16-bit "closed" word, +0x10 .. +0x1c four intersection pointers written by `findUvCriticalInters`,
//! +0x24 a byte of flag bits. An intersection (the 0x78-byte record of `inter.rs`) has its flags at +0x50, the ray at +0x40
//! (`ray + 0x30` is the parameter), a double pair at +0x64 and its previous / next element of the loop at +0x70 / +0x74. A trim
//! record (the surface's trim, `Trim`) has a 16-bit-or-int "perspective" word at +0x3c, the view point at +0x44, the direction at
//! +0x5c and its curve (count +4, knots +0xc, 0x24-byte points +0x2c) at +0x74.

use si_mem::Mem;
use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// [soft 0x0850a330 si_nurbsCurveOrientCreateP]: `(first, last, closed, count)`: a zeroed 0x28-byte record holding them at +0, +4,
/// +0xc (16 bits) and +8; 0 where the allocation fails.
pub fn curve_orient_create_p(m: &mut Mem, first: u32, last: u32, closed: i16, count: u32) -> u32 {
    let p = m.si_calloc(1, 0x28);
    if p != 0 {
        m.set_u32(p, first);
        m.set_u32(p + 4, last);
        m.set_u16(p + 0xc, closed as u16);
        m.set_u32(p + 8, count);
    }
    p
}

/// [soft 0x0850a36c si_nurbsCurveOrientDestroy]: `(pp)`: frees what `*pp` points to and clears `*pp`; a null `pp` or `*pp` does nothing.
pub fn curve_orient_destroy(m: &mut Mem, pp: u32) {
    if pp != 0 && m.u32(pp) != 0 {
        let p = m.u32(pp);
        m.free(p);
        m.set_u32(pp, 0);
    }
}

/// [soft 0x0850c88c pointRelativeToPlane]: `(n0, n1, n2, d, p0, p1, p2)` answers (in st0, 80 bits) `((n0 p0 + n1 p1) + n2 p2) + d`.
/// (Not checked: the emulated original answers in st0 only, which the vectors do not carry.)
pub fn point_relative_to_plane(n: [f64; 3], d: f64, p: [f64; 3]) -> F80 {
    ((x(n[0]) * x(p[0]) + x(n[1]) * x(p[1])) + x(n[2]) * x(p[2])) + x(d)
}

/// [soft 0x0850c8b0 inDomainSamp]: `(trim, v)`: 1 when the first knot of the trim's curve (at `*(trim + 0x74)`) that is above `v` is
/// within 1e-6 of it or (not the first knot) the knot before it is, else 0.
pub fn in_domain_samp(m: &Mem, trim: u32, v: f64) -> u16 {
    let crv = m.u32(trim + 0x74);
    let n = m.i32(crv + 4);
    let knots = m.u32(crv + 0xc);
    let eps = x(1e-6); // 0x08662018
    let mut i = 0i32;
    while i < n {
        let k = x(m.f64(knots.wrapping_add((i as u32).wrapping_mul(8))));
        if x(v).lt(k) {
            if (k - x(v)).abs().lt(eps) {
                return 1;
            }
            if i == 0 {
                return 0;
            }
            let kp = x(m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8)));
            return if (kp - x(v)).abs().lt(eps) { 1 } else { 0 };
        }
        i += 1;
    }
    0
}

fn usable(m: &Mem, i: u32) -> bool {
    let nx = m.u32(i + 0x74);
    m.u32(i + 0x50) & 0x300 == 0 && nx != 0 && m.u32(i + 0x40) != m.u32(nx + 0x40) && m.u32(i + 0x50) & 0xc00 == 0
}

/// [soft 0x0850b63c findPointInCurve]: `(co, trim)`: walks the loop of intersections (next links at +0x74, previous at +0x70) for one
/// that is not marked (flags 0x300 / 0xc00), whose ray differs from its successor's and whose ray parameter is in the trim curve's
/// domain ([`in_domain_samp`]): starting half the count along from the first; failing that, the last unmarked one walking back
/// from where the first walk stopped; 0 where none.
pub fn find_point_in_curve(m: &Mem, co: u32, trim: u32) -> u32 {
    let first = m.u32(co);
    let closed = m.u16(co + 0xc) != 0;
    let nn = m.i32(co + 8);
    let mut half = (nn - (nn >> 31)) >> 1;
    let mut found = false;
    let mut i5 = first;
    let mut i4;
    loop {
        i4 = i5;
        if i5 == 0 || half < 1 {
            break;
        }
        i5 = m.u32(i5 + 0x74);
        half -= 1;
        if i5 == first {
            i4 = i5;
            if closed {
                break;
            }
        }
    }
    loop {
        if i5 == 0 {
            break;
        }
        if usable(m, i5) {
            let ray = m.u32(i5 + 0x40);
            if in_domain_samp(m, trim, m.f64(ray + 0x30)) != 0 {
                found = true;
                break;
            }
        }
        i5 = m.u32(i5 + 0x74);
        if i5 == first && closed {
            break;
        }
    }
    if !found {
        loop {
            if i4 == 0 {
                return 0;
            }
            if usable(m, i4) {
                return i4;
            }
            i4 = m.u32(i4 + 0x70);
            if i4 == first {
                i5 = i4;
                if closed {
                    break;
                }
            }
        }
    }
    i5
}

/// [soft 0x0850bc34 si_nurbsCurveOrientProjPtPara]: `(p, d, q, out)`: projects the point `p` onto the line through `q` along the unit
/// vector `d` (80-bit arithmetic): writes the projection to `out` and answers 1; where `p - q` is within 1e-6 of perpendicular to `d` writes
/// `p` itself, where the perpendicular part is shorter than 1e-6 (squared) answers 0 and writes nothing.
pub fn curve_orient_proj_pt_para(m: &mut Mem, p: [f64; 3], d: [f64; 3], q: [f64; 3], out: u32) -> u16 {
    let t1 = x(p[0]) - x(q[0]);
    let t2 = x(p[1]) - x(q[1]);
    let t3 = x(p[2]) - x(q[2]);
    let t4 = ((t2 * x(d[1])) + (t1 * x(d[0]))) + (t3 * x(d[2]));
    let eps = x(1e-6); // 0x08661fe8
    if !eps.lt(t4.abs()) {
        m.set_f64(out, p[0]);
        m.set_f64(out + 8, p[1]);
        m.set_f64(out + 16, p[2]);
        return 1;
    }
    let t6 = t1 - t4 * x(d[0]);
    let t7 = t2 - x(d[1]) * t4;
    let t9 = t3 - t4 * x(d[2]);
    if ((t9 * t9) + ((t7 * t7) + (t6 * t6))).lt(eps) {
        return 0;
    }
    m.set_f64(out, (t6 + x(q[0])).to_f64());
    m.set_f64(out + 8, (t7 + x(q[1])).to_f64());
    m.set_f64(out + 16, (t9 + x(q[2])).to_f64());
    1
}

/// The unit vector from `b` to `a` as the original builds it: components in 80 bits, the length rounded to a float, and, when
/// that is over 1e-4, each component divided by it; stored as doubles. `swap_sum` selects the order of the sum of squares
/// (`projectPtPers`: z + (y + x); `projectCrvSamples`: (y + x) + z).
pub(crate) fn unit_dir(a: [f64; 3], b: [f64; 3], swap_sum: bool) -> [f64; 3] {
    let t1 = x(a[0]) - x(b[0]);
    let t2 = x(a[1]) - x(b[1]);
    let t3 = x(a[2]) - x(b[2]);
    let s = if swap_sum { (t3 * t3) + ((t2 * t2) + (t1 * t1)) } else { ((t2 * t2) + (t1 * t1)) + (t3 * t3) };
    let len = F80::from_f32(s.sqrt().to_f32());
    if x(1e-4).lt(len.abs()) {
        [(t1 / len).to_f64(), (t2 / len).to_f64(), (t3 / len).to_f64()]
    } else {
        [t1.to_f64(), t2.to_f64(), t3.to_f64()]
    }
}

/// [soft 0x0850c710 projectPtPers]: `(a, b, c, d, out)`: the point where the line through `c` along the unit vector from `d` to `a`
/// ... precisely: with `n` the unit vector from `d` to `a`, answers 0 where `|b . n| < 1e-6`, else writes `d + t n` with
/// `t = (b . d - b . c) / (b . n)` (the plane through `c` normal to `b`; `d` itself where `|t|` is within 1e-6) and answers 1.
pub fn project_pt_pers(m: &mut Mem, a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], out: u32) -> u16 {
    let n = unit_dir(a, d, true);
    let t4 = ((x(b[1]) * x(n[1])) + (x(b[0]) * x(n[0]))) + (x(b[2]) * x(n[2]));
    let eps = x(1e-6); // 0x08662010
    if t4.abs().lt(eps) {
        return 0;
    }
    let bc = (x(c[2]) * x(b[2])) + ((x(c[1]) * x(b[1])) + (x(c[0]) * x(b[0])));
    let bd = (x(b[2]) * x(d[2])) + ((x(b[1]) * x(d[1])) + (x(b[0]) * x(d[0])));
    let t5 = -((-bc + bd) / t4);
    if !eps.lt(t5.abs()) {
        m.set_f64(out, d[0]);
        m.set_f64(out + 8, d[1]);
        m.set_f64(out + 16, d[2]);
    } else {
        m.set_f64(out, (x(d[0]) + t5 * x(n[0])).to_f64());
        m.set_f64(out + 8, (x(d[1]) + t5 * x(n[1])).to_f64());
        m.set_f64(out + 16, (t5 * x(n[2]) + x(d[2])).to_f64());
    }
    1
}

pub(crate) fn dv3(m: &Mem, a: u32) -> [f64; 3] {
    [m.f64(a), m.f64(a + 8), m.f64(a + 16)]
}

/// [soft 0x0850b9b4 projectCrvSamples]: `(trim, co, inter, pout, pidx)`: allocates 0x18 bytes for each point of the trim's curve at `*pout`,
/// projects every point ([`curve_orient_proj_pt_para`] along the trim's direction (+0x5c) from the intersection's ray origin when the
/// trim's word at +0x3c is 0, else [`project_pt_pers`] from its view point (+0x44) with the unit vector from the view point to the curve's
/// first point), sets `*pidx` to the last index whose knot is within 1e-6 of the ray parameter (-1 at first); answers 1, or 0
/// (freeing the array) where a projection fails or no knot matched.
pub fn project_crv_samples(m: &mut Mem, trim: u32, co: u32, inter: u32, pout: u32, pidx: u32) -> u16 {
    if trim == 0 || co == 0 || inter == 0 {
        return 0;
    }
    m.set_i32(pidx, -1);
    let crv0 = m.u32(trim + 0x74);
    let arr = m.si_calloc(m.u32(crv0 + 4), 0x18);
    m.set_u32(pout, arr);
    if arr == 0 {
        return 0;
    }
    let persp = m.u32(trim + 0x3c) != 0;
    let (pt, dir);
    if !persp {
        dir = dv3(m, trim + 0x5c);
        let ray = m.u32(inter + 0x40);
        pt = dv3(m, ray);
    } else {
        let p0 = m.u32(crv0 + 0x2c);
        pt = dv3(m, p0);
        dir = unit_dir(pt, dv3(m, trim + 0x44), false);
    }
    let mut ok = true;
    let mut i = 0i32;
    let mut crv = m.u32(trim + 0x74);
    while i < m.i32(crv + 4) {
        let ray = m.u32(inter + 0x40);
        let knots = m.u32(crv + 0xc);
        if (x(m.f64(ray + 0x30)) - x(m.f64(knots.wrapping_add(8 * i as u32)))).abs().lt(x(1e-6)) {
            m.set_i32(pidx, i);
        }
        crv = m.u32(trim + 0x74);
        let s = dv3(m, m.u32(crv + 0x2c).wrapping_add(0x24 * i as u32));
        let out = arr.wrapping_add(0x18 * i as u32);
        let r = if !persp {
            curve_orient_proj_pt_para(m, s, dir, pt, out)
        } else {
            project_pt_pers(m, s, dir, pt, dv3(m, trim + 0x44), out)
        };
        if r == 0 {
            ok = false;
            break;
        }
        i += 1;
        crv = m.u32(trim + 0x74);
    }
    if ok && m.i32(pidx) != -1 {
        return 1;
    }
    let a = m.u32(pout);
    if a != 0 {
        m.free(a);
        m.set_u32(pout, 0);
    }
    0
}

/// [soft 0x0850b384 reverseCurve]: `(co)`: reverses the direction of the loop of intersections: every element's previous / next links are
/// exchanged, the double pair at +0x64 is moved on to the next element, and the direction bits of the flags at +0x50 are mirrored
/// (0x10 <-> 0x20 with 0x400, 0x40 <-> 0x80 with 0x800; bits 0xcf0 are cleared first); for an open loop the first / last pointers are
/// swapped. Answers `co`.
pub fn reverse_curve(m: &mut Mem, co: u32) -> u32 {
    if co == 0 {
        return 0;
    }
    let first = m.u32(co);
    let closed = m.u16(co + 0xc) != 0;
    let swap = |m: &mut Mem| {
        if !closed {
            let t = m.u32(co + 4);
            m.set_u32(co + 4, m.u32(co));
            m.set_u32(co, t);
        }
    };
    let mut e = first;
    if e == 0 {
        swap(m);
        return co;
    }
    let prev = m.u32(e + 0x70);
    let (mut fl, mut held) = if prev == 0 {
        (m.u32(e + 0x50), (m.u32(e + 0x64), m.u32(e + 0x68)))
    } else {
        (m.u32(prev + 0x50), (m.u32(prev + 0x64), m.u32(prev + 0x68)))
    };
    loop {
        if e == 0 {
            swap(m);
            return co;
        }
        let mut keep = fl;
        if m.u32(e + 0x70) != 0 {
            keep = m.u32(e + 0x50);
            let mut f = keep & 0xffff_f30f;
            m.set_u32(e + 0x50, f);
            if fl & 0x10 == 0 {
                if fl & 0x20 != 0 {
                    f |= 0x410;
                    m.set_u32(e + 0x50, f);
                }
            } else {
                f |= 0x420;
                m.set_u32(e + 0x50, f);
            }
            if fl & 0x40 == 0 {
                if fl & 0x80 != 0 {
                    m.set_u32(e + 0x50, m.u32(e + 0x50) | 0x840);
                }
            } else {
                m.set_u32(e + 0x50, m.u32(e + 0x50) | 0x880);
            }
        }
        let old = (m.u32(e + 0x64), m.u32(e + 0x68));
        let next = m.u32(e + 0x74);
        let pv = m.u32(e + 0x70);
        m.set_u32(e + 0x64, held.0);
        m.set_u32(e + 0x68, held.1);
        m.set_u32(e + 0x70, next);
        m.set_u32(e + 0x74, pv);
        e = next;
        fl = keep;
        held = old;
        if e == first && closed {
            return co;
        }
    }
}

/// [soft 0x0850c930 si_nurbsOrientTraverseNestingTree]: `(elem, flag)`: walks a nesting tree (element: data +0, next +8, child +0x10;
/// the data's word at +0x20 is a trim curve): where `flag` is not 0 reverses the order of the curve's 0x24-byte points and toggles its
/// flag word at +0x38 between bit 2 and bit 4 (bit 2 clear: set 2 and clear 4; else clear 2 and set 4); then the child with the flag
/// inverted and the next sibling with 0.
pub fn orient_traverse_nesting_tree(m: &mut Mem, elem: u32, flag: i16) {
    if elem == 0 {
        return;
    }
    if flag != 0 {
        let data = m.u32(elem) /* SI_genTreeElemGetData (0x08515b68) is the word at +0 */;
        let crv = m.u32(data.wrapping_add(0x20));
        let n = m.i32(crv + 4);
        let half = (n - (n >> 31)) >> 1;
        let pts = m.u32(crv + 0x2c);
        for i in 0..half.max(0) {
            let a = pts.wrapping_add(0x24 * i as u32);
            let b = pts.wrapping_add(0x24u32.wrapping_mul((n - 1 - i) as u32));
            for k in 0..9u32 {
                let (wa, wb) = (m.u32(a + 4 * k), m.u32(b + 4 * k));
                m.set_u32(a + 4 * k, wb);
                m.set_u32(b + 4 * k, wa);
            }
        }
        let f = m.u32(crv + 0x38);
        m.set_u32(crv + 0x38, if f & 2 == 0 { (f & !4) | 2 } else { (f & !2) | 4 });
    }
    let child = m.u32(elem + 0x10);
    if child != 0 {
        orient_traverse_nesting_tree(m, child, (flag == 0) as i16);
    }
    let next = m.u32(elem + 8);
    if next != 0 {
        orient_traverse_nesting_tree(m, next, 0);
    }
}
