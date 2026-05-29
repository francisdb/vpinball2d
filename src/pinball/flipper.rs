//! Flippers, modelled after Visual Pinball.
//!
//! Researched against the upstream Visual Pinball sources (`src/physics/hitflipper.cpp`,
//! `src/parts/flipper.cpp`) and the `exampleTable.vpx` flipper data:
//!
//!   - A flipper is a rod pivoting at its `center`. It rests at `start_angle` and the
//!     solenoid rotates it to `end_angle`; the angle is clamped to the
//!     `[min(start, end), max(start, end)]` range. VPX angles are in degrees with 0
//!     pointing up and positive angles going clockwise.
//!   - VPX has no left/right notion in the geometry. The swing sense comes from a single
//!     flag `m_direction = (end_angle >= start_angle)`: a right-hand flipper increases
//!     its angle towards the end position, a left-hand flipper decreases it. For a
//!     standard table this lines up with the left/right flipper buttons.
//!   - The coil applies a strong torque towards `end_angle` while the button is held; on
//!     release a weaker spring torque (coil strength * return ratio) pulls it back to
//!     `start_angle`. Near the end of stroke VPX also damps the torque (the "EOS" hold
//!     coil), which we do not model yet.
//!   - The example table is mirror-symmetric: LeftFlipper `120.5 deg -> 70 deg`,
//!     RightFlipper `-120.5 deg -> -70 deg`, centres at x=278 and x=596 vpu.
//!
//! We map VPX angles into bevy (0 points +x, positive counter-clockwise) and drive the
//! bat with a `RevoluteJoint` plus a `ConstantTorque`. The right flipper is the mirror of
//! the left: it pivots on the opposite end of the bat with its body angle turned half a
//! turn, which keeps the joint's relative rotation within (-PI, PI] - the range avian's
//! angle limits compare against (`rotation_difference` comes from `Rotation::angle_between`).

use crate::PausableSystems;
use crate::screens::Screen;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::Mesh;
use bevy::prelude::*;
use core::f32::consts::{PI, TAU};
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

/// Torque the solenoid applies while the flipper button is held.
/// TODO Most flippers also reduce the torque when the flipper is fully extended to avoid burning out the coil.
///   In Visual Pinball this is the "EOS" (end-of-stroke) torque damping near the end angle.
const FLIPPER_ENABLED_TORQUE: f32 = 1.5;
/// Weaker torque from the return spring while the button is released.
/// Visual Pinball models the return as the coil strength scaled by the flipper's
/// return ratio (see `FlipperMoverObject::UpdateVelocities` in hitflipper.cpp).
const FLIPPER_RETURN_TORQUE: f32 = 0.5;

#[derive(Component)]
struct Flipper {
    #[allow(dead_code)]
    pub name: String,
    /// Body angle (rad) the flipper rests at when released (Visual Pinball start angle).
    rest_angle: f32,
    /// Body angle (rad) the flipper swings to while energised (Visual Pinball end angle).
    active_angle: f32,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        flipper_movement
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(super) fn spawn_flipper(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    flipper: &vpx::gameitem::flipper::Flipper,
) {
    // Visual Pinball rests the flipper at `start_angle` and the solenoid rotates it to
    // `end_angle`. In vpinball an angle is 0 when the flipper points up and positive
    // angles go clockwise; in bevy 0 points right (+x) and positive angles go
    // counter-clockwise, so the tip direction for each position converts as:
    let rest_tip_dir = (90.0 - flipper.start_angle).to_radians();
    let active_tip_dir = (90.0 - flipper.end_angle).to_radians();

    // Visual Pinball's `m_direction = (end_angle >= start_angle)`: a right-hand flipper
    // increases its angle towards the end position, a left-hand flipper decreases it.
    let right_hand = flipper.end_angle >= flipper.start_angle;

    let shape_flipper = Rectangle::new(
        vpu_to_m(flipper.flipper_radius_max + flipper.end_radius / 2.0),
        0.018,
    );

    // The bat is a rod pivoting at the flipper centre (its base), extending towards the
    // tip. A left flipper's body +x axis points at the tip; a right flipper is the mirror
    // image, pivoting on its other end. Mirroring keeps the joint's relative rotation
    // within (-PI, PI], which is what avian's angle limits compare against.
    let (flipper_pivot, body_turn) = if right_hand {
        (Vec2::new(shape_flipper.half_size.x, 0.0), PI)
    } else {
        (Vec2::new(-shape_flipper.half_size.x, 0.0), 0.0)
    };
    let rest_angle = normalize_angle(rest_tip_dir - body_turn);
    let active_angle = normalize_angle(active_tip_dir - body_turn);
    let (min_angle, max_angle) = (rest_angle.min(active_angle), rest_angle.max(active_angle));

    // this will be overridden by the joint transform
    // TODO place it correctly
    let base_pos = Vec2::new(0.0, -0.5);

    let anchor = parent
        .spawn((
            Name::from(format!("Flipper {} Anchor", flipper.name)),
            Mesh2d(meshes.add(Mesh::from(Circle::new(0.005)))),
            MeshMaterial2d(materials.add(ColorMaterial::from(Color::from(css::YELLOW)))),
            RigidBody::Static,
            Transform::from_xyz(
                vpx_to_bevy_transform.translation.x + vpu_to_m(flipper.center.x),
                vpx_to_bevy_transform.translation.y - vpu_to_m(flipper.center.y),
                0.1, // TODO use flipper.height
            ),
        ))
        .id();

    let mesh = meshes.add(Mesh::from(shape_flipper));
    let material = materials.add(ColorMaterial::from(Color::from(css::ANTIQUE_WHITE)));
    let flipper_entity = parent
        .spawn((
            Flipper {
                name: flipper.name.clone(),
                rest_angle,
                active_angle,
            },
            Name::from(format!("Flipper {}", flipper.name)),
            Mesh2d(mesh),
            MeshMaterial2d(material),
            RigidBody::Dynamic,
            Collider::rectangle(
                shape_flipper.half_size.x * 2.0,
                shape_flipper.half_size.y * 2.0,
            ),
            //SleepingDisabled,
            Mass::from(1.0),
            // flippers have rubbers that make them bouncy
            Restitution::from(0.4),
            Transform::from_xyz(base_pos.x, base_pos.y, 0.0),
        ))
        .id();

    parent.spawn((
        Name::from(format!("Flipper {} Joint", flipper.name)),
        RevoluteJoint::new(anchor, flipper_entity)
            .with_local_anchor1(Vec2::ZERO)
            .with_local_anchor2(flipper_pivot)
            .with_angle_limits(min_angle, max_angle),
    ));
}

/// Wrap an angle into the (-PI, PI] range expected by the revolute joint limits.
fn normalize_angle(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

fn flipper_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    flippers: Query<(Entity, &Flipper)>,
    mut commands: Commands,
) {
    for (entity, flipper) in &flippers {
        // The solenoid drives towards the active angle, so the sign of the swing tells us
        // which way the flipper turns: a counter-clockwise (positive) swing is a left-hand
        // flipper, a clockwise (negative) one is right-hand. Visual Pinball has no left/right
        // flipper concept of its own - the table script binds each named flipper to
        // LeftFlipperKey / RightFlipperKey - so we map it to the matching button here.
        let towards_active = (flipper.active_angle - flipper.rest_angle).signum();
        let pressed = if towards_active > 0.0 {
            keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::ShiftLeft)
        } else {
            keyboard_input.pressed(KeyCode::ArrowRight)
                || keyboard_input.pressed(KeyCode::ShiftRight)
        };

        // While held, drive towards the active angle; when released the return spring pulls
        // back to rest with a weaker torque. Gravity alone is not enough to hold the flipper
        // down, so we always apply a torque towards one of the two limits.
        let torque = if pressed {
            towards_active * FLIPPER_ENABLED_TORQUE
        } else {
            -towards_active * FLIPPER_RETURN_TORQUE
        };
        commands.entity(entity).insert(ConstantTorque(torque));
    }
}
