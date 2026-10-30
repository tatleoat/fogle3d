// SPDX-License-Identifier: MIT
//! `settings.c` (continued): the records of a model's dynamics: defaults, constants and copies of the chain
//! variables (`dynachnvar`), the joint variables (`dynajntvar`), the matter and the joint properties.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`. A float copy goes through the
//! x87 unit (a signalling NaN comes out quiet), so every float word is copied as `x(v).to_f32()`.

use crate::settings::si_dynachn_deltat_compute;
use si_math::matrix3tools::{matrix3_copy, matrix3_scalar, matrix3_tilde};
use si_math::types::{Matrix3, Vector};
use si_mem::{Mem, Ptr32};
use si_x87::x;

/// `SI_matrix3ID` (0x086dc220).
const ID3: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// `SI_matrix3Null` (0x086dc260).
const NULL3: Matrix3 = [[0.0; 3]; 3];

pub(crate) fn put_m3(m: &mut Mem, a: u32, v: &Matrix3) {
    for (i, row) in v.iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            m.set_f32(a + 4 * (3 * i + j) as u32, *c);
        }
    }
}

pub(crate) fn get_m3(m: &Mem, a: u32) -> Matrix3 {
    let mut o = [[0.0f32; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 4 * (3 * i + j) as u32);
        }
    }
    o
}

/// The float copy of the original: `fld` / `fstp` of one word.
pub(crate) fn fcopy(m: &mut Mem, src: u32, dst: u32) {
    let v = x(m.f32(src)).to_f32();
    m.set_f32(dst, v);
}

/// The defaults of a chain's variables record [soft 0x0849f258 SI_dynachnvarSetDefault]: `word` goes to the short at
/// `+0x38`, the integration switches take their fixed values, the time step is computed
/// (`si_dynachnDeltatCompute`), and the matrices at `+0x70` and `+0x94` become the identity.
pub fn si_dynachnvar_set_default(m: &mut Mem, v: u32, word: i16) {
    m.set_i16(v + 0x38, word);
    m.set_u16(v + 0x3a, 0);
    for (o, val) in [(0x3c, 1u16), (0x3e, 1), (0x40, 1), (0x42, 1), (0x44, 1), (0x46, 1), (0x48, 1), (0x4a, 1), (0x4c, 0), (0x4e, 1), (0x50, 0)] {
        m.set_u16(v + o, val);
    }
    si_dynachn_deltat_compute(m, v);
    for o in [0x2c, 0x30, 0x34] {
        m.set_f32(v + o, 1.0);
    }
    for o in [0x28, 0x24, 0x20, 0x60, 0x6c, 0x68, 0x64] {
        m.set_u32(v + o, 0);
    }
    m.set_u32(v + 0x54, 1);
    m.set_u32(v + 0x58, 1);
    m.set_u16(v + 0x5c, 1);
    m.set_u16(v + 0x5e, 0);
    for o in [0x2, 0x4, 0x8, 0x6, 0x12] {
        m.set_u16(v + o, 0);
    }
    for o in [0x14, 0x18, 0x1c] {
        m.set_u32(v + o, 0);
    }
    put_m3(m, v + 0x70, &matrix3_copy(&ID3));
    put_m3(m, v + 0x94, &matrix3_copy(&ID3));
}

/// The defaults of a body's joint variables [soft 0x0849f44c si_dynajntvarSetDefault]: the vectors at `+0x18c`, `+0x19c`,
/// `+0, +0xc0 ... +0x100`, `+0x1a4`, `+0x268` are zeroed (as words), the short at `+0x1b0` is 1, and the five 3 x 3
/// matrices at `+0xc, +0x30, +0x54, +0x78, +0x9c` are the null matrix.
pub fn si_dynajntvar_set_default(m: &mut Mem, v: u32) {
    for o in [0x194, 0x190, 0x18c, 0x1a0, 0x19c, 0x198] {
        m.set_u32(v + o, 0);
    }
    m.set_u16(v + 0x1b0, 1);
    for o in [0xc, 0x30, 0x54, 0x78, 0x9c] {
        put_m3(m, v + o, &matrix3_copy(&NULL3));
    }
    for o in [0x8, 0x4, 0x0, 0xc8, 0xc4, 0xc0, 0xd4, 0xd0, 0xcc, 0xe0, 0xdc, 0xd8, 0xec, 0xe8, 0xe4, 0xf8, 0xf4, 0xf0, 0x104, 0x100, 0xfc,
        0x1ac, 0x1a8, 0x1a4, 0x270, 0x26c, 0x268] {
        m.set_u32(v + o, 0);
    }
}

/// The constants of one body of a chain [soft 0x0849f604 si_dynajntcteCompute]: `chain` is the head model, `body` a model
/// of it with record `r = [body+0x300]`. With `t` the f32 at `[body+0x304]+0x60` (or, without that record, at
/// `[[body+0x2f4]+8]`): `r+0x250..0x258 = |angle| * scale` of the words `body+0x16c..0x174` and `0x158..0x160`;
/// `r+0x180..0x188 = t * [[chain+0x304]+0x20..0x28]`. With `flag == 0` also the vector `v` at `[body+0x304]+0x64` (or
/// `[[body+0x2f4]]+0x20`): `r+0x174.. = t * v`, the tilde matrix of `v` into a local one, the tilde of `r+0x250..` into
/// `r+0x150`, `t` times the identity into `r+0x108` and `t` times the local tilde into `r+0x12c`.
pub fn si_dynajntcte_compute(m: &mut Mem, chain: u32, body: u32, flag: i16) {
    let r = m.u32(body + 0x300);
    let prop = m.u32(body + 0x304);
    let t = if prop != 0 { m.f32(prop + 0x60) } else { m.f32(m.u32(body + 0x2f4) + 8) };
    let t = x(t).to_f32();
    for (a, b, d) in [(0x16c, 0x158, 0x250), (0x170, 0x15c, 0x254), (0x174, 0x160, 0x258)] {
        let v = (x(m.f32(body + a)).abs() * x(m.f32(body + b))).to_f32();
        m.set_f32(r + d, v);
    }
    for (a, d) in [(0x20, 0x180), (0x24, 0x184), (0x28, 0x188)] {
        let cp = m.u32(chain + 0x304);
        let v = (x(t) * x(m.f32(cp + a))).to_f32();
        m.set_f32(r + d, v);
    }
    if flag == 0 {
        let prop = m.u32(body + 0x304);
        let e = if prop != 0 { prop + 0x64 } else { m.u32(body + 0x2f4) + 0x20 };
        let v = [m.u32(e), m.u32(e + 4), m.u32(e + 8)].map(f32::from_bits);
        for (k, d) in [0x174u32, 0x178, 0x17c].into_iter().enumerate() {
            m.set_f32(r + d, (x(t) * x(v[k])).to_f32());
        }
        let local = matrix3_tilde(Vector { x: v[0], y: v[1], z: v[2] });
        let r2 = m.u32(body + 0x300);
        let w = Vector { x: m.f32(r2 + 0x250), y: m.f32(r2 + 0x254), z: m.f32(r2 + 0x258) };
        put_m3(m, r + 0x150, &matrix3_tilde(w));
        put_m3(m, r + 0x108, &matrix3_scalar(t, &ID3));
        put_m3(m, r + 0x12c, &matrix3_scalar(t, &local));
    }
}

/// The constants of every body of a chain [soft 0x0849f790 SI_dynachncteCompute]: the head is `model` itself when it has a
/// parent link (`+0x30` non-zero), else its first link (`+0x2c4`); [`si_dynajntcte_compute`] for each body along `+0x30c`.
pub fn si_dynachncte_compute(m: &mut Mem, model: u32, flag: i16) {
    let mut b = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while b != 0 {
        si_dynajntcte_compute(m, model, b, flag);
        b = m.u32(b + 0x30c);
    }
}

/// The matter record copied [soft 0x0849fb1c SI_dynamatterCopy] (source, destination): floats `+0x0, +0xc .. +0x1c`, word `+0x80`.
pub fn si_dynamatter_copy(m: &mut Mem, src: u32, dst: u32) {
    for o in [0x0, 0xc, 0x10, 0x14, 0x18, 0x1c] {
        fcopy(m, src + o, dst + o);
    }
    let w = m.u32(src + 0x80);
    m.set_u32(dst + 0x80, w);
}

/// The joint property record copied [soft 0x0849fd34 SI_dynajntpropCopy]: three floats.
pub fn si_dynajntprop_copy(m: &mut Mem, src: u32, dst: u32) {
    for o in [0, 4, 8] {
        fcopy(m, src + o, dst + o);
    }
}

/// A chain's variables record copied [soft 0x0849fd54 SI_dynachnvarCopy] (source, destination, flag): the shorts at
/// `+0x38 .. +0x4c` and `+0x10`, `+0x5c`, `+0x5e`, the floats and words in the order of the original, `+0x2` cleared; with
/// `flag != 0` also `+0x8, +0x6, +0x12` and the words `+0x14 .. +0x1c` cleared; the two matrices at `+0x70` and `+0x94` are
/// copied the wrong way round, from the destination into the source (the original's bug).
pub fn si_dynachnvar_copy(m: &mut Mem, src: u32, dst: u32, flag: i16) {
    for o in [0x38, 0x3a, 0x3c, 0x3e, 0x40, 0x4e, 0x50, 0x42, 0x44, 0x46, 0x48, 0x4a, 0x4c] {
        let w = m.u16(src + o);
        m.set_u16(dst + o, w);
    }
    fcopy(m, src + 0xc, dst + 0xc);
    let w = m.u16(src + 0x10);
    m.set_u16(dst + 0x10, w);
    for o in [0x54, 0x58] {
        let w = m.u32(src + o);
        m.set_u32(dst + o, w);
    }
    for o in [0x5c, 0x5e] {
        let w = m.u16(src + o);
        m.set_u16(dst + o, w);
    }
    m.set_u16(dst + 2, 0);
    if flag != 0 {
        for o in [0x8, 0x6, 0x12] {
            m.set_u16(dst + o, 0);
        }
        for o in [0x14, 0x18, 0x1c] {
            m.set_u32(dst + o, 0);
        }
    }
    for o in [0x2c, 0x30, 0x34, 0x20, 0x24, 0x28, 0x60] {
        fcopy(m, src + o, dst + o);
    }
    for o in [0x64, 0x68, 0x6c] {
        let w = m.u32(src + o);
        m.set_u32(dst + o, w);
    }
    // Original bug kept: the two calls pass their arguments the wrong way round, so the matrices go from the
    // destination record into the source record.
    put_m3(m, src + 0x70, &matrix3_copy(&get_m3(m, dst + 0x70)));
    put_m3(m, src + 0x94, &matrix3_copy(&get_m3(m, dst + 0x94)));
}

/// The bounds of the shapes of every body [soft 0x084a0dbc SI_dynachnObstacleBoundingUpdate]: for each body of the chain (the
/// head as in [`si_dynachncte_compute`], then `+0x30c`), the list of definitions at `[body+0x2fc]` (next link `+0x3c`): each
/// whose kind (`+0x4`) is 9 has the model at its `+0x10` bounded (`SI_modelComputeBounding`).
pub fn si_dynachn_obstacle_bounding_update(m: &mut Mem, model: u32) {
    let mut b = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while b != 0 {
        let mut d = m.u32(b + 0x2fc);
        while d != 0 {
            if m.u32(d + 4) == 9 {
                let target = m.u32(d + 0x10);
                si_model::model_image::bounds::compute_bounding(m, Ptr32::new(target));
            }
            d = m.u32(d + 0x3c);
        }
        b = m.u32(b + 0x30c);
    }
}

/// Switches the algorithm of a model between the kinds [soft 0x084a0e14 SI_dynaAlgoTypeSet] (`+0x2ec`): kind 0 with `flag != 0`
/// becomes 12 when the model's own kind (`+0x30`) is 1, 4, 8 or 10; kind 1 with `flag != 0` becomes 2 and so do all bodies of
/// the chain; kind 2 with `flag == 0` becomes 1 (bodies too); kind 12 with `flag == 0` becomes 0. Anything else is left.
pub fn si_dyna_algo_type_set(m: &mut Mem, model: u32, flag: i16) {
    let all = |m: &mut Mem, v: u32| {
        let mut b = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
        while b != 0 {
            m.set_u32(b + 0x2ec, v);
            b = m.u32(b + 0x30c);
        }
    };
    match m.u32(model + 0x2ec) {
        0 => {
            if flag != 0 && matches!(m.u32(model + 0x30), 1 | 4 | 8 | 10) {
                m.set_u32(model + 0x2ec, 12);
            }
        }
        1 => {
            if flag != 0 {
                m.set_u32(model + 0x2ec, 2);
                all(m, 2);
            }
        }
        2 => {
            if flag == 0 {
                m.set_u32(model + 0x2ec, 1);
                all(m, 1);
            }
        }
        12 => {
            if flag == 0 {
                m.set_u32(model + 0x2ec, 0);
            }
        }
        _ => {}
    }
}

/// Links the bodies of a chain into a list [soft 0x084a0f80 SI_dynachnSetChnListPtrs]: starting from the head, each body gets
/// its first child at `rec+0x260`, `rec+0x264 = 0`, and the first child's record gets the body at `rec+0x25c` (only the first
/// child: the original's sibling loop starts after that store); every joint (kind 5) among the children is queued after the
/// list; the head's own `rec+0x25c` is 0 and the settings short at `[model+0x304]` is cleared. Then `model+0x314` is the last
/// queued body, and the queue is chained both ways with `+0x30c` (next) and `+0x310` (previous). Works only when the model
/// has a first link (`+0x2c4`). The queue is the original's temporary array of `(SI_modelNbSubModels as i16) + 1` words
/// from `SI_malloc` (which marks its first and last byte), freed at the end.
pub fn si_dynachn_set_chn_list_ptrs(m: &mut Mem, model: u32) {
    if m.u32(model + 0x2c4) == 0 {
        return;
    }
    let n = si_scene::edtools::model_nb_sub_models(m, Ptr32::new(model)) as i16 as i32;
    let q = m.si_malloc((n.wrapping_mul(4).wrapping_add(4)) as u32);
    let head = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    m.set_u32(q, head);
    let r = m.u32(head + 0x300);
    m.set_u32(r + 0x25c, 0);
    let prop = m.u32(model + 0x304);
    m.set_u16(prop, 0);
    let (mut i, mut last): (i32, i32) = (-1, 0);
    loop {
        i += 1;
        let cur = m.u32(q.wrapping_add((i as u32).wrapping_mul(4)));
        let mut d = m.u32(cur + 0x2c4);
        let r = m.u32(cur + 0x300);
        m.set_u32(r + 0x260, d);
        let cur = m.u32(q.wrapping_add((i as u32).wrapping_mul(4)));
        let r = m.u32(cur + 0x300);
        m.set_u32(r + 0x264, 0);
        if d != 0 {
            let rc = m.u32(d + 0x300);
            if rc != 0 {
                let c = m.u32(q.wrapping_add((i as u32).wrapping_mul(4)));
                m.set_u32(rc + 0x25c, c);
            }
            loop {
                if m.u32(d + 0x30) == 5 {
                    last += 1;
                    m.set_u32(q.wrapping_add((last as u32).wrapping_mul(4)), d);
                }
                d = m.u32(d + 0x2c0);
                if d == 0 {
                    break;
                }
            }
        }
        if !(i < last) {
            break;
        }
    }
    let at = |k: i32| q.wrapping_add((k as u32).wrapping_mul(4));
    let lastp = m.u32(at(last));
    m.set_u32(model + 0x314, lastp);
    let first = m.u32(q);
    m.set_u32(first + 0x310, 0);
    let lastp = m.u32(at(last));
    m.set_u32(lastp + 0x30c, 0);
    for k in 0..last {
        let a = m.u32(at(k));
        let b = m.u32(at(k + 1));
        m.set_u32(a + 0x30c, b);
    }
    for k in 1..=last {
        let a = m.u32(at(k));
        let b = m.u32(at(k - 1));
        m.set_u32(a + 0x310, b);
    }
    if q != 0 {
        m.free(q);
    }
}

/// Whether a model and the models below and beside it hold no dynamics properties [soft 0x084a1424 SI_hierarchyCheckEmptyPhyProps]
/// (`out` receives a 16-bit flag): a model of kind 0, 1, 4, 5, 8 or 10 that has a property record (`+0x304`) or a shape
/// record (`+0x2f4`) makes the answer 0 and stores into the short at `out` 1 for a kind-0 model whose algorithm (`+0x2ec`)
/// is 1 or 2, else 0 (it stays untouched for every other model). The answer then goes on with the first child (`+0x2c4`) and
/// the next brother (`+0x2c0`) while it stays non-zero; the result is the 16-bit answer, sign-extended.
pub fn si_hierarchy_check_empty_phy_props(m: &mut Mem, model: u32, out: u32) -> i32 {
    let mut ok: i16 = 1;
    let kind = m.u32(model + 0x30);
    if matches!(kind, 4 | 1 | 10 | 8 | 0 | 5) && !(m.u32(model + 0x304) == 0 && m.u32(model + 0x2f4) == 0) {
        ok = 0;
        let mut v: u16 = 0;
        if m.u32(model + 0x30) == 0 {
            v = (m.u32(model + 0x2ec).wrapping_sub(1) <= 1) as u16;
        }
        m.set_u16(out, v);
    }
    if ok == 0 {
        return 0;
    }
    let c = m.u32(model + 0x2c4);
    if c != 0 {
        ok = si_hierarchy_check_empty_phy_props(m, c, out) as i16;
    }
    if ok == 0 {
        return 0;
    }
    let s = m.u32(model + 0x2c0);
    if s != 0 {
        ok = si_hierarchy_check_empty_phy_props(m, s, out) as i16;
    }
    ok as i32
}

/// Whether the dynamics records of a hierarchy agree with what the caller expects [soft 0x084a14cc SI_hierarchyCheckPhyPropsConsistancy]:
/// `a` (a value only tested against 0), the model, two shorts. For a model of kind 1, 4, 8 or 10 with `shape = [model+0x2f4]`:
/// `s4` and `a` non-zero with `shape == 0` turn `s4` into 0; the result is 0 when `s3` is non-zero and there is no property
/// record (`+0x304`) or `s3` is zero and there is one, and 0 when `s4 != 0` and `shape == 0` or `s4 == 0` and `shape != 0`; and
/// then `a` becomes the model itself. For other kinds the result is 0 when `a` is 0. A non-zero result goes on through the
/// first child with `s3 = 0` and the (possibly zeroed) `s4`, and through the next brother with `s3 = 0` and the caller's `s4`.
pub fn si_hierarchy_check_phy_props_consistancy(m: &mut Mem, a: u32, model: u32, s3: i16, s4: i16) -> i32 {
    let (mut a, mut ax) = (a, s4);
    let mut ok: i16 = 1;
    let kind = m.u32(model + 0x30);
    if matches!(kind, 4 | 1 | 10 | 8) {
        let shape = m.u32(model + 0x2f4);
        if a != 0 && s4 != 0 && shape == 0 {
            ax = 0;
        }
        a = model;
        let has_prop = m.u32(model + 0x304) != 0;
        if (s3 != 0 && !has_prop) || (s3 == 0 && has_prop) {
            ok = 0;
        }
        if ax != 0 {
            if shape == 0 {
                return 0;
            }
        } else if shape != 0 {
            return 0;
        }
    } else if a == 0 {
        ok = 0;
    }
    if ok == 0 {
        return 0;
    }
    let c = m.u32(model + 0x2c4);
    if c != 0 {
        ok = si_hierarchy_check_phy_props_consistancy(m, a, c, 0, ax) as i16;
    }
    if ok == 0 {
        return 0;
    }
    let s = m.u32(model + 0x2c0);
    if s != 0 {
        ok = si_hierarchy_check_phy_props_consistancy(m, a, s, 0, s4) as i16;
    }
    ok as i32
}
