// SPDX-License-Identifier: MIT
//! Scanning directories: which files a chapter holds, and which versions of an element.
//!
//! The original runs every scan through one recursive routine driven by ten globals
//! (`glbDBInfo`, `glbElemList`, `glbScanSubPath`, `glbScanRec`, `glbListDirs`,
//! `glbRemoveSuffix`, `glbExpandScanned`, `glbCompareFunc`, `glbCompareString`, `glbSubNb`).
//! Here they are the fields of [`Scan`].

use crate::{
    chapter_read_info, chapter_rsrc_path, check_file_existence, concat, dec, ed_get_suffix_from_chapter, file_get_version,
    filename_expand, filename_set_prefix, find, format_full_name, or_null, scan_int, scn_get_list_node, Bytes, ChapterRsrc,
    DbChap, DbInfo, ElemLists, FileSystem, ListNode, Log, X_OK,
};

/// Reads a directory, keeps the entries `select` accepts, and sorts them when asked
/// [soft 0x08120670 l_scandir]. `None` when the directory cannot be opened (−1 in the
/// original). The order is the C library's `alphasort`; the program runs in the "C"
/// locale, where that is plain byte order.
pub fn l_scandir(fs: &dyn FileSystem, path: &[u8], select: &mut dyn FnMut(&[u8]) -> bool, sort: bool) -> Option<Vec<Bytes>> {
    let mut kept: Vec<Bytes> = fs.read_dir(path)?.into_iter().filter(|e| select(e)).collect();
    if sort && kept.len() > 1 {
        kept.sort();
    }
    Some(kept)
}

/// Whether a file is a version of the element `prefixed` [soft 0x08122c38 dbfileComparePrefix].
///
/// The file name must start with `prefixed` (the element's `prefix-name`) followed by a
/// dot; a name ending in `.icn` never matches. With a substitution number given
/// (`glbSubNb`), the text between the first dash after that dot and the next dot (or the
/// end) must equal it; a name with no dash after the dot passes.
pub fn file_compare_prefix(file: &[u8], prefixed: &[u8], sub_nb: Option<&[u8]>) -> bool {
    if file.len() > 4 && file.ends_with(b".icn") {
        return false;
    }
    let at = |i: usize| file.get(i).copied().unwrap_or(0);
    let common = file.iter().zip(prefixed).take_while(|(a, b)| a == b).count();
    if common < 1 || common != prefixed.len() || at(common) != b'.' {
        return false;
    }
    let Some(sub) = sub_nb else { return true };
    let mut i = common + 1;
    loop {
        match at(i) {
            0 => return true,
            b'-' => break,
            _ => i += 1,
        }
    }
    let rest = &file[i + 1..];
    let end = rest.iter().position(|&c| c == b'.').unwrap_or(rest.len());
    &rest[..end] == sub
}

/// Whether a file has the suffix [soft 0x08122dbc dbfileCompareSuffix]: every file when the
/// suffix is null or `all`, else the name must end in a dot and the suffix. (A name that
/// *is* the suffix makes the original look at the byte before the name, which in a
/// directory entry is the entry's type and never a dot: no match.)
pub fn file_compare_suffix(file: &[u8], suffix: Option<&[u8]>) -> bool {
    match suffix {
        None | Some(b"all") => true,
        Some(s) => file.len() > s.len() && file.ends_with(s) && file[file.len() - s.len() - 1] == b'.',
    }
}

/// The test a scan applies to plain files.
#[derive(Clone, Copy, Debug)]
pub enum Compare<'a> {
    /// No test installed: no plain file is selected.
    None,
    /// [`file_compare_prefix`]
    Prefix { name: &'a [u8], sub_nb: Option<&'a [u8]> },
    /// [`file_compare_suffix`]
    Suffix(Option<&'a [u8]>),
    /// The file may be executed [soft 0x08122ebc dbfileCompareExecAccess].
    ExecAccess,
    /// The file may be executed and its name does not end in `.cus`
    /// [soft 0x08122ee0 dbfileCompareChnlDriver].
    ChnlDriver,
}

/// One file (or directory) a scan found.
#[derive(Clone, Debug, PartialEq)]
pub struct Scanned {
    pub node: ListNode,
    /// The text after the file name's last dot, recorded only when the list scanned into
    /// has no suffix of its own or the suffix `all` (the element's field at offset 0x80).
    pub suffix: Option<Bytes>,
    /// The file is under a directory whose path contains `SHARE` (field at offset 0x44).
    pub share: bool,
}

/// The settings of a scan.
#[derive(Clone, Copy)]
pub struct Scan<'a> {
    pub fs: &'a dyn FileSystem,
    /// The directory scanned, with its final slash (`glbDBInfo`'s chapter directory).
    pub chapter_path: &'a [u8],
    /// Go down into sub-directories.
    pub scan_rec: bool,
    /// List sub-directories as if they were files (and do not go down into them).
    pub list_dirs: bool,
    /// Store each file's name without its suffix.
    pub remove_suffix: bool,
    /// Split each file name into its parts.
    pub expand_scanned: bool,
    pub compare: Compare<'a>,
    /// The suffix of the list scanned into (`glbElemList`, offset 0x3c).
    pub list_suffix: Option<&'a [u8]>,
    /// What `time(NULL)` returns, for the nodes made.
    pub now: i32,
}

impl<'a> Scan<'a> {
    /// The filter of every scan [soft 0x08122f3c dbfileSelect]. A directory is taken when
    /// the scan goes down into directories or lists them, unless its name starts with a
    /// dot. Anything else, including an entry that cannot be examined, is up to the test
    /// installed.
    fn select(&self, sub_path: &[u8], name: &[u8]) -> bool {
        let path = concat(&[self.chapter_path, sub_path, name]);
        if self.fs.stat(&path).is_some_and(|st| st.is_dir()) {
            return (self.scan_rec || self.list_dirs) && name.first() != Some(&b'.');
        }
        match self.compare {
            Compare::None => false,
            Compare::Prefix { name: prefixed, sub_nb } => file_compare_prefix(name, prefixed, sub_nb),
            Compare::Suffix(suffix) => file_compare_suffix(name, suffix),
            Compare::ExecAccess => self.fs.access(&path, X_OK),
            Compare::ChnlDriver => {
                self.fs.access(&path, X_OK)
                    && !path.iter().rposition(|&c| c == b'.').is_some_and(|p| &path[p..] == b".cus")
            }
        }
    }

    fn entries(&self, sub_path: &[u8]) -> Vec<Bytes> {
        let dir = concat(&[self.chapter_path, sub_path]);
        l_scandir(self.fs, &dir, &mut |name| self.select(sub_path, name), true).unwrap_or_default()
    }

    /// Lists the files of the directory `chapter_path + sub_path` into `out`
    /// [soft 0x081230cc dbChapterScan].
    ///
    /// Entries are taken in byte order. A sub-directory is scanned in place, at the point
    /// its name comes up, with `sub_path` grown by `name/`; with `list_dirs` it is listed
    /// like a file instead. Each file becomes a list node: with `expand_scanned`, through
    /// [`scn_get_list_node`], with the file's time, mode and size; without, only the file
    /// name and, as version, the number after the name's first dot.
    ///
    /// Where this differs: without `expand_scanned`, a name that has no number after its
    /// first dot gets a version the original never computes (it prints an unset variable);
    /// here the version is then null.
    pub fn chapter_scan(&self, sub_path: &[u8], out: &mut Vec<Scanned>, log: &mut Log) {
        for name in self.entries(sub_path) {
            let path = concat(&[self.chapter_path, sub_path, &name]);
            let Some(st) = self.fs.stat(&path) else {
                log.err.extend_from_slice(&concat(&[b"Invalid file status for [", &path, b"]\n"]));
                continue;
            };
            if st.is_dir() && !self.list_dirs {
                if self.scan_rec {
                    self.chapter_scan(&concat(&[sub_path, &name, b"/"]), out, log);
                }
                continue;
            }
            let suffix = match self.list_suffix {
                None | Some(b"all") => name.iter().rposition(|&c| c == b'.').map(|p| name[p + 1..].to_vec()),
                _ => None,
            };
            let mut node;
            if self.expand_scanned {
                node = scn_get_list_node(&name, None, self.remove_suffix, self.now);
                node.time = st.mtime;
                node.mode = st.mode as u16;
                node.size = st.size;
            } else {
                node = ListNode::allocate(self.now);
                node.full = Some(name.clone());
                node.version = file_get_version(&name).map(dec);
            }
            node.sub_path = Some(sub_path.to_vec());
            out.push(Scanned { node, suffix, share: find(sub_path, b"SHARE").is_some() });
        }
    }

    /// Collects the sub-directories under `chapter_path + sub_path` as paths relative to
    /// the chapter (`dir/`, `dir/inner/`), a directory before its own sub-directories, and
    /// returns how many entries the filter selected in all [soft 0x08123408
    /// dbChapterScanForSubDir].
    pub fn scan_for_sub_dir(&self, sub_path: &[u8], out: &mut Vec<Bytes>) -> i32 {
        let entries = self.entries(sub_path);
        let mut below = 0;
        for name in &entries {
            let path = concat(&[self.chapter_path, sub_path, name]);
            if !self.fs.stat(&path).is_some_and(|st| st.is_dir()) {
                continue;
            }
            let inner = concat(&[sub_path, name, b"/"]);
            out.push(inner.clone());
            if self.scan_rec {
                below += self.scan_for_sub_dir(&inner, out);
            }
        }
        entries.len() as i32 + below
    }

    /// Counts what the filter selects under `chapter_path + sub_path`, sub-directories
    /// always included [soft 0x08123668 dbChapterScanForChapCount]. The directories count
    /// too, each as one.
    pub fn scan_for_chap_count(&self, sub_path: &[u8]) -> i32 {
        let entries = self.entries(sub_path);
        let mut below = 0;
        for name in &entries {
            let path = concat(&[self.chapter_path, sub_path, name]);
            if self.fs.stat(&path).is_some_and(|st| st.is_dir()) {
                below += self.scan_for_chap_count(&concat(&[sub_path, name, b"/"]));
            }
        }
        entries.len() as i32 + below
    }
}

/// What the version routines need besides the database.
#[derive(Clone, Copy)]
pub struct Session<'a> {
    pub fs: &'a dyn FileSystem,
    /// What `time(NULL)` returns.
    pub now: i32,
    /// The "a scene is being loaded" flag of [`crate::name_check_validity`].
    pub loading: bool,
}

fn chapter_dir(info: &DbInfo) -> &[u8] {
    info.chapter_path.as_deref().unwrap_or(b"")
}

/// The scan all version routines share: every file under the database's current chapter
/// directory, sub-directories included, that is a version of the element `prefixed`.
fn versions_scan(s: &Session, info: &DbInfo, prefixed: &[u8], sub_nb: Option<&[u8]>, log: &mut Log) -> Vec<Scanned> {
    let scan = Scan {
        fs: s.fs,
        chapter_path: chapter_dir(info),
        scan_rec: true,
        list_dirs: false,
        remove_suffix: true,
        expand_scanned: true,
        compare: Compare::Prefix { name: prefixed, sub_nb },
        list_suffix: None,
        now: s.now,
    };
    let mut out = Vec::new();
    scan.chapter_scan(b"", &mut out, log);
    out
}

/// `sscanf(node.version, "%d", &v)`: `v` keeps its value when nothing converts.
fn scan_version(node: &ListNode, v: &mut i32) {
    if let Some((n, _)) = scan_int(or_null(&node.version)) {
        *v = n;
    }
}

fn latest_or_next(s: &Session, info: &DbInfo, file_name: &[u8], step: i32, log: &mut Log) -> Bytes {
    let parts = filename_expand(file_name);
    let mut node = ListNode::allocate(s.now);
    node.prefix = Some(parts.prefix);
    node.name = Some(parts.name);
    node.full = Some(format_full_name(&mut node, s.loading));
    let prefixed = filename_set_prefix(&mut node, s.loading);
    let found = versions_scan(s, info, &prefixed, None, log);
    let mut latest = 0;
    // one variable receives every conversion; the original leaves it unset before the first
    let mut v = 0;
    for f in &found {
        scan_version(&f.node, &mut v);
        latest = latest.max(v);
    }
    node.version = Some(dec(latest.wrapping_add(step)));
    format_full_name(&mut node, s.loading)
}

/// The name, without suffix, of the latest version of the element a file name belongs to
/// [soft 0x08125cf4 SI_dbfileFindLatestVersion].
///
/// Only prefix and element name of `file_name` are used. The current chapter directory of
/// `info` is searched with its sub-directories for files `prefix-name.<anything>`; the
/// highest version found goes into the result, **with substitution 0 whatever the files
/// have**. With no file at all the result names version 0.
pub fn file_find_latest_version(s: &Session, info: &DbInfo, file_name: &[u8], log: &mut Log) -> Bytes {
    latest_or_next(s, info, file_name, 0, log)
}

/// The same with the highest version plus one: the name to save the next version under
/// [soft 0x08125ec4 SI_dbfileFindNextVersion].
pub fn file_find_next_version(s: &Session, info: &DbInfo, file_name: &[u8], log: &mut Log) -> Bytes {
    latest_or_next(s, info, file_name, 1, log)
}

/// The versions of a list node's element on disk [soft 0x08126188
/// SI_dbfileFindElemLatestVersion]: returns the highest version (−1 with no file), the
/// lowest (99999 with no file) and the number of files.
///
/// With `update`, a node whose own version differs from the highest is moved to it: its
/// version text and its full name are rewritten, the substitution number staying as it is.
///
/// Kept oddity: the node's own version is read into the variable that last held a file's
/// version, so a node whose version text is not a number is compared as if it had the
/// version of the last file scanned.
pub fn file_find_elem_latest_version(s: &Session, info: &DbInfo, node: &mut ListNode, update: bool, log: &mut Log) -> (i32, i32, i32) {
    let prefixed = filename_set_prefix(node, s.loading);
    let found = versions_scan(s, info, &prefixed, None, log);
    let (mut latest, mut lowest) = (-1, 99999);
    // unset in the original until the first conversion
    let mut v = 0;
    for f in &found {
        scan_version(&f.node, &mut v);
        latest = latest.max(v);
        lowest = lowest.min(v);
    }
    scan_version(node, &mut v);
    if update && latest > -1 && latest != v {
        node.version = Some(dec(latest));
        node.full = Some(format_full_name(node, s.loading));
    }
    (latest, lowest, found.len() as i32)
}

/// The highest version among the files of a node's element **that carry the node's
/// substitution number**, −9999 when there is none [soft 0x08125be8
/// si_dbfileFindSubstitutionFile]. The node's prefix and name are cleaned on the way.
pub fn file_find_substitution_file(s: &Session, info: &DbInfo, node: &mut ListNode, log: &mut Log) -> i32 {
    let prefixed = filename_set_prefix(node, s.loading);
    let sub = node.subst.clone();
    let found = versions_scan(s, info, &prefixed, sub.as_deref(), log);
    let mut latest = -9999;
    let mut v = 0;
    for f in &found {
        scan_version(&f.node, &mut v);
        latest = latest.max(v);
    }
    latest
}

/// The first half of `si_dbfileCheckExistence` (0x0812580c): the version number the next
/// save of an element gets, which is the highest version on disk plus one, counting only
/// files with the substitution number `sub_nb` when one is given. With no file the
/// original's start value shows through: −9998.
///
/// The second half is not ported: when the chapter already holds as many versions as its
/// `VERSIONS` number allows, the files are sorted by version and the lowest
/// `count − VERSIONS + 1` are deleted, each unless a scene still refers to that version.
/// That test reads scene files (`SI_dbfileCheckElement`), which this crate does not do.
pub fn next_version_number(s: &Session, info: &DbInfo, prefixed: &[u8], sub_nb: Option<&[u8]>, log: &mut Log) -> i32 {
    let found = versions_scan(s, info, prefixed, sub_nb, log);
    let mut latest = -9999;
    let mut v = 0;
    for f in &found {
        scan_version(&f.node, &mut v);
        latest = latest.max(v);
    }
    latest + 1
}

/// Lists the elements of a chapter [soft 0x08125378 SI_dbChapterListElements]: the files
/// under the current chapter directory of `info` that end in `.suffix`.
///
/// `expand` asks for names split into parts and stored without suffix. With `recursive`
/// the whole tree below the chapter is listed; without, only the directory
/// `list_sub_path` inside it (the chapter itself when that is null). `list_suffix` is the
/// suffix field of the element list being filled: it is replaced by `suffix` when one is
/// given, and it decides whether each file's own suffix is recorded (see [`Scanned`]).
pub fn chapter_list_elements(
    s: &Session,
    info: &DbInfo,
    suffix: Option<&[u8]>,
    expand: bool,
    recursive: bool,
    list_sub_path: Option<&[u8]>,
    list_suffix: &mut Option<Bytes>,
    log: &mut Log,
) -> Vec<Scanned> {
    let sub: &[u8] = if recursive { b"" } else { list_sub_path.unwrap_or(b"") };
    if let Some(sfx) = suffix {
        *list_suffix = Some(sfx.to_vec());
    }
    let scan = Scan {
        fs: s.fs,
        chapter_path: chapter_dir(info),
        scan_rec: recursive,
        list_dirs: false,
        remove_suffix: expand,
        expand_scanned: expand,
        compare: Compare::Suffix(suffix),
        list_suffix: list_suffix.as_deref(),
        now: s.now,
    };
    let mut out = Vec::new();
    scan.chapter_scan(sub, &mut out, log);
    out
}

/// Kinds of listing for [`directory_list_elements`].
pub const LIST_BY_SUFFIX: i32 = 3;
pub const LIST_EXECUTABLES: i32 = 4;
pub const LIST_CHANNEL_DRIVERS: i32 = 5;

/// Lists one directory that need not belong to a database [soft 0x0812547c
/// SI_dbDirectoryListElements]: its sub-directories (listed, not entered) and the files the
/// test for `how` selects, names split into parts and kept whole.
///
/// `directory` is used as given and the entry names are appended to it directly, so it
/// must end in a slash. For a `how` other than the three known values the original leaves
/// the test of the previous scan installed; that is the `previous` argument here.
pub fn directory_list_elements<'a>(
    s: &Session,
    directory: &[u8],
    suffix: Option<&'a [u8]>,
    how: i32,
    previous: Compare<'a>,
    list_suffix: &mut Option<Bytes>,
    log: &mut Log,
) -> Vec<Scanned> {
    let compare = match how {
        LIST_BY_SUFFIX => Compare::Suffix(suffix),
        LIST_EXECUTABLES => Compare::ExecAccess,
        LIST_CHANNEL_DRIVERS => Compare::ChnlDriver,
        _ => previous,
    };
    if let Some(sfx) = suffix {
        *list_suffix = Some(sfx.to_vec());
    }
    let scan = Scan {
        fs: s.fs,
        chapter_path: directory,
        scan_rec: false,
        list_dirs: true,
        remove_suffix: false,
        expand_scanned: true,
        compare,
        list_suffix: list_suffix.as_deref(),
        now: s.now,
    };
    let mut out = Vec::new();
    scan.chapter_scan(b"", &mut out, log);
    out
}

/// The sub-directories of the current chapter [soft 0x0812508c SI_dbChapterGetSubDirs]:
/// their paths relative to the chapter, and the count the scan returns. Without
/// `recursive` only the directories directly inside `list_sub_path` are given.
pub fn chapter_get_sub_dirs(s: &Session, info: &DbInfo, recursive: bool, list_sub_path: Option<&[u8]>) -> (Vec<Bytes>, i32) {
    let sub: &[u8] = if recursive { b"" } else { list_sub_path.unwrap_or(b"") };
    let scan = Scan {
        fs: s.fs,
        chapter_path: chapter_dir(info),
        scan_rec: recursive,
        list_dirs: true,
        remove_suffix: false,
        expand_scanned: true,
        compare: Compare::None,
        list_suffix: None,
        now: s.now,
    };
    let mut dirs = Vec::new();
    let n = scan.scan_for_sub_dir(sub, &mut dirs);
    (dirs, n)
}

/// How many entries a chapter holds [soft 0x081251b4 SI_dbChapterGetNbFiles]: the files
/// with the chapter's suffix in its whole tree, **plus one for each sub-directory**. The
/// chapter becomes the current chapter of `info`.
pub fn chapter_get_nb_files(s: &Session, info: &mut DbInfo, chap: &DbChap) -> i32 {
    info.set_chapter(&chap.name);
    info.filepath_build();
    let scan = Scan {
        fs: s.fs,
        chapter_path: chapter_dir(info),
        scan_rec: true,
        list_dirs: false,
        remove_suffix: false,
        expand_scanned: false,
        compare: Compare::Suffix(chap.suffix.as_deref()),
        list_suffix: None,
        now: s.now,
    };
    scan.scan_for_chap_count(b"")
}

/// Whether no chapter of the database holds anything [soft 0x0812711c SI_dbDatabaseIsEmpty].
pub fn database_is_empty(s: &Session, info: &mut DbInfo) -> bool {
    for i in 0..info.chapters.len() {
        let chap = info.chapters[i].clone();
        if chapter_get_nb_files(s, info, &chap) > 0 {
            return false;
        }
    }
    true
}

/// Finds the chapters of a database: every sub-directory of its path whose name does not
/// start with a dot [soft 0x08125628 SI_dbDatabaseListChapters] [soft 0x0812301c
/// dbDatabaseSelect]. Each becomes a chapter record filled by [`chapter_read_info`]; the
/// records are then sorted by name. Returns how many chapters have a `Chapter.rsrc`
/// (opened, or with `check_only` merely readable).
///
/// Kept oddity: the records filled are those at positions 0, 1, 2… of the database's
/// chapter array, which are the new ones only when the array was empty before the call.
pub fn database_list_chapters(fs: &dyn FileSystem, lists: &ElemLists, info: &mut DbInfo, check_only: bool, log: &mut Log) -> i32 {
    let db_path = info.path.clone().unwrap_or_default();
    let mut select = |name: &[u8]| {
        fs.stat(&concat(&[&db_path, b"/", name])).is_some_and(|st| st.is_dir()) && name.first() != Some(&b'.')
    };
    let dirs = l_scandir(fs, &db_path, &mut select, false).unwrap_or_default();
    let mut with_rsrc = 0;
    for (i, name) in dirs.iter().enumerate() {
        info.add_chapter(name);
        let path = chapter_rsrc_path(&db_path, name);
        let data;
        let file = if check_only {
            ChapterRsrc::CheckOnly(check_file_existence(fs, &path))
        } else {
            data = fs.read(&path);
            data.as_deref().map_or(ChapterRsrc::Missing, ChapterRsrc::Opened)
        };
        if chapter_read_info(lists, name, &mut info.chapters[i], file, &path, log) {
            with_rsrc += 1;
        }
    }
    info.sort_chapters();
    with_rsrc
}

/// Whether a directory is, or lies inside, a database [soft 0x0812573c
/// SI_dbCheckDatabaseExistence]: it holds at least two readable entries named like
/// chapters, or its own name is a chapter's and its parent passes the same test.
/// `lists` is the dictionary whose list names count as chapter names.
/// (On the second way of succeeding the original leaves the caller's string cut at the
/// last slash.)
pub fn check_database_existence(fs: &dyn FileSystem, path: &[u8], lists: &ElemLists) -> bool {
    let mut found = 0;
    for l in &lists.lists {
        if check_file_existence(fs, &concat(&[path, b"/", &l.name])) {
            found += 1;
        }
        if found > 1 {
            return true;
        }
    }
    if let Some(p) = path.iter().rposition(|&c| c == b'/') {
        for l in &lists.lists {
            if path[p + 1..] == l.name[..] && check_database_existence(fs, &path[..p], lists) {
                return true;
            }
        }
    }
    false
}

/// The scene saved last [soft 0x08126094 SI_dbGetLatestScene]: the file of the `SCENES`
/// chapter (sub-directories included) with the latest modification time, as its name
/// without suffix; the first in listing order among equals. `SCENES` becomes the current
/// chapter of `info`.
pub fn get_latest_scene(s: &Session, info: &mut DbInfo, log: &mut Log) -> Option<Bytes> {
    let suffix = ed_get_suffix_from_chapter(crate::chap::SCENES).as_bytes();
    info.set_chapter(b"SCENES");
    info.filepath_build();
    let mut list_suffix = Some(suffix.to_vec());
    let found = chapter_list_elements(s, info, Some(suffix), true, true, None, &mut list_suffix, log);
    let mut best: Option<&ListNode> = None;
    for f in &found {
        if best.map_or(true, |b| b.time < f.node.time) {
            best = Some(&f.node);
        }
    }
    best.map(|n| or_null(&n.full).to_vec())
}

/// A copy of a path without its suffix [soft 0x083cefd4 SI_strStripSuffix]: cut at the last
/// dot unless a slash or a dash follows it.
pub fn str_strip_suffix(path: &[u8]) -> Bytes {
    match path.iter().rposition(|&c| c == b'.') {
        Some(p) if !path[p + 1..].iter().any(|&c| c == b'/' || c == b'-') => path[..p].to_vec(),
        _ => path.to_vec(),
    }
}

/// Where [`file_find_in_chapter`] found a file.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundFile {
    /// The path without suffix (the global `SI_dBWorkFname`).
    pub path: Bytes,
    /// The path with suffix (`SI_dBWorkFnameExt`).
    pub path_ext: Bytes,
    /// The sub-directory of the list it was found in, when it was found by trying those.
    pub sub_dir: Option<Bytes>,
    /// The element's "shared" flag should be set: the file is in a directory `SHARE…`.
    pub share: bool,
}

/// Finds the file of an element [soft 0x08126420 SI_dbFileFindInChapter].
///
/// `chapter` and `chapter_id` name the chapter (the list's name and the id its suffix is
/// looked up by), `sub_dirs` are the sub-directories recorded in the list. The places are
/// tried in this order, the first readable file winning:
///
/// 1. If the node's sub-directory starts with `/`, `~` or `$`, it is a directory outside
///    the database: only `<that>/<full name>` is tried, as it stands (no suffix added and
///    nothing expanded).
/// 2. `<chapter dir><node's sub-directory><full name>.<suffix>`.
/// 3. `<chapter dir><full name>.<suffix>`; on success the node's sub-directory becomes empty.
/// 4. Each recorded sub-directory in turn.
/// 5. Everything again from 2 with the unversioned name (`prefix-name`) in place of the
///    full name. The node keeps its full name.
///
/// `None` when nothing is found. `chapter` becomes the current chapter of `info`.
pub fn file_find_in_chapter(
    fs: &dyn FileSystem,
    info: &mut DbInfo,
    chapter: &[u8],
    chapter_id: i32,
    sub_dirs: &[Bytes],
    node: &mut ListNode,
) -> Option<FoundFile> {
    find_in_chapter(fs, info, chapter, chapter_id, sub_dirs, node, None)
}

fn find_in_chapter(
    fs: &dyn FileSystem,
    info: &mut DbInfo,
    chapter: &[u8],
    chapter_id: i32,
    sub_dirs: &[Bytes],
    node: &mut ListNode,
    unversioned: Option<&[u8]>,
) -> Option<FoundFile> {
    let full = unversioned.map_or_else(|| or_null(&node.full).to_vec(), |u| u.to_vec());
    if let Some(sub) = node.sub_path.as_deref() {
        if matches!(sub.first(), Some(b'/' | b'~' | b'$')) {
            let path_ext = concat(&[sub, b"/", &full]);
            let path = str_strip_suffix(&path_ext);
            return check_file_existence(fs, &path_ext).then_some(FoundFile { path, path_ext, sub_dir: None, share: false });
        }
    }
    info.set_chapter(chapter);
    info.filepath_build();
    let dir = chapter_dir(info).to_vec();
    let suffix = ed_get_suffix_from_chapter(chapter_id).as_bytes();
    let sub = or_null(&node.sub_path).to_vec();
    let attempt = |place: &[u8]| {
        let path = concat(&[&dir, place, &full]);
        let path_ext = concat(&[&path, b".", suffix]);
        check_file_existence(fs, &path_ext).then_some((path, path_ext))
    };
    if let Some((path, path_ext)) = attempt(&sub) {
        return Some(FoundFile { path, path_ext, sub_dir: None, share: sub.starts_with(b"SHARE") });
    }
    if let Some((path, path_ext)) = attempt(b"") {
        node.sub_path = Some(Vec::new());
        return Some(FoundFile { path, path_ext, sub_dir: None, share: false });
    }
    for d in sub_dirs {
        if let Some((path, path_ext)) = attempt(d) {
            return Some(FoundFile { path, path_ext, sub_dir: Some(d.clone()), share: d.starts_with(b"SHARE") });
        }
    }
    if unversioned.is_some() {
        return None;
    }
    let plain = match &node.prefix {
        None => or_null(&node.name).to_vec(),
        Some(p) => concat(&[p, b"-", or_null(&node.name)]),
    };
    find_in_chapter(fs, info, chapter, chapter_id, sub_dirs, node, Some(&plain))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemFs;

    fn fs() -> MemFs {
        let mut fs = MemFs::new();
        for f in [
            "Bike-A_bike.3-0.hrc",
            "Bike-A_bike.1-0.hrc",
            "Bike-A_bike.2-1.hrc",
            "Bike-A_bike.icn",
            "Bike-A_bikelane.9-0.hrc",
            "Chapter.rsrc",
            "cube.1-0.hrc",
            "notes.txt",
            ".hidden/cube.7-0.hrc",
            "old/Bike-A_bike.10-0.hrc",
            "old/SHARE_lib/cube.4-2.hrc",
        ] {
            fs.add_file_with(format!("/db/MODELS/{f}").as_bytes(), b"data", f.len() as i32, 0o644);
        }
        fs
    }

    fn info() -> DbInfo {
        let mut i = DbInfo::new();
        i.path = Some(b"/db".to_vec());
        i.set_chapter(b"MODELS");
        i.filepath_build();
        i
    }

    fn names(v: &[Scanned]) -> Vec<String> {
        v.iter()
            .map(|f| format!("{}{}", String::from_utf8_lossy(or_null(&f.node.sub_path)), String::from_utf8_lossy(or_null(&f.node.full))))
            .collect()
    }

    #[test]
    fn compare_functions() {
        let p = |f: &str, n: &str, s: Option<&str>| file_compare_prefix(f.as_bytes(), n.as_bytes(), s.map(|x| x.as_bytes()));
        assert!(p("Bike-A_bike.3-0.hrc", "Bike-A_bike", None));
        assert!(!p("Bike-A_bikelane.3-0.hrc", "Bike-A_bike", None));
        assert!(!p("Bike-A_bike", "Bike-A_bike", None));
        assert!(!p("Bike-A_bike.icn", "Bike-A_bike", None));
        assert!(!p(".icn", "", None));
        assert!(!p(".1-0", "", None)); // an empty element name never matches
        assert!(p("Bike-A_bike.3-0.hrc", "Bike-A_bike", Some("0")));
        assert!(!p("Bike-A_bike.3-0.hrc", "Bike-A_bike", Some("1")));
        assert!(!p("Bike-A_bike.3-10.hrc", "Bike-A_bike", Some("1")));
        assert!(!p("Bike-A_bike.3-1.hrc", "Bike-A_bike", Some("10")));
        assert!(p("Bike-A_bike.3-10", "Bike-A_bike", Some("10")));
        assert!(p("walk.12.pic", "walk", Some("5"))); // no dash after the dot: passes
        assert!(p("Bike-A_bike.3--5.hrc", "Bike-A_bike", Some("-5")));
        let s = |f: &str, x: Option<&str>| file_compare_suffix(f.as_bytes(), x.map(|x| x.as_bytes()));
        assert!(s("a.hrc", Some("hrc")));
        assert!(!s("a.hrcx", Some("hrc")));
        assert!(!s("ahrc", Some("hrc")));
        assert!(!s("hrc", Some("hrc")));
        assert!(s(".hrc", Some("hrc")));
        assert!(s("anything", None));
        assert!(s("anything", Some("all")));
        assert!(s("a.", Some("")));
    }

    #[test]
    fn scan_order_and_recursion() {
        let fs = fs();
        let s = Session { fs: &fs, now: 99, loading: false };
        let mut log = Log::default();
        let mut sfx = None;
        let all = chapter_list_elements(&s, &info(), Some(b"hrc"), true, true, None, &mut sfx, &mut log);
        // byte order, a directory's files where its name sorts; dot directories skipped
        assert_eq!(
            names(&all),
            [
                "Bike-A_bike.1-0",
                "Bike-A_bike.2-1",
                "Bike-A_bike.3-0",
                "Bike-A_bikelane.9-0",
                "cube.1-0",
                "old/Bike-A_bike.10-0",
                "old/SHARE_lib/cube.4-2"
            ]
        );
        assert_eq!(sfx.as_deref(), Some(&b"hrc"[..]));
        assert!(all.iter().all(|f| f.suffix.is_none()));
        assert_eq!(all.iter().map(|f| f.share).collect::<Vec<_>>(), [false, false, false, false, false, false, true]);
        let n = &all[1].node;
        assert_eq!((n.version.as_deref(), n.subst.as_deref(), n.size, n.time), (Some(&b"2"[..]), Some(&b"1"[..]), 4, 19));
        assert_eq!(n.mode, (crate::S_IFREG | 0o644) as u16);
        // one directory only, names kept whole, versions from the first dot
        let top = chapter_list_elements(&s, &info(), Some(b"hrc"), false, false, Some(b"old/"), &mut sfx, &mut log);
        assert_eq!(names(&top), ["old/Bike-A_bike.10-0.hrc"]);
        assert_eq!((top[0].node.version.as_deref(), top[0].node.time), (Some(&b"10"[..]), 99));
        assert_eq!(log, Log::default());
    }

    #[test]
    fn latest_and_next() {
        let fs = fs();
        let s = Session { fs: &fs, now: 0, loading: false };
        let mut log = Log::default();
        let i = info();
        assert_eq!(file_find_latest_version(&s, &i, b"Bike-A_bike.1-0.hrc", &mut log), b"Bike-A_bike.10-0");
        assert_eq!(file_find_next_version(&s, &i, b"Bike-A_bike", &mut log), b"Bike-A_bike.11-0");
        // the substitution of the answer is always 0: cube's latest is 4-2 on disk
        assert_eq!(file_find_latest_version(&s, &i, b"cube.1-0.hrc", &mut log), b"cube.4-0");
        assert_eq!(file_find_latest_version(&s, &i, b"nothing.5-0.hrc", &mut log), b"nothing.0-0");
        assert_eq!(file_find_next_version(&s, &i, b"nothing", &mut log), b"nothing.1-0");
        // names are cleaned before the search
        assert_eq!(file_find_next_version(&s, &i, b"Bike-A bike.1-0.hrc", &mut log), b"Bike-A_bike.11-0");

        let mut node = scn_get_list_node(b"Bike-A_bike.2-1.hrc", None, true, 0);
        assert_eq!(file_find_elem_latest_version(&s, &i, &mut node, false, &mut log), (10, 1, 4));
        assert_eq!(node.full.as_deref(), Some(&b"Bike-A_bike.2-1"[..]));
        assert_eq!(file_find_elem_latest_version(&s, &i, &mut node, true, &mut log), (10, 1, 4));
        assert_eq!(node.full.as_deref(), Some(&b"Bike-A_bike.10-1"[..]));
        let mut node = scn_get_list_node(b"ghost.2-0.hrc", None, true, 0);
        assert_eq!(file_find_elem_latest_version(&s, &i, &mut node, true, &mut log), (-1, 99999, 0));
        assert_eq!(node.version.as_deref(), Some(&b"2"[..]));

        let mut node = scn_get_list_node(b"Bike-A_bike.1-1.hrc", None, true, 0);
        assert_eq!(file_find_substitution_file(&s, &i, &mut node, &mut log), 2);
        node.subst = Some(b"0".to_vec());
        assert_eq!(file_find_substitution_file(&s, &i, &mut node, &mut log), 10);
        node.subst = Some(b"7".to_vec());
        assert_eq!(file_find_substitution_file(&s, &i, &mut node, &mut log), -9999);
        assert_eq!(next_version_number(&s, &i, b"Bike-A_bike", None, &mut log), 11);
        assert_eq!(next_version_number(&s, &i, b"Bike-A_bike", Some(b"1"), &mut log), 3);
        assert_eq!(next_version_number(&s, &i, b"ghost", None, &mut log), -9998);
    }

    #[test]
    fn sub_directories_and_counts() {
        let fs = fs();
        let s = Session { fs: &fs, now: 0, loading: false };
        let mut i = info();
        assert_eq!(chapter_get_sub_dirs(&s, &i, true, None), (vec![b"old/".to_vec(), b"old/SHARE_lib/".to_vec()], 2));
        assert_eq!(chapter_get_sub_dirs(&s, &i, false, None), (vec![b"old/".to_vec()], 1));
        assert_eq!(chapter_get_sub_dirs(&s, &i, false, Some(b"old/")), (vec![b"old/SHARE_lib/".to_vec()], 1));
        let chap = DbChap { name: b"MODELS".to_vec(), suffix: Some(b"hrc".to_vec()), browser: None, versions: 4 };
        // seven model files and two directories
        assert_eq!(chapter_get_nb_files(&s, &mut i, &chap), 9);
        i.chapters.push(chap);
        assert!(!database_is_empty(&s, &mut i));
        let mut fs2 = MemFs::new();
        fs2.add_file(b"/db/MODELS/Chapter.rsrc", b"");
        let s2 = Session { fs: &fs2, now: 0, loading: false };
        assert!(database_is_empty(&s2, &mut i));
        fs2.add_dir(b"/db/MODELS/empty_dir");
        let s2 = Session { fs: &fs2, now: 0, loading: false };
        assert!(!database_is_empty(&s2, &mut i)); // an empty sub-directory counts
    }

    #[test]
    fn listing_a_plain_directory() {
        let mut fs = fs();
        fs.add_file_with(b"/db/MODELS/run.sh", b"", 0, 0o755);
        fs.add_file_with(b"/db/MODELS/drv.cus", b"", 0, 0o755);
        let s = Session { fs: &fs, now: 0, loading: false };
        let mut log = Log::default();
        let mut sfx = None;
        let l = directory_list_elements(&s, b"/db/MODELS/", None, LIST_EXECUTABLES, Compare::None, &mut sfx, &mut log);
        assert_eq!(names(&l), ["drv.cus", "old", "run.sh"]);
        assert_eq!(l[0].suffix.as_deref(), Some(&b"cus"[..])); // the list has no suffix: each file's is kept
        let l = directory_list_elements(&s, b"/db/MODELS/", None, LIST_CHANNEL_DRIVERS, Compare::None, &mut sfx, &mut log);
        assert_eq!(names(&l), ["old", "run.sh"]);
        let l = directory_list_elements(&s, b"/db/MODELS/", Some(b"txt"), LIST_BY_SUFFIX, Compare::None, &mut sfx, &mut log);
        assert_eq!(names(&l), ["notes.txt", "old"]);
        assert_eq!(l[0].suffix, None);
        // without the final slash the names are glued to the directory and nothing is found
        let l = directory_list_elements(&s, b"/db/MODELS", Some(b"txt"), LIST_BY_SUFFIX, Compare::None, &mut sfx, &mut log);
        assert_eq!(names(&l), Vec::<String>::new());
    }

    #[test]
    fn chapters_of_a_database() {
        let mut fs = MemFs::new();
        fs.add_file(b"/db/SCENES/Chapter.rsrc", b"SUFFIX dsc \nVERSIONS 6 ");
        fs.add_file(b"/db/MODELS/Chapter.rsrc", b"SUFFIX hrc \nVERSIONS 4 ");
        fs.add_dir(b"/db/LIGHTS").add_dir(b"/db/.trash").add_file(b"/db/README", b"");
        let mut log = Log::default();
        let mut i = DbInfo::new();
        i.path = Some(b"/db".to_vec());
        let lists = ElemLists::standard();
        assert_eq!(database_list_chapters(&fs, &lists, &mut i, false, &mut log), 2);
        let got: Vec<_> = i.chapters.iter().map(|c| (String::from_utf8(c.name.clone()).unwrap(), c.suffix.clone(), c.versions)).collect();
        assert_eq!(
            got,
            [
                ("LIGHTS".to_string(), Some(b"lig".to_vec()), 4),
                ("MODELS".to_string(), Some(b"hrc".to_vec()), 4),
                ("SCENES".to_string(), Some(b"dsc".to_vec()), 6)
            ]
        );
        assert_eq!(log.out, b"ERROR: ReadChapterInfo - cannot open /db/LIGHTS/Chapter.rsrc\n");
        assert!(check_database_existence(&fs, b"/db", &lists));
        assert!(check_database_existence(&fs, b"/db/MODELS", &lists));
        assert!(!check_database_existence(&fs, b"/db/MODELS/old", &lists));
        assert!(!check_database_existence(&fs, b"/", &lists));
        let mut none = DbInfo::new();
        none.path = Some(b"/missing".to_vec());
        assert_eq!(database_list_chapters(&fs, &lists, &mut none, true, &mut log), 0);
    }

    #[test]
    fn latest_scene() {
        let mut fs = MemFs::new();
        fs.add_file_with(b"/db/SCENES/a.1-0.dsc", b"", 50, 0o644);
        fs.add_file_with(b"/db/SCENES/b.2-0.dsc", b"", 70, 0o644);
        fs.add_file_with(b"/db/SCENES/c.1-0.dsc", b"", 70, 0o644);
        fs.add_file_with(b"/db/SCENES/d.9-0.txt", b"", 90, 0o644);
        let s = Session { fs: &fs, now: 0, loading: false };
        let mut i = DbInfo::new();
        i.path = Some(b"/db".to_vec());
        assert_eq!(get_latest_scene(&s, &mut i, &mut Log::default()).unwrap(), b"b.2-0");
        i.path = Some(b"/empty".to_vec());
        assert_eq!(get_latest_scene(&s, &mut i, &mut Log::default()), None);
    }

    #[test]
    fn finding_an_element_file() {
        let mut fs = fs();
        fs.add_file(b"/db/MODELS/Bike-wheel.hrc", b"");
        fs.add_file(b"/ext/models/thing.1-0", b"");
        let mut i = DbInfo::new();
        i.path = Some(b"/db".to_vec());
        let subs = [b"old/".to_vec(), b"old/SHARE_lib/".to_vec()];
        let mut go = |name: &[u8], sub: &[u8]| {
            let mut node = scn_get_list_node(name, None, true, 0);
            node.sub_path = Some(sub.to_vec());
            let f = file_find_in_chapter(&fs, &mut i, b"MODELS", crate::chap::MODELS, &subs, &mut node);
            (f.map(|f| (String::from_utf8(f.path_ext).unwrap(), f.sub_dir.is_some(), f.share)), node.sub_path)
        };
        // where the node says
        assert_eq!(go(b"Bike-A_bike.10-0.hrc", b"old/").0.unwrap(), ("/db/MODELS/old/Bike-A_bike.10-0.hrc".to_string(), false, false));
        // not there: the chapter itself, and the node is corrected
        let (f, sub) = go(b"cube.1-0.hrc", b"old/");
        assert_eq!(f.unwrap(), ("/db/MODELS/cube.1-0.hrc".to_string(), false, false));
        assert_eq!(sub, Some(Vec::new()));
        // then the list's sub-directories
        assert_eq!(go(b"cube.4-2.hrc", b"").0.unwrap(), ("/db/MODELS/old/SHARE_lib/cube.4-2.hrc".to_string(), true, false));
        // then the name without version
        assert_eq!(go(b"Bike-wheel.5-0.hrc", b"").0.unwrap(), ("/db/MODELS/Bike-wheel.hrc".to_string(), false, false));
        assert_eq!(go(b"ghost.1-0.hrc", b"").0, None);
        // a directory outside the database: the full name as it is
        assert_eq!(go(b"thing.1-0.hrc", b"/ext/models").0.unwrap(), ("/ext/models/thing.1-0".to_string(), false, false));
        assert_eq!(str_strip_suffix(b"/a/b.1-0.hrc"), b"/a/b.1-0");
        assert_eq!(str_strip_suffix(b"/a/b.1-0"), b"/a/b.1-0");
        assert_eq!(str_strip_suffix(b"/a.b/c"), b"/a.b/c");
    }
}
