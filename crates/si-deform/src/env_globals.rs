// SPDX-License-Identifier: MIT
//! The global variables the three envelope source files read beyond those of
//! [`crate::globals`], at their addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-deform`. (`SI_identite`, `SI_procData`,
//! `SI_elemDicts` and `SI_cid` are declared in [`crate::globals`] and used from there.)

use crate::env_records::EditVert;
use crate::envelopset::JntList;
use si_mem::{Mem, Opaque, Ptr32};
use si_types::model::Model;

si_mem::globals! {
    /// The animation settings (0x40 bytes by the symbol table). Read here: the 16-bit words
    /// at [`GLB_ANIM_06`] and [`GLB_ANIM_PLAYING`].
    SI_GLB_ANIM: [u16; 0x20] = 0x08781480, "SI_glbAnim";
    /// The manipulation settings (0x3c bytes). Its first word is 1 under classical scaling:
    /// a skin's scaling is then read from `Model` +0x3a0 instead of the global scaling.
    SI_GLB_MANIP: [u8; 0x3c] = 0x08781560, "SI_glbManip";
    /// Bits that hold the viewer's update back; `SI_envelopSkelUpdate` sets bit 1 around its
    /// update of a chain and puts the word back.
    SI_VIEWER_UPD_OPT: i32 = 0x08679678, "SI_viewerUpdOpt", init 0;
    /// Not 0 while the user drags something.
    SI_INTERACTIVE: i16 = 0x0867953e, "SI_interactive", init 0;
    /// Not 0: a patch skin is not tessellated anew while the user drags.
    SI_FAST_INTENV: i16 = 0x0867953c, "SI_fastIntenv", init 0;
    /// The number of the point under the cursor while points are moved by hand.
    SI_MDL_PNT: i32 = 0x0878897c, "SI_mdlPnt";
    /// `envelop.c`'s chain of points edited by hand (`editVertAllocate`, `editVertDispose`).
    EDITVERT: Ptr32<EditVert> = 0x086d72fc, "editvert", init 0;
    /// `envelop.c`'s table of one 16-bit word per point of the skin being deformed
    /// (`dampTabCreate`; freed by `envelopSkel`).
    DAMPTAB: Ptr32<i16> = 0x086d7300, "damptab", init 0;
    /// `envelop.c`'s copy of a rigidly carried node's scaling and rest rotation and
    /// translation: nine floats from `SI_calloc` (`envelopSkel`).
    CURVECT: Ptr32<[f32; 9]> = 0x086d7304, "curvect", init 0;
    // second pass (finding 0029, section 14)
    /// `envelopset.c`'s list of the bones of the skeleton being assigned to
    /// (`envelopSkelBuild`), each element with the node its bone runs to.
    SKEL: Ptr32<JntList> = 0x086d7324, "skel", init 0;
    /// `envelopset.c`'s node being searched for in the skeleton's lists
    /// (`envPrecInListFind` writes it, `SI_envelopAddNewParts` reads it).
    PREC: Ptr32<Model> = 0x086d7320, "prec", init 0;
    /// How many of the nearest bones a point that lies in no bone's volume is shared
    /// between (2 in the binary's data).
    SI_ASSGN_JNTS_DEF_NB: i32 = 0x086d74d4, "SI_AssgnJntsDefNb", init 2;
    /// A mask applied to the display colour a point takes from its main bone.
    SI_SELECT7_WM: u16 = 0x08781638, "SI_select7WM";
    /// Not 0 while a scene is converted for another program: the assignment then leaves
    /// the points' display colours alone.
    SI_ED_SUMATRA_TO_3X: i16 = 0x08680e50, "SI_edSumatraTo3x", init 0;
    /// The colour bits a display node keeps when a bone's colour is put on it.
    SI_UNSELECT_WM: u16 = 0x087817c4, "SI_unselectWM";
    /// The first, second, fourth and fifth of the colours a point is marked with (the
    /// third, sixth and seventh are below): their words follow one another from 0x0878162c.
    SI_SELECT1_WM: u16 = 0x0878162c, "SI_select1WM";
    SI_SELECT2_WM: u16 = 0x0878162e, "SI_select2WM";
    SI_SELECT4_WM: u16 = 0x08781632, "SI_select4WM";
    SI_SELECT5_WM: u16 = 0x08781634, "SI_select5WM";
    /// The colour the bones of a skeleton start with, and the one after
    /// [`SI_SELECT6_WM`].
    SI_SELECT3_WM: u16 = 0x08781630, "SI_select3WM";
    /// The last colour of the bones' cycle.
    SI_SELECT6_WM: u16 = 0x08781636, "SI_select6WM";
    /// What is added to a bone's colour to make the next bone's.
    SI_SELECT_STEP: u16 = 0x0878163a, "SI_selectStep";
    /// The display colour a skin's point takes when a weight is set on no bone
    /// (`SI_envelopVertWeightUpdate`; `si-ui` declares it too).
    SI_OBJECT_WM: u16 = 0x087815b0, "SI_objectWM";
}

/// Index in [`SI_GLB_ANIM`] of the word at +0x06: not 0 while the scene update is inside an
/// animation step (finding 0015).
pub const GLB_ANIM_06: usize = 3;
/// Index in [`SI_GLB_ANIM`] of the word at +0x38: not 0 while a sequence is played or
/// rendered. The envelope then updates a skeleton once a frame and may take the rigid path
/// for a joint without blend.
pub const GLB_ANIM_PLAYING: usize = 0x1c;

/// Writes the initial values of the globals declared here (all zero in the binary).
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
}

#[allow(dead_code)]
fn _opaque(_: Ptr32<Opaque>) {}
