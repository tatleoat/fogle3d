// SPDX-License-Identifier: MIT
//! `globals.c`: the initialisers of five global parameter records, and three small helpers.
//!
//! What the fields of these records mean is not established (the routines that read them
//! belong to other packages), so each record is kept as a memory image at the original's
//! offsets: every offset written here is named by its number only. The images can be
//! compared with the original's memory byte for byte, and a typed record can replace each
//! once its users are read.

use crate::env::World;
use crate::image::adapter::{scratch, AppOutside};
use crate::image::g;
use crate::image::globals_c::{animation_info_init, duplication_info_init, increment_info_init, manipulation_init, schematic_info_init};
use crate::state::App;

macro_rules! image {
    ($(#[$m:meta])* $name:ident, $size:expr) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub [u8; $size]);
        impl Default for $name {
            fn default() -> $name {
                $name([0; $size])
            }
        }
        impl $name {
            pub fn put16(&mut self, offset: usize, v: i16) {
                self.0[offset..offset + 2].copy_from_slice(&v.to_le_bytes());
            }
            pub fn put32(&mut self, offset: usize, v: u32) {
                self.0[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
            }
            pub fn get16(&self, offset: usize) -> i16 {
                i16::from_le_bytes([self.0[offset], self.0[offset + 1]])
            }
            pub fn get32(&self, offset: usize) -> u32 {
                u32::from_le_bytes([self.0[offset], self.0[offset + 1], self.0[offset + 2], self.0[offset + 3]])
            }
        }
    };
}

image!(
    /// `SI_glbManip` (0x08781560, 0x3c bytes). +0x2a is the scaling preference and +0x36
    /// the "optimise untold relations" switch that scene loading reads (finding 0018).
    ManipulationInfo, 0x3c
);
image!(
    /// `SI_glbAnim` (0x08781480, 0x40 bytes).
    AnimationInfo, 0x40
);
image!(
    /// `SI_glbIncrem` (0x08781520, 0x40 bytes, the first 0x30 are twelve floats).
    IncrementInfo, 0x40
);
image!(
    /// `SI_glbDuplic` (0x087814c0, 0x60 bytes).
    DuplicationInfo, 0x60
);
image!(
    /// `SI_glbSchem` (0x087fb960; 0x50 bytes are written). +0x3c is the "a scene is being
    /// loaded" word of finding 0018 (0x087fb99c) and +0x48 the word saved around a load.
    SchematicInfo, 0x50
);

impl<W: World> App<W> {
    // adapter: not in place
    /// `SI_manipulationInit (0x080d3210)`: the image port
    /// ([`crate::image::globals_c::manipulation_init`]) run on a scratch image; the record
    /// is copied into `g.manip`, the environment asked through the world.
    pub fn manipulation_init(&mut self) {
        let mut m = scratch();
        m.set_bytes(g::SI_GLB_MANIP.0, &self.g.manip.0);
        manipulation_init(&mut m, &mut AppOutside(&mut self.w));
        self.g.manip.0.copy_from_slice(m.bytes(g::SI_GLB_MANIP.0, 0x3c));
    }

    // adapter: not in place
    /// `SI_animationInfoInit (0x080d335c)`: the image port, as [`App::manipulation_init`].
    pub fn animation_info_init(&mut self) {
        let mut m = scratch();
        m.set_bytes(g::SI_GLB_ANIM.0, &self.g.anim.0);
        animation_info_init(&mut m, &mut AppOutside(&mut self.w));
        self.g.anim.0.copy_from_slice(m.bytes(g::SI_GLB_ANIM.0, 0x40));
    }

    // adapter: not in place
    /// `SI_incrementInfoInit (0x080d347c)`: the image port; the record's first 0x30 bytes
    /// (the rest of the struct form's 0x40 is not the original's).
    pub fn increment_info_init(&mut self) {
        let mut m = scratch();
        m.set_bytes(g::SI_GLB_INCREM.0, &self.g.increm.0[..0x30]);
        increment_info_init(&mut m);
        self.g.increm.0[..0x30].copy_from_slice(m.bytes(g::SI_GLB_INCREM.0, 0x30));
    }

    // adapter: not in place
    /// `SI_duplicationInfoInit (0x080d34f8)`: the image port; the record's 0x58 bytes.
    pub fn duplication_info_init(&mut self, all: i16) {
        let mut m = scratch();
        m.set_bytes(g::SI_GLB_DUPLIC.0, &self.g.duplic.0[..0x58]);
        duplication_info_init(&mut m, all);
        self.g.duplic.0[..0x58].copy_from_slice(m.bytes(g::SI_GLB_DUPLIC.0, 0x58));
    }

    // adapter: not in place
    /// `SI_schematicInfoInit (0x080d364c)`: the image port.
    pub fn schematic_info_init(&mut self) {
        let mut m = scratch();
        m.set_bytes(g::SI_GLB_SCHEM.0, &self.g.schem.0);
        schematic_info_init(&mut m);
        self.g.schem.0.copy_from_slice(m.bytes(g::SI_GLB_SCHEM.0, 0x50));
    }

    /// [soft 0x080d3738 si_IsIGLOO]: whether the graphics layer answers 1 to inquiry 0x5f;
    /// never in the batch renderer. (`si-gl`'s `getgdesc` answers 0 to it.)
    pub fn is_igloo(&mut self) -> bool {
        if self.g.standalone_renderer == 0 {
            self.w.getgdesc(0x5f) == 1
        } else {
            false
        }
    }

    /// [soft 0x080d3768 okSboxfi]: a filled screen box whose upper right corner is one
    /// pixel further out.
    pub fn ok_sboxfi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.w.sboxfi(x1, y1, x2.wrapping_add(1), y2.wrapping_add(1));
    }
}
