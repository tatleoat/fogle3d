// SPDX-License-Identifier: MIT
//! The two small picture dialogs: the size asked for a resize (`resize.c`) and the settings of
//! the palette maker (`getPalette.c`).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`). Everything
//! they do with the dialog library (open a dialog by its number, put text in an item, set an
//! item's state, run the event loop) is a call into code that belongs to the dialog unit and
//! is not part of this crate: it is the [`Dialogs`] trait, with the same names as the
//! original's routines and the arguments they take. A caller supplies the dialog library; the
//! tests in this file supply a scripted one.
//!
//! Where the original writes a number into the program-wide scratch string `_SI_temp_`
//! (0x08780f00) with `sprintf` and hands that string to `SI_setTexed`, the text is passed
//! directly; the scratch string itself is not modelled.
//!
//! Checked against the original in the emulator (`re/tools/imgfmt2/gen_dialog.py`, replayed by
//! `imgfmt-emucheck`'s `dialog.rs`): every dialog, cursor and GL routine the original calls is
//! a recorded stand-in answering from a script; the port makes the same calls, with the same
//! arguments and text, in the same order, and returns the same results (finding 0014).

use crate::reduce::QuantizeParameters;
use si_picture::PictureRgba;

/// A dialog, as `SI_dialogOpen` hands it out (never 0).
pub type Dlg = u32;

/// The routines of the dialog library (and the few others) that these dialogs call, with the
/// original's argument order, each with its address.
pub trait Dialogs {
    /// `SI_dialogOpen` (0x083859b8): the dialog of that resource number, or `None`.
    fn dialog_open(&mut self, number: i32) -> Option<Dlg>;
    /// `SI_setItemTitle` (0x0838b664).
    fn set_item_title(&mut self, d: Dlg, item: i32, text: &str);
    /// `SI_setTexed` (0x0838bd10): the text of an editable item.
    fn set_texed(&mut self, d: Dlg, item: i32, text: &str);
    /// `SI_getTexedLong` (0x0838c204): the number in an editable item.
    fn get_texed_long(&mut self, d: Dlg, item: i32) -> i32;
    /// `SI_setStateItem` (0x0838b824).
    fn set_state_item(&mut self, d: Dlg, item: i32, state: i32);
    /// `SI_setStateGhost` (0x0838b868): greys an item out when `ghost` is not 0.
    fn set_state_ghost(&mut self, d: Dlg, item: i32, ghost: i32);
    /// `SI_getStateItem` (0x0838bcec): the state of an item (stored through a pointer).
    fn get_state_item(&mut self, d: Dlg, item: i32) -> i16;
    /// `SI_dialDraw` (0x08384224).
    fn dial_draw(&mut self, d: Dlg, flag: i32);
    /// `SI_dialogHandleEvent` (0x08385e8c): waits for the next event and returns its number
    /// (the 16-bit word at +6 of the record the original returns).
    fn handle_event(&mut self, d: Dlg) -> i16;
    /// `SI_dialClose` (0x08385b88).
    fn dial_close(&mut self, d: Dlg, flag: i32);
    /// `SI_strAbbreviatePath` (0x083cf4d0): the path shortened to the given width.
    fn abbreviate_path(&mut self, path: &str, width: i32) -> String;
    /// `SI_dialogRequest` (0x08141c00): a message box with up to two further lines and a
    /// button.
    fn request(&mut self, line1: &str, line2: Option<&str>, line3: Option<&str>, button: &str);
    /// `SI_cursorChange` (0x083d1910).
    fn cursor_change(&mut self, shape: i32);
    /// `SI_resizePictureRGBA` (0x082f34f4): the picture made `width` by `height`, or `None`
    /// when there is not memory for it. `flag` is the dialog's second choice.
    fn resize_picture(&mut self, pic: &PictureRgba, width: i32, height: i32, flag: i16) -> Option<PictureRgba>;
}

/// The graphics and dialog calls that clear the composite window.
pub trait BackBuffer {
    /// `RGBcolor` (0x086111a4).
    fn rgb_color(&mut self, r: i32, g: i32, b: i32);
    /// `clear` (0x08611984).
    fn clear(&mut self);
    /// `swapbuffers` (0x08611804).
    fn swap_buffers(&mut self);
    /// `SI_dialogRefreshDrawOver` (0x0838e834).
    fn dialog_refresh_draw_over(&mut self);
    /// `drawmode` (0x08611608).
    fn draw_mode(&mut self, mode: i32);
}

/// Clears both buffers of the composite window to black and hands the drawing back to the
/// dialogs [soft 0x085ca118 resetBackBufferAndRefresh]: black, clear, swap, black,
/// clear, refresh the dialogs' overlay, draw mode 0x10.
pub fn reset_back_buffer_and_refresh(g: &mut dyn BackBuffer) {
    g.rgb_color(0, 0, 0);
    g.clear();
    g.swap_buffers();
    g.rgb_color(0, 0, 0);
    g.clear();
    g.dialog_refresh_draw_over();
    g.draw_mode(0x10);
}

/// What `getNewSize` hands back through its three pointers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NewSize {
    pub width: i32,
    pub height: i32,
    pub flag: i16,
}

/// Asks for a new size for a picture [soft 0x085be730 getNewSize].
///
/// The dialog (number 0x2777) shows the file name (shortened to 30 characters, item 4) and the
/// current width (items 6 and 10) and height (items 8 and 13); item 0x12 is a switch. Its
/// state is read into `out.flag` and toggled on every click (event 0x12). OK (event 1) reads
/// the two numbers and the switch and answers true unless both numbers are the old ones; Cancel
/// (event 2) sets width and height to 0 and answers false. No dialog, or a missing output,
/// answers false (with the outputs unchanged). `out` is the three pointers: `None` for a null.
pub fn get_new_size(d: &mut dyn Dialogs, path: &str, width: i32, height: i32, out: Option<&mut NewSize>) -> bool {
    let Some(out) = out else { return false };
    let Some(dlg) = d.dialog_open(0x2777) else { return false };
    let title = d.abbreviate_path(path, 0x1e);
    d.set_item_title(dlg, 4, &title);
    let text = width.to_string();
    d.set_item_title(dlg, 6, &text);
    d.set_texed(dlg, 10, &text);
    let text = height.to_string();
    d.set_item_title(dlg, 8, &text);
    d.set_texed(dlg, 0xd, &text);
    d.dial_draw(dlg, 1);
    let answer;
    loop {
        match d.handle_event(dlg) {
            1 => {
                out.width = d.get_texed_long(dlg, 10);
                out.height = d.get_texed_long(dlg, 0xd);
                out.flag = d.get_state_item(dlg, 0x12);
                answer = if out.width == width { out.height != height } else { true };
                break;
            }
            2 => {
                out.height = 0;
                out.width = 0;
                answer = false;
                break;
            }
            0x12 => {
                out.flag = d.get_state_item(dlg, 0x12);
                let was = out.flag;
                out.flag = (was == 0) as i16;
                d.set_state_item(dlg, 0x12, out.flag as i32);
            }
            _ => {}
        }
    }
    d.dial_close(dlg, 1);
    answer
}

/// Asks for a new size and resizes the picture [soft 0x085be8d8 si_handleResizePictRGBA]:
/// no picture, no question. On OK with a size that differs the picture is
/// remade with the wait cursor (shape 5) on, and the new picture answered; if there is not
/// memory the old one is kept and a message says so.
pub fn handle_resize_pict_rgba(d: &mut dyn Dialogs, pic: Option<PictureRgba>, path: &str) -> Option<PictureRgba> {
    let pic = pic?;
    let mut size = NewSize::default();
    if !get_new_size(d, path, pic.width as i32, pic.height as i32, Some(&mut size)) {
        return Some(pic);
    }
    d.cursor_change(5);
    let made = d.resize_picture(&pic, size.width, size.height, size.flag);
    d.cursor_change(0);
    match made {
        Some(p) => Some(p),
        None => {
            d.request("Error: not enough memory to make clip", None, None, "Ok");
            Some(pic)
        }
    }
}

/// Puts the dialog's items in the state of a method choice
/// [soft 0x085bcec0 setPaletteQuantizeParameters]: item 9 (neural network) takes
/// the choice (`neural` is 0 for the boxes); items 10, 11 and 13 are greyed and item 14 (boxes)
/// switched on when it is 0; items 15 to 21 (the box settings) are greyed when it is not.
pub fn set_palette_quantize_parameters(d: &mut dyn Dialogs, dlg: Dlg, neural: i16) {
    let n = neural as i32;
    let boxes = (neural == 0) as i32;
    d.set_state_item(dlg, 9, n);
    d.set_state_ghost(dlg, 10, boxes);
    d.set_state_ghost(dlg, 0xb, boxes);
    d.set_state_ghost(dlg, 0xd, boxes);
    d.set_state_item(dlg, 0xe, boxes);
    for item in 0xf..=0x15 {
        d.set_state_ghost(dlg, item, n);
    }
}

/// The dialog for the settings of the palette maker [soft 0x085bcfa8 SI_getPaletteParameters]
/// (dialog 0x250c): the number of colours (item 4), the bits of each channel (items
/// 0x16, 0x19, 0x1c, 0x1f), the method (item 9 neural network, 0xe boxes), the quality of the
/// network (item 0xb), the box rule (0x10 median, 0x11 variance) and the representative (0x13
/// middle, 0x14 mean of colours, 0x15 mean of pixels).
///
/// - `ghost` not 0 greys the colour count, the depths and their companions: the fixed items.
/// - OK reads the count into `count_out` (even when it is refused): below 2 is refused unless
///   `ghost` is not 0; the depths must each be 1 to 8 and the quality 1 to 10. A refusal shows a
///   message and the dialog goes on. Success fills `depths_out` and `q` and answers 1.
/// - Cancel answers 0. No dialog: a message and 0.
pub fn get_palette_parameters(
    d: &mut dyn Dialogs,
    ghost: i16,
    count: i32,
    depths: &[i32; 4],
    count_out: &mut i32,
    depths_out: &mut [i32; 4],
    q: &mut QuantizeParameters,
) -> i16 {
    let Some(dlg) = d.dialog_open(0x250c) else {
        d.request("Error: unable to open dialog", None, None, "Ok");
        return 0;
    };
    d.set_texed(dlg, 4, &count.to_string());
    for (item, depth) in [0x16, 0x19, 0x1c, 0x1f].into_iter().zip(depths) {
        d.set_texed(dlg, item, &depth.to_string());
    }
    let g = ghost as i32;
    for item in [3, 4, 5, 6, 7, 0x16, 0x19, 0x1c, 0x1f, 0x17, 0x1a, 0x1d, 0x20, 0x18, 0x1b, 0x1e, 0x21] {
        d.set_state_ghost(dlg, item, g);
    }
    let neural = d.get_state_item(dlg, 9);
    set_palette_quantize_parameters(d, dlg, neural);
    d.dial_draw(dlg, 1);
    let mut method = if d.get_state_item(dlg, 9) != 0 { 1 } else { 2 };
    let mut cut = if d.get_state_item(dlg, 0x10) != 0 { 1 } else { 2 };
    let mut representative = if d.get_state_item(dlg, 0x13) != 0 {
        1
    } else if d.get_state_item(dlg, 0x14) != 0 {
        2
    } else {
        3
    };
    loop {
        match d.handle_event(dlg) {
            1 => {
                *count_out = d.get_texed_long(dlg, 4);
                if ghost == 0 && *count_out < 2 {
                    d.set_texed(dlg, 4, "2");
                    d.request("The minimum number of color is 2.", Some("Please enter a value respecting this constraint."), None, "Ok");
                    continue;
                }
                for (slot, item) in depths_out.iter_mut().zip([0x16, 0x19, 0x1c, 0x1f]) {
                    *slot = d.get_texed_long(dlg, item);
                }
                if !depths_out.iter().all(|v| (1..=8).contains(v)) {
                    d.request(
                        "The bits per channel must be between 1 and 8, inclusively.",
                        Some("Please enter a value respecting this constraint."),
                        None,
                        "Ok",
                    );
                    continue;
                }
                q.quality = d.get_texed_long(dlg, 0xb);
                d.get_texed_long(dlg, 0xb);
                if !(1..=10).contains(&q.quality) {
                    d.request(
                        "The neural network quality must be between 1 and 10, inclusively.",
                        Some("Please enter a value respecting this constraint."),
                        None,
                        "Ok",
                    );
                    continue;
                }
                q.method = method;
                q.cut = cut;
                q.representative = representative;
                d.dial_close(dlg, 1);
                return 1;
            }
            2 => {
                d.dial_close(dlg, 1);
                return 0;
            }
            9 => {
                method = 1;
                set_palette_quantize_parameters(d, dlg, 1);
            }
            0xe => {
                method = 2;
                set_palette_quantize_parameters(d, dlg, 0);
            }
            0x10 => {
                cut = 1;
                d.set_state_item(dlg, 0x10, 1);
                d.set_state_item(dlg, 0x11, 0);
            }
            0x11 => {
                cut = 2;
                d.set_state_item(dlg, 0x10, 0);
                d.set_state_item(dlg, 0x11, 1);
            }
            e @ (0x13 | 0x14 | 0x15) => {
                let (a, b, c) = match e {
                    0x13 => (1, 0, 0),
                    0x14 => (0, 1, 0),
                    _ => (0, 0, 1),
                };
                representative = match e {
                    0x13 => 1,
                    0x14 => 2,
                    _ => 3,
                };
                d.set_state_item(dlg, 0x13, a);
                d.set_state_item(dlg, 0x14, b);
                d.set_state_item(dlg, 0x15, c);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dialog library that records every call and answers from lists.
    #[derive(Default)]
    struct Script {
        log: Vec<String>,
        events: Vec<i16>,
        texed: Vec<i32>,
        state: Vec<i16>,
        open: bool,
        resize: Option<PictureRgba>,
    }

    impl Dialogs for Script {
        fn dialog_open(&mut self, n: i32) -> Option<Dlg> {
            self.log.push(format!("open {n:#x}"));
            self.open.then_some(7)
        }
        fn set_item_title(&mut self, _: Dlg, i: i32, t: &str) {
            self.log.push(format!("title {i} {t}"));
        }
        fn set_texed(&mut self, _: Dlg, i: i32, t: &str) {
            self.log.push(format!("texed {i} {t}"));
        }
        fn get_texed_long(&mut self, _: Dlg, i: i32) -> i32 {
            self.log.push(format!("long {i}"));
            self.texed.remove(0)
        }
        fn set_state_item(&mut self, _: Dlg, i: i32, s: i32) {
            self.log.push(format!("state {i} {s}"));
        }
        fn set_state_ghost(&mut self, _: Dlg, i: i32, s: i32) {
            self.log.push(format!("ghost {i} {s}"));
        }
        fn get_state_item(&mut self, _: Dlg, i: i32) -> i16 {
            self.log.push(format!("get {i}"));
            self.state.remove(0)
        }
        fn dial_draw(&mut self, _: Dlg, _: i32) {
            self.log.push("draw".into());
        }
        fn handle_event(&mut self, _: Dlg) -> i16 {
            self.events.remove(0)
        }
        fn dial_close(&mut self, _: Dlg, _: i32) {
            self.log.push("close".into());
        }
        fn abbreviate_path(&mut self, p: &str, w: i32) -> String {
            format!("{p}/{w}")
        }
        fn request(&mut self, a: &str, _: Option<&str>, _: Option<&str>, _: &str) {
            self.log.push(format!("request {a}"));
        }
        fn cursor_change(&mut self, s: i32) {
            self.log.push(format!("cursor {s}"));
        }
        fn resize_picture(&mut self, _: &PictureRgba, w: i32, h: i32, f: i16) -> Option<PictureRgba> {
            self.log.push(format!("resize {w} {h} {f}"));
            self.resize.take()
        }
    }

    #[derive(Default)]
    struct Gl(Vec<&'static str>);
    impl BackBuffer for Gl {
        fn rgb_color(&mut self, _: i32, _: i32, _: i32) {
            self.0.push("color");
        }
        fn clear(&mut self) {
            self.0.push("clear");
        }
        fn swap_buffers(&mut self) {
            self.0.push("swap");
        }
        fn dialog_refresh_draw_over(&mut self) {
            self.0.push("over");
        }
        fn draw_mode(&mut self, _: i32) {
            self.0.push("mode");
        }
    }

    #[test]
    fn back_buffer_is_cleared_twice_with_a_swap_between() {
        let mut g = Gl::default();
        reset_back_buffer_and_refresh(&mut g);
        assert_eq!(g.0, ["color", "clear", "swap", "color", "clear", "over", "mode"]);
    }

    fn pic() -> PictureRgba {
        PictureRgba { width: 4, height: 3, ratio: 1.0, fields: 3, pad: 0, data: vec![0; 48] }
    }

    #[test]
    fn new_size_toggles_the_switch_and_answers_a_change() {
        let mut s = Script { open: true, events: vec![0x12, 5, 1], texed: vec![8, 6], state: vec![0, 1], ..Default::default() };
        let mut out = NewSize::default();
        assert!(get_new_size(&mut s, "a.pic", 4, 3, Some(&mut out)));
        assert_eq!(out, NewSize { width: 8, height: 6, flag: 1 });
        assert_eq!(s.log[0], "open 0x2777");
        assert!(s.log.contains(&"state 18 1".to_string()));
        assert_eq!(s.log.last().unwrap(), "close");
    }

    #[test]
    fn new_size_unchanged_and_cancelled_answer_false() {
        let mut s = Script { open: true, events: vec![1], texed: vec![4, 3], state: vec![0], ..Default::default() };
        let mut out = NewSize::default();
        assert!(!get_new_size(&mut s, "a", 4, 3, Some(&mut out)));
        let mut s = Script { open: true, events: vec![2], ..Default::default() };
        let mut out = NewSize { width: 9, height: 9, flag: 1 };
        assert!(!get_new_size(&mut s, "a", 4, 3, Some(&mut out)));
        assert_eq!((out.width, out.height), (0, 0));
        assert!(!get_new_size(&mut Script::default(), "a", 4, 3, None));
        assert!(!get_new_size(&mut Script::default(), "a", 4, 3, Some(&mut out)));
    }

    #[test]
    fn resize_keeps_the_old_picture_without_memory() {
        let mut s = Script { open: true, events: vec![1], texed: vec![8, 6], state: vec![0], ..Default::default() };
        let r = handle_resize_pict_rgba(&mut s, Some(pic()), "a");
        assert_eq!(r, Some(pic()));
        assert!(s.log.iter().any(|l| l.starts_with("request Error: not enough memory")));
        assert_eq!(handle_resize_pict_rgba(&mut Script::default(), None, "a"), None);
    }

    #[test]
    fn palette_dialog_refuses_then_accepts() {
        let mut s = Script {
            open: true,
            events: vec![1, 0x13, 1],
            texed: vec![1, 16, 5, 4, 3, 2, 7, 7],
            state: vec![0, 0, 0, 0, 0],
            ..Default::default()
        };
        // first OK: a count of 1 is refused; then the representative, then OK
        let (mut n, mut dp, mut q) = (0, [0; 4], QuantizeParameters { method: 0, quality: 0, cut: 0, representative: 0 });
        let r = get_palette_parameters(&mut s, 0, 8, &[8; 4], &mut n, &mut dp, &mut q);
        assert_eq!(r, 1);
        assert_eq!((n, dp), (16, [5, 4, 3, 2]));
        assert_eq!(q, QuantizeParameters { method: 2, quality: 7, cut: 2, representative: 1 });
        assert!(s.log.iter().any(|l| l == "request The minimum number of color is 2."));
    }

    #[test]
    fn palette_dialog_cancel_answers_zero() {
        let mut s = Script { open: true, events: vec![2], state: vec![1, 1, 0, 0, 1], ..Default::default() };
        let (mut n, mut dp, mut q) = (0, [0; 4], QuantizeParameters { method: 0, quality: 0, cut: 0, representative: 0 });
        assert_eq!(get_palette_parameters(&mut s, 1, 8, &[8; 4], &mut n, &mut dp, &mut q), 0);
        assert_eq!(get_palette_parameters(&mut Script::default(), 0, 8, &[8; 4], &mut n, &mut dp, &mut q), 0);
    }
}
