// SPDX-License-Identifier: MIT
//! `dyn_constr.c`: the list of dynamics constraints of a model (`+0x2fc`), the constraint records, the model standing for a
//! constraint kind and the position / orientation a constraint gives a force. The drawing routines of the file
//! (`SI_dynaCdmDraw`, `drawCharacter`, `drawFleche`, `drawQSControl`, `drawFan`, `drawNail`, `SI_dynaConstraintDraw`) are not ported.
//!
//! A constraint record is 0x40 bytes: priority (short, `+0`), kind (int, `+4`), `+8` set to 1, the object model (`+0x10`),
//! the extra record (`+0x38`) and the next record (`+0x3c`); the list is sorted by priority, highest first.
//! Sources are functions of the Linux `soft`, cited as `[soft 0xADDRESS name]`.

use crate::outside::DynaCalls;
use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

/// [soft 0x084957cc SI_conslistFindPriority]: the list priority of a constraint kind (0 to 6 are themselves, 7 to 9 go to 8,
/// 9, 10, 10 to 12 to -3, -2, -1, 13 to 7; anything else -9).
pub fn si_conslist_find_priority(kind: i16) -> i16 {
    match kind {
        0..=6 => kind,
        7 => 8,
        8 => 9,
        9 => 10,
        10 => -3,
        11 => -2,
        12 => -1,
        13 => 7,
        _ => -9,
    }
}

/// [soft 0x0849585c SI_conslistFindPrevConstraint]: the last record of the model's list whose priority is not under `prio`
/// and that is before the place for a record of that priority; null when the place is the head. With `flag` set a record of
/// the same priority, kind and object is looked for and `*found` is set to 1 when there is one (the returned record is then
/// the one before it).
pub fn si_conslist_find_prev_constraint(m: &mut Mem, model: u32, prio: i16, kind: i16, obj: u32, flag: i16, found: u32) -> u32 {
    m.set_u16(found, 0);
    let mut prev = 0u32;
    let mut cur = m.u32(model + 0x2fc);
    if cur == 0 || (m.u16(cur) as i16) < prio {
        return prev;
    }
    loop {
        if flag != 0 && m.u16(cur) as i16 == prio && kind as i32 as u32 == m.u32(cur + 4) && obj == m.u32(cur + 0x10) {
            m.set_u16(found, 1);
            return prev;
        }
        prev = cur;
        cur = m.u32(prev + 0x3c);
        if cur == 0 || (m.u16(cur) as i16) < prio {
            return prev;
        }
    }
}

/// [soft 0x084958d0 SI_conslistConstrAllocate]: a constraint record of the kind for the object model `obj`; a negative `prio`
/// is replaced by the kind's priority. A gravity-like kind 9 (fan) gets a 0x98-byte extra record with its fields cleared and
/// the identity matrix at `+0x34`. The object model of kind 9 or 7 gets a kinematic state if it has none. When the allocation
/// fails the original goes on to read the null record (a fault), as here.
pub fn si_conslist_constr_allocate(m: &mut Mem, kind: i16, prio: i16, obj: u32) -> u32 {
    let prio = if prio < 0 { si_conslist_find_priority(kind) } else { prio };
    let c = m.si_malloc(0x40);
    if c != 0 {
        m.set_u16(c, prio as u16);
        m.set_u32(c + 4, kind as i32 as u32);
        m.set_u32(c + 0x3c, 0);
        m.set_u32(c + 0x10, obj);
        m.set_u16(c + 0xa, 0);
        m.set_u16(c + 0xc, 0);
        m.set_u16(c + 8, 1);
        m.set_u32(c + 0x38, 0);
        if kind == 9 {
            let e = m.si_malloc(0x98);
            m.set_u32(c + 0x38, e);
            if e != 0 {
                m.set_u32(e, 0);
                m.set_u32(e + 4, 0);
                m.set_u16(e + 0x78, 0xffff);
                for off in [0x8u32, 0xc, 0x80, 0x74, 0x84, 0x94, 0x18, 0x14, 0x10, 0x24, 0x20, 0x1c, 0x30, 0x2c, 0x28] {
                    m.set_u32(e + off, 0);
                }
                m.copy(e + 0x34, 0x086d_c160, 64);
            }
        }
        for off in [0x1cu32, 0x18, 0x14, 0x28, 0x24, 0x20, 0x34, 0x30, 0x2c] {
            m.set_u32(c + off, 0);
        }
    }
    let o = m.u32(c.wrapping_add(0x10));
    if o != 0 && (kind == 9 || kind == 7) && m.u32(o + 0x2f0) == 0 {
        let k = si_kine::image::qstretch::kine_state_allocate(m);
        let o = m.u32(c + 0x10);
        m.set_u32(o + 0x2f0, k);
    }
    c
}

/// [soft 0x08495a98 SI_conslistConstrCompleteAllocation]: gives the object model of a kind 9 or 7 record a kinematic state if
/// it has none.
pub fn si_conslist_constr_complete_allocation(m: &mut Mem, c: u32) {
    if c != 0 {
        let o = m.u32(c + 0x10);
        if o != 0 {
            let k = m.u32(c + 4);
            if (k == 9 || k == 7) && m.u32(o + 0x2f0) == 0 {
                let s = si_kine::image::qstretch::kine_state_allocate(m);
                let o = m.u32(c + 0x10);
                m.set_u32(o + 0x2f0, s);
            }
        }
    }
}

/// [soft 0x08495ad8 SI_conslistAddConstraint]: puts the record into the model's list at its priority place. A record with the
/// same priority, kind and object (looked for when `flag` is set) is refused with the status message
/// "Constraint already exits !!" (0); otherwise 1.
pub fn si_conslist_add_constraint(m: &mut Mem, o: &mut dyn Outside, model: u32, c: u32, flag: i16) -> i32 {
    if m.u32(model + 0x2fc) == 0 {
        m.set_u32(model + 0x2fc, c);
        return 1;
    }
    let found = m.local(2);
    let prio = m.u16(c) as i16;
    let kind = m.u16(c + 4) as i16;
    let obj = m.u32(c + 0x10);
    let prev = si_conslist_find_prev_constraint(m, model, prio, kind, obj, flag, found);
    let hit = m.u16(found) != 0;
    m.release(2);
    if hit {
        ParticleCalls::si_status_display_message(&mut *o, m, 0x086d_6c88, 1);
        return 0;
    }
    let next;
    if prev == 0 {
        next = m.u32(model + 0x2fc);
        m.set_u32(model + 0x2fc, c);
    } else {
        next = m.u32(prev + 0x3c);
        m.set_u32(prev + 0x3c, c);
    }
    m.set_u32(c + 0x3c, next);
    1
}

/// [soft 0x08495b74 conslistDisposeOneConstraint]: frees a record with its extra record's three blocks (`+8`, `+0xc`, `+0x80`).
pub fn conslist_dispose_one_constraint(m: &mut Mem, c: u32) {
    if c != 0 {
        let e = m.u32(c + 0x38);
        if e != 0 {
            for off in [0x8u32, 0xc, 0x80] {
                let p = m.u32(m.u32(c + 0x38) + off);
                if p != 0 {
                    m.free(p);
                    let e = m.u32(c + 0x38);
                    m.set_u32(e + off, 0);
                }
            }
            let e = m.u32(c + 0x38);
            if e != 0 {
                m.free(e);
                m.set_u32(c + 0x38, 0);
            }
        }
        m.free(c);
    }
}

/// [soft 0x08495c20 conslistDisposeConstraint]: removes from the model's list the record of the kind and the object `obj`
/// (or, with a null object, every record of the kind), telling the editor (`SI_edRelaxDynaConstraint`) and freeing each.
/// Returns whether a record with the object was found (always 0 with a null object).
pub fn conslist_dispose_constraint(m: &mut Mem, o: &mut dyn Outside, model: u32, kind: i16, obj: u32) -> i16 {
    let prio = si_conslist_find_priority(kind);
    let found = m.local(2);
    m.set_u16(found, 0);
    let relax = |m: &mut Mem, o: &mut dyn Outside, c: u32| {
        let (ob, k) = (m.u32(c + 0x10), m.u32(c + 4));
        o.si_ed_relax_dyna_constraint(m, model, ob, k as i32);
        conslist_dispose_one_constraint(m, c);
    };
    if obj != 0 {
        let prev = si_conslist_find_prev_constraint(m, model, prio, kind, obj, 1, found);
        if m.u16(found) != 0 {
            let c;
            if prev != 0 {
                c = m.u32(prev + 0x3c);
                let n = m.u32(c + 0x3c);
                m.set_u32(prev + 0x3c, n);
            } else {
                c = m.u32(model + 0x2fc);
                let n = m.u32(c + 0x3c);
                m.set_u32(model + 0x2fc, n);
            }
            if c != 0 {
                relax(m, o, c);
            }
        }
    } else {
        let prev = si_conslist_find_prev_constraint(m, model, prio.wrapping_add(1), -1, 0, 0, found);
        let kind32 = kind as i32 as u32;
        if prev != 0 {
            let mut c = m.u32(prev + 0x3c);
            if c != 0 && m.u32(c + 4) == kind32 {
                loop {
                    let n = m.u32(c + 0x3c);
                    m.set_u32(prev + 0x3c, n);
                    relax(m, o, c);
                    c = m.u32(prev + 0x3c);
                    if c == 0 || m.u32(c + 4) != kind32 {
                        break;
                    }
                }
            }
        } else {
            let mut c = m.u32(model + 0x2fc);
            if c != 0 && m.u32(c + 4) == kind32 {
                loop {
                    let n = m.u32(c + 0x3c);
                    m.set_u32(model + 0x2fc, n);
                    relax(m, o, c);
                    c = m.u32(model + 0x2fc);
                    if c == 0 || m.u32(c + 4) != kind32 {
                        break;
                    }
                }
            }
        }
    }
    let r = m.u16(found) as i16;
    m.release(2);
    r
}

/// [soft 0x08495d90 SI_conslistDispose]: with `all` every record of the model's list goes (each told to the editor, the
/// head cleared); otherwise the records of the kind.
pub fn si_conslist_dispose(m: &mut Mem, o: &mut dyn Outside, model: u32, all: i16, kind: i16) {
    let mut c = m.u32(model + 0x2fc);
    if all != 0 {
        while c != 0 {
            let n = m.u32(c + 0x3c);
            let (ob, k) = (m.u32(c + 0x10), m.u32(c + 4));
            o.si_ed_relax_dyna_constraint(m, model, ob, k as i32);
            conslist_dispose_one_constraint(m, c);
            c = n;
        }
        m.set_u32(model + 0x2fc, 0);
    } else {
        conslist_dispose_constraint(m, o, model, kind, 0);
    }
}

/// [soft 0x08495dfc dynaDefConstraintCheckExistence]: for the kind 10 (default gravity) the first model of the current
/// graphic list (kind 2 or 3) that is a null (`+0x30` = 0) of algorithm 8 (`+0x2ec`); 0 for any other kind or when there is none.
pub fn dyna_def_constraint_check_existence(m: &mut Mem, kind: i16) -> u32 {
    if kind != 10 {
        return 0;
    }
    let proc_data = m.u32(0x0867_9314);
    let lists = m.u32(0x0867_9498);
    let i = m.u32(proc_data + 4);
    let mut node = m.u32(lists.wrapping_add(i.wrapping_mul(36)).wrapping_add(4));
    while node != 0 {
        if m.u32(node).wrapping_sub(2) <= 1 {
            let md = m.u32(node + 4);
            if m.u32(md + 0x30) == 0 && m.u32(md + 0x2ec) == 8 {
                return md;
            }
        }
        node = m.u32(node + 0x1c);
    }
    0
}

/// [soft 0x08495e5c SI_dynaConstraintGet]: the model standing for a constraint kind. Kinds 10 and 11 reuse the existing one
/// ([`dyna_def_constraint_check_existence`]); otherwise a new model is made, its fields (`+0x100..+0x108`, the algorithm
/// `+0x2ec` and the name) set by kind, added to the current graphic list and the screen refreshed. For a kind outside
/// the table the base name is whatever the register held in the original (here 0).
pub fn si_dyna_constraint_get(m: &mut Mem, o: &mut dyn Outside, kind: i16, flag: i16) -> u32 {
    if (kind as u16).wrapping_sub(10) <= 1 {
        let e = dyna_def_constraint_check_existence(m, kind);
        if e != 0 {
            return e;
        }
    }
    let md = o.si_model_allocate(m, 0);
    let g = |m: &Mem, a: u32| m.u32(a);
    let mut name = 0u32;
    match kind {
        10 => {
            m.set_u32(md + 0x100, 0x3f80_0000);
            let v = g(m, 0x0866_08d4);
            m.set_u32(md + 0x104, v);
            m.set_u32(md + 0x108, 0x3f80_0000);
            m.set_u32(md + 0x2ec, 8);
            name = 0x086d_6ca4;
        }
        0 => {
            m.set_u32(md + 0x100, 0x3f80_0000);
            let v = g(m, 0x0866_08d4);
            m.set_u32(md + 0x104, v);
            m.set_u32(md + 0x108, 0x3f80_0000);
            m.set_u32(md + 0x2ec, 5);
            name = 0x086d_6cac;
        }
        4 => {
            m.set_u32(md + 0x100, 0x3f80_0000);
            let v = g(m, 0x0866_08d8);
            m.set_u32(md + 0x104, v);
            m.set_u32(md + 0x108, 0x3f80_0000);
            m.set_u32(md + 0x2ec, 6);
            name = 0x086d_6cb1;
        }
        6 | 11 => {
            m.set_u32(0x0882_2008, 0);
            for (off, a) in [(0x100u32, 0x0866_08dc_u32), (0x104, 0x0866_08e0), (0x108, 0x0866_08e4)] {
                let v = g(m, a);
                m.set_u32(md + off, v);
            }
            m.set_u32(md + 0x2ec, 7);
            name = 0x086d_6cb7;
        }
        13 => {
            m.set_u32(0x0882_2008, 0);
            for (off, a) in [(0x100u32, 0x0866_08dc_u32), (0x104, 0x0866_08e0)] {
                let v = g(m, a);
                m.set_u32(md + off, v);
            }
            m.set_u32(md + 0x108, 0x3f80_0000);
            m.set_u32(md + 0x2ec, 9);
            name = 0x086d_6cbc;
        }
        7 => {
            m.set_u32(md + 0x100, 0);
            m.set_u32(md + 0x104, 0x3f80_0000);
            m.set_u32(md + 0x108, 0x3f80_0000);
            m.set_u32(md + 0x2ec, 10);
            name = 0x086d_6cc0;
        }
        _ => {}
    }
    let nm = o.si_ed_model_get_new_name(m, name);
    o.si_model_change_name(m, md, nm);
    let pd = m.u32(0x0867_9314);
    let lists = m.u32(0x0867_9498);
    let at = lists.wrapping_add(m.u32(pd + 4).wrapping_mul(36));
    ParticleCalls::si_graflist_add_data(&mut *o, m, at, md, 2, flag as i32);
    o.si_model_update_son_state(m, 0, md, 0);
    ParticleCalls::si_refresh_all(&mut *o, m, 1);
    ParticleCalls::si_refresh_counter(&mut *o, m);
    md
}

/// [soft 0x08496060 SI_dynaConstraintRelaxFromData]: removes the constraints that the object model `p2` (by its algorithm
/// `+0x2ec`: 5 to 0, 6 to 4, 7 to 6, 8 to 10, 9 to 13, 10 to 7, any other 9) stands for from the list of `p1`.
pub fn si_dyna_constraint_relax_from_data(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32) -> i16 {
    let kind: i16 = match m.u32(p2 + 0x2ec) {
        5 => 0,
        6 => 4,
        7 => 6,
        8 => 10,
        9 => 13,
        10 => 7,
        _ => 9,
    };
    conslist_dispose_constraint(m, o, p1, kind, p2)
}

/// [soft 0x084960cc SI_modeldynaConstrListClean]: drops (and frees) the records of the model's list that have no object model.
pub fn si_modeldyna_constr_list_clean(m: &mut Mem, model: u32) {
    if model == 0 {
        return;
    }
    let mut c = m.u32(model + 0x2fc);
    let mut prev = 0u32;
    while c != 0 {
        if m.u32(c + 0x10) == 0 {
            let d = c;
            c = m.u32(c + 0x3c);
            if d == m.u32(model + 0x2fc) {
                m.set_u32(model + 0x2fc, c);
            } else {
                m.set_u32(prev + 0x3c, c);
            }
            conslist_dispose_one_constraint(m, d);
        } else {
            prev = c;
            c = m.u32(c + 0x3c);
        }
    }
}

/// [soft 0x08495544 si_constraintPositionGet]: the position of the constraint model `p1` (`+0x1b8..`), taken into the frame
/// of `p2` (its position subtracted, its 3 x 3 part `+0x178..` applied) when `flag` is set and `p2` is not null. Written as
/// three floats to `out`.
pub fn si_constraint_position_get(m: &mut Mem, p1: u32, p2: u32, flag: i16, out: u32) {
    let (mut a, mut b, mut c) = (m.f32(p1 + 0x1b8), m.f32(p1 + 0x1bc), m.f32(p1 + 0x1c0));
    if flag != 0 && p2 != 0 {
        let f = |m: &Mem, off: u32| x(m.f32(p2 + off));
        let dx = x(a) - f(m, 0x1b8);
        let dy = x(b) - f(m, 0x1bc);
        let dz = x(c) - f(m, 0x1c0);
        a = (((f(m, 0x178) * dx) + (f(m, 0x17c) * dy)) + (f(m, 0x180) * dz)).to_f32();
        b = (((f(m, 0x188) * dx) + (f(m, 0x18c) * dy)) + (f(m, 0x190) * dz)).to_f32();
        c = (((dx * f(m, 0x198)) + (dy * f(m, 0x19c))) + (dz * f(m, 0x1a0))).to_f32();
    }
    m.set_f32(out, a);
    m.set_f32(out + 4, b);
    m.set_f32(out + 8, c);
}

/// [soft 0x08495624 si_constraintOrientationGet]: the direction of the constraint model `p1` along its x axis (`axis` is
/// `'x'`) or y axis, `flag2` choosing the unit axis (from the program's tables at `0x086dc1ec` / `0x086dc1f8`) over the
/// model's own scale (`+0x16c` or `+0x170`) on that axis; carried through the matrix and position of `p1` (`+0x178..`,
/// `+0x1a8..`) and, when `flag1` is set and `p2` is not null, through the 3 x 3 part of `p2`.
pub fn si_constraint_orientation_get(m: &mut Mem, p1: u32, p2: u32, flag1: i16, flag2: i16, axis: u8, out: u32) {
    let (v0, v1, v2): (f32, f32, f32);
    if axis == b'x' {
        if flag2 != 0 {
            (v0, v1, v2) = (m.f32(0x086d_c1ec), m.f32(0x086d_c1f0), m.f32(0x086d_c1f4));
        } else {
            (v0, v1, v2) = (m.f32(p1 + 0x16c), 0.0, 0.0);
        }
    } else if flag2 != 0 {
        (v0, v1, v2) = (m.f32(0x086d_c1f8), m.f32(0x086d_c1fc), m.f32(0x086d_c200));
    } else {
        (v0, v1, v2) = (0.0, m.f32(p1 + 0x170), 0.0);
    }
    let f = |m: &Mem, off: u32| x(m.f32(p1 + off));
    let (a, b, c) = (x(v0), x(v1), x(v2));
    let t1 = (((b * f(m, 0x188)) + (a * f(m, 0x178))) + (c * f(m, 0x198))) + f(m, 0x1a8);
    let t2 = (((a * f(m, 0x17c)) + (b * f(m, 0x18c))) + (c * f(m, 0x19c))) + f(m, 0x1ac);
    let t3 = ((c * f(m, 0x1a0)) + ((b * f(m, 0x190)) + (a * f(m, 0x180)))) + f(m, 0x1b0);
    let (r0, r1, r2);
    if flag1 != 0 && p2 != 0 {
        let g = |m: &Mem, off: u32| x(m.f32(p2 + off));
        r0 = (((g(m, 0x178) * t1) + (g(m, 0x17c) * t2)) + (g(m, 0x180) * t3)).to_f32();
        r1 = (((g(m, 0x188) * t1) + (g(m, 0x18c) * t2)) + (g(m, 0x190) * t3)).to_f32();
        r2 = (((t2 * g(m, 0x19c)) + (t1 * g(m, 0x198))) + (t3 * g(m, 0x1a0))).to_f32();
    } else {
        (r0, r1, r2) = (t1.to_f32(), t2.to_f32(), t3.to_f32());
    }
    m.set_f32(out, r0);
    m.set_f32(out + 4, r1);
    m.set_f32(out + 8, r2);
}

/// [soft 0x08495384 SI_dynaConstraintBounding]: the two corners (`lo`, `hi`) of the picking box of a constraint model, by its
/// algorithm (`+0x2ec`): 5, 6, 8 give (-1, 0, -1) and (1, 0.01, 1); 16 and 17 (-1, 0, -1) and (1, 1, 1); 9, 7 and 10 scale
/// the box with the model's three limits (`+0x100`, `+0x104`, `+0x108`); 11 to 15 and any other value write nothing.
/// The larger of two lengths is rounded to a float before it is divided (a stack temporary in the original).
pub fn si_dyna_constraint_bounding(m: &mut Mem, model: u32, lo: u32, hi: u32) {
    let (l0, l1, l2) = (m.f32(model + 0x100), m.f32(model + 0x104), m.f32(model + 0x108));
    let set = |m: &mut Mem, a: u32, v: [f32; 3]| {
        for (i, f) in v.iter().enumerate() {
            m.set_f32(a + 4 * i as u32, *f);
        }
    };
    let alg = m.u32(model + 0x2ec).wrapping_sub(5);
    if alg > 12 {
        return;
    }
    match alg + 5 {
        5 | 6 | 8 => {
            set(m, lo, [-1.0, 0.0, -1.0]);
            set(m, hi, [1.0, 0.01, 1.0]);
        }
        16 | 17 => {
            set(m, lo, [-1.0, 0.0, -1.0]);
            set(m, hi, [1.0, 1.0, 1.0]);
        }
        9 => {
            let c = if x(l0).gt(F80::from_i32(0)) { 1.0 } else { -1.0 };
            let a2 = x(l1).abs() * F80::from_f64(0.25);
            let b2 = x(l2).abs();
            let big = if b2.lt(a2) { a2 } else { b2 };
            let big = x(big.to_f32());
            let (a, b) = (big / x(l1), big / x(l2));
            set(m, lo, [0.0, (-a).to_f32(), (-b).to_f32()]);
            set(m, hi, [c, a.to_f32(), b.to_f32()]);
        }
        7 => {
            let p = F80::from_f64(10.0) / x(l0);
            let q = (x(l1) * F80::from_f64(0.25) / x(l2)).to_f32();
            set(m, lo, [0.0, -0.25, (-x(q)).to_f32()]);
            set(m, hi, [p.to_f32(), 0.25, q]);
        }
        10 => {
            let (a2, b2) = (x(l0).abs(), x(l1).abs());
            let big = if b2.lt(a2) { a2 } else { b2 };
            let big = x(big.to_f32());
            let (a, b) = (big / x(l1), big / x(l2));
            set(m, lo, [-1.0, (-a).to_f32(), (-b).to_f32()]);
            set(m, hi, [1.0, a.to_f32(), b.to_f32()]);
        }
        _ => {}
    }
}
