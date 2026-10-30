// SPDX-License-Identifier: MIT
//! `shdb_translator.c`: writes the preview scene as mental ray text down a pipe (finding 0041).
//!
//! The scene record is the one of [`crate::shdb_init`]; the strings go through `writeString`
//! into the global buffer `strWrite` and then to `SI_pipeioWrite`. What is not ported:
//! `handler` and `handler_SIGPIPE` end in a `longjmp` (no check can compare them, see the finding).

use crate::cfmt;
use crate::strlist::si_str_append;
use crate::through_x87;
use si_mem::{clib, Mem, Outside, Routine};

/// `SI_pipeioWrite (0x0842fd68)`: (pipe, text, length, &written). Answers non-zero on failure.
pub const SI_PIPEIO_WRITE: Routine = Routine::new("SI_pipeioWrite", 0x0842_fd68);
/// `SI_mrthreadExit (0x084302cc)`.
pub const SI_MRTHREAD_EXIT: Routine = Routine::new("SI_mrthreadExit", 0x0843_02cc);
/// `SI_mrpipeSet (0x0842c578)`: (pipe, &short, &pointer).
pub const SI_MRPIPE_SET: Routine = Routine::new("SI_mrpipeSet", 0x0842_c578);
/// `SI_mrpipeFlush (0x0842c5d8)`.
pub const SI_MRPIPE_FLUSH: Routine = Routine::new("SI_mrpipeFlush", 0x0842_c5d8);
/// `SI_customPreviewWriteUDF (0x08585600)`: five words.
pub const SI_CUSTOM_PREVIEW_WRITE_UDF: Routine = Routine::new("SI_customPreviewWriteUDF", 0x0858_5600);
/// `SI_shaderGenerateCodes (0x08542830)`, `SI_shaderGenerateLinks (0x08542a88)`,
/// `SI_shaderGenerateDeclare (0x08543250)`.
pub const SI_SHADER_GENERATE_CODES: Routine = Routine::new("SI_shaderGenerateCodes", 0x0854_2830);
pub const SI_SHADER_GENERATE_LINKS: Routine = Routine::new("SI_shaderGenerateLinks", 0x0854_2a88);
pub const SI_SHADER_GENERATE_DECLARE: Routine = Routine::new("SI_shaderGenerateDeclare", 0x0854_3250);
/// `SI_getmi2Output (0x08580cfc)`, `SI_mi2IsIncremental (0x08580d84)`, `SI_setmi2Output (0x08580ce0)`,
/// `SI_setmi2Incremental (0x08580d94)`.
pub const SI_GETMI2_OUTPUT: Routine = Routine::new("SI_getmi2Output", 0x0858_0cfc);
pub const SI_MI2_IS_INCREMENTAL: Routine = Routine::new("SI_mi2IsIncremental", 0x0858_0d84);
pub const SI_SETMI2_OUTPUT: Routine = Routine::new("SI_setmi2Output", 0x0858_0ce0);
pub const SI_SETMI2_INCREMENTAL: Routine = Routine::new("SI_setmi2Incremental", 0x0858_0d94);

/// The write buffer (0x400 bytes), the pointer to the task's terminate flag, and the counters.
pub const STR_WRITE: u32 = 0x0882_4360;
pub const B_TERMINATE: u32 = 0x086e_4f8c;
pub const LINE: u32 = 0x086e_4f80;
pub const ECHO: u32 = 0x086e_4f84;
pub const FRAME: u32 = 0x086e_4f88;
/// `mi_defaults` (pointer to the text), `SI_resdir` (pointer to the resource directory),
/// `SI_customPreviewArgs` (a string buffer), `SI_frameRate` (float).
pub const MI_DEFAULTS: u32 = 0x086e_5180;
pub const SI_RESDIR: u32 = 0x0878_1628;
pub const SI_CUSTOM_PREVIEW_ARGS: u32 = 0x0879_9860;
pub const SI_FRAME_RATE: u32 = 0x0878_204c;

// the strings of the file (.data)
const S_RENDER_FRAME: u32 = 0x086e_5184;
const S_RENDER_END: u32 = 0x086e_519d;
const S_ON1: u32 = 0x086e_51b0;
const S_OFF1: u32 = 0x086e_51b3;
const S_VERBOSE: u32 = 0x086e_51b7;
const S_INCLUDE_DIR: u32 = 0x086e_51c3;
const S_INCLUDE: u32 = 0x086e_51e0;
const S_FRAME1: u32 = 0x086e_51fa;
const S_SPHERE: u32 = 0x086e_5203;
const S_PLANE3: u32 = 0x086e_5214;
const S_PLANE1: u32 = 0x086e_5227;
const S_CUT0: u32 = 0x086e_5234;
const S_CUT1: u32 = 0x086e_5244;
const S_CUT2: u32 = 0x086e_5260;
const S_LIGHTS_OPEN: u32 = 0x086e_52af;
const S_BLANK: u32 = 0x086e_52bb;
const S_LIGHTS_CLOSE: u32 = 0x086e_52bd;
const S_VIEW: u32 = 0x086e_52c0;
const S_OUTPUT_PIPELINE: u32 = 0x086e_52c6;
const S_FOCAL: u32 = 0x086e_52da;
const S_SHUTTER: u32 = 0x086e_52e4;
const S_APERTURE: u32 = 0x086e_52f0;
const S_ASPECT: u32 = 0x086e_52fd;
const S_RESOLUTION: u32 = 0x086e_5308;
const S_ON2: u32 = 0x086e_531a;
const S_OFF2: u32 = 0x086e_531d;
const S_SHADOW: u32 = 0x086e_5321;
const S_MIN_SAMPLES: u32 = 0x086e_532c;
const S_MAX_SAMPLES: u32 = 0x086e_533c;
const S_END_VIEW: u32 = 0x086e_534c;
const S_LIGHT: u32 = 0x086e_5356;
const S_COLOR: u32 = 0x086e_5371;
const S_ON3: u32 = 0x086e_5387;
const S_OFF3: u32 = 0x086e_538a;
const S_SHADOW_L: u32 = 0x086e_538e;
const S_PAREN: u32 = 0x086e_539b;
const S_ORIGIN: u32 = 0x086e_539e;
const S_END_LIGHT: u32 = 0x086e_53af;
const S_DEFAULT_BACKGROUND: u32 = 0x086e_53ba;
const S_DEFAULT_MATERIAL: u32 = 0x086e_53cd;
const S_MATERIAL: u32 = 0x086e_53de;
const S_SQUARE1: u32 = 0x086e_53f1;
const S_SQUARE2: u32 = 0x086e_5409;
const S_LINE: u32 = 0x086e_5421;
const S_SIZE1: u32 = 0x086e_5436;
const S_SIZE2: u32 = 0x086e_5443;
const S_DIFFUSE_C: u32 = 0x086e_5450;
const S_AMBIENT: u32 = 0x086e_5468;
const S_DIFFUSE: u32 = 0x086e_547e;
const S_SPECULAR: u32 = 0x086e_5494;
const S_REFLECTIVITY: u32 = 0x086e_54ab;
const S_SHINY: u32 = 0x086e_54bd;
const S_TRANSP: u32 = 0x086e_54c8;
const S_LIGHTS_M: u32 = 0x086e_54d4;
const S_NAME: u32 = 0x086e_54de;
const S_END_MATERIAL: u32 = 0x086e_54e2;

/// The two stack words of a `float` argument passed to `printf` as a double (`fld`, `fstp qword`).
fn dbl(m: &Mem, addr: u32) -> [u32; 2] {
    let d = f64::from(f32::from_bits(through_x87(m.u32(addr)))).to_bits();
    [d as u32, (d >> 32) as u32]
}

fn on_off(m: &Mem, flag: u32, on: u32, off: u32) -> u32 {
    if m.u16(flag) != 0 { on } else { off }
}

/// [soft 0x0854e0d0 writeString]: formats into `strWrite` and writes it to the pipe `fd`, in as many
/// writes as the pipe takes; a failed write ends it, a set terminate flag exits the thread.
pub fn write_string(m: &mut Mem, o: &mut dyn Outside, fd: u32, fmt: u32, args: &[u32]) {
    let text = cfmt::format(m, fmt, args);
    m.put_cstr(STR_WRITE, &text);
    let mut len = clib::strlen(m, STR_WRITE) as i32;
    let mut off = 0u32;
    if len <= 0 {
        return;
    }
    let written = m.local(4);
    while len > 0 {
        let r = o.call(m, SI_PIPEIO_WRITE, &[fd, STR_WRITE.wrapping_add(off), len as u32, written]);
        if r.eax != 0 {
            break;
        }
        let flag = m.u32(B_TERMINATE);
        if m.u16(flag) != 0 {
            o.call(m, SI_MRTHREAD_EXIT, &[0]);
        }
        let w = m.u32(written);
        len = len.wrapping_sub(w as i32);
        off = off.wrapping_add(w);
    }
    m.release(4);
}

/// [soft 0x0854dae0 generateLightList]: the text `,\nlights [ name name ]\n` for the chain of lights
/// at `light` (next link +0x24), into `*out`. Answers 1; 0 (and `*out` freed) when a part could not be
/// appended.
pub fn generate_light_list(m: &mut Mem, light: u32, out: u32) -> i32 {
    if out == 0 {
        return 0;
    }
    m.set_u32(out, 0);
    if si_str_append(m, out, S_LIGHTS_OPEN) as i16 == 0 {
        return 0;
    }
    let mut failed = false;
    let mut node = light;
    if node != 0 {
        loop {
            let name = m.u32(node);
            if si_str_append(m, out, name) as i16 == 0 {
                failed = true;
            }
            if !failed && si_str_append(m, out, S_BLANK) as i16 == 0 {
                failed = true;
            }
            node = m.u32(node + 0x24);
            if failed || node == 0 {
                break;
            }
        }
    }
    if !failed && si_str_append(m, out, S_LIGHTS_CLOSE) as i16 == 0 {
        failed = true;
    }
    if !failed {
        return 1;
    }
    let s = m.u32(out);
    if s != 0 {
        m.free(s);
        m.set_u32(out, 0);
    }
    0
}

/// [soft 0x0854dcb0 writeShader]: the shader's definition through `SI_customPreviewWriteUDF`
/// (task +0x30, +0x34 are its first words), with the light list. Answers 1, or 0 without a list.
pub fn write_shader(m: &mut Mem, o: &mut dyn Outside, task: u32, fd: u32, shader: u32, scene: u32) -> i32 {
    let list = m.local(4);
    m.set_u32(list, 0);
    let light = m.u32(scene + 0x10c);
    if generate_light_list(m, light, list) as i16 == 0 {
        m.release(4);
        return 0;
    }
    let p = m.local(8);
    m.set_u32(p, 0);
    m.set_u16(p + 4, 0);
    o.call(m, SI_MRPIPE_SET, &[fd, p + 4, p]);
    let (a, b) = (m.u32(task + 0x30), m.u32(task + 0x34));
    let l = m.u32(list);
    o.call(m, SI_CUSTOM_PREVIEW_WRITE_UDF, &[a, b, 0, shader, l]);
    o.call(m, SI_MRPIPE_FLUSH, &[]);
    let l = m.u32(list);
    if l != 0 {
        m.free(l);
        m.set_u32(list, 0);
    }
    let q = m.u32(p);
    if q != 0 {
        m.free(q);
        m.set_u32(p, 0);
    }
    m.release(8);
    m.release(4);
    1
}

/// [soft 0x0854dbb4 writeView]
pub fn write_view(m: &mut Mem, o: &mut dyn Outside, fd: u32, v: u32) {
    write_string(m, o, fd, S_VIEW, &[]);
    let a = m.u32(v + 4);
    write_string(m, o, fd, S_OUTPUT_PIPELINE, &[a]);
    for (off, f) in [(0x10, S_FOCAL), (0x14, S_SHUTTER), (0x18, S_APERTURE), (0x1c, S_ASPECT)] {
        let d = dbl(m, v + off);
        write_string(m, o, fd, f, &d);
    }
    let (x, y) = (m.u32(v + 0x28), m.u32(v + 0x2c));
    write_string(m, o, fd, S_RESOLUTION, &[x, y]);
    let s = on_off(m, v + 0x30, S_ON2, S_OFF2);
    write_string(m, o, fd, S_SHADOW, &[s]);
    let a = m.u32(v + 0x20);
    write_string(m, o, fd, S_MIN_SAMPLES, &[a]);
    let a = m.u32(v + 0x24);
    write_string(m, o, fd, S_MAX_SAMPLES, &[a]);
    write_string(m, o, fd, S_END_VIEW, &[]);
}

/// [soft 0x0854dd64 writeDefaultLightDefinition]
pub fn write_default_light_definition(m: &mut Mem, o: &mut dyn Outside, fd: u32, l: u32) {
    let name = m.u32(l);
    write_string(m, o, fd, S_LIGHT, &[name]);
    let mut a = Vec::new();
    for off in [0x10, 0x14, 0x18, 0x1c] {
        a.extend_from_slice(&dbl(m, l + off));
    }
    write_string(m, o, fd, S_COLOR, &a);
    let s = on_off(m, l + 0x20, S_ON3, S_OFF3);
    write_string(m, o, fd, S_SHADOW_L, &[s]);
    write_string(m, o, fd, S_PAREN, &[]);
    let mut a = Vec::new();
    for off in [4, 8, 0xc] {
        a.extend_from_slice(&dbl(m, l + off));
    }
    write_string(m, o, fd, S_ORIGIN, &a);
    write_string(m, o, fd, S_END_LIGHT, &[]);
}

fn floats(m: &Mem, base: u32, offs: &[u32]) -> Vec<u32> {
    let mut a = Vec::new();
    for &off in offs {
        a.extend_from_slice(&dbl(m, base + off));
    }
    a
}

/// [soft 0x0854de2c writePlaneMaterial]: the plane's material (default or checker) with the list of lights.
pub fn write_plane_material(m: &mut Mem, o: &mut dyn Outside, fd: u32, p: u32) {
    if m.u16(p + 8) == 0 {
        return;
    }
    let checker = m.u16(p + 0x1c) != 0;
    let kind = if checker { S_DEFAULT_BACKGROUND } else { S_DEFAULT_MATERIAL };
    let name = m.u32(p + 0x18);
    write_string(m, o, fd, S_MATERIAL, &[kind, name]);
    if checker {
        let list: [(u32, &[u32]); 7] = [
            (S_SQUARE1, &[0x5c, 0x60, 0x64, 0x68]),
            (S_SQUARE2, &[0x6c, 0x70, 0x74, 0x78]),
            (S_LINE, &[0x7c, 0x80, 0x84, 0x88]),
            (S_SIZE1, &[0x8c]),
            (S_SIZE2, &[0x90]),
            (S_DIFFUSE_C, &[0x94, 0x98, 0x9c, 0xa0]),
            (0, &[]),
        ];
        for (f, offs) in list {
            if f != 0 {
                let a = floats(m, p, offs);
                write_string(m, o, fd, f, &a);
            }
        }
    } else {
        let list: [(u32, &[u32]); 6] = [
            (S_AMBIENT, &[0x20, 0x24, 0x28, 0x2c]),
            (S_DIFFUSE, &[0x30, 0x34, 0x38, 0x3c]),
            (S_SPECULAR, &[0x40, 0x44, 0x48, 0x4c]),
            (S_REFLECTIVITY, &[0x50]),
            (S_SHINY, &[0x54]),
            (S_TRANSP, &[0x58]),
        ];
        for (f, offs) in list {
            let a = floats(m, p, offs);
            write_string(m, o, fd, f, &a);
        }
    }
    write_string(m, o, fd, S_LIGHTS_M, &[]);
    let mut node = m.u32(p + 0xcc);
    m.set_u32(ECHO, 0);
    while node != 0 {
        let n = m.u32(node);
        write_string(m, o, fd, S_NAME, &[n]);
        node = m.u32(node + 0x24);
    }
    write_string(m, o, fd, S_END_MATERIAL, &[]);
    m.set_u32(ECHO, 1);
}

/// `SI_frameToSec (0x08201554)` for the translator: the frame over `SI_frameRate`, divided in the
/// 80-bit register and rounded to a double by the store. Finite rates only (zero, infinite or NaN
/// rates give the plain f64 answer).
fn frame_to_sec_bits(m: &Mem, frame: i32) -> [u32; 2] {
    let rate = f32::from_bits(m.u32(SI_FRAME_RATE));
    let bits = rate.to_bits();
    let (e, frac) = ((bits >> 23) & 0xff, bits & 0x7f_ffff);
    let neg = (frame < 0) != (bits >> 31 != 0);
    if e == 0xff || rate == 0.0 || frame == 0 {
        let d = f64::from(frame) / f64::from(rate);
        return [d.to_bits() as u32, (d.to_bits() >> 32) as u32];
    }
    let (mant, exp) = if e == 0 { (frac as u128, -149i32) } else { ((frac | 0x80_0000) as u128, e as i32 - 150) };
    let fa = frame.unsigned_abs() as u128;
    // quotient fa/mant with 64 significant bits plus sticky, then rounded to 64 bits, then to 53
    let shift = 90u32;
    let n = fa << shift;
    let (q, r) = (n / mant, n % mant);
    let len = 128 - q.leading_zeros();
    let drop = len - 64;
    let mut m64 = (q >> drop) as u128;
    let rest = q & ((1u128 << drop) - 1);
    let half = 1u128 << (drop - 1);
    let sticky = r != 0;
    // value = m64 * 2^(drop - shift) / 2^exp
    let mut e2 = drop as i32 - shift as i32 - exp;
    if rest > half || (rest == half && (sticky || m64 & 1 == 1)) {
        m64 += 1;
        if m64 == 1u128 << 64 {
            m64 >>= 1;
            e2 += 1;
        }
    }
    // to 53 bits, nearest even
    let low = m64 & 0x7ff;
    let mut m53 = m64 >> 11;
    if low > 0x400 || (low == 0x400 && m53 & 1 == 1) {
        m53 += 1;
    }
    let d = (m53 as f64) * 2f64.powi(e2 + 11);
    let d = if neg { -d } else { d };
    [d.to_bits() as u32, (d.to_bits() >> 32) as u32]
}

fn render_line(m: &mut Mem, o: &mut dyn Outside, fd: u32) {
    let frame = m.u32(FRAME);
    let mut a = vec![frame];
    a.extend_from_slice(&frame_to_sec_bits(m, frame as i32));
    write_string(m, o, fd, S_RENDER_FRAME, &a);
}

/// [soft 0x0854d7d4 writeSceneFull]: the whole scene: verbose, the shader's code, the include of the
/// library, the view, light, plane material, shader, sphere, plane, cutplane and render lines.
pub fn write_scene_full(m: &mut Mem, o: &mut dyn Outside, task: u32, fd: u32, shader: u32, scene: u32) -> i32 {
    let v = on_off(m, scene + 0x3e, S_ON1, S_OFF1);
    write_string(m, o, fd, S_VERBOSE, &[v]);
    let p = m.local(8);
    m.set_u32(p, 0);
    m.set_u16(p + 4, 0);
    o.call(m, SI_MRPIPE_SET, &[fd, p + 4, p]);
    let list = m.u32(shader + 0x24);
    o.call(m, SI_SHADER_GENERATE_CODES, &[list]);
    let list = m.u32(shader + 0x24);
    o.call(m, SI_SHADER_GENERATE_LINKS, &[list]);
    let list = m.u32(shader + 0x24);
    o.call(m, SI_SHADER_GENERATE_DECLARE, &[list, 0]);
    o.call(m, SI_MRPIPE_FLUSH, &[]);
    let q = m.u32(p);
    if q != 0 {
        m.free(q);
        m.set_u32(p, 0);
    }
    m.release(8);
    let dir = m.u32(SI_RESDIR);
    if dir != 0 {
        write_string(m, o, fd, S_INCLUDE_DIR, &[dir]);
    } else {
        write_string(m, o, fd, S_INCLUDE, &[]);
    }
    let defaults = m.u32(MI_DEFAULTS);
    write_string(m, o, fd, defaults, &[]);
    write_string(m, o, fd, S_FRAME1, &[]);
    write_view(m, o, fd, scene);
    let light = m.u32(scene + 0x10c);
    write_default_light_definition(m, o, fd, light);
    write_plane_material(m, o, fd, scene + 0x40);
    if write_shader(m, o, task, fd, shader, scene) as i16 == 0 {
        return 0;
    }
    let mut a = floats(m, scene, &[0x40, 0x44]);
    a.push(SI_CUSTOM_PREVIEW_ARGS);
    write_string(m, o, fd, S_SPHERE, &a);
    if m.u16(scene + 0x48) != 0 {
        let (w, h) = (f32::from_bits(m.u32(scene + 0x50)), f32::from_bits(m.u32(scene + 0x54)));
        let name = m.u32(scene + 0x58);
        if w > 0.0 && h > 0.0 {
            let mut a = floats(m, scene, &[0x4c, 0x50, 0x54]);
            a.push(name);
            write_string(m, o, fd, S_PLANE3, &a);
        } else {
            let mut a = floats(m, scene, &[0x4c]);
            a.push(name);
            write_string(m, o, fd, S_PLANE1, &a);
        }
    }
    if m.u16(scene + 0xe4) != 0 {
        match m.i32(scene + 0xe8) {
            0 => {
                let mut a = floats(m, scene, &[0xec]);
                a.push(SI_CUSTOM_PREVIEW_ARGS);
                write_string(m, o, fd, S_CUT0, &a);
            }
            1 => {
                let mut a = floats(m, scene, &[0xf4, 0xf8, 0xfc, 0xf0]);
                a.push(SI_CUSTOM_PREVIEW_ARGS);
                write_string(m, o, fd, S_CUT1, &a);
            }
            2 => {
                let mut a = floats(m, scene, &[0xf4, 0xf8, 0xfc, 0x100, 0x104, 0x108]);
                a.push(SI_CUSTOM_PREVIEW_ARGS);
                write_string(m, o, fd, S_CUT2, &a);
            }
            _ => {}
        }
    }
    if m.u16(scene + 0x3c) != 0 {
        render_line(m, o, fd);
    } else {
        write_string(m, o, fd, S_RENDER_END, &[]);
    }
    1
}

/// [soft 0x0854d75c writeSceneIncremental]: the shader and the render line.
pub fn write_scene_incremental(m: &mut Mem, o: &mut dyn Outside, task: u32, fd: u32, shader: u32, scene: u32) -> i32 {
    if write_shader(m, o, task, fd, shader, scene) as i16 == 0 {
        return 0;
    }
    if m.u16(scene + 0x3c) != 0 {
        render_line(m, o, fd);
    } else {
        write_string(m, o, fd, S_RENDER_END, &[]);
    }
    1
}

/// [soft 0x0854d69c si_shdbTranslatorProc_Protected]: sets the line, echo and terminate globals,
/// switches the mi2 output off, writes the scene (mode 1 full, mode 2 incremental), clears the task's
/// state word and restores the mi2 settings.
pub fn si_shdb_translator_proc_protected(m: &mut Mem, o: &mut dyn Outside, task: u32) {
    m.set_u32(LINE, 1);
    m.set_u32(ECHO, 1);
    m.set_u32(B_TERMINATE, task.wrapping_add(0x22));
    let out = o.call(m, SI_GETMI2_OUTPUT, &[]).eax;
    let inc = o.call(m, SI_MI2_IS_INCREMENTAL, &[]).eax;
    o.call(m, SI_SETMI2_OUTPUT, &[0]);
    o.call(m, SI_SETMI2_INCREMENTAL, &[0]);
    let (fd, shader, scene) = (m.u32(task + 0x14), m.u32(task + 0x28), m.u32(task + 0x2c));
    match m.u32(task + 0x1c) {
        1 => {
            m.set_u32(FRAME, 1);
            write_scene_full(m, o, task, fd, shader, scene);
        }
        2 => {
            let f = m.u32(FRAME).wrapping_add(1);
            m.set_u32(FRAME, f);
            write_scene_incremental(m, o, task, fd, shader, scene);
        }
        _ => {}
    }
    m.set_u32(task, 0);
    o.call(m, SI_SETMI2_OUTPUT, &[out as u16 as i16 as i32 as u32]);
    o.call(m, SI_SETMI2_INCREMENTAL, &[inc as u16 as i16 as i32 as u32]);
}

/// `SI_threadStorageInit (0x080d0f80)` (the record's +8 area) and `SI_threadStorageFree (0x080d0fa8)`.
pub const SI_THREAD_STORAGE_INIT: Routine = Routine::new("SI_threadStorageInit", 0x080d_0f80);
pub const SI_THREAD_STORAGE_FREE: Routine = Routine::new("SI_threadStorageFree", 0x080d_0fa8);
pub const ALARM: Routine = Routine::new("plt:alarm", 0x080c_5108);
pub const SIGNAL: Routine = Routine::new("plt:signal", 0x080c_3bd8);
pub const SETJMP: Routine = Routine::new("plt:_setjmp", 0x080c_4e48);
pub const LONGJMP: Routine = Routine::new("plt:longjmp", 0x080c_4458);
/// `jmpError` of this file, and the two signal handlers' addresses.
pub const JMP_ERROR: u32 = 0x0882_42c0;
pub const HANDLER: u32 = 0x0854_d66c;
pub const HANDLER_SIGPIPE: u32 = 0x0854_d684;

/// handler (0x0854d66c): `longjmp(jmpError, 1)`. The original never returns; the stand-in's return is the Rust's.
pub fn handler(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, LONGJMP, &[JMP_ERROR, 1]);
}

/// handler_SIGPIPE (0x0854d684): `longjmp(jmpError, 2)`.
pub fn handler_sigpipe(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, LONGJMP, &[JMP_ERROR, 2]);
}

/// [soft 0x0854d540 si_shdbTranslatorProc]: the writer thread's entry. A null record writes 8 to address 0 (a fault in the
/// original, which means the state word; no case reaches it). Else `alarm(0)`, the thread's storage is set up at +8 and,
/// when the record's flag at +0x24 is set, `_setjmp` on `jmpError`: a return of 1 (a fault signal) sets the state word to 6,
/// a return of 2 (a broken pipe) to 3, both then remove the three handlers and free the storage; any other return (the first
/// one) installs `handler` for signals 7 and 11 and `handler_SIGPIPE` for 13. Without the flag, and after the installing,
/// the body `si_shdbTranslatorProc_Protected` runs, the storage is freed and, with the flag, the three handlers removed.
pub fn si_shdb_translator_proc(m: &mut Mem, o: &mut dyn Outside, t: u32) {
    if t == 0 {
        m.set_u32(0, 8);
        return;
    }
    o.call(m, ALARM, &[0]);
    o.call(m, SI_THREAD_STORAGE_INIT, &[t.wrapping_add(8)]);
    if m.u16(t.wrapping_add(0x24)) != 0 {
        let r = o.call(m, SETJMP, &[JMP_ERROR]).eax;
        if r == 1 || r == 2 {
            m.set_u32(t, if r == 1 { 6 } else { 3 });
            o.call(m, SIGNAL, &[7, 0]);
            o.call(m, SIGNAL, &[0xb, 0]);
            o.call(m, SIGNAL, &[0xd, 0]);
            o.call(m, SI_THREAD_STORAGE_FREE, &[]);
            return;
        }
        o.call(m, SIGNAL, &[7, HANDLER]);
        o.call(m, SIGNAL, &[0xb, HANDLER]);
        o.call(m, SIGNAL, &[0xd, HANDLER_SIGPIPE]);
    }
    si_shdb_translator_proc_protected(m, o, t);
    o.call(m, SI_THREAD_STORAGE_FREE, &[]);
    if m.u16(t.wrapping_add(0x24)) != 0 {
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
        o.call(m, SIGNAL, &[0xd, 0]);
    }
}
