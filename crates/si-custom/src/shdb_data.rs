// SPDX-License-Identifier: MIT
//! Constant data of the shader-database sources: seven ranges of the original program's
//! initialised data (strings, flags, defaults, the parser's and the scanner's tables). None of
//! it is in this source: each range is named by its address and length in the Linux `soft` and
//! read when it is first stored, from the user's own Softimage|3D 4.0 install (`si-rt`; release
//! model: nothing of the original ships), checked against our own checksum of it
//! (`docs/design/runtime-data.md`). Unit rtdata1 held all seven, byte for byte, against the
//! literals this file carried before.

use si_mem::Mem;
use si_rt::Table;

/// initial .data of the shader-database sources (strings, flags, defaults): 0x086e4c80..0x086e5dc0.
pub const DATA_INIT_ADDR: u32 = 0x086e4c80;
pub static DATA_INIT: Table = Table::new("si_custom.shdb_init", DATA_INIT_ADDR, 0x1140, 0xb887_8084_57f5_0a6d);
/// initial .data of the custom-effect sources (custom_*.c, udf*, dyn*; strings, tables, globals): 0x086e2000..0x086e4c80.
pub const CUSTOM_INIT_ADDR: u32 = 0x086e2000;
pub static CUSTOM_INIT: Table = Table::new("si_custom.custom_init", CUSTOM_INIT_ADDR, 0x2c80, 0x242a_1abc_d705_ebb9);
/// initial .data of the dynamic-link sources (dlkCore, dslist, dynlink, dynsym: messages, the main handle): 0x086fc700..0x086fcc00.
pub const DYN_INIT_ADDR: u32 = 0x086fc700;
pub static DYN_INIT: Table = Table::new("si_custom.dyn_init", DYN_INIT_ADDR, 0x500, 0xa7d7_a433_ec88_c0b4);
/// initial .data in front of CUSTOM_INIT (custom_init.c, custom_io.c, ...: strings, tables, pointers): 0x086df000..0x086e2000.
pub const CUSTOM_INIT0_ADDR: u32 = 0x086df000;
pub static CUSTOM_INIT0: Table = Table::new("si_custom.custom_init0", CUSTOM_INIT0_ADDR, 0x3000, 0xb96d_35cb_0944_5c53);
/// float constants of shdb_init and shdb_hooks (.rodata): 0x086636d0..0x08663720.
pub const FLOATS_ADDR: u32 = 0x086636d0;
pub static FLOATS: Table = Table::new("si_custom.shdb_floats", FLOATS_ADDR, 0x50, 0xe760_1341_82db_28ec);
/// the parser's tables, yytranslate .. yycheck (.rodata): 0x08663940..0x08664540.
pub const YACC_ADDR: u32 = 0x08663940;
pub static YACC: Table = Table::new("si_custom.shdb_yacc", YACC_ADDR, 0xc00, 0xaf0f_6d05_f9bd_2b64);
/// the scanner's transition table (.rodata): 0x08664600..0x08670a9c.
pub const YY_TRANSITION_ADDR: u32 = 0x08664600;
pub static YY_TRANSITION: Table = Table::new("si_custom.shdb_yy_transition", YY_TRANSITION_ADDR, 0xc49c, 0xffd0_5c1c_4fec_16fc);

/// The seven ranges, in the order [`store`] writes them.
pub static ALL: [&Table; 7] = [&CUSTOM_INIT, &DATA_INIT, &DYN_INIT, &CUSTOM_INIT0, &FLOATS, &YACC, &YY_TRANSITION];

/// Puts the data into the image: `.data` writable, the rest read-only. Needs the user's install
/// (panics with the reason without one; a program calls `si_rt::check` first).
pub fn store(m: &mut Mem) {
    for t in &ALL[..4] {
        m.set_bytes(t.addr, t.get());
    }
    for t in &ALL[4..] {
        m.set_rom(t.addr, t.get());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With an install: every range is in the program, its checksum is right, and stored in an
    /// image it reads back the same.
    #[test]
    fn ranges_load_and_store() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        store(&mut m);
        for t in ALL {
            let b = t.get();
            assert_eq!(b.len(), t.len as usize, "{}", t.name);
            assert_eq!(si_rt::fnv1a64(b), t.sum, "{}", t.name);
            assert_eq!(m.u8(t.addr), b[0], "{}", t.name);
            assert_eq!(m.u8(t.end() - 1), b[b.len() - 1], "{}", t.name);
        }
        assert_eq!(DATA_INIT.end(), 0x086e5dc0);
        assert_eq!(YY_TRANSITION.end(), 0x08670a9c);
    }
}
