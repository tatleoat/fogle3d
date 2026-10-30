// SPDX-License-Identifier: MIT
//! `custom_udf.c`: the user data (udf) records of custom effects; finding 0041, "eighth continuation".
//! A record (`__SI_UDF`): +4 the element id, +0xc the enable flag (short), +0xe the dirty flag (short),
//! +0x10 the kind (short, 1 = internal), +0x14 a word, +0x24 the custom shader, +0x2c the dialog's data,
//! +0x30 the value list, +0x34 and +0x38 the raw text and the encoded text.

use crate::cfmt;
use crate::custom_fcv::SI_ELEMDICT_GET_ELEM;
use crate::custom_util::{SI_PROC_DATA};
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_UDF_IS_ACTIVE: Routine = Routine::new("SI_udfIsActive", 0x0854_c748);
pub const SI_UDF_SET_ACTIVE: Routine = Routine::new("SI_udfSetActive", 0x0854_c790);
pub const SI_SHADER_RESET_ACTIVE: Routine = Routine::new("SI_shaderResetActive", 0x0854_068c);
pub const SI_SHADER_SET_ACTIVE: Routine = Routine::new("SI_shaderSetActive", 0x0854_05fc);
pub const SI_SHADER_IS_ACTIVE: Routine = Routine::new("SI_shaderIsActive", 0x0854_06d4);
pub const SI_GET_MI2_OUTPUT: Routine = Routine::new("SI_getmi2Output", 0x0858_0cfc);
pub const SI_SHADER_GET_DIALOG: Routine = Routine::new("SI_shaderGetDialog", 0x0854_0718);
pub const SI_SHADER_GET_FUNC_NAME: Routine = Routine::new("SI_shaderGetFuncName", 0x0854_03b0);
pub const SI_SHADER_GET_FUNC_DECL_NAME: Routine = Routine::new("SI_shaderGetFuncDeclName", 0x0854_04f0);
pub const SI_SHADER_GET_NAME: Routine = Routine::new("SI_shaderGetName", 0x0854_02fc);
pub const SI_SHADER_USE_DEFAULT: Routine = Routine::new("SI_shaderUseDefault", 0x0854_0398);
pub const SI_UDF_PARSE_RAW_DATA: Routine = Routine::new("SI_udfParseRawData", 0x0854_5660);
pub const SI_CUS_DIALOG_HANDLE: Routine = Routine::new("SI_cusDialogHandle", 0x0852_0d18);
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: Routine = Routine::new("SI_elemdictFindElemByData", 0x0813_4364);
pub const SI_ELEMDICT_SET_DIRTY: Routine = Routine::new("SI_elemdictSetDirty", 0x0813_2be8);
pub const SI_CUS_ENCODE_DATA_STRING: Routine = Routine::new("SI_cusEncodeDataString", 0x0852_5b88);
pub const SI_FCURVE_FRAME_VALUE: Routine = Routine::new("SI_fcurveFrameValue", 0x0820_0258);
const SI_ELEM_DICTS: u32 = 0x0867_931c;

fn dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

use crate::custom_enc::st0_f32;

/// [soft 0x08544330 SI_udfInitializeFromDummy]: copies the kind (+0x10) and the word at +0x14 of `src`
/// to `dst` and gives `dst` the active state of `src`.
pub fn si_udf_initialize_from_dummy(m: &mut Mem, o: &mut dyn Outside, dst: u32, src: u32) {
    if src != 0 && dst != 0 {
        let k = m.u16(src.wrapping_add(0x10));
        m.set_u16(dst.wrapping_add(0x10), k);
        let w = m.u32(src.wrapping_add(0x14));
        m.set_u32(dst.wrapping_add(0x14), w);
        let a = o.call(m, SI_UDF_IS_ACTIVE, &[src]).eax;
        o.call(m, SI_UDF_SET_ACTIVE, &[dst, a as u16 as i16 as i32 as u32]);
    }
}

/// [soft 0x085452f4 SI_udfResetActive]: `SI_shaderResetActive`.
pub fn si_udf_reset_active(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, SI_SHADER_RESET_ACTIVE, &[]);
}

/// [soft 0x08545304 SI_udfSetAsSelected]: makes the record's shader (+0x24) the active one.
pub fn si_udf_set_as_selected(m: &mut Mem, o: &mut dyn Outside, u: u32) {
    if u != 0 {
        let sh = m.u32(u.wrapping_add(0x24));
        if sh != 0 {
            o.call(m, SI_SHADER_SET_ACTIVE, &[sh, 1]);
        }
    }
}

/// [soft 0x08545328 SI_udfIsEnabled]: 1 when the record is enabled (+0xc) and its shader active, and the
/// shader's output flags (+0x10 of the shader: 0x40 for mental ray output, 0x20 for the other) allow
/// the current output; 0 for a null record or shader.
pub fn si_udf_is_enabled(m: &mut Mem, o: &mut dyn Outside, u: u32) -> i32 {
    if u == 0 {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    if sh == 0 {
        return 0;
    }
    let mut r: i16 = 0;
    if m.u16(u.wrapping_add(0xc)) != 0 {
        r = (o.call(m, SI_SHADER_IS_ACTIVE, &[sh]).eax as u16 != 0) as i16;
    }
    let mi2 = o.call(m, SI_GET_MI2_OUTPUT, &[]).eax as u16;
    let flags = m.u8(m.u32(u.wrapping_add(0x24)).wrapping_add(0x10));
    let mut deny = mi2 != 0 && flags & 0x40 == 0;
    if !deny {
        let again = o.call(m, SI_GET_MI2_OUTPUT, &[]).eax as u16;
        deny = again == 0 && flags & 0x20 == 0;
    }
    if deny {
        r = 0;
    }
    r as i32
}

/// The shared head of the three name getters: 0 for a null record, the internal kind (1), or a record
/// without a shader whose (null) shader has no dialog.
fn udf_shader_ready(m: &mut Mem, o: &mut dyn Outside, u: u32) -> bool {
    if u == 0 || m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        return false;
    }
    if m.u32(u.wrapping_add(0x24)) == 0 && o.call(m, SI_SHADER_GET_DIALOG, &[0]).eax == 0 {
        return false;
    }
    true
}

/// [soft 0x08545398 SI_udfGetShaderName]: `SI_shaderGetFuncName` of the record's shader, 0 when it has none.
pub fn si_udf_get_shader_name(m: &mut Mem, o: &mut dyn Outside, u: u32, buf: u32, size: i32) -> u32 {
    if !udf_shader_ready(m, o, u) {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    o.call(m, SI_SHADER_GET_FUNC_NAME, &[sh, buf, size as u32]).eax
}

/// [soft 0x085453e4 SI_udfGetShaderFuncDeclName]: `SI_shaderGetFuncDeclName` of the record's shader.
pub fn si_udf_get_shader_func_decl_name(m: &mut Mem, o: &mut dyn Outside, u: u32, buf: u32, size: i32) -> u32 {
    if !udf_shader_ready(m, o, u) {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    o.call(m, SI_SHADER_GET_FUNC_DECL_NAME, &[sh, buf, size as u32]).eax
}

/// [soft 0x08545430 SI_udfGetDescName]: `SI_shaderGetName` of the record's shader (a null buffer or a
/// size below 1 gives 0).
pub fn si_udf_get_desc_name(m: &mut Mem, o: &mut dyn Outside, u: u32, buf: u32, size: i32) -> u32 {
    if u == 0 || buf == 0 || size < 1 || !udf_shader_ready(m, o, u) {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    o.call(m, SI_SHADER_GET_NAME, &[sh, buf, size as u32]).eax
}

/// [soft 0x08545488 SI_udfUseDefault]: 0 without a record, shader or for the internal kind; 1 when the
/// shader has no dialog; else `SI_shaderUseDefault` (a short).
pub fn si_udf_use_default(m: &mut Mem, o: &mut dyn Outside, u: u32) -> i32 {
    if u == 0 || m.u32(u.wrapping_add(0x24)) == 0 || m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    if o.call(m, SI_SHADER_GET_DIALOG, &[sh]).eax == 0 {
        return 1;
    }
    o.call(m, SI_SHADER_USE_DEFAULT, &[sh]).eax as u16 as i16 as i32
}

/// [soft 0x085455e4 setElementDirty]: stores the dirty flag (+0xe) and passes it to the element holding the record.
pub fn set_element_dirty(m: &mut Mem, o: &mut dyn Outside, u: u32, flag: i16) {
    m.set_u16(u.wrapping_add(0xe), flag as u16);
    let d = dict(m);
    let id = m.u32(u.wrapping_add(4));
    let i = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, id, u]).eax as i32;
    if i >= 0 {
        let d = dict(m);
        let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, id, i as u32]).eax;
        o.call(m, SI_ELEMDICT_SET_DIRTY, &[e, flag as i32 as u32]);
    }
}

/// [soft 0x085454d4 SI_udfEditParms]: opens the shader's dialog on the record (its data swapped in at
/// +0x54 of the dialog for the call); an accepted dialog marks the record dirty and drops the raw and
/// encoded texts. 1, or 0 for a null or internal record or one without a shader dialog.
pub fn si_udf_edit_parms(m: &mut Mem, o: &mut dyn Outside, u: u32) -> i32 {
    if u == 0 || m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        return 0;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    if sh == 0 {
        return 0;
    }
    let dlg = o.call(m, SI_SHADER_GET_DIALOG, &[sh]).eax;
    if dlg == 0 {
        return 0;
    }
    o.call(m, SI_UDF_PARSE_RAW_DATA, &[u]);
    let saved = m.u32(dlg.wrapping_add(0x54));
    let d = m.u32(u.wrapping_add(0x2c));
    m.set_u32(dlg.wrapping_add(0x54), d);
    let r = o.call(m, SI_CUS_DIALOG_HANDLE, &[dlg, 1, u, 0]).eax as i32;
    if r > 0 {
        set_element_dirty(m, o, u, 1);
        for off in [0x34u32, 0x38] {
            let p = m.u32(u.wrapping_add(off));
            if p != 0 {
                m.free(p);
                m.set_u32(u.wrapping_add(off), 0);
            }
        }
    }
    let d = m.u32(dlg.wrapping_add(0x54));
    m.set_u32(u.wrapping_add(0x2c), d);
    m.set_u32(dlg.wrapping_add(0x54), saved);
    1
}

/// [soft 0x08545710 udfEncodeRawData]: 1 when the encoded text (+0x38) exists; else it is made from the
/// value list (+0x30) through the shader's dialog (the data swapped in as above) and 1 is returned,
/// 0 when there is no shader or dialog.
pub fn udf_encode_raw_data(m: &mut Mem, o: &mut dyn Outside, u: u32) -> i32 {
    if m.u32(u.wrapping_add(0x38)) != 0 {
        return 1;
    }
    let sh = m.u32(u.wrapping_add(0x24));
    if sh == 0 {
        return 0;
    }
    let dlg = o.call(m, SI_SHADER_GET_DIALOG, &[sh]).eax;
    if dlg == 0 {
        return 0;
    }
    let saved = m.u32(dlg.wrapping_add(0x54));
    let d = m.u32(u.wrapping_add(0x2c));
    m.set_u32(dlg.wrapping_add(0x54), d);
    let vals = m.u32(u.wrapping_add(0x30));
    let s = o.call(m, SI_CUS_ENCODE_DATA_STRING, &[dlg, 0, vals]).eax;
    m.set_u32(u.wrapping_add(0x38), s);
    let d = m.u32(dlg.wrapping_add(0x54));
    m.set_u32(u.wrapping_add(0x2c), d);
    m.set_u32(dlg.wrapping_add(0x54), saved);
    1
}

/// [soft 0x08545780 truncateCustomValueToken]: copies up to `n` characters of the value's name (+0) to
/// `dst`, ends it at `n`, and cuts it at the first '.'.
pub fn truncate_custom_value_token(m: &mut Mem, v: u32, dst: u32, n: i32) {
    let name = m.u32(v);
    clib::strncpy(m, dst, name, n as u32);
    m.set_u8(dst.wrapping_add(n as u32), 0);
    let p = clib::strchr(m, dst, b'.');
    if p != 0 {
        m.set_u8(p, 0);
    }
}

/// [soft 0x085457c0 is_valid_token]: the kind (+4, unsigned short) is below 5.
pub fn is_valid_token(m: &Mem, v: u32) -> i32 {
    (m.u16(v.wrapping_add(4)) < 5) as i32
}

/// [soft 0x08545ec0 formatCustomValue]: the text of a value for the mental ray output. Kind 0: its string
/// (+0x18), 1 if it has one; 1: "%f" of the float (+0x10), first re-evaluated at the frame from the
/// f-curve (+0x2c) when that has keys; 2: "%d" of +0x14; 3: "on"/"off" by +0xc; 4: the four floats
/// (+0x1c) re-evaluated from the four f-curves, no text. The frame is truncated to an integer (`fistp`, RC = 11; a
/// value out of range gives 0x80000000). 0 for other kinds. The f-curve value is the callee's st0, stored as a float.
pub fn format_custom_value(m: &mut Mem, o: &mut dyn Outside, v: u32, frame: f32, dst: u32) -> i32 {
    let trunc = |f: f32| si_x87::F80::from_f32(f).to_i32(si_x87::Round::Zero);
    match m.u16(v.wrapping_add(4)) as i16 {
        0 => {
            let s = m.u32(v.wrapping_add(0x18));
            if s == 0 {
                return 0;
            }
            clib::strcpy(m, dst, s);
            1
        }
        1 => {
            let fc = m.u32(v.wrapping_add(0x2c));
            if fc != 0 && (m.u32(fc.wrapping_add(0xc)) as i32) > 0 {
                let r = o.call(m, SI_FCURVE_FRAME_VALUE, &[fc, trunc(frame) as u32]);
                m.set_f32(v.wrapping_add(0x10), r.st0.map(st0_f32).unwrap_or(f32::NAN));
            }
            let d = m.f32(v.wrapping_add(0x10)) as f64;
            let b = d.to_bits();
            cfmt::sprintf(m, dst, 0x086e_4066, &[b as u32, (b >> 32) as u32]);
            1
        }
        2 => {
            let w = m.u32(v.wrapping_add(0x14));
            cfmt::sprintf(m, dst, 0x086e_4069, &[w]);
            1
        }
        3 => {
            let s = if m.u16(v.wrapping_add(0xc)) != 0 { 0x086e_406c } else { 0x086e_406f };
            cfmt::sprintf(m, dst, 0x086e_4073, &[s]);
            1
        }
        4 => {
            for i in 0..4u32 {
                let fc = m.u32(v.wrapping_add(0x2c).wrapping_add(i * 4));
                if fc != 0 && (m.u32(fc.wrapping_add(0xc)) as i32) > 0 {
                    let r = o.call(m, SI_FCURVE_FRAME_VALUE, &[fc, trunc(frame) as u32]);
                    m.set_f32(v.wrapping_add(0x1c).wrapping_add(i * 4), r.st0.map(st0_f32).unwrap_or(f32::NAN));
                }
            }
            1
        }
        _ => 0,
    }
}

pub const SI_SHADER_GENERATE_DECLARES: Routine = Routine::new("SI_shaderGenerateDeclares", 0x0854_271c);
pub const SI_SHADER_GENERATE_INCLUDES: Routine = Routine::new("SI_shaderGenerateIncludes", 0x0854_2798);
pub const SI_SHADER_GENERATE_CODES: Routine = Routine::new("SI_shaderGenerateCodes", 0x0854_2830);
pub const SI_SHADER_GENERATE_LINKS: Routine = Routine::new("SI_shaderGenerateLinks", 0x0854_2a88);
pub const SI_STR_APPEND: Routine = Routine::new("SI_strAppend", 0x083c_f280);
pub const SI_IS_IGLOO: Routine = Routine::new("si_IsIGLOO", 0x080d_3738);
pub const SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY: Routine = Routine::new("SI_statusDisplayMessageToFileOnly", 0x083c_cf40);
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
const UDF_LIST_POINTER: u32 = 0x086e_38ac;
const SI_UDF_LIST: u32 = 0x086e_49d4;
const SI_NUM_UDF: u32 = 0x086e_49d8;

/// [soft 0x08545fe8 SI_udfGenerateDeclares]: `SI_shaderGenerateDeclares` (a short, sign-extended).
pub fn si_udf_generate_declares(m: &mut Mem, o: &mut dyn Outside, fp: u32) -> i32 {
    o.call(m, SI_SHADER_GENERATE_DECLARES, &[fp]).eax as u16 as i16 as i32
}

/// [soft 0x08546000 SI_udfGenerateIncludes]: `SI_shaderGenerateIncludes`.
pub fn si_udf_generate_includes(m: &mut Mem, o: &mut dyn Outside, fp: u32) -> i32 {
    o.call(m, SI_SHADER_GENERATE_INCLUDES, &[fp]).eax as u16 as i16 as i32
}

/// [soft 0x08546018 SI_udfGenerateCodes]: `SI_shaderGenerateCodes(0)`.
pub fn si_udf_generate_codes(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    o.call(m, SI_SHADER_GENERATE_CODES, &[0]).eax as u16 as i16 as i32
}

/// [soft 0x08546030 SI_udfGenerateLinks]: `SI_shaderGenerateLinks(0)`.
pub fn si_udf_generate_links(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    o.call(m, SI_SHADER_GENERATE_LINKS, &[0]).eax as u16 as i16 as i32
}

/// [soft 0x085479fc SI_udfListPointerUpdate]: when the list pointer exists, stores the list and its count in the
/// entry of the current process (8 bytes per process).
pub fn si_udf_list_pointer_update(m: &mut Mem) {
    let lp = m.u32(UDF_LIST_POINTER);
    if lp != 0 {
        let idx = m.u32(m.u32(SI_PROC_DATA));
        let at = lp.wrapping_add(idx.wrapping_mul(8));
        let (l, n) = (m.u32(SI_UDF_LIST), m.u32(SI_NUM_UDF));
        m.set_u32(at, l);
        m.set_u32(at.wrapping_add(4), n);
    }
}

/// [soft 0x0854761c printUDFArrayWarning]: shows "Warning: array <NAME> of shader <SHADER-RECORD> is empty." for
/// the record (as a message, or to the file only for the batch renderer).
pub fn print_udf_array_warning(m: &mut Mem, o: &mut dyn Outside, dict: u32, u: u32, name: u32) {
    let text = m.local(4);
    m.set_u32(text, 0);
    let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[dict, m.u32(u.wrapping_add(4)), m.u32(u.wrapping_add(8))]).eax;
    o.call(m, SI_STR_APPEND, &[text, 0x086e_44dc]);
    o.call(m, SI_STR_APPEND, &[text, name]);
    o.call(m, SI_STR_APPEND, &[text, 0x086e_44ed]);
    if e != 0 && m.u32(e) != 0 {
        let p = m.u32(m.u32(e).wrapping_add(0x10));
        if p != 0 {
            o.call(m, SI_STR_APPEND, &[text, p]);
            o.call(m, SI_STR_APPEND, &[text, 0x086e_44fb]);
        }
    }
    let n = m.u32(u);
    o.call(m, SI_STR_APPEND, &[text, n]);
    o.call(m, SI_STR_APPEND, &[text, 0x086e_44fd]);
    let msg = m.u32(text);
    if o.call(m, SI_IS_IGLOO, &[]).eax as u16 == 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 2]);
    } else {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, &[msg, 2]);
    }
    let p = m.u32(text);
    if p != 0 {
        m.free(p);
    }
    m.release(4);
}

pub const PLT_SSCANF: Routine = Routine::new("plt:sscanf", 0x080c_4c28);
pub const SI_SPLIT_ELEMENT_NAME: Routine = Routine::new("SI_splitElementName", 0x083c_f39c);
pub const SI_ELEMDICT_FIND_ELEM_BY_PRJ_NAME: Routine = Routine::new("SI_elemdictFindElemByPrjName", 0x0813_4678);
pub const SI_ELEMDICT_FIND_RELATED_ELEM: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);
const CID_LIGHTS: u32 = 0x0878_1678;
const CID_GLOBAL: u32 = 0x0878_1718;

/// SI_collapseSpaceSeparatedString (0x083cf44c): a new copy of the text with each run of spaces between
/// tokens made one space (an empty token after a trailing space still appends a separator); null for null
/// or empty input.
pub fn si_collapse_space_separated(m: &mut Mem, o: &mut dyn Outside, s: u32) -> u32 {
    let tmp = m.local(4);
    m.set_u32(tmp, 0);
    let mut p = s;
    if s != 0 && m.u8(s) != 0 {
        loop {
            while m.u8(p) == b' ' {
                p = p.wrapping_add(1);
            }
            let mut e = p;
            while m.u8(e) != 0 && m.u8(e) != b' ' {
                e = e.wrapping_add(1);
            }
            let c = m.u8(e);
            m.set_u8(e, 0);
            if m.u32(tmp) != 0 {
                o.call(m, SI_STR_APPEND, &[tmp, 0x086c_aac4]);
            }
            o.call(m, SI_STR_APPEND, &[tmp, p]);
            m.set_u8(e, c);
            p = e;
            if c == 0 {
                break;
            }
        }
    }
    let r = m.u32(tmp);
    m.release(4);
    r
}

/// The element table of class `cls` in dictionary `d`: its record array is read at +8 of the 0x40-byte slot.
fn class_records(m: &Mem, d: u32, cls: u32) -> u32 {
    m.u32(m.u32(d.wrapping_add(4)).wrapping_add(cls.wrapping_mul(0x40)).wrapping_add(8))
}

/// [soft 0x085465bc SI_getElemId]: the id of the element named `name` ("project:model"-split) in class `cls`; for
/// the global class an element that is a light proxy (flag at +0x54 of its 0x84 record) maps to its light's id.
/// -1 for a null name.
pub fn si_get_elem_id(m: &mut Mem, o: &mut dyn Outside, name: u32, d: u32, cls: u32) -> i32 {
    if name == 0 {
        return -1;
    }
    let ab = m.local(8);
    o.call(m, SI_SPLIT_ELEMENT_NAME, &[name, ab, ab.wrapping_add(4)]);
    let (a, b) = (m.u32(ab), m.u32(ab.wrapping_add(4)));
    let mut id = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_PRJ_NAME, &[d, cls, 0, a, b, 0, u32::MAX, 0]).eax as i32;
    if a != 0 {
        m.free(a);
        m.set_u32(ab, 0);
    }
    if b != 0 {
        m.free(b);
        m.set_u32(ab.wrapping_add(4), 0);
    }
    m.release(8);
    if id > 0 && cls == m.u32(CID_GLOBAL) {
        let rec = class_records(m, d, cls).wrapping_add(0x54).wrapping_add((id as u32).wrapping_mul(0x84));
        if m.u32(rec) != 0 {
            let lights = m.u32(CID_LIGHTS);
            let le = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, lights, id as u32]).eax;
            let tmp = m.local(8);
            m.set_u32(tmp.wrapping_add(4), id as u32);
            o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[d, le, 1, 1, 0x83e, lights, tmp, tmp.wrapping_add(4)]);
            id = m.u32(tmp.wrapping_add(4)) as i32;
            m.release(8);
        }
    }
    id
}

/// [soft 0x085466a4 SI_verifyDatabaseElementName]: 1 when the name is non-null and exists in the class.
pub fn si_verify_database_element_name(m: &mut Mem, o: &mut dyn Outside, name: u32, d: u32, cls: u32) -> i32 {
    (name != 0 && si_get_elem_id(m, o, name, d, cls) >= 0) as i32
}

/// [soft 0x08547834 SI_verifyElemVisible]: 1 when the element exists and its visible flag (+0x20 of its record) is set.
pub fn si_verify_elem_visible(m: &mut Mem, o: &mut dyn Outside, name: u32, d: u32, cls: u32) -> i32 {
    if name == 0 {
        return 0;
    }
    let id = si_get_elem_id(m, o, name, d, cls);
    if id < 0 {
        return 0;
    }
    let rec = class_records(m, d, cls).wrapping_add(0x20).wrapping_add((id as u32).wrapping_mul(0x84));
    (m.u32(rec) != 0) as i32
}

/// [soft 0x08546750 verifyElementArray]: checks each ' ' or ',' separated name of `s`; a bad name is reported
/// (message, or file only for the batch renderer). 1 when at least one name exists and none was bad, else 0.
pub fn verify_element_array(m: &mut Mem, o: &mut dyn Outside, s: u32, d: u32, cls: u32) -> i32 {
    if s == 0 {
        return 0;
    }
    let mut none_ok = true;
    let mut bad = false;
    let mut p = s;
    while m.u8(p) != 0 {
        let mut e = p;
        loop {
            let c = m.u8(e);
            if c == 0 || c == b' ' || c == b',' {
                break;
            }
            e = e.wrapping_add(1);
        }
        let saved = m.u8(e);
        m.set_u8(e, 0);
        if si_verify_database_element_name(m, o, p, d, cls) != 0 {
            none_ok = false;
        } else {
            let fmt = m.u32(0x086e_4288);
            let n = clib::strlen(m, p).wrapping_add(clib::strlen(m, fmt)).wrapping_add(1);
            let buf = m.si_malloc(n);
            let text = cfmt::format(m, fmt, &[p]);
            m.put_cstr(buf, &text);
            if o.call(m, SI_IS_IGLOO, &[]).eax as u16 == 0 {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
            } else {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, &[buf, 3]);
            }
            m.free(buf);
            bad = true;
        }
        m.set_u8(e, saved);
        if saved == 0 {
            break;
        }
        p = e.wrapping_add(1);
    }
    if none_ok { 0 } else { (!bad) as i32 }
}

/// [soft 0x08547250 extractArrayId]: the id prefix of an array value: skips spaces; "!N..." stores N (sscanf "%d")
/// and answers the position of the first ',' after it (or the end / a space); otherwise stores 1 and answers
/// the text after the spaces. 0 for a null string or id pointer.
pub fn extract_array_id(m: &mut Mem, o: &mut dyn Outside, s: u32, id: u32) -> u32 {
    if id == 0 {
        return 0;
    }
    m.set_u32(id, 1);
    if s == 0 {
        return 0;
    }
    let mut p = s;
    while m.u8(p) == b' ' {
        p = p.wrapping_add(1);
    }
    if m.u8(p) != b'!' {
        return p;
    }
    p = p.wrapping_add(1);
    o.call(m, PLT_SSCANF, &[p, 0x086e_44c1, id]);
    loop {
        let c = m.u8(p);
        if c == 0 || c == b' ' || c == b',' {
            return p;
        }
        p = p.wrapping_add(1);
    }
}

const SI_VERIFY_ELEM_VISIBLE: u32 = 0x0854_7834;

/// [soft 0x08547130 outputArrayListWarnings]: for every token of `list` (a ' ' or ',' separated list; a '#' token
/// ends it) writes the two warning lines on stderr: array `name` of shader `<class>-<record name>` and that
/// the token is not `kind` ("visible"), so it is not passed on in the shader call.
pub fn output_array_list_warnings(m: &mut Mem, o: &mut dyn Outside, d: u32, u: u32, name: u32, list: u32, kind: u32) {
    if list == 0 {
        return;
    }
    let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, m.u32(u.wrapping_add(4)), m.u32(u.wrapping_add(8))]).eax;
    let mut cls = 0;
    if e != 0 && m.u32(e) != 0 {
        cls = m.u32(m.u32(e).wrapping_add(0x10));
    }
    let mut s = list;
    loop {
        let c = m.u8(s);
        if c == 0 {
            return;
        }
        if c == b' ' || c == b',' {
            s = s.wrapping_add(1);
            continue;
        }
        if c == b'#' {
            return;
        }
        let mut t = s;
        loop {
            let c = m.u8(t);
            if c == 0 || c == b' ' || c == b',' {
                break;
            }
            t = t.wrapping_add(1);
        }
        let saved = m.u8(t);
        m.set_u8(t, 0);
        let err = m.u32(crate::STDERR);
        let (a, b) = if cls != 0 { (cls, 0x086e_443a) } else { (0x086e_443c, 0x086e_4439) };
        o.call(m, crate::custom_init::PLT_FPRINTF, &[err, 0x086e_4440, name, a, b, m.u32(u)]);
        o.call(m, crate::custom_init::PLT_FPRINTF, &[err, 0x086e_4480, s, kind]);
        m.set_u8(t, saved);
        s = t;
        if saved == 0 {
            return;
        }
        if saved == b' ' || saved == b',' {
            loop {
                s = s.wrapping_add(1);
                let c = m.u8(s);
                if c == 0 {
                    return;
                }
                if c != b' ' && c != b',' {
                    break;
                }
            }
        }
    }
}

/// Calls the token callback `cb` (an address: `SI_verifyElemVisible` runs here, any other one outside).
fn call_token_cb(m: &mut Mem, o: &mut dyn Outside, cb: u32, tok: u32, d: u32, cls: u32) -> bool {
    if cb == SI_VERIFY_ELEM_VISIBLE {
        si_verify_elem_visible(m, o, tok, d, cls) as u16 != 0
    } else {
        o.call(m, Routine::new("filterCallback", cb), &[tok, d, cls]).eax as u16 != 0
    }
}

/// [soft 0x08547714 SI_filterElemTokens]: in the list `*pp` keeps the tokens for which `cb` answers true (`keep`
/// == 0) or the ones for which it answers false (`keep` != 0), the others are overwritten by spaces; then
/// `*pp` becomes the space-collapsed copy and the old text is freed.
pub fn si_filter_elem_tokens(m: &mut Mem, o: &mut dyn Outside, pp: u32, d: u32, cls: u32, keep: i16, cb: u32) {
    if pp == 0 || m.u32(pp) == 0 {
        return;
    }
    let mut p = m.u32(pp);
    while m.u8(p) != 0 {
        while m.u8(p) == b' ' || m.u8(p) == b',' {
            p = p.wrapping_add(1);
        }
        let mut e = p;
        loop {
            let c = m.u8(e);
            if c == 0 || c == b' ' || c == b',' {
                break;
            }
            e = e.wrapping_add(1);
        }
        let saved = m.u8(e);
        m.set_u8(e, 0);
        let r = call_token_cb(m, o, cb, p, d, cls);
        m.set_u8(e, saved);
        if r == (keep != 0) {
            while m.u8(e) == b' ' || m.u8(e) == b',' {
                e = e.wrapping_add(1);
            }
            while p < e {
                m.set_u8(p, b' ');
                p = p.wrapping_add(1);
            }
        }
        p = e;
    }
    let cur = m.u32(pp);
    let new = si_collapse_space_separated(m, o, cur);
    let cur = m.u32(pp);
    if cur != 0 {
        m.free(cur);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, new);
}

/// [soft 0x08547584 filterHiddenLights]: removes the names of hidden lights from `*pp`: first lists the hidden
/// ones as warnings (on a copy), then drops them.
pub fn filter_hidden_lights(m: &mut Mem, o: &mut dyn Outside, pp: u32, d: u32, u: u32, name: u32) {
    if pp == 0 || m.u32(pp) == 0 {
        return;
    }
    let cls = m.u32(CID_GLOBAL);
    let tmp = m.local(4);
    let dup = clib::strdup(m, m.u32(pp));
    m.set_u32(tmp, dup);
    si_filter_elem_tokens(m, o, tmp, d, cls, 1, SI_VERIFY_ELEM_VISIBLE);
    let t = m.u32(tmp);
    output_array_list_warnings(m, o, d, u, name, t, 0x086e_44d4);
    let t = m.u32(tmp);
    if t != 0 {
        m.free(t);
    }
    m.release(4);
    si_filter_elem_tokens(m, o, pp, d, cls, 0, SI_VERIFY_ELEM_VISIBLE);
}

/// [soft 0x08546bec writeMatrix]: the 4x4 matrix as 16 "%f" numbers, one space after each but the last of a row
/// of the last row; after each of the first three rows `suffix`.
pub fn write_matrix(m: &mut Mem, o: &mut dyn Outside, mat: u32, suffix: u32) {
    for i in 0..4u32 {
        for j in 0..4u32 {
            let v = f32::from_bits(m.u32(mat.wrapping_add((i * 4 + j) * 4))) as f64;
            let b = v.to_bits();
            crate::custom_gen::mr_printf(m, o, 0x086e_42dd, &[b as u32, (b >> 32) as u32]);
            if !(i > 2 && j > 2) {
                crate::custom_gen::mr_printf(m, o, 0x086e_42e0, &[]);
            }
        }
        if i <= 2 {
            crate::custom_gen::mr_printf(m, o, 0x086e_42e2, &[suffix]);
        }
    }
}

pub const SI_MODEL_COMPUTE_BOUNDING: Routine = Routine::new("SI_modelComputeBounding", 0x0846_7e70);
pub const SI_VECTOR_MULTIPLY_MATRIX: Routine = Routine::new("SI_vectorMultiplyMatrix", 0x0851_6d74);
pub const SI_MATRIX_INVERSE: Routine = Routine::new("SI_matrixInverse", 0x0851_672c);
pub const SI_MATRIX_COPY: Routine = Routine::new("SI_matrixCopy", 0x0851_6224);
pub const SI_MATRIX_MULTIPLY: Routine = Routine::new("SI_matrixMultiply", 0x0851_6244);
pub const SI_MATRIX_X_ROTATION: Routine = Routine::new("SI_matrixXRotation", 0x0851_7080);
pub const SI_MATRIX_Y_ROTATION: Routine = Routine::new("SI_matrixYRotation", 0x0851_70e0);
pub const SI_GET_RENDER_CAMERA: Routine = Routine::new("SI_getRenderCamera", 0x082c_b590);
pub const SI_ED_GET_ELEM_FROM_MODEL_U: Routine = Routine::new("SI_edGetElemFromModel", 0x080f_cf20);
pub const SI_MI2_GET_MODEL_NAME: Routine = Routine::new("SI_mi2GetModelName", 0x0859_7994);
const SI_IDENTITE: u32 = 0x086d_c160;
const CID_MODELS: u32 = 0x0878_1720;

fn pf(m: &mut Mem, o: &mut dyn Outside, fmt: u32, args: &[u32]) {
    crate::custom_gen::mr_printf(m, o, fmt, args);
}

fn d3(v: [f64; 3]) -> [u32; 6] {
    let mut r = [0u32; 6];
    for (i, x) in v.iter().enumerate() {
        let b = x.to_bits();
        r[i * 2] = b as u32;
        r[i * 2 + 1] = (b >> 32) as u32;
    }
    r
}

fn f32_at(m: &Mem, a: u32) -> f32 {
    f32::from_bits(m.u32(a))
}

/// [soft 0x08546c80 writeModelArrayElement]: writes one attribute of a model for the mental ray text, chosen by
/// `kind` (name, pos, bound, size, rot, transform, inv_transform, tag, ref); `first` != 0 for the first
/// attribute of a model (no ",\n\t\t" before it). Answers 0 once something was written, otherwise `first`
/// (also for an unknown kind, which is reported on stderr).
pub fn write_model_array_element(m: &mut Mem, o: &mut dyn Outside, kind: u32, token: u32, model: u32, d: u32, elem: u32, first: i16) -> i32 {
    let k = m.cstr(kind);
    let sep = |m: &mut Mem, o: &mut dyn Outside, a: u32| {
        if first == 0 {
            pf(m, o, a, &[]);
        }
    };
    let xf = m.u32(model.wrapping_add(0x44));
    match &k[..] {
        b"name" => {
            sep(m, o, 0x086e_42ea);
            let mut n = m.u32(model.wrapping_add(4));
            if n == 0 {
                n = 0x086e_42ef;
            }
            pf(m, o, 0x086e_42f9, &[n]);
            0
        }
        b"pos" => {
            sep(m, o, 0x086e_430a);
            let v = [f32_at(m, xf + 0x1f4) as f64, f32_at(m, xf + 0x1f8) as f64, f32_at(m, xf + 0x1fc) as f64];
            pf(m, o, 0x086e_430f, &d3(v));
            0
        }
        b"bound" => {
            o.call(m, SI_MODEL_COMPUTE_BOUNDING, &[model]);
            let b = m.local(24);
            o.call(m, SI_VECTOR_MULTIPLY_MATRIX, &[model + 0x10, xf + 0x1c4, b, 1]);
            o.call(m, SI_VECTOR_MULTIPLY_MATRIX, &[model + 0x1c, xf + 0x1c4, b + 12, 1]);
            sep(m, o, 0x086e_4325);
            let v = [f32_at(m, b) as f64, f32_at(m, b + 4) as f64, f32_at(m, b + 8) as f64];
            pf(m, o, 0x086e_432a, &d3(v));
            let v = [f32_at(m, b + 12) as f64, f32_at(m, b + 16) as f64, f32_at(m, b + 20) as f64];
            pf(m, o, 0x086e_4341, &d3(v));
            m.release(24);
            0
        }
        b"size" => {
            o.call(m, SI_MODEL_COMPUTE_BOUNDING, &[model]);
            sep(m, o, 0x086e_435c);
            let one = |m: &Mem, hi: u32, lo: u32, s: u32| {
                ((f32_at(m, model + hi) as f64 - f32_at(m, model + lo) as f64) * f32_at(m, xf + s) as f64).abs()
            };
            let v = [one(m, 0x1c, 0x10, 0x16c), one(m, 0x20, 0x14, 0x170), one(m, 0x24, 0x18, 0x174)];
            pf(m, o, 0x086e_4361, &d3(v));
            0
        }
        b"rot" => {
            let cam = o.call(m, SI_GET_RENDER_CAMERA, &[0]).eax;
            let r = m.local(64);
            let a = m.local(64);
            o.call(m, SI_MATRIX_COPY, &[SI_IDENTITE, a]);
            let ty = if cam != 0 { m.u16(cam.wrapping_add(4)) } else { 0 };
            let half_pi = 0x3fc9_0fdb_u32; // pi/2 as a float
            if ty == 0x7d1 {
                o.call(m, SI_MATRIX_X_ROTATION, &[half_pi, a]);
            } else if ty == 0x7d2 {
                o.call(m, SI_MATRIX_Y_ROTATION, &[half_pi, a]);
            } else {
                o.call(m, SI_MATRIX_COPY, &[SI_IDENTITE, a]);
            }
            o.call(m, SI_MATRIX_MULTIPLY, &[a, xf + 0x178, r]);
            sep(m, o, 0x086e_4376);
            pf(m, o, 0x086e_437b, &[]);
            write_matrix(m, o, r, 0x086e_4383);
            m.release(128);
            0
        }
        b"transform" => {
            sep(m, o, 0x086e_4392);
            pf(m, o, 0x086e_4397, &[]);
            write_matrix(m, o, xf + 0x1c4, 0x086e_43a5);
            0
        }
        b"inv_transform" => {
            sep(m, o, 0x086e_43b8);
            pf(m, o, 0x086e_43bd, &[]);
            let inv = m.local(64);
            o.call(m, SI_MATRIX_INVERSE, &[xf + 0x1c4, inv]);
            write_matrix(m, o, inv, 0x086e_43cf);
            m.release(64);
            0
        }
        b"tag" => {
            let t = m.local(4);
            o.call(m, SI_ED_GET_ELEM_FROM_MODEL_U, &[model, t]);
            sep(m, o, 0x086e_43d8);
            let v = m.u32(t).wrapping_add(1);
            pf(m, o, 0x086e_43dd, &[v]);
            m.release(4);
            0
        }
        b"ref" => {
            if o.call(m, SI_GET_MI2_OUTPUT, &[]).eax as u16 != 0 {
                let buf = m.local(500);
                o.call(m, SI_MI2_GET_MODEL_NAME, &[d, elem, buf, 500]);
                sep(m, o, 0x086e_43eb);
                pf(m, o, 0x086e_43f0, &[buf]);
                m.release(500);
                0
            } else {
                first as i32
            }
        }
        _ => {
            let err = m.u32(crate::STDERR);
            o.call(m, crate::custom_init::PLT_FPRINTF, &[err, 0x086e_4400, kind, token]);
            first as i32
        }
    }
}

/// [soft 0x085468e4 writeLightArray]: `"name" [ "LIG_light", ... ]` for the light names of `names`, written as a
/// mental ray array member called `member`. 0 for missing input or when no name of the list is valid.
pub fn write_light_array(m: &mut Mem, o: &mut dyn Outside, d: u32, names: u32, member: u32, _unused: u32, first: i16) -> i32 {
    let cls = m.u32(CID_GLOBAL);
    if names == 0 || member == 0 || verify_element_array(m, o, names, d, cls) == 0 {
        return 0;
    }
    if first == 0 {
        pf(m, o, 0x086e_428c, &[]);
    }
    pf(m, o, 0x086e_428f, &[member]);
    let mut first_name = true;
    let mut saved = 0u32;
    let mut t = clib::strtok(m, names, 0x086e_4297, &mut saved);
    while t != 0 {
        if si_verify_database_element_name(m, o, t, d, cls) != 0 {
            if !first_name {
                pf(m, o, 0x086e_429a, &[]);
            }
            let pre = if o.call(m, SI_GET_MI2_OUTPUT, &[]).eax as u16 != 0 { 0x086e_429d } else { 0x086e_42a2 };
            pf(m, o, 0x086e_42a3, &[pre, t]);
            first_name = false;
        }
        t = clib::strtok(m, 0, 0x086e_42aa, &mut saved);
    }
    pf(m, o, 0x086e_42ad, &[]);
    1
}

/// [soft 0x085469f4 writeModelArray]: `"member" [ ... ]` for the models named in `names`; with a null `attrs`
/// each model is written as `{ name }`, otherwise as `{ ... }` of the '.'-separated attributes of `attrs`
/// (writeModelArrayElement). 0 for missing input or when no name is valid.
pub fn write_model_array(m: &mut Mem, o: &mut dyn Outside, d: u32, names: u32, member: u32, attrs: u32, first: i16) -> i32 {
    let cls = m.u32(CID_MODELS);
    let mut local_6: i16 = 1;
    if names == 0 || member == 0 || verify_element_array(m, o, names, d, cls) == 0 {
        return 0;
    }
    if first == 0 {
        pf(m, o, 0x086e_42af, &[]);
    }
    pf(m, o, 0x086e_42b2, &[member]);
    let mut saved = 0u32;
    let mut t = clib::strtok(m, names, 0x086e_42ba, &mut saved);
    while t != 0 {
        if si_verify_database_element_name(m, o, t, d, cls) != 0 {
            let id = si_get_elem_id(m, o, t, d, cls);
            let e = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, cls, id as u32]).eax;
            let model = m.u32(e.wrapping_add(4));
            if attrs == 0 {
                if local_6 == 0 {
                    pf(m, o, 0x086e_42bd, &[]);
                }
                let mut n = m.u32(model.wrapping_add(4));
                if n == 0 {
                    n = 0x086e_42c0;
                }
                pf(m, o, 0x086e_42c5, &[n]);
                local_6 = 0;
            } else {
                let mut r: i16 = 1;
                if local_6 == 0 {
                    pf(m, o, 0x086e_42cc, &[]);
                }
                pf(m, o, 0x086e_42d1, &[]);
                let mut p = attrs;
                while m.u8(p) != 0 {
                    let mut q = p;
                    while m.u8(q) != 0 && m.u8(q) != b'.' {
                        q = q.wrapping_add(1);
                    }
                    let c = m.u8(q);
                    m.set_u8(q, 0);
                    r = write_model_array_element(m, o, p, member, model, d, e, r) as i16;
                    m.set_u8(q, c);
                    if c == 0 {
                        break;
                    }
                    p = q.wrapping_add(1);
                }
                if local_6 != 0 {
                    local_6 = r;
                }
                pf(m, o, 0x086e_42d5, &[]);
            }
        }
        t = clib::strtok(m, 0, 0x086e_42d8, &mut saved);
    }
    pf(m, o, 0x086e_42db, &[]);
    1
}

pub const SI_GET_RELATION_LIST: Routine = Routine::new("SI_getRelationList", 0x0810_3eb4);
pub const SI_STR_APPEND_TOKENS: Routine = Routine::new("SI_strAppendTokens", 0x083c_f85c);
pub const SI_DELETE_STRING_ELEMENTS: Routine = Routine::new("SI_deleteStringElements", 0x0852_6fec);
pub const SI_PROCESS_STRING_RELATIONS: Routine = Routine::new("SI_processStringRelations", 0x0852_6a90);

/// [soft 0x085472c8 writeArray]: writes the value of an array parameter `value` of class `cls` (models or lights)
/// as a mental ray array `member`: a value with a relation id ("!N...") is the names of the relation N plus the
/// ones in the text, otherwise the names in the text; unknown names are warned about and dropped, hidden lights
/// are dropped. Calls writeModelArray or writeLightArray; answers its (short) result, 0 for another class.
pub fn write_array(m: &mut Mem, o: &mut dyn Outside, d: u32, u: u32, cls: u32, value: u32, member: u32, attrs: u32, first: i16) -> i32 {
    let (text, light) = if cls == m.u32(CID_MODELS) {
        (0x086e_44c4, false)
    } else if cls == m.u32(CID_GLOBAL) {
        (0x086e_44cc, true)
    } else {
        return 0;
    };
    // the original sprintf's the words into a local 8-byte buffer and passes that to the warnings
    let label = m.local(8);
    cfmt::sprintf(m, label, text, &[]);
    let w = m.local(12);
    let (l24, l20, l28, id) = (w, w + 4, w + 8, m.local(4));
    m.set_u32(id, 1);
    let s = extract_array_id(m, o, value, id);
    let call = |m: &mut Mem, o: &mut dyn Outside, names: u32| -> i32 {
        let r = if light {
            write_light_array(m, o, d, names, member, attrs, first)
        } else {
            write_model_array(m, o, d, names, member, attrs, first)
        };
        r as i16 as i32
    };
    let ret;
    if (m.i32(id)) < 1 {
        let t = clib::strdup(m, s);
        m.set_u32(l24, t);
        o.call(m, SI_DELETE_STRING_ELEMENTS, &[l24, d, cls, 1]);
        let t = m.u32(l24);
        output_array_list_warnings(m, o, d, u, member, t, label);
        let t = m.u32(l24);
        if t != 0 {
            m.free(t);
            m.set_u32(l24, 0);
        }
        let t = clib::strdup(m, s);
        m.set_u32(l20, t);
        o.call(m, SI_DELETE_STRING_ELEMENTS, &[l20, d, cls, 0]);
        if light && m.u32(l20) != 0 {
            filter_hidden_lights(m, o, l20, d, u, member);
        }
        let names = m.u32(l20);
        ret = call(m, o, names);
        let t = m.u32(l20);
        if t != 0 {
            m.free(t);
        }
    } else {
        m.set_u32(l20, 0);
        m.set_u32(l24, 0);
        if s != 0 {
            let t = clib::strdup(m, s);
            m.set_u32(l20, t);
            if t == 0 {
                m.release(4);
                m.release(12);
                m.release(8);
                return 0;
            }
            o.call(m, SI_DELETE_STRING_ELEMENTS, &[l20, d, cls, 0]);
            let t = m.u32(l20);
            o.call(m, SI_PROCESS_STRING_RELATIONS, &[u, d, cls, m.u32(id), t, 1]);
            let t = clib::strdup(m, s);
            m.set_u32(l24, t);
            if t == 0 {
                let t = m.u32(l20);
                if t != 0 {
                    m.free(t);
                }
                m.release(4);
                m.release(12);
                m.release(8);
                return 0;
            }
            o.call(m, SI_DELETE_STRING_ELEMENTS, &[l24, d, cls, 1]);
            let t = m.u32(l24);
            output_array_list_warnings(m, o, d, u, member, t, label);
            let t = m.u32(l24);
            if t != 0 {
                m.free(t);
                m.set_u32(l24, 0);
            }
        }
        let lst = o.call(m, SI_GET_RELATION_LIST, &[d, u, cls, m.u32(id)]).eax;
        m.set_u32(l28, lst);
        let t = m.u32(l20);
        o.call(m, SI_STR_APPEND_TOKENS, &[l28, t]);
        let t = m.u32(l20);
        if t != 0 {
            m.free(t);
            m.set_u32(l20, 0);
        }
        if light && m.u32(l28) != 0 {
            filter_hidden_lights(m, o, l28, d, u, member);
        }
        let names = m.u32(l28);
        ret = call(m, o, names);
        let t = m.u32(l28);
        if t != 0 {
            m.free(t);
        }
    }
    m.release(4);
    m.release(12);
    m.release(8);
    ret
}

pub const PLT_ACCESS: Routine = Routine::new("plt:access", 0x080c_3f18);
pub const PLT_ERRNO: Routine = Routine::new("plt:__errno_location", 0x080c_3e38);
pub const PLT_FOPEN: Routine = Routine::new("plt:fopen", 0x080c_4e18);
pub const PLT_FCLOSE: Routine = Routine::new("plt:fclose", 0x080c_4958);
pub const PLT_FPUTS: Routine = Routine::new("plt:fputs", 0x080c_4408);
pub const SI_CHAP_INFO_FROM_CHAP_ID: Routine = Routine::new("SI_chapInfoFromChapId", 0x0812_0a18);
pub const SI_DBFILE_WRITE_ASC_HEADER: Routine = Routine::new("SI_dbfileWriteAscHeader", 0x083f_b900);
pub const SI_UDF_GET_ORDER: Routine = Routine::new("SI_udfGetOrder", 0x0854_c778);
const SI_SAVE_ELEM_ERROR_CODE: u32 = 0x086c_ebb0;

/// [soft 0x085450c4 SI_udfWrite]: writes the record to the file `path` (chapter `chap`, header version
/// `version`, a float's bits): the stored text (+0x34) or a generated header (type, order, database, shader
/// name, description, version), then the encoded values (made first when missing or when the record is dirty).
/// Error codes in `SI_saveElemErrorCode`: 4 no record, 5 no path, 8 internal record, 3 not writable, 1 no file.
pub fn si_udf_write(m: &mut Mem, o: &mut dyn Outside, u: u32, path: u32, chap: u32, version: u32) -> i32 {
    if u == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 4);
        return 0;
    }
    if path == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 5);
        return 0;
    }
    if m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 8);
        return 0;
    }
    if o.call(m, PLT_ACCESS, &[path, 2]).eax != 0 {
        let e = o.call(m, PLT_ERRNO, &[]).eax;
        if m.u32(e) != 2 {
            m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 3);
            return 0;
        }
    }
    let fp = o.call(m, PLT_FOPEN, &[path, 0x086e_3dcf]).eax;
    if fp == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 1);
        return 0;
    }
    if (m.i32(u.wrapping_add(0x14))) < 1 {
        m.set_u32(u.wrapping_add(0x14), 1);
    }
    let ci = o.call(m, SI_CHAP_INFO_FROM_CHAP_ID, &[chap]).eax;
    let cw = m.u32(ci.wrapping_add(8));
    o.call(m, SI_DBFILE_WRITE_ASC_HEADER, &[fp, version, cw]);
    let text = m.u32(u.wrapping_add(0x34));
    if text == 0 {
        let f = |m: &Mem, a: u32| m.u32(a);
        let t = f(m, 0x086e_3858);
        o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3dd4, t]);
        let ord = o.call(m, SI_UDF_GET_ORDER, &[u]).eax;
        let t = f(m, 0x086e_38a8);
        o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3dd9, t, ord]);
        let db = m.u32(u.wrapping_add(0x20));
        if db != 0 {
            let t = f(m, 0x086e_389c);
            o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3de0, t, db]);
        }
        let sh = m.u32(u.wrapping_add(0x24));
        if sh != 0 {
            let n = m.u32(sh.wrapping_add(0x1c));
            if n != 0 {
                let t = f(m, 0x086e_3870);
                o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3de9, t, n]);
            }
        }
        let ds = m.u32(u.wrapping_add(0x28));
        if ds != 0 {
            let t = f(m, 0x086e_388c);
            o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3df2, t, ds]);
        }
        let ver = m.u32(u.wrapping_add(0x1c));
        if ver != 0 {
            let t = f(m, 0x086e_387c);
            o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3dfb, t, ver]);
        }
        let t = f(m, 0x086e_3864);
        o.call(m, crate::custom_init::PLT_FPRINTF, &[fp, 0x086e_3e02, t]);
    } else {
        o.call(m, PLT_FPUTS, &[text, fp]);
        o.call(m, PLT_FPUTS, &[0x086e_3dd1, fp]);
    }
    let dirty = m.u16(u.wrapping_add(0xe)) != 0;
    if m.u32(u.wrapping_add(0x38)) == 0 || dirty {
        if dirty && m.u32(u.wrapping_add(0x2c)) != 0 {
            m.set_u32(u.wrapping_add(0x38), 0);
        }
        udf_encode_raw_data(m, o, u);
    }
    let enc = m.u32(u.wrapping_add(0x38));
    if enc != 0 {
        o.call(m, PLT_FPUTS, &[enc, fp]);
    }
    o.call(m, PLT_FCLOSE, &[fp]);
    set_element_dirty(m, o, u, 0);
    1
}

pub const SI_SHADER_DISPOSE: Routine = Routine::new("SI_shaderDispose", 0x0854_01d0);
pub const SI_SHD_CHAP_INFO_FROM_CHAP_ID: Routine = Routine::new("si_shdChapInfoFromChapId", 0x0854_c824);
pub const SI_UDF_ALLOCATE: Routine = Routine::new("SI_udfAllocate", 0x0854_c2f4);
pub const SI_UDF_DISPOSE: Routine = Routine::new("SI_udfDispose", 0x0854_c3c0);
pub const SI_UDF_INITIALIZE_UDF: Routine = Routine::new("SI_udfInitializeUdf", 0x0854_c5e4);
pub const SI_DB_DIR_SEARCH_DATABASE_FROM_PATH: Routine = Routine::new("SI_dbDirSearchDatabaseFromPath", 0x0812_18f4);
pub const SI_SHADER_FIND: Routine = Routine::new("SI_shaderFind", 0x0854_074c);
pub const SI_SHADER_ALLOCATE: Routine = Routine::new("SI_shaderAllocate", 0x0854_00f0);
pub const SI_SHADER_INITIALIZE: Routine = Routine::new("SI_shaderInitialize", 0x0854_07f4);
pub const SI_CUSTOM_VALUE_LIST_COPY: Routine = Routine::new("si_customValueListCopy", 0x0852_6488);
const SI_DBDIR: u32 = 0x0878_1390;
const SI_CUS_SHADER_DB_ROOT: u32 = 0x0868_4064;

fn cus_trace(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, crate::custom_gen::SI_CUS_TRACE, args);
}

/// [soft 0x08544370 SI_udfInitialize]: makes the record `u` the instance of the shader file `path` in chapter `chap`
/// under the name `name`. A `.udf` file of the chapter is read as a record and copied; a shader library file is
/// searched in the loaded databases (or under the environment's shader root) and its shader found or created,
/// with copies of its dialog values. 1 when done, 0 when it cannot (null input, internal record, no database,
/// no dialog).
pub fn si_udf_initialize(m: &mut Mem, o: &mut dyn Outside, u: u32, name: u32, path: u32, chap: u32) -> i32 {
    if path == 0 || u == 0 || m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        return 0;
    }
    let old = m.u32(u);
    if old != 0 {
        m.free(old);
        m.set_u32(u, 0);
    }
    let nm = clib::strdup(m, name);
    m.set_u32(u, nm);
    m.set_u32(u.wrapping_add(8), u32::MAX);
    let sh = m.u32(u.wrapping_add(0x24));
    if sh != 0 {
        o.call(m, SI_SHADER_DISPOSE, &[sh]);
    }
    let ci = o.call(m, SI_CHAP_INFO_FROM_CHAP_ID, &[chap]).eax;
    let sci = o.call(m, SI_SHD_CHAP_INFO_FROM_CHAP_ID, &[chap]).eax;
    let dot = clib::strrchr(m, path, b'.');
    let ext = dot.wrapping_add(1);
    let finish = |m: &mut Mem, o: &mut dyn Outside| -> i32 {
        let sh = m.u32(u.wrapping_add(0x24));
        let w = m.u32(m.u32(sh).wrapping_add(0xc));
        m.set_u32(u.wrapping_add(0x18), w);
        m.set_u32(u.wrapping_add(4), chap);
        set_element_dirty(m, o, u, 1);
        1
    };
    if clib::strcmp(m, ext, m.u32(ci.wrapping_add(4))) == 0 {
        cus_trace(m, o, &[0x086e_38b0]);
        let n = o.call(m, SI_UDF_ALLOCATE, &[]).eax;
        m.set_u32(n.wrapping_add(8), u32::MAX);
        if udf_read(m, o, n, path, sci) as u16 == 0 {
            o.call(m, SI_UDF_DISPOSE, &[n]);
            return 0;
        }
        o.call(m, SI_UDF_INITIALIZE_UDF, &[u, n]);
        o.call(m, SI_UDF_DISPOSE, &[n]);
        return finish(m, o);
    }
    if clib::strcmp(m, ext, m.u32(sci.wrapping_add(4))) != 0 {
        return finish(m, o);
    }
    let w = m.local(0x10 + 0x800);
    let (dbname, dbrec, extra, buf) = (w, w + 4, w + 8, w + 0x10);
    m.set_u32(dbname, 0);
    m.set_u32(dbrec, 0);
    cus_trace(m, o, &[0x086e_38e0]);
    let r = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_PATH, &[SI_DBDIR, path, 3, dbrec, extra]).eax as i32;
    if r == 100 {
        let rec = m.u32(dbrec);
        if rec == 0 || m.u32(rec.wrapping_add(0x24)) != 2 || m.u32(rec.wrapping_add(0xc)) == 0 || m.u32(rec.wrapping_add(8)) == 0 {
            cus_trace(m, o, &[0x086e_39a0]);
            cus_trace(m, o, &[0x086e_39e0]);
            m.release(0x10 + 0x800);
            return 0;
        }
        let root = m.u32(rec.wrapping_add(0xc));
        let text = cfmt::format(m, 0x086e_3970, &[root]);
        m.put_cstr(buf, &text);
        let d = clib::strdup(m, m.u32(rec.wrapping_add(8)));
        m.set_u32(dbname, d);
        cus_trace(m, o, &[0x086e_3974, d]);
    } else {
        let root = m.u32(SI_CUS_SHADER_DB_ROOT);
        if root == 0 {
            m.release(0x10 + 0x800);
            return 0;
        }
        let text = cfmt::format(m, 0x086e_391d, &[root]);
        m.put_cstr(buf, &text);
        cus_trace(m, o, &[0x086e_3921, r as u32]);
        cus_trace(m, o, &[0x086e_3940]);
    }
    let mut b = path;
    if m.u8(b) != 0 {
        let mut c = buf;
        loop {
            let (dl, al) = (m.u8(b), m.u8(c));
            if al == 0 || dl != al {
                break;
            }
            b = b.wrapping_add(1);
            c = c.wrapping_add(1);
            if m.u8(b) == 0 {
                break;
            }
        }
    }
    if m.u8(b) == 0 {
        b = path;
    }
    let dbn = m.u32(dbname);
    cus_trace(m, o, &[0x086e_3a24, if dbn == 0 { 0x086e_3a1d } else { dbn }]);
    if dbn == 0 {
        m.set_u32(u.wrapping_add(0x20), 0);
    } else {
        let d = clib::strdup(m, dbn);
        m.set_u32(u.wrapping_add(0x20), d);
    }
    let mut s = o.call(m, SI_SHADER_FIND, &[b, dbn, m.u32(sci.wrapping_add(0xc))]).eax;
    m.set_u32(u.wrapping_add(0x24), s);
    if s == 0 {
        s = o.call(m, SI_SHADER_ALLOCATE, &[]).eax;
        m.set_u32(u.wrapping_add(0x24), s);
        o.call(m, SI_SHADER_INITIALIZE, &[s, b, dbn, sci]);
    }
    if dbn != 0 {
        m.free(dbn);
    }
    m.release(0x10 + 0x800);
    let s = m.u32(u.wrapping_add(0x24));
    if o.call(m, SI_UDF_IS_ACTIVE, &[u]).eax as u16 != 0 {
        o.call(m, SI_SHADER_SET_ACTIVE, &[s, 1]);
    }
    let dlg = o.call(m, SI_SHADER_GET_DIALOG, &[s]).eax;
    if dlg == 0 {
        return 0;
    }
    let c1 = o.call(m, SI_CUSTOM_VALUE_LIST_COPY, &[m.u32(dlg.wrapping_add(0x54))]).eax;
    m.set_u32(u.wrapping_add(0x2c), c1);
    let c2 = o.call(m, SI_CUSTOM_VALUE_LIST_COPY, &[m.u32(m.u32(u.wrapping_add(0x24)).wrapping_add(0xe8))]).eax;
    m.set_u32(u.wrapping_add(0x30), c2);
    m.set_u32(u.wrapping_add(0x34), 0);
    m.set_u32(u.wrapping_add(0x38), 0);
    finish(m, o)
}

pub const PLT_FGETS: Routine = Routine::new("plt:fgets", 0x080c_4388);
pub const SI_DBFILE_READ_ASC_HEADER: Routine = Routine::new("SI_dbfileReadAscHeader", 0x083f_b788);
pub const SI_DB_DIR_SEARCH_DATABASE_FROM_NAME: Routine = Routine::new("SI_dbDirSearchDatabaseFromName", 0x0812_1744);
pub const SI_CUS_READ_DATA_SECTION_U: Routine = Routine::new("SI_cusReadDataSection", 0x0852_40f0);
pub const SI_CUSTOM_VALUE_LIST_DISPOSE: Routine = Routine::new("si_customValueListDispose", 0x0852_64d4);
const SI_CUS_SHADER_DB_ROOT_NAME: u32 = 0x0868_4068;
const TOK_WHITESPACES: u32 = 0x086e_3848;
const TOK_FILETYPE: u32 = 0x086e_3858;
const TOK_END: u32 = 0x086e_3864;
const TOK_NAME: u32 = 0x086e_3870;
const TOK_VERSION: u32 = 0x086e_387c;
const TOK_DESCRIPTION: u32 = 0x086e_388c;
const TOK_DATABASE: u32 = 0x086e_389c;
const TOK_ORDER: u32 = 0x086e_38a8;

/// [soft 0x08544724 udfRead]: reads the record file `path` into `u` (`chap` is the shader chapter info): after the
/// ascii header and the `_FILETYPE` line, the keyed lines DATABASE (moniker looked up in the loaded databases),
/// NAME (the descriptor, whose shader is found or created), ORDER (positive, else 1), DESCRIPTION, VERSION, up
/// to END, after which the data section follows (kept as the encoded text; the value lists are dropped).
/// 1 when read, 0 for missing input, an internal or unreadable file, or EOF before END.
pub fn udf_read(m: &mut Mem, o: &mut dyn Outside, u: u32, path: u32, chap: u32) -> i32 {
    let mut name_copy: u32 = 0;
    if u == 0 || path == 0 {
        return 0;
    }
    if m.u8(path) == 0 || m.u16(u.wrapping_add(0x10)) as i16 == 1 {
        return 0;
    }
    if o.call(m, PLT_ACCESS, &[path, 4]).eax != 0 {
        let msg = m.local(0x400);
        let text = cfmt::format(m, 0x086e_3a40, &[path]);
        m.put_cstr(msg, &text);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
        cus_trace(m, o, &[msg]);
        m.release(0x400);
        return 0;
    }
    let fp = o.call(m, PLT_FOPEN, &[path, 0x086e_3a67]).eax;
    if fp == 0 {
        cus_trace(m, o, &[0x086e_3a80]);
        return 0;
    }
    let hdr = m.local(16);
    o.call(m, SI_DBFILE_READ_ASC_HEADER, &[fp, hdr, 0]);
    m.release(16);
    for off in [0x28u32, 0x20] {
        let p = m.u32(u.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(u.wrapping_add(off), 0);
        }
    }
    let rootname = m.u32(SI_CUS_SHADER_DB_ROOT_NAME);
    m.set_u32(u.wrapping_add(0x20), rootname);
    let line = m.local(1000);
    let mut saved = 0u32;
    let has = |m: &Mem, line: u32, tok: u32| -> bool {
        let t = m.u32(tok);
        let n = clib::strlen(m, t);
        clib::strncmp(m, line, t, n) == 0
    };
    // lines up to _FILETYPE
    loop {
        if o.call(m, PLT_FGETS, &[line, 1000, fp]).eax == 0 {
            o.call(m, PLT_FCLOSE, &[fp]);
            cus_trace(m, o, &[0x086e_3ac0]);
            m.release(1000);
            return 0;
        }
        if has(m, line, TOK_FILETYPE) {
            break;
        }
    }
    loop {
        if o.call(m, PLT_FGETS, &[line, 1000, fp]).eax == 0 {
            cus_trace(m, o, &[0x086e_3b00]);
            o.call(m, PLT_FCLOSE, &[fp]);
            m.release(1000);
            return 0;
        }
        cus_trace(m, o, &[0x086e_3b22, line]);
        let ws = m.u32(TOK_WHITESPACES);
        let after = |m: &Mem, tok: u32| -> u32 { line.wrapping_add(clib::strlen(m, m.u32(tok))) };
        if has(m, line, TOK_END) {
            let data = o.call(m, SI_CUS_READ_DATA_SECTION_U, &[fp]).eax;
            m.set_u32(u.wrapping_add(0x38), data);
            for off in [0x2cu32, 0x30] {
                let p = m.u32(u.wrapping_add(off));
                if p != 0 {
                    o.call(m, SI_CUSTOM_VALUE_LIST_DISPOSE, &[p]);
                    m.set_u32(u.wrapping_add(off), 0);
                }
            }
            if name_copy != 0 {
                m.free(name_copy);
            }
            set_element_dirty(m, o, u, 0);
            o.call(m, PLT_FCLOSE, &[fp]);
            m.release(1000);
            return 1;
        }
        if has(m, line, TOK_DATABASE) {
            cus_trace(m, o, &[0x086e_3b31]);
            let a = after(m, TOK_DATABASE);
            let t = clib::strtok(m, a, ws, &mut saved);
            if t == 0 || m.u8(t) == b'#' {
                cus_trace(m, o, &[0x086e_3b60]);
                continue;
            }
            let t = clib::strtok(m, t, 0x086e_3b85, &mut saved);
            if t == 0 {
                cus_trace(m, o, &[0x086e_3ba0]);
                continue;
            }
            cus_trace(m, o, &[0x086e_3be0, t]);
            let d = clib::strdup(m, t);
            m.set_u32(u.wrapping_add(0x20), d);
            let r = m.local(8);
            let rr = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_NAME, &[SI_DBDIR, t, r, r + 4]).eax as i32;
            let rec = m.u32(r);
            m.release(8);
            if rr != 100 {
                cus_trace(m, o, &[0x086e_3c20]);
            } else if m.u32(rec.wrapping_add(0x24)) != 2 {
                cus_trace(m, o, &[0x086e_3c60]);
            }
            continue;
        }
        if has(m, line, TOK_NAME) {
            cus_trace(m, o, &[0x086e_3c96]);
            let a = after(m, TOK_NAME);
            let t = clib::strtok(m, a, ws, &mut saved);
            let t = if t == 0 || m.u8(t) == b'#' { 0 } else { clib::strtok(m, t, 0x086e_3caa, &mut saved) };
            if t == 0 {
                cus_trace(m, o, &[0x086e_3cc0]);
                continue;
            }
            name_copy = clib::strdup(m, t);
            cus_trace(m, o, &[0x086e_3d00, t]);
            let mut s = o.call(m, SI_SHADER_FIND, &[name_copy, m.u32(u.wrapping_add(0x20)), m.u32(chap.wrapping_add(0xc))]).eax;
            m.set_u32(u.wrapping_add(0x24), s);
            if s == 0 {
                s = o.call(m, SI_SHADER_ALLOCATE, &[]).eax;
                m.set_u32(u.wrapping_add(0x24), s);
                o.call(m, SI_SHADER_INITIALIZE, &[s, name_copy, m.u32(u.wrapping_add(0x20)), chap]);
            }
            continue;
        }
        if has(m, line, TOK_ORDER) {
            cus_trace(m, o, &[0x086e_3d2a]);
            let a = after(m, TOK_ORDER);
            let t = clib::strtok(m, a, ws, &mut saved);
            let mut v: i32 = -1;
            if t != 0 && m.u8(t) != b'#' {
                v = clib::strtol(m, t, 0, 10);
            }
            if t == 0 || m.u8(t) == b'#' || v <= 0 {
                cus_trace(m, o, &[0x086e_3d40]);
                v = 1;
            }
            m.set_u32(u.wrapping_add(0x14), v as u32);
            cus_trace(m, o, &[0x086e_3d60, v as u32]);
            continue;
        }
        if has(m, line, TOK_DESCRIPTION) {
            cus_trace(m, o, &[0x086e_3d71]);
            let a = after(m, TOK_DESCRIPTION);
            let sp = clib::strspn(m, a, ws);
            let mut b = a.wrapping_add(sp);
            if m.u8(b) == b'"' {
                b = b.wrapping_add(1);
                let q = clib::strrchr(m, b, b'"');
                if q != 0 {
                    m.set_u8(q, 0);
                }
            } else {
                b = clib::strtok(m, a, ws, &mut saved);
                if b == 0 {
                    cus_trace(m, o, &[0x086e_3d8c]);
                    m.set_u32(u.wrapping_add(0x28), 0);
                    continue;
                }
            }
            let d = clib::strdup(m, b);
            m.set_u32(u.wrapping_add(0x28), d);
            cus_trace(m, o, &[0x086e_3da6, b]);
            continue;
        }
        if has(m, line, TOK_VERSION) {
            cus_trace(m, o, &[0x086e_3db8]);
        }
    }
}

pub const SI_EMPTY_STRING: Routine = Routine::new("SI_emptyString", 0x082c_b774);
pub const PLT_PRINTF: Routine = Routine::new("plt:printf", 0x080c_48b8);

/// [soft 0x085457dc SI_udfOutputToMI]: writes every valid value of the record (the dialog values +0x2c, then the
/// default values +0x30 when the first list is empty or ended) as a member of the mental ray `parameter`
/// block, by the symbol type of its shader token: scalars and colors, "on/off", strings, RGBA, vector/color
/// runs (consecutive tokens of one name merge), models and lights arrays (writeArray), user images and
/// unknown tokens; AUTOREFRESH is skipped. `frame` is converted to a float for the f-curve values. Answers 0.
pub fn si_udf_output_to_mi(m: &mut Mem, o: &mut dyn Outside, d: u32, u: u32, frame: i32) -> i32 {
    let w = m.local(1040);
    let vbuf = w; // 500 bytes: value text
    let nbuf = w + 504; // 500 bytes: truncated name
    let fl = w + 1008; // 5 floats
    let outs = w + 1032; // [kind_out, text_out]
    let mut first: i16 = 1;
    o.call(m, SI_UDF_PARSE_RAW_DATA, &[u]);
    let mut flag = false;
    let mut cur = m.u32(u.wrapping_add(0x2c));
    if cur == 0 {
        flag = true;
        cur = m.u32(u.wrapping_add(0x30));
    }
    let fframe = frame as f32;
    let sh = |m: &Mem| m.u32(u.wrapping_add(0x24));
    'outer: while cur != 0 {
        let nm = m.u32(cur);
        cus_trace(m, o, &[0x086e_3ea0, if nm == 0 { 0x086e_3e93 } else { nm }, m.u16(cur.wrapping_add(4)) as i16 as i32 as u32]);
        let mut advance = true;
        'tok: {
            if is_valid_token(m, cur) == 0 {
                break 'tok;
            }
            if format_custom_value(m, o, cur, fframe, vbuf) as u16 == 0 {
                cus_trace(m, o, &[0x086e_3ec3]);
                cur = m.u32(cur.wrapping_add(0x44));
                advance = false;
                break 'tok;
            }
            truncate_custom_value_token(m, cur, nbuf, 500);
            let kind = si_shader_get_symbol_type_u(m, sh(m), m.u32(cur), outs, outs + 4);
            if clib::strcmp(m, m.u32(cur), 0x086e_3ed2) == 0 {
                break 'tok;
            }
            let name = nbuf;
            match kind {
                0 => {
                    if first == 0 {
                        pf(m, o, 0x086e_4023, &[]);
                    }
                    o.call(m, PLT_PRINTF, &[0x086e_4026, name]);
                    pf(m, o, 0x086e_403e, &[name, vbuf]);
                    first = 0;
                }
                2 | 3 | 4 => {
                    if first == 0 {
                        pf(m, o, 0x086e_3f9c, &[]);
                    }
                    pf(m, o, 0x086e_3f9f, &[name, vbuf]);
                    first = 0;
                    cus_trace(m, o, &[0x086e_3fa8]);
                }
                5 => {
                    if first == 0 {
                        pf(m, o, 0x086e_3f75, &[]);
                    }
                    pf(m, o, 0x086e_3f78, &[name, vbuf]);
                    first = 0;
                    cus_trace(m, o, &[0x086e_3f83]);
                }
                6 | 7 => {
                    for i in 0..5u32 {
                        m.set_u32(fl + i * 4, 0);
                    }
                    let dup = clib::strdup(m, name);
                    if dup == 0 {
                        m.release(1040);
                        return 0;
                    }
                    let mut kind2 = kind;
                    let mut prev;
                    let mut first_pass = true;
                    loop {
                        if !first_pass {
                            format_custom_value(m, o, cur, fframe, vbuf);
                        }
                        first_pass = false;
                        let idx = m.u32(outs);
                        let bits = m.u32(cur.wrapping_add(0x10));
                        m.set_u32(fl.wrapping_add(idx.wrapping_mul(4)), bits);
                        prev = cur;
                        cur = m.u32(cur.wrapping_add(0x44));
                        if cur == 0 {
                            break;
                        }
                        truncate_custom_value_token(m, cur, nbuf, 500);
                        kind2 = si_shader_get_symbol_type_u(m, sh(m), m.u32(cur), outs, 0);
                        if clib::strcmp(m, dup, nbuf) != 0 || kind2 != kind {
                            break;
                        }
                    }
                    let _ = kind2;
                    if first == 0 {
                        pf(m, o, 0x086e_3fec, &[]);
                    }
                    pf(m, o, 0x086e_3fef, &[dup]);
                    m.free(dup);
                    for i in 1..4u32 {
                        let v = f32::from_bits(m.u32(fl + i * 4)) as f64;
                        let b = v.to_bits();
                        pf(m, o, 0x086e_3ff6, &[b as u32, (b >> 32) as u32]);
                    }
                    let f4 = f32::from_bits(m.u32(fl + 16));
                    if !(f4 == 0.0) && kind == 7 {
                        let b = (f4 as f64).to_bits();
                        pf(m, o, 0x086e_3ffa, &[b as u32, (b >> 32) as u32]);
                    }
                    cur = prev;
                    first = 0;
                    cus_trace(m, o, &[0x086e_3ffe]);
                }
                0xb => {
                    if first == 0 {
                        pf(m, o, 0x086e_3ede, &[]);
                    }
                    pf(m, o, 0x086e_3ee1, &[name, vbuf]);
                    first = 0;
                    cus_trace(m, o, &[0x086e_3eea]);
                }
                0xc | 0x1c => {
                    let cls = m.u32(if kind == 0xc { CID_GLOBAL } else { CID_MODELS });
                    let attrs = m.u32(outs + 4);
                    let r = write_array(m, o, d, u, cls, vbuf, name, attrs, first) as i16;
                    if r == 0 {
                        print_udf_array_warning(m, o, d, u, name);
                    } else {
                        first = 0;
                    }
                    cus_trace(m, o, &[if kind == 0xc { 0x086e_3f24 } else { 0x086e_3f08 }]);
                }
                0x10 => {
                    if first == 0 {
                        pf(m, o, 0x086e_401b, &[]);
                    }
                    pf(m, o, 0x086e_401e, &[vbuf]);
                    first = 0;
                }
                0x14 | 0x15 => {
                    let s = m.u32(cur.wrapping_add(0x18));
                    if o.call(m, SI_EMPTY_STRING, &[s]).eax as u16 == 0 {
                        if first == 0 {
                            pf(m, o, 0x086e_3f40, &[]);
                        }
                        pf(m, o, 0x086e_3f43, &[name, u, name]);
                        first = 0;
                        cus_trace(m, o, &[0x086e_3f5f]);
                    }
                }
                0x1d => {
                    if first == 0 {
                        pf(m, o, 0x086e_3fc1, &[]);
                    }
                    pf(m, o, 0x086e_3fc4, &[name]);
                    let mut a = Vec::new();
                    for off in [0x1cu32, 0x20, 0x24, 0x28] {
                        let b = (f32::from_bits(m.u32(cur.wrapping_add(off))) as f64).to_bits();
                        a.push(b as u32);
                        a.push((b >> 32) as u32);
                    }
                    pf(m, o, 0x086e_3fcb, &a);
                    first = 0;
                    cus_trace(m, o, &[0x086e_3fd7]);
                }
                _ => cus_trace(m, o, &[0x086e_4048]),
            }
        }
        if advance {
            cur = m.u32(cur.wrapping_add(0x44));
            if cur == 0 && !flag {
                flag = true;
                cur = m.u32(u.wrapping_add(0x30));
            }
        }
        continue 'outer;
    }
    pf(m, o, 0x086e_4064, &[]);
    m.release(1040);
    0
}

fn si_shader_get_symbol_type_u(m: &mut Mem, sh: u32, name: u32, kind_out: u32, text_out: u32) -> i32 {
    crate::custom_gen::si_shader_get_symbol_type(m, sh, name, kind_out, text_out)
}
