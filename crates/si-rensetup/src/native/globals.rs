// SPDX-License-Identifier: MIT
//! The global variables the native hand-over reads and writes, at their addresses in the
//! binary, and the message texts its routines pass by address.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-rensetup`; the texts by
//! `re/tools/si-rensetup/check_strings.py`.
//!
//! Nothing in this file reproduces a function of the original.

use super::rec::{MemEntry, ProcData};
use si_rt::Table;
use si_mem::{FnPtr, Mem, Opaque, Ptr32};
use si_types::dict::{ChapIds, ElemDict, RelDict};
use si_types::scene::GrafList;

si_mem::globals! {
    /// The per-process data: a pointer to a record of which these files read three words.
    SI_PROC_DATA: Ptr32<ProcData> = 0x08679314, "SI_procData", init 0x08782108;
    /// The array of graf lists (0x24 bytes each); the scene's is the one the process
    /// record names.
    SI_GRAF_LISTS: Ptr32<GrafList> = 0x08679498, "SI_grafLists", init 0;
    /// The array of dictionaries (0x70 bytes each).
    SI_ELEM_DICTS: Ptr32<ElemDict> = 0x0867931c, "SI_elemDicts", init 0;
    /// The scene dictionary's chapter indices; `model` is the word at 0x08781680.
    SI_CID: ChapIds = 0x08781660, "SI_cid";
    /// The relation dictionary (only its address is used here).
    SI_RD: RelDict = 0x087815f0, "SI_RD";
    /// The list of blocks to free after a frame, newest first.
    MEM_LIST: Ptr32<MemEntry> = 0x0868e3c0, "memList", init 0;
    /// "Enter the material arrays of copied nodes in `memList`."
    REG_INST_MAT_ARRAYS: i16 = 0x0868e3c4, "regInstMatArrays", init 0;
    /// The first word of the manipulation settings: 1 is the "classic" mode, in which a
    /// copied node also gets its master's compensation records.
    SI_GLB_MANIP: i32 = 0x08781560, "SI_glbManip";
    /// The 16-bit word at `SI_glbManip` + 0x20: keep whole turns when angles are taken
    /// from a matrix (finding 0008).
    SI_GLB_MANIP_KEEP_TURNS: i16 = 0x08781580, "";
    /// Non-zero while a scene of the newer program is being converted.
    SI_ED_SUMATRA_TO_3X: i16 = 0x08680e50, "SI_edSumatraTo3x", init 0;
    /// The 16-bit word at `SI_glbDuplic` + 0x20, set to 1 while a lattice is copied.
    SI_GLB_DUPLIC_20: i16 = 0x087814e0, "";
    /// The render controls as the dialogues leave them.
    SI_DEFAULT_CONTROL: [u8; 0x600] = 0x08781800, "SI_defaultControl";
    /// The renderer's own copy of the controls.
    SI_REN: [u8; 0x600] = 0x08790be0, "SI_ren";
    /// The current frame.
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    /// The rest of the play control (the scene description's `PLAY_CONTROL` item): last
    /// frame, pause, rate (a float: frames a second), first frame, step, and the switch
    /// set when a step of 0 was read.
    SI_FRAME_END: i32 = 0x08782044, "SI_frameEnd";
    SI_FRAME_PAUSE: i32 = 0x08782048, "SI_framePause";
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    SI_FRAME_START: i32 = 0x08782050, "SI_frameStart";
    SI_FRAME_STEP: i32 = 0x08782054, "SI_frameStep";
    SI_STEP_ADAPTIVE: i16 = 0x0878fb28, "SI_stepAdaptive";
    /// The screen's greatest x and y.
    SI_XMAXSCREEN: i16 = 0x08781794, "SI_xmaxscreen";
    SI_YMAXSCREEN: i16 = 0x08781796, "SI_ymaxscreen";
    /// "Show controls": when 0, models whose algorithm is 18 to 29 are not rendered.
    SI_SHW_CTL: i16 = 0x08679516, "SI_shwCtl", init 1;
    /// 1 while the render window is open (`renu_display.c`).
    RENDER_WIN_OPEN: i32 = 0x086a6a80, "renderWinOpen", init 0;
    /// The renderer's last message; empty when all went well.
    SI_REN_MESSAGE: [u8; 0x400] = 0x0878ff00, "SI_renMessage";
    /// The renderer's chunk memory.
    SI_REN_CHUNK: Ptr32<Opaque> = 0x0869bc84, "SI_renChunk", init 0;
    /// Non-zero in the stand-alone renderer.
    SI_STANDALONE_RENDERER: i16 = 0x08679400, "SI_standaloneRenderer", init 0;
    /// Non-zero when a tablet is in use (`si_traverseExpandedInstance` fetches its button
    /// event then).
    SI_TABLET_ACTIVE: i32 = 0x086793f8, "SI_tabletActive", init 0;
    /// The tablet's last button.
    SI_TABLET_BUTTON: i32 = 0x086794b4, "SI_tabletButton", init 0;
    /// Cleared by `SI_sequenceRender` before anything else.
    SI_PREVIEW_SELECTION_CALL: i16 = 0x0869bc8a, "SI_previewSelectionCall", init 0;
    /// Set to 1 when an event other than a mouse button ends a pick.
    SI_GOTCHA: i16 = 0x08679538, "SI_gotcha", init 0;
    /// The filled-box routine of the GL layer, called through this pointer.
    SI_SBOXFI: FnPtr = 0x086ee2a4, "SI_sboxfi", init 0x0861236c;
    /// The dialogues' light and dark colours, one 16-bit word a component.
    SI_RGB_DIAL_LIGHT_B: i16 = 0x08817788, "SI_rgbDialLightB";
    SI_RGB_DIAL_LIGHT_G: i16 = 0x0881778a, "SI_rgbDialLightG";
    SI_RGB_DIAL_LIGHT_R: i16 = 0x0881778c, "SI_rgbDialLightR";
    SI_RGB_DIAL_DARK_B: i16 = 0x08817770, "SI_rgbDialDarkB";
    SI_RGB_DIAL_DARK_G: i16 = 0x08817772, "SI_rgbDialDarkG";
    SI_RGB_DIAL_DARK_R: i16 = 0x08817774, "SI_rgbDialDarkR";
    /// The dialogues' line spacing.
    SI_DLEADING: i16 = 0x08781424, "SI_dleading";
    /// The directory a picture whose name does not start with `/` is written to: at
    /// start-up a copy of the path of the output database's RENDER_PICTURES chapter (or of
    /// the working directory when that cannot be written to); the Render dialogue may
    /// replace it.
    SI_PICTURE_PATH: u32 = 0x086793cc, "SI_picturePath", init 0;
    /// The array of one material that a root without materials renders with: a pointer to
    /// `SI_defaultMaterial` (`SI_propagate` hands its address on as a material array).
    PTR_DEFAULT: u32 = 0x0869cf60, "ptrDefault", init 0x0869dde0;
    /// 0 while a scene is being loaded: an object that enters the scene graph's list is
    /// then not given a new element (its element exists already).
    SI_ADD_IN_DICTIONARY: i16 = 0x086792b0, "SI_addInDictionary", init 1;
    /// The stored noise of the previews: a pointer to floats, 32 bytes a pixel, 0xc80 a row.
    NOISE_TABLE: u32 = 0x086b9aec, "noise_table", init 0;
    /// The function that previews a pixel of a 3D texture (a routine address).
    SI_TTP_PREVIEW_SPH_FCT: u32 = 0x086b9a64, "SI_ttpPreviewSphFct", init 0;
    /// The working directory, a block from `getcwd` (set by `SI_initStandAloneRender`).
    SI_CWD: u32 = 0x087fba8c, "SI_cwd";
    /// No `init`: the binary's data holds 0, but the start-up sets them before a scene loads, and
    /// `init_loaded` must keep what the loaded image holds (a404baf zeroed SI_texturePath: 15 scenes lost).
    /// The directories of the object, texture, material, texture and palette libraries
    /// (blocks from `strdup`).
    SI_OBJECT_PATH: u32 = 0x086793b8, "SI_objectPath";
    SI_TEXTURE_PATH: u32 = 0x08679434, "SI_texturePath";
    SI_MATERIAL_LIB: u32 = 0x08679328, "SI_materialLib";
    SI_TEXTURE_LIB: u32 = 0x08679430, "SI_textureLib";
    SI_PALETTE_LIB: u32 = 0x086793c8, "SI_paletteLib";
    /// The input and output databases (`SI_dbi` records).
    SI_P_DBIIN: u32 = 0x08782058, "SI_pDbiin";
    SI_P_DBIOUT: u32 = 0x0878205c, "SI_pDbiout";
    /// The scene's graf list: a pointer into `SI_grafLists`.
    SI_SCNGL: u32 = 0x08695e9c, "SI_scngl", init 0;
    /// `SI_modelAllocate`'s own counter of the nodes it made.
    MODEL_ALLOCATE_COUNT: i32 = 0x086d6520, "count.228", init 0;
}

/// `SI_identite`: the identity matrix the allocators copy from. It is writable data with
/// an initial value of sixteen floats, which [`init`] stores from [`TABLES`].
pub const SI_IDENTITE: Ptr32<[[f32; 4]; 4]> = Ptr32::new(0x086d_c160);

/// Initial values of `.data` that are not scalars: (address, length) ranges of the program's
/// `.data`, read from the user's own install at run time (crates/si-rt) and checked against our
/// FNV-1a of each (`si-rt sum <addr> <len>`); `re/tools/si-rensetup/check_strings.py` holds the
/// list against the binary.
pub static TABLES: [Table; 14] = [
    Table::new("si-rensetup SI_identite", 0x086dc160, 0x40, 0x1b0c_97ae_01a7_ec05),
    // SI_xAxis, SI_yAxis, SI_zAxis (0x086dc1ec, 0x086dc1f8, 0x086dc204): the update's
    // SI_camMat and SI_animQuickStretchUpdate read them
    Table::new("si-rensetup SI_xAxis SI_yAxis SI_zAxis", 0x086dc1ec, 0x24, 0x6ff2_32cb_0d4a_8ad8),
    // SI_defaultMaterial (0x0869dde0, 0xdcc bytes of `.data`): the ranges that are not zero
    Table::new("si-rensetup SI_defaultMaterial +0x0", 0x0869dde0, 0x8, 0x98d5_8345_e9b0_6e9a),
    Table::new("si-rensetup SI_defaultMaterial +0x1c", 0x0869ddfc, 0xc, 0x317b_f7de_2720_6bda),
    Table::new("si-rensetup SI_defaultMaterial +0x34", 0x0869de14, 0xc, 0x97fd_aa78_d631_a5c8),
    Table::new("si-rensetup SI_defaultMaterial +0x44", 0x0869de24, 0xc, 0x413e_d15a_6c84_2fbd),
    Table::new("si-rensetup SI_defaultMaterial +0x54", 0x0869de34, 0xc, 0x8eec_422e_2592_0948),
    Table::new("si-rensetup SI_defaultMaterial +0x64", 0x0869de44, 0x4, 0x4c30_847f_9cfd_c047),
    Table::new("si-rensetup SI_defaultMaterial +0x6c8", 0x0869e4a8, 0x4, 0x4d25_477f_9dcd_c418),
    Table::new("si-rensetup SI_defaultMaterial +0x6dc", 0x0869e4bc, 0x4, 0x4b72_477f_9c5c_2f98),
    Table::new("si-rensetup SI_defaultMaterial +0x748", 0x0869e528, 0x4, 0x4b72_477f_9c5c_2f98),
    Table::new("si-rensetup SI_defaultMaterial +0x75c", 0x0869e53c, 0x8, 0x08a0_69ee_2ce9_5e18),
    Table::new("si-rensetup SI_defaultMaterial +0xda4", 0x0869eb84, 0xc, 0x83e9_1306_58a2_9f91),
    Table::new("si-rensetup SI_defaultMaterial +0xdc8", 0x0869eba8, 0x4, 0xad2e_3077_479b_3a8d),
];

/// Constants of `.rodata` the ports use by value (the image holds `.rodata` read-only and
/// nothing stores them): (address, 32-bit words), held against the binary by
/// `re/tools/si-rensetup/check_strings.py` like [`TABLES`].
pub const RODATA: &[(u32, &[u32])] = &[
    // the float SI_matterInit loads for the default light's position, 100.0
    (0x086551e8, &[0x42c80000]),
    // 0.5 and 0.2: the window position of SI_prepareRendering, the preview's light factors in SI_mentalPreview
    (0x086564e0, &[0x3f000000]),
    (0x086564e4, &[0x3e4ccccd]),
];

/// `SI_defaultMaterial`: the material a root without materials renders with (the name
/// `default`, shading model 4, ambient 0.5, diffuse 0.7, specular 1, exponent 50: its
/// initial value in `.data`, stored by [`init`] from [`TABLES`]).
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;

/// Records of `.data` whose words outside [`TABLES`] are zero in the binary: (address,
/// size), held against it by `re/tools/si-rensetup/check_strings.py`.
pub const ZERO_ELSEWHERE: &[(u32, u32)] = &[(0x0869dde0, 0xdcc)];

/// The message texts of `instance.c`, `matter.c` and `rnd_dial.c` that the ported routines
/// hand on by address, at the addresses the binary has them (all in `.data`). The texts are the
/// user's program's own, read at run time ([`store_data`]); 58 of them, 1388 bytes with their
/// zeros in this order, FNV-1a 0x0d07_2e2b_8c33_eba1 (the test below holds that).
pub const STRINGS: &[u32] = &[
    EXPANDING, NEED_INSTANCE, EMPTY_868E3FF, SWAP_SINGLE_2, SWAP_SINGLE_1, NEED_MASTER, CANCEL, PICK_MASTER_2,
    PICK_MASTER_1, OK, CYCLE_3, CYCLE_2, CYCLE_1, NEED_ROOT, NOT_AN_INSTANCE, TOO_LARGE_FORMAT, NO_MODELS,
    PREVIEW_WIDTH, PREVIEW_TITLE, EMPTY_86A10EF, RESOLUTION_FORMAT, EMPTY_86A2B0D, VOLUNTARY_EXIT,
    DISPLAY_DISABLED, TOO_LARGE_FOR_SCREEN, EMPTY_86A6AEC, MODEL_ALLOCATE_FORMAT, RC_DEFAULT, EMPTY_86A10F9,
    EMPTY_86A10FA, MR_DEFAULT_TEXT, MR_OUT_MI, EMPTY_86A110A, SOFT_INIT_FORMAT, CSH_SOFT_INIT_FORMAT,
    RENDER_PICTURES, PERSPECTIVE_DEF, BRACE_FORMAT, PR_TOO_LARGE_TO_DISPLAY, PR_TRACER, PR_TRACER2,
    PR_TRACER2_UNSET, PR_TRACER_UNSET, PR_CONTOUR_NO_DISPLAY, PR_CONTOUR_BSP, PR_OUT_MI, PR_MEMORY_ERROR_1,
    PR_DOT_MI, PR_MEMORY_ERROR_2, MP_NO_MODELS_SELECTED, MP_NO_MODELS, MP_TRACER, MP_TRACER2,
    MP_TRACER2_UNSET, MP_TRACER_UNSET, MP_TOO_LARGE, MP_TEMP_PREFIX, MP_TMPDIR,
];

/// The texts `SI_renControlInit` and `SI_mentalRayInit` copy: the output name, the two
/// empty scripts, the eight bytes stored at +0x34 of the mental ray part, the name of the
/// mental ray output file and the empty name of the photon map's file.
pub const RC_DEFAULT: u32 = 0x086a10f1;
pub const EMPTY_86A10F9: u32 = 0x086a10f9;
pub const EMPTY_86A10FA: u32 = 0x086a10fa;
pub const MR_DEFAULT_TEXT: u32 = 0x086a10fb;
pub const MR_OUT_MI: u32 = 0x086a1103;
pub const EMPTY_86A110A: u32 = 0x086a110a;

/// The texts of `SI_initStandAloneRender`, and the buffer it formats the start-up script's
/// name and command in.
pub const SOFT_INIT_FORMAT: u32 = 0x086d3400;
pub const CSH_SOFT_INIT_FORMAT: u32 = 0x086d340d;
pub const RENDER_PICTURES: u32 = 0x086d341e;
pub const PERSPECTIVE_DEF: u32 = 0x086d342e;
pub const BRACE_FORMAT: u32 = 0x086d3490;
pub const STRING_BUFFER: u32 = 0x0881ade0;

/// `SI_dbdir`: the list of databases (only its address is passed).
pub const SI_DBDIR: u32 = 0x08781390;

/// The format `SI_modelAllocate` names a node with when it is given no name.
pub const MODEL_ALLOCATE_FORMAT: u32 = 0x086d6524;
pub const EXPANDING: u32 = 0x0868e3c6;
pub const NEED_INSTANCE: u32 = 0x0868e3e1;
pub const EMPTY_868E3FF: u32 = 0x0868e3ff;
pub const SWAP_SINGLE_2: u32 = 0x0868e400;
pub const SWAP_SINGLE_1: u32 = 0x0868e426;
pub const NEED_MASTER: u32 = 0x0868e43f;
pub const CANCEL: u32 = 0x0868e45a;
pub const PICK_MASTER_2: u32 = 0x0868e461;
pub const PICK_MASTER_1: u32 = 0x0868e471;
pub const OK: u32 = 0x0868e481;
pub const CYCLE_3: u32 = 0x0868e4a0;
pub const CYCLE_2: u32 = 0x0868e4e0;
pub const CYCLE_1: u32 = 0x0868e500;
pub const NEED_ROOT: u32 = 0x0868e525;
pub const NOT_AN_INSTANCE: u32 = 0x0868e560;
pub const TOO_LARGE_FORMAT: u32 = 0x086a1080;
pub const NO_MODELS: u32 = 0x086a10c0;
pub const PREVIEW_WIDTH: u32 = 0x086a10df;
pub const PREVIEW_TITLE: u32 = 0x086a10e7;
pub const EMPTY_86A10EF: u32 = 0x086a10ef;
pub const RESOLUTION_FORMAT: u32 = 0x086a1500;
pub const EMPTY_86A2B0D: u32 = 0x086a2b0d;
pub const VOLUNTARY_EXIT: u32 = 0x086a2b0e;
/// The three texts of `SI_openRenderWindow`.
pub const DISPLAY_DISABLED: u32 = 0x086a6aa0;
pub const TOO_LARGE_FOR_SCREEN: u32 = 0x086a6ace;
pub const EMPTY_86A6AEC: u32 = 0x086a6aec;

/// The texts of `SI_prepareRendering` and `SI_mentalPreview` (`rens_main.c`) and the float
/// constants they load (0.5 and 0.2, in [`RODATA`]).
pub const SI_PREV_STRUCT: u32 = 0x086a6620;
pub const PR_TOO_LARGE_TO_DISPLAY: u32 = 0x086a6638;
pub const PR_TRACER: u32 = 0x086a6654;
pub const PR_TRACER2: u32 = 0x086a6661;
pub const PR_TRACER2_UNSET: u32 = 0x086a6680;
pub const PR_TRACER_UNSET: u32 = 0x086a66e0;
pub const PR_CONTOUR_NO_DISPLAY: u32 = 0x086a6740;
pub const PR_CONTOUR_BSP: u32 = 0x086a67a0;
pub const PR_OUT_MI: u32 = 0x086a6803;
pub const PR_MEMORY_ERROR_1: u32 = 0x086a6820;
pub const PR_DOT_MI: u32 = 0x086a6851;
pub const PR_MEMORY_ERROR_2: u32 = 0x086a6860;
pub const MP_NO_MODELS_SELECTED: u32 = 0x086a68a0;
pub const MP_NO_MODELS: u32 = 0x086a68ca;
pub const MP_TRACER: u32 = 0x086a68dc;
pub const MP_TRACER2: u32 = 0x086a68e9;
pub const MP_TRACER2_UNSET: u32 = 0x086a6900;
pub const MP_TRACER_UNSET: u32 = 0x086a6940;
pub const MP_TOO_LARGE: u32 = 0x086a6965;
pub const MP_TEMP_PREFIX: u32 = 0x086a6976;
pub const MP_TMPDIR: u32 = 0x086a6979;

/// Writes what the binary's `.data` holds for these files: the initial values of the
/// scalars above and the message texts.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    store_data(m);
}

/// The same for an image a scene was loaded into: the three pointers the loader's image
/// already holds (`SI_procData`, `SI_grafLists`, `SI_elemDicts`), the loader's own switch
/// `SI_addInDictionary`, and two words the start-up sets before the load are left as they
/// are: `SI_standaloneRenderer` (`standAloneRender`; the loader writes it from the scene)
/// and `SI_renChunk` (`SI_matterInit`) (finding 0023, section 22).
pub fn init_loaded(m: &mut Mem) {
    for g in ALL {
        let kept = [SI_PROC_DATA.0, SI_GRAF_LISTS.0, SI_ELEM_DICTS.0, SI_ADD_IN_DICTIONARY.0, SI_STANDALONE_RENDERER.0, SI_REN_CHUNK.0].contains(&g.addr);
        if let (Some(v), false) = (g.init, kept) {
            m.set_bytes(g.addr, &v.to_le_bytes()[..(g.size as usize).min(8)]);
        }
    }
    store_data(m);
    // the data segment's initial words of si-model's mesh code: `ptrmat` (SI_findMatInfo's one-entry material array, which
    // holds SI_defaultMaterial) and `SI_nbPolMax`; the load world had them zero, so SI_patchConvertToMesh could not be
    // routed in the load's dynamics update. `RENSETUP_NO_PTRMAT=1` leaves them as they were.
    if std::env::var_os("RENSETUP_NO_PTRMAT").is_none() {
        si_mem::init_globals(m, si_model::mesh_image::g::ALL);
    }
}

/// The message texts and the tables of `.data`, from the user's program (si-rt: read and checked
/// once per process; panics with the reason when there is no install, as every program using this
/// checks at its start).
pub fn store_data(m: &mut Mem) {
    for &addr in STRINGS {
        m.put_cstr(addr, si_rt::message(addr));
    }
    for t in &TABLES {
        m.set_bytes(t.addr, t.get());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts are the expected ones (length and FNV-1a of all of them in order, each with its
    /// zero, measured on the Linux program: re/tools/rtb/measure.py), and the tables land.
    #[test]
    fn the_program_texts_and_tables() {
        si_rt::skip_without_install!();
        let mut all = Vec::new();
        for &a in STRINGS {
            all.extend_from_slice(si_rt::message(a));
            all.push(0);
        }
        assert_eq!((STRINGS.len(), all.len(), si_rt::fnv1a64(&all)), (58, 1388, 0x0d07_2e2b_8c33_eba1));
        let mut m = Mem::new();
        store_data(&mut m);
        assert_eq!(m.cstr(SI_DEFAULT_MATERIAL), b"default");
        assert_eq!(m.f32(SI_IDENTITE.addr() + 0x14), 1.0);
        assert_eq!(m.cstr(MP_TMPDIR), si_rt::message(MP_TMPDIR));
    }
}
