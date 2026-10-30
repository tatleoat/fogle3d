// SPDX-License-Identifier: MIT
//! The C library's file calls for the checks of finding 0041, answered from the case: a model
//! of the files of one case (thread-local, because a `Route` is a plain function). Modelled on
//! the one of `tools-checkkit` (finding 0039), reading only.
//!
//! - the case's `f` (`f=FILE(r)`: its bytes) is the stream [`HANDLE0`];
//! - the case's named files (`file:<path>`) are found by `fopen` ("r"), `access`;
//! - `getcwd` answers "/" in a new block (as the emulator's stand-in), `getenv` answers null,
//!   `ferror` 0, `feof` the end-of-file flag set by a read that found no byte.

use si_checkkit::Args;
use si_mem::{Mem, Ret, Routine};
use std::cell::RefCell;

/// The emulator's handle of the first `FILE` argument of a case (the handles are 0x100 apart).
pub const HANDLE0: u32 = 0x6f00_0300;

#[derive(Default, Clone)]
struct Stream {
    data: Vec<u8>,
    pos: usize,
    eof: bool,
}

#[derive(Default)]
struct Fs {
    streams: Vec<Stream>,
    named: Vec<(String, Vec<u8>)>,
}

thread_local! {
    /// Set by `begin`, cleared per case by `reset`: the case's `plt:` calls are answered here only then.
    static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FS: RefCell<Fs> = RefCell::new(Fs::default());
}

macro_rules! bytes_of {
    ($f:expr) => {
        $f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect::<Vec<u8>>()
    };
}

/// The handle of the case's `FILE *` argument.
pub fn handle() -> u32 {
    HANDLE0
}

/// Sets the case's files up: call first in a port.
pub fn begin(a: &Args) {
    ACTIVE.with(|c| c.set(true));
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        *fs = Fs::default();
        // the stream of a `FILE *` argument is the first handle; without one the first `fopen` is
        if let Some(f) = a.case().try_input("f") {
            fs.streams.push(Stream { data: bytes_of!(f), pos: 0, eof: false });
        }
        for f in &a.case().inputs {
            if let Some(path) = f.name.strip_prefix("file:") {
                fs.named.push((path.to_string(), bytes_of!(f)));
            }
        }
    });
}

fn with_stream<R>(h: u32, f: impl FnOnce(&mut Stream) -> R) -> Option<R> {
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        let idx = (h.wrapping_sub(HANDLE0) / 0x100) as usize;
        if h < HANDLE0 || idx >= fs.streams.len() {
            return None;
        }
        Some(f(&mut fs.streams[idx]))
    })
}

pub fn route(m: &mut Mem, r: Routine, a: &[u32]) -> Option<Ret> {
    // a `plt:` call is answered here only in a case that set files up (`begin`): the vectors of other groups
    // record the answers of their own `plt:` calls (getenv, fopen of SI_udfWrite, ...)
    if r.name.starts_with("plt:") && !ACTIVE.with(|c| c.get()) {
        return None;
    }
    let ret = match r.name.strip_prefix("plt:").unwrap_or(r.name) {
        "fopen" => {
            let path = String::from_utf8_lossy(&m.cstr(a[0])).into_owned();
            let mode = m.cstr(a[1]);
            FS.with(|fs| {
                let mut fs = fs.borrow_mut();
                let found = fs.named.iter().position(|n| n.0 == path);
                if mode.first() != Some(&b'r') {
                    return 0;
                }
                let Some(i) = found else { return 0 };
                let data = fs.named[i].1.clone();
                fs.streams.push(Stream { data, pos: 0, eof: false });
                HANDLE0 + 0x100 * (fs.streams.len() as u32 - 1)
            })
        }
        "fgets" => with_stream(a[2], |s| {
            let (buf, n) = (a[0], a[1] as i32);
            let mut out = Vec::new();
            while (out.len() as i32) < n - 1 {
                if s.pos >= s.data.len() {
                    s.eof = true;
                    break;
                }
                let c = s.data[s.pos];
                s.pos += 1;
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
        "feof" => with_stream(a[0], |s| s.eof as u32)?,
        "ferror" => 0,
        "fclose" => 0,
        "access" => {
            let path = String::from_utf8_lossy(&m.cstr(a[0])).into_owned();
            if FS.with(|fs| fs.borrow().named.iter().any(|n| n.0 == path)) { 0 } else { u32::MAX }
        }
        "getcwd" => {
            // the emulator's stand-in: with no buffer a block of the size, holding the working directory ("/")
            let size = a[1].max(2);
            let buf = if a[0] == 0 { m.malloc(size) } else { a[0] };
            m.set_u8(buf, b'/');
            m.set_u8(buf + 1, 0);
            buf
        }
        // only an unprefixed stand-in: the vectors of `plt:getenv` carry their own answers (SI_cusInit)
        "getenv" if !r.name.starts_with("plt:") => 0,
        _ => return None,
    };
    Some(Ret::eax(ret))
}

/// Called before every port: a case that does not call `begin` gets the recorded answers of its `plt:` calls.
pub fn reset() {
    ACTIVE.with(|c| c.set(false));
}
