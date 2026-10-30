// SPDX-License-Identifier: MIT
//! The binary's initialised data that the renderer reads (finding 0012, section 19.3):
//! the globals of `.data` with their initial values, the tables, and the texts whose
//! addresses the routines hand on. Nothing here reproduces a function of the original.
//!
//! A loaded scene's image has what the loader stored and zeros elsewhere; a routine that
//! reads `ptrDefault`, `SI_defaultMaterial` or a format text finds nothing unless somebody
//! writes the binary's data segment there. [`init`] does: once, on an image that is to
//! render.
//!
//! Held against the binary by two scripts, neither of which executes anything:
//! `python3 -I re/tools/si-mem/check_globals.py si-render` (every entry of [`globals`]:
//! address, symbol, initial value, and that what is declared without one is zero in the
//! binary) and `python3 -I re/tools/si-render/check_data.py` ([`TABLES`] and [`TEXTS`]: addresses,
//! lengths and our checksums, against the binary; the
//! filter table's closed form, and that the list of initialised data the renderer's code
//! names is covered).
//!
//! How the list was made: `re/tools/si-render/data_refs.py` prints every address of `.data`
//! that the instruction dumps of the renderer's source files name, with its symbol and
//! whether its bytes are zero.

use si_mem::Mem;
use si_rt::Table;

/// The renderer's globals of `.data`, each with the value the binary gives it.
pub mod globals {
    use si_mem::FnPtr;
    si_mem::globals! {
        /// `SI_procData`: a pointer to the process record (in `.bss`); its first word is
        /// the current dictionary's number, its second the graf list's.
        SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
        /// `SI_rightHand`: 1, the coordinate system's handedness.
        SI_RIGHT_HAND: i16 = 0x086793ec, "SI_rightHand", init 1;
        /// `SI_picturePath`: null until start-up or the Render dialogue sets it.
        SI_PICTURE_PATH: u32 = 0x086793cc, "SI_picturePath", init 0;
        /// `SI_texturePath`: null until start-up sets it.
        SI_TEXTURE_PATH: u32 = 0x08679434, "SI_texturePath", init 0;
        /// `SI_reportFrequency`: a progress line every tenth line of the picture.
        SI_REPORT_FREQUENCY: i16 = 0x0869bc80, "SI_reportFrequency", init 10;
        /// `SI_renChunk`: the renderer's chunk memory, null until `SI_matterInit` runs
        /// `SI_chunkAllocate(200, 5000000)`.
        SI_REN_CHUNK: u32 = 0x0869bc84, "SI_renChunk", init 0;
        /// `pFile` of `rle.c`: the picture being written.
        P_FILE: u32 = 0x0869c440, "pFile", init 0;
        /// `version` of `rnd_tag.c`: the index into `versionName` a tag file's header takes.
        VERSION: i32 = 0x0869c9c0, "version", init 3;
        /// `versionName`: the floats 2.0, 2.1, 2.5, 2.6.
        VERSION_NAME_0: f32 = 0x0869c9c4, "versionName", init 0x40000000;
        VERSION_NAME_1: f32 = 0x0869c9c8, "", init 0x40066666;
        VERSION_NAME_2: f32 = 0x0869c9cc, "", init 0x40200000;
        VERSION_NAME_3: f32 = 0x0869c9d0, "", init 0x40266666;
        /// `mailboxCurrent` of `trace.c`: the ray counter starts at 1.
        MAILBOX_CURRENT: i32 = 0x0869c9f8, "mailboxCurrent", init 1;
        /// `cellVolumn` of `bsptree.c`: 0.01, the volume below which a cell is not split.
        CELL_VOLUMN: f32 = 0x0869ce20, "cellVolumn", init 0x3c23d70a;
        /// `ptrDefault`: a pointer to `SI_defaultMaterial`, handed on as an array of one
        /// material (`SI_propagate`).
        PTR_DEFAULT: u32 = 0x0869cf60, "ptrDefault", init 0x0869dde0;
        /// `SI_sboxfi`: the filled box of the GL layer, called through this pointer.
        SI_SBOXFI: FnPtr = 0x086ee2a4, "SI_sboxfi", init 0x0861236c;
        /// `SI_addInDictionary`: 1; 0 while a scene is being loaded. The dictionary's two
        /// picture routines ([`crate::edtools`]) do nothing while it is 0. Declared for the
        /// check; [`init`] does not write it (it is the loader's switch).
        SI_ADD_IN_DICTIONARY: i16 = 0x086792b0, "SI_addInDictionary", init 1;
        /// `first_call` of `tt_noise.c`: 1 until the noise table of the 3D textures is
        /// filled (`SI_ttnInitNoise`, [`crate::t3d_fct_image::init_noise`]).
        TT_NOISE_FIRST_CALL: i16 = 0x086b9a80, "first_call.225", init 1;
        /// `seed` of `drand48.c`: the generator's three words, least significant first.
        DRAND48_SEED_0: u16 = 0x086ba000, "seed", init 0x330e;
        DRAND48_SEED_1: u16 = 0x086ba002, "", init 0xabcd;
        DRAND48_SEED_2: u16 = 0x086ba004, "", init 0x1234;
        /// `stderr`: the C library's variable, which the dynamic linker copies into the
        /// program's `.bss`; `updDoWrap` hands it to `fprintf`. Not initial data of the
        /// binary: whoever stands for the C library stores its handle here.
        STDERR: u32 = 0x08780c88, "stderr@@GLIBC_2.0";

        // `SI_defaultMaterial` (0x0869dde0, 0xdcc bytes): the material a root without
        // materials renders with. Every word that is not zero, and the stretches between
        // them as bytes without an initial value (which the script holds to be zero).
        /// The name, `default`.
        DEFMAT_000: u32 = 0x0869dde0, "SI_defaultMaterial", init 0x61666564;
        DEFMAT_004: u32 = 0x0869dde4, "", init 0x00746c75;
        DEFMAT_Z008: [u8; 0x14] = 0x0869dde8, "";
        /// The 16-bit word at +0x1e: shading model 4.
        DEFMAT_01C: u32 = 0x0869ddfc, "", init 0x00040000;
        /// +0x20: 2.
        DEFMAT_020: u32 = 0x0869de00, "", init 0x00000002;
        /// +0x24: 1.0.
        DEFMAT_024: u32 = 0x0869de04, "", init 0x3f800000;
        DEFMAT_Z028: [u8; 0xc] = 0x0869de08, "";
        /// Ambient: 0.5 three times.
        DEFMAT_034: u32 = 0x0869de14, "", init 0x3f000000;
        DEFMAT_038: u32 = 0x0869de18, "", init 0x3f000000;
        DEFMAT_03C: u32 = 0x0869de1c, "", init 0x3f000000;
        DEFMAT_Z040: [u8; 0x4] = 0x0869de20, "";
        /// Diffuse: 0.7 three times.
        DEFMAT_044: u32 = 0x0869de24, "", init 0x3f333333;
        DEFMAT_048: u32 = 0x0869de28, "", init 0x3f333333;
        DEFMAT_04C: u32 = 0x0869de2c, "", init 0x3f333333;
        DEFMAT_Z050: [u8; 0x4] = 0x0869de30, "";
        /// Specular: 1.0 three times.
        DEFMAT_054: u32 = 0x0869de34, "", init 0x3f800000;
        DEFMAT_058: u32 = 0x0869de38, "", init 0x3f800000;
        DEFMAT_05C: u32 = 0x0869de3c, "", init 0x3f800000;
        DEFMAT_Z060: [u8; 0x4] = 0x0869de40, "";
        /// The specular exponent: 50.0.
        DEFMAT_064: u32 = 0x0869de44, "", init 0x42480000;
        DEFMAT_Z068: [u8; 0x660] = 0x0869de48, "";
        /// +0x6c8: 0.5.
        DEFMAT_6C8: u32 = 0x0869e4a8, "", init 0x3f000000;
        DEFMAT_Z6CC: [u8; 0x10] = 0x0869e4ac, "";
        /// +0x6dc: 1.0.
        DEFMAT_6DC: u32 = 0x0869e4bc, "", init 0x3f800000;
        DEFMAT_Z6E0: [u8; 0x68] = 0x0869e4c0, "";
        /// +0x748: 1.0.
        DEFMAT_748: u32 = 0x0869e528, "", init 0x3f800000;
        DEFMAT_Z74C: [u8; 0x10] = 0x0869e52c, "";
        /// +0x75c: 1.0.
        DEFMAT_75C: u32 = 0x0869e53c, "", init 0x3f800000;
        /// +0x760: 3.0.
        DEFMAT_760: u32 = 0x0869e540, "", init 0x40400000;
        DEFMAT_Z764: [u8; 0x640] = 0x0869e544, "";
        /// +0xda4: 1.
        DEFMAT_DA4: u32 = 0x0869eb84, "", init 0x00000001;
        /// +0xda8: the element number, -1.
        DEFMAT_DA8: u32 = 0x0869eb88, "", init 0xffffffff;
        /// +0xdac: 1.
        DEFMAT_DAC: u32 = 0x0869eb8c, "", init 0x00000001;
        DEFMAT_ZDB0: [u8; 0x18] = 0x0869eb90, "";
        /// The two vertex colour modes (+0xdc8, +0xdca): 1 each.
        DEFMAT_DC8: u32 = 0x0869eba8, "", init 0x00010001;
    }
}

/// `SI_defaultMaterial` (0x0869dde0): its initial value is the `DEFMAT_…` entries of
/// [`globals`]: the name `default`, shading model 4, ambient 0.5, diffuse 0.7, specular 1,
/// exponent 50.
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;
/// The record's size.
pub const SI_DEFAULT_MATERIAL_SIZE: u32 = 0xdcc;

/// `filters` (0x0869cf80, 0xb58 bytes): six blocks of 11 × 11 floats, one a sampling level;
/// [`crate::render::filter_for_level`] is its closed form, which [`init`] stores.
pub const FILTERS: u32 = 0x0869_cf80;

/// `poisson` (0x0869c680): forty doubles, [`POISSON_TABLE`], [`crate::shade::POISSON`].
pub const POISSON: u32 = 0x0869_c680;

/// `poisson`'s forty doubles, read from the user's program at run time (si-rt) and checked
/// against our FNV-1a of the range.
pub static POISSON_TABLE: Table = Table::new("si-render poisson", POISSON, 0x140, 0x2a96_0d28_d047_ca93);

/// `ctab` of sun_fct.c (eqTime) at 0x086a2ec0: fourteen pairs of a day of the year and a float, the
/// equation of time in minutes at that day; read at run time like [`POISSON_TABLE`].
pub static CTAB_TABLE: Table = Table::new("si-render ctab (sun_fct.c)", 0x086a_2ec0, 0x70, 0x31db_3345_9965_034d);

/// Initial values of `.data` that are not scalars: the ranges of the user's program [`init`] stores.
pub static TABLES: [&Table; 2] = [&POISSON_TABLE, &CTAB_TABLE];

/// Where the texts lie that the routines ported in place hand on by address or read as a
/// format.
pub mod text {
    // rnd_main.c: SI_image's dialogue after a failed licence (the ten other texts of the file
    // are `crate::rnd_main::text`'s)
    pub const DIALOG_OK: u32 = 0x0869_bcbe;
    pub const DIALOG_EMPTY: u32 = 0x0869_bcc1;
    pub const DIALOG_ABORTED: u32 = 0x0869_bcc2;
    pub const DIALOG_LICENSE_FAILED: u32 = 0x0869_bce0;
    // render.c: getRenderLicense (`crate::licence`)
    pub const BLANK_LOST: u32 = 0x0869_c03c;
    pub const LOST_CONNECTION: u32 = 0x0869_c040;
    pub const BLANK_REESTABLISHED: u32 = 0x0869_c063;
    pub const LICENSE_REESTABLISHED: u32 = 0x0869_c080;
    pub const BLANK_RESUMING: u32 = 0x0869_c0a2;
    pub const RESUMING_RENDERING: u32 = 0x0869_c0a4;
    pub const BLANK_FAILED: u32 = 0x0869_c0ba;
    pub const LICENSE_FAILED: u32 = 0x0869_c0c0;
    pub const BLANK_ABORTED: u32 = 0x0869_c0f4;
    pub const RENDERING_ABORTED: u32 = 0x0869_c0f6;
    // rle.c
    pub const FMT_PIC: u32 = 0x0869_c444;
    pub const FMT_PATH_PIC: u32 = 0x0869_c450;
    pub const FMT_PIC_1: u32 = 0x0869_c45f;
    pub const FMT_PATH_PIC_1: u32 = 0x0869_c46d;
    pub const FMT_PIC_2: u32 = 0x0869_c47e;
    pub const FMT_PATH_PIC_2: u32 = 0x0869_c48c;
    pub const NO_DIRECTORY: u32 = 0x0869_c4a0;
    pub const DIRECTORY_REFUSED: u32 = 0x0869_c4e0;
    pub const FMT_SEMA: u32 = 0x0869_c510;
    pub const MODE_WB_KEEP: u32 = 0x0869_c54c;
    pub const FILE_REFUSED: u32 = 0x0869_c560;
    pub const MODE_WB: u32 = 0x0869_c58c;
    pub const FMT_SUBREGION: u32 = 0x0869_c58f;
    pub const COMMENT: u32 = 0x0869_c5c0;
    pub const NO_OUTPUT: u32 = 0x0869_c5e3;
    pub const INCOMPLETE: u32 = 0x0869_c600;
    pub const MODE_RB_PLUS: u32 = 0x0869_c622;
    pub const FMT_STOP_AT_LINE: u32 = 0x0869_c640;
    // renutil.c: si_fatal
    pub const FMT_TWO: u32 = 0x0869_c1e4;
    pub const FMT_LINE: u32 = 0x0869_c1e9;
    pub const FMT_LINE_STATUS: u32 = 0x0869_c1ed;
    // renutil.c: SI_warning, SI_dtime, SI_segvHandler
    pub const FMT_TWO_WARNING: u32 = 0x0869_c1f1;
    pub const FMT_LINE_WARNING: u32 = 0x0869_c1f6;
    pub const FMT_DTIME: u32 = 0x0869_c2e0;
    pub const MALLOC_FAILED: u32 = 0x0869_c3e0;
    pub const BLANK_MALLOC: u32 = 0x0869_c405;
    pub const INTERNAL_ERROR: u32 = 0x0869_c407;
    pub const BLANK_INTERNAL: u32 = 0x0869_c420;
    // t2d_fct.c: SI_txtApplyTransform
    pub const OLD_UV_CONVERSION: u32 = 0x086a_4d4a;
    // matter.c: SI_updateSubregionValues
    pub const RESOLUTION_USED: u32 = 0x086a_1500;
    // materials.c: si_initJitter
    pub const JITTER_BLANK: u32 = 0x0869_be5f;
    pub const JITTER_WARNING: u32 = 0x0869_be80;
    // edtools.c: SI_elemdictAddPicture
    pub const SUFFIX_PIC_FILE: u32 = 0x0868_0bcc;
    // t2d_dial.c: updDoWrap
    pub const UNKNOWN_SURFACE: u32 = 0x086a_4660;
    pub const T2D_DIAL_PATH: u32 = 0x086a_46a0;
    pub const UNEXPECTED_DEFAULT: u32 = 0x086a_4720;
}

/// The texts, at the addresses the binary has them; the bytes are the user's program's own, read
/// at run time ([`place_texts`]): 65 texts, 1380 bytes with their zeros in this order, FNV-1a
/// 0x7f04_295d_b16a_ae4f (the test below holds that). The first ten of `rnd_main.c` are
/// [`crate::rnd_main::text`]'s.
pub const TEXTS: &[u32] = &[
    // rnd_main.c
    0x0869bca8,
    0x0869bd14,
    0x0869bd20,
    0x0869bd3f,
    0x0869bd43,
    0x0869bd79,
    0x0869bd81,
    0x0869bd8e,
    0x0869bd90,
    0x0869bda5,
    text::DIALOG_OK,
    text::DIALOG_EMPTY,
    text::DIALOG_ABORTED,
    text::DIALOG_LICENSE_FAILED,
    // render.c
    text::BLANK_LOST,
    text::LOST_CONNECTION,
    text::BLANK_REESTABLISHED,
    text::LICENSE_REESTABLISHED,
    text::BLANK_RESUMING,
    text::RESUMING_RENDERING,
    text::BLANK_FAILED,
    text::LICENSE_FAILED,
    text::BLANK_ABORTED,
    text::RENDERING_ABORTED,
    // rle.c
    text::FMT_PIC,
    text::FMT_PATH_PIC,
    text::FMT_PIC_1,
    text::FMT_PATH_PIC_1,
    text::FMT_PIC_2,
    text::FMT_PATH_PIC_2,
    text::NO_DIRECTORY,
    text::DIRECTORY_REFUSED,
    text::FMT_SEMA,
    text::MODE_WB_KEEP,
    text::FILE_REFUSED,
    text::MODE_WB,
    text::FMT_SUBREGION,
    text::COMMENT,
    text::NO_OUTPUT,
    text::INCOMPLETE,
    text::MODE_RB_PLUS,
    text::FMT_STOP_AT_LINE,
    // renutil.c
    text::FMT_TWO,
    text::FMT_LINE,
    text::FMT_LINE_STATUS,
    text::FMT_TWO_WARNING,
    text::FMT_LINE_WARNING,
    0x0869c200,
    0x0869c240,
    0x0869c271,
    0x0869c283,
    0x0869c2a0,
    text::FMT_DTIME,
    text::MALLOC_FAILED,
    text::BLANK_MALLOC,
    text::INTERNAL_ERROR,
    text::BLANK_INTERNAL,
    // t2d_fct.c
    text::OLD_UV_CONVERSION,
    // matter.c
    text::RESOLUTION_USED,
    // materials.c
    text::JITTER_BLANK,
    text::JITTER_WARNING,
    // edtools.c
    text::SUFFIX_PIC_FILE,
    // t2d_dial.c
    text::UNKNOWN_SURFACE,
    text::T2D_DIAL_PATH,
    text::UNEXPECTED_DEFAULT,
];

/// Writes the binary's data segment as far as the renderer reads it: the scalars and
/// `SI_defaultMaterial` of [`globals`], [`TABLES`], the filter table and [`TEXTS`].
///
/// Three of the scalars are pointers that a running program has long replaced by the time
/// it renders (`SI_procData` aside, which never changes): `SI_picturePath`,
/// `SI_texturePath` and `SI_renChunk` are **not** written when they already hold
/// something, so that this can be called on an image a loader or a caller has prepared;
/// `SI_addInDictionary` is never written (0 is a value the loader sets on purpose).
pub fn init(m: &mut Mem) {
    let set: [u32; 4] = [0x0867_93cc, 0x0867_9434, 0x0869_bc84, 0x0867_9314];
    for g in globals::ALL {
        if (set.contains(&g.addr) && m.u32(g.addr) != 0) || g.addr == 0x0867_92b0 {
            continue;
        }
        if let Some(v) = g.init {
            m.set_bytes(g.addr, &v.to_le_bytes()[..(g.size as usize).min(8)]);
        }
    }
    for t in TABLES {
        m.set_bytes(t.addr, t.get());
    }
    for level in 0..6i16 {
        let block = crate::render::filter_for_level(level);
        for (i, row) in block.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                m.set_f32(FILTERS + (level as u32 * 121 + i as u32 * 11 + j as u32) * 4, *v);
            }
        }
    }
    place_texts(m);
}

/// The texts alone: [`TEXTS`] at their addresses, from the user's program (si-rt: read and checked
/// once per process; panics with the reason when there is no install).
pub fn place_texts(m: &mut Mem) {
    for &addr in TEXTS {
        m.put_cstr(addr, si_rt::message(addr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_segment_as_the_renderer_reads_it() {
        si_rt::skip_without_install!();
        // the texts are the expected ones: length and FNV-1a of all of them in order, each with its zero
        let mut all = Vec::new();
        for &a in TEXTS {
            all.extend_from_slice(si_rt::message(a));
            all.push(0);
        }
        assert_eq!((TEXTS.len(), all.len(), si_rt::fnv1a64(&all)), (65, 1380, 0x7f04_295d_b16a_ae4f));
        let mut m = Mem::new();
        init(&mut m);
        assert_eq!(m.u32(0x0869_cf60), SI_DEFAULT_MATERIAL);
        assert_eq!(m.cstr(SI_DEFAULT_MATERIAL), b"default");
        // shading model 4 (Phong), ambient 0.5, diffuse 0.7, specular 1, exponent 50
        assert_eq!(m.i16(SI_DEFAULT_MATERIAL + 0x1e), 4);
        assert_eq!([m.f32(SI_DEFAULT_MATERIAL + 0x34), m.f32(SI_DEFAULT_MATERIAL + 0x44), m.f32(SI_DEFAULT_MATERIAL + 0x54), m.f32(SI_DEFAULT_MATERIAL + 0x64)], [0.5, 0.7, 1.0, 50.0]);
        assert_eq!((m.i16(SI_DEFAULT_MATERIAL + 0xdc8), m.i16(SI_DEFAULT_MATERIAL + 0xdca)), (1, 1));
        // the declarations cover the record without a gap or an overlap
        let mut at = SI_DEFAULT_MATERIAL;
        for g in globals::ALL.iter().filter(|g| g.name.starts_with("DEFMAT_")) {
            assert_eq!(g.addr, at, "{}", g.name);
            at += g.size;
        }
        assert_eq!(at, SI_DEFAULT_MATERIAL + SI_DEFAULT_MATERIAL_SIZE);
        // the tables the ports on values hold as constants are the same numbers
        for (i, v) in crate::shade::POISSON.iter().enumerate() {
            assert_eq!(m.f64(POISSON + 8 * i as u32).to_bits(), v.to_bits());
        }
        assert_eq!(m.f32(FILTERS + (2 * 121 + 1 * 11 + 1) * 4), crate::render::filter_for_level(2)[1][1]);
        assert_eq!(m.f32(0x0869_ce20).to_bits(), 0x3c23_d70a);
        // the texts of rnd_main.c are the ones that file places itself
        let mut n = Mem::new();
        crate::rnd_main::place_texts(&mut n);
        for a in [crate::rnd_main::text::SUFFIX_PIC, crate::rnd_main::text::MSG_NO_TAG_FILE, crate::rnd_main::text::LABEL_AFTER_RENDER] {
            assert_eq!(m.cstr(a), n.cstr(a));
        }
        // every address of `text` has its text
        for a in [text::FMT_PIC, text::MODE_RB_PLUS, text::FMT_STOP_AT_LINE, text::FMT_TWO, text::FMT_LINE_STATUS] {
            assert!(TEXTS.contains(&a));
        }
        // a pointer somebody set is left alone
        let mut m = Mem::new();
        m.set_u32(0x0869_bc84, 0x1234);
        init(&mut m);
        assert_eq!(m.u32(0x0869_bc84), 0x1234);
        assert!(m.fault().is_none());
    }
}
