// SPDX-License-Identifier: MIT
//! Raw function-curve segments in text form (`asceneio.c`; finding 0006, section 10): what
//! the Function Curve editor writes and reads through its menu (`scnMenu`: "Save Fcurve
//! Segment (Ascii)", a `.fraw` file, `SI_rawfcurveWrite` and `SI_rawfcurveRead`). The two
//! routines here are the ones that touch the text; the menu routines around them choose the
//! file in a dialog and are not ported.
//!
//! The text: the key count and the curve's interpolation on the first line, then one line a
//! key with its time, value, left slope and right slope:
//!
//! ```text
//! <count>     <interp> \n<time>     <value>     <left slope>     <right slope> \n…
//! ```
//!
//! (the writer prints `"%d "`, four spaces, `"%d "`, and before each key a newline, so the
//! text ends without one). The reader takes it a line at a time with `fgets` and `sscanf`.
//!
//! Both work on the program's `fcurve` record and its key array in the memory image. That
//! record's layout is not established in `si-types`; the fields are addressed by the named
//! offsets of [`fcurve`] and [`key`] (read from these two routines and `si-fcurve`'s key).
//! The routines the reader calls outside this source file (`SI_fcurveRemove`,
//! `SI_fcurveAllocate`, `SI_malloc`, `SI_fkeyInitialize`, `SI_statusDisplayMessage`) are
//! behind [`RawCalls`]; [`OUTSIDE`] names them with their addresses.

use crate::chn::scan::{Arg, CFile};
use crate::cnum;
use si_mem::Mem;

/// Offsets in the `fcurve` record.
pub mod fcurve {
    /// The key array (`0x24` bytes a key).
    pub const KEYS: u32 = 0x08;
    /// The number of keys (an `int`).
    pub const COUNT: u32 = 0x0c;
    /// The curve's interpolation (a `short`).
    pub const INTERP: u32 = 0x1e;
}

/// Offsets in a key of the array (`si-fcurve`'s `Key`).
pub mod key {
    pub const SIZE: u32 = 0x24;
    pub const TIME: u32 = 0x00;
    pub const VALUE: u32 = 0x04;
    pub const RIGHT_SLOPE: u32 = 0x08;
    pub const INTERP: u32 = 0x0e;
    /// The curve the key belongs to.
    pub const CURVE: u32 = 0x10;
    /// The editor's selection mark.
    pub const TAGGED: u32 = 0x14;
    pub const LEFT_SLOPE: u32 = 0x18;
}

/// A routine outside this source file, by its name in the binary and its address.
#[derive(Clone, Copy, Debug)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// What [`rawfcurve_ascii_read`] calls, as the check's stand-ins name them.
pub const OUTSIDE: [Routine; 5] = [
    Routine { name: "SI_fcurveRemove", addr: 0x081f_f0a0 },
    Routine { name: "SI_fcurveAllocate", addr: 0x0820_a810 },
    Routine { name: "SI_malloc", addr: 0x080d_0ee8 },
    Routine { name: "SI_fkeyInitialize", addr: 0x0820_bc50 },
    Routine { name: "SI_statusDisplayMessage", addr: 0x083c_ca94 },
];

/// The reader's two messages, at their addresses in the original's `.rodata` (what
/// `SI_statusDisplayMessage` is given).
pub const MSG_NO_FCURVE: u32 = 0x086c_bcd9; // "Cannot allocate raw fcurve"
pub const MSG_NO_KEYS: u32 = 0x086c_bd00; // "Cannot allocate raw fcurve keys"

/// The routines the reader calls, answered by the caller.
pub trait RawCalls {
    /// SI_fcurveRemove (0x081ff0a0): disposes of the curve `*slot` and clears it.
    fn fcurve_remove(&mut self, m: &mut Mem, slot: u32);
    /// SI_fcurveAllocate (0x0820a810): a new curve.
    fn fcurve_allocate(&mut self, m: &mut Mem, kind: u32, elem: u32) -> u32;
    /// SI_malloc (0x080d0ee8)
    fn malloc(&mut self, m: &mut Mem, size: u32) -> u32;
    /// SI_fkeyInitialize (0x0820bc50)
    fn fkey_initialize(&mut self, m: &mut Mem, key: u32);
    /// SI_statusDisplayMessage (0x083cca94): the message's address and the level.
    fn status_message(&mut self, m: &mut Mem, text: u32, level: i16);
}

/// [soft 0x083e5c14 rawfcurveAsciiWrite]: the curve `fc` as text, appended to `out`. Nothing
/// for a null curve. Always 1.
///
/// The count is read again before each key (the loop compares with the field, not with a
/// copy); the values are singles widened to double for `"%.7g "`.
pub fn rawfcurve_ascii_write(m: &Mem, out: &mut Vec<u8>, fc: u32) -> i32 {
    if fc == 0 {
        return 1;
    }
    let put = |out: &mut Vec<u8>, s: &str| out.extend_from_slice(s.as_bytes());
    let g = |out: &mut Vec<u8>, v: f32| {
        out.extend_from_slice(cnum::fmt_g(v as f64, 7).as_bytes());
        out.push(b' ');
    };
    put(out, &format!("{} ", m.i32(fc + fcurve::COUNT)));
    put(out, "    ");
    put(out, &format!("{} ", m.i16(fc + fcurve::INTERP)));
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(fc + fcurve::COUNT) {
        let k = m.ptr(fc + fcurve::KEYS).wrapping_add(off);
        put(out, "\n");
        g(out, m.f32(k + key::TIME));
        put(out, "    ");
        g(out, m.f32(k + key::VALUE));
        put(out, "    ");
        g(out, m.f32(k + key::LEFT_SLOPE));
        put(out, "    ");
        g(out, m.f32(k + key::RIGHT_SLOPE));
        off = off.wrapping_add(key::SIZE);
        i += 1;
    }
    1
}

/// `fgets(buf, n, f)`: up to `n − 1` bytes, to and with the first newline. `None` (a null
/// result) when the file ends before the first byte. What `sscanf` then sees ends at the
/// first NUL of the buffer.
fn fgets(f: &mut CFile, n: usize) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    while line.len() + 1 < n {
        let c = f.getc();
        if c == -1 {
            break;
        }
        line.push(c as u8);
        if c == b'\n' as i32 {
            break;
        }
    }
    if line.is_empty() && f.feof() {
        return None;
    }
    if let Some(z) = line.iter().position(|&b| b == 0) {
        line.truncate(z);
    }
    Some(line)
}

/// [soft 0x083dd880 rawfcurveAsciiRead]: a curve read from the text into `*slot`.
///
/// The curve already in `*slot` is removed, a new one allocated (`SI_fcurveAllocate(0,
/// elem)`) and stored there; then the first line gives the count and the interpolation
/// (`sscanf("%d %hud")`: the `d` after `%hu` is a literal that never matches). With a count
/// of zero or less the key pointer is cleared and the answer is 1. Otherwise the key array is
/// `SI_malloc`ed for the count rounded up to a multiple of eight, and each key is
/// initialised, read from its line (`"%f %f %f %f"`: time, value, left slope, right slope),
/// and given the curve, a clear selection mark and the curve's interpolation. A line the file
/// does not have leaves the key as its initialiser made it. 0, with a message on the status
/// line, when either allocation fails.
///
/// Every field is read back from the record each time, as the original does (it keeps only
/// the slot's address in a register). `Err` for a null slot: the original reads through it.
pub fn rawfcurve_ascii_read(m: &mut Mem, f: &mut CFile, slot: u32, elem: u32, calls: &mut dyn RawCalls) -> Result<i32, &'static str> {
    if slot == 0 {
        return Err("a null curve slot: the original reads through the null pointer");
    }
    if m.ptr(slot) != 0 {
        calls.fcurve_remove(m, slot);
    }
    let fc = calls.fcurve_allocate(m, 0, elem);
    m.set_ptr(slot, fc);
    if fc == 0 {
        calls.status_message(m, MSG_NO_FCURVE, 3);
        return Ok(0);
    }
    if let Some(line) = fgets(f, 99) {
        let fc = m.ptr(slot);
        let (mut count, mut interp) = (m.i32(fc + fcurve::COUNT), m.u16(fc + fcurve::INTERP));
        let mut l = CFile::new(&line);
        l.scanf("%d %hud", &mut [Arg::Int(&mut count), Arg::Half(&mut interp)]);
        m.set_i32(fc + fcurve::COUNT, count);
        m.set_u16(fc + fcurve::INTERP, interp);
    }
    let fc = m.ptr(slot);
    let count = m.i32(fc + fcurve::COUNT);
    if count <= 0 {
        m.set_ptr(fc + fcurve::KEYS, 0);
        return Ok(1);
    }
    // the count rounded up to a multiple of eight, times 0x24, in 32 bits. (The original also
    // adds one to `SI_secureLicAlloc`, 0x08679660, before the call: a counter of allocations
    // kept beside the licence client; not reproduced, as in the crate's other ports.)
    let rounded = count.wrapping_sub((count.wrapping_sub(1) & 7).wrapping_add(-7));
    let size = rounded.wrapping_mul(9).wrapping_shl(2) as u32;
    let keys = calls.malloc(m, size);
    let fc = m.ptr(slot);
    m.set_ptr(fc + fcurve::KEYS, keys);
    if m.ptr(m.ptr(slot) + fcurve::KEYS) == 0 {
        calls.status_message(m, MSG_NO_KEYS, 3);
        return Ok(0);
    }
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(m.ptr(slot) + fcurve::COUNT) {
        let k = off.wrapping_add(m.ptr(m.ptr(slot) + fcurve::KEYS));
        calls.fkey_initialize(m, k);
        if let Some(line) = fgets(f, 99) {
            let mut v = [m.f32(k + key::TIME), m.f32(k + key::VALUE), m.f32(k + key::LEFT_SLOPE), m.f32(k + key::RIGHT_SLOPE)];
            let mut l = CFile::new(&line);
            let [a, b, c, d] = &mut v;
            l.scanf("%f %f %f %f", &mut [Arg::Float(a), Arg::Float(b), Arg::Float(c), Arg::Float(d)]);
            // a value sscanf did not convert keeps its bits: writing it back changes nothing
            for (j, at) in [key::TIME, key::VALUE, key::LEFT_SLOPE, key::RIGHT_SLOPE].into_iter().enumerate() {
                m.set_f32(k + at, v[j]);
            }
        }
        let fc = m.ptr(slot);
        let keys = m.ptr(fc + fcurve::KEYS);
        m.set_ptr(keys.wrapping_add(off) + key::CURVE, fc);
        let keys = m.ptr(m.ptr(slot) + fcurve::KEYS);
        m.set_i16(keys.wrapping_add(off) + key::TAGGED, 0);
        let fc = m.ptr(slot);
        let interp = m.u16(fc + fcurve::INTERP);
        m.set_u16(m.ptr(fc + fcurve::KEYS).wrapping_add(off) + key::INTERP, interp);
        i += 1;
        off = off.wrapping_add(key::SIZE);
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed {
        fc: u32,
        keys: u32,
        log: Vec<String>,
    }

    impl RawCalls for Fixed {
        fn fcurve_remove(&mut self, _m: &mut Mem, slot: u32) {
            self.log.push(format!("remove {slot:x}"));
        }
        fn fcurve_allocate(&mut self, _m: &mut Mem, _kind: u32, _elem: u32) -> u32 {
            self.fc
        }
        fn malloc(&mut self, _m: &mut Mem, size: u32) -> u32 {
            self.log.push(format!("malloc {size:#x}"));
            self.keys
        }
        fn fkey_initialize(&mut self, m: &mut Mem, k: u32) {
            m.set_bytes(k, &[0; 0x24]);
        }
        fn status_message(&mut self, _m: &mut Mem, text: u32, level: i16) {
            self.log.push(format!("status {text:x} {level}"));
        }
    }

    #[test]
    fn written_text_reads_back() {
        let mut m = Mem::new();
        let (fc, keys, slot) = (m.place(0x40, 8), m.place(16 * key::SIZE, 8), m.place(4, 8));
        m.set_bytes(fc, &[0; 0x40]);
        m.set_ptr(fc + fcurve::KEYS, keys);
        m.set_i32(fc + fcurve::COUNT, 2);
        m.set_i16(fc + fcurve::INTERP, 3);
        for (i, v) in [[1.0f32, 2.5, -0.125, 1e-9], [30.0, -7.0, 0.0, 123456.7]].iter().enumerate() {
            let k = keys + i as u32 * key::SIZE;
            m.set_f32(k + key::TIME, v[0]);
            m.set_f32(k + key::VALUE, v[1]);
            m.set_f32(k + key::LEFT_SLOPE, v[2]);
            m.set_f32(k + key::RIGHT_SLOPE, v[3]);
        }
        let mut out = Vec::new();
        assert_eq!(rawfcurve_ascii_write(&m, &mut out, fc), 1);
        assert_eq!(out, b"2     3 \n1     2.5     -0.125     1e-09 \n30     -7     0     123456.7 ".to_vec());
        // read it into a fresh curve
        let (fc2, keys2) = (m.place(0x40, 8), m.place(16 * key::SIZE, 8));
        m.set_bytes(fc2, &[0; 0x40]);
        m.set_ptr(slot, 0);
        let mut calls = Fixed { fc: fc2, keys: keys2, log: Vec::new() };
        let mut f = CFile::new(&out);
        assert_eq!(rawfcurve_ascii_read(&mut m, &mut f, slot, 0, &mut calls), Ok(1));
        assert_eq!(calls.log, vec![format!("malloc {:#x}", 8 * 0x24)]);
        assert_eq!((m.ptr(slot), m.i32(fc2 + fcurve::COUNT), m.i16(fc2 + fcurve::INTERP)), (fc2, 2, 3));
        let k = keys2 + key::SIZE;
        assert_eq!((m.f32(k + key::TIME), m.f32(k + key::RIGHT_SLOPE)), (30.0, 123456.7));
        assert_eq!((m.ptr(k + key::CURVE), m.i16(k + key::INTERP)), (fc2, 3));
    }

    #[test]
    fn no_count_no_keys_and_failed_allocations() {
        let mut m = Mem::new();
        let (fc, slot) = (m.place(0x40, 8), m.place(4, 8));
        m.set_bytes(fc, &[0x11; 0x40]);
        m.set_i32(fc + fcurve::COUNT, 0);
        m.set_ptr(slot, 0);
        let mut calls = Fixed { fc, keys: 0, log: Vec::new() };
        // a first line with no number: the count the allocator left (0) stands
        assert_eq!(rawfcurve_ascii_read(&mut m, &mut f_of(b"abc\n"), slot, 0, &mut calls), Ok(1));
        assert_eq!(m.ptr(fc + fcurve::KEYS), 0);
        // keys asked for, none given
        assert_eq!(rawfcurve_ascii_read(&mut m, &mut f_of(b"3 1\n"), slot, 0, &mut calls), Ok(0));
        assert_eq!(calls.log.last().unwrap(), &format!("status {MSG_NO_KEYS:x} 3"));
        assert_eq!(calls.log[0], format!("remove {slot:x}"));
        // no curve
        let mut none = Fixed { fc: 0, keys: 0, log: Vec::new() };
        assert_eq!(rawfcurve_ascii_read(&mut m, &mut f_of(b"1 1\n"), slot, 0, &mut none), Ok(0));
        assert_eq!(none.log.last().unwrap(), &format!("status {MSG_NO_FCURVE:x} 3"));
        assert!(rawfcurve_ascii_read(&mut m, &mut f_of(b""), 0, 0, &mut none).is_err());
    }

    fn f_of(b: &'static [u8]) -> CFile<'static> {
        CFile::new(b)
    }
}
