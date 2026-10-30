// SPDX-License-Identifier: MIT
//! Corpus check of the database layer (finding 0007): walks every database under a
//! directory and counts how the ported routines fare on its real names and files.
//!   db-check <dir> [-v]        -v lists every exception, not only the first few
//!
//! A database is found the way the program validates one: a directory with at least one
//! sub-directory holding a readable `Chapter.rsrc`.
use si_db::*;
use std::collections::{BTreeMap, BTreeSet};

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// A counted category with a few examples.
#[derive(Default)]
struct Tally {
    n: usize,
    examples: Vec<String>,
}

#[derive(Default)]
struct Report {
    verbose: bool,
    tallies: BTreeMap<String, Tally>,
}

impl Report {
    fn add(&mut self, key: &str, example: impl FnOnce() -> String) {
        let t = self.tallies.entry(key.to_string()).or_default();
        t.n += 1;
        if self.verbose || t.examples.len() < 4 {
            t.examples.push(example());
        }
    }

    fn count(&mut self, key: &str) {
        self.tallies.entry(key.to_string()).or_default().n += 1;
    }

    fn get(&self, key: &str) -> usize {
        self.tallies.get(key).map_or(0, |t| t.n)
    }

    fn print(&self, prefix: &str) {
        for (k, t) in self.tallies.iter().filter(|(k, _)| k.starts_with(prefix)) {
            println!("{:7}  {}", t.n, &k[prefix.len()..]);
            for e in &t.examples {
                println!("             {e}");
            }
        }
    }
}

/// The databases under `root`: directories that pass the program's own test.
fn find_databases(fs: &RealFs, lists: &ElemLists, root: &[u8], out: &mut Vec<Bytes>) {
    let mut info = DbInfo::new();
    info.path = Some(root.to_vec());
    if database_list_chapters(fs, lists, &mut info, true, &mut Log::default()) > 0 {
        out.push(root.to_vec());
        return;
    }
    let mut subs: Vec<Bytes> = fs.read_dir(root).unwrap_or_default();
    subs.sort();
    for name in subs {
        let p = [root, b"/", &name[..]].concat();
        if fs.stat(&p).is_some_and(|st| st.is_dir()) {
            find_databases(fs, lists, &p, out);
        }
    }
}

/// A reading of `stem.V-S.suffix` written for this check alone (digits only), to have a
/// second opinion beside the ported splitter.
fn plain_version(name: &[u8], suffix: &[u8]) -> Option<(Bytes, i64, i64)> {
    let body = name.strip_suffix(suffix)?.strip_suffix(b".")?;
    let dot = body.iter().rposition(|&c| c == b'.')?;
    let vs = &body[dot + 1..];
    let dash = vs.iter().position(|&c| c == b'-')?;
    let num = |t: &[u8]| -> Option<i64> {
        (!t.is_empty() && t.iter().all(u8::is_ascii_digit)).then(|| t.iter().fold(0i64, |a, &c| a * 10 + (c - b'0') as i64))
    };
    Some((body[..dot].to_vec(), num(&vs[..dash])?, num(&vs[dash + 1..])?))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = args.get(1) else {
        eprintln!("usage: db-check <dir> [-v]");
        std::process::exit(2);
    };
    let fs = RealFs;
    let mut r = Report { verbose: args.iter().any(|a| a == "-v"), ..Default::default() };
    let mut dbs = Vec::new();
    let lists = ElemLists::standard();
    find_databases(&fs, &lists, root.trim_end_matches('/').as_bytes(), &mut dbs);
    let session = Session { fs: &fs, now: 0, loading: false };
    let plan = creation_plan(b"x", b"/x");
    let plan_rsrc: BTreeMap<Bytes, Bytes> = plan
        .iter()
        .filter_map(|c| match c {
            Created::File { path, data } => Some((path[3..path.len() - b"/Chapter.rsrc".len()].to_vec(), data.clone())),
            _ => None,
        })
        .collect();
    let (mut files_total, mut groups_total) = (0usize, 0usize);
    let mut versions_census: BTreeMap<i32, usize> = BTreeMap::new();
    let mut layout: BTreeMap<String, usize> = BTreeMap::new();
    let mut max_version = 0i64;
    let mut walked = 0usize;

    for db in &dbs {
        let rel = s(&db[root.trim_end_matches('/').len()..]);
        let mut log = Log::default();
        let mut info = DbInfo::new();
        info.path = Some(db.clone());
        database_list_chapters(&fs, &lists, &mut info, false, &mut log);

        // --- 4: the layout against what creating a database makes
        let have: BTreeSet<Bytes> = info.chapters.iter().map(|c| c.name.clone()).collect();
        let missing: Vec<String> = plan_rsrc.keys().filter(|k| !have.contains(*k)).map(|k| s(k)).collect();
        let extra: Vec<String> = have.iter().filter(|k| !plan_rsrc.contains_key(*k)).map(|k| s(k)).collect();
        let key = format!(
            "{} of the 33 chapter directories; missing: {}; others: {}",
            33 - missing.len(),
            if missing.is_empty() { "none".into() } else { missing.join(" ") },
            if extra.is_empty() { "none".into() } else { extra.join(" ") }
        );
        *layout.entry(key).or_default() += 1;

        for chap in info.chapters.clone() {
            let cname = s(&chap.name);
            let table = CHAP_INFO.iter().find(|c| c.directory.as_bytes() == chap.name);
            // --- 3: chapters and suffixes against the table
            match table {
                None => r.add("3|chapter directory not in the table", || format!("{rel}/{cname}")),
                Some(_) => r.count("3|chapter directory named in the table"),
            }
            // --- 2: Chapter.rsrc
            let rsrc_path = chapter_rsrc_path(db, &chap.name);
            let data = fs.read(&rsrc_path);
            if data.is_none() {
                r.add("2|directory without a Chapter.rsrc", || format!("{rel}/{cname}"));
                if chap.suffix.is_none() {
                    // not a chapter of the table either: the program has no suffix for it
                    continue;
                }
            }
            let has_rsrc = data.is_some();
            let data = data.unwrap_or_default();
            let rewritten = write_chapter_rsrc(&chap);
            if !has_rsrc {
            } else if data == rewritten {
                r.count("2|Chapter.rsrc parses and is written back byte for byte");
            } else if data.is_empty() {
                r.add("2|Chapter.rsrc is empty (0 bytes)", || format!("{rel}/{cname}"));
            } else {
                // would the program at least read the same values from its own output?
                let mut again = DbChap { name: chap.name.clone(), suffix: None, browser: None, versions: 0 };
                chapter_read_info(&lists, &chap.name, &mut again, ChapterRsrc::Opened(&rewritten), b"", &mut log);
                let same = again == chap || (chap.browser.is_some() && again.suffix == chap.suffix && again.versions == chap.versions);
                let head = s(&data[..data.len().min(40)]).replace('\n', "\\n").replace('\r', "\\r");
                r.add(
                    if same { "2|Chapter.rsrc written back differently (same values)" } else { "2|Chapter.rsrc written back differently (values lost)" },
                    || format!("{rel}/{cname}: {} bytes, starts {head:?}", data.len()),
                );
            }
            if !data.is_empty() {
                *versions_census.entry(chap.versions).or_default() += 1;
                match (table, &chap.suffix) {
                    (Some(t), Some(sfx)) if t.suffix.as_bytes() == &sfx[..] => r.count("3|SUFFIX equals the table's"),
                    (Some(t), sfx) => {
                        r.add("3|SUFFIX differs from the table's", || format!("{rel}/{cname}: file {:?}, table {:?}", sfx.as_deref().map(s), t.suffix))
                    }
                    (None, _) => {}
                }
                if let Some(expect) = plan_rsrc.get(&chap.name) {
                    if &data == expect {
                        r.count("4|Chapter.rsrc identical to the one a new database gets");
                    } else {
                        r.add("4|Chapter.rsrc differs from the one a new database gets", || format!("{rel}/{cname}: {:?}", s(&data[..data.len().min(40)])));
                    }
                }
            }

            // --- 1: every file of the chapter, found with the program's own scan
            info.set_chapter(&chap.name);
            info.filepath_build();
            let scan = Scan {
                fs: &fs,
                chapter_path: info.chapter_path.as_deref().unwrap(),
                scan_rec: true,
                list_dirs: false,
                remove_suffix: false,
                expand_scanned: true,
                compare: Compare::Suffix(None),
                list_suffix: None,
                now: 0,
            };
            let mut all = Vec::new();
            scan.chapter_scan(b"", &mut all, &mut log);
            let suffix = chap.suffix.clone().unwrap_or_default();
            let mut groups: BTreeMap<Bytes, Vec<(Bytes, i64, i64)>> = BTreeMap::new();
            for f in &all {
                files_total += 1;
                let sub = f.node.sub_path.clone().unwrap_or_default();
                let own = f.suffix.clone().unwrap_or_default();
                let name: Bytes = f.node.full.clone().unwrap();
                let show = || format!("{rel}/{cname}/{}{}", s(&sub), s(&name));
                if sub.is_empty() && name == b"Chapter.rsrc" {
                    r.count("1|Chapter.rsrc itself");
                    continue;
                }
                if !file_compare_suffix(&name, Some(&suffix)) {
                    let shader = table
                        .and_then(|t| CHAP_INFO.iter().position(|c| c.directory == t.directory))
                        .and_then(|row| SHD_CHAP_INFO.iter().find(|x| x.chapter == row));
                    let what = match shader {
                        Some(x) if x.suffix.as_bytes() == &own[..] => "the chapter's suffix in the shader table".to_string(),
                        _ if own == b"icn" => "an icon file".to_string(),
                        _ => format!("in {cname}"),
                    };
                    r.add(&format!("1|not the chapter's suffix: .{} ({what})", s(&own)), show);
                    continue;
                }
                r.count("1a|files with their chapter's suffix");
                // the list node's name plus the suffix must be the file name again
                let node = scn_get_list_node(&name, None, true, 0);
                if [&node.full.clone().unwrap()[..], b".", &suffix[..]].concat() == name {
                    r.count("1a|  list node name + suffix gives the file name back");
                } else {
                    r.add("1a|  list node name + suffix does NOT give the file name back", show);
                }
                let parts = filename_expand(&name);
                if parts.suffix.is_empty() {
                    r.add(&format!("1c|no version in the name: {cname}"), show);
                    continue;
                }
                r.count("1b|names of the form [prefix-]name.version-substitution.suffix");
                if !sub.is_empty() {
                    r.count("1b|  of them in a sub-directory of the chapter");
                }
                if !parts.prefix.is_empty() {
                    r.count("1b|  of them with a prefix");
                }
                if parts.subst != 0 {
                    r.add("1b|  of them with a substitution number other than 0", show);
                }
                // the parts, joined as the program joins them, must be the name
                let raw = [
                    &format_prefix_name(&Some(parts.prefix.clone()), &Some(parts.name.clone()))[..],
                    b".",
                    parts.version.to_string().as_bytes(),
                    b"-",
                    parts.subst.to_string().as_bytes(),
                    b".",
                    &parts.suffix[..],
                ]
                .concat();
                if raw == name {
                    r.count("1b|  parts join back to the identical name");
                } else {
                    r.add("1b|  parts do NOT join back to the name", show);
                }
                // and so must the name the program itself builds, which first cleans the parts
                let mut n2 = node.clone();
                if Some(format_full_name(&mut n2, false)) == node.full {
                    r.count("1b|  name rebuilt by the program (parts cleaned first) is identical");
                } else {
                    r.add("1b|  name rebuilt by the program differs (a character it would not write)", show);
                }
                let mut n3 = node.clone();
                if Some(format_full_name(&mut n3, true)) == node.full {
                    r.count("1b|  name rebuilt by the program with the scene-loading flag set is identical");
                } else {
                    r.add("1b|  name rebuilt by the program with the scene-loading flag set differs", show);
                }
                // second opinion
                match plain_version(&name, &suffix) {
                    Some((stem, v, sb)) => {
                        let agree = stem == format_prefix_name(&Some(parts.prefix.clone()), &Some(parts.name.clone()))
                            && v == parts.version as i64
                            && sb == parts.subst as i64;
                        if !agree {
                            r.add("1b|  splitter and plain reading disagree", show);
                        }
                        max_version = max_version.max(v);
                        groups.entry(stem).or_default().push((sub.clone(), v, sb));
                    }
                    None => r.add("1b|  plain reading finds no version", show),
                }
            }

            // --- 5: latest and next version of every element with several files
            for (stem, files) in &groups {
                if files.len() < 2 {
                    r.count("5|elements with one file");
                    continue;
                }
                groups_total += 1;
                let show = || {
                    let mut vs: Vec<String> = files.iter().map(|(d, v, sb)| format!("{}{v}-{sb}", s(d))).collect();
                    vs.sort();
                    format!("{rel}/{cname}/{}: {}", s(stem), vs.join(" "))
                };
                let distinct: BTreeSet<i64> = files.iter().map(|f| f.1).collect();
                if distinct.len() < 2 {
                    r.add("5|  (several files but one version number: copies in sub-directories or substitutions)", show);
                }
                let top = *distinct.iter().max().unwrap();
                let probe = [&stem[..], b".1-0.", &suffix[..]].concat();
                let latest = file_find_latest_version(&session, &info, &probe, &mut log);
                let next = file_find_next_version(&session, &info, &probe, &mut log);
                let want = [&stem[..], format!(".{top}-0").as_bytes()].concat();
                let want_next = [&stem[..], format!(".{}-0", top + 1).as_bytes()].concat();
                if latest == want {
                    r.count("5|  latest version: the highest on disk");
                } else {
                    r.add("5|  latest version: NOT the highest on disk", || format!("{} -> {}", show(), s(&latest)));
                }
                if files.iter().any(|f| f.1 == top && f.2 == 0) {
                    r.count("5|  the file named by the answer exists");
                } else {
                    r.add("5|  the file named by the answer does not exist (highest version has another substitution)", show);
                }
                if next == want_next && !files.iter().any(|f| f.1 == top + 1) {
                    r.count("5|  next version: one above the highest, and free");
                } else {
                    r.add("5|  next version: wrong or taken", || format!("{} -> {}", show(), s(&next)));
                }
                // the node-based routine must agree on highest, lowest and count
                let mut node = scn_get_list_node(&probe, None, true, 0);
                let low = files.iter().map(|f| f.1).min().unwrap();
                let got = file_find_elem_latest_version(&session, &info, &mut node, false, &mut log);
                if got == (top as i32, low as i32, files.len() as i32) {
                    r.count("5|  highest, lowest and number of files agree");
                } else {
                    r.add("5|  highest, lowest or number of files disagree", || format!("{} -> {got:?}", show()));
                }
            }
        }
        // every file under the database, to account for those no chapter scan reaches
        let mut stack = vec![(db.clone(), None::<Bytes>, false)];
        while let Some((d, top, hidden)) = stack.pop() {
            for name in fs.read_dir(&d).unwrap_or_default() {
                let p = [&d[..], b"/", &name[..]].concat();
                let Some(st) = fs.stat(&p) else { continue };
                if st.is_dir() {
                    stack.push((p, top.clone().or(Some(name.clone())), hidden || name.first() == Some(&b'.')));
                    continue;
                }
                walked += 1;
                match &top {
                    None => r.add("6|files directly in the database directory (no chapter)", || format!("{rel}/{}", s(&name))),
                    Some(_) if hidden => r.add("6|files under a directory whose name starts with a dot (never scanned)", || s(&p)),
                    Some(t) if !fs.access(&chapter_rsrc_path(db, t), R_OK) && !CHAP_INFO.iter().any(|c| c.directory.as_bytes() == &t[..]) => {
                        r.add(&format!("6|files in a directory that is no chapter (no Chapter.rsrc, not in the table): {}", s(t)), || {
                            format!("{rel}/{}", s(&p[db.len() + 1..]))
                        })
                    }
                    Some(_) => {}
                }
            }
        }
        if !log.out.is_empty() || !log.err.is_empty() {
            r.add("0|databases where a routine printed a message", || format!("{rel}: {}{}", s(&log.out).trim_end(), s(&log.err).trim_end()));
        }
    }

    println!("{} databases under {root}: {walked} files, of which {files_total} are reached by the chapter scans", dbs.len());
    r.print("6|");
    println!();
    println!("== 1. File names: split and rebuilt");
    r.print("1|");
    r.print("1a|");
    r.print("1b|");
    println!("   names of their chapter's suffix that carry no version (the parts do not hold them; the list node keeps the whole name):");
    r.print("1c|");
    println!("\n== 2. Chapter.rsrc: parsed and written back");
    r.print("2|");
    println!("   VERSIONS values read:");
    for (v, n) in &versions_census {
        println!("{n:7}  VERSIONS {v}");
    }
    println!("\n== 3. Chapters and suffixes against the program's table");
    r.print("3|");
    println!("\n== 4. Layout against what creating a database makes (33 chapter directories, each with Chapter.rsrc)");
    for (k, n) in &layout {
        println!("{n:7}  databases with {k}");
    }
    r.print("4|");
    println!("\n== 5. Versions: {} elements with several files (highest version met: {max_version})", groups_total);
    r.print("5|");
    if r.get("0|databases where a routine printed a message") > 0 {
        println!("\n== Messages");
        r.print("0|");
    }
}
