// SPDX-License-Identifier: MIT
//! The checks of the routines that call into parts not ported.

use super::*;
use si_app::env::Arg;
use si_app::server::Clients;

/// The handle an address of the harness's handle pages stands for (emu_app.py, `H`).
pub fn handle_of(address: u32) -> u32 {
    if address == 0 {
        0
    } else {
        (address - 0x6170_0000) >> 12
    }
}

/// The last value the port gave to a `mem_…` setter, from its record.
pub fn last_mem(a: &A, name: &str) -> Option<i64> {
    let mut found = None;
    for line in a.w.lines() {
        if let Some(rest) = line.strip_prefix(name) {
            let args = rest.trim_start_matches('(').trim_end_matches(')');
            found = args.rsplit(',').next().and_then(|v| v.parse().ok());
        }
    }
    found
}

fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn word(c: &Case, name: &str, k: usize) -> u32 {
    c.input(name).values()[k] as u32
}

fn after_word(c: &Case, name: &str, k: usize) -> u32 {
    c.after(name).values()[k] as u32
}

fn progress_state(c: &Case) -> A {
    let mut a = app(c);
    a.g.standalone_renderer = word(c, "standalone", 0) as i16;
    a.g.in_progress_dial = handle_of(word(c, "dial", 0));
    a.g.emergency.saving = word(c, "saving", 0) as i16;
    a
}

fn progress_after(d: &mut Diff, c: &Case, a: &A) {
    d.eq("standalone", a.g.standalone_renderer, after_word(c, "standalone", 0) as i16);
    d.eq("inProgressDial", a.g.in_progress_dial, handle_of(after_word(c, "dial", 0)));
    d.outside(c, a);
}

/// The request dialog's default button: the port's last `mem_dialog_set_default` against
/// the short at +0x12 of the dialog's page.
fn request_after(d: &mut Diff, c: &Case, a: &A) {
    let before = i16_at(&c.input("h9").to_bytes(), 0x12);
    let port = last_mem(a, "mem_dialog_set_default").map(|v| v as i16).unwrap_or(before);
    d.eq("dialog+0x12", port, i16_at(&c.after("h9").to_bytes(), 0x12));
    d.outside(c, a);
}

fn log_state(c: &Case) -> A {
    let mut a = app(c);
    a.g.log.need_to_log = (word(c, "need", 0) != 0).then(Vec::new);
    a.g.log.check_need_to_log = word(c, "check", 0) as i32;
    let f = c.input("flags").values();
    a.g.log.to_screen = f[0] as i16;
    a.g.log.mouse = f[1] as i16;
    a.g.log.keyboard = f[2] as i16;
    if c.try_input("logf").is_some() {
        a.g.log.file = Some(b"logf".to_vec());
        a.w.files.insert(b"logf".to_vec(), Vec::new());
    }
    a
}

fn log_after(d: &mut Diff, c: &Case, a: &A) {
    if c.try_input("logf").is_some() {
        d.text("log file", a.w.files.get(&b"logf"[..]).map(|v| &v[..]).unwrap_or(b""), &out_bytes(c, "logf"));
    }
    d.outside(c, a);
}

/// Module loops for the routines checked one by one: the call is written down and the
/// script answers; the event stays what it was.
pub struct PlainClients;

impl Clients<Recorder> for PlainClients {
    fn client(&mut self, app: &mut A, module: i16) -> (i16, EventRef) {
        let name = si_app::server::CLIENT_NAMES[module as usize];
        let result = app.w.call(name, &[Arg::Ptr]) as i16;
        (result, EventRef::Active)
    }
}

pub fn check(c: &Case, name: &str) -> Option<Diff> {
    let mut d = Diff::default();
    match name {
        // ------------------------------------------------------------ in_progress.c
        "SI_dialogInProgressOpen" => {
            let mut a = progress_state(c);
            a.in_progress_open(&bytes(c, "title"));
            progress_after(&mut d, c, &a);
        }
        "SI_dialogInProgressSetTexts" => {
            let mut a = progress_state(c);
            a.in_progress_set_texts(&bytes(c, "a"), &bytes(c, "b"));
            progress_after(&mut d, c, &a);
        }
        "SI_dialogInProgressIsOpen" => {
            let a = progress_state(c);
            d.eq("al", a.in_progress_is_open() as u8, c.output("al").values()[0] as u8);
        }
        "SI_dialogInProgressRedraw" => {
            let mut a = progress_state(c);
            a.in_progress_redraw();
            progress_after(&mut d, c, &a);
        }
        "SI_dialogInProgressSet" => {
            let mut a = progress_state(c);
            a.in_progress_set(&bytes(c, "text"), c.input("v").f32());
            progress_after(&mut d, c, &a);
        }
        "SI_dialogInProgressClose" => {
            let mut a = progress_state(c);
            a.in_progress_close();
            progress_after(&mut d, c, &a);
        }
        // ------------------------------------------------------------ request.c
        "SI_dialogRequest" => {
            let mut a = app(c);
            a.dialog_request(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"), &bytes(c, "d"));
            request_after(&mut d, c, &a);
        }
        "SI_dialogRequestLong" => {
            let mut a = app(c);
            let l: Vec<Vec<u8>> = ["a", "b", "c", "d", "e"].iter().map(|k| bytes(c, k)).collect();
            a.dialog_request_long([&l[0], &l[1], &l[2], &l[3], &l[4]], &bytes(c, "f"));
            request_after(&mut d, c, &a);
        }
        "SI_dialogRequest2" => {
            let mut a = app(c);
            let got = a.dialog_request2(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"), &bytes(c, "d"), &bytes(c, "e"));
            d.eq("eax", got, eax(c));
            request_after(&mut d, c, &a);
        }
        "SI_dialogRequest3" => {
            let mut a = app(c);
            let l: Vec<Vec<u8>> = ["a", "b", "c", "d", "e", "f"].iter().map(|k| bytes(c, k)).collect();
            let got = a.dialog_request3([&l[0], &l[1], &l[2]], [&l[3], &l[4], &l[5]]);
            d.eq("eax", got, eax(c));
            request_after(&mut d, c, &a);
        }
        "SI_dialogRequest34" => {
            let mut a = app(c);
            let l: Vec<Vec<u8>> = ["a", "b", "c", "d", "e", "f", "g"].iter().map(|k| bytes(c, k)).collect();
            let got = a.dialog_request34([&l[0], &l[1], &l[2]], [&l[3], &l[4], &l[5], &l[6]]);
            d.eq("eax", got, eax(c));
            request_after(&mut d, c, &a);
        }
        "request2states" => {
            let mut a = app(c);
            let got = a.request2states(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"), &bytes(c, "d"), &bytes(c, "e"));
            d.eq("al", got as u8, eax(c) as u8);
            request_after(&mut d, c, &a);
        }
        "SI_systemWarning" => {
            let mut a = app(c);
            let got = a.system_warning(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"));
            d.eq("al", got as u8, c.output("al").values()[0] as u8);
            request_after(&mut d, c, &a);
        }
        "SI_systemNotice" => {
            let mut a = app(c);
            a.system_notice(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"));
            request_after(&mut d, c, &a);
        }
        "dialogRequest4" => {
            let mut a = app(c);
            let (got, answer) = a.dialog_request4(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"));
            d.eq("eax", got, eax(c));
            if got != -1 {
                d.text("answer", &answer.unwrap_or_default(), &out_bytes(c, "answer"));
            }
            request_after(&mut d, c, &a);
        }
        // ------------------------------------------------------------ log.c
        "SI_logOpen" => {
            let mut a = log_state(c);
            a.log_open();
            d.eq("needToLog set", a.g.log.need_to_log.is_some(), after_word(c, "need", 0) != 0);
            d.eq("file open", a.g.log.file.is_some(), after_word(c, "gfile", 0) != 0);
            d.eq("checkNeedToLog", a.g.log.check_need_to_log, after_word(c, "check", 0) as i32);
            let f = c.after("flags").values();
            d.eq("switches", [a.g.log.to_screen, a.g.log.mouse, a.g.log.keyboard], [f[0] as i16, f[1] as i16, f[2] as i16]);
            d.files(c, &a);
            d.outside(c, &a);
        }
        "SI_log" => {
            let mut a = log_state(c);
            a.si_log(i(c, "kind"), &bytes(c, "text"));
            log_after(&mut d, c, &a);
        }
        "SI_mouseLog" => {
            let mut a = log_state(c);
            a.mouse_log(i(c, "dev"), i16_in(c, "v"), i16_in(c, "x"), i16_in(c, "y"));
            log_after(&mut d, c, &a);
        }
        "SI_keyLog" => {
            let mut a = log_state(c);
            a.key_log(i(c, "dev"), i16_in(c, "v"));
            log_after(&mut d, c, &a);
        }
        _ => return rest::check(c, name),
    }
    Some(d)
}

#[path = "rest.rs"]
mod rest;
