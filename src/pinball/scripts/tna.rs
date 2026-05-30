//! Total Nuclear Annihilation table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`]); a random sound is picked from
//! each list, matching the original script.

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
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
    // The lower flippers' up/down sounds, both sides combined (a random one is picked).
    let mut up: Vec<String> = (1..=6)
        .map(|i| format!("SY_TNA_REV02_Flipper_Lower_Left_Up_Full_Stroke_{i}"))
        .collect();
    up.extend((1..=5).map(|i| format!("SY_TNA_REV02_Flipper_Lower_Right_Up_Full_Stroke_{i}")));
    let mut down: Vec<String> = (1..=7)
        .map(|i| format!("SY_TNA_REV02_Flipper_Lower_Left_Down_{i}"))
        .collect();
    down.extend((1..=7).map(|i| format!("SY_TNA_REV02_Flipper_Lower_Right_Down_{i}")));
    app.insert_resource(FlipperSounds { up, down });
    app.insert_resource(BumperSounds {
        hit: (1..=7)
            .map(|i| format!("SY_TNA_REV03_Pop_Bumper_{i}"))
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
