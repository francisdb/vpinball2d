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
//! see [`plate_mesh`]), so the vpx `image` maps correctly rather than the whole texture atlas being
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
use vpin::vpx::mesh::spinners::{SPINNER_PLATE_INDICES, SPINNER_PLATE_MESH};
use vpin::vpx::units::vpu_to_m;

/// A plate-mesh vertex with this normal.y or more belongs to the front face (the flat disk facing
/// out of the playfield); the rest is the back face and the rim, which the 2D blade does not use.
const FRONT_FACE_NY: f32 = 0.9;
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
    /// The vpx spinner name, the script event prefix (`<name>_spin`).
    name: String,
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
    let half = plate_half_extents();
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

    let texture = vpx_asset.image(spinner.image.as_str()).cloned();
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
            name: spinner.name.clone(),
            last_click: 0,
            next_click_at: 0.0,
        },
        Name::from(format!("Spinner {}", spinner.name)),
        transform,
        Visibility::default(),
        // A sensor over the plate footprint: the ball passes over the spinner without being
        // blocked. It is on the (unscaled) parent so the blade's foreshortening never resizes it.
        RigidBody::Static,
        Collider::rectangle(2.0 * half.x * scale, 2.0 * half.y * scale),
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

/// The spinner plate's front-face triangles, extracted from vpin's plate mesh in mesh units:
/// `(positions [x along shaft, z across], texture uvs, indices)`.
///
/// vpin's plate mesh is a full 3D disk; we keep only the triangles whose every vertex faces out of
/// the playfield (`ny > FRONT_FACE_NY`), which is the flat front disk without the back face or rim.
/// Filtering by triangle (not just by vertex) matters: stray front-facing verts on the rim and
/// wire sit well outside the disk, so a per-vertex bound would oversize it.
fn front_face() -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<u32>) {
    let mut positions: Vec<[f32; 2]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut remap: Vec<Option<u32>> = vec![None; SPINNER_PLATE_MESH.len()];

    let is_front = |i: u16| SPINNER_PLATE_MESH[i as usize].ny > FRONT_FACE_NY;
    for tri in SPINNER_PLATE_INDICES.chunks_exact(3) {
        if !tri.iter().all(|&i| is_front(i)) {
            continue;
        }
        for &i in tri {
            // `remap[i]` is a `Copy` Option, so the match takes a copy and the None arm is free to
            // push to / mutate the buffers without a lingering borrow of `remap`.
            let next = match remap[i as usize] {
                Some(n) => n,
                None => {
                    let v = &SPINNER_PLATE_MESH[i as usize];
                    positions.push([v.x, v.z]);
                    uvs.push([v.tu, v.tv]);
                    let new = positions.len() as u32 - 1;
                    remap[i as usize] = Some(new);
                    new
                }
            };
            indices.push(next);
        }
    }
    (positions, uvs, indices)
}

/// Half-extents (mesh units) of the plate's front face along the shaft (x) and across it (z); used
/// to size the sensor.
fn plate_half_extents() -> Vec2 {
    front_face().0.iter().fold(Vec2::ZERO, |half, [x, z]| {
        half.max(Vec2::new(x.abs(), z.abs()))
    })
}

/// Build the spinner plate's front-face mesh (real shape and texture UVs), scaled to metres.
fn plate_mesh(scale: f32) -> Mesh {
    let (positions, uvs, indices) = front_face();
    let positions: Vec<[f32; 3]> = positions
        .iter()
        .map(|[x, z]| [x * scale, z * scale, 0.0])
        .collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
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
    mut spinner_spun: MessageWriter<crate::scripting::SpinnerSpun>,
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
            // Table scripts score every spin (`<name>_spin`), unthrottled.
            spinner_spun.write(crate::scripting::SpinnerSpun {
                name: spinner.name.clone(),
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// vpin's plate front face is a flat fan: 21 vertices and 20 triangles. Guards against an
    /// upstream mesh change silently altering the rendered plate.
    #[test]
    fn plate_front_face_shape() {
        let mesh = plate_mesh(1.0);
        let Some(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("plate mesh has no positions");
        };
        assert_eq!(positions.len(), 21, "front-face vertex count");
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("plate mesh has no u32 indices");
        };
        assert_eq!(indices.len(), 60, "20 front-face triangles");
        // Half-extents stay close to the disk the embedded data described (~0.36 x ~0.28).
        let half = plate_half_extents();
        assert!((half.x - 0.363).abs() < 0.01, "half x was {}", half.x);
        assert!((half.y - 0.281).abs() < 0.01, "half z was {}", half.y);
    }
}
