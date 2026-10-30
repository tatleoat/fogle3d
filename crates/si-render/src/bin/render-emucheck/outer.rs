// SPDX-License-Identifier: MIT
//! Checks of the routines ported in place, on the memory image (seventh pass; finding 0012,
//! section 16): `rnd_main.c`, `fbuffer.c` and the dictionary walks of `initialize.c`, on the
//! vectors of `re/tools/si-render/emu/gen_outer.py`.
//!
//! A case is loaded into a `si_mem::Mem` as it is: every input buffer through
//! `si_mem::load_case`, a buffer named `G<address>` at that address (a global), a buffer
//! the original only writes as fresh memory of the output's size. The port runs on the
//! image. Compared afterwards: every buffer of the case, word for word (an input buffer the
//! original left alone must be unchanged), the blocks the routine took from `malloc`, the
//! list of freed blocks, the result, and the sequence of calls to routines of other source
//! files with their arguments. Those calls are answered from the case's own record: the
//! generator replaced each callee by a stand-in, and [`Tape`] is the same stand-in here.
//!
//! With `EMUCHECK_MUTATE` one bit of the first byte the port changed in the case's memory
//! is flipped (or of its result; or its last call is dropped) before the comparison.

use crate::prep::mutate;
use si_mem::{Mem, Names, Region, Word as W};
use si_render::img::{f, g, Outside};
use si_render::jitter::{self, Drand48};
use si_render::{fbuffer, initialize, rnd_main};
use si_x87::vectors::{Call, Case, Check, Field, Verdict, Word};

fn word(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn words(f: &Field) -> Vec<W> {
    f.words().iter().map(word).collect()
}

/// The address a buffer named `G<eight hex digits>` stands for.
fn global_addr(name: &str) -> Option<u32> {
    let hex = name.strip_prefix('G')?;
    if hex.len() < 8 {
        return None;
    }
    u32::from_str_radix(&hex[..8], 16).ok()
}

/// Outputs that are not buffers of the case.
fn is_result(name: &str) -> bool {
    matches!(name, "eax" | "ax" | "al" | "st0" | "ptr" | "freed" | "stdout" | "stderr") || name.starts_with("heap") || name.starts_with("str") || name.starts_with("file:")
}

/// A case loaded into an image, for the checks of routines that are ported on the crate's
/// own records: the port's records are then read from the image by the readers of
/// `si_render::inimage` (eighth pass), not built from the vector's words. Only the case's
/// inputs are placed.
pub struct Loaded {
    pub m: Mem,
    pub names: Names,
}

impl Loaded {
    pub fn of(c: &Case) -> Loaded {
        let (m, names) = load_with(c, false).unwrap_or_else(|e| panic!("the case does not load: {e}"));
        Loaded { m, names }
    }

    /// The address of the case's buffer `name`.
    pub fn at(&self, name: &str) -> u32 {
        self.names.addr(&self.m, name).unwrap_or_else(|| panic!("the case has no buffer `{name}`"))
    }

    /// The buffer an address lies in, and the offset.
    pub fn name_of(&self, addr: u32) -> Option<(String, u32)> {
        self.names.name_of(&self.m, addr)
    }
}

/// The case's memory.
pub fn load(c: &Case) -> Result<(Mem, Names), String> {
    load_with(c, true)
}

fn load_with(c: &Case, outputs: bool) -> Result<(Mem, Names), String> {
    let mut m = Mem::new();
    let globals: Vec<(&Field, u32)> = c.inputs.iter().chain(&c.outputs).filter_map(|f| global_addr(&f.name).map(|a| (f, a))).collect();
    // a pointer to a global is a number already
    let resolve = |f: &Field| -> Vec<W> {
        words(f)
            .into_iter()
            .map(|w| match &w {
                W::Ref { name, offset } => match global_addr(name) {
                    Some(a) => W::Value { bytes: 4, value: a.wrapping_add(*offset) as u128 },
                    None => w,
                },
                _ => w,
            })
            .collect()
    };
    let mut held: Vec<(String, Option<Vec<W>>)> = Vec::new();
    for f in &c.inputs {
        if global_addr(&f.name).is_some() || f.name.starts_with("file:") {
            continue;
        }
        let mut w = if f.is_null() { None } else { Some(resolve(f)) };
        // a text argument ends with a NUL the vector does not print (a buffer of bytes gets
        // one byte more, which nothing looks at)
        if let Some(w) = w.as_mut() {
            if w.iter().all(|x| matches!(x, W::Value { bytes: 1, .. })) {
                w.push(W::Value { bytes: 1, value: 0 });
            }
        }
        held.push((f.name.clone(), w));
    }
    // a buffer the original only writes: fresh memory of the output's size
    for f in &c.outputs {
        if !outputs || global_addr(&f.name).is_some() || is_result(&f.name) || c.try_input(&f.name).is_some() {
            continue;
        }
        let blank: Vec<W> = f.words().iter().map(|w| W::Unwritten { bytes: w.bytes() }).collect();
        held.push((f.name.clone(), Some(blank)));
    }
    let fields: Vec<(&str, Option<&[W]>)> = held.iter().map(|(n, w)| (n.as_str(), w.as_deref())).collect();
    let mut names = si_mem::load_case(&mut m, &fields)?;
    for (f, a) in &globals {
        if names.addr(&m, &f.name).is_none() {
            names.add(&f.name, *a, si_mem::words_len(&words(f)));
        }
    }
    for f in &c.inputs {
        if let Some(a) = global_addr(&f.name) {
            m.load_words(a, &resolve(f), &names)?;
        }
    }
    // the binary's initial data these routines read
    m.set_u32(g::SI_SBOXFI, SBOXFI);
    rnd_main::place_texts(&mut m);
    Ok((m, names))
}

/// sboxfi (0x0861236c): what the pointer `SI_sboxfi` holds in the binary's data.
const SBOXFI: u32 = 0x0861_236c;

/// The callees by address.
fn callee(addr: u32) -> Option<&'static str> {
    Some(match addr {
        f::GETENV => "getenv",
        f::SI_DTIME => "SI_dtime",
        f::SI_INITIALIZE => "SI_initialize",
        f::SI_TAG_MDL_RM => "SI_tagMdlRm",
        f::SI_TAG_MDL_UPDATE => "SI_tagMdlUpdate",
        f::SI_TAG_ALLOCATE => "SI_tagAllocate",
        f::SI_TAG_ALLOCATE_CHANNELS => "SI_tagAllocateChannels",
        f::SI_TAG_WRITE_FILE_HEADER => "si_tagWriteFileHeader",
        f::SI_CHUNK_SAVE_CURRENT => "SI_chunkSaveCurrent",
        f::SI_CHUNK_RECLAIM => "SI_chunkReclaim",
        f::SI_CHUNK_FREE_ALL => "SI_chunkFreeAll",
        f::SI_CHUNK_MALLOC => "SI_chunkMalloc",
        f::SI_CHUNK_INITIALIZE => "SI_chunkInitialize",
        f::SI_FB_INIT => "si_fbInit",
        f::SI_FB_WRITE => "SI_fbWrite",
        f::BUILD_NAME => "buildName",
        f::SI_SHOWFRAME => "SI_showframe",
        f::SI_SHOWFIELD => "SI_showfield",
        f::SI_REN_EXIT => "SI_renExit",
        f::PRINTF => "printf",
        f::FCLOSE => "fclose",
        f::RENAME => "rename",
        f::UNLINK => "unlink",
        f::LOCALTIME => "localtime",
        f::MALLOPT => "mallopt",
        f::SETJMP => "_setjmp",
        f::SI_PICT_DISPLAY_ERROR => "SI_pictDisplayError",
        f::SI_PICTURE_OPEN_FILE => "SI_pictureOpenFile",
        f::SI_SCANLINE_READ => "SI_scanlineRead",
        f::SI_RLE_INIT => "si_rleInit",
        f::SI_RLE_WRITE => "SI_rleWrite",
        f::SI_RLE_CLOSE => "SI_rleClose",
        f::SI_SYSTEM => "SI_system",
        f::SI_UPDATE => "SI_update",
        f::SI_FS_DITHER_INIT => "SI_fsDitherInit",
        f::SI_CUS_CHECK_RENDER_ABORT => "SI_cusCheckRenderAbort",
        f::SI_TABLET_GET_BUTTON_UP_EVENT => "SI_tabletGetButtonUpEvent",
        f::GETBUTTON => "getbutton",
        f::QRESET => "qreset",
        f::QREAD => "qread",
        f::GFLUSH => "gflush",
        f::FONT => "font",
        f::STRWIDTH => "strwidth",
        f::RGBCOLOR => "RGBcolor",
        f::CMOV2I => "cmov2i",
        f::CHARSTR => "charstr",
        f::GETORIGIN => "getorigin",
        f::LRECTWRITE => "lrectwrite",
        f::SBOXS => "sboxs",
        SBOXFI => "sboxfi",
        f::SI_VOLUNTARY_EXIT => "SI_voluntaryExit",
        f::SI_WARNING => "SI_warning",
        f::SI_SUPERSAMPLE => "SI_supersample",
        f::SI_SAMPLE => "SI_sample",
        f::SI_CLEAN_UP_TEXTURE => "si_cleanUpTexture",
        f::SI_SIGNAL_INIT => "SI_signalInit",
        f::SI_SCENE_REFRESH => "SI_sceneRefresh",
        f::SI_INSTANCE_DISPOSE => "SI_instanceDispose",
        f::SI_DELETE_MEM_REFERENCES => "SI_deleteMemReferences",
        f::SI_MODEL_MASTER_FROM_INSTANCE => "SI_modelMasterFromInstance",
        f::SI_ELEMDICT_FIND_RELATED_ELEM => "SI_elemdictFindRelatedElem",
        f::SI_ED_GET_ELEM_FROM_MODEL => "SI_edGetElemFromModel",
        f::SI_ED_GET_ELEM_FROM_TEXTURE2D => "SI_edGetElemFromTexture2D",
        f::SI_ED_FIND_SON => "SI_edFindSon",
        f::SI_ED_FIND_NEXT_SON => "SI_edFindNextSon",
        f::SI_ELEMLIST_CLEAR_LOW_BYTE_WORK_FLAG => "SI_elemlistClearLowByteWorkFlag",
        f::SI_TXT2D_SET_WRAP => "SI_txt2dSetWrap",
        f::SI_STATUS_DISPLAY_MESSAGE => "SI_statusDisplayMessage",
        f::INIT_DATA => "initData",
        f::SI_INIT_VIEWPORT => "si_initViewport",
        f::INIT_BUFFERS => "initBuffers",
        f::INIT_FILTER => "initFilter",
        f::SI_CAMERA => "SI_camera",
        f::SI_SET_BOUNDS => "si_setBounds",
        f::SI_TRAVERSE => "si_traverse",
        f::SI_TRAVERSE_EXPANDED_INSTANCE => "si_traverseExpandedInstance",
        f::Z_SAMPLE => "zSample",
        _ => return None,
    })
}

/// What a stand-in does beside answering: writes through the port's argument pointers what
/// the generator's stand-in wrote through the original's. `None` keeps the recorded answer.
pub type Effect = fn(&mut Mem, &Names, &Case, usize, &Call, &[u32]) -> Option<u32>;

/// The recorded calls of a case, replayed to the port.
pub struct Tape<'a> {
    c: &'a Case,
    names: &'a Names,
    next: usize,
    /// The first disagreement.
    pub wrong: Option<String>,
    effect: Option<Effect>,
    /// With the mutation switch: the call to leave out.
    drop_last: bool,
}

impl<'a> Tape<'a> {
    pub fn new(c: &'a Case, names: &'a Names, effect: Option<Effect>) -> Tape<'a> {
        Tape { c, names, next: 0, wrong: None, effect, drop_last: false }
    }

    fn fail(&mut self, text: String) -> u32 {
        if self.wrong.is_none() {
            self.wrong = Some(text);
        }
        0
    }

    /// Every recorded call was made.
    pub fn finish(&self) -> Result<(), String> {
        if let Some(w) = &self.wrong {
            return Err(w.clone());
        }
        let made = if self.drop_last { self.next.saturating_sub(1) } else { self.next };
        if made != self.c.calls.len() {
            return Err(format!("calls: original {}, port {made}", self.c.calls.len()));
        }
        Ok(())
    }

    pub fn made(&self) -> usize {
        self.next
    }
}

/// Recorded arguments the port's are not held against: addresses of the caller's locals.
fn local_args(name: &str) -> &'static [usize] {
    match name {
        "SI_elemdictFindRelatedElem" => &[6, 7],
        "SI_edGetElemFromModel" | "SI_edGetElemFromTexture2D" => &[1],
        "SI_chunkSaveCurrent" => &[1],
        "buildName" => &[0],
        "si_tagWriteFileHeader" => &[0],
        "SI_pictureOpenFile" => &[0, 1, 2, 3, 4, 5],
        "SI_scanlineRead" => &[2],
        "qread" => &[0],
        _ => &[],
    }
}

/// What `SI_camera`'s stand-in records of the controls and the camera copy (`gen_outer.py`,
/// `CAMERA_PROBES`): address and width.
fn probes(name: &str) -> &'static [(u32, u8)] {
    match name {
        "SI_camera" => &[
            (0x0879_117a, 2),
            (0x0879_117c, 2),
            (0x0879_0d34, 4),
            (0x0879_0e04, 2),
            (0x0879_0e06, 2),
            (0x0879_0d38, 2),
            (0x0879_0c7c, 2),
            (0x0879_0650, 4),
            (0x0879_0654, 4),
            (0x0879_0658, 4),
            (0x0879_065c, 4),
            (0x0879_0660, 4),
            (0x0879_0664, 4),
            (0x0879_0670, 4),
            (0x0879_0674, 4),
            (0x0879_0678, 4),
            (0x0879_0680, 4),
            (0x0879_06ac, 4),
        ],
        _ => &[],
    }
}

impl Outside for Tape<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        let Some(name) = callee(addr) else {
            return self.fail(format!("the port calls {addr:#010x}, which the check does not know"));
        };
        let Some(k) = self.c.calls.get(self.next) else {
            return self.fail(format!("call {}: the port calls {name}, the original nothing more", self.next));
        };
        let at = self.next;
        self.next += 1;
        if k.name != name {
            return self.fail(format!("call {at}: original {}, port {name}", k.name));
        }
        // a stand-in that takes no argument may record what it finds in the globals instead
        let probes = probes(name);
        if args.is_empty() && !probes.is_empty() && probes.len() == k.args.len() {
            for ((addr, bytes), want) in probes.iter().zip(&k.args) {
                let got = m.bytes(*addr, *bytes as u32).iter().rev().fold(0u128, |a, b| (a << 8) | *b as u128);
                if want.value() != Some(got) {
                    return self.fail(format!("call {at} {name}: finds {got:#x} at {addr:#010x}, in the original {want:?}"));
                }
            }
        }
        // a stand-in may record more than the arguments (a pointer it answered with)
        for (i, (want, got)) in k.args.iter().zip(args).enumerate() {
            if local_args(name).contains(&i) {
                continue;
            }
            let same = match want {
                Word::Ref { name: buffer, offset } if buffer.starts_with("str") && self.names.addr(m, buffer).is_none() => {
                    let text = self.c.output(buffer).to_bytes();
                    m.cstr(*got) == text && *offset == 0
                }
                Word::Ref { name: buffer, offset } => self.names.addr(m, buffer).map(|a| a.wrapping_add(*offset)) == Some(*got),
                w => w.value().map(|v| v as u32) == Some(*got),
            };
            if !same {
                return self.fail(format!("call {at} {name}: argument {i}: original {want:?}, port {got:#010x}"));
            }
        }
        let answer = match self.effect.and_then(|e| e(m, self.names, self.c, at, k, args)) {
            Some(v) => v,
            None => match &k.result {
                Word::Ref { name, offset } => self.names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
                w => w.value().unwrap_or(0) as u32,
            },
        };
        answer
    }
}

/// A recorded argument as an address.
pub fn recorded_ptr(m: &Mem, names: &Names, w: &Word) -> u32 {
    match w {
        Word::Ref { name, offset } => names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
        w => w.value().unwrap_or(0) as u32,
    }
}

/// Holds the image against everything the case says the original left.
pub fn compare(c: &Case, m: &mut Mem, names: &Names, snapshot: &si_mem::Snapshot, result: Option<(&str, u32)>, tape: &mut Tape) -> Verdict {
    let mut result = result;
    if mutate() {
        // the first byte the port changed in the case's memory; else its result; else its
        // last call; else there is nothing the port produced
        let changed = [Region::Arena, Region::Heap, Region::Data, Region::Bss].into_iter().find_map(|r| {
            // (a block's header and the gaps between buffers are not the case's memory)
            // (nor is the byte behind a block's end, which `name_of` still names: eighth pass)
            let inside = |a: u32| names.iter().any(|(_, s, l)| a >= s && a - s < l) || m.heap_blocks().iter().any(|(s, l)| a >= *s && a - *s < *l);
            m.changed_in(snapshot, r).into_iter().find_map(|(a, bytes)| (0..bytes.len() as u32).find(|i| inside(a + i) && names.name_of(m, a + i).is_some_and(|(n, _)| c.try_output(&n).is_some() || c.try_input(&n).is_some())).map(|i| (a + i, vec![bytes[i as usize]])))
        });
        if let Some((a, bytes)) = changed {
            m.set_strict(false);
            m.set_u8(a, bytes[0] ^ 1);
        } else if let Some((n, v)) = result {
            result = Some((n, v ^ 1));
        } else if tape.made() > 0 {
            tape.drop_last = true;
        } else {
            return Verdict::Skip;
        }
    }
    if let Some(fault) = m.fault() {
        return Verdict::Different(format!("the port faulted: {fault:?}"));
    }
    // (a freed block is compared too)
    m.set_strict(false);
    if let Err(e) = tape.finish() {
        return Verdict::Different(e);
    }
    let m = &*m;
    for f in &c.outputs {
        if f.name == "freed" {
            let want: Vec<u32> = f.words().iter().map(|w| recorded_ptr(m, names, w)).collect();
            if want != m.freed() {
                return Verdict::Different(format!("freed: original {want:x?}, port {:x?}", m.freed()));
            }
            continue;
        }
        if let Some((name, v)) = result {
            if f.name == name {
                let want = f.words()[0].value().unwrap_or(0) as u32;
                let v = if name == "ax" { v & 0xffff } else { v };
                if want != v {
                    return Verdict::Different(format!("{name}: original {want:#x}, port {v:#x}"));
                }
                continue;
            }
        }
        if is_result(&f.name) && !f.name.starts_with("heap") {
            continue;
        }
        let Some(a) = names.addr(m, &f.name) else {
            return Verdict::Different(format!("{}: the port has no such block", f.name));
        };
        if let Err(e) = m.compare_words(a, &words(f), names) {
            return Verdict::Different(format!("{}: {e}", f.name));
        }
    }
    if c.try_output("freed").is_none() && !m.freed().is_empty() {
        return Verdict::Different(format!("freed: original nothing, port {:x?}", m.freed()));
    }
    let blocks = c.outputs.iter().filter(|f| f.name.starts_with("heap")).count();
    if c.outputs.iter().any(|f| f.name.starts_with("heap")) && blocks != m.heap_blocks().len() {
        return Verdict::Different(format!("blocks from malloc: original {blocks}, port {}", m.heap_blocks().len()));
    }
    // a buffer the original left alone
    for f in &c.inputs {
        if f.is_null() || c.try_output(&f.name).is_some() || f.name.starts_with("file:") {
            continue;
        }
        let a = addr(m, names, &f.name);
        let w: Vec<W> = words(f)
            .into_iter()
            .map(|w| match &w {
                W::Ref { name, offset } => match global_addr(name) {
                    Some(a) => W::Value { bytes: 4, value: a.wrapping_add(*offset) as u128 },
                    None => w,
                },
                _ => w,
            })
            .collect();
        if let Err(e) = m.compare_words(a, &w, names) {
            return Verdict::Different(format!("{} (which the original leaves alone): {e}", f.name));
        }
    }
    Verdict::Same
}

/// Loads the case, runs the port, compares.
pub(crate) fn run(c: &Case, effect: Option<Effect>, port: impl FnOnce(&mut Mem, &mut Tape, &Names) -> Option<(&'static str, u32)>) -> Verdict {
    let (mut m, names) = match load(c) {
        Ok(i) => i,
        Err(e) => return Verdict::Different(format!("the case does not load: {e}")),
    };
    m.set_strict(true);
    let snapshot = m.snapshot();
    let mut tape = Tape::new(c, &names, effect);
    let result = port(&mut m, &mut tape, &names);
    compare(c, &mut m, &names, &snapshot, result, &mut tape)
}

/// A by-value argument: a number, or a pointer resolved through `names`.
fn arg(c: &Case, m: &Mem, names: &Names, name: &str) -> u32 {
    recorded_ptr(m, names, &c.input(name).words()[0])
}

fn addr(m: &Mem, names: &Names, name: &str) -> u32 {
    names.addr(m, name).unwrap_or_else(|| panic!("the case has no buffer `{name}`"))
}

/// `getorigin` writes the window's origin the case planned.
fn origin_effect(m: &mut Mem, _: &Names, c: &Case, _: usize, k: &Call, args: &[u32]) -> Option<u32> {
    if k.name == "getorigin" {
        let plan = c.input("plan").i16s();
        m.set_i16(args[0], plan[0]);
        m.set_i16(args[1], plan[1]);
    }
    None
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_freeModelTcoord" => |c: &Case| {
            run(c, None, |m, _, i| {
                rnd_main::free_model_tcoord(m, addr(m, i, "node"));
                None
            })
        },
        "SI_modelClearRenderData" => |c: &Case| {
            run(c, None, |m, _, i| {
                rnd_main::model_clear_render_data(m, addr(m, i, "node"), c.input("all").i16());
                None
            })
        },
        "SI_renExit" => |c: &Case| {
            run(c, None, |m, t, i| {
                let keep = arg(c, m, i, "keep");
                rnd_main::ren_exit(m, t, addr(m, i, "n0"), c.input("all").i16(), keep);
                None
            })
        },
        "buildName" => |c: &Case| {
            run(c, None, |m, _, i| {
                rnd_main::build_name(m, addr(m, i, "buf"), addr(m, i, "suffix"));
                None
            })
        },
        "SI_showframe" => |c: &Case| {
            run(c, None, |m, t, _| {
                rnd_main::show_frame(m, t, c.input("frame").i16(), c.input("y").u32());
                None
            })
        },
        "SI_showfield" => |c: &Case| {
            run(c, None, |m, t, _| {
                rnd_main::show_field(m, t, c.input("frame").i16(), c.input("field").i16(), c.input("y").u32());
                None
            })
        },
        "si_fbInit" => |c: &Case| {
            run(c, Some(world_effect), |m, t, _| {
                fbuffer::fb_init(m, t);
                None
            })
        },
        "SI_fbWrite" => |c: &Case| {
            run(c, Some(origin_effect), |m, t, _| {
                fbuffer::fb_write(m, t, c.input("y").u32());
                None
            })
        },
        _ => return init_lookup(name),
    })
}

/// The stand-ins of the dictionary and the instance code (`gen_outer.py`, `world_stubs`):
/// a routine that answers a pointer has it recorded as one more argument; the routine that
/// clears a chapter's work bytes clears them here too.
fn world_effect(m: &mut Mem, names: &Names, _: &Case, _: usize, k: &Call, args: &[u32]) -> Option<u32> {
    match k.name.as_str() {
        "SI_elemlistClearLowByteWorkFlag" => {
            let chapter = m.u32(args[0] + 4).wrapping_add(0x40u32.wrapping_mul(args[1]));
            for i in 0..m.i32(chapter + 4).max(0) as u32 {
                let e = m.u32(chapter + 8) + 0x84 * i;
                m.set_u8(e + 0x5b, 0);
            }
            None
        }
        "SI_modelMasterFromInstance" | "SI_edGetElemFromModel" | "SI_edGetElemFromTexture2D" | "SI_edFindSon" | "SI_edFindNextSon" | "SI_chunkMalloc" => {
            Some(recorded_ptr(m, names, k.args.last().unwrap()))
        }
        _ => None,
    }
}

/// The stand-ins of `SI_image`'s callees (`gen_outer.py`, `image_stubs`): what each writes
/// is planned in the case's `plan_…` buffers, by the number of the frame or field being
/// rendered (one `SI_chunkSaveCurrent` starts each).
fn image_effect(m: &mut Mem, names: &Names, c: &Case, at: usize, k: &Call, args: &[u32]) -> Option<u32> {
    let prior = |name: &str| c.calls[..at].iter().filter(|x| x.name == name).count();
    let pass = prior("SI_chunkSaveCurrent").saturating_sub(1);
    match k.name.as_str() {
        "SI_chunkSaveCurrent" => m.set_u32(args[1], 0x1000 + prior("SI_chunkSaveCurrent") as u32),
        "buildName" => m.put_cstr(args[0], format!("n{}", prior("buildName")).as_bytes()),
        "SI_pictureOpenFile" => {
            let plan = c.input("plan_open").i16s();
            m.set_u32(args[1], 0x7ff0_0060);
            m.set_u32(args[5], 0x7ff0_0080);
            m.set_i16(args[2], plan[4 * pass + 1]);
            m.set_i16(args[2] + 2, plan[4 * pass + 2]);
            m.set_i16(args[2] + 0x0a, plan[4 * pass + 3]);
        }
        "SI_scanlineRead" => {
            let mut line = 0u32;
            for x in &c.calls[..at] {
                line = if x.name == "SI_pictureOpenFile" { 0 } else { line + (x.name == "SI_scanlineRead") as u32 };
            }
            if (line as i32) < c.input("plan_lines").i16() as i32 {
                let width = m.i16(args[2]).max(0) as u32;
                for j in 0..4u32 {
                    let p = m.u32(args[2] + 0x0c + 4 * j);
                    for i in 0..width {
                        m.set_u8(p + i, (16 * j + line + i) as u8);
                    }
                }
            }
        }
        "SI_update" => m.set_i32(g::SI_STATS, c.input("plan_stats").i32s()[pass]),
        "si_cleanUpTexture" => m.set_u8(g::SI_REN_MESSAGE, c.input("plan_msg").u8s()[0]),
        "SI_tagAllocate" | "localtime" => return Some(recorded_ptr(m, names, k.args.last().unwrap())),
        _ => {}
    }
    None
}

/// `si_initDof`'s stand-ins: chunk memory is the heap (each piece a block, in the order
/// the original took them), and `getenv` answers the text the case planned, recorded as
/// one more argument.
fn dof_effect(m: &mut Mem, names: &Names, _: &Case, _: usize, k: &Call, args: &[u32]) -> Option<u32> {
    match k.name.as_str() {
        "SI_chunkMalloc" => Some(m.malloc(args[1])),
        "getenv" => Some(recorded_ptr(m, names, k.args.last().unwrap())),
        _ => None,
    }
}

/// `shadowBuffer`'s stand-ins: chunk memory is the heap; `si_initViewport` and `SI_camera`
/// leave the planned window height and matrices; `si_setBounds` scribbles on the three
/// records the routine has saved.
fn shadow_effect(m: &mut Mem, names: &Names, _: &Case, _: usize, k: &Call, args: &[u32]) -> Option<u32> {
    let plan = |m: &mut Mem, name: &str, to: u32, n: u32| {
        let from = names.addr(m, name).expect("the plan");
        m.copy(to, from, n);
    };
    match k.name.as_str() {
        "SI_chunkMalloc" => return Some(m.malloc(args[1])),
        "SI_chunkSaveCurrent" => m.set_u32(args[1], 0x4321),
        "si_initViewport" => plan(m, "plan_ymax", 0x0879_07bc, 4),
        "SI_camera" => {
            plan(m, "plan_view", 0x0879_082c, 64);
            plan(m, "plan_persp", 0x0879_08ac, 64);
        }
        "si_setBounds" => {
            m.set_fill_bytes(g::SI_REN + 0x200, 0x5a, 8);
            m.set_fill_bytes(g::SI_DATABASE + 0x300, 0xa5, 8);
            m.set_fill_bytes(g::SI_STATS, 0x77, 8);
        }
        _ => {}
    }
    None
}

/// The state the harness's `drand48` starts every case from (the C library's own start).
const DRAND_START: u64 = 0x1234_abcd_330e;

/// `si_initJitter` (on this crate's records): the warning, the calls for chunk memory (one
/// for the table, one per row), every row's pairs, and the record afterwards. The library's
/// `sinf` and `cosf` are replayed from the case.
fn check_init_jitter(c: &Case) -> Verdict {
    let before = c.input("j");
    let mut j = jitter::Jitter { amplitude: f32::from_bits(before.words()[0].value().unwrap_or(0) as u32), ..jitter::Jitter::default() };
    let count = c.input("count").i32();
    let mut rng = Drand48(DRAND_START);
    let warning = jitter::init_jitter(count, &mut j, c.input("G08790d38").i16(), &mut || rng.next());
    crate::verdict((|| {
        let warned = c.calls.iter().filter(|k| k.name == "SI_warning").count();
        if warned != warning.is_some() as usize {
            return Err(format!("warning: original {warned}, port {warning:?}"));
        }
        let rows = c.calls.iter().filter(|k| k.name == "SI_chunkMalloc").count() - 1;
        if rows != j.table.len() {
            return Err(format!("rows: original {rows}, port {}", j.table.len()));
        }
        for (k, row) in j.table.iter().enumerate() {
            let words: Vec<u32> = row.iter().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]).collect();
            crate::prep::same_words(&format!("row {k}"), c.output(&format!("heap{}", k + 1)), &words)?;
        }
        let after = c.output("j").words();
        let table_set = matches!(&after[1], Word::Ref { name, .. } if name == "heap0");
        let next = after[2].value().map(|v| v as u16);
        if !table_set || next != Some(j.next as u16 ^ mutate() as u16) {
            return Err(format!("the record: table {:?}, next {next:?}; port next {}", after[1], j.next));
        }
        Ok(())
    })())
}

fn check_image(c: &Case) -> Verdict {
    run(c, Some(image_effect), |m, t, n| {
        let (lights, roots) = (arg(c, m, n, "lights"), arg(c, m, n, "roots"));
        let camera = addr(m, n, "camera");
        Some(("eax", rnd_main::image(m, t, camera, c.input("worda").u32(), c.input("wordb").u32(), lights, roots)))
    })
}

fn init_lookup(name: &str) -> Option<Check> {
    const W: Option<Effect> = Some(world_effect);
    Some(match name {
        "SI_printWarnUV" => |c: &Case| {
            run(c, W, |m, t, n| {
                initialize::print_warn_uv(m, t, addr(m, n, "name"));
                None
            })
        },
        "testUvPropagation__FP11__SI_LclMapi" => |c: &Case| run(c, W, |m, _, n| Some(("eax", initialize::test_uv_propagation(m, arg(c, m, n, "map"), c.input("count").i32())))),
        "haveRelation" => |c: &Case| {
            run(c, W, |m, t, n| {
                let (d, e) = (arg(c, m, n, "dictp"), arg(c, m, n, "elem"));
                Some(("eax", initialize::have_relation(m, t, d, e, c.input("rel").u32(), c.input("chap").u32())))
            })
        },
        "mdlInitTxtCnt" => |c: &Case| {
            run(c, W, |m, _, n| {
                initialize::mdl_init_txt_cnt(m, addr(m, n, "n0"));
                None
            })
        },
        "SI_initTxtCnt" => |c: &Case| run(c, W, |m, _, n| Some(("eax", initialize::init_txt_cnt(m, addr(m, n, "dict"))))),
        "pushMaterial" => |c: &Case| {
            run(c, W, |m, t, n| {
                let a = |name: &str| arg(c, m, n, name);
                let (d, l, e, mats, maps, t3) = (a("dictp"), a("list"), a("elem"), a("mats"), a("maps"), a("tex3d"));
                initialize::push_material(m, t, d, l, e, c.input("depth").i32(), c.input("nmat").i16(), mats, c.input("nmaps").i16(), maps, t3);
                None
            })
        },
        "SI_propagate" => |c: &Case| {
            run(c, W, |m, t, n| {
                initialize::propagate(m, t, addr(m, n, "dict"));
                None
            })
        },
        "propagateInstances" => |c: &Case| {
            run(c, W, |m, _, n| {
                let root = arg(c, m, n, "root");
                initialize::propagate_instances(m, addr(m, n, "dict"), root);
                None
            })
        },
        "pushInstance" => |c: &Case| {
            run(c, W, |m, _, n| {
                let a = |name: &str| arg(c, m, n, name);
                let (mats, maps, t3) = (a("mats"), a("maps"), a("tex3d"));
                initialize::push_instance(m, addr(m, n, "n0"), c.input("nmat").i32(), mats, c.input("nmaps").i32(), maps, t3, c.input("elemno").i32());
                None
            })
        },
        "setUvWrapFlags_model" => |c: &Case| {
            run(c, W, |m, t, n| {
                let e = arg(c, m, n, "elem");
                initialize::set_uv_wrap_flags_model(m, t, e);
                None
            })
        },
        "SI_setUvWrapFlags" => |c: &Case| {
            run(c, W, |m, t, n| {
                initialize::set_uv_wrap_flags(m, t, addr(m, n, "dict"));
                None
            })
        },
        "SI_initialize" => |c: &Case| run(c, W, |m, t, n| Some(("eax", initialize::initialize(m, t, addr(m, n, "dict"))))),
        "SI_image" => check_image,
        "unirand" => |c: &Case| {
            // every case starts from the harness's fixed generator state
            let mut rng = Drand48(DRAND_START);
            let v = jitter::unirand(&mut || rng.next());
            let got = si_x87::F80::from_f32(if mutate() { f32::from_bits(v.to_bits() ^ 1) } else { v });
            Verdict::from(c.output("st0").same_f80(got), || format!("port {v:?}"))
        },
        // (tenth pass: also through the routine's form on the memory image)
        "si_initJitter" => |c: &Case| {
            check_init_jitter(c).and(|| {
                // the library's answers are replayed once a run of the port: the first
                // form must have used them up, and the second starts on them again
                if let Err(e) = si_x87::trig::replay::finish() {
                    return Verdict::Different(format!("the form on values: {e}"));
                }
                si_x87::trig::replay::start(c.tape());
                crate::pass10::old_jitter(c)
            })
        },
        "shadowBuffer" => |c: &Case| {
            run(c, Some(shadow_effect), |m, t, n| {
                si_render::lights::shadow_buffer(m, t, addr(m, n, "l"));
                None
            })
        },
        "zSample" => |c: &Case| {
            run(c, Some(dof_effect), |m, t, n| {
                si_render::lights::z_sample(m, t, addr(m, n, "l"));
                None
            })
        },
        "si_initDof" => |c: &Case| {
            run(c, Some(dof_effect), |m, t, _| {
                si_render::camera::init_dof(m, t);
                None
            })
        },
        _ => return None,
    })
}
