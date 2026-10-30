// SPDX-License-Identifier: MIT
//! `actor.c` on the shared memory image: the Actor module's mode switch and the state it
//! keeps (finding 0023, "Actor").
//!
//! The module is a mode machine. `actmode` (a global, 32 bits stored, 16 read) says what the
//! next mouse click does; `actorSetMode` turns the previous mode's button off and the new
//! one's group of buttons on. Everything else the file does is interface: the status line
//! texts of `SI_actorStatus` (0x081c9364), the event loops `SI_actorClient` (0x081c66d0)
//! and `actorWind` (0x081c712c), the pop-up menu `actorMenu` (0x081c79d4). **Not ported**:
//! those four (a long table of status texts; event loops; menu builders and their dialogs).
//! They are reached through [`Outside`] where a ported function calls them.
//!
//! Which button belongs to which mode is `actorGetButton`'s table; which of the menu's
//! items are ticked is refreshed from the module's flags by `refreshActor`.

use si_mem::Mem;

use crate::actout as ao;
use crate::actout::{call, call_eax};
use crate::outside::{si_status_display_message, Outside};

const SI_GRAF_LISTS_ADDR: u32 = 0x0867_9498;
const SI_PROC_DATA_ADDR: u32 = 0x0867_9314;
/// `actmode` (0x086921c0): the current mode, a 32-bit word of which 16 bits are read.
pub const ACTMODE: u32 = 0x0869_21c0;
/// `SI_fastIntenv`, `SI_shwAss`, `SI_busyCursor`, `SI_hrchCpst`, `SI_rightHand`,
/// `SI_zoomONCursor`, `SI_shwDircns`, `SI_shwCZone`: 16-bit flags the menu shows.
pub const SI_FAST_INTENV: u32 = 0x0867_953c;
pub const SI_SHW_ASS: u32 = 0x0867_950e;
pub const SI_BUSY_CURSOR: u32 = 0x0867_92b2;
pub const SI_HRCH_CPST: u32 = 0x0867_949c;
pub const SI_RIGHT_HAND: u32 = 0x0867_93ec;
pub const SI_ZOOM_ON_CURSOR: u32 = 0x0867_945c;
pub const SI_SHW_DIRCNS: u32 = 0x0867_9518;
pub const SI_SHW_CZONE: u32 = 0x0867_9530;
/// Not named in the binary: the 32-bit word `SI_skeletonFlagSet` keeps for the joints 8 and
/// 9 (0x087814b4), and the 16-bit words 0x0878157e, 0x08781580, 0x08781582, 0x08781584
/// that `refreshActor` shows on the preferences menu.
pub const SKELETON_FLAG: u32 = 0x0878_14b4;
pub const PREF_WORDS: [u32; 4] = [0x0878_157e, 0x0878_1580, 0x0878_1582, 0x0878_1584];
/// `SI_glbManip`'s first word (classical scaling).
pub const SI_GLB_MANIP: u32 = 0x0878_1560;
/// `SI_lightLicense`: not zero when the light licence is the one in force.
pub const SI_LIGHT_LICENSE: u32 = 0x0867_9320;
/// A 16-bit word (0x08781594, name unknown) that `actorButton` tests for a swift key.
pub const SWIFT_KEY_SWITCH: u32 = 0x0878_1594;

/// The mode numbers `actorSetMode` is called with by the seven `SI_actSet…Mode` entries.
pub const MODE_CUT_PASTE_KEY: i32 = 0x3e8;
pub const MODE_EDIT_KEY: i32 = 0x3e9;
pub const MODE_SLOPE_MANAGEMENT: i32 = 0x3ea;
pub const MODE_INTERP_MANAGEMENT: i32 = 0x3eb;
pub const MODE_SET_KEY: i32 = 0x3ec;
pub const MODE_EDIT_FCV: i32 = 0x3ed;
pub const MODE_EDIT_TAG: i32 = 0x3ee;

/// The button a mode belongs to and whether setting the mode drops the super mode
/// [soft 0x081c6f34 actorGetButton]. The button is −1 for a mode without one; the flag is
/// zero for modes 0, 9, 10 and 31 and one for every other value, negative ones included.
/// A mode outside 3..=47 has no button (the jump table covers 45 values from 3 on; the
/// index is the low 16 bits of `mode − 3` compared as unsigned).
pub fn actor_get_button(mode: i16) -> (i16, i16) {
    let button = match (mode.wrapping_sub(3)) as u16 as u32 {
        0..=2 => 4,
        3 => 0x208,
        4 => 0x1838,
        6 | 7 | 28 => 0x1806,
        8..=12 | 16..=18 | 39 | 42..=44 => 0x1900,
        13 | 14 | 33..=36 | 41 => 0x189c,
        15 | 37 | 38 => 0x2d3,
        19 => 0xe6,
        24..=26 => 0x186a,
        _ => -1,
    };
    let flag = if mode <= 10 { !(mode >= 9 || mode == 0) } else { mode != 0x1f } as i16;
    (button, flag)
}

/// The current mode, as a 16-bit value [soft 0x081c6fd0 SI_actorGetMode].
pub fn si_actor_get_mode(m: &Mem) -> i32 {
    m.i16(ACTMODE) as i32
}

/// Switches to a mode: the old mode's button is turned off unless the new mode shares it,
/// the super mode is dropped for the modes that call for it, the schematic's open/close
/// state is cleared, and the status line is redrawn [soft 0x081c6fe0 actorSetMode].
pub fn actor_set_mode(m: &mut Mem, o: &mut dyn Outside, mode: i32, arg: i16) {
    let (old, _) = actor_get_button(m.i16(ACTMODE));
    let (new, flag) = actor_get_button(mode as i16);
    if flag != 0 {
        call(o, m, &ao::SI_SUPER_MODE_OFF, &[arg as i32 as u32]);
    }
    if old != new && old > 0 {
        call(o, m, &ao::SI_BUTTON_OFF, &[old as i32 as u32]);
    }
    call(o, m, &ao::SI_SCHEM_TURN_OFF_OPN_CLS, &[]);
    m.set_i32(ACTMODE, mode);
    call(o, m, &ao::SI_ACTOR_STATUS, &[]);
}

/// [soft 0x081c7068 SI_actSetEditKeyMode]
pub fn si_act_set_edit_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_EDIT_KEY, 1);
}

/// [soft 0x081c7084 SI_actSetCutPasteKeyMode]
pub fn si_act_set_cut_paste_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_CUT_PASTE_KEY, 1);
}

/// [soft 0x081c70a0 SI_actSetSlopeManagementMode]
pub fn si_act_set_slope_management_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_SLOPE_MANAGEMENT, 1);
}

/// [soft 0x081c70bc SI_actSetSetKeyMode]
pub fn si_act_set_set_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_SET_KEY, 1);
}

/// [soft 0x081c70d8 SI_actSetEditFcvMode]
pub fn si_act_set_edit_fcv_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_EDIT_FCV, 1);
}

/// [soft 0x081c70f4 SI_actSetEditTagMode]
pub fn si_act_set_edit_tag_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_EDIT_TAG, 1);
}

/// [soft 0x081c7110 SI_actSetInterpManagementMode]
pub fn si_act_set_interp_management_mode(m: &mut Mem, o: &mut dyn Outside) {
    actor_set_mode(m, o, MODE_INTERP_MANAGEMENT, 1);
}

fn menu_state_set(m: &mut Mem, o: &mut dyn Outside, a: i32, b: i32, c: i32, d: i32, e: i32, state: i32) {
    call(o, m, &ao::SI_MENU_STATE_SET, &[a as u32, b as u32, c as u32, d as u32, e as u32, state as u32]);
}

/// The skeleton menu's flags: joint 10 is the constraint-zone flag (`SI_shwCZone`), joints
/// 8 and 9 the two states of one word (1: the first is on, 2: the second); other numbers
/// are ignored [soft 0x081c9ddc SI_skeletonFlagSet].
pub fn si_skeleton_flag_set(m: &mut Mem, o: &mut dyn Outside, which: i32, value: i32) {
    if which > 7 {
        if which < 10 {
            m.set_i32(SKELETON_FLAG, value);
            menu_state_set(m, o, 2, 0x189c, 0x189c, 9, 0, (value == 2) as i32);
            let w = m.i32(SKELETON_FLAG);
            menu_state_set(m, o, 2, 0x189c, 0x189c, 8, 0, (w == 1) as i32);
        } else if which == 10 {
            m.set_i16(SI_SHW_CZONE, value as i16);
            menu_state_set(m, o, 2, 0x189c, 0x189c, 10, 0, value);
        }
    }
}

/// [soft 0x081c9e78 SI_actorDirectionFlagSet]
pub fn si_actor_direction_flag_set(m: &mut Mem, o: &mut dyn Outside, value: i32) {
    m.set_i16(SI_SHW_DIRCNS, value as i16);
    menu_state_set(m, o, 2, 0x2d3, 0x2d3, 5, 0, value);
}

/// Shows the single/multiple selection buttons for the current graf list: the list's
/// first word says which is on [soft 0x081c6cfc actorMultiRefresh].
pub fn actor_multi_refresh(m: &mut Mem, o: &mut dyn Outside) {
    let lists = m.u32(SI_GRAF_LISTS_ADDR);
    let slot = m.i32(m.u32(SI_PROC_DATA_ADDR).wrapping_add(4));
    let multi = m.i16(lists.wrapping_add((slot as u32).wrapping_mul(0x24)));
    if multi == 0 {
        call(o, m, &ao::SI_BUTTON_OFF, &[0x14]);
        call(o, m, &ao::SI_BUTTON_KEEP_HILITE, &[10]);
    } else {
        call(o, m, &ao::SI_BUTTON_OFF, &[10]);
        call(o, m, &ao::SI_BUTTON_KEEP_HILITE, &[0x14]);
    }
}

/// Brings the buttons and the menus in line with the module's flags
/// [soft 0x081c6d48 refreshActor].
pub fn refresh_actor(m: &mut Mem, o: &mut dyn Outside) {
    actor_multi_refresh(m, o);
    call(o, m, &ao::SI_BUTTON_OFF, &[0x26ac]);
    call(o, m, &ao::SI_INIT_SHOW_FLAG, &[2]);
    let w = |m: &Mem, a: u32| m.i16(a) as i32;
    let v = w(m, SI_FAST_INTENV);
    menu_state_set(m, o, 2, 0x1900, 0x1900, 7, 0, v);
    let v = w(m, SI_SHW_ASS);
    menu_state_set(m, o, 2, 0x1900, 0x190a, 0, 0, v);
    let v = w(m, SI_BUSY_CURSOR);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 7, 0, v);
    let v = w(m, SI_HRCH_CPST);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 0, 0, v);
    let v = w(m, SI_RIGHT_HAND);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 6, 0, v);
    let v = w(m, SI_ZOOM_ON_CURSOR);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 8, 0, v);
    let v = w(m, PREF_WORDS[0]);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 1, 0, v);
    let v = w(m, PREF_WORDS[1]);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 2, 0, v);
    let v = w(m, PREF_WORDS[2]);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 0xb, 0, v);
    let v = (m.i32(SI_GLB_MANIP) == 1) as i32;
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 5, 0, v);
    let v = w(m, PREF_WORDS[3]);
    menu_state_set(m, o, 2, 0x1fe, 0xa0, 3, 0, v);
    let v = w(m, SI_SHW_CZONE);
    si_skeleton_flag_set(m, o, 10, v);
    let v = m.i32(SKELETON_FLAG);
    si_skeleton_flag_set(m, o, 9, v);
    let v = w(m, SI_SHW_DIRCNS);
    si_actor_direction_flag_set(m, o, v);
    call(o, m, &ao::SI_ACTOR_STATUS, &[]);
}

/// The menu items `initActor` switches on (the last two words of the call are 1, 1) when
/// the light licence is in force: (menu, menu again, item, ...) as the call has them.
const INIT_ITEMS: &[(i32, i32, &[i32])] = &[
    (0x1838, 0x1838, &[0, 1, 2]),
    (0x6e, 10, &[7]),
    (0x189c, 0x189c, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0xb, 0xc]),
    (0x17d4, 0x17d4, &[0, 1, 2, 3]),
    (0x1806, 0x1806, &[0, 1, 2, 3, 4, 5, 6, 7]),
    (0x1807, 0x1807, &[0, 1, 2]),
    (0x186a, 0x186a, &[0, 1, 2, 3, 4]),
    (0x1900, 0x1900, &[0, 1, 2, 3, 4, 5, 6, 7]),
];

/// The module's start-up: the refresh, mode 0, and under the light licence the 45 menu
/// items switched on [soft 0x081c67c0 initActor].
pub fn init_actor(m: &mut Mem, o: &mut dyn Outside) {
    refresh_actor(m, o);
    m.set_i32(ACTMODE, 0);
    call(o, m, &ao::SI_ACTOR_STATUS, &[]);
    if m.i16(SI_LIGHT_LICENSE) != 0 {
        for (menu, again, items) in INIT_ITEMS {
            for item in items.iter() {
                menu_state_set(m, o, 2, *menu, *again, *item, 1, 1);
            }
        }
    }
}

/// A button event of the Actor window: `button` is the event's word at +4 and `value` the
/// word at +6 (read as 16 bits each) [soft 0x081c925c actorButton]. `event` is the event's
/// address, handed on to `SI_controlOff`.
///
/// The swift-key text is formatted in a 256-byte buffer of the original's frame; a longer
/// description than 224 bytes overruns it there.
pub fn actor_button(m: &mut Mem, o: &mut dyn Outside, event: u32) {
    let button = m.i16(event.wrapping_add(4));
    match button {
        10 => call(o, m, &ao::SI_SET_SINGLE_SELECTION, &[0x081c_6cfc]),
        1..=3 => {
            if m.i16(event.wrapping_add(6)) == 4 {
                call(o, m, &ao::SI_NUMERIC_SRT_HANDLE, &[button as i32 as u32]);
            }
        }
        0x208 => actor_set_mode(m, o, 6, 1),
        0x212 => {
            call(o, m, &ao::SI_CUT_SELECTED, &[]);
            call(o, m, &ao::SI_CONTROL_OFF, &[event, 1]);
        }
        0x14 => call(o, m, &ao::SI_SET_MULTI_SELECTION, &[0x081c_6cfc]),
        _ => {
            let key = call_eax(o, m, &ao::SI_GET_CURRENT_SWIFT_KEY, &[]) as i32;
            if key != -1 {
                if m.i16(SWIFT_KEY_SWITCH) == 0 {
                    let desc = call_eax(o, m, &ao::SI_GET_QUICK_DESCRIPTION, &[key as u32]);
                    let text = m.cstr(desc);
                    let mut line = b"Swift key ".to_vec();
                    line.extend_from_slice(&text);
                    line.extend_from_slice(b" not valid in Actor");
                    let buf = m.local(0x100);
                    m.put_cstr(buf, &line);
                    si_status_display_message(o, m, buf, 2);
                    m.release(0x100);
                } else {
                    call(o, m, &ao::SI_CONTROL_OFF, &[event, 1]);
                    call(o, m, &ao::SI_CHANGE_CURRENT_SWIFT_KEY_MODULE, &[]);
                }
            }
        }
    }
}

