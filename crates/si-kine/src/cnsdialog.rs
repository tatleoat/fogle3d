// SPDX-License-Identifier: MIT
//! The helpers of the two limit set-up dialogs that are not dialog code
//! (`constraint.c`), on the shared memory image (finding 0024). The dialogs themselves
//! (`SI_dialogPoslimSetup` 0x081f6460, `SI_dialogRotlimSetup` 0x081f7ba0) are not
//! ported: they are event loops of the interface layer and stay hooks.
//!
//! A dialog's items are reached through `dialog.c`'s accessors, outside this package.

use si_mem::{Mem, Ptr32};
use si_types::geom::Vector;
use si_types::model::Model;
use si_x87::{x, F80};

use crate::globals::SI_TEMP;
use crate::outside as out;
use crate::outside::Outside;
use crate::rodata::{S_EMPTY_MESSAGE, S_MIN_MAX};
use crate::RotlimData;

fn set_item(o: &mut dyn Outside, m: &mut Mem, dialog: u32, item: i32, state: i32) {
    o.call(m, &out::SI_SET_STATE_ITEM, &[dialog, item as u32, state as u32]);
}

fn set_ghost(o: &mut dyn Outside, m: &mut Mem, dialog: u32, item: i32, ghost: i32) {
    o.call(m, &out::SI_SET_STATE_GHOST, &[dialog, item as u32, ghost as u32]);
}

/// The state of a dialog's toggle (`SI_getStateItem`, which stores 16 bits through its
/// third argument; the local it stores to holds 0 before, or the image's fill).
fn get_item(o: &mut dyn Outside, m: &mut Mem, dialog: u32, item: i32, at: u32) -> i16 {
    o.call(m, &out::SI_GET_STATE_ITEM, &[dialog, item as u32, at]);
    m.i16(at)
}

/// Sets a limit's toggle and ghosts its number field when the toggle is off
/// [soft 0x081f4c04 limitFreezeSet].
pub fn limit_freeze_set(m: &mut Mem, o: &mut dyn Outside, dialog: u32, toggle: i16, field: i16, state: i16) {
    set_item(o, m, dialog, toggle as i32, state as i32);
    set_ghost(o, m, dialog, field as i32, (state == 0) as i32);
}

/// Flips a limit's toggle and ghosts its number field when it is now off
/// [soft 0x081f4c5c limitFreezeToggle].
pub fn limit_freeze_toggle(m: &mut Mem, o: &mut dyn Outside, dialog: u32, toggle: i16, field: i16) {
    let at = m.local(4);
    let now = (get_item(o, m, dialog, toggle as i32, at) == 0) as i16;
    m.release(4);
    set_item(o, m, dialog, toggle as i32, now as i32);
    set_ghost(o, m, dialog, field as i32, (now == 0) as i32);
}

/// Ghosts or frees a limit's toggle; its number field is ghosted with it, and when the
/// toggle is freed the field stays ghosted while the toggle is off
/// [soft 0x081f4cdc limitGhostSet].
pub fn limit_ghost_set(m: &mut Mem, o: &mut dyn Outside, dialog: u32, toggle: i16, field: i16, ghost: i16) {
    set_ghost(o, m, dialog, toggle as i32, ghost as i32);
    if ghost != 0 {
        set_ghost(o, m, dialog, field as i32, 1);
    } else {
        let at = m.local(4);
        let off = get_item(o, m, dialog, toggle as i32, at) == 0;
        m.release(4);
        set_ghost(o, m, dialog, field as i32, off as i32);
    }
}

/// `sprintf(SI_temp_, "%1.6f", v)`: the C library's text of a double with six decimals in
/// the program's scratch buffer (30 bytes; a longer text overruns it, as in the
/// original). For a value that is not finite the text is the C library's (`inf`, `nan`,
/// with a minus sign for a set sign bit); the emulation harness cannot vouch for those.
fn print_degrees(m: &mut Mem, v: f64) {
    let text = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.6}")
    };
    m.put_cstr(SI_TEMP.addr(), text.as_bytes());
}

/// The dialog's "use the current rotation" buttons and its damping fields
/// [soft 0x081f7754 rotlimDefault]. `button` 26 makes the node's rotation the lower
/// bounds, 27 the upper bounds: the three bounds become active, the angles are copied as
/// they are, the degrees are 57.29577951308232 × each angle rounded to float, the three
/// number fields show them with six decimals and are freed, and the three toggles are set.
/// Whatever the button, the two damping values are read back from fields 21 and 23 and
/// brought into range.
pub fn rotlim_default(m: &mut Mem, o: &mut dyn Outside, button: i32, dialog: u32, data: u32, node: Ptr32<Model>) {
    // (active flags, radians, degrees, the number fields, the toggles)
    let side = match button {
        0x1a => Some((0x18u32, 0x24u32, 0x00u32, [6, 9, 0xc], [0xe, 0x10, 0x12])),
        0x1b => Some((0x1e, 0x30, 0x0c, [7, 10, 0xd], [0xf, 0x11, 0x13])),
        _ => None,
    };
    if let Some((active, radians, degrees, fields, toggles)) = side {
        for k in 0..3 {
            m.set_i16(data + active + 2 * k, 1);
        }
        let angles: Vector = m[node].vrot;
        m.write(Ptr32::<Vector>::new(data + radians), angles);
        let k = x(57.29577951308232f64);
        let dx = (k * m.f32(data + radians)).to_f32();
        let dy = (k * m.f32(data + radians + 4)).to_f32();
        m.set_f32(data + degrees + 4, dy);
        m.set_f32(data + degrees, dx);
        let dz = (k * m.f32(data + radians + 8)).to_f32();
        m.set_f32(data + degrees + 8, dz);
        for (i, field) in fields.into_iter().enumerate() {
            let shown = if i == 0 { dx } else { m.f32(data + degrees + 4 * i as u32) };
            print_degrees(m, shown as f64);
            o.call(m, &out::SI_SET_TEXED, &[dialog, field, SI_TEMP.addr()]);
        }
        for field in fields {
            set_ghost(o, m, dialog, field as i32, 0);
        }
        for toggle in toggles {
            set_item(o, m, dialog, toggle, 1);
        }
    }
    let width = o.call(m, &out::SI_GET_TEXED_FLOAT, &[dialog, 0x15]).st0.to_f32();
    m.set_f32(data + 0x3c, width);
    let strength = o.call(m, &out::SI_GET_TEXED_FLOAT, &[dialog, 0x17]).st0.to_f32();
    m.set_f32(data + 0x40, strength);
    damping_in_range(m, data);
}

/// `rotlimDampingInRange` (0x081f7540) on a record of the image.
// adapter: not in place. The first-pass port (`limits::rotlim_damping_in_range`) works on
// a Rust value: the record is copied out, brought into range and its two damping values
// copied back (the routine writes nothing else and allocates nothing).
fn damping_in_range(m: &mut Mem, data: u32) {
    let v = |m: &Mem, at: u32| Vector { x: m.f32(data + at), y: m.f32(data + at + 4), z: m.f32(data + at + 8) };
    let a = |m: &Mem, at: u32| [m.i16(data + at), m.i16(data + at + 2), m.i16(data + at + 4)];
    let mut d = RotlimData { min_deg: v(m, 0), max_deg: v(m, 0x0c), min_active: a(m, 0x18), max_active: a(m, 0x1e), min: v(m, 0x24), max: v(m, 0x30), damping: [m.f32(data + 0x3c), m.f32(data + 0x40)] };
    let before = d.damping;
    crate::limits::rotlim_damping_in_range(&mut d);
    for k in 0..2 {
        if d.damping[k].to_bits() != before[k].to_bits() {
            m.set_f32(data + 0x3c + 4 * k as u32, d.damping[k]);
        }
    }
}

/// Whether a lower bound lies above its upper bound in the dialog
/// [soft 0x081f7a08 rotlimWarning], with a message when that changed. For x, y and z in
/// turn (toggles 14/15, 16/17, 18/19; fields 6/7, 9/10, 12/13) both toggles and both
/// numbers are read; the lower number is rounded to float, the upper one is compared as
/// the accessor left it in the register. The first axis with both toggles on and upper
/// below lower answers 1. When the answer differs from `was`, the status line says
/// `Min > Max` (severity 2), or is cleared with an empty text (severity 0).
pub fn rotlim_warning(m: &mut Mem, o: &mut dyn Outside, dialog: u32, was: i16) -> i32 {
    let sp = m.sp();
    let (lower_on, upper_on) = (m.local(4), m.local(4));
    m.set_i16(lower_on, 0);
    m.set_i16(upper_on, 0);
    let mut bad = 0i16;
    for (toggles, fields) in [([0xe, 0xf], [6u32, 7u32]), ([0x10, 0x11], [9, 10]), ([0x12, 0x13], [0xc, 0xd])] {
        get_item(o, m, dialog, toggles[0], lower_on);
        get_item(o, m, dialog, toggles[1], upper_on);
        let lower = o.call(m, &out::SI_GET_TEXED_FLOAT, &[dialog, fields[0]]).st0.to_f32();
        let upper: F80 = o.call(m, &out::SI_GET_TEXED_FLOAT, &[dialog, fields[1]]).st0;
        if m.i16(lower_on) != 0 && m.i16(upper_on) != 0 && upper.lt(x(lower)) {
            bad = 1;
            break;
        }
    }
    m.set_sp(sp);
    if bad != was {
        if bad != 0 {
            out::si_status_display_message(o, m, S_MIN_MAX.0, 2);
        } else {
            out::si_status_display_message(o, m, S_EMPTY_MESSAGE.0, 0);
        }
    }
    bad as i32
}
