// SPDX-License-Identifier: MIT
//! Paths typed by people: environment variables, home directories and `..`.

use crate::{concat, find, or_null, Bytes};

/// The process environment and the user database, as the path routines consult them.
pub trait Env {
    /// `getenv(name)`.
    fn var(&self, name: &[u8]) -> Option<Bytes>;
    /// `getpwnam(user)->pw_dir`, `None` for an unknown user.
    fn home_of(&self, user: &[u8]) -> Option<Bytes>;
    /// `getpwuid(uid)->pw_name`, `None` for an unknown user id.
    fn user_name(&self, uid: u32) -> Option<Bytes>;
}

/// An environment given as data.
#[derive(Clone, Debug, Default)]
pub struct MapEnv {
    pub vars: Vec<(Bytes, Bytes)>,
    pub homes: Vec<(Bytes, Bytes)>,
    /// User ids and their names.
    pub users: Vec<(u32, Bytes)>,
}

impl MapEnv {
    pub fn new(vars: &[(&str, &str)], homes: &[(&str, &str)]) -> MapEnv {
        let own = |l: &[(&str, &str)]| l.iter().map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec())).collect();
        MapEnv { vars: own(vars), homes: own(homes), users: Vec::new() }
    }
}

impl Env for MapEnv {
    fn var(&self, name: &[u8]) -> Option<Bytes> {
        self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn home_of(&self, user: &[u8]) -> Option<Bytes> {
        self.homes.iter().find(|(k, _)| k == user).map(|(_, v)| v.clone())
    }

    fn user_name(&self, uid: u32) -> Option<Bytes> {
        self.users.iter().find(|(k, _)| *k == uid).map(|(_, v)| v.clone())
    }
}

/// The environment of this process. Home directories are read from `/etc/passwd` only
/// (the original asks the C library, which may also consult network user databases).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnv;

#[cfg(unix)]
impl Env for ProcessEnv {
    fn var(&self, name: &[u8]) -> Option<Bytes> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        if name.is_empty() || name.contains(&b'=') || name.contains(&0) {
            return None;
        }
        std::env::var_os(std::ffi::OsStr::from_bytes(name)).map(|v| v.into_vec())
    }

    fn home_of(&self, user: &[u8]) -> Option<Bytes> {
        let passwd = std::fs::read("/etc/passwd").ok()?;
        passwd.split(|&c| c == b'\n').find_map(|line| {
            let f: Vec<&[u8]> = line.split(|&c| c == b':').collect();
            (f.len() >= 6 && f[0] == user).then(|| f[5].to_vec())
        })
    }

    fn user_name(&self, uid: u32) -> Option<Bytes> {
        let passwd = std::fs::read("/etc/passwd").ok()?;
        let uid = uid.to_string();
        passwd.split(|&c| c == b'\n').find_map(|line| {
            let f: Vec<&[u8]> = line.split(|&c| c == b':').collect();
            (f.len() >= 3 && f[2] == uid.as_bytes()).then(|| f[0].to_vec())
        })
    }
}

/// Removes `dir/..` pairs from a path, in place [soft 0x08121e30 SI_dbPathSimplify].
///
/// While the path contains `..` somewhere after its first character: the slash before it
/// is cut, the path is cut again after the slash before *that*, and what followed `../` is
/// appended. At the end one trailing slash is removed. The routine is rougher than that
/// sounds, and the roughness is kept:
///
/// - It looks for the two characters `..` anywhere, so a name like `a..b` stops it. It
///   stops by returning at once, **without** removing a trailing slash; the same when the
///   `..` is the first component after the root (`/../x`).
/// - A path with no `..` at all also returns at once, trailing slash kept.
/// - When the part before `/..` has no slash, only its first character is kept:
///   `ab/../c` becomes `ac`.
/// - A path that starts with `..` is left alone apart from the trailing slash.
///
/// The original works in a 1024-byte copy on the stack.
pub fn path_simplify(path: &mut Bytes) {
    let mut buf = path.clone();
    let Some(mut p) = find(&buf, b"..") else { return };
    if p != 0 {
        loop {
            // what follows "../", or the end of the string when the path ends first
            let after = (p + 3).min(buf.len());
            p -= 1;
            if p == 0 || buf[p] != b'/' {
                return;
            }
            let keep = buf[..p].iter().rposition(|&c| c == b'/').unwrap_or(0);
            *path = concat(&[&buf[..=keep], &buf[after..]]);
            buf = path.clone();
            match find(&buf, b"..") {
                None | Some(0) => break,
                Some(next) => p = next,
            }
        }
    }
    if path.last() == Some(&b'/') {
        path.pop();
    }
}

/// Expands a leading `~` or `~user` [soft 0x08121fc4 SI_dbPathExpandUserString]. `None`
/// when the path does not start with `~` or the user is unknown.
///
/// - `~` and `~/rest`: the value of `HOME`, then the rest of the path from its slash. With
///   `HOME` unset the original prints a null pointer, which glibc renders as `(null)`.
/// - `~user/rest`: the user's home directory, a slash, and the rest. **A rest of a single
///   character is dropped** (`~bob/x` gives bob's home directory): the test for "nothing
///   follows the slash" is off by one.
///
/// The user name goes through a 128-byte buffer in the original, unchecked.
pub fn path_expand_user_string(path: &[u8], env: &dyn Env) -> Option<Bytes> {
    let len = path.len();
    if path.first() != Some(&b'~') {
        return None;
    }
    if len == 1 || path[1] == b'/' {
        return Some(concat(&[or_null(&env.var(b"HOME")), &path[1..]]));
    }
    let slash = path[1..].iter().position(|&c| c == b'/').map_or(len, |k| k + 1);
    let dir = env.home_of(&path[1..slash])?;
    let rest = slash + 1;
    if rest + 1 >= len {
        Some(dir)
    } else {
        Some(concat(&[&dir, b"/", &path[rest..]]))
    }
}

/// Replaces each `$NAME` by the value of the environment variable `NAME`, where the name
/// runs to the next slash or the end [soft 0x08122134 SI_dbPathExpandEnvVars]. An unset
/// variable is left as written. Returns the new path and whether anything was replaced.
///
/// The search for the next `$` resumes one character after the previous one, in the changed
/// text, so a `$` inside a value is expanded too.
///
/// Where this must differ from the original: for `$/…` the original looks up a name whose
/// first byte it never set, and after replacing a trailing `$NAME` by an empty value it
/// goes on searching one byte past the string's terminator. Here the first is a lookup of
/// the empty name and the second ends the search. The name buffer is 128 bytes in the
/// original and the output buffer is the caller's; neither is checked.
pub fn path_expand_env_vars(path: &[u8], env: &dyn Env) -> (Bytes, bool) {
    let mut out = path.to_vec();
    let mut found = false;
    let mut from = 0;
    while from <= out.len() {
        let Some(at) = out[from..].iter().position(|&c| c == b'$').map(|k| from + k) else { break };
        let end = out[at + 1..].iter().position(|&c| c == b'/').map_or(out.len(), |k| at + 1 + k);
        if let Some(value) = env.var(&out[at + 1..end]) {
            out.splice(at..end, value);
            found = true;
        }
        from = at + 1;
    }
    (out, found)
}

/// A path as typed, made usable [soft 0x08122378 SI_dbPathExpandShortCuts]: environment
/// variables, then a leading `~`, then [`path_simplify`].
pub fn path_expand_shortcuts(path: &[u8], env: &dyn Env) -> Bytes {
    let (mut out, _) = path_expand_env_vars(path, env);
    if let Some(expanded) = path_expand_user_string(&out, env) {
        out = expanded;
    }
    path_simplify(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple(p: &str) -> String {
        let mut b = p.as_bytes().to_vec();
        path_simplify(&mut b);
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn simplify() {
        assert_eq!(simple("/x/a/../b"), "/x/b");
        assert_eq!(simple("/x/a/../b/"), "/x/b");
        assert_eq!(simple("/x/a/.."), "/x");
        assert_eq!(simple("/x/a/b/../../c"), "/x/c");
        // no "..": nothing happens, not even to the trailing slash
        assert_eq!(simple("/x/a/"), "/x/a/");
        assert_eq!(simple(""), "");
        // the rough edges
        assert_eq!(simple("ab/../c"), "ac");
        assert_eq!(simple("/../x/"), "/../x/");
        assert_eq!(simple("/x/a..b/"), "/x/a..b/");
        assert_eq!(simple("../x/"), "../x");
        assert_eq!(simple("x/.."), "x");
    }

    #[test]
    fn user_strings() {
        let env = MapEnv::new(&[("HOME", "/home/me")], &[("bob", "/home/bob")]);
        let u = |p: &str| path_expand_user_string(p.as_bytes(), &env).map(|b| String::from_utf8(b).unwrap());
        assert_eq!(u("/abs"), None);
        assert_eq!(u(""), None);
        assert_eq!(u("~").unwrap(), "/home/me");
        assert_eq!(u("~/db").unwrap(), "/home/me/db");
        assert_eq!(u("~bob").unwrap(), "/home/bob");
        assert_eq!(u("~bob/").unwrap(), "/home/bob");
        assert_eq!(u("~bob/x").unwrap(), "/home/bob"); // one character after the slash is lost
        assert_eq!(u("~bob/xy").unwrap(), "/home/bob/xy");
        assert_eq!(u("~eve/xy"), None);
        let none = MapEnv::default();
        assert_eq!(path_expand_user_string(b"~/db", &none).unwrap(), b"(null)/db");
    }

    #[test]
    fn env_vars() {
        let env = MapEnv::new(&[("DB", "/data/db"), ("E", ""), ("LONGNAME", "s"), ("N", "$DB"), ("M", "x/$DB")], &[]);
        let e = |p: &str| {
            let (b, f) = path_expand_env_vars(p.as_bytes(), &env);
            (String::from_utf8(b).unwrap(), f)
        };
        assert_eq!(e("$DB/models"), ("/data/db/models".into(), true));
        assert_eq!(e("/a/$LONGNAME/b"), ("/a/s/b".into(), true));
        assert_eq!(e("/a/$UNSET/b"), ("/a/$UNSET/b".into(), false));
        assert_eq!(e("$DB"), ("/data/db".into(), true));
        assert_eq!(e("x$E"), ("x".into(), true));
        assert_eq!(e("$/x"), ("$/x".into(), false));
        assert_eq!(e("$DB/$DB"), ("/data/db//data/db".into(), true));
        // the search restarts one past the old dollar, so a value starting with one is not re-read
        assert_eq!(e("$N"), ("$DB".into(), true));
        assert_eq!(e("a$N"), ("a$DB".into(), true));
        // ... but a dollar further inside a value is
        assert_eq!(e("$M"), ("x//data/db".into(), true));
        assert_eq!(e("plain"), ("plain".into(), false));
    }

    #[test]
    fn shortcuts() {
        let env = MapEnv::new(&[("HOME", "/home/me"), ("SI", "~/soft")], &[]);
        let s = |p: &str| String::from_utf8(path_expand_shortcuts(p.as_bytes(), &env)).unwrap();
        assert_eq!(s("$SI/db/../x"), "/home/me/soft/x");
        assert_eq!(s("~/db/"), "/home/me/db/");
        assert_eq!(s("/plain"), "/plain");
    }
}
