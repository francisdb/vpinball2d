//! Drop targets and standup (hit) targets, ported from vpinball's `HitTarget` gameitem.
//!
//! A standup target is a static panel the ball bounces off (elasticity/friction from the vpx
//! item); a drop target is the same until it is hit hard enough, then it "drops" - in this 2D
//! top-down view that means it hides and stops colliding - and is raised again after its delay.
//!
//! The target's flat panel is rendered as a simple rotated rectangle sized from vpin's target
//! mesh footprint (so each `TargetType` and `size`/`rot_z` comes out right) rather than the full
//! 3D mesh, matching this game's 2D style (see pinball::wall, pinball::bumper).

use crate::PausableSystems;
use crate::pinball::ball::Ball;
use crate::screens::Screen;
use avian2d::prelude::*;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use core::time::Duration;
use vpin::vpx::gameitem::hittarget::{HitTarget, TargetType};
use vpin::vpx::units::vpu_to_m;

/// vpinball measures the drop threshold in its own speed units; scale it into m/s here.
/// TODO calibrate against vpx.
const DROP_THRESHOLD_SCALE: f32 = 0.1;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (handle_drop_targets, raise_dropped_targets)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

/// A drop target: drops out of play when hit above `threshold`, raised again after `raise_after`.
#[derive(Component)]
struct DropTarget {
    /// Minimum inbound speed (m/s) for the target to drop.
    threshold: f32,
    /// Delay before the dropped target raises again (vpx `raise_delay`); never if `None`.
    raise_after: Option<Duration>,
    dropped: bool,
}

/// Counts down to raising a dropped target.
#[derive(Component)]
struct RaiseTimer(Timer);

pub(super) fn spawn_target(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    target: &HitTarget,
) {
    // Footprint (width x depth, metres) from vpin's target mesh; None when not visible.
    let Some(size) = footprint(target) else {
        return;
    };

    let transform = Transform {
        translation: Vec3::new(
            vpu_to_m(target.position.x) + vpx_to_bevy_transform.translation.x,
            -vpu_to_m(target.position.y) + vpx_to_bevy_transform.translation.y,
            0.1,
        ),
        // vpx rotates around +Z; this game flips the y axis, so the bevy angle is negated.
        rotation: Quat::from_rotation_z(-target.rot_z.to_radians()),
        scale: Vec3::ONE,
    };

    let is_drop = matches!(
        target.target_type,
        TargetType::DropTargetBeveled
            | TargetType::DropTargetSimple
            | TargetType::DropTargetFlatSimple
    );

    let mut entity = parent.spawn((
        Name::from(format!("Target {}", target.name)),
        Mesh2d(meshes.add(Rectangle::new(size.x, size.y))),
        MeshMaterial2d(materials.add(Color::srgb_u8(210, 200, 90))),
        transform,
    ));

    if target.is_collidable {
        entity.insert((
            RigidBody::Static,
            Collider::rectangle(size.x, size.y),
            Restitution::from(target.elasticity),
            Friction::from(target.friction),
            CollisionEventsEnabled,
        ));
        if is_drop {
            entity.insert(DropTarget {
                threshold: target.threshold * DROP_THRESHOLD_SCALE,
                raise_after: target
                    .raise_delay
                    .map(|ms| Duration::from_millis(ms as u64)),
                dropped: false,
            });
        }
    }
}

/// Width x depth of the target's panel in metres: the per-type base footprint scaled by the
/// item's `size`. `rot_z` is a rigid rotation so it does not change these extents (it is applied
/// through the entity transform). `None` when the target is not visible.
fn footprint(target: &HitTarget) -> Option<Vec2> {
    if !target.is_visible {
        return None;
    }
    let base = base_extent(&target.target_type);
    Some(Vec2::new(
        vpu_to_m(base.x * target.size.x),
        vpu_to_m(base.y * target.size.y),
    ))
}

/// Base footprint (width across the face x depth front-to-back, in mesh units) of each target
/// type, measured from vpin's static target meshes (vpin's mesh builder is not public). The world
/// size is this times the item's `size` (whose default is 32).
fn base_extent(target_type: &TargetType) -> Vec2 {
    let (width, depth) = match target_type {
        TargetType::DropTargetBeveled => (1.050, 0.500),
        TargetType::DropTargetSimple => (1.050, 0.350),
        TargetType::DropTargetFlatSimple => (1.100, 0.200),
        TargetType::HitTargetRound => (1.225, 0.430),
        TargetType::HitTargetRectangle => (1.350, 0.430),
        TargetType::HitFatTargetRectangle => (1.600, 0.450),
        TargetType::HitFatTargetSquare => (1.200, 0.450),
        TargetType::HitTargetSlim => (0.650, 0.430),
        TargetType::HitFatTargetSlim => (0.775, 0.450),
    };
    Vec2::new(width, depth)
}

/// Drop a drop target when a ball strikes it above its threshold, and schedule its raise.
fn handle_drop_targets(
    mut collision_reader: MessageReader<CollisionStart>,
    collisions: Collisions,
    balls: Query<(), With<Ball>>,
    mut targets: Query<(Entity, &mut DropTarget)>,
    mut commands: Commands,
) {
    for collision in collision_reader.read() {
        let (Some(b1), Some(b2)) = (collision.body1, collision.body2) else {
            continue;
        };
        // One body must be a drop target, the other a ball.
        let (target_entity, other) = if targets.contains(b1) {
            (b1, b2)
        } else if targets.contains(b2) {
            (b2, b1)
        } else {
            continue;
        };
        if balls.get(other).is_err() {
            continue;
        }
        let Ok((entity, mut drop_target)) = targets.get_mut(target_entity) else {
            continue;
        };
        if drop_target.dropped {
            continue;
        }
        // Impact speed from the contact's pre-solve `normal_speed` (negative when approaching);
        // reading the ball velocity here would be the post-bounce value (see pinball::wall).
        let inbound = collisions
            .get(collision.collider1, collision.collider2)
            .into_iter()
            .flat_map(|pair| pair.manifolds.iter())
            .flat_map(|manifold| manifold.points.iter())
            .map(|point| -point.normal_speed)
            .fold(0.0_f32, f32::max);
        if inbound < drop_target.threshold {
            continue;
        }
        drop_target.dropped = true;
        let mut e = commands.entity(entity);
        e.insert((ColliderDisabled, Visibility::Hidden));
        if let Some(delay) = drop_target.raise_after {
            e.insert(RaiseTimer(Timer::new(delay, TimerMode::Once)));
        }
    }
}

/// Raise a dropped target once its `RaiseTimer` elapses: re-enable its collider and show it.
fn raise_dropped_targets(
    time: Res<Time>,
    mut commands: Commands,
    mut targets: Query<(Entity, &mut DropTarget, &mut RaiseTimer)>,
) {
    for (entity, mut drop_target, mut timer) in &mut targets {
        if timer.0.tick(time.delta()).just_finished() {
            drop_target.dropped = false;
            commands
                .entity(entity)
                .remove::<ColliderDisabled>()
                .remove::<RaiseTimer>()
                .insert(Visibility::Inherited);
        }
    }
}
