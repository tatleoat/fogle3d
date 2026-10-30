// SPDX-License-Identifier: MIT
//! `custom_io.c`: the user data of models, the saved header and script of icons, and the loading and
//! saving of custom effects in scene files (finding 0041, "tenth continuation").

use crate::cfmt;
use crate::custom_gen::SI_CUS_TRACE;
use crate::custom_udf::PLT_FGETS;
use crate::{fprintf, through_x87};
use crate::STDERR;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::custom_fcv::{SI_ELEMDICT_GET_ELEM, SI_ELEM_DICTS};
use crate::custom_util::SI_PROC_DATA;
const SI_CID_MODELS: u32 = 0x0878_1680;
use si_mem::{clib, Mem, Outside, Routine};

const PLT_FERROR: Routine = Routine::new("plt:ferror", 0x080c_3878);

/// The head of a model's list of user data nodes: 0x14 bytes each (name +0, size +8, data +0xc, next +0x10).
const USER_DATA: u32 = 0x34c;

/// [soft 0x08537154 si_cusModelSearchUserData]: the node named `name` in the user data list of `model`,
/// or 0.
pub fn si_cus_model_search_user_data(m: &mut Mem, model: u32, name: u32) -> u32 {
    let mut n = m.u32(model.wrapping_add(USER_DATA));
    while n != 0 {
        let s = m.u32(n);
        if clib::strcmp(m, s, name) == 0 {
            return n;
        }
        n = m.u32(n.wrapping_add(0x10));
    }
    0
}

/// [soft 0x085370d4 si_cusModelAttachUserData]: appends a node (a copy of `name`, `size`, the `data`
/// block itself) to the user data list of `model`. 1, or 0 when out of memory.
pub fn si_cus_model_attach_user_data(m: &mut Mem, model: u32, name: u32, size: u32, data: u32) -> i32 {
    let n = m.malloc(0x14);
    if n == 0 {
        return 0;
    }
    let s = m.strdup(name);
    m.set_u32(n, s);
    m.set_u32(n.wrapping_add(8), size);
    m.set_u32(n.wrapping_add(0xc), data);
    m.set_u32(n.wrapping_add(0x10), 0);
    let head = m.u32(model.wrapping_add(USER_DATA));
    if head == 0 {
        m.set_u32(model.wrapping_add(USER_DATA), n);
    } else {
        let mut t = head;
        while m.u32(t.wrapping_add(0x10)) != 0 {
            t = m.u32(t.wrapping_add(0x10));
        }
        m.set_u32(t.wrapping_add(0x10), n);
    }
    1
}

/// [soft 0x08537230 si_cusModelDisposeUserData]: unlinks the node named `name` from the user data list of
/// `model` and frees its name, its data and itself. 1, or 0 when there is none.
pub fn si_cus_model_dispose_user_data(m: &mut Mem, model: u32, name: u32) -> i32 {
    let mut prev = 0u32;
    let mut n = m.u32(model.wrapping_add(USER_DATA));
    while n != 0 {
        let s = m.u32(n);
        if clib::strcmp(m, s, name) == 0 {
            let next = m.u32(n.wrapping_add(0x10));
            if prev != 0 {
                m.set_u32(prev.wrapping_add(0x10), next);
            } else {
                m.set_u32(model.wrapping_add(USER_DATA), next);
            }
            let s = m.u32(n);
            if s != 0 {
                m.free(s);
                m.set_u32(n, 0);
            }
            let d = m.u32(n.wrapping_add(0xc));
            if d != 0 {
                m.free(d);
                m.set_u32(n.wrapping_add(0xc), 0);
            }
            m.free(n);
            return 1;
        }
        prev = n;
        n = m.u32(n.wrapping_add(0x10));
    }
    0
}

/// Looks the user data `key` of `model` up for the two icon getters: `*node` gets the node; its data
/// (+0xc), or `msg` on stderr and 0 when there is none.
fn icon_saved(m: &mut Mem, o: &mut dyn Outside, model: u32, node: u32, key: u32, msg: u32) -> u32 {
    let n = si_cus_model_search_user_data(m, model, key);
    m.set_u32(node, n);
    if n != 0 {
        m.u32(n.wrapping_add(0xc))
    } else {
        let err = m.u32(STDERR);
        fprintf(m, o, err, msg, &[]);
        0
    }
}

/// [soft 0x08538558 cusIconGetSavedHeader]: the saved custom header (user data "custom header") of the
/// icon `model`, its node stored at `*node`; 0 (and a complaint on stderr) when there is none.
pub fn cus_icon_get_saved_header(m: &mut Mem, o: &mut dyn Outside, model: u32, node: u32) -> u32 {
    icon_saved(m, o, model, node, 0x086e_05ad, 0x086e_05c0)
}

/// [soft 0x085385a0 cusIconGetSavedScript]: the saved custom script (user data "custom script") of the
/// icon `model`, as [`cus_icon_get_saved_header`].
pub fn cus_icon_get_saved_script(m: &mut Mem, o: &mut dyn Outside, model: u32, node: u32) -> u32 {
    icon_saved(m, o, model, node, 0x086e_05e1, 0x086e_0600)
}

/// The five lists searched by [`si_cus_get_rel_name`]: pointers to a head record whose chain starts at +0xfc
/// (records: name +4, id +0xc, next +0xfc).
const REL_LISTS: [u32; 5] = [0x0882_2dac, 0x0882_2c4c, 0x0882_2aec, 0x0882_2f0c, 0x0882_298c];

/// [soft 0x0853737c SI_cusGetRelName]: the name (+4) of the record with id (+0xc) `id` in the first of
/// five relation lists that has one, else 0.
pub fn si_cus_get_rel_name(m: &mut Mem, id: u32) -> u32 {
    for head in REL_LISTS {
        let mut p = m.u32(head);
        loop {
            p = m.u32(p.wrapping_add(0xfc));
            if p == 0 {
                break;
            }
            if m.u32(p.wrapping_add(0xc)) == id {
                return m.u32(p.wrapping_add(4));
            }
        }
    }
    0
}

/// [soft 0x08537428 si_cusReadDescription]: reads the lines of a description from `fp` up to the first one
/// that starts with the 4 bytes at 0x086e04e2 (the end marker) into one string stored at `*out` (every line,
/// at most 199 bytes of it, appended; a line that cannot be stored is dropped). 1 on the marker; 0 for a
/// null or failed file, a null `out`, a non-null `*out`, or the end of the file (traced).
pub fn si_cus_read_description(m: &mut Mem, o: &mut dyn Outside, fp: u32, out: u32) -> i32 {
    if fp == 0 || o.call(m, PLT_FERROR, &[fp]).eax != 0 || out == 0 || m.u32(out) != 0 {
        return 0;
    }
    let line = m.local(0xcc);
    let r = loop {
        if o.call(m, PLT_FGETS, &[line, 0xc8, fp]).eax == 0 {
            o.call(m, SI_CUS_TRACE, &[0x086e_04c0]);
            break 0;
        }
        m.set_u8(line.wrapping_add(0xc8), 0);
        if clib::strncmp(m, line, 0x086e_04e2, 4) == 0 {
            break 1;
        }
        let cur = m.u32(out);
        let n = clib::strlen(m, line);
        if cur == 0 {
            let b = m.si_calloc(n.wrapping_add(1), 1);
            m.set_u32(out, b);
            if b != 0 {
                clib::memcpy(m, b, line, n);
            }
        } else {
            let have = clib::strlen(m, cur);
            let b = m.si_realloc(cur, n.wrapping_add(have).wrapping_add(2));
            m.set_u32(out, b);
            if b != 0 {
                clib::strcat(m, b, line);
            }
        }
    };
    m.release(0xcc);
    r
}

const EFFECT_MAPPINGS: u32 = 0x086e_024c;
const NB_EFFECT_MAPPINGS: u32 = 0x086e_0244;
const SZ_EFFECT_MAPPINGS: u32 = 0x086e_0248;

/// [soft 0x0853a960 cusEffectMappingCmp]: orders entry `i` of the table of effect mappings (12 bytes:
/// two names and a value) against the pair (`a`, `b`) by the first name, then the second: -1, 0 or 1.
pub fn cus_effect_mapping_cmp(m: &mut Mem, i: u32, a: u32, b: u32) -> i32 {
    let e = m.u32(EFFECT_MAPPINGS).wrapping_add(i.wrapping_mul(12));
    let mut r = clib::strcmp(m, m.u32(e), a);
    if r == 0 {
        r = clib::strcmp(m, m.u32(e.wrapping_add(4)), b);
        if r == 0 {
            return 0;
        }
    }
    if r >= 0 {
        1
    } else {
        -1
    }
}

/// [soft 0x0853a9bc cusEffectMappingGet]: binary search of the pair (`a`, `b`) in the table; `*pos`
/// (when set) gets the index where it is or would go; the value of the entry, or 0.
pub fn cus_effect_mapping_get(m: &mut Mem, a: u32, b: u32, pos: u32) -> u32 {
    let (mut lo, mut hi, mut found) = (0i32, m.u32(NB_EFFECT_MAPPINGS) as i32, -1i32);
    while lo < hi {
        let s = lo.wrapping_add(hi);
        let mid = s.wrapping_add(((s as u32) >> 31) as i32) >> 1;
        match cus_effect_mapping_cmp(m, mid as u32, a, b) {
            0 => {
                lo = mid;
                hi = lo;
                found = mid;
            }
            -1 => lo = mid + 1,
            1 => hi = mid,
            _ => {
                lo = -1;
                hi = -1;
            }
        }
    }
    if pos != 0 {
        m.set_u32(pos, lo as u32);
    }
    if found >= 0 {
        let t = m.u32(EFFECT_MAPPINGS);
        m.u32(t.wrapping_add((found as u32).wrapping_mul(12)).wrapping_add(8))
    } else {
        0
    }
}

/// [soft 0x0853aa70 cusEffectMappingAdd]: inserts the entry (copies of `a` and `b`, value `v`) at index `pos`
/// of the table (grown to twice its size plus 1 when full; nothing is done when that fails).
pub fn cus_effect_mapping_add(m: &mut Mem, pos: i32, a: u32, b: u32, v: u32) {
    let mut new = m.u32(EFFECT_MAPPINGS);
    let nb = m.u32(NB_EFFECT_MAPPINGS);
    if nb == m.u32(SZ_EFFECT_MAPPINGS) {
        let sz = nb.wrapping_add(nb).wrapping_add(1);
        m.set_u32(SZ_EFFECT_MAPPINGS, sz);
        new = m.si_calloc(sz, 12);
        if new == 0 {
            return;
        }
    }
    m.set_u32(NB_EFFECT_MAPPINGS, nb.wrapping_add(1));
    let old = m.u32(EFFECT_MAPPINGS);
    let copy = |m: &mut Mem, from: i32, to: i32| {
        let (s, d) = (old.wrapping_add((from as u32).wrapping_mul(12)), new.wrapping_add((to as u32).wrapping_mul(12)));
        for k in [0u32, 4, 8] {
            let w = m.u32(s.wrapping_add(k));
            m.set_u32(d.wrapping_add(k), w);
        }
    };
    let mut k = nb as i32 - 1;
    if nb as i32 > pos {
        loop {
            copy(m, k, k + 1);
            let again = k > pos;
            k -= 1;
            if !again {
                break;
            }
        }
    }
    while k >= 0 {
        copy(m, k, k);
        k -= 1;
    }
    if new != old {
        if old != 0 {
            m.free(old);
            m.set_u32(EFFECT_MAPPINGS, 0);
        }
        m.set_u32(EFFECT_MAPPINGS, new);
    }
    let e = new.wrapping_add((pos as u32).wrapping_mul(12));
    let sa = m.strdup(a);
    m.set_u32(e, sa);
    let t = m.u32(EFFECT_MAPPINGS);
    let sb = m.strdup(b);
    m.set_u32(t.wrapping_add((pos as u32).wrapping_mul(12)).wrapping_add(4), sb);
    m.set_u32(t.wrapping_add((pos as u32).wrapping_mul(12)).wrapping_add(8), v);
}

/// [soft 0x0853abb4 cusEffectMappingClear]: frees the two names of every entry of the table and empties it
/// (the table itself is kept).
pub fn cus_effect_mapping_clear(m: &mut Mem) {
    let n = m.u32(NB_EFFECT_MAPPINGS) as i32;
    let mut i = 0i32;
    while i < n {
        let e = m.u32(EFFECT_MAPPINGS).wrapping_add((i as u32).wrapping_mul(12));
        for k in [0u32, 4] {
            let p = m.u32(e.wrapping_add(k));
            if p != 0 {
                m.free(p);
                let e2 = m.u32(EFFECT_MAPPINGS).wrapping_add((i as u32).wrapping_mul(12));
                m.set_u32(e2.wrapping_add(k), 0);
            }
        }
        i += 1;
    }
    m.set_u32(NB_EFFECT_MAPPINGS, 0);
}

/// [soft 0x08537f74 computeRGBABufferLength]: the length of the text the four floats at `rgba` take written
/// with `%f` one after the other, each counted with 3 extra bytes for the first only; -1 for a null pointer.
pub fn compute_rgba_buffer_length(m: &mut Mem, rgba: u32) -> i32 {
    if rgba == 0 {
        return -1;
    }
    let buf = m.local(0x40);
    let fmts = [0x086e_053cu32, 0x086e_0540, 0x086e_0544, 0x086e_0548];
    let mut total = 0u32;
    for (k, f) in fmts.into_iter().enumerate() {
        let w = m.u32(rgba.wrapping_add(4 * k as u32));
        let b = (f32::from_bits(w) as f64).to_bits();
        cfmt::sprintf(m, buf, f, &[b as u32, (b >> 32) as u32]);
        total = total.wrapping_add(clib::strlen(m, buf));
        if k == 0 {
            total = total.wrapping_add(3);
        }
    }
    m.release(0x40);
    total as i32
}

const DIV40: u32 = 0xcccc_cccd;

/// The size a text of `n` bytes (written with its length, `%d`) takes in the data string: the bytes with
/// the length's digits and 3, rounded down to a multiple of 40 by the original's multiplication by the
/// constant in the image at 0x08662d40 (0xcccccccd).
fn data_size_step(m: &mut Mem, buf: u32, fmt: u32, len: u32, n: u32) -> u32 {
    cfmt::sprintf(m, buf, fmt, &[len]);
    let x = clib::strlen(m, buf).wrapping_add(n).wrapping_add(3);
    let hi = ((x as u64).wrapping_mul(DIV40 as u64) >> 32) as u32;
    (hi >> 5).wrapping_mul(40)
}

/// [soft 0x08537920 cusDsoDialogGetDataSize]: the size of the string that holds the data of the dialog
/// elements of the chain `list` (next at +0x44): 40 bytes per element, and for a text element (kind 0) or
/// a colour element (kind 4, through computeRGBABufferLength) more for a name and a value that do not fit
/// 40 bytes; stored at `*size`, with the number of non-null entries of the elements' tables (+0x2c, 4
/// words) at `*count`. 1, or 0 when a colour element has a null value.
pub fn cus_dso_dialog_get_data_size(m: &mut Mem, list: u32, size: u32, count: u32) -> i32 {
    let (mut sz, mut cnt) = (0u32, 0u32);
    let buf = m.local(0x14);
    let mut e = list;
    while e != 0 {
        for j in 0..4u32 {
            if m.u32(e.wrapping_add(0x2c + 4 * j)) != 0 {
                cnt = cnt.wrapping_add(1);
            }
        }
        sz = sz.wrapping_add(0x28);
        let kind = m.u16(e.wrapping_add(4));
        if kind == 0 {
            let value = m.u32(e.wrapping_add(0x18));
            let nl = clib::strlen(m, m.u32(e));
            let vl = clib::strlen(m, value);
            let n = vl.wrapping_add(nl);
            if n.wrapping_add(2) as i32 > 0x28 {
                sz = sz.wrapping_add(data_size_step(m, buf, 0x086e_04e7, vl, n));
            }
        } else if kind == 4 {
            let nl = clib::strlen(m, m.u32(e));
            let r = compute_rgba_buffer_length(m, e.wrapping_add(0x1c));
            if r == -1 {
                m.release(0x14);
                return 0;
            }
            let n = nl.wrapping_add(r as u32);
            if n.wrapping_add(1) as i32 > 0x28 {
                sz = sz.wrapping_add(data_size_step(m, buf, 0x086e_04ea, r as u32, n));
            }
        }
        e = m.u32(e.wrapping_add(0x44));
    }
    m.release(0x14);
    m.set_u32(size, sz);
    m.set_u32(count, cnt);
    1
}

pub const SI_FKEY_INITIALIZE: Routine = Routine::new("SI_fkeyInitialize", 0x0820_bc50);
pub const SI_FCURVE_DISPOSE: Routine = Routine::new("SI_fcurveDispose", 0x0820_a928);
pub const SI_FCURVE_ALLOCATE_IO: Routine = Routine::new("SI_fcurveAllocate", 0x0820_a810);

/// `SI_versionInput` (0x086793f0), a float.
const SI_VERSION_INPUT: u32 = 0x0867_93f0;

/// [soft 0x0853a1b0 cusDsoFcvkeyBinaryRead]: reads one function-curve key from the binary block `buf`
/// (`len` bytes) at the offset `*pos`: the key is blanked (SI_fkeyInitialize), then (token, value)
/// pairs follow, each token a 16-bit number and 0 the end: 1 time, 2 value, 3 the float at +0x18, 4 the
/// float at +8 (4 bytes each), 5/6 16-bit words at +0xe/+0xc, 7 the key type word (+0x1c; when
/// `SI_versionInput` is at most 2.64918 and the word has bit 0x10 set only bit 0x20 of the byte at +0x1c
/// is raised), 8 the word at +0x1e, 9 the word at +0x20; an unknown token reads nothing. `*pos` follows
/// every read, also a read that crosses the end (the value is then read all the same and the result is
/// 0). 1 at the closing 0 token, 0 for a null argument or a read past the end.
pub fn cus_dso_fcvkey_binary_read(m: &mut Mem, o: &mut dyn Outside, key: u32, len: u32, buf: u32, pos: u32) -> u32 {
    if key == 0 || buf == 0 || pos == 0 {
        return 0;
    }
    o.call(m, SI_FKEY_INITIALIZE, &[key]);
    let p = m.u32(pos);
    let mut ok = p.wrapping_add(2) <= len;
    let mut tok = m.u16(buf.wrapping_add(p));
    m.set_u32(pos, p.wrapping_add(2));
    if !ok || tok == 0 {
        return ok as u32;
    }
    loop {
        let p = m.u32(pos);
        // the token as a signed short, minus 1, against the table size: 0 and anything above 9 read nothing
        let t = (tok as i16 as i32).wrapping_sub(1) as u32;
        let (size, off): (u32, u32) = match t {
            0 => (4, 0),
            1 => (4, 4),
            2 => (4, 0x18),
            3 => (4, 8),
            4 => (2, 0xe),
            5 => (2, 0xc),
            6 => (2, 0x1c),
            7 => (2, 0x1e),
            8 => (2, 0x20),
            _ => (0, 0),
        };
        ok = true;
        if size != 0 {
            ok = p.wrapping_add(size) <= len;
            if t == 6 {
                let v = m.u16(buf.wrapping_add(p));
                m.set_u32(pos, p.wrapping_add(2));
                // SI_versionInput <= 2.64918 (an unordered compare is false)
                let ver = f32::from_bits(m.u32(SI_VERSION_INPUT)) as f64;
                if ver <= 2.64918 && v & 0x10 != 0 {
                    let b = m.u8(key.wrapping_add(0x1c));
                    m.set_u8(key.wrapping_add(0x1c), b | 0x20);
                } else {
                    m.set_u16(key.wrapping_add(0x1c), v);
                }
            } else if size == 4 {
                let v = m.u32(buf.wrapping_add(p));
                m.set_u32(key.wrapping_add(off), v);
                m.set_u32(pos, p.wrapping_add(4));
            } else {
                let v = m.u16(buf.wrapping_add(p));
                m.set_u16(key.wrapping_add(off), v);
                m.set_u32(pos, p.wrapping_add(2));
            }
        }
        if !ok {
            return 0;
        }
        let p = m.u32(pos);
        ok = p.wrapping_add(2) <= len;
        tok = m.u16(buf.wrapping_add(p));
        m.set_u32(pos, p.wrapping_add(2));
        if !ok {
            return 0;
        }
        if tok == 0 {
            return 1;
        }
    }
}

/// [soft 0x0853a3b4 cusDsoFcurveBinaryRead]: reads a function curve from the binary block `buf` (`len`
/// bytes) at `*pos`, into a new curve stored at `*fc` (an old one is disposed first; the new one comes
/// from SI_fcurveAllocate(0x236, `parent`)). Tokens as in [`cus_dso_fcvkey_binary_read`]: 1 a skipped
/// 16-bit word, 2 the key count as a signed 16-bit word (+0xc), 3..6 16-bit words at +0x18, +0x1a, +0x1c,
/// +0x1e, 7 the keys (an array of 0x24-byte keys rounded up to a multiple of 8 keys, each read with the
/// key reader, its curve link at +0x10 set to the curve and +0x14 cleared; a key count under 1 makes the
/// read fail), 8 the key count as a 32-bit word. On success `*pos` is stored back and 1 returned; the
/// curve is disposed and 0 returned on a short block or a failed key.
pub fn cus_dso_fcurve_binary_read(m: &mut Mem, o: &mut dyn Outside, fc: u32, parent: u32, len: u32, buf: u32, pos: u32) -> u32 {
    if fc == 0 || buf == 0 || pos == 0 {
        return 0;
    }
    let local = m.local(4);
    m.set_u32(local, 0);
    if m.u32(fc) != 0 {
        o.call(m, SI_FCURVE_DISPOSE, &[fc]);
    }
    let c = o.call(m, SI_FCURVE_ALLOCATE_IO, &[0x236, parent]).eax;
    m.set_u32(fc, c);
    let mut p = m.u32(local);
    let mut bx = (p.wrapping_add(2) <= len) as u32;
    let mut tok = m.u16(buf.wrapping_add(p));
    m.set_u32(local, p.wrapping_add(2));
    if bx == 1 && tok != 0 {
        'outer: loop {
            p = m.u32(local);
            let cur = m.u32(fc);
            let t = (tok as i16 as i32).wrapping_sub(1) as u32;
            match t {
                0 => {
                    bx = (p.wrapping_add(2) <= len) as u32;
                    m.set_u32(local, p.wrapping_add(2));
                }
                1 => {
                    bx = (p.wrapping_add(2) <= len) as u32;
                    let v = m.i16(buf.wrapping_add(p)) as i32;
                    m.set_u32(local, p.wrapping_add(2));
                    if bx != 1 {
                        break 'outer;
                    }
                    let cur = m.u32(fc);
                    m.set_u32(cur.wrapping_add(0xc), v as u32);
                }
                7 => {
                    bx = (p.wrapping_add(4) <= len) as u32;
                    let v = m.u32(buf.wrapping_add(p));
                    m.set_u32(cur.wrapping_add(0xc), v);
                    m.set_u32(local, p.wrapping_add(4));
                    if bx != 1 {
                        break 'outer;
                    }
                }
                2..=5 => {
                    let off = [0x18u32, 0x1a, 0x1c, 0x1e][(t - 2) as usize];
                    bx = (p.wrapping_add(2) <= len) as u32;
                    let v = m.u16(buf.wrapping_add(p));
                    m.set_u16(cur.wrapping_add(off), v);
                    m.set_u32(local, p.wrapping_add(2));
                }
                6 => {
                    let n = m.i32(cur.wrapping_add(0xc));
                    if n > 0 {
                        let nn = (n as u32).wrapping_add(7).wrapping_sub(((n as u32).wrapping_sub(1)) & 7);
                        let k = m.si_malloc(nn.wrapping_mul(0x24));
                        let cur = m.u32(fc);
                        m.set_u32(cur.wrapping_add(8), k);
                    } else {
                        m.set_u32(cur.wrapping_add(8), 0);
                        bx = 0;
                    }
                    let mut i = 0i32;
                    let mut off = 0u32;
                    let cur = m.u32(fc);
                    if 0 < m.i32(cur.wrapping_add(0xc)) {
                        if bx != 1 {
                            break 'outer;
                        }
                        loop {
                            let cur = m.u32(fc);
                            let k = m.u32(cur.wrapping_add(8)).wrapping_add(off);
                            bx = cus_dso_fcvkey_binary_read(m, o, k, len, buf, local);
                            let cur = m.u32(fc);
                            let ks = m.u32(cur.wrapping_add(8));
                            m.set_u32(ks.wrapping_add(off).wrapping_add(0x10), cur);
                            m.set_u16(ks.wrapping_add(off).wrapping_add(0x14), 0);
                            i += 1;
                            off = off.wrapping_add(0x24);
                            let cur = m.u32(fc);
                            if i >= m.i32(cur.wrapping_add(0xc)) || bx != 1 {
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
            if bx != 1 {
                break;
            }
            p = m.u32(local);
            bx = (p.wrapping_add(2) <= len) as u32;
            tok = m.u16(buf.wrapping_add(p));
            m.set_u32(local, p.wrapping_add(2));
            if bx != 1 || tok == 0 {
                break;
            }
        }
    }
    let end = m.u32(local);
    m.release(4);
    if bx as u16 == 0 {
        o.call(m, SI_FCURVE_DISPOSE, &[fc]);
    } else {
        m.set_u32(pos, end);
    }
    (bx as u16 as i16) as i32 as u32
}

/// A signed division the way the original compiles it: the high word of x * `mul`, shifted right by 4,
/// plus one for a negative x (`mul` 0x66666667 divides by 40, 0x63e7063f by 41).
fn div_mul(x: i32, mul: i64) -> i32 {
    (((x as i64 * mul) >> 32) as i32 >> 4) - (x >> 31)
}

/// [soft 0x085380b0 cusDialogRestoreData]: restores the values of the dialog elements of the chain `list`
/// (record: name +0, kind +4 as a 16-bit number, short +0xc, float +0x10, int +0x14, text +0x18, four
/// floats +0x1c, next +0x44) from the data string `text` (`len` bytes), the elements of `defaults` (the
/// same chain of the dialog's default values) filling in what the text lacks.
///
/// The text is a row of 40-byte items, "NAME value" (an item starting with a space is a long one:
/// " NAME length pos" and the data, of ((length + pos) / 41) * 40 + 40 bytes in all). A first pass counts the
/// items (n). Then each element is looked up among the items from the start of the unconsumed text (items
/// that are not its own are skipped, 40 bytes or the long item's size); a found item is read with sscanf
/// by kind (0 text, 1 float, 2 int, 3 short, 4 four floats; any other kind sets the result to -1), the
/// text advancing by the item only when the element was found at the very start; a missing one takes
/// the value of `defaults`. Result: -1 for an unknown kind; else 1 when some element had no item
/// plus 2 when fewer items matched than the text holds.
pub fn cus_dialog_restore_data(m: &mut Mem, o: &mut dyn Outside, list: u32, len: i32, text: u32, defaults: u32) -> u32 {
    use crate::custom_udf::PLT_SSCANF;
    let (mut list, mut len, mut text, mut defaults) = (list, len, text, defaults);
    let mut ret: u32 = 0;
    let mut n: i32 = 0;
    let mut idx: i32 = 0;
    let scratch = m.local(0x40);
    let (l4, l8) = (scratch, scratch + 4);
    // pass 1: the number of items
    if 0 < len {
        loop {
            n = n.wrapping_add(1);
            let c = m.u8(text.wrapping_add(idx as u32));
            if c != b' ' {
                idx = idx.wrapping_add(0x28);
            } else {
                o.call(m, PLT_SSCANF, &[text.wrapping_add(idx as u32).wrapping_add(1), 0x086e_054c, l8, l4]);
                let s = m.i32(l8).wrapping_add(m.i32(l4));
                idx = idx.wrapping_add(div_mul(s, 0x6666_6667).wrapping_mul(5).wrapping_add(5).wrapping_mul(8));
            }
            if idx >= len {
                break;
            }
        }
    }
    let mut matched: i32 = 0;
    let mut missing: i32 = 0;
    idx = 0;
    // the pointers into the scratch block (the original's locals)
    let (e10, ec, e14, e18, e1c, e20, e24) = (scratch + 8, scratch + 12, scratch + 16, scratch + 20, scratch + 24, scratch + 28, scratch + 32);
    loop {
        if ret != 0 {
            break;
        }
        if list == 0 {
            if missing > 0 {
                ret = 1;
            }
            if n > matched {
                ret |= 2;
            }
            break;
        }
        let mut consumed: i32 = 0;
        let name = m.u32(list);
        let nlen = clib::strlen(m, name) as i32;
        let kind = m.i16(list.wrapping_add(4)) as i32;
        loop {
            if idx < len {
                let base = text.wrapping_add(idx as u32);
                let sp = (m.u8(base) == b' ') as u32;
                let hit = clib::strncmp(m, base.wrapping_add(sp), name, nlen as u32) == 0
                    && m.u8(base.wrapping_add(sp).wrapping_add(nlen as u32)) == b' ';
                if !hit {
                    if m.u8(base) != b' ' {
                        idx = idx.wrapping_add(0x28);
                    } else {
                        o.call(m, PLT_SSCANF, &[base.wrapping_add(1), 0x086e_0556, e10, ec]);
                        let s = m.i32(e10).wrapping_add(m.i32(ec));
                        idx = idx.wrapping_add(div_mul(s, 0x6666_6667).wrapping_mul(5).wrapping_add(5).wrapping_mul(8));
                    }
                    continue;
                }
                matched = matched.wrapping_add(1);
                match kind as u32 {
                    0 => {
                        if m.u8(base) != b' ' {
                            o.call(m, PLT_SSCANF, &[base, 0x086e_0560, e14]);
                            let old = m.u32(list.wrapping_add(0x18));
                            if old != 0 {
                                m.free(old);
                                m.set_u32(list.wrapping_add(0x18), 0);
                            }
                            let p = m.si_calloc(0x28u32.wrapping_sub(m.u32(e14)), 1);
                            m.set_u32(list.wrapping_add(0x18), p);
                            let k = m.u32(e14);
                            clib::strncpy(m, p, k.wrapping_add(base), 0x27u32.wrapping_sub(k));
                            consumed = 0x28;
                        } else {
                            o.call(m, PLT_SSCANF, &[base.wrapping_add(1), 0x086e_0567, e1c, e18]);
                            let old = m.u32(list.wrapping_add(0x18));
                            if old != 0 {
                                m.free(old);
                                m.set_u32(list.wrapping_add(0x18), 0);
                            }
                            let l = m.u32(e1c);
                            let p = m.si_calloc(l.wrapping_add(1), 1);
                            m.set_u32(list.wrapping_add(0x18), p);
                            let l = m.u32(e1c);
                            let src = m.u32(e18).wrapping_add(1).wrapping_add(base);
                            clib::strncpy(m, p, src, l);
                            let s = m.i32(e1c).wrapping_add(m.i32(e18));
                            consumed = div_mul(s, 0x63e7_063f).wrapping_mul(5).wrapping_mul(8);
                        }
                    }
                    1 => {
                        o.call(m, PLT_SSCANF, &[base, 0x086e_0571, list.wrapping_add(0x10)]);
                        consumed = 0x28;
                    }
                    2 => {
                        o.call(m, PLT_SSCANF, &[base, 0x086e_0578, list.wrapping_add(0x14)]);
                        consumed = 0x28;
                    }
                    3 => {
                        o.call(m, PLT_SSCANF, &[base, 0x086e_057f, list.wrapping_add(0xc)]);
                        consumed = 0x28;
                    }
                    4 => {
                        if m.u8(base) != b' ' {
                            o.call(m, PLT_SSCANF, &[base, 0x086e_0587, list.wrapping_add(0x1c), list.wrapping_add(0x20), list.wrapping_add(0x24), list.wrapping_add(0x28)]);
                            consumed = 0x28;
                        } else {
                            o.call(m, PLT_SSCANF, &[base.wrapping_add(1), 0x086e_0597, e24, e20]);
                            let src = m.u32(e20).wrapping_add(1).wrapping_add(base);
                            o.call(m, PLT_SSCANF, &[src, 0x086e_05a1, list.wrapping_add(0x1c), list.wrapping_add(0x20), list.wrapping_add(0x24), list.wrapping_add(0x28)]);
                            let s = m.i32(e24).wrapping_add(m.i32(e20));
                            consumed = div_mul(s, 0x63e7_063f).wrapping_mul(5).wrapping_mul(8);
                        }
                    }
                    _ => ret = 0xffff_ffff,
                }
            } else {
                missing = missing.wrapping_add(1);
                match kind as u32 {
                    0 => {
                        let old = m.u32(list.wrapping_add(0x18));
                        if old != 0 {
                            m.free(old);
                            m.set_u32(list.wrapping_add(0x18), 0);
                        }
                        let s = m.strdup(m.u32(defaults.wrapping_add(0x18)));
                        m.set_u32(list.wrapping_add(0x18), s);
                    }
                    1 => {
                        let v = through_x87(m.u32(defaults.wrapping_add(0x10)));
                        m.set_u32(list.wrapping_add(0x10), v);
                    }
                    2 => {
                        let v = m.u32(defaults.wrapping_add(0x14));
                        m.set_u32(list.wrapping_add(0x14), v);
                    }
                    3 => {
                        let v = m.u16(defaults.wrapping_add(0xc));
                        m.set_u16(list.wrapping_add(0xc), v);
                    }
                    4 => {
                        for k in [0x1cu32, 0x20, 0x24, 0x28] {
                            let v = through_x87(m.u32(defaults.wrapping_add(k)));
                            m.set_u32(list.wrapping_add(k), v);
                        }
                    }
                    _ => ret = 0xffff_ffff,
                }
            }
            break;
        }
        if idx == 0 {
            text = text.wrapping_add(consumed as u32);
            len = len.wrapping_sub(consumed);
        } else {
            idx = 0;
        }
        list = m.u32(list.wrapping_add(0x44));
        defaults = m.u32(defaults.wrapping_add(0x44));
    }
    m.release(0x40);
    ret
}

pub const SI_CUS_STATUS_GET_EFFECT_BY_ICON: Routine = Routine::new("si_cusStatusGetEffectByIcon", 0x0854_acf4);

/// [soft 0x085385ec cusRestoreDsoFcurves]: gives the effect of the icon `model` (looked up in `status`) its
/// function curves from the user data "CUSD" of the model, a block of `size` bytes: records of a name
/// (text, NUL), a 16-bit count (0..4) and that many curves in binary form (cusDsoFcurveBinaryRead). The
/// curves go to the slots +0x2c.. of the effect's entry (chain at +0x64, next at +0x44) of that name; a
/// name the effect lacks is read into a scratch curve and disposed. 1 when the data is used up (or there
/// is none); 0 on a bad argument, a size that is not `size` (a complaint on stderr), no effect (a
/// complaint), a count over 4 or a curve that fails to read.
pub fn cus_restore_dso_fcurves(m: &mut Mem, o: &mut dyn Outside, model: u32, status: u32, size: i32) -> u32 {
    let loc = m.local(8);
    let (local_c, local_8) = (loc, loc + 4);
    m.set_u32(local_8, 0);
    let mut off: i32 = 0;
    let r = 'body: {
        if model == 0 || status == 0 {
            break 'body 0;
        }
        let node = si_cus_model_search_user_data(m, model, 0x086e_0621);
        if node == 0 || m.u32(node.wrapping_add(0xc)) == 0 {
            break 'body 1;
        }
        let msg;
        if m.i32(node.wrapping_add(8)) == size {
            let eff = o.call(m, SI_CUS_STATUS_GET_EFFECT_BY_ICON, &[status, model]).eax;
            if eff != 0 {
                let data = m.u32(node.wrapping_add(0xc));
                if size < 1 {
                    break 'body 1;
                }
                loop {
                    let name = (off as u32).wrapping_add(data);
                    let nl = clib::strlen(m, name);
                    let mut e = m.u32(eff.wrapping_add(100));
                    while e != 0 && clib::strcmp(m, name, m.u32(e)) != 0 {
                        e = m.u32(e.wrapping_add(0x44));
                    }
                    let count = m.u16(name.wrapping_add(nl).wrapping_add(1));
                    off = off.wrapping_add(nl as i32).wrapping_add(3);
                    if count > 4 {
                        break 'body 0;
                    }
                    if e == 0 {
                        let c = o.call(m, SI_FCURVE_ALLOCATE_IO, &[0x236, model]).eax;
                        m.set_u32(local_c, c);
                    }
                    let mut i = 0i32;
                    let mut slot = 0x2cu32;
                    while i < count as i16 as i32 {
                        let fc = if e != 0 { e.wrapping_add(slot) } else { local_c };
                        let ok = cus_dso_fcurve_binary_read(m, o, fc, eff, (size - off) as u32, (off as u32).wrapping_add(data), local_8) as u16 as i16;
                        if ok == 0 {
                            break 'body 0;
                        }
                        off = off.wrapping_add(m.i32(local_8));
                        slot += 4;
                        i += 1;
                    }
                    if e == 0 {
                        o.call(m, SI_FCURVE_DISPOSE, &[local_c]);
                    }
                    if size <= off {
                        break 'body 1;
                    }
                }
            }
            msg = 0x086e_066eu32;
        } else {
            msg = 0x086e_0640;
        }
        let err = m.u32(STDERR);
        fprintf(m, o, err, msg, &[]);
        0
    };
    m.release(8);
    r
}

pub const SI_CUS_GET_EFFECT_BY_ICON: Routine = Routine::new("SI_cusGetEffectByIcon", 0x0854_ac2c);
pub const SI_CUS_PARAM_ADD: Routine = Routine::new("SI_cusParamAdd", 0x0852_e57c);
pub const SI_DYNAMIC_SYMBOL_LIST_REMOVE_ALL: Routine = Routine::new("SI_dynamicSymbolListRemoveAll", 0x085e_6b8c);

/// [soft 0x08538a5c cusRestoreCustomElem]: records for the effect of the model with the id `id` (looked up
/// in the model dictionary) that the parameter `param` of the group `group` (1-based; `kind` 0 selects the
/// second of the two parameter tables of the effect's data at +0x104, otherwise the first) refers to the
/// element `refd` (an element dictionary entry; stored as its index) with the extra words `a`, `b`, `c`, `d`.
/// A parameter inside the group's current count is overwritten in place; a later one first grows the table
/// by empty entries (SI_cusParamAdd, which this stands in for). 0 when the model, effect or data is missing,
/// when `param` is negative, or when the group is out of range (the effect's list of symbols is then emptied);
/// 1 otherwise.
pub fn cus_restore_custom_elem(m: &mut Mem, o: &mut dyn Outside, id: u32, kind: u16, group: i32, param: i32, refd: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    let dict = m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS));
    let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[dict, m.u32(0x0878_1680), id]).eax;
    if e == 0 || m.u32(e.wrapping_add(4)) == 0 {
        return 0;
    }
    let loc = m.local(24);
    let (st, el) = (loc, loc + 4);
    let r = 'body: {
        let eff = o.call(m, SI_CUS_GET_EFFECT_BY_ICON, &[m.u32(e.wrapping_add(4)), st]).eax;
        if eff == 0 {
            break 'body 0;
        }
        let t = m.u32(eff.wrapping_add(0x104));
        if t == 0 {
            break 'body 0;
        }
        let (count, sizes, arr, pcnt) = if kind == 0 {
            (m.i32(t.wrapping_add(20)), m.u32(t.wrapping_add(24)), t.wrapping_add(32), t.wrapping_add(28))
        } else {
            (m.i32(t.wrapping_add(4)), m.u32(t.wrapping_add(8)), t.wrapping_add(16), t.wrapping_add(12))
        };
        if count < group || group < 1 {
            let l = m.u32(t).wrapping_add(4);
            if m.u32(l) == 0 {
                break 'body 1;
            }
            o.call(m, SI_DYNAMIC_SYMBOL_LIST_REMOVE_ALL, &[l]);
            break 'body 0;
        }
        if param < 0 {
            break 'body 0;
        }
        let idx = (refd.wrapping_sub(m.u32(SI_ELEM_DICTS)) as i32).wrapping_mul(0xb6db_6db7u32 as i32) >> 4;
        let sz = m.i32(sizes.wrapping_add(((group - 1) as u32).wrapping_mul(4)));
        if param < sz {
            let mut sum = 0i32;
            let mut i = 0;
            while i < group - 1 {
                sum = sum.wrapping_add(m.i32(sizes.wrapping_add((i as u32).wrapping_mul(4))).max(1));
                i += 1;
            }
            let p = m.u32(arr).wrapping_add((sum.wrapping_add(param) as u32).wrapping_mul(20));
            m.set_u32(p, idx as u32);
            m.set_u32(p.wrapping_add(12), c);
            m.set_u32(p.wrapping_add(16), d);
            m.set_u32(p.wrapping_add(4), a);
            m.set_u32(p.wrapping_add(8), b);
        } else {
            for (k, v) in [0xffff_ffffu32, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0].iter().enumerate() {
                m.set_u32(el + 4 * k as u32, *v);
            }
            if sz < param {
                for _ in 0..param - sz {
                    o.call(m, SI_CUS_PARAM_ADD, &[el, (group - 1) as u32, count as u32, sizes, pcnt, arr]);
                }
            }
            for (k, v) in [idx as u32, a, b, c, d].iter().enumerate() {
                m.set_u32(el + 4 * k as u32, *v);
            }
            o.call(m, SI_CUS_PARAM_ADD, &[el, (group - 1) as u32, count as u32, sizes, pcnt, arr]);
        }
        1
    };
    m.release(24);
    r
}

pub const SI_CUS_SET_CURRENT_DSO_EFFECT: Routine = Routine::new("SI_cusSetCurrentDsoEffect", 0x0852_e84c);
pub const SI_CUS_EFFECT_LOAD_DSO: Routine = Routine::new("si_cusEffectLoadDso", 0x0852_c51c);
pub const SI_CUS_RESOLVE_SYMBOL: Routine = Routine::new("SI_cusResolveSymbol", 0x0852_c4a0);

/// [soft 0x08538c5c cusReInitDsoEffect]: sets the shared library effect of `model` up again after a load:
/// loads its library, resolves the init and update functions of the library (the plain ones, else the
/// "custom" ones) and stores the update pair in the effect's data (+0x28, +0x2c), runs the init function
/// (the plain one with a context, else the custom one with the effect's parameters) with the effect made
/// current, and counts a post-defined effect (kind 0x73). A load or symbol failure (a message each), an
/// init function that reports 0x5f (a message) or 0x7c deletes the model's graphic list node. 1 when the
/// effect is ready, 0 otherwise.
pub fn cus_re_init_dso_effect(m: &mut Mem, o: &mut dyn Outside, model: u32) -> u32 {
    if model == 0 {
        return 0;
    }
    let loc = m.local(52);
    let (st, l2c, l30, l34, l38, ctx) = (loc, loc + 4, loc + 8, loc + 12, loc + 16, loc + 20);
    for a in [l2c, l30, l34, l38] {
        m.set_u32(a, 0);
    }
    let r = 'body: {
        let eff = o.call(m, SI_CUS_GET_EFFECT_BY_ICON, &[model, st]).eax;
        if eff == 0 {
            break 'body 0;
        }
        let t = m.u32(eff.wrapping_add(0x104));
        if t == 0 {
            break 'body 0;
        }
        let dl = m.u32(t);
        if dl == 0 {
            break 'body 0;
        }
        if m.u16(t.wrapping_add(0x38)) == 0x73 {
            crate::custom_misc::si_cus_inc_nb_post_def_fx(m);
        }
        let mut fail = false;
        let mut r6 = 0u32;
        if o.call(m, SI_CUS_EFFECT_LOAD_DSO, &[eff]).eax as u16 as i16 != 1 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086e_0780, 3]);
            fail = true;
        }
        // (symbol kind, out slot, fallback kind, fallback slot, messages)
        for (k1, s1, k2, s2, m1, m2) in [(9u32, l2c, 1u32, l30, 0x086e_07c0u32, 0x086e_0800u32), (10, l34, 2, l38, 0x086e_0860, 0x086e_08a0)] {
            if fail {
                break;
            }
            let mut r = o.call(m, SI_CUS_RESOLVE_SYMBOL, &[dl, k1, s1]).eax;
            if r != 0 {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[m1, 3]);
            }
            if m.u32(s1) == 0 {
                if r != 0 {
                    fail = true;
                    break;
                }
                r = o.call(m, SI_CUS_RESOLVE_SYMBOL, &[dl, k2, s2]).eax;
                if r != 0 {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[m2, 3]);
                }
            }
            if r != 0 {
                fail = true;
            }
        }
        if !fail {
            let t2 = m.u32(eff.wrapping_add(0x104));
            m.set_u32(t2.wrapping_add(0x28), m.u32(l38));
            let t2 = m.u32(eff.wrapping_add(0x104));
            m.set_u32(t2.wrapping_add(0x2c), m.u32(l34));
            let (f2c, f30) = (m.u32(l2c), m.u32(l30));
            if f2c != 0 || f30 != 0 {
                let d = m.u32(eff.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                if f2c == 0 {
                    let args = [m.u32(eff.wrapping_add(100)), m.u32(d.wrapping_add(0xc)).wrapping_sub(1), m.u32(d.wrapping_add(0x10)), m.u32(d.wrapping_add(0x1c)), m.u32(d.wrapping_add(0x20)), d.wrapping_add(0x24)];
                    r6 = o.call(m, Routine::at(f30), &args).eax;
                } else {
                    crate::custom_util::si_cus_context_init(m, ctx, eff, 9, 1, 0xffff_ffff);
                    r6 = o.call(m, Routine::at(f2c), &[ctx]).eax;
                }
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            }
            if r6 == 0x5f {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086e_0900, 3]);
            } else if r6 != 0x7c {
                break 'body 1;
            }
        }
        let gl = crate::custom_util::graf_list(m);
        let node = o.call(m, crate::custom_util::SI_FIND_GL_DATA, &[gl, model, 2]).eax;
        let gl = crate::custom_util::graf_list(m);
        o.call(m, crate::custom_util::SI_GRAFLIST_DELETE_GRAFNODE, &[gl, node]);
        0
    };
    m.release(52);
    r
}

pub const SI_CUS_PREPARE_EFFECT: Routine = Routine::new("SI_cusPrepareEffect", 0x0853_a688);
pub const CUS_ICON_GET_SAVED_HEADER: Routine = Routine::new("cusIconGetSavedHeader", 0x0853_8558);
pub const CUS_ICON_GET_SAVED_SCRIPT: Routine = Routine::new("cusIconGetSavedScript", 0x0853_85a0);
pub const CUS_DIALOG_RESTORE_DATA: Routine = Routine::new("cusDialogRestoreData", 0x0853_80b0);
pub const CUS_RESTORE_DSO_FCURVES: Routine = Routine::new("cusRestoreDsoFcurves", 0x0853_85ec);
pub const SI_CUS_ADD_MOTION_EFFECT: Routine = Routine::new("si_cusAddMotionEffect", 0x0854_7c14);
pub const SI_CUS_DSO_ALLOC_OCCURRENCE_ARRAYS: Routine = Routine::new("si_cusDsoAllocOccurrenceArrays", 0x0852_e6b4);
pub const SI_CUS_DSO_ALLOC_ARGS_RES: Routine = Routine::new("si_cusDsoAllocArgsRes", 0x0852_e478);
pub const SI_REL_DICT: u32 = 0x0878_15f0;

/// "Error restoring <name> custom icon"-style complaint of cusRestoreDsoEffect: the three strings joined, shown
/// at level 3, freed.
fn restore_complaint(m: &mut Mem, o: &mut dyn Outside, pre: u32, name: u32, post: u32, level: u32) {
    let msg = crate::custom_util::si_cus_concat_strings(m, &[pre, name, post]);
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, level]);
    if msg != 0 {
        m.free(msg);
    }
}

/// [soft 0x085387cc cusRestoreDsoEffect]: rebuilds the library effect of a custom icon (`model` kind 0xd,
/// +0x2ec) from its saved header, script and curves unless it already has one: prepares the effect from the
/// header (library name +0x38 when the header's version is over 1, script name +8), makes it the model's
/// (+0xc), adds it to the status, restores its dialog values (a mismatch is only a message), its curves and
/// the occurrence, argument and result arrays. Each failure shows "Error restoring <name> custom icon" and
/// answers 0; 1 when the effect is there, was already, or cannot be prepared.
pub fn cus_restore_dso_effect(m: &mut Mem, o: &mut dyn Outside, model: u32) -> u32 {
    if model == 0 || m.u32(model.wrapping_add(0x2ec)) != 0xd {
        return 0;
    }
    let loc = m.local(8);
    let (st, node) = (loc, loc + 4);
    let name = m.u32(model.wrapping_add(4));
    let r = 'body: {
        if o.call(m, crate::custom_io::SI_CUS_GET_EFFECT_BY_ICON, &[model, st]).eax != 0 {
            break 'body 1;
        }
        let hdr = o.call(m, CUS_ICON_GET_SAVED_HEADER, &[model, node]).eax;
        let (pre, nm, post);
        'err: {
            if hdr == 0 {
                (pre, nm, post) = (0x086e_0697u32, name, 0x086e_068au32);
                break 'err;
            }
            let lib = if m.i32(hdr) > 1 { hdr.wrapping_add(0x38) } else { 0 };
            let eff = o.call(m, SI_CUS_PREPARE_EFFECT, &[lib, hdr.wrapping_add(8)]).eax;
            if eff == 0 {
                break 'body 1;
            }
            let t = m.u32(eff.wrapping_add(0x104));
            if t == 0 || m.u32(t) == 0 {
                break 'body 1;
            }
            let status = m.u32(eff.wrapping_add(0x114));
            m.set_u32(st, status);
            m.set_u32(eff.wrapping_add(0xc), model);
            let e6 = o.call(m, SI_CUS_ADD_MOTION_EFFECT, &[status, eff, m.u32(hdr.wrapping_add(4))]).eax;
            if e6 == 0 {
                (pre, nm, post) = (0x086e_06b5, name, 0x086e_06a8);
                break 'err;
            }
            let script = o.call(m, CUS_ICON_GET_SAVED_SCRIPT, &[model, node]).eax;
            if m.u32(node) == 0 {
                (pre, nm, post) = (0x086e_06d3, name, 0x086e_06c6);
                break 'err;
            }
            let rd = o.call(m, CUS_DIALOG_RESTORE_DATA, &[m.u32(e6.wrapping_add(100)), m.u32(hdr.wrapping_add(0x30)), script, m.u32(eff.wrapping_add(0xb4)), e6.wrapping_add(0x10)]).eax as i32;
            if rd == -1 {
                (pre, nm, post) = (0x086e_06f1, m.u32(model), 0x086e_06e4);
                break 'err;
            }
            if rd != 0 {
                restore_complaint(m, o, 0x086e_070f, name, 0x086e_0702, 2);
            }
            let fc = o.call(m, CUS_RESTORE_DSO_FCURVES, &[model, m.u32(st), m.u32(hdr.wrapping_add(0x34))]).eax as u16 as i16;
            if fc != 1 {
                (pre, nm, post) = (0x086e_0737, name, 0x086e_072a);
                break 'err;
            }
            let d = m.u32(e6.wrapping_add(0x104));
            let idx = o.call(m, crate::custom_init::SI_RELDICT_GET_REL_INDEX_FROM_NAME, &[SI_REL_DICT, m.u32(st).wrapping_add(2), m.u32(e6.wrapping_add(4))]).eax;
            let a = o.call(m, SI_CUS_DSO_ALLOC_OCCURRENCE_ARRAYS, &[e6, idx, d.wrapping_add(4), d.wrapping_add(0x14), d.wrapping_add(8), d.wrapping_add(0x18)]).eax as u16 as i16;
            if a == 1 {
                let b = o.call(m, SI_CUS_DSO_ALLOC_ARGS_RES, &[e6, idx, d.wrapping_add(0xc), d.wrapping_add(0x1c), d.wrapping_add(0x10), d.wrapping_add(0x20)]).eax as u16 as i16;
                if b == 1 {
                    let d2 = m.u32(e6.wrapping_add(0x104));
                    m.set_u32(d2.wrapping_add(0x24), 0);
                    break 'body 1;
                }
            }
            (pre, nm, post) = (0x086e_0755, name, 0x086e_0748);
        }
        restore_complaint(m, o, pre, nm, post, 3);
        0
    };
    m.release(8);
    r
}

pub const SI_CUS_STATUS_GET_EFFECT_BY_NAME: Routine = Routine::new("SI_cusStatusGetEffectByName", 0x0854_adf4);
pub const SI_CUS_GET_EFFECT_BY_NAME: Routine = Routine::new("SI_cusGetEffectByName", 0x0854_ad34);
pub const SI_CUS_GET_STATUS_BY_NAME: Routine = Routine::new("SI_cusGetStatusByName", 0x0854_b564);
pub const CUS_EFFECT_MAPPING_GET: Routine = Routine::new("cusEffectMappingGet", 0x0853_a9bc);
pub const CUS_EFFECT_MAPPING_ADD: Routine = Routine::new("cusEffectMappingAdd", 0x0853_aa70);
pub const SI_CUS_EFFECT_GET_LATEST: Routine = Routine::new("si_cusEffectGetLatest", 0x0854_b404);
pub const SI_CUS_LOAD_EFFECT: Routine = Routine::new("SI_cusLoadEffect", 0x0853_3138);
pub const SI_DIALOG_REQUEST34: Routine = Routine::new("SI_dialogRequest34", 0x0814_2000);
const CUS_AUTO_UPGRADE: u32 = 0x086e_0240;
const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;

/// The message of SI_cusPrepareEffect: the strings joined, shown at `level`, freed.
fn prepare_message(m: &mut Mem, o: &mut dyn Outside, parts: &[u32], level: u32) {
    let msg = crate::custom_util::si_cus_concat_strings(m, parts);
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, level]);
    if msg != 0 {
        m.free(msg);
    }
}

/// [soft 0x0853a688 SI_cusPrepareEffect]: the effect named `script` (of the status named `lib`, or whichever
/// has it when `lib` is null) ready for use. A mapping already made for the pair (cusEffectMappingGet) is
/// the answer. A missing effect is replaced by the latest version of that name ("Automatically upgrading",
/// a complaint "Error finding" and 0 when there is none). An effect that has a newer version (+0x110 is
/// not itself) is upgraded when the global choice `cusAutoUpgrade` is 1, kept when it is 2, else the user is
/// asked (a renderer without a user: no to all); "Yes"/"Yes to All" upgrade, "No to All" fixes the choice.
/// An unloaded effect (+8 is 0) is loaded (a failed load shows "Error loading custom effect" and answers 0);
/// the pair's mapping is recorded. 0 for a null `script`.
pub fn si_cus_prepare_effect(m: &mut Mem, o: &mut dyn Outside, lib: u32, script: u32) -> u32 {
    if script == 0 {
        return 0;
    }
    let loc = m.local(8);
    let (local_c, local_8) = (loc, loc + 4);
    let r = 'body: {
        let mut eff;
        if lib == 0 {
            eff = o.call(m, SI_CUS_GET_EFFECT_BY_NAME, &[script, local_c]).eax;
        } else {
            eff = o.call(m, CUS_EFFECT_MAPPING_GET, &[lib, script, local_8]).eax;
            if eff != 0 {
                break 'body eff;
            }
            let st = o.call(m, SI_CUS_GET_STATUS_BY_NAME, &[lib]).eax;
            eff = o.call(m, SI_CUS_STATUS_GET_EFFECT_BY_NAME, &[st, script]).eax;
        }
        if eff == 0 {
            eff = o.call(m, SI_CUS_EFFECT_GET_LATEST, &[script]).eax;
            if eff == 0 {
                prepare_message(m, o, &[0x086e_09a8, script, 0x086e_0999], 3);
                break 'body 0;
            }
            prepare_message(m, o, &[0x086e_0980, script, 0x086e_0971], 2);
        }
        if m.u32(eff.wrapping_add(0x110)) != eff {
            // 0: ask / 1: upgrade / other: keep; the answer of the question decides this time
            let mut upgrade = false;
            let choice = m.i32(CUS_AUTO_UPGRADE);
            if choice == 0 {
                let s = if m.u16(SI_STANDALONE_RENDERER) == 0 {
                    let q = crate::custom_util::si_cus_concat_strings(m, &[0x086e_09e1, script, 0x086e_09c0]);
                    let s = o.call(m, SI_DIALOG_REQUEST34, &[q, 0x086e_0a00, 0x086e_09ff, 0x086e_09fb, 0x086e_09f0, 0x086e_09ed, 0x086e_09e3]).eax as u16 as i16;
                    if q != 0 {
                        m.free(q);
                    }
                    s
                } else {
                    3
                };
                if s == 1 {
                    m.set_u32(CUS_AUTO_UPGRADE, 1);
                    upgrade = true;
                } else if s != 2 {
                    if s != 3 {
                        upgrade = true;
                    } else {
                        m.set_u32(CUS_AUTO_UPGRADE, 2);
                    }
                }
            } else if choice == 1 {
                upgrade = true;
            }
            if upgrade {
                eff = m.u32(eff.wrapping_add(0x110));
                prepare_message(m, o, &[0x086e_0a35, script, 0x086e_0a26], 2);
            }
            if eff != m.u32(eff.wrapping_add(0x110)) {
                prepare_message(m, o, &[0x086e_0a5d, script, 0x086e_0a4e], 2);
            }
        }
        if m.u16(eff.wrapping_add(8)) == 0 {
            let st = m.u32(eff.wrapping_add(0x114));
            let rc = o.call(m, SI_CUS_LOAD_EFFECT, &[st, eff]).eax as i32;
            if rc < 0 {
                prepare_message(m, o, &[0x086e_0a71, m.u32(eff.wrapping_add(4)), 0x086e_0a6c, m.u32(eff.wrapping_add(0x10c))], 3);
                break 'body 0;
            }
        }
        let tag = m.u32(eff.wrapping_add(0x114)).wrapping_add(2);
        if lib == 0 {
            o.call(m, CUS_EFFECT_MAPPING_GET, &[tag, script, local_8]);
        }
        let idx = m.u32(local_8);
        o.call(m, CUS_EFFECT_MAPPING_ADD, &[idx, tag, script, eff]);
        eff
    };
    m.release(8);
    r
}

pub const CUS_RESTORE_DSO_EFFECT: Routine = Routine::new("cusRestoreDsoEffect", 0x0853_87cc);
pub const CUS_RE_INIT_DSO_EFFECT: Routine = Routine::new("cusReInitDsoEffect", 0x0853_8c5c);
pub const CUS_RESTORE_CUSTOM_ELEM: Routine = Routine::new("cusRestoreCustomElem", 0x0853_8a5c);
pub const CUS_EFFECT_MAPPING_CLEAR: Routine = Routine::new("cusEffectMappingClear", 0x0853_abb4);

/// The model records of the dictionary `dict` that are kind-0xd icons (record kind +0x46 is 1, model +4 not
/// null, model +0x2ec is 0xd), handed to `f` in order.
fn each_custom_icon(m: &mut Mem, o: &mut dyn Outside, dict: u32, f: Routine) {
    let cls = m.u32(SI_CID_MODELS).wrapping_mul(0x40).wrapping_add(m.u32(dict.wrapping_add(4)));
    let mut i = 0i32;
    while i < m.i32(cls.wrapping_add(4)) {
        let rec = (i as u32).wrapping_mul(0x84).wrapping_add(m.u32(cls.wrapping_add(8)));
        if m.u16(rec.wrapping_add(0x46)) as i16 == 1 {
            let model = m.u32(rec.wrapping_add(4));
            if model != 0 && m.u32(model.wrapping_add(0x2ec)) == 0xd {
                o.call(m, f, &[model]);
            }
        }
        i += 1;
    }
}

/// [soft 0x08538ee0 SI_cusRestoreCustomEffects]: after a scene is read, brings the custom effects back: the
/// effect of every custom icon of the model class is restored (cusRestoreDsoEffect), the automatic upgrade
/// choice and the effect mappings are reset, every value of kind 20000 in the sets of every record of the
/// dictionary has its two element references re-pointed (cusRestoreCustomElem for the input side, then the
/// output side; a failure of either shows "Error restoring <icon or effect name> custom icon / effect"),
/// and the effects are initialised again (cusReInitDsoEffect). Always 1.
pub fn si_cus_restore_custom_effects(m: &mut Mem, o: &mut dyn Outside, dict: u32) -> u32 {
    each_custom_icon(m, o, dict, CUS_RESTORE_DSO_EFFECT);
    m.set_u32(0x086e_0240, 0);
    o.call(m, CUS_EFFECT_MAPPING_CLEAR, &[]);
    let dicts = |m: &Mem| {
        let pd = m.u32(SI_PROC_DATA);
        m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
    };
    let mut c = 0i32;
    while c < m.i32(dicts(m)) {
        let cls = (c as u32).wrapping_mul(0x40).wrapping_add(m.u32(dict.wrapping_add(4)));
        let mut r = 0i32;
        while r < m.i32(cls.wrapping_add(4)) {
            let rec = m.u32(cls.wrapping_add(8)).wrapping_add((r as u32).wrapping_mul(0x84));
            if m.u16(rec.wrapping_add(0x46)) as i16 == 1 {
                let mut s = 0i32;
                while s < m.i32(rec.wrapping_add(0x60)) {
                    let set = m.u32(rec.wrapping_add(100)).wrapping_add((s as u32).wrapping_mul(0x18));
                    let mut k = 0i32;
                    while k < m.i32(set) {
                        let item = (k as u32).wrapping_mul(0x20).wrapping_add(m.u32(set.wrapping_add(12)));
                        let p4 = m.u32(item.wrapping_add(24));
                        if m.u32(item) == 20000 && p4 != 0 {
                            let id = m.u32(p4);
                            let d = dicts(m);
                            let a = m.u16(item.wrapping_add(16)) as i16 as i32 as u32;
                            let mut ok = o.call(m, CUS_RESTORE_CUSTOM_ELEM, &[id, 1, m.u32(p4.wrapping_add(12)), m.u32(p4.wrapping_add(28)), d, a, m.u32(item.wrapping_add(4)), m.u32(p4.wrapping_add(20)), 0]).eax as u16 != 0;
                            if ok {
                                let d = dicts(m);
                                ok = o.call(m, CUS_RESTORE_CUSTOM_ELEM, &[id, 0, m.u32(p4.wrapping_add(8)), m.u32(p4.wrapping_add(24)), d, c as u32, r as u32, m.u32(p4.wrapping_add(16)), 0]).eax as u16 != 0;
                            }
                            if !ok {
                                let mut effect_msg = m.i32(p4) < 0;
                                if !effect_msg {
                                    let d = dicts(m);
                                    let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, m.u32(SI_CID_MODELS), id]).eax;
                                    if e == 0 || m.u32(e.wrapping_add(4)) == 0 {
                                        effect_msg = true;
                                    } else {
                                        let name = m.u32(m.u32(e.wrapping_add(4)).wrapping_add(4));
                                        restore_complaint(m, o, 0x086e_0940, name, 0x086e_0933, 3);
                                    }
                                }
                                if effect_msg {
                                    let name = m.u32(p4.wrapping_add(32));
                                    restore_complaint(m, o, 0x086e_0960, name, 0x086e_0951, 3);
                                }
                            }
                        }
                        k += 1;
                    }
                    s += 1;
                }
            }
            r += 1;
        }
        c += 1;
    }
    each_custom_icon(m, o, dict, CUS_RE_INIT_DSO_EFFECT);
    1
}

pub const CUS_STATUS_CREATE_RELATIONS: Routine = Routine::new("cusStatusCreateRelations", 0x0853_6914);
pub const SI_CUS_CREATE_RELATIONS: Routine = Routine::new("SI_cusCreateRelations", 0x0853_6828);
pub const CUS_STATUS_ADD_USER_DATA: Routine = Routine::new("cusStatusAddUserData", 0x0853_5fbc);

/// [soft 0x08536828 SI_cusCreateRelations]: the relations of the effect of the custom icon `model` (kind
/// 0xd, else "Hey! Not a custom icon!" on stderr and -1): 2 when the icon already has its effect, -1 when it
/// has no saved header, else the effect the header names (a version 2 header: of the status named at +0x38,
/// by the name at +8; older: by name alone) is given to cusStatusCreateRelations, whose answer this is; -1
/// and a complaint "Hey! Custom effect %s is not present!" when there is no such effect.
pub fn si_cus_create_relations(m: &mut Mem, o: &mut dyn Outside, model: u32, flag: u32, out: u32) -> u32 {
    let err = m.u32(STDERR);
    if model == 0 || m.u32(model.wrapping_add(0x2ec)) != 0xd {
        fprintf(m, o, err, 0x086e_0317, &[]);
        return 0xffff_ffff;
    }
    let loc = m.local(8);
    let (st, node) = (loc, loc + 4);
    let r = 'body: {
        if o.call(m, SI_CUS_GET_EFFECT_BY_ICON, &[model, st]).eax != 0 {
            break 'body 2;
        }
        let hdr = o.call(m, CUS_ICON_GET_SAVED_HEADER, &[model, node]).eax;
        if hdr == 0 {
            break 'body 0xffff_ffff;
        }
        let eff;
        if m.i32(hdr) == 2 {
            let s = o.call(m, SI_CUS_GET_STATUS_BY_NAME, &[hdr.wrapping_add(0x38)]).eax;
            m.set_u32(st, s);
            eff = o.call(m, SI_CUS_STATUS_GET_EFFECT_BY_NAME, &[s, hdr.wrapping_add(8)]).eax;
        } else {
            eff = o.call(m, SI_CUS_GET_EFFECT_BY_NAME, &[hdr.wrapping_add(8), st]).eax;
        }
        if eff != 0 {
            let s = m.u32(st);
            break 'body o.call(m, CUS_STATUS_CREATE_RELATIONS, &[s, eff, hdr, model, flag, out]).eax;
        }
        fprintf(m, o, err, 0x086e_0340, &[hdr.wrapping_add(8)]);
        0xffff_ffff
    };
    m.release(8);
    r
}

/// [soft 0x08535ec0 SI_cusSceneRead]: after a scene is read, makes the relations of every custom icon (kind
/// 0xd) among the model nodes (kind 2 or 3) of the current graphic list (SI_cusCreateRelations with no
/// flag and no output).
pub fn si_cus_scene_read(m: &mut Mem, o: &mut dyn Outside) {
    let cell = m.local(4);
    m.set_u32(cell, 0);
    loop {
        let gl = crate::custom_util::graf_list(m);
        o.call(m, crate::custom_util::SI_NEXT_GRAFNODE, &[gl, cell]);
        let node = m.u32(cell);
        if node == 0 {
            break;
        }
        if m.u32(node).wrapping_sub(2) < 2 {
            let model = m.u32(node.wrapping_add(4));
            if m.u32(model.wrapping_add(0x2ec)) == 0xd {
                o.call(m, SI_CUS_CREATE_RELATIONS, &[model, 0, 0]);
            }
        }
    }
    m.release(4);
}

/// [soft 0x08535f30 SI_cusAddUserData]: saves the custom icon `model` as user data into the first of the
/// status records motion, actor, model, matter, tools that takes it (cusStatusAddUserData answering 0 or
/// more); shows "Inactive custom icon encountered..." when none does.
pub fn si_cus_add_user_data(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    use crate::custom_util::{CUS_ACTOR, CUS_MATTER, CUS_MODEL, CUS_MOTION, CUS_TOOLS};
    for st in [CUS_MOTION, CUS_ACTOR, CUS_MODEL, CUS_MATTER, CUS_TOOLS] {
        if (o.call(m, CUS_STATUS_ADD_USER_DATA, &[st, model]).eax as i32) >= 0 {
            return;
        }
    }
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086e_0260, 0]);
}

/// The growth step the writers share: when `cap` kilobytes do not hold `need` bytes, `cap` becomes the
/// number of kilobytes that do (rounded up, a negative `need` the way the original's division rounds) and
/// the buffer is reallocated to that size; a null result clears `ok`.
fn grow(m: &mut Mem, buf: &mut u32, cap: &mut i32, need: i32, ok: &mut bool) {
    if need <= cap.wrapping_mul(0x400) {
        return;
    }
    let mut k = if need < 0 { need.wrapping_add(0x3ff) } else { need };
    k >>= 10;
    if need.wrapping_sub(k.wrapping_mul(0x400)) > 0 {
        k = k.wrapping_add(1);
    }
    *cap = k;
    *buf = m.si_realloc(*buf, (k as u32).wrapping_shl(10));
    if *buf == 0 {
        *ok = false;
    }
}

/// [soft 0x0853920c cusDsoFcvkeyBinaryWrite]: appends the function-curve key `key` in the binary form
/// [`cus_dso_fcvkey_binary_read`] reads to the growing block `*bufp` (`*cap` kilobytes, `*pos` bytes
/// used; the block grows by SI_realloc to the next kilobyte that holds the write): the (token, value)
/// pairs 1 time (+0), 2 value (+4), 3 (+0x18), 4 (+8) as 32-bit words, 5 (+0xe) and 6 (+0xc) as 16-bit
/// words, 8 (+0x1e) only when the type word (+0x1c) is 2, 7 the type word, 9 (+0x20) unless it is -1,
/// and the closing 0 token. A growth that returns null stops the pairs (the stores through the null block
/// are made all the same). 0 for a null key, else 1 with the three variables stored back.
pub fn cus_dso_fcvkey_binary_write(m: &mut Mem, key: u32, pos: u32, cap: u32, bufp: u32) -> u32 {
    if key == 0 {
        return 0;
    }
    let mut buf = m.u32(bufp);
    let base = m.i32(pos);
    let mut cp = m.i32(cap);
    let mut ok = true;
    let mut c = base;
    // the pairs 1..6: the token, then the value
    let pairs: [(u16, u32, u32); 6] = [(1, 0, 4), (2, 4, 4), (3, 0x18, 4), (4, 8, 4), (5, 0xe, 2), (6, 0xc, 2)];
    for &(t, off, size) in pairs.iter() {
        let need = c.wrapping_add(2);
        grow(m, &mut buf, &mut cp, need, &mut ok);
        m.set_u16(buf.wrapping_add(c as u32), t);
        c = need;
        if !ok {
            break;
        }
        let need = c.wrapping_add(size as i32);
        let at = c;
        c = need;
        grow(m, &mut buf, &mut cp, need, &mut ok);
        if size == 4 {
            let v = m.u32(key.wrapping_add(off));
            m.set_u32(buf.wrapping_add(at as u32), v);
        } else {
            let v = m.u16(key.wrapping_add(off));
            m.set_u16(buf.wrapping_add(at as u32), v);
        }
        if !ok {
            break;
        }
    }
    let ty = m.i16(key.wrapping_add(0x1c));
    let mut e: i32;
    // after the pairs: token 8 (only for a type word of 2), then token 7, each with a 16-bit value
    let mut at7 = ty != 2;
    e = c;
    if ty == 2 {
        if ok {
            e = c.wrapping_add(2);
            grow(m, &mut buf, &mut cp, e, &mut ok);
            m.set_u16(buf.wrapping_add(c as u32), 8);
            if ok {
                let e4 = c.wrapping_add(4);
                grow(m, &mut buf, &mut cp, e4, &mut ok);
                let v = m.u16(key.wrapping_add(0x1e));
                m.set_u16(buf.wrapping_add(e as u32), v);
                c = e4;
                at7 = true;
            }
        }
    }
    if at7 {
        e = c;
        if ok {
            e = c.wrapping_add(2);
            grow(m, &mut buf, &mut cp, e, &mut ok);
            m.set_u16(buf.wrapping_add(c as u32), 7);
            if ok {
                let e4 = c.wrapping_add(4);
                grow(m, &mut buf, &mut cp, e4, &mut ok);
                let v = m.u16(key.wrapping_add(0x1c));
                m.set_u16(buf.wrapping_add(e as u32), v);
                e = e4;
            }
        }
    }
    c = e;
    let mut done = false;
    if m.i16(key.wrapping_add(0x20)) != -1 {
        e = c;
        if !ok {
            done = true;
        } else {
            e = c.wrapping_add(2);
            ok = true;
            grow(m, &mut buf, &mut cp, e, &mut ok);
            m.set_u16(buf.wrapping_add(c as u32), 9);
            if !ok {
                done = true;
            } else {
                let e4 = c.wrapping_add(4);
                ok = true;
                grow(m, &mut buf, &mut cp, e4, &mut ok);
                let v = m.u16(key.wrapping_add(0x20));
                m.set_u16(buf.wrapping_add(e as u32), v);
                c = e4;
            }
        }
    }
    if !done {
        e = c;
        if ok {
            e = c.wrapping_add(2);
            grow(m, &mut buf, &mut cp, e, &mut ok);
            m.set_u16(buf.wrapping_add(c as u32), 0);
        }
    }
    c = e;
    m.set_u32(bufp, buf);
    m.set_u32(pos, c as u32);
    m.set_u32(cap, cp as u32);
    1
}

/// `SI_versionOutput` (0x086793f4), a float.
const SI_VERSION_OUTPUT: u32 = 0x0867_93f4;

/// [soft 0x08539a50 cusDsoFcurveBinaryWrite]: writes the function curve `fc` in the binary form
/// [`cus_dso_fcurve_binary_read`] reads: a block of 1 kilobyte from SI_malloc, the tokens 1 (16-bit word
/// +0), the key count (token 2, as a 16-bit word, when `SI_versionOutput` is at most 2.62 or not a number;
/// token 8, a 32-bit word, otherwise) from +0xc, 3..6 the 16-bit words +0x18, +0x1a, +0x1c, +0x1e, the
/// key-array marker 7, the keys one after the other (cusDsoFcvkeyBinaryWrite; a key that fails ends the
/// block there, without the closing word), and the closing 0 token. The block is cut to its size
/// (SI_realloc) and stored to `*bufp`, the size to `*len`. 0 for a null curve, no memory, or a cut that fails.
pub fn cus_dso_fcurve_binary_write(m: &mut Mem, fc: u32, len: u32, bufp: u32) -> u32 {
    if fc == 0 {
        return 0;
    }
    let first = m.si_malloc(0x400);
    if first == 0 {
        return 0;
    }
    let cells = m.local(12);
    let (cbuf, ccap, cpos) = (cells, cells + 4, cells + 8);
    m.set_u32(cbuf, first);
    let mut buf = first;
    m.set_u16(buf, 1);
    let v = m.u16(fc);
    m.set_u16(buf.wrapping_add(2), v);
    let ver = f32::from_bits(m.u32(SI_VERSION_OUTPUT)) as f64;
    let mut pos: i32;
    if !(ver > 2.62) {
        m.set_u16(buf.wrapping_add(4), 2);
        let v = m.u16(fc.wrapping_add(0xc));
        m.set_u16(buf.wrapping_add(6), v);
        pos = 8;
    } else {
        m.set_u16(buf.wrapping_add(4), 8);
        let v = m.u32(fc.wrapping_add(0xc));
        m.set_u32(buf.wrapping_add(6), v);
        pos = 10;
    }
    // the header never outgrows the first kilobyte
    for (t, off) in [(3u16, 0x18u32), (4, 0x1a), (5, 0x1c), (6, 0x1e)] {
        m.set_u16(buf.wrapping_add(pos as u32), t);
        let v = m.u16(fc.wrapping_add(off));
        m.set_u16(buf.wrapping_add(pos as u32 + 2), v);
        pos += 4;
    }
    m.set_u16(buf.wrapping_add(pos as u32), 7);
    pos += 2;
    m.set_u32(ccap, 1);
    m.set_u32(cpos, pos as u32);
    let mut done = false;
    let mut i = 0i32;
    if 0 < m.i32(fc.wrapping_add(0xc)) {
        loop {
            let key = (i as u32).wrapping_mul(0x24).wrapping_add(m.u32(fc.wrapping_add(8)));
            let r = cus_dso_fcvkey_binary_write(m, key, cpos, ccap, cbuf) as u16;
            i += 1;
            if r != 1 {
                done = true;
                break;
            }
            if i >= m.i32(fc.wrapping_add(0xc)) {
                break;
            }
        }
    }
    buf = m.u32(cbuf);
    pos = m.i32(cpos);
    if !done {
        let mut cap = m.i32(ccap);
        let mut ok = true;
        grow(m, &mut buf, &mut cap, pos.wrapping_add(2), &mut ok);
        m.set_u32(cbuf, buf);
        m.set_u32(ccap, cap as u32);
        m.set_u16(buf.wrapping_add(pos as u32), 0);
        pos = pos.wrapping_add(2);
    }
    let cut = m.si_realloc(buf, pos as u32);
    m.set_u32(bufp, cut);
    let r = if cut != 0 {
        m.set_u32(len, pos as u32);
        1
    } else {
        0
    };
    m.release(12);
    r
}

/// [soft 0x08537614 cusStoreDsoFcurves]: packs the function curves of the effect list `list` (chain at
/// +0x64, next at +0x44) into one block, the way [`cus_restore_dso_fcurves`] reads it back: for each entry
/// with at least one of its four curve slots (+0x2c..) filled, its name with the closing NUL, a 16-bit
/// number of filled slots and the filled curves in binary form (cusDsoFcurveBinaryWrite). The block grows
/// by whole kilobytes (SI_realloc), is cut to its size and stored to `*bufp`, the size to `*len`. 0 for a
/// null argument, a growth or a curve that fails (`*len` and `*bufp` have then been cleared), else 1.
pub fn cus_store_dso_fcurves(m: &mut Mem, o: &mut dyn Outside, list: u32, len: u32, bufp: u32) -> u32 {
    let _ = &o;
    if list == 0 || len == 0 || bufp == 0 {
        return 0;
    }
    m.set_u32(len, 0);
    m.set_u32(bufp, 0);
    let mut buf = m.si_malloc(0x400);
    if buf == 0 {
        return 0;
    }
    let cells = m.local(8);
    let (cl, cb) = (cells, cells + 4);
    m.set_u32(cl, 0);
    m.set_u32(cb, 0);
    let mut cap: i32 = 1;
    let mut used: u32 = 0;
    let mut e = m.u32(list.wrapping_add(100));
    let r = 'body: {
        while e != 0 {
            let mut n: i16 = 0;
            for i in 0..4u32 {
                if m.u32(e.wrapping_add(0x2c + 4 * i)) != 0 {
                    n = n.wrapping_add(1);
                }
            }
            if 0 < n {
                let name = m.u32(e);
                let nl = clib::strlen(m, name).wrapping_add(1);
                let mut ok = true;
                grow(m, &mut buf, &mut cap, (used.wrapping_add(nl)) as i32, &mut ok);
                if !ok {
                    break 'body 0;
                }
                clib::memcpy(m, buf.wrapping_add(used), name, nl);
                let at = used.wrapping_add(nl);
                ok = true;
                grow(m, &mut buf, &mut cap, (at.wrapping_add(2)) as i32, &mut ok);
                if !ok {
                    break 'body 0;
                }
                m.set_u16(buf.wrapping_add(at), n as u16);
                used = at.wrapping_add(2);
                for i in 0..4u32 {
                    let slot = m.u32(e.wrapping_add(0x2c + 4 * i));
                    if slot != 0 {
                        let r = cus_dso_fcurve_binary_write(m, slot, cl, cb) as u16;
                        if r == 0 {
                            break 'body 0;
                        }
                        let sz = m.u32(cl);
                        ok = true;
                        grow(m, &mut buf, &mut cap, used.wrapping_add(sz) as i32, &mut ok);
                        if !ok {
                            break 'body 0;
                        }
                        let src = m.u32(cb);
                        clib::memcpy(m, buf.wrapping_add(used), src, sz);
                        used = used.wrapping_add(sz);
                        if src != 0 {
                            m.free(src);
                        }
                        m.set_u32(cb, 0);
                        m.set_u32(cl, 0);
                    }
                }
            }
            e = m.u32(e.wrapping_add(0x44));
        }
        let cut = m.si_realloc(buf, used);
        m.set_u32(bufp, cut);
        if cut != 0 {
            m.set_u32(len, used);
            1
        } else {
            0
        }
    };
    m.release(8);
    r
}

/// How a long item is written (the three formats: the name, the length, the length's frame): " NAME", then the length of the data in digits, then the data, the whole
/// padded to the next multiple of 40 bytes. Writes the item at `cur` (name `name` of `nl` bytes, data written
/// by `data` after the length) and returns where the next item starts.
fn write_long_item(m: &mut Mem, fmts: [u32; 3], cur: u32, name: u32, nl: u32, len: u32, data: impl FnOnce(&mut Mem, u32) -> u32) -> u32 {
    cfmt::sprintf(m, cur, fmts[0], &[name]);
    let at = cur.wrapping_add(nl).wrapping_add(1);
    let tmp = m.local(0x14);
    cfmt::sprintf(m, tmp, fmts[1], &[len]);
    cfmt::sprintf(m, at, fmts[2], &[tmp]);
    let digits = clib::strlen(m, tmp);
    let body = at.wrapping_add(digits).wrapping_add(2);
    let n = data(m, body);
    m.release(0x14);
    let span = body.wrapping_add(n).wrapping_sub(cur) as i32;
    let q = ((span as i64) / 0x28) as i32;
    cur.wrapping_add(((q.wrapping_mul(5).wrapping_add(5)).wrapping_mul(8)) as u32)
}

/// [soft 0x08537b98 cusDsoDialogWriteDataToString]: writes the values of the dialog elements of the chain
/// `list` (next at +0x44, kind +4) as text at `*outp`, 40 bytes for each: "NAME text" (kind 0; a text that
/// does not fit the 40 bytes is written long: " NAME", its length, " " and the text, taking a multiple of
/// 40 bytes), "NAME float" (1, from +0x10), "NAME int" (2, +0x14), "NAME short" (3, +0xc) and "NAME f f f f"
/// (4, +0x1c..; long when the length computeRGBABufferLength gives does not fit). An unknown kind is
/// skipped. 1 with `*outp` moved on past the written text; 0 when the four floats of a kind-4 element are
/// not available (`*outp` is then left alone). The text of a kind-0 element is freed when it is not the
/// element's own after the writing.
pub fn cus_dso_dialog_write_data_to_string(m: &mut Mem, list: u32, outp: u32) -> u32 {
    let mut cur = m.u32(outp);
    let mut e = list;
    while e != 0 {
        let kind = m.i16(e.wrapping_add(4)) as i32;
        let name = m.u32(e);
        match kind as u32 {
            0 => {
                let text = m.u32(e.wrapping_add(0x18));
                let nl = clib::strlen(m, name);
                let tl = clib::strlen(m, text);
                if (nl.wrapping_add(tl).wrapping_add(2) as i32) < 0x29 {
                    cfmt::sprintf(m, cur, 0x086e_04ed, &[name, text]);
                    cur = cur.wrapping_add(0x28);
                } else {
                    cur = write_long_item(m, [0x086e_04f3, 0x086e_04f7, 0x086e_04fa], cur, name, nl, tl, |m, body| {
                        cfmt::sprintf(m, body, 0x086e_04ff, &[text]);
                        tl
                    });
                }
                if text != m.u32(e.wrapping_add(0x18)) && text != 0 {
                    m.free(text);
                }
            }
            1 => {
                let b = (f32::from_bits(m.u32(e.wrapping_add(0x10))) as f64).to_bits();
                cfmt::sprintf(m, cur, 0x086e_0502, &[name, b as u32, (b >> 32) as u32]);
                cur = cur.wrapping_add(0x28);
            }
            2 => {
                let v = m.u32(e.wrapping_add(0x14));
                cfmt::sprintf(m, cur, 0x086e_0508, &[name, v]);
                cur = cur.wrapping_add(0x28);
            }
            3 => {
                let v = m.i16(e.wrapping_add(0xc)) as i32 as u32;
                cfmt::sprintf(m, cur, 0x086e_050e, &[name, v]);
                cur = cur.wrapping_add(0x28);
            }
            4 => {
                let l = compute_rgba_buffer_length(m, e.wrapping_add(0x1c));
                if l == -1 {
                    return 0;
                }
                let nl = clib::strlen(m, name);
                let mut args = vec![];
                for k in [0x1cu32, 0x20, 0x24, 0x28] {
                    let b = (f32::from_bits(m.u32(e.wrapping_add(k))) as f64).to_bits();
                    args.push(b as u32);
                    args.push((b >> 32) as u32);
                }
                if (nl.wrapping_add(l as u32).wrapping_add(1) as i32) < 0x29 {
                    let mut a = vec![name];
                    a.extend_from_slice(&args);
                    cfmt::sprintf(m, cur, 0x086e_0515, &a);
                    cur = cur.wrapping_add(0x28);
                } else {
                    cur = write_long_item(m, [0x086e_0524, 0x086e_0528, 0x086e_052b], cur, name, nl, l as u32, |m, body| {
                        cfmt::sprintf(m, body, 0x086e_0530, &args);
                        l as u32
                    });
                }
            }
            _ => {}
        }
        e = m.u32(e.wrapping_add(0x44));
    }
    m.set_u32(outp, cur);
    1
}

/// The entry kinds of a custom effect's data list that cusStatusAddUserData knows how to size and write
/// (the others are an "unknown command" message in the sizing pass): see [`add_user_data_known`].
fn add_user_data_known(kind: i16) -> bool {
    let k = kind as i32;
    matches!(k, 2 | 0x20 | 0x21 | 0x22)
        || matches!(k, 0xc | 0xe | 0xf | 0x10 | 0x14..=0x1b | 0x1d | 0x1f | 0x23..=0x30 | 0x37..=0x46 | 0x49 | 0x4b | 0x4c)
}

/// An effect whose data is kept in a DSO dialog: its dialog record (+0x104) is set and not zero.
fn effect_is_dso(m: &Mem, eff: u32) -> bool {
    if eff == 0 {
        return false;
    }
    let d = m.u32(eff.wrapping_add(0x104));
    d != 0 && m.u32(d) != 0
}

/// [soft 0x08535fbc cusStatusAddUserData]: stores the state of the custom effect `status` (a node of the
/// effect list at +0x14c, chained at +0xfc, model at +0xc) of `model` as the model's user data: "CUSA" (a 0xb0
/// byte header: 2, the effect's index, its name, the size of the text and of the float table, the status name),
/// "CUSB" (the dialog data as text, 40 bytes per entry, long values rounded up to a multiple of 40) and, when
/// the effect has curve data, "CUSC" (the first value of each curve, then the second, as floats) or, for a DSO
/// effect, "CUSD" (its function curves). The older user data of the four names is deleted first. -1 when the
/// model has no effect in the list, else 1.
pub fn cus_status_add_user_data(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32) -> i32 {
    let mut idx = 0u32;
    let mut eff = m.u32(m.u32(status.wrapping_add(0x14c)).wrapping_add(0xfc));
    if eff == 0 {
        return -1;
    }
    while m.u32(eff.wrapping_add(0xc)) != model {
        idx = idx.wrapping_add(1);
        eff = m.u32(eff.wrapping_add(0xfc));
        if eff == 0 {
            return -1;
        }
    }
    for name in [0x086e_0284u32, 0x086e_0289, 0x086e_028e, 0x086e_0293] {
        si_cus_model_dispose_user_data(m, model, name);
    }
    let hdr = m.malloc(0xb0);
    m.set_u32(hdr, 2);
    m.set_u32(hdr.wrapping_add(4), idx);
    let nm = m.u32(eff.wrapping_add(4));
    clib::strcpy(m, hdr.wrapping_add(8), nm);
    m.set_u32(hdr.wrapping_add(0x30), 0);
    m.set_u32(hdr.wrapping_add(0x34), 0);
    clib::strcpy(m, hdr.wrapping_add(0x38), status.wrapping_add(2));
    let dso = effect_is_dso(m, eff);
    let loc = m.local(0x10);
    let (count, cur_slot, fc_slot) = (loc, loc + 4, loc + 8);
    let (mut cnt, mut nf) = (0u32, 0i32);
    if dso {
        let list = m.u32(eff.wrapping_add(0x64));
        cus_dso_dialog_get_data_size(m, list, hdr.wrapping_add(0x30), count);
    } else {
        let mut e = m.u32(eff.wrapping_add(0xf8));
        while e != 0 {
            let kind = m.i16(e);
            if !add_user_data_known(kind) {
                fprintf(m, o, m.u32(STDERR), 0x086e_02a0, &[kind as i32 as u32]);
            } else if kind == 2 {
                let mut v = m.u32(eff.wrapping_add(0x64));
                while v != 0 {
                    let vk = m.i16(v.wrapping_add(4));
                    let size = m.u32(hdr.wrapping_add(0x30));
                    match vk {
                        1 => {
                            cnt += 1;
                            let t = m.u32(v.wrapping_add(0x2c));
                            if t != 0 {
                                nf = m.u32(t.wrapping_add(0xc)) as i32;
                            }
                            m.set_u32(hdr.wrapping_add(0x30), size.wrapping_add(0x28));
                        }
                        0 => {
                            let text = m.u32(v.wrapping_add(0x18));
                            let name = m.u32(v);
                            let nl = clib::strlen(m, name);
                            let tl = clib::strlen(m, text);
                            let mut size = size.wrapping_add(0x28);
                            if (nl.wrapping_add(tl).wrapping_add(2) as i32) > 0x28 {
                                let buf = m.local(0x14);
                                size = size.wrapping_add(data_size_step(m, buf, 0x086e_0298, tl, nl.wrapping_add(tl)));
                                m.release(0x14);
                            }
                            m.set_u32(hdr.wrapping_add(0x30), size);
                            if text != m.u32(v.wrapping_add(0x18)) && text != 0 {
                                m.free(text);
                            }
                        }
                        2 | 3 => m.set_u32(hdr.wrapping_add(0x30), size.wrapping_add(0x28)),
                        _ => {}
                    }
                    v = m.u32(v.wrapping_add(0x44));
                }
            } else if kind == 0x20 || kind == 0x21 {
                let s = m.u32(hdr.wrapping_add(0x30));
                m.set_u32(hdr.wrapping_add(0x30), s.wrapping_add(0xa0));
            } else if kind == 0x22 {
                let s = m.u32(hdr.wrapping_add(0x30));
                let n = m.u32(e.wrapping_add(0x20));
                m.set_u32(hdr.wrapping_add(0x30), s.wrapping_add(0x28).wrapping_add(n));
            }
            e = m.u32(e.wrapping_add(0x30));
        }
    }
    if nf > 0 {
        let n4 = (nf as u32).wrapping_mul(4);
        m.set_u32(hdr.wrapping_add(0x34), n4.wrapping_add(cnt.wrapping_mul(n4)));
    }
    let size = m.u32(hdr.wrapping_add(0x30)) as i32;
    let data = if size > 0 { m.malloc(size as u32) } else { 0 };
    for i in 0..size.max(0) as u32 {
        m.set_u8(data.wrapping_add(i), 0);
    }
    m.set_u32(cur_slot, data);
    if dso {
        let list = m.u32(eff.wrapping_add(0x64));
        cus_dso_dialog_write_data_to_string(m, list, cur_slot);
    } else {
        let mut cur = data;
        let mut e = m.u32(eff.wrapping_add(0xf8));
        while e != 0 {
            let kind = m.i16(e);
            if kind == 2 {
                let mut v = m.u32(eff.wrapping_add(0x64));
                while v != 0 {
                    let name = m.u32(v);
                    match m.i16(v.wrapping_add(4)) {
                        0 => {
                            let text = m.u32(v.wrapping_add(0x18));
                            let nl = clib::strlen(m, name);
                            let tl = clib::strlen(m, text);
                            if (nl.wrapping_add(tl).wrapping_add(2) as i32) <= 0x28 {
                                cfmt::sprintf(m, cur, 0x086e_02db, &[name, text]);
                                cur = cur.wrapping_add(0x28);
                            } else {
                                cur = write_long_item(m, [0x086e_02e1, 0x086e_02e5, 0x086e_02e8], cur, name, nl, tl, |m, body| {
                                    cfmt::sprintf(m, body, 0x086e_02ed, &[text]);
                                    tl
                                });
                            }
                            if text != m.u32(v.wrapping_add(0x18)) && text != 0 {
                                m.free(text);
                            }
                        }
                        1 => {
                            let b = (f32::from_bits(m.u32(v.wrapping_add(0x10))) as f64).to_bits();
                            cfmt::sprintf(m, cur, 0x086e_02f0, &[name, b as u32, (b >> 32) as u32]);
                            cur = cur.wrapping_add(0x28);
                        }
                        2 => {
                            let x = m.u32(v.wrapping_add(0x14));
                            cfmt::sprintf(m, cur, 0x086e_02f6, &[name, x]);
                            cur = cur.wrapping_add(0x28);
                        }
                        3 => {
                            let x = m.i16(v.wrapping_add(0xc)) as i32 as u32;
                            cfmt::sprintf(m, cur, 0x086e_02fc, &[name, x]);
                            cur = cur.wrapping_add(0x28);
                        }
                        _ => {}
                    }
                    v = m.u32(v.wrapping_add(0x44));
                }
            } else if kind == 0x20 || kind == 0x21 {
                let sel = m.u32(m.u32(e.wrapping_add(0x1c)).wrapping_add(4));
                cfmt::sprintf(m, cur, 0x086e_02c8, &[sel]);
                cur = cur.wrapping_add(0xa0);
            } else if kind == 0x22 {
                let n = m.u32(e.wrapping_add(0x20));
                cfmt::sprintf(m, cur, 0x086e_02d4, &[n]);
                cur = cur.wrapping_add(0x28);
                let src = m.u32(e.wrapping_add(0x24));
                for i in 0..n as i32 {
                    let b = m.u8(src.wrapping_add(i as u32));
                    m.set_u8(cur, b);
                    cur = cur.wrapping_add(1);
                }
            }
            e = m.u32(e.wrapping_add(0x30));
        }
    }
    let mut floats = 0u32;
    if !dso || m.u32(count) as i32 <= 0 {
        let sz = m.u32(hdr.wrapping_add(0x34));
        if (sz as i32) > 0 {
            floats = m.malloc(sz);
            let mut cur = floats;
            let mut v = m.u32(eff.wrapping_add(0x64));
            while v != 0 {
                if m.i16(v.wrapping_add(4)) == 1 {
                    let tbl = m.u32(m.u32(v.wrapping_add(0x2c)).wrapping_add(8));
                    let n = nf as u32;
                    if cur == floats {
                        for j in 0..n {
                            let f = through_x87(m.u32(tbl.wrapping_add(j.wrapping_mul(0x24))));
                            m.set_u32(cur.wrapping_add(j.wrapping_mul(4)), f);
                        }
                        cur = cur.wrapping_add(n.wrapping_mul(4));
                    }
                    for j in 0..n {
                        let f = through_x87(m.u32(tbl.wrapping_add(4).wrapping_add(j.wrapping_mul(0x24))));
                        m.set_u32(cur.wrapping_add(j.wrapping_mul(4)), f);
                    }
                    cur = cur.wrapping_add(n.wrapping_mul(4));
                }
                v = m.u32(v.wrapping_add(0x44));
            }
        }
    } else {
        cus_store_dso_fcurves(m, o, eff, hdr.wrapping_add(0x34), fc_slot);
    }
    si_cus_model_attach_user_data(m, model, 0x086e_0303, 0xb0, hdr);
    si_cus_model_attach_user_data(m, model, 0x086e_0308, m.u32(hdr.wrapping_add(0x30)), data);
    let sz34 = m.u32(hdr.wrapping_add(0x34));
    if (sz34 as i32) > 0 {
        if !dso {
            si_cus_model_attach_user_data(m, model, 0x086e_0312, sz34, floats);
        } else {
            let fc = m.u32(fc_slot);
            si_cus_model_attach_user_data(m, model, 0x086e_030d, sz34, fc);
        }
    }
    m.release(0x10);
    1
}
