//! Desktop-mode layout, like vpinball's desktop view.
//!
//! vpinball renders a table's "full desktop" backdrop image across the whole
//! window, with the playfield showing through a central cutout and the score
//! reels / textboxes overlaid on the windows printed into the backdrop. The reel
//! and textbox gameitems are positioned in a normalized backdrop space
//! (`EDITOR_BG_WIDTH` x `EDITOR_BG_HEIGHT`, see `reel`), i.e. as `[0,1]` fractions
//! of that backdrop.
//!
//! We reproduce this in 2D: the backdrop is a single textured quad, sized so its
//! cutout frames the playfield. The playfield stays at the world origin, full
//! scale (so physics is untouched); the camera simply zooms out to show the whole
//! backdrop, and the reels are placed at their backdrop fractions.

use bevy::image::Image;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use vpin::vpx::gameitem::textbox;

/// The backdrop coordinate space (`EDITOR_BG_WIDTH` x `EDITOR_BG_HEIGHT`): reel
/// and textbox gameitem coordinates are divided by these to get `[0,1]` fractions.
pub(crate) const EDITOR_BG_WIDTH: f32 = 1000.0;
pub(crate) const EDITOR_BG_HEIGHT: f32 = 750.0;

/// Z of the backdrop overlays (reels and textbox text), above the backdrop quad.
pub(crate) const OVERLAY_Z: f32 = 0.2;
/// Glyph atlas size for backdrop text; the entity is scaled down to world size.
const TEXT_FONT_PX: f32 = 96.0;

/// Where the desktop backdrop sits in world space and how its normalized `[0,1]`
/// coordinates (origin top-left) map there. Inserted by `level::spawn_level`,
/// read by `gameplay::fit_camera` and the reel spawners.
#[derive(Resource, Clone, Copy)]
pub(crate) struct DesktopLayout {
    /// World-space centre of the backdrop quad.
    pub(crate) center: Vec2,
    /// World-space size of the backdrop quad.
    pub(crate) size: Vec2,
    /// Whether the table actually has a desktop backdrop image (else the layout
    /// is just the bare playfield and the textbox overlays are not drawn).
    pub(crate) has_backdrop: bool,
}

/// A textbox rendered as text over the backdrop (high score, match, game over,
/// ball in play, ...). Its string is kept in sync with the script's shadow state.
#[derive(Component)]
pub(crate) struct DesktopText {
    /// The textbox name, matched against the script's shadow item.
    pub(crate) name: String,
    /// The textbox box in world space, for fitting the text.
    pub(crate) box_world: Vec2,
}

/// The transform scale that fits `text` inside `box_world` (both dimensions),
/// given glyphs rendered at [`TEXT_FONT_PX`]. Approximate sans-serif metrics.
pub(crate) fn text_scale(text: &str, box_world: Vec2) -> f32 {
    let len = text.chars().count().max(1) as f32;
    let by_width = box_world.x / (len * TEXT_FONT_PX * 0.6);
    let by_height = box_world.y / (TEXT_FONT_PX * 0.95);
    by_width.min(by_height).max(1e-6)
}

/// A textbox rendered as backdrop text, centred on its box. Pair with
/// [`DesktopText`]; the value updates from the script (see `scripting`).
pub(crate) fn desktop_text(layout: &DesktopLayout, tb: &textbox::TextBox) -> impl Bundle {
    let fx = (tb.ver1.x + tb.ver2.x) * 0.5 / EDITOR_BG_WIDTH;
    let fy = (tb.ver1.y + tb.ver2.y) * 0.5 / EDITOR_BG_HEIGHT;
    let bw = (tb.ver2.x - tb.ver1.x).abs() / EDITOR_BG_WIDTH;
    let bh = (tb.ver2.y - tb.ver1.y).abs() / EDITOR_BG_HEIGHT;
    let center = layout.to_world(fx, fy);
    let box_world = layout.to_world_size(bw, bh);
    let color = Color::srgb_u8(tb.font_color.r, tb.font_color.g, tb.font_color.b);
    let scale = text_scale(&tb.text, box_world);
    (
        Name::from(format!("Text {}", tb.name)),
        DesktopText {
            name: tb.name.clone(),
            box_world,
        },
        Text2d::new(tb.text.clone()),
        TextFont::from_font_size(TEXT_FONT_PX),
        TextColor(color),
        TextLayout::justify(Justify::Center),
        Anchor::CENTER,
        Transform::from_xyz(center.x, center.y, OVERLAY_Z).with_scale(Vec3::splat(scale)),
        DespawnOnExit(crate::screens::Screen::Gameplay),
    )
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
    let fallback = DEFAULT_CUTOUT;
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

/// The default playfield cutout (also [`detect_cutout`]'s fallback): the central
/// window the playfield fills, leaving margins around it for backdrop overlays.
const DEFAULT_CUTOUT: Rect = Rect {
    min: Vec2::new(0.30, 0.13),
    max: Vec2::new(0.70, 0.88),
};

/// Compute the backdrop layout so its cutout frames the `table_size` playfield
/// (centred at the origin). `img_size` is the backdrop image pixel size (for its
/// aspect ratio); `image` is its decoded pixels for cutout detection (optional).
pub(crate) fn layout(table_size: Vec2, img_size: Vec2, image: Option<&Image>) -> DesktopLayout {
    let cutout = detect_cutout(image);
    let cut_w = (cutout.max.x - cutout.min.x).max(0.05);
    let img_aspect = if img_size.y > 0.0 {
        img_size.x / img_size.y
    } else {
        16.0 / 9.0
    };
    // Size the backdrop (kept at its native aspect) so the playfield fills the
    // full window height - reaching top and bottom - rather than only the
    // (shorter) cutout. A very wide table instead binds on the cutout width.
    let backdrop_h = table_size.y.max(table_size.x / (cut_w * img_aspect));
    let size = Vec2::new(backdrop_h * img_aspect, backdrop_h);
    // Centre the playfield on the cutout horizontally and on the window
    // vertically (so it fills top to bottom); the reels still map onto the
    // backdrop's printed windows via their `[0,1]` fractions.
    let cut_cx = (cutout.min.x + cutout.max.x) * 0.5;
    let center = Vec2::new(size.x * (0.5 - cut_cx), 0.0);
    DesktopLayout {
        center,
        size,
        has_backdrop: true,
    }
}
