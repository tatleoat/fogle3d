// SPDX-License-Identifier: MIT
//! The layer's initialised tables, as they are in the binary. The ranges of the original's
//! data (key-pad pairs, start-up colours, default cursor, built-in font) are not in this
//! source: each is named by where the Linux `soft` holds it and read from the user's own
//! Softimage|3D 4.0 install when first used (`si-rt`, release model; see
//! `docs/design/runtime-data.md`), checked against our own checksum. The remaining tables are
//! our own constants (GLX attribute numbers, the keysym order from `evt_initKeymap`'s code).
//! `re/tools/si-gl/gen_data.py` writes this file from the binary; do not edit by hand.

use si_rt::Table;
use std::sync::LazyLock;

/// The attribute list handed to `glXChooseVisual` [soft 0x08674578]: GLX_DOUBLEBUFFER,
/// GLX_RED_SIZE 1, GLX_GREEN_SIZE 1, GLX_BLUE_SIZE 1, GLX_DEPTH_SIZE 1, GLX_STENCIL_SIZE 1,
/// GLX_RGBA, None.
pub const VISUAL_ATTRIBUTES: [i32; 13] = [
    5, 8, 1, 9, 1, 10, 1, 12, 1, 13, 1, 4, 0,
];

/// Thirteen (device, replacement) pairs of `evt_initKeymap` [soft 0x08674038]: a key-pad
/// device for the second keycode that maps to a main-block device. The code that would
/// apply them never runs (see `event.rs`). The pairs (u16, u16, 52 bytes) are read from the
/// install, see [`keypad_fixups`].
pub static KEYPAD_FIXUPS: Table = Table::new("si_gl.keypad_fixups", 0x0867_4038, 52, 0xa4c0_4da9_1fe1_b810);

/// The thirteen pairs of [`KEYPAD_FIXUPS`] (read once).
pub fn keypad_fixups() -> &'static [(u16, u16); 13] {
    static PAIRS: LazyLock<[(u16, u16); 13]> = LazyLock::new(|| {
        let b = KEYPAD_FIXUPS.get();
        std::array::from_fn(|i| (u16::from_le_bytes([b[4 * i], b[4 * i + 1]]), u16::from_le_bytes([b[4 * i + 2], b[4 * i + 3]])))
    });
    &PAIRS
}

/// The sixteen colours colour-map entries 0..15 start with, three bytes each (red, green,
/// blue) [soft 0x086ffa00 gfx_vgaColours] (48 bytes, read from the install).
pub static VGA_COLOURS: Table = Table::new("si_gl.gfx_vgaColours", 0x086f_fa00, 48, 0x3f4d_a9e1_e83f_cf12);

/// The default cursor's picture, 16 rows of two bytes, X bitmap order
/// [soft 0x086fee40 g_defaultCursorImage] (32 bytes, read from the install).
pub static DEFAULT_CURSOR_IMAGE: Table = Table::new("si_gl.g_defaultCursorImage", 0x086f_ee40, 32, 0x3092_e875_9d00_24e3);

/// The default cursor's mask [soft 0x086fee60 g_defaultCursorMask] (32 bytes, read from the
/// install).
pub static DEFAULT_CURSOR_MASK: Table = Table::new("si_gl.g_defaultCursorMask", 0x086f_ee60, 32, 0xcdc7_8eb3_3717_de0a);

/// Height of the built-in font [soft 0x086fef00 gl_fontHeight].
pub const FONT_HEIGHT: i32 = 15;
/// Number of characters of the built-in font [soft 0x086fef04 gl_fontNoChar].
pub const FONT_NCHAR: i32 = 128;
/// Number of raster words of the built-in font [soft 0x086ff320 gl_fontNoRaster].
pub const FONT_NRASTER: i32 = 852;

/// The built-in font's character table [soft 0x086fef20 gl_fontDef]: per character the
/// eight bytes of an IRIS GL `Fontchar` (offset into the raster as a little-endian short,
/// width, height, x offset, y offset, advance as a little-endian short). 1,024 bytes, read
/// from the install.
pub static FONT_DEF: Table = Table::new("si_gl.gl_fontDef", 0x086f_ef20, 1024, 0x0a06_8c2c_ad91_2626);

/// The built-in font's raster [soft 0x086ff340 gl_fontRaster]: 852 16-bit words (1,704 bytes),
/// read from the install.
pub static FONT_RASTER_TABLE: Table = Table::new("si_gl.gl_fontRaster", 0x086f_f340, 1704, 0xf99f_943a_b789_ad34);

/// The raster's words of [`FONT_RASTER_TABLE`] (read once).
pub fn font_raster() -> &'static [u16] {
    static WORDS: LazyLock<Vec<u16>> = LazyLock::new(|| FONT_RASTER_TABLE.get().chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect());
    &WORDS
}

/// Keysym to device number, as `evt_initKeymap`'s compare tree decides it
/// [soft 0x0860cbd0 evt_initKeymap]; sorted by keysym; every other keysym gives device 0.
pub const KEYSYM_DEVICE: [(u32, u16); 124] = [
    (0x0020, 0x53), (0x0027, 0x32), (0x002c, 0x2d), (0x002d, 0x2f), (0x002e, 0x34), (0x002f, 0x35),
    (0x0030, 0x2e), (0x0031, 0x08), (0x0032, 0x0e), (0x0033, 0x0f), (0x0034, 0x16), (0x0035, 0x17),
    (0x0036, 0x1e), (0x0037, 0x1f), (0x0038, 0x26), (0x0039, 0x27), (0x003b, 0x2b), (0x003d, 0x36),
    (0x005b, 0x31), (0x005c, 0x39), (0x005d, 0x38), (0x0060, 0x37), (0x0061, 0x0b), (0x0062, 0x24),
    (0x0063, 0x1c), (0x0064, 0x12), (0x0065, 0x11), (0x0066, 0x13), (0x0067, 0x1a), (0x0068, 0x1b),
    (0x0069, 0x28), (0x006a, 0x22), (0x006b, 0x23), (0x006c, 0x2a), (0x006d, 0x2c), (0x006e, 0x25),
    (0x006f, 0x29), (0x0070, 0x30), (0x0071, 0x0a), (0x0072, 0x18), (0x0073, 0x0c), (0x0074, 0x19),
    (0x0075, 0x21), (0x0076, 0x1d), (0x0077, 0x10), (0x0078, 0x15), (0x0079, 0x20), (0x007a, 0x14),
    (0xff08, 0x3d), (0xff09, 0x09), (0xff0d, 0x33), (0xff13, 0xa0), (0xff14, 0x9f), (0xff1b, 0x07),
    (0xff20, 0x91), (0xff50, 0xa2), (0xff51, 0x49), (0xff52, 0x51), (0xff53, 0x50), (0xff54, 0x4a),
    (0xff55, 0xa3), (0xff56, 0xa5), (0xff57, 0xa4), (0xff61, 0x9e), (0xff62, 0x91), (0xff63, 0xa1),
    (0xff69, 0x9f), (0xff7f, 0xa6), (0xff8d, 0x52), (0xff91, 0x48), (0xff92, 0x47), (0xff93, 0x4f),
    (0xff94, 0x4e), (0xff95, 0x43), (0xff96, 0x3f), (0xff97, 0x44), (0xff98, 0x46), (0xff99, 0x40),
    (0xff9a, 0x4b), (0xff9b, 0x41), (0xff9c, 0x3a), (0xff9d, 0x45), (0xff9e, 0x3b), (0xff9f, 0x42),
    (0xffaa, 0xa8), (0xffab, 0xa9), (0xffac, 0x4d), (0xffad, 0x4c), (0xffae, 0x42), (0xffaf, 0xa7),
    (0xffb0, 0x3b), (0xffb1, 0x3a), (0xffb2, 0x40), (0xffb3, 0x41), (0xffb4, 0x3f), (0xffb5, 0x45),
    (0xffb6, 0x46), (0xffb7, 0x43), (0xffb8, 0x44), (0xffb9, 0x4b), (0xffbe, 0x92), (0xffbf, 0x93),
    (0xffc0, 0x94), (0xffc1, 0x95), (0xffc2, 0x96), (0xffc3, 0x97), (0xffc4, 0x98), (0xffc5, 0x99),
    (0xffc6, 0x9a), (0xffc7, 0x9b), (0xffc8, 0x9c), (0xffc9, 0x9d), (0xffca, 0x9e), (0xffcb, 0x9f),
    (0xffcc, 0xa0), (0xffe1, 0x06), (0xffe2, 0x05), (0xffe3, 0x03), (0xffe4, 0x91), (0xffe5, 0x04),
    (0xffe6, 0x04), (0xffe9, 0x8f), (0xffea, 0x90), (0xffff, 0x3e),
];
