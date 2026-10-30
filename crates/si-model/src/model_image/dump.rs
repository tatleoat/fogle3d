// SPDX-License-Identifier: MIT
//! The two debug dumps of `model.c` on the image: a tree of nodes written as text
//! (`SI_modelDump`) and a node's textures (`SI_modelDumpTextures`). The original writes to a
//! `FILE`; here the text goes to a byte vector (what `fprintf` would write), and `fflush`
//! has nothing to do.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;

use super::tree::next_son;

/// C's `%f` of a double: six decimals, `nan`/`inf` spelt the C way.
fn cf(v: f64) -> String {
    if v.is_nan() {
        if v.is_sign_negative() { "-nan".into() } else { "nan".into() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".into() } else { "inf".into() }
    } else {
        format!("{:.6}", v)
    }
}

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

/// `(%f %f %f)` row: three floats at `at`, widened to doubles, printed in memory order.
fn triple(m: &Mem, at: u32) -> String {
    format!("({} {} {})", cf(m.f32(at) as f64), cf(m.f32(at + 4) as f64), cf(m.f32(at + 8) as f64))
}

/// One row of `|%f %f %f %f|`.
fn quad(m: &Mem, at: u32) -> String {
    format!(
        "|{} {} {} {}|",
        cf(m.f32(at) as f64),
        cf(m.f32(at + 4) as f64),
        cf(m.f32(at + 8) as f64),
        cf(m.f32(at + 12) as f64)
    )
}

/// `SI_modelDump(file, model)` [soft 0x0846dc70 SI_modelDump]: a header, then for `model` and
/// every node below it in `SI_modelNextSon` order a block of lines: its address, father,
/// brother, son, name (the string the pointer at +4 names), type (+0x30), `nbdef` and
/// `curdef` (+0x34, +0x36), the `latdep` word (+0x44), the definition (+0x48), then the
/// matrices `hscl`/`hrot` (+0xa8..), `htrs`, `scal`, `vrot`, `mrot`, `trans`, `gscl`, `grot`,
/// `gtrs`, `gmat` as `%f` of the floats widened to doubles, and one `data[ i ]` line per
/// definition of **the first node** (the count is read from `model`, the array from the node
/// being written, as the original does), and an empty line.
pub fn model_dump(m: &Mem, out: &mut Vec<u8>, model: Ptr32<Model>) {
    put(out, "\nDUMP MODEL\n\n");
    let root = model.addr();
    let mut node = root;
    loop {
        put(out, &format!("address     : 0x{:x}\n", node));
        put(out, &format!("father      : 0x{:x}\n", m.u32(node + 0x2bc)));
        put(out, &format!("brother     : 0x{:x}\n", m.u32(node + 0x2c0)));
        put(out, &format!("son         : 0x{:x}\n", m.u32(node + 0x2c4)));
        // (the original hands %s the pointer in the word at +4; a null one is "(null)" in libc)
        let name = m.u32(node + 4);
        let text = if name == 0 { b"(null)".to_vec() } else { m.cstr(name) };
        out.extend_from_slice(b"name        : \"");
        out.extend_from_slice(&text);
        out.extend_from_slice(b"\"\n");
        put(out, &format!("type        : {}\n", m.i32(node + 0x30)));
        put(out, &format!("nbdef       : {}\n", m.u16(node + 0x34)));
        put(out, &format!("curdef      : {}\n", m.u16(node + 0x36)));
        put(out, &format!("latdep      : 0x{:x}\n", m.u32(node + 0x44)));
        put(out, &format!("definition  : 0x{:x}\n", m.u32(node + 0x48)));
        put(out, &format!("hscl        : {}\n", triple(m, node + 0xa8)));
        let rows = |out: &mut Vec<u8>, label: &str, at: u32| {
            for r in 0..4u32 {
                let head = if r == 0 { label } else { "              " };
                put(out, &format!("{}{}\n", head, quad(m, at + 16 * r)));
            }
        };
        rows(out, "hrot        : ", node + 0xb4);
        put(out, &format!("htrs        : {}\n", triple(m, node + 0xf4)));
        put(out, &format!("scal        : {}\n", triple(m, node + 0x100)));
        put(out, &format!("vrot        : {}\n", triple(m, node + 0x10c)));
        rows(out, "mrot        : ", node + 0x118);
        put(out, &format!("trans       : {}\n", triple(m, node + 0x158)));
        put(out, &format!("gscl        : {}\n", triple(m, node + 0x16c)));
        rows(out, "grot        : ", node + 0x178);
        put(out, &format!("gtrs        : {}\n", triple(m, node + 0x1b8)));
        rows(out, "gmat        : ", node + 0x1c4);
        let n = m.u16(root + 0x34) as i16;
        let data = m.u32(node + 0x38);
        let mut i: i16 = 0;
        // (the loop is a do-while on the 16-bit count: a count of 0 writes none)
        if m.u16(root + 0x34) != 0 {
            loop {
                put(out, &format!("         data[ {} ]    : 0x{:x}\n", i, m.u32(data.wrapping_add((i as i32 as u32).wrapping_mul(4)))));
                i = i.wrapping_add(1);
                if !((i as i32) < (n as u16 as i32)) {
                    break;
                }
            }
        }
        put(out, "\n");
        let nx = next_son(m, model, Ptr32::new(node));
        if nx.addr() == 0 {
            break;
        }
        node = nx.addr();
    }
}

/// `SI_modelDumpTextures(file, model, title)` [soft 0x08473140 SI_modelDumpTextures]: the
/// line `DUMP textures...`, the title, the model's address, its material count (`i16` +0x206)
/// and array (+0x20c); for each material (the record at the array's `i`-th word) its texture
/// count (the length of the chain from +0x6ec through the next links at +0x16c) and the
/// chain's records; then the model's own texture count (`i16` +0x216) and its chain
/// (+0x21c, links at +0x16c), and an empty line.
pub fn model_dump_textures(m: &Mem, out: &mut Vec<u8>, model: Ptr32<Model>, title: u32) {
    let md = model.addr();
    put(out, "DUMP textures...\n");
    out.extend_from_slice(&m.cstr(title));
    out.push(b'\n');
    put(out, &format!("mdl = 0x{:x}\n", md));
    put(out, &format!("  mdl.nbMtrl = {}\n", m.i16(md + 0x206)));
    put(out, &format!("  mdl.mtrl = 0x{:x}\n", m.u32(md + 0x20c)));
    let mut i: i32 = 0;
    while (m.i16(md + 0x206) as i32) > i && m.i16(md + 0x206) > 0 {
        let mtrl = m.u32(m.u32(md + 0x20c).wrapping_add((i as u32).wrapping_mul(4)));
        let mut count: i32 = 0;
        let mut t = m.u32(mtrl + 0x6ec);
        while t != 0 {
            t = m.u32(t + 0x16c);
            count += 1;
        }
        put(out, &format!("      mdl.mtrl[{}].nbTxt = {}\n", i, count));
        let mut t = m.u32(mtrl + 0x6ec);
        let mut j = 0;
        while j < count {
            put(out, &format!("         mdl.mtrl[{}].txt[{}] = 0x{:x}\n", i, j, t));
            t = m.u32(t + 0x16c);
            j += 1;
        }
        i += 1;
    }
    put(out, &format!("  mdl.nbTxt = {}\n", m.i16(md + 0x216)));
    let mut t = m.u32(md + 0x21c);
    let mut i: i32 = 0;
    if m.i16(md + 0x216) > 0 {
        loop {
            put(out, &format!("      mdl.txt[{}] = 0x{:x}\n", i, t));
            t = m.u32(t + 0x16c);
            i += 1;
            if !(i < m.i16(md + 0x216) as i32) {
                break;
            }
        }
    }
    put(out, "\n");
}
