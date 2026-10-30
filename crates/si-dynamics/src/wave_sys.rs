// SPDX-License-Identifier: MIT
//! `wave_sys.c` of Softimage|3D 4.0 (0x084c6ef0 … 0x084c7980): the list of waves and their links to models (finding
//! 0045). Only the functions that need no file input or output are here; the callees of other files are hooks on
//! [`ParticleCalls`].
//!
//! A wave record (our names): +4 the name, +8..+0x28 eleven words copied by a duplicate, +0x2c the sample count with the
//! samples (pairs of floats) at +0x48, +0x30..+0x34 two words, +0x38..+0x44 four floats (the icon's extent), +0x68 a
//! block that is freed, +0x6c the first attached model, +0x74 the icon model and +0x78 the next wave in the global list.
//! The status record `si_waveStatus` is at 0x08822120; the head of the list is its word at +0x7c. A model's wave list
//! (+0x2e8) is made of nodes (wave, ?, next), the next at +8. A graph node holds the model at +4.

use crate::outside::DynaCalls;
use crate::particle_outside::ParticleCalls;
use crate::wave_init::{si_init_wave, si_wave_allocate, si_wave_init_horiz_prof};
use crate::wave_fcrv::{si_wave_init_decay, si_wave_init_spread};
use crate::wave_util::{fscanf, si_wave_detach_mdl, si_wave_print_wave, si_wave_read_wave, si_wave_resize_icon};
use si_mem::clib::strcmp;
use si_mem::{Mem, Outside, Routine};
use si_x87::x;

const WAVE_STATUS: u32 = 0x0882_2120;
const WAVE_LIST_HEAD: u32 = WAVE_STATUS + 0x7c;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The model (icon) of a wave [soft 0x084c6ef0 waveGetModel]: the word at +0x74.
pub fn wave_get_model(m: &Mem, wave: u32) -> u32 {
    m.u32(wave + 0x74)
}

/// The icon model of a wave [soft 0x084c6f00 SI_waveGetIconModel]: the word at +0x74.
pub fn si_wave_get_icon_model(m: &Mem, wave: u32) -> u32 {
    m.u32(wave + 0x74)
}

/// The first attached model of a wave, when asked from none [soft 0x084c6f10 SI_waveGetNextAttachedModel]: the word at
/// +0x6c if `prev` is null, else 0.
pub fn si_wave_get_next_attached_model(m: &Mem, wave: u32, prev: u32) -> u32 {
    if prev == 0 {
        m.u32(wave + 0x6c)
    } else {
        0
    }
}

/// The wave after `wave` in a model's wave list [soft 0x084c6f2c waveGetNextWave]: with `wave` null the first one; else
/// the one after the node that holds it, 0 when it is the last or is not in the list.
pub fn wave_get_next_wave(m: &Mem, model: u32, wave: u32) -> u32 {
    let mut node = m.u32(model + 0x2e8);
    if wave == 0 {
        return m.u32(node);
    }
    while node != 0 {
        if wave == m.u32(node) {
            node = m.u32(node + 8);
            if node != 0 {
                return m.u32(node);
            }
            break;
        }
        node = m.u32(node + 8);
    }
    0
}

/// The next wave of a graph node [soft 0x084c7434 grafnodeNextWave]: the word at +0x78, 0 for a null node.
pub fn grafnode_next_wave(m: &Mem, node: u32) -> u32 {
    if node == 0 {
        0
    } else {
        m.u32(node + 0x78)
    }
}

/// The next node of a model's wave list [soft 0x084c744c modelNextWave]: the word at +8, 0 for null.
pub fn model_next_wave(m: &Mem, node: u32) -> u32 {
    if node == 0 {
        0
    } else {
        m.u32(node + 8)
    }
}

/// The name of the wave of a list node [soft 0x084c7468 modelGetWaveName]: the wave's name pointer (+4 of the first
/// word), 0 for null.
pub fn model_get_wave_name(m: &Mem, node: u32) -> u32 {
    if node == 0 {
        0
    } else {
        m.u32(m.u32(node) + 4)
    }
}

/// The name of a wave [soft 0x084c7488 grafnodeGetWaveName]: the word at +4, 0 for null.
pub fn grafnode_get_wave_name(m: &Mem, wave: u32) -> u32 {
    if wave == 0 {
        0
    } else {
        m.u32(wave + 4)
    }
}

/// Whether a wave has a model attached [soft 0x084c74a0 waveIsAttach]: 1 when it is not null and its +0x6c is not 0.
pub fn wave_is_attach(m: &Mem, wave: u32) -> u32 {
    (wave != 0 && m.u32(wave + 0x6c) != 0) as u32
}

/// The wave of that name [soft 0x084c7730 SI_getWaveByName]: a walk of the global list (`strcmp` on the names), 0 if none.
pub fn si_get_wave_by_name(m: &Mem, name: u32) -> u32 {
    let mut w = m.u32(WAVE_LIST_HEAD);
    while w != 0 {
        if strcmp(m, name, m.u32(w + 4)) == 0 {
            return w;
        }
        w = m.u32(w + 0x78);
    }
    0
}

/// Always 1 [soft 0x084c776c waveIsDirty].
pub fn wave_is_dirty() -> u32 {
    1
}

/// Attaches the model of that name to the wave of that name [soft 0x084c74c0 SI_waveAttachByName]: nothing when there
/// is no such wave; else the current graph list is walked and the first node of kind 2 or 3 whose model's name (+4)
/// matches `model_name` gets the wave through `SI_waveAddMdl(wave, model, kind, 1)`.
pub fn si_wave_attach_by_name(m: &mut Mem, o: &mut dyn Outside, wave_name: u32, model_name: u32, kind: i16) {
    let wave = si_get_wave_by_name(m, wave_name);
    if wave == 0 {
        return;
    }
    let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
    let slot = m.local(4);
    m.set_u32(slot, 0);
    o.si_next_grafnode(m, list, slot);
    loop {
        let node = m.u32(slot);
        if node == 0 {
            break;
        }
        if m.u32(node).wrapping_sub(2) < 2 {
            let model = m.u32(node + 4);
            if strcmp(m, m.u32(model + 4), model_name) == 0 {
                o.si_wave_add_mdl(m, wave, model, kind as i32, 1);
                break;
            }
        }
        o.si_next_grafnode(m, list, slot);
    }
    m.release(4);
}

/// Disposes a wave through a pointer slot [soft 0x084c6f68 SI_waveDispose]: nothing for a null slot content. The
/// animation element goes, the attached model (if any) is detached, the icon model is disposed, the blocks at +4, +0x48
/// and +0x68 are freed, the wave leaves the global list, is freed and the slot is cleared.
pub fn si_wave_dispose(m: &mut Mem, o: &mut dyn Outside, slot: u32) {
    let wave = m.u32(slot);
    if wave == 0 {
        return;
    }
    o.si_ed_dispose_wave(m, wave);
    if m.u32(wave + 0x6c) != 0 {
        let a = m.u32(wave + 0x6c);
        si_wave_detach_mdl(m, o, wave, a, 1);
    }
    o.si_model_dispose_node(m, wave + 0x74);
    for off in [4u32, 0x48, 0x68] {
        let p = m.u32(wave + off);
        if p != 0 {
            m.free(p);
        }
    }
    let mut prev = WAVE_STATUS + 4;
    let mut cur = m.u32(WAVE_LIST_HEAD);
    while cur != wave {
        prev = m.u32(prev + 0x78);
        cur = m.u32(prev + 0x78);
    }
    let next = m.u32(wave + 0x78);
    m.set_u32(prev + 0x78, next);
    m.free(wave);
    m.set_u32(slot, 0);
}

/// Detaches every wave from a model [soft 0x084c7018 SI_modelUnlinkAllWaves]: while the model has a wave list, its first
/// wave is detached (`SI_waveDetachMdl(wave, model, 1)`); a first wave whose attached model (+0x6c) is not this model
/// stops it with the message "That's VERY strange." (level 0).
pub fn si_model_unlink_all_waves(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    loop {
        let node = m.u32(model + 0x2e8);
        if node == 0 {
            return;
        }
        let wave = m.u32(node);
        if m.u32(wave + 0x6c) != model {
            break;
        }
        si_wave_detach_mdl(m, o, wave, model, 1);
    }
    o.si_status_display_message(m, 0x086d_9700, 0);
}

/// Copies a wave [soft 0x084c7778 SI_waveDuplicate]: 0 (message "Unable to duplicate item." at level 3) when a new
/// record cannot be made. The sample count and the pairs (a new block of count * 8 bytes when the count is positive),
/// the eleven words at +8..+0x28, +0x30, +0x34 and the extent floats +0x38..+0x44 are copied; the decay, spread, profile
/// and icon are rebuilt and the icon is resized by (w, h, w) with `w = x1 - x0` and `h = y1 - y0` (80-bit difference,
/// rounded to f32 for each).
pub fn si_wave_duplicate(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    let new = si_wave_allocate(m, o, WAVE_STATUS);
    if new == 0 {
        o.si_status_display_message(m, 0x086d_97c1, 3);
        return 0;
    }
    let n = m.i32(src + 0x2c);
    m.set_i32(new + 0x2c, n);
    if 0 < n {
        let p = m.malloc((n << 3) as u32);
        m.set_u32(new + 0x48, p);
        let n = m.i32(new + 0x2c);
        let mut i = 0i32;
        while i < n * 2 {
            let v = m.u32(m.u32(src + 0x48) + 4 * i as u32);
            m.set_u32(m.u32(new + 0x48) + 4 * i as u32, v);
            i += 1;
        }
    }
    for off in [0x38u32, 0x3c, 0x40, 0x44, 0x30, 0x34, 8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28] {
        let v = m.u32(src + off);
        m.set_u32(new + off, v);
    }
    si_wave_init_decay(m, o, new);
    si_wave_init_spread(m, o, new);
    si_wave_init_horiz_prof(m, new);
    o.si_wave_create_icon_mdl(m, new);
    let v = m.local(12);
    let w = (x(m.f32(new + 0x3c)) - x(m.f32(new + 0x38))).to_f32();
    let h = (x(m.f32(new + 0x44)) - x(m.f32(new + 0x40))).to_f32();
    m.set_f32(v, w);
    m.set_f32(v + 4, h);
    m.set_f32(v + 8, w);
    si_wave_resize_icon(m, new, v);
    m.release(12);
    new
}

const FOPEN: Routine = Routine::new("fopen", 0x080c_4e18);
const FCLOSE: Routine = Routine::new("fclose", 0x080c_4958);
const FFLUSH: Routine = Routine::new("fflush", 0x080c_3be8);
const FERROR: Routine = Routine::new("ferror", 0x080c_3878);
const STDERR: u32 = 0x0878_0c88;
const SI_PIPED_FILES: u32 = 0x086c_eba8;
const SI_PIPE: u32 = 0x086c_ebac;
const SI_SAVE_ELEM_ERROR_CODE: u32 = 0x086c_ebb0;

/// `<name>.wav` in a fresh block of length + 5 bytes (the `malloc` + `sprintf` of the file writers and readers).
fn wav_name(m: &mut Mem, name: u32) -> u32 {
    let mut t = m.cstr(name);
    let s = m.malloc(t.len() as u32 + 5);
    t.extend_from_slice(b".wav\0");
    m.set_bytes(s, &t);
    s
}

/// The stream of a wave file: the shared pipe when files are piped, else `fopen(path, mode)` (0 when it fails).
fn open_stream(m: &mut Mem, o: &mut dyn Outside, path: u32, mode: u32) -> u32 {
    let pipe = m.u32(SI_PIPE);
    if m.u16(SI_PIPED_FILES) == 0 {
        o.call(m, FOPEN, &[path, mode]).eax
    } else {
        pipe
    }
}

fn close_stream(m: &mut Mem, o: &mut dyn Outside, stream: u32) {
    if m.u16(SI_PIPED_FILES) == 0 {
        o.call(m, FCLOSE, &[stream]);
    }
}

/// Writes every wave of the list to `<name>.wav` [soft 0x084c7068 owaveBinaryWrite] (the file is text in fact): each wave is
/// printed with `si_wavePrintWave` and followed by a blank line. A file that cannot be opened gives "owaveBinaryWrite:
/// cannot open <name>.wav." on stderr and 0; an empty list "owaveBinaryWrite: No waves to write!" on stderr (the file is
/// still closed); 1 otherwise.
pub fn owave_binary_write(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let path = wav_name(m, name);
    let stream = open_stream(m, o, path, 0x086d_971c);
    let err = m.u32(STDERR);
    if stream == 0 {
        let mut t = b"owaveBinaryWrite: cannot open ".to_vec();
        t.extend_from_slice(&m.cstr(path));
        t.extend_from_slice(b".\n");
        o.print(err, &t);
        m.free(path);
        return 0;
    }
    let mut w = m.u32(WAVE_LIST_HEAD);
    if w == 0 {
        o.print(err, b"owaveBinaryWrite: No waves to write!\n");
    } else {
        while w != 0 {
            si_wave_print_wave(m, o, stream, w);
            o.print(stream, b"\n\n");
            w = m.u32(w + 0x78);
        }
    }
    close_stream(m, o, stream);
    m.free(path);
    1
}

/// Reads the waves of `<name>.wav` [soft 0x084c7194 SI_owaveBinaryRead]: the wave globals are set up first when needed. A
/// keyword `Wave` (read with `%s`) starts a wave: it is allocated, read (`si_waveReadWave`; a failure closes the file and
/// gives 0, the path block is not freed), given its profile and icon (no icon: "Could not create icon for wave.\n" is
/// written to the file, 0), the icon gets the scale (+0x100), rotation (+0x10c) and position (+0x158) read, the rotation
/// matrix (+0x118), a matrix update, a resize by (w, h, w) of the extent, and the wave is added to the current graph list
/// (kind 0xb) after `SI_backSelected(0, 1)`. Returns the last wave read; the original returns an uninitialised local when
/// the file holds no wave (the port gives 0), and 0 when the file cannot be opened.
pub fn si_owave_binary_read(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    if m.u32(WAVE_STATUS + 0x8c) == 0 {
        si_init_wave(m);
    }
    let path = wav_name(m, name);
    let stream = open_stream(m, o, path, 0x086d_9790);
    if stream == 0 {
        m.free(path);
        return 0;
    }
    let loc = m.local(0x5c);
    let (trans, rot, scale, resize, word) = (loc, loc + 0xc, loc + 0x18, loc + 0x24, loc + 0x30);
    let mut last = 0u32;
    while fscanf(m, o, stream, 0x086d_9792, &[word]) == 1 {
        if m.cstr(word) != b"Wave" {
            continue;
        }
        last = si_wave_allocate(m, o, WAVE_STATUS);
        if si_wave_read_wave(m, o, stream, last, scale, rot, trans) as i16 == 0 {
            close_stream(m, o, stream);
            m.release(0x5c);
            return 0;
        }
        si_wave_init_horiz_prof(m, last);
        let icon = o.si_wave_create_icon_mdl(m, last);
        if icon == 0 {
            o.print(stream, b"Could not create icon for wave.\n");
            close_stream(m, o, stream);
            m.free(path);
            m.release(0x5c);
            return 0;
        }
        for k in 0..3u32 {
            let (r, s, t) = (m.u32(rot + 4 * k), m.u32(scale + 4 * k), m.u32(trans + 4 * k));
            m.set_u32(icon + 0x10c + 4 * k, r);
            m.set_u32(icon + 0x100 + 4 * k, s);
            m.set_u32(icon + 0x158 + 4 * k, t);
        }
        o.si_matrix_xyz_rotation(m, rot, icon + 0x118);
        o.si_model_update_matrix(m, icon);
        let w = (x(m.f32(last + 0x3c)) - x(m.f32(last + 0x38))).to_f32();
        let h = (x(m.f32(last + 0x44)) - x(m.f32(last + 0x40))).to_f32();
        m.set_f32(resize, w);
        m.set_f32(resize + 4, h);
        m.set_f32(resize + 8, w);
        si_wave_resize_icon(m, last, resize);
        o.si_back_selected(m, 0, 1);
        let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
        o.si_graflist_add_data(m, list, last, 0xb, 1);
    }
    close_stream(m, o, stream);
    m.free(path);
    m.release(0x5c);
    last
}

/// Writes one wave as text to a file [soft 0x084c78a8 SI_dbwaveAsciiWrite]: no wave sets the error code (0x086cebb0) to 4 and
/// gives false; a file that cannot be opened 1. The wave is printed, a newline follows, and a real file (not piped) is
/// flushed, checked (`ferror`: error code 2, false) and closed.
pub fn si_dbwave_ascii_write(m: &mut Mem, o: &mut dyn Outside, name: u32, wave: u32) -> bool {
    if wave == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 4);
        return false;
    }
    let stream = open_stream(m, o, name, 0x086d_97db);
    if stream == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 1);
        return false;
    }
    si_wave_print_wave(m, o, stream, wave);
    o.print(stream, b"\n");
    let mut ok = true;
    if m.u16(SI_PIPED_FILES) == 0 {
        o.call(m, FFLUSH, &[stream]);
        ok = o.call(m, FERROR, &[stream]).eax == 0;
        if !ok {
            m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 2);
        }
        close_stream(m, o, stream);
    }
    ok
}

/// Reads the waves of a text file [soft 0x084c7980 SI_dbwaveAsciiRead]: as `SI_owaveBinaryRead` with the file name as given
/// (no `.wav`; mode "r"; the stream is the shared pipe when files are piped), the same keyword `Wave`, the same set-up
/// of the icon and the same insertion into the current graph list. A wave that cannot be read closes the file and gives
/// 0; no icon writes "Could not create icon for wave.\n" to the file, closes it and gives 0. Returns the last wave read; the
/// original returns an uninitialised local when the file holds no wave (the port gives 0), and 0 when the file cannot
/// be opened.
pub fn si_dbwave_ascii_read(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    if m.u32(WAVE_STATUS + 0x8c) == 0 {
        si_init_wave(m);
    }
    let stream = open_stream(m, o, name, 0x086d_97df);
    if stream == 0 {
        return 0;
    }
    let loc = m.local(0x5c);
    let (trans, rot, scale, resize, word) = (loc, loc + 0xc, loc + 0x18, loc + 0x24, loc + 0x30);
    let mut last = 0u32;
    while fscanf(m, o, stream, 0x086d_97e1, &[word]) == 1 {
        if m.cstr(word) != b"Wave" {
            continue;
        }
        last = si_wave_allocate(m, o, WAVE_STATUS);
        if si_wave_read_wave(m, o, stream, last, scale, rot, trans) as i16 == 0 {
            close_stream(m, o, stream);
            m.release(0x5c);
            return 0;
        }
        si_wave_init_horiz_prof(m, last);
        let icon = o.si_wave_create_icon_mdl(m, last);
        if icon == 0 {
            o.print(stream, b"Could not create icon for wave.\n");
            close_stream(m, o, stream);
            m.release(0x5c);
            return 0;
        }
        for k in 0..3u32 {
            let (r, s, t) = (m.u32(rot + 4 * k), m.u32(scale + 4 * k), m.u32(trans + 4 * k));
            m.set_u32(icon + 0x10c + 4 * k, r);
            m.set_u32(icon + 0x100 + 4 * k, s);
            m.set_u32(icon + 0x158 + 4 * k, t);
        }
        o.si_matrix_xyz_rotation(m, rot, icon + 0x118);
        o.si_model_update_matrix(m, icon);
        let w = (x(m.f32(last + 0x3c)) - x(m.f32(last + 0x38))).to_f32();
        let h = (x(m.f32(last + 0x44)) - x(m.f32(last + 0x40))).to_f32();
        m.set_f32(resize, w);
        m.set_f32(resize + 4, h);
        m.set_f32(resize + 8, w);
        si_wave_resize_icon(m, last, resize);
        o.si_back_selected(m, 0, 1);
        let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
        o.si_graflist_add_data(m, list, last, 0xb, 1);
    }
    close_stream(m, o, stream);
    m.release(0x5c);
    last
}
