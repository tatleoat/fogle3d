// SPDX-License-Identifier: MIT
//! The dictionaries' dump routines (`enmdict.c`, `reldict.c`, `relations.c`, `elemdict.c`,
//! `updlist.c`, `toposort.c`, `hierani.c`): debugging output of the original, on the memory
//! image. They print and change nothing; the text goes to the outside world's standard
//! error (`fprintf(stderr, ...)`) or standard output (`printf`), as the original's does.
//!
//! A `%s` of a null string is `(null)` as the C library prints it; a `%d` of a pointer is
//! the 32-bit address as a signed number.

use si_dict::elemdict::{get_chap_name, get_elem};
use si_dict::outside::Outside;
use si_dict::reldict::get_rel_name;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, ElemList, EnumVal, EnumValDict, EnumValType, RelDict, RelList, RelRule, Relations};

/// A `%s` argument.
fn s(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

/// Builds a line: literal pieces and arguments in order.
struct Line(Vec<u8>);

impl Line {
    fn new(lit: &[u8]) -> Line {
        Line(lit.to_vec())
    }
    fn d(mut self, v: i32) -> Line {
        self.0.extend_from_slice(v.to_string().as_bytes());
        self
    }
    fn b(mut self, v: &[u8]) -> Line {
        self.0.extend_from_slice(v);
        self
    }
}

/// [soft 0x081386fc SI_enmdictDump]: every type with its values, on the standard error.
pub fn enmdict_dump(m: &Mem, o: &mut dyn Outside, dict: Ptr32<EnumValDict>) {
    o.stderr(b" Dictionary of Enumerated Values:\n");
    o.stderr(b"---------------------------------\n");
    let mut i = 0;
    while i < m[dict].nb_types {
        let t: Ptr32<EnumValType> = m[dict].types.add(i);
        o.stderr(&Line::new(b"").d(i).b(b") ENUM Type [").b(&s(m, m[t].name.addr())).b(b"]\n").0);
        i += 1;
        let mut k = 0;
        while k < m[t].nb_values {
            let v: Ptr32<EnumVal> = m[t].values.add(k);
            o.stderr(&Line::new(b"   ").d(k).b(b") ").d(m[v].id).b(b" ").b(&s(m, m[v].code.addr())).b(b" (").b(&s(m, m[v].name.addr())).b(b") \n").0);
            k += 1;
        }
    }
    o.stderr(b"--------------------------------------\n");
}

/// [soft 0x0813a7bc SI_reldictDump]: every rule of the relation dictionary, on the standard
/// error.
pub fn reldict_dump(m: &Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>) {
    o.stderr(b" Dictionary of Relations:\n");
    o.stderr(b"--------------------------\n");
    let mut i = 0;
    while i < m[rd].nb {
        let r: Ptr32<RelRule> = m[rd].rules.add(i);
        let l = Line::new(b"").d(i).b(b") ").b(&s(m, m[r].name.addr())).b(b"(").d(m[r].relid).b(b") chp[").d(m[r].src_chap as i32).b(b"->").d(m[r].dst_chap as i32);
        let l = l.b(b"] grp[").d(m[r].src_group as i32).b(b"->").d(m[r].dst_group as i32).b(b"] rk(").d(m[r].rank as i32).b(b") fo(").d(m[r].fanout as i32);
        let l = l.b(b") dd(").d(m[r].dope_disp as i32).b(b") act(").d(m[r].active as i32).b(b") actdep(").d(m[r].active_dep as i32).b(b")\n");
        o.stderr(&l.0);
        i += 1;
    }
    o.stderr(b"--------------------------\n");
}

/// [soft 0x081391a0 si_relGroupDump]: one list of an element's relations, on the standard
/// error.
pub fn rel_group_dump(m: &Mem, o: &mut dyn Outside, rels: Ptr32<Relations>, group: i32) {
    let g: Ptr32<RelList> = m[rels].groups.add(group);
    let vis = m[g].unk_10;
    o.stderr(&Line::new(b"  |  |  |  |Group ").d(group).b(b" ------------------ vis[").d(vis).b(b"] --\n").0);
    let mut k = 0;
    while k < m[g].nb {
        let a: Ptr32<ARelation> = m[g].rels.add(k);
        let l = Line::new(b"  |  |  |  |").d(k).b(b" relid[").d(m[a].relid).b(b"] activ[").d(m[a].flag as i32).b(b"] grp[").d(m[a].group as i32);
        let l = l.b(b"] chp[").d(m[a].chapid as i32).b(b"] elem[").d(m[a].elemid).b(b"] data[").d(m[a].data.addr() as i32).b(b"]");
        o.stderr(&l.0);
        if m[a].serial >= 0 {
            o.stderr(&Line::new(b" serialid[").d(m[a].serial).b(b"]").0);
        }
        if m[a].active_elem >= 0 {
            o.stderr(&Line::new(b" actfcv[").d(m[a].active_elem).b(b"]").0);
        }
        o.stderr(b"\n");
        k += 1;
    }
}

/// [soft 0x08134940 SI_elemlistDump]: one chapter's elements, with their name records,
/// their relations and their flags, on the standard error. (The path of a name record is
/// shown by its last 20 characters.)
pub fn elemlist_dump(m: &Mem, o: &mut dyn Outside, list: Ptr32<ElemList>) {
    o.stderr(b"  |  |Elements  ===============================\n");
    let mut i = 0;
    while i < m[list].nb_elems {
        let e: Ptr32<Elem> = m[list].elems.add(i);
        let l = Line::new(b"  |  |  |Element ").d(i).b(b" (selected ").d(m[e].selected as i32).b(b")(dirty ").d(m[e].dirty as u16 as i32).b(b")(new ").d(m[e].new_flag as i32);
        let l = l.b(b")(wrkid ").d(m[e].work_id).b(b")(lev ").d(m[e].hier_level).b(b")(root ").d(m[e].root_id).b(b")(FLAG ").d(m[e].work_flag).b(b")\n");
        o.stderr(&l.0);
        if m[e].status != 0 {
            o.stderr(&Line::new(b"  |  |  |Deleted = ").d(m[e].status as i32).b(b"\n").0);
        }
        let node = m[e].node;
        if !node.is_null() {
            let n = node.addr();
            o.stderr(b"  |  |  |  |");
            o.stderr(&Line::new(b"(").b(&s(m, m.u32(n + 0xc))).b(b") ").0);
            o.stderr(&Line::new(b"").b(&s(m, m.u32(n + 0x24))).b(b" ").0);
            o.stderr(&Line::new(b"[").b(&s(m, m.u32(n + 0x10))).b(b" ").0);
            o.stderr(&Line::new(b"").b(&s(m, m.u32(n + 0x14))).b(b"]").0);
            let path = m.u32(n);
            if path == 0 {
                o.stderr(b"\n");
            } else {
                let len = m.strlen(path);
                let skip = if len > 0x14 { len - 0x14 } else { 0 };
                o.stderr(&Line::new(b" DB ...").b(&s(m, path + skip)).b(b"\n").0);
            }
            o.stderr(&Line::new(b"  |  |  |  | subdir [").b(&s(m, m.u32(n + 8))).b(b"]\n").0);
        }
        o.stderr(&Line::new(b"  |  |  |  |").d(m[e].data.addr() as i32).b(b" aux(").d(m[e].aux_data.addr() as i32).b(b") icn(").d(m[e].icon.addr() as i32).b(b")\n").0);
        o.stderr(&Line::new(b"  |  |  |Relations >> for elem ").d(i).b(b"\n").0);
        i += 1;
        let rels: Ptr32<Relations> = Ptr32::new(e.addr() + 0x60);
        let mut g = 0;
        while g < m[rels].nb_groups {
            rel_group_dump(m, o, rels, g);
            g += 1;
        }
    }
    o.stderr(b"  |  |==========================================\n");
}

/// [soft 0x08134ba8 SI_elemdictDump]: every chapter (in the dictionary's chapter order, when
/// it has one), on the standard error.
pub fn elemdict_dump(m: &Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    o.stderr(b"Dictionnary >>>>>>>>>>>>>>>>>>>>>>>\n");
    let mut i = 0;
    while i < m[ed].nb_lists {
        let c = if m[ed].chap_sort.is_null() { i } else { m[m[ed].chap_sort.add(i)] };
        let l: Ptr32<ElemList> = m[ed].lists.add(c);
        let name: CStr32 = m[l].name;
        o.stderr(&Line::new(b"  |Chapter ").d(c).b(b" (").b(&s(m, name.addr())).b(b") chpicn(").d(m[l].unk_24).b(b")\n").0);
        elemlist_dump(m, o, l);
        i += 1;
    }
    o.stderr(b">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>\n");
}

/// [soft 0x081415e0 SI_updlistDump]: an update list on the standard output. With `raw`
/// zero (as `SI_elemdictTopoSortDump` passes it when asked for the plain form) each step
/// shows its numbers, and the first argument of the step is read whether there is one or
/// not; otherwise the names of the chapters, relations and elements, and the steps of an
/// element that is a free slot are left out.
pub fn updlist_dump(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, list: Ptr32<si_types::dict::UpdList>, raw: i16) {
    o.stdout(b" \n\n UPDATE LIST: \n");
    let mut i = 0;
    while i < m[list].nb {
        let n = m[list].nodes.addr() + (i as u32).wrapping_mul(0x2c);
        let chap = m.i16(n) as i32;
        let group = m.i16(n + 2) as i32;
        let elemid = m.i32(n + 4);
        let relid = m.i32(n + 0x14);
        let args = m.u32(n + 0x1c);
        if raw == 0 {
            let l = Line::new(b"").d(i).b(b") chp,elm,grp[").d(chap).b(b",").d(elemid).b(b",").d(group).b(b"] = rel[").d(relid).b(b"] (chp,elm[");
            let l = l.d(m.i16(args) as i32).b(b",").d(m.i32(args + 4)).b(b"])\n");
            o.stdout(&l.0);
        } else {
            let cname = get_chap_name(m, ed, chap);
            let e = get_elem(m, ed, chap, elemid);
            if m[e].status == 0 {
                let full = m.u32(m[e].node.addr() + 0x24);
                let rname = get_rel_name(m, rd, relid);
                let mark = if m.i32(n + 0x24) == 0 { b' ' } else { b'+' };
                let l = Line::new(b"").d(i).b(b") ").b(&s(m, cname.addr())).b(b" ").b(&s(m, full)).b(b" (grp ").d(group).b(b") = ").b(&s(m, rname.addr())).b(&[mark]).b(b"(");
                o.stdout(&l.0);
                let nb = m.i16(n + 0x18) as i32;
                let mut j = 0;
                while j < nb {
                    let a = args.wrapping_add((j as u32).wrapping_mul(0x14));
                    let c2 = m.i16(a) as i32;
                    let cn = get_chap_name(m, ed, c2);
                    let ae = get_elem(m, ed, c2, m.i32(a + 4));
                    let tilde = if m.i32(a + 0x10) == 0 { b' ' } else { b'~' };
                    let tail: &[u8] = if m.i16(a + 0xc) != 0 { b"" } else { b"-> INACTIVE" };
                    let l = Line::new(b"").b(&s(m, cn.addr())).b(b" ").b(&s(m, m.u32(m[ae].node.addr() + 0x14))).b(&[tilde]).b(b"(g ").d(m.i16(a + 2) as i32).b(b")").b(tail).b(b",");
                    o.stdout(&l.0);
                    j += 1;
                }
                o.stdout(b")");
                if m.i16(n + 0x20) == 0 {
                    o.stdout(b" -> INACTIVE <-");
                }
                o.stdout(b"\n");
            }
        }
        i += 1;
    }
    o.stdout(b"\n\n");
}

/// [soft 0x0813fef4 SI_elemdictTopoSortDump]: says whether the list is the dictionary's own
/// (the one at +0x0c), then dumps the dictionary and the list.
pub fn topo_sort_dump(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, list: Ptr32<si_types::dict::UpdList>, raw: i16) {
    let global = (list.addr() == ed.addr().wrapping_add(0xc)) as i32;
    o.stdout(&Line::new(b" From eledictTopoSortDump... liste globale? (").d(global).b(b")\n").0);
    elemdict_dump(m, o, ed);
    updlist_dump(m, o, ed, rd, list, raw);
}

/// [soft 0x08138a00 SI_hieranimDump]: a hierarchy animation on the standard output: its
/// address and chapter, each element (name, father), and each of its items (a curve with
/// its selection and action flags and relation ids, or a static value).
pub fn hieranim_dump(m: &Mem, o: &mut dyn Outside, anim: u32) {
    o.stdout(&Line::new(b" HIER ANIM DUMP: pha(").d(anim as i32).b(b") chapid(").d(m.i32(anim + 8)).b(b")\n").0);
    let mut i = 0;
    while i < m.i32(anim) {
        let e = m.u32(anim + 4).wrapping_add((i as u32).wrapping_mul(0x28));
        o.stdout(&Line::new(b" ").d(i).b(b") ").b(&s(m, m.u32(e))).b(b" (fth ").d(m.i32(e + 4)).b(b") \n").0);
        i += 1;
        let nb = m.i32(e + 8);
        let mut k = 0;
        while k < nb {
            let off = |base: u32, size: u32| m.u32(e + base).wrapping_add((k as u32).wrapping_mul(size));
            let kind = m.i16(off(0xc, 2));
            if kind == 0 {
                let p = m.u32(off(0x18, 4));
                let l = Line::new(b"   sel[").d(m.i16(off(0x1c, 2)) as i32).b(b"] act[").d(m.i16(off(0x20, 2)) as i32).b(b"] fcv ").d(p as i32).b(b" ").d(m.i32(p));
                let l = l.b(b" (relid ").d(m.i32(off(0x10, 4))).b(b")(occid ").d(m.i16(off(0x14, 2)) as i32).b(b")\n");
                o.stdout(&l.0);
            } else if kind == 1 {
                let p = m.u32(off(0x18, 4));
                let v = f32::from_bits(m.u32(p + 4)) as f64;
                let l = Line::new(b"   sel[").d(m.i16(off(0x1c, 2)) as i32).b(b"] val type(").d(m.i32(p)).b(b") value(").b(format!("{v:.6}").as_bytes()).b(b")\n");
                o.stdout(&l.0);
            }
            k += 1;
        }
    }
}

/// [soft 0x0813eae0 SI_reldictCusOpDump]: does nothing.
pub fn reldict_cus_op_dump(_rd: Ptr32<RelDict>) {}
