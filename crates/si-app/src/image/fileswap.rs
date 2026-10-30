// SPDX-License-Identifier: MIT
//! The rest of `byteswap.c`, of `utilio.c`'s four-float helpers, `in_out.c`, `headers.c`'s
//! installer and `sys_tmpdir.c`, on the image (finding 0019, section 13c).
//!
//! These are the file layer of the loaders and writers that every other file reader hangs
//! on: the in-place byte turns of the custom shader and function-curve blocks of a model
//! file, the swapping reads and writes of the chunks of picture, line and bound records,
//! the helper that reads a quoted string from a text stream, and the directory a temporary
//! file goes to. Everything works on the image the way the original works on memory: the
//! byte turns go through the file's one static byte `tmp` (which keeps the last byte moved,
//! so the global is written exactly as the original writes it), and the four floats of a
//! homogeneous vector pass through `si_fvalue`.
//!
//! The x87 unit matters in two places. `hvectorBinaryWrite` and `si_hvectorBinaryRead` move
//! each float through `fld`/`fstp`: a signalling NaN comes out as the quiet NaN with the
//! same payload ([`quiet`]); every other pattern comes back unchanged. `validate_float`
//! (0x083f9320) and `validate_double` (0x083f9328) do nothing in this build, so the calls
//! to them are left out.
//!
//! Where Rust must differ: the byte reads of the custom blocks (`fix_CustomFcurveKey` and
//! `fix_CustomDsoFcurve`) keep a short in a local that the original leaves uninitialised
//! when the direction flag is neither 0 nor 1; the port starts it at 0 (ending the loop at
//! once). No caller passes another flag. `SI_agetString` collects its text in a local of
//! 256 bytes the original never checks; a longer text overruns that frame there, and here
//! the local is as long as the text needs.

use si_mem::{outside_calls, CStr32, Mem, Outside};

/// The globals of these routines.
pub mod g {
    si_mem::globals! {
        /// `tmp` (byteswap.c): the byte the swaps move through.
        TMP: u8 = 0x08817f38, "tmp";
        /// `si_fvalue` (utilio.c): the float the vector helpers read and write through.
        SI_FVALUE: u32 = 0x08817f40, "si_fvalue";
        /// `scene_version_check` (headers.c): the function the scene reader asks.
        SCENE_VERSION_CHECK: u32 = 0x086cebc0, "scene_version_check";
        /// `temporaryDirectory` (sys_tmpdir.c): the directory found by the first call.
        TEMPORARY_DIRECTORY: u32 = 0x086d1de0, "temporaryDirectory";
    }
}

/// The texts of `sys_tmpdir.c` (in `.data` behind the cache pointer): the directory tried
/// first, and the one used when it cannot be written. The second is an empty string in the
/// original's image (the emulation's `strdup` of it answered one zero byte), so a machine
/// without a writable `/usr/tmp` is given the current directory.
pub const TEXTS: &[(u32, &[u8])] = &[(0x086d1de4, b"/usr/tmp"), (0x086d1ded, b"")];

outside_calls! {
    /// The C library's stream and file calls these routines make.
    pub trait FileCalls {
        /// `fread(buffer, size, count, stream)`: the bytes go into the image; answers the
        /// number of whole items.
        fn fread(buffer: u32, size: u32, count: u32, stream: u32) -> u32 = "fread", 0x080c4108;
        /// `fwrite(buffer, size, count, stream)`: answers the number of whole items.
        fn fwrite(buffer: u32, size: u32, count: u32, stream: u32) -> u32 = "fwrite", 0x080c4f78;
        /// `fgetc(stream)`: the next byte, or -1 at the end.
        fn fgetc(stream: u32) -> i32 = "fgetc", 0x080c3828;
        /// `read(fd, buffer, bytes)`: the bytes go into the image; answers how many.
        fn read(fd: i32, buffer: u32, bytes: u32) -> i32 = "read", 0x080c50e8;
        /// `access(path, mode)`.
        fn access(path: CStr32, mode: i32) -> i32 = "access", 0x080c3f18;
    }
}

// ---------------------------------------------------------------- the byte turns

/// `tmp = p[a]; p[a] = p[b]; p[b] = tmp` (the body of the three swap routines).
fn swap_bytes(m: &mut Mem, p: u32, a: u32, b: u32) {
    let t = m.u8(p.wrapping_add(a));
    m.set_u8(g::TMP.0, t);
    let v = m.u8(p.wrapping_add(b));
    m.set_u8(p.wrapping_add(a), v);
    let t = m.u8(g::TMP.0);
    m.set_u8(p.wrapping_add(b), t);
}

/// SI_swap2byte (0x083f9330), as `swapnbyte` and the other routines here call it.
fn swap2(m: &mut Mem, p: u32) {
    swap_bytes(m, p, 0, 1);
}

/// SI_swap4byte (0x083f9350).
fn swap4(m: &mut Mem, p: u32) {
    swap_bytes(m, p, 0, 3);
    swap_bytes(m, p, 1, 2);
}

/// SI_swap8byte (0x083f9388).
fn swap8(m: &mut Mem, p: u32) {
    swap_bytes(m, p, 0, 7);
    swap_bytes(m, p, 1, 6);
    swap_bytes(m, p, 2, 5);
    swap_bytes(m, p, 3, 4);
}

/// [soft 0x083f93f0 swapnbyte]: the 2, 4 or 8 bytes at `p` turned; any other size does
/// nothing.
pub fn swapnbyte(m: &mut Mem, p: u32, n: u32) {
    match n {
        2 => swap2(m, p),
        4 => swap4(m, p),
        8 => swap8(m, p),
        _ => {}
    }
}

/// A signed byte as the short the routines keep it in (`movsx ax, byte`).
fn sbyte(m: &Mem, a: u32) -> i16 {
    m.u8(a) as i8 as i16
}

/// [soft 0x083f9440 fix_CustomFcurveKey]: one key of a custom function curve, from the
/// offset `*pos` of the block `buf`; `dir` 0 on the way in (the tag is read before it is
/// turned), 1 on the way out (after). Tags 1 to 4 are followed by a float (turned), tags 5
/// to 9 by a short (turned); tag 0 ends the key. `*pos` moves past what was turned.
pub fn fix_custom_fcurve_key(m: &mut Mem, buf: u32, pos: u32, dir: i16) {
    // (uninitialised in the original when `dir` is neither 0 nor 1)
    let mut tag: i16 = 0;
    loop {
        let at = buf.wrapping_add(m.u32(pos));
        if dir == 0 {
            tag = sbyte(m, at);
        }
        swapnbyte(m, at, 2);
        if dir == 1 {
            tag = sbyte(m, at);
        }
        m.set_u32(pos, m.u32(pos).wrapping_add(2));
        match tag {
            1..=4 => {
                // (validate_float on the word before and after: nothing)
                let at = buf.wrapping_add(m.u32(pos));
                swap4(m, at);
                m.set_u32(pos, m.u32(pos).wrapping_add(4));
            }
            5..=9 => {
                let at = buf.wrapping_add(m.u32(pos));
                swapnbyte(m, at, 2);
                m.set_u32(pos, m.u32(pos).wrapping_add(2));
            }
            _ => {}
        }
        if tag == 0 {
            break;
        }
    }
}

/// [soft 0x083f952c fix_CustomDsoFcurve]: one function curve of a custom shader: tags 1 to
/// 6 are followed by a short, tag 2's being the number of keys that tag 7 then holds; tag 8
/// by a long that is also taken for the count; tag 0 ends the curve. `local_c` starts at −1
/// (so tag 7 without a count turns nothing).
pub fn fix_custom_dso_fcurve(m: &mut Mem, buf: u32, pos: u32, dir: i16) {
    let mut keys: i32 = -1;
    let mut tag: i16 = 0; // (uninitialised in the original for a `dir` other than 0 and 1)
    loop {
        let at = buf.wrapping_add(m.u32(pos));
        if dir == 0 {
            tag = sbyte(m, at);
        }
        swapnbyte(m, at, 2);
        if dir == 1 {
            tag = sbyte(m, at);
        }
        m.set_u32(pos, m.u32(pos).wrapping_add(2));
        match tag {
            1..=6 => {
                let at = buf.wrapping_add(m.u32(pos));
                if tag == 2 && dir == 0 {
                    keys = m.u8(at) as i8 as i32;
                }
                swapnbyte(m, at, 2);
                if tag == 2 && dir == 1 {
                    keys = m.u8(at) as i8 as i32;
                }
                m.set_u32(pos, m.u32(pos).wrapping_add(2));
            }
            7 => {
                let mut n = keys;
                if n > 0 {
                    loop {
                        fix_custom_fcurve_key(m, buf, pos, dir);
                        n = n.wrapping_sub(1);
                        if n == 0 {
                            break;
                        }
                    }
                }
            }
            8 => {
                let at = buf.wrapping_add(m.u32(pos));
                if dir == 0 {
                    keys = m.u8(at) as i8 as i32;
                }
                swapnbyte(m, at, 4);
                if dir == 1 {
                    keys = m.u8(at) as i8 as i32;
                }
                m.set_u32(pos, m.u32(pos).wrapping_add(4));
            }
            _ => {}
        }
        if tag == 0 {
            break;
        }
    }
}

/// The data block of a user node: `+8` its byte count, `+0xc` the pointer to its bytes.
fn node_block(m: &Mem, node: u32) -> (u32, i32) {
    (m.u32(node.wrapping_add(0xc)), m.i32(node.wrapping_add(8)))
}

/// [soft 0x083f966c SI_fix_SI_CustomHeaderOld]: the old custom header, as much of it as the
/// block holds: longs at 0 and 4, then those at 0x30 and 0x34 when the block reaches them.
pub fn fix_custom_header_old(m: &mut Mem, node: u32, _dir: i16) {
    let (p, n) = node_block(m, node);
    let mut done: i32 = 0;
    if 0 < n {
        swap4(m, p);
        done = 4;
        if done < n {
            swap4(m, p.wrapping_add(4));
            done = 8;
        }
    }
    if done + 0x28 < n {
        swap4(m, p.wrapping_add(0x30));
        if done + 0x2c < n {
            swap4(m, p.wrapping_add(0x34));
        }
    }
}

/// [soft 0x083f96e4 SI_fix_SI_CustomHeader]: the custom header's four longs, at 0, 4, 0x30
/// and 0x34, whatever the block's size.
pub fn fix_custom_header(m: &mut Mem, node: u32, _dir: i16) {
    let (p, _) = node_block(m, node);
    swapnbyte(m, p, 4);
    swapnbyte(m, p.wrapping_add(4), 4);
    swapnbyte(m, p.wrapping_add(0x30), 4);
    swapnbyte(m, p.wrapping_add(0x34), 4);
}

/// [soft 0x083f9730 SI_fix_CustomDsoFcurves]: the block holds, one after another, a name
/// (a C string), a short (a count, never used) and a function curve; each is turned until
/// the offset reaches the block's size.
pub fn fix_custom_dso_fcurves(m: &mut Mem, node: u32, dir: i16) {
    let (p, n) = node_block(m, node);
    let at = m.local(4);
    m.set_u32(at, 0);
    if 0 < n {
        loop {
            let off = m.u32(at);
            let s = p.wrapping_add(off);
            let mut len = 0u32;
            while m.u8(s.wrapping_add(len)) != 0 {
                len = len.wrapping_add(1);
            }
            let off = off.wrapping_add(len).wrapping_add(1);
            m.set_u32(at, off);
            let here = p.wrapping_add(off);
            // (the short is read into a register whose only use is an empty loop)
            swapnbyte(m, here, 2);
            m.set_u32(at, off.wrapping_add(2));
            fix_custom_dso_fcurve(m, p, at, dir);
            if (m.i32(at)) >= n {
                break;
            }
        }
    }
    m.release(4);
}

/// [soft 0x083f9810 SI_fix_CustomFcurves]: the block's longs, `size / 4` of them, turned
/// one by one.
pub fn fix_custom_fcurves(m: &mut Mem, node: u32, _dir: i16) {
    let (mut p, n) = node_block(m, node);
    let mut count = (n as u32) >> 2;
    while count != 0 {
        swap4(m, p);
        p = p.wrapping_add(4);
        count -= 1;
    }
}

// ---------------------------------------------------------------- reads through read(2)

/// [soft 0x083f9918 SI_byteswap_read]: `read(fd, buf, size × count)`, then each of `count`
/// items turned when `size` is 2, 4 or 8 (whatever the read brought: the loop counts the
/// items asked for). Answers `read`'s answer.
pub fn byteswap_read(m: &mut Mem, o: &mut dyn Outside, fd: i32, buf: u32, size: u32, count: u32) -> i32 {
    let got = o.read(m, fd, buf, size.wrapping_mul(count));
    if size != 1 {
        let (mut p2, mut p4, mut p8) = (buf, buf, buf);
        let mut i = 0u32;
        while i < count {
            match size {
                4 => swap4(m, p4),
                2 => swap2(m, p2),
                8 => swap8(m, p8),
                _ => {}
            }
            p4 = p4.wrapping_add(4);
            p2 = p2.wrapping_add(2);
            p8 = p8.wrapping_add(8);
            i += 1;
        }
    }
    got
}

/// [soft 0x083f99c0 SI_float_read]: `read`, then each of `count` four-byte items turned.
pub fn float_read(m: &mut Mem, o: &mut dyn Outside, fd: i32, buf: u32, size: u32, count: u32) -> i32 {
    let got = o.read(m, fd, buf, count.wrapping_mul(size));
    let mut p = buf;
    let mut i = 0u32;
    while i < count {
        swap4(m, p);
        p = p.wrapping_add(4);
        i += 1;
    }
    got
}

/// [soft 0x083f9a1c SI_double_read]: `read`, then each of `count` eight-byte items turned.
pub fn double_read(m: &mut Mem, o: &mut dyn Outside, fd: i32, buf: u32, size: u32, count: u32) -> i32 {
    let got = o.read(m, fd, buf, count.wrapping_mul(size));
    let mut p = buf;
    let mut i = 0u32;
    while i < count {
        swap8(m, p);
        p = p.wrapping_add(8);
        i += 1;
    }
    got
}

/// [soft 0x083f9b2c SI_read_BitHeader]: `read`, then in each of `count` records of `size`
/// bytes the nine shorts from offset 0x118 turned.
pub fn read_bit_header(m: &mut Mem, o: &mut dyn Outside, fd: i32, buf: u32, size: u32, count: u32) -> i32 {
    let got = o.read(m, fd, buf, size.wrapping_mul(count));
    let mut i = 0u32;
    while i < count {
        let mut p = buf.wrapping_add(size.wrapping_mul(i)).wrapping_add(0x118);
        for _ in 0..9 {
            swap2(m, p);
            p = p.wrapping_add(2);
        }
        i += 1;
    }
    got
}

// ---------------------------------------------------------------- chunks through stdio

/// The four-long records (stride 16) the bound and homogeneous-vector chunks swap: the
/// longs at 0, 4, 8 and 0xc of each of `n` records.
fn swap_records16(m: &mut Mem, p: u32, n: u32) {
    let mut i = 0u32;
    while i < n {
        let q = p.wrapping_add(i.wrapping_mul(16));
        swap4(m, q);
        swap4(m, q.wrapping_add(4));
        swap4(m, q.wrapping_add(8));
        swap4(m, q.wrapping_add(12));
        i += 1;
    }
}

/// The write of a chunk: the `n` records turned in place by `turn`, `fwrite`, turned back.
fn chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, turn: fn(&mut Mem, u32, u32), buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    turn(m, buf, n);
    let written = o.fwrite(m, buf, size, n, stream);
    turn(m, buf, n);
    written
}

/// [soft 0x083fa078 SI_Hvector_fwrite]: `n` homogeneous vectors (four floats, 16 bytes
/// each) turned, written as `n` items of `size` bytes, turned back.
pub fn hvector_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_records16, buf, size, n, stream)
}

/// [soft 0x083fa67c SI_FbndChunk_fwrite]: the float-bounds chunks (four floats each), the
/// same.
pub fn fbnd_chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_records16, buf, size, n, stream)
}

/// [soft 0x083fa81c SI_IbndChunk_fwrite]: the integer-bounds chunks (four longs each), the
/// same.
pub fn ibnd_chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_records16, buf, size, n, stream)
}

/// [soft 0x083fa78c SI_IbndChunk_fread]: `fread`, then the four longs of each item that
/// was read whole turned.
pub fn ibnd_chunk_fread(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    let got = o.fread(m, buf, size, n, stream);
    swap_records16(m, buf, got);
    got
}

fn swap_coords2d(m: &mut Mem, p: u32, n: u32) {
    let mut i = 0u32;
    while i < n {
        let q = p.wrapping_add(i.wrapping_mul(8));
        swap4(m, q);
        swap4(m, q.wrapping_add(4));
        i += 1;
    }
}

/// [soft 0x083fa9a8 SI_Fcoor2D_fwrite]: `n` points of two floats turned, written, turned
/// back.
pub fn fcoor2d_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_coords2d, buf, size, n, stream)
}

/// `SI_fix_SI_PictChunk` (0x083f98a0) on the image: two shorts, a long, two shorts.
fn fix_pict_chunk(m: &mut Mem, p: u32) {
    swap2(m, p);
    swap2(m, p.wrapping_add(2));
    swap4(m, p.wrapping_add(4));
    swap2(m, p.wrapping_add(8));
    swap2(m, p.wrapping_add(10));
}

fn swap_pict_chunks(m: &mut Mem, p: u32, n: u32) {
    let mut i = 0u32;
    while i < n {
        fix_pict_chunk(m, p.wrapping_add(i.wrapping_mul(12)));
        i += 1;
    }
}

/// [soft 0x083fab14 SI_PictChunk_fread]: `fread`, then every chunk read (12 bytes) turned.
pub fn pict_chunk_fread(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    let got = o.fread(m, buf, size, n, stream);
    swap_pict_chunks(m, buf, got);
    got
}

/// [soft 0x083fab74 SI_PictChunk_fwrite]: `n` chunks of 12 bytes turned, written, turned
/// back.
pub fn pict_chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_pict_chunks, buf, size, n, stream)
}

/// `si_fix_LineChunk` (0x083f98f0) on the image: a long, then the short at 6.
fn fix_line_chunk(m: &mut Mem, p: u32) {
    swap4(m, p);
    swap2(m, p.wrapping_add(6));
}

fn swap_line_chunks(m: &mut Mem, p: u32, n: u32) {
    let mut i = 0u32;
    while i < n {
        fix_line_chunk(m, p.wrapping_add(i.wrapping_mul(8)));
        i += 1;
    }
}

/// [soft 0x083fac34 SI_LineChunk_fwrite]: `n` line chunks of 8 bytes turned, written,
/// turned back.
pub fn line_chunk_fwrite(m: &mut Mem, o: &mut dyn Outside, buf: u32, size: u32, n: u32, stream: u32) -> u32 {
    chunk_fwrite(m, o, swap_line_chunks, buf, size, n, stream)
}

// ---------------------------------------------------------------- utilio.c

/// What `fld` then `fstp` of a single does to its bit pattern: a signalling NaN becomes
/// the quiet NaN of the same payload, anything else is unchanged.
pub fn quiet(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// One `SI_byteswap_fwrite(&si_fvalue, 4, 1, stream)` (0x083f9c40).
fn write_fvalue(m: &mut Mem, o: &mut dyn Outside, stream: u32) {
    swap4(m, g::SI_FVALUE.0);
    o.fwrite(m, g::SI_FVALUE.0, 4, 1, stream);
    swap4(m, g::SI_FVALUE.0);
}

/// One `SI_byteswap_fread(&si_fvalue, 4, 1, stream)` (0x083f9ba0).
fn read_fvalue(m: &mut Mem, o: &mut dyn Outside, stream: u32) {
    let got = o.fread(m, g::SI_FVALUE.0, 4, 1, stream);
    if got != 0 {
        swap4(m, g::SI_FVALUE.0);
    }
}

/// [soft 0x083fd3cc hvectorBinaryWrite]: the four floats of a homogeneous vector written
/// one by one, each through `si_fvalue` (as a float: a signalling NaN is quieted) and
/// turned to the file's order. Answers 1; 0, writing nothing, for a null vector.
pub fn hvector_binary_write(m: &mut Mem, o: &mut dyn Outside, stream: u32, vector: u32) -> i32 {
    if vector == 0 {
        return 0;
    }
    for k in 0..4u32 {
        let v = quiet(m.u32(vector.wrapping_add(4 * k)));
        m.set_u32(g::SI_FVALUE.0, v);
        write_fvalue(m, o, stream);
    }
    1
}

/// [soft 0x083fd454 si_hvectorBinaryRead]: four floats read one by one through
/// `si_fvalue`, and stored at `out` when it is not null (the reads happen either way).
/// Answers 1; 0 for a null `out`. A float that could not be read is whatever `si_fvalue`
/// held before.
pub fn hvector_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, out: u32) -> i32 {
    let mut v = [0u32; 4];
    for x in v.iter_mut() {
        read_fvalue(m, o, stream);
        *x = quiet(m.u32(g::SI_FVALUE.0));
    }
    if out == 0 {
        return 0;
    }
    for (k, x) in v.iter().enumerate() {
        m.set_u32(out.wrapping_add(4 * k as u32), quiet(*x));
    }
    1
}

// ---------------------------------------------------------------- in_out.c, headers.c, sys_tmpdir.c

/// [soft 0x083fb990 SI_agetString]: skips to the first `"` of the stream (or its end), then
/// takes the characters up to the next `"` (or the end: the end's `-1` is stored as the byte
/// 0xff first) as a new string, which is stored at `out`. Answers 1.
pub fn aget_string(m: &mut Mem, o: &mut dyn Outside, stream: u32, out: u32) -> i32 {
    let mut c;
    loop {
        c = o.fgetc(m, stream);
        if c == 0x22 || c == -1 {
            break;
        }
    }
    let mut text: Vec<u8> = Vec::new();
    while c != -1 {
        c = o.fgetc(m, stream);
        if c == 0x22 {
            break;
        }
        text.push(c as u8);
    }
    let buf = m.local(text.len() as u32 + 1);
    for (i, b) in text.iter().enumerate() {
        m.set_u8(buf + i as u32, *b);
    }
    m.set_u8(buf + text.len() as u32, 0);
    let s = m.strdup(buf);
    m.release(text.len() as u32 + 1);
    m.set_u32(out, s);
    1
}

/// [soft 0x083fb2e0 SI_sceneversioncheckInstall]: remembers the function the scene reader
/// asks about a scene's version.
pub fn sceneversioncheck_install(m: &mut Mem, function: u32) {
    m.set_u32(g::SCENE_VERSION_CHECK.0, function);
}

/// [soft 0x0841d480 SI_temporaryDirectory]: the directory for temporary files, found on
/// the first call and kept: `/usr/tmp` when `access(…, 6)` allows reading and writing it,
/// with a `/` added unless it ends in one or in a backslash; otherwise (or if the
/// allocation fails) a copy of the empty text behind it.
pub fn temporary_directory(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    if m.u32(g::TEMPORARY_DIRECTORY.0) != 0 {
        return m.u32(g::TEMPORARY_DIRECTORY.0);
    }
    let path = TEXTS[0].0;
    if o.access(m, CStr32::new(path), 6) == 0 {
        let len = m.strlen(path) as i32;
        if len > 0 {
            let last = m.u8(path + len as u32 - 1);
            let extra: u32 = if last == b'/' { 0 } else { (last != b'\\') as u32 };
            let dir = m.calloc(extra + len as u32 + 1, 1);
            m.set_u32(g::TEMPORARY_DIRECTORY.0, dir);
            if dir != 0 {
                for i in 0..=len as u32 {
                    let b = m.u8(path + i);
                    m.set_u8(dir + i, b);
                }
                if extra != 0 {
                    m.set_u8(dir + len as u32, b'/');
                    m.set_u8(dir + len as u32 + 1, 0);
                }
            }
        }
    }
    if m.u32(g::TEMPORARY_DIRECTORY.0) == 0 {
        let d = m.strdup(TEXTS[1].0);
        m.set_u32(g::TEMPORARY_DIRECTORY.0, d);
    }
    m.u32(g::TEMPORARY_DIRECTORY.0)
}
