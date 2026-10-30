// SPDX-License-Identifier: MIT
//! `userpref.c`: the keys file, the one file of user preferences this source file owns.
//! It holds the user's keyboard shortcuts ("direct keys") and the supra keys (keys that
//! switch a mode while held), as text.
//!
//! Ported: where the file is looked for, the load at start-up, and the writer. The reader
//! (`SI_loadUserPref`, `loadDirectKeys`, `loadSupraKeys`) is not read yet; the load goes
//! to it through [`KeyTables::SI_userPrefFileLoadPath`]. The tables of keys themselves
//! belong to another source file and are reached through [`KeyTables`].
//!
//! The file (default name `SIKeys.sks`): the ASCII file header with the type `SKEY`, then
//! one block per direct key and one per supra key, then `EOF`. Every item starts on a new
//! line, indented by three blanks per level, and is a keyword, a blank and its values, each
//! value followed by a blank; texts are in single quotes.
//!
//! ```text
//! DIRECT_KEY_START
//!    DESCRIPTION '<what the key does>'
//!    KEY_DESCRIPTION '<the key, in words>'
//!    KEY_SEQUENCE '<key>' '<key>' …          up to four
//!    MODULE <n>
//!    GROUP <n>
//!    MENU_ID <n>
//!    MENU_SELECTION <n>
//!    TYPE <n>
//!    POSITION <x> <y>
//!    CUSTOM                                  only for a custom command
//! DIRECT_KEY_END
//!
//! SUPRA_KEY_START
//!    MODE '<mode>'
//!    DESCRIPTION '<text>'
//!    KEY_DESCRIPTION '<text>'                 up to four pairs of these two lines
//!    KEY_SEQUENCE '<key>' '<key>' …
//!    IDKEY <n>
//!    SIDE <n>
//! SUPRA_KEY_END
//!
//! EOF
//! ```

use crate::env::{Arg, Bytes, Recorder, World, P};
use crate::state::App;

/// The file type of a keys file in the ASCII header: `SKEY`.
pub const KEYS_MAGIC: u32 = 0x534b_4559;
/// How many supra keys the table has room for.
pub const SUPRA_KEYS: i32 = 0x44;

/// The key tables (another source file's) and the keys file's reader.
#[allow(non_snake_case)]
pub trait KeyTables {
    /// A getter that answers with a text: `SI_getQuickDescription(i)`,
    /// `SI_getKeyDescription(i)`, `SI_supraKey2SupraString(key)` (null when the key has no
    /// name), `SI_getSupraModeToken(i)`, `SI_getSupraDescription(i)`,
    /// `SI_getSupraKeyDescription(i, j)`.
    fn key_text(&mut self, getter: &'static str, args: &[i32]) -> Option<Bytes>;
    /// A getter that answers with a number: `SI_getQuickKeySequence(i, k)`,
    /// `SI_getQuickModule(i)`, `SI_getQuickGroupId(i)`, `SI_getQuickMenuId(i)`,
    /// `SI_getQuickMenuSelection(i)`, `SI_getQuickType(i)`, `SI_getX(i)`, `SI_getY(i)`,
    /// `SI_getCustom(i)`, `SI_getSupraKey(i, j, k)`, `SI_getSupraIdKey(i)`,
    /// `SI_getSupraSide(i)`.
    fn key_number(&mut self, getter: &'static str, args: &[i32]) -> i32;
    /// `SI_userPrefFileLoadPath(path)`: reads a keys file into the tables; not 0 when it
    /// went well.
    fn SI_userPrefFileLoadPath(&mut self, path: &[u8]) -> i32;
    /// `SI_defaultSupraKeys()`, `SI_setKbShortcuts(NULL, 0)`.
    fn SI_defaultSupraKeys(&mut self);
    fn SI_setKbShortcuts(&mut self);
    /// `SI_windowResetAll()`, `SI_windowRestoreAll()` (window.c), and
    /// `SI_setupFileLoadPath(path)` (setup.c, `si-ascii`): reads a set-up file; not 0 when
    /// it went well.
    fn SI_windowResetAll(&mut self);
    fn SI_windowRestoreAll(&mut self);
    fn SI_setupFileLoadPath(&mut self, path: &[u8]) -> i32;
    /// `SI_dbfileWriteAscHeader(stream, SI_versionOutput, magic)`: the header's text
    /// (`si-file` has the port).
    fn asc_header(&mut self, magic: u32) -> Bytes;
}

#[allow(non_snake_case)]
impl KeyTables for Recorder {
    fn key_text(&mut self, getter: &'static str, args: &[i32]) -> Option<Bytes> {
        let a: Vec<Arg> = args.iter().map(|&v| v.into()).collect();
        self.note(getter, &a);
        // a made-up text that depends on the getter and its arguments; a key below 0 has
        // no name
        if getter == "SI_supraKey2SupraString" && args[0] < 0 {
            return None;
        }
        let list: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        Some(format!("{}.{}", &getter[5..], list.join(".")).into_bytes())
    }
    fn key_number(&mut self, getter: &'static str, args: &[i32]) -> i32 {
        let a: Vec<Arg> = args.iter().map(|&v| v.into()).collect();
        self.call(getter, &a) as i32
    }
    fn SI_userPrefFileLoadPath(&mut self, path: &[u8]) -> i32 {
        self.call("SI_userPrefFileLoadPath", &[path.into()]) as i32
    }
    fn SI_defaultSupraKeys(&mut self) {
        self.note("SI_defaultSupraKeys", &[]);
    }
    fn SI_setKbShortcuts(&mut self) {
        self.note("SI_setKbShortcuts", &[0.into(), 0.into()]);
    }
    fn SI_windowResetAll(&mut self) {
        self.note("SI_windowResetAll", &[]);
    }
    fn SI_windowRestoreAll(&mut self) {
        self.note("SI_windowRestoreAll", &[]);
    }
    fn SI_setupFileLoadPath(&mut self, path: &[u8]) -> i32 {
        self.call("SI_setupFileLoadPath", &[path.into()]) as i32
    }
    fn asc_header(&mut self, magic: u32) -> Bytes {
        self.note("SI_dbfileWriteAscHeader", &[Arg::Ptr, P.into(), (magic as i32).into()]);
        Vec::new()
    }
}

/// The text being written, with the indent of [soft 0x083dc75c SI_setIndent] (ported in
/// `si-ascii`): a new line and three blanks per level.
struct Out {
    text: Bytes,
    level: i32,
}

impl Out {
    fn set_indent(&mut self, n: i32) {
        self.level = (self.level + n).max(0);
    }
    /// A new line, the indent, a keyword and a blank.
    fn keyword(&mut self, word: &[u8]) {
        self.text.push(b'\n');
        self.text.resize(self.text.len() + 3 * self.level as usize, b' ');
        self.text.extend_from_slice(word);
        self.text.push(b' ');
    }
    fn quoted(&mut self, text: &[u8]) {
        self.text.push(b'\'');
        self.text.extend_from_slice(text);
        self.text.extend_from_slice(b"' ");
    }
    fn number(&mut self, v: i32) {
        self.text.extend_from_slice(format!("{v} ").as_bytes());
    }
}

/// `fprintf("'%s' ", NULL)`: the C library prints `(null)`.
fn or_null(t: Option<Bytes>) -> Bytes {
    t.unwrap_or_else(|| b"(null)".to_vec())
}

impl<W: World + KeyTables> App<W> {
    /// [soft 0x083fc7c8 SI_findFile]: where a resource file of the user is: the name as
    /// given if it can be read (made absolute with the current directory), else
    /// `<directory>/<name>` if the name is not absolute and that can be read; else nothing,
    /// with a message on the status line.
    ///
    /// For an absolute name that cannot be read the original's message is made from a
    /// buffer it has not filled; the port puts the name there.
    pub fn find_file(&mut self, directory: &[u8], name: &[u8]) -> Option<Bytes> {
        if self.w.access(name, 0) != -1 {
            if name.first() == Some(&b'/') {
                return Some(name.to_vec());
            }
            let cwd = self.g.cwd.clone().unwrap_or_else(|| b"(null)".to_vec());
            return Some([&cwd[..], b"/", name].concat());
        }
        let message;
        if name.first() != Some(&b'/') {
            let path = [directory, b"/", name].concat();
            if self.w.access(&path, 0) != -1 {
                return Some(path);
            }
            let why = self.w.last_error();
            message = [b"file ".as_slice(), &path, b" :(", &why, b")\n"].concat();
        } else {
            let why = self.w.last_error();
            message = [b"Setup file ".as_slice(), name, b": (", &why, b")\n"].concat();
        }
        self.w.SI_statusDisplayMessage(&message, 3);
        None
    }

    /// [soft 0x083be744 SI_setupLoadFromServer]: loads a set-up file: the file is looked for
    /// ([`App::find_file`]; none: 0); the windows are reset, the file is read, and when that
    /// worked the windows are restored. 1 when the file was read, else 0.
    pub fn setup_load_from_server(&mut self, directory: &[u8], name: &[u8]) -> i32 {
        let Some(path) = self.find_file(directory, name) else {
            return 0;
        };
        self.w.SI_windowResetAll();
        if self.w.SI_setupFileLoadPath(&path) == 0 {
            return 0;
        }
        self.w.SI_windowRestoreAll();
        1
    }

    /// [soft 0x083fc744 SI_supraKeysLoadFromServer]: loads the keys at start-up: the file
    /// is looked for ([`App::find_file`]); without one the built-in supra keys are set;
    /// with one it is read, handed to the swift-mouse library if that is loaded, and the
    /// keyboard shortcuts are set from it. 1 when a file was read.
    pub fn supra_keys_load_from_server(&mut self, resources: &[u8], file: Option<&[u8]>) -> i32 {
        let Some(path) = self.find_file(resources, file.unwrap_or(b"")) else {
            self.w.SI_defaultSupraKeys();
            return 0;
        };
        if self.w.SI_userPrefFileLoadPath(&path) == 0 {
            return 0;
        }
        if self.g.swift_mouse[2] != 0 {
            // SI_loadSwiftMouseDSO(path): a call into the optional library
            self.w.dlsym_call(self.g.swift_mouse[2], &path);
        }
        self.w.SI_setKbShortcuts();
        1
    }

    /// [soft 0x083fcc64 saveDirectKeys]: the blocks of the first `count` direct keys.
    fn save_direct_keys(&mut self, out: &mut Out, count: i32) {
        for i in 0..count {
            out.set_indent(0);
            out.keyword(b"DIRECT_KEY_START");
            out.set_indent(1);
            out.keyword(b"DESCRIPTION");
            let t = self.w.key_text("SI_getQuickDescription", &[i]);
            out.quoted(&or_null(t));
            out.keyword(b"KEY_DESCRIPTION");
            let t = self.w.key_text("SI_getKeyDescription", &[i]);
            out.quoted(&or_null(t));
            out.keyword(b"KEY_SEQUENCE");
            for k in 0..4 {
                let key = self.w.key_number("SI_getQuickKeySequence", &[i, k]);
                if let Some(name) = self.w.key_text("SI_supraKey2SupraString", &[key]) {
                    out.quoted(&name);
                }
            }
            for (word, getter) in [
                (&b"MODULE"[..], "SI_getQuickModule"),
                (b"GROUP", "SI_getQuickGroupId"),
                (b"MENU_ID", "SI_getQuickMenuId"),
                (b"MENU_SELECTION", "SI_getQuickMenuSelection"),
                (b"TYPE", "SI_getQuickType"),
            ] {
                out.keyword(word);
                let v = self.w.key_number(getter, &[i]);
                out.number(v);
            }
            out.keyword(b"POSITION");
            let x = self.w.key_number("SI_getX", &[i]);
            out.number(x);
            let y = self.w.key_number("SI_getY", &[i]);
            out.number(y);
            if self.w.key_number("SI_getCustom", &[i]) as i16 != 0 {
                out.keyword(b"CUSTOM");
            }
            out.set_indent(-1);
            out.keyword(b"DIRECT_KEY_END");
            out.text.push(b'\n');
        }
    }

    /// [soft 0x083fd004 saveSupraKeys]: the blocks of the supra keys: each of the 68 table
    /// entries whose first key is set.
    fn save_supra_keys(&mut self, out: &mut Out) {
        for i in 0..SUPRA_KEYS {
            if self.w.key_number("SI_getSupraKey", &[i, 0, 0]) == -1 {
                continue;
            }
            out.set_indent(0);
            out.keyword(b"SUPRA_KEY_START");
            out.set_indent(1);
            out.keyword(b"MODE");
            let t = self.w.key_text("SI_getSupraModeToken", &[i]);
            out.quoted(&or_null(t));
            out.keyword(b"DESCRIPTION");
            let t = self.w.key_text("SI_getSupraDescription", &[i]);
            out.quoted(&or_null(t));
            for j in 0..4 {
                if self.w.key_number("SI_getSupraKey", &[i, j, 0]) == -1 {
                    continue;
                }
                out.keyword(b"KEY_DESCRIPTION");
                let t = self.w.key_text("SI_getSupraKeyDescription", &[i, j]);
                out.quoted(&or_null(t));
                out.keyword(b"KEY_SEQUENCE");
                for k in 0..4 {
                    let key = self.w.key_number("SI_getSupraKey", &[i, j, k]);
                    if let Some(name) = self.w.key_text("SI_supraKey2SupraString", &[key]) {
                        out.quoted(&name);
                    }
                }
            }
            out.keyword(b"IDKEY");
            let v = self.w.key_number("SI_getSupraIdKey", &[i]);
            out.number(v);
            out.keyword(b"SIDE");
            let v = self.w.key_number("SI_getSupraSide", &[i]);
            out.number(v);
            out.set_indent(-1);
            out.keyword(b"SUPRA_KEY_END");
            out.text.push(b'\n');
        }
    }

    /// The text [`App::user_pref_file_save_path`] writes after the header, for a check of
    /// the two block writers on their own.
    pub fn keys_text(&mut self, direct_keys: Option<i32>, supra: bool, level: i32) -> Bytes {
        let mut out = Out { text: Vec::new(), level };
        if let Some(n) = direct_keys {
            self.save_direct_keys(&mut out, n);
        }
        if supra {
            self.save_supra_keys(&mut out);
        }
        out.text
    }

    /// [soft 0x083fc540 SI_userPrefFileSavePath]: writes the keys file `path` with the
    /// first `direct_keys` direct keys: the header, the direct keys, the supra keys, a new
    /// line and `EOF `. True when the file could be opened and written. (When the program
    /// writes through a pipe, `SI_pipedFiles`, the text goes there instead; not ported.)
    pub fn user_pref_file_save_path(&mut self, path: &[u8], direct_keys: i32) -> bool {
        let mut text = self.w.asc_header(KEYS_MAGIC);
        let mut out = Out { text: Vec::new(), level: 0 };
        self.save_direct_keys(&mut out, direct_keys);
        self.save_supra_keys(&mut out);
        out.keyword(b"EOF");
        text.extend_from_slice(&out.text);
        self.w.write_file(path, &text, false)
    }
}
