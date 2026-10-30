// SPDX-License-Identifier: MIT
//! The toolkit driven through the recording stand-ins on made-up memory. Nothing here comes
//! from the original binary; the comparison with the original is `re/tools/si-ui/check.sh`.

use si_ui::globals::*;
use si_ui::mem::{Mem, ARENA, P};
use si_ui::rec::{Answer, SharedLog};
use si_ui::records::{ev, Canvas, Event, Grafport};
use si_ui::sys::Outside;
use si_ui::Ui;

fn ui() -> (Ui, SharedLog) {
    let (o, log) = Outside::recording();
    (Ui::new(o), log)
}

fn ans(vals: &[u64]) -> Answer {
    Answer { vals: vals.to_vec(), pokes: vec![] }
}

/// The standard layout's four windows in the arena, with cameras and no rulers shown.
fn four_views(m: &mut Mem) -> [P<Grafport>; 4] {
    let base: P<Grafport> = P::at(ARENA.0 + 0x1000);
    let boxes = [(130, 638, 89, 530), (130, 638, 533, 974), (641, 1149, 89, 530), (641, 1149, 533, 974)];
    for (i, (l, r, b, t)) in boxes.into_iter().enumerate() {
        let g = base.add(i as i32);
        let camera = ARENA.0 + 0x4000 + 0x200 * i as u32;
        m.set_i16(camera + 4, 2000 + i as i16);
        let w = &mut m[g];
        w.id = 1000 + i as i16;
        w.visible = 1;
        w.enabled = 1;
        w.camera = camera;
        w.ctrl = camera + 0x180;
        w.d_050 = -1;
        (w.left, w.right, w.bottom, w.top) = (l, r, b, t);
    }
    m[SI_TEMPLATE].n_views = 4;
    m[SI_TEMPLATE].views = base;
    m[SI_TITLE_HEIGHT] = 20;
    [base, base.add(1), base.add(2), base.add(3)]
}

#[test]
fn a_window_is_placed_with_its_drawing_area_inside() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    let g = four_views(&mut ui.m)[0];
    ui.window_set_graf(g, 130, 638, 89, 530);
    let w = ui.m[g];
    assert_eq!((w.view_left, w.view_right, w.view_bottom, w.view_top), (131, 637, 90, 510));
    assert_eq!(w.view2, [131, 637, 90, 510]);
    // the small screen: x by 0.8, y by 0.75, truncated
    ui.m[SI_SCREEN_RES] = 1;
    ui.window_set_graf(g, 130, 638, 89, 530);
    let w = ui.m[g];
    assert_eq!((w.left, w.right, w.bottom, w.top), (104, 510, 66, 397));
}

#[test]
fn a_perspective_view_keeps_its_aspect_ratio() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    let g = four_views(&mut ui.m)[3];
    let camera = ui.m[g].camera;
    ui.m.set_i16(camera + 4, 2003);
    ui.m.set_bytes(camera + 0x64, &1.5f32.to_le_bytes());
    ui.si_get_window_viewport(g);
    let w = ui.m[g];
    // 506 wide, 420 high: narrower than 1.5, so the full width and 506 / 1.5 high
    assert_eq!((w.view_left, w.view_right), (642, 1148));
    assert_eq!((w.view_bottom, w.view_top), (575, 912));
}

#[test]
fn queue_entries_are_classified_by_device() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    assert_eq!(ui.si_fnd_event(0x67, 1), 3);
    assert_eq!(ui.si_fnd_event(0x67, 0), 4);
    assert_eq!(ui.si_fnd_event(20, 1), 1);
    assert_eq!(ui.si_fnd_event(20, 0), 2);
    assert_eq!(ui.si_fnd_event(4, 0), 1);
    assert_eq!(ui.si_fnd_event(0x69, 1), 5);
    assert_eq!(ui.si_fnd_event(0x201, 0), 12);
    assert_eq!(ui.si_fnd_event(0x210, 0), 7);
    assert_eq!(ui.si_fnd_event(13, 1), 0);
}

#[test]
fn pushed_events_come_back_newest_first() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    ui.si_push_qe(1, 0, 0, 0, 0, 0);
    ui.si_push_qe(3, 7, 8, 9, 10, 0x1234);
    assert_eq!(ui.m[NB_E], 2);
    let e = ui.pop_qe();
    assert_eq!((ui.m[e].kind, ui.m[e].first, ui.m[e].data), (3, 7, 0x1234));
    let e = ui.pop_qe();
    assert_eq!(ui.m[e].kind, 1);
    assert_eq!(ui.m[IS_E], 0);
}

#[test]
fn a_second_press_close_in_time_and_place_is_a_double_click() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, log) = ui();
    let event: P<Event> = P::at(ARENA.0 + 0x100);
    let canvas: P<Canvas> = P::at(ARENA.0 + 0x200);
    ui.m[SI_ACTIVE_EVENT] = event;
    ui.m[SI_ACTIVE_CANVAS] = canvas;
    ui.m[SI_SYSID] = 1;
    log.borrow_mut().answer("times", ans(&[1000]));
    log.borrow_mut().answer("times", ans(&[1020]));
    log.borrow_mut().answer("times", ans(&[1100]));
    ui.si_mouse_down_(0x67, 1, 500, 400);
    assert_eq!(ui.si_double_click_status(), 0);
    ui.si_mouse_down_(0x67, 1, 501, 400);
    assert_eq!(ui.si_double_click_status(), 1);
    ui.si_mouse_down_(0x67, 1, 501, 400);
    assert_eq!(ui.si_double_click_status(), 0);
}

#[test]
fn a_click_on_a_title_bar_finds_its_part() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    let g = four_views(&mut ui.m)[1];
    ui.m[g].view_top = 954;
    // the selection box, the zoom box, the title
    assert_eq!(ui.fnd_view_part(g, 135, 960), 2);
    assert_eq!(ui.fnd_view_part(g, 630, 960), 3);
    assert_eq!(ui.fnd_view_part(g, 200, 960), 9);
    // the drawing area
    assert_eq!(ui.fnd_view_part(g, 300, 700), 0);
}

#[test]
fn selecting_a_window_deselects_the_other() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, log) = ui();
    let g = four_views(&mut ui.m);
    ui.m[g[2]].hilite = 1;
    ui.window_toggle_select(g[0]);
    assert_eq!((ui.m[g[0]].hilite, ui.m[g[2]].hilite), (1, 0));
    let mut selected = P::NULL;
    ui.si_window_get_hilite(&mut selected);
    assert_eq!(selected, g[0]);
    assert_eq!(log.borrow().lines.iter().filter(|l| l.starts_with("  sboxi(")).count(), 4);
}

#[test]
fn zooming_saves_the_place_and_unzooming_restores_it() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    let g = four_views(&mut ui.m);
    let t = &mut ui.m[SI_TEMPLATE];
    (t.views_left, t.views_right, t.views_bottom, t.views_top) = (128, 1151, 87, 976);
    ui.window_small_to_big(g[3]);
    let w = ui.m[g[3]];
    assert_eq!((w.zoomed, w.left, w.right, w.bottom, w.top), (1, 130, 1149, 89, 974));
    ui.window_big_to_small(g[3]);
    let w = ui.m[g[3]];
    assert_eq!((w.zoomed, w.left, w.right, w.bottom, w.top), (0, 641, 1149, 533, 974));
}

#[test]
fn keys_become_characters() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, _) = ui();
    let out: P<u8> = P::at(ARENA.0 + 0x10);
    let mut c = |dev, shift, caps| {
        ui.si_keyboard_to_car(dev, shift, caps, out);
        ui.m[out]
    };
    assert_eq!(c(11, 0, 0), b'a');
    assert_eq!(c(11, 0, 1), b'A');
    assert_eq!(c(8, 1, 0), b'!');
    assert_eq!(c(8, 0, 1), b'1');
    assert_eq!(c(0x53, 0, 0), b' ');
    assert_eq!(c(200, 0, 0), b'?');
}

#[test]
fn the_escape_key_makes_an_event_on_the_way_up() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut ui, log) = ui();
    let event: P<Event> = P::at(ARENA.0 + 0x100);
    ui.m[SI_ACTIVE_EVENT] = event;
    log.borrow_mut().answer("getvaluator", ans(&[640]));
    assert_eq!(ui.key_down(7, 1), 0);
    ui.key_up(7);
    let e = ui.m[event];
    assert_eq!((e.kind, e.second, e.x), (ev::ESCAPE, 0x1f, 640));
}
