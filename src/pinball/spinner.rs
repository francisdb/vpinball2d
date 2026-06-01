//! Spinner, ported from vpinball's `Spinner` gameitem.
//!
//! A spinner is a flat plate on a horizontal shaft: the ball passes over it (a sensor, so it does
//! not block the ball) and spins it. The real plate rotates out of the playfield plane, which a
//! 2D top-down view cannot show directly, so we fake it: the plate is a textured rectangle whose
//! depth (perpendicular to the shaft) is foreshortened by `|cos(angle)|` as it spins - full when
//! flat, an edge-on sliver at 90 degrees. The angle is integrated as a damped pendulum (gravity
//! pulls it back to flat, `damping` from the vpx slows it), and a click sound plays each
//! half-rotation, like a real spinner.

use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::ball::Ball;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use core::f32::consts::PI;
use vpin::vpx::gameitem;
use vpin::vpx::units::vpu_to_m;

/// Spinner plate base mesh extents (mesh units), measured from vpin's spinner plate mesh: width
/// along the shaft and depth (the blade) perpendicular to it. Scaled by the item's `length`.
const PLATE_WIDTH: f32 = 1.399;
const PLATE_DEPTH: f32 = 0.563;
/// How much of the ball's speed across the shaft (m/s) becomes spin (rad/s). TODO calibrate.
const BALL_COUPLING: f32 = 10.0;
/// Gravity restoring the plate to flat. Weak, so a free spinner spins many turns before settling
/// (escape speed is sqrt(4 * this)). TODO calibrate.
const SPINNER_GRAVITY: f32 = 4.0;
/// Floor for the foreshortening scale so the plate never fully disappears / the collider degenerates.
const MIN_SCALE: f32 = 0.05;

/// Sounds a table plays as a spinner spins (one per half-rotation). A table enables them by
/// inserting this resource.
#[derive(Resource, Default)]
pub struct SpinnerSounds {
    pub spin: Vec<String>,
}

#[derive(Component)]
struct Spinner {
    /// Plate angle (rad); 0 is flat (fully visible).
    angle: f32,
    /// Angular velocity (rad/s).
    angular_velocity: f32,
    /// Per-frame velocity decay from the vpx `damping`.
    damping: f32,
    /// Unit vector perpendicular to the shaft (bevy space); the ball's speed along it drives spin.
    shaft_perp: Vec2,
    /// `floor(angle / PI)` last frame, to play a click each time the plate passes half a turn.
    last_click: i32,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (handle_spinner_hits, spin_spinners)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(super) fn spawn_spinner(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    spinner: &gameitem::spinner::Spinner,
) {
    if !spinner.is_visible {
        return;
    }
    let width = vpu_to_m(spinner.length * PLATE_WIDTH);
    let depth = vpu_to_m(spinner.length * PLATE_DEPTH);
    // vpx rotates around +Z; this game flips the y axis, so the bevy angle is negated.
    let shaft_angle = -spinner.rotation.to_radians();
    let transform = Transform {
        translation: Vec3::new(
            vpu_to_m(spinner.center.x) + vpx_to_bevy_transform.translation.x,
            -vpu_to_m(spinner.center.y) + vpx_to_bevy_transform.translation.y,
            // Render above the playfield at the spinner's mounting height.
            vpu_to_m(spinner.height).max(0.1),
        ),
        rotation: Quat::from_rotation_z(shaft_angle),
        scale: Vec3::ONE,
    };
    let (sin, cos) = shaft_angle.sin_cos();
    let shaft_perp = Vec2::new(-sin, cos);

    let texture = vpx_asset.named_images.get(spinner.image.as_str()).cloned();
    let material = materials.add(ColorMaterial {
        color: material_color(vpx_asset, &spinner.material),
        alpha_mode: AlphaMode2d::Opaque,
        texture,
        ..default()
    });

    parent.spawn((
        Spinner {
            angle: 0.0,
            angular_velocity: 0.0,
            damping: spinner.damping,
            shaft_perp,
            last_click: 0,
        },
        Name::from(format!("Spinner {}", spinner.name)),
        Mesh2d(meshes.add(Rectangle::new(width, depth))),
        MeshMaterial2d(material),
        transform,
        // A sensor: the ball passes over the spinner without being blocked.
        RigidBody::Static,
        Collider::rectangle(width, depth),
        Sensor,
        CollisionEventsEnabled,
    ));
}

/// The base colour of a vpx material, or white when it has none.
fn material_color(vpx_asset: &VpxAsset, material: &str) -> Color {
    vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == material)
        .map(|m| Color::srgb_u8(m.base_color.r, m.base_color.g, m.base_color.b))
        .unwrap_or(Color::WHITE)
}

/// A ball crossing a spinner imparts spin proportional to its speed across the shaft.
fn handle_spinner_hits(
    mut collision_reader: MessageReader<CollisionStart>,
    balls: Query<&LinearVelocity, With<Ball>>,
    mut spinners: Query<&mut Spinner>,
) {
    for collision in collision_reader.read() {
        let (Some(b1), Some(b2)) = (collision.body1, collision.body2) else {
            continue;
        };
        let (spinner_entity, ball_entity) = if spinners.contains(b1) {
            (b1, b2)
        } else if spinners.contains(b2) {
            (b2, b1)
        } else {
            continue;
        };
        let Ok(velocity) = balls.get(ball_entity) else {
            continue;
        };
        let Ok(mut spinner) = spinners.get_mut(spinner_entity) else {
            continue;
        };
        spinner.angular_velocity += velocity.0.dot(spinner.shaft_perp) * BALL_COUPLING;
    }
}

/// Integrate each spinner's rotation (damped pendulum), play a click each half-turn, and
/// foreshorten the plate to fake the out-of-plane spin.
fn spin_spinners(
    time: Res<Time>,
    mut commands: Commands,
    mut spinners: Query<(Entity, &mut Spinner, &mut Transform)>,
    sounds: Option<Res<SpinnerSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    let dt = time.delta_secs();
    for (entity, mut spinner, mut transform) in &mut spinners {
        // Gravity pulls the plate back to flat; damping (a per-frame factor) bleeds off speed.
        spinner.angular_velocity -= SPINNER_GRAVITY * spinner.angle.sin() * dt;
        spinner.angular_velocity *= spinner.damping.powf(dt * 60.0);
        spinner.angle += spinner.angular_velocity * dt;

        // Click sound each time the plate passes a half-turn.
        let click = (spinner.angle / PI).floor() as i32;
        if click != spinner.last_click {
            spinner.last_click = click;
            if let (Some(sounds), Some(table_assets)) = (&sounds, &table_assets) {
                play_sound_at(
                    &mut commands,
                    table_assets,
                    &assets_vpx,
                    entity,
                    &sounds.spin,
                );
            }
        }

        transform.scale.y = spinner.angle.cos().abs().max(MIN_SCALE);
    }
}
