// SPDX-License-Identifier: MIT
//! `utilio.c` and `filetoken.c`: the sub-records several element files share: a value, three
//! values, a vector, a text, and the step over a field nobody knows.

use super::byteswap::{byteswap_fread, float_fread};
use super::globals::{SI_FVALUE, TOKEN_UTILIO};
use super::outside::{fgetc, fread, Outside};
use super::{fmov, NoReturn, Run};
use si_mem::Mem;

/// [soft 0x084040a0 SI_stringBinaryRead]
/// Bytes up to a zero byte or the end of the file, as a text in a block of its own
/// (`SI_malloc(length + 1)`).
///
/// The original collects the bytes in 256 bytes of its stack with no bound: the 257th byte
/// it stores (a text of 256 characters and its zero) is the low byte of its caller's frame
/// pointer. Rust must differ: the port stops there.
pub fn string_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32) -> Run<u32> {
    let sp = m.sp();
    let local = m.local(0x100);
    let mut n = 0u32;
    loop {
        let c = fgetc(o, m, stream);
        if c == -1 || c == 0 {
            break;
        }
        if n >= 0x100 {
            m.set_sp(sp);
            return Err(NoReturn::Stack("SI_stringBinaryRead"));
        }
        m.set_u8(local + n, c as u8);
        n += 1;
    }
    if n >= 0x100 {
        m.set_sp(sp);
        return Err(NoReturn::Stack("SI_stringBinaryRead"));
    }
    m.set_u8(local + n, 0);
    let dest = m.si_malloc(n + 1);
    // strcpy(dest, local): a null `dest` is a fault of the image, as it is in the original
    si_mem::clib::strcpy(m, dest, local);
    m.set_sp(sp);
    Ok(dest)
}

/// [soft 0x083ff174 si_fileSkipField]
/// Steps over the field of a token nobody knows, by the size in the token's high byte: 1
/// to 0xfe that many bytes, 0xff a 32-bit size first, 0 nothing (and the answer is 0).
///
/// The bytes are read into 1 KB of the original's stack, 1 KB at a time while the size is
/// 1 KB or more; a negative size is read in one piece. Rust must differ where that piece
/// is longer than the local (a negative size with more than 1 KB of file left): the port
/// stops. With less left the original reads the rest of the file and so does the port.
pub fn file_skip_field(m: &mut Mem, o: &mut dyn Outside, stream: u32, token: i16) -> Run<u32> {
    let sp = m.sp();
    let buffer = m.local(0x400);
    let size_at = m.local(4);
    let high = (token as u16 >> 8) as u32;
    m.set_u32(size_at, high);
    let mut size = high;
    if high.wrapping_sub(1) > 0xfd {
        if high != 0xff {
            m.set_sp(sp);
            return Ok(0);
        }
        byteswap_fread(m, o, size_at, 4, 1, stream);
        loop {
            size = m.u32(size_at);
            // the signed size divided by 1024, towards zero
            if (size as i32) / 0x400 < 1 {
                break;
            }
            fread(o, m, buffer, 0x400, 1, stream);
            m.set_u32(size_at, size.wrapping_sub(0x400));
        }
        if size == 0 {
            m.set_sp(sp);
            return Ok(1);
        }
    }
    if size > 0x400 && o.file_left(m, stream) > 0x400 {
        m.set_sp(sp);
        return Err(NoReturn::Stack("si_fileSkipField"));
    }
    fread(o, m, buffer, size, 1, stream);
    m.set_sp(sp);
    Ok(1)
}

/// [soft 0x083fd564 si_valueBinaryRead]
/// A value: an inner token, then for 1 a float (stored through `value` when that is not
/// null), for 2 nothing, for anything else the field is stepped over. The two further
/// arguments (a curve's place and the owner) are not looked at.
pub fn value_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, value: u32, _fcurve: u32, _owner: u32) -> Run<u32> {
    byteswap_fread(m, o, TOKEN_UTILIO.addr(), 2, 1, stream);
    let token = m[TOKEN_UTILIO];
    if token == 1 {
        float_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
        if value != 0 {
            let v = fmov(m[SI_FVALUE]);
            m.set_u32(value, v);
        }
    } else if token != 2 {
        file_skip_field(m, o, stream, token as i16)?;
    }
    Ok(1)
}

/// [soft 0x083fd6a0 si_value3BinaryRead]
/// Three values under one inner token: for 1 three floats, each stored through its
/// pointer when that is not null.
#[allow(clippy::too_many_arguments)]
pub fn value3_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, a: u32, b: u32, c: u32, _fa: u32, _fb: u32, _fc: u32, _owner: u32) -> Run<u32> {
    byteswap_fread(m, o, TOKEN_UTILIO.addr(), 2, 1, stream);
    let token = m[TOKEN_UTILIO];
    if token == 1 {
        float_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
        let va = fmov(m[SI_FVALUE]);
        float_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
        let vb = fmov(m[SI_FVALUE]);
        float_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
        let vc = fmov(m[SI_FVALUE]);
        if a != 0 {
            m.set_u32(a, va);
        }
        if b != 0 {
            m.set_u32(b, vb);
        }
        if c != 0 {
            m.set_u32(c, vc);
        }
    } else if token != 2 {
        file_skip_field(m, o, stream, token as i16)?;
    }
    Ok(1)
}

/// [soft 0x083fd350 si_vectorBinaryRead]
/// Three floats, stored when `vector` is not null; answers whether it was not.
pub fn vector_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, vector: u32) -> u32 {
    byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
    let x = fmov(m[SI_FVALUE]);
    byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
    let y = fmov(m[SI_FVALUE]);
    byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
    let z = fmov(m[SI_FVALUE]);
    if vector != 0 {
        m.set_u32(vector, x);
        m.set_u32(vector + 4, y);
        m.set_u32(vector + 8, z);
    }
    (vector != 0) as u32
}
