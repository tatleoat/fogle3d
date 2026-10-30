// SPDX-License-Identifier: MIT
//! The C library's streams and `printf`/`scanf` for the port, answered from the case (finding
//! 0039): a model of the files of one case, thread-local because a `Route` is a plain function.
//!
//! - the case's `f` (`f=FILE(r)`: its bytes are the input `f`; `f=FILE(w)`: no input, whatever
//!   the port writes is the file) is the stream [`HANDLE0`];
//! - named files are the case's inputs `file:<path>`; `fopen` finds them by name ("r"), makes or
//!   truncates one ("w"); one content per name;
//! - what the port wrote is handed to the driver as `stdout` ([`emit`]): the bytes of `f`, then
//!   for each named file written, `"\n--<path>--\n"` and its content. The vector generator
//!   builds the original's `stdout` field the same way (`re/tools/si-tools/gen_write.py`).
//!
//! `fgets`, `feof`, `fclose`, `fscanf`, `sscanf` read like glibc (the same code serves the
//! emulator's stand-ins in `re/tools/emu/standins.py`: `%s`, `%f`, `%d`, spaces); `fprintf`,
//! `sprintf` write `%d %i %u %x %X %c %s %f` with flags, width, precision and `l`/`h`.

use si_checkkit::Args;
use si_mem::{Mem, Ret, Routine};
use std::cell::RefCell;

/// The emulator's handle of the first `FILE` argument of a case (the handles are 0x100 apart).
pub const HANDLE0: u32 = 0x6f00_0300;

#[derive(Default)]
struct Cur {
    data: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl Cur {
    fn getc(&mut self) -> i32 {
        if self.pos >= self.data.len() {
            self.eof = true;
            -1
        } else {
            self.pos += 1;
            self.data[self.pos - 1] as i32
        }
    }
    fn unget(&mut self) {
        self.pos -= 1;
        self.eof = false;
    }
}

#[derive(Default)]
struct Stream {
    cur: Cur,
    /// The index of the named file this stream writes or reads (`None`: the case's `f`).
    name: Option<usize>,
    write: bool,
}

#[derive(Default)]
struct Fs {
    /// Stream 0 is the case's `f`, then one per `fopen`.
    streams: Vec<Stream>,
    /// The case's named files: path, content, whether the port wrote it.
    named: Vec<(String, Vec<u8>, bool)>,
    /// The order in which named files were first opened for writing.
    written: Vec<usize>,
    /// The case has a second stream argument.
    gwritten: bool,
}

thread_local! {
    static FS: RefCell<Fs> = RefCell::new(Fs::default());
}

fn bytes_of(f: &si_x87::vectors::Field) -> Vec<u8> {
    f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect()
}

/// Sets the case's files up: call first in a port.
pub fn begin(a: &Args) {
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        *fs = Fs::default();
        let (data, write) = match a.case().try_input("f") {
            Some(f) => (bytes_of(f), false),
            None => (Vec::new(), true),
        };
        fs.streams.push(Stream { cur: Cur { data, pos: 0, eof: false }, name: None, write });
        for f in &a.case().inputs {
            if let Some(path) = f.name.strip_prefix("file:") {
                fs.named.push((path.to_string(), bytes_of(f), false));
            }
        }
    });
}

/// [`begin`], and a second stream written ([`HANDLE0`] + 0x100) when the case's `hasg` is not 0: the
/// `g=FILE(w)` argument of the writers with a material file.
pub fn begin2(a: &Args) {
    begin(a);
    if a.i("hasg") != 0 {
        FS.with(|fs| { let mut f = fs.borrow_mut(); f.streams.push(Stream { cur: Cur::default(), name: None, write: true }); f.gwritten = true; });
    }
}

/// What the port wrote, as the driver's `stdout`.
pub fn emit(o: &mut dyn si_mem::Outside) {
    let text = FS.with(|fs| {
        let fs = fs.borrow();
        let mut out = Vec::new();
        if fs.streams[0].write {
            out.extend_from_slice(&fs.streams[0].cur.data);
        }
        if fs.streams.len() > 1 && fs.streams[1].name.is_none() && fs.streams[1].write && fs.gwritten {
            out.extend_from_slice(b"\n==g==\n");
            out.extend_from_slice(&fs.streams[1].cur.data);
        }
        for &i in &fs.written {
            out.extend_from_slice(format!("\n--{}--\n", fs.named[i].0).as_bytes());
            out.extend_from_slice(&fs.named[i].1);
        }
        out
    });
    if !text.is_empty() {
        o.print(1, &text);
    }
}

fn ws(c: i32) -> bool {
    c >= 0 && matches!(c as u8, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// glibc's `vfscanf` for the conversions the program uses: `%s %d %f` and white space.
fn scan(m: &mut Mem, cur: &mut Cur, fmt: &[u8], a: &[u32]) -> i32 {
    let (mut done, mut arg) = (0i32, 0usize);
    let mut input_failed = false;
    let mut i = 0;
    'fmt: while i < fmt.len() {
        if ws(fmt[i] as i32) {
            let mut c = cur.getc();
            while ws(c) {
                c = cur.getc();
            }
            if c >= 0 {
                cur.unget();
            }
            i += 1;
            continue;
        }
        if fmt[i] != b'%' {
            let c = cur.getc();
            if c < 0 {
                input_failed = done == 0;
                break 'fmt;
            }
            if c != fmt[i] as i32 {
                cur.unget();
                break 'fmt;
            }
            i += 1;
            continue;
        }
        i += 1;
        // `%*`: converted, not stored, not counted
        let sup = fmt[i] == b'*';
        if sup {
            i += 1;
        }
        let conv = fmt[i];
        i += 1;
        let dst = if sup { 0 } else { a.get(arg).copied().unwrap_or(0) };
        if conv == b'c' {
            // one character, white space not skipped
            let c = cur.getc();
            if c < 0 {
                input_failed = done == 0;
                break 'fmt;
            }
            if !sup {
                m.set_u8(dst, c as u8);
                arg += 1;
                done += 1;
            }
            continue;
        }
        let mut c = cur.getc();
        while ws(c) {
            c = cur.getc();
        }
        if c < 0 {
            input_failed = done == 0;
            break 'fmt;
        }
        let mut text = Vec::new();
        match conv {
            b's' => {
                while c >= 0 && !ws(c) {
                    text.push(c as u8);
                    c = cur.getc();
                }
                if c >= 0 {
                    cur.unget();
                }
                if !sup {
                    for (k, b) in text.iter().enumerate() {
                        m.set_u8(dst + k as u32, *b);
                    }
                    m.set_u8(dst + text.len() as u32, 0);
                }
            }
            b'd' => {
                if c == b'+' as i32 || c == b'-' as i32 {
                    text.push(c as u8);
                    c = cur.getc();
                }
                let mut digits = 0;
                while c >= 0 && (c as u8).is_ascii_digit() {
                    text.push(c as u8);
                    digits += 1;
                    c = cur.getc();
                }
                if c >= 0 {
                    cur.unget();
                }
                if digits == 0 {
                    break 'fmt;
                }
                let v: i64 = std::str::from_utf8(&text).unwrap().parse().unwrap_or(0);
                if !sup {
                    m.set_u32(dst, v.clamp(i32::MIN as i64, i32::MAX as i64) as i32 as u32);
                }
            }
            _ => {
                // %f: sign, digits, a point, digits, an exponent (glibc's collection)
                let isd = |c: i32| c >= 0 && (c as u8).is_ascii_digit();
                if c == b'+' as i32 || c == b'-' as i32 {
                    text.push(c as u8);
                    c = cur.getc();
                }
                let mut digits = 0;
                while isd(c) {
                    text.push(c as u8);
                    digits += 1;
                    c = cur.getc();
                }
                if c == b'.' as i32 {
                    text.push(b'.');
                    c = cur.getc();
                    while isd(c) {
                        text.push(c as u8);
                        digits += 1;
                        c = cur.getc();
                    }
                }
                if digits == 0 {
                    if c >= 0 {
                        cur.unget();
                    }
                    break 'fmt;
                }
                if c == b'e' as i32 || c == b'E' as i32 {
                    c = cur.getc();
                    let mut e = Vec::new();
                    if c == b'+' as i32 || c == b'-' as i32 {
                        e.push(c as u8);
                        c = cur.getc();
                    }
                    let mut ed = false;
                    while isd(c) {
                        e.push(c as u8);
                        ed = true;
                        c = cur.getc();
                    }
                    if ed {
                        text.push(b'e');
                        text.extend_from_slice(&e);
                    }
                }
                if c >= 0 {
                    cur.unget();
                }
                let v: f32 = std::str::from_utf8(&text).unwrap().parse().unwrap_or(0.0);
                if !sup {
                    m.set_u32(dst, v.to_bits());
                }
            }
        }
        if !sup {
            arg += 1;
            done += 1;
        }
    }
    if input_failed {
        -1i32 as u32 as i32
    } else {
        done
    }
}

/// `%.{prec}e` of a finite number: `d.ddde+XX`.
fn fmt_e(v: f64, prec: usize) -> String {
    let s = format!("{v:.prec$e}");
    let (mant, exp) = s.split_once('e').unwrap();
    let e: i32 = exp.parse().unwrap();
    format!("{mant}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// `%.{prec}g` of a finite number: the shorter of `%e` and `%f` by the exponent, trailing zeros dropped.
fn fmt_g(v: f64, prec: usize) -> String {
    let p = prec.max(1);
    let e_form = format!("{v:.pr$e}", pr = p - 1);
    let x: i32 = e_form.split_once('e').unwrap().1.parse().unwrap();
    let trim = |t: String| -> String {
        if t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    };
    if x < -4 || x >= p as i32 {
        let (mant, _) = e_form.split_once('e').unwrap();
        format!("{}e{}{:02}", trim(mant.to_string()), if x < 0 { '-' } else { '+' }, x.abs())
    } else {
        trim(format!("{v:.pr$}", pr = (p as i32 - 1 - x) as usize))
    }
}

/// C `printf` for the formats of the writers.
fn format(m: &Mem, fmt: &[u8], a: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut arg = 0usize;
    let mut next = |n: usize| -> Vec<u32> {
        let v = a[arg..arg + n].to_vec();
        arg += n;
        v
    };
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        i += 1;
        let (mut left, mut zero, mut plus, mut space) = (false, false, false, false);
        while i < fmt.len() && matches!(fmt[i], b'-' | b'0' | b'+' | b' ' | b'#') {
            match fmt[i] {
                b'-' => left = true,
                b'0' => zero = true,
                b'+' => plus = true,
                b' ' => space = true,
                _ => {}
            }
            i += 1;
        }
        let mut width = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            width = width * 10 + (fmt[i] - b'0') as usize;
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            let mut p = 0;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                p = p * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        let mut long = 0;
        let mut short = false;
        while i < fmt.len() && matches!(fmt[i], b'l' | b'h') {
            if fmt[i] == b'l' { long += 1 } else { short = true }
            i += 1;
        }
        let conv = fmt[i];
        i += 1;
        let body: Vec<u8> = match conv {
            b'%' => b"%".to_vec(),
            b'd' | b'i' => {
                let w = next(1)[0];
                let v = if short { w as i16 as i64 } else { w as i32 as i64 };
                let mut s = v.abs().to_string();
                if let Some(p) = prec {
                    while s.len() < p {
                        s.insert(0, '0');
                    }
                }
                let sign = if v < 0 { "-" } else if plus { "+" } else if space { " " } else { "" };
                format!("{sign}{s}").into_bytes()
            }
            b'u' | b'x' | b'X' => {
                let w = next(1)[0];
                let v = if short { w as u16 as u32 } else { w };
                match conv {
                    b'u' => v.to_string(),
                    b'x' => format!("{v:x}"),
                    _ => format!("{v:X}"),
                }
                .into_bytes()
            }
            b'c' => vec![next(1)[0] as u8],
            b's' => {
                let p = next(1)[0];
                let mut s = if p == 0 { b"(null)".to_vec() } else { m.cstr(p) };
                if let Some(pr) = prec {
                    s.truncate(pr);
                }
                s
            }
            b'f' | b'F' => {
                let w = next(2);
                let v = f64::from_bits(w[0] as u64 | (w[1] as u64) << 32);
                let p = prec.unwrap_or(6);
                if v.is_nan() {
                    (if v.is_sign_negative() { "-nan" } else { "nan" }).as_bytes().to_vec()
                } else if v.is_infinite() {
                    (if v < 0.0 { "-inf" } else if plus { "+inf" } else { "inf" }).as_bytes().to_vec()
                } else {
                    let s = format!("{v:.p$}");
                    if plus && !s.starts_with('-') { format!("+{s}") } else if space && !s.starts_with('-') { format!(" {s}") } else { s }.into_bytes()
                }
            }
            b'g' | b'G' | b'e' | b'E' => {
                let w = next(2);
                let v = f64::from_bits(w[0] as u64 | (w[1] as u64) << 32);
                let mut s = if v.is_nan() {
                    (if v.is_sign_negative() { "-nan" } else { "nan" }).to_string()
                } else if v.is_infinite() {
                    (if v < 0.0 { "-inf" } else if plus { "+inf" } else { "inf" }).to_string()
                } else if matches!(conv, b'g' | b'G') {
                    fmt_g(v, prec.unwrap_or(6))
                } else {
                    fmt_e(v, prec.unwrap_or(6))
                };
                if conv.is_ascii_uppercase() {
                    s = s.to_uppercase();
                }
                if plus && !s.starts_with('-') { format!("+{s}") } else if space && !s.starts_with('-') { format!(" {s}") } else { s }.into_bytes()
            }
            other => panic!("printf conversion %{} (long {long})", other as char),
        };
        let pad = width.saturating_sub(body.len());
        if left {
            out.extend_from_slice(&body);
            out.extend(std::iter::repeat_n(b' ', pad));
        } else if zero && prec.is_none() && !matches!(conv, b's' | b'c') {
            let (sign, rest) = if matches!(body.first(), Some(b'-' | b'+' | b' ')) { body.split_at(1) } else { body.split_at(0) };
            out.extend_from_slice(sign);
            out.extend(std::iter::repeat_n(b'0', pad));
            out.extend_from_slice(rest);
        } else {
            out.extend(std::iter::repeat_n(b' ', pad));
            out.extend_from_slice(&body);
        }
    }
    out
}

fn with_stream<R>(h: u32, f: impl FnOnce(&mut Stream, &mut Fs) -> R) -> Option<R> {
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        let idx = (h.wrapping_sub(HANDLE0) / 0x100) as usize;
        if h < HANDLE0 || idx >= fs.streams.len() {
            return None;
        }
        let mut s = std::mem::take(&mut fs.streams[idx]);
        let r = f(&mut s, &mut fs);
        fs.streams[idx] = s;
        Some(r)
    })
}

/// Writes `bytes` at the stream's position (extending the file).
fn put(s: &mut Stream, fs: &mut Fs, bytes: &[u8]) {
    let buf: &mut Vec<u8> = match s.name {
        Some(i) => &mut fs.named[i].1,
        None => &mut s.cur.data,
    };
    if s.cur.pos > buf.len() {
        // written after a seek past the end: the gap is zeros
        buf.resize(s.cur.pos, 0);
    }
    for &b in bytes {
        if s.cur.pos < buf.len() {
            buf[s.cur.pos] = b;
        } else {
            buf.push(b);
        }
        s.cur.pos += 1;
    }
}

pub fn route(m: &mut Mem, r: Routine, a: &[u32]) -> Option<Ret> {
    let ret = match r.name {
        "fopen" => {
            let path = String::from_utf8_lossy(&m.cstr(a[0])).into_owned();
            let mode = m.cstr(a[1]);
            FS.with(|fs| {
                let mut fs = fs.borrow_mut();
                let found = fs.named.iter().position(|n| n.0 == path);
                let mut st = Stream::default();
                if mode.first() == Some(&b'r') {
                    let Some(i) = found else { return 0 };
                    st.cur.data = fs.named[i].1.clone();
                } else {
                    let i = match found {
                        Some(i) => {
                            fs.named[i].1.clear();
                            i
                        }
                        None => {
                            fs.named.push((path.clone(), Vec::new(), false));
                            fs.named.len() - 1
                        }
                    };
                    if !fs.named[i].2 {
                        fs.named[i].2 = true;
                        fs.written.push(i);
                    }
                    st.name = Some(i);
                    st.write = true;
                }
                fs.streams.push(st);
                HANDLE0 + 0x100 * (fs.streams.len() as u32 - 1)
            })
        }
        "fgets" => with_stream(a[2], |s, _| {
            let (buf, n) = (a[0], a[1] as i32);
            let mut out = Vec::new();
            while (out.len() as i32) < n - 1 {
                if s.cur.pos >= s.cur.data.len() {
                    s.cur.eof = true;
                    break;
                }
                let c = s.cur.data[s.cur.pos];
                s.cur.pos += 1;
                out.push(c);
                if c == 10 {
                    break;
                }
            }
            if out.is_empty() {
                0
            } else {
                for (k, b) in out.iter().enumerate() {
                    m.set_u8(buf + k as u32, *b);
                }
                m.set_u8(buf + out.len() as u32, 0);
                buf
            }
        })?,
        "feof" => with_stream(a[0], |s, _| s.cur.eof as u32)?,
        "_IO_getc" | "getc" | "fgetc" => with_stream(a[0], |s, _| s.cur.getc() as u32)?,
        "ungetc" => with_stream(a[1], |s, _| {
            if a[0] as i32 == -1 {
                -1i32 as u32
            } else {
                s.cur.unget();
                s.cur.eof = false;
                a[0] & 0xff
            }
        })?,
        "fclose" => 0,
        "fscanf" => {
            let fmt = m.cstr(a[1]);
            with_stream(a[0], |s, _| scan(m, &mut s.cur, &fmt, &a[2..]) as u32)?
        }
        "sscanf" => {
            let mut cur = Cur { data: m.cstr(a[0]), pos: 0, eof: false };
            let fmt = m.cstr(a[1]);
            scan(m, &mut cur, &fmt, &a[2..]) as u32
        }
        "fprintf" => {
            let text = format(m, &m.cstr(a[1]), &a[2..]);
            with_stream(a[0], |s, fs| put(s, fs, &text))?;
            text.len() as u32
        }
        "sprintf" => {
            let text = format(m, &m.cstr(a[1]), &a[2..]);
            for (k, b) in text.iter().enumerate() {
                m.set_u8(a[0] + k as u32, *b);
            }
            m.set_u8(a[0] + text.len() as u32, 0);
            text.len() as u32
        }
        "ftell" => with_stream(a[0], |s, _| s.cur.pos as u32)?,
        "fseek" => with_stream(a[0], |s, fs| {
            let len = match s.name {
                Some(i) => fs.named[i].1.len(),
                None => s.cur.data.len(),
            } as i64;
            let base = match a[2] {
                0 => 0,
                1 => s.cur.pos as i64,
                _ => len,
            };
            let to = base + a[1] as i32 as i64;
            if to < 0 {
                -1i32 as u32
            } else {
                s.cur.pos = to as usize;
                s.cur.eof = false;
                0
            }
        })?,
        _ => return None,
    };
    Some(Ret::eax(ret))
}
