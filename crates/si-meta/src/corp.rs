// SPDX-License-Identifier: MIT
//! `metaball.c`, reading a metaball scene (`.med`) and adding one ball (finding 0038).
//!
//! The file is text: ten header lines, then records of ten lines each. Of a record the reader
//! uses the third line's three numbers (translation), the fourth's (scaling), the fifth's
//! (rotation, degrees), the sixth's one number (the weight); the other lines are skipped.

use crate::metaball::{current_list, MetaCalls};
use si_hrc::image::meta::{meta_system_allocate, metaball_allocate, system};
use si_mem::{Mem, Outside};

const FMT_NAME: u32 = 0x086d_5bd5; // "%s.med"
const MODE_READ: u32 = 0x086d_5bdc; // "r"
const NAME_SYSTEM: u32 = 0x086d_5bde; // "meta"
const FMT3_TRANS: u32 = 0x086d_5be3;
const FMT3_SCALE: u32 = 0x086d_5bf4;
const FMT3_ROT: u32 = 0x086d_5c05;
const FMT1_WEIGHT: u32 = 0x086d_5c16;
const NAME_BALL: u32 = 0x086d_5c21; // "ball"
const NAME_BALL_ADD: u32 = 0x086d_5cb1; // "ball"
/// The float 0.01745329238474369 (degrees to radians) at 0x0865fb5c.
const DEG_TO_RAD: f32 = 0.017_453_292;
/// `SI_elemDicts (0x0867931c)`, `SI_procData (0x08679314)`, the word at 0x087fb9ac.
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SCHEMATIC_FLAG: u32 = 0x087f_b9ac;

/// (0x0845c254) `SI_readMetaCorp`: the scene `<name>.med` as one metasystem with a node and a ball
/// node per record (a son of the system's node), the whole added to the current list.
///
/// UNINIT: a line `sscanf` does not match leaves the numbers of the record before it (or
/// stale stack words in the first record); the port keeps them in the same frame, whose
/// content a check binary cannot hold equal to the original's, so those cases are not compared.
pub fn si_read_meta_corp(m: &mut Mem, o: &mut dyn Outside, name: u32) {
    // the original's frame: the saved base pointer sits just under the return address
    let frame = m.local(0x154);
    let ebp = frame + 0x150;
    let buf = ebp - 0x100;
    let (trans, scale, rot, weight) = (ebp - 0x10c, ebp - 0x124, ebp - 0x118, ebp - 0x134);
    o.sprintf1(m, buf, FMT_NAME, name);
    let f = o.fopen(m, buf, MODE_READ);
    if f != 0 {
        for _ in 0..10 {
            o.fgets(m, buf, 0x4f, f);
        }
        let sys = meta_system_allocate(m);
        let sysnode = o.si_model_allocate(m, NAME_SYSTEM);
        m.set_u32(sys + system::MODEL, sysnode);
        o.si_model_add_definition(m, sysnode, 7, sys);
        while o.fgets(m, buf, 0x4f, f) != 0 {
            o.fgets(m, buf, 0x4f, f);
            o.fgets(m, buf, 0x4f, f);
            o.sscanf3(m, buf, FMT3_TRANS, trans, trans + 4, trans + 8);
            o.fgets(m, buf, 0x4f, f);
            o.sscanf3(m, buf, FMT3_SCALE, scale, scale + 4, scale + 8);
            o.fgets(m, buf, 0x4f, f);
            o.sscanf3(m, buf, FMT3_ROT, rot, rot + 4, rot + 8);
            o.fgets(m, buf, 0x4f, f);
            o.sscanf1(m, buf, FMT1_WEIGHT, weight);
            o.fgets(m, buf, 0x4f, f);
            o.fgets(m, buf, 0x4f, f);
            let node = o.si_model_allocate(m, NAME_BALL);
            let ball = metaball_allocate(m, node);
            o.si_model_add_definition(m, node, 8, ball);
            m.set_u32(node + 0x2bc, sysnode);
            let first = m.u32(sysnode + 0x2c4);
            m.set_u32(node + 0x2c0, first);
            m.set_u32(sysnode + 0x2c4, node);
            let w = m.f32(weight) as f64;
            m.set_f64(ball + 0xd4, w);
            for k in 0..3 {
                // f32 times f32 is exact in 80 bits, then rounded to f32
                let v = (DEG_TO_RAD as f64 * m.f32(rot + 4 * k) as f64) as f32;
                m.set_f32(rot + 4 * k, v);
            }
            o.si_model_set_rotation(m, node, rot, 0, 0);
            o.si_model_set_translation(m, node, trans, 0, 0);
            o.si_model_set_scaling(m, node, scale, 0, 0);
        }
        o.fclose(m, f);
        let list = current_list(m);
        o.si_graflist_add_data(m, list, sysnode, 2, 1);
        o.si_refresh_system(m, 0, 0, 0x25);
        o.si_refresh_counter(m);
    }
    m.release(0x154);
}

/// (0x0845cb1c) `SI_addMetaball`: a new ball node in the active metasystem (a new system when
/// there is none), selected in the current list; with the schematic window up, the entries
/// that had a mark have it cleared while the window is sorted and set back after.
pub fn si_add_metaball(m: &mut Mem, o: &mut dyn Outside) {
    let mut head = 0u32;
    let mut tail = 0u32;
    if m.u16(SCHEMATIC_FLAG) != 0 {
        let list = current_list(m);
        if m.u16(list) != 0 {
            let mut e = m.u32(list + 4);
            while e != 0 {
                if m.u32(e + 8) != 0 {
                    let rec = m.si_malloc(0xc);
                    if head == 0 {
                        head = rec;
                    } else {
                        m.set_u32(tail + 8, rec);
                    }
                    tail = rec;
                    m.set_u32(rec, e);
                    let mark = m.u32(e + 8);
                    m.set_u32(rec + 4, mark);
                    m.set_u32(rec + 8, 0);
                }
                e = m.u32(e + 0x1c);
            }
        }
    }
    let sys = crate::metaball::si_get_active_meta_system(m, o, 1);
    let node = o.si_model_allocate(m, NAME_BALL_ADD);
    let ball = metaball_allocate(m, node);
    o.si_model_add_definition(m, node, 8, ball);
    let list = current_list(m);
    let ent = o.si_graflist_add_data(m, list, node, 2, 1);
    if sys != 0 {
        let list = current_list(m);
        let parent = o.si_find_gl_data(m, list, m.u32(sys + system::MODEL), 2);
        o.si_model_create_parent(m, parent, ent, 0);
    }
    if m.u16(SCHEMATIC_FLAG) != 0 {
        let mut r = head;
        while r != 0 {
            let e = m.u32(r);
            m.set_u32(e + 8, 0);
            r = m.u32(r + 8);
        }
        let win = m.local(4);
        m.set_u32(win, 0);
        let got = o.si_window_get_next(m, win);
        if got != 0 {
            let w = m.u32(win);
            let procd = m.u32(SI_PROC_DATA);
            let dict = m.u32(SI_ELEM_DICTS).wrapping_add(m.u32(procd).wrapping_mul(0x70));
            o.si_schematic_sort(m, w, dict, 1);
        }
        m.release(4);
        while head != 0 {
            let rec = head;
            let e = m.u32(rec);
            let saved = m.u32(rec + 4);
            m.set_u32(e + 8, saved);
            head = m.u32(rec + 8);
            m.free(rec);
        }
    }
    o.si_refresh_all(m, 1);
    o.si_refresh_counter(m);
}
