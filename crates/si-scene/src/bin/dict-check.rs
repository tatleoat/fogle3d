// SPDX-License-Identifier: MIT
//! `dict-check <dir> [--only TEXT]`: the dictionaries on the image (`si-dict`) against the
//! dictionaries on owned stores, over every sample scene.
//!
//! Each scene description under `<dir>` is loaded the way "Get Scene" does it, by this
//! crate, which builds the scene in a memory image. Every read-only question is then asked
//! twice: of `si-dict`'s functions on that image, and of the store-based ports that are
//! left (`si-types`' and the ones this crate keeps for its saver and for `si-rensetup`) on
//! the stores the image is copied into when a load ends. Reported: how many questions of
//! each kind were asked, and every disagreement. The questions whose store form is gone
//! (the look-ups by name, the father, the hierarchical order) are no longer asked here:
//! `scene-emucheck` and `dict-emucheck` hold those against the original. The chapter sort
//! is asked: its store form is kept for the description writer.
//!
//! While the store form of the loader existed the stores could be taken from it instead
//! (the two loaders held against each other question by question, and their stores field
//! for field): finding 0018, section 14, has that run.

use si_db::{DbInfo, ElemLists, FileSystem, Log, RealFs};
use si_dict::globals::SI_RD;
use si_dict::{elemdict, elemmisc, elemrel, relations, reldict, NoOutside};
use si_mem::{field, Mem, Ptr32};
use si_scene::calls::Recorder;
use si_scene::dict::ElemId;
use si_scene::load::{self, Outcome, Scene};
use si_scene::world;
use si_types::dict::{Elem, ElemDict};
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Tally {
    asked: BTreeMap<&'static str, (u64, u64)>,
    examples: Vec<String>,
}

impl Tally {
    fn ask<T: PartialEq + std::fmt::Debug>(&mut self, kind: &'static str, image: T, store: T, what: impl FnOnce() -> String) {
        let e = self.asked.entry(kind).or_default();
        e.0 += 1;
        if image != store {
            e.1 += 1;
            if self.examples.len() < 40 {
                self.examples.push(format!("{kind}: {}: image {image:?}, stores {store:?}", what()));
            }
        }
    }
}

/// The chapter and index of the element at an address of the image. (Not through
/// `SI_elemdictGetElemId`: for a chapter without slots that routine's range test, taken
/// over from the original, accepts every address.)
fn id_of(m: &Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>) -> ElemId {
    for c in 0..m[ed].nb_lists {
        let l = m[ed].lists.add(c);
        let first = m[l].elems.addr();
        let n = m[l].nb_elems as u32;
        if n > 0 && e.addr() >= first && e.addr() < first + n * 0x84 {
            return ElemId::new(c, ((e.addr() - first) / 0x84) as i32);
        }
    }
    ElemId::new(-1, -1)
}

/// Every question, for one scene: `m` and `ed` are the image the loader built, `sc` holds
/// the stores.
fn compare(m: &mut Mem, ed: Ptr32<ElemDict>, sc: &mut Scene, label: &str, t: &mut Tally) {
    let mut o = NoOutside::default();
    let mut d = sc.dict.clone();
    let cid = sc.program.cid;

    // the relation dictionary
    let ids: Vec<i32> = {
        let mut v: Vec<i32> = sc.program.rd.rules.iter().map(|r| r.relid).collect();
        v.extend([0, 1, -1, 99999]);
        v.dedup();
        v
    };
    for &id in &ids {
        t.ask("rule index by relation id", reldict::get_rel_index_from_id(m, SI_RD, id), sc.program.rd.get_rel_index_from_id(id), || format!("{label} relation {id}"));
        if sc.program.rd.get_rel_index_from_id(id) < 0 {
            continue;
        }
        t.ask("number of rules", reldict::get_rel_nb_rules(m, SI_RD, id), sc.program.rd.get_rel_nb_rules(id), || format!("{label} relation {id}"));
        let a = reldict::get_rel_index_from_id(m, SI_RD, id);
        let image = {
            let r = m.read(m[SI_RD].rules.add(a));
            Some((r.src_chap as i32, r.dst_chap as i32, r.src_group as i32, r.dst_group as i32, r.rank as i32, r.fanout as i32))
        };
        let store = sc.program.rd.get_rel_attributes(id, 1).map(|a| (a.src_chap, a.dst_chap, a.src_group, a.dst_group, a.rank, a.fanout));
        t.ask("rule attributes", image, store, || format!("{label} relation {id}"));
        t.ask("relation active", (reldict::is_rel_active(m, SI_RD, id, 0), reldict::is_rel_active(m, SI_RD, id, 1)), (sc.program.rd.is_rel_active(id, false), sc.program.rd.is_rel_active(id, true)), || format!("{label} relation {id}"));
        let name = reldict::get_rel_name(m, SI_RD, id).addr();
        t.ask("relation name", m.cstr(name), sc.program.rd.get_rel_name(id), || format!("{label} relation {id}"));
        t.ask("rank, render, increment, dope", (reldict::get_rel_rank(m, SI_RD, id), reldict::get_rel_render_flag(m, SI_RD, id), reldict::get_rel_incmode_flag(m, SI_RD, id), reldict::get_rel_dope_disp(m, SI_RD, id)), (sc.program.rd.get_rel_rank(id), sc.program.rd.get_rel_render_flag(id), sc.program.rd.get_rel_incmode_flag(id), sc.program.rd.get_rel_dope_disp(id)), || format!("{label} relation {id}"));
    }

    // chapters and elements
    for c in 0..d.ed.lists.len() as i32 {
        let lp = m[ed].lists.add(c);
        let name = d.ed.lists[c as usize].name.clone();
        t.ask("elements in use per chapter", elemdict::elemlist_count_valid_elems(m, lp), d.ed.lists[c as usize].count_valid_elems(), || format!("{label} chapter {c}"));
        t.ask("chapter by name", elemdict::find_list_by_name(m, ed, m[lp].name), d.ed.find_list_by_name(&name), || format!("{label} chapter {c}"));
        let n = d.ed.lists[c as usize].elems.len() as i32;
        for i in -1..=n {
            t.ask("element by index", !elemdict::get_elem(m, ed, c, i).is_null(), d.ed.get_elem(c, i).is_some(), || format!("{label} {c}/{i}"));
        }
        for i in 0..n {
            let id = ElemId::new(c, i);
            let ep = m[lp].elems.add(i);
            let rec = d.ed.lists[c as usize].elems[i as usize].rec;
            // by object
            t.ask("element by object", elemdict::find_elem_by_data(m, &mut o, ed, c, sc.obj_addr(rec.data.0)), si_scene::animrel::find_elem_by_data(sc, c, rec.data.0), || format!("{label} {c}/{i}"));
            t.ask("element by second object", elemdict::find_elem_by_aux_data(m, ed, c, sc.graf_addr(rec.aux_data.0)), d.ed.find_elem_by_aux_data(c, rec.aux_data), || format!("{label} {c}/{i}"));
            t.ask("slot by object", (elemdict::find_elem_by_del_data(m, ed, c, sc.obj_addr(rec.data.0)), elemdict::find_elem_by_del_aux_data(m, ed, c, sc.graf_addr(rec.aux_data.0))), (d.ed.find_elem_by_del_data(c, rec.data), d.ed.find_elem_by_del_aux_data(c, rec.aux_data)), || format!("{label} {c}/{i}"));
            // relations
            let stores = d.ed.lists[c as usize].elems[i as usize].clone();
            if !stores.backward.is_empty() {
                t.ask("element used", (elemmisc::is_elem_used(m, ep, 0), elemmisc::is_elem_used(m, ep, 1)), (stores.is_elem_used(false), stores.is_elem_used(true)), || format!("{label} {c}/{i}"));
            }
            for (forward, lists) in [(true, &stores.forward), (false, &stores.backward)] {
                let fwd = forward as i16;
                for (g, l) in lists.iter().enumerate() {
                    let g = g as i32;
                    let mut relids: Vec<i32> = l.rels.iter().map(|r| r.relid).collect();
                    relids.dedup();
                    relids.push(110);
                    relids.sort();
                    relids.dedup();
                    for relid in relids {
                        let side: Ptr32<si_types::dict::Relations> = if forward { field!(ep, Elem, forward) } else { field!(ep, Elem, backward) };
                        t.ask("first entry of a relation", relations::rel_check(m, side, g, relid), si_types::store::rel_check(lists, g, relid), || format!("{label} {c}/{i} list {g} relation {relid}"));
                        // the walk: first, then next until there is none
                        let scratch: Ptr32<i32> = Ptr32::new(m.local(4));
                        let mut image = Vec::new();
                        let mut e = elemrel::find_related_elem(m, ed, ep, fwd, g, relid, -1, scratch, scratch);
                        while !e.is_null() && image.len() < 100_000 {
                            image.push(id_of(m, ed, e));
                            e = elemrel::find_next_related_elem(m, ed, ep, fwd, g, relid, -1, scratch, scratch);
                        }
                        m.release(4);
                        let store = si_scene::elemrel::related_elems(&mut d, id, forward, g, relid, -1);
                        t.ask("walk over related elements", image, store, || format!("{label} {c}/{i} list {g} relation {relid}"));
                        for og in 0..6 {
                            t.ask("count of entries", elemrel::relations_count(m, ed, ep, fwd, g, og, relid, -1), si_scene::elemdict::relations_count(&d, id, forward, g, og, relid, -1), || format!("{label} {c}/{i} list {g} relation {relid}"));
                        }
                    }
                }
            }
        }
    }

    // hierarchy information and the hierarchical order of the MODELS chapter
    elemdict::update_hier_info(m, ed, cid.model, 1, 110);
    d.ed.update_hier_info(cid.model, 1, 110);
    if let Some(l) = d.ed.lists.get(cid.model as usize) {
        let lp = m[ed].lists.add(cid.model);
        let image: Vec<(i32, i32)> = (0..l.elems.len() as i32).map(|i| (m[m[lp].elems.add(i)].root_id, m[m[lp].elems.add(i)].hier_level)).collect();
        let store: Vec<(i32, i32)> = l.elems.iter().map(|e| (e.rec.root_id, e.rec.hier_level)).collect();
        t.ask("root and depth of the models", image, store, || label.to_string());
    }
    // the order of every chapter by full name, ascending and descending, and by name:
    // `SI_elemlistSortChapterElements` on the image against the store form this crate
    // keeps for its description writer (one question a slot)
    for c in 0..m[ed].nb_lists {
        for (key, up) in [(si_dict::elemsort::key::FULL_NAME, 1i16), (si_dict::elemsort::key::FULL_NAME, 0), (si_dict::elemsort::key::NAME, 1)] {
            let lp = m[ed].lists.add(c);
            si_dict::elemsort::sort_chapter_elements(m, &mut o, lp, c, key, up, 0);
            let store = si_scene::elemsort::sort_chapter_elements(&d, c, key, up != 0, false, si_scene::elemsort::NameOrder::Linux, (cid.rim, cid.img), None).unwrap_or_default();
            let sort = m[lp].sort;
            for (k, want) in store.iter().enumerate() {
                let got = if sort.is_null() { -1 } else { m[sort.add(k as i32)] };
                t.ask("position in the sorted chapter", got, *want, || format!("{label} chapter {c} key {key} up {up} position {k}"));
            }
        }
    }
    if let Some(f) = m.fault() {
        t.ask("no fault of the image", format!("{f:?}"), String::new(), || label.to_string());
    } else {
        t.ask("no fault of the image", String::new(), String::new(), || label.to_string());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(1).expect("usage: dict-check <directory> [--only TEXT]"));
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
    let mut files = Vec::new();
    walk(&dir, &mut files);
    let Some(rsrc_path) = files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned() else {
        eprintln!("no Relations.rsrc under {}", dir.display());
        std::process::exit(2);
    };
    let rsrc = std::fs::read(&rsrc_path).expect("Relations.rsrc");
    println!("relations resource file: {}", rsrc_path.display());
    let scenes: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "dsc") && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "SCENES"))
        .filter(|p| only.as_ref().is_none_or(|o| p.to_string_lossy().contains(o.as_str())))
        .collect();
    let fs = RealFs;
    let mut t = Tally::default();
    let (mut descriptions, mut loaded, mut complete, mut elements, mut relation_entries) = (0, 0, 0, 0usize, 0usize);
    let mut panics = Vec::new();
    for path in scenes {
        let data = std::fs::read(path).unwrap_or_default();
        if !data.starts_with(b"SDSC") {
            continue;
        }
        descriptions += 1;
        let db = path.parent().unwrap().parent().unwrap();
        let name = path.file_stem().unwrap().as_bytes().to_vec();
        let label = path.strip_prefix(&dir).unwrap_or(path).display().to_string();
        let mut sc = Scene::start(&rsrc).expect("Relations.rsrc");
        sc.assume = load::Assume { effects_installed: true, model_relations_from_description: true };
        let new_info = || {
            let mut info = DbInfo::new();
            info.name = db.file_name().map(|n| n.as_bytes().to_vec());
            info.path = Some(db.as_os_str().as_bytes().to_vec());
            let mut log = Log::default();
            si_db::database_list_chapters(&fs as &dyn FileSystem, &ElemLists::standard(), &mut info, false, &mut log);
            info
        };
        let mut un = Recorder::default();
        if !load::load_scene(&mut sc, &fs, &mut new_info(), &name, &mut un) {
            continue;
        }
        loaded += 1;
        let whole = sc.report.iter().all(|r| matches!(r.outcome, Outcome::Read { complete: true, .. } | Outcome::Node | Outcome::PartOfRoot | Outcome::NotRead | Outcome::Located { .. }));
        if !whole {
            continue;
        }
        complete += 1;
        elements += sc.dict.ed.lists.iter().map(|l| l.elems.len()).sum::<usize>();
        relation_entries += sc.dict.ed.lists.iter().flat_map(|l| &l.elems).flat_map(|e| &e.forward).map(|l| l.rels.len()).sum::<usize>();
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mut m = std::mem::take(&mut sc.mem);
        let ed = world::current_dict(&m);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare(&mut m, ed, &mut sc, &label, &mut t)));
        std::panic::set_hook(hook);
        if result.is_err() {
            panics.push(label);
        }
    }
    println!("{descriptions} scene descriptions, {loaded} loaded, {complete} loaded completely and compared");
    println!("{elements} element slots and {relation_entries} forward relation entries in the images");
    println!("{:<40} {:>10} {:>10}", "question", "asked", "disagree");
    let (mut asked, mut disagree) = (0, 0);
    for (kind, (a, b)) in &t.asked {
        println!("{kind:<40} {a:>10} {b:>10}");
        asked += a;
        disagree += b;
    }
    println!("{:<40} {asked:>10} {disagree:>10}", "TOTAL");
    for e in &t.examples {
        println!("  {e}");
    }
    println!("panics: {}", panics.len());
    for p in &panics {
        println!("  {p}");
    }
    std::process::exit(if disagree == 0 && panics.is_empty() { 0 } else { 1 });
}
