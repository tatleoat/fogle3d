// SPDX-License-Identifier: MIT
//! What the original's `evalScene` prints: the transformations of a scene's models, cameras
//! and lights after the update at a frame (finding 0017, section 14).
//!
//! `evalScene` is a sibling program of `soft` (finding 0001, "Sibling programs"): it links
//! the same code base and adds a `main` and five small functions of its own. The oracle ran
//! it on every sample scene (finding 0031, "Second pass"); its output is the reference the
//! whole evaluation of this crate is held against (`re/tools/si-motion/evalcheck`). This
//! module is what is needed to print the port's state in the same form, written from the
//! disassembly of `evalScene` (its own functions are named in its symbol table). The ledger
//! counts functions of `soft`, so the two functions of `evalScene` below are named
//! `name (evalScene 0xADDRESS)` and not cited in the ledger's form.
//!
//! - [`mark_all`]: the part of `main` (evalScene 0x080c2ef8) that reads the object list of
//!   `-F` (lines "MODELS *", "CAMERAS *", "LIGHTS *"): the work flag (element + 0x58) of every
//!   element of the dictionary is cleared, then set in the chapters the list names;
//! - [`frame_time`]: the time `main` evaluates a frame at;
//! - [`dump_frame`]: the loop of `main` that follows `SI_sceneUpdateItems(time, 1, NULL, 0)`;
//! - [`gn_info_dump`]: `gnInfoDump` (evalScene 0x080c2a5c), one object;
//! - [`fmt_f`]: glibc's `%5.4f`.

use crate::glob;
use crate::mem::MemExt;
use crate::off::{dict, elem, list};
use si_mem::{Addr, Mem};
use si_x87::F80;

/// The double constant `gnInfoDump` multiplies angles by (evalScene 0x086165a0): 180 / π.
pub const RAD2DEG: f64 = 57.295_779_513_082_32;

/// `SI_glbManip` + 0x20 (16 bits): `SI_matrixExtractXyzRotation` moves each angle by whole
/// turns to within π of the previous one when it is set. `SI_manipulationInit`, which
/// `evalScene`'s `main` calls first, sets it to 1.
pub const GLB_MANIP_KEEP_TURNS: u32 = 0x20;

/// glibc's `printf("%5.4f", v)` of a double: four decimals, the exact value rounded to
/// nearest with ties to even, the sign of a negative zero kept, at least five characters.
/// Rust's `{:.4}` rounds finite values the same way (exact decimal expansion, ties to
/// even); infinities and NaNs are written as glibc writes them (`inf`, `-nan` …).
pub fn fmt_f(v: f64) -> String {
    let s = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.4}")
    };
    format!("{s:>5}")
}

/// A `float` of the image as `printf` gets it: widened to a double (`fld`, `fstp QWORD`).
fn f(m: &Mem, a: Addr) -> f64 {
    m.f32(a) as f64
}

/// An angle in radians (a `float`) times [`RAD2DEG`] in the x87 unit, stored as the double
/// argument of `printf` (`fld QWORD c; fmul DWORD v` or `fld DWORD v; fmul QWORD c`, then
/// `fstp QWORD`): the product rounded to 64 bits, then to 53.
fn deg(v: f32) -> f64 {
    (F80::from_f64(RAD2DEG) * F80::from_f32(v)).to_f64()
}

fn three(out: &mut Vec<u8>, v: [f64; 3], end: u8) {
    out.extend_from_slice(format!("{} {} {}", fmt_f(v[0]), fmt_f(v[1]), fmt_f(v[2])).as_bytes());
    out.push(end);
}

fn one(out: &mut Vec<u8>, v: f64, end: u8) {
    out.extend_from_slice(fmt_f(v).as_bytes());
    out.push(end);
}

fn vec3(m: &Mem, a: Addr) -> [f64; 3] {
    [f(m, a), f(m, a + 4), f(m, a + 8)]
}

fn deg3(m: &Mem, a: Addr) -> [f64; 3] {
    [deg(m.f32(a)), deg(m.f32(a + 4)), deg(m.f32(a + 8))]
}

/// The name `gnInfoDump` prints: the element's full name (its name record, + 0x24) when
/// it is given one, else what `fallback` points at.
fn name_of(m: &Mem, el: Addr, fallback: Addr) -> Vec<u8> {
    let p = if el != 0 { m.ptr(m.ptr(el + elem::NODE) + 0x24) } else { fallback };
    cstr(m, p)
}

fn cstr(m: &Mem, mut p: Addr) -> Vec<u8> {
    let mut s = Vec::new();
    if p == 0 {
        // printf's "%s" of a null pointer
        return b"(null)".to_vec();
    }
    loop {
        let c = m.u8(p);
        if c == 0 || s.len() >= 4096 {
            return s;
        }
        s.push(c);
        p = p.wrapping_add(1);
    }
}

/// One object of the scene graph, as `gnInfoDump` (evalScene 0x080c2a5c) prints it; `gn` is
/// the graf node, `kind` its kind (the node's first word), `el` the element (may be null),
/// `local` the `-L` switch. By kind (the jump table at evalScene 0x086165a8):
///
/// | Kind | Header | Values |
/// |---|---|---|
/// | 2, 3 (model) | `Dump of model [name] of scene [scene]` | global: scaling (+0x16c), the angles `SI_matrixExtractXyzRotation` takes out of the rotation matrix (+0x178) starting from zero, in degrees, translation (+0x1b8); local (`-L`): +0x100, +0x10c in degrees, +0x158 |
/// | 4, 5 (camera, its interest) | `Dump of camera of scene [scene]` | position (+0x10), interest (+0x1c), roll (+0x30) in degrees |
/// | 6 (light with an interest) | `Dump of light [name] of scene [scene]` | position (+0x34), interest (+0x58), the two angles at +0x70, +0x74 in degrees |
/// | 8 (point light) | `Dump of light [name] …` | position (+0x34) |
/// | 7, any other | nothing | |
///
/// The name is the element's full name, or (without an element) the model's name (+4) or
/// the bytes of the light record itself. Each value is `%5.4f` of a double; groups of three
/// are separated by a tab, the line ends with a newline.
pub fn gn_info_dump(m: &mut Mem, gn: Addr, kind: i32, scene: &[u8], el: Addr, local: bool, out: &mut Vec<u8>) {
    let data = m.ptr(gn + 4);
    let sc = String::from_utf8_lossy(scene).into_owned();
    match kind {
        2 | 3 => {
            let name = name_of(m, el, m.ptr(data + 4));
            out.extend_from_slice(format!("EVALSCENE: Dump of model [{}] of scene [{sc}]\n", String::from_utf8_lossy(&name)).as_bytes());
            if local {
                three(out, vec3(m, data + 0x100), b'\t');
                three(out, deg3(m, data + 0x10c), b'\t');
                three(out, vec3(m, data + 0x158), b'\n');
            } else {
                three(out, vec3(m, data + 0x16c), b'\t');
                // the angles start as three zeros (locals cleared before the call)
                let keep = m.i16(glob::SI_GLB_MANIP + GLB_MANIP_KEEP_TURNS) != 0;
                let v = si_math::algebra::matrix_extract_xyz_rotation(&m.matrix(data + 0x178), &si_math::types::Vector::new(0.0, 0.0, 0.0), keep);
                three(out, [deg(v.x), deg(v.y), deg(v.z)], b'\t');
                three(out, vec3(m, data + 0x1b8), b'\n');
            }
        }
        4 | 5 => {
            out.extend_from_slice(format!("EVALSCENE: Dump of camera of scene [{sc}]\n").as_bytes());
            three(out, vec3(m, data + 0x10), b'\t');
            three(out, vec3(m, data + 0x1c), b'\t');
            one(out, deg(m.f32(data + 0x30)), b'\n');
        }
        6 => {
            let name = name_of(m, el, data);
            out.extend_from_slice(format!("EVALSCENE: Dump of light [{}] of scene [{sc}]\n", String::from_utf8_lossy(&name)).as_bytes());
            three(out, vec3(m, data + 0x34), b'\t');
            three(out, vec3(m, data + 0x58), b'\t');
            one(out, deg(m.f32(data + 0x70)), b'\t');
            one(out, deg(m.f32(data + 0x74)), b'\n');
        }
        8 => {
            let name = name_of(m, el, data);
            out.extend_from_slice(format!("EVALSCENE: Dump of light [{}] of scene [{sc}]\n", String::from_utf8_lossy(&name)).as_bytes());
            three(out, vec3(m, data + 0x34), b'\n');
        }
        _ => {}
    }
}

/// The element's work flag (+0x58), which `main` uses to mark the objects to dump.
pub const WORK_FLAG: u32 = 0x58;

fn lists(m: &Mem) -> (Addr, i32) {
    let d = glob::cur_dict(m);
    (m.ptr(d + dict::LISTS), m.i32(d + dict::NB_LISTS))
}

fn chap_name(m: &Mem, l: Addr) -> Vec<u8> {
    cstr(m, m.ptr(l + list::NAME))
}

/// The object list of `-F` as `main` applies it before the first frame: the work flag of
/// every element of the current dictionary that is in use and has a graf node is cleared;
/// then for each line "<chapter> *" every such element of that chapter (found by name,
/// `SI_elemdictFindListByName`) gets the flag 1. Answers how many elements were marked.
/// (A line naming one object, or the word `camera`, is looked up by `evalGetElemFromName`
/// (evalScene 0x080c2de8) or `SI_edGetElemFromCamera`: not reproduced; the oracle's list
/// has only "*" lines.)
pub fn mark_all(m: &mut Mem, chapters: &[&[u8]]) -> usize {
    let (ls, n) = lists(m);
    for c in 0..n.max(0) as u32 {
        let l = ls + c * list::SIZE;
        for i in 0..m.i32(l + list::NB_ELEMS).max(0) as u32 {
            let e = m.ptr(l + list::ELEMS) + i * elem::SIZE;
            if m.i16(e + elem::STATUS) == 0 && m.ptr(e + elem::AUX_DATA) != 0 {
                m.set_i32(e + WORK_FLAG, 0);
            }
        }
    }
    let mut marked = 0;
    for want in chapters {
        for c in 0..n.max(0) as u32 {
            let l = ls + c * list::SIZE;
            if chap_name(m, l) != *want {
                continue;
            }
            for i in 0..m.i32(l + list::NB_ELEMS).max(0) as u32 {
                let e = m.ptr(l + list::ELEMS) + i * elem::SIZE;
                if m.i16(e + elem::STATUS) == 0 && m.ptr(e + elem::AUX_DATA) != 0 {
                    m.set_i32(e + WORK_FLAG, 1);
                    marked += 1;
                }
            }
            break;
        }
    }
    marked
}

/// The time `main` evaluates frame `frame` at (without `-t`): the frame number, made a
/// `float` (`fild`, `frndint` toward zero, `fstp DWORD`), divided by `SI_frameRate` in the
/// unit and stored as a `float`.
pub fn frame_time(m: &Mem, frame: i32) -> f32 {
    let t = F80::from_i32(frame).to_f32();
    (F80::from_f32(t) / glob::frame_rate(m)).to_f32()
}

/// What `main` prints for one frame after the update: every element of the current
/// dictionary, chapter by chapter in dictionary order, that is in use, has a graf node and
/// carries the work flag, through [`gn_info_dump`]; then an empty line. The header line
/// "EVALSCENE: Evaluating scene at frame N" comes before the update and is not included.
pub fn dump_frame(m: &mut Mem, scene: &[u8], local: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let (ls, n) = lists(m);
    for c in 0..n.max(0) as u32 {
        let l = ls + c * list::SIZE;
        let mut i = 0u32;
        while i < m.i32(l + list::NB_ELEMS).max(0) as u32 {
            let e = m.ptr(l + list::ELEMS) + i * elem::SIZE;
            let gn = m.ptr(e + elem::AUX_DATA);
            if m.i16(e + elem::STATUS) == 0 && gn != 0 && m.i32(e + WORK_FLAG) != 0 {
                let kind = m.i32(gn);
                gn_info_dump(m, gn, kind, scene, e, local, &mut out);
            }
            i += 1;
        }
    }
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_5_4() {
        assert_eq!(fmt_f(0.0), "0.0000");
        assert_eq!(fmt_f(-0.0), "-0.0000");
        assert_eq!(fmt_f(-0.00001), "-0.0000");
        assert_eq!(fmt_f(0.03125), "0.0312");
        assert_eq!(fmt_f(0.09375), "0.0938");
        assert_eq!(fmt_f(-1250.1726), "-1250.1726");
        assert_eq!(fmt_f(f64::NAN), "  nan");
        assert_eq!(fmt_f(-f64::NAN), " -nan");
        assert_eq!(fmt_f(f64::NEG_INFINITY), " -inf");
    }

    #[test]
    fn degrees_through_the_unit() {
        // π/2 as a float, times the double constant, rounded twice
        let v = std::f32::consts::FRAC_PI_2;
        assert_eq!(fmt_f(deg(v)), "90.0000");
        assert_eq!(deg(0.0), 0.0);
    }
}
