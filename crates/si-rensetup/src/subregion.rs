// SPDX-License-Identifier: MIT
//! `renu_subregion.c`: the arithmetic of the render sub-region box. The user drags a box in
//! the highlighted window to render only part of the picture; the box is kept as fractions
//! of the picture in `SI_defaultControl` (+0x22c left, +0x230 right, +0x234 top, +0x238
//! bottom).
//!
//! Ported: the box made from the fractions, clipped, tested and put right way up. The
//! routine that asks the user (`SI_doSubregion`), its event handler, the three dragging
//! loops and the drawing are the window system's side of the file and are not ported; the
//! one drawing call the ported routines make goes through [`RegionDraw`].

use si_x87::{Round, F80};

/// `SI_Irect`: four 16-bit screen coordinates: left, right, bottom, top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Irect {
    pub x0: i16,
    pub x1: i16,
    pub y0: i16,
    pub y1: i16,
}

/// `SI_Region` (16 bytes, `SI_calloc(1, 0x10)`): the box and the rectangle it must stay in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub rect: Irect,
    pub bounds: Irect,
}

/// The drawing of the box, which goes through the IRIS GL layer.
pub trait RegionDraw {
    /// `drawRegion(box, on)` (0x082ca948): draws the box (`on` 1) or takes it away (0).
    fn draw_region(&mut self, rect: Irect, on: i32);
}

/// Holds a box inside bounds [soft 0x082cb468 clipBox]: each of its two x values is raised
/// to the bounds' left and then lowered to their right, each y value likewise.
pub fn clip_box(r: &mut Irect, b: Irect) {
    for v in [&mut r.x0, &mut r.x1] {
        if *v < b.x0 {
            *v = b.x0;
        }
    }
    for v in [&mut r.x0, &mut r.x1] {
        if b.x1 < *v {
            *v = b.x1;
        }
    }
    for v in [&mut r.y0, &mut r.y1] {
        if *v < b.y0 {
            *v = b.y0;
        }
    }
    for v in [&mut r.y0, &mut r.y1] {
        if b.y1 < *v {
            *v = b.y1;
        }
    }
}

/// Whether a box lies inside bounds, edges included [soft 0x082cb4d0 clippedBox]. (Named
/// for what the caller does when it answers no.)
pub fn clipped_box(a: Irect, b: Irect) -> bool {
    b.x0 <= a.x0 && b.x0 <= a.x1 && a.x0 <= b.x1 && a.x1 <= b.x1 && b.y0 <= a.y0 && b.y0 <= a.y1 && a.y0 <= b.y1 && a.y1 <= b.y1
}

/// Puts a box right way up [soft 0x082cb530 correctBox]: it is taken off the screen, its x
/// values exchanged if the right one is below the left, its y values likewise, and drawn
/// again.
pub fn correct_box(r: &mut Irect, draw: &mut dyn RegionDraw) {
    draw.draw_region(*r, 0);
    if r.x1 < r.x0 {
        std::mem::swap(&mut r.x0, &mut r.x1);
    }
    if r.y1 < r.y0 {
        std::mem::swap(&mut r.y0, &mut r.y1);
    }
    draw.draw_region(*r, 1);
}

/// The sub-region box of a window [soft 0x082ca5b0 regionMake]: `image` is the picture's
/// rectangle in the window, `bounds` the rectangle the box may not leave, `fractions` the
/// four floats at +0x22c, +0x230, +0x234 and +0x238 of `SI_defaultControl`. With w and h the
/// picture's width and height in pixels:
///
/// - left = left of the picture + w × f(+0x22c); right = right − w × f(+0x230);
/// - bottom = bottom + h × f(+0x238); top = top − h × f(+0x234);
///
/// each with 0.5 added, computed in the register and cut towards zero to an integer whose
/// low 16 bits are kept. A box that then lies partly outside the bounds sets
/// `subregionXresized` (0x0879e6ea) and is clipped.
pub fn region_make(image: &Irect, bounds: &Irect, fractions: [f32; 4], resized: &mut i16) -> Region {
    let w = F80::from_i32(image.x1 as i32 - image.x0 as i32);
    let h = F80::from_i32(image.y1 as i32 - image.y0 as i32);
    let half = F80::from_f32(0.5);
    let cut = |v: F80| v.to_i32(Round::Zero) as i16;
    let f = |i: usize| F80::from_f32(fractions[i]);
    let mut rect = Irect {
        x0: cut(F80::from_i32(image.x0 as i32) + f(0) * w + half),
        x1: cut(F80::from_i32(image.x1 as i32) - w * f(1) + half),
        y0: cut(F80::from_i32(image.y0 as i32) + f(3) * h + half),
        y1: cut(F80::from_i32(image.y1 as i32) - h * f(2) + half),
    };
    if !clipped_box(rect, *bounds) {
        *resized = 1;
    }
    clip_box(&mut rect, *bounds);
    Region { rect, bounds: *bounds }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Seen(Vec<(Irect, i32)>);

    impl RegionDraw for Seen {
        fn draw_region(&mut self, rect: Irect, on: i32) {
            self.0.push((rect, on));
        }
    }

    #[test]
    fn a_quarter_box() {
        let image = Irect { x0: 100, x1: 500, y0: 50, y1: 350 };
        let mut resized = 0;
        let r = region_make(&image, &image, [0.25, 0.25, 0.5, 0.0], &mut resized);
        assert_eq!(r.rect, Irect { x0: 200, x1: 400, y0: 50, y1: 200 });
        assert_eq!(resized, 0);
        let r = region_make(&image, &Irect { x0: 250, x1: 300, y0: 0, y1: 100 }, [0.25, 0.25, 0.5, 0.0], &mut resized);
        assert_eq!((r.rect, resized), (Irect { x0: 250, x1: 300, y0: 50, y1: 100 }, 1));
    }

    #[test]
    fn a_box_dragged_backwards_is_turned() {
        let mut r = Irect { x0: 9, x1: 2, y0: 1, y1: 5 };
        let mut seen = Seen(Vec::new());
        correct_box(&mut r, &mut seen);
        assert_eq!(r, Irect { x0: 2, x1: 9, y0: 1, y1: 5 });
        assert_eq!(seen.0, vec![(Irect { x0: 9, x1: 2, y0: 1, y1: 5 }, 0), (r, 1)]);
    }
}
