// SPDX-License-Identifier: MIT
//! `db2scn.c` on the memory image: the scene's own structures tied from the dictionary's
//! relations (`SI_scnRelationsFromElemDict`), the routine "Get Scene" calls 33 times near
//! its end (finding 0018, sections 5.7 and 17).
//!
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_scnrel.py`,
//! `scene-emucheck re/out/si-scene/vec_scnrel`): on made-up dictionaries whose elements
//! point at made-up records, every callee outside the file a recorded stand-in.
//!
//! The store form in [`crate::scnrel`] still keeps the handles the saver reads.

use crate::edtools::ByAddr;
use si_mem::Mem;

/// `SI_nselectionAllocate (0x083460a4)`, `nselections.c`: (name, 0).
pub const SI_NSELECTION_ALLOCATE: u32 = 0x0834_60a4;
/// `SI_elemdictGetElem (0x08132b50)`, `elemdict.c`: (dictionary, chapter, index).
pub const SI_ELEMDICT_GET_ELEM: u32 = 0x0813_2b50;
/// `SI_materialDispose (0x08284900)`, `mat_dial.c`: (model, &material).
pub const SI_MATERIAL_DISPOSE: u32 = 0x0828_4900;
/// `SI_txt2dDispose (0x082ae014)`, `t2d_dial.c`: (model, &texture).
pub const SI_TXT2D_DISPOSE: u32 = 0x082a_e014;
/// `SI_txt3dFree (0x082b4054)`, `t3d_dial.c`: (model, texture).
pub const SI_TXT3D_FREE: u32 = 0x082b_4054;
/// `SI_fittingAllocate (0x0843a130)`, `fitting.c`: (); answers a spline fitting record.
pub const SI_FITTING_ALLOCATE: u32 = 0x0843_a130;
/// `SI_pfittingAllocate (0x0843a384)`, `fitting.c`: (); answers a patch fitting record.
pub const SI_PFITTING_ALLOCATE: u32 = 0x0843_a384;
/// `SI_waveAddMdl (0x084c9810)`, `wave_util.c`: (wave, model, kind, 1); on the image in
/// [`crate::wave_mdl::wave_add_mdl`].
pub const SI_WAVE_ADD_MDL: u32 = 0x084c_9810;
/// `si_relGetDynConsType (0x08101598)`, `edtools.c`: (relation id); answers a 16-bit type.
pub const SI_REL_GET_DYN_CONS_TYPE: u32 = 0x0810_1598;
/// `SI_conslistFindPriority (0x084957cc)`, `conslist.c`: (type); answers a 16-bit priority.
pub const SI_CONSLIST_FIND_PRIORITY: u32 = 0x0849_57cc;
/// `SI_conslistConstrAllocate (0x084958d0)`, `conslist.c`: (type, priority, object).
pub const SI_CONSLIST_CONSTR_ALLOCATE: u32 = 0x0849_58d0;
/// `SI_conslistAddConstraint (0x08495ad8)`, `conslist.c`: (model, constraint, 0).
pub const SI_CONSLIST_ADD_CONSTRAINT: u32 = 0x0849_5ad8;
/// `SI_cusCreateRelations (0x08536828)`, `custom_rel.c`: (model, count, objects).
pub const SI_CUS_CREATE_RELATIONS: u32 = 0x0853_6828;

/// `SI_edSumatraTo3x` (16 bits): a scene of the later program is being converted.
const SUMATRA_TO_3X: u32 = 0x0868_0e50;
/// The words of `SI_cid` the routine reads: ANIMATION (+0x00), CAMERAS (+0x04), LIGHTS
/// (+0x18), MATERIALS (+0x1c), MODELS (+0x20), NSELECTIONS (+0x24), WAVES (+0x70).
const CID: u32 = 0x0878_1660;

/// A float of the relation's data cut to an integer as a C cast does: the unit's
/// conversion with the rounding mode set to "towards zero" (`fistp`; out of range, the
/// "integer indefinite" 0x80000000).
fn cut(m: &Mem, at: u32) -> i32 {
    crate::elemsort::c_cast_to_int(m.f32(at) as f64)
}

/// The scene's structures from one relation id of the dictionary
/// [soft 0x0810b320 SI_scnRelationsFromElemDict]: `SI_scnRelationsFromElemDict(dictionary,
/// unused, chapter, list, relation)`.
///
/// For each element of the chapter whose object is not null and whose new-flag is 1 (with
/// `SI_edSumatraTo3x` set, a named selection of this load without an object first gets one
/// from `SI_nselectionAllocate(name, 0)`): the object is kept as the chapter's (a model,
/// material, camera, light, wave or named selection; another chapter skips the element;
/// **the kept objects stay from one element to the next**). For relation 20000 a model
/// whose algorithm (+0x2ec) is not 13 is skipped. In the forward list `list` (+0x64, no
/// bounds test), the entries with the relation id that do not lead to ANIMATION are
/// counted (n), and of those the ones whose element (`SI_elemdictGetElem`) has an object
/// (k). With n = 0 the element is done. Then, before the entries:
///
/// | Relation | |
/// |---|---|
/// | 300 | when k > 0: the model's materials (+0x20c, +0x206 of them) each `SI_materialDispose(model, &slot)`, the array freed, a new one of k (`SI_calloc(k, 4)`), +0x206 = k, +0x208 = 0 |
/// | 400, 401 | the 2D texture list of the model (+0x21c) or material (+0x6ec), through +0x16c, each `SI_txt2dDispose(null, &texture)`; the head cleared; the count (+0x216, +0x6e8) = k |
/// | 500, 501 | the 3D texture (+0x224, +0x6fc) `SI_txt3dFree(null, texture)` when there is one, then cleared |
/// | 2200 | when k > 0: a table of as many words as the MODELS chapter has slots, cleared, at +0x168 of the light, the count at +0x164 |
/// | 10001 | the named selection's first word = 1, and **the routine ends** |
/// | 20000 | `SI_calloc(n, 4)` for the objects |
///
/// Then each entry with the relation id, not to ANIMATION, whose element has an object
/// (as long as no fitting has been made for this element), in order, with j counting
/// those (from 0) and `data` the entry's data (+0x18):
///
/// | Relation | |
/// |---|---|
/// | 112 | the model's path (+0x2c8) = the object |
/// | 220, 221 | `SI_fittingAllocate()` at +0x60, +0x64; its +4 = the object; the seven floats of the data at +0xc … (through the unit); the data freed and the entry's pointer cleared; no further entry of this element is looked at |
/// | 230, 231 | the same with `SI_pfittingAllocate()` at +0x2d8, +0x2dc and nine floats at +8 … |
/// | 300 | slot j of the material array = the object |
/// | 400, 401 | the texture appended to the list as [`crate::scnrel`] describes (an empty list also sets the tail word +0x218, +0x6f0) |
/// | 500, 501 | +0x224, +0x6fc = the object |
/// | 600, 601 | `SI_waveAddMdl(wave, model, relation is 601, 1)`; the wave is kept |
/// | 612 | the kept wave's model (+0x74), when not null: its path (+0x2c8) = the object; **that model becomes the kept model** |
/// | 800 | the dynamics record (+0x304) of the model, or for a node of kind 0 (+0x30) whose algorithm is 3 or 4, of the first ancestor of kind 0 whose algorithm is 1 or 2: sixteen numbers of the data cut to integers into it (16 bits, the twelfth and fourteenth 32 bits); the data freed |
/// | 801 … 807 | a constraint: `si_relGetDynConsType(relation)`, `SI_conslistFindPriority(type)`, `SI_conslistConstrAllocate(type, priority, object)`; for 807 with an obstacle record (+0x38) and data, the first number cut to 16 bits at +0x78 of it and the data freed; `SI_conslistAddConstraint(model, constraint, 0)` |
/// | 1112, 1114 | the camera's +0xa0, +0xa4 = the object |
/// | 2112, 2114 | the light's +0x144, +0x148 = the object |
/// | 2200 | word (the entry's element index) of the light's table = 1 |
/// | 20000 | object j of the block = the object |
///
/// For 20000, last: `SI_cusCreateRelations(model, j, block)` and the block freed.
///
/// Not done: the original increments `SI_secureLicAlloc` (0x08679660) before each of its
/// three `SI_calloc` calls; that counter belongs to the licence scheme, which is not ported.
pub fn scn_relations_from_elem_dict<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, _unused: u32, chap: i32, group: i32, relid: i32) {
    let cid = |m: &Mem, off: u32| m.i32(CID + off);
    let chapter = m.u32(dict.wrapping_add(4)).wrapping_add((chap as u32).wrapping_shl(6));
    // the objects the routine keeps, as its locals: model, material, camera, light, wave,
    // named selection; and the block of relation 20000. (A relation used on a chapter that
    // does not set the local it works on, 401 on MODELS say, makes the original follow
    // whatever its stack held there; here the local is null and the image records a fault.
    // The loader never makes such a call.)
    let (mut model, mut material, mut camera, mut light, mut wave, mut nsel, mut block) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let mut i = 0i32;
    while i < m.i32(chapter + 4) {
        let e = m.u32(chapter + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        let mut object = m.u32(e + 4);
        let ours = object != 0 && m.i16(e + 0x46) == 1;
        if !ours {
            if m.i16(SUMATRA_TO_3X) == 0 || chap != cid(m, 0x24) || object != 0 || m.i16(e + 0x46) != 1 {
                i += 1;
                continue;
            }
            let name = m.u32(m.u32(e) + 0x14);
            object = o.call_addr(m, SI_NSELECTION_ALLOCATE, &[name, 0]);
            m.set_u32(e + 4, object);
            if object == 0 {
                i += 1;
                continue;
            }
        }
        if chap == cid(m, 0x20) {
            model = object;
        } else if chap == cid(m, 0x1c) {
            material = object;
        } else if chap == cid(m, 0x04) {
            camera = object;
        } else if chap == cid(m, 0x18) {
            light = object;
        } else if chap == cid(m, 0x70) {
            wave = object;
        } else if chap == cid(m, 0x24) {
            nsel = object;
        } else {
            i += 1;
            continue;
        }
        if relid == 20000 && m.i32(model.wrapping_add(0x2ec)) != 13 {
            i += 1;
            continue;
        }
        let list = m.u32(e + 0x64).wrapping_add((group as u32).wrapping_mul(0x18));
        let entry = |m: &Mem, k: i32| m.u32(list + 0xc).wrapping_add((k as u32).wrapping_shl(5));
        let takes = |m: &Mem, r: u32| m.i32(r) == relid && m.i16(r + 0x10) as i32 != cid(m, 0);
        // n: the entries of the relation; k: those whose element has an object
        let (mut n, mut k) = (0i32, 0i32);
        let mut j = 0;
        while j < m.i32(list) {
            let r = entry(m, j);
            if takes(m, r) {
                n += 1;
                let other = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, m.i16(r + 0x10) as i32 as u32, m.u32(r + 4)]);
                if other != 0 && m.u32(other + 4) != 0 {
                    k += 1;
                }
            }
            j += 1;
        }
        if n == 0 {
            i += 1;
            continue;
        }
        match relid {
            300 if k > 0 => {
                if m.u32(model + 0x20c) != 0 {
                    let mut s = 0;
                    while s < m.i16(model + 0x206) as i32 {
                        let slot = m.u32(model + 0x20c).wrapping_add(4 * s as u32);
                        o.call_addr(m, SI_MATERIAL_DISPOSE, &[model, slot]);
                        s += 1;
                    }
                    let array = m.u32(model + 0x20c);
                    if array != 0 {
                        m.free(array);
                        m.set_u32(model + 0x20c, 0);
                    }
                }
                let array = m.si_calloc(k as u32, 4);
                m.set_u32(model + 0x20c, array);
                m.set_u16(model + 0x206, k as u16);
                m.set_u16(model + 0x208, 0);
            }
            400 | 401 => {
                let (owner, head, count) = if relid == 400 { (model, 0x21c, 0x216) } else { (material, 0x6ec, 0x6e8) };
                let mut t = m.u32(owner.wrapping_add(head));
                if t != 0 {
                    // SI_txt2dDispose(null, &local): the local is the routine's own word
                    let local = m.local(4);
                    loop {
                        let next = m.u32(t + 0x16c);
                        m.set_u32(local, t);
                        o.call_addr(m, SI_TXT2D_DISPOSE, &[0, local]);
                        t = next;
                        if next == 0 {
                            break;
                        }
                    }
                    m.release(4);
                }
                m.set_u32(owner.wrapping_add(head), 0);
                m.set_u16(owner.wrapping_add(count), k as u16);
            }
            500 | 501 => {
                let (owner, at) = if relid == 500 { (model, 0x224) } else { (material, 0x6fc) };
                let t = m.u32(owner.wrapping_add(at));
                if t != 0 {
                    o.call_addr(m, SI_TXT3D_FREE, &[0, t]);
                }
                m.set_u32(owner.wrapping_add(at), 0);
            }
            0x898 if k > 0 => {
                let models = m.i32(m.u32(dict.wrapping_add(4)).wrapping_add(4).wrapping_add((cid(m, 0x20) as u32).wrapping_shl(6)));
                let table = m.si_calloc(models as u32, 4);
                m.set_u32(light + 0x168, table);
                m.set_i32(light + 0x164, models);
                let mut w = 0;
                while w < models {
                    let table = m.u32(light + 0x168);
                    m.set_u32(table.wrapping_add(4 * w as u32), 0);
                    w += 1;
                }
            }
            0x2711 => {
                m.set_u32(nsel, 1);
                return;
            }
            20000 => block = m.si_calloc(n as u32, 4),
            _ => {}
        }
        // the entries, in order
        let mut count = 0i32;
        let mut open = true;
        let mut j = 0;
        while j < m.i32(list) {
            let r = entry(m, j);
            let data = m.u32(r + 0x18);
            j += 1;
            if !takes(m, r) {
                continue;
            }
            let other = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, m.i16(r + 0x10) as i32 as u32, m.u32(r + 4)]);
            if other == 0 {
                continue;
            }
            let obj = m.u32(other + 4);
            if obj == 0 || !open {
                continue;
            }
            match relid {
                0xdc | 0xdd | 0xe6 | 0xe7 => {
                    let (patch, at) = match relid {
                        0xdc => (false, 0x60),
                        0xdd => (false, 0x64),
                        0xe6 => (true, 0x2d8),
                        _ => (true, 0x2dc),
                    };
                    let fit = o.call_addr(m, if patch { SI_PFITTING_ALLOCATE } else { SI_FITTING_ALLOCATE }, &[]);
                    m.set_u32(model + at, fit);
                    let obj = m.u32(other + 4);
                    m.set_u32(fit.wrapping_add(4), obj);
                    if data != 0 {
                        let (to, n) = if patch { (8, 9) } else { (0xc, 7) };
                        // each float through the unit (`flds`, `fstps`): a signalling NaN
                        // comes out quiet
                        for f in 0..n {
                            let v = si_x87::x87::F80::from_f32(m.f32(data + 4 * f)).to_f32();
                            m.set_f32(fit.wrapping_add(to + 4 * f), v);
                        }
                        m.free(data);
                        m.set_u32(r + 0x18, 0);
                    }
                    open = false;
                }
                0x70 => m.set_u32(model + 0x2c8, obj),
                300 => {
                    let array = m.u32(model + 0x20c);
                    m.set_u32(array.wrapping_add(4 * count as u32), obj);
                }
                400 | 401 => {
                    let (owner, head, tail) = if relid == 400 { (model, 0x21c, 0x218) } else { (material, 0x6ec, 0x6f0) };
                    let mut p = m.u32(owner.wrapping_add(head));
                    if p == 0 {
                        m.set_u32(owner.wrapping_add(head), obj);
                        m.set_u32(owner.wrapping_add(tail), obj);
                    } else {
                        let mut new = true;
                        while m.u32(p + 0x16c) != 0 {
                            if p == obj {
                                new = false;
                            }
                            p = m.u32(p + 0x16c);
                        }
                        if new && p != obj {
                            m.set_u32(p + 0x16c, obj);
                        }
                    }
                }
                500 => m.set_u32(model + 0x224, obj),
                501 => m.set_u32(material + 0x6fc, obj),
                0x258 | 0x259 => {
                    wave = obj;
                    o.call_addr(m, SI_WAVE_ADD_MDL, &[obj, model, (relid == 0x259) as u32, 1]);
                }
                0x264 => {
                    model = m.u32(wave.wrapping_add(0x74));
                    if model != 0 {
                        m.set_u32(model + 0x2c8, obj);
                    }
                }
                800 => {
                    let mut record = m.u32(model + 0x304);
                    if m.i32(model + 0x30) == 0 && (m.i32(model + 0x2ec) as u32).wrapping_sub(3) <= 1 {
                        let mut node = model;
                        let mut algo = m.i32(node + 0x2ec);
                        while (algo as u32).wrapping_sub(1) > 1 {
                            loop {
                                node = m.u32(node + 0x2bc);
                                if m.i32(node + 0x30) == 0 {
                                    break;
                                }
                            }
                            algo = m.i32(node + 0x2ec);
                        }
                        record = m.u32(node + 0x304);
                    }
                    if record != 0 && data != 0 {
                        // the sixteen numbers, where each goes and whether 32 bits are kept
                        const TO: [(u32, bool); 16] = [
                            (0x38, false), (0x42, false), (0x44, false), (0x46, false), (0x4e, false), (0x3a, false), (0x48, false), (0x4a, false),
                            (0x4c, false), (0x50, false), (0x3e, false), (0x54, true), (0x40, false), (0x58, true), (0x3c, false), (0x5e, false),
                        ];
                        for (f, (to, wide)) in TO.iter().enumerate() {
                            let v = cut(m, data + 4 * f as u32);
                            if *wide {
                                m.set_i32(record + to, v);
                            } else {
                                m.set_u16(record + to, v as u16);
                            }
                        }
                        m.free(data);
                        m.set_u32(r + 0x18, 0);
                    }
                }
                0x321..=0x327 => {
                    let kind = o.call_addr(m, SI_REL_GET_DYN_CONS_TYPE, &[relid as u32]) as i16;
                    let priority = o.call_addr(m, SI_CONSLIST_FIND_PRIORITY, &[kind as i32 as u32]) as i16;
                    let obj = m.u32(other + 4);
                    let constraint = o.call_addr(m, SI_CONSLIST_CONSTR_ALLOCATE, &[kind as i32 as u32, priority as i32 as u32, obj]);
                    if relid == 0x327 {
                        let obstacle = m.u32(constraint.wrapping_add(0x38));
                        if obstacle != 0 && data != 0 {
                            let v = cut(m, data);
                            m.set_u16(obstacle + 0x78, v as u16);
                            m.free(data);
                            m.set_u32(r + 0x18, 0);
                        }
                    }
                    o.call_addr(m, SI_CONSLIST_ADD_CONSTRAINT, &[model, constraint, 0]);
                }
                0x458 => m.set_u32(camera + 0xa0, obj),
                0x45a => m.set_u32(camera + 0xa4, obj),
                0x840 => m.set_u32(light + 0x144, obj),
                0x842 => m.set_u32(light + 0x148, obj),
                0x898 => {
                    let table = m.u32(light + 0x168);
                    let index = m.u32(r + 4);
                    m.set_u32(table.wrapping_add(index.wrapping_mul(4)), 1);
                }
                20000 => m.set_u32(block.wrapping_add(4 * count as u32), obj),
                _ => {}
            }
            count += 1;
        }
        if relid == 20000 {
            o.call_addr(m, SI_CUS_CREATE_RELATIONS, &[model, count as u32, block]);
            if block != 0 {
                m.free(block);
                block = 0;
            }
        }
        i += 1;
    }
}

/// The dynamics record relation 800 writes into, found as the routine finds it (read only).
fn dynamics_record(m: &Mem, model: u32) -> u32 {
    let mut node = model;
    if m.i32(model + 0x30) == 0 && (m.i32(model + 0x2ec) as u32).wrapping_sub(3) <= 1 {
        let mut algo = m.i32(node + 0x2ec);
        let mut steps = 0;
        while (algo as u32).wrapping_sub(1) > 1 && steps < 100_000 {
            loop {
                node = m.u32(node + 0x2bc);
                steps += 1;
                if node == 0 || m.i32(node + 0x30) == 0 || steps >= 100_000 {
                    break;
                }
            }
            if node == 0 {
                return 0;
            }
            algo = m.i32(node + 0x2ec);
        }
    }
    m.u32(node + 0x304)
}

/// Why a call of [`scn_relations_from_elem_dict`] cannot run on the loader's image yet, or
/// `None` when it can. **Not a function of the original**: the loader asks it before each
/// of its calls.
// adapter: not in place
/// The routine follows pointers that the loader's image does not always hold as records:
/// an element's object that is still a value of the loader (`world::OBJECTS`: the waves,
/// the named selections, …), the data of a relation (`world::REL_DATA`), and the
/// constraint that `SI_conslistConstrAllocate` (not on the image) would make. The test
/// goes through the elements and entries as the routine does, without changing anything,
/// and refuses when the routine would read or store through such a pointer: the element's
/// own object when an entry of the relation takes effect (but for relation 20000 and the
/// waves' 600 and 601, whose objects are only handed on), the object at the other end
/// where the routine stores it into a record, the data of 220, 221, 230, 231, 800 and
/// 807 (of 800 only where the model has a dynamics record), and any entry of 807 that
/// takes effect.
pub fn not_on_image(m: &Mem, dict: u32, chap: i32, group: i32, relid: i32) -> Option<&'static str> {
    use si_mem::handle::Kind;
    use si_mem::Ptr32;
    let handle = |p: u32| p != 0 && Kind::of(p).is_some();
    let cid = |off: u32| m.i32(CID + off);
    let kept = [0x20, 0x1c, 0x04, 0x18, 0x70, 0x24].iter().any(|&o| chap == cid(o));
    if !kept {
        return None;
    }
    let ed: Ptr32<si_types::dict::ElemDict> = Ptr32::new(dict);
    let chapter = m.u32(dict + 4).wrapping_add((chap as u32) << 6);
    for i in 0..m.i32(chapter + 4).max(0) as u32 {
        let e = m.u32(chapter + 8) + 0x84 * i;
        let object = m.u32(e + 4);
        if object == 0 || m.i16(e + 0x46) != 1 {
            continue;
        }
        let groups = m.u32(e + 0x64);
        if groups == 0 {
            continue;
        }
        let list = groups + 0x18 * group as u32;
        for k in 0..m.i32(list).max(0) as u32 {
            let r = m.u32(list + 0xc) + 0x20 * k;
            if m.i32(r) != relid || m.i16(r + 0x10) as i32 == cid(0) {
                continue;
            }
            if handle(object) && ![20000, 0x258, 0x259].contains(&relid) {
                return Some("the element's object is not in the image");
            }
            let other = si_dict::elemdict::get_elem(m, ed, m.i16(r + 0x10) as i32, m.i32(r + 4));
            if other.is_null() || m[other].data.is_null() {
                continue;
            }
            if handle(m[other].data.addr()) && ![20000, 0x258, 0x259].contains(&relid) {
                return Some("an object at the other end is not in the image");
            }
            let data = m.u32(r + 0x18);
            // 800 reads the data only for a model with a dynamics record (found as the
            // routine finds it)
            let reads = match relid {
                0xdc | 0xdd | 0xe6 | 0xe7 | 0x327 => true,
                800 => !handle(object) && dynamics_record(m, object) != 0,
                _ => false,
            };
            if reads && handle(data) {
                return Some("the relation's data is not in the image");
            }
            if relid == 0x327 {
                return Some("a constraint (SI_conslistConstrAllocate is not on the image)");
            }
        }
    }
    None
}
