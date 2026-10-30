// SPDX-License-Identifier: MIT
use super::*;
use si_mem::{Ret, Trace};

/// The elements' data from the head, following the next links `count` times.
fn forward(m: &Mem, list: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut e = m.u32(list + HEAD);
    for _ in 0..m.i32(list + COUNT).max(0) {
        out.push(m.u32(e + DATA));
        e = m.u32(e + NEXT);
    }
    out
}

/// The same backwards from the tail.
fn backward(m: &Mem, list: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut e = m.u32(list + TAIL);
    for _ in 0..m.i32(list + COUNT).max(0) {
        out.push(m.u32(e + DATA));
        e = m.u32(e + PREV);
    }
    out
}

#[test]
fn inserts_at_both_ends_keep_the_ring() {
    let mut m = Mem::new();
    let l = create(&mut m);
    assert_ne!(l, 0);
    assert_eq!(insert_e(&mut m, l, 2), m.u32(l + TAIL));
    insert_e(&mut m, l, 3);
    insert_b(&mut m, l, 1);
    assert_eq!(forward(&m, l), [1, 2, 3]);
    assert_eq!(backward(&m, l), [3, 2, 1]);
    let (h, t) = (m.u32(l + HEAD), m.u32(l + TAIL));
    assert_eq!(m.u32(h + PREV), t);
    assert_eq!(m.u32(t + NEXT), h);
    assert_eq!(m.u32(h + LIST), l);
    assert_eq!(insert_e(&mut m, 0, 5), 0);
}

#[test]
fn iteration_flags() {
    let mut m = Mem::new();
    let l = create(&mut m);
    let f = m.local(4);
    assert_eq!(iter_init(&mut m, l, f, f + 2), 0);
    assert_eq!((m.i16(f), m.i16(f + 2)), (1, 1));
    for d in [10, 20, 30] {
        insert_e(&mut m, l, d);
    }
    let h = iter_init(&mut m, l, f, f + 2);
    assert_eq!((m.u32(h), m.i16(f), m.i16(f + 2)), (10, 0, 0));
    let e = iter_next(&mut m, l, f, f + 2);
    assert_eq!((m.u32(e), m.i16(f), m.i16(f + 2)), (20, 0, 0));
    iter_next(&mut m, l, f, f + 2);
    let e = iter_next(&mut m, l, f, f + 2);
    assert_eq!((e, m.i16(f), m.i16(f + 2)), (h, 1, 1));
    let e = iter_prev(&mut m, l, f, f + 2);
    assert_eq!(m.u32(e), 30);
}

#[test]
fn sorted_insert_stops_where_the_comparison_answers_zero() {
    let mut m = Mem::new();
    let l = create(&mut m);
    // a comparison at a made-up address: "go on" while the new data is larger
    let mut o = Trace::with(|_, r, a| Ret::eax(if r.addr == 0x1234 { (a[0] > a[1]) as u32 } else { 0 }));
    for d in [5, 1, 9, 3, 7] {
        insert_s(&mut m, &mut o, l, d, 0x1234);
    }
    assert_eq!(forward(&m, l), [1, 3, 5, 7, 9]);
    assert_eq!(backward(&m, l), [9, 7, 5, 3, 1]);
    assert_eq!(m.i32(l + COUNT), 5);
}

#[test]
fn copy_counts_twice() {
    let mut m = Mem::new();
    let l = create(&mut m);
    for d in [1, 2, 3] {
        insert_e(&mut m, l, d);
    }
    set_flags(&mut m, l, 7);
    // copy: data + 100; comparison: always "go on" (append)
    let mut o = Trace::with(|_, r, a| Ret::eax(if r.addr == 0x2000 { a[0] + 100 } else { 1 }));
    let c = copy(&mut m, &mut o, l, 0x2000, 0x3000);
    assert_eq!(m.i32(c + COUNT), 6);
    assert_eq!(get_flags(&m, c), 7);
    let mut e = m.u32(c + HEAD);
    let mut got = Vec::new();
    for _ in 0..3 {
        got.push(m.u32(e));
        e = m.u32(e + NEXT);
    }
    assert_eq!(got, [101, 102, 103]);
}

#[test]
fn remove_answers_and_links() {
    let mut m = Mem::new();
    let l = create(&mut m);
    let es: Vec<u32> = [1, 2, 3, 4].iter().map(|d| insert_e(&mut m, l, *d)).collect();
    let mut o = Trace::new();
    assert_eq!(remove(&mut m, &mut o, l, es[0], 0), es[1]);
    assert_eq!(remove(&mut m, &mut o, l, es[3], 0), es[2]);
    assert_eq!(forward(&m, l), [2, 3]);
    assert_eq!(remove(&mut m, &mut o, l, es[2], 0x4000), es[1]);
    assert_eq!(o.calls.len(), 1);
    assert_eq!(o.calls[0].args, [es[2]]);
    assert_eq!(forward(&m, l), [2]);
    assert_eq!(m.u32(es[1] + NEXT), es[1]);
}

#[test]
fn find_nth_and_elem_of() {
    let mut m = Mem::new();
    let l = create(&mut m);
    let es: Vec<u32> = [4, 5, 6].iter().map(|d| insert_e(&mut m, l, *d)).collect();
    assert_eq!(find_nth(&mut m, l, 0), es[0]);
    assert_eq!(find_nth(&mut m, l, 2), es[2]);
    assert_eq!(find_nth(&mut m, l, 3), 0);
    assert_eq!(find_nth(&mut m, l, -1), 0);
    let mut o = Trace::with(|_, _, a| Ret::eax((a[0] == a[1]) as u32));
    assert_eq!(elem_of(&mut m, &mut o, l, 5, 0x5000), es[1]);
    assert_eq!(elem_of(&mut m, &mut o, l, 8, 0x5000), 0);
}

#[test]
fn destroy_hands_every_element_to_the_destructor() {
    let mut m = Mem::new();
    let l = create(&mut m);
    let es: Vec<u32> = [1, 2, 3].iter().map(|d| insert_e(&mut m, l, *d)).collect();
    let p = m.local(4);
    m.set_u32(p, l);
    let mut o = Trace::new();
    destroy(&mut m, &mut o, p, 0x6000);
    assert_eq!(m.u32(p), 0);
    let got: Vec<u32> = o.calls.iter().map(|c| c.args[0]).collect();
    assert_eq!(got, es);
}

#[test]
fn dump_prints_through_fprintf() {
    let mut m = Mem::new();
    let l = create(&mut m);
    insert_e(&mut m, l, 1);
    insert_e(&mut m, l, 2);
    let mut o = Trace::new();
    dump(&mut m, &mut o, l, 0x7000, 0x9999);
    let formats: Vec<u32> = o.calls.iter().filter(|c| c.routine == FPRINTF).map(|c| c.args[1]).collect();
    assert_eq!(formats[..4], [text::DUMPING, text::COUNT, text::HEAD_TAIL, text::ITEM]);
    assert_eq!(*formats.last().unwrap(), text::FINISHED);
    assert!(formats.contains(&text::LAST_ITEM));
    assert_eq!(o.calls.iter().filter(|c| c.routine.addr == 0x7000).count(), 2);
}
