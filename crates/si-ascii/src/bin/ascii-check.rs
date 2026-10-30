// SPDX-License-Identifier: MIT
//! Corpus checks for the ASCII files: run the Rust readers and writers over every original
//! file under a directory and count outcomes. The numbers are the evidence quoted in
//! finding 0006.
//!   ascii-check <dir>            every check
//!   ascii-check <dir> dsc -v     one kind of file; -v lists the files behind each count
//!   ascii-check <dir> sts --standalone   setup files as the programs without the interface
//!                                write them (evalScene's saved scenes)
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            files(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Counts with, optionally, the files behind them.
#[derive(Default)]
struct Tally {
    counts: BTreeMap<String, Vec<PathBuf>>,
}

impl Tally {
    fn add(&mut self, key: impl Into<String>, p: &Path) {
        self.counts.entry(key.into()).or_default().push(p.to_path_buf());
    }
    fn print(&self, title: &str, verbose: bool) {
        println!("{title}");
        for (k, v) in &self.counts {
            println!("{:6}  {k}", v.len());
            if verbose {
                for p in v.iter().take(12) {
                    println!("          {}", p.display());
                }
            }
        }
    }
}

/// The first line on which two texts differ, for sorting differences by cause.
fn first_difference(a: &[u8], b: &[u8]) -> (String, String) {
    let (mut la, mut lb) = (a.split(|&c| c == b'\n'), b.split(|&c| c == b'\n'));
    loop {
        match (la.next(), lb.next()) {
            (Some(x), Some(y)) if x == y => continue,
            (x, y) => {
                let s = |v: Option<&[u8]>| v.map_or("(end)".to_string(), |v| String::from_utf8_lossy(v).into_owned());
                return (s(x), s(y));
            }
        }
    }
}

/// The relation dictionary of the first relations resource file under the directory, for the
/// cross-check of the scene descriptions' relation lines.
fn relation_dictionary(all: &[PathBuf]) -> Option<si_ascii::srel::Relations> {
    for p in all.iter().filter(|p| ext(p) == "rsrc") {
        let Ok(data) = std::fs::read(p) else { continue };
        if data.starts_with(b"SREL") {
            return si_ascii::srel::read(&data).ok().map(|(r, _)| r);
        }
    }
    None
}

fn check_dsc(all: &[PathBuf], verbose: bool) {
    use si_ascii::dsc;
    let dictionary = relation_dictionary(all);
    let mut against_dictionary = Tally::default();
    let (mut parse, mut round, mut diff, mut order, mut by_version) =
        (Tally::default(), Tally::default(), Tally::default(), Tally::default(), Tally::default());
    let mut longest_token = 0usize;
    let mut relids: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    let mut distinct = std::collections::BTreeSet::new();
    let mut total = 0;
    for p in all.iter().filter(|p| ext(p) == "dsc") {
        let Ok(data) = std::fs::read(p) else { continue };
        if !data.starts_with(b"SDSC") {
            parse.add("not a scene description (no SDSC tag)", p);
            continue;
        }
        total += 1;
        distinct.insert(data.clone());
        longest_token = longest_token.max(data.split(|c| c.is_ascii_whitespace()).map(|t| t.len()).max().unwrap_or(0));
        let (d, info) = match dsc::read(&data) {
            Ok(x) => x,
            Err(e) => {
                parse.add(format!("the original would not return: {e:?}"), p);
                continue;
            }
        };
        let version = si_file::format_g(d.version as f64);
        let key = match (info.sections_closed, info.all_tokens_consumed, d.messages.is_empty()) {
            (true, true, true) => "read completely: three sections closed, every token consumed, no message".to_string(),
            (true, true, false) => {
                let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                for m in &d.messages {
                    *kinds.entry(format!("{m:?}")).or_default() += 1;
                }
                format!("read to the end, with reports on standard error: {kinds:?}")
            }
            (false, _, _) => "a section did not end on its end word".to_string(),
            (true, false, _) => "tokens left after EndOfENVIRONMENT".to_string(),
        };
        parse.add(key, p);
        for b in &d.relations {
            for r in &b.relations {
                let e = relids.entry(r.relid).or_default();
                e.0 += 1;
                e.1 += r.data.is_some() as usize;
                // Does the line fit the operator record its sub-definition number selects?
                // (Custom effects aside: their number is not a sub-definition.)
                if let (Some(dict), true) = (&dictionary, r.relid != si_ascii::reldata::REL_CUSTOM) {
                    let any = si_ascii::srel::CHAPTER_ANY as i32;
                    let key = if dict.rel_index_from_id(r.relid) < 0 {
                        format!("relation id {} is not in the dictionary", r.relid)
                    } else if r.subdef > dict.rel_nb_rules(r.relid) {
                        format!("sub-definition {} of relation {} is beyond its {} records", r.subdef, r.relid, dict.rel_nb_rules(r.relid))
                    } else {
                        match dict.rel_attributes(r.relid, r.subdef) {
                            Some(rec) => {
                                let (src, dst) = (rec.src_chapter as i32, rec.dst_chapter as i32);
                                if (src == b.list1 || src == any) && (dst == b.list2 || dst == any) {
                                    "chapters agree with the record (result chapter left, argument chapter right, `*` any)".to_string()
                                } else {
                                    format!(
                                        "relation {} sub {}: block {}/{} against record {}/{}",
                                        r.relid,
                                        r.subdef,
                                        String::from_utf8_lossy(&b.name1),
                                        String::from_utf8_lossy(&b.name2),
                                        dict.chapter_name(rec.src_chapter),
                                        dict.chapter_name(rec.dst_chapter)
                                    )
                                }
                            }
                            None => "no record".to_string(),
                        }
                    };
                    against_dictionary.add(key, p);
                }
            }
        }
        // read, write, read again
        let out = d.write(d.version);
        match dsc::read(&out) {
            Ok((d2, _)) if d2 == d => round.add("written and read again: same structure", p),
            Ok(_) => round.add("written and read again: DIFFERENT structure", p),
            Err(e) => round.add(format!("written file does not read: {e:?}"), p),
        }
        if out == data {
            round.add("written file identical to the original, byte for byte (its own version number in the header)", p);
            by_version.add(format!("version {version:>8}: identical"), p);
        } else if d.write_as(d.version, si_ascii::cnum::CLib::Msvc) == data {
            round.add("written file identical to the original with three-digit exponents (the NT build's C library)", p);
            by_version.add(format!("version {version:>8}: identical (NT number format)"), p);
        } else {
            by_version.add(format!("version {version:>8}: different"), p);
            let (a, b) = first_difference(&data, &out);
            let cause = if a.trim_end() == b.trim_end() {
                "trailing blanks on a line".to_string()
            } else if a.contains("WIRECOL 0 7") || a.contains("DISPLAY 0 2") {
                "the file spells out a default the writer leaves out".to_string()
            } else {
                // reduce the line to its shape: numbers to #
                let shape = |s: &str| {
                    s.split_whitespace().map(|t| if t.parse::<f64>().is_ok() { "#" } else { t }).collect::<Vec<_>>().join(" ")
                };
                if shape(&a) == shape(&b) {
                    let (ta, tb): (Vec<&str>, Vec<&str>) = (a.split_whitespace().collect(), b.split_whitespace().collect());
                    let i = (0..ta.len()).find(|&i| ta[i] != tb[i]).unwrap_or(0);
                    let kw = ta[..i].iter().rev().find(|t| t.parse::<f64>().is_err()).copied().unwrap_or("?");
                    format!("a number is printed differently after {kw}: e.g. file {:?}, ours {:?}", ta[i], tb[i])
                } else {
                    format!("first differing line: file {:?} / ours {:?}", shape(&a), shape(&b))
                }
            };
            diff.add(cause, p);
        }
        // is the file laid out as this build's writer would lay it out?
        let names: Vec<&[u8]> = d.element_order.iter().map(|&l| d.chapters[l as usize].name.as_slice()).collect();
        order.add(if names.windows(2).all(|w| w[0] < w[1]) { "ELEMENTS chapters in ascending strcmp order" } else { "ELEMENTS chapters NOT in strcmp order" }, p);
        let ascends = |cmp: &dyn Fn(&[u8], &[u8]) -> i32| {
            d.element_order.iter().filter(|&&l| l != dsc::PICTURES).all(|&l| d.chapters[l as usize].elements.windows(2).all(|w| cmp(&w[0].name, &w[1].name) <= 0))
        };
        let (linux, nt) = (ascends(&dsc::compare_elem_name), ascends(&dsc::compare_elem_name_nt));
        order.add(
            match (linux, nt) {
                (true, true) => "element order (PICTURES aside): fits the Linux and the NT build's comparison alike",
                (true, false) => "element order (PICTURES aside): fits the Linux build's comparison only (case matters)",
                (false, true) => "element order (PICTURES aside): fits the NT build's comparison only (case ignored)",
                (false, false) => "element order (PICTURES aside): fits NEITHER build's comparison",
            },
            p,
        );
        by_version.add(format!("version {version:>8}: element order fits {}", match (linux, nt) { (true, true) => "both", (true, false) => "Linux only", (false, true) => "NT only", _ => "neither" }), p);
        let pos = |b: &dsc::RelationBlock| {
            dsc::RELATION_ORDER.iter().position(|(x, y)| x.as_bytes() == b.name1 && y.as_bytes() == b.name2)
        };
        let idx: Vec<Option<usize>> = d.relations.iter().map(pos).collect();
        if idx.iter().any(|i| i.is_none()) {
            order.add("RELATIONS has a chapter pair that is not in the writer's list", p);
        } else if idx.windows(2).all(|w| w[0] < w[1]) {
            order.add("RELATIONS blocks in the writer's order", p);
        } else {
            order.add("RELATIONS blocks NOT in the writer's order", p);
        }
        let epos: Vec<Option<usize>> =
            d.environment.iter().map(|b| dsc::ENVIRONMENT_ORDER.iter().position(|n| n.as_bytes() == b.name)).collect();
        if epos.iter().any(|i| i.is_none()) {
            order.add("ENVIRONMENT has a chapter that is not in the writer's list", p);
        } else if epos.windows(2).all(|w| w[0] < w[1]) {
            order.add("ENVIRONMENT chapters in the writer's order", p);
        } else {
            order.add("ENVIRONMENT chapters NOT in the writer's order", p);
        }
        let covered = d.environment.iter().all(|b| {
            b.list >= 0 && b.elems.len() == d.chapters[b.list as usize].elements.len()
        });
        order.add(if covered { "every ENVIRONMENT chapter has exactly one line per element" } else { "an ENVIRONMENT chapter does not have one line per element" }, p);
        let ascending = d.environment.iter().all(|b| b.elems.windows(2).all(|w| w[0] < w[1]));
        order.add(if ascending { "ENVIRONMENT lines in ascending element number" } else { "ENVIRONMENT lines NOT in ascending element number" }, p);
        order.add(if d.play.is_some() { "has a PLAYCTRL block" } else { "no PLAYCTRL block" }, p);
    }
    println!("== scene descriptions (.dsc, SDSC): {total} files, {} distinct contents; longest token {longest_token} bytes ==", distinct.len());
    parse.print("-- reading", verbose);
    round.print("-- writing", verbose);
    diff.print("-- files that do not come back byte for byte, by the first difference", verbose);
    by_version.print("-- byte-for-byte result by file version", false);
    order.print("-- layout compared with what this build's writer does", verbose);
    if dictionary.is_some() {
        against_dictionary.print("-- relation lines against the relation dictionary (Relations.rsrc), counted per line", false);
    }
    println!("-- relation ids (lines, lines with RELDATA)");
    let line: Vec<String> = relids.iter().map(|(k, (n, d))| format!("{k}:{n}/{d}")).collect();
    println!("        {}", line.join(" "));
}

/// Setup files (`.sts`, tag `SSET`). `mr19`: check as a program that has mental ray 1.9
/// installed (the reader then keeps the renderer `MENTAL_RAY` instead of turning it into
/// `MENTAL_RAY_2X`). `standalone`: read and write as a program without the interface
/// (`SI_standaloneRenderer` set: `evalScene`, whose saved scenes the oracle keeps), which
/// leaves the show flags and the window layout out, and has no dialogs (no mouse
/// preferences).
fn check_sts(all: &[PathBuf], mr19: bool, standalone: bool) {
    use si_ascii::setup::{self, check};
    let session = setup::Session { mental_ray_19: mr19, standalone_renderer: standalone, mouse_dialog: !standalone, ..Default::default() };
    let (mut summary, mut distinct) = (check::Summary::default(), check::Summary::default());
    let mut seen = std::collections::BTreeSet::new();
    let (mut n, mut other) = (0usize, BTreeMap::<String, usize>::new());
    for p in all.iter().filter(|p| ext(p) == "sts") {
        let Ok(data) = std::fs::read(p) else { continue };
        n += 1;
        if !data.starts_with(b"SSET") {
            let head: Vec<u8> = data.iter().copied().take_while(|b| *b != b'\n').take(12).collect();
            *other.entry(String::from_utf8_lossy(&head).into_owned()).or_default() += 1;
            continue;
        }
        let r = check::check_file(&data, &session);
        let name = p.display().to_string();
        summary.add(&name, &r);
        if seen.insert(data) {
            distinct.add(&name, &r);
        }
    }
    println!("== setup files (.sts): {n} files; not setup files (another product's text format, counted only): {other:?} ==");
    println!("-- all SSET files\n{}", summary.report());
    println!("-- distinct contents\n{}", distinct.report());
}

/// The setup files `evalScene` saved (finding 0031, third pass: `evalScene/saved/<db>/ev*`,
/// the first scene of each sample database evaluated and written back with `-o`) against
/// the port: the sample setup the program read (`<data1>/<db>/SETUP_SOFT/<setup>.sts`, the
/// first scene's row of the oracle's `scenes.tsv`) is read over `evalScene`'s start-up state
/// ([`evalscene_startup`]) and written by the port as a program without the interface
/// (`SI_standaloneRenderer` set, no dialogs), at version 4,
/// and compared byte for byte with each file `evalScene` wrote from it. The writer's start
/// indent and the audio volume flag are program state: each combination is tried and the
/// count says which matched.
fn check_saved_sts(saved: &Path, data1: &Path, scenes_tsv: &Path, verbose: bool, out_dir: Option<&Path>) {
    use si_ascii::setup::{self, Session, WriteOptions};
    let label = |db: &str| db.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect::<String>();
    let text = std::fs::read_to_string(scenes_tsv).expect("scenes.tsv");
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let col = |n: &str| head.iter().position(|h| *h == n).expect("scenes.tsv column");
    let (cdb, csetup) = (col("database"), col("setup"));
    let mut first: BTreeMap<String, (String, String)> = BTreeMap::new();
    for l in lines {
        let f: Vec<&str> = l.split('\t').collect();
        first.entry(label(f[cdb])).or_insert((f[cdb].to_string(), f[csetup].to_string()));
    }
    let session = Session { standalone_renderer: true, mouse_dialog: false, ..Session::default() };
    let (mut tally, mut causes) = (Tally::default(), Tally::default());
    let mut n = 0usize;
    let Ok(rd) = std::fs::read_dir(saved) else { return };
    let mut dbs: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dbs.sort();
    for dbdir in dbs {
        let lab = dbdir.file_name().unwrap().to_string_lossy().into_owned();
        let Some((db, setup_name)) = first.get(&lab) else {
            tally.add("no row in scenes.tsv for this database", &dbdir);
            continue;
        };
        let input = data1.join(db).join("SETUP_SOFT").join(format!("{setup_name}.sts"));
        let Ok(data) = std::fs::read(&input) else {
            tally.add("sample setup not found", &input);
            continue;
        };
        let mut s = evalscene_startup();
        if let Err(e) = setup::read_into(&data, &mut s, &session) {
            tally.add(format!("the port's reader stops: {e}"), &input);
            continue;
        }
        let mut outs: Vec<(String, Vec<u8>)> = Vec::new();
        for indent in [0, 1] {
            for audio_volume in [true, false] {
                let o = WriteOptions { indent, audio_volume, ..WriteOptions::default() };
                outs.push((format!("indent {indent}, audio volume {audio_volume}"), setup::write_with(&s, &o, &session)));
            }
        }
        let mut files_here = Vec::new();
        for sub in ["ev", "ev_m", "ev_z", "ev_s"] {
            all_sts(&dbdir.join(sub).join("SETUP_SOFT"), &mut files_here);
        }
        for p in files_here {
            n += 1;
            let Ok(orig) = std::fs::read(&p) else { continue };
            // which of evalScene's saves: ev (-f), ev_m (-m), ev_z (-z), ev_s (-s, one a frame)
            let sub = p.strip_prefix(&dbdir).ok().and_then(|r| r.iter().next()).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
            match outs.iter().find(|(_, o)| *o == orig) {
                Some((how, _)) => tally.add(format!("identical ({how}), saved in {sub}"), &p),
                None => {
                    tally.add("different", &p);
                    // against the candidate that agrees longest
                    let agree = |o: &[u8]| orig.iter().zip(o).position(|(x, y)| x != y).unwrap_or(orig.len().min(o.len()));
                    let (how, best) = outs.iter().max_by_key(|(_, o)| agree(o)).unwrap();
                    let (a, b) = first_difference(&orig, best);
                    causes.add(format!("first differing line: original {a:?} / port {b:?} ({how})"), &p);
                    if let Some(d) = out_dir {
                        let rel = p.strip_prefix(saved).unwrap_or(&p).to_string_lossy().replace('/', "__");
                        std::fs::create_dir_all(d).unwrap();
                        std::fs::write(d.join(rel), best).unwrap();
                    }
                }
            }
        }
    }
    println!("== setup files evalScene saved, against the port's read and write of the sample it read: {n} files ==");
    tally.print("-- result", verbose);
    causes.print("-- differences, against the port's write that agrees longest", verbose);
}

/// The state `evalScene`'s setup reader starts from (read from `evalScene`'s code, not
/// guessed from its files). Its `main` (evalScene 0x080c2ef8) calls `SI_manipulationInit`,
/// `SI_cusInit`, `SI_initWave`, `SI_metaballInit`, `SI_matterInit` and `SI_colorInit`, but
/// not `SI_animationInfoInit`, `SI_duplicationInfoInit` or `SI_schematicInfoInit`, which the
/// interactive program's `systemInit` calls; `SI_setupFileLoadPath` calls the first two only
/// when `SI_edSumatraTo3x` is set, and it starts clear (0 in `.data`, both programs). Their
/// records `SI_glbAnim`, `SI_glbDuplic`, `SI_glbSchem` are in `.bss`, and so is
/// `SI_autosaveperiode`, which only the auto-save dialog sets: all zero. `SI_backupActive`
/// is 0 in `.data`, as in `soft`. Everything else as [`si_ascii::setup::Setup::default`].
fn evalscene_startup() -> si_ascii::setup::Setup {
    let mut s = si_ascii::setup::Setup::default();
    // the three fields of SI_glbAnim the setup file carries that SI_animationInfoInit sets
    // to something other than zero
    s.pref.confirm_keyedit = 0;
    s.pref.fcv_interp_type = 0;
    s.pref.dope_snap_keys = 0;
    s.duplicate.values = [0; 30];
    let c = &mut s.schematic;
    c.organization_type = 0;
    c.repartition_type = 0;
    c.rel_display = 0;
    c.sort_type = 0;
    c.prefix_option = 0;
    c.auto_clean = 0;
    c.recup_empties = 0;
    c.loading = 0;
    c.use_def_colors = 0;
    c.auto_scroll = 0;
    c.parallel_links = 0;
    c.update = 0;
    c.update_wanted = 0;
    c.grid_lock = 0;
    s.autosave.period = 0;
    s
}

fn all_sts(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut v = Vec::new();
    files(dir, &mut v);
    out.extend(v.into_iter().filter(|p| ext(p) == "sts"));
}

/// Every seventh prefix of every distinct setup file, and each prefix with its last byte
/// spoilt: the reader must come back with a result or with one of its errors.
fn stress_sts(all: &[PathBuf]) {
    use si_ascii::setup;
    let mut seen = std::collections::BTreeSet::new();
    let (mut runs, mut errors) = (0usize, BTreeMap::<String, usize>::new());
    for p in all.iter().filter(|p| ext(p) == "sts") {
        let Ok(data) = std::fs::read(p) else { continue };
        if !data.starts_with(b"SSET") || !seen.insert(data.clone()) {
            continue;
        }
        for n in (0..=data.len()).step_by(7) {
            runs += 1;
            if let Err(e) = setup::read(&data[..n]) {
                *errors.entry(e.to_string()).or_default() += 1;
            }
            if n > 8 {
                let mut spoilt = data[..n].to_vec();
                spoilt[n - 1] = b'#';
                runs += 1;
                if let Err(e) = setup::read(&spoilt) {
                    *errors.entry(e.to_string()).or_default() += 1;
                }
            }
        }
    }
    println!("== stress: {} distinct setup files, {runs} prefixes and spoilt prefixes read, 0 panics; stopped where the original would not return: {errors:?} ==", seen.len());
}

/// The relations resource file (tag `SREL`): read, and compared with a count of the file's
/// lines by first word; then the operator lines of the custom effect files (`.cus`), which the
/// same line readers take.
fn check_srel(all: &[PathBuf]) {
    use si_ascii::srel;
    let mut distinct: Vec<(Vec<u8>, Vec<&PathBuf>)> = Vec::new();
    for p in all {
        let Ok(mut file) = std::fs::File::open(p) else { continue };
        let mut tag = [0u8; 4];
        if std::io::Read::read(&mut file, &mut tag).unwrap_or(0) != 4 || &tag != b"SREL" {
            continue;
        }
        let Ok(data) = std::fs::read(p) else { continue };
        match distinct.iter_mut().find(|(d, _)| *d == data) {
            Some((_, v)) => v.push(p),
            None => distinct.push((data, vec![p])),
        }
    }
    println!(
        "== relations resources (SREL): {} files, {} distinct contents ==",
        distinct.iter().map(|d| d.1.len()).sum::<usize>(),
        distinct.len()
    );
    let mut relations = None;
    for (data, paths) in &distinct {
        let c = srel::check(data);
        for p in paths {
            println!("   {}", p.display());
        }
        for l in &c.lines {
            println!("{l}");
        }
        println!("comparisons failed: {}", c.failures);
        relations = relations.or(c.relations);
    }
    let Some(rel) = relations else { return };
    let mut seen = std::collections::BTreeSet::new();
    let (mut files, mut with_lines, mut lines, mut exact) = (0, 0, 0, 0);
    let (mut tally, mut odd) = (BTreeMap::new(), BTreeMap::new());
    for p in all.iter().filter(|p| ext(p) == "cus") {
        let Ok(data) = std::fs::read(p) else { continue };
        files += 1;
        if !seen.insert(data.clone()) {
            continue;
        }
        let c = srel::check_cus_lines(&rel, &data);
        with_lines += (c.lines > 0) as usize;
        lines += c.lines;
        exact += c.exact;
        for (k, n) in c.tally {
            *tally.entry(k).or_insert(0) += n;
        }
        for o in c.odd {
            *odd.entry(o).or_insert(0usize) += 1;
        }
    }
    println!("-- custom effect files (.cus): {files} files, {} distinct, {with_lines} with RES, NEWRES, ARG or OAT lines in an operator section", seen.len());
    println!("   {lines} such lines; the line reader takes exactly the line in {exact}");
    for (k, n) in &tally {
        println!("{n:6}  {k}");
    }
    for (k, n) in &odd {
        println!("   {n}x {k}");
    }
}

/// Channel setups (`SCHN`), flock setups (`SFLK`) and the driver configuration files (`.drv`,
/// no tag) that a channel setup's drivers are described by.
fn check_chn_flk(all: &[PathBuf], verbose: bool) {
    use si_ascii::{chn, flk};
    let (mut t, mut by_version) = (Tally::default(), Tally::default());
    let mut distinct = std::collections::BTreeSet::new();
    let mut total = 0;
    for p in all {
        let Ok(mut file) = std::fs::File::open(p) else { continue };
        let mut tag = [0u8; 4];
        if std::io::Read::read(&mut file, &mut tag).unwrap_or(0) != 4 || &tag != b"SCHN" {
            continue;
        }
        let Ok(data) = std::fs::read(p) else { continue };
        total += 1;
        distinct.insert(data.clone());
        let c = chn::check(&data);
        let Some(i) = &c.info else {
            t.add(format!("not read: {:?}", c.error), p);
            continue;
        };
        let layout = format!("CHANNELS {}", si_file::format_g(i.channels_version as f64));
        let complete = i.all_tokens_consumed && !i.read_past_end && i.overruns == 0 && i.ok && i.messages.is_empty();
        t.add(if complete { "read completely: nothing left but white space, no statement cut short, reader result 1, no message" } else { "read, but NOT completely" }, p);
        by_version.add(format!("{layout}: files"), p);
        if c.writable {
            t.add("can be written (no connection of the form older than 3.0)", p);
            if c.same_after_rewrite {
                t.add("  written and read again: same structure", p);
            }
            if c.identical {
                t.add("  written file identical byte for byte (own header version; driver data sections echoed as read)", p);
                by_version.add(format!("{layout}: identical"), p);
            }
            if c.identical_without_data {
                t.add("  identical also with empty data sections, as the original writes them for a driver that is not installed", p);
            }
            for cause in &c.causes {
                t.add(format!("  not identical: {cause}"), p);
            }
        } else {
            t.add(format!("cannot be written: holds connections of the older form ({layout})"), p);
            if c.same_without_legacy {
                t.add("  written without them and read again: the rest is the same", p);
            }
        }
    }
    println!("== channel setups (SCHN): {total} files, {} distinct contents ==", distinct.len());
    t.print("-- reading and writing", verbose);
    by_version.print("-- by layout version", false);

    let mut t = Tally::default();
    let (mut total, mut distinct) = (0, std::collections::BTreeSet::new());
    for p in all {
        let Ok(mut file) = std::fs::File::open(p) else { continue };
        let mut tag = [0u8; 4];
        if std::io::Read::read(&mut file, &mut tag).unwrap_or(0) != 4 || &tag != b"SFLK" {
            continue;
        }
        let Ok(data) = std::fs::read(p) else { continue };
        total += 1;
        distinct.insert(data.clone());
        let c = flk::check(&data);
        let i = &c.info;
        let complete = i.all_tokens_consumed && !i.read_past_end && i.overruns == 0 && i.messages.is_empty() && i.particle && i.end_word;
        t.add(if complete { "read completely, up to the word End, no message" } else { "read, but NOT completely" }, p);
        if c.same_after_rewrite {
            t.add("written and read again: same structure", p);
        }
        if c.identical {
            t.add("written file identical byte for byte (own header version)", p);
        }
    }
    println!("== flock setups (SFLK): {total} files, {} distinct contents ==", distinct.len());
    t.print("-- reading and writing", verbose);

    let mut t = Tally::default();
    let (mut total, mut distinct) = (0, std::collections::BTreeSet::new());
    for p in all.iter().filter(|p| ext(p) == "drv") {
        let Ok(data) = std::fs::read(p) else { continue };
        total += 1;
        distinct.insert(data.clone());
        let c = chn::check_description(&data);
        match (&c.error, &c.info) {
            (Some(e), _) => t.add(format!("not read: {e:?}"), p),
            (None, _) if c.counts == c.expected => t.add("read; channels, channel names, error count and error texts agree with a plain count of the file's lines", p),
            _ => t.add(format!("read, counts differ: {:?} against {:?}", c.counts, c.expected), p),
        }
    }
    println!("== driver configuration files (.drv, no tag; read only): {total} files, {} distinct contents ==", distinct.len());
    t.print("-- reading", verbose);
}

/// Every distinct scene description cut short at many places and with bytes overwritten: the
/// reader must come back (with a structure or with "the original would not return") without
/// panicking, and what it returns must write and read again.
fn stress_dsc(all: &[PathBuf]) {
    use si_ascii::dsc;
    let mut distinct = std::collections::BTreeSet::new();
    let (mut runs, mut ok, mut stuck) = (0usize, 0usize, 0usize);
    let mut state = 0x2545_f491u32;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as usize
    };
    for p in all.iter().filter(|p| ext(p) == "dsc") {
        let Ok(data) = std::fs::read(p) else { continue };
        if !data.starts_with(b"SDSC") || !distinct.insert(data.clone()) {
            continue;
        }
        let mut cases: Vec<Vec<u8>> = (0..40).map(|i| data[..data.len() * i / 40].to_vec()).collect();
        for _ in 0..40 {
            let mut m = data.clone();
            for _ in 0..3 {
                let at = next() % m.len();
                m[at] = [b' ', b';', b'\n', 0, 0xff, b'9', b'-', b'X'][next() % 8];
            }
            cases.push(m);
        }
        for c in &cases {
            runs += 1;
            match dsc::read(c) {
                Ok((d, _)) => {
                    ok += 1;
                    let _ = dsc::read(&d.write(d.version));
                }
                Err(_) => stuck += 1,
            }
        }
    }
    println!("== stress: {} distinct scene descriptions, {runs} damaged copies: {ok} read, {stuck} reported as \"the original would not return\", 0 panics ==", distinct.len());
}

/// Channel setups cut at every offset (every 7th above 8 KB), and channel setups, flock
/// setups and driver configuration files with bytes changed, removed, repeated and inserted:
/// the readers and writers must come back without panicking.
fn stress_chn(all: &[PathBuf]) {
    use si_ascii::{chn, flk};
    let tag = |d: &[u8]| -> [u8; 4] { d.get(..4).map_or([0; 4], |t| [t[0], t[1], t[2], t[3]]) };
    let mut seen = std::collections::BTreeSet::new();
    let mut total = [0usize; 5];
    let mut inputs: Vec<(Vec<u8>, u8)> = Vec::new();
    for p in all {
        let kind = match ext(p).as_str() {
            "chn" => b'c',
            "flk" => b'f',
            "drv" => b'd',
            _ => continue,
        };
        let Ok(data) = std::fs::read(p) else { continue };
        if data.is_empty() || !seen.insert(data.clone()) {
            continue;
        }
        if kind == b'c' && &tag(&data) == b"SCHN" {
            let n = chn::truncation_outcomes(&data, if data.len() > 8192 { 7 } else { 1 });
            for (t, x) in total.iter_mut().zip(n) {
                *t += x;
            }
        }
        if (kind == b'c' && &tag(&data) == b"SCHN") || (kind == b'f' && &tag(&data) == b"SFLK") || kind == b'd' {
            inputs.push((data, kind));
        }
    }
    println!(
        "== stress: channel setups cut short, {} prefixes: {} read clean, {} read with a statement cut short, {} refused, {} on which the original loops for ever, {} stopped for a huge driver ==",
        total.iter().sum::<usize>(), total[0], total[1], total[2], total[3], total[4]
    );
    let mut state = 0x2545_f491u32;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as usize
    };
    let (mut runs, mut ok) = (0usize, 0usize);
    for (data, kind) in &inputs {
        for _ in 0..300 {
            let mut m = data.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() % m.len().max(1);
                match next() % 5 {
                    0 => m[at] = (next() % 256) as u8,
                    1 => m.truncate(at),
                    2 => {
                        let end = (at + next() % 40).min(m.len());
                        m.drain(at..end);
                    }
                    3 => {
                        let w: &[u8] = [&b" ON "[..], b"\n", b" -", b" 999999999999 ", b"\"", b" DRIVER ", b" _END ", b"#", b" 1e", b" CALIBRATE\n"][next() % 10];
                        m.splice(at..at, w.iter().copied());
                    }
                    _ => {
                        let end = (at + next() % 200).min(m.len());
                        let piece = m[at..end].to_vec();
                        m.splice(at..at, piece);
                    }
                }
                if m.is_empty() {
                    break;
                }
            }
            runs += 1;
            ok += match kind {
                b'd' => chn::check_description(&m).error.is_none(),
                b'f' => flk::check(&m).same_after_rewrite,
                _ => chn::check(&m).error.is_none(),
            } as usize;
        }
    }
    println!("== stress: {} distinct channel setup, flock setup and driver files, {runs} damaged copies through the readers and writers: {ok} read, 0 panics ==", inputs.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args.get(1) else {
        eprintln!("usage: ascii-check <dir> [dsc|sts|chn|srel|stress|layouts] [-v] [--mr19] [--standalone]");
        std::process::exit(2);
    };
    let verbose = args.iter().any(|a| a == "-v");
    let only: Option<&str> = args.iter().skip(2).map(String::as_str).find(|a| !a.starts_with('-'));
    let mut all = Vec::new();
    files(Path::new(dir), &mut all);
    if only.is_none() || only == Some("dsc") {
        check_dsc(&all, verbose);
    }
    if only.is_none() || only == Some("sts") {
        check_sts(&all, args.iter().any(|a| a == "--mr19"), args.iter().any(|a| a == "--standalone"));
    }
    if only.is_none() || only == Some("chn") {
        check_chn_flk(&all, verbose);
    }
    if only.is_none() || only == Some("srel") {
        check_srel(&all);
    }
    if only == Some("saved-sts") {
        let out_dir = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map(PathBuf::from);
        let rest: Vec<&String> =
            args.iter().skip(3).filter(|a| !a.starts_with('-') && Some(PathBuf::from(a.as_str())) != out_dir).collect();
        if rest.len() < 2 {
            eprintln!("usage: ascii-check <evalScene saved dir> saved-sts <sample data1 dir> <scenes.tsv> [-v] [--out <dir>]");
            std::process::exit(2);
        }
        check_saved_sts(Path::new(dir), Path::new(rest[0]), Path::new(rest[1]), verbose, out_dir.as_deref());
        return;
    }
    if only == Some("stress") {
        stress_dsc(&all);
        stress_chn(&all);
        stress_sts(&all);
    }
    if only == Some("layouts") {
        // the relation-data tables, in the form re/tools/si-ascii/reldata_tables.py prints
        // what it extracts from the binary's reader and writer
        for relid in 0..300 {
            if let Some(l) = si_ascii::reldata::layout(relid) {
                let mut rows: Vec<String> = l
                    .keywords
                    .iter()
                    .map(|k| {
                        let f: Vec<String> = k.fields.iter().map(|(t, o)| format!("{t:?}@{o:#x}")).collect();
                        format!("{relid} {} {}", k.name, f.join(" "))
                    })
                    .collect();
                if !args.iter().any(|a| a == "order") {
                    rows.sort();
                }
                for r in rows {
                    println!("{r}");
                }
            }
        }
    }
}
