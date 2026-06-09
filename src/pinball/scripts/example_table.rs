//! Visual Pinball example table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`], [`SlingshotSounds`], [`TargetSounds`],
//! [`SpinnerSounds`], [`GateSounds`]).

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::gate::GateSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::spinner::SpinnerSounds;
use crate::pinball::targets::TargetSounds;
use crate::pinball::wall::{SlingshotAnimation, SlingshotAnimations, SlingshotSounds};
use bevy::prelude::*;

pub(super) const TABLE: &str = "exampleTable.vpx";

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        setup.run_if(super::is_table(TABLE)),
    );
}

/// Insert the table's sound/animation config when this table enters gameplay.
fn setup(mut commands: Commands) {
    commands.insert_resource(DrainSounds {
        drain: vec!["drain".to_string()],
        release: vec!["ballrelease".to_string()],
    });
    commands.insert_resource(FlipperSounds {
        up: vec!["fx_Flipperup".to_string()],
        down: vec!["fx_Flipperdown".to_string()],
    });
    commands.insert_resource(BumperSounds {
        hit: vec!["fx_bumper4".to_string()],
    });
    commands.insert_resource(SlingshotSounds {
        hit: vec!["left_slingshot".to_string(), "right_slingshot".to_string()],
    });
    // Slingshot rubbers: the rest band plus a flexed frame shown briefly on a hit.
    commands.insert_resource(SlingshotAnimations(vec![
        SlingshotAnimation {
            slingshot: "LeftSlingShot".to_string(),
            rest: "LSling".to_string(),
            flexed: "LSling1".to_string(),
        },
        SlingshotAnimation {
            slingshot: "RightSlingShot".to_string(),
            rest: "RSling".to_string(),
            flexed: "RSling1".to_string(),
        },
    ]));
    commands.insert_resource(TargetSounds {
        hit: vec!["target".to_string()],
    });
    commands.insert_resource(SpinnerSounds {
        spin: vec!["fx_spinner".to_string()],
    });
    commands.insert_resource(GateSounds {
        hit: vec!["gate".to_string()],
    });
}
