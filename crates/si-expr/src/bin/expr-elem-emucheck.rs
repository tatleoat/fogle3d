// SPDX-License-Identifier: MIT
//! `expr-elem-emucheck <dir>`: two small routines of the image form against the emulated
//! original (finding 0023, owed list): `SI_modelSetHiddenParam` (`edparams.c`) and
//! `SI_generalAttrGetValue` (`attrib.c`), from `re/tools/si-expr/gen_attr.py`, whose
//! recorded stand-ins are `SI_edFindSon`, `SI_edFindNextSon`, `SI_metaballTouch` and
//! `SI_mapWireToIndexColour`.
//!
//! Each line of a vector file (made with the harness of `re/tools/emu`) is one call of one
//! original routine on made-up dictionaries, graf lists and models: the buffers it was
//! given (the process record, the dictionaries, the graf lists and the attribute
//! dictionary as globals at their own addresses), what it returned, every buffer it
//! changed, and its calls of the stand-ins with their answers. Nothing here reproduces a
//! function of the original.
//!
//! The comparison is `mattxt-emucheck`'s (si-model): the return value, every buffer of the
//! case, the calls in order with their arguments, that the heap is untouched and that
//! nothing else changed. Columns of the report: cases; identical; identical in the defined
//! part; different; not compared. `EMUCHECK_MUTATE=1` flips one bit of what the port
//! returned and of the first byte it changed; `EMUCHECK_MUTATE=calls` a bit of the first
//! argument of every call.

use si_expr::image::elem;
use si_mem::{Fill, Mem, Names, Ptr32, Region, Ret, Routine, Word as W};
use si_x87::vectors::{Case, Field, Word};
use std::collections::BTreeMap;
use std::io::BufRead;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mutate {
    No,
    Memory,
    Calls,
}

fn mutate() -> Mutate {
    match std::env::var("EMUCHECK_MUTATE").ok().as_deref() {
        None | Some("") | Some("0") => Mutate::No,
        Some("calls") => Mutate::Calls,
        Some(_) => Mutate::Memory,
    }
}

fn words(f: &Field) -> Option<Vec<W>> {
    let convert = |w: &Word| match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    };
    f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
}

fn is_argument(name: &str) -> bool {
    name.starts_with("v_") || name.starts_with("p_")
}

/// A global's address from its field name: `g` and eight hexadecimal digits.
fn global_addr(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if b.len() >= 9 && b[0] == b'g' && b[1..9].iter().all(u8::is_ascii_hexdigit) {
        return u32::from_str_radix(&name[1..9], 16).ok();
    }
    None
}

/// The arguments of a case, by name.
struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
}

impl Args<'_> {
    fn word(&self, name: &str) -> Option<u32> {
        let f = self.case.try_input(name)?;
        match f.words().first() {
            Some(Word::Value { value, .. }) => Some(*value as u32),
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                Some(base.wrapping_add(*offset))
            }
            _ => panic!("argument `{name}` has no value"),
        }
    }

    /// An argument as the 32-bit stack word (by value or a pointer).
    fn v(&self, name: &str) -> u32 {
        self.word(&format!("v_{name}"))
            .or_else(|| self.word(&format!("p_{name}")))
            .unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name))
    }
}

/// The outside world of a case: every call is held against the next one the original
/// made and answered as that one was.
struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    next: usize,
    flip: bool,
    wrong: Option<String>,
}

impl Recorded<'_> {
    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }
}

impl si_mem::Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        let name = routine.name;
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("call {}: the port calls {name} where the original made no further call", self.next));
            return Ret::NONE;
        };
        let n = self.next;
        self.next += 1;
        if want.name != name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {name}, the original {}", want.name));
            return Ret::NONE;
        }
        if want.args.len() != args.len() {
            self.wrong.get_or_insert_with(|| format!("call {n} {name}: {} arguments in the port, {} in the original", args.len(), want.args.len()));
        }
        let mut args = args.to_vec();
        if self.flip {
            if let Some(a) = args.first_mut() {
                *a ^= 1;
            }
        }
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, {expect:08x?} in the original"));
            }
        }
        Ret::int(self.resolve(m, &want.result).unwrap_or(0) as i32)
    }
}

/// From a vector file's routine name to the port: what it returned in `eax`, if anything.
fn run(name: &str, m: &mut Mem, o: &mut Recorded, a: &Args) -> Option<Option<u32>> {
    Some(match name {
        "SI_modelSetHiddenParam" => {
            elem::model_set_hidden_param(m, o, a.v("gn"), a.v("model"), a.v("elem"), a.v("state") as i32, a.v("branch") as i16);
            None
        }
        "SI_materialSetParam" => {
            let (mat, ok) = (a.v("material"), elem::material_set_param_fields(m, a.v("fcv_type") as i32, a.v("material"), a.v("value")));
            if ok {
                si_mem::Outside::call(o, m, Routine::new("SI_materialSetDirty", 0x08281f9c), &[mat, 0x40]);
            }
            Some(ok as u32)
        }
        "si_mdlTxtSetDirtyHW" => {
            elem::mdl_txt_set_dirty_hw(m, a.v("model"), a.v("flag") as i16);
            None
        }
        "SI_generalAttrGetValue" => Some(elem::general_attr_get_value(m, o, Ptr32::new(a.v("ident")), Ptr32::new(a.v("index")), Ptr32::new(a.v("value"))) as u32),
        _ => return None,
    })
}

/// How one case came out.
enum Outcome {
    Same,
    Different(String),
    /// Not compared, with the reason.
    Skip(&'static str),
}

/// A case is run twice, on fresh memory holding a pattern and then its complement (the
/// harness does the same with the original). Both runs must agree with the original. When
/// they do not agree with each other, the port's result depends on memory nobody wrote, and
/// the case is not compared.
fn check(c: &Case, number: u32) -> Outcome {
    let first = attempt(c, number, false);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(c, number, true);
    match (&first, &second) {
        (Outcome::Same, Outcome::Same) => first,
        (Outcome::Different(a), Outcome::Different(b)) if a == b => first,
        (Outcome::Different(_), Outcome::Different(_)) if mutate() != Mutate::No => first,
        _ => Outcome::Skip("the port's result depends on memory nobody wrote (the original's too, unseen by the harness)"),
    }
}

fn attempt(c: &Case, number: u32, second: bool) -> Outcome {
    if c.stopped() {
        return Outcome::Skip("the original stopped");
    }
    if c.has_note("OVERRUN") {
        return Outcome::Skip("the original wrote past a buffer of the case");
    }
    if c.notes.iter().any(|n| n.starts_with("GW:")) {
        return Outcome::Skip("the original wrote a global the case does not declare");
    }
    if c.has_note("UNINIT:calls") {
        return Outcome::Skip("the original's calls depend on memory nobody wrote");
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    si_expr::image::rec::init_globals(&mut m);

    // the buffers: the globals at their addresses, the rest in the arena in the case's order
    let loaded: Vec<(&Field, Option<Vec<W>>)> = c.inputs.iter().filter(|f| !is_argument(&f.name)).map(|f| (f, words(f))).collect();
    let fields: Vec<si_mem::FieldAt<'_>> = loaded.iter().map(|(f, w)| (f.name.as_str(), global_addr(&f.name), w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Outcome::Different(format!("loading the case: {e}")),
    };
    let buffers: Vec<(&Field, u32, &Vec<W>)> = loaded.iter().filter_map(|(f, w)| w.as_ref().map(|w| (*f, names.addr(&m, &f.name).unwrap_or(0), w))).collect();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names };
    let mut outside = Recorded { case: c, names: &names, next: 0, flip: mutate() == Mutate::Calls, wrong: None };
    let routine = c.name.split('#').next().unwrap();
    let Some(mut ret) = run(routine, &mut m, &mut outside, &args) else {
        return Outcome::Skip("no port in the table");
    };
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if outside.next != c.calls.len() {
        return Outcome::Different(format!("the original made {} calls, the port {}", c.calls.len(), outside.next));
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    if mutate() == Mutate::Memory {
        ret = ret.map(|v| v ^ 1);
        for region in [Region::Arena, Region::Data, Region::Bss] {
            if let Some((a, bytes)) = m.changed_in(&before, region).into_iter().next() {
                m.set_u8(a, bytes[0] ^ 1);
                break;
            }
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ax", 0xffff), ("al", 0xff)] {
        if let Some(f) = c.try_output(field) {
            let Some(got) = ret else {
                return Outcome::Different(format!("the original returns {field}, the port nothing"));
            };
            if let Some(Word::Value { value, .. }) = f.words().first() {
                if *value as u32 != got & mask {
                    return Outcome::Different(format!("{field}: original {:x}, port {:x}", value, got & mask));
                }
            }
        }
    }

    // every buffer: as the original left it, or untouched
    for (f, addr, input) in &buffers {
        let expect = match c.try_output(&f.name) {
            Some(out) => words(out).unwrap_or_default(),
            None => (*input).clone(),
        };
        if let Err(e) = m.compare_words(*addr, &expect, &names) {
            return Outcome::Different(format!("buffer `{}`: {e}", f.name));
        }
    }

    // no heap: neither routine allocates
    if !m.heap_blocks().is_empty() || !m.freed().is_empty() {
        return Outcome::Different("the port used the heap".into());
    }

    // nothing else was touched
    for region in [Region::Arena, Region::Data, Region::Bss] {
        for (a, bytes) in m.changed_in(&before, region) {
            let inside = (0..bytes.len() as u32).all(|k| buffers.iter().any(|(_, addr, w)| a + k >= *addr && a + k < *addr + si_mem::words_len(w)));
            if !inside {
                return Outcome::Different(format!("the port changed {} bytes at {a:#010x}, outside every buffer of the case", bytes.len()));
            }
        }
    }
    Outcome::Same
}

#[derive(Default)]
struct Row {
    cases: u32,
    identical: u32,
    defined_part: u32,
    different: u32,
    skipped: u32,
    reasons: BTreeMap<&'static str, u32>,
    examples: Vec<String>,
}

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("expr-elem-emucheck");
    let dir = std::env::args().nth(1).expect("usage: expr-elem-emucheck <directory of vector files>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("the directory").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "vec")).collect();
    paths.sort();
    std::panic::set_hook(Box::new(|_| {}));
    println!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "defined", "different", "not-comp");
    let mut total = Row::default();
    let mut all_reasons: BTreeMap<&'static str, u32> = BTreeMap::new();
    for path in &paths {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let mut row = Row::default();
        let file = std::io::BufReader::new(std::fs::File::open(path).expect("a vector file"));
        for (number, line) in file.lines().enumerate() {
            let line = line.expect("a line");
            let case = match Case::parse(&line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}:{}: {e}", path.display(), number + 1),
            };
            row.cases += 1;
            let outcome = std::panic::catch_unwind(|| check(&case, number as u32)).unwrap_or_else(|p| {
                let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                match p.downcast_ref::<si_mem::Stop>() {
                    Some(s) => Outcome::Different(format!("the port stopped at a fault: {:?}", s.0)),
                    None => Outcome::Different(format!("the port panicked: {}", text.unwrap_or_default())),
                }
            });
            match outcome {
                Outcome::Same if case.has_note("UNINIT") => row.defined_part += 1,
                Outcome::Same => row.identical += 1,
                Outcome::Skip(why) => {
                    row.skipped += 1;
                    *row.reasons.entry(why).or_default() += 1;
                }
                Outcome::Different(d) => {
                    row.different += 1;
                    if row.examples.len() < 2 {
                        let mut text = line.clone();
                        if text.len() > 700 {
                            text.truncate(700);
                            text.push('…');
                        }
                        row.examples.push(format!("line {}: {d}\n      {text}", number + 1));
                    }
                }
            }
        }
        println!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", stem, row.cases, row.identical, row.defined_part, row.different, row.skipped);
        for e in &row.examples {
            println!("    {e}");
        }
        total.cases += row.cases;
        total.identical += row.identical;
        total.defined_part += row.defined_part;
        total.different += row.different;
        total.skipped += row.skipped;
        for (k, v) in row.reasons {
            *all_reasons.entry(k).or_default() += v;
        }
    }
    println!(
        "{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}",
        format!("TOTAL ({} files)", paths.len()),
        total.cases,
        total.identical,
        total.defined_part,
        total.different,
        total.skipped
    );
    for (why, n) in &all_reasons {
        println!("  not compared, {why}: {n}");
    }
    std::process::exit(if total.different == 0 { 0 } else { 1 });
}
