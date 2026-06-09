//! North Pole table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`], [`SlingshotSounds`]).

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::wall::SlingshotSounds;
use bevy::prelude::*;

pub(super) const TABLE: &str = "North Pole (Playmatic 1967) v600.vpx";

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        setup.run_if(super::is_table(TABLE)),
    );
}

fn setup(mut commands: Commands) {
    commands.insert_resource(DrainSounds {
        drain: vec!["fx_drain".to_string()],
        // the script seems to use "fx_Ballrel" which indicates that sound loading is case-insensitive?
        release: vec!["fx_ballrel".to_string()],
    });
    commands.insert_resource(FlipperSounds {
        up: vec!["fx_flipperup".to_string()],
        down: vec!["fx_flipperdown".to_string()],
    });
    commands.insert_resource(BumperSounds {
        hit: vec!["fx_Bumper".to_string()],
    });
    commands.insert_resource(SlingshotSounds {
        hit: vec!["fx_slingshot".to_string()],
    });
}
