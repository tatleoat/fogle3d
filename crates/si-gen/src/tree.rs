// SPDX-License-Identifier: MIT
//! `genTree.c` on the image: the generic containment tree (finding 0044, unit trim7).
//!
//! Records (read from code): a tree, `SI_calloc(1, 0x14)`: the first root element ([`ROOT`]), a block the tree
//! owns and frees on destruction ([`BLOCK`], never set by these routines), the element count ([`COUNT`], signed) and a
//! word of flags the tree only stores ([`FLAGS`]). An element, `SI_calloc(1, 0x18)`: its data ([`DATA`]), the previous
//! and next element of its sibling chain ([`PREV`], [`NEXT`]), its parent ([`PARENT`], null at the top), its first child
//! ([`CHILD`]) and the tree ([`TREE`]). The chain is not circular.
//!
//! A destructor handed to the destroy / remove routines is called with the **element**. The insertion routines are given a
//! containment compare `cmp(a, b)` (a 16-bit answer, non-zero when `a` lies inside `b`; `curveInCurve` in the program) and are
//! called here through [`Outside`].

use si_mem::{Mem, Outside, Routine};

/// The first root element.
pub const ROOT: u32 = 0x00;
/// The block the tree owns (freed by [`destroy`]).
pub const BLOCK: u32 = 0x04;
/// The number of elements (32 bits, signed).
pub const COUNT: u32 = 0x0c;
/// A word the caller sets and reads.
pub const FLAGS: u32 = 0x10;
/// The size of a tree.
pub const TREE_SIZE: u32 = 0x14;

/// An element's data pointer.
pub const DATA: u32 = 0x00;
/// The previous sibling.
pub const PREV: u32 = 0x04;
/// The next sibling.
pub const NEXT: u32 = 0x08;
/// The parent element.
pub const PARENT: u32 = 0x0c;
/// The first child.
pub const CHILD: u32 = 0x10;
/// The tree the element belongs to.
pub const TREE: u32 = 0x14;
/// The size of an element.
pub const ELEM_SIZE: u32 = 0x18;

/// [soft 0x08515830 SI_genTreeCreate]: `SI_calloc(1, 0x14)`.
pub fn create(m: &mut Mem) -> u32 {
    m.si_calloc(1, TREE_SIZE)
}

/// [soft 0x08515b50 SI_genTreeElemCreate]: `SI_calloc(1, 0x18)`.
pub fn elem_create(m: &mut Mem) -> u32 {
    m.si_calloc(1, ELEM_SIZE)
}

/// [soft 0x08515b68 SI_genTreeElemGetData]: the element's data, 0 for a null element.
pub fn elem_get_data(m: &Mem, e: u32) -> u32 {
    if e == 0 {
        0
    } else {
        m.u32(e + DATA)
    }
}

/// [soft 0x08515b7c SI_genTreeGetRoot]: the tree's first root element, 0 for a null tree.
pub fn get_root(m: &Mem, tree: u32) -> u32 {
    if tree == 0 {
        0
    } else {
        m.u32(tree + ROOT)
    }
}

/// [soft 0x08515b98 SI_genTreeElemSetData]: stores `data` (not when it or the element is null: then 0); answers the element.
pub fn elem_set_data(m: &mut Mem, e: u32, data: u32) -> u32 {
    if e == 0 || data == 0 {
        return 0;
    }
    m.set_u32(e + DATA, data);
    e
}

/// [soft 0x08515bb8 SI_genTreeElemReplaceData]: 0 for a null element or data; otherwise `destroy(element)` (called whatever
/// `destroy` is: a null one faults in the original, here the call goes out with address 0), then the new data is stored;
/// answers the element.
pub fn elem_replace_data(m: &mut Mem, o: &mut dyn Outside, e: u32, destroy: u32, data: u32) -> u32 {
    if e == 0 || data == 0 {
        return 0;
    }
    o.call(m, Routine::at(destroy), &[e]);
    elem_set_data(m, e, data);
    e
}

/// [soft 0x08515cbc SI_genTreeSetFlags]: stores the flags; answers them (0 for a null tree).
pub fn set_flags(m: &mut Mem, tree: u32, flags: u32) -> u32 {
    if tree == 0 {
        return 0;
    }
    m.set_u32(tree + FLAGS, flags);
    flags
}

/// [soft 0x08515cd8 SI_genTreeGetFlags]: the flags, 0 for a null tree.
pub fn get_flags(m: &Mem, tree: u32) -> u32 {
    if tree == 0 {
        0
    } else {
        m.u32(tree + FLAGS)
    }
}

fn dec_count(m: &mut Mem, tree: u32) {
    let c = m.u32(tree + COUNT).wrapping_sub(1);
    m.set_u32(tree + COUNT, c);
}

/// [soft 0x0851591c SI_genTreeElemDestroySubTree]: the element at `*pe` with all its descendants and later siblings: first
/// the child chain (recursively), then the next siblings (recursively, through the element's own next word), then the tree's
/// count goes down by one, `destroy(element)` is called when not null, the element is freed and `*pe` cleared. Nothing for a
/// null `pe` or element.
pub fn elem_destroy_sub_tree(m: &mut Mem, o: &mut dyn Outside, pe: u32, destroy: u32) {
    if pe == 0 {
        return;
    }
    let e = m.u32(pe);
    if e == 0 {
        return;
    }
    if m.u32(e + CHILD) != 0 {
        elem_destroy_sub_tree(m, o, e + CHILD, destroy);
    }
    let e = m.u32(pe);
    if m.u32(e + NEXT) != 0 {
        elem_destroy_sub_tree(m, o, e + NEXT, destroy);
    }
    let e = m.u32(pe);
    let tree = m.u32(e + TREE);
    dec_count(m, tree);
    if destroy != 0 {
        let e = m.u32(pe);
        o.call(m, Routine::at(destroy), &[e]);
    }
    let e = m.u32(pe);
    if e != 0 {
        m.free(e);
        m.set_u32(pe, 0);
    }
    m.set_u32(pe, 0);
}

/// [soft 0x08515848 SI_genTreeDestroy]: the tree at `*ptree`: [`elem_destroy_sub_tree`] on its root slot, then the block at
/// +4 of the tree freed (read from `*ptree`), the tree freed and `*ptree` cleared. Nothing for a null `ptree` or tree.
pub fn destroy(m: &mut Mem, o: &mut dyn Outside, ptree: u32, destroy: u32) {
    if ptree == 0 || m.u32(ptree) == 0 {
        return;
    }
    let tree = m.u32(ptree);
    elem_destroy_sub_tree(m, o, tree, destroy);
    let tree = m.u32(ptree);
    let blk = m.u32(tree + BLOCK);
    if blk != 0 {
        m.free(blk);
        let tree = m.u32(ptree);
        m.set_u32(tree + BLOCK, 0);
    }
    let tree = m.u32(ptree);
    if tree != 0 {
        m.free(tree);
        m.set_u32(ptree, 0);
    }
    m.set_u32(ptree, 0);
}

/// [soft 0x085159a0 SI_genTreeElemDestroy]: the element at `*pe` removed alone: its children take its place in the sibling chain
/// (their parent becomes its parent; they and the following siblings are linked between its previous and next), or, without
/// children, the chain is closed over it; the head of the chain (the tree's root or the parent's child) is moved when the
/// element was first. The tree's count goes down, `destroy(element)` is called when not null, the element freed and `*pe`
/// cleared. Answers the element that took its place in the chain (its first child, else its next sibling); 0 for a null
/// `pe` or element.
pub fn elem_destroy(m: &mut Mem, o: &mut dyn Outside, pe: u32, destroy: u32) -> u32 {
    if pe == 0 {
        return 0;
    }
    let e = m.u32(pe);
    if e == 0 {
        return 0;
    }
    let (prev, next, parent, child, tree) = (m.u32(e + PREV), m.u32(e + NEXT), m.u32(e + PARENT), m.u32(e + CHILD), m.u32(e + TREE));
    let ret;
    if child == 0 {
        if prev != 0 {
            m.set_u32(prev + NEXT, next);
        }
        if next != 0 {
            m.set_u32(next + PREV, prev);
        }
        if parent == 0 {
            if prev == 0 {
                m.set_u32(tree + ROOT, next);
            }
        } else if prev == 0 {
            m.set_u32(parent + CHILD, next);
        }
        ret = next;
    } else {
        // every child gets the element's parent; `last` ends as the last child
        let mut last = child;
        loop {
            m.set_u32(last + PARENT, parent);
            let n = m.u32(last + NEXT);
            if n == 0 {
                break;
            }
            last = n;
        }
        m.set_u32(child + PREV, prev);
        if prev != 0 {
            m.set_u32(prev + NEXT, child);
        }
        m.set_u32(last + NEXT, next);
        if next != 0 {
            m.set_u32(next + PREV, last);
        }
        if parent == 0 {
            if prev == 0 {
                m.set_u32(tree + ROOT, child);
            }
        } else if prev == 0 {
            m.set_u32(parent + CHILD, child);
        }
        ret = child;
    }
    dec_count(m, tree);
    if destroy != 0 {
        o.call(m, Routine::at(destroy), &[e]);
    }
    m.free(e);
    m.set_u32(pe, 0);
    ret
}

/// [soft 0x08515bf0 SI_genTreeRemove]: [`elem_destroy`] for a non-null tree, `pe` and element. The original answers whatever
/// its register held when nothing is removed (nobody uses it); this answers 0 then.
pub fn remove(m: &mut Mem, o: &mut dyn Outside, tree: u32, pe: u32, destroy: u32) -> u32 {
    if tree == 0 || pe == 0 || m.u32(pe) == 0 {
        return 0;
    }
    elem_destroy(m, o, pe, destroy)
}

/// [soft 0x08515c1c SI_genTreeRemoveSubTree]: the element at `*pe` and its descendants (not its later siblings) destroyed:
/// first unhooked from the chain (the head of the chain moved to its next sibling when it was first, else the previous
/// sibling's next set to its next; the next sibling's previous and parent set to the element's), then its next and previous are
/// cleared and [`elem_destroy_sub_tree`] frees the element with its children. Answers the next sibling (0 for a null tree,
/// `pe` or element).
pub fn remove_sub_tree(m: &mut Mem, o: &mut dyn Outside, tree: u32, pe: u32, destroy: u32) -> u32 {
    if tree == 0 || pe == 0 {
        return 0;
    }
    let e = m.u32(pe);
    if e == 0 {
        return 0;
    }
    let (prev, next, parent) = (m.u32(e + PREV), m.u32(e + NEXT), m.u32(e + PARENT));
    if prev == 0 {
        if parent == 0 {
            let t = m.u32(e + TREE);
            m.set_u32(t + ROOT, next);
        } else {
            m.set_u32(parent + CHILD, next);
        }
    } else {
        m.set_u32(prev + NEXT, next);
    }
    if next != 0 {
        m.set_u32(next + PREV, prev);
        m.set_u32(next + PARENT, parent);
    }
    let e = m.u32(pe);
    m.set_u32(e + NEXT, 0);
    m.set_u32(e + PREV, 0);
    elem_destroy_sub_tree(m, o, pe, destroy);
    next
}

/// The compare of the insertion routines: `cmp(a, b)` as 16 bits, true when non-zero.
fn contains(m: &mut Mem, o: &mut dyn Outside, cmp: u32, a: u32, b: u32) -> bool {
    o.call(m, Routine::at(cmp), &[a, b]).eax as u16 != 0
}

/// [soft 0x08515d38 SI_genTreeElemInsert]: `data` placed relative to the element `e` by the containment compare `cmp(a, b)`
/// (non-zero: `a` lies inside `b`). Null for a null `e`. When `cmp(data, e.data)` holds, the new element goes into the
/// children of `e` (a new first child when there are none, else the same routine on the first child). Otherwise, when
/// `cmp(e.data, data)` holds, the new element takes `e`'s place and `e` becomes its first child: the siblings after `e`
/// are walked, those `cmp(sibling.data, data)` accepts stay in the chain below the new element (parent set), the others
/// are moved to follow the new element at the old level. Otherwise the same routine goes on at `e`'s next sibling, or a new
/// element is linked after `e` when it is the last. The tree's count goes up by one for the element made. Answers the new
/// element.
pub fn elem_insert(m: &mut Mem, o: &mut dyn Outside, tree: u32, e: u32, data: u32, cmp: u32) -> u32 {
    let mut e = e;
    loop {
        if e == 0 {
            return 0;
        }
        let ed = elem_get_data(m, e);
        let n;
        if contains(m, o, cmp, data, ed) {
            let ch = m.u32(e + CHILD);
            if ch != 0 {
                e = ch;
                continue;
            }
            n = elem_create(m);
            m.set_u32(n + TREE, tree);
            m.set_u32(e + CHILD, n);
            m.set_u32(n + PARENT, e);
            m.set_u32(n + DATA, data);
        } else {
            let ed = elem_get_data(m, e);
            if contains(m, o, cmp, ed, data) {
                n = elem_create(m);
                m.set_u32(n + TREE, tree);
                let (prev, parent) = (m.u32(e + PREV), m.u32(e + PARENT));
                if prev == 0 {
                    if parent == 0 {
                        m.set_u32(tree + ROOT, n);
                    } else {
                        m.set_u32(parent + CHILD, n);
                    }
                    m.set_u32(n + PARENT, parent);
                    m.set_u32(n + CHILD, e);
                    m.set_u32(e + PARENT, n);
                } else {
                    m.set_u32(n + PARENT, parent);
                    m.set_u32(n + CHILD, e);
                    m.set_u32(n + PREV, prev);
                    m.set_u32(prev + NEXT, n);
                    m.set_u32(e + PARENT, n);
                    m.set_u32(e + PREV, 0);
                }
                let mut cur = m.u32(e + NEXT);
                let mut last = n;
                while cur != 0 {
                    let cd = elem_get_data(m, cur);
                    if !contains(m, o, cmp, cd, data) {
                        // not inside: moved out of the chain below `n`, to follow `last`
                        let (cp, cn) = (m.u32(cur + PREV), m.u32(cur + NEXT));
                        m.set_u32(cp + NEXT, cn);
                        if cn != 0 {
                            m.set_u32(cn + PREV, cp);
                        }
                        m.set_u32(last + NEXT, cur);
                        m.set_u32(cur + PREV, last);
                        m.set_u32(cur + NEXT, 0);
                        let np = m.u32(n + PARENT);
                        last = cur;
                        m.set_u32(last + PARENT, np);
                        cur = cn;
                    } else {
                        m.set_u32(cur + PARENT, n);
                        cur = m.u32(cur + NEXT);
                    }
                }
                m.set_u32(n + DATA, data);
            } else {
                let nx = m.u32(e + NEXT);
                if nx != 0 {
                    e = nx;
                    continue;
                }
                n = elem_create(m);
                m.set_u32(n + TREE, tree);
                m.set_u32(e + NEXT, n);
                m.set_u32(n + PREV, e);
                m.set_u32(n + DATA, data);
                let p = m.u32(e + PARENT);
                m.set_u32(n + PARENT, p);
            }
        }
        let c = m.u32(tree + COUNT).wrapping_add(1);
        m.set_u32(tree + COUNT, c);
        return n;
    }
}

/// [soft 0x08515cf0 SI_genTreeInsertS]: null for a null tree; an empty tree gets a first root element holding `data`
/// (count up by one; null when the allocation fails); otherwise [`elem_insert`] from the root.
pub fn insert_s(m: &mut Mem, o: &mut dyn Outside, tree: u32, data: u32, cmp: u32) -> u32 {
    if tree == 0 {
        return 0;
    }
    let root = m.u32(tree + ROOT);
    if root == 0 {
        let n = elem_create(m);
        if n == 0 {
            return 0;
        }
        m.set_u32(n + TREE, tree);
        m.set_u32(n + DATA, data);
        m.set_u32(tree + ROOT, n);
        let c = m.u32(tree + COUNT).wrapping_add(1);
        m.set_u32(tree + COUNT, c);
        return n;
    }
    elem_insert(m, o, tree, root, data, cmp)
}
