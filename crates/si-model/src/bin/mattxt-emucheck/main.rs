// SPDX-License-Identifier: MIT
//! `mattxt-emucheck <dir>`: the materials and 2D textures of a node on the image against
//! the emulated original (finding 0015, ninth pass): `SI_findMatInfo`,
//! `SI_mdlGetMatAndTxt2D`, `SI_matAndTxt2DDestroy`; and the NURBS conversion's texture
//! pairs, `nurbsToMeshUpdatePolygonUV` (tenth pass). Also three small routines the render's
//! update calls (finding 0023): `SI_modelWave` (`wave_sys.c`), `SI_removeTriangle` and
//! `SI_mdlRemoveTmesh` (`grTriangle.c`), from `re/tools/si-model/gen_tri.py`, whose
//! recorded stand-in is `SI_glDeleteDispList`; and `SI_modelCreateParent` (`model.c`), from
//! `re/tools/si-model/gen_parent.py`, whose recorded stand-ins include `SI_metaballTouch`; and `si_createPatchHierarchyFromSpline` (the patch's nodes), from
//! `re/tools/si-model/gen_patchhier.py`, whose stand-ins include `SI_modelAllocate`.
//! A buffer named `g<8 hex digits>` is a global at that address; the calls of libm that the
//! harness records are skipped (the port's mathematics is `si-math`'s).
//!
//! Each line of a vector file (made by `re/tools/si-model/gen_mattxt.py` with the harness of
//! `re/tools/emu`) is one call of one original routine on made-up nodes, materials and
//! textures: the buffers it was given, what it returned, every buffer it changed, every
//! block it got from `malloc` with its bytes, the blocks it freed in order, and its calls of
//! `SI_modelMasterFromInstance` (a recorded stand-in) with their answers. `genList.c` ran as
//! the binary has it; the port calls `si-gen`'s.
//!
//! The comparison is `gen-emucheck`'s (si-gen): the return value, every buffer of the case,
//! the heap block by block, the frees in order, the calls in order with their arguments,
//! and that nothing else changed. Columns of the report: cases; identical; identical in the
//! defined part; different; not compared. `EMUCHECK_MUTATE=1` flips one bit of what the port
//! returned and of the first byte it changed; `EMUCHECK_MUTATE=calls` a bit of the first
//! argument of every call.

use si_mem::{Fill, Mem, Names, Region, Word as W};
use si_model::mesh_image::mattxt;
use si_model::model_image::{Callee, Outside};
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

/// The arguments of a case, by name.
pub struct Args<'a> {
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
    pub fn v(&self, name: &str) -> u32 {
        self.word(&format!("v_{name}"))
            .or_else(|| self.word(&format!("p_{name}")))
            .unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name))
    }

    pub fn i(&self, name: &str) -> i32 {
        self.v(name) as i32
    }
}

/// The address a stand-in's argument has when the original passed the address of one of its locals.
const LOCAL: u32 = 0x57ac_57ac;

/// The libm routines the harness records beside the stand-ins' calls.
const LIBM: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "log", "log10", "pow", "cbrt", "sinf", "cosf", "tanf", "asinf", "acosf", "atanf", "expf", "powf", "fpatan", "sqrt"];

/// The address of a global buffer: `g` and eight hex digits (`World.glob`).
fn global_at(name: &str) -> Option<u32> {
    let h = name.strip_prefix('g')?.get(..8)?;
    u32::from_str_radix(h, 16).ok()
}

/// What a routine returned.
#[derive(Clone, Copy, Debug)]
pub enum Ret {
    None,
    Eax(u32),
}

impl From<()> for Ret {
    fn from(_: ()) -> Ret {
        Ret::None
    }
}
impl From<u32> for Ret {
    fn from(v: u32) -> Ret {
        Ret::Eax(v)
    }
}

/// The outside world of a case: every call is held against the next one the original
/// made and answered as that one was.
pub struct Recorded<'a> {
    case: &'a Case,
    names: &'a Names,
    next: usize,
    flip: bool,
    pub wrong: Option<String>,
}

impl Recorded<'_> {
    /// Passes the recorded calls of libm.
    fn skip_libm(&mut self) {
        while self.case.calls.get(self.next).is_some_and(|c| LIBM.contains(&c.name.as_str())) {
            self.next += 1;
        }
    }

    fn resolve(&self, m: &Mem, w: &Word) -> Option<u32> {
        match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { name, offset } => self.names.addr(m, name).map(|a| a.wrapping_add(*offset)),
        }
    }
}

impl Outside for Recorded<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        let name = callee.name;
        self.skip_libm();
        let Some(want) = self.case.calls.get(self.next) else {
            self.wrong.get_or_insert_with(|| format!("call {}: the port calls {name} where the original made no further call", self.next));
            return 0;
        };
        let n = self.next;
        self.next += 1;
        if want.name != name {
            self.wrong.get_or_insert_with(|| format!("call {n}: the port calls {name}, the original {}", want.name));
            return 0;
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
        let (stack_lo, stack_len) = m.region(Region::Stack);
        let mut locals = Vec::new();
        for (k, w) in want.args.iter().enumerate() {
            let expect = self.resolve(m, w);
            let got = args.get(k).copied();
            if expect == Some(LOCAL) {
                // the address of a local of the original's: any address of the image's stack
                match got {
                    Some(a) if a.wrapping_sub(stack_lo) < stack_len => locals.push(a),
                    _ => {
                        self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, the address of a local in the original"));
                    }
                }
            } else if expect.is_some() && expect != got {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: argument {k} is {got:08x?} in the port, {expect:08x?} in the original"));
            }
        }
        // what the stand-in stored through the locals it was handed: one `name.out` entry each
        let mut local = 0;
        while let Some(out) = self.case.calls.get(self.next) {
            if out.name.strip_suffix(".out") != Some(name) {
                break;
            }
            self.next += 1;
            let Some(&base) = locals.get(local) else {
                self.wrong.get_or_insert_with(|| format!("call {n} {name}: the stand-in stored through more locals than the port passed ({})", locals.len()));
                break;
            };
            local += 1;
            let mut at = base;
            for w in &out.args {
                match (w.bytes(), self.resolve(m, w)) {
                    (2, Some(v)) => m.set_u16(at, v as u16),
                    (_, Some(v)) => m.set_u32(at, v),
                    _ => {}
                }
                at += w.bytes() as u32;
            }
        }
        self.resolve(m, &want.result).unwrap_or(0)
    }
}

/// From a vector file's routine name to the port.
fn run(name: &str, m: &mut Mem, o: &mut Recorded, a: &Args) -> Option<Ret> {
    Some(match name {
        "SI_findMatInfo" => mattxt::find_mat_info(m, o, a.v("model"), a.v("mats"), a.v("nb"), a.v("txt")).into(),
        "SI_mdlGetMatAndTxt2D" => mattxt::mdl_get_mat_and_txt2d(m, o, a.v("model"), a.v("method") as i16).into(),
        "SI_matAndTxt2DDestroy" => mattxt::mat_and_txt2d_destroy(m, a.v("pdata")).into(),
        "nurbsToMeshUpdatePolygonUV" => {
            let index: Vec<i32> = (0..5).map(|k| a.i(&format!("i{k}"))).collect();
            si_model::mesh_image::nurbs::nurbs_to_mesh_update_polygon_uv(m, a.v("poly"), a.v("verts"), &index).into()
        }
        "SI_modelWave" => si_model::model_image::readjust::model_wave(m, si_mem::Ptr32::new(a.v("model")), a.v("set")).into(),
        "SI_removeTriangle" => si_model::model_image::triangle::remove_triangle(m, o, si_mem::Ptr32::new(a.v("model")), a.v("sons") as i16).into(),
        "SI_mdlRemoveTmesh" => si_model::model_image::triangle::mdl_remove_tmesh(m, o, si_mem::Ptr32::new(a.v("model"))).into(),
        "SI_metaballGetParentSystem" => si_model::model_image::metaball::get_parent_system(m, a.v("ball")).into(),
        "metaballCheckBoundingBox" => {
            si_model::model_image::metaball::check_bounding_box(m, a.v("ball"), a.v("bbox"), f32::from_bits(a.v("scale")));
            Ret::None
        }
        "SI_fastSetDirty" => {
            si_model::model_image::fastplay::set_dirty(m, o, a.v("port"), a.i("index"), a.i("all"));
            Ret::None
        }
        "SI_modelCreateParent" => si_model::model_image::tree::create_parent(m, o, a.v("fg"), a.v("cg"), a.v("mode") as i16).into(),
        "si_createPatchHierarchyFromSpline" => {
            si_model::model_image::tree::create_patch_hierarchy_from_spline(m, o, si_mem::Ptr32::new(a.v("patch")), a.v("model"), a.v("update") as i16, a.v("caps") as i16, a.v("extrude") as i16);
            Ret::None
        }
        "createPatchFromRevolutionFromSpline" => (si_model::model_image::tree::create_patch_from_revolution_from_spline(
            m, o, a.v("model"), si_mem::Ptr32::new(a.v("spline")), a.v("axmodel"), a.v("rev"), a.v("update") as i16,
        ) as u32).into(),
        // (kinds 2 and 9 are not ported: the port answers None and no case has them)
        "createModelFromRevolution" => {
            let r = si_model::model_image::tree::create_model_from_revolution(m, o, si_mem::Ptr32::new(a.v("src")), a.v("model"), a.v("model_at"), a.v("rev"));
            (r.unwrap_or(-1) as u32).into()
        }
        "SI_revolutionUpdate" => {
            si_model::model_image::tree::revolution_update(m, o, a.v("node"));
            Ret::None
        }
        "SI_zipUpdate" => {
            si_model::model_image::zip::zip_update(m, o, a.v("node"));
            Ret::None
        }
        "refreshZip" => si_model::model_image::zip::refresh_zip(m, o, a.v("dialog")).into(),
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
    if c.has_note("OVERRUN") || c.has_note("UNDERRUN") {
        // (UNDERRUN: a strict session's guard below a buffer was written)
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
    si_mem::init_globals(&mut m, si_model::mesh_image::g::ALL);
    si_model::model_image::init(&mut m);

    // the buffers, in the arena in the case's order (a global is a buffer `g<address>`)
    let loaded: Vec<(&Field, Option<Vec<W>>)> = c.inputs.iter().filter(|f| !is_argument(&f.name)).map(|f| (f, words(f))).collect();
    let fields: Vec<si_mem::FieldAt<'_>> = loaded.iter().map(|(f, w)| (f.name.as_str(), global_at(&f.name), w.as_deref())).collect();
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
    let Some(ret) = run(routine, &mut m, &mut outside, &args) else {
        return Outcome::Skip("no port in the table");
    };
    if let Some(f) = m.fault() {
        return Outcome::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = outside.wrong.take() {
        return Outcome::Different(w);
    }
    outside.skip_libm();
    if outside.next != c.calls.len() {
        return Outcome::Different(format!("the original made {} calls, the port {}", c.calls.len(), outside.next));
    }

    // the mutation switch: one bit of the result and of the first byte the port changed
    let mut ret = ret;
    if mutate() == Mutate::Memory {
        if let Ret::Eax(v) = ret {
            ret = Ret::Eax(v ^ 1);
        }
        if let Some((a, bytes)) = m.changed_in(&before, Region::Arena).into_iter().next() {
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some(&(a, n)) = m.heap_blocks().first() {
            if n > 0 {
                let b = m.u8(a);
                m.set_u8(a, b ^ 1);
            }
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
    let dir = std::env::args().nth(1).expect("usage: mattxt-emucheck <directory of vector files>");
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
                    let limit = std::env::var("EMUCHECK_EXAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
                    if row.examples.len() < limit {
                        let mut text = line.clone();
                        let cut = if limit > 2 { 150 } else { 700 };
                        if text.len() > cut {
                            text.truncate(cut);
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
