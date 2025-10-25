use crate::PausableSystems;
use crate::pinball::ball::Ball;
use crate::pinball::table::TableAssets;
use crate::vpx::VpxAsset;
use avian2d::math::Scalar;
use avian2d::prelude::*;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::screens::Screen;
use bevy::sprite_render::AlphaMode2d;
use rand::Rng;
use vpin::vpx;
use vpin::vpx::gameitem;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        handle_bumper_collisions
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

#[derive(Component)]
struct Bumper {
    force: Scalar,
}

pub(super) fn spawn_bumper(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    bumper: &gameitem::bumper::Bumper,
) {
    // TODO we might want to create the mesh in the asset loader instead
    let base_radius = vpx::vpu_to_m(bumper.radius);
    // TODO check how big the default cap is in vpinball
    let cap_radius = base_radius + 0.015;
    let mesh = Mesh::from(Circle {
        radius: vpx::vpu_to_m(bumper.radius),
    });
    let vpx_cap_material = vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == bumper.cap_material)
        .unwrap();
    let vpx_base_material = vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == bumper.base_material)
        .unwrap();

    let base_material = materials.add(ColorMaterial {
        color: Srgba::rgb_u8(
            vpx_base_material.base_color.r,
            vpx_base_material.base_color.g,
            vpx_base_material.base_color.b,
        )
        .into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });
    let cap_material = materials.add(ColorMaterial {
        color: Srgba::rgba_u8(
            vpx_cap_material.base_color.r,
            vpx_cap_material.base_color.g,
            vpx_cap_material.base_color.b,
            210, // slightly transparent
        )
        //.darker(0.2)
        .into(),
        // TODO we want to create a proper transparent plastic material type
        alpha_mode: AlphaMode2d::Blend,
        texture: None,
        ..default()
    });

    // use bumper.center to modify the transform
    let transform = Transform::from_xyz(
        vpx::vpu_to_m(bumper.center.x) + vpx_to_bevy_transform.translation.x,
        -vpx::vpu_to_m(bumper.center.y) + vpx_to_bevy_transform.translation.y,
        0.1,
    );
    // not sure what vpinball uses as force but we want newtons
    let force = bumper.force * 0.008;
    parent.spawn((
        Bumper { force },
        Name::from(format!("Bumper{}", bumper.name)),
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(base_material),
        transform,
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(base_radius),
        children![(
            Name::from(format!("Bumper Cap {}", bumper.name)),
            Mesh2d(meshes.add(Mesh::from(Circle { radius: cap_radius }))),
            MeshMaterial2d(cap_material),
            Transform::from_xyz(0.0, 0.0, 0.01),
        ),],
    ));
}

fn handle_bumper_collisions(
    bumper_query: Query<(Entity, &Bumper, &Transform)>,
    mut ball_query: Query<(&Transform, Forces), With<Ball>>,
    mut contact_events: MessageReader<CollisionStart>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for contact_event in contact_events.read() {
        for (bumper_entity, bumper, bumper_transform) in bumper_query.iter() {
            if let (Some(h1), Some(h2)) = (contact_event.body1, contact_event.body2)
                && (h1 == bumper_entity || h2 == bumper_entity)
            {
                let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
                // random sound number between 1 and 4
                // TODO we might want to store these handles in a resource to avoid looking them up every time
                let sound_index = rand::rng().random_range(1..=4);
                let sound_ball_collision = vpx_asset
                    .named_sounds
                    .get(format!("fx_bumper{sound_index}").as_str())
                    .unwrap()
                    .clone();

                commands.spawn((
                    AudioPlayer::new(sound_ball_collision.clone()),
                    PlaybackSettings::ONCE.with_spatial(true),
                    //.with_volume(Volume::Linear(volume)),
                    Transform::from_translation(bumper_transform.translation),
                ));

                // Apply outward pulse to the ball
                let ball_entity = if h1 == bumper_entity { h2 } else { h1 };
                if let Ok((ball_transform, mut forces)) = ball_query.get_mut(ball_entity) {
                    // Calculate direction from bumper center to ball
                    let bumper_pos = bumper_transform.translation.truncate();
                    let ball_pos = ball_transform.translation.truncate();
                    let direction = (ball_pos - bumper_pos).normalize();

                    forces.apply_linear_impulse(direction * bumper.force);
                }
            }
        }
    }
}
