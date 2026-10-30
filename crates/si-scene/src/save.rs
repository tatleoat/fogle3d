// SPDX-License-Identifier: MIT
//! "Save Scene": the scene description written from the element dictionary
//! (`dictout_scn.c`, `dbloadsave.c`).
//!
//! `si-ascii` has the writer's words, number formats and layout, working on its own
//! structure and keeping the order and the numbers it is given. What it leaves to the
//! dictionary is done here ([`build_description`]): the elements of each chapter are sorted
//! and numbered, the relation lines are taken from the relation lists with the
//! sub-definition number the relation dictionary gives, and the display state from the
//! elements.

use crate::calls::{Arg, Unported};
use crate::dict::ElemId;
use crate::elemsort::{self, key, NameOrder};
use crate::load::Scene;
use crate::object::Object;
use si_ascii::cnum::CLib;
use si_ascii::dsc::{self, Chapter, Description, Element, EnvBlock, Recorded, Relation, RelationBlock, SubRelation, CUSTOM_BLOCKS_AT, ENVIRONMENT_ORDER, RELATION_ORDER};
use si_ascii::reldata::{self, RelData};
use si_types::dict::ELEM_VALID;
use std::collections::BTreeMap;

/// Whether an element is left out of a save: `si_elemCheckRestrictedSave(dictionary, chap,
/// element, 1)`, as `dbWriteChapterElements`, `dbWriteChapterRelations` and
/// `dbWriteEnvironment` call it: [`crate::dictout::elem_check_restricted_save`] on the
/// scene's image, through its adapter [`crate::dictout::check_restricted_save`]. With the setup's `SAVE_MAT_TXT_NOT_SEPARATELY` on
/// (`si_saveOptNoMatTxt`, [`crate::dictout::SaveState::no_mat_txt`]) materials and textures
/// that belong to one model only stay inside the model files, and the description leaves
/// them out of all three sections.
fn restricted(sc: &mut Scene, chap: i32, index: i32, un: &mut dyn Unported) -> bool {
    crate::dictout::check_restricted_save(sc, chap, Some(ElemId::new(chap, index)), true, un)
}

/// Whether an element takes part in the save: in use, and with `selected_only` the root of
/// its hierarchy selected (`selected`, +0x42, of the element at its root index).
fn takes_part(sc: &mut Scene, chap: i32, index: i32, selected_only: bool, un: &mut dyn Unported) -> bool {
    let l = &sc.dict.ed.lists[chap as usize];
    let Some(e) = l.elems.get(index as usize) else { return false };
    if e.rec.status != ELEM_VALID {
        return false;
    }
    if selected_only && l.elems.get(e.rec.root_id as usize).is_none_or(|r| r.rec.selected == 0) {
        return false;
    }
    !restricted(sc, chap, index, un)
}

/// The elements of one chapter as the ELEMENTS section lists them
/// [soft 0x0812f1bc dbWriteChapterElements], on the dictionary: the chapter is sorted by
/// full name ([`elemsort::sort_chapter_elements`], ascending, not hierarchical; the two
/// picture chapters by name and frame number), and the elements that are written are
/// numbered from 0 in that order. **The number is stored in the element** (`work_id`,
/// +0x5c): the relation and environment sections print it.
///
/// Written: elements in use that are roots (depth 0), and in MODELS, LIGHTS, CAMERAS and
/// EXPRESSIONS the others too. Nothing is written for the SCENES chapter. In the PICTURES
/// chapter the name printed is the whole path kept in the name record.
///
/// (With piped files the original takes the elements in index order instead of sorted
/// order; that mode is not reproduced.)
pub fn number_chapter_elements(sc: &mut Scene, chap: i32, names: NameOrder, selected_only: bool, un: &mut dyn Unported) -> Vec<Element> {
    let cid = sc.program.cid;
    if chap == cid.dsc {
        return Vec::new();
    }
    let rootable = [cid.model, cid.lig, cid.cam, cid.exp].contains(&chap);
    let Some(order) = elemsort::sort_chapter_elements(&sc.dict, chap, key::FULL_NAME, true, false, names, (cid.rim, cid.img), None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut n = 0;
    for index in order {
        let e = sc.dict.ed.lists[chap as usize].elems[index as usize].rec;
        if !((e.hier_level == 0 || rootable) && takes_part(sc, chap, index, selected_only, un)) {
            continue;
        }
        sc.dict.ed.lists[chap as usize].elems[index as usize].rec.work_id = n;
        let node = sc.dict.node_of(chap, index);
        let name = if chap == cid.img { node.and_then(|x| x.sub_path.clone()) } else { node.and_then(|x| x.full.clone()) };
        out.push(element_of(sc, chap, index, name.unwrap_or_else(|| si_db::NULL_TEXT.to_vec()), n));
        n += 1;
    }
    out
}

/// The display state of an element in the form `si-ascii` prints it (`dbWriteElementSchematics`,
/// `SI_dbWriteElementWireColor`, `SI_dbWriteElementDisplay`, `dbWriteRootModelSRT`,
/// `SI_dbWriteElementFlags`: ported there).
///
/// `MPRFLG` is the flag word of the element's node in the scene graph and is printed only
/// for an element that has a node (`Scene::graf`). `SRT` is printed for a root model whose
/// pointer at +0x3c is set; after a load the numbers are with the model (the loader hands
/// them over and frees them) and something outside the dictionary code sets the pointer
/// again before a save. Models are not ported: the numbers handed over stand in
/// (`Scene::root_state`).
fn element_of(sc: &Scene, chap: i32, index: i32, name: Vec<u8>, number: i32) -> Element {
    let e = &sc.dict.ed.lists[chap as usize].elems[index as usize].rec;
    let id = ElemId::new(chap, index);
    let wire = e.wire_color as i16;
    Element {
        name,
        alias: None,
        not_root: e.hier_level,
        work_id: number,
        schem_x: e.schem_x,
        schem_y: e.schem_y,
        collapsed: e.schem_collapse,
        usr: e.schem_flags & 1 != 0,
        wire: if e.wire_color == sc.wire.default_wire() { (0, 7) } else { (sc.wire.wire_to_index(wire, true), sc.wire.wire_to_index(wire, false)) },
        selected: e.selection,
        line_type: e.wire_type,
        srt: sc.srt.get(e.root_srt.0).copied().or_else(|| sc.root_state.get(&id).copied()),
        mprflg: sc.graf.get(e.aux_data.0).map(|n| n.flags as i16 as f32),
    }
}

/// The lines of one block of the RELATIONS section [soft 0x0812f7b4 dbWriteChapterRelations],
/// taken from the dictionary: for each element of the first chapter that takes part, in
/// **index order** (`order` replaces it for the checks), for each of its forward lists in
/// turn, for each entry whose other end is in the second chapter and takes part.
///
/// Not every relation is written in every block: an expression (20010) only towards
/// EXPRESSIONS, 246 and 247 only towards MODELS, and towards ANIMATION only the relations
/// 15000 to 15010 and the named selection (10000).
///
/// The numbers printed are the elements' numbers from the ELEMENTS section; for a named
/// selection towards ANIMATION the second number is that of the root of the curve's
/// hierarchy. The sub-definition number is the rule of the relation that fits the two
/// chapters and the two lists (`si_reldictGetRelSub`); for a custom effect it is a running
/// count per list when the first element is an effect's icon (a model whose algorithm is
/// 13), else the second value of the relation's data, else 1 with a complaint on standard
/// error.
fn chapter_relations(sc: &mut Scene, chap1: i32, chap2: i32, selected_only: bool, order: Option<&[i32]>, un: &mut dyn Unported) -> Vec<Relation> {
    let cid = sc.program.cid;
    let mut out = Vec::new();
    let n = sc.dict.ed.lists[chap1 as usize].elems.len() as i32;
    let indices: Vec<i32> = match order {
        Some(o) => o.to_vec(),
        None => (0..n).collect(),
    };
    for i in indices {
        if !takes_part(sc, chap1, i, selected_only, un) {
            continue;
        }
        let e1 = sc.dict.ed.lists[chap1 as usize].elems[i as usize].rec;
        let is_icon = chap1 == cid.model
            && match sc.objects.get(e1.data.0) {
                Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).and_then(|t| t.node(*node)).is_some_and(|n| n.algorithm == 0xd),
                _ => false,
            };
        let groups = sc.dict.ed.lists[chap1 as usize].elems[i as usize].forward.len();
        for g in 0..groups {
            let mut running = 1;
            let rels = sc.dict.ed.lists[chap1 as usize].elems[i as usize].forward[g].rels.clone();
            for (k, r) in rels.iter().enumerate() {
                if (r.relid == reldata::REL_EXP && chap2 != cid.exp) || ((r.relid == 0xf6 || r.relid == 0xf7) && chap2 != cid.model) {
                    continue;
                }
                if chap2 == cid.ani && !(15000..=15010).contains(&r.relid) && r.relid != 10000 {
                    continue;
                }
                if r.chapid as i32 != chap2 || !takes_part(sc, chap2, r.elemid, selected_only, un) {
                    continue;
                }
                let other = sc.dict.ed.lists[chap2 as usize].elems[r.elemid as usize].rec;
                let elem2 = if r.relid == 10000 && r.chapid as i32 == cid.ani {
                    sc.dict.ed.lists[chap2 as usize].elems.get(other.root_id as usize).map_or(0, |x| x.rec.work_id)
                } else {
                    other.work_id
                };
                let mut data = sc.rel_data.get(r.data.0).cloned();
                let sub = if r.relid == reldata::REL_CUSTOM {
                    if is_icon {
                        running += 1;
                        running - 1
                    } else {
                        match &data {
                            Some(RelData::Custom(c)) => c.value_0c,
                            _ => {
                                // "Relation derived from custom ope…" on standard error
                                un.call("stderr: relation derived from a custom operator has no data", &[Arg::Int(elem2 as i64)]);
                                1
                            }
                        }
                    }
                } else {
                    sc.program.rd.get_rel_sub(r.relid, chap1, r.chapid as i32, g as i32, r.group as i32).0
                };
                if sub == 1 || r.relid == reldata::REL_CUSTOM {
                    match &mut data {
                        Some(RelData::Custom(c)) => {
                            // SI_cusRelDataSetDscId: the data names the effect's icon by its
                            // number in the file being written
                            if let Some(icon) = sc.dict.ed.get_elem(cid.model, c.icon_elem) {
                                c.icon_id = Some(icon.rec.work_id.to_string().into_bytes());
                            }
                        }
                        Some(_) => {}
                        None => {
                            // SI_elemdictStoreRelationData would build the data from the
                            // scene's objects here (fitting, dynamics, constraints: not
                            // ported); nothing is written when it has none
                            un.call("SI_elemdictStoreRelationData", &[Arg::Chap(chap1), Arg::Int(i as i64), Arg::Int(r.relid as i64)]);
                        }
                    }
                } else {
                    data = None;
                }
                let mut subrels = None;
                if (r.relid == reldata::REL_EXP && chap2 == cid.exp) || ((r.relid == 0xf6 || r.relid == 0xf7) && chap2 == cid.model) {
                    // dbWriteSubRelations (0x0812f5c4): the entries that follow in the
                    // list with the same relation id and serial number
                    let mut subs = Vec::new();
                    for s in rels[k + 1..].iter().take_while(|s| s.relid == r.relid && s.serial == r.serial) {
                        let number = sc.dict.ed.lists.get(s.chapid as usize).and_then(|l| l.elems.get(s.elemid as usize)).map_or(0, |x| x.rec.work_id);
                        let sdata = sc.rel_data.get(s.data.0).cloned();
                        subs.push(SubRelation { chapter: s.chapid as i32, elem: number, own_data: sdata.is_some(), data: sdata });
                    }
                    subrels = Some(subs);
                } else if r.relid == 10000 && r.chapid as i32 == cid.ani {
                    // SI_nselWriteSpecialRelationData (not ported: it reads the scene); the
                    // values read are printed, with the element's number as it is now
                    if let Some(RelData::NamedSelection(n)) = &mut data {
                        if let Some(x) = sc.dict.ed.lists.get(n.chapter as usize).and_then(|l| l.elems.get(n.elem as usize)) {
                            n.elem_id = x.rec.work_id.to_string().into_bytes();
                        }
                    }
                }
                out.push(Relation {
                    elem1: e1.work_id,
                    elem2,
                    relid: r.relid,
                    serial: r.serial,
                    subdef: sub,
                    sub_index: 0,
                    active: r.flag != 0,
                    data,
                    subrels,
                    recorded: Recorded::Plain,
                });
            }
        }
    }
    out
}

/// The chapter pairs the writer adds between its fixed blocks for custom effects
/// [soft 0x0812fe78 saveCustomRelations]: for each of LIGHTS, MATERIALS, TEXTURES2D,
/// TEXTURES3D and CAMERAS from which some relation arrives at an effect's icon (a model
/// with an object whose algorithm is 13), the pairs from that chapter. The five chapters
/// are gone through from the last to the first.
fn custom_pairs(sc: &Scene) -> Vec<(i32, i32)> {
    let cid = sc.program.cid;
    let chapters = [cid.lig, cid.mat, cid.t2d, cid.t3d, cid.cam];
    let mut hit = [false; 5];
    for e in &sc.dict.ed.lists[cid.model as usize].elems {
        let icon = match sc.objects.get(e.rec.data.0) {
            Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).and_then(|t| t.node(*node)).is_some_and(|n| n.algorithm == 0xd),
            _ => false,
        };
        if !icon {
            continue;
        }
        for r in e.backward.iter().flat_map(|l| l.rels.iter()) {
            if let Some(k) = chapters.iter().position(|&c| c == r.chapid as i32) {
                hit[k] = true;
            }
        }
    }
    let mut out = Vec::new();
    for k in (0..5).rev() {
        if !hit[k] {
            continue;
        }
        let c = chapters[k];
        if c == cid.cam || c == cid.lig {
            out.extend([(c, cid.mat), (c, cid.t2d), (c, cid.t3d)]);
        } else if c == cid.t2d || c == cid.t3d {
            out.extend([(c, cid.model), (c, cid.mat), (c, cid.t2d), (c, cid.t3d), (c, cid.cam), (c, cid.lig)]);
        } else {
            out.extend([(c, cid.model), (c, cid.lig), (c, cid.cam)]);
        }
    }
    out
}

/// The description of the scene as the writer prints it [soft 0x08130460
/// SI_dbWriteDescriptionFile], built from the dictionary:
///
/// 1. ELEMENTS: every chapter numbered ([`number_chapter_elements`]); `si-ascii` prints the
///    chapters sorted by name.
/// 2. RELATIONS: the 84 fixed chapter pairs in the writer's order, the custom effect pairs
///    after the 62nd ([`custom_pairs`]); a pair with no line is left out.
/// 3. ENVIRONMENT: twenty chapters in a fixed order, one line per element that takes part,
///    in index order (in ANIMATION roots only); then the play control.
///
/// `order`, for the checks only, gives for a chapter the order in which its elements are
/// gone through instead of index order.
///
/// Afterwards the original clears the scene element's new-flag, dirty and selected words
/// and the selected word of every element; so does this.
pub fn build_description(sc: &mut Scene, names: NameOrder, selected_only: bool, order: Option<&BTreeMap<i32, Vec<i32>>>, un: &mut dyn Unported) -> Description {
    let cid = sc.program.cid;
    let nb = sc.dict.ed.lists.len() as i32;
    let mut chapters = Vec::new();
    for chap in 0..nb {
        let elements = number_chapter_elements(sc, chap, names, selected_only, un);
        chapters.push(Chapter { name: sc.dict.ed.lists[chap as usize].name.clone(), elements });
    }
    let find = |sc: &Scene, name: &str| sc.dict.ed.find_list_by_name(name.as_bytes());
    let mut relations = Vec::new();
    let block = |sc: &mut Scene, c1: i32, c2: i32, relations: &mut Vec<RelationBlock>, un: &mut dyn Unported| {
        if c1 < 0 || c2 < 0 {
            return;
        }
        let lines = chapter_relations(sc, c1, c2, selected_only, order.and_then(|o| o.get(&c1)).map(|v| v.as_slice()), un);
        if !lines.is_empty() {
            relations.push(RelationBlock {
                list1: c1,
                list2: c2,
                name1: sc.dict.ed.lists[c1 as usize].name.clone(),
                name2: sc.dict.ed.lists[c2 as usize].name.clone(),
                relations: lines,
            });
        }
    };
    for (k, (a, b)) in RELATION_ORDER.iter().enumerate() {
        if k == CUSTOM_BLOCKS_AT {
            for (c1, c2) in custom_pairs(sc) {
                block(sc, c1, c2, &mut relations, un);
            }
        }
        let (c1, c2) = (find(sc, a), find(sc, b));
        block(sc, c1, c2, &mut relations, un);
    }
    let mut environment = Vec::new();
    for name in ENVIRONMENT_ORDER {
        let chap = find(sc, name);
        if chap < 0 {
            continue;
        }
        let n = sc.dict.ed.lists[chap as usize].elems.len() as i32;
        let indices: Vec<i32> = match order.and_then(|o| o.get(&chap)) {
            Some(o) => o.clone(),
            None => (0..n).collect(),
        };
        // the environment block prints one line per element with the element's values; the
        // Description keeps the values in the chapter's elements, so the elements of this
        // chapter that have a line are appended to the chapter and the block lists them
        let mut elems = Vec::new();
        for i in indices {
            if !takes_part(sc, chap, i, selected_only, un) {
                continue;
            }
            let e = sc.dict.ed.lists[chap as usize].elems[i as usize].rec;
            if chap == cid.ani && e.hier_level != 0 {
                continue;
            }
            // dbWriteRootModelSRT frees the nine numbers once they are printed
            let srt_printed = chap == cid.model && e.hier_level == 0 && !e.root_srt.is_null();
            let c: &mut Chapter = &mut chapters[chap as usize];
            // an element that the ELEMENTS section lists is found by its number; any other
            // (an element that is not a root in a chapter that lists roots only) gets an
            // entry of its own, which the ELEMENTS writer passes over
            let listed = c.elements.iter().position(|x| x.work_id == e.work_id && (e.hier_level == 0) == (x.not_root == 0));
            match listed {
                Some(p) => elems.push(p),
                None => {
                    let mut x = element_of(sc, chap, i, Vec::new(), e.work_id);
                    x.not_root = 1;
                    c.elements.push(x);
                    elems.push(c.elements.len() - 1);
                }
            }
            if srt_printed {
                if let Some(v) = sc.srt.free(e.root_srt.0) {
                    // (kept for the checks that write a description several times)
                    sc.root_state.insert(ElemId::new(chap, i), v);
                }
                sc.dict.ed.lists[chap as usize].elems[i as usize].rec.root_srt = si_types::Ptr32::NULL;
            }
        }
        if !elems.is_empty() {
            environment.push(EnvBlock { list: chap, name: name.as_bytes().to_vec(), elems });
        }
    }
    // the scene's own element and every element's "selected" word
    if let Some(e) = sc.dict.ed.lists[cid.dsc as usize].elems.iter_mut().find(|e| e.rec.status == ELEM_VALID) {
        e.rec.new_flag = 0;
        e.rec.dirty = 0;
    }
    sc.dict.ed.set_selection(0);
    Description { version: sc.dsc_version, chapters, element_order: Vec::new(), relations, environment, play: Some(sc.play.clone()), messages: Vec::new() }
}

/// Which build's habits a written file follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    /// The Linux build (and IRIX): names compared byte by byte, two-digit exponents.
    Linux,
    /// The NT build: names compared without regard to case, three-digit exponents.
    Nt,
    NtNamesLinuxNumbers,
    LinuxNamesNtNumbers,
    /// An older version (the samples of versions 2.61 to 2.65): names in plain `strcmp`
    /// order. A guess from the files, not read from this binary.
    Old,
}

impl Build {
    fn parts(self) -> (NameOrder, CLib) {
        match self {
            Build::Linux => (NameOrder::Linux, CLib::Glibc),
            Build::Nt => (NameOrder::Nt, CLib::Msvc),
            Build::NtNamesLinuxNumbers => (NameOrder::Nt, CLib::Glibc),
            Build::LinuxNamesNtNumbers => (NameOrder::Linux, CLib::Msvc),
            Build::Old => (NameOrder::Plain, CLib::Glibc),
        }
    }
}

/// The scene description as a build writes it, with a given version number in the header
/// (this build writes 4: `SI_versionOutput`).
pub fn write_description(sc: &mut Scene, build: Build, version: f32, un: &mut dyn Unported) -> Vec<u8> {
    let (names, lib) = build.parts();
    build_description(sc, names, false, None, un).write_as(version, lib)
}

/// For looking at a difference: the description written with the elements gone through in
/// the order the file's lines show (as [`check_against`] does), for one build.
pub fn write_replayed(sc: &mut Scene, file: &[u8], build: Build) -> Vec<u8> {
    let mut un = crate::calls::Recorder::default();
    let (names, lib) = build.parts();
    let mut d = build_description(sc, names, false, None, &mut un);
    if let Ok((p, _)) = dsc::read(file) {
        let order = replay_order(sc, &p);
        d = build_description(sc, names, false, Some(&order), &mut un);
    }
    d.write_as(sc.dsc_version, lib)
}

/// What comparing a written description with a file gave.
#[derive(Clone, Debug)]
pub struct SaveCheck {
    /// The build whose output equals the file byte for byte, with the dictionary as the
    /// load left it (elements in the file's order).
    pub same: Option<Build>,
    /// The build whose output equals the file byte for byte once each chapter's elements
    /// are gone through in the order in which the file's own lines name them: the internal
    /// order the dictionary had in the session that saved the file.
    pub same_replayed: Option<Build>,
    /// Every build whose replayed output equals the file (a file without a number that
    /// needs an exponent fits both number formats; one without names that differ only in
    /// case, or in an underscore against a digit, fits several name orders).
    pub fits: Vec<Build>,
    /// The file has no `MPRFLG` item at all: it was written by a version that had none, and
    /// the comparison was made without them.
    pub no_mprflg: bool,
    /// The ELEMENTS section (order and numbering) equals the file's, and under which names
    /// order.
    pub elements_same: Option<NameOrder>,
    /// The file and the output hold the same lines, in another order.
    pub same_lines: bool,
    /// Where the first difference is (of the replayed output of the best-fitting build).
    pub why: String,
}

fn section<'a>(text: &'a [u8], end: &[u8]) -> &'a [u8] {
    match text.windows(end.len()).position(|w| w == end) {
        Some(p) => &text[..p],
        None => text,
    }
}

fn contains(text: &[u8], word: &[u8]) -> bool {
    text.windows(word.len()).any(|w| w == word)
}

/// The order in which a file's lines name the elements of each chapter: first the
/// environment block of the chapter, then, for chapters without one, the first numbers of
/// the relation lines of blocks that start with the chapter. Elements the file's lines do
/// not name follow in index order. Numbers are turned into element indices through the
/// numbering the last [`build_description`] left in the dictionary.
fn replay_order(sc: &Scene, file: &Description) -> BTreeMap<i32, Vec<i32>> {
    let mut by_chapter: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    fn add(by_chapter: &mut BTreeMap<i32, Vec<i32>>, sc: &Scene, name: &[u8], number: i32) {
        let chap = sc.dict.ed.find_list_by_name(name);
        if chap < 0 {
            return;
        }
        let cid = sc.program.cid;
        let rootable = [cid.model, cid.lig, cid.cam, cid.exp].contains(&chap);
        let l = &sc.dict.ed.lists[chap as usize];
        let found = l.elems.iter().position(|e| e.rec.status == ELEM_VALID && e.rec.work_id == number && (rootable || e.rec.hier_level == 0));
        if let Some(i) = found {
            let v = by_chapter.entry(chap).or_default();
            if !v.contains(&(i as i32)) {
                v.push(i as i32);
            }
        }
    }
    for b in &file.environment {
        if let Some(c) = file.chapters.get(b.list as usize) {
            for &i in &b.elems {
                if let Some(e) = c.elements.get(i) {
                    add(&mut by_chapter, sc, &b.name, e.work_id);
                }
            }
        }
    }
    let with_env: Vec<i32> = by_chapter.keys().copied().collect();
    for b in &file.relations {
        let chap = sc.dict.ed.find_list_by_name(&b.name1);
        if with_env.contains(&chap) {
            continue;
        }
        for r in &b.relations {
            add(&mut by_chapter, sc, &b.name1, r.elem1);
        }
    }
    for (chap, v) in by_chapter.iter_mut() {
        for i in 0..sc.dict.ed.lists[*chap as usize].elems.len() as i32 {
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }
    by_chapter
}

/// Writes the loaded scene back and compares with the file it was loaded from (the corpus
/// check of saving). The header's version is the file's own; with the 4 this build writes
/// every first line differs.
pub fn check_against(sc: &mut Scene, file: &[u8]) -> SaveCheck {
    let version = sc.dsc_version;
    let mut un = crate::calls::Recorder::default();
    let with_mprflg = contains(file, b"MPRFLG");
    let mut out = SaveCheck { same: None, same_replayed: None, fits: Vec::new(), no_mprflg: !with_mprflg, elements_same: None, same_lines: false, why: String::new() };
    let parsed = dsc::read(file).ok().map(|(d, _)| d);
    let write = |sc: &mut Scene, build: Build, replay: bool, un: &mut crate::calls::Recorder| -> Vec<u8> {
        let (names, lib) = build.parts();
        // number the elements first: the replayed order is expressed in those numbers
        let mut d = build_description(sc, names, false, None, un);
        if replay {
            if let Some(p) = &parsed {
                let order = replay_order(sc, p);
                d = build_description(sc, names, false, Some(&order), un);
            }
        }
        if !with_mprflg {
            for c in d.chapters.iter_mut() {
                for e in c.elements.iter_mut() {
                    e.mprflg = None;
                }
            }
        }
        d.write_as(version, lib)
    };
    let builds = [Build::Linux, Build::Nt, Build::NtNamesLinuxNumbers, Build::LinuxNamesNtNumbers, Build::Old];
    let mut best: Option<Vec<u8>> = None;
    for build in builds {
        let plain = write(sc, build, false, &mut un);
        if out.elements_same.is_none() && section(&plain, b"EndOfELEMENTS") == section(file, b"EndOfELEMENTS") {
            out.elements_same = Some(build.parts().0);
        }
        if out.same.is_none() && plain == file {
            out.same = Some(build);
        }
        let replayed = write(sc, build, true, &mut un);
        if replayed == file {
            out.fits.push(build);
            if out.same_replayed.is_none() {
                out.same_replayed = Some(build);
            }
        }
        let fits = Some(build.parts().0) == out.elements_same;
        if best.is_none() || (fits && best.as_ref().is_some_and(|b| section(b, b"EndOfELEMENTS") != section(file, b"EndOfELEMENTS"))) {
            best = Some(replayed.clone());
        }
        let lines = |t: &[u8]| {
            let mut v: Vec<Vec<u8>> = t.split(|&c| c == b'\n').map(|l| l.to_vec()).collect();
            v.sort();
            v
        };
        if lines(&replayed) == lines(file) {
            out.same_lines = true;
        }
    }
    if out.same_replayed.is_none() {
        let written = best.unwrap_or_default();
        let (a, b): (Vec<&[u8]>, Vec<&[u8]>) = (written.split(|&c| c == b'\n').collect(), file.split(|&c| c == b'\n').collect());
        let at = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        let show = |v: &Vec<&[u8]>| v.get(at).map_or(String::from("(end)"), |l| String::from_utf8_lossy(l).trim().chars().take(90).collect());
        out.why = format!("line {}: wrote `{}`, file has `{}`", at + 1, show(&a), show(&b));
    }
    out
}
