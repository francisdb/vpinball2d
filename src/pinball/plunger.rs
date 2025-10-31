use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::vpu_to_m;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(FixedUpdate, launcher_movement);
}

#[derive(Component)]
pub struct Plunger {
    #[allow(dead_code)]
    pub name: String,
    start_point: Vec2,
    stroke: f32,
}

pub(super) fn spawn_plunger(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    plunger: &vpx::gameitem::plunger::Plunger,
    vpx_asset: &VpxAsset,
) {
    let launcher_pos = Vec2::new(
        vpx_to_bevy_transform.translation.x + vpu_to_m(plunger.center.x),
        vpx_to_bevy_transform.translation.y - vpu_to_m(plunger.center.y),
    );

    let transform = Transform::from_xyz(
        launcher_pos.x,
        launcher_pos.y + vpu_to_m(plunger.stroke),
        vpu_to_m(plunger.height),
    );
    info!("spawning plunger {} at {:?}", plunger.name, transform);
    info!("plunger: {:?}", plunger);

    // the width is the width of the whole assembly
    let shape_launcher = Rectangle::new(vpu_to_m(plunger.width), vpu_to_m(plunger.height));

    // Create a fixed anchor for the spring
    let anchor = parent
        .spawn((
            Name::from("Launcher Anchor"),
            Mesh2d(meshes.add(Circle::new(0.005))),
            MeshMaterial2d(materials.add(Color::from(css::BLUE_VIOLET))),
            RigidBody::Static,
            Transform::from_xyz(
                launcher_pos.x,
                launcher_pos.y,
                // just to make it visible above the table
                vpu_to_m(plunger.height + 0.01),
            ),
        ))
        .id();

    // Spawn the launcher with spring joint
    let launcher = parent
        .spawn((
            Plunger {
                name: plunger.name.clone(),
                start_point: launcher_pos,
                stroke: vpu_to_m(plunger.stroke),
            },
            Name::from(format!("Plunger {}", plunger.name)),
            transform,
            Mesh2d(meshes.add(shape_launcher)),
            MeshMaterial2d(materials.add(Color::from(css::CORNFLOWER_BLUE))),
            // physics
            RigidBody::Dynamic,
            Collider::rectangle(shape_launcher.size().x, shape_launcher.size().y),
            Restitution::new(0.5), // rubber
            ConstantForce::new(0.0, 0.0),
            LockedAxes::ROTATION_LOCKED.lock_translation_x(),
            Mass::from(0.2), // Light mass for responsive spring
            SweptCcd::default(),
        ))
        .id();

    // hidden wall just above the anchor to avoid the plunger getting pulled through
    parent.spawn((
        Name::from("Plunger stop"),
        RigidBody::Static,
        Collider::rectangle(vpu_to_m(plunger.width), 0.01),
        Transform::from_xyz(
            launcher_pos.x,
            launcher_pos.y + 0.004,
            vpu_to_m(plunger.height),
        ),
    ));

    // Add prismatic joint (vertical slider) with spring properties
    parent.spawn((
        DistanceJoint::new(anchor, launcher)
            .with_local_anchor1(Vec2::ZERO)
            .with_local_anchor2(Vec2::ZERO)
            .with_compliance(0.002)
            .with_min_distance(vpu_to_m(plunger.stroke))
            .with_max_distance(vpu_to_m(plunger.stroke)),
        // avoid bouncing
        JointDamping {
            linear: 20.0,
            angular: 0.0,
        },
    ));
}

fn launcher_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut launchers: Query<(&Plunger, &Transform, &mut ConstantForce)>,
    time: Res<Time>,
) {
    // Newtons per second applied when pulling the plunger
    const PULL_FORCE_PER_SECOND: f32 = 20.0;
    const MAX_FORCE: f32 = 50.0;

    let dt = time.delta_secs();
    let delta_force = PULL_FORCE_PER_SECOND * dt;

    for (plunger, transform, mut constant_force) in launchers.iter_mut() {
        let current_offset = transform.translation.y - plunger.start_point.y;

        if keyboard_input.pressed(KeyCode::Enter) {
            // Apply downward force if not at max stretch
            if current_offset > -plunger.stroke && constant_force.y > -MAX_FORCE {
                constant_force.y -= delta_force;
                debug!("Pulling launcher down: force.y = {}", constant_force.y);
            }
        } else {
            // We can't use keyboard_input.just_released because the system runs in FixedUpdate
            // https://github.com/bevyengine/bevy/issues/6183
            constant_force.y = 0.0;
        }
    }
}
