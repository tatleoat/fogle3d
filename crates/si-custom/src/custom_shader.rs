// SPDX-License-Identifier: MIT
//! `custom_shader.c`: the table of loaded custom shaders and the list of their links.
//!
//! A shader record is 0x100 bytes (`SI_CustomShader`): +0 the chapter info, +4 a short (set
//! to 0 on allocation), +0xc the use count, +0x14 the active count, +0x18 a short "use
//! default" flag, +0x1c the name, +0x20 the second name, +0x24 a string, +0x28 the chain of
//! declared tokens, +0x2c a null-ended array of link tokens, +0x34 the dialog, +0xec a GUID,
//! +0xfc a word. A token is 0x20 bytes: +0 kind, +4 name, +8 a string, +0xc second name,
//! +0x10 a word, +0x14 the use count, +0x18 the active count, +0x1c the next link.
//! The shader table `shaderList`/`numShader` and the links list `linksList`/`lastlinkptr`
//! are statics of the file.

use crate::cfmt;
use si_mem::{clib, Mem, Outside, Routine};

pub const SHADER_LIST: u32 = 0x086e_21e0;
pub const NUM_SHADER: u32 = 0x086e_21e4;
pub const LINKS_LIST: u32 = 0x086e_21e8;
pub const LAST_LINK_PTR: u32 = 0x086e_21ec;
/// The all-zero GUID the shader's GUID is compared with.
pub const NULL_GUID: u32 = 0x086e_21f0;
/// `"%s"` with the quotes (two different literals, both `"%s"`).
const FMT_FUNC: u32 = 0x086e_28cb;
const FMT_GUID: u32 = 0x086e_28c6;

pub const SI_SHADER_GET_DIALOG: Routine = Routine::new("SI_shaderGetDialog", 0x0854_0718);
pub const SI_DIALOG_DISPOSE: Routine = Routine::new("SI_dialogDispose", 0x0838_f2f8);
pub const SI_GETMI2_OUTPUT: Routine = Routine::new("SI_getmi2Output", 0x0858_0cfc);
pub const SI_SPDL_USE_NAMESRV_SHD_REF: Routine = Routine::new("SI_spdlUseNamesrvShdRef", 0x0858_0e28);
pub const SI_STR_GUID_TO_STRING: Routine = Routine::new("SI_strGuidToString", 0x083c_fc78);

/// [soft 0x085400f0 SI_shaderAllocate]: a new zeroed shader record in the first free slot of the
/// table (the table is created with one slot, and grows by one slot when full). 0 when out of memory.
pub fn si_shader_allocate(m: &mut Mem) -> u32 {
    if m.i32(NUM_SHADER) == 0 {
        let l = m.si_calloc(1, 4);
        m.set_u32(SHADER_LIST, l);
        if l == 0 {
            return 0;
        }
        m.set_i32(NUM_SHADER, 1);
    }
    let mut i = 0i32;
    let n = m.i32(NUM_SHADER);
    if 0 < n {
        let list = m.u32(SHADER_LIST);
        let mut v = m.u32(list);
        while v != 0 {
            i += 1;
            if i >= n {
                break;
            }
            v = m.u32(list.wrapping_add((i as u32) << 2));
        }
    }
    if i == m.i32(NUM_SHADER) {
        let list = m.u32(SHADER_LIST);
        let l = m.si_realloc(list, (i as u32).wrapping_mul(4).wrapping_add(4));
        m.set_u32(SHADER_LIST, l);
        if l == 0 {
            return 0;
        }
        let n = m.u32(NUM_SHADER);
        m.set_u32(l.wrapping_add(n << 2), 0);
        m.set_u32(NUM_SHADER, n.wrapping_add(1));
    }
    let rec = m.si_calloc(1, 0x100);
    let list = m.u32(SHADER_LIST);
    m.set_u32(list.wrapping_add((i as u32) << 2), rec);
    if rec == 0 {
        return 0;
    }
    m.set_u32(rec + 0x14, 0);
    m.set_u32(rec + 0xc, 1);
    m.set_u16(rec + 4, 0);
    m.set_u32(rec + 0xfc, 0);
    rec
}

/// [soft 0x08543f18 linksListFind]: the first token of the links list named `name` (and, when
/// `name2` is given and the token has one, with that second name). 0 when none.
pub fn links_list_find(m: &mut Mem, name: u32, name2: u32) -> u32 {
    let mut t = m.u32(LINKS_LIST);
    if name != 0 {
        while t != 0 {
            let n = m.u32(t + 4);
            if n != 0 && clib::strcmp(m, n, name) == 0 {
                if name2 == 0 {
                    return t;
                }
                let n2 = m.u32(t + 0xc);
                if n2 == 0 {
                    return t;
                }
                if clib::strcmp(m, n2, name2) == 0 {
                    return t;
                }
            }
            t = m.u32(t + 0x1c);
        }
    }
    0
}

/// [soft 0x08543f88 linksListAdd]: appends the token to the links list with use count 1, or, when a
/// token of the same names is in it, counts one more use on that one (the new token is kept by
/// the caller). Answers 1.
pub fn links_list_add(m: &mut Mem, tok: u32) -> i32 {
    let found = links_list_find(m, m.u32(tok + 4), m.u32(tok + 0xc));
    if found == 0 {
        let last = m.u32(LAST_LINK_PTR);
        if last == 0 {
            m.set_u32(LINKS_LIST, tok);
        } else {
            m.set_u32(last + 0x1c, tok);
        }
        m.set_u32(tok + 0x1c, 0);
        m.set_u32(LAST_LINK_PTR, tok);
        m.set_u32(tok + 0x14, 1);
    } else {
        let c = m.u32(found + 0x14).wrapping_add(1);
        m.set_u32(found + 0x14, c);
    }
    1
}

/// [soft 0x08543fe0 linksListRemove]: one use less on the token; at zero it is unlinked from the
/// links list and freed with its name. Answers 1.
pub fn links_list_remove(m: &mut Mem, tok: u32) -> i32 {
    if tok != 0 {
        let c = m.i32(tok + 0x14);
        m.set_i32(tok + 0x14, c.wrapping_sub(1));
        if c == 1 {
            let name = m.u32(tok + 4);
            if name != 0 {
                m.free(name);
                m.set_u32(tok + 4, 0);
            }
            let head = m.u32(LINKS_LIST);
            if tok == head {
                let next = m.u32(tok + 0x1c);
                m.set_u32(LINKS_LIST, next);
                if tok == m.u32(LAST_LINK_PTR) {
                    m.set_u32(LAST_LINK_PTR, 0);
                }
            } else {
                // The original walks until it meets the token; a token not in the list runs off its end.
                let mut prev = head;
                let mut cur = m.u32(head + 0x1c);
                while cur != tok {
                    prev = m.u32(prev + 0x1c);
                    cur = m.u32(prev + 0x1c);
                }
                let next = m.u32(tok + 0x1c);
                m.set_u32(prev + 0x1c, next);
                if tok == m.u32(LAST_LINK_PTR) {
                    m.set_u32(LAST_LINK_PTR, prev);
                }
            }
            m.free(tok);
        }
    }
    1
}

/// [soft 0x085401d0 SI_shaderDispose]: one use less on the shader; at zero its table slot is
/// cleared and its names, token chain, link tokens and dialog are released, then the record.
/// Answers 1; 0 when the shader is not in the table.
pub fn si_shader_dispose(m: &mut Mem, o: &mut dyn Outside, sh: u32) -> i32 {
    if sh == 0 {
        return 1;
    }
    let uses = m.i32(sh + 0xc);
    if uses >= 1 {
        m.set_i32(sh + 0xc, uses.wrapping_sub(1));
        if uses.wrapping_sub(1) >= 1 {
            return 1;
        }
    }
    let n = m.i32(NUM_SHADER);
    let list = m.u32(SHADER_LIST);
    let mut i = 0i32;
    if 0 < n {
        let mut v = m.u32(list);
        while v != sh {
            i += 1;
            if i >= n {
                break;
            }
            v = m.u32(list.wrapping_add((i as u32) << 2));
        }
    }
    if i == m.i32(NUM_SHADER) {
        return 0;
    }
    m.set_u32(m.u32(SHADER_LIST).wrapping_add((i as u32) << 2), 0);
    for off in [0x1cu32, 0x24] {
        let p = m.u32(sh + off);
        if p != 0 {
            m.free(p);
            m.set_u32(sh + off, 0);
        }
    }
    let mut t = m.u32(sh + 0x28);
    while t != 0 {
        let name = m.u32(t + 4);
        if name != 0 {
            m.free(name);
            m.set_u32(t + 4, 0);
        }
        let next = m.u32(t + 0x1c);
        m.free(t);
        t = next;
    }
    let links = m.u32(sh + 0x2c);
    if links != 0 {
        let mut k = 0u32;
        while m.u32(links.wrapping_add(k << 2)) != 0 {
            let tok = m.u32(m.u32(sh + 0x2c).wrapping_add(k << 2));
            links_list_remove(m, tok);
            k += 1;
        }
    }
    let dlg = m.u32(sh + 0x34);
    if dlg != 0 {
        o.call(m, SI_DIALOG_DISPOSE, &[dlg]);
    }
    m.free(sh);
    1
}

/// [soft 0x085402fc SI_shaderGetName]: copies the shader's name into `buf` (cleared first,
/// `size` bytes), at most `size` bytes of it. Answers the count, 0 for a bad argument or no name.
pub fn si_shader_get_name(m: &mut Mem, sh: u32, buf: u32, size: i32) -> i32 {
    if sh == 0 || buf == 0 || size < 1 || m.u32(sh + 0x1c) == 0 {
        return 0;
    }
    clib::memset(m, buf, 0, size as u32);
    let src = m.u32(sh + 0x1c);
    let len = clib::strlen(m, src) as i32;
    let n = if size < len { size } else { len };
    clib::strncpy(m, buf, src, n as u32);
    n
}

/// [soft 0x08540398 SI_shaderUseDefault]: the shader's "use default" short; 0 for no shader.
pub fn si_shader_use_default(m: &Mem, sh: u32) -> i32 {
    if sh == 0 { 0 } else { m.i16(sh + 0x18) as i32 }
}

/// The preamble of the shader accessors: loads the dialog (through `SI_shaderGetDialog`) unless
/// the record has its chapter info and the short at +4 is 1.
fn ensure_loaded(m: &mut Mem, o: &mut dyn Outside, sh: u32) {
    if m.u32(sh) == 0 || m.i16(sh + 4) != 1 {
        o.call(m, SI_SHADER_GET_DIALOG, &[sh]);
    }
}

/// Copies `text` (already formatted) the way the func-name accessors do: length plus the NUL, cut
/// to `size`, with `strncpy` (no NUL when cut). Answers the count.
fn copy_counted(m: &mut Mem, buf: u32, text: u32, size: i32) -> i32 {
    let mut n = clib::strlen(m, text) as i32 + 1;
    if size < n {
        n = size;
    }
    clib::strncpy(m, buf, text, n as u32);
    n
}

/// [soft 0x08540510 shaderGetFuncName]: the quoted name of the first
/// kind-1 token of the shader's chain, copied into `buf`. Answers the count, 0 when there is none.
pub fn shader_get_func_name(m: &mut Mem, o: &mut dyn Outside, sh: u32, buf: u32, size: i32) -> i32 {
    if sh != 0 && buf != 0 && 0 < size {
        ensure_loaded(m, o, sh);
        if m.u32(sh) != 0 && m.u32(sh + 0x28) != 0 {
            clib::memset(m, buf, 0, size as u32);
            let mut t = m.u32(sh + 0x28);
            while t != 0 {
                if m.u32(t) == 1 {
                    let local = m.local(0x400);
                    let text = cfmt::format(m, FMT_FUNC, &[m.u32(t + 4)]);
                    m.put_cstr(local, &text);
                    let n = copy_counted(m, buf, local, size);
                    m.release(0x400);
                    return n;
                }
                t = m.u32(t + 0x1c);
            }
        }
    }
    0
}

/// [soft 0x085404f0 SI_shaderGetFuncDeclName]: `shaderGetFuncName`, the answer dropped.
pub fn si_shader_get_func_decl_name(m: &mut Mem, o: &mut dyn Outside, sh: u32, buf: u32, size: i32) {
    shader_get_func_name(m, o, sh, buf, size);
}

/// [soft 0x085403b0 SI_shaderGetFuncName]: with mental ray output on and name-server shader
/// references in use, and a GUID that is not all zero, the GUID's string in quotes; otherwise
/// `shaderGetFuncName`. The answer is the count copied, 0 for a bad argument or an unloaded shader.
pub fn si_shader_get_func_name(m: &mut Mem, o: &mut dyn Outside, sh: u32, buf: u32, size: i32) -> i32 {
    if sh != 0 && buf != 0 && 0 < size {
        ensure_loaded(m, o, sh);
        if m.u32(sh) != 0 && m.u32(sh + 0x28) != 0 {
            clib::memset(m, buf, 0, size as u32);
            let a = o.call(m, SI_GETMI2_OUTPUT, &[]).eax as u16;
            if a != 0 && o.call(m, SI_SPDL_USE_NAMESRV_SHD_REF, &[]).eax as u16 != 0 {
                if clib::memcmp(m, sh + 0xec, NULL_GUID, 16) != 0 {
                    let gs = m.local(44);
                    m.set_u8(gs, 0);
                    o.call(m, SI_STR_GUID_TO_STRING, &[gs, sh + 0xec]);
                    let local = m.local(0x400);
                    let text = cfmt::format(m, FMT_GUID, &[gs]);
                    m.put_cstr(local, &text);
                    let n = copy_counted(m, buf, local, size);
                    m.release(0x400);
                    m.release(44);
                    return n;
                }
            }
            return shader_get_func_name(m, o, sh, buf, size);
        }
    }
    0
}

/// [soft 0x085405fc SI_shaderSetActive]: with `on` (the low 16 bits equal to 1) one more active use on
/// the shader and on each of its link tokens, otherwise one less on each that is above 0. Answers 1;
/// 0 for no shader, no chapter info or no links.
pub fn si_shader_set_active(m: &mut Mem, o: &mut dyn Outside, sh: u32, on: u32) -> i32 {
    let on = on as u16 == 1;
    if sh == 0 {
        return 0;
    }
    ensure_loaded(m, o, sh);
    if m.u32(sh) == 0 || m.u32(sh + 0x2c) == 0 {
        return 0;
    }
    let bump = |m: &mut Mem, at: u32| {
        let v = m.i32(at);
        if on {
            m.set_i32(at, v.wrapping_add(1));
        } else if 0 < v {
            m.set_i32(at, v.wrapping_sub(1));
        }
    };
    bump(m, sh + 0x14);
    let links = m.u32(sh + 0x2c);
    let mut k = 0u32;
    let mut e = m.u32(links);
    while e != 0 {
        let tok = m.u32(links.wrapping_add(k << 2));
        bump(m, tok + 0x18);
        k += 1;
        e = m.u32(links.wrapping_add(k << 2));
    }
    1
}

/// [soft 0x0854068c SI_shaderResetActive]: the active count of every table shader and of every
/// token of the links list set to 0.
pub fn si_shader_reset_active(m: &mut Mem) {
    let n = m.i32(NUM_SHADER);
    let list = m.u32(SHADER_LIST);
    let mut i = 0i32;
    if 0 < n {
        while i < n {
            let s = m.u32(list.wrapping_add((i as u32) << 2));
            if s != 0 {
                m.set_u32(s + 0x14, 0);
            }
            i += 1;
        }
    }
    let mut t = m.u32(LINKS_LIST);
    while t != 0 {
        m.set_u32(t + 0x18, 0);
        t = m.u32(t + 0x1c);
    }
}

/// [soft 0x085406d4 SI_shaderIsActive]: the active count is above 0; false for no shader.
pub fn si_shader_is_active(m: &Mem, sh: u32) -> i32 {
    (sh != 0 && 0 < m.i32(sh + 0x14)) as i32
}

/// [soft 0x085406f8 SI_shaderGetType]: the kind word (+0xc) of the chapter info; 0 without one.
pub fn si_shader_get_type(m: &Mem, sh: u32) -> u32 {
    if sh == 0 || m.u32(sh) == 0 { 0 } else { m.u32(m.u32(sh) + 0xc) }
}

/// [soft 0x0854074c SI_shaderFind]: the table shader with this name (and second name; none given
/// matches a shader without one), with one more use. 0 when none.
pub fn si_shader_find(m: &mut Mem, name: u32, name2: u32) -> u32 {
    if name != 0 && m.i32(NUM_SHADER) != 0 {
        let mut i = 0i32;
        while i < m.i32(NUM_SHADER) {
            let s = m.u32(m.u32(SHADER_LIST).wrapping_add((i as u32) << 2));
            if s != 0 && m.u32(s + 0x1c) != 0 {
                let mut r = 1i32;
                let mut skip = false;
                if name2 == 0 {
                    r = m.u32(s + 0x20) as i32;
                } else if m.u32(s + 0x20) == 0 {
                    skip = true;
                } else {
                    r = clib::strcmp(m, m.u32(s + 0x20), name2);
                }
                if !skip && r == 0 {
                    let s2 = m.u32(m.u32(SHADER_LIST).wrapping_add((i as u32) << 2));
                    if clib::strcmp(m, m.u32(s2 + 0x1c), name) == 0 {
                        let c = m.u32(s2 + 0xc).wrapping_add(1);
                        m.set_u32(s2 + 0xc, c);
                        return s2;
                    }
                }
            }
            i += 1;
        }
    }
    0
}

/// [soft 0x085407f4 SI_shaderInitialize]: gives the record copies of the two names, no tokens and no
/// links, the chapter info, and clears the 0xb8 bytes from +0x30. Answers 1; 0 for a null shader,
/// name or chapter info.
pub fn si_shader_initialize(m: &mut Mem, sh: u32, name: u32, name2: u32, info: u32) -> i32 {
    if sh == 0 || name == 0 || info == 0 {
        return 0;
    }
    let a = clib::strdup(m, name);
    m.set_u32(sh + 0x1c, a);
    if name2 == 0 {
        m.set_u32(sh + 0x20, 0);
    } else {
        let b = clib::strdup(m, name2);
        m.set_u32(sh + 0x20, b);
    }
    m.set_u32(sh + 0x28, 0);
    m.set_u32(sh + 0x2c, 0);
    m.set_u32(sh, info);
    clib::memset(m, sh + 0x30, 0, 0xb8);
    1
}

/// [soft 0x08540880 SI_shaderCopy]: one more use on the shader, which comes back; null stays null.
pub fn si_shader_copy(m: &mut Mem, sh: u32) -> u32 {
    if sh != 0 {
        let c = m.u32(sh + 0xc).wrapping_add(1);
        m.set_u32(sh + 0xc, c);
    }
    sh
}

/// [soft 0x08543e8c cusAllocateToken]: a zeroed 0x20-byte token with the kind, copies of the name
/// (+4) and the string at +8 (or null), +0xc cleared, the word at +0x10 and the next link. 0 when out of memory.
pub fn cus_allocate_token(m: &mut Mem, kind: u32, name: u32, name2: u32, w10: u32, next: u32) -> u32 {
    let t = m.si_calloc(1, 0x20);
    if t != 0 {
        m.set_u32(t, kind);
        let a = if name == 0 { 0 } else { clib::strdup(m, name) };
        m.set_u32(t + 4, a);
        let b = if name2 == 0 { 0 } else { clib::strdup(m, name2) };
        m.set_u32(t + 8, b);
        m.set_u32(t + 0xc, 0);
        m.set_u32(t + 0x10, w10);
        m.set_u32(t + 0x1c, next);
    }
    t
}
