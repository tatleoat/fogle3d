// SPDX-License-Identifier: MIT
//! The small leftovers of package 35 that the scene file layer and the counter panel
//! share, on the image: the raw f-curve buffer's two accessors (`asceneio.c`), the UV
//! domain id of a 2D texture (`binary_b.c`), the layout window finder (`bsceneio.c`) and the
//! counter panel's resets and the model display values (`counter.c`).
//!
//! Everything they call that is another file's is a hook ([`LeftoverCalls`] and
//! [`super::calls::SystemCalls`]). The increment of `SI_secureLicAlloc` before an allocation
//! is the licence scheme's counter and is not done.

use super::calls::SystemCalls;
use si_mem::{outside_calls, CStr32, Mem, Outside};
use si_x87::F80;

si_mem::globals! {
    /// `rawFcurve`: the f-curve segment the raw f-curve reader leaves for the caller.
    RAW_FCURVE: u32 = 0x086cba20, "rawFcurve";
}

outside_calls! {
    /// The callees of the leftovers that are another file's.
    pub trait LeftoverCalls {
        /// `elemdict.c`: the element of a dictionary with this class id and data id.
        fn elemdict_get_elem(dict: u32, cid: i32, data: i32) -> u32 = "SI_elemdictGetElem", 0x08132b50;
        /// `control.c`: the counter panel (a value is a float's bits).
        fn si_control_set(index: i32, value: u32) = "SI_controlSet", 0x0837efd0;
        fn si_control_set_str(group: i32, index: i32, text: CStr32) = "SI_controlSetStr", 0x0837f090;
        fn si_refresh_all_controls() = "SI_refreshAllControls", 0x0837fc60;
    }
}

/// `SI_procData` (0x08679314), `SI_elemDicts` (0x0867931c), `SI_grafLists` (0x08679498) and the
/// class id `SI_cid.TEXTURES2D` (0x087816c8): the first words of the records they point to.
const PROC_DATA: u32 = 0x0867_9314;
const ELEM_DICTS: u32 = 0x0867_931c;
const GRAF_LISTS: u32 = 0x0867_9498;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
/// `_UVDomainID_`, as the string at 0x086ce7cf (compared) and at 0x086ce7dc (copied).
const UV_DOMAIN_NAME: u32 = 0x086c_e7dc;
const UV_DOMAIN_NAME_CMP: &[u8] = b"_UVDomainID_";
/// The strings the counter panel shows in a group of three, nine texts: `  ...`.
const PANEL_TEXTS: [u32; 9] = [0x0867_cc44, 0x0867_cc4a, 0x0867_cc50, 0x0867_cc56, 0x0867_cc5c, 0x0867_cc62, 0x0867_cc68, 0x0867_cc6e, 0x0867_cc74];

/// [soft 0x083e009c SI_getRawFcurvePointer]: the buffer's f-curve.
pub fn get_raw_fcurve_pointer(m: &Mem) -> u32 {
    m[RAW_FCURVE]
}

/// [soft 0x083e00a8 SI_getRawFcurvePointerPointer]: the address of the variable that holds it.
pub fn get_raw_fcurve_pointer_pointer() -> u32 {
    RAW_FCURVE.addr()
}

/// [soft 0x083f3574 setT2dMapDomainID]: the 2D texture `id` of the current process's
/// dictionary keeps a user-data node `_UVDomainID_` whose value is the id as decimal text.
/// A texture the dictionary does not know changes nothing. The node is found in the list at
/// +0x1d0 of the texture's record (nodes of 0x14 bytes: name +0, length +8, value +0xc, next
/// +0x10); one whose value already reads `id` is left alone, one that reads another number
/// loses its value text, and a missing one is made and put at the head of the list. Then
/// the value is `id` printed with `%d`, its length with the terminator in +8.
pub fn set_t2d_map_domain_id(m: &mut Mem, o: &mut dyn Outside, id: i32) {
    let proc_data = m.u32(PROC_DATA);
    let dict = m.u32(ELEM_DICTS).wrapping_add(m.i32(proc_data).wrapping_mul(7).wrapping_mul(0x10) as u32);
    let cid = m.i32(CID_TEXTURES2D);
    let elem = o.elemdict_get_elem(m, dict, cid, id);
    if elem == 0 {
        return;
    }
    let record = m.u32(elem + 4);
    let mut node = m.u32(record + 0x1d0);
    while node != 0 {
        if m.cstr(m.u32(node)) == UV_DOMAIN_NAME_CMP {
            break;
        }
        node = m.u32(node + 0x10);
    }
    if node != 0 {
        if si_mem::clib::strtol10(m, m.u32(node + 0xc)) == id {
            return;
        }
        let value = m.u32(node + 0xc);
        if value != 0 {
            m.free(value);
            m.set_u32(node + 0xc, 0);
        }
    } else {
        // (the licence counter SI_secureLicAlloc is not touched)
        node = m.si_calloc(1, 0x14);
        let name = m.strdup(UV_DOMAIN_NAME);
        m.set_u32(node, name);
        let next = m.u32(record + 0x1d0);
        m.set_u32(node + 0x10, next);
        m.set_u32(record + 0x1d0, node);
    }
    let text = si_mem::clib::decimal(id);
    let buf = m.local(0x80);
    m.put_cstr(buf, &text);
    m.set_u32(node + 8, text.len() as u32 + 1);
    let copy = m.strdup(buf);
    m.set_u32(node + 0xc, copy);
    m.release(0x80);
}

/// [soft 0x083f90c8 si_getLayoutWindow]: the first window of the list (`SI_windowGetNext`)
/// whose id (+4) is 1000 for layout 1, 0x3ea for 3, 0x3e9 for 5 and 0x3eb for 7; null for
/// another layout or when there is none.
pub fn get_layout_window(m: &mut Mem, o: &mut dyn Outside, layout: i16) -> u32 {
    let slot = m.local(4);
    m.set_u32(slot, 0);
    let id = match layout {
        1 => 1000,
        3 => 0x3ea,
        5 => 0x3e9,
        7 => 0x3eb,
        _ => {
            m.release(4);
            return 0;
        }
    };
    let mut found = 0;
    while o.si_window_get_next(m, slot) != 0 {
        let w = m.u32(slot);
        if m.i16(w + 4) == id {
            found = w;
            break;
        }
    }
    m.release(4);
    found
}

/// A float read from the image and stored again, as `fld` and `fstp` do: a signalling NaN
/// comes back quiet.
fn through_x87(m: &Mem, addr: u32) -> u32 {
    F80::from_f32(f32::from_bits(m.u32(addr))).to_f32().to_bits()
}

/// [soft 0x080d1bdc resetSize]: the three scale controls to 1.
pub fn reset_size(m: &mut Mem, o: &mut dyn Outside) {
    for i in 0..3 {
        o.si_control_set(m, i, 0x3f80_0000);
    }
}

/// [soft 0x080d1c18 resetRotation]: the three rotation controls to 0.
pub fn reset_rotation(m: &mut Mem, o: &mut dyn Outside) {
    for i in 3..6 {
        o.si_control_set(m, i, 0);
    }
}

/// [soft 0x080d1c4c SI_resetCounter]: with nothing selected in the current graph list
/// (`SI_grafLists[SI_procData->+4]`, 0x24 bytes each, +8) every control is refreshed;
/// otherwise the nine texts of groups 1 to 3 are put in the panel.
pub fn reset_counter(m: &mut Mem, o: &mut dyn Outside) {
    let idx = m.i32(m.u32(PROC_DATA) + 4);
    let head = m.u32(m.u32(GRAF_LISTS).wrapping_add((idx.wrapping_mul(9) as u32).wrapping_mul(4)).wrapping_add(8));
    if head == 0 {
        o.si_refresh_all_controls(m);
    } else {
        for (k, text) in PANEL_TEXTS.iter().enumerate() {
            o.si_control_set_str(m, 1 + (k / 3) as i32, (k % 3) as i32, CStr32::new(*text));
        }
    }
}

/// [soft 0x080d1d20 SI_refreshCounter]: every control refreshed.
pub fn refresh_counter(m: &mut Mem, o: &mut dyn Outside) {
    o.si_refresh_all_controls(m);
}

/// [soft 0x080d1320 SI_displayModelScal]: the model's scale for the panel, into `out` (three
/// floats; answers `out`). The scale comes from the patch fitting if the model has one
/// (+8), else the spline fitting (+0xc), else the model (+0x100). With `show` the controls
/// 0 to 2 are set to it. Every float goes through the x87 stack on its way.
pub fn display_model_scal(m: &mut Mem, o: &mut dyn Outside, out: u32, model: u32, show: i16) -> u32 {
    let scratch = m.local(4);
    let mut base = o.si_model_patch_fitting(m, model, scratch);
    let mut at = 8;
    if base == 0 {
        base = o.si_model_spline_fitting(m, model, scratch);
        at = 0xc;
    }
    if base == 0 {
        base = model;
        at = 0x100;
    }
    let mut v = [0u32; 3];
    for (i, w) in v.iter_mut().enumerate() {
        *w = through_x87(m, base + at + 4 * i as u32);
        if show != 0 {
            o.si_control_set(m, i as i32, *w);
        }
    }
    for (i, w) in v.iter().enumerate() {
        m.set_u32(out + 4 * i as u32, *w);
    }
    m.release(4);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::{Ret, Trace};

    /// A texture the dictionary knows, with no user data: the node is made at the head.
    #[test]
    fn domain_id_node_is_made_and_updated() {
        let mut m = Mem::new();
        super::super::init_data(&mut m);
        let pd = m.si_malloc(12);
        m.set_u32(PROC_DATA, pd);
        m.set_u32(ELEM_DICTS, 0x0880_0000);
        let record = m.si_calloc(1, 0x200);
        let elem = m.si_calloc(1, 12);
        m.set_u32(elem + 4, record);
        let mut o = Trace::with(move |_, _, _| Ret::eax(elem));
        set_t2d_map_domain_id(&mut m, &mut o, 42);
        let node = m.u32(record + 0x1d0);
        assert_ne!(node, 0);
        assert_eq!(m.cstr(m.u32(node)), b"_UVDomainID_");
        assert_eq!(m.cstr(m.u32(node + 0xc)), b"42");
        assert_eq!(m.u32(node + 8), 3);
        // a second call with another id replaces the value text of the same node
        set_t2d_map_domain_id(&mut m, &mut o, -7);
        assert_eq!(m.u32(record + 0x1d0), node);
        assert_eq!(m.cstr(m.u32(node + 0xc)), b"-7");
        assert_eq!(m.u32(node + 8), 3);
    }
}
