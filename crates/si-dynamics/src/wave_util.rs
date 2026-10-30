// SPDX-License-Identifier: MIT
//! `wave_util.c` of Softimage|3D 4.0 (0x084c8ef0 … 0x084ca1e2): the pickers and finders of the wave code (finding
//! 0045). Only the functions that need no file input or output are here yet; the callees of other files are hooks on
//! [`ParticleCalls`]. A graph node (our names): +0 the kind, +4 the item. An event: +0 the kind, +6 a short code
//! (0x65 ends the loop), +0xc the graph port.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside, Routine};
use si_x87::{x, F80};

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The wave that is the current selection of the graph list [soft 0x084c9674 si_waveFindFromSelection]: the item of the
/// graph node at +8 of the current list when its kind is 0xb, else 0.
pub fn si_wave_find_from_selection(m: &Mem) -> u32 {
    let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
    let node = m.u32(list + 8);
    if node == 0 || m.u32(node) != 0xb {
        0
    } else {
        m.u32(node + 4)
    }
}

/// The wave whose icon model is `model` [soft 0x084c96a8 waveFindFromIcon]: a walk of the list at +0x7c of the status
/// record (next at +0x78, icon at +0x74); 0 when none.
pub fn wave_find_from_icon(m: &Mem, status: u32, model: u32) -> u32 {
    let mut w = m.u32(status + 0x7c);
    while w != 0 && model != m.u32(w + 0x74) {
        w = m.u32(w + 0x78);
    }
    w
}

/// Waits for the user to pick a model [soft 0x084c96c8 si_waveGetMdl]: events are read until one that is neither a pick
/// (kind 5) nor kind 0, or whose code is 0x65, which gives 0. A pick on a graph node of kind 2 or 3 whose item's kind
/// (+0x30) is not 7 or 8 flashes the node (5 times) and keeps the item (if the item's kind is 7 or 8 the item is still kept
/// but not flashed: the original sets it before testing); the loop ends when an item is held.
pub fn si_wave_get_mdl(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let mut held = 0u32;
    loop {
        let ev = o.si_get_event(m);
        let kind = m.i32(ev);
        if (kind != 5 && kind != 0) || m.u16(ev + 6) == 0x65 {
            return 0;
        }
        if kind == 5 {
            let node = o.si_gl_select(m, m.u32(ev + 0xc));
            if node != 0 && m.u32(node).wrapping_sub(2) < 2 {
                held = m.u32(node + 4);
                if 1 < m.u32(held + 0x30).wrapping_sub(7) {
                    o.si_flash_object(m, node, 5);
                }
            }
        }
        if held != 0 {
            return held;
        }
    }
}

/// Waits for the user to pick a wave [soft 0x084c9740 si_waveGetWave]: as above, but an event that is neither a pick nor
/// kind 0 gives what is held so far, code 0x65 gives 0, and a pick on a node of kind 0xb keeps its item and flashes it.
pub fn si_wave_get_wave(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let mut held = 0u32;
    loop {
        let ev = o.si_get_event(m);
        let kind = m.i32(ev);
        if kind != 5 && kind != 0 {
            return held;
        }
        if m.u16(ev + 6) == 0x65 {
            return 0;
        }
        if kind == 5 {
            let node = o.si_gl_select(m, m.u32(ev + 0xc));
            if node != 0 && m.u32(node) == 0xb {
                held = m.u32(node + 4);
                o.si_flash_object(m, node, 5);
            }
        }
        if held != 0 {
            return held;
        }
    }
}

/// Waits for the user to pick a spline [soft 0x084c97ac waveGetSpl]: kind 5 events select a node; a node of kind 2 or 3
/// whose item has kind 6 (+0x30) gives the entry `item[+0x38][ushort at +0x36]` as the held value; any event of another
/// kind than 5 and 0 gives 0.
pub fn wave_get_spl(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let mut held = 0u32;
    loop {
        let ev = o.si_get_event(m);
        let kind = m.i32(ev);
        if kind == 5 {
            let node = o.si_gl_select(m, m.u32(ev + 0xc));
            if node != 0 && m.u32(node).wrapping_sub(2) < 2 {
                let item = m.u32(node + 4);
                if m.u32(item + 0x30) == 6 {
                    let idx = m.u16(item + 0x36) as u32;
                    held = m.u32(m.u32(item + 0x38).wrapping_add(idx * 4));
                }
            }
        } else if kind != 0 {
            return 0;
        }
        if held != 0 {
            return held;
        }
    }
}

/// Freezes a wave on its attached model [soft 0x084ca1c4 si_waveFreezeWave]: `waveFreezeMdl(wave, wave[+0x6c])` when a
/// model is attached.
pub fn si_wave_freeze_wave(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    let model = m.u32(wave + 0x6c);
    if model != 0 {
        wave_freeze_mdl(m, o, wave, model);
    }
}

/// Freezes a wave on a model: the model's own geometry takes the shape the wave gave it, and the wave is detached
/// [soft 0x084c9af4 waveFreezeMdl]. A model that is not the wave's attached one gets the message "Wave does not belong
/// to that model." (level 0); a model with no wave list does nothing; a branch wave (+0x70 is 1) gives a message at level 3.
/// By the model's kind (+0x30) the points of the geometry (+0x48) are transferred back to the definition selected by
/// +0x36 of the list at +0x38: 1 patch, 2 face, 4 mesh, 6 spline, 9 NURBS curve, 10 NURBS surface; any other kind
/// gives a message at level 3 and stops. Then the model is detached with flag 1.
pub fn wave_freeze_mdl(m: &mut Mem, o: &mut dyn Outside, wave: u32, model: u32) {
    if m.u32(wave + 0x6c) != model {
        o.si_status_display_message(m, 0x086d_9ae0, 0);
        return;
    }
    if m.u32(model + 0x2e8) == 0 {
        return;
    }
    if m.u32(wave + 0x70) == 1 {
        o.si_status_display_message(m, 0x086d_9b20, 3);
        return;
    }
    let a = m.u32(model + 0x48);
    let b = m.u32(m.u32(model + 0x38).wrapping_add(m.u16(model + 0x36) as u32 * 4));
    match m.u32(model + 0x30) {
        1 => o.si_patch_transfer_points(m, a, b),
        2 => o.si_face_transfer_points(m, a, b),
        4 => o.si_mesh_transfer_vertices(m, a, b),
        6 => o.si_spline_transfer_points(m, a, b),
        9 => o.si_nurbs_crv_transfer_points_knots(m, a, b),
        10 => o.si_nurbs_srf_transfer_points_knots(m, a, b),
        _ => {
            o.si_status_display_message(m, 0x086d_9b60, 3);
            return;
        }
    }
    si_wave_detach_mdl(m, o, wave, model, 1);
}

/// Detaches a model from a wave [soft 0x084c9c48 SI_waveDetachMdl]: nothing for a null model; a model that is not the
/// wave's attached one gets the message "..." (level 0). The wave's link (+0x6c) is cleared, the editor unlinks the wave
/// from the model, and the wave's node is taken out of the model's wave list (+0x2e8; nodes: wave at +0, next at +8; the
/// node is freed); with no list nothing more happens. Then the icon model's scale (+0x100..+0x108) is multiplied by
/// the model's (+0x16c..+0x174), its 3 x 3 rotation (+0x118) by the model's (+0x178), the rotation angles (+0x10c)
/// are extracted again, its position (+0x158..+0x160) is carried through the model's matrix (+0x1c4: rows of
/// `x*m[0] + y*m[4] + z*m[8] + m[12]`, all in 80 bits and rounded once), the icon's matrix is updated and a non-zero
/// `flag` (its low 16 bits) updates the dependencies.
pub fn si_wave_detach_mdl(m: &mut Mem, o: &mut dyn Outside, wave: u32, model: u32, flag: i32) {
    let flag = flag as i16;
    if model == 0 {
        return;
    }
    if m.u32(wave + 0x6c) != model {
        o.si_status_display_message(m, 0x086d_9ba0, 0);
        return;
    }
    m.set_u32(wave + 0x6c, 0);
    o.si_ed_model_unlink_wave(m, model, wave);
    let head = m.u32(model + 0x2e8);
    if head == 0 {
        return;
    }
    if m.u32(head) == wave {
        let next = m.u32(head + 8);
        m.set_u32(model + 0x2e8, next);
        m.free(head);
    } else {
        let mut prev = head;
        let mut cur = m.u32(prev + 8);
        while cur != 0 && m.u32(cur) != wave {
            prev = m.u32(prev + 8);
            cur = m.u32(prev + 8);
        }
        let victim = m.u32(prev + 8);
        if victim != 0 {
            let next = m.u32(victim + 8);
            m.set_u32(prev + 8, next);
            m.free(victim);
        }
    }
    let icon = m.u32(wave + 0x74);
    for k in 0..3u32 {
        let v = (x(m.f32(icon + 0x100 + 4 * k)) * x(m.f32(model + 0x16c + 4 * k))).to_f32();
        m.set_f32(icon + 0x100 + 4 * k, v);
    }
    let icon = m.u32(wave + 0x74);
    o.si_matrix_multiply_sub3(m, icon + 0x118, model + 0x178, icon + 0x118);
    let icon = m.u32(wave + 0x74);
    o.si_matrix_extract_xyz_rotation(m, icon + 0x118, icon + 0x10c);
    let icon = m.u32(wave + 0x74);
    let p = [x(m.f32(icon + 0x158)), x(m.f32(icon + 0x15c)), x(m.f32(icon + 0x160))];
    let mm = |m: &Mem, i: u32| x(m.f32(model + 0x1c4 + 4 * i));
    let nx = ((p[0] * mm(m, 0) + p[1] * mm(m, 4)) + p[2] * mm(m, 8)) + mm(m, 12);
    m.set_f32(icon + 0x158, nx.to_f32());
    let ny = ((p[1] * mm(m, 5) + p[0] * mm(m, 1)) + p[2] * mm(m, 9)) + mm(m, 13);
    let icon = m.u32(wave + 0x74);
    m.set_f32(icon + 0x15c, ny.to_f32());
    let nz = ((p[0] * mm(m, 2) + p[1] * mm(m, 6)) + p[2] * mm(m, 10)) + mm(m, 14);
    let icon = m.u32(wave + 0x74);
    m.set_f32(icon + 0x160, nz.to_f32());
    let icon = m.u32(wave + 0x74);
    o.si_model_update_matrix(m, icon);
    if flag != 0 {
        o.si_model_update_dependency(m, model, 0);
    }
}

/// Scales the icon of a wave [soft 0x084c9e30 si_waveResizeIcon]: the vertices (0x14 bytes each, count at +0x14, array at
/// +0 of the record at +0x48 of the icon model) are multiplied by the vector (x, y, z of `v`) by the wave's profile kind
/// (+8): 0 gives (v0, v1, v0), 1 gives (v0, v1) and leaves z, 2 gives (v1, v1, v1); other kinds do nothing.
pub fn si_wave_resize_icon(m: &mut Mem, wave: u32, v: u32) {
    let icon = m.u32(wave + 0x74);
    let geo = m.u32(icon + 0x48);
    let n = m.i32(geo + 0x14);
    let mul: [Option<u32>; 3] = match m.i32(wave + 8) {
        0 => [Some(0), Some(4), Some(0)],
        1 => [Some(0), Some(4), None],
        2 => [Some(4), Some(4), Some(4)],
        _ => return,
    };
    for i in 0..n.max(0) as u32 {
        for (k, f) in mul.iter().enumerate() {
            if let Some(f) = f {
                let a = m.u32(geo) + i * 0x14 + 4 * k as u32;
                let r = (x(m.f32(a)) * x(m.f32(v + f))).to_f32();
                m.set_f32(a, r);
            }
        }
    }
}

/// Rescales the keys of a function curve of a wave to the wave's frame range [soft 0x084ca0dc waveResizeFunctionCurve]:
/// for a curve (key count at +0, keys of 8 bytes at +0x1c, the frame the first float) with more than one key, each key's
/// frame becomes `frame * s + (first_frame_of_wave - first_key * s)` with `s = (last_wave_frame - first_wave_frame) /
/// (last_key - first_key)` (80 bits, one rounding on the store).
pub fn wave_resize_function_curve(m: &mut Mem, wave: u32, fcv: u32) {
    let n = m.i32(fcv);
    if n <= 1 || m.u32(fcv + 0x1c) == 0 {
        return;
    }
    let (f0, f1) = (m.i32(wave + 0x10), m.i32(wave + 0x14));
    let keys = m.u32(fcv + 0x1c);
    let span = F80::from_i32(f1.wrapping_sub(f0));
    let s = span / (x(m.f32(keys + (n as u32) * 8 - 8)) - x(m.f32(keys)));
    let a = F80::from_i32(f0) - x(m.f32(keys)) * s;
    for i in 0..n as u32 {
        let k = m.u32(fcv + 0x1c) + i * 8;
        let r = (s * x(m.f32(k)) + a).to_f32();
        m.set_f32(k, r);
    }
}

const FSCANF: Routine = Routine::new("fscanf", 0x080c_4058);
const STDERR: u32 = 0x0878_0c88;
const WAVE_STATUS: u32 = 0x0882_2120;

/// `printf("%.<prec>f", v)` of a double: the library prints inf and nan in lower case, with the sign of a NaN.
fn fmt_prec(v: f64, prec: usize) -> String {
    if v.is_nan() {
        if v.is_sign_negative() { "-nan" } else { "nan" }.to_string()
    } else if v.is_infinite() {
        if v < 0.0 { "-inf" } else { "inf" }.to_string()
    } else {
        format!("{:.*}", prec, v)
    }
}

/// `%s` of a C string pointer: the text, `(null)` for a null pointer.
fn fmt_s(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

/// Writes the graph of a wave function curve as text [soft 0x084c8ef0 wavePrintGraph]: `DATA "GRAPH   2  <n>"` and one line
/// `  %.2f  %.2f` per key (frame and value, as doubles; the keys are the 8-byte records at +0x1c, the count is the first word).
pub fn wave_print_graph(m: &mut Mem, o: &mut dyn Outside, file: u32, graph: u32) {
    o.print(file, format!("DATA \"GRAPH   2  {}\"\n", m.i32(graph)).as_bytes());
    let mut i = 0u32;
    while (i as i32) < m.i32(graph) {
        let k = m.u32(graph + 0x1c).wrapping_add(i.wrapping_mul(8));
        let t = format!("  {}  {}\n", fmt_prec(m.f32(k) as f64, 2), fmt_prec(m.f32(k + 4) as f64, 2));
        o.print(file, t.as_bytes());
        i += 1;
    }
}

/// Writes a wave as text [soft 0x084c8f58 si_wavePrintWave]: the icon model's scale, rotation and translation are read
/// (`SI_modelState`), then the block `Wave` with its name, type, displacement, frame start and end, velocity, acceleration,
/// falloff start and end (`%.4f`), periodic flag, amplitude extrapolation, the amplitude count and its pairs, the three
/// vectors and `End`.
pub fn si_wave_print_wave(m: &mut Mem, o: &mut dyn Outside, file: u32, wave: u32) {
    let loc = m.local(0x28);
    let (sc, rot, tr) = (loc, loc + 0xc, loc + 0x18);
    let model = m.u32(wave + 0x74);
    o.si_model_state(m, model, sc, rot, tr);
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"Wave\n");
    out.extend_from_slice(b"  name ");
    out.extend_from_slice(&fmt_s(m, m.u32(wave + 4)));
    out.extend_from_slice(b"\n");
    for (label, off) in [("type", 8u32), ("displacement", 0xc), ("frame_start", 0x10), ("frame_end", 0x14)] {
        out.extend_from_slice(format!("  {} {}\n", label, m.i32(wave + off)).as_bytes());
    }
    for (label, off) in [("velocity", 0x18u32), ("acceleration", 0x1c), ("falloff_start", 0x20), ("falloff_end", 0x24)] {
        out.extend_from_slice(format!("  {}  {}\n", label, fmt_prec(m.f32(wave + off) as f64, 4)).as_bytes());
    }
    out.extend_from_slice(format!("  periodic {}\n", m.i32(wave + 0x28)).as_bytes());
    out.extend_from_slice(format!("  amp_extrap {}\n", m.i32(wave + 0x30)).as_bytes());
    out.extend_from_slice(format!("  amplitude {}\n", m.i32(wave + 0x2c)).as_bytes());
    let mut i = 0u32;
    while (i as i32) < m.i32(wave + 0x2c) {
        let k = m.u32(wave + 0x48).wrapping_add(i.wrapping_mul(8));
        out.extend_from_slice(format!("      {}   {}\n", fmt_prec(m.f32(k) as f64, 4), fmt_prec(m.f32(k + 4) as f64, 4)).as_bytes());
        i += 1;
    }
    for (label, v) in [("scale", sc), ("rotate", rot), ("translate", tr)] {
        let t = format!(
            "  {} {}  {}  {}\n",
            label,
            fmt_prec(m.f32(v) as f64, 4),
            fmt_prec(m.f32(v + 4) as f64, 4),
            fmt_prec(m.f32(v + 8) as f64, 4)
        );
        out.extend_from_slice(t.as_bytes());
    }
    out.extend_from_slice(b"End\n");
    m.release(0x28);
    o.print(file, &out);
}

/// `fscanf(file, fmt, ptrs...)` through the library routine.
pub(crate) fn fscanf(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, ptrs: &[u32]) -> i32 {
    let mut words = vec![file, fmt];
    words.extend_from_slice(ptrs);
    o.call(m, FSCANF, &words).eax as i32
}

/// Reads a wave from text [soft 0x084c9180 si_waveReadWave]: keywords (`%s`, at most as many characters as the program's
/// 40-byte buffer holds, more overruns it as the original does) are read until the end of the input (or `End`), which
/// gives 1. `name` reads into the wave's name buffer and then renames every other wave with the same name to `name.N`
/// (a fresh block of length + 3 bytes, N counted in the status record at +0x90, the old name freed); `type`, `displacement`,
/// `frame_start`, `frame_end`, `periodic`, `amp_extrap` are `%d` into +8, +0xc, +0x10, +0x14, +0x28, +0x30;
/// `velocity`, `acceleration`, `falloff_start`, `falloff_end` are `%f` into +0x18..+0x24; `amplitude` reads the count
/// into +0x2c and, with a count above one, a new block of count * 8 bytes at +0x48 with the pairs and their extent
/// (the least x, greatest x, least y and greatest y into +0x38..+0x44: a value below the least becomes the least, else a
/// value above the greatest becomes the greatest); `scale`, `rotate`, `translate` read three floats into the three
/// vectors given. A keyword that is none of these gives "Warning: invalid keyword %s in wave file." on stderr.
pub fn si_wave_read_wave(m: &mut Mem, o: &mut dyn Outside, file: u32, wave: u32, scale: u32, rotate: u32, translate: u32) -> i32 {
    let kw = m.local(40);
    let r = (|| {
        loop {
            if fscanf(m, o, file, 0x086d_99a4, &[kw]) != 1 {
                return 1;
            }
            let word = m.cstr(kw);
            let simple = |w: &[u8]| -> Option<(u32, u32)> {
                Some(match w {
                    b"type" => (0x086d_99ba, 8),
                    b"displacement" => (0x086d_99ca, 0xc),
                    b"frame_start" => (0x086d_99d9, 0x10),
                    b"frame_end" => (0x086d_99e6, 0x14),
                    b"velocity" => (0x086d_99f2, 0x18),
                    b"acceleration" => (0x086d_9a02, 0x1c),
                    b"falloff_start" => (0x086d_9a13, 0x20),
                    b"falloff_end" => (0x086d_9a22, 0x24),
                    b"periodic" => (0x086d_9a2e, 0x28),
                    b"amp_extrap" => (0x086d_9a3c, 0x30),
                    _ => return None,
                })
            };
            if word == b"name" {
                let name = m.u32(wave + 4);
                fscanf(m, o, file, 0x086d_99ac, &[name]);
                let mut other = m.u32(WAVE_STATUS + 0x7c);
                while other != 0 {
                    let a = m.cstr(m.u32(other + 4));
                    let b = m.cstr(m.u32(wave + 4));
                    if a == b && wave != other {
                        let nm = m.u32(wave + 4);
                        let len = m.strlen(nm);
                        let buf = m.malloc(len + 3);
                        let n = m.i32(WAVE_STATUS + 0x90);
                        let mut text = m.cstr(nm);
                        text.extend_from_slice(format!(".{}", n).as_bytes());
                        for (k, byte) in text.iter().enumerate() {
                            m.set_u8(buf + k as u32, *byte);
                        }
                        m.set_u8(buf + text.len() as u32, 0);
                        m.set_i32(WAVE_STATUS + 0x90, n.wrapping_add(1));
                        m.free(m.u32(other + 4));
                        m.set_u32(other + 4, buf);
                    }
                    other = m.u32(other + 0x78);
                }
            } else if let Some((fmt, off)) = simple(&word) {
                fscanf(m, o, file, fmt, &[wave + off]);
            } else if word == b"amplitude" {
                fscanf(m, o, file, 0x086d_9a49, &[wave + 0x2c]);
                let n = m.i32(wave + 0x2c);
                if 1 < n {
                    let blk = m.malloc((n as u32) << 3);
                    m.set_u32(wave + 0x48, blk);
                    fscanf(m, o, file, 0x086d_9a4c, &[blk, blk + 4]);
                    let (mut minx, mut maxx) = (m.f32(blk), m.f32(blk));
                    let (mut miny, mut maxy) = (m.f32(blk + 4), m.f32(blk + 4));
                    let tmp = m.local(8);
                    let mut i = 1u32;
                    while (i as i32) < m.i32(wave + 0x2c) {
                        fscanf(m, o, file, 0x086d_9a53, &[tmp, tmp + 4]);
                        let (px, py) = (m.f32(tmp), m.f32(tmp + 4));
                        if px < minx {
                            minx = px;
                        } else if px > maxx {
                            maxx = px;
                        }
                        if py < miny {
                            miny = py;
                        } else if py > maxy {
                            maxy = py;
                        }
                        m.set_f32(blk + i * 8, px);
                        m.set_f32(blk + i * 8 + 4, py);
                        i += 1;
                    }
                    m.release(8);
                    m.set_f32(wave + 0x38, minx);
                    m.set_f32(wave + 0x3c, maxx);
                    m.set_f32(wave + 0x40, miny);
                    m.set_f32(wave + 0x44, maxy);
                }
            } else if word == b"scale" {
                fscanf(m, o, file, 0x086d_9a60, &[scale, scale + 4, scale + 8]);
            } else if word == b"rotate" {
                fscanf(m, o, file, 0x086d_9a70, &[rotate, rotate + 4, rotate + 8]);
            } else if word == b"translate" {
                fscanf(m, o, file, 0x086d_9a83, &[translate, translate + 4, translate + 8]);
            } else if word == b"End" {
                return 1;
            } else {
                let mut t = b"Warning: invalid keyword ".to_vec();
                t.extend_from_slice(&word);
                t.extend_from_slice(b" in wave file.\n");
                let stream = m.u32(STDERR);
                o.print(stream, &t);
            }
        }
    })();
    m.release(40);
    r
}
