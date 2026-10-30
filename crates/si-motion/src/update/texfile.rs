// SPDX-License-Identifier: MIT
//! The picture of an animated 2D texture (`motionUpdate.c`): a texture whose pictures come
//! from a script (a text file with one picture name a line) or from a numbered sequence
//! gets the name for the frame, and the update is told whether the name changed.

use super::UpdateHost;
use si_mem::{Addr, Mem};

/// `__SI_LclMap` as these routines use it (**new**: all four).
pub mod tex {
    /// The name of the picture in use (a C string the texture owns).
    pub const PICTURE: u32 = 0x10;
    /// The texture's file: the script's path, or the sequence's picture name with its
    /// number as the last extension (a C string the texture owns).
    pub const FILE: u32 = 0x14;
    /// 16 bits: the script is in use (without it the place-holder picture is taken).
    pub const SCRIPT_ON: u32 = 0x1c8;
}

/// `SI_resdir` (0x08781628): a pointer to the path of the program's resource directory.
pub const SI_RESDIR: Addr = 0x0878_1628;
/// The line buffer of the two routines: 4,096 bytes on the stack.
const LINE: u32 = 0x1000;

/// `strdup`: a new block from the C library's `malloc` holding the text and its terminator.
fn strdup(m: &mut Mem, text: Addr) -> Addr {
    m.strdup(text)
}

fn put(m: &mut Mem, at: Addr, text: &[u8]) {
    m.set_bytes(at, text);
    m.set_u8(at + text.len() as u32, 0);
}

/// Gives a texture the name of its picture [soft 0x08229b28 saveMapImage]: 1 when the
/// texture already has that name (or either pointer is null), else the old name is freed, a
/// copy of the new one stored (+0x10), and 0 returned: "not up to date".
pub fn save_map_image(m: &mut Mem, texture: Addr, name: Addr) -> i16 {
    if texture == 0 || name == 0 {
        return 1;
    }
    let old = m.ptr(texture + tex::PICTURE);
    if old != 0 {
        if m.cstr(name) == m.cstr(old) {
            return 1;
        }
        m.free(old);
        m.set_ptr(texture + tex::PICTURE, 0);
    }
    let copy = strdup(m, name);
    m.set_ptr(texture + tex::PICTURE, copy);
    0
}

/// The place-holder of a script that yields no picture: `<SI_resdir>/noIcon`.
fn no_icon(m: &mut Mem, buf: Addr) {
    let mut text = m.cstr(m.ptr(SI_RESDIR));
    text.extend_from_slice(b"/noIcon");
    put(m, buf, &text);
}

/// The picture of a scripted texture at a frame [soft 0x08229ba0 isScriptUpToDate]; the
/// result is that of [`save_map_image`], or 1 for a null texture.
///
/// The picture name is **line `frame`** of the script file (+0x14, opened for reading; lines
/// of up to 4,094 characters), through `SI_stripPicExtension`, whose result (a pointer into
/// the line: blanks and the picture extension taken off) is the name; when that gives null
/// the texture counts as up to date. The place-holder `<SI_resdir>/noIcon` is taken instead
/// when the frame is below 1, the script is off (+0x1c8), the file does not open, or it has
/// fewer lines than the frame.
pub fn is_script_up_to_date<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, texture: Addr, frame: i32) -> i32 {
    if texture == 0 {
        return 1;
    }
    let buf = m.local(LINE);
    let r = script_name(m, h, texture, frame, buf);
    m.release(LINE);
    r
}

fn script_name<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, texture: Addr, frame: i32, buf: Addr) -> i32 {
    if frame <= 0 || m.i16(texture + tex::SCRIPT_ON) == 0 {
        no_icon(m, buf);
        return save_map_image(m, texture, buf) as i32;
    }
    let path = m.ptr(texture + tex::FILE);
    let file = h.fopen(m, path, MODE_R);
    if file == 0 {
        no_icon(m, buf);
        return save_map_image(m, texture, buf) as i32;
    }
    let mut i = 0;
    while i < frame {
        if h.fgets(m, buf, 0xfff, file) == 0 {
            h.fclose(m, file);
            no_icon(m, buf);
            return save_map_image(m, texture, buf) as i32;
        }
        i += 1;
    }
    h.fclose(m, file);
    m.set_u8(buf + 0xfff, 0);
    let name = h.strip_pic_extension(m, buf);
    if name == 0 {
        return 1;
    }
    save_map_image(m, texture, name) as i32
}

/// The text "r" of `isScriptUpToDate`, at its address in the binary's data.
pub const MODE_R: Addr = 0x0869_7846;

/// The picture of a sequence texture at a frame [soft 0x08229ccc isSequenceUpToDate]: the
/// texture's file name (+0x14) with its **last extension replaced by the frame number**
/// (`name.12.pic` → `name.12.<frame>`; a name without a dot gets `.<frame>` appended), then
/// through `SI_stripPicExtension`. Unless that answers null, the old name is freed and a
/// copy of the whole buffer (not of the routine's result) becomes the file name; the result
/// is then 0, "not up to date", whether or not the name changed. 1 for a null texture, a
/// frame below 1, a texture without a file name, or a null answer.
pub fn is_sequence_up_to_date<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, texture: Addr, frame: i32) -> i32 {
    if texture == 0 || frame < 1 {
        return 1;
    }
    let name = m.ptr(texture + tex::FILE);
    if name == 0 {
        return 1;
    }
    let mut text = m.cstr(name);
    if let Some(dot) = text.iter().rposition(|b| *b == b'.') {
        text.truncate(dot);
    }
    text.extend_from_slice(format!(".{frame}").as_bytes());
    let buf = m.local(LINE);
    // the original's buffer has no bound either; here a longer name is cut to the buffer
    text.truncate(LINE as usize - 1);
    put(m, buf, &text);
    let r = if h.strip_pic_extension(m, buf) == 0 {
        1
    } else {
        let old = m.ptr(texture + tex::FILE);
        if old != 0 {
            m.free(old);
            m.set_ptr(texture + tex::FILE, 0);
        }
        let copy = strdup(m, buf);
        m.set_ptr(texture + tex::FILE, copy);
        0
    };
    m.release(LINE);
    r
}
