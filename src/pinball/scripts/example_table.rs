//! Visual Pinball example table script re-implemented in Rust.

use crate::audio::spatial_sound_effect;
use crate::pinball;
use crate::pinball::ball::Ball;
use crate::pinball::scripts::load_sound;
use crate::pinball::wall::Wall;
use avian2d::prelude::CollisionStart;
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        example_table_script.run_if(in_state(crate::screens::Screen::Gameplay)),
    );
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        remove_plunger_wall,
    );
}

fn remove_plunger_wall(mut commands: Commands, wall_query: Query<(Entity, &Wall)>) {
    // TODO on the example table wall 15 is a wall that keeps the
    //   ball in in the lane and allows the plunger to pass through
    //   However we don't know how to allow that behavior yet so we skip it for now
    //   https://github.com/avianphysics/avian/blob/main/crates/avian2d/examples/one_way_platform_2d.rs
    //   Maybe they should be on different collision layers?
    //   The best option would be replacing the single wall with a left and right part
    //   that leaves a gap for the plunger in the center.
    let name = "Wall15";
    if let Some((plunger_wall_entity, _wall)) = wall_query.iter().find(|(_, k)| k.name == name) {
        commands.entity(plunger_wall_entity).despawn();
    } else {
        warn!(
            "Plunger centering wall {} not found, could not remove it",
            name
        );
    }
}

// TODO implement ball spawn and despawn logic that matches the original script

fn example_table_script(
    mut collision_reader: MessageReader<CollisionStart>,
    ball_query: Query<&Ball>,
    kicker_query: Query<(Entity, &pinball::kicker::Kicker, &Transform)>,
    mut commands: Commands,
    table_assets: Res<pinball::table::TableAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    assets_vpx: Res<Assets<crate::vpx::VpxAsset>>,
) {
    // Placeholder for the example table script logic
    // This function would contain the game logic that was originally implemented in VBScript
    // For example, handling scoring, ball launches, and other gameplay mechanics

    // TODO observers might be better?

    // for each collision, check if it's a ball with a trigger
    for collision in collision_reader.read() {
        let entity_a = collision.body1.unwrap();
        let entity_b = collision.body2.unwrap();

        let ball_a = ball_query.get(entity_a).ok();
        let ball_b = ball_query.get(entity_b).ok();

        let trigger_a = kicker_query.get(entity_a).ok();
        let trigger_b = kicker_query.get(entity_b).ok();

        let ball_kicker = if let (Some(ball), Some((_, kicker, _))) = (ball_a, trigger_b) {
            Some(((entity_a, ball), (entity_b, kicker)))
        } else if let (Some(ball), Some((_, kicker, _))) = (ball_b, trigger_a) {
            Some(((entity_b, ball), (entity_a, kicker)))
        } else {
            None
        };

        if let Some(((ball_entity, ball), (drain_kicker_entity, drain_kicker))) = ball_kicker {
            info!(
                "Ball {} - kicker {} collision detected",
                ball.id, drain_kicker.name
            );
            if drain_kicker.name == "Drain" {
                info!("Ball {} drained!", ball.id);
                // play "drain" sound at the kicker location
                let drain_sound_handle = load_sound(&table_assets, &assets_vpx, "drain");
                commands
                    .entity(drain_kicker_entity)
                    .with_child(spatial_sound_effect(drain_sound_handle));

                commands.entity(ball_entity).despawn();

                // find the kicker named "BallRelease" to spawn a new ball there
                let (eject_kicker_entity, _, kicker_transform) = kicker_query
                    .iter()
                    .find(|(_, k, _)| k.name == "BallRelease")
                    .expect("BallRelease kicker not found");

                let release_sound_handle = load_sound(&table_assets, &assets_vpx, "ballrelease");
                commands
                    .entity(eject_kicker_entity)
                    .with_child(spatial_sound_effect(release_sound_handle));

                // TODO we want to delay the kick
                // TODO get rid off all these dependencies to spawn a new ball
                commands.spawn(pinball::ball::ball(
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
    }
}
