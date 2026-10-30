// SPDX-License-Identifier: MIT
//! `updlist.c`: the update list of a dictionary, on the image.
//!
//! A list (`SI_UpdList`, 12 bytes: count, nodes, dirty) is an array of 0x2c-byte nodes
//! grown by one node per addition; a node's arguments are an array of 0x14-byte records
//! grown the same way. Both layouts are in [`crate::records`].

use crate::globals::SI_CID;
use crate::records::{UpdArg, UpdNode};
use si_mem::{Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, Elem, UpdList};

/// [soft 0x081413e0 si_updlistAddArgument]: appends an argument made from a relation
/// entry to an argument array (null for none yet) and answers the array's new address;
/// `*count` is counted up. The two bytes at +0x0e of the new record are not written.
pub fn add_argument(m: &mut Mem, args: Ptr32<UpdArg>, count: Ptr32<i32>, rel: Ptr32<ARelation>, data: Ptr32<Opaque>, active_data: Ptr32<Opaque>) -> Ptr32<UpdArg> {
    let n = m[count];
    let p: Ptr32<UpdArg> = Ptr32::new(if args.is_null() { m.si_malloc(0x14) } else { m.si_realloc(args.addr(), ((n * 5 + 5) * 4) as u32) });
    let a = p.add(n);
    m[a].chapid = m[rel].chapid;
    m[a].elemid = m[rel].elemid;
    m[a].group = m[rel].group;
    m[a].data = data;
    m[a].active = m[rel].flag;
    m[a].active_data = active_data;
    m[count] += 1;
    p
}

/// [soft 0x08141468 SI_updlistInit]: an empty list, marked dirty.
pub fn init(m: &mut Mem, list: Ptr32<UpdList>) {
    m[list].nb = 0;
    m[list].nodes = Ptr32::NULL;
    m[list].dirty = 1;
}

/// [soft 0x08141488 si_updlistAddNode]: appends a step for element `elemid` of chapter
/// `chap`. Nothing is appended for an element of the MODELS chapter that has no object.
/// The words at +0x0c and +0x10 and the bytes at +0x1a and +0x22 of the new node are not
/// written.
#[allow(clippy::too_many_arguments)]
pub fn add_node(m: &mut Mem, list: Ptr32<UpdList>, chap: i32, elemid: i32, elem: Ptr32<Elem>, relid: i32, rel: Ptr32<ARelation>, group: i32, nb_args: i32, args: Ptr32<UpdArg>) {
    if !m[elem].data.is_null() || chap != m[SI_CID].model {
        let n = m[list].nb;
        let old = m[list].nodes;
        let p = if old.is_null() { m.si_malloc(0x2c) } else { m.si_realloc(old.addr(), ((n + 1) * 0x2c) as u32) };
        m[list].nodes = Ptr32::new(p);
        let node: Ptr32<UpdNode> = Ptr32::<UpdNode>::new(p).add(n);
        m[node].chapid = chap as i16;
        m[node].group = group as i16;
        m[node].elemid = elemid;
        m[node].data = m[elem].data;
        m[node].relid = relid;
        m[node].active = 1;
        m[node].rel_data = Ptr32::NULL;
        if !rel.is_null() && !m[rel].data.is_null() {
            m[node].rel_data = m[rel].data;
            m[node].active = m[rel].flag;
        }
        m[node].extra = Ptr32::NULL;
        m[node].nb_args = nb_args as i16;
        m[node].args = args;
        m[list].nb += 1;
    }
}

/// [soft 0x08141554 SI_updlistDispose]: frees every node's arguments and extra block,
/// then the nodes, and leaves the list empty and dirty.
pub fn dispose(m: &mut Mem, list: Ptr32<UpdList>) {
    let mut i = 0;
    while i < m[list].nb {
        let node: Ptr32<UpdNode> = m[list].nodes.cast::<UpdNode>().add(i);
        if !m[node].args.is_null() {
            let p = m[node].args.addr();
            m.free(p);
            m[node].args = Ptr32::NULL;
        }
        if !m[node].extra.is_null() {
            let p = m[node].extra.addr();
            m.free(p);
            m[node].extra = Ptr32::NULL;
        }
        i += 1;
    }
    if !m[list].nodes.is_null() {
        let p = m[list].nodes.addr();
        m.free(p);
        m[list].nodes = Ptr32::NULL;
    }
    init(m, list);
}
