// SPDX-License-Identifier: MIT
//! `wave_dialog.c` of Softimage|3D 4.0 (0x084ca1f0 …): the data moves of the wave dialog (finding 0045). The dialog's own
//! window code (`si_dialogWave`) is user interface and not ported. The dialog toolkit routines are reached on the image
//! through [`Routine`]s: `SI_setTexed(dialog, item, text)`, `SI_setStateItem(dialog, item, state)`,
//! `SI_getTexed(dialog, item, &text)`, `SI_getStateItem(dialog, item, &short)` and, answering in st0,
//! `SI_getTexedFloat(dialog, item)`; `SI_getTexedLong(dialog, item)` answers in eax.

use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

const SI_SET_TEXED: Routine = Routine::new("SI_setTexed", 0x0838_bd10);
const SI_SET_STATE_ITEM: Routine = Routine::new("SI_setStateItem", 0x0838_b824);
const SI_GET_TEXED: Routine = Routine::new("SI_getTexed", 0x0838_c2c0);
const SI_GET_STATE_ITEM: Routine = Routine::new("SI_getStateItem", 0x0838_bcec);
const SI_GET_TEXED_FLOAT: Routine = Routine::new("SI_getTexedFloat", 0x0838_c138);
const SI_GET_TEXED_LONG: Routine = Routine::new("SI_getTexedLong", 0x0838_c204);
/// The empty string (the default of the second text field).
const S_EMPTY: u32 = 0x086d_9bf1;

fn set_state(m: &mut Mem, o: &mut dyn Outside, dialog: u32, item: u32, state: u32) {
    o.call(m, SI_SET_STATE_ITEM, &[dialog, item, state]);
}

/// `sprintf(buf, "%.4f", v)` of a float (the library prints inf and nan in lower case, a NaN with its sign).
fn fmt4(v: f32) -> Vec<u8> {
    let v = v as f64;
    if v.is_nan() {
        if v.is_sign_negative() { "-nan" } else { "nan" }.to_string()
    } else if v.is_infinite() {
        if v < 0.0 { "-inf" } else { "inf" }.to_string()
    } else {
        format!("{:.4}", v)
    }
    .into_bytes()
}

/// Puts the text `text` into the dialog's field `item` (a copy in a local of 40 bytes, as the original's `sprintf` does).
fn set_text(m: &mut Mem, o: &mut dyn Outside, dialog: u32, item: u32, buf: u32, text: &[u8]) {
    let mut t = text.to_vec();
    t.push(0);
    m.set_bytes(buf, &t);
    o.call(m, SI_SET_TEXED, &[dialog, item, buf]);
}

/// Fills the wave dialog from a wave [soft 0x084ca664 waveSetDg] (`wave`, `dialog`, `elem`: the animation element whose first
/// word points at a record with the wave's text at +0x14 and a second text at +0x10, empty when null): field 4 and 0x1e
/// are those texts; the toggles 6, 7, 8 are cleared and the one for the wave type (+8: 0, 1, 2) set; toggle 0x11 is
/// whether +0x30 is 1, 0x12 whether +0x28 is non-0; the toggles 0x13, 0x1b, 0x1c are cleared and the one for +0xc (0, 1, 2)
/// set; the fields 0x14 and 0x15 get the floats +0x18 and +0x1c, 0x16 and 0x17 the integers +0x10 and +0x14, 0x18 and 0x19 the
/// floats +0x20 and +0x24 (floats as `%.4f`, integers as `%d`).
pub fn wave_set_dg(m: &mut Mem, o: &mut dyn Outside, wave: u32, dialog: u32, elem: u32) {
    let buf = m.local(0x28);
    let rec = m.u32(elem);
    let name = m.u32(rec + 0x14);
    o.call(m, SI_SET_TEXED, &[dialog, 4, name]);
    let mut text = m.u32(rec + 0x10);
    if text == 0 {
        text = S_EMPTY;
    }
    o.call(m, SI_SET_TEXED, &[dialog, 0x1e, text]);
    for item in [6, 7, 8] {
        set_state(m, o, dialog, item, 0);
    }
    match m.i32(wave + 8) {
        1 => set_state(m, o, dialog, 7, 1),
        0 => set_state(m, o, dialog, 6, 1),
        2 => set_state(m, o, dialog, 8, 1),
        _ => {}
    }
    let on = (m.i32(wave + 0x30) == 1) as u32;
    set_state(m, o, dialog, 0x11, on);
    let on = (m.u32(wave + 0x28) != 0) as u32;
    set_state(m, o, dialog, 0x12, on);
    for item in [0x13, 0x1b, 0x1c] {
        set_state(m, o, dialog, item, 0);
    }
    match m.i32(wave + 0xc) {
        1 => set_state(m, o, dialog, 0x1b, 1),
        0 => set_state(m, o, dialog, 0x13, 1),
        2 => set_state(m, o, dialog, 0x1c, 1),
        _ => {}
    }
    let t = fmt4(m.f32(wave + 0x18));
    set_text(m, o, dialog, 0x14, buf, &t);
    let t = fmt4(m.f32(wave + 0x1c));
    set_text(m, o, dialog, 0x15, buf, &t);
    let t = m.i32(wave + 0x10).to_string().into_bytes();
    set_text(m, o, dialog, 0x16, buf, &t);
    let t = m.i32(wave + 0x14).to_string().into_bytes();
    set_text(m, o, dialog, 0x17, buf, &t);
    let t = fmt4(m.f32(wave + 0x20));
    set_text(m, o, dialog, 0x18, buf, &t);
    let t = fmt4(m.f32(wave + 0x24));
    set_text(m, o, dialog, 0x19, buf, &t);
    m.release(0x28);
}

fn text_float(m: &mut Mem, o: &mut dyn Outside, dialog: u32, item: u32) -> f32 {
    let st = o.call(m, SI_GET_TEXED_FLOAT, &[dialog, item]).st0.expect("SI_getTexedFloat leaves its value in st0");
    F80::from_bits(u64::from_le_bytes(st[0..8].try_into().unwrap()), u16::from_le_bytes([st[8], st[9]])).to_f32()
}

/// Reads the wave dialog into a wave [soft 0x084ca8a8 waveGetDg]: the old text (+4) is freed and field 4 read into it; the
/// type (+8) is 0 when toggle 6 is on, else 1 when toggle 7 is, else 2; +0x30 is 1 when toggle 0x11 is on, else 0; +0x28 is
/// 1 when toggle 0x12 is on, else 0; +0xc is 0 when toggle 0x13 is on, else 1 when 0x1b is, else 2; the floats +0x18, +0x1c,
/// +0x20, +0x24 come from the fields 0x14, 0x15, 0x18, 0x19 (rounded to f32), the integers +0x10, +0x14 from 0x16, 0x17.
pub fn wave_get_dg(m: &mut Mem, o: &mut dyn Outside, wave: u32, dialog: u32) {
    let st = m.local(4);
    let old = m.u32(wave + 4);
    m.free(old);
    o.call(m, SI_GET_TEXED, &[dialog, 4, wave + 4]);
    let mut on = |m: &mut Mem, o: &mut dyn Outside, item: u32| {
        o.call(m, SI_GET_STATE_ITEM, &[dialog, item, st]);
        m.i16(st) == 1
    };
    let t = if on(m, o, 6) {
        0
    } else if on(m, o, 7) {
        1
    } else {
        2
    };
    m.set_u32(wave + 8, t);
    let v = on(m, o, 0x11) as u32;
    m.set_u32(wave + 0x30, v);
    let v = on(m, o, 0x12) as u32;
    m.set_u32(wave + 0x28, v);
    let t = if on(m, o, 0x13) {
        0
    } else if on(m, o, 0x1b) {
        1
    } else {
        2
    };
    m.set_u32(wave + 0xc, t);
    let v = text_float(m, o, dialog, 0x14);
    m.set_f32(wave + 0x18, v);
    let v = text_float(m, o, dialog, 0x15);
    m.set_f32(wave + 0x1c, v);
    let v = o.call(m, SI_GET_TEXED_LONG, &[dialog, 0x16]).eax;
    m.set_u32(wave + 0x10, v);
    let v = o.call(m, SI_GET_TEXED_LONG, &[dialog, 0x17]).eax;
    m.set_u32(wave + 0x14, v);
    let v = text_float(m, o, dialog, 0x18);
    m.set_f32(wave + 0x20, v);
    let v = text_float(m, o, dialog, 0x19);
    m.set_f32(wave + 0x24, v);
    m.release(4);
}
