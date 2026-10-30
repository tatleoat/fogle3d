// SPDX-License-Identifier: MIT
//! The four-sided NURBS surface (finding 0034, "four1"): a Coons surface made from four curves
//! of four selected models, `si_createFourSidedNurbs` of `surface.c`.
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported (the
//! dialog, the requester) is called through [`si_mem::Outside`], written `name (0xADDRESS)`.
//! The x87 arithmetic follows the unit's order: a sum or product that the compiler kept in a
//! register is an [`F80`] value, one it stored to a stack slot is rounded to a double (`rd`).

use crate::effects::ModelS;
use si_mem::{Mem, Outside, Routine};
use si_model::model_image::{defs, setup};
use si_nurbs::image::Halt;
use si_x87::F80;

/// `si_nurbsFourSidedDialog (0x0819d7d4)`, `surface.c`: the dialog that asks for the
/// parametrisation (UI, a hook; it sets `nurbsParam` and `nurbsOK`).
pub const SI_NURBS_FOUR_SIDED_DIALOG: Routine = Routine::new("si_nurbsFourSidedDialog", 0x0819_d7d4);
/// `SI_dialogRequest (0x08141c00)`, `request.c`: a requester with four lines of text (UI, a hook).
/// [soft 0x084dba5c SI_nurbsSrfDispose] stays a hook here: the original is called with the address of a stack cell.
const SI_NURBS_SRF_DISPOSE: Routine = Routine::new("SI_nurbsSrfDispose", 0x084d_ba5c);
pub const SI_DIALOG_REQUEST: Routine = Routine::new("SI_dialogRequest", 0x0814_1c00);

/// `nurbsParam`: the parametrisation the dialog chose (0 .. 3).
pub const NURBS_PARAM: u32 = 0x0868_dc40;
/// `nurbsOK`: the dialog's answer (a short; 0 is cancel).
pub const NURBS_OK: u32 = 0x0868_dc48;

/// The texts the unit copies or hands to the requester: address, text (the NUL follows).
pub const STRINGS: [(u32, &[u8]); 10] = [
    (0x0868_dd60, b"You need to have visible curve segments to perform this operation."),
    (0x0868_ddc0, b"Two of the opposite curves do not have the same degree."),
    (0x0868_ddf8, b"Some curves are closed."),
    (0x0868_de10, b"Cancel"),
    (0x0868_de17, b"surf"),
    (0x0868_de1c, b"SOFTIMAGE Internal error."),
    (0x0868_de36, b"Ok"),
    (0x0868_de39, b"order "),
    (0x0868_de40, b"properly selected: Please retry using a clock-wise"),
    (0x0868_de80, b"No surface was created because the curves were not"),
];
/// "No surface created." (0x0868deb3).
const NO_SURFACE: u32 = 0x0868_deb3;
/// The name "surf" the new node is allocated with.
const SURF_NAME: u32 = 0x0868_de17;

/// Puts the texts of [`STRINGS`] into the image (the program's loader does it in the original).
pub fn store_names(m: &mut Mem) {
    for (a, s) in STRINGS {
        m.put_cstr(a, s);
    }
    m.put_cstr(NO_SURFACE, b"No surface created.");
}

/// A double stored to a stack slot and loaded again.
fn rd(x: F80) -> F80 {
    F80::from_f64(x.to_f64())
}

fn ld(m: &Mem, p: u32, k: u32) -> F80 {
    F80::from_f64(m.f64(p.wrapping_add(8 * k)))
}

/// `(a - b) * (a - b)` in the registers.
fn sq(a: f64, b: f64) -> F80 {
    let d = F80::from_f64(a) - F80::from_f64(b);
    d * d
}

/// Writes `n` bytes of the text at `from` into the caller's message buffer (the original's `rep movs`).
fn put_msg(m: &mut Mem, msg: u32, from: u32, n: u32) {
    m.copy(msg, from, n);
}

/// The four curves are disposed through the cells they were kept in (`SI_nurbsCrvDispose(&cell)`).
fn dispose_cells(m: &mut Mem, cells: u32) {
    for k in 0..4 {
        si_hrc::image::nurbs::nurbs_crv_dispose(m, cells + 4 * k);
    }
}

/// A point of a curve's control polygon (0x24 bytes each): the first, or the last of `nb`.
fn point(m: &Mem, c: u32, last: bool) -> u32 {
    let pts = m.u32(c.wrapping_add(0x2c));
    if !last {
        return pts;
    }
    let nb = m.i32(c.wrapping_add(4));
    pts.wrapping_add((nb.wrapping_add(nb.wrapping_mul(8)) as u32).wrapping_mul(4)).wrapping_sub(0x24)
}

/// A point divided by its weight: x/w, y/w, z/w (the registers' values).
fn unit(m: &Mem, p: u32) -> [F80; 3] {
    let w = ld(m, p, 3);
    [ld(m, p, 0) / w, ld(m, p, 1) / w, ld(m, p, 2) / w]
}

/// The two end points of a pair of corners moved to their common place. `ex` is the mean x as the
/// registers hold it (the first point gets it unrounded), `e8` the same stored as a double (the
/// second point gets that), `e0` and `d8` the mean y and z stored. The first point is written
/// fully before the second one reads its weight again, so the second sees the first's new weight.
fn join(m: &mut Mem, pa: u32, pb: u32, ex: F80, e8: F80, e0: F80, d8: F80) {
    let half = F80::from_f64(0.5);
    let w = |m: &Mem, p: u32| ld(m, p, 3);
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pa, ((ex * s) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pa.wrapping_add(8), ((s * e0) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pa.wrapping_add(0x10), ((s * d8) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pa.wrapping_add(0x18), (s * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pb, ((s * e8) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pb.wrapping_add(8), ((s * e0) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pb.wrapping_add(0x10), ((s * d8) * half).to_f64());
    let s = w(m, pa) + w(m, pb);
    m.set_f64(pb.wrapping_add(0x18), (half * s).to_f64());
}

/// `si_createFourSidedNurbs (0x0819d954)`, `surface.c`: makes a NURBS surface from four curves
/// (`models[0..4]`, each model's curve at +0x48 brought to the world by the model's matrix at
/// +0x1c4), in clock-wise order, and puts it into `*model_at` (a new node "surf" when it is null).
/// `msg` receives the reason of a failure; `dialog` != 0 asks for the parametrisation through
/// `si_nurbsFourSidedDialog (0x0819d7d4)`, else `**data` is it. Answers 1 when a surface was made,
/// else 0.
///
/// Where the original reads memory nothing wrote (after the inversion of the third curve it
/// copies 24 bytes of an unwritten stack slot over that curve's end point, which is not used
/// again), the Rust leaves the point alone.
pub fn create_four_sided_nurbs(m: &mut Mem, o: &mut dyn Outside, models: u32, model_at: u32, msg: u32, dialog: i16, data: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = create_inner(m, o, models, model_at, msg, dialog, data);
    m.set_sp(sp);
    r
}

fn create_inner(m: &mut Mem, o: &mut dyn Outside, models: u32, model_at: u32, msg: u32, dialog: i16, data: u32) -> Result<u32, Halt> {
    // the cells the copies are kept in: the first one at the lowest address
    let cells = m.local(16);
    let mut k = 0u32;
    while k <= 3 {
        let model = m.u32(models.wrapping_add(4 * k));
        let c = si_nurbs::image::crv_copy(m, m.u32(model.wrapping_add(0x48)));
        m.set_u32(cells + 4 * k, c);
        let model = m.u32(models.wrapping_add(4 * k));
        si_nurbs::image::crv_apply_matrix(m, c, model.wrapping_add(0x1c4), 0);
        k += 1;
    }
    let cp = [m.u32(cells), m.u32(cells + 4), m.u32(cells + 8), m.u32(cells + 12)];
    if (0..4).any(|k| m.i32(cp[k] + 4) < m.i32(cp[k])) {
        put_msg(m, msg, STRINGS[0].0, 67);
        dispose_cells(m, cells);
        return Ok(0);
    }
    if m.i32(cp[0]) != m.i32(cp[2]) || m.i32(cp[1]) != m.i32(cp[3]) {
        put_msg(m, msg, STRINGS[1].0, 56);
        dispose_cells(m, cells);
        return Ok(0);
    }
    if m.i16(cp[0] + 0x20) != 0 || m.i16(cp[2] + 0x20) != 0 || m.i16(cp[1] + 0x20) != m.i16(cp[3] + 0x20) {
        put_msg(m, msg, STRINGS[2].0, 24);
        dispose_cells(m, cells);
        return Ok(0);
    }

    if dialog != 0 {
        o.call(m, SI_NURBS_FOUR_SIDED_DIALOG, &[data]);
    } else {
        let p = m.u32(data);
        let v = m.u32(p);
        m.set_u32(NURBS_PARAM, v);
    }
    if m.i16(NURBS_OK) == 0 {
        dispose_cells(m, cells);
        put_msg(m, msg, STRINGS[3].0, 7);
        return Ok(0);
    }

    let mut node = m.u32(model_at);
    let mut existing = true;
    if node == 0 {
        node = si_hrc::image::node::model_allocate(m, SURF_NAME).addr();
        existing = false;
    }
    let (mut c0, mut c1, mut c2, mut c3) = (cp[0], cp[1], cp[2], cp[3]);

    // the end points of the curves, at the ends of their parameter range
    let mut p: [[f64; 3]; 8] = [[0.0; 3]; 8];
    for (k, c) in [c0, c1, c2, c3].into_iter().enumerate() {
        let t0 = m.f64(c + 0x10);
        p[2 * k] = si_nurbs::image::crv_value(m, c, t0, 0)?;
        let t1 = m.f64(c + 0x18);
        p[2 * k + 1] = si_nurbs::image::crv_value(m, c, t1, 0)?;
    }
    let (p0s, p0e, p1s, p1e, p2s, p2e, p3s, p3e) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);

    // the third curve runs against the first when its start is the farther from the first's start
    // and its end the farther from the first's end
    let a = ((sq(p0s[0], p2s[0]) + sq(p0s[1], p2s[1])) + sq(p0s[2], p2s[2])).sqrt();
    let b = ((sq(p0s[0], p2e[0]) + sq(p0s[1], p2e[1])) + sq(p0s[2], p2e[2])).sqrt();
    if a.gt(b) {
        let a = (sq(p0e[2], p2e[2]) + (sq(p0e[1], p2e[1]) + sq(p0e[0], p2e[0]))).sqrt();
        let b = ((sq(p0e[1], p2s[1]) + sq(p0e[0], p2s[0])) + sq(p0e[2], p2s[2])).sqrt();
        if a.gt(b) {
            si_nurbs::image::crv_inverse(m, o, c2);
        }
    }
    // the same for the fourth against the second (its end points are swapped afterwards, and not read again)
    let a = (sq(p1s[2], p3s[2]) + (sq(p1s[1], p3s[1]) + sq(p1s[0], p3s[0]))).sqrt();
    let b = ((sq(p1s[1], p3e[1]) + sq(p1s[0], p3e[0])) + sq(p1s[2], p3e[2])).sqrt();
    if a.gt(b) {
        let a = (sq(p1e[2], p3e[2]) + (sq(p1e[1], p3e[1]) + sq(p1e[0], p3e[0]))).sqrt();
        let b = ((sq(p1e[1], p3s[1]) + sq(p1e[0], p3s[0])) + sq(p1e[2], p3s[2])).sqrt();
        if a.gt(b) {
            si_nurbs::image::crv_inverse(m, o, c3);
        }
    }

    // the four distances between the ends of the first and the second curve, each stored as a double
    let d1 = (((sq(p0e[1], p1s[1]) + sq(p0e[0], p1s[0])) + sq(p0e[2], p1s[2])).sqrt()).to_f64();
    let r = rd(sq(p0s[0], p1s[0]));
    let r = rd(sq(p0s[1], p1s[1]) + r);
    let d3 = (sq(p0s[2], p1s[2]) + r).sqrt().to_f64();
    let r = rd(sq(p0e[1], p1e[1]) + sq(p0e[0], p1e[0]));
    let d5 = (sq(p0e[2], p1e[2]) + r).sqrt().to_f64();
    let r = rd(sq(p0s[0], p1e[0]));
    let r = rd(sq(p0s[1], p1e[1]) + r);
    let d4 = (sq(p0s[2], p1e[2]) + r).sqrt().to_f64();
    if d1 < d3 && d1 < d5 {
        si_nurbs::image::crv_inverse(m, o, c0);
        si_nurbs::image::crv_inverse(m, o, c2);
    } else if d4 < d5 && d4 < d3 {
        si_nurbs::image::crv_inverse(m, o, c1);
        si_nurbs::image::crv_inverse(m, o, c3);
    } else if d5 < d4 && d5 < d1 {
        for c in [c0, c1, c2, c3] {
            si_nurbs::image::crv_inverse(m, o, c);
        }
    }

    // opposite curves get the same number of points
    if m.i32(c0 + 4) != m.i32(c2 + 4) {
        let pair = m.local(8);
        m.set_u32(pair, c0);
        m.set_u32(pair + 4, c2);
        si_nurbs::blend::nurbs_crv_make_compatible(m, o, pair, 2);
        c0 = m.u32(pair);
        c2 = m.u32(pair + 4);
    }
    if m.i32(c1 + 4) != m.i32(c3 + 4) {
        let pair = m.local(8);
        m.set_u32(pair, c1);
        m.set_u32(pair + 4, c3);
        si_nurbs::blend::nurbs_crv_make_compatible(m, o, pair, 2);
        c1 = m.u32(pair);
        c3 = m.u32(pair + 4);
    }

    // the corners of the four curves are made to meet
    let (f0, l0) = (point(m, c0, false), point(m, c0, true));
    let (f1, l1) = (point(m, c1, false), point(m, c1, true));
    let (f2, l2) = (point(m, c2, false), point(m, c2, true));
    let (f3, l3) = (point(m, c3, false), point(m, c3, true));
    // the divided points: the first points as the registers hold them, the others stored as doubles
    let u_f0 = unit(m, f0);
    let u_f1 = unit(m, f1);
    let stored = |u: [F80; 3]| u.map(rd);
    let (u_l0, u_l1) = (stored(unit(m, l0)), stored(unit(m, l1)));
    let (u_f2, u_l2) = (stored(unit(m, f2)), stored(unit(m, l2)));
    let (u_f3, u_l3) = (stored(unit(m, f3)), stored(unit(m, l3)));
    let half = F80::from_f64(0.5);
    let near = F80::from_f64(0.0001);
    let mean = |a: [F80; 3], b: [F80; 3]| -> (F80, F80, F80) { ((a[0] + b[0]) * half, rd((a[1] + b[1]) * half), rd((a[2] + b[2]) * half)) };
    // the first curve's start with the second curve's start (registers)
    {
        let dist = (((u_f0[0] - u_f1[0]) * (u_f0[0] - u_f1[0]) + (u_f0[1] - u_f1[1]) * (u_f0[1] - u_f1[1])) + (u_f0[2] - u_f1[2]) * (u_f0[2] - u_f1[2])).sqrt();
        if dist.gt(near) {
            let (ex, e0, d8) = mean(u_f1, u_f0);
            join(m, f0, f1, ex, rd(ex), e0, d8);
        }
    }
    // the second curve's end with the third curve's start
    {
        let (a, b) = (u_l1, u_f2);
        let dist = (((a[1] - b[1]) * (a[1] - b[1]) + (a[0] - b[0]) * (a[0] - b[0])) + (a[2] - b[2]) * (a[2] - b[2])).sqrt();
        if dist.gt(near) {
            let (ex, e0, d8) = mean(b, a);
            join(m, l1, f2, ex, rd(ex), e0, d8);
        }
    }
    // the third curve's end with the fourth curve's end
    {
        let (a, b) = (u_l2, u_l3);
        let dist = (((a[1] - b[1]) * (a[1] - b[1]) + (a[0] - b[0]) * (a[0] - b[0])) + (a[2] - b[2]) * (a[2] - b[2])).sqrt();
        if dist.gt(near) {
            let (ex, e0, d8) = mean(b, a);
            join(m, l2, l3, ex, rd(ex), e0, d8);
        }
    }
    // the first curve's end with the fourth curve's start
    {
        let (a, b) = (u_l0, u_f3);
        let dist = (((a[1] - b[1]) * (a[1] - b[1]) + (a[0] - b[0]) * (a[0] - b[0])) + (a[2] - b[2]) * (a[2] - b[2])).sqrt();
        if dist.gt(near) {
            let (ex, e0, d8) = mean(b, a);
            join(m, f3, l0, ex, rd(ex), e0, d8);
        }
    }

    // the tables (the knot vectors) are given back; the reparametrisation below makes new ones
    for c in [c0, c1, c2, c3] {
        let t = m.u32(c + 0xc);
        if t != 0 {
            m.free(t);
            m.set_u32(c + 0xc, 0);
        }
    }

    let srf = si_hrc::image::nurbs::nurbs_srf_allocate(m).addr();
    if srf == 0 {
        put_msg(m, msg, STRINGS[5].0, 26);
        return Ok(0);
    }

    let mode: Option<u32> = match m.i32(NURBS_PARAM) {
        0 => Some(0),
        1 => Some(3),
        2 => Some(4),
        3 => Some(2),
        _ => None,
    };
    if let Some(mode) = mode {
        for c in [c0, c1, c2, c3] {
            m.set_u32(c + 0x24, mode);
        }
        for c in [c0, c1, c2, c3] {
            si_nurbs::image::knots_crv_reparametrization(m, c)?;
        }
        m.set_u32(srf + 0x4c, mode);
    }

    // the first direction from the first curve, the second from the second
    for (c, d, e) in [(c0, 0u32, 0u32), (c1, 0x24, 4)] {
        m.set_u32(srf + d, m.u32(c));
        m.set_u32(srf + 0x70 + e, m.u32(c + 0x34));
        let closed = m.i16(c + 0x20);
        m.set_i16(srf + 0x20 + d, closed);
        let nb = m.i32(c + 4);
        m.set_u32(srf + 4 + d, nb as u32);
        let n = if closed == 0 { m.i32(c).wrapping_add(nb).wrapping_sub(2) } else { nb.wrapping_add(1) };
        m.set_u32(srf + 8 + d, n as u32);
        let knots = si_hrc::image::nurbs::knots_allocate(m, n);
        m.set_u32(srf + 0xc + d, knots);
        let mut i = 0i32;
        while i < m.i32(srf + 8 + d) {
            let v = F80::from_f64(m.f64(m.u32(c + 0xc).wrapping_add((i as u32).wrapping_mul(8)))).to_f64();
            m.set_f64(m.u32(srf + 0xc + d).wrapping_add((i as u32).wrapping_mul(8)), v);
            i = i.wrapping_add(1);
        }
        for q in [0x10u32, 0x18] {
            let v = F80::from_f64(m.f64(c + q)).to_f64();
            m.set_f64(srf + q + d, v);
        }
    }
    let w = m.u16(c0 + 0x28);
    m.set_u16(srf + 0x50, w);
    let count = m.i32(srf + 4).wrapping_mul(m.i32(srf + 0x28));
    m.set_i16(srf + 0x5c, 0);
    m.set_u32(srf + 0x60, 0);
    m.set_u32(srf + 0x48, count as u32);
    let pts = si_hrc::image::nurbs::nurbs_points_allocate(m, count);
    m.set_u32(srf + 0x54, pts);

    // the Coons patch: the two ruled surfaces between the opposite curves, less the bilinear one
    // of the corners
    let nvm1 = m.i32(srf + 0x28).wrapping_sub(1);
    let num1 = m.i32(srf + 4).wrapping_sub(1);
    let mut bad = false;
    let (pt0, pt1, pt2, pt3) = (m.u32(c0 + 0x2c), m.u32(c1 + 0x2c), m.u32(c2 + 0x2c), m.u32(c3 + 0x2c));
    let one = F80::ONE;
    let mut jj = 0i32;
    while jj <= nvm1 {
        let a_d = rd(F80::from_i32(jj) / F80::from_i32(nvm1));
        let a = a_d;
        let b = rd(one - a);
        let mut ii = 0i32;
        while ii <= num1 {
            let t12 = F80::from_i32(ii) / F80::from_i32(num1);
            let c = rd(t12);
            let dd = one - t12;
            let i_off = (ii as u32).wrapping_mul(0x24);
            let j_off = (jj as u32).wrapping_mul(0x24);
            let p0 = pt0.wrapping_add(i_off);
            let p2 = pt2.wrapping_add(i_off);
            let p1 = pt1.wrapping_add(j_off);
            let p3 = pt3.wrapping_add(j_off);
            let t = |k: u32| b * ld(m, p0, k);
            let (t13, t14, t15, t16) = (t(0), t(1), t(2), t(3));
            let u = |k: u32| a * ld(m, p2, k);
            let (t17, t18, t19, t20) = (u(0), u(1), u(2), u(3));
            let v = |k: u32| ld(m, p1, k) * dd;
            let (t21, t22, t23, t24) = (v(0), v(1), v(2), v(3));
            let t26 = (t13 + rd(t17)) + rd(t21);
            let l1e0 = rd((t14 + rd(t18)) + rd(t22));
            let l1d8 = rd(t23 + (t15 + rd(t19)));
            let l1d0 = rd(t24 + (t16 + rd(t20)));
            let w3 = |k: u32| rd(c * ld(m, p3, k));
            let (l380, l258, l250, l248) = (w3(0), w3(1), w3(2), w3(3));
            let out = m.u32(srf + 0x54).wrapping_add((num1.wrapping_add(1).wrapping_mul(jj).wrapping_add(ii) as u32).wrapping_mul(0x24));
            m.set_f64(out, (t26 + l380).to_f64());
            m.set_f64(out + 8, (l1e0 + l258).to_f64());
            m.set_f64(out + 0x10, (l1d8 + l250).to_f64());
            m.set_f64(out + 0x18, (l1d0 + l248).to_f64());

            // the bilinear part of the four corners, taken away
            let first0 = pt0;
            let t27 = b * ld(m, first0, 0);
            let t28 = b * ld(m, first0, 1);
            let t29 = b * ld(m, first0, 2);
            let t30 = t27 * dd;
            let t31 = t28 * dd;
            let t32 = t29 * dd;
            let t33 = (b * ld(m, first0, 3)) * dd;
            let first3 = pt3;
            let t34 = b * ld(m, first3, 0);
            let t35 = b * ld(m, first3, 1);
            let t36 = b * ld(m, first3, 2);
            let t37 = t34 * c;
            let t38 = t35 * c;
            let t39 = t36 * c;
            let t40 = (b * ld(m, first3, 3)) * c;
            let first2 = pt2;
            let t41 = ld(m, first2, 0) * dd;
            let t42 = ld(m, first2, 1) * dd;
            let t43 = ld(m, first2, 2) * dd;
            let t44 = t41 * a;
            let t45 = t42 * a;
            let t46 = t43 * a;
            let t47 = (dd * ld(m, first2, 3)) * a;
            let last3 = pt3.wrapping_add((nvm1 as u32).wrapping_mul(0x24));
            let t48 = c * ld(m, last3, 0);
            let t49 = c * ld(m, last3, 1);
            let t50 = c * ld(m, last3, 2);
            let t51 = (c * ld(m, last3, 3)) * a;
            let t52 = t50 * a;
            let corr_x = ((t30 + rd(t37)) + rd(t44)) + t48 * a;
            let corr_y = rd(((t31 + rd(t38)) + rd(t45)) + t49 * a);
            let corr_z = rd(((t32 + rd(t39)) + rd(t46)) + t52);
            let t53 = t47 + (rd(t33) + rd(t40));
            let corr_w = rd(t53 + rd(t51));
            let x = (ld(m, out, 0) - corr_x).to_f64();
            m.set_f64(out, x);
            let y = (ld(m, out, 1) - corr_y).to_f64();
            m.set_f64(out + 8, y);
            let z = (ld(m, out, 2) - corr_z).to_f64();
            m.set_f64(out + 0x10, z);
            let wv = (ld(m, out, 3) - corr_w).to_f64();
            m.set_f64(out + 0x18, wv);
            if !(x.is_finite() && y.is_finite() && z.is_finite() && wv.is_finite() && 0.0 < wv) {
                bad = true;
            }
            ii = ii.wrapping_add(1);
        }
        jj = jj.wrapping_add(1);
    }

    if bad {
        o.call(m, SI_DIALOG_REQUEST, &[0x0868_de80, 0x0868_de40, 0x0868_de39, 0x0868_de36]);
        // the curves are disposed through the cells of the copies (which MakeCompatible may have replaced)
        dispose_cells(m, cells);
        let cell = m.local(4);
        m.set_u32(cell, srf);
        o.call(m, SI_NURBS_SRF_DISPOSE, &[cell]);
        put_msg(m, msg, NO_SURFACE, 20);
        return Ok(0);
    }

    if existing {
        defs::substitute_idefinition(m, &mut ModelS(o), si_mem::Ptr32::new(node), 0, srf);
    } else {
        setup::add_definition(m, si_mem::Ptr32::new(node), 10, srf);
    }
    m.set_u32(node + 0x30, 10);
    m.set_u32(model_at, node);
    let done = m.local(16);
    for (k, c) in [c0, c1, c2, c3].into_iter().enumerate() {
        m.set_u32(done + 4 * k as u32, c);
    }
    dispose_cells(m, done);
    Ok(1)
}
