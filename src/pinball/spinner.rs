//! Spinner, ported from vpinball's `Spinner` gameitem.
//!
//! A spinner is a flat plate on a horizontal shaft: the ball passes over it (a sensor, so it does
//! not block the ball) and spins it. The real plate rotates out of the playfield plane, which a
//! 2D top-down view cannot show directly, so we fake it: the plate's blade (perpendicular to the
//! shaft) is foreshortened by `|cos(angle)|` as it spins - full when flat, an edge-on sliver at 90
//! degrees. The angle is integrated as a damped pendulum (gravity pulls it back to flat, `damping`
//! from the vpx slows it), and a click sound plays each half-rotation, like a real spinner.
//!
//! The plate is rendered as vpin's spinner plate front-face mesh (its real shape and texture UVs,
//! see [`PLATE_POS`]), so the vpx `image` maps correctly rather than the whole texture atlas being
//! stretched over a rectangle. The collider lives on the parent (a fixed sensor) while the blade
//! is a child that carries the foreshortening, so spinning never resizes the collider.

use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::ball::Ball;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use core::f32::consts::PI;
use vpin::vpx::gameitem;
use vpin::vpx::units::vpu_to_m;

/// Plate half-extents (mesh units) along the shaft (x) and across it (z), from [`PLATE_POS`]; used
/// to size the sensor.
const PLATE_HALF_X: f32 = 0.363;
const PLATE_HALF_Z: f32 = 0.281;
/// How much of the ball's speed across the shaft (m/s) becomes spin (rad/s). High: a real
/// spinner's plate radius is tiny (~1.5 cm), so `omega = v / r` whirrs it fast. TODO calibrate.
const BALL_COUPLING: f32 = 30.0;
/// Gravity restoring the plate to flat. Weak, so a free spinner spins many turns before settling
/// (escape speed is sqrt(4 * this)). TODO calibrate.
const SPINNER_GRAVITY: f32 = 2.0;
/// Floor for the foreshortening scale so the plate never fully disappears.
const MIN_SCALE: f32 = 0.05;
/// Minimum time between click sounds, so a fast whirr does not spawn dozens of sounds a second.
const MIN_CLICK_INTERVAL: f32 = 0.045;

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
    /// `floor(angle / PI)` last frame, to detect each time the plate passes half a turn.
    last_click: i32,
    /// Earliest elapsed time the next click sound may play (throttles a fast whirr).
    next_click_at: f32,
}

/// The plate child of a [`Spinner`]; its `Transform.scale.y` is foreshortened as the plate spins.
#[derive(Component)]
struct SpinnerBlade;

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
    // Mesh-unit -> metres scale (vpin scales the plate mesh by `length`).
    let scale = vpu_to_m(spinner.length);
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
            next_click_at: 0.0,
        },
        Name::from(format!("Spinner {}", spinner.name)),
        transform,
        Visibility::default(),
        // A sensor over the plate footprint: the ball passes over the spinner without being
        // blocked. It is on the (unscaled) parent so the blade's foreshortening never resizes it.
        RigidBody::Static,
        Collider::rectangle(2.0 * PLATE_HALF_X * scale, 2.0 * PLATE_HALF_Z * scale),
        Sensor,
        CollisionEventsEnabled,
        children![(
            SpinnerBlade,
            Name::from("Spinner blade"),
            Mesh2d(meshes.add(plate_mesh(scale))),
            MeshMaterial2d(material),
            Transform::default(),
        )],
    ));
}

/// Build the spinner plate's front-face mesh (real shape and texture UVs), scaled to metres.
fn plate_mesh(scale: f32) -> Mesh {
    let positions: Vec<[f32; 3]> = PLATE_POS
        .iter()
        .map(|[x, z]| [x * scale, z * scale, 0.0])
        .collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, PLATE_UV.to_vec());
    mesh.insert_indices(Indices::U32(PLATE_IDX.to_vec()));
    mesh
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
/// foreshorten the plate (the blade child) to fake the out-of-plane spin.
#[allow(clippy::too_many_arguments)]
fn spin_spinners(
    time: Res<Time>,
    mut commands: Commands,
    mut spinners: Query<(Entity, &mut Spinner, &Children)>,
    mut blades: Query<&mut Transform, With<SpinnerBlade>>,
    sounds: Option<Res<SpinnerSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    let dt = time.delta_secs();
    for (entity, mut spinner, children) in &mut spinners {
        // Gravity pulls the plate back to flat; damping (a per-frame factor) bleeds off speed.
        spinner.angular_velocity -= SPINNER_GRAVITY * spinner.angle.sin() * dt;
        spinner.angular_velocity *= spinner.damping.powf(dt * 60.0);
        spinner.angle += spinner.angular_velocity * dt;

        // Click sound each time the plate passes a half-turn, throttled so a fast whirr does not
        // spawn a sound every frame.
        let click = (spinner.angle / PI).floor() as i32;
        if click != spinner.last_click {
            spinner.last_click = click;
            let now = time.elapsed_secs();
            if now >= spinner.next_click_at {
                spinner.next_click_at = now + MIN_CLICK_INTERVAL;
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
        }

        let scale_y = spinner.angle.cos().abs().max(MIN_SCALE);
        for &child in children {
            if let Ok(mut blade) = blades.get_mut(child) {
                blade.scale.y = scale_y;
            }
        }
    }
}

/// Front face of vpin's spinner plate mesh (21 verts, 20 tris): local (x along
/// shaft, z perpendicular) positions in mesh units, the texture UVs, and triangle indices.
#[rustfmt::skip]
const PLATE_POS: &[[f32; 2]] = &[
    [0.32006, 0.27602],
    [-0.00000, -0.00000],
    [0.29377, 0.28126],
    [0.34236, 0.26111],
    [0.35727, 0.23881],
    [0.36252, 0.21251],
    [-0.29378, 0.28126],
    [-0.32006, 0.27602],
    [-0.34236, 0.26111],
    [-0.35727, 0.23881],
    [-0.36253, 0.21251],
    [-0.36253, -0.21251],
    [-0.35727, -0.23881],
    [-0.34236, -0.26111],
    [-0.32006, -0.27602],
    [-0.29378, -0.28126],
    [0.29378, -0.28126],
    [0.32006, -0.27602],
    [0.34236, -0.26111],
    [0.35727, -0.23881],
    [0.36253, -0.21251],
];
#[rustfmt::skip]
const PLATE_UV: &[[f32; 2]] = &[
    [0.77368, 0.01397],
    [0.50000, 0.25000],
    [0.75120, 0.00949],
    [0.79276, 0.02672],
    [0.80550, 0.04579],
    [0.80999, 0.06828],
    [0.24879, 0.00949],
    [0.22631, 0.01397],
    [0.20724, 0.02672],
    [0.19450, 0.04579],
    [0.19000, 0.06828],
    [0.19000, 0.43172],
    [0.19450, 0.45421],
    [0.20724, 0.47328],
    [0.22631, 0.48602],
    [0.24879, 0.49051],
    [0.75121, 0.49051],
    [0.77368, 0.48602],
    [0.79276, 0.47328],
    [0.80550, 0.45421],
    [0.81000, 0.43172],
];
#[rustfmt::skip]
const PLATE_IDX: &[u32] = &[
    0, 1, 2, 1, 0, 3, 1, 6, 2, 1, 3, 4,
    6, 1, 7, 1, 4, 5, 7, 1, 8, 1, 5, 20,
    8, 1, 9, 19, 1, 20, 1, 10, 9, 18, 1, 19,
    10, 1, 11, 17, 1, 18, 11, 1, 12, 16, 1, 17,
    12, 1, 13, 15, 1, 16, 13, 1, 14, 14, 1, 15,
];
