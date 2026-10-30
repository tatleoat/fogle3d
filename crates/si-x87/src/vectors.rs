// SPDX-License-Identifier: MIT
//! Reader for the test vectors of the emulation harness (`re/tools/emu`, finding 0011).
//!
//! A vector file is text, one case per line: what was handed to one routine of the original
//! and what it left behind when its instructions were stepped through in a CPU emulator.
//! Vectors are derived from the binary and are never committed; a check binary takes the
//! directory that holds them.
//!
//! ```text
//! # comment
//! SI_vectorDistance a=3f800000,40000000,40400000 b=00000000,00000000,00000000 => st0=4000ef7750f1f5aa1c9d
//! name[#variant] input fields  =>  output fields  @recorded(calls)=result  ! notes
//! ```
//!
//! - A field is `name=word,word,…`. A word is hexadecimal and as wide as it is long: two
//!   digits a byte, four a 16-bit value, eight a 32-bit value (a `float` is its bit pattern),
//!   sixteen a 64-bit value, twenty an 80-bit register value. Words are numbers, not memory
//!   order: the float 1.0 is `3f800000`.
//! - `name="text"` is a field of bytes; `\xNN` stands for a byte outside the printable set
//!   (blanks included). `name=null` is a null pointer; `name=` an empty buffer.
//! - `&buffer+0x10` is a pointer into another field's buffer; `&heap3+0x8` into the fourth
//!   block the routine got from `malloc`.
//! - Dashes (`--------`) are an output word the routine did not write.
//! - `@sin(3fe0000000000000)=3ffe…` is a call the routine made that the harness answered and
//!   recorded: the C library's mathematics, or anything the run was told to record.
//! - Notes after `!`: `STOP:…` the case did not run to the routine's return (budget, left the
//!   sandbox, licence code); `NA:…` it ran, but on something the emulator cannot vouch for
//!   (`NA:libm:sin`, `NA:x87:fpatan`); `UNINIT:field` the output depends on memory the
//!   routine never initialised. A case with any of these is not authoritative. Other notes
//!   (`GW:` a global was written, `MOD:` an input buffer was changed, `OVERRUN:`) inform.
//!
//! No part of this module knows the original; it only parses.

use crate::x87::F80;
use std::fmt;
use std::path::Path;

/// One word of a field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Word {
    /// A value of `bytes` bytes.
    Value { bytes: u8, value: u128 },
    /// A word of `bytes` bytes the routine left unwritten.
    Unwritten { bytes: u8 },
    /// A pointer `offset` bytes into the buffer of the field (or heap block) `name`.
    Ref { name: String, offset: u32 },
}

impl Word {
    /// The value of a written, non-pointer word.
    pub fn value(&self) -> Option<u128> {
        match self {
            Word::Value { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// The width in bytes; a pointer is four.
    pub fn bytes(&self) -> u8 {
        match self {
            Word::Value { bytes, .. } | Word::Unwritten { bytes } => *bytes,
            Word::Ref { .. } => 4,
        }
    }

    fn parse(text: &str) -> Result<Word, String> {
        if let Some(r) = text.strip_prefix('&') {
            let (name, off) = match r.split_once('+') {
                Some((n, o)) => (n, parse_int(o)?),
                None => (r, 0),
            };
            return Ok(Word::Ref { name: name.to_string(), offset: off });
        }
        if text.is_empty() || text.len() % 2 != 0 || text.len() > 32 {
            return Err(format!("bad word `{text}`"));
        }
        let bytes = (text.len() / 2) as u8;
        if text.bytes().all(|c| c == b'-') {
            return Ok(Word::Unwritten { bytes });
        }
        let value = u128::from_str_radix(text, 16).map_err(|_| format!("bad word `{text}`"))?;
        Ok(Word::Value { bytes, value })
    }
}

fn parse_int(text: &str) -> Result<u32, String> {
    let r = match text.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => text.parse(),
    };
    r.map_err(|_| format!("bad offset `{text}`"))
}

/// A named buffer or value of a case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// `None` for `name=null`.
    pub words: Option<Vec<Word>>,
}

impl Field {
    fn parse(token: &str) -> Result<Field, String> {
        let (name, text) = token.split_once('=').ok_or_else(|| format!("no `=` in `{token}`"))?;
        let words = if text == "null" {
            None
        } else if let Some(q) = text.strip_prefix('"') {
            let q = q.strip_suffix('"').ok_or_else(|| format!("unterminated text in `{token}`"))?;
            let b = q.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
                    let v = u8::from_str_radix(&q[i + 2..i + 4], 16).map_err(|_| format!("bad escape in `{token}`"))?;
                    out.push(Word::Value { bytes: 1, value: v as u128 });
                    i += 4;
                } else {
                    out.push(Word::Value { bytes: 1, value: b[i] as u128 });
                    i += 1;
                }
            }
            Some(out)
        } else if text.is_empty() {
            Some(Vec::new())
        } else {
            Some(text.split(',').map(Word::parse).collect::<Result<Vec<_>, _>>()?)
        };
        Ok(Field { name: name.to_string(), words })
    }

    pub fn is_null(&self) -> bool {
        self.words.is_none()
    }

    /// The words; a null pointer has none.
    pub fn words(&self) -> &[Word] {
        self.words.as_deref().unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.words().len()
    }

    pub fn is_empty(&self) -> bool {
        self.words().is_empty()
    }

    /// Every word as a number. Panics on an unwritten word or a pointer written as `&name`:
    /// use [`Field::written`] or match on [`Field::words`] where those can occur.
    pub fn values(&self) -> Vec<u128> {
        self.words()
            .iter()
            .map(|w| w.value().unwrap_or_else(|| panic!("field `{}` has an unwritten word or a pointer", self.name)))
            .collect()
    }

    /// Each word's value, `None` where the routine wrote nothing or the word is a pointer.
    pub fn written(&self) -> Vec<Option<u128>> {
        self.words().iter().map(Word::value).collect()
    }

    pub fn u8s(&self) -> Vec<u8> {
        self.values().into_iter().map(|v| v as u8).collect()
    }

    pub fn u16s(&self) -> Vec<u16> {
        self.values().into_iter().map(|v| v as u16).collect()
    }

    pub fn i16s(&self) -> Vec<i16> {
        self.values().into_iter().map(|v| v as u16 as i16).collect()
    }

    pub fn u32s(&self) -> Vec<u32> {
        self.values().into_iter().map(|v| v as u32).collect()
    }

    pub fn i32s(&self) -> Vec<i32> {
        self.values().into_iter().map(|v| v as u32 as i32).collect()
    }

    pub fn u64s(&self) -> Vec<u64> {
        self.values().into_iter().map(|v| v as u64).collect()
    }

    pub fn f32s(&self) -> Vec<f32> {
        self.values().into_iter().map(|v| f32::from_bits(v as u32)).collect()
    }

    pub fn f64s(&self) -> Vec<f64> {
        self.values().into_iter().map(|v| f64::from_bits(v as u64)).collect()
    }

    /// The single word of a one-word field.
    pub fn u32(&self) -> u32 {
        self.one() as u32
    }

    pub fn i32(&self) -> i32 {
        self.one() as u32 as i32
    }

    pub fn i16(&self) -> i16 {
        self.one() as u16 as i16
    }

    pub fn f32(&self) -> f32 {
        f32::from_bits(self.one() as u32)
    }

    pub fn f64(&self) -> f64 {
        f64::from_bits(self.one() as u64)
    }

    /// A twenty-digit word as the register value it is.
    pub fn f80(&self) -> F80 {
        let v = self.one();
        F80::from_bits(v as u64, (v >> 64) as u16)
    }

    fn one(&self) -> u128 {
        let v = self.values();
        assert!(v.len() == 1, "field `{}` has {} words, one expected", self.name, v.len());
        v[0]
    }

    /// The bytes the words occupy in the emulated (little-endian) memory. Unwritten words
    /// and pointers come out as zeros.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for w in self.words() {
            let v = w.value().unwrap_or(0);
            out.extend_from_slice(&v.to_le_bytes()[..w.bytes() as usize]);
        }
        out
    }

    /// True when `got` has the bit pattern of every word the routine wrote; words it left
    /// unwritten are not compared.
    pub fn same_f32(&self, got: &[f32]) -> bool {
        let w = self.written();
        w.len() == got.len() && w.iter().zip(got).all(|(e, g)| e.map_or(true, |e| e as u32 == g.to_bits()))
    }

    pub fn same_f64(&self, got: &[f64]) -> bool {
        let w = self.written();
        w.len() == got.len() && w.iter().zip(got).all(|(e, g)| e.map_or(true, |e| e as u64 == g.to_bits()))
    }

    pub fn same_u32(&self, got: &[u32]) -> bool {
        let w = self.written();
        w.len() == got.len() && w.iter().zip(got).all(|(e, g)| e.map_or(true, |e| e as u32 == *g))
    }

    /// True when the field is one register value with exactly these 80 bits.
    pub fn same_f80(&self, got: F80) -> bool {
        self.len() == 1 && self.f80().to_bits() == got.to_bits()
    }
}

/// A call the routine made that the harness answered: `@name(arguments)=result`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub args: Vec<Word>,
    pub result: Word,
}

impl Call {
    /// The result as a register value (the mathematics functions answer in st0).
    pub fn result_f80(&self) -> F80 {
        let v = self.result.value().unwrap_or(0);
        F80::from_bits(v as u64, (v >> 64) as u16)
    }

    pub fn arg_values(&self) -> Vec<u128> {
        self.args.iter().map(|w| w.value().unwrap_or(0)).collect()
    }
}

/// One line of a vector file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    /// The routine's name, with `#variant` when the spec line had one.
    pub name: String,
    pub inputs: Vec<Field>,
    pub outputs: Vec<Field>,
    pub calls: Vec<Call>,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

impl Case {
    /// Parses one line; `None` for a blank line or a comment.
    pub fn parse(line: &str) -> Result<Option<Case>, String> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return Ok(None);
        }
        let mut tokens = line.split_ascii_whitespace();
        let name = tokens.next().unwrap().to_string();
        let mut case = Case { name, inputs: Vec::new(), outputs: Vec::new(), calls: Vec::new(), notes: Vec::new() };
        let mut part = 0;
        for t in tokens {
            match (part, t) {
                (0, "=>") => part = 1,
                (1, "!") => part = 2,
                (0, _) => case.inputs.push(Field::parse(t)?),
                (1, _) if t.starts_with('@') => {
                    let (head, result) = t[1..].rsplit_once(")=").ok_or_else(|| format!("bad call `{t}`"))?;
                    let (name, args) = head.split_once('(').ok_or_else(|| format!("bad call `{t}`"))?;
                    let args = if args.is_empty() { Vec::new() } else { args.split(',').map(Word::parse).collect::<Result<_, _>>()? };
                    case.calls.push(Call { name: name.to_string(), args, result: Word::parse(result)? });
                }
                (1, _) => case.outputs.push(Field::parse(t)?),
                _ => case.notes.push(t.to_string()),
            }
        }
        if part == 0 {
            return Err("no `=>`".to_string());
        }
        Ok(Some(case))
    }

    pub fn input(&self, name: &str) -> &Field {
        self.inputs.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("case of {} has no input `{name}`", self.name))
    }

    pub fn output(&self, name: &str) -> &Field {
        self.try_output(name).unwrap_or_else(|| panic!("case of {} has no output `{name}`", self.name))
    }

    pub fn try_input(&self, name: &str) -> Option<&Field> {
        self.inputs.iter().find(|f| f.name == name)
    }

    pub fn try_output(&self, name: &str) -> Option<&Field> {
        self.outputs.iter().find(|f| f.name == name)
    }

    /// An output buffer, or, when the routine left an input-only buffer alone, that input.
    pub fn after(&self, name: &str) -> &Field {
        self.try_output(name).unwrap_or_else(|| self.input(name))
    }

    pub fn has_note(&self, prefix: &str) -> bool {
        self.notes.iter().any(|n| n.starts_with(prefix))
    }

    /// The case did not reach the routine's return.
    pub fn stopped(&self) -> bool {
        self.has_note("STOP")
    }

    /// The outputs are what the original computes, with nothing the emulator cannot vouch for.
    pub fn authoritative(&self) -> bool {
        !(self.has_note("STOP") || self.has_note("NA") || self.has_note("UNINIT"))
    }
}

/// Reads a vector file.
pub fn read(path: &Path) -> Result<Vec<Case>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        match Case::parse(line) {
            Ok(Some(c)) => out.push(c),
            Ok(None) => {}
            Err(message) => return Err(Box::new(ParseError { line: i + 1, message })),
        }
    }
    Ok(out)
}

/// Reads every `*.vec` file of a directory, sorted by name: (file stem, cases).
pub fn read_dir(dir: &Path) -> Result<Vec<(String, Vec<Case>)>, Box<dyn std::error::Error>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "vec"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
        out.push((stem, read(&p)?));
    }
    Ok(out)
}

/// Counts for one routine of a check binary's report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Cases in the file.
    pub cases: usize,
    /// Cases whose every compared bit agrees and that are authoritative.
    pub identical: usize,
    /// Cases that agree given the recorded results of calls the emulator cannot vouch for.
    pub identical_given_calls: usize,
    /// Cases that differ.
    pub different: usize,
    /// Cases not compared (stopped, or outside what the port defines), and cases that are
    /// partly undefined in the original and agree in the defined part.
    pub skipped: usize,
    /// The first few differing cases, for the report.
    pub examples: Vec<String>,
}

impl Tally {
    /// Books one case: `same` is the comparison's outcome, `None` when it was not compared.
    pub fn add(&mut self, case: &Case, same: Option<bool>, detail: impl FnOnce() -> String) {
        self.cases += 1;
        match same {
            None => self.skipped += 1,
            Some(true) if case.authoritative() => self.identical += 1,
            // agreement on the defined part of a case whose rest is undefined in the original
            Some(true) if case.has_note("UNINIT") => self.skipped += 1,
            Some(true) => self.identical_given_calls += 1,
            Some(false) => {
                self.different += 1;
                if self.examples.len() < 3 {
                    self.examples.push(detail());
                }
            }
        }
    }

    /// The header that goes with [`Tally::row`].
    pub fn header() -> String {
        format!("{:<44} {:>7} {:>9} {:>9} {:>9} {:>7}", "function", "cases", "identical", "given-lib", "different", "skipped")
    }

    pub fn row(&self, name: &str) -> String {
        format!(
            "{:<44} {:>7} {:>9} {:>9} {:>9} {:>7}",
            name, self.cases, self.identical, self.identical_given_calls, self.different, self.skipped
        )
    }
}

impl Case {
    /// The recorded calls that answered in st0 (the C library's mathematics, `fpatan`), in
    /// order, as a tape for [`crate::trig::replay`].
    pub fn tape(&self) -> Vec<crate::trig::replay::Record> {
        self.calls
            .iter()
            .filter(|c| c.result.bytes() == 10)
            .map(|c| crate::trig::replay::Record { name: c.name.clone(), args: c.arg_values(), result: c.result_f80() })
            .collect()
    }
}

/// What a check says about one case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every compared bit agrees.
    Same,
    /// Something differs; the text says what.
    Different(String),
    /// Not compared: the port does not define this input (say why in the check's comment).
    Skip,
}

impl Verdict {
    /// `Same` when the condition holds, else `Different` with the text.
    pub fn from(ok: bool, detail: impl FnOnce() -> String) -> Verdict {
        if ok {
            Verdict::Same
        } else {
            Verdict::Different(detail())
        }
    }

    /// The first verdict that is not `Same`.
    pub fn and(self, next: impl FnOnce() -> Verdict) -> Verdict {
        match self {
            Verdict::Same => next(),
            other => other,
        }
    }
}

/// Runs the port on one case's inputs and compares with its outputs.
pub type Check = fn(&Case) -> Verdict;

/// Runs a crate's checks over a directory of vector files and returns one tally per file.
///
/// `lookup` maps a case's name (first as written, then without its `#variant`) to the check
/// for it; a file whose routine has no check is reported with every case skipped. A case
/// that stopped, or whose outputs depend on memory the original never initialised, is
/// skipped. The case's recorded mathematics calls are replayed to the port
/// ([`crate::trig::replay`]); a port that makes different calls, or panics, differs.
pub fn check_dir(dir: &Path, lookup: &dyn Fn(&str) -> Option<Check>) -> Result<Vec<(String, Tally)>, Box<dyn std::error::Error>> {
    check_dir_with(dir, lookup, false)
}

/// As [`check_dir`]; with `partly_undefined` the cases marked `UNINIT` are compared too.
///
/// In such a case some output words depend on memory the original never initialised. The
/// harness prints those words as dashes, like unwritten ones, so a check that compares only
/// the words that have a value ([`Field::written`], the `same_…` helpers) still verifies
/// everything the original defines: the state it wrote before it went wrong, for instance.
/// A check that calls [`Field::values`] on such a field panics and is counted as different,
/// which is why this is not the default.
pub fn check_dir_with(
    dir: &Path,
    lookup: &dyn Fn(&str) -> Option<Check>,
    partly_undefined: bool,
) -> Result<Vec<(String, Tally)>, Box<dyn std::error::Error>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = check_files(dir, lookup, partly_undefined);
    std::panic::set_hook(hook);
    result
}

/// The loop of [`check_dir_with`]: one file after another, one line at a time, so that the
/// memory needed does not grow with the number of cases.
fn check_files(dir: &Path, lookup: &dyn Fn(&str) -> Option<Check>, partly_undefined: bool) -> Result<Vec<(String, Tally)>, Box<dyn std::error::Error>> {
    use std::io::BufRead;
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "vec"))
        .collect();
    paths.sort();
    let mut rows = Vec::new();
    for path in paths {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let mut tally = Tally::default();
        for (number, line) in std::io::BufReader::new(std::fs::File::open(&path)?).lines().enumerate() {
            let case = match Case::parse(&line?) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(message) => return Err(Box::new(ParseError { line: number + 1, message: format!("{}: {message}", path.display()) })),
            };
            let case = &case;
            let check = lookup(&case.name).or_else(|| lookup(case.name.split('#').next().unwrap()));
            let Some(check) = check else {
                tally.add(case, None, String::new);
                continue;
            };
            if case.stopped() || (case.has_note("UNINIT") && !partly_undefined) {
                tally.add(case, None, String::new);
                continue;
            }
            crate::trig::replay::start(case.tape());
            let verdict = std::panic::catch_unwind(|| check(case));
            let replayed = crate::trig::replay::finish();
            let verdict = match (verdict, replayed) {
                (Err(p), _) => {
                    let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                    Verdict::Different(format!("the port panicked: {}", text.unwrap_or_default()))
                }
                (Ok(Verdict::Same), Err(e)) => Verdict::Different(e),
                (Ok(v), _) => v,
            };
            match verdict {
                Verdict::Same => tally.add(case, Some(true), String::new),
                Verdict::Skip => tally.add(case, None, String::new),
                Verdict::Different(d) => tally.add(case, Some(false), || format!("{d}\n      {}", line_of(case))),
            }
        }
        rows.push((stem, tally));
    }
    Ok(rows)
}

fn line_of(case: &Case) -> String {
    let show = |f: &Field| {
        let w: Vec<String> = f
            .words()
            .iter()
            .map(|w| match w {
                Word::Value { bytes, value } => format!("{:0w$x}", value, w = 2 * *bytes as usize),
                Word::Unwritten { bytes } => "-".repeat(2 * *bytes as usize),
                Word::Ref { name, offset } => format!("&{name}+{offset:#x}"),
            })
            .collect();
        format!("{}={}", f.name, w.join(","))
    };
    let ins: Vec<String> = case.inputs.iter().map(show).collect();
    let outs: Vec<String> = case.outputs.iter().map(show).collect();
    let mut text = format!("{} {} => {}", case.name, ins.join(" "), outs.join(" "));
    if text.len() > 900 {
        text.truncate(900);
        text.push('…');
    }
    text
}

/// Prints the per-file table and the totals; returns true when nothing differs.
pub fn print_report(rows: &[(String, Tally)]) -> bool {
    println!("{}", Tally::header());
    let mut total = Tally::default();
    for (name, t) in rows {
        println!("{}", t.row(name));
        for e in &t.examples {
            println!("    {e}");
        }
        total.cases += t.cases;
        total.identical += t.identical;
        total.identical_given_calls += t.identical_given_calls;
        total.different += t.different;
        total.skipped += t.skipped;
    }
    println!("{}", total.row(&format!("TOTAL ({} files)", rows.len())));
    total.different == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_line_with_everything() {
        let line = "f#v a=3f800000,40000000 n=0003 p=&a+0x4 s=\"a\\x20b\" z=null e= => eax=00000001 st0=3fff8000000000000000 \
                    out=--------,3f800000 q=&heap0 @sin(3fe0000000000000)=3ffeeb7eb8a0cbd0a000 ! NA:libm:sin GW:x+0x0";
        let c = Case::parse(line).unwrap().unwrap();
        assert_eq!(c.name, "f#v");
        assert_eq!(c.input("a").f32s(), [1.0, 2.0]);
        assert_eq!(c.input("n").i16(), 3);
        assert_eq!(c.input("p").words(), [Word::Ref { name: "a".into(), offset: 4 }]);
        assert_eq!(c.input("s").u8s(), b"a b");
        assert!(c.input("z").is_null());
        assert!(c.input("e").is_empty() && !c.input("e").is_null());
        assert_eq!(c.output("eax").u32(), 1);
        assert_eq!(c.output("st0").f80().to_bits(), F80::ONE.to_bits());
        assert_eq!(c.output("out").written(), [None, Some(0x3f80_0000)]);
        assert!(c.output("out").same_f32(&[123.0, 1.0]));
        assert!(!c.output("out").same_f32(&[123.0, 2.0]));
        assert_eq!(c.output("q").words(), [Word::Ref { name: "heap0".into(), offset: 0 }]);
        assert_eq!(c.calls.len(), 1);
        assert_eq!(c.calls[0].name, "sin");
        assert_eq!(c.calls[0].arg_values(), [0x3fe0_0000_0000_0000]);
        assert_eq!(c.calls[0].result_f80().to_bits(), (0xeb7e_b8a0_cbd0_a000, 0x3ffe));
        assert_eq!(c.notes, ["NA:libm:sin", "GW:x+0x0"]);
        assert!(!c.authoritative() && !c.stopped());
        assert_eq!(c.after("a").f32s(), [1.0, 2.0]);
    }

    #[test]
    fn comments_blanks_and_errors() {
        assert_eq!(Case::parse("# fn x").unwrap(), None);
        assert_eq!(Case::parse("   ").unwrap(), None);
        assert!(Case::parse("f a=1").is_err());
        assert!(Case::parse("f a=123 =>").is_err());
        assert!(Case::parse("f => ! STOP:budget:5").unwrap().unwrap().stopped());
        let c = Case::parse("f =>").unwrap().unwrap();
        assert!(c.authoritative() && c.outputs.is_empty());
    }

    #[test]
    fn bytes_are_little_endian() {
        let c = Case::parse("f a=3f800000,0102 =>").unwrap().unwrap();
        assert_eq!(c.input("a").to_bytes(), [0, 0, 0x80, 0x3f, 2, 1]);
    }

    #[test]
    fn tally_books_by_kind() {
        let ok = Case::parse("f =>").unwrap().unwrap();
        let lib = Case::parse("f => ! NA:libm:sin").unwrap().unwrap();
        let mut t = Tally::default();
        t.add(&ok, Some(true), String::new);
        t.add(&lib, Some(true), String::new);
        t.add(&ok, Some(false), || "x".to_string());
        t.add(&ok, None, String::new);
        assert_eq!((t.cases, t.identical, t.identical_given_calls, t.different, t.skipped), (4, 1, 1, 1, 1));
        assert_eq!(t.examples, ["x"]);
    }
}
