//! Gate, ported from vpinball's `Gate` gameitem.
//!
//! A gate is a wire flap hinged on a horizontal shaft across a lane. The ball pushes it open and
//! passes; the flap then falls back closed under gravity. A one-way gate (`two_way == false`) only
//! yields to a ball travelling in its open direction - a ball coming the other way bounces off it
//! like a wall. A two-way gate yields from either side.
//!
//! Translated to the 2D top-down view:
//! - The closed flap hangs straight down (perpendicular to the playfield), so from above it is
//!   edge-on, a thin sliver; fully open it lies flat and shows its whole face. We fake that swing
//!   by foreshortening the flap child's height by `sin(angle)` (0 closed, 1 open), the same trick
//!   the [`spinner`](super::spinner) uses.
//! - The one-way bounce is real physics: the gate has a thin solid collider, and a
//!   [`CollisionHooks`] implementation ([`GateCollisionHooks`]) ignores the contact (lets the ball
//!   pass) when the ball moves in the gate's open direction, mirroring vpinball's
//!   `dot(ball_velocity, gate_normal)` test. From the blocked side the contact stands and the ball
//!   bounces with the gate's elasticity.
//!
//! A sibling sensor detects a ball entering the gate region (in either direction, whether it
//! passes or bounces) to kick the swing animation and play the gate sound.

use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::ball::Ball;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::entity::hash_set::EntityHashSet;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::ecs::system::SystemParam;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use vpin::vpx::gameitem;
use vpin::vpx::units::vpu_to_m;

/// Thickness (m) of the gate's solid collider across the wire. Thin, since it sits on the hinge
/// line; the ball's swept CCD keeps it from tunnelling through.
const GATE_THICKNESS_M: f32 = 0.01;
/// Thickness (m) of the sensor band straddling the wire, used to detect a ball crossing. Wider than
/// the solid collider so a passing ball reliably registers.
const GATE_SENSOR_THICKNESS_M: f32 = 0.03;
/// How much of the ball's speed along the open direction (m/s) becomes swing speed (rad/s). High,
/// so the light flap is flung fully open by any hit rather than gently arcing. TODO calibrate.
const GATE_COUPLING: f32 = 14.0;
/// Base gravity restoring the flap to closed (scaled by the vpx `gravity_factor`). High, so the
/// flap drops shut quickly instead of floating back like a leaf. TODO calibrate.
const GATE_GRAVITY: f32 = 120.0;
/// Floor for the foreshortening scale so a closed flap never fully disappears.
const GATE_MIN_SCALE: f32 = 0.05;
/// A ball must move at least this fast (m/s) along the open direction to be let through; below it,
/// a one-way gate stays solid (it is essentially at rest against the flap).
const GATE_OPEN_EPS: f32 = 0.0;

/// Sounds a table plays when a ball hits a gate. A table enables them by inserting this resource.
#[derive(Resource, Default)]
pub struct GateSounds {
    pub hit: Vec<String>,
}

#[derive(Component)]
struct Gate {
    /// Whether the gate yields from both sides; a one-way gate (`false`) bounces a ball coming
    /// against [`open_dir`](Self::open_dir).
    two_way: bool,
    /// Unit vector (bevy space) the gate opens toward. A one-way gate passes a ball moving along
    /// `+open_dir` and bounces one moving against it.
    open_dir: Vec2,
    /// Swing angle (rad) from closed; 0 hangs down (edge-on from above), `angle_max` lies flat.
    angle: f32,
    /// Angular velocity (rad/s).
    angular_velocity: f32,
    /// Open limit (rad) from the vpx `angle_max`.
    angle_max: f32,
    /// Per-frame velocity decay from the vpx `damping`.
    damping: f32,
    /// Scales the restoring gravity, from the vpx `gravity_factor`.
    gravity_factor: f32,
    /// Balls currently allowed to pass (still penetrating), so a ball mid-pass keeps passing even
    /// as its velocity changes. Maintained by [`GateCollisionHooks`].
    passing: EntityHashSet,
}

/// The flap child of a [`Gate`]; its `Transform.scale.y` is foreshortened as the flap swings.
#[derive(Component)]
struct GateFlap;

/// A sensor sibling of a [`Gate`]; holds the gate entity it drives.
#[derive(Component)]
struct GateSensor(Entity);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (handle_gate_sensors, swing_gates)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(super) fn spawn_gate(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    gate: &gameitem::gate::Gate,
) {
    if !gate.is_collidable && !gate.is_visible {
        return;
    }
    // vpx rotates around +Z; this game flips the y axis, so the bevy angle is negated.
    let rot = -gate.rotation.to_radians();
    let (sin, cos) = rot.sin_cos();
    // The wire runs along (cos, sin); the gate opens across it. This matches vpinball's gate-line
    // normal (sin, -cos) in vpx space, mapped through the y flip.
    let open_dir = Vec2::new(-sin, cos);

    let length_m = vpu_to_m(gate.length);
    let drop_m = vpu_to_m(gate.height).max(length_m * 0.5);

    let transform = Transform {
        translation: Vec3::new(
            vpu_to_m(gate.center.x) + vpx_to_bevy_transform.translation.x,
            -vpu_to_m(gate.center.y) + vpx_to_bevy_transform.translation.y,
            // Render above the playfield.
            vpu_to_m(gate.height).max(0.1),
        ),
        rotation: Quat::from_rotation_z(rot),
        scale: Vec3::ONE,
    };

    let flap_material = materials.add(ColorMaterial::from_color(material_color(
        vpx_asset,
        &gate.material,
    )));

    let gate_entity = parent
        .spawn((
            Gate {
                two_way: gate.two_way,
                open_dir,
                angle: 0.0,
                angular_velocity: 0.0,
                angle_max: gate.angle_max.max(0.1),
                damping: gate.damping.unwrap_or(0.985),
                gravity_factor: gate.gravity_factor.unwrap_or(1.0),
                passing: EntityHashSet::default(),
            },
            Name::from(format!("Gate {}", gate.name)),
            transform,
            Visibility::default(),
            // A thin solid collider on the hinge line. The ball bounces off it from the blocked side
            // of a one-way gate; `GateCollisionHooks` lets it through from the open side.
            RigidBody::Static,
            Collider::rectangle(length_m, GATE_THICKNESS_M),
            Restitution::new(gate.elasticity),
            Friction::new(gate.friction),
            ActiveCollisionHooks::MODIFY_CONTACTS,
            // So a one-way bounce shows up in the contact log; a pass is suppressed by the hook.
            CollisionEventsEnabled,
            children![(
                GateFlap,
                Name::from("Gate flap"),
                // Flap mesh hangs from the hinge (local y 0..drop) so foreshortening pivots there.
                Mesh2d(meshes.add(flap_mesh(length_m, drop_m))),
                MeshMaterial2d(flap_material),
                Transform::default(),
            )],
        ))
        .id();

    // A sensor straddling the wire to detect a ball entering the gate (either direction), driving
    // the swing animation and sound regardless of whether the ball passes or bounces.
    parent.spawn((
        GateSensor(gate_entity),
        Name::from(format!("Gate sensor {}", gate.name)),
        transform,
        RigidBody::Static,
        Collider::rectangle(length_m, GATE_SENSOR_THICKNESS_M),
        Sensor,
        CollisionEventsEnabled,
    ));
}

/// Build the flap as a rectangle anchored at the hinge: x spans the wire length, y hangs from 0
/// (hinge) to `drop`, so scaling `y` foreshortens it about the hinge line.
fn flap_mesh(length: f32, drop: f32) -> Mesh {
    let hx = length * 0.5;
    let positions = vec![
        [-hx, 0.0, 0.0],
        [hx, 0.0, 0.0],
        [hx, drop, 0.0],
        [-hx, drop, 0.0],
    ];
    let uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let indices = vec![0u32, 1, 2, 0, 2, 3];
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

/// A ball entering a gate's sensor kicks the swing in the direction it pushes and plays the sound.
#[allow(clippy::too_many_arguments)]
fn handle_gate_sensors(
    mut collision_reader: MessageReader<CollisionStart>,
    mut commands: Commands,
    sensors: Query<&GateSensor>,
    balls: Query<&LinearVelocity, With<Ball>>,
    mut gates: Query<&mut Gate>,
    sounds: Option<Res<GateSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for collision in collision_reader.read() {
        let (Some(b1), Some(b2)) = (collision.body1, collision.body2) else {
            continue;
        };
        let (sensor_entity, ball_entity) = if sensors.contains(b1) {
            (b1, b2)
        } else if sensors.contains(b2) {
            (b2, b1)
        } else {
            continue;
        };
        let Ok(GateSensor(gate_entity)) = sensors.get(sensor_entity) else {
            continue;
        };
        let Ok(velocity) = balls.get(ball_entity) else {
            continue;
        };
        let Ok(mut gate) = gates.get_mut(*gate_entity) else {
            continue;
        };
        // Push the flap the way the ball travels; a blocked-side hit pushes it shut (clamped to 0).
        gate.angular_velocity += velocity.0.dot(gate.open_dir) * GATE_COUPLING;

        if let (Some(sounds), Some(table_assets)) = (&sounds, &table_assets) {
            play_sound_at(
                &mut commands,
                table_assets,
                &assets_vpx,
                *gate_entity,
                &sounds.hit,
            );
        }
    }
}

/// Integrate each gate's swing (damped pendulum, clamped to `0..angle_max`) and foreshorten the
/// flap to fake the out-of-plane rotation.
fn swing_gates(
    time: Res<Time>,
    mut gates: Query<(&mut Gate, &Children)>,
    mut flaps: Query<&mut Transform, With<GateFlap>>,
) {
    let dt = time.delta_secs();
    for (mut gate, children) in &mut gates {
        // Gravity pulls the flap back down to closed; damping bleeds off speed.
        gate.angular_velocity -= GATE_GRAVITY * gate.gravity_factor * gate.angle.sin() * dt;
        gate.angular_velocity *= gate.damping.powf(dt * 60.0);
        gate.angle += gate.angular_velocity * dt;

        // Clamp to the swing range and stop dead at the limits.
        if gate.angle <= 0.0 {
            gate.angle = 0.0;
            gate.angular_velocity = gate.angular_velocity.max(0.0);
        } else if gate.angle >= gate.angle_max {
            gate.angle = gate.angle_max;
            gate.angular_velocity = gate.angular_velocity.min(0.0);
        }

        // Closed (angle 0) hangs edge-on -> sliver; open (angle_max) lies flat -> full face.
        let scale_y = gate.angle.sin().clamp(GATE_MIN_SCALE, 1.0);
        for &child in children {
            if let Ok(mut flap) = flaps.get_mut(child) {
                flap.scale.y = scale_y;
            }
        }
    }
}

/// Collision hooks that make one-way gates yield only in their open direction.
///
/// For a ball/gate contact: a two-way gate always yields (contact ignored); a one-way gate yields
/// while the ball moves along its open direction, otherwise the contact stands and the ball
/// bounces. A ball already passing keeps passing until it clears the gate, so it is never trapped
/// mid-swing.
#[derive(SystemParam)]
pub struct GateCollisionHooks<'w, 's> {
    gates: Query<'w, 's, &'static Gate>,
    balls: Query<'w, 's, &'static LinearVelocity, With<Ball>>,
}

impl CollisionHooks for GateCollisionHooks<'_, '_> {
    fn modify_contacts(&self, contacts: &mut ContactPair, commands: &mut Commands) -> bool {
        let (gate_entity, ball_entity) = if self.gates.contains(contacts.collider1) {
            (contacts.collider1, contacts.collider2)
        } else if self.gates.contains(contacts.collider2) {
            (contacts.collider2, contacts.collider1)
        } else {
            // Not a gate contact; keep it. Phantom speculative contacts are now curbed by the higher
            // physics tick rate (see `main`), not filtered here - filtering dropped the speculative
            // approach contact that slingshots read for their inbound speed, breaking the kick.
            return true;
        };
        let Ok(gate) = self.gates.get(gate_entity) else {
            return true;
        };

        // Two-way gates never block.
        if gate.two_way {
            return false;
        }

        // Keep letting a ball through while it is still penetrating; forget it once it has cleared.
        if gate.passing.contains(&ball_entity) {
            let still_penetrating = contacts
                .manifolds
                .iter()
                .any(|m| m.points.iter().any(|p| p.penetration > 0.0));
            if still_penetrating {
                return false;
            }
            commands.queue(move |world: &mut World| {
                if let Some(mut gate) = world.get_mut::<Gate>(gate_entity) {
                    gate.passing.remove(&ball_entity);
                }
            });
        }

        // Let the ball through if it is moving in the open direction; otherwise bounce it.
        let Ok(velocity) = self.balls.get(ball_entity) else {
            return true;
        };
        if velocity.0.dot(gate.open_dir) > GATE_OPEN_EPS {
            commands.queue(move |world: &mut World| {
                if let Some(mut gate) = world.get_mut::<Gate>(gate_entity) {
                    gate.passing.insert(ball_entity);
                }
            });
            return false;
        }
        true
    }
}
