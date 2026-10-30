// SPDX-License-Identifier: MIT
//! `symmetry.c`: the model a model is symmetric to (`SI_symmetryGetElem`), the one routine of the file the
//! ported code calls (the keyframe editing of `si-motion` and the character keys of `si-kine` reach it by
//! address). The dictionary's look-ups are recorded callees here, as they are in every routine of this
//! package.
//!
//! **Compared with the emulated original**: run against the original stepped through in the emulation
//! harness on made-up relation answers (`re/tools/subreg1/emu_gen_symmetry.py`, `rensetup-emucheck`).

use super::current_dict;
use super::globals::*;
use super::outside::{Outside, SI_ED_GET_ELEM_FROM_MODEL, SI_ELEMDICT_FIND_RELATED_ELEM};
use si_mem::{FnPtr, Mem, Ptr32};
use si_types::dict::Elem;
use si_types::model::Model;

/// SI_elemdictGetCurRelation (0x08134db0), `elemdict.c`: (dictionary, element, side, list); answers the
/// entry of the list the cursor is after.
pub const SI_ELEMDICT_GET_CUR_RELATION: FnPtr = FnPtr::new(0x08134db0);
/// The relation id of "symmetric to" and the list it is kept in.
const SYMMETRY_RELATION: u32 = 0x96;
const SYMMETRY_LIST: u32 = 1;
/// The entry's 16-bit "has data" word and the pointer to the data (`SI_RelSymmData`), whose 16-bit word at +2
/// is positive when the symmetry is a user's own.
const ENTRY_HAS_DATA: u32 = 0x14;
const ENTRY_DATA: u32 = 0x18;
const DATA_USER: u32 = 2;

/// The element a model is symmetric to [soft 0x082434a0 SI_symmetryGetElem]: `index` receives the element's
/// index in the models' chapter, `data` the symmetry relation's data (null first). The model's element is
/// looked up and then its symmetry relation, forward first (the model is the one others are symmetric to):
/// when there is one, the answer is 0 if the entry holds no data, if the data exists, `keep_user` is not 0
/// and its user word is not positive; otherwise the model's own element when `own` is not 0, else the related
/// one. When there is none forward, the look-up goes backward; an element found there is looked up forward
/// itself (the answer thrown away, which moves the cursor and fills `index`), and its entry is judged
/// the same way, with that element as the answer.
pub fn symmetry_get_elem<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, index: Ptr32<i32>, data: u32, keep_user: i16, own: i16) -> Ptr32<Elem> {
    m.set_u32(data, 0);
    let scratch = m.local(4);
    let chapter_out = m.local(4);
    let e = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[model.0, scratch]);
    let chapter = m[SI_CID].model as u32;
    let dict = |m: &Mem| current_dict(m).0;
    let find = |o: &mut O, m: &mut Mem, elem: u32, side: u32| {
        let d = dict(m);
        o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, side, SYMMETRY_LIST, SYMMETRY_RELATION, chapter, chapter_out, index.0])
    };
    let r = find(o, m, e, 1);
    let answer = if r != 0 {
        let d = dict(m);
        let entry = o.call(m, SI_ELEMDICT_GET_CUR_RELATION, &[d, e, 1, SYMMETRY_LIST]);
        if m.i16(entry + ENTRY_HAS_DATA) == 0 {
            0
        } else {
            let p = m.u32(entry + ENTRY_DATA);
            m.set_u32(data, p);
            if p != 0 && keep_user != 0 && m.i16(p + DATA_USER) <= 0 {
                0
            } else if own == 0 {
                r
            } else {
                e
            }
        }
    } else {
        let n = find(o, m, e, 0);
        if n == 0 {
            0
        } else {
            find(o, m, n, 1);
            let d = dict(m);
            let entry = o.call(m, SI_ELEMDICT_GET_CUR_RELATION, &[d, n, 1, SYMMETRY_LIST]);
            if m.i16(entry + ENTRY_HAS_DATA) == 0 {
                0
            } else {
                let p = m.u32(entry + ENTRY_DATA);
                m.set_u32(data, p);
                if p != 0 && keep_user != 0 && m.i16(p + DATA_USER) <= 0 {
                    0
                } else {
                    n
                }
            }
        }
    };
    m.release(8);
    Ptr32::new(answer)
}
