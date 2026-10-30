// SPDX-License-Identifier: MIT
//! The port of the raw function-curve text routines (`si_ascii::rawfcv`) against the
//! original, run one routine at a time in the emulator (`re/tools/si-ascii/gen_rawfcv.py`,
//! finding 0006 section 10).
//!
//!   ascii-emucheck <directory of .vec files>
//!   EMUCHECK_MUTATE=1      a bit of every result the port gives is flipped: every case must differ
//!   EMUCHECK_MUTATE=calls  a bit of the first argument of every call the port makes: every case
//!                          with calls must differ
//!
//! A case is placed in a fresh image (`si_mem::load_case`: the buffers in the order given,
//! pointers between them resolved). The writer's text is compared with what the harness's
//! `fprintf` received, the reader's buffers word by word (a pointer as the buffer it points
//! into), with the file's position and end-of-file flag, and the calls the reader makes
//! outside `asceneio.c` with the calls the original made, argument by argument; those calls
//! are answered as the vector says the stand-ins answered.
use si_ascii::chn::scan::CFile;
use si_ascii::rawfcv::{self, RawCalls};
use si_mem::vecfmt::{parse_words, Names, Word};
use si_mem::Mem;
use std::collections::BTreeMap;

/// One case of a vector file.
#[derive(Default)]
struct Case {
    name: String,
    inputs: Vec<(String, Field)>,
    outputs: Vec<(String, Field)>,
    calls: Vec<String>,
    notes: Vec<String>,
}

enum Field {
    Words(Option<Vec<Word>>),
    Text(Vec<u8>),
}

impl Case {
    fn input_words(&self, name: &str) -> Option<&[Word]> {
        self.inputs.iter().find(|(n, _)| n == name).and_then(|(_, f)| match f {
            Field::Words(w) => w.as_deref(),
            Field::Text(_) => None,
        })
    }
    fn text(&self, out: bool, name: &str) -> Option<&[u8]> {
        let list = if out { &self.outputs } else { &self.inputs };
        list.iter().find(|(n, _)| n == name).and_then(|(_, f)| match f {
            Field::Text(t) => Some(&t[..]),
            Field::Words(_) => None,
        })
    }
    fn output_words(&self, name: &str) -> Option<&[Word]> {
        self.outputs.iter().find(|(n, _)| n == name).and_then(|(_, f)| match f {
            Field::Words(w) => w.as_deref(),
            Field::Text(_) => None,
        })
    }
    /// The buffers of the case: every input that is neither an argument nor the file.
    fn buffers(&self) -> Vec<(&str, Option<&[Word]>)> {
        self.inputs
            .iter()
            .filter(|(n, _)| !n.starts_with("p_") && !n.starts_with("v_"))
            .filter_map(|(n, f)| match f {
                Field::Words(w) => Some((n.as_str(), w.as_deref())),
                Field::Text(_) => None,
            })
            .collect()
    }
}

fn tokens(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), None::<usize>, false);
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'"' && (i == 0 || b[i - 1] != b'\\') {
            quoted = !quoted;
        }
        if c == b' ' && !quoted {
            if let Some(s) = start.take() {
                out.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

fn unescape(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&text[i + 2..i + 4], 16) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn parse(line: &str) -> Result<Option<Case>, String> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut c = Case::default();
    let mut part = 0;
    for (k, t) in tokens(line).into_iter().enumerate() {
        if k == 0 {
            c.name = t.to_string();
            continue;
        }
        match (part, t) {
            (_, "=>") => part = 1,
            (_, "!") => part = 2,
            (2, note) => c.notes.push(note.to_string()),
            (_, call) if call.starts_with('@') => c.calls.push(call.to_string()),
            (p, field) => {
                let (name, value) = field.split_once('=').ok_or_else(|| format!("no `=` in `{field}`"))?;
                let f = if let Some(q) = value.strip_prefix('"') {
                    Field::Text(unescape(q.strip_suffix('"').unwrap_or(q)))
                } else if name.contains('.') {
                    // the file's state (f.pos, f.state): plain hexadecimal
                    Field::Words(parse_words(value).ok().flatten())
                } else {
                    Field::Words(parse_words(value)?)
                };
                if p == 0 {
                    c.inputs.push((name.to_string(), f));
                } else {
                    c.outputs.push((name.to_string(), f));
                }
            }
        }
    }
    Ok(Some(c))
}

fn word_value(m: &Mem, names: &Names, w: Option<&[Word]>) -> u32 {
    match w.and_then(|w| w.first()) {
        Some(Word::Value { value, .. }) => *value as u32,
        Some(Word::Ref { name, offset }) => names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
        _ => 0,
    }
}

/// A word as the harness logs a call's argument or result: a buffer of the case by name.
fn logged(m: &Mem, names: &Names, v: u32) -> String {
    match names.name_of(m, v) {
        Some((n, 0)) => format!("&{n}"),
        Some((n, off)) => format!("&{n}+{off:#x}"),
        None => format!("{v:08x}"),
    }
}

/// The stand-ins: every call is logged as the harness logs it; the two that answer something
/// answer what the original's stand-ins answered, call by call.
struct Calls<'n> {
    names: &'n Names,
    answers: BTreeMap<String, Vec<String>>,
    log: Vec<String>,
}

impl Calls<'_> {
    fn answer(&mut self, m: &Mem, name: &str) -> u32 {
        let list = self.answers.entry(name.to_string()).or_default();
        if list.is_empty() {
            return 0;
        }
        let a = list.remove(0);
        if let Some(r) = a.strip_prefix('&') {
            let (n, off) = r.split_once('+').map_or((r, 0), |(n, o)| (n, u32::from_str_radix(o.trim_start_matches("0x"), 16).unwrap_or(0)));
            self.names.addr(m, n).unwrap_or(0).wrapping_add(off)
        } else {
            u32::from_str_radix(&a, 16).unwrap_or(0)
        }
    }
}

impl RawCalls for Calls<'_> {
    fn fcurve_remove(&mut self, m: &mut Mem, slot: u32) {
        self.log.push(format!("@SI_fcurveRemove({})=00000000", logged(m, self.names, slot)));
    }
    fn fcurve_allocate(&mut self, m: &mut Mem, kind: u32, elem: u32) -> u32 {
        let r = self.answer(m, "SI_fcurveAllocate");
        self.log.push(format!("@SI_fcurveAllocate({},{})={}", logged(m, self.names, kind), logged(m, self.names, elem), logged(m, self.names, r)));
        r
    }
    fn malloc(&mut self, m: &mut Mem, size: u32) -> u32 {
        let r = self.answer(m, "SI_malloc");
        self.log.push(format!("@SI_malloc({size:08x})={}", logged(m, self.names, r)));
        r
    }
    fn fkey_initialize(&mut self, m: &mut Mem, key: u32) {
        self.log.push(format!("@SI_fkeyInitialize({})=00000000", logged(m, self.names, key)));
    }
    fn status_message(&mut self, m: &mut Mem, text: u32, level: i16) {
        self.log.push(format!("@SI_statusDisplayMessage({},{:08x})=00000000", logged(m, self.names, text), level as u16));
    }
}

/// The answers the original's stand-ins gave, per routine, in order.
fn recorded_answers(c: &Case) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for call in &c.calls {
        let Some((head, result)) = call[1..].rsplit_once(")=") else { continue };
        let name = head.split('(').next().unwrap_or("");
        out.entry(name.to_string()).or_default().push(result.to_string());
    }
    out
}

#[derive(Default)]
struct Tally {
    cases: usize,
    same: usize,
    differ: usize,
    skipped: usize,
    examples: Vec<String>,
}

fn check_write(c: &Case, mutate: &str) -> Result<(), String> {
    let mut m = Mem::new();
    let names = si_mem::load_case(&mut m, &c.buffers())?;
    let fc = word_value(&m, &names, c.input_words("p_fc"));
    let mut out = Vec::new();
    let mut ret = rawfcv::rawfcurve_ascii_write(&m, &mut out, fc);
    if mutate == "1" {
        ret ^= 1;
    }
    let want_ret = word_value(&m, &names, c.output_words("eax"));
    if ret as u32 != want_ret {
        return Err(format!("eax: original {want_ret:08x}, port {ret:08x}"));
    }
    let want = c.text(true, "f").unwrap_or(&[]);
    if out != want {
        let at = out.iter().zip(want).position(|(a, b)| a != b).unwrap_or(out.len().min(want.len()));
        return Err(format!("text differs at byte {at}: original {:?}, port {:?}", String::from_utf8_lossy(&want[at..want.len().min(at + 40)]), String::from_utf8_lossy(&out[at..out.len().min(at + 40)])));
    }
    Ok(())
}

fn check_read(c: &Case, mutate: &str) -> Result<(), String> {
    let mut m = Mem::new();
    let names = si_mem::load_case(&mut m, &c.buffers())?;
    let slot = word_value(&m, &names, c.input_words("p_slot"));
    let elem = word_value(&m, &names, c.input_words("v_elem"));
    let text = c.text(false, "f").unwrap_or(&[]).to_vec();
    let mut f = CFile::new(&text);
    let mut calls = Calls { names: &names, answers: recorded_answers(c), log: Vec::new() };
    let mut ret = rawfcv::rawfcurve_ascii_read(&mut m, &mut f, slot, elem, &mut calls).map_err(|e| format!("port: {e}"))?;
    if mutate == "1" {
        ret ^= 1;
    }
    if mutate == "calls" {
        if let Some(first) = calls.log.first_mut() {
            // a bit of the first argument: the last hexadecimal digit, or the name's offset
            let pos = first.find(|ch: char| ch == ',' || ch == ')').unwrap_or(first.len()).saturating_sub(1);
            let b = first.as_bytes()[pos];
            let flipped = (b ^ 1) as char;
            first.replace_range(pos..pos + 1, &flipped.to_string());
        }
    }
    let want_ret = word_value(&m, &names, c.output_words("eax"));
    if ret as u32 != want_ret {
        return Err(format!("eax: original {want_ret:08x}, port {ret:08x}"));
    }
    for (name, field) in &c.outputs {
        let Field::Words(Some(w)) = field else { continue };
        match name.as_str() {
            "eax" => {}
            "f.pos" => {
                let want = word_value(&m, &names, Some(w));
                if f.tell() as u32 != want {
                    return Err(format!("file position: original {want:#x}, port {:#x}", f.tell()));
                }
            }
            "f.state" => {
                let want = word_value(&m, &names, Some(w));
                if (f.feof() as u32) != want {
                    return Err(format!("end-of-file flag: original {want}, port {}", f.feof() as u32));
                }
            }
            buf => {
                let a = names.addr(&m, buf).ok_or_else(|| format!("no buffer `{buf}`"))?;
                m.compare_words(a, w, &names).map_err(|e| format!("{buf}: {e}"))?;
            }
        }
    }
    if calls.log != c.calls {
        let i = calls.log.iter().zip(&c.calls).position(|(a, b)| a != b).unwrap_or(calls.log.len().min(c.calls.len()));
        return Err(format!(
            "calls differ at {i}: original {:?}, port {:?}",
            c.calls.get(i).map(String::as_str).unwrap_or("(none)"),
            calls.log.get(i).map(String::as_str).unwrap_or("(none)")
        ));
    }
    Ok(())
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: ascii-emucheck <directory of .vec files>");
        std::process::exit(2);
    });
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("vector directory").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vec")).collect();
    files.sort();
    let mut clean = true;
    let mut with_calls_differ = (0usize, 0usize);
    for p in files {
        let mut t = Tally::default();
        let mut routine = String::new();
        for line in std::fs::read_to_string(&p).expect("vector file").lines() {
            let c = match parse(line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("{}: {e}", p.display());
                    std::process::exit(2);
                }
            };
            routine = c.name.split("__").next().unwrap_or("").to_string();
            t.cases += 1;
            // a stop, memory nobody initialised, or a C library corner the harness does not
            // vouch for (NA:…): not compared
            if c.notes.iter().any(|n| n.starts_with("STOP") || n.starts_with("UNINIT") || n.starts_with("NA:")) {
                t.skipped += 1;
                continue;
            }
            let r = match routine.as_str() {
                "rawfcurveAsciiWrite" => check_write(&c, &mutate),
                "rawfcurveAsciiRead" => check_read(&c, &mutate),
                _ => Err(format!("no port for {routine}")),
            };
            match r {
                Ok(()) => t.same += 1,
                Err(e) => {
                    t.differ += 1;
                    if !c.calls.is_empty() {
                        with_calls_differ.1 += 1;
                    }
                    if t.examples.len() < 8 {
                        let text = c.text(false, "f").unwrap_or(&[]);
                        let head: Vec<u8> = text.iter().copied().take(60).collect();
                        t.examples.push(format!("{e}  [text {:?}]", String::from_utf8_lossy(&head)));
                    }
                }
            }
            if !c.calls.is_empty() {
                with_calls_differ.0 += 1;
            }
        }
        println!("{routine:24} {:5} cases {:5} identical {:5} different {:5} not compared", t.cases, t.same, t.differ, t.skipped);
        for e in &t.examples {
            println!("    {e}");
        }
        clean &= t.differ == 0;
    }
    if !mutate.is_empty() {
        println!("EMUCHECK_MUTATE={mutate}: cases with calls {}, of which different {}", with_calls_differ.0, with_calls_differ.1);
    }
    std::process::exit(if clean { 0 } else { 1 });
}
