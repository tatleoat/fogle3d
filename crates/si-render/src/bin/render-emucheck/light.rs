// SPDX-License-Identifier: MIT
//! Checks of the light and fade set-up on the vectors of `re/tools/si-render/emu/gen_light.py`
//! (finding 0012, section 4): `si_lights` (with `shadowBuffer` recorded) and `si_initFade`.
use crate::prep::{mutate, same_words};
use crate::{bits, fl, light_of, vec3};
use si_math::matrix::RenderCamera;
use si_render::camera::{init_fade, Fade};
use si_render::lights::{lights, LightsEnv, fatal_no_inverse};
use si_render::shade::{ShadeGlobals, ShadeState};
use si_render::types::Rgbazo;
use si_x87::vectors::{Case, Check, Verdict, Word};

struct Recorder(Vec<usize>);
impl LightsEnv for Recorder {
    fn shadow_buffer(&mut self, _: &mut ShadeState, light: usize) {
        self.0.push(light);
    }
}

fn check_lights(c: &Case) -> Verdict {
    let mut st = ShadeState::default();
    let m = c.input("g_mat").f32s();
    let mat = |at: usize| -> si_math::types::Matrix { core::array::from_fn(|i| core::array::from_fn(|j| m[at + 4 * i + j])) };
    st.camera = RenderCamera { world_to_raw_mat: mat(0), raw_to_world_mat: mat(16), vect_mat: mat(32), prsp_mat: mat(48), near_z: 0.0 };
    for name in ["l0", "l1"] {
        let f = c.input(name);
        let mut l = light_of(c, name);
        l.zmap.clear();
        l.pos_given = vec3(f, 14);
        l.interest = vec3(f, 23);
        l.cone = fl(f, 29);
        l.spread = fl(f, 30);
        l.soft_full = bits(f, 73) as i32;
        l.soft_w128 = fl(f, 76);
        l.jitter.amplitude = fl(f, 78);
        st.lights.push(l);
    }
    st.light_head = Some(0);
    let mut raytrace = bits(c.input("g_rt"), 0) as u16;
    let mut env = Recorder(Vec::new());
    let got = lights(&mut st, &mut raytrace, 1, &mut env);
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        // (the recorder lets the original run on with an inverse nobody computed)
        si_x87::trig::replay::start(Vec::new());
        let text = String::from_utf8_lossy(&c.output("str0").to_bytes()).into_owned();
        return Verdict::from(got == Err(fatal_no_inverse()) && text == fatal_no_inverse(), || format!("fatal: original {text:?}, port {got:?}"));
    }
    crate::verdict((|| {
        let messages = got.map_err(|e| format!("the port stops with {e:?}, the original does not"))?;
        let printed = c.try_output("stdout").map(|f| String::from_utf8_lossy(&f.to_bytes()).into_owned()).unwrap_or_default();
        if printed != messages.concat() {
            return Err(format!("printed: original {printed:?}, port {messages:?}"));
        }
        let asked = c.calls.iter().filter(|k| k.name == "shadowBuffer").count();
        if asked != env.0.len() {
            return Err(format!("shadowBuffer: original {asked} calls, port {:?}", env.0));
        }
        let second = matches!(c.input("l0").words()[82], Word::Ref { .. });
        for (k, name) in ["l0", "l1"].iter().enumerate() {
            let (before, after, l) = (c.input(name), c.after(name), &st.lights[k]);
            let mut port: Vec<u32> = (0..97).map(|i| bits(before, i)).collect();
            if k == 0 || second {
                for (i, v) in [l.pos.x, l.pos.y, l.pos.z, l.dir.x, l.dir.y, l.dir.z].iter().enumerate() {
                    port[17 + i] = v.to_bits();
                }
                if l.kind == 2 {
                    for (i, v) in [l.spot_dir.x, l.spot_dir.y, l.spot_dir.z].iter().enumerate() {
                        port[26 + i] = v.to_bits();
                    }
                    (port[31], port[32], port[33]) = (l.cone_cos.to_bits(), l.spread_cos.to_bits(), l.cone_div.to_bits());
                }
                if l.shadow == 3 || l.shadow == 4 {
                    (port[74], port[77]) = (l.soft_size as u32, l.soft_scale.to_bits());
                }
                port[81] = (port[81] & 0xffff_0000) | l.falloff as u32;
            }
            for i in 0..97 {
                let q = port[i] ^ mutate() as u32;
                if bits(after, i) != q {
                    return Err(format!("{name} word {i}: original {:08x}, port {q:08x}", bits(after, i)));
                }
            }
        }
        let mut mats = Vec::new();
        for q in [&st.camera.world_to_raw_mat, &st.camera.raw_to_world_mat, &st.camera.vect_mat, &st.camera.prsp_mat] {
            mats.extend(q.iter().flatten().map(|f| f.to_bits()));
        }
        same_words("the four matrices", c.after("g_mat"), &mats)?;
        same_words("ray tracing word", c.after("g_rt"), &[raytrace as u32])
    })())
}

fn check_init_fade(c: &Case) -> Verdict {
    let fp = c.input("g_fp").f32s();
    let bg = c.input("g_bg").f32s();
    let bl = c.input("g_blank").f32s();
    let mut glb = ShadeGlobals { fade: c.input("g_fade").i16(), fade_start: fp[1], fade_end: fp[3], fade_scale: fp[4], ..ShadeGlobals::default() };
    glb.background = Rgbazo { r: bg[0], g: bg[1], b: bg[2], a: bg[3], z: bg[4], o: bg[5] };
    let mut fade = Fade { start: fp[0], end: fp[2], amount: c.input("g_amt").f32() };
    let blank = Rgbazo { r: bl[0], g: bl[1], b: bl[2], a: bl[3], z: bl[4], o: bl[5] };
    // a byte no line can hold marks what the port did not write
    let mut lines = [[0xa5a5u16; 40]; 4];
    let mut bytes = [[0u8; 40]; 4];
    let width = c.input("g_w").i16();
    let [l0, l1, l2, l3] = &mut bytes;
    let got = init_fade(&mut glb, &mut fade, blank, width, [l0, l1, l2, l3]);
    for k in 0..4 {
        for i in 0..(width.max(0) as usize).min(40) {
            lines[k][i] = bytes[k][i] as u16;
        }
    }
    crate::verdict((|| {
        let b = glb.background;
        same_words("fade", c.after("g_fp"), &[fade.start, glb.fade_start, fade.end, glb.fade_end, glb.fade_scale].map(f32::to_bits))?;
        same_words("background", c.after("g_bg"), &[b.r, b.g, b.b, b.a, b.z, b.o].map(f32::to_bits))?;
        same_words("background bytes", c.after("g_bytes"), &got.map(|v| v as u32))?;
        for (k, name) in ["lr", "lg", "lb", "la"].iter().enumerate() {
            let want = c.output(name).written();
            for i in 0..40 {
                let port = if lines[k][i] == 0xa5a5 { None } else { Some((lines[k][i] as u8 ^ mutate() as u8) as u128) };
                if want[i] != port {
                    return Err(format!("{name}[{i}]: original {:?}, port {port:?}", want[i]));
                }
            }
        }
        Ok(())
    })())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_lights" => check_lights,
        "si_initFade" => check_init_fade,
        _ => return None,
    })
}
