// SPDX-License-Identifier: MIT
//! Lattice keys: `mot_lattice.c` (finding 0017, "Lattice keys").
//!
//! A model keeps, for the node and for the branch, an array of lattices: entry 0 is the
//! **working lattice** (the one on screen, which the update writes its interpolation into);
//! the entries from 1 on are the **lattice keys**. A curve of the lattice's element
//! (relation 211 node, 216 branch; types 66, 67) maps time to a position among the keys: a
//! key of value v stands for lattice key v + 1. The model's word +0x5c is the key that was
//! last copied into the working lattice (0: none).

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::fcurve as fo;
use crate::reset::{self, ResetHost};
use crate::update::geom::GeomHost;
use si_fcurve::Key;
use si_x87::{fmov, Round, F80};

/// Fields of a model for its lattices (**new**: the key counts and the current key).
pub mod mo {
    /// 16 bits, unsigned: number of lattices of the node (working lattice + keys), and the
    /// array of pointers to them.
    pub const NODE_COUNT: u32 = 0x4c;
    pub const NODE_LATTICES: u32 = 0x50;
    /// The same for the branch.
    pub const BRANCH_COUNT: u32 = 0x54;
    pub const BRANCH_LATTICES: u32 = 0x58;
    /// 16 bits, unsigned: the lattice key shown (1 …), 0 for none.
    pub const CURRENT_KEY: u32 = 0x5c;
}

/// Fields of `SI_Lattice` that [`model_copy_lattice`] uses (**new**).
pub mod lat {
    /// Three 16-bit point counts at +2, +4, +6.
    pub const NX: u32 = 0x02;
    pub const NY: u32 = 0x04;
    pub const NZ: u32 = 0x06;
    /// Array of points, three floats each.
    pub const POINTS: u32 = 0x14;
    /// Array of one 16-bit word per point.
    pub const FLAGS: u32 = 0x18;
}

/// Texts of the "replace or add" question and of the refusal to remove.
pub mod text {
    use si_mem::Addr;
    /// "A lattice key exists for the current frame."
    pub const KEY_EXISTS: Addr = 0x0869_9c60;
    /// "with new lattice or add anyway?"
    pub const OR_ADD: Addr = 0x0869_9c40;
    /// "Replace", "Add", "Cancel".
    pub const REPLACE: Addr = 0x0869_9c1f;
    pub const ADD: Addr = 0x0869_9c1b;
    pub const CANCEL: Addr = 0x0869_9c14;
    /// "Use nxt/prv key to select the current lattice key, key not removed"
    pub const SELECT_FIRST: Addr = 0x0869_9ca0;
}

crate::host_trait! {
    /// What `mot_lattice.c` calls outside this package.
    pub trait LatticeHost: GeomHost, ResetHost {
        /// `SI_latticeTouch(model, branch)`: the lattice changed; the deformation is redone.
        fn lattice_touch(mdl: Addr, branch: i16);
        /// `SI_latticeCopy(lattice)`: a new copy.
        fn lattice_copy(lattice: Addr) -> Addr;
        /// `SI_latticeTransfer(from, to)`: the contents of one lattice into another.
        fn lattice_transfer(from: Addr, to: Addr);
        /// `SI_modelAddILattice(model, branch, index, lattice)`: inserts a lattice key.
        fn model_add_i_lattice(mdl: Addr, branch: i16, index: i16, lattice: Addr);
        /// `SI_modelDeleteILattice(model, branch, index)`.
        fn model_delete_i_lattice(mdl: Addr, branch: i16, index: i16);
        /// `SI_dialogRequest3(line 1, line 2, line 3, button 0, button 1, button 2)`: the
        /// number of the button pressed.
        fn dialog_request3(a: Addr, b: Addr, c: Addr, b0: Addr, b1: Addr, b2: Addr) -> i16;
    }
}

fn lattices(m: &Mem, mdl: Addr, branch: bool) -> (u16, Addr) {
    if branch {
        (m.u16(mdl + mo::BRANCH_COUNT), m.ptr(mdl + mo::BRANCH_LATTICES))
    } else {
        (m.u16(mdl + mo::NODE_COUNT), m.ptr(mdl + mo::NODE_LATTICES))
    }
}

/// Shows the next (`dir` = 1) or the previous lattice key [soft 0x08248940 si_modelSetLattice]:
/// that key is copied into the working lattice ([`model_copy_lattice`]), becomes the
/// current key (+0x5c), and the lattice is touched. Returns the key's number less one, −1
/// for a null model.
///
/// From key c of n lattices: next is c + 1, or 1 after the last (c = n − 1); previous is
/// c − 1, or n − 1 before the first. With no current key (0, or a number not below n) the
/// walk starts as if from 0 (next: key 1) or from 2 (previous: key 1).
pub fn model_set_lattice<H: LatticeHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, dir: i32, branch: i32) -> i32 {
    if mdl == 0 {
        return -1;
    }
    let (n, array) = lattices(m, mdl, branch == 1);
    let n = n as u32;
    let mut cur = m.u16(mdl + mo::CURRENT_KEY) as u32;
    if cur == 0 || n <= cur {
        cur = (dir != 1) as u32 * 2;
    }
    let k = if dir == 1 {
        if cur == n.wrapping_sub(1) { 1 } else { cur + 1 }
    } else if cur == 1 {
        n.wrapping_sub(1)
    } else {
        cur.wrapping_sub(1)
    };
    let (dst, src) = (m.ptr(array), m.ptr(array.wrapping_add(k.wrapping_mul(4))));
    model_copy_lattice(m, dst, src);
    m.set_u16(mdl + mo::CURRENT_KEY, k as u16);
    h.lattice_touch(m, mdl, branch as i16);
    k.wrapping_sub(1) as i32
}

/// A lattice key at the current frame [soft 0x082489f4 si_latticeAddKey].
///
/// The lattice's curve is looked up or created (`SI_fcurveCheckAdd1`, relation 211 for the
/// node, 216 for the branch).
///
/// - **No key yet**: a copy of the working lattice becomes lattice key 1 and the curve gets
///   a key of value 0.
/// - **A curve key within the key tolerance of the current frame** (|frame − time × rate| <
///   0.15 frame): the user is asked "A lattice key exists for the current frame. Replace
///   lattice #n with new lattice or add anyway?" (n = the key's value + 0.5, truncated).
///   *Replace*: the working lattice is transferred into lattice key n + 1, nothing else.
///   *Cancel*: nothing. *Add*: as below.
/// - **Otherwise**: the new key's number s is 0 before the first curve key, else the
///   curve's value at the frame, truncated, plus 1. A copy of the working lattice is
///   inserted as lattice key s + 1, every curve value above s − 1 moves up by 1, and the
///   curve gets a key of value s.
pub fn lattice_add_key<H: LatticeHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, branch: i16) {
    if mdl == 0 {
        return;
    }
    let idx = m.local(4);
    let e = h.ed_get_elem_from_lattice(m, mdl, branch, idx);
    let index = m.i32(idx);
    m.release(4);
    if e == 0 {
        return;
    }
    let curve = key::fcurve_check_add1(m, h, e, index, if branch != 0 { 0xd8 } else { 0xd3 });
    let frame = m.i32(glob::SI_FRAME_CURRENT);
    let working = |m: &Mem| m.ptr(lattices(m, mdl, branch != 0).1);
    let value: f32;
    if m.i32(curve + fo::NB_KEYS) < 1 {
        let w = working(m);
        let copy = h.lattice_copy(m, w);
        h.model_add_i_lattice(m, mdl, branch, 1, copy);
        value = 0.0;
    } else {
        let mut i: i16 = 0;
        while (i as i32) < m.i32(curve + fo::NB_KEYS) {
            let k = m.ptr(curve + fo::KEYS).wrapping_add((i as i32 as u32).wrapping_mul(0x24));
            // SI_secAsFrame(time): time × rate, unrounded
            let at = F80::from_f32(fmov(m.f32(k))) * glob::frame_rate(m);
            if (F80::from_i32(m.i32(glob::SI_FRAME_CURRENT)) - at).abs().lt(F80::from_f32(m.f32(glob::SI_GLB_ANIM + glob::anim::KEY_TOLERANCE))) {
                let n = (F80::from_f32(m.f32(k + 4)) + F80::from_f64(0.5)).to_i32(Round::Zero) as i16;
                let mut line = format!("Replace lattice #{}", n as i32).into_bytes();
                line.push(0);
                let buf = m.local(40);
                m.set_bytes(buf, &line);
                let answer = h.dialog_request3(m, text::KEY_EXISTS, buf, text::OR_ADD, text::REPLACE, text::ADD, text::CANCEL);
                m.release(40);
                if answer == 0 {
                    let array = lattices(m, mdl, branch != 0).1;
                    let (from, to) = (m.ptr(array), m.ptr(array.wrapping_add(((n as i32 + 1) as u32).wrapping_mul(4))));
                    h.lattice_transfer(m, from, to);
                    return;
                }
                if answer == 2 {
                    return;
                }
                break;
            }
            i = i.wrapping_add(1);
        }
        let env = fcv::env(m);
        let first = fcv::load_key(m, m.ptr(curve + fo::KEYS)).frame(&env);
        let s: i16 = if frame < first {
            0
        } else {
            let mut e2 = fcv::env(m);
            let mut f = fcv::load(m, curve);
            let v = f.frame_value(frame, &mut e2).map(fcv::f80).unwrap_or(F80::ZERO);
            m.set_f32(curve + fo::LAST_TIME, f.last_time);
            m.set_f32(curve + fo::LAST_VALUE, f.last_value);
            m.set_i32(curve + fo::CACHE, f.cache);
            (v.to_i32(Round::Zero) as i16).wrapping_add(1)
        };
        let w = working(m);
        let copy = h.lattice_copy(m, w);
        h.model_add_i_lattice(m, mdl, branch, s.wrapping_add(1), copy);
        let above = (F80::from_i32(s as i32) - F80::ONE).to_f32();
        fcv::Ops::on(m, h, curve, |f, env| f.inc_value_above(above, 1.0, env));
        value = F80::from_i32(s as i32).to_f32();
    }
    let now = m.i32(glob::SI_FRAME_CURRENT);
    fcv::Ops::on(m, h, curve, |f, env| {
        let k = Key::frame_create(now, value, 0.0, 0, f.interp, env);
        f.add_key(k, env);
    });
}

/// Removes the current lattice key [soft 0x08248cb8 si_latticeRemoveKey]. Returns the key's
/// number less one, or −1 when nothing was removed.
///
/// Needs the lattice's element and its curve (relation 211 or 216 in list 2). The current
/// key c (+0x5c) must be a key (0 < c < number of lattices), else the status line says "Use
/// nxt/prv key to select the current lattice key, key not removed". The lattice key is
/// deleted (`SI_modelDeleteILattice`). If the curve has fewer than two keys or only one
/// lattice key was left (two lattices), the whole animation goes: the keys, the curve's
/// element (`SI_edFcvDispose(working lattice, LATTICES, 2, relation, relation)`) and the
/// curve. Otherwise the curve keys with a value between c − 1 and c are removed and every
/// value above c − 1 moves down by 1. The lattice is touched.
pub fn lattice_remove_key<H: LatticeHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, branch: i32) -> i32 {
    if mdl == 0 {
        return -1;
    }
    let cur = m.u16(mdl + mo::CURRENT_KEY);
    let (n, array) = lattices(m, mdl, branch == 1);
    let working = if array != 0 { m.ptr(array) } else { 0 };
    let rel = if branch == 1 { 0xd8 } else { 0xd3 };
    let idx = m.local(4);
    let e = h.ed_get_elem_from_lattice(m, mdl, branch as i16, idx);
    m.release(4);
    if e == 0 {
        return -1;
    }
    let curve = ed::find_fcv(m, e, 2, rel);
    if curve == 0 {
        return -1;
    }
    if cur == 0 || cur >= n {
        KeyHost::status_display_message(h, m, text::SELECT_FIRST, 2);
        return -1;
    }
    h.model_delete_i_lattice(m, mdl, branch as i16, cur as i16);
    if m.i32(curve + fo::NB_KEYS) < 2 || n == 2 {
        fcv::Ops::on(m, h, curve, |f, env| f.remove_all_keys(env));
        let lattices_chap = glob::chap(m, cid::LATTICES);
        h.ed_fcv_dispose(m, working, lattices_chap, 2, rel, rel);
        reset::fcurve_remove(m, h, curve);
    } else {
        let hi = F80::from_i32(cur as i32);
        let lo = hi - F80::ONE;
        fcv::Ops::on(m, h, curve, |f, env| {
            f.remove_key_between(lo.to_f32(), hi.to_f32(), env);
            f.inc_value_above(lo.to_f32(), -1.0, env);
        });
    }
    h.lattice_touch(m, mdl, branch as i16);
    cur as i32 - 1
}

/// Copies the points of one lattice into another of the same size
/// [soft 0x08248e24 modelCopyLattice]: when both exist and agree in their first count (+2)
/// and in the 32 bits at +4 (the other two counts), nx × ny × nz points (three words each,
/// as integers) and as many 16-bit words of the second array.
pub fn model_copy_lattice(m: &mut Mem, dst: Addr, src: Addr) {
    if dst == 0 || src == 0 || m.i16(dst + lat::NX) != m.i16(src + lat::NX) || m.i32(dst + lat::NY) != m.i32(src + lat::NY) {
        return;
    }
    let (mut p, mut q) = (m.ptr(dst + lat::POINTS), m.ptr(src + lat::POINTS));
    let (mut f, mut g) = (m.ptr(dst + lat::FLAGS), m.ptr(src + lat::FLAGS));
    let n = (m.i16(dst + lat::NX) as i32).wrapping_mul(m.i16(dst + lat::NY) as i32).wrapping_mul(m.i16(dst + lat::NZ) as i32);
    let mut i = 0;
    while i < n {
        for k in 0..3 {
            let w = m.u32(q + 4 * k);
            m.set_u32(p + 4 * k, w);
        }
        let w = m.u16(g);
        m.set_u16(f, w);
        p = p.wrapping_add(12);
        q = q.wrapping_add(12);
        f = f.wrapping_add(2);
        g = g.wrapping_add(2);
        i += 1;
    }
}
