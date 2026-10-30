// SPDX-License-Identifier: MIT
use crate::outside::Recorded;
use crate::{Args, Kit, Ret};
use si_mem::{Fill, Mem, Region, Word as W};
use si_x87::vectors::{Case, Field, Word};

/// What a run asks of the driver.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// `EMUCHECK_MUTATE`: `1` flips a bit of the port's first value, `calls` of the first
    /// argument of every call.
    pub mutate: Mutate,
    /// A callee the case does not record is answered 0 and counted, not an error.
    pub lenient: bool,
    /// Print every differing case with its first difference (not two examples per file).
    pub first_diff: bool,
    /// Only the routines whose name is this one.
    pub only: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mutate {
    #[default]
    No,
    Memory,
    Calls,
}

impl Mutate {
    pub fn from_env() -> Mutate {
        match std::env::var("EMUCHECK_MUTATE").ok().as_deref() {
            None | Some("") | Some("0") => Mutate::No,
            Some("calls") => Mutate::Calls,
            Some(_) => Mutate::Memory,
        }
    }
}

/// How one case came out.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Same,
    Different(String),
    /// Not compared, with the reason.
    Skip(&'static str),
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

fn words(f: &Field) -> Option<Vec<W>> {
    let convert = |w: &Word| match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    };
    f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
}

/// Runs one case twice and settles it. `number` seeds the fill pattern.
pub fn check_case(kit: &Kit, cfg: &Config, c: &Case, number: u32) -> Outcome {
    let first = attempt(kit, cfg, c, number, false);
    if matches!(first, Outcome::Skip(_)) {
        return first;
    }
    let second = attempt(kit, cfg, c, number, true);
    match (&first, &second) {
        (Outcome::Same, Outcome::Same) => first,
        // The routine's own return value is a word nobody set: what it did last hangs on that
        // word too, and the port's choice for it need not be the one both runs made.
        (Outcome::Different(_), _) if ["UNINIT:eax", "UNINIT:ax", "UNINIT:al"].iter().any(|n| c.notes.iter().any(|x| x == n)) => {
            Outcome::Skip("the original returns a word nobody set, and what it does last depends on it")
        }
        (Outcome::Different(a), Outcome::Different(b)) if a == b => first,
        (Outcome::Different(_), Outcome::Different(_)) if cfg.mutate != Mutate::No => first,
        _ => Outcome::Skip("the port's result depends on memory nobody wrote (the original's too, unseen by the harness)"),
    }
}

fn attempt(kit: &Kit, cfg: &Config, c: &Case, number: u32, second: bool) -> Outcome {
    if c.stopped() {
        return Outcome::Skip("the original stopped");
    }
    if c.has_note("OVERRUN") {
        return Outcome::Skip("the original wrote past a buffer of the case");
    }
    if c.notes.iter().any(|n| n.starts_with("GW:") && !kit.allowed_globals.contains(&n.as_str())) {
        return Outcome::Skip("the original wrote a global the case does not declare");
    }
    if c.has_note("UNINIT:calls") {
        return Outcome::Skip("the original's calls depend on memory nobody wrote");
    }
    let routine = c.name.split('#').next().unwrap();
    let Some(port) = kit.ports.iter().find(|p| p.0 == routine).map(|p| p.1) else {
        return Outcome::Skip("no port in the table");
    };
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    if let Some(setup) = kit.setup {
        setup(&mut m);
    }

    for &(a, v) in &kit.scratch {
        m.set_u32(a, v);
    }

    // the buffers: globals at their addresses, the others in the arena in the case's order
    let loaded: Vec<(&Field, Option<u32>, Option<Vec<W>>)> = c.inputs.iter().filter(|f| !is_argument(&f.name)).map(|f| (f, global_addr(&f.name), words(f))).collect();
    let fields: Vec<si_mem::FieldAt<'_>> = loaded.iter().map(|(f, at, w)| (f.name.as_str(), *at, w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Outcome::Different(format!("loading the case: {e}")),
    };
    let buffers: Vec<(&Field, u32, &Vec<W>)> = loaded.iter().filter_map(|(f, _, w)| w.as_ref().map(|w| (*f, names.addr(&m, &f.name).unwrap_or(0), w))).collect();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();
    // (a global the case itself declares is the case's: compared, not restored)
    let scratch: Vec<(u32, u32)> = kit.scratch.iter().filter(|(a, _)| !loaded.iter().any(|(_, at, _)| *at == Some(*a))).map(|&(a, _)| (a, m.u32(a))).collect();

    // the port, with the C library's mathematics played back from the case's recorded calls
    let args = Args::new(c, &names);
    let mut outside = Recorded::new(c, &args, &names, &kit.hooks, cfg.mutate == Mutate::Calls, cfg.lenient);
    outside.stack_args = kit.stack_args;
    outside.entry_sp = m.sp();
    si_x87::trig::replay::start(c.tape());
    let ran = port(&mut m, &mut outside, &args);
    let replayed = si_x87::trig::replay::finish();
    for &(a, v) in &scratch {
        m.set_u32(a, v);
    }
    if let Err(e) = replayed {
        return Outcome::Different(format!("the library's mathematics: {e}"));
    }
    outside.skip_libm();
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    if !cfg.lenient && outside.calls_made() != c.calls.len() {
        return Outcome::Different(format!("the original made {} calls, the port {}", c.calls.len(), outside.calls_made()));
    }
    let (mut stdout, mut stderr) = (std::mem::take(&mut outside.stdout), std::mem::take(&mut outside.stderr));
    if !outside.unported.is_empty() {
        UNPORTED.with(|u| {
            for (k, v) in &outside.unported {
                *u.borrow_mut().entry(k.clone()).or_default() += v;
            }
        });
    }

    // the mutation switch: one bit of the result, of the first byte the port changed, and of
    // the first byte it printed
    let mut ret = ran;
    if cfg.mutate == Mutate::Memory {
        for text in [&mut stdout, &mut stderr] {
            if let Some(b) = text.first_mut() {
                *b ^= 1;
            }
        }
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        let changed = m.changed_in(&before, Region::Arena).into_iter().chain(m.changed_in(&before, Region::Bss)).chain(m.changed_in(&before, Region::Data)).next();
        if let Some((a, bytes)) = changed {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().first() {
            if n > 0 {
                let b = m.u8(a);
                m.set_u8(a, b ^ 1);
            }
        }
    }

    // what was printed
    for (field, got) in [("stdout", &stdout), ("stderr", &stderr)] {
        let want: Vec<u8> = c.try_output(field).map_or(Vec::new(), |f| f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect());
        if want != **got {
            return Outcome::Different(format!("{field}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(got)));
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ax", 0xffff), ("al", 0xff), ("ptr", u32::MAX)] {
        if let Some(f) = c.try_output(field) {
            let Ret::Eax(got) = ret else {
                return Outcome::Different(format!("the original returns {field}, the port nothing"));
            };
            match f.words().first() {
                Some(Word::Value { value, .. }) if *value as u32 != got & mask => {
                    return Outcome::Different(format!("{field}: original {:x}, port {:x}", value, got & mask));
                }
                Some(Word::Ref { name, offset }) => {
                    let want = names.addr(&m, name).map(|a| a.wrapping_add(*offset));
                    if want != Some(got) {
                        return Outcome::Different(format!("{field}: original &{name}+{offset:#x}, port {got:08x}"));
                    }
                }
                _ => {}
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

    // the heap
    let blocks: Vec<(u32, u32)> = m.heap_blocks().to_vec();
    let want_blocks = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
    if want_blocks != blocks.len() {
        return Outcome::Different(format!("the original allocated {want_blocks} blocks, the port {}", blocks.len()));
    }
    for (i, &(addr, size)) in blocks.iter().enumerate() {
        let name = format!("heap{i}");
        let expect = words(c.output(&name)).unwrap_or_default();
        if si_mem::words_len(&expect) != size {
            return Outcome::Different(format!("{name}: {} bytes in the original, {size} in the port", si_mem::words_len(&expect)));
        }
        if let Err(e) = m.compare_words(addr, &expect, &names) {
            return Outcome::Different(format!("{name}: {e}"));
        }
        // a byte the original never wrote must not have been written here either
        if !c.notes.iter().any(|n| n == &format!("UNINIT:{name}")) {
            let mut at = addr;
            for w in &expect {
                if let W::Unwritten { bytes } = w {
                    for k in 0..*bytes as u32 {
                        if m.u8(at + k) != fill.byte(at + k) {
                            return Outcome::Different(format!("{name}+{:#x}: written by the port, not by the original", at + k - addr));
                        }
                    }
                }
                at += w.bytes();
            }
        }
    }

    // the frees, in order
    let want_freed: Vec<Option<u32>> = c.try_output("freed").map_or(Vec::new(), |f| {
        f.words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => names.addr(&m, name).map(|a| a.wrapping_add(*offset)),
                Word::Value { value, .. } => Some(*value as u32),
                Word::Unwritten { .. } => None,
            })
            .collect()
    });
    let got_freed: Vec<Option<u32>> = m.freed().iter().map(|&a| Some(a)).collect();
    if want_freed != got_freed {
        let at = want_freed.iter().zip(&got_freed).position(|(a, b)| a != b);
        return Outcome::Different(format!("freed: {} blocks in the original, {} in the port, first difference at {at:?}", want_freed.len(), got_freed.len()));
    }

    // nothing else was touched
    for region in [Region::Arena, Region::Data, Region::Bss] {
        for (a, bytes) in m.changed_in(&before, region) {
            let inside = (0..bytes.len() as u32).all(|k| buffers.iter().any(|(_, addr, w)| a + k >= *addr && a + k < *addr + si_mem::words_len(w)));
            if !inside {
                // name the neighbourhood: the buffer that ends last before the address
                let near = names.iter().filter(|b| b.1 <= a).max_by_key(|b| b.1).map(|b| format!(", {:#x} after the start of `{}`", a - b.1, b.0)).unwrap_or_default();
                return Outcome::Different(format!("the port changed {} bytes at {a:#010x}{near}, outside every buffer of the case (first byte {:02x})", bytes.len(), bytes[0]));
            }
        }
    }
    Outcome::Same
}

thread_local! {
    /// Callees a lenient run reached that the case does not record, over all its cases.
    pub(crate) static UNPORTED: std::cell::RefCell<std::collections::BTreeMap<String, u32>> = Default::default();
}
