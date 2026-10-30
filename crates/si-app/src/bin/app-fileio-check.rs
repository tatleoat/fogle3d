// SPDX-License-Identifier: MIT
//! `app-fileio-check <dir>`: `si_app::image::fileswap` (finding 0019, section 13c) against the
//! vectors `re/tools/si-app/gen_fileswap.py` writes: one routine of the original per case,
//! stepped in the emulation harness.
//!
//! Per case: a fresh image, the case's buffers and globals placed in it, the port run with a
//! small outside world that stands in for the C library's stream calls on one file in
//! memory (`fread`, `fwrite`, `fgetc`), for `read(2)` (the recorded answer is handed back
//! and the recorded arguments held) and for `access`. Compared: the result, every buffer,
//! heap block and global the original reports, the file's bytes written, and where the read
//! stream stands afterwards. Nothing here reproduces a function of the original.
//!
//! `EMUCHECK_MUTATE=1` flips a bit of everything the port produced before the comparison;
//! every compared case must then differ.

use si_app::image::fileswap as fs;
use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::vectors::{self, Case, Field, Word};
use std::collections::{BTreeMap, HashMap};

/// The stream handle the port is given.
const STREAM: u32 = 0x7f00_0001;
/// The first heap block's address in the original's run.
const HEAP0: u32 = 0x6200_0008;

struct World<'c> {
    c: &'c Case,
    names: HashMap<String, u32>,
    data: Vec<u8>,
    pos: usize,
    written: Vec<u8>,
    eof: bool,
    usr_tmp: bool,
    next_call: usize,
    diff: Option<String>,
    mutate_calls: bool,
}

impl World<'_> {
    fn differ(&mut self, t: String) {
        if self.diff.is_none() {
            self.diff = Some(t);
        }
    }
}

impl Outside for World<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
        match r.name {
            "fread" => {
                let (buf, size, count) = (a[0], a[1] as usize, a[2] as usize);
                let want = size.saturating_mul(count);
                let n = want.min(self.data.len() - self.pos);
                for i in 0..n {
                    m.set_u8(buf + i as u32, self.data[self.pos + i]);
                }
                self.pos += n;
                if n < want {
                    self.eof = true;
                }
                Ret::eax(if size == 0 { 0 } else { (n / size) as u32 })
            }
            "fwrite" => {
                let (buf, size, count) = (a[0], a[1], a[2]);
                self.written.extend_from_slice(m.bytes(buf, size.wrapping_mul(count)));
                Ret::eax(if size == 0 { 0 } else { count })
            }
            "fgetc" => {
                if self.pos < self.data.len() {
                    self.pos += 1;
                    Ret::eax(self.data[self.pos - 1] as u32)
                } else {
                    self.eof = true;
                    Ret::eax(u32::MAX)
                }
            }
            "access" => {
                let path = m.cstr(a[0]);
                if path != b"/usr/tmp" || a[1] != 6 {
                    self.differ(format!("access({:?}, {})", String::from_utf8_lossy(&path), a[1]));
                }
                Ret::eax(if self.usr_tmp { 0 } else { u32::MAX })
            }
            "read" => {
                let k = self.next_call;
                self.next_call += 1;
                let Some(rec) = self.c.calls.get(k) else {
                    self.differ(format!("call {k}: read after the original's last call"));
                    return Ret::eax(0);
                };
                let flip = self.mutate_calls as u32;
                if rec.name != "read" {
                    self.differ(format!("call {k}: original {}, port read", rec.name));
                }
                let (fd, n) = (a[0] ^ flip, a[2]);
                for (i, got) in [(0usize, fd), (2, n)] {
                    if rec.args.get(i).and_then(|w| w.value()).is_some_and(|w| w as u32 != got) {
                        self.differ(format!("call {k} read argument {i}: original {:?}, port {got:#x}", rec.args[i]));
                    }
                }
                if let Some(Word::Ref { name, offset }) = rec.args.get(1) {
                    if self.names.get(name).map(|b| b + offset) != Some(a[1]) {
                        self.differ(format!("call {k} read: buffer {name}+{offset}, port {:#x}", a[1]));
                    }
                }
                Ret::eax(rec.result.value().unwrap_or(0) as u32)
            }
            other => {
                self.differ(format!("the port calls {other}, which the check does not know"));
                Ret::NONE
            }
        }
    }
}

fn is_scalar(n: &str) -> bool {
    matches!(n, "fd" | "size" | "n" | "count" | "dir" | "fn")
}

/// The address a global field is named for (`G08817f38`).
fn global_at(n: &str) -> Option<u32> {
    let h = n.strip_prefix('G')?;
    if h.len() != 8 {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

fn is_buffer(n: &str) -> bool {
    !(is_scalar(n) || global_at(n).is_some() || n == "eax" || n == "f" || n.starts_with("f.") || n.starts_with("file:") || n.starts_with("heap"))
}

fn words_len(f: &Field) -> u32 {
    f.words().iter().map(|w| w.bytes() as u32).sum()
}

fn store(m: &mut Mem, names: &HashMap<String, u32>, mut at: u32, f: &Field) {
    for w in f.words() {
        match w {
            Word::Value { bytes, value } => {
                for i in 0..*bytes as u32 {
                    m.set_u8(at + i, (*value >> (8 * i)) as u8);
                }
            }
            Word::Ref { name, offset } => m.set_u32(at, names.get(name).copied().unwrap_or(0).wrapping_add(*offset)),
            Word::Unwritten { .. } => {}
        }
        at += w.bytes() as u32;
    }
}

/// The words against the image at `at`; heap pointers of the original are matched to the
/// port's first block.
fn compare(m: &Mem, names: &HashMap<String, u32>, mut at: u32, f: &Field, what: &str) -> Result<(), String> {
    for w in f.words() {
        match w {
            Word::Value { bytes, value } => {
                let got = (0..*bytes as u32).fold(0u128, |a, i| a | ((m.u8(at + i) as u128) << (8 * i)));
                let ok = got == *value || (*bytes == 4 && *value as u32 == HEAP0 && m.heap_blocks().first().is_some_and(|b| b.0 as u128 == got));
                if !ok {
                    return Err(format!("{what} at +{}: original {value:#x}, port {got:#x}", at - names.get(what).copied().unwrap_or(0)));
                }
            }
            Word::Ref { name, offset } => {
                let want = names.get(name).copied().unwrap_or(0).wrapping_add(*offset);
                if m.u32(at) != want {
                    return Err(format!("{what}: pointer to {name}+{offset}, port {:#x}", m.u32(at)));
                }
            }
            Word::Unwritten { .. } => {}
        }
        at += w.bytes() as u32;
    }
    Ok(())
}

fn flip(m: &mut Mem, at: u32) {
    let b = m.u8(at);
    m.set_u8(at, b ^ 1);
}

enum Verdict {
    Same,
    Differ(String),
    Skipped,
}

fn check(c: &Case, mutate: &str) -> Verdict {
    if !c.authoritative() || c.has_note("UNINIT") || c.has_note("OVERRUN") || c.stopped() {
        return Verdict::Skipped;
    }
    // a signalling NaN that a short read left in `si_fvalue`: the emulator's `fld`/`fstp` does
    // not quiet it as the unit does (finding 0011), so the original's answer is not the unit's
    if c.name == "si_hvectorBinaryRead" {
        let w = c.try_output("out").map(|f| f.to_bytes()).unwrap_or_default();
        if w.chunks(4).any(|b| b.len() == 4 && { let x = u32::from_le_bytes([b[0], b[1], b[2], b[3]]); x & 0x7f80_0000 == 0x7f80_0000 && x & 0x007f_ffff != 0 && x & 0x0040_0000 == 0 }) {
            return Verdict::Skipped;
        }
    }
    let mut m = Mem::new();
    si_app::image::init_data(&mut m);
    let mut names: HashMap<String, u32> = HashMap::new();
    // buffers: every pointer argument, from the inputs or, for `:o` ones, the outputs
    let mut sized: BTreeMap<String, u32> = BTreeMap::new();
    for f in c.inputs.iter().chain(&c.outputs) {
        if is_buffer(&f.name) && !f.is_null() {
            let e = sized.entry(f.name.clone()).or_default();
            *e = (*e).max(words_len(f));
        }
    }
    for (n, len) in &sized {
        let at = m.place((*len).max(4), 8);
        names.insert(n.clone(), at);
    }
    for f in &c.inputs {
        if let Some(at) = names.get(&f.name).copied() {
            store(&mut m, &names, at, f);
        } else if let Some(at) = global_at(&f.name) {
            store(&mut m, &names, at, f);
        }
    }
    let mut o = World {
        c,
        names: names.clone(),
        data: c.try_input("f").map(|f| f.to_bytes()).unwrap_or_default(),
        pos: 0,
        written: Vec::new(),
        eof: false,
        usr_tmp: c.try_input("file:/usr/tmp").is_some(),
        next_call: 0,
        diff: None,
        mutate_calls: mutate == "calls",
    };
    let p = |n: &str| names.get(n).copied().unwrap_or(0);
    let u = |n: &str| c.try_input(n).map(|f| f.u32()).unwrap_or(0);
    let mut eax: Option<u32> = None;
    match c.name.as_str() {
        "swapnbyte" => fs::swapnbyte(&mut m, p("p"), u("n")),
        "fix_CustomFcurveKey" => fs::fix_custom_fcurve_key(&mut m, p("buf"), p("pos"), u("dir") as i16),
        "fix_CustomDsoFcurve" => fs::fix_custom_dso_fcurve(&mut m, p("buf"), p("pos"), u("dir") as i16),
        "SI_fix_SI_CustomHeader" => fs::fix_custom_header(&mut m, p("node"), u("dir") as i16),
        "SI_fix_SI_CustomHeaderOld" => fs::fix_custom_header_old(&mut m, p("node"), u("dir") as i16),
        "SI_fix_CustomFcurves" => fs::fix_custom_fcurves(&mut m, p("node"), u("dir") as i16),
        "SI_fix_CustomDsoFcurves" => fs::fix_custom_dso_fcurves(&mut m, p("node"), u("dir") as i16),
        "SI_byteswap_read" => eax = Some(fs::byteswap_read(&mut m, &mut o, u("fd") as i32, p("buf"), u("size"), u("count")) as u32),
        "SI_float_read" => eax = Some(fs::float_read(&mut m, &mut o, u("fd") as i32, p("buf"), u("size"), u("count")) as u32),
        "SI_double_read" => eax = Some(fs::double_read(&mut m, &mut o, u("fd") as i32, p("buf"), u("size"), u("count")) as u32),
        "SI_read_BitHeader" => eax = Some(fs::read_bit_header(&mut m, &mut o, u("fd") as i32, p("buf"), u("size"), u("count")) as u32),
        "SI_Hvector_fwrite" | "SI_FbndChunk_fwrite" | "SI_IbndChunk_fwrite" | "SI_IbndChunk_fread" | "SI_Fcoor2D_fwrite" | "SI_PictChunk_fwrite" | "SI_PictChunk_fread" | "SI_LineChunk_fwrite" => {
            let (b, s, n) = (p("p"), u("size"), u("n"));
            eax = Some(match c.name.as_str() {
                "SI_Hvector_fwrite" => fs::hvector_fwrite(&mut m, &mut o, b, s, n, STREAM),
                "SI_FbndChunk_fwrite" => fs::fbnd_chunk_fwrite(&mut m, &mut o, b, s, n, STREAM),
                "SI_IbndChunk_fwrite" => fs::ibnd_chunk_fwrite(&mut m, &mut o, b, s, n, STREAM),
                "SI_IbndChunk_fread" => fs::ibnd_chunk_fread(&mut m, &mut o, b, s, n, STREAM),
                "SI_Fcoor2D_fwrite" => fs::fcoor2d_fwrite(&mut m, &mut o, b, s, n, STREAM),
                "SI_PictChunk_fwrite" => fs::pict_chunk_fwrite(&mut m, &mut o, b, s, n, STREAM),
                "SI_PictChunk_fread" => fs::pict_chunk_fread(&mut m, &mut o, b, s, n, STREAM),
                _ => fs::line_chunk_fwrite(&mut m, &mut o, b, s, n, STREAM),
            });
        }
        "hvectorBinaryWrite" => eax = Some(fs::hvector_binary_write(&mut m, &mut o, STREAM, p("v")) as u32),
        "si_hvectorBinaryRead" => eax = Some(fs::hvector_binary_read(&mut m, &mut o, STREAM, p("out")) as u32),
        "SI_agetString" => eax = Some(fs::aget_string(&mut m, &mut o, STREAM, p("out")) as u32),
        "SI_sceneversioncheckInstall" => fs::sceneversioncheck_install(&mut m, u("fn")),
        "SI_temporaryDirectory" => eax = Some(fs::temporary_directory(&mut m, &mut o)),
        other => return Verdict::Differ(format!("no port for `{other}`")),
    }
    if let Some(d) = o.diff.take() {
        return Verdict::Differ(d);
    }
    if c.name.ends_with("_read") && o.next_call != c.calls.len() {
        return Verdict::Differ(format!("calls: original {}, port {}", c.calls.len(), o.next_call));
    }
    let mut stream_pos = if o.data.is_empty() && !o.written.is_empty() { o.written.len() } else { o.pos };
    if !o.written.is_empty() || c.name.contains("fwrite") || c.name.contains("Write") {
        stream_pos = o.written.len();
    }
    if mutate == "1" {
        if let Some(v) = eax.as_mut() {
            *v ^= 1;
        }
        for (n, at) in &names {
            if c.try_output(n).is_some_and(|f| !f.words().is_empty()) {
                flip(&mut m, *at);
            }
        }
        for f in &c.outputs {
            if let Some(at) = global_at(&f.name) {
                flip(&mut m, at);
            }
        }
        if let Some(b) = o.written.first_mut() {
            *b ^= 1;
        }
        stream_pos ^= 1;
    }
    if let (Some(Some(w)), Some(v)) = (c.try_output("eax").map(|f| f.words().first().and_then(|w| w.value())), eax) {
        // (a heap pointer is matched to the port's first block)
        let ok = w as u32 == v || (w as u32 == HEAP0 && m.heap_blocks().first().is_some_and(|b| b.0 == v));
        if !ok {
            return Verdict::Differ(format!("eax: original {w:#x}, port {v:#x}"));
        }
    }
    for f in &c.outputs {
        let n = f.name.as_str();
        if n == "eax" || f.is_null() {
            continue;
        }
        if n == "f" {
            if c.name.contains("fwrite") || c.name.contains("Write") {
                let want = f.to_bytes();
                if want != o.written {
                    let at = want.iter().zip(&o.written).position(|(a, b)| a != b).unwrap_or(want.len().min(o.written.len()));
                    return Verdict::Differ(format!("file: original {} bytes, port {}; first difference at {at}", want.len(), o.written.len()));
                }
            }
            continue;
        }
        if n == "f.pos" {
            if f.u32() as usize != stream_pos {
                return Verdict::Differ(format!("file position: original {}, port {stream_pos}", f.u32()));
            }
            continue;
        }
        if n == "f.state" {
            let want = f.words().first().and_then(|w| w.value()).unwrap_or(0) != 0;
            if want != o.eof && !c.name.contains("fwrite") {
                return Verdict::Differ(format!("end-of-file flag: original {want}, port {}", o.eof));
            }
            continue;
        }
        let r = if let Some(rest) = n.strip_prefix("heap") {
            let k: usize = rest.parse().unwrap_or(usize::MAX);
            match m.heap_blocks().get(k).copied() {
                Some((at, _)) => compare(&m, &names, at, f, n),
                None => Err(format!("{n}: the port made fewer blocks ({})", m.heap_blocks().len())),
            }
        } else if let Some(at) = global_at(n) {
            compare(&m, &names, at, f, n)
        } else if let Some(at) = names.get(n).copied() {
            compare(&m, &names, at, f, n)
        } else {
            Err(format!("output `{n}`: no such buffer"))
        };
        if let Err(e) = r {
            return Verdict::Differ(e);
        }
    }
    let n_heap = c.outputs.iter().filter(|f| f.name.starts_with("heap") && f.name[4..].bytes().all(|b| b.is_ascii_digit())).count();
    if n_heap != m.heap_blocks().len() {
        return Verdict::Differ(format!("heap blocks: original {n_heap}, port {}", m.heap_blocks().len()));
    }
    if m.fault().is_some() {
        return Verdict::Differ(format!("the port faulted: {:?}", m.fault()));
    }
    Verdict::Same
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: app-fileio-check <vector directory>");
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let all = vectors::read_dir(std::path::Path::new(&dir)).expect("vector directory");
    println!("{:<30} {:>7} {:>9} {:>9} {:>9}", "routine", "cases", "identical", "different", "not comp.");
    let (mut tc, mut ts, mut td, mut tn) = (0, 0, 0, 0);
    let mut shown = 0;
    for (name, cases) in all {
        let (mut same, mut diff, mut skip) = (0, 0, 0);
        for c in &cases {
            match check(c, &mutate) {
                Verdict::Same => same += 1,
                Verdict::Skipped => skip += 1,
                Verdict::Differ(why) => {
                    diff += 1;
                    if shown < 30 && mutate.is_empty() {
                        shown += 1;
                        eprintln!("{name}: {why}");
                    }
                }
            }
        }
        println!("{:<30} {:>7} {:>9} {:>9} {:>9}", name, cases.len(), same, diff, skip);
        tc += cases.len();
        ts += same;
        td += diff;
        tn += skip;
    }
    println!("{:<30} {:>7} {:>9} {:>9} {:>9}", "total", tc, ts, td, tn);
    if !mutate.is_empty() {
        println!("mutation {mutate}: {td} of {} compared cases differ", tc - tn);
    }
}
