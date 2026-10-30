// SPDX-License-Identifier: MIT
//! Databases and their chapters in memory, `Chapter.rsrc`, and the list of databases
//! (`DatabaseDir.rsrc`, `DatabaseSys.rsrc`).

use crate::{
    concat, database_list_chapters, dec, ed_get_suffix_from_chapter, find, or_null, path_expand_shortcuts,
    strtol10, Bytes, ElemLists, Env, FileSystem, FileSystemMut, TokenReader, DEFAULT_VERSIONS, R_OK,
};

/// What the original prints while it works: `out` is its standard output, `err` its
/// standard error. The ported routines append the same bytes instead of printing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    pub out: Bytes,
    pub err: Bytes,
}

/// One chapter of a database (`SI_DbChap`, 16 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct DbChap {
    /// The chapter's name, which is its directory's name.
    pub name: Bytes,
    /// The suffix of its files (`SUFFIX` in `Chapter.rsrc`).
    pub suffix: Option<Bytes>,
    /// The value of an optional `BROWSER` entry in `Chapter.rsrc`. No file on the discs has
    /// one and nothing in `dbexternal.c` uses it beyond a debugging dump.
    pub browser: Option<Bytes>,
    /// How many versions of an element are kept (`VERSIONS`).
    pub versions: i32,
}

/// One database (`SI_DbInfo`, 40 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct DbInfo {
    /// Offsets 0x00 and 0x04: the chapters, sorted by name once listed.
    pub chapters: Vec<DbChap>,
    /// Offset 0x08: the name the database goes by in the list of databases.
    pub name: Option<Bytes>,
    /// Offset 0x0c: its directory.
    pub path: Option<Bytes>,
    /// Offset 0x10: the chapter being worked on.
    pub chapter: Option<Bytes>,
    /// Offset 0x14: that chapter's directory, with a slash at the end.
    pub chapter_path: Option<Bytes>,
    /// Offset 0x18: 1; set to 0 when the directory holds no chapter (or creating it failed).
    pub valid: i16,
    /// Offset 0x1a: non-zero for a database from the system-wide list (`DatabaseSys.rsrc`),
    /// or the number given by a `MASTER` entry. Such a database is never written to the
    /// user's list.
    pub system: i16,
    /// Offset 0x1c: set to 0 on creation; not used in `dbexternal.c`.
    pub w1c: i16,
    /// Offset 0x1e: 1; the debugging dump shows only databases where it is non-zero.
    pub w1e: i16,
    /// Offset 0x20: 1; not used in `dbexternal.c`.
    pub w20: i16,
    /// Offset 0x24: 1 for an ordinary database, 2 for a shader library.
    pub kind: i32,
}

pub const DB_KIND_NORMAL: i32 = 1;
pub const DB_KIND_SHADERLIB: i32 = 2;

impl Default for DbInfo {
    fn default() -> Self {
        DbInfo::new()
    }
}

impl DbInfo {
    /// An empty record [soft 0x081215ac SI_dbInfoInit].
    pub fn new() -> DbInfo {
        DbInfo {
            chapters: Vec::new(),
            name: None,
            path: None,
            chapter: None,
            chapter_path: None,
            valid: 1,
            system: 0,
            w1c: 0,
            w1e: 1,
            w20: 1,
            kind: DB_KIND_NORMAL,
        }
    }

    /// Appends a chapter record with no suffix and the default number of versions
    /// [soft 0x0812164c dbInfoAddChapter].
    pub fn add_chapter(&mut self, name: &[u8]) {
        self.chapters.push(DbChap { name: name.to_vec(), suffix: None, browser: None, versions: DEFAULT_VERSIONS });
    }

    /// The position of a chapter, −1 if the database has none of that name
    /// [soft 0x08121540 SI_dbInfoGetChapterIndex].
    pub fn chapter_index(&self, name: &[u8]) -> i32 {
        self.chapters.iter().position(|c| c.name == name).map_or(-1, |i| i as i32)
    }

    /// The suffix of a chapter; null both for an unknown chapter and for one whose suffix
    /// was never set [soft 0x0812135c SI_dbInfoGetSuffix].
    pub fn suffix_of(&self, chapter: &[u8]) -> Option<&[u8]> {
        self.chapters.iter().find(|c| c.name == chapter).and_then(|c| c.suffix.as_deref())
    }

    /// Sorts the chapters by name, byte order [soft 0x08121628 dbDatabaseSortChapters]
    /// [soft 0x08121608 dbDatabaseCompChapters]. (`qsort` in the original, which need not
    /// keep the order of equal names; two chapters of one name do not occur.)
    pub fn sort_chapters(&mut self) {
        self.chapters.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Makes `chapter` the chapter being worked on [soft 0x08122654 SI_dbSetChapter].
    pub fn set_chapter(&mut self, chapter: &[u8]) {
        self.chapter = Some(chapter.to_vec());
    }

    /// Sets the chapter directory to `<database path>/<chapter>/`
    /// [soft 0x081226cc SI_dbfilepathBuild]. (A null path or chapter crashes the original;
    /// here it counts as empty.)
    pub fn filepath_build(&mut self) {
        let empty: &[u8] = b"";
        self.chapter_path = Some(concat(&[
            self.path.as_deref().unwrap_or(empty),
            b"/",
            self.chapter.as_deref().unwrap_or(empty),
            b"/",
        ]));
    }

    /// `access` on the database directory [soft 0x08120ef4 SI_dbDatabaseCheckAccess].
    pub fn database_check_access(&self, fs: &dyn FileSystem, mode: i32) -> bool {
        self.path.as_deref().is_some_and(|p| fs.access(p, mode))
    }

    /// `access` on the current chapter's directory [soft 0x08120f20 SI_dbChapterCheckAccess].
    pub fn chapter_check_access(&self, fs: &dyn FileSystem, mode: i32) -> bool {
        self.chapter_path.as_deref().is_some_and(|p| fs.access(p, mode))
    }

    /// The debugging dump of the chapters [soft 0x0812125c dbChapInfoDump].
    pub fn chap_info_dump(&self) -> Bytes {
        let mut out = b" CHAPTERS:\n".to_vec();
        for (i, c) in self.chapters.iter().enumerate() {
            out.extend_from_slice(&concat(&[
                b" ",
                &dec(i as i32),
                b" -> ",
                &c.name,
                b" (.",
                or_null(&c.suffix),
                b") ",
                or_null(&c.browser),
                b" vers=",
                &dec(c.versions),
                b"\n",
            ]));
        }
        out
    }
}

/// Whether a file can be read [soft 0x08120f4c SI_dbCheckFileExistence]: `access` with
/// `R_OK`, so a file that exists but is unreadable "does not exist".
pub fn check_file_existence(fs: &dyn FileSystem, path: &[u8]) -> bool {
    fs.access(path, R_OK)
}

/// The three plain concatenations the scans build their paths with
/// [soft 0x08120bf4 filepathSet3] [soft 0x08120c34 filepathSet4] [soft 0x08120c84
/// filepathBuild3]: nothing is put between the pieces. (The first two write into a buffer
/// of the caller, the third allocates.)
pub fn filepath_set(parts: &[&[u8]]) -> Bytes {
    concat(parts)
}

/// `<database path>/<chapter>/Chapter.rsrc`.
pub fn chapter_rsrc_path(db_path: &[u8], chapter: &[u8]) -> Bytes {
    filepath_set(&[db_path, b"/", chapter, b"/Chapter.rsrc"])
}

/// What [`chapter_read_info`] is given in place of the file.
pub enum ChapterRsrc<'a> {
    /// The file was opened: its bytes.
    Opened(&'a [u8]),
    /// The file could not be opened.
    Missing,
    /// The caller only wants to know whether the file is there (`access` with `R_OK`), and
    /// this is the answer.
    CheckOnly(bool),
}

/// Fills a chapter record from the chapter's `Chapter.rsrc` [soft 0x08123f0c dbChapterReadInfo].
///
/// The file is a list of tokens (see [`TokenReader`]). Three words are understood, each
/// followed by one value:
///
/// | Word | Value |
/// |---|---|
/// | `SUFFIX` | the suffix of the chapter's files. **For the chapter `SETUP_SOFT` the value is ignored and `sts` is stored.** |
/// | `BROWSER` | a text, stored |
/// | `VERSIONS` | a decimal number; anything below 1 (or not a number) becomes 1 |
///
/// Any other token is skipped. The value is taken even when reading it "failed" because
/// the file ends right after it, so a file without a final blank still gives its number.
///
/// Without the file (or in check-only mode, where it is never opened) the record gets the
/// default number of versions and the suffix of the program's own table for a chapter of
/// that name, if the dictionary `lists` has a list so called. The return value says whether the file
/// was opened, or in check-only mode whether it is readable. Only a failed open in normal
/// mode prints a message.
pub fn chapter_read_info(
    lists: &ElemLists,
    chapter: &[u8],
    chap: &mut DbChap,
    file: ChapterRsrc,
    path_for_message: &[u8],
    log: &mut Log,
) -> bool {
    let (data, result, complain) = match file {
        ChapterRsrc::Opened(d) => (Some(d), true, false),
        ChapterRsrc::Missing => (None, false, true),
        ChapterRsrc::CheckOnly(readable) => (None, readable, false),
    };
    let Some(data) = data else {
        if complain {
            log.out.extend_from_slice(&concat(&[b"ERROR: ReadChapterInfo - cannot open ", path_for_message, b"\n"]));
        }
        chap.versions = DEFAULT_VERSIONS;
        let id = lists.find_list_by_name(chapter);
        if id != -1 {
            chap.suffix = Some(ed_get_suffix_from_chapter(id).as_bytes().to_vec());
        }
        return result;
    };
    let mut t = TokenReader::new(data);
    while t.read() {
        match t.token() {
            b"SUFFIX" => {
                t.read();
                chap.suffix = Some(if chapter == b"SETUP_SOFT" { b"sts".to_vec() } else { t.token().to_vec() });
            }
            b"BROWSER" => {
                t.read();
                chap.browser = Some(t.token().to_vec());
            }
            b"VERSIONS" => {
                t.read();
                chap.versions = strtol10(t.token()).max(1);
            }
            _ => {}
        }
    }
    result
}

/// The text of a `Chapter.rsrc` [soft 0x081272f0 dbWriteChapterRsrc]: `SUFFIX <suffix> `,
/// a newline, `VERSIONS <n> `. Each word and value is followed by one blank; there is no
/// newline at the end. A `BROWSER` entry is never written back.
pub fn write_chapter_rsrc(chap: &DbChap) -> Bytes {
    concat(&[b"SUFFIX ", or_null(&chap.suffix), b" \nVERSIONS ", &dec(chap.versions), b" "])
}

/// How many versions a chapter keeps [soft 0x081213a4 SI_dbInfoGetNbVersions].
///
/// The default for a chapter the database does not have. With `reread` the chapter's
/// `Chapter.rsrc` is read again first and the record updated: the reader skips to the
/// token `VERSIONS` and takes the next one, **but only if that read succeeded**, which (see
/// [`TokenReader::get`]) needs at least one character after the number. The program's own
/// files end with a blank.
pub fn db_info_get_nb_versions(info: &mut DbInfo, chapter: &[u8], reread: Option<&dyn FileSystem>) -> i32 {
    let Some(i) = info.chapters.iter().position(|c| c.name == chapter) else {
        return DEFAULT_VERSIONS;
    };
    if let Some(fs) = reread {
        let path = chapter_rsrc_path(info.path.as_deref().unwrap_or(b""), &info.chapters[i].name);
        if let Some(data) = fs.read(&path) {
            let mut t = TokenReader::new(&data);
            if t.reach(b"VERSIONS", b';') && t.read() {
                info.chapters[i].versions = strtol10(t.token()).max(1);
            }
        }
    }
    info.chapters[i].versions
}

/// Changes the number of versions of a chapter and rewrites its `Chapter.rsrc`
/// [soft 0x08127390 SI_dbChapterSetNbVersions]. False for an unknown chapter or when the
/// file cannot be opened for writing. The number is stored as given, even below 1.
pub fn chapter_set_nb_versions(fs: &mut dyn FileSystemMut, info: &mut DbInfo, chapter: &[u8], versions: i32) -> bool {
    let i = info.chapter_index(chapter);
    if i == -1 {
        return false;
    }
    info.chapters[i as usize].versions = versions;
    let path = chapter_rsrc_path(info.path.as_deref().unwrap_or(b""), chapter);
    fs.write(&path, &write_chapter_rsrc(&info.chapters[i as usize]), false)
}

/// The list of databases the program knows (`SI_DbDir`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbDir {
    pub databases: Vec<DbInfo>,
    /// Set when a database is added, cleared when the list is written.
    pub modified: i16,
}

/// Results of the two searches below.
pub const DB_FOUND: i32 = 100;
pub const DB_NOT_FOUND: i32 = 0x65;
pub const DB_AMBIGUOUS: i32 = 0x66;

/// How [`DbDir::search_database_from_path`] matches (`SI_DbtSearch`).
pub const SEARCH_EXACT: i32 = 1;
/// The database's path contains the given text.
pub const SEARCH_CONTAINS: i32 = 2;
/// The given path contains the database's path, followed by a slash or the end.
pub const SEARCH_INSIDE: i32 = 3;

/// Results of [`database_dir_write`].
pub const DIR_WRITTEN: i32 = 0x5b;
pub const DIR_NOT_WRITTEN: i32 = 0x5c;

impl DbDir {
    /// An empty list [soft 0x0812158c SI_dbDatabaseDirPrepare].
    pub fn new() -> DbDir {
        DbDir::default()
    }

    /// The position of the database with this name **and** this path
    /// [soft 0x081216c8 SI_dbDirDbExistInList].
    pub fn db_exist_in_list(&self, name: &[u8], path: &[u8]) -> Option<usize> {
        self.databases.iter().position(|d| d.name.as_deref() == Some(name) && d.path.as_deref() == Some(path))
    }

    /// Finds a database by name or by the start of its name
    /// [soft 0x08121744 SI_dbDirSearchDatabaseFromName]. Returns the result code, the
    /// database and how many matched. An exact name wins at once; otherwise one database
    /// whose name starts with the text is found, several are ambiguous.
    pub fn search_database_from_name(&self, name: &[u8]) -> (i32, Option<usize>, i32) {
        let mut count = 0;
        let mut last = None;
        for (i, d) in self.databases.iter().enumerate() {
            let dn = d.name.as_deref().unwrap_or(b"");
            if dn == name {
                return (DB_FOUND, Some(i), 1);
            }
            if dn.starts_with(name) {
                count += 1;
                last = Some(i);
            }
        }
        match count {
            0 => (DB_NOT_FOUND, None, 0),
            1 => (DB_FOUND, last, 1),
            _ => (DB_AMBIGUOUS, None, count),
        }
    }

    /// Finds a database by path [soft 0x081218f4 SI_dbDirSearchDatabaseFromPath]; `mode` is
    /// one of the `SEARCH_` values. An exact path wins at once in every mode.
    pub fn search_database_from_path(&self, path: &[u8], mode: i32) -> (i32, Option<usize>, i32) {
        let mut count = 0;
        let mut last = None;
        for (i, d) in self.databases.iter().enumerate() {
            let dp = d.path.as_deref().unwrap_or(b"");
            if dp == path {
                return (DB_FOUND, Some(i), 1);
            }
            if mode == SEARCH_CONTAINS && find(dp, path).is_some() {
                count += 1;
                last = Some(i);
            }
            if mode == SEARCH_INSIDE {
                // the first place the database's path occurs in the given one, as strstr finds it
                if let Some(at) = find(path, dp) {
                    if matches!(path.get(at + dp.len()), None | Some(b'/')) {
                        count += 1;
                        last = Some(i);
                    }
                }
            }
        }
        match count {
            0 => (DB_NOT_FOUND, None, 0),
            1 => (DB_FOUND, last, 1),
            _ => (DB_AMBIGUOUS, None, count),
        }
    }

    /// Appends a database with this name and path and marks the list as changed
    /// [soft 0x08121a94 SI_dbDirAddDatabase].
    pub fn add_database(&mut self, name: &[u8], path: &[u8]) -> usize {
        let mut info = DbInfo::new();
        info.name = Some(name.to_vec());
        info.path = Some(path.to_vec());
        self.databases.push(info);
        self.modified = 1;
        self.databases.len() - 1
    }

    /// The database called `BACKUP` [soft 0x08121b10 SI_dbDirGetBackupDatabase].
    pub fn get_backup_database(&self) -> Option<usize> {
        self.get_database_from_name(b"BACKUP")
    }

    /// The first database with exactly this name [soft 0x08121b50 SI_dbDirGetDatabaseFromName].
    pub fn get_database_from_name(&self, name: &[u8]) -> Option<usize> {
        self.databases.iter().position(|d| d.name.as_deref() == Some(name))
    }

    /// The position of a database record in the list, −1 if it is not one of the list's
    /// [soft 0x08121ba4 SI_dbDirGetDatabaseIndex]. The record is recognised by its address,
    /// as in the original, not by its content.
    pub fn get_database_index(&self, info: &DbInfo) -> i32 {
        self.databases.iter().position(|d| std::ptr::eq(d, info)).map_or(-1, |i| i as i32)
    }

    /// The database at `index`, null when out of range [soft 0x081225ac SI_dbSetDatabase].
    pub fn set_database(&self, index: i32) -> Option<usize> {
        (index >= 0 && (index as usize) < self.databases.len()).then_some(index as usize)
    }

    /// The default database: the first valid one that is not a shader library
    /// [soft 0x081225dc SI_dbSetCurrentDatabase].
    pub fn set_current_database(&self) -> Option<usize> {
        self.databases.iter().position(|d| d.valid != 0 && d.kind == DB_KIND_NORMAL)
    }

    /// Whether some valid database is the user's own, not a system one
    /// [soft 0x08122610 SI_dbCheckLocalDatabase].
    pub fn check_local_database(&self) -> bool {
        self.databases.iter().any(|d| d.valid != 0 && d.system == 0)
    }
}

/// The two globals that remember the shader library named by the environment variable
/// `SI_CUSTOM_SHADER`: its path (`SI_cusShaderDbRoot`, 0x08684064, read once) and the name
/// of the listed database that has that path (`SI_cusShaderDbRoot_name`, 0x08684068).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderDbRoot {
    pub root: Option<Bytes>,
    pub name: Option<Bytes>,
}

/// Everything [`database_dir_read`] reaches for besides its arguments.
pub struct DirContext<'a> {
    pub fs: &'a dyn FileSystem,
    pub env: &'a dyn Env,
    /// The lists of the current element dictionary.
    pub lists: &'a ElemLists,
    pub shader: &'a mut ShaderDbRoot,
    pub log: &'a mut Log,
}

/// Reads a list of databases and adds its entries to `dir` [soft 0x081237f0
/// SI_dbDatabaseDirRead]. `file` is opened with a plain `fopen` [soft 0x08120da0 dbfileOpen].
///
/// The file is a list of tokens:
///
/// | Tokens | Effect |
/// |---|---|
/// | `DATABASE <name>` | remembers the name for the `PATH` that follows |
/// | `PATH <path>` | the path goes through [`path_expand_shortcuts`]; unless a database with that name and path is already listed, one is added and its directory is searched for chapters (sub-directories with a readable `Chapter.rsrc`). With none, the database stays in the list, marked invalid |
/// | `TYPE SHADERLIB` | marks the database just added as a shader library. `TYPE` itself does nothing; the next token is handled on its own |
/// | `MASTER <n>` | sets the database's "system" number |
/// | `ACCESS <text>` | only prints a line |
/// | `#…` | a comment: everything up to the next `DATABASE` token is skipped |
/// | anything else | the same skip, after an error message |
///
/// The keywords are compared exactly, in capitals.
///
/// `system` is stored in each database added. `all` chooses between two behaviours: when
/// false, the file is not read at all if the list already has a database, and reading
/// **stops at the first database that has chapters**; when true, every entry is read.
/// Returns false when the file could not be opened (or was not wanted).
///
/// Kept oddities: the "no path for this database" error can only fire before the first
/// `PATH` of the file, because the flag it tests is never reset; a database whose path
/// equals `SI_CUSTOM_SHADER` becomes a shader library without a `TYPE` line, the first
/// time only; a `TYPE SHADERLIB` after an already listed database is skipped as a pair.
///
/// Where this differs: `MASTER`, `ACCESS` and `SHADERLIB` act on "the database added
/// last", a local variable the original does not initialise, so before the first
/// database of a file it holds whatever the stack held. Here there is none then, and the
/// three do nothing.
pub fn database_dir_read(dir: &mut DbDir, file: &[u8], system: i16, all: bool, ctx: &mut DirContext) -> bool {
    if !all && !dir.databases.is_empty() {
        return false;
    }
    let Some(data) = ctx.fs.read(file) else { return false };
    if ctx.shader.root.is_none() {
        ctx.shader.root = ctx.env.var(b"SI_CUSTOM_SHADER");
    }
    let mut t = TokenReader::new(&data);
    let mut name: Option<Bytes> = None;
    let mut has_path = false;
    let mut current: Option<usize> = None;
    let mut ok = t.read();
    while ok {
        match t.token() {
            b"DATABASE" => {
                if let (Some(n), false) = (&name, has_path) {
                    ctx.log.err.extend_from_slice(&concat(&[
                        b"\nERROR: No path found for database: [",
                        n,
                        b"]\n       in file [",
                        file,
                        b"].\n",
                    ]));
                    name = None;
                }
                if t.read() {
                    name = Some(t.token().to_vec());
                }
                ok = t.read();
            }
            b"PATH" => {
                let got = t.read();
                has_path = true;
                let path = path_expand_shortcuts(t.token(), ctx.env);
                let mut listed = false;
                if let Some(n) = name.take() {
                    if got {
                        listed = dir.db_exist_in_list(&n, &path).is_some();
                        if !listed {
                            let i = dir.add_database(&n, &path);
                            current = Some(i);
                            dir.databases[i].system = system;
                            if ctx.shader.name.is_none() && ctx.shader.root.as_deref() == Some(&path[..]) {
                                ctx.shader.name = Some(n.clone());
                                dir.databases[i].kind = DB_KIND_SHADERLIB;
                            }
                            if database_list_chapters(ctx.fs, ctx.lists, &mut dir.databases[i], true, ctx.log) > 0 {
                                if !all {
                                    return true;
                                }
                            } else {
                                ctx.log.err.extend_from_slice(&concat(&[
                                    b"ERROR: Database ",
                                    or_null(&dir.databases[i].name),
                                    b" non-existent or invalid\n",
                                ]));
                                dir.databases[i].valid = 0;
                                dir.databases[i].w1e = 1;
                            }
                        }
                    }
                }
                ok = t.read();
                if ok && listed && t.token() == b"TYPE" {
                    ok = t.read();
                    if ok && t.token() == b"SHADERLIB" {
                        ok = t.read();
                    }
                }
            }
            b"MASTER" => {
                let got = t.read();
                if let (Some(i), true) = (current, got) {
                    dir.databases[i].system = strtol10(t.token()) as i16;
                }
                ok = t.read();
            }
            b"ACCESS" => {
                let got = t.read();
                if let (Some(i), true) = (current, got) {
                    ctx.log.out.extend_from_slice(&concat(&[
                        b" Database [",
                        or_null(&dir.databases[i].name),
                        b"] access is -> [",
                        t.token(),
                        b"]\n",
                    ]));
                }
                ok = t.read();
            }
            b"TYPE" => ok = t.read(),
            b"SHADERLIB" => {
                if let Some(i) = current {
                    dir.databases[i].kind = DB_KIND_SHADERLIB;
                }
                ok = t.read();
            }
            other => {
                if other.first() != Some(&b'#') {
                    ctx.log.err.extend_from_slice(&concat(&[
                        b"\nERROR: Invalid token : [",
                        other,
                        b"] in file [",
                        file,
                        b"].\n",
                    ]));
                }
                ok = t.reach(b"DATABASE", b';');
            }
        }
    }
    true
}

/// What writing the list of databases would put where.
#[derive(Clone, Debug, PartialEq)]
pub struct DirWrite {
    pub path: Bytes,
    pub append: bool,
    pub data: Bytes,
}

/// The text and the place of the user's list of databases [soft 0x08123d30
/// SI_dbDatabaseDirWrite], without the writing itself (see [`write_database_dir`]).
///
/// The place: `file` if given; else `$SI_DBDIR/DatabaseDir.rsrc` if that variable is set;
/// else `<home>/DatabaseDir.rsrc`. Per database: `DATABASE <name> `, newline,
/// `PATH <path> `, newline, and for a shader library `TYPE SHADERLIB `, newline. System
/// databases are left out. With `append` only the last database of the list is written, to
/// be added to the end of the file.
pub fn database_dir_write(dir: &DbDir, home: &[u8], file: Option<&[u8]>, append: bool, env: &dyn Env) -> DirWrite {
    let path = match (file, env.var(b"SI_DBDIR")) {
        (Some(f), _) => f.to_vec(),
        (None, Some(d)) => concat(&[&d, b"/DatabaseDir.rsrc"]),
        (None, None) => concat(&[home, b"/DatabaseDir.rsrc"]),
    };
    // (appending with an empty list makes the original read the record before the array)
    let first = if append { dir.databases.len().saturating_sub(1) } else { 0 };
    let mut data = Vec::new();
    for d in dir.databases.iter().skip(first).filter(|d| d.system == 0) {
        data.extend_from_slice(&concat(&[b"DATABASE ", or_null(&d.name), b" \nPATH ", or_null(&d.path), b" \n"]));
        if d.kind == DB_KIND_SHADERLIB {
            data.extend_from_slice(b"TYPE SHADERLIB \n");
        }
    }
    DirWrite { path, append, data }
}

/// [`database_dir_write`] and the write: [`DIR_WRITTEN`], with the list's changed flag
/// cleared, or [`DIR_NOT_WRITTEN`] when the file cannot be opened.
pub fn write_database_dir(
    fs: &mut dyn FileSystemMut,
    dir: &mut DbDir,
    home: &[u8],
    file: Option<&[u8]>,
    append: bool,
    env: &dyn Env,
) -> i32 {
    let w = database_dir_write(dir, home, file, append, env);
    if !fs.write(&w.path, &w.data, w.append) {
        return DIR_NOT_WRITTEN;
    }
    dir.modified = 0;
    DIR_WRITTEN
}

/// Loads the lists of databases at start-up [soft 0x08108130 SI_databaseDirectoryLoad].
///
/// 1. `$SI_DBDIR/DatabaseDir.rsrc`, if the variable is set.
/// 2. If it is not set, or that file could not be read: `<cwd>/DatabaseDir.rsrc`, the
///    directory the program was started in.
/// 3. Always: `<sys_dir>/DatabaseSys.rsrc`, whose databases are marked as system ones.
///
/// Each failure prints two lines. With `all` false the system list adds nothing once the
/// user's list has produced a database (see [`database_dir_read`]).
pub fn database_directory_load(dir: &mut DbDir, sys_dir: &[u8], all: bool, cwd: &[u8], ctx: &mut DirContext) {
    let mut done = false;
    if let Some(d) = ctx.env.var(b"SI_DBDIR") {
        let file = concat(&[&d, b"/DatabaseDir.rsrc"]);
        done = database_dir_read(dir, &file, 0, all, ctx);
        if !done {
            ctx.log.err.extend_from_slice(&concat(&[
                b"\nERROR: Couldn't read the DatabaseDir.rsrc file from the path\n         specified by SI_DBDIR: [",
                &file,
                b"]\n",
            ]));
        }
    }
    if !done {
        let file = concat(&[cwd, b"/DatabaseDir.rsrc"]);
        if !database_dir_read(dir, &file, 0, all, ctx) {
            ctx.log.err.extend_from_slice(
                b"ERROR: Couldn't read the DatabaseDir.rsrc file from the\n       current directory (check SI_DBDIR env. variable)\n",
            );
        }
    }
    let file = concat(&[sys_dir, b"/DatabaseSys.rsrc"]);
    database_dir_read(dir, &file, 1, all, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEnv, MemFs};

    fn chap(name: &str) -> DbChap {
        DbChap { name: name.as_bytes().to_vec(), suffix: None, browser: None, versions: 0 }
    }

    #[test]
    fn chapter_rsrc_read_and_write() {
        let std = ElemLists::standard();
        let mut log = Log::default();
        let mut c = chap("MODELS");
        assert!(chapter_read_info(&std, b"MODELS", &mut c, ChapterRsrc::Opened(b"SUFFIX hrc \nVERSIONS 4 "), b"", &mut log));
        assert_eq!((c.suffix.as_deref(), c.versions), (Some(&b"hrc"[..]), 4));
        assert_eq!(write_chapter_rsrc(&c), b"SUFFIX hrc \nVERSIONS 4 ");
        // no final blank, other line ends, unknown words, a browser entry
        let mut c = chap("MODELS");
        chapter_read_info(&std, b"MODELS", &mut c, ChapterRsrc::Opened(b"junk SUFFIX abc\r\nBROWSER icons\r\nVERSIONS 12"), b"", &mut log);
        assert_eq!((c.suffix.as_deref(), c.browser.as_deref(), c.versions), (Some(&b"abc"[..]), Some(&b"icons"[..]), 12));
        assert_eq!(write_chapter_rsrc(&c), b"SUFFIX abc \nVERSIONS 12 ");
        // numbers below one, and words that are not numbers, give one
        for text in ["VERSIONS 0 ", "VERSIONS -3 ", "VERSIONS many "] {
            let mut c = chap("MODELS");
            chapter_read_info(&std, b"MODELS", &mut c, ChapterRsrc::Opened(text.as_bytes()), b"", &mut log);
            assert_eq!(c.versions, 1, "{text}");
        }
        // the setups chapter always gets "sts"
        let mut c = chap("SETUP_SOFT");
        chapter_read_info(&std, b"SETUP_SOFT", &mut c, ChapterRsrc::Opened(b"SUFFIX set "), b"", &mut log);
        assert_eq!(c.suffix.as_deref(), Some(&b"sts"[..]));
        // an empty file changes nothing
        let mut c = chap("MODELS");
        assert!(chapter_read_info(&std, b"MODELS", &mut c, ChapterRsrc::Opened(b""), b"", &mut log));
        assert_eq!((c.suffix, c.versions), (None, 0));
        assert_eq!(log, Log::default());
    }

    #[test]
    fn chapter_rsrc_missing() {
        let std = ElemLists::standard();
        let mut log = Log::default();
        let mut c = chap("TEXTURES3D");
        assert!(!chapter_read_info(&std, b"TEXTURES3D", &mut c, ChapterRsrc::Missing, b"/db/TEXTURES3D/Chapter.rsrc", &mut log));
        assert_eq!((c.suffix.as_deref(), c.versions), (Some(&b"t3D"[..]), 4));
        assert_eq!(log.out, b"ERROR: ReadChapterInfo - cannot open /db/TEXTURES3D/Chapter.rsrc\n");
        // check-only: the table's suffix, no message, the answer passed through
        let mut log = Log::default();
        let mut c = chap("MyOwn");
        assert!(chapter_read_info(&std, b"MyOwn", &mut c, ChapterRsrc::CheckOnly(true), b"", &mut log));
        assert_eq!((c.suffix.as_deref(), c.versions), (None, 4));
        assert_eq!(log, Log::default());
    }

    #[test]
    fn versions_of_a_chapter() {
        let mut fs = MemFs::new();
        fs.add_file(b"/db/MODELS/Chapter.rsrc", b"SUFFIX hrc \nVERSIONS 7 ");
        fs.add_file(b"/db/SCENES/Chapter.rsrc", b"SUFFIX dsc \nVERSIONS 9");
        let mut info = DbInfo::new();
        info.path = Some(b"/db".to_vec());
        info.add_chapter(b"MODELS");
        info.add_chapter(b"SCENES");
        assert_eq!(db_info_get_nb_versions(&mut info, b"MODELS", None), 4);
        assert_eq!(db_info_get_nb_versions(&mut info, b"MODELS", Some(&fs)), 7);
        assert_eq!(db_info_get_nb_versions(&mut info, b"MODELS", None), 7);
        // a number that is the last thing in the file is not taken by this reader
        assert_eq!(db_info_get_nb_versions(&mut info, b"SCENES", Some(&fs)), 4);
        assert_eq!(db_info_get_nb_versions(&mut info, b"LIGHTS", Some(&fs)), 4);
        assert!(chapter_set_nb_versions(&mut fs, &mut info, b"MODELS", 2));
        assert_eq!(fs.read(b"/db/MODELS/Chapter.rsrc").unwrap(), b"SUFFIX (null) \nVERSIONS 2 ");
        assert!(!chapter_set_nb_versions(&mut fs, &mut info, b"LIGHTS", 2));
    }

    #[test]
    fn info_records() {
        let mut info = DbInfo::new();
        info.path = Some(b"/dbs/demo".to_vec());
        info.set_chapter(b"MODELS");
        info.filepath_build();
        assert_eq!(info.chapter_path.as_deref(), Some(&b"/dbs/demo/MODELS/"[..]));
        info.add_chapter(b"SCENES");
        info.add_chapter(b"MODELS");
        info.chapters[1].suffix = Some(b"hrc".to_vec());
        info.sort_chapters();
        assert_eq!(info.chapter_index(b"MODELS"), 0);
        assert_eq!(info.chapter_index(b"LIGHTS"), -1);
        assert_eq!(info.suffix_of(b"MODELS"), Some(&b"hrc"[..]));
        assert_eq!(info.suffix_of(b"SCENES"), None);
        assert_eq!(info.chap_info_dump(), b" CHAPTERS:\n 0 -> MODELS (.hrc) (null) vers=4\n 1 -> SCENES (.(null)) (null) vers=4\n");
    }

    fn sample_fs() -> MemFs {
        let mut fs = MemFs::new();
        fs.add_file(b"/dbs/one/MODELS/Chapter.rsrc", b"SUFFIX hrc \nVERSIONS 4 ");
        fs.add_file(b"/dbs/one/SCENES/Chapter.rsrc", b"SUFFIX dsc \nVERSIONS 4 ");
        fs.add_file(b"/dbs/two/MODELS/Chapter.rsrc", b"SUFFIX hrc \nVERSIONS 4 ");
        fs.add_dir(b"/dbs/empty/MODELS");
        fs.add_file(b"/lib/shaders/MATERIAL_SHADERS/Chapter.rsrc", b"");
        fs
    }

    #[test]
    fn database_list_read() {
        let std = ElemLists::standard();
        let mut fs = sample_fs();
        fs.add_file(
            b"/home/me/DatabaseDir.rsrc",
            b"# my databases\nDATABASE one\nPATH /dbs/x/../one\nDATABASE gone PATH /dbs/gone\nDATABASE empty\nPATH /dbs/empty\n\
              DATABASE lib PATH /lib/shaders TYPE SHADERLIB\nMASTER 3\nACCESS rw\nbogus words DATABASE two PATH /dbs/two\n\
              DATABASE one PATH /dbs/one TYPE SHADERLIB DATABASE two PATH /dbs/two ",
        );
        let env = MapEnv::default();
        let (mut shader, mut log) = (ShaderDbRoot::default(), Log::default());
        let mut dir = DbDir::new();
        let mut ctx = DirContext { fs: &fs, env: &env, lists: &std, shader: &mut shader, log: &mut log };
        assert!(database_dir_read(&mut dir, b"/home/me/DatabaseDir.rsrc", 0, true, &mut ctx));
        let names: Vec<_> = dir.databases.iter().map(|d| String::from_utf8(d.name.clone().unwrap()).unwrap()).collect();
        assert_eq!(names, ["one", "gone", "empty", "lib", "two"]);
        assert_eq!(dir.databases[0].path.as_deref(), Some(&b"/dbs/one"[..]));
        assert_eq!(dir.databases.iter().map(|d| d.valid).collect::<Vec<_>>(), [1, 0, 0, 1, 1]);
        assert_eq!(dir.databases.iter().map(|d| d.kind).collect::<Vec<_>>(), [1, 1, 1, 2, 1]);
        assert_eq!(dir.databases[3].system, 3);
        assert_eq!(dir.databases[0].chapters.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), [b"MODELS".to_vec(), b"SCENES".to_vec()]);
        assert_eq!(dir.databases[0].chapters[0].suffix.as_deref(), Some(&b"hrc"[..]));
        assert_eq!(log.out, b" Database [lib] access is -> [rw]\n");
        assert_eq!(
            String::from_utf8(log.err.clone()).unwrap(),
            "ERROR: Database gone non-existent or invalid\nERROR: Database empty non-existent or invalid\n\
             \nERROR: Invalid token : [bogus] in file [/home/me/DatabaseDir.rsrc].\n"
        );
        assert_eq!(dir.modified, 1);
        assert_eq!(dir.set_current_database(), Some(0));
        assert!(dir.check_local_database());

        // reading again adds nothing; without `all` it is not even opened
        let mut ctx = DirContext { fs: &fs, env: &env, lists: &std, shader: &mut shader, log: &mut log };
        assert!(database_dir_read(&mut dir, b"/home/me/DatabaseDir.rsrc", 0, true, &mut ctx));
        assert_eq!(dir.databases.len(), 5);
        assert!(!database_dir_read(&mut dir, b"/home/me/DatabaseDir.rsrc", 0, false, &mut ctx));
        assert!(!database_dir_read(&mut DbDir::new(), b"/nope", 0, true, &mut ctx));
    }

    #[test]
    fn first_valid_database_only() {
        let std = ElemLists::standard();
        let mut fs = sample_fs();
        fs.add_file(b"/cwd/DatabaseDir.rsrc", b"DATABASE gone PATH /dbs/gone DATABASE two PATH /dbs/two DATABASE one PATH /dbs/one ");
        fs.add_file(b"/sys/DatabaseSys.rsrc", b"DATABASE lib PATH $LIB ");
        let env = MapEnv::new(&[("LIB", "/lib/shaders"), ("SI_CUSTOM_SHADER", "/lib/shaders")], &[]);
        let (mut shader, mut log) = (ShaderDbRoot::default(), Log::default());
        let mut dir = DbDir::new();
        let mut ctx = DirContext { fs: &fs, env: &env, lists: &std, shader: &mut shader, log: &mut log };
        database_directory_load(&mut dir, b"/sys", false, b"/cwd", &mut ctx);
        // stops after "two"; the system list is then skipped because the list is not empty
        assert_eq!(dir.databases.len(), 2);
        assert_eq!(dir.set_current_database(), Some(1));

        let mut dir = DbDir::new();
        let mut ctx = DirContext { fs: &fs, env: &env, lists: &std, shader: &mut shader, log: &mut log };
        database_directory_load(&mut dir, b"/sys", true, b"/cwd", &mut ctx);
        assert_eq!(dir.databases.len(), 4);
        let lib = &dir.databases[3];
        assert_eq!((lib.system, lib.kind, lib.path.as_deref()), (1, DB_KIND_SHADERLIB, Some(&b"/lib/shaders"[..])));
        assert_eq!(shader.name.as_deref(), Some(&b"lib"[..]));

        // SI_DBDIR set but useless: both messages, then the current directory
        let env = MapEnv::new(&[("SI_DBDIR", "/nowhere")], &[]);
        let (mut shader, mut log) = (ShaderDbRoot::default(), Log::default());
        let mut dir = DbDir::new();
        let mut ctx = DirContext { fs: &fs, env: &env, lists: &std, shader: &mut shader, log: &mut log };
        database_directory_load(&mut dir, b"/sys", true, b"/cwd", &mut ctx);
        assert!(log.err.starts_with(b"\nERROR: Couldn't read the DatabaseDir.rsrc file from the path\n         specified by SI_DBDIR: [/nowhere/DatabaseDir.rsrc]\n"));
        assert_eq!(dir.databases.len(), 4);
    }

    #[test]
    fn database_list_write() {
        let mut dir = DbDir::new();
        dir.add_database(b"one", b"/dbs/one");
        let s = dir.add_database(b"sys", b"/dbs/sys");
        dir.databases[s].system = 1;
        let l = dir.add_database(b"lib", b"/lib/shaders");
        dir.databases[l].kind = DB_KIND_SHADERLIB;
        let env = MapEnv::default();
        let w = database_dir_write(&dir, b"/home/me", None, false, &env);
        assert_eq!(w.path, b"/home/me/DatabaseDir.rsrc");
        assert_eq!(w.data, b"DATABASE one \nPATH /dbs/one \nDATABASE lib \nPATH /lib/shaders \nTYPE SHADERLIB \n");
        let env = MapEnv::new(&[("SI_DBDIR", "/etc/si")], &[]);
        let w = database_dir_write(&dir, b"/home/me", None, true, &env);
        assert_eq!((w.path.as_slice(), w.append), (&b"/etc/si/DatabaseDir.rsrc"[..], true));
        assert_eq!(w.data, b"DATABASE lib \nPATH /lib/shaders \nTYPE SHADERLIB \n");
        assert_eq!(database_dir_write(&dir, b"/home/me", Some(b"/tmp/x"), false, &env).path, b"/tmp/x");

        let mut fs = MemFs::new();
        fs.add_dir(b"/etc/si");
        assert_eq!(write_database_dir(&mut fs, &mut dir, b"/home/me", None, false, &env), DIR_WRITTEN);
        assert_eq!(dir.modified, 0);
        assert_eq!(write_database_dir(&mut fs, &mut dir, b"/home/me", Some(b"/no/dir/f"), false, &env), DIR_NOT_WRITTEN);
    }

    #[test]
    fn searches() {
        let mut dir = DbDir::new();
        dir.add_database(b"demo", b"/dbs/demo");
        dir.add_database(b"demo2", b"/dbs/demo2");
        dir.add_database(b"work", b"/home/me/work");
        dir.add_database(b"BACKUP", b"/bak");
        assert_eq!(dir.search_database_from_name(b"demo"), (DB_FOUND, Some(0), 1));
        assert_eq!(dir.search_database_from_name(b"dem"), (DB_AMBIGUOUS, None, 2));
        assert_eq!(dir.search_database_from_name(b"wo"), (DB_FOUND, Some(2), 1));
        assert_eq!(dir.search_database_from_name(b"x"), (DB_NOT_FOUND, None, 0));
        assert_eq!(dir.search_database_from_path(b"/dbs/demo", SEARCH_EXACT), (DB_FOUND, Some(0), 1));
        assert_eq!(dir.search_database_from_path(b"/dbs", SEARCH_EXACT), (DB_NOT_FOUND, None, 0));
        assert_eq!(dir.search_database_from_path(b"/dbs", SEARCH_CONTAINS), (DB_AMBIGUOUS, None, 2));
        assert_eq!(dir.search_database_from_path(b"me/wo", SEARCH_CONTAINS), (DB_FOUND, Some(2), 1));
        assert_eq!(dir.search_database_from_path(b"/dbs/demo2/MODELS", SEARCH_INSIDE), (DB_FOUND, Some(1), 1));
        assert_eq!(dir.search_database_from_path(b"/dbs/demo/MODELS/x", SEARCH_INSIDE), (DB_FOUND, Some(0), 1));
        assert_eq!(dir.search_database_from_path(b"/dbs/demolition", SEARCH_INSIDE), (DB_NOT_FOUND, None, 0));
        assert_eq!(dir.get_backup_database(), Some(3));
        assert_eq!(dir.db_exist_in_list(b"demo", b"/dbs/demo2"), None);
        assert_eq!(dir.db_exist_in_list(b"demo2", b"/dbs/demo2"), Some(1));
        assert_eq!(dir.set_database(4), None);
        assert_eq!(dir.set_database(-1), None);
        assert_eq!(dir.set_database(3), Some(3));
        assert_eq!(dir.get_database_index(&dir.databases[2]), 2);
        assert_eq!(dir.get_database_index(&dir.databases[2].clone()), -1);
    }
}
