//! North Pole table, re-implemented in Rust.
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
        drain: vec!["fx_drain".to_string()],
        // the script seems to use "fx_Ballrel" which indicates that sound loading is case-insensitive?
        release: vec!["fx_ballrel".to_string()],
    });
    app.insert_resource(FlipperSounds {
        up: vec!["fx_flipperup".to_string()],
        down: vec!["fx_flipperdown".to_string()],
    });
    app.insert_resource(BumperSounds {
        hit: vec!["fx_Bumper".to_string()],
    });
    app.insert_resource(SlingshotSounds {
        hit: vec!["fx_slingshot".to_string()],
    });
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        remove_plunger_wall,
    );
}

// TODO temporary hack, see example_table.rs.
fn remove_plunger_wall(mut commands: Commands, wall_query: Query<(Entity, &Wall)>) {
    let name = "Wall6";
    if let Some((plunger_wall_entity, _wall)) = wall_query.iter().find(|(_, k)| k.name == name) {
        commands.entity(plunger_wall_entity).despawn();
    } else {
        warn!("Plunger centering wall {name} not found, could not remove it");
    }
}
