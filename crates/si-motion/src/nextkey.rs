// SPDX-License-Identifier: MIT
//! The key browser: "next key" and "previous key" over the selected curves or objects
//! (`motionUtils.c` and `motion.c`; finding 0017, section 9f).
//!
//! A step of the browser looks, in every curve that takes part, for the key frame nearest
//! to the current frame in the direction asked for, and makes the best of them the current
//! frame. The frames it may land on are the play range, or the dope sheet's region when one
//! is set. With "wrap" a curve that has no key left in that direction offers its first
//! (last) key of the range instead, and a key reached by going round counts as farther away
//! than any key reached directly.

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::off::{arel, elem, grafnode, list, rellist};
use crate::play::{self, PlayHost};
use crate::utils::{self, UtilsHost};
use si_mem::{Addr, Mem};
use si_x87::F80;

/// The dope sheet's region, at the word [`glob::DOPE_ACTIVE_CHECK`] (0x0878faa8; no symbol of
/// its own): the 16-bit word +0 is set while a region is selected, +4 is its first frame and
/// +6 the frame behind its last (both 16 bits, signed). Read from `elemdictNextKeyElemAnim`
/// and `SI_nextKeyDisplay`; `SI_playbackRun` narrows the play range the same way.
pub mod region {
    pub const ON: u32 = 0x00;
    pub const START: u32 = 0x04;
    pub const END: u32 = 0x06;
}

/// `SI_scngl`: a pointer to the graf list of the curve editor (its nodes are curves).
pub const SI_SCNGL: Addr = 0x0869_5e9c;

/// Texts of [`next_key_display`] (the status line while the browser runs).
pub mod text {
    use si_mem::Addr;
    /// "Browsing through selected fcurve…" (0x086962e0) and "Browsing through selected
    /// object…" (0x08696320).
    pub const CURVES: Addr = 0x0869_62e0;
    pub const OBJECTS: Addr = 0x0869_6320;
}

crate::host_trait! {
    /// What the key browser calls outside this package.
    pub trait BrowseHost: PlayHost, UtilsHost {
        /// `SI_windowCamActive(id)` (0x083d6390): non-zero when a window of that kind is
        /// open (0x7d4: the curve editor).
        fn window_cam_active(id: i32) -> i32;
        /// `SI_graflistCountSelected(graf list, &nodes, flag)` (0x0832db90).
        fn graflist_count_selected(gl: Addr, nodes: Addr, flag: i32) -> i32;
        /// `SI_viewerGetScene()` (0x08303af0): the stand-alone viewer's element dictionary.
        fn viewer_get_scene() -> Addr;
        /// `SI_sceneFrameUpdate(frame)` (0x08222438; ported as
        /// [`crate::update::scene_frame_update`]): taken through the host for the reason
        /// [`PlayHost::scene_update`] is.
        fn scene_frame_update(frame: i32);
        /// `SI_refreshSelectedBothBuffers(flag)` (0x08325d70).
        fn refresh_selected_both_buffers(flag: i32);
    }
}

/// The frames the browser may land on: the dope sheet's region when one is set, else the
/// play range (`SI_frameStart`, `SI_frameEnd`).
fn range(m: &Mem) -> (i32, i32) {
    let r = glob::DOPE_ACTIVE_CHECK;
    if m.i16(r + region::ON) != 0 {
        (m.i16(r + region::START) as i32, (m.i16(r + region::END) as i32).wrapping_sub(1))
    } else {
        (m.i32(glob::SI_FRAME_START), m.i32(glob::SI_FRAME_END))
    }
}

/// One curve's offer to the browser [soft 0x0822de08 si_fcvNextBestKey]: the best frame so
/// far (`*best`) is replaced when the curve has a better one. Returns 1 when the curve has a
/// key the browser could go to at all, 0 when not (the best frame is then left alone).
///
/// With `cur` the current frame, and a key frame k as `SI_fcurveNextKey` /
/// `SI_fcurvePreviousKey` give it (the first key after, the last key before a frame, keys
/// of type 3 left out):
///
/// - **forward, no wrap**: k is the first key after `cur`; it replaces a best frame above it.
/// - **backward, no wrap**: k the last key before `cur`; it replaces a best frame below it.
/// - **forward, wrap**: k is the first key after `cur` if that lies in the range (k ≤ `last`),
///   else the first key after `first − 1` if that does; else the curve has no offer. A k
///   ahead of `cur` replaces a best frame that is `cur` itself, behind `cur`, or farther
///   ahead than k. A k at or behind `cur` (reached by going round) replaces a best frame
///   that is `cur` itself or beyond `last`, and a best frame behind `cur` that lies after k;
///   it never replaces a best frame ahead of `cur`.
/// - **backward, wrap**: the mirror image, with the last key before `cur` if it is in the
///   range (k ≥ `first`), else the last key before `last + 1`.
///
/// The browser starts with a best frame of 0x7fffffff going forward and 0x80000002 going
/// backward, so the first offer always wins.
pub fn fcv_next_best_key(m: &mut Mem, curve: Addr, forward: i16, wrap: i16, first: i32, last: i32, best: Addr) -> i16 {
    let (found, frame) = next_best_key(m, curve, forward, wrap, first, last, m.i32(best));
    m.set_i32(best, frame);
    found
}

/// [`fcv_next_best_key`] on values: (found, the best frame afterwards).
fn next_best_key(m: &Mem, curve: Addr, forward: i16, wrap: i16, first: i32, last: i32, best: i32) -> (i16, i32) {
    let cur = m.i32(glob::SI_FRAME_CURRENT);
    if forward != 0 {
        if wrap == 0 {
            return match fcv::next_key(m, curve, cur) {
                None => (0, best),
                Some(k) => (1, if k < best { k } else { best }),
            };
        }
        let k = match fcv::next_key(m, curve, cur) {
            Some(k) if k <= last => k,
            _ => match fcv::next_key(m, curve, first.wrapping_sub(1)) {
                Some(k) if k <= last => k,
                _ => return (0, best),
            },
        };
        let take = if k > cur {
            best <= cur || k < best
        } else {
            best == cur || best > last || (best < cur && k < best)
        };
        (1, if take { k } else { best })
    } else {
        if wrap == 0 {
            return match fcv::previous_key(m, curve, cur) {
                None => (0, best),
                Some(k) => (1, if k > best { k } else { best }),
            };
        }
        let k = match fcv::previous_key(m, curve, cur) {
            Some(k) if k >= first => k,
            _ => match fcv::previous_key(m, curve, last.wrapping_add(1)) {
                Some(k) if k >= first => k,
                _ => return (0, best),
            },
        };
        let take = if k < cur {
            best >= cur || k > best
        } else {
            best == cur || best < first || (best > cur && k > best)
        };
        (1, if take { k } else { best })
    }
}

/// The offers of one element's curves [soft 0x0822e118 elemdictNextKeyElemAnim]: every
/// forward relation to the ANIMATION chapter except the animation-file links (15000 to
/// 15010), in list order; a curve takes part when its type is selected (in the attribute
/// table, or with `dope` in the dope sheet's own table). Returns 1 when one of them had an
/// offer. The chapter and the index are not used.
///
/// A relation to a deleted ANIMATION element is read through a null pointer by the
/// original; the image records a fault.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_next_key_elem_anim<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, _chap: i32, _index: i32, forward: i16,
                                                          wrap: i16, dope: i16, best: Addr)
                                                          -> i16 {
    let mut found: i16 = 0;
    let mut frame = m.i32(best);
    let (first, last) = range(m);
    let mut g = 0;
    while g < m.i32(e.wrapping_add(elem::FORWARD)) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            if m.i16(r + arel::CHAPID) as i32 == glob::chap(m, cid::ANIMATION) && (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) > 10 {
                let ce = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, m.i32(r + arel::ELEMID));
                let c = m.ptr(ce.wrapping_add(elem::DATA));
                if c != 0 {
                    let selected = if dope != 0 {
                        m.i16(utils::SI_DOPE_FCV_TABLE.wrapping_add((fcv::kind(m, c) as u32).wrapping_mul(2)))
                    } else {
                        h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c))
                    };
                    if selected != 0 {
                        let (f, b) = next_best_key(m, c, forward, wrap, first, last, frame);
                        frame = b;
                        found |= f;
                    }
                }
            }
            i += 1;
        }
        g += 1;
    }
    m.set_i32(best, frame);
    found
}

/// The offers of an element and of what hangs on it [soft 0x0822c410 SI_edNextKeyElemAnim].
///
/// With `dope` the dope sheet's table is filled first (`SI_setDopFcvTableFromDopeMode`).
/// For a MODELS element the parts come first, each found through list 2: the SHAPES element
/// (relation 200), every CLUSTERS element (700), the node lattice (210) and the branch
/// lattice (215); without `dope` the attribute table is set to the part's chapter before
/// its curves are asked (`SI_setFcvTableEntriesFromChap`). Then the element itself, with the
/// table set to its chapter (not for a model under `dope`); its own curves are filtered by
/// the dope sheet's table only when it is a model. Returns 1 when any curve had an offer.
#[allow(clippy::too_many_arguments)]
pub fn ed_next_key_elem_anim<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, index: i32, forward: i16, wrap: i16,
                                                    dope: i16, best: Addr)
                                                    -> i32 {
    if dope != 0 {
        h.set_dop_fcv_table_from_dope_mode(m);
    }
    let models = glob::chap(m, cid::MODELS);
    let mut any = false;
    if chap == models {
        let part = |m: &mut Mem, h: &mut H, pe: Addr, pchap: i32, pindex: i32| -> bool {
            if dope == 0 {
                h.set_fcv_table_entries_from_chap(m, pchap);
            }
            elemdict_next_key_elem_anim(m, h, d, pe, pchap, pindex, forward, wrap, dope, best) != 0
        };
        let shapes = glob::chap(m, cid::SHAPES);
        let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
        if r.elem != 0 {
            any = part(m, h, r.elem, shapes, r.index);
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut r = ed::find_related_elem(m, d, e, true, 2, 700, clusters);
        while r.elem != 0 {
            let got = part(m, h, r.elem, clusters, r.index);
            any = got || any;
            r = ed::find_next_related_elem(m, d, e, true, 2, 700, clusters);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for relid in [0xd2, 0xd7] {
            let r = ed::find_related_elem(m, d, e, true, 2, relid, lattices);
            if r.elem != 0 {
                let got = part(m, h, r.elem, lattices, r.index);
                any = got || any;
            }
        }
    }
    if dope == 0 || chap != models {
        h.set_fcv_table_entries_from_chap(m, chap);
    }
    let own = if dope != 0 { (chap == models) as i16 } else { 0 };
    let got = elemdict_next_key_elem_anim(m, h, d, e, chap, index, forward, wrap, own, best) != 0;
    (got || any) as i32
}

/// One step of the key browser [soft 0x0820eeac SI_nextKeyDisplay]: `forward` next key (else
/// previous), `wrap` going round at the ends of the range, `refresh` with the scene brought
/// to the new frame (else only the frame slider moves). Returns 1 when something that could
/// have keys was selected (and 1 at once in play-box mode −1; 0 for a stand-alone viewer
/// without a scene).
///
/// 1. The range is the play range, or in the program (play-box mode 0) the dope sheet's
///    region when one is set; in that case a current frame outside the region is first moved
///    to just outside its nearer end (start − 1, or the frame behind the last).
/// 2. Who is asked. In the program, when the curve editor is open (window kind 0x7d4) and
///    has selected curves, those: the nodes of `SI_scngl`, each node's object being a curve
///    ("Browsing through selected fcurve…"). Otherwise the nodes of the current graf list,
///    each through its element ([`ed_next_key_elem_anim`]). A node takes part when it is
///    selected (+8 non-zero); a node whose word +0x16 has bit 0x1000 takes part by the bits
///    0xe000 of that word instead. In the stand-alone viewer every live element of every
///    chapter of the viewer's dictionary is asked.
/// 3. If a key was found and it lies in the range, it becomes the current frame: with
///    `refresh` the scene is updated at it (`SI_sceneFrameUpdate`; in the viewer
///    `SI_viewerUpdateScene` and the slider); without, the key marks are set for it
///    (`SI_sceneKeySetColor((float)frame / rate, 1, 0)`, only in the program and with key
///    colouring on, followed by `SI_refreshSelectedBothBuffers(0)`) and the slider drawn.
///    Otherwise the current frame is put back as it was on entry.
/// 4. In the program an open curve dialogue is updated.
///
/// The two locals the element look-up writes through start as zero here; the original's
/// hold what its stack held when the look-up finds an element without writing them.
pub fn next_key_display<H: BrowseHost + ?Sized>(m: &mut Mem, h: &mut H, forward: i16, wrap: i16, refresh: i16) -> i16 {
    if m.u8(play::g::SI_PLAY_BOX_MODE) == 0xff {
        return 1;
    }
    let saved = m.i32(glob::SI_FRAME_CURRENT);
    let r = glob::DOPE_ACTIVE_CHECK;
    let (first, last);
    if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 && m.i16(r + region::ON) != 0 {
        let start = m.i16(r + region::START) as i32;
        if saved < start {
            m.set_i32(glob::SI_FRAME_CURRENT, start - 1);
        } else {
            let end = m.i16(r + region::END) as i32;
            if saved > end - 1 {
                m.set_i32(glob::SI_FRAME_CURRENT, end);
            }
        }
        first = m.i16(r + region::START) as i32;
        last = m.i16(r + region::END) as i32 - 1;
    } else {
        first = m.i32(glob::SI_FRAME_START);
        last = m.i32(glob::SI_FRAME_END);
    }
    let best = m.local(4);
    m.set_i32(best, if forward != 0 { 0x7fff_ffff } else { 0x8000_0002u32 as i32 });
    let mut curves = false;
    if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 && h.window_cam_active(m, 0x7d4) != 0 {
        let scngl = m.ptr(SI_SCNGL);
        if h.graflist_count_selected(m, scngl, 0, 1) > 0 {
            curves = true;
        }
    }
    let gl = if curves { m.ptr(SI_SCNGL) } else { glob::cur_graf_list(m) };
    crate::key::KeyHost::status_display_message(h, m, if curves { text::CURVES } else { text::OBJECTS }, 0);
    let mut found: i16 = 0;
    let mut has: i16 = 0;
    if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 {
        let mut gn = m.ptr(gl + 4);
        while gn != 0 {
            let w = m.u16(gn + 0x16);
            let takes_part = if w & 0x1000 == 0 { m.i32(gn + grafnode::SELECTION) != 0 } else { w & 0xe000 != 0 };
            if takes_part {
                has = 1;
                if curves {
                    let c = m.ptr(gn + grafnode::DATA);
                    found |= fcv_next_best_key(m, c, forward, wrap, first, last, best);
                } else {
                    let out = m.local(8);
                    let e = h.elemdict_find_elem_from_graf_node(m, gn, out, out + 4);
                    if e != 0 {
                        let (chap, index) = (m.i32(out), m.i32(out + 4));
                        let d = glob::cur_dict(m);
                        let got = ed_next_key_elem_anim(m, h, d, e, chap, index, forward, wrap, 0, best) as i16;
                        found = (got != 0 || found != 0) as i16;
                    }
                    m.release(8);
                }
            }
            gn = m.ptr(gn + grafnode::NEXT);
        }
    } else {
        let scene = h.viewer_get_scene(m);
        if scene == 0 {
            m.release(4);
            return 0;
        }
        let mut c = 0;
        while c < m.i32(scene) {
            let l = m.ptr(scene + 4).wrapping_add((c as u32).wrapping_mul(list::SIZE));
            let mut i = 0;
            while i < m.i32(l + list::NB_ELEMS) {
                let e = m.ptr(l + list::ELEMS).wrapping_add((i as u32).wrapping_mul(elem::SIZE));
                if m.i16(e + elem::STATUS) == 0 {
                    has = 1;
                    let got = ed_next_key_elem_anim(m, h, scene, e, c, i, forward, wrap, 0, best) as i16;
                    found = (got != 0 || found != 0) as i16;
                }
                i += 1;
            }
            c += 1;
        }
    }
    let frame = m.i32(best);
    m.release(4);
    if found != 0 && frame >= first && frame <= last {
        m.set_i32(glob::SI_FRAME_CURRENT, frame);
        if refresh != 0 {
            if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 {
                h.scene_frame_update(m, frame);
            } else {
                h.viewer_update_scene(m, frame);
                let now = m.i32(glob::SI_FRAME_CURRENT);
                h.fslider_draw(m, now);
            }
        } else {
            if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 && m.i16(glob::SI_GLB_ANIM + glob::anim::KEY_COLOR) != 0 {
                // (float)frame, made whole in the register (it is), stored as a float, divided
                let t = (F80::from_f32(F80::from_i32(frame).to_f32()) / glob::frame_rate(m)).to_f32();
                h.scene_key_set_color(m, t, 1, 0);
                h.refresh_selected_both_buffers(m, 0);
            }
            let now = m.i32(glob::SI_FRAME_CURRENT);
            h.fslider_draw(m, now);
        }
    } else {
        m.set_i32(glob::SI_FRAME_CURRENT, saved);
    }
    if m.u8(play::g::SI_PLAY_BOX_MODE) == 0 && m.i16(play::g::SI_DIALOG_ACTIVE) != 0 {
        h.dialog_update_fcv(m);
    }
    has
}
