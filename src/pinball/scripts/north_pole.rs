//! North Pole table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`], [`SlingshotSounds`]).

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::wall::{SlingshotAnimation, SlingshotAnimations, SlingshotSounds};
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
    // The slingshots flex through rest band `*Sling1` -> flex frames `*Sling2..4` (the
    // table script cycles a 4-frame animation plus a rotating arm primitive `Lemk`/`Remk`).
    // Our two-state model briefly shows the most-flexed frame (`*Sling4`) on a hit.
    commands.insert_resource(SlingshotAnimations(vec![
        SlingshotAnimation {
            slingshot: "LeftSlingshot".to_string(),
            rest: "LeftSling1".to_string(),
            flexed: "LeftSling4".to_string(),
        },
        SlingshotAnimation {
            slingshot: "RightSlingshot".to_string(),
            rest: "RightSling1".to_string(),
            flexed: "RightSling4".to_string(),
        },
    ]));
}
