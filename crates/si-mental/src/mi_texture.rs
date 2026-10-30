// SPDX-License-Identifier: MIT
//! `mi_texture.c`: the translator's texture lists and matrices for mental ray (package 64, finding 0036).

use crate::mi_patch::{dw, fprintf};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `n3dtex`, `ntex` and `texlist` statics of the file: the 3D texture counter, the number of entries
/// and the array of 0x18-byte entries (+0 and +4 are names).
pub const N3DTEX: u32 = 0x086f_2ccc;
pub const NTEX: u32 = 0x086f_2cc8;
pub const TEXLIST: u32 = 0x086f_2cc0;
/// `init_matrix (0x085983ec)` and `rotate_matrix (0x085986f4)`.
pub const INIT_MATRIX: Routine = Routine::new("init_matrix", 0x0859_83ec);
pub const ROTATE_MATRIX: Routine = Routine::new("rotate_matrix", 0x0859_86f4);

/// Resets the 3D texture counter [soft 0x08591230 SI_initTxtSeqCounter].
pub fn si_init_txt_seq_counter(m: &mut Mem) {
    m.set_u32(N3DTEX, 0);
}

/// Whether a 2D texture needs a texture block: any of the words at +0x112, +0x114, +0x124, +0x118
/// or +0x120 is non-zero [soft 0x08591244 need_texture].
pub fn need_texture(m: &Mem, tex: u32) -> u32 {
    (m.u16(tex + 0x112) != 0 || m.u32(tex + 0x114) != 0 || m.u16(tex + 0x124) != 0 || m.u32(tex + 0x118) != 0 || m.u16(tex + 0x120) != 0) as u32
}

/// Whether a 3D texture needs a block: +0xf4 (32 bits), +0xf8, +0xfc, +0xfa or +0x102 non-zero
/// [soft 0x08591288 need_3D_texture].
pub fn need_3d_texture(m: &Mem, tex: u32) -> u32 {
    (m.u32(tex + 0xf4) != 0 || m.u16(tex + 0xf8) != 0 || m.u16(tex + 0xfc) != 0 || m.u16(tex + 0xfa) != 0 || m.u16(tex + 0x102) != 0) as u32
}

/// The first unused entry (name 0) of the texture list, or -1 [soft 0x08592160 find_empty_entry].
pub fn find_empty_entry(m: &Mem) -> i32 {
    let (n, list) = (m.i32(NTEX), m.u32(TEXLIST));
    (0..n).find(|&i| m.u32(list.wrapping_add(i as u32 * 0x18)) == 0).unwrap_or(-1)
}

fn lookup(m: &Mem, name: u32, off: u32) -> i32 {
    let (n, list) = (m.i32(NTEX), m.u32(TEXLIST));
    let want = m.cstr(name);
    for i in 0..n {
        let s = m.u32(list.wrapping_add(i as u32 * 0x18).wrapping_add(off));
        if s != 0 && m.cstr(s) == want {
            return i;
        }
    }
    -1
}

/// The index of the entry whose first name equals `name`, or -1 [soft 0x085920a4 lookup_2D_texture_list].
pub fn lookup_2d_texture_list(m: &Mem, name: u32) -> i32 {
    lookup(m, name, 0)
}

/// As [`lookup_2d_texture_list`] on the second name (+4) [soft 0x08592100 mi2_lookup_2D_texture_list].
pub fn mi2_lookup_2d_texture_list(m: &Mem, name: u32) -> i32 {
    lookup(m, name, 4)
}

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f32(f32::from_bits(m.u32(a)))
}

fn put(m: &mut Mem, a: u32, v: F80) {
    m.set_u32(a, v.to_f32().to_bits());
}

/// The 2D texture's transform in a matrix: the scales (+0x58, +0x5c, negated when +0x78 / +0x7a, and
/// at least 1e-6) divide the first two rows, the translation (+0x70, +0x74) goes to row 3
/// [soft 0x085936dc calc_2D_matrix].
pub fn calc_2d_matrix(m: &mut Mem, _o: &mut dyn Outside, mat: u32, tex: u32) {
    crate::mi_calc::init_matrix(m, mat);
    let k1 = F80::from_f64(f64::from_bits(0x3eb0_c6f7_a0b5_ed8d));
    let k2 = F80::from_f32(f32::from_bits(0x3586_37bd));
    let sc = |m: &Mem, neg: u32, at: u32| {
        let v = if m.u16(tex + neg) != 0 { -f(m, tex + at) } else { f(m, tex + at) };
        if v.lt(k1) { k2 } else { v }
    };
    let a = sc(m, 0x78, 0x58);
    let b = sc(m, 0x7a, 0x5c);
    let t0 = (-f(m, tex + 0x70)) / a;
    put(m, mat + 0x30, -f(m, tex + 0x70));
    let m0 = f(m, mat) / a;
    put(m, mat, m0);
    let t1 = (-f(m, tex + 0x74)) / b;
    let m1 = f(m, mat + 4) / a;
    let m4 = f(m, mat + 0x10) / b;
    let m5 = f(m, mat + 0x14) / b;
    put(m, mat + 4, m1);
    put(m, mat + 0x10, m4);
    put(m, mat + 0x30, t0);
    put(m, mat + 0x34, t1);
    put(m, mat + 0x14, m5);
}

/// The 3D texture's transform: the rotation (matrix call on +0x9c), the translation (+0xa8) to row 3
/// and the scales (+0x90, +0x94, +0x98) on the three rows [soft 0x08593798 calc_3D_matrix].
pub fn calc_3d_matrix(m: &mut Mem, o: &mut dyn Outside, mat: u32, tex: u32) {
    o.call(m, ROTATE_MATRIX, &[mat, tex + 0x9c]);
    for k in 0..3 {
        let v = f(m, tex + 0xa8 + 4 * k);
        put(m, mat + 0x30 + 4 * k, v);
    }
    for (row, s) in [(0u32, 0x90u32), (0x10, 0x94), (0x20, 0x98)] {
        for k in 0..3 {
            let v = f(m, mat + row + 4 * k) * f(m, tex + s);
            put(m, mat + row + 4 * k, v);
        }
    }
}

/// A matrix as the `transform` block of a mental ray texture [soft 0x08593844 print_matrix].
pub fn print_matrix(m: &mut Mem, o: &mut dyn Outside, mat: u32) {
    fprintf(m, o, &[0x086f_356c]);
    for (fmt, at) in [(0x086f_357au32, 0u32), (0x086f_3589, 0x10), (0x086f_359a, 0x20), (0x086f_35ab, 0x30)] {
        let (x, y, z) = (dw(f(m, mat + at).to_f64()), dw(f(m, mat + at + 4).to_f64()), dw(f(m, mat + at + 8).to_f64()));
        fprintf(m, o, &[fmt, x[0], x[1], y[0], y[1], z[0], z[1]]);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// round 7: the texture-name and picture-file helpers, the list maintenance, find_program, print_texture_names

use crate::cprintf;
use crate::mi_mr_main::ext;
use crate::mi_model::SI_EMPTY_STRING;
use crate::mi_patch::{MEMTRACK_ALLOCATE, MEMTRACK_FREE, MEMTRACK_REALLOC, SFATAL, SI_MEM_PARAMS};
use si_x87::Round;

/// `SI_malloc (0x080d0ee8)`, `SI_byteswap_fread (0x083f9ba0)` and `SI_memtrackStrdup (0x085997e0)`.
pub const SI_MALLOC: Routine = Routine::new("SI_malloc", 0x080d_0ee8);
pub const SI_BYTESWAP_FREAD: Routine = Routine::new("SI_byteswap_fread", 0x083f_9ba0);
pub const SI_MEMTRACK_STRDUP: Routine = Routine::new("SI_memtrackStrdup", 0x0859_97e0);
/// `SI_resdir`, `MI_Local` (the `SI_MI_LCL_MAPS` environment value) and `mi2output` (16 bits).
pub const SI_RESDIR: u32 = 0x0878_1628;
pub const MI_LOCAL: u32 = 0x0882_9f88;
pub const MI2OUTPUT: u32 = 0x086e_f9c0;
/// `texsize`, the fourth static of the list.
pub const TEXSIZE: u32 = 0x086f_2cc4;

fn zero_f() -> F80 {
    F80::from_f32(0.0)
}

/// The texture's repeat bookkeeping for one axis: [`preprocess_texture`] does it twice.
///
/// `neg` is the flag at +0x78 / +0x7a (the scale is negative), `at` the scale, `off` the offset,
/// `flat` whether the axis is wrapped (the pointer argument is 1); the three stores are the
/// 32-bit results at `sum` (+0x38 / +0x3c), `frac` (+0x48 / +0x4c) and `res` (+0x70 / +0x74).
#[allow(clippy::too_many_arguments)]
fn pre_axis(m: &mut Mem, tex: u32, at: u32, off: u32, flag: u32, sum: u32, frac: u32, res: u32, wrap: bool) {
    let zero = zero_f();
    let s = f(m, tex + at);
    let neg = s.lt(zero);
    m.set_u16(tex + flag, neg as u16);
    let mut half = zero;
    if neg {
        half = -s;
        if half.lt(F80::from_f64(f64::from_bits(0x3eb0_c6f7_a0b5_ed8d))) {
            half = F80::from_f32(f32::from_bits(0x3586_37bd));
        }
        let v = f(m, tex + off) + s;
        put(m, tex + sum, v);
    } else {
        let v = f(m, tex + off);
        put(m, tex + sum, v);
    }
    if wrap {
        let t = f(m, tex + sum);
        let k = t.to_i32(Round::Zero);
        let fr = t - F80::from_i32(k);
        put(m, tex + frac, fr);
        if fr.lt(zero) {
            put(m, tex + frac, fr + F80::from_f32(1.0));
        }
        let v = f(m, tex + frac);
        put(m, tex + res, v);
    } else if neg {
        let h = F80::from_f64(0.5);
        let t = F80::from_f32((f(m, tex + at) * h + f(m, tex + off)).to_f32());
        put(m, tex + res, t - h * half);
    } else {
        let v = f(m, tex + off);
        put(m, tex + res, v);
    }
}

/// A 2D texture's repeat state: the two wrap flags (`*pu`, `*pv`, set when the texture kind (+0x2a)
/// is 4..6 and the argument asks), the scale signs (+0x78, +0x7a), the shifted offsets (+0x38,
/// +0x3c), the fractional parts (+0x48, +0x4c) and the resulting offsets (+0x70, +0x74)
/// [soft 0x08594050 preprocess_texture].
pub fn preprocess_texture(m: &mut Mem, tex: u32, wu: u32, wv: u32, pu: u32, pv: u32) {
    let kind_ok = m.u16(tex + 0x2a).wrapping_sub(4) <= 2;
    m.set_u16(pu, (wu as u16 != 0 && kind_ok) as u16);
    m.set_u16(pv, (wv as u16 != 0 && kind_ok) as u16);
    let wrap_u = m.u16(pu) != 0;
    pre_axis(m, tex, 0x58, 0x68, 0x78, 0x38, 0x48, 0x70, wrap_u);
    let wrap_v = m.u16(pv) != 0;
    pre_axis(m, tex, 0x5c, 0x6c, 0x7a, 0x3c, 0x4c, 0x74, wrap_v);
}

/// A picture's size from its header: tries `name`, `name.pic`, `name.1.pic` and `<resdir>/noIcon.pic`
/// for a readable file, reads the 0x5c-byte header and the two 16-bit sizes behind it, answers 1
/// with the width and height in `*pw`, `*ph` and the file name in `out`; 0 when nothing can be
/// read and `fatal` is 0 (with `fatal` the failures call `sfatal`)
/// [soft 0x0859390c precompute_mi_texture].
pub fn precompute_mi_texture(m: &mut Mem, o: &mut dyn Outside, name: u32, out: u32, pw: u32, ph: u32, fatal: u32) -> u32 {
    // the original's locals: the header (0x5c), the two sizes and the 1024-byte name, one block
    let base = m.local(0x460);
    let (hdr, wp, hp, buf) = (base, base + 0x5c, base + 0x5e, base + 0x60);
    let fatal = fatal as u16;
    let mode = m.local(4);
    m.set_bytes(mode, b"rb\0");
    crate::mi_texture::strncpy_n(m, buf, name, 0x3fb);
    let acc = |m: &mut Mem, o: &mut dyn Outside, p: u32| o.call(m, ext::ACCESS, &[p, 4]).eax;
    let mut skip_fatal = false;
    if acc(m, o, buf) != 0 {
        crate::mi_texture::strcat_bytes(m, buf, b".pic");
        if acc(m, o, buf) != 0 {
            crate::mi_texture::strncpy_n(m, buf, name, 0x3f9);
            crate::mi_texture::strcat_bytes(m, buf, b".1.pic");
            if acc(m, o, buf) != 0 {
                let dir = m.u32(SI_RESDIR);
                let mut t = if dir == 0 { b"(null)".to_vec() } else { m.cstr(dir) };
                t.extend_from_slice(b"/noIcon.pic");
                m.put_cstr(buf, &t);
                if acc(m, o, buf) != 0 {
                    if fatal == 0 {
                        m.release(4);
                        m.release(0x460);
                        return 0;
                    }
                    o.call(m, SFATAL, &[0x086f_35e0, name, name]);
                    skip_fatal = true;
                }
            }
        }
    }
    let _ = skip_fatal;
    let f = o.call(m, ext::FOPEN, &[buf, mode]).eax;
    let ok = f != 0
        && o.call(m, SI_BYTESWAP_FREAD, &[hdr, 0x5c, 1, f]).eax == 1
        && o.call(m, SI_BYTESWAP_FREAD, &[wp, 2, 1, f]).eax == 1
        && o.call(m, SI_BYTESWAP_FREAD, &[hp, 2, 1, f]).eax == 1;
    if !ok {
        if fatal == 0 {
            m.release(4);
            m.release(0x460);
            return 0;
        }
        o.call(m, SFATAL, &[0x086f_360f, buf, 1]); // the third word is the count 1 of the fread calls, still on the stack
    }
    o.call(m, ext::FCLOSE, &[f]);
    m.set_u32(pw, m.i16(wp) as i32 as u32);
    m.set_u32(ph, m.i16(hp) as i32 as u32);
    let t = m.cstr(buf);
    m.put_cstr(out, &t);
    m.release(4);
    m.release(0x460);
    1
}

/// `strncpy(dst, src, n)` (zero padded) on the image.
pub(crate) fn strncpy_n(m: &mut Mem, dst: u32, src: u32, n: u32) {
    si_mem::clib::strncpy(m, dst, src, n);
}

/// `strcat(dst, literal)`.
pub(crate) fn strcat_bytes(m: &mut Mem, dst: u32, lit: &[u8]) {
    let at = dst.wrapping_add(m.strlen(dst));
    m.put_cstr(at, lit);
}

fn strdup_buf(m: &mut Mem, s: u32) -> u32 {
    let t = m.cstr(s);
    let p = m.malloc(t.len() as u32 + 1);
    m.put_cstr(p, &t);
    p
}

/// What `fprintf(stderr, "Unexpected default in file '%s' at line %d:\n   %s\n", ...)` of `mi_texture.c` writes.
fn unexpected_default(o: &mut dyn Outside, line: u32) {
    // the format (0x0867d040) with its three conversions filled in: the file, the line, the text; all read
    // from the user's install (si-rt)
    let (fmt, file, text) = (si_rt::message(0x0867_d040), si_rt::message(0x086f_2f60), si_rt::message(0x086f_3684));
    let line = line.to_string();
    let args: [&[u8]; 3] = [file, line.as_bytes(), text];
    let (mut t, mut k, mut i) = (Vec::new(), 0, 0);
    while i < fmt.len() {
        if fmt[i] == b'%' {
            t.extend_from_slice(args[k]);
            k += 1;
            i += 2;
        } else {
            t.push(fmt[i]);
            i += 1;
        }
    }
    o.print(2, &t);
}

/// The warning shown when an animated texture has no picture for the frame: the text names the
/// frame and the script, the picture becomes `<resdir>/noIcon` [soft 0x08593f34 buildDefaultPicName].
pub fn build_default_pic_name(m: &mut Mem, o: &mut dyn Outside, tex: u32, frame: u32, mode: u32) {
    let dir = m.u32(SI_RESDIR);
    let n = m.strlen(dir);
    let p = si_malloc(m, n + 8);
    let mut t = m.cstr(dir);
    t.extend_from_slice(b"/noIcon");
    m.put_cstr(p, &t);
    m.set_u32(tex + 0x20, p);
    let script = m.u32(tex + 0x14);
    let n = m.strlen(script);
    let q = si_malloc(m, n + 0x41);
    let t = cprintf::format_bytes(m, si_rt::message(0x086a_4600), &[frame, script]);
    m.put_cstr(q, &t);
    o.call(m, ext::DISPLAY_MESSAGES, &[mode as u16 as i16 as i32 as u32, q]);
    if q != 0 {
        m.free(q);
    }
}

/// [soft 0x080d0ee8 SI_malloc]: nothing for 0, else a block with `o` first and `k` last (the rest untouched).
fn si_malloc(m: &mut Mem, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let p = m.malloc(n);
    m.set_u8(p, 0x6f);
    m.set_u8(p + n - 1, 0x6b);
    p
}

/// The picture file name of a texture at a frame, into +0x20: the name itself (kind 0), the name
/// with its extension replaced by the frame number (kind 1: `name.N`), or line `frame` of the script
/// file (kind 2) [soft 0x08593cd0 setFileName].
pub fn set_file_name(m: &mut Mem, o: &mut dyn Outside, tex: u32, frame: u32, mode: u32) {
    let l = m.local(0x20c);
    let buf = l + 0x18;
    let old = m.u32(tex + 0x20);
    if old != 0 {
        m.free(old);
        m.set_u32(tex + 0x20, 0);
    }
    let name = m.u32(tex + 0x14);
    let src;
    match m.i16(tex + 0x28) {
        0 => src = Some(name),
        1 => {
            let s = m.cstr(name);
            let mut t = match s.iter().rposition(|&c| c == b'.') {
                Some(i) => s[..i].to_vec(),
                None => s.clone(),
            };
            t.extend_from_slice(format!(".{}", frame as i32).as_bytes());
            m.put_cstr(buf, &t);
            src = Some(buf);
        }
        2 => {
            let mode_r = m.local(4);
            m.set_bytes(mode_r, b"r\0");
            let f = o.call(m, ext::FOPEN, &[name, mode_r]).eax;
            let mut have = f != 0;
            if have {
                let mut i = 0i32;
                while i < frame as i32 {
                    if o.call(m, ext::FGETS, &[buf, 500, f]).eax == 0 {
                        o.call(m, ext::FCLOSE, &[f]);
                        have = false;
                        break;
                    }
                    i += 1;
                }
            }
            m.release(4);
            if !have {
                build_default_pic_name(m, o, tex, frame, mode);
                m.release(0x20c);
                return;
            }
            o.call(m, ext::FCLOSE, &[f]);
            let n = m.strlen(buf);
            m.set_u8(buf.wrapping_add(n).wrapping_sub(1), 0);
            src = Some(buf);
        }
        _ => {
            unexpected_default(o, 0x86b);
            src = None;
        }
    }
    if let Some(s) = src {
        let p = strdup_buf(m, s);
        m.set_u32(tex + 0x20, p);
    }
    m.release(0x20c);
}

/// The name a texture is written under: the file name's stem for a numbered sequence (kind 1), the
/// name itself for the others; kind 0 uses the picture file name (+0x20), kind 2 the script name
/// (+0x14); any other kind says so on stderr and answers 0. The copy is made with
/// `SI_memtrackStrdup` [soft 0x08591ea4 generateTextureName].
pub fn generate_texture_name(m: &mut Mem, o: &mut dyn Outside, tex: u32) -> u32 {
    let buf = m.local(0x410);
    let name = m.u32(tex + 0x14);
    let src = match m.i16(tex + 0x28) {
        0 => m.u32(tex + 0x20),
        1 => {
            let s = m.cstr(name);
            match s.iter().rposition(|&c| c == b'.') {
                Some(i) => {
                    m.put_cstr(buf, &s[..i]);
                    buf
                }
                None => name,
            }
        }
        2 => name,
        _ => {
            unexpected_default(o, 0x458);
            m.release(0x410);
            return 0;
        }
    };
    let r = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, src]).eax;
    m.release(0x410);
    r
}

/// Frees both names of every entry of the texture list, then the list itself
/// [soft 0x08591ff0 destroy_2D_texture_list].
pub fn destroy_2d_texture_list(m: &mut Mem, o: &mut dyn Outside) {
    let mut i = 0i32;
    while i < m.i32(NTEX) {
        let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
        for off in [0u32, 4] {
            let p = m.u32(e + off);
            if p != 0 {
                o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, p]);
                let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
                m.set_u32(e + off, 0);
            }
        }
        i += 1;
    }
    let list = m.u32(TEXLIST);
    m.set_u32(NTEX, 0);
    o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, list]);
    m.set_u32(TEXLIST, 0);
    m.set_u32(TEXSIZE, 0);
}

/// Frees the names of the entries whose use flag (+0x14, 16 bits) is clear
/// [soft 0x0859219c cleanup_2D_texture_list].
pub fn cleanup_2d_texture_list(m: &mut Mem, o: &mut dyn Outside) {
    let mut i = 0i32;
    while i < m.i32(NTEX) {
        let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
        if m.u16(e + 0x14) == 0 {
            for off in [0u32, 4] {
                let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
                let p = m.u32(e + off);
                if p != 0 {
                    o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, p]);
                    let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
                    m.set_u32(e + off, 0);
                }
            }
        }
        i += 1;
    }
}

fn stderr_fmt(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], words: &[u32]) {
    let t = cprintf::format_bytes(m, fmt, words);
    o.print(2, &t);
}

/// The texture list on stderr (debugging aid): the count, then per entry its number, `s-` / `d-`
/// for the use flag, the second name when `mi2output` is on and the first name
/// [soft 0x08592228 dump_2D_texture_list].
pub fn dump_2d_texture_list(m: &mut Mem, o: &mut dyn Outside) {
    let n = m.u32(NTEX);
    stderr_fmt(m, o, si_rt::message(0x086f_3013), &[n]);
    let mut i = 0i32;
    while i < m.i32(NTEX) {
        stderr_fmt(m, o, b"tex #%d: ", &[i as u32]);
        let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
        if m.u16(e + 0x14) != 0 {
            stderr_fmt(m, o, b"s- ", &[]);
        } else {
            stderr_fmt(m, o, b"d- ", &[]);
        }
        if m.u16(MI2OUTPUT) != 0 {
            let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
            let p = m.u32(e + 4);
            if p != 0 {
                stderr_fmt(m, o, b"'%s' ", &[p]);
            }
        }
        let e = m.u32(TEXLIST).wrapping_add(i as u32 * 0x18);
        let p = m.u32(e);
        if p != 0 {
            stderr_fmt(m, o, b"'%s'\n", &[p]);
        } else {
            stderr_fmt(m, o, b"\n", &[]);
        }
        i += 1;
    }
    stderr_fmt(m, o, b"\n", &[]);
}

/// Finds an executable by name on `PATH` (default `/usr/bin:/usr/local/bin:/usr/local/mi/bin`)
/// and writes the full path to `out`; a name with a blank is searched without the part from the
/// blank on, which is appended again; `sfatal` when nothing is found
/// [soft 0x08593ab4 find_program].
pub fn find_program(m: &mut Mem, o: &mut dyn Outside, out: u32, name: u32) {
    if name == 0 {
        return;
    }
    let mut found = false;
    let space = si_mem::clib::strchr(m, name, b' ');
    let mut dest = name;
    if space != 0 {
        let n = m.strlen(name) - m.strlen(space);
        dest = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, n + 1]).eax;
        si_mem::clib::strncpy(m, dest, name, n);
        m.set_u8(dest + n, 0);
    }
    let env = o.call(m, ext::GETENV, &[0x086f_3627]).eax;
    let path: Vec<u8> = if env == 0 { b"/usr/bin:/usr/local/bin:/usr/local/mi/bin".to_vec() } else { m.cstr(env) };
    let rest = if space != 0 { m.cstr(space) } else { Vec::new() };
    if m.u8(dest) == b'/' {
        let d = m.cstr(dest);
        m.put_cstr(out, &d);
        if o.call(m, ext::ACCESS, &[out, 1]).eax == 0 {
            found = true;
            if space != 0 {
                strcat_bytes(m, out, &rest);
            }
        }
    } else {
        let mut at = 0usize;
        let b = |at: usize| path.get(at).copied().unwrap_or(0);
        while b(at) != 0 && !found {
            let mut w = out;
            while b(at) != 0 && b(at) != b':' {
                m.set_u8(w, b(at));
                at += 1;
                w += 1;
            }
            while b(at) == b':' {
                at += 1;
            }
            m.set_u8(w, b'/');
            let d = m.cstr(dest);
            m.put_cstr(w + 1, &d);
            if o.call(m, ext::ACCESS, &[out, 1]).eax == 0 {
                if space != 0 {
                    strcat_bytes(m, out, &rest);
                }
                found = true;
            }
        }
    }
    if !found {
        o.call(m, SFATAL, &[0x086f_366a, dest]);
    }
    if dest != name {
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, dest]);
    }
}

/// One texture of the debugging listing: its name, global name, file and the project of its owner.
fn texture_lines(m: &mut Mem, o: &mut dyn Outside, rec: u32, owner: u32) {
    let (a, b, c) = (m.u32(rec + 0x14), m.u32(rec + 0x18), m.u32(rec + 0x20));
    let proj = m.u32(m.u32(owner) + 0x10);
    stderr_fmt(m, o, si_rt::message(0x086f_379c), &[a]);
    stderr_fmt(m, o, si_rt::message(0x086f_37b5), &[b]);
    stderr_fmt(m, o, si_rt::message(0x086f_37cb), &[c]);
    stderr_fmt(m, o, si_rt::message(0x086f_37e3), &[proj]);
}

/// Lists the 2D textures of an element and of the materials hanging on it on stderr
/// [soft 0x0859425c print_texture_names].
pub fn print_texture_names(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let l = m.local(0x1c);
    let ebp = l + 0x1c;
    let (l4, l8, lc, l10) = (ebp - 4, ebp - 8, ebp - 0xc, ebp - 0x10);
    let tex = m.u32(0x0878_16c8);
    let mats = m.u32(0x0878_167c);
    let mut r = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tex, l8, l4]).eax;
    while m.u32(l8) != 0 {
        texture_lines(m, o, r, m.u32(l8));
        let tex = m.u32(0x0878_16c8);
        r = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tex, l8, l4]).eax;
    }
    o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, lc, l4]);
    if m.u32(lc) != 0 {
        loop {
            let tex = m.u32(0x0878_16c8);
            let mut r = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, m.u32(lc), 1, 3, 0x191, tex, l8, l10]).eax;
            while m.u32(l8) != 0 {
                texture_lines(m, o, r, m.u32(l8));
                let tex = m.u32(0x0878_16c8);
                r = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, m.u32(lc), 1, 3, 0x191, tex, l8, l10]).eax;
            }
            let mats = m.u32(0x0878_167c);
            o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, lc, l4]);
            if m.u32(lc) == 0 {
                break;
            }
        }
    }
    m.release(0x1c);
}

/// `MI_Local = getenv("SI_MI_LCL_MAPS")` at program start (priority 0xffff only)
/// [soft 0x08594474 __static_initialization_and_destruction_0].
pub fn static_init_destruction_0(m: &mut Mem, o: &mut dyn Outside, initialize: u32, priority: u32) {
    if priority == 0xffff && initialize != 0 {
        let v = o.call(m, ext::GETENV, &[0x086f_3864]).eax;
        m.set_u32(MI_LOCAL, v);
    }
}

/// The global constructor of the file [soft 0x085944a0 global.constructors.keyed.to.SI_init_MRSoftColor].
pub fn global_constructors_keyed_to_si_init_mrsoftcolor(m: &mut Mem, o: &mut dyn Outside) {
    static_init_destruction_0(m, o, 1, 0xffff);
}

// ---- round 7, second part: the 3D colour description

/// Fills the 3D texture description `out` from the texture record of an element: the mapping kind
/// (+4: 1, 2 or 0 from the record's kind at +0x24), the repeat flag (+8), the constant 0x29a (+0xc),
/// the bump factor (+0x10: the record's float at +0x120 times -2 when it is outside +-0.0001 and the
/// flag at +0x150 is clear, else 0), two scaled sizes (+0x14, +0x18: 0.05 times the floats at +0x90 and
/// +0x94) and the matrix (+0x1c) [soft 0x08591144 SI_init_MRSoftColor3D].
pub fn si_init_mr_soft_color_3d(m: &mut Mem, o: &mut dyn Outside, first: u32, elem: u32, out: u32) {
    if first == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 {
        return;
    }
    let kind = match m.u16(rec + 0x24) {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    m.set_u16(out + 4, kind);
    m.set_u32(out + 8, if m.u16(rec + 0x2c) == 2 { 2 } else { 1 });
    m.set_u32(out + 0xc, 0x29a);
    let v = f(m, rec + 0x120);
    let outside = v.gt(F80::from_f64(0.0001)) || v.lt(F80::from_f64(-0.0001));
    if outside && m.u16(rec + 0x150) == 0 {
        put(m, out + 0x10, f(m, rec + 0x120) * F80::from_f64(-2.0));
    } else {
        m.set_u32(out + 0x10, 0);
    }
    let k = F80::from_f64(0.05);
    put(m, out + 0x14, k * f(m, rec + 0x90));
    put(m, out + 0x18, k * f(m, rec + 0x94));
    calc_3d_matrix(m, o, out + 0x1c, rec);
}

// ---- round 7, third part: collecting the 2D textures of a scene into the list

const FRAME_CURRENT: u32 = 0x0878_2040;
const ADD_2D_TEXTURE: Routine = Routine::new("add_2D_texture", 0x0859_1ca0);
const PRINT_MODEL_2D_DEFS: Routine = Routine::new("print_model_2D_defs", 0x0859_1b34);
const CID_MODELS: u32 = 0x0878_1680;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_MATERIALS: u32 = 0x0878_167c;

/// Enters the 2D texture of an element in the texture list (`reuse` takes a freed slot first): the
/// picture name is made for the current frame, a texture already in the list is marked used (or, for
/// the mental ray 2 output, keeps its first name under a new picture name when it is animated), a new
/// one gets a list slot (the list grows by 100 entries of 0x18 bytes) [soft 0x08591ca0 add_2D_texture].
pub fn add_2d_texture(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, reuse: u32, mode: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 {
        return;
    }
    let mi2 = m.u16(MI2OUTPUT) != 0;
    if !mi2 && need_texture(m, rec) == 0 {
        return;
    }
    let frame = m.u32(FRAME_CURRENT);
    set_file_name(m, o, rec, frame, mode as u16 as i16 as i32 as u32);
    let mut name2 = 0u32;
    let mut at;
    if mi2 {
        name2 = generate_texture_name(m, o, rec);
        at = mi2_lookup_2d_texture_list(m, name2);
    } else {
        at = lookup_2d_texture_list(m, m.u32(rec + 0x20));
    }
    if at != -1 {
        if mi2 {
            if m.u16(rec + 0x28) != 0 {
                let e = m.u32(TEXLIST).wrapping_add(at as u32 * 0x18);
                let old = m.u32(e);
                o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, old]);
                let n = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, m.u32(rec + 0x20)]).eax;
                let e = m.u32(TEXLIST).wrapping_add(at as u32 * 0x18);
                m.set_u32(e, n);
            }
            o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, name2]);
        } else if m.u16(rec + 0x28) == 0 {
            let e = m.u32(TEXLIST).wrapping_add(at as u32 * 0x18);
            m.set_u16(e + 0x14, 1);
        }
        return;
    }
    if reuse != 0 {
        at = find_empty_entry(m);
    }
    if at == -1 {
        if m.i32(NTEX) >= m.i32(TEXSIZE) {
            let size = m.u32(TEXSIZE).wrapping_add(100);
            m.set_u32(TEXSIZE, size);
            let list = m.u32(TEXLIST);
            let bytes = size.wrapping_mul(0x18);
            let n = if list != 0 {
                o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, list, bytes]).eax
            } else {
                o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, bytes]).eax
            };
            m.set_u32(TEXLIST, n);
            if n == 0 {
                o.call(m, SFATAL, &[0x086f_2f0a]);
            }
        }
        at = m.i32(NTEX);
        m.set_u32(NTEX, (at as u32).wrapping_add(1));
    }
    let n = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, m.u32(rec + 0x20)]).eax;
    let e = m.u32(TEXLIST).wrapping_add(at as u32 * 0x18);
    m.set_u32(e, n);
    m.set_u32(e + 0xc, 0);
    m.set_u32(e + 0x10, 0);
    let unanimated = (m.u16(rec + 0x28) == 0) as u16;
    m.set_u16(e + 0x14, unanimated);
    m.set_u32(e + 4, name2);
    m.set_u16(e + 8, 0);
}

/// Enters the textures of an element and of the materials hanging on it
/// [soft 0x08591b34 print_model_2D_defs].
pub fn print_model_2d_defs(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, reuse: u32, mode: u32) {
    let l = m.local(0x2c);
    let ebp = l + 0x2c;
    let (l4, l8, lc, l10) = (ebp - 4, ebp - 8, ebp - 0xc, ebp - 0x10);
    if dict != 0 && elem != 0 {
        let tex = m.u32(CID_TEXTURES2D);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tex, l8, l4]);
        while m.u32(l8) != 0 {
            let t = m.u32(l8);
            if m.u8(t + 0x5b) == 1 {
                o.call(m, ADD_2D_TEXTURE, &[dict, t, reuse, mode as u16 as i16 as i32 as u32]);
            }
            let tex = m.u32(CID_TEXTURES2D);
            o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tex, l8, l4]);
        }
        let mats = m.u32(CID_MATERIALS);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, lc, l4]);
        while m.u32(lc) != 0 {
            let tex = m.u32(CID_TEXTURES2D);
            let mat = m.u32(lc);
            o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, mat, 1, 3, 0x191, tex, l8, l10]);
            while m.u32(l8) != 0 {
                let t = m.u32(l8);
                if m.u8(t + 0x5b) == 1 {
                    o.call(m, ADD_2D_TEXTURE, &[dict, t, reuse, mode as u16 as i16 as i32 as u32]);
                }
                let tex = m.u32(CID_TEXTURES2D);
                let mat = m.u32(lc);
                o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, mat, 1, 3, 0x191, tex, l8, l10]);
            }
            let mats = m.u32(CID_MATERIALS);
            o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, lc, l4]);
        }
    }
    m.release(0x2c);
}

/// Goes over the model elements of the scene and enters the textures of those that are not hidden and
/// have a block: all of them when `all` is 1, else the ones of kind 1, 2 or 3; without the mental ray 2
/// output and with `reuse` of 0 or 1 only those that have a picture name [soft 0x085912cc add2DMaps].
pub fn add_2d_maps(m: &mut Mem, o: &mut dyn Outside, dict: u32, all: u32, reuse: u32, mode: u32) {
    if dict == 0 {
        return;
    }
    let block = (m.u32(CID_MODELS) << 6).wrapping_add(m.u32(dict + 4));
    let mode = mode as u16 as i16 as i32 as u32;
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(block + 4) {
        let e = off.wrapping_add(m.u32(block + 8));
        if e != 0 && m.u16(e + 0x48) == 0 && m.u32(e + 4) != 0 {
            let k = m.u32(e + 0x24);
            if all as u16 == 1 || k == 1 || k == 3 || k == 2 {
                if reuse > 1 || m.u16(MI2OUTPUT) != 0 || m.u32(e + 0x20) != 0 {
                    o.call(m, PRINT_MODEL_2D_DEFS, &[dict, e, reuse, mode]);
                }
            }
        }
        off = off.wrapping_add(0x84);
        i += 1;
    }
}

// ---- round 7, fourth part: the mental ray 3D texture block

const SOFT_PREFIX: u32 = 0x086e_f9b8;
const INCREMENTAL: u32 = 0x086e_f9bc;
const G_8791168: u32 = 0x0879_1168;
const CID_TEXTURE3D_SHADERS: u32 = 0x0878_1698;

fn strdup_lit(m: &mut Mem, s: &[u8]) -> u32 {
    let p = m.malloc(s.len() as u32 + 1);
    m.put_cstr(p, s);
    p
}

/// Two words of a `%f` argument: the float at `a`, widened to a double.
fn fd(m: &Mem, a: u32) -> [u32; 2] {
    dw(f(m, a).to_f64())
}

/// The block of a 3D texture (`color texture` / `shader`) of an element in the scene file: the
/// header (a user shader's name or the program's own `<prefix>_color_3d`), the `si_default` block,
/// the numbered parameters, the transform and the closing parenthesis
/// [soft 0x08593260 print_3D_map].
pub fn print_3d_map(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, name: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 || m.u16(G_8791168) == 0 {
        return;
    }
    let mi2 = m.u16(MI2OUTPUT) != 0;
    if !mi2 && need_3d_texture(m, rec) == 0 {
        return;
    }
    let l = m.local(0x16c);
    let ebp = l + 0x16c;
    let (buf, mat) = (ebp - 0x140, ebp - 0x40);
    let incr = strdup_lit(m, if m.u32(INCREMENTAL) != 0 { b"incremental " } else { b"" });
    let kind = strdup_lit(m, if mi2 { b"shader" } else { b"color texture" });
    let cid = m.u32(CID_TEXTURE3D_SHADERS);
    let udf = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, 0, 0]).eax;
    let sname = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, buf, 0x100]).eax as u16;
    let enabled = |m: &mut Mem, o: &mut dyn Outside| o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0;
    let usedef = |m: &mut Mem, o: &mut dyn Outside| o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0;
    if sname != 0 && udf != 0 && enabled(m, o) {
        fprintf(m, o, &[0x086f_3416, incr, kind, name, buf]);
    } else {
        let pre = m.u32(SOFT_PREFIX);
        fprintf(m, o, &[0x086f_3424, incr, kind, name, pre]);
    }
    let skip_body = sname != 0 && udf != 0 && !usedef(m, o);
    if !skip_body {
        if enabled(m, o) && usedef(m, o) {
            fprintf(m, o, &[0x086f_343d]);
        }
        let seq = m.u32(N3DTEX);
        m.set_u32(N3DTEX, seq.wrapping_add(1));
        fprintf(m, o, &[0x086f_344e, seq]);
        let t = m.u32(rec + 0xc);
        fprintf(m, o, &[0x086f_345b, t]);
        for (fmt, at) in [(0x086f_3469u32, 0x10u32), (0x086f_3479, 0x14), (0x086f_3488, 0x18), (0x086f_3499, 0x20)] {
            let v = fd(m, rec + at);
            fprintf(m, o, &[fmt, v[0], v[1]]);
        }
        let it = m.u32(rec + 0x1c);
        fprintf(m, o, &[0x086f_34a8, it]);
        for (fmt, at) in [(0x086f_34bau32, 0x80u32), (0x086f_34ca, 0x84), (0x086f_34da, 0x88)] {
            let v = fd(m, rec + at);
            fprintf(m, o, &[fmt, v[0], v[1]]);
        }
        for (fmt, at) in [(0x086f_34eau32, 0x30u32), (0x086f_3502, 0x40), (0x086f_351a, 0x50), (0x086f_3532, 0x60), (0x086f_354a, 0x70)] {
            let mut a = vec![fmt];
            for k in 0..4 {
                a.extend(fd(m, rec + at + 4 * k));
            }
            fprintf(m, o, &a);
        }
        calc_3d_matrix(m, o, mat, rec);
        print_matrix(m, o, mat);
        if enabled(m, o) && usedef(m, o) {
            fprintf(m, o, &[0x086f_3561]);
        }
    }
    if sname != 0 && udf != 0 && enabled(m, o) {
        if usedef(m, o) {
            fprintf(m, o, &[0x086f_3564]);
        }
        let fc = m.u32(FRAME_CURRENT);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
    }
    fprintf(m, o, &[0x086f_3567]);
    m.free(incr);
    m.free(kind);
    m.release(0x16c);
}

/// The text a `%s` prints for a pointer: `(null)` for 0.
fn cstr_or_null(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

const PRINT_3D_MAP: Routine = Routine::new("print_3D_map", 0x0859_3260);
const CID_TEXTURES3D: u32 = 0x0878_16cc;

/// The 3D textures of an element and of the materials hanging on it, each as a block named
/// `"3_<prefix>_<owner name>__0"` / `"3_<prefix>_<owner name>_<material>_0"`
/// [soft 0x08593090 print_model_3D_maps].
pub fn print_model_3d_maps(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, prefix: u32) {
    if dict == 0 || elem == 0 {
        return;
    }
    let owner = m.u32(elem + 4);
    if owner == 0 {
        return;
    }
    let l = m.local(0x41c);
    let ebp = l + 0x41c;
    let (buf, l404, l408, l40c) = (ebp - 0x400, ebp - 0x404, ebp - 0x408, ebp - 0x40c);
    let name_or_empty = |m: &mut Mem, o: &mut dyn Outside| -> Vec<u8> {
        if o.call(m, SI_EMPTY_STRING, &[prefix]).eax as u16 != 0 {
            Vec::new()
        } else {
            m.cstr(prefix)
        }
    };
    let cid = m.u32(CID_TEXTURES3D);
    o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x1f4, cid, l404, 0]);
    let t = m.u32(l404);
    if t != 0 && m.u8(t + 0x5b) == 1 {
        let pre = name_or_empty(m, o);
        let own = cstr_or_null(m, m.u32(owner + 4));
        let mut text = b"\"3_".to_vec();
        text.extend_from_slice(&pre);
        text.push(b'_');
        text.extend_from_slice(&own);
        text.extend_from_slice(b"__0\"");
        m.put_cstr(buf, &text);
        let t = m.u32(l404);
        o.call(m, PRINT_3D_MAP, &[dict, t, buf]);
    }
    let mats = m.u32(CID_MATERIALS);
    let mut mat = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, l40c, l408]).eax;
    while mat != 0 {
        let cid = m.u32(CID_TEXTURES3D);
        let cur = m.u32(l40c);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, cur, 1, 3, 0x1f5, cid, l404, 0]);
        let t = m.u32(l404);
        if t != 0 && m.u8(t + 0x5b) == 1 {
            let pre = name_or_empty(m, o);
            let own = cstr_or_null(m, m.u32(owner + 4));
            let mn = cstr_or_null(m, mat);
            let mut text = b"\"3_".to_vec();
            text.extend_from_slice(&pre);
            text.push(b'_');
            text.extend_from_slice(&own);
            text.push(b'_');
            text.extend_from_slice(&mn);
            text.extend_from_slice(b"_0\"");
            m.put_cstr(buf, &text);
            let t = m.u32(l404);
            o.call(m, PRINT_3D_MAP, &[dict, t, buf]);
        }
        let mats = m.u32(CID_MATERIALS);
        mat = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mats, l40c, l408]).eax;
    }
    m.release(0x41c);
}

const ADD_2D_MAPS: Routine = Routine::new("add2DMaps", 0x0859_12cc);
const PRECOMPUTE_MI_TEXTURE: Routine = Routine::new("precompute_mi_texture", 0x0859_390c);
const G_8791166: u32 = 0x0879_1166;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const SI_DATABASE: u32 = 0x0879_0640;

/// The `color texture` lines of the textures in the list, after collecting them (for each frame of
/// the range when `mode` is 1 and the render parameters ask for a sequence): mode 0 prints every
/// entry, mode 1 those used by a still, mode 2 those that are animated; the entry's picture is
/// read once for its size and its name written with the unix separators
/// [soft 0x08591380 print_2D_texture_map].
pub fn print_2d_texture_map(m: &mut Mem, o: &mut dyn Outside, dict: u32, all: u32, mode: u32, start: u32, end: u32, step: u32, rp: u32) {
    if dict == 0 || m.u16(G_8791166) == 0 {
        return;
    }
    let l = m.local(0x81c);
    let ebp = l + 0x81c;
    let (buf400, buf800) = (ebp - 0x400, ebp - 0x800);
    let incr = strdup_lit(m, if m.u32(INCREMENTAL) != 0 { b"incremental " } else { b"" });
    m.set_u32(N3DTEX, 0);
    let all16 = all as u16 as i16 as i32 as u32;
    if mode == 1 && m.u32(rp + 0x654) != 0 {
        if rp == 0 {
            m.release(0x81c);
            return;
        }
        let mut f = start as i32;
        let stop = (end as i32).wrapping_add(1);
        while f < stop {
            let t = (F80::from_i32(f) / F80::from_f32(f32::from_bits(m.u32(SI_FRAME_RATE)))).to_f32();
            let d4 = m.u32(rp + 4);
            o.call(m, ext::SI_RENDER_SCENE_UPDATE, &[t.to_bits(), SI_DATABASE, d4, dict]);
            let s = m.i16(rp + 0x60e) as i32 as u32;
            o.call(m, ADD_2D_MAPS, &[dict, all16, mode, s]);
            f = f.wrapping_add(step as i32);
        }
    } else {
        let s = m.i16(rp + 0x60e) as i32 as u32;
        o.call(m, ADD_2D_MAPS, &[dict, all16, mode, s]);
    }
    let mut e = m.u32(TEXLIST);
    let mut i = 0i32;
    while i < m.i32(NTEX) {
        let name = m.u32(e);
        if m.u8(name) == 0x28 {
            let resdir = m.u32(SI_RESDIR);
            o.call(m, crate::mi_calc::SWARNING, &[0x086f_2d00, name, resdir, 0x086f_2cde]);
            let mut t = cstr_or_null(m, resdir);
            t.extend_from_slice(b"/noIcon.pic");
            m.put_cstr(name, &t);
        }
        strncpy_n(m, buf400, name, 0x3fc);
        let s = m.cstr(buf400);
        if let Some(at) = s.windows(4).position(|w| w == b".pic") {
            m.set_u8(buf400 + at as u32, 0);
        }
        o.call(m, PRECOMPUTE_MI_TEXTURE, &[name, buf400, e + 0xc, e + 0x10, 1]);
        o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[buf400, buf400, 0x400]);
        let (fl, fn_) = match mode as i32 {
            0 => (Some(true), (0x086f_2d60u32, 0x086f_2d81u32)),
            1 => (Some(m.u16(e + 0x14) == 1), (0x086f_2dc0, 0x086f_2de1)),
            2 => (Some(m.u16(e + 0x14) == 0), (0x086f_2e20, 0x086f_2e41)),
            _ => (None, (0, 0)),
        };
        if fl == Some(true) {
            let text = if m.u16(MI2OUTPUT) != 0 {
                let p = m.u32(e + 4);
                cstr_or_null(m, p)
            } else {
                format!("tex_{}", i).into_bytes()
            };
            m.put_cstr(buf800, &text);
            let fmt = if m.u32(MI_LOCAL) != 0 { fn_.0 } else { fn_.1 };
            fprintf(m, o, &[fmt, incr, buf800, buf400]);
        }
        i += 1;
        e = e.wrapping_add(0x18);
    }
    fprintf(m, o, &[0x086f_2e5c]);
    m.free(incr);
    m.release(0x81c);
}

// ---- round 8: the 2D colour description

const CID_ENVIRON_SHADERS: u32 = 0x0878_16b0;
const FIND_RELATED_ELEM: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);

fn fi16(m: &Mem, a: u32) -> F80 {
    F80::from_i32(m.i16(a) as i32)
}

/// The 2D colour description `out` of an element's texture record: the mapping kind (+4), the
/// repeat kind (+8), the projection (+0xc), the bump factor (+0x10) and its two split halves
/// (+0x14, +0x18), the repeat scales (+0x1c, +0x20) and their inverted forms (+0x24, +0x28), the
/// flag +0x2c, the wrap flags (+0x40, +0x42 by `preprocess_texture`), the picture-size based offsets
/// (+0x30..+0x3c; a picture is read through `precompute_mi_texture` when the record asks for it),
/// the kind code (+0x44) and the record's matrix (+0xb8) [soft 0x08590a20 SI_init_MRSoftColor].
pub fn si_init_mr_soft_color(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, out: u32) {
    // the original's locals (0x43c bytes): the picture name at -0x418, the related-element out words
    let l = m.local(0x43c);
    let ebp = l + 0x43c;
    let (l418, l41c, l420, l424, l428) = (ebp - 0x418, ebp - 0x41c, ebp - 0x420, ebp - 0x424, ebp - 0x428);
    si_init_mr_soft_color_body(m, o, dict, elem, out, (l418, l41c, l420, l424, l428));
    m.release(0x43c);
}

fn si_init_mr_soft_color_body(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, out: u32, ls: (u32, u32, u32, u32, u32)) {
    let (l418, l41c, l420, l424, l428) = ls;
    let (mut state, mut wu, mut wv, mut done) = (0u32, 0u16, 0u16, 0u16);
    if dict == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 {
        return;
    }
    if m.u16(rec + 0x19c) != 0 {
        state = 3;
    }
    m.set_u16(out + 0x48, 0);
    if m.u16(rec + 0x2a).wrapping_sub(7) <= 1 {
        state = 2;
        let cid = m.u32(CID_ENVIRON_SHADERS);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, l41c, 0]);
        if m.u32(l41c) == 0 {
            m.set_u16(out + 0x48, 1);
        }
    }
    let cid = m.u32(CID_MODELS);
    let mut esi = o.call(m, FIND_RELATED_ELEM, &[dict, elem, 0, 0, 0x190, cid, l420, l420]).eax;
    if esi == 0 || m.u32(esi + 4) == 0 {
        let cid = m.u32(CID_MATERIALS);
        let mat = o.call(m, FIND_RELATED_ELEM, &[dict, elem, 0, 0, 0x191, cid, l420, l420]).eax;
        if mat != 0 {
            let cid = m.u32(CID_MODELS);
            esi = o.call(m, FIND_RELATED_ELEM, &[dict, mat, 0, 0, 0x12c, cid, l420, l420]).eax;
        }
    }
    if esi != 0 {
        let mrec = m.u32(esi + 4);
        if mrec != 0 && m.u32(mrec + 0x30) == 0xa {
            let p = m.u32(mrec + 0x48);
            wu = m.u16(p + 0x20);
            wv = m.u16(p + 0x44);
        }
    }
    let k = match m.u16(rec + 0x2a) {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };
    m.set_u32(out + 4, k);
    let r = match m.u16(rec + 0x10a) {
        1 => 1,
        2 => 2,
        4 => 3,
        _ => 0,
    };
    m.set_u16(out + 8, r);
    m.set_u32(out + 0xc, if m.u16(rec + 0x110) == 2 { 2 } else { 1 });
    let src = if state == 2 { 0x144 } else { 0x10c };
    let v = f(m, rec + src);
    put(m, out + 0x10, v);
    let b = f(m, rec + 0x140);
    if b.gt(F80::from_f64(0.0001)) || b.lt(F80::from_f64(-0.0001)) {
        if m.u16(rec + 0x19c) != 0 {
            let v = f(m, rec + 0x140);
            put(m, out + 0x14, v);
            m.set_u32(out + 0x18, 0);
        } else {
            m.set_u32(out + 0x14, 0);
            let v = -f(m, rec + 0x140);
            put(m, out + 0x18, v);
        }
    }
    for at in [0x20u32, 0x1c, 0x28, 0x24] {
        m.set_u32(out + at, 0);
    }
    if state != 2 {
        let one = F80::from_i32(1);
        let a1 = F80::from_i32((m.i16(rec + 0x32) as i32).wrapping_sub(m.i16(rec + 0x30) as i32)) + one;
        let b1 = one + F80::from_i32((m.i16(rec + 0x36) as i32).wrapping_sub(m.i16(rec + 0x34) as i32));
        let mut w = f(m, rec + 0x58);
        let n = m.u16(rec + 0xf8);
        if n != 0 {
            w = w / F80::from_i32(n as i16 as i32);
        }
        let z = F80::from_i32(0);
        let x1 = if a1.eq(z) { w } else { w / a1 };
        put(m, out + 0x1c, x1);
        let mut v = f(m, rec + 0x5c);
        let n = m.u16(rec + 0xfa);
        if n != 0 {
            v = v / F80::from_i32(n as i16 as i32);
        }
        let y1 = if b1.eq(z) { v } else { v / b1 };
        put(m, out + 0x20, y1);
        if m.u16(rec + 0x1cc) != 0 {
            let x = f(m, out + 0x1c) - one;
            put(m, out + 0x24, x);
        }
        if m.u16(rec + 0x1ce) != 0 {
            let x = f(m, out + 0x20) - one;
            put(m, out + 0x28, x);
        }
    }
    m.set_u16(out + 0x2c, (m.u16(rec + 0x18e) != 0 && state != 2) as u16);
    preprocess_texture(m, rec, wu as i16 as i32 as u32, wv as i16 as i32 as u32, out + 0x40, out + 0x42);
    if m.u16(rec + 0x2e) != 0 {
        let fc = m.u32(FRAME_CURRENT);
        set_file_name(m, o, rec, fc, 1);
        let name = m.u32(rec + 0x20);
        let ok = o.call(m, PRECOMPUTE_MI_TEXTURE, &[name, l418, l428, l424, 0]).eax as u16;
        if ok != 0 {
            let pw = F80::from_i32(m.i32(l428));
            let ph = F80::from_i32(m.i32(l424));
            let wp1 = F80::from_i32((m.i16(rec + 0x32) as i32).wrapping_add(1));
            let x30 = fi16(m, rec + 0x30);
            let (x34, x36) = (fi16(m, rec + 0x34), fi16(m, rec + 0x36));
            if m.u16(out + 0x48) != 0 {
                let (p, q) = if m.u16(rec + 0x78) != 0 { (wp1, x30) } else { (x30, wp1) };
                put(m, out + 0x30, p / pw);
                put(m, out + 0x34, q / pw);
                let (p, q) = if m.u16(rec + 0x7a) != 0 { (x36, x34) } else { (x34, x36) };
                put(m, out + 0x38, (ph - p) / ph);
                put(m, out + 0x3c, (ph - q) / ph);
            } else {
                let (p, q) = if m.u8(rec + 0xf8) & 1 == 0 && m.u16(rec + 0x78) != 0 && m.u16(rec + 0xfe) != 0 { (wp1, x30) } else { (x30, wp1) };
                put(m, out + 0x30, p / pw);
                put(m, out + 0x34, q / pw);
                let (p, q) = if m.u8(rec + 0xfa) & 1 == 0 && m.u16(rec + 0x7a) == 0 && m.u16(rec + 0x100) != 0 { (x34, x36) } else { (x36, x34) };
                put(m, out + 0x38, (ph - p) / ph);
                put(m, out + 0x3c, (ph - q) / ph);
            }
            done = 1;
        }
    }
    if done == 0 {
        let (a, b, c, d);
        if m.u16(out + 0x48) != 0 {
            (a, b) = if m.u16(rec + 0x78) != 0 { (1.0f32, 0.0f32) } else { (0.0, 1.0) };
            (c, d) = if m.u16(rec + 0x7a) != 0 { (0.0, 1.0) } else { (1.0, 0.0) };
        } else {
            (a, b) = if m.u8(rec + 0xf8) & 1 == 0 && m.u16(rec + 0x78) != 0 && m.u16(rec + 0xfe) != 0 { (1.0, 0.0) } else { (0.0, 1.0) };
            (c, d) = if m.u8(rec + 0xfa) & 1 == 0 && m.u16(rec + 0x7a) == 0 && m.u16(rec + 0x100) != 0 { (1.0, 0.0) } else { (0.0, 1.0) };
        }
        for (at, v) in [(0x30u32, a), (0x34, b), (0x38, c), (0x3c, d)] {
            m.set_u32(out + at, v.to_bits());
        }
    }
    let code = match m.u16(rec + 0x2a) {
        1 => 0x20,
        2 => 0x21,
        3 => 0x22,
        4 => if m.u16(rec + 0x1d4) != 0 { 0x25 } else { 0x23 },
        _ => 0x24,
    };
    m.set_u32(out + 0x44, code);
    let one = f32::from_bits(0x3f80_0000);
    let (u, v, sx, sy) = (f32::from_bits(m.u32(rec + 0x68)), f32::from_bits(m.u32(rec + 0x6c)), f32::from_bits(m.u32(rec + 0x58)), f32::from_bits(m.u32(rec + 0x5c)));
    let rot = f32::from_bits(m.u32(rec + 0x168));
    if u == 0.0 && v == 0.0 && sx == one && sy == one && rot == 0.0 {
        crate::mi_calc::init_matrix(m, rec + 0xb8);
    } else {
        calc_2d_matrix(m, o, rec + 0xb8, rec);
    }
}

// ---- round 8: the 2D texture blocks and their callers

const PRINT_MODEL_2D_MAPS: Routine = Routine::new("print_model_2D_maps", 0x0859_233c);
const PRINT_2D_MAP: Routine = Routine::new("print_2D_map", 0x0859_28b8);
const SI_ELEMDICT_RELATIONS_COUNT: Routine = Routine::new("SI_elemdictRelationsCount", 0x0813_5700);
const SI_ELEMDICT_GET_ELEM: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
const CLEAR_LOW_BYTE_WORK_FLAG: Routine = Routine::new("SI_elemlistClearLowByteWorkFlag", 0x0813_2c70);
const CID_TEXTURES2D_SHADERS: u32 = 0x0878_1694;
const MI2OUTPUT_G: u32 = MI2OUTPUT;

/// The `color texture` / `shader` block of one 2D texture of an element: the header (a user
/// shader's own name when the element has an enabled one, else the program's `<prefix>_color`), the
/// texture name and repeat settings, the picture-size based limits, the torus flags, the method
/// code, the transform when it is not the identity and the closing text; `wu` and `wv` are the
/// wrap flags of the owning model [soft 0x085928b8 print_2D_map].
pub fn print_2d_map(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, name: u32, wu: u32, wv: u32) {
    // the original's locals, 0x18c bytes: the matrix at -0x40, the shader name at -0x140
    let l = m.local(0x18c);
    let ebp = l + 0x18c;
    print_2d_map_body(m, o, dict, elem, name, wu as u16, wv as u16, ebp);
    m.release(0x18c);
}

#[allow(clippy::too_many_arguments)]
fn print_2d_map_body(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, name: u32, wu: u16, wv: u16, ebp: u32) {
    let (mat, sbuf, l156, l154) = (ebp - 0x40, ebp - 0x140, ebp - 0x156, ebp - 0x154);
    if dict == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 || m.u16(G_8791166) == 0 {
        return;
    }
    let mi2 = m.u16(MI2OUTPUT_G) != 0;
    if !mi2 && need_texture(m, rec) == 0 {
        return;
    }
    let idx: i32;
    if m.u16(MI2OUTPUT_G) != 0 {
        let n = generate_texture_name(m, o, rec);
        idx = mi2_lookup_2d_texture_list(m, n);
        if idx < 0 {
            o.call(m, SFATAL, &[0x086f_31b3, n]);
        }
        o.call(m, MEMTRACK_FREE, &[SI_MEM_PARAMS, n]);
    } else {
        let n = m.u32(rec + 0x20);
        idx = lookup_2d_texture_list(m, n);
        if idx < 0 {
            let n = m.u32(rec + 0x20);
            o.call(m, SFATAL, &[0x086f_31c3, n]);
        }
    }
    let incr = strdup_lit(m, if m.u32(INCREMENTAL) != 0 { b"incremental " } else { b"" });
    let kind = strdup_lit(m, if m.u16(MI2OUTPUT_G) != 0 { b"shader" } else { b"color texture" });
    let cid = if m.u16(rec + 0x2a).wrapping_sub(7) <= 1 { m.u32(CID_ENVIRON_SHADERS) } else { m.u32(CID_TEXTURES2D_SHADERS) };
    let udf = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, 0, 0]).eax;
    let ent = m.u32(TEXLIST).wrapping_add((idx as u32).wrapping_mul(0x18));
    // the sizes are kept as floats (an int to float store)
    let wf = F80::from_i32(m.i32(ent.wrapping_add(0xc))).to_f32();
    let hf = F80::from_i32(m.i32(ent.wrapping_add(0x10))).to_f32();
    let (w, h) = (F80::from_f32(wf), F80::from_f32(hf));
    let sname = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, sbuf, 0x100]).eax as u16;
    let env = m.u32(CID_ENVIRON_SHADERS);
    let enabled = |m: &mut Mem, o: &mut dyn Outside| o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0;
    let usedef = |m: &mut Mem, o: &mut dyn Outside| o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0;
    // `udf` has a record that is not the environment shader's: the original's test on +4
    let own = |m: &Mem| udf != 0 && m.u32(udf + 4) != env;
    if udf != 0 && own(m) && sname != 0 && enabled(m, o) {
        fprintf(m, o, &[0x086f_31f6, incr, kind, name, sbuf]);
    } else {
        let pre = m.u32(SOFT_PREFIX);
        fprintf(m, o, &[0x086f_3204, incr, kind, name, pre]);
    }
    if sname == 0 || udf == 0 || usedef(m, o) {
        let kind_code = m.u16(rec + 0x2a);
        preprocess_texture(m, rec, wu as i16 as i32 as u32, wv as i16 as i32 as u32, l156, l154);
        if udf != 0 && own(m) && enabled(m, o) && usedef(m, o) {
            fprintf(m, o, &[0x086f_321a]);
        }
        if m.u16(MI2OUTPUT_G) != 0 {
            let t = m.u32(m.u32(TEXLIST).wrapping_add((idx as u32).wrapping_mul(0x18)).wrapping_add(4));
            fprintf(m, o, &[0x086f_322b, t]);
        } else {
            fprintf(m, o, &[0x086f_323d, idx as u32]);
        }
        fprintf(m, o, &[0x086f_3253, m.i16(rec + 0xf8) as i32 as u32]);
        fprintf(m, o, &[0x086f_3260, m.i16(rec + 0xfa) as i32 as u32]);
        let onoff = |m: &Mem, at: u32, on: u32, off: u32| if m.u16(at) != 0 { on } else { off };
        let v = onoff(m, rec + 0xfe, 0x086f_326d, 0x086f_3270);
        fprintf(m, o, &[0x086f_3274, v]);
        let v = onoff(m, rec + 0x100, 0x086f_3281, 0x086f_3284);
        fprintf(m, o, &[0x086f_3288, v]);
        let v = onoff(m, rec + 0xfc, 0x086f_3295, 0x086f_3298);
        fprintf(m, o, &[0x086f_329c, v]);
        if m.u16(rec + 0x2e) != 0 {
            let x30 = F80::from_i32(m.i16(rec + 0x30) as i32);
            let x32 = F80::from_i32((m.i16(rec + 0x32) as i32).wrapping_add(1));
            let x34 = F80::from_i32(m.i16(rec + 0x34) as i32);
            let x36 = F80::from_i32((m.i16(rec + 0x36) as i32).wrapping_add(1));
            let d = |x: F80| dw((x).to_f64());
            if m.u16(rec + 0x78) != 0 {
                let a = d(x32 / w);
                fprintf(m, o, &[0x086f_32a9, a[0], a[1]]);
                let a = d(x30 / w);
                fprintf(m, o, &[0x086f_32b6, a[0], a[1]]);
            } else {
                let a = d(x30 / w);
                fprintf(m, o, &[0x086f_32c3, a[0], a[1]]);
                let a = d(x32 / w);
                fprintf(m, o, &[0x086f_32d0, a[0], a[1]]);
            }
            if m.u16(rec + 0x7a) != 0 {
                let a = d((h - x36) / h);
                fprintf(m, o, &[0x086f_32dd, a[0], a[1]]);
                let a = d((h - x34) / h);
                fprintf(m, o, &[0x086f_32ea, a[0], a[1]]);
            } else {
                let a = d((h - x34) / h);
                fprintf(m, o, &[0x086f_32f7, a[0], a[1]]);
                let a = d((h - x36) / h);
                fprintf(m, o, &[0x086f_3304, a[0], a[1]]);
            }
        } else {
            if m.u16(rec + 0x78) != 0 {
                fprintf(m, o, &[0x086f_3311]);
                fprintf(m, o, &[0x086f_331f]);
            } else {
                fprintf(m, o, &[0x086f_332d]);
                fprintf(m, o, &[0x086f_333b]);
            }
            if m.u16(rec + 0x7a) != 0 {
                fprintf(m, o, &[0x086f_3349]);
                fprintf(m, o, &[0x086f_3357]);
            } else {
                fprintf(m, o, &[0x086f_3365]);
                fprintf(m, o, &[0x086f_3373]);
            }
        }
        let v = if m.u16(l156) != 0 { 0x086f_3381 } else { 0x086f_3384 };
        fprintf(m, o, &[0x086f_3388, v]);
        let v = if m.u16(l154) != 0 { 0x086f_3398 } else { 0x086f_339b };
        fprintf(m, o, &[0x086f_339f, v]);
        fprintf(m, o, &[0x086f_33af]);
        match kind_code {
            1 => fprintf(m, o, &[0x086f_33ba]),
            2 => fprintf(m, o, &[0x086f_33bc]),
            3 => fprintf(m, o, &[0x086f_33be]),
            4 => {
                let v = if m.u16(rec + 0x1d4) != 0 { 5 } else { 3 };
                fprintf(m, o, &[0x086f_33c0, v]);
            }
            _ => fprintf(m, o, &[0x086f_33c3]),
        }
        let one = f32::from_bits(0x3f80_0000);
        let fl = |m: &Mem, a: u32| f32::from_bits(m.u32(a));
        let identity = fl(m, rec + 0x68) == 0.0 && fl(m, rec + 0x6c) == 0.0 && fl(m, rec + 0x58) == one && fl(m, rec + 0x5c) == one && fl(m, rec + 0x168) == 0.0;
        if !identity {
            fprintf(m, o, &[0x086f_33c5]);
            calc_2d_matrix(m, o, mat, rec);
            print_matrix(m, o, mat);
        }
        if udf != 0 && own(m) && enabled(m, o) && usedef(m, o) {
            fprintf(m, o, &[0x086f_33c8]);
        }
        if udf == 0 {
            fprintf(m, o, &[0x086f_33ce]);
            m.free(incr);
            m.free(kind);
            return;
        }
    }
    if udf != 0 && own(m) && sname != 0 && enabled(m, o) {
        if usedef(m, o) {
            fprintf(m, o, &[0x086f_33cb]);
        }
        let fc = m.u32(FRAME_CURRENT);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
    }
    fprintf(m, o, &[0x086f_33ce]);
    m.free(incr);
    m.free(kind);
}

/// The 2D texture blocks of an element and of the materials hanging on it; each block gets a
/// name made of `prefix`, the owner's name and a running number (or, for the mental ray 2 output,
/// the texture's own name), and a vertex-colour helper block follows each material's textures
/// [soft 0x0859233c print_model_2D_maps].
pub fn print_model_2d_maps(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, prefix: u32, flag: u32) {
    let _ = flag;
    let l = m.local(0x43c);
    let ebp = l + 0x43c;
    print_model_2d_maps_body(m, o, dict, elem, prefix, ebp);
    m.release(0x43c);
}

fn print_model_2d_maps_body(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, prefix: u32, ebp: u32) {
    let (buf, l404, l408, l40c, l410) = (ebp - 0x400, ebp - 0x404, ebp - 0x408, ebp - 0x40c, ebp - 0x410);
    let (mut wu, mut wv) = (0u16, 0u16);
    let mut l420 = 0u32;
    if dict == 0 || elem == 0 {
        return;
    }
    let rec = m.u32(elem + 4);
    if rec == 0 {
        return;
    }
    match m.u32(rec + 0x30) {
        1 => {
            let p = m.u32(rec + 0x48);
            wu = m.u16(p + 0x10);
            wv = m.u16(p + 0x12);
        }
        10 => {
            let p = m.u32(rec + 0x48);
            wu = m.u16(p + 0x20);
            wv = m.u16(p + 0x44);
        }
        _ => {}
    }
    let tcid = m.u32(CID_TEXTURES2D);
    let l41c = o.call(m, SI_ELEMDICT_RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, tcid]).eax;
    let mut ret = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tcid, l408, l404]).eax;
    let mut l418 = 0u32;
    let empty = |m: &mut Mem, o: &mut dyn Outside, p: u32| o.call(m, SI_EMPTY_STRING, &[p]).eax as u16 != 0;
    let mi2 = |m: &Mem| m.u16(MI2OUTPUT_G) != 0;
    while m.u32(l408) != 0 {
        let t = m.u32(l408);
        if m.u8(t + 0x5b) == 1 {
            let text = if mi2(m) {
                let tn = m.u32(m.u32(t) + 0x10);
                let a = m.u32(ret.wrapping_add(0x18));
                let e = empty(m, o, tn);
                let x = if !e { m.u32(m.u32(m.u32(l408)) + 0x10) } else { 0x086f_3053 };
                cprintf::format_bytes(m, b"\"%s_%s\"", &[x, a])
            } else {
                let own = m.u32(rec + 4);
                let e = empty(m, o, prefix);
                let x = if e { 0x086f_305c } else { prefix };
                cprintf::format_bytes(m, b"\"%s_%s__%d\"", &[x, own, l418])
            };
            m.put_cstr(buf, &text);
            let t = m.u32(l408);
            o.call(m, PRINT_2D_MAP, &[dict, t, buf, wu as i16 as i32 as u32, wv as i16 as i32 as u32]);
        }
        ret = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x190, tcid, l408, l404]).eax;
        l418 = l418.wrapping_add(1);
    }
    let mcid = m.u32(CID_MATERIALS);
    let mut edi = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mcid, l40c, l404]).eax;
    while edi != 0 {
        if m.u16(G_8791166) != 0 {
            let mat = m.u32(l40c);
            let n = o.call(m, SI_ELEMDICT_RELATIONS_COUNT, &[dict, mat, 1, 3, 0, 0x191, tcid]).eax;
            l420 = n.wrapping_add(l41c);
        }
        let mat = m.u32(l40c);
        let mut esi = 0u32;
        if mat != 0 {
            let r = m.u32(mat);
            if r != 0 {
                esi = m.u32(r + 0x10);
            }
        }
        if empty(m, o, esi) {
            esi = 0;
        }
        let tcid = m.u32(CID_TEXTURES2D);
        let mat = m.u32(l40c);
        ret = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, mat, 1, 3, 0x191, tcid, l408, l410]).eax;
        l418 = 0;
        while m.u32(l408) != 0 {
            let t = m.u32(l408);
            if m.u8(t + 0x5b) == 1 {
                let text = if mi2(m) {
                    let tn = m.u32(m.u32(t) + 0x10);
                    let a = m.u32(ret.wrapping_add(0x18));
                    let e = empty(m, o, tn);
                    let x = if !e { m.u32(m.u32(m.u32(l408)) + 0x10) } else { 0x086f_3069 };
                    cprintf::format_bytes(m, b"\"%s_%s\"", &[x, a])
                } else {
                    let x = if esi == 0 { 0x086f_3072 } else { esi };
                    cprintf::format_bytes(m, b"\"%s__%s_%d\"", &[x, edi, l418])
                };
                m.put_cstr(buf, &text);
                let t = m.u32(l408);
                o.call(m, PRINT_2D_MAP, &[dict, t, buf, wu as i16 as i32 as u32, wv as i16 as i32 as u32]);
                let t = m.u32(l408);
                let b = m.u8(t + 0x5b) | 2;
                m.set_u8(t + 0x5b, b);
            }
            let mat = m.u32(l40c);
            ret = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, mat, 1, 3, 0x191, tcid, l408, l410]).eax;
            l418 = l418.wrapping_add(1);
        }
        if m.u32(edi.wrapping_add(0xdc8)) != 0x10001 {
            let k = strdup_lit(m, if mi2(m) { b"shader" } else { b"color texture" });
            let n1 = o.call(m, SI_ELEMDICT_RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, tcid]).eax;
            let mat = m.u32(l40c);
            let n2 = o.call(m, SI_ELEMDICT_RELATIONS_COUNT, &[dict, mat, 1, 3, 0, 0x191, tcid]).eax;
            let total = n2.wrapping_add(n1);
            let own = m.u32(rec + 4);
            let e = empty(m, o, prefix);
            let x = if e { 0x086f_3094 } else { prefix };
            let text = cprintf::format_bytes(m, b"\"%s_%s_%s_%d\"", &[x, own, edi, total]);
            m.put_cstr(buf, &text);
            if crate::mi_mr_main::si_get_mi2_output(m) as u16 == 0 {
                let f = if m.u32(INCREMENTAL) != 0 { 0x086f_30c0 } else { 0x086f_3100 };
                fprintf(m, o, &[f, k, buf]);
            } else {
                let f = if m.u32(INCREMENTAL) != 0 { 0x086f_3140 } else { 0x086f_3180 };
                fprintf(m, o, &[f, k, buf, l420]);
            }
            m.free(k);
        }
        edi = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, mcid, l40c, l404]).eax;
    }
    let _ = ret;
}

/// Prints the 2D textures (and the 3D ones when the 3D flag is on) of every model element that has
/// a texture and is not hidden, each once: the elements are marked with a work byte (+0x5b), a
/// parent element (+0x5c) stands for its children, and the section closes with an empty line
/// [soft 0x085916fc print_2D_texture_defs].
pub fn print_2d_texture_defs(m: &mut Mem, o: &mut dyn Outside, dict: u32, all: u32) {
    let all = all as u16;
    if dict == 0 || (m.u16(G_8791166) == 0 && m.u16(G_8791168) == 0) {
        return;
    }
    let block = (m.u32(CID_MODELS) << 6).wrapping_add(m.u32(dict + 4));
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(block + 4) {
        let e = off.wrapping_add(m.u32(block + 8));
        if e != 0 && m.u16(e + 0x48) == 0 {
            m.set_u8(e + 0x5b, 0);
        }
        i += 1;
        off = off.wrapping_add(0x84);
    }
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(block + 4) {
        let e = off.wrapping_add(m.u32(block + 8));
        if e != 0 && m.u16(e + 0x48) == 0 && m.u32(e + 4) != 0 {
            let k = m.u32(m.u32(e + 4) + 0x30);
            let ok = matches!(k, 10 | 1 | 2 | 4 | 7)
                && (m.u16(MI2OUTPUT) != 0 || m.u32(e + 0x20) != 0)
                && (all == 1 || matches!(m.u32(e + 0x24), 1 | 3 | 2));
            if ok {
                let parent = m.u32(e + 0x5c);
                let target = if parent == 0xffff_ffff {
                    Some(e).filter(|&t| m.u8(t + 0x5b) == 0)
                } else {
                    let cid = m.u32(CID_MODELS);
                    let p = o.call(m, SI_ELEMDICT_GET_ELEM, &[dict, cid, parent]).eax;
                    Some(p).filter(|&t| t != 0 && m.u8(t + 0x5b) == 0)
                };
                if let Some(t) = target {
                    if m.u8(e + 0x58) & 4 != 0 || m.u32(INCREMENTAL) == 0 {
                        let name = m.u32(m.u32(t) + 0x10);
                        if m.u16(G_8791166) != 0 {
                            o.call(m, PRINT_MODEL_2D_MAPS, &[dict, t, name, 1]);
                        }
                        if m.u16(G_8791168) != 0 {
                            let name = m.u32(m.u32(t) + 0x10);
                            o.call(m, PRINT_MODEL_3D_MAPS, &[dict, t, name]);
                        }
                        m.set_u8(t + 0x5b, 1);
                    }
                }
            }
        }
        i += 1;
        off = off.wrapping_add(0x84);
    }
    fprintf(m, o, &[0x086f_2e5e]);
    let cid = m.u32(CID_MODELS);
    o.call(m, CLEAR_LOW_BYTE_WORK_FLAG, &[dict, cid]);
}
const PRINT_MODEL_3D_MAPS: Routine = Routine::new("print_model_3D_maps", 0x0859_3090);

/// As [`print_2d_texture_defs`] for the single element of a shader preview: lists the textures
/// into the texture list, prints one `color texture "tex_N"` line per entry (the picture read once
/// for its size), the preview's in-memory pictures and the element's own texture blocks
/// [soft 0x08591938 print_2D_texture_defs_preview].
pub fn print_2d_texture_defs_preview(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, prev: u32) {
    if dict == 0 || elem == 0 || (m.u16(G_8791166) == 0 && m.u16(G_8791168) == 0) {
        return;
    }
    let l = m.local(0x40c);
    let buf = l + 0x40c - 0x400;
    m.set_u32(N3DTEX, 0);
    if m.u16(G_8791166) != 0 {
        o.call(m, PRINT_MODEL_2D_DEFS, &[dict, elem, 0, 0]);
        let mut e = m.u32(TEXLIST);
        let mut i = 0i32;
        while i < m.i32(NTEX) {
            let name = m.u32(e);
            if m.u8(name) == 0x28 {
                let resdir = m.u32(SI_RESDIR);
                o.call(m, crate::mi_calc::SWARNING, &[0x086f_2e80, name, resdir, 0x086f_2e60]);
                let mut t = cstr_or_null(m, resdir);
                t.extend_from_slice(b"/noIcon.pic");
                m.put_cstr(name, &t);
            }
            strncpy_n(m, buf, name, 0x3fc);
            let s = m.cstr(buf);
            if let Some(at) = s.windows(4).position(|w| w == b".pic") {
                m.set_u8(buf + at as u32, 0);
            }
            o.call(m, PRECOMPUTE_MI_TEXTURE, &[name, buf, e + 0xc, e + 0x10, 1]);
            fprintf(m, o, &[0x086f_2ebe, i as u32, buf]);
            i += 1;
            e = e.wrapping_add(0x18);
        }
        if prev != 0 {
            let mut n = 0u32;
            let mut p = prev.wrapping_add(0xdcc);
            while p != 0 {
                let q = m.u32(p.wrapping_add(0x170));
                if q != 0 {
                    let r = m.u32(q + 4);
                    let (a, b) = (m.i16(r) as i32 as u32, m.i16(r + 2) as i32 as u32);
                    fprintf(m, o, &[0x086f_2ee0, n, a, b, r]);
                }
                p = m.u32(p.wrapping_add(0x16c));
                n = n.wrapping_add(1);
            }
        }
        fprintf(m, o, &[0x086f_2f06]);
        o.call(m, PRINT_MODEL_2D_MAPS, &[dict, elem, 0, 0]);
    }
    if m.u16(G_8791168) != 0 {
        o.call(m, PRINT_MODEL_3D_MAPS, &[dict, elem, 0]);
        fprintf(m, o, &[0x086f_2f08]);
    }
    m.release(0x40c);
}
