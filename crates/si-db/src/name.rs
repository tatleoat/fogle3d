// SPDX-License-Identifier: MIT
//! File names: the parts of a name, splitting a name into them and building it back.
//!
//! An element's file is `<prefix>-<name>.<version>-<substitution>.<suffix>`; the prefix
//! with its dash is optional. The original calls the prefix the *project* (`proj`) and the
//! substitution number the *precision* (`prec`); the suffix belongs to the chapter.

use crate::{concat, dec, or_null, scan_int, Bytes};

/// A text of the form `<int>-<int>` and nothing else, as the two numbers
/// [soft 0x0812283c SI_dbCheckVersionString].
///
/// The original scans with `%d%c%d`, prints the two numbers back with `%d-%d` and accepts
/// only if that reproduces the text. So leading zeros, a plus sign, blanks or anything
/// after the second number disqualify it, while a negative number passes (`1--5` is version
/// 1, substitution −5).
pub fn check_version_string(s: &[u8]) -> Option<(i32, i32)> {
    let (version, n) = scan_int(s)?;
    let sep = *s.get(n)?;
    let (subst, _) = scan_int(&s[n + 1..])?;
    if sep != b'-' {
        return None;
    }
    (concat(&[&dec(version), b"-", &dec(subst)]) == s).then_some((version, subst))
}

/// What [`filename_expand`] finds in a file name.
#[derive(Clone, Debug, PartialEq)]
pub struct Expanded {
    /// Empty when the name has none.
    pub prefix: Bytes,
    pub name: Bytes,
    /// 1 when the name carries no version.
    pub version: i32,
    /// 0 when the name carries no version.
    pub subst: i32,
    /// Filled only for a name that has both a version and something after it.
    pub suffix: Bytes,
}

/// Splits a file name into prefix, element name, version, substitution and suffix
/// [soft 0x081228c0 dbfilenameExpand].
///
/// 1. The text after the last dot is tried as a version (`<int>-<int>`). If it is one, the
///    name has no suffix.
/// 2. Otherwise that text is set aside and the text after the dot before it is tried. If
///    it is a version, the text set aside is the suffix.
/// 3. Otherwise the name has no version: version 1, substitution 0 and **no suffix**, and
///    the name is cut at the second dot from the end if there is one, else at the last.
///    `walk.12.pic` and `walk.pic` both leave `walk`; `a.b.c.d` leaves `a.b`.
/// 4. What is left is split at its first dash into prefix and element name, unless that
///    dash is the first character or follows a dot.
///
/// The original writes the five parts into buffers of its callers (512 bytes for prefix
/// and name where they are locals) without checking lengths; here they are vectors.
pub fn filename_expand(name: &[u8]) -> Expanded {
    let mut s = name.to_vec();
    let (mut version, mut subst, mut suffix) = (1, 0, Vec::new());
    if let Some(p) = s.iter().rposition(|&c| c == b'.') {
        if let Some((v, r)) = check_version_string(&s[p + 1..]) {
            (version, subst) = (v, r);
            s.truncate(p);
        } else {
            let tail = s[p + 1..].to_vec();
            s.truncate(p);
            let q = s.iter().rposition(|&c| c == b'.');
            match q.and_then(|q| check_version_string(&s[q + 1..]).map(|vr| (q, vr))) {
                Some((q, (v, r))) => {
                    (version, subst) = (v, r);
                    s.truncate(q);
                    suffix = tail;
                }
                None => {
                    if let Some(q) = q {
                        s.truncate(q);
                    }
                }
            }
        }
    }
    match s.iter().position(|&c| c == b'-') {
        Some(d) if d > 0 && s[d - 1] != b'.' => {
            Expanded { prefix: s[..d].to_vec(), name: s[d + 1..].to_vec(), version, subst, suffix }
        }
        _ => Expanded { prefix: Vec::new(), name: s, version, subst, suffix },
    }
}

/// Copies characters of `name` from `start` up to a dot, a dash, the end of the string or
/// `end` [soft 0x081227e0 dbfilenameTokenRead]. Returns the token, whether the character
/// that stopped it is something other than a dash, and the position after that character.
///
/// With `start` beyond the string the original reads the stopping character from past the
/// terminator; here anything outside the string reads as the terminator.
pub fn filename_token_read(name: &[u8], start: usize, end: usize) -> (Bytes, bool, usize) {
    let at = |i: usize| name.get(i).copied().unwrap_or(0);
    let mut pos = start;
    let mut token = Vec::new();
    while pos < end && !matches!(at(pos), b'.' | b'-' | 0) {
        token.push(at(pos));
        pos += 1;
    }
    (token, at(pos) != b'-', pos + 1)
}

/// The number that follows the first dot of a file name [soft 0x08122aa4 dbfileGetVersion]:
/// the version of `Bike-A_bike.3-0.hrc` (3), the frame of `walk.12.pic` (12).
///
/// `None` stands for the case where no number is there (`walk.pic`, or no dot at all): the
/// original then returns a local variable it never set. Its token buffer holds six
/// characters, so a longer run of characters after the dot overruns it; no limit here.
pub fn file_get_version(name: &[u8]) -> Option<i32> {
    let dot = name.iter().position(|&c| c == b'.').unwrap_or(name.len());
    let (token, _, _) = filename_token_read(name, dot + 1, name.len());
    scan_int(&token).map(|(v, _)| v)
}

/// The element a file name stands for, as one word [soft 0x08122b50 dbfilenameGetPrefix]:
/// the text before the first dot or dash, and when that was a dash, an underscore and the
/// text up to the next dot or dash. `Bike-A_bike.1-0.hrc` gives `Bike_A_bike`.
/// (Both pieces go through 100-byte buffers in the original, unchecked.)
pub fn filename_get_prefix(name: &[u8]) -> Bytes {
    let (first, not_dash, next) = filename_token_read(name, 0, name.len());
    if not_dash {
        return first;
    }
    let (second, _, _) = filename_token_read(name, next, name.len());
    concat(&[&first, b"_", &second])
}

/// Makes a prefix or an element name fit for a file name
/// [soft 0x08121bdc SI_dbnameCheckValidity]. The string is changed in place and returned;
/// an empty string is freed and null comes back.
///
/// - A name of 100 characters or more is cut to 99.
/// - A blank or a slash becomes an underscore, always.
/// - `! " # $ % & ' ( ) * + , : ; < = > ? @ [ \ ] ^ `` ` `` { | } ~` become underscores,
///   unless `loading` is set.
/// - A dash is treated like those in a prefix; in an element name only when the name
///   *starts* with a dash (so a leading dash goes, later ones stay).
/// - In a prefix, a dot that is the last character is treated like a dash.
///
/// `loading` is the 16-bit flag at offset 0x3c of the schematic's global record
/// (0x087fb99c), which `SI_dbLoadScene` sets to 1: names read from a scene keep their
/// special characters.
///
/// One oddity is kept: the "last character" test uses the length before the cut, so in a
/// prefix longer than 99 characters a dot left at the new end is not replaced.
pub fn name_check_validity(name: Option<Bytes>, is_prefix: bool, loading: bool) -> Option<Bytes> {
    let mut s = name?;
    let len = s.len();
    if len == 0 {
        return None;
    }
    // the original writes the terminator at 99 first and still walks the old length; the
    // characters beyond the cut are no longer part of the string, so only the first 99 matter
    s.truncate(99);
    for i in 0..s.len() {
        let special = match s[i] {
            b' ' | b'/' => {
                s[i] = b'_';
                false
            }
            b'!'..=b',' | b':'..=b'@' | b'['..=b'^' | b'`' | b'{'..=b'~' => true,
            b'-' => is_prefix || s[0] == b'-',
            b'.' => is_prefix && i + 1 >= len,
            _ => false,
        };
        if special && !loading {
            s[i] = b'_';
        }
    }
    Some(s)
}

/// Prefix and element name joined: `prefix-name`, or the name alone when the prefix is null
/// or empty [soft 0x08121ca8 SI_dbFormatPrefixName]. A null name prints as `(null)`.
pub fn format_prefix_name(prefix: &Option<Bytes>, name: &Option<Bytes>) -> Bytes {
    match prefix {
        Some(p) if !p.is_empty() => concat(&[p, b"-", or_null(name)]),
        _ => or_null(name).to_vec(),
    }
}

/// The record the program keeps for one file of a database (`SI_DbListNode`, 0x34 bytes).
/// Its eight strings are owned by the record and freed with it
/// [soft 0x08122450 SI_dbListNodeFree]; here that is the ordinary drop.
#[derive(Clone, Debug, PartialEq)]
pub struct ListNode {
    /// Offset 0x00: a word that the database layer only ever sets to 0.
    pub w00: i32,
    /// Offset 0x04: on the node of a *scene*, the file name by which that scene refers to an
    /// element being looked for; set by the scene check (`SI_dbfileCheckElement`, not ported)
    /// and read back by the version purge.
    pub s04: Option<Bytes>,
    /// Offset 0x08: the sub-directory of the chapter the file is in, empty or `dir/…/`.
    /// A value starting with `/`, `~` or `$` is a whole directory outside the database.
    pub sub_path: Option<Bytes>,
    /// Offset 0x0c: the second argument of [`scn_get_list_node`], kept when not empty.
    pub s0c: Option<Bytes>,
    /// Offset 0x10: the prefix.
    pub prefix: Option<Bytes>,
    /// Offset 0x14: the element name.
    pub name: Option<Bytes>,
    /// Offset 0x18: the version, as text. `1` in a new node.
    pub version: Option<Bytes>,
    /// Offset 0x1c: the substitution number, as text. `0` in a new node.
    pub subst: Option<Bytes>,
    /// Offset 0x20: a time in seconds: the time the node was made, or the file's
    /// modification time after a directory scan.
    pub time: i32,
    /// Offset 0x24: the file name without suffix (`prefix-name.version-subst`).
    pub full: Option<Bytes>,
    /// Offsets 0x28 and 0x2a: two 16-bit fields the database layer sets to 0.
    pub w28: i16,
    pub w2a: i16,
    /// Offset 0x2c: the low 16 bits of the file's mode after a scan.
    pub mode: u16,
    /// Offset 0x30: the file's size after a scan. A new node leaves it unset; 0 here.
    pub size: i32,
}

impl ListNode {
    /// A new node [soft 0x0812250c SI_dbListNodeAllocate]: empty sub-directory, version
    /// `1`, substitution `0`, everything else null or zero. `now` is what `time(NULL)`
    /// returned.
    pub fn allocate(now: i32) -> ListNode {
        ListNode {
            w00: 0,
            s04: None,
            sub_path: Some(Vec::new()),
            s0c: None,
            prefix: None,
            name: None,
            version: Some(b"1".to_vec()),
            subst: Some(b"0".to_vec()),
            time: now,
            full: None,
            w28: 0,
            w2a: 0,
            mode: 0,
            size: 0,
        }
    }
}

/// Cleans the node's prefix and element name with [`name_check_validity`], stores the
/// cleaned strings back in the node, and returns them joined
/// [soft 0x08121d38 SI_dbfilenameSetPrefix].
pub fn filename_set_prefix(node: &mut ListNode, loading: bool) -> Bytes {
    node.prefix = name_check_validity(node.prefix.take(), true, loading);
    node.name = name_check_validity(node.name.take(), false, loading);
    format_prefix_name(&node.prefix, &node.name)
}

/// Prefix and element name joined without cleaning and without the empty-prefix test
/// [soft 0x08122690 SI_dbfilenameSetPrjName]: only a null prefix is left out, so an empty
/// one gives `-name`.
pub fn filename_set_prj_name(node: &ListNode) -> Bytes {
    match &node.prefix {
        None => or_null(&node.name).to_vec(),
        Some(p) => concat(&[p, b"-", or_null(&node.name)]),
    }
}

/// The node's file name without suffix, `prefix-name.version-subst`
/// [soft 0x081223dc SI_dbFormatFullName].
///
/// The original frees the node's stored full name and sets it to null, cleans prefix and
/// name through [`filename_set_prefix`], and returns a fresh copy of the new name; storing
/// it in the node is left to the caller, and most callers do. The same here.
pub fn format_full_name(node: &mut ListNode, loading: bool) -> Bytes {
    node.full = None;
    let prefixed = filename_set_prefix(node, loading);
    concat(&[&prefixed, b".", or_null(&node.version), b"-", or_null(&node.subst)])
}

/// Steps an element's substitution number and renames its node to match
/// [soft 0x08126394 SI_dbfilenameChangePrecision]. `precision` is the element's own copy of
/// the number (its 16-bit field at offset 0x4a). With `down` it is lowered by one, and the
/// routine refuses (false, nothing changed) when it is already 0; otherwise it is raised by
/// one. The node's substitution text and full name are then rewritten.
pub fn filename_change_precision(precision: &mut i16, node: &mut ListNode, down: bool, loading: bool) -> bool {
    if down {
        if *precision == 0 {
            return false;
        }
        *precision = precision.wrapping_sub(1);
    } else {
        *precision = precision.wrapping_add(1);
    }
    node.subst = Some(dec(*precision as i32));
    node.full = Some(format_full_name(node, loading));
    true
}

/// `name.suffix` [soft 0x08121d9c SI_dbfilenameAddSuffix].
///
/// The original compares the last four characters of the name with the suffix
/// (`strncmp(…, 4)`) and adds the suffix when they differ. For a suffix of three
/// characters or fewer they always differ, because the comparison includes the suffix's
/// terminator. For a four-character suffix (`aiff`) a name already ending in those four
/// characters is found equal, **and then nothing is formatted at all**: the function returns
/// a copy of whatever its shared work buffer still holds from an earlier call. That case
/// is `None` here.
///
/// A name shorter than four characters makes the original read before the start of the
/// string; here such a name always counts as different.
pub fn filename_add_suffix(name: &[u8], suffix: &[u8]) -> Option<Bytes> {
    if name.len() >= 4 {
        // strncmp over at most four characters, stopping after a terminator in both
        let tail = &name[name.len() - 4..];
        let mut equal = true;
        for i in 0..4 {
            let b = suffix.get(i).copied().unwrap_or(0);
            if tail[i] != b {
                equal = false;
                break;
            }
            if b == 0 {
                break;
            }
        }
        if equal {
            return None;
        }
    }
    Some(concat(&[name, b".", suffix]))
}

/// The last component of a path [soft 0x08121d84 SI_dbfilenameRemovePath], which is the C
/// library's `basename` (the POSIX one, which also drops trailing slashes): `.` for an
/// empty path, `/` for a path of slashes only.
pub fn filename_remove_path(path: &[u8]) -> Bytes {
    basename(path).0
}

/// The last component of `path`, or `default` when the path has no directory part
/// [soft 0x08120dbc SI_dbfileGetNameFromPath]. (The original tests whether `basename`
/// returned its argument unchanged; that is so for a name without a slash and for a
/// lone `/`.)
pub fn file_get_name_from_path(path: &[u8], default: &[u8]) -> Bytes {
    match basename(path) {
        (_, true) => default.to_vec(),
        (b, false) => b,
    }
}

/// POSIX `basename`: the component, and whether the result is the argument itself (same
/// pointer in C).
fn basename(path: &[u8]) -> (Bytes, bool) {
    if path.is_empty() {
        return (b".".to_vec(), false);
    }
    let Some(p) = path.iter().rposition(|&c| c == b'/') else {
        return (path.to_vec(), true);
    };
    if p + 1 < path.len() {
        return (path[p + 1..].to_vec(), false);
    }
    // a slash at the end: drop the run of slashes, then take the component before it
    let mut end = p;
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        // nothing but slashes: the last one, which is the argument itself when there is only one
        return (b"/".to_vec(), path.len() == 1);
    }
    let start = path[..end].iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1);
    (path[start..end].to_vec(), false)
}

/// Where the suffix of a file name starts: the position of the last dot, provided no dash
/// follows it [soft 0x08131980 filenameCheckSuffix]. A dot at position 0 does not count
/// (the search stops before the first character).
pub fn filename_check_suffix(name: &[u8]) -> Option<usize> {
    let mut i = name.len();
    while i > 0 {
        match name.get(i).copied().unwrap_or(0) {
            b'-' => return None,
            b'.' => return Some(i),
            _ => i -= 1,
        }
    }
    None
}

/// The list node of a file name [soft 0x081319f0 SI_dbScnGetListNode]: the name is split
/// with [`filename_expand`] and the parts stored as text; the full name is the file name
/// itself, or, with `remove_suffix`, the file name up to the dot found by
/// [`filename_check_suffix`].
///
/// So the stored full name is taken from the file, not built from the parts: for a name
/// without a version (`walk.12.pic`) it stays `walk.12`, while the parts say `walk`, 1, 0.
/// `extra` is stored at offset 0x0c when it is not empty.
pub fn scn_get_list_node(file_name: &[u8], extra: Option<&[u8]>, remove_suffix: bool, now: i32) -> ListNode {
    let parts = filename_expand(file_name);
    let mut node = ListNode::allocate(now);
    node.s0c = extra.filter(|e| !e.is_empty()).map(|e| e.to_vec());
    node.prefix = Some(parts.prefix);
    node.name = Some(parts.name);
    node.version = Some(dec(parts.version));
    node.subst = Some(dec(parts.subst));
    node.full = Some(match filename_check_suffix(file_name) {
        Some(n) if remove_suffix => file_name[..n].to_vec(),
        _ => file_name.to_vec(),
    });
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(name: &str) -> (String, String, i32, i32, String) {
        let e = filename_expand(name.as_bytes());
        let s = |b: Bytes| String::from_utf8(b).unwrap();
        (s(e.prefix), s(e.name), e.version, e.subst, s(e.suffix))
    }

    #[test]
    fn version_strings() {
        assert_eq!(check_version_string(b"1-0"), Some((1, 0)));
        assert_eq!(check_version_string(b"12-3"), Some((12, 3)));
        assert_eq!(check_version_string(b"1--5"), Some((1, -5)));
        assert_eq!(check_version_string(b"-1-0"), Some((-1, 0)));
        for bad in ["01-0", "1-00", "+1-0", " 1-0", "1-0 ", "1-0x", "1", "1-", "1.0", "1_0", "", "hrc", "1- 0", "-0-0", "99999999999-0"] {
            assert_eq!(check_version_string(bad.as_bytes()), None, "{bad}");
        }
    }

    #[test]
    fn expand() {
        let t = |a: &str, b: &str, v, r, s: &str| (a.to_string(), b.to_string(), v, r, s.to_string());
        assert_eq!(ex("Bike-A_bike.1-0.hrc"), t("Bike", "A_bike", 1, 0, "hrc"));
        assert_eq!(ex("Bike-A_bike.12-3"), t("Bike", "A_bike", 12, 3, ""));
        assert_eq!(ex("cube.2-0.hrc"), t("", "cube", 2, 0, "hrc"));
        // a dot inside the element name survives when a version follows
        assert_eq!(ex("a-my.model.1-0.hrc"), t("a", "my.model", 1, 0, "hrc"));
        // no version: no suffix either, and up to two dotted pieces are dropped
        assert_eq!(ex("walk.12.pic"), t("", "walk", 1, 0, ""));
        assert_eq!(ex("walk.pic"), t("", "walk", 1, 0, ""));
        assert_eq!(ex("walk"), t("", "walk", 1, 0, ""));
        assert_eq!(ex("a.b.c.d"), t("", "a.b", 1, 0, ""));
        // only the first dash splits; none at the start or after a dot
        assert_eq!(ex("a-b-c.1-0"), t("a", "b-c", 1, 0, ""));
        assert_eq!(ex("-b.1-0"), t("", "-b", 1, 0, ""));
        assert_eq!(ex("x.-b.1-0.hrc"), t("", "x.-b", 1, 0, "hrc"));
        // a version in the wrong place is not one
        assert_eq!(ex("a.1-0.b.hrc"), t("a.1", "0", 1, 0, ""));
        assert_eq!(ex(""), t("", "", 1, 0, ""));
    }

    #[test]
    fn tokens_versions_and_joined_names() {
        assert_eq!(filename_token_read(b"Bike-A.1-0", 0, 10), (b"Bike".to_vec(), false, 5));
        assert_eq!(filename_token_read(b"Bike-A.1-0", 5, 10), (b"A".to_vec(), true, 7));
        assert_eq!(file_get_version(b"Bike-A_bike.3-0.hrc"), Some(3));
        assert_eq!(file_get_version(b"walk.12.pic"), Some(12));
        assert_eq!(file_get_version(b"walk.12a.pic"), Some(12));
        assert_eq!(file_get_version(b"walk.pic"), None);
        assert_eq!(file_get_version(b"walk"), None);
        assert_eq!(file_get_version(b""), None);
        assert_eq!(filename_get_prefix(b"Bike-A_bike.1-0.hrc"), b"Bike_A_bike");
        assert_eq!(filename_get_prefix(b"cube.1-0.hrc"), b"cube");
        assert_eq!(filename_get_prefix(b"a-b-c.1-0"), b"a_b");
    }

    #[test]
    fn validity() {
        let v = |s: &str, p, l| name_check_validity(Some(s.as_bytes().to_vec()), p, l).map(|b| String::from_utf8(b).unwrap());
        assert_eq!(v("", false, false), None);
        assert_eq!(name_check_validity(None, true, false), None);
        assert_eq!(v("my model/1", false, false).unwrap(), "my_model_1");
        assert_eq!(v("a(b)*c", false, false).unwrap(), "a_b__c");
        assert_eq!(v("a(b)*c d", false, true).unwrap(), "a(b)*c_d"); // blanks go even while loading
        // dashes: all of them in a prefix, only a leading one in a name
        assert_eq!(v("a-b-c", true, false).unwrap(), "a_b_c");
        assert_eq!(v("a-b-c", false, false).unwrap(), "a-b-c");
        assert_eq!(v("-a-b", false, false).unwrap(), "_a-b");
        assert_eq!(v("-a-b", false, true).unwrap(), "-a-b");
        // a dot: only the last character of a prefix
        assert_eq!(v("a.b.", true, false).unwrap(), "a.b_");
        assert_eq!(v("a.b.", false, false).unwrap(), "a.b.");
        // 99 characters at most; the dot test still uses the old length
        let long = format!("{}.{}", "x".repeat(98), "y".repeat(10));
        assert_eq!(v(&long, true, false).unwrap(), format!("{}.", "x".repeat(98)));
        assert_eq!(v(&"z".repeat(99), false, false).unwrap().len(), 99);
        assert_eq!(v(&"z".repeat(100), false, false).unwrap().len(), 99);
        // bytes outside ASCII pass
        assert_eq!(name_check_validity(Some(vec![0xe9, 0x7f, 1]), true, false), Some(vec![0xe9, 0x7f, 1]));
    }

    #[test]
    fn building() {
        let mut n = ListNode::allocate(7);
        assert_eq!((n.version.as_deref(), n.subst.as_deref(), n.time), (Some(&b"1"[..]), Some(&b"0"[..]), 7));
        n.prefix = Some(b"My Bike".to_vec());
        n.name = Some(b"front wheel".to_vec());
        n.version = Some(b"3".to_vec());
        n.full = Some(b"stale".to_vec());
        assert_eq!(format_full_name(&mut n, false), b"My_Bike-front_wheel.3-0");
        assert_eq!(n.full, None); // the caller stores the result
        assert_eq!(n.prefix.as_deref(), Some(&b"My_Bike"[..])); // the node keeps the cleaned parts
        n.prefix = Some(Vec::new());
        assert_eq!(filename_set_prj_name(&n), b"-front_wheel");
        assert_eq!(format_full_name(&mut n, false), b"front_wheel.3-0");
        assert_eq!(n.prefix, None); // an empty prefix is dropped
        assert_eq!(filename_set_prj_name(&n), b"front_wheel");
        n.name = None;
        assert_eq!(format_full_name(&mut n, false), b"(null).3-0");
    }

    #[test]
    fn precision() {
        let mut n = scn_get_list_node(b"Bike-A_bike.3-0.hrc", None, true, 0);
        let mut p = 0i16;
        assert!(!filename_change_precision(&mut p, &mut n, true, false));
        assert_eq!(n.full.as_deref(), Some(&b"Bike-A_bike.3-0"[..]));
        assert!(filename_change_precision(&mut p, &mut n, false, false));
        assert!(filename_change_precision(&mut p, &mut n, false, false));
        assert_eq!((p, n.full.as_deref()), (2, Some(&b"Bike-A_bike.3-2"[..])));
        assert!(filename_change_precision(&mut p, &mut n, true, false));
        assert_eq!((p, n.subst.as_deref(), n.full.as_deref()), (1, Some(&b"1"[..]), Some(&b"Bike-A_bike.3-1"[..])));
    }

    #[test]
    fn suffixes() {
        assert_eq!(filename_add_suffix(b"cube.1-0", b"hrc").unwrap(), b"cube.1-0.hrc");
        assert_eq!(filename_add_suffix(b"x.hrc", b"hrc").unwrap(), b"x.hrc.hrc"); // never recognised
        assert_eq!(filename_add_suffix(b"ab", b"pic").unwrap(), b"ab.pic");
        assert_eq!(filename_add_suffix(b"song.aiff", b"aiff"), None); // the stale-buffer case
        assert_eq!(filename_add_suffix(b"songaiff", b"aiff"), None); // no dot needed
        assert_eq!(filename_add_suffix(b"song", b"aiff").unwrap(), b"song.aiff");
        assert_eq!(filename_check_suffix(b"Bike-A_bike.1-0.hrc"), Some(15));
        assert_eq!(filename_check_suffix(b"Bike-A_bike.1-0"), None);
        assert_eq!(filename_check_suffix(b"walk.12.pic"), Some(7));
        assert_eq!(filename_check_suffix(b".pic"), None);
        assert_eq!(filename_check_suffix(b"nodot"), None);
    }

    #[test]
    fn list_node_from_file_name() {
        let n = scn_get_list_node(b"Bike-A_bike.2-1.hrc", None, true, 0);
        assert_eq!(n.full.as_deref(), Some(&b"Bike-A_bike.2-1"[..]));
        assert_eq!((n.prefix.as_deref(), n.name.as_deref()), (Some(&b"Bike"[..]), Some(&b"A_bike"[..])));
        assert_eq!((n.version.as_deref(), n.subst.as_deref()), (Some(&b"2"[..]), Some(&b"1"[..])));
        let n = scn_get_list_node(b"Bike-A_bike.2-1.hrc", Some(b""), false, 0);
        assert_eq!(n.full.as_deref(), Some(&b"Bike-A_bike.2-1.hrc"[..]));
        assert_eq!(n.s0c, None);
        // a picture keeps its frame number in the full name only
        let n = scn_get_list_node(b"walk.12.pic", Some(b"x"), true, 0);
        assert_eq!(n.full.as_deref(), Some(&b"walk.12"[..]));
        assert_eq!((n.name.as_deref(), n.version.as_deref(), n.s0c.as_deref()), (Some(&b"walk"[..]), Some(&b"1"[..]), Some(&b"x"[..])));
    }

    #[test]
    fn base_names() {
        assert_eq!(filename_remove_path(b"/a/b/c.hrc"), b"c.hrc");
        assert_eq!(filename_remove_path(b"c.hrc"), b"c.hrc");
        assert_eq!(filename_remove_path(b"/a/b/"), b"b");
        assert_eq!(filename_remove_path(b"///"), b"/");
        assert_eq!(filename_remove_path(b""), b".");
        assert_eq!(file_get_name_from_path(b"/a/b", b"dflt"), b"b");
        assert_eq!(file_get_name_from_path(b"b", b"dflt"), b"dflt");
        assert_eq!(file_get_name_from_path(b"/", b"dflt"), b"dflt");
        assert_eq!(file_get_name_from_path(b"", b"dflt"), b".");
    }
}
