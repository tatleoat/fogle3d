// SPDX-License-Identifier: MIT
//! A file host for the readers: the C library's streams on byte vectors.
//!
//! Nothing here reproduces a function of the original. [`Files`] answers the stream calls
//! the readers make through [`super::Outside`] (`fread`, `fgetc`, `fopen`, `fclose`, `rewind`) the
//! way the emulation harness's stand-ins answer them (`re/tools/emu/standins.py`): a read
//! at the end of the data stores the bytes that are there and answers the whole items; a
//! stream's address is `0x6f000000 + 0x100 × n`, the first three being the standard
//! streams. A caller's world hands it the calls it does not answer itself.

use super::outside::{Routine, FCLOSE, FGETC, FOPEN, FREAD, REWIND};
use si_mem::{Kind, Mem};
use std::collections::BTreeMap;

/// One open stream.
#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub data: Vec<u8>,
    pub pos: usize,
    /// Set by a read that met the end.
    pub eof: bool,
    pub closed: bool,
}

/// The streams of a run and the files `fopen` finds by name.
#[derive(Clone, Debug)]
pub struct Files {
    streams: Vec<Stream>,
    named: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Default for Files {
    fn default() -> Self {
        Files::new()
    }
}

impl Files {
    /// The three standard streams, empty.
    pub fn new() -> Files {
        Files { streams: vec![Stream::default(), Stream::default(), Stream::default()], named: BTreeMap::new() }
    }

    /// Opens a stream on bytes and answers its address (a `FILE *` for the readers).
    pub fn open(&mut self, data: Vec<u8>) -> u32 {
        self.streams.push(Stream { data, ..Stream::default() });
        Kind::File.nth(self.streams.len() as u32 - 1)
    }

    /// A file `fopen` will find under this path.
    pub fn add_named(&mut self, path: &[u8], data: Vec<u8>) {
        self.named.insert(path.to_vec(), data);
    }

    pub fn stream(&self, address: u32) -> Option<&Stream> {
        Kind::File.number(address).and_then(|n| self.streams.get(n as usize))
    }

    fn stream_mut(&mut self, address: u32) -> Option<&mut Stream> {
        Kind::File.number(address).and_then(|n| self.streams.get_mut(n as usize))
    }

    /// The bytes a stream has left; 0 for what is no open stream.
    pub fn left(&self, address: u32) -> u32 {
        self.stream(address).map_or(0, |s| s.data.len().saturating_sub(s.pos).min(u32::MAX as usize) as u32)
    }

    /// Answers a call of one of the five stream routines; `None` for any other routine.
    /// A call on an address that is no open stream is a fault of the image (the harness
    /// stops such a case) and answers 0, or −1 for `fgetc`.
    pub fn call(&mut self, m: &mut Mem, routine: &Routine, args: &[u32]) -> Option<u32> {
        let arg = |k: usize| args.get(k).copied().unwrap_or(0);
        if routine.addr == FREAD.addr {
            let (buffer, size, count) = (arg(0), arg(1), arg(2));
            let Some(s) = self.stream_mut(arg(3)).filter(|s| !s.closed) else {
                m.record_fault(arg(3), 1, si_mem::Access::Read);
                return Some(0);
            };
            let want = (size as u64 * count as u64).min(usize::MAX as u64) as usize;
            let have = s.data.len().saturating_sub(s.pos);
            let n = want.min(have);
            if n < want {
                s.eof = true;
            }
            let from = s.pos;
            s.pos += n;
            if n > 0 {
                m.set_bytes(buffer, &self.stream(arg(3)).unwrap().data[from..from + n]);
            }
            return Some(if size == 0 { 0 } else { (n / size as usize) as u32 });
        }
        if routine.addr == FGETC.addr {
            let Some(s) = self.stream_mut(arg(0)).filter(|s| !s.closed) else {
                m.record_fault(arg(0), 1, si_mem::Access::Read);
                return Some(u32::MAX);
            };
            if s.pos >= s.data.len() {
                s.eof = true;
                return Some(u32::MAX);
            }
            s.pos += 1;
            return Some(s.data[s.pos - 1] as u32);
        }
        if routine.addr == FOPEN.addr {
            let path = m.cstr(arg(0));
            let Some(data) = self.named.get(&path).cloned() else {
                return Some(0);
            };
            return Some(self.open(data));
        }
        if routine.addr == REWIND.addr {
            match self.stream_mut(arg(0)).filter(|s| !s.closed) {
                Some(s) => {
                    s.pos = 0;
                    s.eof = false;
                }
                None => m.record_fault(arg(0), 1, si_mem::Access::Read),
            }
            return Some(0);
        }
        if routine.addr == FCLOSE.addr {
            match self.stream_mut(arg(0)).filter(|s| !s.closed) {
                Some(s) => s.closed = true,
                None => m.record_fault(arg(0), 1, si_mem::Access::Read),
            }
            return Some(0);
        }
        None
    }
}
