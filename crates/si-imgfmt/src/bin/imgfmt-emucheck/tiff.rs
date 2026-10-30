// SPDX-License-Identifier: MIT
//! Checks for `si_imgfmt::tiff` against the vectors of `re/tools/si-imgfmt/emu_gen4.py`:
//! the TIFF reader on made-up files, with the globals of `tiff.c` handed in at their
//! addresses and compared after the call.
//!
//! These vectors are compared with undefined words allowed (`--partly-undefined`): a pixel
//! or a global that depends on memory nobody set is printed as dashes by the harness, and the
//! port marks the same values as unknown (`None`). A word the port knows and the original
//! has as dashes is a difference; a word the port does not know is not compared.

use crate::util::{mutate, text};
use si_imgfmt::tiff::{Bits, Blk, CNode, CTree, Tiff, TiffPicture, AREA};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

fn word(c: &Case, name: &str, i: usize) -> u128 {
    c.input(name).words()[i].value().unwrap_or(0)
}

fn has(c: &Case, name: &str) -> bool {
    c.try_input(name).is_some()
}

/// A block a pointer word of the case leads to: the whole buffer from the offset on.
fn block_at(c: &Case, w: &Word) -> Blk {
    match w {
        Word::Ref { name, offset } => Some(si_imgfmt::tiff::known(&c.input(name).to_bytes()[*offset as usize..])),
        _ => None,
    }
}

/// A fax tree from a root record (`wt`, `bt`) and the node buffer `nodes`: node j of the
/// buffer is node j + 1 of the arena.
fn tree_in(c: &Case, root: &str) -> CTree {
    let mut t = CTree::default();
    let r = c.input(root).words();
    let kid = |w: &Word| match w {
        Word::Ref { name, offset } if name == "nodes" => Some(*offset as usize / 12 + 1),
        _ => None,
    };
    t.nodes[0] = CNode { value: r[0].value().unwrap_or(0) as u32 as i32, kids: [kid(&r[1]), kid(&r[2])] };
    if let Some(n) = c.try_input("nodes") {
        let w = n.words();
        for j in 0..w.len() / 3 {
            t.nodes.push(CNode { value: w[3 * j].value().unwrap_or(0) as u32 as i32, kids: [kid(&w[3 * j + 1]), kid(&w[3 * j + 2])] });
        }
    }
    t
}

fn bytes8(lo: u128, hi: u128) -> [Option<u8>; 8] {
    let mut b = [None; 8];
    for k in 0..4 {
        b[k] = Some((lo >> (8 * k)) as u8);
        b[4 + k] = Some((hi >> (8 * k)) as u8);
    }
    b
}

/// The globals of the case as the port's state.
pub fn state_in(c: &Case) -> Tiff {
    let mut t = Tiff::default();
    if has(c, "gt") {
        t.code_table = si_imgfmt::tiff::known(&c.input("gt").to_bytes());
    }
    if has(c, "wt") {
        t.white_tree = tree_in(c, "wt");
        t.black_tree = tree_in(c, "bt");
    }
    if has(c, "gi") {
        let w = c.input("gi").words();
        t.strip_byte_cnt = block_at(c, &w[11]);
        t.coutput = block_at(c, &w[5]);
    }
    if has(c, "gc") {
        t.cur_byte = word(c, "gc", 0) as u32 as i32;
        t.lmask = word(c, "gc", 1) as u8;
        t.coutput_pos = word(c, "gc", 4) as u32 as i32;
    }
    if has(c, "gy") {
        t.coutput_len = word(c, "gy", 1) as u32 as i32;
    }
    if has(c, "gz") {
        t.area = c.input("gz").to_bytes();
    }
    if has(c, "gi") {
        t.cinput = block_at(c, &c.input("gi").words()[1]);
    }
    if !has(c, "gx") {
        return t;
    }
    t.compression = word(c, "gi", 3) as u16;
    t.plan_config = word(c, "gi", 6) as u16 as i16;
    t.rows_per_strip = word(c, "gi", 8) as u32 as i32;
    t.samples_per_pix = word(c, "gi", 9) as u16 as i16;
    t.cinput_pos = word(c, "gs", 0) as u32 as i32;
    t.ifd_offset = Some(word(c, "gs", 1) as u32 as i32);
    t.file_pos = word(c, "gs", 2) as u32 as i32;
    if has(c, "gb") {
        t.bit_pos = word(c, "gb", 0) as u32 as i32;
    }
    let gx = |i| word(c, "gx", i) as u32 as i32;
    (t.first_bit, t.last_bit, t.i_269, t.first_byte, t.last_byte) = (gx(6), gx(7), gx(8), gx(9), gx(10));
    t.ret_val = Bits::known(gx(11));
    t.byte_order = gx(15);
    t.cinput_len = gx(16);
    if has(c, "gy") {
        let gy = |i| word(c, "gy", i) as u32 as i32;
        (t.color_len, t.coutput_len, t.img_length, t.img_width) = (gy(0), gy(1), gy(2), gy(3));
    }
    assert_eq!(t.area.len(), AREA);
    let gw = |i| word(c, "gw", i);
    t.strips_per_img = gw(1) as u32 as i32;
    t.width_adj = gw(5) as u32 as i32;
    t.x_res = bytes8(gw(7), gw(8));
    t.y_res = bytes8(gw(9), gw(10));
    t
}

fn ptr(b: &Blk) -> Option<u128> {
    Some(b.is_some() as u128)
}

fn i(v: i32) -> Option<u128> {
    Some(v as u32 as u128)
}

fn res(b: &[Option<u8>; 8], k: usize) -> Option<u128> {
    let mut v = 0u128;
    for j in 0..4 {
        v |= (b[4 * k + j]? as u128) << (8 * j);
    }
    Some(v)
}

/// Compares the words of a field with ours: `None` is not compared; an original word that
/// is a pointer counts as 1 (not null).
fn same_words(what: &str, theirs: Option<&Field>, ours: &[Option<u128>]) -> Verdict {
    same_words_p(what, theirs, ours, &[])
}

/// As [`same_words`]; the words at `ptrs` are pointers: any value but 0 counts as 1.
fn same_words_p(what: &str, theirs: Option<&Field>, ours: &[Option<u128>], ptrs: &[usize]) -> Verdict {
    let Some(theirs) = theirs else { return Verdict::Different(format!("{what}: not in the vector")) };
    let w = theirs.words();
    if w.len() != ours.len() {
        return Verdict::Different(format!("{what}: {} words against {}", w.len(), ours.len()));
    }
    let mut flipped = !mutate();
    for (k, (t, o)) in w.iter().zip(ours).enumerate() {
        let Some(mut o) = *o else { continue };
        let t = match t {
            Word::Value { value, .. } if ptrs.contains(&k) => Some((*value != 0) as u128),
            Word::Value { value, .. } => Some(*value),
            Word::Ref { .. } => Some(1),
            Word::Unwritten { .. } => None,
        };
        if !flipped {
            o ^= 1;
            flipped = true;
        }
        match t {
            Some(t) if t == o => {}
            Some(t) => return Verdict::Different(format!("{what}: word {k} is {t:#x}, port {o:#x}")),
            None => return Verdict::Different(format!("{what}: word {k} is undefined in the original, port {o:#x}")),
        }
    }
    Verdict::Same
}

fn mem_words(m: &[Option<u8>]) -> Vec<Option<u128>> {
    m.iter().map(|b| b.map(|v| v as u128)).collect()
}

/// The globals after the call, as the port left them.
fn same_globals(c: &Case, t: &Tiff) -> Verdict {
    let gi = [
        ptr(&t.bits_per_sample),
        ptr(&t.cinput),
        ptr(&t.color_map),
        Some(t.compression as u128),
        Some(0),
        ptr(&t.coutput),
        Some(t.plan_config as u16 as u128),
        Some(0),
        i(t.rows_per_strip),
        Some(t.samples_per_pix as u16 as u128),
        Some(0),
        ptr(&t.strip_byte_cnt),
        ptr(&t.strip_offset),
    ];
    let gs = [i(t.cinput_pos), t.ifd_offset.and_then(i), i(t.file_pos)];
    let gb = [i(t.bit_pos)];
    let gc = [i(t.cur_byte), Some(t.lmask as u128), Some(0), Some(0), i(t.coutput_pos)];
    let mut gx: Vec<Option<u128>> = c.input("gx").words().iter().map(|w| w.value()).collect();
    gx[6] = i(t.first_bit);
    gx[7] = i(t.last_bit);
    gx[8] = i(t.i_269);
    gx[9] = i(t.first_byte);
    gx[10] = i(t.last_byte);
    gx[11] = t.ret_val.value().and_then(i);
    gx[12] = Some(t.black_tree.nodes[0].value as u32 as u128);
    gx[13] = Some(t.black_tree.nodes[0].kids[0].is_some() as u128);
    gx[14] = Some(t.black_tree.nodes[0].kids[1].is_some() as u128);
    gx[15] = i(t.byte_order);
    gx[16] = i(t.cinput_len);
    let mut gy: Vec<Option<u128>> = c.input("gy").words().iter().map(|w| w.value()).collect();
    gy[0] = i(t.color_len);
    gy[1] = i(t.coutput_len);
    gy[2] = i(t.img_length);
    gy[3] = i(t.img_width);
    let gz: Vec<Option<u128>> = t.area.iter().map(|&b| Some(b as u128)).collect();
    let mut gw: Vec<Option<u128>> = c.input("gw").words().iter().map(|w| w.value()).collect();
    gw[1] = i(t.strips_per_img);
    gw[2] = Some(t.white_tree.nodes[0].value as u32 as u128);
    gw[3] = Some(t.white_tree.nodes[0].kids[0].is_some() as u128);
    gw[4] = Some(t.white_tree.nodes[0].kids[1].is_some() as u128);
    gw[5] = i(t.width_adj);
    gw[7] = res(&t.x_res, 0);
    gw[8] = res(&t.x_res, 1);
    gw[9] = res(&t.y_res, 0);
    gw[10] = res(&t.y_res, 1);
    same_words_p("globals gi", c.try_output("gi"), &gi, &[0, 1, 2, 5, 11, 12])
        .and(|| same_words("globals gs", c.try_output("gs"), &gs))
        .and(|| same_words("globals gb", c.try_output("gb"), &gb))
        .and(|| same_words("globals gc", c.try_output("gc"), &gc))
        .and(|| same_words_p("globals gx", c.try_output("gx"), &gx, &[13, 14]))
        .and(|| same_words("globals gy", c.try_output("gy"), &gy))
        .and(|| same_words("globals gz", c.try_output("gz"), &gz))
        .and(|| same_words_p("globals gw", c.try_output("gw"), &gw, &[3, 4]))
}

/// What went to the status line: (text, level) for each call of the stub.
fn status(c: &Case) -> Vec<(Vec<u8>, i16)> {
    c.calls
        .iter()
        .filter(|k| k.name == "SI_statusDisplayMessage")
        .map(|k| {
            let s = match &k.args[0] {
                Word::Ref { name, .. } => c.try_output(name).map(|f| f.to_bytes()).unwrap_or_default(),
                _ => Vec::new(),
            };
            let s = s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec();
            (s, k.args[1].value().unwrap_or(0) as u16 as i16)
        })
        .collect()
}

fn printed(c: &Case, name: &str) -> Vec<u8> {
    c.try_output(name).map(|f| f.to_bytes()).unwrap_or_default()
}

/// The second switch: with `EMUCHECK_MUTATE=calls` one bit of the calls the port makes (the
/// level of its first status message) is flipped, so every case with such a call must differ.
fn mutate_calls() -> bool {
    std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "calls")
}

fn same_log(c: &Case, t: &Tiff) -> Verdict {
    let st = status(c);
    let mut t = t.clone();
    if mutate_calls() {
        if let Some(m) = t.log.status.first_mut() {
            m.1 ^= 1;
        }
    }
    let t = &t;
    Verdict::from(st == t.log.status, || {
        format!(
            "messages {:?}, port {:?}",
            st.iter().map(|m| String::from_utf8_lossy(&m.0).into_owned()).collect::<Vec<_>>(),
            t.log.status.iter().map(|m| String::from_utf8_lossy(&m.0).into_owned()).collect::<Vec<_>>()
        )
    })
    .and(|| Verdict::from(printed(c, "stdout") == t.log.stdout, || format!("stdout {:?}", c.try_output("stdout").map(text))))
    .and(|| {
        Verdict::from(printed(c, "stderr") == t.log.stderr, || {
            format!("stderr {:?}, port {:?}", c.try_output("stderr").map(text), String::from_utf8_lossy(&t.log.stderr))
        })
    })
}

fn same_picture(c: &Case, p: Option<&TiffPicture>) -> Verdict {
    let rec = c.output("pic").words();
    let Some(p) = p else {
        return Verdict::from(rec.iter().all(|w| matches!(w, Word::Unwritten { .. })), || "the original wrote the picture record".into());
    };
    let head = [Some(p.width as u16 as u128), Some(p.height as u16 as u128), Some(p.ratio as u128), Some(p.fields as u16 as u128), None];
    let mut ours: Vec<Option<u128>> = head.to_vec();
    ours.extend(p.planes.iter().map(ptr));
    let mut v = same_words("picture record", Some(c.output("pic")), &ours);
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    for k in 0..4 {
        v = v.and(|| match (c.try_output(&format!("p{k}")), &p.planes[k]) {
            (Some(f), Some(m)) => same_words(&format!("plane {k}"), Some(f), &mem_words(m)),
            (None, Some(m)) => Verdict::from(n == 0 || n > 65536 || m.len() != n, || format!("plane {k} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("plane {k} missing in the port")),
            (None, None) => Verdict::Same,
        });
    }
    v
}

fn eax(c: &Case) -> Option<u128> {
    c.output("eax").words()[0].value()
}

/// Books a case that agrees with the original on every defined word while the original has
/// undefined ones (the report counts such a case as skipped).
fn booked(c: &Case, v: Verdict) -> Verdict {
    if v == Verdict::Same && c.has_note("UNINIT") {
        crate::PARTLY.with(|m| *m.borrow_mut().entry(c.name.clone()).or_insert(0) += 1);
    }
    v
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureReadTiff" => |c: &Case| booked(c, read_check(c)),
        "SI_pictureWriteTiff" | "SI_pictureWriteTiff#strips" => |c: &Case| booked(c, write_check(c)),
        "codesReadCcitt" | "codesReadCcitt#shipped" => |c: &Case| booked(c, codes_check(c)),
        "bitGet" => |c: &Case| booked(c, bit_check(c)),
        "codeWriteCcitt" => |c: &Case| booked(c, code_write_check(c)),
        "ccittDecompress" => |c: &Case| booked(c, ccitt_check(c)),
        "treeFreeCcitt" => |c: &Case| booked(c, tree_free_check(c)),
        "lzwDecompress" => |c: &Case| booked(c, lzw_check(c)),
        "treeFreeLzw" => |c: &Case| booked(c, tree_free_lzw_check(c)),
        "SI_pictureFull2Field" => |c: &Case| booked(c, full2field_check(c)),
        "SI_pictureField2Full" => |c: &Case| booked(c, field2full_check(c)),
        "SI_getNextFrameInSequence" => |c: &Case| booked(c, next_frame_check(c)),
        _ => return None,
    })
}

fn read_check(c: &Case) -> Verdict {
    {
        {
            let name = crate::util::cstr(&c.input("name").to_bytes()).to_vec();
            let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
            let mac = word(c, "mac", 0) as u32 as i32;
            let mut t = state_in(c);
            let missing = file.is_none();
            let white = c.try_input("file:/usr/softimage/rsrc/white_rl.tiff").map(|f| f.to_bytes());
            let black = c.try_input("file:/usr/softimage/rsrc/black_rl.tiff").map(|f| f.to_bytes());
            match t.read(file.as_deref(), mac, &name, white.as_deref(), black.as_deref()) {
                Ok(r) => {
                    let ret = r.ret.map(|v| v as u32 as u128);
                    same_words("eax", c.try_output("eax"), &[ret])
                        .and(|| same_picture(c, r.picture.as_ref()))
                        .and(|| if missing { Verdict::Same } else { same_log(c, &t) })
                        .and(|| if missing { Verdict::Same } else { same_globals(c, &t) })
                }
                Err(f) => {
                    if std::env::var("EMUCHECK_FAULTS").is_ok() {
                        eprintln!("refused ({}): eax {:?} notes {:?}", f.0, eax(c), c.notes);
                    }
                    crate::FAULTS.with(|m| *m.borrow_mut().entry(format!("{}: {}", c.name, f.0)).or_insert(0) += 1);
                    Verdict::Skip
                }
            }
        }
    }
}

fn write_check(c: &Case) -> Verdict {
    {
        {
            let name = crate::util::cstr(&c.input("name").to_bytes()).to_vec();
            let w = c.input("pic").words();
            let mut pic = crate::util::picture(c, w);
            for (k, n) in ["r", "g", "b"].iter().enumerate() {
                pic.planes[k] = matches!(w[5 + k], Word::Ref { .. }).then(|| c.input(n).to_bytes());
            }
            let mac = word(c, "mac", 0) as u32 as i32;
            let planar = word(c, "planar", 0) as u16 as i16;
            let raw = word(c, "raw", 0) as u16 as i16;
            let date = si_imgfmt::tiff::known(b"2026:10:13 12:34:56\0");
            let mut t = state_in(c);
            let path = match si_picture::soft_open(&String::from_utf8_lossy(&name), Some(".tif"), "wb", &|_| false) {
                si_picture::SoftOpen::Path(p) => p,
                _ => return Verdict::Skip,
            };
            match t.write(&pic, &name, mac, planar, raw, &date) {
                Ok(r) => {
                    let file = c.try_output(&format!("file:{path}"));
                    same_words("eax", c.try_output("eax"), &[Some(r.ret as u32 as u128)])
                        .and(|| match file {
                            Some(f) => {
                                let theirs = f.to_bytes();
                                let ours = mem_words(&r.file);
                                let words: Vec<Option<u128>> = theirs.iter().map(|&b| Some(b as u128)).collect();
                                if words.len() != ours.len() {
                                    return Verdict::Different(format!("file: {} bytes against {}", words.len(), ours.len()));
                                }
                                let mut v = Verdict::Same;
                                let mut flip = mutate();
                                for (k, (a, b)) in words.iter().zip(&ours).enumerate() {
                                    if let Some(mut b) = *b {
                                        if flip {
                                            b ^= 1;
                                            flip = false;
                                        }
                                        if Some(b) != *a {
                                            v = Verdict::Different(format!("file byte {k:#x} is {:#x}, port {b:#x}", a.unwrap()));
                                            break;
                                        }
                                    }
                                }
                                v
                            }
                            None => Verdict::Different("no file written".into()),
                        })
                        .and(|| same_globals(c, &t))
                }
                Err(f) => {
                    if std::env::var("EMUCHECK_FAULTS").is_ok() {
                        eprintln!("refused ({}): eax {:?} notes {:?}", f.0, eax(c), c.notes);
                    }
                    crate::FAULTS.with(|m| *m.borrow_mut().entry(format!("{}: {}", c.name, f.0)).or_insert(0) += 1);
                    Verdict::Skip
                }
            }
        }
    }
}

fn refused(c: &Case, f: si_imgfmt::tiff::Fault) -> Verdict {
    if std::env::var("EMUCHECK_FAULTS").is_ok() {
        eprintln!("refused {} ({}): notes {:?}", c.name, f.0, c.notes);
    }
    crate::FAULTS.with(|m| *m.borrow_mut().entry(format!("{}: {}", c.name, f.0)).or_insert(0) += 1);
    Verdict::Skip
}

fn tree_words(t: &CTree) -> Vec<Option<u128>> {
    t.preorder().into_iter().flat_map(|(v, a, b)| [Some(v as u32 as u128), Some(a as u128), Some(b as u128)]).collect()
}

fn area_words(t: &Tiff) -> Vec<Option<u128>> {
    t.area.iter().map(|&b| Some(b as u128)).collect()
}

fn codes_check(c: &Case) -> Verdict {
    let white = c.try_input("file:/usr/softimage/rsrc/white_rl.tiff").map(|f| f.to_bytes());
    let black = c.try_input("file:/usr/softimage/rsrc/black_rl.tiff").map(|f| f.to_bytes());
    let mut t = state_in(c);
    match t.codes_read_ccitt(white.as_deref(), black.as_deref()) {
        Ok(r) => same_words("eax", c.try_output("eax"), &[Some(r as u32 as u128)])
            .and(|| same_words("code table", c.try_output("gt"), &mem_words(&t.code_table)))
            .and(|| same_words("white tree", c.try_output("wtree"), &tree_words(&t.white_tree)))
            .and(|| same_words("black tree", c.try_output("btree"), &tree_words(&t.black_tree)))
            .and(|| same_words("globals gz", c.try_output("gz"), &area_words(&t)))
            .and(|| same_log(c, &t)),
        Err(f) => refused(c, f),
    }
}

fn gc_words(t: &Tiff) -> Vec<Option<u128>> {
    vec![i(t.cur_byte), Some(t.lmask as u128), Some(0), Some(0), i(t.coutput_pos)]
}

fn bit_check(c: &Case) -> Verdict {
    let mut t = state_in(c);
    let buf = Some(si_imgfmt::tiff::known(&c.input("buf").to_bytes()));
    match t.bit_get(&buf, word(c, "strip", 0) as u32 as i32) {
        Ok(r) => same_words("eax", c.try_output("eax"), &[i(r)]).and(|| same_words("globals gc", c.try_output("gc"), &gc_words(&t))),
        Err(f) => refused(c, f),
    }
}

fn same_coutput(c: &Case, t: &Tiff) -> Verdict {
    match (c.try_output("coutput"), &t.coutput) {
        (Some(f), Some(m)) => same_words("coutput", Some(f), &mem_words(m)),
        (None, None) => Verdict::Same,
        (None, Some(m)) => Verdict::from(m.len() > 0x4000, || "coutput missing in the original".into()),
        (Some(_), None) => Verdict::Different("coutput missing in the port".into()),
    }
}

fn after_coding(c: &Case, t: &Tiff) -> Verdict {
    let mut gy: Vec<Option<u128>> = c.input("gy").words().iter().map(|w| w.value()).collect();
    gy[1] = i(t.coutput_len);
    same_words("globals gc", c.try_output("gc"), &gc_words(t))
        .and(|| same_words("globals gy", c.try_output("gy"), &gy))
        .and(|| same_words("globals gz", c.try_output("gz"), &area_words(t)))
        .and(|| same_coutput(c, t))
}

fn code_write_check(c: &Case) -> Verdict {
    let mut t = state_in(c);
    let text = c.input("text").to_bytes();
    match t.code_write_ccitt(&text, word(c, "n", 0) as u32 as i32, word(c, "flush", 0) as u32 as i32) {
        Ok(r) => same_words("eax", c.try_output("eax"), &[i(r)]).and(|| after_coding(c, &t)),
        Err(f) => refused(c, f),
    }
}

fn ccitt_check(c: &Case) -> Verdict {
    let mut t = state_in(c);
    let buf = Some(si_imgfmt::tiff::known(&c.input("buf").to_bytes()));
    match t.ccitt_decompress(&buf, word(c, "strip", 0) as u32 as i32) {
        Ok(()) => after_coding(c, &t),
        Err(f) => refused(c, f),
    }
}

fn tree_free_check(c: &Case) -> Verdict {
    let mut t = CTree::default();
    let kid = |w: &Word| match w {
        Word::Ref { name, .. } => name[1..].parse::<usize>().ok(),
        _ => None,
    };
    t.nodes.clear();
    for j in 0..16 {
        let w = c.input(&format!("n{j}")).words();
        t.nodes.push(CNode { value: w[0].value().unwrap_or(0) as u32 as i32, kids: [kid(&w[1]), kid(&w[2])] });
    }
    let mut ours: Vec<String> = t.free_order(0).into_iter().map(|j| format!("n{j}")).collect();
    if mutate() {
        ours.reverse();
        ours.push("mutated".into());
    }
    let theirs: Vec<String> = c
        .try_output("freed")
        .map(|f| f.words().iter().filter_map(|w| if let Word::Ref { name, .. } = w { Some(name.clone()) } else { None }).collect())
        .unwrap_or_default();
    Verdict::from(theirs == ours, || format!("freed {theirs:?}, port {ours:?}"))
}

fn lzw_check(c: &Case) -> Verdict {
    let mut t = state_in(c);
    // the globals of the LZW decoder the generator hands in (all at their start-up values
    // but cinput_pos)
    t.code_len = word(c, "gx", 17) as u32 as i32;
    t.word1 = word(c, "gw", 6) as u16;
    t.word2 = (word(c, "gw", 6) >> 16) as u16;
    match t.lzw_decompress(word(c, "strip", 0) as u32 as i32) {
        Ok(()) => {
            let nb = t.nb_entry() as usize;
            let lens: Vec<Option<u128>> = t.length_tbl[..nb.min(0x1000)].iter().map(|&l| Some(l as u128)).collect();
            let mut strs = Vec::new();
            for k in 0..nb.min(0x1000) {
                if let Some(m) = &t.string_tbl[k] {
                    strs.extend(mem_words(&m[..t.length_tbl[k] as usize]));
                }
            }
            let mut tree = Vec::new();
            let mut stack: Vec<usize> = t.string_tree.into_iter().collect();
            while let Some(n) = stack.pop() {
                let nd = &t.lzw_nodes[n];
                tree.push(Some(nd.value as u128));
                tree.extend(nd.kids.iter().map(|k| Some(k.is_some() as u128)));
                for k in nd.kids.iter().rev().flatten() {
                    stack.push(*k);
                }
            }
            let mut gx: Vec<Option<u128>> = c.input("gx").words().iter().map(|w| w.value()).collect();
            gx[0] = Some(t.code as u128);
            gx[1] = Some(t.out_str.is_some() as u128);
            gx[2] = Some(t.old_code as u128);
            gx[3] = Some(t.str_len as u128);
            gx[4] = Some(t.out_len as u128);
            gx[5] = Some(t.i_260 as u128);
            gx[17] = i(t.code_len);
            let mut gw: Vec<Option<u128>> = c.input("gw").words().iter().map(|w| w.value()).collect();
            gw[0] = Some(t.string_tree.is_some() as u128);
            gw[6] = Some(t.word1 as u128 | (t.word2 as u128) << 16);
            let mut gs: Vec<Option<u128>> = c.input("gs").words().iter().map(|w| w.value()).collect();
            gs[0] = i(t.cinput_pos);
            same_words("nb", c.try_output("nb"), &[Some(nb as u128)])
                .and(|| same_words("lens", c.try_output("lens"), &lens))
                .and(|| same_words("strs", c.try_output("strs"), &strs))
                .and(|| same_words("tree", c.try_output("tree"), &tree))
                .and(|| same_coutput(c, &t))
                .and(|| same_words_p("globals gx", c.try_output("gx"), &gx, &[1]))
                .and(|| same_words_p("globals gw", c.try_output("gw"), &gw, &[0]))
                .and(|| same_words("globals gs", c.try_output("gs"), &gs))
                .and(|| same_words("globals gz", c.try_output("gz"), &area_words(&t)))
        }
        Err(f) => refused(c, f),
    }
}

// ---------------------------------------------------------------- pict.c

fn pic_in(c: &Case, rec: &str) -> Option<si_picture::Picture> {
    let w = c.try_input(rec)?.words();
    if w.is_empty() {
        return None;
    }
    let mut p = crate::util::picture(c, w);
    for k in 0..4 {
        p.planes[k] = match &w[5 + k] {
            Word::Ref { name, .. } => Some(c.input(name).to_bytes()),
            _ => None,
        };
    }
    Some(p)
}

fn same_pic_out(c: &Case, rec: &str, p: &si_picture::Picture) -> Verdict {
    let head = [Some(p.width as u16 as u128), Some(p.height as u16 as u128), Some(p.ratio.to_bits() as u128), Some(p.fields as u16 as u128), Some(p.pad as u16 as u128)];
    let mut ours: Vec<Option<u128>> = head.to_vec();
    ours.extend(p.planes.iter().map(|q| Some(q.is_some() as u128)));
    let mut v = same_words(&format!("record {rec}"), c.try_output(rec), &ours);
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    for k in 0..4 {
        v = v.and(|| match (c.try_output(&format!("{rec}{k}")), &p.planes[k]) {
            (Some(f), Some(m)) => same_words(&format!("{rec} plane {k}"), Some(f), &m.iter().map(|&b| Some(b as u128)).collect::<Vec<_>>()),
            (None, Some(_)) => Verdict::from(n == 0 || n > 65536, || format!("{rec} plane {k} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("{rec} plane {k} missing in the port")),
            (None, None) => Verdict::Same,
        });
    }
    v
}

fn refused_with(c: &Case, text: &str, recs: &[&str]) -> Verdict {
    let st = status(c);
    let level = if mutate_calls() { 2 } else { 3 };
    Verdict::from(st.len() == 1 && st[0].0 == text.as_bytes() && st[0].1 == level, || format!("messages {st:?}"))
        .and(|| same_words("eax", c.try_output("eax"), &[Some(0)]))
        .and(|| {
            let untouched = recs.iter().all(|r| c.try_output(r).is_none_or(|f| f.words().iter().all(|w| matches!(w, Word::Unwritten { .. }))));
            Verdict::from(untouched, || "the original wrote a record".into())
        })
}

fn pict_fault(c: &Case, f: si_imgfmt::effect::Fault) -> Verdict {
    if std::env::var("EMUCHECK_FAULTS").is_ok() {
        eprintln!("refused {} ({f:?}): notes {:?}", c.name, c.notes);
    }
    crate::FAULTS.with(|m| *m.borrow_mut().entry(format!("{}: {}", c.name, match f {
        si_imgfmt::effect::Fault::NullPlane => "pict: a plane read without a test is absent",
        si_imgfmt::effect::Fault::ShortPlane => "pict: a plane shorter than the picture",
        si_imgfmt::effect::Fault::BadSize => "pict: a size the port does not allocate",
        si_imgfmt::effect::Fault::DivideError => "pict: an integer division by zero",
        si_imgfmt::effect::Fault::Uninitialised => "pict: a format argument the routine does not pass",
        si_imgfmt::effect::Fault::Exit => "pict: the program ends",
    })).or_insert(0) += 1);
    Verdict::Skip
}

fn full2field_check(c: &Case) -> Verdict {
    use si_imgfmt::pict::{full_to_field, Fields};
    let Some(full) = pic_in(c, "full") else { return Verdict::Skip };
    match full_to_field(&full) {
        Ok(Fields::Done((even, odd))) => same_words("eax", c.try_output("eax"), &[Some(1)]).and(|| same_pic_out(c, "even", &even)).and(|| same_pic_out(c, "odd", &odd)),
        Ok(Fields::Refused(t)) => refused_with(c, t, &["even", "odd"]),
        Err(f) => pict_fault(c, f),
    }
}

fn field2full_check(c: &Case) -> Verdict {
    use si_imgfmt::pict::{field_to_full, Fields};
    let Some(a) = pic_in(c, "a") else { return Verdict::Skip };
    let b = pic_in(c, "b");
    match field_to_full(&a, b.as_ref(), word(c, "flag", 0) as u16 as i16) {
        Ok(Fields::Done(out)) => same_words("eax", c.try_output("eax"), &[Some(1)]).and(|| same_pic_out(c, "out", &out)),
        Ok(Fields::Refused(t)) => refused_with(c, t, &["out"]),
        Err(f) => pict_fault(c, f),
    }
}

fn next_frame_check(c: &Case) -> Verdict {
    use si_imgfmt::pict::{next_frame, Frame, Sequence};
    let q = c.input("seq").words();
    let v = |i: usize| q[i].value().unwrap_or(0);
    let mut s = Sequence {
        first: v(0) as u32 as i32,
        last: v(1) as u32 as i32,
        step: v(2) as u32 as i32,
        current: v(3) as u32 as i32,
        kind: v(4) as u32 as i32,
        cycle_pos: v(5) as u32 as i32,
        cycle: v(6) as u32 as i32,
        stereo: v(7) as u16 as i16,
        just_primed: v(8) as u16 as i16,
        name: matches!(c.input("nm").words()[0], Word::Ref { .. }).then(|| crate::util::cstr(&c.input("old").to_bytes()).to_vec()),
    };
    let name = (!c.input("name").is_null()).then(|| crate::util::cstr(&c.input("name").to_bytes()).to_vec());
    let r = match next_frame(&mut s, name.as_deref(), word(c, "first", 0) as u32 as i32, word(c, "last", 0) as u32 as i32, word(c, "step", 0) as u32 as i32, word(c, "type", 0) as u32 as i32, word(c, "stereo", 0) as u16 as i16) {
        Ok(r) => r,
        Err(f) => return pict_fault(c, f),
    };
    let ret = c.output("ptr").words()[0].clone();
    let ret_ok = match (&r, &ret) {
        (Frame::Null, Word::Value { value: 0, .. }) => Verdict::Same,
        (Frame::Text("first > last"), Word::Value { value, .. }) if *value == 0x086fafc6 => Verdict::Same,
        (Frame::Name(n), Word::Ref { name, offset: 0 }) if name == "out" => {
            let out = c.output("out").to_bytes();
            let mut theirs = crate::util::cstr(&out).to_vec();
            let mut ours = n.clone();
            if mutate() {
                ours.push(b'!');
            }
            theirs.truncate(64);
            Verdict::from(theirs == ours, || format!("name {:?}, port {:?}", String::from_utf8_lossy(&theirs), String::from_utf8_lossy(&ours)))
        }
        _ => Verdict::Different(format!("returned {ret:?}, port {r:?}")),
    };
    let seq = [s.first, s.last, s.step, s.current, s.kind, s.cycle_pos, s.cycle].iter().map(|&x| i(x)).chain([Some(s.stereo as u16 as u128), Some(s.just_primed as u16 as u128)]).collect::<Vec<_>>();
    ret_ok.and(|| same_words("sequence", c.try_output("seq"), &seq)).and(|| {
        let theirs = c.try_output("name").map(|f| f.to_bytes());
        let ours = s.name.clone();
        Verdict::from(theirs == ours, || format!("static name {theirs:?}, port {ours:?}"))
    })
}

/// The cases in which the original ran out of its instruction budget (the harness stops
/// them, and the report does not compare them): how many of them the port says never end.
pub fn never_ends(dir: &std::path::Path) {
    use std::io::BufRead;
    for (file, run) in [("ccittDecompress.vec", ccitt_endless as fn(&Case) -> Option<&'static str>), ("SI_pictureReadTiff.vec", read_endless)] {
        let Ok(fh) = std::fs::File::open(dir.join(file)) else { continue };
        let (mut stopped, mut agree) = (0, 0);
        let mut others: std::collections::BTreeMap<String, usize> = Default::default();
        for line in std::io::BufReader::new(fh).lines().map_while(Result::ok) {
            let Ok(Some(c)) = Case::parse(&line) else { continue };
            if !c.notes.iter().any(|n| n.starts_with("STOP:budget")) {
                continue;
            }
            stopped += 1;
            match run(&c) {
                Some(why) if why.contains("never") || why.contains("for ever") => agree += 1,
                other => *others.entry(format!("{other:?}")).or_insert(0) += 1,
            }
        }
        if stopped > 0 {
            println!("  {file}: {stopped} cases ran out of budget in the original; the port says {agree} of them never end; others {others:?}");
        }
    }
}

fn ccitt_endless(c: &Case) -> Option<&'static str> {
    let mut t = state_in(c);
    let buf = Some(si_imgfmt::tiff::known(&c.input("buf").to_bytes()));
    t.ccitt_decompress(&buf, word(c, "strip", 0) as u32 as i32).err().map(|f| f.0)
}

fn read_endless(c: &Case) -> Option<&'static str> {
    let name = crate::util::cstr(&c.input("name").to_bytes()).to_vec();
    let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
    let white = c.try_input("file:/usr/softimage/rsrc/white_rl.tiff").map(|f| f.to_bytes());
    let black = c.try_input("file:/usr/softimage/rsrc/black_rl.tiff").map(|f| f.to_bytes());
    let mut t = state_in(c);
    t.read(file.as_deref(), word(c, "mac", 0) as u32 as i32, &name, white.as_deref(), black.as_deref()).err().map(|f| f.0)
}

fn tree_free_lzw_check(c: &Case) -> Verdict {
    let kid = |w: &Word| match w {
        Word::Ref { name, .. } => name[1..].parse::<usize>().ok(),
        _ => None,
    };
    let mut t = Tiff::default();
    for j in 0..16 {
        let w = c.input(&format!("n{j}")).words();
        t.lzw_nodes.push(si_imgfmt::tiff::LNode { value: w[0].value().unwrap_or(0) as u32, kids: [kid(&w[1]), kid(&w[2]), kid(&w[3]), kid(&w[4])], freed: false });
    }
    let order = match t.tree_free_lzw(Some(0)) {
        Ok(o) => o,
        Err(f) => return refused(c, f),
    };
    let mut ours: Vec<String> = order.into_iter().map(|j| format!("n{j}")).collect();
    if mutate() {
        ours.push("mutated".into());
    }
    let theirs: Vec<String> = c
        .try_output("freed")
        .map(|f| f.words().iter().filter_map(|w| if let Word::Ref { name, .. } = w { Some(name.clone()) } else { None }).collect())
        .unwrap_or_default();
    Verdict::from(theirs == ours, || format!("freed {theirs:?}, port {ours:?}"))
}
