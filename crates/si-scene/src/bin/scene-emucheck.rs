// SPDX-License-Identifier: MIT
//! Compares the routines of `si-scene` with the original, on vectors made by the emulation
//! harness (`re/tools/emu`, finding 0011) from made-up dictionaries and names.
//!
//!   scene-emucheck <dir of .vec files, or of one directory of them per routine>
//!
//! Two ways of running a case:
//!
//! - **On the memory image** (`emucheck_parts/image.rs`): the case's buffers are loaded
//!   into the image where the harness had them, the port runs on them, and memory is
//!   compared. So are checked the ten name-record routines of `dbnode.rs` (vectors of
//!   `re/tools/si-scene/emu_gen_node.py`), the routines of `world.rs`, `dbvsfcv.rs` and
//!   `dupnames.rs` (`emu_gen_world.py`, `emu_gen_fcv.py`, `emu_gen_dup.py`) and the
//!   dictionary routines of the older vectors (`re/tools/si-scene/emu_gen.py`,
//!   `emu_gen_save.py`) whose only port is `si-dict`'s or on the image here.
//! - **On stores**: for the routines this crate still has in store form (its saver, the
//!   element sort, the copies kept for callers not on the image), the case's dictionary,
//!   laid out as the 32-bit original has it, is rebuilt as the crate's own structures,
//!   the port is run, and the result is laid out again and compared word for word.

use si_scene::calls::Recorder;
use si_scene::dict::{Dict, ElemId, Program};
use si_scene::elemrel;
use si_scene::init;
use si_types::dict::{ARelation, Elem};
use si_types::store::{ElemListStore, ElemStore, RelListStore};
use si_types::{Ptr32, Zero};
use si_x87::vectors::{check_dir, print_report, Case, Check, Field, Verdict, Word};
use std::path::Path;

#[path = "emucheck_parts/image.rs"]
mod image;
#[path = "emucheck_parts/save.rs"]
mod save;

const SLOTS: [(i32, usize); 4] = [(0, 1), (1, 2), (7, 2), (8, 5)];
const NL: usize = 9;
const NG: usize = 3;
const K: usize = 3;
const NAMES: [&str; 9] = ["SCENES", "ANIMATION", "CAMERAS", "CLUSTERS", "PICTURES", "LATTICES", "LIGHTS", "MATERIALS", "MODELS"];

fn base(chap: i32) -> Option<usize> {
    let mut s = 0;
    for (c, cap) in SLOTS {
        if c == chap {
            return Some(s);
        }
        s += cap;
    }
    None
}

fn slot(chap: i32, index: i32) -> usize {
    base(chap).unwrap_or(0) + index.max(0) as usize
}

fn val(w: &Word) -> u32 {
    w.value().unwrap_or(0) as u32
}

fn is_null(w: &Word) -> bool {
    matches!(w, Word::Value { value: 0, .. })
}

fn ref_off(w: &Word) -> Option<(String, u32)> {
    match w {
        Word::Ref { name, offset } => Some((name.clone(), *offset)),
        _ => None,
    }
}

/// The dictionary of a case, with what cannot be told from the crate's structures alone.
struct Built {
    d: Dict,
    p: Program,
}

fn c_string(strs: &[u8], w: &Word) -> Option<Vec<u8>> {
    let (_, off) = ref_off(w)?;
    let s = strs.get(off as usize..)?;
    Some(s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec())
}

fn rel_of(w: &[Word]) -> ARelation {
    let mut r = ARelation::zero();
    r.relid = val(&w[0]) as i32;
    r.elemid = val(&w[1]) as i32;
    r.active_elem = val(&w[2]) as i32;
    r.serial = val(&w[3]) as i32;
    r.chapid = val(&w[4]) as i16;
    r.group = (val(&w[4]) >> 16) as i16;
    r.flag = val(&w[5]) as i16;
    r.unk_16 = (val(&w[5]) >> 16) as i16;
    r.data = Ptr32::new(val(&w[6]));
    r.rank = val(&w[7]) as i32;
    r
}

fn rel_words(r: &ARelation) -> [u32; 8] {
    [
        r.relid as u32,
        r.elemid as u32,
        r.active_elem as u32,
        r.serial as u32,
        (r.chapid as u16 as u32) | ((r.group as u16 as u32) << 16),
        (r.flag as u16 as u32) | ((r.unk_16 as u16 as u32) << 16),
        r.data.0,
        r.rank as u32,
    ]
}

fn build(c: &Case) -> Built {
    let (mut d, mut p) = init::empty_scene();
    let ed = c.input("ed").words().to_vec();
    let lists = c.input("lists").words().to_vec();
    let elems = c.input("elems").words().to_vec();
    let groups = c.input("groups").words().to_vec();
    let rels = c.input("rels").words().to_vec();
    let nodes = c.input("nodes").words().to_vec();
    let strs = c.input("strs").to_bytes();
    d.ed.lists.truncate(NL);
    d.ed.rec.nb_lists = NL as i32;
    d.ed.rec.upd_list.dirty = val(&ed[5]) as i32;
    d.ed.rec.topo_list.dirty = val(&ed[21]) as i32;
    d.ed.rec.rel_dirty = (val(&ed[22]) >> 16) as i16;
    d.ed.rec.hier_dirty = (val(&ed[27]) >> 16) as i16;
    for chap in 0..NL {
        let nb = val(&lists[chap * 16 + 1]) as usize;
        let mut l: ElemListStore = d.ed.lists[chap].clone();
        l.name = NAMES[chap].as_bytes().to_vec();
        l.rec.kind = val(&lists[chap * 16 + 6]) as i16;
        l.elems.clear();
        for i in 0..nb {
            let s = slot(chap as i32, i as i32);
            let w = &elems[s * 33..s * 33 + 33];
            let mut e = Elem::zero();
            if !is_null(&w[0]) {
                let n = &nodes[s * 13..s * 13 + 13];
                let mut node = si_db::ListNode::allocate(val(&n[8]) as i32);
                node.sub_path = c_string(&strs, &n[2]);
                node.s0c = c_string(&strs, &n[3]);
                node.prefix = c_string(&strs, &n[4]);
                node.name = c_string(&strs, &n[5]);
                node.version = c_string(&strs, &n[6]);
                node.subst = c_string(&strs, &n[7]);
                node.full = c_string(&strs, &n[9]);
                node.mode = val(&n[11]) as u16;
                node.size = val(&n[12]) as i32;
                e.node = Ptr32::new(d.nodes.alloc(node));
            }
            e.data = Ptr32::new(val(&w[1]));
            e.aux_data = Ptr32::new(val(&w[2]));
            e.schem_x = f32::from_bits(val(&w[3]));
            e.schem_y = f32::from_bits(val(&w[4]));
            e.wire_type = val(&w[8]) as i32;
            for k in 0..4 {
                e.schem_bbox[k] = f32::from_bits(val(&w[11 + k]));
            }
            e.dirty = val(&w[16]) as i16;
            e.selected = (val(&w[16]) >> 16) as i16;
            e.shared = val(&w[17]) as i16;
            e.new_flag = (val(&w[17]) >> 16) as i16;
            e.status = val(&w[18]) as i16;
            e.subst = (val(&w[18]) >> 16) as i16;
            e.dsc_id = val(&w[19]) as i32;
            e.root_id = val(&w[20]) as i32;
            e.hier_level = val(&w[21]) as i32;
            e.work_flag = val(&w[22]) as i32;
            e.work_id = val(&w[23]) as i32;
            e.forward.nb_groups = NG as i32;
            e.backward.nb_groups = NG as i32;
            let side = |first: usize| -> Vec<RelListStore> {
                (0..NG)
                    .map(|g| {
                        let gi = s * 2 * NG + first + g;
                        let gw = &groups[gi * 6..gi * 6 + 6];
                        let mut rec = si_types::dict::RelList::zero();
                        rec.nb = val(&gw[0]) as i32;
                        rec.capacity = val(&gw[1]) as i32;
                        rec.cur = val(&gw[2]) as i32;
                        rec.unk_10 = val(&gw[4]) as i32;
                        rec.flag = val(&gw[5]) as i16;
                        let list: Vec<ARelation> = (0..rec.nb as usize).map(|k| rel_of(&rels[(gi * K + k) * 8..(gi * K + k) * 8 + 8])).collect();
                        RelListStore { rec, rels: list }
                    })
                    .collect()
            };
            let forward = side(0);
            let backward = side(NG);
            l.elems.push(ElemStore { rec: e, forward, backward });
        }
        l.rec.nb_elems = nb as i32;
        d.ed.lists[chap] = l;
    }
    if let Some(rules) = c.try_input("rules") {
        let w = rules.words();
        for i in 0..w.len() / 20 {
            let r = &w[i * 20..i * 20 + 20];
            let h = |x: u32| x as u16 as i16 as i32;
            let idx = p.rd.add_relation(
                b"",
                val(&r[0]) as i32,
                h(val(&r[2])),
                h(val(&r[2]) >> 16),
                h(val(&r[3])),
                h(val(&r[3]) >> 16),
                h(val(&r[4])),
                h(val(&r[4]) >> 16),
                0,
                h(val(&r[5]) >> 16),
                h(val(&r[6])),
                None,
                0,
                3,
            );
            p.rd.rules[idx as usize].active = (val(&r[6]) >> 16) as i16;
            p.rd.rules[idx as usize].active_dep = val(&r[7]) as i16;
        }
    }
    Built { d, p }
}

/// Compares the dictionary after the port ran with the buffers the original left.
fn same_dict(c: &Case, b: &Built) -> Verdict {
    let d = &b.d;
    let ed = c.after("ed").words();
    let diff = std::cell::RefCell::new(Vec::<String>::new());
    let eq = |what: String, got: u32, w: &Word| {
        if let Word::Value { value, .. } = w {
            if *value as u32 != got {
                diff.borrow_mut().push(format!("{what}: port {got:#x}, original {value:#x}"));
            }
        }
    };
    eq("upd dirty".into(), d.ed.rec.upd_list.dirty as u32, &ed[5]);
    eq("topo dirty".into(), d.ed.rec.topo_list.dirty as u32, &ed[21]);
    eq("rel dirty".into(), 1 | ((d.ed.rec.rel_dirty as u16 as u32) << 16), &ed[22]);
    eq("hier dirty".into(), (d.ed.rec.hier_dirty as u16 as u32) << 16, &ed[27]);
    let lists = c.after("lists").words();
    let elems = c.after("elems").words();
    let groups = c.after("groups").words();
    let rels = c.after("rels").words();
    for chap in 0..NL {
        let l = &d.ed.lists[chap];
        eq(format!("chapter {chap} count"), l.elems.len() as u32, &lists[chap * 16 + 1]);
        for (i, e) in l.elems.iter().enumerate() {
            let s = slot(chap as i32, i as i32);
            let w = &elems[s * 33..s * 33 + 33];
            let r = &e.rec;
            let name = format!("element {chap}/{i}");
            // the name record: null or not
            match (&w[0], r.node.is_null()) {
                (Word::Value { value: 0, .. }, false) => diff.borrow_mut().push(format!("{name}: the original cleared the name record")),
                (Word::Ref { .. }, true) => diff.borrow_mut().push(format!("{name}: the original kept the name record")),
                _ => {}
            }
            eq(format!("{name} data"), r.data.0, &w[1]);
            eq(format!("{name} aux"), r.aux_data.0, &w[2]);
            eq(format!("{name} dirty/selected"), (r.dirty as u16 as u32) | ((r.selected as u16 as u32) << 16), &w[16]);
            eq(format!("{name} shared/new"), (r.shared as u16 as u32) | ((r.new_flag as u16 as u32) << 16), &w[17]);
            eq(format!("{name} status/subst"), (r.status as u16 as u32) | ((r.subst as u16 as u32) << 16), &w[18]);
            eq(format!("{name} root"), r.root_id as u32, &w[20]);
            eq(format!("{name} depth"), r.hier_level as u32, &w[21]);
            eq(format!("{name} work flag"), r.work_flag as u32, &w[22]);
            eq(format!("{name} number"), r.work_id as u32, &w[23]);
            for side in 0..2 {
                let ls = if side == 0 { &e.forward } else { &e.backward };
                for (g, list) in ls.iter().enumerate() {
                    let gi = s * 2 * NG + side * NG + g;
                    let gw = &groups[gi * 6..gi * 6 + 6];
                    let ln = format!("{name} {} list {g}", if side == 0 { "forward" } else { "backward" });
                    eq(format!("{ln} count"), list.rels.len() as u32, &gw[0]);
                    eq(format!("{ln} capacity"), list.rec.capacity as u32, &gw[1]);
                    eq(format!("{ln} cursor"), list.rec.cur as u32, &gw[2]);
                    eq(format!("{ln} state"), list.rec.unk_10 as u32, &gw[4]);
                    eq(format!("{ln} flag"), list.rec.flag as u16 as u32, &gw[5]);
                    // the entries: in the case's buffer, or in a block the routine allocated
                    let words: Option<Vec<Word>> = match &gw[3] {
                        Word::Ref { name, offset } if name == "rels" => Some(rels[(*offset as usize) / 4..].to_vec()),
                        Word::Ref { name, offset } => c.try_output(name).map(|f| f.words()[(*offset as usize) / 4..].to_vec()),
                        Word::Value { value: 0, .. } => {
                            if !list.rels.is_empty() {
                                diff.borrow_mut().push(format!("{ln}: the original has no array"));
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(words) = words {
                        if list.rels.is_empty() {
                            diff.borrow_mut().push(format!("{ln}: the original still has an array"));
                        }
                        for (k, r) in list.rels.iter().enumerate() {
                            let got = rel_words(r);
                            for j in 0..8 {
                                if let Some(w) = words.get(k * 8 + j) {
                                    eq(format!("{ln} entry {k} word {j}"), got[j], w);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let diff = diff.into_inner();
    Verdict::from(diff.is_empty(), || diff.join("; "))
}

fn arg(c: &Case, name: &str) -> i32 {
    val(&c.input(name).words()[0]) as i32
}

/// The element a pointer argument designates: its offset in the `elems` buffer.
fn elem_arg(c: &Case, name: &str) -> ElemId {
    let off = ref_off(&c.input(name).words()[0]).map_or(0, |(_, o)| o as usize) / 0x84;
    let mut s = 0;
    for (chap, cap) in SLOTS {
        if off < s + cap {
            return ElemId::new(chap, (off - s) as i32);
        }
        s += cap;
    }
    ElemId::new(-1, -1)
}

fn exists(b: &Built, id: ElemId) -> bool {
    b.d.slot(id).is_some()
}

fn count_calls(c: &Case, name: &str) -> usize {
    c.calls.iter().filter(|k| k.name == name).count()
}

fn eax(c: &Case) -> i32 {
    c.output("eax").i32()
}

fn find_related(c: &Case, first: bool) -> Verdict {
    let mut b = build(c);
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip; // a slot of the buffer beyond the chapter's count
    }
    let (fwd, group, relid, chap) = (arg(c, "fwd") as i16 != 0, arg(c, "group"), arg(c, "relid"), arg(c, "chap"));
    let got = if first { elemrel::find_related_elem(&mut b.d, e, fwd, group, relid, chap) } else { elemrel::find_next_related_elem(&mut b.d, e, fwd, group, relid, chap) };
    let want = ref_off(&c.output("ptr").words()[0]);
    let same_result = match (&got, &want) {
        (None, None) => true,
        (Some(id), Some((_, off))) => slot(id.chap, id.index) * 0x84 == *off as usize && c.output("oc").i32() == id.chap && c.output("oi").i32() == id.index,
        _ => false,
    };
    Verdict::from(same_result, || format!("port {got:?}, original {want:?}")).and(|| same_dict(c, &b))
}

/// What the routines hand to other packages while they remove things, counted on both sides.
fn same_disposals(c: &Case, b: &Built, nodes_before: usize) -> Verdict {
    let (d1, d2) = (b.d.disposed.rel_data.len(), count_calls(c, "SI_elemdictDisposeRelationData"));
    let (n1, n2) = (nodes_before - b.d.nodes.live(), c.calls.iter().filter(|k| k.name == "SI_dbListNodeFree" && !k.args.first().is_some_and(is_null)).count());
    Verdict::from(d1 == d2 && n1 == n2, || format!("relation data disposed: port {d1}, original {d2}; name records freed: port {n1}, original {n2}"))
}

fn set_delete(c: &Case) -> Verdict {
    let mut b = build(c);
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip;
    }
    let before = b.d.nodes.live();
    let got = elemrel::set_delete_elem(&mut b.d, &mut b.p, e, arg(c, "frm"), arg(c, "p6") as i16 != 0, arg(c, "p7") as i16 != 0);
    Verdict::from(got as i32 == eax(c), || "result".into()).and(|| same_dict(c, &b)).and(|| same_disposals(c, &b, before))
}

fn remove_relations(c: &Case) -> Verdict {
    let mut b = build(c);
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip;
    }
    let before = b.d.nodes.live();
    elemrel::rel_remove_relations(&mut b.d, &mut b.p, e, arg(c, "back") as i16 != 0);
    same_dict(c, &b).and(|| same_disposals(c, &b, before))
}

fn remove_a_relation(c: &Case) -> Verdict {
    let mut b = build(c);
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip;
    }
    let g = ref_off(&c.input("l").words()[0]).map_or(0, |(_, o)| o as usize / 0x18) % (2 * NG);
    let before = b.d.nodes.live();
    elemrel::rel_remove_a_relation(&mut b.d, &mut b.p, e, g < NG, (g % NG) as i32, arg(c, "relid"), arg(c, "chap"), arg(c, "index"), arg(c, "serial"));
    same_dict(c, &b).and(|| same_disposals(c, &b, before))
}

fn add(c: &Case, kind: u8) -> Verdict {
    let mut b = build(c);
    let (src, dst, relid, rule, limit) = (arg(c, "src"), arg(c, "dst"), arg(c, "relid"), arg(c, "rule"), arg(c, "limit"));
    let data = Ptr32::new(arg(c, "data") as u32);
    let flag = arg(c, "flag") as i16;
    let got = match kind {
        0 => elemrel::add_complete_relation(&mut b.d, &mut b.p, src, dst, relid, rule, limit, data, flag),
        _ => elemrel::add_uncomplete_relation(&mut b.d, &mut b.p, arg(c, "sc"), arg(c, "dc"), src, dst, relid, rule, limit, data, flag),
    };
    Verdict::from(got as i32 == eax(c), || format!("result: port {got}, original {}", eax(c))).and(|| same_dict(c, &b))
}

/// Where a pointer into the `rels` buffer points: element slot, side, list, position.
fn rel_at(w: &Word) -> Option<(usize, bool, i32, usize)> {
    let (name, off) = ref_off(w)?;
    if name != "rels" {
        return None;
    }
    let entry = off as usize / 0x20;
    let (gi, k) = (entry / K, entry % K);
    Some((gi / (2 * NG), gi % (2 * NG) < NG, (gi % NG) as i32, k))
}

fn rel_query(c: &Case, kind: u8) -> Verdict {
    use si_scene::elemdict as ed;
    let mut b = build(c);
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip;
    }
    let a = |n: &str| arg(c, n);
    let fwd = c.try_input("fwd").is_some_and(|f| val(&f.words()[0]) as u16 != 0);
    let found = |got: Option<ElemId>, c: &Case, second: Option<i32>| {
        let want = ref_off(&c.output("ptr").words()[0]);
        let same = match (&got, &want) {
            (None, None) => true,
            (Some(id), Some((_, off))) => slot(id.chap, id.index) * 0x84 == *off as usize && c.output("oc").i32() == second.unwrap_or(id.chap) && c.output("oi").i32() == id.index,
            _ => false,
        };
        Verdict::from(same, || format!("port {got:?}, original {want:?}"))
    };
    match kind {
        0 => {
            let got = ed::relations_count(&b.d, e, fwd, a("group"), a("og"), a("relid"), a("chap"));
            Verdict::from(got == eax(c), || format!("port {got}"))
        }
        _ => {
            let got = ed::find_range_related_elem(&mut b.d, e, fwd, a("group"), a("lo"), a("hi"), a("chap"));
            found(got.map(|g| g.0), c, got.map(|g| g.1)).and(|| same_dict(c, &b))
        }
    }
}

fn more(c: &Case, kind: u8) -> Verdict {
    use si_scene::elemdict as ed;
    let mut b = build(c);
    let a = |n: &str| arg(c, n);
    let before = b.d.nodes.live();
    if kind == 1 {
        let got = ed::remove_uncomplete_rel_pair(&mut b.d, &mut b.p, a("sc"), a("dc"), a("src"), a("dst"), a("relid"), a("rule"), a("serial"));
        return Verdict::from(got as i32 == eax(c), || format!("result: port {got}")).and(|| same_dict(c, &b)).and(|| same_disposals(c, &b, before));
    }
    let e = elem_arg(c, "e");
    if !exists(&b, e) {
        return Verdict::Skip;
    }
    let fwd = c.try_input("fwd").is_some_and(|f| val(&f.words()[0]) as u16 != 0);
    let range = |got: Option<(ElemId, i32)>, c: &Case| {
        let want = ref_off(&c.output("ptr").words()[0]);
        let same = match (&got, &want) {
            (None, None) => true,
            (Some((id, relid)), Some((_, off))) => slot(id.chap, id.index) * 0x84 == *off as usize && c.output("oc").i32() == *relid && c.output("oi").i32() == id.index,
            _ => false,
        };
        Verdict::from(same, || format!("port {got:?}, original {want:?}"))
    };
    let entry = |got: Option<(usize, i32, usize)>, c: &Case| {
        let want = rel_at(&c.output("ptr").words()[0]).map(|w| (w.0, w.2, w.3));
        Verdict::from(got == want, || format!("port {got:?}, original {want:?}"))
    };
    match kind {
        3 => {
            let got = ed::find_range_next_related_elem(&mut b.d, e, fwd, a("group"), a("lo"), a("hi"), a("chap"));
            range(got, c).and(|| same_dict(c, &b))
        }
        6 => {
            let got = ed::relation_check(&b.d, e, fwd, a("relid"), a("chap"));
            Verdict::from(got as i32 == eax(c), || format!("port {got}"))
        }
        7 => {
            let got = ed::find_serial_relation(&mut b.d, e, a("relid"), a("g"), a("chap"), a("ahead") as i16 != 0, a("restart") as i16 != 0);
            let g_after = c.after("g").words().first().and_then(|w| w.value()).map(|v| v as u32 as i32);
            entry(got.map(|(g, k)| (slot(e.chap, e.index), g, k)), c)
                .and(|| Verdict::from(got.is_none() || g_after == got.map(|x| x.0), || format!("list: port {got:?}, original {g_after:?}")))
                .and(|| same_dict(c, &b))
        }
        _ => {
            let got = ed::get_nth_related_elem(&mut b.d, e, a("n"), fwd, a("group"), a("relid"), a("chap"));
            let want_elem = ref_off(&c.output("out").words()[0]).map(|(_, off)| off as usize);
            let same = got.data.0 == eax(c) as u32 && got.elem.map(|id| slot(id.chap, id.index) * 0x84) == want_elem;
            Verdict::from(same, || format!("port {got:?}, original {:#x} {want_elem:?}", eax(c))).and(|| same_dict(c, &b))
        }
    }
}

fn opt_text(c: &Case, name: &str) -> Option<Vec<u8>> {
    let f = c.input(name);
    if f.is_null() {
        None
    } else {
        Some(text_of(f))
    }
}

fn text_of(f: &Field) -> Vec<u8> {
    let b = f.to_bytes();
    b[..b.iter().position(|&x| x == 0).unwrap_or(b.len())].to_vec()
}

/// The routines checked on the memory image: the case's buffers are loaded into it and
/// the port runs on them ([`image`]).
fn image_entry(name: &str) -> Option<image::Entry> {
    use si_scene::dbnode as n;
    use si_scene::dbvsfcv as f;
    use si_scene::world as w;
    let port: image::Port = match name {
        // dbexternal.c: name records (vectors of re/tools/si-scene/emu_gen_node.py)
        "SI_dbListNodeAllocate" => |m, o, _| n::list_node_allocate(m, o).into(),
        "SI_dbListNodeFree" => |m, _, a| n::list_node_free(m, a.p("node")).into(),
        "SI_dbnameCheckValidity" => |m, _, a| n::name_check_validity(m, a.p("name"), a.s("is_prefix")).into(),
        "SI_dbFormatPrefixName" => |m, _, a| n::format_prefix_name(m, a.p("out"), a.p("prefix"), a.p("name")).into(),
        "SI_dbfilenameSetPrefix" => |m, _, a| n::filename_set_prefix(m, a.p("out"), a.p("node")).into(),
        "SI_dbFormatFullName" => |m, _, a| n::format_full_name(m, a.p("node")).into(),
        "dbfilenameExpand" => |m, _, a| n::filename_expand(m, a.p("name"), [a.v("proj"), a.v("nam"), a.v("vers"), a.v("prec"), a.v("suff")]).into(),
        "SI_chapInfoOffsetFromChapId" => |m, _, a| n::chap_info_offset_from_chap_id(m, a.i("chap")).into(),
        "SI_edGetSuffixFromChapter" => |m, _, a| n::ed_get_suffix_from_chapter(m, a.i("chap")).into(),
        "SI_edGetChapterFromSuffix" => |m, _, a| n::ed_get_chapter_from_suffix(m, a.p("ed"), a.p("suffix")).into(),
        // edanim.c, elemdict.c and the disposers of a relation's data (vectors of
        // re/tools/si-scene/emu_gen_world.py)
        "SI_edGetFcvFromRelation" => |m, _, a| w::ed_get_fcv_from_relation(m, a.p("rel")).into(),
        "SI_extractDataDispose" => |m, _, a| w::extract_data_dispose(m, a.v("pp")).into(),
        "SI_trimmingDataDispose" => |m, _, a| w::trimming_data_dispose(m, a.v("pp")).into(),
        "SI_cusRelDataDestroy" => |m, _, a| w::cus_rel_data_destroy(m, a.v("pp")).into(),
        "SI_exprRelDataDispose" => |m, _, a| w::expr_rel_data_dispose(m, a.v("data")).into(),
        "SI_elemdictDisposeRelationData" => |m, o, a| w::elemdict_dispose_relation_data(m, o, a.p("rel")).into(),
        // dbvsfcv.c (vectors of re/tools/si-scene/emu_gen_fcv.py; the older vectors of
        // SI_relGetAnimFRelFromChapter are in `dict_entry`)
        // scn2db.c (vectors of re/tools/si-scene/emu_gen_dup.py)
        "si_dbCheckChapterDuplicateNames" => |m, o, a| (si_scene::dupnames::check_chapter_duplicate_names(m, o, a.p("list"), a.i("chap")), ()).1.into(),
        "SI_dbCheckDuplicateNames" => |m, o, a| (si_scene::dupnames::db_check_duplicate_names(m, o, a.p("ed")), ()).1.into(),
        "SI_relGetAnimFRelFromChapter" => |m, _, a| f::rel_get_anim_f_rel_from_chapter(m, a.i("chap")).into(),
        "SI_fcvGetTypeFromName" => |m, _, a| f::fcv_get_type_from_name(m, a.p("name")).into(),
        "SI_fcvGetRelGrpFromType" => |m, _, a| f::fcv_get_rel_grp_from_type(m, a.i("id")).into(),
        "SI_fcvGetNameFromType" => |m, _, a| f::fcv_get_name_from_type(m, a.i("id")).into(),
        "SI_fcvGetChapidFromType" => |m, _, a| f::fcv_get_chapid_from_type(m, a.i("id")).into(),
        "SI_fcvGetName" => |m, _, a| f::fcv_get_name(m, a.v("curve")).into(),
        "SI_fcvGetRelInfo" => |m, _, a| f::fcv_get_rel_info(m, a.i("id"), a.p("relid"), a.p("rule"), a.p("limit")).into(),
        "SI_relGetFcvType" => |m, _, a| f::rel_get_fcv_type(m, a.i("relid")).into(),
        "SI_fcvCheckTypeRange" => |m, _, a| f::fcv_check_type_range(m, a.v("curve"), a.i("lo"), a.i("hi")).into(),
        // the routines around the element readers (vectors of re/tools/si-scene/emu_gen_read.py)
        "si_filenameCleanup" => |m, _, a| si_scene::image::filename_cleanup(m, a.v("name"), a.v("base")).into(),
        "SI_omodelBinaryRead" => |m, o, a| si_scene::image::omodel_binary_read(m, &mut image::HrcSide(o), a.v("name"), a.v("model"), a.s("mode")).into(),
        "SI_modelSetState" => |m, o, a| si_scene::image::model_set_state(o, m, a.v("node"), a.v("scal"), a.v("rot"), a.v("trans")).into(),
        // db2scn.c and the three disposals (vectors of re/tools/si-scene/emu_gen_scnrel.py)
        "SI_scnRelationsFromElemDict" => |m, o, a| si_scene::db2scn::scn_relations_from_elem_dict(o, m, a.v("ed"), a.v("rd"), a.i("chap"), a.i("group"), a.i("relid")).into(),
        "SI_materialDispose" => |m, o, a| si_scene::dispose::material_dispose(o, m, a.v("model"), a.v("slot")).into(),
        "SI_txt2dDispose" => |m, o, a| si_scene::dispose::txt2d_dispose(o, m, a.v("model"), a.v("slot")).into(),
        "SI_txt3dFree" => |m, o, a| si_scene::dispose::txt3d_free(o, m, a.v("model"), a.v("tex")).into(),
        // wave_util.c (vectors of re/tools/si-scene/emu_gen_wavemdl.py)
        "SI_waveAddMdl" => |m, o, a| si_scene::wave_mdl::wave_add_mdl(o, m, a.v("wave"), a.v("model"), a.s("branch"), a.s("keep")).into(),
        // wave_sys.c, wave_update.c, wave_util.c, edtools.c (vectors of
        // re/tools/si-scene/emu_gen_wave.py)
        "SI_waveTransform" => |m, o, a| si_scene::wave_sys::wave_transform(o, m, a.v("list"), a.v("model"), a.s("branch"), f32::from_bits(a.v("time")), a.s("check")).into(),
        "si_waveSplineUpdate" => |m, o, a| si_scene::wave_update::wave_spline_update(o, m, a.v("wave"), a.v("input"), a.v("output"), f32::from_bits(a.v("time")), a.s("check")).into(),
        "si_waveGeomUpdate" => |m, o, a| si_scene::wave_geom::wave_geom_update(o, m, a.v("wave"), a.v("model"), f32::from_bits(a.v("time")), a.s("check")).into(),
        "si_waveComputeVectors" => |m, o, a| si_scene::wave_mdl::wave_compute_vectors(o, m, a.v("wave"), a.v("p"), a.v("u"), a.v("v"), a.v("s")).into(),
        "si_waveFindGraphValue" => |m, _, a| si_scene::wave_mdl::wave_find_graph_value(m, a.v("graph"), f32::from_bits(a.v("x"))).into(),
        "si_waveDotProduct" => |m, _, a| si_scene::wave_mdl::wave_dot_product(m, a.v("a"), a.v("b")).into(),
        "si_waveNorm" => |m, _, a| si_scene::wave_mdl::wave_norm(m, a.v("a")).into(),
        "si_splineIndexToValue" => |m, _, a| si_scene::wave_mdl::spline_index_to_value(m, a.s("kind"), a.s("count"), a.s("index"), a.v("out")).into(),
        "SI_edGetElemFromWave" => |m, o, a| si_scene::edtools::ed_get_elem_from_wave(o, m, a.v("wave"), a.p("index")).into(),
        // edtools.c (vectors of re/tools/si-scene/emu_gen_grafinfo.py)
        "SI_edGetElementsGrafNodeInfo" => |m, _, a| si_scene::edtools::ed_get_elements_graf_node_info(m, a.p("dict")).into(),
        // edtools.c: the update's look-ups (vectors of re/tools/si-scene/emu_gen_edlook.py)
        "SI_dbChapter2GrafType" => |m, o, a| si_scene::edtools::db_chapter2graf_type(o, m, a.i("chap"), a.i("index")).into(),
        "SI_edGetElemFromCamera" => |m, o, a| si_scene::edtools::ed_get_elem_from_camera(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edGetElemFromLight" => |m, o, a| si_scene::edtools::ed_get_elem_from_light(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edGetCamFromCamInt" => |m, o, a| si_scene::edtools::ed_get_cam_from_cam_int(o, m, a.i("index")).into(),
        "SI_edGetLigFromLigInt" => |m, o, a| si_scene::edtools::ed_get_lig_from_lig_int(o, m, a.i("index")).into(),
        "SI_edGetElemFromShape" => |m, o, a| si_scene::edtools::ed_get_elem_from_shape(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edGetElemFromLattice" => |m, o, a| si_scene::edtools::ed_get_elem_from_lattice(o, m, a.v("model"), a.s("branch"), a.p("index")).into(),
        "SI_edIsItemConstrained" => |m, o, a| si_scene::edtools::ed_is_item_constrained(o, m, a.v("dict"), a.v("obj"), a.i("chap"), a.i("relid"), a.s("flag"), f32::from_bits(a.v("time"))).into(),
        // edtools.c: a node's material and textures (vectors of re/tools/si-scene/emu_gen_edmat.py)
        "SI_edGetElemFromMaterial" => |m, o, a| si_scene::edtools::ed_get_elem_from_material(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edGetElemFromTexture3D" => |m, o, a| si_scene::edtools::ed_get_elem_from_texture3d(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edModelSetMaterial" => |m, o, a| si_scene::edtools::ed_model_set_material(o, m, a.p("model"), a.v("obj")).into(),
        "SI_edModelRelaxMaterial" => |m, o, a| si_scene::edtools::ed_model_relax_material(o, m, a.v("model"), a.v("obj")).into(),
        "SI_edModelSetGTexture2D" => |m, o, a| si_scene::edtools::ed_model_set_g_texture2d(o, m, a.p("model"), a.v("obj")).into(),
        "SI_edModelRelaxGTexture2D" => |m, o, a| si_scene::edtools::ed_model_relax_g_texture2d(o, m, a.v("model"), a.v("obj")).into(),
        "SI_edModelSetGTexture3D" => |m, o, a| si_scene::edtools::ed_model_set_g_texture3d(o, m, a.p("model"), a.v("obj")).into(),
        "SI_edModelRelaxGTexture3D" => |m, o, a| si_scene::edtools::ed_model_relax_g_texture3d(o, m, a.v("model"), a.v("obj")).into(),
        // edtools.c, first half (vectors of re/tools/edtools1/emu_gen_edfirst.py)
        "relGetFittingRelId" => |_, _, a| (si_scene::edfirst::rel_get_fitting_rel_id(a.s("a"), a.s("b")) as u32).into(),
        "SI_dbGrafType2Chapter" => |m, _, a| (si_scene::edfirst::db_graf_type2chapter(m, a.i("t")) as u32).into(),
        "SI_relGetConsRelFromGrafType" => |_, _, a| (si_scene::edfirst::rel_get_cons_rel_from_graf_type(a.i("t")) as u32).into(),
        "SI_edGetElemFromNSelection" => |m, o, a| si_scene::edfirst::ed_get_elem_from_nselection(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edGetElemFromAnimFile" => |m, o, a| si_scene::edfirst::ed_get_elem_from_anim_file(o, m, a.v("obj"), a.p("index")).into(),
        "SI_edModelGetNewName" => |m, o, a| si_scene::edfirst::ed_model_get_new_name(o, m, a.v("name")).into(),
        "edFindParent" => |m, o, a| si_scene::edfirst::ed_find_parent(o, m, a.v("elem")).into(),
        "SI_edGetClusterFromModel" => |m, o, a| si_scene::edfirst::ed_get_cluster_from_model(o, m, a.v("elem"), a.i("index")).into(),
        "SI_edModelRelaxLTexture3D" => |m, o, a| si_scene::edfirst::ed_model_relax_l_texture3d(o, m, a.v("mat"), a.v("tex")).into(),
        "SI_edModelRelaxLTexture2D" => |m, o, a| si_scene::edfirst::ed_model_relax_l_texture2d(o, m, a.v("mat"), a.v("tex")).into(),
        "SI_edModelLinkWave" => |m, o, a| si_scene::edfirst::ed_model_link_wave(o, m, a.p("model"), a.v("wave")).into(),
        "SI_edModelSetFitting" => |m, o, a| si_scene::edfirst::ed_model_set_fitting(o, m, a.v("m1"), a.v("m2"), a.s("a"), a.s("b")).into(),
        "SI_edModelRelaxFitting" => |m, o, a| si_scene::edfirst::ed_model_relax_fitting(o, m, a.v("m"), a.s("a"), a.s("b")).into(),
        "SI_edModelSetLigSelection" => |m, o, a| si_scene::edfirst::ed_model_set_lig_selection(o, m, a.v("light"), a.p("model")).into(),
        "SI_edModelRemoveLigSelection" => |m, o, a| si_scene::edfirst::ed_model_remove_lig_selection(o, m, a.v("light"), a.p("model")).into(),
        "SI_elemdictFindElemFromGrafNode" => |m, o, a| si_scene::edfirst::elemdict_find_elem_from_graf_node(o, m, a.v("gn"), a.v("chap"), a.v("index")).into(),
        "elemdictFindElemFromDelGrafNode" => |m, o, a| si_scene::edfirst::elemdict_find_elem_from_del_graf_node(o, m, a.v("gn"), a.v("chap"), a.v("index")).into(),
        "SI_edElementSetDeleteFlag" => |m, o, a| si_scene::edfirst::ed_element_set_delete_flag(o, m, a.v("e"), a.i("chap"), a.i("index"), a.s("flag"), a.v("data"), a.v("aux"), a.s("flag2")).into(),
        "SI_elemdictElementSetDeleteFlag" => |m, o, a| si_scene::edfirst::elemdict_element_set_delete_flag(o, m, a.v("ed"), a.v("e"), a.i("chap"), a.i("index"), a.s("flag"), a.v("data"), a.v("aux"), a.s("flag2")).into(),
        "edModelDeleteFlag" => |m, o, a| si_scene::edfirst::ed_model_delete_flag(o, m, a.v("model"), a.s("deep"), a.s("flag"), a.s("flag2")).into(),
        "transferConstraintToRoot" => |m, o, a| si_scene::edfirst::transfer_constraint_to_root(o, m, a.v("elem"), a.i("from"), a.i("to"), a.v("p4"), a.i("relid")).into(),
        "SI_edElementChangeName" => |m, o, a| si_scene::edname::ed_element_change_name(o, m, a.v("elem"), a.i("chap"), a.v("prefix"), a.v("name_at")).into(),
        "SI_edModelChangeName" => |m, o, a| si_scene::edfirst::ed_model_change_name(o, m, a.v("model"), a.v("name")).into(),
        "SI_edGrafNodeSetDeleteFlag" => |m, o, a| si_scene::edfirst::ed_graf_node_set_delete_flag(o, m, a.v("gn"), a.s("del"), a.v("gn2"), a.s("flag2")).into(),
        "SI_edLinkModel" => |m, o, a| si_scene::edfirst::ed_link_model(o, m, a.v("gn1"), a.v("gn2"), a.s("flag")).into(),
        "SI_edCutModel" => |m, o, a| si_scene::edfirst::ed_cut_model(o, m, a.v("model"), a.v("gn"), a.s("flag")).into(),
        "SI_edRelaxRelFromDelModel" => |m, o, a| si_scene::edfirst::ed_relax_rel_from_del_model(o, m, a.v("model"), a.i("relid")).into(),
        "SI_edElementSetConstraint" => |m, o, a| si_scene::edfirst::ed_element_set_constraint(o, m, a.v("item"), a.i("kind"), a.v("target"), a.i("tkind"), a.s("rel"), a.v("data")).into(),
        "SI_edElementRelaxConstraint" => |m, o, a| si_scene::edfirst::ed_element_relax_constraint(o, m, a.v("item"), a.i("kind"), a.s("rel")).into(),
        // the second half: edtools.c, mat_dial.c, t2d_dial.c, shd_fct.c, scn2db.c (the same vectors)
        "SI_edAddMaterial" => |m, o, a| si_scene::edtools::ed_add_material(o, m, a.v("gn"), a.s("unique"), a.v("material")).into(),
        "SI_edModelSetLTexture2D" => |m, o, a| si_scene::edtools::ed_model_set_l_texture2d(o, m, a.v("material"), a.v("obj")).into(),
        "SI_edModelSetLTexture3D" => |m, o, a| si_scene::edtools::ed_model_set_l_texture3d(o, m, a.v("material"), a.v("obj")).into(),
        "SI_attdictFlagAllMaterialFcurves" => |m, o, _| si_scene::edtools::attdict_flag_all_material_fcurves(o, m).into(),
        "SI_materialCopy" => |m, o, a| si_scene::matconv::material_copy(o, m, a.v("material")).into(),
        "SI_txt2dCopy" => |m, o, a| si_scene::matconv::txt2d_copy(o, m, a.v("texture")).into(),
        "SI_copyT2DShader" => |m, o, a| si_scene::matconv::copy_t2d_shader(o, m, a.v("map"), a.v("udf")).into(),
        "dbConvertUdf2DB" => |m, o, a| si_scene::matconv::db_convert_udf2db(o, m, a.v("udf"), a.s("force"), a.i("chap"), a.v("data")).into(),
        // the third part: mat_dial.c, edtools.c, udf_fct.c (the same vectors)
        "SI_materialTransfer" => |m, o, a| si_scene::matconv::material_transfer(o, m, a.v("from"), a.v("to"), a.s("rename")).into(),
        "SI_edGetElemFromScene" => |m, o, a| si_scene::edtools::ed_get_elem_from_scene(o, m, a.p("index")).into(),
        "SI_edSetUDFRelation" => |m, o, a| si_scene::edtools::ed_set_udf_relation(o, m, a.v("owner"), a.i("chap"), a.v("udf"), a.i("udfchap")).into(),
        "SI_edAddUDF" => |m, o, a| si_scene::edtools::ed_add_udf(o, m, a.v("owner"), a.i("chap"), a.v("udf")).into(),
        "SI_udfSetName" => |m, _, a| si_hrc::image::udf::udf_set_name(m, a.p("udf"), a.v("name")).into(),
        // the fourth part: t2d_dial.c, custom_udf.c, custom_shader.c, custom_dialog.c (the same vectors)
        "SI_txt2dTransfer" => |m, o, a| si_scene::matconv::txt2d_transfer(o, m, a.v("from"), a.v("to"), a.i("how")).into(),
        "SI_udfAttachArrayElements" => |m, o, a| si_scene::custom::udf_attach_array_elements(o, m, a.v("udf")).into(),
        "SI_shaderGetDialog" => |m, o, a| si_scene::custom::shader_get_dialog(o, m, a.v("shader")).into(),
        "SI_udfParseRawData" => |m, o, a| si_scene::custom::udf_parse_raw_data(o, m, a.v("udf")).into(),
        "si_cusDialogAttachArrayElements" => |m, o, a| si_scene::custom::cus_dialog_attach_array_elements(o, m, a.v("dialog"), a.v("udf")).into(),
        // the fifth part: graphsort.c, si-view's (the same vectors)
        "SI_schemComputeElemBBox" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_compute_elem_bbox(m, o, a.p("ed"), a.p("elem"), a.i("chap"), a.v("min"), a.v("max"), a.s("done")).into()
        },
        "schemComputeElemRelBBox" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_compute_elem_rel_bbox(m, o, a.p("ed"), a.p("elem"), a.p("rels"), a.s("backward"), a.v("min"), a.v("max"), a.s("done")).into()
        },
        "SI_schematicHierOperation" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schematic_hier_operation(m, o, a.v("port"), a.p("ed"), a.p("elem"), a.i("chap"), a.i("op")).into()
        },
        // the eighth part: graphsort.c, si-view's (the same vectors)
        "si_schemElemOper" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_elem_oper(m, o, a.p("ed"), a.p("elem"), a.i("op"), a.s("first"), a.v("dx"), a.v("dy")).into()
        },
        "schemElemRelOper" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_elem_rel_oper(m, o, a.p("ed"), a.p("elem"), a.p("rels"), a.s("backward"), a.i("op"), a.v("dx"), a.v("dy")).into()
        },
        // the ninth part: graphsort.c, si-view's (the same vectors)
        "SI_schemComputeHierarchiesBBox" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_compute_hierarchies_bbox(m, o, a.p("ed"), a.s("keep"), a.s("only_new")).into()
        },
        "schemChapterComputeHierarchiesBBox" => |m, o, a| {
            let o = &mut image::ViewSide(o);
            si_view::graphsort::schem_chapter_compute_hierarchies_bbox(m, o, a.p("ed"), a.i("chap"), a.p("scene"), a.s("keep"), a.s("only_new")).into()
        },
        // the sixth part: t3d_dial.c, shd_fct.c, edtools.c (the same vectors)
        "SI_txt3dCopy" => |m, o, a| si_scene::matconv::txt3d_copy(o, m, a.v("texture")).into(),
        "SI_copyT3DShader" => |m, o, a| si_scene::matconv::copy_t3d_shader(o, m, a.v("t3d"), a.v("udf")).into(),
        "SI_edAddTexture3D" => |m, o, a| si_scene::edtools::ed_add_texture3d(o, m, a.v("gn"), a.s("unique"), a.v("texture")).into(),
        "SI_edAddTexture2D" => |m, o, a| si_scene::edtools::ed_add_texture2d(o, m, a.v("gn"), a.s("unique"), a.v("texture")).into(),
        "SI_edDisconnectSharedMaterialOr3DTxt" => |m, o, a| {
            let (ed, elem, shared) = (a.v("ed"), a.v("elem"), a.v("shared"));
            si_scene::edtools::ed_disconnect_shared_material_or3d_txt(o, m, ed, elem, a.i("source"), shared, a.i("index"), a.s("material"), a.i("slot"), a.s("global")).into()
        },
        // the seventh part: t3d_dial.c, t2d_dial.c (the same vectors)
        "SI_txt3dTransfer" => |m, o, a| si_scene::matconv::txt3d_transfer(o, m, a.v("from"), a.v("to"), a.i("how")).into(),
        "SI_texture2DVerifyPicturePath" => |m, o, a| {
            si_scene::t2dpath::verify_picture_path(o, m, a.v("dir"), a.v("db"), a.v("ed"), a.v("elem"), a.s("again"), a.s("copy")).into()
        },
        // the tenth part: scn2db.c, the conversion itself (the same vectors)
        "SI_dbConvertModelMatTxt2DB" => |m, o, a| {
            si_scene::matconv::db_convert_model_mat_txt2db(o, m, a.v("model"), a.s("force"), a.s("recurse"), a.v("material")).into()
        },
        // … and the load's call of it, on a dictionary's MODELS chapter (the same vectors)
        "SI_dbCheckConvertModelsMatTxt" => |m, o, a| si_scene::matconv::db_check_convert_models_mat_txt(o, m, a.v("ed")).into(),
        // the eleventh part: dictout_scn.c, the save's restriction (the same vectors)
        "si_elemCheckRestrictedSave" => |m, o, a| si_scene::dictout::elem_check_restricted_save(o, m, a.v("ed"), a.i("chap"), a.v("elem"), a.s("deep")).into(),
        _ => return None,
    };
    let mut entry = image::Entry::new(port);
    if ["si_filenameCleanup", "SI_omodelBinaryRead", "SI_modelSetState"].contains(&name) {
        entry.prepare = image::prepare_read;
    }
    if name == "SI_edElementChangeName" {
        entry.prepare = image::prepare_edname;
    }
    Some(entry)
}

/// The blocks inside the buffers of a case of the older vectors (`emu_gen.py`) that a
/// dictionary routine may free: each chapter's element array, each side's three relation
/// lists, each list's three entries, each name record and each string. (A block that
/// starts a buffer is the buffer itself.)
fn dict_blocks(c: &Case, names: &si_mem::Names) -> Vec<(u32, u32)> {
    let base = |name: &str| names.iter().find(|b| b.0 == name).map(|b| b.1).filter(|&a| a != 0);
    let mut out = Vec::new();
    if let Some(a) = base("elems") {
        let mut at = 0;
        for (_, cap) in SLOTS {
            out.push((a + at as u32 * 0x84, cap as u32 * 0x84));
            at += cap;
        }
    }
    if let Some(a) = base("groups") {
        out.extend((0..20).map(|k| (a + k * 0x48, 0x48)));
    }
    if let Some(a) = base("rels") {
        out.extend((0..60).map(|k| (a + k * 0x60, 0x60)));
    }
    if let Some(a) = base("nodes") {
        out.extend((0..10).map(|k| (a + k * 0x34, 0x34)));
    }
    if let Some(a) = base("strs") {
        let bytes = c.input("strs").to_bytes();
        let mut start = 0;
        for (i, b) in bytes.iter().enumerate() {
            if *b == 0 {
                out.push((a + start as u32, (i + 1 - start) as u32));
                start = i + 1;
            }
        }
    }
    let starts: Vec<u32> = ["elems", "groups", "rels", "nodes", "strs"].iter().filter_map(|n| base(n)).collect();
    out.retain(|b| !starts.contains(&b.0));
    out
}

/// The update list as the generator of the older vectors read it from memory after
/// `SI_elemdictTopoSort` (`upd_post` of `emu_gen.py`): the count, eight words a step,
/// six an argument.
fn upd_list_as_dumped(c: &Case, m: &si_mem::Mem, names: &si_mem::Names) -> Result<(), String> {
    let ed = names.iter().find(|b| b.0 == "ed").map(|b| b.1).ok_or("no buffer `ed`")?;
    let (nb, nodes) = (m.u32(ed + 0xc), m.u32(ed + 0x10));
    let (mut steps, mut args) = (Vec::new(), Vec::new());
    for i in 0..nb.min(64) {
        let n = nodes + i * 0x2c;
        let nargs = m.u16(n + 0x18) as u32;
        steps.extend([m.u16(n) as u32, m.u16(n + 2) as u32, m.u32(n + 4), m.u32(n + 8), m.u32(n + 0x14), nargs, m.u16(n + 0x20) as u32, m.u32(n + 0x24)]);
        let argp = m.u32(n + 0x1c);
        for k in 0..nargs.min(16) {
            let a = argp + k * 0x14;
            args.extend([m.u16(a) as u32, m.u16(a + 2) as u32, m.u32(a + 4), m.u32(a + 8), m.u16(a + 0xc) as u32, m.u32(a + 0x10)]);
        }
    }
    for (name, got) in [("updn", vec![nb]), ("upd", steps), ("args", args)] {
        let want: Vec<u32> = c.try_output(name).map_or(Vec::new(), |f| f.u32s());
        if want != got {
            let at = want.iter().zip(&got).position(|(a, b)| a != b);
            return Err(format!("{name}: {} words in the original, {} in the port, first difference at {at:?}", want.len(), got.len()));
        }
    }
    Ok(())
}

/// The dictionary routines whose only port is `si-dict`'s, on the older vectors of
/// `re/tools/si-scene/emu_gen.py`: each with the slots of the generator's spec line (a
/// name by value, `name*` a buffer, `name$` a string, `name:N` an output buffer of N
/// bytes, `name@ADDRESS` a global) and whether the line asked for the heap.
fn dict_entry(name: &str) -> Option<image::Entry> {
    use si_dict::{elemdict as d, elemmisc as x, elemname as n, elemrel as r, toposort as t};
    macro_rules! old {
        ($head:literal, $heap:expr, $port:expr) => {{
            let port: image::Port = $port;
            image::Entry::old(port, concat!($head, " ed* lists* elems* groups* rels* nodes* strs* cid@0x08781660 rd@0x087815f0 rules*"), $heap, dict_blocks)
        }};
    }
    Some(match name {
        // elemdict.c: relations added and removed, elements deleted
        "SI_elemdictAddSerialRelation" => old!("edp rdp sc dc sg dg src dst relid serial rule limit data flag chk", true, |m, _, a| {
            r::add_serial_relation(m, a.p("edp"), a.p("rdp"), a.i("sc"), a.i("dc"), a.i("sg"), a.i("dg"), a.i("src"), a.i("dst"), a.i("relid"), a.i("serial"), a.i("rule"), a.i("limit"), a.p("data"), a.s("flag"), a.s("chk")).into()
        }),
        "SI_elemdictRemoveRelationPair" => old!("edp rdp src dst relid rule p7", false, |m, o, a| {
            r::remove_relation_pair(m, o, a.p("edp"), a.p("rdp"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("p7")).into()
        }),
        "SI_elemdictDeleteChapterElems" => old!("edp chap root", false, |m, o, a| d::delete_chapter_elems(m, o, a.p("edp"), a.i("chap"), a.i("root")).into()),
        "SI_elemdictRemoveAllInvalidElems" => old!("edp", false, |m, o, a| x::remove_all_invalid_elems(m, o, a.p("edp")).into()),
        // elemdict.c: look-ups by name
        "SI_elemdictFindElemByName" => old!("edp chap full$ alias$", false, |m, _, a| d::find_elem_by_name(m, a.p("edp"), a.i("chap"), a.p("full"), a.p("alias")).into()),
        "SI_elemdictFindElemByPrjName" => old!("edp chap skip prefix$ name$ p6 root new", false, |m, _, a| {
            n::find_elem_by_prj_name(m, a.p("edp"), a.i("chap"), a.p("skip"), a.p("prefix"), a.p("name"), a.p("p6"), a.i("root"), a.s("new")).into()
        }),
        "SI_elemdictFindElemByNameOnly" => old!("edp chap skip name$ root new", false, |m, _, a| {
            n::find_elem_by_name_only(m, a.p("edp"), a.i("chap"), a.p("skip"), a.p("name"), a.i("root"), a.s("new")).into()
        }),
        "SI_elemdictCheckDuplElemName" => old!("edp chap e", false, |m, _, a| n::check_dupl_elem_name(m, a.p("edp"), a.i("chap"), a.p("e")).into()),
        "SI_elemdictFindElemByFullName" => old!("edp chap skip full$ new", false, |m, _, a| {
            n::find_elem_by_full_name(m, a.p("edp"), a.i("chap"), a.p("skip"), a.p("full"), a.s("new")).into()
        }),
        // elemdict.c: relations followed and counted
        "SI_elemdictRangeRelationsCount" => old!("edp e fwd group lo hi chap", false, |m, _, a| {
            r::range_relations_count(m, a.p("edp"), a.p("e"), a.s("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap")).into()
        }),
        "SI_elemdictFindRelatedElem2" => old!("edp e fwd group og relid chap oc:4 oi:4", false, |m, _, a| {
            r::find_related_elem2(m, a.p("edp"), a.p("e"), a.s("fwd"), a.i("group"), a.i("og"), a.i("relid"), a.i("chap"), a.p("oc"), a.p("oi")).into()
        }),
        "SI_elemdictFindRangeNextRelatedElem2" => old!("edp e fwd group og lo hi chap oc:4 oi:4", false, |m, _, a| {
            r::find_range_next_related_elem2(m, a.p("edp"), a.p("e"), a.s("fwd"), a.i("group"), a.i("og"), a.i("lo"), a.i("hi"), a.i("chap"), a.p("oc"), a.p("oi")).into()
        }),
        "SI_elemdictFindRelatedElemSerial" => old!("edp e fwd group relid chap oc:4 oi:4 serial", false, |m, _, a| {
            r::find_related_elem_serial(m, a.p("edp"), a.p("e"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("oc"), a.p("oi"), a.i("serial")).into()
        }),
        "SI_elemdictFindNextRelatedElemSerial" => old!("edp e fwd group relid chap oc:4 oi:4 serial", false, |m, _, a| {
            r::find_next_related_elem_serial(m, a.p("edp"), a.p("e"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("oc"), a.p("oi"), a.i("serial")).into()
        }),
        "SI_elemdictResetCurrFlag" => old!("e relid", false, |m, _, a| r::reset_curr_flag(m, a.p("e"), a.i("relid")).into()),
        "SI_elemdictSetCurrRelPointer" => old!("edp e relid og:4", false, |m, _, a| r::set_curr_rel_pointer(m, a.p("edp"), a.p("e"), a.i("relid"), a.p("og")).into()),
        "SI_elemdictSerialRelCount" => old!("edp e relid which at:4", false, |m, _, a| r::serial_rel_count(m, a.p("edp"), a.p("e"), a.i("relid"), a.p("which"), a.p("at")).into()),
        "SI_elemdictGetRelationFromIds" => old!("edp e relid chap index serial", false, |m, _, a| {
            r::get_relation_from_ids(m, a.p("edp"), a.p("e"), a.i("relid"), a.i("chap"), a.i("index"), a.i("serial")).into()
        }),
        "SI_elemdictFindSourceRelation" => old!("edp e chap", false, |m, _, a| r::find_source_relation(m, a.p("edp"), a.p("e"), a.i("chap")).into()),
        "SI_elemdictSetRelToSerialid" => old!("edp e relid serial fwd", false, |m, _, a| {
            r::set_rel_to_serialid(m, a.p("edp"), a.p("e"), a.i("relid"), a.i("serial"), a.s("fwd")).into()
        }),
        // the element comparison and its two helpers of text.c (`si-dict`'s), on the older
        // vectors of emu_gen.py
        "elemdictCompareElems" => image::Entry::old(
            |m, _, a| si_dict::elemsort::compare_elems(m, a.p("a"), a.p("b")).into(),
            "a* b* ed* lists* elems* groups* rels* nodes* strs* glbEL@0x08787cf8 glbHS@0x08787cfc glbUp@0x08787cfe glbType@0x08787d00",
            false,
            dict_blocks,
        ),
        "SI_strGetSeqNum" => image::Entry::old(|m, _, a| si_dict::elemsort::str_get_seq_num(m, a.p("s")).into(), "s$", false, |_, _| Vec::new()),
        "SI_strStripDigit" => image::Entry::old(|m, _, a| si_dict::elemsort::str_strip_digit(m, a.p("s")).into(), "s*", false, |_, _| Vec::new()),
        // dbvsfcv.c and attdict.c: two routines of the older vectors of emu_gen_save.py
        "SI_relGetAnimFRelFromChapter" => image::Entry::old(|m, _, a| si_scene::dbvsfcv::rel_get_anim_f_rel_from_chapter(m, a.i("chap")).into(), "chap cid@0x08781660", false, |_, _| Vec::new()),
        "getOldSubdef" => image::Entry::old(|m, _, a| si_dict::attdict::get_old_subdef(m, a.i("id"), a.p("out")).into(), "id out:4", false, |_, _| Vec::new()),
        // toposort.c and the animation chapter's order
        "elemdictFindFather" => old!("e", false, |m, _, a| t::find_father(m, a.p("e")).into()),
        "SI_elemdictSortHierElements" => old!("edp chap", true, |m, _, a| t::sort_hier_elements(m, a.p("edp"), a.i("chap")).into()),
        "SI_elemdictSortAnimElements" => old!("l", true, |m, o, a| x::sort_anim_elements(m, o, a.p("l")).into()),
        "SI_elemdictTopoSort" => {
            let mut e = old!("rdp edp upd p4 p5", false, |m, o, a| t::topo_sort_dict(m, o, a.p("rdp"), a.p("edp"), a.p("upd"), a.s("p4"), a.s("p5")).into());
            e.post = Some(upd_list_as_dumped);
            e
        }
        _ => return None,
    })
}

/// One case. A routine may have vectors of both conventions (the older ones name their
/// fields plainly, the newer ones `v_x`, `p_x`, `gADDRESS`), and of the older ones some
/// are still compared through the stores: the case's own fields say which reads it.
fn dispatch(c: &Case) -> Verdict {
    let routine = c.name.split('#').next().unwrap_or("");
    let newer = c.inputs.is_empty() || c.inputs.iter().any(|f| f.name.starts_with("v_") || f.name.starts_with("p_"));
    let entry = if newer { image_entry(routine) } else { dict_entry(routine) };
    match entry {
        Some(e) => image::check(c, &e),
        None => match on_stores(&c.name) {
            Some(check) => check(c),
            None => Verdict::Skip,
        },
    }
}

fn lookup(name: &str) -> Option<Check> {
    let routine = name.split('#').next().unwrap_or("");
    (image_entry(routine).is_some() || dict_entry(routine).is_some() || on_stores(name).is_some()).then_some(dispatch as Check)
}

/// The routines still compared through the stores: the saver's, and the copies of
/// `si-dict` functions kept for callers that are not on the image.
fn on_stores(name: &str) -> Option<Check> {
    Some(match name {
        "SI_elemdictRemoveUncompleteRelPair" => |c| more(c, 1),
        "SI_elemdictFindRangeNextRelatedElem" => |c| more(c, 3),
        "SI_elemdictRelationCheck" => |c| more(c, 6),
        "SI_elemdictFindSerialRelation" => |c| more(c, 7),
        "SI_elemdictGetNthRelatedElem" => |c| more(c, 9),
        "SI_elemdictRelationsCount" => |c| rel_query(c, 0),
        "SI_elemdictFindRangeRelatedElem" => |c| rel_query(c, 1),
        "SI_elemdictFindRelatedElem" => |c| find_related(c, true),
        "SI_elemdictFindNextRelatedElem" => |c| find_related(c, false),
        "SI_elemdictSetDeleteElem" => set_delete,
        "SI_relRemoveRelations" => remove_relations,
        "si_relRemoveARelation" => remove_a_relation,
        "SI_elemdictAddCompleteRelation" => |c| add(c, 0),
        "SI_elemdictAddUnCompleteRelation" => |c| add(c, 1),
        _ => return save::lookup(name),
    })
}

fn main() {
    // the wave icon tables are read at run time from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("scene-emucheck");
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: scene-emucheck <dir of .vec files, or of one directory of them per routine>");
        std::process::exit(2)
    });
    // one directory of vectors is read into memory at a time
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    if dirs.is_empty() {
        dirs.push(Path::new(&dir).to_path_buf());
    }
    let mut rows = Vec::new();
    for d in dirs {
        match check_dir(&d, &lookup) {
            Ok(r) => rows.extend(r),
            Err(e) => {
                eprintln!("{}: {e}", d.display());
                std::process::exit(2)
            }
        }
    }
    std::process::exit(if print_report(&rows) { 0 } else { 1 });
}
