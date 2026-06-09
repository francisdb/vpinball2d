//! Loads the selected table's assets, then enters gameplay.

use crate::pinball::TablePath;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::theme::widget;
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(Screen::Loading),
        (start_loading_table, spawn_loading_screen),
    );
    app.add_systems(Update, enter_gameplay.run_if(in_state(Screen::Loading)));
}

/// Kick off loading the selected table's vpx (and its image/sound dependencies).
fn start_loading_table(
    mut commands: Commands,
    table_path: Res<TablePath>,
    assets: Res<AssetServer>,
) {
    let file_name = table_path.path.to_string_lossy().to_string();
    commands.insert_resource(TableAssets {
        // Load through the `tables` asset source rooted at the tables folder.
        vpx: assets.load(format!("{}://{}", crate::tables::TABLES_SOURCE, file_name)),
        file_name,
    });
}

fn spawn_loading_screen(mut commands: Commands) {
    commands.spawn((
        widget::ui_root("Loading"),
        DespawnOnExit(Screen::Loading),
        children![widget::label("Loading...")],
    ));
}

/// Enter gameplay once the table and all its dependencies are loaded.
fn enter_gameplay(
    table_assets: Option<Res<TableAssets>>,
    assets: Res<AssetServer>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if let Some(table_assets) = table_assets
        && assets.is_loaded_with_dependencies(&table_assets.vpx)
    {
        next_screen.set(Screen::Gameplay);
    }
}
