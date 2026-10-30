// SPDX-License-Identifier: MIT
//! The element dictionary with what its records point to, and the globals around it.
//!
//! `si-types` lays the records out as the 32-bit original has them and keeps each array the
//! original `malloc`s in a `Vec` beside its record. What an element *points to* (its name
//! record, its object, its relation data) is not there: `si-types` keeps those pointers as
//! opaque 32-bit values. Here they are handles into arenas ([`Arena`]): the value of the
//! pointer field is the handle, zero is still null, and two fields are equal exactly when
//! the original's pointers would be.

use si_db::ListNode;
use si_types::dict::{ChapIds, DbListNode, ElemIcon};
use si_types::reldict::RelDictStore;
use si_types::store::ElemDictStore;
use si_types::{Opaque, Ptr32};

/// A set of heap blocks of one type. A handle is the block's position plus one, so that
/// zero stays the null pointer. A freed slot is not reused: a stale handle finds nothing,
/// where the original would read freed memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Arena<T> {
    items: Vec<Option<T>>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena { items: Vec::new() }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a value and returns its handle (never zero).
    pub fn alloc(&mut self, value: T) -> u32 {
        self.items.push(Some(value));
        self.items.len() as u32
    }

    pub fn get(&self, handle: u32) -> Option<&T> {
        self.items.get((handle as usize).checked_sub(1)?)?.as_ref()
    }

    pub fn get_mut(&mut self, handle: u32) -> Option<&mut T> {
        self.items.get_mut((handle as usize).checked_sub(1)?)?.as_mut()
    }

    /// `free`: takes the value out. Freeing null or a freed block gives `None`.
    pub fn free(&mut self, handle: u32) -> Option<T> {
        self.items.get_mut((handle as usize).checked_sub(1)?)?.take()
    }

    /// Number of live blocks.
    pub fn live(&self) -> usize {
        self.items.iter().filter(|i| i.is_some()).count()
    }

    /// Handles of the live blocks, in the order they were allocated.
    pub fn handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.items.iter().enumerate().filter(|(_, i)| i.is_some()).map(|(i, _)| i as u32 + 1)
    }
}

/// The handle of a name record as an element stores it (`__SI_Elem` +0x00).
pub fn node_ptr(handle: u32) -> Ptr32<DbListNode> {
    Ptr32::new(handle)
}

/// The handle of an object as an element stores it (`__SI_Elem` +0x04, +0x08).
pub fn data_ptr(handle: u32) -> Ptr32<Opaque> {
    Ptr32::new(handle)
}

/// Which element: the chapter's index in the dictionary and the element's index in the
/// chapter. The original passes `__SI_Elem *`; a routine ported here that took such a pointer
/// takes this instead (the pointer is `&ed->lists[chap].elems[index]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElemId {
    pub chap: i32,
    pub index: i32,
}

impl ElemId {
    pub const fn new(chap: i32, index: i32) -> ElemId {
        ElemId { chap, index }
    }
}

/// One element dictionary (`SI_ElemDict`) with the name records its elements point to.
///
/// What an element's `data` and `aux_data` and a relation's `data` designate is not here:
/// the dictionary code never looks inside them (it compares the pointers and passes them
/// on). They stay handles (`Ptr32`), owned by whoever loads the scene.
#[derive(Clone, Debug)]
pub struct Dict {
    pub ed: ElemDictStore,
    /// Name records (`SI_DbListNode`): `Elem::node` holds a handle into this arena.
    pub nodes: Arena<ListNode>,
    /// What the dictionary code has disposed of that it does not own here.
    pub disposed: Disposed,
}

/// Things the dictionary code frees through routines of other packages. The handles are
/// collected here for their owner to release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disposed {
    /// `SI_elemdictDisposeRelationData(relation)`: the relation's id and its data.
    pub rel_data: Vec<(i32, Ptr32<Opaque>)>,
    /// `SI_dbIconFree(element)`: the element's icon.
    pub icons: Vec<Ptr32<ElemIcon>>,
}

impl Default for Dict {
    fn default() -> Self {
        Dict::new()
    }
}

impl Dict {
    /// An initialised dictionary without chapters.
    pub fn new() -> Dict {
        Dict { ed: ElemDictStore::new(), nodes: Arena::new(), disposed: Disposed::default() }
    }

    /// The name record of an element: `ed->lists[chap].elems[index].node`, whatever the
    /// element's status. `None` for a null pointer, a freed record or a slot that does not
    /// exist.
    pub fn node_of(&self, chap: i32, index: i32) -> Option<&ListNode> {
        let e = self.ed.lists.get(usize::try_from(chap).ok()?)?.elems.get(usize::try_from(index).ok()?)?;
        self.nodes.get(e.rec.node.0)
    }

    pub fn node_of_mut(&mut self, chap: i32, index: i32) -> Option<&mut ListNode> {
        let e = self.ed.lists.get(usize::try_from(chap).ok()?)?.elems.get(usize::try_from(index).ok()?)?;
        self.nodes.get_mut(e.rec.node.0)
    }

    /// The slot `ed->lists[chap].elems[index]` whatever its status (the original's pointer
    /// arithmetic, with the bounds it does not check).
    pub fn slot(&self, id: ElemId) -> Option<&si_types::store::ElemStore> {
        self.ed.lists.get(usize::try_from(id.chap).ok()?)?.elems.get(usize::try_from(id.index).ok()?)
    }

    pub fn slot_mut(&mut self, id: ElemId) -> Option<&mut si_types::store::ElemStore> {
        self.ed.lists.get_mut(usize::try_from(id.chap).ok()?)?.elems.get_mut(usize::try_from(id.index).ok()?)
    }
}

/// The program globals the dictionary code reads and writes. One value of this stands for
/// the process.
pub struct Program {
    /// `SI_cid` (0x08781660): the chapter indices of the scene dictionary.
    pub cid: ChapIds,
    /// `SI_RD` (0x0878f6c0…): the relation dictionary.
    pub rd: RelDictStore,
    /// `RelSerialNumber`: the counter behind `SI_elemdictGetRelSerialNumber`.
    pub rel_serial: i32,
    /// What `SI_grafnodeDefColor` (0x0832bff4) returns, `SI_select7WM + SI_color1`: the
    /// wire colour of a new element. Both globals come from the display at start-up.
    pub def_color: i32,
    /// `SI_getLoadOptim()` (0x080d94b0): the environment variable `SI_LOAD_OPTIM` was set.
    pub load_optim: bool,
    /// `SI_useElemListSortTbl`: the loader sets it to `load_optim` while it runs.
    pub use_elem_list_sort_tbl: bool,
    /// The 16-bit global at 0x087fb99c: a scene is being loaded. Names read from files keep
    /// their odd characters while it is set (`SI_dbnameCheckValidity`).
    pub loading: bool,
    /// The 16-bit global at 0x08781596. `SI_elemdictAddElem` saves the model relations only
    /// while it is non-zero; the relation routines mark the hierarchy dirty when it is zero.
    pub backup_enabled: bool,
    /// `SI_addInDictionary`.
    pub add_in_dictionary: i16,
    /// What `time(NULL)` returns: the creation time of new name records.
    pub now: i32,
}

impl Program {
    /// The globals as they stand before any dictionary exists: no chapters, an empty
    /// relation dictionary. `def_color` 0 here; a display would set it.
    pub fn new() -> Program {
        Program {
            cid: <ChapIds as si_types::Zero>::zero(),
            rd: RelDictStore::new(),
            rel_serial: 0,
            def_color: 0,
            load_optim: false,
            use_elem_list_sort_tbl: false,
            loading: false,
            backup_enabled: false,
            add_in_dictionary: 1,
            now: 0,
        }
    }
}

impl Default for Program {
    fn default() -> Self {
        Program::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_handles() {
        let mut a: Arena<i32> = Arena::new();
        let h1 = a.alloc(10);
        let h2 = a.alloc(20);
        assert_eq!((h1, h2), (1, 2));
        assert_eq!(a.get(0), None);
        assert_eq!(a.get(h2), Some(&20));
        assert_eq!(a.free(h1), Some(10));
        assert_eq!(a.free(h1), None);
        assert_eq!(a.get(h1), None);
        assert_eq!(a.alloc(30), 3);
        assert_eq!(a.live(), 2);
        assert_eq!(a.handles().collect::<Vec<_>>(), vec![2, 3]);
    }
}
