// SPDX-License-Identifier: MIT
//! Checks for the dialog-level routines of `si_imgfmt::dialog` against the vectors of
//! `re/tools/imgfmt2/gen_dialog.py`: the original routine runs with every dialog, cursor and GL
//! routine it calls replaced by a recorded stand-in that answers from a script. The port runs
//! over a [`Dialogs`] / [`BackBuffer`] that replays the recorded answers and holds each call
//! it makes (name, arguments, text) against the next recorded call; the result and the
//! outputs are compared afterwards.
//!
//! `EMUCHECK_MUTATE=1` flips one bit of the port's result; `EMUCHECK_MUTATE=calls` one bit of
//! the first argument of the first call that has a compared argument.

use crate::util::mutate;
use si_imgfmt::dialog::{self, BackBuffer, Dialogs, Dlg, NewSize};
use si_imgfmt::reduce::QuantizeParameters;
use si_picture::PictureRgba;
use si_x87::vectors::{Call, Case, Check, Verdict, Word};

fn calls_mutation() -> bool {
    std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "calls")
}

/// The recorded calls of a case, consumed as the port makes its own.
struct Replay<'a> {
    calls: &'a [Call],
    idx: usize,
    bad: Option<String>,
    flipped: bool,
}

#[derive(Clone, Copy)]
enum A {
    /// A full word.
    W(u32),
    /// A short: the low 16 bits.
    H(u16),
}

fn text_of(pre: &Call) -> Vec<u8> {
    let mut v: Vec<u8> = pre.args.iter().map(|w| w.value().unwrap_or(0) as u8).collect();
    if v.last() == Some(&0) {
        v.pop();
    }
    v
}

impl<'a> Replay<'a> {
    fn new(c: &'a Case) -> Replay<'a> {
        Replay { calls: &c.calls, idx: 0, bad: None, flipped: false }
    }

    fn fail(&mut self, why: String) {
        if self.bad.is_none() {
            self.bad = Some(why);
        }
    }

    /// The next call, which must be `name` with these arguments; the "pre:" entries in front of
    /// it are returned too.
    fn take(&mut self, name: &str, args: &[A]) -> Option<(Vec<&'a Call>, &'a Call)> {
        if self.bad.is_some() {
            return None;
        }
        let mut pre = Vec::new();
        while self.calls.get(self.idx).is_some_and(|k| k.name.starts_with("pre:")) {
            pre.push(&self.calls[self.idx]);
            self.idx += 1;
        }
        let Some(k) = self.calls.get(self.idx) else {
            self.fail(format!("the port called {name} after the original's last call"));
            return None;
        };
        self.idx += 1;
        if k.name != name {
            self.fail(format!("the original called {}, the port {name}", k.name));
            return None;
        }
        for (i, a) in args.iter().enumerate() {
            let Some(theirs) = k.args.get(i).and_then(|w| w.value()) else { continue };
            let mut a = *a;
            if calls_mutation() && !self.flipped {
                self.flipped = true;
                a = match a {
                    A::W(v) => A::W(v ^ 1),
                    A::H(v) => A::H(v ^ 1),
                };
            }
            let (t, o) = match a {
                A::W(v) => (theirs as u32, v),
                A::H(v) => (theirs as u16 as u32, v as u32),
            };
            if t != o {
                self.fail(format!("{name} argument {i}: the original {t:#x}, the port {o:#x}"));
                return None;
            }
        }
        Some((pre, k))
    }

    fn take_text(&mut self, name: &str, args: &[A], text: &str) -> Option<&'a Call> {
        let (pre, k) = self.take(name, args)?;
        let mut theirs = pre.iter().find(|p| p.name == "pre:text").map(|p| text_of(p));
        if calls_mutation() && !self.flipped {
            self.flipped = true;
            theirs = theirs.map(|mut t| {
                t.push(b'!');
                t
            });
        }
        if theirs.as_deref() != Some(text.as_bytes()) {
            self.fail(format!("{name} text: the original {:?}, the port {:?}", theirs.map(|t| String::from_utf8_lossy(&t).into_owned()), text));
            return None;
        }
        Some(k)
    }

    fn result(k: Option<&Call>) -> u32 {
        k.and_then(|k| k.result.value()).unwrap_or(0) as u32
    }
}

impl Dialogs for Replay<'_> {
    fn dialog_open(&mut self, number: i32) -> Option<Dlg> {
        let r = Self::result(self.take("SI_dialogOpen", &[A::W(number as u32)]).map(|x| x.1));
        (r != 0).then_some(r)
    }
    fn set_item_title(&mut self, d: Dlg, item: i32, text: &str) {
        self.take_text("SI_setItemTitle", &[A::W(d), A::H(item as u16)], text);
    }
    fn set_texed(&mut self, d: Dlg, item: i32, text: &str) {
        self.take_text("SI_setTexed", &[A::W(d), A::H(item as u16)], text);
    }
    fn get_texed_long(&mut self, d: Dlg, item: i32) -> i32 {
        Self::result(self.take("SI_getTexedLong", &[A::W(d), A::H(item as u16)]).map(|x| x.1)) as i32
    }
    fn set_state_item(&mut self, d: Dlg, item: i32, state: i32) {
        self.take("SI_setStateItem", &[A::W(d), A::H(item as u16), A::H(state as u16)]);
    }
    fn set_state_ghost(&mut self, d: Dlg, item: i32, ghost: i32) {
        self.take("SI_setStateGhost", &[A::W(d), A::W(item as u32), A::W(ghost as u32)]);
    }
    fn get_state_item(&mut self, d: Dlg, item: i32) -> i16 {
        if self.take("SI_getStateItem", &[A::W(d), A::H(item as u16)]).is_none() {
            return 0;
        }
        match self.calls.get(self.idx) {
            Some(k) if k.name == "SI_getStateItem.out" => {
                self.idx += 1;
                k.args.first().and_then(|w| w.value()).unwrap_or(0) as u16 as i16
            }
            _ => {
                self.fail("SI_getStateItem without its stored value".into());
                0
            }
        }
    }
    fn dial_draw(&mut self, d: Dlg, flag: i32) {
        self.take("SI_dialDraw", &[A::W(d), A::H(flag as u16)]);
    }
    fn handle_event(&mut self, d: Dlg) -> i16 {
        match self.take("SI_dialogHandleEvent", &[A::W(d)]) {
            Some((pre, _)) => pre.iter().find(|p| p.name == "pre:event").and_then(|p| p.args.first()).and_then(|w| w.value()).unwrap_or(2) as u16 as i16,
            // after a mismatch or the end of the record, leave every loop
            None => 2,
        }
    }
    fn dial_close(&mut self, d: Dlg, flag: i32) {
        self.take("SI_dialClose", &[A::W(d), A::H(flag as u16)]);
    }
    fn abbreviate_path(&mut self, path: &str, width: i32) -> String {
        // the stand-in keeps the first 8 characters and a "~" when the path is longer
        self.take_text("SI_strAbbreviatePath", &[A::W(0), A::W(0), A::W(width as u32)], path);
        if path.len() > 8 {
            format!("{}~", &path[..8])
        } else {
            path.to_string()
        }
    }
    fn request(&mut self, line1: &str, line2: Option<&str>, line3: Option<&str>, button: &str) {
        let Some((pre, _)) = self.take("SI_dialogRequest", &[]) else { return };
        let mut ours = Vec::new();
        for t in [Some(line1), line2, line3, Some(button)] {
            match t {
                None => ours.push(0xFEu8),
                Some(t) => {
                    ours.extend_from_slice(t.as_bytes());
                    ours.push(0xFF);
                }
            }
        }
        let theirs: Vec<u8> = pre.iter().find(|p| p.name == "pre:request").map(|p| p.args.iter().map(|w| w.value().unwrap_or(0) as u8).collect()).unwrap_or_default();
        if theirs != ours {
            self.fail(format!("SI_dialogRequest text: the original {:?}, the port {:?}", String::from_utf8_lossy(&theirs), String::from_utf8_lossy(&ours)));
        }
    }
    fn cursor_change(&mut self, shape: i32) {
        self.take("SI_cursorChange", &[A::W(shape as u32)]);
    }
    fn resize_picture(&mut self, _pic: &PictureRgba, width: i32, height: i32, flag: i16) -> Option<PictureRgba> {
        let (_, k) = self.take("SI_resizePictureRGBA", &[A::W(0), A::W(width as u32), A::W(height as u32), A::H(flag as u16)])?;
        match &k.result {
            Word::Ref { name, .. } if name == "newpic" => Some(NEW_PIC()),
            _ => None,
        }
    }
}

#[allow(non_snake_case)]
fn NEW_PIC() -> PictureRgba {
    PictureRgba { width: 77, height: 88, ratio: 1.0, fields: 3, pad: 1, data: Vec::new() }
}

impl BackBuffer for Replay<'_> {
    fn rgb_color(&mut self, r: i32, g: i32, b: i32) {
        self.take("RGBcolor", &[A::W(r as u32 ^ mutate() as u32), A::W(g as u32), A::W(b as u32)]);
    }
    fn clear(&mut self) {
        self.take("clear", &[]);
    }
    fn swap_buffers(&mut self) {
        self.take("swapbuffers", &[]);
    }
    fn dialog_refresh_draw_over(&mut self) {
        self.take("SI_dialogRefreshDrawOver", &[]);
    }
    fn draw_mode(&mut self, mode: i32) {
        self.take("drawmode", &[A::W(mode as u32)]);
    }
}

fn flip(v: u32) -> u32 {
    if mutate() {
        v ^ 1
    } else {
        v
    }
}

/// The routine's calls all made and nothing left over.
fn finished(r: &Replay) -> Verdict {
    match &r.bad {
        Some(why) => Verdict::Different(why.clone()),
        None if r.idx != r.calls.len() => Verdict::Different(format!("{} recorded calls were not made by the port", r.calls.len() - r.idx)),
        None => Verdict::Same,
    }
}

fn word32(c: &Case, name: &str, i: usize) -> u32 {
    c.input(name).words()[i].value().unwrap_or(0) as u32
}

fn out_word(c: &Case, name: &str, i: usize) -> Option<u128> {
    c.try_output(name).and_then(|f| f.words().get(i).and_then(|w| w.value()))
}

fn same_value(what: &str, theirs: Option<u128>, ours: u32, mask: u32) -> Verdict {
    match theirs {
        Some(t) if (t as u32 & mask) == (flip(ours) & mask) => Verdict::Same,
        Some(t) => Verdict::Different(format!("{what}: the original {:#x}, the port {:#x}", t as u32 & mask, ours & mask)),
        None => Verdict::Different(format!("{what}: not written in the original")),
    }
}

fn is_ref(w: &Word) -> bool {
    matches!(w, Word::Ref { .. })
}

fn new_size(c: &Case) -> Verdict {
    let path = String::from_utf8_lossy(crate::util::cstr(&c.input("path").to_bytes())).into_owned();
    let (w, h) = (word32(c, "v_w", 0) as i32, word32(c, "v_h", 0) as i32);
    let present = is_ref(&c.input("p_ow").words()[0]) && is_ref(&c.input("p_oh").words()[0]) && is_ref(&c.input("p_of").words()[0]);
    let mut size = NewSize { width: word32(c, "ow", 0) as i32, height: word32(c, "oh", 0) as i32, flag: word32(c, "of", 0) as u16 as i16 };
    let mut r = Replay::new(c);
    let ans = dialog::get_new_size(&mut r, &path, w, h, present.then_some(&mut size));
    finished(&r)
        .and(|| same_value("result", c.try_output("eax").and_then(|f| f.words()[0].value()), ans as u32, 0xff))
        .and(|| same_value("width", out_word(c, "ow", 0), size.width as u32, !0))
        .and(|| same_value("height", out_word(c, "oh", 0), size.height as u32, !0))
        .and(|| same_value("flag", out_word(c, "of", 0), size.flag as u16 as u32, 0xffff))
}

fn pic_of(c: &Case) -> Option<PictureRgba> {
    if !is_ref(&c.input("p_pic").words()[0]) {
        return None;
    }
    let w = c.input("pic").words();
    let g = |i: usize| w[i].value().unwrap_or(0);
    Some(PictureRgba { width: g(0) as u16 as i16, height: g(1) as u16 as i16, ratio: f32::from_bits(g(2) as u32), fields: g(3) as u16 as i16, pad: g(4) as u16 as i16, data: Vec::new() })
}

fn handle_resize(c: &Case) -> Verdict {
    let path = String::from_utf8_lossy(crate::util::cstr(&c.input("path").to_bytes())).into_owned();
    let pic = pic_of(c);
    let mut r = Replay::new(c);
    let got = dialog::handle_resize_pict_rgba(&mut r, pic.clone(), &path);
    // the original answers a pointer: the old record, the new one or null
    let theirs = c.output("ptr").words()[0].clone();
    let ours = match (&got, &pic) {
        (None, _) => "null",
        (Some(g), _) if *g == NEW_PIC() => "newpic",
        (Some(g), Some(p)) if g == p => "pic",
        _ => "other",
    };
    let ours = if mutate() { "mutated" } else { ours };
    let ok = match &theirs {
        Word::Ref { name, .. } => name == ours,
        w => w.value() == Some(0) && ours == "null",
    };
    finished(&r).and(|| Verdict::from(ok, || format!("returned {theirs:?}, the port {ours}")))
}

fn set_params(c: &Case) -> Verdict {
    let mut r = Replay::new(c);
    dialog::set_palette_quantize_parameters(&mut r, 0x5d1a0000, (word32(c, "v_n", 0) as u16 as i16) ^ mutate() as i16);
    finished(&r)
}

fn get_params(c: &Case) -> Verdict {
    let ghost = word32(c, "v_ghost", 0) as u16 as i16;
    let count = word32(c, "v_count", 0) as i32;
    let dw = c.input("depths").words();
    let depths = [0, 1, 2, 3].map(|i| dw[i].value().unwrap_or(0) as i32);
    let mut count_out = word32(c, "ocount", 0) as i32;
    let ow = c.input("odepths").words();
    let mut depths_out = [0, 1, 2, 3].map(|i| ow[i].value().unwrap_or(0) as i32);
    let qw = c.input("q").words();
    let g = |i: usize| qw[i].value().unwrap_or(0) as i32;
    let mut q = QuantizeParameters { method: g(0), quality: g(1), cut: g(2), representative: g(3) };
    let mut r = Replay::new(c);
    let ans = dialog::get_palette_parameters(&mut r, ghost, count, &depths, &mut count_out, &mut depths_out, &mut q);
    finished(&r)
        .and(|| same_value("result", c.try_output("ax").and_then(|f| f.words()[0].value()), ans as u16 as u32, 0xffff))
        .and(|| same_value("count", out_word(c, "ocount", 0), count_out as u32, !0))
        .and(|| (0..4).map(|i| same_value("depth", out_word(c, "odepths", i), depths_out[i] as u32, !0)).fold(Verdict::Same, |a, b| a.and(|| b)))
        .and(|| {
            [q.method, q.quality, q.cut, q.representative]
                .iter()
                .enumerate()
                .map(|(i, v)| same_value("quality record", out_word(c, "q", i), *v as u32, !0))
                .fold(Verdict::Same, |a, b| a.and(|| b))
        })
}

fn reset(c: &Case) -> Verdict {
    let mut r = Replay::new(c);
    dialog::reset_back_buffer_and_refresh(&mut r);
    finished(&r)
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "getNewSize" => |c: &Case| new_size(c),
        "si_handleResizePictRGBA" => |c: &Case| handle_resize(c),
        "setPaletteQuantizeParameters" => |c: &Case| set_params(c),
        "SI_getPaletteParameters" => |c: &Case| get_params(c),
        "resetBackBufferAndRefresh" => |c: &Case| reset(c),
        _ => return None,
    })
}
