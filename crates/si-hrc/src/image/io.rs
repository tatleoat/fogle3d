// SPDX-License-Identifier: MIT
//! The file helpers of the readers, on the image: the swapping reads of `byteswap.c`, the
//! string reader of `filetoken.c`, the skip of an unknown token of `utilio.c`, and the
//! header reader and version test of `headers.c`.
//!
//! Every read is the original's: `fread` straight into the field at its address, then the
//! bytes swapped in place, one item at a time, **only for the items that arrived whole**.
//! At the end of a truncated file a field therefore keeps what it held except for the
//! leading bytes that did arrive, unswapped.

use super::globals::{SI_VERSION_INPUT, TMP};
use super::outside::{sceneversioncheck_exec, Outside};
use si_mem::Mem;

/// [soft 0x083f9330 SI_swap2byte]: the two bytes at `p` exchanged; the first one passes
/// through the global `tmp` and stays there.
pub fn swap2byte(m: &mut Mem, p: u32) {
    let t = m.u8(p);
    m[TMP] = t;
    let b = m.u8(p + 1);
    m.set_u8(p, b);
    m.set_u8(p + 1, t);
}

/// [soft 0x083f9350 SI_swap4byte]: the four bytes at `p` reversed; `tmp` is left holding
/// what was the second one.
pub fn swap4byte(m: &mut Mem, p: u32) {
    let t = m.u8(p);
    m[TMP] = t;
    let b = m.u8(p + 3);
    m.set_u8(p, b);
    m.set_u8(p + 3, t);
    let t = m.u8(p + 1);
    m[TMP] = t;
    let b = m.u8(p + 2);
    m.set_u8(p + 1, b);
    m.set_u8(p + 2, t);
}

/// [soft 0x083f9388 SI_swap8byte]: the eight bytes at `p` reversed; `tmp` is left holding
/// what was the fourth one.
pub fn swap8byte(m: &mut Mem, p: u32) {
    for k in 0..4 {
        let t = m.u8(p + k);
        m[TMP] = t;
        let b = m.u8(p + 7 - k);
        m.set_u8(p + k, b);
        m.set_u8(p + 7 - k, t);
    }
}

/// [soft 0x083f9320 validate_float]: does nothing.
pub fn validate_float(_m: &mut Mem, _p: u32) {}

/// [soft 0x083f9ba0 SI_byteswap_fread]: `fread`, then each whole item of 2, 4 or 8 bytes
/// swapped in place. Items of any other size are left as they came.
pub fn byteswap_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    if size != 1 {
        let mut i = 0;
        while i < got {
            match size {
                4 => swap4byte(m, ptr.wrapping_add(4 * i)),
                2 => swap2byte(m, ptr.wrapping_add(2 * i)),
                8 => swap8byte(m, ptr.wrapping_add(8 * i)),
                _ => {}
            }
            i += 1;
        }
    }
    got
}

/// [soft 0x083f9a7c SI_float_fread]: `fread`, then four bytes swapped per item that
/// arrived, four bytes apart whatever the item size passed.
pub fn float_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    let mut i = 0;
    while i < got {
        let p = ptr.wrapping_add(4 * i);
        swap4byte(m, p);
        validate_float(m, p);
        i += 1;
    }
    got
}

/// [soft 0x083f9328 validate_double]: does nothing.
pub fn validate_double(_m: &mut Mem, _p: u32) {}

/// [soft 0x083f9ad0 SI_double_fread]: `fread`, then eight bytes swapped per item that
/// arrived, eight bytes apart whatever the item size passed.
pub fn double_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    let mut i = 0;
    while i < got {
        let p = ptr.wrapping_add(8u32.wrapping_mul(i));
        swap8byte(m, p);
        validate_double(m, p);
        i += 1;
    }
    got
}

/// The shape shared by the readers of a record of floats: `fread`, then per item that
/// arrived `floats` words swapped, the items `stride` bytes apart.
fn floats_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32, floats: u32, stride: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    let mut i = 0;
    while i < got {
        for k in 0..floats {
            let p = ptr.wrapping_add(stride.wrapping_mul(i)).wrapping_add(4 * k);
            swap4byte(m, p);
            validate_float(m, p);
        }
        i += 1;
    }
    got
}

/// [soft 0x083f9d54 SI_Vector_fread]: three floats per item, items 12 bytes apart.
pub fn vector_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    floats_fread(m, o, ptr, size, n, file, 3, 12)
}

/// [soft 0x083fa35c SI_RGB_fread]: three floats per item, items 12 bytes apart.
pub fn rgb_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    floats_fread(m, o, ptr, size, n, file, 3, 12)
}

/// [soft 0x083fa184 SI_RGBA_fread]: four floats per item, items 16 bytes apart.
pub fn rgba_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    floats_fread(m, o, ptr, size, n, file, 4, 16)
}

/// [soft 0x083f9ed4 SI_3doubles_fread]: `fread`, then three doubles swapped per item
/// **asked for**, whatever arrived.
pub fn three_doubles_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    let mut i = 0;
    while i < n.wrapping_mul(3) {
        swap8byte(m, ptr.wrapping_add(8u32.wrapping_mul(i)));
        i += 1;
    }
    for k in 0..3 {
        validate_double(m, ptr.wrapping_add(8 * k));
    }
    got
}

/// [soft 0x083fa4b4 SI_Matrix_fread]: `fread`, then the sixteen floats at `ptr` swapped,
/// **whatever arrived**: the swap is not held against the number of items read, so after a
/// read that failed the matrix holds its old words byte-reversed, and of several matrices
/// only the first is swapped.
pub fn matrix_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    for k in 0..16 {
        let p = ptr.wrapping_add(4 * k);
        swap4byte(m, p);
        validate_float(m, p);
    }
    got
}

/// [soft 0x083f9878 SI_fix_SI_FileChunk]: the magic and the version of a header swapped.
pub fn fix_file_chunk(m: &mut Mem, p: u32) {
    swap4byte(m, p);
    swap4byte(m, p.wrapping_add(4));
}

/// [soft 0x083faa40 SI_FileChunk_fread]: headers of 0x58 bytes; of each one that arrived
/// the first two words are swapped.
pub fn file_chunk_fread(m: &mut Mem, o: &mut dyn Outside, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
    let got = o.fread(m, ptr, size, n, file);
    let mut i = 0;
    while i < got {
        let p = ptr.wrapping_add(0x58u32.wrapping_mul(i));
        fix_file_chunk(m, p);
        validate_float(m, p.wrapping_add(4));
        i += 1;
    }
    got
}

/// [soft 0x084040a0 SI_stringBinaryRead]: bytes up to a zero byte or the end of the file,
/// in a new block of their number plus one (`SI_malloc`, then `strcpy`).
///
/// The original collects into 256 bytes of its stack without a bound. The four bytes behind
/// them are its own, so a string of up to 259 bytes comes out whole; from 260 on the
/// terminating zero and then the string land on its saved frame pointer and return
/// address. The port has no such frame: it records a fault there and goes on reading.
pub fn string_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32) -> u32 {
    let mut text: Vec<u8> = Vec::new();
    loop {
        let c = o.fgetc(m, file);
        if c == -1 || c == 0 {
            break;
        }
        text.push(c as u8);
    }
    if text.len() >= 260 {
        m.record_fault(m.sp(), text.len() as u32 + 1, si_mem::Access::Write);
    }
    let dest = m.si_malloc(text.len() as u32 + 1);
    m.put_cstr(dest, &text);
    dest
}

/// [soft 0x083ff174 si_fileSkipField]: passes over the data of a token the caller does not
/// know. The token's high byte decides: 0, nothing can be skipped (answers 0); 1 to 254,
/// that many bytes; 255, a 4-byte length follows and then that many bytes, read 1024 at a
/// time.
///
/// A negative length makes the original ask `fread` for one item of that size (as an
/// unsigned number) into its 1024 bytes of stack: everything left in the file arrives. The
/// port reads the same bytes; where more than the buffer and the four bytes behind it
/// would arrive it records a fault (the original's frame is overwritten there).
pub fn file_skip_field(m: &mut Mem, o: &mut dyn Outside, file: u32, token: i16) -> i32 {
    const BUF: u32 = 0x400;
    // the length word, then the buffer and the four bytes of the frame behind it
    let frame = m.local(4 + BUF + 4);
    let (len_at, buf) = (frame, frame + 4);
    let high = (token as u16 >> 8) as u32;
    m.set_u32(len_at, high);
    let result = 'skip: {
        if high.wrapping_sub(1) > 0xfd {
            if high != 0xff {
                break 'skip 0;
            }
            byteswap_fread(m, o, len_at, 4, 1, file);
            loop {
                let len = m.i32(len_at);
                // the compiler's signed division by 1024
                let blocks = (if len < 0 { len.wrapping_add(0x3ff) } else { len }) >> 10;
                if blocks < 1 {
                    break;
                }
                o.fread(m, buf, BUF, 1, file);
                m.set_i32(len_at, len.wrapping_sub(BUF as i32));
            }
            if m.u32(len_at) == 0 {
                break 'skip 1;
            }
        }
        let len = m.u32(len_at);
        if len <= BUF + 4 {
            o.fread(m, buf, len, 1, file);
        } else {
            // see above: the bytes that fit, then the rest of the file passed over
            let mut arrived = o.fread(m, buf, 1, BUF + 4, file);
            let mut over = false;
            while arrived == BUF + 4 {
                arrived = o.fread(m, buf, 1, BUF + 4, file);
                over |= arrived > 0;
            }
            if over {
                m.record_fault(buf + BUF + 4, 1, si_mem::Access::Write);
            }
        }
        1
    };
    m.release(4 + BUF + 4);
    result
}

/// Whether an allocation of `bytes` for `count` items read from the file may be made.
///
/// Not a call of the original, which asks for whatever a count in the file says. This
/// project's memory rule forbids that: a count read from a file is held against the bytes
/// left before allocating for it. The port therefore makes every allocation of up to 1 MB
/// as the original does (so that a short or truncated file leaves the original's blocks),
/// and a larger one only if the file still has at least one byte per item. A refused
/// allocation is recorded as a fault and answered like one the system refused: null. (A
/// request larger than the image's whole heap is passed on: the allocator refuses it, as
/// the original's does.)
pub fn alloc_fits(m: &mut Mem, o: &mut dyn Outside, file: u32, bytes: u32, count: u32) -> bool {
    const FREE: u32 = 1 << 20;
    if bytes <= FREE || bytes > m.region(si_mem::Region::Heap).1 || o.bytes_left(m, file) >= count as u64 {
        return true;
    }
    m.record_fault(file, bytes, si_mem::Access::Read);
    false
}

/// [soft 0x083fb934 si_dbfileIsOldUdfVersion]: whether a file of this version stores
/// shader references in the old form: up to 2.67906, and from 2.69 to below 2.999019
/// (the constants are the doubles at 0x0865c5a0, 0x0865c5a8 and 0x0865c5b0; the version is
/// compared as it is, widened).
pub fn is_old_udf_version(version: f32) -> bool {
    let v = version as f64;
    v <= 2.67906 || (v >= 2.69 && v < 2.999019)
}

/// [soft 0x083fb6fc SI_dbfileReadBinHeader]: reads the 88-byte header. `*version` is −1
/// first; if the magic is one of the 19 of the family (0x5380f634 to 0x5380f646), the
/// header's version goes to `*version` and to `SI_versionInput`, the version check hook
/// is called for a version above 4.0001, and the 80 bytes of text are copied to `text`
/// if that is not null (`strncpy`). Otherwise the stream is rewound and −1 answered. The
/// answer is the magic.
pub fn read_bin_header(m: &mut Mem, o: &mut dyn Outside, file: u32, version: u32, text: u32) -> i32 {
    let chunk = m.local(0x58);
    m.set_f32(version, -1.0);
    file_chunk_fread(m, o, chunk, 0x58, 1, file);
    let mut magic = m.i32(chunk);
    if (magic as u32).wrapping_add(0xac7f_09cc) < 0x13 {
        let v = m.f32(chunk + 4);
        m.set_f32(version, v);
        m[SI_VERSION_INPUT] = v;
        if (m.f32(version) as f64) > 4.0001 {
            sceneversioncheck_exec(m, o);
        }
        if text != 0 {
            si_mem::clib::strncpy(m, text, chunk + 8, 0x50);
        }
    } else {
        o.rewind(m, file);
        magic = -1;
    }
    m.release(0x58);
    magic
}
