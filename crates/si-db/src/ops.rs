// SPDX-License-Identifier: MIT
//! Moving, deleting and protecting files; who owns a database.
//!
//! Several of these hand a command line to the shell (`mv`, `cp`, `rcp`, `rm`), exactly as
//! the original does through `system`: paths are pasted into the line unquoted.

use crate::{
    chapter_get_nb_files, check_file_existence, concat, or_null, Bytes, DbChap, DbDir, DbInfo, Env, FileSystem,
    FileSystemMut, ListNode, Session, LIST_KIND_EXTERNAL, W_OK,
};

/// Runs a command line through the shell [soft 0x08126748 SI_system]. (Beside it in the
/// binary sits a signal handler that does nothing and that nothing refers to
/// [soft 0x08126760 system_handler].)
pub fn system(fs: &mut dyn FileSystemMut, command: &[u8]) -> i32 {
    fs.system(command)
}

/// Runs `<command> <argument>` [soft 0x08126964 SI_dbSystemCommand].
pub fn system_command(fs: &mut dyn FileSystemMut, command: &[u8], argument: &[u8]) -> i32 {
    system(fs, &concat(&[command, b" ", argument]))
}

/// The low 16 bits of a file's mode, 0 when the file cannot be examined
/// [soft 0x08120fdc SI_dbGetFileAccess]. Callers look at the result as a signed character:
/// "negative" means bit 7 is set, which is the owner's write permission.
pub fn get_file_access(fs: &dyn FileSystem, path: &[u8]) -> u16 {
    fs.stat(path).map_or(0, |st| st.mode as u16)
}

/// The test the original writes as "the access word, taken as a signed character, is
/// negative". A file that cannot be examined fails it.
fn owner_may_write(fs: &dyn FileSystem, path: &[u8]) -> bool {
    (get_file_access(fs, path) as u8 as i8) < 0
}

/// The debugging dump of a mode word [soft 0x08120f74 SI_dbFileAccessDump]: the value in
/// hexadecimal and its sixteen bits, highest first.
pub fn file_access_dump(mode: u16) -> Bytes {
    let bits: String = (0..16).rev().map(|b| if mode & (1 << b) != 0 { '1' } else { '0' }).collect();
    format!(" File acces = {mode:X}  [{bits}]\n").into_bytes()
}

/// Write-protects a file or lifts the protection [soft 0x08121048 SI_dbSetFileLock]: with
/// `lock` the three write permissions are removed, without it all three are granted.
/// False when the file cannot be examined or changed.
pub fn set_file_lock(fs: &mut dyn FileSystemMut, path: &[u8], lock: bool) -> bool {
    let Some(st) = fs.stat(path) else { return false };
    fs.chmod(path, if lock { st.mode & !0o222 } else { st.mode | 0o222 })
}

/// Who owns a database, for display [soft 0x08120de0 SI_dbOwnerString]: `loc:<user>` for a
/// database of the user's list, `sys:<user>` for a system one, the user being the owner of
/// the database directory; `loc:unknown` / `sys:unknown` when the directory cannot be
/// examined. `None` when the owner's user id has no name: the original then leaves the
/// caller's buffer as it was.
pub fn owner_string(fs: &dyn FileSystem, env: &dyn Env, info: &DbInfo) -> Option<Bytes> {
    let kind: &[u8] = if info.system != 0 { b"sys:" } else { b"loc:" };
    match fs.stat(or_null(&info.path)) {
        None => Some(concat(&[kind, b"unknown"])),
        Some(st) => env.user_name(st.uid).map(|user| concat(&[kind, &user])),
    }
}

/// The debugging dump of the list of databases [soft 0x081212b8 SI_dbDirDump]: name, owner
/// and path of every database (or of `only`), with its chapters when asked. Databases whose
/// field at offset 0x1e is zero are left out.
///
/// Where this differs: for an owner without a name the original prints its uninitialised
/// buffer; here the brackets are empty.
pub fn dir_dump(fs: &dyn FileSystem, env: &dyn Env, dir: &DbDir, only: Option<usize>, chapters: bool) -> Bytes {
    let mut out = Vec::new();
    for (i, d) in dir.databases.iter().enumerate() {
        if only.is_some_and(|o| o != i) || d.w1e == 0 {
            continue;
        }
        let owner = owner_string(fs, env, d).unwrap_or_default();
        out.extend_from_slice(&concat(&[
            b" Database : ",
            or_null(&d.name),
            b" ------ [",
            &owner,
            b"] \n Path     : ",
            or_null(&d.path),
            b"\n",
        ]));
        if chapters {
            out.extend_from_slice(&d.chap_info_dump());
        }
    }
    out
}

/// Moves or copies a file with a shell command [soft 0x081268d4 SI_dbFileMove]:
/// `mv <from> <to>`, or with `copy`, `cp <from> <to>` (`rcp` when `remote`). A move with
/// `check_access` is refused when the owner may not write the file. True when the command
/// reports success.
pub fn file_move(fs: &mut dyn FileSystemMut, from: &[u8], to: &[u8], copy: bool, remote: bool, check_access: bool) -> bool {
    let command = if !copy {
        if check_access && !owner_may_write(fs, from) {
            return false;
        }
        concat(&[b"mv ", from, b" ", to])
    } else {
        concat(&[if remote { &b"rcp"[..] } else { b"cp" }, b" ", from, b" ", to])
    };
    system(fs, &command) == 0
}

/// Moves or copies the icon file that goes with an element file
/// [soft 0x08126768 SI_dbFileMoveIcon]. The icon of `…/prefix-name.V-S.suffix` is
/// `…/prefix-name.icn`: the path with its last two dotted pieces removed. Nothing is done,
/// and the result is true, when either path has fewer than two dots or the icon is not
/// there. A move is refused (false) when the owner may not write the icon.
pub fn file_move_icon(fs: &mut dyn FileSystemMut, from: &[u8], to: &[u8], copy: bool, remote: bool) -> bool {
    fn icon(path: &[u8]) -> Option<Bytes> {
        let a = path.iter().rposition(|&c| c == b'.')?;
        let b = path[..a].iter().rposition(|&c| c == b'.')?;
        Some(concat(&[&path[..b], b".icn"]))
    }
    let Some(icon_from) = icon(from) else { return true };
    if !check_file_existence(fs, &icon_from) {
        return true;
    }
    let Some(icon_to) = icon(to) else { return true };
    let verb: &[u8] = if copy {
        if remote {
            b"rcp"
        } else {
            b"cp"
        }
    } else {
        if !owner_may_write(fs, &icon_from) {
            return false;
        }
        b"mv"
    };
    system(fs, &concat(&[verb, b" ", &icon_from, b" ", &icon_to])) == 0
}

/// Deletes the file of a list element [soft 0x08125264 SI_dbfileDelete]. Returns 1 when
/// it is gone, 0 when deleting failed, −1 when the owner may not write the file **or the
/// file is not there** (the permission test fails the same way for both).
///
/// The path is the chapter directory of `info`, the node's sub-directory and its full
/// name; for a list of files outside a database (`list_kind` 2) it is the list's own
/// directory (`list_dir`), a slash, and the same two. With `add_suffix` the suffix the
/// database has for the chapter `list_name` is added. With `wildcard` a `*` is put after
/// the name (before the suffix) and the lot is given to `rm` through the shell, with no
/// permission test; otherwise the one file is unlinked.
#[allow(clippy::too_many_arguments)]
pub fn file_delete(
    fs: &mut dyn FileSystemMut,
    info: &DbInfo,
    list_name: &[u8],
    list_kind: i16,
    list_dir: Option<&[u8]>,
    node: &ListNode,
    add_suffix: bool,
    wildcard: bool,
) -> i32 {
    let mut path = if list_kind == LIST_KIND_EXTERNAL {
        concat(&[list_dir.unwrap_or(crate::NULL_TEXT), b"/", or_null(&node.sub_path), or_null(&node.full)])
    } else {
        concat(&[or_null(&info.chapter_path), or_null(&node.sub_path), or_null(&node.full)])
    };
    if wildcard {
        path.push(b'*');
    }
    if add_suffix {
        if let Some(suffix) = info.suffix_of(list_name) {
            path = concat(&[&path, b".", suffix]);
        }
    }
    if owner_may_write(fs, &path) {
        if !wildcard {
            return fs.unlink(&path) as i32;
        }
    } else if !wildcard {
        return -1;
    }
    (system_command(fs, b"rm", &path) == 0) as i32
}

/// Removes a directory tree with `rm -rf` [soft 0x081270cc SI_dbDeleteExtDatabase]. False,
/// and nothing run, unless the directory can be written to; otherwise true, whatever the
/// command reports.
pub fn delete_ext_database(fs: &mut dyn FileSystemMut, path: Option<&[u8]>) -> bool {
    match path {
        Some(p) if fs.access(p, W_OK) => {
            system(fs, &concat(&[b"rm -rf ", p]));
            true
        }
        _ => false,
    }
}

/// Results of [`chapter_delete`].
pub const CHAPTER_NOT_DELETED: i32 = 0x55;
pub const CHAPTER_NOT_EMPTY: i32 = 0x56;
pub const CHAPTER_DELETED: i32 = 0x57;

/// Removes a chapter directory of a database [soft 0x08127284 SI_dbChapterDelete]; with
/// `only_if_empty` it is first counted ([`chapter_get_nb_files`]) and kept if it holds
/// anything.
pub fn chapter_delete(fs: &mut dyn FileSystemMut, now: i32, info: &mut DbInfo, chap: &DbChap, only_if_empty: bool) -> i32 {
    if only_if_empty && chapter_get_nb_files(&Session { fs: &*fs, now, loading: false }, info, chap) > 0 {
        return CHAPTER_NOT_EMPTY;
    }
    let path = concat(&[or_null(&info.path), b"/", &chap.name]);
    if delete_ext_database(fs, Some(&path)) {
        CHAPTER_DELETED
    } else {
        CHAPTER_NOT_DELETED
    }
}

/// Renames a database directory [soft 0x08126ebc SI_dbRenameExtDatabase]: the new path is
/// the old one with its last `strlen(name)` characters replaced by `new_name`, where
/// `name` is the database's name in the list. So the directory is assumed to be called
/// like the database; when it is not, the wrong tail is cut off. Returns what `rename`
/// returns (0 or −1), −1 for a record without a path. The record is not changed.
pub fn rename_ext_database(fs: &mut dyn FileSystemMut, info: &DbInfo, new_name: &[u8]) -> i32 {
    let Some(path) = info.path.as_deref() else { return -1 };
    let cut = info.name.as_deref().map_or(0, |n| n.len());
    // (a name longer than the path makes the original write before its buffer)
    let keep = path.len().saturating_sub(cut);
    if fs.rename(path, &concat(&[&path[..keep], new_name])) {
        0
    } else {
        -1
    }
}

/// What `SI_byteswap_fwrite(piece, len, 1, f)` puts in the file: the routine takes its
/// second argument for an item size, so a piece of exactly 2, 4 or 8 bytes is written
/// byte-reversed (and restored in memory afterwards).
fn written_as_one_item(piece: &[u8]) -> Bytes {
    let mut out = piece.to_vec();
    if matches!(out.len(), 2 | 4 | 8) {
        out.reverse();
    }
    out
}

/// Looks for a byte string in a file and optionally replaces its first occurrence
/// [soft 0x081273fc SI_dbfileSeekEditString]. With `whole` the occurrence must be followed
/// by a zero byte. Returns whether it was found.
///
/// To replace, the original writes a new file `/usr/tmp/SI_tmp.<pid>` (the part before,
/// the replacement, the part after) and then runs `mv` on it through the shell, whether or
/// not the temporary file could be written.
///
/// **Kept defect:** the three parts are written with the byte-swapping writer, each as one
/// item of its own length. A part that is exactly 2, 4 or 8 bytes long therefore reaches
/// the file with its bytes reversed. An occurrence at offset 4 of a binary file turns the
/// file's magic number around.
///
/// Where this differs: near the end of the file the original compares bytes past the end
/// of its buffer (and with `whole`, tests the byte after it); here an occurrence must lie
/// inside the file, and for `whole` the following byte must exist.
pub fn file_seek_edit_string(
    fs: &mut dyn FileSystemMut,
    file: &[u8],
    needle: &[u8],
    replacement: Option<&[u8]>,
    whole: bool,
    pid: i32,
) -> bool {
    let Some(data) = fs.read(file) else { return false };
    let at = (0..data.len()).find(|&pos| {
        data[pos..].starts_with(needle) && (!whole || data.get(pos + needle.len()) == Some(&0))
    });
    let (Some(at), Some(new)) = (at, replacement) else { return at.is_some() };
    let tmp = format!("/usr/tmp/SI_tmp.{pid}").into_bytes();
    let content = concat(&[
        &written_as_one_item(&data[..at]),
        &written_as_one_item(new),
        &written_as_one_item(&data[at + needle.len()..]),
    ]);
    fs.write(&tmp, &content, false);
    fs.system(&concat(&[b"mv ", &tmp, b" ", file]));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scn_get_list_node, MapEnv, MemFs, LIST_KIND_DATABASE};

    fn fs() -> MemFs {
        let mut fs = MemFs::new();
        fs.add_file(b"/db/MODELS/Bike-A.2-0.hrc", b"m");
        fs.add_file(b"/db/MODELS/Bike-A.icn", b"i");
        fs.add_file_with(b"/db/MODELS/ro.1-0.hrc", b"m", 0, 0o444);
        fs.add_file_with(b"/db/MODELS/ro.icn", b"i", 0, 0o444);
        fs
    }

    #[test]
    fn access_and_lock() {
        let mut fs = fs();
        assert_eq!(get_file_access(&fs, b"/db/MODELS/Bike-A.2-0.hrc"), 0x81a4);
        assert_eq!(get_file_access(&fs, b"/nope"), 0);
        assert_eq!(file_access_dump(0x81a4), b" File acces = 81A4  [1000000110100100]\n");
        assert!(set_file_lock(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", true));
        assert_eq!(get_file_access(&fs, b"/db/MODELS/Bike-A.2-0.hrc"), 0x8124);
        assert!(set_file_lock(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", false));
        assert_eq!(get_file_access(&fs, b"/db/MODELS/Bike-A.2-0.hrc"), 0x81b6);
        assert!(!set_file_lock(&mut fs, b"/nope", true));
    }

    #[test]
    fn moves_are_shell_commands() {
        let mut fs = fs();
        assert!(file_move(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", b"/x/Bike-A.2-0.hrc", false, false, true));
        assert!(!file_move(&mut fs, b"/db/MODELS/ro.1-0.hrc", b"/x/ro.1-0.hrc", false, false, true));
        assert!(file_move(&mut fs, b"/db/MODELS/ro.1-0.hrc", b"/x/ro.1-0.hrc", false, false, false));
        assert!(file_move(&mut fs, b"a b", b"c", true, false, true));
        assert!(file_move(&mut fs, b"a", b"host:c", true, true, true));
        assert!(file_move_icon(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", b"/x/New-A.1-0.hrc", false, false));
        assert!(file_move_icon(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", b"/x/New-A.1-0.hrc", true, true));
        assert!(!file_move_icon(&mut fs, b"/db/MODELS/ro.1-0.hrc", b"/x/ro.1-0.hrc", false, false));
        // nothing to do: no icon, or a name with fewer than two dots
        assert!(file_move_icon(&mut fs, b"/db/MODELS/ghost.1-0.hrc", b"/x/g.1-0.hrc", false, false));
        assert!(file_move_icon(&mut fs, b"/db/MODELS/plain.hrc", b"/x/plain.hrc", false, false));
        assert!(file_move_icon(&mut fs, b"/db/MODELS/Bike-A.2-0.hrc", b"plain.hrc", false, false));
        let cmds: Vec<String> = fs.commands.iter().map(|c| String::from_utf8(c.clone()).unwrap()).collect();
        assert_eq!(
            cmds,
            [
                "mv /db/MODELS/Bike-A.2-0.hrc /x/Bike-A.2-0.hrc",
                "mv /db/MODELS/ro.1-0.hrc /x/ro.1-0.hrc",
                "cp a b c",
                "rcp a host:c",
                "mv /db/MODELS/Bike-A.icn /x/New-A.icn",
                "rcp /db/MODELS/Bike-A.icn /x/New-A.icn",
            ]
        );
        fs.system_status = 256;
        assert!(!file_move(&mut fs, b"a", b"b", true, false, false));
    }

    #[test]
    fn deleting() {
        let mut fs = fs();
        let mut info = DbInfo::new();
        info.path = Some(b"/db".to_vec());
        info.set_chapter(b"MODELS");
        info.filepath_build();
        info.add_chapter(b"MODELS");
        info.chapters[0].suffix = Some(b"hrc".to_vec());
        let node = scn_get_list_node(b"Bike-A.2-0.hrc", None, true, 0);
        let ro = scn_get_list_node(b"ro.1-0.hrc", None, true, 0);
        assert_eq!(file_delete(&mut fs, &info, b"MODELS", LIST_KIND_DATABASE, None, &ro, true, false), -1);
        assert_eq!(file_delete(&mut fs, &info, b"MODELS", LIST_KIND_DATABASE, None, &node, true, false), 1);
        assert_eq!(file_delete(&mut fs, &info, b"MODELS", LIST_KIND_DATABASE, None, &node, true, false), -1); // gone already
        assert_eq!(file_delete(&mut fs, &info, b"MODELS", LIST_KIND_DATABASE, None, &node, true, true), 1);
        assert_eq!(file_delete(&mut fs, &info, b"X", LIST_KIND_EXTERNAL, Some(b"/ext"), &node, true, true), 1);
        assert_eq!(fs.commands, [b"rm /db/MODELS/Bike-A.2-0*.hrc".to_vec(), b"rm /ext/Bike-A.2-0*".to_vec()]);

        let chap = info.chapters[0].clone();
        assert_eq!(chapter_delete(&mut fs, 0, &mut info, &chap, true), CHAPTER_NOT_EMPTY);
        assert_eq!(chapter_delete(&mut fs, 0, &mut info, &chap, false), CHAPTER_DELETED);
        assert_eq!(fs.commands.last().unwrap(), b"rm -rf /db/MODELS");
        let none = DbChap { name: b"NONE".to_vec(), suffix: None, browser: None, versions: 4 };
        assert_eq!(chapter_delete(&mut fs, 0, &mut info, &none, true), CHAPTER_NOT_DELETED);
        assert!(!delete_ext_database(&mut fs, None));
    }

    #[test]
    fn seek_and_edit() {
        let mut fs = MemFs::new();
        fs.add_dir(b"/usr/tmp");
        fs.add_file(b"/f", b"head:/old/path\0tail of the file");
        assert!(file_seek_edit_string(&mut fs, b"/f", b"/old/path", None, true, 7));
        assert!(!file_seek_edit_string(&mut fs, b"/f", b"/old", None, true, 7));
        assert!(file_seek_edit_string(&mut fs, b"/f", b"/old", None, false, 7));
        assert!(!file_seek_edit_string(&mut fs, b"/f", b"nothing", Some(b"x"), false, 7));
        assert!(!file_seek_edit_string(&mut fs, b"/missing", b"a", None, false, 7));
        assert!(fs.commands.is_empty());
        assert!(file_seek_edit_string(&mut fs, b"/f", b"/old/path", Some(b"/a/new/place"), true, 7));
        assert_eq!(fs.read(b"/usr/tmp/SI_tmp.7").unwrap(), b"head:/a/new/place\0tail of the file");
        assert_eq!(fs.commands, [b"mv /usr/tmp/SI_tmp.7 /f".to_vec()]);
        // parts of 2, 4 or 8 bytes come out reversed
        fs.add_file(b"/g", b"ABCDneedleXY");
        assert!(file_seek_edit_string(&mut fs, b"/g", b"needle", Some(b"12345678"), false, 8));
        assert_eq!(fs.read(b"/usr/tmp/SI_tmp.8").unwrap(), b"DCBA87654321YX");
    }

    #[test]
    fn renaming_and_owners() {
        let mut fs = fs();
        let mut dir = DbDir::new();
        let i = dir.add_database(b"db", b"/db");
        assert_eq!(rename_ext_database(&mut fs, &dir.databases[i], b"new"), 0);
        assert!(fs.access(b"/new/MODELS/Bike-A.icn", crate::F_OK));
        assert_eq!(rename_ext_database(&mut fs, &dir.databases[i], b"again"), -1);
        // the cut is by the length of the list name, whatever the directory is called
        fs.add_dir(b"/dbs/project");
        let j = dir.add_database(b"prj", b"/dbs/project");
        assert_eq!(rename_ext_database(&mut fs, &dir.databases[j], b"X"), 0);
        assert!(fs.access(b"/dbs/projX", crate::F_OK));

        let mut env = MapEnv::default();
        env.users.push((0, b"root".to_vec()));
        dir.databases[i].path = Some(b"/new".to_vec());
        dir.databases[j].system = 1;
        assert_eq!(owner_string(&fs, &env, &dir.databases[i]).unwrap(), b"loc:root");
        assert_eq!(owner_string(&fs, &env, &dir.databases[j]).unwrap(), b"sys:unknown");
        assert_eq!(owner_string(&fs, &MapEnv::default(), &dir.databases[i]), None);
        dir.databases[i].add_chapter(b"MODELS");
        assert_eq!(
            String::from_utf8(dir_dump(&fs, &env, &dir, None, true)).unwrap(),
            " Database : db ------ [loc:root] \n Path     : /new\n CHAPTERS:\n 0 -> MODELS (.(null)) (null) vers=4\n \
             Database : prj ------ [sys:unknown] \n Path     : /dbs/project\n CHAPTERS:\n"
        );
        assert_eq!(dir_dump(&fs, &env, &dir, Some(1), false), b" Database : prj ------ [sys:unknown] \n Path     : /dbs/project\n");
    }
}
