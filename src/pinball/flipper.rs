use crate::PausableSystems;
use crate::screens::Screen;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::Mesh;
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::vpu_to_m;

/// Flippers have a solenoid that applies a strong torque when activated.
/// TODO Most flippers also reduce the torque when the flipper is fully extended to avoid burning out the coil.
///   not sure if this is distance or time based in real machines
///   To check how this is modeled in Visual Pinball
const FLIPPER_ENABLED_TORQUE: f32 = 1.5;
/// The flipper assembly contains a spring that pulls the flipper down when not activated.
const FLIPPER_DISABLED_TORQUE: f32 = -0.5;

// Typical pinball flipper extents involve a maximum upward swing of about 20 degrees for each flipper,
// and a swing of 55-60 degrees from their resting position.
const FLIPPER_MAX_UP_ANGLE: f32 = 20.0_f32.to_radians();
const FLIPPER_MAX_DOWN_ANGLE: f32 = 35.0_f32.to_radians();

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum FlipperDirection {
    Left,
    Right,
}

#[derive(Component)]
struct Flipper {
    #[allow(dead_code)]
    pub name: String,
    pub direction: FlipperDirection,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        left_flipper_movement
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    )
    .add_systems(
        Update,
        right_flipper_movement
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
    // TODO how do we know the flipper orientation and what button they should be assigned to?

    // in vpinball an angle is 0 when the flipper is pointing up, positive angles go clockwise
    // for bevy we want 0 to be horizontal pointing right, positive angles go counter-clockwise
    let bevy_start_angle = (90.0 - flipper.end_angle).to_radians();
    let bevy_end_angle = (90.0 - flipper.start_angle).to_radians();
    // determine if we have a left or right flipper based on the angles
    let (min_angle, max_angle, direction) = if bevy_start_angle > bevy_end_angle {
        (bevy_start_angle, bevy_end_angle, FlipperDirection::Left)
    } else {
        (bevy_end_angle, bevy_start_angle, FlipperDirection::Right)
    };

    // skip right flipper for now
    if direction == FlipperDirection::Right {
        return;
    }

    let shape_flipper = Rectangle::new(
        vpu_to_m(flipper.flipper_radius_max + flipper.end_radius / 2.0),
        0.018,
    );

    // this will be overridden by the joint transform
    // TODO place it correctly
    let base_pos = Vec2::new(0.0, -0.5);
    let flipper_pivot = Vec2::new(-shape_flipper.half_size.x, 0.0);

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
                direction,
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
            .with_angle_limits(max_angle, min_angle),
    ));
}

fn left_flipper_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut flippers: Query<(Entity, &Flipper)>,
    mut commands: Commands,
) {
    // TODO we could probably be smarter on the key presses
    for (entity, _flipper) in flippers
        .iter_mut()
        .filter(|(_, flipper)| flipper.direction == FlipperDirection::Left)
    {
        if keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::ShiftLeft)
        {
            commands
                .entity(entity)
                .insert(ConstantTorque(FLIPPER_ENABLED_TORQUE));
        } else {
            // since gravity is not pulling enough, we force a torque in the opposite direction
            // commands.entity(flipper).remove::<ConstantTorque>();
            commands
                .entity(entity)
                .insert(ConstantTorque(FLIPPER_DISABLED_TORQUE));
        }
    }
}

fn right_flipper_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut flippers: Query<(Entity, &Flipper)>,
    mut commands: Commands,
) {
    for (entity, _flipper) in flippers
        .iter_mut()
        .filter(|(_, flipper)| flipper.direction == FlipperDirection::Right)
    {
        if keyboard_input.pressed(KeyCode::ArrowRight)
            || keyboard_input.pressed(KeyCode::ShiftRight)
        {
            commands
                .entity(entity)
                .insert(ConstantTorque(-FLIPPER_ENABLED_TORQUE));
        } else {
            // since gravity is not pulling enough we force a torque in the opposite direction
            //commands.entity(flipper).remove::<ConstantTorque>();
            commands
                .entity(entity)
                .insert(ConstantTorque(-FLIPPER_DISABLED_TORQUE));
        }
    }
}
