// SPDX-License-Identifier: MIT
//! The files of the machine, as the renderer's routines on the image use them. Nothing here
//! reproduces a function of the original: this is the C library below `rle.c`, `rnd_tag.c`,
//! `rnd_main.c` and the texture reader, behind one trait.
//!
//! [`FileHost`] has the library's calls those files make (`access`, `fopen`, `fwrite`,
//! `fread`, `rewind`, `fflush`, `ferror`, `fclose`, `creat`, `close`, `unlink`, `rename`). A
//! `FILE *` is an address of [`si_mem::HANDLES`], numbered as the emulation harness numbers
//! its files (the three standard streams first), so that `pFile` holds the same word in a
//! check as in the emulated original.
//!
//! Two implementations: [`MemFiles`], a file system in memory with the harness's rules
//! (one content per name; a file exists when it has a content), for the tests and the
//! emulation check; and [`DiskFiles`], the real one, which takes every path the program
//! names under a root directory (or as it is, when there is no root).
//!
//! [`TexturePictures`] is the texture reader's side: [`crate::frame::PictureFiles`] answered
//! from a [`FileHost`] with the two directories the original builds the names from.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use si_mem::{Handles, Kind};

/// The C library's file calls, as the renderer makes them. Paths and modes are the bytes
/// of the C strings.
pub trait FileHost {
    /// `access(path, mode)`: 0, or −1 (mode 0: the file exists; 2: it may be written).
    fn access(&mut self, path: &[u8], mode: i32) -> i32;
    /// `fopen(path, mode)`: the `FILE *`, 0 for null.
    fn fopen(&mut self, path: &[u8], mode: &[u8]) -> u32;
    /// `fwrite(bytes, 1, n, file)` (and `fputc` for one byte): the number of bytes written.
    fn fwrite(&mut self, file: u32, bytes: &[u8]) -> u32;
    /// `fread(buffer, 1, n, file)`: the bytes read, fewer than `n` at the file's end.
    fn fread(&mut self, file: u32, n: u32) -> Vec<u8>;
    /// `rewind(file)`.
    fn rewind(&mut self, file: u32);
    /// `fflush(file)`: 0, or −1.
    fn fflush(&mut self, file: u32) -> i32;
    /// `ferror(file)`: not 0 once a write or read failed.
    fn ferror(&mut self, file: u32) -> i32;
    /// `fclose(file)`.
    fn fclose(&mut self, file: u32) -> i32;
    /// `creat(path, mode)`: a descriptor, or −1.
    fn creat(&mut self, path: &[u8], mode: u32) -> i32;
    /// `close(descriptor)`.
    fn close(&mut self, fd: i32) -> i32;
    /// `unlink(path)`: 0, or −1.
    fn unlink(&mut self, path: &[u8]) -> i32;
    /// `rename(from, to)`: 0, or −1.
    fn rename(&mut self, from: &[u8], to: &[u8]) -> i32;
    /// A whole file, for a reader that takes its file as bytes (not a call of the library).
    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>>;
    /// A whole file replaced (not a call of the library); false when it cannot be written.
    fn write_file(&mut self, path: &[u8], data: &[u8]) -> bool;
}

struct MemOpen {
    path: Vec<u8>,
    pos: usize,
    readable: bool,
    writable: bool,
    err: bool,
}

/// A file system in memory, with the rules of the emulation harness's stand-ins
/// (`re/tools/emu/standins.py`): a name has one content, which every handle on it sees; a
/// path exists when it has a content (a directory is a name that ends in `/` with an empty
/// content); `fflush` always answers 0.
pub struct MemFiles {
    /// The contents by path.
    pub files: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Paths that may not be written (`access(path, 2)` answers −1 and `fopen` for writing
    /// null).
    pub read_only: Vec<Vec<u8>>,
    /// Every path that was written, created, renamed or removed, once, in order.
    pub touched: Vec<Vec<u8>>,
    /// The library's calls on paths, as text, in order (`access /p/ 0`, `fopen /p/a.pic wb`,
    /// `creat …`, `close …`, `unlink …`, `rename …`).
    pub log: Vec<String>,
    /// With a number: a write that would take a file past so many bytes fails and sets the
    /// handle's error mark (a full disk).
    pub room: Option<usize>,
    open: Handles<MemOpen>,
    next_fd: i32,
}

impl Default for MemFiles {
    fn default() -> MemFiles {
        MemFiles::new()
    }
}

impl MemFiles {
    pub fn new() -> MemFiles {
        let mut open = Handles::new(Kind::File);
        // stdin, stdout, stderr: the harness's first three
        for name in ["stdin", "stdout", "stderr"] {
            open.open(MemOpen { path: name.as_bytes().to_vec(), pos: 0, readable: name == "stdin", writable: name != "stdin", err: false });
        }
        MemFiles { files: BTreeMap::new(), read_only: Vec::new(), touched: Vec::new(), log: Vec::new(), room: None, open, next_fd: 3 }
    }

    /// The path an open `FILE *` is on.
    pub fn path_of(&self, file: u32) -> Option<&[u8]> {
        self.open.get(file).map(|f| f.path.as_slice())
    }

    /// The handle's position and whether its error mark is set.
    pub fn state_of(&self, file: u32) -> Option<(usize, bool)> {
        self.open.get(file).map(|f| (f.pos, f.err))
    }

    /// A `FILE *` on a content the caller supplies, as the harness opens a `FILE(w)`
    /// argument: the name is the handle's own.
    pub fn open_named(&mut self, name: &[u8], data: &[u8], mode: &[u8]) -> u32 {
        self.files.insert(name.to_vec(), data.to_vec());
        let plus = mode.contains(&b'+');
        self.open.open(MemOpen { path: name.to_vec(), pos: 0, readable: mode.contains(&b'r') || plus, writable: mode.contains(&b'w') || mode.contains(&b'a') || plus, err: false })
    }

    fn touch(&mut self, path: &[u8]) {
        if !self.touched.iter().any(|p| p == path) {
            self.touched.push(path.to_vec());
        }
    }

    fn text(path: &[u8]) -> String {
        String::from_utf8_lossy(path).into_owned()
    }
}

impl FileHost for MemFiles {
    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        self.log.push(format!("access {} {mode}", Self::text(path)));
        if !self.files.contains_key(path) || (mode & 2 != 0 && self.read_only.iter().any(|p| p == path)) {
            -1
        } else {
            0
        }
    }
    fn fopen(&mut self, path: &[u8], mode: &[u8]) -> u32 {
        self.log.push(format!("fopen {} {}", Self::text(path), Self::text(mode)));
        let read = mode.contains(&b'r');
        let plus = mode.contains(&b'+');
        if read {
            if !self.files.contains_key(path) {
                return 0;
            }
        } else {
            if self.read_only.iter().any(|p| p == path) {
                return 0;
            }
            let data = self.files.entry(path.to_vec()).or_default();
            if mode.contains(&b'w') {
                data.clear();
            }
        }
        if !read || plus {
            self.touch(path);
        }
        let pos = if mode.contains(&b'a') { self.files[path].len() } else { 0 };
        self.open.open(MemOpen { path: path.to_vec(), pos, readable: read || plus, writable: !read || plus, err: false })
    }
    fn fwrite(&mut self, file: u32, bytes: &[u8]) -> u32 {
        let room = self.room;
        let Some(f) = self.open.get_mut(file) else { return 0 };
        if !f.writable {
            f.err = true;
            return 0;
        }
        let data = self.files.entry(f.path.clone()).or_default();
        let end = f.pos + bytes.len();
        if room.is_some_and(|r| end > r) {
            f.err = true;
            return 0;
        }
        if data.len() < end {
            data.resize(end, 0);
        }
        data[f.pos..end].copy_from_slice(bytes);
        f.pos = end;
        bytes.len() as u32
    }
    fn fread(&mut self, file: u32, n: u32) -> Vec<u8> {
        let Some(f) = self.open.get_mut(file) else { return Vec::new() };
        if !f.readable {
            f.err = true;
            return Vec::new();
        }
        let data = self.files.get(&f.path).map(Vec::as_slice).unwrap_or(&[]);
        let end = (f.pos + n as usize).min(data.len());
        let out = data[f.pos.min(end)..end].to_vec();
        f.pos = end.max(f.pos);
        out
    }
    fn rewind(&mut self, file: u32) {
        if let Some(f) = self.open.get_mut(file) {
            f.pos = 0;
            f.err = false;
        }
    }
    fn fflush(&mut self, _: u32) -> i32 {
        0
    }
    fn ferror(&mut self, file: u32) -> i32 {
        self.open.get(file).is_some_and(|f| f.err) as i32
    }
    fn fclose(&mut self, file: u32) -> i32 {
        self.log.push(format!("fclose {}", self.open.get(file).map_or(String::from("?"), |f| Self::text(&f.path))));
        self.open.close(file);
        0
    }
    fn creat(&mut self, path: &[u8], mode: u32) -> i32 {
        self.log.push(format!("creat {} {mode}", Self::text(path)));
        if self.read_only.iter().any(|p| p == path) {
            return -1;
        }
        self.files.insert(path.to_vec(), Vec::new());
        self.touch(path);
        self.next_fd += 1;
        self.next_fd
    }
    fn close(&mut self, fd: i32) -> i32 {
        self.log.push(format!("close {fd}"));
        0
    }
    fn unlink(&mut self, path: &[u8]) -> i32 {
        self.log.push(format!("unlink {}", Self::text(path)));
        if self.files.remove(path).is_none() {
            return -1;
        }
        self.touch(path);
        0
    }
    fn rename(&mut self, from: &[u8], to: &[u8]) -> i32 {
        self.log.push(format!("rename {} {}", Self::text(from), Self::text(to)));
        let Some(data) = self.files.remove(from) else { return -1 };
        self.files.insert(to.to_vec(), data);
        self.touch(from);
        self.touch(to);
        0
    }
    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }
    fn write_file(&mut self, path: &[u8], data: &[u8]) -> bool {
        if self.read_only.iter().any(|p| p == path) {
            return false;
        }
        self.files.insert(path.to_vec(), data.to_vec());
        self.touch(path);
        true
    }
}

struct DiskOpen {
    file: Option<std::fs::File>,
    err: bool,
}

/// The machine's own files. With a root, every path the program names is taken below it
/// (`/db/PICTURES/a.1.pic` is `<root>/db/PICTURES/a.1.pic`), so that a render writes
/// nowhere else; a path with a `..` component is refused. Without one, paths are used as
/// they are.
pub struct DiskFiles {
    root: Option<PathBuf>,
    open: Handles<DiskOpen>,
    descriptors: Vec<(i32, std::fs::File)>,
    next_fd: i32,
    /// Every file opened for writing or created, as the path on the machine, in order.
    pub written: Vec<PathBuf>,
}

impl DiskFiles {
    /// Paths as the program names them.
    pub fn direct() -> DiskFiles {
        DiskFiles::with(None)
    }

    /// Every path below `root`.
    pub fn under(root: impl Into<PathBuf>) -> DiskFiles {
        DiskFiles::with(Some(root.into()))
    }

    fn with(root: Option<PathBuf>) -> DiskFiles {
        let mut open = Handles::new(Kind::File);
        // the three standard streams keep their numbers; they are not files of this host
        // (a closed handle's address is never given out again)
        for _ in 0..3 {
            let h = open.open(DiskOpen { file: None, err: true });
            open.close(h);
        }
        DiskFiles { root, open, descriptors: Vec::new(), next_fd: 3, written: Vec::new() }
    }

    /// The path on the machine for a path of the program; `None` when it would leave the
    /// root.
    pub fn resolve(&self, path: &[u8]) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
        match &self.root {
            None => Some(p.to_path_buf()),
            Some(root) => {
                let mut out = root.clone();
                for part in p.components() {
                    match part {
                        std::path::Component::Normal(x) => out.push(x),
                        std::path::Component::RootDir | std::path::Component::CurDir => {}
                        _ => return None,
                    }
                }
                Some(out)
            }
        }
    }
}

impl FileHost for DiskFiles {
    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        let Some(p) = self.resolve(path) else { return -1 };
        match std::fs::metadata(&p) {
            Err(_) => -1,
            Ok(meta) => {
                if mode & 2 != 0 && meta.permissions().readonly() {
                    -1
                } else {
                    0
                }
            }
        }
    }
    fn fopen(&mut self, path: &[u8], mode: &[u8]) -> u32 {
        let Some(p) = self.resolve(path) else { return 0 };
        let plus = mode.contains(&b'+');
        let mut o = std::fs::OpenOptions::new();
        if mode.contains(&b'r') {
            o.read(true).write(plus);
        } else if mode.contains(&b'a') {
            o.append(true).create(true).read(plus);
        } else {
            o.write(true).create(true).truncate(true).read(plus);
        }
        match o.open(&p) {
            Ok(file) => {
                if !mode.contains(&b'r') || plus {
                    self.written.push(p);
                }
                self.open.open(DiskOpen { file: Some(file), err: false })
            }
            Err(_) => 0,
        }
    }
    fn fwrite(&mut self, file: u32, bytes: &[u8]) -> u32 {
        let Some(f) = self.open.get_mut(file) else { return 0 };
        let Some(inner) = f.file.as_mut() else { return 0 };
        match inner.write_all(bytes) {
            Ok(()) => bytes.len() as u32,
            Err(_) => {
                f.err = true;
                0
            }
        }
    }
    fn fread(&mut self, file: u32, n: u32) -> Vec<u8> {
        let Some(f) = self.open.get_mut(file) else { return Vec::new() };
        let mut out = Vec::new();
        let Some(inner) = f.file.as_mut() else { return out };
        if inner.take(n as u64).read_to_end(&mut out).is_err() {
            f.err = true;
        }
        out
    }
    fn rewind(&mut self, file: u32) {
        if let Some(f) = self.open.get_mut(file) {
            if let Some(inner) = f.file.as_mut() {
                let _ = inner.seek(SeekFrom::Start(0));
            }
            f.err = false;
        }
    }
    fn fflush(&mut self, file: u32) -> i32 {
        let Some(f) = self.open.get_mut(file) else { return -1 };
        let Some(inner) = f.file.as_mut() else { return -1 };
        match inner.flush() {
            Ok(()) => 0,
            Err(_) => {
                f.err = true;
                -1
            }
        }
    }
    fn ferror(&mut self, file: u32) -> i32 {
        self.open.get(file).is_some_and(|f| f.err) as i32
    }
    fn fclose(&mut self, file: u32) -> i32 {
        match self.open.close(file) {
            Some(_) => 0,
            None => -1,
        }
    }
    fn creat(&mut self, path: &[u8], _mode: u32) -> i32 {
        // (the original passes mode 0: a file nobody may open, which is the lock's point;
        // here the file is made with the host's default mode so that it can be removed)
        let Some(p) = self.resolve(path) else { return -1 };
        match std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&p) {
            Ok(file) => {
                self.next_fd += 1;
                self.descriptors.push((self.next_fd, file));
                self.written.push(p);
                self.next_fd
            }
            Err(_) => -1,
        }
    }
    fn close(&mut self, fd: i32) -> i32 {
        let before = self.descriptors.len();
        self.descriptors.retain(|d| d.0 != fd);
        if self.descriptors.len() < before {
            0
        } else {
            -1
        }
    }
    fn unlink(&mut self, path: &[u8]) -> i32 {
        match self.resolve(path).map(std::fs::remove_file) {
            Some(Ok(())) => 0,
            _ => -1,
        }
    }
    fn rename(&mut self, from: &[u8], to: &[u8]) -> i32 {
        match (self.resolve(from), self.resolve(to)) {
            (Some(a), Some(b)) if std::fs::rename(&a, &b).is_ok() => {
                self.written.push(b);
                0
            }
            _ => -1,
        }
    }
    fn read_file(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        std::fs::read(self.resolve(path)?).ok()
    }
    fn write_file(&mut self, path: &[u8], data: &[u8]) -> bool {
        match self.resolve(path) {
            Some(p) if std::fs::write(&p, data).is_ok() => {
                self.written.push(p);
                true
            }
            _ => false,
        }
    }
}

/// Texture pictures from a [`FileHost`], by the names `SI_readTextureFile` builds
/// (finding 0012, section 13): `<name>.pic` for a name that starts with `/`, else
/// `<SI_texturePath>/<name>.pic`; the default picture is `<SI_resdir>/noIcon.pic`.
pub struct TexturePictures<'a> {
    pub files: &'a mut dyn FileHost,
    /// What `SI_texturePath` (pointer at 0x08679434) points to: the PICTURES directory of
    /// the current database.
    pub texture_path: Vec<u8>,
    /// What `SI_resdir` (pointer at 0x08781628) points to: the program's resource
    /// directory.
    pub resdir: Vec<u8>,
    /// What was asked for: the path, and whether it was found.
    pub asked: Vec<(Vec<u8>, bool)>,
}

impl<'a> TexturePictures<'a> {
    pub fn new(files: &'a mut dyn FileHost, texture_path: &[u8], resdir: &[u8]) -> TexturePictures<'a> {
        TexturePictures { files, texture_path: texture_path.to_vec(), resdir: resdir.to_vec(), asked: Vec::new() }
    }

    /// With the two directories as the image has them: the texts `SI_texturePath`
    /// (0x08679434) and `SI_resdir` (0x08781628) point to; a null pointer is an empty text.
    pub fn from_image(files: &'a mut dyn FileHost, m: &si_mem::Mem) -> TexturePictures<'a> {
        let text = |at: u32| match m.u32(at) {
            0 => Vec::new(),
            p => m.cstr(p),
        };
        let (texture_path, resdir) = (text(crate::img::g::SI_TEXTURE_PATH), text(crate::img::g::SI_RESDIR));
        TexturePictures { files, texture_path, resdir, asked: Vec::new() }
    }

    /// The path of a texture's picture, or of the default picture.
    pub fn path(&self, name: &[u8], default: bool) -> Vec<u8> {
        let mut p = Vec::new();
        if default {
            p.extend_from_slice(&self.resdir);
            p.extend_from_slice(b"/noIcon.pic");
            return p;
        }
        if name.first() != Some(&b'/') {
            p.extend_from_slice(&self.texture_path);
            p.push(b'/');
        }
        p.extend_from_slice(name);
        p.extend_from_slice(b".pic");
        p
    }
}

impl crate::frame::PictureFiles for TexturePictures<'_> {
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        let path = self.path(name, default);
        let data = self.files.read_file(&path);
        self.asked.push((path, data.is_some()));
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::PictureFiles;

    #[test]
    fn memory_files_follow_the_harness() {
        let mut h = MemFiles::new();
        assert_eq!(h.access(b"/p/a", 0), -1);
        h.files.insert(b"/p/".to_vec(), Vec::new());
        assert_eq!((h.access(b"/p/", 0), h.access(b"/p/", 2)), (0, 0));
        let f = h.fopen(b"/p/a", b"wb");
        // the fourth handle: the three standard streams come first
        assert_eq!(f, 0x6f00_0300);
        assert_eq!(h.fwrite(f, b"hello"), 5);
        assert_eq!((h.fflush(f), h.ferror(f), h.fclose(f)), (0, 0, 0));
        let g = h.fopen(b"/p/a", b"rb+");
        assert_eq!(g, 0x6f00_0400);
        assert_eq!(h.fread(g, 3), b"hel");
        h.rewind(g);
        assert_eq!(h.fwrite(g, b"J"), 1);
        h.fclose(g);
        assert_eq!(h.files[&b"/p/a".to_vec()], b"Jello");
        assert_eq!(h.fopen(b"/p/none", b"rb"), 0);
        assert!(h.creat(b"/p/a.sema", 0) > 0);
        assert_eq!((h.unlink(b"/p/a.sema"), h.unlink(b"/p/a.sema")), (0, -1));
        assert_eq!(h.rename(b"/p/a", b"/p/b"), 0);
        assert_eq!(h.touched, vec![b"/p/a".to_vec(), b"/p/a.sema".to_vec(), b"/p/b".to_vec()]);
        // a full disk: the write fails and the mark stays until the file is rewound
        h.room = Some(3);
        let f = h.fopen(b"/p/c", b"wb");
        assert_eq!((h.fwrite(f, b"abcd"), h.ferror(f)), (0, 1));
    }

    #[test]
    fn disk_files_stay_below_their_root() {
        let d = DiskFiles::under("/some/root");
        assert_eq!(d.resolve(b"/db/PICTURES/a.1.pic").unwrap(), PathBuf::from("/some/root/db/PICTURES/a.1.pic"));
        assert_eq!(d.resolve(b"rel/x").unwrap(), PathBuf::from("/some/root/rel/x"));
        assert!(d.resolve(b"/db/../../etc/passwd").is_none());
        assert_eq!(DiskFiles::direct().resolve(b"/a/b").unwrap(), PathBuf::from("/a/b"));
    }

    #[test]
    fn texture_pictures_by_the_original_s_names() {
        let mut h = MemFiles::new();
        h.files.insert(b"/db/PICTURES/wood.pic".to_vec(), vec![1]);
        h.files.insert(b"/abs/marble.pic".to_vec(), vec![2]);
        h.files.insert(b"/res/noIcon.pic".to_vec(), vec![3]);
        let mut t = TexturePictures::new(&mut h, b"/db/PICTURES", b"/res");
        assert_eq!(t.picture_file(b"wood", false), Some(vec![1]));
        assert_eq!(t.picture_file(b"/abs/marble", false), Some(vec![2]));
        assert_eq!(t.picture_file(b"gone", false), None);
        assert_eq!(t.picture_file(b"gone", true), Some(vec![3]));
        assert_eq!(t.asked[2], (b"/db/PICTURES/gone.pic".to_vec(), false));
        // the two directories from the image's globals
        let mut m = si_mem::Mem::new();
        let (a, b) = (m.place_cstr(b"/db/PICTURES"), m.place_cstr(b"/res"));
        m.set_u32(crate::img::g::SI_TEXTURE_PATH, a);
        m.set_u32(crate::img::g::SI_RESDIR, b);
        let mut t = TexturePictures::from_image(&mut h, &m);
        assert_eq!((t.picture_file(b"wood", false), t.picture_file(b"x", true)), (Some(vec![1]), Some(vec![3])));
    }
}
