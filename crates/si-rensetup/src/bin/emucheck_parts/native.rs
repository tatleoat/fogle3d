// SPDX-License-Identifier: MIT
//! Checks of the native hand-over on the memory image (`si_rensetup::native`; finding 0023,
//! section 17). The vectors are those of `re/tools/si-rensetup/emu_gen_native.py` and its
//! companions; `re/tools/si-rensetup/nat.py` describes a case.
//!
//! A case is loaded as it is: the buffers of the harness's test-buffer region through
//! `si_mem::load_case` (which gives each the address it had in the emulator), the globals
//! at their addresses. The port runs on that image. Every callee of another package is
//! answered from the case: the port must make the calls the original made, in order, with
//! the same arguments, and gets the original's answers and whatever the stand-in did to
//! memory. Then everything is compared: the result, every buffer and global of the case
//! (changed or not), every block `malloc` handed out with its address and size, the list
//! of blocks freed; and every byte the port changed must lie in one of those.
//!
//! With `EMUCHECK_MUTATE=1` one bit of the result and one bit of every buffer and global
//! is flipped after the port ran: every compared case must then differ.

use si_mem::{load_case, FnPtr, Mem, Names, Ptr32, Word as W};
use si_rensetup::native::outside::{self, Outside};
use si_rensetup::native::{classic, edtools, envtools, geometry, globals, instance, invkin, matter, model_b, pre_render, preview, prv_main, rens_main, renu_display, renu_filepath, renu_subregion, rnd_dial, skeleton, symmetry, symmetry_vector, sys_render};
use si_view::graflist;
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

/// `EMUCHECK_MUTATE=1`: one bit of every value the port leaves is flipped before the
/// comparison. `EMUCHECK_MUTATE=calls`: one bit of the first argument of every call the
/// port makes into another package is flipped instead (a call without arguments is
/// reported as a call of another routine); cases in which the port makes no call are then
/// not counted.
#[derive(Clone, Copy, PartialEq)]
enum Mutation {
    None,
    Values,
    Calls,
}

fn mutation() -> Mutation {
    match std::env::var("EMUCHECK_MUTATE").as_deref() {
        Err(_) => Mutation::None,
        Ok("calls") => Mutation::Calls,
        Ok(_) => Mutation::Values,
    }
}

fn conv(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn words(f: &Field) -> Option<Vec<W>> {
    f.words.as_ref().map(|w| w.iter().map(conv).collect())
}

/// The address a global's field name carries: `G` and eight hexadecimal digits.
fn global_addr(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('G')?;
    let hex = rest.get(..8)?;
    if rest.len() > 8 && !rest[8..].starts_with('_') {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// The emulator's stack and the image's are the same range; a local's address in it is not.
fn in_stack(a: u32) -> bool {
    let (base, size) = si_mem::STACK;
    a.wrapping_sub(base) < size
}

/// A case in the image.
pub struct Img {
    pub m: Mem,
    pub names: Names,
}

fn load(c: &Case) -> Result<Img, String> {
    let mut m = Mem::new();
    globals::init(&mut m);
    let arena: Vec<(&str, Option<Vec<W>>)> =
        c.inputs.iter().filter(|f| !f.name.starts_with("A_") && global_addr(&f.name).is_none()).map(|f| (f.name.as_str(), words(f))).collect();
    let fields: Vec<(&str, Option<&[W]>)> = arena.iter().map(|(n, w)| (*n, w.as_deref())).collect();
    let mut names = load_case(&mut m, &fields)?;
    let globals: Vec<(&str, u32, Vec<W>)> = c.inputs.iter().filter_map(|f| Some((f.name.as_str(), global_addr(&f.name)?, words(f)?))).collect();
    for (name, addr, w) in &globals {
        names.add(name, *addr, si_mem::words_len(w));
    }
    for (_, addr, w) in &globals {
        m.load_words(*addr, w, &names)?;
    }
    if let Some(f) = m.fault() {
        return Err(format!("loading the case faulted: {f:?}"));
    }
    Ok(Img { m, names })
}

/// The value of one word of the case: a number, or the address a buffer has here.
fn value(m: &Mem, names: &Names, w: &Word) -> u32 {
    match w {
        Word::Value { value, .. } => *value as u32,
        Word::Unwritten { .. } => 0,
        Word::Ref { name, offset } => names.addr(m, name).unwrap_or_else(|| panic!("the case names no buffer `{name}`")).wrapping_add(*offset),
    }
}

/// The arguments of a case, by name.
pub struct Args<'a> {
    c: &'a Case,
    values: Vec<(&'a str, u32)>,
}

impl Args<'_> {
    pub fn get(&self, name: &str) -> u32 {
        self.values.iter().find(|v| v.0 == name).unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.c.name)).1
    }

    pub fn i16(&self, name: &str) -> i16 {
        self.get(name) as i16
    }

    pub fn ptr<T>(&self, name: &str) -> Ptr32<T> {
        Ptr32::new(self.get(name))
    }
}

/// The outside world of a case: the calls the original made, replayed.
pub struct Replay<'a> {
    c: &'a Case,
    names: &'a Names,
    at: usize,
    /// How many callees of other packages the port has called.
    pub made: usize,
}

impl Replay<'_> {
    /// The mathematics library's calls are in the same list; they answer in the register
    /// and are replayed by `si_x87::trig`.
    fn skip_mathematics(&mut self) {
        while self.c.calls.get(self.at).is_some_and(|k| k.result.bytes() == 10) {
            self.at += 1;
        }
    }

    /// The calls of other packages the port did not make.
    fn left(&mut self) -> usize {
        self.c.calls[self.at..].iter().filter(|k| k.result.bytes() != 10 && !k.name.starts_with('!')).count()
    }
}

/// A stack address of the original in terms of the port's, through the locals the call was
/// handed: (the original's address, the port's).
fn translate(stack: &[(u32, u32)], a: u32) -> u32 {
    if !in_stack(a) {
        return a;
    }
    match stack.iter().filter(|p| p.0 <= a && a - p.0 < 0x1000).max_by_key(|p| p.0) {
        Some(p) => p.1 + (a - p.0),
        None => panic!("a stand-in used the stack address {a:#010x}, which its call was not handed"),
    }
}

impl Outside for Replay<'_> {
    fn call(&mut self, m: &mut Mem, f: FnPtr, args: &[u32]) -> u32 {
        self.skip_mathematics();
        let mut name = outside::name_of(f).or_else(|| renu_subregion::name_of(f)).or_else(|| (f == symmetry::SI_ELEMDICT_GET_CUR_RELATION).then_some("SI_elemdictGetCurRelation")).unwrap_or("(a routine that is not in the list of callees)");
        let n = self.made;
        self.made += 1;
        let flipped: Vec<u32>;
        let mut args = args;
        if mutation() == Mutation::Calls {
            flipped = args.iter().enumerate().map(|(i, &a)| if i == 0 { a ^ 0x8000_0100 } else { a }).collect();
            if args.is_empty() {
                name = "(mutated)";
            }
            args = &flipped;
        }
        let c = self.c;
        let Some(k) = c.calls.get(self.at) else {
            panic!("call {n}: the port calls {name}, the original made no further call");
        };
        if k.name != name {
            panic!("call {n}: the port calls {name}, the original {}", k.name);
        }
        if k.args.len() != args.len() {
            panic!("call {n} of {name}: {} arguments, the original's {}", args.len(), k.args.len());
        }
        let mut stack: Vec<(u32, u32)> = Vec::new();
        for (i, (w, &got)) in k.args.iter().zip(args).enumerate() {
            match w {
                Word::Unwritten { .. } => {}
                // a `short` parameter: the callee reads sixteen bits
                Word::Value { bytes: 2, value } => {
                    if got as u16 != *value as u16 {
                        panic!("call {n} of {name}, argument {i}: {:04x}, the original's {:04x}", got as u16, *value as u16);
                    }
                }
                Word::Ref { name: text, .. } if text.starts_with("str") && text[3..].bytes().all(|b| b.is_ascii_digit()) => {
                    let want = c.output(text).to_bytes();
                    let have = m.cstr(got);
                    if got == 0 || have != want {
                        panic!("call {n} of {name}, argument {i}: the text {:?}, the original's {:?}", String::from_utf8_lossy(&have), String::from_utf8_lossy(&want));
                    }
                }
                w => {
                    let want = value(m, self.names, w);
                    if want == got {
                        // the same local as the original's: its stores are matched by identity
                        if in_stack(want) && !stack.contains(&(want, got)) {
                            stack.push((want, got));
                        }
                        continue;
                    }
                    if !(in_stack(want) && in_stack(got)) {
                        panic!("call {n} of {name}, argument {i}: {got:08x}, the original's {want:08x}");
                    }
                    // a local: the same one where the original passes the same one
                    match stack.iter().find(|p| p.0 == want || p.1 == got) {
                        Some(p) if *p != (want, got) => panic!("call {n} of {name}, argument {i}: another local than the original passes"),
                        Some(_) => {}
                        None => stack.push((want, got)),
                    }
                }
            }
        }
        let answer = value(m, self.names, &k.result);
        self.at += 1;
        // what the stand-in did then
        while let Some(e) = c.calls.get(self.at).filter(|e| e.name.starts_with('!')) {
            let v = |i: usize| value(m, self.names, &e.args[i]);
            let a = |i: usize| translate(&stack, value(m, self.names, &e.args[i]));
            match e.name.as_str() {
                "!w" => {
                    let (at, x) = (a(0), v(1));
                    m.set_u32(at, x)
                }
                "!h" => {
                    let (at, x) = (a(0), v(1));
                    m.set_u16(at, x as u16)
                }
                "!b" => {
                    let (at, x) = (a(0), v(1));
                    m.set_u8(at, x as u8)
                }
                "!copy" => {
                    let (dst, src, len) = (a(0), a(1), v(2));
                    m.copy(dst, src, len)
                }
                "!free" => {
                    let p = v(0);
                    m.free(p)
                }
                "!alloc" => {
                    let (size, want) = (v(0), value(m, self.names, &e.result));
                    let p = m.malloc(size);
                    if p != want {
                        panic!("call {n} of {name}: its stand-in's block is at {p:08x} here, at {want:08x} in the original (the port allocated otherwise before)");
                    }
                }
                "!r" => {
                    let at = a(0);
                    for i in 1..e.args.len() {
                        let (have, want) = (m.u32(at + 4 * (i as u32 - 1)), v(i));
                        if have != want {
                            panic!("call {n} of {name}: word {} of what it is handed at {:#x} is {have:08x}, the original's {want:08x}", i - 1, v(0));
                        }
                    }
                }
                other => panic!("the case has an effect `{other}` this check does not know"),
            }
            self.at += 1;
        }
        answer
    }
}

/// Runs the port on a case and compares everything. `f` gets the image, the outside world
/// and the arguments, and answers the routine's result (`None` for a routine without one).
fn preview_call(m: &mut Mem, a: &Args, f: fn(&mut Mem, u32, u32, i32, i32, u32)) -> Option<u32> {
    f(m, a.get("txt"), a.get("pt"), a.get("x") as i32, a.get("y") as i32, a.get("out"));
    None
}

fn run(c: &Case, f: impl FnOnce(&mut Mem, &mut Replay, &Args) -> Option<u32>) -> Verdict {
    if let Some(n) = c.notes.iter().find(|n| n.starts_with("GW:") || n.starts_with("OVERRUN:")) {
        return Verdict::Different(format!("the generator's case is incomplete: {n}"));
    }
    if c.has_note("UNINIT:calls") {
        // the original's calls depend on memory nobody set: there is no sequence to replay
        return Verdict::Skip;
    }
    let Img { mut m, names } = match load(c) {
        Ok(i) => i,
        Err(e) => return Verdict::Different(e),
    };
    let values: Vec<(&str, u32)> = c.inputs.iter().filter_map(|f| Some((f.name.strip_prefix("A_")?, value(&m, &names, f.words().first()?)))).collect();
    let args = Args { c, values };
    let before = m.snapshot();
    let mut replay = Replay { c, names: &names, at: 0, made: 0 };
    // the C library's mathematics is played back from the case's recorded calls
    si_x87::trig::replay::start(c.tape());
    let mut result = f(&mut m, &mut replay, &args);
    if let Err(e) = si_x87::trig::replay::finish() {
        return Verdict::Different(format!("the library's mathematics: {e}"));
    }
    replay.skip_mathematics();
    let left = replay.left();
    if left != 0 {
        return Verdict::Different(format!("the original made {left} more calls into other packages than the port's {}", replay.made));
    }
    if let Some(fault) = m.fault() {
        return Verdict::Different(format!("the port faulted: {fault:?}"));
    }

    // every byte the port changed lies in a buffer or global of the case, or in a heap block
    // (or is a block's header, which is the allocator's own)
    let compared: Vec<(u32, u32)> = names.iter().filter(|r| r.1 != 0).map(|(_, a, n)| (a, n)).chain(m.heap_blocks().iter().copied()).collect();
    let headers: Vec<(u32, u32)> = m.heap_blocks().iter().map(|&(a, _)| (a - 8, 8)).collect();
    let within = |regions: &[(u32, u32)], a: u32| regions.iter().any(|&(base, len)| a.wrapping_sub(base) < len);
    let changed = m.changed(&before);
    for (at, bytes) in &changed {
        if let Some(i) = (0..bytes.len() as u32).find(|&i| !within(&compared, at + i) && !within(&headers, at + i)) {
            return Verdict::Different(format!("the port changed the byte at {:#010x}, which the case does not hold", at + i));
        }
    }

    match mutation() {
        Mutation::None => {}
        // one bit of the result and of every run of bytes the port changed; where it
        // changed nothing, one bit of the case's first buffer, which it must have left alone
        Mutation::Values => {
            result = result.map(|r| r ^ 1);
            let mut flipped = false;
            for (at, bytes) in &changed {
                if let Some(i) = (0..bytes.len() as u32).find(|&i| within(&compared, at + i)) {
                    let b = m.u8(at + i) ^ 0x10;
                    m.set_u8(at + i, b);
                    flipped = true;
                }
            }
            if !flipped && result.is_none() {
                if let Some(&(a, n)) = compared.first() {
                    let at = a + (a.wrapping_mul(0x9e37_79b1) >> 7) % n;
                    let b = m.u8(at) ^ 0x10;
                    m.set_u8(at, b);
                }
            }
        }
        // the calls were changed as the port made them: a case without calls says nothing
        Mutation::Calls => {
            if replay.made == 0 {
                return Verdict::Skip;
            }
        }
    }

    // the result
    for (field, bits) in [("eax", 32), ("ax", 16)] {
        if let Some(want) = c.try_output(field) {
            let Some(got) = result else {
                return Verdict::Different(format!("the check hands no result to compare with `{field}`"));
            };
            let mask = if bits == 32 { u32::MAX } else { 0xffff };
            if let Some(w) = want.written()[0] {
                if w as u32 & mask != got & mask {
                    return Verdict::Different(format!("{field}: port {:08x}, original {:08x}", got & mask, w as u32 & mask));
                }
            }
        }
    }
    // the buffers and globals, as the original left them (or left them alone)
    for (name, addr, _) in names.iter() {
        if addr == 0 {
            continue;
        }
        let Some(w) = words(c.after(name)) else { continue };
        if let Err(e) = m.compare_words(addr, &w, &names) {
            return Verdict::Different(format!("{name}: {e}"));
        }
    }
    // the heap: the same blocks, at the same addresses, of the same sizes, with the same bytes
    let theirs: Vec<&Field> = (0..).map_while(|i| c.try_output(&format!("heap{i}"))).collect();
    if theirs.len() != m.heap_blocks().len() {
        return Verdict::Different(format!("{} heap blocks, the original's {}", m.heap_blocks().len(), theirs.len()));
    }
    for (i, f) in theirs.iter().enumerate() {
        let (addr, size) = m.heap_blocks()[i];
        let w = words(f).unwrap_or_default();
        if si_mem::words_len(&w) != size {
            return Verdict::Different(format!("heap{i}: {size} bytes, the original's {}", si_mem::words_len(&w)));
        }
        if let Err(e) = m.compare_words(addr, &w, &names) {
            return Verdict::Different(format!("heap{i}: {e}"));
        }
    }
    // the blocks freed, in order
    let freed: Vec<u32> = c.try_output("freed").map_or_else(Vec::new, |f| f.words().iter().map(|w| value(&m, &names, w)).collect());
    if m.freed() != freed.as_slice() {
        return Verdict::Different(format!("freed: port {:x?}, original {freed:x?}", m.freed()));
    }
    Verdict::Same
}

pub fn lookup(name: &str) -> Option<Check> {
    let name = name.strip_suffix("#img")?;
    Some(match name {
        "addMemEntry" => |c| {
            run(c, |m, _, a| {
                instance::add_mem_entry(m, a.ptr("block"));
                None
            })
        },
        "SI_deleteMemReferences" => |c| {
            run(c, |m, _, _| {
                instance::delete_mem_references(m);
                None
            })
        },
        "registerInstanceMatArrays" => |c| run(c, |m, _, _| Some(instance::register_instance_mat_arrays(m) as u32)),
        "SI_setRegisterInstMatArraysBool" => |c| {
            run(c, |m, _, a| {
                instance::set_register_inst_mat_arrays_bool(m, a.i16("on"));
                None
            })
        },
        "SI_findGlData" => |c| run(c, |m, _, a| Some(graflist::find_gl_data(m, a.ptr("list"), a.ptr("data"), a.get("kind") as i32).0)),
        "SI_instanceInitializeVisit" => |c| {
            run(c, |m, _, _| {
                instance::instance_initialize_visit(m);
                None
            })
        },
        "SI_instanceTextureShare" => |c| {
            run(c, |m, _, a| {
                instance::instance_texture_share(m, a.ptr("from"), a.ptr("to"));
                None
            })
        },
        "SI_instanceMaterialShare" => |c| {
            run(c, |m, _, a| {
                instance::instance_material_share(m, a.ptr("from"), a.ptr("to"));
                None
            })
        },
        "SI_findInstanceInHierarchy" => |c| run(c, |m, _, a| Some(instance::find_instance_in_hierarchy(m, a.ptr("node"), a.ptr("brothers"), a.i16("any")).0)),
        "SI_updateSubregionValues" => |c| {
            run(c, |m, o, a| {
                matter::update_subregion_values(o, m, a.get("controls"));
                None
            })
        },
        "SI_updateSubregionRes" => |c| {
            run(c, |m, _, a| {
                rnd_dial::update_subregion_res(m, a.get("controls"));
                None
            })
        },
        "selectSubRegionWindow" => |c| run(c, |m, o, _| Some(rnd_dial::select_sub_region_window(o, m) as u32)),
        "SI_elemdictModelMasterFromInstance" => |c| run(c, |m, o, a| Some(instance::elemdict_model_master_from_instance(o, m, a.ptr("dict"), a.ptr("model")).0)),
        "SI_modelMasterFromInstance" => |c| run(c, |m, o, a| Some(instance::model_master_from_instance(o, m, a.ptr("model")).0)),
        "SI_modelIsMaster" => |c| run(c, |m, o, a| Some(instance::model_is_master(o, m, a.ptr("model")) as u32)),
        "SI_instSetRelation" => |c| {
            run(c, |m, o, a| {
                instance::inst_set_relation(o, m, a.ptr("instance"), a.ptr("master"));
                None
            })
        },
        "SI_instanceSetMasterDependency" => |c| run(c, |m, o, a| Some(instance::instance_set_master_dependency(o, m, a.ptr("model"), a.ptr("dependency")).0)),
        "SI_instanceCopyTransformation" => |c| {
            run(c, |m, o, a| {
                instance::instance_copy_transformation(o, m, a.ptr("instance"), a.ptr("copy"));
                None
            })
        },
        "instanceDisposeNodeWithoutGeometry" => |c| {
            run(c, |m, o, a| {
                instance::instance_dispose_node_without_geometry(o, m, a.ptr("node"));
                None
            })
        },
        "SI_instanceDispose" => |c| {
            run(c, |m, o, a| {
                instance::instance_dispose(o, m, a.ptr("node"), a.i16("brothers"));
                None
            })
        },
        "SI_expandCopyInfoFromInstance" => |c| {
            run(c, |m, o, a| {
                instance::expand_copy_info_from_instance(o, m, a.ptr("instance"), a.ptr("copy"));
                None
            })
        },
        "SI_instanceCheckCycle" => |c| run(c, |m, o, a| Some(instance::instance_check_cycle(o, m, a.ptr("master"), a.ptr("candidate"), a.ptr("up")) as u32)),
        "SI_edSwapMaster" => |c| {
            run(c, |m, o, a| {
                instance::ed_swap_master(o, m, a.ptr("master"), a.ptr("new_master"));
                None
            })
        },
        "instanceCopyNodeFromMaster" => |c| run(c, |m, o, a| Some(instance::instance_copy_node_from_master(o, m, a.ptr("master"), a.i16("keep")).0)),
        "SI_copyInstanceRelation" => |c| {
            run(c, |m, o, a| {
                instance::copy_instance_relation(o, m, a.ptr("from"), a.ptr("to"), a.ptr("up"));
                None
            })
        },
        "SI_swapMaster" => |c| {
            run(c, |m, o, _| {
                instance::swap_master(o, m);
                None
            })
        },
        "SI_modelExpandAllWithoutGeometry" => {
            |c| run(c, |m, o, a| Some(instance::model_expand_all_without_geometry(o, m, a.ptr("master"), a.ptr("father"), a.i16("visible"), a.i16("deep")).0))
        }
        "SI_modelExpandAllWithoutGeometryAbsolute" => {
            |c| run(c, |m, o, a| Some(instance::model_expand_all_without_geometry_absolute(o, m, a.ptr("master"), a.ptr("father"), a.i16("deep")).0))
        }
        "connectExpandFromInstance" => |c| {
            run(c, |m, o, a| {
                instance::connect_expand_from_instance(o, m, a.ptr("master"), a.ptr("instance"), a.ptr("copy"));
                None
            })
        },
        "SI_modelExpandInstance" => |c| {
            run(c, |m, o, a| {
                instance::model_expand_instance(o, m, a.ptr("instance"), a.i16("deep"));
                None
            })
        },
        "SI_expandInstance" => |c| {
            run(c, |m, o, a| {
                instance::expand_instance(o, m, a.i16("deep"));
                None
            })
        },
        "SI_frameRender" => |c| {
            run(c, |m, o, a| {
                matter::frame_render(o, m, a.i16("all"), a.get("dialog"), a.i16("full"));
                None
            })
        },
        "SI_sequenceRender" => |c| run(c, |m, o, _| Some(rnd_dial::sequence_render(o, m) as u32)),
        // ---- geometry.c: the per-frame refresh of an instance's copy
        "si_traverseExpandedInstance" => |c| {
            run(c, |m, o, a| {
                geometry::traverse_expanded_instance(o, m, a.ptr("root"));
                None
            })
        },
        "SI_expandUpdateTransfo" => |c| {
            run(c, |m, o, a| {
                geometry::expand_update_transfo(o, m, a.ptr("master"), a.ptr("instance"), a.ptr("copy"));
                None
            })
        },
        "expandUpdateTransfo" => |c| run(c, |m, o, a| Some(geometry::expand_update_transfo_walk(o, m, a.ptr("master"), a.ptr("instance"), a.ptr("copy")).0)),
        // ---- envtools.c, classic.c, skeleton.c
        "SI_envelopLevel" => |c| run(c, |m, _, a| Some(envtools::envelop_level(m, a.ptr("model")) as u32)),
        "SI_envelopVertAllocate" => |c| run(c, |m, _, _| Some(envtools::envelop_vert_allocate(m).0)),
        "SI_envelopListAllocate" => |c| run(c, |m, _, a| Some(envtools::envelop_list_allocate(m, a.get("owner")).0)),
        "SI_envelopInfoAllocate" => |c| run(c, |m, _, _| Some(envtools::envelop_info_allocate(m).0)),
        "compensateInfoListAllocate" => |c| run(c, |m, _, _| Some(classic::compensate_info_list_allocate(m).0)),
        "SI_skeChainRoot" => |c| run(c, |m, _, a| Some(skeleton::ske_chain_root(m, a.ptr("model")).0)),
        "SI_mainGlobEnv" => |c| run(c, |m, _, a| Some(envtools::main_glob_env(m, a.ptr("model")) as u32)),
        "SI_envelopVertCopy" => |c| run(c, |m, _, a| Some(envtools::envelop_vert_copy(m, a.ptr("from")).0)),
        "envelopInfoCopy" => |c| run(c, |m, _, a| Some(envtools::envelop_info_copy(m, a.ptr("from"), a.i16("lists")).0)),
        "SI_classicMdlCompInfoDispose" => |c| {
            run(c, |m, _, a| {
                classic::classic_mdl_comp_info_dispose(m, a.ptr("list"));
                None
            })
        },
        "SI_classicModelCompInfoCopy" => |c| {
            run(c, |m, _, a| {
                classic::classic_model_comp_info_copy(m, a.ptr("from"), a.ptr("to"));
                None
            })
        },
        "SI_modelAllocate" => |c| run(c, |m, _, a| Some(model_b::model_allocate(m, a.ptr("name")).0)),
        "SI_modelAlgoTypeCopy" => |c| {
            run(c, |m, _, a| {
                model_b::model_algo_type_copy(m, a.ptr("from"), a.ptr("to"));
                None
            })
        },
        "SI_generalMatrixAllocateDbl" => |c| run(c, |m, _, a| Some(invkin::general_matrix_allocate_dbl(m, a.get("rows") as i32, a.get("cols") as i32).0)),
        "SI_generalMatrixCopyDbl" => |c| run(c, |m, _, a| Some(invkin::general_matrix_copy_dbl(m, a.ptr("from")).0)),
        "SI_generalMatrixDisposeDbl" => |c| {
            run(c, |m, _, a| {
                invkin::general_matrix_dispose_dbl(m, a.ptr("g"));
                None
            })
        },
        "SI_invKinAllocate" => |c| run(c, |m, _, _| Some(invkin::inv_kin_allocate(m).0)),
        "SI_invKinCopy" => |c| run(c, |m, _, a| Some(invkin::inv_kin_copy(m, a.ptr("from")).0)),
        "SI_invKinDispose" => |c| {
            run(c, |m, _, a| {
                invkin::inv_kin_dispose(m, a.ptr("record"));
                None
            })
        },
        "SI_envelopCopy" => |c| {
            run(c, |m, _, a| {
                envtools::envelop_copy(m, a.ptr("src"), a.ptr("father"), a.ptr("copy"));
                None
            })
        },
        // ---- the fourth pass: edtools.c, model.c, light.c, the controls' start-up state
        "SI_modelNbSubModels" => |c| run(c, |m, _, a| Some(edtools::model_nb_sub_models(m, a.ptr("model")) as u32)),
        "SI_lightGetGrafType" => |c| run(c, |m, _, a| Some(edtools::light_get_graf_type(m, a.get("light")) as u32)),
        "SI_edGetElemFromModel" => |c| run(c, |m, o, a| Some(edtools::ed_get_elem_from_model(o, m, a.ptr("model"), a.ptr("index")).0)),
        "SI_edFindSon" => |c| run(c, |m, o, a| Some(edtools::ed_find_son(o, m, a.ptr("elem")).0)),
        "SI_edFindNextSon" => |c| run(c, |m, o, a| Some(edtools::ed_find_next_son(o, m, a.ptr("elem")).0)),
        "SI_edGetElemFromTexture2D" => |c| run(c, |m, o, a| Some(edtools::ed_get_elem_from_texture2d(o, m, a.get("texture"), a.ptr("index")).0)),
        "SI_elemdictUpdateModelHierarchy" => |c| {
            run(c, |m, o, a| {
                edtools::elemdict_update_model_hierarchy(o, m, a.ptr("dict"));
                None
            })
        },
        "SI_elemdictAddModel" => |c| {
            run(c, |m, o, a| {
                edtools::elemdict_add_model(o, m, a.ptr("dict"), a.ptr("model"), a.ptr("gn"), a.i16("unique"));
                None
            })
        },
        "SI_edAddModel" => |c| {
            run(c, |m, o, a| {
                edtools::ed_add_model(o, m, a.ptr("model"), a.ptr("gn"), a.i16("unique"));
                None
            })
        },
        "SI_fadeInit" => |c| {
            run(c, |m, _, a| {
                matter::fade_init(m, a.get("record"));
                None
            })
        },
        "SI_fogInit" => |c| {
            run(c, |m, _, a| {
                matter::fog_init(m, a.get("record"));
                None
            })
        },
        "SI_matterFadeInit" => |c| {
            run(c, |m, _, _| {
                matter::matter_fade_init(m);
                None
            })
        },
        "fogInit" => |c| {
            run(c, |m, _, _| {
                matter::fog_init_default(m);
                None
            })
        },
        "SI_mentalRayInit" => |c| {
            run(c, |m, _, a| {
                matter::mental_ray_init(m, a.get("mr"));
                None
            })
        },
        "SI_renControlInit" => |c| {
            run(c, |m, _, a| {
                matter::ren_control_init(m, a.get("controls"));
                None
            })
        },
        // the sixth pass (emu_gen_pass5.py): renu_display.c
        "SI_resetRenderWinOpen" => |c| {
            run(c, |m, _, _| {
                renu_display::reset_render_win_open(m);
                None
            })
        },
        "SI_openRenderWindow" => |c| run(c, |m, o, a| Some(renu_display::open_render_window(o, m, a.get("controls")))),
        "SI_closeRenderWindow" => |c| {
            run(c, |m, o, a| {
                renu_display::close_render_window(o, m, a.get("controls"));
                None
            })
        },
        // the seventh pass (emu_gen_pass6.py): pre_render.c
        "si_getRndBox" => |c| {
            run(c, |m, _, a| {
                pre_render::get_rnd_box(m, a.get("box"));
                None
            })
        },
        "si_drawRndBox" => |c| {
            run(c, |m, o, a| {
                pre_render::draw_rnd_box(o, m, a.get("box"));
                None
            })
        },
        "SI_initStandAloneRender" => |c| run(c, |m, o, a| Some(sys_render::init_stand_alone_render(o, m, a.get("name")))),
        "SI_mrfileConvertToUnix" => |c| run(c, |m, o, a| Some(renu_filepath::mrfile_convert_to_unix(o, m, a.get("path"), a.get("out"), a.get("size") as i32))),
        "SI_symmetryGetElem" => |c| run(c, |m, o, a| Some(symmetry::symmetry_get_elem(o, m, a.ptr("model"), a.ptr("index"), a.get("data"), a.get("keep") as i16, a.get("own") as i16).0)),
        "si_symmetryVector" => |c| {
            run(c, |m, _, a| {
                symmetry_vector::symmetry_vector(m, a.get("a"), a.get("b"), a.get("which") as i32, a.get("data"), a.get("flag") as i16, a.get("out"));
                None
            })
        },
        "si_symmetryVectorOLD" => |c| {
            run(c, |m, _, a| {
                symmetry_vector::symmetry_vector_old(m, a.get("a"), a.get("b"), a.get("which") as i32, a.get("data"), a.get("flag") as i16, a.get("out"));
                None
            })
        },
        "SI_doSubregion" => |c| run(c, |m, o, _| Some(renu_subregion::do_subregion(o, m) as u32)),
        "computeBounds" => |c| run(c, |m, _, a| {
            renu_subregion::compute_bounds(m, a.get("gp"));
            None
        }),
        "lookupColor__FP7SI_RGBAfP13SI_LrectPixel" => |c| {
            run(c, |m, _, a| {
                preview::lookup_color(m, a.get("table"), f32::from_bits(a.get("t")), a.get("out"));
                None
            })
        },
        "previewNull" => |c| {
            run(c, |m, _, a| {
                preview::preview_null(m, a.get("out"));
                None
            })
        },
        "previewCloud" => |c| run(c, |m, _, a| preview_call(m, a, preview::preview_cloud)),
        "previewMarble" => |c| run(c, |m, _, a| preview_call(m, a, preview::preview_marble)),
        "previewWood" => |c| run(c, |m, _, a| preview_call(m, a, preview::preview_wood)),
        "previewTxt" => |c| {
            run(c, |m, _, a| {
                // SI_ttnGetNoise (0x083172bc) is the real routine of si-render, on the table the case holds
                let noise = si_render::tt_noise::Noise { table: (0..0x10000u32).map(|i| m.f32(0x087b_afc0 + 4 * i)).collect(), ..Default::default() };
                preview::preview_txt(m, &noise, a.get("txt"), a.get("pt"), a.get("x") as i32, a.get("y") as i32, a.get("out"));
                None
            })
        },
        "screenToRle" => |c| {
            run(c, |m, o, a| {
                let file = prv_main::screen_to_rle(o, m, a.get("w") as i32, a.get("h") as i32, a.get("mode") as i32, a.get("x0") as i32, a.get("y0") as i32);
                // the file the original wrote through its fopen: compared byte for byte (a byte
                // the original left undefined, the stale chunk word, is not compared)
                let name = c.inputs.iter().find(|f| f.name == "path").map(|f| String::from_utf8_lossy(&f.to_bytes()).trim_end_matches('\0').to_string());
                let want = name.and_then(|n| c.try_output(&format!("file:{n}")));
                match (&file, want) {
                    (Some(bytes), Some(w)) => {
                        let mut bytes = bytes.clone();
                        if mutation() == Mutation::Values {
                            if let Some(b) = bytes.first_mut() {
                                *b ^= 0x10;
                            }
                        }
                        let ws = w.written();
                        assert!(ws.len() == bytes.len(), "the file has {} bytes, the original's {}", bytes.len(), ws.len());
                        for (i, (g, x)) in bytes.iter().zip(&ws).enumerate() {
                            // bytes 102 and 103 are the picture chunk's spare word: stale stack in the original
                            if let (Some(x), false) = (x, i == 102 || i == 103) {
                                assert!(*g as u128 == *x, "file byte {i}: port {g:02x}, original {x:02x}");
                            }
                        }
                    }
                    (None, None) => {}
                    (Some(_), None) => panic!("the port wrote a file, the original none"),
                    (None, Some(_)) => panic!("the original wrote a file, the port none"),
                }
                Some(file.is_some() as u32)
            })
        },
        "SI_ttpTformPt" => |c| {
            run(c, |m, _, a| {
                preview::ttp_tform_pt(m, a.get("txt"), a.get("pt"), a.get("out"));
                None
            })
        },
        "SI_ttpInitPixelSphTxt3d" => |c| {
            run(c, |m, _, a| {
                preview::ttp_init_pixel_sph_txt3d(m, a.get("txt"), a.get("img") as i16);
                None
            })
        },
        "SI_sequenceRenderMental" => |c| run(c, |m, o, a| Some(rens_main::sequence_render_mental(o, m, a.get("preview") as i16))),
        // the unit left2 (re/tools/left2/emu_gen_mental.py): the rest of rens_main.c
        "SI_prepareRendering" => |c| run(c, |m, o, a| Some(rens_main::prepare_rendering(o, m, a.get("parms"), a.get("elem"), a.get("maxcoc"), a.get("live") as i16))),
        "SI_mentalPreview" => |c| {
            run(c, |m, o, a| {
                rens_main::mental_preview(o, m, a.get("mode") as i16, a.get("flags"));
                None
            })
        },
        "SI_abortion" => |c| run(c, |m, o, _| Some(pre_render::abortion(o, m))),
        _ => return None,
    })
}
