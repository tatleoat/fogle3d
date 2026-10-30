// SPDX-License-Identifier: MIT
//! What `SI_edAddUDF` does to a shader reference (`__SI_UDF`) once it has its element:
//! `SI_udfAttachArrayElements` and `SI_udfParseRawData` (`custom_udf.c`), with the two
//! callees they share, `SI_shaderGetDialog` (`custom_shader.c`) and
//! `si_cusDialogAttachArrayElements` (`custom_dialog.c`), on the memory image. The custom
//! effects' own routines (loading a shader, copying and parsing its parameter values,
//! the dialogue) go through [`ByAddr`].
//!
//! The reference's fields are those of `si-hrc`'s `image::udf::Udf`. A loaded shader
//! (`SI_CustomShader`) holds its dialogue (`SI_CustomDialog`) at +0x30: the dialogue's
//! +4 (the shader's +0x34) is the window's record (`__SI_Dialog`), null until the shader is
//! loaded; its +0x10 a parameter value (`custom_value_struct`); its +0x54 the list of
//! parameter values.
//!
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_edmat.py`, the
//! fourth part; `scene-emucheck re/out/si-scene/vec_edmat`): `free` ran as it is; the
//! routines named below with their address were recorded stand-ins.

use crate::edtools::ByAddr;
use crate::globals::SI_FRAME_CURRENT;
use si_mem::Mem;

/// `shaderLoad (0x08540898)`, `custom_shader.c`: (shader); answers 16 bits, 0 when the
/// shader could not be loaded.
pub const SHADER_LOAD: u32 = 0x0854_0898;
/// `si_customValueListCopy (0x08526488)`, `custom_dialog.c`: (list); a copy of a list of
/// parameter values.
pub const SI_CUSTOM_VALUE_LIST_COPY: u32 = 0x0852_6488;
/// `SI_cusParseDataString (0x0852487c)`, `custom_dialog.c`: (reference, dialogue, text,
/// 16-bit flag); a list of parameter values read from the text a file stored.
pub const SI_CUS_PARSE_DATA_STRING: u32 = 0x0852_487c;
/// `SI_statusDisplayMessage (0x083cca94)`, `status.c`: (text, 16-bit kind).
pub const SI_STATUS_DISPLAY_MESSAGE: u32 = 0x083c_ca94;
/// `SI_cusDialogSet (0x08523e50)`, `custom_dialog.c`: (window, value, frame).
pub const SI_CUS_DIALOG_SET: u32 = 0x0852_3e50;
/// `expandSelectArrays (0x08526e30)`, `custom_dialog.c`: (window, reference).
pub const EXPAND_SELECT_ARRAYS: u32 = 0x0852_6e30;
/// `collapseSelectArrays (0x08526bbc)`, `custom_dialog.c`: (window, reference).
pub const COLLAPSE_SELECT_ARRAYS: u32 = 0x0852_6bbc;
/// `cusDialogGet (0x08523a34)`, `custom_dialog.c`: (dialogue).
pub const CUS_DIALOG_GET: u32 = 0x0852_3a34;

/// "UDF Parse: No valid descriptor present" (in `.data`): only its address is handed on.
pub const S_NO_DESCRIPTOR: u32 = 0x086e_3e20;
/// "UDF Parse: No dialog present. Cannot link symbols." (in `.data`).
pub const S_NO_DIALOG: u32 = 0x086e_3e60;

/// A shader's dialogue [soft 0x08540718 SI_shaderGetDialog]: null for a null shader. A
/// shader whose +0x34 is null is loaded first (`shaderLoad`), and null is the answer when
/// that answers 0 in its 16 bits. Otherwise the shader's address + 0x30.
pub fn shader_get_dialog<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, shader: u32) -> u32 {
    if shader == 0 {
        return 0;
    }
    if m.u32(shader + 0x34) == 0 && o.call_addr(m, SHADER_LOAD, &[shader]) as u16 == 0 {
        return 0;
    }
    shader + 0x30
}

/// Reads the parameter values a reference brought from its file [soft 0x08545660
/// SI_udfParseRawData]: 1 at once when it has either list (+0x2c, +0x30).
///
/// With no shader (+0x24) or no dialogue ([`shader_get_dialog`]) the status line gets a
/// message (`SI_statusDisplayMessage(text, 3)`) and the answer is 0. Otherwise +0x2c
/// becomes `si_customValueListCopy` of the dialogue's list (+0x54); with a text (+0x38),
/// +0x30 becomes `SI_cusParseDataString(reference, dialogue, text, 1)`. Then the dialogue's
/// list and +0x2c change places, the text is freed and cleared, and the answer is 1.
pub fn udf_parse_raw_data<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, udf: u32) -> i32 {
    if m.u32(udf + 0x2c) != 0 || m.u32(udf + 0x30) != 0 {
        return 1;
    }
    let shader = m.u32(udf + 0x24);
    let text = if shader == 0 {
        S_NO_DESCRIPTOR
    } else {
        let dialog = shader_get_dialog(o, m, shader);
        if dialog != 0 {
            let list = m.u32(dialog + 0x54);
            let copy = o.call_addr(m, SI_CUSTOM_VALUE_LIST_COPY, &[list]);
            m.set_u32(udf + 0x2c, copy);
            let raw = m.u32(udf + 0x38);
            if raw != 0 {
                let parsed = o.call_addr(m, SI_CUS_PARSE_DATA_STRING, &[udf, dialog, raw, 1]);
                m.set_u32(udf + 0x30, parsed);
            }
            let theirs = m.u32(dialog + 0x54);
            m.set_u32(dialog + 0x54, m.u32(udf + 0x2c));
            let raw = m.u32(udf + 0x38);
            m.set_u32(udf + 0x2c, theirs);
            if raw != 0 {
                m.free(raw);
                m.set_u32(udf + 0x38, 0);
            }
            return 1;
        }
        S_NO_DIALOG
    };
    o.call_addr(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 3]);
    0
}

/// Lays a reference's array parameters out in a dialogue [soft 0x085238fc
/// si_cusDialogAttachArrayElements]: nothing when either is null or the dialogue has no
/// window (+4). Otherwise, with the window read once: `SI_cusDialogSet(window, dialogue +
/// 0x10, SI_frameCurrent)`, `expandSelectArrays(window, reference)`,
/// `collapseSelectArrays(window, reference)`, `cusDialogGet(dialogue)`.
pub fn cus_dialog_attach_array_elements<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dialog: u32, udf: u32) {
    if udf == 0 || dialog == 0 {
        return;
    }
    let window = m.u32(dialog + 4);
    if window == 0 {
        return;
    }
    let frame = m[SI_FRAME_CURRENT] as u32;
    o.call_addr(m, SI_CUS_DIALOG_SET, &[window, dialog + 0x10, frame]);
    o.call_addr(m, EXPAND_SELECT_ARRAYS, &[window, udf]);
    o.call_addr(m, COLLAPSE_SELECT_ARRAYS, &[window, udf]);
    o.call_addr(m, CUS_DIALOG_GET, &[dialog]);
}

/// Attaches a reference's array parameters [soft 0x08545584 SI_udfAttachArrayElements]:
/// nothing for a null reference, one whose 16 bits at +0x10 are 1 (internal), one with no
/// shader (+0x24), or when [`shader_get_dialog`] answers null. Otherwise
/// [`udf_parse_raw_data`] (its answer not looked at); then, for the time of
/// [`cus_dialog_attach_array_elements`], the dialogue's list (+0x54) is the reference's
/// (+0x2c): the reference keeps what the dialogue's list is afterwards, and the dialogue
/// gets its own back.
pub fn udf_attach_array_elements<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, udf: u32) {
    if udf == 0 || m.u16(udf + 0x10) == 1 {
        return;
    }
    let shader = m.u32(udf + 0x24);
    if shader == 0 {
        return;
    }
    let dialog = shader_get_dialog(o, m, shader);
    if dialog == 0 {
        return;
    }
    udf_parse_raw_data(o, m, udf);
    let saved = m.u32(dialog + 0x54);
    m.set_u32(dialog + 0x54, m.u32(udf + 0x2c));
    cus_dialog_attach_array_elements(o, m, dialog, udf);
    m.set_u32(udf + 0x2c, m.u32(dialog + 0x54));
    m.set_u32(dialog + 0x54, saved);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every call logged; `shaderLoad` answers `self.1`, the list's copy 0x7100, the parse
    /// 0x7200.
    struct Log(Vec<(u32, Vec<u32>)>, u32);
    impl ByAddr for Log {
        fn call_addr(&mut self, _m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SHADER_LOAD => self.1,
                SI_CUSTOM_VALUE_LIST_COPY => 0x7100,
                SI_CUS_PARSE_DATA_STRING => 0x7200,
                _ => 0,
            }
        }
    }

    fn image() -> Mem {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        crate::globals::store(&mut m);
        m
    }

    #[test]
    fn a_dialog_is_loaded_when_the_shader_has_no_window() {
        let mut m = image();
        let shader = m.calloc(1, 0x88);
        let mut o = Log(Vec::new(), 0x1_0000);
        assert_eq!(shader_get_dialog(&mut o, &mut m, 0), 0);
        // only the low 16 bits of shaderLoad's answer count
        assert_eq!(shader_get_dialog(&mut o, &mut m, shader), 0);
        o.1 = 1;
        assert_eq!(shader_get_dialog(&mut o, &mut m, shader), shader + 0x30);
        assert_eq!(o.0, [(SHADER_LOAD, vec![shader]), (SHADER_LOAD, vec![shader])]);
        m.set_u32(shader + 0x34, 0x5000);
        let mut o = Log(Vec::new(), 0);
        assert_eq!(shader_get_dialog(&mut o, &mut m, shader), shader + 0x30);
        assert!(o.0.is_empty());
    }

    #[test]
    fn raw_data_is_parsed_and_the_lists_change_places() {
        let mut m = image();
        let (udf, shader) = (m.calloc(1, 0x3c), m.calloc(1, 0x88));
        let mut o = Log(Vec::new(), 0);
        // no shader: the message
        assert_eq!(udf_parse_raw_data(&mut o, &mut m, udf), 0);
        m.set_u32(udf + 0x24, shader);
        // a shader that does not load: the other message
        assert_eq!(udf_parse_raw_data(&mut o, &mut m, udf), 0);
        assert_eq!(o.0, [(SI_STATUS_DISPLAY_MESSAGE, vec![S_NO_DESCRIPTOR, 3]), (SHADER_LOAD, vec![shader]), (SI_STATUS_DISPLAY_MESSAGE, vec![S_NO_DIALOG, 3])]);
        m.set_u32(shader + 0x34, 0x5000);
        m.set_u32(shader + 0x84, 0x6000);
        let raw = m.malloc(4);
        m.put_cstr(raw, b"1 2");
        m.set_u32(udf + 0x38, raw);
        let mut o = Log(Vec::new(), 0);
        assert_eq!(udf_parse_raw_data(&mut o, &mut m, udf), 1);
        assert_eq!(o.0, [(SI_CUSTOM_VALUE_LIST_COPY, vec![0x6000]), (SI_CUS_PARSE_DATA_STRING, vec![udf, shader + 0x30, raw, 1])]);
        assert_eq!((m.u32(udf + 0x2c), m.u32(udf + 0x30), m.u32(shader + 0x84), m.u32(udf + 0x38)), (0x6000, 0x7200, 0x7100, 0));
        assert!(m.is_freed(raw));
        // a reference with a list is left alone
        let mut o = Log(Vec::new(), 0);
        assert_eq!(udf_parse_raw_data(&mut o, &mut m, udf), 1);
        assert!(o.0.is_empty());
    }

    #[test]
    fn array_elements_are_attached_with_the_reference_s_list() {
        let mut m = image();
        m[SI_FRAME_CURRENT] = 25;
        let (udf, shader) = (m.calloc(1, 0x3c), m.calloc(1, 0x88));
        m.set_u32(udf + 0x24, shader);
        m.set_u32(udf + 0x2c, 0x6100);
        m.set_u32(shader + 0x34, 0x5000);
        m.set_u32(shader + 0x84, 0x6000);
        let dialog = shader + 0x30;
        // an internal reference is left alone
        m.set_u16(udf + 0x10, 1);
        let mut o = Log(Vec::new(), 0);
        udf_attach_array_elements(&mut o, &mut m, udf);
        assert!(o.0.is_empty());
        m.set_u32(udf + 0x10, 0x0001_0002);
        udf_attach_array_elements(&mut o, &mut m, udf);
        #[rustfmt::skip]
        assert_eq!(o.0, [
            (SI_CUS_DIALOG_SET, vec![0x5000, dialog + 0x10, 25]),
            (EXPAND_SELECT_ARRAYS, vec![0x5000, udf]),
            (COLLAPSE_SELECT_ARRAYS, vec![0x5000, udf]),
            (CUS_DIALOG_GET, vec![dialog]),
        ]);
        assert_eq!((m.u32(udf + 0x2c), m.u32(shader + 0x84)), (0x6100, 0x6000));
        // no window: the dialogue is not shown
        let mut o = Log(Vec::new(), 0);
        cus_dialog_attach_array_elements(&mut o, &mut m, shader + 0x40, udf);
        cus_dialog_attach_array_elements(&mut o, &mut m, dialog, 0);
        assert!(o.0.is_empty());
    }
}
