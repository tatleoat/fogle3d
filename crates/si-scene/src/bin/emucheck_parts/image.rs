// SPDX-License-Identifier: MIT
//! Cases run on the memory image: the case's buffers are loaded where the harness had
//! them (`si_mem::load_case_at`), the port runs on them, and memory is compared.
//!
//! Two conventions of vector lines are read:
//!
//! - the one of `re/tools/si-dict/dictlib.py` (which `re/tools/si-scene/emu_gen_node.py`
//!   follows): `v_x` an argument by value, `p_x` a pointer argument, `gXXXXXXXX…` a global
//!   at that address, anything else a buffer of the arena, in the line's order;
//! - the older one of `re/tools/si-scene/emu_gen.py`, whose lines do not say which field
//!   is what: the routine's entry in the table carries the generator's own spec line
//!   ([`Spec`]), from which the buffers are placed in the harness's order.
//!
//! Compared, as `dict-emucheck` compares: the return value; every buffer of the case,
//! changed or not; the heap, block by block (as many, each of the same size, each written
//! byte the same, each byte the original did not write still holding the fill); the freed
//! blocks in order; the calls outside the source files, in order, with their arguments;
//! and that the port changed nothing outside the case's buffers and the heap.
//!
//! A case is run twice, on a fill pattern and on its complement, as the harness runs the
//! original. `EMUCHECK_MUTATE=1` flips one bit of what the port returned and of the first
//! byte it changed.

use si_dict::outside::{Outside, Routine};
use si_mem::{Fill, Mem, Names, Region, Word as W};
use si_x87::vectors::{Case, Field, Verdict, Word};
use std::sync::atomic::{AtomicU32, Ordering};

#[path = "dictrest.rs"]
pub mod dictrest;
#[path = "edfirst.rs"]
pub mod edfirst;
#[path = "edname.rs"]
pub mod edname;

/// The global the allocation wrappers' callers count in: not ported (`si-dict`'s top).
const SECURE_LIC_ALLOC: &str = "GW:SI_secureLicAlloc+0x0";

/// `EMUCHECK_MUTATE=1` (any value but `calls`): one bit of what the port produces flipped.
pub fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some_and(|v| v != "calls")
}

/// `EMUCHECK_MUTATE=calls`: one bit of every argument the port passes to a recorded call
/// flipped before it is held against the record (as `rensetup-emucheck` does).
pub fn mutate_calls() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some_and(|v| v == "calls")
}

pub fn words(f: &Field) -> Option<Vec<W>> {
    let convert = |w: &Word| match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    };
    f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
}

/// A global's address from its field name: `g` and eight hexadecimal digits.
fn global_addr(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if b.len() >= 9 && b[0] == b'g' && b[1..9].iter().all(u8::is_ascii_hexdigit) {
        return u32::from_str_radix(&name[1..9], 16).ok();
    }
    None
}

/// One slot of a spec line of the older convention.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    /// An argument by value (an integer, or a pointer printed as `&buffer+offset`).
    Value,
    /// A buffer the case fills: an argument that points at it, or a side buffer.
    Buffer,
    /// A string argument: the line holds its characters, the buffer also the terminator.
    Str,
    /// A buffer the routine only writes, of this many bytes.
    Out(u32),
    /// A buffer at a fixed address: a global.
    Global(u32),
}

/// The slots of a routine of the older convention, in the order of the generator's spec
/// line: `name` (by value), `name*` (a buffer), `name$` (a string), `name:N` (an output
/// buffer of N bytes), `name@ADDRESS` (a global).
pub struct Spec(pub &'static str);

impl Spec {
    fn slots(&self) -> Vec<(&'static str, Slot)> {
        self.0
            .split_whitespace()
            .map(|t| {
                if let Some((n, a)) = t.split_once('@') {
                    (n, Slot::Global(u32::from_str_radix(a.trim_start_matches("0x"), 16).expect("an address")))
                } else if let Some((n, size)) = t.split_once(':') {
                    (n, Slot::Out(size.parse().expect("a size")))
                } else if let Some(n) = t.strip_suffix('*') {
                    (n, Slot::Buffer)
                } else if let Some(n) = t.strip_suffix('$') {
                    (n, Slot::Str)
                } else {
                    (t, Slot::Value)
                }
            })
            .collect()
    }
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    case: &'a Case,
    names: &'a Names,
    old: bool,
}

impl Args<'_> {
    fn resolve(&self, f: &Field, what: &str) -> u32 {
        match f.words().first() {
            Some(Word::Value { value, .. }) => *value as u32,
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                base.wrapping_add(*offset)
            }
            _ => panic!("argument `{what}` has no value"),
        }
    }

    /// An argument as the 32-bit stack word: a value, or the address a pointer holds.
    pub fn v(&self, name: &str) -> u32 {
        if self.old {
            // a buffer of that name is passed by its address
            if let Some(b) = self.names.iter().find(|b| b.0 == name) {
                return b.1;
            }
            let f = self.case.try_input(name).unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name));
            if f.is_null() {
                return 0;
            }
            return self.resolve(f, name);
        }
        for key in [format!("v_{name}"), format!("p_{name}")] {
            if let Some(f) = self.case.try_input(&key) {
                return self.resolve(f, name);
            }
        }
        panic!("case of {} has no argument `{name}`", self.case.name)
    }

    pub fn i(&self, name: &str) -> i32 {
        self.v(name) as i32
    }

    pub fn s(&self, name: &str) -> i16 {
        self.v(name) as i16
    }

    pub fn p<T>(&self, name: &str) -> si_mem::Ptr32<T> {
        si_mem::Ptr32::new(self.v(name))
    }

}

/// What a routine returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    None,
    Eax(u32),
    /// What a routine leaves in the x87 register (its two bit fields: significand, sign and
    /// exponent).
    St0(u64, u16),
}

impl From<si_x87::F80> for Ret {
    fn from(v: si_x87::F80) -> Ret {
        let (m, se) = v.to_bits();
        Ret::St0(m, se)
    }
}

impl From<()> for Ret {
    fn from(_: ()) -> Ret {
        Ret::None
    }
}
impl From<i32> for Ret {
    fn from(v: i32) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl From<u32> for Ret {
    fn from(v: u32) -> Ret {
        Ret::Eax(v)
    }
}
impl From<i16> for Ret {
    fn from(v: i16) -> Ret {
        Ret::Eax(v as u16 as u32)
    }
}
impl From<bool> for Ret {
    fn from(v: bool) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl<T> From<si_mem::Ptr32<T>> for Ret {
    fn from(v: si_mem::Ptr32<T>) -> Ret {
        Ret::Eax(v.addr())
    }
}

/// The outside world of a case: every call is held against the next one the original
/// made and answered as that one was. The C library's `free`, which the older vectors
/// record as a call, is not a call here: the frees the port made since the last outside
/// call are read from the image and held against the recorded ones, in order.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    next: usize,
    frees_seen: usize,
    /// The case follows the older convention.
    old: bool,
    pub wrong: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Recorded<'_> {
    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }

    /// Holds the frees made since the last look against the recorded `free` calls.
    fn frees(&mut self, m: &Mem) {
        let done: Vec<u32> = m.freed()[self.frees_seen..].to_vec();
        self.frees_seen = m.freed().len();
        if !self.case.calls.iter().any(|c| c.name == "free") {
            return; // this case lists its frees as an output, compared at the end
        }
        for addr in done {
            let n = self.next;
            match self.case.calls.get(n) {
                Some(want) if want.name == "free" => {
                    self.next += 1;
                    let expect = want.args.first().and_then(|w| self.resolve(m, w));
                    if expect != Some(addr) {
                        self.wrong.get_or_insert_with(|| format!("call {n}: the port frees {addr:08x}, the original {expect:08x?}"));
                    }
                }
                Some(want) => {
                    self.wrong.get_or_insert_with(|| format!("call {n}: the port frees {addr:08x} where the original calls {}", want.name));
                }
                None => {
                    self.wrong.get_or_insert_with(|| format!("call {n}: the port frees {addr:08x} where the original made no further call"));
                }
            }
        }
    }
}

/// The C library's mathematics that the harness logs when a routine that ran as it is calls
/// it (`SI_matrixExtractXyzRotation` under `SI_waveAddMdl`): the port makes those calls
/// inside si-math's port with its own `asin`, `acos`, `cos`, `sin`, not through the
/// outside world, so the logged ones are passed over (such a case is not authoritative:
/// the report counts it apart).
const LIBM: [&str; 4] = ["asin", "acos", "cos", "sin"];

impl Recorded<'_> {
    /// Passes over the logged calls of [`LIBM`] at the cursor (not one of the routine the
    /// port calls now).
    fn skip_libm(&mut self, calling: Option<&str>) {
        while let Some(want) = self.case.calls.get(self.next) {
            if !LIBM.contains(&want.name.as_str()) || calling == Some(want.name.as_str()) {
                break;
            }
            self.next += 1;
        }
    }
}

/// The routines the generator of the older vectors replaced by recorded stand-ins; every
/// other routine outside the dictionary source files ran in the emulator as it is.
const OLD_STUBS: [&str; 4] = ["SI_elemdictDisposeRelationData", "SI_dbCheckExtraUnToldRelation", "SI_cusResortUpdList", "SI_dbListNodeFree"];

impl Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        if self.old && !OLD_STUBS.contains(&routine.name) && routine.name != "time" {
            // not a stand-in in these vectors: the routine itself ran. The name-record
            // routines are this crate's on the image; the default colour and the load
            // optimisation are what the binary's zeroed globals and an empty environment
            // give; the rest is not reached by the routines checked on these vectors.
            use si_dict::outside as out;
            use si_scene::dbnode;
            let a = |k: usize| args.get(k).copied().unwrap_or(0);
            return match routine.addr {
                x if x == out::SI_GET_LOAD_OPTIM.addr || x == out::SI_GRAFNODE_DEF_COLOR.addr => 0,
                x if x == out::SI_DB_LIST_NODE_ALLOCATE.addr => dbnode::list_node_allocate(m, self).addr(),
                x if x == out::SI_DB_FORMAT_FULL_NAME.addr => dbnode::format_full_name(m, si_mem::Ptr32::new(a(0))).addr(),
                x if x == out::SI_DBNAME_CHECK_VALIDITY.addr => dbnode::name_check_validity(m, si_mem::Ptr32::new(a(0)), a(1) as i16).addr(),
                x if x == out::SI_ED_GET_CHAPTER_FROM_SUFFIX.addr => dbnode::ed_get_chapter_from_suffix(m, si_mem::Ptr32::new(a(0)), si_mem::Ptr32::new(a(1))) as u32,
                x if x == out::SI_ED_GET_SUFFIX_FROM_CHAPTER.addr => dbnode::ed_get_suffix_from_chapter(m, a(0) as i32).addr(),
                x if x == out::DBFILENAME_EXPAND.addr => {
                    dbnode::filename_expand(m, si_mem::Ptr32::new(a(0)), [a(1), a(2), a(3), a(4), a(5)]);
                    0
                }
                x if x == out::SI_ED_GET_FCV_FROM_RELATION.addr => si_scene::world::ed_get_fcv_from_relation(m, si_mem::Ptr32::new(a(0))).addr(),
                _ => {
                    self.wrong.get_or_insert_with(|| format!("the port calls {}, which these vectors have no answer for", routine.name));
                    0
                }
            };
        }
        self.frees(m);
        self.skip_libm(Some(routine.name));
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("call {}: the port calls {} where the original made no further call", self.next, routine.name));
            return 0;
        };
        let n = self.next;
        self.next += 1;
        if want.name != routine.name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {}, the original {}", routine.name, want.name));
            return 0;
        }
        // the harness logs as many words as the stand-in's rule names; the port may pass more
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied().map(|g| if mutate_calls() { g ^ 1 } else { g });
            if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {}: argument {k} is {:08x?} in the port, {:08x?} in the original", routine.name, got, expect));
            }
        }
        self.resolve(m, &want.result).unwrap_or(0)
    }

    /// The environment of a case is in its line: an extra argument `v_env_NAME` for each
    /// variable that is set (to "1"). The harness's `getenv` puts the value in a new block
    /// each time it is asked; so does this one.
    fn getenv(&mut self, m: &mut Mem, name: &[u8]) -> u32 {
        if self.case.try_input(&format!("v_env_{}", String::from_utf8_lossy(name))).is_none() {
            return 0;
        }
        let p = m.malloc(2);
        m.put_cstr(p, b"1");
        p
    }

    fn stdout(&mut self, text: &[u8]) {
        self.stdout.extend_from_slice(text);
    }

    fn stderr(&mut self, text: &[u8]) {
        self.stderr.extend_from_slice(text);
    }
}

/// A port as the check calls it.
pub type Port = fn(&mut Mem, &mut Recorded, &Args) -> Ret;

/// One routine of the table.
pub struct Entry {
    pub port: Port,
    /// The spec line of a routine whose vectors follow the older convention.
    pub spec: Option<Spec>,
    /// Blocks inside buffers of the case that the routine may free: called with the image
    /// and the names once the buffers are placed, answers `(address, size)` of each.
    pub blocks: Option<fn(&Case, &Names) -> Vec<(u32, u32)>>,
    /// What the image holds before the case's buffers are stored (constants, globals).
    pub prepare: fn(&mut Mem),
    /// The vectors list every block from `malloc` (`heap=` in the spec line). Without it
    /// the blocks the port made are not held against anything.
    pub heap: bool,
    /// A comparison of the routine's own: outputs the generator read from memory itself.
    pub post: Option<fn(&Case, &Mem, &Names) -> Result<(), String>>,
}

pub fn prepare_edname(m: &mut Mem) {
    prepare_default(m);
    si_scene::edname::store(m);
}

fn prepare_default(m: &mut Mem) {
    si_mem::init_globals(m, si_dict::globals::ALL);
    si_dict::rodata::store(m);
    si_scene::globals::store(m);
}

/// The image of a routine that prints on `stdout`: the slot the dynamic linker fills holds
/// the generator's stream value (`FILES + 0x100` of re/tools/emu/standins.py).
fn prepare_stdout(m: &mut Mem) {
    prepare_default(m);
    m.set_u32(si_scene::dictrest::STDOUT_SLOT, 0x6f00_0100);
}

impl Entry {
    /// A routine whose vectors follow the newer convention and that reads the `stdout` slot.
    pub fn with_stdout(port: Port) -> Entry {
        Entry { prepare: prepare_stdout, ..Entry::new(port) }
    }

    /// A routine whose vectors follow the older convention: its slots in the generator's
    /// order, whether the spec line had `heap=`, and the blocks inside the case's buffers.
    pub fn old(port: Port, spec: &'static str, heap: bool, blocks: fn(&Case, &Names) -> Vec<(u32, u32)>) -> Entry {
        Entry { port, spec: Some(Spec(spec)), blocks: Some(blocks), prepare: prepare_default, heap, post: None }
    }

    pub fn new(port: Port) -> Entry {
        Entry { port, spec: None, blocks: None, prepare: prepare_default, heap: true, post: None }
    }
}

static CASE_NUMBER: AtomicU32 = AtomicU32::new(0);

/// Runs one case on the image, twice. Both runs must agree with the original; when they
/// do not agree with each other the port's result depends on memory nobody wrote, and
/// the case is not compared.
pub fn check(c: &Case, entry: &Entry) -> Verdict {
    let number = CASE_NUMBER.fetch_add(1, Ordering::Relaxed);
    let first = attempt(c, entry, number, false);
    if matches!(first, Verdict::Skip) {
        return first;
    }
    // the recorded mathematics (`si_x87::trig::replay`) once more for the second run: the
    // first run's use of the tape is held here, the second's by the runner
    let first = match (first, si_x87::trig::replay::finish()) {
        (Verdict::Same, Err(e)) => Verdict::Different(e),
        (v, _) => v,
    };
    si_x87::trig::replay::start(c.tape());
    let second = attempt(c, entry, number, true);
    match (&first, &second) {
        (Verdict::Same, Verdict::Same) => first,
        (Verdict::Different(a), Verdict::Different(b)) if a == b => first,
        (Verdict::Different(a), _) | (_, Verdict::Different(a)) => Verdict::Different(format!("on one fill pattern only: {a}")),
        _ => Verdict::Skip,
    }
}

struct Loaded {
    names: Names,
    /// Every buffer of the case: field name, address, the words stored.
    buffers: Vec<(String, u32, Vec<W>)>,
}

fn load(c: &Case, entry: &Entry, m: &mut Mem) -> Result<Loaded, String> {
    // name, address the vector gives, words
    let mut fields: Vec<(String, Option<u32>, Option<Vec<W>>)> = Vec::new();
    match &entry.spec {
        None => {
            for f in c.inputs.iter().filter(|f| !(f.name.starts_with("v_") || f.name.starts_with("p_"))) {
                fields.push((f.name.clone(), global_addr(&f.name), words(f)));
            }
        }
        Some(spec) => {
            for (name, slot) in spec.slots() {
                match slot {
                    Slot::Value => {}
                    Slot::Buffer => {
                        // (a buffer the generator left out of this routine's spec line)
                        if let Some(f) = c.try_input(name) {
                            fields.push((name.to_string(), None, words(f)));
                        }
                    }
                    Slot::Str => {
                        if let Some(f) = c.try_input(name) {
                            fields.push((name.to_string(), None, words(f).map(|mut w| {
                                w.push(W::Value { bytes: 1, value: 0 });
                                w
                            })));
                        }
                    }
                    Slot::Out(size) => fields.push((name.to_string(), None, Some(vec![W::Unwritten { bytes: 1 }; size as usize]))),
                    Slot::Global(addr) => {
                        if let Some(f) = c.try_input(name) {
                            fields.push((name.to_string(), Some(addr), words(f)));
                        }
                    }
                }
            }
        }
    }
    let refs: Vec<si_mem::FieldAt<'_>> = fields.iter().map(|(n, a, w)| (n.as_str(), *a, w.as_deref())).collect();
    let names = si_mem::load_case_at(m, &refs)?;
    // blocks inside the buffers that the routine may free: placed, then the buffers'
    // words stored once more (placing refills with the pattern)
    if let Some(blocks) = entry.blocks {
        for (addr, size) in blocks(c, &names) {
            m.place_at(addr, size);
        }
        for (n, _, w) in &fields {
            if let (Some(a), Some(w)) = (names.addr(m, n), w) {
                m.load_words(a, w, &names)?;
            }
        }
    }
    let buffers = fields.into_iter().filter_map(|(n, _, w)| w.map(|w| (names.addr(m, &n).unwrap_or(0), n, w))).map(|(a, n, w)| (n, a, w)).collect();
    Ok(Loaded { names, buffers })
}

fn attempt(c: &Case, entry: &Entry, number: u32, second: bool) -> Verdict {
    let old = entry.spec.is_some();
    if c.stopped() || c.has_note("OVERRUN") {
        return Verdict::Skip;
    }
    // (the older vectors declare fewer globals: one the routine wrote is a note there, and
    // its value is not in the case)
    if !old && c.notes.iter().any(|n| n.starts_with("GW:") && n != SECURE_LIC_ALLOC) {
        return Verdict::Skip;
    }
    let mut m = Mem::new();
    let fill = Fill::Pattern { second, salt: number.wrapping_mul(4099) };
    m.set_fill(fill);
    (entry.prepare)(&mut m);
    let Loaded { names, buffers } = match load(c, entry, &mut m) {
        Ok(l) => l,
        Err(e) => return Verdict::Different(format!("loading the case: {e}")),
    };
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("loading the case faulted: {f:?}"));
    }
    let before = m.snapshot();

    // the port
    let args = Args { case: c, names: &names, old };
    let mut outside = Recorded { case: c, names: &names, next: 0, frees_seen: 0, old, wrong: None, stdout: Vec::new(), stderr: Vec::new() };
    let ret = (entry.port)(&mut m, &mut outside, &args);
    outside.frees(&m);
    outside.skip_libm(None);
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Verdict::Different(w);
    }
    // calls of the C library that the older vectors record and the image makes itself
    let own = |name: &str| ["malloc", "calloc", "realloc", "strdup"].contains(&name);
    let expected_calls = c.calls.iter().filter(|call| !own(&call.name)).count();
    if outside.next != expected_calls {
        return Verdict::Different(format!("the original made {} outside calls, the port {}", expected_calls, outside.next));
    }

    // what was printed
    for (field, got) in [("stdout", &outside.stdout), ("stderr", &outside.stderr)] {
        let want: Vec<u8> = c.try_output(field).map_or(Vec::new(), |f| f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect());
        if want != **got {
            return Verdict::Different(format!("{field}: original {:?}, port {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(got)));
        }
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    // under the mutation switch, when the port neither returned nor changed anything: the
    // last block it freed is taken for another
    let mut freed_bit = 0;
    if mutate() {
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        if let Ret::St0(v, se) = ret {
            ret = Ret::St0(v ^ 1, se);
        }
        let changed = m.changed_in(&before, Region::Arena).into_iter().chain(m.changed_in(&before, Region::Bss)).chain(m.changed_in(&before, Region::Data)).next();
        if let Some((a, bytes)) = changed {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().first() {
            if n > 0 {
                let b = m.u8(a);
                m.set_u8(a, b ^ 1);
            }
        } else if ret == Ret::None {
            freed_bit = 8;
        }
    }

    // the return value
    for (field, mask) in [("eax", u32::MAX), ("ptr", u32::MAX), ("ax", 0xffff), ("al", 0xff)] {
        if let Some(f) = c.try_output(field) {
            let Ret::Eax(got) = ret else {
                return Verdict::Different(format!("the original returns {field}, the port nothing"));
            };
            match f.words().first() {
                Some(Word::Value { value, .. }) if *value as u32 != got & mask => {
                    return Verdict::Different(format!("{field}: original {:x}, port {:x}", value, got & mask));
                }
                Some(Word::Ref { name, offset }) => {
                    let want = names.addr(&m, name).map(|a| a.wrapping_add(*offset));
                    if want != Some(got) {
                        return Verdict::Different(format!("{field}: original &{name}+{offset:#x}, port {got:08x}"));
                    }
                }
                _ => {}
            }
        }
    }

    // a result in the x87 register
    if let Some(f) = c.try_output("st0") {
        let Ret::St0(got, se) = ret else {
            return Verdict::Different("the original returns st0, the port does not".into());
        };
        let want = f.f80().to_bits();
        if want != (got, se) {
            return Verdict::Different(format!("st0: original {:016x}:{:04x}, port {got:016x}:{se:04x}", want.0, want.1));
        }
    }

    // every buffer: as the original left it, or untouched
    for (name, addr, input) in &buffers {
        let expect = match c.try_output(name) {
            Some(out) => words(out).unwrap_or_default(),
            None => input.clone(),
        };
        if let Err(e) = m.compare_words(*addr, &expect, &names) {
            return Verdict::Different(format!("buffer `{name}`: {e}"));
        }
    }

    // the heap
    let blocks: Vec<(u32, u32)> = if entry.heap { m.heap_blocks().to_vec() } else { Vec::new() };
    let want_blocks = c.outputs.iter().filter(|f| f.name.strip_prefix("heap").is_some_and(|n| n.parse::<u32>().is_ok())).count();
    if entry.heap && want_blocks != blocks.len() {
        return Verdict::Different(format!("the original allocated {want_blocks} blocks, the port {}", blocks.len()));
    }
    for (i, &(addr, size)) in blocks.iter().enumerate() {
        let name = format!("heap{i}");
        let expect = words(c.output(&name)).unwrap_or_default();
        if si_mem::words_len(&expect) != size {
            return Verdict::Different(format!("{name}: {} bytes in the original, {size} in the port", si_mem::words_len(&expect)));
        }
        if let Err(e) = m.compare_words(addr, &expect, &names) {
            return Verdict::Different(format!("{name}: {e}"));
        }
        // a byte the original never wrote must not have been written here either
        let mut at = addr;
        for w in &expect {
            if let W::Unwritten { bytes } = w {
                for k in 0..*bytes as u32 {
                    if m.u8(at + k) != fill.byte(at + k) {
                        return Verdict::Different(format!("{name}+{:#x}: written by the port, not by the original", at + k - addr));
                    }
                }
            }
            at += w.bytes();
        }
    }

    // the frees, in order, where the vector lists them as an output
    if let Some(f) = c.try_output("freed") {
        let want: Vec<Option<u32>> = f
            .words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => names.addr(&m, name).map(|a| a.wrapping_add(*offset)),
                Word::Value { value, .. } => Some(*value as u32),
                Word::Unwritten { .. } => None,
            })
            .collect();
        let mut got: Vec<Option<u32>> = m.freed().iter().map(|&a| Some(a)).collect();
        if let Some(Some(last)) = got.last_mut() {
            *last ^= freed_bit;
        }
        if want != got {
            let at = want.iter().zip(&got).position(|(a, b)| a != b);
            return Verdict::Different(format!("freed: {} blocks in the original, {} in the port, first difference at {at:?}", want.len(), got.len()));
        }
    } else if !c.calls.iter().any(|call| call.name == "free") && !m.freed().is_empty() {
        return Verdict::Different(format!("the port freed {} blocks, the original none", m.freed().len()));
    }

    if let Some(post) = entry.post {
        if let Err(e) = post(c, &m, &names) {
            return Verdict::Different(e);
        }
    }

    // nothing else was touched (the older vectors do not say which globals a routine may
    // write: there only the arena is held)
    let regions: &[Region] = if old { &[Region::Arena] } else { &[Region::Arena, Region::Data, Region::Bss] };
    for &region in regions {
        for (a, bytes) in m.changed_in(&before, region) {
            let inside = buffers.iter().any(|(_, addr, w)| a >= *addr && a + bytes.len() as u32 <= *addr + si_mem::words_len(w));
            if !inside {
                return Verdict::Different(format!("the port changed {} bytes at {a:#010x}, outside every buffer of the case", bytes.len()));
            }
        }
    }
    Verdict::Same
}

/// The callees of the routines around the element readers (`emu_gen_read.py`), as the
/// recorded stand-ins of their vectors are named.
static R_FOPEN: Routine = Routine { name: "fopen", addr: 0x080c_4e18 };
static R_FCLOSE: Routine = Routine { name: "fclose", addr: 0x080c_4958 };
static R_FDMODEL: Routine = Routine { name: "SI_fdmodelBinaryRead", addr: 0x083e_c740 };
static R_UPDATE_SON_STATE: Routine = Routine { name: "SI_modelUpdateSonState", addr: 0x0846_62f8 };
/// The recorded stand-in of the vectors of `emu_gen_edlook.py`.
static R_ACTIVATION: Routine = Routine { name: "SI_activationFcvCheckTime", addr: si_scene::edtools::SI_ACTIVATION_FCV_CHECK_TIME };
/// The recorded stand-ins of the vectors of `emu_gen_edmat.py`.
static R_SCHEMATIC_HIER_OPERATION: Routine = Routine { name: "SI_schematicHierOperation", addr: si_scene::edtools::SI_SCHEMATIC_HIER_OPERATION };
static R_SET_FCURVE_FLAG: Routine = Routine { name: "SI_attdictSetFcurveFlag", addr: si_scene::edtools::SI_ATTDICT_SET_FCURVE_FLAG };
static R_MATERIAL_TRANSFER: Routine = Routine { name: "SI_materialTransfer", addr: si_scene::matconv::SI_MATERIAL_TRANSFER };
static R_TXT2D_TRANSFER: Routine = Routine { name: "SI_txt2dTransfer", addr: si_scene::matconv::SI_TXT2D_TRANSFER };
static R_UDF_COPY: Routine = Routine { name: "SI_udfCopy", addr: si_scene::matconv::SI_UDF_COPY };
static R_ED_ADD_UDF: Routine = Routine { name: "SI_edAddUDF", addr: si_scene::matconv::SI_ED_ADD_UDF };
/// … and of its third part.
static R_CHANGE_NAME: Routine = Routine { name: "SI_elemdictChangeName", addr: si_scene::matconv::SI_ELEMDICT_CHANGE_NAME };
static R_SOFT_ELEM_CHANGE_NAME: Routine = Routine { name: "SI_edSoftElemChangeName", addr: si_scene::matconv::SI_ED_SOFT_ELEM_CHANGE_NAME };
static R_USER_DATA_COPY: Routine = Routine { name: "SI_userDataCopyFromSetup", addr: si_scene::matconv::SI_USER_DATA_COPY_FROM_SETUP };
static R_ATTACH_ARRAY: Routine = Routine { name: "SI_udfAttachArrayElements", addr: si_scene::edtools::SI_UDF_ATTACH_ARRAY_ELEMENTS };
/// … and of its sixth part (the copies and the routines outside `edtools.c` that the
/// textures' elements and the disconnection call).
static TEXTURE_CALLEES: &[&Routine] = {
    use si_scene::edtools as e;
    &[
        &Routine { name: "SI_txt3dTransfer", addr: si_scene::matconv::SI_TXT3D_TRANSFER },
        &Routine { name: "SI_texture2DVerifyPicturePath", addr: e::SI_TEXTURE2D_VERIFY_PICTURE_PATH },
        &Routine { name: "SI_materialCopy", addr: e::SI_MATERIAL_COPY },
        &Routine { name: "SI_txt3dCopy", addr: e::SI_TXT3D_COPY },
        &Routine { name: "SI_txt2dCopy", addr: e::SI_TXT2D_COPY },
        &Routine { name: "SI_txt2dSetWrap", addr: e::SI_TXT2D_SET_WRAP },
        &Routine { name: "SI_graflistAddData", addr: si_scene::wave::SI_GRAFLIST_ADD_DATA },
        &Routine { name: "SI_copyT2DShader", addr: e::SI_COPY_T2D_SHADER },
        &Routine { name: "SI_copyT3DShader", addr: e::SI_COPY_T3D_SHADER },
        &Routine { name: "SI_texture2DLoadFcurveAll", addr: e::SI_TEXTURE2D_LOAD_FCURVE_ALL },
        &Routine { name: "SI_texture3DLoadFcurveAll", addr: e::SI_TEXTURE3D_LOAD_FCURVE_ALL },
        &Routine { name: "SI_elemdictDisconnectANDConnectRel", addr: e::SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL },
        &Routine { name: "SI_elemdictCopyElemAnim", addr: e::SI_ELEMDICT_COPY_ELEM_ANIM },
    ]
};

/// … and of its seventh part (the picture search's callees, outside `t2d_dial.c`, and the
/// 3D texture's turbulence).
static PICTURE_CALLEES: &[&Routine] = {
    use si_scene::t2dpath as t;
    &[
        &Routine { name: "SI_dbCheckFileExistence", addr: t::SI_DB_CHECK_FILE_EXISTENCE },
        &Routine { name: "SI_dbfilenameRemovePath", addr: t::SI_DBFILENAME_REMOVE_PATH },
        &Routine { name: "SI_dbScnGetListNode", addr: t::SI_DB_SCN_GET_LIST_NODE },
        &Routine { name: "SI_dbListNodeFree", addr: t::SI_DB_LIST_NODE_FREE },
        &Routine { name: "SI_dbSetChapter", addr: t::SI_DB_SET_CHAPTER },
        &Routine { name: "SI_dbfilepathBuild", addr: t::SI_DBFILEPATH_BUILD },
        &Routine { name: "SI_dbChapterGetSubDirs", addr: t::SI_DB_CHAPTER_GET_SUB_DIRS },
        &Routine { name: "SI_updTxt3dTurb", addr: si_scene::matconv::SI_UPD_TXT3D_TURB },
    ]
};
/// The callees of the seventh part whose first argument is a text the stand-in logged
/// (`&strN`, an output of the case).
static TEXT_FIRST: &[u32] = {
    use si_scene::t2dpath as t;
    &[t::SI_DB_CHECK_FILE_EXISTENCE, t::SI_DBFILENAME_REMOVE_PATH, t::SI_DB_SCN_GET_LIST_NODE, t::SI_STATUS_DISPLAY_MESSAGE]
};

impl Recorded<'_> {
    /// Holds the text at `got`, the first argument of the call just answered, against the
    /// text the original's stand-in logged for it, when it logged one (`&strN` naming no
    /// buffer of the case).
    fn text_arg(&mut self, m: &Mem, name: &str, got: u32) {
        let n = self.next.wrapping_sub(1);
        let Some(want) = self.case.calls.get(n) else { return };
        if want.name != name {
            return;
        }
        let Some(Word::Ref { name: s, offset: 0 }) = want.args.first() else { return };
        if self.names.addr(m, s).is_some() {
            return;
        }
        let Some(text) = self.case.try_output(s) else { return };
        let got = if mutate_calls() { got ^ 1 } else { got };
        if m.cstr(got) != text.u8s() {
            let (a, b) = (String::from_utf8_lossy(&m.cstr(got)).into_owned(), String::from_utf8_lossy(&text.u8s()).into_owned());
            self.wrong.get_or_insert_with(|| format!("call {n} {name}: text {a:?} in the port, {b:?} in the original"));
        }
    }
}

/// … and of its fourth part (the custom effects' routines).
static CUSTOM_CALLEES: &[&Routine] = {
    use si_scene::custom as c;
    &[
        &Routine { name: "shaderLoad", addr: c::SHADER_LOAD },
        &Routine { name: "si_customValueListCopy", addr: c::SI_CUSTOM_VALUE_LIST_COPY },
        &Routine { name: "SI_cusParseDataString", addr: c::SI_CUS_PARSE_DATA_STRING },
        &Routine { name: "SI_statusDisplayMessage", addr: c::SI_STATUS_DISPLAY_MESSAGE },
        &Routine { name: "SI_cusDialogSet", addr: c::SI_CUS_DIALOG_SET },
        &Routine { name: "expandSelectArrays", addr: c::EXPAND_SELECT_ARRAYS },
        &Routine { name: "collapseSelectArrays", addr: c::COLLAPSE_SELECT_ARRAYS },
        &Routine { name: "cusDialogGet", addr: c::CUS_DIALOG_GET },
    ]
};

/// … and of its tenth part (the conversion's callees outside `edtools.c`'s look-ups and
/// relations and `scn2db.c`).
static CONV_CALLEES: &[&Routine] = {
    use si_scene::matconv as c;
    &[
        &Routine { name: "SI_schemComputeElemBBox", addr: c::SI_SCHEM_COMPUTE_ELEM_BBOX },
        &Routine { name: "SI_edAddMaterial", addr: c::SI_ED_ADD_MATERIAL },
        &Routine { name: "SI_edDisconnectSharedMaterialOr3DTxt", addr: c::SI_ED_DISCONNECT_SHARED_MATERIAL_OR3D_TXT },
    ]
};

/// … and of its fifth part (`graphsort.c`'s own routines that the operations on a
/// hierarchy call).
static SCHEM_CALLEES: &[&Routine] = {
    use si_view::outside as v;
    &[
        &Routine { name: "si_schemElemOper", addr: v::SI_SCHEM_ELEM_OPER.addr },
        &Routine { name: "schemUpdateHierarchy", addr: v::SCHEM_UPDATE_HIERARCHY.addr },
        &Routine { name: "schemCollapseNslElem", addr: v::SCHEM_COLLAPSE_NSL_ELEM.addr },
        &Routine { name: "SI_schemComputeHierarchiesBBox", addr: v::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX.addr },
        // the ninth part's
        &Routine { name: "SI_elemdictCreateDefaultDsc", addr: v::SI_ELEMDICT_CREATE_DEFAULT_DSC.addr },
    ]
};

/// The callees of `SI_scnRelationsFromElemDict` and the three disposals
/// (`emu_gen_scnrel.py`), as their recorded stand-ins are named.
macro_rules! routines {
    ($($id:ident = $name:literal, $addr:expr;)*) => {
        $(static $id: Routine = Routine { name: $name, addr: $addr };)*
        static SCNREL_CALLEES: &[&Routine] = &[$(&$id),*];
    };
}
routines! {
    R_NSEL_ALLOCATE = "SI_nselectionAllocate", si_scene::db2scn::SI_NSELECTION_ALLOCATE;
    R_MATERIAL_DISPOSE = "SI_materialDispose", si_scene::db2scn::SI_MATERIAL_DISPOSE;
    R_TXT2D_DISPOSE = "SI_txt2dDispose", si_scene::db2scn::SI_TXT2D_DISPOSE;
    R_TXT3D_FREE = "SI_txt3dFree", si_scene::db2scn::SI_TXT3D_FREE;
    R_FITTING = "SI_fittingAllocate", si_scene::db2scn::SI_FITTING_ALLOCATE;
    R_PFITTING = "SI_pfittingAllocate", si_scene::db2scn::SI_PFITTING_ALLOCATE;
    R_WAVE_ADD_MDL = "SI_waveAddMdl", si_scene::db2scn::SI_WAVE_ADD_MDL;
    R_DYN_CONS_TYPE = "si_relGetDynConsType", si_scene::db2scn::SI_REL_GET_DYN_CONS_TYPE;
    R_FIND_PRIORITY = "SI_conslistFindPriority", si_scene::db2scn::SI_CONSLIST_FIND_PRIORITY;
    R_CONSTR_ALLOCATE = "SI_conslistConstrAllocate", si_scene::db2scn::SI_CONSLIST_CONSTR_ALLOCATE;
    R_ADD_CONSTRAINT = "SI_conslistAddConstraint", si_scene::db2scn::SI_CONSLIST_ADD_CONSTRAINT;
    R_CUS_CREATE = "SI_cusCreateRelations", si_scene::db2scn::SI_CUS_CREATE_RELATIONS;
    R_REMOVE_TRIANGLE = "SI_removeTriangle", si_scene::dispose::SI_REMOVE_TRIANGLE;
    R_FIND_GL_DATA = "SI_findGlData", si_scene::dispose::SI_FIND_GL_DATA;
    R_ED_DISPOSE_T2D = "SI_edDisposeTexture2D", si_scene::dispose::SI_ED_DISPOSE_TEXTURE2D;
    R_REMOVE_GRAFNODE = "SI_graflistRemoveGrafnode", si_scene::dispose::SI_GRAFLIST_REMOVE_GRAFNODE;
    R_T3D_STILL_USED = "SI_texture3DStillUsed", si_scene::dispose::SI_TEXTURE3D_STILL_USED;
    R_ED_DISPOSE_T3D = "SI_edDisposeTexture3D", si_scene::dispose::SI_ED_DISPOSE_TEXTURE3D;
    R_RESET_MATERIAL = "SI_modelResetMaterial", si_scene::dispose::SI_MODEL_RESET_MATERIAL;
    R_RESET_TEXTURE = "SI_modelResetTexture", si_scene::dispose::SI_MODEL_RESET_TEXTURE;
    R_RESET_TEXTURE3D = "SI_modelResetTexture3D", si_scene::dispose::SI_MODEL_RESET_TEXTURE3D;
    R_UDF_DISPOSE = "SI_udfDispose", si_scene::dispose::SI_UDF_DISPOSE;
    R_USER_DATA_DISPOSE = "SI_userDataDispose", si_scene::dispose::SI_USER_DATA_DISPOSE;
    R_GLOBAL2D_MAP_REMOVE = "SI_global2dMapRemove", si_scene::dispose::SI_GLOBAL2D_MAP_REMOVE;
    R_CROP_VIEW_FREE = "si_cropViewFree", si_scene::dispose::SI_CROP_VIEW_FREE;
}

/// The callees of `SI_waveAddMdl` (`emu_gen_wavemdl.py`), as their recorded stand-ins are
/// named (the three matrix routines ran as they are: the port calls si-math's).
static WAVE_CALLEES: &[&Routine] = {
    use si_scene::wave_mdl as w;
    &[
        &Routine { name: "SI_edModelLinkWave", addr: w::SI_ED_MODEL_LINK_WAVE },
        &Routine { name: "SI_modelUpdateMatrix", addr: w::SI_MODEL_UPDATE_MATRIX },
        &Routine { name: "SI_faceCopy", addr: w::SI_FACE_COPY },
        &Routine { name: "SI_patchCopy", addr: w::SI_PATCH_COPY },
        &Routine { name: "SI_meshCopy", addr: w::SI_MESH_COPY },
        &Routine { name: "SI_nurbsSrfCopy", addr: w::SI_NURBS_SRF_COPY },
        &Routine { name: "SI_modelRoot", addr: w::SI_MODEL_ROOT },
        &Routine { name: "SI_modelUpdateDependency", addr: w::SI_MODEL_UPDATE_DEPENDENCY },
    ]
};

/// The callees of the wave updates (`emu_gen_wave.py`), as their recorded stand-ins are named
/// (`si_waveSplineUpdate`, the helpers of `wave_util.c`, `SI_edGetElemFromWave` and
/// `SI_modelState` ran as they are: the port calls its own).
static WAVE_UPDATE_CALLEES: &[&Routine] = {
    use si_scene::{wave_sys as s, wave_update as u};
    &[
        &Routine { name: "SI_elemdictGetElemFromData", addr: si_scene::edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA },
        &Routine { name: "SI_edFindFcv", addr: u::SI_ED_FIND_FCV },
        &Routine { name: "SI_fcurveValue", addr: u::SI_FCURVE_VALUE },
        &Routine { name: "si_waveGeomUpdate", addr: s::SI_WAVE_GEOM_UPDATE },
        &Routine { name: "SI_patchComputeSurface", addr: s::SI_PATCH_COMPUTE_SURFACE },
        &Routine { name: "SI_meshComputesPolygonNormal", addr: s::SI_MESH_COMPUTES_POLYGON_NORMAL },
        &Routine { name: "SI_meshComputesVertexNormal", addr: s::SI_MESH_COMPUTES_VERTEX_NORMAL },
        &Routine { name: "SI_splineTouch", addr: s::SI_SPLINE_TOUCH },
    ]
};

/// The callees of `si_waveGeomUpdate` that `emu_gen_wave.py`'s `GeomSession` replaces by
/// stand-ins with effects ([`geom_effect`] repeats those on the image; the copies are
/// [`WAVE_CALLEES`]'); `SI_calloc` and the helpers ran as they are.
static GEOM_CALLEES: &[&Routine] = {
    use si_scene::wave_geom as g;
    &[
        &Routine { name: "SI_edGetElemFromModel", addr: g::SI_ED_GET_ELEM_FROM_MODEL },
        &Routine { name: "SI_elemGetNbVertices", addr: g::SI_ELEM_GET_NB_VERTICES },
        &Routine { name: "SI_elemGetVertex", addr: g::SI_ELEM_GET_VERTEX },
        &Routine { name: "SI_elemSetVertex", addr: g::SI_ELEM_SET_VERTEX },
        &Routine { name: "SI_computeMeshVertexNormal", addr: g::SI_COMPUTE_MESH_VERTEX_NORMAL },
        &Routine { name: "SI_patchValueAndNormal", addr: g::SI_PATCH_VALUE_AND_NORMAL },
        &Routine { name: "SI_nurbsSrfValueAndNormal", addr: g::SI_NURBS_SRF_VALUE_AND_NORMAL },
        &Routine { name: "SI_patchDispose", addr: g::SI_PATCH_DISPOSE },
        &Routine { name: "SI_nurbsSrfDispose", addr: g::SI_NURBS_SRF_DISPOSE },
    ]
};

/// The made-up effects of the stand-ins of `GeomSession` (see `emu_gen_wave.py`), on the
/// port's own pointers: the element's points (+4) read and written (+8), the mesh buffer's
/// normals, a patch's and a NURBS surface's point and normal made of (u, v).
fn geom_effect(m: &mut Mem, f: u32, a: &[u32]) {
    use si_scene::wave_geom as g;
    let w = |k: usize| a.get(k).copied().unwrap_or(0);
    let copy12 = |m: &mut Mem, from: u32, to: u32| {
        for k in 0..3u32 {
            let v = m.u32(from.wrapping_add(4 * k));
            m.set_u32(to.wrapping_add(4 * k), v);
        }
    };
    match f {
        g::SI_ELEM_GET_VERTEX => {
            let from = m.u32(w(1).wrapping_add(4)).wrapping_add(w(5).wrapping_mul(12));
            copy12(m, from, w(6));
        }
        g::SI_ELEM_SET_VERTEX => {
            let to = m.u32(w(1).wrapping_add(8)).wrapping_add(w(5).wrapping_mul(12));
            copy12(m, w(6), to);
        }
        g::SI_COMPUTE_MESH_VERTEX_NORMAL => copy12(m, w(1).wrapping_add(w(2).wrapping_mul(12)), w(0)),
        g::SI_PATCH_VALUE_AND_NORMAL => {
            let (u, v) = (f32::from_bits(w(1)) as f64, f32::from_bits(w(2)) as f64);
            for (k, x) in [u, v, 0.0, u * v, v - u, 0.0].into_iter().enumerate() {
                let at = if k < 3 { w(3) } else { w(4) };
                m.set_f32(at + 4 * (k as u32 % 3), x as f32);
            }
        }
        g::SI_NURBS_SRF_VALUE_AND_NORMAL => {
            let d = |lo: u32, hi: u32| f64::from_bits(lo as u64 | (hi as u64) << 32);
            let (u, v) = (d(w(2), w(3)), d(w(4), w(5)));
            for (k, x) in [u, v, u + v, u * v, v - u, 0.0].into_iter().enumerate() {
                let at = if k < 3 { w(0) } else { w(6) };
                m.set_f64(at + 8 * (k as u32 % 3), x);
            }
        }
        _ => {}
    }
}

impl si_scene::wave_update::WaveOutside for Recorded<'_> {
    /// The recorded call's answer in the register; the call itself is held against the
    /// recorded one as any other.
    fn call_f80(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> si_x87::F80 {
        let routine = WAVE_UPDATE_CALLEES.iter().copied().find(|r| r.addr == f).expect("a callee these vectors record");
        self.frees(m);
        self.skip_libm(Some(routine.name));
        let answer = self.case.calls.get(self.next).filter(|c| c.name == routine.name).map(|c| {
            // a call answered in the register is also on the mathematics' tape
            // (`Case::tape`): it is passed there as it is answered here
            let _ = si_x87::trig::replay::next(&c.name, &c.arg_values());
            c.result_f80()
        });
        Outside::call(self, m, routine, args);
        answer.unwrap_or(si_x87::F80::ZERO)
    }
}

impl si_scene::edtools::ByAddr for Recorded<'_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        let a = |k: usize| args.get(k).copied().unwrap_or(0);
        if let Some(r) = dictrest::call(self, m, f, args) {
            return r;
        }
        if f == si_scene::db2scn::SI_ELEMDICT_GET_ELEM {
            // not a stand-in in these vectors: the routine ran as it is (si-dict's port)
            return si_dict::elemdict::get_elem(m, si_mem::Ptr32::new(a(0)), a(1) as i32, a(2) as i32).addr();
        }
        {
            // the look-ups of the dictionary that the vectors of emu_gen_edlook.py ran as
            // they are (si-dict's ports)
            use si_mem::Ptr32 as P;
            use si_scene::edtools as ed;
            match f {
                // a recorded stand-in in the vectors of emu_gen_wave.py
                ed::SI_ELEMDICT_GET_ELEM_FROM_DATA if self.case.calls.iter().any(|c| c.name == "SI_elemdictGetElemFromData") => {
                    return Outside::call(self, m, WAVE_UPDATE_CALLEES[0], args)
                }
                ed::SI_ELEMDICT_GET_ELEM_FROM_DATA => return si_dict::elemmisc::get_elem_from_data(m, self, P::new(a(0)), a(1) as i32, P::new(a(2)), P::new(a(3))).addr(),
                ed::SI_ELEMDICT_FIND_ELEM_BY_DATA => return si_dict::elemdict::find_elem_by_data(m, self, P::new(a(0)), a(1) as i32, P::new(a(2))) as u32,
                ed::SI_ELEMDICT_FIND_RELATED_ELEM => {
                    return si_dict::elemrel::find_related_elem(m, P::new(a(0)), P::new(a(1)), a(2) as i16, a(3) as i32, a(4) as i32, a(5) as i32, P::new(a(6)), P::new(a(7))).addr()
                }
                ed::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM => {
                    return si_dict::elemrel::find_next_related_elem(m, P::new(a(0)), P::new(a(1)), a(2) as i16, a(3) as i32, a(4) as i32, a(5) as i32, P::new(a(6)), P::new(a(7))).addr()
                }
                ed::SI_ELEMDICT_GET_CUR_RELATION => return si_dict::elemrel::get_cur_relation(m, P::new(a(0)), P::new(a(1)), a(2) as i16, a(3) as i32).addr(),
                ed::SI_ACTIVATION_FCV_CHECK_TIME => return Outside::call(self, m, &R_ACTIVATION, args),
                // and those of emu_gen_edmat.py (the schematic's operation a stand-in)
                ed::SI_ELEMDICT_ADD_RELATION => {
                    return si_dict::elemrel::add_relation(m, P::new(a(0)), P::new(a(1)), a(2) as i32, a(3) as i32, a(4) as i32, a(5) as i32, a(6) as i32) as u32
                }
                ed::SI_ELEMDICT_REMOVE_RELATION_PAIR => {
                    return si_dict::elemrel::remove_relation_pair(m, self, P::new(a(0)), P::new(a(1)), a(2) as i32, a(3) as i32, a(4) as i32, a(5) as i32, a(6) as i32) as u32
                }
                ed::SI_SCHEMATIC_HIER_OPERATION => return Outside::call(self, m, &R_SCHEMATIC_HIER_OPERATION, args),
                // the second half's: the name records and the element added ran as they are
                ed::SI_ELEMDICT_GET_NEW_NAME => return si_dict::elemname::get_new_name(m, self, P::new(a(0)), a(1) as i32, P::new(a(2))).addr(),
                ed::SI_DB_LIST_NODE_ALLOCATE => return si_scene::dbnode::list_node_allocate(m, self).addr(),
                ed::SI_DB_FORMAT_FULL_NAME => return si_scene::dbnode::format_full_name(m, P::new(a(0))).addr(),
                ed::SI_ELEMDICT_ADD_ELEM => {
                    return si_dict::elemdict::add_elem(m, self, P::new(a(0)), a(1) as i32, P::new(a(2)), P::new(a(3)), P::new(a(4)), a(5) as i32, a(6) as i32) as u32
                }
                ed::SI_ATTDICT_SET_FCURVE_FLAG => return Outside::call(self, m, &R_SET_FCURVE_FLAG, args),
                x if x == R_MATERIAL_TRANSFER.addr => return Outside::call(self, m, &R_MATERIAL_TRANSFER, args),
                x if x == R_TXT2D_TRANSFER.addr => return Outside::call(self, m, &R_TXT2D_TRANSFER, args),
                x if x == R_UDF_COPY.addr => return Outside::call(self, m, &R_UDF_COPY, args),
                x if x == R_ED_ADD_UDF.addr => return Outside::call(self, m, &R_ED_ADD_UDF, args),
                // the third part's: the relation's routines ran as they are
                ed::SI_ELEMDICT_GET_RELATION_FROM_IDS => {
                    return si_dict::elemrel::get_relation_from_ids(m, P::new(a(0)), P::new(a(1)), a(2) as i32, a(3) as i32, a(4) as i32, a(5) as i32).addr()
                }
                ed::SI_ELEMDICT_ADD_UN_COMPLETE_RELATION => {
                    let (sc, dc, src, dst, relid, rule, pos) = (a(2) as i32, a(3) as i32, a(4) as i32, a(5) as i32, a(6) as i32, a(7) as i32, a(8) as i32);
                    return si_dict::elemrel::add_uncomplete_relation(m, P::new(a(0)), P::new(a(1)), sc, dc, src, dst, relid, rule, pos, P::new(a(9)), a(10) as i16) as u32;
                }
                ed::SI_ELEMDICT_SET_DIRTY => {
                    si_dict::elemdict::set_dirty(m, P::new(a(0)), a(1) as i32);
                    return 0;
                }
                x if x == R_CHANGE_NAME.addr => return Outside::call(self, m, &R_CHANGE_NAME, args),
                x if x == R_SOFT_ELEM_CHANGE_NAME.addr => return Outside::call(self, m, &R_SOFT_ELEM_CHANGE_NAME, args),
                x if x == R_USER_DATA_COPY.addr => return Outside::call(self, m, &R_USER_DATA_COPY, args),
                x if x == R_ATTACH_ARRAY.addr => return Outside::call(self, m, &R_ATTACH_ARRAY, args),
                // the tenth part's: SI_elemdictRelationsCount ran as it is
                si_scene::matconv::SI_ELEMDICT_RELATIONS_COUNT => {
                    return si_dict::elemrel::relations_count(m, P::new(a(0)), P::new(a(1)), a(2) as i16, a(3) as i32, a(4) as i32, a(5) as i32, a(6) as i32) as u32
                }
                // the eleventh part's: what si_elemCheckRestrictedSave calls ran as it is
                si_scene::dictout::SI_ELEMDICT_FIND_SERIAL_RELATION => {
                    return si_dict::elemrel::find_serial_relation(m, P::new(a(0)), P::new(a(1)), a(2) as i32, P::new(a(3)), a(4) as i32, a(5) as i16, a(6) as i16).addr()
                }
                si_scene::dictout::SI_ELEMDICT_GET_CUR_RELATION_INDEX => return si_dict::elemmisc::get_cur_relation_index(m, P::new(a(0)), a(1) as i16, a(2) as i32) as u32,
                si_scene::dictout::SI_ELEMDICT_SET_CUR_RELATION_INDEX => {
                    si_dict::elemmisc::set_cur_relation_index(m, P::new(a(0)), a(1) as i16, a(2) as i32, a(3) as i32);
                    return 0;
                }
                si_scene::dictout::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER => return si_scene::dbvsfcv::rel_get_anim_f_rel_from_chapter(m, a(0) as i32) as u32,
                // the ninth part's: SI_edGetElemFromScene ran as it is
                x if x == si_view::outside::SI_ED_GET_ELEM_FROM_SCENE.addr => return si_scene::edtools::ed_get_elem_from_scene(self, m, P::new(a(0))).addr(),
                // the seventh part's: texture2DReplacePicPath ran as it is
                si_scene::t2dpath::TEXTURE2D_REPLACE_PIC_PATH => {
                    si_scene::t2dpath::texture2d_replace_pic_path(m, a(0), a(1));
                    return 0;
                }
                _ => {}
            }
        }
        if let Some(r) = edname::call(self, m, f, args) {
            return r;
        }
        if let Some(r) = edfirst::call(self, m, f, args) {
            return r;
        }
        if let Some(routine) = GEOM_CALLEES.iter().copied().find(|r| r.addr == f) {
            use si_scene::wave_geom as g;
            if f == g::SI_PATCH_DISPOSE || f == g::SI_NURBS_SRF_DISPOSE {
                // its stand-in logged the copy the caller's local holds
                let held = m.u32(a(0));
                return Outside::call(self, m, routine, &[held]);
            }
            let answer = Outside::call(self, m, routine, args);
            geom_effect(m, f, args);
            return answer;
        }
        if f == R_TXT2D_DISPOSE.addr {
            // its stand-in logged the texture the caller's local holds in place of the
            // local's address
            let tex = m.u32(a(1));
            return Outside::call(self, m, &R_TXT2D_DISPOSE, &[a(0), tex]);
        }
        let routine = [&R_FOPEN, &R_FCLOSE, &R_FDMODEL, &R_UPDATE_SON_STATE]
            .into_iter()
            .chain(SCNREL_CALLEES.iter().copied())
            .chain(CUSTOM_CALLEES.iter().copied())
            .chain(SCHEM_CALLEES.iter().copied())
            .chain(TEXTURE_CALLEES.iter().copied())
            .chain(PICTURE_CALLEES.iter().copied())
            .chain(CONV_CALLEES.iter().copied())
            .chain(WAVE_CALLEES.iter().copied())
            .chain(WAVE_UPDATE_CALLEES.iter().copied())
            .find(|r| r.addr == f)
            .expect("a callee these vectors record");
        let answer = Outside::call(self, m, routine, args);
        if TEXT_FIRST.contains(&f) {
            self.text_arg(m, routine.name, a(0));
        }
        answer
    }
}

/// The recorded calls as `si-view` calls out: by address, as this crate's routines do
/// (the dictionary's routines run as they are, the rest are the recorded stand-ins).
pub struct ViewSide<'a, 'b>(pub &'a mut Recorded<'b>);
impl si_view::Outside for ViewSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, routine: &'static si_view::Routine, args: &[u32]) -> u32 {
        si_scene::edtools::ByAddr::call_addr(self.0, m, routine.addr, args)
    }
}

/// The recorded calls as the readers of `si-hrc` call out: the streams are the recorded
/// `fopen` and `fclose`, `SI_fdmodelBinaryRead` stands in for the port.
pub struct HrcSide<'a, 'b>(pub &'a mut Recorded<'b>);
impl si_hrc::image::Outside for HrcSide<'_, '_> {
    fn fopen(&mut self, m: &mut Mem, _path: u32, _mode: u32) -> u32 {
        Outside::call(self.0, m, &R_FOPEN, &[])
    }
    fn fclose(&mut self, m: &mut Mem, file: u32) -> i32 {
        Outside::call(self.0, m, &R_FCLOSE, &[file]) as i32
    }
    fn fread(&mut self, _m: &mut Mem, _ptr: u32, _size: u32, _n: u32, _file: u32) -> u32 {
        0
    }
    fn fgetc(&mut self, _m: &mut Mem, _file: u32) -> i32 {
        -1
    }
    fn rewind(&mut self, _m: &mut Mem, _file: u32) {}
    fn bytes_left(&mut self, _m: &mut Mem, _file: u32) -> u64 {
        0
    }
    fn call(&mut self, m: &mut Mem, routine: &'static si_hrc::image::Routine, args: &[u32]) -> u32 {
        si_scene::edtools::ByAddr::call_addr(self.0, m, routine.addr, args)
    }
    fn stands_in(&mut self, routine: &'static si_hrc::image::Routine) -> bool {
        routine.addr == R_FDMODEL.addr
    }
}

/// What the routines of `emu_gen_read.py` need in the image: the two literals of
/// `SI_omodelBinaryRead` beside the usual globals.
pub fn prepare_read(m: &mut Mem) {
    prepare_default(m);
    m.put_cstr(si_scene::image::S_HRC_FORMAT, b"%s.hrc");
    m.put_cstr(si_scene::image::S_RB_HRC, b"rb");
}
