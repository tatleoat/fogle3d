// SPDX-License-Identifier: MIT
//! Check of `si_imgfmt::compose::picture_composite_new` against the vectors of
//! `re/tools/img1/gen_compose.py`. The original runs with the window system, the status line,
//! the picture reader and writer, the header read and the file removal replaced by recorded
//! stand-ins; the port runs over a [`ComposeHost`] that holds each of its calls against the next
//! recorded one and answers from the same case data. Standard error is compared as a whole.
//! The depth files are not part of the cases.
//!
//! `EMUCHECK_MUTATE=1` flips the port's result; `EMUCHECK_MUTATE=calls` one bit of the first
//! compared argument (or the first compared text) of the port's calls.

use crate::glue::{calls_mutation, c_str, Shared, Tape};
use crate::util::mutate;
use si_imgfmt::compose::{picture_composite_new, Compose, ComposeHost, LayerInfo};
use si_picture::Picture;
use si_x87::vectors::{Call, Case, Check, Verdict, Word};
use std::cell::RefCell;
use std::rc::Rc;

const HDR: usize = 0x58;
const LP: usize = 256;
const KMAX: usize = 8;

struct Host {
    tape: Shared,
    ctl: Vec<u32>,
    hdr: Vec<u8>,
    lp: Vec<u8>,
    screen: (i32, i32),
    colors: (u16, u16),
    tablet: (bool, i32),
    stderr: Vec<u8>,
    k_win: u32,
    k_btn: u32,
    k_fread: usize,
    k_read: usize,
}

fn some(v: i32) -> Option<u32> {
    Some(v as u32)
}

fn dump(p: &Picture) -> Vec<u32> {
    let mut v = vec![p.width as u16 as u32, p.height as u16 as u32, p.ratio.to_bits(), p.fields as u16 as u32, p.pad as u16 as u32];
    v.push(p.planes.iter().enumerate().map(|(i, m)| (m.is_some() as u32) << i).sum());
    for m in p.planes.iter().flatten() {
        v.extend(m.iter().map(|&b| b as u32));
    }
    v
}

/// The words of a recorded entry; a word the original left undefined (the pad of a picture)
/// comes back as `None` and matches anything.
fn words_of(k: &Call) -> Vec<Option<u32>> {
    k.args.iter().map(|w| w.value().map(|v| v as u32)).collect()
}

fn same_words(theirs: &[Option<u32>], ours: &[u32]) -> bool {
    theirs.len() == ours.len() && theirs.iter().zip(ours).all(|(t, o)| t.is_none_or(|t| t == *o))
}

impl Host {
    fn t(&self) -> std::cell::RefMut<'_, Tape> {
        self.tape.borrow_mut()
    }

    fn plain(&mut self, name: &str, args: &[Option<u32>]) -> Option<Call> {
        self.t().take(name, args).map(|x| x.1)
    }

    fn with_text(&mut self, name: &str, args: &[Option<u32>], text: &str) -> Option<Call> {
        self.t().take_text(name, args, text.as_bytes())
    }

    fn picture_dump(&mut self, name: &str, args: &[Option<u32>], p: &Picture) {
        let mut t = self.t();
        let Some((pre, _)) = t.take(name, args) else { return };
        let mut ours = dump(p);
        if calls_mutation() && !t.flipped {
            t.flipped = true;
            ours[0] ^= 1;
        }
        let theirs = pre.iter().find(|k| k.name == "pre:pic").map(words_of);
        if !theirs.as_ref().is_some_and(|t| same_words(t, &ours)) {
            let cut = |v: &[u32]| v.iter().take(40).copied().collect::<Vec<_>>();
            t.fail(format!("{name} picture: the original {:?}, the port {:?}", theirs.as_deref().map(|t| t.iter().take(40).copied().collect::<Vec<_>>()), cut(&ours)));
        }
    }

    fn ctl_bit(&self, i: usize, k: u32) -> bool {
        (self.ctl[i] >> k.min(31)) & 1 != 0
    }

    fn picture(&self, k: usize) -> Picture {
        let r = &self.lp[LP * k.min(KMAX - 1)..][..LP];
        let w = i16::from_le_bytes([r[0], r[1]]);
        let h = i16::from_le_bytes([r[2], r[3]]);
        let ratio = f32::from_bits(u32::from_le_bytes([r[4], r[5], r[6], r[7]]));
        let fields = i16::from_le_bytes([r[8], r[9]]);
        let pad = i16::from_le_bytes([r[10], r[11]]);
        let alpha = r[12] != 0;
        let n = (w as i32 * h as i32).max(0) as usize;
        let plane = |i: usize| Some(r[16 + 36 * i..][..n].to_vec());
        Picture { width: w, height: h, ratio, fields, pad, planes: [plane(0), plane(1), plane(2), if alpha { plane(3) } else { None }] }
    }
}

impl ComposeHost for Host {
    fn status(&mut self, text: &str, level: i32) {
        self.with_text("SI_statusDisplayMessage", &[None, some(level)], text);
    }
    fn stderr(&mut self, text: &str) {
        self.stderr.extend_from_slice(text.as_bytes());
    }
    fn screen_size(&mut self) -> (i32, i32) {
        self.screen
    }
    fn ui_colors(&mut self) -> (u16, u16) {
        self.colors
    }
    fn tablet_active(&mut self) -> bool {
        self.tablet.0
    }
    fn tablet_button(&mut self) -> i32 {
        self.tablet.1
    }
    fn tablet_get_button_up_event(&mut self) {
        self.plain("SI_tabletGetButtonUpEvent", &[]);
    }
    fn no_border(&mut self) {
        self.plain("noborder", &[]);
    }
    fn pref_position(&mut self, x1: i32, x2: i32, y1: i32, y2: i32) {
        self.plain("prefposition", &[some(x1), some(x2), some(y1), some(y2)]);
    }
    fn win_open(&mut self, title: &str) -> i32 {
        let k = self.with_text("winopen", &[None], title);
        self.k_win += 1;
        Tape::result(&k) as i32
    }
    fn win_close(&mut self, id: i32) {
        self.plain("winclose", &[some(id)]);
    }
    fn win_title(&mut self, title: &str) {
        self.with_text("wintitle", &[None], title);
    }
    fn color(&mut self, c: i32) {
        self.plain("color", &[some(c)]);
    }
    fn clear(&mut self) {
        self.plain("clear", &[]);
    }
    fn get_draw_mode(&mut self) -> i32 {
        let k = self.plain("getdrawmode", &[]);
        Tape::result(&k) as i32
    }
    fn draw_mode(&mut self, mode: i32) {
        self.plain("drawmode", &[some(mode)]);
    }
    fn line_width(&mut self, w: i32) {
        self.plain("linewidth", &[some(w)]);
    }
    fn bgn_closed_line(&mut self) {
        self.plain("bgnclosedline", &[]);
    }
    fn v2i(&mut self, v: [i32; 2]) {
        self.plain("v2i", &[some(v[0]), some(v[1])]);
    }
    fn end_closed_line(&mut self) {
        self.plain("endclosedline", &[]);
    }
    fn rgb_mode(&mut self) {
        self.plain("RGBmode", &[]);
    }
    fn gconfig(&mut self) {
        self.plain("gconfig", &[]);
    }
    fn rgb_color(&mut self, r: i32, g: i32, b: i32) {
        self.plain("RGBcolor", &[some(r), some(g), some(b)]);
    }
    fn getbutton(&mut self, button: i32) -> bool {
        let k = self.plain("getbutton", &[some(button)]);
        self.k_btn += 1;
        // once the tape has failed there is nothing left to answer: end the wait for the button
        self.t().bad.is_some() || Tape::result(&k) != 0
    }
    fn reset_back_buffer_and_refresh(&mut self) {
        self.plain("resetBackBufferAndRefresh", &[]);
    }
    fn picture_display(&mut self, pic: &Picture) {
        self.picture_dump("SI_pictureDisplay", &[None], pic);
    }
    fn soft_open_read(&mut self, name: &str) -> Option<Vec<u8>> {
        let k = self.with_text("SI_softOpen", &[None], name);
        if Tape::result(&k) == 0 {
            return None;
        }
        let r = self.plain("fread", &[None, some(HDR as i32), some(1), None]);
        let ok = Tape::result(&r) == 1;
        let at = self.k_fread.min(KMAX - 1);
        self.k_fread += 1;
        if !ok {
            return Some(Vec::new());
        }
        self.plain("fclose", &[None]);
        Some(self.hdr[HDR * at..][..HDR].to_vec())
    }
    fn read_picture(&mut self, name: &str) -> Option<Picture> {
        let k = self.with_text("SI_pictureRead", &[None, None], name);
        let at = self.k_read;
        self.k_read += 1;
        (Tape::result(&k) != 0).then(|| self.picture(at))
    }
    fn write_picture(&mut self, name: &str, pic: &Picture, comment: &str) {
        let mut t = self.t();
        let Some((pre, _)) = t.take("SI_pictureWrite", &[None, None, None]) else { return };
        let get = |tag: &str| pre.iter().find(|k| k.name == tag).map(words_of);
        let text = |v: Option<Vec<Option<u32>>>| v.map(|v| v.iter().map(|b| b.unwrap_or(0)).take_while(|&b| b != 0).map(|b| b as u8).collect::<Vec<u8>>());
        let mut ours = name.as_bytes().to_vec();
        if calls_mutation() && !t.flipped {
            t.flipped = true;
            ours.push(b'!');
        }
        if text(get("pre:text")).as_deref() != Some(&ours[..]) {
            let w = text(get("pre:text")).map(|v| String::from_utf8_lossy(&v).into_owned());
            t.fail(format!("SI_pictureWrite name: the original {w:?}, the port {:?}", String::from_utf8_lossy(&ours)));
        }
        if text(get("pre:comment")).as_deref() != Some(comment.as_bytes()) {
            t.fail("SI_pictureWrite comment differs".into());
        }
        if !get("pre:pic").is_some_and(|t| same_words(&t, &dump(pic))) {
            t.fail(format!("SI_pictureWrite picture of {name} differs: the original {:?}", get("pre:pic").map(|v| v.into_iter().take(40).collect::<Vec<_>>())));
        }
    }
    fn fopen_read(&mut self, _name: &str) -> Option<Vec<u8>> {
        None
    }
    fn fopen_create(&mut self, _name: &str) -> bool {
        false
    }
    fn write_file(&mut self, _name: &str, _bytes: &[u8]) {}
    fn remove(&mut self, _name: &str) {}
    fn rename(&mut self, _from: &str, _to: &str) {}
    fn getpid(&mut self) -> i32 {
        0
    }
}

fn scalar(c: &Case, name: &str) -> u32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32
}

/// The layer records of the case: (the port's, the byte offsets of each record's words).
fn layers_of(c: &Case, n: usize) -> Vec<LayerInfo> {
    let f = c.input("layers");
    let mut at = 0usize;
    let mut w: Vec<(usize, &Word)> = Vec::new();
    for x in f.words() {
        w.push((at, x));
        at += x.bytes() as usize;
    }
    let val = |layer: usize, off: usize| -> u128 { w.iter().find(|(o, _)| *o == layer * 0x60 + off).and_then(|(_, x)| x.value()).unwrap_or(0) };
    (0..n)
        .map(|i| {
            let name = w.iter().find(|(o, _)| *o == i * 0x60).map(|(_, x)| match x {
                Word::Ref { name, .. } => String::from_utf8_lossy(c_str(&c.input(name).to_bytes())).into_owned(),
                _ => String::new(),
            });
            let fl = |off| f32::from_bits(val(i, off) as u32);
            let iv = |off| val(i, off) as u32 as i32;
            LayerInfo {
                name: name.unwrap_or_default(),
                factor0: fl(0xc),
                factor: fl(0x10),
                factor1: fl(0x14),
                start: iv(0x20),
                cur: iv(0x24),
                x0: iv(0x28),
                x: iv(0x2c),
                x1: iv(0x30),
                y0: iv(0x38),
                y: iv(0x3c),
                y1: iv(0x40),
                mode: iv(0x48),
                blur: iv(0x54),
                sequence: val(i, 0x5a) as u16 as i16,
                full: val(i, 0x5e) as u16 as i16,
            }
        })
        .collect()
}

fn compose(c: &Case) -> Verdict {
    let nl = scalar(c, "v_nl") as i32;
    let a15 = scalar(c, "v_a15");
    let a16 = scalar(c, "v_a16");
    let a17 = scalar(c, "v_a17");
    let name_ref = c.input("p_name").words()[0].clone();
    let name = match &name_ref {
        Word::Ref { name, .. } => String::from_utf8_lossy(c_str(&c.input(name).to_bytes())).into_owned(),
        _ => return crate::pict::refused(c, "a null name"),
    };
    let job = Compose {
        name,
        ratio: f32::from_bits(scalar(c, "v_ratio")),
        first: scalar(c, "v_first") as i32,
        last: scalar(c, "v_last") as i32,
        step: scalar(c, "v_step") as i32,
        repeat: scalar(c, "v_repeat") as i32,
        width: scalar(c, "v_w") as u16 as i16,
        height: scalar(c, "v_h") as u16 as i16,
        fill: [scalar(c, "v_f0") as i32, scalar(c, "v_f1") as i32, scalar(c, "v_f2") as i32],
        numbered: a15 as u16 as i16,
        keep_alpha: (a15 >> 16) as u16 as i16,
        verbose: a16 as u16 as i16,
        display: (a16 >> 16) as u16 as i16,
        batch: a17 as u16 as i16,
        depth: (a17 >> 16) as u16 as i16,
        field_mode: scalar(c, "v_fm") as i32,
    };
    if job.depth != 0 {
        return crate::pict::refused(c, "depth files are not part of the cases");
    }
    let mut layers = layers_of(c, nl.max(0) as usize);
    // the original calls remove and rename on names it never set when the depth is off: not compared
    let calls: Vec<Call> = c.calls.iter().filter(|k| k.name != "remove" && k.name != "rename").cloned().collect();
    let tape = Rc::new(RefCell::new(Tape { calls, idx: 0, bad: None, flipped: false }));
    let u32s = |n: &str| c.input(n).words().iter().map(|w| w.value().unwrap_or(0) as u32).collect::<Vec<_>>();
    let one = |n: &str| u32s(n)[0];
    let mut host = Host {
        tape: tape.clone(),
        ctl: u32s("ctl"),
        hdr: c.input("hdr").to_bytes(),
        lp: c.input("lp").to_bytes(),
        screen: (one("xmax") as u16 as i16 as i32, one("ymax") as u16 as i16 as i32),
        colors: (one("dark") as u16, one("wtitle") as u16),
        tablet: (one("tact") != 0, one("tbtn") as i32),
        stderr: Vec::new(),
        k_win: 0,
        k_btn: 0,
        k_fread: 0,
        k_read: 0,
    };
    let _ = (host.ctl_bit(0, 0), host.k_win, host.k_btn);
    let got = picture_composite_new(&job, &mut layers, &mut host);
    let ok = match got {
        Ok(ok) => ok,
        Err(f) => return crate::pict::refused(c, &format!("{f:?}")),
    };
    {
        let t = tape.borrow();
        if let Some(why) = &t.bad {
            return Verdict::Different(why.clone());
        }
        if t.idx != t.calls.len() {
            return Verdict::Different(format!("{} recorded calls were not made by the port, the next is {}", t.calls.len() - t.idx, t.calls[t.idx].name));
        }
    }
    let theirs = c.try_output("eax").and_then(|f| f.words().first().and_then(|w| w.value()));
    let ours = ok as u32 ^ mutate() as u32;
    let v = match theirs {
        Some(t) if t as u32 == ours => Verdict::Same,
        Some(t) => Verdict::Different(format!("result: the original {t:#x}, the port {ours:#x}")),
        None => Verdict::Different("no result in the original".into()),
    };
    v.and(|| {
        let err: Vec<u8> = c.try_output("err").map(|f| f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect()).unwrap_or_default();
        let err = if err == [0] && host.stderr.is_empty() { Vec::new() } else { err };
        let mut mine = host.stderr.clone();
        mine.truncate(1500);
        Verdict::from(err == mine, || format!("stderr: the original {:?}, the port {:?}", String::from_utf8_lossy(&err), String::from_utf8_lossy(&mine)))
    })
    .and(|| {
        // what the routine changed in the layer records
        let o = c.output("layers");
        let mut at = 0usize;
        let mut words: Vec<(usize, &Word)> = Vec::new();
        for x in o.words() {
            words.push((at, x));
            at += x.bytes() as usize;
        }
        let val = |layer: usize, off: usize| words.iter().find(|(p, _)| *p == layer * 0x60 + off).and_then(|(_, x)| x.value()).map(|x| x as u32);
        for (i, l) in layers.iter().enumerate() {
            let mut fields: Vec<(&str, usize, u32)> = vec![
                ("factor", 0x10, l.factor.to_bits()),
                ("factor1", 0x14, l.factor1.to_bits()),
                ("cur", 0x24, l.cur as u32),
                ("x0", 0x28, l.x0 as u32),
                ("x", 0x2c, l.x as u32),
                ("y0", 0x38, l.y0 as u32),
                ("y", 0x3c, l.y as u32),
                ("mode", 0x48, l.mode as u32),
            ];
            if mutate() {
                fields[0].2 ^= 1;
            }
            for (n, off, mine) in fields {
                match val(i, off) {
                    Some(t) if t == mine => {}
                    Some(t) => return Verdict::Different(format!("layer {i} {n}: the original {t:#x}, the port {mine:#x}")),
                    None => return Verdict::Different(format!("layer {i} {n}: not written in the original")),
                }
            }
        }
        Verdict::Same
    })
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureCompositeNew" => |c: &Case| compose(c),
        _ => return None,
    })
}
