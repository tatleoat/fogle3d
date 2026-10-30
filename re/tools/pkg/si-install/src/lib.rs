// SPDX-License-Identifier: MIT
//! Where the user's Softimage|3D 4.0 install is, and whether it holds what our programs read.
//!
//! Release model (owner, 2026-10-29): nothing of Softimage ships with our build; the engine and the
//! Blender add-on read every asset from the user's own install at run time. This module is the one
//! place that finds that install (`docs/install-requirements.md` lists what is read from it; the
//! Python twin is `blender-addon/softimage3d/install.py`, same rules, same texts).
//!
//! Order of precedence for the root: an explicit argument (the old `render-frames <dir>` form), the
//! environment variable `SI_INSTALL`, then, for development only, the unpacked copy under
//! `re/bin/si3d40-ia/installed` found by walking up from the working directory or the program.
//! Nothing here reproduces a function of the original.

use std::path::{Path, PathBuf};

/// The environment variable that names the install root (by name only; its value is a directory).
pub const ENV: &str = "SI_INSTALL";

/// Development default, relative to the repository root.
pub const DEV_DEFAULT: &str = "re/bin/si3d40-ia/installed";

/// What a program needs from the install.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Need {
    /// `render-frames`: the relations resource file and the default picture beside it.
    Render,
    /// `blend2db`: the same plus a template set-up file (and a database to render from is the user's own).
    Blend2db,
}

/// A file the programs read, by what it is called and how it is found.
pub struct Item {
    pub what: &'static str,
    pub file_name: &'static str,
    /// Directory the file sits in, relative to its resource directory; `None`: searched by name.
    pub needed_by: &'static [Need],
    pub purpose: &'static str,
}

/// The inventory in code form (the doc `docs/install-requirements.md` is the long version).
pub const ITEMS: &[Item] = &[
    Item { what: "relations resource file", file_name: "Relations.rsrc", needed_by: &[Need::Render, Need::Blend2db],
           purpose: "the relation dictionary every scene load and evaluation is built from" },
    Item { what: "default picture", file_name: "noIcon.pic", needed_by: &[Need::Render, Need::Blend2db],
           purpose: "the picture used for a texture/icon that is missing; sits beside Relations.rsrc" },
    Item { what: "template render set-up", file_name: "MODEL-Gears.1-0.sts", needed_by: &[Need::Blend2db],
           purpose: "a stock set-up file in a sample database that blend2db starts a new database from" },
    // Checked by crates/si-rt when render-frames and si-import-dump start (`si_rt::check_or_exit`: length and our
    // checksum), not by `validate`: render-frames' argument may be a database blend2db built, which holds copies of
    // the two resource files but not the program file; si-rt looks under that root, then SI_INSTALL, then the dev copy.
    Item { what: "program file of the Linux build", file_name: "Creative-Tools/3D/bin/soft", needed_by: &[Need::Render],
           purpose: "the original program's own data tables and message texts, read from it at run time (8,472,833 bytes; never run)" },
];

/// What was found.
#[derive(Debug, Clone)]
pub struct Install {
    pub root: PathBuf,
    /// Directory holding `Relations.rsrc` (the program's resource directory).
    pub rsrc_dir: PathBuf,
    pub rsrc: PathBuf,
    pub no_icon: PathBuf,
    /// Only searched for with `Need::Blend2db`.
    pub template_sts: Option<PathBuf>,
}

/// Why an install is not usable; `Display` is the text for the user.
#[derive(Debug)]
pub enum Error {
    /// No path was given anywhere.
    NotSet,
    NotADirectory(PathBuf),
    /// The directory exists but lacks files: (what, file name).
    Missing { root: PathBuf, missing: Vec<(&'static str, &'static str)> },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotSet => write!(
                f,
                "Softimage|3D 4.0 install path not set: give the directory of your own installed copy as the argument, \
                 or in the environment variable {ENV} (the Blender add-on: Preferences > Add-ons > Softimage 3D > \
                 \"Softimage 3D 4.0 install path\")"
            ),
            Error::NotADirectory(p) => write!(f, "Softimage|3D 4.0 install path is not a directory: {}", p.display()),
            Error::Missing { root, missing } => {
                write!(f, "{} does not look like a Softimage|3D 4.0 install; missing:", root.display())?;
                for (what, name) in missing {
                    write!(f, "\n  - {what} ({name})")?;
                }
                write!(f, "\nPoint {ENV} (or the add-on's install path) at the directory the product was installed to, the one that holds Creative-Rsrc/")
            }
        }
    }
}
impl std::error::Error for Error {}

/// Finds `name` below `dir` (breadth-limited recursion, sorted, directory names preferred in the
/// order `prefer` lists: a path holding `/linux/` first, as the add-on always picked). Symlinks are not followed.
pub fn find(dir: &Path, name: &str, prefer: &[&str]) -> Option<PathBuf> {
    let mut hits = Vec::new();
    walk(dir, name, &mut hits, 0);
    hits.sort();
    for p in prefer {
        if let Some(h) = hits.iter().find(|h| h.to_string_lossy().contains(p)) {
            return Some(h.clone());
        }
    }
    hits.into_iter().next()
}

fn walk(dir: &Path, name: &str, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 14 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), name, out, depth + 1);
        } else if e.file_name() == name {
            out.push(e.path());
        }
    }
}

/// Checks the directory against the inventory for `need`.
pub fn validate(root: &Path, need: Need) -> Result<Install, Error> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.to_path_buf()));
    }
    let mut missing = Vec::new();
    // the usual layout first (a direct hit costs no walk), the search otherwise
    let direct = root.join("Creative-Rsrc/3D/rsrc/Relations.rsrc");
    let rsrc = if direct.is_file() { Some(direct) } else { find(root, "Relations.rsrc", &["/linux/"]) };
    let (rsrc_dir, no_icon) = match &rsrc {
        Some(r) => {
            let d = r.parent().unwrap().to_path_buf();
            let n = d.join("noIcon.pic");
            if !n.is_file() {
                missing.push((ITEMS[1].what, ITEMS[1].file_name));
            }
            (d, n)
        }
        None => {
            missing.push((ITEMS[0].what, ITEMS[0].file_name));
            missing.push((ITEMS[1].what, ITEMS[1].file_name));
            (PathBuf::new(), PathBuf::new())
        }
    };
    let template_sts = if need == Need::Blend2db {
        let t = find(root, "MODEL-Gears.1-0.sts", &[]);
        if t.is_none() {
            missing.push((ITEMS[2].what, ITEMS[2].file_name));
        }
        t
    } else {
        None
    };
    if !missing.is_empty() {
        return Err(Error::Missing { root: root.to_path_buf(), missing });
    }
    Ok(Install { root: root.to_path_buf(), rsrc: rsrc.unwrap(), rsrc_dir, no_icon, template_sts })
}

/// The development copy: `re/bin/si3d40-ia/installed` above the working directory or the program.
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

/// The root: argument, else `SI_INSTALL` (set and non-empty), else the development copy.
pub fn root(arg: Option<&str>) -> Result<PathBuf, Error> {
    if let Some(a) = arg.filter(|a| !a.is_empty()) {
        return Ok(PathBuf::from(a));
    }
    if let Some(v) = std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(v));
    }
    dev_default().ok_or(Error::NotSet)
}

/// `root` then `validate`: what a program calls at its start.
pub fn locate(arg: Option<&str>, need: Need) -> Result<Install, Error> {
    validate(&root(arg)?, need)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("si-install-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_dir_lists_everything_missing() {
        let d = tmp("empty");
        let e = validate(&d, Need::Blend2db).unwrap_err().to_string();
        assert!(e.contains("Relations.rsrc") && e.contains("noIcon.pic") && e.contains("MODEL-Gears.1-0.sts"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn layout_found() {
        let d = tmp("ok");
        let r = d.join("Creative-Rsrc/3D/rsrc");
        std::fs::create_dir_all(&r).unwrap();
        std::fs::write(r.join("Relations.rsrc"), b"x").unwrap();
        assert!(validate(&d, Need::Render).is_err());
        std::fs::write(r.join("noIcon.pic"), b"x").unwrap();
        let i = validate(&d, Need::Render).unwrap();
        assert_eq!(i.rsrc_dir, r);
        assert!(validate(&d, Need::Blend2db).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_program_file_is_listed() {
        let i = ITEMS.iter().find(|i| i.file_name.ends_with("/bin/soft")).unwrap();
        assert_eq!(i.needed_by, &[Need::Render]);
    }

    #[test]
    fn not_a_directory() {
        assert!(matches!(validate(Path::new("/nonexistent/si"), Need::Render), Err(Error::NotADirectory(_))));
    }
}
