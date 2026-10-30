// SPDX-License-Identifier: MIT
//! The checks of the batch renderer's front end.

use super::super::super::super::*;
use si_app::state::SRV_BUF;

fn vals(c: &Case, name: &str) -> Vec<u32> {
    c.input(name).values().iter().map(|&v| v as u32).collect()
}

fn after(c: &Case, name: &str) -> Vec<u32> {
    c.after(name).values().iter().map(|&v| v as u32).collect()
}

fn s16s(v: Vec<u32>) -> Vec<i16> {
    v.into_iter().map(|x| x as i16).collect()
}

pub fn check(c: &Case, name: &str) -> Option<Diff> {
    let mut d = Diff::default();
    match name {
        "standAloneRender" => {
            let mut a = app(c);
            a.g.init_buf = SRV_BUF;
            a.g.program_name = upto0(bytes(c, "prog"));
            let n = i(c, "argc") as usize;
            let args: Vec<Vec<u8>> = (0..n).map(|k| upto0(bytes(c, &format!("a{k}")))).collect();
            let lv = vals(c, "lv");
            a.g.line_option = lv[0] as i16;
            a.g.verbose_option = lv[1] as i16;
            a.g.report_frequency = vals(c, "freq")[0] as i16;
            a.g.proc_data.graf_list = vals(c, "pd")[1] as i32;
            let ctl = &mut a.g.default_control;
            ctl.renderer = vals(c, "rend")[0] as i16;
            ctl.out_name = match c.input("outn").words()[0] {
                si_x87::vectors::Word::Ref { .. } => Some(upto0(bytes(c, "outname"))),
                _ => None,
            };
            let f = vals(c, "frames");
            (ctl.first, ctl.last, ctl.step) = (f[0] as i16, f[1] as i16, f[2] as i16);
            let sf = c.input("subf").f32s();
            (ctl.sub_left, ctl.sub_right_inv, ctl.sub_top_inv, ctl.sub_bottom) = (sf[0], sf[1], sf[2], sf[3]);
            let mi = vals(c, "mi");
            (ctl.mi_output, ctl.mi_per_frame) = (mi[0] as i16, mi[1] as i16);
            ctl.switches.copy_from_slice(&s16s(vals(c, "switches")));
            let r = vals(c, "xy");
            (ctl.xres, ctl.yres) = (r[0] as i16, r[1] as i16);
            let ended = a.stand_alone_render(&args, Some(bytes(c, "res"))).err();
            if ended == Some(si_app::render_cmd::FAULT) {
                // the original reads an argument that is not there
                return None;
            }
            let ctl = &a.g.default_control;
            d.eq("renderer", ctl.renderer, after(c, "rend")[0] as i16);
            d.eq("frames", vec![ctl.first, ctl.last, ctl.step], s16s(after(c, "frames")));
            d.eq("skip", ctl.skip_existing, after(c, "skip")[0] as i16);
            d.eq("sub-region", ctl.subregion, after(c, "sub")[0] as i16);
            let sf: Vec<u32> = [ctl.sub_left, ctl.sub_right_inv, ctl.sub_top_inv, ctl.sub_bottom].iter().map(|f| f.to_bits()).collect();
            d.eq("sub-region numbers", sf, after(c, "subf"));
            d.eq("complete", ctl.complete, after(c, "complete")[0] as i16);
            d.eq("z channel", ctl.z_channel, after(c, "zch")[0] as i16);
            d.eq("mi", vec![ctl.mi_output, ctl.mi_per_frame], s16s(after(c, "mi")));
            d.eq("switches", ctl.switches.to_vec(), s16s(after(c, "switches")));
            d.eq("resolution", vec![ctl.xres, ctl.yres], s16s(after(c, "xy")));
            d.text("output name", ctl.out_name.as_deref().unwrap_or(b""), &out_bytes(c, "o.outname"));
            d.text("mi name", ctl.mi_name.as_deref().unwrap_or(b""), &out_bytes(c, "o.miname"));
            d.text("pre-frame script", ctl.pre_script.as_deref().unwrap_or(b""), &out_bytes(c, "o.pre"));
            d.text("post-frame script", ctl.post_script.as_deref().unwrap_or(b""), &out_bytes(c, "o.post"));
            d.text("picture path", a.g.picture_path.as_deref().unwrap_or(b""), &out_bytes(c, "o.picpath"));
            d.eq("standalone", a.g.standalone_renderer, after(c, "standalone")[0] as i16);
            d.eq("line and verbose", vec![a.g.line_option, a.g.verbose_option], s16s(after(c, "lv")));
            d.eq("report frequency", a.g.report_frequency, after(c, "freq")[0] as i16);
            d.eq("camera", a.g.proc_data.camera, super::super::handle_of(after(c, "pd")[2]));
            d.outside(c, &a);
            d.ending(c, ended);
        }
        "testsubRegion" => {
            let mut a = app(c);
            a.g.program_name = upto0(bytes(c, "prog"));
            let sf = c.input("subf").f32s();
            let ctl = &mut a.g.default_control;
            (ctl.sub_left, ctl.sub_right_inv, ctl.sub_top_inv, ctl.sub_bottom) = (sf[0], sf[1], sf[2], sf[3]);
            let ended = a.test_sub_region().err();
            d.outside(c, &a);
            d.ending(c, ended);
        }
        "error" => {
            let mut a = app(c);
            a.g.program_name = upto0(bytes(c, "prog"));
            let ended = a.render_error(&bytes(c, "a"), &bytes(c, "b"));
            d.outside(c, &a);
            d.ending(c, Some(ended));
        }
        // ------------------------------------------------------------ userpref.c
        "SI_findFile" | "SI_supraKeysLoadFromServer" | "SI_setupLoadFromServer" => {
            let mut a = app(c);
            a.g.cwd = Some(upto0(bytes(c, "cwd")));
            if name == "SI_findFile" {
                let got = a.find_file(&bytes(c, "dir"), &bytes(c, "name"));
                d.eq("found", got.is_some(), c.try_output("found").is_some());
                d.text("path", &got.unwrap_or_default(), &out_bytes(c, "found"));
            } else if name == "SI_setupLoadFromServer" {
                d.eq("eax", a.setup_load_from_server(&bytes(c, "dir"), &bytes(c, "name")), eax(c));
            } else {
                a.g.swift_mouse[2] = if vals(c, "lib")[0] != 0 { 50 } else { 0 };
                d.eq("eax", a.supra_keys_load_from_server(&bytes(c, "dir"), Some(&bytes(c, "name"))), eax(c));
            }
            d.outside(c, &a);
        }
        "saveDirectKeys" => {
            let mut a = app(c);
            let text = a.keys_text(Some(i(c, "count")), false, 0);
            d.text("file", &text, &out_bytes(c, "f"));
            d.outside(c, &a);
        }
        "saveSupraKeys" => {
            let mut a = app(c);
            let text = a.keys_text(None, true, 0);
            d.text("file", &text, &out_bytes(c, "f"));
            d.outside(c, &a);
        }
        // ------------------------------------------------------------ more of sys_main.c
        "handleSwiftMouse" => {
            let mut a = app(c);
            a.g.swift_mouse = [1, 2, 3, 4];
            a.handle_swift_mouse();
            let h = super::super::handle_of;
            let l = after(c, "load");
            // the globals' order in memory is load, save, delete; detect is elsewhere
            d.eq("entry points", a.g.swift_mouse, [h(after(c, "detect")[0]), h(l[1]), h(l[0]), h(l[2])]);
            d.outside(c, &a);
        }
        "copyRightClose" => {
            let mut a = app(c);
            a.g.bit_planes = vals(c, "planes")[0] as i32;
            a.g.olbit_planes = vals(c, "olplanes")[0] as i32;
            a.g.sysid = vals(c, "sysid")[0] as i32;
            a.g.irisf = vals(c, "irisf")[0] as i16;
            a.copy_right_close(i(c, "splash"));
            d.outside(c, &a);
        }
        "SI_checkOptions" => {
            let mut a = app(c);
            a.check_options(&bytes(c, "name"));
            // the first graph list's display mode (+0x0e of its 0x24-byte record)
            let k = vals(c, "pd")[1] as usize * 0x24 + 0xe;
            let gl = c.after("gl").to_bytes();
            d.eq("show nothing", i16::from_le_bytes([gl[k], gl[k + 1]]), vals(c, "nul")[0] as i16);
            d.outside(c, &a);
        }
        _ => return None,
    }
    Some(d)
}
