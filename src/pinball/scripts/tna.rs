//! Total Nuclear Annihilation table, re-implemented in Rust.
//!
//! Behaviour is generic and configured declaratively via the sound resources
//! ([`DrainSounds`], [`FlipperSounds`], [`BumperSounds`], [`SlingshotSounds`]); a random
//! sound is picked from each list, matching the original script.

use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::wall::{SlingshotAnimation, SlingshotAnimations, SlingshotSounds};
use bevy::prelude::*;

pub(super) const TABLE: &str = "Total Nuclear Annihilation (Spooky 2017) VPW v2.3.vpx";

pub(super) fn plugin(app: &mut App) {
    // TODO there's also a ramp that brings the ball over the loop side rail which we need
    //   to somehow ignore collisions with until the ball is fully launched.
    app.add_systems(
        OnEnter(crate::screens::Screen::Gameplay),
        setup.run_if(super::is_table(TABLE)),
    );
}

fn setup(mut commands: Commands) {
    commands.insert_resource(DrainSounds {
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
    commands.insert_resource(FlipperSounds { up, down });
    commands.insert_resource(BumperSounds {
        hit: (1..=7)
            .map(|i| format!("SY_TNA_REV03_Pop_Bumper_{i}"))
            .collect(),
    });
    // Both slingshots' main sounds combined (a random one is picked per hit).
    let mut sling: Vec<String> = (1..=7)
        .map(|i| format!("SY_TNA_REV03_Slingshot_Main_Left_{i}"))
        .collect();
    sling.extend((1..=7).map(|i| format!("SY_TNA_REV03_Slingshot_Main_Right_{i}")));
    commands.insert_resource(SlingshotSounds { hit: sling });
    // TNA has five slingshots (two lower, three around the upper reactor), each flexing
    // through rest band `*1` -> flex frames `*2..4` plus a rotating arm primitive in the
    // table script. Our two-state model briefly shows the most-flexed frame (`*4`) on a hit.
    commands.insert_resource(SlingshotAnimations(vec![
        SlingshotAnimation {
            slingshot: "Leftslingshot".to_string(),
            rest: "LeftSling1".to_string(),
            flexed: "LeftSling4".to_string(),
        },
        SlingshotAnimation {
            slingshot: "Rightslingshot".to_string(),
            rest: "RightSling1".to_string(),
            flexed: "RightSling4".to_string(),
        },
        SlingshotAnimation {
            slingshot: "SlingShot1".to_string(),
            rest: "LeftUpSling1".to_string(),
            flexed: "LeftUpSling4".to_string(),
        },
        SlingshotAnimation {
            slingshot: "SlingShot2".to_string(),
            rest: "RightUpSling1".to_string(),
            flexed: "RightUpSling4".to_string(),
        },
        SlingshotAnimation {
            slingshot: "SlingShot3".to_string(),
            rest: "LeftLeftSling1".to_string(),
            flexed: "LeftLeftSling4".to_string(),
        },
    ]));
}
