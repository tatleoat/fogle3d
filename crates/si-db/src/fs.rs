// SPDX-License-Identifier: MIT
//! What the database layer asks of the operating system, as two traits, with a directory
//! tree in memory for tests and the real file system for use.
//!
//! The original calls `opendir`/`readdir`, `stat`, `access`, `fopen`, `mkdir` and `unlink`
//! directly. Nothing here is a port of Softimage code; it is the seam that lets the ported
//! routines run on directory listings passed in as data.

use crate::Bytes;
use std::collections::HashMap;

/// The fields of `struct stat` the database layer reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stat {
    pub mode: u32,
    pub uid: u32,
    pub size: i32,
    pub mtime: i32,
}

pub const S_IFMT: u32 = 0xf000;
pub const S_IFDIR: u32 = 0x4000;
pub const S_IFREG: u32 = 0x8000;

impl Stat {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
}

/// `access` modes.
pub const F_OK: i32 = 0;
pub const X_OK: i32 = 1;
pub const W_OK: i32 = 2;
pub const R_OK: i32 = 4;

pub trait FileSystem {
    /// `opendir` and `readdir` to the end: the entry names in the directory's own order.
    /// `None` when the directory cannot be opened. The entries `.` and `..` may be left out:
    /// no routine of the database layer ever selects a directory whose name starts with a dot.
    fn read_dir(&self, path: &[u8]) -> Option<Vec<Bytes>>;
    /// `stat` (links followed). `None` when it fails.
    fn stat(&self, path: &[u8]) -> Option<Stat>;
    /// `access(path, mode) == 0`.
    fn access(&self, path: &[u8], mode: i32) -> bool;
    /// `fopen(path, "r")` and the whole content. `None` when the open fails.
    fn read(&self, path: &[u8]) -> Option<Bytes>;
}

pub trait FileSystemMut: FileSystem {
    /// `mkdir(path, 0777 & ~umask) != -1`.
    fn mkdir(&mut self, path: &[u8]) -> bool;
    /// `fopen(path, append ? "a" : "w")`, the data, `fclose`. False when the open fails.
    fn write(&mut self, path: &[u8], data: &[u8], append: bool) -> bool;
    /// `unlink(path) == 0`.
    fn unlink(&mut self, path: &[u8]) -> bool;
    /// `chmod(path, mode) == 0`.
    fn chmod(&mut self, path: &[u8], mode: u32) -> bool;
    /// `rename(from, to) == 0`.
    fn rename(&mut self, from: &[u8], to: &[u8]) -> bool;
    /// `system(command)`: the command line is handed to the shell; the result is what
    /// `system` returns (0 for a command that succeeded).
    fn system(&mut self, command: &[u8]) -> i32;
}

#[derive(Clone, Debug)]
enum Node {
    Dir,
    File(Bytes),
}

#[derive(Clone, Debug)]
struct Entry {
    path: Bytes,
    node: Node,
    mode: u32,
    mtime: i32,
}

/// A directory tree in memory. Directory listings come back in the order the entries were
/// added, which lets a test pick the "directory order" the original would have met.
#[derive(Clone, Debug, Default)]
pub struct MemFs {
    entries: Vec<Entry>,
    index: HashMap<Bytes, usize>,
    /// Every command line given to [`FileSystemMut::system`]; none is carried out.
    pub commands: Vec<Bytes>,
    /// What `system` reports for them.
    pub system_status: i32,
}

/// `a//b/` and `a/b` name the same thing; `.` and `..` are not interpreted.
fn normal(path: &[u8]) -> Bytes {
    let mut out = Vec::with_capacity(path.len());
    if path.first() == Some(&b'/') {
        out.push(b'/');
    }
    for part in path.split(|&c| c == b'/').filter(|p| !p.is_empty()) {
        if !out.is_empty() && out.last() != Some(&b'/') {
            out.push(b'/');
        }
        out.extend_from_slice(part);
    }
    out
}

fn parent(path: &[u8]) -> Option<Bytes> {
    match path.iter().rposition(|&c| c == b'/') {
        Some(0) if path.len() > 1 => Some(b"/".to_vec()),
        Some(0) => None,
        Some(i) => Some(path[..i].to_vec()),
        None if path.is_empty() => None,
        None => Some(Vec::new()),
    }
}

impl MemFs {
    pub fn new() -> MemFs {
        MemFs::default()
    }

    fn put(&mut self, path: Bytes, node: Node, mode: u32, mtime: i32) {
        if let Some(up) = parent(&path) {
            if !up.is_empty() && !self.index.contains_key(&up) {
                self.put(up, Node::Dir, S_IFDIR | 0o755, 0);
            }
        }
        match self.index.get(&path) {
            Some(&i) => self.entries[i] = Entry { path, node, mode, mtime },
            None => {
                self.index.insert(path.clone(), self.entries.len());
                self.entries.push(Entry { path, node, mode, mtime });
            }
        }
    }

    /// Adds a directory and any missing parents.
    pub fn add_dir(&mut self, path: &[u8]) -> &mut Self {
        let p = normal(path);
        if !self.index.contains_key(&p) {
            self.put(p, Node::Dir, S_IFDIR | 0o755, 0);
        }
        self
    }

    /// Adds a file (and any missing parents), replacing one of the same name.
    pub fn add_file(&mut self, path: &[u8], data: &[u8]) -> &mut Self {
        self.put(normal(path), Node::File(data.to_vec()), S_IFREG | 0o644, 0);
        self
    }

    /// Adds a file with a modification time and permission bits.
    pub fn add_file_with(&mut self, path: &[u8], data: &[u8], mtime: i32, perm: u32) -> &mut Self {
        self.put(normal(path), Node::File(data.to_vec()), S_IFREG | (perm & 0o7777), mtime);
        self
    }

    /// Every path in the tree, in the order added.
    pub fn paths(&self) -> impl Iterator<Item = (&[u8], bool)> {
        self.entries.iter().map(|e| (e.path.as_slice(), matches!(e.node, Node::Dir)))
    }

    /// Whether the directory that should hold `path` is missing (the roots always exist).
    fn orphan(&self, path: &[u8]) -> bool {
        parent(path).is_some_and(|up| !up.is_empty() && up != b"/" && !self.index.contains_key(&up))
    }

    fn reindex(&mut self) {
        self.index = self.entries.iter().enumerate().map(|(k, e)| (e.path.clone(), k)).collect();
    }

    fn get(&self, path: &[u8]) -> Option<&Entry> {
        self.index.get(&normal(path)).map(|&i| &self.entries[i])
    }
}

impl FileSystem for MemFs {
    fn read_dir(&self, path: &[u8]) -> Option<Vec<Bytes>> {
        let dir = normal(path);
        if !dir.is_empty() && !matches!(self.get(&dir)?.node, Node::Dir) {
            return None;
        }
        Some(
            self.entries
                .iter()
                .filter(|e| parent(&e.path).as_deref() == Some(&dir[..]))
                .map(|e| e.path[e.path.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1)..].to_vec())
                .collect(),
        )
    }

    fn stat(&self, path: &[u8]) -> Option<Stat> {
        let e = self.get(path)?;
        let size = match &e.node {
            Node::Dir => 0,
            Node::File(d) => d.len() as i32,
        };
        Some(Stat { mode: e.mode, uid: 0, size, mtime: e.mtime })
    }

    fn access(&self, path: &[u8], mode: i32) -> bool {
        // the owner's permission bits decide
        self.get(path).is_some_and(|e| (e.mode >> 6) & 7 & mode as u32 == mode as u32)
    }

    fn read(&self, path: &[u8]) -> Option<Bytes> {
        match &self.get(path)?.node {
            Node::File(d) => Some(d.clone()),
            Node::Dir => None,
        }
    }
}

impl FileSystemMut for MemFs {
    fn mkdir(&mut self, path: &[u8]) -> bool {
        let p = normal(path);
        // like mkdir(2): the parent must exist and the name must be free
        if self.index.contains_key(&p) || self.orphan(&p) {
            return false;
        }
        self.put(p, Node::Dir, S_IFDIR | 0o755, 0);
        true
    }

    fn write(&mut self, path: &[u8], data: &[u8], append: bool) -> bool {
        let p = normal(path);
        if self.orphan(&p) {
            return false;
        }
        let mut content = match self.get(&p).map(|e| &e.node) {
            Some(Node::Dir) => return false,
            Some(Node::File(old)) if append => old.clone(),
            _ => Vec::new(),
        };
        content.extend_from_slice(data);
        self.put(p, Node::File(content), S_IFREG | 0o644, 0);
        true
    }

    fn unlink(&mut self, path: &[u8]) -> bool {
        let p = normal(path);
        match self.index.get(&p) {
            Some(&i) if matches!(self.entries[i].node, Node::File(_)) => {
                self.entries.remove(i);
                self.reindex();
                true
            }
            _ => false,
        }
    }

    fn chmod(&mut self, path: &[u8], mode: u32) -> bool {
        let p = normal(path);
        match self.index.get(&p) {
            Some(&i) => {
                let e = &mut self.entries[i];
                e.mode = (e.mode & S_IFMT) | (mode & 0o7777);
                true
            }
            None => false,
        }
    }

    fn rename(&mut self, from: &[u8], to: &[u8]) -> bool {
        let (from, to) = (normal(from), normal(to));
        if !self.index.contains_key(&from) || self.index.contains_key(&to) || self.orphan(&to) {
            return false;
        }
        let inside = [&from[..], b"/"].concat();
        for e in &mut self.entries {
            if e.path == from {
                e.path = to.clone();
            } else if e.path.starts_with(&inside) {
                e.path = [&to[..], &e.path[from.len()..]].concat();
            }
        }
        self.reindex();
        true
    }

    fn system(&mut self, command: &[u8]) -> i32 {
        self.commands.push(command.to_vec());
        self.system_status
    }
}

/// The real file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealFs;

#[cfg(unix)]
mod real {
    use super::*;
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::MetadataExt;

    extern "C" {
        // the C library's access(2), so permission answers are the system's own
        fn access(path: *const std::os::raw::c_char, mode: std::os::raw::c_int) -> std::os::raw::c_int;
    }

    fn os(path: &[u8]) -> &OsStr {
        OsStr::from_bytes(path)
    }

    impl FileSystem for RealFs {
        fn read_dir(&self, path: &[u8]) -> Option<Vec<Bytes>> {
            let rd = std::fs::read_dir(os(path)).ok()?;
            Some(rd.flatten().map(|e| e.file_name().into_vec()).collect())
        }

        fn stat(&self, path: &[u8]) -> Option<Stat> {
            let m = std::fs::metadata(os(path)).ok()?;
            Some(Stat { mode: m.mode(), uid: m.uid(), size: m.size() as i32, mtime: m.mtime() as i32 })
        }

        fn access(&self, path: &[u8], mode: i32) -> bool {
            let Ok(c) = CString::new(path) else { return false };
            // SAFETY: `c` is a valid, zero-terminated string that outlives the call.
            unsafe { access(c.as_ptr(), mode) == 0 }
        }

        fn read(&self, path: &[u8]) -> Option<Bytes> {
            std::fs::read(os(path)).ok()
        }
    }

    impl FileSystemMut for RealFs {
        fn mkdir(&mut self, path: &[u8]) -> bool {
            // create_dir asks for mode 0777 and the kernel applies the umask, as dbMakeDir does
            std::fs::create_dir(os(path)).is_ok()
        }

        fn write(&mut self, path: &[u8], data: &[u8], append: bool) -> bool {
            use std::io::Write;
            let mut o = std::fs::OpenOptions::new();
            o.create(true);
            if append {
                o.append(true);
            } else {
                o.write(true).truncate(true);
            }
            // the original does not check its fprintf calls: only the open decides
            match o.open(os(path)) {
                Ok(mut f) => {
                    let _ = f.write_all(data);
                    true
                }
                Err(_) => false,
            }
        }

        fn unlink(&mut self, path: &[u8]) -> bool {
            std::fs::remove_file(os(path)).is_ok()
        }

        fn chmod(&mut self, path: &[u8], mode: u32) -> bool {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(os(path), std::fs::Permissions::from_mode(mode & 0o7777)).is_ok()
        }

        fn rename(&mut self, from: &[u8], to: &[u8]) -> bool {
            std::fs::rename(os(from), os(to)).is_ok()
        }

        fn system(&mut self, _command: &[u8]) -> i32 {
            // Deliberate difference (lead's decision, 2026-10-13). The original hands these
            // lines (`mv`, `cp`, `rcp`, `rm` with file names pasted in unquoted) to system(3).
            // A file name holding a space, a `;` or a backquote would then run as shell text,
            // so on a real file system the command is refused, as if the shell could not be
            // started. The command lines themselves are still built exactly (see `MemFs`,
            // which records them). To be replaced by the same operations done directly.
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_fs_lists_in_insertion_order() {
        let mut fs = MemFs::new();
        fs.add_file(b"/db/MODELS/b.1-0.hrc", b"x").add_file(b"/db/MODELS/a.1-0.hrc", b"yy").add_dir(b"/db/SCENES/");
        assert_eq!(fs.read_dir(b"/db/MODELS/").unwrap(), [b"b.1-0.hrc".to_vec(), b"a.1-0.hrc".to_vec()]);
        assert_eq!(fs.read_dir(b"/db").unwrap(), [b"MODELS".to_vec(), b"SCENES".to_vec()]);
        assert_eq!(fs.read_dir(b"/nope"), None);
        assert_eq!(fs.read_dir(b"/db/MODELS/a.1-0.hrc"), None);
        assert!(fs.stat(b"/db//MODELS").unwrap().is_dir());
        assert_eq!(fs.stat(b"/db/MODELS/a.1-0.hrc").unwrap().size, 2);
        assert!(fs.access(b"/db/MODELS/a.1-0.hrc", R_OK));
        assert!(!fs.access(b"/db/MODELS/a.1-0.hrc", X_OK));
        assert!(fs.access(b"/db/MODELS", X_OK));
        assert!(!fs.access(b"/db/x", F_OK));
    }

    #[test]
    fn mem_fs_changes() {
        let mut fs = MemFs::new();
        assert!(fs.mkdir(b"/db"));
        assert!(!fs.mkdir(b"/db"));
        assert!(!fs.mkdir(b"/a/b"));
        assert!(fs.write(b"/db/f", b"one ", false));
        assert!(fs.write(b"/db/f", b"two", true));
        assert_eq!(fs.read(b"/db/f").unwrap(), b"one two");
        assert!(!fs.write(b"/nodir/f", b"", false));
        assert!(fs.unlink(b"/db/f"));
        assert!(!fs.unlink(b"/db/f"));
        assert!(!fs.unlink(b"/db"));
        fs.add_file(b"/db/a/f", b"x");
        assert!(fs.rename(b"/db/a", b"/db/b"));
        assert_eq!(fs.read(b"/db/b/f").unwrap(), b"x");
        assert!(!fs.rename(b"/db/a", b"/db/c"));
        assert!(fs.chmod(b"/db/b/f", 0o444));
        assert!(!fs.access(b"/db/b/f", W_OK));
        assert_eq!(fs.system(b"rm -rf /db"), 0);
        assert_eq!(fs.commands, [b"rm -rf /db".to_vec()]);
        assert!(fs.access(b"/db/b/f", F_OK)); // recorded, not carried out
    }
}
