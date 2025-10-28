//! Demo gameplay. All of these modules are only intended for demonstration
//! purposes and should be replaced with your own game logic.
//! Feel free to change the logic found here if you feel like tinkering around
//! to get a feeling for the template.

use bevy::prelude::*;

mod ball;
mod ballcontrol;
mod bumper;
mod kicker;
pub mod level;
mod light;
mod scripts;
pub mod table;
mod trigger;
mod wall;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        level::plugin,
        table::plugin,
        ball::plugin,
        ballcontrol::plugin,
        bumper::plugin,
        scripts::plugin,
    ));
}
