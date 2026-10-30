// SPDX-License-Identifier: MIT
//! The "Save Scene" routines against the original (vectors of
//! `re/tools/si-scene/emu_gen_save.py`). The dictionary of a case is rebuilt as in the main
//! file; here it becomes a `Scene`, with the chapter ids the case gives `SI_cid`.

use super::*;
use si_db::{Bytes, DbInfo, FileSystem, FileSystemMut, ListNode, Stat};
use si_scene::calls::{Answers, Arg, Unported};
use si_scene::dictout::{self, AnimItem, Host, Steps};
use si_scene::load::Scene;
use si_scene::savescene;

/// The case's dictionary as a scene; `SI_cid` as the case has it.
fn scene(c: &Case) -> Scene {
    let b = build(c);
    let mut sc = Scene::from_parts(b.d, b.p);
    if let Some(f) = c.try_input("cid") {
        set_cid(&mut sc.program.cid, f);
        // (the routines of the saver that are on the image read `SI_cid` there)
        let cid = sc.program.cid;
        sc.mem.write(si_dict::globals::SI_CID, cid);
    }
    sc.program.now = 1_000_000_000;
    sc
}

fn set_cid(cid: &mut si_types::dict::ChapIds, f: &Field) {
    let w: Vec<i32> = f.words().iter().map(|w| val(w) as i32).collect();
    let fields: [&mut i32; 36] = [
        &mut cid.ani, &mut cid.cam, &mut cid.cls, &mut cid.dsc, &mut cid.img, &mut cid.lat, &mut cid.lig, &mut cid.mat, &mut cid.model, &mut cid.nsl,
        &mut cid.pal, &mut cid.rim, &mut cid.shd_mat, &mut cid.shd_t2d, &mut cid.shd_t3d, &mut cid.shd_vol, &mut cid.shd_sha, &mut cid.shd_lig,
        &mut cid.shd_cam, &mut cid.shd_out, &mut cid.shd_env, &mut cid.shp, &mut cid.sts, &mut cid.stc, &mut cid.sfk, &mut cid.sky, &mut cid.t2d,
        &mut cid.t3d, &mut cid.wav, &mut cid.exp, &mut cid.chn_drv, &mut cid.out, &mut cid.spd, &mut cid.audio, &mut cid.shd_mat_photon,
        &mut cid.shd_lig_photon,
    ];
    for (f, v) in fields.into_iter().zip(w) {
        *f = v;
    }
}

/// `EMUCHECK_MUTATE=1`: every value the ports produce is changed before it is compared, so
/// that every compared case must then differ: the comparisons are live.
fn mutating() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

fn back(mut sc: Scene) -> Built {
    if mutating() {
        for l in sc.dict.ed.lists.iter_mut() {
            for e in l.elems.iter_mut() {
                e.rec.dirty ^= 1;
            }
        }
    }
    Built { d: sc.dict, p: sc.program }
}

/// The string a pointer word designates after the routine ran: in the case's strings, in a
/// block the routine allocated, or in another buffer of the case.
fn str_after(c: &Case, w: &Word) -> Option<Vec<u8>> {
    let (name, off) = ref_off(w)?;
    let bytes = match c.try_output(&name) {
        Some(f) => f.to_bytes(),
        None => c.try_input(&name)?.to_bytes(),
    };
    let s = bytes.get(off as usize..)?;
    Some(s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec())
}

/// Compares strings of every name record with the original's: `which` are word positions in
/// the record (2 sub-directory, 3 alias, 4 prefix, 5 name, 6 version, 7 substitution, 9 full).
fn same_nodes(c: &Case, sc: &Scene, which: &[usize]) -> Verdict {
    let nodes = c.after("nodes").words();
    let elems = c.after("elems").words();
    let mut diff = Vec::new();
    for (chap, cap) in SLOTS {
        for i in 0..cap.min(sc.dict.ed.lists[chap as usize].elems.len()) {
            let s = slot(chap, i as i32);
            if is_null(&elems[s * 33]) {
                continue;
            }
            let Some(n) = sc.dict.node_of(chap, i as i32) else {
                diff.push(format!("element {chap}/{i}: the port freed the name record"));
                continue;
            };
            for &k in which {
                let want = str_after(c, &nodes[s * 13 + k]);
                let got = match k {
                    2 => &n.sub_path,
                    3 => &n.s0c,
                    4 => &n.prefix,
                    5 => &n.name,
                    6 => &n.version,
                    7 => &n.subst,
                    _ => &n.full,
                };
                // a word the routine did not write and that was null in the case stays null
                let unwritten_null = !matches!(nodes[s * 13 + k], Word::Ref { .. }) && val(&nodes[s * 13 + k]) == 0;
                let want = if unwritten_null { None } else { want };
                if *got != want {
                    diff.push(format!("element {chap}/{i} string {k}: port {:?}, original {:?}", got.as_deref().map(String::from_utf8_lossy), want.as_deref().map(String::from_utf8_lossy)));
                }
            }
        }
    }
    Verdict::from(diff.is_empty(), || diff.join("; "))
}

/// The element a pointer word designates.
fn elem_of_word(w: &Word) -> Option<ElemId> {
    let (name, off) = ref_off(w)?;
    if name != "elems" {
        return None;
    }
    let off = off as usize / 0x84;
    let mut s = 0;
    for (chap, cap) in SLOTS {
        if off < s + cap {
            return Some(ElemId::new(chap, (off - s) as i32));
        }
        s += cap;
    }
    None
}

fn call_text(c: &Case, w: &Word) -> Vec<u8> {
    str_after(c, w).unwrap_or_default()
}

/// A file system that answers every question the same way, and keeps what was asked.
struct FixedFs {
    exists: bool,
    writable: bool,
    asked: std::cell::RefCell<Vec<Bytes>>,
    commands: Vec<Bytes>,
}

impl FixedFs {
    fn new(exists: bool, writable: bool) -> FixedFs {
        FixedFs { exists, writable, asked: Default::default(), commands: Vec::new() }
    }
}

impl FileSystem for FixedFs {
    fn read_dir(&self, _path: &[u8]) -> Option<Vec<Bytes>> {
        None
    }
    fn stat(&self, _path: &[u8]) -> Option<Stat> {
        None
    }
    fn access(&self, path: &[u8], mode: i32) -> bool {
        if mode & si_db::W_OK != 0 {
            return self.writable;
        }
        self.asked.borrow_mut().push(path.to_vec());
        self.exists
    }
    fn read(&self, _path: &[u8]) -> Option<Bytes> {
        None
    }
}

impl FileSystemMut for FixedFs {
    fn mkdir(&mut self, _path: &[u8]) -> bool {
        false
    }
    fn write(&mut self, _path: &[u8], _data: &[u8], _append: bool) -> bool {
        true
    }
    fn unlink(&mut self, _path: &[u8]) -> bool {
        true
    }
    fn chmod(&mut self, _path: &[u8], _mode: u32) -> bool {
        true
    }
    fn rename(&mut self, _from: &[u8], _to: &[u8]) -> bool {
        true
    }
    fn system(&mut self, command: &[u8]) -> i32 {
        self.commands.push(command.to_vec());
        0
    }
}

/// The stand-ins of a case: every callee answers the value the harness's stub answered, and
/// what was called is kept in order.
#[derive(Default)]
struct Stubbed {
    restricted: bool,
    name_result: i32,
    element_result: bool,
    version: i32,
    log: Vec<(&'static str, Option<ElemId>, Option<ElemId>, i32)>,
}

impl Steps for Stubbed {
    fn restricted(&mut self, _sc: &mut Scene, chap: i32, id: ElemId, _un: &mut dyn Unported) -> bool {
        self.log.push(("si_elemCheckRestrictedSave", Some(id), None, chap));
        self.restricted
    }
    fn check_existence(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, _prefixed: &[u8], _subst: Option<&[u8]>, _chapter: &[u8], _purge: bool, listing: Option<&[ListNode]>) -> i32 {
        self.log.push((if listing.is_some() { "si_dbfileCheckExistence2" } else { "si_dbfileCheckExistence" }, None, None, 0));
        self.version
    }
    fn set_names(&mut self, _sc: &mut Scene, id: ElemId, _un: &mut dyn Unported) {
        self.log.push(("si_elemdictSetNamesFromHierarchy", Some(id), None, id.chap));
    }
    fn find_save_name(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, chap: i32, id: ElemId, _ask: &mut i16, _rename: bool, _listing: Option<&[ListNode]>) -> i32 {
        self.log.push(("SI_dbFindSaveName", Some(id), None, chap));
        self.name_result
    }
    fn save_an_element(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, chap: i32, id: ElemId) -> bool {
        self.log.push(("elemdictSaveAnElement", Some(id), None, chap));
        self.element_result
    }
    fn save_anim_file(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32, _selected_only: bool, _emergency: bool, _listing: Option<&[ListNode]>, _ask: &mut i16) -> bool {
        self.log.push(("elemdictSaveAnimFile", Some(anim_elem), target, chap));
        true
    }
    fn save_hier_animation(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32) {
        self.log.push(("elemdictSaveHierAnimation", Some(anim_elem), target, chap));
    }
    fn save_chapter(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, _selected_only: bool, _emergency: bool, only_chap: i32, chap: i32, _ask: &mut i16) -> bool {
        self.log.push(("elemdictSaveChapterElements", None, None, chap * 100 + (only_chap + 1)));
        self.element_result
    }
    fn save_all_anim(&mut self, _sc: &mut Scene, _h: &mut Host, _info: &mut DbInfo, _selected_only: bool, _emergency: bool, _listing: Option<&[ListNode]>, _ask: &mut i16) -> bool {
        self.log.push(("elemdictSaveAllAnimFiles", None, None, 0));
        true
    }
}

/// The number after `tag` in a vector's variant name (`…#files_e1_r0_n3_w1`), negative
/// numbers included.
fn variant(c: &Case, tag: &str) -> i32 {
    let v = c.name.split('#').nth(1).unwrap_or("");
    for part in v.split('_') {
        if let Some(rest) = part.strip_prefix(tag) {
            if let Ok(n) = rest.parse::<i32>() {
                return n;
            }
        }
    }
    0
}

/// The original's calls of the stubs named in `names`, in order: name, the first two element
/// pointers among the arguments, and the argument at `chap_at` as a number.
fn original_log(c: &Case, names: &[(&'static str, usize)]) -> Vec<(&'static str, Option<ElemId>, Option<ElemId>, i32)> {
    let mut out = Vec::new();
    for k in &c.calls {
        if let Some(&(name, chap_at)) = names.iter().find(|(n, _)| *n == k.name) {
            let mut elems = k.args.iter().filter_map(elem_of_word);
            let first = elems.next();
            let chap = k.args.get(chap_at).map_or(0, |w| val(w) as i32);
            out.push((name, first, None, chap));
        }
    }
    out
}

fn info_for(chapter_path: &[u8]) -> DbInfo {
    let mut info = DbInfo::new();
    info.path = Some(b"/db".to_vec());
    info.chapter_path = Some(chapter_path.to_vec());
    info
}

fn change_name(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let (name, old) = (opt_text(c, "name"), opt_text(c, "old"));
    savescene::change_dsc_name(&mut sc, name.as_deref(), arg(c, "only") as i16 != 0, arg(c, "repl") as i16 != 0, old.as_deref());
    same_nodes(c, &sc, &[4, 9]).and(|| same_dict(c, &back(sc)))
}

fn origin(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let info = info_for(b"");
    dictout::set_origin_db(&mut sc, &info);
    let nodes = c.after("nodes").words();
    let elems = c.after("elems").words();
    let mut diff = Vec::new();
    for (chap, cap) in SLOTS {
        for i in 0..cap.min(sc.dict.ed.lists[chap as usize].elems.len()) {
            let s = slot(chap, i as i32);
            if is_null(&elems[s * 33]) {
                continue;
            }
            // the word is the address of the path string: anything but the null the case gave
            let want = !is_null(&nodes[s * 13]);
            let got = sc.dict.node_of(chap, i as i32).is_some_and(|n| n.w00 != 0);
            if want != got {
                diff.push(format!("element {chap}/{i}: port {got}, original {want}"));
            }
        }
    }
    Verdict::from(diff.is_empty() && !mutating(), || diff.join("; "))
}

fn node_slot(c: &Case, name: &str) -> Option<(i32, i32)> {
    let (_, off) = ref_off(&c.input(name).words()[0])?;
    let s = off as usize / 0x34;
    let mut at = 0;
    for (chap, cap) in SLOTS {
        if s < at + cap {
            return Some((chap, (s - at) as i32));
        }
        at += cap;
    }
    None
}

fn copy_info(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let (Some(a), Some(b)) = (node_slot(c, "src"), node_slot(c, "dst")) else { return Verdict::Skip };
    if a == b {
        // one record as source and destination: the original drops three of its strings
        // and then copies their absence. The port takes two records.
        return Verdict::Skip;
    }
    let (Some(src), Some(mut dst)) = (sc.dict.node_of(a.0, a.1).cloned(), sc.dict.node_of(b.0, b.1).cloned()) else { return Verdict::Skip };
    let with_dict = !is_null(&c.input("edp").words()[0]);
    savescene::list_node_copy_info(&sc, with_dict, &src, &mut dst);
    *sc.dict.node_of_mut(b.0, b.1).unwrap() = dst;
    if mutating() {
        return Verdict::from(false, || "mutated".into());
    }
    same_nodes(c, &sc, &[2, 3, 4, 5, 6, 7, 9])
}

fn duplicates(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let chap = arg(c, "chap");
    let mut un = Recorder::default();
    savescene::check_chapter_duplicate_names(&mut sc, chap, &mut un);
    // the renames, in order: element, old name, new name
    let got: Vec<(Option<ElemId>, Vec<u8>, Vec<u8>)> = un
        .calls
        .iter()
        .filter(|k| k.name == "si_edAnimElemChangeName")
        .map(|k| match &k.args[..] {
            [Arg::Elem(e), Arg::Str(a), Arg::Str(b)] => (Some(*e), a.clone(), b.clone()),
            _ => (None, Vec::new(), Vec::new()),
        })
        .collect();
    let want: Vec<(Option<ElemId>, Vec<u8>, Vec<u8>)> =
        c.calls.iter().filter(|k| k.name == "si_edAnimElemChangeName").map(|k| (elem_of_word(&k.args[0]), call_text(c, &k.args[2]), call_text(c, &k.args[4]))).collect();
    Verdict::from(got == want && !mutating(), || format!("renames: port {got:?}, original {want:?}"))
        .and(|| same_nodes(c, &sc, &[5, 9]))
        .and(|| same_dict(c, &back(sc)))
}

fn save_chapter(c: &Case) -> Verdict {
    let mut sc = scene(c);
    // the name records' first word: the database the element was read from
    let nodes = c.input("nodes").words().to_vec();
    sc.db_paths = vec![b"/db".to_vec(), b"/other".to_vec()];
    for (chap, cap) in SLOTS {
        for i in 0..cap.min(sc.dict.ed.lists[chap as usize].elems.len()) {
            let mark = match &nodes[slot(chap, i as i32) * 13] {
                Word::Ref { name, .. } if name == "dbpath" => 1,
                Word::Ref { name, .. } if name == "other" => 2,
                _ => 0,
            };
            if let Some(n) = sc.dict.node_of_mut(chap, i as i32) {
                n.w00 = mark;
            }
        }
    }
    sc.saving.piped = arg(c, "piped") as u16 != 0;
    sc.saving.no_old_ver = arg(c, "nov") as i16;
    sc.saving.error_code = arg(c, "err");
    let chap = arg(c, "chap");
    // "selected only" reads the selected word of each element's root: a root index beyond
    // the chapter's count is a slot the case's chapter does not have (the original reads
    // the memory behind its array)
    if arg(c, "sel") as i16 != 0 {
        let l = &sc.dict.ed.lists[chap as usize];
        if l.elems.iter().any(|e| e.rec.root_id as usize >= l.elems.len()) {
            return Verdict::Skip;
        }
    }
    let mut steps = Stubbed { restricted: variant(c, "r") != 0, name_result: variant(c, "n"), element_result: variant(c, "w") != 0, ..Stubbed::default() };
    let mut fs = FixedFs::new(variant(c, "e") != 0, true);
    let mut un = Recorder::default();
    let mut ui = Answers::default();
    let chapter_path = c.input("chpath").to_bytes();
    let chapter_path = &chapter_path[..chapter_path.iter().position(|&b| b == 0).unwrap_or(chapter_path.len())];
    let mut info = info_for(chapter_path);
    let mut ask = val(&c.input("ask").words()[0]) as i16;
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        // the harness stubs `SI_dbSetChapter` and `SI_dbfilepathBuild`: the chapter's
        // directory stays what the case gave
        let r = save_chapter_elements_fixed_dir(&mut steps, &mut sc, &mut h, &mut info, chapter_path, arg(c, "sel") as i16 != 0, arg(c, "emg") as i16 != 0, arg(c, "only"), chap, &mut ask);
        r
    };
    let want_log = original_log(c, &[("si_elemCheckRestrictedSave", 1), ("SI_dbFindSaveName", 2), ("elemdictSaveAnElement", 2)]);
    let paths: Vec<Vec<u8>> = fs.asked.borrow().clone();
    let want_paths: Vec<Vec<u8>> = c.calls.iter().filter(|k| k.name == "SI_dbCheckFileExistence").map(|k| call_text(c, &k.args[0])).collect();
    let dialogue: Vec<Vec<Vec<u8>>> = ui
        .asked
        .iter()
        .filter(|k| k.name == "SI_dialogRequestLong")
        .map(|k| k.args.iter().filter_map(|a| if let Arg::Str(s) = a { Some(s.clone()) } else { None }).collect())
        .collect();
    let want_dialogue: Vec<Vec<Vec<u8>>> = c.calls.iter().filter(|k| k.name == "SI_dialogRequestLong").map(|k| k.args.iter().take(4).map(|w| call_text(c, w)).collect()).collect();
    let err = sc.saving.error_code;
    let want_err = c.try_output("err").map_or(arg(c, "err"), |f| f.words().first().and_then(|w| w.value()).map_or(arg(c, "err"), |v| v as i32));
    Verdict::from(got as i32 == eax(c) && !mutating(), || format!("result: port {got}, original {}; port calls {:?}; original {want_log:?}; asked {:?}", eax(c), steps.log, ui.asked))
        .and(|| Verdict::from(steps.log == want_log, || format!("calls: port {:?}, original {want_log:?}", steps.log)))
        .and(|| Verdict::from(paths == want_paths, || format!("existence asked for: port {:?}, original {:?}", show(&paths), show(&want_paths))))
        .and(|| Verdict::from(dialogue == want_dialogue, || format!("failure dialogue: port {dialogue:?}, original {want_dialogue:?}")))
        .and(|| Verdict::from(err == want_err, || format!("error code: port {err}, original {want_err}")))
        .and(|| same_nodes(c, &sc, &[2]))
        .and(|| same_dict(c, &back(sc)))
}

fn show(v: &[Vec<u8>]) -> Vec<String> {
    v.iter().map(|s| String::from_utf8_lossy(s).into_owned()).collect()
}

/// `save_chapter_elements_with` on a database record for `/db`: the case gives the chapter's
/// directory as `/db/<chapter>/`, which is what the port builds.
#[allow(clippy::too_many_arguments)]
fn save_chapter_elements_fixed_dir(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, _dir: &[u8], sel: bool, emg: bool, only: i32, chap: i32, ask: &mut i16) -> bool {
    let name = sc.dict.ed.lists[chap as usize].name.clone();
    info.add_chapter(&name);
    dictout::save_chapter_elements_with(steps, sc, h, info, sel, emg, only, chap, ask)
}

const WRITERS: [(&str, &str, &str); 16] = [
    ("SI_elemdictWriteModel", "SI_omodelBinaryWrite", ".hrc"),
    ("SI_elemdictWriteShape", "SI_shapeBinaryWrite", ".shp"),
    ("SI_elemdictWriteClusters", "SI_clusterAnimBinaryWrite", ".cls"),
    ("SI_elemdictWriteLight", "SI_elemdictWriteLight", ""),
    ("SI_elemdictWriteCamera", "SI_elemdictWriteCamera", ""),
    ("SI_elemdictWriteLattice", "SI_elemdictWriteLattice", ""),
    ("SI_elemdictWriteMaterial", "SI_materialWriteBinaryFile", ".mtr"),
    ("SI_elemdictWriteShader", "SI_udfWrite", ""),
    ("SI_elemdictWriteTexture2D", "SI_textureWriteBinaryFile", ".txt"),
    ("SI_elemdictWriteTexture3D", "SI_texture3DWriteBinaryFile", ".t3D"),
    ("SI_elemdictWriteWave", "SI_dbwaveAsciiWrite", ""),
    ("SI_elemdictWriteExpressions", "SI_elemdictWriteExpressions", ""),
    ("SI_elemdictWriteSetup", "SI_setupFileSavePath", ""),
    ("SI_sprdSetupWrite", "SI_sprdSetupWrite", ""),
    ("SI_elemdictWriteChannelSetup", "SI_chnSetupSave", ""),
    ("SI_elemdictWriteParticleSetup", "SI_particleSetupSave", ""),
];

fn save_element(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let chap = arg(c, "chap");
    let e = elem_arg(c, "e");
    if sc.dict.slot(e).is_none() || sc.dict.node_of(e.chap, e.index).is_none() {
        return Verdict::Skip;
    }
    let cid = sc.program.cid;
    // a cluster whose model has no object: the harness's stub stands for a writer that
    // refuses it itself (error 6), which the port does at this level
    if chap == cid.cls {
        if let Some(m) = elemrel::find_related_elem(&mut sc.dict, e, false, 0, 700, cid.model) {
            if sc.dict.slot(m).is_some_and(|x| x.rec.data.is_null()) {
                return Verdict::Skip;
            }
        }
    }
    let writes = variant(c, "w") != 0;
    let mut fs = FixedFs::new(variant(c, "e") != 0, variant(c, "a") == 0);
    let mut un = Recorder::default();
    let mut ui = Answers { default: writes as i32, ..Answers::default() };
    let chapter_path = c.input("chpath").to_bytes();
    let chapter_path = &chapter_path[..chapter_path.iter().position(|&b| b == 0).unwrap_or(chapter_path.len())];
    let mut info = info_for(chapter_path);
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        dictout::save_an_element(&mut sc, &mut h, &mut info, chap, e)
    };
    // which writer, with which file
    let want: Vec<(&str, Vec<u8>)> = c
        .calls
        .iter()
        .filter_map(|k| WRITERS.iter().find(|w| w.0 == k.name).map(|w| (w.1, [&call_text(c, &k.args[0])[..], w.2.as_bytes()].concat())))
        .collect();
    let got_calls: Vec<(&str, Vec<u8>)> = ui
        .asked
        .iter()
        .filter(|k| WRITERS.iter().any(|w| w.1 == k.name))
        .map(|k| (k.name, if let Some(Arg::Str(s)) = k.args.first() { s.clone() } else { Vec::new() }))
        .collect();
    // the two setup wrappers answer 1 whatever their writer did; the harness's stub is the
    // wrapper itself
    let wrapper_always_one = chap == cid.sts || chap == cid.stc;
    // a chapter without writer, a cluster or a lattice without model: the original returns
    // what its caller had in a register
    let shaders = [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon];
    let handled = [cid.model, cid.shp, cid.cls, cid.lig, cid.cam, cid.lat, cid.mat, cid.t2d, cid.t3d, cid.wav, cid.exp, cid.sts, cid.spd, cid.stc, cid.sfk].contains(&chap) || shaders.contains(&chap);
    let no_writer_called = want.is_empty();
    let result_same = wrapper_always_one || !handled || no_writer_called || (got as i32 != 0) == (c.output("ax").i16() != 0);
    let nodes = c.after("nodes").words();
    let time = val(&nodes[slot(e.chap, e.index) * 13 + 8]) as i32;
    let got_time = sc.dict.node_of(e.chap, e.index).map_or(0, |n| n.time);
    Verdict::from(got_calls == want && !mutating(), || format!("writer: port {:?}, original {:?}", got_calls.iter().map(|x| (x.0, String::from_utf8_lossy(&x.1).into_owned())).collect::<Vec<_>>(), want.iter().map(|x| (x.0, String::from_utf8_lossy(&x.1).into_owned())).collect::<Vec<_>>()))
        .and(|| Verdict::from(result_same, || format!("result: port {got}, original {}", c.output("ax").i16())))
        .and(|| Verdict::from(time == got_time, || format!("time: port {got_time}, original {time}")))
        .and(|| same_nodes(c, &sc, &[2]))
        .and(|| same_dict(c, &back(sc)))
}

fn find_name(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let chap = arg(c, "chap");
    let e = elem_arg(c, "e");
    if sc.dict.slot(e).is_none() || sc.dict.node_of(e.chap, e.index).is_none() {
        return Verdict::Skip;
    }
    sc.saving.piped = arg(c, "piped") as u16 != 0;
    sc.saving.no_old_ver = arg(c, "nov") as i16;
    sc.standalone_renderer = arg(c, "sar") as u16 != 0;
    sc.saving.emergency_saving = arg(c, "emg") as u16 != 0;
    let exists = variant(c, "e") != 0;
    let mut steps = Stubbed { version: variant(c, "v"), ..Stubbed::default() };
    let mut fs = FixedFs::new(exists, exists);
    let mut un = Recorder::default();
    let mut ui = Answers { default: variant(c, "a"), ..Answers::default() };
    let chapter_path = c.input("chpath").to_bytes();
    let chapter_path = &chapter_path[..chapter_path.iter().position(|&b| b == 0).unwrap_or(chapter_path.len())];
    let mut info = info_for(chapter_path);
    let name = sc.dict.ed.lists[chap as usize].name.clone();
    info.add_chapter(&name);
    let mut ask = val(&c.input("ask").words()[0]) as i16;
    let listing: Option<Vec<ListNode>> = if is_null(&c.input("lst").words()[0]) { None } else { Some(Vec::new()) };
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        dictout::find_save_name_with(&mut steps, &mut sc, &mut h, &mut info, chap, e, &mut ask, arg(c, "ren") as i16 != 0, listing.as_deref())
    };
    let want_ask = c.after("ask").words().first().map_or(0, |w| val(w) as i16);
    let names = ["si_dbfileCheckExistence", "si_dbfileCheckExistence2", "si_elemdictSetNamesFromHierarchy"];
    let want_log: Vec<&str> = c.calls.iter().filter_map(|k| names.iter().find(|n| **n == k.name).copied()).collect();
    let got_log: Vec<&str> = steps.log.iter().map(|x| x.0).collect();
    let asked = |name: &str| ui.asked.iter().filter(|k| k.name == name).count();
    let (d1, d2) = (asked("SI_dialogRequest34"), asked("SI_dbMRequest2"));
    let (w1, w2) = (count_calls(c, "SI_dialogRequest34"), count_calls(c, "SI_dbMRequest2"));
    // the first dialogue's first line
    let line = ui.asked.iter().find(|k| k.name == "SI_dialogRequest34").and_then(|k| if let Some(Arg::Str(s)) = k.args.first() { Some(s.clone()) } else { None });
    let want_line = c.calls.iter().find(|k| k.name == "SI_dialogRequest34").map(|k| call_text(c, &k.args[0]));
    let moved: Vec<Vec<u8>> = fs.commands.clone();
    let want_moved: Vec<Vec<u8>> = c.calls.iter().filter(|k| k.name == "SI_dbFileMove").map(|k| [&b"cp "[..], &call_text(c, &k.args[0]), b" ", &call_text(c, &k.args[1])].concat()).collect();
    Verdict::from(got == eax(c) && !mutating(), || format!("result: port {got}, original {}", eax(c)))
        .and(|| Verdict::from(ask == want_ask, || format!("ask: port {ask}, original {want_ask}")))
        .and(|| Verdict::from(got_log == want_log, || format!("calls: port {got_log:?}, original {want_log:?}")))
        .and(|| Verdict::from((d1, d2) == (w1, w2), || format!("dialogues: port {:?}, original {:?}", (d1, d2), (w1, w2))))
        .and(|| Verdict::from(line == want_line, || format!("dialogue text: port {line:?}, original {want_line:?}")))
        .and(|| Verdict::from(moved == want_moved, || format!("copies: port {:?}, original {:?}", show(&moved), show(&want_moved))))
        .and(|| same_nodes(c, &sc, &[4, 5, 6, 9]))
        .and(|| same_dict(c, &back(sc)))
}

fn all_anim(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let mut steps = Stubbed::default();
    let mut fs = FixedFs::new(true, true);
    let mut un = Recorder::default();
    let mut ui = Answers::default();
    let mut info = info_for(b"");
    let mut ask = 1i16;
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        dictout::save_all_anim_files_with(&mut steps, &mut sc, &mut h, &mut info, arg(c, "sel") as i16 != 0, arg(c, "emg") as i16 != 0, None, &mut ask)
    };
    let want: Vec<(Option<ElemId>, Option<ElemId>, i32)> = c
        .calls
        .iter()
        .filter(|k| k.name == "elemdictSaveAnimFile")
        .map(|k| (elem_of_word(&k.args[2]), elem_of_word(&k.args[3]), val(&k.args[4]) as i32))
        .collect();
    let got_calls: Vec<(Option<ElemId>, Option<ElemId>, i32)> = steps.log.iter().map(|x| (x.1, x.2, x.3)).collect();
    Verdict::from(got as i32 == eax(c) && !mutating(), || format!("result: port {got}, original {}", eax(c))).and(|| Verdict::from(got_calls == want, || format!("files: port {got_calls:?}, original {want:?}")))
}

fn one_anim(c: &Case) -> Verdict {
    let mut sc = scene(c);
    sc.saving.piped = arg(c, "piped") as u16 != 0;
    let a = elem_arg(c, "a");
    let target = elem_of_word(&c.input("t").words()[0]);
    if sc.dict.slot(a).is_none() || target.is_some_and(|t| sc.dict.slot(t).is_none()) {
        return Verdict::Skip;
    }
    let mut steps = Stubbed { name_result: variant(c, "n"), ..Stubbed::default() };
    let mut fs = FixedFs::new(true, true);
    let mut un = Recorder::default();
    let mut ui = Answers::default();
    let mut info = info_for(b"");
    let mut ask = 1i16;
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        dictout::save_anim_file_with(&mut steps, &mut sc, &mut h, &mut info, a, target, arg(c, "tc"), arg(c, "sel") as i16 != 0, arg(c, "emg") as i16 != 0, None, &mut ask)
    };
    let names = ["SI_dbFindSaveName", "elemdictSaveHierAnimation"];
    let want: Vec<&str> = c.calls.iter().filter_map(|k| names.iter().find(|n| **n == k.name).copied()).collect();
    let got_calls: Vec<&str> = steps.log.iter().map(|x| x.0).collect();
    Verdict::from(got as i32 == eax(c) && !mutating(), || format!("result: port {got}, original {}", eax(c))).and(|| Verdict::from(got_calls == want, || format!("calls: port {got_calls:?}, original {want:?}")))
}

fn element_anim(c: &Case) -> Verdict {
    let mut sc = scene(c);
    let e = elem_arg(c, "e");
    if sc.dict.slot(e).is_none() {
        return Verdict::Skip;
    }
    // relation data: the case's two words stand for an expression's parameter
    let rdata = c.input("rdata").words().to_vec();
    for l in sc.dict.ed.lists.iter_mut() {
        for el in l.elems.iter_mut() {
            for g in el.forward.iter_mut() {
                for r in g.rels.iter_mut() {
                    r.data = si_types::Ptr32::NULL;
                }
            }
        }
    }
    let rels = c.input("rels").words().to_vec();
    for (chap, cap) in SLOTS {
        for i in 0..cap.min(sc.dict.ed.lists[chap as usize].elems.len()) {
            let s = slot(chap, i as i32);
            for g in 0..NG {
                let n = sc.dict.ed.lists[chap as usize].elems[i].forward[g].rels.len();
                for k in 0..n {
                    let w = &rels[((s * 2 * NG + g) * K + k) * 8 + 6];
                    if let Some((name, off)) = ref_off(w) {
                        if name == "rdata" {
                            let param = val(&rdata[off as usize / 4]) as i32;
                            let h = sc.rel_data.alloc(si_ascii::reldata::RelData::Expression(si_ascii::reldata::Expression { fcurve: param, name: None }));
                            sc.dict.ed.lists[chap as usize].elems[i].forward[g].rels[k].data = si_types::Ptr32::new(h);
                        }
                    }
                }
            }
        }
    }
    // the hierarchy animation: two names
    let han = c.input("han").to_bytes();
    let name_at = |o: usize| han[o..].iter().take_while(|&&b| b != 0).copied().collect::<Vec<u8>>();
    let mut ha = si_elemio::anim::HierAnim::new();
    ha.add_elem(&name_at(0), -1);
    ha.add_elem(&name_at(32), 0);
    let mut got: Vec<AnimItem> = Vec::new();
    dictout::element_animation_items(&mut sc, e, &ha, &mut |it| got.push(it));
    let want: Vec<AnimItem> = c
        .calls
        .iter()
        .filter(|k| k.name == "SI_elemdictWriteAnimation")
        .map(|k| AnimItem { curve: val(&k.args[1]), owner: val(&k.args[2]) as i32, act_rel_id: val(&k.args[4]) as i32, act_occ_id: val(&k.args[5]) as i32, flag: val(&k.args[6]) as i16 })
        .collect();
    let kinds_zero = c.calls.iter().filter(|k| k.name == "SI_elemdictWriteAnimation").all(|k| val(&k.args[3]) == 0);
    Verdict::from(got == want && kinds_zero && !mutating(), || format!("items: port {got:?}, original {want:?}"))
}

fn save_elements(c: &Case) -> Verdict {
    let mut sc = scene(c);
    sc.saving.piped = arg(c, "piped") as u16 != 0;
    sc.saving.no_old_ver = arg(c, "nov") as i16;
    let mut steps = Stubbed { element_result: variant(c, "w") != 0, ..Stubbed::default() };
    let mut fs = FixedFs::new(true, true);
    let mut un = Recorder::default();
    let mut ui = Answers::default();
    let mut info = info_for(b"");
    let name = sc.dict.ed.lists[sc.program.cid.ani as usize].name.clone();
    info.add_chapter(&name);
    let mut ask = 1i16;
    let got = {
        let mut h = Host { fs: &mut fs, un: &mut un, ui: &mut ui };
        dictout::save_elements_with(&mut steps, &mut sc, &mut h, &mut info, arg(c, "sel") as i16 != 0, arg(c, "emg") as i16 != 0, arg(c, "only"), &mut ask)
    };
    // the chapters in the order they were handed to the chapter saver, then the animation
    let want: Vec<(&str, i32)> = c
        .calls
        .iter()
        .filter_map(|k| match k.name.as_str() {
            "elemdictSaveChapterElements" => Some(("elemdictSaveChapterElements", val(&k.args[5]) as i32 * 100 + (val(&k.args[4]) as i32 + 1))),
            "elemdictSaveAllAnimFiles" => Some(("elemdictSaveAllAnimFiles", 0)),
            _ => None,
        })
        .collect();
    let got_calls: Vec<(&str, i32)> = steps.log.iter().map(|x| (x.0, x.3)).collect();
    // every name record of a slot not freed learns the database
    let nodes = c.after("nodes").words();
    let elems = c.after("elems").words();
    let mut marks = Vec::new();
    for (chap, cap) in SLOTS {
        for i in 0..cap.min(sc.dict.ed.lists[chap as usize].elems.len()) {
            let s = slot(chap, i as i32);
            if is_null(&elems[s * 33]) {
                continue;
            }
            let want = !is_null(&nodes[s * 13]);
            let got = sc.dict.node_of(chap, i as i32).is_some_and(|n| n.w00 != 0);
            if want != got {
                marks.push(format!("element {chap}/{i}: database mark: port {got}, original {want}"));
            }
        }
    }
    Verdict::from(got as i32 == eax(c) && !mutating(), || format!("result: port {got}, original {}", eax(c)))
        .and(|| Verdict::from(got_calls == want, || format!("calls: port {got_calls:?}, original {want:?}")))
        .and(|| Verdict::from(marks.is_empty(), || marks.join("; ")))
        .and(|| same_dict(c, &back(sc)))
}

pub fn lookup(name: &str) -> Option<Check> {
    let base = name.split('#').next().unwrap_or(name);
    Some(match base {
        "SI_elemdictChangeDscName" => change_name,
        "si_elemdictSetOriginDB" => origin,
        "SI_dbListNodeCopyInfo" => copy_info,
        "si_dbCheckChapterDuplicateNames" => duplicates,
        "elemdictSaveChapterElements" => save_chapter,
        "elemdictSaveAnElement" => save_element,
        "SI_dbFindSaveName" => find_name,
        "elemdictSaveAllAnimFiles" => all_anim,
        "elemdictSaveAnimFile" => one_anim,
        "elemdictSaveElementAnimation" => element_anim,
        "SI_elemdictSaveElements" => save_elements,
        _ => return None,
    })
}
