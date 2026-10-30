// SPDX-License-Identifier: MIT
//! The batch renderer's question-and-answer set-up (`-l` and `-S`) on the image: `newValue`
//! and `readSetup` of `sys_main.c` (finding 0019, section 13a).
//!
//! `readSetup` asks for the render parameters one line at a time (a prompt to `stream`, the
//! answer read from the standard input or, with `-l`, from the next `argv` word) and stores
//! the answers in the render set-up and in the camera record `SI_procData` points at.
//! `newValue` reads one answer line and says whether it holds a value; `r` renders at once
//! and `q` quits.
//!
//! The C library's `fgets` is a call of the outside world; `fprintf` is the port's own
//! formatting (the text goes to [`Outside::print`], 1 for the standard output, 2 for the
//! standard error; any other stream, `/dev/null`, drops it); `sscanf` is done here on the
//! image (decimal integers clamped as `strtol` does, floats read as glibc collects them,
//! words).

use super::batch::{call_render, error_text, next_arg};
use super::calls::BatchCalls;
use super::g;
use crate::render_cmd::FAULT;
use crate::Exit;
use si_mem::{Access, Mem, Outside};
use si_x87::{Round, F80};

/// The render set-up's fields `readSetup` asks for (offsets in `SI_defaultControl`, named
/// after the prompts; those of [`super::batch::ctl`] are used as they are).
pub mod f {
    pub const FIELD_RENDERING: u32 = 0x13c;
    pub const FIELD_ORDER: u32 = 0x13e;
    pub const PIXEL_RATIO: u32 = 0x154;
    /// Sampling (`short`) and the filter kind (`short`); read together as one word to tell
    /// "None" (sampling 1, kind 0).
    pub const SAMPLING: u32 = 0x158;
    pub const FILTER_KIND: u32 = 0x15a;
    pub const MAX_LEVEL: u32 = 0x15c;
    pub const THRESHOLD: u32 = 0x16c;
    pub const TRIANGLES_BY_LEAF: u32 = 0x1da;
    pub const MAX_TREE_DEPTH: u32 = 0x1dc;
    pub const RAY_DEPTH: u32 = 0x1e4;
    pub const TIME_STATS: u32 = 0x1e6;
    pub const TREE_STATS: u32 = 0x1e8;
    pub const MEMORY_STATS: u32 = 0x1ea;
    pub const ONLINE_INFO: u32 = 0x1ec;
    pub const AMBIENT: u32 = 0x1fc;
    pub const BACK_CULLING: u32 = 0x20c;
    pub const MOTION_BLUR: u32 = 0x210;
    pub const SHUTTER: u32 = 0x214;
    pub const MIN_MOVEMENT: u32 = 0x21c;
}

/// The camera record's fields (`SI_procData` + 8 points at it): the picture format index,
/// the aspect, the width and height of a custom size, the pixel aspect.
mod cam {
    pub const FORMAT: u32 = 0x54;
    pub const ASPECT: u32 = 0x58;
    pub const WIDTH: u32 = 0x5c;
    pub const HEIGHT: u32 = 0x60;
    pub const PIXEL_ASPECT: u32 = 0x64;
}

const CTL: u32 = g::SI_DEFAULT_CONTROL.0;
const STRING: u32 = g::STRING.0;
/// `SI_fmtAspect` (fifteen floats) and `SI_fmtName` (fifteen `char *`): the picture formats.
const FMT_ASPECT: u32 = 0x0867_9540;
const FMT_NAME: u32 = 0x0867_9620;
/// `SI_procData` is a `char *`-sized pointer to the per-process record; +8 is the camera.
const PROC_DATA: u32 = g::SI_PROC_DATA.0;

/// A write to `stream`: only the standard output and error are anywhere (the questions go to
/// `/dev/null` for `-l` without `-v`).
fn put(m: &Mem, o: &mut dyn Outside, stream: u32, text: &[u8]) {
    if stream == m.u32(g::STDOUT) {
        o.print(1, text);
    } else if stream == m.u32(g::STDERR) {
        o.print(2, text);
    }
}

/// glibc's text for a `double` printed with `%f`-style `{:.p$}` at `width` (nan and inf are
/// spelt the C way).
fn fixed(v: f64, width: usize, precision: usize) -> String {
    let t = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.precision$}")
    };
    format!("{t:>width$}")
}

/// The C library's `%g`: six significant digits, the shorter of the two notations, trailing
/// zeros dropped; the digits are rounded from the exact value. (Not a function of the
/// original.)
fn percent_g(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let e = format!("{v:.5e}");
    let (mantissa, exponent) = e.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let trim = |t: &str| if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t.to_string() };
    if (-4..6).contains(&exponent) {
        trim(&format!("{:.*}", (5 - exponent) as usize, v))
    } else {
        format!("{}e{}{:02}", trim(mantissa), if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

/// A `%s` argument: glibc prints `(null)` for a null pointer.
fn text_of(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

// ---------------------------------------------------------------------------------------
// sscanf on the answer line

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Where the next conversion starts: past the white space.
fn skip_space(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    i
}

/// `%d` / `%hd`: white space, a sign, decimal digits (none: no match); the value as
/// `strtol` leaves it (clamped to 32 bits).
fn scan_int(s: &[u8], at: usize) -> Option<i32> {
    let mut i = skip_space(s, at);
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let first = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    if i == first {
        return None;
    }
    let v = if neg { -v } else { v };
    Some(v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

/// `%f`: as glibc collects a decimal number (sign, digits, one point, digits, an exponent
/// letter with a sign and digits) and converts it to a `float` in one rounding. Answers
/// the value and the place after it; `None` when there are no digits.
fn scan_float(s: &[u8], at: usize) -> Option<(f32, usize)> {
    let mut i = skip_space(s, at);
    let mut t = String::new();
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        t.push(s[i] as char);
        i += 1;
    }
    let mut digits = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        t.push(s[i] as char);
        digits += 1;
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        t.push('.');
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            t.push(s[i] as char);
            digits += 1;
            i += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        let mut exp = String::from("e");
        if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
            exp.push(s[i] as char);
            i += 1;
        }
        let from = exp.len();
        while i < s.len() && s[i].is_ascii_digit() {
            exp.push(s[i] as char);
            i += 1;
        }
        if exp.len() > from {
            t.push_str(&exp);
        }
    }
    // "5." and ".5" are numbers to strtod
    let t = if t.ends_with('.') { format!("{t}0") } else { t };
    t.parse::<f32>().ok().map(|v| (v, i))
}

/// `sscanf(string, "%hd", dst)`.
fn sscanf_short(m: &mut Mem, dst: u32) {
    if let Some(v) = scan_int(&m.cstr(STRING), 0) {
        m.set_i16(dst, v as i16);
    }
}

/// `sscanf(string, "%d", dst)`.
fn sscanf_int(m: &mut Mem, dst: u32) {
    if let Some(v) = scan_int(&m.cstr(STRING), 0) {
        m.set_i32(dst, v);
    }
}

/// `sscanf(string, "%f %f …", dst, …)`: each conversion stores before the next is tried,
/// and the first that fails ends it.
fn sscanf_floats(m: &mut Mem, dst: &[u32]) {
    let s = m.cstr(STRING);
    let mut at = 0;
    for &d in dst {
        match scan_float(&s, at) {
            Some((v, next)) => {
                m.set_f32(d, v);
                at = next;
            }
            None => return,
        }
    }
}

/// `sscanf(string, "%s", dst)`: the first word, written with its terminator; nothing for an
/// empty line. A null `dst` faults.
fn sscanf_word(m: &mut Mem, dst: u32) -> Result<(), Exit> {
    let s = m.cstr(STRING);
    let i = skip_space(&s, 0);
    let end = s[i..].iter().position(|&c| is_space(c)).map_or(s.len(), |k| i + k);
    if end > i {
        if dst == 0 {
            m.record_fault(0, 1, Access::Write);
            return Err(FAULT);
        }
        m.put_cstr(dst, &s[i..end]);
    }
    Ok(())
}

/// `strstr(string, "y") != 0`.
fn has_y(m: &Mem) -> bool {
    m.cstr(STRING).contains(&b'y')
}

// ---------------------------------------------------------------------------------------
// the two routines

/// The answer line, as `readSetup` itself fetches it: with `-l` (`lineOption`) the next
/// `argv` word is copied into `string` (none left: the error), otherwise one line is read
/// from the standard input (at the end of the input `fgets` leaves `string` as it was);
/// with `-v` the line is echoed to the standard output.
fn get_answer(m: &mut Mem, o: &mut dyn Outside) -> Result<(), Exit> {
    if m.i16(g::LINE_OPTION.0) != 0 {
        m[g::ARGC] -= 1;
        if m[g::ARGC] > 0 {
            let a = next_arg(m);
            if a == 0 {
                m.record_fault(0, 1, Access::Read);
                return Err(FAULT);
            }
            si_mem::clib::strcpy(m, STRING, a);
        } else {
            return Err(error_text(m, o, b"Expected more values for -l option", b""));
        }
    } else {
        let stdin = m.u32(g::STDIN);
        o.fgets(m, STRING, 0x3ff, stdin);
    }
    Ok(())
}

/// [`get_answer`] and, with `-v`, the echo of the line as it is.
fn fetch_line(m: &mut Mem, o: &mut dyn Outside) -> Result<(), Exit> {
    get_answer(m, o)?;
    if m.i16(g::VERBOSE_OPTION.0) != 0 {
        let out = m.u32(g::STDOUT);
        let line = [m.cstr(STRING), b"\n".to_vec()].concat();
        put(m, o, out, &line);
    }
    Ok(())
}

/// [soft 0x080d04b0 newValue]: reads one answer line (as [`fetch_line`] does, but echoing
/// it without its leading blanks and the error with an empty detail) and answers whether
/// it holds a value: 0 for an empty line; `r` renders at once and ends the program with
/// status 0, `q` ends it with status -1; anything else answers 1. The line in `string`
/// keeps its blanks.
pub fn new_value(m: &mut Mem, o: &mut dyn Outside) -> Result<i16, Exit> {
    get_answer(m, o)?;
    let verbose = m.i16(g::VERBOSE_OPTION.0);
    let mut p = STRING;
    if m.u8(p) == b' ' {
        p += 1;
        while m.u8(p) == b' ' {
            p += 1;
        }
    }
    if verbose != 0 {
        let out = m.u32(g::STDOUT);
        let line = [m.cstr(p), b"\n".to_vec()].concat();
        put(m, o, out, &line);
    }
    match m.u8(p) {
        0 => Ok(0),
        b'r' => {
            call_render(m, o);
            Err(Exit(0))
        }
        b'q' => Err(Exit(-1)),
        _ => Ok(1),
    }
}

/// A yes/no question: "`name`: (yes) ? " or "(no)" by the current switch; the answer, when
/// there is one, is yes if it holds a `y`.
fn ask_switch(m: &mut Mem, o: &mut dyn Outside, stream: u32, name: &[u8], switch: u32) -> Result<(), Exit> {
    let state: &[u8] = if m.i16(switch) != 0 { b"yes" } else { b"no" };
    put(m, o, stream, &[name, b": (", state, b") ? "].concat());
    if new_value(m, o)? != 0 {
        let y = has_y(m) as i16;
        m.set_i16(switch, y);
    }
    Ok(())
}

/// A question with a short: "`prompt` (`%d`) ? " and, if there is an answer, `%hd`.
fn ask_short(m: &mut Mem, o: &mut dyn Outside, stream: u32, prompt: &[u8], at: u32) -> Result<(), Exit> {
    let line = [prompt, b" (", m.i16(at).to_string().as_bytes(), b") ? "].concat();
    put(m, o, stream, &line);
    if new_value(m, o)? != 0 {
        sscanf_short(m, at);
    }
    Ok(())
}

/// A question with a float: "`prompt` (`%g` or `%f`) ? " and, if there is an answer, `%f`.
fn ask_float(m: &mut Mem, o: &mut dyn Outside, stream: u32, prompt: &[u8], at: u32, fixed_digits: bool) -> Result<(), Exit> {
    let v = m.f32(at) as f64;
    let shown = if fixed_digits { fixed(v, 0, 6) } else { percent_g(v) };
    put(m, o, stream, &[prompt, b" (", shown.as_bytes(), b") ? "].concat());
    if new_value(m, o)? != 0 {
        sscanf_floats(m, &[at]);
    }
    Ok(())
}

/// The camera record `SI_procData` points at (+8); null faults, as the original does.
fn camera(m: &Mem) -> Result<u32, Exit> {
    let proc = m.u32(PROC_DATA);
    if proc == 0 {
        m.record_fault(0, 4, Access::Read);
        return Err(FAULT);
    }
    let c = m.u32(proc + 8);
    if c == 0 {
        m.record_fault(0, 4, Access::Read);
        return Err(FAULT);
    }
    Ok(c)
}

/// [soft 0x080cf5e0 readSetup]: the render parameter questions, in the original's order:
/// output image, first, last and step frame, picture width, picture format (the table, the
/// camera's, its aspect or size), pixel ratio, the picture's height (computed), filtering,
/// sub-region, field rendering, back culling, ray tracing depth, triangles by leaf,
/// maximum tree depth, the pre- and post-frame scripts, global ambient, motion blur,
/// statistics and online information. The prompts go to `stream`.
///
/// Where `sscanf` finds no number in the filtering answer the original reads an uninitialised
/// stack slot; the port takes 0xffff for it. The field-rendering answer uses the same slot, so
/// a failed scan there leaves the filtering's final choice.
pub fn read_setup(m: &mut Mem, o: &mut dyn Outside, stream: u32) -> Result<(), Exit> {
    for line in [
        &b"RENDERING PARAMETER SETUP...\n\n"[..],
        b"<CR> keep the default setting\n",
        b"'r' use default settings for remaining parameters and start rendering\n",
        b"'q' exit the program without rendering\n\n",
    ] {
        put(m, o, stream, line);
    }
    // the output image: read raw (not through newValue); the name is written where the
    // current one is
    let name = m.u32(CTL + super::batch::ctl::OUT_NAME);
    put(m, o, stream, &[b"Output Image: (".as_slice(), &text_of(m, name), b") ? "].concat());
    fetch_line(m, o)?;
    sscanf_word(m, name)?;
    ask_short(m, o, stream, b"Starting Frame:", CTL + super::batch::ctl::FIRST)?;
    ask_short(m, o, stream, b"Ending Frame:", CTL + super::batch::ctl::LAST)?;
    ask_short(m, o, stream, b"Step Frame:", CTL + super::batch::ctl::STEP)?;
    ask_short(m, o, stream, b"Picture Width:", CTL + super::batch::ctl::XRES)?;

    // the picture format
    put(m, o, stream, b"Picture Format:\n");
    for i in 0..=12u32 {
        let name = text_of(m, m.u32(FMT_NAME + 4 * i));
        let line = [format!("   {i:>2}) ").as_bytes(), &pad_left(&name, 17), format!(" ({})\n", fixed(m.f32(FMT_ASPECT + 4 * i) as f64, 7, 5)).as_bytes()].concat();
        put(m, o, stream, &line);
    }
    for i in [13u32, 14] {
        let name = text_of(m, m.u32(FMT_NAME + 4 * i));
        put(m, o, stream, &[format!("   {i}) ").as_bytes(), &pad_left(&name, 17), b"\n"].concat());
    }
    let cam = camera(m)?;
    let index = m.i32(cam + cam::FORMAT);
    let name = text_of(m, m.u32(FMT_NAME.wrapping_add((index as u32).wrapping_mul(4))));
    if index <= 12 {
        let aspect = m.f32(FMT_ASPECT.wrapping_add((index as u32).wrapping_mul(4))) as f64;
        put(m, o, stream, &[format!("   >> ({index} = ").as_bytes(), &name, format!(": {}) ? ", fixed(aspect, 8, 5)).as_bytes()].concat());
    } else {
        put(m, o, stream, &[format!("   >> ({index} = ").as_bytes(), &name, b") ? "].concat());
    }
    if new_value(m, o)? != 0 {
        sscanf_int(m, cam + cam::FORMAT);
    }
    let index = m.u32(cam + cam::FORMAT);
    let mut set_ratio = true;
    if index > 14 {
        let err = m.u32(g::STDERR);
        put(m, o, err, b"Expected a value from 0 to 14.  Assuming value 13.\n");
        m.set_u32(cam + cam::FORMAT, 13);
        set_ratio = false;
    } else if index > 12 {
        if index == 13 {
            put(m, o, stream, &[b"   Aspect (".as_slice(), percent_g(m.f32(cam + cam::ASPECT) as f64).as_bytes(), b") ? "].concat());
            if new_value(m, o)? != 0 {
                sscanf_floats(m, &[cam + cam::ASPECT]);
            }
            let v = m.f32(cam + cam::ASPECT);
            m.set_f32(FMT_ASPECT + 0x34, v);
        } else {
            let line = format!("   width and height ({} {}) ? ", percent_g(m.f32(cam + cam::WIDTH) as f64), percent_g(m.f32(cam + cam::HEIGHT) as f64));
            put(m, o, stream, line.as_bytes());
            if new_value(m, o)? != 0 {
                sscanf_floats(m, &[cam + cam::WIDTH, cam + cam::HEIGHT]);
                let ratio = F80::from_f32(m.f32(cam + cam::WIDTH)) / m.f32(cam + cam::HEIGHT);
                m.set_f32(FMT_ASPECT + 0x38, ratio.to_f32());
            }
        }
    }
    if set_ratio {
        let index = m.u32(cam + cam::FORMAT);
        let v = m.f32(FMT_ASPECT.wrapping_add(index.wrapping_mul(4)));
        m.set_f32(cam + cam::PIXEL_ASPECT, v);
    }

    // the pixel ratio and the height it gives
    ask_float(m, o, stream, b"Pixel Ratio:", CTL + f::PIXEL_RATIO, false)?;
    let xres = m.i16(CTL + super::batch::ctl::XRES);
    let h = F80::from_i32(xres as i32) / m.f32(cam + cam::PIXEL_ASPECT) * m.f32(CTL + f::PIXEL_RATIO) + 0.5f64;
    let h = h.to_i32(Round::Zero) as i16;
    m.set_i16(CTL + super::batch::ctl::YRES, h);
    put(m, o, stream, format!("Picture Height = {h}\n").as_bytes());

    // filtering
    for line in [&b"Filtering:\n"[..], b"   0)  None\n", b"   1)  Bartlett Window\n", b"   2)  Adaptive\n"] {
        put(m, o, stream, line);
    }
    let mut kept: u16 = 0;
    if m.u32(CTL + f::SAMPLING) == 1 {
        put(m, o, stream, b"   >> (0 = None) ? ");
    } else if m.i16(CTL + f::FILTER_KIND) == 0 {
        kept = 1;
        put(m, o, stream, b"   >> (1 = Bartlett Window) ? ");
    } else {
        kept = 2;
        put(m, o, stream, b"   >> (2 = Adaptive) ? ");
    }
    let mut choice: u16 = 0xffff; // the uninitialised local when the answer has no number
    if new_value(m, o)? != 0 {
        if let Some(v) = scan_int(&m.cstr(STRING), 0) {
            choice = v as u16;
        }
    }
    if choice > 2 {
        choice = kept;
    }
    match choice {
        0 => {
            m.set_i16(CTL + f::FILTER_KIND, 0);
            m.set_i16(CTL + f::SAMPLING, 1);
        }
        1 => {
            m.set_i16(CTL + f::FILTER_KIND, 0);
            ask_short(m, o, stream, b"   Sampling:", CTL + f::SAMPLING)?;
        }
        2 => {
            m.set_i16(CTL + f::FILTER_KIND, 1);
            ask_short(m, o, stream, b"   Max Level:", CTL + f::MAX_LEVEL)?;
            ask_float(m, o, stream, b"   Threshold:", CTL + f::THRESHOLD, false)?;
        }
        _ => {
            let err = m.u32(g::STDERR);
            put(m, o, err, b"   Expected a value from 0-2.  Assuming default filtering value...\n");
        }
    }

    // the sub-region: four numbers asked with %f; right and top are kept as one minus
    ask_switch(m, o, stream, b"Sub-Region Rendering", CTL + super::batch::ctl::SUBREGION)?;
    if m.i16(CTL + super::batch::ctl::SUBREGION) != 0 {
        use super::batch::ctl;
        ask_float(m, o, stream, b"   Left  :", CTL + ctl::SUB_LEFT, true)?;
        ask_float(m, o, stream, b"   Right :", CTL + ctl::SUB_RIGHT_INV, true)?;
        ask_float(m, o, stream, b"   Bottom:", CTL + ctl::SUB_BOTTOM, true)?;
        ask_float(m, o, stream, b"   Top   :", CTL + ctl::SUB_TOP_INV, true)?;
        let top = (F80::ONE - m.f32(CTL + ctl::SUB_TOP_INV)).to_f32();
        let right = (F80::ONE - m.f32(CTL + ctl::SUB_RIGHT_INV)).to_f32();
        m.set_f32(CTL + ctl::SUB_TOP_INV, top);
        m.set_f32(CTL + ctl::SUB_RIGHT_INV, right);
    }

    // field rendering
    ask_switch(m, o, stream, b"Field Rendering", CTL + f::FIELD_RENDERING)?;
    if m.i16(CTL + f::FIELD_RENDERING) != 0 {
        put(m, o, stream, b"   Starting Field Rendering on:\n");
        put(m, o, stream, b"   0)  Even\n");
        put(m, o, stream, b"   1)  Odd\n");
        if m.i16(CTL + f::FIELD_ORDER) == 0 {
            put(m, o, stream, b"   >> (0 = Even) ? ");
        } else {
            m.set_i16(CTL + f::FIELD_ORDER, 1);
            put(m, o, stream, b"   >> (1 = Odd) ? ");
        }
        if new_value(m, o)? != 0 {
            // the same stack slot as the filtering answer: a failed scan leaves what that held
            let mut answer: u16 = choice;
            if let Some(v) = scan_int(&m.cstr(STRING), 0) {
                answer = v as u16;
            }
            match answer {
                0 => m.set_i16(CTL + f::FIELD_ORDER, 0),
                1 => m.set_i16(CTL + f::FIELD_ORDER, 1),
                _ => {
                    let err = m.u32(g::STDERR);
                    put(m, o, err, b"Expected either 0 or 1.  Assuming default value...\n");
                }
            }
        }
    }

    ask_switch(m, o, stream, b"Back Culling", CTL + f::BACK_CULLING)?;
    ask_short(m, o, stream, b"Ray Tracing Depth:", CTL + f::RAY_DEPTH)?;
    ask_short(m, o, stream, b"Triangles by Leaf:", CTL + f::TRIANGLES_BY_LEAF)?;
    ask_short(m, o, stream, b"Max Tree Depth:", CTL + f::MAX_TREE_DEPTH)?;

    // the scripts: read raw, the word written where the current name is
    for (prompt, at) in [(&b"Pre-frame Script: ("[..], CTL + super::batch::ctl::PRE_SCRIPT), (b"Post-frame Script: (", CTL + super::batch::ctl::POST_SCRIPT)] {
        let p = m.u32(at);
        put(m, o, stream, &[prompt, &text_of(m, p), b") ? "].concat());
        fetch_line(m, o)?;
        sscanf_word(m, p)?;
    }

    // global ambient
    let a = CTL + f::AMBIENT;
    let line = format!("Global Ambient: ({} {} {}) ? ", percent_g(m.f32(a) as f64), percent_g(m.f32(a + 4) as f64), percent_g(m.f32(a + 8) as f64));
    put(m, o, stream, line.as_bytes());
    if new_value(m, o)? != 0 {
        sscanf_floats(m, &[a, a + 4, a + 8]);
    }

    // motion blur
    ask_switch(m, o, stream, b"Motion Blur", CTL + f::MOTION_BLUR)?;
    if m.i16(CTL + f::MOTION_BLUR) != 0 {
        ask_float(m, o, stream, b"   Shutter Speed (in frames):", CTL + f::SHUTTER, false)?;
        ask_float(m, o, stream, b"   Min. Movement (in pixels):", CTL + f::MIN_MOVEMENT, false)?;
    }
    for (name, at) in [(&b"Time Statistics"[..], f::TIME_STATS), (b"Memory Statistics", f::MEMORY_STATS), (b"Tree Statistics", f::TREE_STATS), (b"On Line Information", f::ONLINE_INFO)] {
        ask_switch(m, o, stream, name, CTL + at)?;
    }
    Ok(())
}

/// `%-17s`: padded on the right to 17 columns (longer texts unchanged).
fn pad_left(text: &[u8], width: usize) -> Vec<u8> {
    let mut t = text.to_vec();
    while t.len() < width {
        t.push(b' ');
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_clamp_and_floats_follow_the_c_library() {
        assert_eq!(scan_int(b"  -12abc", 0), Some(-12));
        assert_eq!(scan_int(b"+", 0), None);
        assert_eq!(scan_int(b"99999999999", 0), Some(i32::MAX));
        assert_eq!(scan_float(b" .5", 0).map(|x| x.0), Some(0.5));
        assert_eq!(scan_float(b"5.", 0).map(|x| x.0), Some(5.0));
        assert_eq!(scan_float(b"1e", 0).map(|x| x.0), Some(1.0));
        assert_eq!(scan_float(b"-1.5E+2 x", 0), Some((-150.0, 7)));
        assert_eq!(scan_float(b".", 0), None);
    }

    #[test]
    fn numbers_print_the_c_way() {
        assert_eq!(percent_g(1.37f32 as f64), "1.37");
        assert_eq!(percent_g(100000.0), "100000");
        assert_eq!(percent_g(1e6), "1e+06");
        assert_eq!(fixed(1.37f32 as f64, 7, 5), "1.37000");
        assert_eq!(fixed(f64::NAN, 8, 5), "     nan");
        assert_eq!(fixed(0.5, 0, 6), "0.500000");
    }
}

/// [soft 0x080caa1c verifyChecksum]: answers 0 (the original's whole body is `xor eax,eax`).
pub fn verify_checksum() -> i32 {
    0
}
