//! Total Nuclear Annihilation table, re-implemented in Rust.
//!
//! Drain/release behaviour is generic and configured declaratively via [`DrainSounds`];
//! a random sound is picked from each list, matching the original script.

use crate::pinball::kicker::DrainSounds;
use crate::pinball::wall::Wall;
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(DrainSounds {
        drain: (1..=6)
            .map(|i| format!("SY_TNA_REV02_Trough_Drain_{i}"))
            .collect(),
        release: (1..=3)
            .map(|i| format!("SY_TNA_REV02_Shooter_Lane_Metal_BallDrop_{i}"))
            .collect(),
    });
    // TODO there's also a ramp that brings the ball over the loop side rail which we need
    //   to somehow ignore collisions with until the ball is fully launched.
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        remove_plunger_wall,
    );
}

// TODO temporary hack, see example_table.rs.
fn remove_plunger_wall(mut commands: Commands, wall_query: Query<(Entity, &Wall)>) {
    let name = "Wall348";
    if let Some((plunger_wall_entity, _wall)) = wall_query.iter().find(|(_, k)| k.name == name) {
        commands.entity(plunger_wall_entity).despawn();
    } else {
        warn!("Plunger centering wall {name} not found, could not remove it");
    }
}
