// SPDX-License-Identifier: MIT
//! The checks of the emergency save and the batch renderer.

use super::super::super::*;
use super::load_screen;

fn vals(c: &Case, name: &str) -> Vec<u32> {
    c.input(name).values().iter().map(|&v| v as u32).collect()
}

fn after(c: &Case, name: &str) -> Vec<u32> {
    c.after(name).values().iter().map(|&v| v as u32).collect()
}

fn statics(c: &Case, a: &mut A) {
    a.g.emergency.pre_clear = vals(c, "preclear")[0] as i16;
    a.g.emergency.play_sound_cues = vals(c, "cues")[0] as i16;
    a.g.emergency.crash_ui_mode = vals(c, "uimode")[0] as i32;
}

fn statics_after(d: &mut Diff, c: &Case, a: &A) {
    d.eq("playSoundCues", a.g.emergency.play_sound_cues, after(c, "cues")[0] as i16);
    d.eq("crashUIMode", a.g.emergency.crash_ui_mode, after(c, "uimode")[0] as i32);
}

fn field16(c: &Case, name: &str) -> Vec<i16> {
    c.output(name).values().iter().map(|&v| v as i16).collect()
}

pub fn check(c: &Case, name: &str) -> Option<Diff> {
    let mut d = Diff::default();
    match name {
        "tempCrashLogName" => {
            let mut a = app(c);
            let got = a.temp_crash_log_name();
            d.text("name", &got, &upto0(c.after("buf").to_bytes()));
            d.outside(c, &a);
        }
        "SI_lastCrashLog" => {
            let mut a = app(c);
            let got = a.last_crash_log(i16_in(c, "remove"));
            d.eq("found", got.is_some(), c.try_output("line").is_some());
            d.text("line", &got.unwrap_or_default(), &out_bytes(c, "line"));
            d.files(c, &a);
            d.outside(c, &a);
        }
        "emergencyDeleteDirectoryPath" => {
            let mut a = app(c);
            d.eq("eax", a.emergency_delete_directory_path(cstr(c, "path").as_deref(), i16_in(c, "bg")), eax(c));
            d.outside(c, &a);
        }
        "playSoundCue" => {
            let mut a = app(c);
            statics(c, &mut a);
            a.play_sound_cue(cstr(c, "file").as_deref());
            d.outside(c, &a);
        }
        "checkTools" => {
            let mut a = app(c);
            a.g.emergency.sound_crash = upto0(bytes(c, "s1"));
            a.g.emergency.sound_saved = upto0(bytes(c, "s2"));
            a.check_tools();
            let e = &a.g.emergency;
            let h = after(c, "have");
            d.eq("tools", [e.have_sfplay, e.have_xconfirm, e.have_gclear], [h[0] as i16, h[1] as i16, h[2] as i16]);
            d.outside(c, &a);
        }
        "SI_emergencySaving" | "SI_emergencySaveEnabled" | "SI_emergencySaveFunc" => {
            let mut a = app(c);
            a.g.emergency.enabled = vals(c, "r_en")[0] as i16;
            a.g.emergency.saving = vals(c, "r_sav")[0] as i16;
            a.g.emergency.handler = vals(c, "r_fn")[0] != 0;
            match name {
                "SI_emergencySaving" => d.eq("eax", a.emergency_saving(), eax(c)),
                "SI_emergencySaveEnabled" => d.eq("eax", a.emergency_save_enabled(), eax(c)),
                _ => d.eq("a handler", a.emergency_save_func(), eax(c) != 0),
            }
        }
        "SI_setupEmergencySave" => {
            let mut a = app(c);
            statics(c, &mut a);
            a.g.standalone_renderer = vals(c, "standalone")[0] as i16;
            // what the record held before
            let e = &mut a.g.emergency;
            e.entered = 7;
            e.saving = 1;
            e.wanted_dir = b"oldw".to_vec();
            e.save_dir = b"olds".to_vec();
            e.crash_log = b"oldl".to_vec();
            e.sound_crash = b"olda".to_vec();
            e.sound_saved = b"oldb".to_vec();
            e.handler = true;
            e.have_sfplay = 1;
            e.have_xconfirm = 1;
            e.have_gclear = 1;
            a.setup_emergency_save(i16_in(c, "mode"));
            let e = &a.g.emergency;
            d.eq("enabled", e.enabled, field16(c, "r.enabled")[0]);
            d.eq("entered", e.entered, c.output("r.entered").values()[0] as i32);
            d.eq("saving", e.saving, field16(c, "r.saving")[0]);
            d.text("wanted directory", &e.wanted_dir, &out_bytes(c, "r.wanted"));
            d.text("save directory", &e.save_dir, &out_bytes(c, "r.savedir"));
            d.text("crash log", &e.crash_log, &out_bytes(c, "r.crashlog"));
            d.text("crash sound", &e.sound_crash, &out_bytes(c, "r.sound1"));
            d.text("saved sound", &e.sound_saved, &out_bytes(c, "r.sound2"));
            d.eq("handler", e.handler, c.output("r.handler").values()[0] != 0);
            d.eq("tools", vec![e.have_sfplay, e.have_xconfirm, e.have_gclear], field16(c, "r.have"));
            d.eq("word 0x503c", e.word503c, c.output("r.word").values()[0] as i32);
            if e.enabled != 0 {
                d.text("database path", e.db_path.as_deref().unwrap_or(b""), &out_bytes(c, "r.dbpath"));
            }
            statics_after(&mut d, c, &a);
            d.outside(c, &a);
        }
        "crashWarning" => {
            let mut a = app(c);
            statics(c, &mut a);
            a.g.standalone_renderer = vals(c, "standalone")[0] as i16;
            a.g.emergency.have_gclear = vals(c, "gclear")[0] as i16;
            let got = a.crash_warning(&bytes(c, "a"), &bytes(c, "b"), &bytes(c, "c"), i(c, "mode"), i16_in(c, "inform"));
            d.eq("eax", got, eax(c));
            d.eq("standalone", a.g.standalone_renderer, after(c, "standalone")[0] as i16);
            d.outside(c, &a);
        }
        "emergencySave" => {
            let mut a = app(c);
            statics(c, &mut a);
            load_screen(c, &mut a);
            let e = &mut a.g.emergency;
            e.immediate_abort = vals(c, "abort")[0] as i16;
            e.enabled = vals(c, "r0")[0] as i16;
            e.entered = vals(c, "entered")[0] as i32;
            e.saving = vals(c, "saving")[0] as i16;
            e.save_dir = upto0(bytes(c, "rs"));
            e.crash_log = upto0(bytes(c, "rl"));
            e.sound_crash = upto0(bytes(c, "ra"));
            e.sound_saved = upto0(bytes(c, "rb"));
            e.have_gclear = vals(c, "gclear")[0] as i16;
            e.command_line = upto0(bytes(c, "cmd"));
            e.handler = true;
            a.g.signal_coredump = vals(c, "kept")[0] as i16;
            let ended = a.emergency_save().err();
            let e = &a.g.emergency;
            d.eq("immediateAbort", e.immediate_abort, after(c, "abort")[0] as i16);
            d.eq("entered", e.entered, after(c, "entered")[0] as i32);
            d.eq("saving", e.saving, after(c, "saving")[0] as i16);
            d.eq("standalone", a.g.standalone_renderer, after(c, "standalone")[0] as i16);
            d.eq("SI_rgbActive", super::last_mem(&a, "mem_set_rgb_active").unwrap_or(0) as i32, after(c, "rgb")[0] as i32);
            d.eq("coredump kept", a.g.signal_coredump, after(c, "kept")[0] as i16);
            statics_after(&mut d, c, &a);
            d.files(c, &a);
            d.outside(c, &a);
            d.ending(c, ended);
        }
        _ => return render::check(c, name),
    }
    Some(d)
}

#[path = "render.rs"]
mod render;
