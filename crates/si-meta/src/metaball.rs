// SPDX-License-Identifier: MIT
//! `metaball.c` on the image (finding 0038).
//!
//! The records are those of `si_hrc::image::meta` (metaball [`ball`], system [`system`])
//! plus the three small ones below, each read from the listing:
//!
//! - a **group** (`metaGroup`, 0x14 bytes, `SI_malloc`): its name (a copy), a word the
//!   copies carry over, its first primitive link, the group after it and the one before it;
//! - a **link** (`__SI_MetaLink`, 0x14 bytes): the metaball, the link after it and the one
//!   before it, the name of the ball's node while the ball is not resolved, a 16-bit flag;
//! - an **unblend pair** (`metaUnblend`, 0x18 bytes): two groups, the pair before it and the
//!   one after it, and the two group names while the groups are not resolved.
//!
//! A metaball hangs in its group through the word at +0xf4 (the group) and its link; the
//! word at +4 of a ball is set to -1 when a group loses it.

use si_hrc::image::meta::{ball, metaball_allocate, meta_system_allocate, system};
use si_mem::{outside_calls, Mem, Outside, Ptr32};
use si_model::model_image::{metaball as modelball, tree};
use si_types::model::Model;
use si_x87::x;

outside_calls! {
    /// What `metaball.c` calls in other files and nobody has ported (or that lives in a
    /// crate this one does not depend on).
    pub trait MetaCalls {
        /// `SI_fastSetDirty (0x08329898)`, `fastplay.c`, ported in `si-model` (`model_image::fastplay::set_dirty`).
        /// It is a hook here because the check binary cannot match its callee `SI_windowGetNextActive`,
        /// which takes the address of a local (the kit has no `57ac57ac` handling); an integration
        /// answers this hook with that function.
        fn si_fast_set_dirty(port: u32, index: i32, all: i32) = "SI_fastSetDirty", 0x08329898;
        /// `SI_modelAllocate (0x08477cf4)`: a node with the given name.
        fn si_model_allocate(name: u32) -> u32 = "SI_modelAllocate", 0x08477cf4;
        /// `SI_modelAddDefinition (0x08467038)`: (node, kind, definition).
        fn si_model_add_definition(model: u32, kind: i32, def: u32) = "SI_modelAddDefinition", 0x08467038;
        /// `SI_graflistAddData (0x0832d240)`: (list, node, kind, select).
        fn si_graflist_add_data(list: u32, model: u32, kind: i32, select: i32) -> u32 = "SI_graflistAddData", 0x0832d240;
        /// `SI_findGlData (0x0832ee68)`: the entry of the list holding the node.
        fn si_find_gl_data(list: u32, model: u32, kind: i32) -> u32 = "SI_findGlData", 0x0832ee68;
        /// `SI_modelCreateParent (0x0817b500)`: (parent node, child node, flag).
        fn si_model_create_parent(parent: u32, child: u32, flag: i32) = "SI_modelCreateParent", 0x0817b500;
        /// `SI_backSelected (0x083260a0)`.
        fn si_back_selected(a: i32, b: i32) = "SI_backSelected", 0x083260a0;
        /// `SI_refreshSystem (0x08323d44)`: (port, list, what).
        fn si_refresh_system(port: u32, list: u32, what: i32) = "SI_refreshSystem", 0x08323d44;
        /// `SI_refreshCounter (0x080d1d20)`.
        fn si_refresh_counter() = "SI_refreshCounter", 0x080d1d20;
        /// `SI_statusDisplayMessage (0x0841a63c)`, `status.c`: (text, level).
        fn si_status_display_message(text: u32, level: i32) = "SI_statusDisplayMessage", 0x0841a63c;
        /// `VCInit (0x08575ca0)`, `user.c`: the metaball library (a start with the new tables).
        fn vc_init() = "VCInit", 0x08575ca0;
        /// `VCReInit (0x08575cf4)`.
        fn vc_re_init() = "VCReInit", 0x08575cf4;
        /// `VCCreatePrim (0x08575dc0)`: (kind, 4 x 4 matrix of doubles, where the handle goes).
        fn vc_create_prim(kind: i32, matrix: u32, handle: u32) = "VCCreatePrim", 0x08575dc0;
        /// `VCInfoPrim (0x08576060)`: (handle, an out local, where the primitive's index goes).
        fn vc_info_prim(handle: i32, a: u32, index: u32) = "VCInfoPrim", 0x08576060;
        /// `VCSetPrimParam (0x08575dec)`: (handle, which, value address).
        fn vc_set_prim_param(handle: i32, which: i32, value: u32) = "VCSetPrimParam", 0x08575dec;
        /// `VCGetPrimParam (0x08575e0c)`: (handle, which, where the value goes).
        fn vc_get_prim_param(handle: i32, which: i32, value: u32) = "VCGetPrimParam", 0x08575e0c;
        /// `VCDeletePrim (0x08575fa8)`.
        fn vc_delete_prim(handle: i32) = "VCDeletePrim", 0x08575fa8;
        /// `VCAddToGroup (0x08575fc0)`: (primitive handle, group index).
        fn vc_add_to_group(handle: i32, group: i32) = "VCAddToGroup", 0x08575fc0;
        /// `VCUnBlendGroups (0x08576044)`: (group index, group index).
        fn vc_unblend_groups(a: i32, b: i32) = "VCUnBlendGroups", 0x08576044;
        /// `VCGenPolyData (0x085760e8)`: (mode, resolution exponent, box, box, triangle count,
        /// triangles, piece count, pieces).
        fn vc_gen_poly_data(mode: i32, n: i32, b0: u32, b1: u32, ntri: u32, tris: u32, npieces: u32, pieces: u32) = "VCGenPolyData", 0x085760e8;
        /// `VCRmAllTri (0x08575d14)`.
        fn vc_rm_all_tri() = "VCRmAllTri", 0x08575d14;
        /// `SI_sceneFrameUpdate (0x08222438)`: the scene at that frame.
        fn si_scene_frame_update(frame: i32) = "SI_sceneFrameUpdate", 0x08222438;
        /// `SI_edGetElemFromModel (0x080fcf20)`, `edtools.c`: the element of a node; its id goes to `*id`.
        fn si_ed_get_elem_from_model(model: u32, id: u32) -> u32 = "SI_edGetElemFromModel", 0x080fcf20;
        /// `SI_fcurveCheckAdd1 (0x08219560)`, `motionKey.c`: (element, id, parameter code, where the curve goes).
        fn si_fcurve_check_add1(elem: u32, id: u32, code: i32, curve: u32) = "SI_fcurveCheckAdd1", 0x08219560;
        /// `SI_fcurveCheckAdd3 (0x082195e8)`: (element, id, code, three places for the curves).
        fn si_fcurve_check_add3(elem: u32, id: u32, code: i32, c0: u32, c1: u32, c2: u32) = "SI_fcurveCheckAdd3", 0x082195e8;
        /// `SI_fkeyFrameCreate (0x0820b1b0)`, `fkey.c`: (frame, value, slope, slope type, interpolation).
        fn si_fkey_frame_create(frame: i32, value: u32, slope: u32, slope_type: i32, interp: i32) -> u32 = "SI_fkeyFrameCreate", 0x0820b1b0;
        /// `SI_fcurveAddKey (0x081ff400)`, `fcurve.c`: (curve, key).
        fn si_fcurve_add_key(curve: u32, key: u32) = "SI_fcurveAddKey", 0x081ff400;
        /// `SI_savekeyObjectAll (0x08219e04)`: keys the node's parameters.
        fn si_savekey_object_all(node: u32) = "SI_savekeyObjectAll", 0x08219e04;
        /// `SI_tabletGetButtonUpEvent (0x083cec2c)`.
        fn si_tablet_get_button_up_event() = "SI_tabletGetButtonUpEvent", 0x083cec2c;
        /// `getbutton (0x0860e684)`: the state of a device button.
        fn getbutton(button: i32) -> i32 = "getbutton", 0x0860e684;
        /// `SI_modelAddScaling (0x084644ec)`: (node, three floats, relative, 0).
        fn si_model_add_scaling(node: u32, scale: u32, mode: i32, geom: i32) = "SI_modelAddScaling", 0x084644ec;
        /// `SI_edFindFcv (0x080edb20)`, `edanim.c`: (element, kind, parameter code), the curve or 0.
        fn si_ed_find_fcv(elem: u32, kind: i32, code: i32) -> u32 = "SI_edFindFcv", 0x080edb20;
        /// `SI_fcurveNextKey (0x08202a58)`, `fcurve.c`: (curve, frame, where the key's frame goes), a short.
        fn si_fcurve_next_key(curve: u32, frame: i32, out: u32) -> i16 = "SI_fcurveNextKey", 0x08202a58;
        /// `SI_fcurvePreviousKey (0x08202b2c)`: (curve, frame, where the key's frame goes), a short.
        fn si_fcurve_previous_key(curve: u32, frame: i32, out: u32) -> i16 = "SI_fcurvePreviousKey", 0x08202b2c;
        /// `SI_modelSetScaling (0x08463a74)`: (node, vector of three floats, mode, geometry).
        fn si_model_set_scaling(node: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetScaling", 0x08463a74;
        /// `SI_modelSetRotation (0x08463f54)`.
        fn si_model_set_rotation(node: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetRotation", 0x08463f54;
        /// `SI_modelSetTranslation (0x084640cc)`.
        fn si_model_set_translation(node: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetTranslation", 0x084640cc;
        /// `SI_windowGetNext (0x083d6484)`: the next window (its address goes to `*out`); 0 when there is none.
        fn si_window_get_next(out: u32) -> i16 = "SI_windowGetNext", 0x083d6484;
        /// `SI_schematicSort (0x0833afe4)`: (window, element dictionary, flag).
        fn si_schematic_sort(win: u32, dict: u32, flag: i32) = "SI_schematicSort", 0x0833afe4;
        /// `SI_refreshAll (0x08325dc8)`: (flag, 16 bits).
        fn si_refresh_all(flag: i16) = "SI_refreshAll", 0x08325dc8;
        /// `sprintf (0x080c4f58)` (C library) with one argument after the format.
        fn sprintf1(dst: u32, fmt: u32, a: u32) -> i32 = "sprintf", 0x080c4f58;
        /// `fopen (0x080c4e18)` (C library).
        fn fopen(path: u32, mode: u32) -> u32 = "fopen", 0x080c4e18;
        /// `fgets (0x080c4388)` (C library).
        fn fgets(buf: u32, n: i32, stream: u32) -> u32 = "fgets", 0x080c4388;
        /// `fclose (0x080c4958)` (C library).
        fn fclose(stream: u32) -> i32 = "fclose", 0x080c4958;
        /// `sscanf (0x080c4c28)` (C library) with three stores.
        fn sscanf3(text: u32, fmt: u32, a: u32, b: u32, c: u32) -> i32 = "sscanf", 0x080c4c28;
        /// `sscanf` with one store.
        fn sscanf1(text: u32, fmt: u32, a: u32) -> i32 = "sscanf", 0x080c4c28;
    }
}

/// A group: the name, a carried-over word, the first link, the next group, the previous.
pub mod group {
    pub const NAME: u32 = 0x00;
    pub const WORD: u32 = 0x04;
    pub const LINKS: u32 = 0x08;
    pub const NEXT: u32 = 0x0c;
    pub const PREV: u32 = 0x10;
    pub const SIZE: u32 = 0x14;
}

/// A link: the ball, the next link, the previous, the node name, the 16-bit flag.
pub mod link {
    pub const BALL: u32 = 0x00;
    pub const NEXT: u32 = 0x04;
    pub const PREV: u32 = 0x08;
    pub const NAME: u32 = 0x0c;
    pub const FLAG: u32 = 0x10;
    pub const SIZE: u32 = 0x14;
}

/// An unblend pair: the two groups, the previous pair, the next, the two group names.
pub mod unblend {
    pub const GROUP_A: u32 = 0x00;
    pub const GROUP_B: u32 = 0x04;
    pub const PREV: u32 = 0x08;
    pub const NEXT: u32 = 0x0c;
    pub const NAME_A: u32 = 0x10;
    pub const NAME_B: u32 = 0x14;
    pub const SIZE: u32 = 0x18;
}

/// `SI_MetaBall` + 4: set to -1 when a group loses the ball (`metaSysGroupErrDetect`).
pub const BALL_INDEX: u32 = 0x04;
/// `SI_Metaball` + 0xf4: the group the ball is in (0 for none).
pub const BALL_GROUP: u32 = 0xf4;
/// `SI_MetaSystem` + 0x00: a short copies carry over.
pub const SYS_WORD: u32 = 0x00;
/// `SI_MetaSystem` + 0x34 and 0x38: the wire-frame and rendering resolutions.
pub const SYS_RES_WIRE: u32 = 0x34;
pub const SYS_RES_RENDER: u32 = 0x38;
/// `SI_MetaSystem` + 0x40: a short copies carry over; +0x42 the show flag; +0x44 cleared by
/// a touch; +0x46 carried over.
pub const SYS_SHORT_40: u32 = 0x40;
pub const SYS_SHOW: u32 = 0x42;
pub const SYS_SHORT_44: u32 = 0x44;
pub const SYS_SHORT_46: u32 = 0x46;
/// `SI_MetaSystem` + 0x4c: the surface's triangle count; +0x50 the triangles (freed);
/// +0x54 the number of pieces; +0x58 the pieces (0x20 bytes each, the block at +4 freed).
pub const SYS_TRI_COUNT: u32 = 0x4c;
pub const SYS_TRIANGLES: u32 = 0x50;
pub const SYS_PIECE_COUNT: u32 = 0x54;
pub const SYS_PIECES: u32 = 0x58;
/// `SI_MetaSystem` + 0x5c: the first group; +0x60 the first unblend pair; +0x64 cleared by
/// a copy.
pub const SYS_GROUPS: u32 = 0x5c;
pub const SYS_UNBLENDS: u32 = 0x60;
pub const SYS_WORD_64: u32 = 0x64;

/// `SI_MetaSystem` as a node's definition: kind 7 is a system, kind 8 a metaball.
const KIND_BALL: i32 = 8;

fn model(a: u32) -> Ptr32<Model> {
    Ptr32::new(a)
}

/// The next metaball-or-other son of the system's node (`SI_modelNextSon (0x0846d70c)`, ported in si-model).
pub(crate) fn next_son(m: &Mem, top: u32, at: u32) -> u32 {
    tree::next_son(m, model(top), model(at)).addr()
}

/// A double copied through the x87 stack (`fld`, `fstp`): a signalling NaN comes out quiet.
pub(crate) fn copy_f64(m: &mut Mem, src: u32, dst: u32) {
    let v = x(m.f64(src)).to_f64();
    m.set_f64(dst, v);
}

/// A float copied through the x87 stack.
fn copy_f32(m: &mut Mem, src: u32, dst: u32) {
    let v = x(m.f32(src)).to_f32();
    m.set_f32(dst, v);
}

/// `SI_matrixCopy (0x08516224)` is sixteen words; the ported form is in `si-hrc`.
fn matrix_copy(m: &mut Mem, src: u32, dst: u32) {
    for k in 0..16 {
        let w = m.u32(src + 4 * k);
        m.set_u32(dst + 4 * k, w);
    }
}

/// [soft 0x0845dffc metaGroupRemovePrimitive]: the link of `ball` leaves the group's list and
/// is freed; the ball's group word is cleared (also when no link was found, if the ball is
/// not null).
pub fn meta_group_remove_primitive(m: &mut Mem, group_: u32, ball_: u32) {
    let mut l = m.u32(group_ + group::LINKS);
    loop {
        if l == 0 {
            return;
        }
        if m.u32(l + link::BALL) == ball_ {
            break;
        }
        l = m.u32(l + link::NEXT);
    }
    let prev = m.u32(l + link::PREV);
    if prev == 0 {
        let next = m.u32(l + link::NEXT);
        m.set_u32(group_ + group::LINKS, next);
    } else {
        let next = m.u32(l + link::NEXT);
        m.set_u32(prev + link::NEXT, next);
    }
    let next = m.u32(l + link::NEXT);
    if next != 0 {
        let prev = m.u32(l + link::PREV);
        m.set_u32(next + link::PREV, prev);
    }
    m.free(l);
    if ball_ != 0 {
        m.set_u32(ball_ + BALL_GROUP, 0);
    }
}

/// [soft 0x0845ab04 SI_metaballDispose]: the ball at `*pball` leaves its group (if it has
/// one) and is freed; `*pball` is cleared.
pub fn metaball_dispose(m: &mut Mem, pball: u32) {
    let b = m.u32(pball);
    if b != 0 {
        let g = m.u32(b + BALL_GROUP);
        if g != 0 {
            meta_group_remove_primitive(m, g, b);
        }
        let b = m.u32(pball);
        if b != 0 {
            m.free(b);
            m.set_u32(pball, 0);
        }
    }
    m.set_u32(pball, 0);
}

/// [soft 0x0845ab50 SI_metaballCopy]: a new ball of `model_` with the weight, the ratio and
/// the three exponents of `src` (each through the x87 stack, in that order) and its matrix.
pub fn metaball_copy(m: &mut Mem, src: u32, model_: u32) -> u32 {
    let n = metaball_allocate(m, model_);
    if n != 0 {
        for off in [ball::WEIGHT, ball::RATIO, ball::EXPONENT, ball::EXPONENT + 8, ball::EXPONENT + 16] {
            copy_f64(m, src + off, n + off);
        }
        matrix_copy(m, src + ball::MATRIX, n + ball::MATRIX);
    }
    n
}

/// [soft 0x0845afc4 deleteUnBlendList]: frees every pair of the list from `first` on.
pub fn delete_unblend_list(m: &mut Mem, first: u32) {
    let mut p = first;
    while p != 0 {
        let next = m.u32(p + unblend::NEXT);
        m.free(p);
        p = next;
    }
}

/// [soft 0x0845aff8 deleteUnBlendNode]: takes the pair out of the system's list and frees it;
/// answers the pair after it (0 for a null pair or the last).
pub fn delete_unblend_node(m: &mut Mem, sys: u32, pair: u32) -> u32 {
    if pair == 0 {
        return 0;
    }
    let next = m.u32(pair + unblend::NEXT);
    let prev = m.u32(pair + unblend::PREV);
    if prev == 0 {
        m.set_u32(sys + SYS_UNBLENDS, next);
    } else {
        m.set_u32(prev + unblend::NEXT, next);
    }
    if next != 0 {
        let prev = m.u32(pair + unblend::PREV);
        m.set_u32(next + unblend::PREV, prev);
    }
    m.free(pair);
    next
}

/// [soft 0x0845b040 deletePrimLink]: takes the link out of the group's list and frees it;
/// answers the link after it.
pub fn delete_prim_link(m: &mut Mem, group_: u32, l: u32) -> u32 {
    if l == 0 {
        return 0;
    }
    let next = m.u32(l + link::NEXT);
    let prev = m.u32(l + link::PREV);
    if prev == 0 {
        m.set_u32(group_ + group::LINKS, next);
    } else {
        m.set_u32(prev + link::NEXT, next);
    }
    if next != 0 {
        let prev = m.u32(l + link::PREV);
        m.set_u32(next + link::PREV, prev);
    }
    m.free(l);
    next
}

/// [soft 0x0845b088 metaSysGroupErrDetect]: repairs a system's groups. Answers -1 when
/// nothing was wrong, else the last repair's code: 1 the unblend pairs of a system without
/// groups dropped, 2 a pair with a group that is not in the system dropped, 3 a link whose
/// ball is null or not a son of the system dropped, 4 a ball whose group word was wrong
/// set right (or a ball in no group freed of a stale group word).
pub fn meta_sys_group_err_detect(m: &mut Mem, sys: u32) -> i32 {
    let top = m.u32(sys + system::MODEL);
    let mut ret: i32 = -1;
    let mut count: u32 = 0;
    let mut g = m.u32(sys + SYS_GROUPS);
    let mut p = 0;
    loop {
        p = next_son(m, top, p);
        if p == 0 {
            break;
        }
        if m[model(p)].kind == KIND_BALL {
            count = count.wrapping_add(1);
        }
    }
    let arr = m.si_calloc(count, 4);
    let mut i: i32 = 0;
    while i < count as i32 {
        m.set_u32(arr + 4 * i as u32, 0);
        i += 1;
    }
    if g == 0 {
        if m.u32(sys + SYS_UNBLENDS) != 0 {
            ret = 1;
            let first = m.u32(sys + SYS_UNBLENDS);
            delete_unblend_list(m, first);
            m.set_u32(sys + SYS_UNBLENDS, 0);
        }
    } else {
        loop {
            let mut l = m.u32(g + group::LINKS);
            while l != 0 {
                let b = m.u32(l + link::BALL);
                let mut drop = b == 0;
                if !drop {
                    m.set_u16(l + link::FLAG, 0);
                    let mut p = 0;
                    let mut idx: i32 = 0;
                    loop {
                        p = next_son(m, top, p);
                        if p == 0 || m.i16(l + link::FLAG) != 0 {
                            break;
                        }
                        if m[model(p)].kind == KIND_BALL {
                            if m.u32(p + 0x48) == m.u32(l + link::BALL) {
                                m.set_u32(arr.wrapping_add((idx as u32).wrapping_mul(4)), 1);
                                m.set_u16(l + link::FLAG, 1);
                            }
                            idx += 1;
                        }
                    }
                    drop = m.i16(l + link::FLAG) == 0;
                }
                if drop {
                    ret = 3;
                    l = delete_prim_link(m, g, l);
                } else {
                    let b = m.u32(l + link::BALL);
                    if m.u32(b + BALL_GROUP) != g {
                        ret = 4;
                        m.set_u32(b + BALL_GROUP, g);
                    }
                    l = m.u32(l + link::NEXT);
                }
            }
            g = m.u32(g + group::NEXT);
            if g == 0 {
                break;
            }
        }
    }
    let mut u = m.u32(sys + SYS_UNBLENDS);
    while u != 0 {
        let (mut a_missing, mut b_missing) = (true, true);
        let mut gg = m.u32(sys + SYS_GROUPS);
        while gg != 0 {
            if m.u32(u + unblend::GROUP_A) == gg {
                a_missing = false;
            }
            if m.u32(u + unblend::GROUP_B) == gg {
                b_missing = false;
            }
            gg = m.u32(gg + group::NEXT);
        }
        if a_missing || b_missing {
            ret = 2;
            u = delete_unblend_node(m, sys, u);
        } else {
            u = m.u32(u + unblend::NEXT);
        }
    }
    let mut p = 0;
    let mut idx: i32 = 0;
    loop {
        p = next_son(m, top, p);
        if p == 0 {
            break;
        }
        if m[model(p)].kind == KIND_BALL {
            let b = m.u32(p + 0x48);
            if m.u32(arr.wrapping_add((idx as u32).wrapping_mul(4))) == 0 && m.u32(b + BALL_GROUP) != 0 {
                m.set_u32(b + BALL_GROUP, 0);
                m.set_u32(b + BALL_INDEX, 0xffff_ffff);
                ret = 4;
            }
            idx += 1;
        }
    }
    if arr != 0 {
        m.free(arr);
    }
    ret
}

/// [soft 0x0845bdd8 getUnblendGrpPt]: the group of the list starting at `dst` that stands at
/// the place `target` has in the list starting at `src`; 0 for a null target or a target
/// that is not in the list.
pub fn get_unblend_grp_pt(m: &Mem, dst: u32, src: u32, target: u32) -> u32 {
    if target != 0 {
        let (mut a, mut b) = (dst, src);
        while b != 0 {
            if a == 0 || target == b {
                return a;
            }
            a = m.u32(a + group::NEXT);
            b = m.u32(b + group::NEXT);
        }
    }
    0
}

/// [soft 0x0845be0c metaUnblendCopy]: copies the unblend pairs of `src` into `dst`, the group
/// names copied, then points each pair's groups at the groups of `dst` that stand where the
/// original's did.
pub fn meta_unblend_copy(m: &mut Mem, dst: u32, src: u32) {
    let mut s = m.u32(src + SYS_UNBLENDS);
    if s == 0 {
        return;
    }
    let mut prev = 0;
    loop {
        let n = m.si_malloc(unblend::SIZE);
        if n == 0 {
            break;
        }
        for off in [unblend::NAME_A, unblend::NAME_B] {
            let name = m.u32(s + off);
            let v = if name == 0 { 0 } else { m.strdup(name) };
            m.set_u32(n + off, v);
        }
        m.set_u32(n + unblend::PREV, prev);
        if prev == 0 {
            m.set_u32(dst + SYS_UNBLENDS, n);
        } else {
            m.set_u32(prev + unblend::NEXT, n);
        }
        m.set_u32(n + unblend::NEXT, 0);
        s = m.u32(s + unblend::NEXT);
        prev = n;
        if s == 0 {
            break;
        }
    }
    let mut d = m.u32(dst + SYS_UNBLENDS);
    let mut s = m.u32(src + SYS_UNBLENDS);
    while s != 0 {
        let (dg, sg) = (m.u32(dst + SYS_GROUPS), m.u32(src + SYS_GROUPS));
        let a = get_unblend_grp_pt(m, dg, sg, m.u32(s + unblend::GROUP_A));
        m.set_u32(d + unblend::GROUP_A, a);
        let (dg, sg) = (m.u32(dst + SYS_GROUPS), m.u32(src + SYS_GROUPS));
        let b = get_unblend_grp_pt(m, dg, sg, m.u32(s + unblend::GROUP_B));
        m.set_u32(d + unblend::GROUP_B, b);
        d = m.u32(d + unblend::NEXT);
        s = m.u32(s + unblend::NEXT);
    }
}

/// [soft 0x0845bf00 metaGroupElementsCopy]: replaces the groups of `sys` by copies of the list
/// starting at `src` (names copied, the word carried over, no links yet), then gives each new
/// group a copy of its original's links: the same ball (whose group word now names the new
/// group), the flag, a copy of the node name.
pub fn meta_group_elements_copy(m: &mut Mem, sys: u32, src: u32) {
    m.set_u32(sys + SYS_GROUPS, 0);
    let mut prev = 0;
    let mut g = src;
    while g != 0 {
        let n = m.si_malloc(group::SIZE);
        if n != 0 {
            let name = m.u32(g + group::NAME);
            let v = if name == 0 { 0 } else { m.strdup(name) };
            m.set_u32(n + group::NAME, v);
            m.set_u32(n + group::PREV, prev);
            if prev == 0 {
                m.set_u32(sys + SYS_GROUPS, n);
            } else {
                m.set_u32(prev + group::NEXT, n);
            }
            let w = m.u32(g + group::WORD);
            m.set_u32(n + group::WORD, w);
            m.set_u32(n + group::LINKS, 0);
            m.set_u32(n + group::NEXT, 0);
            prev = n;
        }
        g = m.u32(g + group::NEXT);
    }
    let mut dg = m.u32(sys + SYS_GROUPS);
    let mut sg = src;
    while dg != 0 {
        let mut prev = 0;
        let mut l = m.u32(sg + group::LINKS);
        while l != 0 {
            let n = m.si_malloc(link::SIZE);
            if n != 0 {
                let f = m.u16(l + link::FLAG);
                m.set_u16(n + link::FLAG, f);
                let b = m.u32(l + link::BALL);
                m.set_u32(n + link::BALL, b);
                if b != 0 {
                    m.set_u32(b + BALL_GROUP, dg);
                }
                let name = m.u32(l + link::NAME);
                let v = if name == 0 { 0 } else { m.strdup(name) };
                m.set_u32(n + link::NAME, v);
                m.set_u32(n + link::PREV, prev);
                if prev == 0 {
                    m.set_u32(dg + group::LINKS, n);
                } else {
                    m.set_u32(prev + link::NEXT, n);
                }
                m.set_u32(n + link::NEXT, 0);
                prev = n;
            }
            l = m.u32(l + link::NEXT);
        }
        sg = m.u32(sg + group::NEXT);
        dg = m.u32(dg + group::NEXT);
    }
}

/// [soft 0x0845c03c addMetaPrimtLnk]: every ball with a link in a group of `sys` gets that
/// group as its group word.
pub fn add_meta_primt_lnk(m: &mut Mem, sys: u32) {
    let mut g = m.u32(sys + SYS_GROUPS);
    while g != 0 {
        let mut l = m.u32(g + group::LINKS);
        while l != 0 {
            let b = m.u32(l + link::BALL);
            if b != 0 {
                m.set_u32(b + BALL_GROUP, g);
            }
            l = m.u32(l + link::NEXT);
        }
        g = m.u32(g + group::NEXT);
    }
}

/// [soft 0x0845e068 metaGroupAddPrimitive]: `ball_` leaves the group it is in (if any) and
/// becomes the head of the list of `group_`, with a new link (flag 1).
pub fn meta_group_add_primitive(m: &mut Mem, group_: u32, ball_: u32) {
    let g = m.u32(ball_ + BALL_GROUP);
    if g != 0 {
        meta_group_remove_primitive(m, g, ball_);
    }
    let n = m.si_malloc(link::SIZE);
    if n != 0 {
        m.set_u16(n + link::FLAG, 1);
        m.set_u32(n + link::BALL, ball_);
        m.set_u32(n + link::NAME, 0);
        m.set_u32(n + link::PREV, 0);
        let first = m.u32(group_ + group::LINKS);
        m.set_u32(n + link::NEXT, first);
        if first != 0 {
            m.set_u32(first + link::PREV, n);
        }
        m.set_u32(group_ + group::LINKS, n);
        m.set_u32(ball_ + BALL_GROUP, group_);
    }
}

/// [soft 0x0845c81c metaSystemCopyAll]: a new system with the values of `src` (box, the
/// resolutions, the node, the shorts), the surface and pieces empty, copies of its groups
/// and unblend pairs and a null +0x64.
pub fn meta_system_copy_all(m: &mut Mem, src: u32) -> u32 {
    let n = meta_system_allocate(m);
    if n != 0 {
        let w = m.u16(src + SYS_WORD);
        m.set_u16(n + SYS_WORD, w);
        for k in 0..6 {
            copy_f64(m, src + system::BOX + 8 * k, n + system::BOX + 8 * k);
        }
        copy_f32(m, src + SYS_RES_WIRE, n + SYS_RES_WIRE);
        copy_f32(m, src + SYS_RES_RENDER, n + SYS_RES_RENDER);
        let v = m.u32(src + system::MODEL);
        m.set_u32(n + system::MODEL, v);
        let v = m.u16(src + SYS_SHORT_40);
        m.set_u16(n + SYS_SHORT_40, v);
        let v = m.u16(src + SYS_SHOW);
        m.set_u16(n + SYS_SHOW, v);
        m.set_u16(n + SYS_SHORT_44, 0);
        let v = m.u16(src + SYS_SHORT_46);
        m.set_u16(n + SYS_SHORT_46, v);
        for off in [SYS_TRI_COUNT, SYS_TRIANGLES, SYS_PIECE_COUNT, SYS_PIECES, SYS_GROUPS] {
            m.set_u32(n + off, 0);
        }
        let groups = m.u32(src + SYS_GROUPS);
        if groups != 0 {
            meta_group_elements_copy(m, n, groups);
        }
        if m.u32(src + SYS_UNBLENDS) == 0 {
            m.set_u32(n + SYS_UNBLENDS, 0);
        } else {
            meta_unblend_copy(m, n, src);
        }
        m.set_u32(n + SYS_WORD_64, 0);
    }
    n
}

/// [soft 0x0845dc2c metaSystemRemoveGroup]: the first unblend pair that has `group_` is
/// dropped; every link of the group is dropped (its ball's group word cleared); the group
/// leaves the system's list, its name and itself are freed.
pub fn meta_system_remove_group(m: &mut Mem, sys: u32, group_: u32) {
    let mut u = m.u32(sys + SYS_UNBLENDS);
    while u != 0 {
        if m.u32(u + unblend::GROUP_A) == group_ || m.u32(u + unblend::GROUP_B) == group_ {
            let prev = m.u32(u + unblend::PREV);
            let next = m.u32(u + unblend::NEXT);
            if prev == 0 {
                m.set_u32(sys + SYS_UNBLENDS, next);
            } else {
                m.set_u32(prev + unblend::NEXT, next);
            }
            let next = m.u32(u + unblend::NEXT);
            if next != 0 {
                let prev = m.u32(u + unblend::PREV);
                m.set_u32(next + unblend::PREV, prev);
            }
            m.free(u);
            break;
        }
        u = m.u32(u + unblend::NEXT);
    }
    while m.u32(group_ + group::LINKS) != 0 {
        let l = m.u32(group_ + group::LINKS);
        let b = m.u32(l + link::BALL);
        meta_group_remove_primitive(m, group_, b);
    }
    let prev = m.u32(group_ + group::PREV);
    let next = m.u32(group_ + group::NEXT);
    if prev == 0 {
        m.set_u32(sys + SYS_GROUPS, next);
    } else {
        m.set_u32(prev + group::NEXT, next);
    }
    let next = m.u32(group_ + group::NEXT);
    if next != 0 {
        let prev = m.u32(group_ + group::PREV);
        m.set_u32(next + group::PREV, prev);
    }
    let name = m.u32(group_ + group::NAME);
    if name != 0 {
        m.free(name);
        m.set_u32(group_ + group::NAME, 0);
    }
    m.free(group_);
}

/// [soft 0x0845c8fc SI_metaSystemDispose]: the system at `*psys` gives up its triangles, its
/// pieces' blocks and the pieces, and its groups (one at a time); `*psys` is cleared. The
/// system's own block is **not** freed.
pub fn meta_system_dispose(m: &mut Mem, psys: u32) {
    let s = m.u32(psys);
    if s == 0 {
        return;
    }
    let tri = m.u32(s + SYS_TRIANGLES);
    if tri != 0 {
        m.free(tri);
        let s = m.u32(psys);
        m.set_u32(s + SYS_TRIANGLES, 0);
    }
    let mut i: i32 = 0;
    while i < m.i32(m.u32(psys) + SYS_PIECE_COUNT) {
        let s = m.u32(psys);
        let slot = m.u32(s + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
        let blk = m.u32(slot);
        if blk != 0 {
            m.free(blk);
            let s = m.u32(psys);
            let slot = m.u32(s + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
            m.set_u32(slot, 0);
        }
        i += 1;
    }
    let s = m.u32(psys);
    let pieces = m.u32(s + SYS_PIECES);
    if pieces != 0 {
        m.free(pieces);
        let s = m.u32(psys);
        m.set_u32(s + SYS_PIECES, 0);
    }
    while m.u32(m.u32(psys) + SYS_GROUPS) != 0 {
        let s = m.u32(psys);
        let g = m.u32(s + SYS_GROUPS);
        meta_system_remove_group(m, s, g);
    }
    m.set_u32(psys, 0);
}

/// [soft 0x0845cce0 si_metaSystemCopy]: a new system for the node `model_` with the box, the
/// resolutions and the show flag of `src` (the other fields as `SI_metaSystemAllocate` left them).
pub fn si_meta_system_copy(m: &mut Mem, src: u32, model_: u32) -> u32 {
    let n = meta_system_allocate(m);
    if n != 0 {
        let w = m.u16(src + SYS_WORD);
        m.set_u16(n + SYS_WORD, w);
        for k in 0..6 {
            copy_f64(m, src + system::BOX + 8 * k, n + system::BOX + 8 * k);
        }
        copy_f32(m, src + SYS_RES_WIRE, n + SYS_RES_WIRE);
        copy_f32(m, src + SYS_RES_RENDER, n + SYS_RES_RENDER);
        m.set_u32(n + system::MODEL, model_);
        let v = m.u16(src + SYS_SHOW);
        m.set_u16(n + SYS_SHOW, v);
    }
    n
}

/// [soft 0x0845e0d0 metaGroupChangeName]: the group's name is freed (if it has one) and
/// replaced by a copy of `name`.
pub fn meta_group_change_name(m: &mut Mem, group_: u32, name: u32) {
    let old = m.u32(group_ + group::NAME);
    if old != 0 {
        m.free(old);
        m.set_u32(group_ + group::NAME, 0);
    }
    let v = m.strdup(name);
    m.set_u32(group_ + group::NAME, v);
}

/// [soft 0x0845e108 metaGroupToggleBlend]: the unblend pair (`a`, `b`) in either order is
/// dropped if the system has it; otherwise a new pair (names null) is put at the head.
pub fn meta_group_toggle_blend(m: &mut Mem, sys: u32, a: u32, b: u32) {
    let mut u = m.u32(sys + SYS_UNBLENDS);
    loop {
        if u == 0 {
            let n = m.si_malloc(unblend::SIZE);
            if n != 0 {
                m.set_u32(n + unblend::GROUP_A, a);
                m.set_u32(n + unblend::GROUP_B, b);
                m.set_u32(n + unblend::NAME_A, 0);
                m.set_u32(n + unblend::NAME_B, 0);
                m.set_u32(n + unblend::PREV, 0);
                let head = m.u32(sys + SYS_UNBLENDS);
                m.set_u32(n + unblend::NEXT, head);
                if head != 0 {
                    m.set_u32(head + unblend::PREV, n);
                }
                m.set_u32(sys + SYS_UNBLENDS, n);
            }
            return;
        }
        let (ua, ub) = (m.u32(u + unblend::GROUP_A), m.u32(u + unblend::GROUP_B));
        if (ua == a && ub == b) || (ub == a && ua == b) {
            break;
        }
        u = m.u32(u + unblend::NEXT);
    }
    let prev = m.u32(u + unblend::PREV);
    let next = m.u32(u + unblend::NEXT);
    if prev == 0 {
        m.set_u32(sys + SYS_UNBLENDS, next);
    } else {
        m.set_u32(prev + unblend::NEXT, next);
    }
    let next = m.u32(u + unblend::NEXT);
    if next != 0 {
        let prev = m.u32(u + unblend::PREV);
        m.set_u32(next + unblend::PREV, prev);
    }
    m.free(u);
}

/// [soft 0x0845e3b4 SI_metaSystemValidateGroups]: each link's flag is 1 when its ball is the
/// definition of one of the system node's sons of kind 8, else 0.
pub fn meta_system_validate_groups(m: &mut Mem, sys: u32) {
    let top = m.u32(sys + system::MODEL);
    let mut g = m.u32(sys + SYS_GROUPS);
    while g != 0 {
        let mut l = m.u32(g + group::LINKS);
        while l != 0 {
            m.set_u16(l + link::FLAG, 0);
            let mut p = 0;
            loop {
                p = next_son(m, top, p);
                if p == 0 {
                    break;
                }
                if m[model(p)].kind == KIND_BALL && m.u32(p + 0x48) == m.u32(l + link::BALL) {
                    m.set_u16(l + link::FLAG, 1);
                }
            }
            l = m.u32(l + link::NEXT);
        }
        g = m.u32(g + group::NEXT);
    }
}

/// "STEP\n", `.rodata` 0x086d5d1d.
const S_STEP: &[u8] = b"STEP\n";
/// "A meta group has been modified due to some corrupted data.", 0x086d5d40.
const S_MODIFIED: u32 = 0x086d_5d40;

/// [soft 0x0845e210 SI_metaSystemResolveGroupName]: after a file read, each link's node name
/// is looked up among the system node's sons of kind 8 (the first with that name): the link
/// gets the ball, the ball its group, and the name is freed. A link left without a ball
/// prints `STEP`. The unblend pairs' group names are resolved the same way (against the
/// groups' names). Then `metaSysGroupErrDetect` repairs what is left, and a message is
/// shown when it found anything.
pub fn meta_system_resolve_group_name(m: &mut Mem, o: &mut dyn Outside, sys: u32) {
    if sys == 0 {
        return;
    }
    let top = m.u32(sys + system::MODEL);
    let mut g = m.u32(sys + SYS_GROUPS);
    while g != 0 {
        let mut l = m.u32(g + group::LINKS);
        while l != 0 {
            let mut p = 0;
            loop {
                p = next_son(m, top, p);
                if p == 0 {
                    break;
                }
                if m[model(p)].kind == KIND_BALL {
                    let name = m.u32(l + link::NAME);
                    let pname = m[model(p)].name.addr();
                    if si_mem::clib::strcmp(m, name, pname) == 0 {
                        let b = m.u32(p + 0x48);
                        m.set_u32(l + link::BALL, b);
                        m.set_u32(b + BALL_GROUP, g);
                        let name = m.u32(l + link::NAME);
                        if name != 0 {
                            m.free(name);
                            m.set_u32(l + link::NAME, 0);
                        }
                        m.set_u32(l + link::NAME, 0);
                        break;
                    }
                }
            }
            if m.u32(l + link::BALL) == 0 {
                o.print(1, S_STEP);
            }
            l = m.u32(l + link::NEXT);
        }
        g = m.u32(g + group::NEXT);
    }
    let mut u = m.u32(sys + SYS_UNBLENDS);
    while u != 0 {
        let mut g = m.u32(sys + SYS_GROUPS);
        while g != 0 {
            let gname = m.u32(g + group::NAME);
            for (name_off, group_off) in [(unblend::NAME_A, unblend::GROUP_A), (unblend::NAME_B, unblend::GROUP_B)] {
                let name = m.u32(u + name_off);
                if name != 0 && si_mem::clib::strcmp(m, name, gname) == 0 {
                    m.set_u32(u + group_off, g);
                    let name = m.u32(u + name_off);
                    if name != 0 {
                        m.free(name);
                        m.set_u32(u + name_off, 0);
                    }
                    m.set_u32(u + name_off, 0);
                }
            }
            g = m.u32(g + group::NEXT);
        }
        u = m.u32(u + unblend::NEXT);
    }
    if meta_sys_group_err_detect(m, sys) != -1 {
        o.si_status_display_message(m, S_MODIFIED, 2);
    }
}

/// [soft 0x0845db74 getGroupsList]: a text list (8 bytes: a short count, a pointer to
/// `count` entries of 0x14 bytes) of the groups starting at `first`: each entry has two
/// zero shorts, a copy of the name, the group's place, the group, and a zero word.
pub fn get_groups_list(m: &mut Mem, first: u32) -> u32 {
    let mut n: u32 = 0;
    let mut g = first;
    while g != 0 {
        n += 1;
        g = m.u32(g + group::NEXT);
    }
    let list = m.si_malloc(8);
    m.set_u16(list, n as u16);
    let entries = m.si_calloc(n, 0x14);
    m.set_u32(list + 4, entries);
    let mut i: i32 = 0;
    let mut g = first;
    let mut off: u32 = 0;
    while i < m.i16(list) as i32 {
        let name = m.u32(g + group::NAME);
        let copy = m.strdup(name);
        let e = m.u32(list + 4);
        m.set_u32(e + 4 + off, copy);
        let e = m.u32(list + 4);
        m.set_u16(e + off, 0);
        m.set_u16(e + 2 + off, 0);
        m.set_i32(e + 8 + off, i);
        m.set_u32(e + 0xc + off, g);
        m.set_u32(e + 0x10 + off, 0);
        i += 1;
        g = m.u32(g + group::NEXT);
        off += 0x14;
    }
    list
}

/// [soft 0x0845abf8 SI_metaballTouch]: the ball's box is put into the system's (`sys`, or
/// the one the ball hangs under when null; nothing if there is none), the system's surface
/// and pieces are freed and its node's render data cleared (the counts at +0x228 and +0x230,
/// the blocks at +0x22c, +0x234, +0x330), and every window's fast-play buffers are marked.
pub fn metaball_touch(m: &mut Mem, o: &mut dyn Outside, ball_: u32, sys: u32) {
    let mut sys = sys;
    if sys == 0 {
        let parent = modelball::get_parent_system(m, ball_);
        if parent == 0 {
            return;
        }
        sys = m.u32(parent + 0x48);
    }
    if ball_ != 0 {
        modelball::check_bounding_box(m, ball_, sys + system::BOX, 0.0);
    }
    let tri = m.u32(sys + SYS_TRIANGLES);
    if tri != 0 {
        m.set_u32(sys + SYS_TRI_COUNT, 0);
        m.free(tri);
        m.set_u32(sys + SYS_TRIANGLES, 0);
    }
    if m.u32(sys + SYS_PIECE_COUNT) != 0 {
        let mut i: i32 = 0;
        if 0 < m.i32(sys + SYS_PIECE_COUNT) {
            loop {
                let slot = m.u32(sys + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
                let blk = m.u32(slot);
                if blk != 0 {
                    m.free(blk);
                    let slot = m.u32(sys + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
                    m.set_u32(slot, 0);
                }
                i += 1;
                if i >= m.i32(sys + SYS_PIECE_COUNT) {
                    break;
                }
            }
        }
        m.set_u32(sys + SYS_PIECE_COUNT, 0);
        let pieces = m.u32(sys + SYS_PIECES);
        if pieces != 0 {
            m.free(pieces);
            m.set_u32(sys + SYS_PIECES, 0);
        }
        m.set_u32(sys + SYS_PIECES, 0);
    }
    m.set_u16(sys + SYS_SHORT_44, 0);
    let node = m.u32(sys + system::MODEL);
    m.set_u32(node + 0x228, 0);
    for (off, count_off) in [(0x22c, None), (0x234, Some(0x230))] {
        if let Some(c) = count_off {
            m.set_u32(node + c, 0);
        }
        let blk = m.u32(node + off);
        if blk != 0 {
            m.free(blk);
            m.set_u32(node + off, 0);
        }
    }
    let blk = m.u32(m.u32(sys + system::MODEL) + 0x330);
    if blk != 0 {
        m.free(blk);
        let node = m.u32(sys + system::MODEL);
        m.set_u32(node + 0x330, 0);
    }
    o.si_fast_set_dirty(m, 0, -1, 1);
}

/// `SI_procData (0x08679314)`, a pointer to the process record whose word at +4 is the
/// current graf list's index; `SI_grafLists (0x08679498)`, a pointer to the lists (0x24 bytes each).
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
/// Model names the new systems are allocated under, and the two messages.
const NAME_SYSTEM: u32 = 0x086d_5cac;
const NAME_ADDED_SYSTEM: u32 = 0x086d_5cb6;
const MSG_MANY: u32 = 0x086d_5c80;
const MSG_TOO_MANY: u32 = 0x086d_5c40;

/// The address of the current graf list (`SI_grafLists + SI_procData[1] * 0x24`).
pub(crate) fn current_list(m: &Mem) -> u32 {
    let idx = m.u32(m.u32(SI_PROC_DATA) + 4);
    m.u32(SI_GRAF_LISTS).wrapping_add(idx.wrapping_mul(0x24))
}

/// [soft 0x0845c9c0 SI_getActiveMetaSystem]: the one metasystem the current list's selected
/// metaball or system nodes (entry types 2 and 3, selection 1 or 3) belong to; failing that
/// the one system node present at all. Two or more of either: a message and 0. None, and
/// `create` not 0: a new system with a new node, added to the list.
pub fn si_get_active_meta_system(m: &mut Mem, o: &mut dyn Outside, create: i16) -> u32 {
    let (mut sel_sys, mut seen_sys, mut sel_n, mut seen_n) = (0u32, 0u32, 0i32, 0i32);
    let mut e = m.u32(current_list(m).wrapping_add(4));
    loop {
        let ent = e;
        if ent == 0 {
            break;
        }
        e = m.u32(ent + 0x1c);
        if (m.i32(ent) as u32).wrapping_sub(2) > 1 {
            continue;
        }
        let mut node = m.u32(ent + 4);
        let sel = m.i32(ent + 8);
        if sel == 1 || sel == 3 {
            let mut kind = m.i32(node + 0x30);
            if (kind as u32).wrapping_sub(7) > 1 || node == 0 {
                continue;
            }
            let mut up = false;
            while kind != 7 {
                node = m.u32(node + 0x2bc);
                if node == 0 {
                    up = true;
                    break;
                }
                kind = m.i32(node + 0x30);
            }
            if up {
                continue;
            }
            let sys = m.u32(node + 0x48);
            let changed = sel_sys != sys;
            sel_sys = sys;
            if changed {
                sel_n += 1;
            }
        } else if m.i32(node + 0x30) == 7 {
            let sys = m.u32(node + 0x48);
            let changed = seen_sys != sys;
            seen_sys = sys;
            if changed {
                seen_n += 1;
            }
        }
    }
    if sel_n == 1 {
        return sel_sys;
    }
    if sel_n >= 2 {
        o.si_status_display_message(m, MSG_TOO_MANY, 2);
        return 0;
    }
    if seen_n == 1 {
        return seen_sys;
    }
    if seen_n >= 2 {
        o.si_status_display_message(m, MSG_MANY, 2);
        return 0;
    }
    if seen_n != 0 || create == 0 {
        return 0;
    }
    let sys = meta_system_allocate(m);
    let node = o.si_model_allocate(m, NAME_SYSTEM);
    m.set_u32(sys + system::MODEL, node);
    o.si_model_add_definition(m, node, 7, sys);
    let list = current_list(m);
    o.si_graflist_add_data(m, list, node, 2, 0);
    sys
}

/// [soft 0x0845cd68 SI_addMetaSystem]: a new system with its node, added to the current list as
/// the only selection; the windows are refreshed.
pub fn si_add_meta_system(m: &mut Mem, o: &mut dyn Outside) {
    let sys = meta_system_allocate(m);
    if sys == 0 {
        return;
    }
    let node = o.si_model_allocate(m, NAME_ADDED_SYSTEM);
    m.set_u32(sys + system::MODEL, node);
    o.si_model_add_definition(m, node, 7, sys);
    o.si_back_selected(m, 0, 1);
    let list = current_list(m);
    o.si_graflist_add_data(m, list, node, 2, 1);
    o.si_refresh_system(m, 0, 0, 0x25);
    o.si_refresh_counter(m);
}

/// [soft 0x0845e1b8 SI_metaballAddInSystem]: the node becomes a son of the system's node (the
/// system is the active one when `sys` is null; nothing if there is none).
pub fn si_metaball_add_in_system(m: &mut Mem, o: &mut dyn Outside, node: u32, sys: u32) {
    let mut sys = sys;
    if sys == 0 {
        sys = si_get_active_meta_system(m, o, 1);
        if sys == 0 {
            return;
        }
    }
    let list = current_list(m);
    let parent = o.si_find_gl_data(m, list, m.u32(sys + system::MODEL), 2);
    o.si_model_create_parent(m, parent, node, 0);
}
