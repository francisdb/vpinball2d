//! Demo gameplay. All of these modules are only intended for demonstration
//! purposes and should be replaced with your own game logic.
//! Feel free to change the logic found here if you feel like tinkering around
//! to get a feeling for the template.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

mod ball;
mod ballcontrol;
mod bumper;
mod flipper;
mod kicker;
pub mod level;
mod light;
mod plunger;
mod rubber;
mod scripts;
pub mod table;
mod trigger;
mod wall;

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
        scripts::plugin,
        plunger::plugin,
        flipper::plugin,
    ));
}
