//! Demo gameplay. All of these modules are only intended for demonstration
//! purposes and should be replaced with your own game logic.
//! Feel free to change the logic found here if you feel like tinkering around
//! to get a feeling for the template.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

pub(crate) mod ball;
mod ballcontrol;
pub(crate) mod bumper;
pub(crate) mod desktop;
mod flasher;
pub(crate) mod flipper;
pub(crate) mod gate;
pub(crate) mod kicker;
mod layer;
pub mod level;
pub(crate) mod light;
pub(crate) mod lightmap;
pub(crate) mod nudge;
mod physics;
pub mod playfield;
pub(crate) mod plunger;
mod primitive;
mod ramp;
pub(crate) mod reel;
mod rubber;
pub(crate) mod spinner;
pub mod table;
pub(crate) mod targets;
mod trigger;
pub(crate) mod wall;

#[derive(Resource)]
pub struct TablePath {
    pub path: PathBuf,
}
impl TablePath {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    // The table is chosen at runtime (table picker or command line); see `screens`.
    app.add_plugins((
        level::plugin,
        table::plugin,
        ball::plugin,
        ballcontrol::plugin,
        bumper::plugin,
        light::plugin,
        lightmap::plugin,
        plunger::plugin,
        nudge::plugin,
        flipper::plugin,
        kicker::plugin,
        wall::plugin,
        targets::plugin,
        spinner::plugin,
    ));
    app.add_plugins(gate::plugin);
    app.add_plugins(ramp::plugin);
    app.add_plugins(primitive::plugin);
    app.add_plugins(reel::plugin);
}
