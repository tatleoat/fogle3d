// SPDX-License-Identifier: MIT
//! `cam_fct.c`: a new camera and its defaults.

use super::globals::SI_FMT_ASPECT_34;
use super::off::camera;
use super::outside::Outside;
use super::fmov;
use si_mem::Mem;
use si_x87::F80;

/// The floats `SI_resetCamera` loads from the binary's constants (0x086452d0 … 0x086452f4),
/// as their bits: the field of view of a perspective camera (π/4) and of any other mode
/// (20), the focal length (50), the f-stop (5.6), the near and far focus (15, 25), the
/// out-of-focus distance (40), the near and far clipping distances (0.1, 32768) and the
/// position's z (100).
pub const FOV_PERSPECTIVE_BITS: u32 = 0x3f49_0fdb;
pub const TWENTY_BITS: u32 = 0x41a0_0000;
pub const FOCAL_LENGTH_BITS: u32 = 0x4248_0000;
pub const F_STOP_BITS: u32 = 0x40b3_3333;
pub const NEAR_FOCUS_BITS: u32 = 0x4170_0000;
pub const FAR_FOCUS_BITS: u32 = 0x41c8_0000;
pub const OUT_OF_FOCUS_BITS: u32 = 0x4220_0000;
pub const NEAR_BITS: u32 = 0x3dcc_cccd;
pub const FAR_BITS: u32 = 0x4700_0000;
pub const HUNDRED_BITS: u32 = 0x42c8_0000;
/// The double the field of view is multiplied by (0x086452f8).
pub const HALF: f64 = 0.5;
const ONE: u32 = 0x3f80_0000;

/// [soft 0x080d1264 SI_resetCamProjPlane]
/// The projection plane of a camera from its focal length and field of view: distance =
/// focal length; height = (distance + distance) × tan(field of view × 0.5); width = height
/// × the member at +0x64; not active, no offset. Nothing for a null camera.
///
/// On the x87 as the original has it: the half angle is rounded to a double for the call
/// of `tan (0x080c4898)`, whose answer is used at the register's width; the height is
/// rounded to a float before it is stored and multiplied again.
pub fn reset_cam_proj_plane(m: &mut Mem, o: &mut dyn Outside, cam: u32) {
    if cam == 0 {
        return;
    }
    let distance = fmov(m.u32(cam + camera::FOCAL_LENGTH));
    m.set_u32(cam + camera::PLANE_DISTANCE, distance);
    let half = (F80::from_f32(m.f32(cam + camera::FIELD_OF_VIEW)) * F80::from_f64(HALF)).to_f64();
    m.set_u16(cam + camera::PLANE_ACTIVE, 0);
    let tangent = F80::from_f64(o.tan(m, half));
    let d = F80::from_f32(m.f32(cam + camera::PLANE_DISTANCE));
    let height = ((d + d) * tangent).to_f32();
    m.set_u32(cam + camera::PLANE_HEIGHT, height.to_bits());
    let width = (F80::from_f32(height) * F80::from_f32(m.f32(cam + camera::ASPECT))).to_f32();
    m.set_u32(cam + camera::PLANE_OFFSET_X, 0);
    m.set_u32(cam + camera::PLANE_OFFSET_Y, 0);
    m.set_u32(cam + camera::PLANE_WIDTH, width.to_bits());
}

/// [soft 0x080d10e0 SI_resetCamera]
/// The defaults of a camera of a mode (finding 0005 has them): at (0, 0, 100), looking at
/// the origin, with the projection plane `SI_resetCamProjPlane` works out. Nothing for a
/// null camera. The name (+0x00) and what the routine does not name are left alone.
pub fn reset_camera(m: &mut Mem, o: &mut dyn Outside, cam: u32, mode: i32) {
    if cam == 0 {
        return;
    }
    m.set_u16(cam + 0x0a, 0);
    m.set_u16(cam + camera::MODE, mode as u16);
    m.set_u32(cam + camera::FIELD_OF_VIEW, if mode == 1 { FOV_PERSPECTIVE_BITS } else { TWENTY_BITS });
    m.set_u32(cam + 0x54, 0xd);
    let aspect = fmov(m[SI_FMT_ASPECT_34]);
    m.set_u32(cam + 0x58, aspect);
    m.set_u32(cam + 0x5c, ONE);
    m.set_u32(cam + 0x60, ONE);
    m.set_u32(cam + camera::FOCAL_LENGTH, FOCAL_LENGTH_BITS);
    m.set_u32(cam + camera::F_STOP, F_STOP_BITS);
    m.set_u32(cam + camera::FOCAL_DISTANCE, TWENTY_BITS);
    m.set_u32(cam + camera::NEAR_FOCUS, NEAR_FOCUS_BITS);
    m.set_u32(cam + camera::FAR_FOCUS, FAR_FOCUS_BITS);
    m.set_u32(cam + camera::OUT_OF_FOCUS, OUT_OF_FOCUS_BITS);
    m.set_u32(cam + camera::MAXIMUM_COC, TWENTY_BITS);
    m.set_u32(cam + camera::ASPECT, ONE);
    m.set_u32(cam + 0x68, ONE);
    m.set_u32(cam + camera::NEAR, NEAR_BITS);
    m.set_u32(cam + camera::FAR, FAR_BITS);
    m.set_u32(cam + 0x48, 0);
    m.set_u32(cam + 0x4c, 0);
    m.set_u32(cam + 0x50, ONE);
    m.set_u32(cam + 0x10, 0);
    m.set_u32(cam + 0x14, 0);
    m.set_u16(cam + 0x3c, 0);
    m.set_u32(cam + camera::DEPTH_OF_FIELD, 0);
    m.set_u16(cam + camera::ENLARGE, 0);
    m.set_u16(cam + 0x44, 0);
    m.set_u32(cam + 0x18, HUNDRED_BITS);
    for off in [0x24, 0x20, 0x1c, 0x28, 0x2c, 0x30, 0xac, 0xb0] {
        m.set_u32(cam + off, 0);
    }
    m.set_u16(cam + 0xb4, 0);
    m.set_u32(cam + camera::POSITION_MODEL, 0);
    m.set_u32(cam + camera::INTEREST_MODEL, 0);
    m.set_u16(cam + camera::WORD_A8, 1);
    reset_cam_proj_plane(m, o, cam);
    m.set_u32(cam + camera::USER_DATA, 0);
}

/// [soft 0x080d12dc SI_newCamera]
/// A camera record of 0x13c bytes (`SI_malloc`: nothing is cleared) with a copy of the
/// name and the defaults of the mode; null when there is no memory.
pub fn new_camera(m: &mut Mem, o: &mut dyn Outside, name: u32, mode: i32) -> u32 {
    let cam = m.si_malloc(camera::SIZE);
    if cam != 0 {
        let copy = m.strdup(name);
        m.set_u32(cam + camera::NAME, copy);
        reset_camera(m, o, cam, mode);
    }
    cam
}
