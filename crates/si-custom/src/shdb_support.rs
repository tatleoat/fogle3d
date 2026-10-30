// SPDX-License-Identifier: MIT
//! `shdb_support.c`: the node tree and the entry point that parses a shader's text.
//!
//! A node is 0x1c bytes: +0 the type, +4 .. +0x10 four words (ints or floats), +0x14 the
//! first child, +0x18 the next sibling.

use crate::shdb_global::SI_SHDB_ROOT;
use crate::{fflush, fprintf};
use crate::shdb_lex::{si_shdb_get_error_status, si_shdb_init_lexyacc};
use crate::shdb_yacc::shdb_yyparse;
use si_mem::{clib, Mem, Outside};

pub const NODE_SIZE: u32 = 0x1c;
pub const CHILD: u32 = 0x14;
pub const NEXT: u32 = 0x18;

/// `"   "` appended to the text so the scanner meets a few blanks at its end.
const TAIL_BLANKS: u32 = 0x086e_56c4;
/// `" . "`.
const DOT: u32 = 0x086e_56c8;
/// `"%d %d (%f %f %f)\n"`.
const NODE_FORMAT: u32 = 0x086e_56cc;

/// [soft 0x0854f8c0 si_shdbParse]: parses `text`, the error count, the error strings and the
/// root go through the pointers that are not null. Answers the error status (1 for no text).
///
/// The original reads an uninitialised short when `nerrs` or `errs` is null; here it is 0.
pub fn si_shdb_parse(m: &mut Mem, o: &mut dyn Outside, text: u32, nerrs: u32, errs: u32, root: u32) -> i32 {
    if text == 0 || clib::strlen(m, text) == 0 {
        return 1;
    }
    let len = clib::strlen(m, text);
    let copy = m.si_calloc(len + 4, 1);
    if copy == 0 {
        return 0;
    }
    clib::strcpy(m, copy, text);
    clib::strcat(m, copy, TAIL_BLANKS);
    m.set_u32(SI_SHDB_ROOT, 0);
    si_shdb_init_lexyacc(m, copy);
    m.set_u32(crate::shdb_global::SHDB_YYDEBUG, 0);
    shdb_yyparse(m, o);
    let mut status = 0i16;
    if nerrs != 0 && errs != 0 {
        let local = m.local(4);
        si_shdb_get_error_status(m, local, nerrs, errs);
        status = m.i16(local);
        m.release(4);
    }
    if root != 0 {
        let r = m.u32(SI_SHDB_ROOT);
        m.set_u32(root, r);
    }
    status as i32
}

/// [soft 0x0854f9fc si_shdbAllocNode]: a zeroed node.
pub fn si_shdb_alloc_node(m: &mut Mem) -> u32 {
    m.si_calloc(1, NODE_SIZE)
}

/// [soft 0x0854fa14 si_shdbFreeNode]: fills the node with 0xff and frees it.
pub fn si_shdb_free_node(m: &mut Mem, node: u32) {
    if node != 0 {
        clib::memset(m, node, 0xff, NODE_SIZE);
        m.free(node);
    }
}

/// [soft 0x0854fa44 si_shdbFreeTree]: frees the node `*pp` points at; bit 1 of `flags` also
/// the children, bit 0 the siblings. Clears `*pp`.
pub fn si_shdb_free_tree(m: &mut Mem, pp: u32, flags: i32) {
    if pp == 0 {
        return;
    }
    let node = m.u32(pp);
    if node == 0 {
        return;
    }
    if flags & 2 != 0 {
        si_shdb_free_tree(m, node + CHILD, flags);
    }
    if flags & 1 != 0 {
        si_shdb_free_tree(m, node + NEXT, flags);
    }
    si_shdb_free_node(m, node);
    m.set_u32(pp, 0);
}

/// [soft 0x0854fab0 si_shdbFreeAllNodes]: frees the tree at `si_shdbRoot`.
pub fn si_shdb_free_all_nodes(m: &mut Mem) {
    si_shdb_free_tree(m, SI_SHDB_ROOT, 3);
    m.set_u32(SI_SHDB_ROOT, 0);
}

/// [soft 0x0854fad4 si_shdbAddNode]: puts `node` at the end of the chain that starts at `*pp`:
/// mode 1 follows the sibling link, mode 2 the child link; any other mode answers 0.
pub fn si_shdb_add_node(m: &mut Mem, pp: u32, node: u32, mode: i32) -> i32 {
    let link = match mode {
        1 => NEXT,
        2 => CHILD,
        _ => return 0,
    };
    let mut at = pp;
    while m.u32(at) != 0 {
        at = m.u32(at) + link;
    }
    m.set_u32(at, node);
    1
}

/// [soft 0x0854fb28 si_shdbPrintTree]: one line per node to the stream, children one level
/// deeper.
pub fn si_shdb_print_tree(m: &mut Mem, o: &mut dyn Outside, fp: u32, node: u32, depth: i32) {
    if node == 0 {
        return;
    }
    for _ in 0..depth.max(0) {
        fprintf(m, o, fp, DOT, &[]);
    }
    let mut args = vec![m.u32(node), m.u32(node + 4)];
    for off in [4u32, 8, 0xc] {
        let d = f64::from(f32::from_bits(m.u32(node + off)));
        let b = d.to_bits();
        args.push(b as u32);
        args.push((b >> 32) as u32);
    }
    fprintf(m, o, fp, NODE_FORMAT, &args);
    fflush(m, o, fp);
    let child = m.u32(node + CHILD);
    if child != 0 {
        si_shdb_print_tree(m, o, fp, child, depth + 1);
    }
    let next = m.u32(node + NEXT);
    if next != 0 {
        si_shdb_print_tree(m, o, fp, next, depth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shdb_yacc::add_node_integer;

    #[test]
    fn nodes_chain_and_free() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        crate::store_data(&mut m);
        let a = add_node_integer(&mut m, 5, 0, 7);
        let b = add_node_integer(&mut m, 6, 0, 8);
        let head = m.local(4);
        m.set_u32(head, a);
        assert_eq!(si_shdb_add_node(&mut m, head, b, 1), 1);
        assert_eq!(m.u32(a + NEXT), b);
        assert_eq!(si_shdb_add_node(&mut m, head, b, 9), 0);
        si_shdb_free_tree(&mut m, head, 3);
        assert_eq!(m.u32(head), 0);
    }
}
