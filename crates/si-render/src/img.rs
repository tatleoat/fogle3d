// SPDX-License-Identifier: MIT
//! The renderer on the shared memory image (`si_mem::Mem`, finding 0024): what the routines
//! ported in place need to name. Nothing in this file reproduces a function of the original.
//!
//! The seventh pass ported the frame's outer routine and the dictionary walks of
//! `initialize.c` in place: they take the image and the addresses the original takes. The
//! records they touch (`__SI_Model`, `__SI_Material`, `__SI_LclMap`, a dictionary element, a
//! chapter, a light, the picture record) are addressed here by **named offsets**, not by
//! typed records: `si-types` has layouts for some of them, but this crate does not depend on
//! it yet, and for the light, the picture record and the render controls no layout is
//! established. The globals are the addresses the binary gives them.
//!
//! A routine of another source file is called through [`Outside`]: the callee's code
//! address and the argument words in C order, the answer what the callee leaves in `eax`:
//! the calling convention of `si_mem::Registry` and of `si-rensetup`'s trait of the same
//! name, so that one registry of ported functions can stand behind both.

use si_mem::Mem;

/// The rest of the program, as the routines ported in place call it.
pub trait Outside {
    /// Calls the routine at the code address `f` with the stack words `args` and answers
    /// its `eax`. A float argument is its bit pattern.
    fn call(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32;

    /// True once a callee has left through `longjmp` to the buffer `SI_image` set
    /// (`si_fatal`, `SI_voluntaryExit`): `SI_image` then takes its second return from
    /// `_setjmp`. Only `SI_image` asks.
    fn jumped(&mut self) -> bool {
        false
    }
}

/// Offsets into `__SI_Model` (0x430 bytes).
pub mod model {
    /// i32: the node's kind; −1 a dummy, 0 a null, 5 a spline ...
    pub const KIND: u32 = 0x30;
    /// u16: what `pushMaterial` lent the node: bit 0 the material array, bit 2 the 2D
    /// texture list, bit 3 the 3D texture.
    pub const LENT: u32 = 0x204;
    /// i16: number of materials; +0x20c their array.
    pub const NB_MATERIALS: u32 = 0x206;
    pub const MATERIALS: u32 = 0x20c;
    /// u16: the flag word `mdlInitTxtCnt` clears and `setTxtFlag` fills.
    pub const TXT_FLAGS: u32 = 0x214;
    /// i16: number of 2D textures of the node's own list.
    pub const NB_TEXTURES: u32 = 0x216;
    /// A second pointer to the list's head, written beside it.
    pub const TEXTURE_218: u32 = 0x218;
    /// The head of the node's 2D texture list (linked through `__SI_LclMap` +0x16c).
    pub const TEXTURES: u32 = 0x21c;
    /// The node's 3D texture.
    pub const TEXTURE3D: u32 = 0x224;
    /// i32 count and block of render vertices; i32 count and block of render triangles.
    pub const REN_NB_VERTICES: u32 = 0x228;
    pub const REN_VERTICES: u32 = 0x22c;
    pub const REN_NB_TRIANGLES: u32 = 0x230;
    pub const REN_TRIANGLES: u32 = 0x234;
    pub const BROTHER: u32 = 0x2c0;
    pub const SON: u32 = 0x2c4;
    /// i32: the algorithm; 15 an instance.
    pub const ALGO_TYPE: u32 = 0x2ec;
    /// The block of further texture coordinates (`allocExtraTCoord`).
    pub const EXTRA_TCOORDS: u32 = 0x330;
    /// i32: the node's element number in the MODELS chapter.
    pub const ELEM_ID: u32 = 0x354;
}

/// Offsets into `__SI_Material` (0xdcc bytes).
pub mod material {
    /// i16: number of 2D textures; +0x6ec the head of their list.
    pub const NB_TEXTURES: u32 = 0x6e8;
    pub const TEXTURES: u32 = 0x6ec;
    /// The material's 3D texture.
    pub const TEXTURE3D: u32 = 0x6fc;
    /// i16: the count `mdlInitTxtCnt` makes of the list.
    pub const TXT_COUNT: u32 = 0x754;
}

/// Offsets into `__SI_LclMap` (0x1d8 bytes).
pub mod lclmap {
    /// i16: the mapping method; 4 takes the mesh's own coordinates.
    pub const METHOD: u32 = 0x2a;
    /// The next texture of the list.
    pub const NEXT: u32 = 0x16c;
}

/// Offsets into a dictionary element (`__SI_Elem`, 0x84 bytes), a chapter (`SI_ElemList`,
/// 0x40 bytes) and a dictionary (`SI_ElemDict`, 0x70 bytes).
pub mod dict {
    pub const ELEM_SIZE: u32 = 0x84;
    /// The element's name record (its text at +0x14 of that record).
    pub const ELEM_NAME_REC: u32 = 0x00;
    pub const NAME_REC_TEXT: u32 = 0x14;
    /// The element's object.
    pub const ELEM_DATA: u32 = 0x04;
    /// i16: not 0 when the element is not in use.
    pub const ELEM_UNUSED: u32 = 0x48;
    /// i32: the number of fathers (0 for a root).
    pub const ELEM_NB_FATHERS: u32 = 0x54;
    /// u8: the low byte of the work flag.
    pub const ELEM_WORK: u32 = 0x5b;
    /// The element's relation record: +0x18 the number of relations, +0x24 their array of
    /// 0x20 bytes each (+0 the kind, +4 the related element's number).
    pub const ELEM_RELATIONS: u32 = 0x6c;
    pub const REL_COUNT: u32 = 0x18;
    pub const REL_ARRAY: u32 = 0x24;
    pub const REL_SIZE: u32 = 0x20;
    /// The relation kind of a son.
    pub const REL_SON: u32 = 0x6e;
    pub const LIST_SIZE: u32 = 0x40;
    /// i32: the number of elements; +0x08 their array.
    pub const LIST_COUNT: u32 = 0x04;
    pub const LIST_ELEMS: u32 = 0x08;
    pub const DICT_SIZE: u32 = 0x70;
    /// The dictionary's array of chapters.
    pub const DICT_LISTS: u32 = 0x04;
}

/// Offsets into a `light` record.
pub mod light {
    /// i32: the light is selective; +0x164 the length of its per-model table, +0x168 the table.
    pub const SELECTIVE: u32 = 0x15c;
    pub const NB_MODELS: u32 = 0x164;
    pub const MODELS: u32 = 0x168;
}

/// Offsets into `SI_TagImage`.
pub mod tags {
    /// The tag file (`FILE *`).
    pub const FILE: u32 = 0x00;
    pub const WIDTH: u32 = 0x04;
    pub const HEIGHT: u32 = 0x06;
    pub const CHANNELS: u32 = 0x0e;
}

/// The globals, at the addresses the binary gives them.
pub mod g {
    /// `SI_frameCurrent`, i32.
    pub const SI_FRAME_CURRENT: u32 = 0x0878_2040;
    /// `SI_frameRate`, float.
    pub const SI_FRAME_RATE: u32 = 0x0878_204c;
    /// `si_renNoSpecAlpha`, 16 bits.
    pub const SI_REN_NO_SPEC_ALPHA: u32 = 0x0869_bc88;
    /// `SI_renMessage`, text.
    pub const SI_REN_MESSAGE: u32 = 0x0878_ff00;
    /// `SI_database`: the camera's copy (0x13c bytes), then at +0x19c the two words
    /// `SI_image` is handed, the head of the light chain and the root chain.
    pub const SI_DATABASE: u32 = 0x0879_0640;
    pub const DB_WORD_A: u32 = 0x0879_07dc;
    pub const DB_WORD_B: u32 = 0x0879_07e0;
    pub const DB_LIGHTS: u32 = 0x0879_07e4;
    pub const DB_ROOT: u32 = 0x0879_07e8;
    /// The picture: y min and y max, floats.
    pub const PICTURE_Y_MIN: u32 = 0x0879_0788;
    pub const PICTURE_Y_MAX: u32 = 0x0879_078c;
    /// The region in pixels: x min, x max (i32).
    pub const REGION_PX_X_MIN: u32 = 0x0879_07a0;
    pub const REGION_PX_X_MAX: u32 = 0x0879_07a4;
    /// The line number of the outgoing byte lines (16 bits) and the four lines' pointers.
    pub const OUT_LINE: u32 = 0x0879_0b3c;
    pub const OUT_RED: u32 = 0x0879_0b54;
    pub const OUT_GREEN: u32 = 0x0879_0b58;
    pub const OUT_BLUE: u32 = 0x0879_0b5c;
    pub const OUT_ALPHA: u32 = 0x0879_0b60;
    /// `SI_ren` (0x600 bytes): the jump buffer lies at its start.
    pub const SI_REN: u32 = 0x0879_0be0;
    /// 16 bits: 2 a sequence, 1 the preview.
    pub const SEQUENCE: u32 = 0x0879_0c7e;
    /// The background's four bytes.
    pub const BACKGROUND_BYTES: u32 = 0x0879_0d14;
    /// 16 bits each: field rendering, the first field, the field being rendered.
    pub const FIELDS: u32 = 0x0879_0d1c;
    pub const FIELD_FIRST: u32 = 0x0879_0d1e;
    pub const FIELD_NOW: u32 = 0x0879_0d20;
    /// 16 bits: adaptive supersampling.
    pub const ADAPTIVE: u32 = 0x0879_0d3a;
    /// 16 bits each: where the picture's window lies on the screen.
    pub const WINDOW_X: u32 = 0x0879_0d54;
    pub const WINDOW_Y: u32 = 0x0879_0d56;
    /// 16 bits each: display on, picture file on.
    pub const DISPLAY: u32 = 0x0879_0d58;
    pub const PICTURE_FILE: u32 = 0x0879_0d5a;
    /// Pointers to text: the output name, the script before a frame, the script after it.
    pub const NAME: u32 = 0x0879_0d5c;
    pub const PRE_SCRIPT: u32 = 0x0879_0d60;
    pub const POST_SCRIPT: u32 = 0x0879_0d64;
    /// 16 bits: timing statistics.
    pub const TIMING: u32 = 0x0879_0dc6;
    /// The statistics file (`FILE *`).
    pub const STATS_FILE: u32 = 0x0879_0dd0;
    /// 16 bits each: first frame, last frame, step, the frame being rendered.
    pub const FRAME_FIRST: u32 = 0x0879_0dd4;
    pub const FRAME_LAST: u32 = 0x0879_0dd6;
    pub const FRAME_STEP: u32 = 0x0879_0dd8;
    pub const FRAME_NOW: u32 = 0x0879_0dda;
    /// 16 bits: what `si_rleInit` is handed.
    pub const RLE_KEEP: u32 = 0x0879_0dee;
    /// The crop switch as the frame buffer routines test it (32 bits) and its upper half,
    /// which `SI_image` tests alone (16 bits; finding 0023 has it as the switch at +0x226 of
    /// the controls).
    pub const CROP: u32 = 0x0879_0e04;
    pub const CROP_HIGH: u32 = 0x0879_0e06;
    /// Floats: what the crop cuts off at the top and at the bottom.
    pub const CROP_TOP: u32 = 0x0879_0e14;
    pub const CROP_BOTTOM: u32 = 0x0879_0e18;
    /// 16 bits each: the crop's width and height.
    pub const CROP_WIDTH: u32 = 0x0879_0e1c;
    pub const CROP_HEIGHT: u32 = 0x0879_0e1e;
    /// 16 bits each: tag output, "a picture is resumed", resume mode; i32: the resumed
    /// picture's last line.
    pub const TAGS_ON: u32 = 0x0879_0e20;
    pub const RESUMED: u32 = 0x0879_0e22;
    pub const RESUME_MODE: u32 = 0x0879_0e24;
    pub const RESUME_LINE: u32 = 0x0879_0e28;
    /// i32: dither; 16 bits: its argument.
    pub const DITHER: u32 = 0x0879_0e30;
    pub const DITHER_ARG: u32 = 0x0879_0e34;
    /// 16 bits each: the picture's width and height.
    pub const WIDTH: u32 = 0x0879_117a;
    pub const HEIGHT: u32 = 0x0879_117c;
    /// `SI_stats`: its first word counts the triangles the geometry stage filed.
    pub const SI_STATS: u32 = 0x0879_11e0;
    /// i32: three times the frame buffer line's width (`si_fbInit`).
    pub const FB_WIDTH3: u32 = 0x0879_1218;
    /// `SI_tags`, a pointer.
    pub const SI_TAGS: u32 = 0x0879_1224;
    /// `tmp`, the text buffer of `rnd_main.c`.
    pub const TMP: u32 = 0x0879_1240;
    /// `blueNull`, `greenNull`, `redNull`: the frame buffer's background lines.
    pub const BLUE_NULL: u32 = 0x0879_92c0;
    pub const GREEN_NULL: u32 = 0x0879_92c4;
    pub const RED_NULL: u32 = 0x0879_92c8;
    /// The incrustation text (a `strdup`), and `SI_incrust` (16 bits).
    pub const INCRUST_TEXT: u32 = 0x0879_cc04;
    pub const SI_INCRUST: u32 = 0x0879_cc00;
    /// `SI_procData`, `SI_elemDicts`, `SI_grafLists`: pointers.
    pub const SI_PROC_DATA: u32 = 0x0867_9314;
    pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
    pub const SI_GRAF_LISTS: u32 = 0x0867_9498;
    /// `SI_picturePath`, a pointer to text.
    pub const SI_PICTURE_PATH: u32 = 0x0867_93cc;
    /// `SI_tabletActive` (i32), `SI_tabletButton` (i32), `SI_standaloneRenderer` (16 bits).
    pub const SI_TABLET_ACTIVE: u32 = 0x0867_93f8;
    pub const SI_TABLET_BUTTON: u32 = 0x0867_94b4;
    pub const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;
    /// `SI_sboxfi`: the pointer through which the filled box is drawn.
    pub const SI_SBOXFI: u32 = 0x086e_e2a4;
    /// `SI_renChunk`, a pointer.
    pub const SI_REN_CHUNK: u32 = 0x0869_bc84;
    /// `SI_fieldOneOnly`, `SI_updateTimeSet` (16 bits each), `SI_updateTime` (float).
    pub const SI_FIELD_ONE_ONLY: u32 = 0x0869_bca0;
    pub const SI_UPDATE_TIME_SET: u32 = 0x0869_bca2;
    pub const SI_UPDATE_TIME: u32 = 0x0869_bca4;
    /// `ptrDefault`: a pointer to `SI_defaultMaterial`, handed on as an array of one.
    pub const PTR_DEFAULT: u32 = 0x0869_cf60;
    /// `SI_irisf` and `SI_xmaxscreen`, 16 bits each.
    pub const SI_IRISF: u32 = 0x0878_1790;
    pub const SI_XMAXSCREEN: u32 = 0x0878_1794;
    /// `SI_ymaxscreen` and `SI_screenRes`, 16 bits each.
    pub const SI_YMAXSCREEN: u32 = 0x0878_1796;
    pub const SI_SCREEN_RES: u32 = 0x0878_17ae;
    /// 16 bits of `SI_ren` (+0xfa): 9 or 10 makes `si_fatal` write to the status file in
    /// the stand-alone renderer. Its name is not known.
    pub const REN_FA: u32 = 0x0879_0cda;
    /// The file-scope state of `rle.c`: `pFile` (a `FILE *`), `frameName` (512 bytes),
    /// `withAlpha` (16 bits).
    pub const P_FILE: u32 = 0x0869_c440;
    pub const FRAME_NAME: u32 = 0x0879_90a0;
    pub const WITH_ALPHA: u32 = 0x0879_92a0;
    /// The crop's left edge (float); with [`CROP_BOTTOM`] what the picture's comment names.
    pub const CROP_LEFT: u32 = 0x0879_0e0c;
    /// The pixel ratio (float).
    pub const PIXEL_RATIO: u32 = 0x0879_0d34;
    /// `SI_texturePath` and `SI_resdir`: pointers to text.
    pub const SI_TEXTURE_PATH: u32 = 0x0867_9434;
    pub const SI_RESDIR: u32 = 0x0878_1628;
    /// `SI_cid`: the chapter numbers; the words read here.
    /// The PICTURES chapter's number.
    pub const CID_PICTURES: u32 = 0x0878_1670;
    pub const CID_LIGHTS: u32 = 0x0878_1678;
    pub const CID_MATERIALS: u32 = 0x0878_167c;
    pub const CID_MODELS: u32 = 0x0878_1680;
    pub const CID_TEXTURES_2D: u32 = 0x0878_16c8;
    pub const CID_TEXTURES_3D: u32 = 0x0878_16cc;
}

/// The code addresses of the callees: each a routine of another source file, or one of this
/// package that is not ported in place, written as `name (0xADDRESS)`.
pub mod f {
    /// getenv (C library, through the binary's stub at 0x080c3bb8): (name).
    pub const GETENV: u32 = 0x080c_3bb8;
    /// time, times (C library, through the binary's stubs): (&t), (&tms).
    pub const TIME: u32 = 0x080c_4328;
    pub const TIMES: u32 = 0x080c_4988;
    /// pushviewport (0x0860fdcc), popviewport (0x0860fde4), frontbuffer (0x08611770), the GL
    /// layer: (), (), (on).
    pub const PUSHVIEWPORT: u32 = 0x0860_fdcc;
    pub const POPVIEWPORT: u32 = 0x0860_fde4;
    pub const FRONTBUFFER: u32 = 0x0861_1770;
    /// makeFilter (0x08273218), `camera.c`: ported on the crate's own records.
    pub const MAKE_FILTER: u32 = 0x0827_3218;
    /// SI_camera (0x08272450), si_setBounds (0x0826f318), si_traverse (0x08273910),
    /// si_traverseExpandedInstance (0x082739e0): (root); zSample (0x08277c94): (light).
    pub const SI_CAMERA: u32 = 0x0827_2450;
    pub const SI_SET_BOUNDS: u32 = 0x0826_f318;
    pub const SI_TRAVERSE: u32 = 0x0827_3910;
    pub const SI_TRAVERSE_EXPANDED_INSTANCE: u32 = 0x0827_39e0;
    pub const Z_SAMPLE: u32 = 0x0827_7c94;
    /// SI_sceneUpdateItems (0x082221a0, `motionUpdate.c`; ported in si-motion): (time, 1,
    /// camera, copy the camera); SI_edGetElementsGrafNodeInfo (0x08104acc, `edtools.c`):
    /// (dictionary). `SI_update` calls both before it reads the scene.
    pub const SI_SCENE_UPDATE_ITEMS: u32 = 0x0822_21a0;
    pub const SI_ED_GET_ELEMENTS_GRAF_NODE_INFO: u32 = 0x0810_4acc;
    /// SI_dtime (0x08260b78), `renutil.c`: (label or null).
    pub const SI_DTIME: u32 = 0x0826_0b78;
    /// SI_initialize (0x08276580): (dictionary).
    pub const SI_INITIALIZE: u32 = 0x0827_6580;
    /// SI_tagMdlRm (0x08347290), SI_tagMdlUpdate (0x083472f8): the tag channels of the models.
    pub const SI_TAG_MDL_RM: u32 = 0x0834_7290;
    pub const SI_TAG_MDL_UPDATE: u32 = 0x0834_72f8;
    /// SI_tagAllocate (0x0826d580), SI_tagAllocateChannels (0x0826d5c8),
    /// si_tagWriteFileHeader (0x0826d950): `rnd_tag.c`.
    pub const SI_TAG_ALLOCATE: u32 = 0x0826_d580;
    pub const SI_TAG_ALLOCATE_CHANNELS: u32 = 0x0826_d5c8;
    pub const SI_TAG_WRITE_FILE_HEADER: u32 = 0x0826_d950;
    /// SI_realloc (0x080d0f28), SI_malloc (0x080d0ee8): on the image, `si-mem`'s allocator.
    pub const SI_REALLOC: u32 = 0x080d_0f28;
    pub const SI_MALLOC: u32 = 0x080d_0ee8;
    /// SI_chunkSaveCurrent (0x0857ccc4), SI_chunkReclaim (0x0857cd00), SI_chunkFreeAll
    /// (0x0857cb00), SI_chunkMalloc (0x0857cb64), SI_chunkInitialize (0x0857ca90): `mem_chunk.c`.
    pub const SI_CHUNK_SAVE_CURRENT: u32 = 0x0857_ccc4;
    pub const SI_CHUNK_RECLAIM: u32 = 0x0857_cd00;
    pub const SI_CHUNK_FREE_ALL: u32 = 0x0857_cb00;
    pub const SI_CHUNK_MALLOC: u32 = 0x0857_cb64;
    pub const SI_CHUNK_INITIALIZE: u32 = 0x0857_ca90;
    /// si_fbInit (0x08273560), SI_fbWrite (0x082736d0): `fbuffer.c`.
    pub const SI_FB_INIT: u32 = 0x0827_3560;
    pub const SI_FB_WRITE: u32 = 0x0827_36d0;
    /// buildName (0x08258c10), SI_showframe (0x08258988), SI_showfield (0x08258a6c),
    /// SI_renExit (0x08258b84): `rnd_main.c`.
    pub const BUILD_NAME: u32 = 0x0825_8c10;
    pub const SI_SHOWFRAME: u32 = 0x0825_8988;
    pub const SI_SHOWFIELD: u32 = 0x0825_8a6c;
    pub const SI_REN_EXIT: u32 = 0x0825_8b84;
    /// printf, sprintf, fclose, rename, unlink, memcpy, memset, free, strdup, localtime,
    /// mallopt, _setjmp: the C library through the binary's stubs.
    pub const PRINTF: u32 = 0x080c_48b8;
    pub const FCLOSE: u32 = 0x080c_4958;
    pub const RENAME: u32 = 0x080c_3858;
    pub const UNLINK: u32 = 0x080c_3c78;
    pub const LOCALTIME: u32 = 0x080c_3a98;
    pub const MALLOPT: u32 = 0x080c_3c58;
    pub const SETJMP: u32 = 0x080c_4e48;
    /// SI_pictDisplayError (0x085d2900), SI_pictureOpenFile (0x085d2970), SI_scanlineRead
    /// (0x085d2cfc): the picture reader.
    pub const SI_PICT_DISPLAY_ERROR: u32 = 0x085d_2900;
    pub const SI_PICTURE_OPEN_FILE: u32 = 0x085d_2970;
    pub const SI_SCANLINE_READ: u32 = 0x085d_2cfc;
    /// si_rleInit (0x08262220), SI_rleWrite (0x08262854), SI_rleClose (0x082626ec): `rle.c`.
    pub const SI_RLE_INIT: u32 = 0x0826_2220;
    pub const SI_RLE_WRITE: u32 = 0x0826_2854;
    pub const SI_RLE_CLOSE: u32 = 0x0826_26ec;
    /// SI_system (0x08126748): (command).
    pub const SI_SYSTEM: u32 = 0x0812_6748;
    /// SI_update (0x0826edf0): (time, a float's bits).
    pub const SI_UPDATE: u32 = 0x0826_edf0;
    /// SI_fsDitherInit (0x085cfac4): (argument, width).
    pub const SI_FS_DITHER_INIT: u32 = 0x085c_fac4;
    /// SI_cusCheckRenderAbort (0x08535d30).
    pub const SI_CUS_CHECK_RENDER_ABORT: u32 = 0x0853_5d30;
    /// SI_tabletGetButtonUpEvent (0x083cec2c).
    pub const SI_TABLET_GET_BUTTON_UP_EVENT: u32 = 0x083c_ec2c;
    /// getbutton (0x0860e684), qreset (0x0860e12c), qread (0x0860e1bc), gflush (0x08613094),
    /// font (0x0860ec44), strwidth (0x0860f084), RGBcolor (0x086111a4), cmov2i (0x0860f2ac),
    /// charstr (0x0860ecc4), getorigin (0x08619ba4), lrectwrite (0x08611a50), sboxs
    /// (0x08612158): the GL layer.
    pub const GETBUTTON: u32 = 0x0860_e684;
    pub const QRESET: u32 = 0x0860_e12c;
    pub const QREAD: u32 = 0x0860_e1bc;
    /// `qtest`, `SI_tabletGetData`.
    pub const QTEST: u32 = 0x0860_e188;
    pub const SI_TABLET_GET_DATA: u32 = 0x083c_ec18;
    pub const GFLUSH: u32 = 0x0861_3094;
    pub const FONT: u32 = 0x0860_ec44;
    pub const STRWIDTH: u32 = 0x0860_f084;
    pub const RGBCOLOR: u32 = 0x0861_11a4;
    pub const CMOV2I: u32 = 0x0860_f2ac;
    pub const CHARSTR: u32 = 0x0860_ecc4;
    pub const GETORIGIN: u32 = 0x0861_9ba4;
    pub const LRECTWRITE: u32 = 0x0861_1a50;
    pub const SBOXS: u32 = 0x0861_2158;
    /// SI_voluntaryExit (0x08260918), SI_warning (0x0826072c): `renutil.c`.
    pub const SI_VOLUNTARY_EXIT: u32 = 0x0826_0918;
    pub const SI_WARNING: u32 = 0x0826_072c;
    /// SI_supersample (0x0825cd98), SI_sample (0x0825ba18): the two frame loops.
    pub const SI_SUPERSAMPLE: u32 = 0x0825_cd98;
    pub const SI_SAMPLE: u32 = 0x0825_ba18;
    /// si_cleanUpTexture (0x0825a7e0): `materials.c`.
    pub const SI_CLEAN_UP_TEXTURE: u32 = 0x0825_a7e0;
    /// SI_signalInit (0x080d1038): (signal or −1).
    pub const SI_SIGNAL_INIT: u32 = 0x080d_1038;
    /// SI_dialogRequest (0x08141c00): (text, second text, third text, button): the dialogue
    /// `SI_image` shows after a failed licence.
    pub const SI_DIALOG_REQUEST: u32 = 0x0814_1c00;
    /// SI_sceneRefresh (0x0820f470).
    pub const SI_SCENE_REFRESH: u32 = 0x0820_f470;
    /// SI_instanceDispose (0x081a9080): (address of a node pointer, brothers);
    /// SI_deleteMemReferences (0x081a7354); SI_modelMasterFromInstance (0x081a7490):
    /// `instance.c`.
    pub const SI_INSTANCE_DISPOSE: u32 = 0x081a_9080;
    pub const SI_DELETE_MEM_REFERENCES: u32 = 0x081a_7354;
    pub const SI_MODEL_MASTER_FROM_INSTANCE: u32 = 0x081a_7490;
    /// SI_elemdictAddPicture (0x08102ab0): (dictionary, name, picture record, 0);
    /// SI_elemdictPictureElemRemove (0x08102c34): (dictionary, picture record, name).
    pub const SI_ELEMDICT_ADD_PICTURE: u32 = 0x0810_2ab0;
    pub const SI_ELEMDICT_PICTURE_ELEM_REMOVE: u32 = 0x0810_2c34;
    /// SI_elemdictFindRelatedElem (0x0813508c): (dictionary, element, 1, 3, relation,
    /// chapter, &out, &out).
    pub const SI_ELEMDICT_FIND_RELATED_ELEM: u32 = 0x0813_508c;
    /// SI_edGetElemFromModel (0x080fcf20): (node, &out); SI_edGetElemFromTexture2D
    /// (0x080fd3e0): (texture, &out); SI_edFindSon (0x080fcec0), SI_edFindNextSon
    /// (0x080fcef0): (element).
    pub const SI_ED_GET_ELEM_FROM_MODEL: u32 = 0x080f_cf20;
    pub const SI_ED_GET_ELEM_FROM_TEXTURE2D: u32 = 0x080f_d3e0;
    pub const SI_ED_FIND_SON: u32 = 0x080f_cec0;
    pub const SI_ED_FIND_NEXT_SON: u32 = 0x080f_cef0;
    /// SI_elemlistClearLowByteWorkFlag (0x08132c70): (dictionary, chapter).
    pub const SI_ELEMLIST_CLEAR_LOW_BYTE_WORK_FLAG: u32 = 0x0813_2c70;
    /// SI_txt2dSetWrap (0x082ad9f8): (node, texture).
    pub const SI_TXT2D_SET_WRAP: u32 = 0x082a_d9f8;
    /// SI_statusDisplayMessage (0x083cca94): (text, 2).
    pub const SI_STATUS_DISPLAY_MESSAGE: u32 = 0x083c_ca94;
    /// si_fatal (0x08260560), `renutil.c`: (text, more text, seconds); ported in place
    /// ([`crate::renutil_image::fatal`]).
    pub const SI_FATAL: u32 = 0x0826_0560;
    /// SI_addIconToPic (0x085c4808), `geticon.c`: (path).
    pub const SI_ADD_ICON_TO_PIC: u32 = 0x085c_4808;
    /// SI_getStatusFile (0x083cd7ec), `status.c`: () answers a `FILE *`.
    pub const SI_GET_STATUS_FILE: u32 = 0x083c_d7ec;
    /// fprintf, fflush, sleep, longjmp: the C library through the binary's stubs.
    pub const FPRINTF: u32 = 0x080c_3b88;
    pub const FFLUSH: u32 = 0x080c_3be8;
    pub const SLEEP: u32 = 0x080c_4468;
    pub const LONGJMP: u32 = 0x080c_4458;
    /// getdisplaymode (0x086181f0), viewport (0x0860fe90), swapbuffers (0x08611804): the GL
    /// layer.
    pub const GETDISPLAYMODE: u32 = 0x0861_81f0;
    pub const VIEWPORT: u32 = 0x0860_fe90;
    pub const SWAPBUFFERS: u32 = 0x0861_1804;
    /// SI_chunkAllocate (0x0857ca00), SI_chunkDestroy (0x0857ca5c): `mem_chunk.c`.
    pub const SI_CHUNK_ALLOCATE: u32 = 0x0857_ca00;
    pub const SI_CHUNK_DESTROY: u32 = 0x0857_ca5c;
    /// SI_elemdictAddElem (0x08132a5c), SI_elemdictGetElem (0x08132b50),
    /// SI_elemdictFindElemByData (0x08134364): the dictionary (`elemdict.c`).
    pub const SI_ELEMDICT_ADD_ELEM: u32 = 0x0813_2a5c;
    pub const SI_ELEMDICT_GET_ELEM: u32 = 0x0813_2b50;
    pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: u32 = 0x0813_4364;
    /// SI_elemdictElementSetDeleteFlag (0x080fd6c8), `edtools.c`.
    pub const SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG: u32 = 0x080f_d6c8;
    /// SI_dbfilenameRemovePath (0x08121d84), SI_dbfilenameAddSuffix (0x08121d9c),
    /// SI_dbListNodeFree (0x08122450), SI_dbScnGetListNode (0x081319f0): the database's
    /// names and lists.
    pub const SI_DBFILENAME_REMOVE_PATH: u32 = 0x0812_1d84;
    pub const SI_DBFILENAME_ADD_SUFFIX: u32 = 0x0812_1d9c;
    pub const SI_DB_LIST_NODE_FREE: u32 = 0x0812_2450;
    pub const SI_DB_SCN_GET_LIST_NODE: u32 = 0x0813_19f0;
    /// initData (0x082765e8), si_initViewport (0x082768c4), initBuffers (0x08276ab4),
    /// initFilter (0x08276ca0): `initialize.c`, ported on the crate's own records.
    pub const INIT_DATA: u32 = 0x0827_65e8;
    pub const SI_INIT_VIEWPORT: u32 = 0x0827_68c4;
    pub const INIT_BUFFERS: u32 = 0x0827_6ab4;
    pub const INIT_FILTER: u32 = 0x0827_6ca0;
}

/// The C library's `sprintf` for the conversions the routines ported in place use: `%s`,
/// `%d`, `%hd`, `%1d`, `%1hd`, `%2d`, `%02d`, `%4d`. An integer is taken as the 32-bit word
/// pushed (`%hd` prints its low sixteen bits, signed); a string argument is its bytes.
pub fn sprintf(format: &[u8], args: &[FmtArg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut next = args.iter();
    let mut i = 0;
    while i < format.len() {
        if format[i] != b'%' {
            out.push(format[i]);
            i += 1;
            continue;
        }
        i += 1;
        let zero = format.get(i) == Some(&b'0');
        if zero {
            i += 1;
        }
        let mut width = 0usize;
        while let Some(d) = format.get(i).filter(|c| c.is_ascii_digit()) {
            width = width * 10 + (*d - b'0') as usize;
            i += 1;
        }
        let short = format.get(i) == Some(&b'h');
        if short {
            i += 1;
        }
        match format.get(i) {
            Some(b's') => {
                if let Some(FmtArg::Text(t)) = next.next() {
                    out.extend(std::iter::repeat(b' ').take(width.saturating_sub(t.len())));
                    out.extend_from_slice(t);
                }
            }
            Some(b'd') => {
                if let Some(FmtArg::Int(v)) = next.next() {
                    let v = if short { *v as i16 as i32 } else { *v };
                    let digits = v.unsigned_abs().to_string();
                    let sign = if v < 0 { "-" } else { "" };
                    let pad = width.saturating_sub(digits.len() + sign.len());
                    if zero {
                        out.extend_from_slice(sign.as_bytes());
                        out.extend(std::iter::repeat(b'0').take(pad));
                    } else {
                        out.extend(std::iter::repeat(b' ').take(pad));
                        out.extend_from_slice(sign.as_bytes());
                    }
                    out.extend_from_slice(digits.as_bytes());
                }
            }
            Some(b'%') => out.push(b'%'),
            _ => {}
        }
        i += 1;
    }
    out
}

/// The C library's `strtod(text, NULL)` for decimal numbers: leading white space, an
/// optional sign, digits with an optional point and an optional exponent, or a point and
/// digits; anything else, and nothing at all, is 0. (The library also reads hexadecimal
/// numbers, infinities and NaNs; those forms are not reproduced: they give 0 here. The
/// emulation harness's stand-in reads the same grammar.)
pub fn strtod(text: &[u8]) -> f64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start = i;
    if i < text.len() && matches!(text[i], b'+' | b'-') {
        i += 1;
    }
    let digits = |mut k: usize| {
        while k < text.len() && text[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    let int_end = digits(i);
    let mut end = int_end;
    if int_end > i {
        if end < text.len() && text[end] == b'.' {
            end = digits(end + 1);
        }
    } else if i < text.len() && text[i] == b'.' && digits(i + 1) > i + 1 {
        end = digits(i + 1);
    } else {
        return 0.0;
    }
    if end < text.len() && matches!(text[end], b'e' | b'E') {
        let mut k = end + 1;
        if k < text.len() && matches!(text[k], b'+' | b'-') {
            k += 1;
        }
        if digits(k) > k {
            end = digits(k);
        }
    }
    std::str::from_utf8(&text[start..end]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
}

/// One argument of [`sprintf`].
#[derive(Clone, Debug)]
pub enum FmtArg {
    Int(i32),
    Text(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_formats_of_the_outer_routine() {
        let t = |s: &str| FmtArg::Text(s.as_bytes().to_vec());
        assert_eq!(sprintf(b"%s/%s.%1hd.2.%s", &[t("/pics"), t("a"), FmtArg::Int(0x1_fffe), t("pic")]), b"/pics/a.-2.2.pic");
        assert_eq!(sprintf(b"frame %d.%1d", &[FmtArg::Int(12), FmtArg::Int(2)]), b"frame 12.2");
        assert_eq!(sprintf(b"%2d:%02d  %2d/%2d/%4d", &[FmtArg::Int(7), FmtArg::Int(5), FmtArg::Int(10), FmtArg::Int(3), FmtArg::Int(1999)]), b" 7:05  10/ 3/1999");
        assert_eq!(sprintf(b"%02d", &[FmtArg::Int(-5)]), b"-5");
        assert_eq!(strtod(b"  -1.5e2x"), -150.0);
        assert_eq!(strtod(b".25"), 0.25);
        assert_eq!(strtod(b"7."), 7.0);
        assert_eq!(strtod(b"1e"), 1.0);
        assert_eq!(strtod(b"abc"), 0.0);
        assert_eq!(strtod(b""), 0.0);
    }
}
