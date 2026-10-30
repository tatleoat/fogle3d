// SPDX-License-Identifier: MIT
//! The compositing run of the dialog "Composite" (`pic_composite.c`, finding 0014):
//! [`picture_composite_new`] SI_pictureCompositeNew (0x085c6b70) builds a picture of a
//! given size, lays the layers of a list over it frame after frame, and writes (or shows)
//! each result.
//!
//! Everything the routine does that is not picture arithmetic (windows, buttons, files, the
//! status line, standard error) is a call of the [`ComposeHost`] trait, in the order and with
//! the arguments the original makes it; the picture arithmetic is [`crate::composite`].
//!
//! Checked in the emulator (finding 0014, img1 section; `imgfmt-emucheck` module `compose`,
//! vectors from `re/tools/img1/gen_compose.py`) with the depth flag off: the original runs with
//! recorded stand-ins for the window and drawing calls, the status line, the header read, the
//! picture reader, writer and display, and the port's host replays that tape call for call
//! (names, arguments, texts, order), then the result, standard error and the layer records the
//! routine changes are compared. The depth files (`depth` set: three streams and the depth
//! variant of the compositing) are read from the listing only, so the function is cited as
//! `SI_pictureCompositeNew (0x085c6b70)` and not with the checked form.
//!
//! Differences that are deliberate:
//! - The three depth files (`bfp`, `lfp`, `newbzfp`) are read or created through the host,
//!   and the new depth file's bytes reach it when the layer is finished (`fclose`), not as
//!   they are written. A run that is aborted or fails leaves the new depth file as created.
//! - With depth off, the original still calls `remove` and `rename` on two names it never
//!   set (stack contents); here nothing is removed or renamed.
//! - The layer name buffers are not limited in length.

use crate::composite::{layer_blur, layer_get_contour, picture_composite, set_alpha, Depth};
use crate::effect::Fault;
use crate::pict::{field_to_full, full_to_field, Fields};
use si_file::Stream;
use si_picture::Picture;
use si_x87::{Round, F80};

/// One layer of the list (the original's 0x60-byte `SI_LayerInfo`; only the fields this
/// routine uses). The routine changes `factor_now`, `cur`, `x`, `y`, `mode`, and the starts
/// when it must.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerInfo {
    /// +0x00 the file name (without extension; `.pic` is added when opening).
    pub name: String,
    /// +0x0c the factor at the first frame.
    pub factor0: f32,
    /// +0x10 the factor at the current frame (modes 2 and 6 compute it).
    pub factor: f32,
    /// +0x14 the factor at the last frame; a negative one means "same as the first".
    pub factor1: f32,
    /// +0x20 the first frame of a layer that is itself a numbered sequence (0: single file).
    pub start: i32,
    /// +0x24 the frame of that sequence last used (the routine adds the step per layer pass).
    pub cur: i32,
    /// +0x28, +0x2c, +0x30 the x position at the first frame, now, and at the last frame.
    pub x0: i32,
    pub x: i32,
    pub x1: i32,
    /// +0x38, +0x3c, +0x40 the same for y.
    pub y0: i32,
    pub y: i32,
    pub y1: i32,
    /// +0x48 the mode of [`picture_composite`] (6: with a blurred edge).
    pub mode: i32,
    /// +0x54 the blur radius of mode 6.
    pub blur: i32,
    /// +0x5a nonzero: the layer's own sequence is counted from `start` (the depth file name
    /// and the file name follow it).
    pub sequence: i16,
    /// +0x5e zero: the file holds a field (`name.1`), made a full frame by doubling its rows.
    pub full: i16,
}

/// What the dialog hands over (the arguments of the original, in order; the three
/// shorts-in-a-word of the original are separate fields).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compose {
    /// 3: the file name the results are written to (a frame number is added).
    pub name: String,
    /// 4: the pixel ratio of the result.
    pub ratio: f32,
    /// 5, 6, 7: first frame, last frame, step.
    pub first: i32,
    pub last: i32,
    pub step: i32,
    /// 8: how many times a frame is repeated (written under `name.1`, `name.2`, …).
    pub repeat: i32,
    /// 9, 10: the size of the result.
    pub width: i16,
    pub height: i16,
    /// 12, 13, 14: the colour the result starts from.
    pub fill: [i32; 3],
    /// 15 low: nonzero numbers the files by frame and gives each layer its own motion.
    pub numbered: i16,
    /// 15 high: nonzero keeps the alpha plane in the written result.
    pub keep_alpha: i16,
    /// 16 low: nonzero shows progress (status line, or standard error in batch mode).
    pub verbose: i16,
    /// 16 high: nonzero shows the result in a window as it is made.
    pub display: i16,
    /// 17 low: nonzero is the batch run (no frame window, progress on standard error).
    pub batch: i16,
    /// 17 high: nonzero composites with the depth files.
    pub depth: i16,
    /// 18: 3 a full frame; 1 or 2 the two fields (`name.2`/`name.1` for 1, the other way for 2).
    pub field_mode: i32,
}

/// Windows, buttons, files and messages: what the original gets from the graphics library,
/// the tablet driver, the standard library and the picture library.
pub trait ComposeHost {
    /// `SI_statusDisplayMessage` (0x083cca94).
    fn status(&mut self, text: &str, level: i32);
    /// `fprintf (stderr, …)`.
    fn stderr(&mut self, text: &str);
    /// `SI_xmaxscreen`, `SI_ymaxscreen` (0x08781794, 0x08781796).
    fn screen_size(&mut self) -> (i32, i32);
    /// `SI_dialogDark` (0x0878141a) and `SI_windowTitle` (0x087817e6), the frame's colours.
    fn ui_colors(&mut self) -> (u16, u16);
    /// `SI_tabletActive` (0x086793f8).
    fn tablet_active(&mut self) -> bool;
    /// `SI_tabletButton` (0x086794b4).
    fn tablet_button(&mut self) -> i32;
    /// `SI_tabletGetButtonUpEvent` (0x083cec2c).
    fn tablet_get_button_up_event(&mut self);
    /// `noborder` (0x08619828).
    fn no_border(&mut self);
    /// `prefposition` (0x086196fc), arguments x1, x2, y1, y2.
    fn pref_position(&mut self, x1: i32, x2: i32, y1: i32, y2: i32);
    /// `winopen` (0x086193e0); −1 when it fails.
    fn win_open(&mut self, title: &str) -> i32;
    /// `winclose` (0x0861946c).
    fn win_close(&mut self, id: i32);
    /// `wintitle` (0x0861999c).
    fn win_title(&mut self, title: &str);
    /// `color` (0x08610e1c).
    fn color(&mut self, c: i32);
    /// `clear` (0x08611984).
    fn clear(&mut self);
    /// `getdrawmode` (0x086116b0).
    fn get_draw_mode(&mut self) -> i32;
    /// `drawmode` (0x08611608).
    fn draw_mode(&mut self, mode: i32);
    /// `linewidth` (0x086183b0).
    fn line_width(&mut self, w: i32);
    /// `bgnclosedline` (0x08612b7c).
    fn bgn_closed_line(&mut self);
    /// `v2i` (0x08612c18).
    fn v2i(&mut self, v: [i32; 2]);
    /// `endclosedline` (0x08612b90).
    fn end_closed_line(&mut self);
    /// `RGBmode` (0x0860fc48).
    fn rgb_mode(&mut self);
    /// `gconfig` (0x0860fd6c).
    fn gconfig(&mut self);
    /// `RGBcolor` (0x086111a4).
    fn rgb_color(&mut self, r: i32, g: i32, b: i32);
    /// `getbutton` (0x0860e684): whether the button is down.
    fn getbutton(&mut self, button: i32) -> bool;
    /// `resetBackBufferAndRefresh` (0x085ca118, [`crate::dialog::reset_back_buffer_and_refresh`]).
    fn reset_back_buffer_and_refresh(&mut self);
    /// `SI_pictureDisplay` (0x085ca7a0) with all four position arguments 0.
    fn picture_display(&mut self, pic: &Picture);
    /// `SI_softOpen` (0x085e3164) with mode `rb`, extension `.pic` and messages on, then the
    /// whole file read: `None` when it does not open.
    fn soft_open_read(&mut self, name: &str) -> Option<Vec<u8>>;
    /// `SI_pictureRead` (0x085d2dfc): opens the file by name itself, shows its own messages,
    /// `None` for the answer 0.
    fn read_picture(&mut self, name: &str) -> Option<Picture>;
    /// `SI_pictureWrite` (0x085d34a0).
    fn write_picture(&mut self, name: &str, pic: &Picture, comment: &str);
    /// `fopen (name, "rb")`: the file's bytes, `None` when it does not open.
    fn fopen_read(&mut self, name: &str) -> Option<Vec<u8>>;
    /// `fopen (name, "wb+")`: the file made empty; `false` when it does not open.
    fn fopen_create(&mut self, name: &str) -> bool;
    /// What `fclose` of a file opened with `wb+` leaves in it.
    fn write_file(&mut self, name: &str, bytes: &[u8]);
    /// `remove`.
    fn remove(&mut self, name: &str);
    /// `rename`.
    fn rename(&mut self, from: &str, to: &str);
    /// `getpid`.
    fn getpid(&mut self) -> i32;
}

/// `__xpg_basename`: what follows the last slash once trailing slashes are gone; `.` for an
/// empty name and `/` for slashes alone.
pub fn basename(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let t = path.trim_end_matches('/');
    if t.is_empty() {
        return "/".into();
    }
    t.rsplit('/').next().unwrap_or(t).to_string()
}

/// What `sscanf (text, "%s %d %d", …)` stores: the two numbers, each only if reached.
pub fn scan_s_d_d(text: &[u8]) -> (Option<i32>, Option<i32>) {
    let mut i = 0;
    let ws = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let skip = |i: &mut usize| {
        while *i < text.len() && ws(text[*i]) {
            *i += 1;
        }
    };
    skip(&mut i);
    let t0 = i;
    while i < text.len() && !ws(text[i]) {
        i += 1;
    }
    if i == t0 {
        return (None, None);
    }
    let int = |i: &mut usize| -> Option<i32> {
        skip(i);
        let s = *i;
        let mut j = s;
        if j < text.len() && (text[j] == b'+' || text[j] == b'-') {
            j += 1;
        }
        let d = j;
        while j < text.len() && text[j].is_ascii_digit() {
            j += 1;
        }
        if j == d {
            return None;
        }
        *i = j;
        let v: i64 = std::str::from_utf8(&text[s..j]).ok()?.parse().unwrap_or(0);
        Some(v as i32)
    };
    let a = int(&mut i);
    let b = if a.is_some() { int(&mut i) } else { None };
    (a, b)
}

/// `%1.4f` of the C library for a value that went through a float: `nan` or `-nan` by the sign
/// bit, `inf` or `-inf`, otherwise the usual fixed notation.
fn c_fixed4(v: f32) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{:1.4}", v as f64)
}

/// `fstp m32`: the register value stored as a float.
fn f32_round(v: F80) -> f32 {
    v.to_f32()
}

/// The comment of a 0x58-byte picture file header: 80 bytes from offset 8, up to the first 0.
fn header_comment(data: &[u8]) -> Vec<u8> {
    let c = &data[8..0x58];
    c.iter().take_while(|&&b| b != 0).copied().collect()
}

struct Windows {
    /// the frame window (−1: none)
    frame: i32,
    /// the picture window
    shown: i32,
}

fn plane(n: usize, fill: i32) -> Option<Vec<u8>> {
    Some(vec![fill as u8; n])
}

/// Runs the composite SI_pictureCompositeNew (0x085c6b70) (partial, see the module
/// note) and answers whether it completed (1) or stopped (0).
pub fn picture_composite_new(job: &Compose, layers: &mut [LayerInfo], host: &mut dyn ComposeHost) -> Result<bool, Fault> {
    let nl = layers.len() as i32;
    if nl == 0 {
        host.status("No layer specified.", 3);
        return Ok(false);
    }
    if job.repeat < 1 {
        host.status(&format!("{} is an invalid repeat value.\n", job.repeat), 3);
        return Ok(false);
    }
    if job.numbered != 0 {
        for l in layers.iter_mut() {
            l.cur = l.start;
        }
    }
    let (width, height) = (job.width as i32, job.height as i32);
    let n = width.wrapping_mul(height);
    let frames = F80::from_i32(job.last.wrapping_sub(job.first).wrapping_add(1)) / F80::from_i32(job.step);
    let per = f32_round(frames.round_int(Round::Up));
    let count = F80::from_f32(per).to_i32(Round::Zero).wrapping_mul(job.repeat);

    let mut win = Windows { frame: -1, shown: -1 };
    let mut out = Picture { width: job.width, height: job.height, ratio: job.ratio, fields: 3, pad: 0, planes: [None, None, None, None] };
    // the four planes of the result: the original makes them first, only when n is not 0
    let size = if n > 0 { n as usize } else { 0 };
    if n != 0 {
        if n < 0 {
            return Err(Fault::BadSize);
        }
        out.planes = [plane(size, 0), plane(size, 0), plane(size, 0), plane(size, 0)];
    }

    if job.display != 0 {
        let (xmax, ymax) = host.screen_size();
        let mut ox = (xmax - (width - 1)) / 2;
        let xhi;
        if ox < 0 {
            ox = 0;
            xhi = xmax;
        } else {
            xhi = width - 1 + ox;
        }
        let mut oy = (ymax - (height - 1)) / 2;
        let yhi;
        if oy < 0 {
            oy = 0;
            yhi = ymax;
        } else {
            yhi = height - 1 + oy;
        }
        if job.batch == 0 {
            host.no_border();
            host.pref_position(ox - 7, xhi + 7, oy - 7, yhi + 7);
            win.frame = host.win_open("");
            let (dark, title) = host.ui_colors();
            host.color(dark as i32);
            host.clear();
            let mode = host.get_draw_mode();
            host.draw_mode(0x20);
            host.clear();
            host.draw_mode(mode);
            host.color(title as i32);
            host.line_width(5);
            host.bgn_closed_line();
            let mut v = [3, 3];
            host.v2i(v);
            v[1] = (yhi - oy) - (v[1] - 14);
            host.v2i(v);
            v[0] = (xhi - ox) - (v[0] - 14);
            host.v2i(v);
            v[1] = 3;
            host.v2i(v);
            host.end_closed_line();
            if win.frame == -1 {
                host.status("Could not open a window to display the composited image!", 2);
            }
            host.no_border();
        }
        host.pref_position(ox, xhi, oy, yhi);
        win.shown = host.win_open("");
        if win.shown == -1 {
            host.status("Could not open a window to display the composited image!", 2);
        } else {
            host.rgb_mode();
            host.gconfig();
            host.rgb_color(0, 0, 0);
            host.clear();
        }
    }

    let mut frame = job.first;
    let mut outer = 0i32;
    // the name buffer `pcVar21`, the layer file name `local_33c`
    let mut name;
    while frame <= job.last {
        for rep in 0..job.repeat {
            name = if job.numbered != 0 {
                format!("{}.{}", job.name, job.repeat.wrapping_mul(outer).wrapping_add(rep).wrapping_mul(job.step).wrapping_add(job.first))
            } else if job.repeat > 1 {
                format!("{}.{}", job.name, rep + 1)
            } else {
                job.name.clone()
            };
            if job.batch != 0 && job.display != 0 && win.shown != -1 {
                host.win_title(&basename(&name));
            }
            if job.verbose != 0 {
                if job.batch != 0 {
                    host.stderr(&format!("composite {name} "));
                } else {
                    host.status(&format!("Compositing {name}"), 0);
                }
            }
            if n == 0 {
                return Err(Fault::NullPlane);
            }
            for (i, f) in [job.fill[0], job.fill[1], job.fill[2], 0].into_iter().enumerate() {
                out.planes[i] = plane(size, f);
            }
            let counter = outer.wrapping_mul(job.repeat).wrapping_add(rep);
            for li in 0..layers.len() {
                if layers[li].factor1 < 0.0 {
                    layers[li].factor1 = layers[li].factor0;
                }
                let steps = if job.first != job.last || job.repeat >= 2 {
                    let total = if job.first != job.last { count } else { job.repeat };
                    let div = F80::from_i32(total.wrapping_sub(1));
                    let l = &layers[li];
                    let diff = f32_round(F80::from_f32(l.factor1) - l.factor0);
                    let fs = f32_round(F80::from_f32(diff) / div);
                    let xs = f32_round(F80::from_i32(l.x1.wrapping_sub(l.x0)) / div);
                    let ys = f32_round(F80::from_i32(l.y1.wrapping_sub(l.y0)) / div);
                    (fs, xs, ys)
                } else {
                    if job.batch != 0 {
                        let l = &mut layers[li];
                        l.x0 = l.x1;
                        l.y0 = l.y1;
                    }
                    (0.0, 0.0, 0.0)
                };
                let mut depth_files: Option<(Vec<u8>, Vec<u8>, String)> = None;
                let mut new_depth: Vec<u8> = Vec::new();
                let mut bfp_name = String::new();
                if job.depth != 0 {
                    let l = &layers[li];
                    let z = if l.sequence != 0 && l.start > 0 { format!("{}.{}.Zpic", l.name, l.cur.max(l.start)) } else { format!("{}.Zpic", l.name) };
                    bfp_name = format!("{name}.Zpic");
                    let fail = |host: &mut dyn ComposeHost, win: &Windows| {
                        if win.shown != 0 {
                            host.win_close(win.shown);
                        }
                        if win.frame != 0 {
                            host.win_close(win.frame);
                        }
                    };
                    let back = if li == 0 {
                        if host.fopen_create(&bfp_name) {
                            Some(Vec::new())
                        } else {
                            None
                        }
                    } else {
                        host.fopen_read(&bfp_name)
                    };
                    let Some(back) = back else {
                        host.status(&format!("Error opening bfp file {bfp_name}.\n"), 3);
                        fail(host, &win);
                        return Ok(false);
                    };
                    let Some(layer_z) = host.fopen_read(&z) else {
                        host.status(&format!("File {} has no z channel", layers[li].name), 2);
                        fail(host, &win);
                        return Ok(false);
                    };
                    let newbz = format!("newbz{}.Zpic", host.getpid());
                    if !host.fopen_create(&newbz) {
                        host.status(&format!("Error opening newzfp file {newbz}\n"), 3);
                        fail(host, &win);
                        return Ok(false);
                    }
                    depth_files = Some((back, layer_z, newbz));
                }
                // this pass's factor and position
                {
                    let l = &mut layers[li];
                    if l.mode == 2 || l.mode == 6 {
                        l.factor = f32_round(F80::from_i32(counter) * steps.0 + l.factor0);
                    }
                    l.x = (F80::from_i32(l.x0) + F80::from_i32(counter) * steps.1).to_i32(Round::Zero);
                    l.y = (F80::from_i32(l.y0) + F80::from_i32(counter) * steps.2).to_i32(Round::Zero);
                    if l.start != 0 {
                        l.cur = l.cur.wrapping_add(job.step);
                    }
                }
                // the layer file
                let mut lname = {
                    let l = &layers[li];
                    if l.sequence == 1 && l.start > 0 {
                        format!("{}.{}", l.name, l.start.wrapping_add(frame).wrapping_sub(job.first))
                    } else {
                        l.name.clone()
                    }
                };
                if layers[li].full == 0 {
                    lname.push_str(".1");
                }
                let Some(file) = host.soft_open_read(&lname) else {
                    if job.display == 0 {
                        return Ok(false);
                    }
                    if win.shown != -1 {
                        host.win_close(win.shown);
                    }
                    if job.batch != 0 || win.frame == -1 {
                        return Ok(false);
                    }
                    host.win_close(win.frame);
                    host.reset_back_buffer_and_refresh();
                    return Ok(false);
                };
                if file.len() < 0x58 {
                    host.status(&format!("Cannot read input file {lname}\n"), 3);
                    return Ok(false);
                }
                let comment = header_comment(&file);
                let layer_pic = if layers[li].full == 0 {
                    let Some(fieldpic) = host.read_picture(&lname) else { continue };
                    match field_to_full(&fieldpic, None, 1)? {
                        Fields::Done(p) => Some(p),
                        Fields::Refused(m) => {
                            host.status(m, 3);
                            host.status("Conversion to full frame error\n", 3);
                            None
                        }
                    }
                } else {
                    let Some(p) = host.read_picture(&lname) else { continue };
                    Some(p)
                };
                let Some(mut layer) = layer_pic else { continue };
                if comment.first() == Some(&b'S') {
                    let (a, b) = scan_s_d_d(&comment);
                    let l = &mut layers[li];
                    if let Some(a) = a {
                        l.x = a;
                    }
                    if let Some(b) = b {
                        l.y = b;
                    }
                    l.y = l.y.wrapping_add(1);
                }
                if layer.planes[3].is_none() {
                    let l = &mut layers[li];
                    if l.mode == 1 || l.mode == 6 {
                        l.mode = 0;
                        host.status(&format!("File {} has no alpha channel, make operation in overlapping", l.name), 2);
                    }
                    let m = (layer.width as i32).wrapping_mul(layer.height as i32);
                    layer.planes[3] = Some(vec![0xff; m.max(0) as usize]);
                }
                // progress on standard error
                let l_snapshot = layers[li].clone();
                let sub = |l: &LayerInfo| -> String {
                    if l.start != 0 {
                        format!("{}.{} ", l.name, l.start.wrapping_add(frame).wrapping_sub(job.first))
                    } else {
                        format!("{} ", l.name)
                    }
                };
                if job.batch != 0 && job.verbose != 0 {
                    if job.numbered != 0 || job.repeat >= 2 {
                        host.stderr(&sub(&l_snapshot));
                    } else {
                        host.stderr(&format!("{} ", l_snapshot.name));
                    }
                    let flag = match l_snapshot.mode {
                        0 => Some("-o ".to_string()),
                        1 => Some("-c ".to_string()),
                        2 => Some(format!("-f {} ", c_fixed4(l_snapshot.factor))),
                        3 => Some("-P ".to_string()),
                        4 => Some("-N ".to_string()),
                        5 => Some("-B ".to_string()),
                        6 => Some(format!("-h {} {} ", l_snapshot.blur, c_fixed4(l_snapshot.factor))),
                        7 => Some("-C ".to_string()),
                        8 => Some("-D ".to_string()),
                        _ => None,
                    };
                    if let Some(f) = flag {
                        host.stderr(&f);
                    }
                }
                // compose
                let l = layers[li].clone();
                {
                    let mut depth = depth_files.as_ref().map(|(b, z, _)| Depth { back: Stream::new(b), layer: Stream::new(z), out: Vec::new() });
                    if l.mode != 6 {
                        picture_composite(&mut out, &layer, l.x as i16, l.y as i16, l.mode as i16, l.factor, depth.as_mut())?;
                    } else {
                        let mut tmp = Picture { width: out.width, height: out.height, ratio: out.ratio, fields: out.fields, pad: out.pad, planes: [plane(size, 0), plane(size, 0), plane(size, 0), plane(size, 0)] };
                        set_alpha(&mut tmp, &layer, l.x, l.y)?;
                        picture_composite(&mut out, &layer, l.x as i16, l.y as i16, 1, l.factor, depth.as_mut())?;
                        let mask = tmp.planes[3].clone().unwrap_or_default();
                        layer_get_contour(&out, &mut tmp, &mask, 0, 4)?;
                        layer_blur(&mut tmp, l.blur)?;
                        picture_composite(&mut out, &tmp, 0, 0, 6, l.factor, depth.as_mut())?;
                    }
                    if let Some(d) = depth {
                        new_depth = d.out;
                    }
                }
                if job.display != 0 && win.shown != -1 {
                    if job.verbose != 0 {
                        host.status(&format!("Displaying {name}; layer {li}"), 0);
                    }
                    host.picture_display(&out);
                }
                drop(layer);
                if job.display != 0 && [0x67, 0x66, 0x65].into_iter().all(|b| host.getbutton(b) || host.tablet_button() == b) {
                    host.status("Compositing aborted!", 0);
                    if win.shown != -1 {
                        host.win_close(win.shown);
                    }
                    if job.batch == 0 && win.frame != -1 {
                        host.win_close(win.frame);
                        host.reset_back_buffer_and_refresh();
                    }
                    return Ok(false);
                }
                if let Some((_, _, newbz)) = depth_files {
                    // bfp, lfp and newbzfp closed; the new depth file replaces the old
                    host.write_file(&newbz, &new_depth);
                    host.remove(&bfp_name);
                    host.rename(&newbz, &bfp_name);
                }
            }
            let alpha = out.planes[3].take();
            if job.keep_alpha != 0 {
                out.planes[3] = alpha.clone();
            }
            if job.field_mode == 3 {
                host.write_picture(&name, &out, "Composited picture");
            } else {
                match full_to_field(&out)? {
                    Fields::Refused(m) => host.status(m, 3),
                    Fields::Done((even, odd)) => {
                        let first = format!("{name}{}", if job.field_mode == 2 { ".1" } else { ".2" });
                        let mut second = first.clone();
                        second.pop();
                        second.push(if job.field_mode == 2 { '2' } else { '1' });
                        host.write_picture(&first, &even, "Composited picture");
                        host.write_picture(&second, &odd, "Composited picture");
                    }
                }
            }
            out.planes[3] = alpha;
            if job.batch != 0 && job.verbose != 0 {
                host.stderr("\n");
            }
        }
        frame = frame.wrapping_add(job.step);
        outer += 1;
    }
    if job.display != 0 {
        host.status("Done! Please press middle mouse to continue.", 0);
        while !(host.getbutton(0x66) || host.tablet_button() == 0x66) {
            if host.tablet_active() {
                host.tablet_get_button_up_event();
            }
        }
        if job.batch == 0 && win.frame != -1 {
            host.win_close(win.frame);
        }
        if win.shown != -1 {
            host.win_close(win.shown);
        }
        if job.batch == 0 {
            host.reset_back_buffer_and_refresh();
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Rec {
        log: Vec<String>,
        files: HashMap<String, Picture>,
        written: Vec<(String, Picture)>,
    }

    impl ComposeHost for Rec {
        fn status(&mut self, t: &str, l: i32) {
            self.log.push(format!("status {l} {t}"));
        }
        fn stderr(&mut self, t: &str) {
            self.log.push(format!("stderr {t}"));
        }
        fn screen_size(&mut self) -> (i32, i32) {
            (1280, 1024)
        }
        fn ui_colors(&mut self) -> (u16, u16) {
            (1, 2)
        }
        fn tablet_active(&mut self) -> bool {
            false
        }
        fn tablet_button(&mut self) -> i32 {
            0
        }
        fn tablet_get_button_up_event(&mut self) {}
        fn no_border(&mut self) {}
        fn pref_position(&mut self, a: i32, b: i32, c: i32, d: i32) {
            self.log.push(format!("pref {a} {b} {c} {d}"));
        }
        fn win_open(&mut self, _: &str) -> i32 {
            5
        }
        fn win_close(&mut self, id: i32) {
            self.log.push(format!("close {id}"));
        }
        fn win_title(&mut self, t: &str) {
            self.log.push(format!("title {t}"));
        }
        fn color(&mut self, _: i32) {}
        fn clear(&mut self) {}
        fn get_draw_mode(&mut self) -> i32 {
            0
        }
        fn draw_mode(&mut self, _: i32) {}
        fn line_width(&mut self, _: i32) {}
        fn bgn_closed_line(&mut self) {}
        fn v2i(&mut self, _: [i32; 2]) {}
        fn end_closed_line(&mut self) {}
        fn rgb_mode(&mut self) {}
        fn gconfig(&mut self) {}
        fn rgb_color(&mut self, _: i32, _: i32, _: i32) {}
        fn getbutton(&mut self, b: i32) -> bool {
            b == 0x66
        }
        fn reset_back_buffer_and_refresh(&mut self) {
            self.log.push("reset".into());
        }
        fn picture_display(&mut self, _: &Picture) {
            self.log.push("display".into());
        }
        fn soft_open_read(&mut self, name: &str) -> Option<Vec<u8>> {
            self.files.contains_key(name).then(|| vec![0; 0x58])
        }
        fn read_picture(&mut self, name: &str) -> Option<Picture> {
            self.files.get(name).cloned()
        }
        fn write_picture(&mut self, name: &str, p: &Picture, _: &str) {
            self.written.push((name.into(), p.clone()));
        }
        fn fopen_read(&mut self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn fopen_create(&mut self, _: &str) -> bool {
            true
        }
        fn write_file(&mut self, _: &str, _: &[u8]) {}
        fn remove(&mut self, _: &str) {}
        fn rename(&mut self, _: &str, _: &str) {}
        fn getpid(&mut self) -> i32 {
            7
        }
    }

    fn pic(w: i16, h: i16, v: u8) -> Picture {
        let n = (w as usize) * (h as usize);
        Picture { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![v; n]), Some(vec![v; n]), Some(vec![v; n]), Some(vec![255; n])] }
    }

    fn job() -> Compose {
        Compose { name: "out".into(), ratio: 1.0, first: 1, last: 1, step: 1, repeat: 1, width: 4, height: 2, field_mode: 3, keep_alpha: 1, ..Compose::default() }
    }

    #[test]
    fn refusals() {
        let mut h = Rec::default();
        assert!(!picture_composite_new(&job(), &mut [], &mut h).unwrap());
        assert_eq!(h.log, ["status 3 No layer specified."]);
        let mut h = Rec::default();
        let mut j = job();
        j.repeat = 0;
        assert!(!picture_composite_new(&j, &mut [LayerInfo::default()], &mut h).unwrap());
        assert_eq!(h.log, ["status 3 0 is an invalid repeat value.\n"]);
    }

    #[test]
    fn one_copy_layer_is_written_under_the_name() {
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(4, 2, 9));
        let mut l = LayerInfo { name: "a".into(), full: 1, mode: 0, factor0: 1.0, factor1: 1.0, ..LayerInfo::default() };
        assert!(picture_composite_new(&job(), std::slice::from_mut(&mut l), &mut h).unwrap());
        assert_eq!(h.written.len(), 1);
        assert_eq!(h.written[0].0, "out");
        assert_eq!(h.written[0].1.planes[0].as_deref(), Some(&[9u8; 8][..]));
    }

    #[test]
    fn repeats_are_numbered_and_frames_stepped() {
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(4, 2, 3));
        let mut j = job();
        j.last = 5;
        j.step = 2;
        j.repeat = 2;
        let mut l = LayerInfo { name: "a".into(), full: 1, ..LayerInfo::default() };
        assert!(picture_composite_new(&j, std::slice::from_mut(&mut l), &mut h).unwrap());
        let names: Vec<_> = h.written.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, ["out.1", "out.2", "out.1", "out.2", "out.1", "out.2"]);
        j.numbered = 1;
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(4, 2, 3));
        assert!(picture_composite_new(&j, std::slice::from_mut(&mut l), &mut h).unwrap());
        let names: Vec<_> = h.written.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, ["out.1", "out.3", "out.5", "out.7", "out.9", "out.11"]);
    }

    #[test]
    fn layer_motion_interpolates() {
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(1, 1, 1));
        let mut j = job();
        j.last = 5;
        let mut l = LayerInfo { name: "a".into(), full: 1, x0: 0, x1: 8, y0: 0, y1: 4, ..LayerInfo::default() };
        assert!(picture_composite_new(&j, std::slice::from_mut(&mut l), &mut h).unwrap());
        // five frames: the last pass sits at the end position
        assert_eq!((l.x, l.y), (8, 4));
    }

    #[test]
    fn missing_alpha_turns_overlap_into_copy() {
        let mut h = Rec::default();
        let mut p = pic(4, 2, 5);
        p.planes[3] = None;
        h.files.insert("a".into(), p);
        let mut l = LayerInfo { name: "a".into(), full: 1, mode: 1, ..LayerInfo::default() };
        picture_composite_new(&job(), std::slice::from_mut(&mut l), &mut h).unwrap();
        assert_eq!(l.mode, 0);
        assert!(h.log[0].starts_with("status 2 File a has no alpha channel"));
    }

    #[test]
    fn missing_layer_stops() {
        let mut h = Rec::default();
        let mut l = LayerInfo { name: "none".into(), full: 1, ..LayerInfo::default() };
        assert!(!picture_composite_new(&job(), std::slice::from_mut(&mut l), &mut h).unwrap());
        assert!(h.written.is_empty());
    }

    #[test]
    fn fields_go_to_two_files() {
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(4, 2, 5));
        let mut j = job();
        j.field_mode = 1;
        let mut l = LayerInfo { name: "a".into(), full: 1, ..LayerInfo::default() };
        picture_composite_new(&j, std::slice::from_mut(&mut l), &mut h).unwrap();
        let names: Vec<_> = h.written.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, ["out.2", "out.1"]);
    }

    #[test]
    fn display_run_opens_and_closes_windows() {
        let mut h = Rec::default();
        h.files.insert("a".into(), pic(4, 2, 5));
        let mut j = job();
        j.display = 1;
        let mut l = LayerInfo { name: "a".into(), full: 1, ..LayerInfo::default() };
        assert!(picture_composite_new(&j, std::slice::from_mut(&mut l), &mut h).unwrap());
        assert!(h.log.contains(&"display".to_string()));
        assert_eq!(h.log.iter().filter(|s| s.starts_with("close")).count(), 2);
        assert_eq!(h.log.last().map(String::as_str), Some("reset"));
    }

    #[test]
    fn helpers() {
        assert_eq!(basename("/a/b/c.pic"), "c.pic");
        assert_eq!(basename("c//"), "c");
        assert_eq!(basename(""), ".");
        assert_eq!(scan_s_d_d(b"Sname 12 -3"), (Some(12), Some(-3)));
        assert_eq!(scan_s_d_d(b"Sname 12"), (Some(12), None));
        assert_eq!(scan_s_d_d(b"  "), (None, None));
    }
}
