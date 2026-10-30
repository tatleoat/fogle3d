// SPDX-License-Identifier: MIT
//! The spreadsheet and query modules of the program on the shared memory image (finding 0043).
//!
//! Records are addressed by named offsets in the image (their layouts are read from what
//! the routines do with them, not from a type table):
//!
//! - a **query** (`SI_SprdQuery`, 0x1c bytes, always inside a group at +0x0c): the parsed
//!   expression (+0x00), its relation data (+0x04), the text (+0x08, `strdup`), a 16-bit
//!   word (+0x0c), the clause count (+0x10) and the clause array (+0x14, pointers to 0x24
//!   byte clauses, made by `SI_getClauseFromQuery`);
//! - a **group setup** (`__SI_SprdSetupGroup`, 0x50 bytes): the name (+0x00), a query (+0x0c,
//!   see [`g`]), the sort list (+0x28: count, items of 0x14 bytes, 16-bit directions), the field
//!   list (+0x34, the same) and the sub groups (+0x40 count as 16 bits, +0x44 array);
//! - a **setup** (`SI_SprdSetup`, 0x78 bytes): the name (+0x00), the main group (+0x08) and the
//!   element list (+0x58: count, array of 0x18 byte elements, a 16-bit word).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Routines of other source files are reached through
//! [`si_mem::Outside`] under the names and addresses in [`hook`].

pub mod fct;
pub mod io;
pub mod spreadsheet;

use si_mem::{Mem, Routine};

/// Offsets inside a group setup (`__SI_SprdSetupGroup`).
pub mod g {
    pub const NAME: u32 = 0x00;
    pub const QUERY: u32 = 0x0c;
    pub const SORT: u32 = 0x28;
    pub const FIELD: u32 = 0x34;
    pub const NSUB: u32 = 0x40;
    pub const SUB: u32 = 0x44;
    pub const SIZE: u32 = 0x50;
}

/// Offsets of a sort or a field list (inside a group) and the size of an item.
pub mod list {
    pub const COUNT: u32 = 0;
    pub const ITEMS: u32 = 4;
    pub const DIRS: u32 = 8;
    pub const ITEM: u32 = 0x14;
}

/// Globals of the modules (addresses in the binary).
pub mod glob {
    pub const SI_QUERY_MODE: u32 = 0x0869_9ec0;
    pub const SI_FRAME_START: u32 = 0x0878_2050;
    pub const SI_FRAME_END: u32 = 0x0878_2044;
    pub const SORT_LIST: u32 = 0x086f_6ce0;
    pub const SORT_START: u32 = 0x086f_6ce4;
    pub const SORT_END: u32 = 0x086f_6ce8;
    pub const STDERR: u32 = 0x0878_0c88;
    pub const SI_EVD: u32 = 0x0878_1454;
    pub const SI_ATD: u32 = 0x0878_1438;
}

/// The texts of `sprd_fct.c` in `.rodata`: address and bytes. A check (or a run on a bare
/// image) writes them with [`seed_text`]; the routines read them from the image.
pub const TEXTS: &[(u32, &[u8])] = &[
    (0x086f_6cec, b"this.chptyp == ALL"),
    (0x086f_6cff, b"Main_Query"),
    (0x086f_6d0a, b"Main_Query"),
    (0x086f_6d15, b"="),
    (0x086f_6d17, b"query"),
    (0x086f_6d1d, b"chptyp"),
    (0x086f_6d24, b"ALL"),
    (0x086f_6d28, b"TRUE"),
    (0x086f_6d2d, b"FALSE"),
    (0x086f_6d33, b"\"%s\""),
    (0x086f_6d38, b"%f"),
    (0x086f_6d3b, b"%f"),
    (0x086f_6d3e, b"%f"),
    (0x086f_6d41, b"%i"),
    (0x086f_6d44, b"%i"),
    (0x086f_6d47, b"\"%c\""),
    (0x086f_6d4c, b"UNKOWN"),
    (0x086f_6d53, b"  "),
    (0x086f_6d56, b"( "),
    (0x086f_6d59, b"this."),
    (0x086f_6d5f, b") "),
    (0x086f_6d62, b"= %s"),
    (0x086f_6d67, b"Spreadsheet Group Setup [%s]\n"),
    (0x086f_6d85, b"-------------------------\n"),
    (0x086f_6da0, b"Fields list:\n"),
    (0x086f_6dae, b"Elem"),
    (0x086f_6db3, b"Oper"),
    (0x086f_6dc0, b"%d) FIELD ATTRIB [%s] name[%s] fcv[%s]\n"),
    (0x086f_6de8, b"Sort list:\n"),
    (0x086f_6df4, b"Elem"),
    (0x086f_6df9, b"Oper"),
    (0x086f_6e00, b"%d) FIELD ATTRIB [%s] name[%s] fcv[%s]\n"),
    (0x086f_6e28, b" SpreadSheet Setup :\n"),
    (0x086f_6e40, b"====================================\n"),
    (0x086f_6e80, b"====================================\n"),
    // spreadsheet.c
    (0x086c_a345, b"W"),
    (0x086c_a347, b"W"),
    (0x086c_a349, b""),
    (0x086c_a34a, b"%ld"),
    (0x086c_a34e, b""),
    (0x086c_a34f, b"%lf"),
    (0x086c_a353, b""),
];

/// Writes [`TEXTS`] to the image (the loader's job in a whole run).
pub fn seed_text(m: &mut Mem) {
    for (a, t) in TEXTS {
        m.put_cstr(*a, t);
    }
}

/// Routines of other source files that this crate calls and does not reproduce (the
/// expression parser and its data, the attribute and enum dictionaries, the C library's
/// `fprintf`).
pub mod hook {
    use super::Routine;
    pub const GET_CLAUSE_FROM_QUERY: Routine = Routine::new("SI_getClauseFromQuery", 0x0824_f210);
    pub const CHECK_ALLOC_STR: Routine = Routine::new("SI_checkAllocStr", 0x0824_c830);
    pub const GEN_CUS_EXPR_PARSING_TREE: Routine = Routine::new("SI_genCusExprParsingTree", 0x0824_b9ac);
    pub const EXPR_INIT_REL_DATA: Routine = Routine::new("si_exprInitRelData", 0x0824_b2f8);
    pub const EXPRESS_DATA_DISPOSE: Routine = Routine::new("SI_expressDataDispose", 0x0824_e5b0);
    pub const EXPR_REL_DATA_DISPOSE: Routine = Routine::new("SI_exprRelDataDispose", 0x0824_9c74);
    pub const ED_ELEMENT_ATT_COMP_VALUE: Routine = Routine::new("SI_edElementAttCompValue", 0x080f_02a0);
    pub const GET_EXPR_OPER_STR: Routine = Routine::new("SI_getExprOperStr", 0x0824_ed34);
    pub const ENMDICT_ENUM_CODE_GET_BY_VALUE: Routine = Routine::new("SI_enmdictEnumCodeGetByValue", 0x0813_8834);
    pub const ATTDICT_ATTRIBUTE_GET_BY_INDEX: Routine = Routine::new("SI_attdictAttributeGetByIndex", 0x0811_ef0c);
    pub const VALID_CUSTOM_EXPRESS: Routine = Routine::new("SI_validCustomExpress", 0x0824_b7c8);
    pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
}
