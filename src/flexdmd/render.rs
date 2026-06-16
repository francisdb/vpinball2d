//! Rasterises a [`FlexDmd`](super::FlexDmd) scene graph into a backbox DMD image.
//!
//! A CPU compositor (FlexDMD's `SurfaceGraphics`: nearest-neighbour scaled
//! alpha blits, integer translate, rect fill) walks the actor tree each frame
//! into an RGBA buffer, which is uploaded to a Bevy [`Image`] shown as a sprite
//! at the top of the desktop backbox. Image sources (`"VPX.<name>&dmd=N&add"`)
//! are resolved from the table's vpx images, filtered, and cached.

use super::{ActorId, ActorKind, Alignment, FlexDmd, Scaling};
use crate::pinball::desktop::DesktopLayout;
use crate::screens::Screen;
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Z above the backbox quad / reels.
const DMD_Z: f32 = 0.3;
/// Fraction of the backbox width the DMD panel spans (kept modest).
const DMD_WIDTH_FRAC: f32 = 0.22;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<FlexDmdDisplay>();
    app.add_systems(Update, render_flexdmd.run_if(in_state(Screen::Gameplay)));
    app.add_systems(OnExit(Screen::Gameplay), teardown);
}

/// A resolved, filtered bitmap (straight-alpha RGBA8).
struct Resolved {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

#[derive(Resource, Default)]
struct FlexDmdDisplay {
    image: Option<Handle<Image>>,
    sprite: Option<Entity>,
    cache: HashMap<String, Option<Resolved>>,
    size: (u32, u32),
    last_revision: u64,
    spawned_revision: Option<u64>,
}

fn teardown(mut display: ResMut<FlexDmdDisplay>) {
    *display = FlexDmdDisplay::default();
}

#[allow(clippy::too_many_arguments)]
fn render_flexdmd(
    mut commands: Commands,
    runtime: Option<NonSend<crate::scripting::ScriptRuntime>>,
    layout: Option<Res<DesktopLayout>>,
    table_assets: Option<Res<crate::pinball::table::TableAssets>>,
    assets_vpx: Res<Assets<crate::vpx::VpxAsset>>,
    mut images: ResMut<Assets<Image>>,
    mut display: ResMut<FlexDmdDisplay>,
) {
    let (Some(runtime), Some(layout)) = (runtime, layout) else {
        return;
    };
    let host = runtime.host();
    let host = host.borrow();
    let dmd = &host.flexdmd;
    let vpx = table_assets.and_then(|t| assets_vpx.get(&t.vpx));

    // Ensure the output image exists and matches the DMD size.
    if display.size != (dmd.width, dmd.height) || display.image.is_none() {
        let img = blank_image(dmd.width, dmd.height);
        display.image = Some(images.add(img));
        display.size = (dmd.width, dmd.height);
        display.cache.clear();
        // Force a respawn of the sprite with the new handle.
        if let Some(e) = display.sprite.take() {
            commands.entity(e).despawn();
        }
    }

    let visible = dmd.run && dmd.show;
    // Spawn / show the sprite lazily once running.
    if display.sprite.is_none() && visible {
        let handle = display.image.clone().unwrap();
        let (center, size) = dmd_placement(&layout, dmd.width, dmd.height);
        let e = commands
            .spawn((
                Name::from("FlexDMD"),
                Sprite {
                    image: handle,
                    custom_size: Some(size),
                    ..default()
                },
                Transform::from_xyz(center.x, center.y, DMD_Z),
                DespawnOnExit(Screen::Gameplay),
            ))
            .id();
        display.sprite = Some(e);
    }
    if let Some(e) = display.sprite
        && let Ok(mut ec) = commands.get_entity(e)
    {
        ec.insert(if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }

    if !visible || dmd.lock_count > 0 {
        return;
    }
    if display.last_revision == dmd.revision && display.spawned_revision == Some(dmd.revision) {
        return; // nothing changed
    }
    display.last_revision = dmd.revision;
    display.spawned_revision = Some(dmd.revision);

    // Resolve every image source the tree references (cache misses only).
    let srcs = collect_srcs(dmd);
    for src in srcs {
        if !display.cache.contains_key(&src) {
            let resolved = vpx.and_then(|v| resolve_src(&src, v, &mut images));
            display.cache.insert(src, resolved);
        }
    }

    // Composite into a fresh RGBA buffer, then upload.
    let (w, h) = (dmd.width as usize, dmd.height as usize);
    let mut buf = vec![0u8; w * h * 4];
    let mut gfx = Surface {
        buf: &mut buf,
        w,
        h,
        tx: 0,
        ty: 0,
    };
    draw_actor(dmd, dmd.stage(), &display.cache, &mut gfx);

    if let Some(handle) = &display.image
        && let Some(img) = images.get_mut(handle)
    {
        img.data = Some(buf);
    }
}

/// World centre + size for the DMD sprite: a modest panel in the right backbox
/// margin, vertically centred. A table that genuinely renders a FlexDMD shows it
/// here, like an external DMD panel, rather than pinned onto the playfield.
fn dmd_placement(layout: &DesktopLayout, w: u32, h: u32) -> (Vec2, Vec2) {
    let world_w = layout.size.x * DMD_WIDTH_FRAC;
    let aspect = w as f32 / h.max(1) as f32;
    let world_h = world_w / aspect;
    let center = layout.to_world(0.85, 0.5);
    (center, Vec2::new(world_w, world_h))
}

fn blank_image(w: u32, h: u32) -> Image {
    let mut img = Image::new_fill(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // The DMD is a tiny (e.g. 128x32) frame blown up to the backbox panel; sample
    // nearest so it stays crisp/blocky like a real DMD instead of a linear smear.
    img.sampler = bevy::image::ImageSampler::nearest();
    img
}

fn collect_srcs(dmd: &FlexDmd) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dmd.stage()];
    while let Some(id) = stack.pop() {
        let Some(actor) = dmd.actor(id) else { continue };
        match &actor.kind {
            ActorKind::Image { src, .. } if !src.is_empty() => out.push(src.clone()),
            ActorKind::Group { children, .. } => stack.extend(children.iter().copied()),
            _ => {}
        }
    }
    out
}

// --- CPU compositor (SurfaceGraphics port) ---------------------------------

struct Surface<'a> {
    buf: &'a mut [u8],
    w: usize,
    h: usize,
    tx: i32,
    ty: i32,
}

impl Surface<'_> {
    /// Straight-alpha "over" blend of one src pixel onto dst at (x,y).
    fn blend(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8) {
        let (x, y) = (x + self.tx, y + self.ty);
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || a == 0 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 4;
        if a == 255 {
            self.buf[i] = r;
            self.buf[i + 1] = g;
            self.buf[i + 2] = b;
            self.buf[i + 3] = 255;
            return;
        }
        let af = a as f32 / 255.0;
        let inv = 1.0 - af;
        for (k, sv) in [r, g, b].into_iter().enumerate() {
            let dv = self.buf[i + k] as f32;
            self.buf[i + k] = (sv as f32 * af + dv * inv).round().clamp(0.0, 255.0) as u8;
        }
        let da = self.buf[i + 3] as f32 / 255.0;
        self.buf[i + 3] = ((af + da * inv) * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let (r, g, b) = (
            (color & 0xff) as u8,
            (color >> 8 & 0xff) as u8,
            (color >> 16 & 0xff) as u8,
        );
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, r, g, b, 255);
            }
        }
    }

    /// Nearest-neighbour scaled blit of `bmp` into dst rect (x,y,w,h).
    fn blit(&mut self, bmp: &Resolved, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 || bmp.w == 0 || bmp.h == 0 {
            return;
        }
        for dy in 0..h {
            let sy = (dy as u32 * bmp.h / h as u32).min(bmp.h - 1);
            for dx in 0..w {
                let sx = (dx as u32 * bmp.w / w as u32).min(bmp.w - 1);
                let si = (sy * bmp.w + sx) as usize * 4;
                self.blend(
                    x + dx,
                    y + dy,
                    bmp.data[si],
                    bmp.data[si + 1],
                    bmp.data[si + 2],
                    bmp.data[si + 3],
                );
            }
        }
    }
}

fn draw_actor(
    dmd: &FlexDmd,
    id: ActorId,
    cache: &HashMap<String, Option<Resolved>>,
    gfx: &mut Surface,
) {
    let Some(actor) = dmd.actor(id) else { return };
    if !actor.visible {
        return;
    }
    let (ax, ay) = (actor.x as i32, actor.y as i32);
    if actor.clear_background {
        gfx.fill_rect(ax, ay, actor.width, actor.height, 0x00_00_00);
    }
    match &actor.kind {
        ActorKind::Group { children, .. } => {
            gfx.tx += ax;
            gfx.ty += ay;
            for &child in children {
                draw_actor(dmd, child, cache, gfx);
            }
            gfx.tx -= ax;
            gfx.ty -= ay;
        }
        ActorKind::Image {
            src,
            scaling,
            alignment,
        } => {
            if let Some(Some(bmp)) = cache.get(src) {
                let (dw, dh) = scale_size(*scaling, bmp.w, bmp.h, actor.width, actor.height);
                let (ox, oy) = align_offset(*alignment, dw, dh, actor.width, actor.height);
                gfx.blit(bmp, ax + ox, ay + oy, dw, dh);
            }
        }
        ActorKind::Frame {
            thickness,
            border_color,
            fill,
            fill_color,
        } => {
            let (w, h, t) = (actor.width, actor.height, *thickness);
            if *fill {
                gfx.fill_rect(ax + t, ay + t, w - 2 * t, h - 2 * t, *fill_color);
            }
            if t > 0 {
                gfx.fill_rect(ax, ay, w, t, *border_color);
                gfx.fill_rect(ax, ay + h - t, w, t, *border_color);
                gfx.fill_rect(ax, ay + t, t, h - 2 * t, *border_color);
                gfx.fill_rect(ax + w - t, ay + t, t, h - 2 * t, *border_color);
            }
        }
        ActorKind::Label { .. } => {
            // TODO: bitmap-font text rendering.
        }
    }
}

fn scale_size(scaling: Scaling, sw: u32, sh: u32, bw: i32, bh: i32) -> (i32, i32) {
    let (sw, sh) = (sw as f32, sh as f32);
    let (bw_f, bh_f) = (bw as f32, bh as f32);
    match scaling {
        Scaling::Stretch => (bw, bh),
        Scaling::None => (sw as i32, sh as i32),
        Scaling::FillX => (bw, (sh * bw_f / sw).round() as i32),
        Scaling::FillY => ((sw * bh_f / sh).round() as i32, bh),
        Scaling::Fill => {
            let s = (bw_f / sw).max(bh_f / sh);
            ((sw * s).round() as i32, (sh * s).round() as i32)
        }
    }
}

fn align_offset(alignment: Alignment, dw: i32, dh: i32, bw: i32, bh: i32) -> (i32, i32) {
    let (fx, fy) = alignment.fractions();
    (
        ((bw - dw) as f32 * fx).round() as i32,
        ((bh - dh) as f32 * fy).round() as i32,
    )
}

// --- VPX image source resolution + filters ---------------------------------

/// Resolve a `"VPX.<name>&opt&opt"` source to a filtered RGBA bitmap.
fn resolve_src(
    src: &str,
    vpx: &crate::vpx::VpxAsset,
    images: &mut Assets<Image>,
) -> Option<Resolved> {
    let mut parts = src.split('&');
    let path = parts.next()?;
    let name = path.strip_prefix("VPX.").unwrap_or(path);
    let handle = vpx.image(name)?;
    let mut bmp = image_to_rgba(images.get(handle)?)?;

    for opt in parts {
        if let Some(n) = opt.strip_prefix("dmd=") {
            if let Ok(n) = n.parse::<u32>() {
                bmp = dot_filter(&bmp, n.max(1));
            }
        } else if opt == "add" {
            additive_filter(&mut bmp);
        }
        // region=/pad=/dmd2= TODO.
    }
    Some(bmp)
}

/// Convert a Bevy image to straight-alpha RGBA8 bytes.
fn image_to_rgba(img: &Image) -> Option<Resolved> {
    let w = img.texture_descriptor.size.width;
    let h = img.texture_descriptor.size.height;
    let data = img.data.as_ref()?;
    let rgba = match img.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm => data.clone(),
        _ => img.convert(TextureFormat::Rgba8UnormSrgb)?.data?,
    };
    if rgba.len() != (w * h * 4) as usize {
        return None;
    }
    Some(Resolved { w, h, data: rgba })
}

/// FlexDMD's `DotFilter`: NxN box-average downsample with a brightness boost.
fn dot_filter(bmp: &Resolved, n: u32) -> Resolved {
    let (sw, sh) = (bmp.w, bmp.h);
    let (dw, dh) = ((sw / n).max(1), (sh / n).max(1));
    let boost = 1.0 + (n * n) as f32 / 1.8;
    let mut data = vec![0u8; (dw * dh * 4) as usize];
    for dy in 0..dh {
        for dx in 0..dw {
            let (mut r, mut g, mut b, mut a, mut cnt) = (0f32, 0f32, 0f32, 0f32, 0f32);
            for yy in 0..n {
                for xx in 0..n {
                    let sx = dx * n + xx;
                    let sy = dy * n + yy;
                    if sx >= sw || sy >= sh {
                        continue;
                    }
                    let si = (sy * sw + sx) as usize * 4;
                    r += bmp.data[si] as f32;
                    g += bmp.data[si + 1] as f32;
                    b += bmp.data[si + 2] as f32;
                    a += bmp.data[si + 3] as f32;
                    cnt += 1.0;
                }
            }
            if cnt == 0.0 {
                continue;
            }
            let di = (dy * dw + dx) as usize * 4;
            data[di] = (r / cnt * boost).min(255.0) as u8;
            data[di + 1] = (g / cnt * boost).min(255.0) as u8;
            data[di + 2] = (b / cnt * boost).min(255.0) as u8;
            data[di + 3] = (a / cnt).min(255.0) as u8;
        }
    }
    Resolved { w: dw, h: dh, data }
}

/// FlexDMD's `AdditiveFilter`: pixels with all colour channels < 64 become
/// fully transparent (so dark areas read as "off" / blend additively).
fn additive_filter(bmp: &mut Resolved) {
    for px in bmp.data.chunks_exact_mut(4) {
        if px[0] < 64 && px[1] < 64 && px[2] < 64 {
            px[3] = 0;
        }
    }
}
