// SPDX-License-Identifier: MIT
//! The checks of the event reader, the server, the main loop, the emergency save and the
//! batch renderer.

use super::super::*;
pub use super::{handle_of, last_mem};
use super::PlainClients;
use si_x87::vectors::Word;

fn vals(c: &Case, name: &str) -> Vec<u32> {
    c.input(name).values().iter().map(|&v| v as u32).collect()
}

fn after(c: &Case, name: &str) -> Vec<u32> {
    c.after(name).values().iter().map(|&v| v as u32).collect()
}

fn event_of(w: &[u32]) -> Event {
    Event { kind: w[0] as i32, a: w[1] as i16, b: (w[1] >> 16) as i16, c: w[2] as i16, d: (w[2] >> 16) as i16, data: w[3], word10: w[4], device: w[5] as i32 }
}

fn words_of(e: &Event) -> [u32; 6] {
    [e.kind as u32, e.a as u16 as u32 | (e.b as u16 as u32) << 16, e.c as u16 as u32 | (e.d as u16 as u32) << 16, e.data, e.word10, e.device as u32]
}

fn load_queue(c: &Case, a: &mut A) {
    let qe = vals(c, "qe");
    for k in 0..10 {
        a.g.events.posted[k] = event_of(&qe[6 * k..6 * k + 6]);
    }
    a.g.events.posted_count = vals(c, "nbe")[0] as i16;
    a.g.events.is_posted = vals(c, "ise")[0] as i16;
    a.g.events.taken = event_of(&vals(c, "devt"));
}

fn queue_after(d: &mut Diff, c: &Case, a: &A) {
    let qe = after(c, "qe");
    for k in 0..10 {
        d.eq(&format!("qE[{k}]"), words_of(&a.g.events.posted[k]).to_vec(), qe[6 * k..6 * k + 6].to_vec());
    }
    d.eq("nbE", a.g.events.posted_count, after(c, "nbe")[0] as i16);
    d.eq("isE", a.g.events.is_posted, after(c, "ise")[0] as i16);
    d.eq("dEvt", words_of(&a.g.events.taken).to_vec(), after(c, "devt"));
}

fn load_reader(c: &Case, a: &mut A) {
    let e = &mut a.g.events;
    e.supra_mode = vals(c, "supra")[0] as i32;
    e.swiftmode = vals(c, "swift")[0] as i16;
    e.active = event_of(&vals(c, "ev"));
    e.double_click = vals(c, "dclick")[0] as i16;
    e.redraw_rotoscopy = vals(c, "roto")[0] as i16;
    e.redraws_3d = vals(c, "r3d")[0] as i16;
    a.g.active = vals(c, "active")[0] as i16;
    a.g.tablet_active = vals(c, "tablet")[0] as i32;
    a.g.sysid = vals(c, "sysid")[0] as i32;
    let mode = vals(c, "mdlmod")[0] as i64;
    a.w.answer("mem_mdl_mod", &[mode]);
}

fn reader_after(d: &mut Diff, c: &Case, a: &A) {
    let e = &a.g.events;
    d.eq("swiftmode", e.swiftmode, after(c, "swift")[0] as i16);
    d.eq("active event", words_of(&e.active).to_vec(), after(c, "ev"));
    d.eq("doubleClick", e.double_click, after(c, "dclick")[0] as i16);
    d.eq("gbRedrawRotoscopy", e.redraw_rotoscopy, after(c, "roto")[0] as i16);
    d.eq("gn3DRedrawsNeeded", e.redraws_3d, after(c, "r3d")[0] as i16);
    d.eq("SI_active", a.g.active, after(c, "active")[0] as i16);
    let before = vals(c, "tplwin")[0] as i16;
    d.eq("template window", last_mem(a, "mem_template_set_window").map(|v| v as i16).unwrap_or(before), after(c, "tplwin")[0] as i16);
}

/// Which record the returned pointer names.
fn returned(c: &Case, field: &str) -> Option<EventRef> {
    match &c.output(field).words()[0] {
        Word::Ref { name, offset: 0 } if name == "ev" => Some(EventRef::Active),
        Word::Ref { name, offset: 0 } if name == "devt" => Some(EventRef::Posted),
        _ => None,
    }
}

fn load_net(c: &Case, a: &mut A) {
    let b = c.input("net").to_bytes();
    for k in 0..5 {
        let e = &b[16 * k..16 * k + 16];
        a.g.network[k] = NetEntry {
            started: i16::from_le_bytes([e[0], e[1]]),
            state: i16::from_le_bytes([e[2], e[3]]),
            module: i16::from_le_bytes([e[4], e[5]]),
            canvas: handle_of(u32::from_le_bytes([e[8], e[9], e[10], e[11]])),
            disabled: i16::from_le_bytes([e[12], e[13]]),
        };
    }
}

fn net_after(d: &mut Diff, c: &Case, a: &A) {
    let b = c.after("net").to_bytes();
    for k in 0..5 {
        let e = &b[16 * k..16 * k + 16];
        let n = &a.g.network[k];
        d.eq(
            &format!("network[{k}]"),
            (n.started, n.state, n.module, n.canvas, n.disabled),
            (
                i16::from_le_bytes([e[0], e[1]]),
                i16::from_le_bytes([e[2], e[3]]),
                i16::from_le_bytes([e[4], e[5]]),
                handle_of(u32::from_le_bytes([e[8], e[9], e[10], e[11]])),
                i16::from_le_bytes([e[12], e[13]]),
            ),
        );
    }
}

pub fn load_screen(c: &Case, a: &mut A) {
    a.g.bit_planes = vals(c, "planes")[0] as i32;
    a.g.background = vals(c, "bg")[0] as i32;
    a.g.sysid = vals(c, "sysid")[0] as i32;
    let m = vals(c, "oldmouse");
    a.g.oldmouse = [m[0] as i32, m[1] as i32];
}

fn load_exit_state(c: &Case, a: &mut A) {
    load_screen(c, a);
    a.g.standalone_renderer = vals(c, "standalone")[0] as i16;
    a.g.safety_net = vals(c, "net0")[0] as i16;
    a.g.emergency.enabled = vals(c, "enabled")[0] as i16;
    a.g.emergency.handler = vals(c, "fn")[0] != 0;
    a.g.emergency.immediate_abort = vals(c, "abort")[0] as i16;
}

pub fn check(c: &Case, name: &str) -> Option<Diff> {
    let mut d = Diff::default();
    match name {
        // ------------------------------------------------------------ event.c
        "SI_fndEvent" => {
            let mut a = app(c);
            a.g.sysid = vals(c, "sysid")[0] as i32;
            d.eq("eax", a.fnd_event(i(c, "dev"), i16_in(c, "value")), eax(c));
            let before = vals(c, "tplwin")[0] as i16;
            d.eq("template window", last_mem(&a, "mem_template_set_window").map(|v| v as i16).unwrap_or(before), after(c, "tplwin")[0] as i16);
            d.outside(c, &a);
        }
        "si_pushQE" => {
            let mut a = app(c);
            load_queue(c, &mut a);
            a.push_qe(i(c, "kind"), i16_in(c, "a"), i16_in(c, "b"), i16_in(c, "c"), i16_in(c, "d"), vals(c, "data")[0]);
            queue_after(&mut d, c, &a);
        }
        "popQE" => {
            let mut a = app(c);
            load_queue(c, &mut a);
            let r = a.pop_qe();
            d.eq("returned", Some(r), returned(c, "ptr"));
            queue_after(&mut d, c, &a);
        }
        "si_getNextEvent" => {
            let mut a = app(c);
            load_reader(c, &mut a);
            let r = a.get_next_event(i16_in(c, "quick"));
            d.eq("returned", Some(r), returned(c, "ptr"));
            reader_after(&mut d, c, &a);
            d.outside(c, &a);
        }
        "SI_getMainEvent" | "SI_getEvent" => {
            let mut a = app(c);
            load_reader(c, &mut a);
            load_queue(c, &mut a);
            if name == "SI_getEvent" {
                a.get_event();
            } else {
                let r = a.get_main_event(i16_in(c, "quick"));
                d.eq("returned", Some(r), returned(c, "ptr"));
            }
            reader_after(&mut d, c, &a);
            queue_after(&mut d, c, &a);
            d.outside(c, &a);
        }
        "SI_initEvent" | "qdeviceSupraMode" | "si_disableRepeat" => {
            let mut a = app(c);
            if let Some(k) = c.try_input("keys") {
                a.g.events.supra_keys = k.values().iter().map(|&v| v as i16).collect();
            }
            if c.try_input("savedwarp").is_some() {
                a.g.events.saved_keywarp = vals(c, "savedwarp")[0] as i16;
                let k = vals(c, "keywarp");
                a.g.events.keywarp = [k[0] as i16, k[1] as i16];
            }
            match name {
                "SI_initEvent" => {
                    a.g.events.device_use = vals(c, "duse")[0] as i32;
                    a.init_event();
                    d.eq("SI_deviceUse", a.g.events.device_use, after(c, "duse")[0] as i32);
                    d.eq("active event", a.g.events.active, Event::default());
                }
                "qdeviceSupraMode" => a.qdevice_supra_mode(),
                _ => a.disable_repeat(),
            }
            if c.try_input("savedwarp").is_some() {
                d.eq("savedKeywarp", a.g.events.saved_keywarp, after(c, "savedwarp")[0] as i16);
                let k = after(c, "keywarp");
                d.eq("keywarp", a.g.events.keywarp, [k[0] as i16, k[1] as i16]);
            }
            d.outside(c, &a);
        }
        "newEvent" => {
            let zero = c.output("heap0").values().iter().all(|&v| v == 0) && c.output("heap0").len() == 6;
            d.eq("a zeroed record of 0x18 bytes", A::new_event() == Event::default() && zero, true);
        }
        // ------------------------------------------------------------ server.c
        "SI_clientActivate" => {
            let mut a = app(c);
            load_net(c, &mut a);
            load_queue(c, &mut a);
            a.g.first_time = vals(c, "first")[0] as i16;
            a.g.server_back = vals(c, "back")[0] as i16;
            a.client_activate(i16_in(c, "index"), i16_in(c, "redraw"));
            net_after(&mut d, c, &a);
            queue_after(&mut d, c, &a);
            d.eq("first_time", a.g.first_time, after(c, "first")[0] as i16);
            let h = after(c, "hilite");
            d.eq("highlight colours", a.g.button_back_hilite, [h[0] as i16, h[1] as i16]);
            d.eq("SI_activeCanvas", a.g.active_canvas, handle_of(after(c, "canvas")[0]));
            d.outside(c, &a);
        }
        "SI_serverClean" => {
            let mut a = app(c);
            load_net(c, &mut a);
            load_queue(c, &mut a);
            a.g.active = vals(c, "active")[0] as i16;
            a.server_clean(&mut PlainClients);
            queue_after(&mut d, c, &a);
            d.outside(c, &a);
        }
        "SI_softCleanup" => {
            let mut a = app(c);
            load_screen(c, &mut a);
            let e = a.soft_cleanup(i16_in(c, "crashed")).err();
            d.outside(c, &a);
            d.ending(c, e);
        }
        "SI_serverExit" | "NTHandler" => {
            let mut a = app(c);
            load_exit_state(c, &mut a);
            let e = if name == "NTHandler" { a.nt_handler(i(c, "sig")) } else { a.server_exit(i(c, "sig"), i(c, "x")) }.err();
            d.eq("safetyNet", a.g.safety_net, after(c, "net0")[0] as i16);
            d.eq("immediateAbort", a.g.emergency.immediate_abort, after(c, "abort")[0] as i16);
            d.outside(c, &a);
            d.ending(c, e);
        }
        "SI_callRender" => {
            let mut a = app(c);
            a.g.default_control.renderer = vals(c, "rend")[0] as i16;
            d.eq("eax", a.call_render(), eax(c));
            let w = after(c, "w178");
            d.eq("set-up words", [a.g.default_control.word178, a.g.default_control.word17a], [w[0] as i16, w[1] as i16]);
            d.eq("SI_cusUpdateState", a.g.cus_update_state, after(c, "cus")[0] as i32);
            d.eq("the working copy's renderer", a.g.ren.renderer, after(c, "renr")[0] as i16);
            d.outside(c, &a);
        }
        "setModuleActive" => {
            let mut a = app(c);
            load_net(c, &mut a);
            load_queue(c, &mut a);
            load_screen(c, &mut a);
            a.g.active = vals(c, "active")[0] as i16;
            a.g.active_canvas = handle_of(vals(c, "canvas")[0]);
            for k in [16u32, 17] {
                a.w.canvas_modules.insert(k, vals(c, &format!("h{k}"))[0] as i16);
            }
            let mut ev = EventRef::Active;
            let e = a.set_module_active(&mut PlainClients, i16_in(c, "module"), &mut ev).err();
            net_after(&mut d, c, &a);
            queue_after(&mut d, c, &a);
            d.outside(c, &a);
            d.ending(c, e);
        }
        "runMainLoop" => {
            let mut a = app(c);
            load_reader(c, &mut a);
            load_queue(c, &mut a);
            load_screen(c, &mut a);
            load_net(c, &mut a);
            a.g.module_redraw = vals(c, "redraw")[0] as i16;
            a.g.active_canvas = handle_of(vals(c, "canvas")[0]);
            a.g.first_time = vals(c, "first")[0] as i16;
            a.g.server_back = vals(c, "back")[0] as i16;
            for k in 0..5u32 {
                a.w.canvas_modules.insert(16 + k, vals(c, &format!("h{}", 16 + k))[0] as i16);
            }
            let mut ev = EventRef::Active;
            let mut clients = ScriptedClients::default();
            let e = a.run_main_loop(&mut clients, i(c, "done"), &mut ev).err();
            net_after(&mut d, c, &a);
            reader_after(&mut d, c, &a);
            queue_after(&mut d, c, &a);
            d.eq("moduleRedraw", a.g.module_redraw, after(c, "redraw")[0] as i16);
            d.eq("SI_activeCanvas", a.g.active_canvas, handle_of(after(c, "canvas")[0]));
            d.eq("first_time", a.g.first_time, after(c, "first")[0] as i16);
            let h = after(c, "hilite");
            d.eq("highlight colours", a.g.button_back_hilite, [h[0] as i16, h[1] as i16]);
            d.eq("event pointer", Some(ev), returned(c, "evpp"));
            d.outside(c, &a);
            d.ending(c, e);
        }
        _ => return last::check(c, name),
    }
    Some(d)
}

#[path = "last.rs"]
mod last;
