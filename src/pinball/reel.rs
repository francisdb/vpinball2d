//! EM score reels: animated digit wheels, vpinball's `DispReel`.
//!
//! Each visible Reel gameitem renders as a row of digit sprites sampled from its
//! digit strip (a horizontal image of `digit_range + 1` cells). A table script
//! sets the value through the Reel methods (`setvalue` / `addvalue` /
//! `resettozero`); the engine rolls the wheels to it the way vpinball does -
//! per-digit motor pulses paced by `motor_steps` and `update_interval`, a click
//! sound per digit advance, and odometer carry to the left. The animation lives
//! here, so a table script only ever supplies the target value.
//!
//! The same machinery drives a credit reel: a table whose credit is a textbox
//! plus a credit-reel image (vpinball's B2S credit window) can declare it in the
//! `.table.json` sidecar, and the engine renders that single-window reel from
//! the textbox's value - see [`spawn_credit_reel`] and `scripting::sidecar`.

use crate::pinball::desktop::DesktopLayout;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::image::{TextureAtlas, TextureAtlasLayout};
use bevy::prelude::*;
use vpin::vpx::gameitem::reel;
use vpin::vpx::gameitem::textbox;

/// Z of the reels: above all table geometry. Like vpinball's backglass reels
/// these are score displays sitting on the backbox graphic at the table top
/// (which renders at its own height, ~0.03 m), so they draw over everything.
const REEL_Z: f32 = 0.2;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, animate_reels.run_if(in_state(Screen::Gameplay)));
}

/// One digit wheel of a [`ScoreReel`].
struct Digit {
    /// Current shown digit, 0..=digit_range.
    value: i32,
    /// Queued single-digit advances (signed); a script add distributes these.
    pulses: i32,
    /// Motor sub-steps left in the current advance (0 = idle), paces the click.
    step_count: u32,
    /// The sprite showing this digit.
    sprite: Entity,
}

/// An animated EM reel (a Reel gameitem, or a sidecar credit reel). Driven
/// through [`ScoreReel::add_value`] / [`set_value`](ScoreReel::set_value) /
/// [`reset_to_zero`](ScoreReel::reset_to_zero); rolled by [`animate_reels`].
#[derive(Component)]
pub struct ScoreReel {
    /// The reel name, matched against the script's reel item.
    pub name: String,
    /// Digit base, `digit_range + 1` (usually 10).
    base: i32,
    /// Motor sub-steps per single-digit advance (`motor_steps`).
    motor_steps: u32,
    /// Milliseconds per motor sub-step (`update_interval`).
    update_interval_ms: f32,
    /// The click sound played once per digit advance, if the reel has one.
    sound: Option<Handle<AudioSource>>,
    /// Digit wheels, index 0 = leftmost (most significant).
    digits: Vec<Digit>,
    /// Countdown to the next motor sub-step.
    until_next_step_ms: f32,
}

impl ScoreReel {
    /// vpinball's `AddValue`: add the number digit-wise as motor pulses, so a
    /// `+10000` pulses the ten-thousands wheel once, not the units wheel 10000x.
    pub fn add_value(&mut self, value: i64) {
        let negative = value < 0;
        let mut remaining = value.unsigned_abs();
        let base = self.base as u64;
        let mut i = self.digits.len();
        while remaining != 0 && i > 0 {
            i -= 1;
            let digit = (remaining % base) as i32;
            remaining /= base;
            self.digits[i].pulses += if negative { -digit } else { digit };
        }
    }

    /// vpinball's `SetValue`: snap the wheels to the number, clearing any motion.
    pub fn set_value(&mut self, value: i64) {
        for digit in &mut self.digits {
            digit.value = 0;
            digit.pulses = 0;
            digit.step_count = 0;
        }
        let mut remaining = value.unsigned_abs();
        let base = self.base as u64;
        let mut i = self.digits.len();
        while remaining != 0 && i > 0 {
            i -= 1;
            self.digits[i].value = (remaining % base) as i32;
            remaining /= base;
        }
    }

    /// The highest number the wheels can show (all digits at `digit_range`).
    pub fn max_value(&self) -> i64 {
        (self.base as i64).saturating_pow(self.digits.len() as u32) - 1
    }

    /// vpinball's `ResetToZero`: roll every wheel forward to 0, carrying left -
    /// the EM home-reset spin, not a jump.
    pub fn reset_to_zero(&mut self) {
        let overflow = self.base;
        let mut carry = 0;
        for i in (0..self.digits.len()).rev() {
            let adjust = overflow - carry - self.digits[i].value;
            carry = 0;
            if adjust != overflow {
                self.digits[i].pulses = adjust;
                carry = 1;
            }
        }
    }
}

/// Rolls every reel toward its pulses, vpinball's `DispReel::UpdateAnimation`:
/// each motor sub-step advances the moving wheels, a wheel that completes its
/// `motor_steps` sub-steps ticks one digit (playing the click) and carries to
/// the wheel on its left on overflow.
fn animate_reels(
    time: Res<Time>,
    mut commands: Commands,
    mut reels: Query<&mut ScoreReel>,
    mut sprites: Query<&mut Sprite>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    for mut reel in &mut reels {
        let base = reel.base;
        let motor_steps = reel.motor_steps;
        let interval = reel.update_interval_ms.max(1.0);
        let sound = reel.sound.clone();

        reel.until_next_step_ms -= dt_ms;
        // Cap the catch-up so a long pause cannot spin every wheel at once.
        let mut budget = reel.digits.len() as u32 * motor_steps + motor_steps;
        while reel.until_next_step_ms <= 0.0 && budget > 0 {
            reel.until_next_step_ms += interval;
            budget -= 1;
            // Right to left, like an odometer.
            for i in (0..reel.digits.len()).rev() {
                if reel.digits[i].pulses != 0 && reel.digits[i].step_count == 0 {
                    reel.digits[i].step_count = motor_steps;
                    if let Some(sound) = &sound {
                        commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN));
                    }
                }
                if reel.digits[i].step_count != 0 {
                    reel.digits[i].step_count -= 1;
                    if reel.digits[i].step_count == 0 {
                        let dir = reel.digits[i].pulses.signum();
                        reel.digits[i].pulses -= dir;
                        reel.digits[i].value += dir;
                        if reel.digits[i].value < 0 {
                            reel.digits[i].value += base;
                            if i > 0 {
                                reel.digits[i - 1].pulses -= 1;
                            }
                        } else if reel.digits[i].value >= base {
                            reel.digits[i].value -= base;
                            if i > 0 {
                                reel.digits[i - 1].pulses += 1;
                            }
                        }
                    }
                }
            }
        }

        // Show the current digit on each wheel.
        for digit in &reel.digits {
            if let Ok(mut sprite) = sprites.get_mut(digit.sprite)
                && let Some(atlas) = &mut sprite.texture_atlas
            {
                atlas.index = digit.value.clamp(0, base - 1) as usize;
            }
        }
    }
}

/// What a reel needs to render, gathered by the gameitem and credit spawners.
struct ReelSpec {
    name: String,
    image: Handle<Image>,
    /// Atlas cell size in pixels, one per digit value.
    cell: UVec2,
    /// Number of atlas cells (`digit_range + 1`).
    columns: u32,
    /// Digit base, `digit_range + 1`.
    base: i32,
    /// World-space centre of each digit wheel, leftmost first.
    digit_centers: Vec<Vec2>,
    /// On-screen size of one digit (metres).
    digit_size: Vec2,
    motor_steps: u32,
    update_interval_ms: f32,
    sound: Option<Handle<AudioSource>>,
}

/// Build a reel entity carrying the [`ScoreReel`] state with one digit sprite
/// per wheel. Returns the reel entity so callers can tag it (e.g. a credit reel).
fn build_reel(
    commands: &mut Commands,
    atlas_layouts: &mut Assets<TextureAtlasLayout>,
    spec: ReelSpec,
) -> Entity {
    let layout = atlas_layouts.add(TextureAtlasLayout::from_grid(
        spec.cell,
        spec.columns,
        1,
        None,
        None,
    ));
    let reel_entity = commands
        .spawn((
            Name::from(format!("Reel {}", spec.name)),
            Transform::from_xyz(0.0, 0.0, REEL_Z),
            Visibility::default(),
            DespawnOnExit(Screen::Gameplay),
        ))
        .id();

    let digits = spec
        .digit_centers
        .iter()
        .map(|center| {
            let sprite = commands
                .spawn((
                    Sprite {
                        image: spec.image.clone(),
                        texture_atlas: Some(TextureAtlas {
                            layout: layout.clone(),
                            index: 0,
                        }),
                        custom_size: Some(spec.digit_size),
                        ..default()
                    },
                    Transform::from_xyz(center.x, center.y, 0.0),
                    ChildOf(reel_entity),
                ))
                .id();
            Digit {
                value: 0,
                pulses: 0,
                step_count: 0,
                sprite,
            }
        })
        .collect();

    commands.entity(reel_entity).insert(ScoreReel {
        name: spec.name,
        base: spec.base,
        motor_steps: spec.motor_steps,
        update_interval_ms: spec.update_interval_ms,
        sound: spec.sound,
        digits,
        until_next_step_ms: spec.update_interval_ms,
    });
    reel_entity
}

/// vpinball lays score reels out in a normalized "desktop backdrop" space
/// (`EDITOR_BG_WIDTH` x `EDITOR_BG_HEIGHT`): every coordinate is divided by these
/// to get a [0,1] fraction of the backdrop. We map that fraction onto the desktop
/// backdrop quad (see [`DesktopLayout`]), so a reel lands over the window printed
/// for it in the backdrop image.
const EDITOR_BG_WIDTH: f32 = 1000.0;
const EDITOR_BG_HEIGHT: f32 = 750.0;

/// Spawn an animated reel from its Reel gameitem, laid out exactly as vpinball's
/// desktop renderer does (`DispReel::Render`): digits stride across the backdrop
/// from `ver1` by `width + reel_spacing`, each inset by `reel_spacing`.
pub(super) fn spawn_reel(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    atlas_layouts: &mut Assets<TextureAtlasLayout>,
    vpx_asset: &VpxAsset,
    layout: &DesktopLayout,
    reel: &reel::Reel,
) {
    if !reel.is_visible {
        return;
    }
    let Some((image, image_data)) = reel_image(vpx_asset, &reel.image) else {
        warn!("Reel {} image '{}' not found", reel.name, reel.image);
        return;
    };

    let count = reel.reel_count.max(1.0) as u32;
    let digit_range = reel.digit_range.max(1.0) as i32;
    let base = digit_range + 1;
    let columns = base as u32;
    let cell = UVec2::new((image_data.0 / columns).max(1), image_data.1.max(1));

    // Fractions of the backdrop, matching dispreel.cpp.
    let render_w = reel.width / EDITOR_BG_WIDTH;
    let render_h = reel.height / EDITOR_BG_HEIGHT;
    let spacing_x = reel.reel_spacing / EDITOR_BG_WIDTH;
    let spacing_y = reel.reel_spacing / EDITOR_BG_HEIGHT;
    let x1 = reel.ver1.x / EDITOR_BG_WIDTH + spacing_x;
    let y1 = reel.ver1.y / EDITOR_BG_HEIGHT + spacing_y;
    let digit_centers = (0..count)
        .map(|d| {
            let fx = x1 + d as f32 * (spacing_x + render_w) + render_w * 0.5;
            layout.to_world(fx, y1 + render_h * 0.5)
        })
        .collect();

    let sound = vpx_asset
        .named_sounds
        .get(&reel.sound.to_lowercase().into_boxed_str())
        .cloned();

    build_reel(
        &mut parent.commands(),
        atlas_layouts,
        ReelSpec {
            name: reel.name.clone(),
            image,
            cell,
            columns,
            base,
            digit_centers,
            digit_size: layout.to_world_size(render_w, render_h),
            motor_steps: reel.motor_steps.max(1.0) as u32,
            update_interval_ms: reel.update_interval as f32,
            sound,
        },
    );
}

/// Spawn a single-window credit reel filling a textbox's box with the credit
/// strip `image` (cells 0..=digit_range). Returns the reel entity to tag and
/// drive from the textbox value; `None` if the image is missing.
pub(crate) fn spawn_credit_reel(
    commands: &mut Commands,
    atlas_layouts: &mut Assets<TextureAtlasLayout>,
    images: &mut Assets<Image>,
    vpx_asset: &VpxAsset,
    layout: &DesktopLayout,
    textbox: &textbox::TextBox,
    image: &str,
    digit_range: i32,
) -> Option<Entity> {
    let (image_handle, (img_w, img_h)) = reel_image(vpx_asset, image)?;
    // The backdrop already prints the credit window (white), so the strip's own
    // white background would overflow it; key it out so only the digit shows.
    let image_handle = white_keyed(images, &image_handle);
    let columns = (digit_range.max(1) + 1) as u32;
    let cell = UVec2::new((img_w / columns).max(1), img_h.max(1));

    // The credit textbox is a backdrop element too, so it lives in the same
    // normalized space; centre over its box on the backdrop window.
    let fx = (textbox.ver1.x + textbox.ver2.x) * 0.5 / EDITOR_BG_WIDTH;
    let fy = (textbox.ver1.y + textbox.ver2.y) * 0.5 / EDITOR_BG_HEIGHT;
    let box_w = (textbox.ver2.x - textbox.ver1.x).abs() / EDITOR_BG_WIDTH;
    let box_h = (textbox.ver2.y - textbox.ver1.y).abs() / EDITOR_BG_HEIGHT;
    let center = layout.to_world(fx, fy);

    // Unlike a Reel gameitem (which vpinball stretches to its box), the credit is
    // a textbox standing in for rendered text, so keep the digit cell's aspect
    // ratio - fit it inside the box instead of stretching the square strip cell
    // into the box's (wider) shape, which fattens the digit.
    let box_size = layout.to_world_size(box_w, box_h);
    let cell_aspect = cell.x as f32 / cell.y as f32;
    let digit_size = if box_size.x > box_size.y * cell_aspect {
        Vec2::new(box_size.y * cell_aspect, box_size.y)
    } else {
        Vec2::new(box_size.x, box_size.x / cell_aspect)
    };

    Some(build_reel(
        commands,
        atlas_layouts,
        ReelSpec {
            name: textbox.name.clone(),
            image: image_handle,
            cell,
            columns,
            base: columns as i32,
            digit_centers: vec![center],
            digit_size,
            motor_steps: 1,
            update_interval_ms: 60.0,
            sound: None,
        },
    ))
}

/// A copy of `handle` with near-white pixels made transparent, so a strip drawn
/// over a printed window shows only its digit, not its white background. Returns
/// the original handle unchanged if the image is unavailable or not 8-bit RGBA.
fn white_keyed(images: &mut Assets<Image>, handle: &Handle<Image>) -> Handle<Image> {
    let Some(src) = images.get(handle) else {
        return handle.clone();
    };
    let w = src.texture_descriptor.size.width as usize;
    let h = src.texture_descriptor.size.height as usize;
    let Some(data) = src.data.as_ref() else {
        return handle.clone();
    };
    if w == 0 || h == 0 || data.len() != w * h * 4 {
        return handle.clone();
    }
    let mut data = data.clone();
    for px in data.chunks_exact_mut(4) {
        // Colour order (BGRA/RGBA) is irrelevant: white is high in all channels.
        if px[0] > 230 && px[1] > 230 && px[2] > 230 {
            px[3] = 0;
        }
    }
    let mut img = src.clone();
    img.data = Some(data);
    images.add(img)
}

/// The image handle and its pixel size for a reel/credit strip.
fn reel_image(vpx_asset: &VpxAsset, name: &str) -> Option<(Handle<Image>, (u32, u32))> {
    let image = vpx_asset.image(name)?.clone();
    let data = vpx_asset
        .raw
        .images
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(name))?;
    Some((image, (data.width, data.height)))
}
