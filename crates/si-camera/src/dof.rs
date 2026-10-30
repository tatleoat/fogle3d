// SPDX-License-Identifier: MIT
//! The depth of field helpers of `sys_camera.c` (finding 0032, "Depth of field"): the
//! hyperfocal distance, the near and far limits of sharpness, the circle of confusion at
//! which the far limit stops being positive, the out-of-focus distance. The camera dialog
//! (`SI_setCamera`, `updateDof`) calls them.
//!
//! The compiler made the same instructions of them as of the five functions of `camera.c`
//! that `si-render` ports (`findHyperfocal` … `findCoc`, finding 0012, section 8), each copy
//! reading its own copy of the constants (here 0x08645580 … 0x086455b8, the same values):
//! two copies of one static source. Values only: they take and return numbers. The names of
//! the three numbers are a reading of the formulas (focal length f, aperture number N,
//! circle of confusion c, focus distance s), not the original's.
//!
//! The functions answer in the x87 register, unrounded: here [`F80`].

use si_x87::{x, F80};

/// 0x08645580, a float: the least circle of confusion (the float nearest 1e-6).
const COC_MIN: f32 = f32::from_bits(0x3586_37bd);

/// [soft 0x080d3d9c findhyperfocal]: `(a × a) / (b × c)`, unrounded (f² / (N c)).
pub fn find_hyperfocal(a: f32, b: f32, c: f32) -> F80 {
    (x(a) * a) / (x(b) * c)
}

/// [soft 0x080d3db4 findnear]: `(b × a) / ((a + b) − c)`, unrounded.
pub fn find_near(a: f32, b: f32, c: f32) -> F80 {
    (x(b) * a) / ((x(a) + b) - c)
}

/// [soft 0x080d3dd0 findfar]: `(b × a) / ((a − b) − c)`, unrounded; negative once the focus
/// is beyond the hyperfocal distance.
pub fn find_far(a: f32, b: f32, c: f32) -> F80 {
    (x(b) * a) / ((x(a) - b) - c)
}

/// [soft 0x080d4048 findoutofocus]: `findfar(float(findhyperfocal(a, b, float(d × 0.85))),
/// c, a) / 12`, unrounded (0.85 the double at 0x086455b0, 12 the float at 0x086455b8).
pub fn find_out_of_focus(a: f32, b: f32, c: f32, d: f32) -> F80 {
    let coc = (x(d) * 0.85f64).to_f32();
    let h = find_hyperfocal(a, b, coc).to_f32();
    find_far(h, c, a) / 12.0f32
}

/// [soft 0x080d3dec findcoc]: the circle of confusion at which
/// far(coc) = `findfar(float(findhyperfocal(a, b, coc)), c, a)` stops being positive,
/// searched in four passes with steps 0.01, 0.001, 0.0001, 0.00001 (doubles at 0x08645590
/// … 0x086455a8). The first starts at 1e-6 and runs while the stepped value is below 0.1
/// (0x08645588) and far is above zero; each later pass starts 0.01 below where the pass
/// before stopped (0.01 every time, not the pass's step; 1e-6 when that is negative) and
/// runs while below that stopping value and far is above zero. The value is evaluated
/// before it is stepped and tested after, every value rounded to float as it is stepped.
/// The result is the last pass's value less 0.00001, or 1e-6 when that is negative.
pub fn find_coc(a: f32, b: f32, c: f32) -> f32 {
    let far = |coc: f32| find_far(find_hyperfocal(a, b, coc).to_f32(), c, a);
    let floor = |v: f32| if x(0.0f32).gt(x(v)) { COC_MIN } else { v };
    let mut coc = COC_MIN;
    loop {
        let f = far(coc);
        coc = (x(0.01f64) + coc).to_f32();
        if !x(coc).lt(x(0.1f64)) || !x(0.0f32).lt(f) {
            break;
        }
    }
    for step in [0.001f64, 0.0001, 0.00001] {
        let limit = coc;
        coc = floor((x(limit) - 0.01f64).to_f32());
        if x(coc).lt(x(limit)) {
            loop {
                let f = far(coc);
                coc = (x(coc) + step).to_f32();
                if !x(coc).lt(x(limit)) || !x(0.0f32).lt(f) {
                    break;
                }
            }
        }
    }
    floor((x(coc) - 0.00001f64).to_f32())
}
