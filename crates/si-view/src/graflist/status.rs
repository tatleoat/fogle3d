// SPDX-License-Identifier: MIT
//! The status line's text for what is selected, and a point's distance from a camera.

use super::events::scene_list;
use super::flags::{graf_node_is_selected, graf_node_is_wrath_selected};
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::flag;
use si_mem::{clib, CStr32, Mem, Opaque, Ptr32};
use si_types::dict::Elem;
use si_types::scene::GrafNode;
use si_x87::F80;

/// Appends the text at `text` to the status line, unless the line's length plus `more`
/// would pass 0x3ff (the line is 0x400 bytes). The original has `strlen` inline before
/// every `strcat`.
fn append(m: &mut Mem, text: u32, more: u32) {
    let line = FINAL_STRING.addr();
    if clib::strlen(m, line).wrapping_add(more) <= 0x3ff {
        clib::strcat(m, line, text);
    }
}

/// The status line's text for the scene's selection [soft 0x08333978 SI_selectedString]:
/// answers the routine's own line of 0x400 bytes, written afresh at every call, and
/// clears `multipleSelected`.
///
/// The line is: `prefix` (when not null); `" Multi"` when the scene's list is in multiple
/// selection; then, for one node, `": "`, the prefix of its element's name record, `"-"`
/// between the two when there is a prefix, the element's name, and `"[Br]"` when the node
/// is selected as a branch (selection 3, or flag 0x4000). Every piece is left out when
/// the line would grow past 0x3ff bytes.
///
/// The node: in single selection the list's active node (the second place when the
/// first is empty); none means the line ends there. In multiple selection: when
/// `lastSelected` is null, the nodes are counted that are selected (a node that is not
/// editable by its flags, and failing that by its selection word like the others), the
/// walk ending at the second; exactly one makes it the node. Otherwise `lastSelected` is
/// the node, or there is none. `lastSelected` is cleared either way.
///
/// The element comes from `SI_elemdictFindTrueElemFromGrafNode (0x080fcd34)`; the line
/// ends before `": "` when there is none, when it has no name record, when the node is
/// neither selected by its word nor by its three flag bits, or when the record has
/// neither prefix nor name.
pub fn selected_string(m: &mut Mem, o: &mut dyn Outside, prefix: CStr32) -> CStr32 {
    let line = FINAL_STRING.addr();
    m[MULTIPLE_SELECTED] = 0;
    m.set_u8(line, 0);
    if !prefix.is_null() {
        let n = clib::strlen(m, prefix.addr());
        append(m, prefix.addr(), n);
    }
    let list = scene_list(m);
    let multi = m[list].multi;
    let mut node = m[list].active;
    if node.is_null() {
        node = m[list].active2;
    }
    if multi != 0 {
        append(m, text::MULTI, 6);
    }
    if node.is_null() && multi == 0 {
        return FINAL_STRING.cast();
    }
    if multi != 0 {
        let mut count = 0;
        let mut first: Ptr32<GrafNode> = Ptr32::NULL;
        node = Ptr32::NULL;
        if m[LAST_SELECTED].is_null() {
            let list = scene_list(m);
            let mut at = m[list].first;
            while !at.is_null() {
                let selected = (m[at].flags & flag::UNEDITABLE != 0 && graf_node_is_wrath_selected(m, at) as i16 != 0) || graf_node_is_selected(m, at) as i16 != 0;
                if selected {
                    count += 1;
                    if count == 1 {
                        first = at;
                    }
                }
                at = m[at].next;
                if count > 1 || m.fault().is_some() {
                    break;
                }
            }
        }
        if count == 1 {
            node = first;
        } else if !m[LAST_SELECTED].is_null() {
            node = m[LAST_SELECTED];
        }
        m[LAST_SELECTED] = Ptr32::NULL;
    }
    if node.is_null() {
        return FINAL_STRING.cast();
    }
    let index = m.local(4);
    let chap = m.local(4);
    let elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]));
    m.release(8);
    if elem.is_null() {
        return FINAL_STRING.cast();
    }
    let record = m[elem].node;
    if record.is_null() || (m[node].selection == 0 && m[node].flags & flag::SEL_ANY == 0) {
        return FINAL_STRING.cast();
    }
    let (prefix, name) = (m[record].prefix.addr(), m[record].name.addr());
    if prefix == 0 && name == 0 {
        return FINAL_STRING.cast();
    }
    append(m, text::COLON, 2);
    if prefix != 0 {
        let n = clib::strlen(m, prefix);
        append(m, prefix, n);
    }
    if name != 0 {
        if prefix != 0 {
            append(m, text::DASH, 1);
        }
        let n = clib::strlen(m, name);
        append(m, name, n);
    }
    if m[node].selection == 3 || m[node].flags & flag::SEL_BRANCH != 0 {
        append(m, text::BRANCH, 4);
    }
    FINAL_STRING.cast()
}

/// The four constant texts of the status line, in the binary's `.data`.
pub mod text {
    /// `" Multi"`.
    pub const MULTI: u32 = 0x086b_aec8;
    /// `": "`.
    pub const COLON: u32 = 0x086b_aecf;
    /// `"-"`.
    pub const DASH: u32 = 0x086b_aed2;
    /// `"[Br]"`.
    pub const BRANCH: u32 = 0x086b_aed4;
    /// Address and bytes of each (the terminating zero is written with them). Held
    /// against the binary by `re/tools/si-view/check_data.py`.
    pub const ALL: [(u32, &[u8]); 4] = [(MULTI, b" Multi"), (COLON, b": "), (DASH, b"-"), (BRANCH, b"[Br]")];
}

/// Writes the constant texts of this file into the image, where the binary has them.
pub fn init_status_text(m: &mut Mem) {
    for (addr, bytes) in text::ALL {
        m.put_cstr(addr, bytes);
    }
}

/// Offsets in the camera record `SI_vectorDistancectCam` reads. Read from its own
/// accesses; the record belongs to the camera's package.
mod camera {
    /// 16 bits: 0 the view direction goes to the interest, else it is three angles.
    pub const BY_ANGLES: u32 = 0x0a;
    /// 16 bits: not 0 means the distance is the straight one.
    pub const STRAIGHT: u32 = 0x0c;
    pub const POSITION: u32 = 0x10;
    pub const INTEREST: u32 = 0x1c;
    /// The angles handed to the Y, X and Z rotation.
    pub const ANGLE_Y: u32 = 0x28;
    pub const ANGLE_X: u32 = 0x2c;
    pub const ANGLE_Z: u32 = 0x30;
}

/// The float constant at 0x086586ec of the binary's `.rodata`: −1.0, the third coordinate
/// of the view direction before the camera's angles turn it. Held against the binary by
/// `re/tools/si-view/check_data.py`.
pub const MINUS_ONE_AT: u32 = 0x0865_86ec;
pub const MINUS_ONE_BITS: u32 = 0xbf80_0000;

/// How far a point is from a camera [soft 0x0833456c SI_vectorDistancectCam]. Answers
/// what the original leaves in the x87 register.
///
/// The difference point − position is taken coordinate by coordinate and stored as three
/// floats. When the camera's word at +0x0c is set, the answer is the length of that
/// difference **as the register has it, before rounding**: the three squares are added
/// (the first two first, the second as first operand), the root taken and the result
/// rounded to a float.
///
/// Otherwise the answer is the difference, as stored, along the camera's view direction:
/// the sum of the three products, the first two added first, **not rounded** (the
/// register's 80 bits are the answer). The direction is a vector on the stack: with the
/// word at +0x0a clear, interest − position, each coordinate rounded to a float, handed
/// to `SI_vectorNormalize (0x08516158)`; with it set, (0, 0, −1) turned in place by three
/// matrices made in a second local: `SI_matrixZRotation (0x08517140)` of the angle at
/// +0x30, `SI_matrixXRotation (0x08517080)` of the one at +0x2c, `SI_matrixYRotation
/// (0x085170e0)` of the one at +0x28, each followed by `SI_vectorMultiplyMatrix
/// (0x08516d74)` (the vector, the matrix, the vector, 1).
pub fn vector_distance_ct_cam(m: &mut Mem, o: &mut dyn Outside, cam: Ptr32<Opaque>, point: Ptr32<[f32; 3]>) -> F80 {
    let cam = cam.addr();
    let p = m[point];
    let c = [m.f32(cam.wrapping_add(camera::POSITION)), m.f32(cam.wrapping_add(camera::POSITION + 4)), m.f32(cam.wrapping_add(camera::POSITION + 8))];
    let d = [F80::from_f32(p[0]) - F80::from_f32(c[0]), F80::from_f32(p[1]) - F80::from_f32(c[1]), F80::from_f32(p[2]) - F80::from_f32(c[2])];
    let stored = [d[0].to_f32(), d[1].to_f32(), d[2].to_f32()];
    if m.i16(cam.wrapping_add(camera::STRAIGHT)) != 0 {
        let sum = d[2] * d[2] + (d[1] * d[1] + d[0] * d[0]);
        return F80::from_f32(sum.sqrt().to_f32());
    }
    let matrix = m.local(0x40);
    let v = m.local(12);
    if m.i16(cam.wrapping_add(camera::BY_ANGLES)) == 0 {
        for k in 0..3u32 {
            let i = m.f32(cam.wrapping_add(camera::INTEREST + 4 * k));
            m.set_f32(v + 4 * k, (F80::from_f32(i) - F80::from_f32(c[k as usize])).to_f32());
        }
        o.call(m, &outside::SI_VECTOR_NORMALIZE, &[v]);
    } else {
        m.set_u32(v, 0);
        m.set_u32(v + 4, 0);
        m.set_u32(v + 8, MINUS_ONE_BITS);
        for (routine, angle) in [(&outside::SI_MATRIX_Z_ROTATION, camera::ANGLE_Z), (&outside::SI_MATRIX_X_ROTATION, camera::ANGLE_X), (&outside::SI_MATRIX_Y_ROTATION, camera::ANGLE_Y)] {
            let a = m.u32(cam.wrapping_add(angle));
            o.call(m, routine, &[a, matrix]);
            o.call(m, &outside::SI_VECTOR_MULTIPLY_MATRIX, &[v, matrix, v, 1]);
        }
    }
    let (x, y, z) = (m.f32(v), m.f32(v + 4), m.f32(v + 8));
    m.release(12 + 0x40);
    let xy = F80::from_f32(x) * F80::from_f32(stored[0]) + F80::from_f32(y) * F80::from_f32(stored[1]);
    xy + F80::from_f32(z) * F80::from_f32(stored[2])
}
