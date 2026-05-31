//! Visual Pinball example table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`], [`SlingshotSounds`]).

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::wall::{SlingshotSounds, Wall};
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(DrainSounds {
        drain: vec!["drain".to_string()],
        release: vec!["ballrelease".to_string()],
    });
    app.insert_resource(FlipperSounds {
        up: vec!["fx_Flipperup".to_string()],
        down: vec!["fx_Flipperdown".to_string()],
    });
    app.insert_resource(BumperSounds {
        hit: vec!["fx_bumper4".to_string()],
    });
    app.insert_resource(SlingshotSounds {
        hit: vec!["left_slingshot".to_string(), "right_slingshot".to_string()],
    });
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        remove_plunger_wall,
    );
}

// TODO temporary hack: this wall keeps the ball in the lane and lets the plunger pass
//   through. We can't model that one-way behaviour yet, so we remove it for now. See:
//   https://github.com/avianphysics/avian/blob/main/crates/avian2d/examples/one_way_platform_2d.rs
//   The best option would be replacing the single wall with a left and right part that
//   leaves a gap for the plunger in the center.
fn remove_plunger_wall(mut commands: Commands, wall_query: Query<(Entity, &Wall)>) {
    let name = "Wall15";
    if let Some((plunger_wall_entity, _wall)) = wall_query.iter().find(|(_, k)| k.name == name) {
        commands.entity(plunger_wall_entity).despawn();
    } else {
        warn!("Plunger centering wall {name} not found, could not remove it");
    }
}
