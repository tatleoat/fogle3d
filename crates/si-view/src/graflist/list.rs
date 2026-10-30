// SPDX-License-Identifier: MIT
//! The array of lists and a list's nodes: creation, insertion, removal, the walks and the
//! look-ups.

use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::kind;
use si_mem::{clib, CStr32, Mem, Opaque, Ptr32};
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};

/// The model a node of kind 2 or 3 stands for.
#[inline]
pub(crate) fn model_of(m: &Mem, node: Ptr32<GrafNode>) -> Ptr32<Model> {
    m[node].data.cast()
}

/// Whether a kind is one of the two a model node has (the original's `kind - 2 < 2`,
/// unsigned).
#[inline]
pub(crate) fn is_model(kind: i32) -> bool {
    (kind.wrapping_sub(2) as u32) < 2
}

// ---------------------------------------------------------------- small things

/// Stops or allows the schematic view's own layout while nodes come and go
/// [soft 0x0832bfd0 SI_schematicNoRefresh]: the word is kept, and its opposite is the
/// schematic view's "slide the items" switch.
pub fn schematic_no_refresh(m: &mut Mem, on: i16) {
    m[SCHEM_NO_REFRESH] = on;
    m[SI_SCHEM_SLIDE_ITEM] = (on == 0) as i16;
}

/// The wire colour a new node gets [soft 0x0832bff4 SI_grafnodeDefColor]: the sum of two
/// 16-bit globals, each taken without sign.
pub fn grafnode_def_color(m: &Mem) -> i32 {
    m[SI_COLOR1] as i32 + m[SI_SELECT7WM] as i32
}

/// The name of what a node stands for [soft 0x0832c00c SI_grafnodeGetName]: null for no
/// node; for a model node or a wave (kinds 2, 3, 0xb, 0xc) the word at +4 of the object;
/// for a camera (4, 5) and a 3D texture (0x10) the object's first word; for a light and a
/// material (6, 7, 8, 0xe, 0x13, 0x14) the object itself, whose record begins with the
/// text; for a 2D texture (0xf) the word at +0x18.
///
/// For every other kind (below 2, above 0x14, and 9, 10, 0xd, 0x11, 0x12) the original
/// returns without setting the result register: its caller gets whatever the register
/// held. The port answers null there.
pub fn grafnode_get_name(m: &Mem, node: Ptr32<GrafNode>) -> CStr32 {
    if node.is_null() {
        return Ptr32::NULL;
    }
    let data = m[node].data.addr();
    match m[node].kind {
        2 | 3 | 0xb | 0xc => Ptr32::new(m.u32(data.wrapping_add(4))),
        4 | 5 | 0x10 => Ptr32::new(m.u32(data)),
        6 | 7 | 8 | 0xe | 0x13 | 0x14 => Ptr32::new(data),
        0xf => Ptr32::new(m.u32(data.wrapping_add(0x18))),
        _ => Ptr32::NULL,
    }
}

/// Renames what a node stands for [soft 0x0832c05c SI_grafnodeChangeName] and frees the
/// name it was handed (unless that is null).
///
/// A model node (2, 3) goes to `SI_modelChangeName (0x0846db14)`. A camera (4) has the
/// string its first word points to freed and replaced by a copy. For a wave or a named
/// selection (0xb, 0xc) the original frees the *object* if there is one, clears its own
/// copy of the pointer either way and then stores the copy of the name at address 4: a
/// write through a null pointer. The image records that as a fault; the original dies
/// there. No caller of the routine was found in the binary.
pub fn grafnode_change_name(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>, name: CStr32) {
    match m[node].kind {
        2 | 3 => {
            let data = m[node].data.addr();
            o.call(m, &outside::SI_MODEL_CHANGE_NAME, &[data, name.addr()]);
        }
        4 => {
            let camera = m[node].data.addr();
            let old = m.u32(camera);
            if old != 0 {
                m.free(old);
                m.set_u32(camera, 0);
            }
            let copy = m.strdup(name.addr());
            m.set_u32(camera, copy);
        }
        0xb | 0xc => {
            let data = m[node].data.addr();
            if data != 0 {
                m.free(data);
            }
            let copy = m.strdup(name.addr());
            // the original's store through its cleared register: address 4
            m.set_u32(4, copy);
        }
        _ => {}
    }
    if !name.is_null() {
        m.free(name.addr());
    }
}

/// The node a node hangs under, going by the chain alone
/// [soft 0x0832c0f4 si_graflistFindParent]: the nearest node before it whose count of
/// nodes below it reaches this far (the k-th node back must count at least k). Null when
/// the chain's head is passed. No caller was found in the binary.
pub fn graflist_find_parent(m: &Mem, node: Ptr32<GrafNode>) -> Ptr32<GrafNode> {
    let mut steps: i32 = 1;
    let mut p = m[node].prev;
    while !p.is_null() {
        if m[p].nb_sub as u16 as i32 >= steps {
            break;
        }
        p = m[p].prev;
        steps += 1;
    }
    p
}

/// The root node of a model node [soft 0x0832c12c SI_graflistFindRoot]: a node of kind 3
/// is followed backwards to the first node of kind 2; any other node answers itself. The
/// original does not test for the chain's head: there it reads through null and dies; the
/// image records the fault and the walk ends with null.
pub fn graflist_find_root(m: &Mem, node: Ptr32<GrafNode>) -> Ptr32<GrafNode> {
    let mut node = node;
    if m[node].kind == kind::MODEL_NODE {
        loop {
            node = m[node].prev;
            if m[node].kind == kind::MODEL_ROOT || m.fault().is_some() {
                break;
            }
        }
    }
    node
}

// ---------------------------------------------------------------- the array of lists

/// Whether list `index` exists [soft 0x0832c144 SI_grafListExists].
pub fn graf_list_exists(m: &Mem, index: i32) -> i32 {
    if index < 0 || index >= m[SI_NB_GRAF_LISTS] {
        return 0;
    }
    (m[m[GRAF_LIST_EXISTS].add(index)] != 0) as i32
}

/// An empty list [soft 0x0832c170 grafListInit]: every word of the record but the two
/// bytes at +2 is set; all zero except the word at +0x0e, which is 1.
pub fn graf_list_init(m: &mut Mem, list: Ptr32<GrafList>) {
    let l = &mut m[list];
    l.unk_10 = [0; 4];
    l.multi = 0;
    l.first = Ptr32::NULL;
    l.active = Ptr32::NULL;
    l.active2 = Ptr32::NULL;
    l.unk_0c = 0;
    l.unk_0e = 1;
}

/// A new list on the heap [soft 0x0832c29c SI_grafListAllocate]: 0x24 bytes from
/// `SI_malloc`, initialised without a test for null.
pub fn graf_list_allocate(m: &mut Mem) -> Ptr32<GrafList> {
    let list = Ptr32::new(m.si_malloc(0x24));
    graf_list_init(m, list);
    list
}

/// Makes the array of lists with its first list, once
/// [soft 0x0832c1bc SI_grafListCreate]: when `SI_grafLists` is null it becomes a new
/// list, the count 1, the process record's list index 0, and the array of "exists" flags
/// two bytes from `SI_malloc` holding 1.
pub fn graf_list_create(m: &mut Mem) {
    if m[SI_GRAF_LISTS].is_null() {
        let list = graf_list_allocate(m);
        m[SI_GRAF_LISTS] = list;
        let proc = m[SI_PROC_DATA];
        m[SI_NB_GRAF_LISTS] = 1;
        m[proc].list = 0;
        let exists = Ptr32::new(m.si_malloc(2));
        m[GRAF_LIST_EXISTS] = exists;
        m[exists] = 1;
    }
}

/// One more list at the end of the array [soft 0x0832c204 SI_grafListAdd]: the array is
/// reallocated to `(count + 1) × 0x24` bytes and the flags to `(count + 1) × 2`, each
/// result stored before it is tested; then the new list is initialised and flagged. The
/// new list's index, or −1 when either reallocation answers null.
pub fn graf_list_add(m: &mut Mem) -> i32 {
    let index = m[SI_NB_GRAF_LISTS];
    let lists = m[SI_GRAF_LISTS].addr();
    let lists = m.si_realloc(lists, ((index.wrapping_mul(9).wrapping_add(9)) as u32).wrapping_mul(4));
    m[SI_GRAF_LISTS] = Ptr32::new(lists);
    if lists == 0 {
        return -1;
    }
    let exists = m[GRAF_LIST_EXISTS].addr();
    let exists = m.si_realloc(exists, (m[SI_NB_GRAF_LISTS].wrapping_add(1) as u32).wrapping_mul(2));
    m[GRAF_LIST_EXISTS] = Ptr32::new(exists);
    if exists == 0 {
        return -1;
    }
    let n = m[SI_NB_GRAF_LISTS];
    let list = m[SI_GRAF_LISTS].add(n);
    graf_list_init(m, list);
    let flag = m[GRAF_LIST_EXISTS].add(n);
    m[flag] = 1;
    m[SI_NB_GRAF_LISTS] += 1;
    index
}

/// Frees the nodes of a list [soft 0x0832c320 SI_graflistClear] (the nodes only: not what
/// they stand for) and clears its head and its two active nodes. Answers 0.
pub fn graflist_clear(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        let next = m[node].next;
        m.free(node.addr());
        node = next;
    }
    let l = &mut m[list];
    l.first = Ptr32::NULL;
    l.active = Ptr32::NULL;
    l.active2 = Ptr32::NULL;
    0
}

/// Gives up list `index` [soft 0x0832c2c4 SI_grafListRemove]: not the current list of the
/// process record and not an index outside the array (those answer 0). A list whose flag
/// is exactly 1 is cleared and its flag set to 0; the array keeps its length. Answers 1.
pub fn graf_list_remove(m: &mut Mem, index: i32) -> i32 {
    let proc = m[SI_PROC_DATA];
    if index == m[proc].list || index < 0 || index >= m[SI_NB_GRAF_LISTS] {
        return 0;
    }
    let flag = m[GRAF_LIST_EXISTS].add(index);
    if m[flag] == 1 {
        let list = m[SI_GRAF_LISTS].add(index);
        graflist_clear(m, list);
        let flag = m[GRAF_LIST_EXISTS].add(index);
        m[flag] = 0;
    }
    1
}

/// Makes `node` the list's active node [soft 0x0832c488 SI_graflistSetActive]. A list has
/// two places for it: +0x08, and +0x20 for a node that is not editable (flag 0x1000);
/// setting one clears the other. Null clears both. Nothing happens for a null list.
pub fn graflist_set_active(m: &mut Mem, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) {
    if list.is_null() {
        return;
    }
    if node.is_null() || m[node].flags & crate::records::flag::UNEDITABLE == 0 {
        m[list].active2 = Ptr32::NULL;
        m[list].active = node;
    } else {
        m[list].active2 = node;
        m[list].active = Ptr32::NULL;
    }
}

// ---------------------------------------------------------------- nodes

/// A new node [soft 0x0832c890 grafnodeAllocate]: 0x24 bytes from `SI_malloc`, every
/// field zero except the display type, which is 2 (visible). Null when there is no memory.
pub fn grafnode_allocate(m: &mut Mem) -> Ptr32<GrafNode> {
    let node: Ptr32<GrafNode> = Ptr32::new(m.si_malloc(0x24));
    if !node.is_null() {
        let n = &mut m[node];
        n.kind = 0;
        n.data = Ptr32::NULL;
        n.selection = 0;
        n.wire_type = 2;
        n.nb_sub = 0;
        n.wire_color = 0;
        n.unk_14 = 0;
        n.flags = 0;
        n.func = Ptr32::NULL;
        n.next = Ptr32::NULL;
        n.prev = Ptr32::NULL;
    }
    node
}

/// Chains `node` into `list` behind `after`, or at the head when `after` is null
/// [soft 0x0832c8f0 graflistInsertGrafnode].
pub fn graflist_insert_grafnode(m: &mut Mem, list: Ptr32<GrafList>, after: Ptr32<GrafNode>, node: Ptr32<GrafNode>) {
    if after.is_null() {
        m[node].next = m[list].first;
        m[list].first = node;
    } else {
        m[node].next = m[after].next;
        m[after].next = node;
    }
    let next = m[node].next;
    if !next.is_null() {
        m[next].prev = node;
    }
    m[node].prev = after;
}

/// Takes `node` out of `list` [soft 0x0832c9ac SI_graflistRemoveGrafnode], when it is in
/// one: a node without a node before it that is not the list's head is left alone. The
/// list forgets it as active node (the second place is only looked at when the first does
/// not hold it), the chain is closed over it, its element is flagged deleted through
/// `SI_edGrafNodeSetDeleteFlag (0x080fdcf4)` with (node, 1, null, 0), and its link back is
/// cleared. **Its link forward stays**, and the node is not freed. Answers 0.
pub fn graflist_remove_grafnode(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) -> i32 {
    if !m[node].prev.is_null() || node == m[list].first {
        if m[list].active == node {
            m[list].active = Ptr32::NULL;
        } else if m[list].active2 == node {
            m[list].active2 = Ptr32::NULL;
        }
        let (prev, next) = (m[node].prev, m[node].next);
        if prev.is_null() {
            m[list].first = next;
        } else {
            m[prev].next = next;
        }
        if !next.is_null() {
            m[next].prev = prev;
        }
        o.call(m, &outside::SI_ED_GRAF_NODE_SET_DELETE_FLAG, &[node.addr(), 1, 0, 0]);
        m[node].prev = Ptr32::NULL;
    }
    0
}

/// The walk over a list [soft 0x083353d4 SI_nextGrafnode]: `*cursor` becomes the list's
/// head when it is null, else the node after it. Answers whether there is one.
pub fn next_grafnode(m: &mut Mem, list: Ptr32<GrafList>, cursor: Ptr32<Ptr32<GrafNode>>) -> i32 {
    let at = m[cursor];
    let next = if at.is_null() { m[list].first } else { m[at].next };
    m[cursor] = next;
    (!m[cursor].is_null()) as i32
}

// ---------------------------------------------------------------- look-ups

/// The node of an object [soft 0x0832ee68 SI_findGlData]: the first node of the list whose
/// object is `data` and whose kind is `kind`. Kind 3 counts as 2 on both sides, and a
/// `kind` of 0 accepts any. Null when there is none. (`si-rensetup` has the same function
/// in `native/graflist.rs`; this one was written again from the listing.)
pub fn find_gl_data(m: &Mem, list: Ptr32<GrafList>, data: Ptr32<Opaque>, kind: i32) -> Ptr32<GrafNode> {
    let kind = if kind == 3 { 2 } else { kind };
    let mut node = m[list].first;
    while !node.is_null() {
        let mut k = m[node].kind;
        if k == 3 {
            k = 2;
        }
        if (kind == 0 || kind == k) && m[node].data == data {
            return node;
        }
        node = m[node].next;
    }
    Ptr32::NULL
}

/// The selection state of an object's node [soft 0x0832eeb4 SI_stateGlData]: 0 when the
/// object has no node.
pub fn state_gl_data(m: &Mem, list: Ptr32<GrafList>, data: Ptr32<Opaque>) -> i32 {
    let node = find_gl_data(m, list, data, 0);
    if node.is_null() {
        0
    } else {
        m[node].selection
    }
}

/// Kinds as the look-ups by name compare them: 3 counts as 2, and 8, 0x13 and 0x14 as 6.
#[inline]
fn name_kind(k: i32) -> i32 {
    match k {
        3 => 2,
        8 | 0x13 | 0x14 => 6,
        k => k,
    }
}

/// Whether no other node of a kind has this name
/// [soft 0x08336e0c graflistCheckUniqueName]: 0 when a node of the kind asked for (0: any)
/// carries a name equal to `name` at another address, else 1 (also for a null name). The
/// name of a model node is the word at +4 of its model; that of a camera node (4), a light
/// (6, with 8, 0x13, 0x14) and the node in front of a light (7) is **the object's address
/// itself** (right for a light, whose record begins with the text; for a camera the
/// original compares the record's first bytes, a pointer, as text). Other kinds have no
/// name. No caller was found in the binary.
pub fn graflist_check_unique_name(m: &Mem, list: Ptr32<GrafList>, name: CStr32, kind: i32) -> i32 {
    let kind = name_kind(kind);
    if name.is_null() {
        return 1;
    }
    let mut node = m[list].first;
    while !node.is_null() {
        let k = name_kind(m[node].kind);
        if kind == 0 || kind == k {
            let data = m[node].data.addr();
            let s = match k {
                4 | 6 | 7 => data,
                2 => m.u32(data.wrapping_add(4)),
                _ => 0,
            };
            if s != 0 && s != name.addr() && clib::strcmp(m, name.addr(), s) == 0 {
                return 0;
            }
        }
        node = m[node].next;
    }
    1
}

/// The object of a kind that has this name [soft 0x08336edc SI_itemGetByName]. For a
/// camera (kinds 4 and 5) the process record's camera is answered without looking at the
/// name. Otherwise the first node of the kind (0: any) that is a model node (its model's
/// name at +4) or a light or the node in front of one (the object's address as text) and
/// whose name equals `name`; its object, or null. Names are not tested for null. No caller
/// was found in the binary.
pub fn item_get_by_name(m: &Mem, list: Ptr32<GrafList>, name: CStr32, kind: i32) -> Ptr32<Opaque> {
    if (kind.wrapping_sub(4) as u32) < 2 {
        let proc = m[SI_PROC_DATA];
        return m[proc].camera;
    }
    let node = by_name(m, list, name, name_kind(kind));
    if node.is_null() {
        Ptr32::NULL
    } else {
        m[node].data
    }
}

/// The node of a kind that has this name [soft 0x08336fac SI_grafnodeGetByName]: as
/// `SI_itemGetByName` without the camera's exception, answering the node. No caller was
/// found in the binary.
pub fn grafnode_get_by_name(m: &Mem, list: Ptr32<GrafList>, name: CStr32, kind: i32) -> Ptr32<GrafNode> {
    by_name(m, list, name, name_kind(kind))
}

/// The walk the two look-ups by name share (each has its own copy of it in the original).
fn by_name(m: &Mem, list: Ptr32<GrafList>, name: CStr32, kind: i32) -> Ptr32<GrafNode> {
    let mut node = m[list].first;
    while !node.is_null() {
        let k = name_kind(m[node].kind);
        if kind == 0 || kind == k {
            let data = m[node].data.addr();
            let s = match k {
                2 => Some(m.u32(data.wrapping_add(4))),
                6 | 7 => Some(data),
                _ => None,
            };
            if let Some(s) = s {
                if clib::strcmp(m, s, name.addr()) == 0 {
                    return node;
                }
            }
        }
        node = m[node].next;
    }
    Ptr32::NULL
}

// ---------------------------------------------------------------- the hit distance

/// The squared hit distance [soft 0x08335bf0 SI_getHitSize], stored through `out`.
pub fn get_hit_size(m: &mut Mem, out: Ptr32<i32>) {
    m[out] = m[HIT_SIZE];
}

/// Sets it [soft 0x08335c04 SI_setHitSize]. Answers 0.
pub fn set_hit_size(m: &mut Mem, size: i32) -> i32 {
    m[HIT_SIZE] = size;
    0
}

/// Does nothing and answers 0 [soft 0x08335034 SI_clearSelect]: what is left of clearing
/// the selection planes of a view.
pub fn clear_select() -> i32 {
    0
}
