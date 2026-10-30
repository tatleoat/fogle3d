// SPDX-License-Identifier: MIT
//! The dynamic-link files of the custom effects: `dlkCore.c` (the four `SI_dynalink*` calls over
//! `dlopen`/`dlsym`/`dlclose`), `dynsym.c` (a symbol: name, type, address), `dslist.c` (a singly
//! linked list of symbols) and `dynlink.c` (a library: path name, symbol list, handle); finding
//! 0041, "sixth continuation".
//!
//! Records: a symbol is 12 bytes (+0 name, +4 type, +8 address); a list node 8 bytes (+0 symbol,
//! +4 next); a link 12 bytes (+0 path name, +4 symbol list head, +8 handle). The C library's
//! `dlopen`, `dlsym`, `dlclose`, `dlerror` and the error reporter `SI_error` are outside calls.

use crate::fprintf;
use crate::STDERR;
use si_mem::{clib, Mem, Outside, Routine};

pub const DLOPEN: Routine = Routine::new("plt:dlopen", 0x080c_5188);
pub const DLSYM: Routine = Routine::new("plt:dlsym", 0x080c_3b48);
pub const DLCLOSE: Routine = Routine::new("plt:dlclose", 0x080c_4af8);
pub const DLERROR: Routine = Routine::new("plt:dlerror", 0x080c_4798);
pub const SI_ERROR: Routine = Routine::new("SI_error", 0x0814_17d0);

/// `handle.201`, the main program's `dlopen` handle (a function-local static).
const MAIN_HANDLE: u32 = 0x086f_c930;

const S_LOAD_HANDLE: u32 = 0x086f_c7e0;
const S_LOAD_NAME: u32 = 0x086f_c7fb;
const S_LOAD: u32 = 0x086f_c814;
const S_RESOLVE_ADDRESS: u32 = 0x086f_c840;
const S_RESOLVE_HANDLE: u32 = 0x086f_c880;
const S_RESOLVE_NAME: u32 = 0x086f_c8c0;
const S_RESOLVE: u32 = 0x086f_c8e2;
const S_UNLOAD_HANDLE: u32 = 0x086f_c8fe;
const S_UNLOAD: u32 = 0x086f_c91b;
const S_MAIN_ADDRESS: u32 = 0x086f_c940;
const S_MAIN_NAME: u32 = 0x086f_c980;
const S_MAIN: u32 = 0x086f_c9c0;
const S_LIST_BAD: u32 = 0x086f_ca00;
const S_LIST_BEGIN: u32 = 0x086f_ca26;
const S_LIST_END: u32 = 0x086f_ca40;
const S_REALLOC: u32 = 0x086f_ca80;
const S_LINK_BAD: u32 = 0x086f_caa0;
const S_LINK_AT: u32 = 0x086f_cac0;
const S_LINK_PATH: u32 = 0x086f_cae1;
const S_LINK_HANDLE: u32 = 0x086f_caf3;
const S_LINK_SYMBOLS: u32 = 0x086f_cb01;
const S_LINK_END: u32 = 0x086f_cb0f;
const S_SYM_BAD: u32 = 0x086f_cb40;
const S_SYM_AT: u32 = 0x086f_cb80;
const S_SYM_NAME: u32 = 0x086f_cba3;
const S_SYM_TYPE: u32 = 0x086f_cbb1;
const S_SYM_ADDRESS: u32 = 0x086f_cbbd;

fn error2(m: &mut Mem, o: &mut dyn Outside, code: u32, msg: u32) -> i32 {
    o.call(m, SI_ERROR, &[code, msg]).eax as i32
}

fn error3(m: &mut Mem, o: &mut dyn Outside, code: u32, msg: u32) -> i32 {
    let why = o.call(m, DLERROR, &[]).eax;
    o.call(m, SI_ERROR, &[code, msg, why]).eax as i32
}

/// [soft 0x085e6980 SI_dynalinkLoad]: `dlopen(name, 1)` into `*handle`; 0, or the error report
/// (0x10 for a null argument, 0x40 with the `dlerror` text).
pub fn si_dynalink_load(m: &mut Mem, o: &mut dyn Outside, name: u32, handle: u32) -> i32 {
    if handle == 0 {
        return error2(m, o, 0x10, S_LOAD_HANDLE);
    }
    m.set_u32(handle, 0);
    if name == 0 {
        return error2(m, o, 0x10, S_LOAD_NAME);
    }
    let h = o.call(m, DLOPEN, &[name, 1]).eax;
    m.set_u32(handle, h);
    if h != 0 {
        return 0;
    }
    error3(m, o, 0x40, S_LOAD)
}

/// [soft 0x085e69ec SI_dynalinkResolveSymbol]: `dlsym(handle, name)` into `*address`.
pub fn si_dynalink_resolve_symbol(m: &mut Mem, o: &mut dyn Outside, handle: u32, name: u32, address: u32) -> i32 {
    if address == 0 {
        return error2(m, o, 0x10, S_RESOLVE_ADDRESS);
    }
    m.set_u32(address, 0);
    if handle == 0 {
        return error2(m, o, 0x10, S_RESOLVE_HANDLE);
    }
    if name == 0 {
        return error2(m, o, 0x10, S_RESOLVE_NAME);
    }
    let a = o.call(m, DLSYM, &[handle, name]).eax;
    m.set_u32(address, a);
    if a != 0 {
        return 0;
    }
    error3(m, o, 0x41, S_RESOLVE)
}

/// [soft 0x085e6a6c SI_dynalinkUnload]: `dlclose(handle)`; 0, or error 0x42 with the `dlerror` text.
pub fn si_dynalink_unload(m: &mut Mem, o: &mut dyn Outside, handle: u32) -> i32 {
    if handle == 0 {
        return error2(m, o, 0x10, S_UNLOAD_HANDLE);
    }
    if o.call(m, DLCLOSE, &[handle]).eax == 0 {
        return 0;
    }
    error3(m, o, 0x42, S_UNLOAD)
}

/// [soft 0x085e6abc SI_dynalinkResolveMainProgramSymbol]: `dlsym` over the main program's handle
/// (`dlopen(NULL, 1)` the first time, kept in a static).
pub fn si_dynalink_resolve_main_program_symbol(m: &mut Mem, o: &mut dyn Outside, name: u32, address: u32) -> i32 {
    if address == 0 {
        return error2(m, o, 0x10, S_MAIN_ADDRESS);
    }
    m.set_u32(address, 0);
    if name == 0 {
        return error2(m, o, 0x10, S_MAIN_NAME);
    }
    if m.u32(MAIN_HANDLE) == 0 {
        let h = o.call(m, DLOPEN, &[0, 1]).eax;
        m.set_u32(MAIN_HANDLE, h);
    }
    let h = m.u32(MAIN_HANDLE);
    let a = o.call(m, DLSYM, &[h, name]).eax;
    m.set_u32(address, a);
    if a != 0 {
        return 0;
    }
    error3(m, o, 0x41, S_MAIN)
}

/// A copy of the C string `s` in a new block (`SI_malloc(strlen + 1)`, `strcpy`); 0 when out of memory.
fn copy_string(m: &mut Mem, s: u32) -> u32 {
    let p = m.si_malloc(clib::strlen(m, s).wrapping_add(1));
    if p != 0 {
        clib::strcpy(m, p, s);
    }
    p
}

/// [soft 0x085e70b0 SI_dynamicSymbolCreate]: a symbol with a copy of `name` (none for a null name),
/// the type, address 0. 0 when out of memory.
pub fn si_dynamic_symbol_create(m: &mut Mem, name: u32, ty: u32) -> u32 {
    let s = m.si_malloc(12);
    if s == 0 {
        return 0;
    }
    m.set_u32(s + 4, ty);
    m.set_u32(s + 8, 0);
    if name == 0 {
        m.set_u32(s, 0);
        return s;
    }
    let p = copy_string(m, name);
    m.set_u32(s, p);
    if p == 0 {
        m.free(s);
        return 0;
    }
    s
}

/// [soft 0x085e716c SI_dynamicSymbolDelete]: frees the name and the symbol `*pp` names, clears `*pp`.
pub fn si_dynamic_symbol_delete(m: &mut Mem, pp: u32) {
    let s = m.u32(pp);
    if s == 0 {
        return;
    }
    symbol_free(m, s);
    m.set_u32(pp, 0);
}

fn symbol_free(m: &mut Mem, s: u32) {
    let name = m.u32(s);
    if name != 0 {
        m.free(name);
        m.set_u32(s, 0);
    }
    m.free(s);
}

/// [soft 0x085e71b8 SI_dynamicSymbolGetName].
pub fn si_dynamic_symbol_get_name(m: &Mem, s: u32) -> u32 {
    m.u32(s)
}

/// [soft 0x085e71c4 SI_dynamicSymbolResolve]: the symbol's address looked up in the library `handle`.
pub fn si_dynamic_symbol_resolve(m: &mut Mem, o: &mut dyn Outside, handle: u32, s: u32) -> i32 {
    let name = m.u32(s);
    si_dynalink_resolve_symbol(m, o, handle, name, s + 8)
}

/// [soft 0x085e71e4 SI_dynamicSymbolSetAddress].
pub fn si_dynamic_symbol_set_address(m: &mut Mem, address: u32, s: u32) {
    m.set_u32(s + 8, address);
}

/// [soft 0x085e71f4 SI_dynamicSymbolDump]: the symbol's fields as text on `fp`.
pub fn si_dynamic_symbol_dump(m: &mut Mem, o: &mut dyn Outside, s: u32, fp: u32) {
    if fp == 0 || s == 0 {
        let err = m.u32(STDERR);
        fprintf(m, o, err, S_SYM_BAD, &[]);
        return;
    }
    fprintf(m, o, fp, S_SYM_AT, &[s]);
    let (name, ty, addr) = (m.u32(s), m.u32(s + 4), m.u32(s + 8));
    fprintf(m, o, fp, S_SYM_NAME, &[name]);
    fprintf(m, o, fp, S_SYM_TYPE, &[ty]);
    fprintf(m, o, fp, S_SYM_ADDRESS, &[addr]);
}

/// [soft 0x085e6ce0 dynamicSymbolListNodeCreate]: a node (symbol, no next); 0 when out of memory.
fn node_create(m: &mut Mem, s: u32) -> u32 {
    let n = m.si_malloc(8);
    if n == 0 {
        return 0;
    }
    m.set_u32(n + 4, 0);
    m.set_u32(n, s);
    n
}

/// [soft 0x085e6b50 SI_dynamicSymbolListInsert]: the symbol at the head of the list `*list`; 0 for a
/// null list or when out of memory.
pub fn si_dynamic_symbol_list_insert(m: &mut Mem, s: u32, list: u32) -> i32 {
    if list == 0 {
        return 0;
    }
    let n = node_create(m, s);
    if n == 0 {
        return 0;
    }
    let head = m.u32(list);
    m.set_u32(n + 4, head);
    m.set_u32(list, n);
    1
}

/// [soft 0x085e6b8c SI_dynamicSymbolListRemoveAll]: deletes every node and symbol, clears `*list`.
pub fn si_dynamic_symbol_list_remove_all(m: &mut Mem, list: u32) {
    if list == 0 {
        return;
    }
    let mut cur = m.u32(list);
    while cur != 0 {
        let next = m.u32(cur + 4);
        let sym = m.u32(cur);
        m.free(cur);
        if sym != 0 {
            symbol_free(m, sym);
        }
        cur = next;
    }
    m.set_u32(list, 0);
}

/// [soft 0x085e6d0c dynamicSymbolListNodeDelete]: `pp` points at a list node: frees the node, clears `*pp` and answers the node's
/// first word (the symbol it held, not freed); 0 when `pp` or `*pp` is null.
pub fn dynamic_symbol_list_node_delete(m: &mut Mem, pp: u32) -> u32 {
    if pp == 0 {
        return 0;
    }
    let node = m.u32(pp);
    if node == 0 {
        return 0;
    }
    let sym = m.u32(node);
    m.free(node);
    m.set_u32(pp, 0);
    sym
}

/// [soft 0x085e6be8 SI_dynamicSymbolListGetNumSymByType]: how many symbols of the list `*list` have the type.
pub fn si_dynamic_symbol_list_get_num_sym_by_type(m: &Mem, ty: u32, list: u32) -> u32 {
    let mut n = 0;
    let mut node = m.u32(list);
    while node != 0 {
        if m.u32(m.u32(node) + 4) == ty {
            n += 1;
        }
        node = m.u32(node + 4);
    }
    n
}

/// [soft 0x085e6c18 SI_dynamicSymbolListGetSymByType]: stores the symbols of the type, in list order,
/// into `out[0..n]`; 1 when exactly `n` were found, 0 on a null list or array, a null symbol, or
/// more than `n`.
pub fn si_dynamic_symbol_list_get_sym_by_type(m: &mut Mem, ty: u32, list: u32, n: u32, out: u32) -> i32 {
    if list == 0 || out == 0 {
        return 0;
    }
    let mut count: i32 = 0;
    let mut node = m.u32(list);
    while node != 0 {
        let sym = m.u32(node);
        if sym == 0 {
            return 0;
        }
        if m.u32(sym + 4) == ty {
            if count > (n as i32).wrapping_sub(1) {
                return 0;
            }
            m.set_u32(out.wrapping_add((count as u32).wrapping_mul(4)), sym);
            count += 1;
        }
        node = m.u32(node + 4);
    }
    (count == n as i32) as i32
}

/// [soft 0x085e6c70 SI_dynamicSymbolListDump]: "Dynamic Symbol List dump", every symbol, the end line, on `fp`.
pub fn si_dynamic_symbol_list_dump(m: &mut Mem, o: &mut dyn Outside, list: u32, fp: u32) {
    if fp == 0 {
        let err = m.u32(STDERR);
        fprintf(m, o, err, S_LIST_BAD, &[]);
        return;
    }
    fprintf(m, o, fp, S_LIST_BEGIN, &[]);
    let mut node = m.u32(list);
    while node != 0 {
        let sym = m.u32(node);
        si_dynamic_symbol_dump(m, o, sym, fp);
        node = m.u32(node + 4);
    }
    fprintf(m, o, fp, S_LIST_END, &[]);
}

/// [soft 0x085e6d50 SI_dynamicLinkCreate]: an empty link (path, symbols, handle all 0); 0 when out of memory.
pub fn si_dynamic_link_create(m: &mut Mem) -> u32 {
    let l = m.si_malloc(12);
    if l != 0 {
        m.set_u32(l + 8, 0);
        m.set_u32(l + 4, 0);
        m.set_u32(l, 0);
    }
    l
}

/// [soft 0x085e6d88 SI_dynamicLinkReAllocate]: grows the array of 12-byte links `*pp` to `n + 1` entries
/// (`SI_realloc`) and clears entry `n`; answers that entry, or 0 when out of memory.
pub fn si_dynamic_link_re_allocate(m: &mut Mem, o: &mut dyn Outside, n: u32, pp: u32) -> u32 {
    let old = m.u32(pp);
    let p = m.si_realloc(old, n.wrapping_mul(3).wrapping_add(3).wrapping_mul(4));
    m.set_u32(pp, p);
    if p == 0 {
        return 0;
    }
    let e = p.wrapping_add(n.wrapping_mul(12));
    if e == 0 {
        let err = m.u32(STDERR);
        fprintf(m, o, err, S_REALLOC, &[]);
        return 0;
    }
    m.set_u32(e + 8, 0);
    m.set_u32(e + 4, 0);
    m.set_u32(e, 0);
    e
}

/// [soft 0x085e6df4 SI_dynamicLinkDelete]: unloads the library, frees the name and the symbols and the
/// link `*pp` names, clears `*pp`.
pub fn si_dynamic_link_delete(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    let l = m.u32(pp);
    if l == 0 {
        return;
    }
    let h = m.u32(l + 8);
    if h != 0 {
        si_dynalink_unload(m, o, h);
    }
    let name = m.u32(l);
    if name != 0 {
        m.free(name);
        m.set_u32(l, 0);
    }
    if m.u32(l + 4) != 0 {
        si_dynamic_symbol_list_remove_all(m, l + 4);
    }
    m.free(l);
    m.set_u32(pp, 0);
}

/// [soft 0x085e6e6c SI_dynamicLinkSetName]: replaces the link's path name with a copy of `name` (none
/// for null). 1, or 0 for a null link or when out of memory.
pub fn si_dynamic_link_set_name(m: &mut Mem, name: u32, l: u32) -> i32 {
    if l == 0 {
        return 0;
    }
    let old = m.u32(l);
    if old != 0 {
        m.free(old);
        m.set_u32(l, 0);
    }
    if name == 0 {
        return 1;
    }
    let p = copy_string(m, name);
    m.set_u32(l, p);
    (p != 0) as i32
}

/// [soft 0x085e6f1c SI_dynamicLinkGetHandle].
pub fn si_dynamic_link_get_handle(m: &Mem, l: u32) -> u32 {
    if l == 0 { 0 } else { m.u32(l + 8) }
}

/// [soft 0x085e6f38 SI_dynamicLinkSetHandle]: 0, or 0x10 for a null link.
pub fn si_dynamic_link_set_handle(m: &mut Mem, handle: u32, l: u32) -> i32 {
    if l == 0 {
        return 0x10;
    }
    m.set_u32(l + 8, handle);
    0
}

/// [soft 0x085e6f5c SI_dynamicLinkLoad]: loads the library named by the link into its handle.
pub fn si_dynamic_link_load(m: &mut Mem, o: &mut dyn Outside, l: u32) -> i32 {
    if l == 0 {
        return 0x10;
    }
    let name = m.u32(l);
    si_dynalink_load(m, o, name, l + 8)
}

/// [soft 0x085e6f8c SI_dynamicLinkUnload].
pub fn si_dynamic_link_unload(m: &mut Mem, o: &mut dyn Outside, l: u32) -> i32 {
    if l == 0 {
        return 0x10;
    }
    let h = m.u32(l + 8);
    si_dynalink_unload(m, o, h)
}

/// [soft 0x085e6fb0 SI_dynamicLinkAddSymbol]: a new symbol (name, type) at the head of the link's list;
/// answers it, or 0 for a null link or when out of memory.
pub fn si_dynamic_link_add_symbol(m: &mut Mem, l: u32, name: u32, ty: u32) -> u32 {
    if l == 0 {
        return 0;
    }
    let s = si_dynamic_symbol_create(m, name, ty);
    if s == 0 {
        return 0;
    }
    if si_dynamic_symbol_list_insert(m, s, l + 4) != 0 {
        return s;
    }
    symbol_free(m, s);
    0
}

/// [soft 0x085e700c SI_dynamicLinkDump]: the link's fields and symbols as text on `fp`.
pub fn si_dynamic_link_dump(m: &mut Mem, o: &mut dyn Outside, l: u32, fp: u32) {
    if fp == 0 || l == 0 {
        let err = m.u32(STDERR);
        fprintf(m, o, err, S_LINK_BAD, &[]);
        return;
    }
    fprintf(m, o, fp, S_LINK_AT, &[l]);
    let (path, handle) = (m.u32(l), m.u32(l + 8));
    fprintf(m, o, fp, S_LINK_PATH, &[path]);
    fprintf(m, o, fp, S_LINK_HANDLE, &[handle]);
    fprintf(m, o, fp, S_LINK_SYMBOLS, &[]);
    si_dynamic_symbol_list_dump(m, o, l + 4, fp);
    fprintf(m, o, fp, S_LINK_END, &[]);
}

/// [soft 0x0852e7a0 SI_cusRunStandalone]: runs a custom effect as a stand-alone program (`soft -D library symbol args`):
/// loads the library named by `argv[0]`, looks up `symbol` in it, calls it as `symbol(argc, argv)` and unloads the
/// library. Answers the first failing step's status, or what the effect's function returned (the library is then
/// unloaded and the unload's own status dropped, as the original does).
/// `symbol` is the name's address, `argv` the address of the `char *` array.
pub fn si_cus_run_standalone(m: &mut Mem, o: &mut dyn Outside, symbol: u32, argc: i32, argv: u32) -> i32 {
    let handle = m.local(4);
    let func = m.local(4);
    let name = m.u32(argv);
    let mut r = si_dynalink_load(m, o, name, handle);
    if r == 0 {
        let h = m.u32(handle);
        r = si_dynalink_resolve_symbol(m, o, h, symbol, func);
        if r == 0 {
            let f = m.u32(func);
            // the effect's function: `int (*)(int argc, char **argv)`, reached through the resolved address
            r = o.call(m, Routine::at(f), &[argc as u32, argv]).eax as i32;
            let h = m.u32(handle);
            si_dynalink_unload(m, o, h);
        }
    }
    m.release(4);
    m.release(4);
    r
}
