// SPDX-License-Identifier: MIT
//! `motionUtils.c`: what the animation commands share (finding 0017, "Utilities").
//!
//! Ported so far: the attribute table's "selected" marks, the copy of one parameter's
//! curves from one element to another, the look-up of a three-component parameter's
//! curves, and the key marks of the scene ("this object has a key at the current frame").

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::{arel, elem, fcurve as fo, grafnode, key as ko, keyflag, rellist};
use crate::reset::ResetHost;
use si_x87::F80;

/// Fields of the attribute dictionary `SI_ATD` (0x08781438) as these files use them: the
/// table of curve types (12-byte entries; the 16-bit word at +8 of an entry is the type's
/// "selected" mark, which the reset, copy and key commands filter by) and its length.
pub mod atd {
    pub const ENTRIES: u32 = 0x10;
    pub const NB_ENTRIES: u32 = 0x14;
    pub const ENTRY_SIZE: u32 = 0x0c;
    pub const ENTRY_SELECTED: u32 = 0x08;
}

/// `SI_dopeFcvTable`: one 16-bit "selected" mark per curve type, the dope sheet's own.
pub const SI_DOPE_FCV_TABLE: Addr = 0x0878_95c0;
/// `SI_cusValNumCallback`, `SI_fcurveDirty`, `SI_cusFcurveDirty` (finding 0009).
const SI_CUS_VAL_NUM_CALLBACK: Addr = 0x086e_49e8;
const SI_FCURVE_DIRTY: Addr = 0x086e_49dc;
const SI_CUS_FCURVE_DIRTY: Addr = 0x086e_49e0;

/// Texts of [`model_active`].
pub mod text {
    use si_mem::Addr;
    /// "No model currently selected."
    pub const NO_MODEL: Addr = 0x0869_7919;
    /// "This button does not work" / "when many models are selected." / "Select one model
    /// to use this button." / "Ok".
    pub const MANY: [Addr; 4] = [0x0869_78ff, 0x0869_78e0, 0x0869_78a0, 0x0869_7880];
}

/// Fields of a graf list (`SI_GrafList`, 0x24 bytes) that [`model_active`] reads (**new**).
mod graflist {
    /// 16 bits: non-zero when several nodes may be selected at once.
    pub const MULTI: u32 = 0x00;
    pub const FIRST: u32 = 0x04;
    /// The one selected node, when [`MULTI`] is 0.
    pub const SELECTED: u32 = 0x08;
}

crate::host_trait! {
    /// What `motionUtils.c` calls outside this package.
    pub trait UtilsHost: ResetHost {
        /// `SI_dialogRequest(line 1, line 2, line 3, button)`.
        fn dialog_request(a: Addr, b: Addr, c: Addr, button: Addr) -> i16;
        /// `SI_setDopFcvTableFromDopeMode()`: fills `SI_dopeFcvTable` from the dope sheet's mode.
        fn set_dop_fcv_table_from_dope_mode();
        /// `SI_elemdictFindElemFromGrafNode(graf node, &chapter, &index)`.
        fn elemdict_find_elem_from_graf_node(gn: Addr, chap: Addr, index: Addr) -> Addr;
        /// `SI_getWaveFcurvePointer(model, curve type)`.
        fn get_wave_fcurve_pointer(mdl: Addr, kind: i32) -> Addr;
    }
}

/// The model the model commands act on [soft 0x0822b300 si_modelActive]: the object of the
/// selected graf node.
///
/// In single-selection mode (the current graf list's word +0 is 0) that is the list's
/// selected node (+8); it must be a model (kinds 2, 3), else "No model currently selected."
/// Otherwise the list is searched for nodes of kind 2 or 3 that are selected as a node (1)
/// or as a branch (3): exactly one must be (with two or more, a dialogue: "This button does
/// not work when many models are selected. Select one model to use this button."). Null
/// when there is none.
pub fn model_active<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H) -> Addr {
    let gl = glob::cur_graf_list(m);
    let mut p = m.ptr(gl + graflist::FIRST);
    let mut found = 0;
    if m.i16(gl + graflist::MULTI) == 0 {
        found = m.ptr(gl + graflist::SELECTED);
        if found == 0 {
            return 0;
        }
        if (m.i32(found + grafnode::KIND).wrapping_sub(2) as u32) > 1 {
            KeyHost::status_display_message(h, m, text::NO_MODEL, 3);
            return 0;
        }
    } else {
        let mut n = 0;
        while p != 0 {
            let sel = m.i32(p + grafnode::SELECTION);
            if (m.i32(p + grafnode::KIND).wrapping_sub(2) as u32) < 2 && (sel == 1 || sel == 3) {
                n += 1;
                found = p;
            }
            p = m.ptr(p + grafnode::NEXT);
            if n >= 2 {
                break;
            }
        }
        if n > 1 {
            h.dialog_request(m, text::MANY[0], text::MANY[1], text::MANY[2], text::MANY[3]);
            return 0;
        }
    }
    if found != 0 {
        m.ptr(found + grafnode::DATA)
    } else {
        0
    }
}

/// Sets the "selected" mark of **every** curve type to `v` [soft 0x0822b3c4 SI_fcvTableSet].
pub fn fcv_table_set(m: &mut Mem, v: i16) {
    let mut i = 0;
    while i < m.i32(glob::SI_ATD + atd::NB_ENTRIES) {
        let e: Addr = m.ptr(glob::SI_ATD + atd::ENTRIES).wrapping_add((i as u32).wrapping_mul(atd::ENTRY_SIZE));
        m.set_i16(e + atd::ENTRY_SELECTED, v);
        i += 1;
    }
}

/// `SI_fcurveRemoveAllKeys(curve)` (0x081ff8bc, ported in `si-fcurve`) on a curve in the
/// image: the key array is freed, pointer and count cleared, fast playback told.
pub fn fcurve_remove_all_keys<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, c: Addr) {
    let keys = m.ptr(c + fo::KEYS);
    if keys != 0 {
        m.free(keys);
    }
    m.set_ptr(c + fo::KEYS, 0);
    m.set_i32(c + fo::NB_KEYS, 0);
    h.fast_set_dirty(m, 0, -1, 1);
}

/// `SI_fcurveTransfer(source, destination)` (0x081ff0e4, ported in `si-fcurve`) on curves
/// in the image: the destination becomes a copy of the source's whole record (type and
/// owner included) with a key array of its own (room for the count rounded up to a multiple
/// of eight; null for no key) whose keys point back at it, and no snapshot. Its old array is
/// not freed. It keeps its own "dirty" word, and gets one (and enters the counter) if it
/// had none while custom-value callbacks are installed.
pub fn fcurve_transfer<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, src: Addr, dst: Addr) {
    if src != 0 {
        let dirty = m.i16(dst + fo::DIRTY);
        let n = m.i32(src + fo::NB_KEYS);
        let rec = m.bytes(src, fo::SIZE).to_vec();
        m.set_bytes(dst, &rec);
        m.set_ptr(dst + fo::AUX, 0);
        let room = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 7).wrapping_sub(7));
        let keys = m.si_malloc(room.wrapping_mul(ko::SIZE));
        m.set_ptr(dst + fo::KEYS, keys);
        let mut i = 0;
        while i < n {
            let from = m.ptr(src + fo::KEYS).wrapping_add((i as u32).wrapping_mul(ko::SIZE));
            let to = m.ptr(dst + fo::KEYS).wrapping_add((i as u32).wrapping_mul(ko::SIZE));
            let k = m.bytes(from, ko::SIZE).to_vec();
            m.set_bytes(to, &k);
            m.set_ptr(to + ko::CURVE, dst);
            i += 1;
        }
        m.set_i16(dst + fo::DIRTY, dirty);
        if m.i32(SI_CUS_VAL_NUM_CALLBACK) > 0 && dst != 0 && dirty == 0 {
            m.set_i16(dst + fo::DIRTY, 1);
            let counter = if fcv::kind(m, dst) == 0x236 { SI_CUS_FCURVE_DIRTY } else { SI_FCURVE_DIRTY };
            let v = m.i32(counter).wrapping_add(1);
            m.set_i32(counter, v);
        }
    }
    h.fast_set_dirty(m, 0, -1, 1);
}

/// The destination's curve for one component: the existing one emptied, or a new one of
/// type `kind` owned by the destination's object and filed under it.
fn copy_target<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, existing: Addr, kind: i32, to: Addr, chap: i32, index: i32) -> Addr {
    if existing == 0 {
        let c = key::fcurve_allocate(m, kind, m.ptr(to + elem::DATA));
        h.ed_fcv_add(m, c, to, chap, index);
        c
    } else {
        fcurve_remove_all_keys(m, h, existing);
        existing
    }
}

/// Copies the curve of a one-curve parameter from one element to another
/// [soft 0x0822b3f4 si_singleFcurveCopy]: nothing when the relation is unknown or the source
/// has no curve; the destination's curve is emptied, or created (type
/// `SI_relGetFcvType(relid)`) and filed, and then made a copy of the source's
/// (`SI_fcurveTransfer`: the whole record, so the copy also takes the source's owner).
pub fn single_fcurve_copy<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, from: Addr, to: Addr, to_index: i32, relid: i32) {
    let out = m.local(4);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out);
    let group = m.i32(out);
    m.release(4);
    if chap < 0 {
        return;
    }
    let kind = h.rel_get_fcv_type(m, relid);
    let a = ed::find_fcv(m, from, group, relid);
    if a != 0 {
        let b = ed::find_fcv(m, to, group, relid);
        let b = copy_target(m, h, b, kind, to, chap, to_index);
        fcurve_transfer(m, h, a, b);
    }
}

/// The same for a three-curve parameter [soft 0x0822b4a4 SI_tripleFcurveCopy]: only when
/// the source has all three curves (the first three of the relation, in list order). The
/// destination's three (in its list order; missing ones are created with the types T, T + 1,
/// T + 2) receive the source's first, second and third: by **position**, not by type.
pub fn triple_fcurve_copy<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, from: Addr, to: Addr, to_index: i32, relid: i32) {
    let out = m.local(4);
    let chap = h.reldict_get_rel_chap_grp(m, glob::SI_RD, relid, out);
    let group = m.i32(out);
    m.release(4);
    if chap < 0 {
        return;
    }
    let kind = h.rel_get_fcv_type(m, relid);
    let a = [ed::find_fcv(m, from, group, relid), ed::find_next_fcv(m, from, group, relid), ed::find_next_fcv(m, from, group, relid)];
    if a[0] == 0 || a[1] == 0 || a[2] == 0 {
        return;
    }
    let b = [ed::find_fcv(m, to, group, relid), ed::find_next_fcv(m, to, group, relid), ed::find_next_fcv(m, to, group, relid)];
    let mut t = [0; 3];
    for k in 0..3 {
        t[k] = copy_target(m, h, b[k], kind.wrapping_add(k as i32), to, chap, to_index);
    }
    for k in 0..3 {
        fcurve_transfer(m, h, a[k], t[k]);
    }
}

/// Whether an element has a key at a time [soft 0x0822c6fc elemdictCheckElemKey]: 1 when one
/// of its curves (every forward relation to the ANIMATION chapter but the animation-file
/// links 15000 … 15010) whose type is selected has a key within **half a frame** of the
/// time, |key time − time| < 0.5 / rate (both sides unrounded in the register), that is not
/// a key of type 3. The keys of a curve are walked until one lies after the time. "Selected"
/// is the attribute table's mark, or with `dope` the dope sheet's own table.
///
/// A relation to a deleted ANIMATION element is read through a null pointer by the
/// original; the image records a fault.
pub fn elemdict_check_elem_key<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, dope: i16, time: f32) -> i32 {
    if e == 0 {
        return 0;
    }
    let ani = glob::chap(m, cid::ANIMATION);
    let t = F80::from_f32(time);
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            if m.i16(r + arel::CHAPID) as i32 == glob::chap(m, cid::ANIMATION) && (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) > 10 {
                let ce = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, m.i32(r + arel::ELEMID));
                let c = m.ptr(ce + elem::DATA);
                if c != 0 {
                    let selected = if dope == 0 {
                        h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c))
                    } else {
                        m.i16(SI_DOPE_FCV_TABLE.wrapping_add((fcv::kind(m, c) as u32).wrapping_mul(2)))
                    };
                    if selected != 0 {
                        let mut k = 0;
                        while k < m.i32(c + fo::NB_KEYS) {
                            let key = m.ptr(c + fo::KEYS).wrapping_add((k as u32).wrapping_mul(ko::SIZE));
                            let tol = F80::from_f64(0.5) / glob::frame_rate(m);
                            if (F80::from_f32(m.f32(key)) - t).abs().lt(tol) && m.u16(key + ko::FLAGS) & keyflag::TYPE_MASK != 3 {
                                return 1;
                            }
                            if t.lt(F80::from_f32(m.f32(key))) {
                                break;
                            }
                            k += 1;
                        }
                    }
                }
            }
            i += 1;
        }
        g += 1;
    }
    let _ = ani;
    0
}

/// Marks the objects that have a key at a time [soft 0x0822d714 SI_sceneKeySetColor].
///
/// Without `dope` every curve type is selected ([`fcv_table_set`] with 1); with it the dope
/// sheet's table is filled from its mode. Then every graf node of the current list (only
/// the selected ones unless `all`) whose element is found gets its "has a key" word (+0x14):
/// 1 when [`elemdict_check_elem_key`] says so **and** key colouring is on (`SI_glbAnim` + 2),
/// else 0. The dope sheet's table is used for model elements only.
pub fn scene_key_set_color<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H, time: f32, all: i16, dope: i16) {
    if dope == 0 {
        fcv_table_set(m, 1);
    } else {
        h.set_dop_fcv_table_from_dope_mode(m);
    }
    let mut gn = m.ptr(glob::cur_graf_list(m) + graflist::FIRST);
    while gn != 0 {
        if all != 0 || m.i32(gn + grafnode::SELECTION) != 0 {
            let out = m.local(8);
            let e = h.elemdict_find_elem_from_graf_node(m, gn, out, out + 4);
            let chap = m.i32(out);
            m.release(8);
            if e != 0 {
                let flag = if dope != 0 { (chap == glob::chap(m, cid::MODELS)) as i16 } else { 0 };
                let d = glob::cur_dict(m);
                let has = elemdict_check_elem_key(m, h, d, e, flag, time);
                let on = has != 0 && m.i16(glob::SI_GLB_ANIM + glob::anim::KEY_COLOR) != 0;
                m.set_u16(gn + grafnode::HAS_KEY, on as u16);
            }
        }
        gn = m.ptr(gn + grafnode::NEXT);
    }
}

/// The twelve curves of a wave's icon model, by type 163 … 174 (scaling, rotation,
/// translation x y z, the path percentage, decay, spread) [soft 0x0822eaec getWaveFcvPointer]:
/// `SI_getWaveFcurvePointer(model, type)` into twelve consecutive pointers.
pub fn get_wave_fcv_pointer<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, out: Addr) {
    for k in 0..12u32 {
        let c = h.get_wave_fcurve_pointer(m, mdl, 0xa3 + k as i32);
        m.set_ptr(out + 4 * k, c);
    }
}

/// The three curves of a three-component parameter, **by type**
/// [soft 0x0822efcc SI_getFcvTriplet]: the element's first three curves of the relation are
/// fetched (the second only if there is a first, the third only if there is a second), and
/// the three outputs become the one whose type is T (`SI_relGetFcvType(relid)`), the one of
/// type T + 1 and the one of type T + 2, null where no such curve is among the three.
pub fn get_fcv_triplet<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, c1: Addr, c2: Addr, c3: Addr, group: i32, relid: i32) {
    let t = h.rel_get_fcv_type(m, relid);
    m.set_ptr(c3, 0);
    m.set_ptr(c2, 0);
    let a = ed::find_fcv(m, e, group, relid);
    m.set_ptr(c1, a);
    if a != 0 {
        let b = ed::find_next_fcv(m, e, group, relid);
        m.set_ptr(c2, b);
    }
    if m.ptr(c2) != 0 {
        let c = ed::find_next_fcv(m, e, group, relid);
        m.set_ptr(c3, c);
    }
    let got = [m.ptr(c1), m.ptr(c2), m.ptr(c3)];
    let pick = |m: &Mem, kind: i32| got.iter().copied().find(|c| *c != 0 && fcv::kind(m, *c) == kind).unwrap_or(0);
    let (x, y, z) = (pick(m, t), pick(m, t.wrapping_add(1)), pick(m, t.wrapping_add(2)));
    m.set_ptr(c1, x);
    m.set_ptr(c2, y);
    m.set_ptr(c3, z);
}
