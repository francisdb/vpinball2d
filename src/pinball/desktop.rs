//! Desktop-mode layout, like vpinball's desktop view.
//!
//! vpinball renders a table's "full desktop" backdrop image across the whole
//! window, with the playfield showing through a central cutout and the score
//! reels / textboxes overlaid on the windows printed into the backdrop. The reel
//! and textbox gameitems are positioned in a normalized backdrop space
//! (`EDITOR_BG_WIDTH` x `EDITOR_BG_HEIGHT`, see `reel`), i.e. as [0,1] fractions
//! of that backdrop.
//!
//! We reproduce this in 2D: the backdrop is a single textured quad, sized so its
//! cutout frames the playfield. The playfield stays at the world origin, full
//! scale (so physics is untouched); the camera simply zooms out to show the whole
//! backdrop, and the reels are placed at their backdrop fractions.

use bevy::image::Image;
use bevy::prelude::*;

/// Where the desktop backdrop sits in world space and how its normalized [0,1]
/// coordinates (origin top-left) map there. Inserted by `level::spawn_level`,
/// read by `gameplay::fit_camera` and the reel spawners.
#[derive(Resource, Clone, Copy)]
pub(crate) struct DesktopLayout {
    /// World-space centre of the backdrop quad.
    pub(crate) center: Vec2,
    /// World-space size of the backdrop quad.
    pub(crate) size: Vec2,
}

impl DesktopLayout {
    /// Map a backdrop fraction `(fx, fy)` (0..1, origin top-left) to world space.
    pub(crate) fn to_world(self, fx: f32, fy: f32) -> Vec2 {
        Vec2::new(
            self.center.x - self.size.x * 0.5 + fx * self.size.x,
            self.center.y + self.size.y * 0.5 - fy * self.size.y,
        )
    }

    /// Scale a backdrop fraction size to world size.
    pub(crate) fn to_world_size(self, fw: f32, fh: f32) -> Vec2 {
        Vec2::new(fw * self.size.x, fh * self.size.y)
    }
}

/// The playfield cutout rectangle in the backdrop (normalized, origin top-left):
/// the central dark hole the playfield shows through. Falls back to a sensible
/// centred rectangle when the backdrop has no detectable hole.
fn detect_cutout(image: Option<&Image>) -> Rect {
    let fallback = Rect {
        min: Vec2::new(0.30, 0.13),
        max: Vec2::new(0.70, 0.88),
    };
    let Some(image) = image else {
        return fallback;
    };
    let Some(data) = image.data.as_ref() else {
        return fallback;
    };
    let w = image.texture_descriptor.size.width as usize;
    let h = image.texture_descriptor.size.height as usize;
    if w < 2 || h < 2 {
        return fallback;
    }
    let bpp = data.len() / (w * h);
    if bpp < 3 {
        return fallback;
    }
    // "Near black" is channel-order agnostic (BGRA or RGBA): all colour channels low.
    let dark = |x: usize, y: usize| {
        let i = (y * w + x) * bpp;
        data[i] < 24 && data[i + 1] < 24 && data[i + 2] < 24
    };
    let (cx, cy) = (w / 2, h / 2);
    if !dark(cx, cy) {
        return fallback;
    }
    // Grow the hole out from the centre until it hits the printed backdrop.
    let mut x0 = cx;
    while x0 > 0 && dark(x0 - 1, cy) {
        x0 -= 1;
    }
    let mut x1 = cx;
    while x1 + 1 < w && dark(x1 + 1, cy) {
        x1 += 1;
    }
    let mut y0 = cy;
    while y0 > 0 && dark(cx, y0 - 1) {
        y0 -= 1;
    }
    let mut y1 = cy;
    while y1 + 1 < h && dark(cx, y1 + 1) {
        y1 += 1;
    }
    Rect {
        min: Vec2::new(x0 as f32 / w as f32, y0 as f32 / h as f32),
        max: Vec2::new(x1 as f32 / w as f32, y1 as f32 / h as f32),
    }
}

/// Compute the backdrop layout so its cutout frames the `table_size` playfield
/// (centred at the origin). `img_size` is the backdrop image pixel size (for its
/// aspect ratio); `image` is its decoded pixels for cutout detection (optional).
pub(crate) fn layout(table_size: Vec2, img_size: Vec2, image: Option<&Image>) -> DesktopLayout {
    let cutout = detect_cutout(image);
    let cut_w = (cutout.max.x - cutout.min.x).max(0.05);
    let cut_h = (cutout.max.y - cutout.min.y).max(0.05);
    let img_aspect = if img_size.y > 0.0 {
        img_size.x / img_size.y
    } else {
        16.0 / 9.0
    };
    // Size the backdrop (kept at its native aspect) so the playfield fits the
    // cutout in both dimensions; the tall playfield usually binds on height.
    let backdrop_h = (table_size.y / cut_h).max(table_size.x / (cut_w * img_aspect));
    let size = Vec2::new(backdrop_h * img_aspect, backdrop_h);
    // Offset the backdrop so the cutout centre lands on the origin (the playfield
    // centre), keeping reels aligned with the printed windows.
    let cut_cx = (cutout.min.x + cutout.max.x) * 0.5;
    let cut_cy = (cutout.min.y + cutout.max.y) * 0.5;
    let center = Vec2::new(size.x * (0.5 - cut_cx), size.y * (cut_cy - 0.5));
    DesktopLayout { center, size }
}
