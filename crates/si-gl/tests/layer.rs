// SPDX-License-Identifier: MIT
//! The layer driven through the recording back end on made-up answers. Nothing here comes
//! from the original binary; the comparison with the original is `re/tools/si-gl/check.sh`.

use si_gl::event::{keysym_device, INPUTCHANGE, KEYBD, LEFTMOUSE, MOUSEX, MOUSEY, REDRAW, WINQUIT};
use si_gl::rec::{SharedLog, Val};
use si_gl::sys::{Exit, Sys};
use si_gl::IrisGl;

fn layer() -> (IrisGl, SharedLog) {
    let (sys, log) = Sys::recording();
    {
        let mut l = log.borrow_mut();
        for (name, v) in [
            ("XInitThreads", 1),
            ("XOpenDisplay", 0x1000),
            ("screen.root", 0x26),
            ("screen.width", 1280),
            ("screen.height", 1024),
            ("glXChooseVisual", 0x2000),
            ("glXCreateContext", 0x4001),
            ("glXGetCurrentDrawable", 0x5001),
            ("glGetError", 0x504),
            ("XCreateWindow", 0x7003),
        ] {
            l.answer_always(name, vec![Val::U(v)]);
        }
        l.answer_always("XGetGeometry", vec![Val::U(1), Val::W(vec![0x26, 10, 20, 300, 200, 0, 24])]);
        l.answer_always("XQueryPointer", vec![Val::U(1), Val::W(vec![0x26, 0, 400, 300, 0, 0, 0])]);
        l.answer_always("glGetString", vec![Val::S(b"Somebody".to_vec())]);
        for a in [0x101, 0x102, 0x103] {
            l.answer("XInternAtom", vec![Val::U(a)]);
        }
    }
    (IrisGl::new(sys), log)
}

fn event(words: &[(usize, u32)]) -> Vec<Val> {
    let mut w = vec![0u32; 24];
    for &(i, v) in words {
        w[i] = v;
    }
    vec![Val::U(0), Val::W(w)]
}

#[test]
fn start_up_happens_on_the_first_call_in_the_original_order() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    assert!(g.context().is_none());
    assert_eq!(g.winget(), -1);
    let names = log.borrow().names();
    let head = ["getenv", "XInitThreads", "getenv", "XOpenDisplay", "XInternAtom", "XInternAtom", "XInternAtom", "glXChooseVisual", "glXCreateContext", "XCreateColormap", "XCreateWindow", "glXMakeCurrent", "XSync", "glGetString", "glGetString", "glXGetClientString"];
    assert_eq!(&names[..head.len()], &head);
    assert_eq!(log.borrow().lines()[3], "XOpenDisplay(\":0.0\")");
    let c = g.context().unwrap();
    assert_eq!((c.atoms.wm_delete_window, c.atoms.motif_wm_hints, c.atoms.wm_state), (0x101, 0x102, 0x103));
    assert_eq!((c.screen_width, c.screen_height), (1280, 1024));
    assert_eq!(c.fnt.fonts.len(), 1);
    assert_eq!((c.fnt.fonts[0].height, c.fnt.fonts[0].chars.len()), (15, 128));
    assert_eq!(c.crs.cursors.len(), 2);
    assert_eq!(c.crs.cursors[0].id, -1);
    assert_eq!(c.evt.devices[REDRAW as usize].queued, 1);
    assert_eq!(c.evt.devices[INPUTCHANGE as usize].queued, 1);
    assert_eq!(c.evt.devices.iter().filter(|d| d.queued != 0).count(), 2);
    // the colour map: sixteen fixed colours, a grey ramp from 32, a cube from 56
    assert_eq!(&c.gfx.cmap[3..6], &[255, 0, 0]);
    assert_eq!(c.gfx.cmap[32 * 3], 10);
    assert_eq!(&c.gfx.cmap[255 * 3..255 * 3 + 3], &[4 * 51, 255, 4 * 51]);
}

#[test]
fn window_identifiers_are_the_smallest_free_ones() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, _log) = layer();
    assert_eq!(g.winopen(b"a"), 1);
    assert_eq!(g.winopen(b"b"), 2);
    assert_eq!(g.winopen(b"c"), 3);
    assert_eq!(g.winget(), 3);
    g.winclose(2);
    assert_eq!(g.winget(), 3);
    assert_eq!(g.winopen(b"d"), 2);
    g.winclose(2);
    // the current window was closed: the first other window of the table takes over
    assert_eq!(g.winget(), 1);
    assert_eq!(g.swinopen(9), -1);
}

#[test]
fn keysyms_map_to_iris_gl_devices() {
    assert_eq!(keysym_device(0x61), 11); // a
    assert_eq!(keysym_device(0xff1b), 7); // Escape
    assert_eq!(keysym_device(0x20), 0x53); // space
    assert_eq!(keysym_device(0x41), 0); // an upper-case letter is not in the table
    assert_eq!(si_gl::data::KEYSYM_DEVICE.len(), 124);
    assert!(si_gl::data::KEYSYM_DEVICE.windows(2).all(|w| w[0].0 < w[1].0));
}

#[test]
fn a_button_press_queues_the_button_and_its_tied_valuators() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    let id = g.winopen(b"w");
    g.qdevice(LEFTMOUSE);
    g.tie(LEFTMOUSE, MOUSEX, MOUSEY);
    assert_eq!(g.context().unwrap().wnd.event_mask, 0xc);
    {
        let mut l = log.borrow_mut();
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 4), (4, 0x7003), (13, 1)]));
    }
    assert_eq!(g.qtest(), LEFTMOUSE);
    assert_eq!(g.qread(), (LEFTMOUSE, 1));
    assert_eq!(g.qread(), (MOUSEX, 400));
    assert_eq!(g.qread(), (MOUSEY, 1024 - 301));
    assert_eq!(g.qtest(), 0);
    let _ = id;
}

#[test]
fn redraw_comes_from_configure_events_and_from_an_expose_only_by_accident() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    let id = g.winopen(b"w") as i16;
    {
        let mut l = log.borrow_mut();
        // an Expose for the window: offset 0x14 is its x coordinate, not a window
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 12), (4, 0x7003), (5, 17)]));
        // a ConfigureNotify: offset 0x14 is the window
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 22), (4, 0x7003), (5, 0x7003)]));
        // an Expose whose x happens to be the window's identifier
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 12), (4, 0x7003), (5, 0x7003)]));
    }
    assert_eq!(g.qread(), (REDRAW, id));
    assert_eq!(g.qread(), (REDRAW, id));
    assert_eq!(g.qtest(), 0);
}

#[test]
fn a_key_press_gives_the_key_and_the_character() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (sys, log) = Sys::recording();
    {
        let mut l = log.borrow_mut();
        for (name, v) in [("XInitThreads", 1), ("XOpenDisplay", 0x1000), ("glXChooseVisual", 0x2000), ("glXCreateContext", 0x4001), ("glXGetCurrentDrawable", 1)] {
            l.answer_always(name, vec![Val::U(v)]);
        }
        for keycode in 0..256u32 {
            l.answer("XKeycodeToKeysym", vec![Val::U(if keycode == 38 { 0x61 } else { 0 })]);
        }
    }
    let mut g = IrisGl::new(sys);
    g.qdevice(11);
    g.qdevice(KEYBD);
    {
        let mut l = log.borrow_mut();
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 2), (13, 38)]));
        l.answer("XLookupString", vec![Val::U(1), Val::U(b'a' as u32)]);
        l.answer("XEventsQueued", vec![Val::U(1)]);
        l.answer("XNextEvent", event(&[(0, 3), (13, 38)]));
    }
    assert_eq!(g.qread(), (11, 1));
    assert_eq!(g.qread(), (KEYBD, b'a' as i16));
    assert_eq!(g.qread(), (11, 0));
    assert_eq!(g.context().unwrap().evt.keycode_of[11], 38);
}

#[test]
fn the_close_request_ends_the_program_unless_winquit_is_queued() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    let id = g.winopen(b"w") as i16;
    let close = || event(&[(0, 33), (4, 0x7003), (7, 0x101)]);
    g.qdevice(WINQUIT);
    log.borrow_mut().answer("XEventsQueued", vec![Val::U(1)]);
    log.borrow_mut().answer("XNextEvent", close());
    assert_eq!(g.qread(), (WINQUIT, id));
    g.unqdevice(WINQUIT);
    log.borrow_mut().answer("XEventsQueued", vec![Val::U(1)]);
    log.borrow_mut().answer("XNextEvent", close());
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.qtest()));
    let _ = std::panic::take_hook();
    assert_eq!(r.unwrap_err().downcast_ref::<Exit>(), Some(&Exit(0)));
    assert_eq!(log.borrow().lines().last().unwrap(), "exit(0x0)");
}

#[test]
fn colour_map_mode_is_a_table_in_the_layer() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    g.winopen(b"w");
    log.borrow_mut().clear();
    g.color(1);
    assert_eq!(log.borrow().lines().last().unwrap(), "glColor4d(d:3ff0000000000000, d:0000000000000000, d:0000000000000000, d:3ff0000000000000)");
    g.mapcolor(1, 0, 255, 0);
    g.color(1);
    assert_eq!(log.borrow().lines().last().unwrap(), "glColor4d(d:0000000000000000, d:3ff0000000000000, d:0000000000000000, d:3ff0000000000000)");
    assert_eq!(g.getmcolor(1), Some((0, 255, 0)));
    // the write mask is applied to the index
    g.writemask(0);
    assert_eq!(g.grgbcolor(), Some((0, 0, 0)));
    assert_eq!(g.getcolor(), 1);
}

#[test]
fn a_defined_cursor_can_never_be_selected() {
    // the original's tables are read from the user's install (unit rta)
    si_rt::skip_without_install!();
    let (mut g, log) = layer();
    g.winopen(b"w");
    g.defcursor(5, &[0xffff; 16]);
    assert_eq!(g.context().unwrap().crs.cursors.len(), 3);
    assert_eq!(g.context().unwrap().crs.cursors[2].id, 0);
    log.borrow_mut().clear();
    g.setcursor(5);
    assert!(!log.borrow().names().contains(&"XDefineCursor"));
    assert_eq!(g.getcursor(), Some((0, 1)));
}

#[test]
fn lookat_avoids_glu_for_a_vertical_view() {
    let (mut g, log) = layer();
    g.lookat(1.0, 5.0, 2.0, 1.0, 0.0, 2.0, 900);
    assert_eq!(log.borrow().names(), ["glRotated", "glRotated", "glTranslatef"]);
    log.borrow_mut().clear();
    g.lookat(1.0, 5.0, 2.0, 0.0, 0.0, 0.0, 900);
    assert_eq!(log.borrow().names(), ["glRotated", "gluLookAt"]);
}

#[test]
fn the_unimplemented_calls_are_empty() {
    let (mut g, log) = layer();
    assert_eq!(si_gl::unimplemented::STUBS.len(), 291);
    let mut counts = [0; 4];
    for (_, f) in si_gl::unimplemented::STUBS {
        match f(&mut g) {
            None => counts[0] += 1,
            Some(0) => counts[1] += 1,
            Some(-1) => counts[2] += 1,
            Some(1) => counts[3] += 1,
            Some(v) => panic!("a stub returns {v}"),
        }
    }
    assert_eq!(counts, [249, 36, 5, 1]);
    assert_eq!(g.ismex(), 1);
    assert_eq!(g.dopup(), -1);
    assert!(log.borrow().calls.is_empty());
    assert!(g.context().is_none());
}
