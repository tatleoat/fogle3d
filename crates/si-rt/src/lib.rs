// SPDX-License-Identifier: MIT
//! Run-time data: the original program's own tables and messages, read from the user's own
//! Softimage|3D 4.0 install while our program runs.
//!
//! Release model (owner, 2026-10-29): the user supplies their own install, as a recompiled game
//! asks for its ROM; nothing of the original ships, not even the tables and message texts some
//! ported routines need byte for byte. Such a crate does not hold those bytes in its source: it
//! names them by where the original program has them (a virtual address of the Linux `soft`
//! program and a length), and this crate reads them out of the user's copy of that program the
//! first time they are wanted. Design, rules and the conversion guide: `docs/design/runtime-data.md`.
//!
//! ```no_run
//! // a table: one static per range, checked against our own checksum the first time it is read
//! static YACC: si_rt::Table = si_rt::Table::new("si_custom.shdb_yacc", 0x0866_3940, 0xc00, 0xaf0f_6d05_f9bd_2b64);
//! let bytes: &'static [u8] = YACC.get();
//! // a message: the C string a `char *` of the original points to (here the text pointer of the
//! // first entry of the error-format table, three words per entry from 0x0868_6a60)
//! let text: &'static [u8] = si_rt::message_at(0x0868_6a68);
//! ```
//!
//! * **Where the program is.** The first use looks, in order, in the directories a program gave
//!   with [`add_root`] (its own install argument), in the directory the environment variable
//!   `SI_INSTALL` names, then (development only) in `re/bin/si3d40-ia/installed` above the
//!   working directory or the program, for the file `soft` of the Linux build
//!   (`Creative-Tools/3D/bin/soft` under the install root, else found by name and length). The
//!   licence file and anything else of the install is never opened: only a file of that name and
//!   of the expected length is read.
//! * **Which build.** The file must have the length and the checksum (FNV-1a, 64 bits, our own
//!   numbers: [`LINUX_SOFT`]) of the build the port was written against; every [`Table`] also
//!   checks its own range against its own checksum.
//! * **Cost.** The file (8.1 MB) is read once per process and kept; a table or message is a slice
//!   of it (`&'static [u8]`, no copy), so a crate looks it up once (a `static` [`Table`], a
//!   `LazyLock`) and its hot loops pay nothing.
//! * **Without an install** every accessor names the missing install: [`program`] and the
//!   `try_` forms return [`Error`] (its `Display` is the text for the user), the plain forms
//!   panic with it. A program that needs the data calls [`check`] at its start and exits with the
//!   text, so the user sees it before any work. Tests that need the data begin with
//!   [`skip_without_install!`], which skips them on a machine with no install (continuous
//!   integration) unless `SI_RT_REQUIRE` is set.
//!
//! Nothing here reproduces a function of the original. Depends on no other crate.

pub mod elf;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub use elf::{Image, ParseError, Segment};

/// The environment variable that names the install root (shared with `si-install` and the add-on).
pub const ENV: &str = "SI_INSTALL";
/// Set (to anything non-empty) to make [`skip_without_install!`] fail instead of skip.
pub const ENV_REQUIRE: &str = "SI_RT_REQUIRE";
/// Development default, relative to the repository root.
pub const DEV_DEFAULT: &str = "re/bin/si3d40-ia/installed";

/// A build of the original program this crate can serve data from. Only numbers of our own: the
/// file's length and our checksum of all of it.
#[derive(Debug, PartialEq, Eq)]
pub struct Build {
    pub name: &'static str,
    /// The file's name in the install.
    pub file_name: &'static str,
    /// Where it sits under the install root (tried before a search by name).
    pub usual_paths: &'static [&'static str],
    pub len: u64,
    /// [`fnv1a64`] of the whole file.
    pub sum: u64,
}

/// The Linux `soft` of Softimage|3D 4.0, the program every `[soft 0x…]` address in this
/// repository refers to.
pub const LINUX_SOFT: Build = Build {
    name: "Softimage|3D 4.0 for Linux, program `soft`",
    file_name: "soft",
    usual_paths: &["Creative-Tools/3D/bin/soft", "linux/soft3d/Creative-Tools/3D/bin/soft", "soft3d/Creative-Tools/3D/bin/soft"],
    len: 8_472_833,
    sum: 0xd913_24dd_bca9_e849,
};

/// The builds known, in the order they are looked for.
pub const BUILDS: &[&Build] = &[&LINUX_SOFT];

/// FNV-1a, 64 bits: our checksum of a file or a range of one.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Why the data cannot be served. `Display` is the text for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// No candidate directory holds the program file: the directories looked in.
    NotFound { looked_in: Vec<PathBuf> },
    /// A file of the right name was found but is not a known build: (path, why).
    WrongBuild { path: PathBuf, why: String },
    Unreadable { path: PathBuf, why: String },
    /// The range is not in the file bytes of the program's segments (a mistake in the crate
    /// that asks: its address or length is wrong).
    Range { slot: String, addr: u32, len: u32 },
    /// The range is there but its checksum is not the one the asking crate expects.
    Sum { slot: String, addr: u32, len: u32, want: u64, got: u64, path: PathBuf },
    /// [`set_program`] was called after the program was already loaded.
    AlreadyLoaded,
}

const HOW: &str = "Fogle3D reads some of the original program's own data from your Softimage|3D 4.0 install when it runs \
                   (none of it ships with Fogle3D): set the environment variable SI_INSTALL to the directory the product \
                   was installed to (the one holding Creative-Tools/ and Creative-Rsrc/), or, in Blender, the add-on \
                   preference \"Softimage 3D 4.0 install path\"";

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound { looked_in } => {
                write!(f, "Softimage|3D 4.0 install not found: no program file `{}` of the Linux build ({} bytes)", LINUX_SOFT.file_name, LINUX_SOFT.len)?;
                if looked_in.is_empty() {
                    write!(f, " (no install directory given)")?;
                } else {
                    write!(f, " in")?;
                    for d in looked_in {
                        write!(f, "\n  - {}", d.display())?;
                    }
                }
                write!(f, "\n{HOW}")
            }
            Error::WrongBuild { path, why } => write!(
                f,
                "{} is not the Softimage|3D 4.0 Linux build Fogle3D was made for ({why}); {HOW}",
                path.display()
            ),
            Error::Unreadable { path, why } => write!(f, "cannot read {}: {why}", path.display()),
            Error::Range { slot, addr, len } => write!(
                f,
                "run-time data `{slot}`: 0x{addr:08x} + {len} bytes is not in the program's initialised data (a mistake in the crate that asks for it)"
            ),
            Error::Sum { slot, addr, len, want, got, path } => write!(
                f,
                "run-time data `{slot}` (0x{addr:08x}, {len} bytes) of {} has checksum 0x{got:016x}, not 0x{want:016x}: the program file is not the expected build",
                path.display()
            ),
            Error::AlreadyLoaded => write!(f, "the program file was already loaded"),
        }
    }
}
impl std::error::Error for Error {}

/// The user's program file, loaded and checked.
pub struct Program {
    pub path: PathBuf,
    /// `None` for a program given with [`set_program`] unchecked (tests).
    pub build: Option<&'static Build>,
    image: Image<'static>,
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program").field("path", &self.path).field("build", &self.build.map(|b| b.name)).finish()
    }
}

impl Program {
    /// A program from the bytes of a file, checked against [`BUILDS`]. The bytes are kept for the
    /// rest of the process.
    pub fn from_bytes(path: PathBuf, file: Vec<u8>) -> Result<Program, Error> {
        let build = BUILDS.iter().copied().find(|b| b.len == file.len() as u64);
        let Some(build) = build else {
            return Err(Error::WrongBuild { path, why: format!("{} bytes, expected {}", file.len(), LINUX_SOFT.len) });
        };
        let got = fnv1a64(&file);
        if got != build.sum {
            return Err(Error::WrongBuild { path, why: format!("checksum 0x{got:016x}, expected 0x{:016x}", build.sum) });
        }
        let file: &'static [u8] = Box::leak(file.into_boxed_slice());
        let image = Image::parse(file).map_err(|e| Error::WrongBuild { path: path.clone(), why: e.to_string() })?;
        Ok(Program { path, build: Some(build), image })
    }

    /// A program from any ELF file, with no build check: for tests on made-up files.
    pub fn unchecked(path: PathBuf, file: Vec<u8>) -> Result<Program, Error> {
        let file: &'static [u8] = Box::leak(file.into_boxed_slice());
        let image = Image::parse(file).map_err(|e| Error::WrongBuild { path: path.clone(), why: e.to_string() })?;
        Ok(Program { path, build: None, image })
    }

    pub fn image(&self) -> &Image<'static> {
        &self.image
    }

    /// The bytes at `addr .. addr + len` (file bytes of a segment).
    pub fn bytes(&self, slot: &str, addr: u32, len: u32) -> Result<&'static [u8], Error> {
        self.image.bytes(addr, len).ok_or_else(|| Error::Range { slot: slot.to_string(), addr, len })
    }

    /// [`Program::bytes`], checked against `want` ([`fnv1a64`] of the range).
    pub fn bytes_checked(&self, slot: &str, addr: u32, len: u32, want: u64) -> Result<&'static [u8], Error> {
        let b = self.bytes(slot, addr, len)?;
        let got = fnv1a64(b);
        if got != want {
            return Err(Error::Sum { slot: slot.to_string(), addr, len, want, got, path: self.path.clone() });
        }
        Ok(b)
    }

    /// The C string at `addr`, without its zero.
    pub fn message(&self, addr: u32) -> Result<&'static [u8], Error> {
        self.image.cstr(addr).ok_or_else(|| Error::Range { slot: format!("message 0x{addr:08x}"), addr, len: 1 })
    }

    /// The word at `addr`.
    pub fn word(&self, addr: u32) -> Result<u32, Error> {
        self.image.word(addr).ok_or_else(|| Error::Range { slot: format!("word 0x{addr:08x}"), addr, len: 4 })
    }
}

static ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static PROGRAM: OnceLock<Result<Program, Error>> = OnceLock::new();

/// Adds a directory to look in before `SI_INSTALL` (a program's own install argument). Has an
/// effect only before the first use; returns whether it was taken.
pub fn add_root(dir: impl AsRef<Path>) -> bool {
    if PROGRAM.get().is_some() {
        return false;
    }
    ROOTS.lock().unwrap_or_else(|e| e.into_inner()).push(dir.as_ref().to_path_buf());
    true
}

/// Uses `p` as the program (a file a caller loaded itself, or a made-up one in a test). Only
/// before the first use.
pub fn set_program(p: Program) -> Result<(), Error> {
    PROGRAM.set(Ok(p)).map_err(|_| Error::AlreadyLoaded)
}

/// The development copy above the working directory or the program.
pub fn dev_default() -> Option<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(c) = std::env::current_dir() {
        starts.push(c);
    }
    if let Ok(e) = std::env::current_exe() {
        starts.push(e);
    }
    for s in starts {
        let mut d: Option<&Path> = Some(&s);
        while let Some(p) = d {
            let c = p.join(DEV_DEFAULT);
            if c.is_dir() {
                return Some(c);
            }
            d = p.parent();
        }
    }
    None
}

/// The directories looked in, in order: [`add_root`]'s, `SI_INSTALL`, the development copy.
pub fn candidate_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ROOTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(e) = std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        v.push(PathBuf::from(e));
    }
    if let Some(d) = dev_default() {
        v.push(d);
    }
    let mut seen = Vec::new();
    v.retain(|p| {
        let k = p.canonicalize().unwrap_or_else(|_| p.clone());
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    v
}

/// Files under `dir` named `name` with exactly `len` bytes (no symlinks followed, at most ten
/// levels deep, sorted; a path holding `/linux/` first). Only names and sizes are looked at.
pub fn find_files(dir: &Path, name: &str, len: u64) -> Vec<PathBuf> {
    fn walk(dir: &Path, name: &str, len: u64, out: &mut Vec<PathBuf>, depth: u32) {
        if depth > 10 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                walk(&e.path(), name, len, out, depth + 1);
            } else if t.is_file() && e.file_name() == name && e.metadata().is_ok_and(|m| m.len() == len) {
                out.push(e.path());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, name, len, &mut out, 0);
    out.sort();
    out.sort_by_key(|p| !p.to_string_lossy().contains("/linux/"));
    out
}

/// Looks for a known build under `root`: the usual paths first, then a search by name and length.
pub fn load_from(root: &Path) -> Result<Program, Error> {
    let mut wrong = None;
    for b in BUILDS {
        let mut paths: Vec<PathBuf> = b.usual_paths.iter().map(|p| root.join(p)).filter(|p| p.is_file()).collect();
        if root.is_file() && root.file_name().is_some_and(|n| n == b.file_name) {
            paths.insert(0, root.to_path_buf());
        }
        if paths.iter().all(|p| !std::fs::metadata(p).is_ok_and(|m| m.len() == b.len)) {
            paths.extend(find_files(root, b.file_name, b.len));
        }
        for p in paths {
            match std::fs::metadata(&p) {
                Ok(m) if m.len() != b.len => {
                    wrong.get_or_insert(Error::WrongBuild { path: p, why: format!("{} bytes, expected {}", m.len(), b.len) });
                    continue;
                }
                Err(e) => {
                    wrong.get_or_insert(Error::Unreadable { path: p, why: e.to_string() });
                    continue;
                }
                _ => {}
            }
            match std::fs::read(&p) {
                Ok(bytes) => match Program::from_bytes(p.clone(), bytes) {
                    Ok(prog) => return Ok(prog),
                    Err(e) => {
                        wrong.get_or_insert(e);
                    }
                },
                Err(e) => {
                    wrong.get_or_insert(Error::Unreadable { path: p, why: e.to_string() });
                }
            }
        }
    }
    Err(wrong.unwrap_or(Error::NotFound { looked_in: vec![root.to_path_buf()] }))
}

fn load() -> Result<Program, Error> {
    let roots = candidate_roots();
    let mut first_wrong = None;
    for r in &roots {
        match load_from(r) {
            Ok(p) => return Ok(p),
            Err(Error::NotFound { .. }) => {}
            Err(e) => {
                first_wrong.get_or_insert(e);
            }
        }
    }
    Err(first_wrong.unwrap_or(Error::NotFound { looked_in: roots }))
}

/// The user's program, found, read and checked on the first call (thread-safe; later calls
/// return the same result).
pub fn program() -> Result<&'static Program, Error> {
    match PROGRAM.get_or_init(load) {
        Ok(p) => Ok(p),
        Err(e) => Err(e.clone()),
    }
}

/// What a program that needs run-time data calls at its start (the same as [`program`]).
pub fn check() -> Result<&'static Program, Error> {
    program()
}

/// [`check`], or the message on the standard error stream and exit status 2.
pub fn check_or_exit(program_name: &str) -> &'static Program {
    match check() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{program_name}: {e}");
            std::process::exit(2)
        }
    }
}

/// Whether the program can be loaded (tests use this, through [`skip_without_install!`]).
pub fn available() -> bool {
    program().is_ok()
}

#[cold]
fn need(slot: &str, e: Error) -> ! {
    panic!("Fogle3D needs `{slot}` from your own Softimage|3D 4.0 install: {e}")
}

/// The bytes at `addr .. addr + len` of the original program; panics with the reason (no install,
/// wrong build) when they cannot be had. `slot` names them in that message.
pub fn table(slot: &str, addr: u32, len: u32) -> &'static [u8] {
    try_table(slot, addr, len).unwrap_or_else(|e| need(slot, e))
}

pub fn try_table(slot: &str, addr: u32, len: u32) -> Result<&'static [u8], Error> {
    program()?.bytes(slot, addr, len)
}

/// The C string at `addr` of the original program, without its zero; panics as [`table`] does.
pub fn message(addr: u32) -> &'static [u8] {
    try_message(addr).unwrap_or_else(|e| need(&format!("message 0x{addr:08x}"), e))
}

pub fn try_message(addr: u32) -> Result<&'static [u8], Error> {
    program()?.message(addr)
}

/// The C string a pointer at `addr` points to (a table of `char *`).
pub fn message_at(addr: u32) -> &'static [u8] {
    try_message_at(addr).unwrap_or_else(|e| need(&format!("message through 0x{addr:08x}"), e))
}

pub fn try_message_at(addr: u32) -> Result<&'static [u8], Error> {
    let p = program()?;
    p.message(p.word(addr)?)
}

/// [`message`] as `&str`, for a message our code formats or prints itself (`eprintln!("{}", ..)`).
/// The original's messages are ASCII; a byte that is not UTF-8 is a mistake in the address, and
/// panics as a missing range does.
pub fn text(addr: u32) -> &'static str {
    try_text(addr).unwrap_or_else(|e| need(&format!("text 0x{addr:08x}"), e))
}

pub fn try_text(addr: u32) -> Result<&'static str, Error> {
    let b = try_message(addr)?;
    std::str::from_utf8(b).map_err(|_| Error::Range { slot: format!("text 0x{addr:08x} (not UTF-8)"), addr, len: b.len() as u32 })
}

/// The word at `addr` of the original program; panics as [`table`] does.
pub fn word(addr: u32) -> u32 {
    try_word(addr).unwrap_or_else(|e| need(&format!("word 0x{addr:08x}"), e))
}

pub fn try_word(addr: u32) -> Result<u32, Error> {
    program()?.word(addr)
}

/// One range of the original program's data, named, with our checksum of it: the form a crate
/// declares its tables in. Read and checked once, on the first [`Table::get`]; after that a
/// `get` is one atomic load.
pub struct Table {
    pub name: &'static str,
    pub addr: u32,
    pub len: u32,
    /// [`fnv1a64`] of the range (`si-rt sum <addr> <len>` prints it).
    pub sum: u64,
    cell: OnceLock<&'static [u8]>,
}

impl Table {
    pub const fn new(name: &'static str, addr: u32, len: u32, sum: u64) -> Table {
        Table { name, addr, len, sum, cell: OnceLock::new() }
    }

    /// The bytes; panics with the reason when they cannot be had.
    pub fn get(&self) -> &'static [u8] {
        self.try_get().unwrap_or_else(|e| need(self.name, e))
    }

    pub fn try_get(&self) -> Result<&'static [u8], Error> {
        if let Some(b) = self.cell.get() {
            return Ok(b);
        }
        let b = program()?.bytes_checked(self.name, self.addr, self.len, self.sum)?;
        Ok(self.cell.get_or_init(|| b))
    }

    /// The address one past the end.
    pub const fn end(&self) -> u32 {
        self.addr + self.len
    }
}

/// Begins a test that needs the user's install: without one it prints why and returns (the test
/// passes as skipped), unless `SI_RT_REQUIRE` is set, when it fails with the reason.
#[macro_export]
macro_rules! skip_without_install {
    () => {
        if let Err(e) = $crate::program() {
            if std::env::var_os($crate::ENV_REQUIRE).is_some_and(|v| !v.is_empty()) {
                panic!("{} is set and the install is missing: {}", $crate::ENV_REQUIRE, e);
            }
            eprintln!("skipped (no Softimage|3D 4.0 install; set {} to run it): {}", $crate::ENV, e.to_string().lines().next().unwrap_or(""));
            return;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_fnv1a64() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn unchecked_program_serves_ranges() {
        let f = elf::tests::tiny();
        let p = Program::unchecked(PathBuf::from("made-up"), f.clone()).unwrap();
        assert_eq!(p.message(0x1000_0110).unwrap(), b"hello");
        assert_eq!(p.bytes("t", 0x2000_0140, 4).unwrap(), &f[0x140..0x144]);
        let s = fnv1a64(&f[0x140..0x144]);
        assert!(p.bytes_checked("t", 0x2000_0140, 4, s).is_ok());
        let e = p.bytes_checked("t", 0x2000_0140, 4, s ^ 1).unwrap_err();
        assert!(matches!(e, Error::Sum { .. }) && e.to_string().contains("not the expected build"), "{e}");
        let e = p.bytes("t", 0x2000_0140, 0x80).unwrap_err();
        assert!(e.to_string().contains("not in the program's initialised data"), "{e}");
    }

    #[test]
    fn a_file_of_another_build_is_refused() {
        let e = Program::from_bytes(PathBuf::from("x/soft"), elf::tests::tiny()).unwrap_err();
        assert!(matches!(e, Error::WrongBuild { .. }), "{e}");
        assert!(e.to_string().contains("SI_INSTALL"), "{e}");
    }

    #[test]
    fn an_empty_directory_is_not_an_install() {
        let d = std::env::temp_dir().join(format!("si-rt-test-empty-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Creative-Tools/3D/bin")).unwrap();
        std::fs::write(d.join("Creative-Tools/3D/bin/soft"), b"too short").unwrap();
        let e = load_from(&d).unwrap_err();
        assert!(matches!(e, Error::WrongBuild { .. }), "{e}");
        std::fs::remove_file(d.join("Creative-Tools/3D/bin/soft")).unwrap();
        let e = load_from(&d).unwrap_err();
        assert!(matches!(e, Error::NotFound { .. }) && e.to_string().contains("install not found"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// With the install (development copy or SI_INSTALL): the program loads and its first
    /// segment starts with the ELF header.
    #[test]
    fn the_install_program_loads() {
        crate::skip_without_install!();
        let p = program().unwrap();
        assert_eq!(p.build, Some(&LINUX_SOFT));
        assert_eq!(p.image().segments.len(), 2);
        assert_eq!(table("elf header", 0x0804_8000, 4), b"\x7fELF");
        // a message through a pointer and as text: the first si_errorFormat entry's text
        // (si-app's error_table holds the checksum of all 150 of them)
        let m = message_at(0x0868_6a68);
        assert!(!m.is_empty() && m.iter().all(|c| c.is_ascii()));
        assert_eq!(text(word(0x0868_6a68)).as_bytes(), m);
    }
}
