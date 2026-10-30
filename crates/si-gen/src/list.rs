// SPDX-License-Identifier: MIT
//! `genList.c` on the image: a circular doubly linked list of data pointers (finding 0033).
//!
//! Records (read from code: every field is named by what the routines below do with it):
//!
//! - the list, `SI_calloc(1, 0x18)`: [`HEAD`], [`TAIL`], the iteration's cursor
//!   ([`CURSOR`]), the element an iteration started at ([`START`]), the element count
//!   ([`COUNT`], signed), a word of flags the list only stores ([`FLAGS`]);
//! - an element, `SI_calloc(1, 0x10)`: its data ([`DATA`]), the previous and next elements
//!   ([`PREV`], [`NEXT`]; the head's previous is the tail, the tail's next the head) and the
//!   list it belongs to ([`LIST`]).
//!
//! An iteration hands two 16-bit flags back through pointers: the first is 1 when the list is
//! empty (from [`iter_init`]) or when the element reached is the head again (from
//! [`iter_next`], [`iter_prev`]); the second is 1 when the element reached is the one the
//! iteration started at. Both are written 1 first, whatever happens next.
//!
//! A destructor handed to [`destroy`], [`elem_destroy`], [`remove`] and
//! [`elem_replace_data`] is called with the **element**, whose first word is the data (so it
//! receives a pointer to the data pointer, as `SI_matAndTxt2DDestroy` reads it). A
//! comparison (`SI_genListInsertS`, `SI_genListElemOf`, `SI_genListCopy`) is called as
//! `cmp(data, element's data)` and its answer is taken as 16 bits; a copy as `copy(data)`;
//! a dump as `dump(data, file)`.

use si_mem::{Mem, Outside, Routine};

/// The list's first element (null when empty).
pub const HEAD: u32 = 0x00;
/// The list's last element.
pub const TAIL: u32 = 0x04;
/// The element the iteration is at.
pub const CURSOR: u32 = 0x08;
/// The element the iteration started at ([`iter_init`], [`iter_set`]).
pub const START: u32 = 0x0c;
/// The number of elements (32 bits, signed).
pub const COUNT: u32 = 0x10;
/// A word the caller sets and reads ([`set_flags`], [`get_flags`]).
pub const FLAGS: u32 = 0x14;
/// The size of a list.
pub const LIST_SIZE: u32 = 0x18;

/// An element's data pointer.
pub const DATA: u32 = 0x00;
/// The previous element.
pub const PREV: u32 = 0x04;
/// The next element.
pub const NEXT: u32 = 0x08;
/// The list the element belongs to.
pub const LIST: u32 = 0x0c;
/// The size of an element.
pub const ELEM_SIZE: u32 = 0x10;

/// The C library's `fprintf`, through the binary's stub.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);

/// The texts the dumps print, in `.data`.
pub mod text {
    /// `"\nDumping Generic List: %d\n"`
    pub const DUMPING: u32 = 0x086d_bba0;
    /// `"Generic List is empty!\n"`
    pub const EMPTY: u32 = 0x086d_bbbb;
    /// `"List has %d elements\n"`
    pub const COUNT: u32 = 0x086d_bbd3;
    /// `"list head: %d list tail: %d flags %d\n\n"`
    pub const HEAD_TAIL: u32 = 0x086d_bc00;
    /// `"Item %d:\n\n"` (in the loop)
    pub const ITEM: u32 = 0x086d_bc27;
    /// `"Item %d:\n\n"` (the last element, a second copy of the text)
    pub const LAST_ITEM: u32 = 0x086d_bc32;
    /// `"-------Finished dumping Generic List--------\n"`
    pub const FINISHED: u32 = 0x086d_bc40;
    /// `"Dumping Generic List Element: %d\n"`
    pub const ELEM: u32 = 0x086d_bc80;
    /// `"Generic List element is empty!\n"`
    pub const ELEM_EMPTY: u32 = 0x086d_bcc0;
    /// `"Next list elem: %d previous list elem: %d\n"`
    pub const NEXT_PREV: u32 = 0x086d_bce0;
    /// `"list info (pointer back to parent list): %d\n"`
    pub const PARENT: u32 = 0x086d_bd20;
    /// `"\n"`
    pub const NEWLINE: u32 = 0x086d_bd4d;
}

fn inc(m: &mut Mem, at: u32, by: i32) {
    let v = m.i32(at).wrapping_add(by);
    m.set_i32(at, v);
}

/// A new empty list [soft 0x08514ef0 SI_genListCreate]: `SI_calloc(1, 0x18)`.
pub fn create(m: &mut Mem) -> u32 {
    m.si_calloc(1, LIST_SIZE)
}

/// A new element, all zero [soft 0x085153bc SI_genListElemCreate]: `SI_calloc(1, 0x10)`.
pub fn elem_create(m: &mut Mem) -> u32 {
    m.si_calloc(1, ELEM_SIZE)
}

/// The list at `*plist` and its elements freed [soft 0x08514f08 SI_genListDestroy]: nothing
/// for a null `plist` or list. From the head, each element is handed to
/// [`elem_destroy`] with `destroy` once the iteration has moved past it; the walk stops at
/// a null element or when it is back at the head, and the element it is at is destroyed
/// last. Then the list is freed (`free`) and `*plist` cleared (the original stores the
/// zero twice). The list pointer is read again from `*plist` at each step.
pub fn destroy(m: &mut Mem, o: &mut dyn Outside, plist: u32, destroy: u32) {
    if plist == 0 || m.u32(plist) == 0 {
        return;
    }
    // the frame: the current element at +0 (ebp − 8), the two flags at +4 and +6
    let frame = m.local(8);
    let (cur, first, start) = (frame, frame + 4, frame + 6);
    let list = m.u32(plist);
    let e = iter_init(m, list, first, start);
    m.set_u32(cur, e);
    loop {
        let list = m.u32(plist);
        let next = iter_next(m, list, first, start);
        if next == 0 || m.i16(first) == 1 {
            break;
        }
        elem_destroy(m, o, cur, destroy);
        m.set_u32(cur, next);
    }
    if m.u32(cur) != 0 {
        elem_destroy(m, o, cur, destroy);
    }
    let list = m.u32(plist);
    if list != 0 {
        m.free(list);
        m.set_u32(plist, 0);
    }
    m.set_u32(plist, 0);
    m.release(8);
}

/// The iteration started at the head [soft 0x08514fb8 SI_genListIterInit]: both flags 1
/// (`*last` first); for a list with a count above 0 the head becomes the start and the
/// cursor, both flags 0, and the head is the answer. Otherwise null.
pub fn iter_init(m: &mut Mem, list: u32, first: u32, last: u32) -> u32 {
    m.set_i16(last, 1);
    m.set_i16(first, 1);
    if list == 0 || m.i32(list + COUNT) <= 0 {
        return 0;
    }
    let head = m.u32(list + HEAD);
    m.set_u32(list + START, head);
    m.set_u32(list + CURSOR, head);
    m.set_i16(last, 0);
    m.set_i16(first, 0);
    head
}

fn step(m: &mut Mem, list: u32, first: u32, last: u32, link: u32) -> u32 {
    m.set_i16(last, 1);
    m.set_i16(first, 1);
    if list == 0 || m.i32(list + COUNT) <= 0 {
        return 0;
    }
    let cursor = m.u32(list + CURSOR);
    if cursor == 0 {
        return 0;
    }
    let e = m.u32(cursor + link);
    m.set_u32(list + CURSOR, e);
    let at_head = e == m.u32(list + HEAD);
    m.set_i16(first, at_head as i16);
    let at_start = e == m.u32(list + START);
    m.set_i16(last, at_start as i16);
    e
}

/// The next element [soft 0x08514ff4 SI_genListIterNext]: both flags 1 (`*last` first);
/// for a list with a count above 0 and a cursor, the cursor moves to its next element,
/// which is the answer; `*first` says whether it is the head, `*last` whether it is the
/// iteration's start. Otherwise null.
pub fn iter_next(m: &mut Mem, list: u32, first: u32, last: u32) -> u32 {
    step(m, list, first, last, NEXT)
}

/// The previous element [soft 0x08515054 SI_genListIterPrev]: as [`iter_next`] the other way.
pub fn iter_prev(m: &mut Mem, list: u32, first: u32, last: u32) -> u32 {
    step(m, list, first, last, PREV)
}

/// The iteration set at `e` [soft 0x085150b4 SI_genListIterSet]: both flags 1 (`*last`
/// first); with a list and an element, `e` becomes the cursor and the start and both flags
/// 0. The original leaves `e` in its result register.
pub fn iter_set(m: &mut Mem, list: u32, e: u32, first: u32, last: u32) -> u32 {
    m.set_i16(last, 1);
    m.set_i16(first, 1);
    if list != 0 && e != 0 {
        m.set_u32(list + CURSOR, e);
        m.set_u32(list + START, e);
        m.set_i16(last, 0);
        m.set_i16(first, 0);
    }
    e
}

/// `data` in a new element before the head [soft 0x085150ec SI_genListInsertB]; null for a
/// null list or when no element can be allocated. The new element is the head; the count
/// goes up by one. The answer is the element.
pub fn insert_b(m: &mut Mem, list: u32, data: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    let e = elem_create(m);
    if e == 0 {
        return 0;
    }
    m.set_u32(e + DATA, data);
    m.set_u32(e + LIST, list);
    let head = m.u32(list + HEAD);
    if head == 0 {
        m.set_u32(list + TAIL, e);
        m.set_u32(list + HEAD, e);
        m.set_u32(e + NEXT, e);
        m.set_u32(e + PREV, e);
    } else {
        m.set_u32(e + NEXT, head);
        let head = m.u32(list + HEAD);
        m.set_u32(head + PREV, e);
        let tail = m.u32(list + TAIL);
        m.set_u32(list + HEAD, e);
        m.set_u32(tail + NEXT, e);
        let (h, t) = (m.u32(list + HEAD), m.u32(list + TAIL));
        m.set_u32(h + PREV, t);
    }
    inc(m, list + COUNT, 1);
    e
}

/// `data` in a new element after the tail [soft 0x08515154 SI_genListInsertE]; null for a
/// null list or when no element can be allocated. The new element is the tail; the count
/// goes up by one. The answer is the element.
pub fn insert_e(m: &mut Mem, list: u32, data: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    let e = elem_create(m);
    if e == 0 {
        return 0;
    }
    m.set_u32(e + DATA, data);
    m.set_u32(e + LIST, list);
    let tail = m.u32(list + TAIL);
    if tail == 0 {
        m.set_u32(list + HEAD, e);
        m.set_u32(list + TAIL, e);
        m.set_u32(e + NEXT, e);
        m.set_u32(e + PREV, e);
    } else {
        m.set_u32(tail + NEXT, e);
        let tail = m.u32(list + TAIL);
        m.set_u32(e + PREV, tail);
        let head = m.u32(list + HEAD);
        m.set_u32(list + TAIL, e);
        m.set_u32(e + NEXT, head);
        let (h, t) = (m.u32(list + HEAD), m.u32(list + TAIL));
        m.set_u32(h + PREV, t);
    }
    inc(m, list + COUNT, 1);
    e
}

/// A list printed on `file` [soft 0x085151b4 SI_genListDump], each element by
/// [`elem_dump`] with `dump`: `fprintf(file, "\nDumping Generic List: %d\n", list)`; for a
/// null list `"Generic List is empty!\n"` and nothing more; otherwise the count, the head,
/// tail and flags (as `%d`), then from the head, while the iteration has a next element
/// that is not the head again, `"Item %d:\n\n"` with a number from 0 and the element; the
/// element the walk stopped at (when there is one) with the next number and the text's
/// second copy; last the closing line.
pub fn dump(m: &mut Mem, o: &mut dyn Outside, list: u32, dump: u32, file: u32) {
    let frame = m.local(4);
    let (first, last) = (frame, frame + 2);
    let mut n: u32 = 0;
    o.call(m, FPRINTF, &[file, text::DUMPING, list]);
    if list == 0 {
        o.call(m, FPRINTF, &[file, text::EMPTY]);
        m.release(4);
        return;
    }
    let count = m.u32(list + COUNT);
    o.call(m, FPRINTF, &[file, text::COUNT, count]);
    let (head, tail, flags) = (m.u32(list + HEAD), m.u32(list + TAIL), m.u32(list + FLAGS));
    o.call(m, FPRINTF, &[file, text::HEAD_TAIL, head, tail, flags]);
    let mut e = iter_init(m, list, first, last);
    loop {
        let next = iter_next(m, list, first, last);
        if next == 0 || m.i16(first) == 1 {
            break;
        }
        o.call(m, FPRINTF, &[file, text::ITEM, n]);
        n = n.wrapping_add(1);
        elem_dump(m, o, e, dump, file);
        e = next;
    }
    if e != 0 {
        o.call(m, FPRINTF, &[file, text::LAST_ITEM, n]);
        elem_dump(m, o, e, dump, file);
    }
    o.call(m, FPRINTF, &[file, text::FINISHED]);
    m.release(4);
}

/// The element at `*pe` freed [soft 0x085152ec SI_genListElemDestroy]: nothing for a null
/// `pe` or element; `destroy(element)` when `destroy` is not null; then the element (read
/// again from `*pe`) is freed and `*pe` cleared (twice, as in [`destroy`]).
pub fn elem_destroy(m: &mut Mem, o: &mut dyn Outside, pe: u32, destroy: u32) {
    if pe == 0 {
        return;
    }
    let e = m.u32(pe);
    if e == 0 {
        return;
    }
    if destroy != 0 {
        o.call(m, Routine::at(destroy), &[e]);
    }
    let e = m.u32(pe);
    if e != 0 {
        m.free(e);
        m.set_u32(pe, 0);
    }
    m.set_u32(pe, 0);
}

/// An element printed on `file` [soft 0x08515334 SI_genListElemDump]: `"Dumping Generic
/// List Element: %d\n"` with its address; for a null element `"Generic List element is
/// empty!\n"` and nothing more; otherwise its next and previous elements, its list,
/// `dump(data, file)` and a new line.
pub fn elem_dump(m: &mut Mem, o: &mut dyn Outside, e: u32, dump: u32, file: u32) {
    o.call(m, FPRINTF, &[file, text::ELEM, e]);
    if e == 0 {
        o.call(m, FPRINTF, &[file, text::ELEM_EMPTY]);
        return;
    }
    let (next, prev) = (m.u32(e + NEXT), m.u32(e + PREV));
    o.call(m, FPRINTF, &[file, text::NEXT_PREV, next, prev]);
    let list = m.u32(e + LIST);
    o.call(m, FPRINTF, &[file, text::PARENT, list]);
    let data = m.u32(e + DATA);
    o.call(m, Routine::at(dump), &[data, file]);
    o.call(m, FPRINTF, &[file, text::NEWLINE]);
}

/// An element's data [soft 0x085153d4 SI_genListElemGetData]; null for a null element.
pub fn elem_get_data(m: &Mem, e: u32) -> u32 {
    if e == 0 {
        0
    } else {
        m.u32(e + DATA)
    }
}

/// The list's head [soft 0x085153e8 SI_genListGetHead]; null for a null list.
pub fn get_head(m: &Mem, list: u32) -> u32 {
    if list == 0 {
        0
    } else {
        m.u32(list + HEAD)
    }
}

/// An element's data set [soft 0x085153fc SI_genListElemSetData]: nothing (answer 0) for a
/// null element or null data; otherwise the answer is the element.
pub fn elem_set_data(m: &mut Mem, e: u32, data: u32) -> u32 {
    if e == 0 || data == 0 {
        return 0;
    }
    m.set_u32(e + DATA, data);
    e
}

/// `e` linked in after `after` [soft 0x08515418 SI_genListInsertElemAfter]: with both, `e`
/// takes `after` as previous, `after`'s next as next and `after`'s list; the next element's
/// previous becomes `e`, the list's tail too when it was `after`, then `after`'s next, and
/// the list's count goes up by one. The answer is `e` in every case.
pub fn insert_elem_after(m: &mut Mem, after: u32, e: u32) -> u32 {
    if after == 0 || e == 0 {
        return e;
    }
    m.set_u32(e + PREV, after);
    let next = m.u32(after + NEXT);
    m.set_u32(e + NEXT, next);
    let list = m.u32(after + LIST);
    m.set_u32(e + LIST, list);
    let next = m.u32(after + NEXT);
    m.set_u32(next + PREV, e);
    let list = m.u32(after + LIST);
    if m.u32(list + TAIL) == after {
        m.set_u32(list + TAIL, e);
    }
    let list = m.u32(after + LIST);
    m.set_u32(after + NEXT, e);
    inc(m, list + COUNT, 1);
    e
}

/// An element's data replaced [soft 0x08515458 SI_genListElemReplaceData]: nothing (answer
/// 0) for a null element or null data; otherwise `destroy(element)` (not tested for null)
/// and [`elem_set_data`]; the answer is the element.
pub fn elem_replace_data(m: &mut Mem, o: &mut dyn Outside, e: u32, destroy: u32, data: u32) -> u32 {
    if e == 0 || data == 0 {
        return 0;
    }
    o.call(m, Routine::at(destroy), &[e]);
    elem_set_data(m, e, data);
    e
}

/// `e` taken out of `list` and destroyed [soft 0x08515490 SI_genListRemove] (with
/// [`elem_destroy`] and `destroy`); nothing (answer 0) for a null list or element. The
/// links: the only element empties the list; the head hands the head to its next element,
/// the tail the tail to its previous one; the count goes down by one. The answer is the new
/// head, the new tail, or for an element inside the list its previous element. The
/// cursor and the start are not touched.
///
/// Where the port differs: removing the only element, the original answers whatever its
/// caller left in `esi` (the register is not set on that path); the port answers 0.
pub fn remove(m: &mut Mem, o: &mut dyn Outside, list: u32, e: u32, destroy: u32) -> u32 {
    if list == 0 || e == 0 {
        return 0;
    }
    let answer;
    if e == m.u32(list + HEAD) {
        if e == m.u32(list + TAIL) {
            m.set_u32(list + TAIL, 0);
            m.set_u32(list + HEAD, 0);
            answer = 0;
        } else {
            let (next, prev) = (m.u32(e + NEXT), m.u32(e + PREV));
            m.set_u32(next + PREV, prev);
            let head = m.u32(list + HEAD);
            let (hp, hn) = (m.u32(head + PREV), m.u32(head + NEXT));
            m.set_u32(hp + NEXT, hn);
            let head = m.u32(list + HEAD);
            let new = m.u32(head + NEXT);
            m.set_u32(list + HEAD, new);
            answer = new;
        }
    } else if e == m.u32(list + TAIL) {
        let (prev, next) = (m.u32(e + PREV), m.u32(e + NEXT));
        m.set_u32(prev + NEXT, next);
        let tail = m.u32(list + TAIL);
        let (tn, tp) = (m.u32(tail + NEXT), m.u32(tail + PREV));
        m.set_u32(tn + PREV, tp);
        let tail = m.u32(list + TAIL);
        let new = m.u32(tail + PREV);
        m.set_u32(list + TAIL, new);
        answer = new;
    } else {
        let (next, prev) = (m.u32(e + NEXT), m.u32(e + PREV));
        m.set_u32(next + PREV, prev);
        let (prev, next) = (m.u32(e + PREV), m.u32(e + NEXT));
        m.set_u32(prev + NEXT, next);
        answer = m.u32(e + PREV);
    }
    // the element is passed by its argument's address, which the destruction clears
    let cell = m.local(4);
    m.set_u32(cell, e);
    elem_destroy(m, o, cell, destroy);
    m.release(4);
    inc(m, list + COUNT, -1);
    answer
}

/// The flags word set [soft 0x08515544 SI_genListSetFlags]; the answer is `flags`, or 0
/// for a null list (which is not written).
pub fn set_flags(m: &mut Mem, list: u32, flags: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    m.set_u32(list + FLAGS, flags);
    flags
}

/// The flags word [soft 0x0851555c SI_genListGetFlags]; 0 for a null list.
pub fn get_flags(m: &Mem, list: u32) -> u32 {
    if list == 0 {
        0
    } else {
        m.u32(list + FLAGS)
    }
}

/// A copy of a list [soft 0x08515578 SI_genListCopy]: null for a null list or when no list
/// can be allocated. The new list takes the count and the flags of the old one; then,
/// from the head while the walk is not back at the head, each element's data goes through
/// `copy(data)` and the answer into the new list by [`insert_s`] with `cmp`. **The insertions
/// count again**: the copy's count is twice the number of its elements (as in the original).
pub fn copy(m: &mut Mem, o: &mut dyn Outside, list: u32, copy: u32, cmp: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    let new = create(m);
    if new == 0 {
        return 0;
    }
    let frame = m.local(4);
    let (first, last) = (frame, frame + 2);
    let count = m.u32(list + COUNT);
    m.set_u32(new + COUNT, count);
    let flags = m.u32(list + FLAGS);
    m.set_u32(new + FLAGS, flags);
    let mut e = iter_init(m, list, first, last);
    if m.i16(first) != 1 {
        loop {
            let data = elem_get_data(m, e);
            let c = o.call(m, Routine::at(copy), &[data]).eax;
            insert_s(m, o, new, c, cmp);
            e = iter_next(m, list, first, last);
            if m.i16(first) == 1 {
                break;
            }
        }
    }
    m.release(4);
    new
}

/// `data` inserted in order [soft 0x08515608 SI_genListInsertS]: null for a null list; into
/// a list without a tail by [`insert_b`]. Otherwise, from the head, the walk goes on while
/// `cmp(data, element's data)` answers non-zero (16 bits), and stops before the first
/// element where it answers 0 or when it is back at the head (then without an element).
/// Stopped at the head: [`insert_b`]; having walked the whole list: [`insert_e`]; otherwise
/// a new element linked in before the element it stopped at (no test of the allocation for
/// the links: a null element answers null); the count goes up by one. The answer is the new
/// element.
pub fn insert_s(m: &mut Mem, o: &mut dyn Outside, list: u32, data: u32, cmp: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    if m.u32(list + TAIL) == 0 {
        return insert_b(m, list, data);
    }
    // the frame: the previous element at +0 (ebp − 8), the element's data at +4 (ebp − 0xc
    // is below it in the original; the order of the two words does not matter here), the
    // two flags at +8 and +10
    let frame = m.local(12);
    let (first, last) = (frame + 8, frame + 10);
    let mut prev = 0u32;
    let mut e = iter_init(m, list, first, last);
    let mut d = elem_get_data(m, e);
    loop {
        if m.i16(first) == 1 {
            break;
        }
        if o.call(m, Routine::at(cmp), &[data, d]).eax as u16 == 0 {
            break;
        }
        prev = e;
        e = iter_next(m, list, first, last);
        if m.i16(first) == 1 {
            e = 0;
            continue;
        }
        d = elem_get_data(m, e);
    }
    m.release(12);
    if prev == 0 {
        return insert_b(m, list, data);
    }
    if e == 0 {
        return insert_e(m, list, data);
    }
    let new = elem_create(m);
    if new == 0 {
        return 0;
    }
    m.set_u32(new + DATA, data);
    m.set_u32(new + LIST, list);
    m.set_u32(new + PREV, prev);
    m.set_u32(new + NEXT, e);
    let pn = m.u32(prev + NEXT);
    m.set_u32(pn + PREV, new);
    m.set_u32(prev + NEXT, new);
    inc(m, list + COUNT, 1);
    new
}

/// The element numbered `n` from the head (0 the head) [soft 0x08515728 SI_genListFindNth]:
/// null for a null list or `n` outside 0 … count − 1 (signed); otherwise the walk's
/// element after `n` steps, or the element it reached when it came back to the head.
pub fn find_nth(m: &mut Mem, list: u32, n: i32) -> u32 {
    if list == 0 || n < 0 || n > m.i32(list + COUNT).wrapping_sub(1) {
        return 0;
    }
    let frame = m.local(4);
    let (first, last) = (frame, frame + 2);
    let mut e = iter_init(m, list, first, last);
    let mut k = 0i32;
    if m.i16(first) != 1 && k != n {
        loop {
            k = k.wrapping_add(1);
            e = iter_next(m, list, first, last);
            if m.i16(first) == 1 || k == n {
                break;
            }
        }
    }
    m.release(4);
    e
}

/// The first element for which `cmp(data, element's data)` answers non-zero (16 bits)
/// [soft 0x0851579c SI_genListElemOf]: null for a null list, data or comparison, for an
/// empty list, or when the walk from the head comes back to the head.
pub fn elem_of(m: &mut Mem, o: &mut dyn Outside, list: u32, data: u32, cmp: u32) -> u32 {
    if list == 0 || data == 0 || cmp == 0 {
        return 0;
    }
    let frame = m.local(4);
    let (first, last) = (frame, frame + 2);
    let mut e = iter_init(m, list, first, last);
    loop {
        if m.i16(first) != 0 {
            break;
        }
        let d = elem_get_data(m, e);
        if o.call(m, Routine::at(cmp), &[data, d]).eax as u16 != 0 {
            break;
        }
        e = iter_next(m, list, first, last);
    }
    let found = m.i16(first) == 0;
    m.release(4);
    if found {
        e
    } else {
        0
    }
}

#[cfg(test)]
mod tests;
