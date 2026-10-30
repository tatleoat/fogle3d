// SPDX-License-Identifier: MIT
//! Unit tests on made-up data.

use crate::*;
use si_mem::Mem;

#[test]
fn cardinal_weights_sum_to_one_at_the_keys() {
    // at t = 0 the blend is the second key, at t = 1 the third
    let w = cluster::cardinal_coeff(0.0);
    assert_eq!([w[2], w[3], w[4], w[5]], [0.0, 1.0, 0.0, 0.0]);
    let w = cluster::cardinal_coeff(1.0);
    assert_eq!([w[2], w[3], w[4], w[5]], [0.0, 0.0, 1.0, 0.0]);
    let w = cluster::cardinal_coeff(0.5);
    assert_eq!(w[2] + w[3] + w[4] + w[5], 1.0);
}

#[test]
fn a_spline_halfway_between_two_keys() {
    let mut m = Mem::new();
    let mut o = NoOutside::default();
    let mk = |m: &mut Mem, v: f32| {
        let keys = m.place(0x30, 8);
        for c in 0..9 {
            m.set_f32(keys + 4 * c, v);
        }
        let s = m.place(0x74, 8);
        m.set_u16(s + 0x10, 1);
        m.set_u32(s + 0x14, keys);
        Ptr32::new(s)
    };
    let (a, b, d) = (mk(&mut m, 2.0), mk(&mut m, 4.0), mk(&mut m, 0.0));
    let r = shape::spline_interpolate(&mut m, &mut o, a, b, 0.5, d);
    assert_eq!(r, d);
    let keys = m.u32(d.addr() + 0x14);
    assert!((0..9).all(|c| m.f32(keys + 4 * c) == 3.0));
    assert_eq!(o.calls.len(), 1);
    assert_eq!(o.calls[0].0.name, "SI_splineTouch");
}

#[test]
fn frames_of_a_time() {
    let mut m = Mem::new();
    m[globals::SI_FRAME_RATE] = 24.0;
    assert_eq!(actions::actions_sec_to_start_frm(&m, 1.0), 25);
    assert_eq!(actions::actions_sec_to_end_frm(&m, 1.0), 24);
    assert_eq!(actions::actions_sec_to_start_frm(&m, -1.0), -24);
    assert_eq!(actions::actions_sec_to_end_frm(&m, -1.0), -25);
}

/// An action list of `n` actions with names, made in the image.
fn action_list(m: &mut Mem, names: &[&[u8]]) -> Ptr32<actions::Actions> {
    let a = m.place(8, 8);
    let elems = if names.is_empty() { 0 } else { m.place(0x30 * names.len() as u32, 8) };
    for (i, n) in names.iter().enumerate() {
        let s = m.place(n.len() as u32 + 1, 8);
        m.put_cstr(s, n);
        m.set_u32(elems + 0x30 * i as u32, s);
    }
    m.set_u32(a, names.len() as u32);
    m.set_u32(a + 4, elems);
    Ptr32::new(a)
}

#[test]
fn a_new_action_name_is_one_more_than_the_highest() {
    let mut m = Mem::new();
    let a = action_list(&mut m, &[b"walk", b"walk_3", b"run", b"walk7"]);
    let name = m.place(8, 8);
    m.put_cstr(name, b"walk_2");
    let r = actions::actions_get_new_name(&mut m, a, name);
    assert_eq!(m.cstr(r), b"walk_8");
    let r = actions::actions_get_new_name(&mut m, a, 0);
    assert_eq!(m.cstr(r), b"action5");
}

#[test]
fn adding_an_action_keeps_the_list_in_time_order() {
    let mut m = Mem::new();
    let a = action_list(&mut m, &[]);
    assert_eq!(actions::actions_add_elem_time(&mut m, a, 0, 2.0, 3.0, 1, 0), 0);
    assert_eq!(actions::actions_add_elem_time(&mut m, a, 0, 0.0, 1.0, 1, 0), 0);
    assert_eq!(actions::actions_add_elem_time(&mut m, a, 0, 4.0, 3.0, 1, 0), -1);
    let elems = m.u32(a.addr() + 4);
    assert_eq!((m.f32(elems + 8), m.f32(elems + 0x38)), (0.0, 2.0));
    assert_eq!(m.u32(a.addr()), 2);
}

#[test]
fn the_walk_hands_a_step_it_does_not_have_to_the_trait() {
    // an element with no relations, no sons: nothing is called but the look-ups; a step at
    // an address of no routine of the crate is answered 0 by the default `call_addr`
    let mut m = Mem::new();
    let mut o = NoOutside::default();
    let e = m.place(0x84, 8);
    walk::fcv_step(&mut m, &mut o, 0x0812_3456, 0x1000, 0, -1);
    assert!(o.calls.is_empty());
    walk::hieranim_fcv_process(&mut m, &mut o, 0, walk::step::REMOVE, e, 0, -1, 0);
    let names: Vec<_> = o.calls.iter().map(|c| c.0.name).collect();
    assert_eq!(names, ["SI_elemdictFindRangeRelatedElem"; 3].iter().copied().chain(["SI_edFindSon"]).collect::<Vec<_>>());
}

#[test]
fn a_curve_without_keys_is_noted_and_the_last_key_time_kept() {
    let mut m = Mem::new();
    let fcv = m.place(0x20, 8);
    let flag = m.place(4, 8);
    m.set_i32(fcv + 0xc, 0);
    walk::ha_fcv_process_check_empty(&mut m, fcv, flag);
    assert_eq!(m.u16(flag), 1);
    let keys = m.place(0x48, 8);
    m.set_f32(keys, 0.5);
    m.set_f32(keys + 0x24, 2.0);
    m.set_u32(fcv + 8, keys);
    m.set_i32(fcv + 0xc, 2);
    let t = m.place(4, 8);
    m.set_f32(t, 1.0);
    walk::ha_fcv_process_last_key_time(&mut m, fcv, t);
    assert_eq!(m.f32(t), 2.0);
}

#[test]
fn a_new_name_is_refused_when_another_action_has_it() {
    let mut m = Mem::new();
    let names: Vec<u32> = ["walk", "run"].iter().map(|s| m.place_cstr(s.as_bytes())).collect();
    let elems = m.place(0x60, 8);
    m.set_u32(elems, names[0]);
    m.set_u32(elems + 0x30, names[1]);
    let a = m.place(8, 8);
    m.set_i32(a, 2);
    m.set_u32(a + 4, elems);
    let a = Ptr32::new(a);
    let run = m.place_cstr(b"run");
    assert_eq!(actedit::actions_elem_set_name(&mut m, a, 0, run), 0);
    let jump = m.place_cstr(b"jump");
    assert_eq!(actedit::actions_elem_set_name(&mut m, a, 0, jump), 1);
    assert_eq!(m.cstr(m.u32(elems)), b"jump");
}
