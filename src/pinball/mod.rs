//! Demo gameplay. All of these modules are only intended for demonstration
//! purposes and should be replaced with your own game logic.
//! Feel free to change the logic found here if you feel like tinkering around
//! to get a feeling for the template.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

pub(crate) mod ball;
mod ballcontrol;
mod bumper;
pub(crate) mod flipper;
pub(crate) mod gate;
mod kicker;
pub mod level;
mod light;
mod nudge;
pub mod playfield;
pub(crate) mod plunger;
mod rubber;
mod scripts;
mod spinner;
pub mod table;
mod targets;
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
    let file_name = "exampleTable.vpx";
    //let file_name = "North Pole (Playmatic 1967) v600.vpx";
    //let file_name = "Total Nuclear Annihilation (Spooky 2017) VPW v2.3.vpx";
    app.insert_resource(TablePath::new(file_name)).add_plugins((
        level::plugin,
        table::plugin,
        ball::plugin,
        ballcontrol::plugin,
        bumper::plugin,
        light::plugin,
        scripts::plugin,
        plunger::plugin,
        nudge::plugin,
        flipper::plugin,
        kicker::plugin,
        wall::plugin,
        targets::plugin,
        spinner::plugin,
        gate::plugin,
    ));
}
