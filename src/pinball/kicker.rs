use crate::audio::spatial_sound_effect;
use crate::pinball::ball::{Ball, ball as spawn_ball};
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::asset::Assets;
use bevy::color::Color;
use bevy::color::Srgba;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::*;
use rand::Rng;
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

#[derive(Component)]
pub struct Kicker {
    #[allow(dead_code)]
    pub name: String,
}

const KICKER_COLOR: Srgba = css::GREEN;

/// Sounds a table plays for the generic drain / ball-release cycle. A random entry is
/// picked each time, so a single-element list gives a fixed sound. A table enables the
/// generic [`handle_drain`] behaviour by inserting this resource (see the table scripts).
#[derive(Resource, Default)]
pub struct DrainSounds {
    pub drain: Vec<String>,
    pub release: Vec<String>,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        handle_drain
            .run_if(in_state(Screen::Gameplay))
            .run_if(resource_exists::<DrainSounds>),
    );
}

pub(super) fn spawn_kicker(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    kicker: &vpx::gameitem::kicker::Kicker,
) {
    let radius = vpu_to_m(kicker.radius);

    // TODO the orientation field will indicate where the ball is kicked towards
    //   0 is up (positive Y), 90 is right (positive X), etc.
    //   we can draw a small arrow or to indicate the direction visually

    parent.spawn((
        Kicker {
            name: kicker.name.clone(),
        },
        Name::from(format!("Kicker {}", kicker.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(kicker.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(kicker.center.y),
            10.0,
        ),
        Mesh2d(meshes.add(Annulus::new(radius - 0.001, radius))),
        MeshMaterial2d(materials.add(Color::from(KICKER_COLOR))),
        // physics
        CollisionEventsEnabled,
        //RigidBody::Static,
        Collider::circle(radius),
        Sensor,
    ));
}

/// Generic drain handling shared by all tables: when the ball hits the "Drain" kicker,
/// play a drain sound, despawn the ball and release a fresh one at the "BallRelease"
/// kicker with a release sound. Sounds come from the [`DrainSounds`] resource.
#[allow(clippy::too_many_arguments)]
fn handle_drain(
    mut collision_reader: MessageReader<CollisionStart>,
    name_query: Query<&Name>,
    ball_query: Query<&Ball>,
    kicker_query: Query<(Entity, &Kicker, &Transform)>,
    sounds: Res<DrainSounds>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for collision in collision_reader.read() {
        // body1/body2 are None when a collider has no associated rigid body
        let (Some(entity_a), Some(entity_b)) = (collision.body1, collision.body2) else {
            let name_of = |entity| {
                name_query
                    .get(entity)
                    .map(|name| name.to_string())
                    .unwrap_or_else(|_| format!("{entity:?}"))
            };
            warn!(
                "Ignoring collision with a collider that has no rigid body: collider1={} (body1={:?}), collider2={} (body2={:?})",
                name_of(collision.collider1),
                collision.body1,
                name_of(collision.collider2),
                collision.body2
            );
            continue;
        };

        let ball_a = ball_query.get(entity_a).ok();
        let ball_b = ball_query.get(entity_b).ok();
        let kicker_a = kicker_query.get(entity_a).ok();
        let kicker_b = kicker_query.get(entity_b).ok();

        let ball_kicker = if let (Some(ball), Some((_, kicker, _))) = (ball_a, kicker_b) {
            Some(((entity_a, ball), (entity_b, kicker)))
        } else if let (Some(ball), Some((_, kicker, _))) = (ball_b, kicker_a) {
            Some(((entity_b, ball), (entity_a, kicker)))
        } else {
            None
        };

        let Some(((ball_entity, ball), (drain_kicker_entity, drain_kicker))) = ball_kicker else {
            continue;
        };

        info!(
            "Ball {} - kicker {} collision detected",
            ball.id, drain_kicker.name
        );
        if drain_kicker.name != "Drain" {
            continue;
        }

        info!("Ball {} drained!", ball.id);
        // play a drain sound at the kicker location
        if let Some(name) = pick(&sounds.drain) {
            let handle = load_sound(&table_assets, &assets_vpx, name);
            commands
                .entity(drain_kicker_entity)
                .with_child(spatial_sound_effect(handle));
        }

        commands.entity(ball_entity).despawn();

        // find the kicker named "BallRelease" to spawn a new ball there
        let (eject_kicker_entity, _, kicker_transform) = kicker_query
            .iter()
            .find(|(_, k, _)| k.name == "BallRelease")
            .expect("BallRelease kicker not found");

        if let Some(name) = pick(&sounds.release) {
            let handle = load_sound(&table_assets, &assets_vpx, name);
            commands
                .entity(eject_kicker_entity)
                .with_child(spatial_sound_effect(handle));
        }

        // TODO we want to delay the kick
        // TODO get rid off all these dependencies to spawn a new ball
        commands.spawn(spawn_ball(
            0,
            &table_assets,
            &mut meshes,
            &mut materials,
            &assets_vpx,
            Vec2 {
                x: kicker_transform.translation.x,
                y: kicker_transform.translation.y,
            },
        ));
    }
}

/// Pick a random sound name from the list, or `None` when the list is empty.
fn pick(names: &[String]) -> Option<&str> {
    if names.is_empty() {
        return None;
    }
    let index = rand::rng().random_range(0..names.len());
    Some(&names[index])
}

/// Load a named sound from the loaded table asset.
fn load_sound(
    table_assets: &Res<TableAssets>,
    assets_vpx: &Res<Assets<VpxAsset>>,
    name: &str,
) -> Handle<AudioSource> {
    assets_vpx
        .get(&table_assets.vpx)
        .unwrap()
        .named_sounds
        .get(name)
        .unwrap_or_else(|| panic!("Sound {name} not found"))
        .clone()
}
