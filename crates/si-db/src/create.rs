// SPDX-License-Identifier: MIT
//! Creating a database, and the database write lock.

use crate::{
    check_database_existence, check_file_existence, concat, ed_get_suffix_from_chapter, filepath_set, write_chapter_rsrc,
    write_database_dir, Bytes, DbChap, DbDir, DbInfo, ElemLists, Env, FileSystem, FileSystemMut, MemFs, DB_FOUND,
    DEFAULT_VERSIONS, DIR_NOT_WRITTEN, F_OK, LIST_KIND_DATABASE, SEARCH_EXACT, W_OK,
};

/// Results of [`create_database`].
/// The directory above the new database is itself a database, or lies in one.
pub const CREATE_INSIDE_A_DATABASE: i32 = 0x3d;
/// A database with this name and path is already in the list.
pub const CREATE_ALREADY_LISTED: i32 = 0x3e;
/// Something already exists at the path.
pub const CREATE_PATH_EXISTS: i32 = 0x3f;
/// The database directory could not be made.
pub const CREATE_NO_DIRECTORY: i32 = 0x40;
/// A chapter directory or its `Chapter.rsrc` could not be made; the database stays in the
/// list, marked invalid.
pub const CREATE_NO_CHAPTERS: i32 = 0x41;
/// Everything was made but the list of databases could not be written.
pub const CREATE_LIST_NOT_WRITTEN: i32 = 0x42;
pub const CREATE_DONE: i32 = 0x43;

/// Makes one chapter of a database [soft 0x08126b50 SI_dbDatabaseCreateAChapter]:
/// the directory `<database>/<chapter name>` [soft 0x08120eb0 dbMakeDir] and in it a
/// `Chapter.rsrc` with the chapter's suffix from the program's table and 4 versions.
/// `lists` is the dictionary and `chapter_id` the list in it. Only a list of the database
/// kind gets a directory; false if the directory or the file could not be made.
///
/// Unless `quiet`, a chapter the database record does not know yet is added to it, with
/// that suffix and 4 versions, **whether or not it has a directory**: the record of a newly
/// created database lists all 36 chapters, three of them without a directory.
pub fn database_create_a_chapter(fs: &mut dyn FileSystemMut, lists: &ElemLists, info: &mut DbInfo, chapter_id: i32, quiet: bool) -> bool {
    let list = &lists.lists[chapter_id as usize];
    let name = &list.name[..];
    let suffix = ed_get_suffix_from_chapter(chapter_id).as_bytes().to_vec();
    let chap = DbChap { name: name.to_vec(), suffix: Some(suffix), browser: None, versions: DEFAULT_VERSIONS };
    let mut ok = true;
    if list.kind == LIST_KIND_DATABASE {
        let db = info.path.as_deref().unwrap_or(b"");
        ok = fs.mkdir(&filepath_set(&[db, b"/", name]))
            && fs.write(&filepath_set(&[db, b"/", name, b"/Chapter.rsrc"]), &write_chapter_rsrc(&chap), false);
    }
    if !quiet && info.chapter_index(name) == -1 {
        info.add_chapter(name);
        let i = info.chapter_index(name) as usize;
        info.chapters[i] = chap;
        info.sort_chapters();
    }
    ok
}

/// Makes every chapter of the dictionary, in id order; after the first that fails no
/// further one is attempted [soft 0x08126ce0 dbDatabaseCreateChapters].
pub fn database_create_chapters(fs: &mut dyn FileSystemMut, lists: &ElemLists, info: &mut DbInfo, quiet: bool) -> bool {
    let mut ok = true;
    for id in 0..lists.lists.len() as i32 {
        ok = ok && database_create_a_chapter(fs, lists, info, id, quiet);
    }
    ok
}

/// POSIX `dirname`, which the original calls on a copy of the new database's path.
fn dirname(path: &[u8]) -> Bytes {
    let trimmed = match path.iter().rposition(|&c| c != b'/') {
        Some(i) => &path[..=i],
        None if path.is_empty() => return b".".to_vec(),
        None => return b"/".to_vec(),
    };
    match trimmed.iter().rposition(|&c| c == b'/') {
        None => b".".to_vec(),
        Some(s) => match trimmed[..s].iter().rposition(|&c| c != b'/') {
            Some(i) => trimmed[..=i].to_vec(),
            None => b"/".to_vec(),
        },
    }
}

/// Creates a database [soft 0x08126d3c SI_dbCreateDatabase]. Returns one of the `CREATE_`
/// codes and the position of the new database in the list, when it got that far.
///
/// In order: refuse a name and path already listed; refuse when the parent directory is a
/// listed database or looks like one ([`check_database_existence`]); refuse a path that
/// exists; make the directory; add the database to the list; make the chapters
/// ([`database_create_chapters`]); and with `write_list`, append the new database to the
/// user's list of databases ([`write_database_dir`]).
///
/// (For a path without any slash the original frees the constant string `dirname` returns.)
pub fn create_database(
    fs: &mut dyn FileSystemMut,
    lists: &ElemLists,
    dir: &mut DbDir,
    name: &[u8],
    path: &[u8],
    home: &[u8],
    write_list: bool,
    env: &dyn Env,
) -> (i32, Option<usize>) {
    if dir.db_exist_in_list(name, path).is_some() {
        return (CREATE_ALREADY_LISTED, None);
    }
    let parent = dirname(path);
    if dir.search_database_from_path(&parent, SEARCH_EXACT).0 == DB_FOUND
        || check_database_existence(fs, &parent, lists)
    {
        return (CREATE_INSIDE_A_DATABASE, None);
    }
    if fs.access(path, F_OK) {
        return (CREATE_PATH_EXISTS, None);
    }
    if !fs.mkdir(path) {
        return (CREATE_NO_DIRECTORY, None);
    }
    let i = dir.add_database(name, path);
    if !database_create_chapters(fs, lists, &mut dir.databases[i], false) {
        dir.databases[i].valid = 0;
        return (CREATE_NO_CHAPTERS, Some(i));
    }
    if write_list && write_database_dir(fs, dir, home, None, true, env) == DIR_NOT_WRITTEN {
        return (CREATE_LIST_NOT_WRITTEN, Some(i));
    }
    (CREATE_DONE, Some(i))
}

/// Makes the directory and the chapters of a database record that is not in the list
/// [soft 0x08126fcc SI_dbCreateExtDatabase]. The record itself is left alone.
pub fn create_ext_database(fs: &mut dyn FileSystemMut, lists: &ElemLists, info: &mut DbInfo) -> bool {
    let Some(path) = info.path.clone() else { return false };
    fs.mkdir(&path) && database_create_chapters(fs, lists, info, true)
}

/// Makes sure a chapter has its `SHARE` sub-directory [soft 0x08127020
/// SI_dbCheckCreateShareSubdir]: 1 when it was there and can be written to, 0 when it was
/// made, −1 when making it failed, −2 when it cannot be written to. The chapter becomes
/// the current chapter of `info`.
pub fn check_create_share_subdir(fs: &mut dyn FileSystemMut, info: &mut DbInfo, chapter: &[u8]) -> i32 {
    info.set_chapter(chapter);
    info.filepath_build();
    let share = concat(&[info.chapter_path.as_deref().unwrap_or(b""), b"SHARE"]);
    let existed = fs.access(&share, F_OK);
    if !existed && !fs.mkdir(&share) {
        return -1;
    }
    match (fs.access(&share, W_OK), existed) {
        (false, _) => -2,
        (true, true) => 1,
        (true, false) => 0,
    }
}

/// One thing creating a database makes.
#[derive(Clone, Debug, PartialEq)]
pub enum Created {
    Dir(Bytes),
    File { path: Bytes, data: Bytes },
}

/// What [`create_database`] would make for a database at `path` with the standard
/// dictionary, in the order it makes it: the database directory, then for each of the 33 database chapters its directory and
/// its `Chapter.rsrc`. (The list of databases, which is appended to, is not included.)
pub fn creation_plan(name: &[u8], path: &[u8]) -> Vec<Created> {
    let mut fs = MemFs::new();
    let parent = dirname(path);
    fs.add_dir(&parent);
    let before = fs.paths().count();
    let mut dir = DbDir::new();
    create_database(&mut fs, &ElemLists::standard(), &mut dir, name, path, b"", false, &crate::MapEnv::default());
    let made: Vec<(Bytes, bool)> = fs.paths().skip(before).map(|(p, d)| (p.to_vec(), d)).collect();
    made.into_iter()
        .map(|(p, is_dir)| if is_dir { Created::Dir(p) } else { Created::File { data: fs.read(&p).unwrap_or_default(), path: p } })
        .collect()
}

/// What to do with the write lock of a database.
pub const LOCK_TRY: i32 = 1;
pub const LOCK_FORCE: i32 = 2;
pub const LOCK_RELEASE: i32 = 0;

/// The lock file of a database: `<database>/SI_DB_LOCK__`.
pub fn lock_file_path(info: &DbInfo) -> Bytes {
    concat(&[info.path.as_deref().unwrap_or(b""), b"/SI_DB_LOCK__"])
}

/// Takes, forces or releases the write lock of a database [soft 0x081210e0
/// SI_dbSetDbWriteLock]. Returns the routine's result and, when the lock is held by
/// someone else, the text describing the holder (the original leaves it in the global
/// `SI_dBWorkFnameExt`).
///
/// - [`LOCK_TRY`]: if the lock file exists (is readable), the lock is refused: result
///   false, and the holder text is the file's first line, at most 99 characters, or
///   `Lock in progress. (file not readable)` if it cannot be opened after all.
///   Otherwise as forcing.
/// - [`LOCK_FORCE`]: the file is written: `Locked by user <user> on <date> `, where the
///   date is the local time as `yy/mm/dd HH:MM` (passed in here as text). **The result is
///   true even when the file could not be written.**
/// - anything else: the file is removed; true if it was there and the removal worked.
pub fn set_db_write_lock(fs: &mut dyn FileSystemMut, info: &DbInfo, mode: i32, user: &[u8], date: &[u8]) -> (bool, Option<Bytes>) {
    let path = lock_file_path(info);
    let exists = check_file_existence(fs, &path);
    if mode != LOCK_TRY && mode != LOCK_FORCE {
        return (exists && fs.unlink(&path), None);
    }
    if exists && mode == LOCK_TRY {
        let holder = match fs.read(&path) {
            None => b"Lock in progress. (file not readable)".to_vec(),
            Some(data) => {
                // fgets with a buffer of 100
                let line = data.iter().position(|&c| c == b'\n').map_or(data.len(), |i| i + 1).min(99);
                data[..line].to_vec()
            }
        };
        return (false, Some(holder));
    }
    fs.write(&path, &concat(&[b"Locked by user ", user, b" on ", date, b" "]), false);
    (true, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEnv, DB_KIND_NORMAL};

    #[test]
    fn plan() {
        let plan = creation_plan(b"demo", b"/dbs/demo");
        assert_eq!(plan.len(), 1 + 2 * 33);
        assert_eq!(plan[0], Created::Dir(b"/dbs/demo".to_vec()));
        assert_eq!(plan[1], Created::Dir(b"/dbs/demo/SCENES".to_vec()));
        assert_eq!(plan[2], Created::File { path: b"/dbs/demo/SCENES/Chapter.rsrc".to_vec(), data: b"SUFFIX dsc \nVERSIONS 4 ".to_vec() });
        let dirs: Vec<&[u8]> = plan.iter().skip(1).filter_map(|c| if let Created::Dir(p) = c { Some(&p[10..]) } else { None }).collect();
        assert!(!dirs.contains(&&b"NSELECTIONS"[..]) && !dirs.contains(&&b"CHANNEL_DRIVERS"[..]) && !dirs.contains(&&b"OUTSIDE_DB_FILES"[..]));
        assert_eq!(dirs[4], b"PICTURES");
        assert_eq!(*dirs.last().unwrap(), b"LIGHT_PHOTON_SHADERS");
        let rsrc = |ch: &str| {
            plan.iter()
                .find_map(|c| match c {
                    Created::File { path, data } if path.ends_with(format!("/{ch}/Chapter.rsrc").as_bytes()) => Some(data.clone()),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(rsrc("PICTURES"), b"SUFFIX pic \nVERSIONS 4 ");
        assert_eq!(rsrc("TEXTURES3D"), b"SUFFIX t3D \nVERSIONS 4 ");
        assert_eq!(rsrc("AUDIO"), b"SUFFIX aiff \nVERSIONS 4 ");
    }

    #[test]
    fn create_and_refuse() {
        let std = ElemLists::standard();
        let env = MapEnv::default();
        let mut fs = MemFs::new();
        fs.add_dir(b"/dbs").add_dir(b"/home/me");
        let mut dir = DbDir::new();
        let (code, at) = create_database(&mut fs, &std, &mut dir, b"demo", b"/dbs/demo", b"/home/me", true, &env);
        assert_eq!((code, at), (CREATE_DONE, Some(0)));
        assert_eq!(fs.read(b"/home/me/DatabaseDir.rsrc").unwrap(), b"DATABASE demo \nPATH /dbs/demo \n");
        let d = &dir.databases[0];
        assert_eq!((d.valid, d.kind, d.chapters.len()), (1, DB_KIND_NORMAL, 36));
        // the record also lists the three chapters that have no directory
        let ns = d.chapters.iter().find(|c| c.name == b"NSELECTIONS").unwrap();
        assert_eq!(ns.suffix.as_deref(), Some(&b"???"[..]));
        assert!(!fs.access(b"/dbs/demo/NSELECTIONS", F_OK));
        assert!(d.chapters.windows(2).all(|w| w[0].name < w[1].name));

        assert_eq!(create_database(&mut fs, &std, &mut dir, b"demo", b"/dbs/demo", b"/home/me", true, &env).0, CREATE_ALREADY_LISTED);
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"other", b"/dbs/demo", b"/home/me", true, &env).0, CREATE_PATH_EXISTS);
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"in", b"/dbs/demo/inner", b"/home/me", true, &env).0, CREATE_INSIDE_A_DATABASE);
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"in", b"/dbs/demo/MODELS/x", b"/home/me", true, &env).0, CREATE_INSIDE_A_DATABASE);
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"no", b"/nowhere/x", b"/home/me", true, &env).0, CREATE_NO_DIRECTORY);
        // a second database is appended to the list file
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"two", b"/dbs/two", b"/home/me", true, &env), (CREATE_DONE, Some(1)));
        assert_eq!(fs.read(b"/home/me/DatabaseDir.rsrc").unwrap(), b"DATABASE demo \nPATH /dbs/demo \nDATABASE two \nPATH /dbs/two \n");
        assert_eq!(create_database(&mut fs, &std, &mut dir, b"three", b"/dbs/three", b"/no/home", true, &env).0, CREATE_LIST_NOT_WRITTEN);
    }

    #[test]
    fn share_and_external() {
        let std = ElemLists::standard();
        let mut fs = MemFs::new();
        fs.add_dir(b"/ext");
        let mut info = DbInfo::new();
        info.path = Some(b"/ext/db".to_vec());
        assert!(create_ext_database(&mut fs, &std, &mut info));
        assert!(info.chapters.is_empty()); // quiet: the record is not filled
        assert!(fs.access(b"/ext/db/MODELS/Chapter.rsrc", F_OK));
        assert!(!create_ext_database(&mut fs, &std, &mut info)); // the directory is there already
        assert_eq!(check_create_share_subdir(&mut fs, &mut info, b"MODELS"), 0);
        assert_eq!(check_create_share_subdir(&mut fs, &mut info, b"MODELS"), 1);
        assert_eq!(check_create_share_subdir(&mut fs, &mut info, b"NOPE"), -1);
    }

    #[test]
    fn directory_names() {
        for (p, d) in [("/a/b", "/a"), ("/a/b/", "/a"), ("/a", "/"), ("/", "/"), ("a", "."), ("", "."), ("a/b//c//", "a/b")] {
            assert_eq!(dirname(p.as_bytes()), d.as_bytes(), "{p}");
        }
    }

    #[test]
    fn lock() {
        let mut fs = MemFs::new();
        fs.add_dir(b"/db");
        let mut info = DbInfo::new();
        info.path = Some(b"/db".to_vec());
        assert_eq!(set_db_write_lock(&mut fs, &info, LOCK_RELEASE, b"me", b""), (false, None));
        assert_eq!(set_db_write_lock(&mut fs, &info, LOCK_TRY, b"me", b"26/10/13 09:45"), (true, None));
        assert_eq!(fs.read(b"/db/SI_DB_LOCK__").unwrap(), b"Locked by user me on 26/10/13 09:45 ");
        let (ok, holder) = set_db_write_lock(&mut fs, &info, LOCK_TRY, b"you", b"26/10/13 09:50");
        assert_eq!((ok, holder.as_deref()), (false, Some(&b"Locked by user me on 26/10/13 09:45 "[..])));
        assert_eq!(set_db_write_lock(&mut fs, &info, LOCK_FORCE, b"you", b"26/10/13 09:51"), (true, None));
        assert_eq!(fs.read(b"/db/SI_DB_LOCK__").unwrap(), b"Locked by user you on 26/10/13 09:51 ");
        assert_eq!(set_db_write_lock(&mut fs, &info, 3, b"", b""), (true, None));
        assert!(!fs.access(b"/db/SI_DB_LOCK__", F_OK));
        // forcing where nothing can be written still reports success
        info.path = Some(b"/gone".to_vec());
        assert_eq!(set_db_write_lock(&mut fs, &info, LOCK_FORCE, b"me", b""), (true, None));
    }
}
