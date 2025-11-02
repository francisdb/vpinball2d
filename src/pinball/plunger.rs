use crate::PausableSystems;
use crate::audio::spatial_sound_effect;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::vpu_to_m;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        plunger_movement
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
    app.add_systems(
        Update,
        plunger_sound
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
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
    let plunger_pos = Vec2::new(
        vpx_to_bevy_transform.translation.x + vpu_to_m(plunger.center.x),
        vpx_to_bevy_transform.translation.y - vpu_to_m(plunger.center.y) - vpu_to_m(plunger.height),
    );

    let transform = Transform::from_xyz(
        plunger_pos.x,
        plunger_pos.y + vpu_to_m(plunger.stroke),
        vpu_to_m(plunger.height),
    );

    // the width is the width of the whole assembly
    let shape_plunger = Rectangle::new(vpu_to_m(plunger.width), vpu_to_m(plunger.height));

    // Create a fixed anchor for the spring
    let anchor_entity = parent
        .spawn((
            Name::from("plunger Anchor"),
            Mesh2d(meshes.add(Circle::new(0.005))),
            MeshMaterial2d(materials.add(Color::from(css::BLUE_VIOLET))),
            RigidBody::Static,
            Transform::from_xyz(
                plunger_pos.x,
                plunger_pos.y,
                // just to make it visible above the table
                vpu_to_m(plunger.height + 0.01),
            ),
        ))
        .id();

    // Spawn the plunger with spring joint
    let plunger_entity = parent
        .spawn((
            Plunger {
                name: plunger.name.clone(),
                start_point: plunger_pos,
                stroke: vpu_to_m(plunger.stroke),
            },
            Name::from(format!("Plunger {}", plunger.name)),
            transform,
            Mesh2d(meshes.add(shape_plunger)),
            MeshMaterial2d(materials.add(Color::from(css::GHOST_WHITE))),
            // physics
            RigidBody::Dynamic,
            Collider::rectangle(shape_plunger.size().x, shape_plunger.size().y),
            Restitution::new(0.5), // rubber
            ConstantForce::new(0.0, 0.0),
            LockedAxes::ROTATION_LOCKED.lock_translation_x(),
            Mass::from(0.2), // Light mass for responsive spring
            SweptCcd::default(),
        ))
        .id();

    // left and right wall a bit below the plunger head to keep the ball aligned
    let wall_height = 0.005;
    let wall_width = 0.010;
    let wall_margin = 0.002;
    let wall_y = plunger_pos.y + vpu_to_m(plunger.stroke) - 0.010;
    let wall_z = vpu_to_m(plunger.height);
    // left wall
    parent.spawn((
        Name::from("Plunger left guide wall"),
        Mesh2d(meshes.add(Rectangle::new(wall_width, wall_height))),
        MeshMaterial2d(materials.add(Color::from(css::DARK_GRAY))),
        RigidBody::Static,
        Collider::rectangle(wall_width, wall_height),
        Transform::from_xyz(
            plunger_pos.x - vpu_to_m(plunger.width) / 2.0 - wall_width / 2.0 - wall_margin,
            wall_y,
            wall_z,
        ),
    ));
    // right wall
    parent.spawn((
        Name::from("Plunger right guide wall"),
        Mesh2d(meshes.add(Rectangle::new(wall_width, wall_height))),
        MeshMaterial2d(materials.add(Color::from(css::SILVER))),
        RigidBody::Static,
        Collider::rectangle(wall_width, wall_height),
        Transform::from_xyz(
            plunger_pos.x + vpu_to_m(plunger.width) / 2.0 + wall_width / 2.0 + wall_margin,
            wall_y,
            wall_z,
        ),
    ));

    // hidden wall just above the anchor to avoid the plunger getting pulled through
    parent.spawn((
        Name::from("Plunger stop"),
        RigidBody::Static,
        Collider::rectangle(vpu_to_m(plunger.width), 0.01),
        Transform::from_xyz(
            plunger_pos.x,
            plunger_pos.y + 0.004,
            vpu_to_m(plunger.height),
        ),
    ));

    // Add prismatic joint (vertical slider) with spring properties
    parent.spawn((
        DistanceJoint::new(anchor_entity, plunger_entity)
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

fn plunger_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut plungers: Query<(&Plunger, &Transform, &mut ConstantForce)>,
    time: Res<Time>,
) {
    // Newtons per second applied when pulling the plunger
    const PULL_FORCE_PER_SECOND: f32 = 20.0;
    const MAX_FORCE: f32 = 50.0;

    let dt = time.delta_secs();
    let delta_force = PULL_FORCE_PER_SECOND * dt;

    for (plunger, transform, mut constant_force) in plungers.iter_mut() {
        let current_offset = transform.translation.y - plunger.start_point.y;

        if keyboard_input.pressed(KeyCode::Enter) {
            // Apply downward force if not at max stretch
            if current_offset > -plunger.stroke && constant_force.y > -MAX_FORCE {
                constant_force.y -= delta_force;
                debug!("Pulling plunger down: force.y = {}", constant_force.y);
            }
        } else {
            // We can't use keyboard_input.just_released because the system runs in FixedUpdate
            // https://github.com/bevyengine/bevy/issues/6183
            constant_force.y = 0.0;
        }
    }
}

fn plunger_sound(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
    plunger_query: Query<(Entity), With<Plunger>>,
) {
    if keyboard_input.just_pressed(KeyCode::Enter) {
        // play plunger pull sound
        let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
        let sound_name = "plungerpull";
        if let Some(sound) = vpx_asset.named_sounds.get(sound_name) {
            for plunger_entity in plunger_query.iter() {
                commands
                    .entity(plunger_entity)
                    .with_child(spatial_sound_effect(sound.clone()));
            }
        } else {
            warn!("Plunger pull sound '{}' not found in VPX asset", sound_name);
        }
    }
    if keyboard_input.just_released(KeyCode::Enter) {
        // play plunger release sound
        let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
        let sound_name = "plunger";
        if let Some(sound) = vpx_asset.named_sounds.get(sound_name) {
            for plunger_entity in plunger_query.iter() {
                commands
                    .entity(plunger_entity)
                    .with_child(spatial_sound_effect(sound.clone()));
            }
        } else {
            warn!(
                "Plunger release sound '{}' not found in VPX asset",
                sound_name
            );
        }
    }
}
