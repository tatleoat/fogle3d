// SPDX-License-Identifier: MIT
//! Three small dictionary-bound functions of the expression files, on the image
//! (finding 0013, section 15): `convertElemid` (`exp_yacc.c`), `si_fcvGetRelGrpFromCustom`
//! (`exp_parsing.c`), `SI_getExprFcurveUsed` (`exp_fct.c`).

use super::rec::{self, cid};
use super::calls::ExprCalls;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{ARelation, Elem};

/// A camera's or a light's element index replaced by the index of the element it is
/// related to [soft 0x08251484 convertElemid]: for chapter CAMERAS (relation 0x456) and
/// LIGHTS (0x83e) the element's first forward relation of that id in group 1 names a
/// related element of the same chapter, whose index is stored in `*index`. Any other
/// chapter, a missing element or no relation leaves `*index` alone.
pub fn convert_elemid(m: &mut Mem, chap: i32, index: Ptr32<i32>) {
    let (cam, lig) = (rec::chap(m, cid::CAM), rec::chap(m, cid::LIG));
    if chap != cam && chap != lig {
        return;
    }
    let ed = rec::cur_dict(m);
    let e = si_dict::elemdict::get_elem(m, ed, chap, m.read(index));
    if e.is_null() {
        return;
    }
    let relid = if chap == cam { 0x456 } else { 0x83e };
    let sp = m.sp();
    let out_chap = Ptr32::<i32>::new(m.local(4));
    let out_index = Ptr32::<i32>::new(m.local(4));
    let r = si_dict::elemrel::find_related_elem(m, ed, e, 1, 1, relid, chap, out_chap, out_index);
    if !r.is_null() {
        let v = m.read(out_index);
        m.write(index, v);
    }
    m.set_sp(sp);
}

/// The relation list (group) a custom-effect curve belongs to [soft 0x0824b118
/// si_fcvGetRelGrpFromCustom]: always the custom-effect group
/// ([soft 0x081200b4 SI_attdictGetGroupCustomFcurve], 5), stored in `*group`; answers 1.
/// The first three arguments (a model, an element, a name) are not looked at.
pub fn fcv_get_rel_grp_from_custom(m: &mut Mem, group: Ptr32<i32>) -> i32 {
    m.write(group, si_dict::attdict::get_group_custom_fcurve());
    1
}

/// The curves the expressions of an element drive [soft 0x08249a00 SI_getExprFcurveUsed]:
/// for each run of relation 20010 (`0x4e2a`) in the element's forward lists whose target
/// is an EXPRESSIONS element with relation data, the data's curve id (appended to the id
/// array that is returned, `*count` of them), its three access words (to `*access`, 12
/// bytes each) and, when `names` is not null, the custom name (to `*names`). `only` is a
/// curve id to keep or −1 for all; 0x236 (a custom parameter) answers null at once.
///
/// The arrays are `SI_calloc`ed again (the old ones freed) when `*count` is lower than the
/// number of runs, when `ids` is null, or when `*access` is null; with no runs both are
/// freed and `*count` is 0. The relation lists' cursors end where `SI_elemdictSetRelToSerialid`
/// puts them for the serial number the first marked entry had (an unset word of the
/// frame when none is marked, as in the original).
#[allow(clippy::too_many_arguments)]
pub fn get_expr_fcurve_used(m: &mut Mem, e: Ptr32<Elem>, count: Ptr32<i32>, ids: u32, names: Ptr32<u32>, access: Ptr32<u32>, only: i32) -> u32 {
    if only == 0x236 {
        return 0;
    }
    let ed = rec::cur_dict(m);
    let n = si_dict::elemrel::serial_rel_count(m, ed, e, 0x4e2a, Ptr32::<ARelation>::NULL, Ptr32::NULL);
    if n == 0 {
        m.write(count, 0);
        if ids != 0 {
            m.free(ids);
        }
        let a = m.read(access);
        if a == 0 {
            return 0;
        }
        m.free(a);
        m.write(access, 0);
        return 0;
    }
    let mut ids = ids;
    let have = m.read(count);
    let renew = if have < n {
        if ids != 0 {
            m.free(ids);
        }
        true
    } else if ids != 0 {
        if m.read(access) != 0 {
            false
        } else {
            m.free(ids);
            true
        }
    } else {
        true
    };
    if renew {
        let a = m.read(access);
        if a != 0 {
            m.free(a);
            m.write(access, 0);
        }
        ids = m.si_calloc(n as u32, 4);
        let a = m.si_calloc(n as u32, 0xc);
        m.write(access, a);
    }
    m.write(count, 0);
    let mut name_list = 0;
    if !names.is_null() {
        name_list = m.si_calloc(n as u32, 4);
        m.write(names, name_list);
    }
    let sp = m.sp();
    let serial_slot = m.local(4); // `local_c`: set below only when a list holds a marked entry
    let list_index = Ptr32::<i32>::new(m.local(4));
    if si_dict::elemrel::set_curr_rel_pointer(m, ed, e, 0x4e2a, list_index) as i16 != 0 {
        let g = m.read(list_index);
        let r = si_dict::elemrel::get_cur_relation(m, ed, e, 1, g).addr();
        let serial = m.u32(r + 0xc);
        m.set_u32(serial_slot, serial);
        let groups = m.u32(e.addr() + 100);
        m.set_u32(groups.wrapping_add(8).wrapping_add((g as u32).wrapping_mul(0x18)), 0);
    }
    let exp_chap = rec::chap(m, cid::EXP);
    let mut first = 1;
    loop {
        let r = si_dict::elemrel::find_serial_relation(m, ed, e, 0x4e2a, list_index, -1, 1, first).addr();
        if r == 0 {
            break;
        }
        let data = m.u32(r + 0x18);
        if m.i16(r + 0x10) as i32 == exp_chap && data != 0 && (only == -1 || only == m.i32(data)) {
            let k = m.i32(count.addr()) as u32;
            let id = m.u32(data);
            m.set_u32(ids.wrapping_add(4 * k), id);
            if !names.is_null() {
                let name = m.u32(data + 0x10);
                m.set_u32(name_list.wrapping_add(4 * k), name);
            }
            let a = m.read(access);
            for w in 0..3 {
                let v = m.u32(data + 4 + 4 * w);
                m.set_u32(a.wrapping_add(12 * k + 4 * w), v);
            }
            m.write(count, k as i32 + 1);
        }
        first = 0;
    }
    let serial = m.i32(serial_slot);
    si_dict::elemrel::set_rel_to_serialid(m, ed, e, 0x4e2a, serial, 1);
    m.set_sp(sp);
    ids
}

// The statics of `SI_copyElemExpression` (`exp_fct.c`): the number of curves the target's
// expressions drive, their ids, their access words (twelve bytes each: the curve's first and
// second index, the cluster's id), and the text buffer with its size.
const NB_DFCV: u32 = 0x08699e6c;
const DFCV: u32 = 0x08699e70;
const DFCV_SUBID: u32 = 0x08699e74;
const BUF: u32 = 0x08699e64;
const BUF_SIZE: u32 = 0x08699e68;
/// `SI_RD`.
const SI_RD_ADDR: u32 = 0x087815f0;

/// `prefix-name`, or the name alone for a null or empty prefix; a null name prints as `(null)`
/// (what `sprintf` makes of it). The same as `SI_dbFormatPrefixName` (0x08121ca8), which
/// `si-scene` has on the image; it depends on this crate, so the few lines are repeated.
fn prefix_name(m: &Mem, prefix: u32, name: u32) -> Vec<u8> {
    let mut text = Vec::new();
    if prefix != 0 && m.strlen(prefix) != 0 {
        text = m.cstr(prefix);
        text.push(b'-');
    }
    if name == 0 { text.extend_from_slice(b"(null)") } else { text.extend_from_slice(&m.cstr(name)) }
    text
}

/// The element type word of a chapter written to `out`, with the blank after it and a zero byte
/// [soft 0x0824f620 SI_getObjTypeFromChapid]: `CAM `, `LIG `, `MAT `, `T2D `, `T3D `, `WAV ` for those
/// chapters of the scene dictionary (`SI_cid`), `MOD ` for any other.
pub fn get_obj_type_from_chapid(m: &mut Mem, chap: i32, out: u32) {
    let word: &[u8; 4] = if chap == rec::chap(m, cid::CAM) {
        b"CAM "
    } else if chap == rec::chap(m, cid::LIG) {
        b"LIG "
    } else if chap == rec::chap(m, cid::MAT) {
        b"MAT "
    } else if chap == rec::chap(m, cid::T2D) {
        b"T2D "
    } else if chap == rec::chap(m, cid::T3D) {
        b"T3D "
    } else if chap == rec::chap(m, cid::WAV) {
        b"WAV "
    } else {
        b"MOD "
    };
    m.set_bytes(out, word);
    m.set_u8(out + 4, 0);
}

/// Copies the expressions of one element to another of the same kind
/// [soft 0x082493fc SI_copyElemExpression]: for each expression (relation 20010) of the source
/// `src` whose target curve the destination `dst` has no expression for yet, the expression's
/// text is rebuilt with the destination's name in place of the source's (`<type> <name>.<curve>
/// = <right-hand side>`), checked, parsed, made the element's expression and tied to the
/// destination. The curve counts as the destination's when the ids match and, for the cluster
/// curves 0xb7 and 0xb8, both cluster words, for 0x9a the first word, for a custom parameter
/// (0x236) the name; any other id matches on the id alone. A copy that makes the dictionary
/// cyclic is taken back (relation removed, expression given up) and the answer is 1, else 0.
/// A text that does not check prints `Expression invalide(<text>)!`.
///
/// `dchap` and `dindex` are the destination's chapter and index (the removal of a cyclic copy
/// names the relation by them). The parser, the checker and the printer of expressions are
/// outside calls (`calls.rs`).
///
/// Differs from the original where it would read nothing valid: a target text without a dot
/// makes the original read from address 1.
pub fn copy_elem_expression(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Elem>, dst: Ptr32<Elem>, dindex: i32, dchap: i32) -> i16 {
    let sp = m.sp();
    let ed = rec::cur_dict(m);
    let exp_chap = rec::chap(m, cid::EXP);
    let mut cyclic = 0i16;
    let mut saved_serial = -1i32;
    // the expression's name: cleared once on entry, so a later copy reuses the first one's name
    let name_cell = m.local(4);
    m.set_u32(name_cell, 0);
    let idx_cell = Ptr32::<i32>::new(m.local(4));
    if si_dict::elemrel::set_curr_rel_pointer(m, ed, src, 0x4e2a, idx_cell) as i16 != 0 {
        let g = m.read(idx_cell);
        let r = si_dict::elemrel::get_cur_relation(m, ed, src, 1, g);
        saved_serial = m.i32(r.addr() + 0xc);
        let groups = m.u32(src.addr() + 100);
        m.set_u32(groups.wrapping_add(8).wrapping_add((g as u32).wrapping_mul(0x18)), 0);
    }
    let names_cell = Ptr32::<u32>::new(m.local(4));
    m.set_u32(names_cell.addr(), 0);
    let ids = get_expr_fcurve_used(m, dst, Ptr32::new(NB_DFCV), m.u32(DFCV), names_cell, Ptr32::new(DFCV_SUBID), -1);
    m.set_u32(DFCV, ids);
    let rel_cell = Ptr32::<i32>::new(m.local(4));
    let mut first = 1i16;
    let mut rel = si_dict::elemrel::find_serial_relation(m, ed, src, 0x4e2a, rel_cell, -1, 1, first);
    while !rel.is_null() {
        let r = rel.addr();
        let data = m.u32(r + 0x18);
        let mut found = false;
        if m.i16(r + 0x10) as i32 == exp_chap && data != 0 {
            let n = m.i32(NB_DFCV);
            let mut k = 0;
            while k < n {
                let fcv = m.i32(m.u32(DFCV).wrapping_add(4 * k as u32));
                let sub = m.u32(DFCV_SUBID).wrapping_add(12 * k as u32);
                if fcv == m.i32(data) {
                    found = if fcv == 0xb7 || fcv == 0xb8 {
                        m.i32(sub + 4) == m.i32(data + 8) && m.i32(sub) == m.i32(data + 4)
                    } else if fcv == 0x9a {
                        m.i32(sub) == m.i32(data + 4)
                    } else if fcv == 0x236 {
                        let a = m.u32(data + 0x10);
                        let b = m.u32(m.u32(names_cell.addr()).wrapping_add(4 * k as u32));
                        a != 0 && b != 0 && si_mem::clib::strcmp(m, a, b) == 0
                    } else {
                        true
                    };
                    if found {
                        break;
                    }
                }
                k += 1;
            }
        }
        if !found {
            // the expression of the source is copied
            let ee = si_dict::elemdict::get_elem(m, ed, exp_chap, m.i32(r + 4));
            let expr = m.u32(ee.addr() + 4);
            let target = Ptr32::<u32>::new(m.local(4));
            let nvars = Ptr32::<i32>::new(m.local(4));
            let vars = Ptr32::<u32>::new(m.local(4));
            let rhs = o.get_complete_expr_format(m, expr, target, nvars, vars, 1, 0);
            let mut objtype = Vec::new();
            if dchap != rec::chap(m, cid::MODEL) {
                let tmp = m.local(8);
                get_obj_type_from_chapid(m, dchap, tmp);
                objtype = m.cstr(tmp);
            }
            let dot = m.cstr(m.u32(target.addr())).iter().position(|&c| c == b'.');
            let after = match dot {
                Some(i) => m.u32(target.addr()).wrapping_add(i as u32 + 1),
                // (the original's strchr answers null and it reads from address 1)
                None => 1,
            };
            let node = m.u32(dst.addr());
            let pname = prefix_name(m, m.u32(node + 0x10), m.u32(node + 0x14));
            let after_text = m.cstr(after);
            let rhs_text = if rhs == 0 { b"(null)".to_vec() } else { m.cstr(rhs) };
            let need = (objtype.len() + pname.len() + after_text.len() + rhs_text.len() + 6) as i32;
            let have = m.i32(BUF_SIZE);
            if need > have {
                let b = if have == 0 { m.si_calloc(need as u32, 1) } else { m.si_realloc(m.u32(BUF), need as u32) };
                m.set_u32(BUF, b);
                m.set_i32(BUF_SIZE, need);
            }
            let mut line = objtype.clone();
            line.push(b' ');
            line.extend_from_slice(&pname);
            line.push(b'.');
            line.extend_from_slice(&after_text);
            line.extend_from_slice(b" = ");
            line.extend_from_slice(&rhs_text);
            let buf = m.u32(BUF);
            m.put_cstr(buf, &line);
            let err = o.valid_custom_express(m, si_mem::CStr32::new(buf), m.u32(vars.addr()), m.i32(nvars.addr()), Ptr32::NULL, 1, 1);
            if err == 0 {
                let out = Ptr32::<u32>::new(m.local(4));
                o.gen_cus_expr_parsing_tree(m, si_mem::CStr32::new(buf), m.u32(vars.addr()), m.i32(nvars.addr()), out, 1, 0);
                si_dict::elemrel::reset_curr_flag(m, dst, 0x4e2a);
                let made = super::parsing::ed_element_set_expression(m, o, m.u32(out.addr()), name_cell, -1);
                let cycle = si_dict::toposort::check_cycle(m, &mut super::calls::DictOutside(o), Ptr32::new(SI_RD_ADDR), ed);
                if cycle {
                    si_dict::elemrel::reset_curr_flag(m, dst, 0x4e2a);
                    let new_index = m.i32(made + 4);
                    si_dict::elemrel::remove_serial_rel_id(m, &mut super::calls::DictOutside(o), ed, Ptr32::new(SI_RD_ADDR), dchap, dindex, 0x4e2a, m.i32(made + 0xc));
                    super::exio::ed_dispose_expression(m, o, new_index);
                    cyclic = 1;
                }
                si_dict::elemrel::set_rel_to_serialid(m, ed, src, 0x4e2a, m.i32(r + 0xc), 1);
            } else {
                let mut text = b"Expression invalide(".to_vec();
                text.extend_from_slice(&m.cstr(buf));
                text.extend_from_slice(b")!\n");
                o.print(1, &text);
            }
        }
        first = 0;
        rel = si_dict::elemrel::find_serial_relation(m, ed, src, 0x4e2a, rel_cell, -1, 1, first);
    }
    if m.i32(NB_DFCV) != 0 && m.u32(names_cell.addr()) != 0 {
        let n = m.u32(names_cell.addr());
        m.free(n);
        m.set_u32(names_cell.addr(), 0);
    }
    si_dict::elemrel::set_rel_to_serialid(m, ed, src, 0x4e2a, saved_serial, 1);
    m.set_sp(sp);
    cyclic
}
