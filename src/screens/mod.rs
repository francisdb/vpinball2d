//! The game's screens: pick a table, then play it.

mod gameplay;
mod loading;
mod table_select;

use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.init_state::<Screen>();
    app.init_resource::<ExternalFrontend>();
    app.add_plugins((table_select::plugin, loading::plugin, gameplay::plugin));
}

/// The game's top-level screens.
#[derive(States, Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum Screen {
    /// Pick a table to play (Bevy UI list of the tables in the assets folder).
    #[default]
    TableSelect,
    /// Load the selected table's assets, then enter gameplay.
    Loading,
    /// Play the loaded table.
    Gameplay,
}

/// Set when the game is launched with a table on the command line (an external
/// frontend driving us): there is no table picker, so leaving gameplay exits the
/// game instead of returning to selection.
#[derive(Resource, Default)]
pub struct ExternalFrontend(pub bool);
