// SPDX-License-Identifier: MIT
//! Checks of the setup reader and writer against original files: what a check binary needs to
//! turn a directory of `.sts` files into counts.
//!
//! Three questions per file. Does the reader get through it, with every token consumed, no
//! word left unexplained and no token asked for after the end? Does the structure survive
//! being written and read again? And how does what this build's writer produces from it
//! compare, byte for byte, with the file itself — which a newer build than the one that wrote
//! the file cannot match exactly, so the differences are sorted by cause.

use super::{read_into, write_with, ReadInfo, Session, Setup, SetupError, WriteOptions};
use std::collections::BTreeMap;

/// How the rewritten file differs from the original, line by line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    /// The first line (tag and version) differs.
    pub header: bool,
    /// Lines the rewrite has and the original does not, by their first word: keywords this
    /// build writes that the build that wrote the file did not.
    pub added: BTreeMap<String, usize>,
    /// Lines of the original that the rewrite does not have, by first word.
    pub removed: BTreeMap<String, usize>,
    /// Lines that differ in place, by first word and kind of difference.
    pub changed: BTreeMap<(String, Change), usize>,
    /// Lines both files have, at different places, by first word: an older build wrote the
    /// keyword elsewhere in its section.
    pub moved: BTreeMap<String, usize>,
    /// Lines the original has twice and the rewrite once, by first word: an older build
    /// wrote the keyword at two places.
    pub doubled: BTreeMap<String, usize>,
    /// Those lines themselves: kind, the original's, the rewrite's.
    pub changed_lines: Vec<(Change, Vec<u8>, Vec<u8>)>,
    /// The original's lines with no counterpart, and the rewrite's.
    pub removed_lines: Vec<Vec<u8>>,
    pub added_lines: Vec<Vec<u8>>,
}

/// Kinds of difference between an original line and the line written in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    /// The same tokens, laid out differently (indentation, trailing blanks).
    Layout,
    /// The same values as numbers, printed differently.
    NumberFormat,
    /// An empty string in the file, `(null)` in the rewrite.
    EmptyString,
    /// The rewrite's line starts with the original's tokens and goes on: this build puts
    /// more on the line.
    Longer,
    /// The original's line starts with the rewrite's tokens and goes on: this build puts
    /// the rest on a line of its own.
    Shorter,
    /// A different word or value.
    Value,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        !self.header && self.added.is_empty() && self.unmatched()
    }

    fn unmatched(&self) -> bool {
        self.removed.is_empty() && self.changed.is_empty() && self.moved.is_empty() && self.doubled.is_empty()
    }

    /// Nothing but the header line.
    pub fn header_only(&self) -> bool {
        self.header && self.added.is_empty() && self.unmatched()
    }

    /// The header and added lines only: every line of the original is reproduced, in order.
    pub fn additions_only(&self) -> bool {
        self.unmatched() && !self.added.is_empty()
    }

    /// Besides the header and added lines, only lines an older build laid out differently:
    /// the same tokens split over lines another way, or the same line at another place.
    pub fn layout_only(&self) -> bool {
        self.removed.is_empty()
            && !(self.changed.is_empty() && self.moved.is_empty() && self.doubled.is_empty())
            && self.changed.keys().all(|(_, k)| matches!(k, Change::Layout | Change::Longer | Change::Shorter))
    }
}

/// Why a rewritten file differs from the original: the causes at work in one file, each
/// explained by the code of the reader or the writer (see the finding).
pub fn causes(d: &Diff, complete: bool) -> std::collections::BTreeSet<&'static str> {
    let mut out = std::collections::BTreeSet::new();
    if d.header {
        out.insert("A header version (this build writes 4)");
    }
    if !d.added.is_empty() {
        out.insert("B keywords this build adds");
    }
    if !d.moved.is_empty() || !d.doubled.is_empty() {
        out.insert("C same tokens, laid out by an older build (line breaks, order, a line twice)");
    }
    if !complete {
        // whatever else differs follows from the words that were skipped
        out.insert("K a word this build does not know: the rest of that section is skipped");
    }
    for (word, kind) in d.changed.keys() {
        let cause = match kind {
            Change::Layout | Change::Longer | Change::Shorter => "C same tokens, laid out by an older build (line breaks, order, a line twice)",
            Change::NumberFormat => "D exponent with three digits (file written on Windows)",
            Change::EmptyString => "E empty string read as none, written (null)",
            Change::Value if !complete => continue,
            Change::Value => match word.as_str() {
                "CAMERA_TYPE" => "F schematic view spelt the older way (SCHEMATIC_PLAIN …)",
                "RENDERING_TYPE" => "G MENTAL_RAY read as MENTAL_RAY_2X (mental ray 1.9 absent)",
                "ACTIVE_MODULE" => "H module renumbered for the file's version",
                "PREF_MODE" | "PREF_ROTATION" | "PREF_TRANSLATION" => "I manipulation modes normalised by the reader",
                "CAMERA_ASPECT" => "J scene camera aspect replaced by the picture format's",
                _ => "Z unexplained value change",
            },
        };
        out.insert(cause);
    }
    let l1 = d.removed.contains_key("WINDOW_LAYOUT");
    for word in d.removed.keys() {
        let cause = match word.as_str() {
            _ if !complete => continue,
            "UDF_ORDER" => "L shader order not written (the shader's kind is not in the file)",
            _ if l1 => "M windows of a second layout (L1_…) this build does not have",
            "DO_DRAW_OVER" | "DO_SEQUENCE" | "(line of numbers)" => "N draw-over block with no names not written",
            _ => "Z unexplained missing line",
        };
        out.insert(cause);
    }
    out
}

/// What happened to one file.
#[derive(Clone, Debug)]
pub struct FileReport {
    /// The version as the header gives it (text between the tag and the first newline).
    pub version: String,
    /// The reader's error, if it gave up.
    pub error: Option<SetupError>,
    pub info: ReadInfo,
    /// Every token consumed, no unknown word, no message, no deviation, and the reader either
    /// left through the word `EOF` without asking for a token past the end or, in a file
    /// without that word, asked exactly twice (the last section and the main loop).
    pub complete: bool,
    /// Reading the rewritten file gives the same structure.
    pub structure_round_trip: bool,
    /// Where it does not: the fields that differ (as the lines of the two debug prints).
    pub structure_differences: Vec<String>,
    /// Writing that second structure gives the rewritten file again.
    pub second_write_identical: bool,
    /// The same one generation on: the structure read from the second write equals the one
    /// read from the first, with the fields that differ if not.
    pub second_round_trip: bool,
    pub second_differences: Vec<String>,
    /// And once more: by the third write the file no longer changes.
    pub third_write_identical: bool,
    /// The indent level (0 or 1) with which the rewrite comes closest to the file.
    pub indent: i32,
    /// The rewrite equals the file byte for byte.
    pub identical: bool,
    pub diff: Diff,
}

/// Splits into lines, each without its newline.
fn lines(data: &[u8]) -> Vec<&[u8]> {
    data.split(|&b| b == b'\n').collect()
}

/// The word a line starts with; lines that carry only numbers (the values of the keyword on
/// the line before) are lumped together.
fn first_word(line: &[u8]) -> String {
    let w: Vec<u8> = line.iter().copied().skip_while(|b| *b == b' ').take_while(|b| *b != b' ').collect();
    if w.is_empty() {
        "(blank line)".to_string()
    } else if w[0].is_ascii_digit() || w[0] == b'-' || w[0] == b'.' {
        "(line of numbers)".to_string()
    } else if w == b"ACTIVE" || w == b"INACTIVE" {
        "(memo camera state)".to_string()
    } else {
        String::from_utf8_lossy(&w).into_owned()
    }
}

/// The tokens of a line: words, with a quoted string as one token.
fn tokens(line: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if line[i] == b' ' || line[i] == b'\t' {
            i += 1;
            continue;
        }
        let start = i;
        if line[i] == b'\'' {
            i += 1;
            while i < line.len() && line[i] != b'\'' {
                i += 1;
            }
            i = (i + 1).min(line.len());
        } else {
            while i < line.len() && line[i] != b' ' && line[i] != b'\t' {
                i += 1;
            }
        }
        out.push(line[start..i].to_vec());
    }
    out
}

fn classify(old: &[u8], new: &[u8]) -> Change {
    let (a, b) = (tokens(old), tokens(new));
    if a == b {
        return Change::Layout;
    }
    if b.len() > a.len() && b[..a.len()] == a[..] {
        return Change::Longer;
    }
    if a.len() > b.len() && a[..b.len()] == b[..] {
        return Change::Shorter;
    }
    if a.len() == b.len() {
        let number = |t: &[u8]| std::str::from_utf8(t).ok().and_then(|s| s.parse::<f64>().ok());
        let mut kinds = Vec::new();
        for (x, y) in a.iter().zip(&b) {
            if x == y {
                continue;
            }
            kinds.push(match (number(x), number(y)) {
                (Some(p), Some(q)) if p as f32 == q as f32 => Change::NumberFormat,
                _ if x.as_slice() == b"''" && y.as_slice() == b"'(null)'" => Change::EmptyString,
                _ => Change::Value,
            });
        }
        if kinds.iter().all(|k| *k == Change::NumberFormat) {
            return Change::NumberFormat;
        }
        if kinds.iter().all(|k| *k == Change::EmptyString) {
            return Change::EmptyString;
        }
    }
    Change::Value
}

/// A line diff by longest common subsequence, then pairing of removed and added lines that
/// stand at the same place and start with the same word.
pub fn diff(original: &[u8], rewritten: &[u8]) -> Diff {
    let mut d = Diff::default();
    let (a, b) = (lines(original), lines(rewritten));
    if a.first() != b.first() {
        d.header = true;
    }
    let (a, b) = (&a[1.min(a.len())..], &b[1.min(b.len())..]);
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: longest common subsequence of a[i..] and b[j..]
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * (m + 1) + j] = if a[i] == b[j] {
                lcs[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                lcs[(i + 1) * (m + 1) + j].max(lcs[i * (m + 1) + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let flush = |removed: &mut Vec<&[u8]>, added: &mut Vec<&[u8]>, d: &mut Diff| {
        // pair lines of one gap that start with the same word, in order
        let mut used = vec![false; added.len()];
        for old in removed.iter() {
            let w = first_word(old);
            match (0..added.len()).find(|&k| !used[k] && first_word(added[k]) == w) {
                Some(k) => {
                    used[k] = true;
                    let kind = classify(old, added[k]);
                    *d.changed.entry((w, kind)).or_default() += 1;
                    d.changed_lines.push((kind, old.to_vec(), added[k].to_vec()));
                }
                None => {
                    *d.removed.entry(w).or_default() += 1;
                    d.removed_lines.push(old.to_vec());
                }
            }
        }
        for (k, new) in added.iter().enumerate() {
            if !used[k] {
                *d.added.entry(first_word(new)).or_default() += 1;
                d.added_lines.push(new.to_vec());
            }
        }
        removed.clear();
        added.clear();
    };
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(&mut removed, &mut added, &mut d);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i * (m + 1) + j + 1] >= lcs[(i + 1) * (m + 1) + j]) {
            added.push(b[j]);
            j += 1;
        } else {
            removed.push(a[i]);
            i += 1;
        }
    }
    flush(&mut removed, &mut added, &mut d);
    let second_layout = a.iter().any(|l| l.windows(15).any(|w| w == b"WINDOW_TYPE L1_"));
    // a removed line that turns up, byte for byte, among the added ones has moved
    let mut spare: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for line in &d.added_lines {
        *spare.entry(line.clone()).or_default() += 1;
    }
    let mut kept = Vec::new();
    for line in std::mem::take(&mut d.removed_lines) {
        let w = first_word(&line);
        match spare.get_mut(&line) {
            Some(n) if *n > 0 && d.added.get(&w).is_some_and(|c| *c > 0) => {
                *n -= 1;
                *d.moved.entry(w.clone()).or_default() += 1;
                for map in [&mut d.removed, &mut d.added] {
                    if let Some(c) = map.get_mut(&w) {
                        *c -= 1;
                        if *c == 0 {
                            map.remove(&w);
                        }
                    }
                }
            }
            // (not in a file with windows of the second layout: the lines of a dropped
            // window block all have twins in the blocks that stay)
            _ if !second_layout && !line.iter().all(|c| *c == b' ') && b.contains(&line.as_slice()) => {
                *d.doubled.entry(w.clone()).or_default() += 1;
                if let Some(c) = d.removed.get_mut(&w) {
                    *c -= 1;
                    if *c == 0 {
                        d.removed.remove(&w);
                    }
                }
            }
            _ => kept.push(line),
        }
    }
    d.removed_lines = kept;
    d
}

/// The pretty debug print of a structure with every list or option of plain values (a
/// vector, a byte string) drawn together on one line, so that a string that is there in one
/// structure and absent in the other still takes one line in both.
fn flat_debug(s: &Setup) -> Vec<String> {
    let text = format!("{s:#?}");
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let open = line.trim_end();
        if open.ends_with('[') || open.ends_with("Some(") {
            // find the matching close at the same indent; fold if nothing inside names a field
            let indent = line.len() - line.trim_start().len();
            let close = (i + 1..lines.len()).find(|&k| lines[k].len() - lines[k].trim_start().len() == indent);
            if let Some(k) = close {
                if lines[i + 1..k].iter().all(|l| !l.contains(": ") && !l.trim_end().ends_with('{')) {
                    let mut joined = line.to_string();
                    for l in &lines[i + 1..=k] {
                        joined.push_str(l.trim());
                    }
                    out.push(joined);
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(line.to_string());
        i += 1;
    }
    out
}

/// The fields in which two structures differ: lines of their debug prints.
pub fn structure_differences(a: &Setup, b: &Setup) -> Vec<String> {
    let (x, y) = (flat_debug(a), flat_debug(b));
    let (x, y): (Vec<&str>, Vec<&str>) = (x.iter().map(String::as_str).collect(), y.iter().map(String::as_str).collect());
    if x.len() != y.len() {
        return vec!["(the two structures have lists of different lengths)".to_string()];
    }
    // name each differing line by the nearest enclosing field names
    let mut out = Vec::new();
    let mut path: Vec<(usize, String)> = Vec::new();
    for (p, q) in x.iter().zip(&y) {
        let depth = p.len() - p.trim_start().len();
        while path.last().is_some_and(|(d, _)| *d >= depth) {
            path.pop();
        }
        let name = p.trim().split(':').next().unwrap_or("").to_string();
        path.push((depth, name));
        if p != q {
            let names: Vec<&str> = path.iter().map(|(_, n)| n.as_str()).filter(|n| !n.is_empty() && !n.ends_with('{') && !n.ends_with('[') && !n.ends_with('(')).collect();
            out.push(format!("{}: {} -> {}", names.join("."), p.trim().trim_end_matches(','), q.trim().trim_end_matches(',')));
        }
    }
    out
}

/// Runs the three checks on one file, under `session`.
pub fn check_file(data: &[u8], session: &Session) -> FileReport {
    let version = {
        let end = data.iter().position(|&b| b == b'\n').unwrap_or(data.len());
        String::from_utf8_lossy(&data[4.min(end)..end]).trim().to_string()
    };
    let mut setup = Setup::default();
    let (info, error) = match read_into(data, &mut setup, session) {
        Ok(info) => (info, None),
        Err(e) => (ReadInfo::default(), Some(e)),
    };
    let normal_end = (info.ended_on_eof_word && info.reads_past_end == 0) || (!info.ended_on_eof_word && info.reads_past_end == 2);
    let complete = error.is_none()
        && info.all_tokens_consumed
        && info.unknown_words.is_empty()
        && info.messages.is_empty()
        && info.deviations.is_empty()
        && normal_end;
    let mut report = FileReport {
        version,
        error,
        info,
        complete,
        structure_round_trip: false,
        structure_differences: Vec::new(),
        second_write_identical: false,
        second_round_trip: false,
        second_differences: Vec::new(),
        third_write_identical: false,
        indent: 0,
        identical: false,
        diff: Diff::default(),
    };
    if report.error.is_some() {
        return report;
    }
    // the indent level the file was written with shows in its first indented line
    let candidates: Vec<(i32, Vec<u8>, Diff)> = [0, 1]
        .into_iter()
        .map(|indent| {
            let out = write_with(&setup, &WriteOptions { indent, ..WriteOptions::default() }, session);
            let d = diff(data, &out);
            (indent, out, d)
        })
        .collect();
    let weight = |d: &Diff| d.removed.values().sum::<usize>() + d.changed.values().sum::<usize>() + d.added.values().sum::<usize>();
    let (indent, out, d) = candidates.into_iter().min_by_key(|(_, _, d)| weight(d)).unwrap();
    report.indent = indent;
    report.identical = out == data;
    report.diff = d;
    // read the rewrite over a fresh start-up state, write, and again
    let same = |a: &Setup, b: &Setup| a == b || format!("{a:?}") == format!("{b:?}");
    let options = WriteOptions { indent: 1, ..WriteOptions::default() };
    let out1 = write_with(&setup, &options, session);
    let mut second = Setup::default();
    if let Err(e) = read_into(&out1, &mut second, session) {
        report.structure_differences = vec![format!("the rewrite does not read: {e}")];
        return report;
    }
    report.structure_round_trip = same(&setup, &second);
    if !report.structure_round_trip {
        report.structure_differences = structure_differences(&setup, &second);
    }
    let out2 = write_with(&second, &options, session);
    report.second_write_identical = out1 == out2;
    let mut third = Setup::default();
    if let Err(e) = read_into(&out2, &mut third, session) {
        report.second_differences = vec![format!("the second rewrite does not read: {e}")];
        return report;
    }
    report.second_round_trip = same(&second, &third);
    if !report.second_round_trip {
        report.second_differences = structure_differences(&second, &third);
    }
    let out3 = write_with(&third, &options, session);
    let mut fourth = Setup::default();
    report.third_write_identical = read_into(&out3, &mut fourth, session).is_ok() && write_with(&fourth, &options, session) == out3 && same(&third, &fourth);
    report
}

/// Counts over many files.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub files: usize,
    pub by_version: BTreeMap<String, usize>,
    pub errors: Vec<(String, String)>,
    pub complete: usize,
    pub incomplete: Vec<(String, String)>,
    /// Files after which only blanks are left; files in which no word was skipped as unknown;
    /// files that drew a status-bar message.
    pub all_tokens_consumed: usize,
    pub no_unknown_words: usize,
    pub with_messages: usize,
    pub ended_on_eof_word: usize,
    pub ended_with_file: usize,
    pub structure_round_trip: usize,
    pub structure_failures: BTreeMap<String, usize>,
    /// Files per combination of differing fields.
    pub structure_failure_sets: BTreeMap<String, usize>,
    pub second_write_identical: usize,
    pub second_round_trip: usize,
    pub second_failures: BTreeMap<String, usize>,
    pub third_write_identical: usize,
    pub layout_only: usize,
    pub moved: BTreeMap<String, usize>,
    pub doubled: BTreeMap<String, usize>,
    /// Files per cause, and per combination of causes.
    pub causes: BTreeMap<&'static str, usize>,
    pub cause_sets: BTreeMap<String, usize>,
    pub indent: [usize; 2],
    pub identical: usize,
    pub header_only: usize,
    pub additions_only: usize,
    pub other: usize,
    /// For each cause, in how many files it occurs.
    pub added: BTreeMap<String, usize>,
    pub removed: BTreeMap<String, usize>,
    pub changed: BTreeMap<(String, Change), usize>,
}

impl Summary {
    pub fn add(&mut self, name: &str, r: &FileReport) {
        self.files += 1;
        *self.by_version.entry(r.version.clone()).or_default() += 1;
        if let Some(e) = &r.error {
            self.errors.push((name.to_string(), e.to_string()));
            return;
        }
        self.all_tokens_consumed += r.info.all_tokens_consumed as usize;
        self.no_unknown_words += r.info.unknown_words.is_empty() as usize;
        self.with_messages += !r.info.messages.is_empty() as usize;
        if r.complete {
            self.complete += 1;
        } else {
            let i = &r.info;
            let mut why = Vec::new();
            if !i.all_tokens_consumed {
                why.push(format!("stopped at byte {}", i.end));
            }
            if !i.unknown_words.is_empty() {
                why.push(format!("{} unknown words, first {:?}", i.unknown_words.len(), String::from_utf8_lossy(&i.unknown_words[0])));
            }
            if !i.messages.is_empty() {
                why.push(format!("{} messages, first at {} {:?}", i.messages.len(), i.messages[0].keyword, String::from_utf8_lossy(&i.messages[0].token)));
            }
            if !i.deviations.is_empty() {
                why.push(format!("deviation: {}", i.deviations[0]));
            }
            why.push(format!("eof word {}, reads past end {}", i.ended_on_eof_word, i.reads_past_end));
            self.incomplete.push((name.to_string(), why.join("; ")));
        }
        if r.info.ended_on_eof_word {
            self.ended_on_eof_word += 1;
        } else {
            self.ended_with_file += 1;
        }
        if r.structure_round_trip {
            self.structure_round_trip += 1;
        } else {
            let mut seen = std::collections::BTreeSet::new();
            for d in &r.structure_differences {
                // one count per file and field
                let field = d.split(": ").next().unwrap_or(d).to_string();
                if seen.insert(field.clone()) {
                    *self.structure_failures.entry(field).or_default() += 1;
                }
            }
            let set: Vec<String> = seen.iter().map(|f| f.rsplit('.').next().unwrap_or(f).to_string()).collect();
            *self.structure_failure_sets.entry(set.join(" + ")).or_default() += 1;
        }
        if r.second_write_identical {
            self.second_write_identical += 1;
        }
        if r.second_round_trip {
            self.second_round_trip += 1;
        } else {
            let mut seen = std::collections::BTreeSet::new();
            for d in &r.second_differences {
                let field = d.split(": ").next().unwrap_or(d).to_string();
                if seen.insert(field.clone()) {
                    *self.second_failures.entry(field).or_default() += 1;
                }
            }
        }
        if r.third_write_identical {
            self.third_write_identical += 1;
        }
        for k in r.diff.moved.keys() {
            *self.moved.entry(k.clone()).or_default() += 1;
        }
        for k in r.diff.doubled.keys() {
            *self.doubled.entry(k.clone()).or_default() += 1;
        }
        let c = causes(&r.diff, r.complete);
        for k in &c {
            *self.causes.entry(k).or_default() += 1;
        }
        let letters: String = c.iter().map(|k| &k[..1]).collect();
        *self.cause_sets.entry(letters).or_default() += 1;
        self.indent[r.indent as usize] += 1;
        if r.identical {
            self.identical += 1;
        } else if r.diff.header_only() {
            self.header_only += 1;
        } else if r.diff.additions_only() {
            self.additions_only += 1;
        } else if r.diff.layout_only() {
            self.layout_only += 1;
        } else {
            self.other += 1;
        }
        for k in r.diff.added.keys() {
            *self.added.entry(k.clone()).or_default() += 1;
        }
        for k in r.diff.removed.keys() {
            *self.removed.entry(k.clone()).or_default() += 1;
        }
        for k in r.diff.changed.keys() {
            *self.changed.entry(k.clone()).or_default() += 1;
        }
    }

    /// The report as text.
    pub fn report(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(s, "setup files (tag SSET): {}", self.files);
        let _ = writeln!(s, "  versions: {}", self.by_version.iter().map(|(v, n)| format!("{v} x{n}")).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "reader");
        let _ = writeln!(s, "  gave up (the original would crash or hang): {}", self.errors.len());
        for (name, e) in &self.errors {
            let _ = writeln!(s, "    {name}: {e}");
        }
        let _ = writeln!(s, "  every token consumed (only blanks left): {}", self.all_tokens_consumed);
        let _ = writeln!(s, "  no word skipped as unknown: {}; files that drew a status-bar message: {}", self.no_unknown_words, self.with_messages);
        let _ = writeln!(s, "  read completely (all tokens consumed, nothing unexplained): {}", self.complete);
        let _ = writeln!(s, "    left through the word EOF, no token asked for past the end: {}", self.ended_on_eof_word);
        let _ = writeln!(s, "    no EOF word; the file's end stops the last section and then the main loop: {}", self.ended_with_file);
        let _ = writeln!(s, "  not completely: {}", self.incomplete.len());
        for (name, why) in &self.incomplete {
            let _ = writeln!(s, "    {name}: {why}");
        }
        let _ = writeln!(s, "round trip of the structure (read, write, read)");
        let _ = writeln!(s, "  same structure: {}", self.structure_round_trip);
        let _ = writeln!(s, "  different: {}; fields, with the number of files:", self.files - self.errors.len() - self.structure_round_trip);
        for (field, n) in &self.structure_failures {
            let _ = writeln!(s, "    {n:4}  {field}");
        }
        let _ = writeln!(s, "  combinations of differing fields (files):");
        for (set, n) in &self.structure_failure_sets {
            let _ = writeln!(s, "    {n:4}  {set}");
        }
        let _ = writeln!(s, "  second write equals the first: {}", self.second_write_identical);
        let _ = writeln!(s, "one generation on (read the rewrite, write, read)");
        let _ = writeln!(s, "  same structure: {}", self.second_round_trip);
        let _ = writeln!(s, "  different: {}; fields, with the number of files:", self.files - self.errors.len() - self.second_round_trip);
        for (field, n) in &self.second_failures {
            let _ = writeln!(s, "    {n:4}  {field}");
        }
        let _ = writeln!(s, "  from the third write on, file and structure no longer change: {}", self.third_write_identical);
        let _ = writeln!(s, "rewrite against the file, byte for byte");
        let _ = writeln!(s, "  indent level the file was written at: 0 in {} files, 1 in {}", self.indent[0], self.indent[1]);
        let _ = writeln!(s, "  identical: {}", self.identical);
        let _ = writeln!(s, "  header line only: {}", self.header_only);
        let _ = writeln!(s, "  header and lines this build adds, every original line reproduced: {}", self.additions_only);
        let _ = writeln!(s, "  those, and lines an older build laid out differently (same tokens): {}", self.layout_only);
        let _ = writeln!(s, "  other differences as well: {}", self.other);
        let _ = writeln!(s, "  causes (files in which each occurs):");
        for (k, n) in &self.causes {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        let _ = writeln!(s, "  combinations of causes (files):");
        for (k, n) in &self.cause_sets {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        let _ = writeln!(s, "  lines both have, at different places (files):");
        for (k, n) in &self.moved {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        let _ = writeln!(s, "  lines the file has twice and the rewrite once (files):");
        for (k, n) in &self.doubled {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        let _ = writeln!(s, "  lines of the file not reproduced as they are, by first word (files):");
        for ((k, kind), n) in &self.changed {
            let _ = writeln!(s, "    {n:4}  {k}  [{kind:?}]");
        }
        let _ = writeln!(s, "  lines of the file with no counterpart in the rewrite (files):");
        for (k, n) in &self.removed {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        let _ = writeln!(s, "  lines only the rewrite has (files):");
        for (k, n) in &self.added {
            let _ = writeln!(s, "    {n:4}  {k}");
        }
        s
    }
}
