// SPDX-License-Identifier: MIT
//! The constants the image-form modules read or hand on by address: string literals of
//! `constraint.c` and `skeleton.c`. The image holds none of the binary's data; a crate
//! stores what it owns. Every entry is held against the binary by
//! `re/tools/si-kine/check_rodata.py`.

use si_mem::{CStr32, Mem, Ptr32, RODATA};

/// What `si_norConstrComputeNormalVec` puts on the status line for a patch.
pub const S_PATCH_NOT_DONE: CStr32 = Ptr32::new(0x08694c60);

/// The name `si_constr3PntsFormFace` gives its object.
pub const S_FACE: CStr32 = Ptr32::new(0x08694c82);

/// The status line's texts of setting constraints.
pub const S_NO_CLUSTER: CStr32 = Ptr32::new(0x08694c87);
pub const S_HAS_CLUSTER_CNS: CStr32 = Ptr32::new(0x08694ca0);
pub const S_NO_ACTIVE_CLUSTER: CStr32 = Ptr32::new(0x08694ce0);
pub const S_NO_POSLIM_FOR_JOINT: CStr32 = Ptr32::new(0x08694d20);
pub const S_WAVES: CStr32 = Ptr32::new(0x08694d3f);
/// The question `SI_itemSetConstraint` asks about an item on a path: three lines and the
/// two buttons.
pub const S_ON_PATH: [CStr32; 5] = [Ptr32::new(0x08694e00), Ptr32::new(0x08694dc0), Ptr32::new(0x08694d80), Ptr32::new(0x08694d62), Ptr32::new(0x08694d5b)];

/// The name of an automatic envelope's node.
pub const S_AUTOENV: CStr32 = Ptr32::new(0x08693888);

/// `p.243` (0x08693840, `.data`): the six points `SI_skeJointDraw` draws a bone with. The
/// routine writes the first point's x and the third's before every use.
pub const SKE_JOINT_POINTS: Ptr32<[[f32; 3]; 6]> = Ptr32::new(0x0869_3840);
/// The table's initial values.
pub const SKE_JOINT_POINTS_INIT: [[f32; 3]; 6] = [[0.25, 0.0, 0.0], [0.0, 0.25, 0.0], [-0.25, 0.0, 0.0], [0.0, -0.25, 0.0], [0.0, 0.0, -0.125], [0.0, 0.0, 0.125]];

/// The two texts of `rotlimWarning`.
pub const S_MIN_MAX: CStr32 = Ptr32::new(0x08694eb2);
pub const S_EMPTY_MESSAGE: CStr32 = Ptr32::new(0x08694ebc);

/// (address, text) of every string literal a function of this crate reads or hands on.
pub const STRINGS: &[(u32, &str)] = &[
    (S_PATCH_NOT_DONE.0, "Surface is a patch, not done yet!"),
    (S_FACE.0, "face"),
    (S_NO_CLUSTER.0, " No cluster...."),
    (S_HAS_CLUSTER_CNS.0, "Selected model already has a cluster constraint"),
    (S_NO_ACTIVE_CLUSTER.0, "Selected model doesn't have any active cluster"),
    (S_NO_POSLIM_FOR_JOINT.0, "No PosLim constraint for joint"),
    (S_WAVES.0, "Waves cannot be constrained"),
    (S_ON_PATH[0].0, "The selected item is already on a path."),
    (S_ON_PATH[1].0, "Constraint will override path unless"),
    (S_ON_PATH[2].0, "deactivated. Add constraint anyway?"),
    (S_ON_PATH[3].0, "Ok"),
    (S_ON_PATH[4].0, "Cancel"),
    (S_AUTOENV.0, "autoenv"),
    (S_MIN_MAX.0, "Min > Max"),
    (S_EMPTY_MESSAGE.0, ""),
];

/// Stores the crate's constants in the image.
pub fn store(m: &mut Mem) {
    m.write(SKE_JOINT_POINTS, SKE_JOINT_POINTS_INIT);
    for &(addr, text) in STRINGS {
        if addr >= RODATA.0 && addr - RODATA.0 < RODATA.1 {
            m.put_rom_cstr(addr, text.as_bytes());
        } else {
            m.put_cstr(addr, text.as_bytes());
        }
    }
}
