// SPDX-License-Identifier: MIT
//! The frame's outer routine: the original's `rnd_main.c` (0x08257a70 to 0x08258e32, seven
//! functions; finding 0012, sections 1 and 16). Ported in place: every routine takes the
//! memory image and the addresses the original takes, and calls what belongs to another
//! source file through [`Outside`] by its code address.
//!
//! `SI_image` is the entry the program's render commands call (`SI_frameRender`,
//! `SI_sequenceRender`; finding 0023). Its frame loop calls `SI_initialize`, `SI_update`,
//! the two frame loops and the picture writer through [`Outside`] too: those are ported on
//! this crate's own records, and [`crate::adapter`] is where they meet the image.

use crate::img::{f, g, light, model, sprintf, tags, FmtArg, Outside};
use si_mem::Mem;
use si_x87::{x, Round};

/// 0x08654180, 0x08654188, 0x08654190: three doubles, all 0.5.
const HALF: f64 = 0.5;

/// 0x0869bca8.
pub const ENV_NO_SPEC_ALPHA: &[u8] = b"SI_REN_NO_SPEC_ALPHA";
/// 0x0869bd14, 0x0869bd3f, 0x0869bd43: the three suffixes `SI_image` builds names with.
pub const SUFFIX_TAG: &[u8] = b"tag";
pub const SUFFIX_PIC: &[u8] = b"pic";
pub const SUFFIX_UNFINISH: &[u8] = b"unfinish";
/// 0x0869bd20, read from the user's install.
pub fn msg_no_tag_file() -> &'static [u8] {
    si_rt::message(0x0869_bd20)
}
/// 0x0869bd4d and 0x0869bdb3 (the same text twice).
pub fn fmt_script() -> &'static [u8] {
    si_rt::message(0x0869_bd4d)
}
/// 0x0869bd63, 0x0869bd79.
pub const FMT_SUN_TIME: &[u8] = b"%2d:%02d  %2d/%2d/%4d";
pub const NO_SUN: &[u8] = b"NO SUN!";
/// 0x0869bd81, 0x0869bda5: the labels of the two timings.
pub const LABEL_AFTER_UPDATE: &[u8] = b"after update";
pub const LABEL_AFTER_RENDER: &[u8] = b"after render";
/// 0x0869bd90, 0x0869bd8e: the warning of a frame without triangles and its second text.
pub fn msg_no_triangles() -> &'static [u8] {
    si_rt::message(0x0869_bd90)
}
pub const MSG_BLANK: &[u8] = b" ";

/// Where the texts above lie in the binary's data: `SI_image` hands their addresses on.
pub mod text {
    pub const ENV_NO_SPEC_ALPHA: u32 = 0x0869_bca8;
    pub const SUFFIX_TAG: u32 = 0x0869_bd14;
    pub const MSG_NO_TAG_FILE: u32 = 0x0869_bd20;
    pub const SUFFIX_PIC: u32 = 0x0869_bd3f;
    pub const SUFFIX_UNFINISH: u32 = 0x0869_bd43;
    pub const NO_SUN: u32 = 0x0869_bd79;
    pub const LABEL_AFTER_UPDATE: u32 = 0x0869_bd81;
    pub const MSG_BLANK: u32 = 0x0869_bd8e;
    pub const MSG_NO_TRIANGLES: u32 = 0x0869_bd90;
    pub const LABEL_AFTER_RENDER: u32 = 0x0869_bda5;
}

/// Stores the texts of [`text`] where the binary has them, so that a callee handed one of
/// their addresses can read it. Not a routine of the original: its data segment.
pub fn place_texts(m: &mut Mem) {
    for (a, t) in [
        (text::ENV_NO_SPEC_ALPHA, ENV_NO_SPEC_ALPHA),
        (text::SUFFIX_TAG, SUFFIX_TAG),
        (text::MSG_NO_TAG_FILE, msg_no_tag_file()),
        (text::SUFFIX_PIC, SUFFIX_PIC),
        (text::SUFFIX_UNFINISH, SUFFIX_UNFINISH),
        (text::NO_SUN, NO_SUN),
        (text::LABEL_AFTER_UPDATE, LABEL_AFTER_UPDATE),
        (text::MSG_BLANK, MSG_BLANK),
        (text::MSG_NO_TRIANGLES, msg_no_triangles()),
        (text::LABEL_AFTER_RENDER, LABEL_AFTER_RENDER),
    ] {
        m.put_cstr(a, t);
    }
}

/// Frees a node's block of further texture coordinates
/// [soft 0x08258b54 si_freeModelTcoord]: `si_freeModelTcoord(node)`; the pointer (+0x330)
/// is cleared. Nothing for a null pointer. Class: copy.
pub fn free_model_tcoord(m: &mut Mem, node: u32) {
    let p = m.u32(node + model::EXTRA_TCOORDS);
    if p != 0 {
        m.free(p);
        m.set_u32(node + model::EXTRA_TCOORDS, 0);
    }
}

/// Takes back what the render left on a node
/// [soft 0x08258cf8 SI_modelClearRenderData]: `SI_modelClearRenderData(node, all)`.
///
/// 1. With `all` = 1, for a node whose kind is not −1, 0 or 5: the three render blocks
///    (+0x330, +0x22c, +0x234) are freed and cleared, the two counts (+0x228, +0x230) set
///    to 0.
/// 2. When the node has a 3D texture (+0x224): every material of the node whose 3D texture
///    (+0x6fc) is that one loses it. (The texture a node lends its materials during the
///    render; a material that had the same texture of its own loses it too.)
/// 3. When `pushMaterial` lent the node its materials (+0x204 bit 0): the array and its
///    count are cleared; with bit 2 the 2D texture list, its second pointer and its count;
///    with bit 3 the 3D texture; then the word itself.
///
/// Class: copy.
pub fn model_clear_render_data(m: &mut Mem, node: u32, all: i16) {
    let kind = m.i32(node + model::KIND);
    if all == 1 && (kind.wrapping_add(1) as u32) > 1 && kind != 5 {
        for off in [model::EXTRA_TCOORDS, model::REN_VERTICES, model::REN_TRIANGLES] {
            let p = m.u32(node + off);
            if p != 0 {
                m.free(p);
                m.set_u32(node + off, 0);
            }
        }
        m.set_i32(node + model::REN_NB_VERTICES, 0);
        m.set_i32(node + model::REN_NB_TRIANGLES, 0);
    }
    if m.u32(node + model::TEXTURE3D) != 0 {
        let mut i: i32 = 0;
        // the count, the array and the texture are read again for every material
        while i < m.i16(node + model::NB_MATERIALS) as i32 {
            let mat = m.u32(m.u32(node + model::MATERIALS).wrapping_add(4 * i as u32));
            if m.u32(mat + crate::img::material::TEXTURE3D) == m.u32(node + model::TEXTURE3D) {
                m.set_u32(mat + crate::img::material::TEXTURE3D, 0);
            }
            i += 1;
        }
    }
    let lent = m.u16(node + model::LENT);
    if lent & 1 != 0 {
        m.set_u32(node + model::MATERIALS, 0);
        m.set_i16(node + model::NB_MATERIALS, 0);
        if lent & 4 != 0 {
            m.set_u32(node + model::TEXTURES, 0);
            m.set_u32(node + model::TEXTURE_218, 0);
            m.set_i16(node + model::NB_TEXTURES, 0);
        }
        if lent & 8 != 0 {
            m.set_u32(node + model::TEXTURE3D, 0);
        }
        m.set_u16(node + model::LENT, 0);
    }
}

/// The end of a render, over the root chain
/// [soft 0x08258b84 SI_renExit]: `SI_renExit(node, all, keep)`.
///
/// For `node` and each of its brothers: unless the node is `keep`,
/// [`model_clear_render_data`]; then a dummy root of an instance (kind −1, algorithm 15)
/// has the copy under it disposed with `SI_instanceDispose (0x081a9080)`(&son, 0) when it
/// has one, and any other node's sons are walked the same way; then
/// `SI_deleteMemReferences (0x081a7354)`, **once per node visited**. Class: copy.
pub fn ren_exit(m: &mut Mem, env: &mut dyn Outside, mut node: u32, all: i16, keep: u32) {
    while node != 0 {
        if node != keep {
            model_clear_render_data(m, node, all);
        }
        if m.i32(node + model::KIND) == -1 && m.i32(node + model::ALGO_TYPE) == 15 {
            if m.u32(node + model::SON) != 0 {
                env.call(m, f::SI_INSTANCE_DISPOSE, &[node + model::SON, 0]);
            }
        } else {
            let son = m.u32(node + model::SON);
            ren_exit(m, env, son, all, keep);
        }
        env.call(m, f::SI_DELETE_MEM_REFERENCES, &[]);
        node = m.u32(node + model::BROTHER);
    }
}

/// The name of a file of the frame being rendered
/// [soft 0x08258c10 buildName]: `buildName(buffer, suffix)`.
///
/// `<name>.<frame>.<suffix>`, with field rendering `<name>.<frame>.1.<suffix>` for field 0
/// (the word at 0x08790d20) and `.2.` for the other; a name that does not start with `/`
/// gets `SI_picturePath` and a `/` before it. The frame is printed as a 16-bit number. The
/// name pointer is read without a null test. Class: copy.
pub fn build_name(m: &mut Mem, buffer: u32, suffix: u32) {
    let name = m.u32(g::NAME);
    let text = m.cstr(name);
    let frame = FmtArg::Int(m.i16(g::FRAME_NOW) as i32);
    let suffix = FmtArg::Text(m.cstr(suffix));
    let middle: &[u8] = if m.i16(g::FIELDS) == 0 {
        b"."
    } else if m.i16(g::FIELD_NOW) == 0 {
        b".1."
    } else {
        b".2."
    };
    let mut format = Vec::new();
    let mut args = Vec::new();
    if m.u8(name) != b'/' {
        format.extend_from_slice(b"%s/");
        args.push(FmtArg::Text(m.cstr(m.u32(g::SI_PICTURE_PATH))));
    }
    format.extend_from_slice(b"%s.%1hd");
    format.extend_from_slice(middle);
    format.extend_from_slice(b"%s");
    args.push(FmtArg::Text(text));
    args.push(frame);
    args.push(suffix);
    let out = sprintf(&format, &args);
    m.put_cstr(buffer, &out);
}

/// The label both routines below draw: the text is in `tmp`.
fn show_label(m: &mut Mem, env: &mut dyn Outside, y: u32) {
    let width = env.call(m, f::STRWIDTH, &[g::TMP]) as i32;
    // (screen x max × 0.5) − (text width × 0.5), cut to an integer
    let left = (x(m.i16(g::SI_XMAXSCREEN) as i32) * HALF - x(width) * HALF).to_i32(Round::Zero);
    env.call(m, f::RGBCOLOR, &[100, 100, 100]);
    // thirty is added to the whole argument word, the sum's low sixteen bits kept
    let y = y.wrapping_add(0x1e) as i16 as i32;
    let sboxfi = m.u32(g::SI_SBOXFI);
    env.call(m, sboxfi, &[left.wrapping_sub(2) as u32, y.wrapping_sub(5) as u32, width.wrapping_add(3).wrapping_add(left) as u32, y.wrapping_add(15) as u32]);
    env.call(m, f::RGBCOLOR, &[0, 0, 0]);
    env.call(m, f::CMOV2I, &[left as u32, y as u32]);
    env.call(m, f::CHARSTR, &[g::TMP]);
}

/// Writes the frame number above the picture being rendered
/// [soft 0x08258988 SI_showframe]: `SI_showframe(frame, y)`.
///
/// `font (0x0860ec44)`(`SI_irisf`); `frame <n>` goes to `tmp`; its width from `strwidth
/// (0x0860f084)`; the text is centred on the screen: x = trunc(x max × 0.5 − width × 0.5),
/// at y + 30; a grey box (100, 100, 100) through the pointer `SI_sboxfi` from (x − 2, y −
/// 5) to (x + width + 3, y + 15), then the text in black (`RGBcolor (0x086111a4)`, `cmov2i
/// (0x0860f2ac)`, `charstr (0x0860ecc4)`). `y` is the whole argument word. Class: model.
pub fn show_frame(m: &mut Mem, env: &mut dyn Outside, frame: i16, y: u32) {
    env.call(m, f::FONT, &[m.i16(g::SI_IRISF) as i32 as u32]);
    let label = sprintf(b"frame %d", &[FmtArg::Int(frame as i32)]);
    m.put_cstr(g::TMP, &label);
    show_label(m, env, y);
}

/// The same for a field [soft 0x08258a6c SI_showfield]: `SI_showfield(frame, field, y)`
/// writes `frame <n>.<field>`. Class: model.
pub fn show_field(m: &mut Mem, env: &mut dyn Outside, frame: i16, field: i16, y: u32) {
    env.call(m, f::FONT, &[m.i16(g::SI_IRISF) as i32 as u32]);
    let label = sprintf(b"frame %d.%1d", &[FmtArg::Int(frame as i32), FmtArg::Int(field as i32)]);
    m.put_cstr(g::TMP, &label);
    show_label(m, env, y);
}

/// What `SI_image` answers: 0, 0x7e when a frame had no visible triangle or the tag file
/// could not be opened, 0x7f after a fatal error or an abort.
pub const STATUS_NOTHING: u32 = 0x7e;
pub const STATUS_ABORTED: u32 = 0x7f;

/// The locals of `SI_image` that callees write through: taken from the image's stack.
struct Locals {
    /// The chunk memory's mark of the frame.
    mark: u32,
    /// The name buffers: 1024 and 512 bytes.
    name: u32,
    name2: u32,
    /// What `SI_pictureOpenFile` fills: the file, a float, the channel table, the comment
    /// (84 bytes) and the picture record (0x28 bytes: +0 width, +2 height, +0x0a the mark,
    /// +0x0c the four line pointers).
    file: u32,
    ratio: u32,
    channels: u32,
    comment: u32,
    picture: u32,
    /// Two bytes for `qread`.
    event: u32,
}

/// Renders the frames the controls ask for [soft 0x08257a70 SI_image]:
/// `SI_image(camera, word, word, lights, roots)`.
///
/// What it expects in the image is finding 0012, section 16. In this order:
///
/// 1. `si_renNoSpecAlpha` = whether `getenv`(`SI_REN_NO_SPEC_ALPHA`) answers anything;
///    `SI_renMessage` is emptied; the camera record (0x13c bytes) is copied to the start of
///    `SI_database`; the four other arguments go to 0x087907dc, 0x087907e0, 0x087907e4,
///    0x087907e8. With timing on, `SI_dtime(null)`.
/// 2. `_setjmp` on `SI_ren`. A second return (a fatal error, an abort) goes to step 9.
/// 3. `SI_initialize(&SI_elemDicts[SI_procData[0]])`.
/// 4. With tag output: `SI_tagMdlRm`, `SI_tagMdlUpdate`; when that answers 0 tags are
///    switched off, else the tag image is made (`SI_tagAllocate`; width, 1 line, the
///    answer as the number of channels; `SI_tagAllocateChannels`; then the height).
/// 5. Every light of the lights chapter in use that is selective (+0x15c) and whose table
///    length (+0x164) is not the number of model elements gets its table (+0x168) grown to
///    that number + 1 words (`SI_realloc`, which on the image is `si-mem`'s), the new words
///    cleared, and the length stored.
/// 6. With field rendering the crop's top and bottom are halved (each product rounded to
///    float).
/// 7. For each frame from first to last by step (sixteen-bit counters; the frame goes to
///    0x08790dda), for each field (two with field rendering unless `SI_fieldOneOnly`):
///    the chunk mark is saved; the field word (0x08790d20) is the first field's or its
///    opposite; `si_fbInit` when displaying; the tag file is opened (on failure the message
///    is printed, tags are switched off and the status becomes 0x7e); the picture file:
///    see [`open_picture`]; unless that failed: the script before the frame in sequence
///    mode, the frame label when a sequence is displayed, **`SI_update(time)`** with time =
///    (field × 0.5 + frame) / `SI_frameRate` rounded to float (or `SI_updateTime` once,
///    when `SI_updateTimeSet`), the incrustation text, `SI_fsDitherInit`, the timing
///    `after update`; then the frame: without a filed triangle the background lines and
///    the warning (status 0x7e), else **`SI_supersample()` when the word at 0x08790d3a is
///    1, else `SI_sample()`**; the timing `after render`; `SI_rleClose(1, icon)` (icon 1
///    for the first frame only); `gflush` when displaying; the tag file closed; the script
///    after the frame; the chunk mark reclaimed.
/// 8. `si_cleanUpTexture`; `SI_tagMdlRm` with tags; the statistics file closed;
///    `SI_renExit(roots, 1, null)`; `SI_chunkFreeAll`; `SI_signalInit(−1)`; `qreset` unless
///    standalone; in the preview (the sequence word is 1) with an empty message it waits
///    for the middle mouse button and reads two events; `mallopt(1, 1000)`;
///    `SI_frameCurrent` gets back the value it had on entry, and `SI_sceneRefresh` is called
///    when the last frame rendered was another.
/// 9. After a jump: the statistics file closed, `SI_rleClose(0, 0)` with a picture file,
///    `SI_chunkFreeAll`, `SI_signalInit(−1)`, `SI_renExit`, `qreset`, the frame restored as
///    in step 8; the answer is 0x7f.
///
/// **Licence.** `SI_rendererLicenseFailed` (0x0869c020, 16 bits) is cleared before the
/// frame's sampling (after the update), and only `getRenderLicense`
/// ([`crate::licence`]), which the frame loops ask first, sets it. After the frame it is
/// tested, whether or not a frame was rendered, and when set `SI_image` leaves through
/// `longjmp(SI_ren, 0)`. On the jump path, after the frame is restored, the interactive
/// program (not the stand-alone renderer) with the word set shows
/// `SI_dialogRequest("Failed to reestablish connection to license server.", "Rendering
/// aborted.", "", "Ok")` and clears the word.
///
/// Class: model (one product, one quotient).
pub fn image(m: &mut Mem, env: &mut dyn Outside, camera: u32, word_a: u32, word_b: u32, lights: u32, roots: u32) -> u32 {
    let sp = m.sp();
    let saved_frame = m.i32(g::SI_FRAME_CURRENT);
    let has_env = env.call(m, f::GETENV, &[text::ENV_NO_SPEC_ALPHA]) != 0;
    m.set_u16(g::SI_REN_NO_SPEC_ALPHA, has_env as u16);
    m.set_u8(g::SI_REN_MESSAGE, 0);
    m.copy(g::SI_DATABASE, camera, 0x13c);
    m.set_u32(g::DB_WORD_A, word_a);
    m.set_u32(g::DB_WORD_B, word_b);
    m.set_u32(g::DB_LIGHTS, lights);
    m.set_u32(g::DB_ROOT, roots);
    if m.i16(g::TIMING) != 0 {
        env.call(m, f::SI_DTIME, &[0]);
    }
    let status = if env.call(m, f::SETJMP, &[g::SI_REN]) == 0 { frames(m, env, saved_frame) } else { None };
    let status = match status {
        Some(s) => s,
        None => {
            // the second return of _setjmp
            let stats = m.u32(g::STATS_FILE);
            if stats != 0 {
                env.call(m, f::FCLOSE, &[stats]);
            }
            if m.i16(g::PICTURE_FILE) != 0 {
                env.call(m, f::SI_RLE_CLOSE, &[0, 0]);
            }
            let chunk = m.u32(g::SI_REN_CHUNK);
            env.call(m, f::SI_CHUNK_FREE_ALL, &[chunk]);
            env.call(m, f::SI_SIGNAL_INIT, &[0xffff_ffff]);
            let root = m.u32(g::DB_ROOT);
            env.call(m, f::SI_REN_EXIT, &[root, 1, 0]);
            if m.i16(g::SI_STANDALONE_RENDERER) == 0 {
                env.call(m, f::QRESET, &[]);
            }
            restore_frame(m, env, saved_frame);
            if m.i16(g::SI_STANDALONE_RENDERER) == 0 && m.i16(crate::licence::SI_RENDERER_LICENSE_FAILED) != 0 {
                use crate::data::text as t;
                env.call(m, f::SI_DIALOG_REQUEST, &[t::DIALOG_LICENSE_FAILED, t::DIALOG_ABORTED, t::DIALOG_EMPTY, t::DIALOG_OK]);
                m.set_u16(crate::licence::SI_RENDERER_LICENSE_FAILED, 0);
            }
            STATUS_ABORTED
        }
    };
    m.set_sp(sp);
    status
}

/// `SI_frameCurrent` gets its value on entry back; a scene left at another frame is
/// refreshed.
fn restore_frame(m: &mut Mem, env: &mut dyn Outside, saved_frame: i32) {
    m.set_i32(g::SI_FRAME_CURRENT, saved_frame);
    if m.i16(g::FRAME_NOW) as i32 != saved_frame {
        env.call(m, f::SI_SCENE_REFRESH, &[]);
    }
}

/// Steps 3 to 8 of [`image`]; `None` when a callee left through `longjmp`.
fn frames(m: &mut Mem, env: &mut dyn Outside, saved_frame: i32) -> Option<u32> {
    // a call that may not come back
    macro_rules! call {
        ($f:expr, $args:expr) => {{
            let r = env.call(m, $f, $args);
            if env.jumped() {
                return None;
            }
            r
        }};
    }
    let mut status = 0u32;
    let mut icon: i16 = 1;
    let mut failed = false;
    let dict = m.u32(g::SI_ELEM_DICTS).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA)) as u32).wrapping_mul(crate::img::dict::DICT_SIZE));
    call!(f::SI_INITIALIZE, &[dict]);
    let show = m.i16(g::SEQUENCE) == 2 && m.i16(g::DISPLAY) != 0;
    let mut frame_index: i16 = 0;
    let fields: i16 = if m.i16(g::FIELDS) != 0 && m.i16(g::SI_FIELD_ONE_ONLY) == 0 { 2 } else { 1 };
    if m.i16(g::TAGS_ON) != 0 {
        call!(f::SI_TAG_MDL_RM, &[]);
        let channels = call!(f::SI_TAG_MDL_UPDATE, &[]);
        if channels != 0 {
            let t = call!(f::SI_TAG_ALLOCATE, &[]);
            let width = m.u16(g::WIDTH);
            m.set_u32(g::SI_TAGS, t);
            m.set_u16(t + tags::WIDTH, width);
            m.set_u16(t + tags::HEIGHT, 1);
            m.set_u16(t + tags::CHANNELS, channels as u16);
            call!(f::SI_TAG_ALLOCATE_CHANNELS, &[t]);
            let t = m.u32(g::SI_TAGS);
            let height = m.u16(g::HEIGHT);
            m.set_u16(t + tags::HEIGHT, height);
        } else {
            m.set_i16(g::TAGS_ON, 0);
        }
    }
    // the selective lights' tables
    {
        use crate::img::dict::*;
        let lists = m.u32(dict + DICT_LISTS);
        let chapter = lists.wrapping_add((m.u32(g::CID_LIGHTS)).wrapping_mul(LIST_SIZE));
        let models = m.i32(lists.wrapping_add(m.u32(g::CID_MODELS).wrapping_mul(LIST_SIZE)) + LIST_COUNT);
        let mut i: i32 = 0;
        while i < m.i32(chapter + LIST_COUNT) {
            let elem = m.u32(chapter + LIST_ELEMS).wrapping_add((i as u32).wrapping_mul(ELEM_SIZE));
            if m.i16(elem + ELEM_UNUSED) == 0 {
                let l = m.u32(elem + ELEM_DATA);
                if m.i32(l + light::SELECTIVE) != 0 && m.i32(l + light::NB_MODELS) != models {
                    let table = m.u32(l + light::MODELS);
                    let grown = m.si_realloc(table, (models.wrapping_add(1) as u32) << 2);
                    let mut k = m.i32(l + light::NB_MODELS);
                    m.set_u32(l + light::MODELS, grown);
                    while k < models.wrapping_add(1) {
                        let t = m.u32(l + light::MODELS);
                        m.set_u32(t.wrapping_add((k as u32) << 2), 0);
                        k += 1;
                    }
                    m.set_i32(l + light::NB_MODELS, models);
                }
            }
            i += 1;
        }
    }
    if m.i16(g::FIELDS) != 0 {
        let bottom = (x(m.f32(g::CROP_BOTTOM)) * HALF).to_f32();
        let top = (x(m.f32(g::CROP_TOP)) * HALF).to_f32();
        m.set_f32(g::CROP_BOTTOM, bottom);
        m.set_f32(g::CROP_TOP, top);
    }
    let l = Locals {
        name2: m.local(512),
        name: m.local(1024),
        picture: m.local(0x28),
        comment: m.local(84),
        mark: m.local(4),
        channels: m.local(4),
        ratio: m.local(4),
        file: m.local(4),
        event: m.local(4),
    };
    let mut frame = m.i16(g::FRAME_FIRST);
    while frame <= m.i16(g::FRAME_LAST) {
        m.set_i16(g::FRAME_NOW, frame);
        let mut field: i16 = 0;
        'fields: while field < fields {
            let chunk = m.u32(g::SI_REN_CHUNK);
            call!(f::SI_CHUNK_SAVE_CURRENT, &[chunk, l.mark]);
            if m.i16(g::FIELDS) != 0 {
                let first = m.u16(g::FIELD_FIRST);
                m.set_u16(g::FIELD_NOW, if field == 0 { first } else { first ^ 1 });
            }
            if m.i16(g::DISPLAY) != 0 {
                call!(f::SI_FB_INIT, &[]);
            }
            if m.i16(g::TAGS_ON) != 0 {
                call!(f::BUILD_NAME, &[l.name, text::SUFFIX_TAG]);
                let t = m.u32(g::SI_TAGS);
                let file = call!(f::SI_TAG_WRITE_FILE_HEADER, &[l.name, t]);
                let t = m.u32(g::SI_TAGS);
                m.set_u32(t + tags::FILE, file);
                if file == 0 {
                    call!(f::PRINTF, &[text::MSG_NO_TAG_FILE, l.name]);
                    m.set_i16(g::TAGS_ON, 0);
                    status = STATUS_NOTHING;
                }
            }
            if m.i16(g::PICTURE_FILE) != 0 {
                match open_picture(m, env, &l) {
                    None => return None,
                    Some(Opened::Finished) => {
                        // a finished picture is left alone: nothing more for this field,
                        // not even the chunk mark's reclaim
                        field = field.wrapping_add(1);
                        continue 'fields;
                    }
                    Some(Opened::Resumed) => {}
                    Some(Opened::New(ok)) => failed = !ok,
                }
            }
            if !failed {
                if m.i16(g::SEQUENCE) == 2 {
                    let script = m.u32(g::PRE_SCRIPT);
                    if m.u8(script) != 0 {
                        run_script(m, script, frame_index, field);
                        call!(f::SI_SYSTEM, &[g::TMP]);
                    }
                }
                let height = if m.i16(g::CROP_HIGH) != 0 { m.i16(g::CROP_HEIGHT) } else { m.i16(g::HEIGHT) };
                if show {
                    let y = (height as u16).wrapping_add(m.u16(g::WINDOW_Y)) as i16 as i32 as u32;
                    let now = m.i16(g::FRAME_NOW) as i32 as u32;
                    if m.i16(g::FIELDS) != 0 {
                        call!(f::SI_SHOWFIELD, &[now, field.wrapping_add(1) as i32 as u32, y]);
                    } else {
                        call!(f::SI_SHOWFRAME, &[now, y]);
                    }
                }
                if m.i16(g::SI_UPDATE_TIME_SET) != 0 {
                    let time = m.u32(g::SI_UPDATE_TIME);
                    call!(f::SI_UPDATE, &[time]);
                    m.set_i16(g::SI_UPDATE_TIME_SET, 0);
                } else {
                    let time = ((x(field as i32) * HALF + x(m.i16(g::FRAME_NOW) as i32)) / x(m.f32(g::SI_FRAME_RATE))).to_f32();
                    call!(f::SI_UPDATE, &[time.to_bits()]);
                }
                if m.i16(g::SI_INCRUST) != 0 {
                    if incrustation(m, env).is_none() {
                        return None;
                    }
                }
                if m.i32(g::DITHER) != 0 {
                    let width = m.i32(g::REGION_PX_X_MAX).wrapping_sub(m.i32(g::REGION_PX_X_MIN)).wrapping_add(1);
                    call!(f::SI_FS_DITHER_INIT, &[m.i16(g::DITHER_ARG) as i32 as u32, width as u32]);
                }
                if m.i16(g::TIMING) != 0 {
                    call!(f::SI_DTIME, &[text::LABEL_AFTER_UPDATE]);
                }
            }
            // the licence word (see `image`)
            m.set_u16(crate::licence::SI_RENDERER_LICENSE_FAILED, 0);
            if !failed {
                if m.i32(g::SI_STATS) == 0 {
                    call!(f::SI_CUS_CHECK_RENDER_ABORT, &[]);
                    if m.i16(g::SI_STANDALONE_RENDERER) == 0 {
                        if m.i32(g::SI_TABLET_ACTIVE) != 0 {
                            call!(f::SI_TABLET_GET_BUTTON_UP_EVENT, &[]);
                        }
                        if call!(f::GETBUTTON, &[0x66]) != 0 || m.i32(g::SI_TABLET_BUTTON) == 0x66 || call!(f::GETBUTTON, &[7]) != 0 {
                            call!(f::SI_VOLUNTARY_EXIT, &[4, (m.i16(g::DISPLAY) == 0) as u32]);
                        }
                    }
                    // every line of the picture as background, top line first; the counter
                    // is compared in sixteen bits
                    let mut y = x(m.f32(g::PICTURE_Y_MAX)).to_i32(Round::Zero);
                    while x(y as i16 as i32).ge(x(m.f32(g::PICTURE_Y_MIN))) {
                        m.set_i16(g::OUT_LINE, y as i16);
                        for (line, byte) in [g::OUT_RED, g::OUT_GREEN, g::OUT_BLUE, g::OUT_ALPHA].into_iter().zip(0u32..) {
                            let (p, v, n) = (m.u32(line), m.u8(g::BACKGROUND_BYTES + byte), m.i16(g::WIDTH) as i32 as u32);
                            m.set_fill_bytes(p, v, n);
                        }
                        if m.i16(g::PICTURE_FILE) != 0 {
                            let a = [m.i16(g::OUT_LINE) as i32 as u32, m.i16(g::WIDTH) as i32 as u32, m.u32(g::OUT_RED), m.u32(g::OUT_GREEN), m.u32(g::OUT_BLUE), m.u32(g::OUT_ALPHA)];
                            call!(f::SI_RLE_WRITE, &a);
                        }
                        if m.i16(g::DISPLAY) != 0 {
                            call!(f::SI_FB_WRITE, &[m.i16(g::OUT_LINE) as i32 as u32]);
                        }
                        y = y.wrapping_sub(1);
                    }
                    call!(f::SI_WARNING, &[text::MSG_NO_TRIANGLES, text::MSG_BLANK, 2]);
                    status = STATUS_NOTHING;
                } else if m.i16(g::ADAPTIVE) == 1 {
                    call!(f::SI_SUPERSAMPLE, &[]);
                } else {
                    call!(f::SI_SAMPLE, &[]);
                }
            }
            // a failed licence leaves through `longjmp` (which does not come back)
            if m.i16(crate::licence::SI_RENDERER_LICENSE_FAILED) != 0 {
                env.call(m, f::LONGJMP, &[g::SI_REN, 0]);
                return None;
            }
            if !failed && m.i16(g::TIMING) != 0 {
                call!(f::SI_DTIME, &[text::LABEL_AFTER_RENDER]);
            }
            if m.i16(g::PICTURE_FILE) != 0 {
                call!(f::SI_RLE_CLOSE, &[1, icon as i32 as u32]);
            }
            icon = 0;
            if m.i16(g::DISPLAY) != 0 {
                call!(f::GFLUSH, &[]);
            }
            if m.i16(g::TAGS_ON) != 0 {
                let file = m.u32(m.u32(g::SI_TAGS) + tags::FILE);
                call!(f::FCLOSE, &[file]);
            }
            if !failed && m.i16(g::SEQUENCE) == 2 {
                let script = m.u32(g::POST_SCRIPT);
                if m.u8(script) != 0 {
                    run_script(m, script, frame_index, field);
                    call!(f::SI_SYSTEM, &[g::TMP]);
                }
            }
            let (chunk, mark) = (m.u32(g::SI_REN_CHUNK), m.u32(l.mark));
            call!(f::SI_CHUNK_RECLAIM, &[chunk, mark]);
            field = field.wrapping_add(1);
        }
        frame = frame.wrapping_add(m.i16(g::FRAME_STEP));
        frame_index = frame_index.wrapping_add(1);
    }
    call!(f::SI_CLEAN_UP_TEXTURE, &[]);
    if m.i16(g::TAGS_ON) != 0 {
        call!(f::SI_TAG_MDL_RM, &[]);
    }
    let stats = m.u32(g::STATS_FILE);
    if stats != 0 {
        call!(f::FCLOSE, &[stats]);
    }
    let root = m.u32(g::DB_ROOT);
    call!(f::SI_REN_EXIT, &[root, 1, 0]);
    let chunk = m.u32(g::SI_REN_CHUNK);
    call!(f::SI_CHUNK_FREE_ALL, &[chunk]);
    call!(f::SI_SIGNAL_INIT, &[0xffff_ffff]);
    if m.i16(g::SI_STANDALONE_RENDERER) == 0 {
        call!(f::QRESET, &[]);
    }
    if m.u8(g::SI_REN_MESSAGE) == 0 && m.i16(g::SEQUENCE) == 1 {
        // the preview waits for the middle mouse button
        while call!(f::GETBUTTON, &[0x66]) == 0 && m.i32(g::SI_TABLET_BUTTON) != 0x66 {
            if m.i32(g::SI_TABLET_ACTIVE) != 0 {
                call!(f::SI_TABLET_GET_BUTTON_UP_EVENT, &[]);
            }
        }
        call!(f::QREAD, &[l.event]);
        call!(f::QREAD, &[l.event]);
    }
    call!(f::MALLOPT, &[1, 1000]);
    restore_frame(m, env, saved_frame);
    if env.jumped() {
        return None;
    }
    Some(status)
}

/// `csh <script> <name> <frame> <index> <field + 1>` into `tmp`.
fn run_script(m: &mut Mem, script: u32, frame_index: i16, field: i16) {
    let args = [
        FmtArg::Text(m.cstr(script)),
        FmtArg::Text(m.cstr(m.u32(g::NAME))),
        FmtArg::Int(m.i16(g::FRAME_NOW) as i32),
        FmtArg::Int(frame_index as i32),
        FmtArg::Int(field as i32 + 1),
    ];
    let t = sprintf(fmt_script(), &args);
    m.put_cstr(g::TMP, &t);
}

/// The incrustation text of the frame: the hour and date of the scene's sun (the first
/// graf node of kind 0x14 in the scene's list, its object's record at +0x16c, the time at
/// +0x10 of that, through `localtime`), or `NO SUN!`; the text before it is freed.
fn incrustation(m: &mut Mem, env: &mut dyn Outside) -> Option<()> {
    let list = m.u32(g::SI_GRAF_LISTS).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24));
    let mut node = m.u32(list + 4);
    let mut found = false;
    while node != 0 {
        if m.i32(node) == 0x14 {
            let when = m.u32(m.u32(node + 4) + 0x16c).wrapping_add(0x10);
            let tm = env.call(m, f::LOCALTIME, &[when]);
            if env.jumped() {
                return None;
            }
            // struct tm: +4 minutes, +8 hours, +0xc day of the month, +0x10 month, +0x14 year
            let args = [m.i32(tm + 8), m.i32(tm + 4), m.i32(tm + 0x10).wrapping_add(1), m.i32(tm + 0xc), m.i32(tm + 0x14).wrapping_add(0x76c)].map(FmtArg::Int);
            let t = sprintf(FMT_SUN_TIME, &args);
            m.put_cstr(g::TMP, &t);
            let old = m.u32(g::INCRUST_TEXT);
            if old != 0 {
                m.free(old);
                m.set_u32(g::INCRUST_TEXT, 0);
            }
            let copy = m.strdup(g::TMP);
            m.set_u32(g::INCRUST_TEXT, copy);
            found = true;
            break;
        }
        node = m.u32(node + 0x1c);
    }
    if !found {
        let old = m.u32(g::INCRUST_TEXT);
        if old != 0 {
            m.free(old);
            m.set_u32(g::INCRUST_TEXT, 0);
        }
        let copy = m.strdup(text::NO_SUN);
        m.set_u32(g::INCRUST_TEXT, copy);
    }
    Some(())
}

/// What became of the frame's picture file.
enum Opened {
    /// A new file; false when `si_rleInit` failed.
    New(bool),
    /// An unfinished picture was copied into a new file; rendering goes on below its lines.
    Resumed,
    /// The picture on disc is finished.
    Finished,
}

/// The picture file of the frame (step 7 of [`image`]); `None` after a jump.
///
/// Outside resume mode (0x08790e24): the "resumed" word (0x08790e22) is cleared and
/// `si_rleInit(the word at 0x08790dee)` opens the file; a zero answer is a failure, and
/// the frame is then neither prepared nor rendered (but the file is still "closed").
///
/// In resume mode the frame's picture is opened (`SI_pictureOpenFile`, between
/// `SI_pictDisplayError(0)` and `(1)`). When there is none, or its width is not the
/// picture's, or its height not the picture's (half of it with field rendering, a signed
/// sixteen-bit division), the frame is rendered anew as above. A picture with the mark `C`
/// (+0x0a = 0x43) is finished: nothing more happens for this field. Otherwise it is
/// renamed to `…unfinish`, opened again, four lines of its width are taken with
/// `SI_malloc` and stored in the picture record (+0x0c), `si_rleInit(0)` opens the new
/// file (its answer is not looked at), and line by line from the top (height − 1 down)
/// `SI_scanlineRead` fills the four lines, which are copied to the outgoing byte lines,
/// shown and written; then the old file is closed and removed, the "resumed" word set and
/// the number of the line the read stopped at stored at 0x08790e28. **The failure word is
/// not touched on this path: it keeps what the frame before left.** The four lines are not
/// freed.
fn open_picture(m: &mut Mem, env: &mut dyn Outside, l: &Locals) -> Option<Opened> {
    macro_rules! call {
        ($f:expr, $args:expr) => {{
            let r = env.call(m, $f, $args);
            if env.jumped() {
                return None;
            }
            r
        }};
    }
    let fresh = |m: &mut Mem, env: &mut dyn Outside| -> Option<Opened> {
        m.set_i16(g::RESUMED, 0);
        let ok = env.call(m, f::SI_RLE_INIT, &[m.i16(g::RLE_KEEP) as i32 as u32]) as u16 != 0;
        if env.jumped() {
            return None;
        }
        Some(Opened::New(ok))
    };
    if m.i16(g::RESUME_MODE) == 0 {
        return fresh(m, env);
    }
    call!(f::BUILD_NAME, &[l.name, text::SUFFIX_PIC]);
    call!(f::SI_PICT_DISPLAY_ERROR, &[0]);
    let found = call!(f::SI_PICTURE_OPEN_FILE, &[l.name, l.file, l.picture, l.ratio, l.comment, l.channels]) as u16;
    call!(f::SI_PICT_DISPLAY_ERROR, &[1]);
    if found == 0 {
        return fresh(m, env);
    }
    let file = m.u32(l.file);
    call!(f::FCLOSE, &[file]);
    let mut same = false;
    if m.u16(l.picture) == m.u16(g::WIDTH) {
        let h = m.i16(l.picture + 2) as i32;
        let want = if m.i16(g::FIELDS) != 0 {
            let full = m.u16(g::HEIGHT);
            ((full >> 15).wrapping_add(full) as i16 >> 1) as i32
        } else {
            m.i16(g::HEIGHT) as i32
        };
        same = h == want;
    }
    if !same {
        return fresh(m, env);
    }
    if m.u16(l.picture + 0x0a) == 0x43 {
        return Some(Opened::Finished);
    }
    call!(f::BUILD_NAME, &[l.name2, text::SUFFIX_UNFINISH]);
    call!(f::RENAME, &[l.name, l.name2]);
    call!(f::SI_PICTURE_OPEN_FILE, &[l.name2, l.file, l.picture, l.ratio, l.comment, l.channels]);
    for k in 0..4u32 {
        let width = m.i16(l.picture) as i32 as u32;
        let line = m.si_malloc(width);
        m.set_u32(l.picture + 0x0c + 4 * k, line);
    }
    call!(f::SI_RLE_INIT, &[0]);
    let mut y = (m.i16(l.picture + 2) as i32).wrapping_sub(1);
    loop {
        let (file, channels) = (m.u32(l.file), m.u32(l.channels));
        if call!(f::SI_SCANLINE_READ, &[file, channels, l.picture]) as u16 == 0 {
            break;
        }
        m.set_i16(g::OUT_LINE, y as i16);
        for (k, line) in [g::OUT_RED, g::OUT_GREEN, g::OUT_BLUE, g::OUT_ALPHA].into_iter().enumerate() {
            let (to, from, n) = (m.u32(line), m.u32(l.picture + 0x0c + 4 * k as u32), m.i16(l.picture) as i32 as u32);
            m.copy(to, from, n);
        }
        if m.i16(g::DISPLAY) != 0 {
            call!(f::SI_FB_WRITE, &[m.i16(g::OUT_LINE) as i32 as u32]);
        }
        if m.i16(g::PICTURE_FILE) != 0 {
            let p = l.picture;
            let a = [y as u32, m.i16(p) as i32 as u32, m.u32(p + 0x0c), m.u32(p + 0x10), m.u32(p + 0x14), m.u32(p + 0x18)];
            call!(f::SI_RLE_WRITE, &a);
        }
        y = y.wrapping_sub(1);
    }
    let file = m.u32(l.file);
    call!(f::FCLOSE, &[file]);
    call!(f::UNLINK, &[l.name2]);
    m.set_i32(g::RESUME_LINE, y);
    m.set_i16(g::RESUMED, 1);
    Some(Opened::Resumed)
}
